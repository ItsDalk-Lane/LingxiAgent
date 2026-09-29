//! Graceful-shutdown coordinator (R02-T06 step 2 + step 4 退出超时; R02
//! stage-repair R1 / F03 unified budget).
//!
//! Taskbook order (every phase — transport drain included — is bounded by
//! ONE budget anchored at the moment the exit signal arrived, and every
//! phase outcome — including timeouts — is explicit; nothing is ever
//! silent):
//!
//! 1. **stop accepting new requests + drain in-flight HTTP** — the
//!    transport drains under the SAME from-signal budget (the serving loop
//!    reports a timed-out drain explicitly; pre-fix this wait had NO
//!    deadline at all, so a stuck connection held the process past every
//!    configured timeout before any phase even started);
//! 2. **wait/cancel managed tasks** — active WebSocket sessions receive a
//!    close(1001) frame via the broadcast watch and are waited on with the
//!    remaining budget (pre-hello handshakes race the broadcast too — a
//!    client that never sends ClientHello cannot hold the drain);
//! 3. **flush key events + close the DB** — key events are durable in the
//!    same transaction as their run facts (R02-T04), so flushing IS the
//!    bounded-queue drain inside `RunDatabase::close` (FIFO drain →
//!    TRUNCATE checkpoint → worker join; the join runs on a blocking thread
//!    the async side can time out — the process never wedges behind it);
//! 4. **remove our own instance record** — record cleanup verifies the
//!    record is still ours and removes it (never a foreign record). The
//!    synchronous release runs on a DEDICATED thread so the unified
//!    deadline is REAL for this phase too (R9-F04: a timeout-wrapped
//!    block_in_place cannot preempt a never-yielding filesystem call, so
//!    a slow release could overrun the budget and still return Ok); on
//!    timeout the worker is abandoned (not cancelled), the possible
//!    residue is reported honestly, and the binary's deterministic exit
//!    bounds it for good.
//!
//! Exit-timeout semantics (step 4 of the taskbook: "退出超时行为显式"):
//! a phase that exceeds the remaining budget gets the machine-readable
//! `LINGXI_SERVICE_SHUTDOWN_TIMEOUT` marker on stderr plus a tracing
//! diagnostic, then the coordinator CONTINUES with the remaining phases
//! (the instance-record removal is still ATTEMPTED — a timed-out DB close
//! must not leave the home looking owned — though under an exhausted
//! budget the attempt itself is reported as timed out with its possible
//! residue). The binary ends with a deterministic `std::process::exit`,
//! so even a wedged foreign thread (or the abandoned record-cleanup
//! worker) can never hold the process past the budget. The final exit
//! code is deterministic: storage-close failure → 5, record-cleanup
//! failure → 4, any timeout → 6, clean → 0.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;

use crate::instance::InstanceGuard;

/// Default TOTAL shutdown budget (from the signal moment, across all
/// phases) for the production binary.
pub const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 10_000;

/// Machine-readable stderr marker for a phase that exceeded the budget.
pub const SHUTDOWN_TIMEOUT_MARKER: &str = "LINGXI_SERVICE_SHUTDOWN_TIMEOUT";

/// The ONE shutdown budget, anchored at the signal moment (R02
/// stage-repair R1 / F03). Every phase — transport drain (enforced inside
/// the serving loop), WS drain, storage close, record cleanup — is bounded
/// by what remains of this single budget, so the whole shutdown is bounded
/// by `--shutdown-timeout-ms` from the signal, not per phase from whenever
/// the previous wait happened to return.
#[derive(Debug, Clone, Copy)]
pub struct ShutdownBudget {
    signalled_at: std::time::Instant,
    total: Duration,
}

impl ShutdownBudget {
    pub fn new(signalled_at: std::time::Instant, total: Duration) -> Self {
        Self {
            signalled_at,
            total,
        }
    }

    /// Budget left for the remaining phases. Zero means exhausted: every
    /// subsequent phase reports an immediate, explicit timeout instead of
    /// running unbounded.
    pub fn remaining(&self) -> Duration {
        self.total.saturating_sub(self.signalled_at.elapsed())
    }

    /// The configured total (for diagnostics/markers).
    pub fn total_ms(&self) -> u64 {
        self.total.as_millis() as u64
    }
}

