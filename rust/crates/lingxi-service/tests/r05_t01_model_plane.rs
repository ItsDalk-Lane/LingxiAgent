//! R05-T01 integration evidence: the REAL model plane through the REAL
//! production wiring — strict `--config` file → `resolve_model_plane` →
//! `ServiceDeps` → bootstrap (tool registry + approval service + unified
//! invocation gateway + core file tools + gateway-backed provider) → real
//! loopback HTTP to a stub implementing the openai-completions wire family
//! → REAL file-tool execution → durable honest events.
//!
//! Double boundary: the ONLY double here is the stub HTTP server, which is
//! the outside world at the far end of the wire (a real provider
//! substitute), never a stand-in for any in-process component. Every
//! in-process link — config parse/validate, route resolution, credential
//! material flow, request rendering, response parsing, tool declaration
//! snapshot, gateway prepare/execute, file IO, event persistence — is the
//! production code path.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use lingxi_kernel::model_exchange::ModelGatewayPort as _;
use lingxi_protocol::{EventPayload, KnownEventPayload, UsageRecord};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── the loopback stub provider (the far end of the wire) ────────────────────

struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    body: serde_json::Value,
}

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

/// One scripted stub response: `Immediate` is written as soon as the request
/// arrives; `Gated` is held back until the test releases the gate — the only
/// way to keep a model call genuinely IN FLIGHT while the test mutates the
/// configuration underneath it (C05's snapshot leg).
enum StubResponse {
    Immediate(String),
    Gated(String, Arc<tokio::sync::Notify>),
}

impl StubServer {
    /// Starts a raw-TCP HTTP/1.1 stub. One request per connection (every
    /// response says `Connection: close`); canned responses are popped in
    /// order; an exhausted script is a LOUD 500, never an improvised reply.
    async fn start(responses: Vec<String>) -> Self {
        Self::start_mixed(responses.into_iter().map(StubResponse::Immediate).collect()).await
    }

    async fn start_mixed(responses: Vec<StubResponse>) -> Self {
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
                let (path, authorization, body) = read_request(&mut socket).await;
                task_requests
                    .lock()
                    .expect("requests")
                    .push(RecordedRequest {
                        path,
                        authorization,
                        body,
                    });
                // Pop FIRST (the std guard must not live across the gate
                // await — it is not Send).
                let next = task_responses.lock().expect("responses").pop_front();
                let (status, payload) = match next {
                    Some(StubResponse::Immediate(body)) => ("200 OK", body),
                    Some(StubResponse::Gated(body, gate)) => {
                        gate.notified().await;
                        ("200 OK", body)
                    }
                    None => (
                        "500 Internal Server Error",
                        r#"{"error":{"message":"stub script exhausted"}}"#.to_string(),
                    ),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
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

    fn endpoint(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn requests(&self) -> Vec<RecordedRequestView> {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .map(|r| RecordedRequestView {
                path: r.path.clone(),
                authorization: r.authorization.clone(),
                body: r.body.clone(),
            })
            .collect()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task).await;
    }
}

struct RecordedRequestView {
    path: String,
    authorization: Option<String>,
    body: serde_json::Value,
}

/// Reads one HTTP/1.1 request (request line + headers + content-length
/// body) from the socket. Malformed or oversized input panics — the stub
/// is test equipment, loudness is the point.
async fn read_request(
    socket: &mut tokio::net::TcpStream,
) -> (String, Option<String>, serde_json::Value) {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
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
        assert!(
            raw.len() < 256 * 1024,
            "stub: header block unreasonably large"
        );
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("stub: utf8 headers");
    let mut lines = head.split("\r\n");
    let request_line = lines.next().expect("request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("request path")
        .to_string();
    let mut authorization = None;
    let mut content_length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "authorization" {
                authorization = Some(value.to_string());
            }
            if name == "content-length" {
                content_length = Some(value.parse::<usize>().expect("numeric content-length"));
            }
        }
    }
    let content_length = content_length.expect("json posts carry a content-length");
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 4096];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
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
    (path, authorization, body)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ── canned openai-completions responses ─────────────────────────────────────
// R05-T04: production switched to real streaming (`stream:true`), so the stub
// answers with SSE frames (delta → finish_reason → usage → [DONE]). The
// closed-batch semantics are identical to the old buffered bodies.

fn sse_stream(frames: &[serde_json::Value], usage: serde_json::Value) -> String {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"id": "chatcmpl-stub", "choices": [], "usage": usage})
    ));
    body.push_str("data: [DONE]\n\n");
    body
}

fn tool_call_response(call_id: &str, wire_name: &str, arguments: &serde_json::Value) -> String {
    sse_stream(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                // The stub CLAIMS a different model; the persisted identity must
                // stay the resolved route's (C10 — response identity is ignored).
                "model": "the-stub-lies",
                "choices": [{
                    "index": 0,
                    "finish_reason": null,
                    "delta": {
                        "role": "assistant",
                        "tool_calls": [{
                            "index": 0,
                            "id": call_id,
                            "type": "function",
                            "function": {"name": wire_name, "arguments": arguments.to_string()}
                        }]
                    }
                }]
            }),
            serde_json::json!({
                "id": "chatcmpl-stub",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
            }),
        ],
        serde_json::json!({"prompt_tokens": 11, "completion_tokens": 7}),
    )
}

fn final_response(text: &str) -> String {
    sse_stream(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                "model": "the-stub-lies",
                "choices": [{
                    "index": 0,
                    "finish_reason": null,
                    "delta": {"role": "assistant", "content": text}
                }]
            }),
            serde_json::json!({
                "id": "chatcmpl-stub",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
            }),
        ],
        serde_json::json!({"prompt_tokens": 21, "completion_tokens": 9}),
    )
}

// ── harness ─────────────────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t01-{tag}-{}-{}",
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

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
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
    workspace: PathBuf,
    config_path: PathBuf,
    runtime_dir: PathBuf,
}

/// Boots the REAL composition-root wiring exactly like the binary does:
/// strict config file on disk → `read_service_config` (workspace) +
/// `resolve_model_plane` (plane + source) → `ServiceDeps` → bootstrap
/// (which then wires registry/approvals/gateway/file tools/provider).
async fn boot_with_config(tag: &str, plane_json: &str) -> PlaneBoot {
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
    let runtime_dir = layout.runtime_dir.clone();
    // R05-T02 (C01): the production chain wires its credentials through the
    // credential service — the single material exit — seeded from the SAME
    // validated plane (its store lives in the test's private runtime dir;
    // these planes use static/none auth, so no OAuth flow ever runs).
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
    PlaneBoot {
        state,
        home,
        workspace,
        config_path,
        runtime_dir,
    }
}

async fn execute(state: &ServiceState, input: &str) -> String {
    execute_on(state, "sess_local_alpha", input).await
}

async fn execute_on(state: &ServiceState, session_id: &str, input: &str) -> String {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            session_id,
            input,
            1_790_409_600_000,
        )
        .await
        .expect("execute accepted")
        .run_id
}

async fn run_row(state: &ServiceState, run_id: &str) -> (String, Option<String>) {
    let status = state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query status")
        .expect("run row");
    let reason = state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query reason");
    (status, reason)
}

async fn count_events(state: &ServiceState, run_id: &str, event_type: &str) -> i64 {
    state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = ?2",
            vec![run_id.to_string(), event_type.to_string()],
        )
        .await
        .expect("count query")
        .and_then(|v| v.parse().ok())
        .expect("numeric count")
}

/// The decoded known payloads of one run's durable events (the authority).
async fn known_payloads(
    state: &ServiceState,
    session_id: &str,
    run_id: &str,
) -> Vec<KnownEventPayload> {
    use lingxi_kernel::ports::EventStorePort as _;
    state
        .storage()
        .stream_events_after(session_id, lingxi_protocol::Seq::new(0), 10_000)
        .await
        .expect("authority read")
        .into_iter()
        .filter(|e| e.run_id.as_ref().map(|r| r.as_str()) == Some(run_id))
        .filter_map(|e| match e.payload {
            EventPayload::Known(known) => Some(known),
            _ => None,
        })
        .collect()
}

/// The (provider, model) pairs persisted by this run's model_call_started
/// events — the durable identity proof, never the model id alone.
async fn served_identities(
    state: &ServiceState,
    session_id: &str,
    run_id: &str,
) -> Vec<(String, String)> {
    known_payloads(state, session_id, run_id)
        .await
        .iter()
        .filter_map(|p| match p {
            KnownEventPayload::ModelCallStarted(p) => Some((p.provider.clone(), p.model.clone())),
            _ => None,
        })
        .collect()
}

