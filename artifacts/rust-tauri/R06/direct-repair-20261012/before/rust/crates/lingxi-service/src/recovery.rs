//! The startup recovery coordinator (R03-T07 deliverable
//! RecoveryCoordinator): the ONE startup scan that resolves every
//! non-terminal run a previous process left behind.
//!
//! What it does, per non-terminal run found by
//! [`RunDatabase::list_active_runs`]:
//!
//! 1. **journal pass** — the R03-T05 recovery face
//!    ([`crate::invocations::recover_run_invocations`]): loads the durable
//!    invocation receipts, classifies each entry under the per-target
//!    capabilities and persists the honest `unknown` verdict for entries
//!    caught in the crash window (`started`, no receipt). No tool is ever
//!    re-executed by the scan itself.
//! 2. **run-level classification** — the kernel's
//!    [`lingxi_kernel::recovery::classify_run_recovery`] lifts the entry
//!    decisions to the four-state recovery table
//!    (recoverable_wait / retryable_read_only / unknown_side_effect /
//!    interrupted_needs_attention) with a deterministic user reason and
//!    executable next actions.
//! 3. **honest settlement** — the run settles through the SAME single
//!    finalize path every driver uses ([`StoragePort::commit_run_outcome`];
//!    the kernel's `RunStateMachine::finalize` decision runs inside that
//!    storage transaction exactly as in `RunSupervisor::finalize_settlement`):
//!    a prior `cancelling` run becomes `cancelled` (completing the
//!    cancellation the previous process had already requested — never an
//!    interruption of the user's own requested outcome); every other active
//!    status (`queued`, `running`, `waiting_approval`) becomes
//!    `interrupted_needs_attention` (a `queued`/`waiting_approval` run first
//!    re-enters `running` through the legal non-terminal leg — the kernel
//!    machine has no direct `queued → interrupted` edge — with a
//!    recovery-marked reason so the history stays honest).
//!
//!    The run ROW keeps the stable terminal-reason vocabulary
//!    (`interrupted_needs_attention.recovery_unsafe` /
//!    `cancelled.requested`); the terminal `run_state_changed` key event
//!    carries the RICH recovery reason — category, journal facts, next
//!    actions — which is the user-visible explanation surface (event
//!    stream / history projections read exactly that payload field).
//!
//! Honesty rules (R03-T07 taskbook):
//! - **不虚构模型最终回复**: the settlement carries NO final message for
//!   any category — the run ended when the service died, and no code path
//!   invents an answer.
//! - **终态不复活** (T01 contract): the scan LIST only contains non-terminal
//!   rows, and a row that became terminal between the listing and the
//!   finalize (e.g. a concurrently settling drive when the scan is invoked
//!   outside bootstrap) is detected and reported as `already_terminal`,
//!   never re-settled.
//! - **能解释 interrupted，不把重启自动当成功**: a restart never presents
//!   these runs as completed; each gets an explainable terminal with its
//!   category and next actions.
//!
//! Boundary: re-DRIVING a `recoverable_wait` run (resuming the agent loop)
//! needs a provider — that is R05's loop through the same supervisor, not a
//! second scheduler here. The scan parks such runs in the explainable
//! interrupted terminal instead of leaving a row that pretends progress.

use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::{KeyEvent, RunOutcome, StorageError, StoragePort};
use lingxi_kernel::recovery::{
    classify_run_recovery, principal_from_storage_facts, RunRecoveryCategory,
};
use lingxi_kernel::RunFinish;
use lingxi_protocol::{
    AttemptId, EventId, EventPayload, KnownEventPayload, RunId, RunStateChangedPayload, RunStatus,
    SessionId,
};

use crate::events::EventService;
use crate::invocations::{recover_run_invocations, RecoveryCapabilitySource};

/// The R03-T07 RecoveryCoordinator: stateless by design — the composition
/// root invokes [`RecoveryCoordinator::run_startup_scan`] once per process,
/// right after the storage/event surfaces exist and before serving starts.
#[derive(Debug, Default)]
pub struct RecoveryCoordinator;

