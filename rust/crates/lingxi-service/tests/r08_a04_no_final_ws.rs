//! R08-T02 场景 A04「卡片展开不退化」：只有工具结果、没有最终回复的会话，
//! 端到端真链验收（真实 HTTP/WS、现役 Run、ModelGateway、持久历史；仅外部
//! 模型供应商用确定性 loopback 替身，其余零替身）。
//!
//! 场景脚本：供应商替身驱动 1) 先输出 reasoning + 过程叙述并发起一次成功
//! 的 read；2) 第二轮再发起一次必然失败的 read（文件不存在，ENOENT 由真
//! 实文件系统给出）；3) 第三轮返回空终结（finish_reason: stop、零内容、零
//! 工具调用）。内核据此产出诚实终态 `completed.no_final.tool_partial_failure`，
//! 全程无 final message。
//!
//! 断言面：直播事件里成功/失败 tool_end 的 success/status/error 字段与
//! reasoning/text 段帧；持久历史（canonical-history.v1）里 run_terminal
//! 条目存在且字段诚实、失败 tool_call 条目的 result.error 完整、零
//! final_message 条目；runs 表 status/terminal_reason 与 key_events 账本；
//! 归一化输出对接口 golden。原始历史捕获同时充当桌面投影测试的夹具来源。
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

const PROMPT: &str = "R08-A04-NO-FINAL";
const SEED_NAME: &str = "a04-seed.txt";
const SEED_CONTENT: &str = "A04-SEED-CONTENT\n";
const MISSING_NAME: &str = "a04-missing.txt";
/// 真实文件系统 ENOENT 经 filetools 原样转写（raw 参数路径，无临时路径泄漏）。
const MISSING_ERROR: &str = "Could not read file: a04-missing.txt. Error code: ENOENT.";
const NO_FINAL_REASON: &str = "completed.no_final.tool_partial_failure";

fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r08-a04-{tag}-{}-{}",
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

/// 确定性供应商替身：按请求中 `role:"tool"` 消息数推进三幕——
///   0 条：reasoning + 过程叙述 + read 成功（读种子文件）；
///   1 条：read 必然失败（目标文件不存在，ENOENT 真实产生）；
///   ≥2 条：空终结（stop、零内容、零工具调用）→ ProviderTurn::Empty。
/// 失败的工具结果同样回灌为 tool 消息（model_history::render_tool_outcome_text
/// 不区分成败），所以第三轮请求恰携带两条 tool 消息。仅 `chat` 模型走该
/// 脚本，title 等辅助模型拿最小终结帧。
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
                    let body: Value =
                        serde_json::from_slice(&raw[head..head + len]).expect("provider body");
                    w.lock().unwrap().push(body.clone());
                    let model = body["model"].as_str().unwrap_or("");
                    let tool_prompt = body["messages"].to_string().contains(PROMPT);
                    let tool_count = body["messages"]
                        .as_array()
                        .map(|rows| rows.iter().filter(|m| m["role"] == "tool").count())
                        .unwrap_or(0);
                    let payload = if model == "chat" && tool_prompt {
                        match tool_count {
                            // 第一幕：reasoning + 过程叙述 + 成功 read。reasoning_content
                            // 与 content 同轮到达，证明流式聚合器按序收拢两类过程内容。
                            0 => sse_join(&[
                                json!({"id":"r08-a04-t1","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"A04-THINKING-ALOUD"},"finish_reason":null}]}),
                                json!({"id":"r08-a04-t1","choices":[{"index":0,"delta":{"content":"A04-READING-SEED"},"finish_reason":null}]}),
                                json!({"id":"r08-a04-t1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-read-ok","type":"function","function":{"name":"read","arguments":json!({"path":SEED_NAME}).to_string()}}]},"finish_reason":null}]}),
                                json!({"id":"r08-a04-t1","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
                            ]),
                            // 第二幕：read 不存在的文件，真实 ENOENT。
                            1 => sse_join(&[
                                json!({"id":"r08-a04-t2","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-read-missing","type":"function","function":{"name":"read","arguments":json!({"path":MISSING_NAME}).to_string()}}]},"finish_reason":null}]}),
                                json!({"id":"r08-a04-t2","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
                            ]),
                            // 第三幕：空终结——stop、零内容、零工具调用。
                            // openai_completions 据此分类 ProviderTurn::Empty，
                            // runs.rs finish_no_final 归因 tool 部分失败。
                            _ => sse_join(&[
                                json!({"id":"r08-a04-t3","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                                json!({"id":"r08-a04-t3","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":0}}),
                            ]),
                        }
                    } else {
                        sse_join(&[
                            json!({"id":"r08-aux","choices":[{"index":0,"delta":{"role":"assistant","content":"R08-AUX"},"finish_reason":null}]}),
                            json!({"id":"r08-aux","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                            json!({"id":"r08-aux","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1}}),
                        ])
                    };
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", payload.len(), payload);
                    socket.write_all(response.as_bytes()).await.unwrap();
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

