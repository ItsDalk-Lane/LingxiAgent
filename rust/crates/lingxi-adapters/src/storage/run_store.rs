//! The SQLite implementation of the kernel's [`StoragePort`] (R02-T04):
//! the new run/message database behind the bounded single-writer queue.
//!
//! Transaction semantics (contract 02 §7/§8, acceptances R02-A07/A08):
//! - `commit_run_outcome` writes the terminal status, its key events and
//!   the final message in ONE `BEGIN IMMEDIATE` transaction. A failure at
//!   ANY point inside the transaction rolls back everything: the caller
//!   receives an `Err`, no completion events are returned, and after a
//!   restart the database holds either BOTH the terminal row and the
//!   events or NEITHER — never half of a terminal commit.
//! - Events are handed back (for publication) only through the `Ok`
//!   return value, i.e. strictly after the COMMIT succeeded.
//! - `seq` is assigned per stream by this single writer inside the
//!   committing transaction (`MAX(seq)+1`); the `UNIQUE(stream_id, seq)`
//!   constraint is the mechanical backstop.
//! - `record_run_started` is likewise one transaction (run row + first
//!   attempt row + `run_state_changed` key event).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::OptionalExtension;

use lingxi_kernel::ports::{
    CommittedOutcome, InvocationJournalEntry, InvocationPhase, InvocationReceipt, KeyEvent,
    LateResultReason, ReceiptOutcome, RunOutcome, RunRecord, StaleResultFact, StorageError,
    StoragePort,
};
use lingxi_kernel::{FinalizeSettlement, FinalizeVerdict, Principal, RunContext, RunStateMachine};
use lingxi_protocol::canon;
use lingxi_protocol::{
    AttemptId, EventEnvelope, EventId, EventPayload, FinalMessageCommittedPayload,
    KnownEventPayload, RunId, RunStateChangedPayload, RunStatus, Seq, SessionId, StreamId,
    ToolCallId,
};

use super::migrations::{self, MigrationOutcome};
use super::queue::{DbQueue, StoreOptions};

/// Fixed database file name under the service runtime dir (never derived
/// from user input).
pub const RUNS_DB_FILE_NAME: &str = "runs.db";

/// One session row of the new database (the service session surface reads
/// through this shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub session_id: String,
    pub agent_id: String,
    pub owner_user_id: String,
    pub title: String,
    pub created_at_unix_ms: i64,
}

/// One run summary row (recent-runs projections).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummaryRow {
    pub run_id: String,
    pub principal_id: String,
    pub started_at_unix_ms: i64,
    pub status: String,
}

/// Raw `runs` columns of one non-terminal row (pre-parse view; the public
/// shape is [`ActiveRunFacts`]).
struct ActiveRunRow {
    run_id: String,
    session_id: String,
    owner_kind: String,
    owner_subject: String,
    status_name: String,
    generation: i64,
}

/// One NON-terminal run as seen by the R03-T07 startup recovery scan:
/// identity + durable ownership key + status + generation + the run's
/// CURRENT attempt — everything needed to rebuild a write context that the
/// single-writer ownership checks will accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRunFacts {
    pub run_id: String,
    pub session_id: String,
    pub owner_kind: String,
    pub owner_subject: String,
    pub status: RunStatus,
    pub generation: u64,
    pub current_attempt: String,
}

/// The new run/message database: bounded queue + dedicated SQLite worker.
/// Cloning shares the one queue/worker (there is exactly one per database
/// file per process; the composition root owns the original).
#[derive(Clone)]
pub struct RunDatabase {
    queue: Arc<DbQueue>,
    /// Process-wide atomic run-id allocator (R02 stage-repair R1 / F01,
    /// reseeded correctly in stage-repair R2 / R2-F01, strict minted-format
    /// recognition in stage-repair R5 / R5-F02). Seeded at open (AFTER
    /// migrations) from the durable HIGHEST consumed sequence —
    /// `max(COUNT(*), max(seq of stored run ids that STRICTLY match this
    /// allocator's minted shape))` — and shared across clones, so
    /// concurrent callers can never observe the same sequence value and a
    /// restart never re-issues a number that a pre-failure attempt already
    /// consumed (the earlier `COUNT(*)` seed equated "rows present" with
    /// "highest issued": a run whose commit failed left a gap, the count
    /// fell below the highest consumed number, and the next request at the
    /// same clock instant collided with the committed id). Gaps are
    /// harmless; uniqueness is the contract. Foreign/opaque ids count as
    /// consumed ROWS (COUNT) but never contribute a sequence value, and a
    /// genuinely exhausted space surfaces as
    /// [`StorageError::RunIdExhausted`] instead of a panic.
    run_id_seq: Arc<std::sync::atomic::AtomicU64>,
}

impl RunDatabase {
    /// Opens (creating if needed) the run database, runs the open-time
    /// migration pass and verifies receipts. Errors are loud: a newer,
    /// tampered or unreadable database refuses to open.
    ///
    /// R02-T06 adds the integrity half of the recovery gate: after the
    /// receipt check the full `PRAGMA integrity_check` must report `ok`.
    /// A structurally corrupted database is a loud [`StorageError::Corrupted`]
    /// refusal — the file is left untouched (never truncated, never
    /// recreated as an empty stand-in).
    pub async fn open(path: &Path, options: StoreOptions) -> Result<Self, StorageError> {
        let queue = Arc::new(DbQueue::open(path, options)?);
        let db = Self {
            queue,
            run_id_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let applied_by = format!("lingxi-adapters {}", env!("CARGO_PKG_VERSION"));
        let outcome: MigrationOutcome = db
            .queue
            .submit(move |conn| {
                let outcome = migrations::apply_all(conn, &applied_by, now_for_migrations())?;
                migrations::verify_integrity(conn)?;
                Ok(outcome)
            })
            .await?;
        if outcome.applied.is_empty() {
            tracing::debug!(
                db = %db.queue.db_path().display(),
                version = outcome.current_version,
                "run database already at schema version (no migration applied)"
            );
        } else {
            tracing::info!(
                db = %db.queue.db_path().display(),
                applied = ?outcome.applied,
                version = outcome.current_version,
                "run database migrated"
            );
        }
        // Seed the allocator from the durable state AFTER migrations, through
        // the single-writer queue: the seed is the HIGHEST number already
        // consumed — max(COUNT(*), highest seq of any stored run id MINTED
        // BY THIS ALLOCATOR) — so a restart continues above every run id
        // already committed and above every number a failed commit
        // consumed, and ids stay unique across process lifetimes even at an
        // identical millisecond (R2-F01: COUNT(*) alone is the row count,
        // not the high-water mark; a failure gap made it under-seed).
        // R5-F02: only ids that STRICTLY match the minted shape contribute
        // a sequence — opaque/legacy ids (e.g. `…_ffffffffffffffff` tails)
        // never push the seed; their rows are still counted by COUNT(*).
        let seed: u64 = db
            .queue
            .submit(|conn| {
                let mut stmt = conn
                    .prepare("SELECT run_id FROM runs")
                    .map_err(migrations::map_rusqlite)?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(migrations::map_rusqlite)?;
                let mut count = 0u64;
                let mut highest = 0u64;
                for run_id in rows {
                    let run_id = run_id.map_err(migrations::map_rusqlite)?;
                    count += 1;
                    if let Some(seq) = run_id_sequence(&run_id) {
                        highest = highest.max(seq);
                    }
                }
                Ok(count.max(highest))
            })
            .await?;
        db.run_id_seq
            .store(seed, std::sync::atomic::Ordering::Relaxed);
        Ok(db)
    }

    /// Allocates the next unique run id for `now_ms` (R02 stage-repair R1 /
    /// F01). One atomic compare-exchange per call — no read-then-act race,
    /// no two callers ever receive the same id from this process. The id
    /// shape (`run_{millis:016x}_{seq:06x}`) is unchanged; only the seq
    /// source is atomic and seeded from the durable high-water mark at open
    /// (see [`RunDatabase::open`]).
    ///
    /// R02 stage-repair R5 / F02: exhaustion is an explicit, loud
    /// [`StorageError::RunIdExhausted`] — never a panic, never a wrap to 0
    /// that would re-issue numbers owned by stored runs. The counter can
    /// only legitimately sit at `u64::MAX` when a MINTED id of that exact
    /// sequence is stored (foreign ids no longer seed it), so reaching this
    /// error means the space really is consumed for this database.
    pub fn allocate_run_id(&self, now_ms: u64) -> Result<String, StorageError> {
        let mut current = self.run_id_seq.load(std::sync::atomic::Ordering::Relaxed);
        loop {
            let next = current
                .checked_add(1)
                .ok_or_else(|| StorageError::RunIdExhausted {
                    detail: "every u64 sequence value is already consumed by this \
                             database's stored run ids; refusing to wrap or re-issue"
                        .to_string(),
                })?;
            match self.run_id_seq.compare_exchange_weak(
                current,
                next,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(format!("run_{now_ms:016x}_{next:06x}")),
                Err(observed) => current = observed,
            }
        }
    }

    /// Path of the underlying database file.
    pub fn db_path(&self) -> &Path {
        self.queue.db_path()
    }

    /// Queue options (bound/busy/checkpoint knobs actually in force).
    pub fn options(&self) -> &StoreOptions {
        self.queue.options()
    }

    /// R06-T03: crate-internal access to the bounded single-writer queue so
    /// sibling storage modules (session_tree) submit work through the SAME
    /// serialization point as every other mutation — never a second writer.
    pub(crate) fn queue(&self) -> &Arc<DbQueue> {
        &self.queue
    }

    /// Evidence helper for the R02-A11 harness: canonical logical dump of
    /// the fact rows (sorted, read through the single-writer queue).
    /// Compares the LOGICAL content of source vs restored databases —
    /// physical file bytes differ (checkpoint state), the facts must not.
    pub async fn logical_dump(&self) -> Result<String, StorageError> {
        self.queue
            .submit(|conn| {
                let dump_rows = |table: &str,
                                 order: &str|
                 -> Result<serde_json::Value, StorageError> {
                    let sql = format!("SELECT * FROM {table} ORDER BY {order}");
                    let mut stmt = conn.prepare(&sql).map_err(migrations::map_rusqlite)?;
                    let headers: Vec<String> =
                        stmt.column_names().iter().map(|s| s.to_string()).collect();
                    let rows = stmt
                        .query_map([], |row| {
                            let mut values = Vec::with_capacity(headers.len());
                            for index in 0..headers.len() {
                                let value = match row.get_ref(index) {
                                    Ok(rusqlite::types::ValueRef::Null) => serde_json::Value::Null,
                                    Ok(rusqlite::types::ValueRef::Integer(v)) => {
                                        serde_json::json!(v)
                                    }
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
                            Ok(serde_json::Value::Array(values))
                        })
                        .map_err(migrations::map_rusqlite)?;
                    let mut out = Vec::new();
                    for row in rows {
                        out.push(row.map_err(migrations::map_rusqlite)?);
                    }
                    Ok(serde_json::Value::Array(out))
                };
                let doc = serde_json::json!({
                    "sessions": dump_rows("sessions", "session_id")?,
                    "runs": dump_rows("runs", "run_id")?,
                    "run_attempts": dump_rows("run_attempts", "run_id, attempt")?,
                    "key_events": dump_rows("key_events", "stream_id, seq")?,
                });
                serde_json::to_string(&doc).map_err(|err| StorageError::Internal {
                    detail: format!("cannot serialize logical dump: {err}"),
                })
            })
            .await
    }

    /// Evidence helper for the R02-A11 concurrent-writer case: run count,
    /// key-event count and the ordered run ids of a (restored) database.
    pub async fn run_fact_summary(&self) -> Result<(i64, i64, Vec<String>), StorageError> {
        self.queue
            .submit(|conn| {
                let run_count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))
                    .map_err(migrations::map_rusqlite)?;
                let event_count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM key_events", [], |r| r.get(0))
                    .map_err(migrations::map_rusqlite)?;
                let mut stmt = conn
                    .prepare("SELECT run_id FROM runs ORDER BY run_id")
                    .map_err(migrations::map_rusqlite)?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(migrations::map_rusqlite)?;
                let mut ids = Vec::new();
                for row in rows {
                    ids.push(row.map_err(migrations::map_rusqlite)?);
                }
                Ok((run_count, event_count, ids))
            })
            .await
    }

    /// Graceful close: FIFO-drains queued work, checkpoints the WAL
    /// (TRUNCATE) and joins the worker. See [`DbQueue::close`].
    pub async fn close(&self) -> Result<(), StorageError> {
        self.queue.close().await
    }

    /// Online-Backup-API snapshot into `{dest_dir}/{file_stem}.db`
    /// (R02-T06). The job runs on the single-writer worker: writers are
    /// quiesced for its duration AND the copy goes through the SQLite
    /// Online Backup API — never a plain copy of the active main file
    /// (ADR-004 D3). See [`super::backup`].
    pub async fn backup_to(
        &self,
        dest_dir: &Path,
        file_stem: &str,
        options: super::backup::BackupOptions,
    ) -> Result<super::backup::BackupOutcome, StorageError> {
        let wal_bytes_before = std::fs::metadata(wal_sidecar_path(self.db_path()))
            .map(|meta| meta.len())
            .unwrap_or(0);
        let dest = dest_dir.to_path_buf();
        let stem = file_stem.to_string();
        self.queue
            .submit(move |conn| {
                super::backup::backup_database(conn, &dest, &stem, wal_bytes_before, options)
            })
            .await
    }

    /// Idempotent session seed (`INSERT OR IGNORE` on the primary key).
    pub async fn ensure_session_seed(&self, rows: Vec<SessionRow>) -> Result<(), StorageError> {
        self.queue
            .submit(move |conn| {
                let tx = conn
                    .unchecked_transaction()
                    .map_err(migrations::map_rusqlite)?;
                for row in &rows {
                    tx.execute(
                        "INSERT OR IGNORE INTO sessions \
                         (session_id, agent_id, owner_user_id, title, created_at_unix_ms) \
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
                            row.session_id,
                            row.agent_id,
                            row.owner_user_id,
                            row.title,
                            row.created_at_unix_ms
                        ],
                    )
                    .map_err(migrations::map_rusqlite)?;
                }
                tx.commit().map_err(migrations::map_rusqlite)?;
                Ok(())
            })
            .await
    }

