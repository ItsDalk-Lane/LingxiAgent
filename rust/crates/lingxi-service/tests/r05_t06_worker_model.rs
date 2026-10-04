//! R05-T06 acceptance: worker model callbacks through the REAL model plane
//! (§29: C04 real-gateway ride / C05 single-thread liveness / C06 shared
//! quota interlock / C07 replay dedup / C08 host budget caps / C09 payload
//! is not an authority / C10 killable under deadline / C11 no credential
//! material reaches the worker).
//!
//! The far end of the wire is a loopback HTTP stub (test equipment); the
//! worker is a REAL child process (`r04_t07_fixture`); the chain under
//! test is ALL production code: workerrpc executor → BoundedWorkerModel →
//! GatewayWorkerModel → AuxiliaryExecutor → GatewayedProvider →
//! openai-completions wire. Offline; NOT_REAL_API.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult, ToolOutcome,
    ToolRequest, TurnDeltaSink, TurnProviderPort,
};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetId};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, ToolCallId};
use lingxi_service::approval_service::ApprovalService;
use lingxi_service::quotas::{LayeredQuotaLimits, QuotaLimits, QuotaManager, QuotaResource};
use lingxi_service::toolgateway::ToolInvocationGateway;
use lingxi_service::workermodel::{
    GatewayWorkerModel, WorkerCallbackTrace, WorkerCallbackTracePort,
};
use lingxi_service::workerrpc::{
    register_worker_tool, BoundedWorkerModel, WorkerLimits, WorkerModelPort, WorkerModelRefusal,
    WorkerModelReply, WorkerModelRequest, WorkerRuntime, WorkerToolSpec,
    WORKER_CALLBACK_MAX_OUTPUT_TOKENS, WORKER_MAX_CALLBACKS_PER_CALL,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::json;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

fn fixture_exe() -> &'static str {
    env!("CARGO_BIN_EXE_r04_t07_fixture")
}

fn unique_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t06-{tag}-{}-{}",
        std::process::id(),
        DIR_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

// ── the loopback stub provider (the far end of the wire) ────────────────────

/// One scripted stub response; `delay_ms` stalls the answer (the C05
/// starvation probe).
struct StubResponse {
    body: String,
    delay_ms: u64,
}

impl StubResponse {
    fn sse(body: String) -> Self {
        Self { body, delay_ms: 0 }
    }
    fn delayed(body: String, delay_ms: u64) -> Self {
        Self { body, delay_ms }
    }
}

#[derive(Debug)]
struct RecordedRequest {
    path: String,
    headers: Vec<(String, String)>,
    body: serde_json::Value,
}

impl RecordedRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    /// Starts a raw-TCP HTTP/1.1 stub. One request per connection; scripted
    /// responses are popped in order; an exhausted script is a LOUD 500.
    async fn start(responses: Vec<StubResponse>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_requests, task_responses) = (
            Arc::clone(&hits),
            Arc::clone(&requests),
            Arc::clone(&responses),
        );
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                task_hits.fetch_add(1, Ordering::SeqCst);
                let recorded = read_request(&mut socket).await;
                task_requests.lock().expect("requests").push(recorded);
                let next = task_responses.lock().expect("responses").pop_front();
                let response = next.unwrap_or_else(|| {
                    StubResponse::sse(
                        "{\"error\":{\"message\":\"stub script exhausted\"}}".to_string(),
                    )
                });
                if response.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(response.delay_ms)).await;
                }
                let payload = response.body;
                let raw = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = socket.write_all(raw.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            addr,
            hits,
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn v1(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .map(|r| RecordedRequest {
                path: r.path.clone(),
                headers: r.headers.clone(),
                body: r.body.clone(),
            })
            .collect()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> RecordedRequest {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub read stalled")
            .expect("stub read");
        if read == 0 {
            panic!("stub: connection closed before headers completed");
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        assert!(raw.len() < 256 * 1024, "stub: header block too large");
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("stub: utf8 headers");
    let mut lines = head.split("\r\n");
    let request_line = lines.next().expect("request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("request path")
        .to_string();
    let mut headers = Vec::new();
    let mut content_length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = Some(value.parse::<usize>().expect("numeric content-length"));
            }
            headers.push((name, value));
        }
    }
    let content_length = content_length.expect("json posts carry a content-length");
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub body read stalled")
            .expect("stub body read");
        if read == 0 {
            panic!("stub: connection closed mid-body");
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&raw[body_start..body_start + content_length])
            .expect("stub: request body must be JSON");
    RecordedRequest {
        path,
        headers,
        body,
    }
}

/// One openai-completions SSE answer carrying `text`.
fn openai_sse_final(text: &str) -> StubResponse {
    let mut body = String::new();
    for frame in [
        json!({"id":"chatcmpl-t06","model":"the-stub-lies","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
        json!({"id":"chatcmpl-t06","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chatcmpl-t06","choices":[],"usage":{"prompt_tokens":17,"completion_tokens":5}}),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse::sse(body)
}

// ── the recording trace port (C04 correlation evidence) ─────────────────────

#[derive(Default)]
struct RecordingTrace {
    records: Mutex<Vec<WorkerCallbackTrace>>,
}

impl WorkerCallbackTracePort for RecordingTrace {
    fn record<'a>(
        &'a self,
        trace: WorkerCallbackTrace,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::StorageError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.records.lock().expect("trace").push(trace);
            Ok(())
        })
    }
}

// ── the real worker-model chain (production assembly, minus ServiceState) ───

const STUB_API_KEY: &str = "sk-t06-secret-material-9f8e7d6c5b4a";

/// A trace-port double whose ledger write ALWAYS fails — the C10
/// publish-boundary injection (REVIEW-T07 F-03): the provider leg
/// succeeds, then the usage ledger refuses the callback's accounting row.
struct FailingTrace;

impl WorkerCallbackTracePort for FailingTrace {
    fn record<'a>(
        &'a self,
        _trace: WorkerCallbackTrace,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::StorageError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(lingxi_kernel::ports::StorageError::Io {
                detail: "c10: injected usage-ledger write failure".to_string(),
            })
        })
    }
}

