//! R09-T05 逐原断言真实执行（适配器层，真实 Chromium/CDP）。
//!
//! 只用于 R09-T05 生产者 `r09_t05_browser_pdf` 的逐断言 case：
//!   BROWSER-TABS A1/A2、BROWSER-NAVIGATION A1/A2、BROWSER-TOOL A1/A2/A3/A4。
//! 全部使用本机真实 Chromium + 本机 loopback fixture，不访问公网、不写用户目录；
//! 浏览器命令经正式 `BrowserPort`/`ChromiumHost`、真实 CDP pipe 与真实进程。
//!
//! 运行：`cargo test -p lingxi-adapters --test r09_t05_assertions -- --test-threads=1`
#![cfg(unix)]

use lingxi_adapters::{
    browser::{ChromiumConfig, ChromiumHost},
    models::network::{NetworkPlane, NetworkPolicy, ProxyPolicy},
};
use lingxi_kernel::browser::{BrowserError, BrowserOutput, BrowserPort, BrowserScope};
use lingxi_protocol::{BrowserAction as A, BrowserCommand, BrowserWorkspace};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
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
    .find(|path| path.is_file())
    .expect("R09-T05 逐断言真实执行需要本机 Chromium")
}

fn temp() -> PathBuf {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).unwrap();
    let suffix: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let path = std::env::temp_dir().join(format!("lingxi-r09-t05-assert-{suffix}"));
    std::fs::create_dir(&path).unwrap();
    path
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

