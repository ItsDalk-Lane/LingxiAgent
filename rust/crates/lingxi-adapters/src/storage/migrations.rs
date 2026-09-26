//! Versioned migrations of the new run/message database (R02-T04 step 1).
//!
//! Framework contract:
//! - Migrations are an ordered, compile-time-fixed list; each carries a
//!   `version`, a `name` and its exact SQL text. The **fingerprint** is
//!   `sha256(sql)` and is written into an on-disk receipt
//!   (`schema_migrations`) inside the SAME transaction that applies the SQL.
//! - Opening the database verifies receipts against the compiled-in set:
//!   a gap, an unknown version, a renamed migration or a fingerprint
//!   mismatch is a loud [`StorageError::SchemaTampered`] — never a silent
//!   replay (acceptance R02-A08 negative case).
//! - The applied version is recorded BOTH in the receipts table and in
//!   `PRAGMA user_version`; a disagreement between the two is tampering.
//! - A database whose version is NEWER than this build is rejected with
//!   [`StorageError::DatabaseTooNew`] — no downgrade, no replay.
//! - Migration SQL deliberately avoids `IF NOT EXISTS`: an accidental
//!   replay against an already-migrated database must FAIL loudly, not
//!   silently succeed as a no-op.

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use lingxi_kernel::ports::StorageError;

/// One compiled-in migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub version: u64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// sha256 hex fingerprint of a migration's SQL text.
pub fn fingerprint_sql(sql: &str) -> String {
    let digest = Sha256::digest(sql.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Initial schema of the run/message database (version 1).
///
/// Ownership mapping (OWNERSHIP_TARGET.json; one owner per table, the
/// physical writer is always the single rust-service process):
/// - `sessions`, `runs`, `run_attempts`, `key_events`, `messages`:
///   run/session authority facts (`kernel.run-supervisor` /
///   `kernel.session` own the semantics; rows are only written through the
///   [`crate::storage::RunDatabase`] port implementation).
/// - `invocations`: tool-invocation receipts (`kernel.tool-gateway`
///   semantics; populated from R04).
/// - `schema_migrations`: migration receipts owned by the storage adapter.
///
/// `key_events.seq` is assigned per `stream_id` by the single writer inside
/// the committing transaction (UNIQUE constraint is the mechanical backstop).
pub const V1_NAME: &str = "initial_run_message_schema";
pub const V1_SQL: &str = r#"
CREATE TABLE sessions (
    session_id         TEXT PRIMARY KEY,
    agent_id           TEXT NOT NULL,
    owner_user_id      TEXT NOT NULL,
    title              TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE runs (
    run_id             TEXT PRIMARY KEY,
    session_id         TEXT NOT NULL REFERENCES sessions(session_id),
    owner_kind         TEXT NOT NULL,
    owner_subject      TEXT NOT NULL,
    principal_id       TEXT NOT NULL,
    attempt_count      INTEGER NOT NULL DEFAULT 0,
    status             TEXT NOT NULL,
    terminal_reason    TEXT,
    generation         INTEGER NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL,
    last_event_seq     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_runs_session ON runs(session_id);

CREATE TABLE run_attempts (
    run_id             TEXT NOT NULL REFERENCES runs(run_id),
    attempt            TEXT NOT NULL,
    generation         INTEGER NOT NULL,
    started_at_unix_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, attempt)
);

CREATE TABLE invocations (
    invocation_id      TEXT PRIMARY KEY,
    run_id             TEXT NOT NULL REFERENCES runs(run_id),
    attempt            TEXT NOT NULL,
    target             TEXT NOT NULL,
    args_digest        TEXT NOT NULL,
    status             TEXT NOT NULL,
    started_at_unix_ms INTEGER NOT NULL,
    completed_at_unix_ms INTEGER
);

CREATE TABLE messages (
    message_id         TEXT PRIMARY KEY,
    session_id         TEXT NOT NULL REFERENCES sessions(session_id),
    run_id             TEXT NOT NULL REFERENCES runs(run_id),
    role               TEXT NOT NULL,
    content_json       TEXT NOT NULL,
    model_call_id      TEXT,
    committed_at_unix_ms INTEGER NOT NULL,
    seq                INTEGER NOT NULL
);
CREATE INDEX idx_messages_session ON messages(session_id, seq);

CREATE TABLE key_events (
    event_id           TEXT PRIMARY KEY,
    stream_id          TEXT NOT NULL,
    seq                INTEGER NOT NULL,
    session_id         TEXT NOT NULL,
    run_id             TEXT,
    attempt            TEXT,
    event_type         TEXT NOT NULL,
    payload_json       TEXT NOT NULL,
    committed_at_unix_ms INTEGER NOT NULL,
    UNIQUE (stream_id, seq)
);
CREATE INDEX idx_key_events_run ON key_events(run_id);
CREATE INDEX idx_key_events_session ON key_events(session_id, seq);
"#;

/// The full ordered migration list. Appending a migration is a deliberate,
/// reviewed act; editing an existing entry changes its fingerprint and is
/// rejected on every already-migrated database.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: V1_NAME,
    sql: V1_SQL,
}];

/// Highest schema version this build understands.
pub fn supported_version() -> u64 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

/// Result of an open-time migration pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// Versions applied by THIS pass (empty when already up to date).
    pub applied: Vec<u64>,
    /// Schema version the database is at now.
    pub current_version: u64,
    /// Highest version this build supports.
    pub supported_version: u64,
}

