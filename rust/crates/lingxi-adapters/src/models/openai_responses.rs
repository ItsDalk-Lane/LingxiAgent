//! The `openai-responses` protocol adapter (R05-T03): the OpenAI Responses
//! API wire family (`POST {endpoint}/responses`).
//!
//! Wire facts (the incumbent `core/llm-client.ts` openai-responses branch
//! is the reviewed baseline; the tool-calling half follows the family's
//! standard `function` tool / `function_call` / `function_call_output`
//! vocabulary — the incumbent's buffered `callText` path never declared
//! tools, so the tool wire shape is pinned by THIS adapter's goldens,
//! registered in docs/rust-tauri/R05/PROTOCOL_WIRE_MATRIX.json):
//! - Auth: `Authorization: Bearer` (api-key or OAuth access token); an
//!   `authHeader` config goes verbatim; `none` sends nothing.
//! - Body: `{model, input, instructions?, tools?, stream:true}` (R05-T04:
//!   the production wire mode is the family's SSE stream). The
//!   system prompt is `instructions`.
//! - `input` items: user/assistant `message` items (`input_text` /
//!   `output_text` parts), `function_call` items (call_id pairing,
//!   arguments as a JSON string), `function_call_output` items.
//! - Reasoning items are provider state (C10): a `reasoning` output item
//!   parses to `Reasoning` blocks from its summary PLUS one opaque block
//!   carrying the item VERBATIM (encrypted_content and all); on the way
//!   out the opaque item is re-inserted at its position. A bare
//!   `Reasoning` (no item) has no wire shape and stays local.
//! - Server-side session state is NEVER referenced: no
//!   `previous_response_id`, no `store` continuation — every call carries
//!   the full input (C11; the family cannot prove a cross-provider or
//!   post-cancel resume, so the adapter never attempts one).
//! - Identity flows ONLY from the driver; response `id`/`model` are never
//!   read.
//!
//! Streaming (R05-T04): [`ResponsesStreamAccumulator`] decodes the family's
//! SSE vocabulary INCREMENTALLY as the network delivers it — the delta
//! events (`response.output_text.delta` /
//! `response.reasoning_summary_text.delta`) emit live deltas, while the
//! TERMINAL event (`response.completed` / `response.failed` /
//! `response.incomplete`) carries the authoritative aggregate that the
//! SAME [`parse_responses_response_as`] validates (one contract, two wire
//! modes). A duplicate terminal event or any event after the terminal is
//! loud (C10); a stream without a terminal event is truncated, never
//! half-parsed. The codex sibling ([`super::openai_codex_responses`])
//! shares this decoder with its own opaque tag.

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
use super::tool_render::render_tool_outcome_text;
use super::ParsedChat;

/// The `provider` tag of this family's opaque blocks.
pub const FAMILY: &str = "openai-responses";

/// The real HTTP adapter for the openai-responses family. Stateless per
/// call; no redirects (C10); error classification shared.
pub struct OpenAiResponsesAdapter {
    client: dispatch::NetworkClient,
    schema_budget: SchemaBudget,
    timeouts: dispatch::HttpTimeouts,
}

