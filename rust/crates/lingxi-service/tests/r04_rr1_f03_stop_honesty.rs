//! R04-RR1-F03 acceptance: an UNCONFIRMED cleanup (termination
//! requested, exit NOT observed within the bound) must never be
//! converted into an observed exit — no fabricated `ExitFact::Signal
//! (9)`, no `ToolRunStatus::Exited { code: 137 }`, no confirmed
//! `Terminated` receipt, no `Exited(-1)` for a failed wait. 137 is the
//! dedicated fact of an OBSERVED SIGKILL; stating it without the
//! observation is the same lie family as "request sent ⇒ file
//! generated" (R04-T08 §七) and violates the R03 Unknown rule on the
//! process face.
//!
//! Everything here runs against the REAL `ProcessSupervisor` and the
//! REAL tool executors (`ProcessTools::run_exec_command` /
//! `run_write_stdin`) with REAL OS processes.
//!
//! The deterministic unconfirmed window: the direct child exits
//! NATURALLY (the reaper publishes `child_reaped`), while a
//! backgrounded grandchild holds the pipe/pty write ends — the reaper
//! is inside its bounded stdio grace and the record is NOT terminal
//! yet (the G02 "reaped-but-undrained" window). A termination arriving
//! in that window cannot signal anything (ownership unprovable after
//! the reap — audited `group_signal_skipped`), and with
//! `cleanup_timeout` ≈ 0 the bounded wait expires long before the
//! grace can elapse: the exit observation is DETERMINISTICALLY still
//! pending, with ≥ 0.5 s of margin on each side (no dangerous process
//! is manufactured to survive a real SIGKILL; the grandchild is a
//! plain `cmd &` that is never signalled and is disposed of by exact
//! pid at teardown).
//!
//! Test-double boundary: NONE — the supervisor and the tool executors
//! under test are the product types. All test state lives in unique
//! `std::env::temp_dir()` subdirectories; every process referenced by
//! pid was created by this test and is disposed of by exact pid.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use std::collections::VecDeque;
use std::pin::Pin;

use lingxi_kernel::ports::{
    InvocationJournalEntry, InvocationPhase, ProviderDescriptor, ProviderTurn, ProviderTurnResult,
    StoragePort, ToolOutcome, ToolRequest, ToolRunStatus, TurnProviderPort,
};
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, ToolCallId};
use lingxi_service::approval_service::ApprovalService;
use lingxi_service::exectools::{register_process_tools, ProcessTools};
use lingxi_service::inject::SystemClock;
use lingxi_service::procsupervisor::{
    ExitFact, ProcessHandleId, ProcessKind, ProcessOwner, ProcessSupervisor, ReapFaultPoint,
    RecordPhase, SpawnFailure, SpawnSpec, SupervisorLimits, TerminationOutcome, TerminationReason,
};
use lingxi_service::resourceaccess::ResourceAccess;
use lingxi_service::toolgateway::ToolInvocationGateway;
use lingxi_service::{
    approval, prepare_layout, ServiceConfig, ServiceDeps, ServiceState, LOCAL_OWNER_USER_ID,
};
use lingxi_service::{HomeSource, NetworkMode};
use serde_json::json;

/// The stdio grace that keeps the reaper's drain window open (the
/// observation-delaying boundary).
const WINDOW_GRACE: Duration = Duration::from_millis(1500);
/// How long the direct child lives before its natural exit opens the
/// window (the window then spans ~[child_exit, child_exit + grace]).
const CHILD_LIFE: &str = "0.5";

// ── harness ─────────────────────────────────────────────────────────────────

/// A unique test directory (pid + nanos + sequence — no cross-test
/// collisions, never the shared /tmp itself).
fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r04rr1f03-{tag}-{}-{}-{}",
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

fn limits_at(tag: &str) -> SupervisorLimits {
    SupervisorLimits::with_spill_dir(test_dir(tag))
}

/// The controlled-boundary limits: an (effectively) zero cleanup budget
/// against the `WINDOW_GRACE` observation delay.
fn unconfirmed_limits(tag: &str) -> SupervisorLimits {
    let mut limits = limits_at(tag);
    limits.cleanup_timeout = Duration::from_millis(1);
    limits.stdio_grace = WINDOW_GRACE;
    limits
}

fn supervisor_with(limits: SupervisorLimits) -> Arc<ProcessSupervisor> {
    Arc::new(ProcessSupervisor::new(Arc::new(SystemClock), limits).expect("test supervisor"))
}

