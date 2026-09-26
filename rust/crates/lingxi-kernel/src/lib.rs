//! lingxi-kernel — domain rules, run state machine and ports (R01-T01
//! minimal prototype).
//!
//! Contract anchors (taskbook `02_目标架构与强制契约.md`, R01-T01):
//! - Depends only on `lingxi-protocol`. It must never depend on
//!   tauri/electron (no desktop coupling) and never on `lingxi-adapters`
//!   (ports are declared here; implementations are injected by
//!   `lingxi-service` at the composition root). Enforced by
//!   `docs/rust-tauri/R01/r01_t01_check_ownership.py` against
//!   `docs/rust-tauri/R01/DEPENDENCY_RULES.json`.
//! - `RunContext` carries identity and budget facts only. No host handle
//!   (no AppHandle, no window reference, no shell surface) may ever be
//!   added to it: hosts and transports reach the kernel exclusively
//!   through the public kernel API.
//! - The run state machine (§4 of the target contract) is owned here and
//!   only here: terminal states never migrate back to active states, and
//!   finalize is a single idempotent transaction.

use lingxi_protocol::{AttemptId, RunId, RunStatus, SessionId};

pub mod ports;

/// Identity and authority facts of one running user task.
///
/// This is the *only* context shape the kernel uses. Hosts (Tauri
/// desktop, CLI, web) and transports construct it through the service
/// layer after authentication; they never appear inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    /// Authenticated principal resolved by the service auth layer.
    /// Never a model-supplied or frontend-supplied claim.
    pub principal: Principal,
    pub session_id: SessionId,
    pub run_id: RunId,
    /// Attempt/generation fence: results from an older attempt must never
    /// overwrite a newer one.
    pub attempt: AttemptId,
    /// Registry generation the tool/model snapshots were taken at.
    pub generation: u64,
}

/// Authenticated principal kinds, mirroring the incumbent server's three
/// real sources (R00 OWNERSHIP_CURRENT §1): local loopback token, paired
/// device, web session. Automation subjects (cron/heartbeat/subagent)
/// carry fixed, narrowed grants issued by the service layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    LocalUser,
    Device {
        device_id: String,
        /// Owning user of the paired device (ownership column target).
        user_id: String,
    },
    WebSession {
        account_id: String,
    },
    /// No user principal; narrowed, fixed permission surface.
    Automation {
        surface: String,
    },
}

impl Principal {
    /// Stable storage vocabulary for ownership columns of the new run
    /// database (kind half). Mirrors the incumbent wire names; changing a
    /// value is a data migration, not a rename.
    pub fn storage_kind(&self) -> &'static str {
        match self {
            Principal::LocalUser => "local_user",
            Principal::Device { .. } => "device",
            Principal::WebSession { .. } => "web_session",
            Principal::Automation { .. } => "automation",
        }
    }

    /// Subject half of the storage ownership key. For the local owner this
    /// is the same constant the service auth layer uses
    /// (`user_local`), so run rows and session rows agree on one owner id.
    pub fn storage_subject(&self) -> String {
        match self {
            Principal::LocalUser => LOCAL_OWNER_SUBJECT.to_string(),
            Principal::Device { user_id, .. } => user_id.clone(),
            Principal::WebSession { account_id } => account_id.clone(),
            Principal::Automation { surface } => surface.to_string(),
        }
    }
}

/// Owning user id of the local owner (single constant shared by the kernel
/// storage vocabulary and the service auth layer's `LOCAL_OWNER_USER_ID`).
pub const LOCAL_OWNER_SUBJECT: &str = "user_local";

/// Error returned for an illegal run-state transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionError {
    pub from: RunStatus,
    pub to: RunStatus,
    pub reason: &'static str,
}

/// The run lifecycle state machine (target contract §4):
///
/// ```text
/// queued → running ↔ waiting_approval
///    │        │              │
///    └────────┴──────────────┴→ cancelling → cancelled
///            │
///            ├→ completed
///            ├→ failed
///            └→ interrupted_needs_attention
/// ```
///
/// Terminal states accept no further transitions. `cancelling` may only
/// resolve to `cancelled`; late events after a terminal state are audit
/// only and never resurrect the run.
pub struct RunStateMachine;

