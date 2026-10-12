//! The `openai-completions` protocol adapter (R05-T01; T03 extensions:
//! system-prompt slot, full-state tool-outcome rendering, streaming
//! decode).
//!
//! Rendering rules (each is a documented protocol-rendering decision, never
//! a silent state decision):
//! - The transcript is `[system?] + [user(submission)] + render(prior
//!   exchange)`. The system slot renders only when the host resolved one
//!   (R05-T03 `ModelTurnInput::system_prompt`; `None` omits the message
//!   entirely). The submission carries the driver's steering merge
//!   verbatim.
//! - Assistant turns render their text blocks; reasoning / provider-opaque
//!   blocks have no representation in this family's INPUT shape and are not
//!   echoed (a reasoning-only turn with no tool calls has no wire shape and
//!   is skipped at render).
//! - Tool calls re-render with the CURRENT turn's declaration snapshot
//!   (target → wire name); a target missing from the snapshot (the registry
//!   moved mid-run) is a loud render failure, never a name guess.
//! - Tool results require the provider correlation id; an exchange item
//!   without one cannot be paired on this protocol and fails loudly.
//! - Identity flows ONLY from the driver: the request's `model` is the
//!   resolved route's model, the final message's `model_call_id` is the
//!   driver-minted call id. Identity fields in the provider's RESPONSE
//!   (`model`, `id`, ...) are ignored entirely (C10).
//!
//! Streaming (R05-T04): production dispatch is the family's SSE mode
//! (`stream: true` + `stream_options.include_usage`). Chunks are decoded
//! INCREMENTALLY through [`super::streaming::SseDecoder`] as the network
//! delivers them; text/reasoning fragments are emitted to the turn's
//! [`lingxi_kernel::ports::TurnDeltaSink`] at arrival (never buffered to
//! the end and re-sliced), tool-call fragments accumulate per `index`
//! behind the argument bound and are validated + dispatched ONLY after the
//! `[DONE]` terminal marker closes the batch (C05/C09: a half-JSON or
//! truncated batch has zero side effects). The reassembled shape is
//! validated by the SAME [`parse_chat_response`] as the buffered mode —
//! one contract, two wire modes; [`parse_chat_stream`] remains the offline
//! (golden/test) entry over already-decoded events.

use lingxi_kernel::model_exchange::{
    ExchangeItem, ModelTurnInput, ResolvedModelRoute, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{ModelTurnDelta, ProviderTurn, ProviderTurnResult, ToolRequest};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::usage::ReportedUsage;
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError};
use serde::{Deserialize, Serialize};

use super::credentials::ApplicableAuth;
use super::dispatch::{self, BearerStyle};
use super::streaming::{SseEvent, TOOL_ARGUMENTS_MAX_BYTES};
use super::tool_render::render_tool_outcome_text;
use super::ParsedChat;

// ── outbound request shapes (typed serde: the current-schema guarantee on
//    the supplier-bound direction) ──────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ToolSpec>,
    /// This adapter requests a complete (non-streamed) response by
    /// default; the streaming path flips this and adds `stream_options`.
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
    /// R05-T06: the host-decided output-token cap (worker callbacks / aux
    /// slots — incumbent `callText` `max_tokens`). `None` = no cap key on
    /// the wire at all (the incumbent chat path behavior; the field is
    /// never defaulted).
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// One content part of a multimodal user message (R05-T06) — the
/// incumbent `serializeOpenAICompatibleContentBlock` image shape
/// (`core/provider-media-serializer.ts`).
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum UserContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlPart },
}

#[derive(Debug, Serialize)]
struct ImageUrlPart {
    url: String,
}

/// Renders the submission user message: bare-string content when the turn
/// carries no images (byte-identical incumbent chat shape), parts form
/// otherwise (text first, then the images in host order — the incumbent
/// `injectMessageNotes` ordering).
fn submission_user_message(input: &ModelTurnInput) -> RequestMessage {
    if input.images.is_empty() {
        return RequestMessage::User {
            content: input.submission.clone(),
        };
    }
    let mut content = Vec::with_capacity(input.images.len() + 1);
    content.push(UserContentPart::Text {
        text: input.submission.clone(),
    });
    for image in &input.images {
        content.push(UserContentPart::ImageUrl {
            image_url: ImageUrlPart {
                url: super::image_data_url(image),
            },
        });
    }
    RequestMessage::UserParts { content }
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    /// The final chunk carries the real usage (never an estimate).
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct ToolSpec {
    #[serde(rename = "type")]
    kind: &'static str,
    function: FunctionSpec,
}

#[derive(Debug, Serialize)]
struct FunctionSpec {
    name: String,
    description: String,
    /// The verbatim input schema document (never normalized or pruned).
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(tag = "role")]
enum RequestMessage {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User { content: String },
    /// The multimodal user shape (R05-T06): text + host-authorized image
    /// parts. Only emitted when the turn actually carries images — a
    /// text-only turn keeps the incumbent bare-string content
    /// byte-identically.
    #[serde(rename = "user")]
    UserParts { content: Vec<UserContentPart> },
    #[serde(rename = "assistant")]
    Assistant {
        /// `null` when the turn carried tool calls only (the protocol's
        /// required shape for that case).
        content: Option<String>,
        /// R05 RR1 F08: the family's reasoning carrier (the
        /// DeepSeek/Kimi/MiMo/Zhipu `reasoning_content` vocabulary). The
        /// renderer materializes the turn's REAL canonical reasoning here —
        /// the compat replay policy then keeps it (a declared
        /// reasoning_content contract), refuses without it, or strips it
        /// (no contract / thinking off); a carrier is never fabricated.
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<RequestToolCall>,
    },
    #[serde(rename = "tool")]
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct RequestToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    function: RequestFunction,
}

#[derive(Debug, Serialize)]
struct RequestFunction {
    name: String,
    /// The arguments as a JSON-encoded string (the protocol's shape).
    arguments: String,
}

// ── inbound response shapes (typed; unknown provider fields are ignored,
//    identity fields are never read) ───────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    #[serde(default)]
    choices: Vec<ResponseChoice>,
    // R05-T07: the usage object is decoded from the raw body by the
    // STRICT per-family decoder (negative/float/string counts mark the
    // fact invalid instead of failing the whole response shape).
}

