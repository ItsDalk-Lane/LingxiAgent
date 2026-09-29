//! TaskSupervisor (R03-T03 deliverable): every spawned async child has an
//! OWNER, a RECOVERABLE handle and an EXIT RESULT — fire-and-forget is
//! structurally impossible.
//!
//! What this owns:
//! - **Linked children** (`spawn_linked`): a child task bound to a run id
//!   AND a [`CancelScope`] of the run's cancellation tree. The wrapper
//!   races the child future against the scope's cancellation
//!   (`biased` — cancellation wins ties): when the tree fires, the child
//!   future is DROPPED at its await point (the real Rust cancellation
//!   primitive; the same drop a dropped HTTP request performs on its
//!   handler) and the exit is recorded as [`TaskExit::Aborted`].
//! - **Detached background** (`spawn_detached`): owned and reaped the
//!   same way, but linked to NO run scope — cancelling any run never
//!   touches it (R03-A06: 独立任务不被误杀).
//! - **Supervised exits**: panics are contained at the task boundary and
//!   returned as [`TaskExit::Panicked`] to whoever awaits the child
//!   (the driver or the bounded cleanup drain) — a panicking tool/model
//!   child can never kill the request task silently.
//! - **Bounded cleanup** (`drain_run`): after a run's tree fired, the
//!   drain waits for the run's live children under the remaining
//!   [`CancelBudget`](crate::cancel::CancelBudget); at expiry an ABORT is
//!   REQUESTED through an [`tokio::task::AbortHandle`] saved before the
//!   join (real last-resort primitive — effective at the child's next
//!   yield point) and the child is reported as `unconfirmed` — the report
//!   never claims quiet it did not observe (R03-T03 step 2/5).
//!
//! Boundary honesty: `abort()` (and the wrapper's scope-drop) take effect
//! at the child's NEXT yield point. A child that never yields (a truly
//! non-cooperating busy loop) cannot confirm its stop within any budget —
//! its abort stays merely requested and it is reported unconfirmed,
//! exactly the "无法确认停止" the taskbook requires; no false quiet is
//! produced.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use lingxi_kernel::ports::StorageError;

use crate::cancel::{CancelBudget, CancelScope, ScopeKind};

/// Diagnostic vocabulary of one supervised task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    ModelCall,
    ToolCall,
    ChildRun,
    Background,
}

impl TaskKind {
    pub fn name(self) -> &'static str {
        match self {
            TaskKind::ModelCall => "model_call",
            TaskKind::ToolCall => "tool_call",
            TaskKind::ChildRun => "child_run",
            TaskKind::Background => "background",
        }
    }
}

impl From<ScopeKind> for TaskKind {
    fn from(kind: ScopeKind) -> Self {
        match kind {
            ScopeKind::ModelCall => TaskKind::ModelCall,
            ScopeKind::ToolCall => TaskKind::ToolCall,
            ScopeKind::ChildRun => TaskKind::ChildRun,
            ScopeKind::Run | ScopeKind::Background => TaskKind::Background,
        }
    }
}

/// How one supervised task ended (the recoverable exit result).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskExit {
    /// The child future completed normally.
    Completed,
    /// The child ended without delivering its result through the
    /// supervision channel (an internal supervision anomaly — loud, never
    /// guessed into a success).
    Failed(String),
    /// The cancellation tree (or an explicit abort) dropped the child
    /// future at an await point.
    Aborted,
    /// The child PANICKED; the panic was contained at the task boundary.
    /// Carries the panic payload as a best-effort string.
    Panicked(String),
}