/// Shutdown phases, in execution order (diagnostic vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownPhase {
    /// Transport stop + in-flight HTTP drain (enforced in the serving
    /// loop; recorded here for the report).
    TransportDrain,
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
            ShutdownPhase::TransportDrain => "transport_drain",
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
    /// The transport's in-flight drain exceeded the budget (recorded by the
    /// serving loop; wedged connections are bounded at process level).
    pub transport_drain_timed_out: bool,
    pub ws_drain_timed_out: bool,
    /// R03-T06: the background-drive exit drain expired with live drives
    /// (reported unconfirmed; their durable run rows stay active for the
    /// R03-T07 recovery scan).
    pub background_drain_timed_out: bool,
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
        } else if self.any_timeout() {
            6
        } else {
            0
        }
    }

    pub fn any_timeout(&self) -> bool {
        self.transport_drain_timed_out
            || self.ws_drain_timed_out
            || self.background_drain_timed_out
            || self.storage_close_timed_out
            || self.record_cleanup_timed_out
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

/// Runs phases 2–4 of the shutdown contract (phase 1 — stop accepting +
/// drain in-flight HTTP — ran inside the serving loop under the same
/// budget; its outcome arrives as `transport_drain_timed_out`). Every
/// phase is bounded by what REMAINS of `budget` (anchored at the signal
/// moment); timeouts are loud and non-fatal to the remaining phases.
pub async fn graceful_shutdown(
    storage: &RunDatabase,
    ws: &WsShutdown,
    background: &crate::background::BackgroundDriveRegistry,
    guard: InstanceGuard,
    transport_drain_timed_out: bool,
    budget: ShutdownBudget,
) -> ShutdownReport {
    let budget_ms = budget.total_ms();
    let mut report = ShutdownReport {
        transport_drain_timed_out,
        ..ShutdownReport::default()
    };
    if transport_drain_timed_out {
        tracing::error!(
            phase = %ShutdownPhase::TransportDrain,
            budget_ms,
            "transport drain exceeded the shutdown budget; in-flight \
             connections were abandoned (bounded at process level)"
        );
    }

    // Phase 2: wait/cancel managed tasks (WS sessions).
    let remaining = budget.remaining();
    if !ws.close_and_wait(remaining).await {
        report.ws_drain_timed_out = true;
        eprintln!(
            "{SHUTDOWN_TIMEOUT_MARKER} phase=ws_drain budget_ms={budget_ms} remaining_ms={} open_sessions={}",
            remaining.as_millis(),
            ws.open_count()
        );
        tracing::error!(
            phase = %ShutdownPhase::WsDrain,
            budget_ms,
            open_sessions = ws.open_count(),
            "shutdown phase exceeded the remaining budget; continuing with the \
             remaining phases (sessions are force-abandoned)"
        );
    }

    // Phase 2.5 (R03-T06 minimal exit hook): join the live BACKGROUND
    // drives under the remaining budget. The transport and WS drains
    // already abandoned the CLIENTS — the background runs are the ones
    // that must NOT be silently dropped: whatever cannot confirm within
    // the budget is REPORTED (their durable run rows stay active for the
    // R03-T07 recovery scan; no fabricated terminals). The full exit
    // strategy (per-task cancel/wait policy) is R03-T07.
    let background_report = background.drain_within(budget.remaining()).await;
    if !background_report.unconfirmed.is_empty() {
        report.background_drain_timed_out = true;
        eprintln!(
            "{SHUTDOWN_TIMEOUT_MARKER} phase=background_drain budget_ms={budget_ms} \
             unconfirmed={}",
            background_report.unconfirmed.join(",")
        );
    }

    // Phase 3: flush key events + close the DB (drain → checkpoint → join;
    //    the worker join runs on a blocking thread the timeout can abandon
    //    — the process exit bounds it for good).
    let remaining = budget.remaining();
    match tokio::time::timeout(remaining, storage.close()).await {
        Ok(Ok(())) => {
            tracing::info!(phase = %ShutdownPhase::StorageClose, "run database closed (drained, WAL truncated)");
        }
        Ok(Err(err)) => {
            report.storage_error = Some(err.clone());
            tracing::error!(phase = %ShutdownPhase::StorageClose, %err, "run database shutdown FAILED (explicit, exit code 5)");
        }
        Err(_elapsed) => {
            report.storage_close_timed_out = true;
            eprintln!(
                "{SHUTDOWN_TIMEOUT_MARKER} phase=storage_close budget_ms={budget_ms} remaining_ms={}",
                remaining.as_millis()
            );
            tracing::error!(
                phase = %ShutdownPhase::StorageClose,
                budget_ms,
                "shutdown phase exceeded the remaining budget; the DB worker teardown \
                 did not complete — this process continues to record cleanup, \
                 and the WAL applies its own recovery on the next open"
            );
        }
    }

    // Phase 4: remove OUR instance record, then release the lock.
    // R9-F04: `release()` is fully SYNCHRONOUS filesystem work (record
    // re-read + conditional remove + unlock) with no await point, so no
    // executor can preempt it. The previous
    // `timeout(remaining, async { block_in_place(|| guard.release()) })`
    // could NOT enforce the deadline: tokio's Timeout polls the inner
    // future first and a never-yielding future completes inside that
    // first poll — block_in_place lets the REST of the runtime proceed
    // but does not preempt the release itself, so a slow filesystem
    // could overrun the remaining budget (even ZERO remaining) and still
    // return Ok with `record_cleanup_timed_out` unset: the unified budget
    // was silently unenforced for this phase. The cleanup now runs on a
    // DEDICATED OS thread (the guard MOVES there) and the async side
    // awaits its oneshot result under the real timeout, so the deadline
    // elapses in parallel — the timeout is genuine. On timeout the worker
    // thread is abandoned, NOT cancelled (the removal may or may not have
    // completed; the possible residue is reported honestly) and the
    // binary's deterministic `std::process::exit` bounds it for good —
    // exactly the storage-close worker-join contract. The OS lock is
    // released at process exit regardless.
    let remaining = budget.remaining();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut guard = guard;
        let result = guard.release();
        let _ = done_tx.send(result);
    });
    if remaining.is_zero() {
        // The deadline has ALREADY passed: report the timeout immediately
        // and deterministically (a zero-duration timer races the
        // just-spawned worker's first send — a μs-fast win must not
        // re-paint a past-deadline phase green), while the detached
        // worker still ATTEMPTS the removal, bounded by the deterministic
        // process exit.
        report.record_cleanup_timed_out = true;
        eprintln!(
            "{SHUTDOWN_TIMEOUT_MARKER} phase=record_cleanup budget_ms={budget_ms} remaining_ms=0"
        );
        tracing::error!(
            phase = %ShutdownPhase::RecordCleanup,
            budget_ms,
            "shutdown phase started with ZERO remaining budget; the detached cleanup worker \
             was abandoned (its removal may or may not complete — the record may remain on disk \
             and a later instance takes over a stale record); the OS lock is released when this \
             process exits"
        );
        return report;
    }
    match tokio::time::timeout(remaining, done_rx).await {
        Ok(Ok(Ok(()))) => {
            tracing::info!(phase = %ShutdownPhase::RecordCleanup, "own instance record removed, lock released");
        }
        Ok(Ok(Err(err))) => {
            report.record_error = Some(err.to_string());
            tracing::error!(phase = %ShutdownPhase::RecordCleanup, %err, "shutdown instance-record cleanup FAILED (explicit, exit code 4)");
        }
        Ok(Err(_worker_gone)) => {
            // The worker dropped its sender without delivering a result —
            // it ended abruptly mid-cleanup (panic). The record state is
            // UNKNOWN: report it explicitly instead of a silent Ok.
            report.record_error = Some("record-cleanup worker ended without a result".to_string());
            tracing::error!(
                phase = %ShutdownPhase::RecordCleanup,
                "record-cleanup worker ended without delivering a result — the \
                 instance-record state is unknown (exit code 4)"
            );
        }
        Err(_elapsed) => {
            report.record_cleanup_timed_out = true;
            eprintln!(
                "{SHUTDOWN_TIMEOUT_MARKER} phase=record_cleanup budget_ms={budget_ms} remaining_ms={}",
                remaining.as_millis()
            );
            tracing::error!(
                phase = %ShutdownPhase::RecordCleanup,
                budget_ms,
                "shutdown phase exceeded the remaining budget; the detached cleanup worker \
                 was abandoned (its removal may or may not have completed — the record may \
                 remain on disk and a later instance takes over a stale record); the OS lock \
                 is released when this process exits"
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
        report.transport_drain_timed_out = true;
        assert_eq!(
            report.exit_code(),
            6,
            "transport drain timeout is a timeout"
        );
        report.ws_drain_timed_out = true;
        assert_eq!(report.exit_code(), 6);
        report.transport_drain_timed_out = false;
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
    async fn budget_remaining_is_anchored_at_the_signal_moment() {
        // F03: the budget counts from the signal, not from whenever a phase
        // happens to start.
        let signalled = std::time::Instant::now() - Duration::from_millis(80);
        let budget = ShutdownBudget::new(signalled, Duration::from_millis(100));
        let remaining = budget.remaining();
        assert!(
            remaining <= Duration::from_millis(100 - 75),
            "elapsed time must already count: {remaining:?}"
        );
        let exhausted = ShutdownBudget::new(
            std::time::Instant::now() - Duration::from_millis(250),
            Duration::from_millis(100),
        );
        assert_eq!(exhausted.remaining(), Duration::ZERO, "saturating at zero");
        assert_eq!(budget.total_ms(), 100);
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
