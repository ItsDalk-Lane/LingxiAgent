//! 聊天传输：真实 HTTP/WS、现役 Run、ModelGateway 和持久历史。
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
                    if body["messages"]
                        .as_array()
                        .and_then(|rows| rows.iter().rfind(|row| row["role"] == "user"))
                        .is_some_and(|row| row["content"].to_string().contains("LIVE-DELTA"))
                    {
                        let first = format!(
                            "data: {}\n\n",
                            json!({"id":"live","choices":[{"index":0,"delta":{"role":"assistant","content":"LIVE-FIRST"},"finish_reason":null}]})
                        );
                        let tail = format!(
                            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                            json!({"id":"live","choices":[{"index":0,"delta":{"content":"-FINAL"},"finish_reason":"stop"}]}),
                            json!({"id":"live","choices":[],"usage":{"prompt_tokens":12,"completion_tokens":9}})
                        );
                        let head=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",first.len()+tail.len());
                        let _ = socket.write_all(head.as_bytes()).await;
                        let _ = socket.write_all(first.as_bytes()).await;
                        e.add_permits(1);
                        r.acquire().await.unwrap().forget();
                        let _ = socket.write_all(tail.as_bytes()).await;
                        return;
                    }
                    if body["messages"]
                        .as_array()
                        .and_then(|rows| rows.iter().rfind(|row| row["role"] == "user"))
                        .is_some_and(|row| row["content"].to_string().contains("REPLAY-MANY"))
                    {
                        let mut payload = String::new();
                        for _ in 0..4200 {
                            payload.push_str(&format!("data: {}\n\n",json!({"id":"many","choices":[{"index":0,"delta":{"content":"x"},"finish_reason":null}]})));
                        }
                        payload.push_str(&format!("data: {}\n\ndata: [DONE]\n\n",json!({"id":"many","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})));
                        let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                        let _ = socket.write_all(response.as_bytes()).await;
                        return;
                    }
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
                    let tool_prompt = body["messages"].to_string().contains("CHAT-TOOL");
                    let tool_count = body["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|m| m["role"] == "tool")
                        .count();
                    if tool_prompt && tool_count < 2 {
                        let (name, args, ident) = if tool_count == 0 {
                            ("mcp_describe_tool", json!({"name":"read"}), "chat-describe")
                        } else {
                            (
                                "mcp_call",
                                json!({"tool":"read","arguments":{"path":"chat-real.txt"}}),
                                "chat-read",
                            )
                        };
                        let frame = json!({"id":"chat-tool","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":ident,"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]});
                        let payload = format!(
                            "data: {frame}\n\ndata: {}\n\ndata: [DONE]\n\n",
                            json!({"id":"chat-tool","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})
                        );
                        let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
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
                            format!("CHAT-FINAL-{index}")
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
    boot_with_quota(
        home,
        remote,
        summary_window,
        lingxi_service::quotas::QuotaLimits::default(),
    )
    .await
}
async fn boot_with_quota(
    home: PathBuf,
    remote: &Remote,
    summary_window: u64,
    quota_limits: lingxi_service::quotas::QuotaLimits,
) -> Harness {
    let layout = prepare_layout(&home).unwrap();
    let config = json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":format!("http://{}/v1",remote.addr),"auth":{"kind":"apiKey","apiKey":"EX01_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true,"imageInput":true},"compat":{"contextWindow":200000,"reasoning":true}},"title":{"provider":"remote","model":"second","capabilities":{"tools":true,"imageInput":true},"compat":{"contextWindow":200000,"reasoning":true}},"summarize":{"provider":"remote","model":"summary","capabilities":{"tools":true},"compat":{"contextWindow":summary_window}}}});
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
            quota_limits,
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

async fn ws_send(stream: &mut TcpStream, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    let mask = [1u8, 2, 3, 4];
    let mut frame = vec![0x81];
    if bytes.len() < 126 {
        frame.push(0x80 | bytes.len() as u8);
    } else if bytes.len() <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
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
        lingxi_service::ws::WsFrame::Text(bytes) => {
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            if std::env::var("R06T07_DEBUG").is_ok() {
                eprintln!(
                    "WS {}",
                    value.to_string().chars().take(600).collect::<String>()
                );
            }
            value
        }
        other => panic!("unexpected frame {other:?}"),
    }
}
async fn ws_connect(h: &Harness) -> TcpStream {
    ws_connect_api(&h.server).await
}
async fn ws_connect_api(api: &ApiClient) -> TcpStream {
    let mut stream = TcpStream::connect(api.addr).await.unwrap();
    let request=format!("GET /lingxi/v1/ws HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {}\r\n\r\n",api.addr,api.token);
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

async fn until_frame(ws: &mut TcpStream, kind: &str) -> (Value, Vec<Value>) {
    let mut frames = Vec::new();
    for _ in 0..200 {
        let v = ws_json(ws).await;
        frames.push(v.clone());
        if v["type"] == kind {
            return (v, frames);
        }
    }
    panic!("missing {kind}: {frames:?}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_ws_prompt_enters_run_tools_and_live_final_with_typed_input_refusals() {
    let home = unique_dir("chat-wire-prompt");
    fixture(&home);
    std::fs::write(home.join("chat-real.txt"), "CHAT-ACTUAL-READ-BYTES").unwrap();
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("chat").await;
    let mut ws = ws_connect(&h).await;
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"chat","sessionPath":"native:chat","text":"CHAT-TOOL","clientMessageId":"wire-client-1","snapshotVersion":1})).await;
    let (_, frames) = until_frame(&mut ws, "assistant_run_end").await;
    assert!(frames.iter().any(|v|v["type"]=="session_user_message"&&v["clientMessageId"]=="wire-client-1"),"{frames:?}");
    assert!(frames.iter().any(|v| v["type"] == "stage"), "{frames:?}");
    assert!(
        frames.iter().any(|v| v["type"] == "tool_start"),
        "{frames:?}"
    );
    assert!(
        frames.iter().any(|v| v["type"] == "tool_end"
            && v["success"] == true
            && v["status"] == "succeeded"
            && v["name"].as_str().is_some()),
        "{frames:?}"
    );
    assert!(
        frames.iter().any(|v| v["type"] == "assistant_segment_delta"
            && v["delta"].as_str().unwrap_or("").contains("CHAT-FINAL")),
        "{frames:?}"
    );
    assert!(remote.captured().last().unwrap()["messages"]
        .to_string()
        .contains("CHAT-ACTUAL-READ-BYTES"));
    let count = h
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id='chat'",
            vec![],
        )
        .await
        .unwrap();
    let calls = remote.captured().len();
    for bad in [
        json!({"text":""}),
        json!({"text":"bad-image","images":[{"mimeType":"application/exe","data":"AA=="}]}),
        json!({"text":"bad-size","images":[{"mimeType":"image/png","data":""}]}),
        json!({"text":"bad-content","images":[{"mimeType":"image/png","data":"not-base64"}]}),
        json!({"text":"bad-image","images":vec![json!({"mimeType":"image/png","data":"AA=="});11]}),
    ] {
        let mut payload = bad;
        payload["type"] = json!("prompt");
        payload["sessionId"] = json!("chat");
        payload["clientMessageId"] = json!("bad");
        payload["snapshotVersion"] = json!(1);
        ws_send(&mut ws, payload).await;
        let (rejected, _) = until_frame(&mut ws, "input_rejected").await;
        assert_eq!(rejected["outcome"], "not_accepted");
    }
    assert_eq!(remote.captured().len(), calls);
    assert_eq!(
        h.state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id='chat'",
                vec![]
            )
            .await
            .unwrap(),
        count
    );
    drop(ws);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_ws_delta_is_visible_before_provider_terminal_and_abort_stale_guard() {
    let home = unique_dir("chat-wire-live");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("live").await;
    let mut ws = ws_connect(&h).await;
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"live","text":"LIVE-DELTA","clientMessageId":"live-client","snapshotVersion":1})).await;
    remote.parked().await;
    let (delta, frames) = until_frame(&mut ws, "assistant_segment_delta").await;
    assert_eq!(delta["delta"], "LIVE-FIRST");
    assert!(!frames.iter().any(|v| v["type"] == "assistant_run_end"));
    let stream = delta["streamId"].clone();
    ws_send(
        &mut ws,
        json!({"type":"abort","sessionId":"live","streamId":"old-stream"}),
    )
    .await;
    let (stale, _) = until_frame(&mut ws, "abort_result").await;
    assert_eq!(stale["reason"], "stale_stream");
    assert_eq!(stale["streamId"], stream);
    assert_eq!(h.state.storage().list_active_runs().await.unwrap().len(), 1);
    ws_send(
        &mut ws,
        json!({"type":"abort","sessionId":"live","streamId":stream}),
    )
    .await;
    let (accepted, _) = until_frame(&mut ws, "abort_result").await;
    assert_eq!(accepted["status"], "accepted");
    let (idle, _) = until_frame(&mut ws, "status").await;
    assert_eq!(idle["isStreaming"], false);
    remote.release.add_permits(1);
    for _ in 0..1000 {
        if h.state
            .storage()
            .list_active_runs()
            .await
            .unwrap()
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(h
        .state
        .storage()
        .list_active_runs()
        .await
        .unwrap()
        .is_empty());
    assert_eq!(h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id='live' AND role='assistant'",vec![]).await.unwrap().unwrap(),"0");
    drop(ws);
    h.stop().await;
    remote.stop();
}

async fn prompt_done(ws: &mut TcpStream, id: &str, text: &str) -> Vec<Value> {
    ws_send(ws, json!({"type":"prompt","sessionId":id,"text":text})).await;
    let (_, mut frames) = until_frame(ws, "assistant_run_end").await;
    let (idle, tail) = until_frame(ws, "status").await;
    assert_eq!(idle["isStreaming"], false);
    frames.extend(tail);
    frames
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_ws_resume_recovers_exact_missing_events_and_marks_stale_or_empty() {
    let home = unique_dir("chat-resume");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("resume").await;
    h.create("empty").await;
    let mut ws = ws_connect(&h).await;
    ws_send(
        &mut ws,
        json!({"type":"prompt","sessionId":"resume","text":"LIVE-DELTA"}),
    )
    .await;
    remote.parked().await;
    let (first, frames) = until_frame(&mut ws, "assistant_segment_delta").await;
    let stream = first["streamId"].clone();
    let since = first["seq"].as_u64().unwrap();
    assert!(!frames.iter().any(|v| v["type"] == "assistant_run_end"));
    drop(ws);
    let mut ws = ws_connect(&h).await;
    // 持久终态与独立WS投影不是同一时点；在原总预算内同时核对两者。
    tokio::time::timeout(Duration::from_secs(5), async {
        remote.release.add_permits(1);
        for _ in 0..1000 {
            if h.state
                .storage()
                .list_active_runs()
                .await
                .unwrap()
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(h
            .state
            .storage()
            .list_active_runs()
            .await
            .unwrap()
            .is_empty());
        loop {
            let frame = ws_json(&mut ws).await;
            if frame["type"] == "status"
                && frame["sessionId"] == "resume"
                && frame["streamId"] == stream
                && frame["isStreaming"].as_bool() == Some(false)
            {
                break;
            }
        }
    })
    .await
    .expect("同一流须在原5秒总预算内持久结束并真实投递WS空闲状态");
    ws_send(&mut ws,json!({"type":"resume_stream","sessionId":"resume","streamId":stream,"sinceSeq":since,"resumeToken":"same"})).await;
    let (replay, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(replay["reset"], false);
    assert_eq!(replay["truncated"], false);
    assert_eq!(replay["runtimeIsStreaming"], false);
    assert_eq!(replay["resumeToken"], "same");
    let events = replay["events"].as_array().unwrap();
    assert!(!events.is_empty());
    let seqs = events
        .iter()
        .map(|v| v["seq"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(seqs[0], since + 1);
    assert!(seqs.windows(2).all(|v| v[1] == v[0] + 1));
    assert!(events.iter().any(|v| v["event"]["delta"] == "-FINAL"));
    assert_eq!(events.last().unwrap()["event"]["isStreaming"], false);
    ws_send(&mut ws,json!({"type":"resume_stream","sessionId":"resume","streamId":"obsolete","sinceSeq":999999})).await;
    let (reset, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(reset["reset"], true);
    assert_eq!(reset["sinceSeq"], 0);
    ws_send(
        &mut ws,
        json!({"type":"resume_stream","sessionId":"empty","sinceSeq":0}),
    )
    .await;
    let (empty, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(empty["events"], json!([]));
    assert_eq!(empty["runtimeIsStreaming"], false);
    ws_send(&mut ws, json!({"type":"abort","sessionId":"empty"})).await;
    let (stopped, _) = until_frame(&mut ws, "abort_result").await;
    assert_eq!(stopped["status"], "already_stopped");
    drop(ws);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_model_switch_default_thinking_and_image_survive_restart_into_provider() {
    use base64::Engine as _;
    let home = unique_dir("chat-model-media");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("existing").await;
    h.create("untouched").await;
    let mut ws = ws_connect(&h).await;
    prompt_done(&mut ws, "existing", "first").await;
    for body in [
        json!({"sessionId":"existing","provider":"remote","modelId":"second","extra":true}),
        json!({"provider":"remote","modelId":"second"}),
    ] {
        let (code, _) = h
            .server
            .request("POST", "/lingxi/v1/models/switch", Some(body))
            .await;
        assert_eq!(code, 400);
    }
    let (code, choice) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/models/switch",
            Some(json!({"sessionId":"existing","provider":"remote","modelId":"second"})),
        )
        .await;
    assert_eq!(code, 200, "{choice}");
    let (code, _) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/session-thinking-level",
            Some(json!({"sessionPath":"existing","level":"high"})),
        )
        .await;
    assert_eq!(code, 200);
    let (code, _) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/models/set",
            Some(json!({"provider":"remote","modelId":"chat"})),
        )
        .await;
    assert_eq!(code, 200);
    let (code, _) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/session-thinking-level",
            Some(json!({"level":"low"})),
        )
        .await;
    assert_eq!(code, 200);
    let image = base64::engine::general_purpose::STANDARD
        .encode([137, 80, 78, 71, 13, 10, 26, 10, 0, 1, 2, 3]);
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"existing","text":"with-image","images":[{"type":"image","mimeType":"image/png","data":image}]})).await;
    until_frame(&mut ws, "assistant_run_end").await;
    until_frame(&mut ws, "status").await;
    let sent = remote.captured().last().unwrap().clone();
    assert_eq!(sent["model"], "second");
    assert_eq!(sent["reasoning_effort"], "high");
    assert!(sent["messages"].to_string().contains(&image));
    prompt_done(&mut ws, "untouched", "other").await;
    let sent = remote.captured().last().unwrap().clone();
    assert_eq!(sent["model"], "chat");
    assert_eq!(sent["reasoning_effort"], "low");
    assert!(!sent["messages"].to_string().contains(&image));
    let (code, history) = h
        .server
        .request("GET", "/lingxi/v1/sessions/existing/history?all=1", None)
        .await;
    assert_eq!(code, 200);
    assert!(history["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "user_message"
            && v["legacyFields"]["images"][0]["mime"] == "image/png"));
    drop(ws);
    let home = h.stop().await;
    let h = boot(home, &remote, 1000000).await;
    let mut ws = ws_connect(&h).await;
    prompt_done(&mut ws, "existing", "after-restart").await;
    let sent = remote.captured().last().unwrap().clone();
    assert_eq!(sent["model"], "second");
    assert_eq!(sent["reasoning_effort"], "high");
    assert!(sent["messages"].to_string().contains(&image));
    assert!(sent["messages"].to_string().contains("with-image"));
    drop(ws);
    h.execute("existing", "http-execute-after-restart").await;
    let sent = remote.captured().last().unwrap().clone();
    assert_eq!(sent["model"], "second");
    assert_eq!(sent["reasoning_effort"], "high");
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_compaction_refusals_then_real_local_checkpoint_zero_fee_and_next_run() {
    let home = unique_dir("chat-compact");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("compact").await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let mut ws = ws_connect(&h).await;
    let before = h.snapshot("compact").await;
    for (method, reason) in [
        ("unknown", "invalid_compaction_method"),
        ("instant_simple", "experiment_disabled"),
    ] {
        ws_send(
            &mut ws,
            json!({"type":"compact","sessionId":"compact","method":method}),
        )
        .await;
        let (result, frames) = until_frame(&mut ws, "compaction_result").await;
        assert_eq!(result["status"], "failed");
        assert_eq!(result["reason"], reason);
        assert!(!frames.iter().any(|v| v["type"] == "compaction_accepted"));
        assert_eq!(h.snapshot("compact").await, before);
    }
    std::fs::create_dir_all(h.state.memory().user_dir()).unwrap();
    std::fs::write(
        h.state.memory().user_dir().join("preferences.json"),
        json!({"experiments":{"session.instant_simple_compaction":true}}).to_string(),
    )
    .unwrap();
    let calls = remote.captured().len();
    let usage=h.state.storage().query_one_text("SELECT json_group_array(json_object('id',model_call_id,'provider',provider,'model',model,'input',input_tokens,'output',output_tokens,'outcome',outcome,'attempts',transport_attempts,'recorded',recorded_at_unix_ms)) FROM (SELECT * FROM model_call_usage ORDER BY model_call_id)",vec![]).await.unwrap().unwrap();
    ws_send(
        &mut ws,
        json!({"type":"compact","sessionId":"compact","method":"instant_simple"}),
    )
    .await;
    let (result, frames) = until_frame(&mut ws, "compaction_result").await;
    assert!(
        frames
            .iter()
            .position(|v| v["type"] == "compaction_accepted")
            .unwrap()
            < frames.len() - 1,
        "{frames:?}"
    );
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(result["mode"], "lossy_local");
    assert_eq!(remote.captured().len(), calls);
    assert_eq!(h.state.storage().query_one_text("SELECT json_group_array(json_object('id',model_call_id,'provider',provider,'model',model,'input',input_tokens,'output',output_tokens,'outcome',outcome,'attempts',transport_attempts,'recorded',recorded_at_unix_ms)) FROM (SELECT * FROM model_call_usage ORDER BY model_call_id)",vec![]).await.unwrap().unwrap(),usage);
    let after = h.load("compact").await;
    assert!(
        matches!(after.first(),Some(lingxi_kernel::model_exchange::ExchangeItem::CompactionSummary{summary,..}) if summary.contains("Instant Simple")&&summary.contains("OLD-USER-0")&&summary.contains("without a model request"))
    );
    let original_rows=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id='compact' AND role IN ('user','assistant')",vec![]).await.unwrap().unwrap();
    assert_eq!(original_rows, "8");
    drop(ws);
    let home = h.stop().await;
    let h = boot(home, &remote, 1000000).await;
    let mut ws = ws_connect(&h).await;
    prompt_done(&mut ws, "compact", "continue-real").await;
    assert!(remote.captured().last().unwrap()["messages"]
        .to_string()
        .contains("Instant Simple"));
    assert_eq!(remote.captured().len(), calls + 1);
    drop(ws);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_real_steering_tool_continuation_idle_prompt_and_foreign_controls_do_not_mutate() {
    let home = unique_dir("chat-steer-owner");
    fixture(&home);
    std::fs::write(home.join("chat-real.txt"), "STEER-READ-FILE").unwrap();
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("owned").await;
    let (code, device) = h
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
        token: device["secret"].as_str().unwrap().into(),
    };
    let mut outsider = ws_connect_api(&foreign).await;
    let snapshot = h.snapshot("owned").await;
    let calls = remote.captured().len();
    for payload in [
        json!({"type":"prompt","text":"SECRET-ATTEMPT"}),
        json!({"type":"steer","text":"SECRET-ATTEMPT"}),
        json!({"type":"interject","text":"SECRET-ATTEMPT"}),
        json!({"type":"abort"}),
        json!({"type":"resume_stream","sinceSeq":0}),
    ] {
        let mut request = payload;
        request["sessionId"] = json!("owned");
        ws_send(&mut outsider, request).await;
        let (error, _) = until_frame(&mut outsider, "error").await;
        assert_eq!(error["code"], "session_forbidden");
        assert!(!error.to_string().contains("CHAT-FINAL"));
    }
    assert_eq!(h.snapshot("owned").await, snapshot);
    assert_eq!(remote.captured().len(), calls);
    let mut ws = ws_connect(&h).await;
    remote.gate.store(true, Ordering::SeqCst);
    ws_send(
        &mut ws,
        json!({"type":"prompt","sessionId":"owned","text":"CHAT-TOOL"}),
    )
    .await;
    remote.parked().await;
    ws_send(
        &mut ws,
        json!({"type":"steer","sessionId":"owned","text":"ACTUAL-STEERING-NEXT-TURN"}),
    )
    .await;
    let (steered, _) = until_frame(&mut ws, "steered").await;
    assert_eq!(steered["sessionId"], "owned");
    remote.gate.store(false, Ordering::SeqCst);
    remote.release.add_permits(1);
    until_frame(&mut ws, "assistant_run_end").await;
    until_frame(&mut ws, "status").await;
    assert!(remote.captured().iter().skip(1).any(|v| v["messages"]
        .to_string()
        .contains("ACTUAL-STEERING-NEXT-TURN")));
    let oldcalls = remote.captured().len();
    ws_send(
        &mut ws,
        json!({"type":"steer","sessionId":"owned","text":"IDLE-STEER-PROMPT"}),
    )
    .await;
    until_frame(&mut ws, "assistant_run_end").await;
    until_frame(&mut ws, "status").await;
    assert_eq!(remote.captured().len(), oldcalls + 1);
    assert!(remote.captured().last().unwrap()["messages"]
        .to_string()
        .contains("IDLE-STEER-PROMPT"));
    let (code, _) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/models/switch",
            Some(json!({"sessionId":"owned","provider":"remote","modelId":"not-configured"})),
        )
        .await;
    assert_eq!(code, 400);
    assert_eq!(remote.captured().len(), oldcalls + 1);
    drop(ws);
    drop(outsider);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_interject_interrupts_only_target_and_preserves_real_user_receipt() {
    let home = unique_dir("chat-interject");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("target").await;
    h.create("other").await;
    let mut ws = ws_connect(&h).await;
    let mut other = ws_connect(&h).await;
    remote.gate.store(true, Ordering::SeqCst);
    ws_send(
        &mut ws,
        json!({"type":"prompt","sessionId":"target","text":"OLD-TARGET"}),
    )
    .await;
    remote.parked().await;
    ws_send(
        &mut other,
        json!({"type":"prompt","sessionId":"other","text":"OTHER-LIVE"}),
    )
    .await;
    remote.parked().await;
    let live = h.state.storage().list_active_runs().await.unwrap();
    assert_eq!(live.len(), 2);
    let before = h.snapshot("target").await;
    let calls = remote.captured().len();
    for invalid in [
        json!({"images":vec![json!({"data":"","mimeType":"image/png"});11]}),
        json!({"images":[{"data":"","mimeType":"image/svg+xml"}]}),
        json!({"images":[{"data":"","mimeType":"image/png"}]}),
        json!({"sessionPath":"native:other"}),
    ] {
        let mut request =
            json!({"type":"interject","sessionId":"target","text":"MUST-NOT-INTERRUPT"});
        for (key, value) in invalid.as_object().unwrap() {
            request[key] = value.clone();
        }
        ws_send(&mut ws, request).await;
        let (rejected, _) = until_frame(&mut ws, "input_rejected").await;
        assert_eq!(rejected["outcome"], "not_accepted");
        assert_eq!(h.snapshot("target").await, before);
        assert_eq!(remote.captured().len(), calls);
        assert_eq!(h.state.storage().list_active_runs().await.unwrap().len(), 2);
    }
    let old = live
        .iter()
        .find(|r| r.session_id == "target")
        .unwrap()
        .run_id
        .clone();
    let neighbour = live
        .iter()
        .find(|r| r.session_id == "other")
        .unwrap()
        .run_id
        .clone();
    remote.gate.store(false, Ordering::SeqCst);
    ws_send(&mut ws,json!({"type":"interject","sessionId":"target","text":"NEW-INTERJECT","clientMessageId":"interject-input","snapshotVersion":3})).await;
    let mut new_receipt = false;
    let mut final_seen = false;
    for _ in 0..100 {
        let frame = ws_json(&mut ws).await;
        if frame["type"] == "input_rejected" {
            panic!("interject rejected: {frame}")
        }
        if frame["type"] == "session_user_message" && frame["clientMessageId"] == "interject-input"
        {
            new_receipt = true;
        }
        if frame["type"] == "assistant_run_end" && frame["runId"] != old {
            final_seen = true;
            break;
        }
    }
    assert!(new_receipt && final_seen);
    assert_eq!(
        h.state
            .storage()
            .load_run(&lingxi_protocol::RunId::new(&old))
            .await
            .unwrap()
            .unwrap()
            .status,
        lingxi_protocol::RunStatus::Cancelled
    );
    assert!(!h
        .state
        .storage()
        .load_run(&lingxi_protocol::RunId::new(&neighbour))
        .await
        .unwrap()
        .unwrap()
        .status
        .is_terminal());
    remote.release.add_permits(2);
    for _ in 0..1000 {
        if h.state
            .storage()
            .list_active_runs()
            .await
            .unwrap()
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        h.state
            .storage()
            .load_run(&lingxi_protocol::RunId::new(&neighbour))
            .await
            .unwrap()
            .unwrap()
            .status,
        lingxi_protocol::RunStatus::Completed
    );
    let (code, history) = h
        .server
        .request("GET", "/lingxi/v1/sessions/target/history?all=1", None)
        .await;
    assert_eq!(code, 200);
    assert!(history["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "user_message" && v["text"] == "NEW-INTERJECT"));
    assert_eq!(h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id='target' AND message_id=?1",vec![format!("{old}-final")]).await.unwrap().unwrap(),"0");
    drop(ws);
    drop(other);
    h.stop().await;
    remote.stop();
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_current_chat_page_inputarea_tiptap_true_network_and_queue_interject() {
    let home = unique_dir("chat-native");
    fixture(&home);
    std::fs::write(home.join("chat-real.txt"), "NATIVE-REAL-FILE-CONSUMER").unwrap();
    let remote = Remote::start().await;
    let mut limits = lingxi_service::quotas::QuotaLimits::default();
    limits.model.global = 1;
    let h = boot_with_quota(home, &remote, 1000000, limits).await;
    h.create("desktop").await;
    h.create("quota-holder").await;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let url = format!("http://{}", h.server.addr);
    let token = h.server.token.clone();
    let ui_home = h.home.clone();
    let mut child = tokio::task::spawn_blocking(move || {
        std::process::Command::new(std::env::var("R06_NODE_BIN").unwrap_or_else(|_| "node".into()))
            .arg(root.join("desktop/src/react/__tests__/chat-live/launch.mjs"))
            .env("R06_CHAT_HOME", ui_home)
            .env("R06_CHAT_URL", url)
            .env("R06_CHAT_TOKEN", token)
            .output()
            .unwrap()
    });
    let mut provider_released = false;
    let mut holder = None;
    let mut holder_released = false;
    let output = loop {
        tokio::select! {
         output=&mut child=>break output.unwrap(),
         _=tokio::time::sleep(Duration::from_millis(10))=>{
          if !provider_released&&h.home.join("chat-ui-provider-release").exists(){remote.release.add_permits(1);provider_released=true;}
          if holder.is_none()&&h.home.join("chat-ui-quota-hold").exists(){let mut ws=ws_connect(&h).await;ws_send(&mut ws,json!({"type":"prompt","sessionId":"quota-holder","text":"LIVE-DELTA-HOLDER"})).await;holder=Some(ws);}
          if holder.is_some()&&remote.captured().iter().any(|v|v["messages"].as_array().and_then(|m|m.iter().rfind(|m|m["role"]=="user")).is_some_and(|m|m["content"].to_string().contains("LIVE-DELTA-HOLDER"))){std::fs::write(h.home.join("chat-ui-quota-ready"),"real holder request active").unwrap();}
          if !holder_released&&h.home.join("chat-ui-quota-release").exists(){ws_send(holder.as_mut().unwrap(),json!({"type":"abort","sessionId":"quota-holder"})).await;remote.release.add_permits(3);holder_released=true;}
         }
        }
    };

    println!(
        "CHAT-NATIVE: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "{}",
        std::fs::read_to_string(h.home.join("chat-ui-failure.txt")).unwrap_or_default()
    );
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(h.home.join("chat-ui-result.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["checks"].as_array().unwrap().len(), 27, "{receipt}");
    assert!(remote.captured().iter().any(|v| v["messages"]
        .to_string()
        .contains("NATIVE-REAL-FILE-CONSUMER")));
    assert!(remote
        .captured()
        .iter()
        .any(|v| v["messages"].to_string().contains("QUEUED-NEXT")));
    assert!(remote.captured().iter().any(|v| v["model"] == "second"
        && v["reasoning_effort"] == "high"
        && v["messages"]
            .as_array()
            .and_then(|m| m.iter().rfind(|m| m["role"] == "user"))
            .is_some_and(|m| m["content"].to_string().contains("MODEL-THINKING-CHOICE"))));
    assert!(!remote.captured().iter().any(|v| v["messages"]
        .as_array()
        .and_then(|m| m.iter().rfind(|m| m["role"] == "user"))
        .is_some_and(|m| m["content"].to_string().contains("MODEL-QUOTA-WAIT"))));
    h.stop().await;
    remote.stop();
}

async fn attach_snapshot(h: &Harness, session: &str, path: &Path) -> Value {
    let (code, value) = h
        .server
        .request(
            "POST",
            &format!("/lingxi/v1/sessions/{session}/attachments/local"),
            Some(json!({"path":path,"mode":"snapshot"})),
        )
        .await;
    assert_eq!(code, 201, "{value}");
    value["file"].clone()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_refs_real_owner_snapshots_office_bytes_transaction_and_negative_zero_run() {
    use std::io::Write as _;
    let home = unique_dir("chat-files");
    fixture(&home);
    let workspace = unique_dir("chat-source-files");
    std::fs::write(workspace.join("note.txt"), "CHAT-REAL-TEXT-ATTACHMENT").unwrap();
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive.write_all(br#"<w:document xmlns:w="urn:word"><w:body><w:p><w:r><w:t>CHAT-OFFICE-REAL-TEXT</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
    std::fs::write(
        workspace.join("document.docx"),
        archive.finish().unwrap().into_inner(),
    )
    .unwrap();
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("files").await;
    h.create("other").await;
    let text = attach_snapshot(&h, "files", &workspace.join("note.txt")).await;
    let docx = attach_snapshot(&h, "files", &workspace.join("document.docx")).await;
    let forbidden = attach_snapshot(&h, "other", &workspace.join("note.txt")).await;
    let mut ws = ws_connect(&h).await;
    let before = h.snapshot("files").await;
    let calls = remote.captured().len();
    for reference in [
        json!({"fileId":text["fileId"],"sessionId":"other"}),
        json!({"fileId":forbidden["fileId"]}),
        json!({"fileId":"sf_not-real"}),
        json!({"fileId":text["fileId"],"path":"/etc/passwd"}),
    ] {
        ws_send(&mut ws,json!({"type":"prompt","sessionId":"files","text":"bad-ref","sessionFileRefs":[reference]})).await;
        let (rejected, _) = until_frame(&mut ws, "input_rejected").await;
        assert_eq!(rejected["outcome"], "not_accepted");
        assert_eq!(h.snapshot("files").await, before);
        assert_eq!(remote.captured().len(), calls);
    }
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"files","text":"READ-MY-FILES","clientMessageId":"file-client","snapshotVersion":2,"sessionFileRefs":[{"fileId":text["fileId"],"sessionId":"files","sessionPath":"native:files"},{"fileId":docx["fileId"]}]})).await;
    let (_, frames) = until_frame(&mut ws, "assistant_run_end").await;
    until_frame(&mut ws, "status").await;
    assert!(frames.iter().any(|v| v["type"] == "session_user_message"
        && v["message"]["attachments"]
            .as_array()
            .is_some_and(|a| a.len() == 2)));
    let sent = remote.captured().last().unwrap().clone();
    assert!(sent["messages"]
        .to_string()
        .contains("CHAT-REAL-TEXT-ATTACHMENT"));
    assert!(sent["messages"]
        .to_string()
        .contains("CHAT-OFFICE-REAL-TEXT"));
    let (code, history) = h
        .server
        .request(
            "GET",
            "/lingxi/v1/sessions/files/history?all=1&reconciliation=1",
            None,
        )
        .await;
    assert_eq!(code, 200);
    let user = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "user_message")
        .unwrap();
    assert_eq!(user["text"], "READ-MY-FILES");
    assert_eq!(user["legacyFields"]["clientMessageId"], "file-client");
    assert_eq!(user["legacyFields"]["snapshotVersion"], 2);
    assert_eq!(
        user["legacyFields"]["attachments"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(history["reconciliation"]["complete"], true);
    assert_eq!(history["reconciliation"]["runStatus"], "reconciled_idle");
    assert_eq!(
        history["reconciliation"]["snapshotId"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM session_file_refs WHERE session_id='files' AND ref_kind='message_marker'",vec![]).await.unwrap().unwrap(),"2");
    drop(ws);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_attachment_preparation_abort_is_immediately_idle_zero_run_and_no_late_submit() {
    let home = unique_dir("chat-preparing-abort");
    fixture(&home);
    let workspace = unique_dir("chat-preparing-file");
    std::fs::write(workspace.join("note.txt"), "PREPARING-CANCELLED-CONTENT").unwrap();
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("preparing").await;
    let file = attach_snapshot(&h, "preparing", &workspace.join("note.txt")).await;
    let entered = Arc::new(AtomicBool::new(false));
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let wait = Arc::new(Mutex::new(wait));
    let observed = entered.clone();
    h.state
        .session_files()
        .set_payload_read_observer(Arc::new(move || {
            observed.store(true, Ordering::SeqCst);
            wait.lock().unwrap().recv().unwrap();
        }));
    let before = h.snapshot("preparing").await;
    let mut ws = ws_connect(&h).await;
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"preparing","text":"do-not-submit","sessionFileRefs":[{"fileId":file["fileId"]}]})).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    ws_send(&mut ws, json!({"type":"abort","sessionId":"preparing"})).await;
    let (accepted, _) = until_frame(&mut ws, "abort_result").await;
    assert_eq!(accepted["status"], "accepted");
    let (idle, _) = until_frame(&mut ws, "status").await;
    assert_eq!(idle["isStreaming"], false);
    assert_eq!(h.snapshot("preparing").await, before);
    assert!(remote.captured().is_empty());
    release.send(()).unwrap();
    let (rejected, _) = until_frame(&mut ws, "input_rejected").await;
    assert_eq!(rejected["code"], "user_abort");
    assert_eq!(h.snapshot("preparing").await, before);
    assert!(remote.captured().is_empty());
    drop(ws);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn referenced_images_real_20mib_limit_owned_bytes_and_no_run_on_refusal() {
    use std::io::Write as _;
    let home = unique_dir("chat-image-ref");
    fixture(&home);
    let workspace = unique_dir("chat-image-bytes");
    let mut png = std::fs::File::create(workspace.join("large.png")).unwrap();
    png.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
    png.set_len(21 * 1024 * 1024).unwrap();
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("image").await;
    h.create("neighbour").await;
    let (code, view) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/image/attachments/local",
            Some(json!({"path":workspace.join("large.png"),"mode":"reference"})),
        )
        .await;
    assert_eq!(code, 201, "{view}");
    let file_id = &view["file"]["fileId"];
    let mut ws = ws_connect(&h).await;
    let before = h.snapshot("image").await;
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"image","text":"too-large","images":[{"fileId":file_id,"mimeType":"image/png"}]})).await;
    let (rejected, _) = until_frame(&mut ws, "input_rejected").await;
    assert_eq!(rejected["code"], "image_too_large");
    assert_eq!(h.snapshot("image").await, before);
    assert!(remote.captured().is_empty());
    ws_send(&mut ws,json!({"type":"interject","sessionId":"neighbour","text":"foreign-image","images":[{"fileId":file_id,"mimeType":"image/png"}]})).await;
    let (rejected, _) = until_frame(&mut ws, "input_rejected").await;
    assert_eq!(rejected["code"], "image_not_owned");
    assert!(remote.captured().is_empty());
    png.set_len(15 * 1024 * 1024 + 1).unwrap();
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"image","text":"encoded-limit-plus-one","images":[{"fileId":file_id,"mimeType":"image/png"}]})).await;
    let (rejected, _) = until_frame(&mut ws, "input_rejected").await;
    assert_eq!(rejected["code"], "image_too_large");
    assert_eq!(h.snapshot("image").await, before);
    assert!(remote.captured().is_empty());
    // 真实受权图大于恢复文件的旧 8MiB 预算：读取上限不能误用恢复写入上限。
    png.set_len(9 * 1024 * 1024).unwrap();
    drop(png);
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"image","text":"large-but-valid","images":[{"fileId":file_id,"mimeType":"image/png"}]})).await;
    until_frame(&mut ws, "assistant_run_end").await;
    until_frame(&mut ws, "status").await;
    use base64::Engine as _;
    let sent = remote.captured().last().unwrap().clone();
    let parts = sent["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_array()
        .unwrap();
    let data = parts
        .iter()
        .find_map(|part| part["image_url"]["url"].as_str())
        .unwrap()
        .split_once(',')
        .unwrap()
        .1;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    assert_eq!(bytes.len(), 9 * 1024 * 1024);
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(
        h.state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE session_id='image'",
                vec![]
            )
            .await
            .unwrap()
            .unwrap(),
        "1"
    );
    drop(ws);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_compaction_real_accepted_success_noop_failure_and_streaming_negative() {
    let home = unique_dir("chat-compact-normal");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("success").await;
    h.fill("failure").await;
    h.create("empty").await;
    h.create("active").await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let mut ws = ws_connect(&h).await;
    ws_send(&mut ws, json!({"type":"compact","sessionId":"success"})).await;
    let (result, frames) = until_frame(&mut ws, "compaction_result").await;
    assert_eq!(result["status"], "succeeded", "{result}");
    assert!(
        frames
            .iter()
            .position(|v| v["type"] == "compaction_accepted")
            .unwrap()
            < frames.len() - 1
    );
    let archive = h.archive("success").await;
    assert!(archive.to_string().contains("EX01-SUMMARY"));
    ws_send(&mut ws, json!({"type":"compact","sessionId":"empty"})).await;
    let (result, frames) = until_frame(&mut ws, "compaction_result").await;
    assert_eq!(result["status"], "noop", "{result}");
    assert!(frames.iter().any(|v| v["type"] == "compaction_accepted"));
    let before = h.snapshot("failure").await;
    remote.fail.store(true, Ordering::SeqCst);
    ws_send(&mut ws, json!({"type":"compact","sessionId":"failure"})).await;
    let (result, frames) = until_frame(&mut ws, "compaction_result").await;
    assert_eq!(result["status"], "failed", "{result}");
    assert!(frames.iter().any(|v| v["type"] == "compaction_accepted"));
    assert_eq!(h.snapshot("failure").await, before);
    assert!(h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE session_id='failure' AND model='summary'",vec![]).await.unwrap().unwrap().parse::<u64>().unwrap()>0);
    remote.fail.store(false, Ordering::SeqCst);
    ws_send(
        &mut ws,
        json!({"type":"prompt","sessionId":"active","text":"LIVE-DELTA"}),
    )
    .await;
    remote.parked().await;
    until_frame(&mut ws, "assistant_segment_delta").await;
    let before = h.snapshot("active").await;
    ws_send(&mut ws, json!({"type":"compact","sessionId":"active"})).await;
    let (result, frames) = until_frame(&mut ws, "compaction_result").await;
    assert_eq!(result["reason"], "session_streaming");
    assert!(!frames.iter().any(|v| v["type"] == "compaction_accepted"));
    assert_eq!(h.snapshot("active").await, before);
    ws_send(
        &mut ws,
        json!({"type":"compact","sessionId":"not-a-session"}),
    )
    .await;
    let (result, frames) = until_frame(&mut ws, "compaction_result").await;
    assert_eq!(result["status"], "failed");
    assert!(!frames.iter().any(|v| v["type"] == "compaction_accepted"));
    remote.release.add_permits(1);
    until_frame(&mut ws, "assistant_run_end").await;
    until_frame(&mut ws, "status").await;
    drop(ws);
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_provider_many_deltas_resume_reports_truncation_and_retains_contiguous_tail() {
    let home = unique_dir("chat-replay-floor");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("many").await;
    let mut ws = ws_connect(&h).await;
    ws_send(
        &mut ws,
        json!({"type":"prompt","sessionId":"many","text":"REPLAY-MANY"}),
    )
    .await;
    until_frame(&mut ws, "assistant_segment_delta").await;
    drop(ws);
    tokio::time::timeout(Duration::from_secs(30), async {
        while h
            .state
            .storage()
            .query_one_text(
                "SELECT status FROM runs WHERE session_id='many' LIMIT 1",
                vec![],
            )
            .await
            .unwrap()
            .as_deref()
            != Some("completed")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // Run 提交和 WS 投影有独立水位；等待真实缓存终态，连接可因有界背压要求重连。
    let replay = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let mut resumed = ws_connect(&h).await;
            ws_send(
                &mut resumed,
                json!({"type":"resume_stream","sessionId":"many","sinceSeq":0}),
            )
            .await;
            let value = loop {
                let mut head = [0; 2];
                resumed.read_exact(&mut head).await.unwrap();
                assert_eq!(head[1] & 0x80, 0);
                let n = match head[1] & 127 {
                    126 => {
                        let mut b = [0; 2];
                        resumed.read_exact(&mut b).await.unwrap();
                        u16::from_be_bytes(b) as u64
                    }
                    127 => {
                        let mut b = [0; 8];
                        resumed.read_exact(&mut b).await.unwrap();
                        u64::from_be_bytes(b)
                    }
                    v => v as u64,
                };
                assert!(n < 10 * 1024 * 1024);
                let mut bytes = vec![0; n as usize];
                resumed.read_exact(&mut bytes).await.unwrap();
                if head[0] & 0xf == 8 {
                    break None;
                }
                assert_eq!(head[0] & 0xf, 1);
                let frame: Value = serde_json::from_slice(&bytes).unwrap();
                if frame["type"] == "stream_resume" {
                    break Some(frame);
                }
            };
            if let Some(value) = value {
                if value["isStreaming"] == false {
                    break value;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(replay["type"], "stream_resume");
    assert_eq!(
        replay["truncated"],
        true,
        "nextSeq={} frames={}",
        replay["nextSeq"],
        replay["events"].as_array().unwrap().len()
    );
    assert_eq!(replay["reset"], false);
    assert_eq!(replay["runtimeIsStreaming"], false);
    let frames = replay["events"].as_array().unwrap();
    assert!(frames.len() <= 4096);
    assert!(frames.first().unwrap()["seq"].as_u64().unwrap() > 1);
    assert!(frames
        .windows(2)
        .all(|v| v[1]["seq"].as_u64().unwrap() == v[0]["seq"].as_u64().unwrap() + 1));
    assert_eq!(frames.last().unwrap()["event"]["isStreaming"], false);
    assert_eq!(
        frames.last().unwrap()["seq"].as_u64().unwrap() + 1,
        replay["nextSeq"].as_u64().unwrap()
    );
    let (code, history) = h
        .server
        .request("GET", "/lingxi/v1/sessions/many/history?all=1", None)
        .await;
    assert_eq!(code, 200);
    assert!(history["items"].to_string().contains(&"x".repeat(4200)));
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deleted_agent_steer_is_rejected_without_cancelling_neighbour_session() {
    let home = unique_dir("chat-deleted-agent");
    fixture(&home);
    std::fs::create_dir_all(home.join("agents/doomed")).unwrap();
    std::fs::copy(
        home.join("agents/lingxi/config.yaml"),
        home.join("agents/doomed/config.yaml"),
    )
    .unwrap();
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("neighbour").await;
    let (code, v) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions",
            Some(json!({"sessionId":"deleted","agentId":"doomed","title":"deleted"})),
        )
        .await;
    assert_eq!(code, 201, "{v}");
    let (code, v) = h
        .server
        .request("DELETE", "/lingxi/v1/agents/doomed", None)
        .await;
    assert_eq!(code, 200, "{v}");
    let mut live = ws_connect(&h).await;
    ws_send(
        &mut live,
        json!({"type":"prompt","sessionId":"neighbour","text":"LIVE-DELTA"}),
    )
    .await;
    remote.parked().await;
    until_frame(&mut live, "assistant_segment_delta").await;
    let active = h
        .state
        .storage()
        .list_active_runs()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.session_id == "neighbour")
        .unwrap()
        .run_id;
    let before = h.snapshot("deleted").await;
    let calls = remote.captured().len();
    let mut ws = ws_connect(&h).await;
    ws_send(
        &mut ws,
        json!({"type":"steer","sessionId":"deleted","text":"MUST-NOT-SUBMIT"}),
    )
    .await;
    let (error, _) = until_frame(&mut ws, "error").await;
    assert_eq!(error["code"], "agent_unavailable", "{error}");
    assert_eq!(h.snapshot("deleted").await, before);
    assert_eq!(remote.captured().len(), calls);
    assert!(h
        .state
        .storage()
        .list_active_runs()
        .await
        .unwrap()
        .iter()
        .any(|r| r.run_id == active));
    remote.release.add_permits(1);
    until_frame(&mut live, "assistant_run_end").await;
    until_frame(&mut live, "status").await;
    drop(ws);
    drop(live);
    h.stop().await;
    remote.stop();
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_current_app_pages_side_preview_attachment_and_model_response_generation() {
    let home = unique_dir("chat-app-native");
    fixture(&home);
    std::fs::write(home.join("app-preview.txt"), "APP-PREVIEW-REAL-BYTES").unwrap();
    let attachment_root = unique_dir("chat-app-upload");
    let attachment = attachment_root.join("app-attachment.txt");
    std::fs::write(&attachment, "APP-ATTACHMENT-ACTUAL-MODEL-CONSUMER").unwrap();
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("desktop").await;
    h.create("other").await;
    h.execute("other", "OTHER-SEED").await;
    for (session, level) in [("desktop", "high"), ("other", "off")] {
        let (status, value) = h
            .server
            .request(
                "POST",
                "/lingxi/v1/session-thinking-level",
                Some(json!({"sessionPath":session,"level":level})),
            )
            .await;
        assert_eq!(status, 200, "{value}");
    }
    let initial_session_count = h
        .state
        .storage()
        .query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM sessions", vec![])
        .await
        .unwrap()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let url = format!("http://{}", h.server.addr);
    let token = h.server.token.clone();
    let ui_home = h.home.clone();
    let mut child = tokio::task::spawn_blocking(move || {
        std::process::Command::new(std::env::var("R06_NODE_BIN").unwrap_or_else(|_| "node".into()))
            .arg(root.join("desktop/src/react/__tests__/chat-app-live/launch.mjs"))
            .env("R06_CHAT_HOME", ui_home)
            .env("R06_CHAT_ATTACHMENT", attachment)
            .env("R06_CHAT_URL", url)
            .env("R06_CHAT_TOKEN", token)
            .output()
            .unwrap()
    });
    let mut provider_released = false;
    let output = loop {
        tokio::select! {
            output = &mut child => break output.unwrap(),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {
                if !provider_released && h.home.join("app-provider-release").exists() {
                    remote.release.add_permits(1); provider_released = true;
                }
            }
        }
    };
    println!(
        "APP-PAGES-NATIVE: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "{}; home={}",
        std::fs::read_to_string(h.home.join("chat-app-failure.txt")).unwrap_or_default(),
        h.home.display()
    );
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(h.home.join("chat-app-result.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["checks"].as_array().unwrap().len(), 34, "{receipt}");
    assert!(remote.captured().iter().any(|value| value["messages"]
        .to_string()
        .contains("APP-ATTACHMENT-ACTUAL-MODEL-CONSUMER")));
    assert!(remote
        .captured()
        .iter()
        .any(|value| value["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| message["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["type"] == "image_url"
                    && part["image_url"]["url"]
                        .as_str()
                        .is_some_and(|url| url.starts_with("data:image/png;base64,"))))))));
    assert!(h.snapshot("desktop").await.contains("APP-WITH-IMAGE"));
    let side = receipt["side"]["sessionId"].as_str().unwrap();
    assert_ne!(side, "desktop");
    assert!(remote.captured().iter().any(|value| value["messages"]
        .to_string()
        .contains("[用户引用的 Session]")
        && value["messages"].to_string().contains("SIDE-ONLY")));
    assert!(h.snapshot(side).await.contains("SIDE-ONLY"));
    assert!(!h.snapshot("desktop").await.contains("SIDE-ONLY"));
    let proxy: Value =
        serde_json::from_slice(&std::fs::read(h.home.join("app-proxy-receipt.json")).unwrap())
            .unwrap();
    assert_eq!(proxy["delayed"], 2);
    let dropped = proxy["dropped"].as_array().unwrap();
    assert!(!dropped.is_empty());
    assert!(dropped
        .iter()
        .all(|path| path == "/lingxi/v1/session-thinking-level"));
    assert_eq!(
        receipt["requests"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|request| request["method"] == "POST"
                && request["url"]
                    .as_str()
                    .is_some_and(|url| url.ends_with("/lingxi/v1/session-thinking-level")))
            .count(),
        1
    );
    let (status, actual_thinking) = h
        .server
        .request(
            "GET",
            "/lingxi/v1/session-thinking-level?sessionPath=other",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        actual_thinking["thinkingLevel"],
        receipt["thinkingBeforeFailure"]
    );
    assert_eq!(
        h.state
            .storage()
            .query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM sessions", vec![])
            .await
            .unwrap()
            .unwrap(),
        (initial_session_count + 1).to_string()
    );
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn model_switch_uses_real_session_admission_and_rejections_preserve_choice() {
    let home = unique_dir("chat-choice-admission");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("choice").await;
    let (_, before) = h
        .server
        .request(
            "GET",
            "/lingxi/v1/session-thinking-level?sessionPath=choice",
            None,
        )
        .await;
    let permit = h
        .state
        .storage()
        .session_admission()
        .try_acquire(&["choice"], usize::MAX)
        .unwrap();
    let input = json!({"sessionId":"choice","modelId":"second","provider":"remote"});
    let (code, _) = h
        .server
        .request("POST", "/lingxi/v1/models/switch", Some(input.clone()))
        .await;
    assert_eq!(code, 409);
    assert!(remote.captured().is_empty());
    assert!(!h.home.join("chat-input-settings.json").exists());
    assert_eq!(
        h.server
            .request(
                "GET",
                "/lingxi/v1/session-thinking-level?sessionPath=choice",
                None
            )
            .await
            .1,
        before
    );
    drop(permit);
    let (code, changed) = h
        .server
        .request("POST", "/lingxi/v1/models/switch", Some(input))
        .await;
    assert_eq!(code, 200, "{changed}");
    let bytes = std::fs::read(h.home.join("chat-input-settings.json")).unwrap();
    remote.gate.store(true, Ordering::SeqCst);
    let mut ws = ws_connect(&h).await;
    ws_send(
        &mut ws,
        json!({"type":"prompt","sessionId":"choice","text":"CHOICE-PARKED"}),
    )
    .await;
    remote.parked().await;
    let (code, _) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/models/switch",
            Some(json!({"sessionId":"choice","modelId":"chat","provider":"remote"})),
        )
        .await;
    assert_eq!(code, 409);
    assert_eq!(
        std::fs::read(h.home.join("chat-input-settings.json")).unwrap(),
        bytes
    );
    assert_eq!(remote.captured().len(), 1);
    assert_eq!(remote.captured()[0]["model"], "second");
    remote.release.add_permits(1);
    until_frame(&mut ws, "assistant_run_end").await;
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_id_references_are_authorized_preserved_and_never_read_source_body_implicitly() {
    let home = unique_dir("chat-session-references");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.create("reference-source").await;
    h.create("reference-target").await;
    h.execute("reference-source", "REFERENCE-SOURCE-SECRET-NOT-INHERITED")
        .await;
    remote.wire.lock().unwrap().clear();
    let mut ws = ws_connect(&h).await;
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"reference-target","text":"引用片段继续追问","displayMessage":{"text":"继续追问","quotedText":"引用片段"},"sessionRefs":[{"sessionId":"reference-source","label":"实际来源"},{"sessionId":"reference-source","label":"重复"}]})).await;
    until_frame(&mut ws, "assistant_run_end").await;
    assert_eq!(remote.captured().len(), 1);
    let input = remote.captured()[0]["messages"].to_string();
    assert!(input.contains("[用户引用的 Session]"));
    assert!(input.contains("实际来源: reference-source"));
    assert!(input.contains("这些只是 Session ID 引用"));
    assert!(!input.contains("REFERENCE-SOURCE-SECRET-NOT-INHERITED"));
    let stored=h.state.storage().query_one_text("SELECT content_json FROM messages WHERE session_id='reference-target' AND role='user' ORDER BY seq LIMIT 1",vec![]).await.unwrap().unwrap();
    let stored: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored["text"], "继续追问");
    assert_eq!(stored["quotedText"], "引用片段");
    assert_eq!(
        stored["sessionRefs"],
        json!([{"sessionId":"reference-source","label":"实际来源"}])
    );
    let baseline = h.snapshot("reference-target").await;
    for references in [
        json!([{"sessionId":"missing"}]),
        json!([{"sessionId":false}]),
        json!([{"sessionId":"reference-source","label":false}]),
        json!([{"sessionId":"reference-source","extra":true}]),
        json!(vec![json!({"sessionId":"reference-source"}); 33]),
    ] {
        ws_send(&mut ws,json!({"type":"prompt","sessionId":"reference-target","text":"BAD-REFERENCE","sessionRefs":references})).await;
        until_frame(&mut ws, "input_rejected").await;
        assert_eq!(h.snapshot("reference-target").await, baseline);
        assert_eq!(remote.captured().len(), 1);
    }
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"reference-target","text":"BAD-QUOTE","displayMessage":{"quotedText":1}})).await;
    let (rejection, _) = until_frame(&mut ws, "input_rejected").await;
    assert_eq!(rejection["code"], "invalid_quote");
    assert_eq!(h.snapshot("reference-target").await, baseline);
    let (code, device) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/devices/credentials",
            Some(json!({"userId":"reference-foreign","scopes":["chat"]})),
        )
        .await;
    assert_eq!(code, 201);
    let foreign = ApiClient {
        addr: h.server.addr,
        token: device["secret"].as_str().unwrap().into(),
    };
    let (code, value) = foreign
        .request(
            "POST",
            "/lingxi/v1/sessions",
            Some(json!({"sessionId":"foreign-target","agentId":"lingxi","title":"foreign target"})),
        )
        .await;
    assert_eq!(code, 201, "{value}");
    let foreign_before = h.snapshot("foreign-target").await;
    let source_before = h.snapshot("reference-source").await;
    let mut foreign_ws = ws_connect_api(&foreign).await;
    ws_send(&mut foreign_ws,json!({"type":"prompt","sessionId":"foreign-target","text":"FORBIDDEN-REFERENCE","sessionRefs":[{"sessionId":"reference-source","label":"claim"}]})).await;
    let (rejection, _) = until_frame(&mut foreign_ws, "input_rejected").await;
    assert_eq!(rejection["code"], "session_reference_forbidden");
    assert!(!rejection
        .to_string()
        .contains("REFERENCE-SOURCE-SECRET-NOT-INHERITED"));
    assert_eq!(h.snapshot("foreign-target").await, foreign_before);
    assert_eq!(h.snapshot("reference-source").await, source_before);
    assert_eq!(remote.captured().len(), 1);
    h.stop().await;
    remote.stop();
}