fn sse_join(frames: &[Value]) -> String {
    let mut out = String::new();
    for frame in frames {
        out.push_str("data: ");
        out.push_str(&frame.to_string());
        out.push_str("\n\n");
    }
    out.push_str("data: [DONE]\n\n");
    out
}

struct Boot {
    server: TestServer,
    state: ServiceState,
    home: PathBuf,
}

fn fixture(home: &Path) {
    let agent = home.join("agents/lingxi");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("config.yaml"),"locale: zh-CN\nagent:\n  name: R08-A04\n  yuan: lingxi\nmemory:\n  enabled: false\ncompaction:\n  enabled: false\n").unwrap();
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

    async fn create_session(home: PathBuf, provider: &Provider, session: &str) -> Boot {
        let boot = boot_service(home, provider).await;
        boot.create(session).await;
        boot
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

/// runs 表终态双字段：status 与 terminal_reason 必须与历史 run_terminal 同源。
async fn run_terminal_row(state: &ServiceState, run_id: &str) -> (String, Option<String>) {
    let status = state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id=?1",
            vec![run_id.into()],
        )
        .await
        .unwrap()
        .expect("run 行必须存在");
    let reason = state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id=?1",
            vec![run_id.into()],
        )
        .await
        .unwrap();
    (status, reason)
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
            if text.contains("lingxi-r07-verify") || text.contains("lingxi-r08-a04") =>
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

struct A04Outcome {
    frames: Vec<Value>,
    history: Value,
    etag: String,
    home: PathBuf,
}

async fn run_no_final_session() -> (Boot, Provider, A04Outcome) {
    let home = unique_dir("a04");
    fixture(&home);
    std::fs::write(home.join(SEED_NAME), SEED_CONTENT).unwrap();
    // 注意：MISSING_NAME 刻意不创建——失败 read 的 ENOENT 必须真实发生。
    let provider = Provider::start().await;
    let boot = Boot::create_session(home, &provider, "a04").await;
    let mut ws = ws_connect(&boot.server).await;
    let frames = prompt_done(&mut ws, "a04", PROMPT).await;
    let (code, headers, body) = boot
        .server
        .request_raw("GET", "/lingxi/v1/sessions/a04/history?all=1", None)
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
        A04Outcome {
            frames,
            history,
            etag,
            home,
        },
    )
}

