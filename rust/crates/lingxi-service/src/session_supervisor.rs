//! Session serialization owner (R03-T02 deliverable "SessionSupervisor"):
//! ONE explicit owner per session processes that session's state changes,
//! and the model/tool I/O of a running turn happens OUTSIDE any supervisor
//! lock.
//!
//! Frozen semantics this maps (read from the incumbent Node production
//! stack before mapping — the taskbook forbids inventing new interaction
//! semantics):
//! - NORMAL new submission to a BUSY session is REJECTED, not queued:
//!   `core/desktop-session-submit.ts` gates on
//!   `pendingDesktopSessionSubmissions` / `engine.isSessionStreaming` and
//!   throws `session_busy`; `server/routes/sessions.ts` answers 409
//!   `{error:"session_busy"}`; the chat route maps it to the stable code
//!   `session_busy` with `retryable: true` (client-side retry is the
//!   adopted queueing behavior — there is NO server-side queue of normal
//!   inputs in the incumbent).
//! - STEERING / follow-up while the session is running does NOT interrupt
//!   the loop: `session-coordinator.steerSession` → `session.steer(text)`
//!   ("不打断循环，切 Run 语义层", P02 STATE_TRANSITIONS R5). The steered
//!   text reaches the agent loop's NEXT model call. When the session is
//!   idle a steer is a MISS and falls back to a normal submission
//!   (`server/routes/chat.ts` "steer missed, falling back to prompt").
//!   The runSplit history projection of a steered commit is R06; this
//!   module owns the steering CHANNEL, not the projection.
//!
//! Rust mapping: the busy window spans the whole `execute_for` drive
//! (pending → streaming → single finalize — the incumbent's two busy
//! markers are the same window here because the Rust execute drives the
//! full lifecycle). A steered text lands in the session's bounded
//! [`SteeringInbox`] and is drained by the run driver before each provider
//! turn; leftover steering at run end is RETAINED for the session's next
//! run (never silently dropped — the incumbent's steered user message is
//! durable session input).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Session-surface knobs (injected through `ServiceDeps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConcurrencyLimits {
    /// Bounded steering inbox per session: steering beyond this is a loud
    /// rejection, never an unbounded backlog (>= 1).
    pub steering_inbox_capacity: usize,
    /// Hard cap of TRACKED session slots (busy leases + retained steering).
    /// Idle, empty slots are evicted before the cap is consulted, so a
    /// finished session never pins a slot forever (mirror of the R02
    /// rate-limiter registry cap; >= 1).
    pub registry_cap: usize,
}

impl SessionConcurrencyLimits {
    pub const DEFAULT_STEERING_INBOX_CAPACITY: usize = 8;
    pub const DEFAULT_REGISTRY_CAP: usize = 1024;

    /// Loud validation (a 0 capacity would silently disable steering or the
    /// session registry — both are startup errors, never clamps).
    pub fn validate(&self) -> Result<(), lingxi_kernel::ports::StorageError> {
        use lingxi_kernel::ports::StorageError;
        if self.steering_inbox_capacity == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "steering_inbox_capacity must be >= 1 (0 would silently disable \
                         steering)"
                    .to_string(),
            });
        }
        if self.registry_cap == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "session registry_cap must be >= 1 (0 would reject every session)"
                    .to_string(),
            });
        }
        Ok(())
    }
}

impl Default for SessionConcurrencyLimits {
    fn default() -> Self {
        Self {
            steering_inbox_capacity: Self::DEFAULT_STEERING_INBOX_CAPACITY,
            registry_cap: Self::DEFAULT_REGISTRY_CAP,
        }
    }
}

/// Submission kinds the session surface distinguishes (R03-T02 step 2:
/// 普通新提交与 steering/follow-up 不混为一谈).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionKind {
    /// A normal new user submission. Rejected with `session_busy` while
    /// the session is running its current run (frozen incumbent gate).
    NewTurn,
    /// A steering / follow-up input mid-run. Accepted while the session is
    /// busy (bounded); a MISS while idle (caller falls back to a new
    /// submission — frozen incumbent fallback).
    Steering,
}

/// Outcome of one busy-gate reservation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusyGateError {
    /// The session already has an owner driving a run: the frozen
    /// `session_busy` rejection (retryable).
    Busy,
    /// The tracked-slot registry is at its hard cap and no idle slot can
    /// be evicted (service protection; mirror of the R02 registry-full
    /// distinction).
    RegistryFull,
}

/// Failure of a steering submission (distinct from the busy gate: an
/// over-full steering inbox is its own loud condition).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerError {
    /// The bounded steering inbox of the busy session is full — the text
    /// was NOT accepted; the caller decides (frozen incumbent behavior
    /// bounds steering too: preflight input limits).
    InboxFull,
}

/// Outcome of a steering submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerOutcome {
    /// The session is busy and the steering text was accepted into the
    /// run's steering channel.
    Accepted,
    /// The session is idle: steering missed — the caller falls back to a
    /// normal submission (frozen incumbent behavior).
    Miss,
}

