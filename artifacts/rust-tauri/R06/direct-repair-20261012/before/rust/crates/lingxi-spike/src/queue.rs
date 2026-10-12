//! Bounded work queue isolating synchronous SQLite access.
//!
//! Design contract (taskbook 02 §2 + R01-T03 step 2, W14):
//!
//! * `rusqlite::Connection` is synchronous and `!Sync`; all SQL therefore runs
//!   on ONE dedicated std::thread that owns the connection. Callers never
//!   touch the connection directly — they submit [`QueueOp`] jobs through a
//!   **bounded** tokio mpsc channel.
//! * The bound is a hard backpressure edge: [`BoundedQueue::try_submit`]
//!   surfaces `QueueError::Full` instead of buffering unboundedly or dropping
//!   work; [`BoundedQueue::submit`] awaits capacity. No silent degradation
//!   (project red line): every failure mode is an explicit `Err`.
//! * WAL journal mode is enabled so readers are not blocked by the writer
//!   thread; the mode actually negotiated is reported back and asserted in
//!   tests (W06/W07).

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

/// Operations the queue knows how to run against the store.
#[derive(Debug)]
pub enum QueueOp {
    /// Insert or replace a key/value pair.
    Put { key: String, value: String },
    /// Read a value by key.
    Get { key: String },
    /// Number of rows in the store.
    Count,
    /// Block the worker thread on a barrier (test-only; makes the bounded
    /// behaviour deterministic by parking the worker while the queue fills).
    Park(std::sync::Arc<std::sync::Barrier>),
    /// Report the SQLite version and negotiated journal mode.
    Stats,
}

/// Result of a successfully executed [`QueueOp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueResult {
    Ok,
    Value(Option<String>),
    Count(usize),
    Stats {
        sqlite_version: String,
        journal_mode: String,
    },
}

/// Explicit failure modes of the queue. Nothing is silently dropped or
/// retried; the caller always learns the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueError {
    /// Bounded queue is at capacity (`try_submit` only).
    Full,
    /// Worker thread has terminated; the job was not executed.
    Closed,
    /// SQLite reported an error.
    Sqlite(String),
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueError::Full => write!(f, "bounded queue at capacity"),
            QueueError::Closed => write!(f, "queue worker is closed"),
            QueueError::Sqlite(e) => write!(f, "sqlite error: {e}"),
        }
    }
}

impl std::error::Error for QueueError {}

struct Job {
    op: QueueOp,
    reply: oneshot::Sender<Result<QueueResult, QueueError>>,
}

fn run_op(conn: &Connection, op: QueueOp) -> Result<QueueResult, QueueError> {
    match op {
        QueueOp::Put { key, value } => {
            conn.execute(
                "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
                (&key, &value),
            )
            .map_err(|e| QueueError::Sqlite(e.to_string()))?;
            Ok(QueueResult::Ok)
        }
        QueueOp::Get { key } => {
            let mut stmt = conn
                .prepare("SELECT value FROM kv WHERE key = ?1")
                .map_err(|e| QueueError::Sqlite(e.to_string()))?;
            let mut rows = stmt
                .query([&key])
                .map_err(|e| QueueError::Sqlite(e.to_string()))?;
            match rows.next().map_err(|e| QueueError::Sqlite(e.to_string()))? {
                Some(row) => Ok(QueueResult::Value(Some(
                    row.get::<_, String>(0)
                        .map_err(|e| QueueError::Sqlite(e.to_string()))?,
                ))),
                None => Ok(QueueResult::Value(None)),
            }
        }
        QueueOp::Count => {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM kv", [], |r| r.get(0))
                .map_err(|e| QueueError::Sqlite(e.to_string()))?;
            Ok(QueueResult::Count(n as usize))
        }
        QueueOp::Park(barrier) => {
            barrier.wait();
            Ok(QueueResult::Ok)
        }
        QueueOp::Stats => {
            let sqlite_version: String = conn
                .query_row("SELECT sqlite_version()", [], |r| r.get(0))
                .map_err(|e| QueueError::Sqlite(e.to_string()))?;
            let journal_mode: String = conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .map_err(|e| QueueError::Sqlite(e.to_string()))?;
            Ok(QueueResult::Stats {
                sqlite_version,
                journal_mode,
            })
        }
    }
}