impl TaskExit {
    pub fn name(&self) -> &'static str {
        match self {
            TaskExit::Completed => "completed",
            TaskExit::Failed(_) => "failed",
            TaskExit::Aborted => "aborted",
            TaskExit::Panicked(_) => "panicked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskStateInner {
    Running,
    Exited(TaskExit),
}

struct TaskEntry {
    id: u64,
    label: String,
    kind: TaskKind,
    /// Owning run (`None` = detached background work).
    run_id: Option<String>,
    state: Mutex<TaskStateInner>,
    handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TaskEntry {
    fn state(&self) -> TaskStateInner {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn record_exit(&self, exit: TaskExit) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(*state, TaskStateInner::Running) {
            *state = TaskStateInner::Exited(exit);
        }
        // A later observer never overwrites the first recorded exit.
    }

    fn is_live(&self) -> bool {
        matches!(self.state(), TaskStateInner::Running)
    }
}

/// Snapshot of one supervised task (supervision queries / reports).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRef {
    pub id: u64,
    pub label: String,
    pub kind: TaskKind,
    /// Owning run (`None` = detached background).
    pub run_id: Option<String>,
    /// `None` while the task is still running.
    pub exit: Option<TaskExit>,
}

impl TaskRef {
    /// Stable machine-readable rendering (report vocabulary).
    pub fn describe(&self) -> String {
        format!(
            "{}:{} ({})",
            self.kind.name(),
            self.label,
            self.exit.as_ref().map(|e| e.name()).unwrap_or("running")
        )
    }
}

/// Result of one bounded cleanup drain.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CleanupReport {
    /// Children that confirmed their exit within the budget.
    pub confirmed: Vec<TaskRef>,
    /// Children that did NOT confirm within the budget (reported — never
    /// absorbed into a fake quiet).
    pub unconfirmed: Vec<TaskRef>,
}

impl CleanupReport {
    pub fn all_quiet(&self) -> bool {
        self.unconfirmed.is_empty()
    }
}

/// Loud spawn rejection: the supervised registry is at its cap (and no
/// reaped entry could free a slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnRejected {
    pub cap: usize,
}

impl std::fmt::Display for SpawnRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "supervised-task registry is at its cap ({}) — spawn refused loudly, \
             never an unbounded bookkeeping list",
            self.cap
        )
    }
}

impl From<SpawnRejected> for StorageError {
    fn from(rejected: SpawnRejected) -> Self {
        StorageError::InvalidRequest {
            detail: rejected.to_string(),
        }
    }
}

/// The supervisor (R03-T03 deliverable). One instance lives inside the
/// run supervisor; internal locks are held for O(1) registry work only.
pub struct TaskSupervisor {
    cap: usize,
    tasks: Mutex<HashMap<u64, Arc<TaskEntry>>>,
    next_id: AtomicU64,
}

impl Default for TaskSupervisor {
    fn default() -> Self {
        Self::new(crate::cancel::CancelPolicy::default().supervised_task_cap)
    }
}

