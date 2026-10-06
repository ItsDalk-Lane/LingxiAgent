//! The `providers` / `models` configuration sections (R05-T01).
//!
//! The service's `--config` file embeds these sections verbatim; this module
//! owns their shape, their parsing and their validation. Every rule is
//! loud: unknown keys are refused (`deny_unknown_fields`), an unknown
//! protocol family / malformed endpoint / missing credential is a
//! [`ModelConfigError`] at LOAD time — a misconfigured model plane never
//! starts up silently degraded.
//!
//! Credential material (`apiKey`) lives ONLY in this adapter-side config
//! (C11): kernel types carry [`lingxi_kernel::model_exchange::CredentialReference`]
//! — identity + kind, never the secret.

use std::collections::BTreeMap;

use lingxi_kernel::model_exchange::{AuxiliarySlot, ProtocolFamily};
use serde::Deserialize;

/// The model-plane half of the service config file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelPlaneConfig {
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub models: ModelsSection,
    /// The unified network policy section (R05 RR1 F14, T05-C12): the
    /// frozen proxy/NO_PROXY/explicit-CA carrier every outbound consumer of
    /// the model plane reads. Absent = the incumbent default (system mode,
    /// platform roots only).
    #[serde(default)]
    pub network: super::network::NetworkConfigSection,
}

/// One provider entry of the `providers` section.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderConfig {
    /// Protocol family vocabulary (`openai-completions`, ...). Parsed and
    /// validated at load; an unknown family is a loud config error, never
    /// a guess.
    pub protocol: String,
    /// Base URL the adapter dispatches against (`{endpoint}/chat/completions`
    /// for the OpenAI completions family).
    pub endpoint: String,
    /// Credential. EXPLICIT always — a missing `auth` key is a config
    /// error (C11), never a defaulted anonymous client.
    pub auth: AuthConfig,
}

/// The credential shape of one provider (the material half; kernel types
/// carry only the reference).
///
/// R05-T02: the vocabulary now covers every auth mode the incumbent product
/// uses. MATERIAL authority moved to the service-side CredentialService
/// (C01: the single credential exit); the config entry is the SEED for the
/// static shapes (`apiKey` / `authHeader`) and the FLOW DESCRIPTOR for
/// `oauth` (OAuth token material never lives in config — it is minted by a
/// login flow and persisted in the credential store).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum AuthConfig {
    /// Bearer API key.
    #[serde(rename = "apiKey")]
    ApiKey {
        #[serde(rename = "apiKey")]
        api_key: String,
    },
    /// One custom auth header sent verbatim (the incumbent provider
    /// `headers` credential shape, single-header form). The header name is
    /// validated at load (RFC token shape + the hop-by-hop/framing
    /// blocklist); the value is material.
    #[serde(rename = "authHeader")]
    AuthHeader { header: String, value: String },
    /// OAuth (authorization-code+PKCE or device-code). Config carries only
    /// the flow descriptor; token material lives in the credential store.
    #[serde(rename = "oauth")]
    OAuth(OAuthFlowConfig),
    /// Explicitly keyless (a local no-credential server, ollama-style).
    #[serde(rename = "none")]
    None,
}

/// The OAuth flow descriptor of one provider (R05-T02). This is a
/// DESCRIPTOR — no token material. `flow` selects the state machine;
/// endpoints are validated at load (absolute http(s), no userinfo).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OAuthFlowConfig {
    pub flow: OAuthFlowKind,
    pub client_id: String,
    /// Token endpoint (both flows; also the refresh endpoint).
    pub token_endpoint: String,
    /// Authorization endpoint (authorization-code+PKCE only).
    #[serde(default)]
    pub authorize_endpoint: Option<String>,
    /// Device authorization endpoint (device-code only).
    #[serde(default)]
    pub device_authorization_endpoint: Option<String>,
    /// OAuth scope string (sent verbatim; `None` = the server default).
    #[serde(default)]
    pub scopes: Option<String>,
}

/// The two OAuth flow families the incumbent product uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum OAuthFlowKind {
    /// openai-codex style: browser authorize URL + PKCE + loopback redirect
    /// callback with a one-time state.
    #[serde(rename = "authorizationCodePkce")]
    AuthorizationCodePkce,
    /// xai style: device authorization + polling with pending/slow_down.
    #[serde(rename = "deviceCode")]
    DeviceCode,
}

impl AuthConfig {
    /// The kernel-side reference kind of this credential.
    pub fn kind(&self) -> lingxi_kernel::model_exchange::CredentialAuthKind {
        match self {
            AuthConfig::ApiKey { .. } => lingxi_kernel::model_exchange::CredentialAuthKind::ApiKey,
            AuthConfig::AuthHeader { .. } => {
                lingxi_kernel::model_exchange::CredentialAuthKind::AuthHeader
            }
            AuthConfig::OAuth(_) => lingxi_kernel::model_exchange::CredentialAuthKind::OAuth,
            AuthConfig::None => lingxi_kernel::model_exchange::CredentialAuthKind::None,
        }
    }
}

