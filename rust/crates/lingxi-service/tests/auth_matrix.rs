//! R02-T03 acceptance matrix (integration layer): real axum+hyper service
//! loop over real loopback TCP, real RFC 6455 WebSocket upgrade, synthetic
//! isolated home under the system temp dir — never a real user directory,
//! never real credentials.
//!
//! Covers the two REQUIRED scenarios at the contract/service layer:
//!
//! - **R02-A05 伪造身份无效** — no credential, forged principal fields,
//!   forged sessionId, expired/revoked device credentials and cross-
//!   principal sessionId misuse are all rejected (401/403/404) with ZERO
//!   observable server-state change (run counts snapshotted around every
//!   denied request; registry files byte-compared).
//! - **R02-A06 恶意网页无法借 loopback 越权** — foreign Origin on HTTP and
//!   WS, Host-header tampering, expired WS tickets and ticket replay are
//!   each rejected per policy; the legitimate desktop shape (allowed
//!   Origin) and the CLI/curl shape (NO Origin, bearer token) keep working.
//!
//! The binary-level mirror of this matrix (real `lingxi-service` processes)
//! lives in `scripts/rust-tauri/r02_t03_auth_matrix.sh`; both must stay
//! green for the acceptance evidence.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use lingxi_service::ws::WsFrame;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceError,
    ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

fn externally_revoke_credential(home: &std::path::Path, credential_id: &str) -> PathBuf {
    let layout = prepare_layout(home).expect("existing auth layout");
    let path = layout
        .runtime_dir
        .join(lingxi_service::auth::DEVICE_CREDENTIALS_FILE);
    let mut registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read credentials registry"))
            .expect("valid credentials registry");
    let credential = registry["credentials"]
        .as_array_mut()
        .expect("credentials array")
        .iter_mut()
        .find(|record| record["credentialId"] == credential_id)
        .expect("issued credential exists");
    credential["status"] = serde_json::Value::String("revoked".to_string());
    std::fs::write(&path, serde_json::to_vec_pretty(&registry).unwrap())
        .expect("external registry update");
    path
}

// ── harness ────────────────────────────────────────────────────────────────

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    storage: std::sync::Arc<lingxi_adapters::storage::RunDatabase>,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

fn synthetic_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-service-r02t03-{tag}-{}",
        std::process::id()
    ))
}

/// Defaults are test-friendly: 80ms ticket TTL (expiry observable without
/// real waiting), generous rate budget, 4 WS connections.
async fn start_server(tag: &str, ticket_ttl_ms: u64, rate_max: u32, ws_max: usize) -> TestServer {
    let home = synthetic_home(tag);
    let _ = std::fs::remove_dir_all(&home);
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr parses"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_limits(
        config,
        &layout,
        ticket_ttl_ms,
        60_000,
        rate_max,
        ws_max,
    )
    .await
    .expect("bootstrap");
    let token = state.auth().local_token();
    let storage = std::sync::Arc::clone(state.storage());

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None, // no drain budget: the test drives the stop signal itself
        )
        .await
    });
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        addr,
        home,
        token,
        storage,
        stop: stop_tx,
        handle,
    }
}

async fn default_server(tag: &str) -> TestServer {
    start_server(tag, 80, 10_000, 4).await
}

impl TestServer {
    async fn stop_and_assert_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn stop_and_keep_home(self) -> PathBuf {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        self.home
    }

    /// Observable server-state snapshot: the run counts of both seeded
    /// sessions as the owner sees them.
    async fn run_counts(&self) -> (u64, u64) {
        let (status, body) = http(
            self.addr,
            "GET",
            "/lingxi/v1/sessions/sess_local_alpha",
            &[("Authorization", &self.bearer())],
            None,
        )
        .await;
        assert_eq!(status, 200, "owner snapshot read failed: {body}");
        let alpha = serde_json::from_str::<serde_json::Value>(&body).unwrap()["runCount"]
            .as_u64()
            .unwrap_or(u64::MAX);
        let (status, body) = http(
            self.addr,
            "GET",
            "/lingxi/v1/sessions/sess_local_beta",
            &[("Authorization", &self.bearer())],
            None,
        )
        .await;
        assert_eq!(status, 200, "owner snapshot read failed: {body}");
        let beta = serde_json::from_str::<serde_json::Value>(&body).unwrap()["runCount"]
            .as_u64()
            .unwrap_or(u64::MAX);
        (alpha, beta)
    }
}

/// Plain HTTP/1.1 request over a one-shot TCP connection. `Connection:
/// close` keeps the response bounded by EOF; bodies carry Content-Length
/// (required for the server to parse them). Returns (status, body).
async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await.expect("write body");
    }
    let mut raw = Vec::new();
    // Tolerate a hard close (RST) after the response: use what arrived.
    loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                break;
            }
            Err(err) => panic!("read response: {err}"),
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status in {head:?}"));
    (status, body.to_string())
}

/// POST with a tolerant body write: when the server rejects the request
/// early (e.g. 413 on an oversized body) it may close the socket before
/// the whole body is written; the response is still what matters.
async fn http_post_tolerant(
    addr: SocketAddr,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut head = format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    ));
    stream.write_all(head.as_bytes()).await.expect("write head");
    let _ = stream.write_all(body.as_bytes()).await;
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    (status, body.to_string())
}

/// Raw HTTP request with full header control (Host/Origin tampering).
/// Returns the WHOLE response text (status line included).
async fn http_raw(addr: SocketAddr, raw: &str) -> String {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    stream.write_all(raw.as_bytes()).await.expect("write raw");
    let mut out = Vec::new();
    stream.read_to_end(&mut out).await.expect("read raw");
    String::from_utf8_lossy(&out).into_owned()
}

// ── minimal WS client (masked frames per RFC 6455) ─────────────────────────

