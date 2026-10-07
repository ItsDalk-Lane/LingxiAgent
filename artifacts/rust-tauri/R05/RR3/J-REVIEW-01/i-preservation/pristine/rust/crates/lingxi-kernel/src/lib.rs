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

use lingxi_protocol::{AttemptId, NormalizedMessage, RunId, RunStatus, SessionId, ToolCallId};

pub mod invocation;
pub mod model_exchange;
pub mod ports;
pub mod recovery;
pub mod subagent;
pub mod toolcatalog;
pub mod usage;

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

/// Attempt identity of one run (R03-T01 step 2): `{run_id}#a{sequence}`.
///
/// The sequence starts at 1 and increments PER RETRY on the SAME run — a
/// provider reconnect or transient failure mints a new attempt, never a new
/// run id. `RunId` is fixed when the run is created; Session != Run !=
/// Attempt != ModelCall != ToolCall are distinct identity layers.
pub fn attempt_id(run_id: &RunId, sequence: u32) -> AttemptId {
    AttemptId::new(format!("{run_id}#a{sequence}"))
}

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

    /// The finalize decision function (R03-T01 step 3). Terminal state goes
    /// through exactly one finalize path; this pure function is that path's
    /// decision core, consumed by the storage transaction (and pre-checked
    /// by the run driver).
    ///
    /// - `current` is the run's status as durably stored.
    /// - `stored` is the already-persisted settlement when `current` is
    ///   terminal (`None` otherwise).
    /// - `requested` is the settlement being submitted now.
    ///
    /// Semantics (target contract §4: "finalize 只允许一次事务提交，重复请求
    /// 幂等，冲突结果诊断"):
    /// - First finalize (non-terminal `current`): the requested status must
    ///   be terminal AND a legal transition — otherwise rejected (loudly).
    /// - Completely identical re-submission (terminal `current`, stored
    ///   settlement == requested): [`FinalizeVerdict::IdempotentReplay`] —
    ///   the run had already been settled exactly this way; nothing is
    ///   written again, no settlement is counted twice.
    /// - Anything else against a terminal run (different status, different
    ///   reason, different final message): [`FinalizeRejection::Conflict`].
    ///   Note [`Self::transition`] alone would reject EVERY terminal→*
    ///   request; finalize deliberately recognizes the exactly-identical
    ///   duplicate and nothing more.
    pub fn finalize(
        current: RunStatus,
        stored: Option<&FinalizeSettlement>,
        requested: &FinalizeSettlement,
    ) -> Result<FinalizeVerdict, FinalizeRejection> {
        if let Some(stored) = stored {
            if stored.status != current {
                return Err(FinalizeRejection {
                    current,
                    requested: requested.status,
                    kind: FinalizeRejectionKind::CorruptSettlement,
                    detail: format!(
                        "stored settlement claims status {} while the run row holds {}; \
                         the durable state disagrees with itself",
                        stored.status.wire_name(),
                        current.wire_name()
                    ),
                });
            }
            if stored == requested {
                return Ok(FinalizeVerdict::IdempotentReplay { terminal: current });
            }
            return Err(FinalizeRejection {
                current,
                requested: requested.status,
                kind: FinalizeRejectionKind::Conflict,
                detail: format!(
                    "run is already terminal as {} (settled reason {:?}, final message {}); \
                     refusing the conflicting re-finalize to {} (reason {:?}, final message {})",
                    stored.status.wire_name(),
                    stored.reason,
                    if stored.final_message.is_some() {
                        "present"
                    } else {
                        "absent"
                    },
                    requested.status.wire_name(),
                    requested.reason,
                    if requested.final_message.is_some() {
                        "present"
                    } else {
                        "absent"
                    }
                ),
            });
        }
        if !requested.status.is_terminal() {
            return Err(FinalizeRejection {
                current,
                requested: requested.status,
                kind: FinalizeRejectionKind::NotTerminal,
                detail: format!(
                    "finalize requires a terminal status, got {}",
                    requested.status.wire_name()
                ),
            });
        }
        match Self::transition(current, requested.status) {
            Ok(to) => Ok(FinalizeVerdict::Commit { from: current, to }),
            Err(err) => Err(FinalizeRejection {
                current,
                requested: requested.status,
                kind: FinalizeRejectionKind::IllegalTransition,
                detail: err.reason.to_string(),
            }),
        }
    }
}

