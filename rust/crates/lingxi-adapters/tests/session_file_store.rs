//! R06-T05 adapters 层测试：v9 迁移（session_files / aliases / refs /
//! file_history_snapshots）与 SessionFileStore 存取语义。
//!
//! 对照纪律（RC-3）：
//! - sf_ 存储语义对照现役 `lib/session-files/session-file-registry.ts`
//!   sidecar（version 1: files map + refs 数组）的字段集——候选以 SQLite
//!   行承载同一字段集（差异台账 D1：sidecar JSON → DB 表）。
//! - 迁移链保真对照 `migrations.rs` 框架契约：v7→v8→v9 顺序应用、
//!   fingerprint 收据 + PRAGMA user_version 双记录、旧表数据零损耗。
//! - 文件历史存储对照现役 `lib/file-history/history-store.ts`
//!   （workspace_hash 分桶 + rel_path 版本序列 + 快照内容可读回）。

use lingxi_adapters::storage::migrations::{
    apply_all, fingerprint_sql, supported_version, MIGRATIONS,
};
use lingxi_adapters::storage::session_files as sf;
use lingxi_adapters::storage::{RunDatabase, StoreOptions};

fn temp_db(tag: &str) -> (RunDatabase, std::path::PathBuf) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t05-sf-{}-{}-{tag}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("runs.db");
    let db = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(RunDatabase::open(&path, StoreOptions::default()))
        .expect("open db");
    (db, dir)
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(fut)
}

/// 造一行最小合法 SessionFileRow；字段默认值与现役 registerFile 落库
/// 字段一一对应（sidecar files map 的 value 形状）。
fn row(file_id: &str, owner_session_id: &str, source_key: Option<&str>) -> sf::SessionFileRow {
    sf::SessionFileRow {
        file_id: file_id.to_string(),
        owner_session_id: owner_session_id.to_string(),
        owner_key: format!("id:{owner_session_id}"),
        source_key: source_key.map(str::to_string),
        identity_key: format!("/real/{file_id}"),
        file_path: format!("/shown/{file_id}.txt"),
        real_path: format!("/real/{file_id}.txt"),
        storage_kind: sf::StorageKind::ManagedCache,
        status: sf::SessionFileStatus::Available,
        label: Some(format!("label-{file_id}")),
        filename: format!("{file_id}.txt"),
        mime: "text/plain".to_string(),
        size_bytes: 42,
        mtime_ms: 1_700_000_000_000,
        is_directory: false,
        file_kind: sf::FileKind::Document,
        origin: sf::SessionFileOrigin::UserUpload,
        registered_at_ms: 1_000,
        updated_at_ms: 1_000,
        expires_at_ms: None,
        legacy_file_ids: vec![],
        legacy_file_paths: vec![],
    }
}

// ── 迁移链保真（v7 旧库升级到当前顶端）──

#[test]
fn supported_tip_matches_the_current_migration_registry() {
    // 当前顶端固定为 v23；历史 v9、v13 的独立身份不能随顶端被改名。
    assert_eq!(supported_version(), 23);
    let tip = MIGRATIONS.iter().last().expect("tip");
    assert_eq!(tip.version, 23);
    assert_eq!(tip.name, "r07_media_gallery");
    let v22 = MIGRATIONS.iter().find(|m| m.version == 22).expect("v22");
    assert_eq!(v22.name, "r07_beautify_jobs");
    let v21 = MIGRATIONS.iter().find(|m| m.version == 21).expect("v21");
    assert_eq!(v21.name, "r07_media_jobs");
    let v20 = MIGRATIONS.iter().find(|m| m.version == 20).expect("v20");
    assert_eq!(v20.name, "r07_browser_authority");
    let v19 = MIGRATIONS.iter().find(|m| m.version == 19).expect("v19");
    assert_eq!(v19.name, "r07_conversation_phone_collab_receipts");
    let v9 = MIGRATIONS.iter().find(|m| m.version == 9).expect("v9");
    assert_eq!(v9.name, "r06_t05_session_files_resources");
    let v13 = MIGRATIONS.iter().find(|m| m.version == 13).expect("v13");
    assert_eq!(v13.name, "r06_session_workspace_identity");
}

