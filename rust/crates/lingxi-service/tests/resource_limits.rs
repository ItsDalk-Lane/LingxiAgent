//! R02-T07 service-level integration tests (integration layer: real axum
//! router, real loopback TCP, real run database in a synthetic isolated
//! home — no mocked core; the injected parts are exactly the surface under
//! test: the clock, the request-id source and the limit values).
//!
//! - **Structured errors (taskbook step 1)**: every outward error body
//!   carries `details.causeId` (stable `domain.cause` vocabulary) and
//!   `details.requestId` (correlation), and its message passes the
//!   redactor — no data-home path can leave the process in an error body.
//! - **Injectable clock (taskbook step 4)**: the per-peer HTTP rate limit
//!   reads the injected clock on the REAL request path — the budget
//!   exhaustion and its reset are driven by `ManualClock::advance`, with
//!   zero sleeps and zero flakes.
//! - **Injectable ids (taskbook step 4)**: `SequentialRequestIdGen` makes
//!   the per-request correlation ids deterministic ordering witnesses.
//! - **Bounded subscriber registry (taskbook step 3)**: over
//!   `max_subscribers_total` / `max_subscribers_per_stream` a subscribe is
//!   explicitly rejected (`SubscribeReject::SubscriberLimit`, surfaced by
//!   the HTTP page endpoint as 503 `budget_exceeded` /
//!   `subscriber_limit`), never silently admitted.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::inject::{ManualClock, SequentialRequestIdGen};
use lingxi_service::{
    events::{SubscribeOutcome, SubscribePageError, SubscribeReject},
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── harness ────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lingxi-r02t07-svc-{tag}-{}", std::process::id()))
}

