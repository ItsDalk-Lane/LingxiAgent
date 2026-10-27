//! R04-RR1-F01 acceptance: the live-process registry capacity is enforced
//! BEFORE any OS dispatch, and every spawn failure path returns its slot
//! and reclaims what it created.
//!
//! Everything here runs the REAL native process chain: the REAL
//! `ToolInvocationGateway` (prepare → execute_prepared), the REAL
//! `ProcessTools::run_exec_command` / `run_write_stdin` executors
//! registered through `register_process_tools`, and the REAL
//! `ProcessSupervisor` (POSIX process groups, `killpg`, bounded cleanup).
//!
//! Capacity separation (F01 repair requirement 4): the binding capacity in
//! every test below is the ProcessSupervisor's `live_cap`
//! (`SpawnFailure::RegistryFull`), NOT the gateway's prepared-invocation
//! cap (`GatewayRefusal::PreparedRegistryFull` /
//! `DEFAULT_LIVE_PREPARED_CAP` = 1024, left far above `live_cap`). The
//! assertions on the refusal vocabulary check the supervisor's
//! process-capacity wording and explicitly reject the gateway wording.
//!
//! Test-double boundary: no model/provider doubles are needed — the calls
//! go straight through the gateway's user-run surface. The sentinel
//! processes (`/bin/sleep`) are unrelated test-created processes killed
//! by exact pid at teardown. All test state lives in unique
//! `std::env::temp_dir()` subdirectories (never a shared /tmp wildcard).

use std::path::{Path, PathBuf};
use std::pin::Pin;
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
    ProcessHandleId, ProcessSupervisor, SpawnFaultPoint, SupervisorLimits, TerminationOutcome,
    TerminationReason,
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

// ── harness (real gateway + real process tools, no ServiceState needed) ─────

struct CapHarness {
    #[allow(dead_code)]
    registry: Arc<ToolRegistry>,
    gateway: Arc<ToolInvocationGateway>,
    supervisor: Arc<ProcessSupervisor>,
    exec_target: ToolTargetId,
    write_stdin_target: ToolTargetId,
    root: PathBuf,
    ws: PathBuf,
    spill_dir: PathBuf,
    /// Unrelated sentinel processes created by THIS test (killed by exact
    /// pid at teardown — never a name-based pkill, never a shared dir).
    sentinels: std::sync::Mutex<Vec<i32>>,
}

impl CapHarness {
    fn register_sentinel(&self, pid: i32) {
        self.sentinels.lock().unwrap().push(pid);
    }

    /// The number of files currently in the supervisor-owned spill dir —
    /// a refused spawn must not leave even a temporary spill file behind.
    fn spill_file_count(&self) -> usize {
        std::fs::read_dir(&self.spill_dir)
            .map(|d| d.count())
            .unwrap_or(0)
    }
}

