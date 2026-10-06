//! The `google-generative-ai` protocol adapter (R05-T03): the Gemini
//! `generateContent` wire family
//! (`POST {endpoint}/models/{model}:generateContent`, model id
//! URL-encoded, the incumbent's `appendProviderApiPath` join rule).
//!
//! Wire facts (the incumbent `core/llm-client.ts` google branch is the
//! reviewed baseline; the tool-calling half follows the family's standard
//! `functionDeclarations`/`functionCall`/`functionResponse` vocabulary —
//! the incumbent's buffered `callText` path never declared tools, so the
//! tool wire shape is pinned by THIS adapter's goldens, registered in
//! docs/rust-tauri/R05/PROTOCOL_WIRE_MATRIX.json):
//! - Header: `x-goog-api-key` for a bearer credential (NOT
//!   `Authorization`); an `authHeader` config goes verbatim; `none` sends
//!   nothing.
//! - Body: `{systemInstruction?, contents, tools?, generationConfig?}`;
//!   the system prompt is `systemInstruction.parts[].text` (never a
//!   content turn).
//! - Roles: assistant history is `model`; tool results travel as
//!   `functionResponse` parts in a `user` turn, paired by the tool NAME
//!   resolved through the exchange's own tool_call_id → target → wire-name
//!   chain (never a guess; an unpairable result is a loud render failure).
//! - `functionCall.args` is a JSON object (NOT a string); the provider id
//!   round-trips as `id` when the provider issued one.
//! - Schemas ride as `parametersJsonSchema` verbatim (never pruned).
//! - Thought parts (`thought: true`) parse to `Reasoning`; a part's
//!   `thoughtSignature` is protocol state — it parses to an adjacent
//!   opaque block and re-attaches to the SAME part on the way out (C10).
//! - Identity flows ONLY from the driver; response identity fields are
//!   never read (C10).
//!
//! Streaming (R05-T04): production dispatch is the family's SSE mode
//! (`POST {endpoint}/models/{model}:streamGenerateContent?alt=sse`; each
//! frame a partial `GenerateContentResponse`). Frames are decoded
//! INCREMENTALLY as the network delivers them; text/thought parts emit
//! live deltas, functionCall parts ride the CLOSED batch (validated by the
//! SAME [`parse_generate_response`] as the buffered mode), and the stream
//! is trusted ONLY once a terminal frame (a candidate `finishReason` or a
//! `promptFeedback.blockReason`) has arrived — a bare EOF is a truncated
//! stream, never a success.

use std::collections::HashMap;

use lingxi_kernel::model_exchange::{
    ExchangeItem, ModelTurnInput, ResolvedModelRoute, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{ModelTurnDelta, ProviderTurn, ProviderTurnResult, ToolRequest};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::usage::ReportedUsage;
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError};
use serde::Deserialize;

use super::credentials::ApplicableAuth;
use super::dispatch::{self, BearerStyle};
use super::streaming::SseEvent;
use super::tool_render::{outcome_status_word, render_tool_outcome_text};
use super::ParsedChat;

/// The `provider` tag of this family's opaque blocks.
pub const FAMILY: &str = "google-generative-ai";

/// The real HTTP adapter for the google-generative-ai family. Stateless
/// per call; no redirects (C10); error classification shared
/// ([`dispatch::classify_error_status`]).
pub struct GoogleGenerativeAiAdapter {
    client: dispatch::NetworkClient,
    schema_budget: SchemaBudget,
    timeouts: dispatch::HttpTimeouts,
}

impl GoogleGenerativeAiAdapter {
    pub fn new(schema_budget: SchemaBudget) -> Result<Self, ProtocolError> {
        Self::new_with_timeouts(schema_budget, dispatch::HttpTimeouts::default())
    }

    /// Explicit timeout segments (R05-T05; tests inject shorter windows).
    pub fn new_with_timeouts(
        schema_budget: SchemaBudget,
        timeouts: dispatch::HttpTimeouts,
    ) -> Result<Self, ProtocolError> {
        Self::new_with_timeouts_and_network(
            schema_budget,
            timeouts,
            super::network::NetworkPlane::direct_isolated(),
        )
    }

    /// R05 RR1 F14: the client is BOUND to the model plane's reloadable
    /// network policy (proxy / NO_PROXY / explicit CA per configuration
    /// generation; the legacy constructors keep the pre-F14 direct
    /// behavior for isolated tests).
    pub fn new_with_timeouts_and_network(
        schema_budget: SchemaBudget,
        timeouts: dispatch::HttpTimeouts,
        network: std::sync::Arc<super::network::NetworkPlane>,
    ) -> Result<Self, ProtocolError> {
        Ok(Self {
            client: network.client_handle(timeouts)?,
            schema_budget,
            timeouts,
        })
    }