    /// Reads one session row.
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionRow>, StorageError> {
        let session_id = session_id.to_string();
        self.queue
            .submit(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT session_id, agent_id, owner_user_id, title, \
                         created_at_unix_ms FROM sessions WHERE session_id = ?1",
                    )
                    .map_err(migrations::map_rusqlite)?;
                let mut rows = stmt
                    .query([&session_id])
                    .map_err(migrations::map_rusqlite)?;
                match rows.next().map_err(migrations::map_rusqlite)? {
                    Some(row) => Ok(Some(
                        session_from_row(row).map_err(migrations::map_rusqlite)?,
                    )),
                    None => Ok(None),
                }
            })
            .await
    }

    /// 最近会话排前；服务层继续按身份过滤，CLI 读取前 20 项。
    /// REPAIR-R1 FINDING-03：主列表只含 active（归档语义 = 从日常工作面
    /// 消失，对照现役归档文件移出活跃目录；归档会话走 list_archived_sessions）。
    pub async fn list_sessions(&self) -> Result<Vec<SessionRow>, StorageError> {
        self.queue
            .submit(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT session_id, agent_id, owner_user_id, title, \
                         created_at_unix_ms FROM sessions \
                         WHERE lifecycle = 'active' \
                         ORDER BY created_at_unix_ms DESC, session_id DESC",
                    )
                    .map_err(migrations::map_rusqlite)?;
                let rows = stmt
                    .query_map([], session_from_row)
                    .map_err(migrations::map_rusqlite)?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(migrations::map_rusqlite)?);
                }
                Ok(out)
            })
            .await
    }

    /// Number of runs recorded for a session.
    pub async fn count_runs(&self, session_id: &str) -> Result<u64, StorageError> {
        let session_id = session_id.to_string();
        self.queue
            .submit(move |conn| {
                let n: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
                        [&session_id],
                        |row| row.get(0),
                    )
                    .map_err(migrations::map_rusqlite)?;
                Ok(n as u64)
            })
            .await
    }

    /// Total runs across all sessions (used for globally unique run ids).
    pub async fn total_runs(&self) -> Result<u64, StorageError> {
        self.queue
            .submit(move |conn| {
                let n: i64 = conn
                    .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
                    .map_err(migrations::map_rusqlite)?;
                Ok(n as u64)
            })
            .await
    }

    /// R03 RR2/F05-01 — the durable cross-restart request-anchor lookup
    /// (the G04/F05 C05 contract, extended with the legacy compatible
    /// read). Every USER run created with an explicit requestId records
    /// its lineage with `cause_id = "request:{id}"` (origin `user`); this
    /// answers how THIS (owner kind, owner subject, session, CANONICAL
    /// request id) namespace's anchors relate to existing run rows — from
    /// ANY process life. Run rows and lineage rows are never deleted or
    /// rewritten, so every hit is a stable frozen fact.
    ///
    /// The match is done in RUST against the single canonicalization rule
    /// ([`lingxi_kernel::subagent::canonical_request_id`] — the FULL
    /// Unicode `White_Space` set), NOT with SQLite's `TRIM()`, whose
    /// default character set covers only a small ASCII subset and would
    /// silently miss U+3000/U+00A0-style legacy rows. A row matches when
    /// its `cause_id` is `request:` + some raw id whose canonical form
    /// equals the queried canonical id — this keeps the PRE-FIX rows
    /// (whose anchor held the raw, un-normalized id) linkable, which an
    /// exact-equality query would miss.
    ///
    /// Outcomes: [`RequestBindingLookup::Unbound`] (fresh at the durable
    /// layer), [`RequestBindingLookup::Bound`] (exactly one run — the
    /// canonical anchor or a single legacy variant), or
    /// [`RequestBindingLookup::Ambiguous`] (SEVERAL distinct runs bound to
    /// the same logical key — the pre-fix duplicate-execution shape; the
    /// submission surface refuses such a key loudly instead of picking a
    /// binding or re-executing).
    pub async fn find_request_binding(
        &self,
        session_id: &str,
        owner_kind: &str,
        owner_subject: &str,
        canonical_request_id: &str,
    ) -> Result<lingxi_kernel::ports::RequestBindingLookup, StorageError> {
        use lingxi_kernel::ports::RequestBindingLookup;
        use lingxi_kernel::subagent::REQUEST_CAUSE_ID_PREFIX;

        let (session_id, owner_kind, owner_subject, canonical) = (
            session_id.to_string(),
            owner_kind.to_string(),
            owner_subject.to_string(),
            canonical_request_id.to_string(),
        );
        self.queue
            .submit(move |conn| {
                // The LIKE only NARROWS the scan (every user-cause anchor
                // starts with the prefix); the authoritative match is the
                // Rust-side prefix strip + canonical comparison below.
                let mut stmt = conn
                    .prepare(
                        "SELECT l.run_id, l.cause_id FROM run_lineage l \
                         JOIN runs r ON r.run_id = l.run_id \
                         WHERE r.session_id = ?1 AND r.owner_kind = ?2 AND r.owner_subject = ?3 \
                           AND l.origin = 'user' AND l.cause_id LIKE ?4 \
                         ORDER BY r.created_at_unix_ms DESC, l.run_id DESC",
                    )
                    .map_err(migrations::map_rusqlite)?;
                let like_probe = format!("{REQUEST_CAUSE_ID_PREFIX}%");
                let mut rows = stmt
                    .query(rusqlite::params![
                        session_id,
                        owner_kind,
                        owner_subject,
                        like_probe
                    ])
                    .map_err(migrations::map_rusqlite)?;
                // run_lineage's primary key is run_id, so each run appears
                // at most once; collect the matches newest-first.
                let mut bound: Vec<String> = Vec::new();
                while let Some(row) = rows.next().map_err(migrations::map_rusqlite)? {
                    let run_id: String = row.get(0).map_err(migrations::map_rusqlite)?;
                    let cause_id: Option<String> = row.get(1).map_err(migrations::map_rusqlite)?;
                    let Some(raw) = cause_id
                        .as_deref()
                        .and_then(|c| c.strip_prefix(REQUEST_CAUSE_ID_PREFIX))
                    else {
                        continue;
                    };
                    if lingxi_kernel::subagent::canonical_request_id(raw) == canonical {
                        bound.push(run_id);
                    }
                }
                drop(rows);
                drop(stmt);
                Ok(match bound.as_slice() {
                    [] => RequestBindingLookup::Unbound,
                    [only] => RequestBindingLookup::Bound {
                        run_id: only.clone(),
                    },
                    many => RequestBindingLookup::Ambiguous {
                        run_ids: many.to_vec(),
                    },
                })
            })
            .await
    }

    /// Lists every NON-terminal run (R03-T07 startup recovery scan): the
    /// dangling-active rows a previous process left behind — `queued`,
    /// `running`, `waiting_approval` and `cancelling`. Terminal rows are
    /// never returned (恢复后不复活： a settled run is not recovery's business).
    ///
    /// One row carries everything the recovery coordinator needs to rebuild
    /// the run's write context and settle it through the single finalize
    /// path: identity (run id / session), the durable ownership key, the
    /// status, the generation and the CURRENT attempt. An unknown status
    /// value is a loud [`StorageError::Corrupted`], never a guess.
    pub async fn list_active_runs(&self) -> Result<Vec<ActiveRunFacts>, StorageError> {
        self.queue
            .submit(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT run_id, session_id, owner_kind, owner_subject, status, \
                         generation FROM runs \
                         WHERE status IN ('queued', 'running', 'waiting_approval', 'cancelling') \
                         ORDER BY created_at_unix_ms, run_id",
                    )
                    .map_err(migrations::map_rusqlite)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok(ActiveRunRow {
                            run_id: row.get(0)?,
                            session_id: row.get(1)?,
                            owner_kind: row.get(2)?,
                            owner_subject: row.get(3)?,
                            status_name: row.get(4)?,
                            generation: row.get(5)?,
                        })
                    })
                    .map_err(migrations::map_rusqlite)?;
                let mut out = Vec::new();
                for row in rows {
                    let raw = row.map_err(migrations::map_rusqlite)?;
                    let run_id = raw.run_id.clone();
                    let generation =
                        u64::try_from(raw.generation).map_err(|_| StorageError::Corrupted {
                            detail: format!("runs row {run_id} holds a negative generation"),
                        })?;
                    out.push(ActiveRunFacts {
                        run_id,
                        session_id: raw.session_id,
                        owner_kind: raw.owner_kind,
                        owner_subject: raw.owner_subject,
                        status: parse_status(&raw.status_name)?,
                        generation,
                        current_attempt: current_attempt_of(conn, &raw.run_id)?.ok_or_else(
                            || StorageError::Corrupted {
                                detail: format!(
                                    "runs row {} has no attempt rows (record_run_started \
                                     always opens attempt #1)",
                                    raw.run_id
                                ),
                            },
                        )?,
                    });
                }
                Ok(out)
            })
            .await
    }

    /// Most recent runs of a session (newest first).
    pub async fn recent_runs(
        &self,
        session_id: &str,
        limit: u32,
    ) -> Result<Vec<RunSummaryRow>, StorageError> {
        let session_id = session_id.to_string();
        self.queue
            .submit(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT run_id, principal_id, created_at_unix_ms, status \
                         FROM runs WHERE session_id = ?1 \
                         ORDER BY created_at_unix_ms DESC, run_id DESC LIMIT ?2",
                    )
                    .map_err(migrations::map_rusqlite)?;
                let rows = stmt
                    .query_map(rusqlite::params![session_id, limit as i64], |row| {
                        Ok(RunSummaryRow {
                            run_id: row.get(0)?,
                            principal_id: row.get(1)?,
                            started_at_unix_ms: row.get(2)?,
                            status: row.get(3)?,
                        })
                    })
                    .map_err(migrations::map_rusqlite)?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(migrations::map_rusqlite)?);
                }
                Ok(out)
            })
            .await
    }

    /// Retention maintenance (R02-T05): deletes committed key events with
    /// `seq < before_seq` (exclusive) for one stream, returning the number
    /// of rows removed. Runs in one write transaction on the same
    /// single-writer queue as every other mutation. This is the honest
    /// product surface that creates the R02-A10 "cursor beyond retention"
    /// precondition; read paths detect the resulting floor gap explicitly.
    pub async fn purge_events_before(
        &self,
        stream_id: &str,
        before_seq: lingxi_protocol::Seq,
    ) -> Result<u64, StorageError> {
        let stream_id = stream_id.to_string();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let removed = conn
                        .execute(
                            "DELETE FROM key_events WHERE stream_id = ?1 AND seq < ?2",
                            rusqlite::params![stream_id, before_seq.value() as i64],
                        )
                        .map_err(migrations::map_rusqlite)?;
                    Ok(removed as u64)
                })
            })
            .await
    }

    /// One atomic resume read of a stream (R02 stage-repair R1 / F02):
    /// floor + head + first page in a SINGLE queue submission. The
    /// single-writer queue serializes the whole slice against any
    /// concurrent [`RunDatabase::purge_events_before`], so the truncation
    /// verdict the subscriber derives from `floor` and the returned `events`
    /// page are one consistent database state — a purge can no longer
    /// commit between a separate floor read and page read and turn a
    /// truncated resume into a silently holed one.
    pub async fn stream_resume_slice(
        &self,
        stream_id: &str,
        after_seq: lingxi_protocol::Seq,
        limit: u32,
    ) -> Result<StreamResumeSlice, StorageError> {
        let stream_id = stream_id.to_string();
        let after = after_seq.value() as i64;
        self.queue
            .submit(move |conn| {
                let floor: Option<i64> = conn
                    .query_row(
                        "SELECT MIN(seq) FROM key_events WHERE stream_id = ?1",
                        [&stream_id],
                        |row| row.get(0),
                    )
                    .map_err(migrations::map_rusqlite)?;
                let head: Option<i64> = conn
                    .query_row(
                        "SELECT MAX(seq) FROM key_events WHERE stream_id = ?1",
                        [&stream_id],
                        |row| row.get(0),
                    )
                    .map_err(migrations::map_rusqlite)?;
                let mut stmt = conn
                    .prepare(
                        "SELECT event_id, stream_id, seq, session_id, run_id, attempt, \
                         event_type, payload_json FROM key_events \
                         WHERE stream_id = ?1 AND seq > ?2 ORDER BY seq ASC LIMIT ?3",
                    )
                    .map_err(migrations::map_rusqlite)?;
                let rows = stmt
                    .query_map(rusqlite::params![stream_id, after, limit as i64], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    })
                    .map_err(migrations::map_rusqlite)?;
                let mut events = Vec::new();
                for row in rows {
                    events.push(envelope_from_parts(row.map_err(migrations::map_rusqlite)?)?);
                }
                Ok(StreamResumeSlice {
                    floor: floor.map(|f| seq_from_i64(f, "MIN(seq)")).transpose()?,
                    head: head.map(|h| seq_from_i64(h, "MAX(seq)")).transpose()?,
                    events,
                })
            })
            .await
    }

    /// Raw SQL probe (read-only SELECT expected) — evidence/inspection
    /// seam used by tests to query the REAL database file through the same
    /// worker. Never used on the write path.
    pub async fn query_one_text(
        &self,
        sql: &str,
        params: Vec<String>,
    ) -> Result<Option<String>, StorageError> {
        let sql = sql.to_string();
        self.queue
            .submit(move |conn| {
                let mut stmt = conn.prepare(&sql).map_err(migrations::map_rusqlite)?;
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
                let mut rows = stmt
                    .query(param_refs.as_slice())
                    .map_err(migrations::map_rusqlite)?;
                match rows.next().map_err(migrations::map_rusqlite)? {
                    Some(row) => {
                        // Tolerate TEXT and INTEGER results (evidence queries
                        // read both shapes); NULL and other types map to
                        // None, never to a fabricated value.
                        let value = match row.get_ref(0) {
                            Ok(rusqlite::types::ValueRef::Text(bytes)) => {
                                Some(String::from_utf8_lossy(bytes).into_owned())
                            }
                            Ok(rusqlite::types::ValueRef::Integer(n)) => Some(n.to_string()),
                            _ => None,
                        };
                        Ok(value)
                    }
                    None => Ok(None),
                }
            })
            .await
    }
}

