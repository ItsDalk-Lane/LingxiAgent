//! The `anthropic-messages` protocol adapter (R05-T03): the Anthropic
//! Messages API wire family (`POST {endpoint}/v1/messages`, the incumbent's
//! `appendProviderApiPath` join rule).
//!
//! Wire facts (docs/rust-tauri/R05/PROTOCOL_WIRE_MATRIX.json is the
//! registered matrix; the incumbent `core/llm-client.ts` anthropic branch
//! is the reviewed baseline):
//! - Headers: `anthropic-version: 2023-06-01` always; a bearer credential
//!   goes out as `x-api-key` (NOT `Authorization`); an `authHeader` config
//!   goes verbatim; `none` sends nothing.
//! - Body: `{model, max_tokens, system?, messages, tools?, stream:true}`.
//!   `max_tokens` is REQUIRED by this family: the adapter pins the
//!   documented constant [`DEFAULT_MAX_OUTPUT_TOKENS`] (the per-call budget
//!   field is a registered contract gap — R05-T05/T07 own it).
//! - The system prompt is the top-level `system` string (never a message).
//! - Tool declarations are `[{name, description, input_schema}]` with the
//!   schema document verbatim.
//! - Tool calls are `tool_use` content blocks (provider id pairing);
//!   results are `tool_result` blocks inside a USER message, consecutive
//!   results of one turn merged into that one message (the family's
//!   pairing rule). Failures/cancellations/unknowns carry
//!   `is_error: true` with the honest rendered text — never a fake
//!   success.
//! - Thinking blocks round-trip as state (C10): a `thinking` block parses
//!   to `Reasoning` + an adjacent signature [`ContentBlock::Opaque`]; on
//!   the way out the pair recombines verbatim. `redacted_thinking` is an
//!   opaque verbatim block. A bare `Reasoning` (no signature) has no wire
//!   shape and stays local. Opaque blocks of ANOTHER family are never
//!   echoed onto this wire.
//! - Identity flows ONLY from the driver; the response's `id`/`model`
//!   fields are never read (C10).
//!
//! Streaming (R05-T04): production dispatch is the family's SSE mode
//! (`stream: true`). The stream vocabulary — `message_start` /
//! `content_block_start` / `content_block_delta` (`text_delta` /
//! `thinking_delta` / `input_json_delta` / `signature_delta`) /
//! `content_block_stop` / `message_delta` (stop_reason + output usage) /
//! `message_stop` / `ping` / `error` — is decoded INCREMENTALLY as the
//! network delivers it; text/thinking deltas are emitted live, tool input
//! JSON accumulates behind the argument bound and validates ONLY at batch
//! close (`message_stop`), and the reassembled message is validated by the
//! SAME [`parse_messages_response`] as the buffered mode — one contract,
//! two wire modes.

use lingxi_kernel::model_exchange::{
    ExchangeItem, ModelTurnInput, ResolvedModelRoute, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{
    ModelTurnDelta, ProviderTurn, ProviderTurnResult, ToolOutcome, ToolRequest,
};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::usage::ReportedUsage;
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError};
use serde::Deserialize;

use super::credentials::ApplicableAuth;
use super::dispatch::{self, BearerStyle};
use super::streaming::{SseEvent, TOOL_ARGUMENTS_MAX_BYTES};
use super::tool_render::render_tool_outcome_text;
use super::ParsedChat;

/// The family's protocol version header (the incumbent value).
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The pinned `max_tokens` (the protocol REQUIRES the field; the per-call
/// budget is not part of the `ModelTurnInput` contract — a registered gap,
/// see R05_INTERFACE_EVOLUTION.md T03).
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16384;

/// The `provider` tag of this family's opaque blocks.
pub const FAMILY: &str = "anthropic-messages";

/// The real HTTP adapter for the anthropic-messages family. Stateless per
/// call; no redirects (C10); error classification shared
/// ([`dispatch::classify_error_status`]).
pub struct AnthropicMessagesAdapter {
    client: reqwest::Client,
    schema_budget: SchemaBudget,
    max_output_tokens: u32,
    timeouts: dispatch::HttpTimeouts,
}

impl AnthropicMessagesAdapter {
    /// `max_output_tokens` is an explicit constructor argument (never a
    /// hidden default at the dispatch layer); the production wiring passes
    /// [`DEFAULT_MAX_OUTPUT_TOKENS`].
    pub fn new(schema_budget: SchemaBudget, max_output_tokens: u32) -> Result<Self, ProtocolError> {
        Self::new_with_timeouts(
            schema_budget,
            max_output_tokens,
            dispatch::HttpTimeouts::default(),
        )
    }

    /// Explicit timeout segments (R05-T05; tests inject shorter windows).
    pub fn new_with_timeouts(
        schema_budget: SchemaBudget,
        max_output_tokens: u32,
        timeouts: dispatch::HttpTimeouts,
    ) -> Result<Self, ProtocolError> {
        if max_output_tokens == 0 {
            return Err(ProtocolError::new(
                ErrorCode::Internal,
                "anthropic max_tokens must be >= 1".to_string(),
                false,
            ));
        }
        Ok(Self {
            client: dispatch::build_client_with_timeouts(&timeouts)?,
            schema_budget,
            max_output_tokens,
            timeouts,
        })
    }

    /// Executes ONE turn (R05-T04: the production wire mode is the family's
    /// SSE stream). Text/thinking deltas are emitted to `deltas` AS THE
    /// STREAM DELIVERS them; the terminal message is validated only after
    /// `message_stop` closes the batch. An `Err` from the sink means the
    /// driver is gone: the read stops and the call winds down as cancelled.
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
        // R05-T06: a host-decided per-call cap (worker callbacks / aux
        // slots) overrides the family's required constant for THIS call;
        // the incumbent chat path carries None and keeps the constant.
        let max_output_tokens = input.max_output_tokens.unwrap_or(self.max_output_tokens);
        let body = match render_messages_request(input, route, max_output_tokens, true) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        // R05-T05: the ported provider-compat layer patches the rendered
        // envelope before dispatch (None pins the byte-exact golden wire).
        let body = match super::compat::apply_for_call(body, route, compat) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        let url = dispatch::append_provider_api_path(&route.endpoint, "/v1/messages");
        let request = dispatch::apply_auth(
            self.client
                .post(&url)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&body),
            auth,
            BearerStyle::NamedHeader("x-api-key"),
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
        let accumulator = MessagesStreamAccumulator::new();
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
                    lingxi_kernel::model_exchange::ProtocolFamily::AnthropicMessages
                        .config_name()
                        .to_string(),
                ),
                transport_attempts: 1,
                served_by: Some(served_by()),
            },
            Err(error) => {
                let retryable = error.retryable;
                fail(error, retryable)
            }
        }
    }
}

