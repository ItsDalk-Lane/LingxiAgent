//! R03-T07 storage contract: the NON-TERMINAL run listing
//! (`RunDatabase::list_active_runs`) against the REAL database — the
//! recovery scan's only enumeration source.
//!
//! Test tier: contract against the real SQLite store (no provider, no
//! service wiring — the scan that consumes the listing is covered by the
//! lingxi-service recovery suites).

use lingxi_adapters::storage::{RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::ports::{KeyEvent, RunOutcome, StoragePort};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{
    EventId, EventPayload, KnownEventPayload, RunId, RunStateChangedPayload, RunStatus, SessionId,
};

fn temp_db(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-r03t07-listing-{tag}-{}-{}",
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
        .expect("open (applies all migrations)");
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

async fn started_run(db: &RunDatabase, run_id: &str, at_ms: u64) {
    db.record_run_started(&ctx(run_id), at_ms)
        .await
        .expect("run started");
}

#[tokio::test]
async fn lists_only_non_terminal_runs_with_full_recovery_facts() {
    let path = temp_db("active");
    let db = seeded_store(&path).await;

    // run_active stays running; run_wait parks in waiting_approval;
    // run_cancel enters cancelling; run_done settles completed.
    started_run(&db, "run_active", 1_000).await;
    started_run(&db, "run_wait", 2_000).await;
    db.record_run_state_change(
        &ctx("run_wait"),
        RunStatus::Running,
        RunStatus::WaitingApproval,
        None,
        2_100,
    )
    .await
    .expect("waiting leg");
    started_run(&db, "run_cancel", 3_000).await;
    db.record_run_state_change(
        &ctx("run_cancel"),
        RunStatus::Running,
        RunStatus::Cancelling,
        None,
        3_100,
    )
    .await
    .expect("cancelling leg");
    started_run(&db, "run_done", 4_000).await;
    db.commit_run_outcome(
        &ctx("run_done"),
        RunOutcome {
            status: RunStatus::Completed,
            reason: Some("completed.no_final.no_provider_configured".to_string()),
            key_events: vec![KeyEvent {
                event_id: EventId::new("run_done-done".to_string()),
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
        4_100,
    )
    .await
    .expect("settle done");

    let active = db.list_active_runs().await.expect("list");
    assert_eq!(active.len(), 3, "terminal rows are never listed");
    // Deterministic order (creation time).
    let ids: Vec<&str> = active.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(ids, vec!["run_active", "run_wait", "run_cancel"]);
    // The full recovery-fact shape on the first row: identity, ownership
    // key, status, generation, CURRENT attempt.
    let first = &active[0];
    assert_eq!(first.session_id, "sess_a");
    assert_eq!(first.owner_kind, "local_user");
    assert_eq!(first.owner_subject, "user_local");
    assert_eq!(first.status, RunStatus::Running);
    assert_eq!(first.generation, 1);
    assert_eq!(first.current_attempt, "run_active#a1");
    assert_eq!(active[1].status, RunStatus::WaitingApproval);
    assert_eq!(active[2].status, RunStatus::Cancelling);

    db.close().await.expect("close");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_run_without_attempt_rows_is_loud_corruption() {
    // A run row whose attempt rows are missing disagrees with
    // `record_run_started`'s contract (attempt #1 always opens) — the
    // listing refuses to guess instead of fabricating an attempt.
    let path = temp_db("corrupt-attempt");
    let db = seeded_store(&path).await;
    started_run(&db, "run_ok", 1_000).await;
    db.close().await.expect("close");

    let external = rusqlite::Connection::open(&path).expect("external conn");
    external
        .execute(
            "INSERT INTO runs (run_id, session_id, owner_kind, owner_subject, principal_id, \
             attempt_count, status, generation, created_at_unix_ms, updated_at_unix_ms, \
             last_event_seq) \
             VALUES ('run_bad', 'sess_a', 'local_user', 'user_local', 'p', 0, 'running', 1, \
             1, 1, 0)",
            [],
        )
        .expect("plant a row without attempts");
    drop(external);

    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("reopen");
    match db.list_active_runs().await {
        Err(lingxi_kernel::ports::StorageError::Corrupted { detail }) => {
            assert!(detail.contains("run_bad"), "names the row: {detail}");
        }
        other => panic!("expected loud Corrupted, got {other:?}"),
    }
    db.close().await.expect("close 2");
    let _ = std::fs::remove_file(&path);
}