/// How one scanned run was settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoverySettlement {
    /// The terminal was committed by THIS scan (`newly_committed` mirrors
    /// the single-finalize transaction's verdict; `false` = an identical
    /// settlement already existed — the idempotent replay of the kernel's
    /// finalize decision).
    Written {
        target: RunStatus,
        newly_committed: bool,
    },
    /// The run became terminal between the listing and the finalize (a
    /// concurrently settling drive when the scan runs outside bootstrap).
    /// Recorded, never re-settled — 终态不复活.
    AlreadyTerminal { status: RunStatus },
}

/// The per-run outcome of one startup scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecoveryOutcome {
    pub run_id: String,
    pub session_id: String,
    pub prior_status: RunStatus,
    pub category: RunRecoveryCategory,
    /// Per-decision bucket counts of the journal pass (stable order; the
    /// same vocabulary as [`crate::invocations::RunRecoveryReport`]).
    pub decision_counts: Vec<(&'static str, usize)>,
    /// How many crash-window entries THIS scan persisted the `unknown`
    /// verdict for (a re-scan over already-marked entries reports 0).
    pub unknown_verdicts_persisted: usize,
    pub settlement: RecoverySettlement,
    /// The deterministic user-facing explanation + next actions recorded in
    /// the terminal event (empty string only for `AlreadyTerminal`).
    pub user_reason: String,
    pub next_actions: Vec<String>,
}

/// The full report of one startup scan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryScanReport {
    pub scanned: usize,
    pub outcomes: Vec<RunRecoveryOutcome>,
}

impl RecoveryScanReport {
    /// How many runs landed in each category bucket (stable order).
    pub fn category_counts(&self) -> Vec<(&'static str, usize)> {
        let names = [
            "recoverable_wait",
            "retryable_read_only",
            "unknown_side_effect",
            "interrupted_needs_attention",
        ];
        names
            .into_iter()
            .map(|name| {
                (
                    name,
                    self.outcomes
                        .iter()
                        .filter(|o| o.category.name() == name)
                        .count(),
                )
            })
            .collect()
    }
}

impl RecoveryCoordinator {
    /// The startup scan. Errors are LOUD (a corrupted journal/status that
    /// cannot be classified refuses startup rather than serving a guess —
    /// the same fail-closed stance as the R02 integrity gates).
    pub async fn run_startup_scan(
        &self,
        storage: &RunDatabase,
        events: &EventService,
        capabilities: &dyn RecoveryCapabilitySource,
        now_ms: u64,
    ) -> Result<RecoveryScanReport, StorageError> {
        let active = storage.list_active_runs().await?;
        let mut report = RecoveryScanReport {
            scanned: active.len(),
            outcomes: Vec::with_capacity(active.len()),
        };
        for facts in active {
            let principal = principal_from_storage_facts(&facts.owner_kind, &facts.owner_subject)?;
            let ctx = lingxi_kernel::RunContext {
                principal,
                session_id: SessionId::new(facts.session_id.clone()),
                run_id: RunId::new(facts.run_id.clone()),
                attempt: AttemptId::new(facts.current_attempt.clone()),
                generation: facts.generation,
            };
            // 1) The T05 journal pass: classify + persist unknown verdicts.
            let journal =
                recover_run_invocations(storage, &facts.run_id, capabilities, now_ms).await?;
            // 2) The kernel's run-level classification table.
            let decisions: Vec<_> = journal.entries.iter().map(|e| e.decision.clone()).collect();
            let plan = classify_run_recovery(&decisions);
            // 3) The honest settlement through the single finalize path.
            let settlement =
                Self::settle(storage, events, &ctx, facts.status, &plan, now_ms).await?;
            if let RecoverySettlement::AlreadyTerminal { .. } = settlement {
                tracing::info!(
                    run_id = %facts.run_id,
                    prior_status = facts.status.wire_name(),
                    "recovery scan: run settled concurrently before the scan's finalize \
                     (terminal respected, nothing rewritten)"
                );
            } else {
                tracing::info!(
                    run_id = %facts.run_id,
                    prior_status = facts.status.wire_name(),
                    category = plan.category.name(),
                    settlement = ?settlement,
                    "recovery scan settled a dangling-active run with an explainable terminal"
                );
            }
            report.outcomes.push(RunRecoveryOutcome {
                run_id: facts.run_id.clone(),
                session_id: facts.session_id.clone(),
                prior_status: facts.status,
                category: plan.category,
                decision_counts: journal.decision_counts(),
                unknown_verdicts_persisted: journal
                    .entries
                    .iter()
                    .filter(|e| e.unknown_verdict_persisted)
                    .count(),
                settlement,
                user_reason: plan.user_reason.clone(),
                next_actions: plan.next_actions.clone(),
            });
        }
        Ok(report)
    }

