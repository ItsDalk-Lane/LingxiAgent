//! R05-T03 integration evidence: the five real chat protocol adapters
//! through the REAL production wiring (strict `--config` → gateway →
//! credential service → adapter → loopback HTTP stub → real file tools →
//! durable events), pinning C01-C12 at the service boundary.
//!
//! Double boundary: the ONLY double is the stub HTTP server (the outside
//! world at the far end of the wire). Every in-process link is production
//! code.
//!
//! Coverage map (the adapter-level golden suite owns the byte-exact wire
//! pins; this suite owns the SERVICE-level facts; C-numbers are the task
//! book's appendix-B R05-T03 checkpoints):
//! - C01/C02: the same model id under an anthropic and a google provider
//!   never crosses endpoints or credentials; the anthropic round-trip pins
//!   the provider call id as correlation data only.
//! - C02: anthropic tool round-trip with the REAL file tool and its real
//!   result on the wire (provider call id rides verbatim both ways).
//! - C03: google parallel function calls + functionResponse pairing.
//! - C04: the SAME provider call id in two sessions stays isolated — no
//!   dedup, no overwrite, no cross-session leak (REV-T03 R01 F-01 test,
//!   ported from the reviewer's scratch).
//! - C05: a runtime random nonce (generated AFTER the stub script is fixed)
//!   rides the next request byte-exact, and follows changes across legs
//!   (REV-T03 R01 F-02 test, ported from the reviewer's scratch).
//! - C06: an openai-completions mixed content-parts response persists EVERY
//!   block in order.
//! - C07 (credential mapping half): `authHeader` goes verbatim, `none`
//!   sends nothing. (The error-classification half is pinned by the
//!   adapter-level error goldens over real HTTP.)
//! - C08: a read-only policy refusal is never swallowed — it rides the next
//!   wire request as a structured `tool error [forbidden]` failure and the
//!   refused write never executes.
//! - C12: a retryable mid-run failure retries with the CONFIRMED exchange
//!   (read → write → 500 → retry) — no rebuild, no re-execution.
//! - codex: the SSE-only family end to end (headers, store:false,
//!   stream:true, terminal-event aggregation).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use lingxi_protocol::{EventPayload, KnownEventPayload, UsageRecord};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── the loopback stub provider (the far end of the wire) ────────────────────

/// One scripted stub response (status + content-type + verbatim body).
struct StubResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl StubResponse {
    fn sse(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/event-stream",
            body,
        }
    }

    fn error(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json",
            body,
        }
    }
}

#[derive(Debug)]
struct RecordedRequest {
    path: String,
    /// Header names lowercased (wire casing is a transport detail).
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
    /// Starts a raw-TCP HTTP/1.1 stub. One request per connection (every
    /// response says `Connection: close`); scripted responses are popped in
    /// order; an exhausted script is a LOUD 500, never an improvised reply.
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
                let response = match next {
                    Some(scripted) => scripted,
                    None => StubResponse::error(
                        500,
                        r#"{"error":{"message":"stub script exhausted"}}"#.to_string(),
                    ),
                };
                let reason = match response.status {
                    200 => "OK",
                    400 => "Bad Request",
                    401 => "Unauthorized",
                    429 => "Too Many Requests",
                    500 => "Internal Server Error",
                    other => panic!("stub: unscripted status {other}"),
                };
                let payload = response.body;
                let raw = format!(
                    "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    response.status,
                    response.content_type,
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

    /// The bare origin (`http://addr`) — families join their own paths.
    fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The openai-family base (`{origin}/v1`, the production config shape).
    fn v1(&self) -> String {
        format!("{}/v1", self.origin())
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
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task).await;
    }
}

/// Reads one HTTP/1.1 request (request line + all headers + content-length
/// body). Malformed input panics — the stub is test equipment.
async fn read_request(socket: &mut tokio::net::TcpStream) -> RecordedRequest {
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

// ── harness (the R05-T01 boot pattern: the REAL composition root) ───────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t03-{tag}-{}-{}",
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
}

/// Boots the REAL composition-root wiring exactly like the binary does
/// (strict config file → resolve_model_plane → ServiceDeps → bootstrap).
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
    }
}

async fn execute_on(state: &ServiceState, session: &str, input: &str) -> String {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            session,
            input,
            1_790_409_600_000,
        )
        .await
        .expect("execute accepted")
        .run_id
}

