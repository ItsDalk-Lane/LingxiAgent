//! R05-T02 C08 integration evidence: the two OAuth flow state machines
//! against a CONTROLLED 127.0.0.1 stand-in — real TCP, real HTTP/1.1, real
//! reqwest client (the production `OAuthHttp`), zero external network.
//!
//! Double boundary: the stub authorization server is the outside world at
//! the far end of the wire; every in-process component (the state machines,
//! the HTTP client, the classification, the scrubbing) is the production
//! code path. The clock/sleep is injected (`FlowClock`) so polling cadence,
//! slow_down backoff and deadlines are proven without sleeping a real
//! second.
//!
//! Covered (the C08 matrix): device-code pending → success, slow_down
//! (+5s/step, the incumbent's rule), deadline expiry, denial, cancellation
//! (before any poll leaves the machine), the JWT-exp fallback and its loud
//! absence; authorization-code+PKCE happy path (S256 binding proven), wrong
//! state (400, flow survives), error callback, expired state (loop-top and
//! the post-accept re-check), duplicate callback (closed listener); plus
//! the shared refresh grant's classification (invalid_grant / 401 / 429 /
//! 5xx / timeout) and the C10/C09 guards: redirects are never followed,
//! endpoint echoes are scrubbed of in-play material.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_adapters::models::config::{OAuthFlowConfig, OAuthFlowKind};
use lingxi_adapters::models::oauth::{
    refresh, run_device_code_flow, system_flow_clock, AuthorizationCodeFlow, FlowClock,
    NeverCancel, NotifyCancel, OAuthError, OAuthHttp, OAUTH_STATE_TTL,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── injected flow clock (deterministic: recorded sleeps, manual time) ───────

/// The boxed sleep closure a [`FlowClock`] borrows (alias for readability).
type BoxedSleep = Box<
    dyn Fn(Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>
        + Send
        + Sync,
>;

/// A manual-time fixture: `now` is advanced explicitly; `sleep` records the
/// requested duration, optionally advances the manual time by it (the "time
/// actually passed" simulation) and completes immediately — or parks forever
/// (`park_on_sleep`, so the cancellation tests win the `select!` via
/// `NotifyCancel`).
struct ClockFixture {
    now: Arc<AtomicU64>,
    sleeps: Arc<Mutex<Vec<Duration>>>,
}

impl ClockFixture {
    fn new(start_unix_ms: u64) -> Self {
        Self {
            now: Arc::new(AtomicU64::new(start_unix_ms)),
            sleeps: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn now(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }

    fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    fn recorded_sleeps(&self) -> Vec<Duration> {
        self.sleeps.lock().expect("sleeps").clone()
    }

    /// Builds a `'static` [`FlowClock`]. The two closures are `Box::leak`ed —
    /// tests are short-lived processes, and the leak buys the ergonomics of
    /// moving the clock into spawned flow tasks.
    fn clock(&self, advance_on_sleep: bool, park_on_sleep: bool) -> FlowClock<'static> {
        self.clock_with_timeout(advance_on_sleep, park_on_sleep, Duration::from_secs(5))
    }

    fn clock_with_timeout(
        &self,
        advance_on_sleep: bool,
        park_on_sleep: bool,
        request_timeout: Duration,
    ) -> FlowClock<'static> {
        let now = Arc::clone(&self.now);
        let now_fn: Box<dyn Fn() -> u64 + Send + Sync> =
            Box::new(move || now.load(Ordering::SeqCst));
        let sleeps = Arc::clone(&self.sleeps);
        let now_for_sleep = Arc::clone(&self.now);
        let sleep_fn: BoxedSleep = Box::new(move |duration| {
            sleeps.lock().expect("sleeps").push(duration);
            if advance_on_sleep {
                now_for_sleep.fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
            }
            if park_on_sleep {
                Box::pin(std::future::pending())
            } else {
                Box::pin(async move {})
            }
        });
        FlowClock {
            now_unix_ms: Box::leak(now_fn),
            sleep: Box::leak(sleep_fn),
            request_timeout,
        }
    }
}

// ── the controlled authorization-server stand-in (raw TCP, 127.0.0.1) ───────

#[derive(Debug, Clone)]
struct StubAnswer {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl StubAnswer {
    fn json(status: u16, body: serde_json::Value) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct RecordedForm {
    path: String,
    fields: BTreeMap<String, String>,
}

struct AuthStub {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedForm>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl AuthStub {
    /// Starts the stand-in. `scripts` maps the request path (`/token`,
    /// `/device`) to its ordered answer script; an exhausted or unscripted
    /// path is a LOUD 500, never an improvised answer.
    async fn start(scripts: &[(&str, Vec<StubAnswer>)]) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let scripts: BTreeMap<String, VecDeque<StubAnswer>> = scripts
            .iter()
            .map(|(path, answers)| (path.to_string(), answers.iter().cloned().collect()))
            .collect();
        let scripts = Arc::new(Mutex::new(scripts));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_requests, task_scripts) = (Arc::clone(&requests), Arc::clone(&scripts));
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                let (path, body) = read_form_request(&mut socket).await;
                task_requests.lock().expect("requests").push(RecordedForm {
                    path: path.clone(),
                    fields: parse_form(&body),
                });
                let answer = {
                    let mut scripts = task_scripts.lock().expect("scripts");
                    scripts
                        .get_mut(path.as_str())
                        .and_then(VecDeque::pop_front)
                        .unwrap_or_else(|| StubAnswer {
                            status: 500,
                            headers: Vec::new(),
                            body: r#"{"error":"stub_script_exhausted"}"#.to_string(),
                        })
                };
                let status_text = match answer.status {
                    200 => "200 OK".to_string(),
                    302 => "302 Found".to_string(),
                    other => format!("{other} Error"),
                };
                let mut response = format!(
                    "HTTP/1.1 {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
                    answer.body.len(),
                );
                for (name, value) in &answer.headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str(&format!("Connection: close\r\n\r\n{}", answer.body));
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            addr,
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    fn requests_to(&self, path: &str) -> Vec<RecordedForm> {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .filter(|request| request.path == path)
            .cloned()
            .collect()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

/// Reads one form POST (request line + headers + content-length body).
/// Malformed input panics — the stub is test equipment, loudness is the
/// point.
async fn read_form_request(socket: &mut tokio::net::TcpStream) -> (String, String) {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub read stalled")
            .expect("stub read");
        assert!(read > 0, "stub: connection closed before headers");
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        assert!(raw.len() < 256 * 1024, "stub: header block too large");
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("stub: utf8 head");
    let path = head
        .split("\r\n")
        .next()
        .expect("request line")
        .split_whitespace()
        .nth(1)
        .expect("request path")
        .to_string();
    let mut content_length = None;
    for line in head.split("\r\n").skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().parse::<usize>().expect("content-length"));
            }
        }
    }
    let content_length = content_length.expect("form posts carry a content-length");
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub body read stalled")
            .expect("stub body read");
        assert!(read > 0, "stub: connection closed mid-body");
        raw.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8(raw[body_start..body_start + content_length].to_vec())
        .expect("stub: utf8 body");
    (path, body)
}

fn parse_form(body: &str) -> BTreeMap<String, String> {
    body.split('&')
        .filter_map(|pair| {
            pair.split_once('=')
                .map(|(k, v)| (k.to_string(), percent_decode(v)))
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi * 16 + lo) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── config builders ─────────────────────────────────────────────────────────

fn device_flow(stub: &AuthStub) -> OAuthFlowConfig {
    OAuthFlowConfig {
        flow: OAuthFlowKind::DeviceCode,
        client_id: "client-test".to_string(),
        token_endpoint: stub.url("/token"),
        authorize_endpoint: None,
        device_authorization_endpoint: Some(stub.url("/device")),
        scopes: Some("openid offline_access".to_string()),
    }
}

fn pkce_flow(stub: &AuthStub) -> OAuthFlowConfig {
    OAuthFlowConfig {
        flow: OAuthFlowKind::AuthorizationCodePkce,
        client_id: "client-test".to_string(),
        token_endpoint: stub.url("/token"),
        authorize_endpoint: Some(stub.url("/authorize")),
        device_authorization_endpoint: None,
        scopes: Some("openid".to_string()),
    }
}

fn http() -> OAuthHttp {
    OAuthHttp::new().expect("oauth http client")
}

fn tokens_json(access: &str, refresh: Option<&str>, expires_in: u64) -> serde_json::Value {
    let mut value = serde_json::json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": expires_in,
    });
    if let Some(refresh) = refresh {
        value["refresh_token"] = serde_json::Value::String(refresh.to_string());
    }
    value
}

// ── device-code flow (xai type) ─────────────────────────────────────────────

#[tokio::test]
async fn device_code_happy_path_prompts_once_and_mints_tokens() {
    let stub = AuthStub::start(&[
        (
            "/device",
            vec![StubAnswer::json(
                200,
                serde_json::json!({
                    "device_code": "dc-1",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://auth.example.test/activate",
                    "expires_in": 900,
                    "interval": 7
                }),
            )],
        ),
        (
            "/token",
            vec![
                StubAnswer::json(200, serde_json::json!({"error": "authorization_pending"})),
                StubAnswer::json(200, tokens_json("at-1", Some("rt-1"), 3600)),
            ],
        ),
    ])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let tokens = run_device_code_flow(
        &http(),
        &device_flow(&stub),
        &fixture.clock(false, false),
        &NeverCancel,
        {
            let prompts = Arc::clone(&prompts);
            move |prompt: &lingxi_adapters::models::oauth::DeviceCodePrompt| {
                prompts.lock().expect("prompts").push(prompt.clone())
            }
        },
    )
    .await
    .expect("flow succeeds");
    assert_eq!(tokens.access_token, "at-1");
    assert_eq!(tokens.refresh_token, "rt-1");
    assert_eq!(tokens.expires_at_unix_ms, 1_000_000 + 3_600_000);
    // The prompt fired exactly once with the advertised interval/deadline.
    {
        let prompts = prompts.lock().expect("prompts");
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].user_code, "ABCD-EFGH");
        assert_eq!(
            prompts[0].verification_uri,
            "https://auth.example.test/activate"
        );
        assert_eq!(prompts[0].interval, Duration::from_secs(7));
        assert_eq!(prompts[0].expires_in, Duration::from_secs(900));
    }
    // One sleep of the advertised interval before EACH poll.
    assert_eq!(
        fixture.recorded_sleeps(),
        vec![Duration::from_secs(7), Duration::from_secs(7)]
    );
    // The wire shape: device authorization carries client_id+scope; the
    // poll carries the device-code grant triple.
    let device_requests = stub.requests_to("/device");
    assert_eq!(device_requests.len(), 1);
    assert_eq!(device_requests[0].fields["client_id"], "client-test");
    assert_eq!(device_requests[0].fields["scope"], "openid offline_access");
    let polls = stub.requests_to("/token");
    assert_eq!(polls.len(), 2);
    assert_eq!(
        polls[0].fields["grant_type"],
        "urn:ietf:params:oauth:grant-type:device_code"
    );
    assert_eq!(polls[0].fields["device_code"], "dc-1");
    stub.stop().await;
}

