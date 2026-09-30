//! Bounded-resource guards: HTTP request-body limit, per-peer request rate
//! limiting, and a WebSocket connection ceiling (R02-T03 step 3).
//!
//! All limits are real (enforced on the hot path) and injectable for tests:
//! the limiter takes its window/max at construction, and the tests drive
//! small numbers plus an injected clock. Zero new dependencies — a fixed
//! window per peer key is enough for the R02 threat model (burst abuse from
//! one origin), and it fails closed (over limit ⇒ 429) rather than shedding
//! the limit under contention.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Default HTTP request-body limit (1 MiB). Applied via axum's
/// `DefaultBodyLimit`, so oversized bodies are rejected with 413 by the
/// framework before any handler JSON parsing runs.
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Maximum accepted UTF-8 length of a session-submission input (the task
/// content) — the service layer's OFFICIAL input budget (R03 repair
/// G05/F06). It is deliberately the SAME number as the transport budget
/// ([`DEFAULT_BODY_LIMIT_BYTES`] / [`WS_FRAME_LIMIT_BYTES`]): an input is
/// a JSON field inside a request body, so a legal HTTP/WS submission can
/// never carry a longer input than this. The service-level check is the
/// admission-time, loud defense-in-depth leg of that one budget (it also
/// covers in-process callers that bypass the transport); over it the
/// submission is refused with `input_too_large` BEFORE the busy gate,
/// run-id allocation or any durable side effect — input is NEVER
/// silently truncated, and log/display summaries are counts only, never
/// the executed content.
pub const MAX_SUBMISSION_INPUT_BYTES: usize = DEFAULT_BODY_LIMIT_BYTES;

/// Default per-peer HTTP rate budget within one window.
pub const DEFAULT_HTTP_RATE_MAX: u32 = 240;
/// Default rate window length.
pub const DEFAULT_HTTP_RATE_WINDOW_MS: u64 = 10_000;

/// Default maximum concurrently-upgraded WebSocket connections.
pub const DEFAULT_WS_MAX_CONNECTIONS: usize = 16;

/// Maximum size of a single inbound WS frame (control frames included).
pub const WS_FRAME_LIMIT_BYTES: usize = 1024 * 1024;

/// Default hard cap of the rate-limiter peer registry (R02 stage-repair R1
/// / F07): at most this many DISTINCT peers are tracked; a new peer beyond
/// the cap is rejected (`RegistryFull`) instead of growing the map without
/// bound. Expired windows are evicted before the cap is consulted, so a
/// peer that went quiet never occupies a slot forever.
pub const DEFAULT_RATE_MAX_PEERS: usize = 4096;

/// Default cap of concurrently in-flight HTTP requests (R02 stage-repair
/// R1 / F07): requests beyond the cap are rejected with 503 at admission
/// instead of piling up unboundedly at the accept/handler boundary.
pub const DEFAULT_HTTP_MAX_IN_FLIGHT: usize = 64;

/// Default per-request budget (R02 stage-repair R1 / F07): a request whose
/// handling (body read included) exceeds this budget is cancelled and
/// answered 408 — a slow-body client cannot hold a connection/admission
/// slot past the deadline.
pub const DEFAULT_HTTP_REQUEST_BUDGET_MS: u64 = 30_000;

/// Outcome of one rate-limiter check (F07): over-budget and
/// registry-full are DIFFERENT conditions — the first is the peer's own
/// window budget (429), the second is the service protecting its registry
/// against a peer flood (503).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateVerdict {
    Allowed,
    OverBudget,
    RegistryFull,
}

/// Fixed-window per-peer rate limiter with a HARD peer-registry cap.
pub struct RateLimiter {
    window_ms: u64,
    max: u32,
    max_peers: usize,
    inner: Mutex<HashMap<IpAddr, Window>>,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("window_ms", &self.window_ms)
            .field("max", &self.max)
            .field("max_peers", &self.max_peers)
            .finish_non_exhaustive()
    }
}

struct Window {
    start_ms: u64,
    count: u32,
}

impl RateLimiter {
    pub fn new(window_ms: u64, max: u32) -> Self {
        Self::with_max_peers(window_ms, max, DEFAULT_RATE_MAX_PEERS)
    }

