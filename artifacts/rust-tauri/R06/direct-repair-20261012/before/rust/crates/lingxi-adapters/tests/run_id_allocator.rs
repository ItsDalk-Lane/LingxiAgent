//! R2-F01 regression (R02 stage-repair R2): the run-id allocator's
//! open-time seed must be the durable HIGH-WATER MARK of consumed
//! sequences — max(COUNT(*), highest seq parsed from any stored run id) —
//! never the bare row count. A failed first commit consumes a sequence
//! number without leaving a row; reseeding from COUNT(*) then re-issued a
//! number already committed, and a normal new request after the restart
//! collided with the prior completed run (observed exit 101 in the R2
//! review's id-gap-rerun probe at a fixed clock).
//!
//! These tests pin the contract at the storage layer with a REAL database
//! and a REAL injected insert failure (SQL trigger), the same mechanism
//! the independent probe used — no mock of the allocator under test.

use std::path::PathBuf;

use lingxi_adapters::storage::{RunDatabase, StoreOptions};
use lingxi_kernel::ports::{RunOutcome, StoragePort};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    AttemptId, EventId, EventPayload, KnownEventPayload, RunId, RunStateChangedPayload, RunStatus,
    SessionId,
};

fn temp_db(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r2f01-allocator-{}-{tag}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.join("runs.db")
}

fn ctx(run: &str) -> RunContext {
    let run: String = run.to_string();
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("sess_a"),
        run_id: RunId::new(run.clone()),
        attempt: AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}

fn completed(run: &str) -> RunOutcome {
    RunOutcome {
        status: RunStatus::Completed,
        reason: None,
        key_events: vec![lingxi_kernel::ports::KeyEvent {
            event_id: EventId::new(format!("{run}-done")),
            payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                RunStateChangedPayload {
                    from: RunStatus::Running,
                    to: RunStatus::Completed,
                    reason: None,
                },
            )),
        }],
        final_message: None,
    }
}

async fn seed_session(db: &RunDatabase) {
    db.ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
        session_id: "sess_a".into(),
        agent_id: "lingxi".into(),
        owner_user_id: "user_local".into(),
        title: "t".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .expect("seed");
}

