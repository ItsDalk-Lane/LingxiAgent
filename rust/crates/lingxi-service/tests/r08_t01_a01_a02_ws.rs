//! R08-T01 场景 A01/A02：旧 Electron 客户端接入现役 Rust 内核的
//! 端到端真链验收（真实 HTTP/WS、现役 Run、ModelGateway、持久历史；
//! 仅外部模型供应商用确定性 loopback 替身，其余零替身）。
//!
//! A01「旧客户端不用猜新状态」：一次真实 WS prompt 在同一条 Run 内
//! 顺序执行 read → write → edit → exec_command 四个真实工具后给出
//! 终结回答；断言直播事件序、toolCallId/run/session 一致性、终态由
//! Rust 事件单源给出，并核对持久历史（canonical-history.v1）逐卡片
//! 形状、分页与 revision==ETag。全部归一化输出对接口 golden。
//!
//! A02「重连不重新执行」：副作用工具真实落盘后断线，重连旧协议
//! （WS hello + resume_stream）只续读状态：重复 resume、过期游标、
//! 未知流、服务重启四路，工具调用计数（SQL 工具消息行、历史
//! tool_call 条目、供应商线数、exec 标记文件行数）恒不变，
//! 证明零二次副作用。重启后 resume 的诚实行为（events:[]、
//! nextSeq:1、reset:false）按现行契约钉死，旧客户端随后走历史
//! 重建（属 T01 已验收通道），不在此处虚构恢复能力。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

const PROMPT: &str = "R08-A01-TOOLCHAIN";

fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r08-a01a02-{tag}-{}-{}",
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

impl TestServer {
    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.handle.await;
    }
}

impl ApiClient {
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let (status, _, body) = self.request_raw(method, path, body).await;
        let parsed = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap_or(Value::Null)
        };
        (status, parsed)
    }

    async fn request_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        let payload = body
            .as_ref()
            .map(serde_json::to_vec)
            .unwrap_or_else(|| Ok(Vec::new()))
            .unwrap();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.addr,
            self.token,
            payload.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        stream.write_all(&payload).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8_lossy(&raw).to_string();
        let (head, body) = text.split_once("\r\n\r\n").expect("http response");
        let status = head
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u16>()
            .unwrap();
        let headers = head
            .lines()
            .skip(1)
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            })
            .collect::<Vec<_>>();
        (status, headers, body.as_bytes().to_vec())
    }
}