/// Builds the REAL callback chain: config plane (openai-completions chat +
/// summarize aux slot bound to the stub) → ConfigModelGateway →
/// CredentialService → AuxiliaryExecutor → GatewayWorkerModel (the given
/// trace port) → BoundedWorkerModel (the production budget wrapper).
fn real_worker_model(
    stub: &StubServer,
    runtime_dir: &Path,
    quotas: Arc<QuotaManager>,
    trace: Arc<dyn WorkerCallbackTracePort>,
) -> Arc<dyn WorkerModelPort> {
    let plane_json = format!(
        r#"{{
            "providers": {{
                "stub_svc": {{
                    "protocol": "openai-completions",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "{STUB_API_KEY}"}}
                }}
            }},
            "models": {{
                "chat": {{"provider": "stub_svc", "model": "chat-model"}},
                "summarize": {{"provider": "stub_svc", "model": "summarize-model"}}
            }}
        }}"#,
        stub.v1()
    );
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&plane_json)
        .expect("plane parses");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credential service"),
    );
    let gateway =
        Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane));
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        gateway,
        credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
        budget(),
    )
    .expect("auxiliary executor");
    let gateway_model = GatewayWorkerModel::with_trace(Arc::new(executor), quotas, trace);
    BoundedWorkerModel::new(
        Some(Arc::new(gateway_model)),
        WORKER_MAX_CALLBACKS_PER_CALL,
        WORKER_CALLBACK_MAX_OUTPUT_TOKENS,
    )
}

// ── the worker-side harness (the r04_t07 pattern) ───────────────────────────

struct WorkerHarness {
    gateway: Arc<ToolInvocationGateway>,
    registry: Arc<ToolRegistry>,
    access: Arc<lingxi_service::ResourceAccess>,
    runtime: Arc<WorkerRuntime>,
    ws: PathBuf,
}

