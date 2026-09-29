//! TaskSupervisor (R03-T03 deliverable): every spawned async child has an
//! OWNER, a RECOVERABLE handle and an EXIT RESULT — fire-and-forget is
//! structurally impossible.
//!
//! What this owns:
//! - **Linked children** (`spawn_linked`): a child task bound to a run id
//!   AND a [`CancelScope`] of the run's cancellation tree. The wrapper
//!   races the child future against the scope's cancellation
//!   (`biased` — cancellation wins ties).
//!   R03 repair G01/F02 distinguishes TWO shapes here:
//!   - *Call-level children* (model calls, tool calls, approval waits):
//!     when the tree fires the child future is DROPPED at its await point
//!     (the real Rust cancellation primitive; the same drop a dropped
//!     HTTP request performs on its handler) and the exit is recorded as
//!     [`TaskExit::Aborted`].
//!   - *Run-level children* ([`TaskKind::ChildRun`] — subagent child
//!     runs): a tree cancellation OPENS A BOUNDED COOPERATIVE WINDOW
//!     instead of dropping immediately. The child future keeps running
//!     so its own drive can walk the LEGAL cancellation path (durable
//!     `cancelling` leg, child cleanup, receipt finalize, single
//!     `cancelled` finalize, bookkeeping tail) — the drop becomes the
//!     LAST RESORT at the window's expiry (anchored at the scope's first
//!     cancellation moment plus the supervisor's cleanup grace). A
//!     healthy child closes itself out in milliseconds; only a child
//!     that ignores its cancellation is force-dropped, exactly the
//!     "先协作、后强制" the audit requires.
//! - **Detached background** (`spawn_detached`): owned and reaped the
//!   same way, but linked to NO run scope — cancelling any run never
//!   touches it (R03-A06: 独立任务不被误杀).
//! - **Supervised exits**: panics are CONTAINED at the task boundary by
//!   the wrapper itself (a `catch_unwind` around every poll — R03
//!   repair G01/F02: the exit is recorded IN-BAND as
//!   [`TaskExit::Panicked`] and is queryable even when NOBODY ever
//!   awaits the handle; a panicking child can never kill the request
//!   task silently, and a fire-and-forget handle can never lose the
//!   panic diagnosis).
//! - **Bounded cleanup** (`drain_run`): after a run's tree fired, the
//!   drain waits for the run's live children under the remaining
//!   [`CancelBudget`](crate::cancel::CancelBudget); at expiry an ABORT is
//!   REQUESTED through an [`tokio::task::AbortHandle`] saved before the
//!   join (real last-resort primitive — effective at the child's next
//!   yield point) and the child is reported as `unconfirmed` — the report
//!   never claims quiet it did not observe. R03 repair G01/F02: the
//!   expiry branch also leaves a detached COMPLETION OBSERVER that owns
//!   the JoinHandle and records the exit once the abort actually lands —
//!   "requested an abort" is never conflated with "observed the exit",
//!   and the entry can never stay a handle-less Running ghost forever.
//!
//! Boundary honesty: `abort()` (and the last-resort drop) take effect
//! at the child's NEXT yield point. A child that never yields (a truly
//! non-cooperating busy loop) cannot confirm its stop within any budget —
//! its abort stays merely requested and it is reported unconfirmed,
//! exactly the "无法确认停止" the taskbook requires; no false quiet is
//! produced.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

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
    /// The cooperative-cancel window granted to RUN-LEVEL children
    /// (`ChildRun`) when their scope's tree fires (R03 repair G01/F02):
    /// the child keeps running its own cancellation path for this long,
    /// anchored at the scope's first cancellation moment, before the
    /// last-resort drop. The composition root passes the run's cleanup
    /// policy grace so a child's window and its owner's drain share ONE
    /// budget anchor.
    cooperative_grace: Duration,
}

impl Default for TaskSupervisor {
    fn default() -> Self {
        Self::new(crate::cancel::CancelPolicy::default().supervised_task_cap)
    }
}

impl TaskSupervisor {
    pub fn new(cap: usize) -> Self {
        Self::with_cooperative_grace(
            cap,
            Duration::from_millis(crate::cancel::CancelPolicy::DEFAULT_CLEANUP_GRACE_MS),
        )
    }

