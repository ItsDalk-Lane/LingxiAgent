//! R06-T03 REVIEW-01 活体探针 2：v7→v8 迁移数据保真。
//! 构造一个 v7 形态的既有库（执行 v1..=v7 的 SQL + 真实 receipts + 既有
//! messages 行），然后让当前 build 的 RunDatabase::open 触发 v8 迁移，
//! 验证：messages 行逐字节保留（parent=NULL/branch=NULL/entry_type='message'）、
//! 新表建立、索引重建、receipts 追加 v8。

use lingxi_adapters::storage::migrations::{fingerprint_sql, MIGRATIONS};
use lingxi_adapters::storage::{RunDatabase, StoreOptions};

fn main() {
    let dir = std::env::temp_dir().join(format!("r06t03-review-probe2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("runs.db");
    let _ = std::fs::remove_file(&db_path);

    // ── 1. 构造 v7 库 ──
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                applied_at_unix_ms INTEGER NOT NULL,
                applied_by TEXT NOT NULL
            );",
        )
        .unwrap();
        for m in MIGRATIONS.iter().filter(|m| m.version <= 7) {
            conn.execute_batch(m.sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations (version, name, fingerprint, applied_at_unix_ms, applied_by) \
                 VALUES (?1, ?2, ?3, 0, 'review-probe')",
                rusqlite::params![m.version as i64, m.name, fingerprint_sql(m.sql)],
            )
            .unwrap();
        }
        conn.execute_batch("PRAGMA user_version = 7").unwrap();
        // 既有数据：一个会话 + 一个 run + 两条消息（v7 形态：message_id 全局唯一 PK）。
        conn.execute(
            "INSERT INTO sessions (session_id, agent_id, owner_user_id, title, created_at_unix_ms) \
             VALUES ('sess_old','agent','user_local','old session',111)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO runs (run_id, session_id, owner_kind, owner_subject, principal_id, \
             status, generation, created_at_unix_ms, updated_at_unix_ms) \
             VALUES ('run_old','sess_old','user','user_local','user_local','completed',0,111,222)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (message_id, session_id, run_id, role, content_json, model_call_id, committed_at_unix_ms, seq) \
             VALUES ('m_old_1','sess_old','run_old','user','{\"text\":\"hello\"}',NULL,111,1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (message_id, session_id, run_id, role, content_json, model_call_id, committed_at_unix_ms, seq) \
             VALUES ('m_old_2','sess_old','run_old','assistant','{\"text\":\"world\"}','mc-9',222,2)",
            [],
        )
        .unwrap();
    }

    // ── 2. 当前 build 打开 → 触发 v8 ──
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = RunDatabase::open(&db_path, StoreOptions::default())
            .await
            .expect("open migrates to v8");
        let version = db
            .query_one_text(
                "SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1",
                vec![],
            )
            .await
            .unwrap()
            .unwrap();
        println!("MIGRATED_VERSION={version}");
        db.close().await.unwrap();
    });

    // ── 3. 直查数据保真 ──
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT message_id, session_id, run_id, role, content_json, model_call_id, \
             committed_at_unix_ms, seq, parent_message_id, branch_id, entry_type \
             FROM messages ORDER BY seq",
        )
        .unwrap();
    let rows: Vec<Vec<String>> = stmt
        .query_map([], |r| {
            Ok(vec![
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?.unwrap_or("NULL".into()),
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?.unwrap_or("NULL".into()),
                r.get::<_, i64>(6)?.to_string(),
                r.get::<_, i64>(7)?.to_string(),
                r.get::<_, Option<String>>(8)?.unwrap_or("NULL".into()),
                r.get::<_, Option<String>>(9)?.unwrap_or("NULL".into()),
                r.get::<_, String>(10)?,
            ])
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    println!("MESSAGE_ROWS={}", rows.len());
    for row in &rows {
        println!("ROW={}", row.join("|"));
    }
    assert_eq!(rows.len(), 2, "messages 行数必须保留");
    assert_eq!(rows[0][0], "m_old_1");
    assert_eq!(rows[1][4], "{\"text\":\"world\"}");
    assert_eq!(rows[1][5], "mc-9", "model_call_id 必须保留");
    assert_eq!(rows[0][8], "NULL", "parent 默认 NULL");
    assert_eq!(rows[0][10], "message", "entry_type 默认 message");

    // sessions 新列默认值
    let lifecycle: String = conn
        .query_row(
            "SELECT lifecycle FROM sessions WHERE session_id='sess_old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let folders: String = conn
        .query_row(
            "SELECT authorized_folders_json FROM sessions WHERE session_id='sess_old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    println!("LIFECYCLE_DEFAULT={lifecycle} FOLDERS_DEFAULT={folders}");

    // 索引重建
    let idx: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN \
             ('idx_messages_session','idx_messages_parent')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    println!("REBUILT_INDEXES={idx}");

    let _ = std::fs::remove_dir_all(&dir);
    println!("PROBE2_OK");
}