impl OpenAiResponsesAdapter {
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
    /// SSE stream). Delta events emit live text/reasoning to `deltas`; the
    /// terminal event's response object is the authoritative aggregate
    /// validated through [`parse_responses_response_as`]. An `Err` from the
    /// sink means the driver is gone: the read stops and the call winds
    /// down as cancelled.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_chat<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        route: &'a ResolvedModelRoute,
        auth: &'a ApplicableAuth,
        compat: Option<&'a super::compat::CompatCall>,
        deltas: &'a dyn lingxi_kernel::ports::TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(self.execute_chat_stream(ctx, call, input, route, auth, compat, deltas))
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_chat_stream(
        &self,
        ctx: &RunContext,
        call: &ModelCallId,
        input: &ModelTurnInput,
        route: &ResolvedModelRoute,
        auth: &ApplicableAuth,
        compat: Option<&super::compat::CompatCall>,
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
        let body = match render_responses_request(input, route, true) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        // R05-T05: the ported provider-compat layer patches the rendered
        // envelope before dispatch (None pins the byte-exact golden wire).
        let body = match super::compat::apply_for_call(body, route, compat) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        let url = dispatch::append_provider_api_path(&route.endpoint, "/responses");
        let client = match self.client.client() {
            Ok(client) => client,
            Err(error) => return fail(error, false),
        };
        let request = dispatch::apply_auth(
            client.post(&url).json(&body),
            auth,
            BearerStyle::AuthorizationBearer,
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
        let accumulator = ResponsesStreamAccumulator::new(FAMILY);
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
        let salvaged_usage = drive_handler
            .accumulator
            .observed_usage_report(lingxi_kernel::model_exchange::ProtocolFamily::OpenAiResponses);
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
                    lingxi_kernel::model_exchange::ProtocolFamily::OpenAiResponses
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

// ── outbound rendering (pure; shared with the codex sibling) ────────────────

/// Whether an assistant turn's content carries THIS family's opaque state
/// (R05 RR1 F09): reasoning items, function_call anchors and preserved
/// unknown items are bound to the provider/model that minted them.
fn turn_carries_family_state(content: &[ContentBlock], family: &str) -> bool {
    content
        .iter()
        .any(|block| matches!(block, ContentBlock::Opaque { provider, .. } if provider == family))
}

/// R05 RR1 F09: the source-authorization gate for this family's opaque
/// state (same contract as the other families' — a family tag is never
/// source authorization).
fn enforce_turn_origin(
    origin: &Option<lingxi_kernel::model_exchange::TurnOrigin>,
    route: &ResolvedModelRoute,
    family: &str,
) -> Result<(), ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    match origin {
        Some(origin) if origin.authorizes(&route.provider, &route.model) => Ok(()),
        Some(origin) => Err(invalid(format!(
            "the exchange history carries {family} opaque state (reasoning items / call \
             anchors) minted by provider {:?} model {:?}; this request targets provider {:?} \
             model {:?}. Same-protocol-family is NOT source authorization — replaying another \
             origin's encrypted state would corrupt the protocol round-trip. Restart or compact \
             the session onto one serving model, or re-route the run to the origin model.",
            origin.provider, origin.model, route.provider, route.model
        ))),
        None => Err(invalid(format!(
            "the exchange history carries {family} opaque state (reasoning items / call \
             anchors) with no recorded serving origin; unproven protocol state is never \
             forwarded onto a request (restart or compact the session)"
        ))),
    }
}

/// Renders the exchange into the family's `input` items (shared by both
/// responses adapters — the wire vocabulary is identical; only the
/// transport framing differs).
///
/// R05 RR1 F10: the items keep the ORIGINAL relative order of the turn's
/// content — text message segments, reasoning items and anchored
/// function_calls each replay at the position they arrived in; the family
/// never re-sorts history into a fixed "standard" order. (Adjacent text
/// blocks merge into one message segment joined by `\n` — the registered
/// normalization for indistinguishable adjacent segments.)
pub(crate) fn render_input_items(
    input: &ModelTurnInput,
    family: &str,
    route: &ResolvedModelRoute,
) -> Result<Vec<serde_json::Value>, ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let mut items: Vec<serde_json::Value> = Vec::new();
    // R05-T06: a turn carrying host-authorized images appends the
    // `input_image` parts (the incumbent `serializeResponsesContentBlock`
    // shape); a text-only turn keeps the single-input_text shape
    // byte-identically.
    let mut submission_content =
        vec![serde_json::json!({"type": "input_text", "text": input.submission})];
    for image in &input.images {
        submission_content.push(serde_json::json!({
            "type": "input_image",
            "image_url": super::image_data_url(image),
        }));
    }
    items.push(serde_json::json!({
        "type": "message",
        "role": "user",
        "content": submission_content,
    }));
    for item in &input.prior {
        match item {
            ExchangeItem::AssistantTurn {
                content,
                tool_calls,
                origin,
                ..
            } => {
                // R05 RR1 F09: opaque state is source-bound — check before
                // any of it could be echoed.
                if turn_carries_family_state(content, family) {
                    enforce_turn_origin(origin, route, family)?;
                }
                // The position-anchored rendering map: call_id → (the
                // exchange call, consumed-by-anchor yet?).
                let mut calls_by_id: std::collections::HashMap<
                    String,
                    (lingxi_kernel::model_exchange::RequestedToolCall, bool),
                > = std::collections::HashMap::new();
                for call in tool_calls {
                    if let Some(id) = &call.provider_call_id {
                        calls_by_id.insert(id.clone(), (call.clone(), false));
                    }
                }
                // The ordered walk: adjacent text blocks accumulate into the
                // current message segment; every non-text block flushes it
                // FIRST so the segment keeps its position.
                let mut text_run: Vec<String> = Vec::new();
                let flush_text = |items: &mut Vec<serde_json::Value>,
                                  text_run: &mut Vec<String>| {
                    if text_run.is_empty() {
                        return;
                    }
                    let text = text_run.join("\n");
                    text_run.clear();
                    items.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text}],
                    }));
                };
                for block in content {
                    match block {
                        ContentBlock::Text { text } => text_run.push(text.clone()),
                        ContentBlock::Reasoning { .. } => {
                            // A bare Reasoning has no wire shape without its
                            // reasoning item (the opaque companion carries
                            // it); never rendered as answer text. It still
                            // BREAKS the adjacent-text run (two message
                            // segments either side of it stay two segments).
                            flush_text(&mut items, &mut text_run);
                        }
                        ContentBlock::Opaque { provider, data } => {
                            if provider != family {
                                // Another family's state is never echoed
                                // (C10); it still breaks the text run the
                                // same way a same-family item would.
                                flush_text(&mut items, &mut text_run);
                                continue;
                            }
                            match data.get("type").and_then(|t| t.as_str()) {
                                // This family's reasoning items re-insert
                                // verbatim at their position (C10).
                                Some("reasoning") => {
                                    flush_text(&mut items, &mut text_run);
                                    items.push(data.clone());
                                }
                                // R05 RR1 F10: an anchored function_call
                                // replays AT ITS ORIGINAL POSITION.
                                Some("function_call_item") => {
                                    flush_text(&mut items, &mut text_run);
                                    let call_id = data
                                        .get("call_id")
                                        .and_then(|c| c.as_str())
                                        .ok_or_else(|| {
                                            invalid(
                                                "function_call_item anchor without a call_id: \
                                                 the position-bound call cannot be resolved \
                                                 (never a guess)"
                                                    .to_string(),
                                            )
                                        })?
                                        .to_string();
                                    let entry = calls_by_id.get_mut(&call_id).ok_or_else(|| {
                                        invalid(format!(
                                            "function_call_item anchor names call id \
                                                 {call_id:?} which this exchange's tool calls \
                                                 do not contain; the anchor/call pairing is \
                                                 inconsistent (never a guess)"
                                        ))
                                    })?;
                                    if entry.1 {
                                        return Err(invalid(format!(
                                            "function_call_item anchor names call id \
                                             {call_id:?} twice: a duplicated anchor is a \
                                             conflict, never merged"
                                        )));
                                    }
                                    entry.1 = true;
                                    items.push(render_function_call_item(&entry.0.clone(), input)?);
                                }
                                // Any other preserved item re-inserts
                                // verbatim at its position.
                                _ => {
                                    flush_text(&mut items, &mut text_run);
                                    items.push(data.clone());
                                }
                            }
                        }
                        ContentBlock::ResourceRef { resource } => {
                            let text = match &resource.uri {
                                Some(uri) => format!("[resource: {uri}]"),
                                None => "[resource]".to_string(),
                            };
                            text_run.push(text);
                        }
                    }
                }
                flush_text(&mut items, &mut text_run);
                // Calls WITHOUT a position anchor (manually built exchanges,
                // legacy shapes) keep the documented content-first order.
                for call in tool_calls {
                    let anchored = call
                        .provider_call_id
                        .as_ref()
                        .is_some_and(|id| calls_by_id.get(id).is_some_and(|e| e.1));
                    if anchored {
                        continue;
                    }
                    items.push(render_function_call_item(call, input)?);
                }
            }
            ExchangeItem::ToolResult {
                tool_call_id,
                provider_call_id,
                outcome,
            } => {
                let provider_call_id = provider_call_id.clone().ok_or_else(|| {
                    invalid(format!(
                        "exchange history tool result {tool_call_id} carries no provider \
                         correlation id; this protocol requires the pairing"
                    ))
                })?;
                items.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": provider_call_id,
                    "output": render_tool_outcome_text(outcome),
                }));
            }
        }
    }
    Ok(items)
}