fn owner_of(call: &str) -> ProcessOwner {
    ProcessOwner {
        principal_kind: "local_user".to_string(),
        principal_subject: "principal_local".to_string(),
        session_id: "sess_f03".to_string(),
        run_id: "run_f03".to_string(),
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

fn kernel_ctx(session: &str, run: &str) -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(session.to_string()),
        run_id: lingxi_protocol::RunId::new(run.to_string()),
        attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
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

/// The window-opening command: a backgrounded grandchild (`sleep 300`)
/// holds the output write ends; the direct child prints a marker,
/// publishes the grandchild pid and exits naturally after `CHILD_LIFE`.
fn window_cmd(pid_file: &std::path::Path, marker: &str) -> String {
    format!(
        "( sleep 300 ) & printf '%s\\n' \"$!\" > {}; echo {marker}; sleep {CHILD_LIFE}",
        pid_file.display()
    )
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

fn audit_kinds_of(snapshot: &lingxi_service::procsupervisor::ProcessSnapshot) -> Vec<String> {
    snapshot.audit.iter().map(|e| e.kind.to_string()).collect()
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

fn status_of(outcome: &ToolOutcome) -> Option<ToolRunStatus> {
    match outcome {
        ToolOutcome::Success { result } => result.status.as_deref().cloned(),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

/// Panics if the status is ANY `Exited` claim — used at the points where
/// the exit has NOT been observed: on the defect baseline the fabricated
/// `Signal(9)`/`137` lands exactly here (the RED assertion).
fn assert_no_exit_claim(status: &Option<ToolRunStatus>, context: &str) {
    if let Some(ToolRunStatus::Exited { code }) = status {
        panic!(
            "{context}: the exit was NOT observed, yet the result claims Exited({code}) — \
             an unconfirmed cleanup must never surface as an exit"
        );
    }
}

/// The single retained record handle (the one the tool call just
/// spawned) — used to check that statuses reference the ORIGINAL
/// handle.
fn spawned_handle_of(supervisor: &Arc<ProcessSupervisor>) -> String {
    supervisor
        .retained_record_ids()
        .into_iter()
        .next()
        .expect("exactly one retained record")
        .to_string()
}

fn process_alive(pid: i32) -> bool {
    // SAFETY: signal-0 liveness probe on a test-created process.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Kills a test-created process by EXACT pid (never a name-based sweep).
fn kill_owned(pid: i32) {
    // SAFETY: signal a process this test created itself.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
}

async fn read_grandchild_pid(pid_file: &std::path::Path) -> i32 {
    wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await
}

/// Polls until the next spawn over `supervisor` is ADMITTED (the live
/// slot was really returned) — the behavioral probe for exactly-once
/// capacity release under `live_cap = 1`.
async fn wait_for_slot_return(supervisor: &Arc<ProcessSupervisor>, ws: &std::path::Path) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let attempt = supervisor
            .spawn(oneshot_spec(
                "slot-probe",
                vec!["/usr/bin/true".to_string()],
                ws,
            ))
            .await;
        match attempt {
            Ok(spawned) => {
                // Let the probe settle so it does not hold the slot.
                let _ = supervisor.wait_terminal(&spawned.id).await;
                return;
            }
            Err(SpawnFailure::RegistryFull) => {
                assert!(
                    Instant::now() < end,
                    "the live slot was never returned after the confirmed settle"
                );
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            Err(other) => panic!("unexpected spawn failure probing the slot: {other:?}"),
        }
    }
}

// ── the RED repro (written FIRST, against the baseline APIs only) ───────────

/// R04-RR1-F03-C01: a one-shot command whose termination lands in the
/// reaped-but-undrained window under a tiny cleanup budget — the tool's
/// timeout watchdog fires, `terminate` cannot signal anything (the reap
/// invalidated the ownership proof) and the bounded wait expires before
/// the exit observation. The tool result at that instant must carry NO
/// exit claim, the record must stay queryable in the unconfirmed phase
/// with the request/expiry facts separated in the audit timeline, the
/// unsignalled grandchild must SURVIVE (nothing may pretend a kill
/// happened), the capacity slot must stay held, and the deferred REAL
/// exit (the natural `exit 0`, not a fabricated 137) must later be
/// recorded before the slot returns exactly once.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f03_c01_timeout_cleanup_unconfirmed_never_reports_an_exit() {
    let ws = test_dir("c01-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = unconfirmed_limits("c01");
    limits.live_cap = 1;
    let supervisor = supervisor_with(limits);
    let tools = tools_over(&supervisor, &ws);
    let ctx = kernel_ctx("sess_f03_c01", "run_f03_c01");
    let call = ToolCallId::new("call_f03_c01".to_string());
    // The timeout (1 s) lands INSIDE the drain window
    // (~[0.5 s, 0.5 s + 1.5 s]) — comfortably away from both edges.
    let outcome = tools
        .run_exec_command(
            &ctx,
            &call,
            &json!({
                "cmd": window_cmd(&pid_file, "HELLO_F03"),
                "timeout_seconds": 1
            }),
        )
        .await;
    // THE RED ASSERTION: at this instant the exit has not been observed.
    assert_no_exit_claim(&status_of(&outcome), "C01 tool result after the timeout");
    // The strengthened positive form: the honest, queryable,
    // handle-referencing unconfirmed state.
    match status_of(&outcome) {
        Some(ToolRunStatus::StopUnconfirmed { handle, detail }) => {
            assert_eq!(
                handle,
                spawned_handle_of(&supervisor),
                "the original handle"
            );
            assert!(
                detail.contains("not observed"),
                "the detail states the unconfirmed fact: {detail}"
            );
        }
        other => panic!("the unconfirmed timeout status: {other:?}"),
    }
    let text = text_of(&outcome);
    assert!(text.contains("HELLO_F03"), "partial output kept: {text}");
    assert!(
        text.contains("Command timed out after 1 seconds"),
        "the timeout notice stays: {text}"
    );
    assert!(
        text.contains("unconfirmed"),
        "the text states the uncertainty: {text}"
    );
    assert!(
        !text.contains("exited with code"),
        "no exit claim in the text: {text}"
    );
    // The record stays queryable in the unconfirmed phase.
    let handle = supervisor
        .retained_record_ids()
        .into_iter()
        .next()
        .expect("the record is retained while unconfirmed");
    let snapshot = supervisor.record(&handle).expect("queryable");
    assert!(
        matches!(
            &snapshot.phase,
            RecordPhase::CleanupTimedOut {
                reason: TerminationReason::Timeout
            }
        ),
        "the honest unconfirmed phase: {:?}",
        snapshot.phase
    );
    // The facts are separated on one timeline: the termination request
    // was decided (audited) BEFORE the cleanup-bound expiry, and no exit
    // observation exists yet. The direct child WAS reaped (natural
    // exit — the reap is a real observation); the EXIT FACT is the part
    // that is still pending.
    let kinds = audit_kinds_of(&snapshot);
    let request_at = kinds
        .iter()
        .position(|k| *k == "group_signal_skipped" || *k == "killpg_sent")
        .expect("the termination request is audited");
    let expired_at = kinds
        .iter()
        .position(|k| *k == "cleanup_timed_out")
        .expect("the cleanup-bound expiry is audited");
    assert!(request_at < expired_at, "timeline: {kinds:?}");
    assert!(
        !kinds.iter().any(|k| k == "exit_observed"),
        "no exit was observed: {kinds:?}"
    );
    assert!(
        snapshot.child_reaped,
        "the reap itself is a real observation"
    );
    // Nothing was signalled (ownership was honestly unprovable): the
    // unsignalled grandchild SURVIVES — the baseline's Exited(137) claim
    // contradicts the real OS state.
    let grandchild = read_grandchild_pid(&pid_file).await;
    assert!(
        process_alive(grandchild),
        "no signal was sent — the holder survives; a 137 claim would be fabricated"
    );
    assert!(
        supervisor.verification_signal_log().is_empty(),
        "no signal left this supervisor: {:?}",
        supervisor.verification_signal_log()
    );
    // The capacity slot stays HELD while the process end is unconfirmed
    // (live_cap = 1; the drain window leaves ~1 s of margin): admitting
    // a replacement spawn would understate the unconfirmed record.
    let refusal = supervisor
        .spawn(oneshot_spec(
            "c01-refusal",
            vec!["/usr/bin/true".to_string()],
            &ws,
        ))
        .await;
    assert!(
        matches!(refusal, Err(SpawnFailure::RegistryFull)),
        "the unconfirmed record still holds its slot: {refusal:?}"
    );
    // The deferred REAL exit: the drain bound expires, the pumps are
    // closed and the stored natural exit is finalized — the REAL fact
    // (code 0 — NOT a fabricated signal death) and the first cause.
    let terminal = wait_until("the deferred real exit", Duration::from_secs(5), || {
        let snap = supervisor.record(&handle)?;
        match &snap.phase {
            RecordPhase::Terminated {
                reason: TerminationReason::Timeout,
                fact,
            } => {
                let kinds = audit_kinds_of(&snap);
                Some((*fact, kinds))
            }
            _ => None,
        }
    })
    .await;
    let (fact, kinds) = terminal;
    assert!(
        matches!(fact, ExitFact::Code(0)),
        "the deferred observation is the natural exit 0: {fact:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "exit_observed"),
        "observed, not assumed: {kinds:?}"
    );
    // Exactly-once slot return: after the confirmed settle, a new spawn
    // is admitted again (the slot was neither leaked nor double-returned).
    wait_for_slot_return(&supervisor, &ws).await;
    kill_owned(grandchild);
    let _ = std::fs::remove_dir_all(&ws);
}

/// R04-RR1-F03-C02: a legal PTY handle enters `CleanupTimedOut`; the
/// owner polls `write_stdin` — the text and the structured status must
/// AGREE that the stop is unconfirmed (never `Exited { 137 }`), the
/// poll is repeatable, and the access control contrast still refuses a
/// foreign session.
///
/// The deterministic window (macOS delivers the pty hangup as soon as
/// the session leader exits, so the piped-family drain trick has no
/// pty counterpart): the review-sanctioned verification injection
/// `ReapFaultPoint::DelayObservationNext` holds the reaper's terminal
/// recording after the real `killpg` was sent — modeling an OS slow to
/// deliver the exit receipt. The kill really fires (audited, in the
/// signal log); a small cleanup bound expires first, deterministically.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f03_c02_pty_cleanup_unconfirmed_poll_agrees_with_the_text() {
    let ws = test_dir("c02-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let mut limits = limits_at("c02");
    limits.cleanup_timeout = Duration::from_millis(50);
    limits.stdio_grace = Duration::from_millis(300);
    let supervisor = supervisor_with(limits);
    supervisor.arm_reap_fault_for_verification(ReapFaultPoint::DelayObservationNext);
    let tools = tools_over(&supervisor, &ws);
    let ctx = kernel_ctx("sess_f03_c02", "run_f03_c02");
    let call = ToolCallId::new("call_f03_c02".to_string());
    let started = tools
        .run_exec_command(
            &ctx,
            &call,
            &json!({"cmd": "echo PTY_F03; sleep 300", "tty": true}),
        )
        .await;
    let handle = match status_of(&started) {
        Some(ToolRunStatus::Running { handle }) => handle,
        other => panic!("the terminal starts running: {other:?}"),
    };
    let parsed = ProcessHandleId::parse(&handle).expect("minted handle format");
    // The close: the killpg REALLY fires (the child is alive and the
    // ownership is proven at send time), then the bounded wait expires
    // inside the injected observation hold — the honest unconfirmed
    // receipt.
    let receipt = supervisor
        .terminate(&parsed, TerminationReason::Close)
        .await;
    assert!(
        matches!(receipt.outcome, TerminationOutcome::CleanupTimedOut),
        "the close enters the unconfirmed window: {:?}",
        receipt.outcome
    );
    let snapshot = supervisor.record(&parsed).expect("queryable");
    assert!(matches!(
        snapshot.phase,
        RecordPhase::CleanupTimedOut {
            reason: TerminationReason::Close
        }
    ));
    // The two facts are separated: the kill was really sent BEFORE the
    // bound expired, and no exit observation exists yet.
    let kinds = audit_kinds_of(&snapshot);
    let kill_at = kinds
        .iter()
        .position(|k| k == "killpg_sent")
        .expect("the kill was really sent");
    let expired_at = kinds
        .iter()
        .position(|k| k == "cleanup_timed_out")
        .expect("the bound expiry is audited");
    assert!(kill_at < expired_at, "timeline: {kinds:?}");
    assert!(!kinds.iter().any(|k| k == "exit_observed"), "{kinds:?}");
    assert!(
        !supervisor.verification_signal_log().is_empty(),
        "the kill really left this supervisor"
    );
    // The owner polls (empty chars). THE RED ASSERTION + its positive
    // form: text and structured status AGREE on the unconfirmed state.
    for round in 0..3 {
        let poll = tools
            .run_write_stdin(&ctx, &json!({"process_id": handle}))
            .await;
        assert_no_exit_claim(&status_of(&poll), &format!("C02 poll round {round}"));
        match status_of(&poll) {
            Some(ToolRunStatus::StopUnconfirmed { handle: h, detail }) => {
                assert_eq!(h, handle, "the poll references the original handle");
                assert!(
                    detail.contains("not observed"),
                    "the detail states the unconfirmed fact: {detail}"
                );
            }
            other => panic!("poll round {round} stays unconfirmed: {other:?}"),
        }
        let text = text_of(&poll);
        assert!(
            text.contains("cleanup_timed_out"),
            "the text states the unconfirmed phase: {text}"
        );
        assert!(
            text.contains("unconfirmed") || text.contains("not observed"),
            "the text does not pretend the stop is confirmed: {text}"
        );
        assert!(
            text.contains(&handle),
            "the poll references the original handle: {text}"
        );
    }
    // No duplicate confirmation receipts were minted by the polls.
    let snapshot = supervisor.record(&parsed).expect("queryable");
    let kinds = audit_kinds_of(&snapshot);
    assert_eq!(
        kinds.iter().filter(|k| *k == "cleanup_timed_out").count(),
        1,
        "one honest expiry receipt: {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k == "exit_observed"),
        "still unconfirmed: {kinds:?}"
    );
    // Access-control contrast: a foreign session is refused before any
    // status is produced (the honesty change must not loosen ownership).
    let foreign = kernel_ctx("sess_f03_foreign", "run_f03_foreign");
    match tools
        .run_write_stdin(&foreign, &json!({"process_id": handle}))
        .await
    {
        ToolOutcome::Failed { error } => {
            assert_eq!(error.code, lingxi_protocol::ErrorCode::Forbidden);
        }
        other => panic!("the foreign session is refused: {other:?}"),
    }
    // The deferred REAL exit (after the observation hold): the real
    // observed SIGKILL fact and the first cause (Close). A real
    // observation CAN be 137 — that is the whole point.
    let terminal = wait_until("the deferred real exit", Duration::from_secs(6), || {
        let snap = supervisor.record(&parsed)?;
        match &snap.phase {
            RecordPhase::Terminated {
                reason: TerminationReason::Close,
                fact,
            } => Some((*fact, audit_kinds_of(&snap))),
            _ => None,
        }
    })
    .await;
    let (fact, kinds) = terminal;
    assert!(
        matches!(fact, ExitFact::Signal(9)),
        "the deferred observation is the real SIGKILL: {fact:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "exit_observed"),
        "observed, not assumed: {kinds:?}"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

/// R04-RR1-F03-C03: a repeated termination must not auto-upgrade the
/// unconfirmed state into a confirmed `Terminated` receipt (the
/// baseline `terminal_facts` fallback fabricates `Signal(9)` when a
/// waiter observes a terminal-without-fact phase). The deferred real
/// exit keeps the real fact and the FIRST cause, and the live slot
/// returns exactly once (held while unconfirmed, released at the
/// confirmation).
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f03_c03_repeated_terminate_does_not_upgrade_unconfirmed_to_confirmed() {
    let ws = test_dir("c03-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = unconfirmed_limits("c03");
    limits.live_cap = 1;
    let supervisor = supervisor_with(limits);
    let spawned = supervisor
        .spawn(oneshot_spec(
            "c03",
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                window_cmd(&pid_file, "C03_MARKER"),
            ],
            &ws,
        ))
        .await
        .expect("spawn");
    // Barrier: the reaped-but-undrained window.
    wait_until(
        "reaped-but-undrained window",
        Duration::from_secs(5),
        || {
            let snap = supervisor.record(&spawned.id)?;
            (snap.child_reaped && !snap.phase.is_terminal()).then_some(snap)
        },
    )
    .await;
    // First termination: the honest unconfirmed receipt.
    let r1 = supervisor
        .terminate(&spawned.id, TerminationReason::Timeout)
        .await;
    assert!(
        matches!(r1.outcome, TerminationOutcome::CleanupTimedOut),
        "the first receipt stays unconfirmed: {:?}",
        r1.outcome
    );
    // THE CORE INVARIANT (the fabrication counterexample): the record is
    // terminal WITHOUT an observed exit fact, so NO receipt in this
    // state may claim Terminated. A repeated terminate gets the honest
    // AlreadyTerminal echo of the unconfirmed phase.
    let r2 = supervisor
        .terminate(&spawned.id, TerminationReason::Close)
        .await;
    match &r2.outcome {
        TerminationOutcome::AlreadyTerminal(RecordPhase::CleanupTimedOut { .. }) => {}
        other => panic!("the repeat terminate does not confirm the unconfirmed stop: {other:?}"),
    }
    // The capacity slot stays HELD while the exit is unconfirmed
    // (live_cap = 1; ~1 s of drain-window margin).
    let refusal = supervisor
        .spawn(oneshot_spec(
            "c03-refusal",
            vec!["/usr/bin/true".to_string()],
            &ws,
        ))
        .await;
    assert!(
        matches!(refusal, Err(SpawnFailure::RegistryFull)),
        "the unconfirmed record still holds its slot: {refusal:?}"
    );
    // The deferred REAL exit: the drain bound expires and the stored
    // natural exit is finalized — the REAL fact (code 0) and the FIRST
    // cause (Timeout — the reason of the first mark, not the repeat's
    // Close).
    let terminal = wait_until("the deferred real exit", Duration::from_secs(5), || {
        let snap = supervisor.record(&spawned.id)?;
        match &snap.phase {
            RecordPhase::Terminated {
                reason: TerminationReason::Timeout,
                fact,
            } => {
                let kinds = audit_kinds_of(&snap);
                Some((*fact, kinds))
            }
            _ => None,
        }
    })
    .await;
    let (fact, kinds) = terminal;
    assert!(
        matches!(fact, ExitFact::Code(0)),
        "the real observed natural exit: {fact:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "exit_observed"),
        "observed, not assumed: {kinds:?}"
    );
    // AFTER the confirmation, a terminate echoes the REAL fact and the
    // FIRST cause — the confirmed state is preserved, not re-fabricated.
    let r3 = supervisor
        .terminate(&spawned.id, TerminationReason::Close)
        .await;
    match &r3.outcome {
        TerminationOutcome::AlreadyTerminal(RecordPhase::Terminated { reason, fact }) => {
            assert!(
                matches!(reason, TerminationReason::Timeout),
                "the first cause is preserved: {reason:?}"
            );
            assert!(
                matches!(fact, ExitFact::Code(0)),
                "the real fact is preserved: {fact:?}"
            );
        }
        other => panic!("the confirmed record echoes its real terminal fact: {other:?}"),
    }
    // Exactly-once slot return after the confirmation (live_cap = 1).
    wait_for_slot_return(&supervisor, &ws).await;
    let grandchild = read_grandchild_pid(&pid_file).await;
    kill_owned(grandchild);
    let _ = std::fs::remove_dir_all(&ws);
}

// ── R04-RR1-F03-C04: the control group — real states stay accurate ─────────

/// C04: exit 0, exit 7, a REAL observed SIGKILL and a wait error, each
/// through the REAL tool call. The first three must state their REAL
/// status exactly (an observed SIGKILL keeps being a legitimate 137 —
/// the honesty change never degrades real observations into Unknown);
/// the wait error is an OBSERVATION failure — no fabricated -1/137.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f03_c04_control_group_real_states_stay_accurate() {
    let ws = test_dir("c04-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let supervisor = supervisor_with(limits_at("c04"));
    let tools = tools_over(&supervisor, &ws);
    let ctx = kernel_ctx("sess_f03_c04", "run_f03_c04");

    // Group 1: normal exit 0.
    let call = ToolCallId::new("call_f03_c04_ok".to_string());
    let ok = tools
        .run_exec_command(&ctx, &call, &json!({"cmd": "echo OK_F03"}))
        .await;
    match status_of(&ok) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 0),
        other => panic!("exit 0 is stated exactly: {other:?}"),
    }
    let handle = supervisor
        .retained_record_ids()
        .into_iter()
        .next()
        .expect("retained");
    assert!(matches!(
        supervisor.record(&handle).unwrap().phase,
        RecordPhase::Exited {
            fact: ExitFact::Code(0)
        }
    ));

    // Group 2: non-zero exit 7 (a real failure stays a real failure —
    // never reclassified as Unknown).
    let call = ToolCallId::new("call_f03_c04_7".to_string());
    let seven = tools
        .run_exec_command(&ctx, &call, &json!({"cmd": "exit 7"}))
        .await;
    match status_of(&seven) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 7),
        other => panic!("exit 7 is stated exactly: {other:?}"),
    }
    let handle = supervisor
        .retained_record_ids()
        .into_iter()
        .find(|id| *id != handle)
        .expect("retained");
    assert!(matches!(
        supervisor.record(&handle).unwrap().phase,
        RecordPhase::Exited {
            fact: ExitFact::Code(7)
        }
    ));

    // Group 3: an OBSERVED SIGKILL. The timeout chain really kills the
    // group and the wait really observes signal 9 — a legitimate 137.
    let mut kill_limits = limits_at("c04kill");
    kill_limits.cleanup_timeout = Duration::from_secs(2);
    kill_limits.stdio_grace = Duration::from_millis(200);
    let kill_supervisor = supervisor_with(kill_limits);
    let kill_tools = tools_over(&kill_supervisor, &ws);
    let call = ToolCallId::new("call_f03_c04_kill".to_string());
    let killed = kill_tools
        .run_exec_command(
            &ctx,
            &call,
            &json!({"cmd": "sleep 300", "timeout_seconds": 1}),
        )
        .await;
    match status_of(&killed) {
        Some(ToolRunStatus::Exited { code }) => {
            assert_eq!(code, 137, "an OBSERVED SIGKILL is a real 128+9")
        }
        other => panic!("the observed kill is stated exactly: {other:?}"),
    }
    let text = text_of(&killed);
    assert!(
        text.contains("Command timed out after 1 seconds"),
        "the timeout notice: {text}"
    );
    let kill_handle = kill_supervisor
        .retained_record_ids()
        .into_iter()
        .next()
        .expect("retained");
    let snapshot = kill_supervisor.record(&kill_handle).unwrap();
    assert!(matches!(
        snapshot.phase,
        RecordPhase::Terminated {
            reason: TerminationReason::Timeout,
            fact: ExitFact::Signal(9)
        }
    ));
    assert!(
        audit_kinds_of(&snapshot)
            .iter()
            .any(|k| k == "exit_observed"),
        "the 137 is observation-backed"
    );

    // Group 4: a wait ERROR (the verification-injected observation
    // failure — the code path a real failed `wait()` takes). The child
    // ended but no status can be read: an observation failure, never a
    // fabricated -1/137.
    let mut err_limits = limits_at("c04err");
    err_limits.stdio_grace = Duration::from_millis(100);
    let err_supervisor = supervisor_with(err_limits);
    err_supervisor.arm_reap_fault_for_verification(ReapFaultPoint::WaitErrorNext);
    let err_tools = tools_over(&err_supervisor, &ws);
    let call = ToolCallId::new("call_f03_c04_err".to_string());
    let errored = err_tools
        .run_exec_command(&ctx, &call, &json!({"cmd": "echo HI_F03"}))
        .await;
    match status_of(&errored) {
        Some(ToolRunStatus::StopUnconfirmed { handle, detail }) => {
            assert!(
                detail.contains("could not be read") || detail.contains("wait failed"),
                "the observation failure is described: {detail}"
            );
            let parsed = ProcessHandleId::parse(&handle).expect("minted handle");
            let snapshot = err_supervisor.record(&parsed).expect("queryable");
            assert!(
                matches!(
                    snapshot.phase,
                    RecordPhase::Exited {
                        fact: ExitFact::Unobserved
                    }
                ),
                "the honest fact: {:?}",
                snapshot.phase
            );
            assert!(
                audit_kinds_of(&snapshot).iter().any(|k| k == "wait_failed"),
                "the wait failure is audited"
            );
        }
        other => panic!("a failed wait is an observation failure, not an exit: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&ws);
}

/// C04 adversarial: cancellation racing a natural exit — every
/// interleaving lands on a REAL observation; any `Terminated` receipt
/// is backed by `exit_observed`.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f03_c04_adversarial_cancel_racing_natural_exit_stays_real() {
    let ws = test_dir("c04race-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let mut limits = limits_at("c04race");
    limits.stdio_grace = Duration::from_millis(100);
    let supervisor = supervisor_with(limits);
    for round in 0..10u32 {
        let spawned = supervisor
            .spawn(oneshot_spec(
                "c04race",
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "echo RACE_F03".to_string(),
                ],
                &ws,
            ))
            .await
            .expect("spawn");
        let waiter = supervisor.wait_terminal(&spawned.id);
        let terminator = supervisor.terminate(&spawned.id, TerminationReason::CallerDropped);
        let (phase, receipt) = tokio::join!(waiter, terminator);
        let phase = phase.expect("the record settles");
        assert!(phase.is_terminal(), "round {round}: {phase:?}");
        match &receipt.outcome {
            TerminationOutcome::Terminated { fact, .. } => {
                // Either side of the race legitimately wins: the natural
                // exit is Code(0); a terminate that won before the reap
                // fires the ownership-proven killpg and the OBSERVED exit
                // is Signal(9). Both are REAL observations.
                assert!(
                    matches!(fact, ExitFact::Code(0)) || matches!(fact, ExitFact::Signal(9)),
                    "round {round}: real observed exit: {fact:?}"
                );
                let snapshot = supervisor.record(&spawned.id).expect("retained");
                assert!(
                    audit_kinds_of(&snapshot)
                        .iter()
                        .any(|k| k == "exit_observed"),
                    "round {round}: the Terminated receipt is observation-backed"
                );
            }
            TerminationOutcome::AlreadyTerminal(RecordPhase::Exited { fact }) => {
                assert!(
                    matches!(fact, ExitFact::Code(0)),
                    "round {round}: the natural exit is the real code: {fact:?}"
                );
            }
            other => panic!("round {round}: safe receipt: {other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(&ws);
}

// ── R04-RR1-F03-C05: an upper-level cancel does not fake external quiet ────

/// The scripted-steps provider double (external model responses ONLY):
/// captures the TRUSTED run id the driver hands it, so the test can
/// cancel the in-flight run through the real user-facing entry.
struct RunIdCaptureProvider {
    steps: std::sync::Mutex<VecDeque<ProviderTurn>>,
    seen_run: std::sync::Mutex<Option<String>>,
}

impl RunIdCaptureProvider {
    fn new(steps: Vec<ProviderTurn>) -> Arc<Self> {
        Arc::new(Self {
            steps: std::sync::Mutex::new(steps.into_iter().collect()),
            seen_run: std::sync::Mutex::new(None),
        })
    }

    fn run_id(&self) -> Option<String> {
        self.seen_run.lock().unwrap().clone()
    }
}

impl TurnProviderPort for RunIdCaptureProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        _turn: u32,
        _input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        *self.seen_run.lock().unwrap() = Some(ctx.run_id.to_string());
        let next = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| final_turn("f03 done"));
        let ctx_at_issue = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, next) })
    }
}

fn final_turn(text: &str) -> ProviderTurn {
    ProviderTurn::Final {
        message: NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            model_call_id: None,
        },
    }
}

fn tool_request_turn(target: &str, args: serde_json::Value) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        requests: vec![ToolRequest::from_effective_arguments(
            target,
            args,
            &SchemaBudget::default(),
        )
        .expect("effective request")],
    }
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
        web_session_id: None,
        credential_id: None,
    }
}

