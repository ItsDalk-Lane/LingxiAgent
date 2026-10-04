//! The typed per-turn model exchange contract (R05-T01).
//!
//! R03/R04 drove provider doubles with a bare `&str` input: enough to pin
//! the run-lifecycle ownership contract, not enough for a real protocol
//! adapter. A real adapter must see, per model call:
//!
//! - the original submission that opened the run (the request identity),
//! - the COMPLETE prior exchange of the run so far — assistant content
//!   blocks (text / reasoning / provider-opaque, in order) interleaved with
//!   the tool calls the model asked for and the REAL tool outcomes they
//!   produced (all four [`crate::ports::ToolOutcome`] states, truncation and
//!   resource references intact),
//! - the tool declaration snapshot taken from the REAL registry at send
//!   time (its catalog generation included — C08), and
//! - the cancellation/deadline facts of the call.
//!
//! [`ModelTurnInput`] carries exactly that. `TurnProviderPort::next_turn`
//! consumes it (the R03 `&str` shape is gone — there is ONE signature; the
//! frozen thing is the R03/R04 run semantics, never the old parameter
//! list). Every provider-call pairing rule is explicit: the host
//! [`ToolCallId`] is minted by the run driver and stays the only identity
//! that binds journal/receipt/state writes; the provider's own call id
//! (`provider_call_id`, e.g. an OpenAI `tool_calls[].id`) is protocol
//! correlation data only — an adapter maps it back to the host identity
//! through THIS contract, never the other way round.
//!
//! The model gateway half ([`ModelGatewayPort`]) is the unified
//! model-selection + credential-reference resolution point (C04/C06/C07):
//! one place decides WHICH provider/model serves an operation, refuses
//! unsupported capabilities BEFORE any request leaves the process (never a
//! silent fallback, never a silent model swap) and hands the adapter a
//! route whose credential is a REFERENCE — credential material itself never
//! enters a kernel type (the R02 CredentialPort stance: only the
//! service/adapter boundary holds secrets).

use lingxi_protocol::{ArgsDigest, ContentBlock, ModelCallId, ToolCallId, ToolSchemaDocument};

use crate::ports::ToolOutcome;
use crate::toolcatalog::{Availability, ToolRegistry};

/// One tool as declared to the provider. `target` is the host identity
/// (the registry target id — what the journal, the gateway and the policy
/// plane bind to); `wire_name` is the protocol-face name sent to the
/// provider, unique within one snapshot (the snapshot's mapping is the ONLY
/// name resolution the adapter performs — a wire name that maps to nothing
/// is a protocol violation of the provider, not a guess).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDeclaration {
    pub target: String,
    pub wire_name: String,
    pub description: String,
    /// The verbatim input schema document (never normalized or pruned).
    pub input_schema: ToolSchemaDocument,
}

/// The tool catalog snapshot a model call is sent with: the REAL registry
/// state at send time, catalog generation included (C08). The driver
/// rebuilds it per turn from the live registry; an adapter declares exactly
/// these tools to the provider — never a cached, fabricated or widened set.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDeclarationSnapshot {
    /// The registry's catalog generation the declarations were taken at.
    /// `0` is the empty snapshot (no registry wired): it can never equal a
    /// real registry's generation (registry generations start at 1).
    pub catalog_generation: u64,
    pub declarations: Vec<ToolDeclaration>,
}

/// Why a snapshot could not be built from the live registry (loud, never a
/// degraded partial snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolDeclarationError {
    /// Two callable targets map to the same wire name even after the
    /// deterministic disambiguation — the mapping would not be unique
    /// (C08). Naming the colliding names is the diagnostic.
    WireNameConflict {
        wire_name: String,
        targets: Vec<String>,
    },
}

impl std::fmt::Display for ToolDeclarationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolDeclarationError::WireNameConflict { wire_name, targets } => write!(
                f,
                "wire name {wire_name:?} is claimed by several callable targets \
                 {targets:?}: the declaration mapping would not be unique"
            ),
        }
    }
}

impl std::error::Error for ToolDeclarationError {}

impl ToolDeclarationSnapshot {
    /// The empty snapshot (no registry wired — the R03 double wiring):
    /// generation 0, zero declarations.
    pub fn empty() -> Self {
        Self {
            catalog_generation: 0,
            declarations: Vec::new(),
        }
    }

