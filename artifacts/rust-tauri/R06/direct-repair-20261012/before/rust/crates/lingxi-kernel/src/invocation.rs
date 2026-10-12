//! Side-effect invocation receipts and the crash-recovery policy for them
//! (R03-T05).
//!
//! The run layer journals every tool request as a receipt whose lifecycle is
//!
//! ```text
//! prepared → authorized → started → succeeded | failed | unknown
//! ```
//!
//! bound to the owner facts, the run/attempt/generation triple, the target
//! and the normalized argument digest (optionally an idempotency key). The
//! durable write ORDER is the whole point (contract 02 §8: 外部副作用和本地
//! 数据库没有天然跨系统事务):
//!
//! - the intent (`prepared`, then `authorized`, then `started`) is durable
//!   BEFORE the external execution is dispatched;
//! - the receipt (external response / available dedup identifier) is durable
//!   AFTER the external execution returns;
//! - nothing here claims an atomic transaction ACROSS the external system.
//!
//! On recovery, an entry whose intent is durable but whose receipt is not
//! (`started`, no receipt — the crash landed between the external execution
//! and the local commit) is classified UNKNOWN: the external operation may
//! or may not have completed, and no code path may assume either. Whether
//! an automatic, bounded recovery is allowed then depends on the tool's
//! VERIFIED capabilities — a read-only tool or one whose idempotency key
//! the external system honors can be recovered automatically; an unknown
//! side effect of a non-idempotent tool (send message / payment /
//! destructive write) must NEVER be blindly retried and goes to an
//! explainable needs-attention state instead.
//!
//! Boundary (R03): this module is the DECISION core and the receipt
//! vocabulary. The startup scan that walks every non-terminal run and
//! drives the decisions is R03-T07's RecoveryCoordinator; the full
//! per-tool capability registry is R04's tool gateway.

use crate::ports::{InvocationJournalEntry, InvocationPhase, ReceiptOutcome};

/// What the durable journal proves about one invocation after a restart
/// (R03-T05 恢复分类四态: 未执行 / 确认完成 / 确认失败 / 结果无法确认).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryClass {
    /// The external call was never dispatched: the journal holds only an
    /// intent (`prepared`/`authorized`) and no execution ever started.
    NotExecuted,
    /// A receipt proves the invocation completed (`succeeded`).
    ConfirmedCompleted,
    /// A receipt proves the invocation definitively did not succeed
    /// (`failed` — including never-dispatched closures such as a rejected
    /// approval; `dispatched` on the receipt says which).
    ConfirmedFailed,
    /// The intent says `started` (or an earlier recovery already marked the
    /// outcome `unknown`) and no trustworthy receipt exists: the external
    /// outcome CANNOT be determined from local state. Nothing may assume
    /// either completion or non-completion.
    Unknown,
}

/// The per-call recovery decision derived from one journal entry plus the
/// tool's VERIFIED recovery capabilities (R03-T05 怎么做 3/4: 按工具幂等性
/// 和外部查询能力决定核验、重试或人工确认).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryDecision {
    /// 未执行: the external call was never dispatched, so a bounded
    /// automatic re-execution cannot duplicate any side effect. (The
    /// coordination itself — which runs get re-driven — belongs to the
    /// R03-T07 recovery coordinator.)
    SafeToReexecute {
        journal_id: String,
        detail: &'static str,
    },
    /// 确认完成 / 确认失败: the receipt already settles the invocation;
    /// recovery has nothing to do for it.
    ConfirmedSettled {
        journal_id: String,
        outcome: ReceiptOutcome,
        dedup_id: Option<String>,
    },
    /// unknown + 只读: re-executing has no external side effect, so a
    /// bounded automatic re-execution is allowed (R03-T05 怎么做 4).
    UnknownReadOnlyReexecute { journal_id: String },
    /// unknown + 受证实幂等: the journal's idempotency key is honored by
    /// the external system, so the invocation may be re-issued WITH THE
    /// SAME KEY — the external system will not perform the operation twice
    /// and returns/verifies the recorded outcome.
    UnknownResumeWithIdempotencyKey { journal_id: String, key: String },
    /// unknown + 外部可核验: local state cannot decide, but the external
    /// system can be queried. Verify first; settle from what the query
    /// proves (possibly into `unknown` again if the query is inconclusive).
    UnknownVerifyExternally { journal_id: String },
    /// unknown with none of the above: a blind retry could duplicate a
    /// message send / payment / destructive write. FORBIDDEN to retry
    /// automatically — the invocation enters an explainable
    /// needs-attention state (user / operator confirms).
    NeedsAttention { journal_id: String, reason: String },
}

