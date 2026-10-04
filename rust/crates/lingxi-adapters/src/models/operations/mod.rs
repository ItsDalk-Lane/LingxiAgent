//! R05-T06: the media & auxiliary OPERATION plane (§29.9) — the non-chat
//! model operations (embedding / rerank / image / video / speech /
//! transcription) as pure dialect plan/parse layers over the shared
//! dispatch discipline, one module per operation.
//!
//! Layering:
//! - the dialect `build_*` functions are PURE: route + resolved auth +
//!   request → an [`OperationRequestPlan`] (method/url/headers/body). A
//!   plan never lands in logs (headers carry the applied credential
//!   material).
//! - the dialect `parse_*` functions validate the response body into typed
//!   answers (count/order/dimension/finite-value checks per the incumbent
//!   contracts) — a malformed answer is a loud failure, never a guessed
//!   one.
//! - [`OperationDispatcher`] executes plans under the SAME network
//!   discipline as the chat families (no-redirect, no-proxy, three-segment
//!   timeouts, shared error classification) plus bounded mid-read bodies.
//! - binary/URL media products surface as [`MediaProductRef`]; downloading
//!   URL products is the SERVICE layer's job (egress-guarded), never this
//!   crate's.
//!
//! Bounds (§30 registration): JSON operation bodies read to 32 MiB, binary
//! bodies to 64 MiB — both enforced DURING the read, both loud.

use lingxi_kernel::model_exchange::ResolvedModelRoute;
use lingxi_protocol::{ErrorCode, ProtocolError};

use super::credentials::ApplicableAuth;
use super::dispatch::{self, HttpTimeouts};

pub mod embedding;
pub mod image;
pub mod rerank;
pub mod speech;
pub mod tiers;
pub mod transcribe;
pub mod video;

/// JSON operation response bodies are read to this cap, mid-read.
pub const OPERATION_JSON_BODY_MAX_BYTES: usize = 32 * 1024 * 1024;
/// Binary operation bodies (synthesized audio etc.) read to this cap.
pub const OPERATION_BINARY_BODY_MAX_BYTES: usize = 64 * 1024 * 1024;

/// The incumbent `MAX_TEXT_CHARS` (model-operation-client.ts): one text's
/// RAW UTF-16 code-unit cap.
pub const MAX_TEXT_CHARS: usize = 32_000;
/// The incumbent `MAX_TOTAL_TEXT_CHARS`: the RAW UTF-16 total cap.
pub const MAX_TOTAL_TEXT_CHARS: usize = 500_000;

/// One fully-planned provider HTTP call (the dialect output). The headers
/// INCLUDE the applied credential header — a plan is never logged.
#[derive(Debug)]
pub struct OperationRequestPlan {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl OperationRequestPlan {
    /// A JSON POST plan (Content-Type set; body = the compact encoding).
    pub fn post_json(url: String, body: serde_json::Value) -> Self {
        Self {
            method: "POST",
            url,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: serde_json::to_vec(&body).expect("a dialect body serializes"),
        }
    }

