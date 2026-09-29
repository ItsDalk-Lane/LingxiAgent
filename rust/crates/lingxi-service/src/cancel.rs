//! Cancellation tree + four-phase cancellation semantics + the cleanup
//! deadline policy (R03-T03 deliverables "取消树" and "清理期限策略").
//!
//! Frozen incumbent semantics this maps (read from the Node production
//! stack + P02/CANCELLATION_MAP before mapping — no new interaction was
//! invented):
//! - **The tree**: one cancellation must propagate to the work it OWNS,
//!   never just flip a UI status. The incumbent's `abortSession`
//!   (`core/session-coordinator.ts:5477`) fans out to tool executions,
//!   child tasks, subagent runs, deferred results, approval pendings,
//!   terminals and browser sidecars; the Rust mapping is an explicit
//!   scope TREE: run → child attempts/model calls/tool calls/child runs,
//!   where cancelling a parent eagerly cancels every live descendant.
//! - **Independent background survives**: only work that explicitly
//!   linked itself to a run's scope is cancelled — an unrelated
//!   background task never shares a run scope, exactly like the
//!   incumbent's task registry aborting `abortByParentSession` scoped to
//!   ONE parent while everything else keeps running (R03-A06).
//! - **Requesting ≠ clean ≠ confirmed**: the incumbent distinguishes the
//!   control-plane abort mark (`STATE_TRANSITIONS` R6: "abort 标记
//!   isAborted；终态仍由 R4 落") from the run's terminal state and from
//!   the sidecar cleanup (`_cleanupAbortedSessionSidecars`, each step
//!   warn-not-block). The Rust mapping names the four phases explicitly
//!   ([`CancelPhase`]) and NEVER reports "quiet" it did not observe: when
//!   the cleanup budget expires, the un-confirmed items are reported
//!   (phase [`CancelPhase::StopUnconfirmed`]), mirroring the R02 shutdown
//!   coordinator's honest timeout reporting.
//! - **External actions are not promised rolled back**: cancellation
//!   stops the managed work and settles the run; whatever side effects
//!   already left the process stay happened (the incumbent records
//!   "provider acceptance is unknown" instead of retrying — same
//!   honesty, see [`CancelPhase::StopUnconfirmed`] and the run's
//!   `RunFinish::Cancelled` detail).
//!
//! The tree is process-internal by design: like the incumbent's
//! `AbortSignal` (never serialized across the wire as JSON), only the
//! *intent* arrives from outside; scopes are minted inside this process.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// What a scope supervises (diagnostic vocabulary of the tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    /// The root scope of one user task (run).
    Run,
    /// One model call (a network stream in R05 terms).
    ModelCall,
    /// One tool call.
    ToolCall,
    /// A child run (subagent-shaped work linked to this run; the real
    /// subagent surface is R03-T06).
    ChildRun,
    /// Independent background work (never linked to a run scope).
    Background,
}

impl ScopeKind {
    pub fn name(self) -> &'static str {
        match self {
            ScopeKind::Run => "run",
            ScopeKind::ModelCall => "model_call",
            ScopeKind::ToolCall => "tool_call",
            ScopeKind::ChildRun => "child_run",
            ScopeKind::Background => "background",
        }
    }
}

struct ScopeInner {
    label: String,
    kind: ScopeKind,
    cancelled: std::sync::atomic::AtomicBool,
    reason: Mutex<Option<String>>,
    cancelled_at: Mutex<Option<Instant>>,
    notify: tokio::sync::Notify,
    parent: Option<Arc<ScopeInner>>,
    children: Mutex<Vec<Weak<ScopeInner>>>,
}

impl std::fmt::Debug for ScopeInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancelScope")
            .field("label", &self.label)
            .field("kind", &self.kind.name())
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
            .finish_non_exhaustive()
    }
}

/// One node of the cancellation tree. Cheap to clone (an `Arc` inside).
///
/// Cancellation flows DOWN eagerly: cancelling a scope cancels every live
/// descendant in the same call. Ascending is deliberately impossible — a
/// child's work ending never cancels its parent (a model call completing
/// is not the run ending; that distinction is R03-T01's identity layer).
#[derive(Debug, Clone)]
pub struct CancelScope {
    inner: Arc<ScopeInner>,
}

impl CancelScope {
    /// The root scope of one run.
    pub fn run_root(run_id: &str) -> Self {
        Self::new(format!("run:{run_id}"), ScopeKind::Run, None)
    }

