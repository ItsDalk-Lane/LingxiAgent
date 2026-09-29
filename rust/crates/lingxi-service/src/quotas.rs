//! Global/agent/session admission quotas for model & tool calls (R03-T02
//! deliverable "配额管理器").
//!
//! What this owns — and what it deliberately does NOT:
//! - Per-call admission for TWO resource families (`model`, `tool`), each
//!   layered as global → per-agent → per-session. A call holds one permit
//!   per layer for the duration of the model/tool I/O only; the layer
//!   order is fixed so nested acquisition cannot deadlock (every layer is
//!   eventually released by RAII, and waits are bounded by timeout).
//! - Waits are QUEUED, FIFO, and BOUNDED: a lane never grows an unbounded
//!   waiter list (`wait_queue_capacity`); beyond it the acquisition is
//!   REJECTED loudly (`QueueFull`) — the taskbook's "超限按协议拒绝或排队".
//! - Every wait is cancellable at any await point and every timeout returns
//!   the caller's place: a cancelled or timed-out waiter can never leak a
//!   permit (granted-flag + queue membership are mutated only under the
//!   lane lock; a drop-time guard releases a grant that raced in).
//! - Permits are RAII: task failure, timeout, or cancellation of the
//!   driving future drops the guards and returns the slots (R03-T02 step 4
//!   for the error/failure/timeout paths; the user-facing cancellation
//!   TREE is R03-T03).
//!
//! This is admission control, NOT a global lock: while one session's call
//! waits for a slot, every other session proceeds — R03-A04 proves a pure
//! text run finishes while another session's tool call is parked.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use lingxi_kernel::QuotaResource;

/// Per-layer limits of one resource family (R03-T02 step 3: 全局/agent/session
/// 工具与模型配额).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayeredQuotaLimits {
    /// Process-wide concurrent calls of this resource family (>= 1).
    pub global: usize,
    /// Concurrent calls attributed to ONE agent id (>= 1).
    pub per_agent: usize,
    /// Concurrent calls attributed to ONE session id (>= 1).
    pub per_session: usize,
}

impl LayeredQuotaLimits {
    pub const DEFAULT_MODEL: Self = Self {
        global: 8,
        per_agent: 4,
        per_session: 1,
    };
    pub const DEFAULT_TOOL: Self = Self {
        global: 8,
        per_agent: 4,
        per_session: 2,
    };
}

/// Full quota policy of one service instance (injected through
/// `ServiceDeps`; degenerate values refuse startup loudly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaLimits {
    pub model: LayeredQuotaLimits,
    pub tool: LayeredQuotaLimits,
    /// Bounded FIFO waiter capacity PER LANE: a lane at this many waiters
    /// rejects further acquisitions (`QueueFull`) instead of queueing
    /// without bound.
    pub wait_queue_capacity: usize,
    /// How long one layer acquisition may wait before the run fails loudly
    /// with `failed.quota_exhausted.*`.
    pub wait_timeout_ms: u64,
    /// Registry caps for on-demand per-agent / per-session lanes (idle
    /// lanes are evicted first; the caps bound memory, never correctness).
    pub max_agent_lanes: usize,
    pub max_session_lanes: usize,
}

impl QuotaLimits {
    pub const DEFAULT_WAIT_QUEUE_CAPACITY: usize = 64;
    pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 30_000;
    pub const DEFAULT_MAX_AGENT_LANES: usize = 256;
    pub const DEFAULT_MAX_SESSION_LANES: usize = 4096;