#[derive(Debug, Deserialize)]
struct ResponseChoice {
    #[serde(default)]
    message: Option<ResponseMessage>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    /// String, array of parts, or null — normalized in
    /// [`content_blocks_of`].
    #[serde(default)]
    content: Option<serde_json::Value>,
    /// R05 RR1 F08: the buffered wire mode's reasoning carrier (the
    /// DeepSeek-style top-level `message.reasoning_content`; a string or a
    /// parts array). Parsed to canonical reasoning so BOTH wire modes
    /// preserve the replay state.
    #[serde(default)]
    reasoning_content: Option<serde_json::Value>,
    #[serde(default)]
    tool_calls: Option<Vec<ResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ResponseToolCall {
    id: String,
    function: ResponseFunction,
}

#[derive(Debug, Deserialize)]
struct ResponseFunction {
    name: String,
    /// Spec shape is a JSON string; some deployments emit the object
    /// directly — both are accepted and validated identically.
    #[serde(default)]
    arguments: Option<serde_json::Value>,
}

/// The real HTTP adapter for the openai-completions family. Stateless per
/// call: the route and the credential arrive per dispatch (a config reload
/// takes effect on the NEXT call, C05). The client follows NO redirect
/// (C10): a credential-bearing request is never replayed onto a redirected
/// origin — a 3xx answer is a loud protocol failure. R05-T05: the connect /
/// first-byte / total-budget segments of [`dispatch::HttpTimeouts`] +
/// `ModelTurnInput::deadline_unix_ms` apply on every dispatch.
pub struct OpenAiCompletionsAdapter {
    client: dispatch::NetworkClient,
    schema_budget: SchemaBudget,
    timeouts: dispatch::HttpTimeouts,
}

impl OpenAiCompletionsAdapter {
    pub fn new(schema_budget: SchemaBudget) -> Result<Self, ProtocolError> {
        Self::new_with_timeouts(schema_budget, dispatch::HttpTimeouts::default())
    }

    /// Explicit timeout segments (R05-T05; tests inject shorter windows —
    /// production keeps the pre-registered defaults).
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

    /// Executes ONE chat turn against the resolved route with the resolved
    /// credential material (R05-T04: the production wire mode is the
    /// family's SSE stream). Live text/reasoning fragments are emitted to
    /// `deltas` AS THE STREAM DELIVERS them; the terminal turn is
    /// assembled and validated only after the `[DONE]` sentinel closes the
    /// batch. The result fence echoes the issued context; `served_by` names
    /// the route that actually served the call (R05-T01 identity honesty).
    /// Failures are [`ProviderTurn::Failed`] with an honest retryable
    /// classification — never a fabricated reply, never a panic. A 401 maps
    /// to [`ErrorCode::Unauthorized`] (the caller's single bounded
    /// refresh-and-retry applies); a 403 is [`ErrorCode::Forbidden`] and
    /// never triggers a credential refresh. Provider error echoes are
    /// scrubbed of the in-play material before they can travel (C09). An
    /// `Err` from the sink means the driver is gone: the read stops and the
    /// call winds down as cancelled (never buffered "for later").
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
        let body = match render_chat_request(input, route, true) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        // R05-T05: the ported provider-compat layer patches the rendered
        // envelope before dispatch (None pins the byte-exact golden wire).
        let body = match super::compat::apply_for_call(body, route, compat) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        let url = dispatch::append_provider_api_path(&route.endpoint, "/chat/completions");
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
        let accumulator = ChatStreamAccumulator::new();
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
        // failure must not erase it (an unexpected tool response on a
        // tools-less call still accounts).
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
                    lingxi_kernel::model_exchange::ProtocolFamily::OpenAiCompletions
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

/// Builds the request body for one turn (pure — the testable half of the
/// outbound direction). `streaming` flips the wire mode (the T01 default
/// is buffered).
pub fn render_chat_request(
    input: &ModelTurnInput,
    route: &ResolvedModelRoute,
    streaming: bool,
) -> Result<serde_json::Value, ProtocolError> {
    let mut messages = Vec::new();
    if let Some(system) = &input.system_prompt {
        messages.push(RequestMessage::System {
            content: system.clone(),
        });
    }
    messages.push(submission_user_message(input));
    for item in &input.prior {
        match item {
            ExchangeItem::AssistantTurn {
                content,
                tool_calls,
                ..
            } => {
                let mut text = String::new();
                // R05 RR1 F08: the REAL reasoning carrier. The turn's
                // Reasoning blocks (in content order) materialize as the
                // family's `reasoning_content` message field — the compat
                // replay policy then decides per provider/model/purpose
                // whether it rides the wire, is refused as missing, or is
                // stripped. Provider-opaque blocks keep having no
                // representation in this family's INPUT shape.
                let mut reasoning: Vec<String> = Vec::new();
                for block in content {
                    match block {
                        ContentBlock::Text { text: block_text } => {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(block_text);
                        }
                        ContentBlock::Reasoning { text: block_text } => {
                            if !block_text.is_empty() {
                                reasoning.push(block_text.clone());
                            }
                        }
                        ContentBlock::Opaque { .. } => {}
                        ContentBlock::ResourceRef { resource } => {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            match &resource.uri {
                                Some(uri) => text.push_str(&format!("[resource: {uri}]")),
                                None => text.push_str("[resource]"),
                            }
                        }
                    }
                }
                let mut rendered_calls = Vec::with_capacity(tool_calls.len());
                for call in tool_calls {
                    let wire_name =
                        input
                            .tools
                            .wire_name_of_target(&call.target)
                            .ok_or_else(|| {
                                ProtocolError::new(
                                    ErrorCode::InvalidMessage,
                                    format!(
                                    "exchange history names tool target {:?}, which the current \
                                     declaration snapshot does not contain (the registry moved \
                                     mid-run); cannot faithfully re-render the transcript",
                                    call.target
                                ),
                                    false,
                                )
                            })?;
                    let provider_call_id = call.provider_call_id.clone().ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::InvalidMessage,
                            format!(
                                "exchange history tool call {} ({}) carries no provider \
                                 correlation id; this protocol requires the pairing",
                                call.tool_call_id, call.target
                            ),
                            false,
                        )
                    })?;
                    rendered_calls.push(RequestToolCall {
                        id: provider_call_id,
                        kind: "function",
                        function: RequestFunction {
                            name: wire_name.to_string(),
                            arguments: serde_json::to_string(call.arguments.as_value()).map_err(
                                |err| {
                                    ProtocolError::new(
                                        ErrorCode::Internal,
                                        format!("effective arguments serialize: {err}"),
                                        false,
                                    )
                                },
                            )?,
                        },
                    });
                }
                let reasoning_content = (!reasoning.is_empty()).then(|| reasoning.join("\n"));
                if text.is_empty() && rendered_calls.is_empty() {
                    // A reasoning-only turn carries no wire message of its
                    // own: the require-tool-call replay contract binds the
                    // carrier to TOOL-CALL messages (a reasoning-only turn
                    // has none), and emitting a bare assistant message here
                    // would leave an EMPTY assistant message on providers
                    // whose policy strips the carrier. Its content stays
                    // durable in the run's exchange, not re-sent.
                    continue;
                }
                messages.push(RequestMessage::Assistant {
                    content: if text.is_empty() { None } else { Some(text) },
                    reasoning_content,
                    tool_calls: rendered_calls,
                });
            }
            ExchangeItem::ToolResult {
                tool_call_id,
                provider_call_id,
                outcome,
            } => {
                let provider_call_id = provider_call_id.clone().ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::InvalidMessage,
                        format!(
                            "exchange history tool result {tool_call_id} carries no provider \
                             correlation id; this protocol requires the pairing"
                        ),
                        false,
                    )
                })?;
                messages.push(RequestMessage::Tool {
                    tool_call_id: provider_call_id,
                    content: render_tool_outcome_text(outcome),
                });
            }
            // R06-T02: 压缩摘要是普通 user 角色历史（现役 convertToLlm 的
            // 包装），绝不获得系统指令优先级；mid-run 时再补一条 notice
            // user 消息（现役 MIDRUN_COMPACTION_NOTICE，逐字）。
            ExchangeItem::CompactionSummary {
                summary, mid_run, ..
            } => {
                messages.push(RequestMessage::User {
                    content: lingxi_kernel::compaction::render_summary_message_text(summary),
                });
                if *mid_run {
                    messages.push(RequestMessage::User {
                        content: lingxi_kernel::compaction::MIDRUN_COMPACTION_NOTICE.to_string(),
                    });
                }
            }
            // R06-T02: 请求作用域的压缩指令（仅出现在摘要调用自身的
            // prior 末尾；永不进入 run 的 live exchange——kernel planner
            // 对携带它的交换响亮拒绝）。
            ExchangeItem::CompactionInstruction { text } => {
                messages.push(RequestMessage::User {
                    content: text.clone(),
                });
            }
        }
    }
    let tools = input
        .tools
        .declarations
        .iter()
        .map(|declaration| ToolSpec {
            kind: "function",
            function: FunctionSpec {
                name: declaration.wire_name.clone(),
                description: declaration.description.clone(),
                parameters: declaration.input_schema.schema.clone(),
            },
        })
        .collect();
    let request = ChatCompletionsRequest {
        model: route.model.clone(),
        messages,
        tools,
        stream: streaming,
        stream_options: streaming.then_some(StreamOptions {
            include_usage: true,
        }),
        max_tokens: input.max_output_tokens,
    };
    serde_json::to_value(&request).map_err(|err| {
        ProtocolError::new(
            ErrorCode::Internal,
            format!("request serialization failed: {err}"),
            false,
        )
    })
}