    /// Executes ONE turn (R05-T04: the production wire mode is the family's
    /// SSE stream — `:streamGenerateContent?alt=sse`). Text/thought parts
    /// are emitted to `deltas` AS THE STREAM DELIVERS them; the terminal
    /// batch validates through [`parse_generate_response`] once a terminal
    /// frame (finishReason / promptFeedback.blockReason) has arrived. An
    /// `Err` from the sink means the driver is gone: the read stops and the
    /// call winds down as cancelled.
    pub fn execute_chat<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        route: &'a ResolvedModelRoute,
        auth: &'a ApplicableAuth,
        deltas: &'a dyn lingxi_kernel::ports::TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(self.execute_chat_stream(ctx, call, input, route, auth, deltas))
    }

    async fn execute_chat_stream(
        &self,
        ctx: &RunContext,
        call: &ModelCallId,
        input: &ModelTurnInput,
        route: &ResolvedModelRoute,
        auth: &ApplicableAuth,
        deltas: &dyn lingxi_kernel::ports::TurnDeltaSink,
    ) -> ProviderTurnResult {
        let served_by = || lingxi_kernel::ports::ProviderDescriptor {
            provider: route.provider.clone(),
            model: route.model.clone(),
            operation: route.operation.describe(),
        };
        let fail = |error: ProtocolError, retryable: bool| {
            ProviderTurnResult::of_ctx(ctx, ProviderTurn::Failed { error, retryable })
                .with_served_by(served_by())
        };
        let body = match render_generate_request(input, route) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        // The model id is URL path data — percent-encoded, never raw
        // concatenated (a configured model id can never smuggle path or
        // query syntax).
        let encoded_model = urlencoding_of(&route.model);
        let url = format!(
            "{}?alt=sse",
            dispatch::append_provider_api_path(
                &route.endpoint,
                &format!("/models/{encoded_model}:streamGenerateContent"),
            )
        );
        let client = match self.client.client() {
            Ok(client) => client,
            Err(error) => return fail(error, false),
        };
        let request = dispatch::apply_auth(
            client.post(&url).json(&body),
            auth,
            BearerStyle::NamedHeader("x-goog-api-key"),
        );
        let response = match dispatch::send_with_timeouts(
            request,
            &self.timeouts,
            input.deadline_unix_ms,
            auth,
        )
        .await
        {
            Ok(response) => response,
            Err((error, retryable)) => return fail(error, retryable),
        };
        let accumulator = GenerateStreamAccumulator::new();
        let mut drive_handler = dispatch::FamilyStreamDrive {
            accumulator,
            sink: deltas,
        };
        let drive = dispatch::drive_sse_stream_within_budget(
            response,
            &mut drive_handler,
            input.deadline_unix_ms,
        )
        .await;
        if let Err((error, retryable)) = drive {
            return fail(error, retryable);
        }
        // R05 RR1 F38 (F21 same-path fix): capture the usage the stream
        // observed BEFORE `finish` consumes the accumulator — a parse
        // failure must not erase it.
        let salvaged_usage = drive_handler.accumulator.observed_usage_report();
        match drive_handler
            .accumulator
            .finish(call, &input.tools, &self.schema_budget)
        {
            Ok(parsed) => ProviderTurnResult {
                fence: lingxi_kernel::ports::ResultFence::of_ctx(ctx),
                usage: parsed.usage(),
                turn: parsed.turn,
                usage_report: parsed.usage_report,
                served_protocol: Some(
                    lingxi_kernel::model_exchange::ProtocolFamily::GoogleGenerativeAi
                        .config_name()
                        .to_string(),
                ),
                transport_attempts: 1,
                served_by: Some(served_by()),
            },
            Err(error) => {
                let retryable = error.retryable;
                fail(error, retryable).with_usage_report(salvaged_usage)
            }
        }
    }
}

/// Minimal percent-encoding for a path segment (unreserved characters pass
/// verbatim per RFC 3986; everything else is %HH UTF-8).
fn urlencoding_of(raw: &str) -> String {
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

// ── outbound rendering (pure) ───────────────────────────────────────────────

/// Whether an assistant turn's content carries THIS family's opaque state
/// (R05 RR1 F09): thought signatures, functionCall part anchors and unknown
/// preserved parts are all bound to the provider/model that minted them.
fn turn_carries_family_state(content: &[ContentBlock]) -> bool {
    content
        .iter()
        .any(|block| matches!(block, ContentBlock::Opaque { provider, .. } if provider == FAMILY))
}

/// R05 RR1 F09: the source-authorization gate for this family's opaque
/// state (same contract as the anthropic family's — a family tag is never
/// source authorization).
fn enforce_turn_origin(
    origin: &Option<lingxi_kernel::model_exchange::TurnOrigin>,
    route: &ResolvedModelRoute,
) -> Result<(), ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    match origin {
        Some(origin) if origin.authorizes(&route.provider, &route.model) => Ok(()),
        Some(origin) => Err(invalid(format!(
            "the exchange history carries google-generative-ai opaque state (thought signature / \
             functionCall part state) minted by provider {:?} model {:?}; this request targets \
             provider {:?} model {:?}. Same-protocol-family is NOT source authorization — \
             replaying another origin's signed state would corrupt the protocol round-trip. \
             Restart or compact the session onto one serving model, or re-route the run to the \
             origin model.",
            origin.provider, origin.model, route.provider, route.model
        ))),
        None => Err(invalid(
            "the exchange history carries google-generative-ai opaque state (thought signature / \
             functionCall part state) with no recorded serving origin; unproven protocol state \
             is never forwarded onto a request (restart or compact the session)"
                .to_string(),
        )),
    }
}

/// The wire shape of one exchange tool call: `{name, args}` (+ `id` when the
/// provider issued one), with the part's `thoughtSignature` re-attached
/// when the anchor carried one (R05 RR1 F07).
fn render_function_call_part(
    call: &lingxi_kernel::model_exchange::RequestedToolCall,
    input: &ModelTurnInput,
    signature: Option<&str>,
) -> Result<serde_json::Value, ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let wire_name = input
        .tools
        .wire_name_of_target(&call.target)
        .ok_or_else(|| {
            invalid(format!(
                "exchange history names tool target {:?}, which the current declaration \
                 snapshot does not contain (the registry moved mid-run); cannot faithfully \
                 re-render the transcript",
                call.target
            ))
        })?
        .to_string();
    let mut function_call = serde_json::json!({
        "name": wire_name,
        "args": call.arguments.as_value(),
    });
    if let Some(id) = &call.provider_call_id {
        function_call["id"] = serde_json::Value::String(id.clone());
    }
    let mut part = serde_json::json!({"functionCall": function_call});
    if let Some(signature) = signature {
        part["thoughtSignature"] = serde_json::Value::String(signature.to_string());
    }
    Ok(part)
}

