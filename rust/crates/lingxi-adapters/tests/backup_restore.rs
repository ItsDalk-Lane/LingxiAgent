//! R02-A11｜WAL 状态备份可恢复（REQUIRED）— integration with REAL SQLite
//! files, the REAL Online Backup API and REAL concurrent writers.
//!
//! No mock touches the backup/restore core: the backup runs through the
//! single-writer queue of a real `RunDatabase` on real files, the restore
//! copies real bytes and re-opens them through the full recovery path
//! (receipts + integrity), and the transaction-boundary proofs compare the
//! restored rows against the source rows (a rolled-back transaction must
//! NOT be in the backup; every committed one must).
//!
//! Semantics note (repair R1, review F01): a run's start and its terminal
//! commit are TWO committed transactions, and the backup job travels
//! through the same FIFO queue — a snapshot may therefore legally land
//! between the two. The concurrent-writer proof asserts the invariant that
//! actually holds for every interleaving (backup = complete snapshot of a
//! queue-processing prefix: no half transaction, in-flight runs allowed at
//! the prefix tail exactly as committed), NOT the "no in-flight run"
//! proposition the first implementation mistakenly asserted.
//!
//! Additional coverage here:
//! - interrupted backup leaves NO artifact that could masquerade as a good
//!   snapshot (no final name, no partial, no manifest) — the RLIMIT_FSIZE
//!   fault-injection variants live in tests/backup_faults.rs (process-wide
//!   fault state);
//! - restore refuses a tampered backup file (hash mismatch) and refuses to
//!   overwrite an existing target;
//! - database files are tightened to 0600 (T04 REVIEW F04 follow-up);
//! - backup/restore artifacts (directory + db copy + manifest) are
//!   tightened to 0700/0600 as well (repair R1 F02).

use std::path::{Path, PathBuf};

