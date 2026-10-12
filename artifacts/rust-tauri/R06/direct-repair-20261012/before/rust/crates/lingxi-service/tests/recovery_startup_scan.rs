//! R03-T07 permanent tests: the startup RECOVERY SCAN (RecoveryCoordinator)
//! over the REAL run database — the honest presentation of a restart
//! (R03-A13's restart half; the kill -9 half lives in
//! `recovery_crash_points.rs`).
//!
//! Test layer (taskbook 05 §1): 契约/服务集成 — the REAL
//! `ServiceState::bootstrap` composition root and the REAL `RunDatabase`.
//!
//! Crash-survivor construction: the durable crash shapes are built through
//! the REAL StoragePort write sequence the run driver produces
//! (`record_run_started` → `record_run_state_change` → journal
//! intent/advance calls) and the previous "process" is then dropped with
//! the run still active — exactly the durable state a killed process
//! leaves behind (T05's live-chain tests prove the drive writes these
//! shapes; the scan tests exercise the scan over them). No run is ever
//! left half-written by a test shortcut: every fact the scan reads was
//! committed by the port methods production uses.

use std::sync::Arc;

use lingxi_kernel::ports::{InvocationPhase, StoragePort};
use lingxi_kernel::recovery::RunRecoveryCategory;
use lingxi_protocol::{RunId, RunStatus};
use lingxi_service::invocations::RunRecoveryReport;
use lingxi_service::recovery::RecoverySettlement;
use lingxi_service::{prepare_layout, ServiceState};

#[path = "recovery_support/harness.rs"]
mod harness;

use harness::{
    boot_with_caps, capability_source_for, config_for, crash_shape, owner_principal,
    synthetic_home, CrashShape,
};

/// R03-A13 restart half + 怎么做 1/2: a crashed `running` run with a
/// non-idempotent unknown side effect restarts into the explainable
/// `interrupted_needs_attention` — no blank row, no fake success, no
/// fabricated final reply, and the user-visible reason + next actions ride
/// the durable terminal event.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_scan_resolves_a_crashed_running_run_with_explainable_interruption() {
    let home = synthetic_home("scan-running");
    // The previous "process": a running run whose journal holds ONE
    // started-without-receipt entry (the classic crash window).
    let (state, run_id) = crash_shape(
        &home,
        CrashShape {
            target: "notify.double".to_string(),
            idempotency_key: None,
            phase: InvocationPhase::Started,
        },
    )
    .await;
    state.storage().close().await.expect("close first process");
    drop(state);

    // THE RESTART (production shape, conservative capabilities).
    let layout = prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .expect("bootstrap runs the recovery scan");

    // The scan report: exactly the dangling run, needs-attention category.
    let scan = state2.recovery_report().expect("scan ran at bootstrap");
    assert_eq!(scan.scanned, 1);
    let outcome = &scan.outcomes[0];
    assert_eq!(outcome.run_id, run_id);
    assert_eq!(outcome.prior_status, RunStatus::Running);
    assert_eq!(
        outcome.category,
        RunRecoveryCategory::InterruptedNeedsAttention
    );
    assert_eq!(outcome.unknown_verdicts_persisted, 1);
    assert!(
        matches!(
            outcome.settlement,
            RecoverySettlement::Written {
                newly_committed: true,
                ..
            }
        ),
        "{:?}",
        outcome.settlement
    );
    assert!(
        outcome.user_reason.contains("notify.double"),
        "the reason names the affected tool: {}",
        outcome.user_reason
    );

    // The durable run row: interrupted with the STABLE reason vocabulary.
    let status: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("query");
    assert_eq!(status.as_deref(), Some("interrupted_needs_attention"));
    let reason: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("query");
    assert_eq!(
        reason.as_deref(),
        Some("interrupted_needs_attention.recovery_unsafe")
    );

    // The user-visible surface: the terminal run_state_changed EVENT
    // carries the rich reason (category + journal facts + next actions).
    let event_reason: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
             'run_state_changed' ORDER BY seq DESC LIMIT 1",
            vec![run_id.clone()],
        )
        .await
        .expect("query");
    let event_reason = event_reason.expect("the recovery terminal event exists");
    assert!(event_reason.contains("interrupted_needs_attention.recovery_unsafe"));
    assert!(
        event_reason.contains("interrupted_needs_attention"),
        "{event_reason}"
    );
    assert!(event_reason.contains("blind retry"), "{event_reason}");
    assert!(
        event_reason.contains("verify the outcome"),
        "{event_reason}"
    );

    // 不虚构模型最终回复: no final message row for the interrupted run.
    let final_message: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT message_id FROM messages WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .expect("query");
    assert!(final_message.is_none(), "no fabricated final reply");

    // The journal verdict was persisted by the scan (started → unknown).
    let journal = state2
        .storage()
        .load_invocation_journal(&RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].phase, InvocationPhase::Unknown);

    state2.storage().close().await.expect("close 2");
    let _ = std::fs::remove_dir_all(&home);
}

