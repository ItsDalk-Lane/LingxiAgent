//! R04-RR1-F05 acceptance: output COMPLETENESS is decided by the real
//! facts of the whole stream (total bytes, true head/tail retention,
//! in-memory evictions, spill cap/spill errors), never by whether the
//! rolling tail window happened to fit the return budget.
//!
//! Everything here runs the REAL native chain: the REAL
//! `ToolInvocationGateway` (prepare → execute_prepared), the REAL
//! `ProcessTools::run_exec_command` executor registered through
//! `register_process_tools`, and the REAL `ProcessSupervisor` spawning
//! REAL `/bin/bash -c` children. Every size/marker assertion is checked
//! against the actual on-disk spill file and the actual returned bytes.
//!
//! Test-double boundary: no model/provider doubles — the calls go
//! straight through the gateway's user-run surface. All test state lives
//! in unique `std::env::temp_dir()` subdirectories (never a shared /tmp
//! wildcard).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lingxi_kernel::ports::{ToolExecutionResult, ToolOutcome, ToolRequest, ToolRunStatus};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetId};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ToolCallId};
use lingxi_service::approval_service::ApprovalService;
use lingxi_service::exectools::register_process_tools;
use lingxi_service::inject::SystemClock;
use lingxi_service::procsupervisor::{ProcessHandleId, ProcessSupervisor, SupervisorLimits};
use lingxi_service::resourceaccess::ResourceAccess;
use lingxi_service::toolgateway::{
    CallerSurface, InvocationPermissionContext, ToolInvocationGateway, ToolPolicyPort,
    DEFAULT_LIVE_PREPARED_CAP, DEFAULT_PREPARED_TTL_MS,
};
use serde_json::json;

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

// ── harness (real gateway + real process tools, no ServiceState needed) ─────

struct IntegrityHarness {
    #[allow(dead_code)]
    registry: Arc<ToolRegistry>,
    gateway: Arc<ToolInvocationGateway>,
    supervisor: Arc<ProcessSupervisor>,
    exec_target: ToolTargetId,
    write_stdin_target: ToolTargetId,
    #[allow(dead_code)]
    root: PathBuf,
    #[allow(dead_code)]
    ws: PathBuf,
    spill_dir: PathBuf,
}

/// A unique test directory (pid + nanos + sequence — no cross-test
/// collisions, never the shared /tmp itself).
fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r04rr1f05-{tag}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        seq
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Builds the REAL chain with the DEFAULT knobs (100 KiB output window,
/// 64 MiB spill cap) unless a leg overrides them.
fn integrity_harness() -> IntegrityHarness {
    integrity_harness_with(SupervisorLimits::with_spill_dir(test_dir("spill")))
}

fn integrity_harness_with(limits: SupervisorLimits) -> IntegrityHarness {
    let root = test_dir("root");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("workspace");
    std::fs::create_dir_all(&limits.spill_dir).expect("spill dir");
    let registry = Arc::new(ToolRegistry::new());
    let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let approvals = Arc::new(ApprovalService::new(Arc::new(SystemClock)));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn ToolPolicyPort>,
        Arc::new(SystemClock),
        budget(),
        DEFAULT_PREPARED_TTL_MS,
        DEFAULT_LIVE_PREPARED_CAP,
    ));
    let supervisor = Arc::new(
        ProcessSupervisor::new(Arc::new(SystemClock), limits.clone()).expect("supervisor"),
    );
    let core = register_process_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&supervisor),
        Arc::clone(&access),
        ws.clone(),
        None, // the unsandboxed T05 shape; the sandbox face has its own suite
        Arc::new(SystemClock),
        &budget(),
    );
    IntegrityHarness {
        registry,
        gateway,
        supervisor,
        exec_target: core.exec_target,
        write_stdin_target: core.write_stdin_target,
        root,
        ws,
        spill_dir: limits.spill_dir,
    }
}

async fn integrity_teardown(h: &IntegrityHarness) {
    let _ = h.supervisor.shutdown_all().await;
    let _ = std::fs::remove_dir_all(&h.spill_dir);
}

fn kernel_ctx(session: &str, run: &str) -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(session.to_string()),
        run_id: lingxi_protocol::RunId::new(run.to_string()),
        attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}

fn user_operate() -> InvocationPermissionContext {
    InvocationPermissionContext::UserSession {
        mode: SessionPermissionMode::Operate,
    }
}