/// Renders one assistant turn's content blocks into this family's parts,
/// re-attaching each thoughtSignature opaque to the part it signed and
/// placing every anchored functionCall at its ORIGINAL part position
/// (R05 RR1 F07 ordering fidelity). `calls_by_id` maps the provider call id
/// to its exchange entry; a consumed anchor marks its call so the caller
/// appends the un-anchored remainder after the content parts (the
/// documented normalization for exchanges built without anchors).
fn render_model_parts(
    content: &[ContentBlock],
    input: &ModelTurnInput,
    calls_by_id: &mut HashMap<String, (lingxi_kernel::model_exchange::RequestedToolCall, bool)>,
) -> Result<Vec<serde_json::Value>, ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let mut parts: Vec<serde_json::Value> = Vec::new();
    let mut index = 0;
    while index < content.len() {
        // The signature of THIS part, when the next block is this family's
        // thoughtSignature opaque.
        let take_signature = |at: usize| -> Option<&str> {
            match content.get(at) {
                Some(ContentBlock::Opaque { provider, data })
                    if provider == FAMILY
                        && data.get("type").and_then(|t| t.as_str())
                            == Some("thoughtSignature") =>
                {
                    data.get("signature").and_then(|s| s.as_str())
                }
                _ => None,
            }
        };
        match &content[index] {
            ContentBlock::Text { text } => {
                let mut part = serde_json::json!({"text": text});
                if let Some(signature) = take_signature(index + 1) {
                    part["thoughtSignature"] = serde_json::Value::String(signature.to_string());
                    index += 1;
                }
                parts.push(part);
            }
            ContentBlock::Reasoning { text } => {
                // Thought parts round-trip with their marker; a signature
                // re-attaches to the SAME part (C10).
                let mut part = serde_json::json!({"text": text, "thought": true});
                if let Some(signature) = take_signature(index + 1) {
                    part["thoughtSignature"] = serde_json::Value::String(signature.to_string());
                    index += 1;
                }
                parts.push(part);
            }
            ContentBlock::Opaque { provider, data } => {
                // Another family's state is never echoed (C10).
                if provider != FAMILY {
                    index += 1;
                    continue;
                }
                match data.get("type").and_then(|t| t.as_str()) {
                    // A bare signature opaque was consumed above (it signed
                    // its predecessor); reaching it here means it is an
                    // orphan with nothing to sign — skipped, never
                    // fabricated onto a part.
                    Some("thoughtSignature") => {}
                    // R05 RR1 F07: an anchored functionCall returns AT ITS
                    // ORIGINAL POSITION, with its signature on the part.
                    Some("functionCallPart") => {
                        let id = data
                            .get("id")
                            .and_then(|i| i.as_str())
                            .ok_or_else(|| {
                                invalid(
                                    "functionCallPart anchor without an id: the position-bound \
                                     call cannot be resolved (never a guess)"
                                        .to_string(),
                                )
                            })?
                            .to_string();
                        let entry = calls_by_id.get_mut(&id).ok_or_else(|| {
                            invalid(format!(
                                "functionCallPart anchor names call id {id:?} which this \
                                 exchange's tool calls do not contain; the anchor/call pairing \
                                 is inconsistent (never a guess)"
                            ))
                        })?;
                        if entry.1 {
                            return Err(invalid(format!(
                                "functionCallPart anchor names call id {id:?} twice: a \
                                 duplicated anchor is a conflict, never merged"
                            )));
                        }
                        entry.1 = true;
                        let signature = data.get("signature").and_then(|s| s.as_str());
                        parts.push(render_function_call_part(
                            &entry.0.clone(),
                            input,
                            signature,
                        )?);
                    }
                    // Any other preserved part round-trips verbatim.
                    _ => parts.push(data.clone()),
                }
            }
            ContentBlock::ResourceRef { resource } => {
                let text = match &resource.uri {
                    Some(uri) => format!("[resource: {uri}]"),
                    None => "[resource]".to_string(),
                };
                parts.push(serde_json::json!({"text": text}));
            }
        }
        index += 1;
    }
    Ok(parts)
}

/// Builds the request body for one turn (pure — the testable half).
pub fn render_generate_request(
    input: &ModelTurnInput,
    route: &ResolvedModelRoute,
) -> Result<serde_json::Value, ProtocolError> {
    let _ = route; // the URL (not the body) carries the model on this family
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let mut contents: Vec<serde_json::Value> = Vec::new();
    // R05-T06: a turn carrying host-authorized images appends the
    // `inlineData` parts (the incumbent `googlePartsFromContent` shape);
    // a text-only turn keeps the single-text-part shape byte-identically.
    let mut submission_parts = vec![serde_json::json!({"text": input.submission})];
    for image in &input.images {
        submission_parts.push(serde_json::json!({
            "inlineData": {
                "mimeType": image.mime,
                "data": super::image_base64(image),
            },
        }));
    }
    contents.push(serde_json::json!({
        "role": "user",
        "parts": submission_parts,
    }));
    // The pairing chain of THIS exchange: host tool_call_id → the wire
    // name + provider id its call carried (functionResponse pairs by name,
    // so the name must come from the exchange's own history — never a
    // guess).
    let mut call_names: HashMap<String, (String, Option<String>)> = HashMap::new();
    for item in &input.prior {
        match item {
            ExchangeItem::AssistantTurn {
                content,
                tool_calls,
                origin,
                ..
            } => {
                // R05 RR1 F09: opaque state (thought signatures, part
                // anchors, preserved parts) is source-bound — check before
                // any of it could be echoed.
                if turn_carries_family_state(content) {
                    enforce_turn_origin(origin, route)?;
                }
                // The position-anchored rendering map: provider call id →
                // (the exchange call, consumed-by-anchor yet?).
                let mut calls_by_id: HashMap<
                    String,
                    (lingxi_kernel::model_exchange::RequestedToolCall, bool),
                > = HashMap::new();
                for call in tool_calls {
                    if let Some(id) = &call.provider_call_id {
                        calls_by_id.insert(id.clone(), (call.clone(), false));
                    }
                }
                let mut parts = render_model_parts(content, input, &mut calls_by_id)?;
                // R05 RR1 F07: calls WITHOUT a position anchor (id-less
                // wire shapes, manually built exchanges) keep the
                // documented content-first order.
                for call in tool_calls {
                    let anchored = call
                        .provider_call_id
                        .as_ref()
                        .is_some_and(|id| calls_by_id.get(id).is_some_and(|e| e.1));
                    if anchored {
                        continue;
                    }
                    let wire_name = input
                        .tools
                        .wire_name_of_target(&call.target)
                        .ok_or_else(|| {
                            invalid(format!(
                                "exchange history names tool target {:?}, which the current \
                                 declaration snapshot does not contain (the registry moved \
                                 mid-run); cannot faithfully re-render the transcript",
                                call.target
                            ))
                        })?
                        .to_string();
                    let mut function_call = serde_json::json!({
                        "name": wire_name,
                        "args": call.arguments.as_value(),
                    });
                    if let Some(id) = &call.provider_call_id {
                        function_call["id"] = serde_json::Value::String(id.clone());
                    }
                    call_names.insert(
                        call.tool_call_id.to_string(),
                        (wire_name.clone(), call.provider_call_id.clone()),
                    );
                    parts.push(serde_json::json!({"functionCall": function_call}));
                }
                // Anchored calls still register their result-pairing name.
                for call in tool_calls {
                    let anchored = call
                        .provider_call_id
                        .as_ref()
                        .is_some_and(|id| calls_by_id.get(id).is_some_and(|e| e.1));
                    if !anchored {
                        continue;
                    }
                    let wire_name = input
                        .tools
                        .wire_name_of_target(&call.target)
                        .ok_or_else(|| {
                            invalid(format!(
                                "exchange history names tool target {:?}, which the current \
                                 declaration snapshot does not contain (the registry moved \
                                 mid-run); cannot faithfully re-render the transcript",
                                call.target
                            ))
                        })?
                        .to_string();
                    call_names.insert(
                        call.tool_call_id.to_string(),
                        (wire_name, call.provider_call_id.clone()),
                    );
                }
                if parts.is_empty() {
                    continue;
                }
                contents.push(serde_json::json!({"role": "model", "parts": parts}));
            }
            ExchangeItem::ToolResult {
                tool_call_id,
                outcome,
                ..
            } => {
                let (wire_name, provider_id) = call_names
                    .get(&tool_call_id.to_string())
                    .cloned()
                    .ok_or_else(|| {
                    invalid(format!(
                        "exchange history tool result {tool_call_id} has no matching tool \
                             call in this exchange; the functionResponse pairing cannot be \
                             reconstructed (never a guessed name)"
                    ))
                })?;
                let mut function_response = serde_json::json!({
                    "name": wire_name,
                    "response": {
                        "status": outcome_status_word(outcome),
                        "content": render_tool_outcome_text(outcome),
                    },
                });
                if let Some(id) = provider_id {
                    function_response["id"] = serde_json::Value::String(id);
                }
                let block = serde_json::json!({"functionResponse": function_response});
                // R05 RR1 F07: the results of ONE model tool round travel
                // as the parts of ONE user Content (the family's official
                // parallel grouping) — consecutive results merge into the
                // user turn the previous result opened.
                let merged = matches!(
                    contents.last(),
                    Some(last) if last["role"] == "user"
                        && last["parts"]
                            .as_array()
                            .is_some_and(|parts| {
                                !parts.is_empty()
                                    && parts.iter().all(|p| p.get("functionResponse").is_some())
                            })
                );
                if merged {
                    let last = contents.last_mut().expect("checked above");
                    last["parts"]
                        .as_array_mut()
                        .expect("checked above")
                        .push(block);
                } else {
                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": [block],
                    }));
                }
            }
        }
    }
    let declarations: Vec<serde_json::Value> = input
        .tools
        .declarations
        .iter()
        .map(|declaration| {
            serde_json::json!({
                "name": declaration.wire_name,
                "description": declaration.description,
                "parametersJsonSchema": declaration.input_schema.schema,
            })
        })
        .collect();
    let mut body = serde_json::json!({ "contents": contents });
    if let Some(system) = &input.system_prompt {
        body["systemInstruction"] = serde_json::json!({"parts": [{"text": system}]});
    }
    // R05-T06: the host-decided output cap is the incumbent
    // `generationConfig.maxOutputTokens` (only present when decided).
    if let Some(cap) = input.max_output_tokens {
        body["generationConfig"] = serde_json::json!({"maxOutputTokens": cap});
    }
    if !declarations.is_empty() {
        body["tools"] = serde_json::json!([{"functionDeclarations": declarations}]);
    }
    Ok(body)
}

