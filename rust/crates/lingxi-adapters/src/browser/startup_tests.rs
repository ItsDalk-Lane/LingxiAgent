//! 启动失败必须经过真实Chromium进程和管道，未知退出不能靠丢句柄冒充已清理。
use super::*;
use crate::models::network::{NetworkPolicy, ProxyPolicy};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread::JoinHandle,
};

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) host: Arc<ChromiumHost>,
    pub(super) control: Arc<super::super::launcher::StartupControl>,
    stop: Arc<AtomicBool>,
    proxy: Option<JoinHandle<()>>,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let executable = std::env::var_os("LINGXI_TEST_CHROMIUM")
            .map(PathBuf::from)
            .or_else(|| {
                [
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                    "/Applications/Chromium.app/Contents/MacOS/Chromium",
                    "/usr/bin/chromium",
                    "/usr/bin/chromium-browser",
                    "/usr/bin/google-chrome",
                ]
                .iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
            })
            .expect("本组启动故障检查需要真实Chromium");
        let mut random = [0u8; 16];
        getrandom::getrandom(&mut random).unwrap();
        let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let root = std::env::temp_dir().join(format!("lingxi-browser-startup-{name}"));
        std::fs::create_dir(&root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let proxy = std::thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                        let mut bytes = [0u8; 1024];
                        let _ = stream.read(&mut bytes);
                        let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        let host = ChromiumHost::new(ChromiumConfig {
            executable,
            root: root.clone(),
            execution_node: "startup-fixture-node".into(),
            headless: true,
            network: NetworkPlane::isolated(NetworkPolicy {
                proxy: ProxyPolicy::Manual {
                    http: Some(address.clone()),
                    https: Some(address),
                    no_proxy: String::new(),
                },
                trusted_ca_pem: None,
            }),
        })
        .unwrap();
        let control = Arc::new(super::super::launcher::StartupControl::default());
        *host.core.startup_control.lock().unwrap() = Some(control.clone());
        Self {
            root,
            host,
            control,
            stop,
            proxy: Some(proxy),
        }
    }
    pub(super) fn scope(&self, id: &str) -> BrowserScope {
        BrowserScope {
            session_id: id.into(),
            owner_subject: "user_local".into(),
            epoch: 1,
            execution_node: "startup-fixture-node".into(),
            human: false,
        }
    }
    pub(super) async fn start(&self, id: &str) -> Result<BrowserOutput, BrowserError> {
        self.host
            .execute(
                self.scope(id),
                BrowserCommand::new(BrowserAction::Start),
                Arc::new(AtomicBool::new(false)),
            )
            .await
    }
    fn start_task(&self, id: &str) -> tokio::task::JoinHandle<Result<BrowserOutput, BrowserError>> {
        let host = self.host.clone();
        let scope = self.scope(id);
        tokio::spawn(async move {
            host.execute(
                scope,
                BrowserCommand::new(BrowserAction::Start),
                Arc::new(AtomicBool::new(false)),
            )
            .await
        })
    }
    pub(super) async fn wait_spawn_paused(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.control.spawn_reached.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "真实spawn未在预算内到达受控暂停点"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(self.control.spawned_pids.lock().unwrap().len(), 1);
    }
    pub(super) fn assert_reaped(&self) {
        let mut started = self.control.spawned_pids.lock().unwrap().clone();
        started.sort_unstable();
        let mut reaped = self.control.reaped_pids.lock().unwrap().clone();
        reaped.sort_unstable();
        assert_eq!(started, reaped, "必须由真实Child.try_wait确认回收");
    }
    fn owned(&self, id: &str) -> Arc<Live> {
        let pool = self.host.core.pool.lock().unwrap();
        let live = pool.entries[id]
            .live
            .as_ref()
            .expect("未知清理仍须有正式会话owner")
            .clone();
        assert!(pool
            .pending_cleanup
            .iter()
            .any(|candidate| Arc::ptr_eq(candidate, &live)));
        assert!(pool.stopping, "未知清理须关闭新执行入口");
        assert_eq!(
            pool.entries[id].state.unavailable_reason.as_deref(),
            Some("browser_cleanup_unknown")
        );
        live
    }
    pub(super) fn alive(&self, live: &Live) {
        let pids = self.control.spawned_pids.lock().unwrap();
        assert_eq!(pids.len(), 1, "必须实际spawn且只spawn一次");
        assert!(!live.process.is_reaped());
        assert_eq!(
            unsafe { libc::kill(pids[0] as i32, 0) },
            0,
            "注入未知时真实自有进程仍存在"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.control.release_spawn.store(true, Ordering::Release);
        // 断言失败也只清理本fixture的自有进程；退出未确认就保留资料目录。
        if self.host.core.close_all(true).is_ok() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
        self.stop.store(true, Ordering::Release);
        if let Some(proxy) = self.proxy.take() {
            let _ = proxy.join();
        }
    }
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn cdp_preparation_failure_happens_before_any_real_spawn() {
    let fixture = Fixture::new();
    fixture
        .control
        .fail_cdp_prepare
        .store(true, Ordering::Release);
    let cause = fixture.start("cdp-preparation").await.unwrap_err();
    assert_eq!(cause.code, "browser_startup_test_cdp");
    assert!(fixture.control.spawned_pids.lock().unwrap().is_empty());
    let pool = fixture.host.core.pool.lock().unwrap();
    assert_eq!(pool.launching, 0);
    assert!(pool.pending_cleanup.is_empty());
    assert!(pool.entries["cdp-preparation"].live.is_none());
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn live_initialization_failure_keeps_real_owner_until_cleanup_retry_confirms() {
    let fixture = Fixture::new();
    fixture
        .control
        .fail_live_initialize
        .store(true, Ordering::Release);
    fixture.control.unknown_stops.store(1, Ordering::Release);
    let cause = fixture.start("live-initialize").await.unwrap_err();
    assert_eq!(cause.code, "browser_cleanup_unknown");
    assert!(cause.effect_unknown);
    let live = fixture.owned("live-initialize");
    fixture.alive(&live);
    let profile = fixture
        .root
        .join("profiles")
        .join(key(&fixture.scope("live-initialize")));
    assert!(profile.is_dir(), "实际进程未退出不能清除profile");
    assert_eq!(
        fixture.start("must-not-spawn").await.unwrap_err().code,
        "browser_closed"
    );
    assert_eq!(fixture.control.spawned_pids.lock().unwrap().len(), 1);
    fixture.host.shutdown().await.unwrap();
    assert!(live.process.is_reaped());
    assert!(fixture
        .host
        .core
        .pool
        .lock()
        .unwrap()
        .pending_cleanup
        .is_empty());
    assert!(
        fixture.host.core.pool.lock().unwrap().entries["live-initialize"]
            .live
            .is_none()
    );
    assert!(
        profile.is_dir(),
        "普通工作区资料在确认退出后仍按持久登录契约保留"
    );
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn first_target_failure_keeps_actual_process_and_target_until_cleanup_retry() {
    let fixture = Fixture::new();
    fixture
        .control
        .fail_first_target
        .store(true, Ordering::Release);
    fixture.control.unknown_stops.store(1, Ordering::Release);
    let cause = fixture.start("first-target").await.unwrap_err();
    assert_eq!(cause.code, "browser_cleanup_unknown");
    let live = fixture.owned("first-target");
    fixture.alive(&live);
    let targets = fixture.control.created_targets.lock().unwrap().clone();
    assert_eq!(targets.len(), 1, "故障必须在真实createTarget已受理之后触发");
    assert!(!targets[0].is_empty());
    assert_eq!(
        fixture
            .start("no-second-target-process")
            .await
            .unwrap_err()
            .code,
        "browser_closed"
    );
    fixture.host.shutdown().await.unwrap();
    assert!(live.process.is_reaped());
    assert!(fixture
        .host
        .core
        .pool
        .lock()
        .unwrap()
        .pending_cleanup
        .is_empty());
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn auxiliary_initialization_unknown_preserves_profile_and_owner_until_retry() {
    let fixture = Fixture::new();
    fixture
        .control
        .fail_live_initialize
        .store(true, Ordering::Release);
    fixture.control.unknown_stops.store(1, Ordering::Release);
    let cause = fixture
        .host
        .read_auxiliary_page(
            "https://must-not-load.invalid/".into(),
            "document.title".into(),
            None,
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap_err();
    assert_eq!(cause.code, "browser_cleanup_unknown");
    let live = {
        let pool = fixture.host.core.pool.lock().unwrap();
        assert_eq!(pool.aux, 0);
        assert!(pool.stopping);
        assert_eq!(pool.pending_cleanup.len(), 1);
        assert_eq!(
            pool.auxiliary.len(),
            1,
            "任务退出后未知资源的owner仍须可枚举"
        );
        pool.pending_cleanup[0].clone()
    };
    fixture.alive(&live);
    let path = live.download_dir.parent().unwrap().to_path_buf();
    assert!(path.is_dir(), "guard析构不得清除仍在使用的辅助profile");
    assert_eq!(
        std::fs::read_dir(fixture.root.join("aux")).unwrap().count(),
        1
    );
    fixture.host.shutdown().await.unwrap();
    assert!(live.process.is_reaped());
    assert!(!path.exists(), "只有真实退出确认后才清除该owned辅助profile");
    assert!(fixture
        .host
        .core
        .pool
        .lock()
        .unwrap()
        .pending_cleanup
        .is_empty());
    assert!(fixture.host.core.pool.lock().unwrap().auxiliary.is_empty());
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn revoke_waits_for_real_spawn_owner_registration_and_confirmed_reap() {
    let fixture = Fixture::new();
    let unrelated = fixture.scope("unrelated-cold-workspace");
    fixture
        .host
        .restore(unrelated.clone(), empty(&unrelated, true))
        .unwrap();
    fixture
        .control
        .pause_after_spawn
        .store(true, Ordering::Release);
    let start = fixture.start_task("revoke-starting");
    fixture.wait_spawn_paused().await;
    tokio::time::timeout(Duration::from_secs(1), fixture.host.forget(unrelated))
        .await
        .expect("删除无busy的本工作区不能等待其他工作区的真实spawn")
        .unwrap();
    {
        let pool = fixture.host.core.pool.lock().unwrap();
        assert!(pool.entries["revoke-starting"].busy.load(Ordering::Acquire));
        assert!(
            pool.entries["revoke-starting"].live.is_none(),
            "暂停点必须在owner登记前"
        );
    }
    let mut scope = fixture.scope("revoke-starting");
    scope.epoch = 2;
    let host = fixture.host.clone();
    let revoke = tokio::spawn(async move { host.revoke(scope).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !revoke.is_finished(),
        "真实进程仍暂停启动时急停不得提前返回成功"
    );
    assert!(fixture.control.reaped_pids.lock().unwrap().is_empty());
    fixture.control.release_spawn.store(true, Ordering::Release);
    assert!(start.await.unwrap().is_err());
    tokio::time::timeout(Duration::from_secs(6), revoke)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    fixture.assert_reaped();
    let pool = fixture.host.core.pool.lock().unwrap();
    let entry = &pool.entries["revoke-starting"];
    assert!(entry.state.revoked);
    assert!(entry.live.is_none());
    assert!(!entry.busy.load(Ordering::Acquire));
    assert!(!entry.state.running);
    assert!(entry.state.tabs.is_empty());
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn revoke_startup_timeout_preserves_late_unknown_live_until_same_epoch_retry() {
    let fixture = Fixture::new();
    fixture
        .control
        .pause_after_spawn
        .store(true, Ordering::Release);
    fixture.control.unknown_stops.store(1, Ordering::Release);
    let start = fixture.start_task("revoke-starting-unknown");
    fixture.wait_spawn_paused().await;
    let mut scope = fixture.scope("revoke-starting-unknown");
    scope.epoch = 2;
    let cause = tokio::time::timeout(Duration::from_secs(6), fixture.host.revoke(scope.clone()))
        .await
        .expect("急停等待必须有上限")
        .unwrap_err();
    assert_eq!(cause.code, "browser_cleanup_unknown");
    assert!(cause.effect_unknown);
    assert!(fixture.control.reaped_pids.lock().unwrap().is_empty());
    {
        let pool = fixture.host.core.pool.lock().unwrap();
        assert!(pool.stopping);
        assert!(pool.entries["revoke-starting-unknown"].live.is_none());
        assert!(pool.entries["revoke-starting-unknown"]
            .busy
            .load(Ordering::Acquire));
    }
    fixture.control.release_spawn.store(true, Ordering::Release);
    assert_eq!(
        start.await.unwrap().unwrap_err().code,
        "browser_cleanup_unknown"
    );
    let live = fixture.owned("revoke-starting-unknown");
    fixture.alive(&live);
    {
        let pool = fixture.host.core.pool.lock().unwrap();
        assert_eq!(pool.entries["revoke-starting-unknown"].epoch, 2);
        assert!(pool.entries["revoke-starting-unknown"].state.revoked);
    }
    fixture.host.revoke(scope).await.unwrap();
    fixture.assert_reaped();
    assert!(live.process.is_reaped());
    assert!(
        fixture.host.core.pool.lock().unwrap().entries["revoke-starting-unknown"]
            .live
            .is_none()
    );
    assert!(fixture
        .host
        .core
        .pool
        .lock()
        .unwrap()
        .pending_cleanup
        .is_empty());
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn forget_rechecks_late_startup_unknown_before_purging_profile() {
    let fixture = Fixture::new();
    fixture
        .control
        .pause_after_spawn
        .store(true, Ordering::Release);
    fixture.control.unknown_stops.store(1, Ordering::Release);
    let start = fixture.start_task("forget-starting-unknown");
    fixture.wait_spawn_paused().await;
    let mut scope = fixture.scope("forget-starting-unknown");
    scope.epoch = 2;
    let profile = fixture.root.join("profiles").join(key(&scope));
    let downloads = fixture.root.join("downloads").join(key(&scope));
    let host = fixture.host.clone();
    let deleting = scope.clone();
    let forget = tokio::spawn(async move { host.forget(deleting).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!forget.is_finished());
    assert!(profile.is_dir());
    assert!(downloads.is_dir());
    fixture.control.release_spawn.store(true, Ordering::Release);
    assert_eq!(
        start.await.unwrap().unwrap_err().code,
        "browser_cleanup_unknown"
    );
    let cause = tokio::time::timeout(Duration::from_secs(6), forget)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(cause.code, "browser_cleanup_unknown");
    assert!(cause.effect_unknown);
    let live = fixture.owned("forget-starting-unknown");
    fixture.alive(&live);
    assert!(
        profile.is_dir(),
        "launching归零不能覆盖迟到Live的Unknown并清除profile"
    );
    assert!(downloads.is_dir());
    fixture.host.forget(scope).await.unwrap();
    fixture.assert_reaped();
    assert!(live.process.is_reaped());
    assert!(!profile.exists());
    assert!(!downloads.exists());
    assert!(!fixture
        .host
        .core
        .pool
        .lock()
        .unwrap()
        .entries
        .contains_key("forget-starting-unknown"));
    assert!(fixture
        .host
        .core
        .pool
        .lock()
        .unwrap()
        .pending_cleanup
        .is_empty());
}