    /// Full-injection constructor: the peer-registry cap is explicit.
    pub fn with_max_peers(window_ms: u64, max: u32, max_peers: usize) -> Self {
        Self {
            window_ms: window_ms.max(1),
            max,
            max_peers: max_peers.max(1),
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Records one request from `key`. The registry is hard-bounded
    /// (F07): expired windows are evicted first; if the map is still at
    /// the cap and `key` is not tracked, the verdict is
    /// [`RateVerdict::RegistryFull`] and NOTHING is inserted — the map can
    /// never outgrow `max_peers`, no matter how many distinct peers hit
    /// the window.
    pub fn check(&self, key: IpAddr, now_ms: u64) -> RateVerdict {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Eviction pass before the cap check: an expired window must never
        // occupy a registry slot (otherwise stale peers would crowd out
        // live ones).
        if inner.len() >= self.max_peers && !inner.contains_key(&key) {
            inner.retain(|_, w| now_ms.saturating_sub(w.start_ms) < self.window_ms);
            if inner.len() >= self.max_peers {
                return RateVerdict::RegistryFull;
            }
        }
        let window = inner.entry(key).or_insert(Window {
            start_ms: now_ms,
            count: 0,
        });
        if now_ms.saturating_sub(window.start_ms) >= self.window_ms {
            window.start_ms = now_ms;
            window.count = 0;
        }
        if window.count >= self.max {
            return RateVerdict::OverBudget;
        }
        window.count += 1;
        RateVerdict::Allowed
    }
}

/// HTTP admission gate (F07): a hard cap on concurrently in-flight
/// requests plus a per-request wall-clock budget. The cap is enforced at
/// the outer edge (every request, public or authenticated); the guard
/// frees the slot on drop — including cancellation and error paths.
pub struct HttpAdmission {
    max_in_flight: usize,
    request_budget_ms: u64,
    in_flight: AtomicUsize,
}

impl std::fmt::Debug for HttpAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpAdmission")
            .field("max_in_flight", &self.max_in_flight)
            .field("request_budget_ms", &self.request_budget_ms)
            .field("in_flight", &self.current())
            .finish()
    }
}

impl HttpAdmission {
    pub fn new(max_in_flight: usize, request_budget_ms: u64) -> Self {
        Self {
            max_in_flight: max_in_flight.max(1),
            request_budget_ms,
            in_flight: AtomicUsize::new(0),
        }
    }

    pub fn current(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    pub fn request_budget(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.request_budget_ms)
    }

    /// Reserves one in-flight slot; the returned guard frees it on drop
    /// (RAII — no manual decrement to forget on any path).
    pub fn acquire(self: &std::sync::Arc<Self>) -> Option<HttpAdmissionGuard> {
        let now = self.in_flight.fetch_add(1, Ordering::AcqRel);
        if now >= self.max_in_flight {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(HttpAdmissionGuard {
            owner: std::sync::Arc::clone(self),
        })
    }
}

pub struct HttpAdmissionGuard {
    owner: std::sync::Arc<HttpAdmission>,
}

impl Drop for HttpAdmissionGuard {
    fn drop(&mut self) {
        self.owner.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Hard cap of concurrently OPEN transport connections (R02 stage-repair
/// R2 / R2-F04): a connection is counted from `accept` until its socket
/// work ends — the wait for complete request headers INCLUDED. The
/// request-level [`HttpAdmission`] only ever sees a request AFTER its
/// headers parsed, so without this gate any number of slow-headers (or
/// never-headers) clients could hold sockets open entirely outside the
/// configured count/time budgets. Over the cap the freshly-accepted
/// socket is rejected immediately (closed with the transport marker
/// logged); the slot frees when the connection task ends — every exit
/// path, header timeout and graceful close included (RAII).
pub struct ConnectionAdmission {
    max: usize,
    current: AtomicUsize,
}

impl std::fmt::Debug for ConnectionAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionAdmission")
            .field("max", &self.max)
            .field("current", &self.current())
            .finish()
    }
}

impl ConnectionAdmission {
    pub fn new(max: usize) -> Self {
        Self {
            max: max.max(1),
            current: AtomicUsize::new(0),
        }
    }

    pub fn current(&self) -> usize {
        self.current.load(Ordering::Acquire)
    }

    pub fn max(&self) -> usize {
        self.max
    }

    /// Reserves one connection slot; the returned guard frees it on drop
    /// (RAII — no manual decrement to forget on any exit path).
    pub fn acquire(self: &std::sync::Arc<Self>) -> Option<ConnectionAdmissionGuard> {
        let now = self.current.fetch_add(1, Ordering::AcqRel);
        if now >= self.max {
            self.current.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(ConnectionAdmissionGuard {
            owner: std::sync::Arc::clone(self),
        })
    }
}

pub struct ConnectionAdmissionGuard {
    owner: std::sync::Arc<ConnectionAdmission>,
}

impl Drop for ConnectionAdmissionGuard {
    fn drop(&mut self) {
        self.owner.current.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Ceiling on concurrently-upgraded WebSocket connections.
pub struct WsConnectionCounter {
    max: usize,
    current: AtomicUsize,
}

impl std::fmt::Debug for WsConnectionCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsConnectionCounter")
            .field("max", &self.max)
            .field("current", &self.current())
            .finish()
    }
}

impl WsConnectionCounter {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            current: AtomicUsize::new(0),
        }
    }

    pub fn current(&self) -> usize {
        self.current.load(Ordering::Acquire)
    }