/// R05 RR1 F08: the buffered reasoning carrier's text — a plain string
/// verbatim, or a parts array joined on each part's reasoning-bearing text
/// field (the incumbent `reasoningTextFromValue` vocabulary; empty pieces
/// drop, joined with `\n`).
fn reasoning_text_of(value: &Option<serde_json::Value>) -> Option<String> {
    match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Array(parts)) => {
            let pieces: Vec<String> = parts
                .iter()
                .filter_map(|part| {
                    [
                        "reasoning",
                        "reasoning_content",
                        "reasoning_text",
                        "thinking",
                    ]
                    .iter()
                    .find_map(|field| part.get(field).and_then(|t| t.as_str()).map(str::to_string))
                })
                .filter(|piece| !piece.is_empty())
                .collect();
            (!pieces.is_empty()).then(|| pieces.join("\n"))
        }
        // A non-string / non-array carrier is unknown provider state — it
        // stays uninterpreted (never guessed into reasoning text).
        Some(_) => None,
    }
}

/// Normalizes the provider's `content` value (string | parts array | null)
/// into content blocks, preserving the parts' relative order (R05-T03 C06:
/// a mixed response keeps EVERY valid block in place). Text parts stay
/// text; `reasoning` parts stay reasoning; unknown part types are
/// preserved as provider-opaque blocks (never dropped, never guessed).
fn content_blocks_of(content: &Option<serde_json::Value>, provider: &str) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    match content {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::String(text)) => {
            if !text.is_empty() {
                blocks.push(ContentBlock::Text { text: text.clone() });
            }
        }
        Some(serde_json::Value::Array(parts)) => {
            for part in parts {
                let part_type = part.get("type").and_then(|t| t.as_str());
                match part_type {
                    Some("text") | Some("output_text") => {
                        if let Some(part_text) = part.get("text").and_then(|t| t.as_str()) {
                            if !part_text.is_empty() {
                                blocks.push(ContentBlock::Text {
                                    text: part_text.to_string(),
                                });
                            }
                        }
                    }
                    Some("reasoning") | Some("thinking") => {
                        let reasoning = part
                            .get("text")
                            .or_else(|| part.get("reasoning"))
                            .or_else(|| part.get("reasoning_content"))
                            .or_else(|| part.get("thinking"))
                            .and_then(|t| t.as_str());
                        if let Some(reasoning) = reasoning {
                            if !reasoning.is_empty() {
                                blocks.push(ContentBlock::Reasoning {
                                    text: reasoning.to_string(),
                                });
                            }
                        }
                    }
                    _ => blocks.push(ContentBlock::Opaque {
                        provider: provider.to_string(),
                        data: part.clone(),
                    }),
                }
            }
        }
        Some(other) => blocks.push(ContentBlock::Opaque {
            provider: provider.to_string(),
            data: other.clone(),
        }),
    }
    blocks
}

