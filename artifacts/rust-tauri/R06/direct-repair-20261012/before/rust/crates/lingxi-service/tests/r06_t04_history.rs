//! R06-T04 service 层生产链测试：统一消息历史、重连与导出。
//!
//! 验收覆盖：
//! - R06-A07 四方式一致：同一任务（含 MOOD/思考/过程/工具/最终回复）经
//!   实时（WS 订阅实况）、重开（GET /history）、重连（新连接快照补发）、
//!   导出（GET /export）四方式读取，阶段与顺序一致，正文不混入独立思考
//!   与 MOOD。
//! - R06-A08 入口语义：limit 钳制/ETag 条件请求/游标校验（存储层 I/O
//!   探针在 adapters 套件）。
//!
//! 对照纪律（RC-3）：
//! - limit 默认 50 / 上限 200 / ETag 条件 GET：现役
//!   `server/routes/sessions.ts:1415-1419,1468-1510`。
//! - 客户端不自行清洗 MOOD/思考（02 §3 禁止项，现役反例
//!   `desktop/src/react/utils/message-parser.ts:51`）：投影产物即最终语义，
//!   本套件只断言服务端产物，不做任何客户端再解析。
//! - 「有 final 的 run 不另产 RunTerminal」是投影公开规则（四方式同规则）：
//!   实时事件流里终态 run_state_changed 物理存在，测试侧比较阶段序列时
//!   对实时捕获应用同一条公开规则（有 fmc 则跳过终态项），不另造语义。
//! - 跨主体 403：`sessions.rs` can_access（设备主体仅本人）。

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ModelTurnDelta, ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult,
    ToolExecutorPort, ToolOutcome, ToolRequest, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError};
use lingxi_service::ws::WsFrame;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 确定性 provider 替身（只产出外部响应，不写任何状态） ──

enum ScriptStep {
    /// 直接 Final（text 可含带内 <think>/<mood> 标签，走入口规范化）。
    Final(String),
    /// 先发 deltas（段事件的唯一来源；ToolRequests.content 只进模型
    /// 上下文 exchange，不进事件流——runs.rs:1966 ToolRequests 臂无
    /// normalizer 调用），再以工具请求结束本 turn。
    ToolTurn {
        deltas: Vec<ModelTurnDelta>,
        target: String,
        args: serde_json::Value,
    },
    /// 空回（无可用的最终内容）——run 以 completed.no_final.* 终态结束。
    Empty { detail: String },
}

struct ScriptedProvider {
    script: std::sync::Mutex<VecDeque<ScriptStep>>,
}

impl ScriptedProvider {
    fn new(script: Vec<ScriptStep>) -> Arc<Self> {
        Arc::new(Self {
            script: std::sync::Mutex::new(script.into_iter().collect()),
        })
    }
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let step = self.script.lock().unwrap().pop_front();
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            match step {
                Some(ScriptStep::Final(text)) => ProviderTurnResult::of_ctx(
                    &ctx_at_issue,
                    ProviderTurn::Final {
                        message: NormalizedMessage {
                            role: "assistant".to_string(),
                            content: vec![ContentBlock::Text { text }],
                            model_call_id: Some(ModelCallId::new(call_id)),
                        },
                    },
                ),
                Some(ScriptStep::ToolTurn {
                    deltas: turn_deltas,
                    target,
                    args,
                }) => {
                    for delta in turn_deltas {
                        deltas.emit(delta).await.expect("delta sink open");
                    }
                    let request = ToolRequest::from_effective_arguments(
                        &target,
                        args,
                        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                    )
                    .expect("effective tool request");
                    ProviderTurnResult::of_ctx(
                        &ctx_at_issue,
                        ProviderTurn::ToolRequests {
                            requests: vec![request],
                            content: Vec::new(),
                        },
                    )
                }
                Some(ScriptStep::Empty { detail }) => ProviderTurnResult::of_ctx(
                    &ctx_at_issue,
                    ProviderTurn::Empty {
                        detail,
                        content: Vec::new(),
                    },
                ),
                None => ProviderTurnResult::of_ctx(
                    &ctx_at_issue,
                    ProviderTurn::Failed {
                        error: ProtocolError::new(
                            ErrorCode::UpstreamUnavailable,
                            "script exhausted",
                            false,
                        ),
                        retryable: false,
                    },
                ),
            }
        })
    }
}