    /// Reserves one connection slot; the returned guard frees it on drop
    /// (including on error paths — RAII, no manual decrement to forget).
    /// The guard owns an `Arc` to the counter so it can ride into a
    /// `'static` task (the upgraded socket outlives the request).
    pub fn acquire(self: &std::sync::Arc<Self>) -> Option<WsConnectionGuard> {
        let now = self.current.fetch_add(1, Ordering::AcqRel);
        if now >= self.max {
            self.current.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(WsConnectionGuard {
            owner: std::sync::Arc::clone(self),
        })
    }
}

pub struct WsConnectionGuard {
    owner: std::sync::Arc<WsConnectionCounter>,
}

impl Drop for WsConnectionGuard {
    fn drop(&mut self) {
        self.owner.current.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_fixed_window_behavior() {
        let limiter = RateLimiter::new(1000, 3);
        let ip: IpAddr = "127.0.0.9".parse().unwrap();
        assert_eq!(limiter.check(ip, 0), RateVerdict::Allowed);
        assert_eq!(limiter.check(ip, 500), RateVerdict::Allowed);
        assert_eq!(limiter.check(ip, 999), RateVerdict::Allowed);
        assert_eq!(
            limiter.check(ip, 999),
            RateVerdict::OverBudget,
            "4th request in window must be over budget"
        );
        // New window: allowed again.
        assert_eq!(limiter.check(ip, 1000), RateVerdict::Allowed);
        // Independent peer key.
        let other: IpAddr = "127.0.0.8".parse().unwrap();
        assert_eq!(limiter.check(other, 1000), RateVerdict::Allowed);
    }

    #[test]
    fn rate_limiter_peer_registry_has_a_hard_cap_with_eviction() {
        let limiter = RateLimiter::with_max_peers(1000, 10, 2);
        let p1: IpAddr = "127.0.0.1".parse().unwrap();
        let p2: IpAddr = "127.0.0.2".parse().unwrap();
        let p3: IpAddr = "127.0.0.3".parse().unwrap();
        assert_eq!(limiter.check(p1, 0), RateVerdict::Allowed);
        assert_eq!(limiter.check(p2, 0), RateVerdict::Allowed);
        // A third DISTINCT peer inside the same window hits the registry
        // cap — rejected, and the registry does NOT grow.
        assert_eq!(limiter.check(p3, 100), RateVerdict::RegistryFull);
        assert_eq!(limiter.check(p3, 200), RateVerdict::RegistryFull);
        // Existing peers are unaffected by the cap.
        assert_eq!(limiter.check(p1, 300), RateVerdict::Allowed);
        // After the window expires, eviction frees the slots and the new
        // peer is admitted again (no permanent crowd-out).
        assert_eq!(limiter.check(p3, 1001), RateVerdict::Allowed);
        assert_eq!(limiter.check(p1, 1001), RateVerdict::Allowed);
        // …and the cap still holds for the NEXT stranger.
        assert_eq!(limiter.check(p2, 1002), RateVerdict::RegistryFull);
    }

    #[test]
    fn http_admission_enforces_cap_and_frees_on_drop() {
        let admission = std::sync::Arc::new(HttpAdmission::new(1, 30_000));
        let g1 = admission.acquire().expect("first slot");
        assert!(admission.acquire().is_none(), "overflow must be refused");
        assert_eq!(admission.current(), 1);
        drop(g1);
        assert_eq!(admission.current(), 0);
        let _g2 = admission.acquire().expect("freed slot reusable");
        assert_eq!(
            admission.request_budget(),
            std::time::Duration::from_millis(30_000)
        );
    }

    #[test]
    fn connection_admission_enforces_cap_and_frees_on_drop() {
        let admission = std::sync::Arc::new(ConnectionAdmission::new(2));
        let g1 = admission.acquire().expect("first slot");
        let g2 = admission.acquire().expect("second slot");
        assert!(
            admission.acquire().is_none(),
            "third open connection must be refused at the hard cap"
        );
        assert_eq!(admission.current(), 2);
        assert_eq!(admission.max(), 2);
        drop(g1);
        assert_eq!(admission.current(), 1);
        let g3 = admission.acquire().expect("freed slot reusable");
        drop(g2);
        drop(g3);
        assert_eq!(admission.current(), 0);
    }

    #[test]
    fn ws_connection_counter_enforces_ceiling_and_frees() {
        let counter = std::sync::Arc::new(WsConnectionCounter::new(2));
        let g1 = counter.acquire().expect("first slot");
        let g2 = counter.acquire().expect("second slot");
        assert!(
            counter.acquire().is_none(),
            "third concurrent must be refused"
        );
        assert_eq!(counter.current(), 2);
        drop(g2);
        assert_eq!(counter.current(), 1);
        let g3 = counter.acquire().expect("freed slot reusable");
        drop(g1);
        drop(g3);
        assert_eq!(counter.current(), 0);
    }
}
