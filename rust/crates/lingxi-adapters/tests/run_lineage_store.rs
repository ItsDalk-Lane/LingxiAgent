//! R03-T06 storage contract: run LINEAGE (migration V4 table) against
//! the REAL `RunDatabase` (single-writer queue, one transaction per
//! write). Covers the four-part identity round-trip
//! (parentRunId/origin/sourceMessageId/causeId), immutability
//! (identical re-record = idempotent replay; a DIFFERENT lineage is a
//! loud Conflict — a run's parentage is never rewritten), the
//! creation-fact rule (terminal runs refuse lineage) and the loud
//! unknown-origin parse.
//!
//! Test tier: contract against the real SQLite store (no provider, no
//! service wiring — the live-chain recording is covered by the
//! lingxi-service integration suites).

use lingxi_adapters::storage::{RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::ports::{KeyEvent, RunOutcome, StorageError, StoragePort};
use lingxi_kernel::subagent::{RunLineage, RunOrigin};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{EventId, EventPayload, KnownEventPayload, RunId, SessionId};

fn temp_db(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-r03t06-lineage-{tag}-{}-{}",
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
        .expect("open (applies migrations incl. V4)");
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

fn child_lineage(parent: &str) -> RunLineage {
    RunLineage {
        parent_run_id: Some(RunId::new(parent.to_string())),
        origin: RunOrigin::Subagent,
        source_message_id: Some(format!("{parent}-mc0001")),
        cause_id: Some(format!("{parent}-tc0001")),
    }
}

#[tokio::test]
async fn lineage_round_trips_and_is_immutable() {
    let path = temp_db("roundtrip");
    let db = seeded_store(&path).await;
    started_run(&db, "run_parent").await;
    started_run(&db, "run_child").await;

    // Nothing recorded yet.
    assert_eq!(
        db.load_run_lineage(&RunId::new("run_child".to_string()))
            .await
            .expect("load"),
        None
    );

    // Record the four-part identity.
    db.record_run_lineage(&ctx("run_child"), child_lineage("run_parent"), 1_000)
        .await
        .expect("record lineage");
    let loaded = db
        .load_run_lineage(&RunId::new("run_child".to_string()))
        .await
        .expect("load lineage");
    assert_eq!(loaded, Some(child_lineage("run_parent")));

    // The IDENTICAL lineage replays idempotently.
    db.record_run_lineage(&ctx("run_child"), child_lineage("run_parent"), 2_000)
        .await
        .expect("identical replay");

    // A DIFFERENT lineage is a loud Conflict — parentage is never
    // rewritten (whatever the caller claims).
    let mut forged = child_lineage("run_other");
    forged.cause_id = Some("run_other-tc9999".to_string());
    match db
        .record_run_lineage(&ctx("run_child"), forged, 3_000)
        .await
    {
        Err(StorageError::Conflict { detail }) => {
            assert!(detail.contains("refusing to rewrite"), "{detail}");
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
    // The original lineage is intact after the refused rewrite.
    assert_eq!(
        db.load_run_lineage(&RunId::new("run_child".to_string()))
            .await
            .expect("load"),
        Some(child_lineage("run_parent"))
    );

    // A user-submission lineage (no parent) round-trips too.
    started_run(&db, "run_user").await;
    let user_lineage = RunLineage::user_submission(Some("req-7"));
    db.record_run_lineage(&ctx("run_user"), user_lineage.clone(), 1_000)
        .await
        .expect("record user lineage");
    assert_eq!(
        db.load_run_lineage(&RunId::new("run_user".to_string()))
            .await
            .expect("load"),
        Some(user_lineage)
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn lineage_requires_an_existing_owned_active_run() {
    let path = temp_db("guards");
    let db = seeded_store(&path).await;
    started_run(&db, "run_live").await;

    // Unknown run: loud InvalidRequest.
    match db
        .record_run_lineage(&ctx("run_missing"), child_lineage("run_live"), 1_000)
        .await
    {
        Err(StorageError::InvalidRequest { detail }) => {
            assert!(detail.contains("no row"), "{detail}");
        }
        other => panic!("expected InvalidRequest, got {other:?}"),
    }

    // Terminal run: lineage is a creation fact, never a post-mortem
    // annotation.
    db.commit_run_outcome(
        &ctx("run_live"),
        RunOutcome {
            status: lingxi_protocol::RunStatus::Completed,
            reason: Some("completed.with_final".to_string()),
            key_events: vec![KeyEvent {
                event_id: EventId::new("run_live-done".to_string()),
                payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                    lingxi_protocol::RunStateChangedPayload {
                        from: lingxi_protocol::RunStatus::Running,
                        to: lingxi_protocol::RunStatus::Completed,
                        reason: Some("completed.with_final".to_string()),
                    },
                )),
            }],
            final_message: None,
        },
        2_000,
    )
    .await
    .expect("settle run");
    match db
        .record_run_lineage(&ctx("run_live"), child_lineage("run_other"), 3_000)
        .await
    {
        Err(StorageError::InvalidRequest { detail }) => {
            assert!(detail.contains("terminal"), "{detail}");
        }
        other => panic!("expected InvalidRequest, got {other:?}"),
    }

    // A corrupted origin value is loud, never a guess.
    started_run(&db, "run_bad").await;
    let conn = rusqlite::Connection::open(&path).expect("open direct");
    conn.execute(
        "INSERT INTO run_lineage (run_id, origin, recorded_at_unix_ms) VALUES ('run_bad', \
         'root', 1)",
        [],
    )
    .expect("insert corrupted lineage");
    drop(conn);
    match db
        .load_run_lineage(&RunId::new("run_bad".to_string()))
        .await
    {
        Err(StorageError::Corrupted { detail }) => {
            assert!(detail.contains("not in the known vocabulary"), "{detail}");
        }
        other => panic!("expected Corrupted, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}
