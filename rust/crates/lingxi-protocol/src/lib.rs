//! lingxi-protocol — wire types, errors, IDs and schema anchors for the
//! Lingxi agent protocol.
//!
//! Authority rule (taskbook R01-T02): the serde data types in this crate are
//! the single authoritative source of the new wire protocol. JSON Schema and
//! TypeScript bindings under `contracts/generated/` are generated from these
//! types by `src/bin/lingxi-protocol-gen.rs` and must never be hand-edited.
//!
//! Contract anchors (taskbook `02_目标架构与强制契约.md`):
//! - This crate must never depend on a desktop/webview stack
//!   (tauri/electron/tao/wry/webkit2gtk). Enforced by
//!   `docs/rust-tauri/R01/r01_t01_check_ownership.py`.
//! - IDs are opaque strings; legacy IDs are preserved, never renumbered.
//! - Wire monotonic sequences and any other u64 quantities are carried as
//!   decimal strings so values beyond the JS safe-integer range never lose
//!   precision (§3).
//! - Unknown fields on closed (identity/security/negotiation) structs are
//!   hard errors; unknown event types are preserved verbatim and surfaced,
//!   never silently dropped (R01-T02 step 4, fixed in PROTOCOL_SPEC.md §8).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub mod canon;
pub mod handshake;
pub mod wire;

pub use handshake::{
    negotiate_protocol, ClientHello, ServerHello, WIRE_PROTOCOL_MAX_SUPPORTED,
    WIRE_PROTOCOL_MIN_SUPPORTED,
};
pub use wire::*;

macro_rules! opaque_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        ///
        /// Opaque string identifier. Equality and ordering carry no
        /// semantics; the string form is preserved verbatim across
        /// migrations (legacy IDs are never renumbered). On the wire this is
        /// a plain JSON string (`#[serde(transparent)]`).
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps an existing opaque identifier without renumbering.
            pub fn new(raw: impl Into<String>) -> Self {
                Self(raw.into())
            }

            /// The raw opaque string form.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(raw: String) -> Self {
                Self(raw)
            }
        }
    };
}

opaque_id! {
    /// Identity of one user session (conversation thread).
    SessionId
}
opaque_id! {
    /// Identity of one user task execution (one Run).
    RunId
}
opaque_id! {
    /// Identity of one attempt within a run (retry generation fence).
    AttemptId
}
opaque_id! {
    /// Identity of one model call. Run != ModelCall != ToolCall.
    ModelCallId
}
opaque_id! {
    /// Identity of one tool call.
    ToolCallId
}
opaque_id! {
    /// Identity of one persisted resource (session file / attachment).
    ResourceId
}
opaque_id! {
    /// Identity of one event stream (per session or per run subscription).
    StreamId
}
opaque_id! {
    /// Identity of one protocol event (dedup key; unique per stream).
    EventId
}

/// Monotonic per-stream sequence number.
///
/// Carried on the wire as a **decimal string** so values beyond the
/// JavaScript safe-integer range (2^53 - 1) never lose precision
/// (taskbook `02_目标架构与强制契约.md` §3). In memory it is a `u64`.
/// Deserialization accepts only the decimal-string form; a raw JSON number
/// is rejected so a silent precision-losing path never opens up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Seq(u64);

impl Seq {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// Decimal-string wire form (precision-safe for JS consumers).
    pub fn to_wire_string(self) -> String {
        self.0.to_string()
    }

    /// Parses the decimal-string wire form.
    pub fn from_wire_string(raw: &str) -> Result<Self, ProtocolError> {
        raw.parse::<u64>().map(Self).map_err(|_| ProtocolError {
            code: ErrorCode::InvalidMessage,
            message: format!("invalid wire sequence number: {raw:?}"),
            retryable: false,
            details: None,
        })
    }
}

impl fmt::Display for Seq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for Seq {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_wire_string())
    }
}

impl<'de> Deserialize<'de> for Seq {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Seq::from_wire_string(&raw).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Seq {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Seq".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^(0|[1-9][0-9]*)$",
            "description": "Monotonic sequence as a decimal string. Values may exceed 2^53-1; consumers must not parse this into a JS number when precision matters.",
        })
    }
}

