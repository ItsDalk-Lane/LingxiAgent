//! W09：真实 SQLite 的完整计划集合、非法向量与事务回滚。
use lingxi_adapters::storage::knowledge as ks;
use std::path::PathBuf;
fn root(label: &str) -> PathBuf {
    let p = PathBuf::from(std::env::var("LINGXI_HOME").unwrap())
        .join(format!("vector-integrity-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn entry(id: &str, ordinal: usize, vector: Vec<f32>) -> ks::VectorEntry {
    ks::VectorEntry {
        chunk_id: id.into(),
        ordinal,
        vector,
    }
}
#[test]
fn exact_plan_rejects_wrong_ids_even_when_count_and_ordinals_match() {
    let p = root("wrong-id");
    let s = ks::KnowledgeVectorStore::open(&p).unwrap();
    let b = s
        .begin_variant_build("civ", "artifact", "model", "fp", 2)
        .unwrap();
    s.upsert_batch(
        &b.variant_id,
        &[
            entry("wrong", 0, vec![1., 0.]),
            entry("second", 1, vec![0., 1.]),
        ],
    )
    .unwrap();
    assert!(s
        .complete_build_plan(
            &b.variant_id,
            &[("expected".into(), 0), ("second".into(), 1)]
        )
        .is_err());
    assert_ne!(
        s.get_variant(&b.variant_id).unwrap().unwrap().status,
        "ready"
    );
    assert!(s
        .complete_build_plan(&b.variant_id, &[("wrong".into(), 0), ("second".into(), 1)])
        .is_ok());
}
#[test]
fn malformed_batches_and_corrupt_checkpoints_are_refused() {
    let p = root("invalid");
    let s = ks::KnowledgeVectorStore::open(&p).unwrap();
    let b = s
        .begin_variant_build("civ", "artifact", "model", "fp", 2)
        .unwrap();
    for entries in [
        vec![entry("a", 0, vec![f32::NAN, 0.])],
        vec![entry("a", 0, vec![f32::INFINITY, 0.])],
        vec![entry("a", 0, vec![1.])],
        vec![entry("a", 0, vec![1., 0.]), entry("a", 1, vec![0., 1.])],
        vec![entry("a", 0, vec![1., 0.]), entry("b", 0, vec![0., 1.])],
    ] {
        assert!(s.upsert_batch(&b.variant_id, &entries).is_err());
        assert!(s.checkpoint_entries(&b.variant_id).unwrap().is_empty());
    }
    s.upsert_batch(&b.variant_id, &[entry("a", 0, vec![1., 0.])])
        .unwrap();
    let db = rusqlite::Connection::open(p.join("knowledge_vector.db")).unwrap();
    db.execute(
        "UPDATE chunk_vectors SET vector=?1",
        [f32::NAN.to_le_bytes().repeat(2)],
    )
    .unwrap();
    assert!(s.checkpoint_entries(&b.variant_id).is_err());
    assert!(s
        .complete_build_plan(&b.variant_id, &[("a".into(), 0)])
        .is_err());
    assert_ne!(
        s.get_variant(&b.variant_id).unwrap().unwrap().status,
        "ready"
    );
}
#[test]
fn mid_batch_write_failure_rolls_back_every_new_row_and_preserves_paid_bytes() {
    let p = root("rollback");
    let s = ks::KnowledgeVectorStore::open(&p).unwrap();
    let b = s
        .begin_variant_build("civ", "artifact", "model", "fp", 2)
        .unwrap();
    s.upsert_batch(&b.variant_id, &[entry("paid", 0, vec![1., 0.])])
        .unwrap();
    let db = rusqlite::Connection::open(p.join("knowledge_vector.db")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_second BEFORE INSERT ON chunk_vectors WHEN NEW.ordinal=2 BEGIN SELECT RAISE(ABORT,'synthetic partial write'); END;").unwrap();
    assert!(s
        .upsert_batch(
            &b.variant_id,
            &[
                entry("first-new", 1, vec![1., 0.]),
                entry("second-new", 2, vec![0., 1.])
            ]
        )
        .is_err());
    let entries = s.checkpoint_entries(&b.variant_id).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].chunk_id, "paid");
    assert_eq!(entries[0].vector, vec![1., 0.]);
    assert!(s
        .begin_variant_build("civ", "artifact", "model", "changed-fp", 2)
        .is_err());
    assert_eq!(s.checkpoint_entries(&b.variant_id).unwrap().len(), 1);
}

#[tokio::test]
async fn transport_observations_preserve_connect_retryability_and_unpolled_cancellation() {
    use lingxi_adapters::models::{credentials::ApplicableAuth, dispatch};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let client = dispatch::build_client().unwrap();
    let timeouts = dispatch::HttpTimeouts::default();
    let seen = std::sync::Mutex::new(Vec::new());
    let observer = |fact| {
        seen.lock().unwrap().push(fact);
        Ok(())
    };
    let unpolled = dispatch::send_with_timeouts_observed(
        client
            .post(format!("http://{addr}/v1/embeddings"))
            .json(&serde_json::json!({"input":["synthetic"]})),
        &timeouts,
        None,
        &ApplicableAuth::None,
        Some(&observer),
    );
    drop(unpolled);
    assert!(seen.lock().unwrap().is_empty());
    let error = dispatch::send_with_timeouts_observed(
        client
            .post(format!("http://{addr}/v1/embeddings"))
            .json(&serde_json::json!({"input":["synthetic"]})),
        &timeouts,
        None,
        &ApplicableAuth::None,
        Some(&observer),
    )
    .await
    .unwrap_err();
    assert!(error.1);
    assert!(error.0.retryable);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            dispatch::SendObservation::AttemptStarted,
            dispatch::SendObservation::ConnectFailedBeforeSend
        ]
    );
}

