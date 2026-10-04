//! R05-T04 service-level evidence: the REAL streaming chain (loopback stub
//! provider → reqwest incremental read → family accumulator → bounded delta
//! channel → run-driver drain/normalize → durable commit → post-commit
//! publication → live subscription) under the task book's streaming
//! checkpoints.
//!
//! Double boundary: the ONLY double is the stub HTTP server (the far end of
//! the wire). Every in-process link — config resolution, gateway, credential
//! service, adapter, decoder, delta sink, normalizer, run driver, storage,
//! event hub, subscription — is production code.
//!
//! Coverage map (appendix-B R05-T04 checkpoints + the formal A-ids):
//! - C12 (A07's service half): the anti-fake-streaming barrier — a gated
//!   stub flushes the FIRST frame and then holds the stream open; a REAL
//!   subscriber observes the first delta (durable + published) BEFORE the
//!   barrier releases the terminal frames. The durable order is
//!   started → deltas → segment-end → completed (A09's ordering half).
//! - C05: half-JSON tool arguments at stream end dispatch ZERO tools; the
//!   run fails loudly with no side effects.
//! - C06: arguments that are already parseable while the stream is still
//!   open dispatch NOTHING during the held window; exactly one execution
//!   follows the protocol terminal.
//! - C09: a length-truncated batch (one complete tool call inside) executes
//!   nothing; the retry (new attempt, same run) is clean.
//! - C11: stop-reason outcomes at the service surface (non-retryable
//!   refusal fails in place; a retryable truncation retries).
//! - C13: MOOD/think tags vs literal tags — the live event stream
//!   structures standalone think blocks, keeps fenced literal tags, and
//!   drops mood content, while the canonical message keeps the raw text;
//!   the history projection is the SAME scanner.
//! - C15: HTTP-200-then-broken streams are never masked by the status:
//!   clean-EOF without the terminal sentinel, a mid-frame EOF, a provider
//!   error event, a usage-only stream and a duplicated terminal marker each
//!   produce their honest outcome.
//! - C16 + A09 (cancel half): a cancellation mid-stream drops the
//!   provider's socket read (the stub observes the disconnect), settles the
//!   run cancelled exactly once, and no late delta lands afterwards.
//! - A08: an interrupted stream never fabricates a final reply — partial
//!   deltas stay durable, the run fails, no final message row exists.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use lingxi_protocol::{EventPayload, KnownEventPayload, UsageRecord};
use lingxi_service::events::{SubscribeOutcome, SubscriptionFrame};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const NOW_MS: u64 = 1_790_409_600_000;

// ── the stub provider (the far end of the wire; one request per connection) ──

/// One scripted connection of the stub.
enum Script {
    /// Written whole, then the connection closes (the R05-T03 behavior).
    Immediate {
        status: u16,
        content_type: &'static str,
        body: String,
    },
    /// The anti-fake-streaming gate: 200 + SSE headers + `first` are
    /// flushed, then the stub HOLDS the stream open until the test releases
    /// the barrier (or the client disconnects) and only then sends `rest`.
    GatedSse { first: String, rest: String },
}

fn sse(body: String) -> Script {
    Script::Immediate {
        status: 200,
        content_type: "text/event-stream",
        body,
    }
}

fn error(status: u16, body: String) -> Script {
    Script::Immediate {
        status,
        content_type: "application/json",
        body,
    }
}

