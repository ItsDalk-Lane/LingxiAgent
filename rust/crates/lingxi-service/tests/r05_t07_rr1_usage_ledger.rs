//! R05 RR1 F21/F22 permanent regressions at the REAL service level:
//! per-physical-attempt accounting facts (not-sent vs sent, unknown usage
//! never vanished), parent/child JOIN through the REAL worker subprocess
//! chain, operation-plane call context, and the extended usage query
//! surface (date/purpose/model filters + timing/outcome columns).
//!
//! Old-red anchors migrated from the adversarial audit probes P4/P5/P8
//! (audit/worker_usage, `usage_probes.log`) with the assertion texts
//! preserved. Harness boundary: the model HTTP far end is a scripted
//! loopback stub (the ONLY double); the gateway, CredentialService, real
//! SSE/JSON protocol adapters, worker RPC subprocess chain, QuotaManager
//! and the REAL `RunDatabase` usage ledger are all production code.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_kernel::ports::StoragePort as _;
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::usage::{CallOutcome, ModelCallUsageRecord, ModelUsageQuery};
use lingxi_kernel::{Principal as KernelPrincipal, RunContext, RunFinish};
use lingxi_service::runs::{DriveAuthorization, RunSupervisor};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── the loopback stub (the wire's far end) ───────────────────────────────────

struct RecordedRequest {
    path: String,
    #[allow(dead_code)]
    headers: Vec<(String, String)>,
    #[allow(dead_code)]
    body: String,
}

impl RecordedRequest {
    #[allow(dead_code)]
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

struct StubResponse {
    status: u16,
    body: String,
    content_type: &'static str,
}

impl StubResponse {
    fn immediate(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "application/json",
        }
    }

    fn sse(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            content_type: "text/event-stream",
        }
    }
}

struct StubServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start(scripts: Vec<(&str, Vec<StubResponse>)>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let scripts: Arc<Mutex<BTreeMap<String, VecDeque<StubResponse>>>> = Arc::new(Mutex::new(
            scripts
                .into_iter()
                .map(|(path, queue)| (path.to_string(), VecDeque::from(queue)))
                .collect(),
        ));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn({
            let requests = Arc::clone(&requests);
            let scripts = Arc::clone(&scripts);
            async move {
                loop {
                    let accepted = tokio::select! {
                        _ = &mut shutdown_rx => break,
                        accepted = listener.accept() => accepted,
                    };
                    let Ok((mut socket, _)) = accepted else { break };
                    tokio::spawn({
                        let requests = Arc::clone(&requests);
                        let scripts = Arc::clone(&scripts);
                        async move {
                            let recorded = read_request(&mut socket).await;
                            let next = scripts
                                .lock()
                                .expect("scripts")
                                .get_mut(&recorded.path)
                                .and_then(VecDeque::pop_front);
                            requests.lock().expect("requests").push(recorded);
                            let response = match next {
                                Some(scripted) => build_response(
                                    scripted.status,
                                    scripted.content_type,
                                    &scripted.body,
                                ),
                                None => build_response(
                                    500,
                                    "application/json",
                                    r#"{"error":{"message":"stub script exhausted or unknown path"}}"#,
                                ),
                            };
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.shutdown().await;
                        }
                    });
                }
            }
        });
        Self {
            addr,
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    fn hits_for(&self, path: &str) -> usize {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .filter(|r| r.path == path)
            .count()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

fn build_response(status: u16, content_type: &str, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        500 => "Internal Server Error",
        _ => "Status",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
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
        if let Some(pos) = find_subslice(&raw, b"\r\n\r\n") {
            break pos;
        }
        assert!(raw.len() < 256 * 1024, "stub: header block too large");
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("utf8 headers");
    let mut lines = head.split("\r\n");
    let request_line = lines.next().expect("request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("request path")
        .to_string();
    let mut content_length = 0_usize;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().expect("numeric content-length");
            }
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub read stalled")
            .expect("stub read");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8(raw[body_start..body_start + content_length].to_vec())
        .expect("body utf8");
    RecordedRequest {
        path,
        headers,
        body,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn server_error(detail: &str) -> StubResponse {
    StubResponse::immediate(
        500,
        serde_json::json!({"error": {"message": detail}}).to_string(),
    )
}

// ── plane builders ───────────────────────────────────────────────────────────

fn plane_with_aux(endpoint: &str, key: &str) -> String {
    format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": {key}}}
            }},
            "aux": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": {key}}}
            }}
        }},
        "models": {{
            "chat": {{"provider": "main", "model": "stub-model"}},
            "summarize": {{"provider": "aux", "model": "summarize-model"}}
        }}"#,
        endpoint = serde_json::to_string(endpoint).expect("json"),
        key = serde_json::to_string(key).expect("json"),
    )
}

fn plane_with_embedding(endpoint: &str, embed_key: &str) -> String {
    format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "RR1_T07_MAIN_KEY"}}
            }},
            "embed": {{
                "protocol": "openai-embeddings",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": {key}}}
            }}
        }},
        "models": {{
            "chat": {{"provider": "main", "model": "chat"}},
            "embedding": {{"provider": "embed", "model": "embed-model"}}
        }}"#,
        endpoint = serde_json::to_string(endpoint).expect("json"),
        key = serde_json::to_string(embed_key).expect("json"),
    )
}

// ── harness ──────────────────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t07rr1-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn config_for(home: &Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

struct PlaneBoot {
    state: ServiceState,
    home: PathBuf,
}

async fn boot(tag: &str, plane_json: &str) -> PlaneBoot {
    let home = unique_dir(&format!("{tag}-home"));
    let workspace = unique_dir(&format!("{tag}-ws"));
    let config_root = unique_dir(&format!("{tag}-cfg"));
    let config_path = config_root.join("service.json");
    let config_json = format!(
        r#"{{"home": {}, "workspace": {}, {plane_json}}}"#,
        serde_json::to_string(&home.to_string_lossy()).expect("home json"),
        serde_json::to_string(&workspace.to_string_lossy()).expect("ws json"),
    );
    std::fs::write(&config_path, &config_json).expect("write config");
    let layout = prepare_layout(&home).expect("layout");
    let file = lingxi_service::config::read_service_config(&config_path).expect("service config");
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .expect("plane resolves")
            .expect("plane present");
    let credential_service = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credential service"),
    );
    let deps = ServiceDeps {
        model_gateway: Some(Arc::new(
            lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
        )),
        model_plane_source: Some(source),
        workspace_root: file.workspace,
        credential_service: Some(credential_service),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    PlaneBoot { state, home }
}

async fn usage_rows(state: &ServiceState, query: ModelUsageQuery) -> Vec<ModelCallUsageRecord> {
    state
        .storage()
        .query_model_call_usage(query)
        .await
        .expect("usage query")
}

async fn teardown(boot: PlaneBoot) {
    boot.state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(boot.home);
}

fn run_ctx(session: &str, run: &str) -> RunContext {
    RunContext {
        principal: KernelPrincipal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(session),
        run_id: lingxi_protocol::RunId::new(run),
        attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}

// ── P4 (old-red anchor): a physically sent FAILED worker request must be
// accounted — unknown rather than vanished ────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f21_worker_failed_physical_request_must_leave_unknown_usage_row() {
    let stub = StubServer::start(vec![(
        "/v1/chat/completions",
        vec![server_error("audit upstream failure")],
    )])
    .await;
    let boot_res = boot(
        "rr1-f21-worker-failed",
        &plane_with_aux(&stub.endpoint(), "RR1_T07_AUX_KEY"),
    )
    .await;
    let state = &boot_res.state;
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .unwrap();
    let model = lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            state.storage().clone(),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    );
    let ctx = run_ctx("sess_rr1_f21", "run-rr1-f21");
    use lingxi_service::workerrpc::WorkerModelPort as _;
    let parent_call = lingxi_protocol::ToolCallId::new("run-rr1-f21-tc0001");
    let reply = model
        .complete(
            &ctx,
            "plug",
            "rr1-invocation",
            "cb-fail",
            &parent_call,
            &lingxi_service::workerrpc::WorkerModelRequest {
                prompt: "summarize this".into(),
                purpose: "summarize".into(),
                max_output_tokens: 64,
                deadline_unix_ms: None,
            },
        )
        .await;
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    let requests = stub.hits_for("/v1/chat/completions");
    println!(
        "RR1 worker failure: provider_requests={requests}, usage_rows={}, reply={reply:?}",
        rows.len()
    );
    teardown(boot_res).await;
    stub.stop().await;
    assert_eq!(requests, 1, "probe must reach the real provider HTTP path");
    assert!(reply.is_err());
    assert_eq!(
        rows.len(),
        1,
        "a physically sent failed worker request must be accounted, unknown rather than vanished"
    );
    let row = &rows[0];
    assert_eq!(
        row.transport_attempts,
        Some(1),
        "exactly the one physical request"
    );
    assert!(
        row.usage.is_none() && row.invalid_detail.is_none(),
        "no usable usage arrived — the row stays unknown, never zero"
    );
    assert_eq!(row.origin, "worker-callback");
    assert_eq!(row.run_id.as_deref(), Some("run-rr1-f21"));
    // F21: the failed call's row carries the outcome + timing + the REAL
    // parent tool join key (not the random RPC invocation id alone).
    assert_eq!(row.outcome, lingxi_kernel::usage::CallOutcome::Failed);
    assert!(row.started_at_unix_ms.is_some() && row.settled_at_unix_ms.is_some());
    assert_eq!(
        row.parent_tool_call_id.as_deref(),
        Some("run-rr1-f21-tc0001")
    );
    assert_eq!(row.cause_ref.as_deref(), Some("rr1-invocation"));
}

// ── P5 (old-red anchor, F22 at the persistence boundary): a malformed usage
// payload must not carry provider secrets into the permanent ledger ──────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f22_operation_invalid_usage_must_not_persist_secret_payload() {
    let synthetic_secret = "RR1_T07_AUDIT_ONLY_EMBEDDING_CREDENTIAL";
    let body = serde_json::json!({
        "data":[{"index":0,"embedding":[0.25,0.75]}],
        "usage":[{"authorization":format!("Bearer {synthetic_secret}")}]
    });
    let stub = StubServer::start(vec![(
        "/v1/embeddings",
        vec![StubResponse::immediate(200, body.to_string())],
    )])
    .await;
    let boot_res = boot(
        "rr1-f22-op-secret",
        &plane_with_embedding(&stub.endpoint(), synthetic_secret),
    )
    .await;
    let result = boot_res
        .state
        .operations()
        .unwrap()
        .embed(
            lingxi_adapters::models::operations::embedding::EmbeddingRequest {
                inputs: vec!["audit input".into()],
                dimensions: Some(2),
                context_window: None,
                input_type: Default::default(),
            },
            None,
            None,
        )
        .await;
    let rows = usage_rows(&boot_res.state, ModelUsageQuery::default()).await;
    let leaked = rows.iter().any(|row| {
        row.invalid_detail
            .as_deref()
            .unwrap_or("")
            .contains(synthetic_secret)
    });
    println!(
        "RR1 operation secret: result_ok={}, rows={}, leaked={leaked}, invalid_detail={:?}",
        result.is_ok(),
        rows.len(),
        rows.first().and_then(|r| r.invalid_detail.clone())
    );
    teardown(boot_res).await;
    stub.stop().await;
    assert!(result.is_ok(), "valid embedding still returns");
    assert_eq!(rows.len(), 1);
    assert!(
        !leaked,
        "even malformed usage must not copy credential-bearing provider payload into the \
         permanent ledger"
    );
    // The operation itself succeeded while its usage fact is invalid — the
    // two facts stay separate (F22: no fabricated usage to clear a diagnostic).
    assert!(
        rows[0].invalid_detail.is_some(),
        "the malformed fact is invalid"
    );
    assert!(rows[0].usage.is_none(), "no coerced usage either");
}

