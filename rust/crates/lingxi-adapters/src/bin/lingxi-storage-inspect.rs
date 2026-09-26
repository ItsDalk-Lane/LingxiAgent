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
//! Subcommands:
//!   migrations            Migration receipts + compiled-in fingerprints (JSON)
//!   dump [--table T]      Rows of schema_migrations/sessions/runs/
//!                         run_attempts/invocations/messages/key_events (JSON)
//!   counts                Per-table row counts (JSON)

use std::path::PathBuf;

use lingxi_adapters::storage::migrations;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: lingxi-storage-inspect <DB_PATH> \
<migrations|dump|counts> [--table TABLE]";
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
    let mut counts = serde_json::Map::new();
    for table in DUMP_TABLES {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        counts.insert(table.to_string(), serde_json::json!(n));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::Value::Object(counts))?
    );
    Ok(())
}