/// The wire shape of one exchange tool call: `{type, call_id, name,
/// arguments}` (the pairing fields verbatim).
fn render_function_call_item(
    call: &lingxi_kernel::model_exchange::RequestedToolCall,
    input: &ModelTurnInput,
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
    let provider_call_id = call.provider_call_id.clone().ok_or_else(|| {
        invalid(format!(
            "exchange history tool call {} ({}) carries no provider correlation id; this \
             protocol requires the pairing",
            call.tool_call_id, call.target
        ))
    })?;
    Ok(serde_json::json!({
        "type": "function_call",
        "call_id": provider_call_id,
        "name": wire_name,
        "arguments": serde_json::to_string(call.arguments.as_value()).map_err(|err| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!("effective arguments serialize: {err}"),
                false,
            )
        })?,
    }))
}

/// Builds the request body (pure). `streaming` flips `stream` (the codex
/// sibling always passes true; this family defaults to false). NOTE:
/// `store` is deliberately ABSENT here — this family never references
/// server-side session state (C11); the codex sibling sets its protocol's
/// explicit `store: false`.
pub fn render_responses_request(
    input: &ModelTurnInput,
    route: &ResolvedModelRoute,
    streaming: bool,
) -> Result<serde_json::Value, ProtocolError> {
    let items = render_input_items(input, FAMILY, route)?;
    let tools: Vec<serde_json::Value> = input
        .tools
        .declarations
        .iter()
        .map(|declaration| {
            serde_json::json!({
                "type": "function",
                "name": declaration.wire_name,
                "description": declaration.description,
                "parameters": declaration.input_schema.schema,
            })
        })
        .collect();
    let mut body = serde_json::json!({
        "model": route.model,
        "input": items,
        "stream": streaming,
    });
    if let Some(system) = &input.system_prompt {
        body["instructions"] = serde_json::Value::String(system.clone());
    }
    // R05-T06: the host-decided output cap (worker callbacks / aux slots —
    // the incumbent `callText` `max_output_tokens`); absent when undecided.
    if let Some(cap) = input.max_output_tokens {
        body["max_output_tokens"] = serde_json::Value::from(cap);
    }
    if !tools.is_empty() {
        body["tools"] = serde_json::Value::Array(tools);
    }
    Ok(body)
}

