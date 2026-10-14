//! F12：通过实际缓存入口验证原件保护，不替换被测存储。
use lingxi_adapters::storage::knowledge::{KnowledgeIndexStore, KnowledgeVectorStore};
use rusqlite::Connection;
use std::path::PathBuf;

fn fixture(tag: &str) -> PathBuf {
    let home = PathBuf::from(std::env::var_os("LINGXI_HOME").expect("独立 LINGXI_HOME"));
    let root = home.join(format!(
        "{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    eprintln!("synthetic knowledge root={}", root.display());
    root
}

#[test]
fn positive_normal_cache_open_and_reopen() {
    let root = fixture("positive");
    drop(KnowledgeIndexStore::open(&root).expect("正常索引"));
    drop(KnowledgeVectorStore::open(&root).expect("正常向量"));
    assert!(KnowledgeIndexStore::open(&root)
        .unwrap()
        .list_ready_variant_metadata("absent")
        .unwrap()
        .is_empty());
    assert!(KnowledgeVectorStore::open(&root)
        .unwrap()
        .list_variants_by_artifact("absent")
        .unwrap()
        .is_empty());
}

#[test]
fn c01_future_schema_preserves_sentinel_in_both_caches() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture(name);
        let path = root.join(name);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('unique-paid-vector-or-index'); PRAGMA user_version=999;").unwrap();
        drop(conn);
        let before = std::fs::read(&path).unwrap();
        let result = if name == "knowledge_index.db" {
            KnowledgeIndexStore::open(&root).map(|_| ())
        } else {
            KnowledgeVectorStore::open(&root).map(|_| ())
        };
        let error = result.expect_err("未来版本不能伪装损坏后重建");
        assert_eq!(error.code, "KNOWLEDGE_SCHEMA_NEWER");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("SELECT value FROM sentinel", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "unique-paid-vector-or-index"
        );
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            999
        );
    }
}

fn open_cache(
    root: &std::path::Path,
    name: &str,
) -> Result<(), lingxi_kernel::knowledge::KnowledgeError> {
    if name == "knowledge_index.db" {
        KnowledgeIndexStore::open(root).map(|_| ())
    } else {
        KnowledgeVectorStore::open(root).map(|_| ())
    }
}

fn snapshot(path: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    use sha2::Digest;
    let snapshot = ["", "-wal", "-shm"]
        .into_iter()
        .filter_map(|suffix| {
            let p = PathBuf::from(format!("{}{suffix}", path.display()));
            std::fs::read(p).ok().map(|b| (suffix.to_owned(), b))
        })
        .collect::<Vec<_>>();
    let files = snapshot.iter().map(|(suffix, bytes)| serde_json::json!({
        "suffix": suffix, "bytes": bytes.len(), "sha256": sha2::Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "header": if suffix.is_empty() { Some(bytes.iter().take(100).map(|byte| format!("{byte:02x}")).collect::<String>()) } else { None }
    })).collect::<Vec<_>>();
    eprintln!(
        "{}",
        serde_json::json!({"phase":"ACTUAL_SQLITE_FILE_SNAPSHOT","path":path.display().to_string(),"files":files})
    );
    snapshot
}

