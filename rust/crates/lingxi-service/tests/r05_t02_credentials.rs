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
        "models": {{"chat": {{"provider": "main", "model": "stub-model",
            "capabilities": {{"tools": true}}}}}}"#,
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
        "models": {{"chat": {{"provider": "main", "model": "stub-model",
            "capabilities": {{"tools": true}}}}}}"#,
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
            "chat": {{"provider": "main", "model": "stub-model",
                "capabilities": {{"tools": true}}}},
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
            "chat": {{"provider": "main", "model": "stub-model",
                "capabilities": {{"tools": true}}}},
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
                // Same plane as boot (generation 1) — the reload keeps
                // every seed-unchanged cell (R05 RR1 F02: the generation
                // argument mirrors the paired gateway reload).
                service.reload(&plane, 1).await;
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
    let handle = service
        .mint_handle("principal_local", "main")
        .await
        .expect("minted");
    assert_eq!(
        service
            .resolve_handle("principal_local", &handle)
            .await
            .expect("resolves"),
        ApplicableAuth::Bearer("sk-c12-live".to_string())
    );
    let forged = CredentialHandle {
        handle_id: "forged-id-never-minted".to_string(),
        ..handle.clone()
    };
    assert!(matches!(
        service.resolve_handle("principal_local", &forged).await,
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
            service.resolve_handle("principal_local", &handle).await,
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

// ══ R05 RR1 (2026-10-04): F03/F04/F05 permanent counterexamples ═════════════
//
// These tests are the RR1 WP-T02 repair evidence, migrated from the frozen
// adversarial probes (CN-F02, CN-F08) plus the new legs the master prompt
// demands. Behavior assertions are verbatim-strength: they were proven RED
// on the frozen HEAD d80737b6 (and on the pre-repair candidate) before the
// production fix landed — see artifacts/rust-tauri/R05/RR1/WP-T02-E01/.

mod rr1_t02 {
    use super::*;
    use lingxi_adapters::models::dispatch;
    use lingxi_service::credentials::{CredentialService, ProductionRefreshDriver};
    use lingxi_service::inject::ManualClock;

    // ── F03: credential-handle epochs never reuse identity across a
    // provider's lifecycle (CN-F02: rotation / revoke→remove→recreate must
    // never let an OLD handle resolve NEW material).

    fn rr1_static_plane(endpoint: &str, key: &str) -> ModelPlaneConfig {
        ModelPlaneConfig::parse_and_validate(&format!(
            r#"{{
                "providers": {{
                    "main": {{
                        "protocol": "openai-completions",
                        "endpoint": {endpoint},
                        "auth": {{"kind": "apiKey", "apiKey": {key}}}
                    }}
                }},
                "models": {{"chat": {{"provider": "main", "model": "stub-model"}}}}
            }}"#,
            endpoint = serde_json::to_string(endpoint).expect("json"),
            key = serde_json::to_string(key).expect("json"),
        ))
        .expect("valid plane")
    }

    fn rr1_oauth_plane(endpoint: &str, token_endpoint: &str) -> ModelPlaneConfig {
        ModelPlaneConfig::parse_and_validate(&format!(
            r#"{{
                "providers": {{
                    "main": {{
                        "protocol": "openai-completions",
                        "endpoint": {endpoint},
                        "auth": {{"kind": "oauth", "flow": "deviceCode",
                            "clientId": "client-rr1",
                            "tokenEndpoint": {token_endpoint},
                            "deviceAuthorizationEndpoint": {device_endpoint}}}
                    }}
                }},
                "models": {{"chat": {{"provider": "main", "model": "stub-model"}}}}
            }}"#,
            endpoint = serde_json::to_string(endpoint).expect("json"),
            token_endpoint = serde_json::to_string(token_endpoint).expect("json"),
            device_endpoint =
                serde_json::to_string(&format!("{token_endpoint}/device")).expect("json"),
        ))
        .expect("valid plane")
    }

    fn rr1_service(config: &ModelPlaneConfig) -> CredentialService {
        CredentialService::new(
            config,
            None,
            std::sync::Arc::new(ProductionRefreshDriver::new(OAUTH_REQUEST_TIMEOUT).unwrap()),
            std::sync::Arc::new(ManualClock::new(1_000_000)),
        )
    }

    /// CN-F02 counterexample 1, verbatim: a handle minted before a key
    /// rotation must never resolve the NEW key. The number-world reload
    /// mirrors the paired gateway reload (R05 RR1 F02 lockstep).
    #[tokio::test]
    async fn rr1_f03_old_handle_must_not_resolve_reloaded_secret() {
        let old = rr1_static_plane("https://old.example.invalid/v1", "dummy-old-key");
        let next = rr1_static_plane("https://new.example.invalid/v1", "dummy-new-key");
        let svc = rr1_service(&old);
        let handle = svc.mint_handle("principal_local", "main").await.unwrap();
        svc.reload(&next, 2).await;
        let result = svc.resolve_handle("principal_local", &handle).await;
        assert!(
            result.is_err(),
            "old credential handle survived key rotation and returned {result:?}"
        );
        // The positive control: a handle minted AFTER the rotation serves
        // the new material normally (repair, not a blanket refusal).
        let fresh = svc.mint_handle("principal_local", "main").await.unwrap();
        assert_eq!(
            svc.resolve_handle("principal_local", &fresh)
                .await
                .expect("fresh handle resolves"),
            ApplicableAuth::Bearer("dummy-new-key".to_string())
        );
    }

    /// CN-F02 counterexample 2, through the REAL management reload surface:
    /// revoke → provider removed → provider recreated with a new key — the
    /// original handle must stay dead, never revive against the recreated
    /// provider.
    #[tokio::test]
    async fn rr1_f03_removed_then_readded_provider_must_not_revive_old_handle() {
        let stub = StubServer::start(vec![(CHAT_PATH, vec![])]).await;
        let plane_json = plane_api_key(&stub.endpoint(), "dummy-rr1-old");
        let boot = boot("rr1f03a", &plane_json, None).await;
        let management = serve_management(&boot.state).await;
        let service = Arc::clone(boot.state.credential_service().expect("credential service"));

        let handle = service
            .mint_handle("principal_local", "main")
            .await
            .expect("minted");
        service.revoke("main").await.expect("revoke");

        let empty_plane = format!(
            r#"{{"home": {}, "workspace": {}, "providers": {{}}, "models": {{}}}}"#,
            serde_json::to_string(&boot.home.to_string_lossy()).expect("home json"),
            serde_json::to_string(&boot.workspace.to_string_lossy()).expect("ws json"),
        );
        std::fs::write(&boot.config_path, empty_plane).expect("rewrite config (remove)");
        let (status, body) = http_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/reload",
            Some(&management.token),
        )
        .await;
        assert_eq!(status, 200, "remove reload accepted: {body}");

        let recreated = format!(
            r#"{{"home": {}, "workspace": {}, {plane_json}}}"#,
            serde_json::to_string(&boot.home.to_string_lossy()).expect("home json"),
            serde_json::to_string(&boot.workspace.to_string_lossy()).expect("ws json"),
        );
        std::fs::write(&boot.config_path, recreated).expect("rewrite config (recreate)");
        let (status, body) = http_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/reload",
            Some(&management.token),
        )
        .await;
        assert_eq!(status, 200, "recreate reload accepted: {body}");

        let result = service.resolve_handle("principal_local", &handle).await;
        assert!(
            result.is_err(),
            "revoked handle revived after provider recreation and returned {result:?}"
        );
        // A handle minted on the RECREATED provider serves normally.
        let fresh = service
            .mint_handle("principal_local", "main")
            .await
            .expect("minted on the recreated provider");
        assert_eq!(
            service
                .resolve_handle("principal_local", &fresh)
                .await
                .expect("fresh handle resolves"),
            ApplicableAuth::Bearer("dummy-rr1-old".to_string())
        );

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// F03's contract-gap closure (RR1-F03-PRINCIPAL): a handle is bound
    /// to the minting principal; a different subject presenting it is
    /// refused. (The pre-repair API had NO principal parameter at all —
    /// this leg had no old-red by construction; it pins the new contract.)
    #[tokio::test]
    async fn rr1_f03_handles_are_bound_to_the_minting_principal() {
        let svc = rr1_service(&rr1_static_plane(
            "https://old.example.invalid/v1",
            "dummy-principal-key",
        ));
        let handle = svc
            .mint_handle("principal_local", "main")
            .await
            .expect("minted");
        // The rightful subject resolves it.
        assert!(svc.resolve_handle("principal_local", &handle).await.is_ok());
        // A different subject presenting the same handle is refused loudly,
        // before any material moves.
        assert!(
            matches!(
                svc.resolve_handle("principal_intruder", &handle).await,
                Err(CredentialError::HandleRefused { .. })
            ),
            "a handle presented by a different principal must be refused"
        );
        // Cross-principal in the other direction is refused too.
        let foreign = svc
            .mint_handle("principal_intruder", "main")
            .await
            .expect("minted");
        assert!(matches!(
            svc.resolve_handle("principal_local", &foreign).await,
            Err(CredentialError::HandleRefused { .. })
        ));
    }

    /// F03's store-initialization leg: a service that booted WITHOUT any
    /// OAuth provider (no store at bootstrap time, pre-repair) must still
    /// seed — and durably serve — OAuth material when a reload hot-adds an
    /// OAuth provider whose tokens were already on disk.
    #[tokio::test]
    async fn rr1_f03_hot_added_oauth_provider_seeds_tokens_and_survives_restart() {
        let stub = StubServer::start(vec![(CHAT_PATH, vec![]), (TOKEN_PATH, vec![])]).await;
        let static_plane = plane_api_key(&stub.endpoint(), "sk-rr1-hotadd");
        // The store file carries a logged-in token set for `main` BEFORE the
        // OAuth provider even exists in the plane.
        let boot = boot(
            "rr1f03b",
            &static_plane,
            Some(("main", "at-hot-add", "rt-hot-add", UNEXPIRED)),
        )
        .await;
        let service = Arc::clone(boot.state.credential_service().expect("credential service"));

        // Hot-add the OAuth provider through the real reload surface.
        let oauth_plane_json = format!(
            r#""providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": {endpoint},
                    "auth": {{"kind": "oauth", "flow": "deviceCode",
                        "clientId": "client-rr1",
                        "tokenEndpoint": {token_endpoint},
                        "deviceAuthorizationEndpoint": {device_endpoint}}}
                }}
            }},
            "models": {{"chat": {{"provider": "main", "model": "stub-model"}}}}"#,
            endpoint = serde_json::to_string(&stub.endpoint()).expect("json"),
            token_endpoint = serde_json::to_string(&stub.token_endpoint()).expect("json"),
            device_endpoint =
                serde_json::to_string(&format!("{}/device", stub.url())).expect("json"),
        );
        let config_json = format!(
            r#"{{"home": {}, "workspace": {}, {oauth_plane_json}}}"#,
            serde_json::to_string(&boot.home.to_string_lossy()).expect("home json"),
            serde_json::to_string(&boot.workspace.to_string_lossy()).expect("ws json"),
        );
        std::fs::write(&boot.config_path, config_json).expect("rewrite config (hot add)");
        let management = serve_management(&boot.state).await;
        let (status, body) = http_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/reload",
            Some(&management.token),
        )
        .await;
        assert_eq!(status, 200, "hot-add reload accepted: {body}");
        management.stop().await;

        // The hot-added OAuth provider resolves the ON-DISK material.
        let plane = rr1_oauth_plane(&stub.endpoint(), &stub.token_endpoint());
        let route = boot
            .state
            .model_gateway()
            .expect("gateway")
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("route resolves");
        assert_eq!(route.provider, "main");
        assert_eq!(
            service
                .resolve(&route)
                .await
                .expect("hot-added OAuth material"),
            ApplicableAuth::Bearer("at-hot-add".to_string())
        );

        // Restart semantics: a fresh bootstrap on the SAME runtime dir serves
        // the same material (the store is the durable home of OAuth tokens).
        let restarted = CredentialService::bootstrap(
            &plane,
            &boot.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("restart");
        let status = restarted.status().await;
        let main = status.iter().find(|p| p.provider == "main").expect("main");
        assert_eq!(main.kind, "oauth");
        assert_eq!(main.state, "ready");

        stub.stop().await;
        teardown(boot).await;
    }

    // ── F05: scrub BEFORE truncation; protocol errors never carry material
    // across the host boundary (CN-F08).

    /// CN-F08 counterexample 1, verbatim-strength: a credential echoed at
    /// (or across) the 512-char excerpt boundary must not leak a prefix —
    /// the scrub has to run on the FULL text before any truncation.
    #[test]
    fn rr1_f05_truncation_never_exposes_a_credential_prefix_at_any_boundary() {
        let auth = ApplicableAuth::Bearer("secret-cobalt-pearl".into());
        for offset in [0_usize, 1, 100, 504, 505, 509, 510, 511, 512, 513, 520, 600] {
            let echoed = format!(
                "{}{} and then some trailing context",
                " ".repeat(offset),
                "secret-cobalt-pearl"
            );
            let excerpt = dispatch::scrubbed_excerpt(&echoed, &auth);
            let redacted = lingxi_service::redact_line(&excerpt);
            assert!(
                !redacted.contains("secret-c"),
                "offset {offset}: credential prefix leaked after truncation and both \
                 redaction layers: {redacted:?}"
            );
            assert!(
                !redacted.contains("cobalt"),
                "offset {offset}: credential fragment leaked: {redacted:?}"
            );
        }
    }

    /// CN-F08, encoded-echo legs: URL-encoded and JSON-escaped echoes of the
    /// in-play material are scrubbed too (an auth server can echo anything).
    #[test]
    fn rr1_f05_encoded_echos_of_the_material_are_scrubbed() {
        let auth = ApplicableAuth::Bearer("secret+cobalt/pearl=x".into());
        let material = "secret+cobalt/pearl=x";
        // Full percent-encoding (uppercase and lowercase hex) and the
        // unreserved-keeping form.
        let percent_upper: String = material.bytes().map(|b| format!("%{b:02X}")).collect();
        let percent_lower: String = material.bytes().map(|b| format!("%{b:02x}")).collect();
        // The JSON string-body escape (serde escapes `+`? no — but `/`
        // stays literal; force the escaped form through the \u route).
        let json_escaped: String = material
            .chars()
            .map(|c| format!("\\u{:04x}", c as u32))
            .collect();
        for encoded in [percent_upper, percent_lower, json_escaped] {
            let echoed = format!("upstream echoed {encoded} in its error");
            let excerpt = dispatch::scrubbed_excerpt(&echoed, &auth);
            let redacted = lingxi_service::redact_line(&excerpt);
            assert!(
                !redacted.contains(&encoded),
                "encoded echo of the material survived: {redacted:?}"
            );
            assert!(
                !redacted.contains("cobalt"),
                "encoded echo fragment survived: {redacted:?}"
            );
        }
    }

    /// Reads one HTTP request off a raw socket (head + content-length body).
    async fn read_raw_request(conn: &mut tokio::net::TcpStream) -> String {
        let mut raw = Vec::new();
        loop {
            let mut chunk = [0_u8; 4096];
            let read = conn.read(&mut chunk).await.expect("read");
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
            if let Some(end) = find_subslice(&raw, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
                let count: usize = head
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .map(|value| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if raw.len() >= end + 4 + count {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&raw).into_owned()
    }

    /// CN-F08 counterexample 2, verbatim-strength: a provider that echoes
    /// the in-play key through an unknown tool name must not get the FULL
    /// key into the kernel ProtocolError (and the service redactor cannot
    /// be the only net).
    #[tokio::test]
    async fn rr1_f05_protocol_error_echo_must_not_carry_complete_secret_into_kernel() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let _ = read_raw_request(&mut conn).await;
            let body = concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
                "\"id\":\"call_dummy\",\"type\":\"function\",\"function\":",
                "{\"name\":\"dummy-cobalt-key\",\"arguments\":\"{}\"}}]}}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            conn.write_all(response.as_bytes()).await.unwrap();
        });
        let cfg = rr1_static_plane(&format!("http://{addr}/v1"), "dummy-cobalt-key");
        let svc = Arc::new(rr1_service(&cfg));
        let gateway =
            Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(cfg));
        let provider = lingxi_adapters::models::provider::GatewayedProvider::new(
            gateway,
            svc,
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
        )
        .unwrap();
        let context = adapter_ctx("rr1f05");
        let call = ModelCallId::new("rr1-f05-echo-call");
        let input = ModelTurnInputFor::chat("hello");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            lingxi_kernel::ports::TurnProviderPort::next_turn(
                &provider,
                &context,
                &call,
                &input,
                &lingxi_adapters::models::streaming::NullDeltaSink,
            ),
        )
        .await
        .expect("bounded");
        stub.await.unwrap();
        let ProviderTurn::Failed { error, .. } = result.turn else {
            panic!("unexpected non-failure turn: the stub echoes an unknown tool");
        };
        // The request reached the wire (boundary reached) — the stub saw it.
        assert!(
            error.message.contains("tool"),
            "the failure is the unknown-tool protocol error: {}",
            error.message
        );
        let redacted = lingxi_service::redact_line(&error.message);
        assert!(
            !redacted.contains("dummy-cobalt-key"),
            "full credential escaped into the kernel ProtocolError and survived service \
             redaction: {redacted}"
        );
    }

    /// F05's old/new-material union rule: a 401→refresh→retry call had TWO
    /// in-play keys on the wire (the stale one and the fresh one); the
    /// failure that settles the call is scrubbed against BOTH — an echo of
    /// either key (raw or at the truncation boundary) never reaches the
    /// kernel error.
    #[tokio::test]
    async fn rr1_f05_retry_path_scrubs_both_the_stale_and_the_fresh_material() {
        let old_key = "stale-key-rr1-union-0123456789abcdef";
        let new_key = "fresh-key-rr1-union-0123456789abcdef";
        let sse_echo = |key: &str| {
            let body = format!(
                "data: {{\"id\":\"chatcmpl-union\",\"model\":\"stub-model\",\"choices\":[{{\
                 \"index\":0,\"finish_reason\":null,\"delta\":{{\"tool_calls\":[{{\"index\":0,\
                 \"id\":\"call_u\",\"type\":\"function\",\"function\":{{\"name\":\"{key}\",\
                 \"arguments\":\"{{}}\"}}}}]}}]}}]}}\n\n\
                 data: {{\"id\":\"chatcmpl-union\",\"choices\":[{{\"index\":0,\"delta\":{{}},\
                 \"finish_reason\":\"tool_calls\"}}]}}\n\n\
                 data: [DONE]\n\n"
            );
            StubResponse::immediate_sse(body)
        };
        let stub = StubServer::start(vec![
            // Attempt 1: the OLD key is rejected with its own echo.
            (
                CHAT_PATH,
                vec![unauthorized_response(old_key), sse_echo(new_key)],
            ),
            // The refresh mints the NEW key.
            (TOKEN_PATH, vec![token_response(new_key, "rt-union")]),
        ])
        .await;
        // An UNEXPIRED old token serves attempt 1; its 401 forces the
        // refresh→retry path (attempt 2 carries the fresh key).
        let boot = boot(
            "rr1f05u",
            &plane_oauth(&stub.endpoint(), &stub.token_endpoint()),
            Some(("main", old_key, "rt-seed", UNEXPIRED)),
        )
        .await;

        let run_id = execute(&boot.state, "union scrub leg").await;
        let (status, reason) = run_row(&boot.state, &run_id).await;
        assert_eq!(status, "failed");
        assert_eq!(reason.as_deref(), Some("failed.provider_error"));
        // The retry physically happened (both attempts on the wire).
        assert_eq!(stub.hits_for(CHAT_PATH), 2);
        assert_eq!(stub.hits_for(TOKEN_PATH), 1);

        // Neither the stale nor the fresh material survives anywhere under
        // the service home OUTSIDE the credential store — the one
        // whitelisted material location (the unknown-tool echo of the NEW
        // key and the 401 body echo of the OLD key both crossed the host
        // boundary pre-fix).
        let store_path = boot
            .runtime_dir
            .join("credentials.json")
            .to_string_lossy()
            .into_owned();
        let leaks_of = |marker: &str| {
            files_containing(&boot.home, marker)
                .into_iter()
                .filter(|path| {
                    path.to_string_lossy() != store_path
                        && !path.ends_with("lingxi-service/credentials.json")
                })
                .collect::<Vec<_>>()
        };
        assert!(
            leaks_of(old_key).is_empty(),
            "stale material leaked: {:?}",
            leaks_of(old_key)
        );
        assert!(
            leaks_of(new_key).is_empty(),
            "fresh material leaked: {:?}",
            leaks_of(new_key)
        );

        stub.stop().await;
        teardown(boot).await;
    }

    /// F05 full-chain: the same echo through the REAL service wiring must
    /// leave NO marker in any durable state under the service home.
    #[tokio::test]
    async fn rr1_f05_provider_echo_full_chain_never_reaches_durable_state() {
        // The marker deliberately matches no redactor pattern and no
        // provider prefix — only the host-boundary scrub can stop it.
        let marker = "MARKER-key-f05-echo-0123456789abcdef";
        let sse = || {
            let body = format!(
                "data: {{\"id\":\"chatcmpl-rr1f05\",\"model\":\"stub-model\",\"choices\":[{{\
                 \"index\":0,\"finish_reason\":null,\"delta\":{{\"tool_calls\":[{{\"index\":0,\
                 \"id\":\"call_rr1\",\"type\":\"function\",\"function\":{{\"name\":\"{marker}\",\
                 \"arguments\":\"{{}}\"}}}}]}}]}}]}}\n\n\
                 data: {{\"id\":\"chatcmpl-rr1f05\",\"choices\":[{{\"index\":0,\"delta\":{{}},\
                 \"finish_reason\":\"tool_calls\"}}]}}\n\n\
                 data: [DONE]\n\n"
            );
            StubResponse::immediate_sse(body)
        };
        let stub = StubServer::start(vec![(CHAT_PATH, vec![sse(), sse()])]).await;
        let plane_json = plane_api_key(&stub.endpoint(), marker);
        let boot = boot("rr1f05", &plane_json, None).await;

        let run_id = execute(&boot.state, "echo my key back in a tool name").await;
        let (status, reason) = run_row(&boot.state, &run_id).await;
        assert_eq!(status, "failed");
        assert_eq!(reason.as_deref(), Some("failed.provider_error"));
        // Boundary proof: the provider call physically happened.
        assert_eq!(stub.hits(), 1, "the echoing provider was actually called");

        // Durable-state scan: nothing under the service home carries the
        // marker (the in-play material never crosses the host boundary).
        assert!(
            files_containing(&boot.home, marker).is_empty(),
            "marker leaked into durable state: {:?}",
            files_containing(&boot.home, marker)
        );

        stub.stop().await;
        teardown(boot).await;
    }
}