// ── P8 attempts half (old-red anchor): a queue-timeout call NEVER invented a
// physical attempt (the deadline half is F16's, already fixed) ────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f21_operation_queue_timeout_never_invents_an_http_attempt() {
    use lingxi_service::quotas::QuotaResource;
    let stub = StubServer::start(vec![(
        "/v1/embeddings",
        vec![StubResponse::immediate(
            200,
            "{\"data\":[{\"index\":0,\"embedding\":[0.25,0.75]}]}",
        )],
    )])
    .await;
    let boot_res = boot(
        "rr1-f21-op-deadline",
        &plane_with_embedding(&stub.endpoint(), "RR1_T07_EMBED_KEY"),
    )
    .await;
    let quotas = boot_res.state.runs().quotas_shared();
    let mut permits = Vec::new();
    for n in 0..8 {
        permits.push(
            quotas
                .acquire(
                    QuotaResource::Model,
                    &format!("holder-{n}"),
                    &format!("holder-session-{n}"),
                )
                .await
                .unwrap(),
        );
    }
    let released = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(180)).await;
        drop(permits);
    });
    let started = std::time::Instant::now();
    let deadline = lingxi_adapters::models::dispatch::unix_ms_now() + 25;
    let result = boot_res
        .state
        .operations()
        .unwrap()
        .embed(
            lingxi_adapters::models::operations::embedding::EmbeddingRequest {
                inputs: vec!["audit".into()],
                dimensions: Some(2),
                context_window: None,
                input_type: Default::default(),
            },
            Some(deadline),
            None,
        )
        .await;
    let elapsed = started.elapsed().as_millis();
    let rows = usage_rows(&boot_res.state, ModelUsageQuery::default()).await;
    let requests = stub.hits_for("/v1/embeddings");
    println!(
        "RR1 operation deadline: elapsed_ms={elapsed}, http={requests}, rows={}, \
         attempts={:?}, result={result:?}",
        rows.len(),
        rows.iter()
            .map(|r| r.transport_attempts)
            .collect::<Vec<_>>()
    );
    released.await.unwrap();
    teardown(boot_res).await;
    stub.stop().await;
    assert!(result.is_err());
    assert_eq!(requests, 0, "nothing left the process");
    assert!(
        elapsed < 120,
        "25ms deadline must include quota wait, not settle only after a 180ms held permit \
         releases"
    );
    assert_eq!(
        rows.len(),
        1,
        "the refused call still leaves its accounting row (a real invocation was made and \
         settled pre-send — the fact is not-sent, not absent)"
    );
    assert_eq!(
        rows[0].transport_attempts,
        Some(0),
        "an unsent request is NEVER recorded as one physical attempt"
    );
    assert!(
        rows[0].usage.is_none(),
        "no usage exists for a call that never left the process"
    );
    // F21: the refused call's outcome/timing columns state the pre-send
    // settlement.
    assert_eq!(rows[0].outcome, CallOutcome::Failed);
    assert!(rows[0].started_at_unix_ms.is_some());
}

// ── F21: the REAL worker subprocess chain — the ledger JOIN reaches the
// parent tool AND the parent model call ───────────────────────────────────────

/// One openai-completions SSE tool-call turn (the r05_t08 wire shape).
fn openai_sse_tool_call(call_id: &str, name: &str, arguments: &str) -> String {
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-rr1", "model": "stub-model",
            "choices": [{
                "index": 0, "finish_reason": null,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0, "id": call_id, "type": "function",
                        "function": {"name": name, "arguments": arguments}
                    }]
                }
            }]
        }),
        serde_json::json!({
            "id": "chatcmpl-rr1",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
        }),
        serde_json::json!({
            "id": "chatcmpl-rr1", "choices": [],
            "usage": {"prompt_tokens": 30, "completion_tokens": 11}
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body
}

/// One openai-completions SSE final with usage.
fn openai_sse_final(text: &str, input: u64, output: u64) -> String {
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-rr1", "model": "stub-model",
            "choices": [{
                "index": 0, "finish_reason": null,
                "delta": {"role": "assistant", "content": text}
            }]
        }),
        serde_json::json!({
            "id": "chatcmpl-rr1",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
        serde_json::json!({
            "id": "chatcmpl-rr1", "choices": [],
            "usage": {"prompt_tokens": input, "completion_tokens": output}
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rr1_f21_worker_parent_join_through_the_real_subprocess_chain() {
    use lingxi_service::workerrpc::{
        register_worker_tool, BoundedWorkerModel, WorkerLimits, WorkerModelPort, WorkerRuntime,
        WorkerToolSpec, WORKER_CALLBACK_MAX_OUTPUT_TOKENS, WORKER_MAX_CALLBACKS_PER_CALL,
    };
    // The registry target id of a plugin worker is deterministic; the WIRE
    // name the provider must use is the sanitized local_name.
    let target = "tool:plugin:rr1worker:rr1_join_worker";
    let wire_name = "rr1_join_worker";
    // Wire order: (1) the chat turn emits the worker TOOL CALL, (2) the
    // worker's model callback rides the aux summarize slot, (3) the final.
    let stub = StubServer::start(vec![(
        "/v1/chat/completions",
        vec![
            StubResponse::sse(openai_sse_tool_call(
                "call-join-1",
                wire_name,
                r#"{"input":"input.txt"}"#,
            )),
            StubResponse::sse(openai_sse_final("host-side summary", 17, 5)),
            StubResponse::sse(openai_sse_final("all done", 40, 7)),
        ],
    )])
    .await;
    let endpoint = stub.endpoint();
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "RR1_T07_JOIN_MAIN_KEY"}}
            }},
            "aux": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "RR1_T07_JOIN_AUX_KEY"}}
            }}
        }},
        "models": {{
            "chat": {{"provider": "main", "model": "stub-model", "capabilities": {{"tools": true}}}},
            "summarize": {{"provider": "aux", "model": "summarize-model"}}
        }}"#,
        endpoint = serde_json::to_string(&endpoint).expect("json"),
    );
    let boot_res = boot("rr1-f21-join", &plane_json).await;
    let state = &boot_res.state;
    let storage = Arc::clone(state.storage());

    // The REAL worker harness (the r05_t06 pattern): registry + gateway +
    // approvals + runtime + a REAL worker subprocess whose model callback
    // rides the REAL aux chain into the PRODUCTION ledger trace on this
    // storage.
    let probe = unique_dir("rr1-f21-join-ws");
    let ws = probe.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    std::fs::write(ws.join("input.txt"), "granted-content\n").expect("input file");
    let registry = Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new());
    let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
        Arc::new(lingxi_service::inject::SystemClock),
    ));
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let tool_gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(lingxi_service::inject::SystemClock),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    let runtime = Arc::new(WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    ));
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("aux executor");
    let gateway_model = lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            Arc::clone(&storage),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    );
    let model: Arc<dyn WorkerModelPort> = BoundedWorkerModel::new(
        Some(Arc::new(gateway_model)),
        WORKER_MAX_CALLBACKS_PER_CALL,
        WORKER_CALLBACK_MAX_OUTPUT_TOKENS,
    );
    let spec = WorkerToolSpec {
        plugin_id: "rr1worker".to_string(),
        op: "probe".to_string(),
        local_name: "rr1_join_worker".to_string(),
        description: "RR1 F21 join probe worker".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
            "additionalProperties": false
        }),
        path_args: vec!["input".to_string()],
        argv: vec![
            env!("CARGO_BIN_EXE_r04_t07_fixture").to_string(),
            "ask_model".to_string(),
        ],
        env: BTreeMap::new(),
        cwd: ws.clone(),
        model,
        allowed_model_purposes: ["summarize"].iter().map(|s| s.to_string()).collect(),
        claimed_file_contract: None,
    };
    let registered = register_worker_tool(
        &registry,
        tool_gateway.as_ref(),
        &runtime,
        &access,
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
        spec,
    )
    .expect("worker registers");
    assert_eq!(registered.target.as_str(), target, "the scripted tool name");
    // The plugin-lifecycle availability transition the composition root
    // performs when it adopts a registered worker (F24 owns the production
    // registration; the catalog mutation API is the real surface).
    registry
        .set_availability(
            &registered.target,
            lingxi_kernel::toolcatalog::Availability::Available,
        )
        .expect("worker becomes available");

    // The REAL run driver over the REAL provider chain, dispatching tools
    // through the worker-bound gateway.
    let provider = lingxi_adapters::models::provider::GatewayedProvider::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("provider");
    let supervisor = Arc::new(
        RunSupervisor::new(
            Some(Arc::new(provider) as Arc<dyn lingxi_kernel::ports::TurnProviderPort>),
            None,
            lingxi_service::runs::RunDriveLimits::default(),
            lingxi_service::quotas::QuotaManager::new(
                lingxi_service::quotas::QuotaLimits::default(),
            ),
            lingxi_service::cancel::CancelPolicy::default(),
            Some(
                Arc::new(lingxi_service::approval_service::ApprovalService::new(
                    Arc::new(lingxi_service::inject::SystemClock),
                )) as Arc<dyn lingxi_service::approval::ApprovalGate>,
            ),
            None,
        )
        .expect("supervisor")
        .with_tool_gateway(Some(Arc::clone(&tool_gateway))),
    );
    let run_id = format!("run-rr1-join-{}", std::process::id());
    // The driver requires the session row to exist (the production surface
    // creates it at admission); seed it through the real store API.
    storage
        .ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
            session_id: "sess_rr1_join".to_string(),
            agent_id: "lingxi".to_string(),
            owner_user_id: lingxi_service::LOCAL_OWNER_USER_ID.to_string(),
            title: "rr1 f21 join".to_string(),
            created_at_unix_ms: 1_790_409_600_000_i64,
        }])
        .await
        .expect("session seed");
    let finish = supervisor
        .drive_run(
            storage.as_ref(),
            state.events(),
            &KernelPrincipal::LocalUser,
            "sess_rr1_join",
            "lingxi",
            &run_id,
            "summarize the file via the worker",
            1,
            1_790_409_600_000,
            None,
            None,
            DriveAuthorization::user_submission(None, SessionPermissionMode::Operate),
            None,
            "sess_rr1_join",
        )
        .await
        .expect("drive settles");
    assert!(
        matches!(finish, RunFinish::CompletedWithFinal { .. }),
        "the chain completed: {finish:?}"
    );
    let requests = stub.hits_for("/v1/chat/completions");
    // ── the ledger JOIN verdict (queried before teardown) ──
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    teardown(boot_res).await;
    stub.stop().await;
    assert_eq!(requests, 3, "tool turn + aux callback + final");
    // The parent model call (the tool turn) names the emitted tool call.
    let parent = rows
        .iter()
        .find(|row| row.purpose == "chat" && !row.emitted_tool_calls.is_empty())
        .expect("the tool-turn chat row carries its emitted tool calls");
    assert_eq!(parent.run_id.as_deref(), Some(run_id.as_str()));
    let parent_tool = parent.emitted_tool_calls[0].clone();
    assert!(
        parent_tool.starts_with(&run_id) && parent_tool.contains("tc"),
        "a driver-minted tool call id: {parent_tool}"
    );
    // The child (worker callback) row joins to that EXACT tool call — the
    // real ToolCallId the executor passed through the RPC, never a
    // test-minted lookalike (the run above produced both ends).
    let child = rows
        .iter()
        .find(|row| row.origin == "worker-callback")
        .expect("the worker callback row exists");
    assert_eq!(
        child.parent_tool_call_id.as_deref(),
        Some(parent_tool.as_str()),
        "the child joins the REAL parent tool call id"
    );
    assert_eq!(child.run_id, parent.run_id);
    assert_eq!(child.session_id, parent.session_id);
    assert_eq!(child.outcome, CallOutcome::Succeeded);
    // The second hop: the parent MODEL call is reachable from the child
    // through the ledger alone (same run, emitted list contains the join
    // key).
    let parent_model_rows: Vec<&ModelCallUsageRecord> = rows
        .iter()
        .filter(|row| {
            row.run_id == child.run_id
                && row
                    .emitted_tool_calls
                    .iter()
                    .any(|id| id == child.parent_tool_call_id.as_deref().unwrap_or(""))
        })
        .collect();
    assert_eq!(
        parent_model_rows.len(),
        1,
        "exactly ONE parent model call claims the tool batch"
    );
    assert_eq!(parent_model_rows[0].model_call_id, parent.model_call_id);
    // The callback's usage fact is the AUX stub's reported numbers.
    match &child.usage {
        Some(usage) => {
            assert_eq!(usage.input_tokens, Some(17));
            assert_eq!(usage.output_tokens, Some(5));
        }
        None => panic!("the aux slot's usage arrived on the child row"),
    }
    assert_eq!(child.transport_attempts, Some(1));
    assert!(child.started_at_unix_ms.is_some() && child.settled_at_unix_ms.is_some());
}

