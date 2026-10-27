//! F14-C01：真实新库、旧版升级与重复启动的登记/数据库收据相等检查。

use std::path::{Path, PathBuf};
use std::process::Command;

use lingxi_adapters::storage::{migrations, RunDatabase, StoreOptions};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn test_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lingxi-f14-c01-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn registry() -> Vec<Value> {
    // 登记表是独立期望值，不能用被测程序自己的编译列表代替。
    let doc: Value = serde_json::from_str(include_str!(
        "../../../../docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json"
    ))
    .unwrap();
    let points = doc["new_persistence_points"].as_array().unwrap();
    assert_eq!(points.len(), 1);
    let mut rows: Vec<_> = points[0]["migrations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            json!({"version": row["version"], "name": row["name"],
                   "fingerprint": row["fingerprint_sha256"]})
        })
        .collect();
    rows.sort_by_key(|row| row["version"].as_u64().unwrap());
    assert!(!rows.is_empty());
    assert_eq!(
        rows.iter()
            .map(|row| row["version"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        (1..=rows.len() as u64).collect::<Vec<_>>()
    );
    rows
}

fn digest(path: &Path) -> String {
    Sha256::digest(std::fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn inspect(path: &Path, command: &str) -> Value {
    let before = digest(path);
    let output = Command::new(env!("CARGO_BIN_EXE_lingxi-storage-inspect"))
        .arg(path)
        .arg(command)
        .output()
        .expect("运行真实数据库检查工具");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(digest(path), before, "只读检查不能改写数据库");
    serde_json::from_slice(&output.stdout).expect("检查工具输出真实JSON")
}

fn receipt_identity(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .map(|row| json!({"version": row["version"], "name": row["name"], "fingerprint": row["fingerprint"]}))
        .collect()
}

fn assert_registry_equal(doc: &Value, registered: &[Value]) {
    let receipts = doc["receipts"].as_array().unwrap();
    let compiled = doc["compiledIn"].as_array().unwrap();
    assert_eq!(doc["userVersion"], json!(registered.len()));
    assert_eq!(doc["supportedVersion"], json!(registered.len()));
    assert_eq!(
        receipt_identity(receipts).as_slice(),
        registered,
        "数据库收据必须严格等于登记表"
    );
    assert_eq!(
        compiled.as_slice(),
        registered,
        "检查工具编译列表必须严格等于登记表"
    );
}

fn old_database(path: &Path, version: u64) {
    // 复用已有 session_file_store 旧版夹具方法，按原SQL顺序建立真实旧库。
    let conn = rusqlite::Connection::open(path).unwrap();
    for migration in migrations::MIGRATIONS
        .iter()
        .filter(|row| row.version <= version)
    {
        conn.execute_batch(migration.sql).unwrap();
    }
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
    for migration in migrations::MIGRATIONS
        .iter()
        .filter(|row| row.version <= version)
    {
        conn.execute(
            "INSERT INTO schema_migrations VALUES (?1, ?2, ?3, 1000, 'old-fixture')",
            rusqlite::params![
                migration.version as i64,
                migration.name,
                migrations::fingerprint_sql(migration.sql)
            ],
        )
        .unwrap();
    }
    conn.pragma_update(None, "user_version", i64::try_from(version).unwrap())
        .unwrap();
    conn.execute_batch(
        "INSERT INTO sessions (session_id,agent_id,owner_user_id,title,created_at_unix_ms)
         VALUES ('old-session','agent','user_local','旧数据原样保留',1);
         INSERT INTO runs (run_id,session_id,owner_kind,owner_subject,principal_id,status,generation,created_at_unix_ms,updated_at_unix_ms)
         VALUES ('old-run','old-session','local','user_local','local_user','completed',1,1,1);
         INSERT INTO messages (message_id,session_id,run_id,role,content_json,committed_at_unix_ms,seq)
         VALUES ('old-message','old-session','old-run','assistant','[{\"type\":\"text\",\"text\":\"旧消息\"}]',1,1);",
    )
    .unwrap();
}

fn schema(path: &Path) -> Vec<(String, String, Option<String>)> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut statement = conn
        .prepare("SELECT type,name,sql FROM sqlite_master ORDER BY type,name")
        .unwrap();
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows
}

async fn open_and_close(path: &Path) {
    let db = RunDatabase::open(path, StoreOptions::default())
        .await
        .unwrap();
    db.close().await.unwrap();
}

async fn check(tag: &str, old_version: Option<u64>) {
    let root = test_root(tag);
    let path = root.join("runs.db");
    let registered = registry();
    let old = old_version.map(|version| {
        old_database(&path, version);
        let doc = inspect(&path, "migrations");
        assert_eq!(doc["userVersion"], json!(version));
        assert_eq!(
            receipt_identity(doc["receipts"].as_array().unwrap()).as_slice(),
            &registered[..version as usize]
        );
        doc
    });

    // 由真实存储启动入口完成新建或升级，不在测试中模拟升级结果。
    open_and_close(&path).await;
    let baseline = inspect(&path, "migrations");
    assert_registry_equal(&baseline, &registered);
    if let Some(old) = old.as_ref() {
        let old_receipts = old["receipts"].as_array().unwrap();
        assert_eq!(
            &baseline["receipts"].as_array().unwrap()[..old_receipts.len()],
            old_receipts.as_slice(),
            "升级不能重写任何历史收据字段"
        );
        let conn = rusqlite::Connection::open(&path).unwrap();
        let title: String = conn
            .query_row(
                "SELECT title FROM sessions WHERE session_id='old-session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let message: String = conn
            .query_row(
                "SELECT content_json FROM messages WHERE message_id='old-message'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "旧数据原样保留");
        assert_eq!(message, r#"[{"type":"text","text":"旧消息"}]"#);
    }
    let counts = inspect(&path, "counts");
    let structure = schema(&path);
    let mut rounds = Vec::new();
    for round in 1..=3 {
        open_and_close(&path).await;
        let actual = inspect(&path, "migrations");
        assert_registry_equal(&actual, &registered);
        assert_eq!(actual, baseline, "第{round}次启动不能改写收据");
        assert_eq!(inspect(&path, "counts"), counts, "重复启动不能重复写入数据");
        assert_eq!(schema(&path), structure, "重复启动不能改变数据库结构");
        rounds.push(actual);
    }
    let evidence = json!({"id":"R06-AR-F14-C01","fixture":tag,"oldVersion":old_version,
        "registered":registered,"beforeUpgrade":old,"afterUpgrade":baseline,"restartRounds":rounds,
        "counts":counts,"restartCount":3,"strictEquality":true,"historicalReceiptsPreserved":true});
    println!("f14_migration_receipts_evidence={evidence}");
    if let Some(output) = std::env::var_os("LINGXI_F14_EVIDENCE") {
        let output = PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            output.join(format!("{tag}.json")),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn c01_fresh_database_receipts_equal_registry_and_inspector() {
    check("fresh", None).await;
}

#[tokio::test]
async fn c01_v7_upgrade_preserves_receipts_and_restarts_are_idempotent() {
    check("v7", Some(7)).await;
}

#[tokio::test]
async fn c01_v8_upgrade_preserves_receipts_and_restarts_are_idempotent() {
    check("v8", Some(8)).await;
}
