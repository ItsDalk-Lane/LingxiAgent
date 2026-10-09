//! The auxiliary-slot executor (R05-T06 §29.8): the incumbent `callText`
//! surface — title/summarize/memory/vision/approval/guard slots — driven
//! through the REAL five chat protocol families (never a parallel
//! implementation). One call = one single-turn exchange (no tools, no
//! prior, the host's prompt as the submission, optional host-authorized
//! images and output cap), dispatched by [`GatewayedProvider`] through
//! the slot's OWN route binding with the shared gateway/credential/
//! 401-refresh/compat discipline.
//!
//! Honesty rules:
//! - an unconfigured slot is a loud failure at route resolution (no
//!   fallback onto the chat route — the fail-closed approval/guard
//!   posture holds by construction);
//! - a `ToolRequests` turn never executes anything: on the single-turn
//!   text contract ([`AuxiliaryExecutor::complete`]) it fails loudly with
//!   a message that names whether the request declared tools; slots with
//!   an incumbent recovery semantic (R06-T02 compaction: ONE first-turn
//!   single-call intent is answered with a placeholder result) consume
//!   [`AuxiliaryExecutor::complete_step`] and judge the intent themselves;
//! - an empty/reasoning-only answer is an explicit failure, never a
//!   fabricated text;
//! - live deltas have no subscriber in an auxiliary call (the caller
//!   awaits the settled turn), so the sink discards them — the streamed
//!   wire mode stays the production one.

use std::sync::Arc;

