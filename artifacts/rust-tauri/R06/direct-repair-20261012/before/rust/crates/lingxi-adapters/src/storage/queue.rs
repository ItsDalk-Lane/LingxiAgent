//! Bounded DB work queue: ALL synchronous SQLite access of the run/message
//! database runs on ONE dedicated std::thread owning the connection
//! (R02-T04 step 2).
//!
//! Contract:
//! - Callers submit closures through a **bounded** tokio mpsc channel. The
//!   bound is a hard backpressure edge: [`DbQueue::try_submit`] surfaces
//!   [`StorageError::QueueFull`] instead of buffering unboundedly, and
//!   [`DbQueue::submit`] awaits capacity only up to
//!   `StoreOptions::queue_wait_timeout_ms` before surfacing the same
//!   [`StorageError::QueueFull`] — the WAITERS are bounded too, so a full
//!   queue cannot accumulate unlimited pending futures (R02 stage-repair
//!   R1 / F07). Nothing is ever dropped silently.
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
    /// Maximum time ONE submission may wait for queue capacity (R02
    /// stage-repair R1 / F07): the queue being FULL is a transient,
    /// retryable condition, but a caller that waits for capacity without
    /// a bound is an unbounded waiter — the exact hole the capacity bound
    /// was supposed to close. On expiry the submission fails with
    /// [`StorageError::QueueFull`] (the job was never queued, so nothing
    /// is dropped silently) which the HTTP surface maps to 503.
    pub queue_wait_timeout_ms: u64,
}

/// Largest [`StoreOptions::queue_capacity`] the bounded channel can
/// actually hold (R02 stage-repair R3 / R3-F02): tokio's bounded mpsc
/// channel is backed by a semaphore whose permit count is capped at
/// `Semaphore::MAX_PERMITS` (= `usize::MAX >> 3`); asking for more PANICS
/// inside tokio (`batch_semaphore.rs`), which is exactly how the pre-fix
/// `--db-queue-bound 18446744073709551615` startup died with exit 101.
/// The bound is part of the validated contract so the failure is a loud,
/// checkable configuration error instead. A capacity this large still
/// allocates nothing eagerly — the channel buffer grows per queued job.
pub const MAX_QUEUE_CAPACITY: usize = tokio::sync::Semaphore::MAX_PERMITS;

/// Largest [`StoreOptions::queue_wait_timeout_ms`] supported (R02
/// stage-repair R3 / R3-F02): the wait is handed to
/// `tokio::time::timeout`, whose deadline arithmetic (`Instant::now() +
/// duration`) overflows platform monotonic clocks for absurd durations
/// (tokio's own docs note ~1000 years overflows macOS, ~100 years
/// FreeBSD). 30 days exceeds any legitimate queue-wait budget by orders
/// of magnitude while keeping the arithmetic unambiguously safe on every
/// supported platform. The service CLI applies the same bound to its
/// time-budget flags.
pub const MAX_QUEUE_WAIT_TIMEOUT_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Largest [`StoreOptions::busy_timeout_ms`] SQLite actually honors (R02
/// stage-repair R4 / R4-F01): rusqlite interpolates the pragma value into
/// the SQL text as an integer literal (`Sql::push_value`), and SQLite then
/// parses it with `sqlite3Atoi`, whose consumer is a SIGNED 32-BIT int
/// (`sqlite3GetInt32`; libsqlite3-sys 0.38.2 sqlite3.c:37740). An
/// out-of-range value is NOT an error there — the parse fails and leaves
/// the target at 0, and `sqlite3_busy_timeout(db, 0)` REMOVES the busy
/// handler entirely (sqlite3.c:189041, ms<=0 branch). An i64-clean but
/// i32-dirty value such as 2147483648 therefore silently disabled the busy
/// wait while `validate` claimed it was legal. The validated bound is the
/// real consumer's, not the Rust binding's. 0 stays valid: it is SQLite's
/// documented "no waiting" setting and round-trips honestly.
pub const MAX_BUSY_TIMEOUT_MS: u64 = i32::MAX as u64;

/// Largest [`StoreOptions::checkpoint_pages`] SQLite actually honors
/// (R4-F01): the same signed-32-bit `sqlite3Atoi` consumer as
/// `busy_timeout` (sqlite3.c:147081), and
/// `sqlite3_wal_autocheckpoint(db, n<=0)` REMOVES the autocheckpoint hook
/// (sqlite3.c:189694, nFrame<=0 branch). An out-of-range value silently
/// turned automatic WAL checkpointing OFF — the exact unbounded-WAL
/// outcome the `checkpoint_pages >= 1` rule exists to prevent. The
/// validated bound is the real consumer's, not the Rust binding's.
pub const MAX_CHECKPOINT_PAGES: u64 = i32::MAX as u64;

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            queue_capacity: 64,
            busy_timeout_ms: 5_000,
            checkpoint_pages: 1_000,
            queue_wait_timeout_ms: 10_000,
        }
    }
}