// ── F21: operation context carries real session/run/causal anchor ───────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f21_operation_context_carries_session_run_and_cause() {
    let stub = StubServer::start(vec![(
        "/v1/embeddings",
        vec![StubResponse::immediate(
            200,
            serde_json::json!({
                "data":[{"index":0,"embedding":[0.25,0.75]}],
                "usage":{"prompt_tokens":5,"total_tokens":9}
            })
            .to_string(),
        )],
    )])
    .await;
    let boot_res = boot(
        "rr1-f21-op-context",
        &plane_with_embedding(&stub.endpoint(), "RR1_T07_EMBED_KEY"),
    )
    .await;
    let context = lingxi_service::operations::OperationCallContext {
        session_id: "sess_rr1_ctx".to_string(),
        run_id: "run-rr1-ctx".to_string(),
        attempt: Some("run-rr1-ctx#a1".to_string()),
        cause_ref: Some("run-rr1-ctx-tc0007".to_string()),
    };
    let result = boot_res
        .state
        .operations()
        .unwrap()
        .embed(
            lingxi_adapters::models::operations::embedding::EmbeddingRequest {
                inputs: vec!["context-bound input".into()],
                dimensions: Some(2),
                context_window: None,
                input_type: Default::default(),
            },
            None,
            Some(&context),
        )
        .await;
    assert!(result.is_ok(), "embedding succeeds: {result:?}");
    let rows = usage_rows(&boot_res.state, ModelUsageQuery::default()).await;
    teardown(boot_res).await;
    stub.stop().await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.session_id.as_deref(), Some("sess_rr1_ctx"));
    assert_eq!(row.run_id.as_deref(), Some("run-rr1-ctx"));
    assert_eq!(row.attempt.as_deref(), Some("run-rr1-ctx#a1"));
    assert_eq!(row.cause_ref.as_deref(), Some("run-rr1-ctx-tc0007"));
    assert_eq!(row.outcome, CallOutcome::Succeeded);
    assert!(row.started_at_unix_ms.is_some() && row.settled_at_unix_ms.is_some());
    assert!(row.settled_at_unix_ms.unwrap() >= row.started_at_unix_ms.unwrap());
    // The usage fact itself decodes (the plane-root independence is the
    // None-context case — pinned by the P5/P8 legs above).
    assert!(row.usage.is_some());
}

// ── F21: the query surface — date/category/model filters + timing ────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f21_usage_query_filters_by_date_purpose_and_model() {
    let stub = StubServer::start(vec![(
        "/v1/embeddings",
        vec![
            StubResponse::immediate(
                200,
                serde_json::json!({"data":[{"index":0,"embedding":[0.1,0.2]}],
                    "usage":{"prompt_tokens":5,"total_tokens":9}})
                .to_string(),
            ),
            StubResponse::immediate(
                200,
                serde_json::json!({"data":[{"index":0,"embedding":[0.3,0.4]}],
                    "usage":{"prompt_tokens":6,"total_tokens":10}})
                .to_string(),
            ),
        ],
    )])
    .await;
    let boot_res = boot(
        "rr1-f21-query",
        &plane_with_embedding(&stub.endpoint(), "RR1_T07_EMBED_KEY"),
    )
    .await;
    let operations = boot_res.state.operations().unwrap();
    for _ in 0..2 {
        operations
            .embed(
                lingxi_adapters::models::operations::embedding::EmbeddingRequest {
                    inputs: vec!["q".into()],
                    dimensions: Some(2),
                    context_window: None,
                    input_type: Default::default(),
                },
                None,
                None,
            )
            .await
            .expect("embed");
    }
    let before = lingxi_adapters::models::dispatch::unix_ms_now();
    let rows = usage_rows(&boot_res.state, ModelUsageQuery::default()).await;
    assert_eq!(rows.len(), 2, "both rows visible to the internal query");
    let recorded = rows[0]
        .started_at_unix_ms
        .expect("timing present on the driver-written row")
        .max(rows[0].settled_at_unix_ms.unwrap_or(0));
    assert!(
        recorded <= before + 5_000,
        "sane clock: {recorded} vs {before}"
    );

    // Category (purpose) filter.
    let embedding_only = usage_rows(
        &boot_res.state,
        ModelUsageQuery {
            purpose: Some("embedding".to_string()),
            ..ModelUsageQuery::default()
        },
    )
    .await;
    assert_eq!(embedding_only.len(), 2);
    let chat_only = usage_rows(
        &boot_res.state,
        ModelUsageQuery {
            purpose: Some("chat".to_string()),
            ..ModelUsageQuery::default()
        },
    )
    .await;
    assert!(chat_only.is_empty(), "no chat rows in this boot");

    // Model filter.
    let model_rows = usage_rows(
        &boot_res.state,
        ModelUsageQuery {
            model: Some("embed-model".to_string()),
            ..ModelUsageQuery::default()
        },
    )
    .await;
    assert_eq!(model_rows.len(), 2);
    assert!(model_rows.iter().all(|row| row.model == "embed-model"));
    let wrong_model = usage_rows(
        &boot_res.state,
        ModelUsageQuery {
            model: Some("no-such-model".to_string()),
            ..ModelUsageQuery::default()
        },
    )
    .await;
    assert!(wrong_model.is_empty());

    // Date window: the future excludes everything; the past-to-now window
    // includes both rows.
    let future = usage_rows(
        &boot_res.state,
        ModelUsageQuery {
            recorded_from_unix_ms: Some(before + 60_000),
            ..ModelUsageQuery::default()
        },
    )
    .await;
    assert!(future.is_empty());
    let window = usage_rows(
        &boot_res.state,
        ModelUsageQuery {
            recorded_from_unix_ms: Some(before - 3_600_000),
            recorded_to_unix_ms: Some(before + 3_600_000),
            ..ModelUsageQuery::default()
        },
    )
    .await;
    assert_eq!(window.len(), 2);
    // Combined filters.
    let combined = usage_rows(
        &boot_res.state,
        ModelUsageQuery {
            purpose: Some("embedding".to_string()),
            model: Some("embed-model".to_string()),
            recorded_from_unix_ms: Some(before - 3_600_000),
            recorded_to_unix_ms: Some(before + 3_600_000),
            ..ModelUsageQuery::default()
        },
    )
    .await;
    assert_eq!(combined.len(), 2);
    teardown(boot_res).await;
    stub.stop().await;
}

// ── F21: a pre-send refusal on the worker lane records ZERO attempts ─────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f21_worker_pre_send_refusal_records_zero_attempts() {
    // The aux (summarize) provider is OAuth-configured but never logged
    // in: credential resolution refuses BEFORE anything leaves the
    // process — the not-sent fact (0 attempts), never a fabricated 1.
    let stub = StubServer::start(vec![(
        "/v1/chat/completions",
        vec![StubResponse::immediate(
            200,
            serde_json::json!({"unused": true}).to_string(),
        )],
    )])
    .await;
    let endpoint = stub.endpoint();
    let token_endpoint = format!("http://{}/token", stub.addr);
    let device_endpoint = format!("{token_endpoint}/device");
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "RR1_T07_MAIN_KEY"}}
            }},
            "aux": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "oauth", "flow": "deviceCode",
                    "clientId": "client-rr1-f21",
                    "tokenEndpoint": {token_endpoint},
                    "deviceAuthorizationEndpoint": {device_endpoint}}}
            }}
        }},
        "models": {{
            "chat": {{"provider": "main", "model": "stub-model"}},
            "summarize": {{"provider": "aux", "model": "summarize-model"}}
        }}"#,
        endpoint = serde_json::to_string(&endpoint).expect("json"),
        token_endpoint = serde_json::to_string(&token_endpoint).expect("json"),
        device_endpoint = serde_json::to_string(&device_endpoint).expect("json"),
    );
    let boot_res = boot("rr1-f21-presend", &plane_json).await;
    let state = &boot_res.state;
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .unwrap();
    let model = lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            state.storage().clone(),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    );
    let ctx = run_ctx("sess_rr1_presend", "run-rr1-presend");
    use lingxi_service::workerrpc::WorkerModelPort as _;
    let parent_call = lingxi_protocol::ToolCallId::new("run-rr1-presend-tc0001");
    let reply = model
        .complete(
            &ctx,
            "plug",
            "rr1-presend-inv",
            "cb-1",
            &parent_call,
            &lingxi_service::workerrpc::WorkerModelRequest {
                prompt: "summarize this".into(),
                purpose: "summarize".into(),
                max_output_tokens: 64,
                deadline_unix_ms: None,
            },
        )
        .await;
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    let requests = stub.hits_for("/v1/chat/completions");
    teardown(boot_res).await;
    stub.stop().await;
    assert!(reply.is_err(), "the callback refuses honestly");
    assert_eq!(requests, 0, "nothing left the process");
    assert_eq!(rows.len(), 1, "the pre-send refusal still accounts");
    assert_eq!(
        rows[0].transport_attempts,
        Some(0),
        "a credential-refused callback is NOT-SENT — zero physical attempts, never 1"
    );
    assert!(rows[0].usage.is_none(), "unknown usage, never zero");
    assert_eq!(rows[0].outcome, CallOutcome::Failed);
    assert_eq!(
        rows[0].parent_tool_call_id.as_deref(),
        Some("run-rr1-presend-tc0001")
    );
}

