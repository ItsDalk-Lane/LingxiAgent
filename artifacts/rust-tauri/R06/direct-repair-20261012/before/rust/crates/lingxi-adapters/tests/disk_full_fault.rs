//! R02-A07｜提交失败无假成功（REQUIRED）— REAL kernel-level disk-write
//! fault, dedicated test PROCESS.
//!
//! Fault injection method (argued in R02-T04_REPORT.md): `setrlimit(
//! RLIMIT_FSIZE)` makes the next WAL append that would grow the file past
//! the limit fail with EFBIG inside the kernel — a REAL write error on
//! the REAL database files, delivered to SQLite at COMMIT time through
//! the already-open WAL descriptor. (An earlier candidate — macOS
//! `chflags uchg` — was probed and rejected: it blocks OPENING the file
//! for write but not writes through an already-open descriptor, so it
//! cannot fault an established connection. Plain chmod has the same
//! limitation.)
//!
//! This file is a SEPARATE test binary on purpose: RLIMIT_FSIZE and the
//! SIGXFSZ disposition are per-process state; running the fault window in
//! the shared test process would corrupt unrelated parallel tests.
//! The other storage tests (crash windows, atomicity) live in
//! storage_transactions.rs.

use std::path::PathBuf;

use lingxi_adapters::storage::{RunDatabase, StoreOptions};
use lingxi_kernel::ports::{RunOutcome, StorageError, StoragePort};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    AttemptId, EventId, EventPayload, KnownEventPayload, RunId, RunStateChangedPayload, RunStatus,
    SessionId,
};

fn temp_db(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r02t04-diskfull-{}-{}",
        std::process::id(),
        tag
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

/// Raw independent read of the run state (the same read path
/// lingxi-storage-inspect uses).
fn raw_run_state(path: &PathBuf, run: &str) -> (Option<String>, i64) {
    let conn = rusqlite::Connection::open(path).expect("raw open");
    let status: Option<String> = conn
        .query_row("SELECT status FROM runs WHERE run_id = ?1", [run], |r| {
            r.get(0)
        })
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .expect("query runs");
    let events: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_id = ?2",
            rusqlite::params![run, format!("{run}-done")],
            |r| r.get(0),
        )
        .expect("query key_events");
    (status, events)
}

#[cfg(unix)]
mod unix_fault {
    use super::*;