/// The `-wal` sidecar path of a database file (diagnostics/backup stats).
pub fn wal_sidecar_path(db_path: &Path) -> PathBuf {
    let mut s = db_path.as_os_str().to_os_string();
    s.push("-wal");
    PathBuf::from(s)
}

fn session_from_row(row: &rusqlite::Row<'_>) -> Result<SessionRow, rusqlite::Error> {
    Ok(SessionRow {
        session_id: row.get(0)?,
        agent_id: row.get(1)?,
        owner_user_id: row.get(2)?,
        title: row.get(3)?,
        created_at_unix_ms: row.get(4)?,
    })
}

/// Migration receipts carry the caller-supplied time; the queue closure
/// cannot capture per-call values from `open`, so the open-time pass uses
/// the real clock (documented: receipts are informational rows; version
/// correctness is enforced by the receipts themselves).
fn now_for_migrations() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── SQL helpers shared by the port implementation ───────────────────────────

struct RunRow {
    session_id: String,
    owner_kind: String,
    owner_subject: String,
    status: String,
    terminal_reason: Option<String>,
    #[allow(dead_code)]
    attempt_count: i64,
    last_event_seq: i64,
}

fn load_run_row(conn: &rusqlite::Connection, run_id: &str) -> Result<Option<RunRow>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT session_id, owner_kind, owner_subject, status, terminal_reason, \
             attempt_count, last_event_seq FROM runs WHERE run_id = ?1",
        )
        .map_err(migrations::map_rusqlite)?;
    let mut rows = stmt.query([run_id]).map_err(migrations::map_rusqlite)?;
    match rows.next().map_err(migrations::map_rusqlite)? {
        Some(row) => Ok(Some(RunRow {
            session_id: row.get(0).map_err(migrations::map_rusqlite)?,
            owner_kind: row.get(1).map_err(migrations::map_rusqlite)?,
            owner_subject: row.get(2).map_err(migrations::map_rusqlite)?,
            status: row.get(3).map_err(migrations::map_rusqlite)?,
            terminal_reason: row.get(4).map_err(migrations::map_rusqlite)?,
            attempt_count: row.get(5).map_err(migrations::map_rusqlite)?,
            last_event_seq: row.get(6).map_err(migrations::map_rusqlite)?,
        })),
        None => Ok(None),
    }
}

fn parse_status(name: &str) -> Result<RunStatus, StorageError> {
    match name {
        "queued" => Ok(RunStatus::Queued),
        "running" => Ok(RunStatus::Running),
        "waiting_approval" => Ok(RunStatus::WaitingApproval),
        "cancelling" => Ok(RunStatus::Cancelling),
        "cancelled" => Ok(RunStatus::Cancelled),
        "completed" => Ok(RunStatus::Completed),
        "failed" => Ok(RunStatus::Failed),
        "interrupted_needs_attention" => Ok(RunStatus::InterruptedNeedsAttention),
        other => Err(StorageError::Corrupted {
            detail: format!("runs.status holds unknown value {other:?}"),
        }),
    }
}

/// Next per-stream sequence, assigned inside the caller's transaction.
fn next_seq(conn: &rusqlite::Connection, stream_id: &str) -> Result<i64, StorageError> {
    let current: Option<i64> = conn
        .query_row(
            "SELECT MAX(seq) FROM key_events WHERE stream_id = ?1",
            [stream_id],
            |row| row.get(0),
        )
        .map_err(migrations::map_rusqlite)?;
    Ok(current.unwrap_or(0) + 1)
}

/// Persists one key event row inside the open transaction and advances the
/// run's `last_event_seq`.
fn stage_event(
    conn: &rusqlite::Connection,
    stream_id: &str,
    session_id: &str,
    now_ms: i64,
    event: &KeyEvent,
    run_id: Option<&str>,
    attempt: Option<&str>,
) -> Result<(EventEnvelope, i64), StorageError> {
    let seq = next_seq(conn, stream_id)?;
    let payload_json = canon::canonical_string(&event.payload);
    conn.execute(
        "INSERT INTO key_events \
         (event_id, stream_id, seq, session_id, run_id, attempt, event_type, \
          payload_json, committed_at_unix_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            event.event_id.as_str(),
            stream_id,
            seq,
            session_id,
            run_id,
            attempt,
            event.payload.event_type(),
            payload_json,
            now_ms
        ],
    )
    .map_err(migrations::map_rusqlite)?;
    let envelope = EventEnvelope::new(
        event.event_id.clone(),
        StreamId::new(stream_id.to_string()),
        Seq::new(seq as u64),
        SessionId::new(session_id.to_string()),
        run_id.map(lingxi_protocol::RunId::new),
        attempt.map(AttemptId::new),
        event.payload.clone(),
    );
    Ok((envelope, seq))
}

fn ctx_facts(ctx: &RunContext) -> (String, String, String, String, String, u64) {
    (
        ctx.run_id.to_string(),
        ctx.session_id.to_string(),
        ctx.principal.storage_kind().to_string(),
        ctx.principal.storage_subject(),
        principal_audit_id(&ctx.principal),
        ctx.generation,
    )
}

fn principal_audit_id(principal: &Principal) -> String {
    match principal {
        Principal::LocalUser => "principal_local".to_string(),
        Principal::Device { device_id, .. } => format!("principal_device_{device_id}"),
        Principal::WebSession { account_id } => format!("principal_web_{account_id}"),
        Principal::Automation { surface } => format!("principal_automation_{surface}"),
    }
}

/// The run's CURRENT attempt id: the most recently OPENED attempt (single
/// writer ⇒ insertion order = opening order; `rowid` is the monotonic
/// witness). `None` when the run has no attempt rows at all (a run started
/// through `record_run_started` always has attempt #1).
fn current_attempt_of(
    conn: &rusqlite::Connection,
    run_id: &str,
) -> Result<Option<String>, StorageError> {
    conn.query_row(
        "SELECT attempt FROM run_attempts WHERE run_id = ?1 ORDER BY rowid DESC LIMIT 1",
        [run_id],
        |row| row.get(0),
    )
    .map(Some)
    .or_else(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(migrations::map_rusqlite(other)),
    })
}

/// Inserts one audit-only stale-result row inside the caller's transaction
/// (the R03-T04 fence's durable trace). NEVER touches key_events, runs or
/// messages — a refused write leaves exactly this trace and nothing else.
fn insert_stale_audit(
    conn: &rusqlite::Connection,
    ctx: &RunContext,
    reason: LateResultReason,
    refused_event_types: &[String],
    now_ms: u64,
) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO stale_result_audit \
         (run_id, session_id, attempt, generation, owner_kind, owner_subject, reason, \
          refused_event_types, recorded_at_unix_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            ctx.run_id.as_str(),
            ctx.session_id.as_str(),
            ctx.attempt.as_str(),
            ctx.generation as i64,
            ctx.principal.storage_kind(),
            ctx.principal.storage_subject(),
            reason.name(),
            refused_event_types.join(","),
            now_ms as i64
        ],
    )
    .map(|_| ())
    .map_err(migrations::map_rusqlite)
}

/// The event-type summary of a refused delivery (audit payload: types only,
/// never full payloads — refused content is not run state).
fn event_type_summary(events: &[KeyEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| e.payload.event_type().to_string())
        .collect()
}

// ── R03-T05: invocation-journal helpers ──────────────────────────────────────

/// One raw `invocation_journal` row (the write-path view; the read path
/// rebuilds the public [`InvocationJournalEntry`] via
/// [`journal_entry_from_row`]).
struct JournalRow {
    session_id: String,
    run_id: String,
    attempt: String,
    generation: u64,
    owner_kind: String,
    owner_subject: String,
    target: String,
    args_digest: String,
    args_summary: Option<String>,
    idempotency_key: Option<String>,
    phase: InvocationPhase,
    receipt: Option<InvocationReceipt>,
}

/// Loads one journal row by id (`None` when no such entry).
fn load_journal_row(
    conn: &rusqlite::Connection,
    journal_id: &str,
) -> Result<Option<JournalRow>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT session_id, run_id, attempt, generation, owner_kind, owner_subject, target, \
             args_digest, args_summary, idempotency_key, phase, receipt_outcome, receipt_detail, \
             dedup_id, dispatched \
             FROM invocation_journal WHERE journal_id = ?1",
        )
        .map_err(migrations::map_rusqlite)?;
    let mut rows = stmt.query([journal_id]).map_err(migrations::map_rusqlite)?;
    match rows.next().map_err(migrations::map_rusqlite)? {
        Some(row) => {
            let cell = |index: usize| -> Result<String, StorageError> {
                row.get::<_, String>(index)
                    .map_err(migrations::map_rusqlite)
            };
            Ok(Some(JournalRow {
                session_id: cell(0)?,
                run_id: cell(1)?,
                attempt: cell(2)?,
                generation: u64::try_from(row.get::<_, i64>(3).map_err(migrations::map_rusqlite)?)
                    .map_err(|_| StorageError::Corrupted {
                        detail: format!(
                            "invocation_journal {journal_id} holds a negative generation"
                        ),
                    })?,
                owner_kind: cell(4)?,
                owner_subject: cell(5)?,
                target: cell(6)?,
                args_digest: cell(7)?,
                args_summary: row.get(8).map_err(migrations::map_rusqlite)?,
                idempotency_key: row.get(9).map_err(migrations::map_rusqlite)?,
                phase: InvocationPhase::parse(&cell(10)?).ok_or_else(|| {
                    StorageError::Corrupted {
                        detail: format!(
                            "invocation_journal {journal_id} holds an unknown phase value"
                        ),
                    }
                })?,
                receipt: receipt_from_columns(
                    row.get::<_, Option<String>>(12)
                        .map_err(migrations::map_rusqlite)?,
                    row.get::<_, Option<String>>(11)
                        .map_err(migrations::map_rusqlite)?,
                    row.get(13).map_err(migrations::map_rusqlite)?,
                    row.get(14).map_err(migrations::map_rusqlite)?,
                )?,
            }))
        }
        None => Ok(None),
    }
}

/// Rebuilds the receipt columns into the public receipt (a receipt_outcome
/// without its detail/dedup/dispatched siblings is corruption — they commit
/// together in one UPDATE).
fn receipt_from_columns(
    detail: Option<String>,
    outcome_name: Option<String>,
    dedup_id: Option<String>,
    dispatched: Option<i64>,
) -> Result<Option<InvocationReceipt>, StorageError> {
    match (outcome_name, detail) {
        (None, None) => Ok(None),
        (Some(outcome_name), Some(detail)) => {
            let outcome = match outcome_name.as_str() {
                "succeeded" => ReceiptOutcome::Succeeded,
                "failed" => ReceiptOutcome::Failed,
                "unknown" => ReceiptOutcome::Unknown,
                _ => {
                    return Err(StorageError::Corrupted {
                        detail: format!(
                            "invocation_journal holds an unknown receipt outcome {outcome_name:?}"
                        ),
                    })
                }
            };
            let dispatched = match dispatched {
                Some(0) => false,
                Some(_) => true,
                None => {
                    return Err(StorageError::Corrupted {
                        detail: "invocation_journal receipt row is missing its dispatched flag"
                            .to_string(),
                    })
                }
            };
            Ok(Some(InvocationReceipt {
                outcome,
                detail,
                dedup_id,
                dispatched,
            }))
        }
        _ => Err(StorageError::Corrupted {
            detail: "invocation_journal holds a half-written receipt (outcome without detail)"
                .to_string(),
        }),
    }
}

/// Row mapper producing the public journal entry (read path). A row whose
/// phase/receipt vocabulary or generation/timestamp columns are not legal
/// is a LOUD error (corruption is never guessed into a value).
fn journal_entry_from_row(row: &rusqlite::Row<'_>) -> Result<InvocationJournalEntry, StorageError> {
    let corrupt = |what: &str| StorageError::Corrupted {
        detail: format!("invocation_journal row is corrupt: {what}"),
    };
    let phase_name: String = row.get(11).map_err(migrations::map_rusqlite)?;
    let phase = InvocationPhase::parse(&phase_name)
        .ok_or_else(|| corrupt(&format!("unknown phase value {phase_name:?}")))?;
    let receipt_outcome: Option<String> = row.get(12).map_err(migrations::map_rusqlite)?;
    let receipt_detail: Option<String> = row.get(13).map_err(migrations::map_rusqlite)?;
    let dedup_id: Option<String> = row.get(14).map_err(migrations::map_rusqlite)?;
    let dispatched: Option<i64> = row.get(15).map_err(migrations::map_rusqlite)?;
    let receipt = match (receipt_outcome, receipt_detail, dispatched) {
        (None, None, None) => None,
        (Some(outcome_name), Some(detail), Some(dispatched)) => {
            let outcome = match outcome_name.as_str() {
                "succeeded" => ReceiptOutcome::Succeeded,
                "failed" => ReceiptOutcome::Failed,
                "unknown" => ReceiptOutcome::Unknown,
                _ => {
                    return Err(corrupt(&format!(
                        "unknown receipt outcome {outcome_name:?}"
                    )))
                }
            };
            Some(InvocationReceipt {
                outcome,
                detail,
                dedup_id,
                dispatched: dispatched != 0,
            })
        }
        _ => return Err(corrupt("half-written receipt columns")),
    };
    let generation = u64::try_from(row.get::<_, i64>(4).map_err(migrations::map_rusqlite)?)
        .map_err(|_| corrupt("negative generation"))?;
    let prepared_at_unix_ms =
        u64::try_from(row.get::<_, i64>(16).map_err(migrations::map_rusqlite)?)
            .map_err(|_| corrupt("negative prepared_at"))?;
    let updated_at_unix_ms =
        u64::try_from(row.get::<_, i64>(17).map_err(migrations::map_rusqlite)?)
            .map_err(|_| corrupt("negative updated_at"))?;
    Ok(InvocationJournalEntry {
        journal_id: row.get(0).map_err(migrations::map_rusqlite)?,
        session_id: row.get(1).map_err(migrations::map_rusqlite)?,
        run_id: row.get(2).map_err(migrations::map_rusqlite)?,
        attempt: row.get(3).map_err(migrations::map_rusqlite)?,
        generation,
        owner_kind: row.get(5).map_err(migrations::map_rusqlite)?,
        owner_subject: row.get(6).map_err(migrations::map_rusqlite)?,
        target: row.get(7).map_err(migrations::map_rusqlite)?,
        args_digest: row.get(8).map_err(migrations::map_rusqlite)?,
        args_summary: row.get(9).map_err(migrations::map_rusqlite)?,
        idempotency_key: row.get(10).map_err(migrations::map_rusqlite)?,
        phase,
        receipt,
        prepared_at_unix_ms,
        updated_at_unix_ms,
    })
}

