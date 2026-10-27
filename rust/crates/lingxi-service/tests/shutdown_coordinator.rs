//! R02-T06 step 2/4｜graceful shutdown coordinator — integration with a
//! REAL `RunDatabase` (real queue drain + WAL checkpoint) and a REAL
//! `InstanceGuard` (real lock file + instance record on disk).
//!
//! Contract pinned here:
//! - the clean path removes the own instance record, drains the database
//!   and reports exit code 0;
//! - a DB close that exceeds the deadline is LOUD (the coordinator records
//!   the timeout), the remaining phases still run (the instance record
//!   removal is still ATTEMPTED — a timed-out close must not leave the
//!   home looking owned) and the exit code is 6;
//! - a storage-close FAILURE (double close → queue closed) maps to exit 5;
//! - a record-cleanup FAILURE (unreadable record) maps to exit 4;
//! - WS sessions are managed tasks: opening/closing is counted, and the
//!   drain waits for them (see the `ws_shutdown` unit tests in `shutdown.rs`).
//!
//! R9-F04: the record-cleanup phase runs on a DEDICATED OS thread awaited
//! under the real timeout (the old `timeout(.. block_in_place(release))`
//! could not preempt a never-yielding filesystem call, so a slow release
//! overran the budget and still returned Ok unflagged — even at ZERO
//! remaining). When the phase's own budget is already ~exhausted, the
//! coordinator reports `record_cleanup_timed_out` and RETURNS while the
//! detached worker still attempts the removal; in the product the
//! deterministic `std::process::exit` bounds that worker, and here the
//! tests bounded-observe its completion. The multi-thread runtime flavor
//! matches the production binary.

use std::time::Duration;

use lingxi_adapters::storage::{RunDatabase, StoreOptions};
use lingxi_service::shutdown::{graceful_shutdown, ShutdownBudget, WsShutdown};