fn migration_receipts(conn: &rusqlite::Connection) -> Vec<(i64, String, String, i64, String)> {
    conn.prepare("SELECT version, name, fingerprint, applied_at_unix_ms, applied_by FROM schema_migrations ORDER BY version")
        .expect("prepare receipts")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .expect("query receipts")
        .collect::<Result<Vec<_>, _>>()
        .expect("read receipts")
}

#[test]
fn v7_to_tip_chain_preserves_prior_data_and_creates_new_tables() {
    // 造一个停在 v7 的库：手工顺序执行 V1..=V7 SQL + 收据 + user_version，
    // 塞入旧表数据，再交给 apply_all 补 v8..=v14。
    let conn = rusqlite::Connection::open_in_memory().expect("mem db");
    for m in MIGRATIONS.iter().filter(|m| m.version <= 7) {
        conn.execute_batch(m.sql).expect("apply old sql");
    }
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
            version            INTEGER PRIMARY KEY,
            name               TEXT NOT NULL,
            fingerprint        TEXT NOT NULL,
            applied_at_unix_ms INTEGER NOT NULL,
            applied_by         TEXT NOT NULL
        );",
    )
    .expect("receipts ddl");
    for m in MIGRATIONS.iter().filter(|m| m.version <= 7) {
        conn.execute(
            "INSERT INTO schema_migrations (version, name, fingerprint, applied_at_unix_ms, applied_by)
             VALUES (?1, ?2, ?3, ?4, 'test')",
            rusqlite::params![m.version as i64, m.name, fingerprint_sql(m.sql), 1_000i64],
        )
        .expect("receipt row");
    }
    conn.pragma_update(None, "user_version", 7).expect("pragma");
    // 旧表事实：一个会话（v7 列全集）。
    conn.execute(
        "INSERT INTO sessions (session_id, agent_id, owner_user_id, title, created_at_unix_ms)
         VALUES ('s-old', 'agent', 'user_local', 't', 1)",
        [],
    )
    .expect("seed session");

    let old_receipts = migration_receipts(&conn);
    let out = apply_all(&conn, "test", 2_000).expect("apply v8..=v23");
    assert_eq!(
        out.applied,
        vec![8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23]
    );
    assert_eq!(out.current_version, 23);
    assert_eq!(out.supported_version, 23);

    // 旧数据零损耗。
    let title: String = conn
        .query_row(
            "SELECT title FROM sessions WHERE session_id = 's-old'",
            [],
            |r| r.get(0),
        )
        .expect("old row survives");
    assert_eq!(title, "t");
    // v8 既有列已被 v8 迁移赋予（lifecycle 默认 active）。
    let lifecycle: String = conn
        .query_row(
            "SELECT lifecycle FROM sessions WHERE session_id = 's-old'",
            [],
            |r| r.get(0),
        )
        .expect("v8 column present");
    assert_eq!(lifecycle, "active");
    // v9 新表全部存在且无 IF NOT EXISTS 静默。
    for table in [
        "session_files",
        "session_file_aliases",
        "session_file_refs",
        "file_history_snapshots",
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .expect("table probe");
        assert_eq!(n, 1, "missing v9 table {table}");
    }
    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .expect("user_version");
    // 当前顶端 v23（v21 r07_media_jobs、v22 r07_beautify_jobs、v23 r07_media_gallery），与
    // supported_tip_matches_the_current_migration_registry 的登记断言一致。
    assert_eq!(user_version, 23);
    // v23 工作台媒体任务投影表存在且挂在媒体作业的外键上。
    let gallery_table: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='media_gallery_v1'",
            [],
            |r| r.get(0),
        )
        .expect("gallery table probe");
    assert_eq!(gallery_table, 1, "missing v23 table media_gallery_v1");

    // 升级前的旧会话默认关闭主动委派，且列本身有非空与默认值约束。
    let delegation: i64 = conn
        .query_row(
            "SELECT proactive_delegation FROM sessions WHERE session_id = 's-old'",
            [],
            |r| r.get(0),
        )
        .expect("old session delegation");
    assert_eq!(delegation, 0);
    let column: (String, i64, String) = conn.query_row(
        "SELECT type, \"notnull\", dflt_value FROM pragma_table_info('sessions') WHERE name = 'proactive_delegation'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).expect("v14 column metadata");
    assert_eq!(column, ("INTEGER".to_string(), 1, "0".to_string()));

    // 实际收据逐项对独立登记；旧收据的时间和写入者也必须原样保留。
    let receipts = migration_receipts(&conn);
    assert_eq!(&receipts[..7], old_receipts.as_slice());
    let registry: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json"
    ))
    .expect("independent migration registry");
    let registered = registry["new_persistence_points"][0]["migrations"]
        .as_array()
        .expect("registered migrations");
    assert_eq!(registered.len(), 23);
    assert_eq!(receipts.len(), 23);
    for (receipt, registered) in receipts.iter().zip(registered) {
        assert_eq!(
            receipt.0,
            registered["version"].as_i64().expect("registered version")
        );
        assert_eq!(
            receipt.1,
            registered["name"].as_str().expect("registered name")
        );
        assert_eq!(
            receipt.2,
            registered["fingerprint_sha256"]
                .as_str()
                .expect("registered fingerprint")
        );
        assert_eq!(receipt.3, if receipt.0 <= 7 { 1_000 } else { 2_000 });
        assert_eq!(receipt.4, "test");
    }
    let schema_version: i64 = conn
        .pragma_query_value(None, "schema_version", |r| r.get(0))
        .expect("schema version");
    let again = apply_all(&conn, "second-open", 3_000).expect("idempotent reopen");
    assert!(again.applied.is_empty());
    assert_eq!(again.current_version, 23);
    assert_eq!(again.supported_version, 23);
    assert_eq!(migration_receipts(&conn), receipts);
    assert_eq!(
        conn.pragma_query_value(None, "schema_version", |r| r.get::<_, i64>(0))
            .expect("unchanged schema"),
        schema_version
    );
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .expect("unchanged version"),
        23
    );
    let old_row: (String, String, String, i64, String, i64) = conn.query_row(
        "SELECT agent_id, owner_user_id, title, created_at_unix_ms, lifecycle, proactive_delegation FROM sessions WHERE session_id = 's-old'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    ).expect("unchanged old row");
    assert_eq!(
        old_row,
        (
            "agent".to_string(),
            "user_local".to_string(),
            "t".to_string(),
            1,
            "active".to_string(),
            0
        )
    );
}

