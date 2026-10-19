//! 只访问本机fixture：正式Chromium、管道、会话profile与文件字节。
#![cfg(unix)]
use lingxi_adapters::{
    browser::{ChromiumConfig, ChromiumHost},
    models::network::{NetworkPlane, NetworkPolicy, ProxyPolicy},
};
use lingxi_kernel::browser::{BrowserOutput, BrowserPort, BrowserScope};
use lingxi_protocol::{BrowserAction as A, BrowserCommand, BrowserSettings};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

fn chrome() -> PathBuf {
    if let Some(path) = std::env::var_os("LINGXI_TEST_CHROMIUM") {
        return PathBuf::from(path);
    }
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
    .expect("本机Chromium是本组真实集成测试的前置条件")
}
fn temp() -> PathBuf {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).unwrap();
    let suffix: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let p = std::env::temp_dir().join(format!("lingxi-browser-{suffix}"));
    std::fs::create_dir(&p).unwrap();
    p
}
fn scope(id: &str) -> BrowserScope {
    BrowserScope {
        session_id: id.into(),
        owner_subject: "user_local".into(),
        epoch: 1,
        execution_node: "fixture-server".into(),
        human: false,
    }
}
struct Site {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    requests: Arc<Mutex<Vec<String>>>,
}
impl Site {
    fn start() -> Self {
        Self::start_mode(false)
    }
    fn guard() -> Self {
        Self::start_mode(true)
    }
    fn start_mode(network_guard: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let audit = requests.clone();
        let worker = std::thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut io, _)) => {
                        io.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                        let mut request = Vec::new();
                        let mut bytes = [0u8; 2048];
                        while request.len() < 16384 {
                            match io.read(&mut bytes) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    request.extend_from_slice(&bytes[..n]);
                                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                            }
                        }
                        // Chrome可预开尚未发送请求的连接；不能对空闲/半个请求擅自返回默认页面。
                        if !request.windows(4).any(|w| w == b"\r\n\r\n") {
                            let mut audit = audit.lock().unwrap();
                            if audit.len() < 256 {
                                audit.push("idle/incomplete -> no response".into());
                            }
                            continue;
                        }
                        if network_guard {
                            let _=io.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                            continue;
                        }
                        let request = String::from_utf8_lossy(&request);
                        let path = request
                            .lines()
                            .next()
                            .and_then(|l| l.split_whitespace().nth(1))
                            .unwrap_or("/");
                        let mut trace = audit.lock().unwrap();
                        if trace.len() < 256 {
                            trace.push(format!(
                                "{} -> {}",
                                request
                                    .lines()
                                    .next()
                                    .unwrap_or("missing request")
                                    .chars()
                                    .take(160)
                                    .collect::<String>(),
                                if path.starts_with("/slow") {
                                    "delayed"
                                } else if path.starts_with("/echo") {
                                    "echo"
                                } else if path.starts_with("/download") {
                                    "download"
                                } else {
                                    "form"
                                }
                            ));
                        }
                        drop(trace);
                        let (body, extra, kind) = if path.starts_with("/download") {
                            (
                                b"downloaded real bytes\n".to_vec(),
                                "Content-Disposition: attachment; filename=proof.txt\r\n",
                                "application/octet-stream",
                            )
                        } else if path.starts_with("/slow") {
                            std::thread::sleep(Duration::from_millis(200));
                            (b"<!doctype html><title>Delayed browser fixture</title><body>delayed response</body>".to_vec(), "", "text/html; charset=utf-8")
                        } else if path.starts_with("/echo") {
                            let cookie = request
                                .lines()
                                .find(|l| l.to_ascii_lowercase().starts_with("cookie:"))
                                .filter(|line| {
                                    line.split_once(':')
                                        .is_some_and(|(_, value)| !value.trim().is_empty())
                                })
                                .unwrap_or("no-cookie");
                            let language = request
                                .lines()
                                .find(|l| l.to_ascii_lowercase().starts_with("accept-language:"))
                                .unwrap_or("no-language");
                            (
                                format!(
                                    "<!doctype html><body><pre>{cookie}\n{language}</pre></body>"
                                )
                                .into_bytes(),
                                "",
                                "text/html; charset=utf-8",
                            )
                        } else {
                            (br#"<!doctype html><title>Local browser fixture</title><body><label>Identity <input id='name' aria-label='Identity'></label><select id='choice' aria-label='Choice'><option value='a'>Alpha</option><option value='b'>Beta</option></select><button id='popup' onclick="window.open('/popup','_blank')">Popup</button><button id='dialog' onclick="alert('local dialog');document.body.dataset.dialog='done'">Dialog</button><a href='/download'>Download</a><p>fixture text</p></body>"#.to_vec(),if path.starts_with("/login"){"Set-Cookie: login=fixture; Path=/; Max-Age=3600\r\n"}else if path.starts_with("/blocked-cookie"){"Set-Cookie: blocked=must-not-persist; Path=/; Max-Age=3600\r\n"}else{""},"text/html; charset=utf-8")
                        };
                        let header=format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",body.len());
                        let _ = io.write_all(header.as_bytes());
                        let _ = io.write_all(&body);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            stop,
            worker: Some(worker),
            requests,
        }
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
fn host(root: &std::path::Path) -> (Arc<ChromiumHost>, Site) {
    // 非loopback流量只能到这个本机拒绝代理；不访问真实账号或公网。
    let guard = Site::guard();
    let network = NetworkPlane::isolated(NetworkPolicy {
        proxy: ProxyPolicy::Manual {
            http: Some(guard.url.clone()),
            https: Some(guard.url.clone()),
            no_proxy: "localhost,127.0.0.1,::1".into(),
        },
        trusted_ca_pem: None,
    });
    let host = ChromiumHost::new(ChromiumConfig {
        executable: chrome(),
        root: root.into(),
        execution_node: "fixture-server".into(),
        headless: true,
        network,
    })
    .unwrap();
    (host, guard)
}
async fn execute(h: &ChromiumHost, s: BrowserScope, c: BrowserCommand) -> BrowserOutput {
    execute_checked(h, s, c, None).await
}
async fn execute_checked(
    h: &ChromiumHost,
    s: BrowserScope,
    c: BrowserCommand,
    context: Option<(&std::path::Path, &Site, &str)>,
) -> BrowserOutput {
    let action = c.action;
    let tab = c.tab_id.clone();
    match h.execute(s, c, Arc::new(AtomicBool::new(false))).await {
        Ok(output) => output,
        Err(cause) => {
            // 只在失败时保存自有fixture现场，不输出命令参数、输入值或页面正文。
            let workspaces = h.workspaces();
            if let Some((root, site, phase)) = context {
                let requests = site.requests.lock().unwrap().clone();
                panic!(
                    "真实浏览器命令失败: action={action:?}, tab={tab:?}, phase={phase}, owned_root={}, controlled_requests={requests:?}, workspaces={workspaces:?}, cause={cause:?}",
                    root.display()
                );
            }
            panic!(
                "真实浏览器命令失败: action={action:?}, tab={tab:?}, workspaces={workspaces:?}, cause={cause:?}"
            );
        }
    }
}
async fn navigate(h: &ChromiumHost, s: &BrowserScope, url: String) -> BrowserOutput {
    navigate_checked(h, s, url, None).await
}
async fn navigate_checked(
    h: &ChromiumHost,
    s: &BrowserScope,
    url: String,
    context: Option<(&std::path::Path, &Site, &str)>,
) -> BrowserOutput {
    let mut c = BrowserCommand::new(A::Navigate);
    c.url = Some(url);
    execute_checked(h, s.clone(), c, context).await
}
async fn eval(
    h: &ChromiumHost,
    s: &BrowserScope,
    tab: &str,
    expression: &str,
) -> serde_json::Value {
    let mut c = BrowserCommand::new(A::Evaluate);
    c.tab_id = Some(tab.into());
    c.expression = Some(expression.into());
    execute(h, s.clone(), c).await.reply.result["value"].clone()
}

