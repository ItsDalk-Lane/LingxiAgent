//! R05-T02 integration evidence: the credential authority and runtime
//! safety of the model plane, exercised through the REAL production wiring —
//! strict `--config` file → validated plane → `ServiceDeps` → bootstrap →
//! `CredentialService` (the single material exit, C01) → real loopback HTTP
//! to path-routed stubs (the outside world at the far end of the wire) →
//! durable honest outcomes.
//!
//! Double boundary: the ONLY doubles are the stub HTTP servers (a real
//! provider / OAuth authorization-server substitute at the far end of the
//! wire) and, in exactly one test (C06), a `StoreIo` write-failure injector
//! sitting under the real `CredentialStore` — the sanctioned fault seam.
//! Every in-process link — config validation, route resolution, credential
//! resolution/refresh/revoke, single-flight merge, persist-first write-back,
//! generation fences, management surface, event persistence — is the
//! production code path. All stubs bind 127.0.0.1 only.
//!
//! Check → test map (appendix B, R05-T02):
//! - C01  c01_all_purposes_resolve_through_the_single_credential_service
//! - C02  c02_concurrent_401s_merge_into_one_refresh_full_chain
//! - C03  c03_a_blocked_refresh_never_blocks_other_providers
//! - C04  c04_a_cancelled_waiter_leaves_the_shared_refresh_intact
//! - C05  c05_a_revoked_credential_never_resurrects_full_chain
//! - C06  c06_a_persist_failure_is_reported_honestly_full_chain
//! - C07  c07_* (five bounded-failure legs)
//! - C08  rust/crates/lingxi-adapters/tests/r05_t02_oauth_flows.rs (19 tests)
//! - C09  c09_marker_secret_never_reaches_durable_state_and_echoes_are_scrubbed
//! - C10  c10_a_redirect_is_never_followed_with_credentials
//! - C11  c11_store_concurrency_fidelity_and_restart_semantics
//! - C12  c12_management_surface_is_local_only_and_forgery_is_refused
//! - O1   o1_same_model_id_on_two_keyed_providers_never_crosses_material

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_adapters::models::config::ModelPlaneConfig;
use lingxi_adapters::models::credentials::{
    ApplicableAuth, CredentialError, ProviderCredentialPort as _,
};
use lingxi_adapters::models::openai_completions::OpenAiCompletionsAdapter;
use lingxi_kernel::model_exchange::{
    AuxiliarySlot, CredentialAuthKind, CredentialReference, ModelGatewayPort as _, ModelOperation,
    ModelRouteRequest, ProtocolFamily, ResolvedModelRoute, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::ProviderTurn;
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::{Principal as KernelPrincipal, RunContext};
use lingxi_protocol::{AttemptId, ModelCallId, RunId, SessionId};
use lingxi_service::credentials::store::{CredentialStore, FsStoreIo, StoreIo};
use lingxi_service::credentials::{
    CredentialHandle, CredentialService, ProductionRefreshDriver, OAUTH_REQUEST_TIMEOUT,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::watch;

// ── the loopback stub (the far end of the wire) ─────────────────────────────
//
// One task per connection (a gated response must never block the accept loop
// — several tests hold two connections parked at once). Responses are
// scripted PER PATH; an unknown path or an exhausted script is a LOUD 500,
// never an improvised reply.

struct RecordedRequest {
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

struct StubResponse {
    status: u16,
    body: String,
    content_type: &'static str,
    extra_headers: Vec<(String, String)>,
    gate: Option<watch::Receiver<bool>>,
}

impl StubResponse {
    fn immediate(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "application/json",
            extra_headers: Vec::new(),
            gate: None,
        }
    }

    /// R05-T04: the chat endpoint streams — the stub answers SSE.
    fn immediate_sse(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            content_type: "text/event-stream",
            extra_headers: Vec::new(),
            gate: None,
        }
    }

    fn gated(status: u16, body: impl Into<String>, gate: &watch::Sender<bool>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "application/json",
            extra_headers: Vec::new(),
            gate: Some(gate.subscribe()),
        }
    }

    fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            body: String::new(),
            content_type: "application/json",
            extra_headers: vec![("Location".to_string(), location.to_string())],
            gate: None,
        }
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
    async fn start(scripts: Vec<(&str, Vec<StubResponse>)>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let scripts: Arc<Mutex<BTreeMap<String, VecDeque<StubResponse>>>> = Arc::new(Mutex::new(
            scripts
                .into_iter()
                .map(|(path, queue)| (path.to_string(), VecDeque::from(queue)))
                .collect(),
        ));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn({
            let hits = Arc::clone(&hits);
            let requests = Arc::clone(&requests);
            let scripts = Arc::clone(&scripts);
            async move {
                loop {
                    let accepted = tokio::select! {
                        _ = &mut shutdown_rx => break,
                        accepted = listener.accept() => accepted,
                    };
                    let Ok((mut socket, _)) = accepted else { break };
                    // One task per connection: a gated response must never
                    // stall the accept loop.
                    tokio::spawn({
                        let hits = Arc::clone(&hits);
                        let requests = Arc::clone(&requests);
                        let scripts = Arc::clone(&scripts);
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            let recorded = read_request(&mut socket).await;
                            // Pop FIRST (the std guard must not live across
                            // the gate await — it is not Send).
                            let next = scripts
                                .lock()
                                .expect("scripts")
                                .get_mut(&recorded.path)
                                .and_then(VecDeque::pop_front);
                            requests.lock().expect("requests").push(recorded);
                            let response = match next {
                                Some(scripted) => {
                                    if let Some(mut gate) = scripted.gate {
                                        // Bounded: a test bug must surface as
                                        // a wrong answer, never as a hang.
                                        let _ = tokio::time::timeout(
                                            Duration::from_secs(20),
                                            gate.wait_for(|released| *released),
                                        )
                                        .await;
                                    }
                                    build_response(
                                        scripted.status,
                                        scripted.content_type,
                                        &scripted.extra_headers,
                                        &scripted.body,
                                    )
                                }
                                None => build_response(
                                    500,
                                    "application/json",
                                    &[],
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
            hits,
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The provider `endpoint` (the adapter appends `/chat/completions`).
    fn endpoint(&self) -> String {
        format!("{}/v1", self.url())
    }

    fn token_endpoint(&self) -> String {
        format!("{}/token", self.url())
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn hits_for(&self, path: &str) -> usize {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .filter(|r| r.path == path)
            .count()
    }

    fn requests_for(&self, path: &str) -> Vec<(Vec<(String, String)>, String)> {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .filter(|r| r.path == path)
            .map(|r| (r.headers.clone(), r.body.clone()))
            .collect()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

fn build_response(
    status: u16,
    content_type: &str,
    extra_headers: &[(String, String)],
    body: &str,
) -> String {
    let reason = match status {
        200 => "OK",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    };
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in extra_headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(body);
    response
}

/// Reads one HTTP/1.1 request (request line + headers + content-length body)
/// from the socket. The body is recorded RAW (the chat endpoint speaks JSON,
/// the token endpoint speaks form-encoding — the stub never parses either).
/// Malformed or oversized input panics — the stub is test equipment,
/// loudness is the point.
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
    let mut headers = Vec::new();
    let mut content_length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().expect("numeric content-length");
            }
            headers.push((name, value));
        }
    }
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
    let body = String::from_utf8(raw[body_start..body_start + content_length].to_vec())
        .expect("stub: body utf8");
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

// ── canned answers ──────────────────────────────────────────────────────────

const CHAT_PATH: &str = "/v1/chat/completions";
const TOKEN_PATH: &str = "/token";

/// OAuth expiry ledger values for the seeded store.
const UNEXPIRED: u64 = 4_000_000_000_000; // far future — resolve uses it directly
const EXPIRED: u64 = 1_000; // 1970 — resolve refreshes before any request

fn final_response(text: &str) -> StubResponse {
    // R05-T04: production streams — delta → finish_reason → usage → [DONE].
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-stub",
            "model": "stub-model",
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
        serde_json::json!({
            "id": "chatcmpl-stub",
            "choices": [],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3}
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse::immediate_sse(body)
}

fn unauthorized_response(detail: &str) -> StubResponse {
    StubResponse::immediate(
        401,
        serde_json::json!({"error": {"message": detail}}).to_string(),
    )
}

fn gated_unauthorized(gate: &watch::Sender<bool>) -> StubResponse {
    StubResponse::gated(
        401,
        serde_json::json!({"error": {"message": "stale token"}}).to_string(),
        gate,
    )
}

fn token_response(access: &str, refresh: &str) -> StubResponse {
    StubResponse::immediate(
        200,
        serde_json::json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_in": 3600
        })
        .to_string(),
    )
}

fn gated_token_response(access: &str, refresh: &str, gate: &watch::Sender<bool>) -> StubResponse {
    StubResponse::gated(
        200,
        serde_json::json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_in": 3600
        })
        .to_string(),
        gate,
    )
}

// ── plane builders (config-file fragments) ──────────────────────────────────

fn plane_api_key(endpoint: &str, key: &str) -> String {
    format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": {key}}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "stub-model"}}}}"#,
        endpoint = serde_json::to_string(endpoint).expect("json"),
        key = serde_json::to_string(key).expect("json"),
    )
}

fn plane_oauth(endpoint: &str, token_endpoint: &str) -> String {
    format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "oauth", "flow": "deviceCode", "clientId": "client-r05t02",
                    "tokenEndpoint": {token_endpoint},
                    "deviceAuthorizationEndpoint": {device_endpoint}}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "stub-model"}}}}"#,
        endpoint = serde_json::to_string(endpoint).expect("json"),
        token_endpoint = serde_json::to_string(token_endpoint).expect("json"),
        device_endpoint = serde_json::to_string(&format!("{token_endpoint}/device")).expect("json"),
    )
}