    /// Builds the declaration snapshot from the registry's CURRENT state:
    /// every CALLABLE-and-resident target (the incumbent's system-prompt
    /// surface; `Deferred` tools are callable but not resident, so they are
    /// discovered on demand, never declared here), with the catalog
    /// generation pinned. Wire names are unique by construction: the local
    /// name wins when unclaimed; a collision across origins is resolved
    /// deterministically by namespacing the later (target-id order) claim
    /// with its origin prefix, and if even that collides the build fails
    /// loudly ([`ToolDeclarationError::WireNameConflict`]) instead of
    /// presenting an ambiguous mapping to the provider.
    pub fn from_registry(registry: &ToolRegistry) -> Result<Self, ToolDeclarationError> {
        let snapshot = registry.snapshot();
        let mut declarations = Vec::new();
        // Deterministic order: target id sort (snapshot order is the
        // registry's BTreeMap iteration — already sorted by target id).
        for tool in &snapshot.tools {
            if tool.availability != Availability::Available {
                continue;
            }
            let full = registry.describe_full(&tool.target_id).map_err(|_| {
                ToolDeclarationError::WireNameConflict {
                    // Unreachable in a consistent registry (the snapshot and
                    // the describe read the same locked state); surfaced as a
                    // loud build failure rather than a skipped tool.
                    wire_name: tool.local_name.clone(),
                    targets: vec![tool.target_id.as_str().to_string()],
                }
            })?;
            declarations.push(ToolDeclaration {
                target: tool.target_id.as_str().to_string(),
                wire_name: sanitize_wire_name(&tool.local_name),
                description: tool.display_name.clone(),
                input_schema: full.input_schema,
            });
        }
        // Uniqueness pass: same wire name from two targets is resolved by
        // the deterministic namespaced form; still colliding = loud.
        let mut by_name: std::collections::BTreeMap<String, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (index, declaration) in declarations.iter().enumerate() {
            by_name
                .entry(declaration.wire_name.clone())
                .or_default()
                .push(index);
        }
        for (wire_name, indexes) in by_name {
            if indexes.len() <= 1 {
                continue;
            }
            // Deterministic disambiguation: every claimant gets the
            // origin-namespaced form of its target id as its wire name.
            let mut namespaced: Vec<String> = Vec::with_capacity(indexes.len());
            for &index in &indexes {
                namespaced.push(sanitize_wire_name(&declarations[index].target));
            }
            let unique: std::collections::BTreeSet<&String> = namespaced.iter().collect();
            if unique.len() != namespaced.len()
                || namespaced
                    .iter()
                    .any(|name| declarations.iter().any(|d| &d.wire_name == name))
            {
                return Err(ToolDeclarationError::WireNameConflict {
                    wire_name,
                    targets: indexes
                        .iter()
                        .map(|&i| declarations[i].target.clone())
                        .collect(),
                });
            }
            for (&index, name) in indexes.iter().zip(namespaced) {
                declarations[index].wire_name = name;
            }
        }
        Ok(Self {
            catalog_generation: snapshot.catalog_generation,
            declarations,
        })
    }

    /// Reverse lookup: the host target id a provider-facing wire name maps
    /// to within THIS snapshot (`None` = the provider invented a name — a
    /// protocol violation the adapter reports, never a guess).
    pub fn target_of_wire_name(&self, wire_name: &str) -> Option<&str> {
        self.declarations
            .iter()
            .find(|declaration| declaration.wire_name == wire_name)
            .map(|declaration| declaration.target.as_str())
    }

    /// Forward lookup: the wire name a host target was declared under in
    /// THIS snapshot. An adapter re-rendering the exchange history (the
    /// assistant turn's tool calls) uses THIS mapping; a target missing
    /// from the current snapshot (the registry moved mid-run) is a loud
    /// render failure, never a name guess.
    pub fn wire_name_of_target(&self, target: &str) -> Option<&str> {
        self.declarations
            .iter()
            .find(|declaration| declaration.target == target)
            .map(|declaration| declaration.wire_name.as_str())
    }
}

/// Provider-facing tool names must fit the strictest protocol face this
/// stage serves (OpenAI: `^[a-zA-Z0-9_-]{1,64}$`). Every other byte
/// becomes `_`; overlong names are truncated at 64 bytes. The snapshot
/// builder guarantees post-sanitization uniqueness.
fn sanitize_wire_name(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let mut truncated = sanitized;
    if truncated.len() > 64 {
        truncated.truncate(64);
        while !truncated.is_char_boundary(truncated.len()) {
            truncated.pop();
        }
    }
    truncated
}

/// One tool call the model asked for, as the exchange history carries it:
/// the host identity (driver-minted [`ToolCallId`]) PAIRED with the
/// provider's own correlation id when the protocol has one. The provider id
/// never binds any host-side state — it exists so the adapter can emit the
/// protocol-required pairing (e.g. OpenAI `tool_call_id` on tool messages)
/// and so audit trails can join the two vocabularies.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestedToolCall {
    pub tool_call_id: ToolCallId,
    pub provider_call_id: Option<String>,
    pub target: String,
    /// The complete effective arguments the executor consumed (the trusted
    /// boundary's digest travels with them).
    pub arguments: crate::toolcatalog::EffectiveArguments,
    pub args_digest: ArgsDigest,
    pub args_summary: Option<String>,
}

/// One item of the run's prior model exchange, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum ExchangeItem {
    /// One completed assistant turn: the model's content blocks (text /
    /// reasoning / provider-opaque, in the order they arrived — a mixed
    /// response keeps EVERY block) plus the tool calls it asked for.
    AssistantTurn {
        call: ModelCallId,
        content: Vec<ContentBlock>,
        tool_calls: Vec<RequestedToolCall>,
    },
    /// The REAL outcome of one executed tool call — all four
    /// [`ToolOutcome`] states with content blocks, resource refs,
    /// truncation and process status intact (the adapter renders it into
    /// the protocol's tool-result shape; nothing is flattened to a bare
    /// digest or a fabricated success).
    ToolResult {
        tool_call_id: ToolCallId,
        provider_call_id: Option<String>,
        outcome: ToolOutcome,
    },
}