// ── outbound rendering (pure) ───────────────────────────────────────────────

/// Renders one assistant turn's content blocks into this family's content
/// array. Returns the blocks; the caller appends tool_use blocks after
/// them (the exchange contract separates content from tool calls, so the
/// wire order is content-first — a documented normalization, see the wire
/// matrix C06 note).
fn render_assistant_blocks(content: &[ContentBlock]) -> Vec<serde_json::Value> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < content.len() {
        match &content[index] {
            ContentBlock::Text { text } => {
                blocks.push(serde_json::json!({"type": "text", "text": text}));
            }
            ContentBlock::Reasoning { text } => {
                // A thinking block needs its signature companion; the pair
                // recombines verbatim (C10). Without it the reasoning has
                // no wire shape on this family and stays local (durable in
                // the exchange history, not re-sent).
                let signature = match content.get(index + 1) {
                    Some(ContentBlock::Opaque { provider, data })
                        if provider == FAMILY
                            && data.get("type").and_then(|t| t.as_str())
                                == Some("thinking_signature") =>
                    {
                        data.get("signature").and_then(|s| s.as_str())
                    }
                    _ => None,
                };
                if let Some(signature) = signature {
                    blocks.push(serde_json::json!({
                        "type": "thinking",
                        "thinking": text,
                        "signature": signature,
                    }));
                    index += 1; // the signature opaque was consumed
                }
            }
            ContentBlock::Opaque { provider, data } => {
                if provider == FAMILY {
                    match data.get("type").and_then(|t| t.as_str()) {
                        // Verbatim provider state (e.g. redacted_thinking)
                        // round-trips byte-identically.
                        Some("redacted_thinking") => blocks.push(data.clone()),
                        // A signature orphan has nothing to sign — skipped
                        // (never fabricated into a thinking block).
                        Some("thinking_signature") => {}
                        _ => {}
                    }
                }
                // Another family's opaque state is never echoed here (C10).
            }
            ContentBlock::ResourceRef { resource } => {
                let text = match &resource.uri {
                    Some(uri) => format!("[resource: {uri}]"),
                    None => "[resource]".to_string(),
                };
                blocks.push(serde_json::json!({"type": "text", "text": text}));
            }
        }
        index += 1;
    }
    blocks
}

/// The tool_result block of one outcome (the family's pairing + the honest
/// error flag).
fn render_tool_result_block(provider_call_id: &str, outcome: &ToolOutcome) -> serde_json::Value {
    serde_json::json!({
        "type": "tool_result",
        "tool_use_id": provider_call_id,
        "content": render_tool_outcome_text(outcome),
        // Every non-success state is honestly marked; the text carries the
        // full state detail (C07/C08).
        "is_error": !matches!(outcome, ToolOutcome::Success { .. }),
    })
}

/// Builds the request body for one turn (pure — the testable half).
/// `streaming` flips the wire mode (production is streaming, R05-T04; the
/// buffered shape stays pinned by the golden tests).
pub fn render_messages_request(
    input: &ModelTurnInput,
    route: &ResolvedModelRoute,
    max_output_tokens: u32,
    streaming: bool,
) -> Result<serde_json::Value, ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let mut messages: Vec<serde_json::Value> = Vec::new();
    // R05-T06: a turn carrying host-authorized images renders the
    // submission as the parts shape (text first, then `image` base64
    // blocks — the incumbent `convertContentForApi` anthropic mapping);
    // a text-only turn keeps the bare-string content byte-identically.
    if input.images.is_empty() {
        messages.push(serde_json::json!({
            "role": "user",
            "content": input.submission,
        }));
    } else {
        let mut content = Vec::with_capacity(input.images.len() + 1);
        content.push(serde_json::json!({"type": "text", "text": input.submission}));
        for image in &input.images {
            content.push(serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.mime,
                    "data": super::image_base64(image),
                },
            }));
        }
        messages.push(serde_json::json!({"role": "user", "content": content}));
    }
    for item in &input.prior {
        match item {
            ExchangeItem::AssistantTurn {
                content,
                tool_calls,
                ..
            } => {
                let mut blocks = render_assistant_blocks(content);
                for call in tool_calls {
                    let wire_name =
                        input
                            .tools
                            .wire_name_of_target(&call.target)
                            .ok_or_else(|| {
                                invalid(format!(
                                    "exchange history names tool target {:?}, which the current \
                                 declaration snapshot does not contain (the registry moved \
                                 mid-run); cannot faithfully re-render the transcript",
                                    call.target
                                ))
                            })?;
                    let provider_call_id = call.provider_call_id.clone().ok_or_else(|| {
                        invalid(format!(
                            "exchange history tool call {} ({}) carries no provider \
                             correlation id; this protocol requires the pairing",
                            call.tool_call_id, call.target
                        ))
                    })?;
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": provider_call_id,
                        "name": wire_name,
                        "input": call.arguments.as_value(),
                    }));
                }
                if blocks.is_empty() {
                    // No wire shape (e.g. a bare reasoning-only turn) —
                    // skipped, never an empty assistant message.
                    continue;
                }
                messages.push(serde_json::json!({"role": "assistant", "content": blocks}));
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
                let block = render_tool_result_block(&provider_call_id, outcome);
                // The family's pairing rule: the tool results of one
                // assistant turn travel in ONE user message — merge with
                // the immediately preceding tool-result user message.
                let merged = matches!(
                    messages.last(),
                    Some(last) if last["role"] == "user"
                        && last["content"].is_array()
                        && last["content"][0]["type"] == "tool_result"
                );
                if merged {
                    let last = messages.last_mut().expect("checked above");
                    last["content"]
                        .as_array_mut()
                        .expect("checked above")
                        .push(block);
                } else {
                    messages.push(serde_json::json!({"role": "user", "content": [block]}));
                }
            }
        }
    }
    let tools: Vec<serde_json::Value> = input
        .tools
        .declarations
        .iter()
        .map(|declaration| {
            serde_json::json!({
                "name": declaration.wire_name,
                "description": declaration.description,
                "input_schema": declaration.input_schema.schema,
            })
        })
        .collect();
    let mut body = serde_json::json!({
        "model": route.model,
        "max_tokens": max_output_tokens,
        "messages": messages,
        "stream": streaming,
    });
    if let Some(system) = &input.system_prompt {
        body["system"] = serde_json::Value::String(system.clone());
    }
    if !tools.is_empty() {
        body["tools"] = serde_json::Value::Array(tools);
    }
    Ok(body)
}