#[test]
fn c01_future_wal_live_writer_preserves_all_bytes() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("future-wal");
        open_cache(&root, name).unwrap();
        let path = root.join(name);
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('paid-vector'); PRAGMA user_version=999; BEGIN IMMEDIATE; INSERT INTO sentinel VALUES('active-uncommitted');").unwrap();
        let before = snapshot(&path);
        assert_eq!(before.len(), 3, "夹具必须真的有WAL和SHM");
        let error = open_cache(&root, name).expect_err("未来版本必须拒绝");
        assert_eq!(error.code, "KNOWLEDGE_SCHEMA_NEWER");
        assert_eq!(snapshot(&path), before, "主库及必要WAL/SHM不改变");
        writer.execute_batch("COMMIT;").unwrap();
        assert_eq!(
            writer
                .query_row("SELECT count(*) FROM sentinel", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}

#[test]
fn c02_sqlite_exclusive_lock_refuses_without_removal() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("locked");
        open_cache(&root, name).unwrap();
        let path = root.join(name);
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('lock-evidence'); BEGIN EXCLUSIVE;").unwrap();
        let before = snapshot(&path);
        let error = open_cache(&root, name).expect_err("锁冲突不得伪成功");
        assert_eq!(error.code, "KNOWLEDGE_STORAGE_BUSY", "{error}");
        assert_eq!(snapshot(&path), before);
        writer.execute_batch("COMMIT;").unwrap();
        drop(writer);
        open_cache(&root, name).unwrap();
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("SELECT value FROM sentinel", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "lock-evidence"
        );
    }
}

#[cfg(unix)]
#[test]
fn c02_permission_errors_preserve_cache() {
    use std::os::unix::fs::PermissionsExt;
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("permission");
        open_cache(&root, name).unwrap();
        let path = root.join(name);
        let before = snapshot(&path);
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        let result = open_cache(&root, name);
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let error = result.expect_err("真实权限错误");
        assert_eq!(error.code, "KNOWLEDGE_STORAGE_PERMISSION", "{error}");
        assert_eq!(snapshot(&path), before);
        open_cache(&root, name).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn c02_io_child() {
    let Some(root) = std::env::var_os("F12_IO_ROOT") else {
        return;
    };
    let name = std::env::var("F12_IO_CACHE").unwrap();
    // 限额只作用于此合成子进程，避免影响并行验收者。
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        let cap = std::env::var("F12_IO_LIMIT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let limit = libc::rlimit {
            rlim_cur: cap,
            rlim_max: cap,
        };
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
    }
    let error = open_cache(std::path::Path::new(&root), &name).expect_err("实际文件写限额应失败");
    assert_eq!(error.code, "KNOWLEDGE_STORAGE_IO", "{error}");
}

/// 子进程输出逐行加前缀后再回显：原样回显会让子进程的 `test … ok` 行混入父测试输出，
/// 门禁的单测试结算检查会因此误判。
fn quote_child(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|line| format!("\n  | {line}"))
        .collect()
}

#[cfg(unix)]
#[test]
fn c02_actual_io_limit_preserves_original() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("io-limit");
        open_cache(&root, name).unwrap();
        let path = root.join(name);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('io-evidence');").unwrap();
        drop(conn);
        let before = std::fs::read(&path).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "c02_io_child", "--nocapture"])
            .env("F12_IO_ROOT", &root)
            .env("F12_IO_CACHE", name)
            .output()
            .unwrap();
        eprintln!(
            "I/O child status={} stdout={} stderr={}",
            output.status,
            quote_child(&output.stdout),
            quote_child(&output.stderr)
        );
        assert!(output.status.success());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        open_cache(&root, name).unwrap();
    }
}

fn quarantine_originals(root: &std::path::Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".quarantine-")
                .then(|| path.join("original.db"))
        })
        .collect()
}