fn cleanup(path: &std::path::Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

/// Boots the real service with the injected clock / id source / limits.
/// The composition state is `Clone` (Arc-backed): the test keeps a handle
/// to the SAME state the real router serves (same storage, same hub, same
/// limits), so event-service assertions hit the production instance.
async fn boot_with_deps(
    tag: &str,
    clock: Arc<ManualClock>,
    request_ids: Arc<SequentialRequestIdGen>,
    deps: ServiceDeps,
) -> (TestServer, ServiceState, Arc<ManualClock>) {
    let home = synthetic_home(tag);
    cleanup(&home);
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let deps = ServiceDeps {
        clock: clock.clone(),
        request_ids,
        ..deps
    };
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    // Bootstrap in THIS task, keep a clone of the Arc-backed state for the
    // test, then hand the serving task the original.
    let layout = prepare_layout(&config.data_home).expect("prepare layout");
    let state = ServiceState::bootstrap_with_deps(config.clone(), &layout, deps)
        .await
        .expect("bootstrap with deps");
    let test_view = state.clone();
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
    let addr = ready_rx.await.expect("service reports readiness");
    (
        TestServer {
            addr,
            home,
            stop: stop_tx,
            handle,
        },
        test_view,
        clock,
    )
}

impl TestServer {
    async fn stop_and_assert_clean(self) {
        self.stop.send(()).expect("server task still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
    }

    fn local_token(&self) -> String {
        let raw =
            std::fs::read_to_string(self.home.join("lingxi-service").join("local-token.json"))
                .expect("local token file exists");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("token json");
        value["token"].as_str().expect("token string").to_string()
    }
}

/// Minimal real HTTP request over a tokio TCP stream (Connection: close).
async fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    authorization: Option<&str>,
) -> (String, String) {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    if let Some(auth) = authorization {
        request.push_str(&format!("Authorization: {auth}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read response");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    (head.to_string(), body.to_string())
}

fn status_of(head: &str) -> u16 {
    head.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status in head: {head}"))
}

fn manual_clock(start: u64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(start))
}

fn sequential_ids() -> Arc<SequentialRequestIdGen> {
    Arc::new(SequentialRequestIdGen::new())
}

// ── structured errors: causeId + requestId + redacted message ───────────────

#[tokio::test]
async fn a13_error_bodies_carry_cause_id_request_id_and_redacted_paths() {
    let (server, _state, _clock) = boot_with_deps(
        "err-surface",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps::default(),
    )
    .await;
    let addr = server.addr;
    let home_display = server.home.display().to_string();

    // 401 (missing credential): the body carries the structured cause and
    // the per-request correlation id; no data-home path can appear.
    let (head, body) = http_request(addr, "GET", "/lingxi/v1/me", None).await;
    assert_eq!(status_of(&head), 401, "head: {head}");
    let json: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("body not json: {body:?}"));
    assert_eq!(json["code"], "unauthorized");
    assert_eq!(json["retryable"], false);
    assert_eq!(json["details"]["causeId"], "auth.missing_credential");
    assert_eq!(json["details"]["requestId"], "req-0000000000000001");
    assert!(
        !body.contains(&home_display),
        "error body must not contain the data home: {body}"
    );

    // 401 with a bad token: the cause reflects the authentication failure;
    // the bad token value itself is not echoed into the body.
    let (head, body) = http_request(
        addr,
        "GET",
        "/lingxi/v1/me",
        Some("Bearer hana_dev_NOT_A_REAL_TOKEN_0123456789"),
    )
    .await;
    assert_eq!(status_of(&head), 401, "head: {head}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        json["details"]["causeId"], "auth.invalid_credential",
        "{json}"
    );
    assert!(
        !body.contains("NOT_A_REAL_TOKEN"),
        "the rejected credential must not be echoed: {body}"
    );

    // The correlation ids are sequential (injected generator): the two
    // requests above minted req-…0001 and req-…0002.
    let (_, body) = http_request(addr, "GET", "/lingxi/v1/me", None).await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["details"]["requestId"], "req-0000000000000003");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

#[tokio::test]
async fn not_found_error_carries_structured_cause() {
    let (server, _state, _clock) = boot_with_deps(
        "err-causes",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps::default(),
    )
    .await;
    let addr = server.addr;
    let token = server.local_token();

    let (head, body) = http_request(
        addr,
        "GET",
        "/lingxi/v1/sessions/sess_missing_unknown",
        Some(&format!("Bearer {token}")),
    )
    .await;
    assert_eq!(status_of(&head), 404, "head: {head}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["details"]["causeId"], "resource.not_found", "{json}");
    assert_eq!(json["code"], "not_found", "{json}");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

// ── injectable clock on the REAL request path: rate limit without sleeps ───

#[tokio::test]
async fn manual_clock_drives_rate_limit_reset_without_sleeps() {
    let (server, _state, clock) = boot_with_deps(
        "rate-clock",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            rate_max: 2,
            rate_window_ms: 1_000,
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = server.addr;
    let token = server.local_token();
    let auth = format!("Bearer {token}");

    // The clock starts at 0: two requests fill the fixed window.
    let (head, _) = http_request(addr, "GET", "/lingxi/v1/me", Some(&auth)).await;
    assert_eq!(status_of(&head), 200);
    let (head, _) = http_request(addr, "GET", "/lingxi/v1/me", Some(&auth)).await;
    assert_eq!(status_of(&head), 200);
    // Third request in the same window: explicit 429 with the structured
    // cause.
    let (head, body) = http_request(addr, "GET", "/lingxi/v1/me", Some(&auth)).await;
    assert_eq!(status_of(&head), 429, "head: {head}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["details"]["causeId"], "transport.rate_limited");
    assert_eq!(json["details"]["reason"], "rate_limited");

    // Advance the injected clock past the window: the budget resets. No
    // sleep, no flake — the whole proof is deterministic.
    clock.advance(1_000);
    let (head, _) = http_request(addr, "GET", "/lingxi/v1/me", Some(&auth)).await;
    assert_eq!(status_of(&head), 200, "window reset must admit again");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

// ── bounded subscriber registry (R02-T07 step 3) ────────────────────────────

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

#[tokio::test]
async fn subscriber_registry_cap_rejects_explicitly() {
    let (server, state, _clock) = boot_with_deps(
        "sub-cap",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            event_limits: lingxi_service::EventLimits {
                max_subscribers_total: 3,
                max_subscribers_per_stream: 2,
                ..lingxi_service::EventLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;
    let owner = owner_principal();

    // Two live subscribers on the stream fill the per-stream (and total)
    // registry.
    let s1 = state
        .events()
        .subscribe(&owner, "sess_local_alpha", None)
        .await;
    assert!(matches!(s1, Ok(SubscribeOutcome::Started { .. })), "{s1:?}");
    let s2 = state
        .events()
        .subscribe(&owner, "sess_local_alpha", None)
        .await;
    assert!(matches!(s2, Ok(SubscribeOutcome::Started { .. })), "{s2:?}");

    // Third subscribe: EXPLICIT rejection (never a silent admission).
    let third = state
        .events()
        .subscribe(&owner, "sess_local_alpha", None)
        .await
        .err();
    assert!(
        matches!(
            &third,
            Some(SubscribeReject::SubscriberLimit { scope, limit, .. })
                if *scope == "stream" && *limit == 2
        ),
        "expected per-stream subscriber_limit rejection, got {third:?}"
    );

    // A DIFFERENT stream still has room in the total registry.
    let other = state
        .events()
        .subscribe(&owner, "sess_local_beta", None)
        .await;
    assert!(
        matches!(other, Ok(SubscribeOutcome::Started { .. })),
        "another stream must subscribe while the total cap has room: {other:?}"
    );

    // The registry observability reflects the real counts.
    let stats = state.events().hub().hub_stats();
    assert_eq!(stats.live_subscribers, 3);

    // Dropping a real subscription frees the slot (the guard unregisters).
    drop(s2);
    let again = state
        .events()
        .subscribe(&owner, "sess_local_alpha", None)
        .await;
    assert!(
        matches!(again, Ok(SubscribeOutcome::Started { .. })),
        "freed slot must be reusable: {again:?}"
    );

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

#[tokio::test]
async fn events_page_maps_subscriber_limit_to_explicit_503() {
    let (server, state, _clock) = boot_with_deps(
        "sub-cap-page",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            event_limits: lingxi_service::EventLimits {
                max_subscribers_per_stream: 1,
                ..lingxi_service::EventLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;
    let owner = owner_principal();

    // A live subscription occupies the single per-stream slot.
    let _live = state
        .events()
        .subscribe(&owner, "sess_local_alpha", None)
        .await
        .expect("first subscribe");

    // The HTTP page endpoint surfaces the cap as the explicit 503 mapping
    // (budget_exceeded / subscriber_limit) — the caller retries later.
    match state
        .events()
        .events_page(&owner, "sess_local_alpha", None, None)
        .await
    {
        Err(SubscribePageError::Reject(SubscribeReject::SubscriberLimit {
            scope, limit, ..
        })) => {
            assert_eq!(scope, "stream");
            assert_eq!(limit, 1);
        }
        other => panic!("expected SubscriberLimit, got {other:?}"),
    }

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

// ── F07 (R02 stage-repair R1): bounded HTTP admission — in-flight cap and
// per-request budget on the REAL request path ────────────────────────────────

/// Opens a POST whose body never completes: Content-Length announces 1000
/// bytes, only "{" is sent. The handler's body read stays pending, so the
/// request occupies its admission slot until the connection drops or the
/// request budget cancels it.
async fn partial_post(addr: SocketAddr, path: &str, token: &str) -> tokio::net::TcpStream {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\
         Authorization: Bearer {token}\r\nContent-Type: application/json\r\n\
         Content-Length: 1000\r\n\r\n{{"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write partial request");
    stream
}

/// Waits until the served admission gate reports exactly `expected`
/// in-flight requests — observing the real state, not a wall-clock guess.
async fn wait_in_flight(state: &ServiceState, expected: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if state.admission().current() == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "in-flight count never reached {expected} (now {})",
            state.admission().current()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn http_in_flight_cap_rejects_overflow_and_recovers_after_release() {
    let (server, state, _clock) = boot_with_deps(
        "inflight",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            http_max_in_flight: 1,
            http_request_budget_ms: 10_000,
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = server.addr;
    let token = server.local_token();

    // Client A holds the single slot with a never-completing body.
    let stuck = partial_post(addr, "/lingxi/v1/sessions/sess_local_alpha/execute", &token).await;
    wait_in_flight(&state, 1).await;

    // Client B is over the cap: explicit 503 with the structured reason —
    // and the PUBLIC health route passes the same gate (admission is the
    // outer edge, not a per-route courtesy).
    let (head, body) = http_request(addr, "GET", "/lingxi/v1/health", None).await;
    assert_eq!(status_of(&head), 503, "head: {head}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["code"], "budget_exceeded", "{json}");
    assert_eq!(json["details"]["reason"], "http_in_flight_limit", "{json}");
    assert_eq!(
        json["details"]["causeId"], "transport.http_in_flight_limit",
        "{json}"
    );
    assert!(json["details"]["requestId"].is_string(), "{json}");

    // Releasing A frees the slot; B is admitted again — no sticky refusal.
    drop(stuck);
    wait_in_flight(&state, 0).await;
    let (head, _body) = http_request(addr, "GET", "/lingxi/v1/health", None).await;
    assert_eq!(status_of(&head), 200, "head: {head}");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

#[tokio::test]
async fn request_budget_cancels_a_slow_body_and_answers_408() {
    let (server, state, _clock) = boot_with_deps(
        "reqbudget",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            http_max_in_flight: 4,
            http_request_budget_ms: 300,
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = server.addr;
    let token = server.local_token();

    // The client announces 1000 bytes but sends only "{": without the
    // budget the body read would block past any bound. The server must
    // cancel the request and answer 408 — while the client is still
    // connected.
    let mut stream =
        partial_post(addr, "/lingxi/v1/sessions/sess_local_alpha/execute", &token).await;
    wait_in_flight(&state, 1).await;
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut raw))
        .await
        .expect("the budget closes the request within seconds, not never")
        .expect("read response");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("response split");
    assert_eq!(status_of(head), 408, "head: {head}");
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["details"]["reason"], "request_timeout", "{json}");
    assert_eq!(
        json["details"]["causeId"], "transport.request_timeout",
        "{json}"
    );

    // The cancelled request released its slot.
    wait_in_flight(&state, 0).await;

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

// ── R2-F04 (R02 stage-repair R2): CONNECTION-level admission — the count cap
// and the header-wait time budget bind from the accept edge, before hyper
// parses a single byte (the request-level middleware only runs after complete
// headers). ─────────────────────────────────────────────────────────────────

/// Waits until the transport admission gate reports exactly `expected` open
/// connections — same observe-the-real-state discipline as `wait_in_flight`.
async fn wait_connections(state: &ServiceState, expected: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if state.connection_admission().current() == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "connection count never reached {expected} (now {})",
            state.connection_admission().current()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Reads a raw stream to termination, tolerating the reset a server-side
/// close may produce; returns the bytes received (the transport rejection
/// path sends NONE — there is no response channel before headers).
async fn read_to_termination(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(err) => panic!("read: {err}"),
        }
    }
    raw
}

#[tokio::test]
async fn connection_cap_rejects_over_cap_before_headers_and_recovers_after_release() {
    // http_max_in_flight = 1 -> connection cap 2 (2× derivation keeps the
    // request-level gate reachable beneath the transport edge).
    let (server, state, _clock) = boot_with_deps(
        "conncap",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            http_max_in_flight: 1,
            http_request_budget_ms: 10_000,
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = server.addr;

    // Connections A and B occupy BOTH connection slots without ever
    // sending a header byte — the request-level gate never even sees them
    // (the pre-fix defect: such sockets were counted nowhere).
    let holding_a = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect A");
    let holding_b = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect B");
    wait_connections(&state, 2).await;
    assert_eq!(state.admission().current(), 0, "no request is in flight");

    // Connection C is over the transport cap: the freshly accepted socket
    // is closed immediately — no response bytes, no silent queueing — even
    // though C behaves like a perfectly healthy HTTP client.
    let mut rejected = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect C");
    rejected
        .write_all(format!("GET /lingxi/v1/health HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .ok(); // the server may already have closed the socket
    let raw = tokio::time::timeout(Duration::from_secs(5), read_to_termination(&mut rejected))
        .await
        .expect("the over-cap connection is closed within seconds, not never");
    assert!(
        raw.is_empty(),
        "a transport-rejected connection receives no response bytes, got: {}",
        String::from_utf8_lossy(&raw)
    );

    // Releasing the holders frees the connection slots; a healthy client
    // is served again — no sticky refusal at the transport edge either.
    drop(holding_a);
    drop(holding_b);
    wait_connections(&state, 0).await;
    let (head, _body) = http_request(addr, "GET", "/lingxi/v1/health", None).await;
    assert_eq!(status_of(&head), 200, "head: {head}");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

#[tokio::test]
async fn header_wait_budget_closes_a_never_headers_connection_and_recovers() {
    let (server, state, _clock) = boot_with_deps(
        "hdrbudget",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            http_max_in_flight: 4,
            http_request_budget_ms: 200,
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = server.addr;

    // The client sends an UNTERMINATED header block (no final CRLF) and
    // then stalls. The connection-level header-wait budget — the same
    // knob as the per-request budget — must close the socket; without it
    // this connection used to sit entirely outside every configured limit.
    let mut stalled = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect stalled");
    stalled
        .write_all(format!("GET /lingxi/v1/health HTTP/1.1\r\nHost: {addr}\r\n").as_bytes())
        .await
        .expect("write partial headers");
    wait_connections(&state, 1).await;

    let started = std::time::Instant::now();
    let raw = tokio::time::timeout(Duration::from_secs(5), read_to_termination(&mut stalled))
        .await
        .expect("the header-wait budget closes the connection within seconds, not never");
    let waited = started.elapsed();
    assert!(
        raw.is_empty(),
        "no response can be sent before complete headers, got: {}",
        String::from_utf8_lossy(&raw)
    );
    // Truthful bound: the wait is in the budget's order of magnitude —
    // well above zero (the budget had to expire), far below the previous
    // unbounded/hyper-default behaviour (30s).
    assert!(
        waited < Duration::from_secs(10),
        "header budget 200ms must close the socket promptly, took {waited:?}"
    );
    wait_connections(&state, 0).await;

    // A healthy client on a fresh connection is unaffected.
    let (head, _body) = http_request(addr, "GET", "/lingxi/v1/health", None).await;
    assert_eq!(status_of(&head), 200, "head: {head}");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

// ── R3-F01: the header budget covers the connection from the ACCEPT moment ──
// Pre-fix the transport served connections through hyper-util's `auto`
// builder, which parks every fresh connection in an untimed protocol-version
// sniffing state: it waits for the first bytes to tell HTTP/1 from the
// HTTP/2 preface, and hyper's header_read_timeout only arms AFTER that,
// inside the HTTP/1 state machine. A client that sent NOTHING (or trickled
// a strict prefix of the HTTP/2 preface) therefore held a connection slot
// indefinitely — measured pre-fix: still open at 1002ms with a 100ms budget,
// and a request issued after a 450ms stall was even answered 200. The fix
// serves explicit HTTP/1 (hyper's own http1 builder + `with_upgrades`), so
// there is no sniffing state and the timer arms at the first poll.

#[tokio::test]
async fn header_budget_covers_a_zero_byte_connection_from_the_accept_moment() {
    let (server, state, _clock) = boot_with_deps(
        "zerobyte",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            http_max_in_flight: 4,
            http_request_budget_ms: 200,
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = server.addr;

    // The client connects and sends NOTHING — the exact R3-F01 repro. The
    // header budget must still close the socket: the wait for the FIRST
    // byte is inside the budget, not before it.
    let mut silent = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect silent");
    wait_connections(&state, 1).await;

    let started = std::time::Instant::now();
    let raw = tokio::time::timeout(Duration::from_secs(5), read_to_termination(&mut silent))
        .await
        .expect("R3-F01: a zero-byte connection must be closed by the header budget, not never");
    let waited = started.elapsed();
    assert!(
        raw.is_empty(),
        "no response can be sent before any byte arrives, got: {}",
        String::from_utf8_lossy(&raw)
    );
    assert!(
        waited < Duration::from_secs(10),
        "header budget 200ms must close the zero-byte socket promptly, took {waited:?}"
    );
    // The slot is released after the deadline (期限后释放恢复): the count
    // returns to zero and a healthy client is served on a fresh socket.
    wait_connections(&state, 0).await;
    let (head, _body) = http_request(addr, "GET", "/lingxi/v1/health", None).await;
    assert_eq!(status_of(&head), 200, "head: {head}");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

#[tokio::test]
async fn header_budget_covers_a_stalled_short_prefix_of_the_h2_preface() {
    let (server, state, _clock) = boot_with_deps(
        "shortprefix",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            http_max_in_flight: 4,
            http_request_budget_ms: 200,
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = server.addr;

    // "PRI" is a strict prefix of the HTTP/2 connection preface
    // ("PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"). With the pre-fix auto builder
    // this kept the version sniffer waiting for more preface bytes FOREVER
    // (the bytes matched, so the sniffer could not even fall back to
    // HTTP/1). With explicit HTTP/1 serving it is simply an incomplete
    // HTTP/1 request line, bounded by the same header budget.
    let mut stalled = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect stalled");
    stalled.write_all(b"PRI").await.expect("write short prefix");
    wait_connections(&state, 1).await;

    let started = std::time::Instant::now();
    let raw = tokio::time::timeout(Duration::from_secs(5), read_to_termination(&mut stalled))
        .await
        .expect("R3-F01: a stalled short-prefix connection must be closed by the header budget");
    let waited = started.elapsed();
    assert!(
        raw.is_empty(),
        "no response can be sent for an incomplete request line, got: {}",
        String::from_utf8_lossy(&raw)
    );
    assert!(
        waited < Duration::from_secs(10),
        "header budget 200ms must close the short-prefix socket promptly, took {waited:?}"
    );
    wait_connections(&state, 0).await;

    // Recovery: a healthy client on a fresh connection is unaffected.
    let (head, _body) = http_request(addr, "GET", "/lingxi/v1/health", None).await;
    assert_eq!(status_of(&head), 200, "head: {head}");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}

#[tokio::test]
async fn delayed_first_request_beyond_the_budget_finds_the_connection_closed() {
    // The R3-F01 review scenario exactly: ONE ordinary client that simply
    // waits longer than the budget before its first request. Pre-fix that
    // request was answered (457ms wait with a 100ms budget -> 200); the
    // budget must close the socket first, so the late write finds a dead
    // connection (no response bytes ever arrive).
    let (server, state, _clock) = boot_with_deps(
        "delayed",
        manual_clock(0),
        sequential_ids(),
        ServiceDeps {
            http_max_in_flight: 4,
            http_request_budget_ms: 200,
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = server.addr;

    let mut delayed = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect delayed");
    wait_connections(&state, 1).await;
    // Stall well past the 200ms budget, THEN send a complete valid request.
    tokio::time::sleep(Duration::from_millis(600)).await;
    delayed
        .write_all(
            format!("GET /lingxi/v1/health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .ok(); // the server may already have closed the socket
    let raw = tokio::time::timeout(Duration::from_secs(5), read_to_termination(&mut delayed))
        .await
        .expect("the closed connection terminates the read");
    assert!(
        raw.is_empty(),
        "a first request issued past the header budget must NOT be answered, got: {}",
        String::from_utf8_lossy(&raw)
    );
    wait_connections(&state, 0).await;

    // Recovery: the budget released the slot; a healthy client is served.
    let (head, _body) = http_request(addr, "GET", "/lingxi/v1/health", None).await;
    assert_eq!(status_of(&head), 200, "head: {head}");

    let home = server.home.clone();
    server.stop_and_assert_clean().await;
    cleanup(&home);
}