// ── inbound parsing (pure) ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ResponsesBody {
    #[serde(default)]
    output: Vec<serde_json::Value>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    error: Option<ResponsesError>,
    #[serde(default)]
    incomplete_details: Option<serde_json::Value>,
    // R05-T07: the usage object is decoded from the raw body by the
    // STRICT per-family decoder (see MessagesResponse).
}

#[derive(Debug, Deserialize)]
struct ResponsesError {
    #[serde(default)]
    message: Option<String>,
}

/// Parses one successful (or honestly failed) response body. Loud on every
/// malformed shape. `family` tags the opaque blocks (`openai-responses` or
/// `openai-codex-responses` — opaque state is bound to the family that
/// minted it and never crosses, C10).
pub(crate) fn parse_responses_response_as(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    body: &serde_json::Value,
    schema_budget: &SchemaBudget,
    family: &str,
) -> Result<ParsedChat, ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let response: ResponsesBody = serde_json::from_value(body.clone()).map_err(|err| {
        invalid(format!(
            "provider response violates the response schema: {err}"
        ))
    })?;
    // R05-T07: the strict per-family decode (cached/reasoning detail
    // tokens + provenance + invalid-vs-unknown).
    let usage_report = match super::usage::decode_family_usage(
        lingxi_kernel::model_exchange::ProtocolFamily::OpenAiResponses,
        body,
    ) {
        super::usage::UsageDecode::Absent => ReportedUsage::Unknown,
        super::usage::UsageDecode::Usage(usage) => ReportedUsage::Known(usage),
        super::usage::UsageDecode::Invalid { detail } => ReportedUsage::Invalid { detail },
    };
    // A server-side FAILED response is an honest failure, never an empty
    // turn (the provider's own error text travels; no material scrubbing
    // needed at this layer — a 200 body's error field is model-side
    // content, same trust as provider text).
    if response.status.as_deref() == Some("failed") {
        let detail = response
            .error
            .and_then(|error| error.message)
            .unwrap_or_else(|| "the provider marked the response failed".to_string());
        return Err(ProtocolError::new(
            ErrorCode::UpstreamUnavailable,
            format!("provider response status is failed: {detail}"),
            false,
        ));
    }
    // C11 stop-reason honesty (R05-T04) + R05 RR1 F12: transport end is not
    // protocol completion. An incomplete response classifies by WHY it is
    // incomplete — a budget truncation is a retryable BudgetExceeded (zero
    // side effects from this turn; the partial content stays in the call's
    // delta events), a content-filter stop is a non-retryable Forbidden. A
    // server-side CANCELLED response is a non-retryable Cancelled. A
    // non-terminal status (`queued` / `in_progress`) in this terminal slot
    // is a loud protocol violation — the adapter never asked for a
    // background response. An ABSENT or unmapped status is equally loud:
    // the turn never classifies, nothing dispatches, and no Final is ever
    // guessed from body content alone.
    match response.status.as_deref() {
        Some("completed") => {}
        Some("incomplete") => {
            let reason = response
                .incomplete_details
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|r| r.as_str());
            match reason {
                Some("max_output_tokens") | Some("max_tool_calls") => {
                    return Ok(ParsedChat {
                        turn: ProviderTurn::Failed {
                            error: ProtocolError::new(
                                ErrorCode::BudgetExceeded,
                                format!(
                                    "provider stopped at the budget (incomplete_details.reason: \
                                     {}): the turn is truncated; the partial content stays in \
                                     the call's delta events and nothing from this turn was \
                                     dispatched",
                                    reason.expect("matched above")
                                ),
                                true,
                            ),
                            retryable: true,
                        },
                        usage_report: usage_report.clone(),
                    });
                }
                Some("content_filter") => {
                    return Ok(ParsedChat {
                        turn: ProviderTurn::Failed {
                            error: ProtocolError::new(
                                ErrorCode::Forbidden,
                                "provider refused the turn through its content filter \
                                 (incomplete_details.reason: content_filter); not retryable \
                                 as-is"
                                    .to_string(),
                                false,
                            ),
                            retryable: false,
                        },
                        usage_report: usage_report.clone(),
                    });
                }
                other => {
                    return Err(invalid(format!(
                        "provider response is incomplete with an UNMAPPED reason ({other:?}): \
                         an incomplete turn is a truncation fact, never a content-classified \
                         final"
                    )));
                }
            }
        }
        Some("cancelled") => {
            return Ok(ParsedChat {
                turn: ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::Cancelled,
                        "provider marked the response cancelled server-side".to_string(),
                        false,
                    ),
                    retryable: false,
                },
                usage_report: usage_report.clone(),
            });
        }
        Some(status @ ("queued" | "in_progress")) => {
            return Err(invalid(format!(
                "provider response carries the NON-TERMINAL status {status:?} in a terminal \
                 slot: this adapter never requests background responses — a non-terminal body \
                 is a protocol violation, never a fabricated final"
            )));
        }
        Some(other) => {
            return Err(invalid(format!(
                "provider response carries the UNKNOWN status {other:?}: an unmapped terminal \
                 is never guessed into a completed turn"
            )));
        }
        None => {
            return Err(invalid(
                "provider response ended WITHOUT a status: the transport's end is not protocol \
                 completion — the turn never classifies, nothing dispatches"
                    .to_string(),
            ));
        }
    }
    let mut content: Vec<ContentBlock> = Vec::new();
    let mut requests: Vec<ToolRequest> = Vec::new();
    for item in &response.output {
        let item_type = item
            .get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| invalid("output item without a type".to_string()))?;
        match item_type {
            "message" => {
                let parts = item
                    .get("content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                for part in parts {
                    match part.get("type").and_then(|t| t.as_str()) {
                        Some("output_text") => {
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    content.push(ContentBlock::Text {
                                        text: text.to_string(),
                                    });
                                }
                            }
                        }
                        // A refusal part is provider state, never rendered
                        // as answer text — preserved verbatim (documented
                        // in the wire matrix).
                        _ => content.push(ContentBlock::Opaque {
                            provider: family.to_string(),
                            data: part.clone(),
                        }),
                    }
                }
            }
            "reasoning" => {
                // Summary text becomes Reasoning blocks; the ITEM itself
                // (encrypted_content and all) round-trips as opaque state.
                if let Some(summary) = item.get("summary").and_then(|s| s.as_array()) {
                    for part in summary {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                content.push(ContentBlock::Reasoning {
                                    text: text.to_string(),
                                });
                            }
                        }
                    }
                }
                content.push(ContentBlock::Opaque {
                    provider: family.to_string(),
                    data: item.clone(),
                });
            }
            "function_call" => {
                let call_id = item
                    .get("call_id")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| invalid("function_call without a call_id".to_string()))?;
                let name = item
                    .get("name")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| invalid("function_call without a name".to_string()))?;
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
                let arguments = match item.get("arguments") {
                    None | Some(serde_json::Value::Null) => serde_json::json!({}),
                    Some(serde_json::Value::String(raw)) => {
                        serde_json::from_str(raw).map_err(|err| {
                            invalid(format!(
                                "function_call arguments for {name:?} are not JSON: {err}"
                            ))
                        })?
                    }
                    Some(value @ serde_json::Value::Object(_)) => value.clone(),
                    Some(_) => {
                        return Err(invalid(format!(
                            "function_call arguments for {name:?} have an unsupported shape"
                        )));
                    }
                };
                let request =
                    ToolRequest::from_effective_arguments(target, arguments, schema_budget)
                        .ok_or_else(|| {
                            invalid(format!(
                                "tool call arguments for {name:?} violate the argument invariants"
                            ))
                        })?
                        .with_provider_call_id(call_id);
                requests.push(request);
                // R05 RR1 F10: the function_call item is an ordered position
                // of the response — the exchange keeps an anchor opaque HERE
                // so the renderer replays the call at this exact position
                // (text/reasoning/tool relative order is never normalized
                // into a fixed "standard" order).
                content.push(ContentBlock::Opaque {
                    provider: family.to_string(),
                    data: serde_json::json!({
                        "type": "function_call_item",
                        "call_id": call_id,
                    }),
                });
            }
            // Every other output item type (web_search_call, ...) is
            // provider state — preserved verbatim, never dropped.
            _ => content.push(ContentBlock::Opaque {
                provider: family.to_string(),
                data: item.clone(),
            }),
        }
    }
    // R05 RR1 F11: the batch-level identity admission — a same-id re-send
    // with identical shape collapses, a same-id CONFLICT rejects the whole
    // turn (zero requests admitted, zero side effects).
    let requests = super::batch_admission::admit_provider_call_ids(family, requests)?;
    if !requests.is_empty() {
        return Ok(ParsedChat {
            turn: ProviderTurn::ToolRequests { requests, content },
            usage_report: usage_report.clone(),
        });
    }
    // R05 RR1 F12: a normally-stopped turn whose output carries NO answer
    // text (reasoning/opaque only) is process content, not a final answer.
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
            "provider returned no content and no tool calls (status: {})",
            response.status.as_deref().unwrap_or("absent")
        )
    } else {
        format!(
            "provider turn completed with process-only content (reasoning/opaque state, no \
             answer text; status: {})",
            response.status.as_deref().unwrap_or("absent")
        )
    };
    Ok(ParsedChat {
        turn: ProviderTurn::Empty {
            detail,
            // R05 RR1 F12: the process-state blocks ride along for replay.
            content,
        },
        usage_report,
    })
}