pub const FAMILY: &str = "openai-completions";

/// Parses a successful response body into the provider turn plus the
/// reported usage (pure — the testable half of the inbound direction).
/// Every malformed shape is a loud [`ErrorCode::InvalidMessage`], never a
/// guessed default.
pub fn parse_chat_response(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    body: &serde_json::Value,
    schema_budget: &SchemaBudget,
) -> Result<ParsedChat, ProtocolError> {
    let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
    let response: ChatCompletionsResponse =
        serde_json::from_value(body.clone()).map_err(|err| {
            invalid(format!(
                "provider response violates the response schema: {err}"
            ))
        })?;
    // R05-T07: the strict per-family decode (component tokens +
    // provenance + the invalid-vs-unknown distinction). The wire
    // projection is DERIVED from the fact.
    let usage_report = match super::usage::decode_family_usage(
        lingxi_kernel::model_exchange::ProtocolFamily::OpenAiCompletions,
        body,
    ) {
        super::usage::UsageDecode::Absent => ReportedUsage::Unknown,
        super::usage::UsageDecode::Usage(usage) => ReportedUsage::Known(usage),
        super::usage::UsageDecode::Invalid { detail } => ReportedUsage::Invalid { detail },
    };
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| invalid("provider response carries zero choices".to_string()))?;
    let message = choice
        .message
        .ok_or_else(|| invalid("provider response choice carries no message".to_string()))?;
    let mut content = content_blocks_of(&message.content, "openai-completions");
    // R05 RR1 F08: the buffered top-level reasoning carrier (a string, or a
    // parts array the incumbent's `reasoningTextFromValue` joins) becomes
    // canonical reasoning — BEFORE the content blocks (the family's arrival
    // order: the reasoning precedes the visible answer), and only when the
    // content parts did not already carry the reasoning (never doubled).
    if !content
        .iter()
        .any(|block| matches!(block, ContentBlock::Reasoning { .. }))
    {
        if let Some(reasoning) = reasoning_text_of(&message.reasoning_content) {
            if !reasoning.is_empty() {
                content.insert(0, ContentBlock::Reasoning { text: reasoning });
            }
        }
    }
    let tool_calls = message.tool_calls.unwrap_or_default();
    let mut requests = Vec::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        let target = snapshot
            .target_of_wire_name(&tool_call.function.name)
            .ok_or_else(|| {
                invalid(format!(
                    "provider requested unknown tool {:?}: not a name this call declared \
                     (a wire name maps to a target through the send-time snapshot only — \
                     never a guess)",
                    tool_call.function.name
                ))
            })?
            .to_string();
        let arguments = match tool_call.function.arguments {
            Some(serde_json::Value::String(raw)) => serde_json::from_str(&raw).map_err(|err| {
                invalid(format!(
                    "tool call arguments for {:?} are not JSON: {err}",
                    tool_call.function.name
                ))
            })?,
            Some(value @ serde_json::Value::Object(_)) => value,
            Some(other) => {
                return Err(invalid(format!(
                    "tool call arguments for {:?} have an unsupported shape ({})",
                    tool_call.function.name,
                    match other {
                        serde_json::Value::Null => "null",
                        serde_json::Value::Bool(_) => "bool",
                        serde_json::Value::Number(_) => "number",
                        serde_json::Value::Array(_) => "array",
                        _ => "unknown",
                    }
                )));
            }
            None => serde_json::json!({}),
        };
        let request = ToolRequest::from_effective_arguments(target, arguments, schema_budget)
            .ok_or_else(|| {
                invalid(format!(
                    "tool call arguments for {:?} violate the argument invariants",
                    tool_call.function.name
                ))
            })?
            .with_provider_call_id(tool_call.id);
        requests.push(request);
    }
    // R05 RR1 F11: the batch-level identity admission — a same-id re-send
    // with identical shape collapses, a same-id CONFLICT rejects the whole
    // turn (zero requests admitted, zero side effects).
    let requests = super::batch_admission::admit_provider_call_ids(FAMILY, requests)?;
    // C11 stop-reason honesty (R05-T04) + R05 RR1 F12: transport end is not
    // protocol completion. Every argument/shape validation above already
    // ran (a half-JSON batch stays a loud InvalidMessage — nothing
    // dispatched), and the CLOSED turn classifies ONLY by a KNOWN normal
    // terminal reason: a `[DONE]` sentinel (or a buffered body) without a
    // finish_reason, or an unknown finish value, is a protocol surprise —
    // never a guessed Final, never a dispatched batch. A truncated turn
    // (length) never forms a Final and never dispatches its (parseable)
    // tool batch; the partial content lives on in the call's delta events
    // and the retried attempt re-runs the turn — this call produced zero
    // side effects, so the retry is safe.
    match choice.finish_reason.as_deref() {
        Some("stop") | Some("tool_calls") | Some("function_call") => {}
        Some("length") => {
            return Ok(ParsedChat::with_usage_report(
                ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::BudgetExceeded,
                        "provider stopped at the output length limit (finish_reason: length): \
                         the turn is truncated; the partial content stays in the call's delta \
                         events and nothing from this turn was dispatched"
                            .to_string(),
                        true,
                    ),
                    retryable: true,
                },
                usage_report.clone(),
            ));
        }
        Some("content_filter") => {
            return Ok(ParsedChat::with_usage_report(
                ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::Forbidden,
                        "provider refused the turn through its content filter (finish_reason: \
                         content_filter); not retryable as-is"
                            .to_string(),
                        false,
                    ),
                    retryable: false,
                },
                usage_report.clone(),
            ));
        }
        Some(other) => {
            return Err(invalid(format!(
                "provider response carries the UNKNOWN finish_reason {other:?}: an unmapped \
                 terminal is never guessed into a completed turn"
            )));
        }
        None => {
            return Err(invalid(
                "provider response ended WITHOUT a finish_reason: the transport's end (the \
                 [DONE] sentinel or the buffered body's last byte) is not protocol completion \
                 — the turn never classifies, nothing dispatches"
                    .to_string(),
            ));
        }
    }
    if !requests.is_empty() {
        return Ok(ParsedChat::with_usage_report(
            ProviderTurn::ToolRequests { requests, content },
            usage_report,
        ));
    }
    // R05 RR1 F12: a normally-stopped turn whose blocks carry NO answer
    // text (reasoning/opaque only) is process content, not a final answer —
    // it settles as an explicit process-only empty outcome; the driver
    // gives it the honest no-final terminal (nothing is committed as a
    // final message).
    let has_answer_text = content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { .. }));
    if has_answer_text {
        return Ok(ParsedChat::with_usage_report(
            ProviderTurn::Final {
                message: NormalizedMessage {
                    role: "assistant".to_string(),
                    content,
                    // Identity from the DRIVER, never from the response (C10).
                    model_call_id: Some(call.clone()),
                },
            },
            usage_report,
        ));
    }
    let detail = if content.is_empty() {
        format!(
            "provider returned no content and no tool calls (finish_reason: {})",
            choice.finish_reason.as_deref().unwrap_or("absent")
        )
    } else {
        format!(
            "provider turn completed with process-only content (reasoning/opaque state, no \
             answer text; finish_reason: {})",
            choice.finish_reason.as_deref().unwrap_or("absent")
        )
    };
    Ok(ParsedChat::with_usage_report(
        ProviderTurn::Empty {
            detail,
            // R05 RR1 F12: the process-state blocks ride along for replay.
            content,
        },
        usage_report,
    ))
}

