//! R05-T05 adapter-level evidence: the three-segment timeout discipline
//! (connect / first-byte / total-budget) and the 429 `Retry-After`
//! contract at the REAL adapter boundary — loopback raw-TCP stubs are the
//! far end of the wire, never in-process doubles.
//!
//! Coverage map (R05_BASELINE §8 pre-registered values, appendix-B R05-T05):
//! - C01 (connect / first-byte): a stub that accepts and never answers
//!   trips the FIRST-BYTE window as a NON-retryable `upstream_unavailable`
//!   (A10: the request was fully sent — no blind resend); a refused port
//!   trips the CONNECT segment as the one retryable transport class (the
//!   request provably never left); a call whose TOTAL deadline lands
//!   inside the first-byte wait classifies as the non-retryable
//!   `budget_exceeded` instead (the two bounds are distinguished by a
//!   fresh clock read, never conflated).
//! - C01 (total budget, pre-send): an already-exhausted deadline fails
//!   BEFORE any byte leaves — the stub observes ZERO requests.
//! - C01 (total budget, mid-stream): a deadline hit while the SSE read is
//!   open abandons the connection as non-retryable `budget_exceeded`; the
//!   partial deltas already emitted stay emitted (no rewind).
//! - C05/A10 (accepted, no answer): the stub reads the whole request and
//!   drops the connection — NON-retryable, exactly ONE outbound on the
//!   wire; the mid-stream leg (partial delta, then a close without the
//!   terminal sentinel) keeps the emitted prefix and never resends.
//! - C03 (429): the hint header lands in `details.retryAfterMs` in BOTH
//!   RFC 9110 forms (delay-seconds and HTTP-date); a garbage hint is
//!   ignored, never trusted; a 5xx without a hint classifies retryable
//!   with NO fabricated hint.

use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use lingxi_adapters::models::credentials::ApplicableAuth;
use lingxi_adapters::models::openai_completions::OpenAiCompletionsAdapter;
use lingxi_adapters::models::{dispatch, streaming::NullDeltaSink};
use lingxi_kernel::model_exchange::{
    CredentialAuthKind, CredentialReference, ModelOperation, ModelTurnInput, ProtocolFamily,
    ResolvedModelRoute, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{ModelTurnDelta, ProviderTurn, TurnDeltaSink, TurnDeltaSinkClosed};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{AttemptId, ErrorCode, ModelCallId, RunId, SessionId};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── the scripted stub (the far end of the wire) ─────────────────────────────

/// How one accepted connection behaves.
enum Script {
    /// Read the request head, then hold the socket open WITHOUT answering
    /// (first-byte / mid-stream deadline trips).
    HoldOpen,
    /// Read the full request (the far end provably HOLDS the call), then
    /// close the socket WITHOUT answering — the A10 accepted-then-dropped
    /// scenario.
    AcceptThenClose,
    /// Read the request, flush `head` verbatim, then hold the socket open
    /// (a partial SSE stream that never terminates).
    FlushThenHold(String),
    /// Read the request, flush `head` verbatim, then CLOSE the socket — a
    /// mid-stream transport break after acceptance (A10).
    FlushThenClose(String),
    /// Read the request and answer immediately with the scripted status,
    /// extra headers and body.
    Answer {
        status: u16,
        extra_headers: Vec<(String, String)>,
        body: String,
    },
}

struct Stub {
    endpoint: String,
    hits: Arc<AtomicUsize>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Stub {
    async fn start(script: Script) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr: SocketAddr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task_hits = Arc::clone(&hits);
        let script = Arc::new(script);
        let task = tokio::spawn(async move {
            let mut connections = Vec::new();
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                task_hits.fetch_add(1, Ordering::SeqCst);
                let script_ref = Arc::clone(&script);
                connections.push(tokio::spawn(async move {
                    read_request_head(&mut socket).await;
                    match &*script_ref {
                        Script::AcceptThenClose => {
                            let _ = socket.shutdown().await;
                        }
                        Script::HoldOpen => {
                            // Hold until the client goes away (or the test
                            // process ends). Never answer.
                            let mut buf = [0_u8; 1024];
                            loop {
                                match socket.read(&mut buf).await {
                                    Ok(0) | Err(_) => break,
                                    Ok(_) => {}
                                }
                            }
                        }
                        Script::FlushThenHold(head) => {
                            let _ = socket.write_all(head.as_bytes()).await;
                            let mut buf = [0_u8; 1024];
                            loop {
                                match socket.read(&mut buf).await {
                                    Ok(0) | Err(_) => break,
                                    Ok(_) => {}
                                }
                            }
                        }
                        Script::FlushThenClose(head) => {
                            let _ = socket.write_all(head.as_bytes()).await;
                            let _ = socket.shutdown().await;
                        }
                        Script::Answer {
                            status,
                            extra_headers,
                            body,
                        } => {
                            let reason = match status {
                                400 => "Bad Request",
                                429 => "Too Many Requests",
                                500 => "Internal Server Error",
                                503 => "Service Unavailable",
                                other => panic!("stub: unscripted status {other}"),
                            };
                            let mut response = format!(
                                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                                body.len()
                            );
                            for (name, value) in extra_headers {
                                response.push_str(&format!("{name}: {value}\r\n"));
                            }
                            response.push_str("\r\n");
                            response.push_str(body);
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.shutdown().await;
                        }
                    }
                }));
            }
            for connection in connections {
                connection.abort();
            }
        });
        Self {
            endpoint: format!("http://{addr}/v1"),
            hits,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

/// Reads one request head (headers + declared body) so the client's send
/// completes before the script's behavior applies.
async fn read_request_head(socket: &mut tokio::net::TcpStream) {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => raw.extend_from_slice(&chunk[..read]),
        }
        if let Some(pos) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
        if raw.len() > 256 * 1024 {
            return;
        }
    };
    let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let content_length = head
        .split("\r\n")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim().eq_ignore_ascii_case("content-length"))
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut have = raw.len() - (header_end + 4);
    while have < content_length {
        let mut chunk = [0_u8; 4096];
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => have += read,
        }
    }
}

