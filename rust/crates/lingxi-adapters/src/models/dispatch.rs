//! The shared HTTP dispatch half of the chat protocol adapters (R05-T03):
//! every family dispatches ONE POST with the SAME transport discipline —
//! a no-redirect client (C10: a credential-bearing request is never
//! replayed onto a redirected origin), the incumbent status→error
//! classification (scrubbed of the in-play material, C09), and the
//! family-specific auth-header mapping.
//!
//! R05-T05 (network discipline, R05_BASELINE §8 pre-registered values):
//! - CONNECT timeout (10 s) on the shared client builder — a stalled
//!   handshake surfaces as a retryable upstream failure, never an unbounded
//!   park.
//! - FIRST-BYTE timeout (30 s) around `send()` ([`send_with_timeouts`]) —
//!   the response headers must arrive within the window (the send future
//!   spans connect, so this also backs the connect segment).
//! - TOTAL per-call budget ([`ModelTurnInput::deadline_unix_ms`]) — checked
//!   before send and wrapped around the stream drive
//!   ([`drive_sse_stream_within_budget`]). A deadline hit is a NON-retryable
//!   [`ErrorCode::BudgetExceeded`]: the call's registered budget
//!   (queue wait + credential refresh + backoff + network + streaming) is
//!   spent, so another attempt under it would start already exhausted.
//! - 429 `Retry-After` travels with the classified failure
//!   ([`RETRY_AFTER_MS_DETAIL`]) — the run driver's bounded backoff honors
//!   it. A 429 is NEVER silently absorbed into a global throttle here: it
//!   surfaces as an explicit retryable failure and the driver's attempt
//!   policy decides.

use lingxi_protocol::{ErrorCode, ProtocolError};

use super::credentials::{scrub_materials, ApplicableAuth};

/// R05_BASELINE §8 `http_connect_timeout_ms`.
pub const HTTP_CONNECT_TIMEOUT_MS: u64 = 10_000;
/// R05_BASELINE §8 `http_first_byte_timeout_ms` (response headers must
/// arrive within this window once the request is sent).
pub const HTTP_FIRST_BYTE_TIMEOUT_MS: u64 = 30_000;

/// The `ProtocolError.details` key carrying the provider's `Retry-After`
/// hint (milliseconds, u64) on a 429 response. The run driver's bounded
/// retry backoff honors it; the value is transport metadata (a header the
/// provider sent), never payload, so it is safe to carry on the error.
pub const RETRY_AFTER_MS_DETAIL: &str = "retryAfterMs";

/// The two adapter-side timeout segments (R05-T05). The default pair is the
/// pre-registered R05_BASELINE §8 values; tests inject shorter windows
/// through [`super::provider::GatewayedProvider::with_http_timeouts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpTimeouts {
    pub connect: std::time::Duration,
    pub first_byte: std::time::Duration,
}

impl Default for HttpTimeouts {
    fn default() -> Self {
        Self {
            connect: std::time::Duration::from_millis(HTTP_CONNECT_TIMEOUT_MS),
            first_byte: std::time::Duration::from_millis(HTTP_FIRST_BYTE_TIMEOUT_MS),
        }
    }
}

/// The wall clock the deadline carrier speaks (unix milliseconds).
pub fn unix_ms_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Milliseconds remaining until the absolute deadline (`None` = no deadline
/// wired; `Some(0)` = already exhausted).
pub fn remaining_budget_ms(deadline_unix_ms: Option<u64>) -> Option<u64> {
    deadline_unix_ms.map(|deadline| deadline.saturating_sub(unix_ms_now()))
}

/// The explicit failure of a deadline hit (R05-T05): `BudgetExceeded`,
/// NON-retryable — the call's total budget is spent across queue wait,
/// credential refresh, backoff, network and streaming, so a fresh attempt
/// under the same budget starts already exhausted.
pub fn budget_exceeded_error(detail: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::BudgetExceeded, detail, false)
}

/// Builds the shared no-redirect client (one per adapter instance) with the
/// pre-registered timeout segments.
pub fn build_client() -> Result<reqwest::Client, ProtocolError> {
    build_client_with_timeouts(&HttpTimeouts::default())
}