// ── streaming decode (R05-T03 contract; R05-T04 incremental accumulator) ────

/// One in-flight tool call of the stream (fragments keyed by `index`; the
/// id/name appear on the first fragment of each call, argument fragments
/// concatenate behind the [`TOOL_ARGUMENTS_MAX_BYTES`] bound).
#[derive(Debug, Default)]
struct PendingToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// The incremental accumulator of one chat-completions SSE stream
/// (R05-T04): [`ChatStreamAccumulator::handle_event`] consumes each decoded
/// event AS IT ARRIVES and returns the live deltas to emit (text /
/// reasoning only — tool fragments are batch state, never live progress);
/// [`ChatStreamAccumulator::finish`] validates the CLOSED batch through the
/// same buffered parser ([`parse_chat_response`]). Content parts keep their
/// ARRIVAL order (adjacent same-kind fragments merge) so an interleaved
/// reasoning/text stream round-trips in order.
///
/// Conflict honesty (C10): a second DIFFERENT id or name for the same tool
/// `index`, two different `finish_reason` values, or two different `usage`
/// objects are loud protocol violations — identical re-sends are tolerated
/// (a re-send carries no new information; a conflict is never merged). Any
/// event after the `[DONE]` sentinel is loud.
pub struct ChatStreamAccumulator {
    /// Arrival-ordered content parts (adjacent same-kind merged).
    parts: Vec<ContentBlock>,
    tool_order: Vec<u64>,
    tools: std::collections::BTreeMap<u64, PendingToolCall>,
    finish_reason: Option<String>,
    usage: Option<serde_json::Value>,
    done_seen: bool,
}

impl Default for ChatStreamAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatStreamAccumulator {
    pub fn new() -> Self {
        Self {
            parts: Vec::new(),
            tool_order: Vec::new(),
            tools: std::collections::BTreeMap::new(),
            finish_reason: None,
            usage: None,
            done_seen: false,
        }
    }

    /// R05 RR1 F38: the usage fact the stream observed so far (the raw
    /// usage chunk decoded through the SAME strict family decoder) — the
    /// salvage a parse-failed turn still accounts with. Never a guess:
    /// absent stays `Unknown`, an invalid object stays `Invalid`.
    pub fn observed_usage_report(&self) -> lingxi_kernel::usage::ReportedUsage {
        super::usage::salvage_usage_report(
            lingxi_kernel::model_exchange::ProtocolFamily::OpenAiCompletions,
            self.usage.as_ref(),
        )
    }