#[derive(Debug)]
struct RecordedRequest {
    path: String,
    headers: Vec<(String, String)>,
    body: serde_json::Value,
}

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    /// Fires when the gated script's first frame is on the wire.
    flushed: Arc<tokio::sync::Notify>,
    /// The test's barrier release for the gated script.
    release: Arc<tokio::sync::Notify>,
    /// How often the stub observed the client going away mid-stream.
    disconnects: Arc<AtomicUsize>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    /// Starts a raw-TCP HTTP/1.1 stub. Scripts are popped in order; an
    /// exhausted script is a LOUD 500, never an improvised reply. At most
    /// ONE GatedSse script per server (the barrier is a single-shot
    /// test instrument).
    async fn start(scripts: Vec<Script>) -> Self {
        assert!(
            scripts
                .iter()
                .filter(|s| matches!(s, Script::GatedSse { .. }))
                .count()
                <= 1,
            "one gated script per stub (the barrier is single-shot)"
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let scripts = Arc::new(Mutex::new(VecDeque::from(scripts)));
        let flushed = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let disconnects = Arc::new(AtomicUsize::new(0));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_requests, task_scripts) = (
            Arc::clone(&hits),
            Arc::clone(&requests),
            Arc::clone(&scripts),
        );
        let (task_flushed, task_release, task_disconnects) = (
            Arc::clone(&flushed),
            Arc::clone(&release),
            Arc::clone(&disconnects),
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
                let script = task_scripts.lock().expect("scripts").pop_front();
                let script = match script {
                    Some(script) => script,
                    None => error(
                        500,
                        r#"{"error":{"message":"stub script exhausted"}}"#.to_string(),
                    ),
                };
                match script {
                    Script::Immediate {
                        status,
                        content_type,
                        body,
                    } => {
                        let reason = match status {
                            200 => "OK",
                            400 => "Bad Request",
                            401 => "Unauthorized",
                            429 => "Too Many Requests",
                            500 => "Internal Server Error",
                            other => panic!("stub: unscripted status {other}"),
                        };
                        let raw = format!(
                            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(raw.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    }
                    Script::GatedSse { first, rest } => {
                        // Deliberately NO Content-Length: the body is
                        // close-delimited, so the first frame reaches the
                        // client's incremental read BEFORE the stream ends.
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
                        if socket
                            .write_all(format!("{head}{first}").as_bytes())
                            .await
                            .is_err()
                        {
                            task_disconnects.fetch_add(1, Ordering::SeqCst);
                            continue;
                        }
                        let _ = socket.flush().await;
                        task_flushed.notify_one();
                        let (mut rd, mut wr) = tokio::io::split(socket);
                        let mut buf = [0_u8; 512];
                        tokio::select! {
                            _ = task_release.notified() => {
                                let _ = wr.write_all(rest.as_bytes()).await;
                                let _ = wr.shutdown().await;
                            }
                            read = rd.read(&mut buf) => {
                                // The client went away mid-stream (cancel /
                                // drop) — the connection-release witness.
                                let _ = read;
                                task_disconnects.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    }
                }
            }
        });
        Self {
            addr,
            hits,
            requests,
            flushed,
            release,
            disconnects,
            shutdown: Some(shutdown),
            task,
        }
    }

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

    fn disconnects(&self) -> usize {
        self.disconnects.load(Ordering::SeqCst)
    }

    /// Waits (bounded) for the gated script's first frame to be flushed.
    async fn wait_flushed(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.flushed.notified())
            .await
            .expect("the gated stub flushed its first frame");
    }

    /// Releases the barrier: the stub sends the remaining frames and closes.
    fn release_rest(&self) {
        self.release.notify_one();
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

// ── harness (the R05-T01 boot pattern: the REAL composition root) ────────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t04-{tag}-{}-{}",
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

/// The openai plane pinned at the stub.
fn openai_plane(stub: &StubServer) -> String {
    format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-openai-t04"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "gpt-t04"}}}}"#,
        stub.v1()
    )
}

/// The anthropic plane pinned at the stub.
fn anthropic_plane(stub: &StubServer) -> String {
    format!(
        r#""providers": {{
            "anthropic_svc": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-ant-t04"}}
            }}
        }},
        "models": {{"chat": {{"provider": "anthropic_svc", "model": "claude-t04"}}}}"#,
        stub.origin()
    )
}