fn user_version(conn: &Connection) -> Result<u64, StorageError> {
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(map_rusqlite)?;
    u64::try_from(v).map_err(|_| StorageError::Corrupted {
        detail: format!("negative PRAGMA user_version {v}"),
    })
}

fn set_user_version(conn: &Connection, version: u64) -> Result<(), StorageError> {
    conn.pragma_update(None, "user_version", version as i64)
        .map_err(map_rusqlite)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, StorageError> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get(0),
        )
        .map_err(map_rusqlite)?;
    Ok(n > 0)
}

/// Bootstraps the receipts table itself (the one piece of DDL that cannot
/// live inside a receipted transaction). Loudly refuses when the database
/// is not empty but has no receipts table: that is a tampered or foreign
/// database, not a fresh one.
fn ensure_receipts_table(conn: &Connection) -> Result<(), StorageError> {
    if table_exists(conn, "schema_migrations")? {
        return Ok(());
    }
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' \
             AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(map_rusqlite)?;
    if n > 0 {
        return Err(StorageError::SchemaTampered {
            detail: format!(
                "database has {n} non-system table(s) but no schema_migrations \
                 receipts; refusing to adopt an unregistered schema"
            ),
        });
    }
    conn.execute(
        "CREATE TABLE schema_migrations (
            version            INTEGER PRIMARY KEY,
            name               TEXT NOT NULL,
            fingerprint        TEXT NOT NULL,
            applied_at_unix_ms INTEGER NOT NULL,
            applied_by         TEXT NOT NULL
        )",
        [],
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

/// On-disk receipt row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub version: u64,
    pub name: String,
    pub fingerprint: String,
    pub applied_at_unix_ms: i64,
    pub applied_by: String,
}

fn read_receipts(conn: &Connection) -> Result<Vec<Receipt>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT version, name, fingerprint, applied_at_unix_ms, applied_by \
             FROM schema_migrations ORDER BY version",
        )
        .map_err(map_rusqlite)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Receipt {
                version: row.get::<_, i64>(0)? as u64,
                name: row.get(1)?,
                fingerprint: row.get(2)?,
                applied_at_unix_ms: row.get(3)?,
                applied_by: row.get(4)?,
            })
        })
        .map_err(map_rusqlite)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_rusqlite)?);
    }
    Ok(out)
}