/// The `models` section: one optional route binding per operation slot.
/// Every slot resolves INDEPENDENTLY (C07) — a slot without its own
/// binding is an explicit unconfigured state, never an accidental share of
/// another slot's route. Unknown slot keys are refused.
///
/// R05-T06: the operation bindings (`embedding`/`rerank`/`image`/`video`/
/// `speech`/`speechRecognition`) join the chat/auxiliary slots with the
/// same independent-resolution discipline.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelsSection {
    #[serde(default)]
    pub chat: Option<RouteBinding>,
    #[serde(default)]
    pub title: Option<RouteBinding>,
    #[serde(default)]
    pub summarize: Option<RouteBinding>,
    #[serde(default)]
    pub memory: Option<RouteBinding>,
    #[serde(default)]
    pub vision: Option<RouteBinding>,
    #[serde(default)]
    pub approval: Option<RouteBinding>,
    #[serde(default)]
    pub guard: Option<RouteBinding>,
    #[serde(default)]
    pub embedding: Option<RouteBinding>,
    #[serde(default)]
    pub rerank: Option<RouteBinding>,
    #[serde(default)]
    pub image: Option<RouteBinding>,
    #[serde(default)]
    pub video: Option<RouteBinding>,
    #[serde(default)]
    pub speech: Option<RouteBinding>,
    #[serde(default)]
    pub speech_recognition: Option<RouteBinding>,
}

/// One route binding: provider + model as ONE identity unit (C04 — the
/// same model id under another provider is another model).
///
/// R05-T05: the optional `compat` block carries the declared protocol hints
/// the ported provider-compat layer reads ([`RouteCompatHints`]) — the
/// config-side half of the TS model object's `compat` / capability fields.
/// Absent = derivation by provider/endpoint/api only (the incumbent
/// behavior for a route without declared metadata).
///
/// R05-T06: `group_id` is the MiniMax `GroupId` query parameter (the
/// incumbent model entry's `groupId` field — `minimax-embeddings` /
/// `minimax-tts` / `minimax-images` build it into the URL; a binding onto a
/// MiniMax family without it is a loud dispatch-time refusal, never a
/// guessed account). It is inert for every other family.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteBinding {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub compat: Option<RouteCompatHints>,
    #[serde(default)]
    pub group_id: Option<String>,
    /// R05 RR1 F01: the DECLARED per-model capabilities of this binding.
    /// Absent = the undeclared state (a turn that NEEDS a capability is
    /// refused locally before any request leaves the process — R05-A02;
    /// a model NAME is never treated as a capability database).
    #[serde(default)]
    pub capabilities: Option<RouteCapabilities>,
}

/// The declared capability block of one route binding (R05 RR1 F01). Each
/// field is a three-state declaration: `Some(true)` = the model declares
/// support, `Some(false)` = explicitly unsupported, `None` = undeclared.
/// Only `Some(true)` authorizes a turn that needs the capability — an
/// undeclared or explicitly-unsupported need is a LOUD local refusal with
/// zero physical requests, never a silent tool/image drop, provider swap
/// or model substitution.
///
/// Scope note: `tools` covers tool declarations on the turn input;
/// `imageInput` covers host-authorized image inputs (the chat route and
/// the auxiliary `vision` slot). Reasoning support / context and output
/// budgets / protocol options are the `compat` block's carriers (the TS
/// model object's `reasoning`, `maxTokens`, `contextWindow` fields) — this
/// block does not duplicate them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteCapabilities {
    /// Whether the bound model accepts tool declarations (`Some(true)`
    /// required for any turn carrying a non-empty tool snapshot).
    #[serde(default)]
    pub tools: Option<bool>,
    /// Whether the bound model accepts image inputs (`Some(true)`
    /// required for any turn carrying host-authorized images).
    #[serde(default)]
    pub image_input: Option<bool>,
}

/// The declared compat hints of one route binding (R05-T05). Every field
/// mirrors the TS model field of the same protocol meaning
/// (`shared/model-capabilities.ts` / `core/provider-compat/*`); the Rust
/// compat layer ([`super::compat`]) reads ONLY these declared values plus
/// its derivation chain — never a guessed default.
///
/// Field mapping to the TS model object:
/// - `thinking_format` ↔ `model.compat.thinkingFormat`
/// - `reasoning_profile` ↔ `model.compat.reasoningProfile`
/// - `cache_control_format` ↔ `model.compat.cacheControlFormat`
/// - `reasoning` ↔ `model.reasoning`
/// - `max_tokens` ↔ `model.maxTokens` (the `model.maxOutput` fallback read
///   is NOT representable here — registered gap; declare `maxTokens`)
/// - `context_window` ↔ `model.contextWindow`
/// - `quirks` ↔ `model.quirks`
/// - `thinking_level_map` ↔ `model.thinkingLevelMap`
/// - `output_includes_thinking` ↔ `model.compat.outputIncludesThinking`
///   (and the top-level `model.outputIncludesThinking` projection)
/// - `video` ↔ `model.video === true || model.compat.hanaVideoInput === true`
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteCompatHints {
    #[serde(default)]
    pub thinking_format: Option<String>,
    #[serde(default)]
    pub reasoning_profile: Option<String>,
    #[serde(default)]
    pub cache_control_format: Option<String>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub quirks: Vec<String>,
    /// Thinking-level → provider wire value (`null` = the level has no wire
    /// representation). Keys are thinking-level vocabulary; values are the
    /// provider's effort string.
    #[serde(default)]
    pub thinking_level_map: Option<BTreeMap<String, Option<String>>>,
    #[serde(default)]
    pub output_includes_thinking: Option<bool>,
    #[serde(default)]
    pub video: Option<bool>,
}

