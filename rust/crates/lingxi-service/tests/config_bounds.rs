//! R02 stage-repair R3 / R3-F02 — binary-level resource-bound tests.
//!
//! The REAL `lingxi-service` binary must reject an out-of-range
//! resource-limit value with exit code 2 BEFORE any readiness line: the
//! pre-fix binary instead PANICKED (exit 101 inside tokio's
//! bounded-channel semaphore for `--db-queue-bound 18446744073709551615`)
//! or silently truncated (`--http-rate-max 4294967296` became 0 via
//! `as u32` and every request answered 429 after a READY line).
//!
//! Every case runs with `--test-mode` and compile-time literal arguments
//! only, executed directly as an argv array (there is no shell anywhere
//! in this test). The rejection happens inside `parse_cli` (main.rs),
//! which runs BEFORE any home resolution or directory creation — so a
//! rejected startup leaves no data home and no service layout, by
//! construction.
//!
//! These tests only check REJECTION — per the R3 review discipline no
//! test allocates a huge resource; acceptance of the documented boundary
//! values is proved by the parse-level unit tests in `src/config.rs`
//! (parsing inspects the number only).

/// Asserts the loud exit-2 rejection contract for one literal flag/value
/// pair: exit code 2, the error names the flag, and no readiness line was
/// printed. Both arguments must be string LITERALS at the call site.
macro_rules! assert_exit2_rejection {
    ($flag:literal, $value:literal) => {{
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_lingxi-service"))
            .args(["--test-mode", $flag, $value])
            .output()
            .expect("spawn lingxi-service");
        let code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            code, 2,
            "{} {}: expected exit 2 (loud config rejection), got {code}\nstdout: {stdout}\nstderr: {stderr}",
            $flag, $value,
        );
        assert!(
            stderr.contains($flag),
            "{} {}: the rejection must name the flag, stderr: {stderr}",
            $flag, $value,
        );
        assert!(
            !stdout.contains("LINGXI_SERVICE_READY"),
            "{} {}: no readiness line may be printed, stdout: {stdout}",
            $flag, $value,
        );
    }};
}

/// The R3-F02 case-1 value: above tokio's semaphore maximum the pre-fix
/// binary panicked with exit 101 deep inside `mpsc::channel`; the fixed
/// binary rejects at parse time with exit 2.
#[test]
fn db_queue_bound_above_the_tokio_bound_is_exit2_not_a_panic() {
    assert_exit2_rejection!("--db-queue-bound", "18446744073709551615");
    // tokio Semaphore::MAX_PERMITS + 1 — the exact boundary violation.
    assert_exit2_rejection!("--db-queue-bound", "2305843009213693952");
}

/// The R3-F02 case-2 value: u32::MAX + 1 truncated to 0 via `as u32`
/// pre-fix (READY, then a permanent 429 for every peer); the fixed binary
/// rejects it at parse time.
#[test]
fn http_rate_max_above_u32_is_exit2_not_a_truncation() {
    assert_exit2_rejection!("--http-rate-max", "4294967296");
    assert_exit2_rejection!("--http-rate-max", "18446744073709551615");
}

/// Above usize::MAX/2 the 2x connection-cap derivation could overflow —
/// pre-fix it saturated silently; the fixed binary rejects at parse time.
#[test]
fn http_max_in_flight_above_the_derivation_bound_is_exit2() {
    assert_exit2_rejection!("--http-max-in-flight", "9223372036854775808");
    assert_exit2_rejection!("--http-max-in-flight", "18446744073709551615");
}

/// Time budgets above 30 days risk overflowing platform monotonic-clock
/// timer arithmetic at runtime; all three time flags reject loudly.
#[test]
fn time_budgets_above_the_platform_safe_bound_are_exit2() {
    assert_exit2_rejection!("--http-request-budget-ms", "2592000001");
    assert_exit2_rejection!("--db-wait-budget-ms", "2592000001");
    assert_exit2_rejection!("--db-wait-budget-ms", "18446744073709551615");
    assert_exit2_rejection!("--shutdown-timeout-ms", "2592000001");
}

/// Lower-bound violations are the same loud exit-2 class.
#[test]
fn below_minimum_values_are_exit2() {
    assert_exit2_rejection!("--event-subscriber-queue", "1");
    assert_exit2_rejection!("--log-max-bytes", "63");
    assert_exit2_rejection!("--log-max-files", "1");
    assert_exit2_rejection!("--max-ws-connections", "0");
}
