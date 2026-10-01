//! REVIEWER-R04-RR1-G03-R01 independent counterexample (baseline-
//! compilable: only APIs present on BOTH da15c4bd9 and the candidate).
//!
//! Three externally observable invariants, each falsified by the
//! un-repaired baseline and held by the candidate:
//!
//! R1 (C01-equivalent): a one-shot whose timeout lands in the
//!     reaped-but-undrained window (a grandchild holds the pipe write
//!     ends; the kill is honestly SKIPPED — group ownership is no
//!     longer provable after the reap) must NOT return `Exited{137}`
//!     while the never-signalled grandchild is provably ALIVE.
//! R2 (terminal_facts / repeated-terminator upgrade): a terminator
//!     whose wait observes another terminator's `CleanupTimedOut`
//!     expiry must NOT receive a confirmed `Terminated{Signal(9)}`
//!     receipt — no exit observation exists at that instant.
//! R3 (write_stdin text-vs-structured): while a PTY record is in
//!     `CleanupTimedOut`, `write_stdin` must not answer a text of
//!     "cleanup_timed_out" with a structured `Exited{137}`.
//!
//! All state lives in unique temp subdirectories; every process
//! signalled or probed was created by this test (exact pid, never a
//! name-based sweep).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lingxi_kernel::ports::{ToolOutcome, ToolRunStatus};
use lingxi_kernel::RunContext;
use lingxi_protocol::ToolCallId;
use lingxi_service::exectools::ProcessTools;
use lingxi_service::inject::SystemClock;
use lingxi_service::procsupervisor::{
    ExitFact, ProcessHandleId, ProcessKind, ProcessOwner, ProcessSupervisor, RecordPhase,
    SpawnSpec, SupervisorLimits, TerminationOutcome, TerminationReason,
};
use lingxi_service::resourceaccess::ResourceAccess;
use serde_json::json;
use std::sync::Arc;

// Portable across the candidate's `Option<Box<ToolRunStatus>>` and the
// baseline's inline `Option<ToolRunStatus>` (the ONLY API delta this
// file touches).
trait StatusInspect {
    fn status_exit_code(&self) -> Option<i64>;
    fn status_running_handle(&self) -> Option<String>;
}
impl StatusInspect for ToolRunStatus {
    fn status_exit_code(&self) -> Option<i64> {
        match self {
            ToolRunStatus::Exited { code } => Some(*code),
            _ => None,
        }
    }
    fn status_running_handle(&self) -> Option<String> {
        match self {
            ToolRunStatus::Running { handle } => Some(handle.clone()),
            _ => None,
        }
    }
}
impl StatusInspect for Box<ToolRunStatus> {
    fn status_exit_code(&self) -> Option<i64> {
        (**self).status_exit_code()
    }
    fn status_running_handle(&self) -> Option<String> {
        (**self).status_running_handle()
    }
}

fn test_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-g03r1-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn limits_at(tag: &str) -> SupervisorLimits {
    SupervisorLimits::with_spill_dir(test_dir(tag))
}

fn supervisor_with(limits: SupervisorLimits) -> Arc<ProcessSupervisor> {
    Arc::new(ProcessSupervisor::new(Arc::new(SystemClock), limits).expect("supervisor"))
}

fn owner_of(call: &str) -> ProcessOwner {
    ProcessOwner {
        principal_kind: "local_user".to_string(),
        principal_subject: "principal_local".to_string(),
        session_id: "sess_g03r1".to_string(),
        run_id: "run_g03r1".to_string(),
        tool_call_id: ToolCallId::new(call.to_string()),
    }
}

fn oneshot_spec(call: &str, argv: Vec<String>, cwd: &std::path::Path) -> SpawnSpec {
    SpawnSpec {
        argv,
        cwd: cwd.to_path_buf(),
        env: BTreeMap::new(),
        owner: owner_of(call),
        kind: ProcessKind::OneShot,
        cols: 80,
        rows: 24,
    }
}

fn kernel_ctx() -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_g03r1".to_string()),
        run_id: lingxi_protocol::RunId::new("run_g03r1".to_string()),
        attempt: lingxi_protocol::AttemptId::new("run_g03r1#a1".to_string()),
        generation: 1,
    }
}

fn tools_over(supervisor: &Arc<ProcessSupervisor>, ws: &PathBuf) -> ProcessTools {
    ProcessTools::new(
        Arc::clone(supervisor),
        Arc::new(ResourceAccess::new(std::slice::from_ref(ws)).expect("access")),
        ws.clone(),
    )
}