#[tokio::test]
async fn device_code_slow_down_grows_the_interval_by_five_seconds() {
    let stub = AuthStub::start(&[
        (
            "/device",
            vec![StubAnswer::json(
                200,
                serde_json::json!({
                    "device_code": "dc-1",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://auth.example.test/activate",
                    "expires_in": 900
                    // interval absent: the incumbent default is 5s
                }),
            )],
        ),
        (
            "/token",
            vec![
                StubAnswer::json(200, serde_json::json!({"error": "slow_down"})),
                StubAnswer::json(200, serde_json::json!({"error": "slow_down"})),
                StubAnswer::json(200, tokens_json("at-1", Some("rt-1"), 60)),
            ],
        ),
    ])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let tokens = run_device_code_flow(
        &http(),
        &device_flow(&stub),
        &fixture.clock(false, false),
        &NeverCancel,
        |_| {},
    )
    .await
    .expect("flow succeeds");
    assert_eq!(tokens.access_token, "at-1");
    // Default 5s, then +5s per slow_down (the incumbent's rule): 5, 10, 15.
    assert_eq!(
        fixture.recorded_sleeps(),
        vec![
            Duration::from_secs(5),
            Duration::from_secs(10),
            Duration::from_secs(15)
        ]
    );
    stub.stop().await;
}