async fn execute(state: &ServiceState, input: &str) -> String {
    execute_on(state, "sess_local_alpha", input).await
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
async fn known_payloads_for(
    state: &ServiceState,
    session: &str,
    run_id: &str,
) -> Vec<KnownEventPayload> {
    use lingxi_kernel::ports::EventStorePort as _;
    state
        .storage()
        .stream_events_after(session, lingxi_protocol::Seq::new(0), 10_000)
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

async fn known_payloads(state: &ServiceState, run_id: &str) -> Vec<KnownEventPayload> {
    known_payloads_for(state, "sess_local_alpha", run_id).await
}

/// A runtime-generated nonce the stub cannot know: process id + nanos + a
/// counter. Generated AFTER the stub script is fixed, per invocation
/// (REV-T03 R01 F-02 strong form).
fn runtime_nonce(tag: &str) -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!(
        "NONCE-{tag}-{}-{nanos}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

/// The (provider, model) pairs persisted by this run's model_call_started
/// events — the durable identity proof.
async fn served_identities(state: &ServiceState, run_id: &str) -> Vec<(String, String)> {
    known_payloads(state, run_id)
        .await
        .iter()
        .filter_map(|p| match p {
            KnownEventPayload::ModelCallStarted(p) => Some((p.provider.clone(), p.model.clone())),
            _ => None,
        })
        .collect()
}

/// The per-call usage the run persisted, in call order.
async fn persisted_usages(state: &ServiceState, run_id: &str) -> Vec<Option<UsageRecord>> {
    known_payloads(state, run_id)
        .await
        .iter()
        .filter_map(|p| match p {
            KnownEventPayload::ModelCallCompleted(p) => Some(p.usage.clone()),
            _ => None,
        })
        .collect()
}

async fn final_message_content(state: &ServiceState, run_id: &str) -> String {
    state
        .storage()
        .query_one_text(
            "SELECT content_json FROM messages WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("final message query")
        .expect("final message row")
}

async fn teardown(state: &ServiceState, home: &Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

async fn teardown_boot(boot: &PlaneBoot) {
    teardown(&boot.state, &boot.home).await;
    let _ = std::fs::remove_dir_all(&boot.workspace);
    let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
}

// ── canned provider responses (R05-T04 D3: every chat family streams in
//    production — the far end of the wire speaks SSE) ───────────────────────

/// One anthropic SSE body: the wire carries the event field AND the typed
/// data frame (they agree, as the real service sends them).
fn anthropic_sse(frames: &[serde_json::Value]) -> StubResponse {
    let mut body = String::new();
    for frame in frames {
        let ty = frame["type"].as_str().expect("typed anthropic frame");
        body.push_str(&format!("event: {ty}\ndata: {frame}\n\n"));
    }
    StubResponse::sse(body)
}

/// One openai-completions SSE body (data frames + the [DONE] sentinel).
fn openai_sse(frames: &[serde_json::Value]) -> StubResponse {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse::sse(body)
}

/// One google SSE body (data frames; the terminal frame carries the
/// candidate finishReason / promptFeedback).
fn google_sse(frames: &[serde_json::Value]) -> StubResponse {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    StubResponse::sse(body)
}

fn anthropic_tool_use(call_id: &str, name: &str, input: serde_json::Value) -> StubResponse {
    anthropic_sse(&[
        serde_json::json!({"type":"message_start","message":{"id":"msg_stub","model":"the-stub-lies","usage":{"input_tokens":15}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":call_id,"name":name}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":input.to_string()}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":6}}),
        serde_json::json!({"type":"message_stop"}),
    ])
}

fn anthropic_final(text: &str) -> StubResponse {
    anthropic_sse(&[
        serde_json::json!({"type":"message_start","message":{"id":"msg_stub","model":"the-stub-lies","usage":{"input_tokens":40}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}),
        serde_json::json!({"type":"message_stop"}),
    ])
}

fn google_final(text: &str) -> StubResponse {
    google_sse(&[serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 30, "candidatesTokenCount": 8}
    })])
}

fn openai_final(text: &str) -> StubResponse {
    openai_sse(&[
        serde_json::json!({"id":"chatcmpl-stub","model":"the-stub-lies","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-stub","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        serde_json::json!({"id":"chatcmpl-stub","choices":[],"usage":{"prompt_tokens":21,"completion_tokens":9}}),
    ])
}

/// One openai-completions tool-call turn (fragments arrive in the streaming
/// shape: id+name first, the arguments as one fragment, then the finish).
fn openai_tool_call(
    id: &str,
    name: &str,
    arguments: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> StubResponse {
    openai_sse(&[
        serde_json::json!({"id":format!("chatcmpl-{id}"),"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":""}}]},"finish_reason":null}]}),
        serde_json::json!({"id":format!("chatcmpl-{id}"),"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":arguments}}]},"finish_reason":null}]}),
        serde_json::json!({"id":format!("chatcmpl-{id}"),"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        serde_json::json!({"id":format!("chatcmpl-{id}"),"choices":[],"usage":{"prompt_tokens":prompt_tokens,"completion_tokens":completion_tokens}}),
    ])
}