fn text_of(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|b| match b {
                lingxi_protocol::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect(),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn exit_code_of(outcome: &ToolOutcome) -> Option<i64> {
    match outcome {
        ToolOutcome::Success { result } => result.status.as_ref().and_then(|s| s.status_exit_code()),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn process_alive(pid: i32) -> bool {
    // SAFETY: signal-0 liveness probe on a process this test created.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn kill_owned(pid: i32) {
    // SAFETY: SIGKILL a process this test created (exact pid).
    unsafe { libc::kill(pid, libc::SIGKILL); }
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
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn read_pid_file(pid_file: &std::path::Path) -> i32 {
    wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await
}

fn audit_has_exit_observed(snapshot: &lingxi_service::procsupervisor::ProcessSnapshot) -> bool {
    snapshot
        .audit
        .iter()
        .any(|e| e.kind == "exit_observed")
}

/// R1: the timeout's CleanupTimedOut must not surface as Exited(137)
/// while the never-signalled grandchild is alive.
#[tokio::test]
async fn r1_exec_timeout_in_drain_window_never_claims_exited_137() {
    let ws = test_dir("r1-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("r1");
    limits.cleanup_timeout = Duration::from_millis(1);
    limits.stdio_grace = Duration::from_millis(1500);
    let supervisor = supervisor_with(limits);
    let tools = tools_over(&supervisor, &ws);
    let ctx = kernel_ctx();
    let call = ToolCallId::new("call_g03_r1".to_string());
    // The direct child exits at ~0.5s; the backgrounded `sleep 300`
    // holds the pipe write ends through the whole 1.5s grace; the
    // timeout at 1s is INSIDE that window (>=0.5s from both edges).
    let outcome = tools
        .run_exec_command(
            &ctx,
            &call,
            &json!({
                "cmd": format!(
                    "( sleep 300 ) & printf '%s\\n' \"$!\" > {}; echo R1_MARKER; sleep 0.5",
                    pid_file.display()
                ),
                "timeout_seconds": 1
            }),
        )
        .await;
    let grandchild = read_pid_file(&pid_file).await;
    // The grandchild was never signalled (ownership unprovable after the
    // reap — the G02 gate). It is ALIVE. Any Exited claim — and on the
    // baseline specifically Exited(137), the fabricated SIGKILL — is
    // contradicted by the real OS state at this instant.
    assert!(
        process_alive(grandchild),
        "precondition: the never-signalled holder is alive"
    );
    if exit_code_of(&outcome) == Some(137) {
        panic!(
            "R1 VIOLATED: the tool result claims Exited(137) while the never-signalled \
             grandchild pid {grandchild} is alive — the exit was never observed. \
             Text: {}",
            text_of(&outcome)
        );
    }
    if exit_code_of(&outcome) == Some(-1) {
        panic!(
            "R1 VIOLATED: the tool result claims Exited(-1) — a fabricated code. Text: {}",
            text_of(&outcome)
        );
    }
    kill_owned(grandchild);
    let _ = std::fs::remove_dir_all(&ws);
}

/// R2: a second terminator whose wait observes the first terminator's
/// CleanupTimedOut expiry must not receive a confirmed Terminated{
/// Signal(9)} receipt.
#[tokio::test]
async fn r2_concurrent_terminator_never_receives_fabricated_terminated() {
    let ws = test_dir("r2-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("r2");
    limits.cleanup_timeout = Duration::from_millis(300);
    limits.stdio_grace = Duration::from_millis(2000);
    let supervisor = supervisor_with(limits);
    let spawned = supervisor
        .spawn(oneshot_spec(
            "r2",
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!(
                    "( sleep 300 ) & printf '%s\\n' \"$!\" > {}; echo R2_MARKER; sleep 0.5",
                    pid_file.display()
                ),
            ],
            &ws,
        ))
        .await
        .expect("spawn");
    // The reaped-but-undrained window (child exited naturally; the
    // grandchild holds the write ends through the 2s grace).
    wait_until("drain window", Duration::from_secs(5), || {
        let snap = supervisor.record(&spawned.id)?;
        (snap.child_reaped && !snap.phase.is_terminal()).then_some(snap)
    })
    .await;
    // T1 starts now (deadline now+300ms); T2 starts 50ms later and is
    // parked on the phase watch when T1's expiry broadcast lands — well
    // before T2's own deadline. The real observation is >=1s away.
    let t1 = {
        let supervisor = Arc::clone(&supervisor);
        let id = spawned.id.clone();
        tokio::spawn(async move {
            supervisor
                .terminate(&id, TerminationReason::Timeout)
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let t2 = {
        let supervisor = Arc::clone(&supervisor);
        let id = spawned.id.clone();
        tokio::spawn(async move {
            supervisor.terminate(&id, TerminationReason::Close).await
        })
    };
    let r1 = t1.await.expect("t1");
    let r2 = t2.await.expect("t2");
    for (label, receipt) in [("t1", &r1), ("t2", &r2)] {
        if let TerminationOutcome::Terminated { fact, .. } = &receipt.outcome {
            let snapshot = supervisor
                .record(&spawned.id)
                .expect("record retained while checking");
            let observed = audit_has_exit_observed(&snapshot);
            let phase_terminal_with_fact = matches!(
                snapshot.phase,
                RecordPhase::Exited { .. } | RecordPhase::Terminated { .. }
            );
            if matches!(fact, ExitFact::Signal(9)) && (!observed || !phase_terminal_with_fact) {
                panic!(
                    "R2 VIOLATED ({label}): the receipt claims a confirmed kill \
                     Terminated{{Signal(9)}} without an observed exit (record phase {:?}, \
                     exit_observed in audit: {observed})",
                    snapshot.phase
                );
            }
        }
    }
    // Cleanup: let the deferred observation land, then dispose of the
    // grandchild by exact pid.
    wait_until("deferred observation", Duration::from_secs(5), || {
        let snap = supervisor.record(&spawned.id)?;
        snap.phase.is_terminal().then_some(snap)
    })
    .await;
    let grandchild = read_pid_file(&pid_file).await;
    kill_owned(grandchild);
    let _ = std::fs::remove_dir_all(&ws);
}

/// R3: write_stdin on a PTY in CleanupTimedOut must not answer the
/// "cleanup_timed_out" text with a structured Exited(137).
///
/// Construction note (verified twice on this OS): macOS closes every
/// deterministic PTY unconfirmed window — the master gets EOF at
/// session-leader exit even with a slave-holding grandchild, and a
/// close-kill observation lands within the first poll's prologue. The
/// window below is therefore best-effort: a real killpg fires, the nanos
/// cleanup bound expires first (receipt CleanupTimedOut), and a tight
/// phase-probe calls write_stdin the moment the record reads
/// CleanupTimedOut. When the window opens, the assertion is strict.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r3_write_stdin_text_and_status_agree_during_cleanup_timed_out() {
    let ws = test_dir("r3-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let ctx = kernel_ctx();
    let call = ToolCallId::new("call_g03_r3".to_string());
    let mut unconfirmed_closes = 0u32;
    let mut window_opens = 0u32;
    for round in 0..20u32 {
        let mut limits = limits_at(&format!("r3-{round}"));
        limits.cleanup_timeout = Duration::from_nanos(1);
        limits.stdio_grace = Duration::from_millis(200);
        let supervisor = Arc::new(
            ProcessSupervisor::new(Arc::new(SystemClock), limits).expect("supervisor"),
        );
        let tools = tools_over(&supervisor, &ws);
        let started = tools
            .run_exec_command(
                &ctx,
                &call,
                &json!({"cmd": "echo R3_MARKER; sleep 300", "tty": true}),
            )
            .await;
        let handle = match &started {
            ToolOutcome::Success { result } => result
                .status
                .as_ref()
                .and_then(|s| s.status_running_handle())
                .expect("pty start must be Running"),
            other => panic!("pty start: {other:?}"),
        };
        let parsed = ProcessHandleId::parse(&handle).expect("minted handle");
        let receipt = supervisor
            .terminate(&parsed, TerminationReason::Close)
            .await;
        if !matches!(receipt.outcome, TerminationOutcome::CleanupTimedOut) {
            continue; // the observation won this round — no window to probe
        }
        unconfirmed_closes += 1;
        // Tight probe: the instant the record reads CleanupTimedOut, poll.
        let end = Instant::now() + Duration::from_millis(100);
        while Instant::now() < end {
            match supervisor.phase_of(&parsed) {
                Some(RecordPhase::CleanupTimedOut { .. }) => {
                    window_opens += 1;
                    let poll = tools
                        .run_write_stdin(&ctx, &json!({"process_id": handle}))
                        .await;
                    let text = text_of(&poll);
                    if text.contains("cleanup_timed_out")
                        && exit_code_of(&poll) == Some(137)
                    {
                        panic!(
                            "R3 VIOLATED: the poll text says cleanup_timed_out (exit NOT \
                             observed) yet the structured status claims Exited(137). Text: {text}"
                        );
                    }
                }
                Some(other) if other.is_terminal() => break,
                _ => {}
            }
        }
    }
    eprintln!(
        "R3 unconfirmed_closes={unconfirmed_closes} window_opens={window_opens} of 20 rounds \
         (macOS closes the deterministic PTY window; see the construction note)"
    );
    let _ = std::fs::remove_dir_all(&ws);
}