/// The full settlement facts one finalize submits (R03-T01 step 3). Two
/// submissions are "completely identical" only when status, reason AND
/// final message all agree — a same-status re-finalize with a different
/// final message is a conflict, not a replay.
// NOTE: not `Eq` — `NormalizedMessage` carries provider-opaque JSON values.
#[derive(Debug, Clone, PartialEq)]
pub struct FinalizeSettlement {
    /// MUST be terminal; [`RunStateMachine::finalize`] rejects otherwise.
    pub status: RunStatus,
    /// Stable outcome vocabulary (see [`RunFinish::terminal_reason`]).
    pub reason: Option<String>,
    /// Final normalized assistant message. `Some` ONLY when the run truly
    /// produced one — never fabricated (contract 02 §4).
    pub final_message: Option<NormalizedMessage>,
}

/// Verdict of [`RunStateMachine::finalize`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizeVerdict {
    /// Legal first finalize: commit the transition `from → to`.
    Commit { from: RunStatus, to: RunStatus },
    /// Completely identical duplicate of the already-durable settlement:
    /// nothing is written or counted again (events may still be returned so
    /// late subscribers can catch up from the store).
    IdempotentReplay { terminal: RunStatus },
}

/// Machine-diagnosable rejection kinds of a finalize request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizeRejectionKind {
    /// A different settlement already owns this run's terminal state.
    Conflict,
    /// Finalize submitted a non-terminal status.
    NotTerminal,
    /// The transition is illegal from the current (non-terminal) status.
    IllegalTransition,
    /// Stored state disagrees with itself (run row vs settlement row).
    CorruptSettlement,
}

/// Rejection of a finalize request, carrying a diagnosable detail string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizeRejection {
    pub current: RunStatus,
    pub requested: RunStatus,
    pub kind: FinalizeRejectionKind,
    pub detail: String,
}

impl FinalizeRejection {
    /// Storage-layer mapping: a conflict against an existing terminal is a
    /// [`lingxi_protocol::ErrorCode::Conflict`]-class fact; pre-terminal
    /// rejections are invalid requests.
    pub fn is_conflict(&self) -> bool {
        self.kind == FinalizeRejectionKind::Conflict
    }
}

impl std::fmt::Display for FinalizeRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "finalize rejected ({} -> {}): {}",
            self.current.wire_name(),
            self.requested.wire_name(),
            self.detail
        )
    }
}

/// Why a run completed WITHOUT a final assistant message (R03-T01 step 4).
/// Completion state and delivery quality are separate facts: each cause is
/// explicit and none of them fabricates a final answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoFinalCause {
    /// The provider finished with zero usable content.
    EmptyReply,
    /// Only process content (tool calls / continuation turns) was produced;
    /// no final answer exists.
    ProcessOnly,
    /// At least one tool call failed or was cancelled; nothing is invented
    /// as an answer on top of that.
    ToolPartialFailure,
    /// No provider is configured (real providers arrive with R05); the run
    /// honestly completes with no model content instead of faking a reply.
    NoProviderConfigured,
}

impl NoFinalCause {
    pub fn cause_code(&self) -> &'static str {
        match self {
            NoFinalCause::EmptyReply => "empty_reply",
            NoFinalCause::ProcessOnly => "process_only",
            NoFinalCause::ToolPartialFailure => "tool_partial_failure",
            NoFinalCause::NoProviderConfigured => "no_provider_configured",
        }
    }
}

