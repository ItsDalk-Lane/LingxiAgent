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

/// Default per-peer HTTP rate budget within one window.
pub const DEFAULT_HTTP_RATE_MAX: u32 = 240;
/// Default rate window length.
pub const DEFAULT_HTTP_RATE_WINDOW_MS: u64 = 10_000;

/// Default maximum concurrently-upgraded WebSocket connections.
pub const DEFAULT_WS_MAX_CONNECTIONS: usize = 16;

/// Maximum size of a single inbound WS frame (control frames included).
pub const WS_FRAME_LIMIT_BYTES: usize = 1024 * 1024;

/// Fixed-window per-peer rate limiter.
pub struct RateLimiter {
    window_ms: u64,
    max: u32,
    inner: Mutex<HashMap<IpAddr, Window>>,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("window_ms", &self.window_ms)
            .field("max", &self.max)
            .finish_non_exhaustive()
    }
}

struct Window {
    start_ms: u64,
    count: u32,
}

impl RateLimiter {
    pub fn new(window_ms: u64, max: u32) -> Self {
        Self {
            window_ms: window_ms.max(1),
            max,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Records one request from `key`; returns `true` when it is within
    /// budget. Also opportunistically prunes expired windows so the map
    /// stays bounded by (distinct peers × window), not by total requests.
    pub fn check(&self, key: IpAddr, now_ms: u64) -> bool {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Bound the map: if it grew pathologically, drop expired entries.
        if inner.len() > 4096 {
            inner.retain(|_, w| now_ms.saturating_sub(w.start_ms) < self.window_ms);
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
            return false;
        }
        window.count += 1;
        true
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
        assert!(limiter.check(ip, 0));
        assert!(limiter.check(ip, 500));
        assert!(limiter.check(ip, 999));
        assert!(
            !limiter.check(ip, 999),
            "4th request in window must be over budget"
        );
        // New window: allowed again.
        assert!(limiter.check(ip, 1000));
        // Independent peer key.
        let other: IpAddr = "127.0.0.8".parse().unwrap();
        assert!(limiter.check(other, 1000));
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