// ── inbound parsing (pure) ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    // R05-T07: usageMetadata is decoded from the raw body by the STRICT
    // per-family decoder (no typed field here — see MessagesResponse).
    #[serde(default, rename = "promptFeedback")]
    prompt_feedback: Option<PromptFeedback>,
}

#[derive(Debug, Deserialize)]
struct PromptFeedback {
    #[serde(default, rename = "blockReason")]
    block_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    #[serde(default)]
    content: Option<CandidateContent>,
    #[serde(default, rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CandidateContent {
    #[serde(default)]
    parts: Vec<serde_json::Value>,
}

/// Parses a successful response into the provider turn + usage. Loud on
/// every malformed shape; identity fields are never read (C10).
pub fn parse_generate_response(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    body: &serde_json::Value,
    schema_budget: &SchemaBudget,
) -> Result<ParsedChat, ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let response: GenerateResponse = serde_json::from_value(body.clone()).map_err(|err| {
        invalid(format!(
            "provider response violates the response schema: {err}"
        ))
    })?;
    // R05-T07: the strict per-family decode (cachedContent/thoughts as
    // documented components + provenance + invalid-vs-unknown).
    let usage_report = match super::usage::decode_family_usage(
        lingxi_kernel::model_exchange::ProtocolFamily::GoogleGenerativeAi,
        body,
    ) {
        super::usage::UsageDecode::Absent => ReportedUsage::Unknown,
        super::usage::UsageDecode::Usage(usage) => ReportedUsage::Known(usage),
        super::usage::UsageDecode::Invalid { detail } => ReportedUsage::Invalid { detail },
    };
    // C11 stop-reason honesty (R05-T04): a provider-blocked prompt is a
    // refusal classification, never a zero-candidates protocol error.
    if let Some(feedback) = &response.prompt_feedback {
        if let Some(reason) = &feedback.block_reason {
            return Ok(ParsedChat {
                turn: ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::Forbidden,
                        format!(
                            "provider blocked the prompt (promptFeedback.blockReason: {reason}); \
                             not retryable as-is"
                        ),
                        false,
                    ),
                    retryable: false,
                },
                usage_report: usage_report.clone(),
            });
        }
    }
    let candidate = response
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| invalid("provider response carries zero candidates".to_string()))?;
    let mut content: Vec<ContentBlock> = Vec::new();
    let mut requests: Vec<ToolRequest> = Vec::new();
    let parts = candidate
        .content
        .map(|content| content.parts)
        .unwrap_or_default();
    for part in &parts {
        if let Some(function_call) = part.get("functionCall") {
            let name = function_call
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| invalid("functionCall without a name".to_string()))?;
            let target = snapshot
                .target_of_wire_name(name)
                .ok_or_else(|| {
                    invalid(format!(
                        "provider requested unknown tool {name:?}: not a name this call \
                         declared (a wire name maps to a target through the send-time \
                         snapshot only — never a guess)"
                    ))
                })?
                .to_string();
            let arguments = match function_call.get("args") {
                None | Some(serde_json::Value::Null) => serde_json::json!({}),
                Some(value @ serde_json::Value::Object(_)) => value.clone(),
                Some(_) => {
                    return Err(invalid(format!(
                        "functionCall args for {name:?} are not an object"
                    )));
                }
            };
            let mut request =
                ToolRequest::from_effective_arguments(target, arguments, schema_budget)
                    .ok_or_else(|| {
                        invalid(format!(
                            "tool call arguments for {name:?} violate the argument invariants"
                        ))
                    })?;
            if let Some(id) = function_call.get("id").and_then(|i| i.as_str()) {
                request = request.with_provider_call_id(id);
            }
            let anchor_id = request.provider_call_id.clone();
            requests.push(request);
            // R05 RR1 F07: the functionCall PART is an ordered position of
            // the model turn — the exchange keeps an anchor opaque HERE so
            // the renderer can put the call (and its signature, when the
            // part carried one) back at this exact position. A
            // `thoughtSignature` riding the functionCall part is required
            // protocol state: it must return ON the function part of the
            // next request, byte for byte. (An id-less functionCall cannot
            // be position-bound — those calls keep the documented
            // content-first normalization.)
            if let Some(id) = &anchor_id {
                let mut anchor = serde_json::json!({"type": "functionCallPart", "id": id.clone()});
                if let Some(signature) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
                    anchor["signature"] = serde_json::Value::String(signature.to_string());
                }
                content.push(ContentBlock::Opaque {
                    provider: FAMILY.to_string(),
                    data: anchor,
                });
            }
            continue;
        }
        let text = part.get("text").and_then(|t| t.as_str());
        match text {
            Some(text) if !text.is_empty() => {
                if part.get("thought").and_then(|t| t.as_bool()) == Some(true) {
                    content.push(ContentBlock::Reasoning {
                        text: text.to_string(),
                    });
                } else {
                    content.push(ContentBlock::Text {
                        text: text.to_string(),
                    });
                }
            }
            _ => {
                // A part with neither text nor functionCall is unknown
                // provider state — preserved verbatim, never dropped.
                content.push(ContentBlock::Opaque {
                    provider: FAMILY.to_string(),
                    data: part.clone(),
                });
                continue;
            }
        }
        // The signature is protocol state riding the part it signed.
        if let Some(signature) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
            content.push(ContentBlock::Opaque {
                provider: FAMILY.to_string(),
                data: serde_json::json!({
                    "type": "thoughtSignature",
                    "signature": signature,
                }),
            });
        }
    }
    // R05 RR1 F11: the batch-level identity admission — a same-id re-send
    // with identical shape collapses, a same-id CONFLICT rejects the whole
    // turn (zero requests admitted, zero side effects).
    let requests = super::batch_admission::admit_provider_call_ids(FAMILY, requests)?;
    // C11 stop-reason honesty (R05-T04) + R05 RR1 F12: the transport's end
    // is not protocol completion. The CLOSED turn classifies ONLY by a
    // KNOWN normal `finishReason`: its absence, or an unmapped value, is a
    // protocol surprise — never a guessed Final, never a dispatched batch.
    // A truncated or safety-stopped turn never forms a Final and never
    // dispatches its tool batch; the partial content lives on in the call's
    // delta events (zero side effects → a truncation retry is safe).
    match candidate.finish_reason.as_deref() {
        Some("STOP") => {}
        Some("MAX_TOKENS") => {
            return Ok(ParsedChat {
                turn: ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::BudgetExceeded,
                        "provider stopped at the token budget (finishReason: MAX_TOKENS): the \
                         turn is truncated; the partial content stays in the call's delta \
                         events and nothing from this turn was dispatched"
                            .to_string(),
                        true,
                    ),
                    retryable: true,
                },
                usage_report: usage_report.clone(),
            });
        }
        Some(
            reason @ ("SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII"
            | "IMAGE_SAFETY"),
        ) => {
            return Ok(ParsedChat {
                turn: ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::Forbidden,
                        format!(
                            "provider refused the turn through its safety/recitation filter \
                             (finishReason: {reason}); not retryable as-is"
                        ),
                        false,
                    ),
                    retryable: false,
                },
                usage_report: usage_report.clone(),
            });
        }
        Some("MALFORMED_FUNCTION_CALL") => {
            return Ok(ParsedChat {
                turn: ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::InvalidMessage,
                        "provider reported MALFORMED_FUNCTION_CALL: the model's own function \
                         call was malformed (the family's terminal honesty signal); not \
                         retryable as-is"
                            .to_string(),
                        false,
                    ),
                    retryable: false,
                },
                usage_report: usage_report.clone(),
            });
        }
        Some(other) => {
            return Err(invalid(format!(
                "provider response carries the UNKNOWN finishReason {other:?}: an unmapped \
                 terminal is never guessed into a completed turn"
            )));
        }
        None => {
            return Err(invalid(
                "provider response ended WITHOUT a finishReason: the transport's end is not \
                 protocol completion — the turn never classifies, nothing dispatches"
                    .to_string(),
            ));
        }
    }
    if !requests.is_empty() {
        return Ok(ParsedChat {
            turn: ProviderTurn::ToolRequests { requests, content },
            usage_report: usage_report.clone(),
        });
    }
    // R05 RR1 F12: a normally-stopped turn whose parts carry NO answer text
    // (thought/opaque only) is process content, not a final answer.
    let has_answer_text = content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { .. }));
    if has_answer_text {
        return Ok(ParsedChat {
            turn: ProviderTurn::Final {
                message: NormalizedMessage {
                    role: "assistant".to_string(),
                    content,
                    model_call_id: Some(call.clone()),
                },
            },
            usage_report: usage_report.clone(),
        });
    }
    let detail = if content.is_empty() {
        format!(
            "provider returned no content and no tool calls (finishReason: {})",
            candidate.finish_reason.as_deref().unwrap_or("absent")
        )
    } else {
        format!(
            "provider turn completed with process-only content (thought/opaque state, no \
             answer text; finishReason: {})",
            candidate.finish_reason.as_deref().unwrap_or("absent")
        )
    };
    Ok(ParsedChat {
        turn: ProviderTurn::Empty {
            detail,
            // R05 RR1 F12: the process-state blocks ride along for replay.
            content,
        },
        usage_report: usage_report.clone(),
    })
}