/// 怎么做 1 (the classification table): read-only and idempotency-key
/// unknowns classify into their own categories with category-specific
/// next actions — still settled honestly interrupted (R03 has no re-drive
/// loop; the decision data is the handoff).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_scan_classifies_readonly_and_idempotent_unknowns() {
    for (target, key, expected_category, reason_needle, action_needle) in [
        (
            "grep",
            None,
            RunRecoveryCategory::RetryableReadOnly,
            "read-only",
            "safe to resubmit",
        ),
        (
            "ledger.double",
            Some("run_x_tc0001".to_string()),
            RunRecoveryCategory::UnknownSideEffect,
            "idempotency key",
            "run_x_tc0001",
        ),
    ] {
        let home = synthetic_home("scan-caps");
        let (state, run_id) = crash_shape(
            &home,
            CrashShape {
                target: target.to_string(),
                idempotency_key: key.clone(),
                phase: InvocationPhase::Started,
            },
        )
        .await;
        state.storage().close().await.expect("close");
        drop(state);

        // Restart with a capability source that VERIFIES this target's
        // recovery capability (R04's registry replaces the resolution).
        let caps = capability_source_for(target);
        let state2 = boot_with_caps(&home, caps).await;
        let scan = state2.recovery_report().expect("scan");
        assert_eq!(scan.scanned, 1, "{target}");
        let outcome = &scan.outcomes[0];
        assert_eq!(outcome.category, expected_category, "{target}");
        assert!(outcome.user_reason.contains(reason_needle), "{target}");
        assert!(
            outcome
                .next_actions
                .iter()
                .any(|a| a.contains(action_needle)),
            "{target}: {:?}",
            outcome.next_actions
        );
        // Still the honest interrupted terminal (never completed).
        let status: Option<String> = state2
            .storage()
            .query_one_text(
                "SELECT status FROM runs WHERE run_id = ?1",
                vec![run_id.clone()],
            )
            .await
            .expect("query");
        assert_eq!(
            status.as_deref(),
            Some("interrupted_needs_attention"),
            "{target}"
        );
        state2.storage().close().await.expect("close 2");
        let _ = std::fs::remove_dir_all(&home);
    }
}