#[test]
fn v14_delegation_accepts_only_boolean_values_and_rejects_null() {
    let conn = rusqlite::Connection::open_in_memory().expect("mem db");
    apply_all(&conn, "constraint-test", 1_000).expect("apply current schema");
    conn.execute(
        "INSERT INTO sessions (session_id, agent_id, owner_user_id, title, created_at_unix_ms) VALUES ('constraint-row', 'agent', 'user_local', 'unchanged', 1)",
        [],
    ).expect("insert with default");
    for value in [0_i64, 1] {
        assert_eq!(
            conn.execute(
                "UPDATE sessions SET proactive_delegation = ?1 WHERE session_id = 'constraint-row'",
                [value]
            )
            .expect("valid boolean"),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT proactive_delegation FROM sessions WHERE session_id = 'constraint-row'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .expect("boolean readback"),
            value
        );
    }
    // 真实写入必须由 SQLite 拒绝，不能只检查 SQL 字符串里是否有约束。
    for (value, expected_code) in [
        (Some(2_i64), rusqlite::ffi::SQLITE_CONSTRAINT_CHECK),
        (Some(-1), rusqlite::ffi::SQLITE_CONSTRAINT_CHECK),
        (None, rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL),
    ] {
        let error = conn.execute("UPDATE sessions SET proactive_delegation = ?1, title = 'must-rollback' WHERE session_id = 'constraint-row'", [value]).expect_err("invalid value must be rejected");
        match error {
            rusqlite::Error::SqliteFailure(code, _) => {
                assert_eq!(code.extended_code, expected_code)
            }
            other => panic!("unexpected constraint error: {other}"),
        }
        let unchanged: (i64, String) = conn.query_row("SELECT proactive_delegation, title FROM sessions WHERE session_id = 'constraint-row'", [], |r| Ok((r.get(0)?, r.get(1)?))).expect("rejected write preserved row");
        assert_eq!(unchanged, (1, "unchanged".to_string()));
    }
}

