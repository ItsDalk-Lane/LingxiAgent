//! SQLite Online Backup API backups and verified restores (R02-T06 step 1,
//! acceptance R02-A11).
//!
//! Mechanism contract (frozen by ADR-004 D3 + ROLLBACK_DESIGN §1.4):
//! - Every backup goes through the **SQLite Online Backup API**
//!   (`sqlite3_backup_*` via `rusqlite::backup`), NEVER a plain file copy of
//!   the active `.db` main file (01 §8: WAL-resident committed data would be
//!   missed).
//! - The default flow is the documented quiescence point: writers are
//!   already quiesced (the backup job runs on the single-writer worker, so
//!   no other job executes concurrently), then
//!   `wal_checkpoint(TRUNCATE)` (W07), then the Online Backup API.
//!   `BackupOptions::pre_checkpoint=false` selects the pure online-API mode
//!   (the backup API reads committed WAL content directly) — both are real
//!   Online-Backup-API backups; the checkpoint first is belt and braces.
//! - A backup is only ever published under its FINAL name after the copied
//!   file passed `PRAGMA integrity_check`. While being built it lives under
//!   a `.partial-*` name that is REMOVED on any failure: an interrupted
//!   backup can never leave a half-written file that a later restore might
//!   mistake for a good snapshot (A11 fault-injection requirement).
//! - The backup directory carries a `manifest.json` (schema-versioned)
//!   with per-file sha256; the restore verifies the hash BEFORE promoting
//!   the bytes and refuses to overwrite an existing target.
//! - Restore = copy + hash-verify against the manifest + open through the
//!   normal recovery path (`RunDatabase::open`: receipts + integrity), so
//!   a restored database must satisfy exactly the same startup gate as a
//!   live one.
//! - Backup artifacts carry the same owner-only depth as the live store
//!   (T04 REVIEW F04 follow-up, R02-T06 repair R1 F02): a backup is an
//!   external copy of the full run facts, so the destination directory is
//!   tightened to 0700 and every artifact (the staged `.partial` db, the
//!   final db copy, the manifest and its temp file) to 0600 on unix —
//!   both on create and for pre-existing destinations. A tightening
//!   failure is loud: the backup/restore aborts before the artifact is
//!   trusted. On platforms without unix permission bits this is a
//!   documented no-op (protection rests on the host's directory
//!   conventions).

use std::path::{Path, PathBuf};

use rusqlite::backup::Backup;
use sha2::{Digest, Sha256};

use lingxi_kernel::ports::StorageError;

use super::migrations::map_rusqlite;
use super::queue::checkpoint_truncate;
use super::run_store::RUNS_DB_FILE_NAME;

/// Manifest schema version (bump on incompatible manifest changes).
pub const MANIFEST_SCHEMA_VERSION: u64 = 1;

/// Knobs of one backup run. Defaults are the production values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupOptions {
    /// `true` (default): run `wal_checkpoint(TRUNCATE)` on the source
    /// before the Online Backup API copy (the ADR-004 D3 / W07 quiescence
    /// point). `false`: pure Online-Backup-API copy over a non-empty WAL —
    /// the API itself reads committed WAL content, used to prove the API
    /// captures WAL-resident data.
    pub pre_checkpoint: bool,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            pre_checkpoint: true,
        }
    }
}

/// One file entry of the backup manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifestFile {
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
}

/// The manifest written next to every backup (schema-versioned).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub schema_version: u64,
    pub file_stem: String,
    pub created_at_unix_ms: i64,
    pub pre_checkpoint: bool,
    pub wal_bytes_before: u64,
    pub integrity: String,
    pub files: Vec<BackupManifestFile>,
}

/// Successful outcome of [`backup_database`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupOutcome {
    pub dest_dir: PathBuf,
    pub file_name: String,
    pub manifest_name: String,
    pub sha256: String,
    pub bytes: u64,
    pub wal_bytes_before: u64,
    pub pre_checkpoint: bool,
}

