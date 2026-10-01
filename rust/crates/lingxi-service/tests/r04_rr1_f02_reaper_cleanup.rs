//! R04-RR1-F02 acceptance: the one-shot reaper's output pumps are REALLY
//! closed (timeout on a JoinHandle is NOT an abort — a dropped handle
//! detaches the task, which keeps holding the pipe read ends and keeps
//! mutating the settled collector), and a group signal is only ever fired
//! at a process group whose ownership is STILL provable at send time.
//!
//! Everything here runs against the REAL `ProcessSupervisor` (POSIX
//! process groups, killpg, bounded cleanup) with REAL OS processes:
//! `/bin/sh -c` direct children whose backgrounded grandchildren hold
//! the stdout/stderr pipe write ends (the C01/C03 shape), short-lived
//! children whose termination races the reap boundary (the C02 shape),
//! and repeated short-parent cycles (the C04 shape).
//!
//! Test-double boundary: NONE — the supervisor under test is the product
//! type. The sentinel processes (`/bin/sleep`) are unrelated test-created
//! processes, killed by exact pid at teardown. All test state lives in
//! unique `std::env::temp_dir()` subdirectories (never a shared /tmp
//! wildcard, never a name-based pkill).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lingxi_kernel::ports::{ToolOutcome, ToolRunStatus};
use lingxi_kernel::RunContext;
use lingxi_protocol::ToolCallId;
use lingxi_service::exectools::ProcessTools;
use lingxi_service::inject::SystemClock;
use lingxi_service::procsupervisor::{
    ProcessKind, ProcessOwner, ProcessSupervisor, PumpFaultPoint, RecordPhase, SignalTarget,
    SpawnSpec, SupervisorLimits, TerminationOutcome, TerminationReason,
};
use lingxi_service::resourceaccess::ResourceAccess;
use serde_json::json;

// ── harness ─────────────────────────────────────────────────────────────────

/// A unique test directory (pid + nanos + sequence — no cross-test
/// collisions, never the shared /tmp itself).
fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r04rr1f02-{tag}-{}-{}-{}",
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

fn supervisor_with(limits: SupervisorLimits) -> Arc<ProcessSupervisor> {
    Arc::new(ProcessSupervisor::new(Arc::new(SystemClock), limits).expect("test supervisor"))
}