struct FullHarness {
    state: ServiceState,
    supervisor: Arc<ProcessSupervisor>,
    root: PathBuf,
    home: PathBuf,
}

/// The full composition-root harness (the REAL run driver, journal,
/// registry, gateway, approvals and the REAL native process tools),
/// with the F03 controlled-boundary limits and the observation hold
/// armed: the cancel's kill really fires, and the exit receipt is
/// deterministically delayed past the cleanup bound.
async fn full_harness(
    provider: Arc<dyn TurnProviderPort>,
    limits: SupervisorLimits,
) -> FullHarness {
    let home = test_dir("c05-home");
    let root = test_dir("c05-root");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("workspace");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");
    let registry = Arc::new(ToolRegistry::new());
    let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let approvals = Arc::new(ApprovalService::new(Arc::new(SystemClock)));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(SystemClock),
        SchemaBudget::default(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    let supervisor =
        Arc::new(ProcessSupervisor::new(Arc::new(SystemClock), limits).expect("supervisor"));
    register_process_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&supervisor),
        Arc::clone(&access),
        ws.clone(),
        None,
        Arc::new(SystemClock),
        &SchemaBudget::default(),
    );
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_gateway: Some(Arc::clone(&gateway)),
        approval_gate: Some(Arc::clone(&approvals) as Arc<dyn approval::ApprovalGate>),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    FullHarness {
        state,
        supervisor,
        root,
        home,
    }
}