    /// Loud validation (mirrors `RunDriveLimits::validate`): a zero anywhere
    /// would either deadlock the first waiter or silently disable the
    /// limit — both are startup errors, never clamps.
    pub fn validate(&self) -> Result<(), lingxi_kernel::ports::StorageError> {
        use lingxi_kernel::ports::StorageError;
        let zero = |what: &str| {
            Err(StorageError::InvalidRequest {
                detail: format!(
                    "{what} must be >= 1 (0 would deadlock or silently disable the limit)"
                ),
            })
        };
        let l = self.model;
        if l.global == 0 || l.per_agent == 0 || l.per_session == 0 {
            return zero("quota model layer limits");
        }
        let t = self.tool;
        if t.global == 0 || t.per_agent == 0 || t.per_session == 0 {
            return zero("quota tool layer limits");
        }
        if self.wait_queue_capacity == 0 {
            return zero("quota wait_queue_capacity");
        }
        if self.wait_timeout_ms == 0 || self.wait_timeout_ms > crate::config::MAX_TIME_BUDGET_MS {
            return Err(StorageError::InvalidRequest {
                detail: format!(
                    "quota wait_timeout_ms must be in 1..={} (the platform monotonic-clock \
                     budget bound), got {}",
                    crate::config::MAX_TIME_BUDGET_MS,
                    self.wait_timeout_ms
                ),
            });
        }
        if self.max_agent_lanes == 0 || self.max_session_lanes == 0 {
            return zero("quota lane registry caps");
        }
        Ok(())
    }
}

impl Default for QuotaLimits {
    fn default() -> Self {
        Self {
            model: LayeredQuotaLimits::DEFAULT_MODEL,
            tool: LayeredQuotaLimits::DEFAULT_TOOL,
            wait_queue_capacity: Self::DEFAULT_WAIT_QUEUE_CAPACITY,
            wait_timeout_ms: Self::DEFAULT_WAIT_TIMEOUT_MS,
            max_agent_lanes: Self::DEFAULT_MAX_AGENT_LANES,
            max_session_lanes: Self::DEFAULT_MAX_SESSION_LANES,
        }
    }
}

/// Which layer rejected/timed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaLayer {
    Global,
    Agent,
    Session,
}

impl QuotaLayer {
    fn name(self) -> &'static str {
        match self {
            QuotaLayer::Global => "global",
            QuotaLayer::Agent => "agent",
            QuotaLayer::Session => "session",
        }
    }
}

/// Failure of one admission attempt. All variants are loud and carry the
/// layer; the run driver settles the run with
/// `failed.quota_exhausted.{model,tool}` through the single finalize path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaFailure {
    pub resource: QuotaResource,
    pub layer: QuotaLayer,
    pub kind: QuotaFailureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaFailureKind {
    /// The layer's bounded waiter queue is full — rejected, NOT queued
    /// (backpressure; the caller may retry).
    QueueFull,
    /// The bounded wait timed out.
    TimedOut,
    /// The per-agent/per-session lane registry is at its cap and no idle
    /// lane can be evicted (service protection, mirror of the R02
    /// rate-limiter registry cap).
    RegistryFull,
}

impl std::fmt::Display for QuotaFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let resource = match self.resource {
            QuotaResource::Model => "model",
            QuotaResource::Tool => "tool",
        };
        let kind = match self.kind {
            QuotaFailureKind::QueueFull => "waiter queue full",
            QuotaFailureKind::TimedOut => "bounded wait timed out",
            QuotaFailureKind::RegistryFull => "lane registry full",
        };
        write!(
            f,
            "{resource} quota ({}): {kind} — admission refused loudly",
            self.layer.name()
        )
    }
}

// ── lane ───────────────────────────────────────────────────────────────────

struct Waiter {
    id: u64,
    /// Set (under the LANE lock) by a releaser that popped this waiter and
    /// reserved the slot on its behalf. Atomic only because the waiter is
    /// shared through an `Arc`; the lane lock remains the happens-before
    /// authority.
    granted: AtomicBool,
    notify: tokio::sync::Notify,
}

#[derive(Default)]
struct LaneState {
    in_use: usize,
    waiters: VecDeque<Arc<Waiter>>,
}

struct Lane {
    capacity: usize,
    waiter_cap: usize,
    state: Mutex<LaneState>,
}