    /// Consumes one decoded SSE event; returns the live deltas it produced
    /// (the caller emits them to the turn's sink in order). Every malformed
    /// or conflicting shape is a loud [`ErrorCode::InvalidMessage`].
    pub fn handle_event(&mut self, event: &SseEvent) -> Result<Vec<ModelTurnDelta>, ProtocolError> {
        let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
        let mut emitted = Vec::new();
        let data = event.data.trim();
        if self.done_seen {
            return Err(invalid(
                "stream event after the [DONE] sentinel: the terminal marker ends the stream"
                    .to_string(),
            ));
        }
        if data == "[DONE]" {
            self.done_seen = true;
            return Ok(emitted);
        }
        if data.is_empty() {
            // An empty data frame (e.g. `data:\n\n`) carries nothing.
            return Ok(emitted);
        }
        let chunk: serde_json::Value = serde_json::from_str(data)
            .map_err(|err| invalid(format!("stream chunk is not JSON: {err}")))?;
        if let Some(chunk_usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            match &self.usage {
                Some(existing) if existing != chunk_usage => {
                    return Err(invalid(
                        "stream carried two DIFFERENT usage objects: the usage chunk is \
                         authoritative exactly once (a conflict is never merged)"
                            .to_string(),
                    ));
                }
                Some(_) => {} // identical re-send: tolerated
                None => self.usage = Some(chunk_usage.clone()),
            }
        }
        let choices = chunk
            .get("choices")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        for choice in choices {
            let index = choice.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
            if index != 0 {
                return Err(invalid(format!(
                    "stream carries choice index {index}: this adapter never requests n>1, \
                     a multi-choice stream is a protocol surprise"
                )));
            }
            if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
                match &self.finish_reason {
                    Some(existing) if existing != reason => {
                        return Err(invalid(format!(
                            "stream carried two different finish_reason values ({existing:?} \
                             then {reason:?}): a conflict is never merged"
                        )));
                    }
                    _ => self.finish_reason = Some(reason.to_string()),
                }
            }
            let Some(delta) = choice.get("delta") else {
                continue;
            };
            if let Some(fragment) = delta.get("content").and_then(|c| c.as_str()) {
                if !fragment.is_empty() {
                    Self::push_part(&mut self.parts, true, fragment);
                    emitted.push(ModelTurnDelta::Text(fragment.to_string()));
                }
            }
            // The reasoning fragment vocabulary of the family's reasoning
            // deployments (deepseek-style `reasoning_content`).
            if let Some(fragment) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                if !fragment.is_empty() {
                    Self::push_part(&mut self.parts, false, fragment);
                    emitted.push(ModelTurnDelta::Reasoning(fragment.to_string()));
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
                for call_delta in calls {
                    let tool_index = call_delta
                        .get("index")
                        .and_then(|i| i.as_u64())
                        .ok_or_else(|| {
                            invalid("stream tool_call fragment without an index".to_string())
                        })?;
                    if !self.tools.contains_key(&tool_index) {
                        self.tool_order.push(tool_index);
                        self.tools.insert(tool_index, PendingToolCall::default());
                    }
                    let pending = self.tools.get_mut(&tool_index).expect("inserted above");
                    if let Some(id) = call_delta.get("id").and_then(|i| i.as_str()) {
                        if !id.is_empty() {
                            match &pending.id {
                                Some(existing) if existing != id => {
                                    return Err(invalid(format!(
                                        "stream tool call {tool_index} carried two DIFFERENT \
                                         ids ({existing:?} then {id:?}): a conflict is never \
                                         merged"
                                    )));
                                }
                                Some(_) => {} // identical re-send: tolerated
                                None => pending.id = Some(id.to_string()),
                            }
                        }
                    }
                    if let Some(function) = call_delta.get("function") {
                        if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                            if !name.is_empty() {
                                match &pending.name {
                                    Some(existing) if existing != name => {
                                        return Err(invalid(format!(
                                            "stream tool call {tool_index} carried two \
                                             DIFFERENT names ({existing:?} then {name:?}): a \
                                             conflict is never merged"
                                        )));
                                    }
                                    Some(_) => {} // identical re-send: tolerated
                                    None => pending.name = Some(name.to_string()),
                                }
                            }
                        }
                        if let Some(fragment) = function.get("arguments").and_then(|a| a.as_str()) {
                            if pending.arguments.len() + fragment.len() > TOOL_ARGUMENTS_MAX_BYTES {
                                return Err(invalid(format!(
                                    "stream tool call {tool_index} arguments exceeded the \
                                     {TOOL_ARGUMENTS_MAX_BYTES}-byte bound; refusing (a runaway \
                                     argument stream is a protocol failure, never an unbounded \
                                     buffer)"
                                )));
                            }
                            pending.arguments.push_str(fragment);
                        }
                    }
                }
            }
        }
        Ok(emitted)
    }

    /// Appends one content fragment in ARRIVAL order (adjacent same-kind
    /// fragments merge into one block; an interleaved reasoning/text stream
    /// keeps its order).
    fn push_part(parts: &mut Vec<ContentBlock>, text: bool, fragment: &str) {
        let mergeable = match parts.last_mut() {
            Some(ContentBlock::Text { text: body }) if text => Some(body),
            Some(ContentBlock::Reasoning { text: body }) if !text => Some(body),
            _ => None,
        };
        match mergeable {
            Some(body) => body.push_str(fragment),
            None if text => parts.push(ContentBlock::Text {
                text: fragment.to_string(),
            }),
            None => parts.push(ContentBlock::Reasoning {
                text: fragment.to_string(),
            }),
        }
    }

    /// Validates the CLOSED batch (after the `[DONE]` sentinel) through the
    /// SAME buffered parser — one contract, two wire modes. A stream that
    /// ended without `[DONE]` is truncated: loud, never half-parsed into a
    /// turn (C05/C09: the batch is validated as a whole before anything
    /// executes — the caller's zero-side-effect guarantee).
    pub fn finish(
        self,
        call: &ModelCallId,
        snapshot: &ToolDeclarationSnapshot,
        schema_budget: &SchemaBudget,
    ) -> Result<ParsedChat, ProtocolError> {
        let invalid = |detail: String| ProtocolError::new(ErrorCode::InvalidMessage, detail, false);
        if !self.done_seen {
            return Err(invalid(
                "stream ended without the [DONE] sentinel: a truncated stream is never \
                 half-parsed into a turn"
                    .to_string(),
            ));
        }
        // Rebuild the buffered response shape and reuse the SAME parser.
        let mut message = serde_json::json!({});
        if !self.parts.is_empty() {
            let parts: Vec<serde_json::Value> = self
                .parts
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => {
                        serde_json::json!({"type": "text", "text": text})
                    }
                    ContentBlock::Reasoning { text } => {
                        serde_json::json!({"type": "reasoning", "text": text})
                    }
                    other => serde_json::json!({"type": "opaque", "data": other}),
                })
                .collect();
            message["content"] = serde_json::Value::Array(parts);
        }
        if !self.tool_order.is_empty() {
            let mut rendered = Vec::new();
            for index in &self.tool_order {
                let pending = self.tools.get(index).expect("ordered above");
                let name = pending.name.clone().ok_or_else(|| {
                    invalid("stream tool call never carried a function name".to_string())
                })?;
                let id = pending
                    .id
                    .clone()
                    .ok_or_else(|| invalid("stream tool call never carried an id".to_string()))?;
                rendered.push(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": pending.arguments,
                    }
                }));
            }
            message["tool_calls"] = serde_json::Value::Array(rendered);
        }
        let mut body = serde_json::json!({
            "choices": [{
                "index": 0,
                "finish_reason": self.finish_reason,
                "message": message,
            }]
        });
        if let Some(usage) = self.usage {
            body["usage"] = usage;
        }
        parse_chat_response(call, snapshot, &body, schema_budget)
    }
}

impl super::dispatch::StreamAccumulator for ChatStreamAccumulator {
    fn handle_event(&mut self, event: &SseEvent) -> Result<Vec<ModelTurnDelta>, ProtocolError> {
        ChatStreamAccumulator::handle_event(self, event)
    }
}