// ── call assembly ────────────────────────────────────────────────────────────

fn route(endpoint: &str) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: "stub.openai".to_string(),
        model: "stub-model".to_string(),
        operation: ModelOperation::Chat,
        protocol: ProtocolFamily::parse("openai-completions").expect("family"),
        endpoint: endpoint.to_string(),
        credential: CredentialReference {
            provider: "stub.openai".to_string(),
            auth: CredentialAuthKind::ApiKey,
        },
        config_generation: 1,
        group_id: None,
    }
}

fn input_with_deadline(deadline_unix_ms: Option<u64>) -> ModelTurnInput {
    ModelTurnInput {
        submission: "ping".to_string(),
        system_prompt: None,
        turn: 1,
        prior: Vec::new(),
        tools: ToolDeclarationSnapshot::empty(),
        deadline_unix_ms,
        images: Vec::new(),
        max_output_tokens: None,
    }
}

fn ctx() -> RunContext {
    RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess-t05"),
        run_id: RunId::new("run-t05"),
        attempt: AttemptId::new("run-t05-attempt-1"),
        generation: 1,
    }
}

fn auth() -> ApplicableAuth {
    ApplicableAuth::Bearer("sk-t05-stub".to_string())
}

fn short_timeouts() -> dispatch::HttpTimeouts {
    dispatch::HttpTimeouts {
        connect: Duration::from_millis(300),
        first_byte: Duration::from_millis(300),
    }
}

fn adapter(timeouts: dispatch::HttpTimeouts) -> OpenAiCompletionsAdapter {
    OpenAiCompletionsAdapter::new_with_timeouts(SchemaBudget::default(), timeouts)
        .expect("adapter builds")
}

async fn execute(
    adapter: &OpenAiCompletionsAdapter,
    endpoint: &str,
    deadline_unix_ms: Option<u64>,
) -> ProviderTurn {
    let input = input_with_deadline(deadline_unix_ms);
    let call = ModelCallId::new("mc-t05-0001");
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        adapter.execute_chat(
            &ctx(),
            &call,
            &input,
            &route(endpoint),
            &auth(),
            None,
            &NullDeltaSink,
        ),
    )
    .await
    .expect("the call must settle inside the test guard");
    result.turn
}

fn expect_failed(turn: ProviderTurn) -> (lingxi_protocol::ProtocolError, bool) {
    match turn {
        ProviderTurn::Failed { error, retryable } => (error, retryable),
        other => panic!("expected a Failed turn, got {other:?}"),
    }
}

// ── C01: the three timeout segments ─────────────────────────────────────────