    fn new(label: String, kind: ScopeKind, parent: Option<Arc<ScopeInner>>) -> Self {
        Self {
            inner: Arc::new(ScopeInner {
                label,
                kind,
                cancelled: std::sync::atomic::AtomicBool::new(false),
                reason: Mutex::new(None),
                cancelled_at: Mutex::new(None),
                notify: tokio::sync::Notify::new(),
                parent,
                children: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Derives a CHILD scope: it inherits the parent's cancellation (the
    /// parent's `cancel` reaches it through the tree) while keeping its
    /// own label/kind for supervision and cleanup reporting.
    pub fn child(&self, label: String, kind: ScopeKind) -> CancelScope {
        let scope = Self::new(label, kind, Some(Arc::clone(&self.inner)));
        self.inner
            .children
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(Arc::downgrade(&scope.inner));
        scope
    }

    pub fn label(&self) -> &str {
        &self.inner.label
    }

    pub fn kind(&self) -> ScopeKind {
        self.inner.kind
    }

    /// Fires cancellation through this subtree: this scope AND every live
    /// descendant flip to cancelled and every waiter wakes. Idempotent —
    /// only the FIRST call on a scope wins (returns `true`); a second
    /// reason never overwrites the first (the incumbent keeps the first
    /// abort reason the same way).
    pub fn cancel(&self, reason: &str) -> bool {
        let mut first_anywhere = false;
        cancel_recursive(&self.inner, reason, &mut first_anywhere);
        first_anywhere
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner
            .cancelled
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// The first cancellation reason recorded on this scope (ancestors are
    /// not consulted — a cancelled ancestor means `is_cancelled` is true
    /// via propagation, and the ancestor's own reason is queryable there).
    pub fn reason(&self) -> Option<String> {
        self.inner
            .reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// When the first cancellation of this scope fired.
    pub fn cancelled_at(&self) -> Option<Instant> {
        *self
            .inner
            .cancelled_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Resolves when this scope is cancelled (flag check + broadcast
    /// wakeups — safe against missed notifications).
    pub async fn cancelled(&self) {
        while !self.is_cancelled() {
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    /// Live descendant labels (supervision/cleanup observability; the
    /// scope tree is walked depth-first, dead weak links pruned).
    pub fn live_descendant_labels(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_live_descendants(&self.inner, &mut out);
        out
    }

    /// The labels from the tree ROOT down to this scope (diagnostics:
    /// where a cancelled child sits in the cancellation tree).
    pub fn path(&self) -> Vec<String> {
        let mut chain = Vec::new();
        let mut current = Some(Arc::clone(&self.inner));
        while let Some(node) = current {
            chain.push(format!("{}:{}", node.kind.name(), node.label));
            current = node.parent.clone();
        }
        chain.reverse();
        chain
    }
}

fn cancel_recursive(inner: &Arc<ScopeInner>, reason: &str, first_anywhere: &mut bool) {
    let already = inner
        .cancelled
        .swap(true, std::sync::atomic::Ordering::AcqRel);
    if !already {
        *inner
            .reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reason.to_string());
        *inner
            .cancelled_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
        *first_anywhere = true;
    }
    inner.notify.notify_waiters();
    // Children are taken and RESTORED under the same lock hold so a
    // concurrent `child()` registration on this scope can never be lost;
    // the recursion locks only descendant mutexes (a tree has no cycles,
    // and no path ascends), so holding this lock is deadlock-free.
    let mut guard = inner
        .children
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let children = std::mem::take(&mut *guard);
    for child in children.iter().filter_map(Weak::upgrade) {
        cancel_recursive(&child, reason, first_anywhere);
    }
    *guard = children;
    drop(guard);
}

fn collect_live_descendants(inner: &Arc<ScopeInner>, out: &mut Vec<String>) {
    let mut guard = inner
        .children
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut children = std::mem::take(&mut *guard);
    children.retain(|weak| weak.strong_count() > 0);
    let upgraded: Vec<Arc<ScopeInner>> = children.iter().filter_map(Weak::upgrade).collect();
    *guard = children;
    drop(guard);
    for child in upgraded {
        out.push(format!("{}:{}", child.kind.name(), child.label));
        collect_live_descendants(&child, out);
    }
}

// ── four-phase cancellation ─────────────────────────────────────────────────

/// The four distinguished phases of one cancellation (taskbook R03-T03
/// step 2: 区别请求取消、清理完成和无法确认停止). A run that was never
/// cancelled reports [`CancelPhase::Active`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelPhase {
    /// No cancellation was requested (the run may still be driving or may
    /// have settled normally).
    Active,
    /// Phase 1 — 收到取消: the cancellation intent arrived and the scope
    /// tree fired; the managed work has NOT necessarily reacted yet.
    Requested { reason: String },
    /// Phase 2 — 开始清理: the driver observed the request, persisted the
    /// durable `cancelling` phase and is draining its supervised children
    /// under the cleanup budget.
    Cleaning { reason: String },
    /// Phase 3 — 已确认终止: every supervised child of the run confirmed
    /// its exit within the budget and the single finalize committed (or is
    /// committing) the `cancelled` terminal.
    ConfirmedTerminated {
        reason: String,
        /// Labels of the children that confirmed their exit.
        confirmed: Vec<String>,
    },
    /// Phase 4 — 无法确认停止: the cleanup budget expired with live
    /// children. The run still settles (the cancellation itself is not
    /// revoked) but NOTHING claims the system went quiet: the un-confirmed
    /// items are listed here and in the run's terminal detail. External
    /// actions those children already performed are NOT promised rolled
    /// back.
    StopUnconfirmed {
        reason: String,
        confirmed: Vec<String>,
        /// Labels of the children that did NOT confirm within the budget.
        unconfirmed: Vec<String>,
    },
    /// The run's driving future disappeared BEFORE any finalize (crash,
    /// dropped transport request or process exit): the scope tree fired so
    /// linked children stop, but NO terminal state was written — the
    /// durable run row honestly stays active and its recovery
    /// classification belongs to R03-T07's startup scan.
    Abandoned { reason: String },
}

impl CancelPhase {
    /// True once a cancellation was requested at all.
    pub fn cancel_requested(&self) -> bool {
        !matches!(self, CancelPhase::Active)
    }

    /// A short machine-readable phase name (evidence vocabulary).
    pub fn phase_name(&self) -> &'static str {
        match self {
            CancelPhase::Active => "active",
            CancelPhase::Requested { .. } => "requested",
            CancelPhase::Cleaning { .. } => "cleaning",
            CancelPhase::ConfirmedTerminated { .. } => "confirmed_terminated",
            CancelPhase::StopUnconfirmed { .. } => "stop_unconfirmed",
            CancelPhase::Abandoned { .. } => "abandoned",
        }
    }
}

/// One live run's cancellation state (registry entry).
pub struct RunCancelEntry {
    pub run_id: String,
    /// The run's ROOT scope — children (attempts' model/tool calls, child
    /// runs) derive from it and inherit its cancellation.
    pub scope: CancelScope,
    phase: Mutex<CancelPhase>,
    registered_at: Instant,
}

impl RunCancelEntry {
    pub fn phase(&self) -> CancelPhase {
        self.phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Driver-side phase advance (R03-T03 four-phase machine). The
    /// registry's `fire` owns `Active → Requested`; the driver owns the
    /// later legs (Cleaning, Confirmed/Unconfirmed, Abandoned).
    pub fn advance_phase(&self, phase: CancelPhase) {
        *self
            .phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = phase;
    }

    pub fn registered_at(&self) -> Instant {
        self.registered_at
    }
}

/// Outcome of a cancellation request against the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FireOutcome {
    /// The request fired the run's tree (first cancellation wins).
    Fired,
    /// The run is live but a cancellation was already in flight.
    AlreadyCancelling,
    /// No live run with this id is registered in this process.
    NotLive,
}

/// The cleanup deadline policy (taskbook R03-T03 deliverable "清理期限
/// 策略"; injected through `ServiceDeps`, degenerate values refuse
/// startup loudly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelPolicy {
    /// The ONE cleanup budget per cancellation, anchored at the moment the
    /// cancellation request fired (mirror of the R02 shutdown budget):
    /// draining the run's supervised children may not exceed this from
    /// the REQUEST moment. Expiry reports the un-confirmed items — it
    /// never fabricates quiet and never blocks the single finalize.
    pub cleanup_grace_ms: u64,
    /// Hard cap of supervised-task registry entries (live or unreaped) —
    /// a bounded registry, never unbounded bookkeeping.
    pub supervised_task_cap: usize,
}

impl CancelPolicy {
    pub const DEFAULT_CLEANUP_GRACE_MS: u64 = 5_000;
    pub const DEFAULT_SUPERVISED_TASK_CAP: usize = 512;

    pub fn validate(&self) -> Result<(), lingxi_kernel::ports::StorageError> {
        use lingxi_kernel::ports::StorageError;
        if self.cleanup_grace_ms == 0 || self.cleanup_grace_ms > crate::config::MAX_TIME_BUDGET_MS {
            return Err(StorageError::InvalidRequest {
                detail: format!(
                    "cancel_policy.cleanup_grace_ms must be in 1..={} (the platform \
                     monotonic-clock budget bound), got {}",
                    crate::config::MAX_TIME_BUDGET_MS,
                    self.cleanup_grace_ms
                ),
            });
        }
        if self.supervised_task_cap == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "cancel_policy.supervised_task_cap must be >= 1 (0 would reject every \
                         supervised spawn)"
                    .to_string(),
            });
        }
        Ok(())
    }
}

impl Default for CancelPolicy {
    fn default() -> Self {
        Self {
            cleanup_grace_ms: Self::DEFAULT_CLEANUP_GRACE_MS,
            supervised_task_cap: Self::DEFAULT_SUPERVISED_TASK_CAP,
        }
    }
}

/// The remaining cleanup budget for one cancellation (anchored at the
/// request moment — a driver that only reaches cleanup late may not
/// re-extend the deadline).
#[derive(Debug, Clone, Copy)]
pub struct CancelBudget {
    requested_at: Instant,
    total: Duration,
}

impl CancelBudget {
    pub fn new(requested_at: Instant, total: Duration) -> Self {
        Self {
            requested_at,
            total,
        }
    }

    pub fn remaining(&self) -> Duration {
        self.total.saturating_sub(self.requested_at.elapsed())
    }

    pub fn total(&self) -> Duration {
        self.total
    }
}

/// Registry of the live runs' cancellation states. The run driver
/// registers at drive start and deregisters on every exit path; the
/// cancellation surface resolves run ids through it. The LAST phase of a
/// deregistered run stays queryable in a bounded recent record (a live
/// run's phase is live; a settled run's phase is the four-phase verdict
/// — `ConfirmedTerminated` / `StopUnconfirmed` / `Abandoned` — which is
/// exactly what a "取消状态可查询" surface needs after the drive ended).
pub struct CancelRegistry {
    runs: Mutex<HashMap<String, Arc<RunCancelEntry>>>,
    settled: Mutex<VecDeque<(String, CancelPhase)>>,
}

/// Bound of the recent-settled record (memory only; the oldest verdict
/// is dropped first — a bounded diagnostic log, not a state claim).
const RECENT_SETTLED_CAP: usize = 1024;

impl Default for CancelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelRegistry {
    pub fn new() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            settled: Mutex::new(VecDeque::new()),
        }
    }

    /// Registers one live run and returns its entry (root scope + phase).
    pub fn register(&self, run_id: &str) -> Arc<RunCancelEntry> {
        let entry = Arc::new(RunCancelEntry {
            run_id: run_id.to_string(),
            scope: CancelScope::run_root(run_id),
            phase: Mutex::new(CancelPhase::Active),
            registered_at: Instant::now(),
        });
        self.runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(run_id.to_string(), Arc::clone(&entry));
        entry
    }

    /// Deregisters the run (every driver exit path). A cancellation
    /// verdict (any phase past `Active`) is retained in the bounded
    /// recent record so the settled run's four-phase outcome stays
    /// queryable; normally-settled runs leave no record (nothing to
    /// report — their durable terminal IS the answer).
    pub fn deregister(&self, run_id: &str) {
        let removed = self
            .runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(run_id);
        if let Some(entry) = removed {
            let phase = entry.phase();
            if phase.cancel_requested() {
                let mut settled = self
                    .settled
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if settled.len() >= RECENT_SETTLED_CAP {
                    settled.pop_front();
                }
                settled.push_back((run_id.to_string(), phase));
            }
        }
    }

    /// The retained four-phase verdict of a run whose driver already
    /// exited (`None` when the run is still live — query [`Self::get`] —
    /// or settled without any cancellation).
    pub fn recent_verdict(&self, run_id: &str) -> Option<CancelPhase> {
        self.settled
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .rev()
            .find(|(id, _)| id == run_id)
            .map(|(_, phase)| phase.clone())
    }

    pub fn get(&self, run_id: &str) -> Option<Arc<RunCancelEntry>> {
        self.runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(run_id)
            .cloned()
    }

    /// Live run ids (observability / acceptance evidence).
    pub fn live_run_ids(&self) -> Vec<String> {
        self.runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Fires one cancellation request: flips the phase to
    /// [`CancelPhase::Requested`] and cancels the run's scope tree (every
    /// live descendant wakes). First request wins.
    pub fn fire(&self, run_id: &str, reason: &str) -> FireOutcome {
        let Some(entry) = self.get(run_id) else {
            return FireOutcome::NotLive;
        };
        let already_requested = entry.phase().cancel_requested();
        entry.scope.cancel(reason);
        if already_requested {
            return FireOutcome::AlreadyCancelling;
        }
        entry.advance_phase(CancelPhase::Requested {
            reason: reason.to_string(),
        });
        FireOutcome::Fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_a_parent_eagerly_cancels_the_whole_tree() {
        let root = CancelScope::run_root("run_x");
        let model = root.child("run_x-mc0001".to_string(), ScopeKind::ModelCall);
        let tool = root.child("run_x-tc0001".to_string(), ScopeKind::ToolCall);
        let grandchild = model.child("stream-read".to_string(), ScopeKind::ModelCall);
        assert!(!root.is_cancelled());
        assert!(!grandchild.is_cancelled());

        assert!(root.cancel("user"), "first cancel wins");
        assert!(!root.cancel("second reason"), "second cancel is a no-op");
        assert_eq!(root.reason().as_deref(), Some("user"));
        for scope in [&model, &tool, &grandchild] {
            assert!(scope.is_cancelled(), "descendant cancelled by the tree");
        }
        // Waiters resolve.
        tokio::time::timeout(Duration::from_millis(50), root.cancelled())
            .await
            .expect("root waiter resolves");
        // Descendant labels are enumerable for cleanup reporting.
        assert_eq!(root.live_descendant_labels().len(), 3);
    }

    #[tokio::test]
    async fn a_child_scope_never_cancels_its_parent() {
        let root = CancelScope::run_root("run_y");
        let child = root.child("run_y-mc0001".to_string(), ScopeKind::ModelCall);
        child.cancel("child-level abort");
        assert!(child.is_cancelled());
        assert!(
            !root.is_cancelled(),
            "child work ending is never the run ending (R03-T01 identity layer)"
        );
    }

    #[tokio::test]
    async fn registry_fire_tracks_the_first_reason_and_is_idempotent() {
        let registry = CancelRegistry::new();
        let entry = registry.register("run_z");
        assert_eq!(entry.phase(), CancelPhase::Active);
        assert_eq!(registry.fire("run_z", "user"), FireOutcome::Fired);
        assert_eq!(
            registry.fire("run_z", "another"),
            FireOutcome::AlreadyCancelling
        );
        match registry.get("run_z").unwrap().phase() {
            CancelPhase::Requested { reason } => assert_eq!(reason, "user"),
            other => panic!("expected Requested, got {other:?}"),
        }
        registry.deregister("run_z");
        assert_eq!(registry.fire("run_z", "late"), FireOutcome::NotLive);
    }

    #[tokio::test]
    async fn settled_verdicts_stay_queryable_and_normal_settles_leave_none() {
        let registry = CancelRegistry::new();
        // A cancelled run's verdict survives deregistration.
        let entry = registry.register("run_verdict");
        registry.fire("run_verdict", "user");
        entry.advance_phase(CancelPhase::ConfirmedTerminated {
            reason: "user".to_string(),
            confirmed: vec!["model_call:r-mc0001".to_string()],
        });
        registry.deregister("run_verdict");
        match registry.recent_verdict("run_verdict") {
            Some(CancelPhase::ConfirmedTerminated { confirmed, .. }) => {
                assert_eq!(confirmed.len(), 1);
            }
            other => panic!("expected retained ConfirmedTerminated, got {other:?}"),
        }
        // A normally-settled run (no cancellation) leaves NO verdict
        // record — its durable terminal is the answer, not a phase.
        let normal = registry.register("run_normal");
        drop(normal);
        registry.deregister("run_normal");
        assert_eq!(registry.recent_verdict("run_normal"), None);
    }

    #[tokio::test]
    async fn budget_counts_from_the_request_moment() {
        let budget = CancelBudget::new(
            Instant::now() - Duration::from_millis(80),
            Duration::from_millis(100),
        );
        assert!(budget.remaining() <= Duration::from_millis(25));
        let exhausted = CancelBudget::new(
            Instant::now() - Duration::from_millis(250),
            Duration::from_millis(100),
        );
        assert_eq!(exhausted.remaining(), Duration::ZERO);
    }

    #[test]
    fn degenerate_policy_refuses_validation() {
        assert!(CancelPolicy::default().validate().is_ok());
        assert!(CancelPolicy {
            cleanup_grace_ms: 0,
            ..CancelPolicy::default()
        }
        .validate()
        .is_err());
        assert!(CancelPolicy {
            supervised_task_cap: 0,
            ..CancelPolicy::default()
        }
        .validate()
        .is_err());
    }
}