/// Aggregates one complete SSE chunk stream (the offline/golden entry over
/// already-decoded events) through the SAME incremental accumulator the
/// production read drives — one decode contract, two delivery modes.
pub fn parse_chat_stream(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    events: &[SseEvent],
    schema_budget: &SchemaBudget,
) -> Result<ParsedChat, ProtocolError> {
    let mut accumulator = ChatStreamAccumulator::new();
    for event in events {
        accumulator.handle_event(event)?;
    }
    accumulator.finish(call, snapshot, schema_budget)
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

    fn route() -> ResolvedModelRoute {
        ResolvedModelRoute {
            provider: "main".to_string(),
            model: "gpt-test".to_string(),
            operation: ModelOperation::Chat,
            protocol: ProtocolFamily::OpenAiCompletions,
            endpoint: "https://api.example.test/v1".to_string(),
            credential: CredentialReference {
                provider: "main".to_string(),
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
    fn first_turn_request_carries_the_route_model_and_declarations() {
        let input = ModelTurnInput::first_turn("hello there", snapshot_with_read());
        let body = render_chat_request(&input, &route(), false).expect("renders");
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["stream"], false);
        assert!(body.get("stream_options").is_none());
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hello there");
        assert_eq!(body["tools"][0]["function"]["name"], "read");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn system_prompt_renders_as_the_first_system_message() {
        let mut input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
        input.system_prompt = Some("you are precise".to_string());
        let body = render_chat_request(&input, &route(), false).expect("renders");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "you are precise");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn streaming_request_flips_the_wire_mode() {
        let input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
        let body = render_chat_request(&input, &route(), true).expect("renders");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn tool_call_exchange_renders_the_protocol_pairing() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:read",
            serde_json::json!({"path": "/tmp/a"}),
            &budget,
        )
        .expect("effective")
        .with_provider_call_id("call_abc");
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
            // The completions input shape renders no family-opaque state, so
            // no origin binding is exercised here.
            origin: None,
        });
        input.prior.push(ExchangeItem::ToolResult {
            tool_call_id: ToolCallId::new("run-tc0001"),
            provider_call_id: Some("call_abc".to_string()),
            outcome: ToolOutcome::success_text("file body"),
        });
        let body = render_chat_request(&input, &route(), false).expect("renders");
        let assistant = &body["messages"][1];
        assert_eq!(assistant["role"], "assistant");
        assert_eq!(assistant["content"], "reading");
        assert_eq!(assistant["tool_calls"][0]["id"], "call_abc");
        assert_eq!(assistant["tool_calls"][0]["function"]["name"], "read");
        assert_eq!(
            assistant["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"/tmp/a\"}"
        );
        let tool = &body["messages"][2];
        assert_eq!(tool["role"], "tool");
        assert_eq!(tool["tool_call_id"], "call_abc");
        assert_eq!(tool["content"], "file body");
    }

    #[test]
    fn a_history_target_missing_from_the_snapshot_fails_loudly() {
        let budget = SchemaBudget::default();
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:gone",
            serde_json::json!({}),
            &budget,
        )
        .expect("effective")
        .with_provider_call_id("call_x");
        let mut input = ModelTurnInput::first_turn("hi", snapshot_with_read());
        input.prior.push(ExchangeItem::AssistantTurn {
            call: ModelCallId::new("mc-1"),
            content: Vec::new(),
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
        let err = render_chat_request(&input, &route(), false).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidMessage);
        assert!(err.message.contains("declaration snapshot"));
    }

    #[test]
    fn parses_tool_calls_with_usage_and_identity_from_the_driver() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0007");
        let body = serde_json::json!({
            "id": "chatcmpl-ignored",
            "model": "the-provider-claims-this",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "read", "arguments": "{\"path\": \"/tmp/a\"}"}
                    }]
                }
            }],
            "usage": {"prompt_tokens": 11, "completion_tokens": 7}
        });
        let parsed = parse_chat_response(&call, &snapshot, &body, &budget).expect("parses");
        assert_eq!(
            parsed.usage(),
            Some(UsageRecord {
                input_tokens: 11,
                output_tokens: 7
            })
        );
        match parsed.turn {
            ProviderTurn::ToolRequests { requests, content } => {
                assert!(content.is_empty());
                assert_eq!(requests.len(), 1);
                assert_eq!(requests[0].target, "tool:first-party:read");
                assert_eq!(requests[0].provider_call_id.as_deref(), Some("call_1"));
                assert!(requests[0].digest_matches_arguments());
            }
            other => panic!("expected ToolRequests, got {other:?}"),
        }
    }

    #[test]
    fn unknown_wire_names_are_protocol_violations_never_guesses() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0001");
        let body = serde_json::json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "invented_tool", "arguments": "{}"}
                    }]
                }
            }]
        });
        let err = parse_chat_response(&call, &snapshot, &body, &budget).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidMessage);
        assert!(err.message.contains("unknown tool"));
    }

    #[test]
    fn final_answer_identity_comes_from_the_driver_not_the_response() {
        let snapshot = ToolDeclarationSnapshot::empty();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0042");
        let body = serde_json::json!({
            "id": "chatcmpl-ignored",
            "model": "totally-different-model",
            "choices": [{
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "the answer"}
            }],
            "usage": {"prompt_tokens": 5}
        });
        let parsed = parse_chat_response(&call, &snapshot, &body, &budget).expect("parses");
        // Half-reported usage is no usage.
        assert_eq!(parsed.usage(), None);
        match parsed.turn {
            ProviderTurn::Final { message } => {
                assert_eq!(message.role, "assistant");
                assert_eq!(message.model_call_id, Some(call));
                assert_eq!(
                    message.content,
                    vec![ContentBlock::Text {
                        text: "the answer".to_string()
                    }]
                );
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn zero_choices_and_empty_content_are_loud_or_explicit() {
        let snapshot = ToolDeclarationSnapshot::empty();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0001");
        let err = parse_chat_response(
            &call,
            &snapshot,
            &serde_json::json!({"choices": []}),
            &budget,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidMessage);
        // C11 (R05-T04): a length-truncated turn is a retryable budget
        // failure, never a silent Empty (the T03 classification was Empty —
        // the T04 stop-reason table supersedes it; the partial content lives
        // in the call's delta events, nothing of the turn was dispatched).
        let parsed = parse_chat_response(
            &call,
            &snapshot,
            &serde_json::json!({"choices": [{"finish_reason": "length", "message": {"content": null}}]}),
            &budget,
        )
        .expect("parses");
        match parsed.turn {
            ProviderTurn::Failed { error, retryable } => {
                assert_eq!(error.code, ErrorCode::BudgetExceeded);
                assert!(retryable);
                assert!(error.message.contains("length"));
            }
            other => panic!("expected Failed(length), got {other:?}"),
        }
        let parsed = parse_chat_response(
            &call,
            &snapshot,
            &serde_json::json!({"choices": [{"finish_reason": "content_filter", "message": {"content": null}}]}),
            &budget,
        )
        .expect("parses");
        match parsed.turn {
            ProviderTurn::Failed { error, retryable } => {
                assert_eq!(error.code, ErrorCode::Forbidden);
                assert!(!retryable);
            }
            other => panic!("expected Failed(content_filter), got {other:?}"),
        }
    }

    #[test]
    fn mixed_content_parts_keep_every_block_in_order() {
        let snapshot = ToolDeclarationSnapshot::empty();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0002");
        let body = serde_json::json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {"content": [
                    {"type": "reasoning", "text": "thinking first"},
                    {"type": "text", "text": "answer one"},
                    {"type": "unknown-future-part", "x": 1},
                    {"type": "text", "text": "answer two"}
                ]}
            }]
        });
        let parsed = parse_chat_response(&call, &snapshot, &body, &budget).expect("parses");
        match parsed.turn {
            ProviderTurn::Final { message } => {
                assert_eq!(message.content.len(), 4);
                assert!(
                    matches!(&message.content[0], ContentBlock::Reasoning { text } if text == "thinking first")
                );
                assert!(
                    matches!(&message.content[1], ContentBlock::Text { text } if text == "answer one")
                );
                assert!(
                    matches!(&message.content[2], ContentBlock::Opaque { provider, data }
                        if provider == "openai-completions" && data["type"] == "unknown-future-part")
                );
                assert!(
                    matches!(&message.content[3], ContentBlock::Text { text } if text == "answer two")
                );
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn tool_outcome_rendering_is_honest_for_all_four_states() {
        assert_eq!(
            render_tool_outcome_text(&ToolOutcome::success_text("ok body")),
            "ok body"
        );
        let failed = render_tool_outcome_text(&ToolOutcome::Failed {
            error: ProtocolError::new(ErrorCode::Forbidden, "denied by policy", false),
        });
        assert!(failed.contains("forbidden") && failed.contains("denied by policy"));
        assert!(render_tool_outcome_text(&ToolOutcome::Cancelled).contains("cancelled"));
        let unknown = render_tool_outcome_text(&ToolOutcome::Unknown {
            reason: "receipt lost".to_string(),
        });
        assert!(unknown.contains("receipt lost"));
    }

    // ── streaming decode ────────────────────────────────────────────────────

    fn sse_events(frames: &[&str]) -> Vec<super::super::streaming::SseEvent> {
        let body = frames
            .iter()
            .map(|frame| format!("data: {frame}\n\n"))
            .collect::<String>();
        super::super::streaming::decode_complete_body(&body).expect("frames parse")
    }

    #[test]
    fn stream_aggregates_text_and_usage_and_requires_done() {
        let snapshot = ToolDeclarationSnapshot::empty();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0009");
        let events = sse_events(&[
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":"lo"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#,
            "[DONE]",
        ]);
        let parsed = parse_chat_stream(&call, &snapshot, &events, &budget).expect("parses");
        assert_eq!(
            parsed.usage(),
            Some(UsageRecord {
                input_tokens: 3,
                output_tokens: 2
            })
        );
        match parsed.turn {
            ProviderTurn::Final { message } => {
                assert_eq!(
                    message.content,
                    vec![ContentBlock::Text {
                        text: "Hello".to_string()
                    }]
                );
            }
            other => panic!("expected Final, got {other:?}"),
        }
        // Without the sentinel the stream is truncated — loud, never parsed.
        let truncated = sse_events(&[r#"{"choices":[{"index":0,"delta":{"content":"Hel"}}]}"#]);
        let err = parse_chat_stream(&call, &snapshot, &truncated, &budget).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidMessage);
        assert!(err.message.contains("[DONE]"));
    }

    #[test]
    fn stream_aggregates_tool_call_fragments_and_reasoning_in_order() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0010");
        let events = sse_events(&[
            r#"{"choices":[{"index":0,"delta":{"reasoning_content":"need the file"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":"reading now"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_s1","type":"function","function":{"name":"read","arguments":"{\"pa"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"/tmp/a\"}"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        let parsed = parse_chat_stream(&call, &snapshot, &events, &budget).expect("parses");
        match parsed.turn {
            ProviderTurn::ToolRequests { requests, content } => {
                assert_eq!(requests.len(), 1);
                assert_eq!(requests[0].provider_call_id.as_deref(), Some("call_s1"));
                assert_eq!(requests[0].target, "tool:first-party:read");
                // Reasoning came before text and BOTH survived (C06).
                assert_eq!(content.len(), 2);
                assert!(
                    matches!(&content[0], ContentBlock::Reasoning { text } if text == "need the file")
                );
                assert!(
                    matches!(&content[1], ContentBlock::Text { text } if text == "reading now")
                );
            }
            other => panic!("expected ToolRequests, got {other:?}"),
        }
    }

    #[test]
    fn stream_rejects_multi_choice_and_missing_ids_loudly() {
        let snapshot = snapshot_with_read();
        let budget = SchemaBudget::default();
        let call = ModelCallId::new("run-mc0011");
        let multi = sse_events(&[
            r#"{"choices":[{"index":1,"delta":{"content":"surprise"}}]}"#,
            "[DONE]",
        ]);
        let err = parse_chat_stream(&call, &snapshot, &multi, &budget).unwrap_err();
        assert!(err.message.contains("n>1"));
        let idless = sse_events(&[
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"read","arguments":"{}"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        let err = parse_chat_stream(&call, &snapshot, &idless, &budget).unwrap_err();
        assert!(err.message.contains("never carried an id"));
    }
}
