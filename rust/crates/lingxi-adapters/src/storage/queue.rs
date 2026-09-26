//! Bounded DB work queue: ALL synchronous SQLite access of the run/message
//! database runs on ONE dedicated std::thread owning the connection
//! (R02-T04 step 2).
//!
//! Contract:
//! - Callers submit closures through a **bounded** tokio mpsc channel. The
//!   bound is a hard backpressure edge: [`DbQueue::try_submit`] surfaces
//!   [`StorageError::QueueFull`] instead of buffering unboundedly, and
//!   [`DbQueue::submit`] awaits capacity. Nothing is ever dropped silently.
//! - Connection pragmas are negotiated ON the worker before readiness is
//!   announced, so a broken path/permission is reported to the opener:
//!   WAL journal mode (verified, not assumed), `busy_timeout`,
//!   `wal_autocheckpoint`, `foreign_keys=ON`, `synchronous=FULL`
//!   (WAL + FULL keeps every acknowledged commit durable across power
//!   loss, not just process crash).
//! - Disk-full and real IO failures map through
//!   [`crate::storage::migrations::map_rusqlite`] into explicit
//!   [`StorageError`] variants — the caller always learns the outcome.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::JoinHandle;

use lingxi_kernel::ports::StorageError;
use tokio::sync::{mpsc, oneshot};

use super::migrations;

/// Tunables of the store/queue. Defaults are the production values; tests
/// inject smaller ones instead of sleeping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreOptions {
    /// Hard bound of pending jobs. Full = explicit backpressure.
    pub queue_capacity: usize,
    /// SQLite busy timeout in milliseconds (write-lock contention).
    pub busy_timeout_ms: u64,
    /// WAL autocheckpoint threshold in pages.
    pub checkpoint_pages: u64,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            queue_capacity: 64,
            busy_timeout_ms: 5_000,
            checkpoint_pages: 1_000,
        }
    }
}

impl StoreOptions {
    /// Validates the knobs that must never be degenerate.
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.queue_capacity == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "queue_capacity must be >= 1 (0 would be an unusable \
                         store, not infinite capacity)"
                    .to_string(),
            });
        }
        if self.checkpoint_pages == 0 {
            // 0 disables autocheckpoint entirely; that is a deliberate
            // setting in some embedded setups, but for this store it would
            // let the WAL grow without bound — reject it loudly.
            return Err(StorageError::InvalidRequest {
                detail: "checkpoint_pages must be >= 1 (0 would disable \
                         autocheckpoint and grow the WAL unboundedly)"
                    .to_string(),
            });
        }
        Ok(())
    }
}

type WorkFn = Box<dyn FnOnce(&rusqlite::Connection) -> Box<dyn Any + Send> + Send>;
type ReplyPayload = Result<Box<dyn Any + Send>, StorageError>;

struct Job {
    work: WorkFn,
    reply: oneshot::Sender<ReplyPayload>,
}

/// The bounded queue in front of one SQLite database file.
pub struct DbQueue {
    tx: Mutex<Option<mpsc::Sender<Job>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    db_path: PathBuf,
    options: StoreOptions,
}