/// Foreground execute (drives the run to its terminal before returning).
async fn execute(state: &ServiceState, input: &str) -> String {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            input,
            NOW_MS,
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

/// The decoded known payloads of one run's durable events, in commit order
/// (the authority; `stream_events_after` returns seq order).
async fn known_payloads(state: &ServiceState, run_id: &str) -> Vec<KnownEventPayload> {
    use lingxi_kernel::ports::EventStorePort as _;
    state
        .storage()
        .stream_events_after("sess_local_alpha", lingxi_protocol::Seq::new(0), 10_000)
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

async fn final_message_content(state: &ServiceState, run_id: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(
            "SELECT content_json FROM messages WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("final message query")
}

async fn teardown_boot(boot: &PlaneBoot) {
    boot.state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&boot.home);
    let _ = std::fs::remove_dir_all(&boot.workspace);
    let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
}

/// Bounded condition wait (condition-polled, never a sleep-based ordering
/// proof).
async fn wait_until(what: &str, mut probe: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !probe() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

// ── canned provider streams ──────────────────────────────────────────────────

fn openai_frame(frame: serde_json::Value) -> String {
    format!("data: {frame}\n\n")
}

fn openai_done() -> String {
    "data: [DONE]\n\n".to_string()
}

fn openai_text_frame(text: &str) -> String {
    openai_frame(
        serde_json::json!({"id":"chatcmpl-t04","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
    )
}

fn openai_finish_frame(reason: &str) -> String {
    openai_frame(
        serde_json::json!({"id":"chatcmpl-t04","choices":[{"index":0,"delta":{},"finish_reason":reason}]}),
    )
}

fn openai_usage_frame(prompt: u64, completion: u64) -> String {
    openai_frame(
        serde_json::json!({"id":"chatcmpl-t04","choices":[],"usage":{"prompt_tokens":prompt,"completion_tokens":completion}}),
    )
}

/// A complete openai final-answer stream (one text delta + stop + usage).
fn openai_final(text: &str) -> Script {
    sse(format!(
        "{}{}{}{}",
        openai_text_frame(text),
        openai_finish_frame("stop"),
        openai_usage_frame(21, 9),
        openai_done()
    ))
}

fn anthropic_sse(frames: &[serde_json::Value]) -> Script {
    let mut body = String::new();
    for frame in frames {
        let ty = frame["type"].as_str().expect("typed anthropic frame");
        body.push_str(&format!("event: {ty}\ndata: {frame}\n\n"));
    }
    sse(body)
}

fn anthropic_final(text: &str) -> Script {
    anthropic_sse(&[
        serde_json::json!({"type":"message_start","message":{"id":"msg_t04","model":"m","usage":{"input_tokens":40}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}),
        serde_json::json!({"type":"message_stop"}),
    ])
}

// ── C12: real streaming, proven by a real subscriber at a barrier ────────────

#[tokio::test]
async fn c12_first_delta_reaches_the_subscriber_before_the_external_stream_ends() {
    // The gated stub: the FIRST text frame is flushed, then the stream
    // holds open until the test releases the barrier.
    let first = openai_text_frame("第一片 🌊");
    let rest = format!(
        "{}{}{}{}",
        openai_text_frame("第二片"),
        openai_finish_frame("stop"),
        openai_usage_frame(30, 12),
        openai_done()
    );
    let stub = StubServer::start(vec![Script::GatedSse { first, rest }]).await;
    let boot = boot_with_config("c12", &openai_plane(&stub)).await;

    // Drive the run on a spawned task (the foreground execute blocks until
    // the run's terminal).
    let drive_state = boot.state.clone();
    let drive = tokio::spawn(async move {
        let storage = Arc::clone(drive_state.storage());
        drive_state
            .sessions()
            .execute_for(
                storage.as_ref(),
                drive_state.events(),
                drive_state.runs(),
                &owner_principal(),
                "sess_local_alpha",
                "stream please",
                NOW_MS,
            )
            .await
            .expect("execute accepted")
            .run_id
    });

    // The session appears when the run starts; subscribe as soon as it
    // exists (a readiness retry on the explicit StreamNotFound, never a
    // sleep guess).
    let subscription = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match boot
                .state
                .events()
                .subscribe(&owner_principal(), "sess_local_alpha", None)
                .await
            {
                Ok(SubscribeOutcome::Started { cut, subscription }) => {
                    break (cut, subscription);
                }
                Ok(SubscribeOutcome::RequiresSnapshot(required)) => {
                    panic!("a fresh stream must never require a snapshot: {required:?}")
                }
                Err(lingxi_service::events::SubscribeReject::StreamNotFound { .. }) => {
                    tokio::task::yield_now().await;
                }
                Err(other) => panic!("subscribe failed: {other:?}"),
            }
        }
    })
    .await
    .expect("subscribe ready");
    let (cut, subscription) = subscription;

    // The stub's first frame is on the wire; the stream is HELD OPEN (the
    // barrier is structural: `rest` cannot be sent before release_rest).
    stub.wait_flushed().await;

    // The first delta must reach the subscriber NOW — through the durable
    // commit + post-commit publication — while the provider's response has
    // NOT ended. Scan the cut page first (events committed before the
    // subscribe boundary), then the live mailbox.
    let cut_has_delta = cut.events.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            EventPayload::Known(KnownEventPayload::AssistantSegmentDelta(payload))
                if payload.delta.contains("第一片")
        ) || matches!(
            &envelope.payload,
            EventPayload::Known(KnownEventPayload::ModelCallDelta(payload))
                if payload.delta.contains("第一片")
        )
    });
    let mut live_saw_delta = false;
    if !cut_has_delta {
        live_saw_delta = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                match subscription.mailbox().recv().await {
                    Some(SubscriptionFrame::Event(envelope)) => {
                        if let EventPayload::Known(known) = &envelope.payload {
                            match known {
                                KnownEventPayload::AssistantSegmentDelta(payload)
                                    if payload.delta.contains("第一片") =>
                                {
                                    break true;
                                }
                                KnownEventPayload::ModelCallDelta(payload)
                                    if payload.delta.contains("第一片") =>
                                {
                                    break true;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(SubscriptionFrame::SnapshotRequired { reason }) => {
                        panic!("subscription detached mid-proof: {reason:?}")
                    }
                    None => panic!("subscription closed mid-proof"),
                }
            }
        })
        .await
        .expect("the first delta reached the subscriber while the stream was held open");
    }
    assert!(
        cut_has_delta || live_saw_delta,
        "the first delta is observable before the barrier releases"
    );

    // The proof of NOT-fake-streaming, stated precisely: at this instant
    // the external stream has NOT ended (the stub is still holding it —
    // structurally incapable of having sent the terminal frames), yet the
    // delta is already durable and published.
    assert_eq!(
        stub.hits(),
        1,
        "exactly one provider connection, still open"
    );
    // And the run has NOT completed (no terminal row state).
    // (The run id is not known until the drive returns; the durable
    // evidence below pins the same fact through event order.)

    // Release the barrier: the terminal frames flow, the run completes.
    stub.release_rest();
    let run_id = tokio::time::timeout(std::time::Duration::from_secs(30), drive)
        .await
        .expect("drive settles")
        .expect("drive task");
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    // The final message is the concatenation of the pre-barrier and
    // post-barrier fragments — the pre-barrier fragment was REAL content,
    // not a re-sliced replay.
    let content = final_message_content(&boot.state, &run_id)
        .await
        .expect("final message row");
    assert!(content.contains("第一片 🌊第二片"), "{content}");

    // A09's ordering half, on the durable authority: started → deltas →
    // segment ends → completed, strictly in commit order.
    let payloads = known_payloads(&boot.state, &run_id).await;
    let kind_index = |predicate: &dyn Fn(&KnownEventPayload) -> bool| -> Vec<usize> {
        payloads
            .iter()
            .enumerate()
            .filter_map(|(i, p)| predicate(p).then_some(i))
            .collect()
    };
    let started = kind_index(&|p| matches!(p, KnownEventPayload::ModelCallStarted(_)));
    let deltas = kind_index(&|p| {
        matches!(
            p,
            KnownEventPayload::ModelCallDelta(_) | KnownEventPayload::AssistantSegmentDelta(_)
        )
    });
    let segment_ends = kind_index(&|p| matches!(p, KnownEventPayload::AssistantSegmentEnd(_)));
    let completed = kind_index(&|p| matches!(p, KnownEventPayload::ModelCallCompleted(_)));
    assert_eq!(started.len(), 1, "one model call");
    assert!(!deltas.is_empty(), "live deltas are durable facts");
    assert_eq!(completed.len(), 1, "the call's completed fact");
    assert!(
        started[0] < deltas[0],
        "started is durable before the first delta"
    );
    assert!(
        deltas.iter().all(|d| *d < completed[0]),
        "every delta precedes the call's completed fact"
    );
    assert!(
        segment_ends.iter().all(|e| *e < completed[0]),
        "segments close before the call completes"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C05: half-JSON arguments dispatch zero tools (service surface) ───────────

#[tokio::test]
async fn c05_half_json_arguments_dispatch_zero_tools() {
    // The arguments stop mid-string, then the provider terminates the
    // stream "cleanly". The batch must refuse the whole turn; nothing
    // dispatches; the run fails loudly.
    let body = format!(
        "{}{}{}",
        openai_frame(
            serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_half","type":"function","function":{"name":"write","arguments":"{\"path\":\"out.txt\",\"content\":\"x"}}]},"finish_reason":null}]})
        ),
        openai_finish_frame("tool_calls"),
        openai_done()
    );
    let stub = StubServer::start(vec![sse(body)]).await;
    let boot = boot_with_config("c05", &openai_plane(&stub)).await;

    let run_id = execute(&boot.state, "write out.txt").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    // Zero dispatch: no tool intent, no tool events, no side effect.
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_started").await,
        0,
        "a half-JSON batch never dispatches"
    );
    assert!(
        !boot.workspace.join("out.txt").exists(),
        "no file materialized"
    );
    // The call's facts closed honestly (started + completed), the failure
    // is the run's terminal — no fabricated final message.
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_started").await,
        1
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_completed").await,
        1
    );
    assert!(
        final_message_content(&boot.state, &run_id).await.is_none(),
        "no fabricated final"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C06: parseable arguments are not completion (service barrier) ────────────

#[tokio::test]
async fn c06_parseable_arguments_do_not_dispatch_before_the_terminal() {
    // The gated first part already contains the tool call with COMPLETE,
    // parseable arguments — but the protocol terminal ([DONE]) is held
    // behind the barrier.
    let first = openai_frame(
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_full","type":"function","function":{"name":"read","arguments":"{\"path\":\"note.txt\"}"}}]},"finish_reason":null}]}),
    ) + &openai_finish_frame("tool_calls");
    let rest = format!("{}{}", openai_usage_frame(10, 4), openai_done());
    let stub = StubServer::start(vec![
        Script::GatedSse { first, rest },
        openai_final("c06 完成"),
    ])
    .await;
    let boot = boot_with_config("c06", &openai_plane(&stub)).await;
    std::fs::write(boot.workspace.join("note.txt"), "c06 真实内容").expect("seed note");

    let drive_state = boot.state.clone();
    let drive = tokio::spawn(async move {
        let storage = Arc::clone(drive_state.storage());
        drive_state
            .sessions()
            .execute_for(
                storage.as_ref(),
                drive_state.events(),
                drive_state.runs(),
                &owner_principal(),
                "sess_local_alpha",
                "read note.txt",
                NOW_MS,
            )
            .await
            .expect("execute accepted")
            .run_id
    });
    stub.wait_flushed().await;

    // The parseable-but-open window: the stream delivered the complete
    // arguments and a finish_reason, but NOT the terminal sentinel. The
    // run must be alive with ZERO tool dispatch — admission waits for the
    // closed batch.
    //
    // Find the run id (the admission is durable by now) and assert no tool
    // facts exist for it while the barrier holds.
    let run_id: String = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(id) = boot
                .state
                .storage()
                .query_one_text(
                    "SELECT run_id FROM runs WHERE session_id = 'sess_local_alpha' ORDER BY rowid DESC LIMIT 1",
                    vec![],
                )
                .await
                .expect("run lookup")
            {
                break id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("run admitted");
    // Hold the barrier a beat and probe repeatedly: at NO point may a tool
    // fact exist.
    for _ in 0..50 {
        assert_eq!(
            count_events(&boot.state, &run_id, "tool_call_started").await,
            0,
            "parseable is not complete: zero dispatch while the stream is open"
        );
        tokio::task::yield_now().await;
    }
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "running", "the run waits on the open stream");

    // Release the terminal: the batch closes, the tool executes EXACTLY
    // once, and the continuation turn sees the real content.
    stub.release_rest();
    let run_id = tokio::time::timeout(std::time::Duration::from_secs(30), drive)
        .await
        .expect("drive settles")
        .expect("drive task");
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_started").await,
        1,
        "exactly one dispatch, after the terminal"
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_completed").await,
        1
    );
    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "tool turn + continuation turn");
    let messages = requests[1].body["messages"].as_array().expect("messages");
    assert_eq!(messages[2]["tool_call_id"], "call_full");
    assert_eq!(messages[2]["content"], "c06 真实内容");

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C09: a truncated batch is zero-side-effect at the service surface ────────

