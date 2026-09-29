//! Background drive submissions (R03-T06 deliverable 后台提交接口): the
//! submission policy whose run OUTLIVES the client connection.
//!
//! Frozen semantics (target contract 02 §1: "关闭一个客户端不等于取消
//! 所有任务"):
//! - **客户端断线 ≠ 取消**: a background submission admits the run
//!   (ownership → busy gate → requestId dedup — the exact admission chain
//!   of the foreground surface) and then spawns the drive as a DETACHED
//!   supervised task of the SAME [`crate::runs::RunSupervisor`] — no
//!   second scheduler. Dropping the caller (a vanished HTTP response, a
//!   closed window) cannot touch the drive; the run settles through its
//!   own single finalize.
//! - **桌面关窗 = 客户端断线**: the desktop is just another client of
//!   the independent service process (Tauri host lifecycle = R09; the
//!   run-layer semantics are identical to a client disconnect).
//! - **服务退出**: the process exit is bounded by the MINIMAL exit hook
//!   here — [`BackgroundDriveRegistry::drain`] joins the live background
//!   drives under the remaining shutdown budget and reports the rest as
//!   unconfirmed; whatever the exit leaves behind are durable active run
//!   rows whose honest recovery classification is R03-T07 (the full exit
//!   strategy belongs there).
//! - **不重复提交**: reconnecting clients re-submit with the SAME
//!   explicit requestId + content and hit the T04 idempotent replay (the
//!   original run id returns, nothing re-executes); the same id with
//!   CHANGED content is the T04 loud conflict.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::Principal as KernelPrincipal;

use crate::events::EventService;
use crate::runs::{DriveAuthorization, RunSupervisor};
use crate::session_supervisor::SessionLease;
use crate::task_supervisor::{ChildHandle, TaskExit};

/// Hard cap of concurrently tracked background drives (service
/// protection; full is a loud refusal — never an unbounded backlog).
pub const BACKGROUND_DRIVE_CAP: usize = 1024;

/// Bound of the recent-exit diagnostic ring (the same bounded-vocabulary
/// as the other registries).
const RECENT_EXITS_CAP: usize = 256;

/// One tracked background drive (the recoverable handle of the detached
/// supervised task).
struct BackgroundDrive {
    run_id: String,
    handle: ChildHandle<()>,
}

/// The bounded registry of live background drives + the minimal
/// service-exit hook (R03-T06).
#[derive(Default)]
pub struct BackgroundDriveRegistry {
    drives: Mutex<HashMap<String, BackgroundDrive>>,
    /// Finished drives' observed exits (R03 repair G01/F02: a finished or
    /// PANICKING drive's TaskExit stays diagnosable after its slot was
    /// reclaimed — a bounded ring, never unbounded growth). Shared with
    /// the detached reclamation waiters.
    recent_exits: Arc<Mutex<Vec<(String, TaskExit)>>>,
}

impl std::fmt::Debug for BackgroundDriveRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundDriveRegistry")
            .field(
                "live",
                &self
                    .drives
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .len(),
            )
            .finish()
    }
}

/// Outcome of one bounded exit-hook drain.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackgroundDrainReport {
    /// Drives that confirmed their drive within the budget.
    pub confirmed: Vec<String>,
    /// Drives still running at budget expiry (reported — never a fake
    /// quiet; the process exit bounds them and their durable run rows
    /// stay honestly active for the R03-T07 recovery scan).
    pub unconfirmed: Vec<String>,
}

/// Loud spawn refusal: the registry is at its cap (and no finished entry
/// could free a slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundSpawnRejected {
    pub cap: usize,
}

impl std::fmt::Display for BackgroundSpawnRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "background-drive registry is at its cap ({}) — submission refused loudly",
            self.cap
        )
    }
}