/// A bounded, single-writer queue in front of a SQLite database file.
pub struct BoundedQueue {
    tx: Option<mpsc::Sender<Job>>,
    worker: Option<JoinHandle<()>>,
    db_path: PathBuf,
    capacity: usize,
}

impl BoundedQueue {
    /// Open (creating if needed) the database at `path` and start the worker
    /// thread. `capacity` is the hard bound of the job queue.
    pub async fn open(path: &Path, capacity: usize) -> Result<Self, QueueError> {
        if capacity == 0 {
            return Err(QueueError::Sqlite(
                "queue capacity must be >= 1".to_string(),
            ));
        }
        let path_buf = path.to_path_buf();
        // Open + initialise the connection before announcing readiness so a
        // broken path/permission is reported to the caller, not deferred into
        // the worker thread (no silent degradation).
        let (init_tx, init_rx) = oneshot::channel::<Result<(), QueueError>>();
        let (tx, mut rx) = mpsc::channel::<Job>(capacity);
        let worker_path = path_buf.clone();
        let worker = std::thread::spawn(move || {
            let init = (|| -> Result<Connection, QueueError> {
                let conn = Connection::open(&worker_path)
                    .map_err(|e| QueueError::Sqlite(e.to_string()))?;
                conn.pragma_update(None, "journal_mode", "WAL")
                    .map_err(|e| QueueError::Sqlite(e.to_string()))?;
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS kv (
                        key   TEXT PRIMARY KEY,
                        value TEXT NOT NULL
                     );",
                )
                .map_err(|e| QueueError::Sqlite(e.to_string()))?;
                Ok(conn)
            })();
            let conn = match init {
                Ok(conn) => {
                    let _ = init_tx.send(Ok(()));
                    conn
                }
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                    return;
                }
            };
            while let Some(job) = rx.blocking_recv() {
                let result = run_op(&conn, job.op);
                // If the caller is gone the job outcome is still not lost: the
                // error path is observable via tracing, never silently dropped.
                if job.reply.send(result).is_err() {
                    eprintln!("lingxi-spike queue: caller dropped before reply");
                }
            }
        });
        match init_rx.await {
            Ok(Ok(())) => Ok(Self {
                tx: Some(tx),
                worker: Some(worker),
                db_path: path_buf,
                capacity,
            }),
            Ok(Err(e)) => {
                let _ = worker.join();
                Err(e)
            }
            Err(_) => {
                let _ = worker.join();
                Err(QueueError::Closed)
            }
        }
    }

    /// Submit a job, awaiting queue capacity (backpressure).
    pub async fn submit(&self, op: QueueOp) -> Result<QueueResult, QueueError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .as_ref()
            .ok_or(QueueError::Closed)?
            .send(Job {
                op,
                reply: reply_tx,
            })
            .await
            .map_err(|_| QueueError::Closed)?;
        reply_rx.await.map_err(|_| QueueError::Closed)?
    }

    /// Submit a job without waiting; fails with [`QueueError::Full`] when the
    /// bounded queue is at capacity.
    pub fn try_submit(&self, op: QueueOp) -> Result<QueueResultRx, QueueError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .as_ref()
            .ok_or(QueueError::Closed)?
            .try_send(Job {
                op,
                reply: reply_tx,
            })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => QueueError::Full,
                mpsc::error::TrySendError::Closed(_) => QueueError::Closed,
            })?;
        Ok(QueueResultRx { rx: reply_rx })
    }

    /// The bound this queue was opened with.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Path of the underlying database file.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Drain the queue and stop the worker thread: the sender is dropped so
    /// the worker exits after finishing in-flight jobs, then it is joined.
    pub async fn close(mut self) -> Result<(), QueueError> {
        drop(self.tx.take());
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| QueueError::Closed)?;
        }
        Ok(())
    }
}

/// Awaitable handle returned by [`BoundedQueue::try_submit`].
pub struct QueueResultRx {
    rx: oneshot::Receiver<Result<QueueResult, QueueError>>,
}