/// The owner-triple check every ctx-carrying journal mutation performs (a
/// journal write under a foreign owner is a boundary violation).
fn check_journal_owner(
    entry: &JournalRow,
    run_id: &str,
    session_id: &str,
    owner_kind: &str,
    owner_subject: &str,
    verb: &str,
) -> Result<(), StorageError> {
    if entry.run_id != run_id
        || entry.session_id != session_id
        || entry.owner_kind != owner_kind
        || entry.owner_subject != owner_subject
    {
        return Err(StorageError::Conflict {
            detail: format!(
                "cannot {verb} invocation: the journal entry belongs to run {}/{}/{}/{} but the \
                 caller claims run {run_id}/{session_id}/{owner_kind}/{owner_subject}",
                entry.run_id, entry.session_id, entry.owner_kind, entry.owner_subject
            ),
        });
    }
    Ok(())
}

/// BEGIN IMMEDIATE + f + COMMIT/ROLLBACK with busy-error decoration.
pub(crate) fn with_write_txn<T>(
    conn: &rusqlite::Connection,
    busy_timeout_ms: u64,
    f: impl FnOnce(&rusqlite::Connection) -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|err| migrations::map_rusqlite_busy(err, busy_timeout_ms))?;
    match f(conn) {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .map_err(|err| migrations::map_rusqlite_busy(err, busy_timeout_ms))?;
            Ok(value)
        }
        Err(err) => {
            if let Err(rollback_err) = conn.execute_batch("ROLLBACK") {
                return Err(StorageError::Internal {
                    detail: format!(
                        "transaction failed ({err}) AND rollback failed ({rollback_err})"
                    ),
                });
            }
            Err(err)
        }
    }
}