/// 确定性供应商替身：按请求中 `role:"tool"` 消息数依次驱动
/// read → write → edit → exec_command → 终结回答（模型直调工具名，
/// 与现役 provider tool_calls 形状一致）；仅 `chat` 模型走工具链脚本，title 等
/// 辅助模型拿最小终结帧。每个请求体都留在 `wire` 里供计数断言。
struct Provider {
    addr: SocketAddr,
    wire: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Provider {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let wire = Arc::new(Mutex::new(Vec::new()));
        let w = Arc::clone(&wire);
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let w = Arc::clone(&w);
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
                    let model = body["model"].as_str().unwrap_or("").to_string();
                    let tool_prompt = body["messages"].to_string().contains(PROMPT);
                    let tool_count = body["messages"]
                        .as_array()
                        .map(|rows| rows.iter().filter(|m| m["role"] == "tool").count())
                        .unwrap_or(0);
                    let payload = if model == "chat" && tool_prompt && tool_count < 4 {
                        let (tool, args, ident) = match tool_count {
                            0 => ("read", json!({"path":"a01-seed.txt"}), "call-read"),
                            1 => (
                                "write",
                                json!({"path":"a01-written.txt","content":"A01-WRITTEN-v1\n"}),
                                "call-write",
                            ),
                            2 => (
                                "edit",
                                json!({"path":"a01-written.txt","edits":[{"oldText":"v1","newText":"v2"}]}),
                                "call-edit",
                            ),
                            _ => (
                                "exec_command",
                                json!({"cmd":"echo A01-EXEC-MARKER >> exec-marker.txt"}),
                                "call-exec",
                            ),
                        };
                        let frame = json!({"id":"r08-tool","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":ident,"type":"function","function":{"name":tool,"arguments":args.to_string()}}]},"finish_reason":null}]});
                        format!(
                            "data: {frame}\n\ndata: {}\n\ndata: [DONE]\n\n",
                            json!({"id":"r08-tool","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})
                        )
                    } else {
                        let text = if model == "chat" && tool_prompt {
                            "A01-FINAL-ANSWER"
                        } else {
                            "R08-AUX"
                        };
                        let mut payload = String::new();
                        for frame in [
                            json!({"id":"r08-final","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
                            json!({"id":"r08-final","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                            json!({"id":"r08-final","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":10}}),
                        ] {
                            payload.push_str(&format!("data: {frame}\n\n"));
                        }
                        payload.push_str("data: [DONE]\n\n");
                        payload
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self { addr, wire, task }
    }

    fn captured(&self) -> Vec<Value> {
        self.wire.lock().unwrap().clone()
    }

    fn chat_requests(&self) -> Vec<Value> {
        self.captured()
            .into_iter()
            .filter(|body| body["model"] == "chat")
            .collect()
    }

    fn stop(self) {
        self.task.abort();
    }
}

struct Boot {
    server: TestServer,
    state: ServiceState,
    home: PathBuf,
}

fn fixture(home: &Path) {
    let agent = home.join("agents/lingxi");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("config.yaml"),"locale: zh-CN\nagent:\n  name: R08-A01\n  yuan: lingxi\nmemory:\n  enabled: false\ncompaction:\n  enabled: false\n").unwrap();
}

async fn boot_service(home: PathBuf, provider: &Provider) -> Boot {
    let layout = prepare_layout(&home).unwrap();
    let config = json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":format!("http://{}/v1",provider.addr),"auth":{"kind":"apiKey","apiKey":"R08_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"title":{"provider":"remote","model":"title","capabilities":{"tools":false},"compat":{"contextWindow":200000}}}});
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
                    os_release: String::from("r08"),
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
    Boot {
        server,
        state,
        home,
    }
}

impl Boot {
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
        Duration::from_secs(15),
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

async fn ws_connect(api: &ApiClient) -> TcpStream {
    let mut stream = TcpStream::connect(api.addr).await.unwrap();
    let request = format!("GET /lingxi/v1/ws HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {}\r\n\r\n", api.addr, api.token);
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut b = [0];
        stream.read_exact(&mut b).await.unwrap();
        head.push(b[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 101"));
    ws_send(&mut stream, json!({"protocol":"lingxi.wire","clientKind":"old-electron","clientVersion":"0","protocolMin":1,"protocolMax":1})).await;
    assert_eq!(ws_json(&mut stream).await["protocol"], "lingxi.wire");
    stream
}

async fn until_frame(ws: &mut TcpStream, kind: &str) -> (Value, Vec<Value>) {
    let mut frames = Vec::new();
    for _ in 0..400 {
        let v = ws_json(ws).await;
        frames.push(v.clone());
        if v["type"] == kind {
            return (v, frames);
        }
    }
    panic!("missing {kind}: {frames:?}")
}

/// 旧客户端的一次 prompt：跑完 run 直到 Rust 事件源给出 idle。
async fn prompt_done(ws: &mut TcpStream, id: &str, text: &str) -> Vec<Value> {
    ws_send(ws, json!({"type":"prompt","sessionId":id,"text":text})).await;
    let (_, mut frames) = until_frame(ws, "assistant_run_end").await;
    let (idle, tail) = until_frame(ws, "status").await;
    assert_eq!(idle["isStreaming"], false);
    frames.extend(tail);
    frames
}

async fn key_event_count(state: &ServiceState, run_id: &str, event_type: &str) -> i64 {
    state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM key_events WHERE run_id=?1 AND event_type=?2",
            vec![run_id.into(), event_type.into()],
        )
        .await
        .unwrap()
        .unwrap()
        .parse::<i64>()
        .unwrap()
}

async fn run_status(state: &ServiceState, run_id: &str) -> String {
    state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id=?1",
            vec![run_id.into()],
        )
        .await
        .unwrap()
        .expect("run 行必须存在")
}

async fn run_count(state: &ServiceState, session: &str) -> i64 {
    state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE session_id=?1",
            vec![session.into()],
        )
        .await
        .unwrap()
        .unwrap()
        .parse::<i64>()
        .unwrap()
}

fn marker_lines(home: &Path) -> usize {
    let text = std::fs::read_to_string(home.join("exec-marker.txt")).unwrap();
    text.lines()
        .filter(|line| *line == "A01-EXEC-MARKER")
        .count()
}

/// 归一化：id/时间戳/etag 类不稳定字段替换成占位，结构与其余内容
/// 原样保留，供 golden 精确比对（禁占位跳过：替换只针对名称命中
/// 的不稳定键，结构漂移会在 golden diff 中显红）。
fn normalize(value: &Value) -> Value {
    const ID_KEYS: &[&str] = &[
        "id",
        "runId",
        "streamId",
        "messageId",
        "segmentId",
        "modelCallId",
        "resumeToken",
        "turnId",
        "turnInputEntryId",
        "sourceEntryId",
        "toolCallId",
    ];
    const TS_KEYS: &[&str] = &["ts", "committedAtUnixMs", "timestamp", "revision", "etag"];
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, v) in map {
                // argsDigest 是参数（内含唯一临时 home 路径）的内容摘要，
                // 逐次运行不同；摘要字段存在性与算法形状由上方断言钉死。
                if key == "argsDigest" {
                    out.insert(key.clone(), json!("<argsDigest>"));
                } else if ID_KEYS.contains(&key.as_str()) && !v.is_null() {
                    out.insert(key.clone(), json!(format!("<{key}>")));
                } else if TS_KEYS.contains(&key.as_str()) && !v.is_null() {
                    out.insert(key.clone(), json!(format!("<{key}>")));
                } else {
                    out.insert(key.clone(), normalize(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(rows) => Value::Array(rows.iter().map(normalize).collect()),
        // 字符串值内嵌唯一临时工作区路径（resourceId/uri 等）→ 整体占位。
        Value::String(text)
            if text.contains("lingxi-r07-verify") || text.contains("lingxi-r08-a01a02") =>
        {
            json!("<workspace-path>")
        }
        other => other.clone(),
    }
}

fn capture_dir() -> Option<PathBuf> {
    std::env::var("LINGXI_R08_CAPTURE_DIR")
        .ok()
        .map(PathBuf::from)
}

fn write_capture(name: &str, value: &Value) {
    if let Some(dir) = capture_dir() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), serde_json::to_string_pretty(value).unwrap()).unwrap();
    }
}

struct A01Outcome {
    frames: Vec<Value>,
    history: Value,
    etag: String,
    home: PathBuf,
}

async fn run_toolchain_session(tag: &str, session: &str) -> (Boot, Provider, A01Outcome) {
    let home = unique_dir(tag);
    fixture(&home);
    std::fs::write(home.join("a01-seed.txt"), "A01-SEED-CONTENT\n").unwrap();
    let provider = Provider::start().await;
    let boot = Boot::create_session(home, &provider, session).await;
    let mut ws = ws_connect(&boot.server).await;
    let frames = prompt_done(&mut ws, session, PROMPT).await;
    let (code, headers, body) = boot
        .server
        .request_raw(
            "GET",
            &format!("/lingxi/v1/sessions/{session}/history?all=1"),
            None,
        )
        .await;
    assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
    let history: Value = serde_json::from_slice(&body).unwrap();
    let etag = headers
        .iter()
        .find(|(k, _)| k == "etag")
        .map(|(_, v)| v.clone())
        .expect("history revision etag header");
    drop(ws);
    let home = boot.home.clone();
    (
        boot,
        provider,
        A01Outcome {
            frames,
            history,
            etag,
            home,
        },
    )
}

impl Boot {
    async fn create_session(home: PathBuf, provider: &Provider, session: &str) -> Boot {
        let boot = boot_service(home, provider).await;
        boot.create(session).await;
        boot
    }
}

fn assert_history_shape(history: &Value, session: &str, etag: &str) -> Vec<String> {
    assert_eq!(history["schemaVersion"], "lingxi.canonical-history.v1");
    assert_eq!(history["sessionId"], session);
    let revision = history["revision"].as_str().expect("revision string");
    assert!(!revision.is_empty());
    assert_eq!(revision, etag, "revision 必须与 ETag 同源");
    assert_eq!(history["page"]["hasMore"], false);
    assert!(history["page"]["nextBefore"].is_null());
    let items = history["items"].as_array().unwrap();
    let users = items
        .iter()
        .filter(|v| v["kind"] == "user_message")
        .collect::<Vec<_>>();
    assert_eq!(users.len(), 1, "{items:?}");
    assert!(users[0]["text"].as_str().unwrap().contains(PROMPT));
    let tools = items
        .iter()
        .filter(|v| v["kind"] == "tool_call")
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 4, "{items:?}");
    let names = tools
        .iter()
        .map(|v| v["target"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "tool:first-party:read",
            "tool:first-party:write",
            "tool:first-party:edit",
            "tool:first-party:exec_command"
        ],
        "{tools:?}"
    );
    let mut history_tool_ids = Vec::new();
    for tool in &tools {
        assert_eq!(tool["status"], "success", "{tool:?}");
        assert!(tool["result"].is_object(), "工具结果缺失: {tool:?}");
        let id = tool["toolCallId"].as_str().unwrap();
        assert!(!id.is_empty(), "{tool:?}");
        history_tool_ids.push(id.to_string());
    }
    let mut dedup = history_tool_ids.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(
        dedup.len(),
        4,
        "toolCallId 必须稳定唯一: {history_tool_ids:?}"
    );
    let finals = items
        .iter()
        .filter(|v| v["kind"] == "final_message")
        .collect::<Vec<_>>();
    assert_eq!(finals.len(), 1, "{items:?}");
    assert!(finals[0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .any(|block| block["type"] == "text"
            && block["text"]
                .as_str()
                .unwrap_or("")
                .contains("A01-FINAL-ANSWER")),);
    assert!(finals[0]["modelCallId"].as_str().is_some());
    // 终态诚实契约：有 final 的 run 不发 run_terminal（终态由 final +
    // runs 表状态单源给出，协议历史不重复编造终结条目）。
    let terminals = items
        .iter()
        .filter(|v| v["kind"] == "run_terminal")
        .collect::<Vec<_>>();
    assert_eq!(terminals.len(), 0, "{items:?}");
    // run 归属一致：所有 run 绑定条目指向同一 runId。
    let run = finals[0]["run"]["runId"]
        .as_str()
        .expect("final 必须绑定 run");
    for tool in &tools {
        assert_eq!(tool["run"]["state"], "known", "{tool:?}");
        assert_eq!(tool["run"]["runId"], run, "{tool:?}");
    }
    assert_eq!(finals[0]["run"]["runId"], run);
    history_tool_ids
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a01_old_client_events_and_history_come_from_rust_single_source() {
    let (boot, provider, outcome) = run_toolchain_session("a01", "a01").await;
    let frames = &outcome.frames;

    // ── 直播事件：四工具 start/end 配对、名称序、终结由事件单源给出 ──
    let starts = frames
        .iter()
        .filter(|v| v["type"] == "tool_start")
        .collect::<Vec<_>>();
    let ends = frames
        .iter()
        .filter(|v| v["type"] == "tool_end")
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 4, "{frames:?}");
    assert_eq!(ends.len(), 4, "{frames:?}");
    let start_names = starts
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        start_names,
        vec![
            "tool:first-party:read",
            "tool:first-party:write",
            "tool:first-party:edit",
            "tool:first-party:exec_command"
        ],
        "{starts:?}"
    );
    let start_ids = starts
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let end_ids = ends
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(start_ids, end_ids, "start/end 工具调用 id 必须一一配对");
    for end in &ends {
        assert_eq!(end["success"], true, "{end:?}");
        assert_eq!(end["status"], "succeeded", "{end:?}");
    }
    // run 归属：一条 Run，全程一致（工具/段帧以 turnId 绑定）；终态帧
    // 存在且其后状态 idle。
    let run = frames
        .iter()
        .find(|v| v["type"] == "assistant_run_start")
        .expect("assistant_run_start")["runId"]
        .as_str()
        .unwrap()
        .to_string();
    for frame in frames.iter().filter(|v| {
        matches!(
            v["type"].as_str(),
            Some("tool_start") | Some("tool_end") | Some("assistant_segment_delta")
        )
    }) {
        assert_eq!(frame["turnId"].as_str().unwrap(), run, "{frame:?}");
    }
    let run_end = frames
        .iter()
        .position(|v| v["type"] == "assistant_run_end")
        .unwrap();
    assert!(frames[run_end..]
        .iter()
        .any(|v| v["type"] == "status" && v["isStreaming"] == false));
    // seq 连续递增（旧客户端按序续读的底层保证）。
    let seqs = frames
        .iter()
        .filter_map(|v| v["seq"].as_u64())
        .collect::<Vec<_>>();
    assert!(seqs.len() > 10, "{seqs:?}");
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    assert!(
        frames.iter().any(|v| v["type"] == "assistant_segment_delta"
            && v["delta"]
                .as_str()
                .unwrap_or("")
                .contains("A01-FINAL-ANSWER")),
        "{frames:?}"
    );

    // ── 真实副作用落盘 ──
    assert_eq!(
        std::fs::read_to_string(outcome.home.join("a01-written.txt")).unwrap(),
        "A01-WRITTEN-v2\n"
    );
    assert_eq!(marker_lines(&outcome.home), 1);

    // ── 持久历史（ reopened 视图）：逐卡片形状；跨通道 toolCallId 一致 ──
    let history_tool_ids = assert_history_shape(&outcome.history, "a01", &outcome.etag);
    assert_eq!(history_tool_ids, start_ids, "历史与直播必须同源同 id");

    // ── 供应商线：5 次 chat 请求，tool 消息数 0..4 递增 ──
    let chat = provider.chat_requests();
    assert_eq!(chat.len(), 5, "{chat:?}");
    for (index, body) in chat.iter().enumerate() {
        let tools = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "tool")
            .count();
        assert_eq!(
            tools,
            index,
            "第 {} 次 chat 请求应携带 {} 条 tool 消息",
            index + 1,
            index
        );
    }
    assert!(chat[4]["messages"].to_string().contains("A01-SEED-CONTENT"));
    // 工具调用回执账本：started/completed 各四张，Run 唯一。
    assert_eq!(
        key_event_count(&boot.state, &run, "tool_call_started").await,
        4
    );
    assert_eq!(
        key_event_count(&boot.state, &run, "tool_call_completed").await,
        4
    );
    assert_eq!(run_count(&boot.state, "a01").await, 1);
    assert_eq!(run_status(&boot.state, &run).await, "completed");

    // ── 接口 golden + 原始捕获（供前端投影测试夹具与审阅）──
    let normalized = normalize(&json!({
        "events": frames,
        "history": outcome.history,
        "etag": outcome.etag,
        "files": {
            "a01-written.txt": "A01-WRITTEN-v2\n",
            "exec-marker.txt": "A01-EXEC-MARKER\n",
        },
        "providerChatRequests": chat.len(),
    }));
    write_capture("a01-history-raw.json", &outcome.history);
    write_capture(
        "a01-events-raw.json",
        &json!({ "events": frames, "etag": outcome.etag }),
    );
    write_capture("a01-golden-normalized.json", &normalized);
    let golden_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r08_a01_ws_golden.json");
    if std::env::var("LINGXI_R08_A01_GOLDEN_WRITE").is_ok() {
        std::fs::write(
            &golden_path,
            serde_json::to_string_pretty(&normalized).unwrap(),
        )
        .unwrap();
    } else {
        let golden: Value = serde_json::from_str(
            &std::fs::read_to_string(&golden_path)
                .expect("golden 缺失：先以 LINGXI_R08_A01_GOLDEN_WRITE=1 生成并人工审阅"),
        )
        .unwrap();
        assert_eq!(normalized, golden, "接口 golden 漂移");
    }

    let home = boot.stop().await;
    provider.stop();
    drop(home);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a02_reconnect_resume_replays_without_any_reexecution() {
    let (boot, provider, outcome) = run_toolchain_session("a02", "a02").await;

    // ── 基线计数：四类工具调用证据各就各位 ──
    let run = outcome
        .frames
        .iter()
        .find(|v| v["type"] == "assistant_run_start")
        .expect("assistant_run_start")["runId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        key_event_count(&boot.state, &run, "tool_call_started").await,
        4
    );
    assert_eq!(
        key_event_count(&boot.state, &run, "tool_call_completed").await,
        4
    );
    assert_eq!(run_count(&boot.state, "a02").await, 1);
    assert_eq!(run_status(&boot.state, &run).await, "completed");
    assert_eq!(provider.chat_requests().len(), 5);
    assert_eq!(marker_lines(&outcome.home), 1);
    assert_history_shape(&outcome.history, "a02", &outcome.etag);

    let stream = outcome
        .frames
        .iter()
        .find_map(|v| v["streamId"].as_str())
        .expect("stream id")
        .to_string();
    let last_seq = outcome
        .frames
        .iter()
        .filter_map(|v| v["seq"].as_u64())
        .max()
        .expect("frames carry seq");

    // ── 断线 → 重连旧协议订阅（WS hello + resume_stream）──
    // 游标在流尖：无新事件。
    let mut ws = ws_connect(&boot.server).await;
    ws_send(&mut ws, json!({"type":"resume_stream","sessionId":"a02","streamId":stream,"sinceSeq":last_seq,"resumeToken":"r1"})).await;
    let (tip, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(tip["reset"], false, "{tip:?}");
    assert_eq!(tip["truncated"], false, "{tip:?}");
    assert_eq!(tip["runtimeIsStreaming"], false, "{tip:?}");
    assert_eq!(tip["isStreaming"], false, "{tip:?}");
    assert_eq!(tip["resumeToken"], "r1");
    assert_eq!(tip["nextSeq"], last_seq + 1, "{tip:?}");
    assert_eq!(tip["events"], json!([]), "流尖游标不得回吐事件: {tip:?}");

    // ── 游标回退三格：恰好回放尾部三帧，无工具帧 ──
    // （工具帧位于 seq 5/7/9/11，尾三帧是 segment_end/run_end/status。）
    ws_send(&mut ws, json!({"type":"resume_stream","sessionId":"a02","streamId":stream,"sinceSeq":last_seq-3,"resumeToken":"r2"})).await;
    let (tail, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(tail["reset"], false, "{tail:?}");
    assert_eq!(tail["resumeToken"], "r2");
    let tail_events = tail["events"].as_array().unwrap();
    let tail_seqs = tail_events
        .iter()
        .map(|v| v["seq"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        tail_seqs,
        vec![last_seq - 2, last_seq - 1, last_seq],
        "{tail_events:?}"
    );
    let tail_types = tail_events
        .iter()
        .map(|v| v["event"]["type"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        tail_types,
        vec!["assistant_segment_end", "assistant_run_end", "status"],
        "{tail_events:?}"
    );

    // ── 中流游标：回放含工具卡片帧（旧客户端据此重绘），
    //    但任何回放都不触发第二次工具副作用（下方计数核对）──
    ws_send(&mut ws, json!({"type":"resume_stream","sessionId":"a02","streamId":stream,"sinceSeq":6,"resumeToken":"r3"})).await;
    let (mid, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(mid["reset"], false, "{mid:?}");
    let mid_events = mid["events"].as_array().unwrap();
    let mid_seqs = mid_events
        .iter()
        .map(|v| v["seq"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        mid_seqs,
        (7..=last_seq).collect::<Vec<u64>>(),
        "中流游标必须恰好回放 7..=last: {mid_seqs:?}"
    );
    assert!(
        mid_events.iter().any(|v| v["event"]["type"] == "tool_end"),
        "中流回放必须携带工具卡片帧: {mid_events:?}"
    );

    // ── 重复 resume：幂等回放，逐字节一致 ──
    ws_send(&mut ws, json!({"type":"resume_stream","sessionId":"a02","streamId":stream,"sinceSeq":last_seq-3,"resumeToken":"r2b"})).await;
    let (again, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(again["reset"], false);
    assert_eq!(again["events"], tail["events"]);

    // ── 过期游标（超过 nextSeq）：诚实 reset 并从头全量回放
    //    （chat_wire.rs:1416-1434 钉死的现行契约；回放是只读重绘数据）──
    ws_send(
        &mut ws,
        json!({"type":"resume_stream","sessionId":"a02","streamId":stream,"sinceSeq":999999}),
    )
    .await;
    let (stale, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(stale["reset"], true, "{stale:?}");
    assert_eq!(stale["sinceSeq"], 0, "{stale:?}");
    assert_eq!(stale["truncated"], false, "{stale:?}");
    let stale_events = stale["events"].as_array().unwrap();
    assert_eq!(stale_events.len() as u64, last_seq, "{stale_events:?}");
    assert_eq!(stale_events[0]["seq"], 1, "{stale_events:?}");

    // ── 未知流：同样诚实 reset + 全量回放 ──
    ws_send(
        &mut ws,
        json!({"type":"resume_stream","sessionId":"a02","streamId":"obsolete-stream","sinceSeq":0}),
    )
    .await;
    let (unknown, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(unknown["reset"], true, "{unknown:?}");
    assert_eq!(unknown["events"], stale["events"], "{unknown:?}");

    // ── 四路计数核对：重连续读零二次副作用 ──
    assert_eq!(
        key_event_count(&boot.state, &run, "tool_call_started").await,
        4,
        "重连后工具回执翻倍"
    );
    assert_eq!(
        key_event_count(&boot.state, &run, "tool_call_completed").await,
        4,
        "重连后工具回执翻倍"
    );
    assert_eq!(run_count(&boot.state, "a02").await, 1, "重连后产生新 Run");
    assert_eq!(provider.chat_requests().len(), 5, "重连后供应商被再次请求");
    assert_eq!(marker_lines(&outcome.home), 1, "exec 副作用重放");
    assert_eq!(
        std::fs::read_to_string(outcome.home.join("a01-written.txt")).unwrap(),
        "A01-WRITTEN-v2\n"
    );
    drop(ws);

    // ── 服务重启：回放缓冲为内存态，resume 诚实报空 ──
    let home = boot.stop().await;
    provider.stop();
    let provider2 = Provider::start().await;
    let boot2 = boot_service(home.clone(), &provider2).await;
    let mut ws = ws_connect(&boot2.server).await;
    ws_send(&mut ws, json!({"type":"resume_stream","sessionId":"a02","streamId":stream,"sinceSeq":last_seq,"resumeToken":"r3"})).await;
    let (after, _) = until_frame(&mut ws, "stream_resume").await;
    // 现行契约钉死：重启后内存回放缓冲为空 → events:[], nextSeq:1,
    // reset:false, truncated:false。旧客户端随后走历史重建通道
    // （T01 已验收），不得在此处虚构“跨重启续播”。
    assert_eq!(after["events"], json!([]), "{after:?}");
    assert_eq!(after["nextSeq"], 1, "{after:?}");
    assert_eq!(after["reset"], false, "{after:?}");
    assert_eq!(after["truncated"], false, "{after:?}");
    assert_eq!(after["runtimeIsStreaming"], false, "{after:?}");
    assert_eq!(after["isStreaming"], false, "{after:?}");

    // ── 重启后重复 resume 同样幂等 ──
    ws_send(
        &mut ws,
        json!({"type":"resume_stream","sessionId":"a02","streamId":stream,"sinceSeq":last_seq}),
    )
    .await;
    let (after2, _) = until_frame(&mut ws, "stream_resume").await;
    assert_eq!(after2["events"], json!([]), "{after2:?}");

    // ── 重启后计数与历史：零重执行、零丢失 ──
    assert_eq!(
        provider2.chat_requests().len(),
        0,
        "重启后 resume 触发供应商调用"
    );
    assert_eq!(
        key_event_count(&boot2.state, &run, "tool_call_started").await,
        4
    );
    assert_eq!(
        key_event_count(&boot2.state, &run, "tool_call_completed").await,
        4
    );
    assert_eq!(run_count(&boot2.state, "a02").await, 1);
    assert_eq!(marker_lines(&home), 1);
    let (code, headers, body) = boot2
        .server
        .request_raw("GET", "/lingxi/v1/sessions/a02/history?all=1", None)
        .await;
    assert_eq!(code, 200);
    let history: Value = serde_json::from_slice(&body).unwrap();
    let etag = headers
        .iter()
        .find(|(k, _)| k == "etag")
        .map(|(_, v)| v.clone())
        .unwrap();
    assert_history_shape(&history, "a02", &etag);
    write_capture("a02-history-after-restart.json", &history);

    drop(ws);
    boot2.stop().await;
    provider2.stop();
}