/// The typed input of ONE model turn (R05-T01): what
/// `TurnProviderPort::next_turn` consumes. Everything the adapter needs to
/// build the provider request comes from HERE plus the gateway's resolved
/// route — never from model-supplied identity claims (C10) and never from
/// process-global state.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelTurnInput {
    /// The user submission that opened the run, with any steering text
    /// drained before THIS turn appended under the frozen
    /// `[steering]` convention (the incumbent semantics: steered text
    /// reaches the next model call's input; the loop never interrupts).
    pub submission: String,
    /// The system prompt the host layer resolved for this run (R05-T03:
    /// the contract anchor the persona/system layer lands on). `None`
    /// today — the driver passes `None` and every adapter omits the
    /// family-specific system slot entirely (except codex, whose protocol
    /// demands an `instructions` string and falls back to its documented
    /// constant); persona wiring is a later stage's decision, never an
    /// adapter-local guess.
    pub system_prompt: Option<String>,
    /// 1-based turn index of the run drive (matches the model call id's
    /// sequence; monotonic across the attempts of one run — a retried
    /// attempt keeps numbering its calls after the failed attempt's).
    pub turn: u32,
    /// The run's prior exchange, oldest first (empty on turn 1 of a
    /// run). R05-T03 C12: a retried attempt CONTINUES this exchange — the
    /// confirmed assistant/tool exchanges of the failed attempt are REAL
    /// history the next call must see (a retry never rebuilds from the
    /// bare submission, never re-executes a confirmed write); only the
    /// failed call itself leaves no assistant turn behind.
    pub prior: Vec<ExchangeItem>,
    /// The tool declaration snapshot at send time (C08).
    pub tools: ToolDeclarationSnapshot,
    /// Absolute deadline of the call when one exists (None today: the
    /// run-level cancellation tree is the cancel authority; the field is
    /// the contract anchor budget wiring lands on).
    pub deadline_unix_ms: Option<u64>,
    /// R05-T06 (auxiliary vision slot / media-capable aux calls): image
    /// inputs the HOST authorized and read (bytes + MIME, bounded) — the
    /// model-facing request renders them per family (openai `image_url`
    /// data URLs, anthropic `image` blocks, google `inlineData`,
    /// responses `input_image`). The main chat path never sets this
    /// (empty), so the incumbent chat wire is byte-identical.
    pub images: Vec<InputImage>,
    /// R05-T06: the host-decided output-token bound of THIS call (worker
    /// callbacks / aux slots). `None` = the family default (the incumbent
    /// chat path behavior; anthropic keeps its required 16384 constant).
    pub max_output_tokens: Option<u32>,
}

/// One host-authorized image input of a model call (R05-T06). The bytes
/// were read through the resource-authorization boundary with a size
/// bound; the MIME is the sniffed/declared image type. No path travels in
/// this type — a local path never reaches the provider payload (N-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputImage {
    pub bytes: Vec<u8>,
    pub mime: String,
}

impl ModelTurnInput {
    /// The first-turn shape: a bare submission, empty exchange, explicit
    /// tool snapshot, no deadline, no system prompt.
    pub fn first_turn(submission: impl Into<String>, tools: ToolDeclarationSnapshot) -> Self {
        Self {
            submission: submission.into(),
            system_prompt: None,
            turn: 1,
            prior: Vec::new(),
            tools,
            deadline_unix_ms: None,
            images: Vec::new(),
            max_output_tokens: None,
        }
    }
}

// ── The model gateway (unified selection + credential references) ──────────

/// The protocol family a route speaks. The vocabulary lives in the kernel
/// (it is a contract fact, not an implementation detail); the ADAPTERS own
/// the per-family wire rendering. T01 implemented `OpenAiCompletions`;
/// T03 landed the four remaining chat families (`AnthropicMessages`,
/// `GoogleGenerativeAi`, `OpenAiResponses`, `OpenAiCodexResponses`). T06
/// landed the media/operation dialects (embedding / rerank / image /
/// video / speech / speech-recognition) with REAL per-family encode +
/// decode; `SystemSpeechRecognition` resolves but refuses LOUDLY at the
/// dispatch boundary (the Swift helper's TCC grant belongs to the desktop
/// host bundle — a headless service cannot hold it). A
/// configured-but-unimplemented family is never silently routed onto
/// another family's wire shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolFamily {
    OpenAiCompletions,
    AnthropicMessages,
    GoogleGenerativeAi,
    OpenAiResponses,
    OpenAiCodexResponses,
    VolcengineBigAsr,
    SystemSpeech,
    // ── R05-T06 operation dialects (mirrors of the incumbent protocolId
    // vocabulary in shared/model-operations.ts + core/media-adapters/* +
    // core/speech-recognition/adapters.ts) ──
    OpenAiEmbeddings,
    OllamaEmbed,
    GeminiEmbed,
    VoyageEmbeddings,
    MinimaxEmbeddings,
    CohereRerank,
    SiliconflowRerank,
    VoyageRerank,
    DashscopeRerank,
    OpenAiImages,
    OpenAiCodexResponsesImage,
    VolcengineImages,
    MinimaxImages,
    DashscopeImages,
    GeminiGenerateContentImage,
    AgnesImages,
    AgnesVideos,
    OpenAiAudioSpeech,
    MinimaxTts,
    DashscopeQwenTts,
    OpenAiAudioTranscriptions,
    MimoChatCompletionsAsr,
    DashscopeQwenAsrChat,
    SystemSpeechRecognition,
}