fn worker_harness(root: &Path, limits: WorkerLimits) -> WorkerHarness {
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let registry = Arc::new(ToolRegistry::new());
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let approvals = Arc::new(ApprovalService::new(Arc::new(
        lingxi_service::inject::SystemClock,
    )));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(lingxi_service::inject::SystemClock),
        budget(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    let runtime = WorkerRuntime::new(limits, None, Arc::new(lingxi_service::inject::SystemClock));
    WorkerHarness {
        gateway,
        registry,
        access,
        runtime,
        ws,
    }
}

fn register_t06_worker(
    h: &WorkerHarness,
    model: Arc<dyn WorkerModelPort>,
    mode: &str,
    extra: &str,
    purposes: &[&str],
) -> ToolTargetId {
    let input = h.ws.join("input.txt");
    std::fs::write(&input, "granted-content\n").expect("input file");
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let local_name = format!("t06w_{mode}_{}", SEQ.fetch_add(1, Ordering::SeqCst));
    let mut argv = vec![fixture_exe().to_string(), mode.to_string()];
    if !extra.is_empty() {
        argv.push(extra.to_string());
    }
    let spec = WorkerToolSpec {
        plugin_id: "t06worker".to_string(),
        op: "probe".to_string(),
        local_name: local_name.clone(),
        description: format!("T06 synthetic worker ({mode})"),
        input_schema: json!({
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
            "additionalProperties": false
        }),
        path_args: vec!["input".to_string()],
        argv,
        env: BTreeMap::new(),
        cwd: h.ws.clone(),
        model,
        allowed_model_purposes: purposes.iter().map(|s| s.to_string()).collect(),
        claimed_file_contract: None,
    };
    register_worker_tool(
        &h.registry,
        h.gateway.as_ref(),
        &h.runtime,
        &h.access,
        &budget(),
        spec,
    )
    .expect("worker registers")
    .target
}

async fn call_worker(h: &WorkerHarness, target: &ToolTargetId) -> ToolExecutionResult {
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_t06"),
        run_id: lingxi_protocol::RunId::new(format!("run-t06-{}", std::process::id())),
        attempt: lingxi_protocol::AttemptId::new("a-1"),
        generation: 1,
    };
    let request = ToolRequest::from_effective_arguments(
        target.as_str(),
        json!({"input": "input.txt"}),
        &budget(),
    )
    .expect("effective request");
    let call_id = ToolCallId::new(format!("call-t06-{}", uuidish()));
    let prepared = h
        .gateway
        .prepare_from_request(
            &ctx,
            lingxi_service::toolgateway::CallerSurface::UserRun,
            "agent",
            lingxi_service::toolgateway::InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &call_id,
            &request,
        )
        .expect("prepare");
    h.gateway
        .execute_prepared(&ctx, &call_id, &prepared.handle)
        .await
        .expect("execute")
}

fn uuidish() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos()
}

fn success_text(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("expected a successful worker call, got {other:?}"),
    }
}

// ── C04: the callback rides the REAL gateway; the trace joins parent/child ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c04_worker_callback_rides_the_real_gateway_and_trace_joins_parent_and_child() {
    let root = unique_dir("c04");
    let stub = StubServer::start(vec![openai_sse_final("host-side summary")]).await;
    let trace = Arc::new(RecordingTrace::default());
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
    let model = real_worker_model(&stub, &root.join("runtime"), quotas, trace.clone());
    let h = worker_harness(&root, WorkerLimits::default());
    let target = register_t06_worker(&h, model, "ask_model", "", &["summarize"]);

    let result = call_worker(&h, &target).await;
    let text = success_text(&result);
    let report: serde_json::Value = serde_json::from_str(&text).expect("worker report is json");
    assert_eq!(report["reply"]["ok"], json!(true), "{text}");
    assert_eq!(
        report["reply"]["text"],
        json!("host-side summary"),
        "the answer is the STUB's text through the real provider chain: {text}"
    );

    // The wire leg: exactly one provider call, the AUX SLOT's route.
    assert_eq!(stub.hits(), 1);
    let request = &stub.requests()[0];
    assert_eq!(request.path, "/v1/chat/completions");
    assert_eq!(request.body["model"], json!("summarize-model"));
    assert_eq!(request.body["max_tokens"], json!(64));
    assert_eq!(request.body["stream"], json!(true));
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {STUB_API_KEY}").as_str())
    );

    // The trace leg: the settled callback carries the RESOLVED identity
    // and joins the parent invocation by the id the worker echoed back.
    let records = trace.records.lock().expect("trace").clone();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.cb_id, "cb-1");
    assert_eq!(record.slot, "summarize");
    assert_eq!(record.purpose, "summarize");
    assert_eq!(record.provider, "stub_svc");
    assert_eq!(record.model, "summarize-model");
    assert_eq!(
        record.invocation,
        report["request_id"].as_str().expect("request id"),
        "the trace joins THIS invocation (parent) and the callback (child)"
    );
    stub.stop().await;
}

// ── C05: a single-threaded runtime serves a delayed callback (no starvation) ─

