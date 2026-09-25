//! Ports: the only way the kernel talks to the outside world.
//!
//! Every dependency of the kernel on infrastructure (persistence, model
//! providers, tool execution, credentials, clock) is declared here as a
//! trait. Implementations live in `lingxi-adapters` and are injected by
//! the `lingxi-service` composition root. This is what makes the
//! dependency rule "kernel never imports adapters" enforceable rather
//! than aspirational.
//!
//! R01-T01 minimal prototype: only the port vocabulary needed to pin the
//! ownership contract is declared. Method surfaces grow in R02+.

use lingxi_protocol::{ModelCallId, ProtocolError, RunId, RunStatus, Seq, SessionId, ToolCallId};

use crate::RunContext;

/// Persistent record of one run's lifecycle, owned by the kernel's
/// RunSupervisor and stored through this port (implementation:
/// `lingxi-adapters` storage module; physical store: the new Rust
/// run/message database).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub status: RunStatus,
    /// Monotonic stream cursor of the last persisted key event.
    pub last_event_seq: Seq,
}

/// Persistence of run/session authority data.
///
/// Finalize must be a single idempotent transaction: committing the
/// terminal status and its key events together, exactly once; repeated
/// requests are idempotent and conflicting results are diagnosed, not
/// silently merged.
pub trait RunStore {
    /// Atomically commits a terminal status transition for `run_id`.
    /// Returns `Ok(false)` when the identical terminal record already
    /// exists (idempotent replay), `Err(Conflict)` when a *different*
    /// terminal record exists.
    fn finalize_run(&self, ctx: &RunContext, record: &RunRecord) -> Result<bool, ProtocolError>;

    fn load_run(&self, run_id: &RunId) -> Result<Option<RunRecord>, ProtocolError>;
}

/// Model provider access. Every request is pinned to
/// principal/run/attempt/modelCall/purpose/provider/model/operation/
/// budget/deadline by the kernel; credentials are resolved server-side
/// through [`CredentialPort`], never by callers and never by workers.
pub trait ModelPort {
    fn complete(
        &self,
        ctx: &RunContext,
        call: ModelCallId,
        request_digest: &str,
    ) -> Result<String, ProtocolError>;
}

/// Execution of one prepared, authorized tool invocation.
///
/// The kernel produces a `PreparedInvocation` internally; it is not
/// forgeable from model output. Cross-worker transfer uses short-lived
/// authorization tickets verified by the host — a JSON field saying
/// `"approved": true` is never an authorization.
pub trait ToolPort {
    fn execute_prepared(
        &self,
        ctx: &RunContext,
        call: ToolCallId,
        prepared_digest: &str,
    ) -> Result<ToolOutcome, ProtocolError>;
}

/// Result of one tool call. `Unknown` is mandatory: an externally
/// completed side effect with no local receipt is never silently
/// retried and never reported as success.
// NOTE: not `Eq` — it carries `ProtocolError`, whose `details` holds
// arbitrary JSON values.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    Success { content_digest: String },
    Failed { error: ProtocolError },
    Cancelled,
    Unknown { reason: String },
}

/// Server-side resolution of provider credentials. Only the service
/// layer may hold credential material; the kernel sees resolved,
/// scoped handles.
pub trait CredentialPort {
    fn resolve_provider_credential(
        &self,
        ctx: &RunContext,
        provider: &str,
    ) -> Result<CredentialHandle, ProtocolError>;
}

/// Opaque, scoped credential handle. Contains no secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialHandle {
    pub handle_id: String,
    pub provider: String,
    pub expires_at_unix_ms: u64,
}