/// serde helper: carries a `u64` field on the wire as a decimal string
/// (same precision rule as [`Seq`]). Usage: `#[serde(with = "u64_wire_string")]`
/// plus `#[schemars(with = "String")]` on the field.
pub mod u64_wire_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse::<u64>()
            .map_err(|_| serde::de::Error::custom(format!("invalid u64 wire string: {raw:?}")))
    }
}

/// serde helper for `Option<u64>` as a decimal string.
pub mod opt_u64_wire_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.serialize_some(&v.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        let raw = Option::<String>::deserialize(deserializer)?;
        raw.map(|s| {
            s.parse::<u64>()
                .map_err(|_| serde::de::Error::custom(format!("invalid u64 wire string: {s:?}")))
        })
        .transpose()
    }
}

/// Lifecycle state of one Run (taskbook `02_目标架构与强制契约.md` §4).
///
/// Terminal states never migrate back to active states. Completion state
/// and delivery quality are separate facts: a run may complete without a
/// final assistant message, and that must not be fabricated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    WaitingApproval,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
    /// Recovery cannot safely continue; needs operator attention.
    InterruptedNeedsAttention,
}

impl RunStatus {
    /// Wire name (stable; external legacy status names are mapped by a
    /// compatibility layer owned by the transport, never by renaming).
    pub fn wire_name(self) -> &'static str {
        match self {
            RunStatus::Queued => "queued",
            RunStatus::Running => "running",
            RunStatus::WaitingApproval => "waiting_approval",
            RunStatus::Cancelling => "cancelling",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::InterruptedNeedsAttention => "interrupted_needs_attention",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunStatus::Cancelled
                | RunStatus::Completed
                | RunStatus::Failed
                | RunStatus::InterruptedNeedsAttention
        )
    }

    pub fn is_active(self) -> bool {
        !self.is_terminal()
    }
}

/// Machine-diagnosable error code for the protocol error envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidMessage,
    VersionIncompatible,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Cancelled,
    BudgetExceeded,
    UpstreamUnavailable,
    /// Pagination cursor is too old or unknown; the client must re-fetch a
    /// snapshot instead of silently missing events (contract §7).
    CursorExpired,
    /// A tool/provider schema arrived in a dialect this boundary does not
    /// support. Never falls back to lenient parsing (contract §5).
    UnknownSchemaDialect,
    Internal,
}

impl ErrorCode {
    pub fn wire_name(self) -> &'static str {
        match self {
            ErrorCode::InvalidMessage => "invalid_message",
            ErrorCode::VersionIncompatible => "version_incompatible",
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Conflict => "conflict",
            ErrorCode::Cancelled => "cancelled",
            ErrorCode::BudgetExceeded => "budget_exceeded",
            ErrorCode::UpstreamUnavailable => "upstream_unavailable",
            ErrorCode::CursorExpired => "cursor_expired",
            ErrorCode::UnknownSchemaDialect => "unknown_schema_dialect",
            ErrorCode::Internal => "internal",
        }
    }
}

/// Structured protocol error envelope.
///
/// `retryable` is advisory for clients; safety-critical retries (external
/// side effects) additionally require idempotency keys or state
/// verification, which is a kernel/service concern, not a wire concern.
/// `details` carries machine-readable diagnostics (e.g. supported version
/// ranges on `version_incompatible`); extensions to an error go there, so
/// the envelope itself stays closed to unknown fields.
// NOTE: not `Eq` — `details` holds arbitrary JSON values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Map<String, serde_json::Value>) -> Self {
        self.details = Some(details);
        self
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.wire_name(), self.message)
    }
}

impl std::error::Error for ProtocolError {}