// ── C01/C04: same model id, two families, no crossing ───────────────────────

#[tokio::test]
async fn c01_c04_same_model_id_two_families_never_cross_wire_or_credentials() {
    // Leg 1 — chat pinned to the ANTHROPIC provider serving "shared-model".
    let stub_a = StubServer::start(vec![anthropic_final("from anthropic")]).await;
    let stub_g = StubServer::start(vec![]).await; // empty: any contact is loud
    let plane_json = format!(
        r#""providers": {{
            "anthropic_svc": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-ant-svc"}}
            }},
            "google_svc": {{
                "protocol": "google-generative-ai",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-goog-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "anthropic_svc", "model": "shared-model"}}}}"#,
        stub_a.origin(),
        stub_g.origin()
    );
    let boot = boot_with_config("c01a", &plane_json).await;
    let run_id = execute(&boot.state, "hi").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        stub_g.hits(),
        0,
        "the google provider must never be contacted"
    );
    assert_eq!(stub_a.hits(), 1);
    let request = &stub_a.requests()[0];
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(request.header("x-api-key"), Some("sk-ant-svc"));
    assert_eq!(
        request.header("authorization"),
        None,
        "anthropic never sends Authorization"
    );
    assert_eq!(request.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(request.body["model"], "shared-model");
    assert_eq!(request.body["stream"], true);
    assert!(
        request.body.get("system").is_none(),
        "no host system prompt resolved → the key is absent entirely"
    );
    assert_eq!(
        served_identities(&boot.state, &run_id).await,
        vec![("anthropic_svc".to_string(), "shared-model".to_string())]
    );
    stub_a.stop().await;
    stub_g.stop().await;
    teardown_boot(&boot).await;

    // Leg 2 — the SAME model id pinned to the GOOGLE provider.
    let stub_a = StubServer::start(vec![]).await;
    let stub_g = StubServer::start(vec![google_final("from google")]).await;
    let plane_json = format!(
        r#""providers": {{
            "anthropic_svc": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-ant-svc"}}
            }},
            "google_svc": {{
                "protocol": "google-generative-ai",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-goog-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "google_svc", "model": "shared-model"}}}}"#,
        stub_a.origin(),
        stub_g.origin()
    );
    let boot = boot_with_config("c01b", &plane_json).await;
    let run_id = execute(&boot.state, "hi").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        stub_a.hits(),
        0,
        "the anthropic provider must never be contacted"
    );
    assert_eq!(stub_g.hits(), 1);
    let request = &stub_g.requests()[0];
    assert_eq!(
        request.path,
        "/models/shared-model:streamGenerateContent?alt=sse"
    );
    assert_eq!(request.header("x-goog-api-key"), Some("sk-goog-svc"));
    assert_eq!(
        request.header("authorization"),
        None,
        "google never sends Authorization"
    );
    assert!(
        request.body.get("model").is_none(),
        "the model rides the URL, not the body"
    );
    assert_eq!(
        served_identities(&boot.state, &run_id).await,
        vec![("google_svc".to_string(), "shared-model".to_string())]
    );
    assert_eq!(
        persisted_usages(&boot.state, &run_id).await,
        vec![Some(UsageRecord {
            input_tokens: 30,
            output_tokens: 8
        })]
    );
    stub_a.stop().await;
    stub_g.stop().await;
    teardown_boot(&boot).await;
}

// ── C02: anthropic tool round-trip over the real wire ───────────────────────

const FILE_BODY: &str = "R05-T03 real file body: 真实内容-77\n";

