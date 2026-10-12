//! Run-level recovery classification (R03-T07 deliverable 恢复分类表).
//!
//! The per-invocation crash facts and decisions are R03-T05's
//! [`crate::invocation`] vocabulary; this module lifts them to the RUN level:
//! given the recovery decisions of every journal entry of one non-terminal
//! run, WHICH of the four frozen categories governs the run, and what the
//! user-visible explanation + next actions are.
//!
//! 恢复分类表 (the four categories, worst-governs priority order):
//!
//! | category (wire name) | durable journal facts | action at R03 | user-visible reason |
//! |---|---|---|---|
//! | `interrupted_needs_attention` | ≥1 entry `NeedsAttention` — an UNKNOWN outcome of a non-idempotent tool (started, no receipt) | terminal `interrupted_needs_attention`; NO auto action — a blind retry could duplicate a message send / payment / destructive write | lists the affected targets + journal ids; next action: verify externally or confirm manually, then resubmit |
//! | `unknown_side_effect` | ≥1 entry `UnknownResumeWithIdempotencyKey` / `UnknownVerifyExternally` and none worse | terminal `interrupted_needs_attention`; the resume/verify decision is recorded as data (the startup wiring has no executor/provider to perform it — R05's loop is the driver that will) | next actions: resubmit resumes through the idempotency key(s) / verify externally first |
//! | `retryable_read_only` | ≥1 entry `UnknownReadOnlyReexecute` and none worse | terminal `interrupted_needs_attention`; re-execution of the read-only calls is side-effect free | next action: safe to resubmit the task |
//! | `recoverable_wait` | no crash-window entries (journal empty, intent-only, or fully settled) | terminal `interrupted_needs_attention` (R03 has no re-drive loop; the row must not dangle active forever pretending progress) | next action: safe to resubmit the task |
//!
//! Two honesty rules bind every category (target contract 02 §4/§8 and the
//! R03-T07 taskbook):
//! - **不虚构模型最终回复**: no category fabricates a final assistant
//!   message; the run settles WITHOUT one and says so.
//! - **终态不复活**: the classification only ever applies to NON-terminal
//!   runs; the service-side coordinator (R03-T07 `lingxi-service`) skips
//!   terminal rows and never re-settles them.
//!
//! Boundary: this module is pure (no I/O, no clock) — it is a function of
//! the durable journal facts. The startup scan that walks the non-terminal
//! runs, persists the T05 `unknown` verdicts and commits the terminals is
//! the service-side RecoveryCoordinator; R05's provider loop is what will
//! eventually re-drive `recoverable_wait` runs instead of parking them.

use crate::invocation::RecoveryDecision;
use crate::ports::StorageError;

/// The run-level recovery category (R03-T07 恢复分类表, four states).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunRecoveryCategory {
    /// 可恢复等待: the journal holds NO crash-window entry — every entry is
    /// intent-only (never dispatched) or already settled. No external side
    /// effect is in question anywhere in the run.
    RecoverableWait,
    /// 可重试只读: the crash window caught read-only call(s) whose
    /// re-execution cannot duplicate any side effect, and nothing worse.
    RetryableReadOnly,
    /// unknown 外部副作用（可恢复路径）: unknown outcome(s) whose tool either
    /// honors the journaled idempotency key or is externally verifiable —
    /// recoverable WITH the key / through verification, never blindly.
    UnknownSideEffect,
    /// interrupted/needs_attention: at least one unknown outcome of a
    /// non-idempotent external side effect. Automatic retry is FORBIDDEN;
    /// the user/operator must resolve it.
    InterruptedNeedsAttention,
}

impl RunRecoveryCategory {
    /// Stable machine-readable tag (evidence / logging vocabulary).
    pub fn name(self) -> &'static str {
        match self {
            RunRecoveryCategory::RecoverableWait => "recoverable_wait",
            RunRecoveryCategory::RetryableReadOnly => "retryable_read_only",
            RunRecoveryCategory::UnknownSideEffect => "unknown_side_effect",
            RunRecoveryCategory::InterruptedNeedsAttention => "interrupted_needs_attention",
        }
    }
}