/// This family's parse entry (opaque blocks tagged `openai-responses`).
pub fn parse_responses_response(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    body: &serde_json::Value,
    schema_budget: &SchemaBudget,
) -> Result<ParsedChat, ProtocolError> {
    parse_responses_response_as(call, snapshot, body, schema_budget, FAMILY)
}

/// The incremental accumulator of one responses-family SSE stream
/// (R05-T04): [`ResponsesStreamAccumulator::handle_event`] consumes each
/// decoded event as it arrives — `response.output_text.delta` /
/// `response.reasoning_summary_text.delta` emit live deltas; every other
/// non-terminal event is progress vocabulary the terminal aggregate
/// supersedes (ignored for live emission, never an error). The TERMINAL
/// event (`response.completed` / `response.failed` / `response.incomplete`)
/// carries the authoritative aggregate;
/// [`ResponsesStreamAccumulator::finish`] validates it through the same
/// buffered parser.
///
/// Honesty rules (C10/C11): a SECOND terminal event is a loud conflict
/// (never last-one-wins), any event after the terminal is loud, an `error`
/// event is a loud failure carrying the provider's message, and a stream
/// ending without a terminal event is truncated — never half-parsed.
pub struct ResponsesStreamAccumulator {
    family: String,
    terminal: Option<serde_json::Value>,
}