/// Rebuilds the durable envelopes of a run's already-committed key events
/// (idempotent replay path: publication can catch up from the store).
fn read_run_events(
    conn: &rusqlite::Connection,
    run_id: &str,
) -> Result<Vec<EventEnvelope>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT event_id, stream_id, seq, session_id, attempt, event_type, payload_json \
             FROM key_events WHERE run_id = ?1 ORDER BY seq",
        )
        .map_err(migrations::map_rusqlite)?;
    let rows = stmt
        .query_map([run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(migrations::map_rusqlite)?;
    let mut out = Vec::new();
    for row in rows {
        let (event_id, stream_id, seq, session_id, attempt, _event_type, payload_json) =
            row.map_err(migrations::map_rusqlite)?;
        let payload: EventPayload =
            serde_json::from_str(&payload_json).map_err(|err| StorageError::Corrupted {
                detail: format!("key_events payload for {event_id} is not valid JSON: {err}"),
            })?;
        out.push(EventEnvelope::new(
            EventId::new(event_id),
            StreamId::new(stream_id),
            Seq::new(seq as u64),
            SessionId::new(session_id),
            Some(lingxi_protocol::RunId::new(run_id.to_string())),
            attempt.map(AttemptId::new),
            payload,
        ));
    }
    Ok(out)
}

/// Loads the final message a terminal run committed (`{run_id}-final` row),
/// if any — the comparison half of the strengthened idempotency check
/// (R03-T01 step 3: only COMPLETELY identical settlements replay).
fn read_stored_final_message(
    conn: &rusqlite::Connection,
    run_id: &str,
) -> Result<Option<lingxi_protocol::NormalizedMessage>, StorageError> {
    let mut stmt = conn
        .prepare("SELECT content_json FROM messages WHERE message_id = ?1")
        .map_err(migrations::map_rusqlite)?;
    let mut rows = stmt
        .query([format!("{run_id}-final")])
        .map_err(migrations::map_rusqlite)?;
    match rows.next().map_err(migrations::map_rusqlite)? {
        Some(row) => {
            let content_json: String = row.get(0).map_err(migrations::map_rusqlite)?;
            serde_json::from_str(&content_json).map_err(|err| StorageError::Corrupted {
                detail: format!("final message of run {run_id} is not valid JSON: {err}"),
            })
        }
        None => Ok(None),
    }
}

impl StoragePort for RunDatabase {
    async fn record_run_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> Result<CommittedOutcome, StorageError> {
        let (run_id, session_id, owner_kind, owner_subject, principal_id, generation) =
            ctx_facts(ctx);
        let attempt = ctx.attempt.to_string();
        let stream_id = session_id.clone();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    // The session must exist: a run outside any session is
                    // not a legal fact.
                    let session: Option<String> = conn
                        .query_row(
                            "SELECT session_id FROM sessions WHERE session_id = ?1",
                            [&session_id],
                            |row| row.get(0),
                        )
                        .map(Some)
                        .or_else(|err| match err {
                            rusqlite::Error::QueryReturnedNoRows => Ok(None),
                            other => Err(other),
                        })
                        .map_err(migrations::map_rusqlite)?;
                    if session.is_none() {
                        return Err(StorageError::InvalidRequest {
                            detail: format!("run {run_id} references unknown session {session_id}"),
                        });
                    }
                    if let Some(existing) = load_run_row(conn, &run_id)? {
                        // Idempotent replay: identical start already
                        // recorded for the same session/owner/attempt.
                        let same = existing.session_id == session_id
                            && existing.owner_kind == owner_kind
                            && existing.owner_subject == owner_subject
                            && existing.status == "running";
                        let attempt_present: bool = conn
                            .query_row(
                                "SELECT 1 FROM run_attempts WHERE run_id = ?1 AND attempt = ?2",
                                rusqlite::params![run_id, attempt],
                                |_| Ok(()),
                            )
                            .is_ok();
                        if same && attempt_present {
                            let events = read_run_events(conn, &run_id)?;
                            return Ok(CommittedOutcome {
                                newly_committed: false,
                                events,
                            });
                        }
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} already exists (session {}, status {})",
                                existing.session_id, existing.status
                            ),
                        });
                    }
                    conn.execute(
                        "INSERT INTO runs \
                         (run_id, session_id, owner_kind, owner_subject, principal_id, \
                          attempt_count, status, generation, created_at_unix_ms, \
                          updated_at_unix_ms, last_event_seq) \
                         VALUES (?1, ?2, ?3, ?4, ?5, 1, 'running', ?6, ?7, ?7, 0)",
                        rusqlite::params![
                            run_id,
                            session_id,
                            owner_kind,
                            owner_subject,
                            principal_id,
                            generation as i64,
                            now_unix_ms as i64
                        ],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    conn.execute(
                        "INSERT INTO run_attempts \
                         (run_id, attempt, generation, started_at_unix_ms) \
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![run_id, attempt, generation as i64, now_unix_ms as i64],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    let event = KeyEvent {
                        event_id: EventId::new(format!("{run_id}-start")),
                        payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                            RunStateChangedPayload {
                                from: RunStatus::Queued,
                                to: RunStatus::Running,
                                reason: None,
                            },
                        )),
                    };
                    let (envelope, seq) = stage_event(
                        conn,
                        &stream_id,
                        &session_id,
                        now_unix_ms as i64,
                        &event,
                        Some(&run_id),
                        Some(&attempt),
                    )?;
                    conn.execute(
                        "UPDATE runs SET last_event_seq = ?1, updated_at_unix_ms = ?2 \
                         WHERE run_id = ?3",
                        rusqlite::params![seq, now_unix_ms as i64, run_id],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(CommittedOutcome {
                        newly_committed: true,
                        events: vec![envelope],
                    })
                })
            })
            .await
    }

    async fn commit_run_outcome(
        &self,
        ctx: &RunContext,
        outcome: RunOutcome,
        now_unix_ms: u64,
    ) -> Result<CommittedOutcome, StorageError> {
        outcome.validate_terminal()?;
        let (run_id, session_id, owner_kind, owner_subject, _principal_id, _generation) =
            ctx_facts(ctx);
        let attempt = ctx.attempt.to_string();
        let target_status = outcome.status.wire_name().to_string();
        let reason = outcome.reason.clone();
        let key_events = outcome.key_events;
        let final_message = outcome.final_message;
        let stream_id = session_id.clone();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let Some(run) = load_run_row(conn, &run_id)? else {
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "cannot finalize run {run_id}: no run row was ever committed"
                            ),
                        });
                    };
                    if run.session_id != session_id || run.owner_kind != owner_kind
                        || run.owner_subject != owner_subject
                    {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} belongs to session {}/{}/{}; the finalize \
                                 context claims {session_id}/{owner_kind}/{owner_subject}",
                                run.session_id, run.owner_kind, run.owner_subject
                            ),
                        });
                    }
                    let stored_status = parse_status(&run.status)?;
                    // R03-T01 step 3: the kernel's finalize decision is the
                    // single authority. When the run is already terminal,
                    // the STORED settlement (status + reason + final
                    // message) is loaded and compared — only a COMPLETELY
                    // identical re-submission replays idempotently; a
                    // same-status settlement with a different reason or a
                    // different final message is a diagnosed conflict, never
                    // a silent merge.
                    let stored_settlement: Option<FinalizeSettlement> = if stored_status
                        .is_terminal()
                    {
                        Some(FinalizeSettlement {
                            status: stored_status,
                            reason: run.terminal_reason.clone(),
                            final_message: read_stored_final_message(conn, &run_id)?,
                        })
                    } else {
                        None
                    };
                    let requested = FinalizeSettlement {
                        status: parse_status(&target_status)?,
                        reason: reason.clone(),
                        final_message: final_message.clone(),
                    };
                    match RunStateMachine::finalize(
                        stored_status,
                        stored_settlement.as_ref(),
                        &requested,
                    ) {
                        Ok(FinalizeVerdict::IdempotentReplay { .. }) => {
                            let events = read_run_events(conn, &run_id)?;
                            return Ok(CommittedOutcome {
                                newly_committed: false,
                                events,
                            });
                        }
                        Ok(FinalizeVerdict::Commit { .. }) => {
                            // Legal first finalize: fall through to stage the
                            // events, the final message and the status in
                            // this same transaction.
                        }
                        Err(rejection) => {
                            return Err(if rejection.is_conflict() {
                                StorageError::Conflict {
                                    detail: format!("run {run_id}: {rejection}"),
                                }
                            } else if matches!(
                                rejection.kind,
                                lingxi_kernel::FinalizeRejectionKind::CorruptSettlement
                            ) {
                                StorageError::Corrupted {
                                    detail: format!("run {run_id}: {rejection}"),
                                }
                            } else {
                                StorageError::InvalidRequest {
                                    detail: format!("run {run_id}: {rejection}"),
                                }
                            });
                        }
                    }

                    let mut events = Vec::new();
                    let mut last_seq = run.last_event_seq;
                    for event in &key_events {
                        let (envelope, seq) = stage_event(
                            conn,
                            &stream_id,
                            &session_id,
                            now_unix_ms as i64,
                            event,
                            Some(&run_id),
                            Some(&attempt),
                        )?;
                        last_seq = seq;
                        events.push(envelope);
                    }
                    // A final message, when present, is committed in the
                    // SAME transaction: message row + its
                    // final_message_committed key event.
                    if let Some(message) = &final_message {
                        let message_seq: i64 = conn
                            .query_row(
                                "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_id = ?1",
                                [&session_id],
                                |row| row.get(0),
                            )
                            .map_err(migrations::map_rusqlite)?;
                        let content_json = canon::canonical_string(message);
                        // R06-T03: the final message is a node of the session
                        // message TREE — its parent is the current branch head
                        // (session_branch_heads; fallback: the session's
                        // max-seq message for pre-v8 rows that predate branch
                        // heads) and it ADVANCES the branch head, so the
                        // branch projection / fork copies see the assistant
                        // reply, not just the user input. Without this the
                        // chain would break at every final message (parent
                        // NULL) and fork would copy user-only histories.
                        let final_message_id = format!("{run_id}-final");
                        let recorded_head: Option<String> = conn
                            .query_row(
                                "SELECT head_message_id FROM session_branch_heads \
                                 WHERE session_id = ?1",
                                [&session_id],
                                |row| row.get::<_, Option<String>>(0),
                            )
                            .optional()
                            .map_err(migrations::map_rusqlite)?
                            .flatten();
                        let parent_message_id: Option<String> = match recorded_head {
                            Some(head) => Some(head),
                            None => conn
                                .query_row(
                                    "SELECT message_id FROM messages WHERE session_id = ?1 \
                                     ORDER BY seq DESC LIMIT 1",
                                    [&session_id],
                                    |row| row.get(0),
                                )
                                .optional()
                                .map_err(migrations::map_rusqlite)?,
                        };
                        conn.execute(
                            "INSERT INTO messages \
                             (message_id, session_id, run_id, role, content_json, \
                              model_call_id, committed_at_unix_ms, seq, parent_message_id) \
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                            rusqlite::params![
                                final_message_id,
                                session_id,
                                run_id,
                                message.role,
                                content_json,
                                message.model_call_id.as_ref().map(|id| id.to_string()),
                                now_unix_ms as i64,
                                message_seq,
                                parent_message_id,
                            ],
                        )
                        .map_err(migrations::map_rusqlite)?;
                        // R06-T03: the branch head advances to the final
                        // message in the SAME transaction (the head never
                        // points at a superseded position after a terminal
                        // commit).
                        conn.execute(
                            "INSERT INTO session_branch_heads \
                             (session_id, head_message_id, observed_tail_message_id, revision, \
                              head_resolution, updated_at_unix_ms) \
                             VALUES (?1, ?2, ?2, 0, 'persisted_head', ?3) \
                             ON CONFLICT(session_id) DO UPDATE SET \
                               head_message_id = excluded.head_message_id, \
                               revision = session_branch_heads.revision + 1, \
                               head_resolution = 'persisted_head', \
                               updated_at_unix_ms = excluded.updated_at_unix_ms",
                            rusqlite::params![session_id, final_message_id, now_unix_ms as i64],
                        )
                        .map_err(migrations::map_rusqlite)?;
                        let event = KeyEvent {
                            event_id: EventId::new(format!("{run_id}-final")),
                            payload: EventPayload::Known(
                                KnownEventPayload::FinalMessageCommitted(
                                    FinalMessageCommittedPayload {
                                        message: message.clone(),
                                    },
                                ),
                            ),
                        };
                        let (envelope, seq) = stage_event(
                            conn,
                            &stream_id,
                            &session_id,
                            now_unix_ms as i64,
                            &event,
                            Some(&run_id),
                            Some(&attempt),
                        )?;
                        last_seq = seq;
                        events.push(envelope);
                    }
                    conn.execute(
                        "UPDATE runs SET status = ?1, terminal_reason = ?2, \
                         last_event_seq = ?3, updated_at_unix_ms = ?4 WHERE run_id = ?5",
                        rusqlite::params![
                            target_status,
                            reason,
                            last_seq,
                            now_unix_ms as i64,
                            run_id
                        ],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(CommittedOutcome {
                        newly_committed: true,
                        events,
                    })
                })
            })
            .await
    }

    async fn load_run(
        &self,
        run_id: &lingxi_protocol::RunId,
    ) -> Result<Option<RunRecord>, StorageError> {
        let run_id = run_id.to_string();
        self.queue
            .submit(move |conn| {
                let Some(run) = load_run_row(conn, &run_id)? else {
                    return Ok(None);
                };
                Ok(Some(RunRecord {
                    run_id: lingxi_protocol::RunId::new(run_id),
                    session_id: SessionId::new(run.session_id),
                    status: parse_status(&run.status)?,
                    last_event_seq: Seq::new(run.last_event_seq as u64),
                }))
            })
            .await
    }

    async fn record_run_events(
        &self,
        ctx: &RunContext,
        events: Vec<KeyEvent>,
        now_unix_ms: u64,
    ) -> Result<CommittedOutcome, StorageError> {
        let (run_id, session_id, owner_kind, owner_subject, _principal_id, _generation) =
            ctx_facts(ctx);
        let attempt = ctx.attempt.to_string();
        let stream_id = session_id.clone();
        // The audit legs below record the CLAIMED identity facts; the
        // queue closure must own them ('static).
        let audit_ctx = ctx.clone();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                // Validation reads run inside this single-writer job: the
                // bounded queue is one worker, so no other storage job can
                // interleave between these reads and the committed outcome
                // below (the same serialization a BEGIN IMMEDIATE would
                // rely on). A REFUSED delivery commits its audit-only
                // trace in its own small transaction first — a rollback of
                // a refusing transaction must never swallow the audit
                // ("拒写但留审计痕迹").
                let Some(run) = load_run_row(conn, &run_id)? else {
                    return Err(StorageError::InvalidRequest {
                        detail: format!("cannot attach events to run {run_id}: no run row exists"),
                    });
                };
                if run.session_id != session_id
                    || run.owner_kind != owner_kind
                    || run.owner_subject != owner_subject
                {
                    // A cross-owner write is a boundary violation, not a
                    // late result of this run's own work: loud conflict,
                    // no audit row (the security-audit surfaces own that
                    // class of probe).
                    return Err(StorageError::Conflict {
                        detail: format!(
                            "run {run_id} belongs to session {}/{}/{}; the event context \
                             claims {session_id}/{owner_kind}/{owner_subject}",
                            run.session_id, run.owner_kind, run.owner_subject
                        ),
                    });
                }
                let stored_status = parse_status(&run.status)?;
                if stored_status.is_terminal() {
                    // R03-T04 full fence: a late result for a settled run
                    // is REFUSED for state purposes but its arrival is
                    // committed as an audit-only stale fact — a durable,
                    // diagnosable trace, never a resurrection and never a
                    // silent drop.
                    let refused = event_type_summary(&events);
                    with_write_txn(conn, busy, |conn| {
                        insert_stale_audit(
                            conn,
                            &audit_ctx,
                            LateResultReason::RunTerminal,
                            &refused,
                            now_unix_ms,
                        )
                    })?;
                    tracing::warn!(
                        run_id = %run_id,
                        attempt = %attempt,
                        status = %run.status,
                        refused_types = ?refused,
                        "late result for a terminal run refused and audited \
                         (stale_result_audit reason=run_terminal)"
                    );
                    return Err(StorageError::Conflict {
                        detail: format!(
                            "run {run_id} is already terminal as {}; mid-run events \
                             for a settled run are refused and audited as a stale result \
                             (stale_result_audit reason=run_terminal)",
                            run.status
                        ),
                    });
                }
                // Attempt fence (R03-T04, upgrading the T01 floor): the
                // claimed attempt must be the run's CURRENT attempt — the
                // most recently opened one. An attempt that never started
                // cannot pose as run output; an OPENED attempt that a
                // retry superseded is a late result from the old attempt:
                // refused + audited, so old results can never pollute the
                // current attempt's stream.
                let attempt_open: bool = conn
                    .query_row(
                        "SELECT 1 FROM run_attempts WHERE run_id = ?1 AND attempt = ?2",
                        rusqlite::params![run_id, attempt],
                        |_| Ok(()),
                    )
                    .is_ok();
                if !attempt_open {
                    let refused = event_type_summary(&events);
                    with_write_txn(conn, busy, |conn| {
                        insert_stale_audit(
                            conn,
                            &audit_ctx,
                            LateResultReason::AttemptNeverOpened,
                            &refused,
                            now_unix_ms,
                        )
                    })?;
                    tracing::warn!(
                        run_id = %run_id,
                        attempt = %attempt,
                        refused_types = ?refused,
                        "result for an attempt that never opened refused and audited \
                         (stale_result_audit reason=attempt_never_opened)"
                    );
                    return Err(StorageError::Conflict {
                        detail: format!(
                            "attempt {attempt} was never opened on run {run_id}; results \
                             cannot attach to an attempt that never started (refused and \
                             audited: stale_result_audit reason=attempt_never_opened)"
                        ),
                    });
                }
                let current_attempt = current_attempt_of(conn, &run_id)?;
                if current_attempt.as_deref() != Some(attempt.as_str()) {
                    let superseding = current_attempt
                        .clone()
                        .unwrap_or_else(|| "<none>".to_string());
                    let refused = event_type_summary(&events);
                    with_write_txn(conn, busy, |conn| {
                        insert_stale_audit(
                            conn,
                            &audit_ctx,
                            LateResultReason::AttemptStale,
                            &refused,
                            now_unix_ms,
                        )
                    })?;
                    tracing::warn!(
                        run_id = %run_id,
                        attempt = %attempt,
                        current_attempt = %superseding,
                        refused_types = ?refused,
                        "late result from a superseded attempt refused and audited \
                         (stale_result_audit reason=attempt_stale)"
                    );
                    return Err(StorageError::Conflict {
                        detail: format!(
                            "attempt {attempt} is no longer the current attempt of run \
                             {run_id} (current: {superseding}); the late result is refused \
                             and audited (stale_result_audit reason=attempt_stale)"
                        ),
                    });
                }
                with_write_txn(conn, busy, |conn| {
                    if events.is_empty() {
                        // Nothing to stage; still a legal no-op read-only
                        // fact (the validation above ran).
                        return Ok(CommittedOutcome {
                            newly_committed: false,
                            events: Vec::new(),
                        });
                    }
                    let mut envelopes = Vec::with_capacity(events.len());
                    let mut last_seq = run.last_event_seq;
                    for event in &events {
                        let (envelope, seq) = stage_event(
                            conn,
                            &stream_id,
                            &session_id,
                            now_unix_ms as i64,
                            event,
                            Some(&run_id),
                            Some(&attempt),
                        )?;
                        last_seq = seq;
                        envelopes.push(envelope);
                    }
                    conn.execute(
                        "UPDATE runs SET last_event_seq = ?1, updated_at_unix_ms = ?2 \
                         WHERE run_id = ?3",
                        rusqlite::params![last_seq, now_unix_ms as i64, run_id],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(CommittedOutcome {
                        newly_committed: true,
                        events: envelopes,
                    })
                })
            })
            .await
    }

    /// Audit-only stale-result record (R03-T04 fence): the durable trace of
    /// a refused late result. Writes exactly one `stale_result_audit` row —
    /// never the run's stream, status or messages. Legal whatever the run's
    /// state (the audited CLAIM may be bogus; the audit proves what
    /// arrived).
    async fn record_stale_result(
        &self,
        ctx: &RunContext,
        refused: StaleResultFact,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        let ctx = ctx.clone();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    insert_stale_audit(
                        conn,
                        &ctx,
                        refused.reason,
                        &refused.refused_event_types,
                        now_unix_ms,
                    )
                })
            })
            .await
    }

    async fn record_attempt_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> Result<CommittedOutcome, StorageError> {
        let (run_id, session_id, owner_kind, owner_subject, _principal_id, generation) =
            ctx_facts(ctx);
        let attempt = ctx.attempt.to_string();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let Some(run) = load_run_row(conn, &run_id)? else {
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "cannot start an attempt on run {run_id}: no run row exists"
                            ),
                        });
                    };
                    if run.session_id != session_id
                        || run.owner_kind != owner_kind
                        || run.owner_subject != owner_subject
                    {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} belongs to session {}/{}/{}; the attempt context \
                                 claims {session_id}/{owner_kind}/{owner_subject}",
                                run.session_id, run.owner_kind, run.owner_subject
                            ),
                        });
                    }
                    let stored_status = parse_status(&run.status)?;
                    if stored_status.is_terminal() {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} is already terminal as {}; a finished run never \
                                 gains attempts — re-execution is a NEW run",
                                run.status
                            ),
                        });
                    }
                    if stored_status != RunStatus::Running {
                        // T01 opens attempts on running runs; waiting-
                        // approval attempts arrive with the R04 approval
                        // gateway.
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "attempt can only start on a running run; {run_id} is {}",
                                stored_status.wire_name()
                            ),
                        });
                    }
                    let exists: bool = conn
                        .query_row(
                            "SELECT 1 FROM run_attempts WHERE run_id = ?1 AND attempt = ?2",
                            rusqlite::params![run_id, attempt],
                            |_| Ok(()),
                        )
                        .is_ok();
                    if exists {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "attempt {attempt} already exists on run {run_id}; each retry \
                                 is a FRESH attempt identity"
                            ),
                        });
                    }
                    conn.execute(
                        "INSERT INTO run_attempts \
                         (run_id, attempt, generation, started_at_unix_ms) \
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![run_id, attempt, generation as i64, now_unix_ms as i64],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    conn.execute(
                        "UPDATE runs SET attempt_count = attempt_count + 1, \
                         updated_at_unix_ms = ?1 WHERE run_id = ?2",
                        rusqlite::params![now_unix_ms as i64, run_id],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    // The run's STATUS does not change (still running); the
                    // durable attempt row is the fact. No key event: the
                    // known vocabulary has no attempt_started type, and
                    // model-call/tool events carry their attempt id in the
                    // envelope.
                    Ok(CommittedOutcome {
                        newly_committed: true,
                        events: Vec::new(),
                    })
                })
            })
            .await
    }

    async fn record_run_state_change(
        &self,
        ctx: &RunContext,
        from: RunStatus,
        to: RunStatus,
        reason: Option<String>,
        now_unix_ms: u64,
    ) -> Result<CommittedOutcome, StorageError> {
        let (run_id, session_id, owner_kind, owner_subject, _principal_id, _generation) =
            ctx_facts(ctx);
        let attempt = ctx.attempt.to_string();
        let stream_id = session_id.clone();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let Some(run) = load_run_row(conn, &run_id)? else {
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "cannot change the phase of run {run_id}: no run row exists"
                            ),
                        });
                    };
                    if run.session_id != session_id
                        || run.owner_kind != owner_kind
                        || run.owner_subject != owner_subject
                    {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} belongs to session {}/{}/{}; the phase-change \
                                 context claims {session_id}/{owner_kind}/{owner_subject}",
                                run.session_id, run.owner_kind, run.owner_subject
                            ),
                        });
                    }
                    // Terminal targets belong to commit_run_outcome alone —
                    // this surface owns the NON-terminal legs only (the
                    // durable `cancelling` entry and `waiting_approval`
                    // round trips).
                    if to.is_terminal() {
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "record_run_state_change only persists non-terminal targets; {} \
                                 is terminal and must go through commit_run_outcome",
                                to.wire_name()
                            ),
                        });
                    }
                    let stored_status = parse_status(&run.status)?;
                    if stored_status.is_terminal() {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} is already terminal as {}; an active phase change \
                                 on a settled run is refused",
                                run.status
                            ),
                        });
                    }
                    if stored_status != from {
                        // The caller operated on a stale view of the run:
                        // diagnosed loudly, never silently absorbed (the
                        // run may legitimately already sit in the target
                        // phase — the caller then re-observes and proceeds).
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "phase change {}->{} claimed the run was {}, but the \
                                 durable row holds {}; stale-view callers are diagnosed, never \
                                 silently absorbed",
                                from.wire_name(),
                                to.wire_name(),
                                from.wire_name(),
                                stored_status.wire_name()
                            ),
                        });
                    }
                    // The kernel state machine is the single transition
                    // authority (same as every other write of this port).
                    RunStateMachine::transition(stored_status, to).map_err(|err| {
                        StorageError::InvalidRequest {
                            detail: format!(
                                "phase change {}->{} rejected by the kernel state \
                                 machine: {}",
                                from.wire_name(),
                                to.wire_name(),
                                err.reason
                            ),
                        }
                    })?;
                    // One transaction: the status update + a
                    // `run_state_changed` key event (+ last_event_seq). The
                    // event id derives from the seq it will occupy inside
                    // this transaction (unique within the stream).
                    let seq = next_seq(conn, &stream_id)?;
                    let event = KeyEvent {
                        event_id: EventId::new(format!("{run_id}-sc{seq}")),
                        payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                            RunStateChangedPayload {
                                from,
                                to,
                                reason: reason.clone(),
                            },
                        )),
                    };
                    let (envelope, last_seq) = stage_event(
                        conn,
                        &stream_id,
                        &session_id,
                        now_unix_ms as i64,
                        &event,
                        Some(&run_id),
                        Some(&attempt),
                    )?;
                    debug_assert_eq!(seq, last_seq);
                    conn.execute(
                        "UPDATE runs SET status = ?1, last_event_seq = ?2, \
                         updated_at_unix_ms = ?3 WHERE run_id = ?4",
                        rusqlite::params![to.wire_name(), last_seq, now_unix_ms as i64, run_id],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(CommittedOutcome {
                        newly_committed: true,
                        events: vec![envelope],
                    })
                })
            })
            .await
    }

    // ── R03-T05: the InvocationJournal ───────────────────────────────────

    async fn record_invocation_intent(
        &self,
        ctx: &RunContext,
        intent: lingxi_kernel::ports::InvocationIntent,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        let (run_id, session_id, owner_kind, owner_subject, _principal_id, generation) =
            ctx_facts(ctx);
        let attempt = ctx.attempt.to_string();
        let journal_id = intent.journal_id.to_string();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let run = load_run_row(conn, &run_id)?.ok_or_else(|| {
                        StorageError::InvalidRequest {
                            detail: format!(
                                "cannot journal invocation {journal_id}: run {run_id} has no row"
                            ),
                        }
                    })?;
                    if run.session_id != session_id
                        || run.owner_kind != owner_kind
                        || run.owner_subject != owner_subject
                    {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} belongs to session {}/{}/{}; the journal context \
                                 claims {session_id}/{owner_kind}/{owner_subject}",
                                run.session_id, run.owner_kind, run.owner_subject
                            ),
                        });
                    }
                    if let Some(existing) = load_journal_row(conn, &journal_id)? {
                        // Idempotent replay: the IDENTICAL intent (same
                        // binding, still intent-only) is a no-op; anything
                        // else under this journal id is a loud conflict.
                        let identical = existing.run_id == run_id
                            && existing.attempt == attempt
                            && existing.generation == generation
                            && existing.session_id == session_id
                            && existing.owner_kind == owner_kind
                            && existing.owner_subject == owner_subject
                            && existing.target == intent.target
                            && existing.args_digest == intent.args_digest
                            && existing.args_summary == intent.args_summary
                            && existing.idempotency_key == intent.idempotency_key
                            && !existing.phase.is_closed()
                            && existing.phase != InvocationPhase::Unknown;
                        if identical {
                            return Ok(());
                        }
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "journal id {journal_id} already holds a {} entry for run {} \
                                 (attempt {}); a conflicting intent is refused",
                                existing.phase.wire_name(),
                                existing.run_id,
                                existing.attempt
                            ),
                        });
                    }
                    conn.execute(
                        "INSERT INTO invocation_journal \
                         (journal_id, session_id, run_id, attempt, generation, owner_kind, \
                          owner_subject, target, args_digest, args_summary, idempotency_key, \
                          phase, prepared_at_unix_ms, updated_at_unix_ms) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'prepared', \
                          ?12, ?12)",
                        rusqlite::params![
                            journal_id,
                            session_id,
                            run_id,
                            attempt,
                            generation as i64,
                            owner_kind,
                            owner_subject,
                            intent.target,
                            intent.args_digest,
                            intent.args_summary,
                            intent.idempotency_key,
                            now_unix_ms as i64
                        ],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(())
                })
            })
            .await
    }

    async fn advance_invocation(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        to: InvocationPhase,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        if !matches!(to, InvocationPhase::Authorized | InvocationPhase::Started) {
            return Err(StorageError::InvalidRequest {
                detail: format!(
                    "advance_invocation only moves an entry to authorized/started, not {}",
                    to.wire_name()
                ),
            });
        }
        let (run_id, session_id, owner_kind, owner_subject, _principal_id, _generation) =
            ctx_facts(ctx);
        let journal_id = journal_id.to_string();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let entry = load_journal_row(conn, &journal_id)?.ok_or_else(|| {
                        StorageError::InvalidRequest {
                            detail: format!(
                                "cannot advance invocation {journal_id}: no journal entry exists"
                            ),
                        }
                    })?;
                    check_journal_owner(
                        &entry,
                        &run_id,
                        &session_id,
                        &owner_kind,
                        &owner_subject,
                        "advance",
                    )?;
                    let current = entry.phase;
                    if current == to {
                        // Idempotent re-advance to the current phase (a
                        // retry after a lost response, for example).
                        return Ok(());
                    }
                    let legal = matches!(
                        (current, to),
                        (InvocationPhase::Prepared, InvocationPhase::Authorized)
                            | (InvocationPhase::Authorized, InvocationPhase::Started)
                            | (InvocationPhase::Prepared, InvocationPhase::Started)
                    );
                    if !legal {
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "invocation {journal_id} cannot advance {} → {} (ladder is \
                                 prepared → authorized → started; closed entries never move)",
                                current.wire_name(),
                                to.wire_name()
                            ),
                        });
                    }
                    conn.execute(
                        "UPDATE invocation_journal SET phase = ?1, updated_at_unix_ms = ?2 \
                         WHERE journal_id = ?3",
                        rusqlite::params![to.wire_name(), now_unix_ms as i64, journal_id],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(())
                })
            })
            .await
    }

    async fn record_invocation_receipt(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        receipt: InvocationReceipt,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        let (run_id, session_id, owner_kind, owner_subject, _principal_id, _generation) =
            ctx_facts(ctx);
        let journal_id = journal_id.to_string();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let entry = load_journal_row(conn, &journal_id)?.ok_or_else(|| {
                        StorageError::InvalidRequest {
                            detail: format!(
                                "cannot close invocation {journal_id}: no journal entry exists"
                            ),
                        }
                    })?;
                    check_journal_owner(
                        &entry,
                        &run_id,
                        &session_id,
                        &owner_kind,
                        &owner_subject,
                        "close",
                    )?;
                    if let Some(existing) = &entry.receipt {
                        if entry.phase.is_closed() {
                            // Closed entries replay only the COMPLETELY
                            // identical receipt (mirrors the run-finalize
                            // idempotency rule).
                            if existing.outcome == receipt.outcome
                                && existing.detail == receipt.detail
                                && existing.dedup_id == receipt.dedup_id
                                && existing.dispatched == receipt.dispatched
                            {
                                return Ok(());
                            }
                            return Err(StorageError::Conflict {
                                detail: format!(
                                    "invocation {journal_id} already holds a {} receipt ({:?}); \
                                     a conflicting receipt is refused",
                                    entry.phase.wire_name(),
                                    existing.outcome.wire_name()
                                ),
                            });
                        }
                        // The entry sits at `unknown` with a placeholder
                        // receipt (a recovery verdict or an unobserved
                        // result). `unknown` is NOT a settlement: an
                        // identical re-close replays, and a VERIFIED
                        // receipt settles it (falls through to the close
                        // below — exactly the resume-with-key shape of
                        // R03-A10).
                        if entry.phase == InvocationPhase::Unknown
                            && receipt.outcome == ReceiptOutcome::Unknown
                            && existing.detail == receipt.detail
                        {
                            return Ok(());
                        }
                    }
                    // A SUCCESS receipt proves external execution happened:
                    // it is only legal from `started` (the live path) or
                    // from `unknown` (a verified settlement of a formerly
                    // unconfirmed outcome). Failed/Unknown receipts are
                    // legal from any not-yet-closed phase (a rejected
                    // approval closes from `prepared` with
                    // dispatched=false, for example).
                    if receipt.outcome == ReceiptOutcome::Succeeded
                        && !matches!(
                            entry.phase,
                            InvocationPhase::Started | InvocationPhase::Unknown
                        )
                    {
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "invocation {journal_id} cannot record a succeeded receipt from \
                                 phase {} (success proves the external execution happened; it is \
                                 only legal from started or a verified-unknown settlement)",
                                entry.phase.wire_name()
                            ),
                        });
                    }
                    let phase = match receipt.outcome {
                        ReceiptOutcome::Succeeded => InvocationPhase::Succeeded,
                        ReceiptOutcome::Failed => InvocationPhase::Failed,
                        ReceiptOutcome::Unknown => InvocationPhase::Unknown,
                    };
                    conn.execute(
                        "UPDATE invocation_journal SET phase = ?1, receipt_outcome = ?2, \
                         receipt_detail = ?3, dedup_id = ?4, dispatched = ?5, \
                         updated_at_unix_ms = ?6 WHERE journal_id = ?7",
                        rusqlite::params![
                            phase.wire_name(),
                            receipt.outcome.wire_name(),
                            receipt.detail,
                            receipt.dedup_id,
                            receipt.dispatched,
                            now_unix_ms as i64,
                            journal_id
                        ],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(())
                })
            })
            .await
    }

    async fn record_invocation_unknown(
        &self,
        journal_id: &ToolCallId,
        detail: String,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        let journal_id = journal_id.to_string();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let entry = load_journal_row(conn, &journal_id)?.ok_or_else(|| {
                        StorageError::InvalidRequest {
                            detail: format!(
                                "cannot mark invocation {journal_id} unknown: no journal entry \
                                 exists"
                            ),
                        }
                    })?;
                    match entry.phase {
                        // The recovery window: started without a receipt.
                        InvocationPhase::Started => {}
                        // Already classified unknown: idempotent replay of
                        // the same honest verdict.
                        InvocationPhase::Unknown if entry.receipt.is_some() => return Ok(()),
                        // Anything else (intent-only, closed) is not an
                        // unknown-outcome entry — refusing loudly beats
                        // papering over a caller bug.
                        other => {
                            return Err(StorageError::InvalidRequest {
                                detail: format!(
                                    "invocation {journal_id} is {} (receipt: {}); only a \
                                     started entry without a receipt can be marked unknown at \
                                     recovery",
                                    other.wire_name(),
                                    if entry.receipt.is_some() {
                                        "present"
                                    } else {
                                        "absent"
                                    }
                                ),
                            })
                        }
                    }
                    conn.execute(
                        "UPDATE invocation_journal SET phase = 'unknown', receipt_outcome = \
                         'unknown', receipt_detail = ?1, dedup_id = NULL, dispatched = 1, \
                         updated_at_unix_ms = ?2 WHERE journal_id = ?3",
                        rusqlite::params![detail, now_unix_ms as i64, journal_id],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(())
                })
            })
            .await
    }

    async fn load_invocation_journal(
        &self,
        run_id: &RunId,
    ) -> Result<Vec<InvocationJournalEntry>, StorageError> {
        let run_id = run_id.to_string();
        self.queue
            .submit(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT journal_id, session_id, run_id, attempt, generation, owner_kind, \
                         owner_subject, target, args_digest, args_summary, idempotency_key, \
                         phase, receipt_outcome, receipt_detail, dedup_id, dispatched, \
                         prepared_at_unix_ms, updated_at_unix_ms \
                         FROM invocation_journal WHERE run_id = ?1 ORDER BY rowid",
                    )
                    .map_err(migrations::map_rusqlite)?;
                let mut rows = stmt.query([&run_id]).map_err(migrations::map_rusqlite)?;
                let mut out = Vec::new();
                while let Some(row) = rows.next().map_err(migrations::map_rusqlite)? {
                    out.push(journal_entry_from_row(row)?);
                }
                Ok(out)
            })
            .await
    }

    async fn record_run_lineage(
        &self,
        ctx: &RunContext,
        lineage: lingxi_kernel::subagent::RunLineage,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        let (run_id, session_id, owner_kind, owner_subject, _principal_id, _generation) =
            ctx_facts(ctx);
        let parent_run_id = lineage.parent_run_id.as_ref().map(|p| p.to_string());
        let origin = lineage.origin.wire_name().to_string();
        let source_message_id = lineage.source_message_id.clone();
        let cause_id = lineage.cause_id.clone();
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    let run = load_run_row(conn, &run_id)?.ok_or_else(|| {
                        StorageError::InvalidRequest {
                            detail: format!("cannot record lineage: run {run_id} has no row"),
                        }
                    })?;
                    if run.session_id != session_id
                        || run.owner_kind != owner_kind
                        || run.owner_subject != owner_subject
                    {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} belongs to session {}/{}/{}; the lineage context \
                                 claims {session_id}/{owner_kind}/{owner_subject}",
                                run.session_id, run.owner_kind, run.owner_subject
                            ),
                        });
                    }
                    if parse_status(&run.status)?.is_terminal() {
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "cannot record lineage for run {run_id}: already terminal \
                                 ({}); lineage is a creation fact, never a post-mortem \
                                 annotation",
                                run.status
                            ),
                        });
                    }
                    if let Some(existing) = load_lineage_row(conn, &run_id)? {
                        // Lineage is IMMUTABLE: the identical four-part
                        // identity is an idempotent replay; a different one
                        // is a loud conflict (a run's parentage is never
                        // rewritten).
                        let identical = existing.parent_run_id == parent_run_id
                            && existing.origin == origin
                            && existing.source_message_id == source_message_id
                            && existing.cause_id == cause_id;
                        if identical {
                            return Ok(());
                        }
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} already holds lineage (parent {:?}, origin {}, \
                                 source {:?}, cause {:?}); refusing to rewrite it to (parent \
                                 {parent_run_id:?}, origin {origin}, source \
                                 {source_message_id:?}, cause {cause_id:?})",
                                existing.parent_run_id,
                                existing.origin,
                                existing.source_message_id,
                                existing.cause_id
                            ),
                        });
                    }
                    conn.execute(
                        "INSERT INTO run_lineage \
                         (run_id, parent_run_id, origin, source_message_id, cause_id, \
                          recorded_at_unix_ms) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![
                            run_id,
                            parent_run_id,
                            origin,
                            source_message_id,
                            cause_id,
                            now_unix_ms as i64
                        ],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(())
                })
            })
            .await
    }

    async fn load_run_lineage(
        &self,
        run_id: &RunId,
    ) -> Result<Option<lingxi_kernel::subagent::RunLineage>, StorageError> {
        let run_id = run_id.to_string();
        self.queue
            .submit(move |conn| {
                let Some(row) = load_lineage_row(conn, &run_id)? else {
                    return Ok(None);
                };
                let origin =
                    lingxi_kernel::subagent::RunOrigin::parse(&row.origin).ok_or_else(|| {
                        StorageError::Corrupted {
                            detail: format!(
                                "run {run_id} lineage origin {:?} is not in the known vocabulary",
                                row.origin
                            ),
                        }
                    })?;
                Ok(Some(lingxi_kernel::subagent::RunLineage {
                    parent_run_id: row.parent_run_id.map(lingxi_protocol::RunId::new),
                    origin,
                    source_message_id: row.source_message_id,
                    cause_id: row.cause_id,
                }))
            })
            .await
    }

    async fn record_model_call_usage(
        &self,
        record: lingxi_kernel::usage::ModelCallUsageRecord,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        // Provenance decomposed into ledger columns (usage_state + the
        // state's payload); the round trip is pinned by the storage tests.
        let (usage_state, missing_fields, estimate_basis) = match &record.usage {
            Some(usage) => match &usage.provenance {
                lingxi_kernel::usage::UsageProvenance::Reported => ("reported", None, None),
                lingxi_kernel::usage::UsageProvenance::Partial { missing } => {
                    ("partial", Some(missing.join(",")), None)
                }
                lingxi_kernel::usage::UsageProvenance::Estimated { basis } => {
                    ("estimated", None, Some(basis.clone()))
                }
            },
            // unknown vs invalid is carried by invalid_detail presence.
            None => ("unknown", None, None),
        };
        let usage_state = if record.invalid_detail.is_some() {
            "invalid"
        } else {
            usage_state
        }
        .to_string();
        // SQLite stores i64; the u64 token counts convert with an explicit
        // bound (a token count beyond i64::MAX is a corrupted fact, refused
        // loudly — never truncated).
        let token_i64 =
            |field: &'static str, value: Option<u64>| -> Result<Option<i64>, StorageError> {
                value
                    .map(|v| {
                        i64::try_from(v).map_err(|_| StorageError::InvalidRequest {
                            detail: format!(
                                "usage token field {field} ({v}) exceeds the storable integer \
                             range; the fact is refused rather than truncated"
                            ),
                        })
                    })
                    .transpose()
            };
        let (input_tokens, output_tokens, cache_read, cache_write, reasoning) = match &record.usage
        {
            Some(usage) => (
                token_i64("input_tokens", usage.input_tokens)?,
                token_i64("output_tokens", usage.output_tokens)?,
                token_i64("cache_read_tokens", usage.cache_read_tokens)?,
                token_i64("cache_write_tokens", usage.cache_write_tokens)?,
                token_i64("reasoning_tokens", usage.reasoning_tokens)?,
            ),
            None => (None, None, None, None, None),
        };
        let busy = self.queue.options().busy_timeout_ms;
        self.queue
            .submit(move |conn| {
                with_write_txn(conn, busy, |conn| {
                    // Idempotent replay / loud conflict — a call's
                    // accounting is never rewritten (same stance as the
                    // lineage rows).
                    if let Some(existing) = load_usage_row(conn, &record.model_call_id)? {
                        if existing == record {
                            return Ok(());
                        }
                        let existing_state = if existing.invalid_detail.is_some() {
                            "invalid"
                        } else {
                            existing
                                .usage
                                .as_ref()
                                .map(|u| u.provenance.state_name())
                                .unwrap_or("unknown")
                        };
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "model call {} already holds a usage row (state {}, \
                                 {:?}/{:?}); refusing to rewrite it to (state {}, \
                                 {:?}/{:?}) — a call's accounting is immutable",
                                record.model_call_id,
                                existing_state,
                                existing.usage.as_ref().and_then(|u| u.input_tokens),
                                existing.usage.as_ref().and_then(|u| u.output_tokens),
                                if record.invalid_detail.is_some() {
                                    "invalid"
                                } else {
                                    record
                                        .usage
                                        .as_ref()
                                        .map(|u| u.provenance.state_name())
                                        .unwrap_or("unknown")
                                },
                                input_tokens,
                                output_tokens,
                            ),
                        });
                    }
                    conn.execute(
                        "INSERT INTO model_call_usage \
                         (model_call_id, session_id, run_id, attempt, purpose, origin, \
                          parent_run_id, cause_ref, parent_tool_call_id, provider, model, \
                          protocol, usage_state, input_tokens, output_tokens, \
                          cache_read_tokens, cache_write_tokens, reasoning_tokens, \
                          missing_fields, estimate_basis, invalid_detail, \
                          transport_attempts, outcome, started_at_unix_ms, \
                          settled_at_unix_ms, emitted_tool_calls, cost_basis, \
                          recorded_at_unix_ms) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
                                 ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, \
                                 ?27, ?28)",
                        rusqlite::params![
                            record.model_call_id,
                            record.session_id,
                            record.run_id,
                            record.attempt,
                            record.purpose,
                            record.origin,
                            record.parent_run_id,
                            record.cause_ref,
                            record.parent_tool_call_id,
                            record.provider,
                            record.model,
                            record.protocol,
                            usage_state,
                            input_tokens,
                            output_tokens,
                            cache_read,
                            cache_write,
                            reasoning,
                            missing_fields,
                            estimate_basis,
                            record.invalid_detail,
                            // R05 RR1 F38: NULL = attempts unknown after a
                            // drop (the column is nullable since v7); a
                            // present count binds as i64.
                            record.transport_attempts.map(|attempts| attempts as i64),
                            record.outcome.wire_name(),
                            record.started_at_unix_ms.map(|v| v as i64),
                            record.settled_at_unix_ms.map(|v| v as i64),
                            if record.emitted_tool_calls.is_empty() {
                                None
                            } else {
                                Some(record.emitted_tool_calls.join(","))
                            },
                            record.cost_basis,
                            now_unix_ms as i64
                        ],
                    )
                    .map_err(migrations::map_rusqlite)?;
                    Ok(())
                })
            })
            .await
    }

    async fn query_model_call_usage(
        &self,
        query: lingxi_kernel::usage::ModelUsageQuery,
    ) -> Result<Vec<lingxi_kernel::usage::ModelCallUsageRecord>, StorageError> {
        self.queue
            .submit(move |conn| {
                // One statement per scope shape; the owner scope joins the
                // session owner (authorization isolation is part of the
                // read — a foreign principal's rows are never returned,
                // and session-less plane rows are internal-only).
                //
                // R05 RR1 F21: purpose/model/date-window filters append to
                // every scope shape (the taskbook's 按日期/类别/模型/会话
                // 筛选).
                let sql_base = "SELECT m.model_call_id, m.session_id, m.run_id, m.attempt, \
                     m.purpose, m.origin, m.parent_run_id, m.cause_ref, m.parent_tool_call_id, \
                     m.provider, m.model, m.protocol, m.usage_state, m.input_tokens, \
                     m.output_tokens, m.cache_read_tokens, m.cache_write_tokens, \
                     m.reasoning_tokens, m.missing_fields, m.estimate_basis, m.invalid_detail, \
                     m.transport_attempts, m.cost_basis, m.recorded_at_unix_ms, m.outcome, \
                     m.started_at_unix_ms, m.settled_at_unix_ms, m.emitted_tool_calls \
                     FROM model_call_usage m";
                let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                let mut sql = match (&query.owner_user_id, &query.session_id, &query.run_id) {
                    (Some(owner), session, run) => {
                        let mut sql = format!(
                            "{sql_base} JOIN sessions s ON s.session_id = m.session_id \
                             WHERE s.owner_user_id = ?{}",
                            params.len() + 1
                        );
                        params.push(Box::new(owner.clone()));
                        if let Some(session) = session {
                            sql.push_str(&format!(" AND m.session_id = ?{}", params.len() + 1));
                            params.push(Box::new(session.clone()));
                        }
                        if let Some(run) = run {
                            sql.push_str(&format!(" AND m.run_id = ?{}", params.len() + 1));
                            params.push(Box::new(run.clone()));
                        }
                        sql
                    }
                    (None, Some(session), run) => {
                        let mut sql =
                            format!("{sql_base} WHERE m.session_id = ?{}", params.len() + 1);
                        params.push(Box::new(session.clone()));
                        if let Some(run) = run {
                            sql.push_str(&format!(" AND m.run_id = ?{}", params.len() + 1));
                            params.push(Box::new(run.clone()));
                        }
                        sql
                    }
                    (None, None, Some(run)) => {
                        let sql = format!("{sql_base} WHERE m.run_id = ?{}", params.len() + 1);
                        params.push(Box::new(run.clone()));
                        sql
                    }
                    (None, None, None) => sql_base.to_string(),
                };
                // The F21 filters append to whichever shape above; the
                // first one opens its own WHERE when the scope was empty.
                let append = |sql: &mut String,
                              params: &mut Vec<Box<dyn rusqlite::ToSql>>,
                              condition: String,
                              value: Box<dyn rusqlite::ToSql>| {
                    if sql.contains(" WHERE ") {
                        sql.push_str(" AND ");
                    } else {
                        sql.push_str(" WHERE ");
                    }
                    sql.push_str(&condition);
                    params.push(value);
                };
                if let Some(purpose) = &query.purpose {
                    let index = params.len() + 1;
                    append(
                        &mut sql,
                        &mut params,
                        format!("m.purpose = ?{index}"),
                        Box::new(purpose.clone()),
                    );
                }
                if let Some(model) = &query.model {
                    let index = params.len() + 1;
                    append(
                        &mut sql,
                        &mut params,
                        format!("m.model = ?{index}"),
                        Box::new(model.clone()),
                    );
                }
                if let Some(from) = query.recorded_from_unix_ms {
                    let index = params.len() + 1;
                    append(
                        &mut sql,
                        &mut params,
                        format!("m.recorded_at_unix_ms >= ?{index}"),
                        Box::new(from as i64),
                    );
                }
                if let Some(to) = query.recorded_to_unix_ms {
                    let index = params.len() + 1;
                    append(
                        &mut sql,
                        &mut params,
                        format!("m.recorded_at_unix_ms <= ?{index}"),
                        Box::new(to as i64),
                    );
                }
                sql.push_str(" ORDER BY m.rowid");
                let mut stmt = conn.prepare(&sql).map_err(migrations::map_rusqlite)?;
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let mut rows = stmt
                    .query(param_refs.as_slice())
                    .map_err(migrations::map_rusqlite)?;
                let mut out = Vec::new();
                while let Some(row) = rows.next().map_err(migrations::map_rusqlite)? {
                    out.push(usage_row_to_record(row)?);
                }
                Ok(out)
            })
            .await
    }
}