impl DbQueue {
    /// Opens the database (creating the file if needed) on a dedicated
    /// worker thread and negotiates all pragmas BEFORE reporting readiness.
    /// A failure here leaves no worker thread behind.
    pub fn open(path: &Path, options: StoreOptions) -> Result<Self, StorageError> {
        options.validate()?;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), StorageError>>();
        let (tx, rx) = mpsc::channel::<Job>(options.queue_capacity);
        let mut rx = rx;
        let worker_path = path.to_path_buf();
        let busy = options.busy_timeout_ms;
        let pages = options.checkpoint_pages;
        let handle = std::thread::Builder::new()
            .name("lingxi-db-worker".to_string())
            .spawn(move || {
                let conn = match open_and_pragma(&worker_path, busy, pages) {
                    Ok((conn, journal)) => {
                        tracing::debug!(
                            db = %worker_path.display(),
                            journal_mode = %journal,
                            "db worker connection ready"
                        );
                        conn
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                if ready_tx.send(Ok(())).is_err() {
                    // Opener vanished: nothing to serve.
                    return;
                }
                while let Some(job) = rx.blocking_recv() {
                    let result: ReplyPayload =
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            (job.work)(&conn)
                        })) {
                            Ok(boxed) => Ok(boxed),
                            Err(panic) => Err(StorageError::Internal {
                                detail: format!("db worker job panicked: {panic:?}"),
                            }),
                        };
                    if job.reply.send(result).is_err() {
                        tracing::warn!("db job caller dropped before reply");
                    }
                }
                // Sender side gone: the connection drops here; SQLite's own
                // close performs the final WAL cleanup when it is the last
                // connection.
            })
            .map_err(|source| StorageError::Internal {
                detail: format!("cannot spawn db worker thread: {source}"),
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx: Mutex::new(Some(tx)),
                worker: Mutex::new(Some(handle)),
                db_path: path.to_path_buf(),
                options,
            }),
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(err)
            }
            Err(_) => {
                let _ = handle.join();
                Err(StorageError::Internal {
                    detail: "db worker died before signalling readiness".to_string(),
                })
            }
        }
    }

    /// The database file path (diagnostics).
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Options this queue was opened with.
    pub fn options(&self) -> &StoreOptions {
        &self.options
    }

    /// Submits `work`, awaiting queue capacity (backpressure without loss).
    pub async fn submit<R: Send + 'static>(
        &self,
        work: impl FnOnce(&rusqlite::Connection) -> Result<R, StorageError> + Send + 'static,
    ) -> Result<R, StorageError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let job = Job {
            work: Box::new(move |conn| Box::new(work(conn))),
            reply: reply_tx,
        };
        self.send_job(job).await?;
        match reply_rx.await {
            Ok(result) => unwrap_typed(result),
            Err(_) => Err(StorageError::QueueClosed),
        }
    }

    /// Submits without waiting; a full queue is an immediate
    /// [`StorageError::QueueFull`] and the job is NOT executed.
    pub fn try_submit<R: Send + 'static>(
        &self,
        work: impl FnOnce(&rusqlite::Connection) -> Result<R, StorageError> + Send + 'static,
    ) -> Result<QueueReply<R>, StorageError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let job = Job {
            work: Box::new(move |conn| Box::new(work(conn))),
            reply: reply_tx,
        };
        {
            let guard = self
                .tx
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(sender) = guard.as_ref() else {
                return Err(StorageError::QueueClosed);
            };
            match sender.try_send(job) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => return Err(StorageError::QueueFull),
                Err(mpsc::error::TrySendError::Closed(_)) => return Err(StorageError::QueueClosed),
            }
        }
        Ok(QueueReply {
            rx: reply_rx,
            _marker: std::marker::PhantomData,
        })
    }

    async fn send_job(&self, job: Job) -> Result<(), StorageError> {
        let sender = {
            let guard = self
                .tx
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match guard.as_ref() {
                Some(sender) => sender.clone(),
                None => return Err(StorageError::QueueClosed),
            }
        };
        if sender.send(job).await.is_err() {
            return Err(StorageError::QueueClosed);
        }
        Ok(())
    }

    /// Graceful close: drains in-flight jobs, checkpoints the WAL
    /// (TRUNCATE), then joins the worker. A failed checkpoint is returned
    /// as an error (documented, not masked).
    pub async fn close(&self) -> Result<(), StorageError> {
        // 1. Ask the worker to checkpoint; the job itself runs only after
        //    everything queued ahead of it has finished (FIFO channel).
        let checkpoint = self
            .submit(|conn| {
                checkpoint_truncate(conn)?;
                Ok(())
            })
            .await;
        // 2. Drop the sender so the worker loop ends, then join.
        let handle = {
            let mut tx_guard = self
                .tx
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut worker_guard = self
                .worker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *tx_guard = None;
            worker_guard.take()
        };
        if let Some(handle) = handle {
            if handle.join().is_err() {
                return Err(StorageError::Internal {
                    detail: "db worker thread panicked".to_string(),
                });
            }
        }
        checkpoint
    }
}

impl Drop for DbQueue {
    fn drop(&mut self) {
        // Best-effort teardown when close() was not called: drop the sender
        // and join. No checkpoint is attempted — a drop-path teardown is
        // crash-equivalent and SQLite WAL recovery applies on next open.
        *self
            .tx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        if let Some(handle) = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = handle.join();
        }
    }
}

/// Awaitable reply of a [`DbQueue::try_submit`] call.
pub struct QueueReply<R> {
    rx: oneshot::Receiver<ReplyPayload>,
    _marker: std::marker::PhantomData<fn() -> R>,
}

impl<R: Send + 'static> QueueReply<R> {
    pub async fn wait(self) -> Result<R, StorageError> {
        match self.rx.await {
            Ok(result) => unwrap_typed(result),
            Err(_) => Err(StorageError::QueueClosed),
        }
    }
}