/// chat → main (oauth), title → aux (apiKey) — the two-provider plane.
fn plane_oauth_plus_static(endpoint: &str, token_endpoint: &str, aux_key: &str) -> String {
    format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "oauth", "flow": "deviceCode", "clientId": "client-r05t02",
                    "tokenEndpoint": {token_endpoint},
                    "deviceAuthorizationEndpoint": {device_endpoint}}}
            }},
            "aux": {{
                "protocol": "openai-completions",
                "endpoint": "http://127.0.0.1:9/unused",
                "auth": {{"kind": "apiKey", "apiKey": {aux_key}}}
            }}
        }},
        "models": {{
            "chat": {{"provider": "main", "model": "stub-model"}},
            "title": {{"provider": "aux", "model": "aux-model"}}
        }}"#,
        endpoint = serde_json::to_string(endpoint).expect("json"),
        token_endpoint = serde_json::to_string(token_endpoint).expect("json"),
        device_endpoint = serde_json::to_string(&format!("{token_endpoint}/device")).expect("json"),
        aux_key = serde_json::to_string(aux_key).expect("json"),
    )
}

/// Seeds `{runtime_dir}/credentials.json` (the v1 store shape — the test
/// writes it directly, doubling as file-format evidence).
fn seed_store_file(runtime_dir: &Path, provider: &str, access: &str, refresh: &str, expiry: u64) {
    let json = serde_json::json!({
        "version": 1,
        "providers": {
            provider: {
                "tokens": {
                    "accessToken": access,
                    "refreshToken": refresh,
                    "expiresAtUnixMs": expiry,
                }
            }
        }
    });
    std::fs::write(
        runtime_dir.join("credentials.json"),
        serde_json::to_string_pretty(&json).expect("store json"),
    )
    .expect("seed credential store");
}

fn read_store_file(runtime_dir: &Path) -> serde_json::Value {
    let raw = std::fs::read(runtime_dir.join("credentials.json")).expect("read credential store");
    serde_json::from_slice(&raw).expect("credential store is valid JSON")
}

// ── harness ─────────────────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t02-{tag}-{}-{}",
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
    config_root: PathBuf,
    config_path: PathBuf,
    runtime_dir: PathBuf,
    plane_json: String,
}

/// Boots the REAL composition-root wiring exactly like the binary does, with
/// an optional pre-seeded credential store file (OAuth token material's only
/// legitimate on-disk home).
async fn boot(
    tag: &str,
    plane_json: &str,
    store_seed: Option<(&str, &str, &str, u64)>,
) -> PlaneBoot {
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
    if let Some((provider, access, refresh, expiry)) = store_seed {
        seed_store_file(&layout.runtime_dir, provider, access, refresh, expiry);
    }
    let file = lingxi_service::config::read_service_config(&config_path).expect("service config");
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .expect("plane resolves")
            .expect("plane present");
    let runtime_dir = layout.runtime_dir.clone();
    let credential_service = Arc::new(
        CredentialService::bootstrap(
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
        config_root,
        config_path,
        runtime_dir,
        plane_json: plane_json.to_string(),
    }
}

/// Boots with a caller-assembled credential service (the C06 fault-injection
/// seam): the factory runs AFTER the layout exists, receiving the validated
/// plane and the runtime dir, so the injected store lives at the REAL store
/// path of the booted home. The refresh driver stays the REAL production
/// driver (real HTTP to the stub token endpoint); only the store IO is
/// wrapped.
async fn boot_with_credential_service(
    tag: &str,
    plane_json: &str,
    make_service: impl FnOnce(&ModelPlaneConfig, &Path) -> Arc<CredentialService>,
) -> PlaneBoot {
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
    let credential_service = make_service(&plane, &layout.runtime_dir);
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
        config_root,
        config_path,
        runtime_dir,
        plane_json: plane_json.to_string(),
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

/// Polls the stub until `path` has been hit `expected` times (bounded — a
/// missing request is a loud failure, not a hang).
async fn wait_until_path_hits(stub: &StubServer, path: &str, expected: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while stub.hits_for(path) < expected {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "stub never received {expected} requests on {path} (has {})",
            stub.hits_for(path)
        )
    });
}

async fn teardown(boot: PlaneBoot) {
    boot.state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&boot.home);
    let _ = std::fs::remove_dir_all(&boot.workspace);
    let _ = std::fs::remove_dir_all(&boot.config_root);
}

// ── the real management surface over loopback ───────────────────────────────

struct ManagementSurface {
    addr: SocketAddr,
    token: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}

async fn serve_management(state: &ServiceState) -> ManagementSurface {
    let token = state.auth().local_token();
    let state_for_server = state.clone();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let task = tokio::spawn(async move {
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
    ManagementSurface {
        addr,
        token,
        stop: Some(stop),
        task,
    }
}

impl ManagementSurface {
    async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(10), &mut self.task).await;
    }
}

