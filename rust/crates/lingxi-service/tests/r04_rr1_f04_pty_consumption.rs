//! R04-RR1-F04 acceptance (C04): a REAL PTY terminal whose program writes
//! MIXED bytes (an ASCII lead and half a multibyte character in ONE
//! write) split into two stages by a HANDSHAKE — the second half is
//! written only after the test confirmed the first stage's readiness and
//! sent its acknowledgement. No sleep-based greening: every wait is a
//! bounded poll-until-marker over the REAL tool chain.
//!
//! The chain under test is the full REAL one: the REAL
//! `ToolInvocationGateway` (prepare → execute_prepared), the REAL
//! `ProcessTools::run_exec_command` / `run_write_stdin` executors behind
//! `register_process_tools`, the REAL `ProcessSupervisor` with a REAL
//! posix_openpt pair and a REAL `/bin/bash` child. The transcript
//! consumption model (byte-exact, hold-back-not-loss) is the code under
//! verification; nothing is mocked.
//!
//! Test-double boundary: no provider doubles — calls go through the
//! gateway's user-run surface directly. Echo is disabled by the program
//! itself (`stty -echo`) so the only transcript content is what the
//! program actually wrote. All state lives in unique temp dirs.

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

struct PtyHarness {
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
}

fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r04rr1f04-{tag}-{}-{}-{}",
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

