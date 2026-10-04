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
//! - a `ToolRequests` turn is a protocol anomaly here (the call declared
//!   NO tools): it fails loudly, the calls are never executed;
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

/// A loud auxiliary failure. The message is already scrubbed of any
/// in-play credential material (the provider layer scrubs before
/// classifying).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxiliaryFailure {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
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

    /// Runs ONE auxiliary call to settlement. `call` is the host-minted
    /// correlation id of this invocation (worker callbacks pass
    /// `aux-{slot}-{invocation}-{cb_id}`-shaped identities so a trace can
    /// join parent and child — C04).
    pub async fn complete(
        &self,
        ctx: &RunContext,
        slot: AuxiliarySlot,
        call: &ModelCallId,
        request: &AuxiliaryRequest,
    ) -> Result<AuxiliaryOutcome, AuxiliaryFailure> {
        let input = ModelTurnInput {
            submission: request.prompt.clone(),
            system_prompt: None,
            turn: 1,
            prior: Vec::new(),
            tools: ToolDeclarationSnapshot::empty(),
            deadline_unix_ms: request.deadline_unix_ms,
            images: request.images.clone(),
            max_output_tokens: request.max_output_tokens,
        };
        let result = self
            .provider
            .next_turn_for_operation(
                ctx,
                call,
                ModelOperation::Auxiliary(slot),
                &input,
                &NullSink,
            )
            .await;
        let served_by = result
            .served_by
            .unwrap_or_else(|| self.provider.descriptor());
        let fail = |code: ErrorCode, message: String, retryable: bool| AuxiliaryFailure {
            code,
            message,
            retryable,
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
                            "auxiliary slot {} settled with a final turn carrying no text \
                             (an empty answer is an explicit failure, never a fabricated one)",
                            slot.config_key()
                        ),
                        true,
                    ));
                }
                Ok(AuxiliaryOutcome {
                    text,
                    usage: result.usage,
                    usage_report: result.usage_report,
                    served_protocol: result.served_protocol,
                    transport_attempts: result.transport_attempts,
                    served_by,
                })
            }
            ProviderTurn::ToolRequests { .. } => Err(fail(
                ErrorCode::InvalidMessage,
                format!(
                    "auxiliary slot {} declared NO tools but the provider answered with tool \
                     requests; the calls were NOT executed (a tool-requesting auxiliary turn \
                     is a protocol anomaly, never silently served)",
                    slot.config_key()
                ),
                false,
            )),
            ProviderTurn::Continue { process_note } => Err(fail(
                ErrorCode::UpstreamUnavailable,
                format!(
                    "auxiliary slot {} produced a process-only turn ({process_note}); a \
                     reasoning-only auxiliary answer carries no usable text",
                    slot.config_key()
                ),
                true,
            )),
            ProviderTurn::Empty { detail } => Err(fail(
                ErrorCode::UpstreamUnavailable,
                format!(
                    "auxiliary slot {} returned an empty turn: {detail}",
                    slot.config_key()
                ),
                true,
            )),
            ProviderTurn::Failed { error, retryable } => Err(fail(
                error.code,
                format!(
                    "auxiliary slot {} failed: {}",
                    slot.config_key(),
                    error.message
                ),
                retryable,
            )),
        }
    }
}