    /// A bodyless GET plan.
    pub fn get(url: String) -> Self {
        Self {
            method: "GET",
            url,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Applies the resolved credential material per the family's rule.
    pub fn with_auth(mut self, auth: &ApplicableAuth, style: dispatch::BearerStyle) -> Self {
        match auth {
            ApplicableAuth::None => {}
            ApplicableAuth::Bearer(token) => match style {
                dispatch::BearerStyle::AuthorizationBearer => self
                    .headers
                    .push(("authorization".to_string(), format!("Bearer {token}"))),
                dispatch::BearerStyle::NamedHeader(name) => {
                    self.headers.push((name.to_string(), token.clone()))
                }
            },
            ApplicableAuth::Header { name, value } => {
                self.headers.push((name.clone(), value.clone()))
            }
        }
        self
    }

    /// Appends one protocol header (anthropic-version, OpenAI-Beta, …).
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
}

/// One produced media artifact reference: either bytes already on the wire
/// (base64-decoded, MIME from the family mapping) or a provider URL the
/// SERVICE layer downloads through the egress guard (C11B — never with
/// credentials, never this crate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaProductRef {
    Bytes {
        bytes: Vec<u8>,
        mime: String,
    },
    Url {
        url: String,
        mime_hint: Option<String>,
    },
}

/// One reference image input (the incumbent `params.image` entries). The
/// SERVICE layer resolves what the incumbent resolves inside the adapters:
/// a local file path is read host-side into [`ImageReference::Bytes`]
/// (filename + wire MIME derived from the path's extension per the
/// incumbent `imageMime` table: png/jpg/jpeg/webp, else `image/png`),
/// everything else rides as its string form. A plan never logs these.
#[derive(Debug, Clone)]
pub enum ImageReference {
    /// Host-read bytes of a local file (the incumbent's fs.readFileSync
    /// path): bytes + the wire filename (the path's basename) + wire MIME.
    Bytes {
        bytes: Vec<u8>,
        mime: String,
        filename: String,
    },
    /// A `data:<mime>;base64,...` URL string.
    DataUrl(String),
    /// An http(s) URL string (pass-through for the URL-accepting families;
    /// the gemini family refuses it — the service pre-fetches through the
    /// egress guard into bytes instead).
    RemoteUrl(String),
    /// Any other provider-side reference string (an openai `file-…` id, a
    /// gemini `file_uri`, …).
    ProviderRef(String),
}

impl ImageReference {
    /// The wire string form for the pass-through families (volcengine /
    /// minimax / dashscope / agnes / codex): strings ride verbatim; bytes
    /// become the base64 data URL the incumbent's `localImageToDataUrl`
    /// would have produced.
    pub fn wire_string(&self) -> String {
        match self {
            ImageReference::Bytes { bytes, mime, .. } => image_data_url(bytes, mime),
            ImageReference::DataUrl(url) | ImageReference::RemoteUrl(url) => url.clone(),
            ImageReference::ProviderRef(reference) => reference.clone(),
        }
    }
}

/// The outcome of a media SUBMIT call (image or video): either the
/// products settled synchronously (the fake-async families) or an async
/// task was accepted and the caller polls with the family's query plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSubmitOutcome {
    Done { products: Vec<MediaProductRef> },
    Pending { task_id: String },
}

/// The outcome of one async-task POLL (dashscope image query, agnes video
/// query): honestly pending, failed with the provider's reason, or done
/// with the products.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskPollOutcome {
    Pending,
    Failed { reason: String },
    Done { products: Vec<MediaProductRef> },
}

/// Executes operation plans under the shared dispatch discipline.
pub struct OperationDispatcher {
    client: reqwest::Client,
    timeouts: HttpTimeouts,
}

impl OperationDispatcher {
    pub fn new() -> Result<Self, ProtocolError> {
        Self::with_timeouts(HttpTimeouts::default())
    }

    pub fn with_timeouts(timeouts: HttpTimeouts) -> Result<Self, ProtocolError> {
        Ok(Self {
            client: dispatch::build_client_with_timeouts(&timeouts)?,
            timeouts,
        })
    }

    fn build_request(&self, plan: &OperationRequestPlan) -> reqwest::RequestBuilder {
        let mut request = match plan.method {
            "GET" => self.client.get(&plan.url),
            _ => self.client.post(&plan.url),
        };
        for (name, value) in &plan.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if !plan.body.is_empty() {
            request = request.body(plan.body.clone());
        }
        request
    }

    /// Executes the plan and reads a bounded JSON body. Non-2xx answers
    /// classify through the shared table (the excerpt is scrubbed against
    /// the in-play auth material).
    pub async fn execute_json(
        &self,
        plan: &OperationRequestPlan,
        deadline_unix_ms: Option<u64>,
        auth: &ApplicableAuth,
    ) -> Result<serde_json::Value, (ProtocolError, bool)> {
        Ok(self
            .execute_json_full(plan, deadline_unix_ms, auth)
            .await?
            .0)
    }