// ── F21 honesty leg: an empty-text final STILL carries its usage fact ────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f21_empty_answer_failure_keeps_the_reported_usage() {
    // A well-formed SSE final whose text is EMPTY but whose usage object
    // arrived: the auxiliary call FAILS honestly (no fabricated text) and
    // the accounting row keeps the provider's REAL numbers (usage ≠ 0,
    // never vanished by the failure).
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-rr1", "model": "stub-model",
            "choices": [{
                "index": 0, "finish_reason": null,
                "delta": {"role": "assistant", "content": "   "}
            }]
        }),
        serde_json::json!({
            "id": "chatcmpl-rr1",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
        serde_json::json!({
            "id": "chatcmpl-rr1", "choices": [],
            "usage": {"prompt_tokens": 21, "completion_tokens": 8}
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    let stub = StubServer::start(vec![(
        "/v1/chat/completions",
        vec![StubResponse::sse(body)],
    )])
    .await;
    let boot_res = boot(
        "rr1-f21-empty-usage",
        &plane_with_aux(&stub.endpoint(), "RR1_T07_AUX_KEY"),
    )
    .await;
    let state = &boot_res.state;
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .unwrap();
    let model = lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            state.storage().clone(),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    );
    let ctx = run_ctx("sess_rr1_empty", "run-rr1-empty");
    use lingxi_service::workerrpc::WorkerModelPort as _;
    let reply = model
        .complete(
            &ctx,
            "plug",
            "rr1-empty-inv",
            "cb-1",
            &lingxi_protocol::ToolCallId::new("run-rr1-empty-tc0001"),
            &lingxi_service::workerrpc::WorkerModelRequest {
                prompt: "summarize this".into(),
                purpose: "summarize".into(),
                max_output_tokens: 64,
                deadline_unix_ms: None,
            },
        )
        .await;
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    let requests = stub.hits_for("/v1/chat/completions");
    teardown(boot_res).await;
    stub.stop().await;
    assert_eq!(requests, 1);
    assert!(reply.is_err(), "an empty answer is an explicit failure");
    assert_eq!(rows.len(), 1, "the failed call still accounts");
    assert_eq!(rows[0].transport_attempts, Some(1));
    assert_eq!(rows[0].outcome, CallOutcome::Failed);
    match &rows[0].usage {
        Some(usage) => {
            assert_eq!(usage.input_tokens, Some(21));
            assert_eq!(usage.output_tokens, Some(8));
        }
        None => panic!("the provider's usage fact survives the empty-answer failure"),
    }
}

// ── R05 RR1 F38 (r1 repair): cancel-after-send accounting — a possibly
// billable in-flight request NEVER vanishes with a cancellation fence ────────

/// A stub that serves SSE requests and PARKS mid-stream: headers plus one
/// first delta frame are written, then the connection is held open (the
/// physical request HAS left the process — the cancel window under test)
/// until released or shut down.
struct ParkedSseStub {
    addr: SocketAddr,
    arrivals: Arc<std::sync::atomic::AtomicUsize>,
    release: tokio::sync::watch::Sender<bool>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl ParkedSseStub {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("parked stub bind");
        let addr = listener.local_addr().expect("parked stub addr");
        let arrivals = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (release_tx, release_rx) = tokio::sync::watch::channel(false);
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let arrivals_for_task = Arc::clone(&arrivals);
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                let mut release_park = release_rx.clone();
                let arrivals = Arc::clone(&arrivals_for_task);
                tokio::spawn(async move {
                    let _recorded = read_request(&mut socket).await;
                    arrivals.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    // The stream OPENS (a real SSE head + one delta) and
                    // then parks: the request is in flight, mid-stream.
                    let head = concat!(
                        "HTTP/1.1 200 OK\r\n",
                        "Content-Type: text/event-stream\r\n",
                        "Cache-Control: no-store\r\n",
                        "\r\n",
                        "data: {\"id\":\"chatcmpl-f38park\",\"choices\":[{\"index\":0,",
                        "\"finish_reason\":null,\"delta\":{\"role\":\"assistant\",",
                        "\"content\":\"\"}}]}\n\n"
                    );
                    if socket.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    // Hold the connection open until released (or dropped).
                    let _ = release_park.changed().await;
                });
            }
        });
        Self {
            addr,
            arrivals,
            release: release_tx,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    fn arrivals(&self) -> usize {
        self.arrivals.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Bounded wait until the physical request count reaches `want` (the
    /// barrier: the test only proceeds once the request is REALLY out).
    async fn wait_arrivals(&self, want: usize, bound: Duration) -> bool {
        let started = std::time::Instant::now();
        while self.arrivals() < want {
            if started.elapsed() > bound {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        true
    }

    async fn stop(mut self) {
        let _ = self.release.send(true);
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

/// The ledger row assertions of a cancel-after-send row: present, honest,
/// nothing fabricated (F38).
fn assert_cancelled_unknown_row(row: &ModelCallUsageRecord) {
    assert_eq!(row.outcome, CallOutcome::Cancelled);
    assert!(
        row.usage.is_none() && row.invalid_detail.is_none(),
        "usage unknown after the drop — never a number, never a diagnostic"
    );
    assert_eq!(
        row.transport_attempts, None,
        "attempts UNKNOWN after the drop — never 0 (not-sent is a DIFFERENT \
         fact), never 1 (a fabricated sent count)"
    );
    assert!(row.started_at_unix_ms.is_some() && row.settled_at_unix_ms.is_some());
}

/// Driver leg: the run is cancelled while the model call's SSE stream is
/// IN FLIGHT (the physical request demonstrably reached the wire first —
/// the barrier). The cancellation settle must not erase the accounting
/// row, and the row must survive a storage restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rr1_f38_driver_cancel_of_in_flight_stream_leads_a_cancelled_row_across_restart() {
    let stub = ParkedSseStub::start().await;
    let endpoint = stub.endpoint();
    let plane_json = plane_with_aux(&endpoint, "RR1_T38_MAIN_KEY");
    let boot_res = boot("rr1-f38-driver-cancel", &plane_json).await;
    let state = &boot_res.state;
    let storage = Arc::clone(state.storage());

    let provider = lingxi_adapters::models::provider::GatewayedProvider::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("provider");
    let supervisor = Arc::new(
        RunSupervisor::new(
            Some(Arc::new(provider) as Arc<dyn lingxi_kernel::ports::TurnProviderPort>),
            None,
            lingxi_service::runs::RunDriveLimits::default(),
            lingxi_service::quotas::QuotaManager::new(
                lingxi_service::quotas::QuotaLimits::default(),
            ),
            lingxi_service::cancel::CancelPolicy::default(),
            None,
            None,
        )
        .expect("supervisor"),
    );
    let run_id = format!("run-rr1-f38-cancel-{}", std::process::id());
    storage
        .ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
            session_id: "sess_rr1_f38".to_string(),
            agent_id: "lingxi".to_string(),
            owner_user_id: lingxi_service::LOCAL_OWNER_USER_ID.to_string(),
            title: "rr1 f38 driver cancel".to_string(),
            created_at_unix_ms: 1_790_409_600_000_i64,
        }])
        .await
        .expect("session seed");

    let drive_supervisor = Arc::clone(&supervisor);
    let drive_run_id = run_id.clone();
    let events = state.events().clone();
    let driven = tokio::spawn(async move {
        drive_supervisor
            .drive_run(
                storage.as_ref(),
                &events,
                &KernelPrincipal::LocalUser,
                "sess_rr1_f38",
                "lingxi",
                &drive_run_id,
                "park mid stream then get cancelled",
                1,
                1_790_409_600_000,
                None,
                None,
                DriveAuthorization::user_submission(None, SessionPermissionMode::Operate),
                None,
                "sess_rr1_f38",
            )
            .await
    });

    // The barrier: the physical request REALLY left the process before
    // the cancellation fires (cancel-after-send, never a pre-send race).
    assert!(
        stub.wait_arrivals(1, Duration::from_secs(10)).await,
        "the parked stub must receive the in-flight request (arrivals={})",
        stub.arrivals()
    );
    assert!(
        matches!(
            supervisor.cancel_run(&run_id, "rr1 f38 driver cancel leg"),
            lingxi_service::cancel::FireOutcome::Fired
        ),
        "the cancellation fires while the call is in flight"
    );
    let finish = tokio::time::timeout(Duration::from_secs(30), driven)
        .await
        .expect("drive settles")
        .expect("drive task alive")
        .expect("drive ok");
    assert!(
        matches!(finish, RunFinish::Cancelled { .. }),
        "the run settles cancelled: {finish:?}"
    );

    // The in-flight call's accounting row: present and honest.
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    assert_eq!(rows.len(), 1, "the cancelled in-flight call still accounts");
    let row = &rows[0];
    assert_eq!(row.purpose, "chat");
    assert_eq!(row.run_id.as_deref(), Some(run_id.as_str()));
    assert_eq!(row.provider, "main", "the dispatch-moment route identity");
    assert_cancelled_unknown_row(row);

    // Restart: a FRESH RunDatabase over the same file still returns the
    // row (the accounting fact is durable, not a live-process artifact).
    let db_path = state.storage().db_path().to_path_buf();
    state.storage().close().await.expect("close storage");
    let reopened = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("reopen after restart");
    let restarted = reopened
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("query after restart");
    reopened.close().await.expect("close reopened");
    assert_eq!(restarted.len(), 1, "the cancelled row survives the restart");
    assert_cancelled_unknown_row(&restarted[0]);
    assert_eq!(restarted[0].model_call_id, row.model_call_id);

    let _ = std::fs::remove_dir_all(&boot_res.home);
    stub.stop().await;
}

/// The in-flight callback double of the worker leg: `complete()` signals
/// its dispatch (the barrier — the callback is demonstrably IN FLIGHT
/// before the deadline settles it) and then parks forever WITHOUT a
/// self-budget. This is exactly the shape the RPC deadline branch exists
/// for: a callback whose port does not settle on its own before the
/// invocation deadline. The ACCOUNTING is still 100% production —
/// `abandoned` delegates to the real
/// [`lingxi_service::workermodel::GatewayWorkerModel`], whose trace port
/// writes the ledger row through the real storage port.
struct ParkedCallbackModel {
    real: Arc<lingxi_service::workermodel::GatewayWorkerModel>,
    dispatched: tokio::sync::watch::Sender<usize>,
}

impl lingxi_service::workerrpc::WorkerModelPort for ParkedCallbackModel {
    fn complete<'a>(
        &'a self,
        _ctx: &'a RunContext,
        _worker: &'a str,
        _invocation: &'a str,
        _cb_id: &'a str,
        _parent_tool_call: &'a lingxi_protocol::ToolCallId,
        _request: &'a lingxi_service::workerrpc::WorkerModelRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        lingxi_service::workerrpc::WorkerModelReply,
                        lingxi_service::workerrpc::WorkerModelRefusal,
                    >,
                > + Send
                + 'a,
        >,
    > {
        let dispatched = self.dispatched.clone();
        Box::pin(async move {
            dispatched.send_modify(|count| *count += 1);
            // In flight, never settles on its own (the invocation
            // deadline is the only thing that ends it).
            std::future::pending().await
        })
    }

    fn abandoned(
        &self,
        fact: lingxi_service::workerrpc::AbandonedWorkerCallback,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        // The production accounting path: the REAL GatewayWorkerModel
        // writes the abandoned row through the REAL ledger trace port.
        lingxi_service::workerrpc::WorkerModelPort::abandoned(self.real.as_ref(), fact)
    }
}

/// Worker leg: the invocation deadline expires while the worker's model
/// callback is IN FLIGHT (the barrier proves the callback was dispatched
/// from the REAL subprocess chain before the deadline settles it). The
/// dropped `complete()` must still leave its accounting row (outcome=
/// cancelled, usage unknown, attempts unknown), joinable to the parent
/// tool call, durable across a restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rr1_f38_worker_deadline_expiry_of_in_flight_callback_leads_a_cancelled_row() {
    use lingxi_service::workerrpc::{
        register_worker_tool, BoundedWorkerModel, WorkerLimits, WorkerModelPort, WorkerRuntime,
        WorkerToolSpec, WORKER_CALLBACK_MAX_OUTPUT_TOKENS, WORKER_MAX_CALLBACKS_PER_CALL,
    };
    let wire_name = "rr1_f38_deadline_worker";
    let stub = StubServer::start(vec![(
        "/v1/chat/completions",
        vec![
            // 1) the chat turn emits the worker TOOL CALL;
            StubResponse::sse(openai_sse_tool_call(
                "call-f38-dl",
                wire_name,
                r#"{"input":"input.txt"}"#,
            )),
            // 2) after the tool settles Unknown, the model gets the final.
            StubResponse::sse(openai_sse_final("settled after the deadline", 44, 9)),
        ],
    )])
    .await;
    let endpoint = stub.endpoint();
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "RR1_T38_WL_MAIN_KEY"}}
            }},
            "aux": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "RR1_T38_WL_AUX_KEY"}}
            }}
        }},
        "models": {{
            "chat": {{"provider": "main", "model": "stub-model", "capabilities": {{"tools": true}}}},
            "summarize": {{"provider": "aux", "model": "summarize-model"}}
        }}"#,
        endpoint = serde_json::to_string(&endpoint).expect("json"),
    );
    let boot_res = boot("rr1-f38-worker-deadline", &plane_json).await;
    let state = &boot_res.state;
    let storage = Arc::clone(state.storage());

    let probe = unique_dir("rr1-f38-wl-ws");
    let ws = probe.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    std::fs::write(ws.join("input.txt"), "granted-content\n").expect("input file");
    let registry = Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new());
    let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
        Arc::new(lingxi_service::inject::SystemClock),
    ));
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let tool_gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(lingxi_service::inject::SystemClock),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    // A SHORT invocation deadline: the worker's callback will still be
    // in flight when it expires.
    let runtime = Arc::new(WorkerRuntime::new(
        WorkerLimits {
            deadline_ms: 1_500,
            ..WorkerLimits::default()
        },
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    ));
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("aux executor");
    let gateway_model = Arc::new(lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            Arc::clone(&storage),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    ));
    let (dispatched_tx, mut dispatched_rx) = tokio::sync::watch::channel(0_usize);
    let model: Arc<dyn WorkerModelPort> = BoundedWorkerModel::new(
        Some(Arc::new(ParkedCallbackModel {
            real: Arc::clone(&gateway_model),
            dispatched: dispatched_tx,
        })),
        WORKER_MAX_CALLBACKS_PER_CALL,
        WORKER_CALLBACK_MAX_OUTPUT_TOKENS,
    );
    let spec = WorkerToolSpec {
        plugin_id: "rr1f38".to_string(),
        op: "probe".to_string(),
        local_name: wire_name.to_string(),
        description: "RR1 F38 deadline probe worker".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
            "additionalProperties": false
        }),
        path_args: vec!["input".to_string()],
        argv: vec![
            env!("CARGO_BIN_EXE_r04_t07_fixture").to_string(),
            "ask_model".to_string(),
        ],
        env: BTreeMap::new(),
        cwd: ws.clone(),
        model,
        allowed_model_purposes: ["summarize"].iter().map(|s| s.to_string()).collect(),
        claimed_file_contract: None,
    };
    let registered = register_worker_tool(
        &registry,
        tool_gateway.as_ref(),
        &runtime,
        &access,
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
        spec,
    )
    .expect("worker registers");
    registry
        .set_availability(
            &registered.target,
            lingxi_kernel::toolcatalog::Availability::Available,
        )
        .expect("worker becomes available");

    let provider = lingxi_adapters::models::provider::GatewayedProvider::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("provider");
    let supervisor = Arc::new(
        RunSupervisor::new(
            Some(Arc::new(provider) as Arc<dyn lingxi_kernel::ports::TurnProviderPort>),
            None,
            lingxi_service::runs::RunDriveLimits::default(),
            lingxi_service::quotas::QuotaManager::new(
                lingxi_service::quotas::QuotaLimits::default(),
            ),
            lingxi_service::cancel::CancelPolicy::default(),
            Some(
                Arc::new(lingxi_service::approval_service::ApprovalService::new(
                    Arc::new(lingxi_service::inject::SystemClock),
                )) as Arc<dyn lingxi_service::approval::ApprovalGate>,
            ),
            None,
        )
        .expect("supervisor")
        .with_tool_gateway(Some(Arc::clone(&tool_gateway))),
    );
    let run_id = format!("run-rr1-f38-wl-{}", std::process::id());
    storage
        .ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
            session_id: "sess_rr1_f38_wl".to_string(),
            agent_id: "lingxi".to_string(),
            owner_user_id: lingxi_service::LOCAL_OWNER_USER_ID.to_string(),
            title: "rr1 f38 worker deadline".to_string(),
            created_at_unix_ms: 1_790_409_600_000_i64,
        }])
        .await
        .expect("session seed");
    let finish = tokio::time::timeout(
        Duration::from_secs(30),
        supervisor.drive_run(
            storage.as_ref(),
            state.events(),
            &KernelPrincipal::LocalUser,
            "sess_rr1_f38_wl",
            "lingxi",
            &run_id,
            "worker callback parked past the invocation deadline",
            1,
            1_790_409_600_000,
            None,
            None,
            DriveAuthorization::user_submission(None, SessionPermissionMode::Operate),
            None,
            "sess_rr1_f38_wl",
        ),
    )
    .await
    .expect("drive settles within the bound")
    .expect("drive ok");

    // The barrier PROOF: the callback was demonstrably dispatched (in
    // flight from the REAL subprocess chain) before the deadline ended it.
    assert!(
        *dispatched_rx.borrow_and_update() >= 1,
        "the worker's callback must demonstrably be in flight"
    );
    assert!(
        matches!(finish, RunFinish::CompletedWithFinal { .. }),
        "the run itself settles (the tool returned the honest Unknown): {finish:?}"
    );

    // The abandoned callback's row.
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    let abandoned = rows
        .iter()
        .find(|row| row.outcome == CallOutcome::Cancelled)
        .expect("the deadline-dropped callback leaves its cancelled row");
    assert_eq!(abandoned.origin, "worker-callback");
    assert_eq!(abandoned.run_id.as_deref(), Some(run_id.as_str()));
    assert_eq!(abandoned.provider, "unreported");
    assert_eq!(abandoned.model, "unreported");
    assert!(
        abandoned
            .parent_tool_call_id
            .as_deref()
            .is_some_and(|id| id.starts_with(&run_id)),
        "the abandoned row still joins its driver-minted parent tool call: {:?}",
        abandoned.parent_tool_call_id
    );
    assert_cancelled_unknown_row(abandoned);
    // The parent chat rows settled normally alongside (tool turn + final).
    assert_eq!(
        rows.iter()
            .filter(|row| row.purpose == "chat" && row.outcome == CallOutcome::Succeeded)
            .count(),
        2,
        "tool turn + final both account normally"
    );

    // Restart durability.
    let db_path = state.storage().db_path().to_path_buf();
    state.storage().close().await.expect("close storage");
    let reopened = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("reopen after restart");
    let restarted = reopened
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("query after restart");
    reopened.close().await.expect("close reopened");
    let restarted_abandoned = restarted
        .iter()
        .find(|row| row.model_call_id == abandoned.model_call_id)
        .expect("the abandoned row survives the restart");
    assert_cancelled_unknown_row(restarted_abandoned);

    let _ = std::fs::remove_dir_all(&boot_res.home);
    stub.stop().await;
}

