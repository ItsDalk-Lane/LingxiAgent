//! 真实供应商流、认证服务、持久事件、重启和当前桌面组件的情绪轨迹证明。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
};

async fn read_request(socket: &mut TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut block = [0u8; 4096];
    loop {
        let n = socket.read(&mut block).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&block[..n]);
        if let Some(index) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let headers = String::from_utf8(bytes[..index].to_vec()).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= index + 4 + length {
                return (headers, bytes[index + 4..index + 4 + length].to_vec());
            }
        }
    }
}
fn frame(text: &str) -> String {
    format!(
        "data: {}\n\n",
        json!({"id":"mood", "choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]})
    )
}
struct Stub {
    addr: SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}
impl Stub {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let release = Arc::new(tokio::sync::Notify::new());
        let served_release = release.clone();
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut jobs = tokio::task::JoinSet::new();
            loop {
                let accepted =
                    tokio::select! { _=&mut stopped=>break, result=listener.accept()=>result };
                let (mut socket, _) = accepted.unwrap();
                let released = served_release.clone();
                jobs.spawn(async move {
                    let (headers, body) = read_request(&mut socket).await;
                    if headers.starts_with("POST /release ") {
                        released.notify_one();
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                        return;
                    }
                    let input: Value = serde_json::from_slice(&body).unwrap();
                    let gated = input.to_string().contains("LIVE_UI");
                    let empty = input.to_string().contains("NO_MOOD");
                    let many = input.to_string().contains("REVIEW_MANY_MOOD");
                    let many_text = (0..300).map(|n| format!("<mood>轨迹{n:03}</mood>")).collect::<String>();
                    let first = if many { frame(&many_text) } else { frame(if empty { "普通回复" } else { "<mo" }) };
                    let middle = if empty || many { String::new() } else { frame("od>清醒") };
                    let tail = if empty || many { String::new() } else { frame("\nWill: 保持克制</mood>正文") };
                    let final_frame = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",json!({"id":"mood","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),json!({"id":"mood","choices":[],"usage":{"prompt_tokens":12,"completion_tokens":8}}));
                    let length = first.len()+middle.len()+tail.len()+final_frame.len();
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n");
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket.write_all(first.as_bytes()).await.unwrap();
                    socket.write_all(middle.as_bytes()).await.unwrap();
                    if gated { released.notified().await; }
                    let _ = socket.write_all(tail.as_bytes()).await;
                    let _ = socket.write_all(final_frame.as_bytes()).await;
                });
            }
            jobs.abort_all();
            while jobs.join_next().await.is_some() {}
        });
        Self { addr, stop, task }
    }
    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap();
    }
}
struct Server {
    addr: SocketAddr,
    state: ServiceState,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}
