//! R02-T06 step 2/4｜graceful shutdown coordinator — integration with a
//! REAL `RunDatabase` (real queue drain + WAL checkpoint) and a REAL
//! `InstanceGuard` (real lock file + instance record on disk).
//!
//! Contract pinned here:
//! - the clean path removes the own instance record, drains the database
//!   and reports exit code 0;
//! - a DB close that exceeds the deadline is LOUD (the coordinator records
//!   the timeout), the remaining phases still run (the instance record is
//!   still removed — a timed-out close must not leave the home looking
//!   owned) and the exit code is 6;
//! - a storage-close FAILURE (double close → queue closed) maps to exit 5;
//! - a record-cleanup FAILURE (unreadable record) maps to exit 4;
//! - WS sessions are managed tasks: opening/closing is counted, and the
//!   drain waits for them (see the `ws_shutdown` unit tests in `shutdown.rs`).
//!
//! The multi-thread runtime flavor is REQUIRED: the record-cleanup phase
//! uses `tokio::task::block_in_place` (a sync filesystem call bounded by
//! the deadline), which panics on a current-thread runtime.

use std::time::Duration;

use lingxi_adapters::storage::{RunDatabase, StoreOptions};
use lingxi_service::shutdown::{graceful_shutdown, WsShutdown};

fn temp_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r02t06-shutdown-{}-{}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp home");
    dir
}

async fn open_db(home: &std::path::Path) -> RunDatabase {
    let data_dir = home.join("lingxi-service").join("data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    RunDatabase::open(&data_dir.join("runs.db"), StoreOptions::default())
        .await
        .expect("open run database")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clean_shutdown_reports_zero_and_removes_the_own_record() {
    let home = temp_home("clean");
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    let db = open_db(&home).await;
    let (mut guard, stale) = lingxi_service::acquire(&layout).expect("acquire");
    assert!(stale.is_none());
    guard
        .publish("127.0.0.1:1".parse().unwrap())
        .expect("publish");
    assert!(layout.record_path.exists());

    let ws = WsShutdown::new();
    let report = graceful_shutdown(&db, &ws, &mut guard, Duration::from_secs(10), 10_000).await;

    assert_eq!(report.exit_code(), 0, "{report:?}");
    assert!(!report.any_timeout());
    assert!(report.storage_error.is_none());
    assert!(!layout.record_path.exists(), "own record must be removed");
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn storage_close_timeout_is_recorded_and_record_cleanup_still_runs() {
    let home = temp_home("timeout");
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    // A short worker busy-timeout keeps the abandoned close bounded while
    // still exceeding the coordinator deadline many times over.
    let data_dir = home.join("lingxi-service").join("data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    let db = RunDatabase::open(
        &data_dir.join("runs.db"),
        StoreOptions {
            busy_timeout_ms: 500,
            ..StoreOptions::default()
        },
    )
    .await
    .expect("open run database");

    // REAL blocked-worker scenario: an external SQLite connection holds
    // the WAL write lock, so the close's TRUNCATE checkpoint cannot
    // complete within the deadline (the worker is inside SQLite's own
    // busy handling). The coordinator's deadline fires first.
    let external = rusqlite::Connection::open(data_dir.join("runs.db")).expect("external conn");
    external
        .pragma_update(None, "busy_timeout", 10_000)
        .expect("external busy timeout");
    external
        .execute_batch("BEGIN EXCLUSIVE;")
        .expect("external exclusive transaction");

    let (mut guard, _) = lingxi_service::acquire(&layout).expect("acquire");
    guard
        .publish("127.0.0.1:2".parse().unwrap())
        .expect("publish");
    let ws = WsShutdown::new();

    let started = std::time::Instant::now();
    let report = graceful_shutdown(&db, &ws, &mut guard, Duration::from_millis(120), 120).await;
    let elapsed = started.elapsed();

    assert!(report.storage_close_timed_out, "{report:?}");
    assert_eq!(report.exit_code(), 6, "{report:?}");
    // The deadline bounded the phase (the blocked close would have taken
    // ~500ms+; the coordinator gave up at ~120ms).
    assert!(
        elapsed < Duration::from_millis(400),
        "the deadline must bound the phase, took {elapsed:?}"
    );
    // The remaining phase still ran: the instance record IS removed.
    assert!(
        !layout.record_path.exists(),
        "a timed-out DB close must not leave the home looking owned"
    );

    // Release the external lock so the worker's abandoned checkpoint can
    // finish and the test can clean up.
    drop(external);
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn storage_close_failure_maps_to_exit_five() {
    let home = temp_home("storage-fail");
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    let db = open_db(&home).await;
    // First close succeeds; a SECOND close hits a closed queue — the
    // coordinator must surface it as a storage error (exit 5), not swallow.
    db.close().await.expect("first close");

    let (mut guard, _) = lingxi_service::acquire(&layout).expect("acquire");
    guard
        .publish("127.0.0.1:3".parse().unwrap())
        .expect("publish");
    let ws = WsShutdown::new();
    let report = graceful_shutdown(&db, &ws, &mut guard, Duration::from_secs(5), 5_000).await;

    assert!(report.storage_error.is_some(), "{report:?}");
    assert_eq!(report.exit_code(), 5, "{report:?}");
    assert!(!layout.record_path.exists(), "record cleanup still ran");
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn record_cleanup_failure_maps_to_exit_four() {
    let home = temp_home("record-fail");
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    let db = open_db(&home).await;
    let (mut guard, _) = lingxi_service::acquire(&layout).expect("acquire");
    guard
        .publish("127.0.0.1:4".parse().unwrap())
        .expect("publish");

    // Sabotage: replace the record with garbage so ownership verification
    // fails — the coordinator must report the failure (exit 4), never
    // silently delete a record it cannot verify.
    std::fs::write(&layout.record_path, b"{ not json").unwrap();

    let ws = WsShutdown::new();
    let report = graceful_shutdown(&db, &ws, &mut guard, Duration::from_secs(5), 5_000).await;

    assert!(report.record_error.is_some(), "{report:?}");
    assert_eq!(report.exit_code(), 4, "{report:?}");
    assert!(layout.record_path.exists(), "the unverifiable record stays");
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_ws_sessions_block_the_drain_until_closed() {
    let ws = std::sync::Arc::new(WsShutdown::new());
    let session = lingxi_service::shutdown::WsSessionGuard::new(std::sync::Arc::clone(&ws));
    assert_eq!(ws.open_count(), 1);

    let waiter = {
        let ws = std::sync::Arc::clone(&ws);
        tokio::spawn(async move { ws.close_and_wait(Duration::from_millis(2_000)).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    // Still draining while the session is open.
    assert!(
        !waiter.is_finished(),
        "drain must wait for the open session"
    );
    drop(session); // the session ends (any exit path decrements)
    let drained = tokio::time::timeout(Duration::from_millis(2_000), waiter)
        .await
        .expect("drain completes")
        .expect("task");
    assert!(drained);
}