/// Which admission lane a run could not enter (R03-T02: global/agent/session
/// model & tool quotas). Part of the failure cause so a quota-exhausted run
/// settles LOUDLY through the single finalize path — never as a fake success
/// and never conflated with a provider failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuotaResource {
    /// Concurrent model-call admission slots.
    Model,
    /// Concurrent tool-call admission slots.
    Tool,
}

impl QuotaResource {
    pub fn code(&self) -> &'static str {
        match self {
            QuotaResource::Model => "model",
            QuotaResource::Tool => "tool",
        }
    }
}

/// Why a run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureCause {
    /// The provider call failed and no retry attempt remains (or the
    /// failure was not retryable). Carries the provider error code.
    ProviderFailed { code: String, retryable: bool },
    /// The per-run model-turn budget was exhausted before a final answer.
    TurnBudgetExceeded { max_turns: u32 },
    /// The provider requested tool calls while no tool executor is wired
    /// (loud failure — never silently skipping the tools).
    ToolExecutorUnavailable,
    /// A global/agent/session admission quota could not be acquired within
    /// its bounded wait (R03-T02): the run fails loudly instead of waiting
    /// without bound or pretending the call happened. `resource` names the
    /// lane family; the layer (global/agent/session) rides in the message.
    QuotaExhausted { resource: QuotaResource },
}

impl FailureCause {
    pub fn cause_code(&self) -> &'static str {
        match self {
            FailureCause::ProviderFailed { .. } => "provider_error",
            FailureCause::TurnBudgetExceeded { .. } => "turn_budget_exceeded",
            FailureCause::ToolExecutorUnavailable => "tool_executor_unavailable",
            FailureCause::QuotaExhausted { .. } => "quota_exhausted",
        }
    }

    /// Full stable reason segment (the caller prefixes the status), e.g.
    /// `provider_error`, `quota_exhausted.tool`. Quota failures carry the
    /// resource so `runs.terminal_reason` distinguishes model-slot from
    /// tool-slot exhaustion diagnostically.
    pub fn reason_segment(&self) -> String {
        match self {
            FailureCause::QuotaExhausted { resource } => {
                format!("quota_exhausted.{}", resource.code())
            }
            other => other.cause_code().to_string(),
        }
    }
}

/// The run outcome contract (R03-T01 step 4 / deliverable "运行结果契约"):
/// the closed vocabulary of ways one run ends. Empty reply, process-only
/// content, tool partial failure and cancellation each have their own
/// explicit outcome; a final assistant message exists ONLY when the
/// provider truly produced one.
#[derive(Debug, Clone, PartialEq)]
pub enum RunFinish {
    /// Completed WITH a committed final assistant message (same-transaction
    /// message + `final_message_committed` event).
    CompletedWithFinal { message: NormalizedMessage },
    /// Completed WITHOUT a final assistant message; the cause names why.
    CompletedWithoutFinal { cause: NoFinalCause },
    /// Failed: no result is delivered and the cause is explicit.
    Failed { cause: FailureCause },
    /// Cancelled by an authorized requester (the cancellation TREE is
    /// R03-T03; the outcome contract and terminal path are defined here so
    /// every cancellation settles through the same single finalize).
    Cancelled { detail: String },
    /// Recovery cannot safely continue (R03-T07 semantics). Constructible
    /// here so recovery also goes through the same finalize path.
    InterruptedNeedsAttention { detail: String },
}

impl RunFinish {
    /// Terminal status this finish maps to (always terminal by contract).
    pub fn status(&self) -> RunStatus {
        match self {
            RunFinish::CompletedWithFinal { .. } | RunFinish::CompletedWithoutFinal { .. } => {
                RunStatus::Completed
            }
            RunFinish::Failed { .. } => RunStatus::Failed,
            RunFinish::Cancelled { .. } => RunStatus::Cancelled,
            RunFinish::InterruptedNeedsAttention { .. } => RunStatus::InterruptedNeedsAttention,
        }
    }