impl RunStateMachine {
    /// Validates a single transition. Returns the target state on success.
    pub fn transition(from: RunStatus, to: RunStatus) -> Result<RunStatus, TransitionError> {
        if from.is_terminal() {
            return Err(TransitionError {
                from,
                to,
                reason: "terminal status never migrates back to active",
            });
        }
        let allowed = match from {
            RunStatus::Queued => matches!(to, RunStatus::Running | RunStatus::Cancelling),
            RunStatus::Running => matches!(
                to,
                RunStatus::WaitingApproval
                    | RunStatus::Cancelling
                    | RunStatus::Completed
                    | RunStatus::Failed
                    | RunStatus::InterruptedNeedsAttention
            ),
            RunStatus::WaitingApproval => matches!(to, RunStatus::Running | RunStatus::Cancelling),
            RunStatus::Cancelling => to == RunStatus::Cancelled,
            terminal => unreachable!("terminal state {terminal:?} handled above"),
        };
        if allowed {
            Ok(to)
        } else {
            Err(TransitionError {
                from,
                to,
                reason: "transition not permitted by the run lifecycle contract",
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_protocol::Seq;

    fn ctx() -> RunContext {
        RunContext {
            principal: Principal::LocalUser,
            session_id: SessionId::new("s-1"),
            run_id: RunId::new("r-1"),
            attempt: AttemptId::new("a-1"),
            generation: 7,
        }
    }

    #[test]
    fn run_context_carries_identity_only() {
        let c = ctx();
        assert_eq!(c.generation, 7);
        assert_eq!(c.principal, Principal::LocalUser);
    }

    #[test]
    fn happy_path_queued_to_completed() {
        let s = RunStateMachine::transition(RunStatus::Queued, RunStatus::Running).unwrap();
        let s = RunStateMachine::transition(s, RunStatus::WaitingApproval).unwrap();
        let s = RunStateMachine::transition(s, RunStatus::Running).unwrap();
        let s = RunStateMachine::transition(s, RunStatus::Completed).unwrap();
        assert!(s.is_terminal());
    }

    #[test]
    fn cancellation_resolves_only_to_cancelled() {
        let s = RunStateMachine::transition(RunStatus::Running, RunStatus::Cancelling).unwrap();
        let s = RunStateMachine::transition(s, RunStatus::Cancelled).unwrap();
        assert_eq!(s, RunStatus::Cancelled);
    }

    #[test]
    fn terminal_states_reject_every_transition() {
        for terminal in [
            RunStatus::Cancelled,
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::InterruptedNeedsAttention,
        ] {
            for target in [
                RunStatus::Queued,
                RunStatus::Running,
                RunStatus::WaitingApproval,
                RunStatus::Cancelling,
                RunStatus::Cancelled,
                RunStatus::Completed,
                RunStatus::Failed,
                RunStatus::InterruptedNeedsAttention,
            ] {
                let err = RunStateMachine::transition(terminal, target).unwrap_err();
                assert_eq!(err.reason, "terminal status never migrates back to active");
            }
        }
    }

    #[test]
    fn queued_cannot_skip_to_completed() {
        assert!(RunStateMachine::transition(RunStatus::Queued, RunStatus::Completed).is_err());
        assert!(RunStateMachine::transition(RunStatus::Queued, RunStatus::Failed).is_err());
    }

    #[test]
    fn cancelling_cannot_be_revoked_back_to_running() {
        let err =
            RunStateMachine::transition(RunStatus::Cancelling, RunStatus::Running).unwrap_err();
        assert_eq!(err.from, RunStatus::Cancelling);
    }

    #[test]
    fn seq_precision_holds_across_kernel_boundary() {
        // Kernel passes wire sequences through without narrowing.
        let seq = Seq::from_wire_string("9007199254740993").unwrap();
        assert_eq!(seq.to_wire_string(), "9007199254740993");
    }
}