/// Minimal real-HTTP client (loopback, one request per connection) for the
/// management surface; `bearer: None` = an UNAUTHENTICATED request.
async fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
) -> (u16, serde_json::Value) {
    let exchanged = tokio::time::timeout(Duration::from_secs(10), async {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to real service");
        let auth = match bearer {
            Some(token) => format!("Authorization: Bearer {token}\r\n"),
            None => String::new(),
        };
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Length: 0\r\nConnection: close\r\n\r\n"
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

// ── C01: one credential service for every purpose ───────────────────────────

#[tokio::test]
async fn c01_all_purposes_resolve_through_the_single_credential_service() {
    let stub = StubServer::start(vec![(CHAT_PATH, vec![final_response("c01 answer")])]).await;
    // chat → main (apiKey); title (an auxiliary slot) → aux (authHeader).
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "sk-c01-main"}}
            }},
            "aux": {{
                "protocol": "openai-completions",
                "endpoint": "http://127.0.0.1:9/unused",
                "auth": {{"kind": "authHeader", "header": "X-Custom-Auth", "value": "aux-secret-c01"}}
            }}
        }},
        "models": {{
            "chat": {{"provider": "main", "model": "stub-model"}},
            "title": {{"provider": "aux", "model": "aux-model"}}
        }}"#,
        endpoint = serde_json::to_string(&stub.endpoint()).expect("json"),
    );
    let boot = boot("c01", &plane_json, None).await;
    let management = serve_management(&boot.state).await;

    // The main chat path: the material reached the wire as the bearer header.
    let run_id = execute(&boot.state, "c01 hello").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    let chat = stub.requests_for(CHAT_PATH);
    assert_eq!(chat.len(), 1, "exactly one provider call");
    assert_eq!(
        chat[0]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer sk-c01-main"),
        "the config-seeded material flowed through the credential service"
    );

    // An auxiliary slot resolves its route through the SAME gateway and its
    // material through the SAME credential service (the only material exit).
    let aux_route = boot
        .state
        .model_gateway()
        .expect("gateway")
        .resolve_route(&ModelRouteRequest::for_operation(
            ModelOperation::Auxiliary(AuxiliarySlot::Title),
        ))
        .expect("aux route resolves");
    assert_eq!(aux_route.provider, "aux");
    assert_eq!(aux_route.credential.auth, CredentialAuthKind::AuthHeader);
    let aux_auth = boot
        .state
        .credential_service()
        .expect("credential service")
        .resolve(&aux_route)
        .await
        .expect("aux material resolves");
    assert_eq!(
        aux_auth,
        ApplicableAuth::Header {
            name: "X-Custom-Auth".to_string(),
            value: "aux-secret-c01".to_string()
        },
        "the auxiliary purpose drew its material from the same service"
    );

    // The management status surface is material-free (C09-adjacent honesty).
    let (status, body) = http_request(
        management.addr,
        "GET",
        "/lingxi/v1/models/credentials",
        Some(&management.token),
    )
    .await;
    assert_eq!(status, 200, "credentials status: {body}");
    let text = body.to_string();
    assert!(
        text.contains("\"main\"") && text.contains("\"aux\""),
        "{text}"
    );
    assert!(
        text.contains("\"apiKey\"") && text.contains("\"authHeader\""),
        "{text}"
    );
    assert!(
        !text.contains("sk-c01-main") && !text.contains("aux-secret-c01"),
        "the status surface must never carry material: {text}"
    );
    let providers = body["providers"].as_array().expect("providers array");
    for provider in providers {
        assert_eq!(provider["state"], "ready");
        assert_eq!(provider["persisted"], true);
    }

    management.stop().await;
    stub.stop().await;
    teardown(boot).await;
}

// ── C02: concurrent 401s merge into exactly one refresh ─────────────────────

#[tokio::test]
async fn c02_concurrent_401s_merge_into_one_refresh_full_chain() {
    let chat_gate = watch::channel(false);
    let stub = StubServer::start(vec![
        (
            CHAT_PATH,
            vec![
                gated_unauthorized(&chat_gate.0),
                gated_unauthorized(&chat_gate.0),
                final_response("alpha refreshed answer"),
                final_response("beta refreshed answer"),
            ],
        ),
        (TOKEN_PATH, vec![token_response("at-fresh", "rt-2")]),
    ])
    .await;
    let plane_json = plane_oauth(&stub.endpoint(), &stub.token_endpoint());
    // Pre-seeded UNEXPIRED tokens: both runs dispatch with at-old first.
    let boot = boot(
        "c02",
        &plane_json,
        Some(("main", "at-old", "rt-1", UNEXPIRED)),
    )
    .await;

    // Two concurrent runs (separate sessions) park inside the stub's 401s.
    let state_a = boot.state.clone();
    let run_a =
        tokio::spawn(async move { execute_on(&state_a, "sess_local_alpha", "alpha task").await });
    let state_b = boot.state.clone();
    let run_b =
        tokio::spawn(async move { execute_on(&state_b, "sess_local_beta", "beta task").await });
    wait_until_path_hits(&stub, CHAT_PATH, 2).await;
    chat_gate.0.send(true).expect("release the 401 gate");

    let (run_a, run_b) = tokio::join!(run_a, run_b);
    let run_a = run_a.expect("run a task");
    let run_b = run_b.expect("run b task");
    for run_id in [&run_a, &run_b] {
        let (status, reason) = run_row(&boot.state, run_id).await;
        assert_eq!(
            status, "completed",
            "run {run_id} completed after the merged refresh"
        );
        assert_eq!(reason.as_deref(), Some("completed.with_final"));
    }

    // THE core assertion: N concurrent 401s produced exactly ONE refresh.
    assert_eq!(
        stub.hits_for(TOKEN_PATH),
        1,
        "the concurrent 401s merged into a single refresh"
    );
    assert_eq!(stub.hits_for(CHAT_PATH), 4, "two originals + two retries");
    let chat = stub.requests_for(CHAT_PATH);
    assert_eq!(
        chat[0]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer at-old")
    );
    assert_eq!(
        chat[1]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer at-old")
    );
    for retry in &chat[2..] {
        assert_eq!(
            retry
                .0
                .iter()
                .find(|(n, _)| n == "authorization")
                .map(|(_, v)| v.as_str()),
            Some("Bearer at-fresh"),
            "every retry carries the freshly minted token"
        );
    }
    // The refresh authenticated with the OLD refresh token in the form body,
    // never through a header.
    let token = stub.requests_for(TOKEN_PATH);
    assert_eq!(token.len(), 1);
    assert!(
        token[0].1.contains("grant_type=refresh_token"),
        "{}",
        token[0].1
    );
    assert!(token[0].1.contains("refresh_token=rt-1"), "{}", token[0].1);
    assert!(
        token[0]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .is_none(),
        "the refresh grant travels in the body, never a header"
    );
    // The minted token set is durable (refresh-token rollover included).
    let store = read_store_file(&boot.runtime_dir);
    assert_eq!(
        store["providers"]["main"]["tokens"]["accessToken"],
        "at-fresh"
    );
    assert_eq!(store["providers"]["main"]["tokens"]["refreshToken"], "rt-2");

    stub.stop().await;
    teardown(boot).await;
}

// ── C03: a blocked refresh never blocks another provider ────────────────────