fn spawn_user_run(h: &FullHarness, session: &str, input: &str) -> tokio::task::JoinHandle<String> {
    let state = h.state.clone();
    let principal = owner_principal();
    let session = session.to_string();
    let input = input.to_string();
    tokio::spawn(async move {
        state
            .sessions()
            .execute_for(
                state.storage().as_ref(),
                state.events(),
                state.runs(),
                &principal,
                &session,
                &input,
                1_790_409_600_000,
            )
            .await
            .expect("user run drives")
            .run_id
    })
}

async fn journal_of(h: &FullHarness, run_id: &str) -> Vec<InvocationJournalEntry> {
    h.state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.to_string()))
        .await
        .expect("journal loads")
}

/// C05: the run requests cancellation while the process cleanup is
/// UNCONFIRMED. The Run-side fact (a cancelled control flow, the R03
/// Unknown window) and the process-side fact (a kill that was really
/// sent with the exit not yet observed) stay SEPARATE and queryable —
/// the receipt never claims all resources are quiet, and the deferred
/// REAL observation later lands on the record with the first cause.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f03_c05_run_cancel_keeps_control_flow_and_external_unconfirmed_separate() {
    let mut limits = limits_at("c05");
    limits.cleanup_timeout = Duration::from_millis(50);
    limits.stdio_grace = Duration::from_millis(200);
    let provider = RunIdCaptureProvider::new(vec![
        tool_request_turn(
            "tool:first-party:exec_command",
            json!({"cmd": "echo C05_F03; sleep 300"}),
        ),
        final_turn("c05 done"),
    ]);
    let h = full_harness(provider.clone(), limits).await;
    // The observation hold makes the exit receipt deterministically late
    // (the kill really fires at cancel; the receipt arrives after).
    h.supervisor
        .arm_reap_fault_for_verification(ReapFaultPoint::DelayObservationNext);
    // The bootstrap seeds the synthetic local session the driver
    // requires ("sess_local_alpha" — the t05/t08 harness precedent).
    let run = spawn_user_run(&h, "sess_local_alpha", "F03 C05 cancel leg");
    let handle = wait_until("live process record", Duration::from_secs(20), || {
        h.supervisor.live_handles().first().cloned()
    })
    .await;
    let run_id = wait_until("trusted run id", Duration::from_secs(20), || {
        provider.run_id()
    })
    .await;
    let fired = h
        .state
        .runs()
        .cancel_run(run_id.as_str(), "user cancelled the hanging command");
    assert!(
        matches!(
            fired,
            lingxi_service::cancel::FireOutcome::Fired
                | lingxi_service::cancel::FireOutcome::AlreadyCancelling
        ),
        "cancellation fired: {fired:?}"
    );
    // FACT 1 (process side): the kill really fired and the exit was NOT
    // observed within the bound — the record is honestly unconfirmed,
    // never marked Terminated/Exited while unconfirmed.
    let unconfirmed = wait_until("the unconfirmed phase", Duration::from_secs(5), || {
        let snap = h.supervisor.record(&handle)?;
        matches!(snap.phase, RecordPhase::CleanupTimedOut { .. }).then_some(snap)
    })
    .await;
    let kinds = audit_kinds_of(&unconfirmed);
    let kill_at = kinds
        .iter()
        .position(|k| k == "killpg_sent")
        .expect("the cancel's kill really fired");
    let expired_at = kinds
        .iter()
        .position(|k| k == "cleanup_timed_out")
        .expect("the bound expiry is audited");
    assert!(kill_at < expired_at, "timeline: {kinds:?}");
    assert!(!kinds.iter().any(|k| k == "exit_observed"), "{kinds:?}");
    // FACT 2 (control-flow side): the Run settles cancelled and the
    // invocation journals the R03 Unknown window — started without a
    // receipt, nothing fabricated as the external result.
    let settled = match run.await {
        Ok(id) => id,
        Err(err) => panic!("run settles cancelled: {err}"),
    };
    assert_eq!(settled, run_id, "the settled run is the cancelled one");
    let journal = journal_of(&h, &run_id).await;
    let entry = journal
        .iter()
        .find(|e| e.target.ends_with("exec_command"))
        .expect("the exec_command journal entry");
    assert!(
        matches!(entry.phase, InvocationPhase::Started),
        "the cancelled control flow journals Started (Unknown window): {:?}",
        entry.phase
    );
    assert!(
        entry.receipt.is_none(),
        "no fabricated receipt while the external outcome is unconfirmed"
    );
    // FACT 3 (the deferred real observation): the kill's receipt arrives
    // after the hold — the real fact with the first cause (CallerDropped)
    // — and only then the live slot returns (the two facts were never
    // merged into a premature "all quiet").
    let terminal = wait_until("the deferred real exit", Duration::from_secs(6), || {
        let snap = h.supervisor.record(&handle)?;
        match &snap.phase {
            RecordPhase::Terminated {
                reason: TerminationReason::CallerDropped,
                fact,
            } => Some((*fact, audit_kinds_of(&snap))),
            _ => None,
        }
    })
    .await;
    let (fact, kinds) = terminal;
    assert!(
        matches!(fact, ExitFact::Signal(9)),
        "the real observed kill: {fact:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "exit_observed"),
        "observed, not assumed: {kinds:?}"
    );
    wait_until("nothing live", Duration::from_secs(5), || {
        h.supervisor.live_handles().is_empty().then_some(())
    })
    .await;
    h.state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&h.home);
    let _ = std::fs::remove_dir_all(&h.root);
}

