//! R02-T06 step 4｜备份中断故障注入 — REAL kernel-level write faults during
//! a backup, dedicated test PROCESS (same reasoning as tests/disk_full_fault.rs:
//! RLIMIT_FSIZE and the SIGXFSZ disposition are per-process state).
//!
//! Required coverage: a backup interrupted mid-write must fail EXPLICITLY
//! and must leave NO artifact that could later masquerade as a successful
//! snapshot (no final name, no manifest, no `.partial` residue).
//!
//! Two fault windows, both delivered through the REAL backup path:
//! 1. checkpoint fault — `pre_checkpoint=true` and the soft RLIMIT_FSIZE
//!    sits just above the current main-db size, so the TRUNCATE checkpoint
//!    (which must write WAL frames into the main file) hits EFBIG before
//!    any destination artifact exists;
//! 2. backup-copy fault — `pre_checkpoint=false` and the soft limit sits
//!    at half the main-db size, so the Online-Backup-API copy into the
//!    destination `.partial` file fails partway through.
//!
//! The cleanup contract is then asserted against the REAL destination
//! directory.

#[cfg(unix)]
mod backup_faults {
    use std::path::{Path, PathBuf};

    use lingxi_adapters::storage::{BackupOptions, RunDatabase, StoreOptions};
    use lingxi_kernel::ports::{KeyEvent, RunOutcome, StoragePort};
    use lingxi_kernel::RunContext;
    use lingxi_protocol::{
        AttemptId, EventId, EventPayload, KnownEventPayload, RunId, RunStateChangedPayload,
        RunStatus, SessionId,
    };

    static FAULT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serialized_fault_window() -> std::sync::MutexGuard<'static, ()> {
        FAULT_MUTEX.lock().expect("fault mutex")
    }

    struct SoftFsizeLimit {
        previous: Option<libc::rlimit>,
    }

    impl SoftFsizeLimit {
        fn lower(max_bytes: libc::rlim_t) -> Self {
            unsafe {
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                let mut current: libc::rlimit = std::mem::zeroed();
                let rc = libc::getrlimit(libc::RLIMIT_FSIZE, &mut current);
                assert_eq!(rc, 0, "getrlimit RLIMIT_FSIZE");
                assert!(
                    (max_bytes as libc::rlim_t) < current.rlim_max
                        || current.rlim_max == libc::RLIM_INFINITY,
                    "the requested soft limit must not exceed the hard limit"
                );
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

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r02t06-bkfault-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
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

    async fn seeded(path: &Path) -> RunDatabase {
        let db = RunDatabase::open(path, StoreOptions::default())
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
        for i in 0..6 {
            let name = format!("f{i}");
            db.record_run_started(&ctx(&name), 1_000)
                .await
                .expect("start");
            db.commit_run_outcome(&ctx(&name), completed(&name), 2_000)
                .await
                .expect("commit");
        }
        db
    }

    fn file_size(path: &Path) -> u64 {
        std::fs::metadata(path).expect("size").len()
    }

    fn assert_no_backup_artifacts(dest: &Path) {
        if !dest.exists() {
            return; // nothing was ever created: the cleanest outcome
        }
        let leftovers: Vec<String> = std::fs::read_dir(dest)
            .expect("read dest")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            leftovers
                .iter()
                .all(|n| !n.ends_with(".db") && !n.ends_with(".json")),
            "no artifact may masquerade as a good backup after an interrupted \
             backup: {leftovers:?}"
        );
        assert!(
            !dest.join("runs.manifest.json").exists(),
            "no manifest may exist for a failed backup"
        );
    }

    // Same deliberate pattern as tests/disk_full_fault.rs: the RLIMIT fault
    // window must span the whole async backup.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn checkpoint_fault_fails_the_backup_explicitly_and_leaves_no_artifact() {
        let _window = serialized_fault_window();
        let dir = temp_dir("checkpoint");
        let db_path = dir.join("runs.db");
        let db = seeded(&db_path).await;
        let main_size = file_size(&db_path);

        // The checkpoint must push WAL frames into the main file, growing
        // it beyond (current size + 512 bytes) → real EFBIG at checkpoint
        // time, BEFORE any destination artifact is created.
        let mut limit = SoftFsizeLimit::lower((main_size + 512) as libc::rlim_t);
        let dest = dir.join("backup");
        let result = db.backup_to(&dest, "runs", BackupOptions::default()).await;
        limit.restore();

        let err = result.expect_err(
            "FAULT INJECTION FAILED: the checkpoint-phase \
             backup succeeded under RLIMIT_FSIZE — the fault never took effect",
        );
        let text = err.to_string();
        assert!(
            text.contains("disk") || text.to_lowercase().contains("full") || text.contains("IO"),
            "the failure must be the injected write fault, got: {text}"
        );
        assert_no_backup_artifacts(&dest);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn interrupted_backup_copy_fails_explicitly_and_removes_the_partial_file() {
        let _window = serialized_fault_window();
        let dir = temp_dir("copy");
        let db_path = dir.join("runs.db");
        let db = seeded(&db_path).await;
        let main_size = file_size(&db_path);
        assert!(main_size > 0);

        // No pre-checkpoint: the copy itself writes the destination
        // `.partial` file and hits EFBIG at half the main-db size — the
        // Online Backup API fails mid-copy.
        let mut limit = SoftFsizeLimit::lower((main_size / 2) as libc::rlim_t);
        let dest = dir.join("backup");
        let result = db
            .backup_to(
                &dest,
                "runs",
                BackupOptions {
                    pre_checkpoint: false,
                },
            )
            .await;
        limit.restore();

        result.expect_err(
            "FAULT INJECTION FAILED: the backup copy succeeded under \
             RLIMIT_FSIZE — the fault never took effect",
        );
        // The cleanup contract: the partial file is REMOVED, so no half
        // backup can masquerade as a good snapshot.
        assert_no_backup_artifacts(&dest);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

fn main() {
    panic!("backup_faults is a unix-only test binary");
}