impl TaskSupervisor {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            tasks: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    fn register(
        &self,
        label: String,
        kind: TaskKind,
        run_id: Option<String>,
    ) -> Result<(u64, Arc<TaskEntry>), SpawnRejected> {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if tasks.len() >= self.cap {
            // Reap ended entries first (their exit results were already
            // observed by their owner through wait/drain); only a registry
            // that is genuinely full of LIVE work refuses.
            tasks.retain(|_, entry| entry.is_live());
            if tasks.len() >= self.cap {
                return Err(SpawnRejected { cap: self.cap });
            }
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = Arc::new(TaskEntry {
            id,
            label,
            kind,
            run_id,
            state: Mutex::new(TaskStateInner::Running),
            handle: Mutex::new(None),
        });
        tasks.insert(id, Arc::clone(&entry));
        Ok((id, entry))
    }

    /// Spawns one LINKED child: owned by `run_id`, cancelled through
    /// `scope` (a scope of the run's cancellation tree). The child future
    /// is dropped at its await point when the scope fires — biased select
    /// so a cancellation that lands together with completion wins (取消后
    /// 不得启动新模型调用/新工具).
    pub fn spawn_linked<F>(
        self: &Arc<Self>,
        run_id: &str,
        scope: &CancelScope,
        label: String,
        fut: F,
    ) -> Result<ChildHandle<F::Output>, SpawnRejected>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let kind = TaskKind::from(scope.kind());
        let (id, entry) = self.register(label, kind, Some(run_id.to_string()))?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let cancel_scope = scope.clone();
        let task_entry = Arc::clone(&entry);
        let join = tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = cancel_scope.cancelled() => {
                    task_entry.record_exit(TaskExit::Aborted);
                    let _ = tx.send(Err(TaskExit::Aborted));
                }
                outcome = fut => {
                    task_entry.record_exit(TaskExit::Completed);
                    let _ = tx.send(Ok(outcome));
                }
            }
        });
        *entry
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(join);
        Ok(ChildHandle {
            id,
            entry,
            supervisor: Arc::downgrade(self),
            rx,
        })
    }

    /// Spawns one DETACHED background task: supervised (owner = `None`,
    /// recoverable handle, exit result) but linked to NO cancellation
    /// scope — run cancellations never touch it (R03-A06).
    pub fn spawn_detached<F>(
        self: &Arc<Self>,
        label: String,
        fut: F,
    ) -> Result<ChildHandle<F::Output>, SpawnRejected>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (id, entry) = self.register(label, TaskKind::Background, None)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task_entry = Arc::clone(&entry);
        let join = tokio::spawn(async move {
            let outcome = fut.await;
            task_entry.record_exit(TaskExit::Completed);
            let _ = tx.send(Ok(outcome));
        });
        *entry
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(join);
        Ok(ChildHandle {
            id,
            entry,
            supervisor: Arc::downgrade(self),
            rx,
        })
    }

    /// The supervised-task registry snapshot (supervision queries).
    pub fn tasks(&self) -> Vec<TaskRef> {
        self.tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .map(|entry| task_ref_of(entry))
            .collect()
    }

    /// Live (still-running) children of one run.
    pub fn live_children_of(&self, run_id: &str) -> Vec<TaskRef> {
        self.tasks()
            .into_iter()
            .filter(|task| {
                task.run_id.as_deref().is_some_and(|owner| owner == run_id) && task.exit.is_none()
            })
            .collect()
    }

    /// The exit result of one task id, if still recoverable.
    pub fn exit_of(&self, id: u64) -> Option<TaskExit> {
        self.tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&id)
            .and_then(|entry| match entry.state() {
                TaskStateInner::Running => None,
                TaskStateInner::Exited(exit) => Some(exit),
            })
    }

    fn reap(&self, id: u64) {
        self.tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&id);
    }

    /// Bounded cleanup drain of one run's children (the R03-T03 cleanup
    /// deadline policy): the tree has fired already, so linked children
    /// are exiting on their own; this joins each of them under the
    /// REMAINING budget and, at expiry, aborts and reports the rest as
    /// unconfirmed. The run's own finalize is never blocked past the
    /// budget. The joined children's exits live in the returned report
    /// (the owner's recoverable result) and leave the registry.
    pub async fn drain_run(&self, run_id: &str, budget: CancelBudget) -> CleanupReport {
        let mut report = CleanupReport::default();
        let live = self.live_children_of(run_id);
        if live.is_empty() {
            return report;
        }
        for task in live {
            let Some(entry) = self
                .tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&task.id)
                .cloned()
            else {
                continue;
            };
            let join = entry
                .handle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            let Some(join) = join else {
                // No handle to join (another reaper owns it): report the
                // OBSERVED state honestly.
                match entry.state() {
                    TaskStateInner::Exited(_) => report.confirmed.push(task_ref_of(&entry)),
                    TaskStateInner::Running => report.unconfirmed.push(task_ref_of(&entry)),
                }
                continue;
            };
            // R03-T03 review R1-D1 fix: the JoinHandle is about to be
            // MOVED into the timeout future (and dropped with it when the
            // budget expires), so the abort capability is saved BEFORE the
            // timeout — the expiry branch below now performs a REAL abort
            // instead of reading an always-empty handle slot.
            let abort_handle = join.abort_handle();
            let remaining = budget.remaining();
            match tokio::time::timeout(remaining, join).await {
                Ok(Ok(())) => {
                    // The wrapper recorded its exit (Completed/Aborted);
                    // an unrecorded finish is an internal anomaly — record
                    // it loudly instead of guessing a success.
                    if entry.is_live() {
                        entry.record_exit(TaskExit::Failed(
                            "child join finished without an exit record".to_string(),
                        ));
                    }
                    report.confirmed.push(task_ref_of(&entry));
                    self.reap(entry.id);
                }
                Ok(Err(join_err)) => {
                    let exit = if join_err.is_cancelled() {
                        TaskExit::Aborted
                    } else {
                        TaskExit::Panicked(panic_payload(join_err))
                    };
                    entry.record_exit(exit);
                    report.confirmed.push(task_ref_of(&entry));
                    self.reap(entry.id);
                }
                Err(_elapsed) => {
                    // Budget expired: abort through the handle saved BEFORE
                    // the timeout consumed the JoinHandle (R1-D1 fix — the
                    // previous read of `entry.handle` here was dead code:
                    // always `None`). The abort is the last-resort real
                    // primitive: REQUESTED here, effective at the wrapper's
                    // next yield point. The child is still REPORTED as
                    // unconfirmed — an abort that was requested is not a
                    // stop that was observed.
                    abort_handle.abort();
                    tracing::warn!(
                        task = %entry.label,
                        run_id = %run_id,
                        budget_ms = budget.total().as_millis() as u64,
                        "cleanup budget expired before this child confirmed its exit — \
                         reported unconfirmed (abort requested through the saved \
                         AbortHandle; it takes effect at the child's next yield point; \
                         no false quiet)"
                    );
                    report.unconfirmed.push(TaskRef {
                        id: entry.id,
                        label: entry.label.clone(),
                        kind: entry.kind,
                        run_id: entry.run_id.clone(),
                        exit: None,
                    });
                }
            }
        }
        report
    }
}