#[tokio::test]
async fn c03_a_blocked_refresh_never_blocks_other_providers() {
    let token_gate = watch::channel(false);
    let stub = StubServer::start(vec![
        (CHAT_PATH, vec![final_response("c03 refreshed answer")]),
        (
            TOKEN_PATH,
            vec![gated_token_response("at-fresh", "rt-2", &token_gate.0)],
        ),
    ])
    .await;
    let plane_json =
        plane_oauth_plus_static(&stub.endpoint(), &stub.token_endpoint(), "sk-c03-aux");
    // main's seeded token is EXPIRED: the run's resolve goes straight into a
    // (gated) refresh flight before any provider request.
    let boot = boot(
        "c03",
        &plane_json,
        Some(("main", "at-old", "rt-1", EXPIRED)),
    )
    .await;

    let state_a = boot.state.clone();
    let run_a =
        tokio::spawn(async move { execute_on(&state_a, "sess_local_alpha", "alpha task").await });
    wait_until_path_hits(&stub, TOKEN_PATH, 1).await;
    assert!(
        !run_a.is_finished(),
        "run A is genuinely parked inside the gated refresh"
    );

    // Provider B (static) resolves through the SAME credential service
    // without waiting for A's network — bounded, immediate.
    let aux_route = boot
        .state
        .model_gateway()
        .expect("gateway")
        .resolve_route(&ModelRouteRequest::for_operation(
            ModelOperation::Auxiliary(AuxiliarySlot::Title),
        ))
        .expect("aux route");
    let resolved = tokio::time::timeout(Duration::from_secs(2), async {
        boot.state
            .credential_service()
            .expect("credential service")
            .resolve(&aux_route)
            .await
    })
    .await
    .expect("provider B resolved while A's refresh was blocked")
    .expect("aux material");
    assert_eq!(resolved, ApplicableAuth::Bearer("sk-c03-aux".to_string()));

    // Releasing A's refresh lets A finish with the fresh token.
    token_gate.0.send(true).expect("release the token gate");
    let run_a = tokio::time::timeout(Duration::from_secs(15), run_a)
        .await
        .expect("run A settles")
        .expect("run A task");
    let (status, _) = run_row(&boot.state, &run_a).await;
    assert_eq!(status, "completed");
    assert_eq!(stub.hits_for(TOKEN_PATH), 1);
    assert_eq!(stub.hits_for(CHAT_PATH), 1);
    let chat = stub.requests_for(CHAT_PATH);
    assert_eq!(
        chat[0]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer at-fresh"),
        "the resolve-time refresh led the first provider call"
    );

    stub.stop().await;
    teardown(boot).await;
}

// ── C04: a cancelled waiter leaves the shared refresh intact ────────────────

#[tokio::test]
async fn c04_a_cancelled_waiter_leaves_the_shared_refresh_intact() {
    let chat_gate = watch::channel(false);
    let token_gate = watch::channel(false);
    let stub = StubServer::start(vec![
        (
            CHAT_PATH,
            vec![
                gated_unauthorized(&chat_gate.0),
                gated_unauthorized(&chat_gate.0),
                final_response("survivor answer"),
            ],
        ),
        (
            TOKEN_PATH,
            vec![gated_token_response("at-fresh", "rt-2", &token_gate.0)],
        ),
    ])
    .await;
    let plane_json = plane_oauth(&stub.endpoint(), &stub.token_endpoint());
    let boot = boot(
        "c04",
        &plane_json,
        Some(("main", "at-old", "rt-1", UNEXPIRED)),
    )
    .await;

    // Two runs share ONE 401-triggered refresh flight.
    let state_a = boot.state.clone();
    let run_a =
        tokio::spawn(async move { execute_on(&state_a, "sess_local_alpha", "alpha task").await });
    let state_b = boot.state.clone();
    let run_b =
        tokio::spawn(async move { execute_on(&state_b, "sess_local_beta", "beta task").await });
    wait_until_path_hits(&stub, CHAT_PATH, 2).await;
    chat_gate.0.send(true).expect("release the 401 gate");
    // Both runs are now waiting on the single gated refresh flight.
    wait_until_path_hits(&stub, TOKEN_PATH, 1).await;

    // Cancel run A mid-wait; the flight and run B must be undisturbed.
    let run_a_id = boot
        .state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE session_id = 'sess_local_alpha'",
            vec![],
        )
        .await
        .expect("run a row")
        .expect("run a exists");
    boot.state
        .sessions()
        .cancel_run_for(
            boot.state.storage().as_ref(),
            boot.state.runs(),
            &owner_principal(),
            &run_a_id,
        )
        .await
        .expect("cancel accepted");
    token_gate.0.send(true).expect("release the token gate");

    let settled_a = tokio::time::timeout(Duration::from_secs(15), run_a)
        .await
        .expect("run A settles")
        .expect("run A task");
    assert_eq!(settled_a, run_a_id);
    let run_b = tokio::time::timeout(Duration::from_secs(15), run_b)
        .await
        .expect("run B settles")
        .expect("run B task");

    let (status_a, _) = run_row(&boot.state, &run_a_id).await;
    assert_eq!(
        status_a, "cancelled",
        "the cancelled waiter settled cancelled"
    );
    let (status_b, reason_b) = run_row(&boot.state, &run_b).await;
    assert_eq!(status_b, "completed");
    assert_eq!(reason_b.as_deref(), Some("completed.with_final"));

    // Exactly one refresh; the cancelled run NEVER retried (3 chat hits:
    // A's original, B's original, B's retry).
    assert_eq!(stub.hits_for(TOKEN_PATH), 1, "one shared refresh");
    assert_eq!(
        stub.hits_for(CHAT_PATH),
        3,
        "the cancelled run never retried after the refresh"
    );
    let chat = stub.requests_for(CHAT_PATH);
    assert_eq!(
        chat[2]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer at-fresh"),
        "the surviving waiter completed with the fresh token"
    );
    let store = read_store_file(&boot.runtime_dir);
    assert_eq!(
        store["providers"]["main"]["tokens"]["accessToken"],
        "at-fresh"
    );

    stub.stop().await;
    teardown(boot).await;
}

// ── C05: a revoked credential never resurrects ──────────────────────────────