impl QueueResultRx {
    pub async fn wait(self) -> Result<QueueResult, QueueError> {
        self.rx.await.map_err(|_| QueueError::Closed)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-spike-queue-test-{}-{}",
            std::process::id(),
            tag
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir.join("queue.db")
    }

    #[tokio::test]
    async fn put_get_count_roundtrip_and_wal() {
        let path = temp_db_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        let queue = BoundedQueue::open(&path, 8).await.expect("open queue");

        queue
            .submit(QueueOp::Put {
                key: "问候".into(),
                value: "你好，灵犀".into(),
            })
            .await
            .expect("put");
        let got = queue
            .submit(QueueOp::Get {
                key: "问候".into()
            })
            .await
            .expect("get");
        assert_eq!(got, QueueResult::Value(Some("你好，灵犀".into())));

        let stats = queue.submit(QueueOp::Stats).await.expect("stats");
        match stats {
            QueueResult::Stats {
                sqlite_version,
                journal_mode,
            } => {
                assert!(!sqlite_version.is_empty());
                assert_eq!(journal_mode, "wal", "WAL mode must be negotiated");
            }
            other => panic!("unexpected stats result: {other:?}"),
        }

        queue.close().await.expect("close");
        assert!(path.exists(), "db file must exist after close");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn many_jobs_all_complete_in_order() {
        let path = temp_db_path("many");
        let _ = std::fs::remove_file(&path);
        let queue = BoundedQueue::open(&path, 4).await.expect("open queue");
        const N: usize = 500;
        for i in 0..N {
            queue
                .submit(QueueOp::Put {
                    key: format!("k{i:04}"),
                    value: format!("v{i}"),
                })
                .await
                .expect("put");
        }
        let count = queue.submit(QueueOp::Count).await.expect("count");
        assert_eq!(count, QueueResult::Count(N));
        let last = queue
            .submit(QueueOp::Get {
                key: format!("k{:04}", N - 1),
            })
            .await
            .expect("get");
        assert_eq!(last, QueueResult::Value(Some(format!("v{}", N - 1))));
        queue.close().await.expect("close");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn bounded_queue_reports_full_instead_of_dropping() {
        let path = temp_db_path("full");
        let _ = std::fs::remove_file(&path);
        let queue = BoundedQueue::open(&path, 1).await.expect("open queue");
        // Park the worker on a barrier so the bounded channel deterministically
        // fills: one job executing + one queued = capacity 1 reached.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        queue
            .try_submit(QueueOp::Park(barrier.clone()))
            .expect("park job fits");
        // Wait until the worker has actually taken the Park job off the queue.
        // Barrier has 2 parties: the worker (inside run_op) and this test.
        // We cannot wait() here without blocking the runtime, so spin on
        // queue capacity instead: once the worker holds Park, one more job
        // fills the channel.
        let mut filler = None;
        for _ in 0..10_000 {
            match queue.try_submit(QueueOp::Count) {
                Ok(rx) => {
                    filler = Some(rx);
                    break;
                }
                Err(QueueError::Full) => std::thread::yield_now(),
                Err(e) => panic!("unexpected error while filling: {e}"),
            }
        }
        let _filler = filler.expect("worker must drain Park into execution");
        // Now: worker parked on barrier + one job queued (capacity 1) => Full.
        let overflow = queue.try_submit(QueueOp::Count);
        match overflow {
            Err(QueueError::Full) => {}
            Err(e) => panic!("expected Full, got {e:?}"),
            Ok(_) => panic!("expected Full, but job was accepted"),
        }
        // Release the worker; the queued job must still complete (no loss).
        barrier.wait();
        let filler_result = _filler.wait().await.expect("queued job completes");
        assert!(matches!(filler_result, QueueResult::Count(_)));
        queue.close().await.expect("close");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn zero_capacity_is_rejected() {
        let path = temp_db_path("zero");
        let err = BoundedQueue::open(&path, 0).await;
        assert!(err.is_err(), "capacity 0 must be rejected");
    }
}