/// 即时成功的工具替身（对照 admission_dedup_adversarial 的 ImmediateTool
/// 形态）：target → 固定成功文本。
struct EchoTool;

impl ToolExecutorPort for EchoTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a lingxi_protocol::ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let ctx_at_issue = ctx.clone();
        let text = format!("tool-result:{}", request.target);
        Box::pin(async move {
            ToolExecutionResult::of_ctx(&ctx_at_issue, ToolOutcome::success_text(text))
        })
    }
}

// ── harness（与 r06_t03_session_tree 同款：真 TCP + 真 HTTP + 真 SQLite） ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    state: ServiceState,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t04-svc-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn start_server(tag: &str, provider: Arc<ScriptedProvider>, with_tool: bool) -> TestServer {
    let home = synthetic_home(tag);
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(
        config,
        &layout,
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: with_tool.then(|| Arc::new(EchoTool) as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let serve_state = state.clone();
    let handle = tokio::spawn(async move {
        run(
            serve_state,
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
        addr,
        home,
        token,
        state,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
    }

    async fn post(&self, path: &str, body: &str) -> (u16, String) {
        http(
            &self.addr,
            "POST",
            path,
            &[("Authorization", &self.bearer())],
            Some(body),
        )
        .await
        .into_status_body()
    }

    async fn get(&self, path: &str) -> (u16, String) {
        http(
            &self.addr,
            "GET",
            path,
            &[("Authorization", &self.bearer())],
            None,
        )
        .await
        .into_status_body()
    }

    /// 带响应头的 GET（ETag 条件请求断言用）。
    async fn get_full(&self, path: &str, extra: &[(&str, &str)]) -> HttpResponse {
        let mut headers: Vec<(String, String)> = vec![("Authorization".to_string(), self.bearer())];
        for (name, value) in extra {
            headers.push(((*name).to_string(), (*value).to_string()));
        }
        let refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();
        http(&self.addr, "GET", path, &refs, None).await
    }

    /// 设备主体令牌（跨主体 403 测试）。
    async fn mint_device_token(&self, user_id: &str) -> String {
        let (status, body) = self
            .post(
                "/lingxi/v1/devices/credentials",
                &serde_json::json!({ "userId": user_id, "scopes": ["chat"] }).to_string(),
            )
            .await;
        assert_eq!(status, 201, "mint device credential: {body}");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// 等待已知 run 到达终态；返回 (status, terminal_reason)。
    async fn wait_terminal(&self, run_id: &str) -> (String, Option<String>) {
        for _ in 0..100 {
            if let Some((status, reason)) = run_row(self.state.storage(), run_id).await {
                if !matches!(
                    status.as_str(),
                    "queued" | "running" | "waiting_approval" | "cancelling"
                ) {
                    return (status, reason);
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("run {run_id} did not finalize within 5s");
    }

    /// 执行一轮并等待 run 终态；返回 (run_id, 终态 status, terminal_reason)。
    async fn execute_wait(
        &self,
        session_id: &str,
        input: &str,
    ) -> (String, String, Option<String>) {
        let (status, body) = self
            .post(
                &format!("/lingxi/v1/sessions/{session_id}/execute"),
                &serde_json::json!({ "input": input }).to_string(),
            )
            .await;
        assert_eq!(status, 200, "execute {session_id}: {body}");
        let run_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["runId"]
            .as_str()
            .unwrap()
            .to_string();
        let (status, reason) = self.wait_terminal(&run_id).await;
        (run_id, status, reason)
    }

    /// GET /history?all=1 的 items（投影全集）。
    async fn history_all(&self, session_id: &str) -> serde_json::Value {
        let (status, body) = self
            .get(&format!("/lingxi/v1/sessions/{session_id}/history?all=1"))
            .await;
        assert_eq!(status, 200, "history {session_id}: {body}");
        serde_json::from_str(&body).unwrap()
    }

    /// GET /export 的 items（导出全集）。
    async fn export_all(&self, session_id: &str) -> serde_json::Value {
        let (status, body) = self
            .get(&format!("/lingxi/v1/sessions/{session_id}/export"))
            .await;
        assert_eq!(status, 200, "export {session_id}: {body}");
        serde_json::from_str(&body).unwrap()
    }
}

async fn create_session(server: &TestServer, session_id: &str, title: &str) {
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": session_id,
                "agentId": "agent",
                "title": title,
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create session {session_id}: {body}");
}

async fn run_row(
    storage: &Arc<lingxi_adapters::storage::RunDatabase>,
    run_id: &str,
) -> Option<(String, Option<String>)> {
    let status = storage
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query status")?;
    let reason = storage
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query reason");
    Some((status, reason))
}

// ── HTTP/WS 裸客户端 ──

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl HttpResponse {
    fn into_status_body(self) -> (u16, String) {
        (self.status, self.body)
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

async fn http(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> HttpResponse {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await.expect("write body");
    }
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                break;
            }
            Err(err) => panic!("read response: {err}"),
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            line.split_once(':')
                .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    HttpResponse {
        status,
        headers,
        body: body.to_string(),
    }
}

fn client_ws_key() -> String {
    use base64::Engine as _;
    let mut bytes = [0u8; 16];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for (i, slot) in bytes.iter_mut().enumerate() {
        *slot = (((nanos >> (i * 4)) & 0xff) as u8).wrapping_add((i as u8).wrapping_mul(31));
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

async fn ws_upgrade_bearer(addr: SocketAddr, bearer: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let key = client_ws_key();
    let head = format!(
        "GET /lingxi/v1/ws HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\
         Authorization: Bearer {bearer}\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).await.expect("upgrade");
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    while stream.read_exact(&mut byte).await.is_ok() {
        seen.push(byte[0]);
        if seen.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(
        seen.starts_with(b"HTTP/1.1 101"),
        "upgrade failed: {}",
        String::from_utf8_lossy(&seen)
    );
    stream
}

async fn client_ws_send(stream: &mut TcpStream, payload: &[u8]) {
    let mask: [u8; 4] = [0x11, 0x22, 0x33, 0x44];
    let mut out = vec![0x81];
    if payload.len() < 126 {
        out.push(0x80 | payload.len() as u8);
    } else {
        out.push(0x80 | 126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    let masked: Vec<u8> = payload
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i % 4])
        .collect();
    out.extend_from_slice(&masked);
    stream.write_all(&out).await.expect("write client frame");
}

async fn client_ws_read(stream: &mut TcpStream) -> WsFrame {
    lingxi_service::ws::read_ws_frame(stream)
        .await
        .expect("read server frame")
        .expect("server frame")
}

fn frame_text(frame: &WsFrame) -> String {
    match frame {
        WsFrame::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        other => panic!("expected text frame, got {other:?}"),
    }
}

async fn ws_connect(server: &TestServer) -> TcpStream {
    let mut stream = ws_upgrade_bearer(server.addr, &server.token).await;
    let hello = serde_json::json!({
        "protocol": "lingxi.wire",
        "clientKind": "test",
        "clientVersion": "0",
        "protocolMin": 1,
        "protocolMax": 1,
    });
    client_ws_send(&mut stream, hello.to_string().as_bytes()).await;
    let hello_back = frame_text(&client_ws_read(&mut stream).await);
    assert!(
        hello_back.contains("lingxi.wire"),
        "server hello {hello_back}"
    );
    stream
}

async fn ws_subscribe(stream: &mut TcpStream, stream_id: &str) {
    let frame = serde_json::json!({"type": "subscribe_events", "streamId": stream_id});
    client_ws_send(stream, frame.to_string().as_bytes()).await;
}

/// 读取一帧（10s 超时），返回 JSON。
async fn read_json_frame(stream: &mut TcpStream) -> serde_json::Value {
    let frame = tokio::time::timeout(Duration::from_secs(10), client_ws_read(stream))
        .await
        .expect("frame within 10s");
    serde_json::from_str(&frame_text(&frame)).expect("frame json")
}

/// 实时捕获一个 run 的全部事件信封（读到终态 run_state_changed 与
/// final_message_committed 均出现；expect_final=false 时只等终态）。
async fn capture_run_events(
    stream: &mut TcpStream,
    run_id: &str,
    expect_final: bool,
) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    let mut saw_terminal = false;
    let mut saw_final = false;
    loop {
        let value = read_json_frame(stream).await;
        if value["frameKind"] == "control" {
            continue;
        }
        if value["runId"].as_str() != Some(run_id) {
            continue;
        }
        let ty = value["payload"]["type"].as_str().unwrap_or("").to_string();
        if ty == "run_state_changed" {
            let to = value["payload"]["to"].as_str().unwrap_or("");
            if matches!(
                to,
                "completed" | "failed" | "cancelled" | "interrupted_needs_attention"
            ) {
                saw_terminal = true;
            }
        }
        if ty == "final_message_committed" {
            saw_final = true;
        }
        events.push(value);
        if saw_terminal && (saw_final || !expect_final) {
            break;
        }
    }
    events
}

/// 实时/重连事件信封 → 阶段序列（(阶段, 稳定ID)）。
///
/// 投影公开规则「有 final 的 run 不另产 RunTerminal」在此对实时捕获应用
/// 同一条规则：存在 fmc 时终态 run_state_changed 不占阶段位。
fn realtime_stages(events: &[serde_json::Value]) -> Vec<(String, String)> {
    let has_final = events
        .iter()
        .any(|e| e["payload"]["type"] == "final_message_committed");
    let mut out = Vec::new();
    for e in events {
        let payload = &e["payload"];
        match payload["type"].as_str().unwrap_or("") {
            "assistant_segment_start" => out.push((
                "segment".to_string(),
                payload["segmentId"]
                    .as_str()
                    .expect("segmentId")
                    .to_string(),
            )),
            "tool_call_started" => out.push((
                "tool".to_string(),
                payload["toolCall"]["toolCallId"]
                    .as_str()
                    .expect("toolCallId")
                    .to_string(),
            )),
            "final_message_committed" => out.push((
                "final".to_string(),
                e["eventId"].as_str().expect("eventId").to_string(),
            )),
            "run_state_changed" if !has_final => {
                let to = payload["to"].as_str().unwrap_or("");
                if matches!(
                    to,
                    "completed" | "failed" | "cancelled" | "interrupted_needs_attention"
                ) {
                    out.push((
                        "terminal".to_string(),
                        e["runId"].as_str().expect("runId").to_string(),
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// 历史投影 items → run 范围阶段序列（与 realtime_stages 同一词汇表）。
fn history_stages(items: &[serde_json::Value]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for item in items {
        match item["kind"].as_str().unwrap_or("") {
            "assistant_segment" => out.push((
                "segment".to_string(),
                item["segmentId"].as_str().expect("segmentId").to_string(),
            )),
            "tool_call" => out.push((
                "tool".to_string(),
                item["toolCallId"].as_str().expect("toolCallId").to_string(),
            )),
            "final_message" => out.push((
                "final".to_string(),
                item["messageId"].as_str().expect("messageId").to_string(),
            )),
            "run_terminal" => out.push((
                "terminal".to_string(),
                item["runId"].as_str().expect("runId").to_string(),
            )),
            _ => {}
        }
    }
    out
}

// ──────────────────────────────────────────── A07：四方式语义一致

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a07_realtime_reopen_reconnect_export_agree() {
    let provider = ScriptedProvider::new(vec![
        ScriptStep::ToolTurn {
            deltas: vec![
                ModelTurnDelta::Text("<mood>happy</mood>让我想想".to_string()),
                ModelTurnDelta::Text("<think>内部推理A</think>".to_string()),
                ModelTurnDelta::Text("先调用读取工具".to_string()),
            ],
            target: "read".to_string(),
            args: serde_json::json!({"path": "a.txt"}),
        },
        ScriptStep::Final("<think>终稿思考</think>最终答案B<mood>proud</mood>".to_string()),
    ]);
    let server = start_server("a07", provider, true).await;
    create_session(&server, "s", "a07").await;

    // 方式一·实时：先订阅再执行，实况捕获本 run 的全部事件。
    let mut live = ws_connect(&server).await;
    ws_subscribe(&mut live, "s").await;
    let subscribed = read_json_frame(&mut live).await;
    assert_eq!(subscribed["type"], "subscribed", "{subscribed}");
    let exec_body = serde_json::json!({ "input": "读取 a.txt 并总结" }).to_string();
    let exec = server.post("/lingxi/v1/sessions/s/execute", &exec_body);
    let (status, body) = exec.await;
    assert_eq!(status, 200, "execute: {body}");
    let run_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["runId"]
        .as_str()
        .unwrap()
        .to_string();
    let realtime = capture_run_events(&mut live, &run_id, true).await;
    drop(live);
    let (status, _reason) = server.wait_terminal(&run_id).await;
    assert_eq!(status, "completed");

    // 方式三·重连：全新连接无游标订阅，快照补发同一持久事件序列。
    let mut resumed = ws_connect(&server).await;
    ws_subscribe(&mut resumed, "s").await;
    let resumed_events = collect_snapshot_run_events(&mut resumed, &run_id).await;
    drop(resumed);

    let realtime_ids: Vec<(String, String)> = realtime
        .iter()
        .map(|e| {
            (
                e["eventId"].as_str().unwrap().to_string(),
                e["seq"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let resumed_ids: Vec<(String, String)> = resumed_events
        .iter()
        .map(|e| {
            (
                e["eventId"].as_str().unwrap().to_string(),
                e["seq"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        realtime_ids, resumed_ids,
        "重连快照补发的事件序列必须等于实时持久序列（同一份 key_events）"
    );

    // 方式二·重开 与 方式四·导出。
    let history = server.history_all("s").await;
    let export = server.export_all("s").await;
    assert_eq!(
        history["items"], export["items"],
        "导出全集必须等于历史分页投影全集（同一投影函数）"
    );
    let items = history["items"].as_array().expect("items array");

    // 阶段及顺序：实时 == 重开 ==（已证相等）重连/导出。
    let live_stages = realtime_stages(&realtime);
    let hist_stages = history_stages(items);
    assert_eq!(
        live_stages, hist_stages,
        "四方式阶段及顺序必须一致（实时 {live_stages:?} vs 重开 {hist_stages:?}）"
    );
    // 阶段构成：2 段（文本过程/思考——同一 turn 的正文段在 think 前后
    // 续写同一段）+ 1 工具 + 1 final。
    assert_eq!(
        live_stages.iter().filter(|(k, _)| k == "segment").count(),
        2,
        "文本过程+思考两段: {live_stages:?}"
    );

    // 正文纯度：任何方式下 MOOD/独立思考标记与 MOOD 内容都不得出现。
    let realtime_text = serde_json::to_string(&realtime).unwrap();
    let history_text = serde_json::to_string(&history["items"]).unwrap();
    for (mode, blob) in [("realtime", &realtime_text), ("history", &history_text)] {
        for forbidden in [
            "<mood", "</mood>", "<think", "</think>", "happy", "calm", "proud",
        ] {
            assert!(
                !blob.contains(forbidden),
                "{mode} 不得混入 MOOD/思考标记或 MOOD 内容 {forbidden:?}"
            );
        }
    }
    // 独立思考仍在——以「推理段/推理块」身份，而不是混进正文。
    let reasoning_segment = items
        .iter()
        .find(|i| i["kind"] == "assistant_segment" && i["segmentKind"] == "reasoning")
        .expect("思考必须以推理段存在");
    assert!(
        reasoning_segment["text"]
            .as_str()
            .unwrap()
            .contains("内部推理A"),
        "推理段承载思考内容: {reasoning_segment}"
    );
    let final_item = items
        .iter()
        .find(|i| i["kind"] == "final_message")
        .expect("final message");
    assert_eq!(
        final_item["content"],
        serde_json::json!([
            {"type": "reasoning", "text": "终稿思考"},
            {"type": "text", "text": "最终答案B"},
        ]),
        "final 正文经同一 scanner 重切：think→推理块、mood 剥离"
    );

    // 段文本保真：历史段文本 == 实时 delta 拼接。
    let mut delta_by_segment: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for e in &realtime {
        if e["payload"]["type"] == "assistant_segment_delta" {
            *delta_by_segment
                .entry(e["payload"]["segmentId"].as_str().unwrap().to_string())
                .or_default() += e["payload"]["delta"].as_str().unwrap_or("");
        }
    }
    for item in items {
        if item["kind"] != "assistant_segment" {
            continue;
        }
        let seg_id = item["segmentId"].as_str().unwrap();
        let hist_text = item["text"].as_str().expect("segment text");
        let live_text = delta_by_segment.get(seg_id).cloned().unwrap_or_default();
        assert_eq!(
            hist_text, live_text,
            "段 {seg_id} 文本：历史必须等于实时 delta 拼接"
        );
        assert_eq!(item["complete"], true, "完整 run 的段必须 complete");
    }

    // 工具卡：稳定 id 跨方式相等（任务书步骤⑤），结果与实时完成事件一致。
    let hist_tool = items
        .iter()
        .find(|i| i["kind"] == "tool_call")
        .expect("tool item");
    let tool_call_id = hist_tool["toolCallId"].as_str().unwrap();
    let live_completed = realtime
        .iter()
        .find(|e| e["payload"]["type"] == "tool_call_completed")
        .expect("live tool completion");
    assert_eq!(
        live_completed["payload"]["toolCallId"].as_str().unwrap(),
        tool_call_id,
        "工具卡稳定 id 实时==历史"
    );
    assert!(
        tool_call_id.starts_with(&format!("{run_id}-tc")),
        "tool_call_id 承载 run 关联: {tool_call_id}"
    );
    assert_eq!(hist_tool["status"], "success");
    assert_eq!(
        hist_tool["result"]["content"], live_completed["payload"]["result"]["content"],
        "工具结果历史==实时"
    );

    // 用户消息：恰好一条，承载输入原文，先于该 run 的项。
    let user_pos = items
        .iter()
        .position(|i| i["kind"] == "user_message")
        .expect("user message");
    assert_eq!(items[user_pos]["text"], "读取 a.txt 并总结");
    let first_run_pos = items
        .iter()
        .position(|i| {
            matches!(
                i["kind"].as_str(),
                Some("assistant_segment" | "tool_call" | "final_message")
            )
        })
        .expect("run items");
    assert!(user_pos < first_run_pos, "user 锚在 run 组之前");

    server.stop_and_clean().await;
}

/// 无 final 的 run（工具执行后空回）：历史如实给 ToolCall + RunTerminal，
/// 绝不编造 final（02 §4 / 任务书步骤⑤「仅工具执行无最终回复也可展开」）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_only_run_projects_terminal_honestly() {
    let provider = ScriptedProvider::new(vec![
        ScriptStep::ToolTurn {
            deltas: vec![ModelTurnDelta::Text("调用计数工具".to_string())],
            target: "counter.bump".to_string(),
            args: serde_json::json!({"op": "bump"}),
        },
        ScriptStep::Empty {
            detail: "provider returned no usable content".to_string(),
        },
    ]);
    let server = start_server("tool-only", provider, true).await;
    create_session(&server, "s", "tool-only").await;
    let (run_id, status, reason) = server.execute_wait("s", "计数").await;
    assert_eq!(status, "completed");
    assert!(
        reason
            .as_deref()
            .unwrap_or("")
            .starts_with("completed.no_final."),
        "无 final 必须显式 no_final 终态: {reason:?}"
    );

    let history = server.history_all("s").await;
    let items = history["items"].as_array().unwrap();
    assert!(
        !items
            .iter()
            .any(|i| i["kind"] == "final_message" && i["run"]["runId"] == run_id),
        "无 final 的 run 不得编造 FinalMessage: {items:?}"
    );
    let tool = items
        .iter()
        .find(|i| i["kind"] == "tool_call")
        .expect("tool item present");
    assert_eq!(tool["status"], "success");
    let terminal = items
        .iter()
        .find(|i| i["kind"] == "run_terminal" && i["runId"] == run_id)
        .expect("无 final 的 run 必须有 RunTerminal");
    assert_eq!(terminal["status"], "completed");
    assert!(
        terminal["terminalReason"]
            .as_str()
            .unwrap_or("")
            .starts_with("completed.no_final."),
        "RunTerminal 携带真实终态原因: {terminal}"
    );

    server.stop_and_clean().await;
}

/// A08 入口语义：limit 钳制（现役上限 200）、ETag 条件 GET 304、all=1 全量。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_etag_conditional_get_and_limit_clamp() {
    let provider = ScriptedProvider::new(vec![
        ScriptStep::Final("a1".to_string()),
        ScriptStep::Final("a2".to_string()),
    ]);
    let server = start_server("etag", provider, false).await;
    create_session(&server, "s", "etag").await;
    server.execute_wait("s", "q1").await;

    // ETag：首轮后条件 GET 命中 304。
    let first = server.get_full("/lingxi/v1/sessions/s/history", &[]).await;
    assert_eq!(first.status, 200, "history: {}", first.body);
    let etag = first.header("etag").expect("ETag header").to_string();
    let conditional = server
        .get_full("/lingxi/v1/sessions/s/history", &[("If-None-Match", &etag)])
        .await;
    assert_eq!(
        conditional.status, 304,
        "If-None-Match 命中 head_revision → 304（现役语义）: {}",
        conditional.body
    );
    // 新一轮推进 head revision → 旧 ETag 失效回 200。
    server.execute_wait("s", "q2").await;
    let stale = server
        .get_full("/lingxi/v1/sessions/s/history", &[("If-None-Match", &etag)])
        .await;
    assert_eq!(stale.status, 200, "头推进后旧 ETag 必须失效");
    let new_etag = stale.header("etag").expect("new ETag");
    assert_ne!(etag, new_etag, "revision 变化必须体现到 ETag");

    // limit 钳制与解析（现役 sessions.ts:1415-1419）。
    let clamped = server
        .get_full("/lingxi/v1/sessions/s/history?limit=9999", &[])
        .await;
    assert_eq!(clamped.status, 200);
    let parsed: serde_json::Value = serde_json::from_str(&clamped.body).unwrap();
    assert_eq!(parsed["page"]["limit"], 200, "limit 超上限钳 200");
    for bad in ["limit=abc", "limit=0", "limit=-3", "limit=1&limit=2"] {
        let (status, body) = server
            .get(&format!("/lingxi/v1/sessions/s/history?{bad}"))
            .await;
        assert_eq!(status, 400, "非法分页参数 {bad} 必须 400: {body}");
    }

    server.stop_and_clean().await;
}

/// 游标校验与归属闸：伪造/畸形游标 400；跨主体 403；未知会话 404。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_cursor_validation_and_owner_isolation() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("a1".to_string())]);
    let server = start_server("isolation", provider, false).await;
    create_session(&server, "s", "iso").await;
    server.execute_wait("s", "q1").await;

    for bad in ["before=!!!", "before=AAAA", "before="] {
        let (status, body) = server
            .get(&format!("/lingxi/v1/sessions/s/history?{bad}"))
            .await;
        assert_eq!(status, 400, "畸形游标 {bad} 必须响亮 400: {body}");
    }

    let (status, _body) = server.get("/lingxi/v1/sessions/no_such/history").await;
    assert_eq!(status, 404, "未知会话 404");
    let (status, _body) = server.get("/lingxi/v1/sessions/no_such/export").await;
    assert_eq!(status, 404, "未知会话导出 404");

    // 跨主体：设备主体（另一用户）读本地主人的会话 → 403。
    let device = server.mint_device_token("someone_else").await;
    let foreign = http(
        &server.addr,
        "GET",
        "/lingxi/v1/sessions/s/history",
        &[("Authorization", &format!("Bearer {device}"))],
        None,
    )
    .await;
    assert_eq!(
        foreign.status, 403,
        "跨主体读历史必须 403: {}",
        foreign.body
    );
    let foreign_export = http(
        &server.addr,
        "GET",
        "/lingxi/v1/sessions/s/export",
        &[("Authorization", &format!("Bearer {device}"))],
        None,
    )
    .await;
    assert_eq!(foreign_export.status, 403, "跨主体导出必须 403");

    server.stop_and_clean().await;
}

/// 导出 == 分页遍历装配（同一分页器逐页收集，limit=2 强制多页）。
/// 装配语义与聊天 UI 一致：分页器最新页先出，向上翻更老的页**前插**——
/// 装配结果与页大小无关，逐项等于导出的全链转录序（旧→新）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_equals_paged_concat() {
    let provider = ScriptedProvider::new(vec![
        ScriptStep::Final("a1".to_string()),
        ScriptStep::Final("a2".to_string()),
        ScriptStep::Final("a3".to_string()),
    ]);
    let server = start_server("export-eq", provider, false).await;
    create_session(&server, "s", "export-eq").await;
    for q in ["q1", "q2", "q3"] {
        server.execute_wait("s", q).await;
    }

    // 小页遍历（limit=2，6 条消息 → ≥3 页），每页前插装配。
    let mut paged: Vec<serde_json::Value> = Vec::new();
    let mut before: Option<String> = None;
    let mut page_count = 0usize;
    loop {
        page_count += 1;
        let mut path = "/lingxi/v1/sessions/s/history?limit=2".to_string();
        if let Some(cursor) = &before {
            path.push_str(&format!("&before={cursor}"));
        }
        let (status, body) = server.get(&path).await;
        assert_eq!(status, 200, "page {page_count}: {body}");
        let page: serde_json::Value = serde_json::from_str(&body).unwrap();
        let items = page["items"].as_array().expect("page items");
        // 前插：本页（更老）排在已收集（更新）之前。
        let mut assembled = items.clone();
        assembled.append(&mut paged);
        paged = assembled;
        if page["page"]["hasMore"] == false {
            break;
        }
        before = Some(
            page["page"]["nextBefore"]
                .as_str()
                .expect("nextBefore cursor")
                .to_string(),
        );
        assert!(page_count < 50, "分页必须终止");
    }
    assert!(page_count >= 3, "limit=2 下 6 消息必须多页: {page_count}");

    let export = server.export_all("s").await;
    assert_eq!(
        serde_json::Value::Array(paged),
        export["items"],
        "逐页前插装配必须逐项等于导出全集（不丢不重、次序与页大小无关）"
    );

    server.stop_and_clean().await;
}

/// fork 副本的 run 关联：副本消息 run_id 置 NULL（T03 D9），投影必须标
/// unknown 而绝不按时间猜；源会话同一消息仍是 known。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forked_copy_marks_run_association_unknown() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("a1".to_string())]);
    let server = start_server("fork-unknown", provider, false).await;
    create_session(&server, "src", "src").await;
    let (run_id, _, _) = server.execute_wait("src", "q1").await;

    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/src/fork",
            &serde_json::json!({
                "newSessionId": "forked",
                "boundaryMessageId": format!("{run_id}-final"),
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "fork: {body}");

    let src_history = server.history_all("src").await;
    let src_final = src_history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "final_message")
        .expect("src final");
    assert_eq!(
        src_final["run"]["state"], "known",
        "源会话 final 的 run 关联已知: {src_final}"
    );
    assert_eq!(src_final["run"]["runId"], run_id);

    let fork_history = server.history_all("forked").await;
    let fork_final = fork_history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "final_message")
        .expect("fork final");
    assert_eq!(
        fork_final["run"]["state"], "unknown",
        "fork 副本 run_id 置 NULL（T03 D9）→ 关联必须 unknown，绝不按时间猜: {fork_final}"
    );

    server.stop_and_clean().await;
}

/// 快照补发收集：无游标订阅后按 snapshotSeq 边界收齐本 run 的事件。
async fn collect_snapshot_run_events(
    stream: &mut TcpStream,
    run_id: &str,
) -> Vec<serde_json::Value> {
    let control = read_json_frame(stream).await;
    assert_eq!(control["type"], "subscribed", "{control}");
    assert_eq!(control["mode"], "snapshot");
    let snapshot_seq: u64 = control["snapshotSeq"]
        .as_str()
        .expect("snapshotSeq")
        .parse()
        .expect("snapshotSeq numeric");
    let mut events = Vec::new();
    let mut max_seen = 0u64;
    while max_seen < snapshot_seq {
        let value = read_json_frame(stream).await;
        if value["frameKind"] == "control" {
            continue;
        }
        let seq: u64 = value["seq"].as_str().unwrap().parse().unwrap();
        max_seen = max_seen.max(seq);
        if value["runId"].as_str() == Some(run_id) {
            events.push(value);
        }
    }
    events
}