// ── SessionFileStore CRUD / 去重 / alias / refs ──

#[test]
fn insert_and_get_roundtrip_preserves_all_fields() {
    let (db, _dir) = temp_db("roundtrip");
    let mut r = row("sf_abcdefghijklmnop", "s1", Some("upload:path:v1:abc"));
    r.legacy_file_ids = vec!["sf_oldone".to_string()];
    r.legacy_file_paths = vec!["/old/path.txt".to_string()];
    block_on(sf::insert_session_file(&db, &r)).expect("insert");
    let got = block_on(sf::get_session_file(&db, "sf_abcdefghijklmnop"))
        .expect("get")
        .expect("present");
    assert_eq!(got, r);
}

#[test]
fn source_key_dedup_is_scoped_to_owner() {
    let (db, _dir) = temp_db("dedup");
    block_on(sf::insert_session_file(
        &db,
        &row("sf_aaaaaaaaaaaaaaaa", "s1", Some("k")),
    ))
    .expect("insert s1");
    // 同 owner 同 sourceKey：唯一索引拒绝（现役 getBySourceKey 复用前置）。
    let dup = block_on(sf::insert_session_file(
        &db,
        &row("sf_bbbbbbbbbbbbbbbb", "s1", Some("k")),
    ));
    assert!(dup.is_err(), "same-owner duplicate source_key must fail");
    // 不同 owner 同 sourceKey：允许。
    block_on(sf::insert_session_file(
        &db,
        &row("sf_bbbbbbbbbbbbbbbb", "s2", Some("k")),
    ))
    .expect("other owner ok");
    let found = block_on(sf::get_session_file_by_source_key(&db, "s1", "k"))
        .expect("query")
        .expect("found");
    assert_eq!(found.file_id, "sf_aaaaaaaaaaaaaaaa");
}

#[test]
fn list_for_owner_orders_by_registration_then_id() {
    let (db, _dir) = temp_db("list");
    let mut a = row("sf_cccccccccccccccc", "s1", None);
    a.registered_at_ms = 2;
    let mut b = row("sf_dddddddddddddddd", "s1", None);
    b.registered_at_ms = 1;
    block_on(sf::insert_session_file(&db, &a)).expect("insert a");
    block_on(sf::insert_session_file(&db, &b)).expect("insert b");
    block_on(sf::insert_session_file(
        &db,
        &row("sf_eeeeeeeeeeeeeeee", "s2", None),
    ))
    .expect("insert other");
    let list = block_on(sf::list_session_files_for_owner(&db, "s1")).expect("list");
    let ids: Vec<&str> = list.iter().map(|r| r.file_id.as_str()).collect();
    assert_eq!(ids, vec!["sf_dddddddddddddddd", "sf_cccccccccccccccc"]);
}

