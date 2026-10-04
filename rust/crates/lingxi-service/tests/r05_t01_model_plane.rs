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
        "models": {{"chat": {{"provider": "main", "model": "stub-model"}}}}"#,
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
        "models": {{"chat": {{"provider": "b", "model": "shared-model"}}}}"#,
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
        "models": {{"chat": {{"provider": "main", "model": "{model}"}}}}"#
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