/// Run-cancel DROP leg: the run's cancellation drops the ENTIRE worker
/// execute future while the callback is IN FLIGHT — only `Drop` code runs
/// (the `CallbackAbandonGuard` detaches the accounting row into the
/// runtime). The row still lands (bounded poll — the detached write races
/// the test), honest and joinable, and the run settles cancelled.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rr1_f38_run_cancel_drop_of_in_flight_callback_leads_a_cancelled_row() {
    use lingxi_service::workerrpc::{
        register_worker_tool, BoundedWorkerModel, WorkerLimits, WorkerModelPort, WorkerRuntime,
        WorkerToolSpec, WORKER_CALLBACK_MAX_OUTPUT_TOKENS, WORKER_MAX_CALLBACKS_PER_CALL,
    };
    let wire_name = "rr1_f38_drop_worker";
    // The main path serves ONLY the tool-call turn — the run never gets a
    // second turn (it settles cancelled inside the tool execution).
    let stub = StubServer::start(vec![(
        "/v1/chat/completions",
        vec![StubResponse::sse(openai_sse_tool_call(
            "call-f38-drop",
            wire_name,
            r#"{"input":"input.txt"}"#,
        ))],
    )])
    .await;
    let endpoint = stub.endpoint();
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "RR1_T38_DROP_MAIN_KEY"}}
            }},
            "aux": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "RR1_T38_DROP_AUX_KEY"}}
            }}
        }},
        "models": {{
            "chat": {{"provider": "main", "model": "stub-model", "capabilities": {{"tools": true}}}},
            "summarize": {{"provider": "aux", "model": "summarize-model"}}
        }}"#,
        endpoint = serde_json::to_string(&endpoint).expect("json"),
    );
    let boot_res = boot("rr1-f38-run-cancel-drop", &plane_json).await;
    let state = &boot_res.state;
    let storage = Arc::clone(state.storage());

    let probe = unique_dir("rr1-f38-drop-ws");
    let ws = probe.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    std::fs::write(ws.join("input.txt"), "granted-content\n").expect("input file");
    let registry = Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new());
    let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
        Arc::new(lingxi_service::inject::SystemClock),
    ));
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let tool_gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(lingxi_service::inject::SystemClock),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    // A LONG invocation deadline: the run's CANCELLATION must be what
    // drops the in-flight callback, never the deadline.
    let runtime = Arc::new(WorkerRuntime::new(
        WorkerLimits {
            deadline_ms: 30_000,
            ..WorkerLimits::default()
        },
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    ));
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("aux executor");
    let gateway_model = Arc::new(lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            Arc::clone(&storage),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    ));
    let (dispatched_tx, mut dispatched_rx) = tokio::sync::watch::channel(0_usize);
    let model: Arc<dyn WorkerModelPort> = BoundedWorkerModel::new(
        Some(Arc::new(ParkedCallbackModel {
            real: Arc::clone(&gateway_model),
            dispatched: dispatched_tx,
        })),
        WORKER_MAX_CALLBACKS_PER_CALL,
        WORKER_CALLBACK_MAX_OUTPUT_TOKENS,
    );
    let spec = WorkerToolSpec {
        plugin_id: "rr1f38drop".to_string(),
        op: "probe".to_string(),
        local_name: wire_name.to_string(),
        description: "RR1 F38 run-cancel drop probe worker".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
            "additionalProperties": false
        }),
        path_args: vec!["input".to_string()],
        argv: vec![
            env!("CARGO_BIN_EXE_r04_t07_fixture").to_string(),
            "ask_model".to_string(),
        ],
        env: BTreeMap::new(),
        cwd: ws.clone(),
        model,
        allowed_model_purposes: ["summarize"].iter().map(|s| s.to_string()).collect(),
        claimed_file_contract: None,
    };
    let registered = register_worker_tool(
        &registry,
        tool_gateway.as_ref(),
        &runtime,
        &access,
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
        spec,
    )
    .expect("worker registers");
    registry
        .set_availability(
            &registered.target,
            lingxi_kernel::toolcatalog::Availability::Available,
        )
        .expect("worker becomes available");

    let provider = lingxi_adapters::models::provider::GatewayedProvider::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("provider");
    let supervisor = Arc::new(
        RunSupervisor::new(
            Some(Arc::new(provider) as Arc<dyn lingxi_kernel::ports::TurnProviderPort>),
            None,
            lingxi_service::runs::RunDriveLimits::default(),
            lingxi_service::quotas::QuotaManager::new(
                lingxi_service::quotas::QuotaLimits::default(),
            ),
            lingxi_service::cancel::CancelPolicy::default(),
            Some(
                Arc::new(lingxi_service::approval_service::ApprovalService::new(
                    Arc::new(lingxi_service::inject::SystemClock),
                )) as Arc<dyn lingxi_service::approval::ApprovalGate>,
            ),
            None,
        )
        .expect("supervisor")
        .with_tool_gateway(Some(Arc::clone(&tool_gateway))),
    );
    let run_id = format!("run-rr1-f38-drop-{}", std::process::id());
    storage
        .ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
            session_id: "sess_rr1_f38_drop".to_string(),
            agent_id: "lingxi".to_string(),
            owner_user_id: lingxi_service::LOCAL_OWNER_USER_ID.to_string(),
            title: "rr1 f38 run cancel drop".to_string(),
            created_at_unix_ms: 1_790_409_600_000_i64,
        }])
        .await
        .expect("session seed");
    let drive_supervisor = Arc::clone(&supervisor);
    let drive_run_id = run_id.clone();
    let events = state.events().clone();
    let driven = tokio::spawn(async move {
        drive_supervisor
            .drive_run(
                storage.as_ref(),
                &events,
                &KernelPrincipal::LocalUser,
                "sess_rr1_f38_drop",
                "lingxi",
                &drive_run_id,
                "callback in flight when the run is cancelled",
                1,
                1_790_409_600_000,
                None,
                None,
                DriveAuthorization::user_submission(None, SessionPermissionMode::Operate),
                None,
                "sess_rr1_f38_drop",
            )
            .await
    });

    // The barrier: the callback is demonstrably in flight, then the run's
    // cancellation (not a deadline) drops the whole execute future.
    let barrier_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if *dispatched_rx.borrow_and_update() >= 1 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < barrier_deadline,
            "the callback must demonstrably be in flight (dispatched={:?})",
            dispatched_rx.borrow()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        matches!(
            supervisor.cancel_run(&run_id, "rr1 f38 run-cancel drop leg"),
            lingxi_service::cancel::FireOutcome::Fired
        ),
        "the cancellation fires while the callback is in flight"
    );
    let finish = tokio::time::timeout(Duration::from_secs(30), driven)
        .await
        .expect("drive settles")
        .expect("drive task alive")
        .expect("drive ok");
    assert!(
        matches!(finish, RunFinish::Cancelled { .. }),
        "the run settles cancelled: {finish:?}"
    );

    // The abandoned row lands through the DETACHED write (bounded poll —
    // the guard's spawn races this assertion).
    let mut abandoned_row = None;
    let poll_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while abandoned_row.is_none() {
        let rows = usage_rows(state, ModelUsageQuery::default()).await;
        abandoned_row = rows
            .into_iter()
            .find(|row| row.outcome == CallOutcome::Cancelled && row.origin == "worker-callback");
        if abandoned_row.is_some() || tokio::time::Instant::now() > poll_deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let abandoned = abandoned_row.expect("the drop-detached abandoned row lands within the bound");
    assert_eq!(abandoned.run_id.as_deref(), Some(run_id.as_str()));
    assert_eq!(abandoned.provider, "unreported");
    assert!(
        abandoned
            .parent_tool_call_id
            .as_deref()
            .is_some_and(|id| id.starts_with(&run_id)),
        "the abandoned row joins its driver-minted parent tool call: {:?}",
        abandoned.parent_tool_call_id
    );
    assert_cancelled_unknown_row(&abandoned);
    // The tool-turn chat row settled normally BEFORE the cancellation.
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    assert_eq!(
        rows.iter()
            .filter(|row| row.purpose == "chat" && row.outcome == CallOutcome::Succeeded)
            .count(),
        1,
        "the tool turn's chat row accounts normally"
    );

    // Restart durability.
    let db_path = state.storage().db_path().to_path_buf();
    state.storage().close().await.expect("close storage");
    let reopened = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("reopen after restart");
    let restarted = reopened
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("query after restart");
    reopened.close().await.expect("close reopened");
    let restarted_abandoned = restarted
        .iter()
        .find(|row| row.model_call_id == abandoned.model_call_id)
        .expect("the drop-detached row survives the restart");
    assert_cancelled_unknown_row(restarted_abandoned);

    let _ = std::fs::remove_dir_all(&boot_res.home);
    stub.stop().await;
}