/// 持久历史逐卡片形状断言；返回历史 toolCallId 序（与直播同序同源）。
fn assert_history_shape(history: &Value, etag: &str) -> (Vec<String>, String) {
    assert_eq!(history["schemaVersion"], "lingxi.canonical-history.v1");
    assert_eq!(history["sessionId"], "a04");
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

    // 两条助手段：reasoning（思考）与未终结的过程叙述（unresolved，绝不
    // 升格为 final_answer——没有 driver 确认的 final）。
    let segments = items
        .iter()
        .filter(|v| v["kind"] == "assistant_segment")
        .collect::<Vec<_>>();
    assert_eq!(segments.len(), 2, "{items:?}");
    let reasoning = segments
        .iter()
        .find(|v| v["segmentKind"] == "reasoning")
        .expect("reasoning 段必须存在");
    assert_eq!(reasoning["phase"], "reasoning", "{reasoning:?}");
    assert_eq!(reasoning["complete"], true, "{reasoning:?}");
    assert!(
        reasoning["text"]
            .as_str()
            .unwrap_or("")
            .contains("A04-THINKING-ALOUD"),
        "{reasoning:?}"
    );
    let narration = segments
        .iter()
        .find(|v| v["segmentKind"] == "text")
        .expect("过程叙述段必须存在");
    assert_eq!(narration["phase"], "unresolved", "{narration:?}");
    assert_eq!(narration["complete"], true, "{narration:?}");
    assert!(
        narration["text"]
            .as_str()
            .unwrap_or("")
            .contains("A04-READING-SEED"),
        "{narration:?}"
    );

    // 两条工具调用：一成一败，失败条目的错误诚实可查。
    let tools = items
        .iter()
        .filter(|v| v["kind"] == "tool_call")
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 2, "{items:?}");
    for tool in &tools {
        assert_eq!(tool["target"], "tool:first-party:read", "{tool:?}");
    }
    let ok = tools[0];
    assert_eq!(ok["status"], "success", "{ok:?}");
    assert_eq!(ok["result"]["status"], "success", "{ok:?}");
    assert_eq!(
        ok["result"]["content"][0]["text"].as_str().unwrap_or(""),
        SEED_CONTENT,
        "{ok:?}"
    );
    assert_eq!(
        ok["result"]["details"]["source"]["path"], SEED_NAME,
        "{ok:?}"
    );
    assert!(
        ok["result"]["error"].is_null(),
        "成功调用不得携带 error: {ok:?}"
    );
    let failed = tools[1];
    assert_eq!(failed["status"], "failed", "{failed:?}");
    assert_eq!(failed["result"]["status"], "failed", "{failed:?}");
    assert_eq!(failed["result"]["error"]["code"], "not_found", "{failed:?}");
    assert_eq!(
        failed["result"]["error"]["message"].as_str().unwrap_or(""),
        MISSING_ERROR,
        "{failed:?}"
    );
    // 参数摘要存在且两次调用不同（参数不同 → 摘要不同，禁塌缩）。
    let digest_ok = ok["argsDigest"]["hex"]
        .as_str()
        .expect("argsDigest.hex 必须存在");
    let digest_failed = failed["argsDigest"]["hex"]
        .as_str()
        .expect("argsDigest.hex 必须存在");
    assert_ne!(digest_ok, digest_failed, "不同参数的摘要不得相同");
    let history_tool_ids = tools
        .iter()
        .map(|v| {
            let id = v["toolCallId"].as_str().unwrap();
            assert!(!id.is_empty(), "{v:?}");
            id.to_string()
        })
        .collect::<Vec<_>>();

    // 诚实终态契约：无 final 的 run 恰发一条 run_terminal，字段齐全。
    let finals = items
        .iter()
        .filter(|v| v["kind"] == "final_message")
        .collect::<Vec<_>>();
    assert_eq!(finals.len(), 0, "本场景不得出现 final_message: {items:?}");
    let terminals = items
        .iter()
        .filter(|v| v["kind"] == "run_terminal")
        .collect::<Vec<_>>();
    assert_eq!(terminals.len(), 1, "{items:?}");
    let terminal = terminals[0];
    assert_eq!(terminal["status"], "completed", "{terminal:?}");
    assert_eq!(
        terminal["terminalReason"].as_str().unwrap_or(""),
        NO_FINAL_REASON,
        "{terminal:?}"
    );
    let event_seq = terminal["eventSeq"]
        .as_str()
        .expect("eventSeq 十进制字符串");
    assert!(
        !event_seq.is_empty() && event_seq.chars().all(|c| c.is_ascii_digit()),
        "eventSeq 必须是十进制字符串: {terminal:?}"
    );
    let run = terminal["runId"].as_str().expect("terminal 必须绑定 run");

    // run 归属一致：段/工具条目与 terminal 指向同一 runId。
    for entry in segments.iter().chain(tools.iter()) {
        assert_eq!(entry["run"]["state"], "known", "{entry:?}");
        assert_eq!(entry["run"]["runId"], run, "{entry:?}");
    }
    (history_tool_ids, run.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a04_tool_failure_then_empty_final_records_honest_terminal() {
    let (boot, provider, outcome) = run_no_final_session().await;
    let frames = &outcome.frames;

    // ── 直播事件：两次工具 start/end 配对，成败字段诚实 ──
    let starts = frames
        .iter()
        .filter(|v| v["type"] == "tool_start")
        .collect::<Vec<_>>();
    let ends = frames
        .iter()
        .filter(|v| v["type"] == "tool_end")
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2, "{frames:?}");
    assert_eq!(ends.len(), 2, "{frames:?}");
    for start in &starts {
        assert_eq!(start["name"], "tool:first-party:read", "{start:?}");
    }
    let start_ids = starts
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let end_ids = ends
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(start_ids, end_ids, "start/end 工具调用 id 必须一一配对");
    assert_eq!(ends[0]["success"], true, "{:?}", ends[0]);
    assert_eq!(ends[0]["status"], "succeeded", "{:?}", ends[0]);
    assert!(ends[0]["error"].is_null(), "{:?}", ends[0]);
    assert_eq!(ends[1]["success"], false, "{:?}", ends[1]);
    assert_eq!(ends[1]["status"], "failed", "{:?}", ends[1]);
    assert_eq!(
        ends[1]["error"].as_str().unwrap_or(""),
        MISSING_ERROR,
        "失败 tool_end 的错误必须原文可查: {:?}",
        ends[1]
    );

    // reasoning 与过程叙述的直播段帧都在，且过程叙述永不携带
    // final_answer 相位（本 run 没有 driver 确认的 final）。
    // 实测线格式：kind 只在 assistant_segment_start 帧；delta/end 帧
    // 只携带 semanticPhase（A01 原语同理，本套件以实测帧为准）。
    assert!(
        frames.iter().any(|v| v["type"] == "assistant_segment_start"
            && v["kind"].as_str() == Some("reasoning")
            && v["semanticPhase"].as_str() == Some("reasoning")),
        "{frames:?}"
    );
    assert!(
        frames.iter().any(|v| v["type"] == "assistant_segment_start"
            && v["kind"].as_str() == Some("text")
            && v["semanticPhase"].as_str() == Some("unresolved")),
        "{frames:?}"
    );
    assert!(
        frames.iter().any(|v| v["type"] == "assistant_segment_delta"
            && v["semanticPhase"].as_str() == Some("reasoning")
            && v["delta"]
                .as_str()
                .unwrap_or("")
                .contains("A04-THINKING-ALOUD")),
        "{frames:?}"
    );
    assert!(
        frames.iter().any(|v| v["type"] == "assistant_segment_delta"
            && v["semanticPhase"].as_str() == Some("unresolved")
            && v["delta"]
                .as_str()
                .unwrap_or("")
                .contains("A04-READING-SEED")),
        "{frames:?}"
    );
    for frame in frames.iter().filter(|v| {
        v["type"]
            .as_str()
            .map(|t| t.starts_with("assistant_segment_"))
            .unwrap_or(false)
    }) {
        assert_ne!(
            frame["semanticPhase"].as_str(),
            Some("final_answer"),
            "无 final 的 run 不得出现 final_answer 段帧: {frame:?}"
        );
    }
    // 终态帧存在且其后状态 idle；全程无任何 final_message 帧。
    let run = frames
        .iter()
        .find(|v| v["type"] == "assistant_run_start")
        .expect("assistant_run_start")["runId"]
        .as_str()
        .unwrap()
        .to_string();
    let run_end = frames
        .iter()
        .position(|v| v["type"] == "assistant_run_end")
        .unwrap();
    assert!(frames[run_end..]
        .iter()
        .any(|v| v["type"] == "status" && v["isStreaming"] == false));
    assert!(
        !frames.iter().any(|v| v["type"] == "final_message"),
        "本场景不得出现 final_message 帧: {frames:?}"
    );
    // seq 连续递增（旧客户端按序续读的底层保证）。
    let seqs = frames
        .iter()
        .filter_map(|v| v["seq"].as_u64())
        .collect::<Vec<_>>();
    assert!(seqs.len() > 10, "{seqs:?}");
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");

    // ── 真实副作用与反证：种子内容原样，失败目标从未被创建 ──
    assert_eq!(
        std::fs::read_to_string(outcome.home.join(SEED_NAME)).unwrap(),
        SEED_CONTENT
    );
    assert!(
        !outcome.home.join(MISSING_NAME).exists(),
        "失败的 read 不得产生任何文件"
    );

    // ── 持久历史：逐卡片形状 + 诚实 run_terminal；跨通道 toolCallId 一致 ──
    let (history_tool_ids, history_run) = assert_history_shape(&outcome.history, &outcome.etag);
    assert_eq!(history_tool_ids, start_ids, "历史与直播必须同源同 id");
    assert_eq!(history_run, run, "历史 terminal 与直播 runId 必须同源");

    // ── 供应商线：3 次 chat 请求，tool 消息数 0/1/2 递增 ──
    let chat = provider.chat_requests();
    assert_eq!(chat.len(), 3, "{chat:?}");
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
    // 失败结果同样回灌（模型线可见真实错误，替身据此进入第三幕）。
    assert!(
        chat[2]["messages"].to_string().contains(MISSING_NAME),
        "失败 tool 结果必须回灌进后续请求: {}",
        chat[2]["messages"]
    );

    // ── 账本：工具回执各两张；runs 表终态与历史 terminal 同源同文 ──
    assert_eq!(
        key_event_count(&boot.state, &run, "tool_call_started").await,
        2
    );
    assert_eq!(
        key_event_count(&boot.state, &run, "tool_call_completed").await,
        2
    );
    let (status, reason) = run_terminal_row(&boot.state, &run).await;
    assert_eq!(status, "completed");
    assert_eq!(
        reason.as_deref(),
        Some(NO_FINAL_REASON),
        "runs.terminal_reason 必须与历史 run_terminal 一致"
    );

    // ── 接口 golden + 原始捕获（原始历史即桌面投影测试夹具来源）──
    let normalized = normalize(&json!({
        "events": frames,
        "history": outcome.history,
        "etag": outcome.etag,
        "files": {
            SEED_NAME: SEED_CONTENT,
        },
        "providerChatRequests": chat.len(),
    }));
    write_capture("a04-history-raw.json", &outcome.history);
    write_capture(
        "a04-events-raw.json",
        &json!({ "events": frames, "etag": outcome.etag }),
    );
    write_capture("a04-golden-normalized.json", &normalized);
    let golden_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r08_a04_ws_golden.json");
    if std::env::var("LINGXI_R08_A04_GOLDEN_WRITE").is_ok() {
        std::fs::write(
            &golden_path,
            serde_json::to_string_pretty(&normalized).unwrap(),
        )
        .unwrap();
    } else {
        let golden: Value = serde_json::from_str(
            &std::fs::read_to_string(&golden_path)
                .expect("golden 缺失：先以 LINGXI_R08_A04_GOLDEN_WRITE=1 生成并人工审阅"),
        )
        .unwrap();
        assert_eq!(normalized, golden, "接口 golden 漂移");
    }

    let home = boot.stop().await;
    provider.stop();
    drop(home);
}