/// Direct gateway call (prepare → execute_prepared) — the same two steps
/// the run driver performs, minus the driving loop.
async fn call_exec(
    h: &IntegrityHarness,
    ctx: &RunContext,
    call: &str,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, lingxi_service::toolgateway::GatewayRefusal> {
    let request = ToolRequest::from_effective_arguments(h.exec_target.as_str(), args, &budget())
        .expect("effective request");
    let call_id = ToolCallId::new(call.to_string());
    let prepared = h.gateway.prepare_from_request(
        ctx,
        CallerSurface::UserRun,
        "agent",
        user_operate(),
        &call_id,
        &request,
    )?;
    h.gateway
        .execute_prepared(ctx, &call_id, &prepared.handle)
        .await
}

/// The write_stdin twin of [`call_exec`].
async fn call_write_stdin(
    h: &IntegrityHarness,
    ctx: &RunContext,
    call: &str,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, lingxi_service::toolgateway::GatewayRefusal> {
    let request =
        ToolRequest::from_effective_arguments(h.write_stdin_target.as_str(), args, &budget())
            .expect("effective request");
    let call_id = ToolCallId::new(call.to_string());
    let prepared = h.gateway.prepare_from_request(
        ctx,
        CallerSurface::UserRun,
        "agent",
        user_operate(),
        &call_id,
        &request,
    )?;
    h.gateway
        .execute_prepared(ctx, &call_id, &prepared.handle)
        .await
}

fn text_of(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn truncated_of(result: &ToolExecutionResult) -> bool {
    match &result.outcome {
        ToolOutcome::Success { result } => result.truncated,
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn resource_refs_of(result: &ToolExecutionResult) -> Vec<lingxi_protocol::ResourceRef> {
    match &result.outcome {
        ToolOutcome::Success { result } => result.resource_refs.clone(),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn exited_code_of(result: &ToolExecutionResult) -> i64 {
    match &result.outcome {
        ToolOutcome::Success { result } => match result.status.as_deref() {
            Some(ToolRunStatus::Exited { code }) => *code,
            other => panic!("expected Exited, got {other:?}"),
        },
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

/// Produces a single-line stream of exactly `fill` filler bytes between
/// the two markers via a real bash pipeline.
fn single_line_cmd(fill: usize) -> String {
    format!("printf 'HEAD_MARK|'; head -c {fill} /dev/zero | tr '\\0' x; printf '|TAIL_MARK'")
}

async fn wait_until<T>(what: &str, deadline: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + deadline;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        if Instant::now() > end {
            panic!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

// ── R04-RR1-F05-C01: a lost window prefix can never read as complete ───────

/// Leg (a) — the review's exact counter-example shape: 150 020 bytes of
/// single-line output against the default 100 KiB rolling window, with a
/// LEGAL `max_output_bytes` of 200 000 (larger than the window). The
/// retained tail fits the return budget, but the true stream HEAD was
/// evicted: the result must never present this as complete/whole.
#[tokio::test]
async fn f05_c01_hidden_window_prefix_loss_is_flagged_not_hidden() {
    let h = integrity_harness();
    let ctx = kernel_ctx("sess_f05_c01", "run_f05_c01");
    let started = Instant::now();
    let result = call_exec(
        &h,
        &ctx,
        "call-f05c01",
        json!({"cmd": single_line_cmd(150_000), "max_output_bytes": 200_000}),
    )
    .await
    .expect("the command runs");
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "bounded: {:?}",
        started.elapsed()
    );
    assert_eq!(exited_code_of(&result), 0);
    let text = text_of(&result);
    let truncated = truncated_of(&result);
    // The head marker MUST be visible: either the true head was retained
    // (then it is there) or the loss is flagged. Whatever the retention
    // strategy, a result whose text lacks the true head may NOT claim
    // completeness.
    assert!(
        text.contains("HEAD_MARK"),
        "the true stream head must be visible or the loss flagged; truncated={truncated}; \
         text head: {:?}… text tail: …{:?}",
        &text[..text.len().min(120)],
        &text[text.len().saturating_sub(120)..],
    );
    assert!(text.contains("TAIL_MARK"), "the tail survives: {text}");
    if !truncated {
        // Not truncated == a completeness claim over the returned body:
        // then the FULL single line (all 150 020 bytes) must be present,
        // byte-exactly.
        let expected = format!("HEAD_MARK|{}|TAIL_MARK", "x".repeat(150_000));
        assert_eq!(
            text, expected,
            "truncated=false claims completeness — the exact full stream must be present"
        );
    } else {
        // Flagged as truncated: the eviction/omission facts must be
        // stated in the text (never a silent hole).
        assert!(
            text.contains("evicted") || text.contains("omitted"),
            "a flagged loss states its fact: {text}"
        );
    }
    // The spill file (the full-output claim) is real and complete.
    let refs = resource_refs_of(&result);
    assert_eq!(refs.len(), 1, "one spill ref: {refs:?}");
    let uri = refs[0].uri.as_deref().expect("file uri");
    let path = uri.strip_prefix("file://").expect("file scheme");
    let spill_len = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or_else(|_| panic!("spill exists at {path}"));
    assert_eq!(spill_len, 150_020, "the spill holds the full stream");
    integrity_teardown(&h).await;
}

/// Leg (b) — the adversarial CONTROL: the whole stream fits the in-memory
/// windows (80 KiB < 100 KiB) but EXCEEDS the return budget. Then
/// truncated=true is the RETURN-level fact (head+tail with an omission
/// marker) while NOTHING was evicted from memory — the two facts are
/// stated separately ("Showing first/last", no eviction notice), and the
/// spill file is complete.
#[tokio::test]
async fn f05_c01_control_small_stream_big_budget_distinguishes_facts() {
    let h = integrity_harness();
    let ctx = kernel_ctx("sess_f05_c01b", "run_f05_c01b");
    let result = call_exec(
        &h,
        &ctx,
        "call-f05c01b",
        json!({"cmd": single_line_cmd(80 * 1024), "max_output_bytes": 60 * 1024}),
    )
    .await
    .expect("the command runs");
    assert_eq!(exited_code_of(&result), 0);
    let text = text_of(&result);
    assert!(truncated_of(&result), "return-level truncation is flagged");
    assert!(
        text.contains("HEAD_MARK|") && text.contains("|TAIL_MARK"),
        "both true ends visible in the truncated view"
    );
    assert!(
        text.contains("Showing first") && text.contains("lines"),
        "the incumbent's head/tail notice: {text}"
    );
    assert!(
        !text.contains("evicted"),
        "no in-memory eviction happened — none may be claimed: {text}"
    );
    // The spill holds the FULL stream (it was never capped).
    let refs = resource_refs_of(&result);
    assert_eq!(refs.len(), 1, "one spill ref: {refs:?}");
    assert_eq!(
        refs[0].display_name.as_deref(),
        Some("exec_command full output"),
        "an uncapped, complete spill may be called full: {refs:?}"
    );
    let path = refs[0]
        .uri
        .as_deref()
        .unwrap()
        .strip_prefix("file://")
        .unwrap();
    assert_eq!(
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX),
        80 * 1024 + 20,
        "spill file holds the whole stream"
    );
    integrity_teardown(&h).await;
}

/// Leg (c) — bigger than BOTH windows (300 020 bytes > 2 × 100 KiB): the
/// middle is evicted from memory, the TRUE head and tail are retained,
/// the eviction is stated with its exact byte count, and the completeness
/// flag is true no matter how large the caller's budget is.
#[tokio::test]
async fn f05_c01_big_stream_marks_eviction_and_keeps_true_head_and_tail() {
    let h = integrity_harness();
    let ctx = kernel_ctx("sess_f05_c01c", "run_f05_c01c");
    let result = call_exec(
        &h,
        &ctx,
        "call-f05c01c",
        json!({"cmd": single_line_cmd(300_000), "max_output_bytes": 200_000}),
    )
    .await
    .expect("the command runs");
    assert_eq!(exited_code_of(&result), 0);
    let text = text_of(&result);
    assert!(truncated_of(&result), "eviction marks truncated");
    assert!(
        text.starts_with("HEAD_MARK|"),
        "the shown beginning IS the stream's true head: {}…",
        &text[..40]
    );
    assert!(
        text.contains("|TAIL_MARK"),
        "the true tail survives: …{}",
        &text[text.len().saturating_sub(40)..]
    );
    let evicted = 300_020u64 - 2 * 102_400;
    assert!(
        text.contains(&format!(
            "{evicted} bytes of the output middle were evicted"
        )),
        "the exact eviction fact is stated: {text}"
    );
    // The spill is complete and may carry the full-output name.
    let refs = resource_refs_of(&result);
    assert_eq!(refs.len(), 1);
    assert_eq!(
        refs[0].display_name.as_deref(),
        Some("exec_command full output")
    );
    let path = refs[0]
        .uri
        .as_deref()
        .unwrap()
        .strip_prefix("file://")
        .unwrap();
    assert_eq!(
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX),
        300_020,
        "the full stream is on disk"
    );
    integrity_teardown(&h).await;
}

// ── R04-RR1-F05-C02: a single huge line keeps REAL head/tail content ───────

/// The integration leg: a single multibyte-only line over the DEFAULT
/// return budget (50 KiB), with key markers at both ends and the newline
/// only at the very end — byte-boundary-safe head/tail content survives.
#[tokio::test]
async fn f05_c02_single_multibyte_line_keeps_head_and_tail_content() {
    let h = integrity_harness();
    let ctx = kernel_ctx("sess_f05_c02", "run_f05_c02");
    let result = call_exec(
        &h,
        &ctx,
        "call-f05c02",
        json!({
            "cmd": "printf '头标记|'; printf '中%.0s' {1..20000}; printf '|尾标记\\n'"
        }),
    )
    .await
    .expect("the command runs");
    assert_eq!(exited_code_of(&result), 0);
    let text = text_of(&result);
    assert!(truncated_of(&result));
    assert!(
        text.contains("头标记|"),
        "the head marker content survives: {}…",
        &text[..60.min(text.len())]
    );
    assert!(
        text.contains("|尾标记"),
        "the tail marker content survives: …{}",
        &text[text.len().saturating_sub(60)..]
    );
    assert!(
        text.contains("omitted"),
        "the omitted middle is marked: {text}"
    );
    assert!(
        !text.contains('\u{FFFD}'),
        "cuts land on UTF-8 character boundaries: {}…",
        &text[..80.min(text.len())]
    );
    integrity_teardown(&h).await;
}

// ── R04-RR1-F05-C03: a capped spill is never "Full output" ─────────────────

#[tokio::test]
async fn f05_c03_capped_spill_is_partial_and_honest() {
    let mut limits = SupervisorLimits::with_spill_dir(test_dir("c03-spill"));
    limits.spill_cap_bytes = 64 * 1024;
    let h = integrity_harness_with(limits);
    let ctx = kernel_ctx("sess_f05_c03", "run_f05_c03");
    // The BEYOND_CAP marker exists only past the 64 KiB spill cap — the
    // capped file can never contain it.
    let cmd = "printf 'HEAD_MARK|'; head -c 130000 /dev/zero | tr '\\0' x; printf '|BEYOND_CAP'";
    let result = call_exec(&h, &ctx, "call-f05c03", json!({"cmd": cmd})).await;
    let result = result.expect("the command runs");
    assert_eq!(exited_code_of(&result), 0);
    let text = text_of(&result);
    // The memory result still shows both ends.
    assert!(
        text.contains("HEAD_MARK|") && text.contains("|BEYOND_CAP"),
        "{text}"
    );
    assert!(truncated_of(&result));
    // The spill file: bounded at its cap, PARTIAL — never "full".
    let refs = resource_refs_of(&result);
    assert_eq!(
        refs.len(),
        1,
        "the capped spill is still referenced: {refs:?}"
    );
    let display = refs[0].display_name.as_deref().unwrap();
    assert!(
        !display.contains("exec_command full output"),
        "a capped spill never claims full: {display}"
    );
    assert!(
        display.contains("partial"),
        "partial is explicit: {display}"
    );
    let path = refs[0]
        .uri
        .as_deref()
        .unwrap()
        .strip_prefix("file://")
        .unwrap()
        .to_string();
    assert_eq!(
        std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(u64::MAX),
        64 * 1024,
        "the spill stops exactly at its cap"
    );
    let spill_bytes = std::fs::read(&path).expect("spill readable");
    assert!(
        spill_bytes.windows(10).any(|w| w == b"HEAD_MARK|"),
        "the capped file holds the stream's beginning"
    );
    assert!(
        !spill_bytes.windows(11).any(|w| w == b"BEYOND_CAP"),
        "the beyond-cap marker is NOT in the capped file (it exists only in memory)"
    );
    // The body notice says the full output is unavailable, not "at path".
    assert!(
        text.contains("Full output: unavailable"),
        "the availability claim is explicit: {text}"
    );
    assert!(text.contains("capped at"), "the cap is stated: {text}");
    integrity_teardown(&h).await;
}

// ── R04-RR1-F05-C05: memory and resources stay bounded ─────────────────────

fn open_fd_count() -> usize {
    std::fs::read_dir("/dev/fd").map(|d| d.count()).unwrap_or(0)
}

/// One PTY session producing a large transcript, polled many times, plus
/// one-shots with small and large return budgets and two concurrent
/// streams: after bounded pressure and cleanup the FD count returns to
/// its baseline, every retained window is at its cap, and the spill
/// never exceeds its bound. The fix does NOT keep the full stream in
/// memory to achieve honesty.
#[tokio::test]
async fn f05_c05_high_output_and_many_pty_polls_stay_bounded() {
    let spill_dir = test_dir("c05-spill");
    let mut limits = SupervisorLimits::with_spill_dir(spill_dir.clone());
    limits.spill_cap_bytes = 256 * 1024;
    let h = integrity_harness_with(limits);
    let ctx = kernel_ctx("sess_f05_c05", "run_f05_c05");
    let fd_baseline = open_fd_count();

    // 1) A PTY terminal producing ~170 KiB of ticks with many polls.
    let started = call_exec(
        &h,
        &ctx,
        "call-c05-pty",
        json!({
            "argv": [
                "/bin/bash", "--noprofile", "--norc", "-c",
                "stty -echo; for ((i=0;i<30000;i++)); do printf 'tick %d\\n' $i; done; printf 'PTY_END\\n'"
            ],
            "tty": true
        }),
    )
    .await
    .expect("terminal starts");
    let handle = match &started.outcome {
        ToolOutcome::Success { result } => match result.status.as_deref() {
            Some(ToolRunStatus::Running { handle }) => handle.clone(),
            other => panic!("running handle: {other:?}"),
        },
        other => panic!("expected a success outcome, got {other:?}"),
    };
    let mut pty_seen = String::new();
    let parsed = ProcessHandleId::parse(&handle).unwrap();
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let poll = call_write_stdin(&h, &ctx, "call-c05-poll", json!({"process_id": handle}))
            .await
            .expect("pty poll");
        let text = text_of(&poll);
        if let Some((_, output)) = text.clone().split_once("\noutput:\n") {
            let content: String = output
                .split('\n')
                .take_while(|line| {
                    !(line.starts_with('[') && line.contains("dropped by the bounded ring"))
                        && !line.starts_with("transcript_path:")
                })
                .collect::<Vec<_>>()
                .join("\n");
            pty_seen.push_str(&content);
        }
        if pty_seen.contains("PTY_END") {
            break;
        }
        assert!(
            Instant::now() < end,
            "pty output bounded wait; seen tail: …{}",
            &pty_seen[pty_seen.len().saturating_sub(200)..]
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // The transcript ring bounded each delivery; the conversation saw
    // both the beginning and the end (they arrived, whether held in the
    // ring or re-read via poll cadence).
    assert!(pty_seen.contains("tick 0"), "early output observed");
    assert!(pty_seen.contains("PTY_END"), "late output observed");
    // Wait for the terminal's natural exit, then final poll + shutdown.
    wait_until("pty exit", Duration::from_secs(20), || {
        h.supervisor
            .phase_of(&parsed)
            .filter(|phase| phase.is_terminal())
    })
    .await;
    let _ = call_write_stdin(&h, &ctx, "call-c05-final", json!({"process_id": handle}))
        .await
        .expect("final poll");
    let receipts = h.supervisor.shutdown_all().await;
    assert!(receipts.len() <= 1);

    // 2) One-shots with a tiny and a huge return budget + concurrent
    //    streams (the bounded windows are per-record, both complete).
    let small = call_exec(
        &h,
        &ctx,
        "call-c05-small",
        json!({"cmd": "seq 1 20000", "max_output_bytes": 1000}),
    )
    .await
    .expect("small-budget command");
    assert!(truncated_of(&small), "a 1000-byte budget truncates");
    assert_eq!(exited_code_of(&small), 0);
    let big = call_exec(
        &h,
        &ctx,
        "call-c05-big",
        json!({"cmd": "seq 1 40000; seq 1 40000 >&2", "max_output_bytes": 200_000}),
    )
    .await
    .expect("big-budget concurrent streams");
    assert_eq!(exited_code_of(&big), 0);
    // Every settled record's windows are at their bounds and the spill
    // directory never exceeds the cap per file.
    for id in h.supervisor.retained_record_ids() {
        if let Some(snapshot) = h.supervisor.output_snapshot(&id) {
            assert!(snapshot.window.len() <= 102_400, "tail window bounded");
            assert!(snapshot.head.len() <= 102_400, "head window bounded");
            assert!(
                snapshot.total_bytes
                    <= snapshot.head.len() as u64
                        + snapshot.window.len() as u64
                        + snapshot.lost_middle_bytes(),
                "accounting identity per record"
            );
            if let Some(spill) = snapshot.spill {
                assert!(
                    spill.bytes_written <= 256 * 1024,
                    "spill bounded by its cap"
                );
            }
        }
    }
    for entry in std::fs::read_dir(&spill_dir).expect("spill dir readable") {
        let entry = entry.expect("entry");
        assert!(
            entry.metadata().map(|m| m.len()).unwrap_or(0) <= 256 * 1024,
            "each spill file is bounded"
        );
    }
    assert!(
        h.supervisor.live_handles().is_empty(),
        "nothing stays live after the bounded pressure"
    );
    // FD reclamation: back at the baseline once everything settled.
    wait_until("fd count back to baseline", Duration::from_secs(10), || {
        let now = open_fd_count();
        (now <= fd_baseline + 2).then_some(now)
    })
    .await;
    integrity_teardown(&h).await;
}

/// Cancel-mid-output leg: a long output storm is terminated mid-flight —
/// the spill stops growing (closed), the record turns terminal, and the
/// retained windows stay bounded.
#[tokio::test]
async fn f05_c05_cancelling_mid_output_closes_the_spill_and_bounds_everything() {
    let spill_dir = test_dir("c05b-spill");
    let mut limits = SupervisorLimits::with_spill_dir(spill_dir.clone());
    limits.spill_cap_bytes = 512 * 1024;
    let h = integrity_harness_with(limits);
    let ctx = kernel_ctx("sess_f05_c05b", "run_f05_c05b");
    let mut storm = Box::pin(call_exec(
        &h,
        &ctx,
        "call-c05b-storm",
        json!({"cmd": "printf 'lingxi-storm-line\\n'; sleep 30", "timeout_seconds": 60}),
    ));
    // Drive the call future while probing for the live record (a pinned
    // future only progresses when polled).
    let handle = loop {
        tokio::select! {
            settled = &mut storm => {
                panic!("the one-shot settled before the probe: {:?}", settled.map(|_| ()))
            }
            _ = tokio::time::sleep(Duration::from_millis(15)) => {
                if let Some(id) = h.supervisor.live_handles().first().cloned() {
                    break id;
                }
            }
        }
    };
    // Let some output flow (bounded wait for the spill to start).
    wait_until("spill starts", Duration::from_secs(10), || {
        std::fs::read_dir(&spill_dir)
            .ok()
            .and_then(|mut entries| entries.next())
            .map(|_| ())
    })
    .await;
    let receipt = h
        .supervisor
        .terminate(
            &handle,
            lingxi_service::procsupervisor::TerminationReason::Close,
        )
        .await;
    assert!(matches!(
        receipt.outcome,
        lingxi_service::procsupervisor::TerminationOutcome::Terminated { .. }
    ));
    let settled = storm.await.expect("the terminated call settles");
    let text = text_of(&settled);
    assert!(
        text.contains("lingxi-storm-line") || text.contains("(no output)"),
        "{text}"
    );
    // The spill file is closed and stable.
    let mut sizes: Vec<u64> = Vec::new();
    for _ in 0..2 {
        let total: u64 = std::fs::read_dir(&spill_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.metadata().ok().map(|m| m.len()))
                    .sum()
            })
            .unwrap_or(0);
        sizes.push(total);
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    assert_eq!(sizes[0], sizes[1], "the spill stops growing once closed");
    assert!(
        sizes[0] <= 512 * 1024,
        "spill total stays under its cap: {sizes:?}"
    );
    assert!(h.supervisor.live_handles().is_empty());
    integrity_teardown(&h).await;
}
