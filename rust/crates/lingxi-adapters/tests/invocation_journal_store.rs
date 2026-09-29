//! R03-T05 storage contract: the InvocationJournal against the REAL
//! `RunDatabase` (migration V3 table, single-writer queue, one transaction
//! per write). Covers the receipt lifecycle ladder, the write-order
//!witness (intent-only rows recover as 未执行; `started` without a receipt
//! recovers as unknown), idempotency/conflict rules and the recovery
//! unknown-verdict close.
//!
//! Test tier: contract against the real SQLite store (no provider, no
//! service wiring — the drive-chain write ORDER is covered by the
//! lingxi-service integration suite `invocation_journal.rs`).

use lingxi_adapters::storage::{RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::invocation::{invocation_recovery_class, RecoveryClass, ToolRecoveryCapability};
use lingxi_kernel::ports::{
    InvocationIntent, InvocationPhase, InvocationReceipt, ReceiptOutcome, StorageError, StoragePort,
};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{RunId, SessionId, ToolCallId};

fn temp_db(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-r03t05-store-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

async fn seeded_store(path: &std::path::Path) -> RunDatabase {
    let db = RunDatabase::open(path, StoreOptions::default())
        .await
        .expect("open (applies migrations incl. V3)");
    db.ensure_session_seed(vec![SessionRow {
        session_id: "sess_a".into(),
        agent_id: "lingxi".into(),
        owner_user_id: "user_local".into(),
        title: "t".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .expect("seed");
    db
}

fn ctx(run_id: &str) -> RunContext {
    let run = RunId::new(run_id.to_string());
    RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess_a".to_string()),
        attempt: lingxi_kernel::attempt_id(&run, 1),
        run_id: run,
        generation: 1,
    }
}

async fn started_run(db: &RunDatabase, run_id: &str) {
    db.record_run_started(&ctx(run_id), 1_000)
        .await
        .expect("run started");
}

fn intent(journal_id: &str, key: Option<&str>) -> InvocationIntent {
    InvocationIntent {
        journal_id: ToolCallId::new(journal_id.to_string()),
        target: "send_message".to_string(),
        args_digest: "ab12".to_string(),
        args_summary: Some("send to #general".to_string()),
        idempotency_key: key.map(|k| k.to_string()),
    }
}

fn receipt(outcome: ReceiptOutcome, dispatched: bool) -> InvocationReceipt {
    InvocationReceipt {
        outcome,
        detail: match outcome {
            ReceiptOutcome::Succeeded => "external content digest dd".to_string(),
            ReceiptOutcome::Failed => "forbidden: not approved".to_string(),
            ReceiptOutcome::Unknown => "receipt lost".to_string(),
        },
        dedup_id: (outcome == ReceiptOutcome::Succeeded).then(|| "dd".to_string()),
        dispatched,
    }
}

fn journal_id(id: &str) -> ToolCallId {
    ToolCallId::new(id.to_string())
}

/// The receipt ladder: intent lands `prepared`; authorized/started advance
/// in order; the receipt closes the entry; the loaded entry carries the
/// FULL binding (owner, run/attempt/generation, target, args digest,
/// idempotency key).
#[tokio::test]
async fn journal_lifecycle_binds_identity_and_follows_the_phase_ladder() {
    let path = temp_db("lifecycle");
    let db = seeded_store(&path).await;
    started_run(&db, "run_j1").await;

    db.record_invocation_intent(
        &ctx("run_j1"),
        intent("run_j1-tc0001", Some("run_j1-tc0001")),
        2_000,
    )
    .await
    .expect("intent");
    let mut seen = db
        .load_invocation_journal(&RunId::new("run_j1".to_string()))
        .await
        .expect("load");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].phase, InvocationPhase::Prepared);
    assert_eq!(seen[0].owner_kind, "local_user");
    assert_eq!(seen[0].owner_subject, "user_local");
    assert_eq!(seen[0].attempt, "run_j1#a1");
    assert_eq!(seen[0].generation, 1);
    assert_eq!(seen[0].target, "send_message");
    assert_eq!(seen[0].args_digest, "ab12");
    assert_eq!(
        seen[0].idempotency_key.as_deref(),
        Some("run_j1-tc0001"),
        "the idempotency key is a durable receipt fact"
    );

    db.advance_invocation(
        &ctx("run_j1"),
        &journal_id("run_j1-tc0001"),
        InvocationPhase::Authorized,
        2_001,
    )
    .await
    .expect("authorized");
    db.advance_invocation(
        &ctx("run_j1"),
        &journal_id("run_j1-tc0001"),
        InvocationPhase::Started,
        2_002,
    )
    .await
    .expect("started");
    db.record_invocation_receipt(
        &ctx("run_j1"),
        &journal_id("run_j1-tc0001"),
        receipt(ReceiptOutcome::Succeeded, true),
        2_003,
    )
    .await
    .expect("receipt");
    seen = db
        .load_invocation_journal(&RunId::new("run_j1".to_string()))
        .await
        .expect("reload");
    assert_eq!(seen[0].phase, InvocationPhase::Succeeded);
    let stored = seen[0].receipt.as_ref().expect("receipt present");
    assert_eq!(stored.outcome, ReceiptOutcome::Succeeded);
    assert_eq!(stored.dedup_id.as_deref(), Some("dd"));
    assert!(stored.dispatched);
    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(&path);
}

/// Re-advancing to the CURRENT phase and re-recording the IDENTICAL
/// receipt are idempotent replays (a retry after a lost response writes
/// nothing); a CONFLICTING receipt under a closed entry is refused.
#[tokio::test]
async fn journal_replays_are_idempotent_and_conflicts_are_loud() {
    let path = temp_db("idempotent");
    let db = seeded_store(&path).await;
    started_run(&db, "run_j2").await;
    let ctx = ctx("run_j2");
    db.record_invocation_intent(&ctx, intent("run_j2-tc0001", None), 1_000)
        .await
        .expect("intent");
    // Identical intent replays.
    db.record_invocation_intent(&ctx, intent("run_j2-tc0001", None), 1_001)
        .await
        .expect("identical intent replays");
    // A conflicting intent under the same journal id is refused.
    match db
        .record_invocation_intent(&ctx, intent("run_j2-tc0001", Some("other")), 1_002)
        .await
    {
        Err(StorageError::Conflict { detail }) => {
            assert!(detail.contains("already holds"), "detail: {detail}")
        }
        other => panic!("expected conflict, got {other:?}"),
    }
    db.advance_invocation(
        &ctx,
        &journal_id("run_j2-tc0001"),
        InvocationPhase::Authorized,
        1_003,
    )
    .await
    .expect("authorized");
    // Same-phase re-advance is a no-op.
    db.advance_invocation(
        &ctx,
        &journal_id("run_j2-tc0001"),
        InvocationPhase::Authorized,
        1_004,
    )
    .await
    .expect("same-phase replay");
    // Skipping ahead straight to started from authorized is the legal
    // compressed ladder (prepared→started), and re-advance replays.
    db.advance_invocation(
        &ctx,
        &journal_id("run_j2-tc0001"),
        InvocationPhase::Started,
        1_005,
    )
    .await
    .expect("started");
    db.advance_invocation(
        &ctx,
        &journal_id("run_j2-tc0001"),
        InvocationPhase::Started,
        1_006,
    )
    .await
    .expect("same-phase replay");
    // Identical receipt replays; a different one conflicts.
    db.record_invocation_receipt(
        &ctx,
        &journal_id("run_j2-tc0001"),
        receipt(ReceiptOutcome::Failed, true),
        1_007,
    )
    .await
    .expect("close");
    db.record_invocation_receipt(
        &ctx,
        &journal_id("run_j2-tc0001"),
        receipt(ReceiptOutcome::Failed, true),
        1_008,
    )
    .await
    .expect("identical receipt replays");
    match db
        .record_invocation_receipt(
            &ctx,
            &journal_id("run_j2-tc0001"),
            receipt(ReceiptOutcome::Unknown, true),
            1_009,
        )
        .await
    {
        Err(StorageError::Conflict { detail }) => {
            assert!(detail.contains("already holds"), "detail: {detail}")
        }
        other => panic!("expected conflict, got {other:?}"),
    }
    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(&path);
}

/// The ladder is enforced: illegal advances and illegal receipt shapes are
/// loud InvalidRequests, never absorbed.
#[tokio::test]
async fn journal_rejects_illegal_ladder_moves_and_receipt_shapes() {
    let path = temp_db("illegal");
    let db = seeded_store(&path).await;
    started_run(&db, "run_j3").await;
    let ctx = ctx("run_j3");
    db.record_invocation_intent(&ctx, intent("run_j3-tc0001", None), 1_000)
        .await
        .expect("intent tc1");
    db.record_invocation_intent(&ctx, intent("run_j3-tc0002", None), 1_010)
        .await
        .expect("intent tc2");
    // advance_invocation never targets a receipt phase.
    for bad in [
        InvocationPhase::Prepared,
        InvocationPhase::Succeeded,
        InvocationPhase::Failed,
        InvocationPhase::Unknown,
    ] {
        match db
            .advance_invocation(&ctx, &journal_id("run_j3-tc0001"), bad, 1_001)
            .await
        {
            Err(StorageError::InvalidRequest { detail }) => {
                assert!(detail.contains("authorized/started"), "detail: {detail}")
            }
            other => panic!("expected invalid request for {bad:?}, got {other:?}"),
        }
    }
    // A SUCCEEDED receipt from a phase that proves no external dispatch
    // (prepared) is refused — success must be witnessed by `started`
    // (or settle a formerly-unknown entry).
    match db
        .record_invocation_receipt(
            &ctx,
            &journal_id("run_j3-tc0002"),
            receipt(ReceiptOutcome::Succeeded, true),
            1_004,
        )
        .await
    {
        Err(StorageError::InvalidRequest { detail }) => {
            assert!(detail.contains("succeeded receipt"), "detail: {detail}")
        }
        other => panic!("expected invalid request, got {other:?}"),
    }
    // A failed receipt closes even from prepared (never-dispatched
    // failure, e.g. a rejected approval).
    db.record_invocation_receipt(
        &ctx,
        &journal_id("run_j3-tc0002"),
        receipt(ReceiptOutcome::Failed, false),
        1_005,
    )
    .await
    .expect("never-dispatched failure closes");
    // Regression is illegal: once started, advancing back to authorized is
    // refused by the ladder.
    db.advance_invocation(
        &ctx,
        &journal_id("run_j3-tc0001"),
        InvocationPhase::Authorized,
        1_002,
    )
    .await
    .expect("authorized");
    db.advance_invocation(
        &ctx,
        &journal_id("run_j3-tc0001"),
        InvocationPhase::Started,
        1_003,
    )
    .await
    .expect("started");
    match db
        .advance_invocation(
            &ctx,
            &journal_id("run_j3-tc0001"),
            InvocationPhase::Authorized,
            1_003,
        )
        .await
    {
        Err(StorageError::InvalidRequest { detail }) => {
            assert!(detail.contains("cannot advance"), "detail: {detail}")
        }
        other => panic!("expected invalid request, got {other:?}"),
    }
    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(&path);
}

/// The recovery classification read over the durable phases: intent-only
/// rows are 未执行; `started` without a receipt is the crash window
/// (unknown); a closed receipt settles its class.
#[tokio::test]
async fn journal_recovery_classes_follow_the_durable_phase() {
    let path = temp_db("classes");
    let db = seeded_store(&path).await;
    started_run(&db, "run_j4").await;
    let ctx = ctx("run_j4");
    // tc1: intent only (prepared). tc2: authorized. tc3: started, no
    // receipt (the crash window). tc4: started then closed succeeded.
    db.record_invocation_intent(&ctx, intent("run_j4-tc0001", None), 1_000)
        .await
        .expect("t1");
    db.record_invocation_intent(&ctx, intent("run_j4-tc0002", None), 1_001)
        .await
        .expect("t2");
    db.advance_invocation(
        &ctx,
        &journal_id("run_j4-tc0002"),
        InvocationPhase::Authorized,
        1_002,
    )
    .await
    .expect("a2");
    db.record_invocation_intent(&ctx, intent("run_j4-tc0003", Some("run_j4-tc0003")), 1_003)
        .await
        .expect("t3");
    db.advance_invocation(
        &ctx,
        &journal_id("run_j4-tc0003"),
        InvocationPhase::Authorized,
        1_004,
    )
    .await
    .expect("a3");
    db.advance_invocation(
        &ctx,
        &journal_id("run_j4-tc0003"),
        InvocationPhase::Started,
        1_005,
    )
    .await
    .expect("s3");
    db.record_invocation_intent(&ctx, intent("run_j4-tc0004", None), 1_006)
        .await
        .expect("t4");
    db.advance_invocation(
        &ctx,
        &journal_id("run_j4-tc0004"),
        InvocationPhase::Authorized,
        1_007,
    )
    .await
    .expect("a4");
    db.advance_invocation(
        &ctx,
        &journal_id("run_j4-tc0004"),
        InvocationPhase::Started,
        1_008,
    )
    .await
    .expect("s4");
    db.record_invocation_receipt(
        &ctx,
        &journal_id("run_j4-tc0004"),
        receipt(ReceiptOutcome::Succeeded, true),
        1_009,
    )
    .await
    .expect("r4");

    let entries = db
        .load_invocation_journal(&RunId::new("run_j4".to_string()))
        .await
        .expect("load");
    assert_eq!(entries.len(), 4, "oldest first, one row per invocation");
    assert_eq!(
        invocation_recovery_class(&entries[0]),
        RecoveryClass::NotExecuted
    );
    assert_eq!(
        invocation_recovery_class(&entries[1]),
        RecoveryClass::NotExecuted
    );
    assert_eq!(
        invocation_recovery_class(&entries[2]),
        RecoveryClass::Unknown
    );
    assert_eq!(
        invocation_recovery_class(&entries[3]),
        RecoveryClass::ConfirmedCompleted
    );

    // The recovery unknown-verdict close: legal exactly for the crash
    // window (started, no receipt); idempotent on re-run; refused for
    // anything else.
    db.record_invocation_unknown(
        &journal_id("run_j4-tc0003"),
        "crash window".to_string(),
        2_000,
    )
    .await
    .expect("mark unknown");
    db.record_invocation_unknown(
        &journal_id("run_j4-tc0003"),
        "crash window".to_string(),
        2_001,
    )
    .await
    .expect("re-mark replays idempotently");
    for other in ["run_j4-tc0001", "run_j4-tc0002", "run_j4-tc0004"] {
        match db
            .record_invocation_unknown(&journal_id(other), "misuse".to_string(), 2_002)
            .await
        {
            Err(StorageError::InvalidRequest { detail }) => {
                assert!(detail.contains("only a started entry"), "detail: {detail}")
            }
            result => panic!("expected refusal for {other}, got {result:?}"),
        }
    }
    let entries = db
        .load_invocation_journal(&RunId::new("run_j4".to_string()))
        .await
        .expect("reload");
    assert_eq!(entries[2].phase, InvocationPhase::Unknown);
    assert_eq!(
        entries[2]
            .receipt
            .as_ref()
            .expect("verdict receipt")
            .outcome,
        ReceiptOutcome::Unknown
    );
    // The unknown verdict is NOT final: a VERIFIED receipt settles it.
    db.record_invocation_receipt(
        &ctx,
        &journal_id("run_j4-tc0003"),
        receipt(ReceiptOutcome::Succeeded, true),
        3_000,
    )
    .await
    .expect("verified settlement of a formerly-unknown entry");
    let entries = db
        .load_invocation_journal(&RunId::new("run_j4".to_string()))
        .await
        .expect("final load");
    assert_eq!(entries[2].phase, InvocationPhase::Succeeded);
    assert_eq!(
        invocation_recovery_class(&entries[2]),
        RecoveryClass::ConfirmedCompleted
    );
    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(&path);
}

/// Cross-owner journal writes are boundary violations (loud conflict), and
/// the kernel decision ladder maps the crash-window entry correctly under
/// the idempotent capability (smoke check of the seam this store serves).
#[tokio::test]
async fn journal_refuses_cross_owner_writes_and_serves_the_decision_seam() {
    use lingxi_kernel::invocation::classify_invocation_recovery;
    let path = temp_db("owner");
    let db = seeded_store(&path).await;
    started_run(&db, "run_j5").await;
    db.record_invocation_intent(
        &ctx("run_j5"),
        intent("run_j5-tc0001", Some("run_j5-tc0001")),
        1_000,
    )
    .await
    .expect("intent");
    // A foreign principal (web session) writing under this run's journal
    // id is refused.
    let run = RunId::new("run_j5".to_string());
    let foreign = RunContext {
        principal: Principal::WebSession {
            account_id: "acct_9".to_string(),
        },
        session_id: SessionId::new("sess_a".to_string()),
        run_id: run,
        attempt: lingxi_kernel::attempt_id(&RunId::new("run_j5".to_string()), 1),
        generation: 1,
    };
    match db
        .advance_invocation(
            &foreign,
            &journal_id("run_j5-tc0001"),
            InvocationPhase::Authorized,
            1_001,
        )
        .await
    {
        Err(StorageError::Conflict { detail }) => {
            assert!(detail.contains("belongs to"), "detail: {detail}")
        }
        other => panic!("expected conflict, got {other:?}"),
    }
    // The decision seam: crash-window entry + idempotent capability →
    // resume with the SAME key.
    db.advance_invocation(
        &ctx("run_j5"),
        &journal_id("run_j5-tc0001"),
        InvocationPhase::Authorized,
        1_002,
    )
    .await
    .expect("authorized");
    db.advance_invocation(
        &ctx("run_j5"),
        &journal_id("run_j5-tc0001"),
        InvocationPhase::Started,
        1_003,
    )
    .await
    .expect("started");
    let entries = db
        .load_invocation_journal(&RunId::new("run_j5".to_string()))
        .await
        .expect("load");
    let decision = classify_invocation_recovery(
        &entries[0],
        &ToolRecoveryCapability {
            honors_idempotency_key: true,
            ..ToolRecoveryCapability::CONSERVATIVE
        },
    );
    match decision {
        lingxi_kernel::invocation::RecoveryDecision::UnknownResumeWithIdempotencyKey {
            key,
            ..
        } => {
            assert_eq!(key, "run_j5-tc0001")
        }
        other => panic!("expected resume-with-key, got {other:?}"),
    }
    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(&path);
}