#[tokio::test]
async fn same_real_target_input_snapshot_generations_and_empty_workspace() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    let s = scope("input");
    let opened = navigate(&h, &s, format!("{}/form", site.url)).await;
    let tab = opened.reply.state.active_tab_id.unwrap();
    let text = opened.reply.result["text"].as_str().unwrap();
    let reference = regex::Regex::new(r"\[(\d+)\][^\n]*Identity")
        .unwrap()
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().parse::<u32>().unwrap())
        .expect("真实快照有输入框编号");
    let mut typed = BrowserCommand::new(A::Type);
    typed.tab_id = Some(tab.clone());
    typed.element_ref = Some(reference);
    typed.text = Some("中文 before takeover".into());
    execute(&h, s.clone(), typed.clone()).await;
    assert_eq!(
        eval(&h, &s, &tab, "document.querySelector('#name').value").await,
        "中文 before takeover"
    );
    assert_eq!(
        h.execute(s.clone(), typed, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap_err()
            .code,
        "browser_stale_snapshot",
        "新快照不得让旧ref撞到新元素"
    );
    let mut human = s.clone();
    human.human = true;
    assert_eq!(eval(&h,&human,&tab,"document.querySelector('#name').value='human newest';document.querySelector('#name').value").await,"human newest");
    assert_eq!(
        eval(&h, &s, &tab, "document.querySelector('#name').value").await,
        "human newest",
        "Agent与用户始终是同一真实target"
    );
    let screenshot = execute(&h, s.clone(), BrowserCommand::new(A::Screenshot)).await;
    assert!(screenshot
        .screenshot
        .unwrap()
        .starts_with(b"\x89PNG\r\n\x1a\n"));
    let delayed = navigate(&h, &s, format!("{}/slow", site.url)).await;
    assert_eq!(delayed.reply.result["title"], "Delayed browser fixture");
    let back = execute(&h, s.clone(), BrowserCommand::new(A::Back)).await;
    assert_eq!(
        back.reply.result["currentUrl"],
        format!("{}/form", site.url)
    );
    let forward = execute(&h, s.clone(), BrowserCommand::new(A::Forward)).await;
    assert_eq!(forward.reply.result["title"], "Delayed browser fixture");
    assert_eq!(
        eval(&h, &s, &tab, "document.URL").await,
        format!("{}/slow", site.url),
        "前进回执必须与真实当前文档一致"
    );
    eval(&h, &s, &tab, "window.__reloadMarker=true").await;
    let reloaded = execute(&h, s.clone(), BrowserCommand::new(A::Reload)).await;
    assert_eq!(
        reloaded.reply.result["title"],
        "Delayed browser fixture",
        "真实重载回执: {}; HTTP事实: {:?}",
        reloaded.reply.result,
        site.requests.lock().unwrap()
    );
    assert_eq!(
        eval(&h, &s, &tab, "typeof window.__reloadMarker").await,
        "undefined",
        "重载返回时必须已经进入真实新文档"
    );
    let mut close = BrowserCommand::new(A::CloseTab);
    close.tab_id = Some(tab);
    let closed = execute(&h, s.clone(), close).await;
    assert!(closed.reply.state.tabs.is_empty());
    assert!(closed.reply.state.active_tab_id.is_none());
    let next = execute(&h, s, BrowserCommand::new(A::NewTab)).await;
    assert_eq!(next.reply.state.tabs.len(), 1);
    h.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn fixture_idle_connection_never_invents_a_form_response() {
    let site = Site::start();
    let address = site.url.strip_prefix("http://").unwrap();
    let mut idle = std::net::TcpStream::connect(address).unwrap();
    idle.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut response = [0u8; 4096];
    let count = idle.read(&mut response).unwrap();
    assert_eq!(
        count,
        0,
        "空闲连接尚无请求，不得预先返回默认表单: {}",
        String::from_utf8_lossy(&response[..count])
    );
    assert_eq!(
        site.requests.lock().unwrap().as_slice(),
        &["idle/incomplete -> no response"]
    );
}
#[tokio::test]
async fn independent_persistent_profiles_auxiliary_locale_and_site_data_cleanup() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    let a = scope("owner-a");
    let b = scope("owner-b");
    let first = navigate_checked(
        &h,
        &a,
        format!("{}/login", site.url),
        Some((&root, &site, "initial_login")),
    )
    .await;
    let tab = first.reply.state.active_tab_id.clone().unwrap();
    assert!(eval(&h, &a, &tab, "document.cookie")
        .await
        .as_str()
        .unwrap()
        .contains("login=fixture"));
    let second = navigate_checked(
        &h,
        &b,
        format!("{}/form", site.url),
        Some((&root, &site, "isolated_owner_b_form")),
    )
    .await;
    let other = second.reply.state.active_tab_id.unwrap();
    assert_eq!(eval(&h, &b, &other, "document.cookie").await, "");
    navigate_checked(
        &h,
        &a,
        format!("{}/form", site.url),
        Some((&root, &site, "owner_a_form_before_restore")),
    )
    .await;
    let saved = h
        .workspaces()
        .unwrap()
        .into_iter()
        .find(|w| w.session_id == a.session_id)
        .unwrap();
    execute(&h, a.clone(), BrowserCommand::new(A::Stop)).await;
    h.restore(a.clone(), saved).unwrap();
    let resumed = execute_checked(
        &h,
        a.clone(),
        BrowserCommand::new(A::Resume),
        Some((&root, &site, "restore_saved_profile_cookies_allowed")),
    )
    .await;
    let mut tab = resumed.reply.state.active_tab_id.unwrap();
    assert!(eval(&h, &a, &tab, "document.cookie")
        .await
        .as_str()
        .unwrap()
        .contains("login=fixture"));
    let aux = h
        .read_auxiliary_page(
            format!("{}/echo", site.url),
            "document.body.innerText".into(),
            Some("zh-TW".into()),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
    let aux = aux.as_str().unwrap();
    assert!(aux.contains("no-cookie"));
    assert!(aux.to_ascii_lowercase().contains("accept-language: zh-tw"));
    assert_eq!(h.workspaces().unwrap().len(), 2, "辅助搜索不是用户工作区");
    h.configure(BrowserSettings {
        accept_cookies: false,
        ..BrowserSettings::default()
    })
    .await
    .unwrap();
    assert!(
        h.workspaces()
            .unwrap()
            .iter()
            .all(|workspace| !workspace.running),
        "原生Cookie策略切换后应保留冷态，不自动重开或假称DOM保留"
    );
    tab = execute_checked(
        &h,
        a.clone(),
        BrowserCommand::new(A::Resume),
        Some((&root, &site, "resume_cookies_disabled")),
    )
    .await
    .reply
    .state
    .active_tab_id
    .unwrap();
    assert_eq!(eval(&h, &a, &tab, "document.cookie").await, "");
    let echo = navigate_checked(
        &h,
        &a,
        format!("{}/echo", site.url),
        Some((&root, &site, "echo_cookies_disabled")),
    )
    .await;
    let actual_echo_body = eval(&h, &a, &tab, "document.body.innerText").await;
    assert!(
        echo.reply.result["text"]
            .as_str()
            .unwrap()
            .contains("no-cookie"),
        "禁用后真实HTTP响应: {}; 实际DOM正文: {}; HTTP事实: {:?}",
        echo.reply.result,
        actual_echo_body,
        site.requests.lock().unwrap()
    );
    assert!(!echo.reply.result["text"]
        .as_str()
        .unwrap()
        .contains("login=fixture"));
    navigate_checked(
        &h,
        &a,
        format!("{}/blocked-cookie", site.url),
        Some((&root, &site, "set_cookie_while_disabled")),
    )
    .await;
    // 重开时恢复的页面本身不得再次合法设置同名Cookie，否则会混淆禁用期间的因果。
    navigate_checked(
        &h,
        &a,
        format!("{}/echo", site.url),
        Some((&root, &site, "echo_before_reenable")),
    )
    .await;
    h.configure(BrowserSettings::default()).await.unwrap();
    assert!(h
        .workspaces()
        .unwrap()
        .iter()
        .all(|workspace| !workspace.running));
    tab = execute_checked(
        &h,
        a.clone(),
        BrowserCommand::new(A::Resume),
        Some((&root, &site, "resume_cookies_reenabled")),
    )
    .await
    .reply
    .state
    .active_tab_id
    .unwrap();
    let restored_cookie = eval(&h, &a, &tab, "document.cookie").await;
    let restored_cookie = restored_cookie.as_str().unwrap();
    assert!(
        restored_cookie.contains("login=fixture"),
        "禁用Cookie不得清空已有持久登录"
    );
    assert!(
        !restored_cookie.contains("blocked="),
        "禁用期间不得接受新的响应Cookie"
    );
    h.clear_site_data().await.unwrap();
    h.configure(BrowserSettings::default()).await.unwrap();
    let resumed = execute_checked(
        &h,
        a.clone(),
        BrowserCommand::new(A::Resume),
        Some((&root, &site, "resume_after_clear_site_data")),
    )
    .await;
    let tab = resumed.reply.state.active_tab_id.unwrap();
    assert_eq!(eval(&h, &a, &tab, "document.cookie").await, "");
    h.shutdown().await.unwrap();
    assert!(std::fs::read_dir(root.join("aux"))
        .unwrap()
        .next()
        .is_none());
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn downloads_are_actual_owned_bytes_and_ack_removes_only_confirmed_guid() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    let s = scope("download");
    navigate(&h, &s, format!("{}/form", site.url)).await;
    navigate(&h, &s, format!("{}/download", site.url)).await;
    let deadline = Instant::now() + Duration::from_secs(5);
    let download = loop {
        let out = execute(&h, s.clone(), BrowserCommand::new(A::Snapshot)).await;
        if let Some(d) = out.downloads.into_iter().find(|d| d.completed) {
            break d;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(
        download.bytes.as_deref(),
        Some(b"downloaded real bytes\n".as_slice())
    );
    assert_eq!(download.suggested_filename, "proof.txt");
    h.acknowledge_downloads(s.clone(), vec![download.id.clone()])
        .await
        .unwrap();
    h.acknowledge_downloads(s.clone(), vec![download.id])
        .await
        .unwrap();
    assert!(execute(&h, s, BrowserCommand::new(A::Snapshot))
        .await
        .downloads
        .is_empty());
    h.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn scope_revoke_interrupts_real_pending_page_and_stale_node_never_launches() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    let s = scope("revoke");
    let mut wrong = s.clone();
    wrong.execution_node = "remote-other".into();
    let err = h
        .execute(
            wrong,
            BrowserCommand::new(A::Start),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "browser_wrong_node");
    assert!(h.workspaces().unwrap().is_empty());
    let mut show = BrowserCommand::new(A::Show);
    show.timeout = Some(100);
    assert_eq!(
        h.execute(s.clone(), show, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap_err()
            .code,
        "browser_visible_unavailable"
    );
    let opened = navigate(&h, &s, format!("{}/form", site.url)).await;
    let mut command = BrowserCommand::new(A::Evaluate);
    command.tab_id = opened.reply.state.active_tab_id;
    command.expression = Some("new Promise(()=>{})".into());
    let driver = h.clone();
    let old = s.clone();
    let task = tokio::spawn(async move {
        driver
            .execute(old, command, Arc::new(AtomicBool::new(false)))
            .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut revoked = s.clone();
    revoked.epoch = 2;
    h.revoke(revoked).await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(
        h.execute(
            s.clone(),
            BrowserCommand::new(A::Start),
            Arc::new(AtomicBool::new(false))
        )
        .await
        .unwrap_err()
        .code,
        "browser_revoked"
    );
    let mut next = s;
    next.epoch = 3;
    execute(&h, next, BrowserCommand::new(A::Start)).await;
    h.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn pool_lru_is_bounded_and_idle_reap_keeps_cold_metadata() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    for index in 0..6 {
        navigate(
            &h,
            &scope(&format!("session-{index}")),
            format!("{}/form", site.url),
        )
        .await;
    }
    let states = h.workspaces().unwrap();
    assert_eq!(states.iter().filter(|w| w.running).count(), 5);
    assert!(
        !states
            .iter()
            .find(|w| w.session_id == "session-0")
            .unwrap()
            .running
    );
    h.reap_idle(u64::MAX).await.unwrap();
    let states = h.workspaces().unwrap();
    assert!(states.iter().all(|w| !w.running && w.resumable));
    h.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn trusted_click_discovers_a_real_popup_and_dialog_events_do_not_deadlock() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    let s = scope("popup-dialog");
    let mut navigation = BrowserCommand::new(A::Navigate);
    navigation.url = Some(format!("{}/form", site.url));
    let opened = execute_checked(
        &h,
        s.clone(),
        navigation,
        Some((&root, &site, "navigate_form")),
    )
    .await;
    let tab = opened.reply.state.active_tab_id.clone().unwrap();
    let mut dialog = BrowserCommand::new(A::Evaluate);
    dialog.tab_id = Some(tab.clone());
    dialog.expression =
        Some("document.querySelector('#dialog').click();document.body.dataset.dialog".into());
    assert_eq!(
        execute_checked(
            &h,
            s.clone(),
            dialog,
            Some((&root, &site, "evaluate_dialog")),
        )
        .await
        .reply
        .result["value"],
        "done"
    );
    let fresh = execute_checked(
        &h,
        s.clone(),
        BrowserCommand::new(A::Snapshot),
        Some((&root, &site, "snapshot_before_click")),
    )
    .await;
    let reference = regex::Regex::new(r"\[(\d+)\][^\[\]\n]*Popup")
        .unwrap()
        .captures(fresh.reply.result["text"].as_str().unwrap())
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().parse::<u32>().unwrap())
        .unwrap();
    let mut click = BrowserCommand::new(A::Click);
    click.tab_id = Some(tab.clone());
    click.element_ref = Some(reference);
    execute_checked(
        &h,
        s.clone(),
        click,
        Some((&root, &site, "trusted_popup_click")),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let out = execute_checked(
            &h,
            s.clone(),
            BrowserCommand::new(A::Snapshot),
            Some((&root, &site, "poll_popup_snapshot")),
        )
        .await;
        if out.reply.state.tabs.len() == 2 {
            assert!(out
                .reply
                .state
                .tabs
                .iter()
                .any(|t| t.url.as_deref().is_some_and(|u| u.ends_with("/popup"))));
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    h.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn permanent_forget_removes_owned_profile_and_rejects_late_scopes() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    let s = scope("permanently-deleted");
    let opened = navigate(&h, &s, format!("{}/login", site.url)).await;
    let cold = opened.reply.state;
    assert_eq!(std::fs::read_dir(root.join("profiles")).unwrap().count(), 1);
    let mut retired = s.clone();
    retired.epoch += 1;
    h.forget(retired.clone()).await.unwrap();
    assert!(h.workspaces().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(root.join("profiles")).unwrap().count(), 0);
    assert_eq!(
        std::fs::read_dir(root.join("downloads")).unwrap().count(),
        0
    );
    h.forget(retired.clone()).await.unwrap();
    for epoch in [s.epoch, retired.epoch, 999] {
        let mut late = s.clone();
        late.epoch = epoch;
        assert_eq!(
            h.execute(
                late.clone(),
                BrowserCommand::new(A::Start),
                Arc::new(AtomicBool::new(false))
            )
            .await
            .unwrap_err()
            .code,
            "browser_revoked"
        );
        assert!(h.restore(late, cold.clone()).is_err());
    }
    let fresh = scope("other-session");
    assert!(
        navigate(&h, &fresh, format!("{}/form", site.url))
            .await
            .reply
            .state
            .running
    );
    h.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn password_values_never_enter_normal_or_compact_snapshots_and_stay_in_the_page() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    let s = scope("login-password");
    let opened = navigate(&h, &s, format!("{}/form", site.url)).await;
    let tab = opened.reply.state.active_tab_id.unwrap();
    assert_eq!(eval(&h, &s, &tab, "(()=>{document.body.insertAdjacentHTML('beforeend','<section><input type=\"password\" aria-label=\"Login password\" value=\"synthetic-login-secret\"></section><section><input type=\"password\" value=\"synthetic-compact-one\"><input type=\"password\" value=\"synthetic-compact-two\"><input type=\"password\" value=\"synthetic-compact-three\"></section>');return document.querySelectorAll('input[type=password]').length===4;})()").await, true);
    let snapshot = execute(&h, s.clone(), BrowserCommand::new(A::Snapshot)).await;
    let recorded = serde_json::to_string(&snapshot.reply).unwrap();
    for secret in [
        "synthetic-login-secret",
        "synthetic-compact-one",
        "synthetic-compact-two",
        "synthetic-compact-three",
    ] {
        assert!(!recorded.contains(secret), "密码不能进入快照、名称或回执");
    }
    assert!(recorded.contains("Login password"));
    assert!(recorded.contains("value=[redacted]"));
    assert_eq!(
        eval(
            &h,
            &s,
            &tab,
            "document.querySelector('input[type=password]').value==='synthetic-login-secret'"
        )
        .await,
        true,
        "隐藏快照中的密码不能清空用户页面里的登录内容"
    );
    h.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn page_cannot_move_or_forge_a_snapshot_reference_to_another_element() {
    let root = temp();
    let site = Site::start();
    let (h, _network_guard) = host(&root);
    let s = scope("frozen-elements");
    let opened = navigate(&h, &s, format!("{}/form", site.url)).await;
    let tab = opened.reply.state.active_tab_id.clone().unwrap();
    let input_ref = |output: &BrowserOutput| {
        regex::Regex::new(r"\[(\d+)\][^\n]*Identity")
            .unwrap()
            .captures(output.reply.result["text"].as_str().unwrap())
            .unwrap()[1]
            .parse::<u32>()
            .unwrap()
    };
    let initial = input_ref(&opened);
    eval(&h,&s,&tab,&format!("(()=>{{const original=document.querySelector('#name');original.removeAttribute('data-hana-ref');const decoy=document.createElement('input');decoy.id='decoy';decoy.setAttribute('aria-label','Decoy');decoy.setAttribute('data-hana-ref','{initial}');document.body.appendChild(decoy);globalThis.__lingxiFrozenReferences=new Map([[{initial},decoy]]);return true;}})()")).await;
    let mut typed = BrowserCommand::new(A::Type);
    typed.tab_id = Some(tab.clone());
    typed.element_ref = Some(initial);
    typed.text = Some("only original element".into());
    let frozen = execute(&h, s.clone(), typed).await;
    assert_eq!(
        eval(&h, &s, &tab, "document.querySelector('#name').value").await,
        "only original element"
    );
    assert_eq!(
        eval(&h, &s, &tab, "document.querySelector('#decoy').value").await,
        ""
    );
    let reference = input_ref(&frozen);
    eval(&h,&s,&tab,&format!("(()=>{{const original=document.querySelector('#name');const replacement=original.cloneNode(true);replacement.id='replacement';replacement.value='';replacement.setAttribute('data-hana-ref','{reference}');original.replaceWith(replacement);globalThis.__lingxiFrozenReferences=new Map([[{reference},replacement]]);return true;}})()")).await;
    let mut refused = BrowserCommand::new(A::Type);
    refused.tab_id = Some(tab.clone());
    refused.element_ref = Some(reference);
    refused.text = Some("must never reach replacement".into());
    assert_eq!(
        h.execute(s.clone(), refused, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap_err()
            .code,
        "browser_stale_snapshot"
    );
    assert_eq!(
        eval(&h, &s, &tab, "document.querySelector('#replacement').value").await,
        ""
    );
    h.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