fn panic_payload(join_err: tokio::task::JoinError) -> String {
    join_err
        .into_panic()
        .downcast::<String>()
        .map(|msg| *msg)
        .unwrap_or_else(|payload| {
            payload
                .downcast::<&str>()
                .map(|msg| msg.to_string())
                .unwrap_or_else(|_| "opaque panic payload".to_string())
        })
}

fn task_ref_of(entry: &TaskEntry) -> TaskRef {
    TaskRef {
        id: entry.id,
        label: entry.label.clone(),
        kind: entry.kind,
        run_id: entry.run_id.clone(),
        exit: match entry.state() {
            TaskStateInner::Running => None,
            TaskStateInner::Exited(exit) => Some(exit),
        },
    }
}

/// The recoverable handle of one supervised task (linked or detached).
pub struct ChildHandle<T> {
    id: u64,
    entry: Arc<TaskEntry>,
    supervisor: Weak<TaskSupervisor>,
    rx: tokio::sync::oneshot::Receiver<Result<T, TaskExit>>,
}

impl<T> ChildHandle<T> {
    pub fn task_id(&self) -> u64 {
        self.id
    }

    pub fn label(&self) -> &str {
        &self.entry.label
    }

    /// Aborts the child (the real tokio primitive; takes effect at the
    /// child's next yield point). The exit is recorded when the task is
    /// next awaited/reaped.
    pub fn abort(&self) {
        if let Some(handle) = self
            .entry
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            handle.abort();
        }
    }

    pub fn is_finished(&self) -> bool {
        self.entry
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(true)
    }

    /// Waits for the child and returns its value, or the supervised exit
    /// (`Aborted` = the tree/explicit abort dropped the future;
    /// `Panicked` = the child panicked — contained, never crashing the
    /// waiter). Records the exit and reaps the registry entry.
    pub async fn wait(self) -> Result<T, TaskExit> {
        match self.rx.await {
            Ok(outcome) => {
                if let Some(supervisor) = self.supervisor.upgrade() {
                    supervisor.reap(self.id);
                }
                outcome
            }
            Err(_sender_gone) => {
                // The wrapper died before delivering: panic or explicit
                // abort. Resolve which from the join result.
                let join = self
                    .entry
                    .handle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                let exit = match join {
                    Some(join) => match join.await {
                        Ok(()) => {
                            TaskExit::Failed("child ended without delivering a result".to_string())
                        }
                        Err(join_err) if join_err.is_cancelled() => TaskExit::Aborted,
                        Err(join_err) => TaskExit::Panicked(panic_payload(join_err)),
                    },
                    None => match self.entry.state() {
                        TaskStateInner::Exited(exit) => exit,
                        TaskStateInner::Running => TaskExit::Failed(
                            "child handle already reaped without a result".to_string(),
                        ),
                    },
                };
                self.entry.record_exit(exit.clone());
                if let Some(supervisor) = self.supervisor.upgrade() {
                    supervisor.reap(self.id);
                }
                Err(exit)
            }
        }
    }
}

