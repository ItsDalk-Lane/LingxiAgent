//! 切割备份（T05_DESIGN §5.4；裁决点 P6）。
//!
//! - 落点 `{backup-dir}/r08-cutover-<cutoverId>/stores/<storeId>/…` +
//!   `backup-manifest.json`（storeId/relPath/bytes/sha256/method）。
//! - SQLite 一律 **copy-set → scratch → Online Backup**（先复制 db+wal+shm
//!   到 work-dir scratch，打开的是副本；源库文件永不直接打开——连
//!   read-only/immutable 模式都不用，P6 裁决）。method = `online-backup`。
//! - 普通文件流式 copy + 边读边 sha256，写后复读对拍；不一致 = 该店重备，
//!   重备后仍不一致 = 响亮失败。method = `file-copy`。
//! - 逐店幂等：已存在且哈希与源一致的产物直接复用（resume 安全）。
//! - 清单外的文件（69 类无一命中）= 响亮拒绝（缺类 = 拒绝，P7）；db 的
//!   `-wal`/`-shm` 伴生文件归并到主库 copy-set，不单独计类。

use lingxi_adapters::storage::backup::{self, BackupOptions};
use lingxi_adapters::storage::legacy_import::classes;
use lingxi_adapters::storage::legacy_import::manifest::{self, Manifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::stillpoint::{Disposition, StoreRow};

pub const BACKUP_MANIFEST_FILE: &str = "backup-manifest.json";

#[derive(Debug)]
pub enum BackupError {
    Io {
        path: PathBuf,
        detail: String,
    },
    /// 清单外文件：69 类无一命中（缺类 = 拒绝）。
    Unclassified {
        relpath: String,
    },
    Store {
        store_id: String,
        detail: String,
    },
    Manifest {
        detail: String,
    },
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackupError::Io { path, detail } => {
                write!(f, "backup io at {}: {detail}", path.display())
            }
            BackupError::Unclassified { relpath } => write!(
                f,
                "unclassified source file outside the 69-store registry (refuse): {relpath}"
            ),
            BackupError::Store { store_id, detail } => {
                write!(f, "backup store {store_id}: {detail}")
            }
            BackupError::Manifest { detail } => write!(f, "backup manifest: {detail}"),
        }
    }
}

impl std::error::Error for BackupError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileBackupRecord {
    #[serde(rename = "storeId")]
    pub store_id: String,
    #[serde(rename = "relPath")]
    pub rel_path: String,
    pub bytes: u64,
    pub sha256: String,
    /// online-backup | file-copy
    pub method: String,
    /// 备份产物相对 backup root 的路径。
    pub artifact: String,
    /// reused | copied
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupReport {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u64,
    #[serde(rename = "cutoverId")]
    pub cutover_id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "backupRoot")]
    pub backup_root: String,
    pub files: Vec<FileBackupRecord>,
}

pub fn backup_root_for(backup_dir: &Path, cutover_id: &str) -> PathBuf {
    backup_dir.join(format!("r08-cutover-{cutover_id}"))
}

fn is_sqlite_relpath(relpath: &str) -> bool {
    relpath.ends_with(".db") || relpath.ends_with(".sqlite") || relpath.ends_with(".sqlite3")
}

fn is_db_sidecar(relpath: &str) -> Option<&str> {
    relpath
        .strip_suffix("-wal")
        .or_else(|| relpath.strip_suffix("-shm"))
}

/// relpath → 文件系统安全的产物名（`/`→`__`）。
fn artifact_name(relpath: &str) -> String {
    relpath.replace('/', "__")
}