/// The operation CLASS a family serves (the family↔operation servable
/// matrix is a contract fact: an embedding route onto a chat-completions
/// provider is refused before dispatch, never silently served).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationClass {
    Chat,
    Auxiliary,
    Embedding,
    Rerank,
    Image,
    Video,
    Speech,
    SpeechRecognition,
}

impl ProtocolFamily {
    /// Stable wire/config vocabulary (the `protocol` key of a provider
    /// config entry — mirrors the incumbent `api` identifiers).
    pub fn config_name(self) -> &'static str {
        match self {
            ProtocolFamily::OpenAiCompletions => "openai-completions",
            ProtocolFamily::AnthropicMessages => "anthropic-messages",
            ProtocolFamily::GoogleGenerativeAi => "google-generative-ai",
            ProtocolFamily::OpenAiResponses => "openai-responses",
            ProtocolFamily::OpenAiCodexResponses => "openai-codex-responses",
            ProtocolFamily::VolcengineBigAsr => "volcengine-bigasr",
            ProtocolFamily::SystemSpeech => "system-speech",
            ProtocolFamily::OpenAiEmbeddings => "openai-embeddings",
            ProtocolFamily::OllamaEmbed => "ollama-embed",
            ProtocolFamily::GeminiEmbed => "gemini-embed",
            ProtocolFamily::VoyageEmbeddings => "voyage-embeddings",
            ProtocolFamily::MinimaxEmbeddings => "minimax-embeddings",
            ProtocolFamily::CohereRerank => "cohere-rerank",
            ProtocolFamily::SiliconflowRerank => "siliconflow-rerank",
            ProtocolFamily::VoyageRerank => "voyage-rerank",
            ProtocolFamily::DashscopeRerank => "dashscope-rerank",
            ProtocolFamily::OpenAiImages => "openai-images",
            ProtocolFamily::OpenAiCodexResponsesImage => "openai-codex-responses-image",
            ProtocolFamily::VolcengineImages => "volcengine-images",
            ProtocolFamily::MinimaxImages => "minimax-images",
            ProtocolFamily::DashscopeImages => "dashscope-images",
            ProtocolFamily::GeminiGenerateContentImage => "gemini-generate-content-image",
            ProtocolFamily::AgnesImages => "agnes-images",
            ProtocolFamily::AgnesVideos => "agnes-videos",
            ProtocolFamily::OpenAiAudioSpeech => "openai-audio-speech",
            ProtocolFamily::MinimaxTts => "minimax-tts",
            ProtocolFamily::DashscopeQwenTts => "dashscope-qwen-tts",
            ProtocolFamily::OpenAiAudioTranscriptions => "openai-audio-transcriptions",
            ProtocolFamily::MimoChatCompletionsAsr => "mimo-chat-completions-asr",
            ProtocolFamily::DashscopeQwenAsrChat => "dashscope-qwen-asr-chat",
            ProtocolFamily::SystemSpeechRecognition => "system-speech-recognition",
        }
    }

    /// Parses the config vocabulary (unknown families are a loud config
    /// error, never a guess).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "openai-completions" => ProtocolFamily::OpenAiCompletions,
            "anthropic-messages" => ProtocolFamily::AnthropicMessages,
            "google-generative-ai" => ProtocolFamily::GoogleGenerativeAi,
            "openai-responses" => ProtocolFamily::OpenAiResponses,
            "openai-codex-responses" => ProtocolFamily::OpenAiCodexResponses,
            "volcengine-bigasr" => ProtocolFamily::VolcengineBigAsr,
            "system-speech" => ProtocolFamily::SystemSpeech,
            "openai-embeddings" => ProtocolFamily::OpenAiEmbeddings,
            "ollama-embed" => ProtocolFamily::OllamaEmbed,
            "gemini-embed" => ProtocolFamily::GeminiEmbed,
            "voyage-embeddings" => ProtocolFamily::VoyageEmbeddings,
            "minimax-embeddings" => ProtocolFamily::MinimaxEmbeddings,
            "cohere-rerank" => ProtocolFamily::CohereRerank,
            "siliconflow-rerank" => ProtocolFamily::SiliconflowRerank,
            "voyage-rerank" => ProtocolFamily::VoyageRerank,
            "dashscope-rerank" => ProtocolFamily::DashscopeRerank,
            "openai-images" => ProtocolFamily::OpenAiImages,
            "openai-codex-responses-image" => ProtocolFamily::OpenAiCodexResponsesImage,
            "volcengine-images" => ProtocolFamily::VolcengineImages,
            "minimax-images" => ProtocolFamily::MinimaxImages,
            "dashscope-images" => ProtocolFamily::DashscopeImages,
            "gemini-generate-content-image" => ProtocolFamily::GeminiGenerateContentImage,
            "agnes-images" => ProtocolFamily::AgnesImages,
            "agnes-videos" => ProtocolFamily::AgnesVideos,
            "openai-audio-speech" => ProtocolFamily::OpenAiAudioSpeech,
            "minimax-tts" => ProtocolFamily::MinimaxTts,
            "dashscope-qwen-tts" => ProtocolFamily::DashscopeQwenTts,
            "openai-audio-transcriptions" => ProtocolFamily::OpenAiAudioTranscriptions,
            "mimo-chat-completions-asr" => ProtocolFamily::MimoChatCompletionsAsr,
            "dashscope-qwen-asr-chat" => ProtocolFamily::DashscopeQwenAsrChat,
            "system-speech-recognition" => ProtocolFamily::SystemSpeechRecognition,
            _ => return None,
        })
    }

    /// Every family of the vocabulary (load-time diagnostics, tests).
    pub fn all() -> &'static [ProtocolFamily] {
        &[
            ProtocolFamily::OpenAiCompletions,
            ProtocolFamily::AnthropicMessages,
            ProtocolFamily::GoogleGenerativeAi,
            ProtocolFamily::OpenAiResponses,
            ProtocolFamily::OpenAiCodexResponses,
            ProtocolFamily::VolcengineBigAsr,
            ProtocolFamily::SystemSpeech,
            ProtocolFamily::OpenAiEmbeddings,
            ProtocolFamily::OllamaEmbed,
            ProtocolFamily::GeminiEmbed,
            ProtocolFamily::VoyageEmbeddings,
            ProtocolFamily::MinimaxEmbeddings,
            ProtocolFamily::CohereRerank,
            ProtocolFamily::SiliconflowRerank,
            ProtocolFamily::VoyageRerank,
            ProtocolFamily::DashscopeRerank,
            ProtocolFamily::OpenAiImages,
            ProtocolFamily::OpenAiCodexResponsesImage,
            ProtocolFamily::VolcengineImages,
            ProtocolFamily::MinimaxImages,
            ProtocolFamily::DashscopeImages,
            ProtocolFamily::GeminiGenerateContentImage,
            ProtocolFamily::AgnesImages,
            ProtocolFamily::AgnesVideos,
            ProtocolFamily::OpenAiAudioSpeech,
            ProtocolFamily::MinimaxTts,
            ProtocolFamily::DashscopeQwenTts,
            ProtocolFamily::OpenAiAudioTranscriptions,
            ProtocolFamily::MimoChatCompletionsAsr,
            ProtocolFamily::DashscopeQwenAsrChat,
            ProtocolFamily::SystemSpeechRecognition,
        ]
    }

    /// The operation classes this family can serve (the servable matrix).
    /// The gateway refuses any (operation, family) pair outside this
    /// matrix BEFORE dispatch — an embedding route onto a chat provider
    /// is a loud `OperationUnsupportedByProvider`, never a silent
    /// re-route (C06/C03).
    pub fn serves(self, class: OperationClass) -> bool {
        use ProtocolFamily as F;
        match self {
            F::OpenAiCompletions
            | F::AnthropicMessages
            | F::GoogleGenerativeAi
            | F::OpenAiResponses
            | F::OpenAiCodexResponses => {
                matches!(class, OperationClass::Chat | OperationClass::Auxiliary)
            }
            F::OpenAiEmbeddings
            | F::OllamaEmbed
            | F::GeminiEmbed
            | F::VoyageEmbeddings
            | F::MinimaxEmbeddings => matches!(class, OperationClass::Embedding),
            F::CohereRerank | F::SiliconflowRerank | F::VoyageRerank | F::DashscopeRerank => {
                matches!(class, OperationClass::Rerank)
            }
            F::OpenAiImages
            | F::OpenAiCodexResponsesImage
            | F::VolcengineImages
            | F::MinimaxImages
            | F::DashscopeImages
            | F::GeminiGenerateContentImage
            | F::AgnesImages => matches!(class, OperationClass::Image),
            F::AgnesVideos => matches!(class, OperationClass::Video),
            F::OpenAiAudioSpeech | F::MinimaxTts | F::DashscopeQwenTts | F::SystemSpeech => {
                matches!(class, OperationClass::Speech)
            }
            F::VolcengineBigAsr
            | F::OpenAiAudioTranscriptions
            | F::MimoChatCompletionsAsr
            | F::DashscopeQwenAsrChat
            | F::SystemSpeechRecognition => {
                matches!(class, OperationClass::SpeechRecognition)
            }
        }
    }
}