async fn teardown(state: &ServiceState, home: &Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

const FILE_BODY: &str = "R05-T01 real file body: 真实内容-42\n";

// ── C02: the full chain over real HTTP with a real tool result ──────────────

#[tokio::test]
async fn c02_full_chain_real_wiring_real_tool_real_result() {
    let stub = StubServer::start(vec![
        tool_call_response(
            "call_r05_1",
            "read",
            &serde_json::json!({"path": "note.txt"}),
        ),
        final_response("stub final answer"),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-test-r05"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "stub-model",
            "capabilities": {{"tools": true}}}}}}"#,
        stub.endpoint()
    );
    let boot = boot_with_config("c02", &plane_json).await;
    std::fs::write(boot.workspace.join("note.txt"), FILE_BODY).expect("seed workspace file");

    let run_id = execute(&boot.state, "please read note.txt").await;

    // The run completed with a real final answer through ONE attempt.
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed", "terminal reason: {reason:?}");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    // The stub saw EXACTLY the two protocol turns — no extra probing, no
    // retries — both against the chat-completions route.
    let requests = stub.requests();
    assert_eq!(stub.hits(), 2, "exactly the two turns reached the wire");
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.path, "/v1/chat/completions");
        // The credential material flowed as the bearer header (C11).
        assert_eq!(request.authorization.as_deref(), Some("Bearer sk-test-r05"));
        assert_eq!(request.body["model"], "stub-model");
        // R05-T04: production streams (the stub answers SSE).
        assert_eq!(request.body["stream"], true);
    }
    // Turn 1: the declared tool snapshot went out with the request.
    let tools = requests[0].body["tools"].as_array().expect("tools array");
    assert!(
        tools.iter().any(|t| t["function"]["name"] == "read"
            && t["function"]["parameters"]["properties"]["path"]["type"] == "string"),
        "the read tool declaration rode the request: {tools:?}"
    );
    assert_eq!(
        requests[0].body["messages"]
            .as_array()
            .expect("messages")
            .len(),
        1
    );
    // Turn 2: the driver passed the typed prior exchange and the adapter
    // rendered it — assistant tool call + the REAL tool result content.
    let messages = requests[1].body["messages"].as_array().expect("messages");
    assert_eq!(
        messages.len(),
        3,
        "user + assistant(tool_calls) + tool result"
    );
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_r05_1");
    assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "read");
    assert_eq!(
        messages[1]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"note.txt\"}"
    );
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["tool_call_id"], "call_r05_1");
    assert_eq!(
        messages[2]["content"], FILE_BODY,
        "the tool result on the wire is the REAL file content"
    );

    // Durable facts: two model calls (each with its REAL reported usage)
    // and one tool call with the real result; the persisted identity is
    // the resolved route's — never what the stub claimed.
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_started").await,
        2
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_completed").await,
        2
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_started").await,
        1
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_completed").await,
        1
    );
    let payloads = known_payloads(&boot.state, "sess_local_alpha", &run_id).await;
    let mut usages = Vec::new();
    let mut served = Vec::new();
    let mut tool_contents = Vec::new();
    for payload in &payloads {
        match payload {
            KnownEventPayload::ModelCallStarted(p) => {
                served.push((p.provider.clone(), p.model.clone()));
            }
            KnownEventPayload::ModelCallCompleted(p) => usages.push(p.usage.clone()),
            KnownEventPayload::ToolCallCompleted(p) => {
                tool_contents.push(p.result.content.clone());
            }
            _ => {}
        }
    }
    assert_eq!(
        served,
        vec![
            ("main".to_string(), "stub-model".to_string()),
            ("main".to_string(), "stub-model".to_string())
        ],
        "served_by persists the RESOLVED route, not the stub's claim"
    );
    assert_eq!(
        usages,
        vec![
            Some(UsageRecord {
                input_tokens: 11,
                output_tokens: 7
            }),
            Some(UsageRecord {
                input_tokens: 21,
                output_tokens: 9
            })
        ],
        "the provider-reported usage is persisted per call"
    );
    let tool_text = match &tool_contents[0][0] {
        lingxi_protocol::ContentBlock::Text { text } => text.clone(),
        other => panic!("tool result content must be text: {other:?}"),
    };
    assert_eq!(tool_text, FILE_BODY);

    // The committed final message is the stub's real answer.
    let final_content = boot
        .state
        .storage()
        .query_one_text(
            "SELECT content_json FROM messages WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("final message query")
        .expect("final message row");
    assert!(
        final_content.contains("stub final answer"),
        "{final_content}"
    );

    stub.stop().await;
    teardown(&boot.state, &boot.home).await;
    let _ = std::fs::remove_dir_all(boot.workspace);
    let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
}

// ── C03: no model plane = the explicit unconfigured outcome ─────────────────

#[tokio::test]
async fn c03_no_model_plane_is_the_explicit_unconfigured_outcome() {
    let home = unique_dir("c03-home");
    let layout = prepare_layout(&home).expect("layout");
    // The real resolver over a home WITHOUT any plane source: Ok(None).
    let resolved =
        lingxi_service::config::resolve_model_plane(None, &layout.runtime_dir).expect("resolves");
    assert!(
        resolved.is_none(),
        "no source is the explicit unconfigured state"
    );
    let state =
        ServiceState::bootstrap_with_deps(config_for(&home), &layout, ServiceDeps::default())
            .await
            .expect("bootstrap");

    let run_id = execute(&state, "hello with no model configured").await;
    let (status, reason) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        reason.as_deref(),
        Some("completed.no_final.no_provider_configured"),
        "honest unconfigured outcome — never a fabricated reply"
    );
    assert_eq!(
        count_events(&state, &run_id, "model_call_started").await,
        0,
        "no model call was even started"
    );
    teardown(&state, &home).await;
}

// ── C04: same model id under two providers routes by identity pair ──────────

#[tokio::test]
async fn c04_same_model_id_different_providers_never_cross_wire() {
    // An EMPTY script on A: any accidental contact is a loud 500 that fails
    // the run, never an improvised success.
    let stub_a = StubServer::start(vec![]).await;
    let stub_b = StubServer::start(vec![
        final_response("from B sequential"),
        final_response("from B concurrent"),
        final_response("from B concurrent"),
    ])
    .await;
    // BOTH providers declare the SAME model id; the chat route pins the
    // pair (b, shared-model). The endpoint that receives the request is the
    // identity proof — never the model id.
    let plane_json = format!(
        r#""providers": {{
            "a": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "none"}}
            }},
            "b": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "none"}}
            }}
        }},
        "models": {{"chat": {{"provider": "b", "model": "shared-model",
            "capabilities": {{"tools": true}}}}}}"#,
        stub_a.endpoint(),
        stub_b.endpoint()
    );
    let boot = boot_with_config("c04", &plane_json).await;

    // Leg 1 — one sequential call pins (b, shared-model).
    let run_id = execute(&boot.state, "hi").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(stub_a.hits(), 0, "provider A must never be contacted");
    assert_eq!(stub_b.hits(), 1, "the pinned provider B served the call");
    assert_eq!(stub_b.requests()[0].body["model"], "shared-model");
    assert!(
        stub_b.requests()[0].authorization.is_none(),
        "auth kind none sends no credential header"
    );
    assert_eq!(
        served_identities(&boot.state, "sess_local_alpha", &run_id).await,
        vec![("b".to_string(), "shared-model".to_string())],
        "the persisted identity is the (provider, model) pair"
    );

    // Leg 2 — concurrent calls from two sessions never cross-wire either.
    let (alpha_run, beta_run) = tokio::join!(
        execute_on(&boot.state, "sess_local_alpha", "concurrent alpha"),
        execute_on(&boot.state, "sess_local_beta", "concurrent beta"),
    );
    for (session, concurrent_run) in [
        ("sess_local_alpha", &alpha_run),
        ("sess_local_beta", &beta_run),
    ] {
        let (status, _) = run_row(&boot.state, concurrent_run).await;
        assert_eq!(status, "completed");
        assert_eq!(
            served_identities(&boot.state, session, concurrent_run).await,
            vec![("b".to_string(), "shared-model".to_string())],
            "concurrent run {concurrent_run} stayed on the pinned pair"
        );
    }
    assert_eq!(
        stub_a.hits(),
        0,
        "provider A must never be contacted, even under concurrency"
    );
    assert_eq!(
        stub_b.hits(),
        3,
        "every call landed on the pinned provider B"
    );
    for request in stub_b.requests() {
        assert_eq!(request.body["model"], "shared-model");
    }

    stub_a.stop().await;
    stub_b.stop().await;
    teardown(&boot.state, &boot.home).await;
    let _ = std::fs::remove_dir_all(boot.workspace);
    let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
}

// ── C06: an unimplemented protocol family refuses before ANY request ────────

#[tokio::test]
async fn c06_unsupported_family_makes_zero_requests() {
    let stub = StubServer::start(vec![]).await;
    // R05-T03 landed the four remaining CHAT families; the media/speech
    // families (volcengine-bigasr / system-speech) stay loud-refused until
    // R05-T06 — this pin now guards that boundary.
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "volcengine-bigasr",
                "endpoint": "{}",
                "auth": {{"kind": "none"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "asr-test"}}}}"#,
        stub.endpoint()
    );
    let boot = boot_with_config("c06", &plane_json).await;

    let run_id = execute(&boot.state, "hi").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        stub.hits(),
        0,
        "a configured-but-unimplemented family must never hit the wire"
    );
    stub.stop().await;
    teardown(&boot.state, &boot.home).await;
    let _ = std::fs::remove_dir_all(boot.workspace);
    let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
}

// ── C05: reload through the management surface swaps atomically ─────────────

/// Minimal real-HTTP client (loopback, one request per connection) for the
/// management surface; the same bounded-wait discipline as the R00 harness.
async fn http_post(addr: SocketAddr, path: &str, bearer: &str) -> (u16, serde_json::Value) {
    let exchanged = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to real service");
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {bearer}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        socket
            .write_all(request.as_bytes())
            .await
            .expect("write request");
        let mut raw = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match socket.read(&mut chunk).await {
                Ok(0) => break,
                Ok(read) => raw.extend_from_slice(&chunk[..read]),
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    break;
                }
                Err(err) => panic!("read response: {err}"),
            }
        }
        raw
    })
    .await
    .expect("management request stalled — environment failure, honestly surfaced");
    let text = String::from_utf8_lossy(&exchanged);
    let (head, body) = text.split_once("\r\n\r\n").expect("header/body split");
    let status = head
        .split_whitespace()
        .nth(1)
        .expect("status")
        .parse::<u16>()
        .expect("numeric status");
    (
        status,
        serde_json::from_str(body).expect("JSON response body"),
    )
}