/// The declared `thinkingFormat` vocabulary (mirror of
/// `MODEL_THINKING_FORMATS` in shared/model-capabilities.ts). An unknown
/// declared value is a loud config error — TS `normalizeModelProtocolCompat`
/// silently DROPS unknown values, which is exactly the silent degradation
/// this plane refuses: a typo'd format must surface at load.
const COMPAT_THINKING_FORMATS: &[&str] = &[
    "anthropic",
    "qwen",
    "qwen-chat-template",
    "zhipu",
    "deepseek",
    "openrouter",
    "kimi",
    "volcengine",
    "longcat",
];

/// The declared `reasoningProfile` vocabulary (mirror of
/// `MODEL_REASONING_PROFILES`).
const COMPAT_REASONING_PROFILES: &[&str] = &[
    "anthropic-adaptive-only",
    "deepseek-v4-anthropic",
    "deepseek-v4-openai",
    "deepseek-v4-responses",
    "mimo-openai",
    "openrouter-anthropic-adaptive",
    "zhipu-openai",
    "kimi-openai",
];

/// The declared `cacheControlFormat` vocabulary (only `anthropic` has a
/// wire meaning in the ported compat layer).
const COMPAT_CACHE_CONTROL_FORMATS: &[&str] = &["anthropic"];

/// The thinking-level keys a `thinkingLevelMap` may name (the
/// session-thinking-level vocabulary plus `auto`/`minimal`).
const COMPAT_THINKING_LEVEL_KEYS: &[&str] = &[
    "off", "minimal", "low", "medium", "high", "xhigh", "max", "auto",
];

impl ModelsSection {
    /// The binding of one auxiliary slot.
    pub fn binding_for(&self, slot: AuxiliarySlot) -> Option<&RouteBinding> {
        match slot {
            AuxiliarySlot::Title => self.title.as_ref(),
            AuxiliarySlot::Summarize => self.summarize.as_ref(),
            AuxiliarySlot::Memory => self.memory.as_ref(),
            AuxiliarySlot::Vision => self.vision.as_ref(),
            AuxiliarySlot::Approval => self.approval.as_ref(),
            AuxiliarySlot::Guard => self.guard.as_ref(),
        }
    }

    /// The binding of one operation (R05-T06: chat, the six auxiliary
    /// slots and the six operation bindings share ONE resolution surface —
    /// an unconfigured operation is an explicit state, never a share).
    pub fn binding_for_operation(
        &self,
        operation: lingxi_kernel::model_exchange::ModelOperation,
    ) -> Option<&RouteBinding> {
        use lingxi_kernel::model_exchange::{MediaGenerationKind, ModelOperation};
        match operation {
            ModelOperation::Chat => self.chat.as_ref(),
            ModelOperation::Auxiliary(slot) => self.binding_for(slot),
            ModelOperation::Embedding => self.embedding.as_ref(),
            ModelOperation::Rerank => self.rerank.as_ref(),
            ModelOperation::MediaGeneration { kind } => match kind {
                MediaGenerationKind::Image => self.image.as_ref(),
                MediaGenerationKind::Video => self.video.as_ref(),
                MediaGenerationKind::Speech => self.speech.as_ref(),
            },
            ModelOperation::SpeechRecognition => self.speech_recognition.as_ref(),
        }
    }
}

/// A loud model-plane configuration error. Every variant names the
/// offending key; none is ever recovered by guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelConfigError {
    /// The JSON itself is malformed or violates the closed schema.
    InvalidJson { detail: String },
    /// The `network` section violates its closed shape (R05 RR1 F14): an
    /// unknown proxy mode, an invalid proxy URL, a manual mode without any
    /// URL, or an unparseable trusted-CA PEM bundle.
    InvalidNetworkSection { detail: String },
    /// A provider id is empty.
    EmptyProviderId,
    /// The protocol family name is not in the contract vocabulary.
    UnknownProtocol { provider: String, protocol: String },
    /// The endpoint is not an absolute http(s) URL with a host.
    InvalidEndpoint {
        provider: String,
        endpoint: String,
        detail: String,
    },
    /// `auth: {"kind": "apiKey"}` with an empty key (a remote endpoint is
    /// never contacted with an empty credential).
    EmptyApiKey { provider: String },
    /// A route binding names a provider the `providers` section does not
    /// define.
    UnknownRouteProvider { operation: String, provider: String },
    /// A route binding's model id is empty.
    EmptyModel { operation: String },
    /// `auth: {"kind": "authHeader"}` with an empty value, an invalid
    /// header name (RFC token shape) or a hop-by-hop/framing header name
    /// (mirrors the incumbent FORBIDDEN_PROVIDER_HEADERS rule).
    InvalidAuthHeader { provider: String, detail: String },
    /// `auth: {"kind": "oauth"}` with an empty client id, a missing
    /// flow-required endpoint or an endpoint outside the URL rules.
    InvalidOAuthFlow { provider: String, detail: String },
    /// A route binding's `compat` block violates its closed shape (R05-T05):
    /// an unknown thinking format / reasoning profile / cache-control
    /// format / thinking-level key, a non-positive token bound, or an empty
    /// quirk string. Never silently dropped — a typo'd hint must surface at
    /// load, not disappear from the wire behavior.
    InvalidRouteCompat { operation: String, detail: String },
    /// An endpoint URL embedding userinfo (`https://user:pass@host/…`) —
    /// credentials-in-URL are refused at load so they can never be
    /// dispatched (C10).
    EndpointCarriesUserinfo { provider: String },
    /// R05-T06: an operation binding's provider speaks a protocol family
    /// that cannot serve the operation class (an `embedding` route onto a
    /// chat-completions provider). Loud at LOAD time — the misconfiguration
    /// never reaches dispatch.
    RouteFamilyMismatch {
        operation: String,
        provider: String,
        protocol: String,
    },
}