/// A budget anchored NOW with the given total (the coordinator tests drive
/// phases directly; the signal anchor is the test's own start).
fn budget(total: Duration) -> ShutdownBudget {
    ShutdownBudget::new(std::time::Instant::now(), total)
}

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
    // R03-T06: the graceful-shutdown coordinator now carries the
    // background-drive exit hook (empty registry here — no background
    // drives are spawned by these fixtures; the drain phase is exercised
    // in the R03-T06 acceptance tests).
    // R03-T07: the coordinator also carries the run supervisor for the
    // exit cancellation of live background drives (none here — a provider-
    // less supervisor is enough for these fixtures).
    let background = lingxi_service::background::BackgroundDriveRegistry::new();
    let runs = lingxi_service::runs::RunSupervisor::without_provider();
    let report = graceful_shutdown(
        &db,
        &ws,
        &background,
        &runs,
        guard,
        false,
        budget(Duration::from_secs(10)),
    )
    .await;

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
    // R03-T06: the graceful-shutdown coordinator now carries the
    // background-drive exit hook (empty registry here — no background
    // drives are spawned by these fixtures; the drain phase is exercised
    // in the R03-T06 acceptance tests).
    // R03-T07: the coordinator also carries the run supervisor for the
    // exit cancellation of live background drives (none here — a provider-
    // less supervisor is enough for these fixtures).
    let background = lingxi_service::background::BackgroundDriveRegistry::new();
    let runs = lingxi_service::runs::RunSupervisor::without_provider();

    let started = std::time::Instant::now();
    let report = graceful_shutdown(
        &db,
        &ws,
        &background,
        &runs,
        guard,
        false,
        budget(Duration::from_millis(120)),
    )
    .await;
    let elapsed = started.elapsed();

    assert!(report.storage_close_timed_out, "{report:?}");
    assert_eq!(report.exit_code(), 6, "{report:?}");
    // The deadline bounded the phase (the blocked close would have taken
    // ~500ms+; the coordinator gave up at ~120ms).
    assert!(
        elapsed < Duration::from_millis(400),
        "the deadline must bound the phase, took {elapsed:?}"
    );
    // R9-F04: the record-cleanup phase started with ~zero remaining, so
    // the coordinator reports its timeout and returns while the DETACHED
    // worker still attempts the removal — bounded-observe the completion
    // here (in the product the deterministic process exit bounds instead).
    let removal_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while layout.record_path.exists() {
        assert!(
            std::time::Instant::now() < removal_deadline,
            "the detached record-cleanup worker should still remove the record: {report:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

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
    // R03-T06: the graceful-shutdown coordinator now carries the
    // background-drive exit hook (empty registry here — no background
    // drives are spawned by these fixtures; the drain phase is exercised
    // in the R03-T06 acceptance tests).
    // R03-T07: the coordinator also carries the run supervisor for the
    // exit cancellation of live background drives (none here — a provider-
    // less supervisor is enough for these fixtures).
    let background = lingxi_service::background::BackgroundDriveRegistry::new();
    let runs = lingxi_service::runs::RunSupervisor::without_provider();
    let report = graceful_shutdown(
        &db,
        &ws,
        &background,
        &runs,
        guard,
        false,
        budget(Duration::from_secs(5)),
    )
    .await;

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
    // R03-T06: the graceful-shutdown coordinator now carries the
    // background-drive exit hook (empty registry here — no background
    // drives are spawned by these fixtures; the drain phase is exercised
    // in the R03-T06 acceptance tests).
    // R03-T07: the coordinator also carries the run supervisor for the
    // exit cancellation of live background drives (none here — a provider-
    // less supervisor is enough for these fixtures).
    let background = lingxi_service::background::BackgroundDriveRegistry::new();
    let runs = lingxi_service::runs::RunSupervisor::without_provider();
    let report = graceful_shutdown(
        &db,
        &ws,
        &background,
        &runs,
        guard,
        false,
        budget(Duration::from_secs(5)),
    )
    .await;

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

// ── R02 stage-repair R1 / F03: ONE budget anchored at the signal ───────────

/// An already-mostly-spent budget leaves only the remainder for every later
/// phase: pre-fix each phase got a FRESH deadline (the drain's own wait had
/// none at all), so the total shutdown could stretch to N× the configured
/// timeout; now the phases share the from-signal budget.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exhausted_budget_bounds_every_remaining_phase() {
    let home = temp_home("budget");
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    let db = open_db(&home).await;
    let (mut guard, _) = lingxi_service::acquire(&layout).expect("acquire");
    guard
        .publish("127.0.0.1:5".parse().unwrap())
        .expect("publish");

    let ws = WsShutdown::new();
    // R03-T06: the graceful-shutdown coordinator now carries the
    // background-drive exit hook (empty registry here — no background
    // drives are spawned by these fixtures; the drain phase is exercised
    // in the R03-T06 acceptance tests).
    // R03-T07: the coordinator also carries the run supervisor for the
    // exit cancellation of live background drives (none here — a provider-
    // less supervisor is enough for these fixtures).
    let background = lingxi_service::background::BackgroundDriveRegistry::new();
    let runs = lingxi_service::runs::RunSupervisor::without_provider();
    ws.connection_opened(); // a session that never closes on its own

    // The signal arrived 150ms ago; the total budget is 100ms — it is
    // EXHAUSTED before the coordinator even starts.
    let started = std::time::Instant::now();
    let spent = ShutdownBudget::new(
        std::time::Instant::now() - Duration::from_millis(150),
        Duration::from_millis(100),
    );
    assert_eq!(spent.remaining(), Duration::ZERO);
    let report = graceful_shutdown(&db, &ws, &background, &runs, guard, false, spent).await;
    let elapsed = started.elapsed();

    assert!(report.ws_drain_timed_out, "{report:?}");
    assert!(report.storage_close_timed_out, "{report:?}");
    // R9-F04: with ZERO remaining, the record-cleanup phase is a timeout
    // BY DEFINITION — the deadline already passed. The old contract let
    // the synchronous release complete inside the first poll and return
    // Ok unflagged (the budget bounded only waiting, never the work);
    // the phase now runs on a dedicated thread under the real timeout,
    // so it is REPORTED as timed out, while the detached worker still
    // ATTEMPTS the removal — bounded-observed here (in the product the
    // deterministic process exit bounds it instead).
    assert!(report.record_cleanup_timed_out, "{report:?}");
    assert_eq!(report.exit_code(), 6, "{report:?}");
    let removal_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while layout.record_path.exists() {
        assert!(
            std::time::Instant::now() < removal_deadline,
            "the detached record-cleanup worker should still remove the record: {report:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        elapsed < Duration::from_millis(2_000),
        "an exhausted budget must bound every phase immediately, took {elapsed:?}"
    );
    ws.connection_closed();
    let _ = std::fs::remove_dir_all(&home);
}

/// The transport drain itself is inside the budget (F03 core): a connection
/// that sent a partial HTTP body and then stalled can no longer hold the
/// server past the deadline — pre-fix this exact shape kept the process
/// alive indefinitely (review: 1010ms past a 100ms budget, still alive).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_drain_timeout_abandons_a_stuck_partial_body_connection() {
    let home = temp_home("drain");
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    let config = lingxi_service::ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        data_home: home.clone(),
        home_source: lingxi_service::HomeSource::Cli,
        network_mode: lingxi_service::NetworkMode::Loopback,
        shutdown_timeout_ms: 100,
    };
    let state = lingxi_service::ServiceState::bootstrap(config, &layout)
        .await
        .expect("bootstrap");
    // Observation handle for the accept-edge state (the stuck connection
    // must be TRACKED before the stop signal — see below).
    let state_view = state.clone();
    let token = {
        let raw = std::fs::read_to_string(home.join("lingxi-service").join("local-token.json"))
            .expect("token file");
        let json: serde_json::Value = serde_json::from_str(&raw).expect("token json");
        json["token"].as_str().expect("token").to_string()
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<std::net::SocketAddr>();
    let serve = tokio::spawn(async move {
        lingxi_service::run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            Some(Duration::from_millis(100)),
        )
        .await
    });
    let addr = ready_rx.await.expect("ready");

    // The stuck client: authenticated POST, declared Content-Length 1000,
    // only "{" ever sent — then it just holds the socket.
    let mut stuck = tokio::net::TcpStream::connect(addr).await.expect("connect");
    use tokio::io::AsyncWriteExt as _;
    stuck
        .write_all(
            format!(
                "POST /lingxi/v1/sessions/sess_local_alpha/execute HTTP/1.1\r\n\
                 Host: {addr}\r\n\
                 Authorization: Bearer {token}\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: 1000\r\n\r\n{{"
            )
            .as_bytes(),
        )
        .await
        .expect("partial write");

    // Deterministic setup (R2-F04 follow-up): the stop signal must land
    // only AFTER the stuck connection is accepted and tracked. A completed
    // loopback handshake sits in the listen backlog until the serve loop's
    // `accept` polls it, and the accept-vs-signal select fairly races the
    // two — a signal that wins first is LEGITIMATE product behavior (a
    // never-accepted socket is simply dropped with the listener; there is
    // nothing to drain). The property under test is the drain of a
    // connection the server had actually taken, so wait for the transport
    // admission gate to observe it.
    let accept_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while state_view.connection_admission().current() != 1 {
        assert!(
            std::time::Instant::now() < accept_deadline,
            "the stuck connection was never accepted"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let started = std::time::Instant::now();
    stop_tx.send(()).expect("stop");
    let outcome = tokio::time::timeout(Duration::from_secs(5), serve)
        .await
        .expect("the drain budget must bound the serve loop")
        .expect("serve task")
        .expect("serve outcome");
    let elapsed = started.elapsed();
    assert!(
        outcome.drain_timed_out,
        "the stuck connection must outlive the drain budget: {outcome:?}"
    );
    assert!(
        elapsed < Duration::from_millis(2_000),
        "the 100ms drain budget must bound the wait, took {elapsed:?}"
    );

    drop(stuck);
    let _ = std::fs::remove_dir_all(&home);
}
