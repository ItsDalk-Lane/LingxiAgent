//! R02-T05 adapters-layer event reads: the durable key_events log through
//! the real SQLite file — head/floor/events_after fidelity, the retention
//! purge that creates the expired-cursor precondition, and the loud
//! corruption check when the stored event_type disagrees with the payload.

use lingxi_adapters::storage::{RunDatabase, StoreOptions};
use lingxi_kernel::ports::{EventStorePort, StoragePort};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{
    AttemptId, EventPayload, KnownEventPayload, RunId, RunStateChangedPayload, RunStatus, Seq,
    SessionId,
};

fn temp_db(tag: &str) -> (RunDatabase, std::path::PathBuf) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r02t05-adapter-{}-{}-{tag}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("runs.db");
    let db = tokio_block_on(RunDatabase::open(&path, StoreOptions::default())).expect("open db");
    // The port refuses runs outside a session; seed the test session.
    tokio_block_on(
        db.ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
            session_id: "sess_stream".to_string(),
            agent_id: "lingxi".to_string(),
            owner_user_id: "user_local".to_string(),
            title: "event store test".to_string(),
            created_at_unix_ms: 0,
        }]),
    )
    .expect("seed session");
    (db, dir)
}

/// Minimal single-thread executor for these tests (adapters' tokio feature
/// set includes rt; using it directly keeps the test dependency surface at
/// zero).
fn tokio_block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime")
        .block_on(fut)
}

fn ctx(run: &str) -> RunContext {
    RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess_stream".to_string()),
        run_id: RunId::new(run.to_string()),
        attempt: AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}

/// Commits a run through the real port: 2 key events per run on the
/// session stream (`sess_stream`), seqs assigned by the single writer.
async fn commit_run(db: &RunDatabase, run: &str, now_ms: u64) {
    db.record_run_started(&ctx(run), now_ms)
        .await
        .expect("start");
    let outcome = lingxi_kernel::ports::RunOutcome {
        status: RunStatus::Completed,
        reason: None,
        key_events: vec![lingxi_kernel::ports::KeyEvent {
            event_id: lingxi_protocol::EventId::new(format!("{run}-done")),
            payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                RunStateChangedPayload {
                    from: RunStatus::Running,
                    to: RunStatus::Completed,
                    reason: None,
                },
            )),
        }],
        final_message: None,
    };
    db.commit_run_outcome(&ctx(run), outcome, now_ms)
        .await
        .expect("commit");
}

#[test]
fn head_floor_and_events_after_read_the_durable_log() {
    let (db, dir) = temp_db("reads");
    tokio_block_on(async {
        assert_eq!(db.stream_head("sess_stream").await.unwrap(), None);
        assert_eq!(db.stream_floor("sess_stream").await.unwrap(), None);
        assert!(db
            .stream_events_after("sess_stream", Seq::new(0), 100)
            .await
            .unwrap()
            .is_empty());

        for i in 0..5 {
            commit_run(&db, &format!("run_{i}"), 1000 + i).await;
        }
        // 10 events, seqs 1..=10 assigned by the single writer.
        assert_eq!(
            db.stream_head("sess_stream").await.unwrap(),
            Some(Seq::new(10))
        );
        assert_eq!(
            db.stream_floor("sess_stream").await.unwrap(),
            Some(Seq::new(1))
        );

        let all = db
            .stream_events_after("sess_stream", Seq::new(0), 100)
            .await
            .unwrap();
        assert_eq!(all.len(), 10);
        assert_eq!(all[0].seq, Seq::new(1));
        assert_eq!(all[9].seq, Seq::new(10));
        // Envelope fidelity: run_id/attempt/eventType/payload all round-trip.
        assert_eq!(
            all[0].run_id.as_ref().map(|r| r.to_string()).unwrap(),
            "run_0"
        );
        assert_eq!(
            all[0].attempt.as_ref().map(|a| a.to_string()).unwrap(),
            "run_0#a1"
        );
        assert_eq!(all[0].event_type, "run_state_changed");
        assert_eq!(all[0].stream_id.to_string(), "sess_stream");

        // after=3, limit=4 → seqs 4..=7.
        let page = db
            .stream_events_after("sess_stream", Seq::new(3), 4)
            .await
            .unwrap();
        let seqs: Vec<u64> = page.iter().map(|e| e.seq.value()).collect();
        assert_eq!(seqs, vec![4, 5, 6, 7]);
    });
    tokio_block_on(db.close()).expect("close");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn purge_moves_the_floor_and_reports_removed_rows() {
    let (db, dir) = temp_db("purge");
    tokio_block_on(async {
        for i in 0..5 {
            commit_run(&db, &format!("run_{i}"), 2000 + i).await;
        }
        // Retention: drop seqs < 5 (exclusive) → 4 rows removed.
        let removed = db
            .purge_events_before("sess_stream", Seq::new(5))
            .await
            .unwrap();
        assert_eq!(removed, 4);
        assert_eq!(
            db.stream_floor("sess_stream").await.unwrap(),
            Some(Seq::new(5))
        );
        assert_eq!(
            db.stream_head("sess_stream").await.unwrap(),
            Some(Seq::new(10))
        );
        let remaining = db
            .stream_events_after("sess_stream", Seq::new(0), 100)
            .await
            .unwrap();
        let seqs: Vec<u64> = remaining.iter().map(|e| e.seq.value()).collect();
        assert_eq!(seqs, vec![5, 6, 7, 8, 9, 10]);
        // Purging an empty range is a no-op (never an error).
        let again = db
            .purge_events_before("sess_stream", Seq::new(5))
            .await
            .unwrap();
        assert_eq!(again, 0);
        // Unknown stream: zero rows, no error.
        assert_eq!(
            db.purge_events_before("sess_other", Seq::new(9))
                .await
                .unwrap(),
            0
        );
    });
    tokio_block_on(db.close()).expect("close");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn event_type_column_tampering_is_loud_corruption() {
    let (db, dir) = temp_db("tamper");
    tokio_block_on(async {
        commit_run(&db, "run_x", 3000).await;
    });
    // Out-of-band tamper (the WAL permits a second connection): rewrite
    // the stored event_type so it disagrees with the payload tag.
    {
        let conn = rusqlite::Connection::open(dir.join("runs.db")).expect("open direct");
        conn.execute(
            "UPDATE key_events SET event_type = 'tool_call_completed' WHERE seq = 1",
            [],
        )
        .expect("tamper");
    }
    tokio_block_on(async {
        let err = db
            .stream_events_after("sess_stream", Seq::new(0), 10)
            .await
            .expect_err("tampered row must fail the read");
        match err {
            lingxi_kernel::ports::StorageError::Corrupted { detail } => {
                assert!(detail.contains("tool_call_completed"), "detail: {detail}");
            }
            other => panic!("expected Corrupted, got {other:?}"),
        }
    });
    tokio_block_on(db.close()).expect("close");
    let _ = std::fs::remove_dir_all(dir);
}