/// Builds the shared no-redirect client with explicit timeout segments
/// (C10: a credential-bearing request is never replayed onto a redirected
/// origin). R05-T05: proxy auto-detection is DISABLED (`no_proxy`) —
/// incumbent parity (the TS model path uses Node fetch, which never
/// consults ambient proxies), and an ambient proxy breaks the A10
/// acceptance proof (it synthesizes its own answers for refused/dropped
/// upstreams, masking the real wire outcome) while silently exposing
/// credential material. An explicit, opt-in proxy/TLS configuration
/// surface is the registered C12 leftover (see INTERFACE_EVOLUTION §25).
pub fn build_client_with_timeouts(
    timeouts: &HttpTimeouts,
) -> Result<reqwest::Client, ProtocolError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(timeouts.connect)
        .build()
        .map_err(|err| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!("http client construction failed: {err}"),
                false,
            )
        })
}

/// Sends one built request under the three-segment timeout discipline
/// (R05-T05):
///
/// 1. PRE-SEND budget check — an already-exhausted deadline fails the call
///    BEFORE any byte leaves (a loud `BudgetExceeded`, never a send that
///    races its own budget).
/// 2. FIRST-BYTE segment — `send()` (which spans connect) must deliver the
///    response head within `min(first_byte, remaining budget)`. A first-byte
///    hit is a NON-retryable `UpstreamUnavailable` (A10: the request was
///    fully sent, so the server may hold it — no blind resend); a deadline
///    hit inside the same window is the non-retryable `BudgetExceeded`
///    (distinguished by a fresh clock read).
/// 3. Non-2xx answers classify through [`classify_error_status`] — a 429
///    additionally carries its `Retry-After` hint in the error details.
///
/// The caller's streamed-read phase applies the remaining budget through
/// [`drive_sse_stream_within_budget`].
pub async fn send_with_timeouts(
    request: reqwest::RequestBuilder,
    timeouts: &HttpTimeouts,
    deadline_unix_ms: Option<u64>,
    auth: &ApplicableAuth,
) -> Result<reqwest::Response, (ProtocolError, bool)> {
    let response = send_head_with_timeouts(request, timeouts, deadline_unix_ms).await?;
    let status = response.status();
    if !status.is_success() {
        let retry_after_ms = parse_retry_after(response.headers());
        let body_text = response.text().await.unwrap_or_default();
        return Err(classify_error_status(
            status,
            &body_text,
            auth,
            retry_after_ms,
        ));
    }
    Ok(response)
}

/// The send half of [`send_with_timeouts`] WITHOUT status classification
/// (R05-T06): the agnes video query flow branches on the raw status itself
/// (a non-2xx primary answer falls back to the legacy endpoint before any
/// error surfaces). Every other caller uses [`send_with_timeouts`] — an
/// unclassified response must have its status handled by the caller,
/// never dropped on the floor.
pub async fn send_head_with_timeouts(
    request: reqwest::RequestBuilder,
    timeouts: &HttpTimeouts,
    deadline_unix_ms: Option<u64>,
) -> Result<reqwest::Response, (ProtocolError, bool)> {
    if let Some(0) = remaining_budget_ms(deadline_unix_ms) {
        let error = budget_exceeded_error(
            "model call total budget (model_call_total_budget_ms) is exhausted before the \
             request could be sent: queue wait / credential refresh / earlier backoff spent \
             it; not retryable under the same budget"
                .to_string(),
        );
        return Err((error, false));
    }
    let window = match remaining_budget_ms(deadline_unix_ms) {
        Some(remaining) => timeouts
            .first_byte
            .min(std::time::Duration::from_millis(remaining)),
        None => timeouts.first_byte,
    };
    let response = match tokio::time::timeout(window, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            // A10 (已接受不盲重发): a CONNECT-phase failure proves the
            // request never reached the server — a bounded retry is
            // side-effect-free. Any LATER send/head failure means the
            // server may already hold the (billable, non-idempotent)
            // request: no idempotency key exists, so the call settles
            // NON-retryable — never a blind second send. (Incumbent
            // parity: core/model-operation-client.ts marks only 429/5xx
            // retryable; every transport failure is terminal there.)
            let retryable = err.is_connect();
            return Err((
                ProtocolError::new(
                    ErrorCode::UpstreamUnavailable,
                    format!("provider request failed before a response: {err}"),
                    retryable,
                ),
                retryable,
            ));
        }
        Err(_elapsed) => {
            // Which bound fired? A fresh clock read decides: an exhausted
            // deadline is the non-retryable budget failure; otherwise the
            // first-byte window fired. BOTH are non-retryable (A10): the
            // request was fully sent, so the server may have accepted it —
            // a resend would be an unprovable duplicate.
            if remaining_budget_ms(deadline_unix_ms) == Some(0) {
                let error = budget_exceeded_error(
                    "model call total budget exhausted while waiting for the response head \
                     (deadline_unix_ms reached); not retryable under the same budget"
                        .to_string(),
                );
                return Err((error, false));
            }
            return Err((
                ProtocolError::new(
                    ErrorCode::UpstreamUnavailable,
                    format!(
                        "provider response head did not arrive within {} ms (pre-registered \
                         http_first_byte_timeout_ms): the request was already sent, so the \
                         call fails NON-retryable — no blind resend of a possibly-accepted \
                         request (A10)",
                        timeouts.first_byte.as_millis()
                    ),
                    false,
                ),
                false,
            ));
        }
    };
    Ok(response)
}