#[tokio::test]
async fn device_code_deadline_expires_the_flow() {
    let stub = AuthStub::start(&[
        (
            "/device",
            vec![StubAnswer::json(
                200,
                serde_json::json!({
                    "device_code": "dc-1",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://auth.example.test/activate",
                    "expires_in": 12,
                    "interval": 5
                }),
            )],
        ),
        (
            "/token",
            // Pending forever — the deadline must end the flow, never an
            // unbounded poll loop.
            (0..8)
                .map(|_| {
                    StubAnswer::json(200, serde_json::json!({"error": "authorization_pending"}))
                })
                .collect(),
        ),
    ])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    // Sleeps advance the manual clock — the deadline arrives deterministically.
    let err = run_device_code_flow(
        &http(),
        &device_flow(&stub),
        &fixture.clock(true, false),
        &NeverCancel,
        |_| {},
    )
    .await
    .expect_err("the deadline ends the flow");
    assert!(matches!(err, OAuthError::Expired { .. }), "{err}");
    assert!(fixture.now() >= 1_000_000 + 12_000);
    stub.stop().await;
}

#[tokio::test]
async fn device_code_denial_and_expired_token_are_terminal_and_distinct() {
    for (error_code, expect_expired) in [("access_denied", false), ("expired_token", true)] {
        let stub = AuthStub::start(&[
            (
                "/device",
                vec![StubAnswer::json(
                    200,
                    serde_json::json!({
                        "device_code": "dc-1",
                        "user_code": "ABCD-EFGH",
                        "verification_uri": "https://auth.example.test/activate",
                        "expires_in": 900
                    }),
                )],
            ),
            (
                "/token",
                vec![StubAnswer::json(
                    400,
                    serde_json::json!({"error": error_code, "error_description": "nope"}),
                )],
            ),
        ])
        .await;
        let fixture = ClockFixture::new(1_000_000);
        let err = run_device_code_flow(
            &http(),
            &device_flow(&stub),
            &fixture.clock(false, false),
            &NeverCancel,
            |_| {},
        )
        .await
        .expect_err("terminal");
        if expect_expired {
            assert!(matches!(err, OAuthError::Expired { .. }), "{err}");
        } else {
            assert!(
                matches!(err, OAuthError::ReauthorizationRequired { .. }),
                "{err}"
            );
        }
        // Exactly one poll: a terminal answer is never retried.
        assert_eq!(stub.requests_to("/token").len(), 1);
        stub.stop().await;
    }
}

