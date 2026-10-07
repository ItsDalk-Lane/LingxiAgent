//! Injectable clock and ID generation (R02-T07 step 4).
//!
//! Production wiring uses [`SystemClock`] + [`RandomRequestIdGen`]; tests
//! inject [`ManualClock`] and [`SequentialRequestIdGen`] so ordering and
//! expiry behavior is proven deterministically — never with random sleeps
//! (the established fixed-scheduling style of the R02 suites).
//!
//! Injection surface: the traits are objects and live on
//! [`crate::ServiceState`] (built via `ServiceDeps`), so the served request
//! paths — per-peer rate limiting, WS ticket issue/consume, execute
//! timestamps, request-id minting — are all clock/ID driven through the
//! same injection point. Startup-only timestamps that happen before a
//! `ServiceState` exists (auth bootstrap, instance identity) keep the
//! documented system-clock behavior.

use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic-enough wall clock in UNIX milliseconds (the service's only
/// time vocabulary — same units as the incumbent `now_unix_ms`).
pub trait ServiceClock: Send + Sync + std::fmt::Debug {
    fn now_unix_ms(&self) -> u64;
}

/// Production clock: the real system time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl ServiceClock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Test clock: the value is set/advanced by the test — zero sleeps, zero
/// flakes.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(start_unix_ms: u64) -> Self {
        Self(AtomicU64::new(start_unix_ms))
    }

    pub fn set(&self, unix_ms: u64) {
        self.0.store(unix_ms, Ordering::Release);
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::AcqRel);
    }
}

impl ServiceClock for ManualClock {
    fn now_unix_ms(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}

/// Per-request correlation id minting (the id is carried in request
/// extensions, machine-readable rejection markers and error response
/// bodies so any diagnostic can be traced back to its request — the A13
/// "关联 ID 保留" anchor).
pub trait RequestIdGen: Send + Sync + std::fmt::Debug {
    fn next_request_id(&self) -> String;
}

/// Production source: the system secure random device, same entropy
/// authority as the auth secrets (`req-` + 16 random bytes hex).
#[derive(Debug, Clone, Copy, Default)]
pub struct RandomRequestIdGen;

impl RequestIdGen for RandomRequestIdGen {
    fn next_request_id(&self) -> String {
        // R9-F05: request ids are CORRELATION handles, not credentials —
        // they never key authorization. The system CSPRNG is the only
        // random source; when it fails the id degrades EXPLICITLY (a
        // `req-degraded-` prefix plus a per-process counter, and a warn
        // log) so every consumer can SEE the degradation — never a silent
        // fallback that looks random. Auth credentials never take this
        // path: they refuse issuance instead (auth.rs).
        match crate::auth::hex_random_public(16) {
            Ok(hex) => format!("req-{hex}"),
            Err(err) => {
                static DEGRADED_SEQ: AtomicU64 = AtomicU64::new(0);
                tracing::warn!(
                    error = %err,
                    "system secure random source unavailable: minting an EXPLICITLY degraded \
                     request id (correlation only, never a credential)"
                );
                format!(
                    "req-degraded-{}-{}",
                    std::process::id(),
                    DEGRADED_SEQ.fetch_add(1, Ordering::AcqRel)
                )
            }
        }
    }
}

/// Deterministic source for tests: `req-0000000000000001`, `…0002`, … —
/// the id sequence itself becomes the ordering witness.
#[derive(Debug, Default)]
pub struct SequentialRequestIdGen(AtomicU64);

impl SequentialRequestIdGen {
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }
}

impl RequestIdGen for SequentialRequestIdGen {
    fn next_request_id(&self) -> String {
        format!("req-{:016x}", self.0.fetch_add(1, Ordering::AcqRel) + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_drives_time_without_sleeps() {
        let clock = ManualClock::new(1_000);
        assert_eq!(clock.now_unix_ms(), 1_000);
        clock.advance(250);
        assert_eq!(clock.now_unix_ms(), 1_250);
        clock.set(5_000);
        assert_eq!(clock.now_unix_ms(), 5_000);
    }

    #[test]
    fn system_clock_reads_unix_milliseconds() {
        let now = SystemClock.now_unix_ms();
        // 2026-09-26 is about 1.79e12 ms; a wrong unit (seconds/nanos) or a
        // pre-2021 clock is a bug.
        assert!(now > 1_700_000_000_000, "implausible now: {now}");
    }

    #[test]
    fn sequential_ids_are_deterministic_ordering_witnesses() {
        let gen = SequentialRequestIdGen::new();
        assert_eq!(gen.next_request_id(), "req-0000000000000001");
        assert_eq!(gen.next_request_id(), "req-0000000000000002");
    }

    #[test]
    fn random_ids_carry_the_request_prefix_and_are_unique() {
        let gen = RandomRequestIdGen;
        let a = gen.next_request_id();
        let b = gen.next_request_id();
        // The healthy path: 16 system-CSPRNG bytes as hex. (An EXPLICITLY
        // degraded `req-degraded-…` id is only minted when the OS CSPRNG
        // fails — R9-F05; not exercised here, this machine has one.)
        assert!(a.starts_with("req-") && a.len() == "req-".len() + 32);
        assert_ne!(a, b);
    }
}