/// The whole chain — stub server, worker process, gateway, real provider —
/// on ONE worker thread. A blocking callback path (block_on / sync I/O on
/// the loop) would starve the stub task and deadlock; a heartbeat task
/// proves the loop kept turning while the stub delayed its answer.
#[tokio::test(flavor = "current_thread")]
async fn c05_single_threaded_runtime_serves_a_delayed_callback_without_starvation() {
    let root = unique_dir("c05");
    let mut delayed = openai_sse_final("delayed summary");
    delayed = StubResponse::delayed(delayed.body, 400);
    let stub = StubServer::start(vec![delayed]).await;
    let trace = Arc::new(RecordingTrace::default());
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
    let model = real_worker_model(&stub, &root.join("runtime"), quotas, trace);
    let h = worker_harness(&root, WorkerLimits::default());
    let target = register_t06_worker(&h, model, "ask_model", "", &["summarize"]);

    let heartbeat = Arc::new(AtomicUsize::new(0));
    let beat = Arc::clone(&heartbeat);
    let beater = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            beat.fetch_add(1, Ordering::SeqCst);
        }
    });
    let result = call_worker(&h, &target).await;
    beater.abort();
    let text = success_text(&result);
    assert!(text.contains("delayed summary"), "{text}");
    assert!(
        heartbeat.load(Ordering::SeqCst) >= 5,
        "the runtime kept turning while the stub delayed (a blocking callback would starve it)"
    );
    stub.stop().await;
}

// ── C10: a hung callback is killed at the deadline and settles UNKNOWN ──────

struct HangingModel;
impl WorkerModelPort for HangingModel {
    fn complete<'a>(
        &'a self,
        _ctx: &'a RunContext,
        _worker: &'a str,
        _invocation: &'a str,
        _cb_id: &'a str,
        _request: &'a WorkerModelRequest,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<WorkerModelReply, WorkerModelRefusal>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            std::future::pending::<Result<WorkerModelReply, WorkerModelRefusal>>().await
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c10_a_hung_callback_kills_the_worker_and_settles_unknown() {
    let root = unique_dir("c10");
    let limits = WorkerLimits {
        deadline_ms: 500,
        ..WorkerLimits::default()
    };
    let h = worker_harness(&root, limits);
    let target = register_t06_worker(&h, Arc::new(HangingModel), "ask_model", "", &["summarize"]);
    let started = std::time::Instant::now();
    let result = call_worker(&h, &target).await;
    let elapsed = started.elapsed();
    match &result.outcome {
        ToolOutcome::Unknown { reason } => {
            assert!(
                reason.contains("deadline"),
                "the honest deadline settlement: {reason}"
            );
        }
        other => panic!("expected the unknown settlement, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(20),
        "the deadline path is bounded (kill + grace), not a hang: {elapsed:?}"
    );
}

// ── C10 (T07 leg, REVIEW-T07 F-03): a usage-ledger write failure at the
// publish boundary refuses the callback — no reply without accounting. ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c10_a_failing_usage_trace_refuses_the_callback_reply_never_answers_unaccounted() {
    let root = unique_dir("c10trace");
    let stub = StubServer::start(vec![openai_sse_final("would-be unaccounted summary")]).await;
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
    // The injection point is the PUBLISH BOUNDARY (after the provider
    // leg): the physical request succeeds — it may be billable — and the
    // ledger then refuses the accounting row. The worker must receive a
    // loud refusal, never a success reply whose row was lost.
    let model = real_worker_model(&stub, &root.join("runtime"), quotas, Arc::new(FailingTrace));
    let h = worker_harness(&root, WorkerLimits::default());
    let target = register_t06_worker(&h, model, "ask_model", "", &["summarize"]);

    let result = call_worker(&h, &target).await;
    let text = success_text(&result);
    let report: serde_json::Value = serde_json::from_str(&text).expect("worker report is json");
    assert_eq!(report["reply"]["ok"], json!(false), "{text}");
    assert_eq!(
        report["reply"]["error"]["code"],
        json!("model_provider_refused"),
        "the ledger failure surfaces as a loud callback refusal: {text}"
    );
    let message = report["reply"]["error"]["message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        message.contains("usage ledger"),
        "the refusal names the accounting failure (not a provider fault): {text}"
    );
    // The provider leg DID run — one real physical request (possibly
    // billable) — the honest settlement is the refusal, not a re-ask.
    assert_eq!(stub.hits(), 1);
    stub.stop().await;
}

// ── C07/C08/C09 legs over the real chain ────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c07_a_replayed_cb_id_is_answered_from_the_receipt_cache() {
    let root = unique_dir("c07");
    let stub = StubServer::start(vec![openai_sse_final("cached answer")]).await;
    let trace = Arc::new(RecordingTrace::default());
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
    let model = real_worker_model(&stub, &root.join("runtime"), quotas, trace.clone());
    let h = worker_harness(&root, WorkerLimits::default());
    let target = register_t06_worker(&h, model, "callback_replay", "", &["summarize"]);