fn unwrap_typed<R: Send + 'static>(result: ReplyPayload) -> Result<R, StorageError> {
    match result {
        // The boxed payload holds the job's `Result<R, StorageError>`
        // (the worker wraps it once; catch_unwind adds the outer layer).
        Ok(boxed) => match boxed.downcast::<Result<R, StorageError>>() {
            Ok(typed) => *typed,
            Err(_) => Err(StorageError::Internal {
                detail: "db job reply type mismatch".to_string(),
            }),
        },
        Err(err) => Err(err),
    }
}

/// Opens the connection and negotiates every pragma, returning the
/// connection plus the ACTUAL journal mode (verified, never assumed).
fn open_and_pragma(
    path: &Path,
    busy_timeout_ms: u64,
    checkpoint_pages: u64,
) -> Result<(rusqlite::Connection, String), StorageError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.is_dir() {
            return Err(StorageError::Io {
                detail: format!(
                    "database directory {} does not exist or is not a directory",
                    parent.display()
                ),
            });
        }
    }
    let conn = rusqlite::Connection::open(path).map_err(migrations::map_rusqlite)?;
    conn.pragma_update(None, "busy_timeout", busy_timeout_ms as i64)
        .map_err(migrations::map_rusqlite)?;
    let journal: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(migrations::map_rusqlite)?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(StorageError::Io {
            detail: format!(
                "WAL journal mode was refused (negotiated {journal:?}); this \
                 store requires WAL"
            ),
        });
    }
    conn.pragma_update(None, "wal_autocheckpoint", checkpoint_pages as i64)
        .map_err(migrations::map_rusqlite)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(migrations::map_rusqlite)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(migrations::map_rusqlite)?;
    // R02-T06 (T04 REVIEW F04 follow-up): defense in depth — the files live
    // in a 0700 directory, but the files themselves are also tightened to
    // owner-only so a future directory-permission regression cannot expose
    // them. A failure here is a loud open error (the store must never run
    // with looser-than-intended file semantics silently).
    tighten_db_file_permissions(path)?;
    Ok((conn, journal))
}

/// Tightens the database file and its present WAL sidecars to 0600 on unix.
/// The main file is strict (failure = open failure); sidecars may not exist
/// yet on first open, so a missing sidecar is skipped and a failing sidecar
/// chmod is logged loudly (the sidecars are recreated per-session by SQLite
/// and are re-tightened on every subsequent open).
fn tighten_db_file_permissions(path: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tighten = |p: &Path| -> std::io::Result<bool> {
            let Ok(meta) = std::fs::metadata(p) else {
                return Ok(false); // sidecar not created yet on a first open
            };
            let mut perms = meta.permissions();
            if perms.mode() & 0o777 == 0o600 {
                return Ok(false);
            }
            perms.set_mode(0o600);
            std::fs::set_permissions(p, perms)?;
            Ok(true)
        };
        tighten(path).map_err(|source| StorageError::Io {
            detail: format!(
                "cannot tighten database file permissions to 0600 ({}): {source}",
                path.display()
            ),
        })?;
        for suffix in ["-wal", "-shm"] {
            let sidecar = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
            match tighten(&sidecar) {
                Ok(true) => {
                    tracing::debug!(
                        file = %sidecar.display(),
                        "tightened WAL sidecar permissions to 0600"
                    );
                }
                Ok(false) => {}
                Err(source) => {
                    // Non-fatal by design (the containing directory is 0700
                    // and the sidecar is recreated per session), but never
                    // silent: the condition is logged with the OS error.
                    tracing::warn!(
                        file = %sidecar.display(),
                        %source,
                        "cannot tighten WAL sidecar permissions (directory remains 0700; will retry on next open)"
                    );
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// TRUNCATE checkpoint: rewinds the WAL file to zero length. Errors are
/// surfaced to the caller (a read-only WAL after a fault injection fails
/// here — by design).
pub fn checkpoint_truncate(conn: &rusqlite::Connection) -> Result<(), StorageError> {
    let mut stmt = conn
        .prepare("PRAGMA wal_checkpoint(TRUNCATE)")
        .map_err(migrations::map_rusqlite)?;
    let mut rows = stmt.query([]).map_err(migrations::map_rusqlite)?;
    let row = rows.next().map_err(migrations::map_rusqlite)?;
    let Some(row) = row else {
        return Err(StorageError::Internal {
            detail: "wal_checkpoint returned no row".to_string(),
        });
    };
    // (busy, log_pages, checkpointed_pages); busy=1 means the checkpoint
    // could not run to completion.
    let busy = row.get::<_, i64>(0).map_err(migrations::map_rusqlite)?;
    if busy != 0 {
        return Err(StorageError::Busy { timeout_ms: 0 });
    }
    Ok(())
}