use lingxi_adapters::storage::{
    restore_backup, wal_sidecar_path, BackupOptions, RunDatabase, StoreOptions,
};
use lingxi_kernel::ports::{KeyEvent, RunOutcome, StoragePort};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    AttemptId, EventId, EventPayload, KnownEventPayload, RunId, RunStateChangedPayload, RunStatus,
    SessionId,
};

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("lingxi-r02t06-a11-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

async fn open_store(path: &Path, options: StoreOptions) -> RunDatabase {
    let db = RunDatabase::open(path, options).await.expect("open");
    db.ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
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
        key_events: vec![KeyEvent {
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

/// Commits run `name` through the port (start + terminal in two
/// transactions, matching the production execute flow).
async fn commit_run(db: &RunDatabase, name: &str) {
    db.record_run_started(&ctx(name), 1_000)
        .await
        .expect("record_run_started");
    db.commit_run_outcome(&ctx(name), completed(name), 2_000)
        .await
        .expect("commit_run_outcome");
}

fn backup_dir(dir: &Path) -> PathBuf {
    dir.join("backup")
}

// ── A11 core: WAL-resident committed data lands in the backup ───────────────

#[tokio::test]
async fn a11_online_backup_captures_wal_resident_committed_data_and_restores() {
    let dir = temp_dir("wal");
    let db_path = dir.join("runs.db");
    // Suppress the autocheckpoint so the committed rows STAY in the WAL
    // (the acceptance precondition: data committed but not yet checkpointed).
    let db = open_store(
        &db_path,
        StoreOptions {
            queue_capacity: 64,
            busy_timeout_ms: 5_000,
            checkpoint_pages: 10_000_000,
        },
    )
    .await;
    for i in 0..5 {
        commit_run(&db, &format!("r{i}")).await;
    }
    let wal_len = std::fs::metadata(wal_sidecar_path(&db_path))
        .expect("WAL sidecar must exist")
        .len();
    assert!(
        wal_len > 0,
        "precondition violated: WAL is empty, the committed data is not \
         WAL-resident (len={wal_len})"
    );

    let dest = backup_dir(&dir);
    let options = BackupOptions::default(); // frozen ADR-004 flow: checkpoint first, then the API
    let outcome = db.backup_to(&dest, "runs", options).await.expect("backup");
    assert_eq!(outcome.file_name, "runs.db");
    assert_eq!(outcome.wal_bytes_before, wal_len);
    assert!(outcome.bytes > 0);
    // Manifest exists next to the backup and names the same hash.
    let manifest_raw = std::fs::read_to_string(dest.join("runs.manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
    assert_eq!(
        manifest["files"][0]["sha256"],
        serde_json::json!(outcome.sha256)
    );
    assert_eq!(manifest["integrity"], serde_json::json!("ok"));
    // No partial residue after a successful backup.
    let leftovers: Vec<_> = std::fs::read_dir(&dest)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("partial"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    // Restore into ANOTHER directory and open it through the full recovery
    // path (receipts + integrity + migrations).
    let restore_dir = dir.join("restored");
    let restored = restore_backup(&dest, "runs", &restore_dir).expect("restore");
    let restored_db = RunDatabase::open(&restored.restored_path, StoreOptions::default())
        .await
        .expect("restored db must open through the normal recovery path");

    // Logical content of the restored copy == the live source (all five
    // committed runs, their terminal rows AND events intact).
    let source_dump = db.logical_dump().await.expect("dump");
    let restored_dump = restored_db.logical_dump().await.expect("dump");
    assert_eq!(source_dump, restored_dump, "logical content must match");
    assert!(source_dump.contains("r0") && source_dump.contains("r4"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a11_pure_online_backup_without_precheckpoint_is_also_complete() {
    // The second mechanism variant named by the taskbook: the Online
    // Backup API alone (no preceding checkpoint) over a non-empty WAL.
    let dir = temp_dir("pure");
    let db_path = dir.join("runs.db");
    let db = open_store(
        &db_path,
        StoreOptions {
            queue_capacity: 64,
            busy_timeout_ms: 5_000,
            checkpoint_pages: 10_000_000,
        },
    )
    .await;
    for i in 0..3 {
        commit_run(&db, &format!("p{i}")).await;
    }
    assert!(
        std::fs::metadata(wal_sidecar_path(&db_path))
            .expect("wal")
            .len()
            > 0,
        "WAL must be non-empty"
    );

    let dest = backup_dir(&dir);
    let outcome = db
        .backup_to(
            &dest,
            "runs",
            BackupOptions {
                pre_checkpoint: false,
            },
        )
        .await
        .expect("backup");
    assert!(outcome.wal_bytes_before > 0);

    let restore_dir = dir.join("restored");
    let restored = restore_backup(&dest, "runs", &restore_dir).expect("restore");
    let restored_db = RunDatabase::open(&restored.restored_path, StoreOptions::default())
        .await
        .expect("open");
    assert_eq!(
        db.logical_dump().await.expect("dump"),
        restored_db.logical_dump().await.expect("dump")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a11_transaction_boundary_rolled_back_data_is_absent_from_backup() {
    // Transaction-boundary proof with a REAL rollback: a run whose
    // record_run_started references a missing session fails (the whole
    // start transaction rolls back). The backup must contain exactly the
    // committed runs and nothing from the rolled-back transaction.
    let dir = temp_dir("boundary");
    let db_path = dir.join("runs.db");
    let db = open_store(&db_path, StoreOptions::default()).await;
    commit_run(&db, "before-fail").await;

    // A failing start (unknown session): nothing of this transaction may
    // survive anywhere.
    let bad_ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("sess_missing"),
        run_id: RunId::new("rolled-back-run"),
        attempt: AttemptId::new("rolled-back-run#a1"),
        generation: 1,
    };
    db.record_run_started(&bad_ctx, 1_000)
        .await
        .expect_err("unknown session must fail the whole start transaction");

    commit_run(&db, "after-fail").await;

    let dest = backup_dir(&dir);
    db.backup_to(&dest, "runs", BackupOptions::default())
        .await
        .expect("backup");
    let restore_dir = dir.join("restored");
    let restored = restore_backup(&dest, "runs", &restore_dir).expect("restore");
    let restored_db = RunDatabase::open(&restored.restored_path, StoreOptions::default())
        .await
        .expect("open");

    let restored_dump = restored_db.logical_dump().await.expect("dump");
    assert!(restored_dump.contains("before-fail"));
    assert!(restored_dump.contains("after-fail"));
    assert!(
        !restored_dump.contains("rolled-back-run"),
        "the rolled-back transaction must NOT appear in the backup: {restored_dump}"
    );
    assert_eq!(db.logical_dump().await.expect("dump"), restored_dump);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Reads one column of one run row through the queue's raw read seam
/// (evidence path, SELECT only).
async fn text_query(db: &RunDatabase, sql: &str, params: Vec<String>) -> String {
    db.query_one_text(sql, params)
        .await
        .expect("evidence query")
        .expect("evidence query returned a row")
}

#[tokio::test]
async fn a11_backup_during_concurrent_commits_is_a_consistent_prefix() {
    // Online backup while a writer keeps committing runs.
    //
    // CORRECTED SEMANTICS (repair R1, review F01): a run's start
    // (`record_run_started`: runs row + run_attempts row + start key event
    // + last_event_seq, ONE committed transaction) and its terminal commit
    // (`commit_run_outcome`: terminal status + terminal key event, a SECOND
    // committed transaction) are two separate queue jobs, and the backup is
    // a third job submitted through the same single-writer FIFO queue — so
    // the snapshot MAY legally land between the two. What A11's "事务边界
    // 正确" actually requires is therefore NOT "no in-flight run" but "the
    // backup is a COMPLETE snapshot of some queue-processing prefix":
    // every transaction committed before the snapshot appears in full, no
    // transaction contributes a partial set of its rows. The exact prefix
    // length is scheduling-dependent and deliberately NOT asserted; the
    // assertions below hold for EVERY interleaving (deterministic by
    // construction):
    // - the ids form the contiguous prefix w00..wK;
    // - every run row carries its start event and attempt row (same
    //   committed transaction — no half start);
    // - status=completed ⇔ done event present (status and terminal event
    //   commit together — no half commit);
    // - the event total is exactly one start per run plus one done per
    //   completed run (an in-flight run contributes exactly one event);
    // - at most ONE in-flight run can exist and only as the LAST prefix
    //   element (the writer commits run i before starting run i+1);
    // - the per-stream event seqs are contiguous 1..M (assigned
    //   MAX(seq)+1 inside each committing transaction — a torn snapshot
    //   would leave holes).
    let dir = temp_dir("concurrent");
    let db_path = dir.join("runs.db");
    let db = open_store(&db_path, StoreOptions::default()).await;
    let writer_db = std::sync::Arc::new(db);

    // Deterministic lower bound: the writer signals after w00 is fully
    // committed and the backup job is only submitted AFTER that signal —
    // the FIFO queue has already executed both w00 jobs, so the snapshot
    // always contains at least w00 (run_count >= 1 below is guaranteed,
    // not hopeful).
    let (w00_committed_tx, w00_committed_rx) = tokio::sync::oneshot::channel::<()>();
    let writer = {
        let db = std::sync::Arc::clone(&writer_db);
        let mut signal = Some(w00_committed_tx);
        tokio::spawn(async move {
            for i in 0..20 {
                db.record_run_started(&ctx(&format!("w{i:02}")), 1_000)
                    .await
                    .expect("start");
                db.commit_run_outcome(
                    &ctx(&format!("w{i:02}")),
                    completed(&format!("w{i:02}")),
                    2_000,
                )
                .await
                .expect("commit");
                if i == 0 {
                    if let Some(tx) = signal.take() {
                        let _ = tx.send(());
                    }
                }
                // Yield so the queue interleaves with the backup job.
                tokio::task::yield_now().await;
            }
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), w00_committed_rx)
        .await
        .expect("writer committed w00 within the timeout (no hang)")
        .expect("writer committed w00 and signalled");

    let backup_db = std::sync::Arc::clone(&writer_db);
    let dest = backup_dir(&dir);
    let dest_for_restore = dest.clone();
    let backup = tokio::spawn(async move {
        backup_db
            .backup_to(
                &dest,
                "runs",
                BackupOptions {
                    pre_checkpoint: false,
                },
            )
            .await
            .expect("backup")
    });
    let (writer_result, backup_result) = tokio::join!(writer, backup);
    writer_result.expect("writer task");
    let outcome = backup_result.expect("backup task");

    let restore_dir = dir.join("restored");
    let restored = restore_backup(&dest_for_restore, "runs", &restore_dir).expect("restore");
    let restored_db = RunDatabase::open(&restored.restored_path, StoreOptions::default())
        .await
        .expect("open");

    let (run_count, event_count, ids) = restored_db.run_fact_summary().await.expect("counts");
    assert!((1..=20).contains(&run_count), "run_count={run_count}");
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(
            *id,
            format!("w{index:02}"),
            "ids must form the committed prefix"
        );
    }

    // Transaction consistency, per run (holds for every interleaving).
    let mut completed_runs = 0i64;
    for id in &ids {
        let status = text_query(
            &restored_db,
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![id.clone()],
        )
        .await;
        assert!(
            status == "running" || status == "completed",
            "run {id}: unexpected status {status:?}"
        );
        let start_event = text_query(
            &restored_db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_id = ?2",
            vec![id.clone(), format!("{id}-start")],
        )
        .await;
        assert_eq!(
            start_event, "1",
            "run {id}: the committed start transaction must be complete \
             (run row + attempt + start event together)"
        );
        let attempts = text_query(
            &restored_db,
            "SELECT COUNT(*) FROM run_attempts WHERE run_id = ?1",
            vec![id.clone()],
        )
        .await;
        assert_eq!(
            attempts, "1",
            "run {id}: the attempt row commits in the same start transaction"
        );
        let done_event = text_query(
            &restored_db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_id = ?2",
            vec![id.clone(), format!("{id}-done")],
        )
        .await;
        if status == "completed" {
            assert_eq!(
                done_event, "1",
                "run {id}: terminal status and done event commit together"
            );
            completed_runs += 1;
        } else {
            assert_eq!(
                done_event, "0",
                "run {id}: a non-terminal (in-flight) run must not carry the \
                 terminal event"
            );
        }
    }
    assert_eq!(
        event_count,
        run_count + completed_runs,
        "event total = one start event per run + one done event per \
         completed run (an in-flight run contributes exactly one event)"
    );

    // At most one in-flight run, and only as the LAST prefix element.
    let inflight = text_query(
        &restored_db,
        "SELECT COUNT(*) FROM runs WHERE status = 'running'",
        vec![],
    )
    .await;
    assert!(
        inflight == "0" || inflight == "1",
        "at most one in-flight run (run i+1 starts only after run i \
         commits): {inflight}"
    );
    if inflight == "1" {
        let inflight_id = text_query(
            &restored_db,
            "SELECT run_id FROM runs WHERE status = 'running'",
            vec![],
        )
        .await;
        let last = ids.last().expect("non-empty prefix");
        assert_eq!(
            &inflight_id, last,
            "the in-flight run must be the newest prefix element"
        );
    }

    // No torn transaction: the per-stream event seqs are contiguous 1..M.
    let max_seq = text_query(
        &restored_db,
        "SELECT MAX(seq) FROM key_events WHERE stream_id = 'sess_a'",
        vec![],
    )
    .await;
    let seq_rows = text_query(&restored_db, "SELECT COUNT(*) FROM key_events", vec![]).await;
    assert_eq!(
        max_seq, seq_rows,
        "event seqs must be contiguous 1..M (no torn transaction holes)"
    );
    assert!(outcome.bytes > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a11_backup_of_a_started_but_uncommitted_run_is_a_complete_prefix() {
    // Fixed interleaving (deterministic): the exact mid-flight state a
    // concurrent backup can legally observe — two fully committed runs,
    // then ONE started-but-not-yet-committed run, then the backup. The
    // started run is itself a COMMITTED transaction (row + attempt + start
    // event), so the snapshot must preserve it EXACTLY as it was: present
    // as 'running' with its start event, without any invented terminal
    // state — and the restored copy must equal the source logical dump.
    // This pins the semantics that the concurrent test above tolerates.
    let dir = temp_dir("inflight");
    let db_path = dir.join("runs.db");
    let db = open_store(&db_path, StoreOptions::default()).await;
    commit_run(&db, "w00").await;
    commit_run(&db, "w01").await;
    db.record_run_started(&ctx("w02"), 1_000)
        .await
        .expect("start");

    let dest = backup_dir(&dir);
    db.backup_to(&dest, "runs", BackupOptions::default())
        .await
        .expect("backup");
    // The source does not advance after the snapshot, so a full logical
    // equality IS a valid assertion here (unlike the concurrent case).
    let source_dump = db.logical_dump().await.expect("dump");

    let restore_dir = dir.join("restored");
    let restored = restore_backup(&dest, "runs", &restore_dir).expect("restore");
    let restored_db = RunDatabase::open(&restored.restored_path, StoreOptions::default())
        .await
        .expect("open");
    let restored_dump = restored_db.logical_dump().await.expect("dump");
    assert_eq!(
        source_dump, restored_dump,
        "the in-flight run is part of the committed queue prefix and must \
         be preserved exactly"
    );

    let (run_count, event_count, ids) = restored_db.run_fact_summary().await.expect("counts");
    assert_eq!(run_count, 3);
    assert_eq!(
        event_count, 5,
        "2 committed runs x (start+done) + 1 in-flight run x start"
    );
    assert_eq!(ids, vec!["w00", "w01", "w02"]);

    let status = |id: &str| {
        text_query(
            &restored_db,
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![id.to_string()],
        )
    };
    assert_eq!(status("w00").await, "completed");
    assert_eq!(status("w01").await, "completed");
    assert_eq!(
        status("w02").await,
        "running",
        "the started-but-uncommitted run is legitimately in the snapshot \
         as 'running'"
    );
    assert_eq!(
        text_query(
            &restored_db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = 'w02' AND event_id = 'w02-start'",
            vec![],
        )
        .await,
        "1",
        "the start transaction is complete in the snapshot"
    );
    assert_eq!(
        text_query(
            &restored_db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = 'w02' AND event_id = 'w02-done'",
            vec![],
        )
        .await,
        "0",
        "the terminal transaction had not run at snapshot time"
    );
    assert_eq!(
        text_query(
            &restored_db,
            "SELECT COUNT(*) FROM run_attempts WHERE run_id = 'w02'",
            vec![],
        )
        .await,
        "1",
        "the attempt row commits with the start"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Fault: interrupted backup leaves no masquerading artifact ───────────────

#[tokio::test]
async fn backup_into_unwritable_destination_fails_without_artifacts() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = temp_dir("unwritable");
        let db_path = dir.join("runs.db");
        let db = open_store(&db_path, StoreOptions::default()).await;
        commit_run(&db, "u0").await;

        let parent = dir.join("backup-parent");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
        let dest = parent.join("backup");
        let err = db.backup_to(&dest, "runs", BackupOptions::default()).await;
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            err.is_err(),
            "backup into an unwritable destination must fail"
        );

        // NOTHING may exist that a later restore could mistake for a good
        // snapshot: no final file, no manifest, no partial.
        if dest.exists() {
            let leftovers: Vec<_> = std::fs::read_dir(&dest)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert!(
                leftovers
                    .iter()
                    .all(|n| !n.ends_with(".db") && !n.ends_with(".json")),
                "no backup artifact may survive a failed backup: {leftovers:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ── Restore-side guards ──────────────────────────────────────────────────────

#[tokio::test]
async fn restore_refuses_a_tampered_backup_file() {
    let dir = temp_dir("tamper");
    let db_path = dir.join("runs.db");
    let db = open_store(&db_path, StoreOptions::default()).await;
    commit_run(&db, "t0").await;
    let dest = backup_dir(&dir);
    db.backup_to(&dest, "runs", BackupOptions::default())
        .await
        .expect("backup");

    // Flip bytes in the backup file (keep size) after the manifest was
    // written: the hash no longer matches.
    let backup_file = dest.join("runs.db");
    let mut bytes = std::fs::read(&backup_file).unwrap();
    let last = bytes.len() - 16;
    bytes[last] ^= 0xff;
    std::fs::write(&backup_file, &bytes).unwrap();

    let restore_dir = dir.join("restored");
    let err = restore_backup(&dest, "runs", &restore_dir).expect_err("must refuse");
    let text = err.to_string();
    assert!(text.contains("mismatch"), "{text}");
    assert!(
        !restore_dir.join("runs.db").exists(),
        "a refused restore must not leave a target file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn restore_refuses_an_existing_target() {
    let dir = temp_dir("overwrite");
    let db_path = dir.join("runs.db");
    let db = open_store(&db_path, StoreOptions::default()).await;
    commit_run(&db, "o0").await;
    let dest = backup_dir(&dir);
    db.backup_to(&dest, "runs", BackupOptions::default())
        .await
        .expect("backup");

    let restore_dir = dir.join("restored");
    std::fs::create_dir_all(&restore_dir).unwrap();
    std::fs::write(restore_dir.join("runs.db"), b"pre-existing").unwrap();
    let err = restore_backup(&dest, "runs", &restore_dir).expect_err("must refuse");
    assert!(err.to_string().contains("refusing to overwrite"));
    assert_eq!(
        std::fs::read(restore_dir.join("runs.db")).unwrap(),
        b"pre-existing",
        "the existing target must be untouched"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn backup_validates_the_file_stem() {
    let dir = temp_dir("stem");
    let db_path = dir.join("runs.db");
    let db = open_store(&db_path, StoreOptions::default()).await;
    let err = db
        .backup_to(&dir.join("b"), "../escape", BackupOptions::default())
        .await
        .expect_err("path traversal stem must be rejected");
    assert!(err.to_string().contains("invalid backup file stem"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ── F04 follow-up: database files are owner-only (0600) ─────────────────────

#[cfg(unix)]
#[tokio::test]
async fn database_files_are_tightened_to_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = temp_dir("perms");
    let db_path = dir.join("runs.db");
    let db = open_store(&db_path, StoreOptions::default()).await;
    commit_run(&db, "p0").await;
    // T04 REVIEW F04: the db file must now be 0600 (previously 0644).
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&db_path), 0o600, "runs.db must be owner-only");
    assert_eq!(
        mode(&wal_sidecar_path(&db_path)),
        0o600,
        "the WAL sidecar must be owner-only"
    );
    let shm = db_path.with_file_name("runs.db-shm");
    assert_eq!(mode(&shm), 0o600, "the shm sidecar must be owner-only");
    db.close().await.expect("close");
    assert_eq!(mode(&db_path), 0o600, "close keeps the tightened mode");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── F02 repair: backup/restore artifacts are owner-only too ─────────────────

#[cfg(unix)]
#[tokio::test]
async fn backup_and_restore_artifacts_are_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = temp_dir("artifact-perms");
    let db_path = dir.join("runs.db");
    let db = open_store(&db_path, StoreOptions::default()).await;
    commit_run(&db, "perm0").await;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

    // Pre-create BOTH destination directories LOOSE on purpose (0777, not
    // umask luck): the module must tighten them itself, so the assertions
    // below verify the explicit tightening rather than the ambient umask.
    let dest = dir.join("loose-backup");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o777)).unwrap();
    db.backup_to(&dest, "runs", BackupOptions::default())
        .await
        .expect("backup");
    assert_eq!(
        mode(&dest),
        0o700,
        "the backup directory must be owner-only"
    );
    assert_eq!(
        mode(&dest.join("runs.db")),
        0o600,
        "the backup db copy must be owner-only"
    );
    assert_eq!(
        mode(&dest.join("runs.manifest.json")),
        0o600,
        "the manifest must be owner-only"
    );

    let restore_dir = dir.join("loose-restored");
    std::fs::create_dir_all(&restore_dir).unwrap();
    std::fs::set_permissions(&restore_dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let restored = restore_backup(&dest, "runs", &restore_dir).expect("restore");
    assert_eq!(
        mode(&restore_dir),
        0o700,
        "the restore target directory must be owner-only"
    );
    assert_eq!(
        mode(&restored.restored_path),
        0o600,
        "the restored db file must be owner-only"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