// ── streaming decode (R05-T04) ──────────────────────────────────────────────

/// The incremental accumulator of one `:streamGenerateContent?alt=sse`
/// stream (R05-T04): each SSE frame is a partial `GenerateContentResponse`.
/// [`GenerateStreamAccumulator::handle_event`] consumes each frame as it
/// arrives (text/thought parts emit live deltas; `functionCall` parts are
/// batch state — complete per part on this family, bounded transitively by
/// the per-frame bound) and [`GenerateStreamAccumulator::finish`] validates
/// the CLOSED stream through the same buffered parser
/// ([`parse_generate_response`]).
///
/// Terminal honesty (C11/C15): this family has NO sentinel frame — the
/// trusted terminal condition is a frame carrying a candidate
/// `finishReason` or a `promptFeedback.blockReason`. A bare EOF without one
/// is a truncated stream: loud, never half-parsed. Text fragments merge
/// with the immediately preceding part of the same thought-ness (the
/// buffered equivalence: one logical part arrives split over frames);
/// parts carrying a `thoughtSignature` never merge (the signature stays on
/// the part it signed). Conflicting finishReason/usage re-sends and any
/// CONTENT frame after the terminal are loud (C10).
pub struct GenerateStreamAccumulator {
    parts: Vec<serde_json::Value>,
    finish_reason: Option<String>,
    // R05-T07 (C04): streaming usageMetadata frames are RUNNING cumulative
    // snapshots — decoded and folded per the family's RunningTotal mode (a
    // growing frame replaces, an identical repeat is a no-op, a shrink is
    // loud; never summed).
    usage: lingxi_kernel::usage::UsageFolder,
    // R05-T07 fix (REVIEW-T07 F-01): a usageMetadata frame whose numbers
    // violate the contract (negative / float / string count) is NEVER
    // silently dropped. The raw frame is kept here and spliced into the
    // buffered finish body, where the SAME strict decoder marks the whole
    // fact invalid and names the violation (C06) — identical to the
    // buffered wire mode. Sticky: the FIRST violation wins.
    usage_violation: Option<serde_json::Value>,
    prompt_block_reason: Option<String>,
    terminal_seen: bool,
}