/// Successful outcome of [`restore_backup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub restored_path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn sha256_file(path: &Path) -> Result<(String, u64), StorageError> {
    let bytes = std::fs::read(path).map_err(|source| StorageError::Io {
        detail: format!("cannot read {} for hashing: {source}", path.display()),
    })?;
    let len = bytes.len() as u64;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    Ok((hex, len))
}

/// Best-effort removal of a partial artifact. Never masks the original
/// error: removal failures are logged loudly.
fn remove_partial_quietly(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::error!(
                file = %path.display(),
                %err,
                "cannot remove the partial backup artifact; a stale .partial file \
                 may remain — it is NOT a valid backup (only manifest-backed \
                 final names are restorable)"
            );
        }
    }
}

fn fsync_file(path: &Path) -> Result<(), StorageError> {
    let file = std::fs::File::open(path).map_err(|source| StorageError::Io {
        detail: format!("cannot reopen {} for fsync: {source}", path.display()),
    })?;
    file.sync_all().map_err(|source| StorageError::Io {
        detail: format!("cannot fsync {}: {source}", path.display()),
    })
}

/// Tightens one created backup/restore artifact to owner-only (directory
/// 0700, file 0600) — the same depth as the live store
/// (`queue::tighten_db_file_permissions`, T04 REVIEW F04): the backup is a
/// full external copy of the run facts, so a future directory-permission
/// regression must not expose it either. Idempotent (already-tight paths
/// are left untouched) and loud on failure. Non-unix platforms have no
/// permission bits to tighten (documented no-op, see the module docs).
fn tighten_owner_only(path: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).map_err(|source| StorageError::Io {
            detail: format!(
                "cannot stat {} for backup permission tightening: {source}",
                path.display()
            ),
        })?;
        let target: u32 = if meta.is_dir() { 0o700 } else { 0o600 };
        let mut perms = meta.permissions();
        if perms.mode() & 0o777 != target {
            perms.set_mode(target);
            std::fs::set_permissions(path, perms).map_err(|source| StorageError::Io {
                detail: format!(
                    "cannot tighten backup artifact {} to {:o}: {source}",
                    path.display(),
                    target
                ),
            })?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Validates the file stem: non-empty, no separators (the backup file names
/// are derived from it and must never escape `dest_dir`).
fn validate_stem(file_stem: &str) -> Result<(), StorageError> {
    if file_stem.is_empty()
        || file_stem.contains('/')
        || file_stem.contains('\\')
        || file_stem.contains("..")
    {
        return Err(StorageError::InvalidRequest {
            detail: format!(
                "invalid backup file stem {file_stem:?}: must be non-empty \
                 without path separators"
            ),
        });
    }
    Ok(())
}

/// Runs one Online-Backup-API snapshot of `source` into
/// `{dest_dir}/{file_stem}.db` + `{dest_dir}/{file_stem}.manifest.json`.
///
/// Runs on the single-writer worker (callers reach it through
/// [`super::run_store::RunDatabase::backup_to`]), so no other DB job can
/// interleave: the snapshot is taken with writers quiesced AND through the
/// backup API. `wal_bytes_before` is informational (recorded in the
/// manifest); the caller stats it before submitting the job.
pub fn backup_database(
    source: &rusqlite::Connection,
    dest_dir: &Path,
    file_stem: &str,
    wal_bytes_before: u64,
    options: BackupOptions,
) -> Result<BackupOutcome, StorageError> {
    validate_stem(file_stem)?;
    std::fs::create_dir_all(dest_dir).map_err(|source| StorageError::Io {
        detail: format!(
            "cannot create backup destination {}: {source}",
            dest_dir.display()
        ),
    })?;
    // F02 repair: the destination directory (created or pre-existing) is
    // owner-only BEFORE any artifact may land in it. A failure aborts the
    // backup before anything is published.
    tighten_owner_only(dest_dir)?;

    if options.pre_checkpoint {
        // The frozen quiescence point: push the WAL into the main file and
        // truncate it. A failed checkpoint aborts the backup BEFORE any
        // destination artifact exists (fail loud, nothing published).
        checkpoint_truncate(source)?;
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let partial_path = dest_dir.join(format!(
        "{file_stem}.db.partial-{}-{nanos:x}",
        std::process::id()
    ));
    let final_path = dest_dir.join(format!("{file_stem}.db"));
    let manifest_path = dest_dir.join(format!("{file_stem}.manifest.json"));

    let build = (|| -> Result<(), StorageError> {
        let mut dest = rusqlite::Connection::open(&partial_path).map_err(map_rusqlite)?;
        // Owner-only from the first byte on disk: the staged file keeps its
        // mode through the atomic rename, so the published backup copy is
        // 0600 (F02 repair).
        tighten_owner_only(&partial_path)?;
        {
            let backup = Backup::new(source, &mut dest).map_err(map_rusqlite)?;
            // Pages-per-step 64 keeps each step small; the source is
            // quiesced, so there is no restart pressure. `run_to_completion`
            // surfaces every step error (no swallow).
            backup
                .run_to_completion(64, std::time::Duration::from_millis(0), None)
                .map_err(map_rusqlite)?;
        }
        // The copied snapshot must be a fully valid database BEFORE it may
        // take the final name.
        let verdict: String = dest
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(map_rusqlite)?;
        if verdict != "ok" {
            return Err(StorageError::Corrupted {
                detail: format!("backup integrity_check reported: {verdict}"),
            });
        }
        drop(dest);
        fsync_file(&partial_path)?;
        let (sha256, bytes) = sha256_file(&partial_path)?;
        // Promote: rename is atomic within the destination directory.
        std::fs::rename(&partial_path, &final_path).map_err(|source| StorageError::Io {
            detail: format!(
                "cannot promote backup to {}: {source}",
                final_path.display()
            ),
        })?;
        let manifest = BackupManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            file_stem: file_stem.to_string(),
            created_at_unix_ms: now_unix_ms(),
            pre_checkpoint: options.pre_checkpoint,
            wal_bytes_before,
            integrity: "ok".to_string(),
            files: vec![BackupManifestFile {
                name: final_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                sha256,
                bytes,
            }],
        };
        write_manifest_atomically(&manifest_path, &manifest)?;
        fsync_file(&manifest_path)?;
        Ok(())
    })();

    if let Err(err) = build {
        // An interrupted/failed backup must not leave a half-written file
        // behind that could masquerade as a good snapshot. The final name
        // is only taken by the atomic rename AFTER integrity passed, so at
        // most a `.partial-*` can remain — remove it (loudly on failure).
        remove_partial_quietly(&partial_path);
        return Err(err);
    }

    let (sha256, bytes) = sha256_file(&final_path)?;
    Ok(BackupOutcome {
        dest_dir: dest_dir.to_path_buf(),
        file_name: final_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        manifest_name: manifest_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        sha256,
        bytes,
        wal_bytes_before,
        pre_checkpoint: options.pre_checkpoint,
    })
}

fn write_manifest_atomically(path: &Path, manifest: &BackupManifest) -> Result<(), StorageError> {
    let json = serde_json::to_string_pretty(manifest).map_err(|err| StorageError::Internal {
        detail: format!("cannot serialize backup manifest: {err}"),
    })?;
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    {
        use std::io::Write as _;
        let mut file = std::fs::File::create(&tmp).map_err(|source| StorageError::Io {
            detail: format!(
                "cannot create manifest temp file {}: {source}",
                tmp.display()
            ),
        })?;
        // Owner-only before any manifest bytes are written (F02 repair);
        // the atomic rename preserves the mode.
        tighten_owner_only(&tmp)?;
        file.write_all(json.as_bytes())
            .map_err(|source| StorageError::Io {
                detail: format!("cannot write manifest: {source}"),
            })?;
        file.sync_all().map_err(|source| StorageError::Io {
            detail: format!("cannot fsync manifest: {source}"),
        })?;
    }
    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        StorageError::Io {
            detail: format!("cannot promote manifest {}: {source}", path.display()),
        }
    })
}