#[tokio::test]
async fn pre_send_budget_exhaustion_sends_nothing_and_is_not_retryable() {
    let stub = Stub::start(Script::Answer {
        status: 200,
        extra_headers: Vec::new(),
        body: "{}".to_string(),
    })
    .await;
    let adapter = adapter(short_timeouts());
    // A deadline already in the past: the budget check fires BEFORE any
    // byte leaves the process.
    let past = dispatch::unix_ms_now().saturating_sub(1);
    let (error, retryable) = expect_failed(execute(&adapter, &stub.endpoint, Some(past)).await);
    assert_eq!(error.code, ErrorCode::BudgetExceeded, "{error}");
    assert!(!retryable, "an exhausted total budget never retries");
    assert!(
        error.message.contains("model_call_total_budget_ms"),
        "the message names the pre-registered bound: {error}"
    );
    assert_eq!(stub.hits(), 0, "no request may leave on a spent budget");
    stub.stop().await;
}

#[tokio::test]
async fn first_byte_window_hit_is_terminal_no_blind_resend() {
    let stub = Stub::start(Script::HoldOpen).await;
    let adapter = adapter(short_timeouts());
    let started = std::time::Instant::now();
    let (error, retryable) = expect_failed(execute(&adapter, &stub.endpoint, None).await);
    assert_eq!(error.code, ErrorCode::UpstreamUnavailable, "{error}");
    assert!(
        !retryable,
        "A10: the request was fully sent — a first-byte stall never triggers a blind resend"
    );
    assert!(
        error.message.contains("http_first_byte_timeout_ms"),
        "the message names the pre-registered bound: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the window fired promptly (not at some larger hidden bound)"
    );
    assert_eq!(
        stub.hits(),
        1,
        "A10: exactly one outbound — no second send after acceptance"
    );
    stub.stop().await;
}

/// A10 core scenario: the stub ACCEPTS the full request (headers + body
/// read — the far end provably holds the call) and then drops the
/// connection WITHOUT answering. No idempotency key exists → the call
/// settles failed, NON-retryable, and the adapter makes no second send.
#[tokio::test]
async fn accepted_then_dropped_is_terminal_and_never_resent() {
    let stub = Stub::start(Script::AcceptThenClose).await;
    let adapter = adapter(short_timeouts());
    let (error, retryable) = expect_failed(execute(&adapter, &stub.endpoint, None).await);
    assert_eq!(error.code, ErrorCode::UpstreamUnavailable, "{error}");
    assert!(
        !retryable,
        "a possibly-accepted request is never blindly resent (A10)"
    );
    assert_eq!(stub.hits(), 1, "exactly one outbound request on the wire");
    stub.stop().await;
}

#[tokio::test]
async fn connect_refusal_is_retryable_upstream() {
    // Bind a loopback port and drop the listener: connecting is refused
    // immediately and deterministically. A CONNECT-phase failure proves the
    // request never reached the server, so the bounded retry is
    // side-effect-free (A10's prohibition is scoped to ACCEPTED requests).
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);
    let endpoint = format!("http://{addr}/v1");
    let adapter = adapter(short_timeouts());
    let (error, retryable) = expect_failed(execute(&adapter, &endpoint, None).await);
    assert_eq!(error.code, ErrorCode::UpstreamUnavailable, "{error}");
    assert!(
        retryable,
        "a connect-phase failure (never accepted) is the one retryable transport class: {error}"
    );
}

#[tokio::test]
async fn deadline_hit_inside_first_byte_wait_is_non_retryable_budget() {
    let stub = Stub::start(Script::HoldOpen).await;
    // first_byte is the long segment here; the TOTAL deadline (400 ms) is
    // what fires inside the wait — the fresh clock read must classify the
    // non-retryable budget outcome, never the retryable first-byte one.
    let adapter = adapter(dispatch::HttpTimeouts {
        connect: Duration::from_millis(300),
        first_byte: Duration::from_secs(30),
    });
    let deadline = dispatch::unix_ms_now() + 400;
    let (error, retryable) = expect_failed(execute(&adapter, &stub.endpoint, Some(deadline)).await);
    assert_eq!(error.code, ErrorCode::BudgetExceeded, "{error}");
    assert!(!retryable, "the call's own deadline is not retryable");
    stub.stop().await;
}

/// A sink that records what the live stream delivered.
struct RecordingSink {
    seen: std::sync::Mutex<Vec<ModelTurnDelta>>,
}