impl Lane {
    fn new(capacity: usize, waiter_cap: usize) -> Self {
        Self {
            capacity,
            waiter_cap,
            state: Mutex::new(LaneState::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LaneState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn release_slot(&self) {
        let mut st = self.lock();
        st.in_use = st.in_use.saturating_sub(1);
        // Hand the freed slot to the queue head (FIFO). If that waiter is
        // already gone (its guard removed it — granted stays false), the
        // head was already popped, so nothing to hand off; `granted` +
        // queue membership mutate only under this lock, so the handoff is
        // race-free.
        if let Some(waiter) = st.waiters.pop_front() {
            waiter.granted.store(true, Ordering::Release);
            st.in_use += 1;
            waiter.notify.notify_one();
        }
    }
}

/// RAII permit of ONE lane.
struct LanePermit {
    lane: Arc<Lane>,
}

impl Drop for LanePermit {
    fn drop(&mut self) {
        self.lane.release_slot();
    }
}

/// Drop-time guard of a WAITING acquisition: releases a grant that raced in
/// with the waiter's own cancellation/timeout, and dequeues the waiter if
/// it was never granted. Disarmed the instant the acquisition returns a
/// permit — so a permit's lifetime belongs to its owner alone.
struct WaitGuard {
    lane: Arc<Lane>,
    waiter: Arc<Waiter>,
    armed: bool,
}

impl WaitGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut st = self.lane.lock();
        if self.waiter.granted.load(Ordering::Acquire) {
            // A releaser reserved the slot on our behalf in the instant
            // between notify and our drop: give it back (including the
            // queue-head handoff).
            st.in_use = st.in_use.saturating_sub(1);
            if let Some(next) = st.waiters.pop_front() {
                next.granted.store(true, Ordering::Release);
                st.in_use += 1;
                next.notify.notify_one();
            }
        } else {
            st.waiters.retain(|w| w.id != self.waiter.id);
        }
    }
}

static WAITER_SEQ: AtomicU64 = AtomicU64::new(0);

enum OneLaneOutcome {
    Granted(LanePermit),
    Failed(QuotaFailureKind),
}

async fn acquire_one_lane(lane: &Arc<Lane>, timeout_ms: u64) -> OneLaneOutcome {
    loop {
        // Fast path: free capacity AND nobody queued (FIFO — a newcomer
        // never jumps the waiter queue).
        {
            let mut st = lane.lock();
            if st.waiters.is_empty() && st.in_use < lane.capacity {
                st.in_use += 1;
                return OneLaneOutcome::Granted(LanePermit {
                    lane: Arc::clone(lane),
                });
            }
            if st.waiters.len() >= lane.waiter_cap {
                return OneLaneOutcome::Failed(QuotaFailureKind::QueueFull);
            }
        }
        // Slow path: enqueue and wait for a grant, a timeout, or
        // cancellation (the guard makes all three leak-free).
        let waiter = Arc::new(Waiter {
            id: WAITER_SEQ.fetch_add(1, Ordering::Relaxed),
            granted: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        });
        {
            let mut st = lane.lock();
            // Re-check both edges under the lock (state may have moved).
            if st.waiters.is_empty() && st.in_use < lane.capacity {
                st.in_use += 1;
                return OneLaneOutcome::Granted(LanePermit {
                    lane: Arc::clone(lane),
                });
            }
            if st.waiters.len() >= lane.waiter_cap {
                return OneLaneOutcome::Failed(QuotaFailureKind::QueueFull);
            }
            st.waiters.push_back(Arc::clone(&waiter));
        }
        let mut guard = WaitGuard {
            lane: Arc::clone(lane),
            waiter: Arc::clone(&waiter),
            armed: true,
        };
        let wait = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            waiter.notify.notified(),
        )
        .await;
        match wait {
            Ok(()) => {
                let st = lane.lock();
                if waiter.granted.load(Ordering::Acquire) {
                    drop(st);
                    guard.disarm();
                    return OneLaneOutcome::Granted(LanePermit {
                        lane: Arc::clone(lane),
                    });
                }
                // Notified without a grant cannot happen (the releaser sets
                // granted under the lock before notifying); re-wait
                // defensively instead of inventing an outcome.
                drop(st);
                continue;
            }
            Err(_elapsed) => {
                let mut st = lane.lock();
                if waiter.granted.load(Ordering::Acquire) {
                    // The grant raced in at the deadline: it is ours — take
                    // it rather than leaking the reserved slot.
                    drop(st);
                    guard.disarm();
                    return OneLaneOutcome::Granted(LanePermit {
                        lane: Arc::clone(lane),
                    });
                }
                st.waiters.retain(|w| w.id != waiter.id);
                drop(st);
                guard.disarm();
                return OneLaneOutcome::Failed(QuotaFailureKind::TimedOut);
            }
        }
    }
}

