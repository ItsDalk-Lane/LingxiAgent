//! Read-only evidence inspector for the new run/message database
//! (R02-T04). Used by acceptance scripts to prove REAL database state
//! (R02-A07 "重启后不出现半条终态" queries, R02-A08 row-count stability).
//!
//! The tool opens the database with normal flags (a WAL database may need
//! to write the shared-memory file even for reads, e.g. after a writer
//! crash) but executes ONLY SELECT/PRAGMA statements — never a write. The
//! evidence scripts prove this by hashing the database files before and
//! after each inspection.
//!
//! R02-T06 adds the backup/restore evidence commands (they run the REAL
//! Online-Backup-API path from `lingxi_adapters::storage::backup`):
//!   backup <DEST_DIR> [STEM]     Online-API backup of <DB_PATH> into
//!                                DEST_DIR/{STEM}.db + manifest (STEM
//!                                defaults to "runs"). The destination is
//!                                only promoted after integrity_check and
//!                                a failed backup leaves no artifacts.
//!   restore-verify <BACKUP_DIR> <STEM> <TARGET_DIR>
//!                                Hash-verified restore into
//!                                TARGET_DIR/runs.db, then the copy is
//!                                opened through the full recovery path
//!                                (receipts + integrity) and its row
//!                                counts are printed as JSON.
//!
//! Legacy inspection subcommands (first argument = DB path):
//!   migrations            Migration receipts + compiled-in fingerprints (JSON)
//!   dump [--table T]      Rows of schema_migrations/sessions/runs/
//!                         run_attempts/invocations/messages/key_events (JSON)
//!   counts                Per-table row counts (JSON)

use std::path::PathBuf;

use lingxi_adapters::storage::{backup_database, migrations, restore_backup, BackupOptions};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: lingxi-storage-inspect <DB_PATH> \
<migrations|dump|counts|backup> [...]\n\
       lingxi-storage-inspect <DB_PATH> backup <DEST_DIR> [STEM]\n\
       lingxi-storage-inspect restore-verify <BACKUP_DIR> <STEM> <TARGET_DIR>";

    // R02-T06: restore-verify does not take a DB path (the restored copy is
    // produced by the command itself).
    if args.first().map(String::as_str) == Some("restore-verify") {
        if args.len() != 4 {
            eprintln!("{usage}");
            std::process::exit(2);
        }
        // run_restore_verify never returns (it exits the process with a
        // code that reflects the verification outcome).
        run_restore_verify(&PathBuf::from(&args[1]), &args[2], &PathBuf::from(&args[3]));
    }

    if args.len() < 2 {
        eprintln!("{usage}");
        std::process::exit(2);
    }
    let db_path = PathBuf::from(&args[0]);
    let command = args[1].as_str();
    let table_filter = args
        .iter()
        .position(|a| a == "--table")
        .and_then(|pos| args.get(pos + 1))
        .cloned();

    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("cannot open {}: {err}", db_path.display());
            std::process::exit(1);
        }
    };

    let result = match command {
        "migrations" => print_migrations(&conn),
        "dump" => print_dump(&conn, table_filter.as_deref()),
        "counts" => print_counts(&conn),
        "backup" => {
            // backup <DEST_DIR> [STEM] — runs the real adapters backup
            // path (Online Backup API; quiescent: this process is the only
            // connection).
            let Some(dest_dir) = args.get(2) else {
                eprintln!("{usage}");
                std::process::exit(2);
            };
            let stem = args.get(3).cloned().unwrap_or_else(|| "runs".to_string());
            let db_file_name = db_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let wal_path = db_path.with_file_name(format!("{db_file_name}-wal"));
            let wal_bytes_before = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
            match backup_database(
                &conn,
                std::path::Path::new(dest_dir),
                &stem,
                wal_bytes_before,
                BackupOptions::default(),
            ) {
                Ok(outcome) => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "result": "ok",
                            "destPath": "withheld-from-evidence",
                            "fileName": outcome.file_name,
                            "manifestName": outcome.manifest_name,
                            "sha256": outcome.sha256,
                            "bytes": outcome.bytes,
                            "walBytesBefore": outcome.wal_bytes_before,
                            "preCheckpoint": outcome.pre_checkpoint,
                        })
                    );
                    Ok(())
                }
                Err(err) => {
                    // A failed backup publishes NOTHING (no final name, no
                    // manifest, no partial) — surface the failure loudly.
                    eprintln!("backup failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        other => {
            eprintln!("unknown command {other:?}; {usage}");
            std::process::exit(2);
        }
    };
    if let Err(err) = result {
        eprintln!("inspection failed: {err}");
        std::process::exit(1);
    }
}