/// Bounded per-session steering channel. The run driver drains it before
/// each provider turn; leftover text survives to the session's next run.
#[derive(Debug, Default)]
pub struct SteeringInbox {
    pending: Mutex<VecDeque<String>>,
}

impl SteeringInbox {
    fn push_bounded(&self, text: String, capacity: usize) -> Result<(), SteerOverflow> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.len() >= capacity {
            return Err(SteerOverflow);
        }
        pending.push_back(text);
        Ok(())
    }

    /// Drains everything pending, joined for the next provider turn's
    /// input. Returns None when nothing new arrived.
    pub fn drain_joined(&self) -> Option<String> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.is_empty() {
            return None;
        }
        let joined = pending.drain(..).collect::<Vec<_>>().join("\n");
        Some(joined)
    }

    fn len(&self) -> usize {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

struct SteerOverflow;

struct SessionSlot {
    busy: bool,
    inbox: Arc<SteeringInbox>,
}

/// The session supervisor: the registry of per-session owners. The internal
/// lock is held for O(1) registry work ONLY — model/tool I/O happens while
/// the caller holds a [`SessionLease`], which owns no lock.
pub struct SessionSupervisor {
    limits: SessionConcurrencyLimits,
    slots: Mutex<HashMap<String, SessionSlot>>,
}

impl std::fmt::Debug for SessionSupervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionSupervisor")
            .field("limits", &self.limits)
            .field("active_runs", &self.active_run_count())
            .finish_non_exhaustive()
    }
}

/// The owner token of one session's current run: exactly one exists per
/// busy session at any time. Dropping it (normal settle, error, timeout or
/// cancellation of the driving future) frees the session for the next
/// submission — RAII, no path forgets.
pub struct SessionLease {
    supervisor: Arc<SessionSupervisor>,
    session_id: String,
    inbox: Arc<SteeringInbox>,
}

impl std::fmt::Debug for SessionLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SessionLease(<{}>)", self.session_id)
    }
}

impl SessionLease {
    /// The run's steering channel (the driver drains it before each
    /// provider turn).
    pub fn steering_inbox(&self) -> &SteeringInbox {
        &self.inbox
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        let mut slots = self
            .supervisor
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(slot) = slots.get_mut(&self.session_id) {
            slot.busy = false;
            // A slot stays TRACKED while it still holds steering for the
            // session's next run; otherwise it may be evicted at the cap.
            if slot.inbox.len() == 0 {
                slots.remove(&self.session_id);
            }
        }
    }
}