#[tokio::test]
async fn c05_reload_through_management_surface_swaps_atomically() {
    let stub_a = StubServer::start(vec![final_response("from generation 1")]).await;
    let stub_b = StubServer::start(vec![final_response("from generation 2")]).await;
    let boot = {
        let plane_json = plane_json_for(&stub_a.endpoint(), "stub-model-v1");
        boot_with_config("c05", &plane_json).await
    };
    // The REAL serving surface (management routes included) on loopback.
    let token = boot.state.auth().local_token();
    let state_for_server = boot.state.clone();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let serve_task = tokio::spawn(async move {
        lingxi_service::run(
            state_for_server,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready.send(addr);
            },
            None,
        )
        .await
    });
    let addr = tokio::time::timeout(std::time::Duration::from_secs(10), ready_rx)
        .await
        .expect("service READY within budget")
        .expect("service READY");

    // Reload over the REAL management endpoint: the SAME source (the
    // --config file) is re-read; the new plane points at stub B.
    let generation_1 = boot
        .state
        .model_gateway()
        .expect("gateway wired")
        .config_generation();
    assert_eq!(generation_1, 1);
    rewrite_plane(&boot, &plane_json_for(&stub_b.endpoint(), "stub-model-v2"));
    let (status, body) = http_post(addr, "/lingxi/v1/models/reload", &token).await;
    assert_eq!(status, 200, "reload accepted: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["generation"], 2);
    assert_eq!(
        boot.state
            .model_gateway()
            .expect("gateway")
            .config_generation(),
        2
    );

    // A broken source is a loud 409 and the running snapshot is untouched.
    rewrite_plane(&boot, &plane_json_for("not-a-url", "stub-model-v2"));
    let (status, body) = http_post(addr, "/lingxi/v1/models/reload", &token).await;
    assert_eq!(status, 409, "invalid plane refused loudly: {body}");
    assert_eq!(
        boot.state
            .model_gateway()
            .expect("gateway")
            .config_generation(),
        2,
        "a refused reload never half-swaps the snapshot"
    );
    rewrite_plane(&boot, &plane_json_for(&stub_b.endpoint(), "stub-model-v2"));
    let (status, _) = http_post(addr, "/lingxi/v1/models/reload", &token).await;
    assert_eq!(status, 200);

    // The NEXT run's dispatch resolves against the new snapshot: stub B,
    // model v2; generation 1's stub never hears anything.
    let run_id = execute(&boot.state, "hi after reload").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(stub_a.hits(), 0, "the old route is gone after reload");
    assert_eq!(stub_b.hits(), 1);
    assert_eq!(stub_b.requests()[0].body["model"], "stub-model-v2");
    let payloads = known_payloads(&boot.state, "sess_local_alpha", &run_id).await;
    let served: Vec<_> = payloads
        .iter()
        .filter_map(|p| match p {
            KnownEventPayload::ModelCallStarted(p) => Some((p.provider.clone(), p.model.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        served,
        vec![("main".to_string(), "stub-model-v2".to_string())]
    );

    // The reload was audited through the management journal.
    let management = std::fs::read(boot.runtime_dir.join("management.json")).expect("audit file");
    let management: serde_json::Value = serde_json::from_slice(&management).expect("audit json");
    let audit = management["audit"].as_array().expect("audit array");
    assert!(
        audit
            .iter()
            .any(|entry| entry["action"] == "models.reload" && entry["metadata"]["generation"] == 2),
        "the reload is journaled: {audit:?}"
    );

    let _ = stop.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), serve_task).await;
    stub_a.stop().await;
    stub_b.stop().await;
    teardown(&boot.state, &boot.home).await;
    let _ = std::fs::remove_dir_all(boot.workspace);
    let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
}

#[tokio::test]
async fn c05_in_flight_call_keeps_its_snapshot_and_cancel_is_unaffected() {
    // Generation 1's stub HOLDS its final answer until the test releases the
    // gate — the only honest way to keep a model call genuinely IN FLIGHT
    // while the configuration changes underneath it.
    let gate_a = Arc::new(tokio::sync::Notify::new());
    let gate_b = Arc::new(tokio::sync::Notify::new());
    let stub_a = StubServer::start_mixed(vec![StubResponse::Gated(
        final_response("generation 1 in-flight answer"),
        Arc::clone(&gate_a),
    )])
    .await;
    let stub_b = StubServer::start_mixed(vec![
        StubResponse::Immediate(final_response("generation 2 answer")),
        StubResponse::Gated(
            final_response("generation 2 late answer — must never land"),
            Arc::clone(&gate_b),
        ),
    ])
    .await;
    let boot = {
        let plane_json = plane_json_for(&stub_a.endpoint(), "stub-model-v1");
        boot_with_config("c05-inflight", &plane_json).await
    };

    // Run 1 dispatches against generation 1 and parks inside the stub.
    let run_state = boot.state.clone();
    let run1 = tokio::spawn(async move { execute(&run_state, "in-flight call").await });
    wait_until_hits(&stub_a, 1).await;

    // Reload while run 1 is IN FLIGHT (the same ConfigModelGateway::reload
    // call the management surface makes after re-reading the source).
    let new_plane =
        lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&format!(
            "{{{}}}",
            plane_json_for(&stub_b.endpoint(), "stub-model-v2")
        ))
        .expect("valid plane");
    let generation = boot
        .state
        .model_gateway()
        .expect("gateway")
        .reload(new_plane);
    assert_eq!(generation, 2);

    // Releasing generation 1's answer completes run 1 through the SNAPSHOT
    // IT RESOLVED AT DISPATCH — never re-resolved against generation 2.
    gate_a.notify_one();
    let run1_id = tokio::time::timeout(std::time::Duration::from_secs(15), run1)
        .await
        .expect("run 1 settles within budget")
        .expect("run 1 task");
    let (status, reason) = run_row(&boot.state, &run1_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(
        served_identities(&boot.state, "sess_local_alpha", &run1_id).await,
        vec![("main".to_string(), "stub-model-v1".to_string())],
        "the in-flight call persisted its dispatch-time snapshot identity"
    );
    let final1 = final_message_text(&boot.state, &run1_id).await;
    assert!(
        final1.contains("generation 1 in-flight answer"),
        "run 1 committed generation 1's answer: {final1}"
    );

    // The NEXT run resolves against generation 2; generation 1's stub is
    // never contacted again.
    let run2_id = execute(&boot.state, "after reload").await;
    let (status, _) = run_row(&boot.state, &run2_id).await;
    assert_eq!(status, "completed");
    assert_eq!(stub_a.hits(), 1, "generation 1 never heard another call");
    assert_eq!(stub_b.hits(), 1);
    assert_eq!(stub_b.requests()[0].body["model"], "stub-model-v2");

    // Cancellation is unaffected by the snapshot discipline: run 3 (its own
    // session, so its id is unambiguous while in flight) parks at generation
    // 2's gated answer, is cancelled, and must settle cancelled; the late
    // answer, once released, must never be committed.
    let run_state = boot.state.clone();
    let run3 =
        tokio::spawn(async move { execute_on(&run_state, "sess_local_beta", "cancel me").await });
    wait_until_hits(&stub_b, 2).await;
    let run3_id = boot
        .state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE session_id = 'sess_local_beta'",
            vec![],
        )
        .await
        .expect("run 3 row")
        .expect("run 3 exists once its model call is in flight");
    let outcome = boot
        .state
        .sessions()
        .cancel_run_for(
            boot.state.storage().as_ref(),
            boot.state.runs(),
            &owner_principal(),
            &run3_id,
        )
        .await
        .expect("cancel accepted");
    let _ = outcome;
    let settled_run3 = tokio::time::timeout(std::time::Duration::from_secs(15), run3)
        .await
        .expect("run 3 settles within budget")
        .expect("run 3 task");
    assert_eq!(settled_run3, run3_id);
    let (status, _) = run_row(&boot.state, &run3_id).await;
    assert_eq!(status, "cancelled", "cancellation works mid-flight");
    gate_b.notify_one();
    tokio::task::yield_now().await;
    let (status, _) = run_row(&boot.state, &run3_id).await;
    assert_eq!(
        status, "cancelled",
        "the released late answer must not resurrect the run"
    );
    assert!(
        final_message_text_opt(&boot.state, &run3_id)
            .await
            .is_none(),
        "no final message is ever committed for a cancelled run"
    );

    stub_a.stop().await;
    stub_b.stop().await;
    teardown(&boot.state, &boot.home).await;
    let _ = std::fs::remove_dir_all(boot.workspace);
    let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
}

/// Polls the stub's request counter until it reaches `expected` (bounded —
/// a missing request is a loud failure, not a hang).
async fn wait_until_hits(stub: &StubServer, expected: usize) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while stub.hits() < expected {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "stub never received {expected} requests (has {})",
            stub.hits()
        )
    });
}

/// The committed final message text of a run (panics when absent).
async fn final_message_text(state: &ServiceState, run_id: &str) -> String {
    final_message_text_opt(state, run_id)
        .await
        .expect("a committed final message")
}

async fn final_message_text_opt(state: &ServiceState, run_id: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(
            "SELECT content_json FROM messages WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("final message query")
}

#[tokio::test]
async fn c05_reload_without_a_configured_plane_is_a_loud_404() {
    let home = unique_dir("c05-none-home");
    let layout = prepare_layout(&home).expect("layout");
    let state =
        ServiceState::bootstrap_with_deps(config_for(&home), &layout, ServiceDeps::default())
            .await
            .expect("bootstrap");
    let token = state.auth().local_token();
    let state_for_server = state.clone();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let serve_task = tokio::spawn(async move {
        lingxi_service::run(
            state_for_server,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready.send(addr);
            },
            None,
        )
        .await
    });
    let addr = tokio::time::timeout(std::time::Duration::from_secs(10), ready_rx)
        .await
        .expect("service READY within budget")
        .expect("service READY");
    let (status, body) = http_post(addr, "/lingxi/v1/models/reload", &token).await;
    assert_eq!(status, 404, "no plane configured is a loud 404: {body}");
    assert_eq!(body["details"]["reason"], "model_plane_unconfigured");
    let _ = stop.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), serve_task).await;
    teardown(&state, &home).await;
}

fn plane_json_for(endpoint: &str, model: &str) -> String {
    format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{endpoint}",
                "auth": {{"kind": "none"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "{model}",
            "capabilities": {{"tools": true}}}}}}"#
    )
}

/// Rewrites the SAME --config file with a new embedded plane (the reload
/// source contract: re-read the same file, never a guessed location).
fn rewrite_plane(boot: &PlaneBoot, plane_json: &str) {
    let config_json = format!(
        r#"{{"home": {}, "workspace": {}, {plane_json}}}"#,
        serde_json::to_string(&boot.home.to_string_lossy()).expect("home json"),
        serde_json::to_string(&boot.workspace.to_string_lossy()).expect("ws json"),
    );
    std::fs::write(&boot.config_path, config_json).expect("rewrite config");
}

// ═══ R05 RR1 WP-T01: F01/F02 permanent regressions ═══════════════════════════
//
// Migrated from the adversarial evidence package
// (`artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/audit/
// credential_network/src/lib.rs`, findings CN-F03 and CN-F01), adapted to
// this crate's fixtures. The behavioral assertions are NOT weakened: a
// model that has not DECLARED a capability must produce ZERO physical HTTP
// requests when the turn needs it (R05-A02), and a configuration reload
// must never hand NEW credential material to a route resolved against the
// OLD generation (R05-T01-C05 / T02-C10) — the old endpoint must never
// receive the new endpoint's key, not on the first resolve and not on the
// bounded 401 retry.
//
// Boundary discipline (per the RR1 master prompt §3.3): every negative
// test first proves it REACHES the dispatch boundary (the same fixture
// with the capability declared — or without the capability need — really
// hits the wire), then asserts the refusal; an earlier
// parse/config/refusal cannot stand in for the boundary evidence.

mod rr1_f01_f02 {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use lingxi_adapters::models::config::ModelPlaneConfig;
    use lingxi_adapters::models::credentials::{ApplicableAuth, ProviderCredentialPort};
    use lingxi_adapters::models::gateway::ConfigModelGateway;
    use lingxi_adapters::models::provider::GatewayedProvider;
    use lingxi_kernel::model_exchange::{
        InputImage, ModelGatewayPort as _, ModelOperation, ModelRouteRequest, ModelTurnInput,
        ToolDeclaration, ToolDeclarationSnapshot,
    };
    use lingxi_kernel::ports::{
        ModelTurnDelta, ProviderTurn, TurnDeltaSink, TurnDeltaSinkClosed, TurnProviderPort as _,
    };
    use lingxi_kernel::toolcatalog::SchemaBudget;
    use lingxi_protocol::{AttemptId, ModelCallId, RunId, SessionId, ToolSchemaDocument};
    use lingxi_service::credentials::{CredentialService, ProductionRefreshDriver};
    use lingxi_service::inject::ManualClock;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    /// The SSE body a well-behaved openai-completions stub settles with.
    const SSE_FINAL: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