impl ModelOperation {
    /// The operation class of this operation (the servable matrix's
    /// left-hand side).
    pub fn operation_class(self) -> OperationClass {
        match self {
            ModelOperation::Chat => OperationClass::Chat,
            ModelOperation::Auxiliary(_) => OperationClass::Auxiliary,
            ModelOperation::Embedding => OperationClass::Embedding,
            ModelOperation::Rerank => OperationClass::Rerank,
            ModelOperation::MediaGeneration { kind } => match kind {
                MediaGenerationKind::Image => OperationClass::Image,
                MediaGenerationKind::Video => OperationClass::Video,
                MediaGenerationKind::Speech => OperationClass::Speech,
            },
            ModelOperation::SpeechRecognition => OperationClass::SpeechRecognition,
        }
    }
}

/// The operations the model plane serves (the REAL slots of the incumbent
/// surface — the dead `knowledge` slot of the TS layer is deliberately
/// absent: nothing resolves to it, so nothing can silently serve it).
/// R05-T06: every operation class now has REAL Rust dispatch — `Embedding`/
/// `Rerank`/`MediaGeneration`/`SpeechRecognition` resolve through the
/// gateway's operation bindings and execute through the per-family
/// dialect adapters; a class with no configured binding still fails with
/// the explicit [`ModelGatewayError::RouteNotConfigured`] BEFORE any
/// request leaves the process (C06) — never a silent fallback onto a
/// chat model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelOperation {
    /// The main chat loop (the run driver's turn provider).
    Chat,
    /// One auxiliary slot of the incumbent surface.
    Auxiliary(AuxiliarySlot),
    Embedding,
    Rerank,
    MediaGeneration {
        kind: MediaGenerationKind,
    },
    SpeechRecognition,
}