#[tokio::test]
async fn device_code_cancellation_wins_at_the_wait() {
    let stub = AuthStub::start(&[(
        "/device",
        vec![StubAnswer::json(
            200,
            serde_json::json!({
                "device_code": "dc-1",
                "user_code": "ABCD-EFGH",
                "verification_uri": "https://auth.example.test/activate",
                "expires_in": 900
            }),
        )],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    // Sleeps PARK — the cancellation must win the select against them.
    let cancel = NotifyCancel::new();
    let flow_config = device_flow(&stub);
    let clock = fixture.clock(false, true);
    let http = http();
    let run = run_device_code_flow(&http, &flow_config, &clock, cancel.as_ref(), |_| {});
    tokio::pin!(run);
    // Drive the flow to its first (parked) sleep, then cancel.
    tokio::select! {
        outcome = &mut run => panic!("the parked flow cannot complete: {outcome:?}"),
        _ = async {
            while fixture.recorded_sleeps().is_empty() {
                tokio::task::yield_now().await;
            }
        } => {}
    }
    cancel.cancel();
    let err = run.await.expect_err("cancelled");
    assert!(matches!(err, OAuthError::Cancelled), "{err}");
    // Cancellation happened BEFORE any poll left the machine.
    assert_eq!(stub.requests_to("/token").len(), 0);
    stub.stop().await;
}

#[tokio::test]
async fn device_code_endpoint_echoes_are_scrubbed_of_in_play_material() {
    let stub = AuthStub::start(&[
        (
            "/device",
            vec![StubAnswer::json(
                200,
                serde_json::json!({
                    "device_code": "dc-SECRET-device-code",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://auth.example.test/activate",
                    "expires_in": 900
                }),
            )],
        ),
        (
            "/token",
            vec![StubAnswer::json(
                400,
                serde_json::json!({
                    "error": "invalid_grant",
                    // The auth server echoes the in-play device code — the
                    // error that travels must be scrubbed (C09).
                    "error_description": "grant dc-SECRET-device-code rejected"
                }),
            )],
        ),
    ])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let err = run_device_code_flow(
        &http(),
        &device_flow(&stub),
        &fixture.clock(false, false),
        &NeverCancel,
        |_| {},
    )
    .await
    .expect_err("terminal");
    let text = err.to_string();
    assert!(!text.contains("dc-SECRET-device-code"), "{text}");
    assert!(text.contains("[redacted]"), "{text}");
    stub.stop().await;
}

#[tokio::test]
async fn device_code_untrusted_verification_uri_is_a_loud_protocol_failure() {
    let stub = AuthStub::start(&[(
        "/device",
        vec![StubAnswer::json(
            200,
            serde_json::json!({
                "device_code": "dc-1",
                "user_code": "ABCD-EFGH",
                // Plain http on a NON-loopback host: the user would type
                // the code into an attacker-observable page.
                "verification_uri": "http://evil.example.test/activate",
                "expires_in": 900
            }),
        )],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let err = run_device_code_flow(
        &http(),
        &device_flow(&stub),
        &fixture.clock(false, false),
        &NeverCancel,
        |_| {},
    )
    .await
    .expect_err("refused");
    assert!(matches!(err, OAuthError::Protocol { .. }), "{err}");
    assert_eq!(stub.requests_to("/token").len(), 0);
    stub.stop().await;
}

// ── refresh grant (shared) ──────────────────────────────────────────────────

#[tokio::test]
async fn refresh_happy_path_and_refresh_token_rollover_rule() {
    // A response WITHOUT a new refresh token keeps the previous one (the
    // incumbent's rule); with one, it rotates.
    let stub = AuthStub::start(&[(
        "/token",
        vec![
            StubAnswer::json(200, tokens_json("at-2", None, 3600)),
            StubAnswer::json(200, tokens_json("at-3", Some("rt-2"), 3600)),
        ],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let flow = device_flow(&stub);
    let first = refresh(&http(), &flow, "rt-1", &fixture.clock(false, false))
        .await
        .expect("refresh");
    assert_eq!(first.access_token, "at-2");
    assert_eq!(first.refresh_token, "rt-1", "no new refresh token → keep");
    let second = refresh(
        &http(),
        &flow,
        &first.refresh_token,
        &fixture.clock(false, false),
    )
    .await
    .expect("refresh");
    assert_eq!(second.refresh_token, "rt-2", "a new refresh token rotates");
    let polls = stub.requests_to("/token");
    assert_eq!(polls[0].fields["grant_type"], "refresh_token");
    assert_eq!(polls[0].fields["refresh_token"], "rt-1");
    stub.stop().await;
}

#[tokio::test]
async fn refresh_classification_invalid_grant_401_transient_and_scrub() {
    // invalid_grant → re-authorization (terminal); 401 → re-authorization;
    // 429/5xx → transient; an echo of the refresh token is scrubbed.
    let cases: Vec<(StubAnswer, &'static str)> = vec![
        (
            StubAnswer::json(
                400,
                serde_json::json!({"error": "invalid_grant", "error_description": "grant rt-SECRET died"}),
            ),
            "reauth",
        ),
        (
            StubAnswer::json(401, serde_json::json!({"error": "whatever"})),
            "reauth",
        ),
        (
            StubAnswer::json(429, serde_json::json!({"error": "slow_down"})),
            "transient",
        ),
        (StubAnswer::json(503, serde_json::json!({})), "transient"),
    ];
    for (answer, expectation) in cases {
        let stub = AuthStub::start(&[("/token", vec![answer])]).await;
        let fixture = ClockFixture::new(1_000_000);
        let err = refresh(
            &http(),
            &device_flow(&stub),
            "rt-SECRET",
            &fixture.clock(false, false),
        )
        .await
        .expect_err("fails");
        match expectation {
            "reauth" => assert!(
                matches!(err, OAuthError::ReauthorizationRequired { .. }),
                "{err}"
            ),
            _ => assert!(matches!(err, OAuthError::Transient { .. }), "{err}"),
        }
        let text = err.to_string();
        assert!(!text.contains("rt-SECRET"), "{text}");
        stub.stop().await;
    }
}

#[tokio::test]
async fn refresh_timeout_is_transient_never_terminal() {
    // The stub accepts and never answers; the request timeout bounds it.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let holder = tokio::spawn(async move {
        let _hold = listener.accept().await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let flow = OAuthFlowConfig {
        flow: OAuthFlowKind::DeviceCode,
        client_id: "client-test".to_string(),
        token_endpoint: format!("http://{addr}/token"),
        authorize_endpoint: None,
        device_authorization_endpoint: Some(format!("http://{addr}/device")),
        scopes: None,
    };
    let fixture = ClockFixture::new(1_000_000);
    let err = refresh(
        &http(),
        &flow,
        "rt-1",
        &fixture.clock_with_timeout(false, false, Duration::from_millis(100)),
    )
    .await
    .expect_err("timeout");
    assert!(matches!(err, OAuthError::Transient { .. }), "{err}");
    holder.abort();
}

#[tokio::test]
async fn token_endpoint_redirect_is_never_followed() {
    // C10: a 302 from the token endpoint is a loud protocol failure; the
    // redirect target receives NOTHING.
    let target = AuthStub::start(&[("/token", vec![])]).await;
    let stub = AuthStub::start(&[(
        "/token",
        vec![StubAnswer {
            status: 302,
            headers: vec![("Location".to_string(), target.url("/token"))],
            body: String::new(),
        }],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let err = refresh(
        &http(),
        &device_flow(&stub),
        "rt-1",
        &fixture.clock(false, false),
    )
    .await
    .expect_err("redirect refused");
    assert!(matches!(err, OAuthError::Protocol { .. }), "{err}");
    assert!(
        target.requests_to("/token").is_empty(),
        "the redirect target was never contacted"
    );
    stub.stop().await;
    target.stop().await;
}

/// REV-T02 R01 F-01, trigger 1 (mint-side pin): a sub-second `expires_in`
/// (e.g. 0.4) is a positive number the incumbent's seconds acceptance
/// truncates to 0 — the minted token is ALREADY expired at install time.
/// The loud bound (one refresh per resolve) lives in the credential
/// service; this pin records the honest mint-side fact: such a response
/// yields `expires_at == now`, never a silent extension.
#[tokio::test]
async fn sub_second_expires_in_mints_an_immediately_expired_token() {
    let stub = AuthStub::start(&[(
        "/token",
        vec![StubAnswer::json(
            200,
            serde_json::json!({
                "access_token": "at-truncated",
                "token_type": "Bearer",
                "refresh_token": "rt-1",
                "expires_in": 0.4
            }),
        )],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let flow = device_flow(&stub);
    let tokens = refresh(&http(), &flow, "rt-0", &fixture.clock(false, false))
        .await
        .expect("refresh");
    assert_eq!(
        tokens.expires_at_unix_ms, 1_000_000,
        "0.4s truncates to zero: expired at mint time"
    );
    stub.stop().await;
}

#[tokio::test]
async fn missing_expires_in_falls_back_to_jwt_exp_and_refuses_when_absent() {
    // JWT with exp = 1060 (seconds) → expiry 60s after the manual now
    // (1_000_000 ms).
    let jwt = {
        use base64::Engine as _;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"exp":1060}"#);
        format!("{header}.{payload}.")
    };
    let stub = AuthStub::start(&[(
        "/token",
        vec![
            StubAnswer::json(
                200,
                serde_json::json!({
                    "access_token": jwt,
                    "refresh_token": "rt-1"
                    // no expires_in
                }),
            ),
            StubAnswer::json(
                200,
                serde_json::json!({
                    "access_token": "opaque-no-exp-anywhere",
                    "refresh_token": "rt-1"
                }),
            ),
        ],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let flow = device_flow(&stub);
    let tokens = refresh(&http(), &flow, "rt-0", &fixture.clock(false, false))
        .await
        .expect("jwt fallback");
    assert_eq!(tokens.expires_at_unix_ms, 1_060_000);
    let err = refresh(&http(), &flow, "rt-0", &fixture.clock(false, false))
        .await
        .expect_err("no expiry anywhere is loud");
    assert!(matches!(err, OAuthError::Protocol { .. }), "{err}");
    stub.stop().await;
}

// ── authorization-code + PKCE flow (openai-codex type) ─────────────────────

/// Drives a real user-agent GET against the flow's loopback listener and
/// returns (status, body).
async fn drive_callback(addr: SocketAddr, target: &str) -> (u16, String) {
    let mut socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("callback connect");
    let request = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    socket
        .write_all(request.as_bytes())
        .await
        .expect("callback write");
    let mut raw = Vec::new();
    socket
        .read_to_end(&mut raw)
        .await
        .expect("callback response");
    let text = String::from_utf8(raw).expect("utf8 response");
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status code");
    let body = text
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    (status, body)
}

fn authorize_url_params(flow: &AuthorizationCodeFlow) -> BTreeMap<String, String> {
    let url = flow.authorize_url().to_string();
    let (_, query) = url.split_once('?').expect("authorize url query");
    query
        .split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').expect("param");
            (key.to_string(), percent_decode(value))
        })
        .collect()
}

#[tokio::test]
async fn pkce_happy_path_binds_state_and_verifier_to_the_exchange() {
    let stub = AuthStub::start(&[(
        "/token",
        vec![StubAnswer::json(
            200,
            tokens_json("at-1", Some("rt-1"), 3600),
        )],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let flow = AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false))
        .expect("flow begins");
    let params = authorize_url_params(&flow);
    // The authorize URL carries the incumbent's shape.
    assert_eq!(params["response_type"], "code");
    assert_eq!(params["client_id"], "client-test");
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["scope"], "openid");
    assert!(!params["state"].is_empty());
    assert!(!params["code_challenge"].is_empty());
    assert_eq!(
        params["redirect_uri"],
        format!("http://{}/callback", flow.callback_addr())
    );
    let state = params["state"].clone();
    let challenge = params["code_challenge"].clone();
    let addr = flow.callback_addr();
    let clock = fixture.clock(false, false);
    let wait = tokio::spawn(async move { flow.await_callback(&clock, &NeverCancel).await });
    tokio::task::yield_now().await;
    let (status, _body) =
        drive_callback(addr, &format!("/callback?state={state}&code=code-1")).await;
    assert_eq!(status, 200);
    let grant = wait.await.expect("joined").expect("callback accepted");
    // The exchange posts the code + the PKCE verifier whose S256 is the
    // challenge in the authorize URL.
    let tokens = AuthorizationCodeFlow::exchange(
        &http(),
        &pkce_flow(&stub),
        &grant,
        &fixture.clock(false, false),
    )
    .await
    .expect("exchange");
    assert_eq!(tokens.access_token, "at-1");
    let exchanges = stub.requests_to("/token");
    assert_eq!(exchanges.len(), 1);
    let posted = &exchanges[0].fields;
    assert_eq!(posted["grant_type"], "authorization_code");
    assert_eq!(posted["code"], "code-1");
    assert_eq!(posted["client_id"], "client-test");
    assert!(posted["redirect_uri"].starts_with("http://127.0.0.1:"));
    // S256(verifier) == challenge — the PKCE binding is real.
    use base64::Engine as _;
    use sha2::Digest as _;
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(posted["code_verifier"].as_bytes()));
    assert_eq!(computed, challenge);
    assert_ne!(
        posted["code_verifier"], challenge,
        "the verifier itself never travels in the URL"
    );
    stub.stop().await;
}

#[tokio::test]
async fn pkce_wrong_state_is_a_400_and_the_flow_survives_for_the_valid_one() {
    let stub = AuthStub::start(&[(
        "/token",
        vec![StubAnswer::json(
            200,
            tokens_json("at-1", Some("rt-1"), 3600),
        )],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let flow = AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false))
        .expect("flow begins");
    let state = authorize_url_params(&flow)["state"].clone();
    let addr = flow.callback_addr();
    let clock = fixture.clock(false, false);
    let wait = tokio::spawn(async move { flow.await_callback(&clock, &NeverCancel).await });
    tokio::task::yield_now().await;
    // A foreign-state callback: HTTP 400, NO exchange, the flow keeps waiting.
    let (status, _) = drive_callback(addr, "/callback?state=forged-state&code=code-evil").await;
    assert_eq!(status, 400);
    assert!(stub.requests_to("/token").is_empty());
    // An unrelated path on the same port: 404, still waiting.
    let (status, _) = drive_callback(addr, "/not-the-callback").await;
    assert_eq!(status, 404);
    // The valid callback still succeeds.
    let (status, _) = drive_callback(addr, &format!("/callback?state={state}&code=code-1")).await;
    assert_eq!(status, 200);
    let grant = wait.await.expect("joined").expect("accepted");
    let _ = AuthorizationCodeFlow::exchange(
        &http(),
        &pkce_flow(&stub),
        &grant,
        &fixture.clock(false, false),
    )
    .await
    .expect("exchange");
    assert_eq!(stub.requests_to("/token").len(), 1, "exactly one exchange");
    stub.stop().await;
}

#[tokio::test]
async fn pkce_expired_state_is_rejected_at_the_loop_top_and_after_accept() {
    let stub = AuthStub::start(&[]).await;
    let fixture = ClockFixture::new(1_000_000);
    // Leg 1: deadline passed BEFORE any callback — the flow expires without
    // ever accepting (the listener dies with it).
    let flow = AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false))
        .expect("flow begins");
    let addr = flow.callback_addr();
    fixture.advance(OAUTH_STATE_TTL.as_millis() as u64 + 1);
    let err = flow
        .await_callback(&fixture.clock(false, false), &NeverCancel)
        .await
        .expect_err("expired");
    assert!(matches!(err, OAuthError::Expired { .. }), "{err}");
    assert!(
        tokio::net::TcpStream::connect(addr).await.is_err(),
        "the listener is gone with the expired flow"
    );

    // Leg 2: the callback sat in the backlog past the TTL — the post-accept
    // re-check answers 400 and expires. Deterministic staging: connect,
    // send only a PARTIAL head (the flow accepts and parks in the read),
    // advance the clock past the TTL, then complete the request.
    let flow = AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false))
        .expect("flow begins");
    let state = authorize_url_params(&flow)["state"].clone();
    let addr = flow.callback_addr();
    let clock = fixture.clock(false, false);
    let wait = tokio::spawn(async move { flow.await_callback(&clock, &NeverCancel).await });
    tokio::task::yield_now().await;
    let mut socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
    socket
        .write_all(
            format!("GET /callback?state={state}&code=code-1 HTTP/1.1\r\nHost: 127.0.0.1\r\n")
                .as_bytes(),
        )
        .await
        .expect("partial write");
    // Let the flow accept + consume the partial head, then expire the state.
    tokio::time::sleep(Duration::from_millis(50)).await;
    fixture.advance(OAUTH_STATE_TTL.as_millis() as u64 + 1);
    socket
        .write_all(b"Connection: close\r\n\r\n")
        .await
        .expect("complete write");
    let mut raw = Vec::new();
    socket.read_to_end(&mut raw).await.expect("response");
    let text = String::from_utf8(raw).expect("utf8");
    assert!(text.starts_with("HTTP/1.1 400"), "{text}");
    let err = wait.await.expect("joined").expect_err("expired");
    assert!(matches!(err, OAuthError::Expired { .. }), "{err}");
    assert!(
        stub.requests_to("/token").is_empty(),
        "no exchange ever ran"
    );
    stub.stop().await;
}

#[tokio::test]
async fn pkce_error_callback_and_codeless_callback_are_terminal_and_loud() {
    let stub = AuthStub::start(&[]).await;
    let fixture = ClockFixture::new(1_000_000);
    // error=access_denied with the RIGHT state → ReauthorizationRequired.
    let flow = AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false))
        .expect("flow begins");
    let state = authorize_url_params(&flow)["state"].clone();
    let addr = flow.callback_addr();
    let clock = fixture.clock(false, false);
    let wait = tokio::spawn(async move { flow.await_callback(&clock, &NeverCancel).await });
    tokio::task::yield_now().await;
    let (status, _) = drive_callback(
        addr,
        &format!("/callback?state={state}&error=access_denied"),
    )
    .await;
    assert_eq!(status, 400);
    let err = wait.await.expect("joined").expect_err("denied");
    assert!(
        matches!(err, OAuthError::ReauthorizationRequired { .. }),
        "{err}"
    );
    // A valid-state callback WITHOUT a code is a protocol violation.
    let flow = AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false))
        .expect("flow begins");
    let state = authorize_url_params(&flow)["state"].clone();
    let addr = flow.callback_addr();
    let clock = fixture.clock(false, false);
    let wait = tokio::spawn(async move { flow.await_callback(&clock, &NeverCancel).await });
    tokio::task::yield_now().await;
    let (status, _) = drive_callback(addr, &format!("/callback?state={state}")).await;
    assert_eq!(status, 400);
    let err = wait.await.expect("joined").expect_err("malformed");
    assert!(matches!(err, OAuthError::Protocol { .. }), "{err}");
    assert!(stub.requests_to("/token").is_empty());
    stub.stop().await;
}

#[tokio::test]
async fn pkce_duplicate_callback_is_refused_by_the_closed_listener() {
    let stub = AuthStub::start(&[(
        "/token",
        vec![StubAnswer::json(
            200,
            tokens_json("at-1", Some("rt-1"), 3600),
        )],
    )])
    .await;
    let fixture = ClockFixture::new(1_000_000);
    let flow = AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false))
        .expect("flow begins");
    let state = authorize_url_params(&flow)["state"].clone();
    let addr = flow.callback_addr();
    let clock = fixture.clock(false, false);
    let wait = tokio::spawn(async move { flow.await_callback(&clock, &NeverCancel).await });
    tokio::task::yield_now().await;
    let (status, _) = drive_callback(addr, &format!("/callback?state={state}&code=code-1")).await;
    assert_eq!(status, 200);
    let _grant = wait.await.expect("joined").expect("accepted");
    // The listener closed with the consumed flow: a duplicate callback
    // cannot even connect.
    assert!(
        tokio::net::TcpStream::connect(addr).await.is_err(),
        "a duplicate callback is refused — the listener is closed"
    );
    stub.stop().await;
}

#[tokio::test]
async fn pkce_cancellation_while_waiting_is_clean() {
    let stub = AuthStub::start(&[]).await;
    let fixture = ClockFixture::new(1_000_000);
    let flow = AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false))
        .expect("flow begins");
    let addr = flow.callback_addr();
    let cancel = NotifyCancel::new();
    let clock = fixture.clock(false, false);
    let wait = tokio::spawn({
        let cancel = Arc::clone(&cancel);
        async move { flow.await_callback(&clock, cancel.as_ref()).await }
    });
    tokio::task::yield_now().await;
    cancel.cancel();
    let err = wait.await.expect("joined").expect_err("cancelled");
    assert!(matches!(err, OAuthError::Cancelled), "{err}");
    assert!(
        tokio::net::TcpStream::connect(addr).await.is_err(),
        "the listener is gone with the cancelled flow"
    );
    stub.stop().await;
}

#[tokio::test]
async fn pkce_each_begin_mints_fresh_state_and_verifier() {
    let stub = AuthStub::start(&[]).await;
    let fixture = ClockFixture::new(1_000_000);
    let one =
        AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false)).expect("one");
    let two =
        AuthorizationCodeFlow::begin(&pkce_flow(&stub), &fixture.clock(false, false)).expect("two");
    let one = authorize_url_params(&one);
    let two = authorize_url_params(&two);
    assert_ne!(one["state"], two["state"]);
    assert_ne!(one["code_challenge"], two["code_challenge"]);
    stub.stop().await;
}

// ── F-02 (REVIEW-T05 R01): ambient proxies never see credential material ────

/// An env-var guard: restores the touched proxy variables on drop, so even a
/// failed assertion never leaks a poisoned environment into sibling tests.
struct ProxyEnvGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl ProxyEnvGuard {
    /// Points HTTP_PROXY at `proxy` and clears every other variable the
    /// reqwest/hyper-util matcher consults (HTTP(S)_PROXY / ALL_PROXY, both
    /// cases, plus the NO_PROXY exclusion list) — the counting listener is
    /// then the ONLY ambient-proxy influence a wrongly-built client could
    /// capture.
    fn point_at(proxy: &str) -> Self {
        const VARS: [&str; 8] = [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
        ];
        let saved = VARS
            .iter()
            .map(|var| (*var, std::env::var_os(var)))
            .collect::<Vec<_>>();
        for (var, _) in &saved {
            std::env::remove_var(var);
        }
        std::env::set_var("HTTP_PROXY", proxy);
        Self { saved }
    }
}

impl Drop for ProxyEnvGuard {
    fn drop(&mut self) {
        for (var, value) in self.saved.drain(..) {
            match value {
                Some(value) => std::env::set_var(var, value),
                None => std::env::remove_var(var),
            }
        }
    }
}

#[tokio::test]
async fn the_credential_bearing_client_never_consults_the_ambient_proxy() {
    // F-02 (REVIEW-T05 R01): the token/refresh requests this client sends
    // carry client_secret / refresh_token. reqwest's default `system-proxy`
    // feature reads HTTP(S)_PROXY (and the OS proxy store) into every client
    // built without `no_proxy()` — routing these through an ambient proxy is
    // a behavior regression against the incumbent (the pi-sdk OAuth flows
    // use bare fetch, proven by the reviewer's node-fetch-proxy-probe: zero
    // proxy hits) and a silent credential exposure. The matcher is captured
    // at BUILD time (reqwest client.rs `auto_sys_proxy`), so the discipline
    // is: poison the env → build the client → restore the env → drive the
    // flow.
    let stub = AuthStub::start(&[(
        "/token",
        vec![StubAnswer::json(
            200,
            tokens_json("at-1", Some("rt-1"), 3600),
        )],
    )])
    .await;
    // The "ambient proxy": a bare counting listener — ANY connection to it
    // fails the test, whether or not it could answer.
    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy bind");
    let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
    let proxy_hits = Arc::new(AtomicU64::new(0));
    let proxy_task = {
        let hits = Arc::clone(&proxy_hits);
        tokio::spawn(async move {
            while let Ok((socket, _)) = proxy_listener.accept().await {
                hits.fetch_add(1, Ordering::SeqCst);
                drop(socket);
            }
        })
    };
    let http = {
        let _guard = ProxyEnvGuard::point_at(&format!("http://{proxy_addr}"));
        http() // the proxy matcher is read HERE, under the poisoned env
    };
    let tokens = refresh(
        &http,
        &pkce_flow(&stub),
        "rt-0",
        &system_flow_clock(Duration::from_secs(5)),
    )
    .await
    .expect("refresh answers");
    assert_eq!(tokens.access_token, "at-1");
    let exchanges = stub.requests_to("/token");
    assert_eq!(
        exchanges.len(),
        1,
        "the credential-bearing request went DIRECT to the token endpoint"
    );
    assert_eq!(exchanges[0].fields["refresh_token"], "rt-0");
    assert_eq!(
        proxy_hits.load(Ordering::SeqCst),
        0,
        "the ambient proxy was never touched"
    );
    proxy_task.abort();
    stub.stop().await;
}