async fn cap_teardown(h: &CapHarness) {
    for pid in h.sentinels.lock().unwrap().drain(..) {
        // SAFETY: signal a process this test spawned itself.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    let _ = h.supervisor.shutdown_all().await;
    let _ = std::fs::remove_dir_all(&h.root);
}

/// A unique test directory (pid + nanos + sequence — no cross-test
/// collisions, never the shared /tmp itself).
fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r04rr1f01-{tag}-{}-{}-{}",
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

/// Builds the REAL chain with the ProcessSupervisor's `live_cap` as the
/// binding capacity: registry → gateway (prepared cap 1024) →
/// `register_process_tools` → ProcessSupervisor(live_cap).
fn cap_harness(live_cap: usize) -> CapHarness {
    let root = test_dir("root");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("workspace");
    let spill_dir = test_dir("spill");
    let mut limits = SupervisorLimits::with_spill_dir(spill_dir.clone());
    limits.live_cap = live_cap;
    let registry = Arc::new(ToolRegistry::new());
    let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let approvals = Arc::new(ApprovalService::new(Arc::new(SystemClock)));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn ToolPolicyPort>,
        Arc::new(SystemClock),
        budget(),
        DEFAULT_PREPARED_TTL_MS,
        // Deliberately far above live_cap: the gateway prepared cap is a
        // DIFFERENT capacity and must never be the one that fires here.
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
    CapHarness {
        registry,
        gateway,
        supervisor,
        exec_target: core.exec_target,
        write_stdin_target: core.write_stdin_target,
        root,
        ws,
        spill_dir,
        sentinels: std::sync::Mutex::new(Vec::new()),
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

/// Direct gateway call (prepare → execute_prepared) — the same two steps
/// the run driver performs, minus the driving loop.
async fn call_exec(
    h: &CapHarness,
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
    h: &CapHarness,
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
                lingxi_protocol::ContentBlock::Text { text } => Some(text.clone()),
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

fn status_of(result: &ToolExecutionResult) -> Option<ToolRunStatus> {
    match &result.outcome {
        ToolOutcome::Success { result } => result.status.as_deref().cloned(),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

// ── real-OS probes ──────────────────────────────────────────────────────────

fn process_alive(pid: i32) -> bool {
    // SAFETY: signal-0 liveness probe.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Spawns an UNRELATED sentinel (`/bin/sleep`) owned by the test.
fn spawn_sentinel(h: &CapHarness, seconds: u32) -> i32 {
    let child = std::process::Command::new("/bin/sleep")
        .arg(seconds.to_string())
        .spawn()
        .expect("sentinel spawns");
    let pid = child.id() as i32;
    h.register_sentinel(pid);
    // The handle is deliberately leaked: the test manages sentinel liveness
    // by exact pid and kills it at teardown.
    std::mem::forget(child);
    pid
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

fn sha256_of(path: &Path) -> String {
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

/// The number of open file descriptors of THIS process (macOS/Linux
/// `/dev/fd`) — the zero-new-FD probe for refused spawns.
fn fd_count() -> usize {
    std::fs::read_dir("/dev/fd")
        .map(|d| d.count())
        .unwrap_or(usize::MAX)
}

/// A test-owned helper script. Every real OS dispatch of itself records
/// one line into `counter` and overwrites `sentinel` (the durable
/// dispatch / side-effect proofs); with argument `park` it additionally
/// execs `/bin/sleep 300` (a lingering, findable orphan if a "refused"
/// spawn really ran — the C01/C04 zero-dispatch probe), otherwise it
/// exits 0 immediately (a racing winner in C02 resolves fast instead of
/// parking the racer's call for the full sleep).
fn write_dispatch_helper(dir: &Path, counter: &Path, sentinel: &Path) -> PathBuf {
    let helper = dir.join("dispatch-helper.sh");
    std::fs::write(
        &helper,
        format!(
            "#!/bin/sh\nprintf 'dispatch\\n' >> {0}\nprintf RAN > {1}\nif [ \"$1\" = park ]; \
             then exec /bin/sleep 300; fi\nexit 0\n",
            counter.display(),
            sentinel.display()
        ),
    )
    .expect("write helper");
    make_executable(&helper);
    helper
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .expect("helper metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("chmod helper");
}

/// Dispatch count recorded by the helper (0 when it never ran).
fn dispatch_count(counter: &Path) -> usize {
    std::fs::read_to_string(counter)
        .map(|text| text.lines().filter(|l| l.trim() == "dispatch").count())
        .unwrap_or(0)
}

type ExecOutcome = Result<ToolExecutionResult, lingxi_service::toolgateway::GatewayRefusal>;
type ExecFut<'a> = Pin<Box<dyn std::future::Future<Output = ExecOutcome> + Send + 'a>>;

/// Serializes THIS FILE's tests: the fd-table probes (`fd_count`) are
/// process-global observations, so sibling tests running in parallel
/// (libtest's default) would churn the very table a zero-new-FD
/// assertion reads. One guard per test keeps every probe honest without
/// weakening any other assertion. A POISONED lock (a sibling test
/// panicked while holding it) is recovered, not spun on — one failing
/// test must not brick the remaining ones.
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

/// A barrier-synchronized competitor: waits at the barrier so both (all)
/// racers arrive at the spawn boundary together (a FIXED interleaving,
/// not a random retry), then performs the real prepare → execute chain.
/// Owns its state so it can run as a `tokio` task.
async fn barrier_competitor(
    gateway: Arc<ToolInvocationGateway>,
    target: ToolTargetId,
    ctx: RunContext,
    call: String,
    args: serde_json::Value,
    barrier: Arc<tokio::sync::Barrier>,
) -> ExecOutcome {
    barrier.wait().await;
    let request =
        ToolRequest::from_effective_arguments(target.as_str(), args, &budget()).expect("request");
    let call_id = ToolCallId::new(call);
    let prepared = gateway.prepare_from_request(
        &ctx,
        CallerSurface::UserRun,
        "agent",
        user_operate(),
        &call_id,
        &request,
    )?;
    gateway
        .execute_prepared(&ctx, &call_id, &prepared.handle)
        .await
}

/// Terminates through the real bounded chain and waits for the settle.
async fn terminate_and_settle(h: &CapHarness, id: &ProcessHandleId, reason: TerminationReason) {
    let receipt = h.supervisor.terminate(id, reason).await;
    assert!(
        matches!(receipt.outcome, TerminationOutcome::Terminated { .. }),
        "bounded termination: {:?}",
        receipt.outcome
    );
    wait_until("settle", Duration::from_secs(10), || {
        (!h.supervisor.live_handles().contains(id)).then_some(())
    })
    .await;
}

/// Starts one long-running one-shot through the REAL tool chain and waits
/// until it is registered (the live record exists). Returns the still
/// in-flight call future and the live handle id.
async fn start_long_oneshot<'a>(
    h: &'a CapHarness,
    ctx: &'a RunContext,
    call: &'a str,
) -> (ExecFut<'a>, ProcessHandleId) {
    let mut fut = Box::pin(call_exec(
        h,
        ctx,
        call,
        json!({"argv": ["/bin/sleep", "300"], "timeout_seconds": 600}),
    ));
    let id = loop {
        tokio::select! {
            settled = &mut fut => {
                panic!("the long one-shot settled before registration: {:?}",
                    settled.map(|_| ()))
            }
            _ = tokio::time::sleep(Duration::from_millis(15)) => {
                if let Some(id) = h.supervisor.live_handles().first().cloned() {
                    break id;
                }
            }
        }
    };
    (fut, id)
}

// ── the red reproduction (R04-RR1-F01, minimal form) ────────────────────────

/// Minimal real-runtime reproduction of the finding: with `live_cap = 1`
/// and the only slot held by a running managed process, a second one-shot
/// that writes an isolated sentinel file is REFUSED with the registry-full
/// vocabulary — and on the unfixed baseline the sentinel is ACTUALLY
/// OVERWRITTEN (the command really executed), which is the defect: the
/// refusal path dispatched a process nobody owns. This test uses only
/// baseline-stable vocabulary so the reproduction is a runtime red, not a
/// compile error.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn repro_rr1_f01_registry_full_refusal_must_not_execute_the_command() {
    let _serial = serial().await;
    let h = cap_harness(1);
    let ctx = kernel_ctx("sess_rr1f01_red", "run_red");
    let unrelated = spawn_sentinel(&h, 300);

    // The first controlled process holds the only live slot.
    let (first_fut, first_id) = start_long_oneshot(&h, &ctx, "call-red-first").await;
    assert_eq!(h.supervisor.live_handles().len(), 1);

    // An isolated sentinel with known content; the second command would
    // overwrite it if it executed.
    let sentinel = h.ws.join("red-sentinel.txt");
    std::fs::write(&sentinel, b"BASELINE").expect("write sentinel");
    let before = sha256_of(&sentinel);

    let second = call_exec(
        &h,
        &ctx,
        "call-red-second",
        json!({
            "argv": ["/bin/bash", "-c", format!("printf RAN > {}", sentinel.display())]
        }),
    )
    .await
    .expect("the second call resolves (as a refusal)");

    // The refusal is a capacity refusal of the ProcessSupervisor.
    let err = error_text(&second);
    assert!(
        err.contains("registry is full"),
        "expected the supervisor capacity vocabulary: {err}"
    );

    // ZERO EXECUTION: the sentinel must remain unchanged through a bounded
    // observation window (the dispatched-but-unregistered child of the
    // unfixed baseline writes within milliseconds — an instantaneous read
    // could race it; a window cannot). This is the assertion that is RED
    // on the unfixed baseline: the command really ran and overwrote the
    // file before/around the registry check.
    let window = Duration::from_millis(1000);
    let end = Instant::now() + window;
    while Instant::now() < end {
        assert_eq!(
            sha256_of(&sentinel),
            before,
            "R04-RR1-F01: the refused one-shot must not have executed"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        std::fs::read(&sentinel).unwrap(),
        b"BASELINE",
        "the sentinel content itself is the durable proof"
    );

    // The first process is unaffected and still the only live record.
    assert_eq!(h.supervisor.live_handles().len(), 1);
    assert!(
        process_alive(h.supervisor.record(&first_id).unwrap().pid),
        "the first process keeps running"
    );
    assert!(process_alive(unrelated), "the unrelated sentinel survives");

    // Cleanup through the real bounded chain.
    let receipt = h
        .supervisor
        .terminate(&first_id, TerminationReason::Close)
        .await;
    assert!(matches!(
        receipt.outcome,
        TerminationOutcome::Terminated { .. }
    ));
    let _ = first_fut.await.expect("the first call settles");
    cap_teardown(&h).await;
}

// ── R04-RR1-F01-C01: registry full ⇒ zero dispatch (full evidence) ─────────

/// The full C01 case: `live_cap = 1`, the first controlled process
/// running, the second one-shot able to write an isolated sentinel —
/// issued through the REAL tool chain. The refusal must be the
/// ProcessSupervisor's capacity vocabulary (distinguished from both the
/// plain EXEC_SPAWN_FAILED and the gateway's PreparedRegistryFull), and
/// zero execution must be proven against the real registry, the real OS
/// dispatch counter, the file digests, the fd table and the spill dir —
/// not just the returned error.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn rr1_f01_c01_registry_full_refuses_with_zero_dispatch() {
    let _serial = serial().await;
    let h = cap_harness(1);
    let ctx = kernel_ctx("sess_rr1f01_c01", "run_c01");
    let unrelated = spawn_sentinel(&h, 300);

    // The first controlled process holds the only live slot.
    let (first_fut, first_id) = start_long_oneshot(&h, &ctx, "call-c01-first").await;
    let first_pid = h.supervisor.record(&first_id).unwrap().pid;

    // Isolated sentinel + dispatch-counting helper (test-owned).
    let sentinel = h.ws.join("c01-sentinel.txt");
    std::fs::write(&sentinel, b"BASELINE").expect("write sentinel");
    let before = sha256_of(&sentinel);
    let counter = h.ws.join("c01-dispatches.txt");
    let helper = write_dispatch_helper(&h.ws, &counter, &sentinel);

    // Baseline evidence snapshots.
    let fds_before = fd_count();
    let spills_before = h.spill_file_count();
    assert_eq!(dispatch_count(&counter), 0, "nothing dispatched yet");

    let second = call_exec(
        &h,
        &ctx,
        "call-c01-second",
        json!({ "argv": [helper, "park"] }),
    )
    .await
    .expect("the second call resolves (as a refusal)");

    // The refusal is the ProcessSupervisor capacity, distinctly coded:
    // its own vocabulary, neither the generic spawn failure nor the
    // gateway prepared-registry wording (a different capacity).
    let err = error_text(&second);
    assert!(
        err.contains("EXEC_PROCESS_REGISTRY_FULL"),
        "supervisor capacity vocabulary: {err}"
    );
    assert!(
        err.contains("registry is full"),
        "the underlying refusal text: {err}"
    );
    assert!(
        !err.contains("EXEC_SPAWN_FAILED"),
        "a capacity refusal is not a spawn failure: {err}"
    );
    assert!(
        !err.contains("gateway_prepared_registry_full")
            && !err.contains("prepared-invocation registry"),
        "this is NOT the gateway prepared cap: {err}"
    );

    // ZERO EXECUTION through a bounded observation window — checked
    // against the OS dispatch counter and the sentinel digest, not just
    // the returned Err.
    let window = Duration::from_millis(1000);
    let end = Instant::now() + window;
    while Instant::now() < end {
        assert_eq!(sha256_of(&sentinel), before, "sentinel untouched");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(dispatch_count(&counter), 0, "zero real OS dispatches");
    assert_eq!(
        std::fs::read(&sentinel).unwrap(),
        b"BASELINE",
        "the durable side-effect proof"
    );

    // No new FDs and no temporary spill file were left behind.
    assert_eq!(
        fd_count(),
        fds_before,
        "a refused spawn opens no fd (reservation is userspace-only)"
    );
    assert_eq!(
        h.spill_file_count(),
        spills_before,
        "a refused spawn creates no spill file"
    );

    // The real registry still holds exactly the first process, healthy.
    let live = h.supervisor.live_handles();
    assert_eq!(live.len(), 1, "registry snapshot: {live:?}");
    assert_eq!(live[0], first_id);
    assert!(process_alive(first_pid), "the first process is unaffected");
    assert!(process_alive(unrelated), "the unrelated sentinel survives");

    // Cleanup through the real bounded chain.
    terminate_and_settle(&h, &first_id, TerminationReason::Close).await;
    let _ = first_fut.await.expect("the first call settles");
    assert!(!process_alive(first_pid), "no orphan of the first process");
    cap_teardown(&h).await;
}

// ── R04-RR1-F01-C02: barrier race for the last slot ────────────────────────

/// One barrier leg: `live_cap = 2` with one slot already occupied, two
/// competitors released simultaneously by a tokio Barrier. Exactly one
/// may dispatch (the winner), the loser is a zero-dispatch capacity
/// refusal, and the accounting survives rotation (the slot is usable
/// again after the winner settles — no leak). The competitors run the
/// dispatch-counting helper so the real OS dispatch count is provable.
/// A PTY competitor runs the helper with `park` (it execs `/bin/sleep
/// 300` after recording its dispatch receipt): a PTY tool call returns
/// "running" the moment the terminal starts — the helper's receipt and
/// the terminal's continued liveness are ASYNCHRONOUS facts, so the
/// leg parks the PTY winner (deterministic "status: running") and
/// awaits the receipt with a bounded wait instead of reading it
/// synchronously. A one-shot competitor runs the quick-exit form (its
/// Success already means the helper ran to completion).
async fn run_barrier_leg(
    h: &CapHarness,
    ctx: &RunContext,
    tag: &str,
    a_tty: bool,
    b_tty: bool,
    counter: &Path,
    helper: &Path,
) {
    let before_dispatches = dispatch_count(counter);
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mk_args = |tty: bool| {
        let mut args = json!({"argv": [helper], "timeout_seconds": 600});
        if tty {
            args["argv"] = json!([helper, "park"]);
            args["tty"] = json!(true);
        }
        args
    };
    let a = tokio::spawn(barrier_competitor(
        Arc::clone(&h.gateway),
        h.exec_target.clone(),
        ctx.clone(),
        format!("call-{tag}-a"),
        mk_args(a_tty),
        Arc::clone(&barrier),
    ));
    let b = tokio::spawn(barrier_competitor(
        Arc::clone(&h.gateway),
        h.exec_target.clone(),
        ctx.clone(),
        format!("call-{tag}-b"),
        mk_args(b_tty),
        Arc::clone(&barrier),
    ));
    let a = a.await.expect("racer a settles");
    let b = b.await.expect("racer b settles");

    // Exactly one success (≤ the remaining capacity of 1), and the
    // loser is a zero-dispatch capacity refusal.
    let mut winner: Option<ToolExecutionResult> = None;
    let mut refusals = 0usize;
    for outcome in [a, b] {
        let result = outcome.expect("the racer resolves");
        match &result.outcome {
            ToolOutcome::Success { .. } => {
                assert!(winner.is_none(), "at most ONE success across the barrier");
                winner = Some(result);
            }
            ToolOutcome::Failed { error } => {
                assert!(
                    error.message.contains("EXEC_PROCESS_REGISTRY_FULL"),
                    "the loser is a capacity refusal: {}",
                    error.message
                );
                refusals += 1;
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    let winner = winner.expect("the reservation is not all-or-nothing: one racer wins");
    assert_eq!(refusals, 1, "exactly one refusal");
    // Exactly ONE real dispatch across the race (the winner's). For a
    // PTY winner the receipt lands asynchronously after the Running
    // return — a BOUNDED wait for the helper's counter line, never a
    // sleep-and-hope (and never a synchronous read that races it).
    let expected = before_dispatches + 1;
    wait_until(
        "the winner's dispatch receipt",
        Duration::from_secs(10),
        || (dispatch_count(counter) == expected).then_some(()),
    )
    .await;
    assert_eq!(
        dispatch_count(counter),
        expected,
        "exactly ONE real dispatch across the race (the winner's)"
    );

    // The winner is dispatched and managed. A one-shot winner of the
    // quick-exit helper has ALREADY exited (Exited 0) and settled; a PTY
    // winner is a live interactive terminal (the legal control: a poll
    // round-trip), and gets terminated here.
    let mut winner_handle: Option<ProcessHandleId> = None;
    let winner_pid: Option<i32>;
    match status_of(&winner) {
        Some(ToolRunStatus::Exited { code }) => {
            assert_eq!(code, 0, "the one-shot winner ran the helper to completion");
            winner_pid = None;
        }
        Some(ToolRunStatus::Running { handle }) => {
            let handle = ProcessHandleId::parse(&handle).expect("the winner exposes a handle");
            let snapshot = h.supervisor.record(&handle).expect("winner registered");
            assert_eq!(
                snapshot.kind.wire_name(),
                "persistent_terminal",
                "a running winner is a pty terminal"
            );
            let poll = call_write_stdin(
                h,
                ctx,
                &format!("call-{tag}-poll"),
                json!({"process_id": handle.as_str()}),
            )
            .await
            .expect("pty poll");
            assert!(
                text_of(&poll).contains("status: running"),
                "the PTY winner really works: {}",
                text_of(&poll)
            );
            winner_pid = Some(snapshot.pid);
            winner_handle = Some(handle);
        }
        other => panic!("the winner reports its real state: {other:?}"),
    }
    assert!(
        h.supervisor.live_handles().len() <= 2,
        "never above the cap"
    );

    // Rotation (no count leak): after the winner settles, the freed slot
    // is usable again — repeatedly.
    if let Some(handle) = winner_handle.take() {
        terminate_and_settle(h, &handle, TerminationReason::Close).await;
    } else {
        // The one-shot winner must have settled already (its reaper ran);
        // the registry is back to the single occupant.
        wait_until("one-shot winner settles", Duration::from_secs(10), || {
            (h.supervisor.live_handles().len() == 1).then_some(())
        })
        .await;
    }
    if let Some(pid) = winner_pid {
        assert!(!process_alive(pid), "no orphaned winner");
    }
    for round in 0..3 {
        let refill = call_exec(
            h,
            ctx,
            &format!("call-{tag}-refill-{round}"),
            json!({"argv": ["/bin/sleep", "300"], "tty": true}),
        )
        .await
        .expect("the freed slot is reusable (no leak)");
        let refill_handle = match status_of(&refill) {
            Some(ToolRunStatus::Running { handle }) => {
                ProcessHandleId::parse(&handle).expect("refill handle")
            }
            other => panic!("refill running: {other:?}"),
        };
        terminate_and_settle(h, &refill_handle, TerminationReason::Close).await;
    }
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn rr1_f01_c02_two_one_shots_race_the_last_slot_through_a_barrier() {
    let _serial = serial().await;
    let h = cap_harness(2);
    let ctx = kernel_ctx("sess_rr1f01_c02a", "run_c02a");
    let (first_fut, first_id) = start_long_oneshot(&h, &ctx, "call-c02a-occupy").await;
    let counter = h.ws.join("c02a-dispatches.txt");
    std::fs::write(&counter, b"").expect("counter base");
    let sentinel = h.ws.join("c02a-sentinel.txt");
    std::fs::write(&sentinel, b"BASELINE").expect("sentinel base");
    let helper = write_dispatch_helper(&h.ws, &counter, &sentinel);
    run_barrier_leg(&h, &ctx, "c02a", false, false, &counter, &helper).await;
    terminate_and_settle(&h, &first_id, TerminationReason::Close).await;
    let _ = first_fut.await.expect("the occupant settles");
    cap_teardown(&h).await;
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn rr1_f01_c02_two_ptys_race_the_last_slot_through_a_barrier() {
    let _serial = serial().await;
    let h = cap_harness(2);
    let ctx = kernel_ctx("sess_rr1f01_c02b", "run_c02b");
    let (first_fut, first_id) = start_long_oneshot(&h, &ctx, "call-c02b-occupy").await;
    let counter = h.ws.join("c02b-dispatches.txt");
    std::fs::write(&counter, b"").expect("counter base");
    let sentinel = h.ws.join("c02b-sentinel.txt");
    std::fs::write(&sentinel, b"BASELINE").expect("sentinel base");
    let helper = write_dispatch_helper(&h.ws, &counter, &sentinel);
    run_barrier_leg(&h, &ctx, "c02b", true, true, &counter, &helper).await;
    terminate_and_settle(&h, &first_id, TerminationReason::Close).await;
    let _ = first_fut.await.expect("the occupant settles");
    cap_teardown(&h).await;
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn rr1_f01_c02_one_shot_and_pty_race_the_last_slot_through_a_barrier() {
    let _serial = serial().await;
    let h = cap_harness(2);
    let ctx = kernel_ctx("sess_rr1f01_c02c", "run_c02c");
    let (first_fut, first_id) = start_long_oneshot(&h, &ctx, "call-c02c-occupy").await;
    let counter = h.ws.join("c02c-dispatches.txt");
    std::fs::write(&counter, b"").expect("counter base");
    let sentinel = h.ws.join("c02c-sentinel.txt");
    std::fs::write(&sentinel, b"BASELINE").expect("sentinel base");
    let helper = write_dispatch_helper(&h.ws, &counter, &sentinel);
    run_barrier_leg(&h, &ctx, "c02c", false, true, &counter, &helper).await;
    terminate_and_settle(&h, &first_id, TerminationReason::Close).await;
    let _ = first_fut.await.expect("the occupant settles");
    cap_teardown(&h).await;
}

// ── R04-RR1-F01-C03: post-dispatch failure compensation ────────────────────

/// Extracts the pid from a dispatched-rollback error ("pid N").
fn pid_from_dispatched_fact(err: &str) -> i32 {
    err.split("pid ")
        .nth(1)
        .and_then(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<i32>()
                .ok()
        })
        .unwrap_or_else(|| panic!("the dispatched fact carries the pid: {err}"))
}

/// The dispatched-then-failed boundary (verification fault at the
/// process-group verification, one-shot kind): the rollback is a real
/// bounded kill + reap of the REALLY dispatched child, the error keeps
/// the dispatched fact, and the slot is returned EXACTLY once — proven
/// by capacity arithmetic (a follow-up spawn succeeds, the one after it
/// is refused), repeated across several rounds (the "rollback fails
/// again" adversarial: repeated compensations never drift the count).
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn rr1_f01_c03_one_shot_group_verify_failure_compensates_exactly_once() {
    let _serial = serial().await;
    let h = cap_harness(2);
    let ctx = kernel_ctx("sess_rr1f01_c03a", "run_c03a");
    let (first_fut, first_id) = start_long_oneshot(&h, &ctx, "call-c03a-first").await;
    let first_pid = h.supervisor.record(&first_id).unwrap().pid;

    for round in 0..3 {
        let fds_before = fd_count();
        h.supervisor
            .arm_spawn_fault_for_verification(SpawnFaultPoint::GroupVerify);
        let failed = call_exec(
            &h,
            &ctx,
            &format!("call-c03a-fault-{round}"),
            json!({"argv": ["/bin/sleep", "300"], "timeout_seconds": 600}),
        )
        .await
        .expect("the faulty call resolves");
        let err = error_text(&failed);
        assert!(
            err.contains("EXEC_SPAWN_FAILED") && err.contains("process-group ownership"),
            "the real failure surface: {err}"
        );
        assert!(
            err.contains("dispatch DID happen"),
            "the dispatched fact is preserved: {err}"
        );
        // The dispatched child was really killed AND reaped before the
        // error returned (bounded, observable).
        let pid = pid_from_dispatched_fact(&err);
        assert!(
            !process_alive(pid),
            "the dispatched child (pid {pid}) is reaped, round {round}"
        );
        // The failed spawn is NOT registered and leaves no fd residue.
        let live = h.supervisor.live_handles();
        assert_eq!(live.len(), 1, "only the first process remains: {live:?}");
        assert_eq!(live[0], first_id);
        assert_eq!(fd_count(), fds_before, "pipes of the rollback are closed");
    }

    // Exactly-once release, proven by capacity arithmetic: with the
    // first process still occupying one of the two slots, the released
    // slot admits exactly ONE more spawn — a zero-release bug refuses
    // it, a double-release bug admits two.
    let refill = call_exec(
        &h,
        &ctx,
        "call-c03a-refill",
        json!({"argv": ["/bin/sleep", "300"], "tty": true}),
    )
    .await
    .expect("the released slot is usable exactly once (not zero)");
    let refill_handle = match status_of(&refill) {
        Some(ToolRunStatus::Running { handle }) => {
            ProcessHandleId::parse(&handle).expect("refill handle")
        }
        other => panic!("refill running: {other:?}"),
    };
    let over = call_exec(
        &h,
        &ctx,
        "call-c03a-over",
        json!({"argv": ["/bin/sleep", "300"], "timeout_seconds": 600}),
    )
    .await
    .expect("the over-capacity call resolves");
    assert!(
        error_text(&over).contains("EXEC_PROCESS_REGISTRY_FULL"),
        "no double release: {}",
        error_text(&over)
    );

    terminate_and_settle(&h, &refill_handle, TerminationReason::Close).await;
    terminate_and_settle(&h, &first_id, TerminationReason::Close).await;
    let _ = first_fut.await.expect("the first call settles");
    assert!(!process_alive(first_pid));
    cap_teardown(&h).await;
}

/// The PTY twin of the group-verification failure: the master/slave fds
/// the spawn really created are closed by the rollback (fd-count proof),
/// the dispatched child is reaped, the dispatched fact is kept, and the
/// slot returns exactly once.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn rr1_f01_c03_pty_group_verify_failure_closes_the_pty_and_returns_the_slot() {
    let _serial = serial().await;
    let h = cap_harness(2);
    let ctx = kernel_ctx("sess_rr1f01_c03b", "run_c03b");
    let (first_fut, first_id) = start_long_oneshot(&h, &ctx, "call-c03b-first").await;

    let fds_before = fd_count();
    h.supervisor
        .arm_spawn_fault_for_verification(SpawnFaultPoint::GroupVerify);
    let failed = call_exec(
        &h,
        &ctx,
        "call-c03b-fault",
        json!({"argv": ["/bin/sleep", "300"], "tty": true}),
    )
    .await
    .expect("the faulty pty call resolves");
    let err = error_text(&failed);
    assert!(
        err.contains("EXEC_SPAWN_FAILED") && err.contains("process-group ownership"),
        "the real failure surface: {err}"
    );
    assert!(
        err.contains("dispatch DID happen"),
        "the dispatched fact is preserved: {err}"
    );
    let pid = pid_from_dispatched_fact(&err);
    assert!(!process_alive(pid), "the dispatched pty child is reaped");
    // The pty pair the spawn opened is closed by the rollback — no fd
    // residue, no reliance on "closing the pty signals the child".
    assert_eq!(
        fd_count(),
        fds_before,
        "the master/slave fds of the failed pty spawn are closed"
    );
    // The failed pty spawn is not registered; the slot returned exactly
    // once (one refill admitted, the next refused).
    let live = h.supervisor.live_handles();
    assert_eq!(live.len(), 1, "only the first process: {live:?}");
    let refill = call_exec(
        &h,
        &ctx,
        "call-c03b-refill",
        json!({"argv": ["/bin/sleep", "300"], "tty": true}),
    )
    .await
    .expect("the slot is usable exactly once");
    let refill_handle = match status_of(&refill) {
        Some(ToolRunStatus::Running { handle }) => {
            ProcessHandleId::parse(&handle).expect("refill handle")
        }
        other => panic!("refill running: {other:?}"),
    };
    let over = call_exec(
        &h,
        &ctx,
        "call-c03b-over",
        json!({"argv": ["/bin/sleep", "300"], "timeout_seconds": 600}),
    )
    .await
    .expect("the over-capacity call resolves");
    assert!(
        error_text(&over).contains("EXEC_PROCESS_REGISTRY_FULL"),
        "no double release: {}",
        error_text(&over)
    );

    terminate_and_settle(&h, &refill_handle, TerminationReason::Close).await;
    terminate_and_settle(&h, &first_id, TerminationReason::Close).await;
    let _ = first_fut.await.expect("the first call settles");
    cap_teardown(&h).await;
}

/// The pre-dispatch PTY initialization failure (verification fault at
/// `open_pty_pair`): ZERO dispatch, zero fd residue, and the slot
/// returns exactly once (the very next spawn is admitted under a
/// live_cap of 1).
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn rr1_f01_c03_pty_open_failure_is_zero_dispatch_and_returns_the_slot() {
    let _serial = serial().await;
    let h = cap_harness(1);
    let ctx = kernel_ctx("sess_rr1f01_c03c", "run_c03c");
    let fds_before = fd_count();
    h.supervisor
        .arm_spawn_fault_for_verification(SpawnFaultPoint::PtyPairOpen);
    let failed = call_exec(
        &h,
        &ctx,
        "call-c03c-fault",
        json!({"argv": ["/bin/sleep", "300"], "tty": true}),
    )
    .await
    .expect("the faulty pty call resolves");
    let err = error_text(&failed);
    assert!(
        err.contains("EXEC_SPAWN_FAILED"),
        "the failure surface: {err}"
    );
    assert!(
        err.contains("zero dispatch"),
        "the error says honestly that nothing was dispatched: {err}"
    );
    assert!(h.supervisor.live_handles().is_empty(), "nothing registered");
    assert_eq!(fd_count(), fds_before, "no pty fd was left open");
    // Exactly-once release under live_cap=1: the next spawn succeeds.
    let ok = call_exec(
        &h,
        &ctx,
        "call-c03c-ok",
        json!({"argv": ["/bin/sleep", "1"]}),
    )
    .await
    .expect("the released slot is immediately reusable");
    match status_of(&ok) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 0),
        other => panic!("legal control exits cleanly: {other:?}"),
    }
    assert!(h.supervisor.live_handles().is_empty());
    cap_teardown(&h).await;
}

// ── R04-RR1-F01-C04: same-process reuse after refusals and releases ────────

/// More than `live_cap` rounds of refusal + release on the SAME
/// supervisor, then the legal controls still work (one-shot AND PTY),
/// repeated cancels / late polls on settled records are harmless, and
/// no orphan or unrelated-process damage remains.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn rr1_f01_c04_same_process_usable_after_repeated_refusals_and_releases() {
    let _serial = serial().await;
    let h = cap_harness(1);
    let ctx = kernel_ctx("sess_rr1f01_c04", "run_c04");
    let unrelated = spawn_sentinel(&h, 300);
    let sentinel = h.ws.join("c04-sentinel.txt");
    std::fs::write(&sentinel, b"BASELINE").expect("write sentinel");
    let before = sha256_of(&sentinel);
    let counter = h.ws.join("c04-dispatches.txt");
    let helper = write_dispatch_helper(&h.ws, &counter, &sentinel);
    let mut settled_ids: Vec<ProcessHandleId> = Vec::new();
    let mut settled_pids: Vec<i32> = Vec::new();

    // > live_cap rounds (live_cap = 1, four rounds) of occupy → refuse →
    // release, WITHOUT restarting anything.
    for round in 0..4 {
        let occupy_call = format!("call-c04-occupy-{round}");
        let (occupant_fut, occupant_id) = start_long_oneshot(&h, &ctx, &occupy_call).await;
        let occupant_pid = h.supervisor.record(&occupant_id).unwrap().pid;

        let refused = call_exec(
            &h,
            &ctx,
            &format!("call-c04-refuse-{round}"),
            json!({
                "argv": [helper, "park"]
            }),
        )
        .await
        .expect("the over-capacity call resolves");
        assert!(
            error_text(&refused).contains("EXEC_PROCESS_REGISTRY_FULL"),
            "round {round} refusal"
        );

        // Release through the real bounded chain.
        terminate_and_settle(&h, &occupant_id, TerminationReason::Close).await;
        let _ = occupant_fut.await.expect("the occupant settles");
        assert!(
            !process_alive(occupant_pid),
            "round {round} leaves no orphan"
        );
        settled_ids.push(occupant_id);
        settled_pids.push(occupant_pid);

        // Adversarial: REPEATED cancels on the settled record must be
        // harmless AlreadyTerminal no-ops (never a stray killpg, never a
        // live_count decrement — the next round's spawn proves that).
        for _ in 0..3 {
            let receipt = h
                .supervisor
                .terminate(
                    &settled_ids[settled_ids.len() - 1],
                    TerminationReason::Close,
                )
                .await;
            assert!(matches!(
                receipt.outcome,
                TerminationOutcome::AlreadyTerminal(_)
            ));
        }
        assert!(
            h.supervisor.live_handles().is_empty(),
            "round {round}: the slot is free again"
        );
    }

    // Every round's refusal was zero-dispatch (the sentinel and the
    // counter are the durable proofs) and the unrelated sentinel is
    // untouched throughout.
    assert_eq!(dispatch_count(&counter), 0, "zero dispatches in all rounds");
    assert_eq!(sha256_of(&sentinel), before);
    for pid in &settled_pids {
        assert!(!process_alive(*pid));
    }
    assert!(process_alive(unrelated), "the unrelated sentinel survives");
    assert!(h.supervisor.live_handles().is_empty());

    // Legal control 1: a normal one-shot still runs to completion.
    let ok = call_exec(
        &h,
        &ctx,
        "call-c04-control-oneshot",
        json!({"argv": ["/bin/echo", "C04_CONTROL_OK"]}),
    )
    .await
    .expect("the legal one-shot control works after the failure rounds");
    assert!(text_of(&ok).contains("C04_CONTROL_OK"), "{}", text_of(&ok));
    match status_of(&ok) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 0),
        other => panic!("control exits 0: {other:?}"),
    }

    // Legal control 2: a normal PTY still starts, answers and closes.
    let pty = call_exec(
        &h,
        &ctx,
        "call-c04-control-pty",
        json!({"argv": ["/bin/sleep", "300"], "tty": true}),
    )
    .await
    .expect("the legal pty control works after the failure rounds");
    let pty_handle = match status_of(&pty) {
        Some(ToolRunStatus::Running { handle }) => {
            ProcessHandleId::parse(&handle).expect("pty handle")
        }
        other => panic!("pty running: {other:?}"),
    };
    let poll = call_write_stdin(
        &h,
        &ctx,
        "call-c04-control-poll",
        json!({"process_id": pty_handle.as_str()}),
    )
    .await
    .expect("pty poll");
    assert!(
        text_of(&poll).contains("status: running"),
        "{}",
        text_of(&poll)
    );
    terminate_and_settle(&h, &pty_handle, TerminationReason::Close).await;

    // Adversarial: LATE traffic on the settled pty handle reports the
    // honest terminal state and does not disturb anything.
    let late_poll = call_write_stdin(
        &h,
        &ctx,
        "call-c04-late-poll",
        json!({"process_id": pty_handle.as_str()}),
    )
    .await
    .expect("late poll resolves");
    assert!(
        text_of(&late_poll).contains("terminated"),
        "the late poll sees the settled state: {}",
        text_of(&late_poll)
    );
    let late_receipt = h
        .supervisor
        .terminate(&pty_handle, TerminationReason::Close)
        .await;
    assert!(matches!(
        late_receipt.outcome,
        TerminationOutcome::AlreadyTerminal(_)
    ));

    // And after ALL of that, one more normal round still works.
    let (final_fut, final_id) = start_long_oneshot(&h, &ctx, "call-c04-final").await;
    terminate_and_settle(&h, &final_id, TerminationReason::Close).await;
    let _ = final_fut.await.expect("the final round settles");
    assert!(h.supervisor.live_handles().is_empty());
    assert!(
        process_alive(unrelated),
        "the unrelated sentinel still lives"
    );
    cap_teardown(&h).await;
}
