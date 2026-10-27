//! Slash 五叶：真实 HTTP/WS、统一派发器、ModelGateway 和持久历史。
//! 仅外部供应商使用真实 loopback 替身。
use lingxi_kernel::ports::StoragePort;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r06-ex01-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));

    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn config_for(home: &Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Env,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

struct ApiClient {
    addr: SocketAddr,
    token: String,
}
struct TestServer {
    api: ApiClient,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn serve(state: ServiceState) -> TestServer {
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        api: ApiClient { addr, token },
        stop: stop_tx,
        handle,
    }
}

impl std::ops::Deref for TestServer {
    type Target = ApiClient;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl ApiClient {
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let (status, _, body) = self.request_raw(method, path, body).await;
        if std::env::var("R06T07_DEBUG").is_ok() {
            eprintln!(
                "DEBUG {method} {path} -> {status}: {}",
                String::from_utf8_lossy(&body)
            );
        }
        let parsed = if body.is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(_) => {
                    eprintln!(
                        "DEBUG non-JSON body for {method} {path}: {}",
                        String::from_utf8_lossy(&body)
                    );
                    Value::Null
                }
            }
        };
        (status, parsed)
    }

    async fn request_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            self.token,
            payload.len()
        );
        let mut stream = TcpStream::connect(self.addr).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut buf)).await;
        let header_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("http header end");
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let headers: Vec<(String, String)> = head
            .split("\r\n")
            .skip(1)
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            })
            .collect();
        (status, headers, buf[header_end + 4..].to_vec())
    }
}

impl TestServer {
    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
    }
}