impl Default for GenerateStreamAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl GenerateStreamAccumulator {
    pub fn new() -> Self {
        Self {
            parts: Vec::new(),
            finish_reason: None,
            usage: lingxi_kernel::usage::UsageFolder::new(
                lingxi_kernel::usage::UsageAggregationMode::RunningTotal,
            ),
            usage_violation: None,
            prompt_block_reason: None,
            terminal_seen: false,
        }
    }

    /// R05 RR1 F38: the usage fact folded so far (the running-total fold
    /// cloned and finished — a half-known account stays `Partial` naming
    /// its missing half) — the salvage a parse-failed turn still accounts
    /// with. A kept violation re-enters the SAME strict decoder as
    /// `Invalid`. Never a guess: nothing observed stays `Unknown`.
    pub fn observed_usage_report(&self) -> lingxi_kernel::usage::ReportedUsage {
        if let Some(violation) = &self.usage_violation {
            return super::usage::salvage_usage_report(
                lingxi_kernel::model_exchange::ProtocolFamily::GoogleGenerativeAi,
                Some(violation),
            );
        }
        match self.usage.clone().finish() {
            Some(usage) => lingxi_kernel::usage::ReportedUsage::Known(usage),
            None => lingxi_kernel::usage::ReportedUsage::Unknown,
        }
    }

