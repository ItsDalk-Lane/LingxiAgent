//! Authoritative wire types of the `lingxi.wire` protocol v1 (R01-T02).
//!
//! Field naming on the wire is camelCase (matching the envelope vocabulary
//! of taskbook `02_目标架构与强制契约.md` §7); enum values are snake_case
//! (matching the incumbent wire names such as `waiting_approval`).
//!
//! Unknown-content policy implemented here (PROTOCOL_SPEC.md §8):
//!   - Every struct is *closed* (`deny_unknown_fields`): an unknown field is
//!     a hard `invalid_message`, never a silent drop.
//!   - Three explicitly open extension points preserve third-party or future
//!     content verbatim: [`EventPayload::Unknown`], [`ContentBlock::Opaque`],
//!     [`ToolSchemaDocument::schema`], plus [`crate::ProtocolError::details`].
//!   - u64 quantities are decimal strings on the wire (see `Seq`).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    opt_u64_wire_string, u64_wire_string, AttemptId, ErrorCode, EventId, ModelCallId,
    ProtocolError, ResourceId, RunId, RunStatus, Seq, SessionId, StreamId, ToolCallId,
};

/// `schemaVersion` of the event envelope fixed by R01-T02.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

// ── Pagination ─────────────────────────────────────────────────────────────

/// Opaque pagination cursor. The server owns its meaning; clients must
/// treat it as an untranslatable token. An expired/unknown cursor fails with
/// `cursor_expired`; the client must re-fetch a snapshot (contract §7).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Cursor(String);

impl Cursor {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Request half of cursor pagination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Response half of cursor pagination. `snapshotSeq` pins the stream
/// position the page was cut at, so a following subscription can join with
/// a consistent cursor boundary (contract §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
    pub snapshot_seq: Seq,
}

/// One entry of a paginated history listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryEntry {
    pub seq: Seq,
    pub event_id: EventId,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

// ── Resources and digests ──────────────────────────────────────────────────

/// Hash of canonical content (protocol §6). `algorithm` is `sha256`;
/// `canonicalization` names the canonical JSON profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentDigest {
    pub algorithm: String,
    pub canonicalization: String,
    pub hex: String,
}

/// Digest of normalized tool arguments — the value approvals bind to
/// (contract §5: approval binds target, principal, run, normalized argument
/// digest, resources, generation, expiry and use count).
pub type ArgsDigest = ContentDigest;

/// Computes the argument digest of an already-normalized argument object.
pub fn digest_arguments(args: &serde_json::Value) -> ArgsDigest {
    ContentDigest {
        algorithm: "sha256".to_string(),
        canonicalization: crate::canon::CANONICALIZATION_ID.to_string(),
        hex: crate::canon::canonical_sha256_hex(args),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    SessionFile,
    Attachment,
    Artifact,
    Export,
}

/// Reference to a real, authorized resource (contract §3: ResourceService
/// owns real files and grants; a model claiming a file exists never creates
/// a resource reference by itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceRef {
    pub resource_id: ResourceId,
    pub kind: ResourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<ContentDigest>,
    /// Byte size as a decimal string (precision rule: all u64 on the wire
    /// are strings).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_u64_wire_string"
    )]
    #[schemars(with = "Option<String>")]
    pub size_bytes: Option<u64>,
}

// ── Content blocks (normalized message model) ──────────────────────────────

/// Canonical assistant phase, aligned with the incumbent
/// `server/assistant-event-normalizer.ts` vocabulary (S09). There is exactly
/// one authoritative MOOD/thinking-tag parsing entry; clients never
/// re-clean phases themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPhase {
    Reasoning,
    Commentary,
    FinalAnswer,
    /// Phase could not be resolved from provider events; surfaced, never
    /// guessed into `final_answer` silently (a diagnostic accompanies it).
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    Text,
    Reasoning,
}

/// One block of normalized message content.
///
/// `opaque` preserves provider-specific state (e.g. reasoning signatures /
/// opaque blocks) **verbatim** — contract §6 forbids flattening such state
/// into ordinary text or dropping it for uniformity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ResourceRef {
        resource: ResourceRef,
    },
    /// Provider-opaque payload, preserved byte-identically.
    Opaque {
        provider: String,
        data: serde_json::Value,
    },
}