impl ResponsesStreamAccumulator {
    pub fn new(family: &str) -> Self {
        Self {
            family: family.to_string(),
            terminal: None,
        }
    }

    /// R05 RR1 F38: the usage fact the terminal response object observed
    /// (decoded through the SAME strict family decoder) — the salvage a
    /// parse-failed turn still accounts with. Never a guess.
    pub fn observed_usage_report(
        &self,
        family: lingxi_kernel::model_exchange::ProtocolFamily,
    ) -> lingxi_kernel::usage::ReportedUsage {
        let raw = self
            .terminal
            .as_ref()
            .and_then(|terminal| terminal.get("usage").filter(|usage| !usage.is_null()));
        super::usage::salvage_usage_report(family, raw)
    }

    /// Consumes one decoded SSE event; returns the live deltas it produced.
    pub fn handle_event(&mut self, event: &SseEvent) -> Result<Vec<ModelTurnDelta>, ProtocolError> {
        let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
        let mut emitted = Vec::new();
        let data = event.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(emitted);
        }
        if self.terminal.is_some() {
            return Err(invalid(
                "stream event after the terminal response event: the terminal closes the \
                 stream; a late event is never silently absorbed"
                    .to_string(),
            ));
        }
        let frame: serde_json::Value = serde_json::from_str(data)
            .map_err(|err| invalid(format!("stream frame is not JSON: {err}")))?;
        match frame.get("type").and_then(|t| t.as_str()) {
            Some("response.completed") | Some("response.failed") | Some("response.incomplete") => {
                let response = frame.get("response").cloned().ok_or_else(|| {
                    invalid("terminal stream event without a response object".to_string())
                })?;
                self.terminal = Some(response);
            }
            Some("response.output_text.delta") => {
                if let Some(fragment) = frame.get("delta").and_then(|d| d.as_str()) {
                    if !fragment.is_empty() {
                        emitted.push(ModelTurnDelta::Text(fragment.to_string()));
                    }
                }
            }
            Some("response.reasoning_summary_text.delta") => {
                if let Some(fragment) = frame.get("delta").and_then(|d| d.as_str()) {
                    if !fragment.is_empty() {
                        emitted.push(ModelTurnDelta::Reasoning(fragment.to_string()));
                    }
                }
            }
            Some("error") => {
                let message = frame
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("provider stream error event");
                return Err(invalid(format!("provider stream error: {message}")));
            }
            // Every other event of the family's progress vocabulary
            // (response.created, output_item.added/done, content_part.*,
            // function_call_arguments.delta, ...) is superseded by the
            // terminal aggregate — ignored for live emission, never an
            // error.
            _ => {}
        }
        Ok(emitted)
    }

    /// Validates the terminal aggregate through the SAME buffered parser. A
    /// stream without a terminal event is truncated: loud, never
    /// half-parsed (C05/C09/C11).
    pub fn finish(
        self,
        call: &ModelCallId,
        snapshot: &ToolDeclarationSnapshot,
        schema_budget: &SchemaBudget,
    ) -> Result<ParsedChat, ProtocolError> {
        let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
        let terminal = self.terminal.ok_or_else(|| {
            invalid(
                "stream ended without a terminal response event (response.completed/failed/\
                 incomplete): a truncated stream is never half-parsed into a turn"
                    .to_string(),
            )
        })?;
        parse_responses_response_as(call, snapshot, &terminal, schema_budget, &self.family)
    }
}