// ── inbound parsing (pure) ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    content: Vec<serde_json::Value>,
    #[serde(default)]
    stop_reason: Option<String>,
    // R05-T07: the usage numbers are decoded from the raw body by the
    // STRICT per-family decoder (no typed field here — a negative or
    // non-integer count must not fail the whole response shape).
}

/// Parses a successful response into the provider turn + usage. Loud on
/// every malformed shape; the family's identity fields are never read.
pub fn parse_messages_response(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    body: &serde_json::Value,
    schema_budget: &SchemaBudget,
) -> Result<ParsedChat, ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let response: MessagesResponse = serde_json::from_value(body.clone()).map_err(|err| {
        invalid(format!(
            "provider response violates the response schema: {err}"
        ))
    })?;
    // R05-T07: the strict per-family decode (cache categories +
    // provenance + invalid-vs-unknown). The wire projection is DERIVED
    // from the fact.
    let usage_report = match super::usage::decode_family_usage(
        lingxi_kernel::model_exchange::ProtocolFamily::AnthropicMessages,
        body,
    ) {
        super::usage::UsageDecode::Absent => ReportedUsage::Unknown,
        super::usage::UsageDecode::Usage(usage) => ReportedUsage::Known(usage),
        super::usage::UsageDecode::Invalid { detail } => ReportedUsage::Invalid { detail },
    };
    let mut content: Vec<ContentBlock> = Vec::new();
    let mut requests: Vec<ToolRequest> = Vec::new();
    for block in &response.content {
        let block_type = block
            .get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| invalid("content block without a type".to_string()))?;
        match block_type {
            "text" => {
                let text = block
                    .get("text")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| invalid("text block without text".to_string()))?;
                if !text.is_empty() {
                    content.push(ContentBlock::Text {
                        text: text.to_string(),
                    });
                }
            }
            "thinking" => {
                let thinking = block
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| invalid("thinking block without thinking text".to_string()))?;
                if !thinking.is_empty() {
                    content.push(ContentBlock::Reasoning {
                        text: thinking.to_string(),
                    });
                }
                // The signature is protocol state, never user-facing text:
                // it round-trips as an adjacent opaque block (C10).
                if let Some(signature) = block.get("signature").and_then(|s| s.as_str()) {
                    content.push(ContentBlock::Opaque {
                        provider: FAMILY.to_string(),
                        data: serde_json::json!({
                            "type": "thinking_signature",
                            "signature": signature,
                        }),
                    });
                }
            }
            "tool_use" => {
                let id = block
                    .get("id")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| invalid("tool_use block without an id".to_string()))?;
                let name = block
                    .get("name")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| invalid("tool_use block without a name".to_string()))?;
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
                let arguments = match block.get("input") {
                    None | Some(serde_json::Value::Null) => serde_json::json!({}),
                    Some(value @ serde_json::Value::Object(_)) => value.clone(),
                    Some(other) => {
                        return Err(invalid(format!(
                            "tool_use input for {name:?} is not an object ({})",
                            match other {
                                serde_json::Value::Bool(_) => "bool",
                                serde_json::Value::Number(_) => "number",
                                serde_json::Value::String(_) => "string",
                                serde_json::Value::Array(_) => "array",
                                _ => "unknown",
                            }
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
                        .with_provider_call_id(id);
                requests.push(request);
            }
            // redacted_thinking and every unknown block type round-trip as
            // provider-opaque state verbatim (never dropped, never guessed).
            _ => {
                content.push(ContentBlock::Opaque {
                    provider: FAMILY.to_string(),
                    data: block.clone(),
                });
            }
        }
    }
    // C11 stop-reason honesty (R05-T04): the blocks above already passed
    // every shape/argument validation (a malformed tool input stays a loud
    // InvalidMessage — nothing dispatched), so the CLOSED turn classifies
    // by its terminal reason. A truncated or refused turn never forms a
    // Final and never dispatches its tool batch; the partial content lives
    // on in the call's delta events (zero side effects → retry is safe).
    match response.stop_reason.as_deref() {
        Some("max_tokens") | Some("model_context_window_exceeded") => {
            return Ok(ParsedChat {
                turn: ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::BudgetExceeded,
                        format!(
                            "provider stopped at the token budget (stop_reason: {}): the turn \
                             is truncated; the partial content stays in the call's delta events \
                             and nothing from this turn was dispatched",
                            response.stop_reason.as_deref().expect("matched above")
                        ),
                        true,
                    ),
                    retryable: true,
                },
                usage_report: usage_report.clone(),
            });
        }
        Some("refusal") => {
            return Ok(ParsedChat {
                turn: ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::Forbidden,
                        "provider refused the turn (stop_reason: refusal); not retryable as-is"
                            .to_string(),
                        false,
                    ),
                    retryable: false,
                },
                usage_report: usage_report.clone(),
            });
        }
        _ => {}
    }
    if !requests.is_empty() {
        return Ok(ParsedChat {
            turn: ProviderTurn::ToolRequests { requests, content },
            usage_report: usage_report.clone(),
        });
    }
    if !content.is_empty() {
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
    Ok(ParsedChat {
        turn: ProviderTurn::Empty {
            detail: format!(
                "provider returned no content and no tool calls (stop_reason: {})",
                response.stop_reason.as_deref().unwrap_or("absent")
            ),
        },
        usage_report,
    })
}