/// The stream-read half of the total budget (R05-T05): the incremental SSE
/// drive runs under the remaining deadline when one is wired. A deadline hit
/// mid-stream is the non-retryable `BudgetExceeded` (the partial deltas
/// already emitted stay in the run's event log; the turn itself settles
/// through the caller's failure classification, same as any drive failure).
pub async fn drive_sse_stream_within_budget<H: SseStreamHandler>(
    response: reqwest::Response,
    handler: &mut H,
    deadline_unix_ms: Option<u64>,
) -> Result<(), (ProtocolError, bool)> {
    let Some(remaining) = remaining_budget_ms(deadline_unix_ms) else {
        return drive_sse_stream(response, handler).await;
    };
    match tokio::time::timeout(
        std::time::Duration::from_millis(remaining),
        drive_sse_stream(response, handler),
    )
    .await
    {
        Ok(result) => result,
        Err(_elapsed) => Err((
            budget_exceeded_error(
                "model call total budget exhausted mid-stream (deadline_unix_ms reached): the \
                 connection read is abandoned; partial deltas stay durable and the turn settles \
                 failed — not retryable under the same budget"
                    .to_string(),
            ),
            false,
        )),
    }
}

/// Parses the provider's `Retry-After` header (RFC 9110 §10.2.3): either a
/// non-negative integer of seconds or an HTTP-date. Returns milliseconds;
/// a malformed/absent value is `None` (the caller's computed backoff then
/// applies — a garbage hint is ignored, never trusted).
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let raw = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if raw.is_empty() {
        return None;
    }
    if raw.bytes().all(|b| b.is_ascii_digit()) {
        return raw
            .parse::<u64>()
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000));
    }
    let date = httpdate::parse_http_date(raw).ok()?;
    let target = date.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    Some(target.saturating_sub(unix_ms_now()))
}

/// How a family applies the resolved material to the request. `Bearer`
/// (static key or OAuth access token) maps to the family's conventional
/// header: `Authorization: Bearer` for the OpenAI families, `x-api-key` for
/// anthropic, `x-goog-api-key` for google (the incumbent's exact mapping —
/// docs/rust-tauri/R05/PROTOCOL_WIRE_MATRIX.json). An `authHeader` config
/// always goes out verbatim; `None` sends nothing (the explicit keyless
/// local contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BearerStyle {
    /// `Authorization: Bearer <token>`.
    AuthorizationBearer,
    /// `<name>: <token>` (anthropic `x-api-key`, google `x-goog-api-key`).
    NamedHeader(&'static str),
}

pub fn apply_auth(
    request: reqwest::RequestBuilder,
    auth: &ApplicableAuth,
    bearer_style: BearerStyle,
) -> reqwest::RequestBuilder {
    match auth {
        ApplicableAuth::None => request,
        ApplicableAuth::Bearer(token) => match bearer_style {
            BearerStyle::AuthorizationBearer => request.bearer_auth(token),
            BearerStyle::NamedHeader(name) => request.header(name, token.as_str()),
        },
        ApplicableAuth::Header { name, value } => request.header(name.as_str(), value.as_str()),
    }
}

/// The incumbent base-URL join rule (lib/llm/provider-client.ts
/// `appendProviderApiPath`): trailing slashes stripped; when the base
/// PATH already ends with the target path (case-insensitive) the base is
/// used as-is; when the target starts with `/v1/` and the base path ends
/// with `/v1` the shared prefix merges once; otherwise concatenate. The
/// incumbent's query-string carryover is unused (no T03 target path carries
/// a query). Never a guessed host or scheme.
pub fn append_provider_api_path(endpoint: &str, target_path: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    // The base's URL pathname (leading slash; empty for a bare origin).
    let base_path: String = match base.split_once("://") {
        Some((_, rest)) => match rest.find('/') {
            Some(index) => rest[index..].trim_end_matches('/').to_string(),
            None => String::new(),
        },
        None => base.to_string(),
    };
    let target = target_path.trim_end_matches('/');
    if !target.is_empty()
        && base_path
            .to_ascii_lowercase()
            .ends_with(&target.to_ascii_lowercase())
    {
        return base.to_string();
    }
    if let Some(stripped) = target.strip_prefix("/v1/") {
        if base_path.to_ascii_lowercase().ends_with("/v1") {
            return format!("{base}/{stripped}");
        }
    }
    format!("{base}{target}")
}