use lingxi_kernel::model_exchange::{
    AuxiliarySlot, InputImage, ModelOperation, ModelTurnInput, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{
    ModelTurnDelta, ProviderDescriptor, ProviderTurn, TurnDeltaSink, TurnDeltaSinkClosed,
    TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, ProtocolError, UsageRecord};

use super::credentials::ProviderCredentialPort;
use super::gateway::ConfigModelGateway;
use super::provider::GatewayedProvider;

/// One auxiliary call's input (host-decided; the worker/caller payload is
/// never an authority on routing or identity).
#[derive(Debug, Clone)]
pub struct AuxiliaryRequest {
    pub prompt: String,
    /// Host-authorized image inputs (vision slot) — read + bounded
    /// host-side before they reach here.
    pub images: Vec<InputImage>,
    /// Host-decided output-token cap (None = the family default).
    pub max_output_tokens: Option<u32>,
    /// Host-computed absolute deadline clamping the call's network budget.
    pub deadline_unix_ms: Option<u64>,
    /// R06-T02: 压缩请求的会话系统提示（现役缓存保留形状的第一项
    /// `[system, submission, ...exchange, instruction]`）。None = 无系统
    /// 槽（其他 auxiliary 槽的现役行为不变）。
    pub system_prompt: Option<String>,
    /// R06-T02: 压缩请求的活历史（缓存保留形状的中间段），末尾一项是
    /// 请求作用域的 `CompactionInstruction`。空 = 无历史（其他槽不变）。
    pub prior: Vec<lingxi_kernel::model_exchange::ExchangeItem>,
    /// R06-T02: 压缩请求携带的 live 工具声明快照——历史里 assistant
    /// 工具调用的 wire 名解析必需（空快照会让每个带 tool_calls 的历史
    /// turn 渲染失败）；模型回调工具时本执行器绝不执行任何调用——
    /// `complete()` 响亮失败，`complete_step()` 把意图交回宿主（压缩槽
    /// 的现役 placeholder 单次恢复由 service 层实现）。None = 空快照
    /// （其他槽不变）。
    pub tools: Option<ToolDeclarationSnapshot>,
}

/// A settled auxiliary answer plus its correlation facts (the served-by
/// identity is the RESOLVED route, never a claimed one). R05-T07: the
/// richer usage fact (provenance + components + invalid-vs-unknown), the
/// protocol family and the physical request count ride along for the
/// usage ledger.
#[derive(Debug, Clone)]
pub struct AuxiliaryOutcome {
    pub text: String,
    pub usage: Option<UsageRecord>,
    pub usage_report: lingxi_kernel::usage::ReportedUsage,
    pub served_protocol: Option<String>,
    pub transport_attempts: u32,
    pub served_by: ProviderDescriptor,
}

/// R06-T02: one settled auxiliary turn with the tool-intent half exposed.
/// [`AuxiliaryExecutor::complete`] keeps the single-turn text contract
/// (a tool-requesting turn is a loud failure); the compaction slot consumes
/// [`AuxiliaryExecutor::complete_step`] instead — the incumbent
/// cache-preserving compaction run answers ONE first-turn single-call tool
/// intent with a placeholder result and continues (`tool_recovery` phase),
/// so the requests must reach the host instead of failing in place.
#[derive(Debug, Clone)]
pub enum AuxiliaryStep {
    /// The provider answered a final text message.
    Text(AuxiliaryOutcome),
    /// The provider requested tool calls (never executed — an auxiliary
    /// call has no tool plane). Carries the attempt's settle facts: a
    /// tool-intent turn is a settled provider answer and accounts exactly
    /// like a text turn (R05 RR1 F21).
    ToolRequests {
        requests: Vec<lingxi_kernel::ports::ToolRequest>,
        /// The turn's own content blocks (text/reasoning/opaque) — the
        /// recovery request replays them as the assistant half of the
        /// placeholder exchange.
        content: Vec<ContentBlock>,
        usage_report: lingxi_kernel::usage::ReportedUsage,
        served_protocol: Option<String>,
        transport_attempts: u32,
        served_by: ProviderDescriptor,
    },
}

/// A loud auxiliary failure. The message is already scrubbed of any
/// in-play credential material (the provider layer scrubs before
/// classifying).
///
/// R05 RR1 F21: the failure carries the SETTLE FACTS of the attempt — a
/// failed call still accounts. `usage_report`/`transport_attempts`/
/// `served_by`/`served_protocol` are exactly what the equivalent success
/// would have carried: a physically-sent 500 holds `transport_attempts =
/// 1` and an unknown usage (never vanished); a pre-send refusal (route /
/// capability / credential resolution) holds `transport_attempts = 0`
/// (not-sent, never a fabricated 1); a provider that answered garbage
/// WITH a usage object keeps that usage fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxiliaryFailure {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    /// The usage fact of the attempt (unknown when nothing usable
    /// arrived — never zero).
    pub usage_report: lingxi_kernel::usage::ReportedUsage,
    /// Physical provider requests of the attempt (0 = refused before
    /// anything left the process).
    pub transport_attempts: u32,
    /// The RESOLVED route identity the attempt was on (the same fallback
    /// a success carries).
    pub served_by: ProviderDescriptor,
    pub served_protocol: Option<String>,
}

impl std::fmt::Display for AuxiliaryFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AuxiliaryFailure {}

/// The auxiliary sink: streamed deltas have no live subscriber (the
/// caller awaits the settled turn). Never errors — a closed-run signal
/// does not exist on this path.
struct NullSink;

impl TurnDeltaSink for NullSink {
    fn emit<'a>(
        &'a self,
        _delta: ModelTurnDelta,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}

/// The executor: one [`GatewayedProvider`] (the full five-family
/// dispatch) behind a slot-keyed entry.
pub struct AuxiliaryExecutor {
    provider: GatewayedProvider,
}

impl AuxiliaryExecutor {
    pub fn new(
        gateway: Arc<ConfigModelGateway>,
        credentials: Arc<dyn ProviderCredentialPort>,
        schema_budget: lingxi_kernel::toolcatalog::SchemaBudget,
    ) -> Result<Self, ProtocolError> {
        Ok(Self {
            provider: GatewayedProvider::new(gateway, credentials, schema_budget)?,
        })
    }