#[tokio::test]
async fn c02_anthropic_tool_roundtrip_with_the_real_file_tool() {
    let stub = StubServer::start(vec![
        anthropic_tool_use(
            "toolu_svc_1",
            "read",
            serde_json::json!({"path": "note.txt"}),
        ),
        anthropic_final("anthropic final 完成"),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "anthropic_svc": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-ant-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "anthropic_svc", "model": "claude-svc"}}}}"#,
        stub.origin()
    );
    let boot = boot_with_config("c02", &plane_json).await;
    std::fs::write(boot.workspace.join("note.txt"), FILE_BODY).expect("seed workspace file");

    let run_id = execute(&boot.state, "please read note.txt").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "exactly the two protocol turns");
    // Turn 1: the family's tool declaration shape.
    let tools = requests[0].body["tools"].as_array().expect("tools");
    assert!(
        tools
            .iter()
            .any(|t| t["name"] == "read"
                && t["input_schema"]["properties"]["path"]["type"] == "string"),
        "the read declaration rode the request: {tools:?}"
    );
    assert_eq!(requests[0].body["messages"].as_array().expect("m").len(), 1);
    // Turn 2: assistant content blocks + the merged tool_result user message.
    let messages = requests[1].body["messages"].as_array().expect("messages");
    assert_eq!(
        messages.len(),
        3,
        "user + assistant(tool_use) + user(tool_result)"
    );
    let assistant_blocks = messages[1]["content"].as_array().expect("blocks");
    assert_eq!(
        assistant_blocks[0],
        serde_json::json!({"type": "tool_use", "id": "toolu_svc_1", "name": "read", "input": {"path": "note.txt"}})
    );
    let result_blocks = messages[2]["content"].as_array().expect("result blocks");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(
        result_blocks[0],
        serde_json::json!({
            "type": "tool_result",
            "tool_use_id": "toolu_svc_1",
            "content": FILE_BODY,
            "is_error": false,
            // R05-T05 consumer sync: the anthropic compat patch marks the
            // trailing cacheable user message with ephemeral cache_control
            // (the incumbent's exact behavior, golden-pinned by the T05
            // compat suite).
            "cache_control": {"type": "ephemeral"}
        }),
        "the tool result on the wire is the REAL file content, honestly flagged"
    );

    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_started").await,
        2
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_completed").await,
        1
    );
    assert_eq!(
        persisted_usages(&boot.state, &run_id).await,
        vec![
            Some(UsageRecord {
                input_tokens: 15,
                output_tokens: 6
            }),
            Some(UsageRecord {
                input_tokens: 40,
                output_tokens: 9
            })
        ]
    );
    assert_eq!(
        served_identities(&boot.state, &run_id).await,
        vec![
            ("anthropic_svc".to_string(), "claude-svc".to_string()),
            ("anthropic_svc".to_string(), "claude-svc".to_string())
        ]
    );
    let final_content = final_message_content(&boot.state, &run_id).await;
    assert!(
        final_content.contains("anthropic final 完成"),
        "{final_content}"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C03: google parallel function calls + functionResponse pairing ──────────

#[tokio::test]
async fn c03_google_parallel_calls_pair_function_responses_by_exchange_mapping() {
    let parallel = google_sse(&[serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [
                {"functionCall": {"name": "read", "args": {"path": "note-a.txt"}, "id": "fc-a"}},
                {"functionCall": {"name": "read", "args": {"path": "note-b.txt"}, "id": "fc-b"}}
            ]},
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 20, "candidatesTokenCount": 5}
    })]);
    let stub = StubServer::start(vec![parallel, google_final("both read 完成")]).await;
    let plane_json = format!(
        r#""providers": {{
            "google_svc": {{
                "protocol": "google-generative-ai",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-goog-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "google_svc", "model": "gemini-svc"}}}}"#,
        stub.origin()
    );
    let boot = boot_with_config("c03", &plane_json).await;
    std::fs::write(boot.workspace.join("note-a.txt"), "body A 内容").expect("seed a");
    std::fs::write(boot.workspace.join("note-b.txt"), "body B 内容").expect("seed b");

    let run_id = execute(&boot.state, "read both notes").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    let contents = requests[1].body["contents"].as_array().expect("contents");
    // user(submission) + model(2 functionCall parts) + user(fr a) + user(fr b)
    assert_eq!(contents.len(), 4, "{contents:?}");
    let model_parts = contents[1]["parts"].as_array().expect("model parts");
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(
        model_parts[0],
        serde_json::json!({"functionCall": {"name": "read", "args": {"path": "note-a.txt"}, "id": "fc-a"}})
    );
    assert_eq!(
        model_parts[1],
        serde_json::json!({"functionCall": {"name": "read", "args": {"path": "note-b.txt"}, "id": "fc-b"}})
    );
    // Each functionResponse pairs by the EXCHANGE's own call→name mapping
    // (never a guessed name) and carries the REAL file content.
    let fr_a = &contents[2]["parts"][0]["functionResponse"];
    assert_eq!(contents[2]["role"], "user");
    assert_eq!(fr_a["name"], "read");
    assert_eq!(fr_a["id"], "fc-a");
    assert_eq!(fr_a["response"]["status"], "succeeded");
    assert_eq!(fr_a["response"]["content"], "body A 内容");
    let fr_b = &contents[3]["parts"][0]["functionResponse"];
    assert_eq!(fr_b["id"], "fc-b");
    assert_eq!(fr_b["response"]["content"], "body B 内容");

    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_started").await,
        2
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_completed").await,
        2
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C04: the same provider call id in two sessions stays isolated ────────────
// Ported from the REV-T03 R01 scratch (F-01).