#[tokio::test]
async fn c05_a_revoked_credential_never_resurrects_full_chain() {
    let token_gate = watch::channel(false);
    // Any chat hit is a loud 500 (empty script) — the run must fail BEFORE
    // any provider call because its token was revoked mid-refresh.
    let stub = StubServer::start(vec![
        (CHAT_PATH, vec![]),
        (
            TOKEN_PATH,
            vec![gated_token_response("at-late", "rt-late", &token_gate.0)],
        ),
    ])
    .await;
    let plane_json = plane_oauth(&stub.endpoint(), &stub.token_endpoint());
    let boot = boot(
        "c05",
        &plane_json,
        Some(("main", "at-old", "rt-1", EXPIRED)),
    )
    .await;
    let management = serve_management(&boot.state).await;

    // The run parks inside the gated refresh (expired seed → resolve refreshes).
    let state_a = boot.state.clone();
    let run_a =
        tokio::spawn(async move { execute_on(&state_a, "sess_local_alpha", "alpha task").await });
    wait_until_path_hits(&stub, TOKEN_PATH, 1).await;

    // Revoke through the REAL management surface while the refresh is in
    // flight; then release the late token.
    let (status, body) = http_request(
        management.addr,
        "POST",
        "/lingxi/v1/models/credentials/main/revoke",
        Some(&management.token),
    )
    .await;
    assert_eq!(status, 200, "revoke accepted: {body}");
    assert_eq!(body["ok"], true);
    token_gate.0.send(true).expect("release the late token");

    let run_a = tokio::time::timeout(Duration::from_secs(15), run_a)
        .await
        .expect("run settles")
        .expect("run task");
    let (status, reason) = run_row(&boot.state, &run_a).await;
    assert_eq!(
        status, "failed",
        "a revoked credential fails the run honestly"
    );
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));

    // The generation fence discarded the late token: it never landed in
    // memory (status: revoked) nor in the store (no row, no at-late bytes),
    // and no retry was started against the provider.
    let (status, body) = http_request(
        management.addr,
        "GET",
        "/lingxi/v1/models/credentials",
        Some(&management.token),
    )
    .await;
    assert_eq!(status, 200);
    let main = &body["providers"][0];
    assert_eq!(main["provider"], "main");
    assert_eq!(main["state"], "revoked", "{main}");
    assert_eq!(main["generation"], 2, "the revoke bumped the generation");
    assert_eq!(main["refreshInFlight"], false);
    let store = read_store_file(&boot.runtime_dir);
    assert!(
        store["providers"].get("main").is_none(),
        "the revoke deleted the store row: {store}"
    );
    assert!(
        !store.to_string().contains("at-late") && !store.to_string().contains("rt-late"),
        "the late token never reached the store: {store}"
    );
    assert_eq!(
        stub.hits_for(CHAT_PATH),
        0,
        "no provider call was ever attempted with a revoked credential"
    );

    // The revocation is journaled.
    let management_log =
        std::fs::read(boot.runtime_dir.join("management.json")).expect("audit file");
    let management_log: serde_json::Value =
        serde_json::from_slice(&management_log).expect("audit json");
    let audit = management_log["audit"].as_array().expect("audit array");
    assert!(
        audit
            .iter()
            .any(|entry| entry["action"] == "models.credentials.revoke"
                && entry["metadata"]["provider"] == "main"),
        "the revoke is journaled: {audit:?}"
    );

    management.stop().await;
    stub.stop().await;
    teardown(boot).await;
}

// ── C06: a persist failure is reported honestly ─────────────────────────────

/// The fault-injection seam: reads go to the real store file; writes fail
/// while the flag is raised.
struct FlakyStoreIo {
    inner: FsStoreIo,
    fail_writes: Arc<AtomicBool>,
}

impl StoreIo for FlakyStoreIo {
    fn read(&self) -> Result<Option<String>, String> {
        self.inner.read()
    }
    fn write(&self, content: &str) -> Result<(), String> {
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err("injected credential-store write failure (C06)".to_string());
        }
        self.inner.write(content)
    }
}

#[tokio::test]
async fn c06_a_persist_failure_is_reported_honestly_full_chain() {
    let stub = StubServer::start(vec![
        (CHAT_PATH, vec![final_response("c06 answer")]),
        (TOKEN_PATH, vec![token_response("at-fresh", "rt-2")]),
    ])
    .await;
    let plane_json = plane_oauth(&stub.endpoint(), &stub.token_endpoint());

    // Assemble the credential service inside the boot: real plane, real
    // production refresh driver, the REAL store file of the booted home —
    // only the write primitive fails (reads stay honest).
    let boot = boot_with_credential_service("c06", &plane_json, |plane, runtime_dir| {
        seed_store_file(runtime_dir, "main", "at-old", "rt-1", EXPIRED);
        let store = CredentialStore::load(Arc::new(FlakyStoreIo {
            inner: FsStoreIo::new(runtime_dir),
            fail_writes: Arc::new(AtomicBool::new(true)),
        }))
        .expect("store loads (reads are honest)");
        Arc::new(CredentialService::new(
            plane,
            Some(store),
            Arc::new(
                ProductionRefreshDriver::new(OAUTH_REQUEST_TIMEOUT).expect("production driver"),
            ),
            Arc::new(lingxi_service::inject::SystemClock),
        ))
    })
    .await;
    let management = serve_management(&boot.state).await;

    // The refresh mints; the persist FAILS; the in-memory credential still
    // serves the run (the minted token is real) — and the failure is
    // reported, never claimed as safely saved.
    let run_id = execute(&boot.state, "c06 task").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    let chat = stub.requests_for(CHAT_PATH);
    assert_eq!(
        chat[0]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer at-fresh"),
        "the minted token is real and usable in memory"
    );

    // The honesty surface: persisted == false, the failure named.
    let (status, body) = http_request(
        management.addr,
        "GET",
        "/lingxi/v1/models/credentials",
        Some(&management.token),
    )
    .await;
    assert_eq!(status, 200);
    let main = &body["providers"][0];
    assert_eq!(main["state"], "ready");
    assert_eq!(
        main["persisted"], false,
        "a failed persist is never reported as safely saved: {main}"
    );
    assert!(
        main["lastPersistFailure"]
            .as_str()
            .expect("failure detail")
            .contains("injected credential-store write failure"),
        "{main}"
    );
    assert_eq!(main["generation"], 2);
    assert!(main["lastRefreshAtUnixMs"].is_u64());

    // The durable state is still the OLD credential — a restart recovers it.
    let store = read_store_file(&boot.runtime_dir);
    assert_eq!(
        store["providers"]["main"]["tokens"]["accessToken"],
        "at-old"
    );
    assert_eq!(store["providers"]["main"]["tokens"]["refreshToken"], "rt-1");
    assert!(!store.to_string().contains("at-fresh"));
    let plane_again = ModelPlaneConfig::parse_and_validate(&format!("{{{}}}", boot.plane_json))
        .expect("valid plane");
    let restarted = CredentialService::bootstrap(
        &plane_again,
        &boot.runtime_dir,
        Arc::new(lingxi_service::inject::SystemClock),
    )
    .expect("restart boots from the old durable state");
    let restarted_status = restarted.status().await;
    let main = restarted_status
        .iter()
        .find(|p| p.provider == "main")
        .expect("main status");
    assert_eq!(main.state, "ready");
    assert_eq!(
        main.expires_at_unix_ms,
        Some(EXPIRED),
        "the restart sees the OLD recoverable token set, never the lost mint"
    );

    management.stop().await;
    stub.stop().await;
    teardown(boot).await;
}

// ── C07: refresh failures and retry bounds ──────────────────────────────────

/// Boot an oauth provider with seeded tokens against the given stub.
async fn boot_oauth(tag: &str, stub: &StubServer, seed: Option<(&str, &str, u64)>) -> PlaneBoot {
    let plane_json = plane_oauth(&stub.endpoint(), &stub.token_endpoint());
    boot(
        tag,
        &plane_json,
        seed.map(|(access, refresh, expiry)| ("main", access, refresh, expiry)),
    )
    .await
}

#[tokio::test]
async fn c07_a_second_401_after_refresh_never_retriggers_a_refresh() {
    let stub = StubServer::start(vec![
        (
            CHAT_PATH,
            vec![
                unauthorized_response("stale"),
                unauthorized_response("still stale"),
            ],
        ),
        (TOKEN_PATH, vec![token_response("at-fresh", "rt-2")]),
    ])
    .await;
    let boot = boot_oauth("c07a", &stub, Some(("at-old", "rt-1", UNEXPIRED))).await;

    let run_id = execute(&boot.state, "always unauthorized").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        stub.hits_for(CHAT_PATH),
        2,
        "the original call + EXACTLY ONE retry — no 401 loop"
    );
    assert_eq!(
        stub.hits_for(TOKEN_PATH),
        1,
        "exactly one refresh; the second 401 is terminal"
    );
    let chat = stub.requests_for(CHAT_PATH);
    assert_eq!(
        chat[1]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer at-fresh")
    );

    stub.stop().await;
    teardown(boot).await;
}