/// The R2 probe scenario end to end at the storage layer, at a FIXED
/// clock: injected failure consumes a sequence, recovery commits one run,
/// restart, then a normal new request must NOT collide.
#[tokio::test]
async fn reseed_after_failure_gap_never_reissues_a_consumed_id() {
    let path = temp_db("gap-restart");
    let now: u64 = 1_790_409_600_000; // fixed clock — same contract as the probe
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("open");
    seed_session(&db).await;

    // Injected storage failure: every INSERT into runs aborts.
    let conn = rusqlite::Connection::open(&path).expect("raw open");
    conn.execute_batch(
        "CREATE TRIGGER reject_run BEFORE INSERT ON runs BEGIN \
         SELECT RAISE(ABORT,'injected temporary storage failure'); END;",
    )
    .expect("create trigger");
    let failed_id = db.allocate_run_id(now).expect("alloc");
    let failed = db.record_run_started(&ctx(&failed_id), now).await;
    assert!(failed.is_err(), "injected failure must surface: {failed:?}");
    conn.execute_batch("DROP TRIGGER reject_run")
        .expect("drop trigger");
    drop(conn);

    // Recovery: a different request commits (one durable run; the failed
    // attempt already consumed a sequence number — COUNT(*) = 1 < 2).
    let committed_id = db.allocate_run_id(now).expect("alloc");
    let committed_ctx = ctx(&committed_id);
    db.record_run_started(&committed_ctx, now)
        .await
        .expect("recovered start");
    db.commit_run_outcome(&committed_ctx, completed(&committed_id), now + 1)
        .await
        .expect("commit");
    assert_eq!(
        db.total_runs().await.expect("count"),
        1,
        "exactly one durable run after the recovered commit"
    );
    db.close().await.expect("close");

    // Restart at the SAME fixed clock: the reseed must skip every number
    // already consumed, so the next id is fresh and the request succeeds.
    let reopened = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("reopen");
    seed_session(&reopened).await;
    let next_id = reopened.allocate_run_id(now).expect("alloc");
    assert_ne!(
        next_id, committed_id,
        "restart must not re-issue the committed run id"
    );
    assert_ne!(
        next_id, failed_id,
        "restart must not re-issue the failed attempt's id either"
    );
    reopened
        .record_run_started(&ctx(&next_id), now)
        .await
        .expect("normal new request after restart must succeed");
    reopened.close().await.expect("close");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Concurrency contract preserved (R1 F01 must stay closed): many
/// concurrent allocations at one fixed instant are pairwise distinct, and
/// a restart continues above all of them.
#[tokio::test]
async fn concurrent_allocations_stay_unique_and_reseed_above_all() {
    let path = temp_db("concurrent");
    let now: u64 = 1_790_409_600_000;
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("open");
    seed_session(&db).await;

    let mut handles = Vec::new();
    for _ in 0..64 {
        let db = db.clone();
        handles.push(tokio::spawn(async move { db.allocate_run_id(now) })); // Result, joined below
    }
    let mut ids = Vec::new();
    for handle in handles {
        ids.push(handle.await.expect("alloc task").expect("concurrent alloc"));
    }
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(
        unique.len(),
        64,
        "64 concurrent ids must be pairwise distinct"
    );

    // Persist only HALF of them (the other numbers stay consumed without
    // rows — the failure-gap shape), then restart. The restart contract is
    // about DURABLE state: the reseeded allocator must never collide with
    // a persisted id (numbers consumed only in-memory before the restart
    // are unknowable — gaps are harmless, uniqueness over committed facts
    // is the contract).
    let persisted: std::collections::HashSet<_> = ids.iter().take(32).collect();
    for (index, id) in ids.iter().take(32).enumerate() {
        db.record_run_started(&ctx(id), now + index as u64)
            .await
            .expect("start");
    }
    db.close().await.expect("close");

    let reopened = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("reopen");
    let after = reopened.allocate_run_id(now).expect("alloc");
    assert!(
        !persisted.contains(&after),
        "the restarted allocator re-issued persisted id {after}"
    );
    reopened
        .record_run_started(&ctx(&after), now)
        .await
        .expect("the reseeded id must start cleanly");
    reopened.close().await.expect("close");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// R5-F02 regression: a stored OPAQUE run id (accepted through the public
/// `StoragePort` surface) must never seed the sequence allocator. The old
/// tail-after-last-`_` heuristic parsed `…_ffffffffffffffff` as
/// `u64::MAX`, seeded the allocator to the top, and the FIRST allocation
/// after a reopen panicked with `attempt to add with overflow` (real
/// cargo exit 101 in the R5 review's probe). With strict minted-format
/// recognition the opaque row counts toward COUNT(*) only, and a normal
/// new request after the restart simply succeeds.
#[tokio::test]
async fn opaque_existing_run_id_never_seeds_and_reopen_allocates_cleanly() {
    let path = temp_db("opaque-reopen");
    let now: u64 = 1_790_409_600_000; // fixed clock, same as the probe
    let opaque = "opaque_existing_ffffffffffffffff".to_string();

    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("open");
    seed_session(&db).await;
    // The public StoragePort accepts the opaque id — this is the exact
    // legal-store-call shape of the review's fixture, no raw SQL.
    db.record_run_started(&ctx(&opaque), now)
        .await
        .expect("opaque id is a legal stored run");
    db.commit_run_outcome(&ctx(&opaque), completed(&opaque), now + 1)
        .await
        .expect("commit");
    db.close().await.expect("close");

    // Normal close/reopen, then a fresh allocation: the pre-fix build
    // panicked HERE; the fixed build hands out a fresh minted id.
    let reopened = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("reopen");
    let next = reopened
        .allocate_run_id(now)
        .expect("allocation after opaque-id reopen must not panic");
    assert_ne!(next, opaque, "a minted id is never the opaque id");
    assert!(
        next.starts_with("run_") && next.ends_with("_000002"),
        "seed comes from COUNT(*) (1 row) and the opaque tail is ignored: {next}"
    );
    reopened
        .record_run_started(&ctx(&next), now)
        .await
        .expect("normal new request after restart must succeed");
    reopened.close().await.expect("close");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Smaller foreign hex tails must not shift the high-water mark either:
/// the seed stays the COUNT lower bound plus only MINTED sequences.
#[tokio::test]
async fn non_minted_hex_tails_do_not_seed_the_high_water_mark() {
    let path = temp_db("non-minted-tails");
    let now: u64 = 1_790_409_600_000;
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("open");
    seed_session(&db).await;
    for foreign in [
        "legacy_000000000000beef",
        "run-0000017f3b0e0000-000009",
        "run_0000017f3b0e0000_0000AB", // uppercase tail: not minted
        "imported_run_0000017f3b0e0000_000001_x",
    ] {
        db.record_run_started(&ctx(foreign), now)
            .await
            .expect("foreign id is a legal stored run");
    }
    db.close().await.expect("close");

    let reopened = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("reopen");
    let next = reopened.allocate_run_id(now).expect("alloc");
    // 4 rows -> COUNT lower bound 4; no foreign tail contributed a seq.
    assert!(
        next.ends_with("_000005"),
        "seed must be the COUNT lower bound (4), got {next}"
    );
    reopened.close().await.expect("close");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Genuinely exhausted space: a stored MINTED id whose sequence is exactly
/// `u64::MAX` makes the next allocation fail LOUDLY with
/// `RunIdExhausted` — never a panic, never a wrap to 0 that would re-issue
/// numbers owned by stored runs. One below the top still mints the last
/// id before refusing.
#[tokio::test]
async fn exhausted_sequence_fails_loudly_instead_of_panicking_or_wrapping() {
    use lingxi_kernel::ports::StorageError;
    let path = temp_db("exhausted");
    let now: u64 = 1_790_409_600_000;
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("open");
    seed_session(&db).await;
    let max_id = format!("run_{now:016x}_{:06x}", u64::MAX);
    assert_eq!(max_id, format!("run_{now:016x}_ffffffffffffffff"));
    db.record_run_started(&ctx(&max_id), now)
        .await
        .expect("store the maximal minted id");
    db.close().await.expect("close");

    let reopened = RunDatabase::open(&path, StoreOptions::default())
        .await
        .expect("reopen");
    let err = reopened
        .allocate_run_id(now)
        .expect_err("space is exhausted: must be a loud error");
    assert!(
        matches!(err, StorageError::RunIdExhausted { .. }),
        "expected RunIdExhausted, got {err:?}"
    );
    reopened.close().await.expect("close");

    // One below the top: the first allocation mints the LAST id
    // (u64::MAX), the very next one refuses.
    let path2 = temp_db("near-exhausted");
    let db2 = RunDatabase::open(&path2, StoreOptions::default())
        .await
        .expect("open");
    seed_session(&db2).await;
    let near_id = format!("run_{now:016x}_{:06x}", u64::MAX - 1);
    db2.record_run_started(&ctx(&near_id), now)
        .await
        .expect("store the near-maximal minted id");
    db2.close().await.expect("close");

    let reopened2 = RunDatabase::open(&path2, StoreOptions::default())
        .await
        .expect("reopen");
    let last = reopened2
        .allocate_run_id(now)
        .expect("u64::MAX is still free");
    assert_eq!(
        last,
        format!("run_{now:016x}_ffffffffffffffff"),
        "the last available id must be minted exactly once"
    );
    let err2 = reopened2
        .allocate_run_id(now)
        .expect_err("no id remains: must refuse");
    assert!(
        matches!(err2, StorageError::RunIdExhausted { .. }),
        "expected RunIdExhausted, got {err2:?}"
    );
    reopened2.close().await.expect("close");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let _ = std::fs::remove_dir_all(path2.parent().unwrap());
}