#[tokio::test]
async fn storage_close_is_once_and_concurrent_waiters_and_cancelled_waiter_keep_actual_join() {
    use lingxi_adapters::storage::{RunDatabase, StoreOptions};
    use std::sync::Arc;
    let db = Arc::new(
        RunDatabase::open(
            &root("close-concurrent").join("runs.db"),
            StoreOptions::default(),
        )
        .await
        .unwrap(),
    );
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let hook: lingxi_adapters::storage::run_store::BeforeCloseHook = {
        let count = count.clone();
        let entered = entered.clone();
        let release = release.clone();
        Arc::new(move || {
            let count = count.clone();
            let entered = entered.clone();
            let release = release.clone();
            Box::pin(async move {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                entered.add_permits(1);
                release.acquire().await.unwrap().forget();
                Ok(())
            })
        })
    };
    db.register_before_close(hook.clone()).unwrap();
    assert!(db.register_before_close(hook).is_err());
    let first = tokio::spawn({
        let db = db.clone();
        async move { db.close().await }
    });
    entered.acquire().await.unwrap().forget();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let second = tokio::spawn({
        let db = db.clone();
        async move { db.close().await }
    });
    let third = tokio::spawn({
        let db = db.clone();
        async move { db.close().await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!second.is_finished() && !third.is_finished());
    release.add_permits(1);
    second.await.unwrap().unwrap();
    third.await.unwrap().unwrap();
    db.close().await.unwrap();
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(db.run_fact_summary().await.is_err());
}
#[tokio::test]
async fn storage_close_callback_error_and_panic_are_explicit_and_database_is_still_reaped() {
    use lingxi_adapters::storage::{RunDatabase, StoreOptions};
    use std::sync::Arc;
    for mode in ["error", "panic"] {
        let db = RunDatabase::open(
            &root(&format!("close-{mode}")).join("runs.db"),
            StoreOptions::default(),
        )
        .await
        .unwrap();
        db.register_before_close(Arc::new(move || {
            Box::pin(async move {
                if mode == "panic" {
                    panic!("W09 injected close callback panic");
                }
                Err(lingxi_kernel::ports::StorageError::Internal {
                    detail: "W09 injected close callback error".into(),
                })
            })
        }))
        .unwrap();
        let first = db.close().await.unwrap_err();
        let again = db.close().await.unwrap_err();
        assert_eq!(first, again);
        assert!(first.to_string().contains("W09 injected close callback"));
        assert!(db.run_fact_summary().await.is_err());
    }
}

#[tokio::test]
async fn storage_close_keeps_both_callback_error_and_real_pinned_wal_checkpoint_failure() {
    use lingxi_adapters::storage::{RunDatabase, SessionRow, StoreOptions};
    use std::sync::Arc;
    let path = root("close-double-error").join("runs.db");
    let db = RunDatabase::open(
        &path,
        StoreOptions {
            busy_timeout_ms: 50,
            ..StoreOptions::default()
        },
    )
    .await
    .unwrap();
    let reader =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    reader.execute_batch("BEGIN").unwrap();
    let count: i64 = reader
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    db.ensure_session_seed(vec![SessionRow {
        session_id: "w09-synthetic".into(),
        agent_id: "synthetic".into(),
        owner_user_id: "synthetic".into(),
        title: "synthetic".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .unwrap();
    db.register_before_close(Arc::new(|| {
        Box::pin(async {
            Err(lingxi_kernel::ports::StorageError::Internal {
                detail: "W09 callback error with pinned reader".into(),
            })
        })
    }))
    .unwrap();
    let error = db.close().await.unwrap_err();
    let text = error.to_string();
    assert!(
        text.contains("resource close failed")
            && text.contains("W09 callback error with pinned reader")
            && text.contains("database close failed"),
        "{text}"
    );
    assert_eq!(db.close().await.unwrap_err(), error);
    assert!(db.run_fact_summary().await.is_err());
    reader.execute_batch("ROLLBACK").unwrap();
    drop(reader);
    let reopened = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(reopened.list_sessions().await.unwrap().len(), 1);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn bare_database_keeps_original_double_close_error_and_refuses_late_resource_registration() {
    use lingxi_adapters::storage::{RunDatabase, StoreOptions};
    use lingxi_kernel::ports::StorageError;
    use std::sync::Arc;
    let home = root("bare-double-close");
    let db = RunDatabase::open(&home.join("runs.db"), StoreOptions::default())
        .await
        .unwrap();
    db.close().await.unwrap();
    assert_eq!(db.close().await.unwrap_err(), StorageError::QueueClosed);
    assert!(db
        .register_before_close(Arc::new(|| Box::pin(async { Ok(()) })))
        .is_err());
    assert!(db.run_fact_summary().await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_registration_racing_close_is_either_joined_once_or_explicitly_refused() {
    use lingxi_adapters::storage::{RunDatabase, StoreOptions};
    use std::sync::Arc;
    for i in 0..32 {
        let db = Arc::new(
            RunDatabase::open(
                &root(&format!("registration-race-{i}")).join("runs.db"),
                StoreOptions::default(),
            )
            .await
            .unwrap(),
        );
        let start = Arc::new(tokio::sync::Barrier::new(3));
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registration = tokio::spawn({
            let db = db.clone();
            let start = start.clone();
            let count = count.clone();
            async move {
                start.wait().await;
                db.register_before_close(Arc::new(move || {
                    let count = count.clone();
                    Box::pin(async move {
                        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    })
                }))
            }
        });
        let close = tokio::spawn({
            let db = db.clone();
            let start = start.clone();
            async move {
                start.wait().await;
                db.close().await
            }
        });
        start.wait().await;
        let registered = registration.await.unwrap().is_ok();
        close.await.unwrap().unwrap();
        assert_eq!(
            count.load(std::sync::atomic::Ordering::SeqCst),
            usize::from(registered)
        );
        assert!(db.run_fact_summary().await.is_err());
        assert!(db
            .register_before_close(Arc::new(|| Box::pin(async { Ok(()) })))
            .is_err());
    }
}
