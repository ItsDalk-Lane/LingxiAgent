//! R05-T07 acceptance: usage ledger, trace causality and persistence at
//! the REAL service level (C01/C02/C03/C05/C06/C08/C09/C10).
//!
//! Harness boundary: the model HTTP far end is a scripted loopback stub
//! (the ONLY allowed double); everything else is the production chain —
//! config-file plane → `ConfigModelGateway` → `CredentialService` → real
//! SSE protocol adapter → `RunSupervisor::drive_run` → the REAL
//! `RunDatabase` single-writer transactions (including the new
//! `model_call_usage` ledger) → the real `EventService`.
//!
//! What each case pins:
//! - C01: a session's runs keep one continuous trace; every model call is
//!   independently queryable by run AND session (no per-call session
//!   roots);
//! - C02: background / subagent / worker-callback rows carry their TRUE
//!   parentage (origin + parent run + cause ref from the driver's
//!   lineage facts — never timing);
//! - C03: physical requests are never merged into invisibility (a
//!   401-refresh retry is ONE row with `transport_attempts = 2`; a
//!   retryable-failed call and its retry are TWO rows; a request with no
//!   usage is `unknown`, never 0);
//! - C05: reported / partial / unknown / invalid states stay distinct in
//!   both the ledger row and the wire event (missing is never written as
//!   an actual 0);
//! - C06: a provider usage object with illegal numbers marks the row
//!   `invalid` (the turn still settles — no truncation, no saturation);
//! - C08: cost stays explicitly unknown (no price source exists; token
//!   facts are recorded either way);
//! - C09: owner-scoped queries isolate; the provider secret never lands
//!   in the runs database;
//! - C10: a ledger write failure publishes NO completion event and fails
//!   the run loudly; the retry is idempotent.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_kernel::ports::StoragePort as _;
use lingxi_kernel::subagent::{RunLineage, RunOrigin, SessionPermissionMode};
use lingxi_kernel::usage::{ModelCallUsageRecord, ModelUsageQuery, UsageProvenance};
use lingxi_kernel::{Principal as KernelPrincipal, RunFinish};
use lingxi_service::runs::{DriveAuthorization, RunGrant};
use lingxi_service::{
    prepare_layout, ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState, SubscribeOutcome,
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
    #[allow(dead_code)] // the per-path accessor below is the read surface
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
                    tokio::spawn({
                        let hits = Arc::clone(&hits);
                        let requests = Arc::clone(&requests);
                        let scripts = Arc::clone(&scripts);
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
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
            hits,
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    fn token_endpoint(&self) -> String {
        format!("http://{}/token", self.addr)
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
            .expect("stub body read stalled")
            .expect("stub body read");
        if read == 0 {
            panic!("stub: connection closed mid-body");
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

// ── canned answers ───────────────────────────────────────────────────────────

const CHAT_PATH: &str = "/v1/chat/completions";
const TOKEN_PATH: &str = "/token";
const UNEXPIRED: u64 = 4_000_000_000_000;

/// A complete streaming final with a FULL usage object.
fn final_with_usage(text: &str, input: u64, output: u64) -> StubResponse {
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-stub", "model": "stub-model",
            "choices": [{"index": 0, "finish_reason": null,
                "delta": {"role": "assistant", "content": text}}]
        }),
        serde_json::json!({
            "id": "chatcmpl-stub",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
        serde_json::json!({
            "id": "chatcmpl-stub", "choices": [],
            "usage": {"prompt_tokens": input, "completion_tokens": output}
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse::sse(body)
}

/// A complete streaming final WITHOUT any usage object (C05's unknown leg).
fn final_without_usage(text: &str) -> StubResponse {
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-stub", "model": "stub-model",
            "choices": [{"index": 0, "finish_reason": null,
                "delta": {"role": "assistant", "content": text}}]
        }),
        serde_json::json!({
            "id": "chatcmpl-stub",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse::sse(body)
}

/// A HALF-reported usage: the input half only (C05's partial leg).
fn final_with_half_usage(text: &str, input: u64) -> StubResponse {
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-stub", "model": "stub-model",
            "choices": [{"index": 0, "finish_reason": null,
                "delta": {"role": "assistant", "content": text}}]
        }),
        serde_json::json!({
            "id": "chatcmpl-stub",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
        serde_json::json!({
            "id": "chatcmpl-stub", "choices": [],
            "usage": {"prompt_tokens": input}
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse::sse(body)
}

/// An ILLEGAL usage object (negative count — C06's invalid leg).
fn final_with_negative_usage(text: &str) -> StubResponse {
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-stub", "model": "stub-model",
            "choices": [{"index": 0, "finish_reason": null,
                "delta": {"role": "assistant", "content": text}}]
        }),
        serde_json::json!({
            "id": "chatcmpl-stub",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
        serde_json::json!({
            "id": "chatcmpl-stub", "choices": [],
            "usage": {"prompt_tokens": -3, "completion_tokens": 5}
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse::sse(body)
}

fn unauthorized(detail: &str) -> StubResponse {
    StubResponse::immediate(
        401,
        serde_json::json!({"error": {"message": detail}}).to_string(),
    )
}

fn server_error(detail: &str) -> StubResponse {
    StubResponse::immediate(
        500,
        serde_json::json!({"error": {"message": detail}}).to_string(),
    )
}

fn token_response(access: &str, refresh: &str) -> StubResponse {
    StubResponse::immediate(
        200,
        serde_json::json!({
            "access_token": access, "refresh_token": refresh, "expires_in": 3600
        })
        .to_string(),
    )
}

// ── plane builders ───────────────────────────────────────────────────────────

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

fn plane_oauth(endpoint: &str, token_endpoint: &str) -> String {
    format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "oauth", "flow": "deviceCode", "clientId": "client-r05t07",
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

// ── harness ──────────────────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t07-{tag}-{}-{}",
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
    runtime_dir: PathBuf,
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
    let runtime_dir = layout.runtime_dir.clone();
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
        runtime_dir,
    }
}

async fn boot_oauth(tag: &str, plane_json: &str) -> PlaneBoot {
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
    seed_store_file(&layout.runtime_dir, "main", "at-old", "rt-1", UNEXPIRED);
    let file = lingxi_service::config::read_service_config(&config_path).expect("service config");
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .expect("plane resolves")
            .expect("plane present");
    let runtime_dir = layout.runtime_dir.clone();
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
        runtime_dir,
    }
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

async fn run_status(state: &ServiceState, run_id: &str) -> String {
    state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query status")
        .expect("run row")
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

// ── C01: session trace continuity + per-call queryability ────────────────────

#[tokio::test]
async fn c01_multi_run_session_trace_stays_continuous_and_per_call_queryable() {
    let stub = StubServer::start(vec![(
        CHAT_PATH,
        vec![
            final_with_usage("first answer", 10, 4),
            final_with_usage("second answer", 20, 6),
        ],
    )])
    .await;
    let boot = boot("c01", &plane_api_key(&stub.endpoint(), "sk-t07-c01")).await;

    let run_one = execute_on(&boot.state, "sess_local_alpha", "first task").await;
    let run_two = execute_on(&boot.state, "sess_local_alpha", "second task").await;
    for run_id in [&run_one, &run_two] {
        assert_eq!(
            run_status(&boot.state, run_id).await,
            "completed",
            "run {run_id} completed"
        );
    }

    // Session-level association: ONE query returns BOTH runs' rows, in
    // order, sharing the session identity — no per-call session root was
    // invented between them.
    let rows = usage_rows(
        &boot.state,
        ModelUsageQuery {
            owner_user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
            session_id: Some("sess_local_alpha".to_string()),
            run_id: None,
        },
    )
    .await;
    assert_eq!(rows.len(), 2, "one row per model call, session-scoped");
    assert_eq!(rows[0].session_id.as_deref(), Some("sess_local_alpha"));
    assert_eq!(rows[1].session_id.as_deref(), Some("sess_local_alpha"));
    assert_eq!(rows[0].run_id.as_deref(), Some(run_one.as_str()));
    assert_eq!(rows[1].run_id.as_deref(), Some(run_two.as_str()));
    assert_ne!(rows[0].run_id, rows[1].run_id);
    assert_eq!(rows[0].model_call_id, format!("{run_one}-mc0001"));
    assert_eq!(rows[1].model_call_id, format!("{run_two}-mc0001"));

    // Per-request independence: each run's query returns exactly its own
    // call, with the full correlation set (C01's "每次请求独立ModelCall关联").
    for (run_id, input, output) in [(&run_one, 10, 4), (&run_two, 20, 6)] {
        let rows = usage_rows(
            &boot.state,
            ModelUsageQuery {
                owner_user_id: None,
                session_id: None,
                run_id: Some(run_id.clone()),
            },
        )
        .await;
        assert_eq!(rows.len(), 1, "one independently queryable row");
        let row = &rows[0];
        assert_eq!(row.purpose, "chat");
        assert_eq!(row.origin, "user");
        assert_eq!(row.provider, "main");
        assert_eq!(row.model, "stub-model");
        assert_eq!(row.protocol, "openai-completions");
        assert_eq!(
            row.attempt.as_deref(),
            Some(format!("{run_id}#a1").as_str())
        );
        let usage = row.usage.as_ref().expect("reported usage");
        assert_eq!(usage.input_tokens, Some(input));
        assert_eq!(usage.output_tokens, Some(output));
        assert_eq!(usage.provenance, UsageProvenance::Reported);
    }

    stub.stop().await;
    teardown(boot).await;
}

// ── C02: background / subagent / worker-callback parentage ───────────────────

#[tokio::test]
async fn c02_background_subagent_and_worker_rows_carry_true_parentage() {
    // The stub serves: the main run, the background run, the subagent
    // child run, and the worker callback's aux answer.
    let stub = StubServer::start(vec![(
        CHAT_PATH,
        vec![
            final_with_usage("main answer", 10, 4),
            final_with_usage("background answer", 20, 5),
            final_with_usage("child answer", 30, 6),
            final_with_usage("summary answer", 40, 7),
        ],
    )])
    .await;
    let boot = boot("c02", &plane_with_aux(&stub.endpoint(), "sk-t07-c02")).await;
    let state = &boot.state;

    // 1) The MAIN run (user submission).
    let main_run = execute_on(state, "sess_local_alpha", "main task").await;
    assert_eq!(run_status(state, &main_run).await, "completed");

    // 2) The INDEPENDENT BACKGROUND run (its own root, same session by
    //    design — the ledger must keep the two roots distinct).
    let storage = Arc::clone(state.storage());
    let accepted = state
        .sessions()
        .execute_background_for(
            &storage,
            state.events(),
            state.runs(),
            state.background(),
            &owner_principal(),
            "sess_local_alpha",
            &ExecuteSubmission {
                input: "background task",
                request_id: None,
            },
            1_790_409_600_000,
        )
        .await
        .expect("background accepted");
    let background_run = accepted.run_id.clone();
    // Wait for the detached drive to settle.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if run_status(state, &background_run).await == "completed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        run_status(state, &background_run).await,
        "completed",
        "the background run settled"
    );

    // 3) The SUBAGENT child run — the REAL drive chain with the subagent
    //    lineage (exactly what the subagent runtime hands the supervisor).
    let child_run =
        lingxi_protocol::RunId::new(format!("run-subagent-child-{}", std::process::id()));
    let authorization = DriveAuthorization {
        lineage: RunLineage {
            parent_run_id: Some(lingxi_protocol::RunId::new(main_run.clone())),
            origin: RunOrigin::Subagent,
            source_message_id: Some(format!("{main_run}-mc0001")),
            cause_id: Some(format!("{main_run}-tc0001")),
        },
        grant: RunGrant::Subagent {
            tier: lingxi_kernel::subagent::ToolAccessTier::ReadOnly,
        },
        session_mode: SessionPermissionMode::Operate,
    };
    let finish = state
        .runs()
        .drive_run(
            storage.as_ref(),
            state.events(),
            &KernelPrincipal::LocalUser,
            "sess_local_alpha",
            "lingxi",
            child_run.as_str(),
            "child task",
            1,
            1_790_409_600_000,
            None,
            None,
            authorization,
            None,
            "sess_local_alpha",
        )
        .await
        .expect("child drive");
    assert!(
        matches!(finish, RunFinish::CompletedWithFinal { .. }),
        "child completed: {finish:?}"
    );

    // 4) The WORKER CALLBACK through the production ledger trace: the
    //    aux summarize route serves one callback under the MAIN run's
    //    context; its row must join to the parent invocation.
    let gateway = state.model_gateway().expect("gateway").clone();
    let credentials = state.credential_service().expect("credentials").clone();
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        gateway,
        credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("aux executor");
    let worker_model = lingxi_service::workermodel::GatewayWorkerModel::with_trace(
        Arc::new(executor),
        state.runs().quotas_shared(),
        Arc::new(lingxi_service::workermodel::LedgerWorkerCallbackTrace::new(
            Arc::clone(&storage),
            Arc::new(lingxi_service::inject::SystemClock),
        )),
    );
    let ctx = lingxi_kernel::RunContext {
        principal: KernelPrincipal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha"),
        run_id: lingxi_protocol::RunId::new(main_run.clone()),
        attempt: lingxi_protocol::AttemptId::new(format!("{main_run}#a1")),
        generation: 1,
    };
    use lingxi_service::workerrpc::WorkerModelPort as _;
    let reply = worker_model
        .complete(
            &ctx,
            "plug",
            &format!("{main_run}-tc0001"),
            "cb-1",
            &lingxi_service::workerrpc::WorkerModelRequest {
                prompt: "summarize this".to_string(),
                purpose: "summarize".to_string(),
                max_output_tokens: 64,
                deadline_unix_ms: None,
            },
        )
        .await
        .expect("callback settled");
    assert_eq!(reply.text, "summary answer");

    // ── the ledger verdict ──
    let rows = usage_rows(
        state,
        ModelUsageQuery {
            owner_user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
            session_id: Some("sess_local_alpha".to_string()),
            run_id: None,
        },
    )
    .await;
    assert_eq!(rows.len(), 4, "main + background + child + callback");

    let main_row = rows
        .iter()
        .find(|r| r.run_id.as_deref() == Some(main_run.as_str()) && r.purpose == "chat")
        .expect("main row");
    assert_eq!(main_row.origin, "user");
    assert_eq!(main_row.parent_run_id, None);

    let background_row = rows
        .iter()
        .find(|r| r.run_id.as_deref() == Some(background_run.as_str()))
        .expect("background row");
    // The background root is its OWN run — never merged into the main
    // run's accounting (its parent stays user-level).
    assert_eq!(background_row.origin, "user");
    assert_eq!(background_row.parent_run_id, None);

    let child_row = rows
        .iter()
        .find(|r| r.run_id.as_deref() == Some(child_run.as_str()))
        .expect("child row");
    assert_eq!(child_row.origin, "subagent");
    assert_eq!(
        child_row.parent_run_id.as_deref(),
        Some(main_run.as_str()),
        "the child's parent run is the REAL parent (a lineage fact)"
    );
    assert_eq!(
        child_row.cause_ref.as_deref(),
        Some(format!("{main_run}-tc0001").as_str()),
        "the causal anchor is the parent's tool call id"
    );

    let callback_row = rows
        .iter()
        .find(|r| r.purpose == "auxiliary.summarize")
        .expect("callback row");
    assert_eq!(callback_row.origin, "worker-callback");
    assert_eq!(callback_row.run_id.as_deref(), Some(main_run.as_str()));
    assert_eq!(
        callback_row.cause_ref.as_deref(),
        Some(format!("{main_run}-tc0001").as_str()),
        "the callback joins its parent worker invocation"
    );
    assert_eq!(callback_row.provider, "aux");
    assert_eq!(callback_row.model, "summarize-model");
    let usage = callback_row.usage.as_ref().expect("callback usage");
    assert_eq!(usage.input_tokens, Some(40));
    assert_eq!(usage.output_tokens, Some(7));

    stub.stop().await;
    teardown(boot).await;
}

// ── C03: physical requests are never merged into invisibility ────────────────

#[tokio::test]
async fn c03_refresh_retry_is_two_physical_requests_in_one_honest_row() {
    // OAuth plane; the chat endpoint answers 401 ONCE (the stale seeded
    // token), then succeeds after the single coordinated refresh. TWO
    // physical requests leave the process for ONE logical call.
    let stub = StubServer::start(vec![
        (
            CHAT_PATH,
            vec![
                unauthorized("stale token"),
                final_with_usage("refreshed answer", 12, 5),
            ],
        ),
        (TOKEN_PATH, vec![token_response("at-fresh", "rt-2")]),
    ])
    .await;
    let plane_json = plane_oauth(&stub.endpoint(), &stub.token_endpoint());
    let boot = boot_oauth("c03-refresh", &plane_json).await;

    let run_id = execute_on(&boot.state, "sess_local_alpha", "refresh task").await;
    assert_eq!(run_status(&boot.state, &run_id).await, "completed");
    assert_eq!(stub.hits_for(CHAT_PATH), 2, "two physical requests");
    assert_eq!(stub.hits_for(TOKEN_PATH), 1, "one coordinated refresh");

    let rows = usage_rows(
        &boot.state,
        ModelUsageQuery {
            owner_user_id: None,
            session_id: None,
            run_id: Some(run_id.clone()),
        },
    )
    .await;
    assert_eq!(rows.len(), 1, "ONE logical call — one row");
    let row = &rows[0];
    assert_eq!(
        row.transport_attempts, 2,
        "the refresh retry is counted as a second physical request"
    );
    let usage = row.usage.as_ref().expect("the successful request's usage");
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.output_tokens, Some(5));
    assert_eq!(usage.provenance, UsageProvenance::Reported);

    stub.stop().await;
    teardown(boot).await;
}

#[tokio::test]
async fn c03_retryable_failure_then_success_are_two_rows_and_unknown_is_not_zero() {
    // The first call FAILS with a retryable 500 (a possibly-billable
    // request whose usage is UNKNOWN); the driver's retry (a NEW attempt
    // of the same run) succeeds with usage. Two model calls — two rows,
    // never merged, never zeroed.
    let stub = StubServer::start(vec![(
        CHAT_PATH,
        vec![
            server_error("boom"),
            final_with_usage("retry answer", 15, 6),
        ],
    )])
    .await;
    let boot = boot("c03-retry", &plane_api_key(&stub.endpoint(), "sk-t07-c03r")).await;

    let run_id = execute_on(&boot.state, "sess_local_alpha", "retry task").await;
    assert_eq!(run_status(&boot.state, &run_id).await, "completed");
    assert_eq!(stub.hits_for(CHAT_PATH), 2, "the physical retry happened");

    let rows = usage_rows(
        &boot.state,
        ModelUsageQuery {
            owner_user_id: None,
            session_id: None,
            run_id: Some(run_id.clone()),
        },
    )
    .await;
    assert_eq!(rows.len(), 2, "each model call keeps its own row");
    assert_eq!(rows[0].model_call_id, format!("{run_id}-mc0001"));
    assert_eq!(rows[1].model_call_id, format!("{run_id}-mc0002"));
    // The failed request: usage UNKNOWN — not present, not zero.
    assert!(
        rows[0].usage.is_none(),
        "the failed request's usage stays unknown"
    );
    assert!(rows[0].invalid_detail.is_none());
    assert_eq!(rows[0].transport_attempts, 1);
    assert_eq!(
        rows[0].attempt.as_deref(),
        Some(format!("{run_id}#a1").as_str()),
        "the failed call ran under the first attempt"
    );
    // The successful retry: reported.
    let usage = rows[1].usage.as_ref().expect("retry usage");
    assert_eq!(usage.input_tokens, Some(15));
    assert_eq!(usage.output_tokens, Some(6));
    assert_eq!(
        rows[1].attempt.as_deref(),
        Some(format!("{run_id}#a2").as_str())
    );

    stub.stop().await;
    teardown(boot).await;
}

// ── C05 + C06: unknown / partial / invalid stay distinct, never zero ─────────

#[tokio::test]
async fn c05_unknown_partial_and_reported_states_stay_distinct_everywhere() {
    let stub = StubServer::start(vec![(
        CHAT_PATH,
        vec![
            final_without_usage("no usage answer"),
            final_with_half_usage("half usage answer", 7),
            final_with_usage("full usage answer", 9, 3),
        ],
    )])
    .await;
    let boot = boot("c05", &plane_api_key(&stub.endpoint(), "sk-t07-c05")).await;

    let unknown_run = execute_on(&boot.state, "sess_local_alpha", "task one").await;
    let partial_run = execute_on(&boot.state, "sess_local_alpha", "task two").await;
    let reported_run = execute_on(&boot.state, "sess_local_alpha", "task three").await;

    // Ledger: three DISTINCT states — unknown ≠ partial ≠ reported, and
    // no missing number was ever written as an actual 0.
    async fn row_of(state: &ServiceState, run: &str) -> ModelCallUsageRecord {
        let rows = usage_rows(
            state,
            ModelUsageQuery {
                owner_user_id: None,
                session_id: None,
                run_id: Some(run.to_string()),
            },
        )
        .await;
        assert_eq!(rows.len(), 1, "one row for {run}");
        rows.into_iter().next().unwrap()
    }
    let unknown_row = row_of(&boot.state, &unknown_run).await;
    let partial_row = row_of(&boot.state, &partial_run).await;
    let reported_row = row_of(&boot.state, &reported_run).await;

    assert!(unknown_row.usage.is_none(), "absent usage is unknown");
    assert_eq!(unknown_row.invalid_detail, None);
    let partial = partial_row.usage.as_ref().expect("partial fact exists");
    assert_eq!(partial.input_tokens, Some(7));
    assert_eq!(partial.output_tokens, None, "the missing half stays None");
    assert_eq!(
        partial.provenance,
        UsageProvenance::Partial {
            missing: vec!["output_tokens"]
        }
    );
    let reported = reported_row.usage.as_ref().expect("reported fact");
    assert_eq!(reported.input_tokens, Some(9));
    assert_eq!(reported.output_tokens, Some(3));
    assert_eq!(reported.provenance, UsageProvenance::Reported);

    // Wire interface (the UI-facing event): usage is null when the fact is
    // absent or half-known — never a zero masquerading as a measurement.
    for (run, expect_null) in [
        (&unknown_run, true),
        (&partial_run, true),
        (&reported_run, false),
    ] {
        let payload = boot
            .state
            .storage()
            .query_one_text(
                "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
                 'model_call_completed' ORDER BY seq DESC LIMIT 1",
                vec![run.to_string()],
            )
            .await
            .expect("event query")
            .expect("completed event exists");
        let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(
            value["usage"].is_null(),
            expect_null,
            "run {run}: the wire usage field is null exactly when the fact is not fully known"
        );
    }

    stub.stop().await;
    teardown(boot).await;
}

#[tokio::test]
async fn c06_illegal_usage_numbers_mark_the_row_invalid_and_never_distort() {
    let stub = StubServer::start(vec![(
        CHAT_PATH,
        vec![final_with_negative_usage("answer over illegal usage")],
    )])
    .await;
    let boot = boot("c06", &plane_api_key(&stub.endpoint(), "sk-t07-c06")).await;

    let run_id = execute_on(&boot.state, "sess_local_alpha", "task").await;
    // The TURN still settles (the response was good; only the usage object
    // was garbage) — but no number from it is trusted.
    assert_eq!(run_status(&boot.state, &run_id).await, "completed");

    let rows = usage_rows(
        &boot.state,
        ModelUsageQuery {
            owner_user_id: None,
            session_id: None,
            run_id: Some(run_id.clone()),
        },
    )
    .await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert!(
        row.usage.is_none(),
        "no number from an illegal usage object is trusted"
    );
    let detail = row.invalid_detail.as_deref().expect("invalid detail");
    assert!(
        detail.contains("prompt_tokens") && detail.contains("negative"),
        "the violation names its field and kind: {detail}"
    );

    // The wire event's usage stays null (never a truncated/saturated 0).
    let payload = boot
        .state
        .storage()
        .query_one_text(
            "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
             'model_call_completed' ORDER BY seq DESC LIMIT 1",
            vec![run_id.clone()],
        )
        .await
        .expect("event query")
        .expect("completed event");
    let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
    assert!(value["usage"].is_null(), "invalid projects no usage number");

    stub.stop().await;
    teardown(boot).await;
}

// ── C08: no price basis, no invented cost ────────────────────────────────────

#[tokio::test]
async fn c08_cost_stays_unknown_without_a_price_basis() {
    let stub = StubServer::start(vec![(CHAT_PATH, vec![final_with_usage("answer", 11, 2)])]).await;
    let boot = boot("c08", &plane_api_key(&stub.endpoint(), "sk-t07-c08")).await;
    let run_id = execute_on(&boot.state, "sess_local_alpha", "task").await;

    let rows = usage_rows(
        &boot.state,
        ModelUsageQuery {
            owner_user_id: None,
            session_id: None,
            run_id: Some(run_id.clone()),
        },
    )
    .await;
    let row = &rows[0];
    // Token facts recorded; the cost basis stays explicitly absent.
    assert_eq!(row.usage.as_ref().unwrap().input_tokens, Some(11));
    assert_eq!(
        row.cost_basis, None,
        "no price source exists — cost is unknown, never a market guess"
    );
    // And no price columns were invented anywhere in the schema: the
    // ledger's only cost fact is the basis reference.
    let unexpected = boot
        .state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM pragma_table_info('model_call_usage') WHERE name LIKE \
             '%price%' OR name LIKE '%cost_micros%' OR name LIKE '%amount%'",
            vec![],
        )
        .await
        .expect("schema probe");
    assert_eq!(
        unexpected.as_deref(),
        Some("0"),
        "no cost/price columns exist"
    );

    stub.stop().await;
    teardown(boot).await;
}

// ── C09: owner scoping + the secret never enters the database ────────────────

#[tokio::test]
async fn c09_owner_scoped_queries_isolate_and_secrets_never_enter_the_db() {
    const SECRET: &str = "sk-t07-c09-secret-material";
    let stub = StubServer::start(vec![(
        CHAT_PATH,
        vec![
            final_with_usage("owner answer", 5, 1),
            final_with_usage("other answer", 6, 2),
        ],
    )])
    .await;
    let boot = boot("c09", &plane_with_aux(&stub.endpoint(), SECRET)).await;

    let owner_run = execute_on(&boot.state, "sess_local_alpha", "owner task").await;
    assert_eq!(run_status(&boot.state, &owner_run).await, "completed");

    // A SECOND user's session: inserted directly (the ledger's owner
    // scoping joins the session owner), then a run driven under it.
    let db_path = boot.runtime_dir.join("data").join("runs.db");
    {
        let conn = rusqlite::Connection::open(&db_path).expect("open db");
        conn.execute(
            "INSERT OR IGNORE INTO sessions (session_id, agent_id, owner_user_id, title, \
             created_at_unix_ms) VALUES ('sess_other_user', 'lingxi', 'user_other', \
             'Other owner', 1)",
            [],
        )
        .expect("seed other-owner session");
    }
    let other_run_id = format!("run-other-{}", std::process::id());
    boot.state
        .runs()
        .drive_run(
            boot.state.storage().as_ref(),
            boot.state.events(),
            &KernelPrincipal::LocalUser,
            "sess_other_user",
            "lingxi",
            &other_run_id,
            "other task",
            1,
            1_790_409_600_000,
            None,
            None,
            DriveAuthorization::user_submission(None, SessionPermissionMode::Operate),
            None,
            "sess_other_user",
        )
        .await
        .expect("other drive settles");
    assert_eq!(run_status(&boot.state, &other_run_id).await, "completed");

    // Owner scoping: the LOCAL OWNER's scoped query sees ONLY its own
    // session's rows — never the other user's.
    let owner_rows = usage_rows(
        &boot.state,
        ModelUsageQuery {
            owner_user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
            session_id: None,
            run_id: None,
        },
    )
    .await;
    assert!(
        owner_rows
            .iter()
            .all(|row| row.run_id.as_deref() != Some(other_run_id.as_str())),
        "the other user's rows never cross the owner scope"
    );
    let other_rows = usage_rows(
        &boot.state,
        ModelUsageQuery {
            owner_user_id: Some("user_other".to_string()),
            session_id: None,
            run_id: None,
        },
    )
    .await;
    assert_eq!(other_rows.len(), 1);
    assert_eq!(
        other_rows[0].run_id.as_deref(),
        Some(other_run_id.as_str()),
        "the other owner sees exactly its own row"
    );

    // The secret scan: the API key's material must appear NOWHERE in the
    // runs database file (credentials live only in the credential store —
    // a different file, out of this assertion's scope by design).
    let raw = std::fs::read(&db_path).expect("read runs.db");
    assert!(
        !find_subslice(&raw, SECRET.as_bytes()).is_some(),
        "the provider secret never entered the usage/run database"
    );

    stub.stop().await;
    teardown(boot).await;
}

// ── C10: a ledger write failure publishes no completion ──────────────────────

/// A StoragePort decorator that fails `record_model_call_usage` while the
/// flag is armed (everything else forwards to the REAL RunDatabase).
struct FailingLedgerPort {
    inner: Arc<lingxi_adapters::storage::RunDatabase>,
    fail_ledger: Arc<AtomicBool>,
}

impl lingxi_kernel::ports::StoragePort for FailingLedgerPort {
    async fn record_run_started(
        &self,
        ctx: &lingxi_kernel::RunContext,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, lingxi_kernel::ports::StorageError> {
        self.inner.record_run_started(ctx, now_unix_ms).await
    }
    async fn commit_run_outcome(
        &self,
        ctx: &lingxi_kernel::RunContext,
        outcome: lingxi_kernel::ports::RunOutcome,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, lingxi_kernel::ports::StorageError> {
        self.inner
            .commit_run_outcome(ctx, outcome, now_unix_ms)
            .await
    }
    async fn record_run_events(
        &self,
        ctx: &lingxi_kernel::RunContext,
        events: Vec<lingxi_kernel::ports::KeyEvent>,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, lingxi_kernel::ports::StorageError> {
        self.inner.record_run_events(ctx, events, now_unix_ms).await
    }
    async fn record_stale_result(
        &self,
        ctx: &lingxi_kernel::RunContext,
        refused: lingxi_kernel::ports::StaleResultFact,
        now_unix_ms: u64,
    ) -> Result<(), lingxi_kernel::ports::StorageError> {
        self.inner
            .record_stale_result(ctx, refused, now_unix_ms)
            .await
    }
    async fn record_attempt_started(
        &self,
        ctx: &lingxi_kernel::RunContext,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, lingxi_kernel::ports::StorageError> {
        self.inner.record_attempt_started(ctx, now_unix_ms).await
    }
    async fn record_run_state_change(
        &self,
        ctx: &lingxi_kernel::RunContext,
        from: lingxi_protocol::RunStatus,
        to: lingxi_protocol::RunStatus,
        reason: Option<String>,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, lingxi_kernel::ports::StorageError> {
        self.inner
            .record_run_state_change(ctx, from, to, reason, now_unix_ms)
            .await
    }
    async fn record_invocation_intent(
        &self,
        ctx: &lingxi_kernel::RunContext,
        intent: lingxi_kernel::ports::InvocationIntent,
        now_unix_ms: u64,
    ) -> Result<(), lingxi_kernel::ports::StorageError> {
        self.inner
            .record_invocation_intent(ctx, intent, now_unix_ms)
            .await
    }
    async fn advance_invocation(
        &self,
        ctx: &lingxi_kernel::RunContext,
        journal_id: &lingxi_protocol::ToolCallId,
        to: lingxi_kernel::ports::InvocationPhase,
        now_unix_ms: u64,
    ) -> Result<(), lingxi_kernel::ports::StorageError> {
        self.inner
            .advance_invocation(ctx, journal_id, to, now_unix_ms)
            .await
    }
    async fn record_invocation_receipt(
        &self,
        ctx: &lingxi_kernel::RunContext,
        journal_id: &lingxi_protocol::ToolCallId,
        receipt: lingxi_kernel::ports::InvocationReceipt,
        now_unix_ms: u64,
    ) -> Result<(), lingxi_kernel::ports::StorageError> {
        self.inner
            .record_invocation_receipt(ctx, journal_id, receipt, now_unix_ms)
            .await
    }
    async fn record_invocation_unknown(
        &self,
        journal_id: &lingxi_protocol::ToolCallId,
        detail: String,
        now_unix_ms: u64,
    ) -> Result<(), lingxi_kernel::ports::StorageError> {
        self.inner
            .record_invocation_unknown(journal_id, detail, now_unix_ms)
            .await
    }
    async fn load_invocation_journal(
        &self,
        run_id: &lingxi_protocol::RunId,
    ) -> Result<Vec<lingxi_kernel::ports::InvocationJournalEntry>, lingxi_kernel::ports::StorageError>
    {
        self.inner.load_invocation_journal(run_id).await
    }
    async fn record_run_lineage(
        &self,
        ctx: &lingxi_kernel::RunContext,
        lineage: lingxi_kernel::subagent::RunLineage,
        now_unix_ms: u64,
    ) -> Result<(), lingxi_kernel::ports::StorageError> {
        self.inner
            .record_run_lineage(ctx, lineage, now_unix_ms)
            .await
    }
    async fn load_run_lineage(
        &self,
        run_id: &lingxi_protocol::RunId,
    ) -> Result<Option<lingxi_kernel::subagent::RunLineage>, lingxi_kernel::ports::StorageError>
    {
        self.inner.load_run_lineage(run_id).await
    }
    async fn record_model_call_usage(
        &self,
        record: ModelCallUsageRecord,
        _now_unix_ms: u64,
    ) -> Result<(), lingxi_kernel::ports::StorageError> {
        if self.fail_ledger.load(Ordering::SeqCst) {
            return Err(lingxi_kernel::ports::StorageError::Io {
                detail: format!(
                    "injected ledger failure for {} (C10: no success event may publish)",
                    record.model_call_id
                ),
            });
        }
        self.inner
            .record_model_call_usage(record, _now_unix_ms)
            .await
    }
    async fn query_model_call_usage(
        &self,
        query: ModelUsageQuery,
    ) -> Result<Vec<ModelCallUsageRecord>, lingxi_kernel::ports::StorageError> {
        self.inner.query_model_call_usage(query).await
    }
    async fn load_run(
        &self,
        run_id: &lingxi_protocol::RunId,
    ) -> Result<Option<lingxi_kernel::ports::RunRecord>, lingxi_kernel::ports::StorageError> {
        self.inner.load_run(run_id).await
    }
}

#[tokio::test]
async fn c10_a_ledger_write_failure_publishes_no_completion_and_the_retry_is_idempotent() {
    let stub = StubServer::start(vec![(
        CHAT_PATH,
        vec![
            final_with_usage("answer", 8, 2),
            final_with_usage("retry", 9, 3),
        ],
    )])
    .await;
    let boot = boot("c10", &plane_api_key(&stub.endpoint(), "sk-t07-c10")).await;
    let state = &boot.state;

    let subscription = match state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("subscribe")
    {
        SubscribeOutcome::Started { subscription, .. } => subscription,
        SubscribeOutcome::RequiresSnapshot(_) => panic!("fresh stream needs no snapshot"),
    };

    let fail_ledger = Arc::new(AtomicBool::new(true));
    let gated = Arc::new(FailingLedgerPort {
        inner: Arc::clone(state.storage()),
        fail_ledger: Arc::clone(&fail_ledger),
    });

    // The submission runs against the DECORATED port: the first ledger
    // write (the model call's usage row, which precedes the completed
    // event by contract) fails.
    let submission = ExecuteSubmission {
        input: "failing task",
        request_id: Some("req-t07-c10-a"),
    };
    let failed = state
        .sessions()
        .execute_submission_for(
            gated.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            &submission,
            1_790_409_600_000,
        )
        .await;
    assert!(
        failed.is_err(),
        "the storage failure surfaces loudly, never as a fake success: {failed:?}"
    );

    // THE core C10 assertion: NO model_call_completed event was published
    // (drain the mailbox; the completed event of the ONLY model call must
    // be absent — the ledger commit precedes publication by contract).
    let mut saw_completed = false;
    let deadline = std::time::Instant::now() + Duration::from_millis(300);
    while std::time::Instant::now() < deadline {
        while let Some(frame) = subscription.mailbox().try_recv() {
            if let lingxi_service::SubscriptionFrame::Event(envelope) = frame {
                if let lingxi_protocol::EventPayload::Known(
                    lingxi_protocol::KnownEventPayload::ModelCallCompleted(_),
                ) = envelope.payload
                {
                    saw_completed = true;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        !saw_completed,
        "no completion event published when the usage ledger commit failed"
    );
    // And the ledger itself holds no row (the failure was real).
    let rows = usage_rows(
        state,
        ModelUsageQuery {
            owner_user_id: None,
            session_id: Some("sess_local_alpha".to_string()),
            run_id: None,
        },
    )
    .await;
    assert!(rows.is_empty(), "the failed write left no row");

    // Disarm the failure and re-run: the row lands EXACTLY once, and a
    // replay of the SAME row is an idempotent no-op (C10's retry rule).
    fail_ledger.store(false, Ordering::SeqCst);
    let run_id = execute_on(state, "sess_local_alpha", "retry task").await;
    assert_eq!(run_status(state, &run_id).await, "completed");
    let rows = usage_rows(
        state,
        ModelUsageQuery {
            owner_user_id: None,
            session_id: None,
            run_id: Some(run_id.clone()),
        },
    )
    .await;
    assert_eq!(rows.len(), 1, "exactly one accounting row");
    let replay = state
        .storage()
        .record_model_call_usage(rows[0].clone(), 1_790_409_700_000)
        .await;
    assert!(replay.is_ok(), "an identical replay is an idempotent no-op");
    let rows_after = usage_rows(
        state,
        ModelUsageQuery {
            owner_user_id: None,
            session_id: None,
            run_id: Some(run_id.clone()),
        },
    )
    .await;
    assert_eq!(rows_after.len(), 1, "the replay added nothing");

    stub.stop().await;
    teardown(boot).await;
}