    let result = call_worker(&h, &target).await;
    let text = success_text(&result);
    let replies: Vec<serde_json::Value> = serde_json::from_str(&text).expect("replies json");
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0], replies[1], "the replay got the FIRST receipt");
    assert_eq!(replies[0]["ok"], json!(true));
    assert_eq!(
        stub.hits(),
        1,
        "the duplicate callback never reached the provider twice"
    );
    assert_eq!(trace.records.lock().expect("trace").len(), 1);
    stub.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c08_an_overcap_token_ask_is_refused_before_any_provider_contact() {
    let root = unique_dir("c08");
    let stub = StubServer::start(vec![]).await; // any contact is loud
    let trace = Arc::new(RecordingTrace::default());
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
    let model = real_worker_model(&stub, &root.join("runtime"), quotas, trace);
    let h = worker_harness(&root, WorkerLimits::default());
    let target = register_t06_worker(&h, model, "ask_model_overcap", "", &["summarize"]);

    let result = call_worker(&h, &target).await;
    let text = success_text(&result);
    let reply: serde_json::Value = serde_json::from_str(&text).expect("reply json");
    assert_eq!(reply["ok"], json!(false), "{text}");
    assert_eq!(reply["error"]["code"], json!("model_budget_exceeded"));
    assert_eq!(stub.hits(), 0, "the refusal precedes ANY provider contact");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c09_identity_fields_and_ungranted_purposes_are_refused() {
    let root = unique_dir("c09");
    let stub = StubServer::start(vec![]).await; // any contact is loud
    let trace = Arc::new(RecordingTrace::default());
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
    let model = real_worker_model(&stub, &root.join("runtime"), quotas, trace);
    let h = worker_harness(&root, WorkerLimits::default());

    // Leg 1: an identity-claiming field — loud refusal, no dispatch.
    let forged = register_t06_worker(&h, model.clone(), "forged_identity", "", &["summarize"]);
    let result = call_worker(&h, &forged).await;
    let text = success_text(&result);
    let reply: serde_json::Value = serde_json::from_str(&text).expect("reply json");
    assert_eq!(reply["ok"], json!(false), "{text}");
    assert_eq!(
        reply["error"]["code"],
        json!("worker_identity_not_negotiable")
    );

    // Leg 2: a purpose OUTSIDE the host-granted whitelist.
    let espionage = register_t06_worker(
        &h,
        model,
        "ask_model",
        "espionage",
        &["summarize"], // the grant; "espionage" is not in it
    );
    let result = call_worker(&h, &espionage).await;
    let text = success_text(&result);
    let report: serde_json::Value = serde_json::from_str(&text).expect("report json");
    assert_eq!(report["reply"]["ok"], json!(false), "{text}");
    assert_eq!(
        report["reply"]["error"]["code"],
        json!("model_purpose_not_granted")
    );
    assert_eq!(stub.hits(), 0, "neither refusal reached the provider");
}

// ── C11: credential material never reaches the worker on the real chain ─────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c11_the_worker_never_sees_credential_material_on_the_real_chain() {
    let root = unique_dir("c11");
    let stub = StubServer::start(vec![openai_sse_final("classified summary")]).await;
    let trace = Arc::new(RecordingTrace::default());
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
    let model = real_worker_model(&stub, &root.join("runtime"), quotas, trace);
    let h = worker_harness(&root, WorkerLimits::default());
    let target = register_t06_worker(&h, model, "ask_model", "", &["summarize"]);

    let result = call_worker(&h, &target).await;
    let text = success_text(&result);
    assert!(
        !text.contains(STUB_API_KEY),
        "no credential material in any worker-visible byte: {text}"
    );
    // …while the host DID authenticate to the provider (the material lives
    // on the wire leg only).
    let request = &stub.requests()[0];
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {STUB_API_KEY}").as_str())
    );
    stub.stop().await;
}

// ── C06: one shared model budget between the main loop and the callback ─────

struct StepsProvider {
    steps: Mutex<VecDeque<ProviderTurn>>,
}