impl super::dispatch::StreamAccumulator for ResponsesStreamAccumulator {
    fn handle_event(&mut self, event: &SseEvent) -> Result<Vec<ModelTurnDelta>, ProtocolError> {
        ResponsesStreamAccumulator::handle_event(self, event)
    }
}

/// Aggregates one responses-family SSE stream (the offline/golden entry
/// over already-decoded events) through the SAME incremental accumulator
/// the production read drives — one decode contract, two delivery modes.
pub(crate) fn parse_responses_stream_as(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    events: &[SseEvent],
    schema_budget: &SchemaBudget,
    family: &str,
) -> Result<ParsedChat, ProtocolError> {
    let mut accumulator = ResponsesStreamAccumulator::new(family);
    for event in events {
        accumulator.handle_event(event)?;
    }
    accumulator.finish(call, snapshot, schema_budget)
}

/// This family's stream parse entry (the golden tests exercise it; T04
/// wires the incremental delivery).
pub fn parse_responses_stream(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    events: &[SseEvent],
    schema_budget: &SchemaBudget,
) -> Result<ParsedChat, ProtocolError> {
    parse_responses_stream_as(call, snapshot, events, schema_budget, FAMILY)
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
            provider: "deepseek-responses".to_string(),
            model: "responses-test".to_string(),
        })
    }

    fn route() -> ResolvedModelRoute {
        ResolvedModelRoute {
            provider: "deepseek-responses".to_string(),
            model: "responses-test".to_string(),
            operation: ModelOperation::Chat,
            protocol: ProtocolFamily::OpenAiResponses,
            endpoint: "https://responses.example.test/v1".to_string(),
            credential: CredentialReference {
                provider: "deepseek-responses".to_string(),
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
    fn request_shape_instructions_tools_and_no_server_state() {
        let mut input = ModelTurnInput::first_turn("hello", snapshot_with_read());
        input.system_prompt = Some("be terse".to_string());
        let body = render_responses_request(&input, &route(), false).expect("renders");
        assert_eq!(body["model"], "responses-test");
        assert_eq!(body["instructions"], "be terse");
        assert_eq!(body["stream"], false);
        assert!(
            body.get("store").is_none(),
            "never a server-side store reference"
        );
        assert!(
            body.get("previous_response_id").is_none(),
            "never a server-side session reference (C11)"
        );
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "read");
    }

    #[test]
    fn tool_roundtrip_uses_function_call_pairing() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:read",
            serde_json::json!({"path": "/tmp/a"}),
            &budget,
        )
        .expect("effective")
        .with_provider_call_id("call_1");
        let mut input = ModelTurnInput::first_turn("read it", snapshot);
        input.prior.push(ExchangeItem::AssistantTurn {
            call: ModelCallId::new("mc-1"),
            content: vec![ContentBlock::Text {
                text: "reading".to_string(),
            }],
            tool_calls: vec![lingxi_kernel::model_exchange::RequestedToolCall {
                tool_call_id: ToolCallId::new("run-tc0001"),
                provider_call_id: request.provider_call_id.clone(),
                target: request.target.clone(),
                arguments: request.arguments.clone(),
                args_digest: request.args_digest.clone(),
                args_summary: None,
            }],
            origin: None,
        });
        input.prior.push(ExchangeItem::ToolResult {
            tool_call_id: ToolCallId::new("run-tc0001"),
            provider_call_id: Some("call_1".to_string()),
            outcome: ToolOutcome::success_text("file body"),
        });
        let body = render_responses_request(&input, &route(), false).expect("renders");
        assert_eq!(
            body["input"][1],
            serde_json::json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"reading"}]})
        );
        assert_eq!(
            body["input"][2],
            serde_json::json!({"type":"function_call","call_id":"call_1","name":"read","arguments":"{\"path\":\"/tmp/a\"}"})
        );
        assert_eq!(
            body["input"][3],
            serde_json::json!({"type":"function_call_output","call_id":"call_1","output":"file body"})
        );
    }

    #[test]
    fn reasoning_items_roundtrip_verbatim_with_summary_as_reasoning() {
        let snapshot = ToolDeclarationSnapshot::empty();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0001");
        let reasoning_item = serde_json::json!({
            "type": "reasoning",
            "id": "rs_1",
            "summary": [{"type": "summary_text", "text": "checked the file"}],
            "encrypted_content": "enc-bytes-xyz"
        });
        let body = serde_json::json!({
            "id": "resp_ignored",
            "status": "completed",
            "output": [
                reasoning_item,
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "done"}]}
            ],
            "usage": {"input_tokens": 8, "output_tokens": 3}
        });
        let parsed = parse_responses_response(&call, &snapshot, &body, &budget).expect("parses");
        assert_eq!(
            parsed.usage(),
            Some(UsageRecord {
                input_tokens: 8,
                output_tokens: 3
            })
        );
        let message = match parsed.turn {
            ProviderTurn::Final { message } => message,
            other => panic!("expected Final, got {other:?}"),
        };
        assert_eq!(message.content.len(), 3);
        assert!(
            matches!(&message.content[0], ContentBlock::Reasoning { text } if text == "checked the file")
        );
        assert!(
            matches!(&message.content[1], ContentBlock::Opaque { provider, data }
            if provider == FAMILY && data == &reasoning_item)
        );
        assert!(matches!(&message.content[2], ContentBlock::Text { text } if text == "done"));

        // The opaque item re-inserts verbatim (C10); the bare reasoning
        // text is not double-rendered.
        let mut input = ModelTurnInput::first_turn("go on", snapshot);
        input.prior.push(ExchangeItem::AssistantTurn {
            call: call.clone(),
            content: message.content.clone(),
            tool_calls: Vec::new(),
            origin: origin(),
        });
        let rendered = render_responses_request(&input, &route(), false).expect("renders");
        assert_eq!(rendered["input"][1], reasoning_item);
        assert_eq!(
            rendered["input"][2],
            serde_json::json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]})
        );
        assert_eq!(rendered["input"].as_array().expect("items").len(), 3);
    }

    #[test]
    fn failed_status_is_an_honest_failure_never_an_empty_turn() {
        let call = ModelCallId::new("run-mc0002");
        let err = parse_responses_response(
            &call,
            &ToolDeclarationSnapshot::empty(),
            &serde_json::json!({
                "status": "failed",
                "error": {"message": "model exploded"},
                "output": []
            }),
            &SchemaBudget::default(),
        )
        .unwrap_err();
        assert!(err.message.contains("model exploded"));
    }

    #[test]
    fn stream_terminal_event_carries_the_authoritative_aggregate() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0003");
        let events = vec![
            SseEvent {
                event: None,
                data: r#"{"type":"response.output_text.delta","delta":"ig"}"#.to_string(),
            },
            SseEvent {
                event: None,
                data: r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"call_s","name":"read","arguments":"{\"path\":\"/tmp/b\"}"}],"usage":{"input_tokens":5,"output_tokens":2}}}"#.to_string(),
            },
            SseEvent {
                event: None,
                data: "[DONE]".to_string(),
            },
        ];
        let parsed = parse_responses_stream(&call, &snapshot, &events, &budget).expect("parses");
        assert_eq!(
            parsed.usage(),
            Some(UsageRecord {
                input_tokens: 5,
                output_tokens: 2
            })
        );
        match parsed.turn {
            ProviderTurn::ToolRequests { requests, .. } => {
                assert_eq!(requests[0].provider_call_id.as_deref(), Some("call_s"));
                assert_eq!(requests[0].target, "tool:first-party:read");
            }
            other => panic!("expected ToolRequests, got {other:?}"),
        }
        // A stream without a terminal event is truncated — loud.
        let err = parse_responses_stream(&call, &snapshot, &events[..1], &budget).unwrap_err();
        assert!(err.message.contains("terminal response event"));
    }
}