/// C05 adversarial: the tool future is DROPPED outside any run driver
/// (the pure ProcessOwnershipGuard seam) — the bounded cleanup
/// responsibility survives the dropped future, stays honest about the
/// unconfirmed window, and the deferred observation still lands.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f03_c05_adversarial_dropped_tool_future_keeps_the_honest_chain() {
    let ws = test_dir("c05drop-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let mut limits = limits_at("c05drop");
    limits.cleanup_timeout = Duration::from_millis(50);
    limits.stdio_grace = Duration::from_millis(200);
    let supervisor = supervisor_with(limits);
    supervisor.arm_reap_fault_for_verification(ReapFaultPoint::DelayObservationNext);
    let tools = tools_over(&supervisor, &ws);
    let ctx = kernel_ctx("sess_f03_c05d", "run_f03_c05d");
    let call = ToolCallId::new("call_f03_c05d".to_string());
    let driver = tokio::spawn(async move {
        tools
            .run_exec_command(
                &ctx,
                &call,
                &json!({"cmd": "echo DROP_F03; sleep 300", "timeout_seconds": 30}),
            )
            .await
    });
    let handle = wait_until("live process record", Duration::from_secs(5), || {
        supervisor.live_handles().first().cloned()
    })
    .await;
    // The caller's future vanishes mid-flight (the run-driver cancel
    // drops it exactly like this).
    driver.abort();
    // The guard's detached chain really fired the kill and honestly
    // reports the unconfirmed window.
    let unconfirmed = wait_until("the unconfirmed phase", Duration::from_secs(5), || {
        let snap = supervisor.record(&handle)?;
        matches!(snap.phase, RecordPhase::CleanupTimedOut { .. }).then_some(snap)
    })
    .await;
    let kinds = audit_kinds_of(&unconfirmed);
    assert!(
        kinds.iter().any(|k| k == "killpg_sent"),
        "the drop guard really sent the kill: {kinds:?}"
    );
    assert!(!kinds.iter().any(|k| k == "exit_observed"), "{kinds:?}");
    // The deferred real observation lands with the first cause
    // (CallerDropped — the drop-guard's reason).
    wait_until("the deferred real exit", Duration::from_secs(6), || {
        let snap = supervisor.record(&handle)?;
        matches!(
            &snap.phase,
            RecordPhase::Terminated {
                reason: TerminationReason::CallerDropped,
                fact: ExitFact::Signal(9)
            }
        )
        .then_some(())
    })
    .await;
    wait_until("nothing live", Duration::from_secs(5), || {
        supervisor.live_handles().is_empty().then_some(())
    })
    .await;
    let _ = std::fs::remove_dir_all(&ws);
}