#[tokio::test]
async fn c04_same_provider_call_id_in_two_sessions_stays_isolated() {
    let stub = StubServer::start(vec![
        // session 1: same external id, file one.txt
        anthropic_tool_use(
            "toolu_SHARED",
            "read",
            serde_json::json!({"path": "one.txt"}),
        ),
        anthropic_final("session one final"),
        // session 2: the SAME external id, file two.txt
        anthropic_tool_use(
            "toolu_SHARED",
            "read",
            serde_json::json!({"path": "two.txt"}),
        ),
        anthropic_final("session two final"),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "anthropic_svc": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-ant-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "anthropic_svc", "model": "claude-svc"}}}}"#,
        stub.origin()
    );
    let boot = boot_with_config("c04", &plane_json).await;
    std::fs::write(boot.workspace.join("one.txt"), "session-one-body-Ⅰ").expect("seed one");
    std::fs::write(boot.workspace.join("two.txt"), "session-two-body-Ⅱ").expect("seed two");

    let run_1 = execute_on(&boot.state, "sess_local_alpha", "read one.txt").await;
    let run_2 = execute_on(&boot.state, "sess_local_beta", "read two.txt").await;
    assert_ne!(run_1, run_2);
    assert_eq!(run_row(&boot.state, &run_1).await.0, "completed");
    assert_eq!(run_row(&boot.state, &run_2).await.0, "completed");

    let requests = stub.requests();
    assert_eq!(requests.len(), 4, "two turns per session");
    // Session 1's continuation carries session 1's REAL body under the shared id.
    let s1_blocks = requests[1].body["messages"][2]["content"]
        .as_array()
        .expect("s1 blocks");
    assert_eq!(s1_blocks[0]["tool_use_id"], "toolu_SHARED");
    assert_eq!(s1_blocks[0]["content"], "session-one-body-Ⅰ");
    // Session 2's continuation carries session 2's REAL body under the SAME id.
    let s2_blocks = requests[3].body["messages"][2]["content"]
        .as_array()
        .expect("s2 blocks");
    assert_eq!(s2_blocks[0]["tool_use_id"], "toolu_SHARED");
    assert_eq!(s2_blocks[0]["content"], "session-two-body-Ⅱ");
    // No dedup across sessions: each session's journal holds its own completed call.
    for (session, run, marker) in [
        ("sess_local_alpha", &run_1, "session-one-body-Ⅰ"),
        ("sess_local_beta", &run_2, "session-two-body-Ⅱ"),
    ] {
        let payloads = known_payloads_for(&boot.state, session, run).await;
        let completed: Vec<_> = payloads
            .iter()
            .filter_map(|p| match p {
                KnownEventPayload::ToolCallCompleted(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(completed.len(), 1, "{session}: exactly one tool completion");
        let text = serde_json::to_string(&completed[0].result).expect("wire json");
        assert!(
            text.contains(marker),
            "{session}: its own body, never the other session's"
        );
    }
    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C05: a runtime nonce the stub cannot know rides the next request ─────────
// Ported from the REV-T03 R01 scratch (F-02): the stub script is fixed FIRST
// (it never sees the nonce), then the nonce is generated and written, then the
// run executes. Two legs with different nonces.

/// One full C05 leg. Returns the nonce and the tool_result content the second
/// request carried.
async fn c05_leg(tag: &str) -> (String, String) {
    // 1. The stub script is FIXED before any nonce exists.
    let stub = StubServer::start(vec![
        anthropic_tool_use(
            "toolu_svc_c05",
            "read",
            serde_json::json!({"path": "nonce.txt"}),
        ),
        anthropic_final("c05 final answer"),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "anthropic_svc": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-ant-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "anthropic_svc", "model": "claude-svc"}}}}"#,
        stub.origin()
    );
    let boot = boot_with_config(tag, &plane_json).await;
    // 2. The nonce is generated at RUNTIME, after the stub script exists.
    let nonce = runtime_nonce(tag);
    std::fs::write(boot.workspace.join("nonce.txt"), &nonce).expect("seed nonce file");

    let run_id = execute(&boot.state, "read nonce.txt").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "tool turn + final turn");
    let messages = requests[1].body["messages"].as_array().expect("messages");
    let blocks = messages[2]["content"]
        .as_array()
        .expect("tool_result blocks");
    assert_eq!(blocks[0]["type"], "tool_result");
    assert_eq!(blocks[0]["tool_use_id"], "toolu_svc_c05");
    assert_eq!(blocks[0]["is_error"], false);
    let content = blocks[0]["content"]
        .as_str()
        .expect("text content")
        .to_string();
    stub.stop().await;
    teardown_boot(&boot).await;
    (nonce, content)
}