/// restore-verify: hash-verified restore + full recovery-path open + counts.
fn run_restore_verify(backup_dir: &std::path::Path, stem: &str, target_dir: &std::path::Path) -> ! {
    let restored = match restore_backup(backup_dir, stem, target_dir) {
        Ok(restored) => restored,
        Err(err) => {
            eprintln!("restore failed: {err}");
            std::process::exit(1);
        }
    };
    // Open through the REAL recovery path (receipts + integrity + no
    // downgrade): a restored database must satisfy the same startup gate
    // as a live one. This sync binary drives the async open/close with a
    // tiny current-thread runtime (adapters' locked tokio, `rt` feature).
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("cannot build runtime: {err}");
            std::process::exit(1);
        }
    };
    let open_result = runtime.block_on(async {
        let db = lingxi_adapters::storage::RunDatabase::open(
            &restored.restored_path,
            lingxi_adapters::storage::StoreOptions::default(),
        )
        .await?;
        db.close().await?;
        Ok::<(), lingxi_kernel::ports::StorageError>(())
    });
    if let Err(err) = open_result {
        eprintln!("restored database failed the recovery open: {err}");
        std::process::exit(1);
    }
    // Recovery-open proven; now the evidence counts (same SELECT-only
    // statements as the legacy `counts` subcommand).
    let conn = match rusqlite::Connection::open(&restored.restored_path) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("cannot reopen restored database for counts: {err}");
            std::process::exit(1);
        }
    };
    let counts = match print_counts_value(&conn) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("restored database counts failed: {err}");
            std::process::exit(1);
        }
    };
    println!(
        "{}",
        serde_json::json!({
            "result": "ok",
            "restoredPath": "withheld-from-evidence",
            "sha256": restored.sha256,
            "bytes": restored.bytes,
            "counts": counts,
        })
    );
    std::process::exit(0);
}

fn print_migrations(conn: &rusqlite::Connection) -> Result<(), Box<dyn std::error::Error>> {
    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let mut stmt = conn.prepare(
        "SELECT version, name, fingerprint, applied_at_unix_ms, applied_by \
         FROM schema_migrations ORDER BY version",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "version": row.get::<_, i64>(0)?,
            "name": row.get::<_, String>(1)?,
            "fingerprint": row.get::<_, String>(2)?,
            "appliedAtUnixMs": row.get::<_, i64>(3)?,
            "appliedBy": row.get::<_, String>(4)?,
        }))
    })?;
    let mut receipts = Vec::new();
    for row in rows {
        receipts.push(row?);
    }
    let compiled: Vec<serde_json::Value> = migrations::MIGRATIONS
        .iter()
        .map(|m| {
            serde_json::json!({
                "version": m.version,
                "name": m.name,
                "fingerprint": migrations::fingerprint_sql(m.sql),
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "dbPath": "withheld-from-evidence",
            "userVersion": user_version,
            "supportedVersion": migrations::supported_version(),
            "receipts": receipts,
            "compiledIn": compiled,
        }))?
    );
    Ok(())
}

const DUMP_TABLES: &[&str] = &[
    "schema_migrations",
    "sessions",
    "runs",
    "run_attempts",
    "invocations",
    "messages",
    "key_events",
];

fn print_dump(
    conn: &rusqlite::Connection,
    table_filter: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    for table in DUMP_TABLES {
        if let Some(filter) = table_filter {
            if *table != filter {
                continue;
            }
        }
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table}"))?;
        let headers: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let rows = stmt.query_map([], |row| {
            let mut values = Vec::with_capacity(headers.len());
            for index in 0..headers.len() {
                let value = match row.get_ref(index) {
                    Ok(rusqlite::types::ValueRef::Null) => serde_json::Value::Null,
                    Ok(rusqlite::types::ValueRef::Integer(v)) => serde_json::json!(v),
                    Ok(rusqlite::types::ValueRef::Real(v)) => serde_json::json!(v),
                    Ok(rusqlite::types::ValueRef::Text(v)) => {
                        serde_json::json!(String::from_utf8_lossy(v))
                    }
                    Ok(rusqlite::types::ValueRef::Blob(v)) => {
                        serde_json::json!(format!("blob:{}", v.len()))
                    }
                    Err(_) => serde_json::Value::Null,
                };
                values.push(value);
            }
            Ok(serde_json::json!({ "table": table, "row": values }))
        })?;
        for row in rows {
            let value = row?;
            println!("{value}");
        }
    }
    eprintln!("columns: {DUMP_TABLES:?} — use --table to filter");
    Ok(())
}

fn print_counts(conn: &rusqlite::Connection) -> Result<(), Box<dyn std::error::Error>> {
    let value = print_counts_value(conn)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

/// Shared by the legacy `counts` subcommand and restore-verify evidence.
fn print_counts_value(
    conn: &rusqlite::Connection,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut counts = serde_json::Map::new();
    for table in DUMP_TABLES {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        counts.insert(table.to_string(), serde_json::json!(n));
    }
    Ok(serde_json::Value::Object(counts))
}