#[tokio::test]
async fn c07_invalid_grant_is_terminal_reauthorization_never_a_retry() {
    let stub = StubServer::start(vec![
        (CHAT_PATH, vec![unauthorized_response("stale")]),
        (
            TOKEN_PATH,
            vec![StubResponse::immediate(
                400,
                r#"{"error":"invalid_grant","error_description":"refresh token revoked"}"#,
            )],
        ),
    ])
    .await;
    let boot = boot_oauth("c07b", &stub, Some(("at-old", "rt-1", UNEXPIRED))).await;

    let run_id = execute(&boot.state, "dead grant").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(stub.hits_for(CHAT_PATH), 1, "no retry after a dead grant");
    assert_eq!(stub.hits_for(TOKEN_PATH), 1, "exactly one refresh attempt");

    stub.stop().await;
    teardown(boot).await;
}

#[tokio::test]
async fn c07_oauth_without_a_login_fails_before_any_request() {
    // No store seed: the provider has a flow descriptor but no token.
    let stub = StubServer::start(vec![(CHAT_PATH, vec![]), (TOKEN_PATH, vec![])]).await;
    let boot = boot_oauth("c07c", &stub, None).await;

    let run_id = execute(&boot.state, "never logged in").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        stub.hits(),
        0,
        "NotLoggedIn fails loudly before ANY network request"
    );

    stub.stop().await;
    teardown(boot).await;
}

#[tokio::test]
async fn c07_a_transient_refresh_failure_is_bounded_by_the_attempt_policy() {
    let stub = StubServer::start(vec![
        (
            CHAT_PATH,
            vec![
                unauthorized_response("stale"),
                unauthorized_response("stale again"),
                unauthorized_response("still stale"),
                unauthorized_response("a fourth hit would prove an unbounded loop"),
                unauthorized_response("a fifth hit would prove an unbounded loop"),
            ],
        ),
        (
            TOKEN_PATH,
            vec![
                StubResponse::immediate(
                    503,
                    r#"{"error":"server_error","error_description":"auth server unavailable"}"#,
                ),
                StubResponse::immediate(
                    503,
                    r#"{"error":"server_error","error_description":"auth server unavailable"}"#,
                ),
                StubResponse::immediate(
                    503,
                    r#"{"error":"server_error","error_description":"auth server unavailable"}"#,
                ),
            ],
        ),
    ])
    .await;
    let boot = boot_oauth("c07d", &stub, Some(("at-old", "rt-1", UNEXPIRED))).await;

    let run_id = execute(&boot.state, "transient auth outage").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    // The run's bounded attempt policy (the R05-T05 pre-registered default
    // `transport_retry_max_attempts` = 3: one initial + two retries)
    // contains the transient classification: 3 provider calls, 3 refresh
    // attempts, stop.
    assert_eq!(
        stub.hits_for(CHAT_PATH),
        3,
        "bounded: one call per attempt, three attempts"
    );
    assert_eq!(
        stub.hits_for(TOKEN_PATH),
        3,
        "bounded: one refresh per attempt, then the run fails"
    );

    stub.stop().await;
    teardown(boot).await;
}

#[tokio::test]
async fn c07_a_static_key_401_is_terminal_without_any_refresh() {
    let stub = StubServer::start(vec![(
        CHAT_PATH,
        vec![
            unauthorized_response("bad key"),
            unauthorized_response("a second hit would prove a retry"),
        ],
    )])
    .await;
    let plane_json = plane_api_key(&stub.endpoint(), "sk-c07-static");
    let boot = boot("c07e", &plane_json, None).await;

    let run_id = execute(&boot.state, "static key rejected").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        stub.hits(),
        1,
        "a static credential is NotRefreshable: no retry, no refresh endpoint exists"
    );

    stub.stop().await;
    teardown(boot).await;
}

// ── C09: secrets never cross the host boundary ──────────────────────────────

/// The marker deliberately matches NO provider-prefix and NO high-entropy
/// pattern the log redactor knows — if it survives anywhere, the adapter's
/// exact-match scrub (the primary defense) is what failed.
const MARKER: &str = "MARKER-key-0123456789abcdef";

/// Every file under `root` whose bytes contain the marker (recursive).
fn files_containing(root: &Path, marker: &str) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                if bytes
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes())
                {
                    hits.push(path);
                }
            }
        }
    }
    hits
}

fn adapter_ctx(tag: &str) -> RunContext {
    RunContext {
        principal: KernelPrincipal::LocalUser,
        session_id: SessionId::new(format!("sess_t02_{tag}")),
        run_id: RunId::new(format!("run-t02-{tag}")),
        attempt: AttemptId::new(format!("a-{tag}-1")),
        generation: 1,
    }
}

fn adapter_route(endpoint: &str) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: "main".to_string(),
        model: "stub-model".to_string(),
        operation: ModelOperation::Chat,
        protocol: ProtocolFamily::OpenAiCompletions,
        endpoint: endpoint.to_string(),
        credential: CredentialReference {
            provider: "main".to_string(),
            auth: CredentialAuthKind::ApiKey,
        },
        config_generation: 1,
        group_id: None,
    }
}

#[tokio::test]
async fn c09_marker_secret_never_reaches_durable_state_and_echoes_are_scrubbed() {
    let echo = || {
        unauthorized_response(&format!(
            "the key {MARKER} was rejected by the upstream auth gateway"
        ))
    };
    let stub = StubServer::start(vec![(CHAT_PATH, vec![echo(), echo()])]).await;
    let plane_json = plane_api_key(&stub.endpoint(), MARKER);
    let boot = boot("c09", &plane_json, None).await;

    // The full chain: the provider ECHOES the secret in its 401 body; the
    // run fails honestly.
    let run_id = execute(&boot.state, "c09 marker run").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));

    // The scrubbed error text itself (the production adapter, real HTTP):
    // the in-play material is replaced BEFORE the excerpt can travel.
    let adapter = OpenAiCompletionsAdapter::new(SchemaBudget::default()).expect("adapter");
    let input = ModelTurnInputFor::chat("c09 direct leg");
    let result = adapter
        .execute_chat(
            &adapter_ctx("c09"),
            &ModelCallId::new("run-t02-c09-mc1"),
            &input,
            &adapter_route(&stub.endpoint()),
            &ApplicableAuth::Bearer(MARKER.to_string()),
            // R05-T05: the direct-adapter legs pin the raw wire (no compat).
            None,
            &lingxi_adapters::models::streaming::NullDeltaSink,
        )
        .await;
    let ProviderTurn::Failed { error, .. } = result.turn else {
        panic!("the echo stub answers 401 — the turn must fail");
    };
    assert!(
        !error.message.contains(MARKER),
        "the error excerpt must be scrubbed of the in-play material: {}",
        error.message
    );
    assert!(
        error.message.contains("[redacted]"),
        "the scrub is visible: {}",
        error.message
    );
    assert!(
        error
            .message
            .contains("was rejected by the upstream auth gateway"),
        "the non-secret echo survives: {}",
        error.message
    );

    // Durable-state scan: events, messages, run rows, journal files, the
    // management journal, the runtime dir — NOTHING under the service home
    // may carry the marker. (The config file lives outside the home tree —
    // the whitelisted material location — and static keys never reach the
    // credential store at all.)
    assert!(
        files_containing(&boot.home, MARKER).is_empty(),
        "marker leak under home: {:?}",
        files_containing(&boot.home, MARKER)
    );
    assert!(
        !boot.runtime_dir.join("credentials.json").exists(),
        "a static key never enters the credential store"
    );
    let (status_row, reason_row) = run_row(&boot.state, &run_id).await;
    assert!(!status_row.contains(MARKER));
    assert!(!reason_row.unwrap_or_default().contains(MARKER));

    stub.stop().await;
    teardown(boot).await;
}