impl BackgroundDriveRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Reclaims every FINISHED drive entry: each finished handle is
    /// CONSUMED by a detached waiter task (`ChildHandle::wait` records the
    /// exit into the supervised registry AND reaps its entry), and the
    /// observed exit lands in the bounded recent-exit ring (R03 repair
    /// G01/F02: neither registry leaks — a finished drive leaves BOTH the
    /// drive map and the task supervisor's entry set, and a panicking
    /// drive's [`TaskExit`] stays diagnosable).
    fn reclaim_finished(&self) {
        let finished: Vec<(String, ChildHandle<()>)> = {
            let mut drives = self
                .drives
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let ids: Vec<String> = drives
                .iter()
                .filter(|(_, drive)| drive.handle.is_finished())
                .map(|(run_id, _)| run_id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|run_id| drives.remove(&run_id).map(|drive| (run_id, drive.handle)))
                .collect()
        };
        for (run_id, handle) in finished {
            // Consume the exit: wait() records/reaps the SUPERVISED entry;
            // the observed exit (Completed / Panicked / ...) is preserved
            // in this registry's bounded ring for diagnosis.
            let ring = Arc::clone(&self.recent_exits);
            tokio::spawn(async move {
                let exit = match handle.wait().await {
                    Ok(()) => TaskExit::Completed,
                    Err(exit) => exit,
                };
                record_recent_exit(&ring, run_id.clone(), exit.clone());
                if let TaskExit::Panicked(detail) = exit {
                    tracing::error!(
                        run_id = %run_id,
                        panic = %detail,
                        "background drive PANICKED — contained at the supervised boundary; \
                         the durable run row stays honest (recovery classification is R03-T07)"
                    );
                }
            });
        }
    }

    /// The observed exits of recently finished drives (diagnosis surface:
    /// panics included), most recent last.
    pub fn recent_exits(&self) -> Vec<(String, TaskExit)> {
        self.recent_exits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Live background drives (queries/evidence). Finished entries are
    /// reclaimed (both registries) before answering.
    pub fn live_ids(&self) -> Vec<String> {
        self.reclaim_finished();
        self.drives
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// The minimal service-exit hook (R03-T06): join the live background
    /// drives under the remaining shutdown budget. Expired drives are
    /// REPORTED unconfirmed — the process exit bounds them and their
    /// durable run rows stay active for the R03-T07 recovery scan; no
    /// fake quiet, no fabricated terminals.
    pub async fn drain_within(&self, budget: std::time::Duration) -> BackgroundDrainReport {
        let budget = crate::cancel::CancelBudget::new(std::time::Instant::now(), budget);
        let mut report = BackgroundDrainReport::default();
        let drives: Vec<BackgroundDrive> = {
            let mut drives = self
                .drives
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            drives.drain().map(|(_, drive)| drive).collect()
        };
        for drive in drives {
            let remaining = budget.remaining();
            match tokio::time::timeout(remaining, drive.handle.wait()).await {
                Ok(_) => report.confirmed.push(drive.run_id),
                Err(_elapsed) => report.unconfirmed.push(drive.run_id),
            }
        }
        if !report.unconfirmed.is_empty() {
            tracing::warn!(
                unconfirmed = ?report.unconfirmed,
                budget_ms = budget.total().as_millis() as u64,
                "background-drive exit drain expired with live drives; they are reported \
                 (their durable run rows stay active — recovery classification is R03-T07)"
            );
        }
        report
    }
}

/// Appends one observed drive exit to the bounded ring (oldest dropped).
fn record_recent_exit(ring: &Arc<Mutex<Vec<(String, TaskExit)>>>, run_id: String, exit: TaskExit) {
    let mut ring = ring.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if ring.len() >= RECENT_EXITS_CAP {
        let overflow = ring.len() + 1 - RECENT_EXITS_CAP;
        ring.drain(..overflow);
    }
    ring.push((run_id, exit));
}

/// Spawns ONE background drive: the run's lifecycle runs to its single
/// finalize on the SAME supervisor, owned by a DETACHED supervised task
/// (owner `None` — no client connection and no other run's cancellation
/// owns it; the run's OWN cancellation still settles it through the
/// cancel surface). The session lease moves INTO the drive so the session
/// stays honestly busy until the background run settles.
#[allow(clippy::too_many_arguments)]
pub fn spawn_background_drive(
    supervisor: &Arc<RunSupervisor>,
    storage: &Arc<RunDatabase>,
    events: &Arc<EventService>,
    registry: &Arc<BackgroundDriveRegistry>,
    principal: KernelPrincipal,
    session_id: String,
    agent_id: String,
    run_id: String,
    input: String,
    authorization: DriveAuthorization,
    now_ms: u64,
    lease: SessionLease,
) -> Result<(), BackgroundSpawnRejected> {
    // Finished entries are reclaimed (BOTH registries — R03 repair
    // G01/F02) before the cap is consulted.
    registry.reclaim_finished();
    {
        let drives = registry
            .drives
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if drives.len() >= BACKGROUND_DRIVE_CAP {
            return Err(BackgroundSpawnRejected {
                cap: BACKGROUND_DRIVE_CAP,
            });
        }
    }
    let drive_supervisor = Arc::clone(supervisor);
    let drive_storage = Arc::clone(storage);
    let drive_events = Arc::clone(events);
    let drive_run_id = run_id.clone();
    let drive_session = session_id.clone();
    let handle = supervisor
        .task_supervisor()
        .spawn_detached(format!("background_drive:{run_id}"), async move {
            // The lease lives as long as the drive: the session frees
            // exactly when the background run settles (every exit path).
            let _lease = lease;
            let finish = drive_supervisor
                .drive_run(
                    drive_storage.as_ref(),
                    drive_events.as_ref(),
                    &principal,
                    &drive_session,
                    &agent_id,
                    &drive_run_id,
                    &input,
                    1,
                    now_ms,
                    None,
                    None,
                    authorization,
                    &drive_session,
                )
                .await;
            match finish {
                Ok(finish) => {
                    tracing::info!(
                        run_id = %drive_run_id,
                        outcome = %finish.terminal_reason(),
                        "background drive settled through the single finalize path"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        run_id = %drive_run_id,
                        error = ?err,
                        "background drive FAILED before its finalize (loud; the durable row \
                         stays honest)"
                    );
                }
            }
        })
        .map_err(|rejected| BackgroundSpawnRejected { cap: rejected.cap })?;
    registry
        .drives
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(run_id.clone(), BackgroundDrive { run_id, handle });
    Ok(())
}
