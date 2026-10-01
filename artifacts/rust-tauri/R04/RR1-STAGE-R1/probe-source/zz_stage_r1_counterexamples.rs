//! STAGE-REVIEWER-R04-RR1 independent counterexamples (old-red / new-green).
//!
//! This file is written by the stage reviewer ONLY, is compiled into TWO
//! isolated /tmp git worktrees (773d5a696 "old" and 629e15b85 "candidate")
//! and never touches the main tree. It uses ONLY API surface common to
//! both trees so the same source is a RUNTIME red on the old tree and a
//! green on the candidate — the compile-level differences between the
//! trees are irrelevant to the assertions.
//!
//! Tests here re-derive, independently of any G01-G05 report:
//!   F01 — a registry-full refusal must not have executed the command;
//!   F02 — the settled collector must be frozen after the terminal phase;
//!   F03 — an unconfirmed cleanup stop must not surface as an exit;
//!   F05 — a lost head / capped spill must not be presented as complete.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lingxi_kernel::ports::{ToolExecutionResult, ToolOutcome, ToolRequest, ToolRunStatus};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetId};
use lingxi_kernel::RunContext;
use lingxi_protocol::ToolCallId;
use lingxi_service::approval_service::ApprovalService;
use lingxi_service::exectools::register_process_tools;
use lingxi_service::inject::SystemClock;
use lingxi_service::procsupervisor::{
    ProcessKind, ProcessOwner, ProcessSupervisor, SpawnSpec, SupervisorLimits,
    TerminationOutcome, TerminationReason,
};
use lingxi_service::resourceaccess::ResourceAccess;
use lingxi_service::toolgateway::{
    CallerSurface, InvocationPermissionContext, ToolInvocationGateway, ToolPolicyPort,
    DEFAULT_LIVE_PREPARED_CAP, DEFAULT_PREPARED_TTL_MS,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-stager1-{tag}-{}-{}-{}",
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

struct Harness {
    #[allow(dead_code)]
    registry: Arc<ToolRegistry>,
    gateway: Arc<ToolInvocationGateway>,
    supervisor: Arc<ProcessSupervisor>,
    exec_target: ToolTargetId,
    root: PathBuf,
    ws: PathBuf,
    /// Test-created holder processes to kill by EXACT pid at teardown.
    holders: std::sync::Mutex<Vec<i32>>,
}

impl Harness {
    fn register_holder(&self, pid: i32) {
        self.holders.lock().unwrap().push(pid);
    }
}

async fn teardown(h: &Harness) {
    for pid in h.holders.lock().unwrap().drain(..) {
        // SAFETY: a process this test spawned itself.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    let _ = h.supervisor.shutdown_all().await;
    let _ = std::fs::remove_dir_all(&h.root);
}

fn harness(limits: SupervisorLimits) -> Harness {
    let root = test_dir("root");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("workspace");
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
        None,
        Arc::new(SystemClock),
        &budget(),
    );
    Harness {
        registry,
        gateway,
        supervisor,
        exec_target: core.exec_target,
        root,
        ws,
        holders: std::sync::Mutex::new(Vec::new()),
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

async fn call_exec(
    h: &Harness,
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

/// Cross-tree shim: the candidate boxes `ToolSuccess::status`
/// (`Option<Box<ToolRunStatus>>`, G03) while the old tree uses
/// `Option<ToolRunStatus>` — one extension trait serves both.
trait StatusOf {
    fn status_of(&self) -> Option<ToolRunStatus>;
}
impl StatusOf for Option<ToolRunStatus> {
    fn status_of(&self) -> Option<ToolRunStatus> {
        self.clone()
    }
}
impl StatusOf for Option<Box<ToolRunStatus>> {
    fn status_of(&self) -> Option<ToolRunStatus> {
        self.as_deref().cloned()
    }
}

fn success_parts(
    result: &ToolExecutionResult,
) -> (String, bool, Vec<lingxi_protocol::ResourceRef>, Option<ToolRunStatus>) {
    match &result.outcome {
        ToolOutcome::Success { result } => {
            let text = result
                .content
                .iter()
                .filter_map(|block| match block {
                    lingxi_protocol::ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                text,
                result.truncated,
                result.resource_refs.clone(),
                result.status.status_of(),
            )
        }
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn error_text(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Failed { error } => error.message.clone(),
        other => panic!("expected a failed outcome, got {other:?}"),
    }
}

async fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    loop {
        match LOCK.try_lock() {
            Ok(guard) => return guard,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                return poisoned.into_inner();
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
}

fn sha256_of(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|_| panic!("read {}", path.display()));
    hex(&Sha256::digest(&bytes))
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

// ── F01: a registry-full refusal must not have executed the command ─────────

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn stage_r1_f01_refused_oneshot_must_not_execute() {
    let _serial = serial().await;
    let spill = test_dir("spill-f01");
    let mut limits = SupervisorLimits::with_spill_dir(spill);
    limits.live_cap = 1;
    let h = harness(limits);
    let ctx = kernel_ctx("stager1_f01", "run_f01");

    // Occupy the only live slot with a long-running one-shot.
    let mut first = Box::pin(call_exec(
        &h,
        &ctx,
        "f01-first",
        json!({"argv": ["/bin/sleep", "300"], "timeout_seconds": 600}),
    ));
    let first_id = loop {
        tokio::select! {
            settled = &mut first => {
                panic!("the first one-shot settled early: {:?}", settled.map(|_| ()))
            }
            _ = tokio::time::sleep(Duration::from_millis(15)) => {
                if let Some(id) = h.supervisor.live_handles().first().cloned() {
                    break id;
                }
            }
        }
    };
    assert_eq!(h.supervisor.live_handles().len(), 1);

    let sentinel = h.ws.join("f01-sentinel.txt");
    std::fs::write(&sentinel, b"BASELINE").expect("write sentinel");
    let before = sha256_of(&sentinel);

    let second = call_exec(
        &h,
        &ctx,
        "f01-second",
        json!({
            "argv": ["/bin/bash", "-c", format!("printf RAN > {}", sentinel.display())]
        }),
    )
    .await
    .expect("the second call resolves (as a refusal)");

    let err = error_text(&second);
    // The refusal must be the ProcessSupervisor capacity — NOT the gateway
    // prepared-registry vocabulary (two different capacities).
    assert!(
        err.contains("registry is full"),
        "expected the supervisor capacity vocabulary: {err}"
    );
    assert!(
        !err.contains("prepared-invocation registry"),
        "the gateway prepared cap is a different capacity and must not fire here: {err}"
    );

    // ZERO EXECUTION — this is the assertion that is RED on the old tree:
    // the dispatched-but-unregistered child overwrites the sentinel.
    let end = Instant::now() + Duration::from_millis(1000);
    while Instant::now() < end {
        assert_eq!(
            sha256_of(&sentinel),
            before,
            "STAGE-R1-F01: the refused one-shot must not have executed"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        std::fs::read(&sentinel).unwrap(),
        b"BASELINE",
        "the sentinel content is the durable zero-dispatch proof"
    );
    assert_eq!(h.supervisor.live_handles().len(), 1, "no leaked record");

    let receipt = h
        .supervisor
        .terminate(&first_id, TerminationReason::Close)
        .await;
    assert!(
        matches!(receipt.outcome, TerminationOutcome::Terminated { .. }),
        "first process terminates: {:?}",
        receipt.outcome
    );
    let _ = first.await.expect("first settles");
    teardown(&h).await;
}

// ── F02: the settled collector must be frozen after the terminal phase ──────

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn stage_r1_f02_settled_collector_frozen_after_terminal() {
    let _serial = serial().await;
    let spill = test_dir("spill-f02");
    let mut limits = SupervisorLimits::with_spill_dir(spill);
    limits.live_cap = 4;
    limits.stdio_grace = Duration::from_millis(250);
    let h = harness(limits);

    // The direct child exits immediately; a background grandchild inherits
    // the stdout pipe write end and writes LATE one second later (long
    // after the stdio grace).
    let script = "/usr/bin/python3 -c 'import sys,time; time.sleep(1.0); \
                  sys.stdout.write(\"LATE_A\\n\"); sys.stdout.flush(); time.sleep(30)' &\nexit 0\n";
    let spec = SpawnSpec {
        argv: vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()],
        cwd: h.ws.clone(),
        env: BTreeMap::new(),
        owner: ProcessOwner {
            principal_kind: "local".to_string(),
            principal_subject: "stager1".to_string(),
            session_id: "stager1_f02".to_string(),
            run_id: "run_f02".to_string(),
            tool_call_id: ToolCallId::new("stager1-f02".to_string()),
        },
        kind: ProcessKind::OneShot,
        cols: 80,
        rows: 24,
    };
    let spawned = h.supervisor.spawn(spec).await.expect("spawn");
    let _phase = h.supervisor.wait_terminal(&spawned.id).await;

    let s1 = h
        .supervisor
        .output_snapshot(&spawned.id)
        .expect("snapshot after terminal");
    // Leave a generous window past the grandchild's late write.
    tokio::time::sleep(Duration::from_millis(1600)).await;
    let s2 = h
        .supervisor
        .output_snapshot(&spawned.id)
        .expect("second snapshot");

    // RED on the old tree: the detached pump keeps appending to the
    // SETTLED collector (total_bytes grows) and the read ends stay open.
    assert_eq!(
        s1.total_bytes, s2.total_bytes,
        "STAGE-R1-F02: the settled collector changed after the terminal phase ({} -> {})",
        s1.total_bytes, s2.total_bytes
    );
    assert_eq!(
        s1.window, s2.window,
        "STAGE-R1-F02: the settled window changed after the terminal phase"
    );
    teardown(&h).await;
}

// ── F03: an unconfirmed cleanup stop must not surface as an exit ────────────

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn stage_r1_f03_unconfirmed_stop_must_not_claim_an_exit() {
    let _serial = serial().await;
    let spill = test_dir("spill-f03");
    let mut limits = SupervisorLimits::with_spill_dir(spill);
    limits.live_cap = 4;
    // killpg resolves fast, but the grandchild (which escaped the process
    // group via setsid) holds the pipe write ends, so the drain cannot
    // finish inside the cleanup bound — the stop is unconfirmed.
    limits.cleanup_timeout = Duration::from_millis(300);
    limits.stdio_grace = Duration::from_millis(1500);
    let h = harness(limits);
    let ctx = kernel_ctx("stager1_f03", "run_f03");

    let pidfile = h.ws.join("f03-holder.pid");
    let script = format!(
        "/usr/bin/python3 -c 'import os,time; os.setsid(); \
         open(\"{pid}\",\"w\").write(str(os.getpid())); time.sleep(60)' &\nexec /bin/sleep 60\n",
        pid = pidfile.display()
    );
    let result = call_exec(
        &h,
        &ctx,
        "f03-timeout",
        json!({
            "argv": ["/bin/sh", "-c", script],
            "timeout_seconds": 1
        }),
    )
    .await
    .expect("the call resolves");

    // Track the holder for precise teardown.
    if let Ok(text) = std::fs::read_to_string(&pidfile) {
        if let Ok(pid) = text.trim().parse::<i32>() {
            h.register_holder(pid);
        }
    }

    let (text, _truncated, _refs, status) = success_parts(&result);
    // RED on the old tree: the unconfirmed stop surfaces as
    // ToolRunStatus::Exited{code:137} — an exit that was never observed.
    if let Some(ToolRunStatus::Exited { code }) = status {
        panic!(
            "STAGE-R1-F03: the result claims an exit (code {code}) while the stop was \
             never observed as a trusted receipt; text tail: {}",
            text.chars().rev().take(200).collect::<String>().chars().rev().collect::<String>()
        );
    }
    assert!(
        !text.contains("exited with code"),
        "STAGE-R1-F03: the text claims an exit that was never observed: {text}"
    );
    teardown(&h).await;
}

// ── F05: a lost head / capped spill must not be presented as complete ───────

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn stage_r1_f05_lost_head_must_be_visible_and_flagged() {
    let _serial = serial().await;
    let spill = test_dir("spill-f05a");
    let limits = SupervisorLimits::with_spill_dir(spill);
    let h = harness(limits);
    let ctx = kernel_ctx("stager1_f05a", "run_f05a");

    // One single line of ~150 027 bytes (default rolling window 100 KiB),
    // return budget 200 000 — bigger than the whole stream.
    let script = "printf 'HEAD_MARK_9f3a'; /usr/bin/head -c 150000 /dev/zero | \
                  /usr/bin/tr '\\0' 'x'; printf 'TAIL_MARK_c17d\\n'";
    let result = call_exec(
        &h,
        &ctx,
        "f05-head",
        json!({
            "argv": ["/bin/sh", "-c", script],
            "max_output_bytes": 200000
        }),
    )
    .await
    .expect("the call resolves");
    let (text, truncated, _refs, _status) = success_parts(&result);

    // RED on the old tree: the head marker was evicted from the rolling
    // tail window yet truncated == false.
    assert!(
        text.contains("HEAD_MARK_9f3a"),
        "STAGE-R1-F05: the true stream head must be visible in the result"
    );
    assert!(
        text.contains("TAIL_MARK_c17d"),
        "STAGE-R1-F05: the true stream tail must be visible in the result"
    );
    if !truncated {
        assert!(
            text.contains(&"x".repeat(64)) && text.contains("HEAD_MARK_9f3a"),
            "STAGE-R1-F05: truncated == false requires the whole stream to be present"
        );
    }
    teardown(&h).await;
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn stage_r1_f05_capped_spill_must_not_be_claimed_full() {
    let _serial = serial().await;
    let spill = test_dir("spill-f05b");
    let mut limits = SupervisorLimits::with_spill_dir(spill);
    limits.spill_cap_bytes = 65536;
    let h = harness(limits);
    let ctx = kernel_ctx("stager1_f05b", "run_f05b");

    // 100 023 bytes on one line; the spill is capped at 64 KiB.
    let script = "printf 'CAP_HEAD_51c2'; /usr/bin/head -c 100000 /dev/zero | \
                  /usr/bin/tr '\\0' 'y'; printf 'CAP_TAIL_8d4f\\n'";
    let result = call_exec(
        &h,
        &ctx,
        "f05-cap",
        json!({
            "argv": ["/bin/sh", "-c", script],
            "max_output_bytes": 30000
        }),
    )
    .await
    .expect("the call resolves");
    let (text, _truncated, refs, _status) = success_parts(&result);
    let total: u64 = 100_023;

    // Every textual "Full output: <path>" claim that is NOT an explicit
    // "unavailable" must reference a file holding the WHOLE stream.
    let mut rest = text.as_str();
    while let Some(pos) = rest.find("Full output: ") {
        let after = &rest[pos + "Full output: ".len()..];
        let line = after.lines().next().unwrap_or("");
        let trimmed = line.trim_end_matches(']').trim();
        if !trimmed.starts_with("unavailable") {
            let meta = std::fs::metadata(trimmed)
                .unwrap_or_else(|_| panic!("STAGE-R1-F05: claimed full-output file missing: {trimmed}"));
            assert!(
                meta.len() >= total,
                "STAGE-R1-F05: a capped spill ({}) is claimed as the full output ({total} bytes): {trimmed}",
                meta.len()
            );
        }
        rest = after;
    }

    // Structured side: any resource ref PRESENTING itself as the full
    // output (the affirmative "exec_command full output" name — the
    // candidate's capped vocabulary explicitly negates it) must point at
    // a file that actually holds the whole stream.
    for r in &refs {
        let display = r.display_name.as_deref().unwrap_or("");
        if display.starts_with("exec_command full output") {
            let size = r
                .size_bytes
                .unwrap_or_else(|| panic!("STAGE-R1-F05: full-output ref without a size"));
            assert!(
                size >= total,
                "STAGE-R1-F05: ref claims full output but only holds {size} of {total} bytes"
            );
        }
    }
    teardown(&h).await;
}