impl std::fmt::Display for ModelConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelConfigError::InvalidJson { detail } => {
                write!(
                    f,
                    "model plane config is not valid JSON for its closed schema: {detail}"
                )
            }
            ModelConfigError::EmptyProviderId => {
                write!(f, "a provider id in the providers section is empty")
            }
            ModelConfigError::UnknownProtocol { provider, protocol } => write!(
                f,
                "provider {provider:?} declares unknown protocol {protocol:?} (known: {})",
                ProtocolFamily::all()
                    .iter()
                    .map(|family| family.config_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ModelConfigError::InvalidEndpoint {
                provider,
                endpoint,
                detail,
            } => write!(
                f,
                "provider {provider:?} endpoint {endpoint:?} is not an absolute http(s) URL \
                 with a host ({detail})"
            ),
            ModelConfigError::EmptyApiKey { provider } => write!(
                f,
                "provider {provider:?} declares auth kind apiKey with an empty key"
            ),
            ModelConfigError::UnknownRouteProvider {
                operation,
                provider,
            } => write!(
                f,
                "route for operation {operation} names provider {provider:?}, which the \
                 providers section does not define (provider+model are one identity unit — \
                 the provider is never guessed from the model id)"
            ),
            ModelConfigError::EmptyModel { operation } => {
                write!(f, "route for operation {operation} has an empty model id")
            }
            ModelConfigError::InvalidAuthHeader { provider, detail } => write!(
                f,
                "provider {provider:?} declares an invalid authHeader credential: {detail}"
            ),
            ModelConfigError::InvalidOAuthFlow { provider, detail } => write!(
                f,
                "provider {provider:?} declares an invalid oauth flow descriptor: {detail}"
            ),
            ModelConfigError::InvalidNetworkSection { detail } => write!(
                f,
                "the network section declares an invalid network policy: {detail}"
            ),
            ModelConfigError::InvalidRouteCompat { operation, detail } => write!(
                f,
                "route for operation {operation} declares an invalid compat block: {detail}"
            ),
            ModelConfigError::EndpointCarriesUserinfo { provider } => write!(
                f,
                "provider {provider:?} endpoint embeds userinfo (user:password@host); \
                 credentials never travel inside URLs (C10)"
            ),
            ModelConfigError::RouteFamilyMismatch {
                operation,
                provider,
                protocol,
            } => write!(
                f,
                "route for operation {operation} binds provider {provider:?} whose protocol \
                 family {protocol:?} cannot serve that operation class (the family↔operation \
                 matrix is a contract fact — never a silent re-route)"
            ),
        }
    }
}

impl std::error::Error for ModelConfigError {}