/// The provider's error body is attacker-influenceable echo: the excerpt
/// that travels into the run's error message is truncated and scrubbed of
/// the in-play credential material first (C09).
pub fn scrubbed_excerpt(body_text: &str, auth: &ApplicableAuth) -> String {
    scrub_materials(
        &body_text.chars().take(512).collect::<String>(),
        &auth.materials(),
    )
}

/// The per-family incremental decode half of [`drive_sse_stream`] (R05-T04)
/// — implemented by each family's stream accumulator. Pure and synchronous:
/// the accumulator consumes one decoded event and returns the live deltas
/// to emit; delivery (awaiting sink capacity) is the drive's job.
pub trait StreamAccumulator {
    fn handle_event(
        &mut self,
        event: &super::streaming::SseEvent,
    ) -> Result<Vec<lingxi_kernel::ports::ModelTurnDelta>, ProtocolError>;
}

/// The handler adapter from a family's [`StreamAccumulator`] to the drive:
/// emits every returned delta to the turn's sink IN ORDER as the stream
/// delivers it. A closed sink (the driver is gone) stops the read with a
/// non-retryable `Cancelled` — the provider never buffers "for later".
pub struct FamilyStreamDrive<'a, A> {
    pub accumulator: A,
    pub sink: &'a dyn lingxi_kernel::ports::TurnDeltaSink,
}

/// The boxed handler future of [`SseStreamHandler`] (the tuple is
/// `(error, retryable)` — the drive's failure vocabulary).
pub type SseHandleFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(), (ProtocolError, bool)>> + Send + 'a>,
>;

/// Object-safe handler surface of [`drive_sse_stream`] (boxed futures, the
/// same shape [`lingxi_kernel::ports::TurnDeltaSink`] uses — an
/// `async FnMut` closure cannot express the `Send` generalization the
/// provider's boxed turn future needs).
pub trait SseStreamHandler {
    fn handle<'a>(&'a mut self, event: super::streaming::SseEvent) -> SseHandleFuture<'a>;
}

impl<A: StreamAccumulator + Send> SseStreamHandler for FamilyStreamDrive<'_, A> {
    fn handle<'a>(&'a mut self, event: super::streaming::SseEvent) -> SseHandleFuture<'a> {
        Box::pin(async move {
            let emitted = match self.accumulator.handle_event(&event) {
                Ok(emitted) => emitted,
                Err(error) => {
                    let retryable = error.retryable;
                    return Err((error, retryable));
                }
            };
            for delta in emitted {
                if self.sink.emit(delta).await.is_err() {
                    return Err((
                        ProtocolError::new(
                            ErrorCode::Cancelled,
                            "the turn delta sink is closed (the run driver is gone); the \
                             provider stops reading the stream"
                                .to_string(),
                            false,
                        ),
                        false,
                    ));
                }
            }
            Ok(())
        })
    }
}