#[tokio::test]
async fn c05_runtime_nonce_rides_the_next_request_and_follows_changes() {
    let (nonce_a, wire_a) = c05_leg("c05a").await;
    let (nonce_b, wire_b) = c05_leg("c05b").await;
    assert_ne!(
        nonce_a, nonce_b,
        "the two legs must use different runtime nonces"
    );
    // The wire carries the REAL file bytes of THAT leg — a preset answer or
    // a fixed done-string fails here.
    assert_eq!(
        wire_a, nonce_a,
        "leg A: the wire carries leg A's runtime nonce"
    );
    assert_eq!(
        wire_b, nonce_b,
        "leg B: the wire carries leg B's runtime nonce"
    );
    // A canned "done" marker must NOT be what rides the wire.
    assert!(!wire_a.contains("done"), "no preset-done substitution");
}

// ── C06: mixed content parts persist every block in order ───────────────────

#[tokio::test]
async fn c06_openai_completions_mixed_content_parts_survive_in_order() {
    // The streaming shape of a mixed turn: reasoning and text interleave in
    // arrival order. (The buffered unknown-part leg is pinned by the golden
    // suite — the openai streaming wire has no unknown-part shape.)
    let mixed = openai_sse(&[
        serde_json::json!({"id":"chatcmpl-mixed","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"thinking first 先想"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-mixed","choices":[{"index":0,"delta":{"content":"answer one 第一"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-mixed","choices":[{"index":0,"delta":{"reasoning_content":"thinking second 再想"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-mixed","choices":[{"index":0,"delta":{"content":"answer two 第二"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-mixed","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        serde_json::json!({"id":"chatcmpl-mixed","choices":[],"usage":{"prompt_tokens":12,"completion_tokens":8}}),
    ]);
    let stub = StubServer::start(vec![mixed]).await;
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-openai-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "gpt-svc"}}}}"#,
        stub.v1()
    );
    let boot = boot_with_config("c06", &plane_json).await;

    let run_id = execute(&boot.state, "mixed content please").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    // The persisted final message keeps EVERY block in the provider's
    // ARRIVAL order (reasoning → text → reasoning → text). R05-T04: the
    // streaming wire carries content as string deltas, so the unknown-part
    // leg of the old buffered case moved to the golden suite (the buffered
    // parser remains the terminal validator); the arrival-order contract is
    // what this end-to-end leg pins.
    let content_json = final_message_content(&boot.state, &run_id).await;
    let message: serde_json::Value = serde_json::from_str(&content_json).expect("message json");
    assert_eq!(message["role"], "assistant");
    assert_eq!(
        message["content"],
        serde_json::json!([
            {"type": "reasoning", "text": "thinking first 先想"},
            {"type": "text", "text": "answer one 第一"},
            {"type": "reasoning", "text": "thinking second 再想"},
            {"type": "text", "text": "answer two 第二"}
        ]),
        "every valid block persisted in relative order (C06)"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C07 (credential mapping): authHeader verbatim, none sends nothing ───────

#[tokio::test]
async fn c07_auth_header_goes_verbatim_and_none_sends_nothing() {
    // authHeader — the custom header rides verbatim, never an Authorization.
    let stub = StubServer::start(vec![openai_final("header auth ok")]).await;
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "authHeader", "header": "X-Custom-Key", "value": "custom-secret-42"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "gpt-svc"}}}}"#,
        stub.v1()
    );
    let boot = boot_with_config("c07a", &plane_json).await;
    let run_id = execute(&boot.state, "hi").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    let request = &stub.requests()[0];
    assert_eq!(request.header("x-custom-key"), Some("custom-secret-42"));
    assert_eq!(request.header("authorization"), None);
    stub.stop().await;
    teardown_boot(&boot).await;

    // none — explicitly keyless: no credential header at all.
    let stub = StubServer::start(vec![openai_final("none auth ok")]).await;
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "none"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "gpt-svc"}}}}"#,
        stub.v1()
    );
    let boot = boot_with_config("c07b", &plane_json).await;
    let run_id = execute(&boot.state, "hi").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    let request = &stub.requests()[0];
    assert_eq!(request.header("authorization"), None);
    assert_eq!(request.header("x-api-key"), None);
    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C08: a policy refusal rides the next request as a structured failure ────