impl StoreOptions {
    /// Validates the knobs that must never be degenerate — INCLUDING the
    /// upper bounds the downstream consumers impose (R02 stage-repair R3 /
    /// R3-F02): every conversion between this struct and tokio/SQLite is
    /// checked, and an out-of-range value is a loud configuration error
    /// here, never a panic or a silent truncation later.
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.queue_capacity == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "queue_capacity must be >= 1 (0 would be an unusable \
                         store, not infinite capacity)"
                    .to_string(),
            });
        }
        if self.queue_capacity > MAX_QUEUE_CAPACITY {
            return Err(StorageError::InvalidRequest {
                detail: format!(
                    "queue_capacity must be <= {MAX_QUEUE_CAPACITY} (tokio's \
                     bounded-channel semaphore maximum; more would panic \
                     inside tokio::sync::mpsc::channel), got {}",
                    self.queue_capacity
                ),
            });
        }
        if self.busy_timeout_ms > MAX_BUSY_TIMEOUT_MS {
            // R4-F01: the pragma's real consumer is SQLite's sqlite3Atoi
            // (signed 32-bit), not the i64 of the rusqlite binding. A wider
            // value does not error inside SQLite — it reads back as 0 and
            // REMOVES the busy handler (silent degradation). Reject it here,
            // before the worker thread and the database file exist.
            return Err(StorageError::InvalidRequest {
                detail: format!(
                    "busy_timeout_ms must be <= {MAX_BUSY_TIMEOUT_MS} (SQLite \
                     parses the pragma with sqlite3Atoi, a signed 32-bit \
                     consumer; larger values silently read back as 0 and \
                     disable the busy handler), got {}",
                    self.busy_timeout_ms
                ),
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
        if self.checkpoint_pages > MAX_CHECKPOINT_PAGES {
            // R4-F01: same signed-32-bit sqlite3Atoi consumer as
            // busy_timeout — an i32-dirty value silently reads back as 0,
            // which removes the autocheckpoint hook and undoes the >= 1
            // rule above. Reject it before the worker/DB open.
            return Err(StorageError::InvalidRequest {
                detail: format!(
                    "checkpoint_pages must be <= {MAX_CHECKPOINT_PAGES} (SQLite \
                     parses the pragma with sqlite3Atoi, a signed 32-bit \
                     consumer; larger values silently read back as 0 and \
                     disable WAL autocheckpoint), got {}",
                    self.checkpoint_pages
                ),
            });
        }
        if self.queue_wait_timeout_ms == 0 {
            // 0 = "wait forever", which reopens the unbounded-waiter hole
            // (F07) the capacity bound was supposed to close.
            return Err(StorageError::InvalidRequest {
                detail: "queue_wait_timeout_ms must be >= 1 (0 would make \
                         submissions wait for capacity without a bound)"
                    .to_string(),
            });
        }
        if self.queue_wait_timeout_ms > MAX_QUEUE_WAIT_TIMEOUT_MS {
            return Err(StorageError::InvalidRequest {
                detail: format!(
                    "queue_wait_timeout_ms must be <= {MAX_QUEUE_WAIT_TIMEOUT_MS} \
                     (30 days — larger deadlines risk overflowing platform \
                     monotonic-clock arithmetic inside tokio::time::timeout), \
                     got {}",
                    self.queue_wait_timeout_ms
                ),
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

    /// Submits `work`, awaiting queue capacity for at most
    /// `queue_wait_timeout_ms` (bounded backpressure without loss): a
    /// full queue beyond the budget fails with [`StorageError::QueueFull`]
    /// and the job is NEVER queued — the caller learns the outcome and the
    /// worker never sees abandoned work.
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
        // F07: the wait for capacity is BOUNDED. An unbounded `send().await`
        // would let unlimited callers pile up as pending futures behind the
        // capacity bound — the channel stays bounded but the WAITERS do not.
        // On expiry the job was never queued: the caller gets a loud,
        // retryable QueueFull (503 on the HTTP surface), never a silent drop.
        let wait = std::time::Duration::from_millis(self.options.queue_wait_timeout_ms);
        match tokio::time::timeout(wait, sender.send(job)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(StorageError::QueueClosed),
            Err(_elapsed) => Err(StorageError::QueueFull),
        }
    }

    /// Graceful close: drains in-flight jobs, checkpoints the WAL
    /// (TRUNCATE), then joins the worker. A failed checkpoint is returned
    /// as an error (documented, not masked).
    ///
    /// The worker join runs on a blocking thread (R02 stage-repair R1 /
    /// F03): a synchronous `JoinHandle::join` inside this async fn would
    /// park the calling executor thread and ignore any caller-side timeout
    /// forever (the thread, not the future, was wedged). With
    /// `spawn_blocking` the async caller's timeout genuinely abandons the
    /// wait; the detached blocking thread is then bounded by the process
    /// exit at the end of the shutdown sequence.
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
            let joined = tokio::task::spawn_blocking(move || handle.join())
                .await
                .map_err(|err| StorageError::Internal {
                    detail: format!("db worker join task failed: {err}"),
                })?;
            if joined.is_err() {
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
            if handle.is_finished() {
                // Fast path: the worker already drained and exited — the
                // join returns immediately (and still surfaces a panic).
                let _ = handle.join();
            } else {
                // The worker is still busy — e.g. waiting on a contended
                // SQLite lock up to busy_timeout, or unwinding from an
                // abandoned shutdown-budget timeout (R2-F02). A synchronous
                // join HERE would park the dropping thread (an unwind path
                // included) behind that foreign wait: exactly the unbounded
                // teardown the unified shutdown budget forbids. Detach the
                // join onto its own thread instead; the worker finishes (or
                // the process exit bounds it for good) and WAL recovery
                // applies on next open. The teardown is still logged.
                let detached = std::thread::Builder::new()
                    .name("lingxi-db-worker-join".to_string())
                    .spawn(move || {
                        if handle.join().is_err() {
                            tracing::warn!("db worker thread panicked (detached drop-path join)");
                        }
                    });
                if let Err(err) = detached {
                    // Thread spawn failing is an OS-level resource fault —
                    // say so; the un-joined handle is dropped (detached) and
                    // the process-exit bound still applies.
                    tracing::warn!(%err, "cannot spawn detached db-worker join thread");
                }
            }
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
        #[cfg(windows)]
        super::windows_acl::ensure_private_directory(parent).map_err(|source| {
            StorageError::Io {
                detail: format!(
                    "cannot protect database directory {}: {source}",
                    parent.display()
                ),
            }
        })?;
    }
    #[cfg(windows)]
    for existing in [
        path.to_path_buf(),
        std::path::PathBuf::from(format!("{}-wal", path.display())),
        std::path::PathBuf::from(format!("{}-shm", path.display())),
    ] {
        match super::windows_acl::ensure_private_file(&existing) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(StorageError::Io {
                    detail: format!(
                        "cannot protect existing database file {}: {source}",
                        existing.display()
                    ),
                })
            }
        }
    }
    #[cfg(windows)]
    if !path.exists() {
        drop(
            super::windows_acl::create_private_file(path).map_err(|source| StorageError::Io {
                detail: format!(
                    "cannot create private database {}: {source}",
                    path.display()
                ),
            })?,
        );
    }
    let conn = rusqlite::Connection::open(path).map_err(migrations::map_rusqlite)?;
    // R4-F01: checked conversions to the REAL consumer width. Both pragmas
    // are parsed by SQLite's sqlite3Atoi (signed 32-bit) — an i64 that does
    // not fit i32 would not error there, it would silently read back as 0
    // (busy handler / autocheckpoint hook removed). StoreOptions::validate
    // already rejects values above i32::MAX before the worker spawns, so
    // these cannot fail in practice; they stay checked so this function is
    // total on its own (no silent read-back-as-0 pragma, ever).
    let busy_timeout_i32 =
        i32::try_from(busy_timeout_ms).map_err(|_| StorageError::InvalidRequest {
            detail: format!(
                "busy_timeout_ms {busy_timeout_ms} exceeds the SQLite pragma range (signed 32-bit)"
            ),
        })?;
    let checkpoint_pages_i32 =
        i32::try_from(checkpoint_pages).map_err(|_| StorageError::InvalidRequest {
            detail: format!(
                "checkpoint_pages {checkpoint_pages} exceeds the SQLite pragma range (signed 32-bit)"
            ),
        })?;
    conn.pragma_update(None, "busy_timeout", busy_timeout_i32)
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
    conn.pragma_update(None, "wal_autocheckpoint", checkpoint_pages_i32)
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
    #[cfg(windows)]
    {
        super::windows_acl::ensure_private_file(path).map_err(|source| StorageError::Io {
            detail: format!("cannot protect database {}: {source}", path.display()),
        })?;
        for suffix in ["-wal", "-shm"] {
            let sidecar = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
            match super::windows_acl::ensure_private_file(&sidecar) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(StorageError::Io {
                        detail: format!(
                            "cannot protect database sidecar {}: {source}",
                            sidecar.display()
                        ),
                    })
                }
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        return Err(StorageError::Io {
            detail: "private database files unsupported".into(),
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "lingxi-queue-test-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir.join("runs.db")
    }

    #[test]
    fn zero_queue_wait_budget_is_rejected_loudly() {
        let options = StoreOptions {
            queue_wait_timeout_ms: 0,
            ..StoreOptions::default()
        };
        let err = options.validate().expect_err("0 = unbounded waiter");
        assert!(
            matches!(err, StorageError::InvalidRequest { .. }),
            "unexpected error: {err:?}"
        );
    }

    /// R3-F02: the channel-capacity upper bound is validated at the library
    /// layer — a capacity above tokio's semaphore maximum used to PANIC
    /// inside `mpsc::channel` (the pre-fix `--db-queue-bound
    /// 18446744073709551615` startup died with exit 101). Validation only
    /// inspects the number; no huge channel is ever allocated here.
    #[test]
    fn queue_capacity_above_the_tokio_bound_is_rejected_loudly() {
        // The documented maximum itself is accepted (validation only — the
        // channel grows lazily per queued job, nothing is allocated).
        let options = StoreOptions {
            queue_capacity: MAX_QUEUE_CAPACITY,
            ..StoreOptions::default()
        };
        options.validate().expect("the documented maximum is valid");

        let options = StoreOptions {
            queue_capacity: MAX_QUEUE_CAPACITY + 1,
            ..StoreOptions::default()
        };
        let err = options
            .validate()
            .expect_err("above the tokio semaphore bound");
        assert!(
            matches!(err, StorageError::InvalidRequest { .. }),
            "unexpected error: {err:?}"
        );
        let options = StoreOptions {
            queue_capacity: usize::MAX,
            ..StoreOptions::default()
        };
        assert!(options.validate().is_err());
    }

    /// R4-F01: pragma-bound knobs are validated against the REAL consumer
    /// range — SQLite's sqlite3Atoi is signed 32-bit, so values above
    /// i32::MAX (not i64::MAX) must be rejected loudly. Pre-fix, e.g.
    /// 2147483648 passed validation and then silently read back as 0,
    /// removing the busy handler / autocheckpoint hook. Validation only
    /// inspects the numbers; no database is opened and nothing is
    /// allocated here.
    #[test]
    fn pragma_knobs_above_the_sqlite_i32_range_are_rejected_loudly() {
        for options in [
            // One past the real supported maximum.
            StoreOptions {
                busy_timeout_ms: MAX_BUSY_TIMEOUT_MS + 1,
                ..StoreOptions::default()
            },
            StoreOptions {
                checkpoint_pages: MAX_CHECKPOINT_PAGES + 1,
                ..StoreOptions::default()
            },
            // i64-clean but i32-dirty: the exact pre-fix silent-degradation
            // window (validate said OK, SQLite read back 0).
            StoreOptions {
                busy_timeout_ms: 2_147_483_648,
                ..StoreOptions::default()
            },
            StoreOptions {
                checkpoint_pages: 2_147_483_648,
                ..StoreOptions::default()
            },
            // Far beyond the pragma range entirely.
            StoreOptions {
                busy_timeout_ms: i64::MAX as u64 + 1,
                ..StoreOptions::default()
            },
            StoreOptions {
                checkpoint_pages: i64::MAX as u64 + 1,
                ..StoreOptions::default()
            },
            StoreOptions {
                busy_timeout_ms: u64::MAX,
                ..StoreOptions::default()
            },
            StoreOptions {
                checkpoint_pages: u64::MAX,
                ..StoreOptions::default()
            },
        ] {
            let err = options.validate().expect_err("beyond the pragma range");
            assert!(
                matches!(err, StorageError::InvalidRequest { .. }),
                "unexpected error: {err:?}"
            );
        }
        // Zero: busy_timeout 0 is SQLite's documented "no waiting" setting
        // and stays valid; checkpoint_pages 0 would disable autocheckpoint
        // (unbounded WAL) and is rejected by its own rule.
        let options = StoreOptions {
            busy_timeout_ms: 0,
            ..StoreOptions::default()
        };
        options
            .validate()
            .expect("busy_timeout_ms 0 is a valid setting");
        let options = StoreOptions {
            checkpoint_pages: 0,
            ..StoreOptions::default()
        };
        assert!(options.validate().is_err());
        // The supported boundaries themselves are valid.
        let options = StoreOptions {
            busy_timeout_ms: MAX_BUSY_TIMEOUT_MS,
            checkpoint_pages: MAX_CHECKPOINT_PAGES,
            ..StoreOptions::default()
        };
        options
            .validate()
            .expect("the signed 32-bit boundary is valid");
    }

    /// R4-F01: rejection happens BEFORE the worker thread and the database
    /// file exist — an out-of-range knob must leave no file, no worker, no
    /// half-opened store behind.
    #[test]
    fn out_of_range_pragma_knobs_are_rejected_before_any_db_open() {
        for options in [
            StoreOptions {
                busy_timeout_ms: MAX_BUSY_TIMEOUT_MS + 1,
                ..StoreOptions::default()
            },
            StoreOptions {
                checkpoint_pages: MAX_CHECKPOINT_PAGES + 1,
                ..StoreOptions::default()
            },
            StoreOptions {
                checkpoint_pages: 0,
                ..StoreOptions::default()
            },
        ] {
            let path = temp_db_path("reject-before-open");
            let err = match DbQueue::open(&path, options) {
                Err(err) => err,
                Ok(_queue) => panic!("out-of-range knob must be rejected before open"),
            };
            assert!(
                matches!(err, StorageError::InvalidRequest { .. }),
                "unexpected error: {err:?}"
            );
            assert!(
                !path.exists(),
                "rejected options must not create the database file"
            );
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    /// R4-F01: validate-vs-actual-consumer split. These values PASS
    /// validation, so the real SQLite database must then hold EXACTLY the
    /// negotiated values — a silent read-back-as-0 (the pre-fix failure
    /// mode) fails this test. Covers common values, the signed-32-bit
    /// maximum, and the legitimate busy_timeout=0 ("no waiting") setting.
    #[tokio::test]
    async fn accepted_pragma_knobs_round_trip_through_real_sqlite() {
        for (busy, pages) in [
            (1_000u64, 1_000u64),                        // common
            (0, 1_000),                                  // busy 0 = no waiting (documented)
            (MAX_BUSY_TIMEOUT_MS, MAX_CHECKPOINT_PAGES), // signed-32-bit maxima
        ] {
            let path = temp_db_path("pragma-roundtrip");
            let queue = DbQueue::open(
                &path,
                StoreOptions {
                    busy_timeout_ms: busy,
                    checkpoint_pages: pages,
                    ..StoreOptions::default()
                },
            )
            .expect("open queue with in-range knobs");
            let (actual_busy, actual_pages) = queue
                .submit(move |conn| {
                    let busy: i64 = conn
                        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
                        .map_err(migrations::map_rusqlite)?;
                    let pages: i64 = conn
                        .query_row("PRAGMA wal_autocheckpoint", [], |row| row.get(0))
                        .map_err(migrations::map_rusqlite)?;
                    Ok((busy, pages))
                })
                .await
                .expect("read pragmas back");
            assert_eq!(
                (actual_busy, actual_pages),
                (busy as i64, pages as i64),
                "SQLite must hold exactly the negotiated values (silent \
                 read-back-as-0 is the R4-F01 failure mode)"
            );
            queue.close().await.expect("close");
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    /// R3-F02: the queue-wait budget has a documented upper bound (30 days)
    /// so the tokio timeout deadline arithmetic can never overflow a
    /// platform monotonic clock.
    #[test]
    fn queue_wait_budget_above_the_documented_max_is_rejected_loudly() {
        let options = StoreOptions {
            queue_wait_timeout_ms: MAX_QUEUE_WAIT_TIMEOUT_MS,
            ..StoreOptions::default()
        };
        options.validate().expect("the documented maximum is valid");
        let options = StoreOptions {
            queue_wait_timeout_ms: MAX_QUEUE_WAIT_TIMEOUT_MS + 1,
            ..StoreOptions::default()
        };
        let err = options
            .validate()
            .expect_err("above the documented wait bound");
        assert!(
            matches!(err, StorageError::InvalidRequest { .. }),
            "unexpected error: {err:?}"
        );
    }

    /// F07: with the worker occupied and the bounded channel full, a third
    /// submission must not wait forever — it fails with QueueFull after
    /// the configured wait budget, and the queued work still runs.
    #[tokio::test]
    async fn queue_wait_budget_turns_a_full_queue_into_loud_queue_full() {
        let path = temp_db_path("waitbudget");
        let queue = std::sync::Arc::new(
            DbQueue::open(
                &path,
                StoreOptions {
                    queue_capacity: 1,
                    busy_timeout_ms: 1_000,
                    checkpoint_pages: 1_000,
                    queue_wait_timeout_ms: 150,
                },
            )
            .expect("open queue"),
        );

        // Job A occupies the single worker until the test releases it.
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let queue_a = std::sync::Arc::clone(&queue);
        let job_a = tokio::spawn(async move {
            queue_a
                .submit(move |_| {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                    Ok(())
                })
                .await
        });
        started_rx.await.expect("job A reached the worker");

        // Job B fills the single channel slot (worker busy with A).
        let queued = queue
            .try_submit(|_| Ok(()))
            .expect("slot free for the queued job");

        // Job C now faces a full channel and a busy worker: the wait
        // budget must fire — loudly, with QueueFull, in bounded time.
        let started = std::time::Instant::now();
        let outcome = queue.submit(|_| Ok(())).await;
        let elapsed = started.elapsed();
        assert!(
            matches!(outcome, Err(StorageError::QueueFull)),
            "full queue beyond the wait budget must be QueueFull, got {outcome:?}"
        );
        assert!(
            elapsed >= std::time::Duration::from_millis(100),
            "the budget must actually be awaited (elapsed {elapsed:?})"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "the budget must bound the wait (elapsed {elapsed:?})"
        );

        // Nothing was dropped silently: A and B still execute.
        release_tx.send(()).expect("release job A");
        job_a.await.expect("job A task").expect("job A ran");
        queued.wait().await.expect("queued job B ran");
        queue.close().await.expect("close");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
