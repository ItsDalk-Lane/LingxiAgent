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

use super::credentials::{sanitize_diagnostic, ApplicableAuth};

/// The shared client-handle type every network consumer holds (R05 RR1
/// F14; defined in [`super::network`], re-exported for the families).
pub use super::network::NetworkClient;

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
/// pre-registered timeout segments. R05 RR1 F14: this is the DIRECT-policy
/// legacy entry — production consumers hold a
/// [`super::network::NetworkClient`] bound to the model plane's reloadable
/// [`super::network::NetworkPlane`] instead.
pub fn build_client() -> Result<reqwest::Client, ProtocolError> {
    build_client_with_timeouts(&HttpTimeouts::default())
}

/// Builds the shared no-redirect client with explicit timeout segments
/// (C10: a credential-bearing request is never replayed onto a redirected
/// origin). The DIRECT policy leg of [`build_client_under_policy`] — the
/// pre-F14 behavior kept for the isolated/test constructors.
pub fn build_client_with_timeouts(
    timeouts: &HttpTimeouts,
) -> Result<reqwest::Client, ProtocolError> {
    build_client_under_policy(
        timeouts,
        &super::network::NetworkPolicy {
            proxy: super::network::ProxyPolicy::Direct,
            trusted_ca_pem: None,
        },
    )
}

/// Builds the shared no-redirect client UNDER one frozen
/// [`super::network::NetworkPolicy`] (R05 RR1 F14, T05-C12): the policy's
/// proxy routing (system / manual / direct with the incumbent NO_PROXY and
/// forced-loopback-bypass grammar, via a per-URL proxy interceptor) and its
/// EXPLICIT trusted CA bundle (added ON TOP of the platform verifier's
/// roots). The default chain, hostname and validity verification are never
/// relaxed — no `accept_invalid_certs`, no hostname bypass; a CA or proxy
/// misconfiguration is a loud construction failure, never a degraded
/// no-verify fallback.
pub fn build_client_under_policy(
    timeouts: &HttpTimeouts,
    policy: &super::network::NetworkPolicy,
) -> Result<reqwest::Client, ProtocolError> {
    build_client_pinned_under_policy(timeouts, policy, "", &[])
}

/// [`build_client_under_policy`] with an optional DNS PIN (R05 RR1 F15):
/// when `pinned_host` is non-empty, the client's resolution of that ONE
/// host is overridden to the pre-verified `pinned_addrs`
/// (`ClientBuilder::resolve_to_addrs`) — the transport cannot re-resolve
/// the name to an address the caller never verified. An empty host builds
/// the ordinary shared client.
pub fn build_client_pinned_under_policy(
    timeouts: &HttpTimeouts,
    policy: &super::network::NetworkPolicy,
    pinned_host: &str,
    pinned_addrs: &[std::net::SocketAddr],
) -> Result<reqwest::Client, ProtocolError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeouts.connect);
    if !pinned_host.is_empty() {
        builder = builder.resolve_to_addrs(pinned_host, pinned_addrs);
    }
    match proxy_intercept(policy) {
        Some(intercept) => builder = builder.proxy(intercept),
        None => builder = builder.no_proxy(),
    }
    if let Some(pem) = policy.trusted_ca_pem.as_deref() {
        for certificate in reqwest::Certificate::from_pem_bundle(pem.as_bytes()).map_err(|err| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!(
                    "the network policy's trusted CA bundle is not parseable PEM: {err} \
                         (validated at config load — this is a wiring bug, never a \
                         no-verify fallback)"
                ),
                false,
            )
        })? {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder.build().map_err(|err| {
        ProtocolError::new(
            ErrorCode::Internal,
            format!("http client construction failed: {err}"),
            false,
        )
    })
}