/// Drives one SSE response stream INCREMENTALLY (R05-T04): every chunk is
/// fed to the decoder AS THE NETWORK DELIVERS it, every completed event is
/// handed to `handler` immediately (the family's accumulator emits its live
/// deltas there — nothing is buffered to the end and re-sliced).
///
/// Bounds and failure honesty (C04/C15):
/// - The TOTAL bytes read are bounded by
///   [`super::streaming::SSE_BUFFER_LIMIT`] (8 MiB — tighter than the
///   R05_BASELINE pre-registered 16 MiB `stream_total_buffer_max_bytes`;
///   §8 permits tightening). The per-frame bound lives in the decoder.
/// - A decoder refusal (invalid UTF-8 / bound) or a handler refusal
///   ABANDONS the stream: the response is dropped (the connection read is
///   cancelled) and the error travels — a 200 status never masks a broken
///   stream body (C15).
/// - A transport read error mid-body is `UpstreamUnavailable`
///   NON-retryable (A10 — the server accepted the request once the head
///   arrived; a resend would be an unprovable duplicate. The partial deltas
///   already emitted stay in the run's event log; the turn itself settles
///   through the caller's failure classification).
/// - A trailing UNTERMINATED frame at EOF is a loud `InvalidMessage` (the
///   decoder reports it; the family layer's terminal-marker check decides
///   whether the stream closed cleanly).
pub async fn drive_sse_stream<H: SseStreamHandler>(
    mut response: reqwest::Response,
    handler: &mut H,
) -> Result<(), (ProtocolError, bool)> {
    let mut decoder = super::streaming::SseDecoder::new();
    let mut total_read: usize = 0;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                total_read += chunk.len();
                if total_read > super::streaming::SSE_BUFFER_LIMIT {
                    return Err((
                        ProtocolError::new(
                            ErrorCode::InvalidMessage,
                            format!(
                                "SSE stream exceeded the {}-byte total read bound; refusing \
                                 (a runaway stream is a protocol failure, never an unbounded \
                                 read)",
                                super::streaming::SSE_BUFFER_LIMIT
                            ),
                            false,
                        ),
                        false,
                    ));
                }
                let events = decoder.feed(&chunk).map_err(|error| (error, false))?;
                for event in events {
                    handler.handle(event).await?;
                }
            }
            Ok(None) => break,
            Err(err) => {
                // A10: the response head already arrived — the server
                // accepted the request. A broken mid-read settles
                // NON-retryable (no blind resend of an accepted call); the
                // partial deltas already emitted stay in the run's event
                // log, and the caller's failure classification settles the
                // turn.
                return Err((
                    ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        format!("provider stream broke mid-read: {err}"),
                        false,
                    ),
                    false,
                ));
            }
        }
    }
    let finish = decoder.finish().map_err(|error| (error, false))?;
    for event in finish.events {
        handler.handle(event).await?;
    }
    if finish.discarded_partial {
        return Err((
            ProtocolError::new(
                ErrorCode::InvalidMessage,
                "SSE stream ended with an unterminated trailing frame; the partial event is \
                 discarded and reported, never half-parsed into a turn"
                    .to_string(),
                false,
            ),
            false,
        ));
    }
    Ok(())
}

