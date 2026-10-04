//! The model plane (R05-T01/T02/T03): the config-backed
//! [`ModelGatewayPort`] (unified model selection + credential-reference
//! resolution), the five real chat protocol adapters (openai-completions,
//! anthropic-messages, google-generative-ai, openai-responses,
//! openai-codex-responses), the shared SSE decoder and tool-outcome
//! rendering, the OAuth flow machines, and the gateway-backed production
//! [`TurnProviderPort`]. R05-T06 adds [`operations`] — the media &
//! auxiliary operation plane (embedding / rerank / image / video / speech
//! / transcription dialects over the shared dispatch discipline).
//!
//! Trust split (C01): kernel types carry
//! [`lingxi_kernel::model_exchange::CredentialReference`] (identity + kind);
//! credential MATERIAL leaves this crate only through
//! [`credentials::ProviderCredentialPort`] (the production implementation is
//! the service-side `CredentialService`) — the gateway holds the validated
//! config seed for kind mapping and exposes NO material accessor. The
//! resolved route hands the adapter everything it needs to dispatch ONE
//! call — provider/model as one identity unit, protocol family, endpoint,
//! credential reference and the config generation the resolution was taken
//! at.

pub mod anthropic_messages;
pub mod auxiliary;
pub mod compat;
pub mod config;
pub mod credentials;
pub mod dispatch;
pub mod egress;
pub mod gateway;
pub mod google_generative_ai;
pub mod oauth;
pub mod openai_codex_responses;
pub mod openai_completions;
pub mod openai_responses;
pub mod operations;
pub mod provider;
pub mod streaming;
pub mod tool_render;
pub mod usage;

use base64::Engine as _;
use lingxi_kernel::ports::ProviderTurn;
use lingxi_kernel::usage::ReportedUsage;
use lingxi_protocol::UsageRecord;

/// The parsed half of a successful response, shared by every family
/// adapter: the turn plus the usage fact. [`ParsedChat::usage`] derives
/// the frozen wire projection (`Some` only when BOTH totals are known);
/// `usage_report` (R05-T07) is the richer ledger fact — component tokens,
/// provenance, and the invalid-vs-unknown distinction (a usage object
/// whose numbers violate the contract marks the fact invalid; the turn
/// itself still settles).
#[derive(Debug)]
pub struct ParsedChat {
    pub turn: ProviderTurn,
    pub usage_report: ReportedUsage,
}

impl ParsedChat {
    /// Builds the parsed turn from a decode outcome.
    pub fn with_usage_report(turn: ProviderTurn, report: ReportedUsage) -> Self {
        Self {
            turn,
            usage_report: report,
        }
    }

    /// The frozen wire projection of the usage fact: `Some` only when
    /// BOTH totals are known (a half-known or invalid usage projects
    /// `None` — never a number that could be read as the request's real
    /// usage).
    pub fn usage(&self) -> Option<UsageRecord> {
        match &self.usage_report {
            ReportedUsage::Known(usage) => usage.wire_record(),
            _ => None,
        }
    }
}

/// base64 body of one host-authorized image input (R05-T06). The bytes
/// were read and bounded host-side; the kernel type carries no encoder
/// (kernel stays dependency-free), so the adapters own the wire encoding.
pub(crate) fn image_base64(image: &lingxi_kernel::model_exchange::InputImage) -> String {
    base64::engine::general_purpose::STANDARD.encode(&image.bytes)
}

/// The `data:` URL form of one image input (openai `image_url`, responses
/// `input_image`, media-adapter reference images).
pub(crate) fn image_data_url(image: &lingxi_kernel::model_exchange::InputImage) -> String {
    format!("data:{};base64,{}", image.mime, image_base64(image))
}