/// A normalized assistant/user message as committed to history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NormalizedMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_call_id: Option<ModelCallId>,
}

// ── Model calls (contract §6) ──────────────────────────────────────────────

/// Authenticated principal reference on the wire (identity facts only;
/// credential material never crosses the wire).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipalRef {
    pub kind: PrincipalKind,
    pub principal_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    LocalUser,
    Device,
    WebSession,
    Automation,
}

/// Budget facts pinned to a model request (contract §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Budget {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_u64_wire_string"
    )]
    #[schemars(with = "Option<String>")]
    pub max_tokens: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_u64_wire_string"
    )]
    #[schemars(with = "Option<String>")]
    pub max_cost_micros: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_u64_wire_string"
    )]
    #[schemars(with = "Option<String>")]
    pub deadline_unix_ms: Option<u64>,
}

/// The fixed identity/authority set every model request carries
/// (contract §6). Credentials are resolved server-side; `provider`+`model`
/// are a pair — same model id under a different provider is a different
/// model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelRequest {
    pub principal: PrincipalRef,
    pub run_id: RunId,
    pub attempt: AttemptId,
    pub model_call_id: ModelCallId,
    pub purpose: String,
    pub provider: String,
    pub model: String,
    pub operation: String,
    pub budget: Budget,
    /// Registry/configuration generation the request snapshot was taken at.
    #[serde(with = "u64_wire_string")]
    #[schemars(with = "String")]
    pub config_generation: u64,
}

/// Token usage of one model call; usage and causal trace belong to exactly
/// one modelCall record (contract §3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageRecord {
    #[serde(with = "u64_wire_string")]
    #[schemars(with = "String")]
    pub input_tokens: u64,
    #[serde(with = "u64_wire_string")]
    #[schemars(with = "String")]
    pub output_tokens: u64,
}

// ── Tool calls (contract §5) ───────────────────────────────────────────────

/// Wire descriptor of one tool call as announced at start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolCallDescriptor {
    pub tool_call_id: ToolCallId,
    /// Registry target id at the pinned generation.
    pub target: String,
    pub args_digest: ArgsDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Success,
    Failed,
    Cancelled,
    /// Externally completed side effect with no local receipt; never
    /// silently retried and never reported as success.
    Unknown,
}

/// Structured result of one tool call (contract §5: distinguishes
/// success/failed/cancelled/unknown, carries content blocks, resource refs,
/// truncation and the error with retry semantics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolResultWire {
    pub status: ToolResultStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_refs: Vec<ResourceRef>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

// ── Approvals (contract §5) ────────────────────────────────────────────────