/// The per-URL proxy interceptor of one policy (a `Proxy::custom` closure
/// consulting the frozen policy — installing ANY explicit proxy also
/// disables reqwest's own ambient proxy detection, so the policy is the
/// single routing authority).
fn proxy_intercept(policy: &super::network::NetworkPolicy) -> Option<reqwest::Proxy> {
    let resolved = super::network::resolve_proxy_set(&policy.proxy);
    if resolved.is_empty() {
        return None;
    }
    Some(reqwest::Proxy::custom(move |url: &reqwest::Url| {
        super::network::pick_proxy(url, &resolved).map(str::to_string)
    }))
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
///    R05 RR1 F16: the error body itself is read under the REMAINING
///    absolute budget and a hard size cap ([`read_error_body_bounded`]) —
///    a stalled, trickling or oversized error body can no longer park the
///    call past its total deadline or buffer unboundedly, and the
///    timeout/truncation/read-failure FACT is preserved in the classified
///    error instead of being swallowed into an empty excerpt.
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
        let (body_text, read_note) = read_error_body_bounded(response, deadline_unix_ms).await;
        let (mut error, retryable) =
            classify_error_status(status, &body_text, auth, retry_after_ms);
        if let Some(note) = read_note {
            error.message.push_str(&format!("; {note}"));
        }
        return Err((error, retryable));
    }
    Ok(response)
}

/// The hard size cap of one non-2xx error body read (R05 RR1 F16): the
/// excerpt that reaches the run's error message is bounded to
/// [`EXCERPT_BOUND_CHARS`] after scrubbing anyway; 64 KiB is far above any
/// diagnostic value and far below an unbounded buffer.
pub const ERROR_BODY_MAX_BYTES: usize = 64 * 1024;

/// Reads one non-2xx body under the remaining absolute budget and
/// [`ERROR_BODY_MAX_BYTES`] (R05 RR1 F16). Returns the bytes that arrived
/// plus an honest NOTE of how the read ended:
/// - `None` — the body closed cleanly;
/// - deadline — "…abandoned at the call's total deadline" (the call's
///   outcome is already determined by the status; the read is cut, never
///   parked past its budget);
/// - cap — "…truncated at the error-body byte cap";
/// - transport failure — "…read failed: {err}".
///
/// The pre-fix shape (`response.text().await.unwrap_or_default()`) had
/// neither bound and SWALLOWED every read failure — a stalled error body
/// outlived the total deadline and a mid-read break vanished silently.
pub async fn read_error_body_bounded(
    mut response: reqwest::Response,
    deadline_unix_ms: Option<u64>,
) -> (String, Option<String>) {
    let mut out: Vec<u8> = Vec::new();
    loop {
        let next = response.chunk();
        let chunk = match remaining_budget_ms(deadline_unix_ms) {
            Some(remaining) => {
                match tokio::time::timeout(std::time::Duration::from_millis(remaining), next).await
                {
                    Ok(result) => result,
                    Err(_) => {
                        return (
                            String::from_utf8_lossy(&out).into_owned(),
                            Some(
                                "the error body read was abandoned at the call's total \
                                 deadline (deadline_unix_ms reached); the excerpt holds only \
                                 the bytes that arrived"
                                    .to_string(),
                            ),
                        );
                    }
                }
            }
            None => next.await,
        };
        match chunk {
            Ok(Some(bytes)) => {
                if out.len() + bytes.len() > ERROR_BODY_MAX_BYTES {
                    out.extend_from_slice(&bytes[..ERROR_BODY_MAX_BYTES - out.len()]);
                    return (
                        String::from_utf8_lossy(&out).into_owned(),
                        Some(format!(
                            "the error body was truncated at the {ERROR_BODY_MAX_BYTES} byte \
                             read cap"
                        )),
                    );
                }
                out.extend_from_slice(&bytes);
            }
            Ok(None) => return (String::from_utf8_lossy(&out).into_owned(), None),
            Err(err) => {
                return (
                    String::from_utf8_lossy(&out).into_owned(),
                    Some(format!(
                        "the error body read failed mid-body: {err} (the excerpt holds only \
                         the bytes that arrived)"
                    )),
                );
            }
        }
    }
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
/// that travels into the run's error message is scrubbed of the in-play
/// credential material (raw AND encoded echoes, C09) FIRST and bounded-
/// truncated second — the truncation can never cut a key in half and leave
/// an unmatched prefix behind (R05 RR1 F05).
pub fn scrubbed_excerpt(body_text: &str, auth: &ApplicableAuth) -> String {
    sanitize_diagnostic(body_text, &auth.materials(), EXCERPT_BOUND_CHARS)
}

/// The bounded excerpt length (characters, after the scrub).
pub const EXCERPT_BOUND_CHARS: usize = 512;

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