    /// Stable terminal-reason vocabulary persisted with the run row:
    /// `{status}.{class}.{cause}`, e.g. `completed.no_final.empty_reply`,
    /// `failed.provider_error`, `cancelled.requested`.
    pub fn terminal_reason(&self) -> String {
        match self {
            RunFinish::CompletedWithFinal { .. } => "completed.with_final".to_string(),
            RunFinish::CompletedWithoutFinal { cause } => {
                format!("completed.no_final.{}", cause.cause_code())
            }
            RunFinish::Failed { cause } => format!("failed.{}", cause.reason_segment()),
            RunFinish::Cancelled { .. } => "cancelled.requested".to_string(),
            RunFinish::InterruptedNeedsAttention { .. } => {
                "interrupted_needs_attention.recovery_unsafe".to_string()
            }
        }
    }

    /// The final assistant message, if and only if the run truly produced
    /// one. No variant fabricates a message.
    pub fn final_message(&self) -> Option<&NormalizedMessage> {
        match self {
            RunFinish::CompletedWithFinal { message } => Some(message),
            _ => None,
        }
    }

    /// The settlement this finish submits through the single finalize path.
    pub fn settlement(&self) -> FinalizeSettlement {
        FinalizeSettlement {
            status: self.status(),
            reason: Some(self.terminal_reason()),
            final_message: match self {
                RunFinish::CompletedWithFinal { message } => Some(message.clone()),
                _ => None,
            },
        }
    }
}

/// Mints the identity of one model call (R03-T01 step 2). ModelCall identity
/// is INDEPENDENT of run/attempt identity: `{run_id}-mc{sequence}` — a
/// provider reconnect reuses the same run (new attempt), and each model call
/// within it still gets its own id.
pub fn model_call_id(run_id: &RunId, sequence: u32) -> lingxi_protocol::ModelCallId {
    lingxi_protocol::ModelCallId::new(format!("{run_id}-mc{sequence:04}"))
}