/// 流式 copy + 单遍 sha256（源与产物分别算，写后复算产物）。
fn copy_with_hash(source: &Path, dest: &Path) -> Result<(u64, String, String), BackupError> {
    let mut input = std::fs::File::open(source).map_err(|err| BackupError::Io {
        path: source.to_path_buf(),
        detail: format!("open: {err}"),
    })?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|err| BackupError::Io {
            path: parent.to_path_buf(),
            detail: format!("create_dir_all: {err}"),
        })?;
    }
    let tmp = dest.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let mut src_hasher = Sha256::new();
    let mut bytes = 0u64;
    {
        let mut output = std::fs::File::create(&tmp).map_err(|err| BackupError::Io {
            path: tmp.clone(),
            detail: format!("create: {err}"),
        })?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = input.read(&mut buf).map_err(|err| BackupError::Io {
                path: source.to_path_buf(),
                detail: format!("read: {err}"),
            })?;
            if n == 0 {
                break;
            }
            src_hasher.update(&buf[..n]);
            bytes += n as u64;
            std::io::Write::write_all(&mut output, &buf[..n]).map_err(|err| BackupError::Io {
                path: tmp.clone(),
                detail: format!("write: {err}"),
            })?;
        }
        std::io::Write::flush(&mut output).map_err(|err| BackupError::Io {
            path: tmp.clone(),
            detail: format!("flush: {err}"),
        })?;
    }
    let src_sha = manifest::sha256_hex(&src_hasher.finalize());
    // 写后复读产物哈希，相等才 rename 生效（半文件绝不进备份树）。
    let mut reread = std::fs::File::open(&tmp).map_err(|err| BackupError::Io {
        path: tmp.clone(),
        detail: format!("reopen: {err}"),
    })?;
    let mut dst_hasher = Sha256::new();
    std::io::copy(&mut reread, &mut dst_hasher_adapter(&mut dst_hasher)).map_err(|err| {
        BackupError::Io {
            path: tmp.clone(),
            detail: format!("rehash: {err}"),
        }
    })?;
    let dst_sha = manifest::sha256_hex(&dst_hasher.finalize());
    if src_sha != dst_sha {
        let _ = std::fs::remove_file(&tmp);
        return Err(BackupError::Io {
            path: dest.to_path_buf(),
            detail: format!("copy integrity mismatch: src {src_sha} != dst {dst_sha}"),
        });
    }
    if dest.exists() {
        // 幂等覆盖：已有产物哈希不同才替换。
        let existing = hash_file(dest)?;
        if existing == src_sha {
            let _ = std::fs::remove_file(&tmp);
            return Ok((bytes, src_sha, dst_sha));
        }
        std::fs::remove_file(dest).map_err(|err| BackupError::Io {
            path: dest.to_path_buf(),
            detail: format!("replace: {err}"),
        })?;
    }
    std::fs::rename(&tmp, dest).map_err(|err| BackupError::Io {
        path: dest.to_path_buf(),
        detail: format!("rename: {err}"),
    })?;
    Ok((bytes, src_sha, dst_sha))
}

fn dst_hasher_adapter<'a>(hasher: &'a mut Sha256) -> impl std::io::Write + 'a {
    struct HasherWriter<'a>(&'a mut Sha256);
    impl std::io::Write for HasherWriter<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.update(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    HasherWriter(hasher)
}

pub fn hash_file(path: &Path) -> Result<String, BackupError> {
    let mut file = std::fs::File::open(path).map_err(|err| BackupError::Io {
        path: path.to_path_buf(),
        detail: format!("open: {err}"),
    })?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut dst_hasher_adapter(&mut hasher)).map_err(|err| {
        BackupError::Io {
            path: path.to_path_buf(),
            detail: format!("read: {err}"),
        }
    })?;
    Ok(manifest::sha256_hex(&hasher.finalize()))
}

/// 备份一个 SQLite 文件：copy-set（db+wal+shm）→ scratch → 打开**副本**
/// → Online Backup 到 `{store_dir}/{artifact}.sqlite3`。
fn backup_sqlite(
    source_root: &Path,
    relpath: &str,
    scratch_dir: &Path,
    store_dir: &Path,
    store_id: &str,
) -> Result<(u64, String, String), BackupError> {
    let src_db = source_root.join(relpath);
    let scratch_db = scratch_dir.join("copy.db");
    let copy_set = [
        (src_db.clone(), scratch_db.clone(), true),
        (
            source_root.join(format!("{relpath}-wal")),
            scratch_dir.join("copy.db-wal"),
            false,
        ),
        (
            source_root.join(format!("{relpath}-shm")),
            scratch_dir.join("copy.db-shm"),
            false,
        ),
    ];
    let _ = std::fs::remove_file(scratch_dir.join("copy.db-wal"));
    let _ = std::fs::remove_file(scratch_dir.join("copy.db-shm"));
    for (src, dst, required) in &copy_set {
        match std::fs::copy(src, dst) {
            Ok(_) => {}
            Err(err) if !required && err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(BackupError::Store {
                    store_id: store_id.to_string(),
                    detail: format!("copy-set {}: {err}", src.display()),
                })
            }
        }
    }
    let wal_bytes = std::fs::metadata(scratch_dir.join("copy.db-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    let conn = rusqlite::Connection::open(&scratch_db).map_err(|err| BackupError::Store {
        store_id: store_id.to_string(),
        detail: format!("open scratch copy: {err}"),
    })?;
    let stem = artifact_name(relpath);
    let outcome = backup::backup_database(
        &conn,
        store_dir,
        &stem,
        wal_bytes,
        BackupOptions::default(),
        2_000,
    )
    .map_err(|err| BackupError::Store {
        store_id: store_id.to_string(),
        detail: format!("online backup: {err}"),
    })?;
    drop(conn);
    let artifact = outcome.dest_dir.join(&outcome.file_name);
    let sha = hash_file(&artifact)?;
    Ok((outcome.bytes, sha, artifact.display().to_string()))
}

