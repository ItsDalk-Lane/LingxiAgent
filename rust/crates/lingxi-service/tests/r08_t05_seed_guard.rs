//! R08-T05 P9 (adjudicated 2026-10-23): the durable session seed is a
//! fresh-database parity artifact (R02-T03 in-memory seed). The legacy Node
//! stack never contained the synthetic ids in any form, so seeding into a
//! non-empty (e.g. adopted/migrated) database exceeds the old stack's shape
//! and breaks the R08-T05 self-check count invariant. The production boot
//! path therefore uses `ensure_session_seed_if_empty`; the unguarded
//! `ensure_session_seed` stays the general store/test API, unchanged.
//! These tests pin all three states of the guarded variant.
use lingxi_adapters::storage::{RunDatabase, SessionRow, StoreOptions};
use std::{path::PathBuf, sync::Arc};

fn seed_rows() -> Vec<SessionRow> {
    vec![
        SessionRow {
            session_id: "sess_local_alpha".into(),
            agent_id: "agent".into(),
            owner_user_id: "user_local".into(),
            title: "Synthetic session alpha".into(),
            created_at_unix_ms: 1,
        },
        SessionRow {
            session_id: "sess_local_beta".into(),
            agent_id: "agent".into(),
            owner_user_id: "user_local".into(),
            title: "Synthetic session beta".into(),
            created_at_unix_ms: 1,
        },
    ]
}

async fn fresh_db(tag: &str) -> (Arc<RunDatabase>, PathBuf) {
    let home = std::env::temp_dir().join(format!(
        "r08-t05-seed-guard-{}-{}",
        tag,
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let db = Arc::new(
        RunDatabase::open(&home.join("runs.db"), StoreOptions::default())
            .await
            .unwrap(),
    );
    (db, home)
}

async fn done(home: PathBuf, db: Arc<RunDatabase>) {
    drop(db);
    let _ = std::fs::remove_dir_all(home);
}

#[tokio::test]
async fn empty_database_is_seeded() {
    let (db, home) = fresh_db("empty").await;
    db.ensure_session_seed_if_empty(seed_rows()).await.unwrap();
    let sessions = db.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 2, "fresh database receives the full seed");
    assert!(sessions
        .iter()
        .any(|row| row.session_id == "sess_local_alpha"));
    assert!(sessions
        .iter()
        .any(|row| row.session_id == "sess_local_beta"));
    done(home, db).await;
}

#[tokio::test]
async fn non_empty_database_without_seed_ids_is_not_seeded() {
    // The R08-T05 adopted-root shape: imported rows exist, synthetic ids absent.
    let (db, home) = fresh_db("adopted").await;
    db.ensure_session_seed(vec![SessionRow {
        session_id: "sess-imported-0001".into(),
        agent_id: "agent".into(),
        owner_user_id: "user_local".into(),
        title: "Imported legacy session".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .unwrap();
    db.ensure_session_seed_if_empty(seed_rows()).await.unwrap();
    let sessions = db.list_sessions().await.unwrap();
    assert_eq!(
        sessions.len(),
        1,
        "adopted root keeps exactly its imported rows (no +2 seed delta)"
    );
    assert_eq!(sessions[0].session_id, "sess-imported-0001");
    done(home, db).await;
}

#[tokio::test]
async fn non_empty_database_with_seed_ids_is_a_no_op() {
    // Existing installs already carry the seed rows; repeat boots change nothing.
    let (db, home) = fresh_db("existing").await;
    db.ensure_session_seed_if_empty(seed_rows()).await.unwrap();
    let before = db.list_sessions().await.unwrap();
    db.ensure_session_seed_if_empty(seed_rows()).await.unwrap();
    let after = db.list_sessions().await.unwrap();
    assert_eq!(before.len(), 2);
    assert_eq!(
        before.iter().map(|row| row.session_id.clone()).collect::<Vec<_>>(),
        after.iter().map(|row| row.session_id.clone()).collect::<Vec<_>>(),
        "repeat seeding on an existing install is a strict no-op"
    );
    done(home, db).await;
}

#[tokio::test]
async fn unguarded_seed_still_inserts_into_a_non_empty_database() {
    // The general store/test API contract is unchanged: callers like the
    // r07_bridge_notice fixture seed distinct rows into the same database
    // across calls.
    let (db, home) = fresh_db("unguarded").await;
    db.ensure_session_seed(vec![SessionRow {
        session_id: "sess-first".into(),
        agent_id: "agent".into(),
        owner_user_id: "user_local".into(),
        title: "First".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .unwrap();
    db.ensure_session_seed(vec![SessionRow {
        session_id: "sess-second".into(),
        agent_id: "agent".into(),
        owner_user_id: "user_local".into(),
        title: "Second".into(),
        created_at_unix_ms: 2,
    }])
    .await
    .unwrap();
    assert_eq!(
        db.list_sessions().await.unwrap().len(),
        2,
        "unguarded ensure_session_seed keeps its non-empty-table contract"
    );
    done(home, db).await;
}