    /// Consumes one decoded SSE event; returns the live deltas it produced.
    pub fn handle_event(&mut self, event: &SseEvent) -> Result<Vec<ModelTurnDelta>, ProtocolError> {
        let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
        let mut emitted = Vec::new();
        let data = event.data.trim();
        if data.is_empty() {
            return Ok(emitted);
        }
        let frame: serde_json::Value = serde_json::from_str(data)
            .map_err(|err| invalid(format!("stream frame is not JSON: {err}")))?;
        // Content is late only when the terminal arrived in an EARLIER
        // frame (a terminal frame may still carry its own final parts).
        let terminal_before = self.terminal_seen;
        if let Some(reason) = frame
            .get("promptFeedback")
            .and_then(|f| f.get("blockReason"))
            .and_then(|r| r.as_str())
        {
            match &self.prompt_block_reason {
                Some(existing) if existing != reason => {
                    return Err(invalid(format!(
                        "stream carried two different promptFeedback.blockReason values \
                         ({existing:?} then {reason:?}): a conflict is never merged"
                    )));
                }
                _ => self.prompt_block_reason = Some(reason.to_string()),
            }
            self.terminal_seen = true;
        }
        if let Some(usage) = frame.get("usageMetadata").filter(|u| !u.is_null()) {
            // Absent halves fold nothing; a contract-violating count is
            // kept RAW (usage_violation) and spliced into the buffered
            // finish body, so the buffered parse marks the fact invalid
            // there — never a silent drop (C06).
            match super::usage::decode_family_usage(
                lingxi_kernel::model_exchange::ProtocolFamily::GoogleGenerativeAi,
                &serde_json::json!({"usageMetadata": usage}),
            ) {
                super::usage::UsageDecode::Usage(fact) => {
                    self.usage.fold(fact).map_err(|conflict| {
                        invalid(format!(
                            "usageMetadata conflicts with the running snapshot: {conflict}"
                        ))
                    })?;
                }
                super::usage::UsageDecode::Invalid { .. } => {
                    self.usage_violation.get_or_insert_with(|| usage.clone());
                }
                super::usage::UsageDecode::Absent => {}
            }
        }
        let candidates = frame
            .get("candidates")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        if candidates.len() > 1 {
            return Err(invalid(format!(
                "stream frame carries {} candidates: this adapter never requests more than \
                 one, a multi-candidate stream is a protocol surprise",
                candidates.len()
            )));
        }
        if let Some(candidate) = candidates.into_iter().next() {
            if terminal_before {
                return Err(invalid(
                    "candidate content arrived after the terminal frame (finishReason / \
                     blockReason already seen): a late content frame is never silently \
                     absorbed"
                        .to_string(),
                ));
            }
            if let Some(index) = candidate.get("index").and_then(|i| i.as_u64()) {
                if index != 0 {
                    return Err(invalid(format!(
                        "stream carries candidate index {index}: a multi-candidate stream is \
                         a protocol surprise"
                    )));
                }
            }
            if let Some(parts) = candidate
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                for part in parts {
                    if part.get("functionCall").is_some() {
                        // Complete-per-part on this family (arguments are a
                        // JSON object in one frame — bounded transitively by
                        // the per-frame SSE bound); batch state, never live
                        // progress.
                        self.parts.push(part.clone());
                        continue;
                    }
                    let text = part.get("text").and_then(|t| t.as_str());
                    let thought = part.get("thought").and_then(|t| t.as_bool()) == Some(true);
                    match text {
                        Some(fragment) if !fragment.is_empty() => {
                            emitted.push(if thought {
                                ModelTurnDelta::Reasoning(fragment.to_string())
                            } else {
                                ModelTurnDelta::Text(fragment.to_string())
                            });
                            // Buffered equivalence: a fragment merges into the
                            // preceding part of the same thought-ness — never
                            // across a signature boundary (the signature stays
                            // on the part it signed).
                            let signed = part.get("thoughtSignature").is_some();
                            let mergeable = !signed
                                && matches!(
                                    self.parts.last_mut(),
                                    Some(last) if last.get("text").is_some()
                                        && last.get("functionCall").is_none()
                                        && (last.get("thought").and_then(|t| t.as_bool()) == Some(true)) == thought
                                        && last.get("thoughtSignature").is_none()
                                );
                            if mergeable {
                                let last = self.parts.last_mut().expect("checked above");
                                let merged = format!(
                                    "{}{}",
                                    last.get("text").and_then(|t| t.as_str()).expect("text"),
                                    fragment
                                );
                                last["text"] = serde_json::Value::String(merged);
                            } else {
                                self.parts.push(part.clone());
                            }
                        }
                        _ => {
                            // A part with neither text nor functionCall is
                            // unknown provider state — preserved verbatim.
                            self.parts.push(part.clone());
                        }
                    }
                }
            }
            if let Some(reason) = candidate.get("finishReason").and_then(|r| r.as_str()) {
                match &self.finish_reason {
                    Some(existing) if existing != reason => {
                        return Err(invalid(format!(
                            "stream carried two different finishReason values ({existing:?} \
                             then {reason:?}): a conflict is never merged"
                        )));
                    }
                    _ => self.finish_reason = Some(reason.to_string()),
                }
                self.terminal_seen = true;
            }
        }
        Ok(emitted)
    }

    /// Validates the CLOSED stream through the SAME buffered parser. A
    /// stream without a terminal frame is truncated: loud, never
    /// half-parsed (C05/C09/C11 — a bare socket close is never a success).
    pub fn finish(
        self,
        call: &ModelCallId,
        snapshot: &ToolDeclarationSnapshot,
        schema_budget: &SchemaBudget,
    ) -> Result<ParsedChat, ProtocolError> {
        let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
        if !self.terminal_seen {
            return Err(invalid(
                "stream ended without a terminal frame (no finishReason, no \
                 promptFeedback.blockReason): a truncated stream is never half-parsed into a \
                 turn"
                    .to_string(),
            ));
        }
        let mut candidate = serde_json::json!({
            "content": {"role": "model", "parts": self.parts},
        });
        if let Some(reason) = self.finish_reason {
            candidate["finishReason"] = serde_json::Value::String(reason);
        }
        let mut body = serde_json::json!({ "candidates": [candidate] });
        // R05-T07: the folded running snapshot splices back into the
        // buffered shape; the buffered parse decodes it through the SAME
        // strict decoder (identical semantics in both wire modes). A
        // contract-violating frame (F-01) splices its RAW numbers through
        // the same body — the buffered decode marks the WHOLE fact
        // invalid and names the violation; a violating count never
        // disappears silently.
        //
        // R05 RR1 F23 round-trip note: the FOLD is the UNIFIED fact
        // (`output_tokens` already contains the thoughts component), but
        // the buffered decode re-applies the family normalization
        // (`candidates + thoughts`). The splice therefore expresses the
        // folded output as its CANDIDATE part (`output - reasoning`) so
        // decoding the spliced body reproduces the fold exactly — no
        // double add, no lost component. The subtraction is exact by the
        // fold's own invariant: every folded frame satisfied
        // `output = candidates + thoughts` with non-negative parts, and
        // running-total merges only grow fields.
        let folded = self.usage.clone().finish();
        let mut usage_metadata = serde_json::json!({});
        if let Some(folded) = folded {
            if let Some(tokens) = folded.input_tokens {
                usage_metadata["promptTokenCount"] = serde_json::json!(tokens);
            }
            match (folded.output_tokens, folded.reasoning_tokens) {
                (Some(output), Some(reasoning)) => {
                    usage_metadata["candidatesTokenCount"] =
                        serde_json::json!(output.saturating_sub(reasoning));
                    usage_metadata["thoughtsTokenCount"] = serde_json::json!(reasoning);
                }
                (Some(output), None) => {
                    usage_metadata["candidatesTokenCount"] = serde_json::json!(output);
                }
                (None, Some(reasoning)) => {
                    usage_metadata["thoughtsTokenCount"] = serde_json::json!(reasoning);
                }
                (None, None) => {}
            }
            if let Some(tokens) = folded.cache_read_tokens {
                usage_metadata["cachedContentTokenCount"] = serde_json::json!(tokens);
            }
        }
        if let Some(violation) = &self.usage_violation {
            if let (Some(target), Some(source)) =
                (usage_metadata.as_object_mut(), violation.as_object())
            {
                for (field, value) in source {
                    target.insert(field.clone(), value.clone());
                }
            }
        }
        if !usage_metadata
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
        {
            body["usageMetadata"] = usage_metadata;
        }
        if let Some(reason) = self.prompt_block_reason {
            body["promptFeedback"] = serde_json::json!({"blockReason": reason});
        }
        parse_generate_response(call, snapshot, &body, schema_budget)
    }
}

impl super::dispatch::StreamAccumulator for GenerateStreamAccumulator {
    fn handle_event(&mut self, event: &SseEvent) -> Result<Vec<ModelTurnDelta>, ProtocolError> {
        GenerateStreamAccumulator::handle_event(self, event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_kernel::model_exchange::{
        CredentialAuthKind, CredentialReference, ModelOperation, ProtocolFamily, ToolDeclaration,
        ToolDeclarationSnapshot,
    };
    use lingxi_kernel::ports::ToolOutcome;
    use lingxi_protocol::ToolCallId;
    use lingxi_protocol::UsageRecord;

    fn origin() -> Option<lingxi_kernel::model_exchange::TurnOrigin> {
        Some(lingxi_kernel::model_exchange::TurnOrigin {
            provider: "gemini".to_string(),
            model: "gemini-test".to_string(),
        })
    }

    fn route() -> ResolvedModelRoute {
        ResolvedModelRoute {
            provider: "gemini".to_string(),
            model: "gemini-test".to_string(),
            operation: ModelOperation::Chat,
            protocol: ProtocolFamily::GoogleGenerativeAi,
            endpoint: "https://generativelanguage.example.test".to_string(),
            credential: CredentialReference {
                provider: "gemini".to_string(),
                auth: CredentialAuthKind::ApiKey,
            },
            config_generation: 1,
            group_id: None,
        }
    }

    fn snapshot_with_read() -> ToolDeclarationSnapshot {
        ToolDeclarationSnapshot {
            catalog_generation: 3,
            declarations: vec![ToolDeclaration {
                target: "tool:first-party:read".to_string(),
                wire_name: "read".to_string(),
                description: "Read a file".to_string(),
                input_schema: lingxi_protocol::ToolSchemaDocument {
                    dialect: "json-schema/2020-12".to_string(),
                    schema: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                },
            }],
        }
    }

    #[test]
    fn request_shape_system_instruction_and_function_declarations() {
        let mut input = ModelTurnInput::first_turn("hello", snapshot_with_read());
        input.system_prompt = Some("be terse".to_string());
        let body = render_generate_request(&input, &route()).expect("renders");
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "hello");
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be terse");
        assert_eq!(body["tools"][0]["functionDeclarations"][0]["name"], "read");
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["properties"]
                ["path"]["type"],
            "string"
        );
        let bare = render_generate_request(
            &ModelTurnInput::first_turn("hi", ToolDeclarationSnapshot::empty()),
            &route(),
        )
        .expect("renders");
        assert!(bare.get("systemInstruction").is_none());
        assert!(bare.get("tools").is_none());
    }

