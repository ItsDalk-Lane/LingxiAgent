//! Graceful-shutdown coordinator (R02-T06 step 2 + step 4 退出超时).
//!
//! Taskbook order (each phase is bounded by ONE deadline and every phase
//! outcome — including timeouts — is explicit; nothing is ever silent):
//!
//! 1. **stop accepting new requests** — the transport stops first (axum's
//!    graceful shutdown drains in-flight HTTP; this runs BEFORE the
//!    coordinator because the same signal future drives it);
//! 2. **wait/cancel managed tasks** — active WebSocket sessions receive a
//!    close(1001) frame via the broadcast watch and are waited on with the
//!    deadline (they are the long-lived managed tasks; in-flight HTTP is
//!    already drained by the transport);
//! 3. **flush key events + close the DB** — key events are durable in the
//!    same transaction as their run facts (R02-T04), so flushing IS the
//!    bounded-queue drain inside `RunDatabase::close` (FIFO drain →
//!    TRUNCATE checkpoint → worker join);
//! 4. **remove our own instance record** — record cleanup verifies the
//!    record is still ours and removes it (never a foreign record).
//!
//! Exit-timeout semantics (step 4 of the taskbook: "退出超时行为显式"):
//! a phase that exceeds the deadline gets the machine-readable
//! `LINGXI_SERVICE_SHUTDOWN_TIMEOUT` marker on stderr plus a tracing
//! diagnostic, then the coordinator CONTINUES with the remaining phases
//! (the instance record is still removed — a timed-out DB close must not
//! leave the home looking owned). The final exit code is deterministic:
//! storage-close failure → 5, record-cleanup failure → 4, any timeout → 6,
//! clean → 0.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;

use crate::instance::InstanceGuard;

/// Default shutdown deadline (per phase) for the production binary.
pub const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 10_000;

/// Machine-readable stderr marker for a phase that exceeded the deadline.
pub const SHUTDOWN_TIMEOUT_MARKER: &str = "LINGXI_SERVICE_SHUTDOWN_TIMEOUT";

/// Shutdown phases, in execution order (diagnostic vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownPhase {
    /// Managed WebSocket sessions cancelled + waited.
    WsDrain,
    /// Bounded-queue drain + TRUNCATE checkpoint + worker join.
    StorageClose,
    /// Own instance record removal + lock release.
    RecordCleanup,
}

impl std::fmt::Display for ShutdownPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ShutdownPhase::WsDrain => "ws_drain",
            ShutdownPhase::StorageClose => "storage_close",
            ShutdownPhase::RecordCleanup => "record_cleanup",
        })
    }
}

/// Full outcome of the shutdown sequence. Every field is explicit; the
/// binary maps [`ShutdownReport::exit_code`] 1:1 onto the process exit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShutdownReport {
    pub ws_drain_timed_out: bool,
    pub storage_close_timed_out: bool,
    pub storage_error: Option<StorageError>,
    pub record_cleanup_timed_out: bool,
    pub record_error: Option<String>,
}

impl ShutdownReport {
    /// Deterministic exit-code mapping (documented in the binary usage).
    pub fn exit_code(&self) -> u8 {
        if self.storage_error.is_some() {
            5
        } else if self.record_error.is_some() {
            4
        } else if self.ws_drain_timed_out
            || self.storage_close_timed_out
            || self.record_cleanup_timed_out
        {
            6
        } else {
            0
        }
    }

    pub fn any_timeout(&self) -> bool {
        self.ws_drain_timed_out || self.storage_close_timed_out || self.record_cleanup_timed_out
    }
}

/// The managed-task handle for WebSocket sessions: a shutdown broadcast
/// (watch channel) plus an open-connection counter the coordinator waits
/// on. Sessions register at loop start and unregister on ALL exits (guard
/// drop), so the counter is the drain signal.
#[derive(Debug)]
pub struct WsShutdown {
    tx: watch::Sender<bool>,
    open: AtomicUsize,
}

impl Default for WsShutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl WsShutdown {
    pub fn new() -> Self {
        let (tx, _) = watch::channel(false);
        Self {
            tx,
            open: AtomicUsize::new(0),
        }
    }