/// Loads one ledger row by call id (None when absent).
fn load_usage_row(
    conn: &rusqlite::Connection,
    model_call_id: &str,
) -> Result<Option<lingxi_kernel::usage::ModelCallUsageRecord>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT model_call_id, session_id, run_id, attempt, purpose, origin, \
             parent_run_id, cause_ref, parent_tool_call_id, provider, model, protocol, \
             usage_state, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
             reasoning_tokens, missing_fields, estimate_basis, invalid_detail, \
             transport_attempts, cost_basis, recorded_at_unix_ms, outcome, \
             started_at_unix_ms, settled_at_unix_ms, emitted_tool_calls \
             FROM model_call_usage WHERE model_call_id = ?1",
        )
        .map_err(migrations::map_rusqlite)?;
    let mut rows = stmt
        .query([model_call_id])
        .map_err(migrations::map_rusqlite)?;
    match rows.next().map_err(migrations::map_rusqlite)? {
        Some(row) => Ok(Some(usage_row_to_record(row)?)),
        None => Ok(None),
    }
}

/// Rebuilds the record from a ledger row. A `usage_state` outside the
/// vocabulary is [`StorageError::Corrupted`] — never a guess.
fn usage_row_to_record(
    row: &rusqlite::Row<'_>,
) -> Result<lingxi_kernel::usage::ModelCallUsageRecord, StorageError> {
    use lingxi_kernel::usage::{ModelCallUsage, UsageProvenance};
    // SQLite returns i64; the columns are u64 facts by contract (a negative
    // stored value is corruption, never a wrap-around guess).
    let token_u64 = |index: usize, field: &'static str| -> Result<Option<u64>, StorageError> {
        row.get::<_, Option<i64>>(index)
            .map_err(migrations::map_rusqlite)?
            .map(|value| {
                u64::try_from(value).map_err(|_| StorageError::Corrupted {
                    detail: format!("usage row {field} is negative ({value})"),
                })
            })
            .transpose()
    };
    // Column order of the SELECT statements above (RR1 F21 added
    // parent_tool_call_id at 8 and outcome/started/settled/emitted at
    // 24..=27 — every later index shifted).
    let invalid_detail: Option<String> = row.get(20).map_err(migrations::map_rusqlite)?;
    let missing_fields: Option<String> = row.get(18).map_err(migrations::map_rusqlite)?;
    let estimate_basis: Option<String> = row.get(19).map_err(migrations::map_rusqlite)?;
    let usage_state: String = row.get(12).map_err(migrations::map_rusqlite)?;
    let usage = if invalid_detail.is_some() {
        if usage_state != "invalid" {
            return Err(StorageError::Corrupted {
                detail: format!(
                    "usage row holds invalid_detail but state {usage_state:?}; the state \
                     vocabulary is never mixed"
                ),
            });
        }
        None
    } else {
        match usage_state.as_str() {
            "unknown" => None,
            "reported" => Some(ModelCallUsage {
                input_tokens: token_u64(13, "input_tokens")?,
                output_tokens: token_u64(14, "output_tokens")?,
                cache_read_tokens: token_u64(15, "cache_read_tokens")?,
                cache_write_tokens: token_u64(16, "cache_write_tokens")?,
                reasoning_tokens: token_u64(17, "reasoning_tokens")?,
                provenance: UsageProvenance::Reported,
            }),
            "partial" => {
                let missing = match missing_fields.as_deref() {
                    None => Vec::new(),
                    Some(joined) => joined
                        .split(',')
                        .map(|field| match field.trim() {
                            "input_tokens" => Some("input_tokens"),
                            "output_tokens" => Some("output_tokens"),
                            _ => None,
                        })
                        .collect::<Option<Vec<&'static str>>>()
                        .ok_or_else(|| StorageError::Corrupted {
                            detail: format!(
                                "partial usage row carries unknown missing-fields \
                                 {missing_fields:?}"
                            ),
                        })?,
                };
                Some(ModelCallUsage {
                    input_tokens: token_u64(13, "input_tokens")?,
                    output_tokens: token_u64(14, "output_tokens")?,
                    cache_read_tokens: token_u64(15, "cache_read_tokens")?,
                    cache_write_tokens: token_u64(16, "cache_write_tokens")?,
                    reasoning_tokens: token_u64(17, "reasoning_tokens")?,
                    provenance: UsageProvenance::Partial { missing },
                })
            }
            "estimated" => Some(ModelCallUsage {
                input_tokens: token_u64(13, "input_tokens")?,
                output_tokens: token_u64(14, "output_tokens")?,
                cache_read_tokens: token_u64(15, "cache_read_tokens")?,
                cache_write_tokens: token_u64(16, "cache_write_tokens")?,
                reasoning_tokens: token_u64(17, "reasoning_tokens")?,
                provenance: UsageProvenance::Estimated {
                    basis: estimate_basis.clone().unwrap_or_default(),
                },
            }),
            other => {
                return Err(StorageError::Corrupted {
                    detail: format!(
                        "usage row state {other:?} is not in the ledger vocabulary \
                         (reported/partial/estimated/unknown/invalid)"
                    ),
                });
            }
        }
    };
    let outcome_name: String = row.get(24).map_err(migrations::map_rusqlite)?;
    let outcome = lingxi_kernel::usage::CallOutcome::parse(&outcome_name).ok_or_else(|| {
        StorageError::Corrupted {
            detail: format!(
                "usage row outcome {outcome_name:?} is not in the vocabulary \
                 (succeeded/failed/cancelled/unknown)"
            ),
        }
    })?;
    let timestamp_u64 = |index: usize, field: &'static str| -> Result<Option<u64>, StorageError> {
        row.get::<_, Option<i64>>(index)
            .map_err(migrations::map_rusqlite)?
            .map(|value| {
                u64::try_from(value).map_err(|_| StorageError::Corrupted {
                    detail: format!("usage row {field} is negative ({value})"),
                })
            })
            .transpose()
    };
    let emitted_tool_calls: Option<String> = row.get(27).map_err(migrations::map_rusqlite)?;
    Ok(lingxi_kernel::usage::ModelCallUsageRecord {
        session_id: row.get(1).map_err(migrations::map_rusqlite)?,
        run_id: row.get(2).map_err(migrations::map_rusqlite)?,
        attempt: row.get(3).map_err(migrations::map_rusqlite)?,
        model_call_id: row.get(0).map_err(migrations::map_rusqlite)?,
        purpose: row.get(4).map_err(migrations::map_rusqlite)?,
        origin: row.get(5).map_err(migrations::map_rusqlite)?,
        parent_run_id: row.get(6).map_err(migrations::map_rusqlite)?,
        cause_ref: row.get(7).map_err(migrations::map_rusqlite)?,
        parent_tool_call_id: row.get(8).map_err(migrations::map_rusqlite)?,
        provider: row.get(9).map_err(migrations::map_rusqlite)?,
        model: row.get(10).map_err(migrations::map_rusqlite)?,
        protocol: row.get(11).map_err(migrations::map_rusqlite)?,
        usage,
        invalid_detail,
        transport_attempts: row
            .get::<_, Option<i64>>(21)
            .map_err(migrations::map_rusqlite)?
            .map(|value| {
                u32::try_from(value).map_err(|_| StorageError::Corrupted {
                    detail: format!("usage row transport_attempts is negative ({value})"),
                })
            })
            .transpose()?,
        outcome,
        started_at_unix_ms: timestamp_u64(25, "started_at_unix_ms")?,
        settled_at_unix_ms: timestamp_u64(26, "settled_at_unix_ms")?,
        emitted_tool_calls: emitted_tool_calls
            .map(|joined| {
                joined
                    .split(',')
                    .map(str::to_string)
                    .filter(|id| !id.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        cost_basis: row.get(22).map_err(migrations::map_rusqlite)?,
    })
}

/// One `run_lineage` row (R03-T06).
struct LineageRow {
    parent_run_id: Option<String>,
    origin: String,
    source_message_id: Option<String>,
    cause_id: Option<String>,
}

fn load_lineage_row(
    conn: &rusqlite::Connection,
    run_id: &str,
) -> Result<Option<LineageRow>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT parent_run_id, origin, source_message_id, cause_id FROM run_lineage \
             WHERE run_id = ?1",
        )
        .map_err(migrations::map_rusqlite)?;
    let mut rows = stmt.query([run_id]).map_err(migrations::map_rusqlite)?;
    match rows.next().map_err(migrations::map_rusqlite)? {
        Some(row) => Ok(Some(LineageRow {
            parent_run_id: row.get(0).map_err(migrations::map_rusqlite)?,
            origin: row.get(1).map_err(migrations::map_rusqlite)?,
            source_message_id: row.get(2).map_err(migrations::map_rusqlite)?,
            cause_id: row.get(3).map_err(migrations::map_rusqlite)?,
        })),
        None => Ok(None),
    }
}