// ── streaming decode (R05-T04) ──────────────────────────────────────────────

/// One in-flight content block of the stream (keyed by its `index`).
#[derive(Debug)]
enum PendingBlock {
    Text(String),
    Thinking {
        text: String,
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        partial_json: String,
    },
    /// A complete-at-start block of a type this adapter does not structure
    /// (e.g. `redacted_thinking`, `server_tool_use`) — preserved verbatim
    /// (C14), never dropped. A DELTA for such a block is a loud failure
    /// (the adapter cannot know the delta's semantics; silently dropping it
    /// would lose content).
    Opaque(serde_json::Value),
}

/// The incremental accumulator of one anthropic-messages SSE stream
/// (R05-T04): [`MessagesStreamAccumulator::handle_event`] consumes each
/// decoded event as it arrives and returns the live deltas to emit;
/// [`MessagesStreamAccumulator::finish`] validates the CLOSED batch
/// (`message_stop` seen) through the same buffered parser
/// ([`parse_messages_response`]).
///
/// Honesty rules: the SSE `event:` field and the data's `type` must agree
/// when both are present; a delta for a never-opened or already-closed
/// block, a second `content_block_start` on one index, a conflicting
/// stop_reason/usage re-send, and any event after `message_stop` are all
/// loud [`ErrorCode::InvalidMessage`]s (C10). Identical re-sends are
/// tolerated. An `error` event is classified by the provider's own error
/// type (`overloaded_error`/`api_error` retryable-upstream; anything else
/// a non-retryable protocol failure).
pub struct MessagesStreamAccumulator {
    block_order: Vec<u64>,
    blocks: std::collections::BTreeMap<u64, PendingBlock>,
    closed: std::collections::BTreeSet<u64>,
    stop_reason: Option<String>,
    // R05-T07 (C04): usage folds through the family's RUNNING-TOTAL mode
    // — message_start carries the input half (input_tokens + the cache
    // categories), message_delta carries the running output total. The
    // folder refuses a backwards total and treats identical repeats as
    // no-ops; a cumulative snapshot is never summed.
    usage: lingxi_kernel::usage::UsageFolder,
    usage_seen: bool,
    // R05-T07 fix (REVIEW-T07 F-01): a usage fragment whose numbers
    // violate the contract (negative / float / string count) is NEVER
    // silently dropped. The raw fragment is kept here and spliced into
    // the buffered finish body, where the SAME strict decoder marks the
    // whole fact invalid and names the violation (C06) — identical to
    // the buffered wire mode. Sticky: the FIRST violation wins; a later
    // valid fragment never "heals" a violated fact.
    usage_violation: Option<serde_json::Value>,
    message_stop_seen: bool,
}

impl Default for MessagesStreamAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl MessagesStreamAccumulator {
    pub fn new() -> Self {
        Self {
            block_order: Vec::new(),
            blocks: std::collections::BTreeMap::new(),
            closed: std::collections::BTreeSet::new(),
            stop_reason: None,
            usage: lingxi_kernel::usage::UsageFolder::new(
                lingxi_kernel::usage::UsageAggregationMode::RunningTotal,
            ),
            usage_seen: false,
            usage_violation: None,
            message_stop_seen: false,
        }
    }