    /// R05 RR1 F14: the production constructor — the auxiliary slots'
    /// provider is BOUND to the same shared, reloadable network policy as
    /// the chat loop (one plane, one policy generation for every model
    /// consumer).
    pub fn new_with_network(
        gateway: Arc<ConfigModelGateway>,
        credentials: Arc<dyn ProviderCredentialPort>,
        schema_budget: lingxi_kernel::toolcatalog::SchemaBudget,
        network: std::sync::Arc<super::network::NetworkPlane>,
    ) -> Result<Self, ProtocolError> {
        Ok(Self {
            provider: GatewayedProvider::new_with_network(
                gateway,
                credentials,
                schema_budget,
                network,
            )?,
        })
    }

    /// Runs ONE auxiliary call to settlement. `call` is the host-minted
    /// correlation id of this invocation (worker callbacks pass
    /// `aux-{slot}-{invocation}-{cb_id}`-shaped identities so a trace can
    /// join parent and child — C04).
    ///
    /// Single-turn text contract: a tool-requesting turn is a loud failure
    /// here (the message names whether the request declared tools — the
    /// compaction slot declares the live snapshot for wire-name resolution;
    /// every other slot declares none). Slots with an incumbent tool-intent
    /// recovery semantic (compaction) use [`Self::complete_step`] instead.
    // R05 RR1 F21: the failure carries the attempt's settle facts (usage
    // report, attempts, resolved identity) — one construction per settled
    // call, never in a loop; boxing it would ripple through every caller
    // for no functional gain.
    #[allow(clippy::result_large_err)]
    pub async fn complete(
        &self,
        ctx: &RunContext,
        slot: AuxiliarySlot,
        call: &ModelCallId,
        request: &AuxiliaryRequest,
    ) -> Result<AuxiliaryOutcome, AuxiliaryFailure> {
        let declared_tools = request
            .tools
            .as_ref()
            .is_some_and(|snapshot| !snapshot.declarations.is_empty());
        match self.complete_step(ctx, slot, call, request).await {
            Ok(AuxiliaryStep::Text(outcome)) => Ok(outcome),
            Ok(AuxiliaryStep::ToolRequests {
                usage_report,
                transport_attempts,
                served_by,
                served_protocol,
                ..
            }) => {
                let message = if declared_tools {
                    format!(
                        "auxiliary slot {} declared tools for history wire-name resolution \
                         ONLY (an auxiliary call never executes tools) but the provider \
                         answered with tool requests; the calls were NOT executed",
                        slot.config_key()
                    )
                } else {
                    format!(
                        "auxiliary slot {} declared NO tools but the provider answered with \
                         tool requests; the calls were NOT executed (a tool-requesting \
                         auxiliary turn is a protocol anomaly, never silently served)",
                        slot.config_key()
                    )
                };
                Err(AuxiliaryFailure {
                    code: ErrorCode::InvalidMessage,
                    message,
                    retryable: false,
                    usage_report,
                    transport_attempts,
                    served_by,
                    served_protocol,
                })
            }
            Err(failure) => Err(failure),
        }
    }

    /// Runs ONE auxiliary call to settlement and reports the settled turn
    /// shape ([`AuxiliaryStep`]): text, or the provider's tool requests with
    /// the attempt's full settle facts. Every failure path is identical to
    /// [`Self::complete`].
    #[allow(clippy::result_large_err)]
    pub async fn complete_step(
        &self,
        ctx: &RunContext,
        slot: AuxiliarySlot,
        call: &ModelCallId,
        request: &AuxiliaryRequest,
    ) -> Result<AuxiliaryStep, AuxiliaryFailure> {
        self.complete_step_for_operation(
            ctx,
            ModelOperation::Auxiliary(slot),
            &format!("auxiliary slot {}", slot.config_key()),
            call,
            request,
        )
        .await
    }

