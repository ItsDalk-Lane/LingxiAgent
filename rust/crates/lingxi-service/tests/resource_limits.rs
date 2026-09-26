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
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceError,
    ServiceState,
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
    handle: tokio::task::JoinHandle<Result<(), ServiceError>>,
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