/// `ModelTurnInput::first_turn` with the empty tool snapshot (adapter legs).
struct ModelTurnInputFor;

impl ModelTurnInputFor {
    fn chat(text: &str) -> lingxi_kernel::model_exchange::ModelTurnInput {
        lingxi_kernel::model_exchange::ModelTurnInput::first_turn(
            text,
            ToolDeclarationSnapshot::empty(),
        )
    }
}

// ── C10: a redirect is never followed with credentials ──────────────────────

#[tokio::test]
async fn c10_a_redirect_is_never_followed_with_credentials() {
    let stub_b =
        StubServer::start(vec![(CHAT_PATH, vec![final_response("must never land")])]).await;
    let redirect_target = format!("{}/v1/chat/completions", stub_b.url());
    let stub_a = StubServer::start(vec![(
        CHAT_PATH,
        vec![
            StubResponse::redirect(&redirect_target),
            StubResponse::redirect(&redirect_target),
        ],
    )])
    .await;
    let plane_json = plane_api_key(&stub_a.endpoint(), "sk-c10-marker");
    let boot = boot("c10", &plane_json, None).await;

    let run_id = execute(&boot.state, "redirect me").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        stub_b.hits(),
        0,
        "the credential was NEVER replayed onto the redirect target"
    );
    assert_eq!(
        stub_a.hits_for(CHAT_PATH),
        1,
        "a 302 is a loud terminal failure, not a retry"
    );

    // The failure text names the policy, never the (attacker-controlled)
    // redirect target.
    let adapter = OpenAiCompletionsAdapter::new(SchemaBudget::default()).expect("adapter");
    let input = ModelTurnInputFor::chat("c10 direct leg");
    let result = adapter
        .execute_chat(
            &adapter_ctx("c10"),
            &ModelCallId::new("run-t02-c10-mc1"),
            &input,
            &adapter_route(&stub_a.endpoint()),
            &ApplicableAuth::Bearer("sk-c10-marker".to_string()),
            // R05-T05: the direct-adapter legs pin the raw wire (no compat).
            None,
            &lingxi_adapters::models::streaming::NullDeltaSink,
        )
        .await;
    let ProviderTurn::Failed { error, .. } = result.turn else {
        panic!("the redirect stub answers 302 — the turn must fail");
    };
    assert!(error.message.contains("redirect"), "{}", error.message);
    assert!(
        !error.message.contains(&stub_b.url()),
        "the redirect target is never echoed: {}",
        error.message
    );
    assert!(
        !error.message.contains("sk-c10-marker"),
        "no material in the failure text: {}",
        error.message
    );

    stub_a.stop().await;
    stub_b.stop().await;
    teardown(boot).await;
}

// ── C11: store concurrency, fidelity and restart semantics ──────────────────

#[tokio::test]
async fn c11_store_concurrency_fidelity_and_restart_semantics() {
    // Leg 1 — the store file under REAL thread-level concurrency stays a
    // valid v1 document and never drops unknown fields.
    let dir = unique_dir("c11-store");
    let seed = serde_json::json!({
        "version": 1,
        "futureTopLevel": {"keep": true},
        "providers": {
            "main": {"tokens": {"accessToken": "at-0", "refreshToken": "rt-0", "expiresAtUnixMs": UNEXPIRED}},
            "kept": {"tokens": {"accessToken": "at-k", "refreshToken": "rt-k", "expiresAtUnixMs": UNEXPIRED}, "futureField": 42}
        }
    });
    std::fs::write(
        dir.join("credentials.json"),
        serde_json::to_string_pretty(&seed).expect("seed json"),
    )
    .expect("seed store");
    let store = CredentialStore::load(Arc::new(FsStoreIo::new(&dir))).expect("load");
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let mut joiners = Vec::new();
    for thread in 0..4 {
        let store = store.clone();
        let barrier = Arc::clone(&barrier);
        joiners.push(tokio::task::spawn_blocking(move || {
            barrier.wait();
            for iteration in 0..25_u32 {
                let tokens = lingxi_adapters::models::oauth::OAuthTokens {
                    access_token: format!("at-t{thread}-i{iteration}"),
                    refresh_token: format!("rt-t{thread}-i{iteration}"),
                    expires_at_unix_ms: UNEXPIRED,
                };
                match (thread + iteration) % 3 {
                    0 => store.put_tokens("main", &tokens).expect("put"),
                    1 => store.remove_tokens("main").expect("remove"),
                    _ => store.put_tokens("kept", &tokens).expect("put kept"),
                }
            }
        }));
    }
    for joiner in joiners {
        joiner.await.expect("store worker");
    }
    // The on-disk file is a valid v1 document with its unknown fields intact
    // (read-modify-write of the full state, atomic replace, one writer).
    let on_disk = read_store_file(&dir);
    assert_eq!(on_disk["version"], 1);
    assert_eq!(on_disk["futureTopLevel"]["keep"], true);
    assert_eq!(
        on_disk["providers"]["kept"]["futureField"], 42,
        "unknown per-provider fields survive concurrent writes: {on_disk}"
    );

    // Leg 2 — service-level concurrent revoke + reload, then the restart
    // semantics per credential kind.
    let stub = StubServer::start(vec![(CHAT_PATH, vec![]), (TOKEN_PATH, vec![])]).await;
    let plane_json =
        plane_oauth_plus_static(&stub.endpoint(), &stub.token_endpoint(), "sk-c11-aux");
    let boot = boot(
        "c11",
        &plane_json,
        Some(("main", "at-old", "rt-1", UNEXPIRED)),
    )
    .await;
    let service = Arc::clone(boot.state.credential_service().expect("credential service"));
    let plane = ModelPlaneConfig::parse_and_validate(&format!("{{{}}}", boot.plane_json))
        .expect("valid plane");
    let mut racers = Vec::new();
    for round in 0..8 {
        let service = Arc::clone(&service);
        let plane = plane.clone();
        racers.push(tokio::spawn(async move {
            if round % 2 == 0 {
                let _ = service.revoke("main").await;
            } else {
                service.reload(&plane).await;
            }
        }));
    }
    for racer in racers {
        racer.await.expect("service racer");
    }
    // The store file is still valid; the revoked OAuth row is gone; the
    // service status is coherent (revoked under every interleaving: reload
    // keeps a cell whose seed is unchanged).
    let on_disk = read_store_file(&boot.runtime_dir);
    assert_eq!(on_disk["version"], 1);
    assert!(on_disk["providers"].get("main").is_none());
    let status = service.status().await;
    let main = status.iter().find(|p| p.provider == "main").expect("main");
    assert_eq!(main.state, "revoked");
    let aux = status.iter().find(|p| p.provider == "aux").expect("aux");
    assert_eq!(aux.state, "ready");

    // Restart (same runtime dir, same plane): the revoked OAuth credential
    // stays gone (not_logged_in — its store row was deleted at revoke time);
    // the static key is re-seeded from the config (the config is the
    // authority for static material — revocation of a static key is a
    // runtime-session fact, honestly documented).
    let restarted = CredentialService::bootstrap(
        &plane,
        &boot.runtime_dir,
        Arc::new(lingxi_service::inject::SystemClock),
    )
    .expect("restart");
    let restarted_status = restarted.status().await;
    let main = restarted_status
        .iter()
        .find(|p| p.provider == "main")
        .expect("main");
    assert_eq!(
        main.state, "not_logged_in",
        "an OAuth revocation survives the restart (the token row is gone)"
    );
    let aux = restarted_status
        .iter()
        .find(|p| p.provider == "aux")
        .expect("aux");
    assert_eq!(
        aux.state, "ready",
        "a static key re-seeds from the config on restart (documented semantics)"
    );

    stub.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
    teardown(boot).await;
}

