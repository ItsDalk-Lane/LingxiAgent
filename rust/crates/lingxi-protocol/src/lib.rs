//! lingxi-protocol — wire types, errors, IDs and schema anchors for the
//! Lingxi agent protocol (R01-T01 minimal prototype).
//!
//! Contract anchors (taskbook `02_目标架构与强制契约.md`):
//! - This crate must never depend on a desktop/webview stack
//!   (tauri/electron/tao/wry/webkit2gtk). Enforced by
//!   `docs/rust-tauri/R01/r01_t01_check_ownership.py`.
//! - IDs are opaque strings; legacy IDs are preserved, never renumbered.
//! - Wire monotonic sequences that could exceed the JS safe-integer range
//!   are carried as decimal strings, not numbers.
//!
//! The authoritative serde wire schema, JSON Schema export and TS binding
//! generation are R01-T02 scope; this prototype fixes only the identity
//! vocabulary, the run-state vocabulary and the error envelope so that
//! `lingxi-kernel` has a real, compilable protocol surface to build on.

use std::fmt;

macro_rules! opaque_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        ///
        /// Opaque string identifier. Equality and ordering carry no
        /// semantics; the string form is preserved verbatim across
        /// migrations (legacy IDs are never renumbered).
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

/// Monotonic per-stream sequence number.
///
/// Carried on the wire as a **decimal string** so values beyond the
/// JavaScript safe-integer range (2^53 - 1) never lose precision
/// (taskbook `02_目标架构与强制契约.md` §3). In memory it is a `u64`.
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
        })
    }
}

impl fmt::Display for Seq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lifecycle state of one Run (taskbook `02_目标架构与强制契约.md` §4).
///
/// Terminal states never migrate back to active states. Completion state
/// and delivery quality are separate facts: a run may complete without a
/// final assistant message, and that must not be fabricated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            ErrorCode::Internal => "internal",
        }
    }
}

/// Structured protocol error envelope.
///
/// `retryable` is advisory for clients; safety-critical retries (external
/// side effects) additionally require idempotency keys or state
/// verification, which is a kernel/service concern, not a wire concern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.wire_name(), self.message)
    }
}

impl std::error::Error for ProtocolError {}

/// Protocol version triplet mirrored from the incumbent
/// `shared/contract-versions.json` (R00 baseline: PRELOAD_API_VERSION 1,
/// SERVER_PROTOCOL_VERSION 1, DATA_EPOCH 1). Target-side negotiation rules
/// are fixed by R01-T02; this type only anchors the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    }

    #[test]
    fn seq_wire_string_roundtrips_beyond_js_safe_integer() {
        let beyond_js_safe: u64 = (1u64 << 53) + 7;
        let seq = Seq::new(beyond_js_safe);
        let wire = seq.to_wire_string();
        assert_eq!(wire, beyond_js_safe.to_string());
        assert_eq!(Seq::from_wire_string(&wire).unwrap(), seq);
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