    /// Builds a one-provider plane whose chat binding carries `extra`
    /// (`serde_json::json!({})` = the undeclared-capability state).
    fn chat_plane(endpoint: &str, key: &str, extra: serde_json::Value) -> ModelPlaneConfig {
        let mut binding = serde_json::json!({
            "provider": "main",
            "model": "synthetic-model-without-declared-capabilities"
        });
        if let (Some(binding), Some(extra)) = (binding.as_object_mut(), extra.as_object()) {
            for (field, value) in extra {
                binding.insert(field.clone(), value.clone());
            }
        }
        ModelPlaneConfig::from_value(serde_json::json!({
            "providers": {"main": {
                "protocol": "openai-completions",
                "endpoint": endpoint,
                "auth": {"kind": "apiKey", "apiKey": key}
            }},
            "models": {"chat": binding}
        }))
        .expect("valid plane")
    }

    /// A plane whose chat binding names an explicit model (the kept-seed /
    /// hot-reload legs need two generations of the same provider).
    fn chat_plane_model(
        endpoint: &str,
        key: &str,
        model: &str,
        extra: serde_json::Value,
    ) -> ModelPlaneConfig {
        let mut plane = chat_plane(endpoint, key, extra);
        plane.models.chat.as_mut().expect("chat").model = model.to_string();
        plane
    }

    /// A two-provider plane (the same model id under two providers —
    /// capability declarations belong to the ROUTE identity, C04/F01):
    /// `chat` binds `alpha`, `title` binds `beta`, both to `shared-model`.
    fn two_provider_plane(
        alpha_endpoint: &str,
        alpha_key: &str,
        beta_endpoint: &str,
        beta_key: &str,
        chat_extra: serde_json::Value,
        title_extra: serde_json::Value,
    ) -> ModelPlaneConfig {
        let mut chat = serde_json::json!({"provider": "alpha", "model": "shared-model"});
        let mut title = serde_json::json!({"provider": "beta", "model": "shared-model"});
        for (binding, extra) in [(&mut chat, chat_extra), (&mut title, title_extra)] {
            if let (Some(binding), Some(extra)) = (binding.as_object_mut(), extra.as_object()) {
                for (field, value) in extra {
                    binding.insert(field.clone(), value.clone());
                }
            }
        }
        ModelPlaneConfig::from_value(serde_json::json!({
            "providers": {
                "alpha": {
                    "protocol": "openai-completions",
                    "endpoint": alpha_endpoint,
                    "auth": {"kind": "apiKey", "apiKey": alpha_key}
                },
                "beta": {
                    "protocol": "openai-completions",
                    "endpoint": beta_endpoint,
                    "auth": {"kind": "apiKey", "apiKey": beta_key}
                }
            },
            "models": {"chat": chat, "title": title}
        }))
        .expect("valid plane")
    }

    /// Extracts the JSON body out of a captured raw HTTP request.
    fn request_body(raw: &str) -> serde_json::Value {
        let body = raw
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or_default();
        serde_json::from_str(body.trim()).expect("request body is JSON")
    }

    fn credentials(cfg: &ModelPlaneConfig) -> CredentialService {
        CredentialService::new(
            cfg,
            None,
            Arc::new(ProductionRefreshDriver::new(Duration::from_secs(1)).expect("driver")),
            Arc::new(ManualClock::new(1000)),
        )
    }

    struct NullSink;
    impl TurnDeltaSink for NullSink {
        fn emit<'a>(
            &'a self,
            _: ModelTurnDelta,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>,
        > {
            Box::pin(async { Ok(()) })
        }
    }