fn client_ws_key() -> String {
    use base64::Engine as _;
    let mut bytes = [0u8; 16];
    // Deterministic-enough key for tests (any 16 bytes base64'd).
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for (i, slot) in bytes.iter_mut().enumerate() {
        *slot = (((nanos >> (i * 4)) & 0xff) as u8).wrapping_add((i as u8).wrapping_mul(31));
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Sends an upgrade request with full header control. On 101 returns the
/// upgraded stream (ready for frames); otherwise the full response text.
async fn ws_upgrade(
    addr: SocketAddr,
    path: &str,
    headers: &[(&str, &str)],
) -> Result<TcpStream, String> {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let key = client_ws_key();
    let mut head = format!(
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .await
        .expect("write upgrade");

    // Read the status line + headers byte-wise until the blank line so no
    // frame bytes get swallowed.
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read_exact(&mut byte).await {
            Ok(_) => {
                seen.push(byte[0]);
                if seen.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(err) => {
                return Err(format!("connection closed while reading headers: {err}"));
            }
        }
    }
    let text = String::from_utf8_lossy(&seen).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    if status == 101 {
        Ok(stream)
    } else {
        // Read the JSON body too so assertions can check machine reasons.
        let content_length = text
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            stream
                .read_exact(&mut body)
                .await
                .unwrap_or_else(|e| panic!("read rejection body: {e}"));
        }
        Err(format!(
            "{}\r\n\r\n{}",
            text.trim_end_matches("\r\n"),
            String::from_utf8_lossy(&body)
        ))
    }
}

/// Writes one masked client text frame.
async fn client_ws_send(stream: &mut TcpStream, payload: &[u8]) {
    let mask: [u8; 4] = [0x11, 0x22, 0x33, 0x44];
    let mut out = vec![0x81];
    if payload.len() < 126 {
        out.push(0x80 | payload.len() as u8);
    } else {
        out.push(0x80 | 126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    let masked: Vec<u8> = payload
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i % 4])
        .collect();
    out.extend_from_slice(&masked);
    stream.write_all(&out).await.expect("write client frame");
}

async fn client_ws_read(stream: &mut TcpStream) -> WsFrame {
    lingxi_service::ws::read_ws_frame(stream)
        .await
        .expect("read server frame")
        .expect("server frame")
}

fn frame_text(frame: &WsFrame) -> String {
    match frame {
        WsFrame::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        other => panic!("expected text frame, got {other:?}"),
    }
}

fn close_code(frame: &WsFrame) -> Option<u16> {
    match frame {
        WsFrame::Close(code, _) => Some(*code),
        _ => None,
    }
}

/// Full happy-path WS session: upgrade → ClientHello → ServerHello →
/// session_read → result frame.
async fn ws_session_read_roundtrip(
    addr: SocketAddr,
    headers: &[(&str, &str)],
    session_id: &str,
) -> (String, Option<WsFrame>) {
    let mut stream = ws_upgrade(addr, "/lingxi/v1/ws", headers)
        .await
        .expect("upgrade succeeds");
    let hello = serde_json::json!({
        "protocol": "lingxi.wire",
        "clientKind": "test",
        "clientVersion": "0",
        "protocolMin": 1,
        "protocolMax": 1,
    });
    client_ws_send(&mut stream, hello.to_string().as_bytes()).await;
    let server_hello = frame_text(&client_ws_read(&mut stream).await);
    assert!(
        server_hello.contains("lingxi.wire"),
        "server hello: {server_hello}"
    );
    let request = serde_json::json!({"type": "session_read", "sessionId": session_id});
    client_ws_send(&mut stream, request.to_string().as_bytes()).await;
    let result = client_ws_read(&mut stream).await;
    let text = match &result {
        WsFrame::Text(_) => frame_text(&result),
        _ => String::new(),
    };
    let follow_up = if text.is_empty() {
        Some(client_ws_read(&mut stream).await)
    } else {
        None
    };
    (text, follow_up)
}

// ── R02-A05: forged identity is ineffective ────────────────────────────────

#[tokio::test]
async fn a05_no_credential_is_rejected_without_side_effects() {
    let server = default_server("a05-none").await;
    let before = server.run_counts().await;

    // Read endpoint.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha",
        &[],
        None,
    )
    .await;
    assert_eq!(status, 401, "unauthenticated read must be 401: {body}");
    assert!(body.contains("missing_credential"), "body: {body}");

    // Execute endpoint.
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &[],
        Some(r#"{"input":"forged"}"#),
    )
    .await;
    assert_eq!(status, 401, "unauthenticated execute must be 401: {body}");

    // Ticket issuance endpoint. R14-F01: the R00 leaf scenario
    // R00-T02-LA-5816DA563ED8 pins the original assertion "无主体返回 403"
    // (incumbent `server/routes/ws-auth.ts`), so this route denies an
    // unauthenticated caller with 403, unlike the 401 read/execute paths.
    let (status, body) = http(server.addr, "POST", "/lingxi/v1/ws-ticket", &[], None).await;
    assert_eq!(status, 403, "unauthenticated ws-ticket must be 403: {body}");

    // WS upgrade without any credential.
    let denied = ws_upgrade(server.addr, "/lingxi/v1/ws", &[])
        .await
        .expect_err("must be denied");
    assert!(denied.starts_with("HTTP/1.1 401"), "ws upgrade: {denied}");
    assert!(
        denied.contains("missing_credential"),
        "ws upgrade: {denied}"
    );

    let after = server.run_counts().await;
    assert_eq!(before, after, "no server-state change from denied requests");
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_forged_principal_fields_are_ignored_not_trusted() {
    let server = default_server("a05-forgedp").await;

    // A request that tries to ASSERT an identity via headers/body without
    // any credential stays unauthenticated (401).
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha",
        &[
            ("X-Lingxi-Principal", "principal_device_forged"),
            ("X-Lingxi-User", "user_victim"),
        ],
        None,
    )
    .await;
    assert_eq!(status, 401, "identity-shaped headers must not authenticate");

    // With a VALID token, the server-computed principal wins: /me reflects
    // the loopback-token owner, never the forged header values.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[
            ("Authorization", &server.bearer()),
            ("X-Lingxi-Principal", "principal_device_forged"),
            ("X-Lingxi-User", "user_victim"),
        ],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let me: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(me["kind"], "local_user", "me body: {body}");
    assert_eq!(me["credentialKind"], "loopback_token");
    assert_eq!(
        me["principalId"],
        "principal_local_user_user_local_no_studio_no_node"
    );
    assert_ne!(me["principalId"], "principal_device_forged");

    // Identity fields smuggled into the execute BODY are a parse error
    // (deny_unknown_fields), and the request carries a valid token — so the
    // failure is 400 invalid_message, NOT a forged execution.
    let before = server.run_counts().await;
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &[("Authorization", &server.bearer())],
        Some(r#"{"input":"x","principalId":"forged","userId":"user_victim"}"#),
    )
    .await;
    assert_eq!(status, 400, "identity-shaped body must be rejected: {body}");
    assert!(body.contains("invalid_message"), "body: {body}");
    let after = server.run_counts().await;
    assert_eq!(before, after, "rejected body must not execute anything");

    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_forged_and_unknown_session_ids() {
    let server = default_server("a05-sid").await;

    // Authenticated request for a NONEXISTENT session: not found.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_does_not_exist",
        &[("Authorization", &server.bearer())],
        None,
    )
    .await;
    assert_eq!(status, 404, "unknown session must be 404: {body}");
    assert!(body.contains("not_found"), "body: {body}");

    // Execute against a nonexistent session: 404, no side effects.
    let before = server.run_counts().await;
    let (status, _) = http(
        server.addr,
        "POST",
        "/lingxi/v1/sessions/sess_does_not_exist/execute",
        &[("Authorization", &server.bearer())],
        Some(r#"{"input":"x"}"#),
    )
    .await;
    assert_eq!(status, 404);
    assert_eq!(before, server.run_counts().await);

    // A garbage/garbled bearer is not a principal.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha",
        &[("Authorization", "Bearer 00000000000000000000000000000000")],
        None,
    )
    .await;
    assert_eq!(status, 401, "wrong token must be 401: {body}");
    assert!(body.contains("invalid_credential"), "body: {body}");

    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_expired_and_revoked_device_credentials_are_rejected_without_side_effects() {
    let server = default_server("a05-devcred").await;
    // Issue synthetic credentials through the owner-only management route.
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/devices/credentials",
        &[("Authorization", &server.bearer())],
        Some(r#"{"userId":"user_remote","scopes":["chat"],"expiresAtUnixMs":1}"#),
    )
    .await;
    assert_eq!(status, 201, "issue expired-track credential: {body}");
    let expired_secret = serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/devices/credentials",
        &[("Authorization", &server.bearer())],
        Some(r#"{"userId":"user_remote","scopes":["chat"]}"#),
    )
    .await;
    assert_eq!(status, 201, "issue revocable credential: {body}");
    let revocable_secret = serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let revocable_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["credentialId"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {revocable_secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 200, "active device credential must work: {body}");

    let before = server.run_counts().await;

    // EXPIRED credential (expiresAtUnixMs=1): denied.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha",
        &[("Authorization", &format!("Bearer {expired_secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 401, "expired credential must be 401: {body}");
    assert!(body.contains("invalid_credential"), "body: {body}");
    let (status, _) = http(
        server.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &[("Authorization", &format!("Bearer {expired_secret}"))],
        Some(r#"{"input":"x"}"#),
    )
    .await;
    assert_eq!(status, 401);

    // 模拟另一个进程在服务运行期间撤销凭证；下一次 HTTP 请求必须读到磁盘变更。
    let registry_path = externally_revoke_credential(&server.home, &revocable_id);
    let revoked_bytes = std::fs::read(&registry_path).unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {revocable_secret}"))],
        None,
    )
    .await;
    assert_eq!(
        status, 401,
        "externally revoked credential must fail: {body}"
    );
    assert!(body.contains("invalid_credential"), "body: {body}");
    assert_eq!(std::fs::read(&registry_path).unwrap(), revoked_bytes);

    let tampered = format!(
        "{}{}",
        &revocable_secret[..18],
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    );
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha",
        &[("Authorization", &format!("Bearer {tampered}"))],
        None,
    )
    .await;
    assert_eq!(status, 401, "tampered secret must be 401: {body}");
    assert!(body.contains("invalid_credential"), "body: {body}");

    let after = server.run_counts().await;
    assert_eq!(
        before, after,
        "denied device credentials must not mutate state"
    );
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_unreadable_registry_fails_closed_and_recovers() {
    let server = default_server("a05-registry-reload").await;
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/devices/credentials",
        &[("Authorization", &server.bearer())],
        Some(r#"{"userId":"user_remote","scopes":["chat"]}"#),
    )
    .await;
    assert_eq!(status, 201, "issue device credential: {body}");
    let secret = serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let bearer = format!("Bearer {secret}");
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 200);

    let layout = prepare_layout(&server.home).unwrap();
    let path = layout
        .runtime_dir
        .join(lingxi_service::auth::DEVICE_CREDENTIALS_FILE);
    let valid = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"{invalid json").unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 401, "invalid registry must deny: {body}");
    assert!(body.contains("auth_registry_unavailable"), "body: {body}");
    assert_eq!(std::fs::read(&path).unwrap(), b"{invalid json");

    std::fs::remove_file(&path).unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 401, "missing registry must deny: {body}");
    assert!(body.contains("auth_registry_unavailable"), "body: {body}");
    assert!(
        !path.exists(),
        "denied request must not recreate the registry"
    );

    std::fs::write(&path, valid).unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 200, "repaired registry must restore access: {body}");

    let devices_path = layout.runtime_dir.join(lingxi_service::auth::DEVICES_FILE);
    let valid_devices = std::fs::read(&devices_path).unwrap();
    std::fs::remove_file(&devices_path).unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 401, "missing device registry must deny: {body}");
    assert!(body.contains("auth_registry_unavailable"), "body: {body}");
    std::fs::write(&devices_path, valid_devices).unwrap();
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_live_device_status_and_expiry_changes_take_effect() {
    let server = default_server("a05-live-device-state").await;
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/devices/credentials",
        &[("Authorization", &server.bearer())],
        Some(r#"{"userId":"user_remote","scopes":["chat"]}"#),
    )
    .await;
    assert_eq!(status, 201, "issue device credential: {body}");
    let issued: serde_json::Value = serde_json::from_str(&body).unwrap();
    let bearer = format!("Bearer {}", issued["secret"].as_str().unwrap());
    let credential_id = issued["credentialId"].as_str().unwrap();
    let device_id = issued["deviceId"].as_str().unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 200, "active device credential must work: {body}");
    let layout = prepare_layout(&server.home).unwrap();
    let credentials_path = layout
        .runtime_dir
        .join(lingxi_service::auth::DEVICE_CREDENTIALS_FILE);
    let mut credentials: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&credentials_path).unwrap()).unwrap();
    let record = credentials["credentials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|record| record["credentialId"] == credential_id)
        .unwrap();
    record["expiresAtUnixMs"] = serde_json::json!(1);
    std::fs::write(
        &credentials_path,
        serde_json::to_vec_pretty(&credentials).unwrap(),
    )
    .unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 401, "external expiry must deny: {body}");

    credentials["credentials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|record| record["credentialId"] == credential_id)
        .unwrap()["expiresAtUnixMs"] = serde_json::Value::Null;
    std::fs::write(
        &credentials_path,
        serde_json::to_vec_pretty(&credentials).unwrap(),
    )
    .unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 200, "restored expiry must restore access: {body}");

    let devices_path = layout.runtime_dir.join(lingxi_service::auth::DEVICES_FILE);
    let mut devices: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&devices_path).unwrap()).unwrap();
    let device = devices["devices"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|record| record["deviceId"] == device_id)
        .unwrap();
    device["status"] = serde_json::Value::String("revoked".to_string());
    std::fs::write(&devices_path, serde_json::to_vec_pretty(&devices).unwrap()).unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 401, "external device revoke must deny: {body}");
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_ws_ticket_and_live_socket_observe_external_revocation() {
    let server = default_server("a05-ws-revocation").await;
    let issue = |server: &TestServer| {
        let addr = server.addr;
        let bearer = server.bearer();
        async move {
            let (status, body) = http(
                addr,
                "POST",
                "/lingxi/v1/devices/credentials",
                &[("Authorization", &bearer)],
                Some(r#"{"userId":"user_remote","scopes":["chat"]}"#),
            )
            .await;
            assert_eq!(status, 201, "issue device credential: {body}");
            let value: serde_json::Value = serde_json::from_str(&body).unwrap();
            (
                value["credentialId"].as_str().unwrap().to_string(),
                value["secret"].as_str().unwrap().to_string(),
            )
        }
    };

    let (ticket_id, ticket_secret) = issue(&server).await;
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/ws-ticket",
        &[("Authorization", &format!("Bearer {ticket_secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 200, "issue device ticket: {body}");
    let ticket = serde_json::from_str::<serde_json::Value>(&body).unwrap()["ticket"]
        .as_str()
        .unwrap()
        .to_string();
    externally_revoke_credential(&server.home, &ticket_id);
    let denied = ws_upgrade(
        server.addr,
        &format!("/lingxi/v1/ws?wsTicket={ticket}"),
        &[],
    )
    .await
    .expect_err("revoked ticket principal must not upgrade");
    assert!(denied.starts_with("HTTP/1.1 401"), "ticket: {denied}");

    let (live_id, live_secret) = issue(&server).await;
    let mut ws = ws_upgrade(
        server.addr,
        "/lingxi/v1/ws",
        &[("Authorization", &format!("Bearer {live_secret}"))],
    )
    .await
    .expect("active device socket upgrades");
    let hello = serde_json::json!({
        "protocol": "lingxi.wire", "clientKind": "test", "clientVersion": "0",
        "protocolMin": 1, "protocolMax": 1,
    });
    client_ws_send(&mut ws, hello.to_string().as_bytes()).await;
    let _ = frame_text(&client_ws_read(&mut ws).await);
    externally_revoke_credential(&server.home, &live_id);
    client_ws_send(
        &mut ws,
        br#"{"type":"session_read","sessionId":"sess_local_alpha"}"#,
    )
    .await;
    let error = frame_text(&client_ws_read(&mut ws).await);
    assert!(error.contains("invalid_credential"), "error: {error}");
    assert_eq!(close_code(&client_ws_read(&mut ws).await), Some(4401));
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_cross_principal_session_misuse_is_forbidden_without_side_effects() {
    let server = default_server("a05-cross").await;
    // A device credential for ANOTHER user (the "他人 token").
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/devices/credentials",
        &[("Authorization", &server.bearer())],
        Some(r#"{"userId":"user_remote_b","scopes":["chat"]}"#),
    )
    .await;
    assert_eq!(status, 201, "issue foreign-user credential: {body}");
    let secret = serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
        .as_str()
        .unwrap()
        .to_string();

    // The device principal authenticates fine (it IS a valid principal)…
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["kind"],
        "device"
    );

    // …but the session belongs to user_local: read forbidden, execute
    // forbidden, no side effects.
    let before = server.run_counts().await;
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha",
        &[("Authorization", &format!("Bearer {secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 403, "cross-principal read must be 403: {body}");
    assert!(body.contains("cross_principal_access"), "body: {body}");

    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &[("Authorization", &format!("Bearer {secret}"))],
        Some(r#"{"input":"hijack"}"#),
    )
    .await;
    assert_eq!(status, 403, "cross-principal execute must be 403: {body}");

    // Session list shows nothing owned by the foreign user.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions",
        &[("Authorization", &format!("Bearer {secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["sessions"]
            .as_array()
            .map(Vec::len),
        Some(0),
        "foreign principal must see zero sessions: {body}"
    );

    // Over WS the same ownership rule applies (close 4403).
    let (_text, follow) = ws_session_read_roundtrip(
        server.addr,
        &[("Authorization", &format!("Bearer {secret}"))],
        "sess_local_alpha",
    )
    .await;
    if let Some(frame) = follow {
        assert_eq!(
            close_code(&frame),
            Some(4403),
            "ws cross-principal must close 4403"
        );
    }

    let after = server.run_counts().await;
    assert_eq!(
        before, after,
        "cross-principal denial must not mutate state"
    );
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_loopback_token_over_nonlocal_transport_is_denied_at_policy_layer() {
    // The loopback token presented on a LAN-classified connection is
    // refused BEFORE it ever becomes a principal (unit-covered infer +
    // authenticate; here: query-token shape, which is local-only by
    // policy, attempted with a foreign Origin to force the transport
    // layer's view of a browser).
    let server = default_server("a05-lbtok").await;
    let raw = format!(
        "GET /lingxi/v1/me?token={} HTTP/1.1\r\nHost: {}\r\nOrigin: http://evil.example\r\nConnection: close\r\n\r\n",
        server.token,
        server.addr
    );
    let response = http_raw(server.addr, &raw).await;
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "foreign origin must be rejected before token semantics: {response}"
    );
    server.stop_and_assert_clean().await;
}

// ── R02-A06: malicious web page cannot ride loopback ───────────────────────

#[tokio::test]
async fn a06_origin_matrix_http() {
    let server = default_server("a06-origin").await;

    // Foreign Origin is rejected EVEN on the public health route.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/health",
        &[("Origin", "http://evil.example")],
        None,
    )
    .await;
    assert_eq!(status, 403, "evil origin on health: {body}");
    assert!(body.contains("bad_origin"), "body: {body}");

    // …also with a valid token (transport precedes auth).
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[
            ("Origin", "https://attacker.invalid"),
            ("Authorization", &server.bearer()),
        ],
        None,
    )
    .await;
    assert_eq!(status, 403);

    // Allowed Origin shapes: localhost with any port.
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/health",
        &[("Origin", "http://localhost:5173")],
        None,
    )
    .await;
    assert_eq!(status, 200);
    // 127.0.0.1 origin.
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/health",
        &[(
            "Origin",
            &format!("http://127.0.0.1:{}", server.addr.port()),
        )],
        None,
    )
    .await;
    assert_eq!(status, 200);
    // Electron file:// forms and the sandboxed "null" origin.
    for origin in ["file://", "file:///", "null"] {
        let (status, _) = http(
            server.addr,
            "GET",
            "/lingxi/v1/health",
            &[("Origin", origin)],
            None,
        )
        .await;
        assert_eq!(status, 200, "origin {origin} must be allowed");
    }

    // NO Origin (CLI/curl shape) works and is the documented allowance.
    let (status, _) = http(server.addr, "GET", "/lingxi/v1/health", &[], None).await;
    assert_eq!(status, 200);

    // Subdomain/rebinding-shaped origins are NOT allowed.
    for origin in [
        "http://sub.localhost:1",
        "http://127.0.0.1.evil.example",
        "ws://localhost",
    ] {
        let (status, _) = http(
            server.addr,
            "GET",
            "/lingxi/v1/health",
            &[("Origin", origin)],
            None,
        )
        .await;
        assert_eq!(status, 403, "origin {origin} must be rejected");
    }

    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a06_host_header_tampering_is_rejected() {
    let server = default_server("a06-host").await;

    // DNS-rebinding signature: foreign Host on the loopback listener.
    let raw = "GET /lingxi/v1/health HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n"
        .to_string();
    let response = http_raw(server.addr, &raw).await;
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "foreign Host must be rejected: {response}"
    );
    assert!(
        response.contains("loopback_host_mismatch"),
        "response: {response}"
    );

    // Even with a valid token: transport rejection precedes auth.
    let raw = format!(
        "GET /lingxi/v1/me HTTP/1.1\r\nHost: evil.example\r\nAuthorization: {}\r\nConnection: close\r\n\r\n",
        server.bearer()
    );
    let response = http_raw(server.addr, &raw).await;
    assert!(response.starts_with("HTTP/1.1 403"), "response: {response}");

    // localhost / 127.x / [::1] Hosts are the accepted loopback shapes.
    for host in [
        "localhost",
        &format!("127.0.0.1:{}", server.addr.port()),
        "[::1]",
    ] {
        let raw =
            format!("GET /lingxi/v1/health HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        let response = http_raw(server.addr, &raw).await;
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "Host {host} must be accepted: {response}"
        );
    }

    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a06_ws_origin_and_host_matrix() {
    let server = default_server("a06-ws-o").await;

    // Foreign Origin on the WS upgrade: rejected before the 101.
    let denied = ws_upgrade(
        server.addr,
        "/lingxi/v1/ws",
        &[
            ("Origin", "http://evil.example"),
            ("Authorization", &server.bearer()),
        ],
    )
    .await
    .expect_err("evil origin must be denied");
    assert!(
        denied.starts_with("HTTP/1.1 403"),
        "ws evil origin: {denied}"
    );
    assert!(denied.contains("bad_origin"), "ws evil origin: {denied}");

    // Foreign Host on the WS upgrade: rejected (rebinding guard).
    let mut stream = TcpStream::connect(server.addr).await.unwrap();
    let key = client_ws_key();
    let raw = format!(
        "GET /lingxi/v1/ws HTTP/1.1\r\nHost: rebinder.example\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await.unwrap_or(0);
    let text = String::from_utf8_lossy(&buf[..n]).into_owned();
    assert!(
        text.starts_with("HTTP/1.1 403") || text.is_empty(),
        "ws foreign host must be rejected: {text}"
    );

    // Legitimate Origin + ticket upgrades fine (desktop shape).
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/ws-ticket",
        &[("Authorization", &server.bearer())],
        None,
    )
    .await;
    assert_eq!(status, 200, "ticket issue: {body}");
    let ticket = serde_json::from_str::<serde_json::Value>(&body).unwrap()["ticket"]
        .as_str()
        .unwrap()
        .to_string();
    let allowed_origin = format!("http://localhost:{}", server.addr.port());
    let mut ws = ws_upgrade(
        server.addr,
        &format!("/lingxi/v1/ws?wsTicket={ticket}"),
        &[("Origin", &allowed_origin)],
    )
    .await
    .expect("legit origin + ticket must upgrade");
    // Complete the lingxi.wire handshake to prove the full path works.
    let hello = serde_json::json!({
        "protocol": "lingxi.wire", "clientKind": "desktop", "clientVersion": "1",
        "protocolMin": 1, "protocolMax": 1,
    });
    client_ws_send(&mut ws, hello.to_string().as_bytes()).await;
    let server_hello = frame_text(&client_ws_read(&mut ws).await);
    assert!(
        server_hello.contains("\"selectedProtocol\":1"),
        "server hello: {server_hello}"
    );

    // NO Origin (CLI shape) with a bearer token also upgrades fine.
    let mut ws2 = ws_upgrade(
        server.addr,
        "/lingxi/v1/ws",
        &[("Authorization", &server.bearer())],
    )
    .await
    .expect("no-origin CLI shape must upgrade");
    client_ws_send(&mut ws2, hello.to_string().as_bytes()).await;
    let server_hello2 = frame_text(&client_ws_read(&mut ws2).await);
    assert!(
        server_hello2.contains("lingxi.wire"),
        "server hello: {server_hello2}"
    );

    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a06_invalid_ws_upgrade_keeps_its_protocol_status() {
    let server = default_server("a06-upgrade-status").await;
    let bearer = server.bearer();
    let key = client_ws_key();
    for headers in [
        format!("Upgrade: websocket\r\nConnection: Upgrade, close\r\nSec-WebSocket-Key: {key}\r\n"),
        "Upgrade: websocket\r\nConnection: Upgrade, close\r\nSec-WebSocket-Version: 13\r\n"
            .to_string(),
    ] {
        let raw = format!(
            "GET /lingxi/v1/ws HTTP/1.1\r\nHost: {}\r\nAuthorization: {bearer}\r\n{headers}\r\n",
            server.addr
        );
        let response = http_raw(server.addr, &raw).await;
        assert!(
            response.starts_with("HTTP/1.1 400"),
            "invalid upgrade shape must be 400: {response}"
        );
        assert!(response.contains("invalid_message"), "response: {response}");
    }
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a06_unmasked_client_frame_closes_with_protocol_error() {
    let server = default_server("a06-unmasked-frame").await;
    let mut ws = ws_upgrade(
        server.addr,
        "/lingxi/v1/ws",
        &[("Authorization", &server.bearer())],
    )
    .await
    .expect("owner socket upgrades");
    ws.write_all(&[0x81, 0x00]).await.unwrap();
    let close = tokio::time::timeout(Duration::from_secs(2), client_ws_read(&mut ws))
        .await
        .expect("server closes unmasked client frame");
    assert_eq!(close_code(&close), Some(1002));

    // 错误连接不能影响下一条合法连接。
    let mut fresh = ws_upgrade(
        server.addr,
        "/lingxi/v1/ws",
        &[("Authorization", &server.bearer())],
    )
    .await
    .expect("fresh owner socket upgrades");
    let hello = serde_json::json!({
        "protocol": "lingxi.wire", "clientKind": "test", "clientVersion": "0",
        "protocolMin": 1, "protocolMax": 1,
    });
    client_ws_send(&mut fresh, hello.to_string().as_bytes()).await;
    assert!(frame_text(&client_ws_read(&mut fresh).await).contains("selectedProtocol"));
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a06_trailing_slashes_do_not_inherit_registered_route_permissions() {
    let server = default_server("a06-route-exactness").await;
    for path in [
        "/lingxi/v1/health/",
        "/lingxi/v1/me/",
        "/lingxi/v1/sessions/",
        "/lingxi/v1/ws/",
    ] {
        let (status, body) = http(server.addr, "GET", path, &[], None).await;
        assert_eq!(status, 401, "stranger {path}: {body}");
        let (status, body) = http(
            server.addr,
            "GET",
            path,
            &[("Authorization", &server.bearer())],
            None,
        )
        .await;
        assert_eq!(status, 404, "owner unknown route {path}: {body}");
    }
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a06_expired_and_replayed_ws_tickets_are_rejected() {
    // Ticket TTL is 80ms in this server instance.
    let server = start_server("a06-ticket", 80, 10_000, 4).await;
    let issue = |server: &TestServer| {
        let addr = server.addr;
        let bearer = server.bearer();
        async move {
            let (status, body) = http(
                addr,
                "POST",
                "/lingxi/v1/ws-ticket",
                &[("Authorization", &bearer)],
                None,
            )
            .await;
            assert_eq!(status, 200, "ticket issue: {body}");
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["ticket"]
                .as_str()
                .unwrap()
                .to_string()
        }
    };

    // EXPIRED: issue, wait past the TTL, upgrade → 401 invalid_ws_ticket.
    let ticket = issue(&server).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let denied = ws_upgrade(
        server.addr,
        &format!("/lingxi/v1/ws?wsTicket={ticket}"),
        &[],
    )
    .await
    .expect_err("expired ticket must be denied");
    assert!(
        denied.starts_with("HTTP/1.1 401"),
        "expired ticket: {denied}"
    );
    assert!(
        denied.contains("invalid_ws_ticket"),
        "expired ticket: {denied}"
    );

    // REPLAY: a fresh ticket used once (successful upgrade) cannot be
    // reused.
    let ticket = issue(&server).await;
    let path = format!("/lingxi/v1/ws?wsTicket={ticket}");
    let mut ws = ws_upgrade(server.addr, &path, &[])
        .await
        .expect("first use works");
    // Complete handshake so the first use is a real completed consumption.
    let hello = serde_json::json!({
        "protocol": "lingxi.wire", "clientKind": "cli", "clientVersion": "1",
        "protocolMin": 1, "protocolMax": 1,
    });
    client_ws_send(&mut ws, hello.to_string().as_bytes()).await;
    let _ = frame_text(&client_ws_read(&mut ws).await);
    drop(ws);
    let denied = ws_upgrade(server.addr, &path, &[])
        .await
        .expect_err("replayed ticket must be denied");
    assert!(denied.starts_with("HTTP/1.1 401"), "replay: {denied}");
    assert!(denied.contains("invalid_ws_ticket"), "replay: {denied}");

    // A ticket on the WRONG path is invalid.
    let ticket = issue(&server).await;
    // Same path with an extra query pair is still /lingxi/v1/ws — it must
    // still upgrade (path binding is on the path, not the whole query).
    drop(
        ws_upgrade(
            server.addr,
            &format!("/lingxi/v1/ws?wsTicket={ticket}&x=1"),
            &[],
        )
        .await
        .expect("extra query pair must not break path binding"),
    );
    // WRONG PATH: a fresh ticket presented on another path is invalid.
    let ticket = issue(&server).await;
    let wrong_path = format!("/lingxi/v1/other?wsTicket={ticket}");
    let denied = ws_upgrade(server.addr, &wrong_path, &[])
        .await
        .expect_err("wrong-path ticket must be denied");
    assert!(
        denied.starts_with("HTTP/1.1 401") || denied.starts_with("HTTP/1.1 403"),
        "wrong path: {denied}"
    );

    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a06_cli_shape_full_business_roundtrip() {
    let server = default_server("a06-cli").await;

    // The CLI/curl shape: no Origin, Host = the real address, bearer token.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha",
        &[("Authorization", &server.bearer())],
        None,
    )
    .await;
    assert_eq!(status, 200, "owner read: {body}");
    assert!(body.contains("sess_local_alpha"));

    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &[("Authorization", &server.bearer())],
        Some(r#"{"input":"run one"}"#),
    )
    .await;
    assert_eq!(status, 200, "owner execute: {body}");
    let accepted: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(accepted["runId"]
        .as_str()
        .is_some_and(|id| id.starts_with("run_")));
    assert_eq!(accepted["runCount"].as_u64(), Some(1));

    // Query-token form (local only) also works for the CLI shape.
    let (status, _) = http(
        server.addr,
        "GET",
        &format!("/lingxi/v1/me?token={}", server.token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, 200, "query token on local connection must work");

    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a07_closed_storage_returns_http_error_without_run_or_completion_event() {
    let server = default_server("a07-http-storage-error").await;
    let bearer = server.bearer();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 200, "正常存储应能读取会话: {body}");

    server
        .storage
        .close()
        .await
        .expect("close test database queue");
    for (method, path, request_body) in [
        ("GET", "/lingxi/v1/sessions", None),
        ("GET", "/lingxi/v1/sessions/sess_local_alpha", None),
        ("GET", "/lingxi/v1/sessions/sess_local_alpha/events", None),
        (
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(r#"{"input":"must not commit"}"#),
        ),
    ] {
        let (status, body) = http(
            server.addr,
            method,
            path,
            &[("Authorization", &bearer)],
            request_body,
        )
        .await;
        assert_eq!(status, 500, "存储关闭后 {method} {path}: {body}");
        let error: serde_json::Value = serde_json::from_str(&body).expect("structured error");
        assert_eq!(error["code"], "internal", "{path}: {error}");
        assert_eq!(error["details"]["reason"], "db_failure", "{path}: {error}");
        assert_eq!(error["details"]["retryable"], false, "{path}: {error}");
        assert_eq!(
            error["details"]["causeId"], "storage.queue_closed",
            "{path}: {error}"
        );
        assert!(error.get("runId").is_none(), "失败响应不能携带成功 runId");
    }

    let home = server.stop_and_keep_home().await;
    let layout = prepare_layout(&home).expect("restart layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let restarted = ServiceState::bootstrap(config, &layout)
        .await
        .expect("restart database");
    assert_eq!(
        restarted
            .storage()
            .count_runs("sess_local_alpha")
            .await
            .expect("reloaded run count"),
        0,
        "重启后不得出现失败请求的 run"
    );
    assert_eq!(
        restarted
            .storage()
            .query_one_text(
                "SELECT COUNT(*) FROM key_events WHERE event_id LIKE '%-done'",
                Vec::new(),
            )
            .await
            .expect("completion event query")
            .as_deref(),
        Some("0"),
        "重启后不得出现完成事件"
    );
    restarted.storage().close().await.expect("close restart DB");
    let _ = std::fs::remove_dir_all(home);
}

#[tokio::test]
async fn limits_body_size_and_rate_and_ws_ceiling() {
    // Dedicated instance: tiny rate budget (5/window), 1 WS connection.
    let server = start_server("limits", 80, 5, 1).await;

    // Body limit: a >1MiB execute body is rejected 413 by the framework.
    let huge = format!("{{\"input\":\"{}\"}}", "x".repeat(1024 * 1024 + 64));
    let (status, _body) = http_post_tolerant(
        server.addr,
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &[("Authorization", &server.bearer())],
        &huge,
    )
    .await;
    assert_eq!(status, 413, "oversized body must be 413");

    // WS ceiling: hold one upgraded connection; the second is refused.
    let first = ws_upgrade(
        server.addr,
        "/lingxi/v1/ws",
        &[("Authorization", &server.bearer())],
    )
    .await
    .expect("first ws connection");
    let refused = ws_upgrade(
        server.addr,
        "/lingxi/v1/ws",
        &[("Authorization", &server.bearer())],
    )
    .await
    .expect_err("second concurrent ws must be refused");
    assert!(
        refused.starts_with("HTTP/1.1 503"),
        "ws ceiling must 503: {refused}"
    );
    drop(first);

    // Rate limit: 5 requests per window from this peer; the 6th is 429.
    // (The requests above consumed part of the budget; hammer until 429.)
    let mut saw_429 = false;
    for _ in 0..12 {
        let (status, _) = http(server.addr, "GET", "/lingxi/v1/health", &[], None).await;
        if status == 429 {
            saw_429 = true;
            break;
        }
    }
    assert!(saw_429, "rate limiter must eventually answer 429");

    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn a05_loopback_mode_refuses_non_loopback_bind_at_config_layer() {
    // Machine-check: loopback is the default network mode and a
    // non-loopback bind under it is a loud configuration error (LAN only
    // through the explicit flag).
    let err = ServiceConfig::from_cli_args(vec![
        "--home".to_string(),
        "/tmp/lingxi-a06-bind".to_string(),
        "--bind".to_string(),
        "0.0.0.0:8080".to_string(),
    ])
    .unwrap_err();
    assert!(matches!(
        err,
        lingxi_service::ConfigError::NetworkModeBindMismatch { .. }
    ));
    let ok = ServiceConfig::from_cli_args(vec![
        "--home".to_string(),
        "/tmp/lingxi-a06-bind".to_string(),
        "--bind".to_string(),
        "0.0.0.0:8080".to_string(),
        "--network-mode".to_string(),
        "lan".to_string(),
    ])
    .unwrap();
    assert_eq!(ok.network_mode, NetworkMode::Lan);
}

#[tokio::test]
async fn r00_account_password_web_cookie_and_logout_have_real_state() {
    let server = default_server("r00-account-web-cookie").await;
    let owner = server.bearer();
    let path = prepare_layout(&server.home)
        .unwrap()
        .runtime_dir
        .join("management.json");
    let before = std::fs::read(&path).unwrap();
    let (status, _) = http(
        server.addr,
        "PUT",
        "/lingxi/v1/access/account/profile",
        &[("Authorization", &owner)],
        Some(r#"{"username":"bad/name"}"#),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(
        before,
        std::fs::read(&path).unwrap(),
        "无效资料不能改状态或审计"
    );

    let (status, body) = http(
        server.addr,
        "PUT",
        "/lingxi/v1/access/account/profile",
        &[("Authorization", &owner)],
        Some(r#"{"username":"alice","displayName":"Alice"}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let account: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(account["account"]["username"], "alice");
    let (status, body) = http(
        server.addr,
        "PUT",
        "/lingxi/v1/access/account/password",
        &[("Authorization", &owner)],
        Some(r#"{"password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["account"]["passwordSet"],
        true
    );
    let login_body =
        r#"{"username":"alice","password":"correct horse battery staple","clientKind":"mobile"}"#;
    let raw = http_raw(server.addr, &format!("POST /lingxi/v1/web-auth/login HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", server.addr, login_body.len(), login_body)).await;
    assert!(raw.starts_with("HTTP/1.1 200"), "登录应成功: {raw}");
    let cookie_line = raw
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .unwrap();
    assert!(
        cookie_line.contains("HttpOnly")
            && cookie_line.contains("SameSite=Strict")
            && cookie_line.contains("Max-Age=1209600")
    );
    assert!(
        !cookie_line.contains("; Secure"),
        "plain HTTP must not advertise a TLS cookie"
    );
    let cookie = cookie_line
        .split_once(':')
        .unwrap()
        .1
        .trim()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    assert!(cookie.starts_with("hana_session=hana_web_"));
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let web: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(web["authenticated"], true);
    assert_eq!(web["principal"]["credentialKind"], "web_session");
    assert!(
        !web["principal"]["scopes"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("settings.write")),
        "mobile profile must have restricted scopes"
    );
    assert!(
        !body.contains("hana_web_"),
        "session secret must not appear in JSON"
    );
    let priority = r#"{"credential":"invalid-device-secret","username":"alice","password":"correct horse battery staple"}"#;
    let (status, _) = http(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[],
        Some(priority),
    )
    .await;
    assert_eq!(
        status, 403,
        "present credential must take precedence over password fallback"
    );
    let desktop_login =
        r#"{"username":"alice","password":"correct horse battery staple","clientKind":"desktop"}"#;
    let desktop_raw = http_raw(server.addr, &format!("POST /lingxi/v1/web-auth/login HTTP/1.1\r\nHost: {}\r\nX-Forwarded-Proto: https\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", server.addr, desktop_login.len(), desktop_login)).await;
    assert!(
        desktop_raw.starts_with("HTTP/1.1 200"),
        "desktop login should succeed locally: {desktop_raw}"
    );
    let desktop_cookie_line = desktop_raw
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .unwrap();
    assert!(
        !desktop_cookie_line.contains("; Secure"),
        "forwarded header cannot spoof TLS"
    );
    let desktop_cookie = desktop_cookie_line
        .split_once(':')
        .unwrap()
        .1
        .trim()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &desktop_cookie)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["principal"]["scopes"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("studio.owner"))
    );
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let me: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(me["serverNodeKind"], "lingxi-service");
    assert!(me["capabilities"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("chat")));
    let raw = http_raw(server.addr, &format!("POST /lingxi/v1/web-auth/logout HTTP/1.1\r\nHost: {}\r\nCookie: {cookie}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", server.addr)).await;
    assert!(raw.starts_with("HTTP/1.1 200") && raw.contains("Max-Age=0"));
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["authenticated"],
        false
    );
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, 401);
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Cookie", &desktop_cookie)],
        None,
    )
    .await;
    assert_eq!(
        status, 200,
        "logging out one session must preserve other sessions"
    );
    let (status, body) = http(
        server.addr,
        "DELETE",
        "/lingxi/v1/access/account/password",
        &[("Authorization", &owner)],
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["account"]["passwordSet"],
        false
    );
    let (status, _) = http(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[],
        Some(login_body),
    )
    .await;
    assert_eq!(status, 403, "removed password must reject fresh login");
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn r00_access_devices_pairing_thinking_qr_and_static_boundaries() {
    let server = default_server("r00-management-boundaries").await;
    let owner = server.bearer();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/access/summary",
        &[("Authorization", &owner)],
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let summary: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(summary["network"]["actualPort"], server.addr.port());
    assert_eq!(summary["account"]["passwordSet"], false);
    let (status, _) = http(server.addr, "GET", "/lingxi/v1/access/summary", &[], None).await;
    assert_eq!(status, 401);
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/access/mobile-qr.svg",
        &[("Authorization", &owner)],
        None,
    )
    .await;
    assert_eq!(status, 400, "LAN unavailable must be explicit: {body}");
    let manager_file = prepare_layout(&server.home)
        .unwrap()
        .runtime_dir
        .join("management.json");
    let before_invalid_network = std::fs::read(&manager_file).unwrap();
    let (status, _) = http(
        server.addr,
        "PUT",
        "/lingxi/v1/access/network",
        &[("Authorization", &owner)],
        Some(r#"{"mode":"lan","listenPort":80}"#),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(
        std::fs::read(&manager_file).unwrap(),
        before_invalid_network,
        "invalid network request must not write state or audit"
    );
    let (status, body) = http(
        server.addr,
        "PUT",
        "/lingxi/v1/access/network",
        &[("Authorization", &owner)],
        Some(r#"{"mode":"loopback","listenPort":14500,"publicBaseUrl":"https://example.test"}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/access/mobile-qr.svg",
        &[("Authorization", &owner)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert!(body.starts_with("<svg ") && body.contains("<path fill=\"#000\""));
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/access/mobile-credentials",
        &[("Authorization", &owner)],
        Some("{}"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let issued: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(issued["accessUrl"], "https://example.test/mobile/");
    let (status, body) = http(
        server.addr,
        "PUT",
        "/lingxi/v1/access/network",
        &[("Authorization", &owner)],
        Some(r#"{"mode":"loopback","listenPort":14500}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["network"]["publicBaseUrl"],
        "https://example.test",
        "omitted public URL must preserve the saved value"
    );
    let (status, body) = http(
        server.addr,
        "PUT",
        "/lingxi/v1/access/network",
        &[("Authorization", &owner)],
        Some(r#"{"mode":"loopback","listenPort":14500,"publicBaseUrl":null}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["network"]["publicBaseUrl"]
            .is_null()
    );
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/access/mobile-qr.svg",
        &[("Authorization", &owner)],
        None,
    )
    .await;
    assert_eq!(
        status, 400,
        "cleared public URL and unavailable LAN must not reuse the old URL: {body}"
    );
    let secret = issued["secret"].as_str().unwrap();
    let cred_id = issued["credential"]["credentialId"].as_str().unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/devices",
        &[("Authorization", &owner)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert!(!body.contains(secret) && !body.contains("secretHash") && !body.contains("secretSalt"));
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/devices",
        &[("Authorization", &format!("Bearer {secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 403, "device cannot read management list");
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/devices/pairing-sessions",
        &[("Authorization", &owner)],
        Some(r#"{"requestedDevice":{"deviceKind":"mobile","displayName":"Phone"}}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let pairing: serde_json::Value = serde_json::from_str(&body).unwrap();
    let pair_id = pairing["pairingSessionId"].as_str().unwrap();
    let code = pairing["userCode"].as_str().unwrap();
    let approve_body = serde_json::json!({"userCode": code}).to_string();
    let (status, body) = http(
        server.addr,
        "POST",
        &format!("/lingxi/v1/devices/pairing-sessions/{pair_id}/approve"),
        &[("Authorization", &owner)],
        Some(&approve_body),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let paired: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(paired["secret"].as_str().unwrap().starts_with("hana_dev_"));
    let (status, _) = http(
        server.addr,
        "POST",
        &format!("/lingxi/v1/devices/pairing-sessions/{pair_id}/approve"),
        &[("Authorization", &owner)],
        Some(&approve_body),
    )
    .await;
    assert_eq!(status, 400, "pairing code must be one-time");
    let (status, _) = http(
        server.addr,
        "POST",
        &format!("/lingxi/v1/devices/credentials/{cred_id}/revoke"),
        &[("Authorization", &owner)],
        Some("{}"),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 401);
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/session-thinking-level?pendingNewSession=1",
        &[("Authorization", &owner)],
        None,
    )
    .await;
    assert_eq!(status, 503);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["details"]["reason"],
        "model_state_unavailable"
    );
    let (status, _) = http(
        server.addr,
        "POST",
        "/lingxi/v1/session-thinking-level",
        &[("Authorization", &owner)],
        Some(r#"{"sessionPath":"sess_local_alpha","level":"high"}"#),
    )
    .await;
    assert_eq!(status, 503);
    let (status, _) = http(
        server.addr,
        "GET",
        "/lingxi/v1/session-thinking-level?sessionPath=sess_local_alpha",
        &[("Authorization", &owner)],
        None,
    )
    .await;
    assert_eq!(status, 503);
    let (status, _) = http(
        server.addr,
        "POST",
        "/lingxi/v1/session-thinking-level",
        &[("Authorization", &owner)],
        Some(r#"{"sessionPath":"sess_local_alpha","level":"nonsense"}"#),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = http(server.addr, "GET", "/mobile/assets/missing.js", &[], None).await;
    assert_eq!(status, 404);
    let (status, body) = http(server.addr, "GET", "/mobile", &[], None).await;
    assert_eq!(status, 200);
    assert!(body.contains("网页界面"));
    server.stop_and_assert_clean().await;
}

#[tokio::test]
async fn device_registry_failure_is_not_reported_as_bad_credentials() {
    let server = default_server("device-registry-error-status").await;
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/devices/credentials",
        &[("Authorization", &server.bearer())],
        Some("{}"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let secret = serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let credential_login = serde_json::json!({"credential": secret}).to_string();
    let raw = http_raw(server.addr, &format!("POST /lingxi/v1/web-auth/login HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", server.addr, credential_login.len(), credential_login)).await;
    assert!(raw.starts_with("HTTP/1.1 200"), "{raw}");
    let cookie = raw
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .unwrap()
        .split_once(':')
        .unwrap()
        .1
        .trim()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let registry_path = prepare_layout(&server.home)
        .unwrap()
        .runtime_dir
        .join(lingxi_service::auth::DEVICE_CREDENTIALS_FILE);
    let original = std::fs::read(&registry_path).unwrap();
    std::fs::write(&registry_path, b"{corrupt").unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {secret}"))],
        None,
    )
    .await;
    assert_eq!(status, 500, "bearer registry failure: {body}");
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[],
        Some(&credential_login),
    )
    .await;
    assert_eq!(status, 500, "web login registry failure: {body}");
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, 500, "cookie registry failure: {body}");
    std::fs::write(&registry_path, original).unwrap();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {secret}"))],
        None,
    )
    .await;
    assert_eq!(
        status, 200,
        "valid registry should restore device auth: {body}"
    );
    server.stop_and_assert_clean().await;
}