#[test]
fn c03_owned_corruption_quarantines_only_one_cache_and_preserves_sources_and_vectors() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("quarantine");
        let facts = KnowledgeFactStore::open(&root).unwrap();
        let (_, source_snapshot) = facts
            .create_source_with_snapshot(
                "studio",
                "pasted_text",
                "原文",
                &serde_json::json!({}),
                b"immutable source evidence",
                "text/plain",
            )
            .unwrap();
        drop(facts);
        open_cache(&root, "knowledge_index.db").unwrap();
        let vectors = KnowledgeVectorStore::open(&root).unwrap();
        let build = vectors
            .begin_variant_build(
                "civ-unique",
                "artifact-unique",
                "paid-model",
                "fingerprint",
                2,
            )
            .unwrap();
        vectors
            .upsert_batch(
                &build.variant_id,
                &[lingxi_adapters::storage::knowledge::VectorEntry {
                    chunk_id: "chunk".into(),
                    ordinal: 0,
                    vector: vec![0.2, 0.8],
                }],
            )
            .unwrap();
        vectors.complete_build(&build.variant_id, 1).unwrap();
        drop(vectors);
        let facts_before = std::fs::read(root.join("knowledge.db")).unwrap();
        let source_before = std::fs::read(root.join(&source_snapshot.storage_path)).unwrap();
        let other = if name == "knowledge_index.db" {
            "knowledge_vector.db"
        } else {
            "knowledge_index.db"
        };
        let other_before = std::fs::read(root.join(other)).unwrap();
        let corrupt = b"not a sqlite file, retained for diagnostics";
        std::fs::write(root.join(name), corrupt).unwrap();
        open_cache(&root, name).expect("确证自有静止损坏可恢复");
        let backups = quarantine_originals(&root);
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), corrupt);
        assert_eq!(
            std::fs::read(root.join("knowledge.db")).unwrap(),
            facts_before
        );
        assert_eq!(
            std::fs::read(root.join(&source_snapshot.storage_path)).unwrap(),
            source_before
        );
        assert_eq!(std::fs::read(root.join(other)).unwrap(), other_before);
        open_cache(&root, name).expect("重启后正常");
        assert_eq!(quarantine_originals(&root).len(), 1, "不能再次隔离正常新库");
    }
}

#[test]
fn c03_unknown_corruption_and_wal_never_auto_rebuild() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("unowned");
        let path = root.join(name);
        std::fs::write(&path, b"unknown object").unwrap();
        assert_eq!(
            open_cache(&root, name).unwrap_err().code,
            "KNOWLEDGE_STORAGE_CORRUPT"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"unknown object");
        assert!(quarantine_originals(&root).is_empty());
        let owned = fixture("wal-corrupt");
        open_cache(&owned, name).unwrap();
        let path = owned.join(name);
        std::fs::write(&path, b"confirmed old cache corruption").unwrap();
        std::fs::write(
            PathBuf::from(format!("{}-wal", path.display())),
            b"valuable WAL must not be discarded",
        )
        .unwrap();
        let before = snapshot(&path);
        assert_eq!(
            open_cache(&owned, name).unwrap_err().code,
            "KNOWLEDGE_STORAGE_BUSY"
        );
        assert_eq!(snapshot(&path), before);
        assert!(quarantine_originals(&owned).is_empty());
    }
}

#[test]
fn c04_active_production_store_blocks_corruption_quarantine() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("active");
        let index =
            (name == "knowledge_index.db").then(|| KnowledgeIndexStore::open(&root).unwrap());
        let vector =
            (name == "knowledge_vector.db").then(|| KnowledgeVectorStore::open(&root).unwrap());
        let path = root.join(name);
        std::fs::write(&path, b"external-corruption-while-active").unwrap();
        let before = snapshot(&path);
        let error = open_cache(&root, name).unwrap_err();
        assert_eq!(error.code, "KNOWLEDGE_STORAGE_BUSY");
        assert_eq!(snapshot(&path), before);
        assert!(quarantine_originals(&root).is_empty());
        drop(index);
        drop(vector);
    }
}

#[cfg(unix)]
#[test]
fn adversarial_symlink_and_hardlink_never_touch_external_target() {
    use std::os::unix::fs::symlink;
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        for link_kind in ["symlink", "hardlink"] {
            let root = fixture(link_kind);
            let outside = fixture("sentinel").join("outside.db");
            std::fs::write(&outside, b"external-sentinel").unwrap();
            if link_kind == "symlink" {
                symlink(&outside, root.join(name)).unwrap();
            } else {
                std::fs::hard_link(&outside, root.join(name)).unwrap();
            }
            let error = open_cache(&root, name).unwrap_err();
            assert_eq!(error.code, "KNOWLEDGE_STORAGE_UNSAFE");
            assert_eq!(std::fs::read(&outside).unwrap(), b"external-sentinel");
        }
    }
}