    fn ctx() -> lingxi_kernel::RunContext {
        lingxi_kernel::RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: SessionId::new("rr1_wp_t01_session"),
            run_id: RunId::new("rr1_wp_t01_run"),
            attempt: AttemptId::new("rr1_wp_t01_attempt"),
            generation: 1,
        }
    }

    fn one_tool_snapshot() -> ToolDeclarationSnapshot {
        ToolDeclarationSnapshot {
            catalog_generation: 1,
            declarations: vec![ToolDeclaration {
                target: "read".into(),
                wire_name: "read".into(),
                description: "Read".into(),
                input_schema: ToolSchemaDocument {
                    dialect: "https://json-schema.org/draft/2020-12/schema".into(),
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    }),
                },
            }],
        }
    }

    /// A raw loopback capture server: accepts connections until aborted,
    /// records every raw request (head + body) and answers each with a
    /// well-formed SSE 200. Used by the F01 refusal legs — the physical
    /// request count IS the evidence.
    async fn capture_server() -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("capture bind");
        let addr = listener.local_addr().expect("capture addr");
        let observed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::clone(&observed);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut conn, _)) = listener.accept().await else {
                    break;
                };
                let raw = read_raw_request(&mut conn).await;
                state.lock().expect("observed").push(raw);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{SSE_FINAL}",
                    SSE_FINAL.len()
                );
                let _ = conn.write_all(response.as_bytes()).await;
                let _ = conn.shutdown().await;
            }
        });
        (format!("http://{addr}/v1"), observed, task)
    }

    // ── F01: undeclared capability ⇒ zero requests, loud local refusal ──────

    #[tokio::test]
    async fn f01_undeclared_tools_capability_makes_zero_requests() {
        let (endpoint, observed, stub) = capture_server().await;
        let cfg = chat_plane(&endpoint, "dummy-key", serde_json::json!({}));
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider =
            GatewayedProvider::new(gateway, svc, SchemaBudget::default()).expect("provider");
        let input = ModelTurnInput::first_turn("read a file", one_tool_snapshot());
        let context = ctx();
        let call = ModelCallId::new("rr1_f01_tools_call");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call, &input, &NullSink),
        )
        .await
        .expect("turn settles within budget");
        stub.abort();
        assert_eq!(
            observed.lock().expect("observed").len(),
            0,
            "the gateway dispatched a tools-bearing request for a model with NO declared \
             tools capability (R05-A02: refuse before any request leaves the process)"
        );
        assert!(
            matches!(result.turn, ProviderTurn::Failed { .. }),
            "the refusal must be a loud local failure naming the missing declared \
             capability — never a silent tool drop, model swap or re-route: {:?}",
            result.turn
        );
    }

    #[tokio::test]
    async fn f01_undeclared_image_capability_makes_zero_requests() {
        let (endpoint, observed, stub) = capture_server().await;
        let cfg = chat_plane(&endpoint, "dummy-key", serde_json::json!({}));
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider =
            GatewayedProvider::new(gateway, svc, SchemaBudget::default()).expect("provider");
        let mut input =
            ModelTurnInput::first_turn("describe image", ToolDeclarationSnapshot::empty());
        input.images.push(InputImage {
            bytes: vec![137, 80, 78, 71],
            mime: "image/png".into(),
        });
        let context = ctx();
        let call = ModelCallId::new("rr1_f01_image_call");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call, &input, &NullSink),
        )
        .await
        .expect("turn settles within budget");
        stub.abort();
        assert_eq!(
            observed.lock().expect("observed").len(),
            0,
            "the gateway dispatched an image-bearing request for a model with NO \
             declared image-input capability (R05-A02)"
        );
        assert!(
            matches!(result.turn, ProviderTurn::Failed { .. }),
            "the refusal must be a loud local failure: {:?}",
            result.turn
        );
    }

    /// The normal control of the two refusal legs: the SAME undeclared
    /// model with a turn that needs NO capability really hits the wire —
    /// proving the refusal above is the capability check, not a fixture
    /// that never dispatches.
    #[tokio::test]
    async fn f01_control_no_capability_needs_still_dispatches() {
        let (endpoint, observed, stub) = capture_server().await;
        let cfg = chat_plane(&endpoint, "dummy-key", serde_json::json!({}));
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider =
            GatewayedProvider::new(gateway, svc, SchemaBudget::default()).expect("provider");
        let input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
        let context = ctx();
        let call = ModelCallId::new("rr1_f01_control_call");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call, &input, &NullSink),
        )
        .await
        .expect("turn settles within budget");
        stub.abort();
        assert_eq!(
            observed.lock().expect("observed").len(),
            1,
            "the control dispatches exactly one real request"
        );
        assert!(
            matches!(result.turn, ProviderTurn::Final { .. }),
            "the control settles a final turn: {:?}",
            result.turn
        );
    }

    // ── F02: reload/401 must not mix credential material across generations ──

    /// The headline counterexample (CN-F01), barrier-controlled on real
    /// loopback HTTP: endpoint A holds request 1 (old key) until the test
    /// swaps the plane to endpoint B + new key, then answers 401. The
    /// bounded 401 retry must NEVER put the new key on the wire toward A
    /// (and B is never contacted by the stale call).
    #[tokio::test]
    async fn f02_reload_during_401_never_sends_new_key_to_old_endpoint() {
        let listener_a = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind A");
        let addr_a = listener_a.local_addr().expect("addr A");
        let listener_b = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind B");
        let addr_b = listener_b.local_addr().expect("addr B");
        let (first_seen_tx, first_seen_rx) = tokio::sync::oneshot::channel::<String>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let to_a: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let to_b: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        // Endpoint A: request 1 → gate → 401; then any further connection
        // (the defect's retry) is captured and answered so the OLD code
        // path completes and the leak is observable.
        let task_a = {
            let to_a = Arc::clone(&to_a);
            tokio::spawn(async move {
                let (mut first, _) = listener_a.accept().await.expect("A conn 1");
                let request = read_raw_request(&mut first).await;
                to_a.lock().expect("to_a").push(request.clone());
                let _ = first_seen_tx.send(request);
                release_rx.await.expect("release");
                let _ = first
                    .write_all(
                        b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = first.shutdown().await;
                // Any RETRY toward the old endpoint is the defect; accept
                // a bounded number so the old-red run observes it instead
                // of hanging.
                for _ in 0..3 {
                    let Ok(Ok((mut conn, _))) =
                        tokio::time::timeout(Duration::from_millis(500), listener_a.accept()).await
                    else {
                        break;
                    };
                    let request = read_raw_request(&mut conn).await;
                    to_a.lock().expect("to_a").push(request);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{SSE_FINAL}",
                        SSE_FINAL.len()
                    );
                    let _ = conn.write_all(response.as_bytes()).await;
                    let _ = conn.shutdown().await;
                }
            })
        };
        // Endpoint B: the NEW endpoint — the stale call must never reach
        // it either (its route was frozen at the old generation).
        let task_b = {
            let to_b = Arc::clone(&to_b);
            tokio::spawn(async move {
                while let Ok(Ok((mut conn, _))) =
                    tokio::time::timeout(Duration::from_millis(500), listener_b.accept()).await
                {
                    let request = read_raw_request(&mut conn).await;
                    to_b.lock().expect("to_b").push(request);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{SSE_FINAL}",
                        SSE_FINAL.len()
                    );
                    let _ = conn.write_all(response.as_bytes()).await;
                    let _ = conn.shutdown().await;
                }
            })
        };

        let cfg_a = chat_plane(
            &format!("http://{addr_a}/v1"),
            "dummy-old-key",
            serde_json::json!({}),
        );
        let svc = Arc::new(credentials(&cfg_a));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg_a));
        let provider = GatewayedProvider::new(
            Arc::clone(&gateway),
            Arc::clone(&svc) as Arc<dyn ProviderCredentialPort>,
            SchemaBudget::default(),
        )
        .expect("provider");
        let call_task = {
            let provider = provider;
            tokio::spawn(async move {
                let context = ctx();
                let call = ModelCallId::new("rr1_f02_401_call");
                let input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
                provider.next_turn(&context, &call, &input, &NullSink).await
            })
        };

        // Boundary proof: the old endpoint really received request 1 with
        // the OLD key (the call reached the wire before the reload).
        let first = tokio::time::timeout(Duration::from_secs(5), first_seen_rx)
            .await
            .expect("request 1 reaches endpoint A")
            .expect("request 1 channel");
        assert!(
            first.contains("Bearer dummy-old-key"),
            "boundary: endpoint A received the old key first: {first}"
        );

        // Swap the plane to endpoint B + new key — the same two-step reload
        // the management surface performs: the credential re-seed stamped
        // with the generation the gateway swap is about to publish (2),
        // then the gateway swap itself.
        let cfg_b = chat_plane(
            &format!("http://{addr_b}/v1"),
            "dummy-new-key",
            serde_json::json!({}),
        );
        svc.reload(&cfg_b, gateway.upcoming_generation()).await;
        gateway.reload(cfg_b);

        release_tx.send(()).expect("release 401");
        let result = tokio::time::timeout(Duration::from_secs(5), call_task)
            .await
            .expect("call settles within budget")
            .expect("call task");
        let _ = tokio::time::timeout(Duration::from_secs(2), task_a).await;
        task_b.abort();

        let to_a = to_a.lock().expect("to_a").clone();
        let to_b = to_b.lock().expect("to_b").clone();
        for (index, request) in to_a.iter().enumerate() {
            assert!(
                !request.contains("dummy-new-key"),
                "the OLD endpoint A captured the NEW endpoint's key on request #{index} \
                 (401 retry mixed credential material across config generations): {}",
                request
                    .lines()
                    .find(|l| l.to_lowercase().starts_with("authorization:"))
                    .unwrap_or("<missing>")
            );
        }
        for (index, request) in to_b.iter().enumerate() {
            assert!(
                !request.contains("dummy-new-key"),
                "the stale in-flight call reached endpoint B (#{index}) — its route was \
                 frozen at the old generation"
            );
        }
        assert!(
            matches!(result.turn, ProviderTurn::Failed { .. }),
            "the stale call must settle as an honest safe failure (never a silent \
             success on mixed-generation material): {:?}",
            result.turn
        );
    }

    /// The direct leg (CN-F01's second test): a route resolved at the OLD
    /// generation must never resolve the NEW generation's material — no
    /// network involved, the credential service itself is under test.
    #[tokio::test]
    async fn f02_old_route_never_resolves_new_generation_material() {
        let old = chat_plane(
            "https://old.example.invalid/v1",
            "dummy-old-key",
            serde_json::json!({}),
        );
        let next = chat_plane(
            "https://new.example.invalid/v1",
            "dummy-new-key",
            serde_json::json!({}),
        );
        let gateway = ConfigModelGateway::from_validated(old.clone());
        let svc = credentials(&old);
        let old_route = gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("old route resolves");
        assert_eq!(
            old_route.endpoint, "https://old.example.invalid/v1",
            "boundary: the route under test really targets the old endpoint"
        );
        // The plane install the paired gateway reload would publish (2) —
        // the same lockstep the management surface maintains.
        svc.reload(&next, gateway.upcoming_generation()).await;
        let resolved = svc.resolve(&old_route).await;
        assert!(
            !matches!(resolved, Ok(ApplicableAuth::Bearer(ref key)) if key == "dummy-new-key"),
            "an old-generation route was handed the NEW endpoint's key: {resolved:?}"
        );
        // Strengthened (the fix's contract): the refusal is the loud
        // stale-generation error, never a quiet hand-out of any material.
        assert!(
            matches!(
                resolved,
                Err(lingxi_adapters::models::credentials::CredentialError::StaleRoute { .. })
            ),
            "an old-generation route must be refused with the stale-generation \
             error (safe failure), got: {resolved:?}"
        );
    }

    /// Reads one full HTTP/1.1 request (headers + content-length body).
    async fn read_raw_request(conn: &mut tokio::net::TcpStream) -> String {
        let mut raw = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let read = tokio::time::timeout(Duration::from_secs(10), conn.read(&mut chunk))
                .await
                .expect("read stalled")
                .expect("read");
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
            if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse().ok())
                    })
                    .unwrap_or(0);
                if raw.len() >= end + 4 + len {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&raw).into_owned()
    }

    // ── F01 positive legs: the declared states and the identity rules ────────

    /// `Some(true)` is the ONLY authorizing state: a declared model really
    /// dispatches and the wire carries the tool declaration verbatim.
    #[tokio::test]
    async fn f01_declared_tools_capability_sends_the_tools_payload() {
        let (endpoint, observed, stub) = capture_server().await;
        let cfg = chat_plane(
            &endpoint,
            "dummy-key",
            serde_json::json!({"capabilities": {"tools": true}}),
        );
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider =
            GatewayedProvider::new(gateway, svc, SchemaBudget::default()).expect("provider");
        let input = ModelTurnInput::first_turn("read a file", one_tool_snapshot());
        let context = ctx();
        let call = ModelCallId::new("rr1_f01_pos_tools");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call, &input, &NullSink),
        )
        .await
        .expect("turn settles within budget");
        stub.abort();
        let observed = observed.lock().expect("observed").clone();
        assert_eq!(observed.len(), 1, "the declared model dispatches");
        assert!(
            matches!(result.turn, ProviderTurn::Final { .. }),
            "the declared model settles normally: {:?}",
            result.turn
        );
        let body = request_body(&observed[0]);
        let tools = body["tools"].as_array().expect("tools on the wire");
        assert!(
            tools
                .iter()
                .any(|tool| tool["function"]["name"] == "read" && tool["type"] == "function"),
            "the declaration travels verbatim (never silently dropped): {body}"
        );
    }

    /// `Some(false)` (explicitly unsupported) behaves exactly like the
    /// undeclared state: zero requests, loud refusal.
    #[tokio::test]
    async fn f01_explicitly_unsupported_tools_capability_makes_zero_requests() {
        let (endpoint, observed, stub) = capture_server().await;
        let cfg = chat_plane(
            &endpoint,
            "dummy-key",
            serde_json::json!({"capabilities": {"tools": false}}),
        );
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider =
            GatewayedProvider::new(gateway, svc, SchemaBudget::default()).expect("provider");
        let input = ModelTurnInput::first_turn("read a file", one_tool_snapshot());
        let context = ctx();
        let call = ModelCallId::new("rr1_f01_false_tools");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call, &input, &NullSink),
        )
        .await
        .expect("turn settles within budget");
        stub.abort();
        assert_eq!(
            observed.lock().expect("observed").len(),
            0,
            "an explicitly-unsupported capability is refused before dispatch"
        );
        let message = match result.turn {
            ProviderTurn::Failed { error, .. } => error.message,
            other => panic!("expected a loud failure, got {other:?}"),
        };
        assert!(
            message.contains("tools") && message.contains("declared unsupported"),
            "the refusal names the capability and its declared state: {message}"
        );
    }

    /// The declared image-input leg: the image really travels (as the
    /// openai-completions family renders it — a data-URL content part).
    #[tokio::test]
    async fn f01_declared_image_capability_sends_the_image_payload() {
        let (endpoint, observed, stub) = capture_server().await;
        let cfg = chat_plane(
            &endpoint,
            "dummy-key",
            serde_json::json!({"capabilities": {"imageInput": true}}),
        );
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider =
            GatewayedProvider::new(gateway, svc, SchemaBudget::default()).expect("provider");
        let mut input =
            ModelTurnInput::first_turn("describe image", ToolDeclarationSnapshot::empty());
        input.images.push(InputImage {
            bytes: vec![137, 80, 78, 71],
            mime: "image/png".into(),
        });
        let context = ctx();
        let call = ModelCallId::new("rr1_f01_pos_image");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call, &input, &NullSink),
        )
        .await
        .expect("turn settles within budget");
        stub.abort();
        let observed = observed.lock().expect("observed").clone();
        assert_eq!(observed.len(), 1, "the declared model dispatches");
        assert!(
            matches!(result.turn, ProviderTurn::Final { .. }),
            "the declared model settles normally: {:?}",
            result.turn
        );
        let body = request_body(&observed[0]);
        let content = body["messages"][0]["content"]
            .as_array()
            .expect("multimodal content parts");
        assert!(
            content.iter().any(|part| part["type"] == "image_url"
                && part["image_url"]["url"]
                    .as_str()
                    .is_some_and(|url| url.starts_with("data:image/png;base64,"))),
            "the host-authorized image travels as a data-URL part: {body}"
        );
    }

    /// A legal local keyless model (`auth: none`) with declared tools
    /// still works — the capability check must not break the explicit
    /// local-no-credential contract (C11), and no auth header is sent.
    #[tokio::test]
    async fn f01_keyless_local_model_with_declared_tools_works() {
        let (endpoint, observed, stub) = capture_server().await;
        let cfg = ModelPlaneConfig::from_value(serde_json::json!({
            "providers": {"main": {
                "protocol": "openai-completions",
                "endpoint": endpoint,
                "auth": {"kind": "none"}
            }},
            "models": {"chat": {
                "provider": "main",
                "model": "local-model",
                "capabilities": {"tools": true}
            }}
        }))
        .expect("valid plane");
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider =
            GatewayedProvider::new(gateway, svc, SchemaBudget::default()).expect("provider");
        let input = ModelTurnInput::first_turn("read a file", one_tool_snapshot());
        let context = ctx();
        let call = ModelCallId::new("rr1_f01_keyless");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call, &input, &NullSink),
        )
        .await
        .expect("turn settles within budget");
        stub.abort();
        let observed = observed.lock().expect("observed").clone();
        assert_eq!(
            observed.len(),
            1,
            "the keyless local model dispatches with declared tools"
        );
        assert!(
            matches!(result.turn, ProviderTurn::Final { .. }),
            "the keyless local model settles normally: {:?}",
            result.turn
        );
        assert!(
            !observed[0].to_lowercase().contains("authorization:"),
            "a keyless route sends no credential header: {}",
            observed[0]
        );
        assert!(
            request_body(&observed[0])["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty()),
            "the tools declaration still travels"
        );
    }

    /// Same model id under two providers (C04): the capability declaration
    /// belongs to the ROUTE (provider+model), and the auxiliary slot is
    /// under the same pre-send rule as chat.
    #[tokio::test]
    async fn f01_same_model_id_across_providers_and_auxiliary_slot_follow_the_route() {
        let (alpha_endpoint, alpha_observed, alpha_stub) = capture_server().await;
        let (beta_endpoint, beta_observed, beta_stub) = capture_server().await;
        let cfg = two_provider_plane(
            &alpha_endpoint,
            "dummy-alpha-key",
            &beta_endpoint,
            "dummy-beta-key",
            serde_json::json!({"capabilities": {"tools": true}}),
            serde_json::json!({}),
        );
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider =
            GatewayedProvider::new(gateway, svc, SchemaBudget::default()).expect("provider");

        // Chat (alpha, DECLARED): the shared model id dispatches with tools.
        let chat_input = ModelTurnInput::first_turn("read a file", one_tool_snapshot());
        let context = ctx();
        let call = ModelCallId::new("rr1_f01_shared_chat");
        let chat_result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn_for_operation(
                &context,
                &call,
                ModelOperation::Chat,
                &chat_input,
                &NullSink,
            ),
        )
        .await
        .expect("chat settles within budget");

        // Title (beta, UNDECLARED, same model id): refused locally.
        let aux_input = ModelTurnInput::first_turn("title this", one_tool_snapshot());
        let aux_call = ModelCallId::new("rr1_f01_shared_title");
        let aux_result = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn_for_operation(
                &context,
                &aux_call,
                ModelOperation::Auxiliary(lingxi_kernel::model_exchange::AuxiliarySlot::Title),
                &aux_input,
                &NullSink,
            ),
        )
        .await
        .expect("auxiliary settles within budget");
        alpha_stub.abort();
        beta_stub.abort();

        let alpha_observed = alpha_observed.lock().expect("alpha").clone();
        let beta_observed = beta_observed.lock().expect("beta").clone();
        assert_eq!(
            alpha_observed.len(),
            1,
            "the DECLARING provider served the chat turn"
        );
        assert!(
            matches!(chat_result.turn, ProviderTurn::Final { .. }),
            "the declaring route settles normally: {:?}",
            chat_result.turn
        );
        assert_eq!(
            beta_observed.len(),
            0,
            "the undeclared provider was never contacted for the same model id"
        );
        let message = match aux_result.turn {
            ProviderTurn::Failed { error, .. } => error.message,
            other => panic!("expected a loud auxiliary refusal, got {other:?}"),
        };
        assert!(
            message.contains("beta") && message.contains("tools"),
            "the auxiliary refusal names the route and the capability: {message}"
        );
    }

    /// Hot reload (C05/F01): removing the declaration (or the binding
    /// entirely) refuses NEW calls — the running snapshot is authoritative.
    #[tokio::test]
    async fn f01_reload_removing_the_declaration_or_binding_refuses_new_calls() {
        let (endpoint, observed, stub) = capture_server().await;
        let v1 = chat_plane(
            &endpoint,
            "dummy-key",
            serde_json::json!({"capabilities": {"tools": true}}),
        );
        let svc = Arc::new(credentials(&v1));
        let gateway = Arc::new(ConfigModelGateway::from_validated(v1));
        let provider = GatewayedProvider::new(
            Arc::clone(&gateway),
            Arc::clone(&svc) as Arc<dyn ProviderCredentialPort>,
            SchemaBudget::default(),
        )
        .expect("provider");
        let context = ctx();
        let input = ModelTurnInput::first_turn("read a file", one_tool_snapshot());

        // Generation 1 dispatches (the declaration is present).
        let call1 = ModelCallId::new("rr1_f01_reload_v1");
        let result1 = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call1, &input, &NullSink),
        )
        .await
        .expect("v1 settles within budget");
        assert!(matches!(result1.turn, ProviderTurn::Final { .. }));

        // Reload: same provider/model/key, declaration REMOVED.
        let v2 = chat_plane(&endpoint, "dummy-key", serde_json::json!({}));
        svc.reload(&v2, gateway.upcoming_generation()).await;
        gateway.reload(v2);
        let call2 = ModelCallId::new("rr1_f01_reload_v2");
        let result2 = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call2, &input, &NullSink),
        )
        .await
        .expect("v2 settles within budget");
        assert!(
            matches!(result2.turn, ProviderTurn::Failed { .. }),
            "generation 2 refuses the tools turn: {:?}",
            result2.turn
        );

        // Reload: the chat binding REMOVED entirely — the loud
        // unconfigured state, never a share of another route.
        let v3 = ModelPlaneConfig::from_value(serde_json::json!({
            "providers": {"main": {
                "protocol": "openai-completions",
                "endpoint": endpoint,
                "auth": {"kind": "apiKey", "apiKey": "dummy-key"}
            }}
        }))
        .expect("valid v3");
        svc.reload(&v3, gateway.upcoming_generation()).await;
        gateway.reload(v3);
        let call3 = ModelCallId::new("rr1_f01_reload_v3");
        let result3 = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call3, &input, &NullSink),
        )
        .await
        .expect("v3 settles within budget");
        let message = match result3.turn {
            ProviderTurn::Failed { error, .. } => error.message,
            other => panic!("expected the loud unconfigured state, got {other:?}"),
        };
        assert!(
            message.contains("no model route configured"),
            "the removed binding is the explicit unconfigured state: {message}"
        );
        stub.abort();
        assert_eq!(
            observed.lock().expect("observed").len(),
            1,
            "only generation 1 ever hit the wire"
        );
    }

    // ── F02: the remaining generation-consistency matrix ──────────────────────

    /// Key-only rotation on the SAME endpoint: the in-flight call (its
    /// route is generation 1) never receives the new key — its 401 retry
    /// settles as a safe failure — while a FRESH call resolves generation
    /// 2 and dispatches with the new key normally.
    #[tokio::test]
    async fn f02_key_only_rotation_safe_failure_then_new_call_uses_new_key() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (first_seen_tx, first_seen_rx) = tokio::sync::oneshot::channel::<String>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let observed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let task = {
            let observed = Arc::clone(&observed);
            tokio::spawn(async move {
                // Request 1: old key, gated.
                let (mut first, _) = listener.accept().await.expect("conn 1");
                let request = read_raw_request(&mut first).await;
                observed.lock().expect("obs").push(request.clone());
                let _ = first_seen_tx.send(request);
                release_rx.await.expect("release");
                let _ = first
                    .write_all(
                        b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = first.shutdown().await;
                // Any later connection: the fresh generation-2 call (and,
                // under the defect, the stale retry) — answered normally.
                loop {
                    let Ok(Ok((mut conn, _))) =
                        tokio::time::timeout(Duration::from_millis(500), listener.accept()).await
                    else {
                        break;
                    };
                    let request = read_raw_request(&mut conn).await;
                    observed.lock().expect("obs").push(request);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{SSE_FINAL}",
                        SSE_FINAL.len()
                    );
                    let _ = conn.write_all(response.as_bytes()).await;
                    let _ = conn.shutdown().await;
                }
            })
        };
        let endpoint = format!("http://{addr}/v1");
        let cfg_v1 = chat_plane(&endpoint, "dummy-key-v1", serde_json::json!({}));
        let svc = Arc::new(credentials(&cfg_v1));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg_v1));
        let provider = Arc::new(
            GatewayedProvider::new(
                Arc::clone(&gateway),
                Arc::clone(&svc) as Arc<dyn ProviderCredentialPort>,
                SchemaBudget::default(),
            )
            .expect("provider"),
        );
        let stale_task = {
            let provider = Arc::clone(&provider);
            tokio::spawn(async move {
                let context = ctx();
                let call = ModelCallId::new("rr1_f02_stale_key_call");
                let input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
                provider.next_turn(&context, &call, &input, &NullSink).await
            })
        };
        let first = tokio::time::timeout(Duration::from_secs(5), first_seen_rx)
            .await
            .expect("request 1 arrives")
            .expect("channel");
        assert!(
            first.contains("Bearer dummy-key-v1"),
            "boundary: request 1 carries the old key"
        );
        // Key-only rotation, same endpoint.
        let cfg_v2 = chat_plane(&endpoint, "dummy-key-v2", serde_json::json!({}));
        svc.reload(&cfg_v2, gateway.upcoming_generation()).await;
        gateway.reload(cfg_v2);
        release_tx.send(()).expect("release 401");
        let stale = tokio::time::timeout(Duration::from_secs(5), stale_task)
            .await
            .expect("stale call settles")
            .expect("task");
        assert!(
            matches!(stale.turn, ProviderTurn::Failed { .. }),
            "the generation-1 call settles as a safe failure (its retry must not \
             carry the new key): {:?}",
            stale.turn
        );
        // The fresh call resolves generation 2 and uses the new key.
        let context = ctx();
        let call = ModelCallId::new("rr1_f02_fresh_key_call");
        let input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
        let fresh = tokio::time::timeout(
            Duration::from_secs(5),
            provider.next_turn(&context, &call, &input, &NullSink),
        )
        .await
        .expect("fresh call settles");
        let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
        let observed = observed.lock().expect("obs").clone();
        assert!(
            matches!(fresh.turn, ProviderTurn::Final { .. }),
            "the fresh call dispatches with the new key: {:?}",
            fresh.turn
        );
        assert_eq!(observed.len(), 2, "exactly two physical requests");
        assert!(
            observed[0].contains("Bearer dummy-key-v1")
                && observed[1].contains("Bearer dummy-key-v2"),
            "old key first, new key only on the fresh generation-2 call: {:?}",
            observed
                .iter()
                .map(|r| r
                    .lines()
                    .find(|l| l.to_lowercase().starts_with("authorization:")))
                .collect::<Vec<_>>()
        );
    }

    /// A reload whose seed is UNCHANGED (same key, different model AND a
    /// different endpoint — the "endpoint-only rotation" shape) keeps the
    /// cell: the material still serves routes of BOTH its own generation
    /// and the new one — no over-refusal, and no new material ever exists
    /// to leak (the C05 in-flight snapshot semantics stay intact).
    #[tokio::test]
    async fn f02_kept_seed_serves_both_generations() {
        let old = chat_plane_model(
            "https://old.example.invalid/v1",
            "dummy-stable-key",
            "model-v1",
            serde_json::json!({}),
        );
        let next = chat_plane_model(
            "https://new.example.invalid/v1",
            "dummy-stable-key",
            "model-v2",
            serde_json::json!({}),
        );
        let gateway = ConfigModelGateway::from_validated(old);
        let svc = credentials(&next);
        let route_v1 = gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("v1 route");
        assert_eq!(route_v1.endpoint, "https://old.example.invalid/v1");
        svc.reload(&next, gateway.upcoming_generation()).await;
        gateway.reload(next);
        let route_v2 = gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("v2 route");
        assert_eq!(route_v2.config_generation, 2);
        assert_eq!(
            route_v2.endpoint, "https://new.example.invalid/v1",
            "boundary: generation 2 really moved the endpoint"
        );
        // The unchanged material serves the NEW generation…
        assert!(
            matches!(
                svc.resolve(&route_v2).await,
                Ok(ApplicableAuth::Bearer(ref key)) if key == "dummy-stable-key"
            ),
            "an unchanged seed still serves the new generation (endpoint-only \
             rotation keeps the cell)"
        );
        // …and its own old generation (the in-flight call's snapshot) — the
        // still-authorized old snapshot, never a refusal and never a
        // cross-generation material swap.
        assert!(
            matches!(
                svc.resolve(&route_v1).await,
                Ok(ApplicableAuth::Bearer(ref key)) if key == "dummy-stable-key"
            ),
            "an unchanged seed still serves in-flight routes of its own generation"
        );
    }

    /// A reload that rotates provider `alpha` must not disturb a route to
    /// an UNRELATED provider whose seed did not change (per-provider
    /// epochs — no cross-provider over-refusal), while `alpha`'s old route
    /// refuses.
    #[tokio::test]
    async fn f02_unrelated_provider_reload_is_isolated_per_provider() {
        let alpha_ep = "https://alpha.example.invalid/v1";
        let beta_ep = "https://beta.example.invalid/v1";
        let v1 = two_provider_plane(
            alpha_ep,
            "dummy-alpha-key-v1",
            beta_ep,
            "dummy-beta-key",
            serde_json::json!({}),
            serde_json::json!({}),
        );
        // v2 rotates ONLY alpha's key (beta untouched).
        let v2 = two_provider_plane(
            alpha_ep,
            "dummy-alpha-key-v2",
            beta_ep,
            "dummy-beta-key",
            serde_json::json!({}),
            serde_json::json!({}),
        );
        let gateway = ConfigModelGateway::from_validated(v1.clone());
        let svc = credentials(&v1);
        let route_alpha = gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("alpha route");
        let route_beta = gateway
            .resolve_route(&ModelRouteRequest::for_operation(
                ModelOperation::Auxiliary(lingxi_kernel::model_exchange::AuxiliarySlot::Title),
            ))
            .expect("beta route");
        svc.reload(&v2, gateway.upcoming_generation()).await;
        gateway.reload(v2);
        assert!(
            matches!(
                svc.resolve(&route_alpha).await,
                Err(lingxi_adapters::models::credentials::CredentialError::StaleRoute { .. })
            ),
            "the rotated provider's old-generation route refuses"
        );
        assert!(
            matches!(
                svc.resolve(&route_beta).await,
                Ok(ApplicableAuth::Bearer(ref key)) if key == "dummy-beta-key"
            ),
            "the unrelated provider's route still resolves (kept cell)"
        );
    }

    /// A reload that REMOVES the provider: the old route's resolve is the
    /// explicit unconfigured refusal (never a cross-provider fallback).
    #[tokio::test]
    async fn f02_removed_provider_old_route_is_not_configured() {
        let old = chat_plane(
            "https://old.example.invalid/v1",
            "dummy-old-key",
            serde_json::json!({}),
        );
        let gateway = ConfigModelGateway::from_validated(old.clone());
        let svc = credentials(&old);
        let old_route = gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("route");
        let empty = ModelPlaneConfig::default();
        svc.reload(&empty, gateway.upcoming_generation()).await;
        gateway.reload(empty);
        let resolved = svc.resolve(&old_route).await;
        assert!(
            matches!(
                resolved,
                Err(lingxi_adapters::models::credentials::CredentialError::NotConfigured { .. })
            ),
            "the removed provider's route is the explicit unconfigured state: {resolved:?}"
        );
    }

    /// The dispatch bundle freeze (F02's compat leg): `resolve_dispatch`
    /// carries route + compat + capabilities of ONE generation — a reload
    /// between resolutions never mixes them.
    #[tokio::test]
    async fn f02_resolve_dispatch_freezes_compat_and_capabilities_with_the_route() {
        let endpoint = "https://frozen.example.invalid/v1";
        let v1 = chat_plane(
            endpoint,
            "dummy-key",
            serde_json::json!({
                "capabilities": {"tools": true},
                "compat": {"maxTokens": 4096}
            }),
        );
        let v2 = chat_plane(
            endpoint,
            "dummy-key",
            serde_json::json!({
                "capabilities": {"tools": false, "imageInput": true},
                "compat": {"maxTokens": 8192}
            }),
        );
        let gateway = ConfigModelGateway::from_validated(v1);
        let request = ModelRouteRequest::for_operation(ModelOperation::Chat);
        let d1 = gateway.resolve_dispatch(&request).expect("dispatch v1");
        gateway.reload(v2);
        let d2 = gateway.resolve_dispatch(&request).expect("dispatch v2");
        assert_eq!(d1.route.config_generation, 1);
        assert_eq!(d2.route.config_generation, 2);
        assert_eq!(d1.capabilities.tools, Some(true));
        assert_eq!(d1.capabilities.image_input, None);
        assert_eq!(d1.compat.as_ref().and_then(|c| c.max_tokens), Some(4096));
        assert_eq!(d2.capabilities.tools, Some(false));
        assert_eq!(d2.capabilities.image_input, Some(true));
        assert_eq!(d2.compat.as_ref().and_then(|c| c.max_tokens), Some(8192));
        // The kernel-trait projection stays the bare route (unchanged API).
        assert_eq!(
            gateway
                .resolve_route(&request)
                .expect("route")
                .config_generation,
            2
        );
    }
}