    /// Constructs the supervisor with an explicit cooperative-cancel
    /// window for run-level children (tests inject small windows; the
    /// composition root passes the cancel policy's cleanup grace).
    pub fn with_cooperative_grace(cap: usize, cooperative_grace: Duration) -> Self {
        Self {
            cap,
            tasks: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            cooperative_grace,
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
            // observed by their owner through wait/drain — or recorded
            // in-band by the supervised wrapper); only a registry that is
            // genuinely full of LIVE work refuses.
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
    /// `scope` (a scope of the run's cancellation tree) — biased select
    /// so a cancellation that lands together with completion wins (取消后
    /// 不得启动新模型调用/新工具).
    ///
    /// R03 repair G01/F02 — the TWO shapes of tree cancellation:
    /// - CALL-level children (model/tool/approval): the child future is
    ///   dropped at its await point when the scope fires (the real Rust
    ///   cancellation primitive) — recorded [`TaskExit::Aborted`].
    /// - RUN-level children (`ChildRun`, e.g. subagent child runs): a
    ///   bounded COOPERATIVE WINDOW opens instead — the child future
    ///   keeps being polled so its own drive can finish its durable
    ///   cancellation path and its bookkeeping tail; at window expiry
    ///   the future is dropped as the LAST RESORT (then `Aborted`).
    ///
    /// Panics are contained in BOTH shapes and recorded in-band.
    pub fn spawn_linked<F>(
        self: &Arc<Self>,
        run_id: &str,
        scope: &CancelScope,
        label: String,
        fut: F,
    ) -> Result<ChildHandle<F::Output>, SpawnRejected>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let kind = TaskKind::from(scope.kind());
        let (id, entry) = self.register(label, kind, Some(run_id.to_string()))?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let cancel_scope = scope.clone();
        let task_entry = Arc::clone(&entry);
        let grace = self.cooperative_grace;
        let join = tokio::spawn(async move {
            let mut fut = Box::pin(fut);
            if !matches!(kind, TaskKind::ChildRun) {
                // CALL-level: the scope's cancellation drops the child at
                // its await point (biased — cancel wins ties).
                tokio::select! {
                    biased;
                    _ = cancel_scope.cancelled() => {
                        task_entry.record_exit(TaskExit::Aborted);
                        let _ = tx.send(Err(TaskExit::Aborted));
                    }
                    exit = PanicGuard { fut: fut.as_mut() } => {
                        deliver_supervised_exit(&task_entry, tx, exit);
                    }
                }
                return;
            }
            // RUN-level: completion, or the tree fires and a bounded
            // cooperative window opens.
            let tree_fired = tokio::select! {
                biased;
                _ = cancel_scope.cancelled() => true,
                exit = PanicGuard { fut: fut.as_mut() } => {
                    deliver_supervised_exit(&task_entry, tx, exit);
                    return;
                }
            };
            if !tree_fired {
                return;
            }
            // The window is anchored at the scope's FIRST cancellation
            // moment (the same anchor the owning run's cleanup budget
            // uses): a child that reached cleanup late keeps only the
            // remainder — never a fresh full budget.
            let deadline = cancel_scope
                .cancelled_at()
                .map(|at| tokio::time::Instant::from_std(at) + grace)
                .unwrap_or_else(|| tokio::time::Instant::now() + grace);
            let outcome = {
                // Scoped so the guarded borrow (and the child future it
                // guards, on expiry) ends BEFORE the exit is recorded.
                let guarded = PanicGuard { fut: fut.as_mut() };
                tokio::time::timeout_at(deadline, guarded).await
            };
            match outcome {
                Ok(exit) => deliver_supervised_exit(&task_entry, tx, exit),
                Err(_elapsed) => {
                    // Last resort: the timeout consumed (and dropped) the
                    // guarded future above — the child's own Drop cleanup
                    // (guards, permit releases) ran with it. Record the
                    // honest supervised abort; its durable run row stays
                    // whatever it last honestly wrote (recovery
                    // classification is R03-T07).
                    tracing::warn!(
                        task = %task_entry.label,
                        grace_ms = grace.as_millis() as u64,
                        "run-level child ignored its cooperative cancellation window —                          last-resort drop applied"
                    );
                    task_entry.record_exit(TaskExit::Aborted);
                    let _ = tx.send(Err(TaskExit::Aborted));
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
    /// scope — run cancellations never touch it (R03-A06). Panics are
    /// contained at the wrapper and recorded IN-BAND (R03 repair
    /// G01/F02: a fire-and-forget handle never loses the diagnosis).
    pub fn spawn_detached<F>(
        self: &Arc<Self>,
        label: String,
        fut: F,
    ) -> Result<ChildHandle<F::Output>, SpawnRejected>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (id, entry) = self.register(label, TaskKind::Background, None)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task_entry = Arc::clone(&entry);
        let join = tokio::spawn(async move {
            let mut fut = Box::pin(fut);
            let exit = PanicGuard { fut: fut.as_mut() }.await;
            deliver_supervised_exit(&task_entry, tx, exit);
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
            let Some(mut join) = join else {
                // No handle to join (another reaper owns it): report the
                // OBSERVED state honestly.
                match entry.state() {
                    TaskStateInner::Exited(_) => report.confirmed.push(task_ref_of(&entry)),
                    TaskStateInner::Running => report.unconfirmed.push(task_ref_of(&entry)),
                }
                continue;
            };
            // R03-T03 review R1-D1 fix: the abort capability is saved
            // BEFORE the join. R03 repair G01/F02: the JoinHandle itself
            // now stays owned by THIS frame (the timeout polls `&mut
            // join`), so an expired budget hands it to a detached
            // completion OBSERVER below instead of dropping it into the
            // detached void — the abort can be requested AND the actual
            // exit later observed.
            let abort_handle = join.abort_handle();
            let remaining = budget.remaining();
            match tokio::time::timeout(remaining, &mut join).await {
                Ok(Ok(())) => {
                    // The wrapper recorded its exit in-band
                    // (Completed/Aborted/Panicked); an unrecorded finish is
                    // an internal anomaly — record it loudly instead of
                    // guessing a success.
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
                    // Budget expired: abort through the saved handle — the
                    // last-resort real primitive, REQUESTED here and
                    // effective at the wrapper's next yield point. The
                    // child is still REPORTED as unconfirmed — an abort
                    // that was requested is not a stop that was observed.
                    abort_handle.abort();
                    tracing::warn!(
                        task = %entry.label,
                        run_id = %run_id,
                        budget_ms = budget.total().as_millis() as u64,
                        "cleanup budget expired before this child confirmed its exit — \
                         reported unconfirmed (abort requested through the saved \
                         AbortHandle; a detached completion observer will record the \
                         exit when it actually lands; no false quiet)"
                    );
                    // R03 repair G01/F02 (F02-C04): keep a recoverable
                    // completion observation — the observer owns the
                    // JoinHandle and records the exit once the abort
                    // actually lands (or the task finishes on its own),
                    // so the entry can never stay a handle-less Running
                    // ghost forever; the registry reaps it at pressure.
                    let observer_entry = Arc::clone(&entry);
                    tokio::spawn(async move {
                        let exit = match join.await {
                            Ok(()) if observer_entry.is_live() => TaskExit::Failed(
                                "child join finished without an exit record".to_string(),
                            ),
                            Ok(()) => return, // already recorded in-band
                            Err(join_err) if join_err.is_cancelled() => TaskExit::Aborted,
                            Err(join_err) => TaskExit::Panicked(panic_payload(join_err)),
                        };
                        observer_entry.record_exit(exit);
                    });
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

/// The supervised outcome of one polled child future: its value, or the
/// contained panic payload.
enum SupervisedPoll<T> {
    Completed(T),
    Panicked(String),
}

/// A poll-level panic guard (R03 repair G01/F02): wraps a pinned child
/// future and CATCHES panics at every poll — the supervised wrapper task
/// itself never panics, so the exit is always recorded IN-BAND and
/// queryable even when nobody ever awaits the handle. The guard only
/// BORROWS the future (a `Pin<&mut F>`), so the wrapper's `select!`
/// branches can drop the guard without dropping the child (the
/// cooperative-window shape depends on exactly that).
struct PanicGuard<'a, F: Future> {
    fut: Pin<&'a mut F>,
}

impl<F: Future> Future for PanicGuard<'_, F> {
    type Output = SupervisedPoll<F::Output>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let PanicGuard { fut } = self.get_mut();
        let poll_result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fut.as_mut().poll(cx)));
        match poll_result {
            Ok(std::task::Poll::Ready(value)) => {
                std::task::Poll::Ready(SupervisedPoll::Completed(value))
            }
            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
            Err(payload) => {
                std::task::Poll::Ready(SupervisedPoll::Panicked(panic_message_of(&payload)))
            }
        }
    }
}

/// Best-effort panic payload rendering (the same vocabulary
/// `ChildHandle::wait` uses for uncaught wrapper panics).
fn panic_message_of(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    "opaque panic payload".to_string()
}

/// Records one supervised poll outcome and delivers it to the handle
/// (consumes the one-shot sender — exactly one delivery per child).
fn deliver_supervised_exit<T>(
    entry: &Arc<TaskEntry>,
    tx: tokio::sync::oneshot::Sender<Result<T, TaskExit>>,
    exit: SupervisedPoll<T>,
) {
    match exit {
        SupervisedPoll::Completed(value) => {
            entry.record_exit(TaskExit::Completed);
            let _ = tx.send(Ok(value));
        }
        SupervisedPoll::Panicked(detail) => {
            tracing::error!(
                task = %entry.label,
                panic = %detail,
                "supervised child PANICKED — contained at the task boundary and recorded                  in-band (never a silent loss, never a crashed waiter)"
            );
            entry.record_exit(TaskExit::Panicked(detail.clone()));
            let _ = tx.send(Err(TaskExit::Panicked(detail)));
        }
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

    /// R03 repair G01/F02: a RUN-LEVEL child (ChildRun) whose scope fires
    /// gets a bounded COOPERATIVE WINDOW — the child future is NOT
    /// dropped while it finishes its own cancellation path; only a child
    /// that ignores the window is force-dropped at expiry (Aborted).
    #[tokio::test]
    async fn run_level_child_finishes_its_own_cancellation_within_the_window() {
        let supervisor = Arc::new(TaskSupervisor::with_cooperative_grace(
            8,
            Duration::from_millis(300),
        ));
        let root = CancelScope::run_root("run_coop");
        let scope = root.child("child_run:coop".to_string(), ScopeKind::ChildRun);
        let observed_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&observed_cancel);
        let handle = supervisor
            .spawn_linked(
                "run_coop",
                &scope,
                "child_run:coop".to_string(),
                async move {
                    // The child's own cancellation path: it observes the
                    // scope, does a little work, and completes on its own
                    // (exactly what a drive's settle_cancellation does).
                    let mut ticks = 0u32;
                    loop {
                        if flag.load(std::sync::atomic::Ordering::Acquire) {
                            break;
                        }
                        ticks += 1;
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    format!("settled after {ticks} ticks")
                },
            )
            .expect("spawn");
        // Start the child, then fire the tree: the cooperative window
        // opens and the child KEEPS RUNNING until it observes the flag.
        tokio::time::sleep(Duration::from_millis(10)).await;
        root.cancel("user");
        tokio::time::sleep(Duration::from_millis(40)).await;
        // The window is still open: the child future is alive, NOT yet
        // aborted (it is mid-own-cancellation).
        assert!(
            !handle.is_finished(),
            "the cooperative window keeps the run-level child alive while it \
             finishes its own cancellation path"
        );
        observed_cancel.store(true, std::sync::atomic::Ordering::Release);
        // The child completes NORMALLY through its own tail.
        let value = handle.wait().await.expect("cooperative completion");
        assert!(value.starts_with("settled after"));
    }

    /// R03 repair G01/F02: a run-level child that IGNORES its cooperative
    /// window (never finishes) is force-dropped at expiry — Aborted, the
    /// honest last resort (never an unbounded wait).
    #[tokio::test]
    async fn run_level_child_ignoring_the_window_is_dropped_at_expiry() {
        let supervisor = Arc::new(TaskSupervisor::with_cooperative_grace(
            8,
            Duration::from_millis(80),
        ));
        let root = CancelScope::run_root("run_coop2");
        let scope = root.child("child_run:stubborn".to_string(), ScopeKind::ChildRun);
        struct DropProbe(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        let future_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = Arc::clone(&future_dropped);
        let handle = supervisor
            .spawn_linked(
                "run_coop2",
                &scope,
                "child_run:stubborn".to_string(),
                async move {
                    let _probe = DropProbe(probe);
                    std::future::pending::<()>().await;
                },
            )
            .expect("spawn");
        let task_id = handle.task_id();
        root.cancel("user");
        // The window expires (80ms) and the future is REALLY dropped
        // (the Drop probe fires) — the last resort is a real drop.
        let exit = handle.wait().await.expect_err("last-resort abort");
        assert_eq!(exit, TaskExit::Aborted);
        assert!(
            future_dropped.load(std::sync::atomic::Ordering::Acquire),
            "the expiry drop ran the child's Drop cleanup"
        );
        assert_eq!(supervisor.exit_of(task_id), None, "reaped by wait");
    }

    /// R03 repair G01/F02: the never-first-polled window — a tree that
    /// fires BEFORE the wrapper task polls the child still records the
    /// supervised abort (deterministic on the current-thread runtime:
    /// the cancel lands synchronously before any yield).
    #[tokio::test(flavor = "current_thread")]
    async fn tree_firing_before_the_first_poll_still_records_the_abort() {
        let supervisor = Arc::new(TaskSupervisor::new(8));
        let root = CancelScope::run_root("run_prepoll");
        let scope = root.child("tool_call:never-polled".to_string(), ScopeKind::ToolCall);
        let polled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&polled);
        let handle = supervisor
            .spawn_linked(
                "run_prepoll",
                &scope,
                "tool_call:never-polled".to_string(),
                async move {
                    flag.store(true, std::sync::atomic::Ordering::Release);
                    std::future::pending::<()>().await;
                },
            )
            .expect("spawn");
        // Synchronous cancel BEFORE any yield: the wrapper task has not
        // polled the child future yet.
        root.cancel("user");
        let exit = handle.wait().await.expect_err("aborted before first poll");
        assert_eq!(exit, TaskExit::Aborted);
        assert!(
            !polled.load(std::sync::atomic::Ordering::Acquire),
            "the child future was never polled (the pre-poll window)"
        );
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