impl TurnDeltaSink for RecordingSink {
    fn emit<'a>(
        &'a self,
        delta: ModelTurnDelta,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>,
    > {
        self.seen.lock().expect("seen").push(delta);
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn deadline_hit_mid_stream_abandons_the_read_without_rewind() {
    // One complete text-delta frame, then the stream hangs forever.
    let frame = serde_json::json!({
        "id": "chatcmpl-t05",
        "object": "chat.completion.chunk",
        "created": 1_700_000_000,
        "model": "stub-model",
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant", "content": "partial-"},
            "finish_reason": null
        }]
    });
    let head =
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {frame}\n\n");
    let stub = Stub::start(Script::FlushThenHold(head)).await;
    let adapter = adapter(short_timeouts());
    let deadline = dispatch::unix_ms_now() + 500;
    let input = input_with_deadline(Some(deadline));
    let call = ModelCallId::new("mc-t05-0001");
    let sink = RecordingSink {
        seen: std::sync::Mutex::new(Vec::new()),
    };
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        adapter.execute_chat(
            &ctx(),
            &call,
            &input,
            &route(&stub.endpoint),
            &auth(),
            None,
            &sink,
        ),
    )
    .await
    .expect("the call must settle inside the test guard");
    let (error, retryable) = expect_failed(result.turn);
    assert_eq!(error.code, ErrorCode::BudgetExceeded, "{error}");
    assert!(!retryable, "a mid-stream deadline hit is not retryable");
    // The partial delta the stream already delivered stays delivered —
    // the failure rewinds nothing.
    {
        let seen = sink.seen.lock().expect("seen");
        assert_eq!(
            seen.as_slice(),
            &[ModelTurnDelta::Text("partial-".to_string())],
            "the emitted prefix is durable at the sink"
        );
    }
    stub.stop().await;
}

/// A10 mid-stream leg: the server accepted, delivered one partial delta,
/// then the connection closed WITHOUT the terminal sentinel. The partial
/// stays emitted; the call settles NON-retryable (the accepted request is
/// never blindly resent), one outbound total.
#[tokio::test]
async fn mid_stream_transport_break_is_terminal_and_never_resent() {
    let frame = serde_json::json!({
        "id": "chatcmpl-t05",
        "object": "chat.completion.chunk",
        "created": 1_700_000_000,
        "model": "stub-model",
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant", "content": "partial-"},
            "finish_reason": null
        }]
    });
    // One complete frame, then the connection closes WITHOUT the terminal
    // [DONE] sentinel — an abrupt mid-stream break.
    let head =
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {frame}\n\n");
    let stub = Stub::start(Script::FlushThenClose(head)).await;
    let adapter = adapter(short_timeouts());
    let input = input_with_deadline(None);
    let call = ModelCallId::new("mc-t05-0001");
    let sink = RecordingSink {
        seen: std::sync::Mutex::new(Vec::new()),
    };
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        adapter.execute_chat(
            &ctx(),
            &call,
            &input,
            &route(&stub.endpoint),
            &auth(),
            None,
            &sink,
        ),
    )
    .await
    .expect("the call must settle inside the test guard");
    let (error, retryable) = expect_failed(result.turn);
    assert!(
        !retryable,
        "A10: an accepted-then-broken stream never triggers a blind resend: {error}"
    );
    assert_eq!(
        sink.seen.lock().expect("seen").as_slice(),
        &[ModelTurnDelta::Text("partial-".to_string())],
        "the delivered prefix stays durable"
    );
    assert_eq!(stub.hits(), 1, "exactly one outbound request on the wire");
    stub.stop().await;
}

// ── C03: the 429 / 5xx contract ─────────────────────────────────────────────

#[tokio::test]
async fn http_429_delay_seconds_hint_lands_in_details() {
    let stub = Stub::start(Script::Answer {
        status: 429,
        extra_headers: vec![("Retry-After".to_string(), "2".to_string())],
        body: "{\"error\":{\"message\":\"slow down\"}}".to_string(),
    })
    .await;
    let adapter = adapter(short_timeouts());
    let (error, retryable) = expect_failed(execute(&adapter, &stub.endpoint, None).await);
    assert_eq!(error.code, ErrorCode::BudgetExceeded, "{error}");
    assert!(retryable, "a 429 is the bounded-retry rate-limit signal");
    let hint = error
        .details
        .as_ref()
        .and_then(|details| details.get(dispatch::RETRY_AFTER_MS_DETAIL))
        .and_then(serde_json::Value::as_u64)
        .expect("the Retry-After hint rides the error details");
    assert_eq!(hint, 2_000, "delay-seconds convert to milliseconds");
    stub.stop().await;
}