// ── C12: the management surface is local-only; forgery is refused ───────────

#[tokio::test]
async fn c12_management_surface_is_local_only_and_forgery_is_refused() {
    let stub = StubServer::start(vec![(CHAT_PATH, vec![])]).await;
    let plane_json = plane_api_key(&stub.endpoint(), "sk-c12-live");
    let boot = boot("c12", &plane_json, None).await;
    let management = serve_management(&boot.state).await;

    // Unauthenticated requests are refused.
    let (status, _) = http_request(
        management.addr,
        "GET",
        "/lingxi/v1/models/credentials",
        None,
    )
    .await;
    assert_eq!(status, 401, "no token, no status surface");
    let (status, _) = http_request(
        management.addr,
        "POST",
        "/lingxi/v1/models/credentials/main/revoke",
        None,
    )
    .await;
    assert_eq!(status, 401, "no token, no revocation");

    // A forged provider reference is a loud 404 (never an anonymous success).
    let (status, body) = http_request(
        management.addr,
        "POST",
        "/lingxi/v1/models/credentials/ghost/revoke",
        Some(&management.token),
    )
    .await;
    assert_eq!(status, 404, "unknown provider refused: {body}");

    // Credential handles: minted for a real provider, bound to its
    // generation; a revocation invalidates every outstanding handle.
    let service = Arc::clone(boot.state.credential_service().expect("credential service"));
    let handle = service.mint_handle("main").await.expect("minted");
    assert_eq!(
        service.resolve_handle(&handle).await.expect("resolves"),
        ApplicableAuth::Bearer("sk-c12-live".to_string())
    );
    let forged = CredentialHandle {
        handle_id: "forged-id-never-minted".to_string(),
        ..handle.clone()
    };
    assert!(matches!(
        service.resolve_handle(&forged).await,
        Err(CredentialError::HandleRefused { .. })
    ));

    let (status, body) = http_request(
        management.addr,
        "POST",
        "/lingxi/v1/models/credentials/main/revoke",
        Some(&management.token),
    )
    .await;
    assert_eq!(status, 200, "real revoke accepted: {body}");
    assert!(
        matches!(
            service.resolve_handle(&handle).await,
            Err(CredentialError::HandleRefused { .. })
        ),
        "the revocation's generation bump invalidates outstanding handles"
    );

    // Post-revoke, a run fails before any provider call.
    let run_id = execute(&boot.state, "after revoke").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        stub.hits(),
        0,
        "a revoked credential never reaches the wire"
    );

    management.stop().await;
    stub.stop().await;
    teardown(boot).await;
}

// ── O1: same model id on two keyed providers never crosses material ─────────

#[tokio::test]
async fn o1_same_model_id_on_two_keyed_providers_never_crosses_material() {
    let stub_a = StubServer::start(vec![(CHAT_PATH, vec![final_response("from A")])]).await;
    let stub_b = StubServer::start(vec![(CHAT_PATH, vec![final_response("from B")])]).await;
    let plane_json = format!(
        r#""providers": {{
            "a": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint_a},
                "auth": {{"kind": "apiKey", "apiKey": "sk-o1-aaa"}}
            }},
            "b": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint_b},
                "auth": {{"kind": "apiKey", "apiKey": "sk-o1-bbb"}}
            }}
        }},
        "models": {{"chat": {{"provider": "a", "model": "shared-model"}}}}"#,
        endpoint_a = serde_json::to_string(&stub_a.endpoint()).expect("json"),
        endpoint_b = serde_json::to_string(&stub_b.endpoint()).expect("json"),
    );
    let boot = boot("o1", &plane_json, None).await;
    let management = serve_management(&boot.state).await;

    // Pinned to A: A's key reaches A's endpoint; B is never contacted.
    let run_id = execute(&boot.state, "served by A").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(stub_b.hits(), 0);
    let a_requests = stub_a.requests_for(CHAT_PATH);
    assert_eq!(a_requests.len(), 1);
    assert_eq!(
        a_requests[0]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer sk-o1-aaa")
    );

    // Re-pin the SAME model id to B through the real reload surface: the
    // credential service re-seeds from the SAME re-read source and B's key —
    // and only B's key — reaches B's endpoint.
    let repinned = boot.plane_json.replace(
        r#""chat": {"provider": "a", "model": "shared-model"}"#,
        r#""chat": {"provider": "b", "model": "shared-model"}"#,
    );
    assert_ne!(
        repinned, boot.plane_json,
        "the re-pin actually changed the plane"
    );
    let config_json = format!(
        r#"{{"home": {}, "workspace": {}, {repinned}}}"#,
        serde_json::to_string(&boot.home.to_string_lossy()).expect("home json"),
        serde_json::to_string(&boot.workspace.to_string_lossy()).expect("ws json"),
    );
    std::fs::write(&boot.config_path, config_json).expect("rewrite config");
    let (status, body) = http_request(
        management.addr,
        "POST",
        "/lingxi/v1/models/reload",
        Some(&management.token),
    )
    .await;
    assert_eq!(status, 200, "reload accepted: {body}");

    let run_id = execute(&boot.state, "served by B").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    let b_requests = stub_b.requests_for(CHAT_PATH);
    assert_eq!(b_requests.len(), 1);
    assert_eq!(
        b_requests[0]
            .0
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str()),
        Some("Bearer sk-o1-bbb")
    );
    assert_eq!(stub_a.hits(), 1, "A heard nothing after the re-pin");
    // The keys never crossed: each endpoint saw only its own material.
    assert!(
        stub_a
            .requests_for(CHAT_PATH)
            .iter()
            .all(|(_, body)| !body.contains("sk-o1-bbb")),
        "B's key never touched A's endpoint"
    );
    assert!(
        stub_a
            .requests_for(CHAT_PATH)
            .iter()
            .all(|(headers, _)| headers
                .iter()
                .all(|(_, value)| !value.contains("sk-o1-bbb"))),
        "B's key never appeared in a header to A"
    );

    management.stop().await;
    stub_a.stop().await;
    stub_b.stop().await;
    teardown(boot).await;
}