/// Maps a non-2xx status onto the wire error vocabulary (the incumbent
/// classification, shared verbatim across families): 401 Unauthorized
/// (never retried by the adapter — the caller's single bounded
/// refresh-and-retry applies), 403 Forbidden, 3xx a loud refusal (redirects
/// are never followed with credential-bearing requests, C10 — the Location
/// target is provider-controlled and never echoed), 408/5xx retryable
/// upstream, 429 budget (retryable; a parsed `Retry-After` hint travels in
/// the error details under [`RETRY_AFTER_MS_DETAIL`] for the driver's
/// bounded backoff), 400/404/422 invalid message.
pub fn classify_error_status(
    status: reqwest::StatusCode,
    body_text: &str,
    auth: &ApplicableAuth,
    retry_after_ms: Option<u64>,
) -> (ProtocolError, bool) {
    let excerpt = scrubbed_excerpt(body_text, auth);
    let (code, retryable, note) = match status.as_u16() {
        401 => (ErrorCode::Unauthorized, false, excerpt),
        403 => (ErrorCode::Forbidden, false, excerpt),
        s if (300..400).contains(&s) => (
            ErrorCode::UpstreamUnavailable,
            false,
            "redirect answer; redirects are never followed with credential-bearing \
             requests (C10)"
                .to_string(),
        ),
        // A10: a 408 means the server timed the request out — it may still
        // hold it. NON-retryable (incumbent parity: only 429/5xx retry).
        408 => (ErrorCode::UpstreamUnavailable, false, excerpt),
        429 => (ErrorCode::BudgetExceeded, true, excerpt),
        400 | 404 | 422 => (ErrorCode::InvalidMessage, false, excerpt),
        s if s >= 500 => (ErrorCode::UpstreamUnavailable, true, excerpt),
        _ => (ErrorCode::UpstreamUnavailable, false, excerpt),
    };
    let mut error = ProtocolError::new(
        code,
        format!("provider returned HTTP {status}: {note}"),
        retryable,
    );
    if status.as_u16() == 429 {
        if let Some(hint) = retry_after_ms {
            error = error.with_details(serde_json::Map::from_iter([(
                RETRY_AFTER_MS_DETAIL.to_string(),
                serde_json::Value::from(hint),
            )]));
        }
    }
    (error, retryable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_incumbent_join_rule_is_verbatim() {
        assert_eq!(
            append_provider_api_path("https://api.anthropic.com", "/v1/messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            append_provider_api_path("https://api.anthropic.com/", "/v1/messages"),
            "https://api.anthropic.com/v1/messages"
        );
        // The shared /v1 prefix merges exactly once.
        assert_eq!(
            append_provider_api_path("https://proxy.test/v1", "/v1/messages"),
            "https://proxy.test/v1/messages"
        );
        // An endpoint already naming the target path is used as-is.
        assert_eq!(
            append_provider_api_path("https://proxy.test/v1/messages", "/v1/messages"),
            "https://proxy.test/v1/messages"
        );
        // A non-/v1 target path (google) concatenates.
        assert_eq!(
            append_provider_api_path(
                "https://generativelanguage.googleapis.com",
                "/models/gemini-x:generateContent"
            ),
            "https://generativelanguage.googleapis.com/models/gemini-x:generateContent"
        );
        // The /v1 dedup is path-suffix exact, never a substring guess.
        assert_eq!(
            append_provider_api_path("https://proxy.test/services/v11", "/v1/messages"),
            "https://proxy.test/services/v11/v1/messages"
        );
    }

    #[test]
    fn error_status_classification_matches_the_incumbent() {
        let auth = ApplicableAuth::Bearer("sk-secret".to_string());
        let (err, retry) = classify_error_status(
            reqwest::StatusCode::UNAUTHORIZED,
            "bad key sk-secret",
            &auth,
            None,
        );
        assert_eq!(err.code, ErrorCode::Unauthorized);
        assert!(!retry);
        assert!(err.message.contains("[redacted]") && !err.message.contains("sk-secret"));
        let (err, retry) = classify_error_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "slow down",
            &auth,
            None,
        );
        assert_eq!(err.code, ErrorCode::BudgetExceeded);
        assert!(retry);
        assert!(err.details.is_none(), "no Retry-After hint, no details");
        let (err, retry) =
            classify_error_status(reqwest::StatusCode::FOUND, "http://evil.test/", &auth, None);
        assert_eq!(err.code, ErrorCode::UpstreamUnavailable);
        assert!(!retry);
        assert!(!err.message.contains("evil.test"), "Location never echoed");
        let (_err, retry) = classify_error_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "boom",
            &auth,
            None,
        );
        assert!(retry);
        let (err, retry) =
            classify_error_status(reqwest::StatusCode::BAD_REQUEST, "bad", &auth, None);
        assert_eq!(err.code, ErrorCode::InvalidMessage);
        assert!(!retry);
    }

    #[test]
    fn a_429_carries_its_retry_after_hint_in_the_error_details() {
        let auth = ApplicableAuth::Bearer("sk-secret".to_string());
        let (err, retry) = classify_error_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "slow down",
            &auth,
            Some(1_250),
        );
        assert_eq!(err.code, ErrorCode::BudgetExceeded);
        assert!(retry);
        assert_eq!(
            err.details
                .as_ref()
                .and_then(|d| d.get(RETRY_AFTER_MS_DETAIL))
                .and_then(|v| v.as_u64()),
            Some(1_250)
        );
        // A hint on a non-429 status is never attached.
        let (err, _) = classify_error_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "boom",
            &auth,
            Some(9_999),
        );
        assert!(err.details.is_none());
    }

    #[test]
    fn retry_after_parses_seconds_and_http_dates_and_refuses_garbage() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);
        headers.insert(reqwest::header::RETRY_AFTER, "2".parse().expect("header"));
        assert_eq!(parse_retry_after(&headers), Some(2_000));
        headers.insert(reqwest::header::RETRY_AFTER, "0".parse().expect("header"));
        assert_eq!(parse_retry_after(&headers), Some(0));
        // HTTP-date one hour out (RFC 9110 allows the date form).
        let date = httpdate::fmt_http_date(
            std::time::SystemTime::now() + std::time::Duration::from_secs(3_600),
        );
        headers.insert(
            reqwest::header::RETRY_AFTER,
            date.parse().expect("date header"),
        );
        let hint = parse_retry_after(&headers).expect("date form parses");
        assert!(
            (3_500_000..=3_600_000).contains(&hint),
            "date form maps to a ms delta from now: {hint}"
        );
        for garbage in ["soon", "-5", "1.5", "", "  "] {
            headers.insert(
                reqwest::header::RETRY_AFTER,
                garbage.parse().expect("header"),
            );
            assert_eq!(
                parse_retry_after(&headers),
                None,
                "garbage hint {garbage:?} is ignored, never trusted"
            );
        }
    }
}