    #[test]
    fn tool_roundtrip_pairs_function_responses_by_exchange_name() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:read",
            serde_json::json!({"path": "/tmp/a"}),
            &budget,
        )
        .expect("effective")
        .with_provider_call_id("fc-1");
        let mut input = ModelTurnInput::first_turn("read it", snapshot);
        input.prior.push(ExchangeItem::AssistantTurn {
            call: ModelCallId::new("mc-1"),
            content: vec![],
            tool_calls: vec![lingxi_kernel::model_exchange::RequestedToolCall {
                tool_call_id: ToolCallId::new("run-tc0001"),
                provider_call_id: request.provider_call_id.clone(),
                target: request.target.clone(),
                arguments: request.arguments.clone(),
                args_digest: request.args_digest.clone(),
                args_summary: None,
            }],
            origin: origin(),
        });
        input.prior.push(ExchangeItem::ToolResult {
            tool_call_id: ToolCallId::new("run-tc0001"),
            provider_call_id: Some("fc-1".to_string()),
            outcome: ToolOutcome::success_text("file body"),
        });
        let body = render_generate_request(&input, &route()).expect("renders");
        let model = &body["contents"][1];
        assert_eq!(model["role"], "model");
        assert_eq!(
            model["parts"][0]["functionCall"],
            serde_json::json!({"name": "read", "args": {"path": "/tmp/a"}, "id": "fc-1"})
        );
        let result = &body["contents"][2];
        assert_eq!(result["role"], "user");
        assert_eq!(
            result["parts"][0]["functionResponse"],
            serde_json::json!({
                "name": "read",
                "id": "fc-1",
                "response": {"status": "succeeded", "content": "file body"}
            })
        );
    }

    #[test]
    fn an_unpairable_tool_result_is_a_loud_render_failure() {
        let mut input = ModelTurnInput::first_turn("hi", snapshot_with_read());
        input.prior.push(ExchangeItem::ToolResult {
            tool_call_id: ToolCallId::new("run-tc9999"),
            provider_call_id: None,
            outcome: ToolOutcome::success_text("orphan"),
        });
        let err = render_generate_request(&input, &route()).unwrap_err();
        assert!(err.message.contains("no matching tool call"));
    }

    #[test]
    fn thoughts_signatures_and_function_calls_parse_in_order() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0001");
        let body = serde_json::json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "checking", "thought": true, "thoughtSignature": "sig-g1"},
                    {"text": "reading it"},
                    {"functionCall": {"name": "read", "args": {"path": "/tmp/a"}, "id": "fc-9"}}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 12, "candidatesTokenCount": 6, "thoughtsTokenCount": 3}
        });
        let parsed = parse_generate_response(&call, &snapshot, &body, &budget).expect("parses");
        assert_eq!(
            parsed.usage(),
            Some(UsageRecord {
                input_tokens: 12,
                // RR1 F23: the unified output is candidates (6) + thoughts
                // (3) — the total generated output.
                output_tokens: 9
            })
        );
        let (requests, content) = match parsed.turn {
            ProviderTurn::ToolRequests { requests, content } => (requests, content),
            other => panic!("expected ToolRequests, got {other:?}"),
        };
        assert_eq!(requests[0].provider_call_id.as_deref(), Some("fc-9"));
        assert_eq!(requests[0].target, "tool:first-party:read");
        // R05 RR1 F07: the functionCall part leaves an ordered anchor opaque
        // at its part position (after the thought + signature + text).
        assert_eq!(content.len(), 4);
        assert!(matches!(&content[0], ContentBlock::Reasoning { text } if text == "checking"));
        assert!(
            matches!(&content[1], ContentBlock::Opaque { provider, data }
            if provider == FAMILY && data["type"] == "thoughtSignature" && data["signature"] == "sig-g1")
        );
        assert!(matches!(&content[2], ContentBlock::Text { text } if text == "reading it"));
        assert!(
            matches!(&content[3], ContentBlock::Opaque { provider, data }
            if provider == FAMILY && data["type"] == "functionCallPart" && data["id"] == "fc-9")
        );

        // The same state re-attaches to the SAME part on the way out (C10).
        let mut input = ModelTurnInput::first_turn("go on", snapshot);
        input.prior.push(ExchangeItem::AssistantTurn {
            call: call.clone(),
            content: content.clone(),
            tool_calls: vec![lingxi_kernel::model_exchange::RequestedToolCall {
                tool_call_id: ToolCallId::new("run-tc0001"),
                provider_call_id: requests[0].provider_call_id.clone(),
                target: requests[0].target.clone(),
                arguments: requests[0].arguments.clone(),
                args_digest: requests[0].args_digest.clone(),
                args_summary: None,
            }],
            origin: origin(),
        });
        let rendered = render_generate_request(&input, &route()).expect("renders");
        let parts = rendered["contents"][1]["parts"].as_array().expect("parts");
        assert_eq!(
            parts[0],
            serde_json::json!({"text": "checking", "thought": true, "thoughtSignature": "sig-g1"})
        );
        assert_eq!(parts[1], serde_json::json!({"text": "reading it"}));
        assert_eq!(
            parts[2]["functionCall"],
            serde_json::json!({"name": "read", "args": {"path": "/tmp/a"}, "id": "fc-9"})
        );
    }

    #[test]
    fn model_ids_are_percent_encoded_into_the_path() {
        assert_eq!(urlencoding_of("gemini-2.5-pro"), "gemini-2.5-pro");
        assert_eq!(urlencoding_of("a/b?c"), "a%2Fb%3Fc");
        let url = dispatch::append_provider_api_path(
            "https://generativelanguage.example.test",
            &format!("/models/{}:generateContent", urlencoding_of("a/b?c")),
        );
        assert_eq!(
            url,
            "https://generativelanguage.example.test/models/a%2Fb%3Fc:generateContent"
        );
    }
}