    /// Settles one scanned run. The terminal goes through
    /// [`StoragePort::commit_run_outcome`] — the SAME single-finalize
    /// transaction the run drivers use (`RunSupervisor::finalize_settlement`
    /// is the driver-side wrapper of the same storage path; the coordinator
    /// builds its outcome directly so the terminal EVENT can carry the rich
    /// recovery reason while the run ROW keeps the stable reason
    /// vocabulary).
    async fn settle(
        storage: &RunDatabase,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        prior_status: RunStatus,
        plan: &lingxi_kernel::recovery::RunRecoveryPlan,
        now_ms: u64,
    ) -> Result<RecoverySettlement, StorageError> {
        let run_id = ctx.run_id.clone();
        // A `cancelling` run completes the cancellation the previous process
        // had already requested — `cancelled`, not interrupted.
        let (finish, from) = if prior_status == RunStatus::Cancelling {
            (
                RunFinish::Cancelled {
                    detail: format!(
                        "service restarted while the run was cancelling; completing the \
                         requested cancellation (recovery category {})",
                        plan.category.name()
                    ),
                },
                RunStatus::Cancelling,
            )
        } else {
            let from = if prior_status == RunStatus::Running {
                RunStatus::Running
            } else {
                // `queued`/`waiting_approval` have no direct edge to a
                // terminal in the kernel machine: re-enter `running` through
                // the legal non-terminal leg first (reason-marked so the
                // durable history says this was the recovery walk, not a
                // driver-observed phase).
                let committed = storage
                    .record_run_state_change(
                        ctx,
                        prior_status,
                        RunStatus::Running,
                        Some(format!(
                            "recovery_scan: service restarted with the run {}; the \
                             re-entry into running only records its interruption",
                            prior_status.wire_name()
                        )),
                        now_ms,
                    )
                    .await?;
                events.publish_committed(&committed.events);
                RunStatus::Running
            };
            (
                RunFinish::InterruptedNeedsAttention {
                    detail: plan.user_reason.clone(),
                },
                from,
            )
        };
        // Row reason keeps the kernel's stable vocabulary; the EVENT reason
        // carries the rich explanation + next actions (the user-visible
        // surface reads the event payload).
        let row_reason = finish.terminal_reason();
        let event_reason = format!(
            "{row_reason}; {}; next actions: {}",
            plan.user_reason,
            plan.next_actions.join(" | ")
        );
        let outcome = RunOutcome {
            status: finish.status(),
            reason: Some(row_reason),
            key_events: vec![KeyEvent {
                event_id: EventId::new(format!("{run_id}-recovery")),
                payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                    RunStateChangedPayload {
                        from,
                        to: finish.status(),
                        reason: Some(event_reason),
                    },
                )),
            }],
            final_message: None,
        };
        match storage.commit_run_outcome(ctx, outcome, now_ms).await {
            Ok(committed) => {
                events.publish_committed(&committed.events);
                Ok(RecoverySettlement::Written {
                    target: finish.status(),
                    newly_committed: committed.newly_committed,
                })
            }
            Err(StorageError::Conflict { detail }) => {
                // The run settled concurrently (the scan ran while a drive
                // finalized). Re-check: genuinely terminal now → report the
                // respected terminal; anything else is a real conflict.
                if let Some(record) = storage.load_run(&run_id).await? {
                    if record.status.is_terminal() {
                        return Ok(RecoverySettlement::AlreadyTerminal {
                            status: record.status,
                        });
                    }
                }
                Err(StorageError::Conflict { detail })
            }
            Err(other) => Err(other),
        }
    }
}
