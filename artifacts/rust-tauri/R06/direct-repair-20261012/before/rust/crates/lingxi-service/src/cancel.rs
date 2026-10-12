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

    /// The root scope of one run that is a CHILD of `parent` (R03-T06):
    /// cancelling the parent propagates into this run's tree; this run's
    /// own cancellation still never ascends.
    ///
    /// R03 repair G01/F01: the link is now built as a PAIR — the child
    /// carries the parent reference AND is registered in the parent's
    /// children under the no-miss linking protocol of [`link_under`] (a
    /// parent that is already cancelled is INHERITED, never escaped).
    pub fn run_root_under(run_id: &str, parent: &CancelScope) -> Self {
        Self::new_child_scope(format!("run:{run_id}"), ScopeKind::Run, parent)
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
    /// parent's `cancel` reaches it through the tree — including a
    /// cancellation that already fired BEFORE this call, which is
    /// inherited at link time) while keeping its own label/kind for
    /// supervision and cleanup reporting.
    pub fn child(&self, label: String, kind: ScopeKind) -> CancelScope {
        Self::new_child_scope(label, kind, self)
    }

    /// The shared construction path of EVERY parented node (`child` and
    /// `run_root_under`): the child-side parent reference and the
    /// parent-side registration are established together in
    /// [`link_under`].
    fn new_child_scope(label: String, kind: ScopeKind, parent: &CancelScope) -> CancelScope {
        let scope = Self::new(label, kind, Some(Arc::clone(&parent.inner)));
        link_under(&parent.inner, &scope.inner);
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

    /// Whether THIS scope or any of its ANCESTORS was cancelled. The walk
    /// is monotonic (a cancellation never un-happens), so a `true` answer
    /// is stable; a scope whose ancestor fired is observably cancelled
    /// even in the instant before its own inherited flag lands (R03
    /// repair G01/F01: `is_cancelled` no longer reads only this node).
    pub fn is_cancelled(&self) -> bool {
        let mut current = Some(Arc::clone(&self.inner));
        while let Some(node) = current {
            if node.cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return true;
            }
            current = node.parent.clone();
        }
        false
    }

    /// The first cancellation reason recorded on THIS scope. A scope
    /// linked under an already-cancelled parent inherits that parent's
    /// first reason at link time ([`link_under`]); a scope reached by a
    /// later traversal records the reason its own first cancellation
    /// carried. Ancestors' reasons are queryable on the ancestors.
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

/// Links `child` under `parent` — the no-miss protocol every parented
/// node construction shares (R03 repair G01/F01).
///
/// The parent's `children` lock is the linearization point on BOTH sides:
/// - a cancellation traversal (`cancel_recursive`) holds this lock while
///   visiting the registered children;
/// - a registration holds this lock while pushing the new child AND while
///   re-checking the parent's cancelled flag.
///
/// Therefore the child is either (a) pushed before the traversal takes
/// the lock — the traversal then reaches it — or (b) linked after/below a
/// traversal already in flight — the flag re-check under the lock observes
/// the parent's cancellation and the child INHERITS it right here (first
/// reason and first MOMENT of the parent's own cancellation). No window
/// exists in which a successfully attributed node escapes cancellation.
///
/// Deadlock safety: the inheritance path only locks the CHILD's own
/// mutexes (a freshly constructed child has no other holders), matching
/// the traversal's parent.children → child.* lock order exactly; the
/// parent's `reason`/`cancelled_at` are read WITHOUT their locks being
/// held anywhere else — the first-writer leg of `cancel_recursive` fills
/// and RELEASES both before storing the cancelled flag, so a reader that
/// observes the flag `true` (Acquire) always sees complete facts.
fn link_under(parent: &Arc<ScopeInner>, child: &Arc<ScopeInner>) {
    let mut guard = parent
        .children
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if parent.cancelled.load(std::sync::atomic::Ordering::Acquire)
        && !child.cancelled.load(std::sync::atomic::Ordering::Acquire)
    {
        let reason = parent
            .reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .unwrap_or_else(|| "cancelled (reason unrecorded)".to_string());
        let inherited_at = *parent
            .cancelled_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Cancel the CHILD (its own mutexes only — nobody else holds them).
        let mut first = false;
        cancel_recursive(child, &reason, &mut first);
        // The inherited MOMENT is the parent's first cancellation moment
        // (the tree's first cancellation, not the linking instant).
        if let Some(at) = inherited_at {
            *child
                .cancelled_at
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(at);
        }
    }
    guard.push(Arc::downgrade(child));
    drop(guard);
}

fn cancel_recursive(inner: &Arc<ScopeInner>, reason: &str, first_anywhere: &mut bool) {
    // First-writer leg (R03 repair G01/F01 ordering): the reason and the
    // moment are filled and their locks RELEASED before the cancelled
    // flag is stored. Readers that observe the flag (Acquire) therefore
    // always see complete first-cancellation facts — the inheritance read
    // in `link_under` depends on exactly this ordering. A concurrent
    // writer that loses the reason race simply skips (the winner stores
    // the flag before reaching the children below).
    if !inner.cancelled.load(std::sync::atomic::Ordering::Acquire) {
        let mut reason_guard = inner
            .reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if reason_guard.is_none() {
            *reason_guard = Some(reason.to_string());
            drop(reason_guard);
            let mut at_guard = inner
                .cancelled_at
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if at_guard.is_none() {
                *at_guard = Some(Instant::now());
            }
            drop(at_guard);
            inner
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            *first_anywhere = true;
        }
    }
    inner.notify.notify_waiters();
    // Children are taken and RESTORED under the same lock hold so a
    // concurrent registration on this scope can never be lost; the
    // recursion locks only descendant mutexes (a tree has no cycles, and
    // no path ascends), so holding this lock is deadlock-free.
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
    /// R03 repair G02/F03 — the driver's IRREVOCABLE TERMINAL CLAIM: the
    /// single finalize transaction for a NON-cancellation terminal
    /// (completed/failed) has begun and no cancellation request had been
    /// accepted when it was claimed. This is the frozen linearization
    /// point of the cancel-vs-terminal race: a cancellation arriving now
    /// is honestly TOO LATE ([`FireOutcome::TooLate`]) — it is never
    /// reported Accepted (that would promise a stop that will not
    /// happen) and it does not fire the tree of a run that is already
    /// settling. `terminal` carries the claimed terminal-reason
    /// vocabulary (e.g. `completed.with_final`) for diagnosis.
    ///
    /// This is NOT a cancellation phase: `cancel_requested()` stays
    /// `false` for it and a normally-settled run leaves no verdict
    /// record, exactly like [`CancelPhase::Active`].
    Settling { terminal: String },
}

impl CancelPhase {
    /// True once a cancellation was requested at all.
    pub fn cancel_requested(&self) -> bool {
        matches!(
            self,
            CancelPhase::Requested { .. }
                | CancelPhase::Cleaning { .. }
                | CancelPhase::ConfirmedTerminated { .. }
                | CancelPhase::StopUnconfirmed { .. }
                | CancelPhase::Abandoned { .. }
        )
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
            CancelPhase::Settling { .. } => "settling",
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

    /// Fires one cancellation request against this run (R03-T03 phase 1).
    ///
    /// R03 repair G02/F03: the whole decision is ONE critical section on
    /// the phase mutex — the read of the current phase, the scope-tree
    /// cancellation and the `Active → Requested` write can no longer
    /// interleave with (a) another `fire` (which could otherwise observe
    /// `Active` twice, double-report `Fired` and overwrite the first
    /// reason) or (b) the driver's phase legs (which could otherwise be
    /// regressed from `Cleaning` back to `Requested`). The first reason
    /// is additionally protected by the scope tree's own first-writer
    /// rule ([`CancelScope::cancel`]); the retained phase reason is READ
    /// BACK from the scope so the two can never disagree.
    ///
    /// Lock order (deadlock freedom): the only nested acquisition this
    /// makes is `phase → scope.*`; no code path acquires a scope lock and
    /// then a phase lock (the driver's legs take them sequentially).
    pub fn fire(&self, reason: &str) -> FireOutcome {
        let mut phase = self
            .phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // The frozen irrevocable point: a terminal that was already
        // claimed wins. Honest answer, no tree fire, no acceptance.
        if matches!(&*phase, CancelPhase::Settling { .. }) {
            return FireOutcome::TooLate;
        }
        // First-writer rule lives in the scope tree: the reason recorded
        // here is whatever the tree's FIRST cancellation carried.
        self.scope.cancel(reason);
        match &*phase {
            CancelPhase::Active => {
                *phase = CancelPhase::Requested {
                    reason: self.scope.reason().unwrap_or_else(|| reason.to_string()),
                };
                FireOutcome::Fired
            }
            // A cancellation is already in flight (Requested/Cleaning/
            // Confirmed/Unconfirmed/Abandoned): idempotent no-op.
            _ => FireOutcome::AlreadyCancelling,
        }
    }

    /// R03 repair G02/F03 — the driver-side half of the unified
    /// cancel-vs-terminal adjudication. Atomically (against
    /// [`Self::fire`]) claims the right to settle a NON-cancellation
    /// terminal, or observes that an accepted cancellation already won:
    ///
    /// - [`TerminalAdjudication::Claimed`] — no cancellation had been
    ///   requested; the entry records [`CancelPhase::Settling`] (the
    ///   irrevocable point) and the caller proceeds into the single
    ///   finalize. Any `fire` from now on reports
    ///   [`FireOutcome::TooLate`].
    /// - [`TerminalAdjudication::CancelledBy`] — a cancellation request
    ///   was accepted first (its FIRST reason is returned); the caller
    ///   MUST divert its intended terminal through the four-phase
    ///   cancellation settle instead of committing completed/failed.
    ///
    /// The cancellation settle itself (`RunFinish::Cancelled`) does NOT
    /// pass through here — by construction the cancellation already won.
    pub fn claim_terminal(&self, terminal: &str) -> TerminalAdjudication {
        let mut phase = self
            .phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match (*phase).clone() {
            CancelPhase::Active => {
                // A PARENT-tree cancellation can have fired this scope
                // through the G01 traversal WITHOUT any local fire — the
                // scope flag is the monotonic fact, the phase lags. The
                // claim honors it: a run under a cancelled tree never
                // completes, and the phase is brought to the `requested`
                // leg it should have had. (The reverse order — a parent
                // traversal landing between this claim and the commit —
                // leaves the child's already-claimed terminal standing,
                // symmetric to the run's own frozen linearization rule.)
                if self.scope.is_cancelled() {
                    let reason = self
                        .scope
                        .reason()
                        .unwrap_or_else(|| "cancelled".to_string());
                    *phase = CancelPhase::Requested {
                        reason: reason.clone(),
                    };
                    return TerminalAdjudication::CancelledBy { reason };
                }
                *phase = CancelPhase::Settling {
                    terminal: terminal.to_string(),
                };
                TerminalAdjudication::Claimed
            }
            CancelPhase::Settling { .. } => {
                // A healthy driver claims exactly once per run. A second
                // claim is a driver invariant violation — keep the FIRST
                // claimed terminal (never rewrite it) and surface the
                // anomaly loudly.
                tracing::error!(
                    run_id = %self.run_id,
                    second_terminal = %terminal,
                    "run driver claimed the terminal right twice; the first claim stands"
                );
                TerminalAdjudication::Claimed
            }
            other => TerminalAdjudication::CancelledBy {
                reason: first_reason_of(&other).unwrap_or_else(|| "cancelled".to_string()),
            },
        }
    }
}

/// The FIRST cancellation reason carried by a cancel-requested phase.
fn first_reason_of(phase: &CancelPhase) -> Option<String> {
    match phase {
        CancelPhase::Requested { reason }
        | CancelPhase::Cleaning { reason }
        | CancelPhase::Abandoned { reason } => Some(reason.clone()),
        CancelPhase::ConfirmedTerminated { reason, .. }
        | CancelPhase::StopUnconfirmed { reason, .. } => Some(reason.clone()),
        CancelPhase::Active | CancelPhase::Settling { .. } => None,
    }
}

/// The driver-side verdict of [`RunCancelEntry::claim_terminal`] — the
/// unified cancel-vs-terminal adjudication (R03 repair G02/F03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalAdjudication {
    /// No cancellation was accepted: the caller irrevocably owns the
    /// terminal settlement (recorded as [`CancelPhase::Settling`]).
    Claimed,
    /// A cancellation request was accepted FIRST (with its first reason):
    /// the caller must settle through the cancellation path.
    CancelledBy { reason: String },
}

/// Outcome of a cancellation request against the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FireOutcome {
    /// The request fired the run's tree (first cancellation wins).
    Fired,
    /// The run is live but a cancellation was already in flight.
    AlreadyCancelling,
    /// The run's driver already irrevocably claimed its terminal
    /// settlement (R03 repair G02/F03): the single finalize transaction
    /// for a completed/failed terminal is in flight. Nothing was
    /// cancelled and nothing stopped on this request — reporting
    /// `Fired`/acceptance would promise a stop that will not happen.
    TooLate,
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
        self.register_scope(run_id, CancelScope::run_root(run_id))
    }

    /// Registers one live run whose ROOT is a CHILD of `parent_scope`
    /// (R03-T06 subagent child runs): cancelling the parent propagates
    /// down into this run's own tree (the incumbent's
    /// `abortByParentSession` semantics — STATE_TRANSITIONS T8), while
    /// this run's own cancellation never ascends. Everything else
    /// (phases, verdict retention) is identical to [`Self::register`].
    ///
    /// R03 repair G01/F01: when the parent's tree is ALREADY cancelled at
    /// registration time, the linked root INHERITS the cancellation at
    /// construction and the entry starts at the `requested` phase with
    /// the inherited first reason (never Active-under-a-cancelled-scope).
    pub fn register_linked(&self, run_id: &str, parent_scope: &CancelScope) -> Arc<RunCancelEntry> {
        self.register_scope(run_id, CancelScope::run_root_under(run_id, parent_scope))
    }

    fn register_scope(&self, run_id: &str, scope: CancelScope) -> Arc<RunCancelEntry> {
        let entry = Arc::new(RunCancelEntry {
            run_id: run_id.to_string(),
            scope,
            phase: Mutex::new(CancelPhase::Active),
            registered_at: Instant::now(),
        });
        // A scope that arrived already cancelled (inherited at link time)
        // starts the four-phase machine at Requested with the inherited
        // first reason — a driver registering under a cancelled parent
        // observes a live cancellation, not a fresh Active run.
        if entry.scope.is_cancelled() {
            entry.advance_phase(CancelPhase::Requested {
                reason: entry
                    .scope
                    .reason()
                    .unwrap_or_else(|| "inherited cancellation".to_string()),
            });
        }
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
    /// live descendant wakes). First request wins. The decision is one
    /// critical section on the entry (see [`RunCancelEntry::fire`]).
    pub fn fire(&self, run_id: &str, reason: &str) -> FireOutcome {
        let Some(entry) = self.get(run_id) else {
            return FireOutcome::NotLive;
        };
        entry.fire(reason)
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

    /// R03 repair G01/F01: `run_root_under` links BOTH ways — the linked
    /// root receives the parent's cancellation through the traversal.
    #[tokio::test]
    async fn linked_run_root_receives_the_parent_cancellation() {
        let parent = CancelScope::run_root("run_linked_p");
        let child = CancelScope::run_root_under("run_linked_c", &parent);
        let grand = CancelScope::run_root_under("run_linked_g", &child);
        let call = grand.child("mc".to_string(), ScopeKind::ModelCall);
        assert!(!child.is_cancelled());
        parent.cancel("user");
        assert!(child.is_cancelled());
        assert!(grand.is_cancelled());
        assert!(call.is_cancelled());
        assert_eq!(child.reason().as_deref(), Some("user"));
        // The linked root is enumerable from the parent (supervision).
        assert!(parent
            .live_descendant_labels()
            .iter()
            .any(|label| label.contains("run_linked_c")));
    }

    /// R03 repair G01/F01: `register_linked` under an ALREADY-cancelled
    /// parent inherits the cancellation (first reason + moment) and the
    /// registry entry starts at the `requested` phase.
    #[tokio::test]
    async fn register_linked_under_cancelled_parent_inherits() {
        let registry = CancelRegistry::new();
        let parent = registry.register("run_inh_p");
        assert!(parent.scope.cancel("first-reason"));
        let first_at = parent.scope.cancelled_at().unwrap();
        let child = registry.register_linked("run_inh_c", &parent.scope);
        assert!(child.scope.is_cancelled());
        assert_eq!(child.scope.reason().as_deref(), Some("first-reason"));
        assert_eq!(child.scope.cancelled_at(), Some(first_at));
        match child.phase() {
            CancelPhase::Requested { reason } => assert_eq!(reason, "first-reason"),
            other => panic!("expected Requested, got {other:?}"),
        }
        // A late different-reason cancel of the parent never rewrites the
        // inherited first reason.
        parent.scope.cancel("second-reason");
        assert_eq!(child.scope.reason().as_deref(), Some("first-reason"));
    }

    /// R03 repair G01/F01: nodes created AFTER the traversal still
    /// inherit (the registration-vs-cancellation window misses nothing).
    #[test]
    fn late_children_of_a_cancelled_scope_inherit_without_new_reason() {
        let parent = CancelScope::run_root("run_late_p");
        parent.cancel("only-reason");
        for kind in [ScopeKind::ToolCall, ScopeKind::ChildRun] {
            let late = parent.child(format!("{kind:?}").to_lowercase(), kind);
            assert!(late.is_cancelled());
            assert_eq!(late.reason().as_deref(), Some("only-reason"));
        }
    }

    /// R03 repair G01/F01: `is_cancelled` consults ANCESTORS — a node
    /// whose ancestor fired reads as cancelled even in the instant before
    /// its own inherited flag would be observed by a single-node read.
    #[test]
    fn is_cancelled_walks_the_ancestor_chain() {
        let root = CancelScope::run_root("run_walk");
        let mid = CancelScope::run_root_under("run_walk_mid", &root);
        assert!(!mid.is_cancelled());
        assert!(root.cancel("ancestor"));
        assert!(mid.is_cancelled(), "the ancestor's cancellation is visible");
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

    // ── R03 repair G02/F03: the unified cancel-vs-terminal adjudication ──

    /// A terminal claim taken BEFORE any cancellation makes every later
    /// fire honestly TooLate — the tree is never fired for a run that is
    /// already irrevocably settling.
    #[tokio::test]
    async fn fire_after_the_terminal_claim_is_too_late_and_never_fires_the_tree() {
        let registry = CancelRegistry::new();
        let entry = registry.register("run_f03_claim_first");
        assert_eq!(
            entry.claim_terminal("completed.with_final"),
            TerminalAdjudication::Claimed
        );
        assert_eq!(
            entry.phase(),
            CancelPhase::Settling {
                terminal: "completed.with_final".to_string()
            }
        );
        assert!(
            !entry.scope.is_cancelled(),
            "no tree fire for a settling run"
        );
        assert_eq!(
            registry.fire("run_f03_claim_first", "user"),
            FireOutcome::TooLate
        );
        assert!(
            !entry.scope.is_cancelled(),
            "a TooLate fire must not cancel the tree either"
        );
        // Settling is NOT a cancellation verdict: the phase machine reports
        // "no cancellation requested" and deregistration keeps the
        // normally-settled run out of the verdict record.
        assert!(!entry.phase().cancel_requested());
        registry.deregister("run_f03_claim_first");
        assert_eq!(registry.recent_verdict("run_f03_claim_first"), None);
    }

    /// A cancellation accepted BEFORE the terminal claim wins the
    /// adjudication: the claim returns the cancellation's FIRST reason
    /// and a later duplicate fire keeps it.
    #[tokio::test]
    async fn claim_after_an_accepted_cancel_diverts_with_the_first_reason() {
        let registry = CancelRegistry::new();
        let entry = registry.register("run_f03_cancel_first");
        assert_eq!(
            registry.fire("run_f03_cancel_first", "user"),
            FireOutcome::Fired
        );
        assert_eq!(
            registry.fire("run_f03_cancel_first", "second caller"),
            FireOutcome::AlreadyCancelling
        );
        assert_eq!(
            entry.claim_terminal("completed.with_final"),
            TerminalAdjudication::CancelledBy {
                reason: "user".to_string()
            }
        );
        // The diversion never regressed the phase machine.
        match entry.phase() {
            CancelPhase::Requested { reason } => assert_eq!(reason, "user"),
            other => panic!("expected Requested, got {other:?}"),
        }
    }

    /// A PARENT-tree cancellation that traversed into a linked run root
    /// (scope flag set, local phase still Active) also wins the claim:
    /// the child never completes under a cancelled tree, and the phase is
    /// brought to the `requested` leg with the tree's first reason.
    #[tokio::test]
    async fn claim_honors_a_parent_tree_cancellation_the_phase_has_not_seen() {
        let registry = CancelRegistry::new();
        let parent = registry.register("run_f03_parent");
        // The child registers while the parent is still live, then the
        // parent's tree fires — the traversal sets the child's scope flag
        // without any local fire.
        let child = registry.register_linked("run_f03_child", &parent.scope);
        assert!(parent.scope.cancel("parent user"));
        assert!(child.scope.is_cancelled());
        assert_eq!(child.phase(), CancelPhase::Active);
        assert_eq!(
            child.claim_terminal("completed.with_final"),
            TerminalAdjudication::CancelledBy {
                reason: "parent user".to_string()
            }
        );
        match child.phase() {
            CancelPhase::Requested { reason } => assert_eq!(reason, "parent user"),
            other => panic!("expected Requested, got {other:?}"),
        }
    }

    /// The phase machine never regresses: a fire landing while the driver
    /// is already cleaning reports AlreadyCancelling and writes nothing.
    #[tokio::test]
    async fn fire_landing_after_the_cleaning_leg_writes_nothing_back() {
        let registry = CancelRegistry::new();
        let entry = registry.register("run_f03_mono");
        assert_eq!(registry.fire("run_f03_mono", "user"), FireOutcome::Fired);
        entry.advance_phase(CancelPhase::Cleaning {
            reason: "user".to_string(),
        });
        assert_eq!(
            registry.fire("run_f03_mono", "late duplicate"),
            FireOutcome::AlreadyCancelling
        );
        match entry.phase() {
            CancelPhase::Cleaning { reason } => assert_eq!(reason, "user"),
            other => panic!("phase regressed: {other:?}"),
        }
    }

    /// The adjudication serializes consistently under REAL concurrency:
    /// per round exactly one side wins — either the terminal is claimed
    /// and the concurrent fire reads TooLate (tree untouched), or the
    /// cancellation is accepted and the concurrent claim reads
    /// CancelledBy with the first reason. No interleaving may produce a
    /// pair where both sides believe they won.
    #[test]
    fn claim_and_fire_serialize_consistently_under_real_concurrency() {
        const ROUNDS: usize = 1_000;
        for round in 0..ROUNDS {
            let registry = CancelRegistry::new();
            let entry = registry.register(&format!("run_f03_race_{round}"));
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let claimer = {
                let entry = Arc::clone(&entry);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    entry.claim_terminal("completed.with_final")
                })
            };
            let firer = {
                let entry = Arc::clone(&entry);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    entry.fire("user")
                })
            };
            let claim = claimer.join().expect("claimer");
            let fire = firer.join().expect("firer");
            match (claim, fire) {
                (TerminalAdjudication::Claimed, FireOutcome::TooLate) => {
                    assert!(
                        !entry.scope.is_cancelled(),
                        "round {round}: the losing fire must not touch the tree"
                    );
                }
                (
                    TerminalAdjudication::CancelledBy { reason },
                    FireOutcome::Fired | FireOutcome::AlreadyCancelling,
                ) => {
                    assert_eq!(reason, "user", "round {round}: first reason preserved");
                }
                inconsistent => {
                    panic!("round {round}: inconsistent adjudication {inconsistent:?}")
                }
            }
        }
    }
}