/// Verifies the receipts against the compiled-in migration list: contiguous
/// from 1, known versions/names, matching fingerprints, and in agreement
/// with `PRAGMA user_version`.
pub fn verify_receipts(conn: &Connection) -> Result<Vec<Receipt>, StorageError> {
    let receipts = read_receipts(conn)?;
    let pragma = user_version(conn)?;
    let max_receipt = receipts.last().map(|r| r.version).unwrap_or(0);
    if pragma != max_receipt {
        return Err(StorageError::SchemaTampered {
            detail: format!(
                "PRAGMA user_version ({pragma}) disagrees with the newest \
                 migration receipt ({max_receipt}); version bookkeeping was \
                 tampered with"
            ),
        });
    }
    // An EMPTY receipts table over a database that already holds non-system
    // tables is a wiped bookkeeping trail, not a fresh database: replaying
    // migrations over existing tables is forbidden (loud, classified).
    if receipts.is_empty() {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' \
                 AND name NOT LIKE 'sqlite_%' AND name != 'schema_migrations'",
                [],
                |row| row.get(0),
            )
            .map_err(map_rusqlite)?;
        if n > 0 {
            return Err(StorageError::SchemaTampered {
                detail: format!(
                    "schema_migrations is empty but the database holds {n} \
                     non-system table(s); receipts were wiped - refusing to \
                     replay migrations over existing tables"
                ),
            });
        }
        return Ok(receipts);
    }
    for (index, receipt) in receipts.iter().enumerate() {
        let expected_version = (index as u64) + 1;
        if receipt.version != expected_version {
            return Err(StorageError::SchemaTampered {
                detail: format!(
                    "migration receipts must be contiguous from 1; position \
                     {index} holds version {}",
                    receipt.version
                ),
            });
        }
        let Some(expected) = MIGRATIONS.iter().find(|m| m.version == receipt.version) else {
            return Err(StorageError::DatabaseTooNew {
                found_version: receipt.version,
                supported_version: supported_version(),
            });
        };
        if receipt.name != expected.name {
            return Err(StorageError::SchemaTampered {
                detail: format!(
                    "migration {expected_version} recorded as {:?} but this \
                     build expects {:?}",
                    receipt.name, expected.name
                ),
            });
        }
        let expected_fp = fingerprint_sql(expected.sql);
        if receipt.fingerprint != expected_fp {
            return Err(StorageError::SchemaTampered {
                detail: format!(
                    "migration {expected_version} ({}) fingerprint mismatch: \
                     on-disk {}, compiled-in {}; the SQL text drifted or was \
                     tampered with",
                    receipt.name, receipt.fingerprint, expected_fp
                ),
            });
        }
    }
    Ok(receipts)
}

/// Runs the open-time migration pass: verify receipts, reject newer
/// databases, apply pending migrations one transaction each.
pub fn apply_all(
    conn: &Connection,
    applied_by: &str,
    now_unix_ms: i64,
) -> Result<MigrationOutcome, StorageError> {
    ensure_receipts_table(conn)?;
    let receipts = verify_receipts(conn)?;
    let current = receipts.last().map(|r| r.version).unwrap_or(0);
    let supported = supported_version();
    if current > supported {
        return Err(StorageError::DatabaseTooNew {
            found_version: current,
            supported_version: supported,
        });
    }
    let mut applied = Vec::new();
    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        let fp = fingerprint_sql(migration.sql);
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_rusqlite)?;
        let txn_result = (|| -> Result<(), StorageError> {
            conn.execute_batch(migration.sql).map_err(map_rusqlite)?;
            conn.execute(
                "INSERT INTO schema_migrations \
                 (version, name, fingerprint, applied_at_unix_ms, applied_by) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    migration.version as i64,
                    migration.name,
                    fp,
                    now_unix_ms,
                    applied_by
                ],
            )
            .map_err(map_rusqlite)?;
            set_user_version(conn, migration.version)?;
            Ok(())
        })();
        match txn_result {
            Ok(()) => {
                conn.execute_batch("COMMIT").map_err(map_rusqlite)?;
                applied.push(migration.version);
            }
            Err(err) => {
                // Roll back loudly; a rollback failure is itself surfaced
                // (never masked by the original error).
                if let Err(rollback_err) = conn.execute_batch("ROLLBACK") {
                    return Err(StorageError::Internal {
                        detail: format!(
                            "migration {} failed ({err}) AND rollback failed \
                             ({rollback_err})",
                            migration.version
                        ),
                    });
                }
                return Err(err);
            }
        }
    }
    Ok(MigrationOutcome {
        applied,
        current_version: supported_version(),
        supported_version: supported,
    })
}

