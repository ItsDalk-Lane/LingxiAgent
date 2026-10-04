//! REVIEWER-R05-T02 full-chain C09 probe: a synthetic secret of a form the
//! implementer's fixtures never used (`x9Rvw/Q7+kFullChain=Marker0123456789+/==`
//! — standard-base64 alphabet with literal `+`, `/` and `=` padding) driven
//! through the REAL production wiring (config file → plane → bootstrap →
//! CredentialService → GatewayedProvider → real loopback HTTP stub), with the
//! provider ECHOING the secret in its 401 body. Then: scan every durable byte
//! under the service home and the surfaced error text for the marker.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const MARKER: &str = "x9Rvw/Q7+kFullChain=Marker0123456789+/==";

struct RecordedRequest {
    headers: Vec<(String, String)>,
    body: String,
}

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start_echo_401() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn({
            let hits = Arc::clone(&hits);
            let requests = Arc::clone(&requests);
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
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            let recorded = read_request(&mut socket).await;
                            requests.lock().expect("requests").push(recorded);
                            // The provider ECHOES the in-play secret (C09's
                            // adversarial case), 401 both times (the retry
                            // after a terminal static-401 never happens for
                            // apiKey; one script answer is enough).
                            let body = serde_json::json!({
                                "error": {"message": format!("key {MARKER} rejected at the edge")}
                            })
                            .to_string();
                            let response = format!(
                                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
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
        assert!(read > 0, "stub: closed before headers");
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("utf8 head");
    let mut lines = head.split("\r\n");
    let _request_line = lines.next().expect("request line");
    let mut headers = Vec::new();
    let mut content_length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().expect("content-length");
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
        assert!(read > 0, "stub: closed mid-body");
        raw.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8(raw[body_start..body_start + content_length].to_vec())
        .expect("body utf8");
    RecordedRequest { headers, body }
}

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t02-review-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
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

#[tokio::test]
async fn probe_c09_full_chain_my_marker_never_persists_or_echoes() {
    let stub = StubServer::start_echo_401().await;
    let home = unique_dir("home");
    let workspace = unique_dir("ws");
    let config_root = unique_dir("cfg");
    let config_path = config_root.join("service.json");
    let config_json = format!(
        r#"{{"home": {home}, "workspace": {ws},
            "providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": {endpoint},
                    "auth": {{"kind": "apiKey", "apiKey": {key}}}
                }}
            }},
            "models": {{"chat": {{"provider": "main", "model": "stub-model"}}}}
        }}"#,
        home = serde_json::to_string(&home.to_string_lossy()).expect("json"),
        ws = serde_json::to_string(&workspace.to_string_lossy()).expect("json"),
        endpoint = serde_json::to_string(&format!("http://{}/v1", stub.addr)).expect("json"),
        key = serde_json::to_string(MARKER).expect("json"),
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
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");

    let run_id = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            "say hi",
            1_790_409_600_000,
        )
        .await
        .expect("execute accepted")
        .run_id;
    let status = state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query")
        .expect("run row");
    assert_eq!(status, "failed", "a rejected static key fails the run");

    // The secret REALLY flowed onto the wire (otherwise the scan is vacuous),
    // and the stub received exactly ONE call (a static-key 401 is terminal —
    // no retry, no refresh endpoint).
    {
        let requests = stub.requests.lock().expect("requests");
        assert_eq!(requests.len(), 1, "one provider call: {}", requests.len());
        let auth = requests[0]
            .headers
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            auth,
            Some(format!("Bearer {MARKER}").as_str()),
            "the marker really reached the wire"
        );
    }

    // NOTHING under the service home may carry the marker: events DB, run
    // rows, journal files, logs — the full durable tree.
    let leaks = files_containing(&home, MARKER);
    assert!(leaks.is_empty(), "marker leaked into durable state: {leaks:?}");
    // The run's terminal reason carries no marker either (DB projection).
    let reason = state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("query")
        .unwrap_or_default();
    assert!(!reason.contains(MARKER), "terminal reason leaked: {reason}");
    // The key_events table (raw payload_json) is marker-free.
    let events_dump = state
        .storage()
        .query_one_text(
            "SELECT group_concat(COALESCE(payload_json,''), '') FROM key_events",
            vec![],
        )
        .await
        .expect("key_events dump query");
    if let Some(dump) = events_dump {
        assert!(!dump.contains(MARKER), "event payloads leaked the marker");
    }

    state.storage().close().await.expect("close storage");
    stub.stop().await;
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&config_root);
    println!("PROBE c09 full-chain marker scan: PASS (0 durable leaks, wire carried the marker)");
}