// ── EventStorePort (R02-T05 read half) ──────────────────────────────────────

/// One atomic resume read of a stream (R02 stage-repair R1 / F02): the
/// retention floor, the durable head and the first `limit` events after a
/// cursor, all from ONE submission to the single-writer queue. Because the
/// queue serializes this read against any `purge_events_before`, the
/// truncation verdict and the page can never disagree — the pre-fix
/// separate floor/page submissions let a purge commit between them and a
/// resumed subscription silently skipped events without a snapshot
/// directive.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamResumeSlice {
    /// Lowest retained seq (`None` when the stream has no retained events).
    pub floor: Option<Seq>,
    /// Highest committed seq (`None` when the stream has no events).
    pub head: Option<Seq>,
    /// Retained events with `seq > after_seq`, ascending, at most `limit`.
    pub events: Vec<EventEnvelope>,
}

/// Negative seq columns are corruption (loud), never a sign flip.
fn seq_from_i64(value: i64, which: &str) -> Result<Seq, StorageError> {
    u64::try_from(value)
        .map(Seq::new)
        .map_err(|_| StorageError::Corrupted {
            detail: format!("key_events {which} value {value} is negative"),
        })
}

/// Parses the allocator sequence out of a stored run id IF AND ONLY IF the
/// id was minted by this allocator — the exact shape
/// `run_{millis:016x}_{seq:06x}`: the literal `run_` prefix, a millis half
/// of exactly 16 LOWERCASE hex digits (a `u64` millisecond timestamp
/// formatted `:016x` is never wider), a single `_`, and a seq half of
/// 6..=16 lowercase hex digits (`:06x` is a MINIMUM width). Anything else
/// — legacy, imported or opaque ids such as
/// `opaque_existing_ffffffffffffffff` — yields `None` and MUST NOT seed
/// the sequence: the previous tail-after-last-`_` heuristic parsed a
/// foreign id's tail as `u64::MAX`, seeded the allocator to the top of the
/// space, and the next allocation panicked on the overflowing `+ 1`
/// (R02 stage-repair R5 / F02). Unrecognized ids stay covered by the
/// seed's `COUNT(*)` lower bound: their rows count as consumed numbers
/// without ever contributing a sequence value.
fn run_id_sequence(run_id: &str) -> Option<u64> {
    let rest = run_id.strip_prefix("run_")?;
    let (millis, seq) = rest.split_once('_')?;
    if millis.contains('_') || seq.contains('_') {
        return None;
    }
    if millis.len() != 16 || !(6..=16).contains(&seq.len()) {
        return None;
    }
    if !is_lower_hex(millis) || !is_lower_hex(seq) {
        return None;
    }
    u64::from_str_radix(seq, 16).ok()
}