async fn start(home: &Path, stub: &Stub) -> Server {
    let root = home.parent().unwrap();
    let product = root.join("product");
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(product.join("yuan")).unwrap();
    std::fs::write(
        product.join("yuan/hanako.md"),
        "你是协作助手。情绪不是正文。",
    )
    .unwrap();
    let layout = prepare_layout(home).unwrap();
    std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
    std::fs::write(
        home.join("agents/lingxi/config.yaml"),
        "locale: zh\nagent:\n  name: 灵犀\n  yuan: hanako\nmemory:\n  enabled: false\n",
    )
    .unwrap();
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&json!({"providers":{"stub":{"protocol":"openai-completions","endpoint":format!("http://{}/v1",stub.addr),"auth":{"kind":"apiKey","apiKey":"synthetic-mood-key"}}},"models":{"chat":{"provider":"stub","model":"mood-model","capabilities":{"tools":true},"compat":{"contextWindow":200000}}}}).to_string()).unwrap();
    let credential = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &root.join("credentials"),
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let compiler = Arc::new(
        lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
            lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                home,
                product.clone(),
                lingxi_service::context_compiler::HostFacts {
                    platform: "darwin".into(),
                    os_type: "Darwin".into(),
                    os_release: "test".into(),
                    shell_label: "zsh".into(),
                    workspace: workspace.to_string_lossy().into_owned(),
                },
            ),
        )),
    );
    let state = ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.to_path_buf(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        },
        &layout,
        ServiceDeps {
            product_material_dir: Some(product),
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            credential_service: Some(credential),
            network_plane: Some(lingxi_adapters::models::network::NetworkPlane::direct_isolated()),
            workspace_root: Some(workspace),
            context_compiler: Some(compiler),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap();
    let token = state.auth().local_token();
    let (stop, stop_rx) = tokio::sync::oneshot::channel();
    let (ready, ready_rx) = tokio::sync::oneshot::channel();
    let served = state.clone();
    let task = tokio::spawn(async move {
        run(
            served,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready.send(addr);
            },
            None,
        )
        .await
        .unwrap();
    });
    Server {
        addr: ready_rx.await.unwrap(),
        state,
        token,
        stop,
        task,
    }
}
impl Server {
    async fn request(&self, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
        Self::request_at(self.addr, method, path, token, body).await
    }
    async fn request_at(
        addr: SocketAddr,
        method: &str,
        path: &str,
        token: &str,
        body: Value,
    ) -> (u16, Value) {
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let body = body.to_string();
        let head=format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
        socket.write_all(head.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        if let Err(error) =
            tokio::time::timeout(Duration::from_secs(50), socket.read_to_end(&mut bytes))
                .await
                .unwrap()
        {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        }
        let cut = bytes.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
        let headers = String::from_utf8(bytes[..cut].to_vec()).unwrap();
        (
            headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
            serde_json::from_slice(&bytes[cut + 4..]).unwrap(),
        )
    }
    async fn get(&self, path: &str) -> Value {
        let (status, body) = self.request("GET", path, &self.token, Value::Null).await;
        assert_eq!(status, 200, "{body}");
        body
    }
    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap();
    }
}
fn home(tag: &str) -> PathBuf {
    std::env::temp_dir()
        .join(format!(
            "r06-mood-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
        .join("home")
}
fn moods(history: &Value) -> Vec<Value> {
    history["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "assistant_segment" && item["segmentKind"] == "mood")
        .cloned()
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn saved_mood_trajectory_survives_real_restart_and_empty_history_creates_no_event() {
    let home = home("restart");
    let stub = Stub::start().await;
    let server = start(&home, &stub).await;
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            &server.token,
            json!({"input":"保存情绪"}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let history = server
        .get("/lingxi/v1/sessions/sess_local_alpha/mood-history")
        .await;
    let saved = moods(&history);
    assert_eq!(saved.len(), 1, "{history}");
    assert_eq!(saved[0]["text"], "清醒\nWill: 保持克制");
    assert_eq!(saved[0]["complete"], true);
    let body_history = server
        .get("/lingxi/v1/sessions/sess_local_alpha/history?all=1")
        .await;
    let finals: Vec<_> = body_history["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "final_message")
        .collect();
    assert_eq!(finals.len(), 1, "真实正文完成记录必须存在");
    assert!(finals[0].to_string().contains("正文"));
    assert!(
        !body_history["items"].to_string().contains("清醒"),
        "情绪不能混进正文历史"
    );
    let denied = server
        .state
        .auth()
        .issue_device_credential("user_local", &[], None)
        .unwrap()
        .secret;
    assert!(matches!(
        server
            .request(
                "GET",
                "/lingxi/v1/sessions/sess_local_alpha/mood-history",
                &denied,
                Value::Null
            )
            .await
            .0,
        401 | 403
    ));
    let before = server
        .get("/lingxi/v1/sessions/sess_local_alpha/mood-events")
        .await;
    server.stop().await;
    let restored = start(&home, &stub).await;
    let history2 = restored
        .get("/lingxi/v1/sessions/sess_local_alpha/mood-history")
        .await;
    assert_eq!(moods(&history2), saved, "真实重启恢复同一份已保存轨迹");
    let after = restored
        .get("/lingxi/v1/sessions/sess_local_alpha/mood-events")
        .await;
    assert_eq!(before, after, "读取历史不能新造实时事件");
    assert!(moods(
        &restored
            .get("/lingxi/v1/sessions/sess_local_beta/mood-history")
            .await
    )
    .is_empty());
    let empty_before = restored
        .get("/lingxi/v1/sessions/sess_local_beta/mood-events")
        .await;
    let _ = restored
        .get("/lingxi/v1/sessions/sess_local_beta/mood-history")
        .await;
    assert_eq!(
        restored
            .get("/lingxi/v1/sessions/sess_local_beta/mood-events")
            .await,
        empty_before,
        "无轨迹读取无副作用"
    );
    restored.stop().await;
    stub.stop().await;
    std::fs::remove_dir_all(home.parent().unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn current_desktop_mood_real_ws_before_final_disconnect_and_history_empty_states() {
    let home = home("ui");
    let stub = Stub::start().await;
    let server = start(&home, &stub).await;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output_file = home.parent().unwrap().join("ui-result.json");
    let output = tokio::process::Command::new(
        std::env::var("LINGXI_NODE_BIN").unwrap_or_else(|_| "node".into()),
    )
    .arg(repo.join("node_modules/vitest/vitest.mjs"))
    .args([
        "run",
        "desktop/src/react/components/chat/__tests__/R06RustMoodLive.test.tsx",
        "-t",
        "^rust_mood_current_chat_real_service$",
        "--reporter=json",
        "--maxWorkers=1",
    ])
    .arg(format!("--outputFile={}", output_file.display()))
    .current_dir(&repo)
    .env("R06_MOOD_UI_BASE", format!("http://{}", server.addr))
    .env("R06_MOOD_UI_TOKEN", &server.token)
    .env(
        "R06_MOOD_UI_RELEASE",
        format!("http://{}/release", stub.addr),
    )
    .output()
    .await
    .unwrap();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        eprintln!("MOOD-UI-CHILD: {line}");
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        eprintln!("MOOD-UI-CHILD-ERR: {line}");
    }
    assert!(output.status.success(), "真实情绪界面检查失败");
    let proof: Value = serde_json::from_slice(&std::fs::read(&output_file).unwrap()).unwrap();
    assert_eq!(proof["success"], true);
    assert_eq!(proof["numTotalTests"], 1);
    assert_eq!(proof["numPassedTests"], 1);
    assert_eq!(proof["numFailedTests"], 0);
    assert_eq!(proof["numPendingTests"], 0);
    let history = server
        .get("/lingxi/v1/sessions/sess_local_alpha/mood-history")
        .await;
    assert_eq!(moods(&history).len(), 1);
    assert_eq!(moods(&history)[0]["complete"], true);
    assert!(moods(
        &server
            .get("/lingxi/v1/sessions/sess_local_beta/mood-history")
            .await
    )
    .is_empty());
    server.stop().await;
    stub.stop().await;
    std::fs::remove_dir_all(home.parent().unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupted_mood_is_partial_after_restart_and_foreign_principal_cannot_read() {
    let home = home("partial");
    let stub = Stub::start().await;
    let server = start(&home, &stub).await;
    let addr = server.addr;
    let token = server.token.clone();
    let drive = tokio::spawn(async move {
        Server::request_at(
            addr,
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            &token,
            json!({"input":"LIVE_UI"}),
        )
        .await
    });
    let (run_id, partial) = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let history = server
                .get("/lingxi/v1/sessions/sess_local_alpha/mood-history")
                .await;
            let saved = moods(&history);
            if let Some(item) = saved.first().filter(|item| item["text"] == "清醒") {
                break (
                    item["run"]["runId"].as_str().unwrap().to_string(),
                    item.clone(),
                );
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(partial["complete"], false, "没有真实闭合标签不能造完成轨迹");
    assert!(!drive.is_finished(), "情绪必须在回复结束前真实保存");
    let other = server
        .state
        .auth()
        .issue_device_credential("mood_other", &["chat"], None)
        .unwrap()
        .secret;
    for suffix in ["mood-history", "mood-events"] {
        let (status, body) = server
            .request(
                "GET",
                &format!("/lingxi/v1/sessions/sess_local_alpha/{suffix}"),
                &other,
                Value::Null,
            )
            .await;
        assert_eq!(status, 403, "跨主体读取拒绝: {body}");
        assert!(!body.to_string().contains("清醒"));
    }
    let stream = lingxi_adapters::storage::run_store::mood_stream_id("sess_local_alpha");
    let principal = server
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", server.token)),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    assert!(
        server
            .state
            .events()
            .subscribe(&principal, &stream, None)
            .await
            .is_err(),
        "不能借普通订阅绕过情绪实际会话授权"
    );
    server
        .state
        .runs()
        .cancel_run(&run_id, "验证未完成情绪的真实取消");
    let (status, body) = tokio::time::timeout(Duration::from_secs(15), drive)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, 200, "{body}");
    assert!(server
        .state
        .storage()
        .list_active_runs()
        .await
        .unwrap()
        .iter()
        .all(|run| run.run_id != run_id));
    let saved = moods(
        &server
            .get("/lingxi/v1/sessions/sess_local_alpha/mood-history")
            .await,
    );
    assert_eq!(saved, vec![partial]);
    server.stop().await;
    let restored = start(&home, &stub).await;
    assert_eq!(
        moods(
            &restored
                .get("/lingxi/v1/sessions/sess_local_alpha/mood-history")
                .await
        ),
        saved
    );
    let body_history = restored
        .get("/lingxi/v1/sessions/sess_local_alpha/history?all=1")
        .await;
    assert!(
        body_history["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["kind"] != "final_message"),
        "取消不能补造最终回复"
    );
    restored.stop().await;
    stub.stop().await;
    std::fs::remove_dir_all(home.parent().unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reviewer_mood_http_page_cursor_remains_bound_to_actual_mood_stream() {
    let home = home("review-page");
    let stub = Stub::start().await;
    let server = start(&home, &stub).await;
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            &server.token,
            json!({"input":"真实分页检查"}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let first = server
        .get("/lingxi/v1/sessions/sess_local_alpha/mood-events?limit=1")
        .await;
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    let cursor = first["nextCursor"]
        .as_str()
        .expect("真实流至少4个情绪事件需要续页");
    let (status, next) = server
        .request(
            "GET",
            &format!("/lingxi/v1/sessions/sess_local_alpha/mood-events?limit=1&cursor={cursor}"),
            &server.token,
            Value::Null,
        )
        .await;
    println!("review first mood page={first}; next status={status}; next={next}");
    assert_eq!(
        status, 200,
        "服务自己返回的下一页游标必须能用同一真实情绪接口续读"
    );
    assert_eq!(first["streamId"], next["streamId"]);
    server.stop().await;
    stub.stop().await;
    std::fs::remove_dir_all(home.parent().unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reviewer_current_mood_ui_restores_paginated_real_trajectory_without_cursor_failure() {
    let home = home("review-ui-page");
    let stub = Stub::start().await;
    let server = start(&home, &stub).await;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output_file = home.parent().unwrap().join("review-ui-result.json");
    let output = tokio::process::Command::new(
        std::env::var("LINGXI_NODE_BIN").unwrap_or_else(|_| "node".into()),
    )
    .arg(repo.join("node_modules/vitest/vitest.mjs"))
    .args([
        "run",
        "desktop/src/react/components/chat/__tests__/R06ReviewRustMoodLive.test.tsx",
        "-t",
        "^reviewer_mood_paginated_history_actual_network$",
        "--reporter=json",
        "--maxWorkers=1",
    ])
    .arg(format!("--outputFile={}", output_file.display()))
    .current_dir(&repo)
    .env("R06_MOOD_UI_BASE", format!("http://{}", server.addr))
    .env("R06_MOOD_UI_TOKEN", &server.token)
    .output()
    .await
    .unwrap();
    println!(
        "REVIEW-MOOD-UI stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!(
        "REVIEW-MOOD-UI result={}",
        std::fs::read_to_string(&output_file).unwrap()
    );
    assert!(
        output.status.success(),
        "当前生产情绪组件应恢复分页的真实轨迹"
    );
    let proof: Value = serde_json::from_slice(&std::fs::read(&output_file).unwrap()).unwrap();
    assert_eq!(proof["numTotalTests"], 1);
    assert_eq!(proof["numPassedTests"], 1);
    assert_eq!(proof["numPendingTests"], 0);
    server.stop().await;
    stub.stop().await;
    std::fs::remove_dir_all(home.parent().unwrap()).unwrap();
}