    /// As [`Self::execute_json`], additionally returning the response
    /// headers (volcengine-bigasr carries its status code in a HEADER).
    pub async fn execute_json_full(
        &self,
        plan: &OperationRequestPlan,
        deadline_unix_ms: Option<u64>,
        auth: &ApplicableAuth,
    ) -> Result<(serde_json::Value, reqwest::header::HeaderMap), (ProtocolError, bool)> {
        let response = dispatch::send_with_timeouts(
            self.build_request(plan),
            &self.timeouts,
            deadline_unix_ms,
            auth,
        )
        .await?;
        let headers = response.headers().clone();
        let bytes =
            read_body_bounded(response, OPERATION_JSON_BODY_MAX_BYTES, deadline_unix_ms).await?;
        let value = serde_json::from_slice(&bytes).map_err(|err| {
            (
                ProtocolError::new(
                    ErrorCode::InvalidMessage,
                    format!("operation response is not valid JSON: {err}"),
                    false,
                ),
                false,
            )
        })?;
        Ok((value, headers))
    }

    /// Executes the plan and reads a bounded binary body, returning the
    /// bytes with the response Content-Type when present.
    pub async fn execute_bytes(
        &self,
        plan: &OperationRequestPlan,
        deadline_unix_ms: Option<u64>,
        auth: &ApplicableAuth,
    ) -> Result<(Vec<u8>, Option<String>), (ProtocolError, bool)> {
        let response = dispatch::send_with_timeouts(
            self.build_request(plan),
            &self.timeouts,
            deadline_unix_ms,
            auth,
        )
        .await?;
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes =
            read_body_bounded(response, OPERATION_BINARY_BODY_MAX_BYTES, deadline_unix_ms).await?;
        Ok((bytes, mime))
    }

    /// Sends the plan WITHOUT status classification (the agnes video query
    /// branches on the raw status: a non-2xx primary answer falls back to
    /// the legacy endpoint before any error surfaces). The caller MUST
    /// handle every status — use [`Self::classify_error_response`] for the
    /// failure branch and [`Self::read_json_body`] for the success branch.
    pub async fn send_unclassified(
        &self,
        plan: &OperationRequestPlan,
        deadline_unix_ms: Option<u64>,
    ) -> Result<reqwest::Response, (ProtocolError, bool)> {
        dispatch::send_head_with_timeouts(
            self.build_request(plan),
            &self.timeouts,
            deadline_unix_ms,
        )
        .await
    }

    /// Reads a response's body under the JSON cap and parses it.
    pub async fn read_json_body(
        &self,
        response: reqwest::Response,
        deadline_unix_ms: Option<u64>,
    ) -> Result<serde_json::Value, (ProtocolError, bool)> {
        let bytes =
            read_body_bounded(response, OPERATION_JSON_BODY_MAX_BYTES, deadline_unix_ms).await?;
        serde_json::from_slice(&bytes).map_err(|err| {
            (
                ProtocolError::new(
                    ErrorCode::InvalidMessage,
                    format!("operation response is not valid JSON: {err}"),
                    false,
                ),
                false,
            )
        })
    }

    /// Classifies a non-2xx unclassified response through the shared table
    /// (the body excerpt is read bounded and scrubbed against the in-play
    /// auth material).
    pub async fn classify_error_response(
        &self,
        response: reqwest::Response,
        auth: &ApplicableAuth,
    ) -> (ProtocolError, bool) {
        let status = response.status();
        let retry_after_ms = dispatch::parse_retry_after(response.headers());
        let body_text = match read_body_bounded(response, OPERATION_JSON_BODY_MAX_BYTES, None).await
        {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        };
        dispatch::classify_error_status(status, &body_text, auth, retry_after_ms)
    }
}