/// A run whose cancellation was already in flight completes its OWN
/// requested outcome (`cancelled`), and terminal rows are never revived
/// (终态不复活, T01 contract): the completed sibling is untouched.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_scan_completes_cancelling_runs_and_respects_terminal_rows() {
    let home = synthetic_home("scan-cancelling");
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .expect("bootstrap");

    // Run A: parked in `cancelling` (the durable two-phase leg) with a
    // started-no-receipt journal entry.
    let ctx_a = harness::run_context("run_a");
    state
        .storage()
        .record_run_started(&ctx_a, 1_000)
        .await
        .expect("start a");
    harness::journal_started(&state, &ctx_a, "notify.double", None).await;
    state
        .storage()
        .record_run_state_change(
            &ctx_a,
            RunStatus::Running,
            RunStatus::Cancelling,
            None,
            2_000,
        )
        .await
        .expect("cancelling leg");

    // Run B: fully settled (completed) BEFORE the crash.
    let ctx_b = harness::run_context("run_b");
    state
        .storage()
        .record_run_started(&ctx_b, 1_500)
        .await
        .expect("start b");
    use lingxi_kernel::ports::{KeyEvent, RunOutcome};
    use lingxi_protocol::{EventId, EventPayload, KnownEventPayload, RunStateChangedPayload};
    state
        .storage()
        .commit_run_outcome(
            &ctx_b,
            RunOutcome {
                status: RunStatus::Completed,
                reason: Some("completed.no_final.no_provider_configured".to_string()),
                key_events: vec![KeyEvent {
                    event_id: EventId::new("run_b-done".to_string()),
                    payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                        RunStateChangedPayload {
                            from: RunStatus::Running,
                            to: RunStatus::Completed,
                            reason: None,
                        },
                    )),
                }],
                final_message: None,
            },
            1_600,
        )
        .await
        .expect("settle b");
    let events_b_before: Option<String> = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            vec!["run_b".to_string()],
        )
        .await
        .expect("count");
    state.storage().close().await.expect("close");
    drop(state);

    // THE RESTART.
    let layout2 = prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout2)
        .await
        .expect("bootstrap 2");
    let scan = state2.recovery_report().expect("scan");
    // Only the cancelling run was scanned — the completed run is terminal,
    // not recovery's business.
    assert_eq!(scan.scanned, 1);
    let outcome = &scan.outcomes[0];
    assert_eq!(outcome.run_id, "run_a");
    assert_eq!(outcome.prior_status, RunStatus::Cancelling);
    assert!(matches!(
        outcome.settlement,
        RecoverySettlement::Written {
            target: RunStatus::Cancelled,
            ..
        }
    ));
    let status_a: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec!["run_a".to_string()],
        )
        .await
        .expect("query");
    assert_eq!(status_a.as_deref(), Some("cancelled"));
    // The journal pass still ran for the cancelling run (unknown verdict).
    let journal = state2
        .storage()
        .load_invocation_journal(&RunId::new("run_a".to_string()))
        .await
        .expect("journal");
    assert_eq!(journal[0].phase, InvocationPhase::Unknown);

    // Terminal row untouched: same status, same events.
    let status_b: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec!["run_b".to_string()],
        )
        .await
        .expect("query");
    assert_eq!(status_b.as_deref(), Some("completed"));
    let events_b_after: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            vec!["run_b".to_string()],
        )
        .await
        .expect("count");
    assert_eq!(
        events_b_after, events_b_before,
        "no new events for a terminal run"
    );

    state2.storage().close().await.expect("close 2");
    let _ = std::fs::remove_dir_all(&home);
}

/// A run parked in `waiting_approval` walks the LEGAL machine path to its
/// interruption (waiting_approval → running → interrupted), with the
/// intermediate leg reason-marked as the recovery walk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn waiting_approval_run_walks_the_legal_path_to_interruption() {
    let home = synthetic_home("scan-waiting");
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .expect("bootstrap");
    let ctx = harness::run_context("run_w");
    state
        .storage()
        .record_run_started(&ctx, 1_000)
        .await
        .expect("start");
    state
        .storage()
        .record_run_state_change(
            &ctx,
            RunStatus::Running,
            RunStatus::WaitingApproval,
            Some("approval requested".to_string()),
            1_100,
        )
        .await
        .expect("waiting leg");
    state.storage().close().await.expect("close");
    drop(state);

    let layout2 = prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout2)
        .await
        .expect("bootstrap 2");
    let scan = state2.recovery_report().expect("scan");
    assert_eq!(scan.scanned, 1);
    assert_eq!(scan.outcomes[0].prior_status, RunStatus::WaitingApproval);
    let status: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec!["run_w".to_string()],
        )
        .await
        .expect("query");
    assert_eq!(status.as_deref(), Some("interrupted_needs_attention"));
    // The durable history shows the marked recovery leg.
    let leg: Option<String> = state2
        .storage()
        .query_one_text(
            "SELECT payload_json FROM key_events WHERE run_id = ?1 AND payload_json LIKE \
             '%recovery_scan%' ORDER BY seq DESC LIMIT 1",
            vec!["run_w".to_string()],
        )
        .await
        .expect("query");
    assert!(leg.is_some(), "the waiting→running leg is reason-marked");

    state2.storage().close().await.expect("close 2");
    let _ = std::fs::remove_dir_all(&home);
}