/// The incumbent's auxiliary model slots (`callText` family): each resolves
/// independently (C07) — a slot without its own binding is a loud
/// unconfigured state, never an accidental share of another slot's route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuxiliarySlot {
    Title,
    Summarize,
    Memory,
    Vision,
    Approval,
    Guard,
}

impl AuxiliarySlot {
    /// The config key of the slot (the `models` section's entry name).
    pub fn config_key(self) -> &'static str {
        match self {
            AuxiliarySlot::Title => "title",
            AuxiliarySlot::Summarize => "summarize",
            AuxiliarySlot::Memory => "memory",
            AuxiliarySlot::Vision => "vision",
            AuxiliarySlot::Approval => "approval",
            AuxiliarySlot::Guard => "guard",
        }
    }

    /// R05-T06 (C09): the host-side purpose→slot mapping. A worker
    /// callback's `purpose` string is a REQUEST, never an authority — the
    /// host maps it to a slot here (after the worker's granted-purpose
    /// whitelist already passed); an unknown purpose maps to NO slot and
    /// is refused, never guessed.
    pub fn from_purpose(purpose: &str) -> Option<Self> {
        match purpose {
            "title" => Some(AuxiliarySlot::Title),
            "summarize" => Some(AuxiliarySlot::Summarize),
            "memory" => Some(AuxiliarySlot::Memory),
            "vision" => Some(AuxiliarySlot::Vision),
            "approval" => Some(AuxiliarySlot::Approval),
            "guard" => Some(AuxiliarySlot::Guard),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaGenerationKind {
    Image,
    Video,
    Speech,
}

impl ModelOperation {
    /// Human/config diagnostic name.
    pub fn describe(self) -> String {
        match self {
            ModelOperation::Chat => "chat".to_string(),
            ModelOperation::Auxiliary(slot) => format!("auxiliary.{}", slot.config_key()),
            ModelOperation::Embedding => "embedding".to_string(),
            ModelOperation::Rerank => "rerank".to_string(),
            ModelOperation::MediaGeneration { kind } => format!(
                "media_generation.{}",
                match kind {
                    MediaGenerationKind::Image => "image",
                    MediaGenerationKind::Video => "video",
                    MediaGenerationKind::Speech => "speech",
                }
            ),
            ModelOperation::SpeechRecognition => "speech_recognition".to_string(),
        }
    }
}

/// Which credential SHAPE a route needs (a reference, never the material —
/// the kernel never holds secrets; the adapter resolves the reference
/// against its own config-side store).
///
/// R05-T02: the vocabulary covers every auth mode the incumbent product
/// uses (`auth_modes_in_use` in R05_BASELINE.json): a bearer API key, a
/// custom auth header, OAuth (material lives in the CredentialService's
/// store, minted by a login flow and rotated by refresh) and the explicit
/// keyless local mode. The CredentialService is the ONLY material exit
/// (C01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredentialAuthKind {
    /// An API key sent as a bearer token. The key itself lives in the
    /// CredentialService (seeded from the provider's config entry).
    ApiKey,
    /// A custom auth header (`{name: value}`) sent verbatim. The material
    /// lives in the CredentialService (seeded from config).
    AuthHeader,
    /// An OAuth bearer token (authorization-code+PKCE or device-code
    /// flows). The material lives ONLY in the CredentialService's store;
    /// config carries the flow descriptor (client id, endpoints, scopes).
    OAuth,
    /// Explicitly keyless (a local no-credential server, ollama-style).
    /// Only legal when the provider config says so EXPLICITLY — never
    /// defaulted onto a remote endpoint (C11).
    None,
}

/// The credential reference of a resolved route (identity + kind; no
/// material).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialReference {
    pub provider: String,
    pub auth: CredentialAuthKind,
}