#[test]
fn alias_resolution_maps_legacy_ids_to_canonical() {
    let (db, _dir) = temp_db("alias");
    block_on(sf::insert_session_file(
        &db,
        &row("sf_ffffffffffffffff", "s1", None),
    ))
    .expect("insert");
    block_on(sf::insert_alias(
        &db,
        "sf_legacyoldid0000",
        "sf_ffffffffffffffff",
        5,
    ))
    .expect("alias");
    assert_eq!(
        block_on(sf::resolve_alias(&db, "sf_legacyoldid0000")).expect("resolve"),
        Some("sf_ffffffffffffffff".to_string())
    );
    assert_eq!(
        block_on(sf::resolve_alias(&db, "sf_unknown000000000")).expect("resolve miss"),
        None
    );
    // 现役 getByFilePath 等价：按 real_path 找同 owner 的文件。
    let by_path = block_on(sf::get_session_file_by_real_path(
        &db,
        "s1",
        "/real/sf_ffffffffffffffff.txt",
    ))
    .expect("by path")
    .expect("found");
    assert_eq!(by_path.file_id, "sf_ffffffffffffffff");
}

#[test]
fn status_transitions_record_expiry_timestamp() {
    let (db, _dir) = temp_db("status");
    block_on(sf::insert_session_file(
        &db,
        &row("sf_1111111111111111", "s1", None),
    ))
    .expect("insert");
    block_on(sf::update_session_file_status(
        &db,
        "sf_1111111111111111",
        sf::SessionFileStatus::Expired,
        Some(9_999),
        9_999,
    ))
    .expect("expire");
    let got = block_on(sf::get_session_file(&db, "sf_1111111111111111"))
        .expect("get")
        .expect("present");
    assert_eq!(got.status, sf::SessionFileStatus::Expired);
    assert_eq!(got.expires_at_ms, Some(9_999));
    assert_eq!(got.updated_at_ms, 9_999);
}

#[test]
fn refs_roundtrip_and_session_scoped_delete() {
    let (db, _dir) = temp_db("refs");
    block_on(sf::insert_session_file(
        &db,
        &row("sf_2222222222222222", "s1", None),
    ))
    .expect("insert");
    let refr = sf::SessionFileRefRow {
        file_id: "sf_2222222222222222".to_string(),
        session_id: "s1".to_string(),
        message_id: Some("m1".to_string()),
        ref_kind: sf::RefKind::MessageMarker,
        created_at_ms: 7,
    };
    block_on(sf::insert_ref(&db, &refr)).expect("insert ref");
    let refs = block_on(sf::list_refs_for_session(&db, "s1")).expect("list");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].file_id, "sf_2222222222222222");
    assert_eq!(refs[0].message_id.as_deref(), Some("m1"));
    let for_file = block_on(sf::list_refs_for_file(&db, "sf_2222222222222222")).expect("by file");
    assert_eq!(for_file.len(), 1);
    block_on(sf::delete_refs_for_session(&db, "s1")).expect("delete");
    assert!(block_on(sf::list_refs_for_session(&db, "s1"))
        .expect("list")
        .is_empty());
}

#[test]
fn delete_session_file_removes_row_and_aliases() {
    let (db, _dir) = temp_db("delete");
    block_on(sf::insert_session_file(
        &db,
        &row("sf_3333333333333333", "s1", None),
    ))
    .expect("insert");
    block_on(sf::insert_alias(
        &db,
        "sf_aliasdel0000000",
        "sf_3333333333333333",
        1,
    ))
    .expect("alias");
    block_on(sf::delete_session_file(&db, "sf_3333333333333333")).expect("delete");
    assert!(block_on(sf::get_session_file(&db, "sf_3333333333333333"))
        .expect("get")
        .is_none());
    assert_eq!(
        block_on(sf::resolve_alias(&db, "sf_aliasdel0000000")).expect("resolve"),
        None
    );
}

// ── file_history_snapshots 存取 ──