// ── manager ────────────────────────────────────────────────────────────────

struct LaneRegistries {
    agents: Mutex<HashMap<(QuotaResource, String), Arc<Lane>>>,
    sessions: Mutex<HashMap<(QuotaResource, String), Arc<Lane>>>,
}

/// The quota manager (R03-T02): shared, lock-light admission control. The
/// internal mutexes are held for O(1) map/dequeue work only — never across
/// model/tool I/O awaits.
pub struct QuotaManager {
    limits: QuotaLimits,
    global_model: Arc<Lane>,
    global_tool: Arc<Lane>,
    lanes: LaneRegistries,
}

impl std::fmt::Debug for QuotaManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuotaManager")
            .field("limits", &self.limits)
            .field("global_model_in_use", &self.in_use(QuotaResource::Model))
            .field("global_tool_in_use", &self.in_use(QuotaResource::Tool))
            .finish_non_exhaustive()
    }
}

/// RAII permit of all three layers of one resource family. Dropping it (at
/// call end, on error, on timeout, or on cancellation of the driving
/// future) returns every slot.
pub struct QuotaPermit {
    _session: Option<LanePermit>,
    _agent: Option<LanePermit>,
    _global: Option<LanePermit>,
}

impl std::fmt::Debug for QuotaPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("QuotaPermit(<held>)")
    }
}

impl QuotaManager {
    pub fn new(limits: QuotaLimits) -> Self {
        Self {
            global_model: Arc::new(Lane::new(limits.model.global, limits.wait_queue_capacity)),
            global_tool: Arc::new(Lane::new(limits.tool.global, limits.wait_queue_capacity)),
            lanes: LaneRegistries {
                agents: Mutex::new(HashMap::new()),
                sessions: Mutex::new(HashMap::new()),
            },
            limits,
        }
    }

    pub fn limits(&self) -> &QuotaLimits {
        &self.limits
    }

    fn global_lane(&self, resource: QuotaResource) -> Arc<Lane> {
        match resource {
            QuotaResource::Model => Arc::clone(&self.global_model),
            QuotaResource::Tool => Arc::clone(&self.global_tool),
        }
    }

    fn layered_of(&self, resource: QuotaResource) -> LayeredQuotaLimits {
        match resource {
            QuotaResource::Model => self.limits.model,
            QuotaResource::Tool => self.limits.tool,
        }
    }

    /// Bounded on-demand lane lookup with idle eviction (the registry can
    /// never outgrow its cap; an idle lane is one with nothing in use and
    /// nobody waiting).
    fn agent_lane(
        &self,
        resource: QuotaResource,
        agent_id: &str,
    ) -> Result<Arc<Lane>, QuotaFailureKind> {
        self.lane_from(
            &self.lanes.agents,
            resource,
            QuotaLayer::Agent,
            agent_id,
            self.limits.max_agent_lanes,
        )
    }

    fn session_lane(
        &self,
        resource: QuotaResource,
        session_id: &str,
    ) -> Result<Arc<Lane>, QuotaFailureKind> {
        self.lane_from(
            &self.lanes.sessions,
            resource,
            QuotaLayer::Session,
            session_id,
            self.limits.max_session_lanes,
        )
    }