impl ModelPlaneConfig {
    /// Parses and validates the sections. Loud on every rule above.
    pub fn parse_and_validate(json: &str) -> Result<Self, ModelConfigError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|err| ModelConfigError::InvalidJson {
                detail: err.to_string(),
            })?;
        Self::from_value(value)
    }

    /// Validates an already-parsed JSON value (the service config file
    /// embeds these sections under its own envelope and hands the subvalue
    /// over).
    pub fn from_value(value: serde_json::Value) -> Result<Self, ModelConfigError> {
        let config: ModelPlaneConfig =
            serde_json::from_value(value).map_err(|err| ModelConfigError::InvalidJson {
                detail: err.to_string(),
            })?;
        config.validate()?;
        Ok(config)
    }

    /// Validates an already-parsed config (the service config file parses
    /// its own envelope first and hands the sections over).
    pub fn validate(&self) -> Result<(), ModelConfigError> {
        // R05 RR1 F14: the network policy section validates with the rest
        // of the plane (a bad proxy URL or a malformed CA bundle is a loud
        // LOAD error, never a runtime surprise).
        self.network
            .validate()
            .map_err(|error| ModelConfigError::InvalidNetworkSection {
                detail: error.message,
            })?;
        for (id, provider) in &self.providers {
            if id.trim().is_empty() {
                return Err(ModelConfigError::EmptyProviderId);
            }
            if ProtocolFamily::parse(&provider.protocol).is_none() {
                return Err(ModelConfigError::UnknownProtocol {
                    provider: id.clone(),
                    protocol: provider.protocol.clone(),
                });
            }
            validate_endpoint(id, &provider.endpoint)?;
            match &provider.auth {
                AuthConfig::ApiKey { api_key } => {
                    if api_key.is_empty() {
                        return Err(ModelConfigError::EmptyApiKey {
                            provider: id.clone(),
                        });
                    }
                }
                AuthConfig::AuthHeader { header, value } => {
                    validate_auth_header(id, header, value)?;
                }
                AuthConfig::OAuth(flow) => validate_oauth_flow(id, flow)?,
                AuthConfig::None => {}
            }
        }
        let validate_binding =
            |operation: &str, binding: &Option<RouteBinding>| -> Result<(), ModelConfigError> {
                if let Some(binding) = binding {
                    if binding.model.trim().is_empty() {
                        return Err(ModelConfigError::EmptyModel {
                            operation: operation.to_string(),
                        });
                    }
                    if !self.providers.contains_key(&binding.provider) {
                        return Err(ModelConfigError::UnknownRouteProvider {
                            operation: operation.to_string(),
                            provider: binding.provider.clone(),
                        });
                    }
                    if let Some(compat) = &binding.compat {
                        validate_route_compat(operation, compat)?;
                    }
                }
                Ok(())
            };
        validate_binding("chat", &self.models.chat)?;
        for slot in [
            AuxiliarySlot::Title,
            AuxiliarySlot::Summarize,
            AuxiliarySlot::Memory,
            AuxiliarySlot::Vision,
            AuxiliarySlot::Approval,
            AuxiliarySlot::Guard,
        ] {
            validate_binding(
                &format!("auxiliary.{}", slot.config_key()),
                &self.models.binding_for(slot).cloned(),
            )?;
        }
        // R05-T06: the operation bindings. Beyond the shared shape rules,
        // the bound provider's family must SERVE the operation class —
        // an `embedding` route onto a chat-completions provider is a loud
        // load-time error, never a dispatch-time surprise (C06).
        for (operation, binding) in [
            (
                lingxi_kernel::model_exchange::ModelOperation::Embedding,
                &self.models.embedding,
            ),
            (
                lingxi_kernel::model_exchange::ModelOperation::Rerank,
                &self.models.rerank,
            ),
            (
                lingxi_kernel::model_exchange::ModelOperation::MediaGeneration {
                    kind: lingxi_kernel::model_exchange::MediaGenerationKind::Image,
                },
                &self.models.image,
            ),
            (
                lingxi_kernel::model_exchange::ModelOperation::MediaGeneration {
                    kind: lingxi_kernel::model_exchange::MediaGenerationKind::Video,
                },
                &self.models.video,
            ),
            (
                lingxi_kernel::model_exchange::ModelOperation::MediaGeneration {
                    kind: lingxi_kernel::model_exchange::MediaGenerationKind::Speech,
                },
                &self.models.speech,
            ),
            (
                lingxi_kernel::model_exchange::ModelOperation::SpeechRecognition,
                &self.models.speech_recognition,
            ),
        ] {
            validate_binding(&operation.describe(), binding)?;
            if let Some(binding) = binding {
                if let Some(provider) = self.providers.get(&binding.provider) {
                    let family =
                        ProtocolFamily::parse(&provider.protocol).expect("validated above");
                    if !family.serves(operation.operation_class()) {
                        return Err(ModelConfigError::RouteFamilyMismatch {
                            operation: operation.describe(),
                            provider: binding.provider.clone(),
                            protocol: provider.protocol.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// R05-T05: the `compat` block's value-domain rules. Every declared value
/// is checked against its closed vocabulary — the TS projection layer
/// (`normalizeModelProtocolCompat`) silently DROPS unknown values; a
/// user-authored config must instead fail loudly at load (no silent
/// degradation of the wire behavior).
fn validate_route_compat(
    operation: &str,
    compat: &RouteCompatHints,
) -> Result<(), ModelConfigError> {
    let invalid = |detail: String| ModelConfigError::InvalidRouteCompat {
        operation: operation.to_string(),
        detail,
    };
    if let Some(format) = &compat.thinking_format {
        if !COMPAT_THINKING_FORMATS.contains(&format.as_str()) {
            return Err(invalid(format!(
                "thinkingFormat {format:?} is not in the known vocabulary {COMPAT_THINKING_FORMATS:?}"
            )));
        }
    }
    if let Some(profile) = &compat.reasoning_profile {
        if !COMPAT_REASONING_PROFILES.contains(&profile.as_str()) {
            return Err(invalid(format!(
                "reasoningProfile {profile:?} is not in the known vocabulary {COMPAT_REASONING_PROFILES:?}"
            )));
        }
    }
    if let Some(format) = &compat.cache_control_format {
        if !COMPAT_CACHE_CONTROL_FORMATS.contains(&format.as_str()) {
            return Err(invalid(format!(
                "cacheControlFormat {format:?} is not in the known vocabulary {COMPAT_CACHE_CONTROL_FORMATS:?}"
            )));
        }
    }
    if let Some(max_tokens) = compat.max_tokens {
        if max_tokens == 0 {
            return Err(invalid("maxTokens must be >= 1".to_string()));
        }
    }
    if let Some(context_window) = compat.context_window {
        if context_window == 0 {
            return Err(invalid("contextWindow must be >= 1".to_string()));
        }
    }
    if let Some(quirk) = compat.quirks.iter().find(|quirk| quirk.trim().is_empty()) {
        return Err(invalid(format!(
            "quirks entries must be non-empty, got {quirk:?}"
        )));
    }
    if let Some(map) = &compat.thinking_level_map {
        for (level, value) in map {
            if !COMPAT_THINKING_LEVEL_KEYS.contains(&level.as_str()) {
                return Err(invalid(format!(
                    "thinkingLevelMap key {level:?} is not a thinking level {COMPAT_THINKING_LEVEL_KEYS:?}"
                )));
            }
            if let Some(mapped) = value {
                if mapped.trim().is_empty() {
                    return Err(invalid(format!(
                        "thinkingLevelMap[{level:?}] must be null or a non-empty provider value"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Endpoint rule: absolute `http://` / `https://` URL with a non-empty
/// host and NO userinfo (credentials never travel inside URLs — C10; the
/// full parse happens in reqwest at dispatch; anything beyond this shape is
/// refused at load).
fn validate_endpoint(provider: &str, endpoint: &str) -> Result<(), ModelConfigError> {
    let invalid = |detail: &str| ModelConfigError::InvalidEndpoint {
        provider: provider.to_string(),
        endpoint: endpoint.to_string(),
        detail: detail.to_string(),
    };
    let rest = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .ok_or_else(|| invalid("scheme must be https or http"))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return Err(invalid("missing host"));
    }
    if host.contains('@') {
        return Err(ModelConfigError::EndpointCarriesUserinfo {
            provider: provider.to_string(),
        });
    }
    if endpoint.chars().any(char::is_whitespace) {
        return Err(invalid("whitespace in URL"));
    }
    Ok(())
}

/// Hop-by-hop / framing header names a credential header must never occupy
/// (mirror of the incumbent `FORBIDDEN_PROVIDER_HEADERS`).
const FORBIDDEN_AUTH_HEADER_NAMES: &[&str] = &[
    "accept-encoding",
    "connection",
    "content-length",
    "expect",
    "host",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Auth-header rule (mirror of the incumbent `normalizeProviderHeaders`
/// name rule): an RFC 7230 token name, not on the framing blocklist, and a
/// non-empty value.
fn validate_auth_header(provider: &str, header: &str, value: &str) -> Result<(), ModelConfigError> {
    let invalid = |detail: &str| ModelConfigError::InvalidAuthHeader {
        provider: provider.to_string(),
        detail: detail.to_string(),
    };
    if header.is_empty()
        || !header.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
    {
        return Err(invalid("header name is not an RFC 7230 token"));
    }
    if FORBIDDEN_AUTH_HEADER_NAMES.contains(&header.to_ascii_lowercase().as_str()) {
        return Err(invalid(
            "header name is hop-by-hop/framing and can never carry a credential",
        ));
    }
    if value.is_empty() {
        return Err(invalid("header value is empty"));
    }
    if value.bytes().any(|b| b == b'\r' || b == b'\n') {
        return Err(invalid("header value contains CR/LF (header splitting)"));
    }
    Ok(())
}

/// OAuth flow descriptor rules: non-empty client id; every present endpoint
/// passes the endpoint rule; each flow's required endpoint exists.
fn validate_oauth_flow(provider: &str, flow: &OAuthFlowConfig) -> Result<(), ModelConfigError> {
    let invalid = |detail: String| ModelConfigError::InvalidOAuthFlow {
        provider: provider.to_string(),
        detail,
    };
    if flow.client_id.trim().is_empty() {
        return Err(invalid("clientId is empty".to_string()));
    }
    validate_endpoint(provider, &flow.token_endpoint).map_err(|err| match err {
        ModelConfigError::InvalidEndpoint { detail, .. } => {
            invalid(format!("tokenEndpoint: {detail}"))
        }
        ModelConfigError::EndpointCarriesUserinfo { .. } => {
            invalid("tokenEndpoint embeds userinfo".to_string())
        }
        other => other,
    })?;
    match flow.flow {
        OAuthFlowKind::AuthorizationCodePkce => {
            let endpoint = flow.authorize_endpoint.as_deref().ok_or_else(|| {
                invalid("authorizationCodePkce requires authorizeEndpoint".to_string())
            })?;
            validate_endpoint(provider, endpoint).map_err(|err| match err {
                ModelConfigError::InvalidEndpoint { detail, .. } => {
                    invalid(format!("authorizeEndpoint: {detail}"))
                }
                ModelConfigError::EndpointCarriesUserinfo { .. } => {
                    invalid("authorizeEndpoint embeds userinfo".to_string())
                }
                other => other,
            })?;
        }
        OAuthFlowKind::DeviceCode => {
            let endpoint = flow
                .device_authorization_endpoint
                .as_deref()
                .ok_or_else(|| {
                    invalid("deviceCode requires deviceAuthorizationEndpoint".to_string())
                })?;
            validate_endpoint(provider, endpoint).map_err(|err| match err {
                ModelConfigError::InvalidEndpoint { detail, .. } => {
                    invalid(format!("deviceAuthorizationEndpoint: {detail}"))
                }
                ModelConfigError::EndpointCarriesUserinfo { .. } => {
                    invalid("deviceAuthorizationEndpoint embeds userinfo".to_string())
                }
                other => other,
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_json() -> &'static str {
        r#"{
            "providers": {
                "main": {
                    "protocol": "openai-completions",
                    "endpoint": "https://api.example.test/v1",
                    "auth": {"kind": "apiKey", "apiKey": "sk-test"}
                }
            },
            "models": {
                "chat": {"provider": "main", "model": "gpt-test"},
                "title": {"provider": "main", "model": "gpt-test-mini"}
            }
        }"#
    }

    #[test]
    fn valid_config_parses() {
        let config = ModelPlaneConfig::parse_and_validate(valid_json()).expect("valid");
        assert_eq!(config.providers.len(), 1);
        assert_eq!(
            config.models.chat.as_ref().map(|b| b.model.as_str()),
            Some("gpt-test")
        );
        assert!(config.models.guard.is_none());
    }

    #[test]
    fn unknown_keys_are_refused() {
        let bad = valid_json().replace(
            "\"models\": {",
            "\"models\": {\n\"translate\": {\"provider\": \"main\", \"model\": \"e\"},",
        );
        assert!(
            matches!(
                ModelPlaneConfig::parse_and_validate(&bad),
                Err(ModelConfigError::InvalidJson { .. })
            ),
            "an unknown slot key is refused at load (C06)"
        );
        let bad = valid_json().replace("\"endpoint\":", "\"baseUrl\":");
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidJson { .. })
        ));
    }

    #[test]
    fn t06_operation_bindings_parse_and_family_mismatch_is_loud() {
        // The six operation bindings are first-class slots (T06).
        let good = valid_json()
            .replace(
                "\"models\": {",
                "\"models\": {\n\"embedding\": {\"provider\": \"emb\", \"model\": \"e\"},",
            )
            .replace(
                "\"providers\": {",
                "\"providers\": {\n\"emb\": {\"protocol\": \"openai-embeddings\", \
             \"endpoint\": \"https://emb.example.test/v1\", \
             \"auth\": {\"kind\": \"apiKey\", \"apiKey\": \"sk-emb\"}},",
            );
        let config = ModelPlaneConfig::parse_and_validate(&good).expect("valid");
        assert_eq!(
            config.models.embedding.as_ref().map(|b| b.model.as_str()),
            Some("e")
        );
        // A chat-family provider bound to an operation slot is a loud
        // load-time mismatch (never a dispatch-time surprise).
        let bad = valid_json().replace(
            "\"models\": {",
            "\"models\": {\n\"embedding\": {\"provider\": \"main\", \"model\": \"e\"},",
        );
        assert!(
            matches!(
                ModelPlaneConfig::parse_and_validate(&bad),
                Err(ModelConfigError::RouteFamilyMismatch { .. })
            ),
            "embedding onto openai-completions refuses at load"
        );
    }

    #[test]
    fn credential_is_explicit_and_never_defaulted() {
        let bad = valid_json().replace(
            "\"auth\": {\"kind\": \"apiKey\", \"apiKey\": \"sk-test\"}",
            "\"auth\": {\"kind\": \"apiKey\", \"apiKey\": \"\"}",
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::EmptyApiKey { .. })
        ));
        // A missing auth key entirely is a schema error (C11).
        let bad = valid_json().replace(
            "            \"auth\": {\"kind\": \"apiKey\", \"apiKey\": \"sk-test\"}\n",
            "",
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidJson { .. })
        ));
    }

    #[test]
    fn unknown_protocol_and_route_provider_are_loud() {
        let bad = valid_json().replace("openai-completions", "made-up-protocol");
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::UnknownProtocol { .. })
        ));
        let bad = valid_json().replace(
            "\"provider\": \"main\", \"model\": \"gpt-test-mini\"",
            "\"provider\": \"ghost\", \"model\": \"gpt-test-mini\"",
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::UnknownRouteProvider { .. })
        ));
    }

    #[test]
    fn endpoint_shape_is_validated() {
        let bad = valid_json().replace("https://api.example.test/v1", "not-a-url");
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidEndpoint { .. })
        ));
        let local = valid_json().replace("https://api.example.test/v1", "http://127.0.0.1:8787/v1");
        assert!(ModelPlaneConfig::parse_and_validate(&local).is_ok());
    }

    // ── R05-T02: the authHeader / oauth credential shapes (C01) ─────────────

    #[test]
    fn auth_header_shape_is_validated() {
        let auth = r#""auth": {"kind": "authHeader", "header": "X-Api-Key", "value": "k-1"}"#;
        let ok = valid_json().replace(r#""auth": {"kind": "apiKey", "apiKey": "sk-test"}"#, auth);
        let parsed = ModelPlaneConfig::parse_and_validate(&ok).expect("authHeader parses");
        assert!(matches!(
            parsed.providers["main"].auth,
            AuthConfig::AuthHeader { .. }
        ));
        assert_eq!(
            parsed.providers["main"].auth.kind(),
            lingxi_kernel::model_exchange::CredentialAuthKind::AuthHeader
        );
        for (header, value, why) in [
            ("Host", "k-1", "framing header name refused"),
            (
                "content-length",
                "k-1",
                "framing header name refused (case-insensitive)",
            ),
            ("not a header", "k-1", "non-token header name refused"),
            ("X-Api-Key", "", "empty value refused"),
            ("X-Api-Key", "k\r\nInjected: 1", "CR/LF value refused"),
        ] {
            let bad = valid_json().replace(
                r#""auth": {"kind": "apiKey", "apiKey": "sk-test"}"#,
                &format!(
                    r#""auth": {{"kind": "authHeader", "header": {}, "value": {}}}"#,
                    serde_json::to_string(header).expect("json"),
                    serde_json::to_string(value).expect("json"),
                ),
            );
            assert!(
                matches!(
                    ModelPlaneConfig::parse_and_validate(&bad),
                    Err(ModelConfigError::InvalidAuthHeader { .. })
                ),
                "{why}"
            );
        }
    }

    #[test]
    fn oauth_flow_descriptors_are_validated() {
        let device = r#""auth": {"kind": "oauth", "flow": "deviceCode",
            "clientId": "client-1",
            "tokenEndpoint": "https://auth.example.test/token",
            "deviceAuthorizationEndpoint": "https://auth.example.test/device",
            "scopes": "openid offline_access"}"#;
        let ok = valid_json().replace(r#""auth": {"kind": "apiKey", "apiKey": "sk-test"}"#, device);
        let parsed = ModelPlaneConfig::parse_and_validate(&ok).expect("oauth deviceCode parses");
        assert_eq!(
            parsed.providers["main"].auth.kind(),
            lingxi_kernel::model_exchange::CredentialAuthKind::OAuth
        );
        // Missing the flow-required endpoint is loud.
        let bad = valid_json().replace(
            r#""auth": {"kind": "apiKey", "apiKey": "sk-test"}"#,
            r#""auth": {"kind": "oauth", "flow": "deviceCode",
                "clientId": "client-1",
                "tokenEndpoint": "https://auth.example.test/token"}"#,
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidOAuthFlow { .. })
        ));
        // Empty client id is loud.
        let bad = valid_json().replace(
            r#""auth": {"kind": "apiKey", "apiKey": "sk-test"}"#,
            r#""auth": {"kind": "oauth", "flow": "authorizationCodePkce",
                "clientId": "  ",
                "tokenEndpoint": "https://auth.example.test/token",
                "authorizeEndpoint": "https://auth.example.test/authorize"}"#,
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidOAuthFlow { .. })
        ));
    }

    // ── R05-T05: the route-binding compat block (closed shape + vocabulary) ──

    #[test]
    fn route_compat_block_parses_and_validates() {
        let with_compat = valid_json().replace(
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\"}",
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\", \"compat\": {
                \"thinkingFormat\": \"zhipu\",
                \"reasoningProfile\": \"zhipu-openai\",
                \"reasoning\": true,
                \"maxTokens\": 65536,
                \"contextWindow\": 131072,
                \"quirks\": [\"enable_thinking\"],
                \"thinkingLevelMap\": {\"xhigh\": \"max\", \"low\": null},
                \"outputIncludesThinking\": false,
                \"video\": true
            }}",
        );
        let parsed = ModelPlaneConfig::parse_and_validate(&with_compat).expect("compat parses");
        let compat = parsed
            .models
            .chat
            .expect("chat")
            .compat
            .expect("compat block");
        assert_eq!(compat.thinking_format.as_deref(), Some("zhipu"));
        assert_eq!(compat.max_tokens, Some(65536));
        assert_eq!(
            compat
                .thinking_level_map
                .as_ref()
                .and_then(|m| m.get("low")),
            Some(&None)
        );
        // A binding without the block stays legal (the absent-hints state).
        let parsed = ModelPlaneConfig::parse_and_validate(valid_json()).expect("plain parses");
        assert!(parsed.models.chat.expect("chat").compat.is_none());
    }

    #[test]
    fn route_compat_block_is_loud_on_unknown_keys_and_values() {
        // Unknown key inside the closed block.
        let bad = valid_json().replace(
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\"}",
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\", \"compat\": {\"thinking\": \"zhipu\"}}",
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidJson { .. })
        ));
        // Unknown thinking format — loudly refused, never silently dropped.
        let bad = valid_json().replace(
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\"}",
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\", \"compat\": {\"thinkingFormat\": \"zhpu\"}}",
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidRouteCompat { .. })
        ));
        // Zero token bound.
        let bad = valid_json().replace(
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\"}",
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\", \"compat\": {\"maxTokens\": 0}}",
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidRouteCompat { .. })
        ));
        // Unknown thinking-level-map key.
        let bad = valid_json().replace(
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\"}",
            "\"chat\": {\"provider\": \"main\", \"model\": \"gpt-test\", \"compat\": {\"thinkingLevelMap\": {\"turbo\": \"x\"}}}",
        );
        assert!(matches!(
            ModelPlaneConfig::parse_and_validate(&bad),
            Err(ModelConfigError::InvalidRouteCompat { .. })
        ));
    }
}