    /// Consumes one decoded SSE event; returns the live deltas it produced.
    pub fn handle_event(&mut self, event: &SseEvent) -> Result<Vec<ModelTurnDelta>, ProtocolError> {
        let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
        let mut emitted = Vec::new();
        let data = event.data.trim();
        if self.message_stop_seen {
            return Err(invalid(
                "stream event after message_stop: the terminal marker ends the stream".to_string(),
            ));
        }
        if data.is_empty() {
            return Ok(emitted);
        }
        let frame: serde_json::Value = serde_json::from_str(data)
            .map_err(|err| invalid(format!("stream frame is not JSON: {err}")))?;
        let frame_type = frame
            .get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| invalid("stream frame without a type".to_string()))?;
        if let Some(event_name) = &event.event {
            if event_name != frame_type {
                return Err(invalid(format!(
                    "SSE event field {event_name:?} disagrees with the frame type \
                     {frame_type:?}: a corrupted frame is never guessed"
                )));
            }
        }
        match frame_type {
            "message_start" => {
                if self.usage_seen || !self.block_order.is_empty() {
                    return Err(invalid(
                        "message_start after the stream already opened: a second message is a \
                         protocol surprise"
                            .to_string(),
                    ));
                }
                // R05-T07: the input half (input_tokens + cache
                // categories) decodes through the same strict per-family
                // decoder and folds. A contract-violating count is kept
                // RAW (usage_violation) and spliced into the buffered
                // finish body, so the buffered parse marks the fact
                // invalid there — never a silent drop (C06).
                if let Some(message_usage) = frame.get("message").and_then(|m| m.get("usage")) {
                    self.usage_seen = true;
                    match super::usage::decode_family_usage(
                        lingxi_kernel::model_exchange::ProtocolFamily::AnthropicMessages,
                        &serde_json::json!({"usage": message_usage}),
                    ) {
                        super::usage::UsageDecode::Usage(fact) => {
                            self.usage.fold(fact).map_err(|conflict| {
                                invalid(format!(
                                    "message_start usage conflicts with an earlier fragment: \
                                     {conflict}"
                                ))
                            })?;
                        }
                        super::usage::UsageDecode::Invalid { .. } => {
                            self.usage_violation
                                .get_or_insert_with(|| message_usage.clone());
                        }
                        super::usage::UsageDecode::Absent => {}
                    }
                }
            }
            "content_block_start" => {
                let index = frame
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .ok_or_else(|| invalid("content_block_start without an index".to_string()))?;
                let block = frame.get("content_block").cloned().ok_or_else(|| {
                    invalid("content_block_start without a content_block".to_string())
                })?;
                if self.blocks.contains_key(&index) {
                    return Err(invalid(format!(
                        "content_block_start reopens index {index}: a duplicate block start is \
                         a conflict, never merged"
                    )));
                }
                let block_type = block
                    .get("type")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| invalid("content_block without a type".to_string()))?;
                let pending = match block_type {
                    "text" => {
                        let text = block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default()
                            .to_string();
                        if !text.is_empty() {
                            emitted.push(ModelTurnDelta::Text(text.clone()));
                        }
                        PendingBlock::Text(text)
                    }
                    "thinking" => {
                        let text = block
                            .get("thinking")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default()
                            .to_string();
                        if !text.is_empty() {
                            emitted.push(ModelTurnDelta::Reasoning(text.clone()));
                        }
                        PendingBlock::Thinking {
                            text,
                            signature: block
                                .get("signature")
                                .and_then(|s| s.as_str())
                                .map(str::to_string),
                        }
                    }
                    "tool_use" => PendingBlock::ToolUse {
                        id: block
                            .get("id")
                            .and_then(|i| i.as_str())
                            .ok_or_else(|| {
                                invalid("tool_use block start without an id".to_string())
                            })?
                            .to_string(),
                        name: block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .ok_or_else(|| {
                                invalid("tool_use block start without a name".to_string())
                            })?
                            .to_string(),
                        partial_json: String::new(),
                    },
                    // Complete-at-start unknown state — preserved verbatim.
                    _ => PendingBlock::Opaque(block),
                };
                self.block_order.push(index);
                self.blocks.insert(index, pending);
            }
            "content_block_delta" => {
                let index = frame
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .ok_or_else(|| invalid("content_block_delta without an index".to_string()))?;
                if self.closed.contains(&index) {
                    return Err(invalid(format!(
                        "content_block_delta for the already-closed block {index}: a late \
                         delta is never silently absorbed"
                    )));
                }
                let delta = frame
                    .get("delta")
                    .cloned()
                    .ok_or_else(|| invalid("content_block_delta without a delta".to_string()))?;
                let delta_type = delta.get("type").and_then(|t| t.as_str()).ok_or_else(|| {
                    invalid("content_block_delta's delta without a type".to_string())
                })?;
                let block = self.blocks.get_mut(&index).ok_or_else(|| {
                    invalid(format!(
                        "content_block_delta for never-opened block {index}: a delta without \
                         its block start is a protocol violation"
                    ))
                })?;
                match (delta_type, block) {
                    ("text_delta", PendingBlock::Text(text)) => {
                        let fragment = delta
                            .get("text")
                            .and_then(|t| t.as_str())
                            .ok_or_else(|| invalid("text_delta without text".to_string()))?;
                        if !fragment.is_empty() {
                            text.push_str(fragment);
                            emitted.push(ModelTurnDelta::Text(fragment.to_string()));
                        }
                    }
                    ("thinking_delta", PendingBlock::Thinking { text, .. }) => {
                        let fragment =
                            delta
                                .get("thinking")
                                .and_then(|t| t.as_str())
                                .ok_or_else(|| {
                                    invalid("thinking_delta without thinking".to_string())
                                })?;
                        if !fragment.is_empty() {
                            text.push_str(fragment);
                            emitted.push(ModelTurnDelta::Reasoning(fragment.to_string()));
                        }
                    }
                    ("signature_delta", PendingBlock::Thinking { signature, .. }) => {
                        let fragment =
                            delta
                                .get("signature")
                                .and_then(|s| s.as_str())
                                .ok_or_else(|| {
                                    invalid("signature_delta without signature".to_string())
                                })?;
                        match signature {
                            Some(existing) if existing != fragment => {
                                return Err(invalid(
                                    "a thinking block carried two DIFFERENT signatures: a \
                                     conflict is never merged"
                                        .to_string(),
                                ));
                            }
                            Some(_) => {}
                            None => *signature = Some(fragment.to_string()),
                        }
                    }
                    ("input_json_delta", PendingBlock::ToolUse { partial_json, .. }) => {
                        let fragment = delta
                            .get("partial_json")
                            .and_then(|t| t.as_str())
                            .ok_or_else(|| {
                                invalid("input_json_delta without partial_json".to_string())
                            })?;
                        if partial_json.len() + fragment.len() > TOOL_ARGUMENTS_MAX_BYTES {
                            return Err(invalid(format!(
                                "stream tool_use block {index} input exceeded the \
                                 {TOOL_ARGUMENTS_MAX_BYTES}-byte bound; refusing (a runaway \
                                 argument stream is a protocol failure, never an unbounded \
                                 buffer)"
                            )));
                        }
                        partial_json.push_str(fragment);
                    }
                    (delta_type, PendingBlock::Opaque(_)) => {
                        return Err(invalid(format!(
                            "stream delta {delta_type:?} for an opaque (complete-at-start) \
                             block {index}: the adapter cannot know its semantics and never \
                             drops content silently"
                        )));
                    }
                    (delta_type, _) => {
                        return Err(invalid(format!(
                            "stream delta {delta_type:?} does not match its block {index}'s \
                             type: a mismatched delta is a protocol violation"
                        )));
                    }
                }
            }
            "content_block_stop" => {
                let index = frame
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .ok_or_else(|| invalid("content_block_stop without an index".to_string()))?;
                if !self.blocks.contains_key(&index) {
                    return Err(invalid(format!(
                        "content_block_stop for never-opened block {index}"
                    )));
                }
                if !self.closed.insert(index) {
                    return Err(invalid(format!(
                        "content_block_stop for the already-closed block {index}"
                    )));
                }
            }
            "message_delta" => {
                if let Some(reason) = frame
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|r| r.as_str())
                {
                    match &self.stop_reason {
                        Some(existing) if existing != reason => {
                            return Err(invalid(format!(
                                "stream carried two different stop_reason values ({existing:?} \
                                 then {reason:?}): a conflict is never merged"
                            )));
                        }
                        _ => self.stop_reason = Some(reason.to_string()),
                    }
                }
                // R05-T07 (C04): message_delta.usage.output_tokens is the
                // RUNNING cumulative total — a growing value REPLACES the
                // earlier one (never summed, never a conflict); a value
                // going backwards is the loud conflict it would be.
                if let Some(delta_usage) = frame.get("usage") {
                    self.usage_seen = true;
                    match super::usage::decode_family_usage(
                        lingxi_kernel::model_exchange::ProtocolFamily::AnthropicMessages,
                        &serde_json::json!({"usage": delta_usage}),
                    ) {
                        super::usage::UsageDecode::Usage(fact) => {
                            self.usage.fold(fact).map_err(|conflict| {
                                invalid(format!(
                                    "message_delta usage conflicts with the running total: \
                                     {conflict}"
                                ))
                            })?;
                        }
                        // Same C06 rule as the buffered mode: the violating
                        // fragment is preserved for the finish splice — a
                        // drop would erase the violation's only trace.
                        super::usage::UsageDecode::Invalid { .. } => {
                            self.usage_violation
                                .get_or_insert_with(|| delta_usage.clone());
                        }
                        super::usage::UsageDecode::Absent => {}
                    }
                }
            }
            "message_stop" => self.message_stop_seen = true,
            "ping" => {}
            "error" => {
                let error_type = frame
                    .get("error")
                    .and_then(|e| e.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");
                let message = frame
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("provider stream error event");
                return Err(match error_type {
                    "overloaded_error" | "api_error" => ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        format!("provider stream error ({error_type}): {message}"),
                        true,
                    ),
                    _ => ProtocolError::new(
                        ErrorCode::InvalidMessage,
                        format!("provider stream error ({error_type}): {message}"),
                        false,
                    ),
                });
            }
            other => {
                return Err(invalid(format!(
                    "unknown stream event type {other:?}: an unrecognized event may carry \
                     content; it is never dropped silently"
                )));
            }
        }
        Ok(emitted)
    }

    /// Validates the CLOSED batch (`message_stop` seen) through the SAME
    /// buffered parser. A stream without `message_stop` is truncated: loud,
    /// never half-parsed (C05/C09).
    pub fn finish(
        self,
        call: &ModelCallId,
        snapshot: &ToolDeclarationSnapshot,
        schema_budget: &SchemaBudget,
    ) -> Result<ParsedChat, ProtocolError> {
        let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
        if !self.message_stop_seen {
            return Err(invalid(
                "stream ended without message_stop: a truncated stream is never half-parsed \
                 into a turn"
                    .to_string(),
            ));
        }
        let mut content: Vec<serde_json::Value> = Vec::new();
        for index in &self.block_order {
            match self.blocks.get(index).expect("ordered above") {
                PendingBlock::Text(text) => {
                    content.push(serde_json::json!({"type": "text", "text": text}));
                }
                PendingBlock::Thinking { text, signature } => {
                    let mut block = serde_json::json!({"type": "thinking", "thinking": text});
                    if let Some(signature) = signature {
                        block["signature"] = serde_json::Value::String(signature.clone());
                    }
                    content.push(block);
                }
                PendingBlock::ToolUse {
                    id,
                    name,
                    partial_json,
                } => {
                    let raw = if partial_json.is_empty() {
                        "{}"
                    } else {
                        partial_json.as_str()
                    };
                    let input: serde_json::Value = serde_json::from_str(raw).map_err(|err| {
                        invalid(format!(
                            "tool_use block {index} input is not complete JSON at stream end: \
                             {err} (a half-JSON batch is never dispatched)"
                        ))
                    })?;
                    content.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
                PendingBlock::Opaque(block) => content.push(block.clone()),
            }
        }
        let mut body = serde_json::json!({ "content": content });
        if let Some(stop_reason) = self.stop_reason {
            body["stop_reason"] = serde_json::Value::String(stop_reason);
        }
        // R05-T07: the folded usage fact splices back into the buffered
        // shape (component fields included) — the buffered parse then
        // decodes it through the SAME strict decoder, so provenance,
        // components and invalid-vs-unknown behave identically in both
        // wire modes. An interrupted stream keeps whatever halves arrived
        // (a Partial fact — never a fabricated zero). A contract-violating
        // fragment (F-01) splices its RAW numbers through the same body:
        // the buffered decode then marks the WHOLE fact invalid and names
        // the violation — a violating count never disappears silently.
        let folded = self.usage.clone().finish();
        let mut usage = serde_json::json!({});
        if let Some(folded) = folded {
            if let Some(tokens) = folded.input_tokens {
                usage["input_tokens"] = serde_json::json!(tokens);
            }
            if let Some(tokens) = folded.output_tokens {
                usage["output_tokens"] = serde_json::json!(tokens);
            }
            if let Some(tokens) = folded.cache_read_tokens {
                usage["cache_read_input_tokens"] = serde_json::json!(tokens);
            }
            if let Some(tokens) = folded.cache_write_tokens {
                usage["cache_creation_input_tokens"] = serde_json::json!(tokens);
            }
        }
        if let Some(violation) = &self.usage_violation {
            if let (Some(target), Some(source)) = (usage.as_object_mut(), violation.as_object()) {
                for (field, value) in source {
                    target.insert(field.clone(), value.clone());
                }
            }
        }
        if !usage.as_object().is_some_and(serde_json::Map::is_empty) {
            body["usage"] = usage;
        }
        parse_messages_response(call, snapshot, &body, schema_budget)
    }
}