#[test]
fn reset_and_delete_artifact_keep_live_inode_and_other_source_index() {
    let root = fixture("reset");
    let index = KnowledgeIndexStore::open(&root).unwrap();
    let first = index
        .ensure_chunk_index_variant("artifact-a", "profile", "finger-a")
        .unwrap();
    let second = index
        .ensure_chunk_index_variant("artifact-b", "profile", "finger-b")
        .unwrap();
    let path = root.join("knowledge_index.db");
    let reader = Connection::open(&path).unwrap();
    let count = || {
        reader
            .query_row("SELECT count(*) FROM chunk_index_variants", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert_eq!(count(), 2);
    index.remove_artifact("artifact-a").unwrap();
    assert_eq!(count(), 1);
    assert!(index.get_variant(&second.id).unwrap().is_some());
    assert!(index.get_variant(&first.id).unwrap().is_none());
    index.reset().unwrap();
    assert!(path.exists(), "活跃数据库不能unlink");
    assert_eq!(count(), 0, "已有连接与新连接看到相同事务");
    assert!(KnowledgeIndexStore::open(&root).is_ok());
}

#[cfg(unix)]
#[test]
fn future_crash_child() {
    let Some(path) = std::env::var_os("F12_CRASH_DB") else {
        return;
    };
    let conn = Connection::open(std::path::Path::new(&path)).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('uncheckpointed-paid-vector'); PRAGMA user_version=999;").unwrap();
    // 模拟真实进程崩溃：不运行Connection的Drop，不使用孤儿进程。
    unsafe {
        libc::_exit(0);
    }
}

#[cfg(unix)]
#[test]
fn c01_crashed_future_wal_is_not_checkpointed_by_rejection() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("crashed-future");
        open_cache(&root, name).unwrap();
        let path = root.join(name);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "future_crash_child", "--nocapture"])
            .env("F12_CRASH_DB", &path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let before = snapshot(&path);
        assert_eq!(before.len(), 3);
        let error = open_cache(&root, name).unwrap_err();
        assert_eq!(error.code, "KNOWLEDGE_SCHEMA_NEWER");
        let after = snapshot(&path);
        assert_eq!(
            &after[..2],
            &before[..2],
            "拒绝本身不能checkpoint或删除未来主库/WAL"
        );
        assert_eq!(
            after[2].1.len(),
            before[2].1.len(),
            "SHM保持存在；SQLite读者可重建易失的WAL索引/读锁"
        );
    }
}

#[cfg(unix)]
#[test]
fn c03_rebuild_io_failure_keeps_quarantined_original() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("rebuild-failure");
        open_cache(&root, name).unwrap();
        let corrupt = b"owned corruption before recovery failure";
        std::fs::write(root.join(name), corrupt).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "c02_io_child", "--nocapture"])
            .env("F12_IO_ROOT", &root)
            .env("F12_IO_CACHE", name)
            .env("F12_IO_LIMIT", "512")
            .output()
            .unwrap();
        eprintln!(
            "rebuild failure child {} {} {}",
            output.status,
            quote_child(&output.stdout),
            quote_child(&output.stderr)
        );
        assert!(output.status.success());
        let original = quarantine_originals(&root);
        assert_eq!(original.len(), 1);
        assert_eq!(
            std::fs::read(&original[0]).unwrap(),
            corrupt,
            "重建失败不能丢唯一原件"
        );
    }
}