/// Reads a response body chunk-by-chunk, enforcing `cap` DURING the read
/// (an endless/huge body is cut at the cap with a loud InvalidMessage,
/// never buffered whole) under the remaining deadline.
async fn read_body_bounded(
    mut response: reqwest::Response,
    cap: usize,
    deadline_unix_ms: Option<u64>,
) -> Result<Vec<u8>, (ProtocolError, bool)> {
    let mut out = Vec::new();
    loop {
        let next = response.chunk();
        let chunk = match dispatch::remaining_budget_ms(deadline_unix_ms) {
            Some(remaining) => {
                match tokio::time::timeout(std::time::Duration::from_millis(remaining), next).await
                {
                    Ok(result) => result,
                    Err(_) => {
                        return Err((
                            dispatch::budget_exceeded_error(
                                "model call total budget exhausted mid-body (operation read)"
                                    .to_string(),
                            ),
                            false,
                        ));
                    }
                }
            }
            None => next.await,
        };
        match chunk {
            Ok(Some(bytes)) => {
                if out.len() + bytes.len() > cap {
                    return Err((
                        ProtocolError::new(
                            ErrorCode::InvalidMessage,
                            format!(
                                "operation response body exceeded the {cap} byte cap mid-read; \
                                 the answer is refused, never truncated"
                            ),
                            false,
                        ),
                        false,
                    ));
                }
                out.extend_from_slice(&bytes);
            }
            Ok(None) => return Ok(out),
            Err(err) => {
                return Err((
                    ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        format!("operation response read failed mid-body: {err}"),
                        false,
                    ),
                    false,
                ));
            }
        }
    }
}

/// The incumbent `assertTextList` (core/model-operation-client.ts): 1..=
/// `max_items` items, each a non-empty-after-trim string whose RAW UTF-16
/// code-unit length (`String.length` — NOT trimmed, NOT code points) is
/// within [`MAX_TEXT_CHARS`], the RAW UTF-16 total within
/// [`MAX_TOTAL_TEXT_CHARS`]. Refusal texts are verbatim.
pub fn assert_text_list(
    value: &[String],
    label: &str,
    max_items: usize,
) -> Result<(), ProtocolError> {
    if value.is_empty() || value.len() > max_items {
        return Err(parse::invalid(format!(
            "{label} must contain between 1 and {max_items} items"
        )));
    }
    let mut total = 0usize;
    for (index, item) in value.iter().enumerate() {
        let raw_len: usize = item.chars().map(|c| c.len_utf16()).sum();
        if item.trim().is_empty() || raw_len > MAX_TEXT_CHARS {
            return Err(parse::invalid(format!(
                "{label}[{index}] must be a non-empty string no longer than {MAX_TEXT_CHARS} \
                 characters"
            )));
        }
        total += raw_len;
    }
    if total > MAX_TOTAL_TEXT_CHARS {
        return Err(parse::invalid(format!(
            "{label} exceeds the total character limit"
        )));
    }
    Ok(())
}

/// The incumbent `operationUrl` (model-operation-client.ts): trailing
/// slashes trimmed; a base already ending in `/{suffix}` (CASE-SENSITIVE
/// `endsWith`, unlike the chat plane's `append_provider_api_path`) is used
/// as-is, otherwise the suffix is appended.
pub fn operation_url(base: &str, suffix: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with(&format!("/{suffix}")) {
        trimmed.to_string()
    } else {
        format!("{trimmed}/{suffix}")
    }
}

/// The MiniMax `GroupId` discriminator required by the minimax embedding /
/// speech dialects (the model entry's `groupId`). Absent OR EMPTY refuses
/// with the incumbent's verbatim per-operation text (the two texts differ
/// only in the operation noun/verb, so each dialect passes its own).
pub fn require_group_id<'a>(
    route: &'a ResolvedModelRoute,
    refusal: &str,
) -> Result<&'a str, ProtocolError> {
    route
        .group_id
        .as_deref()
        .filter(|group_id| !group_id.is_empty())
        .ok_or_else(|| parse::invalid(refusal.to_string()))
}