impl super::dispatch::StreamAccumulator for MessagesStreamAccumulator {
    fn handle_event(&mut self, event: &SseEvent) -> Result<Vec<ModelTurnDelta>, ProtocolError> {
        MessagesStreamAccumulator::handle_event(self, event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_kernel::model_exchange::{
        CredentialAuthKind, CredentialReference, ModelOperation, ProtocolFamily, ToolDeclaration,
        ToolDeclarationSnapshot,
    };
    use lingxi_protocol::{ToolCallId, UsageRecord};

    fn route() -> ResolvedModelRoute {
        ResolvedModelRoute {
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            operation: ModelOperation::Chat,
            protocol: ProtocolFamily::AnthropicMessages,
            endpoint: "https://api.anthropic.test".to_string(),
            credential: CredentialReference {
                provider: "anthropic".to_string(),
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
    fn request_shape_system_tools_and_max_tokens() {
        let mut input = ModelTurnInput::first_turn("hello", snapshot_with_read());
        input.system_prompt = Some("be terse".to_string());
        let body = render_messages_request(&input, &route(), DEFAULT_MAX_OUTPUT_TOKENS, false)
            .expect("renders");
        assert_eq!(body["model"], "claude-test");
        assert_eq!(body["max_tokens"], 16384);
        assert_eq!(body["system"], "be terse");
        assert_eq!(body["stream"], false);
        assert_eq!(
            body["messages"][0],
            serde_json::json!({"role":"user","content":"hello"})
        );
        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(
            body["tools"][0]["input_schema"]["properties"]["path"]["type"],
            "string"
        );
        // No system prompt → the key is absent entirely.
        let bare = render_messages_request(
            &ModelTurnInput::first_turn("hi", ToolDeclarationSnapshot::empty()),
            &route(),
            DEFAULT_MAX_OUTPUT_TOKENS,
            false,
        )
        .expect("renders");
        assert!(bare.get("system").is_none());
        assert!(bare.get("tools").is_none());
    }

    #[test]
    fn tool_roundtrip_renders_tool_use_and_merged_tool_results() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let mk = |path: &str, id: &str, seq: u32| {
            let request = ToolRequest::from_effective_arguments(
                "tool:first-party:read",
                serde_json::json!({"path": path}),
                &budget,
            )
            .expect("effective")
            .with_provider_call_id(id);
            lingxi_kernel::model_exchange::RequestedToolCall {
                tool_call_id: ToolCallId::new(format!("run-tc{seq:04}")),
                provider_call_id: request.provider_call_id.clone(),
                target: request.target.clone(),
                arguments: request.arguments.clone(),
                args_digest: request.args_digest.clone(),
                args_summary: None,
            }
        };
        let mut input = ModelTurnInput::first_turn("read both", snapshot);
        input.prior.push(ExchangeItem::AssistantTurn {
            call: ModelCallId::new("mc-1"),
            content: vec![ContentBlock::Text {
                text: "reading both".to_string(),
            }],
            tool_calls: vec![mk("/tmp/a", "toolu_a", 1), mk("/tmp/b", "toolu_b", 2)],
        });
        input.prior.push(ExchangeItem::ToolResult {
            tool_call_id: ToolCallId::new("run-tc0001"),
            provider_call_id: Some("toolu_a".to_string()),
            outcome: ToolOutcome::success_text("body a"),
        });
        input.prior.push(ExchangeItem::ToolResult {
            tool_call_id: ToolCallId::new("run-tc0002"),
            provider_call_id: Some("toolu_b".to_string()),
            outcome: ToolOutcome::Failed {
                error: ProtocolError::new(ErrorCode::Forbidden, "denied", false),
            },
        });
        let body = render_messages_request(&input, &route(), DEFAULT_MAX_OUTPUT_TOKENS, false)
            .expect("renders");
        let assistant = &body["messages"][1];
        assert_eq!(assistant["role"], "assistant");
        let blocks = assistant["content"].as_array().expect("blocks");
        assert_eq!(
            blocks[0],
            serde_json::json!({"type":"text","text":"reading both"})
        );
        assert_eq!(
            blocks[1],
            serde_json::json!({"type":"tool_use","id":"toolu_a","name":"read","input":{"path":"/tmp/a"}})
        );
        assert_eq!(
            blocks[2],
            serde_json::json!({"type":"tool_use","id":"toolu_b","name":"read","input":{"path":"/tmp/b"}})
        );
        // Parallel results merged into ONE user message (C03 pairing).
        let results = &body["messages"][2];
        assert_eq!(results["role"], "user");
        let results = results["content"].as_array().expect("results");
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0],
            serde_json::json!({"type":"tool_result","tool_use_id":"toolu_a","content":"body a","is_error":false})
        );
        assert_eq!(results[1]["tool_use_id"], "toolu_b");
        assert_eq!(results[1]["is_error"], true);
        assert!(results[1]["content"].as_str().unwrap().contains("denied"));
    }

    #[test]
    fn thinking_and_signature_roundtrip_as_state_never_text() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0001");
        let body = serde_json::json!({
            "id": "msg_ignored",
            "model": "the-provider-claims-this",
            "content": [
                {"type": "thinking", "thinking": "checking the file", "signature": "sig-abc"},
                {"type": "text", "text": "let me read"},
                {"type": "redacted_thinking", "data": "encrypted-xyz"},
                {"type": "tool_use", "id": "toolu_1", "name": "read", "input": {"path": "/tmp/a"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 9, "output_tokens": 4}
        });
        let parsed = parse_messages_response(&call, &snapshot, &body, &budget).expect("parses");
        assert_eq!(
            parsed.usage(),
            Some(UsageRecord {
                input_tokens: 9,
                output_tokens: 4
            })
        );
        let (requests, content) = match parsed.turn {
            ProviderTurn::ToolRequests { requests, content } => (requests, content),
            other => panic!("expected ToolRequests, got {other:?}"),
        };
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].provider_call_id.as_deref(), Some("toolu_1"));
        // Order preserved: reasoning, signature opaque, text, redacted opaque.
        assert_eq!(content.len(), 4);
        assert!(
            matches!(&content[0], ContentBlock::Reasoning { text } if text == "checking the file")
        );
        assert!(
            matches!(&content[1], ContentBlock::Opaque { provider, data }
            if provider == FAMILY && data["type"] == "thinking_signature" && data["signature"] == "sig-abc")
        );
        assert!(matches!(&content[2], ContentBlock::Text { text } if text == "let me read"));
        assert!(
            matches!(&content[3], ContentBlock::Opaque { provider, data }
            if provider == FAMILY && data["type"] == "redacted_thinking")
        );

        // The SAME exchange re-renders the state verbatim (C10 round trip).
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
        });
        let body = render_messages_request(&input, &route(), DEFAULT_MAX_OUTPUT_TOKENS, false)
            .expect("renders");
        let blocks = body["messages"][1]["content"].as_array().expect("blocks");
        assert_eq!(
            blocks[0],
            serde_json::json!({"type":"thinking","thinking":"checking the file","signature":"sig-abc"})
        );
        assert_eq!(
            blocks[1],
            serde_json::json!({"type":"text","text":"let me read"})
        );
        assert_eq!(
            blocks[2],
            serde_json::json!({"type":"redacted_thinking","data":"encrypted-xyz"})
        );
        assert_eq!(blocks[3]["type"], "tool_use");
    }

    #[test]
    fn cross_family_opaque_state_is_never_echoed() {
        let mut input = ModelTurnInput::first_turn("go on", ToolDeclarationSnapshot::empty());
        input.prior.push(ExchangeItem::AssistantTurn {
            call: ModelCallId::new("mc-1"),
            content: vec![
                ContentBlock::Reasoning {
                    text: "unsigned reasoning".to_string(),
                },
                ContentBlock::Opaque {
                    provider: "openai-responses".to_string(),
                    data: serde_json::json!({"type": "reasoning", "id": "rs_x"}),
                },
                ContentBlock::Text {
                    text: "visible".to_string(),
                },
            ],
            tool_calls: Vec::new(),
        });
        let body = render_messages_request(&input, &route(), DEFAULT_MAX_OUTPUT_TOKENS, false)
            .expect("renders");
        let blocks = body["messages"][1]["content"].as_array().expect("blocks");
        // Only the text survives: unsigned reasoning has no wire shape and
        // the other family's opaque is never echoed (C10).
        assert_eq!(
            blocks.as_slice(),
            &[serde_json::json!({"type":"text","text":"visible"})]
        );
    }

    #[test]
    fn unknown_tool_and_malformed_blocks_are_loud() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0002");
        let err = parse_messages_response(
            &call,
            &snapshot,
            &serde_json::json!({
                "content": [{"type":"tool_use","id":"t1","name":"invented","input":{}}],
                "stop_reason": "tool_use"
            }),
            &budget,
        )
        .unwrap_err();
        assert!(err.message.contains("unknown tool"));
        let err = parse_messages_response(
            &call,
            &snapshot,
            &serde_json::json!({"content": [{"nope": true}]}),
            &budget,
        )
        .unwrap_err();
        assert!(err.message.contains("without a type"));
    }

    /// R05-T04 (C11): `max_tokens` is a TRUNCATION — the call settles
    /// `Failed { BudgetExceeded, retryable: true }` (zero side effects, so
    /// the run's attempt policy may retry), never a silent Empty.
    #[test]
    fn a_max_tokens_stop_maps_to_retryable_budget_failure() {
        let call = ModelCallId::new("run-mc0003");
        let parsed = parse_messages_response(
            &call,
            &ToolDeclarationSnapshot::empty(),
            &serde_json::json!({"content": [], "stop_reason": "max_tokens"}),
            &SchemaBudget::default(),
        )
        .expect("parses");
        match parsed.turn {
            ProviderTurn::Failed { error, retryable } => {
                assert_eq!(error.code, ErrorCode::BudgetExceeded);
                assert!(retryable);
                assert!(error.message.contains("max_tokens"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        // `refusal` is a non-retryable Forbidden.
        let parsed = parse_messages_response(
            &call,
            &ToolDeclarationSnapshot::empty(),
            &serde_json::json!({"content": [], "stop_reason": "refusal"}),
            &SchemaBudget::default(),
        )
        .expect("parses");
        match parsed.turn {
            ProviderTurn::Failed { error, retryable } => {
                assert_eq!(error.code, ErrorCode::Forbidden);
                assert!(!retryable);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