/// Mints the identity of one tool call: `{run_id}-tc{sequence}`.
pub fn tool_call_id(run_id: &RunId, sequence: u32) -> ToolCallId {
    ToolCallId::new(format!("{run_id}-tc{sequence:04}"))
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

    // ── R03-T01: finalize decision function ────────────────────────────────

    fn final_message(text: &str) -> NormalizedMessage {
        NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![lingxi_protocol::ContentBlock::Text {
                text: text.to_string(),
            }],
            model_call_id: Some(lingxi_protocol::ModelCallId::new("mc-1")),
        }
    }

    fn settled_completed(text: Option<&str>) -> FinalizeSettlement {
        FinalizeSettlement {
            status: RunStatus::Completed,
            reason: Some("completed.with_final".to_string()),
            final_message: text.map(final_message),
        }
    }

    #[test]
    fn finalize_from_running_commits_legal_terminal() {
        // From `running` the direct terminals are completed / failed /
        // interrupted. `cancelled` is NOT reachable directly: cancellation
        // goes running -> cancelling -> cancelled (two-phase, contract §4).
        for terminal in [
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::InterruptedNeedsAttention,
        ] {
            let request = FinalizeSettlement {
                status: terminal,
                reason: None,
                final_message: None,
            };
            match RunStateMachine::finalize(RunStatus::Running, None, &request).unwrap() {
                FinalizeVerdict::Commit { from, to } => {
                    assert_eq!(from, RunStatus::Running);
                    assert_eq!(to, terminal);
                }
                other => panic!("expected Commit, got {other:?}"),
            }
        }
        // Cancelling resolves only to cancelled (the second half of the
        // cancellation phase settles through the same finalize path).
        let cancelled = FinalizeSettlement {
            status: RunStatus::Cancelled,
            reason: None,
            final_message: None,
        };
        match RunStateMachine::finalize(RunStatus::Cancelling, None, &cancelled).unwrap() {
            FinalizeVerdict::Commit { from, to } => {
                assert_eq!(from, RunStatus::Cancelling);
                assert_eq!(to, RunStatus::Cancelled);
            }
            other => panic!("expected Commit, got {other:?}"),
        }
        // running -> cancelled directly is rejected (skips the phase).
        let err = RunStateMachine::finalize(RunStatus::Running, None, &cancelled).unwrap_err();
        assert_eq!(err.kind, FinalizeRejectionKind::IllegalTransition);
    }

    #[test]
    fn finalize_rejects_non_terminal_request() {
        let request = FinalizeSettlement {
            status: RunStatus::Running,
            reason: None,
            final_message: None,
        };
        let err = RunStateMachine::finalize(RunStatus::Running, None, &request).unwrap_err();
        assert_eq!(err.kind, FinalizeRejectionKind::NotTerminal);
        assert!(!err.is_conflict());
    }

    #[test]
    fn finalize_rejects_illegal_pre_terminal_transition() {
        // queued -> completed skips running: illegal even for finalize.
        let request = FinalizeSettlement {
            status: RunStatus::Completed,
            reason: None,
            final_message: None,
        };
        let err = RunStateMachine::finalize(RunStatus::Queued, None, &request).unwrap_err();
        assert_eq!(err.kind, FinalizeRejectionKind::IllegalTransition);
    }

    #[test]
    fn finalize_is_idempotent_only_for_completely_identical_settlements() {
        let stored = settled_completed(Some("done"));
        // Completely identical => idempotent replay.
        let same = settled_completed(Some("done"));
        match RunStateMachine::finalize(RunStatus::Completed, Some(&stored), &same).unwrap() {
            FinalizeVerdict::IdempotentReplay { terminal } => {
                assert_eq!(terminal, RunStatus::Completed)
            }
            other => panic!("expected IdempotentReplay, got {other:?}"),
        }
        // Different status => conflict.
        let failed = FinalizeSettlement {
            status: RunStatus::Failed,
            reason: None,
            final_message: None,
        };
        let err =
            RunStateMachine::finalize(RunStatus::Completed, Some(&stored), &failed).unwrap_err();
        assert_eq!(err.kind, FinalizeRejectionKind::Conflict);
        assert!(err.is_conflict());
        // Same status, different final message => conflict (NOT a replay).
        let changed_message = settled_completed(Some("different"));
        let err = RunStateMachine::finalize(RunStatus::Completed, Some(&stored), &changed_message)
            .unwrap_err();
        assert_eq!(err.kind, FinalizeRejectionKind::Conflict);
        // Same status + message, different reason => conflict.
        let changed_reason = FinalizeSettlement {
            status: RunStatus::Completed,
            reason: Some("completed.no_final.empty_reply".to_string()),
            final_message: stored.final_message.clone(),
        };
        let err = RunStateMachine::finalize(RunStatus::Completed, Some(&stored), &changed_reason)
            .unwrap_err();
        assert_eq!(err.kind, FinalizeRejectionKind::Conflict);
    }

    #[test]
    fn finalize_diagnoses_self_disagreeing_stored_state() {
        // Stored settlement claims cancelled while the run row says
        // completed: corrupted pairing is loud, never guessed.
        let stored = FinalizeSettlement {
            status: RunStatus::Cancelled,
            reason: None,
            final_message: None,
        };
        let err = RunStateMachine::finalize(
            RunStatus::Completed,
            Some(&stored),
            &settled_completed(None),
        )
        .unwrap_err();
        assert_eq!(err.kind, FinalizeRejectionKind::CorruptSettlement);
    }

    /// R03-A02 property core (pure half): for ANY order of settlement
    /// submissions against one run, exactly one Commit verdict is possible,
    /// every later submission is an IdempotentReplay of that winner or a
    /// Conflict — the winner never flips.
    #[test]
    fn finalize_property_first_settlement_wins_and_never_flips() {
        // Deterministic xorshift; the seed is part of the test's identity.
        let mut seed: u64 = 0x00C0_FFEE_5EED_0001;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // First-round pool: settlements legal from `running`.
        let first_pool = || {
            vec![
                settled_completed(Some("answer-a")),
                settled_completed(Some("answer-b")),
                FinalizeSettlement {
                    status: RunStatus::Failed,
                    reason: Some("failed.provider_error".to_string()),
                    final_message: None,
                },
                FinalizeSettlement {
                    status: RunStatus::InterruptedNeedsAttention,
                    reason: Some("interrupted_needs_attention.recovery_unsafe".to_string()),
                    final_message: None,
                },
            ]
        };
        // Late-arrival pool: may ALSO contain out-of-order conflicting
        // settlements that were never legal from `running` directly.
        let late_pool = |winner: &FinalizeSettlement| {
            let mut pool = first_pool();
            pool.push(FinalizeSettlement {
                status: RunStatus::Cancelled,
                reason: Some("cancelled.requested".to_string()),
                final_message: None,
            });
            pool.push(winner.clone());
            pool
        };
        for _ in 0..500 {
            let pool = first_pool();
            let first = pool[(next() % pool.len() as u64) as usize].clone();
            // The first submission from `running` always commits.
            let winner = match RunStateMachine::finalize(RunStatus::Running, None, &first).unwrap()
            {
                FinalizeVerdict::Commit { to, .. } => to,
                other => panic!("first finalize must commit, got {other:?}"),
            };
            let late = late_pool(&first);
            let mut commits_after_first = 0u32;
            let mut replays = 0u32;
            let mut conflicts = 0u32;
            for _ in 0..12 {
                let request = late[(next() % late.len() as u64) as usize].clone();
                match RunStateMachine::finalize(winner, Some(&first), &request) {
                    Ok(FinalizeVerdict::IdempotentReplay { terminal }) => {
                        assert_eq!(terminal, winner);
                        assert_eq!(request, first, "only the identical request replays");
                        replays += 1;
                    }
                    Ok(other) => panic!("post-terminal finalize committed again: {other:?}"),
                    Err(rej) => {
                        assert_eq!(rej.kind, FinalizeRejectionKind::Conflict);
                        assert_ne!(request, first);
                        conflicts += 1;
                    }
                }
                commits_after_first += 0; // no verdict path can commit again
            }
            assert_eq!(commits_after_first, 0);
            assert_eq!(replays + conflicts, 12);
        }
    }

    // ── R03-T01: run outcome contract ──────────────────────────────────────

    #[test]
    fn run_finish_maps_every_variant_to_its_status_and_reason_code() {
        let cases: Vec<(RunFinish, RunStatus, &str)> = vec![
            (
                RunFinish::CompletedWithFinal {
                    message: final_message("hi"),
                },
                RunStatus::Completed,
                "completed.with_final",
            ),
            (
                RunFinish::CompletedWithoutFinal {
                    cause: NoFinalCause::EmptyReply,
                },
                RunStatus::Completed,
                "completed.no_final.empty_reply",
            ),
            (
                RunFinish::CompletedWithoutFinal {
                    cause: NoFinalCause::ProcessOnly,
                },
                RunStatus::Completed,
                "completed.no_final.process_only",
            ),
            (
                RunFinish::CompletedWithoutFinal {
                    cause: NoFinalCause::ToolPartialFailure,
                },
                RunStatus::Completed,
                "completed.no_final.tool_partial_failure",
            ),
            (
                RunFinish::CompletedWithoutFinal {
                    cause: NoFinalCause::NoProviderConfigured,
                },
                RunStatus::Completed,
                "completed.no_final.no_provider_configured",
            ),
            (
                RunFinish::Failed {
                    cause: FailureCause::ProviderFailed {
                        code: "upstream_unavailable".to_string(),
                        retryable: true,
                    },
                },
                RunStatus::Failed,
                "failed.provider_error",
            ),
            (
                RunFinish::Failed {
                    cause: FailureCause::TurnBudgetExceeded { max_turns: 8 },
                },
                RunStatus::Failed,
                "failed.turn_budget_exceeded",
            ),
            (
                RunFinish::Failed {
                    cause: FailureCause::ToolExecutorUnavailable,
                },
                RunStatus::Failed,
                "failed.tool_executor_unavailable",
            ),
            (
                RunFinish::Failed {
                    cause: FailureCause::QuotaExhausted {
                        resource: QuotaResource::Model,
                    },
                },
                RunStatus::Failed,
                "failed.quota_exhausted.model",
            ),
            (
                RunFinish::Failed {
                    cause: FailureCause::QuotaExhausted {
                        resource: QuotaResource::Tool,
                    },
                },
                RunStatus::Failed,
                "failed.quota_exhausted.tool",
            ),
            (
                RunFinish::Cancelled {
                    detail: "user".to_string(),
                },
                RunStatus::Cancelled,
                "cancelled.requested",
            ),
            (
                RunFinish::InterruptedNeedsAttention {
                    detail: "unknown side effect".to_string(),
                },
                RunStatus::InterruptedNeedsAttention,
                "interrupted_needs_attention.recovery_unsafe",
            ),
        ];
        for (finish, status, reason) in cases {
            assert!(finish.status().is_terminal());
            assert_eq!(finish.status(), status);
            assert_eq!(finish.terminal_reason(), reason);
        }
    }

    #[test]
    fn final_message_exists_only_when_truly_produced() {
        let with_final = RunFinish::CompletedWithFinal {
            message: final_message("real answer"),
        };
        assert_eq!(
            with_final.final_message().map(|m| m.content.len()),
            Some(1),
            "with_final yields the real message"
        );
        for finish in [
            RunFinish::CompletedWithoutFinal {
                cause: NoFinalCause::EmptyReply,
            },
            RunFinish::Failed {
                cause: FailureCause::ToolExecutorUnavailable,
            },
            RunFinish::Cancelled {
                detail: String::new(),
            },
            RunFinish::InterruptedNeedsAttention {
                detail: String::new(),
            },
        ] {
            assert!(
                finish.final_message().is_none(),
                "no variant fabricates a final message"
            );
            assert!(finish.settlement().final_message.is_none());
        }
    }

    #[test]
    fn settlement_of_with_final_carries_the_message_and_stable_reason() {
        let finish = RunFinish::CompletedWithFinal {
            message: final_message("answer"),
        };
        let settlement = finish.settlement();
        assert_eq!(settlement.status, RunStatus::Completed);
        assert_eq!(settlement.reason.as_deref(), Some("completed.with_final"));
        assert!(settlement.final_message.is_some());
        // Identical finish => identical settlement => idempotent finalize.
        let again = RunFinish::CompletedWithFinal {
            message: final_message("answer"),
        };
        match RunStateMachine::finalize(
            RunStatus::Completed,
            Some(&settlement),
            &again.settlement(),
        )
        .unwrap()
        {
            FinalizeVerdict::IdempotentReplay { .. } => {}
            other => panic!("expected replay, got {other:?}"),
        }
    }

    // ── R03-T01: identity layers ───────────────────────────────────────────

    #[test]
    fn identity_layers_are_distinct_and_stable() {
        let run = RunId::new("run_0000017f3b0e0000_000001");
        // Attempt ids increment on the SAME run.
        assert_eq!(
            attempt_id(&run, 1).as_str(),
            "run_0000017f3b0e0000_000001#a1"
        );
        assert_eq!(
            attempt_id(&run, 2).as_str(),
            "run_0000017f3b0e0000_000001#a2"
        );
        // Model/tool call ids are their own layers, never the run/attempt id.
        assert_eq!(
            model_call_id(&run, 1).as_str(),
            "run_0000017f3b0e0000_000001-mc0001"
        );
        assert_eq!(
            tool_call_id(&run, 1).as_str(),
            "run_0000017f3b0e0000_000001-tc0001"
        );
        assert_ne!(attempt_id(&run, 1), AttemptId::new(run.to_string()));
        assert_ne!(model_call_id(&run, 1).as_str(), run.as_str());
        assert_ne!(model_call_id(&run, 1), model_call_id(&run, 2));
        assert_ne!(tool_call_id(&run, 1), tool_call_id(&run, 2));
    }
}

// 合法未提交候选改动