impl SessionSupervisor {
    pub fn new(limits: SessionConcurrencyLimits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            slots: Mutex::new(HashMap::new()),
        })
    }

    pub fn limits(&self) -> &SessionConcurrencyLimits {
        &self.limits
    }

    /// Reserves the session for ONE run (the "明确 owner"). Non-blocking by
    /// design: a busy session is REJECTED per the frozen incumbent gate —
    /// a server-side queue of normal inputs does not exist in the adopted
    /// semantics.
    pub fn try_begin_run(
        self: &Arc<Self>,
        session_id: &str,
    ) -> Result<SessionLease, BusyGateError> {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(slot) = slots.get_mut(session_id) {
            if slot.busy {
                return Err(BusyGateError::Busy);
            }
            slot.busy = true;
            return Ok(SessionLease {
                supervisor: Arc::clone(self),
                session_id: session_id.to_string(),
                inbox: Arc::clone(&slot.inbox),
            });
        }
        if slots.len() >= self.limits.registry_cap {
            // Evict idle, steering-free slots before refusing.
            slots.retain(|_, slot| slot.busy || slot.inbox.len() > 0);
            if slots.len() >= self.limits.registry_cap {
                return Err(BusyGateError::RegistryFull);
            }
        }
        let inbox = Arc::new(SteeringInbox::default());
        slots.insert(
            session_id.to_string(),
            SessionSlot {
                busy: true,
                inbox: Arc::clone(&inbox),
            },
        );
        Ok(SessionLease {
            supervisor: Arc::clone(self),
            session_id: session_id.to_string(),
            inbox,
        })
    }

    /// Submits steering for a busy session (frozen semantics: never
    /// interrupts the loop; idle sessions MISS and the caller falls back).
    pub fn steering_submit(
        self: &Arc<Self>,
        session_id: &str,
        text: &str,
    ) -> Result<SteerOutcome, SteerError> {
        let slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(slot) = slots.get(session_id) else {
            return Ok(SteerOutcome::Miss);
        };
        if !slot.busy {
            return Ok(SteerOutcome::Miss);
        }
        match slot
            .inbox
            .push_bounded(text.to_string(), self.limits.steering_inbox_capacity)
        {
            Ok(()) => Ok(SteerOutcome::Accepted),
            Err(SteerOverflow) => Err(SteerError::InboxFull),
        }
    }

    /// Observability: how many sessions currently own a run.
    pub fn active_run_count(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .filter(|slot| slot.busy)
            .count()
    }

    /// Observability: whether one session currently owns a run.
    pub fn is_busy(&self, session_id: &str) -> bool {
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(session_id)
            .map(|slot| slot.busy)
            .unwrap_or(false)
    }

    /// Observability: pending steering of one session (leftover steering
    /// from a finished run is retained for the next run — this counts it).
    pub fn steering_pending(&self, session_id: &str) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(session_id)
            .map(|slot| slot.inbox.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degenerate_session_limits_refuse_validation() {
        assert!(SessionConcurrencyLimits::default().validate().is_ok());
        assert!(SessionConcurrencyLimits {
            steering_inbox_capacity: 0,
            registry_cap: 4,
        }
        .validate()
        .is_err());
        assert!(SessionConcurrencyLimits {
            steering_inbox_capacity: 4,
            registry_cap: 0,
        }
        .validate()
        .is_err());
    }

    #[test]
    fn busy_gate_admits_one_owner_and_rejects_the_rest() {
        let supervisor = SessionSupervisor::new(SessionConcurrencyLimits::default());
        let lease = supervisor.try_begin_run("sess_a").expect("first owner");
        assert!(supervisor.is_busy("sess_a"));
        assert_eq!(supervisor.active_run_count(), 1);
        assert_eq!(
            supervisor.try_begin_run("sess_a").unwrap_err(),
            BusyGateError::Busy,
            "second submission to a busy session is the frozen session_busy rejection"
        );
        // Other sessions are unaffected — no global lock.
        let other = supervisor.try_begin_run("sess_b").expect("other session");
        assert_eq!(supervisor.active_run_count(), 2);
        drop(other);
        drop(lease);
        assert!(!supervisor.is_busy("sess_a"));
        assert_eq!(supervisor.active_run_count(), 0);
        // The slot is reusable after the lease ends.
        assert!(supervisor.try_begin_run("sess_a").is_ok());
    }

    #[test]
    fn steering_is_accepted_while_busy_misses_while_idle_and_is_bounded() {
        let supervisor = SessionSupervisor::new(SessionConcurrencyLimits {
            steering_inbox_capacity: 2,
            registry_cap: 8,
        });
        assert_eq!(
            supervisor.steering_submit("sess_a", "idle steer").unwrap(),
            SteerOutcome::Miss,
            "idle session: steering misses (caller falls back to a new submission)"
        );
        let lease = supervisor.try_begin_run("sess_a").expect("owner");
        assert_eq!(
            supervisor.steering_submit("sess_a", "first steer").unwrap(),
            SteerOutcome::Accepted
        );
        assert_eq!(
            supervisor
                .steering_submit("sess_a", "second steer")
                .unwrap(),
            SteerOutcome::Accepted
        );
        assert_eq!(supervisor.steering_pending("sess_a"), 2);
        // Bounded inbox: the third steering text is a loud rejection.
        assert_eq!(
            supervisor.steering_submit("sess_a", "third").unwrap_err(),
            SteerError::InboxFull,
            "steering inbox is bounded"
        );
        // The driver drains the channel.
        let drained = lease.steering_inbox().drain_joined().unwrap();
        assert_eq!(drained, "first steer\nsecond steer");
        assert!(lease.steering_inbox().drain_joined().is_none());
        drop(lease);
    }

    #[test]
    fn leftover_steering_survives_the_run_for_the_session_next_turn() {
        let supervisor = SessionSupervisor::new(SessionConcurrencyLimits::default());
        let lease = supervisor.try_begin_run("sess_a").expect("owner");
        supervisor
            .steering_submit("sess_a", "arrived too late for this run")
            .unwrap();
        drop(lease); // run ends without consuming it
        assert_eq!(supervisor.steering_pending("sess_a"), 1);
        // The next run of the session starts with the retained steering.
        let next = supervisor.try_begin_run("sess_a").expect("next owner");
        assert_eq!(
            next.steering_inbox().drain_joined().as_deref(),
            Some("arrived too late for this run")
        );
        // And once drained, the slot may be evicted at the cap again.
        drop(next);
        assert_eq!(supervisor.steering_pending("sess_a"), 0);
    }

    #[test]
    fn registry_cap_evicts_idle_slots_and_refuses_at_the_cap() {
        let supervisor = SessionSupervisor::new(SessionConcurrencyLimits {
            steering_inbox_capacity: 2,
            registry_cap: 2,
        });
        let a = supervisor.try_begin_run("sess_a").expect("a");
        let b = supervisor.try_begin_run("sess_b").expect("b");
        // Idle, steering-free sessions are evictable — but a and b are BUSY.
        assert_eq!(
            supervisor.try_begin_run("sess_c").unwrap_err(),
            BusyGateError::RegistryFull
        );
        drop(a);
        drop(b);
        // Both slots were removed on lease drop (idle + empty) — new
        // sessions fit again.
        assert!(supervisor.try_begin_run("sess_c").is_ok());
    }
}