#[tokio::test]
async fn c08_a_policy_refusal_rides_the_next_request_as_a_structured_failure() {
    let stub = StubServer::start(vec![
        anthropic_tool_use(
            "toolu_svc_c08",
            "write",
            serde_json::json!({"path": "out.txt", "content": "x"}),
        ),
        anthropic_final("understood — the write was refused"),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "anthropic_svc": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-ant-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "anthropic_svc", "model": "claude-svc"}}}}"#,
        stub.origin()
    );
    let boot = boot_with_config("c08", &plane_json).await;
    // A read-only session denies the write class at the adjudicated policy
    // verdict — zero dispatch, never an auto-allow.
    boot.state
        .sessions()
        .set_permission_mode_for(
            &owner_principal(),
            "sess_local_alpha",
            lingxi_kernel::subagent::SessionPermissionMode::ReadOnly,
        )
        .await
        .expect("set read-only mode");

    let run_id = execute(&boot.state, "write out.txt").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    // The refusal is NOT swallowed: it is reported as a completed-with-failure
    // tool event and rides the next model request as a structured failure.
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_completed").await,
        1,
        "the refused call is reported, never silently dropped"
    );
    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "the refusal must ride the next request");
    let messages = requests[1].body["messages"].as_array().expect("messages");
    let result_blocks = messages[2]["content"].as_array().expect("result blocks");
    let block = &result_blocks[0];
    assert_eq!(block["type"], "tool_result");
    assert_eq!(block["tool_use_id"], "toolu_svc_c08");
    assert_eq!(block["is_error"], true);
    let content = block["content"].as_str().expect("text content");
    assert!(
        content.contains("tool error [forbidden]:")
            && content.contains("ACTION_BLOCKED_BY_READ_ONLY"),
        "the structured refusal reaches the model verbatim: {content}"
    );
    assert!(
        !boot.workspace.join("out.txt").exists(),
        "the refused write never executed"
    );
    let final_content = final_message_content(&boot.state, &run_id).await;
    assert!(final_content.contains("understood"), "{final_content}");

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C12: a retry continues the CONFIRMED exchange ───────────────────────────