// ═══ R05 RR1 WP-T01 R2: F29 — the combined scenario, pinned as a test ═══════
//
// F02's management-surface behavior was verified by the R1 independent
// reviewer through a session-scoped isolated probe; F29 pins that exact
// combination into the permanent suite. The unit matrix alone cannot catch
// a rewiring of `management.rs` — a lost `upcoming_generation` stamp on
// the credential re-seed, the two reloads swapped back, or the state's
// gateway forking away from the production provider's — every unit stays
// green while the production wiring regresses (the GATE-N08 "production
// wiring removed" shape). This test reloads through the REAL management
// surface (composition-root boot → `lingxi_service::run` →
// `POST /lingxi/v1/models/reload`) while a run is parked IN FLIGHT at
// endpoint A's gated 401, exactly like `c05_…swaps_atomically` meets
// `rr1_f01_f02::f02_reload_during_401_…`.
//
// Invariants under assertion:
//   • endpoint A sees exactly ONE request — the old key's — and NEVER any
//     byte of the new key (any new-key request on A is the leak, red);
//   • the reload publishes generation 2;
//   • the parked generation-1 model call settles with NO fabricated delta;
//   • any dispatch to B is a BRAND-NEW model_call_id serving the gen-2
//     model (an explicit new identity re-route, never an in-flight
//     rewrite of the parked call);
//   • after the reload a fresh run completes `completed.with_final`
//     through B with the new key.
mod rr1_f29 {
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use lingxi_kernel::model_exchange::ModelGatewayPort as _;
    use lingxi_protocol::KnownEventPayload;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use super::{
        boot_with_config, execute_on, final_message_text, http_post, known_payloads, rewrite_plane,
        run_row, served_identities, teardown,
    };