impl RecoveryDecision {
    /// Stable machine-readable tag (evidence / logging vocabulary).
    pub fn name(&self) -> &'static str {
        match self {
            RecoveryDecision::SafeToReexecute { .. } => "safe_to_reexecute",
            RecoveryDecision::ConfirmedSettled { .. } => "confirmed_settled",
            RecoveryDecision::UnknownReadOnlyReexecute { .. } => "unknown_read_only_reexecute",
            RecoveryDecision::UnknownResumeWithIdempotencyKey { .. } => {
                "unknown_resume_with_idempotency_key"
            }
            RecoveryDecision::UnknownVerifyExternally { .. } => "unknown_verify_externally",
            RecoveryDecision::NeedsAttention { .. } => "needs_attention",
        }
    }

    /// The journal entry this decision belongs to.
    pub fn journal_id(&self) -> &str {
        match self {
            RecoveryDecision::SafeToReexecute { journal_id, .. }
            | RecoveryDecision::ConfirmedSettled { journal_id, .. }
            | RecoveryDecision::UnknownReadOnlyReexecute { journal_id }
            | RecoveryDecision::UnknownResumeWithIdempotencyKey { journal_id, .. }
            | RecoveryDecision::UnknownVerifyExternally { journal_id }
            | RecoveryDecision::NeedsAttention { journal_id, .. } => journal_id,
        }
    }
}

/// The VERIFIED recovery capabilities of one tool target, as consumed by
/// [`classify_invocation_recovery`]. In R03 these facts are supplied by the
/// caller (tests / the R03 run layer); R04's tool registry owns the real
/// per-target classification and must NOT default unknown tools to
/// optimistic values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolRecoveryCapability {
    /// The tool has no external side effect (a read): re-execution cannot
    /// duplicate anything.
    pub read_only: bool,
    /// The external system honors the invocation's idempotency key:
    /// re-issuing with the same key never performs the operation twice.
    pub honors_idempotency_key: bool,
    /// The external system's state can be queried to confirm whether the
    /// operation completed.
    pub externally_verifiable: bool,
}

impl ToolRecoveryCapability {
    /// The conservative default for any tool whose recovery behavior is
    /// NOT verified: no automatic recovery of unknown outcomes is allowed
    /// (they go to needs-attention). This is the only safe default —
    /// optimistic defaults would blindly retry unconfirmed side effects.
    pub const CONSERVATIVE: ToolRecoveryCapability = ToolRecoveryCapability {
        read_only: false,
        honors_idempotency_key: false,
        externally_verifiable: false,
    };
}

/// Classifies one durable journal entry into the four-state recovery class.
///
/// Pure local reasoning over the durable facts — no I/O, no clock: the
/// decision is a function of what survived the crash.
pub fn invocation_recovery_class(entry: &InvocationJournalEntry) -> RecoveryClass {
    match entry.phase {
        // Intent-only entries: `started` was never durably recorded, and
        // the run layer dispatches the external execution only AFTER the
        // `started` write commits — so the external call never happened.
        InvocationPhase::Prepared | InvocationPhase::Authorized => RecoveryClass::NotExecuted,
        // `started` without a receipt is exactly the crash window between
        // the external execution and the local receipt commit.
        InvocationPhase::Started => RecoveryClass::Unknown,
        // Closed receipts settle the class outright.
        InvocationPhase::Succeeded => RecoveryClass::ConfirmedCompleted,
        InvocationPhase::Failed => RecoveryClass::ConfirmedFailed,
        // An earlier recovery (or a fenced/unobserved result) already
        // recorded the honest unknown.
        InvocationPhase::Unknown => RecoveryClass::Unknown,
    }
}