/// Approval request: bound to target, principal, run, normalized argument
/// digest, resources, generation, expiry and use count. A JSON field saying
/// `"approved": true` is never by itself an authorization — the kernel-side
/// `PreparedInvocation` is not forgeable from model output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalRequest {
    pub approval_id: String,
    pub target: String,
    pub principal: PrincipalRef,
    pub run_id: RunId,
    pub attempt: AttemptId,
    pub args_digest: ArgsDigest,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceRef>,
    #[serde(with = "u64_wire_string")]
    #[schemars(with = "String")]
    pub generation: u64,
    #[serde(with = "u64_wire_string")]
    #[schemars(with = "String")]
    pub expires_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_uses: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalDecision {
    pub approval_id: String,
    pub approved: bool,
    /// Who decided (principal id); set by the service auth layer, never by
    /// the model or the frontend.
    pub decided_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ── Events (contract §7) ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunStateChangedPayload {
    pub from: RunStatus,
    pub to: RunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCallStartedPayload {
    pub model_call_id: ModelCallId,
    pub provider: String,
    pub model: String,
    pub operation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCallDeltaPayload {
    pub model_call_id: ModelCallId,
    pub phase: AssistantPhase,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCallCompletedPayload {
    pub model_call_id: ModelCallId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolCallStartedPayload {
    pub tool_call: ToolCallDescriptor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolCallCompletedPayload {
    pub tool_call_id: ToolCallId,
    pub result: ToolResultWire,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalRequestedPayload {
    pub request: ApprovalRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalDecidedPayload {
    pub decision: ApprovalDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantSegmentStartPayload {
    pub segment_id: String,
    pub kind: SegmentKind,
    pub semantic_phase: AssistantPhase,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantSegmentDeltaPayload {
    pub segment_id: String,
    pub delta: String,
    pub semantic_phase: AssistantPhase,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantSegmentEndPayload {
    pub segment_id: String,
    pub semantic_phase: AssistantPhase,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinalMessageCommittedPayload {
    pub message: NormalizedMessage,
}

/// The known event vocabulary of `lingxi.wire` v1. Closed per variant, but
/// the vocabulary itself is open through [`EventPayload::Unknown`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KnownEventPayload {
    RunStateChanged(RunStateChangedPayload),
    ModelCallStarted(ModelCallStartedPayload),
    ModelCallDelta(ModelCallDeltaPayload),
    ModelCallCompleted(ModelCallCompletedPayload),
    ToolCallStarted(ToolCallStartedPayload),
    ToolCallCompleted(ToolCallCompletedPayload),
    ApprovalRequested(ApprovalRequestedPayload),
    ApprovalDecided(ApprovalDecidedPayload),
    AssistantSegmentStart(AssistantSegmentStartPayload),
    AssistantSegmentDelta(AssistantSegmentDeltaPayload),
    AssistantSegmentEnd(AssistantSegmentEndPayload),
    FinalMessageCommitted(FinalMessageCommittedPayload),
}

/// Wire tags of the known event vocabulary.
pub const KNOWN_EVENT_TYPES: &[&str] = &[
    "run_state_changed",
    "model_call_started",
    "model_call_delta",
    "model_call_completed",
    "tool_call_started",
    "tool_call_completed",
    "approval_requested",
    "approval_decided",
    "assistant_segment_start",
    "assistant_segment_delta",
    "assistant_segment_end",
    "final_message_committed",
];

/// An event whose type is not in the known vocabulary. The **entire raw
/// payload is preserved verbatim** (including the `type` key) and must be
/// surfaced to the consumer as unsupported — never silently dropped,
/// because an unseen event may carry permission or history semantics
/// (R01-T02 step 4).
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownEventPayload {
    pub event_type: String,
    /// The complete raw payload object, `type` key included.
    pub raw: serde_json::Map<String, serde_json::Value>,
}

/// Event payload: a known, fully typed variant, or a preserved unknown one.
#[derive(Debug, Clone, PartialEq)]
pub enum EventPayload {
    Known(KnownEventPayload),
    Unknown(UnknownEventPayload),
}

impl EventPayload {
    /// The wire `type` tag of this payload.
    pub fn event_type(&self) -> &str {
        match self {
            EventPayload::Known(known) => match known {
                KnownEventPayload::RunStateChanged(_) => "run_state_changed",
                KnownEventPayload::ModelCallStarted(_) => "model_call_started",
                KnownEventPayload::ModelCallDelta(_) => "model_call_delta",
                KnownEventPayload::ModelCallCompleted(_) => "model_call_completed",
                KnownEventPayload::ToolCallStarted(_) => "tool_call_started",
                KnownEventPayload::ToolCallCompleted(_) => "tool_call_completed",
                KnownEventPayload::ApprovalRequested(_) => "approval_requested",
                KnownEventPayload::ApprovalDecided(_) => "approval_decided",
                KnownEventPayload::AssistantSegmentStart(_) => "assistant_segment_start",
                KnownEventPayload::AssistantSegmentDelta(_) => "assistant_segment_delta",
                KnownEventPayload::AssistantSegmentEnd(_) => "assistant_segment_end",
                KnownEventPayload::FinalMessageCommitted(_) => "final_message_committed",
            },
            EventPayload::Unknown(u) => &u.event_type,
        }
    }
}

impl Serialize for EventPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            EventPayload::Known(known) => known.serialize(serializer),
            // Verbatim re-serialization: nothing dropped, nothing reordered
            // semantically (key order is normalized by the canonical layer).
            EventPayload::Unknown(u) => u.raw.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for EventPayload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let obj = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("event payload must be a JSON object"))?;
        let event_type = obj.get("type").and_then(|t| t.as_str()).ok_or_else(|| {
            serde::de::Error::custom("event payload requires a string \"type\" tag")
        })?;
        if KNOWN_EVENT_TYPES.contains(&event_type) {
            let known = serde_json::from_value::<KnownEventPayload>(value)
                .map_err(serde::de::Error::custom)?;
            return Ok(EventPayload::Known(known));
        }
        Ok(EventPayload::Unknown(UnknownEventPayload {
            event_type: event_type.to_string(),
            raw: obj.clone(),
        }))
    }
}

impl JsonSchema for EventPayload {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "EventPayload".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let known = generator.subschema_for::<KnownEventPayload>();
        let mut known_value = known.to_value();
        // Inline the $ref target's anyOf so the union with the open fallback
        // stays legible; keep the $ref if the known schema is not an anyOf.
        let known_branch =
            if let Some(any_of) = known_value.as_object_mut().and_then(|o| o.remove("anyOf")) {
                serde_json::json!({ "anyOf": any_of })
            } else {
                known_value
            };
        schemars::json_schema!({
            "anyOf": [
                known_branch,
                {
                    "type": "object",
                    "required": ["type"],
                    "properties": { "type": { "type": "string" } },
                    "additionalProperties": true,
                    "description": "Unknown event type: preserved verbatim and surfaced as unsupported; never silently dropped.",
                }
            ]
        })
    }
}

/// The event envelope (contract §7 minimal fields:
/// schemaVersion/eventId/streamId/seq/sessionId/runId/attempt/eventType/
/// payload). `eventType` is denormalized from the payload tag for routing;
/// deserialization rejects a mismatch instead of guessing which one is
/// authoritative.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventEnvelope {
    pub schema_version: u32,
    pub event_id: EventId,
    pub stream_id: StreamId,
    pub seq: Seq,
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<AttemptId>,
    pub event_type: String,
    pub payload: EventPayload,
}

impl EventEnvelope {
    pub fn new(
        event_id: EventId,
        stream_id: StreamId,
        seq: Seq,
        session_id: SessionId,
        run_id: Option<RunId>,
        attempt: Option<AttemptId>,
        payload: EventPayload,
    ) -> Self {
        let event_type = payload.event_type().to_string();
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            event_id,
            stream_id,
            seq,
            session_id,
            run_id,
            attempt,
            event_type,
            payload,
        }
    }
}

impl<'de> Deserialize<'de> for EventEnvelope {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Shadow {
            schema_version: u32,
            event_id: EventId,
            stream_id: StreamId,
            seq: Seq,
            session_id: SessionId,
            #[serde(default)]
            run_id: Option<RunId>,
            #[serde(default)]
            attempt: Option<AttemptId>,
            event_type: String,
            payload: EventPayload,
        }
        let s = Shadow::deserialize(deserializer)?;
        if s.payload.event_type() != s.event_type {
            return Err(serde::de::Error::custom(format!(
                "eventType {:?} does not match payload type {:?}",
                s.event_type,
                s.payload.event_type()
            )));
        }
        if s.schema_version != EVENT_SCHEMA_VERSION {
            return Err(serde::de::Error::custom(format!(
                "unsupported schemaVersion {}; this build speaks {}",
                s.schema_version, EVENT_SCHEMA_VERSION
            )));
        }
        Ok(Self {
            schema_version: s.schema_version,
            event_id: s.event_id,
            stream_id: s.stream_id,
            seq: s.seq,
            session_id: s.session_id,
            run_id: s.run_id,
            attempt: s.attempt,
            event_type: s.event_type,
            payload: s.payload,
        })
    }
}

// ── Third-party tool schemas (boundary rule) ───────────────────────────────

/// Schema dialects this boundary can validate. Anything else is an explicit
/// `unknown_schema_dialect` error — contract §5 forbids leniently treating
/// an unrecognized dialect as JSON Schema.
pub const SUPPORTED_SCHEMA_DIALECTS: &[&str] = &["json-schema/2020-12", "json-schema/draft-07"];

/// A third-party tool schema document. `schema` is preserved **verbatim**
/// (never normalized, merged or pruned); the boundary validates the dialect
/// and, for supported dialects, validates values against the schema at the
/// invocation boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolSchemaDocument {
    pub dialect: String,
    pub schema: serde_json::Value,
}

/// Boundary check for third-party schema dialects.
pub fn check_schema_dialect(dialect: &str) -> Result<(), ProtocolError> {
    if SUPPORTED_SCHEMA_DIALECTS.contains(&dialect) {
        return Ok(());
    }
    Err(ProtocolError::new(
        ErrorCode::UnknownSchemaDialect,
        format!("unsupported schema dialect {dialect:?}; supported: {SUPPORTED_SCHEMA_DIALECTS:?}"),
        false,
    )
    .with_details(serde_json::Map::from_iter([(
        "dialect".into(),
        dialect.into(),
    )])))
}

/// Convenience: the full error body used on handshake/negotiation failures
/// over HTTP and WS. Alias kept for readability at call sites.
pub type ErrorBody = ProtocolError;

/// Unused-import silencer for the helpers re-exported through this module's
/// types (kept private).
#[allow(unused)]
fn _assert_helpers_linked(m: &BTreeMap<String, String>) {
    let _ = m.len();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canon::canonical_string;

    fn ids() -> (SessionId, RunId, AttemptId, StreamId) {
        (
            SessionId::new("s-1"),
            RunId::new("r-1"),
            AttemptId::new("a-1"),
            StreamId::new("stream-1"),
        )
    }

    #[test]
    fn envelope_roundtrips_and_validates_event_type() {
        let (sid, rid, aid, stid) = ids();
        let env = EventEnvelope::new(
            EventId::new("evt-1"),
            stid,
            Seq::new((1u64 << 53) + 7),
            sid,
            Some(rid),
            Some(aid),
            EventPayload::Known(KnownEventPayload::AssistantSegmentDelta(
                AssistantSegmentDeltaPayload {
                    segment_id: "seg-1".into(),
                    delta: "你好，灵犀".into(),
                    semantic_phase: AssistantPhase::FinalAnswer,
                },
            )),
        );
        let json = canonical_string(&env);
        let back: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, env);

        // eventType mismatch is a hard error, not a guess
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("eventType".into(), "run_state_changed".into());
        assert!(serde_json::from_value::<EventEnvelope>(v).is_err());

        // wrong schemaVersion is a hard error
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("schemaVersion".into(), 2.into());
        assert!(serde_json::from_value::<EventEnvelope>(v).is_err());

        // unknown top-level field is a hard error
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("smuggled".into(), true.into());
        assert!(serde_json::from_value::<EventEnvelope>(v).is_err());
    }

    #[test]
    fn unknown_event_payload_is_preserved_verbatim() {
        let raw = serde_json::json!({
            "type": "future_recall_started",
            "permissionHint": "memory.write",
            "payload": {"nested": [1, 2, 3], "zh": "未来事件"},
        });
        let parsed: EventPayload = serde_json::from_value(raw.clone()).unwrap();
        match &parsed {
            EventPayload::Unknown(u) => {
                assert_eq!(u.event_type, "future_recall_started");
                assert_eq!(serde_json::Value::Object(u.raw.clone()), raw);
            }
            _ => panic!("must parse as Unknown"),
        }
        // re-serialization is verbatim
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            raw,
            "unknown payload must survive a serialize round-trip byte-identically"
        );
    }

    #[test]
    fn closed_payload_variants_reject_unknown_fields() {
        let raw = serde_json::json!({
            "type": "model_call_delta",
            "modelCallId": "mc-1",
            "phase": "final_answer",
            "delta": "x",
            "smuggled": true
        });
        assert!(serde_json::from_value::<EventPayload>(raw).is_err());
    }

    #[test]
    fn dialect_check_is_explicit() {
        assert!(check_schema_dialect("json-schema/2020-12").is_ok());
        let err = check_schema_dialect("typescript-tcomb").unwrap_err();
        assert_eq!(err.code, ErrorCode::UnknownSchemaDialect);
    }

    #[test]
    fn u64_fields_are_decimal_strings() {
        let u = UsageRecord {
            input_tokens: (1u64 << 53) + 7,
            output_tokens: 42,
        };
        let json = serde_json::to_value(&u).unwrap();
        assert_eq!(json["inputTokens"], "9007199254740999");
        assert_eq!(json["outputTokens"], "42");
        let back: UsageRecord = serde_json::from_value(json).unwrap();
        assert_eq!(back, u);
    }
}