#[tokio::test]
async fn c12_retry_continues_the_confirmed_exchange_without_rebuild_or_redo() {
    let read_call = openai_tool_call("call_read_1", "read", "{\"path\":\"note.txt\"}", 10, 4);
    let write_call = openai_tool_call(
        "call_write_1",
        "write",
        "{\"content\":\"v1\",\"path\":\"out.txt\"}",
        18,
        5,
    );
    let flaky = StubResponse::error(
        500,
        r#"{"error":{"message":"flaky upstream 抖动"}}"#.to_string(),
    );
    let stub = StubServer::start(vec![
        read_call,
        write_call,
        flaky,
        openai_final("done after retry 完成"),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-openai-svc"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "gpt-svc"}}}}"#,
        stub.v1()
    );
    let boot = boot_with_config("c12", &plane_json).await;
    std::fs::write(boot.workspace.join("note.txt"), FILE_BODY).expect("seed note");

    let run_id = execute(&boot.state, "read note.txt then write out.txt").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    // Four wire calls: attempt 1 = read-turn, write-turn, 500; attempt 2's
    // FIRST call already carries the whole confirmed exchange.
    let requests = stub.requests();
    assert_eq!(requests.len(), 4, "{requests:?}");
    for request in &requests[2..] {
        let messages = request.body["messages"].as_array().expect("messages");
        assert_eq!(
            messages.len(),
            5,
            "user + assistant(read) + tool(read) + assistant(write) + tool(write): {messages:?}"
        );
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call_read_1");
        assert_eq!(messages[2]["tool_call_id"], "call_read_1");
        assert_eq!(messages[2]["content"], FILE_BODY);
        assert_eq!(messages[3]["tool_calls"][0]["id"], "call_write_1");
        assert_eq!(messages[4]["tool_call_id"], "call_write_1");
        assert!(
            !messages[4]["content"]
                .as_str()
                .expect("tool text")
                .is_empty(),
            "the write's REAL outcome text rides the retry"
        );
    }

    // Nothing was rebuilt and nothing was re-executed: the read and the
    // write each ran EXACTLY once across both attempts.
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_started").await,
        2
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_completed").await,
        2
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_started").await,
        4
    );
    assert_eq!(
        std::fs::read_to_string(boot.workspace.join("out.txt")).expect("out.txt written"),
        "v1",
        "the confirmed write landed exactly once"
    );
    let final_content = final_message_content(&boot.state, &run_id).await;
    assert!(
        final_content.contains("done after retry 完成"),
        "{final_content}"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── codex: the SSE-only family end to end ───────────────────────────────────

/// The golden JWT whose payload carries the codex account claim
/// (`{"https://api.openai.com/auth":{"chatgpt_account_id":"acct-t03-service"}}`).
const CODEX_GOLDEN_JWT: &str = "goldenheader.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdC10MDMtc2VydmljZSJ9fQ.goldensig";

#[tokio::test]
async fn codex_family_runs_end_to_end_over_sse() {
    let sse_body = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_svc\",\"status\":\"in_progress\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ignored incremental\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_svc\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"codex service 完成\"}]}],\"usage\":{\"input_tokens\":12,\"output_tokens\":4}}}\n\n",
    );
    let stub = StubServer::start(vec![StubResponse::sse(sse_body.to_string())]).await;
    let plane_json = format!(
        r#""providers": {{
            "codex_svc": {{
                "protocol": "openai-codex-responses",
                "endpoint": "{}/backend-api",
                "auth": {{"kind": "apiKey", "apiKey": "{}"}}
            }}
        }},
        "models": {{"chat": {{"provider": "codex_svc", "model": "gpt-codex-svc"}}}}"#,
        stub.origin(),
        CODEX_GOLDEN_JWT
    );
    let boot = boot_with_config("codex", &plane_json).await;

    let run_id = execute(&boot.state, "ping codex").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    let requests = stub.requests();
    assert_eq!(requests.len(), 1, "one SSE turn");
    let request = &requests[0];
    assert_eq!(request.path, "/backend-api/codex/responses");
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {CODEX_GOLDEN_JWT}").as_str())
    );
    assert_eq!(
        request.header("openai-beta"),
        Some("responses=experimental")
    );
    assert_eq!(request.header("originator"), Some("pi"));
    assert_eq!(
        request.header("chatgpt-account-id"),
        Some("acct-t03-service")
    );
    assert_eq!(request.header("x-api-key"), None);
    // The family's fixed wire facts.
    assert_eq!(request.body["store"], false);
    assert_eq!(request.body["stream"], true);
    assert_eq!(
        request.body["instructions"],
        "You are Hana's utility model.\nFollow the user request exactly and return only the requested content."
    );
    assert!(
        request.body.get("previous_response_id").is_none(),
        "no server-side session (C11)"
    );
    assert_eq!(request.body["input"][0]["role"], "user");

    assert_eq!(
        persisted_usages(&boot.state, &run_id).await,
        vec![Some(UsageRecord {
            input_tokens: 12,
            output_tokens: 4
        })],
        "the terminal event's usage is the persisted usage"
    );
    let final_content = final_message_content(&boot.state, &run_id).await;
    assert!(
        final_content.contains("codex service 完成"),
        "{final_content}"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}
