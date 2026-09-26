//! R02-A08｜重放迁移幂等（REQUIRED）— integration against real files.
//!
//! Given: a database already upgraded to the target version.
//! When: startup/migration check runs repeatedly (>= 3 times).
//! Then: no duplicate tables/data (row counts stable), the version never
//! regresses, the schema fingerprint stays identical; a tampered version
//! rollback attempt is rejected loudly (no silent replay).
//!
//! Test layer: real file system + real SQLite (no mocks). The same
//! semantics are additionally covered at binary level by
//! scripts/rust-tauri/r02_t04_storage_tx.sh.

use std::path::{Path, PathBuf};

use lingxi_adapters::storage::{migrations, RunDatabase, StoreOptions};
use lingxi_kernel::ports::StorageError;

fn temp_db(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("lingxi-r02t04-a08-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.join("runs.db")
}

fn schema_fingerprint(path: &PathBuf) -> String {
    let conn = rusqlite::Connection::open(path).expect("open for fingerprint");
    let mut stmt = conn
        .prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
        .expect("prepare sqlite_master query");
    let rows: Vec<String> = stmt
        .query_map([], |row| {
            Ok(format!(
                "{}|{}|{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default()
            ))
        })
        .expect("query sqlite_master")
        .map(|r| r.expect("row"))
        .collect();
    let joined = rows.join("\n");
    let digest = sha2::Sha256::digest(joined.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

use sha2::Digest;

async fn open_store(path: &Path) -> RunDatabase {
    RunDatabase::open(path, StoreOptions::default())
        .await
        .expect("open run database")
}

#[tokio::test]
async fn repeated_migration_checks_are_idempotent() {
    let path = temp_db("idempotent");
    let _ = std::fs::remove_file(&path);

    // Baseline: create, migrate, seed one session, write one full run.
    let db = open_store(&path).await;
    db.ensure_session_seed(vec![lingxi_adapters::storage::SessionRow {
        session_id: "sess_a".into(),
        agent_id: "lingxi".into(),
        owner_user_id: "user_local".into(),
        title: "t".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .expect("seed");
    let ctx = run_ctx("r-1");
    use lingxi_kernel::ports::StoragePort as _;
    db.record_run_started(&ctx, 100).await.expect("start");
    db.commit_run_outcome(&ctx, completed_outcome(), 200)
        .await
        .expect("commit");
    db.close().await.expect("close");
    let fingerprint_before = schema_fingerprint(&path);
    let counts_before = table_counts(&path);

    // Re-run the open-time migration check at least 3 more times.
    for round in 1..=3u32 {
        let db = open_store(&path).await;
        let version: String = db
            .query_one_text(
                "SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1",
                vec![],
            )
            .await
            .expect("query version")
            .expect("receipt row");
        assert_eq!(version, "1", "round {round}: version must not move");
        // Close WITHOUT new writes; counts must be byte-stable.
        db.close().await.expect("close");
        assert_eq!(
            schema_fingerprint(&path),
            fingerprint_before,
            "round {round}: schema fingerprint must be identical"
        );
        assert_eq!(
            table_counts(&path),
            counts_before,
            "round {round}: row counts must be stable (no replayed data)"
        );
    }

    // The compiled-in fingerprint and the on-disk receipt agree.
    let db = open_store(&path).await;
    let receipt_fp = db
        .query_one_text(
            "SELECT fingerprint FROM schema_migrations WHERE version = 1",
            vec![],
        )
        .await
        .expect("query receipt")
        .expect("receipt row");
    assert_eq!(receipt_fp, migrations::fingerprint_sql(migrations::V1_SQL));
    db.close().await.expect("close");

    let _ = std::fs::remove_file(&path);
}

fn table_counts(path: &PathBuf) -> Vec<(String, i64)> {
    let conn = rusqlite::Connection::open(path).expect("open for counts");
    let tables = [
        "schema_migrations",
        "sessions",
        "runs",
        "run_attempts",
        "invocations",
        "messages",
        "key_events",
    ];
    tables
        .iter()
        .map(|t| {
            let n: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                .expect("count");
            (t.to_string(), n)
        })
        .collect()
}

#[tokio::test]
async fn tampered_version_rollback_is_rejected_not_replayed() {
    let path = temp_db("tamper-rollback");
    let _ = std::fs::remove_file(&path);
    let db = open_store(&path).await;
    db.close().await.expect("close");

    // Tamper: roll the recorded receipt version AND user_version back so a
    // naive migrator would "replay" migration 1 over existing tables.
    {
        let conn = rusqlite::Connection::open(&path).expect("open tamper");
        conn.execute(
            "UPDATE schema_migrations SET version = 99 WHERE version = 1",
            [],
        )
        .expect("tamper receipts");
        conn.pragma_update(None, "user_version", 99)
            .expect("tamper pragma");
    }
    match RunDatabase::open(&path, StoreOptions::default()).await {
        // Both variants are LOUD rejections with no replay; which one
        // fires depends on the check order (contiguity inspects position
        // 0 before the version comparison).
        Err(StorageError::DatabaseTooNew {
            found_version,
            supported_version,
        }) => {
            assert_eq!(found_version, 99);
            assert_eq!(supported_version, migrations::supported_version());
        }
        Err(StorageError::SchemaTampered { detail }) => {
            assert!(detail.contains("contiguous"), "detail: {detail}")
        }
        Err(other) => panic!("expected DatabaseTooNew/SchemaTampered, got Err({other})"),
        Ok(db) => panic!("expected rejection, got Ok({:?})", db.db_path()),
    }

    // Tamper shape 2: delete the receipt row (version bookkeeping gap)
    // while the tables still exist.
    {
        let conn = rusqlite::Connection::open(&path).expect("open tamper2");
        conn.execute(
            "UPDATE schema_migrations SET version = 1 WHERE version = 99",
            [],
        )
        .expect("restore row");
        conn.pragma_update(None, "user_version", 1)
            .expect("restore pragma");
        conn.execute("DELETE FROM schema_migrations WHERE version = 1", [])
            .expect("delete receipt");
        conn.pragma_update(None, "user_version", 0)
            .expect("zero pragma");
    }
    match RunDatabase::open(&path, StoreOptions::default()).await {
        Err(StorageError::SchemaTampered { detail }) => {
            assert!(
                detail.contains("non-system table") || detail.contains("disagrees"),
                "detail: {detail}"
            );
        }
        Err(other) => panic!("expected SchemaTampered, got Err({other})"),
        Ok(db) => panic!("expected SchemaTampered, got Ok({:?})", db.db_path()),
    }

    let _ = std::fs::remove_file(&path);
}

fn run_ctx(run: &str) -> lingxi_kernel::RunContext {
    lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_a"),
        run_id: lingxi_protocol::RunId::new(run.to_string()),
        attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}

fn completed_outcome() -> lingxi_kernel::ports::RunOutcome {
    use lingxi_kernel::ports::KeyEvent;
    lingxi_kernel::ports::RunOutcome {
        status: lingxi_protocol::RunStatus::Completed,
        reason: None,
        key_events: vec![KeyEvent {
            event_id: lingxi_protocol::EventId::new("evt-done"),
            payload: lingxi_protocol::EventPayload::Known(
                lingxi_protocol::KnownEventPayload::RunStateChanged(
                    lingxi_protocol::RunStateChangedPayload {
                        from: lingxi_protocol::RunStatus::Running,
                        to: lingxi_protocol::RunStatus::Completed,
                        reason: None,
                    },
                ),
            ),
        }],
        final_message: None,
    }
}