/// The full recovery plan for one non-terminal run: the governing category
/// plus the user-visible explanation and the executable next actions
/// (R03-T07 怎么做 2: 对用户暴露原因及可执行后续动作).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecoveryPlan {
    pub category: RunRecoveryCategory,
    /// Deterministic, user-readable explanation of what the durable facts
    /// prove. Contains no timestamps and no fabricated model content.
    pub user_reason: String,
    /// Executable next actions for the user/operator, most relevant first.
    pub next_actions: Vec<String>,
    /// The journal ids the category is about (attention targets, resume
    /// keys' entries, read-only entries — empty for `recoverable_wait`).
    pub subject_journal_ids: Vec<String>,
}

/// Classifies one non-terminal run from its journal decisions.
///
/// Priority (worst governs — safety first):
/// `NeedsAttention` > resume/verify unknowns > read-only unknowns > none.
/// An empty decision list is [`RunRecoveryCategory::RecoverableWait`]:
/// the run crashed before any tool call was journaled (or drove without
/// tools), so nothing external is in question.
pub fn classify_run_recovery(decisions: &[RecoveryDecision]) -> RunRecoveryPlan {
    let needs_attention: Vec<&RecoveryDecision> = decisions
        .iter()
        .filter(|d| matches!(d, RecoveryDecision::NeedsAttention { .. }))
        .collect();
    if !needs_attention.is_empty() {
        let subjects: Vec<String> = needs_attention
            .iter()
            .map(|d| d.journal_id().to_string())
            .collect();
        let reasons: Vec<String> = needs_attention
            .iter()
            .map(|d| match d {
                RecoveryDecision::NeedsAttention { reason, .. } => reason.clone(),
                _ => unreachable!("filtered above"),
            })
            .collect();
        return RunRecoveryPlan {
            category: RunRecoveryCategory::InterruptedNeedsAttention,
            user_reason: format!(
                "service restarted while the task was in flight; {} external side \
                 effect(s) have UNKNOWN outcomes (blind retry could duplicate them): {}",
                subjects.len(),
                reasons.join("; ")
            ),
            next_actions: vec![
                "verify the outcome of the listed tool calls with the external system, \
                 or confirm them manually"
                    .to_string(),
                "resubmit the task only after the unknown outcomes are resolved".to_string(),
            ],
            subject_journal_ids: subjects,
        };
    }
    let resumable: Vec<&RecoveryDecision> = decisions
        .iter()
        .filter(|d| {
            matches!(
                d,
                RecoveryDecision::UnknownResumeWithIdempotencyKey { .. }
                    | RecoveryDecision::UnknownVerifyExternally { .. }
            )
        })
        .collect();
    if !resumable.is_empty() {
        let subjects: Vec<String> = resumable
            .iter()
            .map(|d| d.journal_id().to_string())
            .collect();
        let keys: Vec<String> = resumable
            .iter()
            .filter_map(|d| match d {
                RecoveryDecision::UnknownResumeWithIdempotencyKey { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        let mut next_actions = Vec::new();
        if !keys.is_empty() {
            next_actions.push(format!(
                "resubmitting the task resumes through the journaled idempotency \
                 key(s) without repeating the external operation: {}",
                keys.join(", ")
            ));
        }
        next_actions.push("or verify the outcome externally first, then resubmit".to_string());
        return RunRecoveryPlan {
            category: RunRecoveryCategory::UnknownSideEffect,
            user_reason: format!(
                "service restarted while the task was in flight; {} external side \
                 effect(s) have UNKNOWN outcomes that are recoverable through their \
                 idempotency key or external verification (entries: {})",
                subjects.len(),
                subjects.join(", ")
            ),
            next_actions,
            subject_journal_ids: subjects,
        };
    }
    let read_only: Vec<&RecoveryDecision> = decisions
        .iter()
        .filter(|d| matches!(d, RecoveryDecision::UnknownReadOnlyReexecute { .. }))
        .collect();
    if !read_only.is_empty() {
        let subjects: Vec<String> = read_only
            .iter()
            .map(|d| d.journal_id().to_string())
            .collect();
        return RunRecoveryPlan {
            category: RunRecoveryCategory::RetryableReadOnly,
            user_reason: format!(
                "service restarted while the task was in flight; the in-question tool \
                 call(s) were read-only, so no external side effect can be duplicated \
                 (entries: {})",
                subjects.join(", ")
            ),
            next_actions: vec!["safe to resubmit the task".to_string()],
            subject_journal_ids: subjects,
        };
    }
    RunRecoveryPlan {
        category: RunRecoveryCategory::RecoverableWait,
        user_reason: "service restarted while the task was in flight; the invocation journal \
             holds no unresolved external side effect (no tool call was in its crash \
             window), and no final model reply existed when the service stopped"
            .to_string(),
        next_actions: vec!["safe to resubmit the task".to_string()],
        subject_journal_ids: Vec::new(),
    }
}

/// Rebuilds a [`crate::Principal`] from the durable ownership facts of a run
/// row (`owner_kind` + `owner_subject` — exactly the two columns every
/// storage write validates against). The reconstruction preserves the
/// ownership KEY; any non-ownership detail of the original principal (e.g. a
/// device id, which the runs table never stored) is deliberately NOT claimed
/// — an empty placeholder is used where the variant requires a field.
///
/// Unknown vocabulary is a loud [`StorageError::Corrupted`], never a guess.
pub fn principal_from_storage_facts(
    kind: &str,
    subject: &str,
) -> Result<crate::Principal, StorageError> {
    match kind {
        "local_user" => {
            if subject != crate::LOCAL_OWNER_SUBJECT {
                return Err(StorageError::Corrupted {
                    detail: format!(
                        "runs row holds local_user owner with unexpected subject {subject:?}"
                    ),
                });
            }
            Ok(crate::Principal::LocalUser)
        }
        "device" => Ok(crate::Principal::Device {
            device_id: String::new(),
            user_id: subject.to_string(),
        }),
        "web_session" => Ok(crate::Principal::WebSession {
            account_id: subject.to_string(),
        }),
        "automation" => Ok(crate::Principal::Automation {
            surface: subject.to_string(),
        }),
        other => Err(StorageError::Corrupted {
            detail: format!("runs row holds unknown owner_kind {other:?}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invocation::RecoveryDecision;
    use crate::ports::StorageError;
    use crate::Principal;

    fn attention(id: &str) -> RecoveryDecision {
        RecoveryDecision::NeedsAttention {
            journal_id: id.to_string(),
            reason: "unknown external outcome for non-idempotent tool \"send\"".to_string(),
        }
    }

    fn resume(id: &str, key: &str) -> RecoveryDecision {
        RecoveryDecision::UnknownResumeWithIdempotencyKey {
            journal_id: id.to_string(),
            key: key.to_string(),
        }
    }

    fn verify(id: &str) -> RecoveryDecision {
        RecoveryDecision::UnknownVerifyExternally {
            journal_id: id.to_string(),
        }
    }

    fn read_only(id: &str) -> RecoveryDecision {
        RecoveryDecision::UnknownReadOnlyReexecute {
            journal_id: id.to_string(),
        }
    }

    fn safe(id: &str) -> RecoveryDecision {
        RecoveryDecision::SafeToReexecute {
            journal_id: id.to_string(),
            detail: "intent durable",
        }
    }

    fn settled(id: &str) -> RecoveryDecision {
        RecoveryDecision::ConfirmedSettled {
            journal_id: id.to_string(),
            outcome: crate::ports::ReceiptOutcome::Succeeded,
            dedup_id: Some("d".to_string()),
        }
    }

    #[test]
    fn empty_or_clean_journals_are_recoverable_wait() {
        for decisions in [vec![], vec![safe("a")], vec![settled("a"), safe("b")]] {
            let plan = classify_run_recovery(&decisions);
            assert_eq!(
                plan.category,
                RunRecoveryCategory::RecoverableWait,
                "{decisions:?}"
            );
            assert_eq!(plan.category.name(), "recoverable_wait");
            assert!(plan.subject_journal_ids.is_empty());
            assert!(
                plan.user_reason
                    .contains("no unresolved external side effect"),
                "{}",
                plan.user_reason
            );
            assert!(
                plan.next_actions.iter().any(|a| a.contains("resubmit")),
                "{:?}",
                plan.next_actions
            );
        }
    }

    #[test]
    fn read_only_unknowns_are_retryable_read_only() {
        let plan = classify_run_recovery(&[settled("a"), read_only("b"), safe("c")]);
        assert_eq!(plan.category, RunRecoveryCategory::RetryableReadOnly);
        assert_eq!(plan.category.name(), "retryable_read_only");
        assert_eq!(plan.subject_journal_ids, vec!["b".to_string()]);
        assert!(plan.user_reason.contains("read-only"));
        assert!(plan.next_actions[0].contains("safe to resubmit"));
    }

    #[test]
    fn resumable_unknowns_are_unknown_side_effect_with_their_keys() {
        let plan = classify_run_recovery(&[read_only("a"), resume("b", "key-1"), verify("c")]);
        assert_eq!(plan.category, RunRecoveryCategory::UnknownSideEffect);
        assert_eq!(plan.category.name(), "unknown_side_effect");
        assert_eq!(
            plan.subject_journal_ids,
            vec!["b".to_string(), "c".to_string()]
        );
        assert!(plan.user_reason.contains("idempotency key"));
        assert!(
            plan.next_actions[0].contains("key-1"),
            "the resume action names the SAME key: {:?}",
            plan.next_actions
        );
    }

    #[test]
    fn needs_attention_governs_over_everything_else() {
        let plan = classify_run_recovery(&[read_only("a"), resume("b", "k"), attention("c")]);
        assert_eq!(
            plan.category,
            RunRecoveryCategory::InterruptedNeedsAttention
        );
        assert_eq!(plan.category.name(), "interrupted_needs_attention");
        assert_eq!(plan.subject_journal_ids, vec!["c".to_string()]);
        assert!(plan.user_reason.contains("UNKNOWN outcomes"));
        assert!(plan.user_reason.contains("blind retry"));
        assert!(plan
            .next_actions
            .iter()
            .any(|a| a.contains("verify the outcome")));
    }

    #[test]
    fn no_category_fabricates_a_final_reply() {
        // The plan vocabulary has no "final message" notion at all; the
        // reason text must say the run settles without one.
        for decisions in [
            vec![],
            vec![attention("a")],
            vec![read_only("a")],
            vec![resume("a", "k")],
        ] {
            let plan = classify_run_recovery(&decisions);
            assert!(
                !plan.user_reason.to_lowercase().contains("completed"),
                "no completion claim: {}",
                plan.user_reason
            );
        }
    }

    #[test]
    fn category_vocabulary_is_stable() {
        assert_eq!(
            RunRecoveryCategory::RecoverableWait.name(),
            "recoverable_wait"
        );
        assert_eq!(
            RunRecoveryCategory::RetryableReadOnly.name(),
            "retryable_read_only"
        );
        assert_eq!(
            RunRecoveryCategory::UnknownSideEffect.name(),
            "unknown_side_effect"
        );
        assert_eq!(
            RunRecoveryCategory::InterruptedNeedsAttention.name(),
            "interrupted_needs_attention"
        );
    }

    #[test]
    fn principal_reconstruction_round_trips_the_ownership_key() {
        let roundtrip = |p: &Principal| {
            let rebuilt = principal_from_storage_facts(p.storage_kind(), &p.storage_subject())
                .expect("rebuild");
            assert_eq!(rebuilt.storage_kind(), p.storage_kind());
            assert_eq!(rebuilt.storage_subject(), p.storage_subject());
        };
        roundtrip(&Principal::LocalUser);
        roundtrip(&Principal::Device {
            device_id: "dev-1".to_string(),
            user_id: "user-9".to_string(),
        });
        roundtrip(&Principal::WebSession {
            account_id: "acc-2".to_string(),
        });
        roundtrip(&Principal::Automation {
            surface: "cron".to_string(),
        });
        match principal_from_storage_facts("local_user", "someone_else") {
            Err(StorageError::Corrupted { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
        match principal_from_storage_facts("guest", "x") {
            Err(StorageError::Corrupted { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