fn pty_harness() -> PtyHarness {
    let root = test_dir("root");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("workspace");
    let spill_dir = test_dir("spill");
    std::fs::create_dir_all(&spill_dir).expect("spill dir");
    let limits = SupervisorLimits::with_spill_dir(spill_dir);
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
    let supervisor =
        Arc::new(ProcessSupervisor::new(Arc::new(SystemClock), limits).expect("supervisor"));
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
    PtyHarness {
        registry,
        gateway,
        supervisor,
        exec_target: core.exec_target,
        write_stdin_target: core.write_stdin_target,
        root,
        ws,
    }
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

async fn call(
    h: &PtyHarness,
    ctx: &RunContext,
    call: &str,
    target: &ToolTargetId,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, lingxi_service::toolgateway::GatewayRefusal> {
    let request = ToolRequest::from_effective_arguments(target.as_str(), args, &budget())
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

fn full_text(result: &ToolExecutionResult) -> String {
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

fn error_text(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Failed { error } => error.message.clone(),
        other => panic!("expected a failed outcome, got {other:?}"),
    }
}

/// Extracts the transcript CONTENT of one write_stdin result (the text
/// after the `output:` header, up to the first supervisor notice line)
/// plus its structured `truncated` flag.
fn transcript_of(result: &ToolExecutionResult) -> (String, bool) {
    let text = full_text(result);
    let truncated = match &result.outcome {
        ToolOutcome::Success { result } => result.truncated,
        other => panic!("expected a success outcome, got {other:?}"),
    };
    let output = text
        .strip_prefix("process_id: ")
        .and_then(|rest| rest.split_once("\noutput:\n"))
        .map(|(_, output)| output.to_string())
        .unwrap_or_default();
    // The notice lines the executor may append after the content.
    let is_notice = |line: &str| {
        line.starts_with("transcript_path:")
            || (line.starts_with('[')
                && (line.contains("held back until the rest arrives")
                    || line.contains("dropped by the bounded ring")
                    || line.contains("closed mid-character")))
    };
    let mut content = String::new();
    for line in output.split('\n') {
        if is_notice(line) {
            break;
        }
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(line);
    }
    (content, truncated)
}

/// Polls (empty chars = pure poll) until the accumulated NEW transcript
/// content contains `needle`; returns (all_new_content, truncated_flags).
async fn poll_until_content(
    h: &PtyHarness,
    ctx: &RunContext,
    handle: &str,
    needle: &str,
    timeout: Duration,
) -> (String, Vec<bool>) {
    let end = Instant::now() + timeout;
    let mut seen = String::new();
    let mut flags = Vec::new();
    loop {
        let poll = call(
            h,
            ctx,
            "pty-poll",
            &h.write_stdin_target,
            json!({"process_id": handle}),
        )
        .await
        .expect("pty poll");
        let (content, truncated) = transcript_of(&poll);
        seen.push_str(&content);
        flags.push(truncated);
        if seen.contains(needle) {
            return (seen, flags);
        }
        if Instant::now() > end {
            panic!("timed out waiting for {needle:?}; transcript so far:\n{seen}");
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

// ── R04-RR1-F04-C04: real PTY, handshake-driven two-stage mixed write ──────

#[tokio::test]
async fn f04_c04_real_pty_handshake_delivers_mixed_bytes_exactly_once() {
    let h = pty_harness();
    let ctx = kernel_ctx("sess_f04_c04", "run_f04_c04");
    // The program: disables echo (the only transcript content is what IT
    // writes), announces PHASE1_READY, then prints 'M1a' + the first TWO
    // bytes of "日" (E6 97) as ONE printf (one write(2) → one pty read
    // chunk — the MIXED-chunk shape), then BLOCKS on `read` — the
    // handshake. The partial bytes are the LAST bytes of the stage, so
    // they are a genuine trailing-incomplete sequence (held back). Only
    // after the test's acknowledgement does it write the completing byte
    // (A5) + TAIL_DONE and exit 7.
    let script = "stty -echo; printf 'PHASE1_READY\\n'; printf 'M1a\\xe6\\x97'; \
                  IFS= read -r line; printf '\\xa5TAIL_DONE\\n'; exit 7";
    let started = call(
        &h,
        &ctx,
        "call-pty-start",
        &h.exec_target,
        json!({
            "argv": ["/bin/bash", "--noprofile", "--norc", "-c", script],
            "tty": true,
            "cols": 80,
            "rows": 24
        }),
    )
    .await
    .expect("the pty terminal starts");
    let handle = match &started.outcome {
        ToolOutcome::Success { result } => match result.status.as_deref() {
            Some(ToolRunStatus::Running { handle }) => handle.clone(),
            other => panic!("expected a running handle, got {other:?}"),
        },
        other => panic!("expected a success outcome, got {other:?}"),
    };
    let parsed = ProcessHandleId::parse(&handle).expect("minted handle parses");
    let snapshot = h.supervisor.record(&parsed).expect("the record exists");
    assert_eq!(snapshot.kind.wire_name(), "persistent_terminal");

    // Stage 1: poll until the program's OWN readiness marker arrives.
    // The half-character must be HELD BACK (not delivered mangled, not
    // reported as dropped).
    let (phase1, phase1_flags) =
        poll_until_content(&h, &ctx, &handle, "PHASE1_READY", Duration::from_secs(15)).await;
    assert!(phase1.contains("M1a"), "the ASCII lead arrived: {phase1:?}");
    assert!(
        !phase1.contains('\u{FFFD}'),
        "no mangled delivery of the held half-character: {phase1:?}"
    );
    assert!(
        !phase1.contains('日'),
        "the incomplete character is not delivered early: {phase1:?}"
    );
    // Idle re-polls with no new data: nothing new, nothing repeated, no
    // false drop claims (waiting ≠ loss — R04-RR1-F04-C02 through the
    // REAL chain).
    for i in 0..3 {
        let poll = call(
            &h,
            &ctx,
            "pty-idle-poll",
            &h.write_stdin_target,
            json!({"process_id": handle}),
        )
        .await
        .expect("idle poll");
        let (content, truncated) = transcript_of(&poll);
        assert!(
            !content.contains('a') && !content.contains('M'),
            "idle poll {i} delivered nothing new: {content:?}"
        );
        // Structured truncated + the text notices must AGREE: no drop
        // message while the bytes are merely held back.
        if truncated {
            assert!(
                !full_text(&poll).contains("dropped by the bounded ring"),
                "held-back bytes are not reported as dropped: {}",
                full_text(&poll)
            );
        }
    }

    // The handshake: acknowledge, which releases the completing byte.
    let ack = call(
        &h,
        &ctx,
        "pty-ack",
        &h.write_stdin_target,
        json!({"process_id": handle, "chars": "\n"}),
    )
    .await
    .expect("ack write");
    let (ack_content, _) = transcript_of(&ack);
    let (mut all, flags2) = (
        format!("{phase1}{ack_content}"),
        poll_until_content(&h, &ctx, &handle, "TAIL_DONE", Duration::from_secs(15)).await,
    );
    all.push_str(&flags2.0);
    let mut all_truncated = phase1_flags;
    all_truncated.extend(flags2.1);

    // The terminal content, normalized (ONLCR): exactly ONE 'a', exactly
    // ONE 日 — the mixed chunk's prefix was never re-delivered.
    let normalized = all.replace("\r\n", "\n");
    assert_eq!(
        normalized.matches('日').count(),
        1,
        "the completed character arrives exactly once: {normalized:?}"
    );
    assert_eq!(
        normalized.matches("M1a").count(),
        1,
        "the mixed chunk's prefix is delivered exactly once: {normalized:?}"
    );
    assert!(
        normalized.contains("PHASE1_READY\nM1a"),
        "stage-1 content intact: {normalized:?}"
    );
    assert!(
        normalized.contains("日TAIL_DONE\n"),
        "stage-2 completes the character: {normalized:?}"
    );
    assert!(
        !normalized.contains("aa") && !normalized.contains("a日\nM1"),
        "no duplicated prefix anywhere: {normalized:?}"
    );
    // No drop message across the whole conversation unless real loss was
    // flagged — and none is (the ring is far from full).
    assert!(
        all_truncated.iter().all(|flag| !flag)
            || !normalized.contains("dropped by the bounded ring"),
        "structured flags: {all_truncated:?}, content: {normalized:?}"
    );

    // The persistent handle does not degrade: cross-session write_stdin
    // is still refused while the owning session keeps working.
    let foreign_ctx = kernel_ctx("sess_f04_c04_foreign", "run_f04_c04_foreign");
    let foreign = call(
        &h,
        &foreign_ctx,
        "pty-foreign",
        &h.write_stdin_target,
        json!({"process_id": handle, "chars": "echo INJECTED\n"}),
    )
    .await
    .expect("foreign call resolves");
    assert!(
        error_text(&foreign).contains("WRITE_STDIN_NOT_OWNED"),
        "cross-session refusal intact: {}",
        error_text(&foreign)
    );

    // The program exits 7 after TAIL_DONE; the final (forced) delivery
    // flushes anything left and reports the exit.
    let final_phase = {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            let phase = h.supervisor.phase_of(&parsed).expect("the record exists");
            if phase.is_terminal() {
                break phase;
            }
            if Instant::now() > end {
                panic!("terminal did not exit; last transcript: {normalized:?}");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    let _ = final_phase;
    let final_poll = call(
        &h,
        &ctx,
        "pty-final-poll",
        &h.write_stdin_target,
        json!({"process_id": handle}),
    )
    .await
    .expect("final poll");
    match &final_poll.outcome {
        ToolOutcome::Success { result } => match result.status.as_deref() {
            Some(ToolRunStatus::Exited { code }) => assert_eq!(*code, 7),
            other => panic!("final poll reports the exit: {other:?}"),
        },
        other => panic!("expected a success outcome, got {other:?}"),
    }
    let (final_content, _) = transcript_of(&final_poll);
    let final_all = format!("{normalized}{}", final_content.replace("\r\n", "\n"));
    assert_eq!(
        final_all.matches('日').count(),
        1,
        "still exactly one 日 after the forced final drain: {final_all:?}"
    );

    let _ = h.supervisor.shutdown_all().await;
    let _ = std::fs::remove_dir_all(test_dir("cleanup-marker"));
}