    /// R06-T02（R3-F-01，§十#17）：`complete_step` 的操作显式形——调用方
    /// 已自行完成生效路由决策（压缩服务的 summarize 槽→chat 回退链）并直接
    /// 点名 operation；`label` 是失败消息里的诊断名。执行器自身**仍然**
    /// 永不回退：operation 照常在 gateway 解析自己的绑定（R05 C07 纪律
    /// 不变），回退决策只发生在调用方（显式、可声明、可测试）。
    #[allow(clippy::result_large_err)]
    pub async fn complete_step_for_operation(
        &self,
        ctx: &RunContext,
        operation: ModelOperation,
        label: &str,
        call: &ModelCallId,
        request: &AuxiliaryRequest,
    ) -> Result<AuxiliaryStep, AuxiliaryFailure> {
        let input = ModelTurnInput {
            submission: request.prompt.clone(),
            // R06-T02: the compaction slot passes the session system prompt
            // through (the cache-preserving shape's first segment); every
            // other slot keeps the incumbent None.
            system_prompt: request.system_prompt.clone(),
            turn: 1,
            // R06-T02: the compaction slot's live history rides here.
            prior: request.prior.clone(),
            // R06-T02: the compaction slot's live snapshot (wire-name
            // resolution for history tool calls); None keeps the incumbent
            // empty snapshot.
            tools: request
                .tools
                .clone()
                .unwrap_or_else(ToolDeclarationSnapshot::empty),
            deadline_unix_ms: request.deadline_unix_ms,
            images: request.images.clone(),
            max_output_tokens: request.max_output_tokens,
        };
        let result = self
            .provider
            .next_turn_for_operation(ctx, call, operation, &input, &NullSink)
            .await;
        let served_by = result
            .served_by
            .clone()
            .unwrap_or_else(|| self.provider.descriptor());
        // R05 RR1 F21: EVERY exit path carries the attempt's settle facts —
        // the failure is a classified ANSWER about an attempt that really
        // happened, never a discard of its accounting.
        let fail = |code: ErrorCode, message: String, retryable: bool| AuxiliaryFailure {
            code,
            message,
            retryable,
            usage_report: result.usage_report.clone(),
            transport_attempts: result.transport_attempts,
            served_by: served_by.clone(),
            served_protocol: result.served_protocol.clone(),
        };
        match result.turn {
            ProviderTurn::Final { message } => {
                let text = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
                if text.is_empty() {
                    return Err(fail(
                        ErrorCode::UpstreamUnavailable,
                        format!(
                            "{label} settled with a final turn carrying no text \
                             (an empty answer is an explicit failure, never a fabricated one)"
                        ),
                        true,
                    ));
                }
                Ok(AuxiliaryStep::Text(AuxiliaryOutcome {
                    text,
                    usage: result.usage,
                    usage_report: result.usage_report,
                    served_protocol: result.served_protocol,
                    transport_attempts: result.transport_attempts,
                    served_by,
                }))
            }
            // R06-T02: the tool-intent half is REPORTED, not judged here —
            // `complete()` fails it loudly (single-turn slots), while the
            // compaction slot answers ONE first-turn single-call intent with
            // a placeholder result (the incumbent `tool_recovery` phase).
            // The calls are NEVER executed either way.
            ProviderTurn::ToolRequests { requests, content } => Ok(AuxiliaryStep::ToolRequests {
                requests,
                content,
                usage_report: result.usage_report,
                served_protocol: result.served_protocol,
                transport_attempts: result.transport_attempts,
                served_by,
            }),
            ProviderTurn::Continue { process_note } => Err(fail(
                ErrorCode::UpstreamUnavailable,
                format!(
                    "{label} produced a process-only turn ({process_note}); a \
                     reasoning-only auxiliary answer carries no usable text"
                ),
                true,
            )),
            ProviderTurn::Empty { detail, .. } => Err(fail(
                ErrorCode::UpstreamUnavailable,
                format!("{label} returned an empty turn: {detail}"),
                true,
            )),
            ProviderTurn::Failed { error, retryable } => Err(fail(
                error.code,
                format!("{label} failed: {}", error.message),
                retryable,
            )),
        }
    }
}