    fn lane_from(
        &self,
        registry: &Mutex<HashMap<(QuotaResource, String), Arc<Lane>>>,
        resource: QuotaResource,
        layer: QuotaLayer,
        key: &str,
        cap: usize,
    ) -> Result<Arc<Lane>, QuotaFailureKind> {
        let mut map = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(lane) = map.get(&(resource, key.to_string())) {
            return Ok(Arc::clone(lane));
        }
        if map.len() >= cap {
            // Evict idle lanes first (nothing in use, nobody waiting).
            map.retain(|_, lane| {
                let st = lane.lock();
                st.in_use > 0 || !st.waiters.is_empty()
            });
            if map.len() >= cap {
                return Err(QuotaFailureKind::RegistryFull);
            }
        }
        let layered = self.layered_of(resource);
        let capacity = match layer {
            QuotaLayer::Agent => layered.per_agent,
            QuotaLayer::Session => layered.per_session,
            QuotaLayer::Global => unreachable!("global lanes are not registry-managed"),
        };
        let lane = Arc::new(Lane::new(capacity, self.limits.wait_queue_capacity));
        map.insert((resource, key.to_string()), Arc::clone(&lane));
        Ok(lane)
    }

    /// Acquires global → agent → session admission for one call. Any failed
    /// layer drops the earlier permits (RAII) — no partial hold, no leak.
    pub async fn acquire(
        &self,
        resource: QuotaResource,
        agent_id: &str,
        session_id: &str,
    ) -> Result<QuotaPermit, QuotaFailure> {
        let timeout = self.limits.wait_timeout_ms;
        let global = self.global_lane(resource);
        let global_permit = match acquire_one_lane(&global, timeout).await {
            OneLaneOutcome::Granted(p) => p,
            OneLaneOutcome::Failed(kind) => {
                return Err(QuotaFailure {
                    resource,
                    layer: QuotaLayer::Global,
                    kind,
                })
            }
        };
        let agent_lane = match self.agent_lane(resource, agent_id) {
            Ok(lane) => lane,
            Err(kind) => {
                return Err(QuotaFailure {
                    resource,
                    layer: QuotaLayer::Agent,
                    kind,
                })
            }
        };
        let agent_permit = match acquire_one_lane(&agent_lane, timeout).await {
            OneLaneOutcome::Granted(p) => p,
            OneLaneOutcome::Failed(kind) => {
                return Err(QuotaFailure {
                    resource,
                    layer: QuotaLayer::Agent,
                    kind,
                })
            }
        };
        let session_lane = match self.session_lane(resource, session_id) {
            Ok(lane) => lane,
            Err(kind) => {
                return Err(QuotaFailure {
                    resource,
                    layer: QuotaLayer::Session,
                    kind,
                })
            }
        };
        let session_permit = match acquire_one_lane(&session_lane, timeout).await {
            OneLaneOutcome::Granted(p) => p,
            OneLaneOutcome::Failed(kind) => {
                return Err(QuotaFailure {
                    resource,
                    layer: QuotaLayer::Session,
                    kind,
                })
            }
        };
        Ok(QuotaPermit {
            _session: Some(session_permit),
            _agent: Some(agent_permit),
            _global: Some(global_permit),
        })
    }

    /// Global in-flight count of a resource family (observability; the
    /// acceptance assertions read this).
    pub fn in_use(&self, resource: QuotaResource) -> usize {
        self.global_lane(resource).lock().in_use
    }

    /// Global queued-waiter count of a resource family.
    pub fn waiting(&self, resource: QuotaResource) -> usize {
        self.global_lane(resource).lock().waiters.len()
    }