// ══ R05 RR1 F04: the six R00-T02 exclusive OAuth leaves, through the REAL
// authenticated management surface (loopback HTTP against the production
// axum routes) with a controlled OAuth stand-in at the far end ══════════

mod rr1_f04 {
    use super::*;
    use lingxi_service::credentials::{CredentialService, ProductionRefreshDriver};

    const DEVICE_PATH: &str = "/token/device";

    /// A body-carrying real-HTTP client for the management surface.
    async fn http_json_request(
        addr: SocketAddr,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<&str>,
    ) -> (u16, serde_json::Value) {
        let raw = tokio::time::timeout(Duration::from_secs(15), async {
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect to real service");
            let auth = match bearer {
                Some(token) => format!("Authorization: Bearer {token}\r\n"),
                None => String::new(),
            };
            let body = body.unwrap_or("");
            let request = format!(
                "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: \
                 application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(request.as_bytes()).await.expect("write");
            let mut raw = Vec::new();
            let mut chunk = [0_u8; 8192];
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
        .expect("management request stalled");
        let text = String::from_utf8_lossy(&raw);
        let (head, body) = text.split_once("\r\n\r\n").expect("header/body split");
        let status = head
            .split_whitespace()
            .nth(1)
            .expect("status")
            .parse::<u16>()
            .expect("numeric status");
        let value = if body.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(body)
                .unwrap_or_else(|err| panic!("JSON response body: {err}; raw: {text:?}"))
        };
        (status, value)
    }

    fn device_authorization_response() -> StubResponse {
        StubResponse::immediate(
            200,
            serde_json::json!({
                "device_code": "dev-code-rr1",
                "user_code": "ABCD-EFGH",
                "verification_uri": "https://auth.example.invalid/activate",
                "expires_in": 900,
                "interval": 1
            })
            .to_string(),
        )
    }

    fn poll_response(error: Option<&str>) -> StubResponse {
        match error {
            None => token_response("at-device-login", "rt-device-login"),
            Some(code) => {
                StubResponse::immediate(400, serde_json::json!({"error": code}).to_string())
            }
        }
    }

    fn pkce_plane(endpoint: &str, token_endpoint: &str) -> String {
        format!(
            r#""providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": {endpoint},
                    "auth": {{"kind": "oauth", "flow": "authorizationCodePkce",
                        "clientId": "client-rr1-pkce",
                        "tokenEndpoint": {token_endpoint},
                        "authorizeEndpoint": {authorize_endpoint}}}
                }},
                "aux": {{
                    "protocol": "openai-completions",
                    "endpoint": "http://127.0.0.1:9/unused",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-rr1-f04-aux"}}
                }}
            }},
            "models": {{"chat": {{"provider": "main", "model": "stub-model",
                "capabilities": {{"tools": true}}}}}}"#,
            endpoint = serde_json::to_string(endpoint).expect("json"),
            token_endpoint = serde_json::to_string(token_endpoint).expect("json"),
            authorize_endpoint =
                serde_json::to_string(&format!("{token_endpoint}/authorize")).expect("json"),
        )
    }

    fn device_plane(endpoint: &str, token_endpoint: &str) -> String {
        format!(
            r#""providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": {endpoint},
                    "auth": {{"kind": "oauth", "flow": "deviceCode",
                        "clientId": "client-rr1-dev",
                        "tokenEndpoint": {token_endpoint},
                        "deviceAuthorizationEndpoint": {device_endpoint}}}
                }},
                "aux": {{
                    "protocol": "openai-completions",
                    "endpoint": "http://127.0.0.1:9/unused",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-rr1-f04-aux"}}
                }}
            }},
            "models": {{"chat": {{"provider": "main", "model": "stub-model",
                "capabilities": {{"tools": true}}}}}}"#,
            endpoint = serde_json::to_string(endpoint).expect("json"),
            token_endpoint = serde_json::to_string(token_endpoint).expect("json"),
            device_endpoint =
                serde_json::to_string(&format!("{token_endpoint}/device")).expect("json"),
        )
    }

    async fn credential_status(
        management: &ManagementSurface,
        provider: &str,
    ) -> serde_json::Value {
        let (status, body) = http_request(
            management.addr,
            "GET",
            "/lingxi/v1/models/credentials",
            Some(&management.token),
        )
        .await;
        assert_eq!(status, 200, "status surface: {body}");
        body["providers"]
            .as_array()
            .expect("providers array")
            .iter()
            .find(|row| row["provider"] == provider)
            .cloned()
            .unwrap_or_else(|| panic!("provider {provider} in status"))
    }

    /// Extracts the `state` query parameter of an authorize URL.
    fn state_of_authorize_url(url: &str) -> String {
        let query = url.split_once('?').expect("query").1;
        for pair in query.split('&') {
            if let Some((name, value)) = pair.split_once('=') {
                if name == "state" {
                    return value.to_string();
                }
            }
        }
        panic!("no state in {url}")
    }

    // ── LA-99D6C304D697 (PKCE half): start guidance, manual-code callback
    // to an installed credential, one-shot semantics, a REAL model call.

    #[tokio::test]
    async fn rr1_f04_pkce_start_manual_callback_install_and_real_model_call() {
        let stub = StubServer::start(vec![
            (
                TOKEN_PATH,
                vec![token_response("at-pkce-login", "rt-pkce-login")],
            ),
            (CHAT_PATH, vec![final_response("f04 pkce answer")]),
        ])
        .await;
        let boot = boot(
            "rr1f04pk",
            &pkce_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        // LA-99D6C304D697 start half: the guidance carries the authorize
        // URL (state + S256 PKCE challenge + loopback redirect) — and the
        // unauthenticated call is refused.
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            None,
            None,
        )
        .await;
        assert_eq!(status, 401, "no token, no login surface");
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "login start: {body}");
        let start = &body["start"];
        assert_eq!(start["flow"], "authorizationCodePkce");
        let authorize_url = start["authorizeUrl"].as_str().expect("authorizeUrl");
        assert!(authorize_url.contains("code_challenge_method=S256"));
        assert!(authorize_url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A"));
        let state = state_of_authorize_url(authorize_url);
        assert!(!state.is_empty(), "the state is the login's secret");

        // Not logged in yet, and no token endpoint traffic so far.
        let row = credential_status(&management, "main").await;
        assert_eq!(row["loggedIn"], false);
        assert_eq!(row["availableModels"], 0);
        assert_eq!(stub.hits_for(TOKEN_PATH), 0);

        // A WRONG state refuses with zero writes — and leaves the pending
        // login alive (the correct completion still works afterwards).
        let wrong = serde_json::json!({"state": "attacker-guess", "code": "code-x"});
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/callback",
            Some(&management.token),
            Some(&wrong.to_string()),
        )
        .await;
        assert_eq!(status, 409, "wrong state refused: {body}");
        assert_eq!(
            stub.hits_for(TOKEN_PATH),
            0,
            "zero exchanges on a wrong state"
        );
        assert_eq!(
            credential_status(&management, "main").await["loggedIn"],
            false
        );

        // The manual-code callback (手输码): correct state + code → the
        // credential installs through the SAME authority.
        let correct = serde_json::json!({"state": state, "code": "code-rr1-grant"});
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/callback",
            Some(&management.token),
            Some(&correct.to_string()),
        )
        .await;
        assert_eq!(status, 200, "manual callback completes: {body}");
        assert_eq!(body["outcome"]["status"], "loggedIn");
        assert_eq!(body["outcome"]["persisted"], true);

        // The exchange hit the REAL token endpoint with the PKCE verifier.
        assert_eq!(stub.hits_for(TOKEN_PATH), 1);
        let exchange = &stub.requests_for(TOKEN_PATH)[0].1;
        assert!(
            exchange.contains("grant_type=authorization_code"),
            "{exchange}"
        );
        assert!(exchange.contains("code=code-rr1-grant"), "{exchange}");
        assert!(exchange.contains("code_verifier="), "{exchange}");

        // Status: loggedIn with the config-bound model available.
        let row = credential_status(&management, "main").await;
        assert_eq!(row["kind"], "oauth");
        assert_eq!(row["state"], "ready");
        assert_eq!(row["loggedIn"], true);
        assert_eq!(row["availableModels"], 1, "the chat-bound stub-model");

        // The one-shot rule: a replayed callback finds nothing to consume.
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/callback",
            Some(&management.token),
            Some(&correct.to_string()),
        )
        .await;
        assert_eq!(status, 409, "a replayed callback is refused");
        assert_eq!(stub.hits_for(TOKEN_PATH), 1, "no second exchange");

        // A REAL model call through the logged-in credential.
        let run_id = execute(&boot.state, "f04 pkce login roundtrip").await;
        let (status, reason) = run_row(&boot.state, &run_id).await;
        assert_eq!(status, "completed", "reason: {reason:?}");
        let chat = stub.requests_for(CHAT_PATH);
        assert_eq!(chat.len(), 1);
        assert!(
            chat[0]
                .0
                .iter()
                .any(|(name, value)| name == "authorization" && value == "Bearer at-pkce-login"),
            "the installed token served the real model call"
        );

        // Persistence: the store carries the tokens; a restart serves them.
        let store = read_store_file(&boot.runtime_dir);
        assert_eq!(
            store["providers"]["main"]["tokens"]["accessToken"],
            "at-pkce-login"
        );
        let plane = ModelPlaneConfig::parse_and_validate(&format!(
            "{{{}}}",
            pkce_plane(&stub.endpoint(), &stub.token_endpoint())
        ))
        .expect("valid plane");
        let restarted = CredentialService::bootstrap(
            &plane,
            &boot.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("restart");
        let row = restarted
            .status()
            .await
            .into_iter()
            .find(|row| row.provider == "main")
            .expect("main");
        assert_eq!(row.state, "ready");
        assert!(row.logged_in);

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// LA-99D6C304D697 (browser half): the SPAWNED loopback listener is
    /// the production callback path — a real TCP GET with the correct
    /// state completes the login; a wrong-state GET answers 400 and
    /// installs nothing.
    #[tokio::test]
    async fn rr1_f04_pkce_browser_callback_listener_completes_the_login() {
        let stub = StubServer::start(vec![(
            TOKEN_PATH,
            vec![token_response("at-browser", "rt-browser")],
        )])
        .await;
        let boot = boot(
            "rr1f04br",
            &pkce_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "login start: {body}");
        let callback_addr = body["start"]["callbackAddr"]
            .as_str()
            .expect("callbackAddr")
            .to_string();
        let state = state_of_authorize_url(body["start"]["authorizeUrl"].as_str().expect("url"));

        // A wrong-state browser callback: HTTP 400, zero exchanges, the
        // login stays pending.
        let wrong = format!(
            "GET /callback?code=steal&state=nope HTTP/1.1\r\nHost: {callback_addr}\r\nConnection: close\r\n\r\n"
        );
        let mut socket = tokio::net::TcpStream::connect(&callback_addr)
            .await
            .expect("connect callback listener");
        socket.write_all(wrong.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = socket.read_to_end(&mut buf).await;
        let answer = String::from_utf8_lossy(&buf);
        assert!(
            answer.starts_with("HTTP/1.1 400"),
            "wrong state 400: {answer}"
        );
        assert_eq!(stub.hits_for(TOKEN_PATH), 0);

        // The correct browser callback: 200 and the spawned task installs.
        let correct = format!(
            "GET /callback?code=browser-grant&state={state} HTTP/1.1\r\nHost: {callback_addr}\r\nConnection: close\r\n\r\n"
        );
        let mut socket = tokio::net::TcpStream::connect(&callback_addr)
            .await
            .expect("connect callback listener");
        socket.write_all(correct.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = socket.read_to_end(&mut buf).await;
        let answer = String::from_utf8_lossy(&buf);
        assert!(
            answer.starts_with("HTTP/1.1 200"),
            "valid state 200: {answer}"
        );
        // The listener task redeems through the token endpoint.
        wait_until_path_hits(&stub, TOKEN_PATH, 1).await;
        let row = credential_status(&management, "main").await;
        assert_eq!(
            row["loggedIn"], true,
            "the browser path installed the login"
        );

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// F30 (RR1 R2): the BROWSER-listener completion CONSUMES the one-shot
    /// login transaction. After the spawned listener redeemed the callback
    /// and installed the login, a manual-code replay of the SAME (state,
    /// code) through the management surface must be REFUSED (409) with ZERO
    /// further token exchanges and ZERO writes — per login.rs's own contract
    /// ("the state is CONSUMED by the first completing attempt; a replayed
    /// callback finds nothing left", T02-C08). The stub scripts a SECOND
    /// token response on purpose: the controlled stand-in does not enforce
    /// code single-use, so a replayed exchange would SUCCEED — only the
    /// one-shot rule can keep the count at 1.
    #[tokio::test]
    async fn rr1_f04_browser_completion_then_manual_replay_refused_zero_new_exchanges() {
        let stub = StubServer::start(vec![(
            TOKEN_PATH,
            vec![
                token_response("at-browser-f30", "rt-browser-f30"),
                token_response("at-browser-f30-replay", "rt-browser-f30-replay"),
            ],
        )])
        .await;
        let boot = boot(
            "rr1f04f30",
            &pkce_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "login start: {body}");
        let callback_addr = body["start"]["callbackAddr"]
            .as_str()
            .expect("callbackAddr")
            .to_string();
        let state = state_of_authorize_url(body["start"]["authorizeUrl"].as_str().expect("url"));

        // The browser leg: a real TCP GET with the correct state installs.
        let correct = format!(
            "GET /callback?code=browser-f30-code&state={state} HTTP/1.1\r\nHost: {callback_addr}\r\nConnection: close\r\n\r\n"
        );
        let mut socket = tokio::net::TcpStream::connect(&callback_addr)
            .await
            .expect("connect callback listener");
        socket.write_all(correct.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = socket.read_to_end(&mut buf).await;
        let answer = String::from_utf8_lossy(&buf);
        assert!(
            answer.starts_with("HTTP/1.1 200"),
            "valid state 200: {answer}"
        );
        wait_until_path_hits(&stub, TOKEN_PATH, 1).await;
        let row = credential_status(&management, "main").await;
        assert_eq!(
            row["loggedIn"], true,
            "the browser path installed the login"
        );
        assert_eq!(stub.hits_for(TOKEN_PATH), 1);

        // The REPLAY: the same (state, code) through the manual-code
        // surface. One-shot: the browser leg already consumed the
        // transaction — nothing is left to complete with.
        let replay = serde_json::json!({"state": state, "code": "browser-f30-code"});
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/callback",
            Some(&management.token),
            Some(&replay.to_string()),
        )
        .await;
        assert_eq!(
            status, 409,
            "the manual replay of a browser-completed login must be refused: {body}"
        );
        assert_eq!(
            stub.hits_for(TOKEN_PATH),
            1,
            "the replay must offer ZERO further token exchanges"
        );

        // Zero writes: still logged in with the FIRST (browser-leg) tokens —
        // the store never saw the replay tokens.
        let row = credential_status(&management, "main").await;
        assert_eq!(row["loggedIn"], true);
        let store = read_store_file(&boot.runtime_dir);
        assert_eq!(
            store["providers"]["main"]["tokens"]["accessToken"], "at-browser-f30",
            "no second install: the store keeps the browser-leg tokens"
        );

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// F30 (RR1 R2, 误吃防护): a completion carrying the state of an OLDER
    /// flow must never eat the transaction a NEWER start installed — after a
    /// re-start replaced the pending login, a stale-state completion is
    /// refused with zero exchanges and the NEW transaction stays alive (the
    /// correct state for the new flow still completes afterwards).
    #[tokio::test]
    async fn rr1_f04_stale_completion_after_restart_never_eats_the_new_transaction() {
        let stub = StubServer::start(vec![(
            TOKEN_PATH,
            vec![token_response("at-f30-restart", "rt-f30-restart")],
        )])
        .await;
        let boot = boot(
            "rr1f04f30b",
            &pkce_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "first login start: {body}");
        let stale_state =
            state_of_authorize_url(body["start"]["authorizeUrl"].as_str().expect("url"));

        // A second start REPLACES the pending transaction (the first
        // listener is cancelled; the first state is dead).
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "second login start: {body}");
        let fresh_state =
            state_of_authorize_url(body["start"]["authorizeUrl"].as_str().expect("url"));
        assert_ne!(stale_state, fresh_state, "each start mints a new state");

        // The stale completion: refused, zero exchanges, and it must NOT
        // kill the new transaction.
        let stale = serde_json::json!({"state": stale_state, "code": "stale-code"});
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/callback",
            Some(&management.token),
            Some(&stale.to_string()),
        )
        .await;
        assert_eq!(status, 409, "stale state refused: {body}");
        assert_eq!(
            stub.hits_for(TOKEN_PATH),
            0,
            "zero exchanges for the stale leg"
        );

        // The NEW transaction still completes with its own state.
        let fresh = serde_json::json!({"state": fresh_state, "code": "fresh-code"});
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/callback",
            Some(&management.token),
            Some(&fresh.to_string()),
        )
        .await;
        assert_eq!(status, 200, "the new transaction survives: {body}");
        assert_eq!(body["outcome"]["status"], "loggedIn");
        assert_eq!(stub.hits_for(TOKEN_PATH), 1, "exactly one exchange");
        let row = credential_status(&management, "main").await;
        assert_eq!(row["loggedIn"], true);

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// LA-99D6C304D697 (device half): start returns the device prompt;
    /// poll walks pending → slow_down → done; the installed credential is
    /// durable; a logout clears the login state.
    #[tokio::test]
    async fn rr1_f04_device_start_poll_done_then_logout() {
        let stub = StubServer::start(vec![
            (DEVICE_PATH, vec![device_authorization_response()]),
            (
                TOKEN_PATH,
                vec![
                    poll_response(Some("authorization_pending")),
                    poll_response(Some("slow_down")),
                    poll_response(None),
                ],
            ),
        ])
        .await;
        let boot = boot(
            "rr1f04dev",
            &device_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        // Start: the device prompt (user code + verification URI + cadence).
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "device login start: {body}");
        let start = &body["start"];
        assert_eq!(start["flow"], "deviceCode");
        assert_eq!(start["userCode"], "ABCD-EFGH");
        assert_eq!(
            start["verificationUri"],
            "https://auth.example.invalid/activate"
        );
        assert!(start["intervalMs"].as_u64().unwrap() >= 1000);

        // The device authorization request hit the stand-in.
        assert_eq!(stub.hits_for(DEVICE_PATH), 1);
        let device_request = &stub.requests_for(DEVICE_PATH)[0].1;
        assert!(
            device_request.contains("client_id=client-rr1-dev"),
            "{device_request}"
        );

        // Poll 1: pending.
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "poll 1: {body}");
        assert_eq!(body["outcome"]["status"], "pending");
        assert_eq!(body["outcome"]["slowDown"], false);

        // Poll 2: slow_down — the interval GROWS (the incumbent's +5s rule).
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "poll 2: {body}");
        assert_eq!(body["outcome"]["status"], "pending");
        assert_eq!(body["outcome"]["slowDown"], true);
        assert!(
            body["outcome"]["intervalMs"].as_u64().unwrap() >= 6_000,
            "slow_down grew the interval: {body}"
        );

        // Poll 3: done — the token installs durably.
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "poll 3: {body}");
        assert_eq!(body["outcome"]["status"], "loggedIn");

        // The three polls carried the device grant (and no auth headers).
        let polls = stub.requests_for(TOKEN_PATH);
        assert_eq!(polls.len(), 3);
        for (_headers, body) in &polls {
            assert!(
                body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"),
                "{body}"
            );
            assert!(body.contains("device_code=dev-code-rr1"), "{body}");
        }

        let row = credential_status(&management, "main").await;
        assert_eq!(row["loggedIn"], true);
        let store = read_store_file(&boot.runtime_dir);
        assert_eq!(
            store["providers"]["main"]["tokens"]["accessToken"],
            "at-device-login"
        );

        // LA-FC80B6C4FBE4 ties in: logout clears the auth cache and the
        // available models drop to zero.
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/logout",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "logout: {body}");
        let row = credential_status(&management, "main").await;
        assert_eq!(row["loggedIn"], false);
        assert_eq!(row["state"], "not_logged_in");
        assert_eq!(row["availableModels"], 0);

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// LA-99D6C304D697 failure honesty: a terminal poll verdict (denial)
    /// consumes the login and never reports logged in; the store stays
    /// empty of tokens.
    #[tokio::test]
    async fn rr1_f04_device_denial_never_reports_logged_in() {
        let stub = StubServer::start(vec![
            (DEVICE_PATH, vec![device_authorization_response()]),
            (TOKEN_PATH, vec![poll_response(Some("access_denied"))]),
        ])
        .await;
        let boot = boot(
            "rr1f04deny",
            &device_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 409, "a denial is a loud failure: {body}");
        let row = credential_status(&management, "main").await;
        assert_eq!(row["loggedIn"], false, "failure never reports logged in");
        assert_eq!(row["state"], "not_logged_in");
        // The transaction is spent: another poll refuses (no silent retry).
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 409, "the failed login is over");
        assert!(
            !boot.runtime_dir.join("credentials.json").exists(),
            "a failed login writes no credential store"
        );

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// The login fences: a revoke mid-login kills the transaction (a late
    /// completion cannot install); a replacing reload mid-device-login
    /// fences the done install against the OLD instance.
    #[tokio::test]
    async fn rr1_f04_login_midflight_revoke_and_reload_fence_late_installs() {
        // Leg 1: revoke mid-pending → the transaction is gone.
        let stub = StubServer::start(vec![
            (DEVICE_PATH, vec![device_authorization_response()]),
            (TOKEN_PATH, vec![poll_response(None)]),
        ])
        .await;
        let boot_one = boot(
            "rr1f04fence",
            &device_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot_one.state).await;
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        // The revoke lands while the login is pending.
        let (status, _) = http_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/revoke",
            Some(&management.token),
        )
        .await;
        assert_eq!(status, 200);
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 409, "a revoked provider has no live login: {body}");
        let row = credential_status(&management, "main").await;
        assert_eq!(row["state"], "revoked");
        assert_eq!(row["loggedIn"], false);
        // Nothing installed: the store has no token row.
        if boot_one.runtime_dir.join("credentials.json").exists() {
            let store = read_store_file(&boot_one.runtime_dir);
            assert!(
                store["providers"]["main"]["tokens"].is_null(),
                "a late login installed into a revoked provider: {store}"
            );
        }
        management.stop().await;
        stub.stop().await;
        teardown(boot_one).await;

        // Leg 2: a REPLACING reload mid-device-login fences the done
        // install (the transaction's cell instance is gone).
        let stub = StubServer::start(vec![
            (DEVICE_PATH, vec![device_authorization_response()]),
            (
                TOKEN_PATH,
                vec![
                    poll_response(Some("authorization_pending")),
                    poll_response(None),
                ],
            ),
        ])
        .await;
        let plane_json = device_plane(&stub.endpoint(), &stub.token_endpoint());
        let boot_two = boot("rr1f04fence2", &plane_json, None).await;
        let management = serve_management(&boot_two.state).await;
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        // Poll once (pending), then REPLACE the seed (a different client
        // id) through the real reload surface.
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let replaced = plane_json.replace("client-rr1-dev", "client-rr1-dev-v2");
        assert_ne!(replaced, plane_json);
        let config_json = format!(
            r#"{{"home": {}, "workspace": {}, {replaced}}}"#,
            serde_json::to_string(&boot_two.home.to_string_lossy()).expect("home json"),
            serde_json::to_string(&boot_two.workspace.to_string_lossy()).expect("ws json"),
        );
        std::fs::write(&boot_two.config_path, config_json).expect("rewrite config");
        let (status, body) = http_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/reload",
            Some(&management.token),
        )
        .await;
        assert_eq!(status, 200, "replacing reload: {body}");
        // The done poll fences against the replaced instance: refused, and
        // the NEW cell stays logged OUT (the old world's tokens never land).
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 409, "the late done is fenced: {body}");
        let row = credential_status(&management, "main").await;
        assert_eq!(
            row["loggedIn"], false,
            "no token landed in the replaced world"
        );
        if boot_two.runtime_dir.join("credentials.json").exists() {
            let store = read_store_file(&boot_two.runtime_dir);
            assert!(
                store["providers"]["main"]["tokens"].is_null()
                    || store["providers"].get("main").is_none(),
                "the fenced install never persisted: {store}"
            );
        }

        management.stop().await;
        stub.stop().await;
        teardown(boot_two).await;
    }

    /// LA-CA0BF9A7AEA9: every OAuth provider reports loggedIn and its
    /// available model count (0 when logged out); non-OAuth kinds report
    /// the documented non-applicable shape.
    #[tokio::test]
    async fn rr1_f04_status_reports_logged_in_and_available_model_counts() {
        let stub = StubServer::start(vec![
            (DEVICE_PATH, vec![device_authorization_response()]),
            (TOKEN_PATH, vec![poll_response(None)]),
        ])
        .await;
        let boot = boot(
            "rr1f04stat",
            &device_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        // Logged out: zero available models.
        let row = credential_status(&management, "main").await;
        assert_eq!(row["kind"], "oauth");
        assert_eq!(row["loggedIn"], false);
        assert_eq!(row["availableModels"], 0);

        // Login + a custom model: the count is the config-bound ∪ custom.
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/oauth/main/models",
            Some(&management.token),
            Some(r#"{"modelId": "custom-f04-model"}"#),
        )
        .await;
        assert_eq!(status, 200, "custom model add: {body}");
        let row = credential_status(&management, "main").await;
        assert_eq!(row["loggedIn"], true);
        assert_eq!(
            row["availableModels"], 2,
            "config-bound stub-model + the custom id"
        );

        // The static aux provider: the OAuth fields are explicitly
        // non-applicable (loggedIn=false, 0 models) with its own state.
        let row = credential_status(&management, "aux").await;
        assert_eq!(row["kind"], "apiKey");
        assert_eq!(row["state"], "ready");
        assert_eq!(row["loggedIn"], false);
        assert_eq!(row["availableModels"], 0);

        // Logout: the available count drops to zero with the login.
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/logout",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let row = credential_status(&management, "main").await;
        assert_eq!(row["loggedIn"], false);
        assert_eq!(row["availableModels"], 0);

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// LA-CFEC64F68DDE: the model ids of a specified OAuth provider; a
    /// non-OAuth provider is EXPLICITLY rejected; an unknown provider 404s.
    #[tokio::test]
    async fn rr1_f04_oauth_model_listing_and_non_oauth_rejection() {
        let stub = StubServer::start(vec![
            (DEVICE_PATH, vec![device_authorization_response()]),
            (TOKEN_PATH, vec![poll_response(None)]),
        ])
        .await;
        let boot = boot(
            "rr1f04list",
            &device_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        // Logged out: the listing is honest (empty, loggedIn=false).
        let (status, body) = http_json_request(
            management.addr,
            "GET",
            "/lingxi/v1/models/oauth/main/models",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "listing: {body}");
        assert_eq!(body["loggedIn"], false);
        assert_eq!(body["models"].as_array().map(Vec::len), Some(0));

        // Log in and add a custom model: the listing is the dedup union.
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/login/poll",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/oauth/main/models",
            Some(&management.token),
            Some(r#"{"modelId": "custom-list-model"}"#),
        )
        .await;
        assert_eq!(status, 200);
        let (status, body) = http_json_request(
            management.addr,
            "GET",
            "/lingxi/v1/models/oauth/main/models",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let models: Vec<String> = body["models"]
            .as_array()
            .expect("models")
            .iter()
            .map(|value| value.as_str().expect("id").to_string())
            .collect();
        assert!(models.contains(&"stub-model".to_string()), "{models:?}");
        assert!(
            models.contains(&"custom-list-model".to_string()),
            "{models:?}"
        );
        assert_eq!(models.len(), 2, "deduplicated union: {models:?}");

        // A NON-OAuth provider is explicitly rejected (not an empty list).
        let (status, body) = http_json_request(
            management.addr,
            "GET",
            "/lingxi/v1/models/oauth/aux/models",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(
            status, 409,
            "non-OAuth providers are explicitly rejected: {body}"
        );
        assert!(
            body.to_string().to_lowercase().contains("oauth"),
            "the rejection names the OAuth-only rule: {body}"
        );

        // An unknown provider is a 404.
        let (status, _) = http_json_request(
            management.addr,
            "GET",
            "/lingxi/v1/models/oauth/ghost/models",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 404);

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// LA-16CEB6D12A6A: adding a custom modelId refreshes and returns the
    /// new list; duplicates, empty ids and non-OAuth providers change
    /// NOTHING; the registry survives a restart (unknown store fields ride
    /// along untouched).
    #[tokio::test]
    async fn rr1_f04_add_custom_model_id_refreshes_and_persists() {
        let stub = StubServer::start(vec![(CHAT_PATH, vec![]), (TOKEN_PATH, vec![])]).await;
        let plane_json = device_plane(&stub.endpoint(), &stub.token_endpoint());
        let boot = boot("rr1f04add", &plane_json, None).await;
        let management = serve_management(&boot.state).await;

        // The add serves even while logged out (the registry is not
        // credential material) and returns the refreshed listing.
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/oauth/main/models",
            Some(&management.token),
            Some(r#"{"modelId": "f04-added-model"}"#),
        )
        .await;
        assert_eq!(status, 200, "add: {body}");
        assert_eq!(body["loggedIn"], false, "still logged out");

        // A duplicate add changes nothing and reports loudly.
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/oauth/main/models",
            Some(&management.token),
            Some(r#"{"modelId": "f04-added-model"}"#),
        )
        .await;
        assert_eq!(status, 409, "duplicate add refused: {body}");
        let store = read_store_file(&boot.runtime_dir);
        assert_eq!(
            store["providers"]["main"]["customModels"]
                .as_array()
                .map(Vec::len),
            Some(1),
            "the registry is unchanged after the refused add: {store}"
        );

        // Empty / whitespace ids are refused.
        for bad in ["", "   ", "has space"] {
            let payload = serde_json::json!({"modelId": bad}).to_string();
            let (status, _) = http_json_request(
                management.addr,
                "POST",
                "/lingxi/v1/models/oauth/main/models",
                Some(&management.token),
                Some(&payload),
            )
            .await;
            assert_eq!(status, 409, "invalid model id {bad:?} refused");
        }

        // A non-OAuth provider is explicitly rejected.
        let (status, _) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/oauth/aux/models",
            Some(&management.token),
            Some(r#"{"modelId": "never"}"#),
        )
        .await;
        assert_eq!(status, 409, "non-OAuth registry add rejected");

        // The registry persists across a restart, and unknown fields ride
        // along untouched.
        let store_path = boot.runtime_dir.join("credentials.json");
        let mut on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&store_path).expect("store")).expect("json");
        on_disk["providers"]["main"]["futureField"] = serde_json::json!(42);
        std::fs::write(
            &store_path,
            serde_json::to_string_pretty(&on_disk).expect("pretty"),
        )
        .expect("rewrite store");
        let plane =
            ModelPlaneConfig::parse_and_validate(&format!("{{{plane_json}}}")).expect("plane");
        let restarted = CredentialService::bootstrap(
            &plane,
            &boot.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("restart");
        let listing = restarted.oauth_models("main").await.expect("listing");
        assert!(!listing.logged_in, "still logged out after the restart");
        let after: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&store_path).expect("store")).expect("json");
        assert_eq!(
            after["providers"]["main"]["futureField"], 42,
            "unknown fields kept"
        );
        assert_eq!(
            after["providers"]["main"]["customModels"][0], "f04-added-model",
            "the custom registry survived the restart: {after}"
        );

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// LA-8060BE8AA02C: removing a custom modelId refreshes and returns
    /// the new list; removing an absent id changes nothing and reports.
    #[tokio::test]
    async fn rr1_f04_remove_custom_model_id_refreshes_the_list() {
        let stub = StubServer::start(vec![(CHAT_PATH, vec![]), (TOKEN_PATH, vec![])]).await;
        let boot = boot(
            "rr1f04rm",
            &device_plane(&stub.endpoint(), &stub.token_endpoint()),
            None,
        )
        .await;
        let management = serve_management(&boot.state).await;

        for id in ["f04-remove-me", "f04-keep-me"] {
            let (status, _) = http_json_request(
                management.addr,
                "POST",
                "/lingxi/v1/models/oauth/main/models",
                Some(&management.token),
                Some(&serde_json::json!({"modelId": id}).to_string()),
            )
            .await;
            assert_eq!(status, 200);
        }

        // Remove one: the refreshed registry no longer carries it (the
        // LISTING serves only while logged in, so verify the store — the
        // durable registry — plus the response's own shape).
        let (status, body) = http_json_request(
            management.addr,
            "DELETE",
            "/lingxi/v1/models/oauth/main/models/f04-remove-me",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 200, "remove: {body}");
        assert_eq!(body["loggedIn"], false, "still logged out");
        let store = read_store_file(&boot.runtime_dir);
        let registry: Vec<&str> = store["providers"]["main"]["customModels"]
            .as_array()
            .expect("registry")
            .iter()
            .map(|value| value.as_str().expect("id"))
            .collect();
        assert!(!registry.contains(&"f04-remove-me"), "{registry:?}");
        assert!(registry.contains(&"f04-keep-me"), "{registry:?}");

        // Removing an absent id is a loud no-op.
        let (status, _) = http_json_request(
            management.addr,
            "DELETE",
            "/lingxi/v1/models/oauth/main/models/f04-remove-me",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 409, "absent removal refused");
        let store = read_store_file(&boot.runtime_dir);
        assert_eq!(
            store["providers"]["main"]["customModels"]
                .as_array()
                .map(Vec::len),
            Some(1),
            "the registry is unchanged after the refused removal: {store}"
        );

        // A non-OAuth provider is explicitly rejected.
        let (status, _) = http_json_request(
            management.addr,
            "DELETE",
            "/lingxi/v1/models/oauth/aux/models/anything",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(status, 409, "non-OAuth registry removal rejected");

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// LA-FC80B6C4FBE4: logout deletes the credential, clears the auth
    /// cache and refreshes the model list; the result is honest for a
    /// logged-out provider; the custom registry SURVIVES logout; a
    /// non-OAuth logout is explicitly rejected.
    #[tokio::test]
    async fn rr1_f04_logout_clears_credentials_cache_and_refreshes_models() {
        let stub = StubServer::start(vec![(CHAT_PATH, vec![]), (TOKEN_PATH, vec![])]).await;
        let plane_json = device_plane(&stub.endpoint(), &stub.token_endpoint());
        let boot = boot("rr1f04out", &plane_json, None).await;
        let management = serve_management(&boot.state).await;

        // Not logged in yet: logout is still an honest, explicit outcome.
        let (status, body) = http_json_request(
            management.addr,
            "POST",
            "/lingxi/v1/models/credentials/main/logout",
            Some(&management.token),
            None,
        )
        .await;
        assert_eq!(
            status, 200,
            "logout of a logged-out provider is honest: {body}"
        );

        // Seed a logged-in state directly through the store (the login
        // flow itself is covered above; here the logout legs matter).
        seed_store_file(
            &boot.runtime_dir,
            "main",
            "at-logout",
            "rt-logout",
            UNEXPIRED,
        );
        let plane =
            ModelPlaneConfig::parse_and_validate(&format!("{{{plane_json}}}")).expect("plane");
        let logged_in = CredentialService::bootstrap(
            &plane,
            &boot.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("bootstrap");
        assert!(
            logged_in
                .status()
                .await
                .iter()
                .find(|r| r.provider == "main")
                .unwrap()
                .logged_in
        );

        // A custom model exists BEFORE logout — the registry is not a
        // credential and must survive.
        logged_in
            .oauth_add_model("main", "f04-survivor")
            .await
            .expect("add");

        let listing = logged_in.logout("main").await.expect("logout");
        assert!(!listing.logged_in);
        assert!(
            listing.models.is_empty(),
            "a logged-out provider serves zero available models"
        );
        let row = logged_in
            .status()
            .await
            .into_iter()
            .find(|row| row.provider == "main")
            .expect("main");
        assert_eq!(row.state, "not_logged_in");
        assert!(!row.logged_in);
        assert_eq!(row.available_models, 0);
        // The store: tokens gone, the custom registry kept.
        let store = read_store_file(&boot.runtime_dir);
        assert!(
            store["providers"]["main"]["tokens"].is_null(),
            "logout deleted the token material: {store}"
        );
        assert_eq!(
            store["providers"]["main"]["customModels"][0], "f04-survivor",
            "the custom model registry survived the logout: {store}"
        );
        // The registry is intact after a restart (still logged out).
        let restarted = CredentialService::bootstrap(
            &plane,
            &boot.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("restart");
        let registry = restarted.oauth_models("main").await.expect("listing");
        assert!(!registry.logged_in);
        // Non-OAuth logout: explicitly rejected.
        assert!(restarted.logout("aux").await.is_err());

        management.stop().await;
        stub.stop().await;
        teardown(boot).await;
    }

    /// Service-level determinism leg the HTTP surface cannot time-travel:
    /// past the 10-minute state TTL, the CORRECT state still refuses (zero
    /// writes) — driven by the injected ManualClock.
    #[tokio::test]
    async fn rr1_f04_expired_state_refuses_even_with_the_correct_state() {
        use lingxi_service::inject::ManualClock;
        let clock = Arc::new(ManualClock::new(1_000_000));
        let stub = StubServer::start(vec![(TOKEN_PATH, vec![])]).await;
        let service = CredentialService::new(
            &ModelPlaneConfig::parse_and_validate(&format!(
                "{{{}}}",
                pkce_plane("http://127.0.0.1:9/v1", &stub.token_endpoint())
            ))
            .expect("plane"),
            None,
            Arc::new(ProductionRefreshDriver::new(OAUTH_REQUEST_TIMEOUT).unwrap()),
            clock.clone(),
        );
        let start = service
            .oauth_start("principal_local", "main")
            .await
            .expect("start");
        let lingxi_service::credentials::login::LoginStart::AuthorizationCodePkce {
            authorize_url,
            ..
        } = &start
        else {
            panic!("pkce start");
        };
        let state = state_of_authorize_url(authorize_url);
        // Before expiry: a wrong state refuses (zero writes, login alive).
        assert!(service
            .oauth_complete_code("principal_local", "main", "wrong", "code-x")
            .await
            .is_err());
        // Past the 10-minute state TTL: the correct state refuses too.
        clock.advance(601_000);
        assert!(
            service
                .oauth_complete_code("principal_local", "main", &state, "code-x")
                .await
                .is_err(),
            "an expired state never completes"
        );
        assert_eq!(
            stub.hits_for(TOKEN_PATH),
            0,
            "no exchange ever left on a refused/expired state"
        );
        stub.stop().await;
    }
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
        "models": {{"chat": {{"provider": "a", "model": "shared-model",
            "capabilities": {{"tools": true}}}}}}"#,
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
        r#""chat": {"provider": "a", "model": "shared-model",
            "capabilities": {"tools": true}}"#,
        r#""chat": {"provider": "b", "model": "shared-model",
            "capabilities": {"tools": true}}"#,
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