#[test]
fn positive_system_temp_alias_root_is_supported_without_dropping_nofollow() {
    let root = std::env::temp_dir().join(format!(
        "lingxi-f12-system-alias-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let facts = lingxi_adapters::storage::knowledge::KnowledgeFactStore::open(&root)
        .expect("系统临时目录别名正常首建");
    assert!(facts.list_notebooks("studio").unwrap().is_empty());
    assert!(KnowledgeIndexStore::open(&root).is_ok());
    assert!(KnowledgeVectorStore::open(&root).is_ok());
    drop(facts);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn c04_existing_old_handles_cannot_write_after_schema_upgrade() {
    let root = fixture("old-handles");
    let index = KnowledgeIndexStore::open(&root).unwrap();
    let vector = KnowledgeVectorStore::open(&root).unwrap();
    let variant = index
        .ensure_chunk_index_variant("status-artifact", "profile", "fingerprint")
        .unwrap();
    index.set_variant_status(&variant.id, "ready").unwrap();
    assert_eq!(
        index.get_variant(&variant.id).unwrap().unwrap().status,
        "ready"
    );
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let conn = Connection::open(root.join(name)).unwrap();
        conn.execute_batch("PRAGMA user_version=999").unwrap();
    }
    assert_eq!(
        index
            .ensure_chunk_index_variant("artifact", "profile", "fingerprint")
            .unwrap_err()
            .code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
    assert_eq!(
        index
            .set_variant_status(&variant.id, "failed")
            .unwrap_err()
            .code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
    assert_eq!(
        index.get_variant(&variant.id).unwrap().unwrap().status,
        "ready"
    );
    assert_eq!(
        index
            .replace_artifact_chunks("artifact", "profile", "fingerprint", &[], &[])
            .unwrap_err()
            .code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
    assert_eq!(index.reset().unwrap_err().code, "KNOWLEDGE_SCHEMA_NEWER");
    assert_eq!(
        index.remove_artifact("artifact").unwrap_err().code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
    let result = vector.begin_variant_build("civ", "artifact", "model", "fingerprint", 2);
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("旧版本活跃句柄不得写future库"),
    };
    assert_eq!(error.code, "KNOWLEDGE_SCHEMA_NEWER");
    assert_eq!(
        vector.upsert_batch("viv", &[]).unwrap_err().code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
    assert_eq!(
        vector.complete_build("viv", 1).unwrap_err().code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
    assert_eq!(
        vector.fail_build("viv").unwrap_err().code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
    assert_eq!(
        vector.remove_artifact("artifact").unwrap_err().code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
}

#[test]
fn guard_holder_child() {
    let Some(root) = std::env::var_os("F12_GUARD_ROOT") else {
        return;
    };
    let name = std::env::var("F12_GUARD_CACHE").unwrap();
    let root = std::path::Path::new(&root);
    let index = (name == "knowledge_index.db").then(|| KnowledgeIndexStore::open(root).unwrap());
    let vector = (name == "knowledge_vector.db").then(|| KnowledgeVectorStore::open(root).unwrap());
    use std::io::{Read, Write};
    println!("guard-ready");
    std::io::stdout().flush().unwrap();
    let mut release = [0u8; 1];
    std::io::stdin().read_exact(&mut release).unwrap();
    drop(index);
    drop(vector);
}

#[test]
fn c04_other_process_production_handle_prevents_quarantine() {
    use std::io::{BufRead, Write};
    use std::process::Stdio;
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("other-process");
        open_cache(&root, name).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "guard_holder_child", "--nocapture"])
            .env("F12_GUARD_ROOT", &root)
            .env("F12_GUARD_CACHE", name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
        loop {
            let mut line = String::new();
            assert!(
                reader.read_line(&mut line).unwrap() > 0,
                "真实子进程守卫就绪"
            );
            if line.contains("guard-ready") {
                break;
            }
        }
        let path = root.join(name);
        std::fs::write(&path, b"damaged-while-other-process-holds-store").unwrap();
        let before = snapshot(&path);
        let result = open_cache(&root, name);
        assert_eq!(snapshot(&path), before, "释放子进程守卫前原始文件必须不变");
        child.stdin.take().unwrap().write_all(b"x").unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(result.unwrap_err().code, "KNOWLEDGE_STORAGE_BUSY");
        // 活跃子进程退出可checkpoint，因此在释放守卫前只要求原库未隔离。
        assert!(!before.is_empty());
        assert!(quarantine_originals(&root).is_empty());
    }
}

#[test]
fn c04_old_handles_close_without_checkpointing_future_wal_after_upgrader_closes() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("close-future-wal");
        let path = root.join(name);
        let index =
            (name == "knowledge_index.db").then(|| KnowledgeIndexStore::open(&root).unwrap());
        let vector =
            (name == "knowledge_vector.db").then(|| KnowledgeVectorStore::open(&root).unwrap());
        let store = (index, vector);
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA wal_autocheckpoint=0; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('future-uncheckpointed-original'); PRAGMA user_version=999;").unwrap();
        drop(writer);
        let durable = |entries: Vec<(String, Vec<u8>)>| {
            entries
                .into_iter()
                .filter(|entry| entry.0 != "-shm")
                .collect::<Vec<_>>()
        };
        let before = durable(snapshot(&path));
        assert!(
            std::fs::metadata(path.with_file_name(format!("{name}-wal")))
                .unwrap()
                .len()
                > 32
        );
        drop(store);
        let after = durable(snapshot(&path));
        assert_eq!(before, after, "关闭旧句柄不能checkpoint未来主库或删除WAL");
        assert_eq!(
            open_cache(&root, name).unwrap_err().code,
            "KNOWLEDGE_SCHEMA_NEWER"
        );
        let refused = durable(snapshot(&path));
        assert_eq!(before, refused);
        let check =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            check
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            999
        );
        assert_eq!(
            check
                .query_row("SELECT value FROM sentinel", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "future-uncheckpointed-original"
        );
    }
}

#[test]
fn c04_supported_rollback_caches_switch_wal_and_release_initialization_locks() {
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let root = fixture("rollback-normal");
        let path = root.join(name);
        open_cache(&root, name).unwrap();
        let writer = Connection::open(&path).unwrap();
        writer
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        writer.execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('normal-original');").unwrap();
        let index =
            (name == "knowledge_index.db").then(|| KnowledgeIndexStore::open(&root).unwrap());
        let vector =
            (name == "knowledge_vector.db").then(|| KnowledgeVectorStore::open(&root).unwrap());
        let store = (index, vector);
        writer.busy_timeout(std::time::Duration::ZERO).unwrap();
        writer
            .execute("UPDATE sentinel SET value='normal-write-after-open'", [])
            .unwrap();
        let check = Connection::open(&path).unwrap();
        assert_eq!(
            check
                .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            check
                .query_row("SELECT value FROM sentinel", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "normal-write-after-open"
        );
        drop(check);
        drop(writer);
        drop(store);
        open_cache(&root, name).unwrap();
    }
}

#[test]
fn sqlite_initialization_lock_protocol_retains_lock_until_wal_then_releases_it() {
    // 这里只验证真实 SQLite 锁语义；生产入口的正常/失败行为由其它案例覆盖。
    let root = fixture("sqlite-lock-protocol");
    let path = root.join("protocol.db");
    let current = Connection::open(&path).unwrap();
    current.execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('normal-original'); PRAGMA user_version=1;").unwrap();
    let writer = Connection::open(&path).unwrap();
    writer.busy_timeout(std::time::Duration::ZERO).unwrap();
    current
        .pragma_update(None, "locking_mode", "EXCLUSIVE")
        .unwrap();
    current.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert_eq!(
        current
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    current.execute_batch("COMMIT").unwrap();
    current
        .pragma_update(None, "locking_mode", "NORMAL")
        .unwrap();
    let error = writer.pragma_update(None, "user_version", 999).unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    assert_eq!(
        current
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    writer
        .execute("UPDATE sentinel SET value='write-after-release'", [])
        .unwrap();
    assert_eq!(
        current
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
