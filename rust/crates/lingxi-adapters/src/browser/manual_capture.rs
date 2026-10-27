//! 仅显式执行的原生窗口证据：录制本测试创建的单一target，不采集桌面或其他窗口。
use super::*;
use base64::Engine;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread::JoinHandle,
};

struct LocalSite {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl LocalSite {
    fn new(guard: bool, title: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = std::thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut io, _)) => {
                        io.set_read_timeout(Some(Duration::from_millis(300)))
                            .unwrap();
                        let mut request = Vec::new();
                        let mut bytes = [0u8; 2048];
                        while request.len() < 16_384
                            && !request.windows(4).any(|w| w == b"\r\n\r\n")
                        {
                            match io.read(&mut bytes) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => request.extend_from_slice(&bytes[..n]),
                            }
                        }
                        if !request.windows(4).any(|w| w == b"\r\n\r\n") {
                            continue;
                        }
                        let (status, body) = if guard {
                            ("403 Forbidden", String::new())
                        } else {
                            ("200 OK",format!("<!doctype html><meta charset='utf-8'><title>{title}</title><body style='font:24px sans-serif;padding:48px'><h1>灵犀同页接管本地检查</h1><p>仅录制此测试表单。请把输入内容改为 human-latest。</p><label>测试输入 <input id='handoff-input' aria-label='测试输入' style='font:24px sans-serif;width:360px;padding:12px'></label></body>"))
                        };
                        let response=format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",body.len());
                        let _ = io.write_all(response.as_bytes());
                    }
                    Err(cause) if cause.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for LocalSite {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn random() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).unwrap();
    bytes.iter().map(|n| format!("{n:02x}")).collect()
}
fn write_json(path: &Path, value: Value) {
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
struct Safety {
    host: Arc<ChromiumHost>,
    profile: PathBuf,
    evidence: PathBuf,
}
impl Drop for Safety {
    fn drop(&mut self) {
        let stopped = self.host.core.close_all(true);
        let confirmed = stopped.is_ok();
        let _ = std::fs::write(
            self.evidence.join("cleanup.json"),
            serde_json::to_vec(
                &json!({"confirmed":stopped.is_ok(),"error":stopped.err().map(|error|error.code)}),
            )
            .unwrap(),
        );
        if confirmed {
            let _ = std::fs::remove_dir_all(&self.profile);
        }
    }
}
#[derive(Default)]
struct Frames {
    records: Vec<Value>,
    bytes: usize,
    error: Option<String>,
}
struct Recording {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    frames: Arc<Mutex<Frames>>,
}
impl Recording {
    fn start(live: &Live, sid: String, directory: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let cdp = live.cdp().clone();
        let frames = Arc::new(Mutex::new(Frames::default()));
        let collected = frames.clone();
        let mut cursor = cdp.cursor();
        let worker = std::thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                let events = match cdp.events(cursor) {
                    Ok(events) => events,
                    Err(error) => {
                        collected.lock().unwrap().error = Some(error.code);
                        break;
                    }
                };
                for event in events {
                    cursor = event.sequence;
                    if event.value["sessionId"] != sid
                        || event.value["method"] != "Page.screencastFrame"
                    {
                        continue;
                    }
                    let frame = &event.value["params"];
                    let outcome = (|| -> Result<(), String> {
                        let encoded = frame["data"].as_str().ok_or("missing frame")?;
                        if encoded.len() > 3 * 1024 * 1024 {
                            return Err("frame budget".into());
                        }
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(encoded)
                            .map_err(|_| "invalid frame")?;
                        if !bytes.starts_with(&[0xff, 0xd8]) {
                            return Err("not JPEG".into());
                        }
                        let mut frames = collected.lock().unwrap();
                        if frames.records.len() >= 512
                            || frames.bytes + bytes.len() > 32 * 1024 * 1024
                        {
                            return Err("recording budget".into());
                        }
                        let filename = format!("frame-{:04}.jpg", frames.records.len());
                        std::fs::write(directory.join(&filename), &bytes)
                            .map_err(|_| "frame write failed")?;
                        frames.bytes += bytes.len();
                        frames.records.push(json!({"file":filename,"cdpTimestamp":frame["metadata"]["timestamp"],"receivedAtUnixMs":now_ms(),"bytes":bytes.len()}));
                        Ok(())
                    })();
                    // 不论是否到预算，都确认本target帧；不让录制器阻塞真实页面。
                    let ack = cdp.call(
                        Some(&sid),
                        "Page.screencastFrameAck",
                        json!({"sessionId":frame["sessionId"]}),
                        Instant::now() + Duration::from_secs(1),
                        &flag,
                        false,
                    );
                    if let Err(error) = outcome {
                        collected.lock().unwrap().error = Some(error);
                        flag.store(true, Ordering::Release);
                    }
                    if let Err(error) = ack {
                        collected.lock().unwrap().error = Some(error.code);
                        flag.store(true, Ordering::Release);
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Self {
            stop,
            worker: Some(worker),
            frames,
        }
    }
    fn finish(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
async fn command(
    host: &ChromiumHost,
    scope: &BrowserScope,
    command: BrowserCommand,
) -> BrowserOutput {
    host.execute(scope.clone(), command, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "需要显式提供证据目录，并在原生Chrome测试表单中手动输入"]
async fn same_page_native_human_takeover_records_only_owned_target() {
    let base = std::env::var_os("LINGXI_BROWSER_MANUAL_EVIDENCE_DIR")
        .map(PathBuf::from)
        .expect("显式设置证据父目录");
    assert!(base.is_absolute());
    private_dir(&base).unwrap();
    let nonce = random();
    let evidence = base.join(format!("capture-{nonce}"));
    private_dir(&evidence).unwrap();
    let frames_dir = evidence.join("frames");
    private_dir(&frames_dir).unwrap();
    let title = format!("Lingxi R07 Own Browser {nonce}");
    let site = LocalSite::new(false, title.clone());
    let proxy = LocalSite::new(true, String::new());
    let profile = std::env::temp_dir().join(format!("lingxi-browser-manual-{nonce}"));
    let executable = std::env::var_os("LINGXI_TEST_CHROMIUM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        });
    let host = ChromiumHost::new(ChromiumConfig {
        executable,
        root: profile.clone(),
        execution_node: "manual-fixture-node".into(),
        headless: false,
        network: NetworkPlane::isolated(crate::models::network::NetworkPolicy {
            proxy: ProxyPolicy::Manual {
                http: Some(proxy.url.clone()),
                https: Some(proxy.url.clone()),
                no_proxy: "localhost,127.0.0.1,::1".into(),
            },
            trusted_ca_pem: None,
        }),
    })
    .unwrap();
    let _safety = Safety {
        host: host.clone(),
        profile,
        evidence: evidence.clone(),
    };
    let scope = BrowserScope {
        session_id: format!("manual-{nonce}"),
        owner_subject: "user_local".into(),
        epoch: 1,
        execution_node: "manual-fixture-node".into(),
        human: false,
    };
    let mut navigate = BrowserCommand::new(BrowserAction::Navigate);
    navigate.url = Some(format!("{}/own-form", site.url));
    let opened = command(&host, &scope, navigate).await;
    let tab = opened.reply.state.active_tab_id.unwrap();
    let reference = regex::Regex::new(r"\[(\d+)\][^\n]*测试输入")
        .unwrap()
        .captures(opened.reply.result["text"].as_str().unwrap())
        .unwrap()[1]
        .parse::<u32>()
        .unwrap();
    let mut fill = BrowserCommand::new(BrowserAction::Type);
    fill.tab_id = Some(tab.clone());
    fill.element_ref = Some(reference);
    fill.text = Some("agent-initial".into());
    command(&host, &scope, fill).await;
    let live = host.core.pool.lock().unwrap().entries[&scope.session_id]
        .live
        .as_ref()
        .unwrap()
        .clone();
    let sid = live
        .attach(
            &tab,
            Instant::now() + Duration::from_secs(5),
            &AtomicBool::new(false),
        )
        .unwrap();
    let mut recording = Recording::start(&live, sid.clone(), frames_dir);
    live.call(
        Some(&sid),
        "Page.startScreencast",
        json!({"format":"jpeg","quality":60,"maxWidth":1024,"maxHeight":768,"everyNthFrame":1}),
        Instant::now() + Duration::from_secs(5),
        &AtomicBool::new(false),
        false,
    )
    .unwrap();
    let mut human = scope.clone();
    human.human = true;
    command(&host, &human, BrowserCommand::new(BrowserAction::Takeover)).await;
    assert!(live.visible.load(Ordering::Acquire));
    let mut blocked = BrowserCommand::new(BrowserAction::Scroll);
    blocked.direction = Some("down".into());
    assert_eq!(
        host.execute(scope.clone(), blocked, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap_err()
            .code,
        "browser_user_takeover"
    );
    let continue_path = evidence.join("continue.json");
    write_json(
        &evidence.join("ready.json"),
        json!({"phase":"user_takeover","url":format!("{}/own-form",site.url),"targetId":tab,"sessionId":scope.session_id,"windowTitle":title,"inputId":"handoff-input","expectedValue":"human-latest","continuePath":continue_path,"recordingScope":"this_target_only"}),
    );
    println!("MANUAL_READY={}", evidence.join("ready.json").display());
    let wait = std::env::var("LINGXI_BROWSER_MANUAL_WAIT_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(120)
        .clamp(30, 120);
    let deadline = Instant::now() + Duration::from_secs(wait);
    while !continue_path.is_file() {
        assert!(
            Instant::now() < deadline,
            "等待原生表单操作超时，已准备清理自有Chrome；证据目录 {}",
            evidence.display()
        );
        let error = recording.frames.lock().unwrap().error.clone();
        assert!(error.is_none(), "录制失败: {error:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let observed = command(&host, &scope, BrowserCommand::new(BrowserAction::Snapshot)).await;
    assert_eq!(
        observed.reply.state.active_tab_id.as_deref(),
        Some(tab.as_str())
    );
    assert!(
        observed.reply.result["text"]
            .as_str()
            .unwrap()
            .contains("human-latest"),
        "原生表单修改未进入同一真实target: {}",
        observed.reply.result
    );
    command(&host, &human, BrowserCommand::new(BrowserAction::Release)).await;
    let released = command(&host, &scope, BrowserCommand::new(BrowserAction::Snapshot)).await;
    assert_eq!(
        released.reply.state.active_tab_id.as_deref(),
        Some(tab.as_str())
    );
    assert!(released.reply.result["text"]
        .as_str()
        .unwrap()
        .contains("human-latest"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    live.call(
        Some(&sid),
        "Page.stopScreencast",
        json!({}),
        Instant::now() + Duration::from_secs(3),
        &AtomicBool::new(false),
        false,
    )
    .unwrap();
    recording.finish();
    let (capture, frame_count) = {
        let frames = recording.frames.lock().unwrap();
        assert!(frames.error.is_none(), "{:?}", frames.error);
        assert!(
            frames.records.len() >= 2,
            "需要初始页面与真实编辑后的录制帧"
        );
        (
            json!({"targetId":tab,"frames":frames.records,"totalBytes":frames.bytes,"onlyOwnedTarget":true}),
            frames.records.len(),
        )
    };
    write_json(&evidence.join("capture.json"), capture);
    write_json(
        &evidence.join("result.json"),
        json!({"sameTarget":true,"targetId":tab,"afterTakeover":observed.reply.result,"afterRelease":released.reply.result,"frames":frame_count,"initialValue":"agent-initial","expectedNativeValue":"human-latest"}),
    );
    host.shutdown().await.unwrap();
    println!("MANUAL_COMPLETE={}", evidence.display());
}

#[tokio::test]
#[ignore = "需要可见Chrome窗口，单独执行原生恢复与回收检查"]
async fn externally_restored_suspended_window_is_never_reaped_or_evicted() {
    let nonce = random();
    let site = LocalSite::new(false, format!("Lingxi R07 Own LRU {nonce}"));
    let proxy = LocalSite::new(true, String::new());
    let evidence = std::env::temp_dir().join(format!("lingxi-browser-visible-evidence-{nonce}"));
    private_dir(&evidence).unwrap();
    let profile = std::env::temp_dir().join(format!("lingxi-browser-visible-{nonce}"));
    let executable = std::env::var_os("LINGXI_TEST_CHROMIUM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        });
    let host = ChromiumHost::new(ChromiumConfig {
        executable,
        root: profile.clone(),
        execution_node: "visible-fixture-node".into(),
        headless: false,
        network: NetworkPlane::isolated(crate::models::network::NetworkPolicy {
            proxy: ProxyPolicy::Manual {
                http: Some(proxy.url.clone()),
                https: Some(proxy.url.clone()),
                no_proxy: "localhost,127.0.0.1,::1".into(),
            },
            trusted_ca_pem: None,
        }),
    })
    .unwrap();
    let _safety = Safety {
        host: host.clone(),
        profile,
        evidence: evidence.clone(),
    };
    let scopes = (0..6)
        .map(|i| BrowserScope {
            session_id: format!("visible-{nonce}-{i}"),
            owner_subject: "user_local".into(),
            epoch: 1,
            execution_node: "visible-fixture-node".into(),
            human: false,
        })
        .collect::<Vec<_>>();
    for scope in scopes.iter().take(5) {
        let mut navigate = BrowserCommand::new(BrowserAction::Navigate);
        navigate.url = Some(format!("{}/owned", site.url));
        command(&host, scope, navigate).await;
    }
    let first = &scopes[0];
    let live = host.core.pool.lock().unwrap().entries[&first.session_id]
        .live
        .as_ref()
        .unwrap()
        .clone();
    let tab = live.active.lock().unwrap().clone().unwrap();
    let externally_restore = || {
        assert!(!live.visible.load(Ordering::Acquire));
        let window = live
            .call(
                None,
                "Browser.getWindowForTarget",
                json!({"targetId":tab}),
                Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false),
                false,
            )
            .unwrap();
        let id = window["windowId"].as_i64().unwrap();
        live.call(
            None,
            "Browser.setWindowBounds",
            json!({"windowId":id,"bounds":{"windowState":"normal"}}),
            Instant::now() + Duration::from_secs(2),
            &AtomicBool::new(false),
            false,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let bounds = live
                .call(
                    None,
                    "Browser.getWindowBounds",
                    json!({"windowId":id}),
                    deadline,
                    &AtomicBool::new(false),
                    false,
                )
                .unwrap();
            if bounds["bounds"]["windowState"] == "normal" {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !live.visible.load(Ordering::Acquire),
            "外部恢复不能借Port顺便更新缓存，否则未覆盖旧缺陷"
        );
    };
    command(&host, first, BrowserCommand::new(BrowserAction::Suspend)).await;
    externally_restore();
    host.reap_idle(u64::MAX).await.unwrap();
    assert!(
        host.workspaces()
            .unwrap()
            .iter()
            .find(|s| s.session_id == first.session_id)
            .unwrap()
            .running,
        "原生恢复后不得被idle回收"
    );
    command(&host, first, BrowserCommand::new(BrowserAction::Suspend)).await;
    externally_restore();
    let error = host
        .execute(
            scopes[5].clone(),
            BrowserCommand::new(BrowserAction::Start),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "browser_capacity");
    let visible = host.workspaces().unwrap();
    assert_eq!(visible.iter().filter(|s| s.running).count(), 5);
    assert_eq!(
        visible
            .iter()
            .find(|s| s.session_id == first.session_id)
            .unwrap()
            .active_tab_id
            .as_deref(),
        Some(tab.as_str())
    );
    write_json(
        &evidence.join("result.json"),
        json!({"actualExternalRestore":true,"idleKeptTarget":tab,"lruRejectedSixth":true,"running":5}),
    );
    host.shutdown().await.unwrap();
    println!("VISIBLE_REAP_EVIDENCE={}", evidence.display());
}