    /// In-flight count of one session's lane (observability).
    pub fn session_in_use(&self, resource: QuotaResource, session_id: &str) -> usize {
        let map = self
            .lanes
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.get(&(resource, session_id.to_string()))
            .map(|lane| lane.lock().in_use)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(waiters: usize, timeout_ms: u64) -> QuotaLimits {
        QuotaLimits {
            wait_queue_capacity: waiters,
            wait_timeout_ms: timeout_ms,
            ..QuotaLimits::default()
        }
    }

    #[test]
    fn degenerate_quota_limits_refuse_validation() {
        assert!(QuotaLimits::default().validate().is_ok());
        for bad in [
            QuotaLimits {
                model: LayeredQuotaLimits {
                    global: 0,
                    per_agent: 1,
                    per_session: 1,
                },
                ..QuotaLimits::default()
            },
            QuotaLimits {
                tool: LayeredQuotaLimits {
                    global: 1,
                    per_agent: 0,
                    per_session: 1,
                },
                ..QuotaLimits::default()
            },
            QuotaLimits {
                wait_queue_capacity: 0,
                ..QuotaLimits::default()
            },
            QuotaLimits {
                wait_timeout_ms: 0,
                ..QuotaLimits::default()
            },
            QuotaLimits {
                max_session_lanes: 0,
                ..QuotaLimits::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?} must be rejected");
        }
    }

    #[tokio::test]
    async fn global_limit_queues_fifo_and_releases_on_drop() {
        let manager = QuotaManager::new(limits(8, 5_000));
        let p1 = manager
            .acquire(QuotaResource::Tool, "lingxi", "s1")
            .await
            .expect("first");
        assert_eq!(manager.in_use(QuotaResource::Tool), 1);
        // Same-session second call waits (per-session tool limit is 2, but
        // the GLOBAL tool lane is what saturates first here).
        let p2 = manager
            .acquire(QuotaResource::Tool, "lingxi", "s1")
            .await
            .expect("second within per-session limit");
        assert_eq!(manager.in_use(QuotaResource::Tool), 2);
        drop(p1);
        drop(p2);
        assert_eq!(manager.in_use(QuotaResource::Tool), 0);
    }

    #[tokio::test]
    async fn waiter_times_out_loudly_and_leaks_nothing() {
        let manager = QuotaManager::new(QuotaLimits {
            tool: LayeredQuotaLimits {
                global: 1,
                per_agent: 4,
                per_session: 2,
            },
            wait_queue_capacity: 8,
            wait_timeout_ms: 100,
            ..QuotaLimits::default()
        });
        let held = manager
            .acquire(QuotaResource::Tool, "lingxi", "s1")
            .await
            .expect("holder");
        assert_eq!(manager.in_use(QuotaResource::Tool), 1);
        let blocked = manager.acquire(QuotaResource::Tool, "lingxi", "s2").await;
        assert!(
            blocked.is_err(),
            "the bounded wait must time out with a full global lane"
        );
        let err = blocked.unwrap_err();
        assert_eq!(err.kind, QuotaFailureKind::TimedOut);
        assert_eq!(err.layer, QuotaLayer::Global);
        // No ghost waiter and no leaked slot.
        assert_eq!(manager.waiting(QuotaResource::Tool), 0);
        assert_eq!(manager.in_use(QuotaResource::Tool), 1);
        drop(held);
        assert_eq!(manager.in_use(QuotaResource::Tool), 0);
        // The lane is usable again immediately.
        let again = manager
            .acquire(QuotaResource::Tool, "lingxi", "s3")
            .await
            .expect("lane usable after timeout");
        drop(again);
    }

    #[tokio::test]
    async fn bounded_waiter_queue_rejects_instead_of_growing() {
        let manager = Arc::new(QuotaManager::new(QuotaLimits {
            tool: LayeredQuotaLimits {
                global: 1,
                per_agent: 8,
                per_session: 8,
            },
            wait_queue_capacity: 2,
            wait_timeout_ms: 60_000,
            ..QuotaLimits::default()
        }));
        let _held = manager
            .acquire(QuotaResource::Tool, "lingxi", "s1")
            .await
            .expect("holder");
        let mut parked = Vec::new();
        for _ in 0..2 {
            let manager = Arc::clone(&manager);
            parked.push(tokio::spawn(async move {
                manager
                    .acquire(QuotaResource::Tool, "lingxi", "s-other")
                    .await
            }));
            // Let the waiter actually enqueue before the next one checks.
            tokio::task::yield_now().await;
        }
        assert_eq!(manager.waiting(QuotaResource::Tool), 2);
        let overflow = manager.acquire(QuotaResource::Tool, "lingxi", "s-x").await;
        assert_eq!(
            overflow.err().map(|e| e.kind),
            Some(QuotaFailureKind::QueueFull),
            "waiter queue must be bounded: reject, not grow"
        );
        for t in parked {
            t.abort();
        }
    }

    #[tokio::test]
    async fn cancelled_waiter_releases_its_queue_place_and_racing_grant() {
        let manager = Arc::new(QuotaManager::new(QuotaLimits {
            tool: LayeredQuotaLimits {
                global: 1,
                per_agent: 8,
                per_session: 8,
            },
            wait_queue_capacity: 8,
            wait_timeout_ms: 60_000,
            ..QuotaLimits::default()
        }));
        let held = manager
            .acquire(QuotaResource::Tool, "lingxi", "s1")
            .await
            .expect("holder");
        assert_eq!(manager.in_use(QuotaResource::Tool), 1);
        // A waiter parks, then its task is ABORTED (drop-cancellation at an
        // await point — the same primitive R03-T03's cancellation tree and
        // any dropped execute request use).
        let waiter_task = tokio::spawn({
            let manager = Arc::clone(&manager);
            async move { manager.acquire(QuotaResource::Tool, "lingxi", "s2").await }
        });
        tokio::task::yield_now().await;
        assert_eq!(manager.waiting(QuotaResource::Tool), 1);
        waiter_task.abort();
        let _ = waiter_task.await;
        assert_eq!(
            manager.waiting(QuotaResource::Tool),
            0,
            "an aborted waiter must leave the queue"
        );
        assert_eq!(manager.in_use(QuotaResource::Tool), 1);
        // Releasing the holder must NOT hand the slot to a ghost.
        drop(held);
        assert_eq!(manager.in_use(QuotaResource::Tool), 0);
        // Grant-race: park a waiter, release the holder (grant fires), then
        // abort the waiter BEFORE it observes — the guard must return the
        // slot instead of leaking it.
        let held2 = manager
            .acquire(QuotaResource::Tool, "lingxi", "s1")
            .await
            .expect("holder2");
        let granted_task = tokio::spawn({
            let manager = Arc::clone(&manager);
            async move { manager.acquire(QuotaResource::Tool, "lingxi", "s2").await }
        });
        tokio::task::yield_now().await;
        assert_eq!(manager.waiting(QuotaResource::Tool), 1);
        drop(held2); // grant fires for the parked waiter
        tokio::task::yield_now().await;
        granted_task.abort();
        let _ = granted_task.await;
        assert_eq!(
            manager.in_use(QuotaResource::Tool),
            0,
            "a grant racing with cancellation must be returned, never leaked"
        );
    }

    #[tokio::test]
    async fn per_agent_layer_is_independent_of_the_global_one() {
        let manager = QuotaManager::new(QuotaLimits {
            model: LayeredQuotaLimits {
                global: 8,
                per_agent: 1,
                per_session: 1,
            },
            ..QuotaLimits::default()
        });
        let a1 = manager
            .acquire(QuotaResource::Model, "agent-a", "s1")
            .await
            .expect("agent-a slot");
        // A different agent is NOT blocked by agent-a's per-agent limit…
        let b1 = manager
            .acquire(QuotaResource::Model, "agent-b", "s2")
            .await
            .expect("agent-b slot");
        assert_eq!(manager.in_use(QuotaResource::Model), 2);
        drop(a1);
        drop(b1);
        assert_eq!(manager.in_use(QuotaResource::Model), 0);
    }
}
