//! R05-T03 REVIEW scratch harness — independent verifier equipment.
//!
//! The double boundary is identical to the candidate's own service suite:
//! the ONLY double is a raw-TCP loopback HTTP stub (the outside world at the
//! far end of the wire). Every in-process link is production code (strict
//! config -> resolve_model_plane -> ConfigModelGateway -> CredentialService
//! -> protocol adapter -> real file tools -> durable events).
//!
//! The verifier-written scenarios here are deliberately adversarial in ways
//! the candidate's own suite is NOT:
//! - C05: the stub script is fixed BEFORE a runtime random nonce is written
//!   into the workspace file; the nonce is generated fresh per run and the
//!   captured next HTTP request must carry it (a preset answer fails).
//! - C03: tool results are rendered from an exchange whose completion order
//!   is REVERSED relative to the request order.
//! - C04: two sessions receive the SAME provider call id from the stub.
//! - C07: a REAL failing tool (read of a missing file) must ride the wire as
//!   an honest failure, cross-checked against the durable journal payload.
//! - C10: opaque blocks at non-canonical positions; cross-family opaque
//!   never echoed.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use lingxi_protocol::{EventPayload, KnownEventPayload};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── loopback stub provider ───────────────────────────────────────────────────

pub struct StubResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl StubResponse {
    pub fn json(body: serde_json::Value) -> Self {
        Self { status: 200, content_type: "application/json", body: body.to_string() }
    }
    pub fn sse(body: String) -> Self {
        Self { status: 200, content_type: "text/event-stream", body }
    }
}

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: serde_json::Value,
}

impl RecordedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

pub struct StubServer {
    pub addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    pub async fn start(responses: Vec<StubResponse>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_requests, task_responses) =
            (Arc::clone(&hits), Arc::clone(&requests), Arc::clone(&responses));
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
                    None => StubResponse {
                        status: 500,
                        content_type: "application/json",
                        body: r#"{"error":{"message":"stub script exhausted"}}"#.to_string(),
                    },
                };
                let reason = match response.status {
                    200 => "OK",
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
        Self { addr, hits, requests, shutdown: Some(shutdown), task }
    }

    pub fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn v1(&self) -> String {
        format!("{}/v1", self.origin())
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("requests").clone()
    }

    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task).await;
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> RecordedRequest {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
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
    let path = request_line.split_whitespace().nth(1).expect("request path").to_string();
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
        let read = tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
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
    RecordedRequest { path, headers, body }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

// ── the real composition-root boot ──────────────────────────────────────────

pub fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t03-review-{tag}-{}-{}",
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

pub fn owner_principal() -> lingxi_service::Principal {
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

pub struct PlaneBoot {
    pub state: ServiceState,
    pub home: PathBuf,
    pub workspace: PathBuf,
    pub config_path: PathBuf,
}

pub async fn boot_with_config(tag: &str, plane_json: &str) -> PlaneBoot {
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
    PlaneBoot { state, home, workspace, config_path }
}

pub async fn execute(state: &ServiceState, session: &str, input: &str) -> String {
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

pub async fn run_row(state: &ServiceState, run_id: &str) -> (String, Option<String>) {
    let status = state
        .storage()
        .query_one_text("SELECT status FROM runs WHERE run_id = ?1", vec![run_id.to_string()])
        .await
        .expect("query status")
        .expect("run row");
    let reason = state
        .storage()
        .query_one_text("SELECT terminal_reason FROM runs WHERE run_id = ?1", vec![run_id.to_string()])
        .await
        .expect("query reason");
    (status, reason)
}

pub async fn known_payloads(
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

pub async fn teardown_boot(boot: &PlaneBoot) {
    boot.state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&boot.home);
    let _ = std::fs::remove_dir_all(&boot.workspace);
    let _ = boot.config_path.parent().map(std::fs::remove_dir_all);
}

/// A runtime-generated nonce the stub cannot know: process id + nanos +
/// a counter, shaped to be greppable. Generated AFTER the stub script is
/// fixed, per invocation.
pub fn runtime_nonce(tag: &str) -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("NONCE-{tag}-{}-{nanos}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst))
}