#[tokio::test]
async fn c09_length_truncated_batch_executes_nothing_and_the_retry_is_clean() {
    // Attempt 1: one COMPLETE tool call, then finish_reason "length" (a
    // second call truncated away). Attempt 2: a clean final answer.
    let truncated = sse(format!(
        "{}{}{}{}",
        openai_frame(
            serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_complete","type":"function","function":{"name":"write","arguments":"{\"path\":\"out.txt\",\"content\":\"v1\"}"}}]},"finish_reason":null}]})
        ),
        openai_finish_frame("length"),
        openai_usage_frame(11, 33),
        openai_done()
    ));
    let stub = StubServer::start(vec![truncated, openai_final("retry 完成")]).await;
    let boot = boot_with_config("c09", &openai_plane(&stub)).await;

    let run_id = execute(&boot.state, "write out.txt").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    // Zero side effects from the truncated batch: the complete tool call
    // inside it NEVER dispatched (batch admission is all-or-nothing per
    // closed turn) — no tool facts, no file.
    assert_eq!(
        count_events(&boot.state, &run_id, "tool_call_started").await,
        0,
        "the truncated batch's complete call never dispatched"
    );
    assert!(!boot.workspace.join("out.txt").exists());
    // The retry was a NEW ATTEMPT on the same run: two model calls, the
    // failed call's usage honestly recorded.
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_started").await,
        2
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_completed").await,
        2
    );
    let usages: Vec<Option<UsageRecord>> = known_payloads(&boot.state, &run_id)
        .await
        .iter()
        .filter_map(|p| match p {
            KnownEventPayload::ModelCallCompleted(p) => Some(p.usage.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        usages,
        vec![
            Some(UsageRecord {
                input_tokens: 11,
                output_tokens: 33
            }),
            Some(UsageRecord {
                input_tokens: 21,
                output_tokens: 9
            })
        ],
        "the truncated call's real usage is not lost"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C11: stop-reason outcomes at the service surface ─────────────────────────

#[tokio::test]
async fn c11_a_content_refusal_fails_in_place_without_retry() {
    let refused = sse(format!(
        "{}{}{}",
        openai_text_frame("I cannot"),
        openai_finish_frame("content_filter"),
        openai_done()
    ));
    // A second script that must NEVER be consumed: a refusal is not
    // retryable, so any second contact proves a classification bug.
    let stub = StubServer::start(vec![refused]).await;
    let boot = boot_with_config("c11rf", &openai_plane(&stub)).await;

    let run_id = execute(&boot.state, "say something refused").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        stub.hits(),
        1,
        "a content refusal never retries (non-retryable)"
    );
    // The partial content that DID stream stays durable (the honest
    // partial record), and no final was fabricated.
    assert!(
        count_events(&boot.state, &run_id, "model_call_delta").await >= 1,
        "the refusal's partial content is preserved"
    );
    assert!(final_message_content(&boot.state, &run_id).await.is_none());

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C15 + A08: HTTP 200 followed by a broken stream is never masked ──────────

#[tokio::test]
async fn c15_a08_http200_stream_failures_keep_the_run_honest() {
    // One boot, five legs (sequential runs on one session):
    let stub = StubServer::start(vec![
        // Leg 1 (A08): text deltas stream, then the connection CLOSES
        // without the [DONE] sentinel — a clean-EOF truncation.
        sse(format!(
            "{}{}",
            openai_text_frame("半截回答"),
            openai_text_frame("继续")
        )),
        // Leg 2: a mid-frame EOF — the last frame is not blank-line
        // terminated before the close.
        sse(format!(
            "{}data: {{\"truncat",
            openai_text_frame("先发一帧")
        )),
        // Leg 3: usage arrived but no content and no finish — then [DONE].
        sse(format!("{}{}", openai_usage_frame(7, 0), openai_done())),
        // Leg 4: a duplicated terminal marker (a frame after [DONE]).
        sse(format!(
            "{}{}{}",
            openai_text_frame("done then extra"),
            openai_done(),
            openai_text_frame("late frame")
        )),
    ])
    .await;

    // Legs 1-4 run on the openai plane.
    let boot = boot_with_config("c15o", &openai_plane(&stub)).await;

    // Leg 1: clean EOF without the sentinel → loud failure, partials
    // durable, no fabricated final (A08).
    let run_id = execute(&boot.state, "leg one").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed", "a socket close is never a success");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert!(
        count_events(&boot.state, &run_id, "model_call_delta").await >= 2,
        "the partial deltas stay durable"
    );
    assert!(
        final_message_content(&boot.state, &run_id).await.is_none(),
        "no fabricated final out of the partial stream"
    );

    // Leg 2: mid-frame EOF → the unterminated trailing frame is reported;
    // the run fails; the intact frame's delta stayed durable.
    let run_id = execute(&boot.state, "leg two").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert!(
        count_events(&boot.state, &run_id, "model_call_delta").await >= 1,
        "the intact frame before the truncation was delivered"
    );

    // Leg 3: usage-only stream → an honest empty outcome WITH the usage
    // the provider reported — not a fake answer, not a fake zero.
    let run_id = execute(&boot.state, "leg three").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        reason.as_deref(),
        Some("completed.no_final.empty_reply"),
        "a usage-only stream is an explicit empty, never a final"
    );
    let usages: Vec<Option<UsageRecord>> = known_payloads(&boot.state, &run_id)
        .await
        .iter()
        .filter_map(|p| match p {
            KnownEventPayload::ModelCallCompleted(p) => Some(p.usage.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        usages,
        vec![Some(UsageRecord {
            input_tokens: 7,
            output_tokens: 0
        })],
        "the provider-reported usage (a real zero) is persisted as reported"
    );

    // Leg 4: a frame after [DONE] → the stream is corrupt, the run fails.
    let run_id = execute(&boot.state, "leg four").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed", "a post-terminal frame is never absorbed");

    stub.stop().await;
    teardown_boot(&boot).await;
}

/// The anthropic family's in-stream error event is retryable: the attempt
/// fails loudly, the retry (a new model call on the same run) is clean.
/// (Separate boot: the plane is fixed at boot time.)
#[tokio::test]
async fn c15_anthropic_in_stream_error_event_retries_cleanly() {
    let stub = StubServer::start(vec![
        anthropic_sse(&[
            serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":5}}}),
            serde_json::json!({"type":"error","error":{"type":"overloaded_error","message":"overloaded 抖动"}}),
        ]),
        anthropic_final("恢复后的回答"),
    ])
    .await;
    let boot = boot_with_config("c15a", &anthropic_plane(&stub)).await;

    let run_id = execute(&boot.state, "anthropic stream error leg").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(stub.hits(), 2, "retryable in-stream error retried once");
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_started").await,
        2
    );
    let content = final_message_content(&boot.state, &run_id)
        .await
        .expect("final");
    assert!(content.contains("恢复后的回答"), "{content}");

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C16 + A09 (cancel half): a mid-stream cancel releases the connection ─────

#[tokio::test]
async fn c16_a09_midstream_cancel_releases_the_connection_and_settles_once() {
    // The gated stub: one text delta flushed, then the stream HELD OPEN.
    // `rest` is deliberately never released — the cancel must end the call
    // by dropping the socket read, not by consuming a terminal.
    let first = openai_text_frame("取消前的片段");
    let rest = format!(
        "{}{}{}",
        openai_text_frame("绝不该送达的片段"),
        openai_finish_frame("stop"),
        openai_done()
    );
    let stub = StubServer::start(vec![Script::GatedSse { first, rest }]).await;
    let boot = boot_with_config("c16", &openai_plane(&stub)).await;

    // Drive on the BACKGROUND surface (the foreground execute would block
    // the test on the held stream).
    let submission = lingxi_service::ExecuteSubmission::plain("c16 stream then cancel");
    let storage = Arc::clone(boot.state.storage());
    let events = Arc::clone(boot.state.events());
    let runs = Arc::clone(boot.state.runs());
    let background = Arc::clone(boot.state.background());
    let run_id = boot
        .state
        .sessions()
        .execute_background_for(
            &storage,
            &events,
            &runs,
            &background,
            &owner_principal(),
            "sess_local_alpha",
            &submission,
            NOW_MS,
        )
        .await
        .expect("background accepted")
        .run_id;

    // The stub flushed its first frame; the stream is held open. The
    // started fact is durable before the first delta (D6) — poll for it
    // (a readiness probe, never a sleep-based ordering guess).
    stub.wait_flushed().await;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if count_events(&boot.state, &run_id, "model_call_started").await == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("model_call_started durable while the stream is held open");

    // The cancel: accepted through the session surface, the run settles
    // cancelled exactly once, the background registry drains.
    let outcome = boot
        .state
        .sessions()
        .cancel_run_for(
            boot.state.storage().as_ref(),
            boot.state.runs(),
            &owner_principal(),
            &run_id,
        )
        .await
        .expect("cancel request");
    assert!(
        matches!(outcome, lingxi_service::CancelRunOutcome::Accepted { .. }),
        "cancel accepted: {outcome:?}"
    );
    wait_until("background drive settles", || {
        boot.state.background().live_ids().is_empty()
    })
    .await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));

    // A09's cancel half: dropping the in-flight read released the
    // provider connection — the stub OBSERVED the disconnect.
    wait_until("stub observed the client disconnect", || {
        stub.disconnects() == 1
    })
    .await;

    // The call's facts stayed honest: started is durable, completed is
    // NOT fabricated for a call that never ended; no final message exists.
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_started").await,
        1
    );
    assert_eq!(
        count_events(&boot.state, &run_id, "model_call_completed").await,
        0,
        "a cancelled call is never closed with a fabricated completed"
    );
    assert!(
        final_message_content(&boot.state, &run_id).await.is_none(),
        "no final message after an accepted cancellation"
    );

    // No late delta lands after the settle: the durable event count is
    // stable across a real time window (the fence drops late emissions;
    // the cancelled child can no longer emit into a committed event).
    let total_before: i64 = {
        let payloads = known_payloads(&boot.state, &run_id).await;
        payloads.len() as i64
    };
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let total_after = known_payloads(&boot.state, &run_id).await.len() as i64;
    assert_eq!(
        total_before, total_after,
        "no late event commits after the cancellation settle"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C13: live normalization and history projection are the same source ───────

#[tokio::test]
async fn c13_live_normalization_and_history_projection_share_the_scanner() {
    // One text block whose raw content mixes: a FENCED literal think tag
    // (must stay literal text), a standalone think block (structured as
    // reasoning), and a mood block (stripped from the event stream, kept
    // raw in the canonical message). Split across three deltas at
    // mid-tag boundaries to exercise the cross-frame pending-tag hold.
    let raw =
        "引用块：\n```\n<think>不是标签</think>\n```\n<think>内部推理</think><mood>开心</mood>完成";
    let cut1 = "引用块：\n```\n<think>不是标签</think>\n```\n<th";
    let cut2 = "ink>内部推理</think><mood>开";
    let cut3 = "心</mood>完成";
    assert_eq!(format!("{cut1}{cut2}{cut3}"), raw);
    let stub = StubServer::start(vec![anthropic_sse(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":12}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":cut1}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":cut2}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":cut3}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}),
        serde_json::json!({"type":"message_stop"}),
    ])])
    .await;
    let boot = boot_with_config("c13", &anthropic_plane(&stub)).await;

    let run_id = execute(&boot.state, "c13 mixed reserved tags").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    let payloads = known_payloads(&boot.state, &run_id).await;
    let mut reasoning_deltas = String::new();
    let mut text_deltas = String::new();
    let mut segment_reasoning = String::new();
    let mut segment_text = String::new();
    for payload in &payloads {
        match payload {
            KnownEventPayload::ModelCallDelta(p) => match p.phase {
                lingxi_protocol::AssistantPhase::Reasoning => reasoning_deltas.push_str(&p.delta),
                lingxi_protocol::AssistantPhase::FinalAnswer => text_deltas.push_str(&p.delta),
                other => panic!("unexpected model_call_delta phase {other:?}"),
            },
            KnownEventPayload::AssistantSegmentDelta(p) => match p.semantic_phase {
                lingxi_protocol::AssistantPhase::Reasoning => segment_reasoning.push_str(&p.delta),
                lingxi_protocol::AssistantPhase::FinalAnswer => segment_text.push_str(&p.delta),
                other => panic!("unexpected segment delta phase {other:?}"),
            },
            _ => {}
        }
    }
    // The standalone think block streamed as REASONING on both vocabularies.
    assert!(
        reasoning_deltas.contains("内部推理"),
        "think block is live reasoning: {reasoning_deltas:?}"
    );
    assert!(
        segment_reasoning.contains("内部推理"),
        "the segment view carries the same reasoning: {segment_reasoning:?}"
    );
    // The fenced literal stayed TEXT on both views.
    assert!(
        text_deltas.contains("<think>不是标签</think>"),
        "fenced tags stay literal text: {text_deltas:?}"
    );
    assert!(segment_text.contains("<think>不是标签</think>"));
    assert!(text_deltas.contains("完成"), "{text_deltas:?}");
    // D5: mood content NEVER enters the event stream — on any phase,
    // either vocabulary.
    for payload in &payloads {
        let delta = match payload {
            KnownEventPayload::ModelCallDelta(p) => Some(p.delta.as_str()),
            KnownEventPayload::AssistantSegmentDelta(p) => Some(p.delta.as_str()),
            _ => None,
        };
        if let Some(delta) = delta {
            assert!(
                !delta.contains("开心"),
                "mood content leaked into the event stream: {delta:?}"
            );
        }
    }

    // The canonical message keeps the RAW text (mood included).
    let content = final_message_content(&boot.state, &run_id)
        .await
        .expect("final message row");
    assert!(content.contains("<mood>开心</mood>"), "{content}");
    assert!(content.contains("<think>不是标签</think>"), "{content}");
    assert!(content.contains("内部推理"), "{content}");

    // Same-source: the history projection of the FINAL raw text is the
    // same scanner — the standalone think block structures identically,
    // the fenced literal stays text.
    let segments = lingxi_service::streaming_norm::split_reserved_tag_segments(
        raw,
        &lingxi_service::streaming_norm::THINK_TAGS,
    );
    let has_think_block = segments.iter().any(|segment| {
        matches!(
            segment,
            lingxi_service::streaming_norm::ReservedTagSegment::Block { tag, content }
                if tag == "think" && content.contains("内部推理")
        )
    });
    assert!(
        has_think_block,
        "the one-shot split structures the think block: {segments:?}"
    );
    let literal_text: String = segments
        .iter()
        .filter_map(|segment| match segment {
            lingxi_service::streaming_norm::ReservedTagSegment::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        literal_text.contains("<think>不是标签</think>"),
        "the fenced literal stays text in the projection: {literal_text:?}"
    );
    assert!(literal_text.contains("完成"), "{literal_text:?}");

    stub.stop().await;
    teardown_boot(&boot).await;
}