/// The base64 data-URL of one host-read input image.
pub fn image_data_url(bytes: &[u8], mime: &str) -> String {
    use base64::Engine as _;
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// Minimal percent-encoding for one URL path segment (the same unreserved
/// rule as the google chat family's `urlencoding_of`).
pub fn urlencoding_of(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The JS `encodeURIComponent` rule (unreserved:
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`; UTF-8 bytes percent-encoded uppercase)
/// — the incumbent gemini/dashscope query-string escape.
pub fn encode_uri_component(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The `application/x-www-form-urlencoded` rule of `URLSearchParams`
/// (unreserved: `A-Z a-z 0-9 * - . _`; space → `+`; UTF-8 bytes
/// percent-encoded uppercase) — the incumbent agnes query-string escape.
pub fn form_urlencoded_of(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// A hand-rolled multipart/form-data part (reqwest's `multipart` feature
/// is deliberately NOT in the lockfile discipline — the encoder is 40
/// lines of obvious bytes).
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

impl MultipartPart {
    /// A UTF-8 field part.
    pub fn field(name: &str, value: &str) -> Self {
        Self {
            name: name.to_string(),
            filename: None,
            content_type: None,
            data: value.as_bytes().to_vec(),
        }
    }

    /// A file part (bytes + MIME + wire filename).
    pub fn file(name: &str, filename: &str, content_type: &str, data: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            filename: Some(filename.to_string()),
            content_type: Some(content_type.to_string()),
            data,
        }
    }
}

/// Encodes parts into (body, `Content-Type` header value) with a CSPRNG
/// boundary. Part names/filenames are dialect constants (never attacker
/// strings), so no quote-escaping surface exists.
pub fn encode_multipart(parts: &[MultipartPart]) -> Result<(Vec<u8>, String), ProtocolError> {
    let mut entropy = [0u8; 12];
    getrandom::getrandom(&mut entropy).map_err(|err| {
        ProtocolError::new(
            ErrorCode::Internal,
            format!("multipart boundary entropy failed: {err}"),
            false,
        )
    })?;
    let boundary: String = entropy.iter().map(|b| format!("{b:02x}")).collect();
    let boundary = format!("lingxi-{boundary}");
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let disposition = match &part.filename {
            Some(filename) => format!(
                "Content-Disposition: form-data; name=\"{}\"; filename=\"{filename}\"\r\n",
                part.name
            ),
            None => format!("Content-Disposition: form-data; name=\"{}\"\r\n", part.name),
        };
        body.extend_from_slice(disposition.as_bytes());
        if let Some(content_type) = &part.content_type {
            body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&part.data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((body, format!("multipart/form-data; boundary={boundary}")))
}

/// The shared loud parse helpers every dialect uses.
pub mod parse {
    use lingxi_protocol::{ErrorCode, ProtocolError};

    pub fn invalid(detail: String) -> ProtocolError {
        ProtocolError::new(ErrorCode::InvalidMessage, detail, false)
    }

    /// A finite f64 from a JSON number (NaN/Infinity are not JSON, but a
    /// 1e999 literal IS and parses to +inf — the incumbent's
    /// `Number.isFinite` check, ported exactly).
    pub fn finite_f64(value: &serde_json::Value, what: &str) -> Result<f64, ProtocolError> {
        let number = value
            .as_f64()
            .ok_or_else(|| invalid(format!("{what} is not a number: {value}")))?;
        if !number.is_finite() {
            return Err(invalid(format!("{what} is not finite: {number}")));
        }
        Ok(number)
    }

    /// The JS `Number(x)` coercion over a JSON value: null → 0, bool →
    /// 0/1, number → itself, string → trimmed parse (empty → 0,
    /// unparseable → NaN), empty array → 0, single-element array → the
    /// element coerced, anything else → NaN. (JS hex/"0x…" string parsing
    /// is NOT ported — registered; no provider payload uses it.)
    pub fn js_number(value: &serde_json::Value) -> f64 {
        use serde_json::Value;
        match value {
            Value::Null => 0.0,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    0.0
                } else {
                    trimmed.parse::<f64>().unwrap_or(f64::NAN)
                }
            }
            Value::Array(items) if items.is_empty() => 0.0,
            Value::Array(items) if items.len() == 1 => js_number(&items[0]),
            _ => f64::NAN,
        }
    }

    /// Renders an f64 as the JSON value `JSON.stringify` would produce for
    /// the same JS number (integer-valued doubles become integer literals,
    /// so `12.0` serializes as `12`).
    pub fn js_number_to_json(number: f64) -> serde_json::Value {
        if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_992.0 {
            return serde_json::Value::from(number as i64);
        }
        serde_json::Number::from_f64(number)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null)
    }
}