    /// Serialized fault window (this process runs only the tests in this
    /// file, but the window is still mutex-guarded for future additions).
    /// The guard is held across awaits ON PURPOSE: the RLIMIT_FSIZE window
    /// must span the whole async commit, and this binary is dedicated to
    /// these tests (no other task can contend).
    static FAULT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serialized_fault_window() -> std::sync::MutexGuard<'static, ()> {
        FAULT_MUTEX.lock().expect("fault mutex")
    }

    // The per-test `#[tokio::test]` bodies below hold the guard across
    // awaits on purpose; the allow lives on the helper and each call site
    // is annotated here once (clippy attributes on expressions are not
    // stable, so the functions carry the allowance).

    struct SoftFsizeLimit {
        previous: Option<libc::rlimit>,
    }

    impl SoftFsizeLimit {
        /// Lowers the SOFT RLIMIT_FSIZE only (the hard limit stays, so the
        /// original value can be restored). SIGXFSZ is ignored process-wide
        /// so the over-limit write surfaces as EFBIG instead of killing the
        /// process.
        fn lower(max_bytes: libc::rlim_t) -> Self {
            unsafe {
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                let mut current: libc::rlimit = std::mem::zeroed();
                let rc = libc::getrlimit(libc::RLIMIT_FSIZE, &mut current);
                assert_eq!(rc, 0, "getrlimit RLIMIT_FSIZE");
                let mut lowered = current;
                lowered.rlim_cur = max_bytes;
                let rc = libc::setrlimit(libc::RLIMIT_FSIZE, &lowered);
                assert_eq!(rc, 0, "setrlimit RLIMIT_FSIZE soft");
                Self {
                    previous: Some(current),
                }
            }
        }

        fn restore(&mut self) {
            if let Some(previous) = self.previous.take() {
                unsafe {
                    let rc = libc::setrlimit(libc::RLIMIT_FSIZE, &previous);
                    assert_eq!(rc, 0, "restore RLIMIT_FSIZE");
                }
            }
        }
    }

    impl Drop for SoftFsizeLimit {
        fn drop(&mut self) {
            self.restore();
        }
    }

    /// The A07 core scenario: the commit of the terminal status + its
    /// completion event hits a REAL kernel write failure (EFBIG when the
    /// WAL append would grow the file past the limit). The caller receives
    /// an explicit error and no events; after the fault clears and the
    /// database is reopened, there is NO half terminal state — and the
    /// identical commit then succeeds.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn commit_fails_for_real_at_commit_time_without_fake_success() {
        let _guard = serialized_fault_window();
        let path = temp_db("efbig");
        let db = RunDatabase::open(&path, StoreOptions::default())
            .await
            .expect("open");
        db.ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
            session_id: "sess_a".into(),
            agent_id: "lingxi".into(),
            owner_user_id: "user_local".into(),
            title: "t".into(),
            created_at_unix_ms: 1,
        }])
        .await
        .expect("seed");
        let run_ctx = ctx("r-efbig");
        db.record_run_started(&run_ctx, 100).await.expect("start");

        // Grow the WAL deterministically past the limit inside the fault
        // window: the commit must append frames to the WAL to durably
        // record the terminal + event, and that append is the write that
        // fails.
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let wal_size = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        let mut limit = SoftFsizeLimit::lower((wal_size + 8) as libc::rlim_t);

        let result = db
            .commit_run_outcome(&run_ctx, completed("r-efbig"), 200)
            .await;
        limit.restore();
        match &result {
            Err(err @ StorageError::DiskFull { .. }) => {
                assert!(
                    err.retryable(),
                    "disk-full is retryable after space is freed"
                );
            }
            Err(err @ StorageError::Io { .. }) => {
                assert!(!err.retryable());
            }
            Err(other) => panic!("expected DiskFull/Io, got Err({other})"),
            Ok(committed) => panic!(
                "FAULT INJECTION FAILED: commit succeeded under RLIMIT_FSIZE \
                 (events: {})",
                committed.events.len()
            ),
        }

        // Reopen (the store itself stays usable after the fault cleared)
        // and prove no half terminal state exists.
        let (status, done_events) = raw_run_state(&path, "r-efbig");
        assert_eq!(
            status.as_deref(),
            Some("running"),
            "failed commit must leave NO terminal status"
        );
        assert_eq!(
            done_events, 0,
            "failed commit must leave NO completion event"
        );
        // The identical commit succeeds once the fault is gone — the
        // earlier failure left no residue to conflict with.
        db.commit_run_outcome(&run_ctx, completed("r-efbig"), 300)
            .await
            .expect("retry after fault recovery commits cleanly");
        let (status, done_events) = raw_run_state(&path, "r-efbig");
        assert_eq!(status.as_deref(), Some("completed"));
        assert_eq!(done_events, 1);
        db.close().await.expect("close");
        let _ = std::fs::remove_file(&path);
    }

    /// A second real-fault shape: a start-commit also fails (not only the
    /// terminal commit) — proving the failure is not a special case of
    /// the finalize path.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn start_commit_also_fails_under_the_fault() {
        let _guard = serialized_fault_window();
        let path = temp_db("efbig-start");
        let db = RunDatabase::open(&path, StoreOptions::default())
            .await
            .expect("open");
        db.ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
            session_id: "sess_a".into(),
            agent_id: "lingxi".into(),
            owner_user_id: "user_local".into(),
            title: "t".into(),
            created_at_unix_ms: 1,
        }])
        .await
        .expect("seed");
        // The seed itself grew the WAL; measure after it.
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let wal_size = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        let mut limit = SoftFsizeLimit::lower((wal_size + 8) as libc::rlim_t);
        let result = db.record_run_started(&ctx("r-start"), 100).await;
        limit.restore();
        match result {
            Err(StorageError::DiskFull { .. }) | Err(StorageError::Io { .. }) => {}
            Err(other) => panic!("expected DiskFull/Io, got Err({other})"),
            Ok(_) => panic!("FAULT INJECTION FAILED: start commit succeeded under the limit"),
        }
        let runs: i64 = {
            let conn = rusqlite::Connection::open(&path).expect("raw open");
            conn.query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))
                .expect("count runs")
        };
        assert_eq!(runs, 0, "failed start leaves no run row");
        db.close().await.expect("close");
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(not(unix))]
mod unix_fault {
    // Platform boundary: RLIMIT_FSIZE is a unix facility; recorded in the
    // report. Windows fault injection is not exercised in R02-T04.
}