/// 真实 loopback fixture：页面 a/b/login/echo。
struct Site {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Site {
    fn start() -> Self {
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
                                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                            }
                        }
                        if !request.windows(4).any(|window| window == b"\r\n\r\n") {
                            continue;
                        }
                        let request = String::from_utf8_lossy(&request).into_owned();
                        let path = request
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/")
                            .to_string();
                        if let Ok(mut trace) = audit.lock() {
                            if trace.len() < 256 {
                                trace.push(path.clone());
                            }
                        }
                        let (body, extra) = match path.as_str() {
                            "/b" => (
                                "<!doctype html><title>R09 Page B</title><body><h1>PAGE-B-MARKER</h1><p>beta content</p></body>".to_string(),
                                String::new(),
                            ),
                            "/echo" => {
                                let cookie = request
                                    .lines()
                                    .find(|line| {
                                        line.to_ascii_lowercase().starts_with("cookie:")
                                    })
                                    .unwrap_or("no-cookie");
                                (
                                    format!(
                                        "<!doctype html><title>R09 Echo</title><body><pre>{cookie}</pre></body>"
                                    ),
                                    String::new(),
                                )
                            }
                            path if path.starts_with("/login") => (
                                "<!doctype html><title>R09 Login</title><body><h1>PAGE-LOGIN-MARKER</h1></body>".to_string(),
                                "Set-Cookie: login=fixture; Path=/; Max-Age=3600\r\n".to_string(),
                            ),
                            _ => (
                                "<!doctype html><title>R09 Page A</title><body><h1>PAGE-A-MARKER</h1><a id='to-b' href='/b'>to b</a><input id='name' aria-label='Name'><button id='btn'>Click</button><p>alpha content</p></body>".to_string(),
                                String::new(),
                            ),
                        };
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
                            body.len()
                        );
                        let _ = io.write_all(header.as_bytes());
                        let _ = io.write_all(body.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
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
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn host(root: &std::path::Path) -> (Arc<ChromiumHost>, Site) {
    let site = Site::start();
    // 非 loopback 真正出网路径指向本机不可达端口；loopback fixture 显式绕过代理。
    let network = NetworkPlane::isolated(NetworkPolicy {
        proxy: ProxyPolicy::Manual {
            http: Some("http://127.0.0.1:9".into()),
            https: Some("http://127.0.0.1:9".into()),
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
    (host, site)
}

async fn execute(
    host: &ChromiumHost,
    scope: &BrowserScope,
    command: BrowserCommand,
) -> BrowserOutput {
    let action = command.action;
    match host
        .execute(scope.clone(), command, Arc::new(AtomicBool::new(false)))
        .await
    {
        Ok(output) => output,
        Err(cause) => panic!("真实浏览器命令意外失败: action={action:?}, cause={cause:?}"),
    }
}

async fn execute_err(
    host: &ChromiumHost,
    scope: &BrowserScope,
    command: BrowserCommand,
) -> BrowserError {
    let action = command.action;
    match host
        .execute(scope.clone(), command, Arc::new(AtomicBool::new(false)))
        .await
    {
        Err(cause) => cause,
        Ok(output) => panic!(
            "真实浏览器命令应明确失败却成功: action={action:?}, state={:?}",
            output.reply.state
        ),
    }
}

async fn navigate(host: &ChromiumHost, scope: &BrowserScope, url: &str) -> BrowserOutput {
    let mut command = BrowserCommand::new(A::Navigate);
    command.url = Some(url.into());
    execute(host, scope, command).await
}

async fn snapshot(host: &ChromiumHost, scope: &BrowserScope, tab: &str) -> serde_json::Value {
    let mut command = BrowserCommand::new(A::Snapshot);
    command.tab_id = Some(tab.into());
    execute(host, scope, command).await.reply.result
}

async fn eval(
    host: &ChromiumHost,
    scope: &BrowserScope,
    tab: &str,
    expression: &str,
) -> serde_json::Value {
    let mut command = BrowserCommand::new(A::Evaluate);
    command.tab_id = Some(tab.into());
    command.expression = Some(expression.into());
    execute(host, scope, command).await.reply.result["value"].clone()
}

fn workspace(host: &ChromiumHost, session: &str) -> Option<BrowserWorkspace> {
    host.workspaces()
        .unwrap()
        .into_iter()
        .find(|row| row.session_id == session)
}

fn tab_ids(workspace: &BrowserWorkspace) -> Vec<String> {
    workspace
        .tabs
        .iter()
        .map(|tab| tab.tab_id.clone())
        .collect()
}

fn assert_state_unchanged(before: &BrowserWorkspace, after: &BrowserWorkspace) {
    assert_eq!(tab_ids(before), tab_ids(after), "标签集合必须保持不变");
    assert_eq!(
        before.active_tab_id, after.active_tab_id,
        "活动标签必须保持不变"
    );
    assert_eq!(before.url, after.url, "URL 必须保持不变");
    assert_eq!(before.running, after.running, "运行状态必须保持不变");
}

// ── BROWSER-TABS ────────────────────────────────────────────────────────────

/// A1：真实 Chromium target 的标签集合/活动标签与服务端快照逐步同步。
#[tokio::test]
async fn browser_tabs_a1_tab_set_and_active_follow_real_target_changes() {
    let root = temp();
    let (host, site) = host(&root);
    let session = scope("tabs-a1");
    let first = navigate(&host, &session, &format!("{}/a", site.url)).await;
    let tab1 = first.reply.state.active_tab_id.clone().unwrap();
    let mut state = first.reply.state.clone();
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.tabs[0].tab_id, tab1);
    assert_eq!(
        state.url.as_deref(),
        Some(format!("{}/a", site.url).as_str())
    );

    let mut new_tab = BrowserCommand::new(A::NewTab);
    new_tab.url = Some(format!("{}/b", site.url));
    let second = execute(&host, &session, new_tab).await;
    let tab2 = second.reply.state.active_tab_id.clone().unwrap();
    assert_ne!(tab1, tab2);
    assert_eq!(second.reply.state.tabs.len(), 2);
    assert_eq!(
        tab_ids(&second.reply.state),
        vec![tab1.clone(), tab2.clone()]
    );

    let mut switch = BrowserCommand::new(A::SwitchTab);
    switch.tab_id = Some(tab1.clone());
    let switched = execute(&host, &session, switch).await;
    assert_eq!(
        switched.reply.state.active_tab_id.as_deref(),
        Some(tab1.as_str())
    );
    assert_eq!(
        switched.reply.state.url.as_deref(),
        Some(format!("{}/a", site.url).as_str())
    );

    // 服务端快照跟随：快照 url/title 与活动 target 一致。
    let snap = snapshot(&host, &session, &tab1).await;
    assert_eq!(snap["url"], format!("{}/a", site.url));
    assert_eq!(snap["title"], "R09 Page A");
    assert!(snap["text"].as_str().unwrap().contains("PAGE-A-MARKER"));

    let mut close = BrowserCommand::new(A::CloseTab);
    close.tab_id = Some(tab2.clone());
    let closed = execute(&host, &session, close).await;
    state = closed.reply.state.clone();
    assert_eq!(tab_ids(&state), vec![tab1.clone()]);
    assert_eq!(state.active_tab_id.as_deref(), Some(tab1.as_str()));
    let current = workspace(&host, "tabs-a1").unwrap();
    assert_eq!(tab_ids(&current), vec![tab1.clone()]);
    assert_eq!(current.active_tab_id.as_deref(), Some(tab1.as_str()));
    // 真实 fixture 确实收到过两个页面的请求（不是缓存/伪造状态）。
    let requests = site.requests.lock().unwrap().clone();
    assert!(requests.iter().any(|path| path == "/a"));
    assert!(requests.iter().any(|path| path == "/b"));
    host.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// A2：无效 tabId 不执行（协议非法 + 未知 tab），关闭失败如实报错且状态不变。
#[tokio::test]
async fn browser_tabs_a2_invalid_tab_id_and_close_failure_are_explicit_and_state_unchanged() {
    let root = temp();
    let (host, site) = host(&root);
    let session = scope("tabs-a2");
    let first = navigate(&host, &session, &format!("{}/a", site.url)).await;
    let tab = first.reply.state.active_tab_id.clone().unwrap();
    let before = workspace(&host, "tabs-a2").unwrap();

    // 协议层非法 tabId：不派发任何外部动作。
    for invalid in [
        "bad*id",
        "bad?id",
        " leading",
        "trailing ",
        "with\u{1f}control",
    ] {
        let mut command = BrowserCommand::new(A::SwitchTab);
        command.tab_id = Some(invalid.into());
        let error = execute_err(&host, &session, command).await;
        assert_eq!(
            error.code, "invalid_browser_command",
            "非法 tabId 必须显式拒绝"
        );
    }
    // 未知但格式合法的 tabId：真实浏览器命令被拒（browser_stale_tab）。
    let mut unknown = BrowserCommand::new(A::CloseTab);
    unknown.tab_id = Some("tab-that-does-not-exist".into());
    let error = execute_err(&host, &session, unknown).await;
    assert_eq!(error.code, "browser_stale_tab");
    let mut unknown_switch = BrowserCommand::new(A::SwitchTab);
    unknown_switch.tab_id = Some("tab-that-does-not-exist".into());
    assert_eq!(
        execute_err(&host, &session, unknown_switch).await.code,
        "browser_stale_tab"
    );
    // 关闭已关闭的标签：失败如实返回，不得显示已关闭。
    let mut close = BrowserCommand::new(A::CloseTab);
    close.tab_id = Some(tab.clone());
    execute(&host, &session, close).await;
    let mut close_again = BrowserCommand::new(A::CloseTab);
    close_again.tab_id = Some(tab.clone());
    let error = execute_err(&host, &session, close_again).await;
    assert_eq!(error.code, "browser_stale_tab");

    let after = workspace(&host, "tabs-a2").unwrap();
    assert!(after.tabs.is_empty(), "关闭失败不能把标签显示为仍存在");
    assert_eq!(after.running, before.running);
    let reload = BrowserCommand::new(A::Reload);
    let error = execute_err(&host, &session, reload).await;
    assert_eq!(error.code, "browser_empty_workspace");
    let mut reload_stale = BrowserCommand::new(A::Reload);
    reload_stale.tab_id = Some(tab.clone());
    assert_eq!(
        execute_err(&host, &session, reload_stale).await.code,
        "browser_stale_tab"
    );
    host.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

// ── BROWSER-NAVIGATION ──────────────────────────────────────────────────────

/// A1：活动会话真实导航（后退/前进/刷新）更新 URL/标题/可用状态。
#[tokio::test]
async fn browser_navigation_a1_back_forward_reload_update_url_title_and_flags() {
    let root = temp();
    let (host, site) = host(&root);
    let session = scope("nav-a1");
    let first = navigate(&host, &session, &format!("{}/a", site.url)).await;
    let tab = first.reply.state.active_tab_id.clone().unwrap();
    let second = navigate(&host, &session, &format!("{}/b", site.url)).await;
    assert_eq!(
        second.reply.state.url.as_deref(),
        Some(format!("{}/b", site.url).as_str())
    );
    assert_eq!(second.reply.state.title.as_deref(), Some("R09 Page B"));
    assert!(second.reply.state.tabs[0].can_go_back);

    let back = execute(&host, &session, BrowserCommand::new(A::Back)).await;
    assert_eq!(
        back.reply.state.url.as_deref(),
        Some(format!("{}/a", site.url).as_str())
    );
    assert_eq!(back.reply.state.title.as_deref(), Some("R09 Page A"));
    assert!(back.reply.state.tabs[0].can_go_forward);
    let snap = snapshot(&host, &session, &tab).await;
    assert_eq!(snap["url"], format!("{}/a", site.url));

    let forward = execute(&host, &session, BrowserCommand::new(A::Forward)).await;
    assert_eq!(
        forward.reply.state.url.as_deref(),
        Some(format!("{}/b", site.url).as_str())
    );
    assert_eq!(forward.reply.state.title.as_deref(), Some("R09 Page B"));

    let reload = execute(&host, &session, BrowserCommand::new(A::Reload)).await;
    assert_eq!(
        reload.reply.state.url.as_deref(),
        Some(format!("{}/b", site.url).as_str())
    );
    assert_eq!(reload.reply.state.title.as_deref(), Some("R09 Page B"));
    host.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// A2：无活动 view 不执行；非法地址（javascript:/file:/data:/无 scheme/userinfo）不加载。
#[tokio::test]
async fn browser_navigation_a2_no_active_view_and_illegal_urls_fail_explicitly() {
    let root = temp();
    let (host, site) = host(&root);
    let session = scope("nav-a2");
    let first = navigate(&host, &session, &format!("{}/a", site.url)).await;
    let tab = first.reply.state.active_tab_id.clone().unwrap();
    let mut close = BrowserCommand::new(A::CloseTab);
    close.tab_id = Some(tab.clone());
    execute(&host, &session, close).await;
    // 无活动视图：这些动作必须显式失败，不得成功或误操作。
    for mut command in [
        BrowserCommand::new(A::Back),
        BrowserCommand::new(A::Forward),
        BrowserCommand::new(A::Reload),
        BrowserCommand::new(A::Snapshot),
        BrowserCommand::new(A::Screenshot),
        {
            let mut scroll = BrowserCommand::new(A::Scroll);
            scroll.direction = Some("down".into());
            scroll.amount = Some(1.0);
            scroll
        },
    ] {
        let error = execute_err(&host, &session, command).await;
        assert_eq!(error.code, "browser_empty_workspace");
    }
    // 重新开一个真实标签后，非法地址不得加载。
    let mut new_tab = BrowserCommand::new(A::NewTab);
    new_tab.url = Some(format!("{}/a", site.url));
    let reopened = execute(&host, &session, new_tab).await;
    let tab = reopened.reply.state.active_tab_id.clone().unwrap();
    // NewTab 回复里的 state 可能仍是建页瞬间；先读一次真实快照得到加载后的地址基线。
    let before = snapshot(&host, &session, &tab).await["url"].clone();
    for illegal in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,<script>alert(1)</script>",
        "ftp://127.0.0.1/x",
        "//127.0.0.1/x",
        "http://user:pass@127.0.0.1/x",
    ] {
        let mut command = BrowserCommand::new(A::Navigate);
        command.tab_id = Some(tab.clone());
        command.url = Some(illegal.into());
        let error = execute_err(&host, &session, command).await;
        assert_eq!(
            error.code, "browser_url_forbidden",
            "非法地址必须被拒: {illegal}"
        );
    }
    let state = workspace(&host, "nav-a2").unwrap();
    assert_eq!(
        serde_json::json!(state.url),
        before,
        "非法地址不得改变真实页面地址"
    );
    let snap = snapshot(&host, &session, &tab).await;
    assert!(snap["text"].as_str().unwrap().contains("PAGE-A-MARKER"));
    host.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

// ── BROWSER-TOOL ────────────────────────────────────────────────────────────

/// A1：导航后标题/快照匹配真实页面。
#[tokio::test]
async fn browser_tool_a1_title_and_snapshot_match_real_navigation() {
    let root = temp();
    let (host, site) = host(&root);
    let session = scope("tool-a1");
    let first = navigate(&host, &session, &format!("{}/a", site.url)).await;
    let tab = first.reply.state.active_tab_id.clone().unwrap();
    assert_eq!(first.reply.state.title.as_deref(), Some("R09 Page A"));
    let snap = snapshot(&host, &session, &tab).await;
    assert_eq!(snap["title"], "R09 Page A");
    assert_eq!(snap["url"], format!("{}/a", site.url));
    let text = snap["text"].as_str().unwrap();
    assert!(text.contains("PAGE-A-MARKER"));
    assert!(text.contains("alpha content"));
    assert!(text.contains("Name"), "表单元素应进入快照");
    let title = eval(&host, &session, &tab, "document.title").await;
    assert_eq!(title, "R09 Page A");
    let second = navigate(&host, &session, &format!("{}/b", site.url)).await;
    let snap = snapshot(&host, &session, &tab).await;
    assert_eq!(second.reply.state.title.as_deref(), Some("R09 Page B"));
    assert_eq!(snap["title"], "R09 Page B");
    assert!(snap["text"].as_str().unwrap().contains("PAGE-B-MARKER"));
    host.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// A2：过期快照引用明确失败（页面更换后旧 ref 不得复用）。
#[tokio::test]
async fn browser_tool_a2_stale_snapshot_reference_fails_explicitly() {
    let root = temp();
    let (host, site) = host(&root);
    let session = scope("tool-a2");
    let first = navigate(&host, &session, &format!("{}/a", site.url)).await;
    let tab = first.reply.state.active_tab_id.clone().unwrap();
    snapshot(&host, &session, &tab).await;
    let reference = eval(
        &host,
        &session,
        &tab,
        "Array.from(document.querySelectorAll('[data-hana-ref]')).slice(0,1).map(e=>e.getAttribute('data-hana-ref')).join('')",
    )
    .await;
    let reference: u32 = reference
        .as_str()
        .expect("快照必须给页面元素分配真实引用")
        .parse()
        .expect("引用必须为数字");
    // 页面更换（loader 变化）后，旧引用必须显式失败。
    navigate(&host, &session, &format!("{}/b", site.url)).await;
    let mut click = BrowserCommand::new(A::Click);
    click.tab_id = Some(tab.clone());
    click.element_ref = Some(reference);
    let error = execute_err(&host, &session, click).await;
    assert_eq!(error.code, "browser_stale_snapshot");
    let mut typed = BrowserCommand::new(A::Type);
    typed.tab_id = Some(tab.clone());
    typed.element_ref = Some(reference);
    typed.text = Some("SHOULD-NOT-ENTER".into());
    assert_eq!(
        execute_err(&host, &session, typed).await.code,
        "browser_stale_snapshot"
    );
    let body = eval(&host, &session, &tab, "document.body.innerText").await;
    assert!(!body.as_str().unwrap().contains("SHOULD-NOT-ENTER"));
    host.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// A3：会话 A 的状态/快照/Cookie 不泄露给会话 B（真实独立 Chromium 进程/profile）。
#[tokio::test]
async fn browser_tool_a3_session_state_and_cookies_never_leak_between_sessions() {
    let root = temp();
    let (host, site) = host(&root);
    let a = scope("tool-a3-a");
    let b = scope("tool-a3-b");
    let first = navigate(&host, &a, &format!("{}/login", site.url)).await;
    let tab_a = first.reply.state.active_tab_id.clone().unwrap();
    let second = navigate(&host, &b, &format!("{}/b", site.url)).await;
    let tab_b = second.reply.state.active_tab_id.clone().unwrap();
    assert_ne!(tab_a, tab_b);
    let workspace_a = host
        .workspaces()
        .unwrap()
        .into_iter()
        .find(|row| row.session_id == "tool-a3-a")
        .unwrap();
    let workspace_b = host
        .workspaces()
        .unwrap()
        .into_iter()
        .find(|row| row.session_id == "tool-a3-b")
        .unwrap();
    assert_ne!(workspace_a.tabs[0].tab_id, workspace_b.tabs[0].tab_id);
    assert!(workspace_a.url.as_deref().unwrap().contains("/login"));
    assert!(workspace_b.url.as_deref().unwrap().ends_with("/b"));
    let snap_a = snapshot(&host, &a, &tab_a).await;
    let snap_b = snapshot(&host, &b, &tab_b).await;
    let text_a = snap_a["text"].as_str().unwrap();
    let text_b = snap_b["text"].as_str().unwrap();
    assert!(text_a.contains("PAGE-LOGIN-MARKER"));
    assert!(
        !text_a.contains("PAGE-B-MARKER"),
        "会话 A 不得读到 B 的页面"
    );
    assert!(text_b.contains("PAGE-B-MARKER"));
    assert!(
        !text_b.contains("PAGE-LOGIN-MARKER"),
        "会话 B 不得读到 A 的页面"
    );
    let cookie_a = eval(&host, &a, &tab_a, "document.cookie").await;
    let cookie_b = eval(&host, &b, &tab_b, "document.cookie").await;
    assert!(cookie_a.as_str().unwrap().contains("login=fixture"));
    assert_eq!(cookie_b, "", "独立 profile 的会话 B 不得继承 A 的 Cookie");
    host.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// A4：无会话/浏览器错误显式失败，且不误操作另一会话。
#[tokio::test]
async fn browser_tool_a4_browser_errors_are_explicit_and_never_touch_another_session() {
    let root = temp();
    let (host, site) = host(&root);
    let a = scope("tool-a4-a");
    let b = scope("tool-a4-b");
    let first_b = navigate(&host, &b, &format!("{}/b", site.url)).await;
    let tab_b = first_b.reply.state.active_tab_id.clone().unwrap();
    let b_before = workspace(&host, "tool-a4-b").unwrap();
    let first_a = navigate(&host, &a, &format!("{}/a", site.url)).await;
    let tab_a = first_a.reply.state.active_tab_id.clone().unwrap();
    // 真实浏览器错误：不可达地址给出显式错误，不得返回成功或空内容。
    let mut broken = BrowserCommand::new(A::Navigate);
    broken.tab_id = Some(tab_a.clone());
    broken.url = Some("http://127.0.0.1:1/closed".into());
    let error = execute_err(&host, &a, broken).await;
    assert_eq!(error.code, "browser_navigation");
    assert!(!error.effect_unknown);
    // 无会话身份（不存在的 session）也必须显式失败。
    let missing = scope("tool-a4-missing");
    let missing_error = execute_err(&host, &missing, BrowserCommand::new(A::Snapshot)).await;
    assert!(
        matches!(
            missing_error.code.as_str(),
            "browser_scope" | "browser_empty_workspace" | "browser_not_running"
        ),
        "无会话必须显式失败: {missing_error:?}"
    );
    // 另一会话的真实状态不受影响。
    let b_after = workspace(&host, "tool-a4-b").unwrap();
    assert_state_unchanged(&b_before, &b_after);
    assert_eq!(b_after.active_tab_id.as_deref(), Some(tab_b.as_str()));
    let snap_b = snapshot(&host, &b, &tab_b).await;
    assert!(snap_b["text"].as_str().unwrap().contains("PAGE-B-MARKER"));
    host.shutdown().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