/// Maps a rusqlite error onto the kernel storage error vocabulary, keying
/// on SQLite's numeric primary codes (the stable wire between SQLite
/// versions and rusqlite enum surface):
/// 5/6 busy+locked, 13 disk full, 8/14 readonly/cant-open (real IO faults),
/// 10 io_err, 11 corrupt, 18/too-many-files... anything else internal.
pub fn map_rusqlite(err: rusqlite::Error) -> StorageError {
    if let rusqlite::Error::SqliteFailure(ffi_err, message) = &err {
        let primary = ffi_err.extended_code & 0xff;
        let detail = message.clone().unwrap_or_else(|| err.to_string());
        return match primary {
            5 | 6 => StorageError::Busy { timeout_ms: 0 },
            13 => StorageError::DiskFull { detail },
            8 | 14 => StorageError::Io { detail },
            10 => StorageError::Io { detail },
            11 => StorageError::Corrupted { detail },
            _ => StorageError::Internal { detail },
        };
    }
    StorageError::Internal {
        detail: err.to_string(),
    }
}

/// Maps busy with the configured timeout for accurate reporting.
pub fn map_rusqlite_busy(err: rusqlite::Error, timeout_ms: u64) -> StorageError {
    match map_rusqlite(err) {
        StorageError::Busy { .. } => StorageError::Busy { timeout_ms },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> Connection {
        Connection::open_in_memory().expect("open in-memory db")
    }

    fn hex_len_ok(fp: &str) -> bool {
        fp.len() == 64 && fp.chars().all(|c| c.is_ascii_hexdigit())
    }

    #[test]
    fn fingerprints_are_stable_sha256_hex() {
        let fp = fingerprint_sql(V1_SQL);
        assert!(hex_len_ok(&fp), "fingerprint shape: {fp}");
        // Deterministic: same SQL, same fingerprint.
        assert_eq!(fp, fingerprint_sql(V1_SQL));
        // Sensitive: any edit changes it.
        let mut edited = V1_SQL.to_string();
        edited.push('\n');
        assert_ne!(fp, fingerprint_sql(&edited));
    }

    #[test]
    fn migrations_are_contiguous_from_one() {
        for (index, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version, index as u64 + 1);
            assert!(!m.name.is_empty());
            assert!(m.sql.contains("CREATE TABLE"));
        }
    }

    #[test]
    fn migration_sql_avoids_if_not_exists() {
        // Accidental replays must fail loudly, never no-op.
        for m in MIGRATIONS {
            assert!(
                !m.sql.to_ascii_lowercase().contains("if not exists"),
                "migration {} uses IF NOT EXISTS",
                m.version
            );
        }
    }

    #[test]
    fn apply_then_verify_is_idempotent() {
        let conn = in_memory();
        let out = apply_all(&conn, "test", 1_000).unwrap();
        assert_eq!(out.applied, vec![1]);
        assert_eq!(out.current_version, 1);
        // Re-run: nothing applied, no error.
        let again = apply_all(&conn, "test", 2_000).unwrap();
        assert!(again.applied.is_empty());
        assert_eq!(again.current_version, 1);
        assert!(verify_receipts(&conn).is_ok());
    }

    #[test]
    fn tampered_version_row_is_rejected() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        // Direct tampering with the receipt version (down to 0).
        conn.execute("UPDATE schema_migrations SET version = 0", [])
            .unwrap();
        match apply_all(&conn, "test", 2_000) {
            Err(StorageError::SchemaTampered { detail }) => {
                // Either check fires: the pragma/receipt disagreement (the
                // row moved to 0 while user_version stayed 1) or the
                // contiguity check. Both are loud tamper rejections.
                assert!(
                    detail.contains("contiguous") || detail.contains("disagrees"),
                    "detail: {detail}"
                )
            }
            other => panic!("expected SchemaTampered, got {other:?}"),
        }
    }

    #[test]
    fn tampered_user_version_is_rejected() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        conn.pragma_update(None, "user_version", 0).unwrap();
        match apply_all(&conn, "test", 2_000) {
            Err(StorageError::SchemaTampered { detail }) => {
                assert!(detail.contains("disagrees"), "detail: {detail}")
            }
            other => panic!("expected SchemaTampered, got {other:?}"),
        }
    }

    #[test]
    fn tampered_fingerprint_is_rejected() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        conn.execute(
            "UPDATE schema_migrations SET fingerprint = 'deadbeef' WHERE version = 1",
            [],
        )
        .unwrap();
        match apply_all(&conn, "test", 2_000) {
            Err(StorageError::SchemaTampered { detail }) => {
                assert!(detail.contains("fingerprint mismatch"), "detail: {detail}")
            }
            other => panic!("expected SchemaTampered, got {other:?}"),
        }
    }

    #[test]
    fn foreign_tables_without_receipts_are_rejected() {
        let conn = in_memory();
        conn.execute("CREATE TABLE stranger (x INTEGER)", [])
            .unwrap();
        match apply_all(&conn, "test", 1_000) {
            Err(StorageError::SchemaTampered { detail }) => {
                assert!(detail.contains("refusing to adopt"), "detail: {detail}")
            }
            other => panic!("expected SchemaTampered, got {other:?}"),
        }
    }

    #[test]
    fn newer_database_is_rejected_not_downgraded() {
        let conn = in_memory();
        apply_all(&conn, "test", 1_000).unwrap();
        // Simulate a database from a newer build: receipt v2 + user_version 2.
        conn.execute(
            "INSERT INTO schema_migrations \
             (version, name, fingerprint, applied_at_unix_ms, applied_by) \
             VALUES (2, 'future', 'fp', 1, 'future-build')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        match apply_all(&conn, "test", 2_000) {
            Err(StorageError::DatabaseTooNew {
                found_version,
                supported_version,
            }) => {
                assert_eq!(found_version, 2);
                assert_eq!(supported_version, 1);
            }
            other => panic!("expected DatabaseTooNew, got {other:?}"),
        }
    }

    #[test]
    fn sqlite_error_mapping_by_primary_code() {
        use rusqlite::ffi::Error as FfiError;
        let mk = |code: i32| {
            rusqlite::Error::SqliteFailure(FfiError::new(code), Some("synthetic".to_string()))
        };
        assert!(matches!(
            map_rusqlite(mk(13)),
            StorageError::DiskFull { .. }
        ));
        assert!(matches!(
            map_rusqlite(mk(266)), // SQLITE_IOERR_READ (extended, primary 10)
            StorageError::Io { .. }
        ));
        assert!(matches!(
            map_rusqlite(mk(261)), // SQLITE_BUSY_RECOVERY (extended, primary 5)
            StorageError::Busy { .. }
        ));
        assert!(matches!(
            map_rusqlite(mk(5)),
            StorageError::Busy { timeout_ms: 0 }
        ));
        assert!(matches!(
            map_rusqlite_busy(mk(5), 7_000),
            StorageError::Busy { timeout_ms: 7_000 }
        ));
        assert!(matches!(map_rusqlite(mk(8)), StorageError::Io { .. }));
        assert!(matches!(map_rusqlite(mk(10)), StorageError::Io { .. }));
        assert!(matches!(
            map_rusqlite(mk(11)),
            StorageError::Corrupted { .. }
        ));
        assert!(matches!(
            map_rusqlite(rusqlite::Error::InvalidParameterCount(1, 2)),
            StorageError::Internal { .. }
        ));
    }
}