fn owner_of(call: &str) -> ProcessOwner {
    ProcessOwner {
        principal_kind: "local_user".to_string(),
        principal_subject: "principal_local".to_string(),
        session_id: "sess_f02".to_string(),
        run_id: "run_f02".to_string(),
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

/// A one-shot `/bin/sh -c <cmd>` spec.
fn sh_spec(call: &str, cmd: String, cwd: &std::path::Path) -> SpawnSpec {
    oneshot_spec(
        call,
        vec!["/bin/sh".to_string(), "-c".to_string(), cmd],
        cwd,
    )
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

/// Spawns an UNRELATED sentinel (`/bin/sleep`) owned by the test; the
/// caller probes its liveness and disposes of it by exact pid.
fn spawn_sentinel(seconds: u32) -> i32 {
    let child = std::process::Command::new("/bin/sleep")
        .arg(seconds.to_string())
        .spawn()
        .expect("sentinel spawns");
    let pid = child.id() as i32;
    // The handle is deliberately leaked: the test manages sentinel
    // liveness by exact pid and kills it at teardown.
    std::mem::forget(child);
    pid
}

fn audit_kinds_of(snapshot: &lingxi_service::procsupervisor::ProcessSnapshot) -> Vec<&str> {
    snapshot.audit.iter().map(|e| e.kind).collect()
}

/// File-wide serial guard (the G01 suite's precedent): several tests
/// below compare ABSOLUTE open-descriptor counts of THIS test process
/// (the fd-table evidence for read-end closure), which are only
/// meaningful without sibling tests opening/closing descriptors in
/// parallel. Poisoned locks are recovered (a panicked test must not
/// brick the rest of the file).
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

/// Waits for the supervisor's owned-task count to return to `expected`
/// (the reaper exits shortly AFTER settle — the terminal phase alone is
/// not yet the whole observation).
async fn tasks_settle_to(supervisor: &ProcessSupervisor, expected: usize) {
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let count = supervisor.owned_task_count();
        if count == expected {
            return;
        }
        assert!(
            Instant::now() < end,
            "owned task count stuck at {count} (expected {expected}) — a supervisor-owned \
             task never ended"
        );
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

// ── real-OS probes ──────────────────────────────────────────────────────────

fn process_alive(pid: i32) -> bool {
    // SAFETY: signal-0 liveness probe.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// The number of fds THIS test process currently has open (macOS lists
/// them under /dev/fd; Linux under /proc/self/fd). The observation the
/// C01/C03 evidence needs: the pipe READ ends a detached pump would keep
/// open show up here after the record went terminal.
fn count_open_fds() -> usize {
    let listed = std::fs::read_dir("/dev/fd")
        .or_else(|_| std::fs::read_dir("/proc/self/fd"))
        .map(|entries| entries.count())
        .unwrap_or(0);
    if listed > 0 {
        return listed;
    }
    // Fallback for platforms without a listing directory: a dense
    // fcntl(F_GETFD) scan (open descriptors are allocated lowest-first).
    let mut count = 0usize;
    for fd in 0..1024i32 {
        // SAFETY: F_GETFD probes descriptor validity only.
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } != -1 {
            count += 1;
        }
    }
    count
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

/// Kills a test-created process by EXACT pid (never a name-based sweep).
fn kill_owned(pid: i32) {
    // SAFETY: signal a process this test created itself.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
}

// ── the RED repro (written FIRST, against the baseline APIs only) ───────────

/// R04-RR1-F02 minimal counterexample: the direct child exits, a
/// backgrounded grandchild keeps holding BOTH pipe write ends and keeps
/// writing slow ticks. After the record goes terminal (the stdio grace
/// expired), the settled collector MUST be frozen and the pipe read ends
/// MUST be closed. On the baseline the `timeout(grace, pump)` future
/// merely DROPS the JoinHandle — the detached pump keeps running, keeps
/// appending to the settled collector and keeps the read fds open.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_repro_grandchild_pump_outlives_the_grace_and_mutates_the_settled_collector() {
    let _serial = serial().await;
    let ws = test_dir("repro-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("repro");
    limits.stdio_grace = Duration::from_millis(250);
    let supervisor = supervisor_with(limits);
    let fds_baseline = count_open_fds();
    let spec = oneshot_spec(
        "repro",
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            format!(
                "( sleep 1; echo TICK_ONE; sleep 3; echo TICK_TWO ) & \
                 printf '%s\\n' \"$!\" > {}; echo PARENT_DONE",
                pid_file.display()
            ),
        ],
        &ws,
    );
    let spawned = supervisor.spawn(spec).await.expect("the one-shot spawns");
    let phase = supervisor
        .wait_terminal(&spawned.id)
        .await
        .expect("the record reaches a terminal phase");
    assert!(phase.is_terminal(), "{phase:?}");
    // The settled collector snapshot at the terminal phase.
    let at_terminal = supervisor
        .output_snapshot(&spawned.id)
        .expect("the settled record is retained for receipts");
    assert!(
        String::from_utf8_lossy(&at_terminal.window).contains("PARENT_DONE"),
        "the drained prefix is kept: {:?}",
        String::from_utf8_lossy(&at_terminal.window)
    );
    // INVARIANT 1 (the counterexample): nothing owned by this record may
    // change after the terminal phase. The grandchild writes TICK_ONE at
    // ~1s and TICK_TWO at ~2s; a detached pump (if one survived the
    // grace) would append them to the settled collector.
    let mut mutated: Option<(u64, u64)> = None;
    let poll_end = Instant::now() + Duration::from_millis(2400);
    while Instant::now() < poll_end {
        tokio::time::sleep(Duration::from_millis(120)).await;
        let snap = supervisor
            .output_snapshot(&spawned.id)
            .expect("the settled record is retained");
        if snap.total_bytes != at_terminal.total_bytes {
            mutated = Some((at_terminal.total_bytes, snap.total_bytes));
            break;
        }
    }
    assert!(
        mutated.is_none(),
        "the settled collector CHANGED after the terminal phase (total_bytes {:?}): \
         a pump task survived the stdio grace and kept mutating settled state",
        mutated
    );
    // INVARIANT 2: the read ends are closed — the fd table returns to the
    // pre-spawn baseline (a detached pump would still hold both).
    let fds_after = count_open_fds();
    assert_eq!(
        fds_after, fds_baseline,
        "pipe read ends must be closed at the terminal phase (fds {fds_after} vs baseline \
         {fds_baseline})"
    );
    // The backgrounded grandchild (a normal `cmd &`) is NEVER signalled
    // by the supervisor — the approved lifetime semantics. A WRITING
    // holder may die of its own SIGPIPE once the read end is closed
    // (the OS contract of a pipe whose reader is gone — the incumbent's
    // behavior too); that death is not ours, and the signal log proves
    // it: zero signals left this supervisor for the whole lifecycle.
    let signals = supervisor.verification_signal_log();
    assert!(
        signals.is_empty(),
        "a naturally-exited one-shot must not be signalled: {signals:?}"
    );
    let grandchild = wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await;
    // Dispose of the test-created grandchild by exact pid (a no-op if it
    // already ended of its own SIGPIPE).
    kill_owned(grandchild);
    let _ = std::fs::remove_dir_all(&ws);
}

// ── same-family regression: spawn-side "reaping precedes ownership" ─────────

/// The spawn-side instance of F02's "OS reaping precedes the ownership
/// state update" (observed flaking the baseline suite ~1/3 runs): a
/// FAST-exiting child can be reaped by tokio's opportunistic process
/// driver BEFORE `verify_group`'s getpgid runs, which made the baseline
/// refuse a legitimately-ours child with EXEC_SPAWN_FAILED (the command
/// HAD run — its output was already collected). The ownership proof now
/// falls back to OUR OWN Child's `try_wait`: a wait-confirmed exit is
/// proof the child was ours and is gone; the record admits, the reaper
/// publishes the reap fact, and the whole chain behaves like any natural
/// exit. Twenty back-to-back fast exits make the old race near-certain.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_fast_exiting_child_is_not_refused_by_the_reap_race() {
    let _serial = serial().await;
    let ws = test_dir("fastexit-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let supervisor = supervisor_with(limits_at("fastexit"));
    for round in 0..20u32 {
        let spec = oneshot_spec("fastexit", vec!["/usr/bin/true".to_string()], &ws);
        let spawned = match supervisor.spawn(spec).await {
            Ok(spawned) => spawned,
            Err(failure) => {
                panic!("round {round}: a fast-exiting child of ours must not be refused: {failure}")
            }
        };
        let phase = supervisor
            .wait_terminal(&spawned.id)
            .await
            .expect("round: terminal");
        match phase {
            RecordPhase::Exited { fact } => assert_eq!(fact.status_code(), 0),
            other => panic!("round {round}: natural exit 0, got {other:?}"),
        }
        let snapshot = supervisor.record(&spawned.id).expect("retained");
        assert!(snapshot.child_reaped, "round {round}");
        assert_eq!(snapshot.pumps_alive, 0, "round {round}");
        assert!(snapshot.drained_stdio, "round {round}: true drains fast");
        assert!(snapshot.reclaimed, "round {round}");
        assert!(
            supervisor.verification_signal_log().is_empty(),
            "round {round}: no signal for natural exits"
        );
    }
    assert_eq!(supervisor.live_handles().len(), 0);
    tasks_settle_to(&supervisor, 0).await;
    let _ = std::fs::remove_dir_all(&ws);
}

// ── R04-RR1-F02-C01: grandchild holds the output ends ───────────────────────

/// C01 normal leg, through the REAL tool surface (`ProcessTools::
/// run_exec_command` → the REAL supervisor): the direct child exits, a
/// SILENT backgrounded grandchild holds both write ends. The tool result
/// is only produced after the pumps were really closed; the settled
/// collector is frozen; the fd table is back to baseline; the grandchild
/// (a normal `cmd &`) is never signalled and SURVIVES.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c01_grandchild_holding_both_ends_freezes_the_result_and_closes_the_read_ends() {
    let _serial = serial().await;
    let ws = test_dir("c01-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("c01");
    limits.stdio_grace = Duration::from_millis(300);
    let supervisor = supervisor_with(limits);
    let tools = ProcessTools::new(
        Arc::clone(&supervisor),
        Arc::new(ResourceAccess::new(std::slice::from_ref(&ws)).expect("access")),
        ws.clone(),
    );
    let sentinel = spawn_sentinel(300);
    let fds_baseline = count_open_fds();
    let ctx = kernel_ctx("sess_f02_c01", "run_c01");
    let call = ToolCallId::new("call_c01".to_string());
    let outcome = tools
        .run_exec_command(
            &ctx,
            &call,
            &json!({
                "cmd": format!(
                    "( sleep 5 ) & printf '%s\\n' \"$!\" > {}; echo PARENT_DONE",
                    pid_file.display()
                )
            }),
        )
        .await;
    // The completed result: exit 0 with the drained prefix.
    match &outcome {
        ToolOutcome::Success { result } => {
            let text: String = result
                .content
                .iter()
                .filter_map(|b| match b {
                    lingxi_protocol::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(text.contains("PARENT_DONE"), "{text}");
            match &result.status {
                Some(ToolRunStatus::Exited { code }) => assert_eq!(*code, 0),
                other => panic!("exit 0: {other:?}"),
            }
        }
        other => panic!("success outcome: {other:?}"),
    }
    // The record behind the handle the tool returned.
    let live = supervisor.live_handles();
    assert!(live.is_empty(), "the one-shot is fully settled: {live:?}");
    let handle = supervisor
        .retained_record_ids()
        .into_iter()
        .next()
        .expect("one retained record");
    let snapshot = supervisor.record(&handle).expect("retained");
    assert!(matches!(snapshot.phase, RecordPhase::Exited { fact } if fact.status_code() == 0));
    assert!(snapshot.child_reaped, "the reap was really observed");
    // Pump-exit observation: zero output tasks remain for this record.
    wait_until("pumps closed", Duration::from_secs(2), || {
        let snap = supervisor.record(&handle)?;
        (snap.pumps_alive == 0).then_some(snap)
    })
    .await;
    let snapshot = supervisor.record(&handle).unwrap();
    assert!(
        !snapshot.drained_stdio,
        "honest: holders kept the streams open"
    );
    assert!(
        snapshot.reclaimed,
        "every task end was OBSERVED (abort+join)"
    );
    let kinds = audit_kinds_of(&snapshot);
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == "output_task_closed_after_abort")
            .count(),
        2,
        "both streams were aborted-and-observed: {snapshot:?}"
    );
    // FD evidence: the pipe read ends are closed (back to the baseline).
    let fds_now = count_open_fds();
    assert_eq!(fds_now, fds_baseline, "read ends closed at settle");
    // Collector freeze: the settled collector never changes again.
    let frozen = supervisor.output_snapshot(&handle).unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let recheck = supervisor.output_snapshot(&handle).unwrap();
    assert_eq!(recheck.total_bytes, frozen.total_bytes);
    assert_eq!(recheck.window, frozen.window);
    // The backgrounded grandchild SURVIVES (approved lifetime semantics:
    // a natural exit never kills the group) and NO signal was sent.
    let grandchild = wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await;
    assert!(
        process_alive(grandchild),
        "the silent backgrounded grandchild lives (cmd & semantics)"
    );
    assert!(
        supervisor.verification_signal_log().is_empty(),
        "a naturally-exited one-shot is never signalled"
    );
    assert!(process_alive(sentinel), "the unrelated sentinel survives");
    kill_owned(grandchild);
    kill_owned(sentinel);
    let _ = std::fs::remove_dir_all(&ws);
}

/// C01's PTY-family leg: the direct terminal child exits while a
/// backgrounded grandchild holds the PTY SLAVE open — the master stays
/// readable forever on the baseline (an unsupervised reader task holding
/// a dup'd master fd). After the bounded grace the reaper must abort the
/// reader AND observe its end: the dup'd master fd drops with it, the
/// last master reference is closed, the transcript is frozen and no
/// output task survives the record.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c01_pty_family_grandchild_holding_the_slave_closes_the_master() {
    let _serial = serial().await;
    let ws = test_dir("c01pty-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("c01pty");
    limits.stdio_grace = Duration::from_millis(300);
    let supervisor = supervisor_with(limits);
    let sentinel = spawn_sentinel(300);
    let fds_baseline = count_open_fds();
    let spec = SpawnSpec {
        argv: vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            format!(
                "( sleep 8 ) & printf '%s\\n' \"$!\" > {}; sleep 0.4; echo PTY_DONE",
                pid_file.display()
            ),
        ],
        cwd: ws.clone(),
        env: BTreeMap::new(),
        owner: owner_of("c01pty"),
        kind: ProcessKind::PersistentTerminal,
        cols: 80,
        rows: 24,
    };
    let spawned = supervisor
        .spawn(spec)
        .await
        .expect("the pty terminal spawns");
    let phase = supervisor
        .wait_terminal(&spawned.id)
        .await
        .expect("terminal phase");
    match phase {
        RecordPhase::Exited { fact } => assert_eq!(fact.status_code(), 0),
        other => panic!("natural exit 0: {other:?}"),
    }
    // Fresh post-terminal facts (never the probe's possibly-stale
    // snapshot: the reader can observe the hangup BEFORE the reaper
    // publishes the reap).
    let snapshot = wait_until("pty reader closed", Duration::from_secs(2), || {
        let snap = supervisor.record(&spawned.id)?;
        (snap.pumps_alive == 0 && snap.child_reaped && snap.phase.is_terminal()).then_some(snap)
    })
    .await;
    assert!(snapshot.child_reaped);
    assert!(snapshot.reclaimed, "the reader's end was observed");
    // The reader's disposition is the OS's pty semantics, honestly
    // recorded either way: macOS may deliver the hangup to the master as
    // soon as the session leader exits (natural EOF — drained_stdio
    // true), or the grandchild's slave fd may keep the master readable
    // until the grace expires (abort + observed join — drained_stdio
    // false). Both are REAL observations; neither may hide a surviving
    // task or an open master.
    let kinds = audit_kinds_of(&snapshot);
    let natural = kinds.contains(&"output_task_finished");
    let aborted = kinds.contains(&"output_task_closed_after_abort");
    assert!(
        natural || aborted,
        "the pty reader's end is audited: {snapshot:?}"
    );
    assert_eq!(
        natural, snapshot.drained_stdio,
        "drained_stdio matches the observed disposition: {snapshot:?}"
    );
    // The master is fully closed (our fd in finalize + the reader's dup
    // with the observed task end): the fd table is back at baseline.
    assert_eq!(count_open_fds(), fds_baseline, "pty master fds closed");
    // The transcript is frozen once terminal (deliver drains the ring;
    // a second delivery returns nothing new).
    let first = supervisor.pty_deliver(&spawned.id).await.expect("deliver");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let second = supervisor.pty_deliver(&spawned.id).await.expect("deliver");
    assert_eq!(second.total_bytes, first.total_bytes, "transcript frozen");
    assert!(
        String::from_utf8_lossy(first.text.as_bytes()).contains("PTY_DONE")
            || first.text.is_empty() && second.text.is_empty(),
        "delivered transcript: {:?}",
        first.text
    );
    // No signal was ever sent (natural exit); the grandchild and the
    // sentinel survive; the supervisor owns no task anymore.
    assert!(
        supervisor.verification_signal_log().is_empty(),
        "{:?}",
        supervisor.verification_signal_log()
    );
    tasks_settle_to(&supervisor, 0).await;
    assert!(process_alive(sentinel), "the sentinel survives");
    let grandchild = wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await;
    // NOTE (honest): unlike the one-shot pipe family, a PTY grandchild's
    // fate is decided by the TERMINAL, not by us — the kernel sends
    // SIGHUP to the foreground group when the session leader exits, so
    // the backgrounded holder usually dies on its own. The supervisor's
    // obligation (proven above: empty signal log) is that WE sent
    // nothing; whether the grandchild survives the hangup is the OS's
    // pty semantics and is recorded, not asserted.
    let _grandchild_alive = process_alive(grandchild);
    kill_owned(grandchild); // exact-pid disposal (no-op if SIGHUP got it)
    kill_owned(sentinel);
    let _ = std::fs::remove_dir_all(&ws);
}

/// C01 adversarial leg: the grandchild INHERITS BOTH output ends and
/// keeps SLOWLY WRITING after the terminal phase — the settled collector
/// must not move by a single byte, and the writer's own SIGPIPE death is
/// not a supervisor signal.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c01_adversarial_slow_writer_cannot_mutate_the_settled_collector() {
    let _serial = serial().await;
    let ws = test_dir("c01adv-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("c01adv");
    limits.stdio_grace = Duration::from_millis(250);
    let supervisor = supervisor_with(limits);
    let sentinel = spawn_sentinel(300);
    let spec = sh_spec(
        "c01adv",
        format!(
            "( sleep 0.6; echo LATE_A; sleep 1.0; echo LATE_B; sleep 1.0 ) & \
             printf '%s\\n' \"$!\" > {}; echo PARENT_DONE",
            pid_file.display()
        ),
        &ws,
    );
    let spawned = supervisor.spawn(spec).await.expect("spawn");
    let phase = supervisor
        .wait_terminal(&spawned.id)
        .await
        .expect("terminal");
    assert!(phase.is_terminal());
    let at_terminal = supervisor.output_snapshot(&spawned.id).unwrap();
    assert!(
        String::from_utf8_lossy(&at_terminal.window).contains("PARENT_DONE"),
        "{:?}",
        String::from_utf8_lossy(&at_terminal.window)
    );
    let snapshot = supervisor.record(&spawned.id).unwrap();
    assert_eq!(snapshot.pumps_alive, 0, "no detached pump survives");
    // Slow writes continue for ~2.6s after the terminal phase: probe the
    // collector the whole time — not one byte may land.
    let probe_end = Instant::now() + Duration::from_millis(2000);
    while Instant::now() < probe_end {
        let snap = supervisor.output_snapshot(&spawned.id).unwrap();
        assert_eq!(
            snap.total_bytes, at_terminal.total_bytes,
            "the settled collector gained late output"
        );
        assert_eq!(snap.window, at_terminal.window, "the window moved");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // The writer's fate is its own (SIGPIPE on the closed pipe) — never
    // a supervisor signal.
    assert!(
        supervisor.verification_signal_log().is_empty(),
        "{:?}",
        supervisor.verification_signal_log()
    );
    assert!(process_alive(sentinel), "the sentinel survives");
    let grandchild = wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await;
    kill_owned(grandchild);
    kill_owned(sentinel);
    let _ = std::fs::remove_dir_all(&ws);
}

// ── R04-RR1-F02-C02: the reaped-but-undrained window ────────────────────────

/// C02 normal leg: the direct child is REALLY reaped (`wait()` resolved)
/// while the pump is still inside the stdio-grace window (the grandchild
/// holds the write ends). A terminate arriving in that window must fire
/// NO group signal at the stale pgid and must report the direct child's
/// real exit plus the honest pipe state.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c02_terminate_in_the_reaped_undrained_window_signals_nothing() {
    let _serial = serial().await;
    let ws = test_dir("c02-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("c02");
    limits.stdio_grace = Duration::from_millis(1500);
    let supervisor = supervisor_with(limits);
    let sentinel = spawn_sentinel(300);
    let fds_baseline = count_open_fds();
    let spec = sh_spec(
        "c02",
        format!(
            "( sleep 8 ) & printf '%s\\n' \"$!\" > {}; echo DONE",
            pid_file.display()
        ),
        &ws,
    );
    let spawned = supervisor.spawn(spec).await.expect("spawn");
    // Barrier: the REAL wait boundary — child reaped, record NOT yet
    // terminal (the reaper is inside its drain window).
    let in_window = wait_until(
        "reaped-but-undrained window",
        Duration::from_secs(5),
        || {
            let snap = supervisor.record(&spawned.id)?;
            (snap.child_reaped && !snap.phase.is_terminal()).then_some(snap)
        },
    )
    .await;
    assert!(in_window.child_reaped);
    // The cancel/close request INSIDE the window.
    let receipt = supervisor
        .terminate(&spawned.id, TerminationReason::Close)
        .await;
    // Honest receipt: the direct child's REAL exit (code 0), reclaimed
    // (every pump end observed), NOT drained (the holders kept the ends).
    match receipt.outcome {
        TerminationOutcome::Terminated {
            fact,
            reclaimed,
            drained_stdio,
        } => {
            assert_eq!(fact.status_code(), 0, "the real direct-child exit");
            assert!(reclaimed, "read-end closure was observed");
            assert!(!drained_stdio, "the pipe state is honest");
        }
        other => panic!("terminated receipt in the window: {other:?}"),
    }
    let snapshot = supervisor.record(&spawned.id).unwrap();
    assert!(matches!(
        snapshot.phase,
        RecordPhase::Terminated {
            reason: TerminationReason::Close,
            fact
        } if fact.status_code() == 0
    ));
    // THE C02 core: no group signal was ever fired — the exhaustive
    // signal log is empty and the skip is audited with the reap reason.
    assert!(
        supervisor.verification_signal_log().is_empty(),
        "no signal at the stale pgid: {:?}",
        supervisor.verification_signal_log()
    );
    let kinds = audit_kinds_of(&snapshot);
    assert!(
        kinds.contains(&"group_signal_skipped"),
        "the skip is audited: {snapshot:?}"
    );
    assert!(!kinds.contains(&"killpg_sent"), "{snapshot:?}");
    assert_eq!(snapshot.pumps_alive, 0);
    // The backgrounded grandchild survives the terminate (nothing was
    // signalled); the sentinel is untouched; the fds are back.
    let grandchild = wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await;
    assert!(
        process_alive(grandchild),
        "no group signal → the backgrounded grandchild survives"
    );
    assert!(process_alive(sentinel), "the unrelated sentinel survives");
    assert_eq!(count_open_fds(), fds_baseline, "read ends closed");
    kill_owned(grandchild);
    kill_owned(sentinel);
    let _ = std::fs::remove_dir_all(&ws);
}

/// C02 adversarial: the terminate hits the reaped-but-undrained window at
/// DIFFERENT offsets (varying the critical grace), and every leg lands on
/// the no-signal side. The identity-invalidation form that cannot be
/// built on a real host (recycled pid becoming a foreign group leader) is
/// covered by the send-time kernel-identity recheck (unit test
/// `group_signal_is_skipped_when_the_kernel_identity_disagrees` uses a
/// record whose pid provably belongs to nobody).
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c02_adversarial_window_offsets_never_signal_a_stale_group() {
    let _serial = serial().await;
    for (leg, grace_ms) in [(1u8, 200u64), (2, 700), (3, 1500)] {
        let ws = test_dir(&format!("c02adv{leg}-ws"));
        std::fs::create_dir_all(&ws).expect("ws");
        let pid_file = ws.join("grandchild.pid");
        let mut limits = limits_at(&format!("c02adv{leg}"));
        limits.stdio_grace = Duration::from_millis(grace_ms);
        let supervisor = supervisor_with(limits);
        let sentinel = spawn_sentinel(300);
        let spec = sh_spec(
            "c02adv",
            format!(
                "( sleep 8 ) & printf '%s\\n' \"$!\" > {}; echo DONE",
                pid_file.display()
            ),
            &ws,
        );
        let spawned = supervisor.spawn(spec).await.expect("spawn");
        // Hit the window as EARLY as it opens (the tightest race with the
        // reap publish); with a short grace this also lets some legs
        // MISS the window (AlreadyTerminal) — both sides must be safe.
        let maybe_window = wait_until("window or terminal", Duration::from_secs(5), || {
            let snap = supervisor.record(&spawned.id)?;
            // Return once the reap is published (the window is open) OR
            // the record is already terminal (the window was missed) —
            // both sides of the race must be safe.
            (snap.child_reaped || snap.phase.is_terminal()).then_some(snap)
        })
        .await;
        let receipt = supervisor
            .terminate(&spawned.id, TerminationReason::Close)
            .await;
        match receipt.outcome {
            TerminationOutcome::Terminated {
                fact,
                reclaimed: _,
                drained_stdio: _,
            } => assert_eq!(fact.status_code(), 0),
            TerminationOutcome::AlreadyTerminal(RecordPhase::Exited { fact }) => {
                assert_eq!(fact.status_code(), 0)
            }
            other => panic!("leg {leg}: safe receipt: {other:?}"),
        }
        // Whatever side of the race: NO signal left this supervisor.
        assert!(
            supervisor
                .verification_signal_log()
                .iter()
                .all(|s| s.target == SignalTarget::Child(0)),
            "leg {leg}: no group signal: {:?}",
            supervisor.verification_signal_log()
        );
        assert!(process_alive(sentinel), "leg {leg}: sentinel survives");
        let _ = maybe_window;
        let grandchild = wait_until("grandchild pid file", Duration::from_secs(5), || {
            std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|t| t.trim().parse::<i32>().ok())
        })
        .await;
        assert!(
            process_alive(grandchild),
            "leg {leg}: no group kill reached the grandchild"
        );
        kill_owned(grandchild);
        kill_owned(sentinel);
        let _ = std::fs::remove_dir_all(&ws);
    }
}

// ── R04-RR1-F02-C03: double-pump budget and deferred observation ────────────

/// C03 normal leg: BOTH pumps never end (a silent grandchild holds both
/// write ends) under a very small test grace — the whole chain (grace
/// timeout → abort → observed join → settle → shutdown) completes within
/// the one explicit budget and leaves nothing unqueryable behind.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c03_double_stuck_pumps_complete_within_the_single_budget() {
    let _serial = serial().await;
    let ws = test_dir("c03-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let grace = Duration::from_millis(80);
    let join_bound = Duration::from_millis(150);
    let mut limits = limits_at("c03");
    limits.stdio_grace = grace;
    limits.pump_abort_join = join_bound;
    let supervisor = supervisor_with(limits);
    let sentinel = spawn_sentinel(300);
    let fds_baseline = count_open_fds();
    let started = Instant::now();
    let spec = sh_spec(
        "c03",
        format!(
            "( sleep 30 ) & printf '%s\\n' \"$!\" > {}; echo PARENT_DONE",
            pid_file.display()
        ),
        &ws,
    );
    let spawned = supervisor.spawn(spec).await.expect("spawn");
    let phase = supervisor
        .wait_terminal(&spawned.id)
        .await
        .expect("terminal");
    let chain_elapsed = started.elapsed();
    assert!(phase.is_terminal());
    // The single explicit budget: one grace + both abort-joins, plus a
    // fixed process-spawn slack. Nothing multi-stage hides behind "done".
    let budget = grace + join_bound + join_bound + Duration::from_millis(400);
    assert!(
        chain_elapsed <= budget,
        "the whole chain fit the budget: {chain_elapsed:?} > {budget:?}"
    );
    assert!(chain_elapsed >= grace, "the grace was really waited");
    let snapshot = supervisor.record(&spawned.id).unwrap();
    assert_eq!(snapshot.pumps_alive, 0, "no task left unqueryable");
    assert!(!snapshot.drained_stdio);
    assert!(snapshot.reclaimed, "both aborts were observed by joining");
    let kinds = audit_kinds_of(&snapshot);
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == "output_task_closed_after_abort")
            .count(),
        2,
        "both stuck pumps: {snapshot:?}"
    );
    assert!(
        kinds.contains(&"output_drain_summary"),
        "the budget timeline is audited: {snapshot:?}"
    );
    // The deferred exit has an observer-slot ONLY while the record lives:
    // after the terminal phase there is nothing left to observe — the
    // collector is frozen and the fds are back at the baseline.
    let frozen = supervisor.output_snapshot(&spawned.id).unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let recheck = supervisor.output_snapshot(&spawned.id).unwrap();
    assert_eq!(recheck.total_bytes, frozen.total_bytes);
    assert_eq!(count_open_fds(), fds_baseline, "read ends closed");
    // Every supervisor-owned task (reaper + both pumps) has ended.
    tasks_settle_to(&supervisor, 0).await;
    // The shutdown leg of the chain: nothing live remains, bounded.
    let shutdown_started = Instant::now();
    let receipts = supervisor.shutdown_all().await;
    assert!(receipts.is_empty(), "{receipts:?}");
    assert!(shutdown_started.elapsed() < Duration::from_secs(1));
    assert!(process_alive(sentinel), "the sentinel survives");
    let grandchild = wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await;
    kill_owned(grandchild);
    kill_owned(sentinel);
    let _ = std::fs::remove_dir_all(&ws);
}

/// C03 adversarial leg 1: a pump PANICS (the verification fault exercises
/// the real panic disposition). The reaper must observe the end, record
/// the panic honestly, still finish within the budget and leave nothing.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c03_adversarial_panicking_pump_is_observed_and_honest() {
    let _serial = serial().await;
    let ws = test_dir("c03panic-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let grace = Duration::from_millis(120);
    let join_bound = Duration::from_millis(200);
    let mut limits = limits_at("c03panic");
    limits.stdio_grace = grace;
    limits.pump_abort_join = join_bound;
    let supervisor = supervisor_with(limits);
    supervisor.arm_pump_fault_for_verification(PumpFaultPoint::PanicNext);
    let fds_baseline = count_open_fds();
    let started = Instant::now();
    let spec = sh_spec("c03panic", "echo HI".to_string(), &ws);
    let spawned = supervisor.spawn(spec).await.expect("spawn");
    let phase = supervisor
        .wait_terminal(&spawned.id)
        .await
        .expect("terminal");
    assert!(phase.is_terminal());
    assert!(
        started.elapsed() <= grace + join_bound + Duration::from_millis(400),
        "a panicked pump does not stall the chain: {:?}",
        started.elapsed()
    );
    let snapshot = supervisor.record(&spawned.id).unwrap();
    let kinds = audit_kinds_of(&snapshot);
    assert!(
        kinds.contains(&"output_task_panicked"),
        "the panic disposition is audited: {snapshot:?}"
    );
    assert!(!snapshot.drained_stdio, "a panic is not a natural EOF");
    assert!(
        snapshot.reclaimed,
        "the panicked end was observed by joining"
    );
    assert_eq!(snapshot.pumps_alive, 0);
    tasks_settle_to(&supervisor, 0).await;
    assert_eq!(count_open_fds(), fds_baseline, "no fd leaked by the panic");
    let _ = std::fs::remove_dir_all(&ws);
}

/// C03 adversarial leg 2: natural exit and the cancel request arrive
/// SIMULTANEOUSLY (racing the reap boundary), under alternating tiny and
/// normal grace values. Every interleaving lands on a safe receipt, and
/// any group signal that ever fires is one whose ownership was proven at
/// send time.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c03_adversarial_exit_and_cancel_racing_never_signals_unprovably() {
    let _serial = serial().await;
    let ws = test_dir("c03race-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let sentinel = spawn_sentinel(300);
    for round in 0..15u32 {
        let mut limits = limits_at(&format!("c03race{round}"));
        limits.stdio_grace = if round % 2 == 0 {
            Duration::from_millis(30)
        } else {
            Duration::from_millis(250)
        };
        let supervisor = supervisor_with(limits);
        let spec = sh_spec("c03race", "echo RACE".to_string(), &ws);
        let spawned = supervisor.spawn(spec).await.expect("spawn");
        // Exit observation and the cancel request start together.
        let waiter = supervisor.wait_terminal(&spawned.id);
        let terminator = supervisor.terminate(&spawned.id, TerminationReason::CallerDropped);
        let (phase, receipt) = tokio::join!(waiter, terminator);
        let phase = phase.expect("the record settles");
        assert!(phase.is_terminal(), "round {round}: {phase:?}");
        match receipt.outcome {
            TerminationOutcome::Terminated { fact, .. } => {
                // Either side of the race legitimately wins: the natural
                // exit is Code(0); a terminate that won BEFORE the reap
                // fires the ownership-proven killpg and the OBSERVED exit
                // is Signal(9). Both are real observations — the contract
                // is that no UNPROVABLE signal ever fires.
                assert!(
                    matches!(fact, lingxi_service::procsupervisor::ExitFact::Code(0))
                        || matches!(fact, lingxi_service::procsupervisor::ExitFact::Signal(9)),
                    "round {round}: real observed exit: {fact:?}"
                );
            }
            TerminationOutcome::AlreadyTerminal(RecordPhase::Exited { fact }) => {
                assert_eq!(fact.status_code(), 0, "round {round}")
            }
            other => panic!("round {round}: safe receipt: {other:?}"),
        }
        // Any group signal that fired was ownership-proven at send time
        // (the only two strings the sender ever stamps).
        for signal in supervisor.verification_signal_log() {
            assert!(
                signal.ownership == "verified_at_send"
                    || signal.ownership.starts_with("spawn_rollback"),
                "round {round}: unprovably-owned signal: {signal:?}"
            );
        }
        assert!(supervisor.live_handles().is_empty(), "round {round}");
        tasks_settle_to(&supervisor, 0).await;
    }
    assert!(process_alive(sentinel), "the sentinel survives every race");
    kill_owned(sentinel);
    let _ = std::fs::remove_dir_all(&ws);
}

// ── R04-RR1-F02-C04: stress — nothing left behind ───────────────────────────

/// C04 (+ its adversarial eviction/deferred-exit form): the SAME process
/// runs many short-parent cycles whose grandchildren hold the pipes,
/// exceeding the settled-ring retention multiple times (terminal-record
/// eviction interleaves with the deferred exits). After every round the
/// supervisor's own objects are fully reclaimed; after all rounds the
/// declared steady state holds: no live records, no owned tasks, fds at
/// baseline, retention within the ring cap, sentinel untouched.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rr1_f02_c04_stress_cycles_return_to_the_declared_steady_state() {
    let _serial = serial().await;
    let ws = test_dir("c04-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    const RING_CAP: usize = 3;
    const ROUNDS: usize = 12;
    let mut limits = limits_at("c04");
    limits.settled_ring_cap = RING_CAP;
    limits.live_cap = 3;
    limits.stdio_grace = Duration::from_millis(120);
    let supervisor = supervisor_with(limits);
    let sentinel = spawn_sentinel(300);
    let fds_baseline = count_open_fds();
    let mut grandchildren: Vec<i32> = Vec::new();
    for round in 0..ROUNDS {
        // Every 4th round the holder WRITES late (dies of its own SIGPIPE
        // once the read end closed — the deferred-exit form); the others
        // hold silently and are disposed of by exact pid at the end.
        let holder_cmd = if round % 4 == 3 {
            "( sleep 0.5; echo LATE ) &".to_string()
        } else {
            "( sleep 3 ) &".to_string()
        };
        let pid_file = ws.join(format!("g{round}.pid"));
        let spec = sh_spec(
            "c04",
            format!(
                "{holder_cmd} printf '%s\\n' \"$!\" > {}; echo ROUND_{round}",
                pid_file.display()
            ),
            &ws,
        );
        let spawned = supervisor.spawn(spec).await.expect("spawn");
        let phase = supervisor
            .wait_terminal(&spawned.id)
            .await
            .expect("terminal");
        assert!(phase.is_terminal(), "round {round}");
        // Per-round reclamation: pumps observed, record settled, tasks
        // gone, fds back — DURING the ring churn.
        let snapshot = wait_until("round pumps closed", Duration::from_secs(2), || {
            let snap = supervisor.record(&spawned.id)?;
            (snap.pumps_alive == 0).then_some(snap)
        })
        .await;
        assert!(snapshot.reclaimed, "round {round}: {snapshot:?}");
        assert_eq!(
            supervisor.live_handles().len(),
            0,
            "round {round}: nothing live"
        );
        assert_eq!(
            count_open_fds(),
            fds_baseline,
            "round {round}: fd table at baseline"
        );
        tasks_settle_to(&supervisor, 0).await;
        if let Ok(text) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = text.trim().parse::<i32>() {
                grandchildren.push(pid);
            }
        }
    }
    // The declared steady state after exceeding the retention ring many
    // times over: bounded retention, no tasks, no fds, sentinel alive.
    assert!(
        supervisor.retained_record_count() <= RING_CAP,
        "settled ring bounds retention: {}",
        supervisor.retained_record_count()
    );
    assert_eq!(supervisor.owned_task_count(), 0, "no owned task leaked");
    assert_eq!(count_open_fds(), fds_baseline);
    assert!(process_alive(sentinel), "no unrelated process was killed");
    for pid in grandchildren {
        kill_owned(pid); // exact-pid disposal (no-op for SIGPIPE dead)
    }
    kill_owned(sentinel);
    let _ = std::fs::remove_dir_all(&ws);
}