/// What the gateway resolves a route request to: everything an adapter
/// needs to dispatch ONE model call, snapshotted at resolution time (the
/// `config_generation` pins the snapshot — C05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelRoute {
    pub provider: String,
    pub model: String,
    pub operation: ModelOperation,
    pub protocol: ProtocolFamily,
    /// The base URL the adapter POSTs against (not a secret).
    pub endpoint: String,
    pub credential: CredentialReference,
    /// The gateway's configuration generation at resolution time.
    pub config_generation: u64,
    /// R05-T06: the MiniMax account discriminator (the incumbent model
    /// entry's `groupId`) when the resolved binding declares one — the
    /// minimax dialects build it into the URL query; every other family
    /// ignores it. Not a credential.
    pub group_id: Option<String>,
}

/// A route request: the operation plus an optional explicit provider/model
/// pin (a run-level model override pins BOTH halves of the pair — the
/// provider is never guessed from the model id: two providers may serve
/// the same model id without being the same model, C04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteRequest {
    pub operation: ModelOperation,
    /// Explicit provider id (`None` = the operation's configured default).
    pub provider: Option<String>,
    /// Explicit model id (legal only together with `provider`).
    pub model: Option<String>,
}

impl ModelRouteRequest {
    pub fn for_operation(operation: ModelOperation) -> Self {
        Self {
            operation,
            provider: None,
            model: None,
        }
    }
}

/// Loud resolution failures (none is ever silently downgraded into a
/// fallback route or a swapped model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelGatewayError {
    /// The operation has no configured route and no pin was given (an
    /// unconfigured capability is an explicit state — C03/C07).
    RouteNotConfigured { operation: String },
    /// The named provider does not exist in the configuration.
    UnknownProvider { provider: String },
    /// A model pin without its provider pin (the pair rule).
    ModelPinRequiresProvider { model: String },
    /// The operation class is not servable by this build at all
    /// (embedding / rerank / media / speech at T01) — refused BEFORE any
    /// request leaves the process (C06).
    CapabilityUnsupported { operation: String },
    /// The resolved provider is configured but the operation is outside
    /// what the provider's protocol family can serve (e.g. an embedding
    /// route onto a chat-completions provider) — refused before dispatch.
    OperationUnsupportedByProvider {
        operation: String,
        provider: String,
        protocol: &'static str,
    },
    /// The route resolved but the protocol family has no adapter in this
    /// build — loud, never a silent re-route onto another family.
    ProtocolNotImplemented { protocol: &'static str },
    /// The provider requires a credential its config does not hold (a
    /// remote endpoint without an explicit key is an error — never an
    /// anonymous request; C11).
    MissingCredential { provider: String },
}

impl std::fmt::Display for ModelGatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelGatewayError::RouteNotConfigured { operation } => write!(
                f,
                "no model route configured for operation {operation} (explicit \
                 unconfigured state — never a silent fallback)"
            ),
            ModelGatewayError::UnknownProvider { provider } => {
                write!(f, "unknown provider {provider:?} in model route")
            }
            ModelGatewayError::ModelPinRequiresProvider { model } => write!(
                f,
                "model pin {model:?} without its provider pin: provider+model are one \
                 identity unit (same model id under another provider is another model)"
            ),
            ModelGatewayError::CapabilityUnsupported { operation } => write!(
                f,
                "operation {operation} is not servable by this build; refused before \
                 any request leaves the process (no silent fallback, no model swap)"
            ),
            ModelGatewayError::OperationUnsupportedByProvider {
                operation,
                provider,
                protocol,
            } => write!(
                f,
                "operation {operation} is not servable by provider {provider:?} \
                 (protocol {protocol}); refused before dispatch"
            ),
            ModelGatewayError::ProtocolNotImplemented { protocol } => write!(
                f,
                "protocol family {protocol} has no adapter in this build (R05-T03 \
                 implements the five chat families; media/speech families land with \
                 their own stage); the route resolves but cannot dispatch — never \
                 silently re-routed"
            ),
            ModelGatewayError::MissingCredential { provider } => write!(
                f,
                "provider {provider:?} requires an api-key credential its \
                 configuration does not hold (a remote endpoint is never \
                 contacted anonymously)"
            ),
        }
    }
}

impl std::error::Error for ModelGatewayError {}

/// The unified model-selection + credential-reference resolution point
/// (R05-T01): ONE place decides which provider/model pair serves an
/// operation. Implementations live in `lingxi-adapters` (the config-backed
/// gateway) and are injected by the service composition root. Resolution is
/// a pure snapshot read — no I/O, no network, no clock.
pub trait ModelGatewayPort: Send + Sync {
    /// Resolves the route for one operation. Every failure is a loud
    /// [`ModelGatewayError`]; there is no fallback route and no silent
    /// model substitution.
    fn resolve_route(
        &self,
        request: &ModelRouteRequest,
    ) -> Result<ResolvedModelRoute, ModelGatewayError>;