    const OLD_KEY: &str = "dummy-real-entry-old-key";
    const NEW_KEY: &str = "dummy-real-entry-new-key";

    /// The only legitimate completion: generation 2's answer via endpoint B.
    const SSE_FINAL_B: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"generation 2 answered via B\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

    /// What endpoint A answers the DEFECT's leaked 401-retry with: a
    /// completion read through the WRONG endpoint — it must never become
    /// any run's final. Under the fix A never receives that retry at all.
    const SSE_A_POISONED: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"poisoned completion via A\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

    /// A keyed single-provider plane (the F02 shape: static material that
    /// the reload ROTATES — endpoint AND key together). `tools` is
    /// declared because the booted composition root registers the core
    /// file tools, so a chat turn genuinely needs the capability.
    fn keyed_plane_json(endpoint: &str, key: &str, model: &str) -> String {
        format!(
            r#""providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": "{endpoint}",
                    "auth": {{"kind": "apiKey", "apiKey": "{key}"}}
                }}
            }},
            "models": {{"chat": {{"provider": "main", "model": "{model}",
                "capabilities": {{"tools": true}}}}}}"#
        )
    }

    /// Reads one full HTTP/1.1 request (headers + content-length body).
    async fn read_raw_request(conn: &mut tokio::net::TcpStream) -> String {
        let mut raw = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let read = tokio::time::timeout(Duration::from_secs(10), conn.read(&mut chunk))
                .await
                .expect("read stalled")
                .expect("read");
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
            if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse().ok())
                    })
                    .unwrap_or(0);
                if raw.len() >= end + 4 + len {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&raw).into_owned()
    }

    /// Answers one request with a canned 200 SSE body.
    async fn sse_200(conn: &mut tokio::net::TcpStream, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = conn.write_all(response.as_bytes()).await;
        let _ = conn.shutdown().await;
    }

    fn authorization_headers(requests: &[String]) -> Vec<&str> {
        requests
            .iter()
            .map(|r| {
                r.lines()
                    .find(|l| l.to_lowercase().starts_with("authorization:"))
                    .unwrap_or("<missing authorization>")
            })
            .collect()
    }

    #[tokio::test]
    async fn f29_management_reload_during_inflight_401_never_leaks_the_new_key() {
        // Endpoint A: request 1 (the old key) parks at a GATED 401; any
        // LATER connection toward A is the defect's leak — captured and
        // answered (the poisoned SSE) so an old-red run observes the leak
        // complete instead of hanging.
        let listener_a = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind A");
        let addr_a = listener_a.local_addr().expect("addr A");
        // Endpoint B: generation 2's endpoint — records everything and
        // answers normally (run until aborted; no accept-timeout window
        // could otherwise close it between the two legitimate dispatches).
        let listener_b = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind B");
        let addr_b = listener_b.local_addr().expect("addr B");
        let (first_seen_tx, first_seen_rx) = tokio::sync::oneshot::channel::<String>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let to_a: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let to_b: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let task_a = {
            let to_a = Arc::clone(&to_a);
            tokio::spawn(async move {
                let (mut first, _) = listener_a.accept().await.expect("A conn 1");
                let request = read_raw_request(&mut first).await;
                to_a.lock().expect("to_a").push(request.clone());
                let _ = first_seen_tx.send(request);
                release_rx.await.expect("release");
                let _ = first
                    .write_all(
                        b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = first.shutdown().await;
                // Bounded extra accepts: under the defect the 401 retry
                // comes back HERE carrying the new key — captured so the
                // leak is observable, then answered so the poisoned path
                // completes and the old-red assertions see real facts.
                for _ in 0..3 {
                    let Ok(Ok((mut conn, _))) =
                        tokio::time::timeout(Duration::from_millis(500), listener_a.accept()).await
                    else {
                        break;
                    };
                    let request = read_raw_request(&mut conn).await;
                    to_a.lock().expect("to_a").push(request);
                    sse_200(&mut conn, SSE_A_POISONED).await;
                }
            })
        };
        let task_b = {
            let to_b = Arc::clone(&to_b);
            tokio::spawn(async move {
                loop {
                    let Ok((mut conn, _)) = listener_b.accept().await else {
                        break;
                    };
                    let request = read_raw_request(&mut conn).await;
                    to_b.lock().expect("to_b").push(request);
                    sse_200(&mut conn, SSE_FINAL_B).await;
                }
            })
        };

        // The REAL composition root on generation 1: provider main →
        // endpoint A with the OLD key, chat = stub-model-v1.
        let boot = {
            let plane_json =
                keyed_plane_json(&format!("http://{addr_a}/v1"), OLD_KEY, "stub-model-v1");
            boot_with_config("f29", &plane_json).await
        };
        // The REAL serving surface — the management route this test
        // reloads through lives exactly here, never in a test double.
        let token = boot.state.auth().local_token();
        let state_for_server = boot.state.clone();
        let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let (ready, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
        let serve_task = tokio::spawn(async move {
            lingxi_service::run(
                state_for_server,
                async {
                    let _ = stop_rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        let addr = tokio::time::timeout(Duration::from_secs(10), ready_rx)
            .await
            .expect("service READY within budget")
            .expect("service READY");

        // Run 1 dispatches against generation 1 and parks inside A's 401.
        let run_state = boot.state.clone();
        let run1 = tokio::spawn(async move {
            execute_on(&run_state, "sess_local_alpha", "park me at A's 401").await
        });
        let first = tokio::time::timeout(Duration::from_secs(5), first_seen_rx)
            .await
            .expect("request 1 reaches endpoint A")
            .expect("first-seen channel");
        assert!(
            first.contains(&format!("Bearer {OLD_KEY}")),
            "boundary: endpoint A received the OLD key first — the call was genuinely \
             in flight at the old endpoint before any reload: {first}"
        );

        // While run 1 is PARKED: rewrite --config to endpoint B + the NEW
        // key and reload through the REAL management route (the source is
        // re-read by the handler itself — no test-side plane install).
        rewrite_plane(
            &boot,
            &keyed_plane_json(&format!("http://{addr_b}/v1"), NEW_KEY, "stub-model-v2"),
        );
        let (status, body) = http_post(addr, "/lingxi/v1/models/reload", &token).await;
        assert_eq!(status, 200, "reload accepted: {body}");
        assert_eq!(body["ok"], true);
        assert_eq!(body["generation"], 2, "the reload publishes generation 2");
        assert_eq!(
            boot.state
                .model_gateway()
                .expect("gateway wired")
                .config_generation(),
            2,
            "the running snapshot is generation 2"
        );

        // Release the 401: the parked generation-1 call must settle as a
        // safe failure WITHOUT the new key ever leaving toward A (its own
        // 401 retry is refused as a stale route), and the run's bounded
        // retry re-routes as a NEW model-call identity against the
        // CURRENT generation — through B, with the new key.
        release_tx.send(()).expect("release 401");
        let run1_id = tokio::time::timeout(Duration::from_secs(20), run1)
            .await
            .expect("run 1 settles within budget (bounded backoff + B round trip)")
            .expect("run 1 task");
        let (status, reason) = run_row(&boot.state, &run1_id).await;
        assert_eq!(status, "completed");
        assert_eq!(
            reason.as_deref(),
            Some("completed.with_final"),
            "run 1 completes through the generation-2 re-route"
        );

        // After the reload a FRESH run serves through B with the new key.
        let run2_id = execute_on(&boot.state, "sess_local_alpha", "after the reload").await;
        let (status, reason) = run_row(&boot.state, &run2_id).await;
        assert_eq!(status, "completed");
        assert_eq!(reason.as_deref(), Some("completed.with_final"));
        assert_eq!(
            served_identities(&boot.state, "sess_local_alpha", &run2_id).await,
            vec![("main".to_string(), "stub-model-v2".to_string())],
            "the post-reload run serves the generation-2 model"
        );

        // Every live actor settles before the wire/durable assertions.
        let _ = stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), serve_task).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), task_a).await;
        task_b.abort();

        // ── Invariant 1: endpoint A saw exactly the ONE old-key request —
        // and NEVER the new key (checked first: the leak is the headline).
        let to_a = to_a.lock().expect("to_a").clone();
        for (index, request) in to_a.iter().enumerate() {
            assert!(
                !request.contains(NEW_KEY),
                "the OLD endpoint A captured the NEW endpoint's key on request #{index} — \
                 the management reload during an in-flight 401 mixed credential material \
                 across config generations: {}",
                authorization_headers(&to_a)[index]
            );
        }
        assert_eq!(
            to_a.len(),
            1,
            "endpoint A must see exactly the ONE parked old-key request; anything more \
             is a stale-route retry: {:?}",
            authorization_headers(&to_a)
        );
        assert!(
            to_a[0].contains(&format!("Bearer {OLD_KEY}")),
            "the one request A saw carried the old key: {}",
            authorization_headers(&to_a)[0]
        );

        // ── Invariant 2: run 1 settled as exactly two model calls — the
        // parked generation-1 call, then the bounded retry as a BRAND-NEW
        // model_call identity serving the generation-2 model (an explicit
        // new identity re-route, never an in-flight rewrite).
        let payloads = known_payloads(&boot.state, "sess_local_alpha", &run1_id).await;
        let calls: Vec<(String, String, String)> = payloads
            .iter()
            .filter_map(|p| match p {
                KnownEventPayload::ModelCallStarted(p) => Some((
                    p.model_call_id.as_str().to_string(),
                    p.provider.clone(),
                    p.model.clone(),
                )),
                _ => None,
            })
            .collect();
        let mc1 = format!("{run1_id}-mc0001");
        let mc2 = format!("{run1_id}-mc0002");
        assert_ne!(mc1, mc2, "sanity: the retry identity is a new id");
        assert_eq!(
            calls,
            vec![
                (mc1.clone(), "main".to_string(), "stub-model-v1".to_string()),
                (mc2.clone(), "main".to_string(), "stub-model-v2".to_string()),
            ],
            "run 1's model calls: the parked gen-1 call (dispatched against \
             stub-model-v1) settles safely, and only a NEW model-call identity \
             re-routes onto the gen-2 model"
        );

        // ── Invariant 3: the parked generation-1 model call produced NO
        // fabricated delta — the 401 settles from the STATUS with nothing
        // on the stream; every durable delta of run 1 belongs to mc2.
        let delta_calls: Vec<&str> = payloads
            .iter()
            .filter_map(|p| match p {
                KnownEventPayload::ModelCallDelta(p) => Some(p.model_call_id.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !delta_calls.contains(&mc1.as_str()),
            "the parked generation-1 model call fabricated deltas — its 401 must \
             settle as a safe failure with nothing on the stream: {delta_calls:?}"
        );
        assert!(
            !delta_calls.is_empty() && delta_calls.iter().all(|id| *id == mc2),
            "every durable delta of run 1 belongs to the NEW model-call identity: \
             {delta_calls:?}"
        );

        // Run 1's final came through B's generation-2 answer — never the
        // poisoned completion the defect's leaked retry would read off A.
        let final1 = final_message_text(&boot.state, &run1_id).await;
        assert!(
            final1.contains("generation 2 answered via B"),
            "run 1 committed generation 2's answer through B: {final1}"
        );
        assert!(
            !final1.contains("poisoned completion via A"),
            "a completion read through endpoint A must never become a final: {final1}"
        );
        let final2 = final_message_text(&boot.state, &run2_id).await;
        assert!(
            final2.contains("generation 2 answered via B"),
            "the post-reload run committed generation 2's answer: {final2}"
        );

        // ── Invariant 4: endpoint B carried exactly the two generation-2
        // dispatches (run 1's retry + the post-reload run), every one with
        // the NEW key — and never the old one.
        let to_b = to_b.lock().expect("to_b").clone();
        assert_eq!(
            to_b.len(),
            2,
            "endpoint B sees exactly the two generation-2 dispatches: {:?}",
            authorization_headers(&to_b)
        );
        for (index, request) in to_b.iter().enumerate() {
            assert!(
                request.contains(&format!("Bearer {NEW_KEY}")),
                "B request #{index} carries the NEW key: {:?}",
                authorization_headers(&to_b)
            );
            assert!(
                !request.contains(OLD_KEY),
                "B request #{index} must never carry the old key: {:?}",
                authorization_headers(&to_b)
            );
            let body = request
                .split_once("\r\n\r\n")
                .map(|(_, body)| body.trim())
                .unwrap_or_default();
            let body: serde_json::Value = serde_json::from_str(body)
                .unwrap_or_else(|err| panic!("B request #{index} body is JSON: {err}"));
            assert_eq!(
                body["model"], "stub-model-v2",
                "B request #{index} serves the generation-2 model"
            );
        }

        teardown(&boot.state, &boot.home).await;
        let _ = std::fs::remove_dir_all(boot.workspace);
        let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
    }
}