impl TurnProviderPort for StepsProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let next = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ProviderTurn::Final {
                message: NormalizedMessage {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "done".to_string(),
                    }],
                    model_call_id: None,
                },
            });
        let ctx_at_issue = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, next) })
    }
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_id: None,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
        web_session_id: None,
    }
}

/// Quotas with the GLOBAL MODEL lane pinched to 1 (C06's witness shape);
/// per-agent/session lanes stay loose so the global lane is the only pinch.
fn c06_quota_limits(wait_timeout_ms: u64) -> QuotaLimits {
    QuotaLimits {
        model: LayeredQuotaLimits {
            global: 1,
            per_agent: 4,
            per_session: 4,
        },
        wait_timeout_ms,
        ..QuotaLimits::default()
    }
}

/// Positive leg: a run whose model turn asks for the worker tool releases
/// its model permit before tool execution (runs.rs drops it right after
/// the turn resolves), so the worker's callback acquires the SAME global
/// lane and completes — the run finishes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c06_main_run_worker_and_callback_complete_under_global_model_concurrency_one() {
    let root = unique_dir("c06-pos");
    let home = unique_dir("c06-pos-home");
    let stub = StubServer::start(vec![openai_sse_final("mid-run summary")]).await;
    let trace = Arc::new(RecordingTrace::default());

    let h = worker_harness(&root, WorkerLimits::default());
    let provider = Arc::new(StepsProvider {
        steps: Mutex::new(VecDeque::new()),
    });
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
        tool_gateway: Some(Arc::clone(&h.gateway)),
        approval_gate: Some(Arc::new(ApprovalService::new(Arc::new(
            lingxi_service::inject::SystemClock,
        )))
            as Arc<dyn lingxi_service::approval::ApprovalGate>),
        quota_limits: c06_quota_limits(5_000),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");

    // The callback chain shares the supervisor's quota manager (C06).
    let model = real_worker_model(
        &stub,
        &root.join("runtime"),
        state.runs().quotas_shared(),
        trace.clone(),
    );
    let target = register_t06_worker(&h, model, "ask_model", "", &["summarize"]);
    provider
        .steps
        .lock()
        .unwrap()
        .push_back(ProviderTurn::ToolRequests {
            content: Vec::new(),
            requests: vec![ToolRequest::from_effective_arguments(
                target.as_str(),
                json!({"input": "input.txt"}),
                &budget(),
            )
            .expect("effective request")],
        });

    let storage = Arc::clone(state.storage());
    let run_id = state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            "run the worker",
            1_790_409_600_000,
        )
        .await
        .expect("execute accepted")
        .run_id;
    let status = state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("status query")
        .expect("run row");
    assert_eq!(status, "completed", "the run finishes THROUGH the callback");
    assert_eq!(stub.hits(), 1, "the callback reached the provider once");
    let records = trace.records.lock().expect("trace").clone();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].slot, "summarize");
    stub.stop().await;
}

/// Negative leg: with the only global model permit HELD elsewhere, the
/// callback's bounded admission wait refuses loudly (budget) instead of
/// deadlocking the invocation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c06_a_held_global_model_permit_makes_the_callback_refuse_bounded_not_deadlock() {
    let root = unique_dir("c06-neg");
    let stub = StubServer::start(vec![]).await; // any contact is loud
    let trace = Arc::new(RecordingTrace::default());
    let quotas = Arc::new(QuotaManager::new(c06_quota_limits(400)));
    let model = real_worker_model(&stub, &root.join("runtime"), Arc::clone(&quotas), trace);
    let h = worker_harness(&root, WorkerLimits::default());
    let target = register_t06_worker(&h, model, "ask_model", "", &["summarize"]);

    // Hold the ONLY global model permit outside the worker path.
    let held = quotas
        .acquire(QuotaResource::Model, "external-holder", "sess_elsewhere")
        .await
        .expect("the single global permit");
    let started = std::time::Instant::now();
    let result = call_worker(&h, &target).await;
    let elapsed = started.elapsed();
    drop(held);

    let text = success_text(&result);
    let report: serde_json::Value = serde_json::from_str(&text).expect("report json");
    assert_eq!(report["reply"]["ok"], json!(false), "{text}");
    assert_eq!(
        report["reply"]["error"]["code"],
        json!("model_budget_exceeded"),
        "the bounded admission wait refuses with the budget code: {text}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "bounded, not a deadlock: {elapsed:?}"
    );
    assert_eq!(
        stub.hits(),
        0,
        "the refused callback never reached the wire"
    );
}