#[test]
fn file_history_store_lists_files_versions_and_reads_content() {
    let (db, _dir) = temp_db("fh");
    let ws = "0123456789abcdef";
    for (rel, at, origin) in [
        ("src/a.txt", 1_000u64, "baseline"),
        ("src/a.txt", 2_000, "write"),
        ("src/b.txt", 3_000, "edit"),
    ] {
        block_on(sf::insert_file_snapshot(
            &db,
            &sf::FileSnapshotInsert {
                workspace_hash: ws,
                rel_path: rel,
                captured_at_ms: at,
                origin,
                content: format!("content-{rel}-{at}").into_bytes(),
            },
        ))
        .expect("insert snapshot");
    }
    let files = block_on(sf::list_file_history_files(&db, ws)).expect("files");
    assert_eq!(files.len(), 2);
    let a = files.iter().find(|f| f.rel_path == "src/a.txt").expect("a");
    assert_eq!(a.version_count, 2);
    assert_eq!(a.last_captured_at_ms, 2_000);

    let versions =
        block_on(sf::list_file_history_versions(&db, ws, "src/a.txt")).expect("versions");
    assert_eq!(versions.len(), 2);
    // 新版本在前（现役 listVersions 按时间倒序）。
    assert!(versions[0].captured_at_ms > versions[1].captured_at_ms);
    assert_eq!(versions[1].origin, "baseline");

    let snap = block_on(sf::get_file_snapshot(&db, ws, versions[1].id))
        .expect("get")
        .expect("present");
    assert_eq!(snap.rel_path, "src/a.txt");
    assert_eq!(
        String::from_utf8(snap.content).expect("utf-8"),
        "content-src/a.txt-1000"
    );
    // 其他 workspace 隔离。
    assert!(
        block_on(sf::list_file_history_files(&db, "ffffffffffffffff"))
            .expect("other ws")
            .is_empty()
    );
    assert!(block_on(sf::get_file_snapshot(
        &db,
        "ffffffffffffffff",
        versions[1].id
    ))
    .expect("other ws get")
    .is_none());
}

#[test]
fn upsert_session_activity_marks_coldness_for_cleanup_probe() {
    let (db, _dir) = temp_db("cold");
    // 无行 → 插入最小行并写入 last_activity；有行 → 只更新该列。
    block_on(sf::upsert_session_activity(&db, "s-cold", 1_000)).expect("insert arm");
    block_on(sf::upsert_session_activity(&db, "s-cold", 5_000)).expect("update arm");
    block_on(sf::upsert_session_activity(&db, "s-warm", 9_000)).expect("warm row");
    // 冷度探针：cutoff=6_000 → 只有 s-cold 冷。
    let cold = block_on(sf::list_cold_session_ids(&db, 6_000)).expect("cold probe");
    assert_eq!(cold, vec!["s-cold".to_string()]);
}

#[test]
fn file_history_prune_enforces_age_and_total_bytes() {
    let (db, _dir) = temp_db("fhprune");
    let ws = "0123456789abcdef";
    // 三个快照：一个超龄、两个在龄但合计超字节预算。
    for (rel, at, bytes) in [
        ("old.txt", 1_000u64, 10usize),
        ("new1.txt", 9_000, 100),
        ("new2.txt", 9_500, 100),
    ] {
        block_on(sf::insert_file_snapshot(
            &db,
            &sf::FileSnapshotInsert {
                workspace_hash: ws,
                rel_path: rel,
                captured_at_ms: at,
                origin: "write",
                content: vec![b'x'; bytes],
            },
        ))
        .expect("insert");
    }
    // 现役 FILE_HISTORY_DEFAULTS: maxAgeMs=30d, maxTotalBytes=500MB —— 测试用
    // 缩小参数驱动同一保留策略：超龄先删，再按总字节从旧到新删。
    let pruned = block_on(sf::prune_file_history(&db, ws, 5_000, 150, 10_000)).expect("prune");
    assert!(
        pruned >= 2,
        "expected old + oldest in-budget pruned, got {pruned}"
    );
    let files = block_on(sf::list_file_history_files(&db, ws)).expect("files");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].rel_path, "new2.txt");
}