// ── R05 RR1 F38 non-blocking legs (F21 self-check coverage): the two
// settlement shapes whose usage facts ride the shared failure closure but
// were only pinned indirectly by the empty-body leg ─────────────────────────

/// An unexpected TOOL-RESPONSE on an auxiliary call (the slot declared NO
/// tools) is a loud failure — and the provider's usage object that came
/// WITH the tool turn still accounts on the row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f21_unexpected_tool_response_failure_keeps_the_reported_usage() {
    let stub = StubServer::start(vec![(
        "/v1/chat/completions",
        // A tool-call SSE turn WITH usage (30/11) — the join leg's shape.
        vec![StubResponse::sse(openai_sse_tool_call(
            "call-aux-unexpected",
            "some_tool",
            r#"{"x":1}"#,
        ))],
    )])
    .await;
    let boot_res = boot(
        "rr1-f21-toolresp-usage",
        &plane_with_aux(&stub.endpoint(), "RR1_T07_AUX_KEY"),
    )
    .await;
    let state = &boot_res.state;
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .unwrap();
    let model = lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            state.storage().clone(),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    );
    let ctx = run_ctx("sess_rr1_toolresp", "run-rr1-toolresp");
    use lingxi_service::workerrpc::WorkerModelPort as _;
    let reply = model
        .complete(
            &ctx,
            "plug",
            "rr1-toolresp-inv",
            "cb-1",
            &lingxi_protocol::ToolCallId::new("run-rr1-toolresp-tc0001"),
            &lingxi_service::workerrpc::WorkerModelRequest {
                prompt: "summarize this".into(),
                purpose: "summarize".into(),
                max_output_tokens: 64,
                deadline_unix_ms: None,
            },
        )
        .await;
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    let requests = stub.hits_for("/v1/chat/completions");
    teardown(boot_res).await;
    stub.stop().await;
    assert_eq!(requests, 1, "the physical request really happened");
    assert!(
        reply.is_err(),
        "a tool-requesting auxiliary turn is refused"
    );
    assert_eq!(rows.len(), 1, "the refused-but-sent call still accounts");
    assert_eq!(rows[0].transport_attempts, Some(1));
    assert_eq!(rows[0].outcome, CallOutcome::Failed);
    match &rows[0].usage {
        Some(usage) => {
            assert_eq!(usage.input_tokens, Some(30));
            assert_eq!(usage.output_tokens, Some(11));
        }
        None => panic!("the usage that arrived WITH the tool turn survives the refusal"),
    }
}

/// A PARTIAL usage fact (the input half of an interrupted account) on a
/// failed settlement: the known half is kept, the missing half stays
/// absent — never zero, never dropped with the failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f21_partial_usage_survives_a_failed_aux_settlement() {
    // A well-formed SSE final whose text is EMPTY and whose usage object
    // carries ONLY the input half: the call fails honestly (no fabricated
    // text) and the PARTIAL usage fact (missing output_tokens, by name)
    // rides the failure row.
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-rr1", "model": "stub-model",
            "choices": [{
                "index": 0, "finish_reason": null,
                "delta": {"role": "assistant", "content": "   "}
            }]
        }),
        serde_json::json!({
            "id": "chatcmpl-rr1",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
        serde_json::json!({
            "id": "chatcmpl-rr1", "choices": [],
            "usage": {"prompt_tokens": 9}
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    let stub = StubServer::start(vec![(
        "/v1/chat/completions",
        vec![StubResponse::sse(body)],
    )])
    .await;
    let boot_res = boot(
        "rr1-f21-partial-usage",
        &plane_with_aux(&stub.endpoint(), "RR1_T07_AUX_KEY"),
    )
    .await;
    let state = &boot_res.state;
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .unwrap();
    let model = lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            state.storage().clone(),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    );
    let ctx = run_ctx("sess_rr1_partial", "run-rr1-partial");
    use lingxi_service::workerrpc::WorkerModelPort as _;
    let reply = model
        .complete(
            &ctx,
            "plug",
            "rr1-partial-inv",
            "cb-1",
            &lingxi_protocol::ToolCallId::new("run-rr1-partial-tc0001"),
            &lingxi_service::workerrpc::WorkerModelRequest {
                prompt: "summarize this".into(),
                purpose: "summarize".into(),
                max_output_tokens: 64,
                deadline_unix_ms: None,
            },
        )
        .await;
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    teardown(boot_res).await;
    stub.stop().await;
    assert!(
        reply.is_err(),
        "the empty answer is still an explicit failure"
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, CallOutcome::Failed);
    assert_eq!(rows[0].transport_attempts, Some(1));
    match &rows[0].usage {
        Some(usage) => {
            assert_eq!(usage.input_tokens, Some(9), "the known half is kept");
            assert_eq!(usage.output_tokens, None, "the missing half stays absent");
            assert_eq!(
                usage.provenance,
                lingxi_kernel::usage::UsageProvenance::Partial {
                    missing: vec!["output_tokens"]
                },
                "the partial fact names its missing half"
            );
        }
        None => panic!("the partial usage fact survives the failed settlement"),
    }
}

// ── R05 RR1 F39: permanent pins for the fence-Stale+cancelled arm ─────────────
//
// F38 landed `persist_model_call_cancelled_in_flight` at TWO call sites
// (the select race arm and the fence-Stale+cancelled arm) but only the
// race arm carried a permanent leg — deleting the fence arm's row write
// had no test consequence (the F39 coverage gap). These legs pin the
// FENCE arm: a turn that HAS settled (real usage, real physical attempt
// count, resolved route on the result) whose state write is fenced by a
// cancellation observed AFTER the select loop exited.
//
// - CancelledBeforeWrite (main leg): the cancellation lands in the exit
//   arm's drain/persist await window. Determinism comes from the
//   storage-side gate, never from timing: the `model_call_started`
//   write parks first, so the scripted turn resolves while the driver
//   is OUTSIDE the select loop; when that write releases, the biased
//   select polls the (already resolved) exit arm BEFORE the streaming
//   tick arm — the queued delta survives for the drain — and the
//   drain's `record_run_events` parks open, holding the driver inside
//   the exact window where `fence_verdict` (not the biased select)
//   classifies the cancellation.
//
// - FenceMismatch+cancelled (variant leg): the turn carries a WRONG
//   fence (the raced-adapter delivery shape), so the verdict is a
//   mismatch regardless of timing; the stale-result AUDIT write happens
//   BEFORE the `is_cancelled` check — a cancellation landing inside
//   that parked audit write converges on the SAME fence arm.
//
// The built-in differentiator from the race arm (which also writes an
// outcome=Cancelled row but NO audit): the fence arm writes the
// `stale_result` audit row first. The race arm keeps its own leg
// (`rr1_f38_driver…`); each arm is separately pinned.

use std::sync::atomic::{AtomicBool, Ordering};

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    CommittedOutcome, InvocationIntent, InvocationJournalEntry, InvocationPhase, InvocationReceipt,
    KeyEvent, ModelTurnDelta, ProviderDescriptor, ProviderTurn, ProviderTurnResult, ResultFence,
    RunOutcome, RunRecord, StaleResultFact, StorageError, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{
    ContentBlock, EventPayload, KnownEventPayload, ModelCallId, NormalizedMessage,
    RunId as ProtocolRunId, RunStatus, ToolCallId,
};

/// The real usage the scripted turns report (values no other leg uses,
/// so a mix-up with another row is impossible).
const F39_USAGE_INPUT_TOKENS: u64 = 411;
const F39_USAGE_OUTPUT_TOKENS: u64 = 239;

/// One one-shot storage-write park: the first matching write claims the
/// gate, signals engagement and waits for the test's release. The write
/// still DELEGATES to the real `RunDatabase` afterwards — the gate only
/// widens an await window the test can observe; no read is ever faked.
#[derive(Clone)]
struct ParkGate {
    consumed: Arc<AtomicBool>,
    engaged: Arc<AtomicBool>,
    release: Arc<tokio::sync::watch::Sender<bool>>,
}

impl ParkGate {
    fn new() -> Self {
        let (release, _) = tokio::sync::watch::channel(false);
        Self {
            consumed: Arc::new(AtomicBool::new(false)),
            engaged: Arc::new(AtomicBool::new(false)),
            release: Arc::new(release),
        }
    }

    /// Claims the gate for one write; `false` when a previous write
    /// already consumed it (exactly one park per gate).
    fn claim(&self) -> bool {
        !self.consumed.swap(true, Ordering::SeqCst)
    }

    fn mark_engaged(&self) {
        self.engaged.store(true, Ordering::SeqCst);
    }

    fn is_engaged(&self) -> bool {
        self.engaged.load(Ordering::SeqCst)
    }

    async fn wait_release(&self) {
        let mut rx = self.release.subscribe();
        if *rx.borrow() {
            return;
        }
        let _ = rx.changed().await;
    }

    fn release(&self) {
        let _ = self.release.send(true);
    }
}

/// A delegating [`lingxi_kernel::ports::StoragePort`] wrapper: every one
/// of the 16 methods forwards to the REAL `RunDatabase`, so every ledger
/// row the legs assert lands in the real store (restart-queryable) — the
/// wrapper never fabricates a read. Exactly three writes can be parked
/// open, one gate each (one-shot):
/// - the `model_call_started` `record_run_events` (holds the driver
///   BEFORE the biased select);
/// - the exit-arm drain persist of the delta batch (THE fence-arm
///   window of the CancelledBeforeWrite verdict);
/// - the fence's `record_stale_result` audit (written BEFORE the
///   `is_cancelled` re-check — the FenceMismatch variant's window).
struct GatedStorage {
    inner: Arc<lingxi_adapters::storage::RunDatabase>,
    park_started_write: Option<ParkGate>,
    park_drain_delta_write: Option<ParkGate>,
    park_stale_audit: Option<ParkGate>,
}

impl GatedStorage {
    fn park_for_write(&self, events: &[KeyEvent]) -> Option<ParkGate> {
        let started = events.iter().any(|event| {
            matches!(
                &event.payload,
                EventPayload::Known(KnownEventPayload::ModelCallStarted(_))
            )
        });
        let delta = events.iter().any(|event| {
            matches!(
                &event.payload,
                EventPayload::Known(
                    KnownEventPayload::ModelCallDelta(_)
                        | KnownEventPayload::AssistantSegmentStart(_)
                        | KnownEventPayload::AssistantSegmentDelta(_)
                        | KnownEventPayload::AssistantSegmentEnd(_)
                )
            )
        });
        let gate = if started {
            self.park_started_write.as_ref()
        } else if delta {
            self.park_drain_delta_write.as_ref()
        } else {
            None
        };
        match gate {
            Some(gate) if gate.claim() => Some(gate.clone()),
            _ => None,
        }
    }
}

impl lingxi_kernel::ports::StoragePort for GatedStorage {
    fn record_run_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        self.inner.record_run_started(ctx, now_unix_ms)
    }

    fn commit_run_outcome(
        &self,
        ctx: &RunContext,
        outcome: RunOutcome,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        self.inner.commit_run_outcome(ctx, outcome, now_unix_ms)
    }

    fn load_run(
        &self,
        run_id: &ProtocolRunId,
    ) -> impl std::future::Future<Output = Result<Option<RunRecord>, StorageError>> + Send {
        self.inner.load_run(run_id)
    }

    fn record_run_events(
        &self,
        ctx: &RunContext,
        events: Vec<KeyEvent>,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        let park = self.park_for_write(&events);
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        async move {
            if let Some(gate) = park {
                gate.mark_engaged();
                gate.wait_release().await;
            }
            inner.record_run_events(&ctx, events, now_unix_ms).await
        }
    }

    fn record_stale_result(
        &self,
        ctx: &RunContext,
        refused: StaleResultFact,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let park = match self.park_stale_audit.as_ref() {
            Some(gate) if gate.claim() => Some(gate.clone()),
            _ => None,
        };
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        async move {
            if let Some(gate) = park {
                gate.mark_engaged();
                gate.wait_release().await;
            }
            inner.record_stale_result(&ctx, refused, now_unix_ms).await
        }
    }

    fn record_attempt_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        self.inner.record_attempt_started(ctx, now_unix_ms)
    }

    fn record_run_state_change(
        &self,
        ctx: &RunContext,
        from: RunStatus,
        to: RunStatus,
        reason: Option<String>,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        self.inner
            .record_run_state_change(ctx, from, to, reason, now_unix_ms)
    }

    fn record_invocation_intent(
        &self,
        ctx: &RunContext,
        intent: InvocationIntent,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        self.inner
            .record_invocation_intent(ctx, intent, now_unix_ms)
    }

    fn advance_invocation(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        to: InvocationPhase,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        self.inner
            .advance_invocation(ctx, journal_id, to, now_unix_ms)
    }

    fn record_invocation_receipt(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        receipt: InvocationReceipt,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        self.inner
            .record_invocation_receipt(ctx, journal_id, receipt, now_unix_ms)
    }

    fn record_invocation_unknown(
        &self,
        journal_id: &ToolCallId,
        detail: String,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        self.inner
            .record_invocation_unknown(journal_id, detail, now_unix_ms)
    }

    fn load_invocation_journal(
        &self,
        run_id: &ProtocolRunId,
    ) -> impl std::future::Future<Output = Result<Vec<InvocationJournalEntry>, StorageError>> + Send
    {
        self.inner.load_invocation_journal(run_id)
    }

    fn record_run_lineage(
        &self,
        ctx: &RunContext,
        lineage: lingxi_kernel::subagent::RunLineage,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        self.inner.record_run_lineage(ctx, lineage, now_unix_ms)
    }

    fn load_run_lineage(
        &self,
        run_id: &ProtocolRunId,
    ) -> impl std::future::Future<
        Output = Result<Option<lingxi_kernel::subagent::RunLineage>, StorageError>,
    > + Send {
        self.inner.load_run_lineage(run_id)
    }

    fn record_model_call_usage(
        &self,
        record: ModelCallUsageRecord,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        self.inner.record_model_call_usage(record, now_unix_ms)
    }

    fn query_model_call_usage(
        &self,
        query: ModelUsageQuery,
    ) -> impl std::future::Future<Output = Result<Vec<ModelCallUsageRecord>, StorageError>> + Send
    {
        self.inner.query_model_call_usage(query)
    }
}