/// Protocol version triplet mirrored from the incumbent
/// `shared/contract-versions.json` (R00 baseline: PRELOAD_API_VERSION 1,
/// SERVER_PROTOCOL_VERSION 1, DATA_EPOCH 1).
///
/// These are the *legacy* axes. The new wire protocol version negotiated by
/// [`handshake`] is a separate axis (`lingxi.wire` v1); see
/// `docs/rust-tauri/R01/PROTOCOL_SPEC.md` §2 for the relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContractVersions {
    pub preload_api: u32,
    pub server_protocol: u32,
    pub data_epoch: u32,
}

impl ContractVersions {
    /// The versions frozen by the R00 baseline of the incumbent system.
    pub const R00_BASELINE: Self = Self {
        preload_api: 1,
        server_protocol: 1,
        data_epoch: 1,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_opaque_and_preserved() {
        let legacy = RunId::new("legacy-旧-run-0001");
        assert_eq!(legacy.as_str(), "legacy-旧-run-0001");
        assert_eq!(legacy.to_string(), "legacy-旧-run-0001");
        // serde transparent round-trip
        let json = serde_json::to_string(&legacy).unwrap();
        assert_eq!(json, "\"legacy-旧-run-0001\"");
        assert_eq!(serde_json::from_str::<RunId>(&json).unwrap(), legacy);
    }

    #[test]
    fn seq_wire_string_roundtrips_beyond_js_safe_integer() {
        let beyond_js_safe: u64 = (1u64 << 53) + 7;
        let seq = Seq::new(beyond_js_safe);
        let wire = seq.to_wire_string();
        assert_eq!(wire, beyond_js_safe.to_string());
        assert_eq!(Seq::from_wire_string(&wire).unwrap(), seq);
        // serde wire form is a decimal string, not a number
        let json = serde_json::to_string(&seq).unwrap();
        assert_eq!(json, format!("\"{beyond_js_safe}\""));
        assert_eq!(serde_json::from_str::<Seq>(&json).unwrap(), seq);
        // a raw JSON number is rejected (no silent precision path)
        assert!(serde_json::from_str::<Seq>(&beyond_js_safe.to_string()).is_err());
    }

    #[test]
    fn seq_wire_string_rejects_garbage() {
        let err = Seq::from_wire_string("12x").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidMessage);
        assert!(!err.retryable);
    }

    #[test]
    fn terminal_states_are_terminal_and_named_stably() {
        for status in [
            RunStatus::Cancelled,
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::InterruptedNeedsAttention,
        ] {
            assert!(status.is_terminal());
            assert!(!status.is_active());
        }
        assert_eq!(RunStatus::WaitingApproval.wire_name(), "waiting_approval");
        assert_eq!(
            RunStatus::InterruptedNeedsAttention.wire_name(),
            "interrupted_needs_attention"
        );
        let json = serde_json::to_string(&RunStatus::InterruptedNeedsAttention).unwrap();
        assert_eq!(json, "\"interrupted_needs_attention\"");
        assert_eq!(
            serde_json::from_str::<RunStatus>("\"interrupted_needs_attention\"").unwrap(),
            RunStatus::InterruptedNeedsAttention
        );
    }

    #[test]
    fn error_codes_have_stable_wire_names() {
        for (code, wire) in [
            (ErrorCode::CursorExpired, "cursor_expired"),
            (ErrorCode::UnknownSchemaDialect, "unknown_schema_dialect"),
            (ErrorCode::VersionIncompatible, "version_incompatible"),
        ] {
            assert_eq!(code.wire_name(), wire);
            assert_eq!(serde_json::to_string(&code).unwrap(), format!("\"{wire}\""));
        }
    }

    #[test]
    fn protocol_error_rejects_unknown_fields() {
        let err = serde_json::from_str::<ProtocolError>(
            r#"{"code":"internal","message":"x","retryable":false,"surprise":1}"#,
        );
        assert!(
            err.is_err(),
            "closed error envelope must reject unknown fields"
        );
    }

    #[test]
    fn r00_baseline_versions_are_one() {
        assert_eq!(
            ContractVersions::R00_BASELINE,
            ContractVersions {
                preload_api: 1,
                server_protocol: 1,
                data_epoch: 1
            }
        );
    }
}