/// Lowercase hex only (`:x` never emits uppercase); the strict half of the
/// minted-format check above.
fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

/// Column order of the `key_events` SELECT used by
/// [`EventStorePort::stream_events_after`].
type KeyEventRow = (
    String,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
);

/// Rebuilds the durable envelope of one key_events row, cross-checking the
/// stored `event_type` column against the payload tag (a disagreement is
/// corruption, not a guess about which side is authoritative — mirrors the
/// envelope invariant of `lingxi.wire`).
fn envelope_from_parts(
    (event_id, stream_id, seq, session_id, run_id, attempt, stored_event_type, payload_json): KeyEventRow,
) -> Result<EventEnvelope, StorageError> {
    let payload: EventPayload =
        serde_json::from_str(&payload_json).map_err(|err| StorageError::Corrupted {
            detail: format!("key_events payload for {event_id} is not valid JSON: {err}"),
        })?;
    let payload_tag = payload.event_type().to_string();
    if payload_tag != stored_event_type {
        return Err(StorageError::Corrupted {
            detail: format!(
                "key_events row {event_id} (stream {stream_id} seq {seq}) stores \
                 event_type {stored_event_type:?} but its payload tag is {payload_tag:?}; \
                 refusing to guess which is authoritative"
            ),
        });
    }
    Ok(EventEnvelope::new(
        EventId::new(event_id),
        StreamId::new(stream_id),
        Seq::new(u64::try_from(seq).map_err(|_| StorageError::Corrupted {
            detail: format!("key_events seq {seq} is negative"),
        })?),
        SessionId::new(session_id),
        run_id.map(lingxi_protocol::RunId::new),
        attempt.map(AttemptId::new),
        payload,
    ))
}

impl lingxi_kernel::ports::EventStorePort for RunDatabase {
    async fn stream_head(&self, stream_id: &str) -> Result<Option<Seq>, StorageError> {
        let stream_id = stream_id.to_string();
        self.queue
            .submit(move |conn| {
                let head: Option<i64> = conn
                    .query_row(
                        "SELECT MAX(seq) FROM key_events WHERE stream_id = ?1",
                        [&stream_id],
                        |row| row.get(0),
                    )
                    .map_err(migrations::map_rusqlite)?;
                head.map(|h| seq_from_i64(h, "MAX(seq)")).transpose()
            })
            .await
    }

    async fn stream_floor(&self, stream_id: &str) -> Result<Option<Seq>, StorageError> {
        let stream_id = stream_id.to_string();
        self.queue
            .submit(move |conn| {
                let floor: Option<i64> = conn
                    .query_row(
                        "SELECT MIN(seq) FROM key_events WHERE stream_id = ?1",
                        [&stream_id],
                        |row| row.get(0),
                    )
                    .map_err(migrations::map_rusqlite)?;
                floor.map(|f| seq_from_i64(f, "MIN(seq)")).transpose()
            })
            .await
    }

    async fn stream_events_after(
        &self,
        stream_id: &str,
        after_seq: Seq,
        limit: u32,
    ) -> Result<Vec<EventEnvelope>, StorageError> {
        let stream_id = stream_id.to_string();
        let after = after_seq.value() as i64;
        self.queue
            .submit(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT event_id, stream_id, seq, session_id, run_id, attempt, \
                         event_type, payload_json FROM key_events \
                         WHERE stream_id = ?1 AND seq > ?2 ORDER BY seq ASC LIMIT ?3",
                    )
                    .map_err(migrations::map_rusqlite)?;
                let rows = stmt
                    .query_map(rusqlite::params![stream_id, after, limit as i64], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    })
                    .map_err(migrations::map_rusqlite)?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(envelope_from_parts(row.map_err(migrations::map_rusqlite)?)?);
                }
                Ok(out)
            })
            .await
    }

    async fn purge_events_before(
        &self,
        stream_id: &str,
        before_seq: Seq,
    ) -> Result<u64, StorageError> {
        RunDatabase::purge_events_before(self, stream_id, before_seq).await
    }
}

#[cfg(test)]
mod run_id_sequence_tests {
    use super::run_id_sequence;

    #[test]
    fn recognizes_only_ids_this_allocator_mints() {
        // Realistic minted shapes (seq padding 6; wider only past 0x100000).
        assert_eq!(run_id_sequence("run_0000017f3b0e0000_000001"), Some(1));
        assert_eq!(run_id_sequence("run_0000017f3b0e0000_0000ff"), Some(0xff));
        assert_eq!(
            run_id_sequence("run_0000017f3b0e0000_fffffffffffffffe"),
            Some(u64::MAX - 1)
        );
        assert_eq!(
            run_id_sequence("run_0000017f3b0e0000_ffffffffffffffff"),
            Some(u64::MAX)
        );
        // Full-width millis (a u64::MAX clock) is still exactly 16 digits.
        assert_eq!(run_id_sequence("run_ffffffffffffffff_000001"), Some(1));
    }

    #[test]
    fn foreign_and_malformed_shapes_never_seed() {
        // The R5-F02 shape: a legal opaque id whose tail parses as u64::MAX.
        assert_eq!(run_id_sequence("opaque_existing_ffffffffffffffff"), None);
        // Legacy/imported tails that are NOT this allocator's shape.
        assert_eq!(run_id_sequence("legacy_ffffffffffffffff"), None);
        assert_eq!(run_id_sequence("run-0000017f3b0e0000-000001"), None);
        assert_eq!(run_id_sequence("run_0000017f3b0e0000_000001_extra"), None);
        assert_eq!(run_id_sequence("run__000001"), None);
        // Wrong half widths (minted millis is EXACTLY 16, seq AT LEAST 6).
        assert_eq!(run_id_sequence("run_fff_000001"), None);
        assert_eq!(run_id_sequence("run_0000017f3b0e00_000001"), None);
        assert_eq!(run_id_sequence("run_0000017f3b0e0000_0001"), None);
        assert_eq!(
            run_id_sequence("run_0000017f3b0e0000_0000010000000000000000"),
            None
        );
        // Uppercase hex / non-hex tails are not minted by `:x`.
        assert_eq!(run_id_sequence("run_0000017f3b0e0000_0000AB"), None);
        assert_eq!(run_id_sequence("run_0000017f3b0e0000_0000zz"), None);
        assert_eq!(run_id_sequence(""), None);
    }
}