/// The scripted turn of the F39 legs: streams ONE identifiable text
/// delta (so the exit arm's drain has a real batch to persist — a text
/// delta emits its segment events immediately), then resolves carrying
/// REAL settle facts — a known usage report, one physical transport
/// attempt, an explicit resolved serving route (a DIFFERENT identity
/// than the dispatch descriptor, so the row proves the result's
/// `served_by` won) — and optionally a WRONG fence (the raced-adapter
/// delivery shape of the FenceMismatch variant). Signals the test the
/// moment the turn has resolved.
struct FenceArmProvider {
    delta_text: Option<&'static str>,
    fence_override: Option<ResultFence>,
    resolved: Arc<AtomicBool>,
}

impl FenceArmProvider {
    /// The dispatch-moment descriptor (the supervisor's static view).
    fn dispatch_descriptor() -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "rr1f39.dispatch".to_string(),
            model: "rr1f39-dispatch-model".to_string(),
            operation: "chat".to_string(),
        }
    }

    /// The route the RESULT says served the turn — must beat the
    /// dispatch descriptor in the persisted row (the fence arm carries
    /// resolved facts, and `runs.rs` prefers `served_by`).
    fn resolved_route() -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "rr1f39.resolved".to_string(),
            model: "rr1f39-served-model".to_string(),
            operation: "chat".to_string(),
        }
    }
}

impl TurnProviderPort for FenceArmProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        Self::dispatch_descriptor()
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        deltas: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let ctx_at_issue = ctx.clone();
        let delta_text = self.delta_text;
        let fence_override = self.fence_override.clone();
        let resolved = Arc::clone(&self.resolved);
        Box::pin(async move {
            if let Some(text) = delta_text {
                deltas
                    .emit(ModelTurnDelta::Text(text.to_string()))
                    .await
                    .expect("the driver's live-delta sink accepts the delta");
            }
            resolved.store(true, Ordering::SeqCst);
            let turn = ProviderTurn::Final {
                message: NormalizedMessage {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "f39 final that the fence refuses to land".to_string(),
                    }],
                    model_call_id: None,
                },
            };
            let result = match fence_override {
                // The CLAIMED fence wins — the late/raced delivery shape
                // the driver must fence.
                Some(fence) => ProviderTurnResult {
                    fence,
                    turn,
                    usage: None,
                    usage_report: lingxi_kernel::usage::ReportedUsage::Unknown,
                    served_protocol: None,
                    transport_attempts: 1,
                    served_by: None,
                },
                None => ProviderTurnResult::of_ctx(&ctx_at_issue, turn),
            };
            let result = result
                .with_usage_report(lingxi_kernel::usage::ReportedUsage::Known(
                    lingxi_kernel::usage::ModelCallUsage::reported(
                        F39_USAGE_INPUT_TOKENS,
                        F39_USAGE_OUTPUT_TOKENS,
                    ),
                ))
                .with_transport_attempts(1)
                .with_served_by(Self::resolved_route());
            let mut result = result;
            result.served_protocol = Some("rr1-f39-fence-protocol".to_string());
            result
        })
    }
}

/// Boots the real service state (real `RunDatabase` + real event
/// service) and seeds the session — the same construction shape as the
/// F38 driver leg. The plane's endpoint is NEVER contacted: these legs'
/// supervisor consumes the scripted provider double, and the synthetic
/// apiKey plane exists only because `boot` resolves one.
async fn f39_boot_and_seed(tag: &str, session: &'static str) -> PlaneBoot {
    let boot_res = boot(
        tag,
        &plane_with_aux("http://127.0.0.1:9", "RR1_T39_NEVER_USED_KEY"),
    )
    .await;
    boot_res
        .state
        .storage()
        .ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
            session_id: session.to_string(),
            agent_id: "lingxi".to_string(),
            owner_user_id: lingxi_service::LOCAL_OWNER_USER_ID.to_string(),
            title: format!("rr1 f39 {tag}"),
            created_at_unix_ms: 1_790_409_600_000_i64,
        }])
        .await
        .expect("session seed");
    boot_res
}

/// Spawns the REAL driver (real `RunSupervisor`, real quotas, real
/// cancellation tree) over the gated delegating storage.
async fn f39_spawn_drive(
    state: &ServiceState,
    provider: Arc<FenceArmProvider>,
    gated: GatedStorage,
    session: &'static str,
    run_id: String,
) -> (
    Arc<RunSupervisor>,
    tokio::task::JoinHandle<Result<RunFinish, lingxi_service::runs::DriveError>>,
) {
    let supervisor = Arc::new(
        RunSupervisor::new(
            Some(provider as Arc<dyn TurnProviderPort>),
            None,
            lingxi_service::runs::RunDriveLimits::default(),
            lingxi_service::quotas::QuotaManager::new(
                lingxi_service::quotas::QuotaLimits::default(),
            ),
            lingxi_service::cancel::CancelPolicy::default(),
            None,
            None,
        )
        .expect("supervisor"),
    );
    let events = state.events().clone();
    let drive_supervisor = Arc::clone(&supervisor);
    let driven = tokio::spawn(async move {
        drive_supervisor
            .drive_run(
                &gated,
                &events,
                &KernelPrincipal::LocalUser,
                session,
                "lingxi",
                &run_id,
                "rr1 f39 fence-arm leg",
                1,
                1_790_409_600_000,
                None,
                None,
                DriveAuthorization::user_submission(None, SessionPermissionMode::Operate),
                None,
                session,
            )
            .await
    });
    (supervisor, driven)
}