    /// The CURRENT configuration generation (monotonic per reload; C05).
    fn config_generation(&self) -> u64;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin,
    };

    fn manifest(local_name: &str, origin: ToolOrigin) -> ToolManifest {
        ToolManifest {
            origin,
            local_name: local_name.to_string(),
            display_name: format!("The {local_name} tool"),
            aliases: Vec::new(),
            version: "1.0.0".to_string(),
            description: format!("Does the {local_name} thing"),
            input_schema: ToolSchemaDocument {
                dialect: "json-schema/2020-12".to_string(),
                schema: serde_json::json!({"type": "object"}),
            },
            output_schema: None,
            permission: PermissionContract {
                kind: PermissionKind::Read,
                capability_base: "fs".to_string(),
            },
            availability: Availability::Available,
            timeout_ms: None,
            max_concurrency: None,
            declared_permission: DeclaredPermission::ReadOnly,
            recovery: crate::invocation::ToolRecoveryCapability::CONSERVATIVE,
        }
    }

    #[test]
    fn empty_snapshot_is_generation_zero() {
        let snapshot = ToolDeclarationSnapshot::empty();
        assert_eq!(snapshot.catalog_generation, 0);
        assert!(snapshot.declarations.is_empty());
    }

    #[test]
    fn snapshot_declares_only_callable_resident_targets_at_the_pinned_generation() {
        let registry = ToolRegistry::new();
        let budget = SchemaBudget::default();
        registry
            .register(manifest("read", ToolOrigin::FirstParty), &budget)
            .expect("read registers");
        // Deferred: callable but NOT resident — never declared to the model.
        registry
            .register(
                ToolManifest {
                    availability: Availability::Deferred,
                    ..manifest("deep_research", ToolOrigin::FirstParty)
                },
                &budget,
            )
            .expect("deferred registers");
        // Disabled: not callable at all.
        registry
            .register(
                ToolManifest {
                    availability: Availability::Disabled {
                        reason: "off".to_string(),
                    },
                    ..manifest("write", ToolOrigin::FirstParty)
                },
                &budget,
            )
            .expect("disabled registers");
        let generation = registry.catalog_generation();
        let snapshot = ToolDeclarationSnapshot::from_registry(&registry).expect("snapshot builds");
        assert_eq!(snapshot.catalog_generation, generation);
        let names: Vec<&str> = snapshot
            .declarations
            .iter()
            .map(|d| d.wire_name.as_str())
            .collect();
        assert_eq!(names, vec!["read"], "only the resident callable target");
        assert_eq!(
            snapshot.target_of_wire_name("read"),
            Some("tool:first-party:read")
        );
        assert_eq!(snapshot.target_of_wire_name("write"), None);
    }

    #[test]
    fn wire_name_collisions_resolve_deterministically_or_fail_loudly() {
        let registry = ToolRegistry::new();
        let budget = SchemaBudget::default();
        // Same local name from two origins: allowed by the registry (name
        // ownership is per-source), so the snapshot must disambiguate.
        registry
            .register(manifest("read", ToolOrigin::FirstParty), &budget)
            .expect("first-party registers");
        registry
            .register(
                manifest(
                    "read",
                    ToolOrigin::Plugin {
                        plugin_id: "plug".to_string(),
                    },
                ),
                &budget,
            )
            .expect("plugin registers");
        let snapshot = ToolDeclarationSnapshot::from_registry(&registry).expect("snapshot builds");
        let names: Vec<&str> = snapshot
            .declarations
            .iter()
            .map(|d| d.wire_name.as_str())
            .collect();
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], names[1], "the mapping is unique (C08)");
        // Both are the deterministic namespaced forms of their target ids.
        for declaration in &snapshot.declarations {
            assert_eq!(
                declaration.wire_name,
                sanitize_wire_name(&declaration.target),
                "collision fell back to the namespaced target id form"
            );
            assert_eq!(
                snapshot.target_of_wire_name(&declaration.wire_name),
                Some(declaration.target.as_str()),
                "reverse mapping resolves back to the same target"
            );
        }
    }

    #[test]
    fn wire_name_sanitization_fits_the_strictest_protocol_face() {
        assert_eq!(sanitize_wire_name("read"), "read");
        assert_eq!(
            sanitize_wire_name("tool:first-party:read"),
            "tool_first-party_read"
        );
        assert_eq!(sanitize_wire_name("a b/c.d"), "a_b_c_d");
        let long = "x".repeat(200);
        assert_eq!(sanitize_wire_name(&long).len(), 64);
    }

    #[test]
    fn protocol_family_vocabulary_round_trips() {
        for family in ProtocolFamily::all() {
            assert_eq!(ProtocolFamily::parse(family.config_name()), Some(*family));
        }
        assert_eq!(ProtocolFamily::parse("bogus"), None);
    }
}