// A dropped handle without `wait` leaves the entry recoverable in the
// registry (bounded by the cap; ended entries are reaped at registry
// pressure or by the owning run's drain). The child itself is cancelled
// by the TREE (linked spawns), never orphaned mid-air.

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn linked_child_is_aborted_by_the_tree_and_returns_its_exit() {
        let supervisor = Arc::new(TaskSupervisor::new(16));
        let root = CancelScope::run_root("run_a");
        let child_scope = root.child("run_a-mc0001".to_string(), ScopeKind::ModelCall);
        let (fired_tx, fired_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = supervisor
            .spawn_linked(
                "run_a",
                &child_scope,
                "model_call:run_a-mc0001".to_string(),
                async move {
                    let _ = fired_tx.send(());
                    std::future::pending::<()>().await;
                    #[allow(unreachable_code)]
                    42
                },
            )
            .expect("spawn");
        // Let the child start and park.
        fired_rx.await.expect("child started");
        assert_eq!(supervisor.live_children_of("run_a").len(), 1);
        root.cancel("user");
        let task_id = handle.task_id();
        let exit = handle.wait().await.expect_err("cancelled child");
        assert_eq!(exit, TaskExit::Aborted);
        assert!(
            supervisor.exit_of(task_id).is_none(),
            "entry reaped by wait"
        );
        assert!(supervisor.live_children_of("run_a").is_empty());
    }

    #[tokio::test]
    async fn panicking_child_is_contained_and_returned_as_panicked() {
        let supervisor = Arc::new(TaskSupervisor::new(16));
        let root = CancelScope::run_root("run_b");
        let scope = root.child("run_b-tc0001".to_string(), ScopeKind::ToolCall);
        let handle = supervisor
            .spawn_linked(
                "run_b",
                &scope,
                "tool_call:run_b-tc0001".to_string(),
                async {
                    panic!("tool double exploded");
                },
            )
            .expect("spawn");
        let exit = handle.wait().await.expect_err("panicking child");
        match exit {
            TaskExit::Panicked(detail) => {
                assert!(detail.contains("tool double exploded"), "payload: {detail}")
            }
            other => panic!("expected Panicked, got {other:?}"),
        }
        assert!(supervisor.live_children_of("run_b").is_empty());
    }

    #[tokio::test]
    async fn detached_background_survives_a_run_cancellation_and_reaps_cleanly() {
        let supervisor = Arc::new(TaskSupervisor::new(16));
        let root = CancelScope::run_root("run_c");
        let ticks = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counter = Arc::clone(&ticks);
        let handle = supervisor
            .spawn_detached("background:heartbeat".to_string(), async move {
                for _ in 0..5 {
                    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                    counter.fetch_add(1, Ordering::Relaxed);
                }
                "done"
            })
            .expect("spawn");
        // Cancelling the run's tree must not touch the detached task.
        root.cancel("user");
        assert!(
            !handle.is_finished(),
            "detached background keeps running through a run cancellation"
        );
        let value = handle.wait().await.expect("detached completes normally");
        assert_eq!(value, "done");
        assert_eq!(ticks.load(Ordering::Acquire), 5);
    }

    #[tokio::test]
    async fn drain_confirms_exited_children_within_the_budget() {
        let supervisor = Arc::new(TaskSupervisor::new(16));
        let root = CancelScope::run_root("run_d");
        let scope = root.child("run_d-mc0001".to_string(), ScopeKind::ModelCall);
        let handle = supervisor
            .spawn_linked(
                "run_d",
                &scope,
                "model_call:run_d-mc0001".to_string(),
                async {
                    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                    7
                },
            )
            .expect("spawn");
        root.cancel("user");
        drop(handle); // the driver may drop the handle without waiting
        let budget = CancelBudget::new(
            std::time::Instant::now(),
            std::time::Duration::from_millis(500),
        );
        let report = supervisor.drain_run("run_d", budget).await;
        assert!(report.all_quiet(), "report: {report:?}");
        assert_eq!(report.confirmed.len(), 1);
        assert_eq!(report.confirmed[0].exit, Some(TaskExit::Aborted));
        assert!(supervisor.live_children_of("run_d").is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn noncooperating_child_is_reported_unconfirmed_never_fake_quiet() {
        let supervisor = Arc::new(TaskSupervisor::new(16));
        let root = CancelScope::run_root("run_e");
        let scope = root.child("run_e-child".to_string(), ScopeKind::ChildRun);
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started_flag = Arc::clone(&started);
        let handle = supervisor
            .spawn_linked(
                "run_e",
                &scope,
                "child_run:noncooperating".to_string(),
                async move {
                    started_flag.store(true, Ordering::Release);
                    // A truly non-cooperating child: never awaits, so the
                    // cancellation drop cannot land (bounded by self-exit).
                    let deadline =
                        std::time::Instant::now() + std::time::Duration::from_millis(300);
                    while std::time::Instant::now() < deadline {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                },
            )
            .expect("spawn");
        while !started.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        root.cancel("user");
        drop(handle);
        let budget = CancelBudget::new(
            std::time::Instant::now(),
            std::time::Duration::from_millis(50),
        );
        let report = supervisor.drain_run("run_e", budget).await;
        assert!(
            !report.all_quiet(),
            "a non-cooperating child must NOT be reported quiet"
        );
        assert_eq!(report.unconfirmed.len(), 1);
        assert_eq!(report.unconfirmed[0].label, "child_run:noncooperating");
    }

    /// R03-T03 review R1-D1 regression: the drain's expiry branch must
    /// REALLY abort. Before the fix, the `JoinHandle` was moved into the
    /// timeout future and the expiry branch re-read `entry.handle` —
    /// always `None` after the take — so the claimed "abort sent" was dead
    /// code. A yielding child parked mid-future (no tree cancellation —
    /// the drain runs here purely on its own budget) proves the fix: when
    /// the budget expires, the saved AbortHandle drops the wrapper and the
    /// child future observes its own Drop.
    #[tokio::test]
    async fn drain_expiry_branch_really_aborts_a_yielding_child() {
        let supervisor = Arc::new(TaskSupervisor::new(16));
        let root = CancelScope::run_root("run_d1");
        let scope = root.child("run_d1-child".to_string(), ScopeKind::ToolCall);
        struct DropProbe(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let future_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = Arc::clone(&future_dropped);
        let handle = supervisor
            .spawn_linked(
                "run_d1",
                &scope,
                "tool_call:yielding-parked".to_string(),
                async move {
                    let _probe = DropProbe(probe);
                    // A yielding child that would otherwise run for a long
                    // time: it parks at await points, so an abort lands.
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                },
            )
            .expect("spawn");
        let _ = handle; // dropped: the drain owns the reaping from here
                        // NO tree cancellation — only the drain's own small budget expiring.
        let budget = CancelBudget::new(
            std::time::Instant::now(),
            std::time::Duration::from_millis(20),
        );
        let report = supervisor.drain_run("run_d1", budget).await;
        assert!(!report.all_quiet(), "expiry must report unconfirmed");
        assert_eq!(report.unconfirmed.len(), 1);
        // The regression assertion: the saved AbortHandle REALLY aborted
        // the wrapper, which dropped the child future (observed through the
        // Drop probe) — not just a reported-unconfirmed no-op.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while !future_dropped.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < deadline,
                "expiry abort never dropped the child future (R1-D1 regression)"
            );
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }

    #[tokio::test]
    async fn registry_cap_refuses_loudly_and_reaps_ended_entries() {
        let supervisor = Arc::new(TaskSupervisor::new(2));
        let root = CancelScope::run_root("run_f");
        let scope_a = root.child("a".to_string(), ScopeKind::ToolCall);
        let scope_b = root.child("b".to_string(), ScopeKind::ToolCall);
        let _a = supervisor
            .spawn_linked("run_f", &scope_a, "a".to_string(), async { 1 })
            .expect("a");
        let b = supervisor
            .spawn_linked("run_f", &scope_b, "b".to_string(), async { 2 })
            .expect("b");
        let scope_c = root.child("c".to_string(), ScopeKind::ToolCall);
        assert!(
            supervisor
                .spawn_linked("run_f", &scope_c, "c".to_string(), async { 3 })
                .is_err(),
            "cap of live entries refuses"
        );
        assert_eq!(b.wait().await.expect("b completes"), 2);
        // The ended entry was reaped by wait: one slot is free again.
        assert!(
            supervisor
                .spawn_linked("run_f", &scope_c, "c2".to_string(), async { 3 })
                .is_ok(),
            "reaped entries free their slot"
        );
    }
}