/// Bounded wait until a gate engages (the parked write arrived).
async fn f39_wait_gate(gate: &ParkGate, bound_ms: u64, what: &str) {
    let deadline = std::time::Instant::now() + Duration::from_millis(bound_ms);
    while !gate.is_engaged() {
        assert!(
            std::time::Instant::now() < deadline,
            "the {what} never engaged (the driver never reached the parked write)"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Bounded wait until a flag flips.
async fn f39_wait_flag(flag: &AtomicBool, bound_ms: u64, what: &str) {
    let deadline = std::time::Instant::now() + Duration::from_millis(bound_ms);
    while !flag.load(Ordering::SeqCst) {
        assert!(
            std::time::Instant::now() < deadline,
            "{what} never happened"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn f39_query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("f39 query")
}

/// (audit row count, latest reason, latest refused_event_types) of one run.
async fn f39_audit_of(state: &ServiceState, run_id: &str) -> (i64, Option<String>, String) {
    let count = f39_query_text(
        state,
        "SELECT COUNT(*) FROM stale_result_audit WHERE run_id = ?1",
        run_id,
    )
    .await
    .and_then(|v| v.parse::<i64>().ok())
    .unwrap_or(0);
    let reason = f39_query_text(
        state,
        "SELECT reason FROM stale_result_audit WHERE run_id = ?1 ORDER BY audit_id DESC LIMIT 1",
        run_id,
    )
    .await;
    let refused = f39_query_text(
        state,
        "SELECT refused_event_types FROM stale_result_audit WHERE run_id = ?1 ORDER BY \
         audit_id DESC LIMIT 1",
        run_id,
    )
    .await
    .unwrap_or_default();
    (count, reason, refused)
}

/// Key events of one run whose type is `model_call_completed` — must
/// stay ZERO for every cancelled call (the A09/C16 discipline: the
/// ledger row is the accounting fact, never a fabricated completion).
async fn f39_completed_event_count(state: &ServiceState, run_id: &str) -> i64 {
    f39_query_text(
        state,
        "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = \
         'model_call_completed'",
        run_id,
    )
    .await
    .and_then(|v| v.parse::<i64>().ok())
    .unwrap_or(-1)
}

/// The fence-arm row shape: outcome=cancelled while carrying the turn's
/// REAL facts — the reported usage (never unknown, never a number the
/// race arm would fabricate), the observed attempt count, the RESOLVED
/// serving route (not the dispatch-moment descriptor).
fn assert_f39_fence_arm_row(row: &ModelCallUsageRecord) {
    assert_eq!(row.outcome, CallOutcome::Cancelled);
    match &row.usage {
        Some(usage) => {
            assert_eq!(usage.input_tokens, Some(F39_USAGE_INPUT_TOKENS));
            assert_eq!(usage.output_tokens, Some(F39_USAGE_OUTPUT_TOKENS));
            assert_eq!(
                usage.provenance,
                lingxi_kernel::usage::UsageProvenance::Reported,
                "the turn's real reported usage survives the fence"
            );
        }
        None => panic!(
            "the fence arm carries the settled turn's REAL usage — unknown here means the \
             leg landed on the race arm instead (a harness wiring bug, never a weakening)"
        ),
    }
    assert!(row.invalid_detail.is_none());
    assert_eq!(
        row.transport_attempts,
        Some(1),
        "the turn's observed physical attempt count — not None (unknown), not 0 (not-sent)"
    );
    assert_eq!(
        row.provider, "rr1f39.resolved",
        "the RESOLVED serving route wins over the dispatch descriptor"
    );
    assert_eq!(row.model, "rr1f39-served-model");
    assert_eq!(row.protocol, "rr1-f39-fence-protocol");
    assert!(row.started_at_unix_ms.is_some() && row.settled_at_unix_ms.is_some());
}

/// F39 main leg (CancelledBeforeWrite): the turn HAS settled with real
/// facts; the cancellation is observed inside the exit arm's
/// drain/persist window — AFTER the select loop exited, so
/// `fence_verdict` (not the biased select) classifies it. The refused
/// content never lands, the accounting row keeps the turn's real
/// usage/attempts/resolved identity, the stale_result audit names
/// `cancelled_before_write` (the race arm writes no audit — the built-in
/// differentiator), no `model_call_completed` event exists, and the row
/// survives a storage restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rr1_f39_fence_cancelled_before_write_keeps_the_turns_real_usage_row() {
    let session = "sess_rr1_f39_cbw";
    let boot_res = f39_boot_and_seed("rr1-f39-cbw", session).await;
    let state = &boot_res.state;
    let storage = Arc::clone(state.storage());
    let run_id = format!("run-rr1-f39-cbw-{}", std::process::id());

    let resolved_flag = Arc::new(AtomicBool::new(false));
    let provider = Arc::new(FenceArmProvider {
        delta_text: Some("RR1F39 drain window delta"),
        fence_override: None,
        resolved: Arc::clone(&resolved_flag),
    });

    let started_gate = ParkGate::new();
    let drain_gate = ParkGate::new();
    let gated = GatedStorage {
        inner: Arc::clone(&storage),
        park_started_write: Some(started_gate.clone()),
        park_drain_delta_write: Some(drain_gate.clone()),
        park_stale_audit: None,
    };
    let (supervisor, driven) =
        f39_spawn_drive(state, provider, gated, session, run_id.clone()).await;

    // Window 1: the driver parks on the `model_call_started` write —
    // OUTSIDE the select loop. The scripted turn resolves (one delta
    // queued in the live channel, the turn returned) while the driver
    // waits here.
    f39_wait_gate(&started_gate, 10_000, "model_call_started write").await;
    f39_wait_flag(&resolved_flag, 10_000, "scripted turn resolution").await;

    // Releasing the start write: the driver re-enters the biased select
    // with the child ALREADY resolved — the exit arm is polled before
    // the streaming tick arm, deterministically. The drain persists the
    // queued delta batch and parks in window 2.
    started_gate.release();
    f39_wait_gate(&drain_gate, 10_000, "exit-arm drain persist").await;

    // THE FENCE WINDOW: the cancellation fires while the driver is past
    // the select loop, inside the drain persist await.
    assert!(
        matches!(
            supervisor.cancel_run(&run_id, "rr1 f39 cancelled-before-write"),
            lingxi_service::cancel::FireOutcome::Fired
        ),
        "the cancellation fires inside the drain/persist window"
    );
    drain_gate.release();

    let finish = tokio::time::timeout(Duration::from_secs(30), driven)
        .await
        .expect("drive settles")
        .expect("drive task alive")
        .expect("drive ok");
    assert!(
        matches!(finish, RunFinish::Cancelled { .. }),
        "the run settles cancelled: {finish:?}"
    );

    // The accounting row: exactly the fenced call's, with REAL facts.
    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    assert_eq!(rows.len(), 1, "exactly the fenced call's row");
    let row = &rows[0];
    assert_eq!(row.purpose, "chat");
    assert_eq!(row.run_id.as_deref(), Some(run_id.as_str()));
    assert_f39_fence_arm_row(row);

    // The fence arm's audit — the race arm writes NONE of these.
    let (audit_count, audit_reason, audit_refused) = f39_audit_of(state, &run_id).await;
    assert_eq!(audit_count, 1, "one stale_result audit row");
    assert_eq!(
        audit_reason.as_deref(),
        Some("cancelled_before_write"),
        "the fence verdict classifies the cancellation (the race arm writes no audit row)"
    );
    assert!(
        audit_refused.contains("model_call_result"),
        "the audit names the refused model_call_result: {audit_refused}"
    );

    // No fabricated completion (A09/C16).
    assert_eq!(
        f39_completed_event_count(state, &run_id).await,
        0,
        "a cancelled call never closes with model_call_completed"
    );

    // Restart durability: a fresh RunDatabase over the same file still
    // returns the row (the accounting fact is durable, not a
    // live-process artifact).
    let db_path = state.storage().db_path().to_path_buf();
    state.storage().close().await.expect("close storage");
    let reopened = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("reopen after restart");
    let restarted = reopened
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("query after restart");
    reopened.close().await.expect("close reopened");
    assert_eq!(restarted.len(), 1, "the fence-arm row survives the restart");
    assert_f39_fence_arm_row(&restarted[0]);
    assert_eq!(restarted[0].model_call_id, row.model_call_id);

    let _ = std::fs::remove_dir_all(&boot_res.home);
}

/// F39 variant leg (FenceMismatch+cancelled): the turn carries a WRONG
/// fence, so `fence_verdict` is a mismatch regardless of timing; the
/// stale-result audit write happens BEFORE the `is_cancelled` check — a
/// cancellation landing inside that parked audit write converges on the
/// SAME fence arm. The row keeps the turn's real usage; the audit names
/// `fence_mismatch`; no completion event; durable across a restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rr1_f39_fence_mismatch_cancelled_during_audit_keeps_the_turns_real_usage_row() {
    let session = "sess_rr1_f39_mism";
    let boot_res = f39_boot_and_seed("rr1-f39-mism", session).await;
    let state = &boot_res.state;
    let storage = Arc::clone(state.storage());
    let run_id = format!("run-rr1-f39-mism-{}", std::process::id());

    let foreign_run = ProtocolRunId::new("run-rr1-f39b-foreign".to_string());
    let foreign_attempt = lingxi_kernel::attempt_id(&foreign_run, 1);
    let resolved_flag = Arc::new(AtomicBool::new(false));
    let provider = Arc::new(FenceArmProvider {
        delta_text: None,
        fence_override: Some(ResultFence {
            run_id: foreign_run,
            attempt: foreign_attempt,
            generation: 1,
        }),
        resolved: Arc::clone(&resolved_flag),
    });

    let audit_gate = ParkGate::new();
    let gated = GatedStorage {
        inner: Arc::clone(&storage),
        park_started_write: None,
        park_drain_delta_write: None,
        park_stale_audit: Some(audit_gate.clone()),
    };
    let (supervisor, driven) =
        f39_spawn_drive(state, provider, gated, session, run_id.clone()).await;

    // The driver is past `fence_verdict` (a mismatch by construction)
    // and parked INSIDE the audit write — which runs BEFORE the
    // `is_cancelled` re-check of the fence arm.
    f39_wait_gate(&audit_gate, 10_000, "fence audit write").await;
    f39_wait_flag(&resolved_flag, 10_000, "scripted turn resolution").await;

    assert!(
        matches!(
            supervisor.cancel_run(&run_id, "rr1 f39 fence-mismatch"),
            lingxi_service::cancel::FireOutcome::Fired
        ),
        "the cancellation fires inside the parked audit write"
    );
    audit_gate.release();

    let finish = tokio::time::timeout(Duration::from_secs(30), driven)
        .await
        .expect("drive settles")
        .expect("drive task alive")
        .expect("drive ok");
    assert!(
        matches!(finish, RunFinish::Cancelled { .. }),
        "the run settles cancelled (the cancelled fence arm, not the loud mismatch failure): \
         {finish:?}"
    );

    let rows = usage_rows(state, ModelUsageQuery::default()).await;
    assert_eq!(rows.len(), 1, "exactly the fenced call's row");
    let row = &rows[0];
    assert_eq!(row.purpose, "chat");
    assert_eq!(row.run_id.as_deref(), Some(run_id.as_str()));
    assert_f39_fence_arm_row(row);

    // The audit carries the CLAIMED fence identity (the foreign run),
    // not the driving run — it proves WHAT arrived late, never that the
    // claim was legitimate (the R03-T04 audit contract).
    let (audit_count, audit_reason, audit_refused) =
        f39_audit_of(state, "run-rr1-f39b-foreign").await;
    assert_eq!(audit_count, 1, "one stale_result audit row");
    assert_eq!(
        audit_reason.as_deref(),
        Some("fence_mismatch"),
        "the wrong-fence verdict is audited before the cancellation is re-checked"
    );
    assert!(
        audit_refused.contains("model_call_result"),
        "the audit names the refused model_call_result: {audit_refused}"
    );
    assert_eq!(
        f39_completed_event_count(state, &run_id).await,
        0,
        "a cancelled call never closes with model_call_completed"
    );

    let db_path = state.storage().db_path().to_path_buf();
    state.storage().close().await.expect("close storage");
    let reopened = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("reopen after restart");
    let restarted = reopened
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("query after restart");
    reopened.close().await.expect("close reopened");
    assert_eq!(restarted.len(), 1, "the fence-arm row survives the restart");
    assert_f39_fence_arm_row(&restarted[0]);
    assert_eq!(restarted[0].model_call_id, row.model_call_id);

    let _ = std::fs::remove_dir_all(&boot_res.home);
}