/// The scan is idempotent across DOUBLE restarts: after the first scan
/// settled everything, the second scan finds nothing and rewrites nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_scan_is_idempotent_across_double_restarts() {
    let home = synthetic_home("scan-idempotent");
    let (state, _run_id) = crash_shape(
        &home,
        CrashShape {
            target: "notify.double".to_string(),
            idempotency_key: None,
            phase: InvocationPhase::Started,
        },
    )
    .await;
    state.storage().close().await.expect("close");
    drop(state);

    let layout = prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .expect("bootstrap 2");
    assert_eq!(state2.recovery_report().expect("scan 1").scanned, 1);
    let events_after_first: Option<String> = state2
        .storage()
        .query_one_text("SELECT COUNT(*) FROM key_events", vec![])
        .await
        .expect("count");
    state2.storage().close().await.expect("close 2");
    drop(state2);

    let layout3 = prepare_layout(&home).expect("layout 3");
    let state3 = ServiceState::bootstrap(config_for(&home), &layout3)
        .await
        .expect("bootstrap 3 (nothing left to recover)");
    let scan3 = state3.recovery_report().expect("scan 3");
    assert_eq!(scan3.scanned, 0, "terminal rows are never revived");
    let events_after_third: Option<String> = state3
        .storage()
        .query_one_text("SELECT COUNT(*) FROM key_events", vec![])
        .await
        .expect("count");
    assert_eq!(
        events_after_third, events_after_first,
        "the second scan wrote nothing"
    );
    state3.storage().close().await.expect("close 3");
    let _ = std::fs::remove_dir_all(&home);
}

/// The exit-hook half of the honesty loop (T06 residue → T07 scan): a run
/// left active by a timed-out exit drain is exactly what the next scan
/// resolves. This test drives the real
/// [`lingxi_service::invocations::recover_run_invocations`] replay AFTER a
/// scan-settled journal to prove the two surfaces agree (the explicit pass
/// replays with nothing new persisted).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_recovery_pass_replays_idempotently_after_the_scan() {
    let home = synthetic_home("scan-replay");
    let (state, run_id) = crash_shape(
        &home,
        CrashShape {
            target: "notify.double".to_string(),
            idempotency_key: None,
            phase: InvocationPhase::Started,
        },
    )
    .await;
    state.storage().close().await.expect("close");
    drop(state);

    let layout = prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .expect("bootstrap 2");
    let report: RunRecoveryReport = lingxi_service::invocations::recover_run_invocations(
        state2.storage().as_ref(),
        &run_id,
        &lingxi_service::invocations::ConservativeCapabilities,
        9_999,
    )
    .await
    .expect("explicit pass");
    assert_eq!(report.entries.len(), 1);
    assert!(
        !report.entries[0].unknown_verdict_persisted,
        "the scan already persisted the verdict — the explicit pass replays"
    );
    state2.storage().close().await.expect("close 2");
    let _ = std::fs::remove_dir_all(&home);
}

/// A08-adjacent admission check kept green through the scan change: a
/// submission AFTER the restart works normally on the recovered store
/// (the intake is open again in the new process — the closure is
/// per-process, not durable).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn submissions_work_normally_after_a_recovery_scan() {
    let home = synthetic_home("scan-then-submit");
    let (state, _run_id) = crash_shape(
        &home,
        CrashShape {
            target: "notify.double".to_string(),
            idempotency_key: None,
            phase: InvocationPhase::Started,
        },
    )
    .await;
    state.storage().close().await.expect("close");
    drop(state);

    let layout = prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .expect("bootstrap 2");
    let storage = Arc::clone(state2.storage());
    let accepted = state2
        .sessions()
        .execute_for(
            storage.as_ref(),
            state2.events(),
            state2.runs(),
            &owner_principal(),
            "sess_local_beta",
            "post-recovery submission",
            9_000,
        )
        .await
        .expect("the restarted service accepts new work (fresh intake)");
    assert!(!accepted.replayed);
    state2.storage().close().await.expect("close 2");
    let _ = std::fs::remove_dir_all(&home);
}