/// The recovery DECISION for one journal entry under one tool's verified
/// capabilities (R03-T05 怎么做 3/4).
///
/// Rules:
/// - 未执行 (intent only) → bounded automatic re-execution is safe.
/// - receipt settled → nothing to do (the receipt's dedup id / detail
///   travels along for verification).
/// - unknown outcome:
///   - read-only tool → re-execute (no side effect to duplicate);
///   - idempotency key present AND honored → resume with the SAME key;
///   - externally verifiable → verify, then settle;
///   - otherwise → needs attention: blind retry forbidden.
pub fn classify_invocation_recovery(
    entry: &InvocationJournalEntry,
    capability: &ToolRecoveryCapability,
) -> RecoveryDecision {
    let journal_id = entry.journal_id.clone();
    match invocation_recovery_class(entry) {
        RecoveryClass::NotExecuted => RecoveryDecision::SafeToReexecute {
            journal_id,
            detail: "intent durable, external execution never dispatched",
        },
        RecoveryClass::ConfirmedCompleted => RecoveryDecision::ConfirmedSettled {
            journal_id,
            outcome: ReceiptOutcome::Succeeded,
            dedup_id: entry.receipt.as_ref().and_then(|r| r.dedup_id.clone()),
        },
        RecoveryClass::ConfirmedFailed => RecoveryDecision::ConfirmedSettled {
            journal_id,
            outcome: ReceiptOutcome::Failed,
            dedup_id: entry.receipt.as_ref().and_then(|r| r.dedup_id.clone()),
        },
        RecoveryClass::Unknown => {
            if capability.read_only {
                RecoveryDecision::UnknownReadOnlyReexecute { journal_id }
            } else if let (true, Some(key)) = (
                capability.honors_idempotency_key,
                entry.idempotency_key.as_ref(),
            ) {
                RecoveryDecision::UnknownResumeWithIdempotencyKey {
                    journal_id,
                    key: key.clone(),
                }
            } else if capability.externally_verifiable {
                RecoveryDecision::UnknownVerifyExternally { journal_id }
            } else {
                RecoveryDecision::NeedsAttention {
                    journal_id,
                    reason: format!(
                        "unknown external outcome for non-idempotent tool {:?} (started \
                         without receipt; blind retry could duplicate the side effect)",
                        entry.target
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::InvocationReceipt;

    fn entry(phase: InvocationPhase) -> InvocationJournalEntry {
        InvocationJournalEntry {
            journal_id: "run_x_tc0001".to_string(),
            session_id: "s".to_string(),
            run_id: "run_x".to_string(),
            attempt: "run_x#a1".to_string(),
            generation: 1,
            owner_kind: "local_user".to_string(),
            owner_subject: "user_local".to_string(),
            target: "send_message".to_string(),
            args_digest: "ab".to_string(),
            args_summary: None,
            idempotency_key: Some("run_x_tc0001".to_string()),
            phase,
            receipt: None,
            prepared_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        }
    }

    fn receipt(outcome: ReceiptOutcome, dedup: Option<&str>) -> Option<InvocationReceipt> {
        Some(InvocationReceipt {
            outcome,
            detail: "verified".to_string(),
            dedup_id: dedup.map(|d| d.to_string()),
            dispatched: true,
        })
    }

    #[test]
    fn four_state_classification_follows_the_durable_phase() {
        assert_eq!(
            invocation_recovery_class(&entry(InvocationPhase::Prepared)),
            RecoveryClass::NotExecuted
        );
        assert_eq!(
            invocation_recovery_class(&entry(InvocationPhase::Authorized)),
            RecoveryClass::NotExecuted
        );
        assert_eq!(
            invocation_recovery_class(&entry(InvocationPhase::Started)),
            RecoveryClass::Unknown
        );
        assert_eq!(
            invocation_recovery_class(&entry(InvocationPhase::Unknown)),
            RecoveryClass::Unknown
        );
        let mut done = entry(InvocationPhase::Succeeded);
        done.receipt = receipt(ReceiptOutcome::Succeeded, Some("dedup-1"));
        assert_eq!(
            invocation_recovery_class(&done),
            RecoveryClass::ConfirmedCompleted
        );
        let mut failed = entry(InvocationPhase::Failed);
        failed.receipt = receipt(ReceiptOutcome::Failed, None);
        assert_eq!(
            invocation_recovery_class(&failed),
            RecoveryClass::ConfirmedFailed
        );
    }

    #[test]
    fn not_executed_entries_are_safe_to_reexecute_regardless_of_capability() {
        for phase in [InvocationPhase::Prepared, InvocationPhase::Authorized] {
            let decision =
                classify_invocation_recovery(&entry(phase), &ToolRecoveryCapability::CONSERVATIVE);
            assert!(
                matches!(decision, RecoveryDecision::SafeToReexecute { .. }),
                "{phase:?} must be safe to re-execute"
            );
            assert_eq!(decision.name(), "safe_to_reexecute");
        }
    }

    #[test]
    fn settled_receipts_need_no_recovery_action() {
        let mut done = entry(InvocationPhase::Succeeded);
        done.receipt = receipt(ReceiptOutcome::Succeeded, Some("dedup-9"));
        match classify_invocation_recovery(&done, &ToolRecoveryCapability::CONSERVATIVE) {
            RecoveryDecision::ConfirmedSettled {
                journal_id,
                outcome,
                dedup_id,
            } => {
                assert_eq!(journal_id, "run_x_tc0001");
                assert_eq!(outcome, ReceiptOutcome::Succeeded);
                assert_eq!(dedup_id.as_deref(), Some("dedup-9"));
            }
            other => panic!("expected ConfirmedSettled, got {other:?}"),
        }
    }

    #[test]
    fn unknown_non_idempotent_side_effect_needs_attention_never_retry() {
        let decision = classify_invocation_recovery(
            &entry(InvocationPhase::Started),
            &ToolRecoveryCapability::CONSERVATIVE,
        );
        match &decision {
            RecoveryDecision::NeedsAttention { journal_id, reason } => {
                assert_eq!(journal_id, "run_x_tc0001");
                assert!(
                    reason.contains("blind retry") && reason.contains("send_message"),
                    "reason must be explainable: {reason}"
                );
            }
            other => panic!("expected NeedsAttention, got {other:?}"),
        }
        assert_eq!(decision.name(), "needs_attention");
    }

    #[test]
    fn unknown_read_only_tool_allows_bounded_reexecution() {
        let capability = ToolRecoveryCapability {
            read_only: true,
            ..ToolRecoveryCapability::CONSERVATIVE
        };
        let decision = classify_invocation_recovery(&entry(InvocationPhase::Started), &capability);
        assert!(matches!(
            decision,
            RecoveryDecision::UnknownReadOnlyReexecute { .. }
        ));
    }

    #[test]
    fn unknown_with_honored_idempotency_key_resumes_with_the_same_key() {
        let capability = ToolRecoveryCapability {
            honors_idempotency_key: true,
            ..ToolRecoveryCapability::CONSERVATIVE
        };
        match classify_invocation_recovery(&entry(InvocationPhase::Started), &capability) {
            RecoveryDecision::UnknownResumeWithIdempotencyKey { journal_id, key } => {
                assert_eq!(journal_id, "run_x_tc0001");
                assert_eq!(key, "run_x_tc0001", "the SAME key must be reused");
            }
            other => panic!("expected UnknownResumeWithIdempotencyKey, got {other:?}"),
        }
    }

    #[test]
    fn honored_capability_without_a_journaled_key_cannot_resume() {
        // The external system honors keys, but this invocation never got
        // one journaled (e.g. a pre-key legacy entry): no key to present,
        // so resume is impossible — the decision must not pretend
        // otherwise. Verifiable falls through; conservative → attention.
        let capability = ToolRecoveryCapability {
            honors_idempotency_key: true,
            ..ToolRecoveryCapability::CONSERVATIVE
        };
        let mut no_key = entry(InvocationPhase::Started);
        no_key.idempotency_key = None;
        assert!(matches!(
            classify_invocation_recovery(&no_key, &capability),
            RecoveryDecision::NeedsAttention { .. }
        ));
        // Externally verifiable without a key: verify instead.
        let verifiable = ToolRecoveryCapability {
            externally_verifiable: true,
            ..ToolRecoveryCapability::CONSERVATIVE
        };
        assert!(matches!(
            classify_invocation_recovery(&no_key, &verifiable),
            RecoveryDecision::UnknownVerifyExternally { .. }
        ));
    }

    #[test]
    fn read_only_takes_precedence_over_key_and_verify() {
        // Order of the decision ladder is part of the contract: read-only
        // is the strongest safety property (no side effect at all).
        let capability = ToolRecoveryCapability {
            read_only: true,
            honors_idempotency_key: true,
            externally_verifiable: true,
        };
        assert!(matches!(
            classify_invocation_recovery(&entry(InvocationPhase::Started), &capability),
            RecoveryDecision::UnknownReadOnlyReexecute { .. }
        ));
    }

    #[test]
    fn conservative_capability_is_fully_closed() {
        let cap = ToolRecoveryCapability::CONSERVATIVE;
        assert!(!cap.read_only && !cap.honors_idempotency_key && !cap.externally_verifiable);
    }

    #[test]
    fn decision_vocabulary_is_stable() {
        let e = entry(InvocationPhase::Started);
        let pairs = [
            (
                classify_invocation_recovery(
                    &entry(InvocationPhase::Prepared),
                    &ToolRecoveryCapability::CONSERVATIVE,
                ),
                "safe_to_reexecute",
            ),
            (
                classify_invocation_recovery(
                    &e,
                    &ToolRecoveryCapability {
                        read_only: true,
                        ..ToolRecoveryCapability::CONSERVATIVE
                    },
                ),
                "unknown_read_only_reexecute",
            ),
            (
                classify_invocation_recovery(
                    &e,
                    &ToolRecoveryCapability {
                        honors_idempotency_key: true,
                        ..ToolRecoveryCapability::CONSERVATIVE
                    },
                ),
                "unknown_resume_with_idempotency_key",
            ),
            (
                classify_invocation_recovery(
                    &e,
                    &ToolRecoveryCapability {
                        externally_verifiable: true,
                        ..ToolRecoveryCapability::CONSERVATIVE
                    },
                ),
                "unknown_verify_externally",
            ),
            (
                classify_invocation_recovery(&e, &ToolRecoveryCapability::CONSERVATIVE),
                "needs_attention",
            ),
        ];
        for (decision, name) in pairs {
            assert_eq!(decision.name(), name);
            assert_eq!(decision.journal_id(), "run_x_tc0001");
        }
    }
}