    /// A fresh receiver for one WS session task.
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.tx.subscribe()
    }

    /// Broadcasts "shutdown requested" WITHOUT waiting (the composition
    /// root calls this at signal time, BEFORE the transport's own graceful
    /// wait: axum waits for upgraded connections to finish, so the
    /// sessions must learn about the shutdown through this broadcast —
    /// sending it only after the transport returned would deadlock).
    pub fn request_close(&self) {
        let _ = self.tx.send(true);
    }

    /// Marks one WS session as open (call when the session loop starts).
    pub fn connection_opened(&self) {
        self.open.fetch_add(1, Ordering::AcqRel);
    }

    /// Marks one WS session as finished (guarded — see [`WsSessionGuard`]).
    pub fn connection_closed(&self) {
        self.open.fetch_sub(1, Ordering::AcqRel);
    }

    pub fn open_count(&self) -> usize {
        self.open.load(Ordering::Acquire)
    }

    /// Broadcasts shutdown and waits (bounded) for open sessions to
    /// finish. Returns `true` when all sessions drained in time.
    pub async fn close_and_wait(&self, deadline: Duration) -> bool {
        let _ = self.tx.send(true);
        let waited = tokio::time::timeout(deadline, async {
            while self.open_count() > 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        waited.is_ok()
    }
}

/// RAII registration of one WS session against [`WsShutdown`]: increments
/// on creation, decrements on drop (every exit path of the session loop).
pub struct WsSessionGuard {
    handle: Arc<WsShutdown>,
}

impl WsSessionGuard {
    pub fn new(handle: Arc<WsShutdown>) -> Self {
        handle.connection_opened();
        Self { handle }
    }
}

impl Drop for WsSessionGuard {
    fn drop(&mut self) {
        self.handle.connection_closed();
    }
}

/// Runs phases 2–4 of the shutdown contract (phase 1 — stop accepting —
/// belongs to the transport's own graceful shutdown). Every phase is
/// bounded by `deadline`; timeouts are loud and non-fatal to the
/// remaining phases.
pub async fn graceful_shutdown(
    storage: &RunDatabase,
    ws: &WsShutdown,
    guard: &mut InstanceGuard,
    deadline: Duration,
    deadline_ms: u64,
) -> ShutdownReport {
    let mut report = ShutdownReport::default();

    // Phase 2: wait/cancel managed tasks (WS sessions).
    if !ws.close_and_wait(deadline).await {
        report.ws_drain_timed_out = true;
        eprintln!(
            "{SHUTDOWN_TIMEOUT_MARKER} phase=ws_drain deadline_ms={deadline_ms} open_sessions={}",
            ws.open_count()
        );
        tracing::error!(
            phase = %ShutdownPhase::WsDrain,
            deadline_ms,
            open_sessions = ws.open_count(),
            "shutdown phase exceeded its deadline; continuing with the \
             remaining phases (sessions are force-abandoned)"
        );
    }

    // Phase 3: flush key events + close the DB (drain → checkpoint → join).
    match tokio::time::timeout(deadline, storage.close()).await {
        Ok(Ok(())) => {
            tracing::info!(phase = %ShutdownPhase::StorageClose, "run database closed (drained, WAL truncated)");
        }
        Ok(Err(err)) => {
            report.storage_error = Some(err.clone());
            tracing::error!(phase = %ShutdownPhase::StorageClose, %err, "run database shutdown FAILED (explicit, exit code 5)");
        }
        Err(_elapsed) => {
            report.storage_close_timed_out = true;
            eprintln!("{SHUTDOWN_TIMEOUT_MARKER} phase=storage_close deadline_ms={deadline_ms}");
            tracing::error!(
                phase = %ShutdownPhase::StorageClose,
                deadline_ms,
                "shutdown phase exceeded its deadline; the DB worker teardown \
                 did not complete — this process continues to record cleanup, \
                 and the WAL applies its own recovery on the next open"
            );
        }
    }

    // Phase 4: remove OUR instance record, then release the lock.
    let cleanup = tokio::time::timeout(deadline, async {
        // release() is sync + cheap filesystem ops; wrap for the deadline.
        tokio::task::block_in_place(|| guard.release())
    })
    .await;
    match cleanup {
        Ok(Ok(())) => {
            tracing::info!(phase = %ShutdownPhase::RecordCleanup, "own instance record removed, lock released");
        }
        Ok(Err(err)) => {
            report.record_error = Some(err.to_string());
            tracing::error!(phase = %ShutdownPhase::RecordCleanup, %err, "shutdown instance-record cleanup FAILED (explicit, exit code 4)");
        }
        Err(_) => {
            report.record_cleanup_timed_out = true;
            eprintln!("{SHUTDOWN_TIMEOUT_MARKER} phase=record_cleanup deadline_ms={deadline_ms}");
            tracing::error!(
                phase = %ShutdownPhase::RecordCleanup,
                deadline_ms,
                "shutdown phase exceeded its deadline; the instance record may \
                 remain on disk (the OS lock is still released when this \
                 process exits)"
            );
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_precedence_is_deterministic() {
        let mut report = ShutdownReport::default();
        assert_eq!(report.exit_code(), 0);
        report.ws_drain_timed_out = true;
        assert_eq!(report.exit_code(), 6);
        report.ws_drain_timed_out = false;
        report.record_error = Some("boom".to_string());
        assert_eq!(report.exit_code(), 4);
        report.storage_error = Some(StorageError::QueueClosed);
        assert_eq!(
            report.exit_code(),
            5,
            "storage failure outranks record failure"
        );
    }

    #[tokio::test]
    async fn ws_drain_times_out_loudly_with_open_sessions() {
        let ws = Arc::new(WsShutdown::new());
        ws.connection_opened();
        let started = std::time::Instant::now();
        let drained = ws.close_and_wait(Duration::from_millis(40)).await;
        assert!(!drained, "one open session must not drain by itself");
        assert!(started.elapsed() >= Duration::from_millis(35));
        // A session finishing after the deadline still decrements cleanly.
        ws.connection_closed();
        assert_eq!(ws.open_count(), 0);
        let drained_now = ws.close_and_wait(Duration::from_millis(40)).await;
        assert!(drained_now);
    }

    #[tokio::test]
    async fn ws_session_guard_covers_all_drop_paths() {
        let ws = Arc::new(WsShutdown::new());
        {
            let _guard = WsSessionGuard::new(Arc::clone(&ws));
            assert_eq!(ws.open_count(), 1);
        }
        assert_eq!(ws.open_count(), 0);
    }
}
