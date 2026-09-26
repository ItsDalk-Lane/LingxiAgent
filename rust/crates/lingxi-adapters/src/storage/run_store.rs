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

use std::path::Path;
use std::sync::Arc;

use lingxi_kernel::ports::{
    CommittedOutcome, KeyEvent, RunOutcome, RunRecord, StorageError, StoragePort,
};
use lingxi_kernel::{Principal, RunContext, RunStateMachine};
use lingxi_protocol::canon;
use lingxi_protocol::{
    AttemptId, EventEnvelope, EventId, EventPayload, FinalMessageCommittedPayload,
    KnownEventPayload, RunStateChangedPayload, RunStatus, Seq, SessionId, StreamId,
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

/// The new run/message database: bounded queue + dedicated SQLite worker.
/// Cloning shares the one queue/worker (there is exactly one per database
/// file per process; the composition root owns the original).
#[derive(Clone)]
pub struct RunDatabase {
    queue: Arc<DbQueue>,
}

impl RunDatabase {
    /// Opens (creating if needed) the run database, runs the open-time
    /// migration pass and verifies receipts. Errors are loud: a newer,
    /// tampered or unreadable database refuses to open.
    pub async fn open(path: &Path, options: StoreOptions) -> Result<Self, StorageError> {
        let queue = Arc::new(DbQueue::open(path, options)?);
        let db = Self { queue };
        let applied_by = format!("lingxi-adapters {}", env!("CARGO_PKG_VERSION"));
        let outcome: MigrationOutcome = db
            .queue
            .submit(move |conn| migrations::apply_all(conn, &applied_by, now_for_migrations()))
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
        Ok(db)
    }

    /// Path of the underlying database file.
    pub fn db_path(&self) -> &Path {
        self.queue.db_path()
    }

    /// Queue options (bound/busy/checkpoint knobs actually in force).
    pub fn options(&self) -> &StoreOptions {
        self.queue.options()
    }

    /// Graceful close: FIFO-drains queued work, checkpoints the WAL
    /// (TRUNCATE) and joins the worker. See [`DbQueue::close`].
    pub async fn close(&self) -> Result<(), StorageError> {
        self.queue.close().await
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

    /// Lists every session row (ownership filtering stays in the service's
    /// domain logic; the row set is tiny).
    pub async fn list_sessions(&self) -> Result<Vec<SessionRow>, StorageError> {
        self.queue
            .submit(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT session_id, agent_id, owner_user_id, title, \
                         created_at_unix_ms FROM sessions ORDER BY created_at_unix_ms, session_id",
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

/// BEGIN IMMEDIATE + f + COMMIT/ROLLBACK with busy-error decoration.
fn with_write_txn<T>(
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
                    if stored_status.is_terminal() {
                        // Idempotent when identical, conflict otherwise.
                        if stored_status == parse_status(&target_status)? {
                            let events = read_run_events(conn, &run_id)?;
                            return Ok(CommittedOutcome {
                                newly_committed: false,
                                events,
                            });
                        }
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "run {run_id} is already terminal as {} (recorded reason \
                                 {:?}); refusing to overwrite with {target_status}",
                                run.status, run.terminal_reason
                            ),
                        });
                    }
                    // The kernel state machine is the single transition
                    // authority, enforced again at the storage boundary.
                    let to = parse_status(&target_status)?;
                    RunStateMachine::transition(stored_status, to).map_err(|err| {
                        StorageError::InvalidRequest {
                            detail: format!(
                                "illegal transition {} -> {}: {}",
                                stored_status.wire_name(),
                                to.wire_name(),
                                err.reason
                            ),
                        }
                    })?;

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
                        conn.execute(
                            "INSERT INTO messages \
                             (message_id, session_id, run_id, role, content_json, \
                              model_call_id, committed_at_unix_ms, seq) \
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                            rusqlite::params![
                                format!("{run_id}-final"),
                                session_id,
                                run_id,
                                message.role,
                                content_json,
                                message.model_call_id.as_ref().map(|id| id.to_string()),
                                now_unix_ms as i64,
                                message_seq
                            ],
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
}

// ── EventStorePort (R02-T05 read half) ──────────────────────────────────────

/// Negative seq columns are corruption (loud), never a sign flip.
fn seq_from_i64(value: i64, which: &str) -> Result<Seq, StorageError> {
    u64::try_from(value)
        .map(Seq::new)
        .map_err(|_| StorageError::Corrupted {
            detail: format!("key_events {which} value {value} is negative"),
        })
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