/// Restores `{backup_dir}/{file_stem}.db` into `target_dir/runs.db` after
/// verifying the file's sha256 against the backup manifest. Refuses to
/// overwrite an existing target. The restored copy still has to pass the
/// normal open path (receipts + integrity) before any consumer may trust
/// it — callers open it through [`super::run_store::RunDatabase::open`].
pub fn restore_backup(
    backup_dir: &Path,
    file_stem: &str,
    target_dir: &Path,
) -> Result<RestoreOutcome, StorageError> {
    validate_stem(file_stem)?;
    let manifest_path = backup_dir.join(format!("{file_stem}.manifest.json"));
    let source_path = backup_dir.join(format!("{file_stem}.db"));
    let manifest_raw =
        std::fs::read_to_string(&manifest_path).map_err(|source| StorageError::Io {
            detail: format!(
                "cannot read backup manifest {}: {source}",
                manifest_path.display()
            ),
        })?;
    let manifest: BackupManifest =
        serde_json::from_str(&manifest_raw).map_err(|err| StorageError::Corrupted {
            detail: format!(
                "backup manifest {} is not valid JSON: {err}",
                manifest_path.display()
            ),
        })?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(StorageError::Corrupted {
            detail: format!(
                "backup manifest schemaVersion {} is not supported (this build \
                 understands {MANIFEST_SCHEMA_VERSION})",
                manifest.schema_version
            ),
        });
    }
    if manifest.integrity.as_str() != "ok" {
        return Err(StorageError::Corrupted {
            detail: format!(
                "backup manifest records integrity={:?}; refusing to restore a \
                 backup that did not pass integrity_check",
                manifest.integrity
            ),
        });
    }
    let (source_sha, source_bytes) = sha256_file(&source_path)?;
    let source_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let entry = manifest
        .files
        .iter()
        .find(|f| f.name == source_name)
        .ok_or_else(|| StorageError::Corrupted {
            detail: format!("backup manifest does not list {}", source_path.display()),
        })?;
    if entry.sha256 != source_sha || entry.bytes != source_bytes {
        return Err(StorageError::Corrupted {
            detail: format!(
                "backup file hash mismatch: manifest {}:{} ({} bytes) vs file \
                 {}:{} ({} bytes); the backup is incomplete or was tampered with",
                entry.name,
                entry.sha256,
                entry.bytes,
                source_path.display(),
                source_sha,
                source_bytes
            ),
        });
    }

    std::fs::create_dir_all(target_dir).map_err(|source| StorageError::Io {
        detail: format!(
            "cannot create restore target {}: {source}",
            target_dir.display()
        ),
    })?;
    // F02 repair: the restore target directory (created or pre-existing) is
    // owner-only before the copy lands; the restored db file is tightened
    // immediately after the copy, before it is fsynced or trusted.
    tighten_owner_only(target_dir)?;
    let target_path = target_dir.join(RUNS_DB_FILE_NAME);
    if target_path.exists() {
        return Err(StorageError::InvalidRequest {
            detail: format!(
                "refusing to overwrite existing {} (restore must target an \
                 empty directory)",
                target_path.display()
            ),
        });
    }
    // Copy + fsync, then re-verify the copy's hash before declaring success.
    std::fs::copy(&source_path, &target_path).map_err(|source| {
        let _ = std::fs::remove_file(&target_path);
        StorageError::Io {
            detail: format!(
                "cannot copy backup into {}: {source}",
                target_path.display()
            ),
        }
    })?;
    if let Err(err) = tighten_owner_only(&target_path) {
        let _ = std::fs::remove_file(&target_path);
        return Err(err);
    }
    fsync_file(&target_path)?;
    let (copy_sha, copy_bytes) = sha256_file(&target_path)?;
    if copy_sha != source_sha || copy_bytes != source_bytes {
        let _ = std::fs::remove_file(&target_path);
        return Err(StorageError::Corrupted {
            detail: format!(
                "restored copy hash mismatch ({copy_sha} vs {source_sha}); the \
                 partial copy was removed"
            ),
        });
    }
    Ok(RestoreOutcome {
        restored_path: target_path,
        sha256: copy_sha,
        bytes: copy_bytes,
    })
}