struct Remote {
    addr: SocketAddr,
    wire: Arc<Mutex<Vec<Value>>>,
    fail: Arc<AtomicBool>,
    gate: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    task: tokio::task::JoinHandle<()>,
}
fn summary() -> &'static str {
    "## Goal\nEX01-SUMMARY\n## Constraints & Preferences\n保留结果\n## Progress\n### Done\n旧工作\n### In Progress\n继续\n### Blocked\n无\n## Key Decisions\n真实来源\n## Next Steps\n下一轮\n## Critical Context\nEX01-SUMMARY"
}
impl Remote {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let wire = Arc::new(Mutex::new(Vec::new()));
        let fail = Arc::new(AtomicBool::new(false));
        let gate = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let number = Arc::new(AtomicUsize::new(0));
        let (w, f, g, e, r) = (
            wire.clone(),
            fail.clone(),
            gate.clone(),
            entered.clone(),
            release.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (w, f, g, e, r, n) = (
                    w.clone(),
                    f.clone(),
                    g.clone(),
                    e.clone(),
                    r.clone(),
                    number.clone(),
                );
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let (head, len) = loop {
                        let mut buf = [0u8; 65536];
                        let count = socket.read(&mut buf).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..count]);
                        if let Some(pos) = raw.windows(4).position(|v| v == b"\r\n\r\n") {
                            let text = String::from_utf8_lossy(&raw[..pos]);
                            let len = text
                                .lines()
                                .find_map(|line| {
                                    line.split_once(':')
                                        .filter(|(key, _)| {
                                            key.eq_ignore_ascii_case("content-length")
                                        })
                                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            break (pos + 4, len);
                        }
                    };
                    while raw.len() < head + len {
                        let mut buf = [0u8; 65536];
                        let count = socket.read(&mut buf).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..count]);
                    }
                    let body: Value = serde_json::from_slice(&raw[head..head + len]).unwrap();
                    w.lock().unwrap().push(body.clone());
                    let is_summary = body["model"] == "summary";
                    if g.load(Ordering::SeqCst) {
                        e.add_permits(1);
                        r.acquire().await.unwrap().forget();
                    }
                    if is_summary && f.load(Ordering::SeqCst) {
                        let payload =
                            json!({"error":{"message":"EX01_SYNTHETIC_PROVIDER_FAILURE"}})
                                .to_string();
                        let response=format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                        let _ = socket.write_all(response.as_bytes()).await;
                        return;
                    }
                    let text = if is_summary {
                        summary().to_string()
                    } else {
                        let index = n.fetch_add(1, Ordering::SeqCst);
                        if body["messages"].to_string().contains("EX01-SUMMARY") {
                            "EX01-NEXT-ACTUAL-RUN".to_string()
                        } else {
                            format!("OLD-TURN-{index} ").repeat(3500)
                        }
                    };
                    let mut payload = String::new();
                    for frame in [
                        json!({"id":"remote-ex01","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
                        json!({"id":"remote-ex01","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                        json!({"id":"remote-ex01","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":10}}),
                    ] {
                        payload.push_str(&format!("data: {frame}\n\n"));
                    }
                    payload.push_str("data: [DONE]\n\n");
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self {
            addr,
            wire,
            fail,
            gate,
            entered,
            release,
            task,
        }
    }
    fn captured(&self) -> Vec<Value> {
        self.wire.lock().unwrap().clone()
    }
    async fn parked(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    fn stop(self) {
        self.release.add_permits(20);
        self.task.abort();
    }
}
struct Harness {
    server: TestServer,
    state: ServiceState,
    home: PathBuf,
}
fn fixture(home: &Path) {
    let agent = home.join("agents/lingxi");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("config.yaml"),"locale: zh-CN\nagent:\n  name: EX01-OLD-NAME\n  yuan: lingxi\nmemory:\n  enabled: false\ncompaction:\n  enabled: false\nunknown_future:\n  keep: EX01-UNKNOWN-PRESERVED\n").unwrap();
}
async fn boot(home: PathBuf, remote: &Remote, summary_window: u64) -> Harness {
    let layout = prepare_layout(&home).unwrap();
    let config = json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":format!("http://{}/v1",remote.addr),"auth":{"kind":"apiKey","apiKey":"EX01_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"summarize":{"provider":"remote","model":"summary","capabilities":{"tools":true},"compat":{"contextWindow":summary_window}}}});
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::from_value(config).unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let product = home.join("product");
    std::fs::create_dir_all(product.join("yuan")).unwrap();
    let template = product.join("yuan/lingxi.md");
    if !template.exists() {
        std::fs::write(&template, "你叫{{agentName}}，与用户一起完成任务。").unwrap();
    }
    let compiler = Arc::new(
        lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
            lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                &home,
                product,
                lingxi_service::context_compiler::HostFacts {
                    platform: std::env::consts::OS.into(),
                    os_type: std::env::consts::OS.into(),
                    os_release: String::from_utf8(
                        std::process::Command::new("uname")
                            .arg("-r")
                            .output()
                            .unwrap()
                            .stdout,
                    )
                    .unwrap()
                    .trim()
                    .into(),
                    shell_label: "zsh".into(),
                    workspace: home.to_string_lossy().into_owned(),
                },
            ),
        )),
    );
    let state = ServiceState::bootstrap_with_deps(
        config_for(&home),
        &layout,
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            credential_service: Some(credentials),
            context_compiler: Some(compiler),
            workspace_root: Some(home.clone()),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap();
    let server = serve(state.clone()).await;
    Harness {
        server,
        state,
        home,
    }
}
impl Harness {
    async fn create(&self, id: &str) {
        let (code, value) = self
            .server
            .request(
                "POST",
                "/lingxi/v1/sessions",
                Some(json!({"sessionId":id,"agentId":"lingxi","title":id})),
            )
            .await;
        assert_eq!(code, 201, "{value}");
    }
    async fn execute(&self, id: &str, input: &str) -> String {
        let (code, value) = self
            .server
            .request(
                "POST",
                &format!("/lingxi/v1/sessions/{id}/execute"),
                Some(json!({"input":input})),
            )
            .await;
        assert_eq!(code, 200, "{value}");
        let run = value["runId"].as_str().unwrap().to_string();
        let status = self
            .state
            .storage()
            .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run.clone()])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, "completed", "{value}");
        run
    }
    async fn fill(&self, id: &str) {
        self.create(id).await;
        for index in 0..4 {
            self.execute(id, &format!("OLD-USER-{index}")).await;
        }
    }
    async fn settings(&self, value: Value) {
        let (code, reply) = self
            .server
            .request("PUT", "/lingxi/v1/agents/lingxi/config", Some(value))
            .await;
        assert_eq!(code, 200, "{reply}");
    }
    async fn snapshot(&self, id: &str) -> String {
        self.state.storage().query_one_text("SELECT json_object('messages',(SELECT group_concat(message_id||':'||content_json,'|') FROM (SELECT * FROM messages WHERE session_id=?1 ORDER BY seq)),'head',(SELECT head_message_id FROM session_branch_heads WHERE session_id=?1),'revision',(SELECT revision FROM session_branch_heads WHERE session_id=?1),'runs',(SELECT COUNT(*) FROM runs WHERE session_id=?1))",vec![id.into()]).await.unwrap().unwrap()
    }
    async fn archive(&self, id: &str) -> Value {
        let text=self.state.storage().query_one_text("SELECT content_json FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%' ORDER BY seq DESC LIMIT 1",vec![id.into()]).await.unwrap().unwrap();
        serde_json::from_str(&text).unwrap()
    }
    async fn load(&self, id: &str) -> Vec<lingxi_kernel::model_exchange::ExchangeItem> {
        lingxi_service::model_history::ModelHistory::new(self.state.storage().clone())
            .load(&lingxi_kernel::Principal::LocalUser, "lingxi", id, "")
            .await
            .unwrap()
    }
    async fn stop(self) -> PathBuf {
        self.server.stop().await;
        self.home
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slash_unknown_uses_real_dispatcher_and_explicit_result() {
    let remote = Remote::start().await;
    let home = unique_dir("slash-unknown");
    fixture(&home);
    let h = boot(home, &remote, 200000).await;
    h.create("desktop").await;
    let (status, reply) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/desktop/slash",
            Some(json!({"text":"/unknown-command"})),
        )
        .await;
    assert_eq!(
        status, 200,
        "production slash dispatcher must exist: {reply}"
    );
    assert_eq!(reply["results"][0]["type"], "slash_result");
    assert_eq!(reply["results"][0]["level"], "error");
    assert!(reply["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("未知命令"));
    assert!(remote.wire.lock().unwrap().is_empty());
    h.stop().await;
    remote.stop();
}

async fn slash(h: &Harness, id: &str, text: &str) -> Value {
    let (code, result) = h
        .server
        .request(
            "POST",
            &format!("/lingxi/v1/sessions/{id}/slash"),
            Some(json!({"text":text})),
        )
        .await;
    assert_eq!(code, 200, "{result}");
    result
}
async fn register(h: &Harness, id: &str) {
    let (code, result) = h
        .server
        .request(
            "POST",
            &format!("/lingxi/v1/sessions/{id}/bridge"),
            Some(json!({"channelKey":format!("synthetic-platform:{id}")})),
        )
        .await;
    assert_eq!(code, 201, "{result}");
}
async fn attach(h: &Harness, bridge: &str, desktop: &str) {
    let (code, result) = h
        .server
        .request(
            "POST",
            &format!("/lingxi/v1/sessions/{bridge}/bridge/attachment"),
            Some(json!({"desktopSessionId":desktop})),
        )
        .await;
    assert_eq!(code, 200, "{result}");
}
async fn ws_send(stream: &mut TcpStream, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    let mask = [1u8, 2, 3, 4];
    let mut frame = vec![0x81];
    if bytes.len() < 126 {
        frame.push(0x80 | bytes.len() as u8);
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(bytes.iter().enumerate().map(|(i, v)| v ^ mask[i % 4]));
    stream.write_all(&frame).await.unwrap();
}
async fn ws_json(stream: &mut TcpStream) -> Value {
    match tokio::time::timeout(
        Duration::from_secs(10),
        lingxi_service::ws::read_ws_frame(stream),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap()
    {
        lingxi_service::ws::WsFrame::Text(bytes) => serde_json::from_slice(&bytes).unwrap(),
        other => panic!("unexpected frame {other:?}"),
    }
}
async fn ws_connect(h: &Harness) -> TcpStream {
    let mut stream = TcpStream::connect(h.server.addr).await.unwrap();
    let request=format!("GET /lingxi/v1/ws HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {}\r\n\r\n",h.server.addr,h.server.token);
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut b = [0];
        stream.read_exact(&mut b).await.unwrap();
        head.push(b[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 101"));
    ws_send(&mut stream,json!({"protocol":"lingxi.wire","clientKind":"test","clientVersion":"0","protocolMin":1,"protocolMax":1})).await;
    assert_eq!(ws_json(&mut stream).await["protocol"], "lingxi.wire");
    stream
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_compact_reports_progress_before_real_paid_call_then_commits_only_target() {
    let home = unique_dir("slash-ws");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("desktop").await;
    h.fill("bridge").await;
    h.fill("unrelated").await;
    register(&h, "bridge").await;
    attach(&h, "bridge", "desktop").await;
    let bridge_before = h.snapshot("bridge").await;
    let unrelated_before = h.snapshot("unrelated").await;
    let desktop_before = h.snapshot("desktop").await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    remote.gate.store(true, Ordering::SeqCst);
    let mut ws = ws_connect(&h).await;
    ws_send(&mut ws,json!({"type":"slash","sessionId":"bridge","text":"/compact precise-focus","agentId":"lingxi"})).await;
    let progress = ws_json(&mut ws).await;
    assert_eq!(progress["type"], "slash_result");
    assert_eq!(progress["sessionId"], "bridge");
    assert_eq!(progress["targetSessionId"], "desktop");
    assert!(progress["text"].as_str().unwrap().contains("正在压缩"));
    remote.parked().await;
    assert_eq!(
        h.snapshot("desktop").await,
        desktop_before,
        "waiting model cannot mutate history"
    );
    remote.release.add_permits(1);
    let done = ws_json(&mut ws).await;
    assert_eq!(done["level"], "success");
    assert!(done["text"].as_str().unwrap().contains("tokens"));
    assert_ne!(h.snapshot("desktop").await, desktop_before);
    assert_eq!(h.snapshot("bridge").await, bridge_before);
    assert_eq!(h.snapshot("unrelated").await, unrelated_before);
    assert!(
        h.archive("desktop").await["refreshedContext"]["request"]["customInstructions"]
            .as_str()
            .unwrap()
            .contains("precise-focus")
    );
    let calls = remote.captured();
    let summary = calls
        .iter()
        .filter(|c| c["model"] == "summary")
        .collect::<Vec<_>>();
    assert_eq!(summary.len(), 1);
    assert!(summary[0]["messages"].to_string().contains("precise-focus"));
    let usage = h
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
            session_id: Some("desktop".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let aux: Vec<_> = usage
        .iter()
        .filter(|u| u.origin == "session.manual_compact")
        .collect();
    assert_eq!(aux.len(), 1);
    assert_eq!(aux[0].outcome, lingxi_kernel::usage::CallOutcome::Succeeded);
    drop(ws);
    let home = h.stop().await;
    let h = boot(home, &remote, 1000000).await;
    assert!(format!("{:?}", h.load("desktop").await).contains("EX01-SUMMARY"));
    // Bridge 身份从真实索引恢复，接管关系按原约定重启清空。
    let done = slash(&h, "bridge", "/reset").await;
    assert!(done["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("历史已清除"));
    assert_eq!(h.snapshot("unrelated").await, unrelated_before);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_refreshes_real_prompt_memory_and_tools_then_failure_preserves_original_history() {
    let home = unique_dir("slash-fresh");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("fresh").await;
    h.settings(json!({"agent":{"name":"SLASH-FRESH-CURRENT"},"memory":{"enabled":true},"compaction":{"keepRecentTokens":1}})).await;
    std::fs::create_dir_all(h.home.join("agents/lingxi/memory")).unwrap();
    std::fs::write(
        h.home.join("agents/lingxi/identity.md"),
        "SLASH-NEW-IDENTITY",
    )
    .unwrap();
    std::fs::write(
        h.home.join("agents/lingxi/memory/memory.md"),
        "SLASH-NEW-COMPILED-MEMORY",
    )
    .unwrap();
    let result = slash(&h, "fresh", "/fresh-compact").await;
    assert_eq!(result["results"].as_array().unwrap().len(), 2);
    assert!(result["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("刷新提示词与记忆"));
    assert_eq!(result["results"][1]["level"], "success");
    let archive = h.archive("fresh").await;
    let prompt = archive["refreshedContext"]["render"].as_str().unwrap();
    assert!(prompt.contains("SLASH-FRESH-CURRENT"));
    assert!(prompt.contains("SLASH-NEW-IDENTITY"));
    assert!(prompt.contains("SLASH-NEW-COMPILED-MEMORY"));
    assert_eq!(archive["refreshedContext"]["fresh"], true);
    let home = h.stop().await;
    let h = boot(home, &remote, 1000000).await;
    h.execute("fresh", "SLASH-NEXT-CONSUMER").await;
    let call = remote.captured().last().unwrap().clone();
    assert!(call["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("SLASH-NEW-COMPILED-MEMORY"));
    assert!(call["messages"].to_string().contains("EX01-SUMMARY"));
    for tool in archive["refreshedContext"]["tools"]["declarations"]
        .as_array()
        .unwrap()
    {
        assert!(call["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == tool["wireName"]));
    }
    h.fill("failure").await;
    let before = h.snapshot("failure").await;
    remote.fail.store(true, Ordering::SeqCst);
    let result = slash(&h, "failure", "/fresh-compact").await;
    assert_eq!(result["results"][1]["level"], "error");
    assert!(result["results"][1]["text"]
        .as_str()
        .unwrap()
        .contains("fresh-compact 失败"));
    assert_eq!(h.snapshot("failure").await, before);
    let usage = h
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
            session_id: Some("failure".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        usage
            .iter()
            .filter(|u| u.origin == "session.fresh_compact"
                && u.outcome == lingxi_kernel::usage::CallOutcome::Failed)
            .count(),
        1
    );
    assert!(
        std::fs::read_to_string(h.home.join("agents/lingxi/config.yaml"))
            .unwrap()
            .contains("EX01-UNKNOWN-PRESERVED")
    );
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attached_reset_and_fresh_refuse_and_compact_failure_preserves_all_sources() {
    let home = unique_dir("slash-refusals");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("desktop").await;
    h.fill("bridge").await;
    h.fill("other").await;
    register(&h, "bridge").await;
    attach(&h, "bridge", "desktop").await;
    let original = [
        h.snapshot("desktop").await,
        h.snapshot("bridge").await,
        h.snapshot("other").await,
    ];
    let call_count = remote.captured().len();
    for command in ["/reset", "/fresh-compact"] {
        let result = slash(&h, "bridge", command).await;
        assert_eq!(result["results"][0]["level"], "error");
        assert!(result["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("接管桌面会话期间"));
    }
    assert_eq!(remote.captured().len(), call_count);
    remote.fail.store(true, Ordering::SeqCst);
    let result = slash(&h, "bridge", "/compact").await;
    assert_eq!(result["results"][1]["level"], "error");
    assert!(result["results"][1]["text"]
        .as_str()
        .unwrap()
        .contains("压缩失败"));
    for (i, id) in ["desktop", "bridge", "other"].iter().enumerate() {
        assert_eq!(h.snapshot(id).await, original[i]);
    }
    let (code, _) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/missing/slash",
            Some(json!({"text":"/reset"})),
        )
        .await;
    assert_eq!(code, 404);
    assert_eq!(h.snapshot("other").await, original[2]);
    let result = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/desktop/slash",
            Some(json!({"text":"/reset","agentId":"someone-else"})),
        )
        .await;
    assert_eq!(result.0, 200);
    assert_eq!(result.1["results"][0]["level"], "error");
    assert_eq!(h.snapshot("desktop").await, original[0]);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_physically_clears_only_current_history_preserves_charges_and_next_run_restart() {
    let home = unique_dir("slash-reset");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("current").await;
    h.fill("other").await;
    register(&h, "current").await;
    let other = h.snapshot("other").await;
    let charges = h
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    let before_rev = h
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COALESCE(MAX(seq),0) AS TEXT) FROM key_events WHERE stream_id=?1",
            vec!["current".into()],
        )
        .await
        .unwrap()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let result = slash(&h, "current", "/reset").await;
    assert_eq!(result["results"][0]["level"], "success");
    assert!(result["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("历史已清除"));
    assert!(h.load("current").await.is_empty());
    assert_eq!(h.snapshot("other").await, other);
    assert_eq!(
        h.state
            .storage()
            .query_model_call_usage(Default::default())
            .await
            .unwrap(),
        charges
    );
    for table in ["messages", "run_history_anchors", "checkpoints"] {
        assert_eq!(
            h.state
                .storage()
                .query_one_text(
                    &format!("SELECT CAST(COUNT(*) AS TEXT) FROM {table} WHERE session_id=?1"),
                    vec!["current".into()]
                )
                .await
                .unwrap()
                .unwrap(),
            "0"
        );
    }
    let events=h.state.storage().query_one_text("SELECT json_group_array(json_object('type',event_type,'seq',seq,'body',payload_json)) FROM key_events WHERE stream_id=?1",vec!["current".into()]).await.unwrap().unwrap();
    let events: Value = serde_json::from_str(&events).unwrap();
    assert_eq!(events.as_array().unwrap().len(), 1);
    assert_eq!(events[0]["type"], "session_history_cleared");
    assert!(events[0]["seq"].as_u64().unwrap() > before_rev);
    assert!(!events.to_string().contains("OLD-USER"));
    let home = h.stop().await;
    let h = boot(home, &remote, 1000000).await;
    assert!(h.load("current").await.is_empty());
    h.execute("current", "SLASH-CLEAN-NEXT").await;
    let sent = remote.captured().last().unwrap().clone();
    assert!(!sent["messages"].to_string().contains("OLD-USER"));
    assert!(sent["messages"].to_string().contains("SLASH-CLEAN-NEXT"));
    assert_eq!(h.snapshot("other").await, other);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_takeover_cancels_real_desktop_only_busy_reset_refuses_and_idle_is_explicit() {
    let home = unique_dir("slash-stop");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    for id in ["desktop", "bridge", "other"] {
        h.create(id).await;
    }
    register(&h, "bridge").await;
    attach(&h, "bridge", "desktop").await;
    assert!(slash(&h, "other", "/stop").await["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("当前无活动回复"));
    remote.gate.store(true, Ordering::SeqCst);
    let mut jobs = Vec::new();
    for id in ["desktop", "bridge", "other"] {
        let api = ApiClient {
            addr: h.server.addr,
            token: h.server.token.clone(),
        };
        let id = id.to_string();
        jobs.push(tokio::spawn(async move {
            api.request(
                "POST",
                &format!("/lingxi/v1/sessions/{id}/execute"),
                Some(json!({"input":format!("ACTIVE-{id}")})),
            )
            .await
        }));
        remote.parked().await;
    }
    let live = h.state.storage().list_active_runs().await.unwrap();
    assert_eq!(live.len(), 3);
    let target = live
        .iter()
        .find(|r| r.session_id == "desktop")
        .unwrap()
        .run_id
        .clone();
    let reset = slash(&h, "other", "/reset").await;
    assert_eq!(reset["results"][0]["level"], "error");
    assert!(reset["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("session_busy"));
    let stop = slash(&h, "bridge", "/stop").await;
    assert_eq!(stop["results"][0]["targetSessionId"], "desktop");
    assert_eq!(stop["results"][0]["level"], "success");
    assert!(stop["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("正在停止"));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = h
                .state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id=?1",
                    vec![target.clone()],
                )
                .await
                .unwrap()
                .unwrap();
            if status == "cancelled" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    for row in live.iter().filter(|r| r.session_id != "desktop") {
        assert_eq!(
            h.state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id=?1",
                    vec![row.run_id.clone()]
                )
                .await
                .unwrap()
                .unwrap(),
            "running"
        );
    }
    assert!(slash(&h, "bridge", "/stop").await["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("当前无活动回复"));
    remote.gate.store(false, Ordering::SeqCst);
    remote.release.add_permits(3);
    for job in jobs {
        assert_eq!(job.await.unwrap().0, 200);
    }
    assert_eq!(h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id=?1 AND role='assistant'",vec!["desktop".into()]).await.unwrap().unwrap(),"0","late provider response must not become cancelled reply");
    for row in live.iter().filter(|r| r.session_id != "desktop") {
        assert_eq!(
            h.state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id=?1",
                    vec![row.run_id.clone()]
                )
                .await
                .unwrap()
                .unwrap(),
            "completed"
        );
    }
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_agent_real_unready_and_foreign_bridge_target_are_refused_without_any_mutation() {
    let home = unique_dir("slash-auth");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("local").await;
    h.create("bridge").await;
    register(&h, "bridge").await;
    let (code, token) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/devices/credentials",
            Some(json!({"userId":"foreign","scopes":["chat"]})),
        )
        .await;
    assert_eq!(code, 201);
    let foreign = ApiClient {
        addr: h.server.addr,
        token: token["secret"].as_str().unwrap().into(),
    };
    let (code, result) = foreign
        .request(
            "POST",
            "/lingxi/v1/sessions",
            Some(json!({"sessionId":"foreign-desktop","agentId":"lingxi","title":"foreign"})),
        )
        .await;
    assert_eq!(code, 201, "{result}");
    let original = h.snapshot("local").await;
    let foreign_before = h.snapshot("foreign-desktop").await;
    let (code, _) = foreign
        .request(
            "POST",
            "/lingxi/v1/sessions/local/slash",
            Some(json!({"text":"/reset"})),
        )
        .await;
    assert_eq!(code, 403);
    let (code, _) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/bridge/bridge/attachment",
            Some(json!({"desktopSessionId":"foreign-desktop"})),
        )
        .await;
    assert_eq!(code, 409);
    assert_eq!(h.snapshot("local").await, original);
    assert_eq!(h.snapshot("foreign-desktop").await, foreign_before);
    let calls = remote.captured().len();
    std::fs::rename(
        h.home.join("agents/lingxi"),
        h.home.join("agents/lingxi-removed"),
    )
    .unwrap();
    let (code, result) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/local/slash",
            Some(json!({"text":"/compact"})),
        )
        .await;
    assert!((400..500).contains(&code), "{result}");
    assert_eq!(remote.captured().len(), calls);
    assert_eq!(h.snapshot("local").await, original);
    h.stop().await;
    remote.stop();
    let home = unique_dir("slash-real-unready");
    fixture(&home);
    let layout = prepare_layout(&home).unwrap();
    let state = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .unwrap();
    let server = serve(state).await;
    assert_eq!(
        server
            .request(
                "POST",
                "/lingxi/v1/sessions",
                Some(json!({"sessionId":"not-ready","agentId":"lingxi","title":"not-ready"}))
            )
            .await
            .0,
        201
    );
    let (code, result) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/not-ready/slash",
            Some(json!({"text":"/compact"})),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(result["results"][0]["level"], "error");
    assert!(result["results"][0]["text"]
        .as_str()
        .unwrap()
        .contains("未就绪"));
    server.stop().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_slash_production_input_real_ws_and_inline_result() {
    let home = unique_dir("slash-ui");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("desktop").await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let url = format!("http://{}", h.server.addr);
    let token = h.server.token.clone();
    let ui_home = h.home.clone();
    let mut child = tokio::task::spawn_blocking(move || {
        std::process::Command::new(std::env::var("R06_NODE_BIN").unwrap_or_else(|_| "node".into()))
            .arg(root.join("desktop/src/react/__tests__/slash-live/launch.mjs"))
            .env("R06_SLASH_HOME", ui_home)
            .env("R06_SLASH_URL", url)
            .env("R06_SLASH_TOKEN", token)
            .output()
            .unwrap()
    });
    tokio::select! {
        result = &mut child => panic!("actual UI exited before compaction observation: {:?}", result),
        result = tokio::time::timeout(Duration::from_secs(30), async { while !h.home.join("slash-ui-compacted").exists() { tokio::time::sleep(Duration::from_millis(20)).await; } }) => { result.expect("actual UI compaction checkpoint"); }
    }
    let model_history = format!("{:?}", h.load("desktop").await);
    assert!(
        model_history.contains("EX01-SUMMARY"),
        "actual model exchange uses the committed summary"
    );
    std::fs::write(h.home.join("slash-ui-model-observed"), &model_history).unwrap();
    let output = child.await.unwrap();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        println!("SLASH-UI: {line}");
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        println!("SLASH-UI-ERR: {line}");
    }
    assert!(
        output.status.success(),
        "actual Electron failed: {}",
        std::fs::read_to_string(h.home.join("slash-ui-failure.txt")).unwrap_or_default()
    );
    let proof: Value =
        serde_json::from_slice(&std::fs::read(h.home.join("slash-ui-result.json")).unwrap())
            .unwrap();
    assert_eq!(proof["checks"].as_array().unwrap().len(), 8);
    assert!(proof["sent"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["type"] == "slash")
        .all(|v| v["sessionId"] == "desktop"));
    assert!(proof["compactHistory"].to_string().contains("OLD-USER"));
    assert!(!proof["compactHistory"]
        .to_string()
        .contains("lingxi-model-exchange-v1"));
    assert!(
        h.load("desktop").await.is_empty(),
        "actual UI reset cleared durable model history"
    );
    h.stop().await;
    remote.stop();
}