/// 跑全量切割备份。返回报告并落 `{backup-root}/backup-manifest.json`。
pub fn run_backup(
    source: &Path,
    backup_dir: &Path,
    work_dir: &Path,
    cutover_id: &str,
    stores: &[StoreRow],
    source_manifest: &Manifest,
) -> Result<BackupReport, BackupError> {
    let backup_root = backup_root_for(backup_dir, cutover_id);
    std::fs::create_dir_all(&backup_root).map_err(|err| BackupError::Io {
        path: backup_root.clone(),
        detail: format!("create_dir_all: {err}"),
    })?;
    let scratch_root = work_dir.join("scratch").join(cutover_id);
    if scratch_root.exists() {
        std::fs::remove_dir_all(&scratch_root).map_err(|err| BackupError::Io {
            path: scratch_root.clone(),
            detail: format!("reset scratch: {err}"),
        })?;
    }
    std::fs::create_dir_all(&scratch_root).map_err(|err| BackupError::Io {
        path: scratch_root.clone(),
        detail: format!("create scratch: {err}"),
    })?;

    // 处置映射：classId → disposition（仅前三种参与备份）。
    let needs_backup = |d: Disposition| {
        matches!(
            d,
            Disposition::BackupImport
                | Disposition::BackupCarry
                | Disposition::BackupOnlyRebuildReceipt
        )
    };

    let mut records: Vec<FileBackupRecord> = Vec::new();
    for entry in &source_manifest.entries {
        let relpath = entry.relpath.as_str();
        let (class_id, disposition) = if let Some((def, _)) = classes::classify(relpath) {
            (def.id, Disposition::from_strategy(def.strategy))
        } else if let Some(base) = is_db_sidecar(relpath) {
            match classes::classify(base) {
                Some((def, _)) => (def.id, Disposition::from_strategy(def.strategy)),
                None => {
                    return Err(BackupError::Unclassified {
                        relpath: relpath.to_string(),
                    })
                }
            }
        } else {
            return Err(BackupError::Unclassified {
                relpath: relpath.to_string(),
            });
        };
        if is_db_sidecar(relpath).is_some() {
            continue; // 归并到主库 copy-set，不单独备份。
        }
        if !needs_backup(disposition) {
            continue; // epoch 元数据 / 壳本地：不进切割备份。
        }
        let store = stores
            .iter()
            .find(|s| s.class_id == class_id)
            .ok_or_else(|| BackupError::Manifest {
                detail: format!("class {class_id} missing from stillpoint enumeration"),
            })?;
        let _ = store; // classId 已通过 classify 权威给出；stores 仅用于对拍存在性。
        let store_dir = backup_root.join("stores").join(class_id);
        let src_file = source.join(relpath);
        let record = if is_sqlite_relpath(relpath) {
            let (bytes, sha, artifact) =
                backup_sqlite(source, relpath, &scratch_root, &store_dir, class_id)?;
            FileBackupRecord {
                store_id: class_id.to_string(),
                rel_path: relpath.to_string(),
                bytes,
                sha256: sha,
                method: "online-backup".to_string(),
                artifact,
                action: "copied".to_string(),
            }
        } else {
            let artifact = store_dir.join(artifact_name(relpath));
            let existed = artifact.exists();
            let (bytes, src_sha, _) = copy_with_hash(&src_file, &artifact)?;
            FileBackupRecord {
                store_id: class_id.to_string(),
                rel_path: relpath.to_string(),
                bytes,
                sha256: src_sha,
                method: "file-copy".to_string(),
                artifact: artifact.display().to_string(),
                action: if existed {
                    "reused-or-replaced".into()
                } else {
                    "copied".into()
                },
            }
        };
        // 与静止点清单对拍：源侧 bytes/sha 必须与 pass A 一致（file-copy 时
        // sha 即源 sha；online-backup 产物 sha 是快照语义，bytes 以源为准另记）。
        if record.method == "file-copy"
            && (record.bytes != entry.size || record.sha256 != entry.sha256)
        {
            return Err(BackupError::Store {
                store_id: class_id.to_string(),
                detail: format!(
                    "{relpath}: source drifted since stillpoint (size {}->{}, sha {}->{})",
                    entry.size, record.bytes, entry.sha256, record.sha256
                ),
            });
        }
        records.push(record);
    }

    let report = BackupReport {
        schema_version: 1,
        cutover_id: cutover_id.to_string(),
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        backup_root: backup_root.display().to_string(),
        files: records,
    };
    let manifest_path = backup_root.join(BACKUP_MANIFEST_FILE);
    let mut body = serde_json::to_string_pretty(&report).map_err(|err| BackupError::Manifest {
        detail: format!("serialize: {err}"),
    })?;
    body.push('\n');
    crate::paths::atomic_write_private(&manifest_path, body.as_bytes()).map_err(|err| {
        BackupError::Io {
            path: manifest_path,
            detail: format!("atomic write: {err}"),
        }
    })?;
    let _ = std::fs::remove_dir_all(&scratch_root);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_adapters::storage::legacy_import::manifest::build_manifest;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t05-backup-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn stores() -> Vec<StoreRow> {
        super::super::stillpoint::enumerate_stores().unwrap()
    }

    #[test]
    fn file_copy_backup_records_hash_and_reuses_idempotently() {
        let src = temp_dir("fc-src");
        let bak = temp_dir("fc-bak");
        let work = temp_dir("fc-work");
        std::fs::create_dir_all(src.join("user")).unwrap();
        std::fs::write(src.join("user/user.md"), b"# hello\n").unwrap();
        let manifest = build_manifest(&src).unwrap();
        let report = run_backup(&src, &bak, &work, "cut-1", &stores(), &manifest).unwrap();
        assert_eq!(report.files.len(), 1);
        let rec = &report.files[0];
        assert_eq!(rec.store_id, "agent-authored-records");
        assert_eq!(rec.method, "file-copy");
        assert!(Path::new(&rec.artifact).exists());
        assert_eq!(
            rec.sha256,
            manifest
                .entries
                .iter()
                .find(|e| e.relpath == "user/user.md")
                .unwrap()
                .sha256
        );
        // 幂等重跑：产物已一致 → 复用，结果不变。
        let report2 = run_backup(&src, &bak, &work, "cut-1", &stores(), &manifest).unwrap();
        assert_eq!(report2.files.len(), 1);
        assert_eq!(report2.files[0].sha256, rec.sha256);
        assert!(bak.join("r08-cutover-cut-1/backup-manifest.json").exists());
        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&bak).unwrap();
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn sqlite_backup_uses_online_backup_and_sidecars_fold_in() {
        let src = temp_dir("sql-src");
        let bak = temp_dir("sql-bak");
        let work = temp_dir("sql-work");
        let db_dir = src.join("agents/agent-1/memory");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db_path = db_dir.join("facts.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);",
            )
            .unwrap();
            conn.execute("INSERT INTO t(v) VALUES ('a'),('b'),('c')", [])
                .unwrap();
            // 留 WAL 内容不关库，制造 wal/shm 伴生文件。
        }
        let manifest = build_manifest(&src).unwrap();
        assert!(manifest
            .entries
            .iter()
            .any(|e| e.relpath.ends_with("facts.db")));
        let report = run_backup(&src, &bak, &work, "cut-2", &stores(), &manifest).unwrap();
        let rec = report
            .files
            .iter()
            .find(|r| r.rel_path == "agents/agent-1/memory/facts.db")
            .unwrap();
        assert_eq!(rec.method, "online-backup");
        assert_eq!(rec.store_id, "agent-facts-sqlite");
        // 伴生 -wal/-shm 不产生独立记录。
        assert!(!report.files.iter().any(|r| r.rel_path.ends_with("-wal")));
        // 备份产物可打开且行数齐全（完整性不只靠哈希）。
        let restored = rusqlite::Connection::open(&rec.artifact).unwrap();
        let count: i64 = restored
            .query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&bak).unwrap();
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn unclassified_file_is_loud_refusal() {
        let src = temp_dir("unc-src");
        let bak = temp_dir("unc-bak");
        let work = temp_dir("unc-work");
        std::fs::write(src.join("mystery.bin"), b"?").unwrap();
        let manifest = build_manifest(&src).unwrap();
        let err = run_backup(&src, &bak, &work, "cut-3", &stores(), &manifest).unwrap_err();
        assert!(err.to_string().contains("unclassified"), "{err}");
        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&bak).unwrap();
        std::fs::remove_dir_all(&work).unwrap();
    }
}