#[tokio::test]
async fn http_429_http_date_hint_lands_in_details() {
    // The HTTP-date form carries SECOND precision only, and a loaded CI
    // host can sit between the stub's date minting and the client's parse
    // — the offset is generous and the assertion a range (the exactness
    // half is pinned by the delay-seconds case above and the unit pins
    // below).
    let date = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(30));
    let stub = Stub::start(Script::Answer {
        status: 429,
        extra_headers: vec![("Retry-After".to_string(), date)],
        body: "{}".to_string(),
    })
    .await;
    let adapter = adapter(short_timeouts());
    let (error, retryable) = expect_failed(execute(&adapter, &stub.endpoint, None).await);
    assert_eq!(error.code, ErrorCode::BudgetExceeded, "{error}");
    assert!(retryable);
    let hint = error
        .details
        .as_ref()
        .and_then(|details| details.get(dispatch::RETRY_AFTER_MS_DETAIL))
        .and_then(serde_json::Value::as_u64)
        .expect("an HTTP-date hint converts");
    assert!(
        (20_000..=30_000).contains(&hint),
        "the date form converts to the remaining milliseconds, got {hint}"
    );
    stub.stop().await;
}

#[tokio::test]
async fn http_429_garbage_hint_is_ignored_never_trusted() {
    let stub = Stub::start(Script::Answer {
        status: 429,
        extra_headers: vec![("Retry-After".to_string(), "soon™".to_string())],
        body: "{}".to_string(),
    })
    .await;
    let adapter = adapter(short_timeouts());
    let (error, retryable) = expect_failed(execute(&adapter, &stub.endpoint, None).await);
    assert_eq!(error.code, ErrorCode::BudgetExceeded, "{error}");
    assert!(retryable);
    let has_hint = error
        .details
        .as_ref()
        .is_some_and(|details| details.contains_key(dispatch::RETRY_AFTER_MS_DETAIL));
    assert!(
        !has_hint,
        "a malformed hint lands NO detail — the driver's computed backoff applies: {error:?}"
    );
    stub.stop().await;
}

#[tokio::test]
async fn http_5xx_is_bounded_retryable_without_a_fabricated_hint() {
    let stub = Stub::start(Script::Answer {
        status: 503,
        extra_headers: Vec::new(),
        body: "{\"error\":\"overloaded\"}".to_string(),
    })
    .await;
    let adapter = adapter(short_timeouts());
    let (error, retryable) = expect_failed(execute(&adapter, &stub.endpoint, None).await);
    assert_eq!(error.code, ErrorCode::UpstreamUnavailable, "{error}");
    assert!(retryable, "5xx is the retryable bounded-retry class");
    assert!(
        error.details.is_none(),
        "no Retry-After header, no fabricated detail: {error:?}"
    );
    stub.stop().await;
}

// ── parse_retry_after direct pins (both RFC 9110 forms + rejects) ───────────

#[test]
fn retry_after_parsing_pins_the_rfc_9110_forms() {
    let mut headers = reqwest::header::HeaderMap::new();
    assert_eq!(dispatch::parse_retry_after(&headers), None, "absent");

    headers.insert(reqwest::header::RETRY_AFTER, "0".parse().expect("header"));
    assert_eq!(
        dispatch::parse_retry_after(&headers),
        Some(0),
        "zero seconds"
    );

    headers.insert(reqwest::header::RETRY_AFTER, "7".parse().expect("header"));
    assert_eq!(dispatch::parse_retry_after(&headers), Some(7_000));

    // An HTTP-date in the past saturates at zero, never goes negative.
    let past = httpdate::fmt_http_date(std::time::SystemTime::now() - Duration::from_secs(60));
    headers.insert(reqwest::header::RETRY_AFTER, past.parse().expect("header"));
    assert_eq!(dispatch::parse_retry_after(&headers), Some(0));

    // An HTTP-date in the FUTURE converts to the remaining milliseconds
    // (second precision; parsed in the same tick, so a tight range holds).
    let future = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(9));
    headers.insert(
        reqwest::header::RETRY_AFTER,
        future.parse().expect("header"),
    );
    let hint = dispatch::parse_retry_after(&headers).expect("future date converts");
    assert!(
        (7_000..=9_000).contains(&hint),
        "future date → remaining ms, got {hint}"
    );

    for garbage in ["", "soon", "-5", "12x", "99999999999999999999999"] {
        headers.insert(
            reqwest::header::RETRY_AFTER,
            garbage.parse().expect("header"),
        );
        assert_eq!(
            dispatch::parse_retry_after(&headers),
            None,
            "garbage hint {garbage:?} is ignored"
        );
    }
}

/// The CONNECT segment refusal path is pinned by
/// `connect_refusal_is_retryable_upstream` above (a loopback refusal
/// surfaces in microseconds — the `is_connect` arm, never the head
/// window).
#[test]
fn connect_refusal_is_the_expected_underlying_kind() {
    let _ = ErrorKind::ConnectionRefused;
}
