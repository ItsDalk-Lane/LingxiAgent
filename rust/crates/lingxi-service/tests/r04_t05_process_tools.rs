//! R04-T05 acceptance: native exec_command / write_stdin, PTY and REAL
//! process-tree cancellation (R04-A09 / R04-A10 + the adversarial
//! additions).
//!
//! Everything here runs against the REAL composition root
//! (`ServiceState::bootstrap_with_deps`) with the REAL run driver, the
//! REAL journal, the REAL registry, the REAL gateway and the REAL
//! ApprovalService — and the two REAL native process executors
//! (`lingxi_service::exectools`) registered through
//! `register_process_tools`, backed by the REAL
//! `lingxi_service::procsupervisor::ProcessSupervisor` (POSIX process
//! groups, killpg, bounded cleanup). Every assertion about processes is
//! a REAL OS observation (`kill(pid, 0)` probes against processes THIS
//! TEST created and registered — never foreign processes).
//!
//! Test-double boundary (the R04 scope matrix `test_double_boundary`):
//! - `StepsProvider`/`RunIdCaptureProvider` (`TurnProviderPort`) —
//!   external model responses ONLY; the run-id capture reads the
//!   TRUSTED `RunContext` the driver hands the provider (never model
//!   data) so the test can cancel the in-flight run through the real
//!   user-facing entry.
//! - The authenticated approver is the test calling the real
//!   `ApprovalService` surface.
//! - The sentinel processes (`/bin/sleep`) are UNRELATED test-created
//!   processes (R04-A09's 无关哨兵); the test kills them by exact PID
//!   at teardown. No wildcard /tmp cleanup, no bare-PID kills of
//!   unknown processes.
//!
//! Scenarios:
//! - `r04_a09_*` — a command creating child/grandchild processes is
//!   cancelled through the REAL user-facing run cancellation: the whole
//!   managed tree exits (PID-level), the unrelated sentinel SURVIVES,
//!   pipes are reclaimed, the journal keeps `started`-without-receipt
//!   (the R03 Unknown window) and the supervisor receipt is accurate.
//! - `r04_a10_*` — a real PTY interactive program: input, multiple
//!   reads, resize (`stty size`), interrupt (\x03 → SIGINT to the
//!   foreground group) and exit-with-code.
//! - adversarial legs — parent exits while a grandchild holds the
//!   output pipe; output storm (bounded window + capped spill + head/
//!   tail truncation + 落盘引用); UTF-8 boundary safety; unresponsive
//!   process (timeout chain); stdout+stderr both blocking; cross-session
//!   write_stdin; forged/stale handles; cancel racing natural exit;
//!   gateway/approval integration (read_only denial, ask round-trip,
//!   workdir resource judgment incl. symlinks); the env whitelist.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lingxi_kernel::ports::{
    InvocationJournalEntry, InvocationPhase, ProviderDescriptor, ProviderTurn, ProviderTurnResult,
    StoragePort, ToolExecutionResult, ToolOutcome, ToolRequest, ToolRunStatus, TurnProviderPort,
};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetId};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, ToolCallId};
use lingxi_service::approval_service::{Answer, AnswerOutcome, ApprovalService};
use lingxi_service::exectools::register_process_tools;
use lingxi_service::procsupervisor::{
    ProcessHandleId, ProcessSupervisor, RecordPhase, SupervisorLimits, TerminationOutcome,
    TerminationReason,
};
use lingxi_service::resourceaccess::ResourceAccess;
use lingxi_service::toolgateway::{
    GatewayRefusal, InvocationPermissionContext, ToolInvocationGateway,
};
use lingxi_service::{
    approval, prepare_layout, ServiceConfig, ServiceDeps, ServiceState, LOCAL_OWNER_USER_ID,
};
use lingxi_service::{HomeSource, NetworkMode};
use serde_json::json;

const NOW_MS: u64 = 1_790_409_600_000;
const EXEC_TARGET: &str = "tool:first-party:exec_command";

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

// ── doubles (external stand-ins only) ──────────────────────────────────────

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

/// Scripted steps + a capture of the TRUSTED run id the driver hands
/// the provider (so the test can cancel the in-flight run through the
/// real user-facing cancellation entry).
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
            .unwrap_or_else(|| final_turn("done"));
        let ctx_at_issue = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, next) })
    }
}

fn tool_request(target: &str, args: serde_json::Value) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        requests: vec![
            ToolRequest::from_effective_arguments(target, args, &budget())
                .expect("effective request"),
        ],
    }
}

// ── composition-root harness ───────────────────────────────────────────────

struct Harness {
    state: ServiceState,
    gateway: Arc<ToolInvocationGateway>,
    approvals: Arc<ApprovalService>,
    supervisor: Arc<ProcessSupervisor>,
    exec_target: ToolTargetId,
    write_stdin_target: ToolTargetId,
    root: PathBuf,
    ws: PathBuf,
    home: PathBuf,
    /// Unrelated sentinel processes created by THIS test (killed by
    /// exact pid at teardown).
    sentinels: std::sync::Mutex<Vec<i32>>,
}

impl Harness {
    fn register_sentinel(&self, pid: i32) {
        self.sentinels.lock().unwrap().push(pid);
    }
}

async fn teardown(h: &Harness) {
    for pid in h.sentinels.lock().unwrap().drain(..) {
        // SAFETY: signal a process this test spawned itself.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    let _ = h.supervisor.shutdown_all().await;
    h.state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&h.home);
    let _ = std::fs::remove_dir_all(&h.root);
}

/// A unique test directory (pid + nanos + sequence — no cross-test
/// collisions, never a shared /tmp wildcard).
fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r04t05-{tag}-{}-{}-{}",
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

/// The full harness over a pre-created test root (the workspace lives at
/// `<root>/ws`; a restricted sibling proves the resource boundary).
async fn process_harness_at(
    root: PathBuf,
    provider: Arc<dyn TurnProviderPort>,
    limits: SupervisorLimits,
) -> Harness {
    let home = test_dir("home");
    let ws = root.join("ws");
    let restricted = root.join("restricted");
    std::fs::create_dir_all(&ws).expect("workspace");
    std::fs::create_dir_all(&restricted).expect("restricted area");
    std::fs::write(restricted.join("secret.txt"), b"TOP SECRET SENTINEL").expect("sentinel");

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
    let approvals = Arc::new(ApprovalService::new(Arc::new(
        lingxi_service::inject::SystemClock,
    )));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(lingxi_service::inject::SystemClock),
        budget(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    let supervisor = Arc::new(
        ProcessSupervisor::new(Arc::new(lingxi_service::inject::SystemClock), limits)
            .expect("supervisor"),
    );
    let core = register_process_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&supervisor),
        Arc::clone(&access),
        ws.clone(),
        Arc::new(lingxi_service::inject::SystemClock),
        &budget(),
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
    Harness {
        state,
        gateway,
        approvals,
        supervisor,
        exec_target: core.exec_target,
        write_stdin_target: core.write_stdin_target,
        root,
        ws,
        home,
        sentinels: std::sync::Mutex::new(Vec::new()),
    }
}

async fn process_harness(provider: Arc<dyn TurnProviderPort>) -> Harness {
    let root = test_dir("root");
    process_harness_at(
        root,
        provider,
        SupervisorLimits::with_spill_dir(test_dir("spill")),
    )
    .await
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

fn permission(mode: SessionPermissionMode) -> InvocationPermissionContext {
    InvocationPermissionContext::UserSession { mode }
}

/// Direct gateway call (prepare → execute_prepared) — the same two steps
/// the run driver performs, minus the driving loop.
async fn call_process_tool(
    h: &Harness,
    ctx: &RunContext,
    call: &str,
    target: &ToolTargetId,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, GatewayRefusal> {
    call_process_tool_with(h, ctx, call, target, args, user_operate()).await
}

async fn call_process_tool_with(
    h: &Harness,
    ctx: &RunContext,
    call: &str,
    target: &ToolTargetId,
    args: serde_json::Value,
    perm: InvocationPermissionContext,
) -> Result<ToolExecutionResult, GatewayRefusal> {
    let request = ToolRequest::from_effective_arguments(target.as_str(), args, &budget())
        .expect("effective request");
    let call_id = ToolCallId::new(call.to_string());
    let prepared = h.gateway.prepare_from_request(
        ctx,
        lingxi_service::toolgateway::CallerSurface::UserRun,
        "agent",
        perm,
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

fn error_text(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Failed { error } => error.message.clone(),
        other => panic!("expected a failed outcome, got {other:?}"),
    }
}

fn status_of(result: &ToolExecutionResult) -> Option<ToolRunStatus> {
    match &result.outcome {
        ToolOutcome::Success { result } => result.status.clone(),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn resource_uris(result: &ToolExecutionResult) -> Vec<String> {
    match &result.outcome {
        ToolOutcome::Success { result } => result
            .resource_refs
            .iter()
            .filter_map(|r| r.uri.clone())
            .collect(),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn truncated_of(result: &ToolExecutionResult) -> bool {
    match &result.outcome {
        ToolOutcome::Success { result } => result.truncated,
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

// ── real-OS probes ─────────────────────────────────────────────────────────

fn process_alive(pid: i32) -> bool {
    // SAFETY: signal-0 liveness probe.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Spawns an UNRELATED sentinel (`/bin/sleep`) owned by the test.
fn spawn_sentinel(h: &Harness, seconds: u32) -> i32 {
    let child = std::process::Command::new("/bin/sleep")
        .arg(seconds.to_string())
        .spawn()
        .expect("sentinel spawns");
    let pid = child.id() as i32;
    h.register_sentinel(pid);
    // The handle is deliberately leaked: the test manages sentinel
    // liveness by exact pid and kills it at teardown.
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

fn read_pid_file(path: &Path) -> Vec<i32> {
    let text = std::fs::read_to_string(path).expect("pid file readable");
    text.split_whitespace()
        .filter_map(|token| token.parse::<i32>().ok())
        .collect()
}

fn spawn_user_run(h: &Harness, session: &str, input: &str) -> tokio::task::JoinHandle<String> {
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
                NOW_MS,
            )
            .await
            .expect("user run drives")
            .run_id
    })
}

async fn set_mode(h: &Harness, session: &str, mode: SessionPermissionMode) {
    h.state
        .sessions()
        .set_permission_mode_for(&owner_principal(), session, mode)
        .await
        .expect("set permission mode");
}

async fn journal_of(h: &Harness, run_id: &str) -> Vec<InvocationJournalEntry> {
    h.state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.to_string()))
        .await
        .expect("journal loads")
}

fn exec_entry_of(journal: &[InvocationJournalEntry]) -> &InvocationJournalEntry {
    journal
        .iter()
        .find(|e| e.target.ends_with("exec_command"))
        .expect("the exec_command journal entry")
}

// ── R04-A09: real grandchild cleanup, sentinel survives ───────────────────

#[tokio::test]
async fn r04_a09_managed_tree_is_killed_and_the_unrelated_sentinel_survives() {
    let root = test_dir("a09");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("r04a09-tree.pids");
    // The command creates a child (bash) + two grandchildren (sleeps),
    // publishes their pids (the barrier), then waits forever.
    let cmd = format!(
        "sleep 300 & A=$!; sleep 300 & B=$!; printf '%s %s %s\\n' \"$A\" \"$B\" \"$$\" > {}; wait",
        pid_file.display()
    );
    let provider = RunIdCaptureProvider::new(vec![
        tool_request(EXEC_TARGET, json!({"cmd": cmd})),
        final_turn("a09 done"),
    ]);
    let h = process_harness_at(
        root,
        provider.clone(),
        SupervisorLimits::with_spill_dir(test_dir("a09-spill")),
    )
    .await;
    let sentinel = spawn_sentinel(&h, 300);
    let run = spawn_user_run(&h, "sess_local_alpha", "A09: tree + sentinel");
    // Barrier 1: the tree published its pids (child + grandchildren alive).
    wait_until("tree pid file", Duration::from_secs(20), || {
        pid_file.exists().then_some(())
    })
    .await;
    let pids = read_pid_file(&pid_file);
    assert_eq!(pids.len(), 3, "child + two grandchildren: {pids:?}");
    for pid in &pids {
        assert!(process_alive(*pid), "pid {pid} alive before cancel");
    }
    // Barrier 2: the supervisor holds the live record.
    let handle = wait_until("live process record", Duration::from_secs(20), || {
        h.supervisor.live_handles().first().cloned()
    })
    .await;
    // Barrier 3: the trusted run id (captured from the driver's context).
    let run_id = wait_until("run id", Duration::from_secs(20), || provider.run_id()).await;
    let fired = h
        .state
        .runs()
        .cancel_run(run_id.as_str(), "user cancelled the tree command");
    assert!(
        matches!(
            fired,
            lingxi_service::cancel::FireOutcome::Fired
                | lingxi_service::cancel::FireOutcome::AlreadyCancelling
        ),
        "cancellation fired: {fired:?}"
    );
    // The whole managed tree exits (PID-level; our supervisor reaps).
    for pid in &pids {
        wait_until("managed pid to exit", Duration::from_secs(10), || {
            (!process_alive(*pid)).then_some(())
        })
        .await;
    }
    // The run settles cancelled.
    let settled = match run.await {
        Ok(id) => id,
        Err(err) => panic!("run settles cancelled: {err}"),
    };
    assert_eq!(settled, run_id, "the settled run is the cancelled one");
    // The UNRELATED sentinel SURVIVES the cancellation.
    assert!(
        process_alive(sentinel),
        "the unrelated sentinel must survive the managed tree's cancellation"
    );
    // The supervisor receipt is accurate: killpg was sent, the exit was
    // observed, resources were reclaimed.
    let snapshot = h
        .supervisor
        .record(&handle)
        .expect("the record is retained for receipts");
    match snapshot.phase {
        RecordPhase::Terminated {
            reason: TerminationReason::CallerDropped,
            ..
        } => {}
        other => panic!("expected Terminated(caller_dropped), got {other:?}"),
    }
    let audit_kinds: Vec<&str> = snapshot.audit.iter().map(|e| e.kind).collect();
    assert!(audit_kinds.contains(&"killpg_sent"), "audit: {snapshot:?}");
    assert!(
        audit_kinds.contains(&"exit_observed"),
        "audit: {snapshot:?}"
    );
    let settled_audit = snapshot
        .audit
        .iter()
        .find(|e| e.kind == "termination_settled")
        .expect("termination settled receipt");
    assert!(
        settled_audit.detail.contains("reclaimed=true"),
        "receipt: {}",
        settled_audit.detail
    );
    // Journal honesty: the invocation was started and never received a
    // receipt — the R03 Unknown window, never a fabricated result.
    let journal = journal_of(&h, &run_id).await;
    let entry = exec_entry_of(&journal);
    assert!(
        matches!(entry.phase, InvocationPhase::Started),
        "phase must be Started without a receipt: {:?}",
        entry.phase
    );
    assert!(entry.receipt.is_none(), "no receipt after cancellation");
    teardown(&h).await;
}

// ── R04-A10: PTY interaction does not regress ──────────────────────────────

/// Writes `chars` then polls (empty chars) until the accumulated
/// transcript contains `needle` — the bounded handshake for the
/// asynchronous PTY pipeline (never a bare sleep).
async fn send_and_expect(
    h: &Harness,
    ctx: &RunContext,
    handle: &str,
    chars: &str,
    needle: &str,
    timeout: Duration,
) -> String {
    if !chars.is_empty() {
        call_process_tool(
            h,
            ctx,
            "pty-write",
            &h.write_stdin_target,
            json!({"process_id": handle, "chars": chars}),
        )
        .await
        .expect("pty write");
    }
    let end = Instant::now() + timeout;
    let mut seen = String::new();
    loop {
        let poll = call_process_tool(
            h,
            ctx,
            "pty-poll",
            &h.write_stdin_target,
            json!({"process_id": handle}),
        )
        .await
        .expect("pty poll");
        seen.push_str(&text_of(&poll));
        if seen.contains(needle) {
            return seen;
        }
        if Instant::now() > end {
            panic!("timed out waiting for {needle:?}; transcript so far:\n{seen}");
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

#[tokio::test]
async fn r04_a10_pty_input_reads_resize_interrupt_and_exit() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn("pty harness")])).await;
    let ctx = kernel_ctx("sess_local_pty", "run_a10");
    let started = call_process_tool(
        &h,
        &ctx,
        "call-pty-start",
        &h.exec_target,
        json!({
            "argv": ["/bin/bash", "--noprofile", "--norc", "-i"],
            "tty": true,
            "cols": 80,
            "rows": 24
        }),
    )
    .await
    .expect("the pty terminal starts");
    let handle = match status_of(&started) {
        Some(ToolRunStatus::Running { handle }) => handle,
        other => panic!("expected a running handle, got {other:?}"),
    };
    // "Started" never masquerades as completion.
    let start_text = text_of(&started);
    assert!(handle.starts_with("proc:"), "opaque handle: {handle}");
    assert!(start_text.contains("process_id:"), "{start_text}");
    assert!(start_text.contains("running"), "{start_text}");
    assert!(
        !start_text.contains("exited") && !start_text.contains("exit code"),
        "started is not a completion claim: {start_text}"
    );

    // 1) Input + multiple reads: an echo round-trips through the PTY.
    let out1 = send_and_expect(
        &h,
        &ctx,
        &handle,
        "echo MARKER_ONE\n",
        "MARKER_ONE",
        Duration::from_secs(15),
    )
    .await;
    assert!(out1.contains("MARKER_ONE"), "{out1}");

    // 2) Window size 80x24 is live: `stty size` prints "24 80".
    let sized = send_and_expect(
        &h,
        &ctx,
        &handle,
        "stty size\n",
        "24 80",
        Duration::from_secs(15),
    )
    .await;
    assert!(sized.contains("24 80"), "{sized}");

    // 3) Resize to 100x40 takes effect (TIOCSWINSZ on the master).
    h.supervisor
        .pty_resize(&ProcessHandleId::parse(&handle).unwrap(), 100, 40)
        .expect("resize");
    let resized = send_and_expect(
        &h,
        &ctx,
        &handle,
        "stty size\n",
        "40 100",
        Duration::from_secs(15),
    )
    .await;
    assert!(resized.contains("40 100"), "{resized}");

    // 4) Interrupt: Ctrl-C (\x03) → the line discipline delivers SIGINT
    //    to the foreground group: the foreground job is killed
    //    (NEVER_REACHED stays absent) and the shell answers again.
    let mut transcript = send_and_expect(
        &h,
        &ctx,
        &handle,
        "echo BEGIN_INT && sleep 30 && echo NEVER_\"REACHED\"\n",
        "BEGIN_INT",
        Duration::from_secs(15),
    )
    .await;
    // Send the INTR character; give the pipeline a short bounded settle
    // (the echo of ^C is best-effort — the hard assertions follow).
    call_process_tool(
        &h,
        &ctx,
        "call-intr",
        &h.write_stdin_target,
        json!({"process_id": handle, "chars": "\u{3}"}),
    )
    .await
    .expect("interrupt write");
    for _ in 0..10 {
        let poll = call_process_tool(
            &h,
            &ctx,
            "call-intr-poll",
            &h.write_stdin_target,
            json!({"process_id": handle}),
        )
        .await
        .expect("interrupt poll");
        transcript.push_str(&text_of(&poll));
        if transcript.contains("^C") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    let revived = send_and_expect(
        &h,
        &ctx,
        &handle,
        "echo AFTER_INTR\n",
        "AFTER_INTR",
        Duration::from_secs(15),
    )
    .await;
    assert!(
        revived.contains("AFTER_INTR"),
        "shell alive after SIGINT: {revived}"
    );
    assert!(
        !transcript.contains("NEVER_REACHED") && !revived.contains("NEVER_REACHED"),
        "the foreground job was really interrupted: {transcript}{revived}"
    );

    // 5) Exit with a code: `exit 7` → exited (7), final poll reports it.
    call_process_tool(
        &h,
        &ctx,
        "call-exit",
        &h.write_stdin_target,
        json!({"process_id": handle, "chars": "exit 7\n"}),
    )
    .await
    .expect("exit write");
    let parsed = ProcessHandleId::parse(&handle).unwrap();
    let final_phase = wait_until("terminal exit", Duration::from_secs(15), || {
        h.supervisor
            .phase_of(&parsed)
            .filter(|phase| phase.is_terminal())
    })
    .await;
    match final_phase {
        RecordPhase::Exited { fact } => assert_eq!(fact.status_code(), 7),
        other => panic!("expected natural exit 7, got {other:?}"),
    }
    let final_poll = call_process_tool(
        &h,
        &ctx,
        "call-final-poll",
        &h.write_stdin_target,
        json!({"process_id": handle}),
    )
    .await
    .expect("final poll");
    match status_of(&final_poll) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 7),
        other => panic!("final poll reports the exit: {other:?}"),
    }
    let final_text = text_of(&final_poll);
    assert!(final_text.contains("exited (exit code 7)"), "{final_text}");
    // Writing to an exited terminal fails honestly.
    let late = call_process_tool(
        &h,
        &ctx,
        "call-late",
        &h.write_stdin_target,
        json!({"process_id": handle, "chars": "echo TOO_LATE\n"}),
    )
    .await
    .expect("late write resolves");
    let late_err = error_text(&late);
    assert!(
        late_err.contains("not running"),
        "late write on an exited terminal: {late_err}"
    );
    teardown(&h).await;
}

// ── persistent terminal lifetime (explicit registration + shutdown) ───────

#[tokio::test]
async fn persistent_terminal_lifetime_is_explicit_and_shutdown_is_bounded() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn("term harness")])).await;
    let ctx = kernel_ctx("sess_local_term", "run_term");
    let started = call_process_tool(
        &h,
        &ctx,
        "call-term-start",
        &h.exec_target,
        json!({"argv": ["/bin/sleep", "300"], "tty": true}),
    )
    .await
    .expect("terminal starts");
    let handle = match status_of(&started) {
        Some(ToolRunStatus::Running { handle }) => handle,
        other => panic!("running handle: {other:?}"),
    };
    let parsed = ProcessHandleId::parse(&handle).unwrap();
    let snapshot = h.supervisor.record(&parsed).expect("record");
    assert_eq!(snapshot.kind.wire_name(), "persistent_terminal");
    let pid = snapshot.pid;
    assert!(
        process_alive(pid),
        "the terminal survives its starting call"
    );
    // The tool call already returned — the lifetime is the registered
    // persistent one, not the tool call's.
    assert_eq!(h.supervisor.live_handles().len(), 1);
    // A poll read is fine while running.
    let poll = call_process_tool(
        &h,
        &ctx,
        "call-term-poll",
        &h.write_stdin_target,
        json!({"process_id": handle}),
    )
    .await
    .expect("poll");
    assert!(
        text_of(&poll).contains("status: running"),
        "{}",
        text_of(&poll)
    );
    // The frozen application-exit policy: shutdown_all terminates every
    // managed process, bounded, with accurate receipts.
    let receipts = h.supervisor.shutdown_all().await;
    assert_eq!(receipts.len(), 1, "one managed process terminated");
    let receipt = &receipts[0];
    assert_eq!(receipt.id, parsed);
    assert!(matches!(receipt.reason, TerminationReason::Shutdown));
    match &receipt.outcome {
        TerminationOutcome::Terminated {
            fact,
            reclaimed,
            drained_stdio: _,
        } => {
            assert_eq!(fact.status_code(), 137, "SIGKILL convention");
            assert!(reclaimed, "resources reclaimed");
        }
        other => panic!("bounded shutdown receipt: {other:?}"),
    }
    wait_until("terminal pid to exit", Duration::from_secs(10), || {
        (!process_alive(pid)).then_some(())
    })
    .await;
    teardown(&h).await;
}

// ── adversarial: parent exits while a grandchild holds the output pipe ─────

#[tokio::test]
async fn adversarial_parent_exit_with_grandchild_holding_the_pipe_is_bounded() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn("bg harness")])).await;
    let ctx = kernel_ctx("sess_local_bg", "run_bg");
    let pid_file = h.ws.join("bg-grandchild.pid");
    let started_at = Instant::now();
    let result = call_process_tool(
        &h,
        &ctx,
        "call-bg",
        &h.exec_target,
        json!({
            "cmd": format!(
                "sleep 2 & printf '%s\\n' \"$!\" > {}; echo PARENT_DONE",
                pid_file.display()
            )
        }),
    )
    .await
    .expect("the one-shot returns");
    let elapsed = started_at.elapsed();
    // The direct child exited fast; the bounded stdio grace must NOT
    // wait for the 2 s grandchild holding the pipe (the incumbent's
    // `exitStdioGraceMs` semantics).
    assert!(
        elapsed < Duration::from_millis(1500),
        "bounded grace, elapsed: {elapsed:?}"
    );
    let text = text_of(&result);
    assert!(text.contains("PARENT_DONE"), "{text}");
    match status_of(&result) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 0),
        other => panic!("exit 0: {other:?}"),
    }
    // The grandchild briefly survives (the incumbent's documented
    // behavior: normal exit does not kill the group).
    let grandchild = wait_until("grandchild pid file", Duration::from_secs(5), || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
    })
    .await;
    assert!(
        process_alive(grandchild),
        "grandchild alive after parent exit"
    );
    // Cleanup: kill the test-registered grandchild by exact pid.
    // SAFETY: the grandchild was created by this test's command.
    unsafe {
        libc::kill(grandchild, libc::SIGKILL);
    }
    teardown(&h).await;
}

// ── adversarial: output storm is bounded, truncated, with a spill ref ──────

#[tokio::test]
async fn adversarial_output_storm_is_bounded_truncated_and_spilled() {
    let root = test_dir("storm");
    let mut limits = SupervisorLimits::with_spill_dir(test_dir("storm-spill"));
    // A small spill cap makes the cap reachable with a small storm.
    limits.spill_cap_bytes = 64 * 1024;
    let h = process_harness_at(
        root,
        RunIdCaptureProvider::new(vec![final_turn("storm")]),
        limits,
    )
    .await;
    let ctx = kernel_ctx("sess_local_storm", "run_storm");
    let started_at = Instant::now();
    let result = call_process_tool(
        &h,
        &ctx,
        "call-storm",
        &h.exec_target,
        json!({"cmd": "seq 1 40000 | sed 's/^/storm line /'"}),
    )
    .await
    .expect("the storm completes");
    // ~500 KB of output resolves in bounded time and bounded memory.
    assert!(
        started_at.elapsed() < Duration::from_secs(30),
        "bounded storm: {:?}",
        started_at.elapsed()
    );
    assert!(truncated_of(&result), "the storm result is truncated");
    let text = text_of(&result);
    assert!(
        text.contains("Showing first") && text.contains("lines"),
        "head/tail notice: {text}"
    );
    // The rolling window (2x the result budget, incumbent parity) keeps
    // the tail; once evictions began the "head" is the head of the
    // retained window — both ends of the retained storm are visible.
    assert!(
        text.contains("storm line 40000"),
        "the tail survives: {text}"
    );
    assert!(
        text.split('\n')
            .any(|line| line.starts_with("storm line ") || line.starts_with("ne ")),
        "retained window head visible: {text}"
    );
    // The spill reference (落盘引用) is capped and accurate.
    let uris = resource_uris(&result);
    assert_eq!(uris.len(), 1, "one spill resource ref: {uris:?}");
    let spill_path = uris[0]
        .strip_prefix("file://")
        .expect("file uri")
        .to_string();
    let spill_bytes = std::fs::metadata(&spill_path)
        .map(|m| m.len())
        .unwrap_or_else(|_| panic!("spill exists at {spill_path}"));
    assert_eq!(spill_bytes, 64 * 1024, "spill hit its cap exactly");
    match status_of(&result) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 0),
        other => panic!("exit 0: {other:?}"),
    }
    teardown(&h).await;
}

// ── adversarial: UTF-8 chunk boundaries ─────────────────────────────────────

#[tokio::test]
async fn adversarial_utf8_output_survives_chunk_splits() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn("utf8 harness")])).await;
    let ctx = kernel_ctx("sess_local_utf8", "run_utf8");
    // Multibyte characters split across two writes separated by a
    // sleep: the final decode joins the window bytes (never mangled).
    let result = call_process_tool(
        &h,
        &ctx,
        "call-utf8",
        &h.exec_target,
        json!({"cmd": "printf 'a中文'; sleep 1; printf '字符ok\\n'"}),
    )
    .await
    .expect("utf8 command completes");
    let text = text_of(&result);
    assert!(text.contains("a中文字符ok"), "no mangled multibyte: {text}");
    teardown(&h).await;
}

// ── adversarial: unresponsive process → the timeout chain ─────────────────

#[tokio::test]
async fn adversarial_unresponsive_process_times_out_within_the_bound() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn(
        "timeout harness",
    )]))
    .await;
    let ctx = kernel_ctx("sess_local_to", "run_to");
    let started_at = Instant::now();
    let result = call_process_tool(
        &h,
        &ctx,
        "call-timeout",
        &h.exec_target,
        json!({"argv": ["/bin/sleep", "300"], "timeout_seconds": 1}),
    )
    .await
    .expect("the timeout resolves");
    let elapsed = started_at.elapsed();
    assert!(
        elapsed >= Duration::from_secs(1) && elapsed < Duration::from_secs(6),
        "bounded timeout chain: {elapsed:?}"
    );
    let text = text_of(&result);
    assert!(
        text.contains("Command timed out after 1 seconds"),
        "timeout notice: {text}"
    );
    assert!(
        text.contains("tty=true"),
        "the notice teaches the tty continuation: {text}"
    );
    match status_of(&result) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 137, "128+SIGKILL"),
        other => panic!("status after timeout: {other:?}"),
    }
    assert!(
        h.supervisor.live_handles().is_empty(),
        "nothing stays live after the bounded timeout"
    );
    teardown(&h).await;
}

// ── adversarial: stdout and stderr both blocking ───────────────────────────

#[tokio::test]
async fn adversarial_both_streams_blocking_still_completes() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn(
        "streams harness",
    )]))
    .await;
    let ctx = kernel_ctx("sess_local_streams", "run_streams");
    // Each stream writes ~180KB — far beyond the 64KB pipe buffer: only
    // concurrent pumping of BOTH streams completes (a sequential reader
    // would deadlock).
    let result = call_process_tool(
        &h,
        &ctx,
        "call-streams",
        &h.exec_target,
        json!({"cmd": "seq 1 40000; seq 1 40000 >&2"}),
    )
    .await
    .expect("both-stream command completes");
    match status_of(&result) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 0),
        other => panic!("exit 0: {other:?}"),
    }
    let text = text_of(&result);
    assert!(
        text.contains("1") && text.contains("40000"),
        "both ends of the storm visible: {text}"
    );
    teardown(&h).await;
}

// ── adversarial: cross-session write_stdin + forged/stale handles ──────────

#[tokio::test]
async fn adversarial_cross_session_and_forged_write_stdin_are_refused() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn("authz harness")])).await;
    let session_a = kernel_ctx("sess_local_alpha", "run_a");
    let session_b = kernel_ctx("sess_local_beta", "run_b");
    let started = call_process_tool(
        &h,
        &session_a,
        "call-a-start",
        &h.exec_target,
        json!({"argv": ["/bin/sleep", "300"], "tty": true}),
    )
    .await
    .expect("terminal starts in session A");
    let handle = match status_of(&started) {
        Some(ToolRunStatus::Running { handle }) => handle,
        other => panic!("running handle: {other:?}"),
    };
    // Cross-SESSION write_stdin is refused — never delivered.
    let foreign = call_process_tool(
        &h,
        &session_b,
        "call-b-foreign",
        &h.write_stdin_target,
        json!({"process_id": handle, "chars": "echo INJECTED\n"}),
    )
    .await
    .expect("the foreign write resolves");
    let err = error_text(&foreign);
    assert!(
        err.contains("WRITE_STDIN_NOT_OWNED"),
        "cross-session refusal: {err}"
    );
    // The owning session can still use the terminal.
    let owned = call_process_tool(
        &h,
        &session_a,
        "call-a-poll",
        &h.write_stdin_target,
        json!({"process_id": handle}),
    )
    .await
    .expect("owner poll");
    assert!(
        text_of(&owned).contains("status: running"),
        "{}",
        text_of(&owned)
    );
    // Forged handle: the format gate.
    let forged = call_process_tool(
        &h,
        &session_a,
        "call-forged",
        &h.write_stdin_target,
        json!({"process_id": "proc:deadbeef", "chars": "x"}),
    )
    .await
    .expect("forged resolves");
    assert!(
        error_text(&forged).contains("WRITE_STDIN_UNKNOWN_PROCESS"),
        "forged handle: {}",
        error_text(&forged)
    );
    // Well-formed but never minted.
    let ghost = call_process_tool(
        &h,
        &session_a,
        "call-ghost",
        &h.write_stdin_target,
        json!({"process_id": "proc:0000000000000000000000000000ffff", "chars": "x"}),
    )
    .await
    .expect("ghost resolves");
    assert!(
        error_text(&ghost).contains("WRITE_STDIN_UNKNOWN_PROCESS"),
        "never-minted handle: {}",
        error_text(&ghost)
    );
    // A ONE-SHOT process handle is not an interactive terminal: drive
    // one concurrently and grab its live handle from the supervisor.
    let oneshot_ctx = kernel_ctx("sess_local_alpha", "run_oneshot");
    let mut oneshot_fut = Box::pin(call_process_tool(
        &h,
        &oneshot_ctx,
        "call-oneshot-bg",
        &h.exec_target,
        json!({"argv": ["/bin/sleep", "300"], "timeout_seconds": 60}),
    ));
    let oneshot_handle = loop {
        tokio::select! {
            settled = &mut oneshot_fut => {
                panic!("the one-shot settled before the probe: {:?}",
                    settled.map(|_| ()))
            }
            _ = tokio::time::sleep(Duration::from_millis(15)) => {
                if let Some(id) = h
                    .supervisor
                    .live_handles()
                    .into_iter()
                    .find(|id| id.as_str() != handle)
                {
                    break id;
                }
            }
        }
    };
    let not_interactive = call_process_tool(
        &h,
        &session_a,
        "call-not-interactive",
        &h.write_stdin_target,
        json!({"process_id": oneshot_handle.as_str(), "chars": "x"}),
    )
    .await
    .expect("not-interactive resolves");
    assert!(
        error_text(&not_interactive).contains("WRITE_STDIN_NOT_INTERACTIVE"),
        "one-shot handles are not terminals: {}",
        error_text(&not_interactive)
    );
    // Cleanup the background one-shot through the bounded chain; the
    // in-flight call then settles on the terminated phase.
    h.supervisor
        .terminate(&oneshot_handle, TerminationReason::Close)
        .await;
    let _ = oneshot_fut
        .await
        .expect("the one-shot call settles after termination");
    teardown(&h).await;
}

// ── adversarial: cancel racing natural exit ────────────────────────────────

#[tokio::test]
async fn adversarial_cancel_racing_natural_exit_keeps_invariants() {
    let root = test_dir("race");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let ready = ws.join("race-ready.txt");
    let cmd = format!("echo ready > {}; sleep 1; exit 0", ready.display());
    let provider = RunIdCaptureProvider::new(vec![
        tool_request(EXEC_TARGET, json!({"cmd": cmd})),
        final_turn("race done"),
    ]);
    let h = process_harness_at(
        root,
        provider.clone(),
        SupervisorLimits::with_spill_dir(test_dir("race-spill")),
    )
    .await;
    let run = spawn_user_run(&h, "sess_local_alpha", "race run");
    let handle = wait_until("live record", Duration::from_secs(20), || {
        h.supervisor.live_handles().first().cloned()
    })
    .await;
    let snapshot = h.supervisor.record(&handle).unwrap();
    let pid = snapshot.pid;
    // Barrier: the command is mid-flight (its ready marker exists).
    wait_until("ready marker", Duration::from_secs(20), || {
        ready.exists().then_some(())
    })
    .await;
    let run_id = wait_until("run id", Duration::from_secs(20), || provider.run_id()).await;
    // Cancel DURING the in-flight window (racing the natural exit).
    let _ = h
        .state
        .runs()
        .cancel_run(run_id.as_str(), "user cancelled mid-race");
    let settled = match run.await {
        Ok(id) => id,
        Err(err) => panic!("run settles either way: {err}"),
    };
    assert_eq!(settled, run_id);
    // The invariant: the record reaches a terminal phase, the process is
    // gone, nothing hangs — whichever side of the race won.
    let phase = wait_until("terminal phase", Duration::from_secs(10), || {
        h.supervisor
            .record(&handle)
            .and_then(|s| s.phase.is_terminal().then_some(s.phase))
    })
    .await;
    assert!(phase.is_terminal(), "{phase:?}");
    wait_until("pid gone", Duration::from_secs(10), || {
        (!process_alive(pid)).then_some(())
    })
    .await;
    let journal = journal_of(&h, &run_id).await;
    let entry = exec_entry_of(&journal);
    // Either the run completed (receipt present) or it was cancelled
    // (started without receipt — never a fabricated result).
    match entry.phase {
        InvocationPhase::Started => assert!(entry.receipt.is_none()),
        InvocationPhase::Succeeded | InvocationPhase::Failed => {
            assert!(entry.receipt.is_some())
        }
        other => panic!("unexpected phase {other:?}"),
    }
    teardown(&h).await;
}

// ── gateway + approval face + cwd resource judgment ────────────────────────

#[tokio::test]
async fn process_tools_go_through_the_gateway_policy_and_cwd_authorization() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn("gwharness")])).await;
    let ctx = kernel_ctx("sess_local_gw", "run_gw");

    // read_only: the Execute-class exec_command is DENIED at the policy
    // face during PREPARE — a loud zero-dispatch refusal.
    let denied = call_process_tool_with(
        &h,
        &ctx,
        "call-ro",
        &h.exec_target,
        json!({"argv": ["/usr/bin/true"]}),
        permission(SessionPermissionMode::ReadOnly),
    )
    .await;
    match denied {
        Err(refusal) => assert!(
            format!("{refusal}").contains("ACTION_BLOCKED_BY_READ_ONLY"),
            "read_only denial: {refusal}"
        ),
        Ok(result) => panic!(
            "read_only must refuse the Execute-class tool: {}",
            text_of(&result)
        ),
    }
    assert!(h.supervisor.live_handles().is_empty(), "zero dispatch");

    // The cwd/workdir is a resource: outside the authorized roots the
    // PREPARE refuses (zero dispatch, the approver is never asked).
    let outside = call_process_tool(
        &h,
        &ctx,
        "call-outside",
        &h.exec_target,
        json!({
            "argv": ["/bin/pwd"],
            "workdir": h.root.join("restricted").display().to_string()
        }),
    )
    .await;
    match outside {
        Err(refusal) => assert!(
            format!("{refusal}").contains("gateway_resource_scope_denied"),
            "workdir outside roots: {refusal}"
        ),
        Ok(result) => panic!(
            "workdir outside the roots must refuse at prepare: {}",
            text_of(&result)
        ),
    }
    // A symlinked workdir is judged by its REAL target.
    let link = h.ws.join("to-restricted");
    std::os::unix::fs::symlink(h.root.join("restricted"), &link).expect("symlink");
    let via_link = call_process_tool(
        &h,
        &ctx,
        "call-link",
        &h.exec_target,
        json!({"argv": ["/bin/pwd"], "workdir": link.display().to_string()}),
    )
    .await;
    match via_link {
        Err(refusal) => assert!(
            format!("{refusal}").contains("gateway_resource_scope_denied"),
            "symlink judged by its real target: {refusal}"
        ),
        Ok(result) => panic!("symlink escape must refuse: {}", text_of(&result)),
    }
    // Restricted area untouched.
    assert_eq!(
        std::fs::read(h.root.join("restricted").join("secret.txt")).unwrap(),
        b"TOP SECRET SENTINEL"
    );

    // Legal contrast: a workdir inside the workspace (canonicalized).
    let inner = h.ws.join("inner");
    std::fs::create_dir_all(&inner).expect("inner dir");
    let ok = call_process_tool(
        &h,
        &ctx,
        "call-inner",
        &h.exec_target,
        json!({"argv": ["/bin/pwd"], "workdir": inner.display().to_string()}),
    )
    .await
    .expect("in-workspace workdir is authorized");
    let text = text_of(&ok);
    let canonical = std::fs::canonicalize(&inner).unwrap();
    assert!(
        text.contains(canonical.to_str().unwrap()),
        "the process ran with the canonical cwd: {text}"
    );
    match status_of(&ok) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 0),
        other => panic!("exit 0: {other:?}"),
    }
    teardown(&h).await;
}

/// The ask-tier approval face round-trips a real exec_command through
/// the REAL approval service (the T03 surface on the T05 tools).
#[tokio::test]
async fn ask_session_exec_command_round_trips_the_approval_face() {
    let h = process_harness(RunIdCaptureProvider::new(vec![
        tool_request(EXEC_TARGET, json!({"argv": ["/usr/bin/true"]})),
        final_turn("ask done"),
    ]))
    .await;
    let session = "sess_local_alpha";
    set_mode(&h, session, SessionPermissionMode::Ask).await;
    let handle = spawn_user_run(&h, session, "ask: exec_command approval");
    let pending = wait_until("pending approval", Duration::from_secs(20), || {
        h.approvals
            .pending_of(&kernel_ctx(session, "pending-probe"), session)
            .into_iter()
            .find(|view| view.target.ends_with("exec_command"))
    })
    .await;
    // The approver sees the resource scope of the invocation (the cwd).
    let canonical_ws = std::fs::canonicalize(&h.ws).unwrap();
    assert!(
        pending
            .resources
            .iter()
            .any(|scope| scope.path == canonical_ws),
        "pending carries the workdir scope: {:?}",
        pending.resources
    );
    match h.approvals.answer(
        &kernel_ctx(session, "approver"),
        session,
        &pending.approval_id,
        Answer::Approve,
    ) {
        AnswerOutcome::Settled(Answer::Approve) => {}
        other => panic!("approval settles: {other:?}"),
    }
    let run_id = match handle.await {
        Ok(id) => id,
        Err(err) => panic!("run settles after approval: {err}"),
    };
    let journal = journal_of(&h, &run_id).await;
    let entry = exec_entry_of(&journal);
    assert!(matches!(entry.phase, InvocationPhase::Succeeded));
    let receipt = entry.receipt.as_ref().expect("receipt");
    assert!(receipt.dispatched, "the command really ran");
    assert!(h.supervisor.live_handles().is_empty());
    teardown(&h).await;
}

// ── environment whitelist ──────────────────────────────────────────────────

#[tokio::test]
async fn environment_is_whitelist_plus_explicit_overrides_never_full_inherit() {
    // Plant a server-side secret in THIS process's environment: a child
    // that inherits wholesale would leak it.
    // SAFETY: test setup before any threads read the environment.
    unsafe {
        std::env::set_var("LINGXI_SECRET_TOKEN", "super-secret-token-value");
    }
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn("env harness")])).await;
    let ctx = kernel_ctx("sess_local_env", "run_env");
    let result = call_process_tool(
        &h,
        &ctx,
        "call-env",
        &h.exec_target,
        json!({
            "argv": ["/usr/bin/env"],
            "env": {"LINGXI_TEST_MARKER": "42"}
        }),
    )
    .await
    .expect("env command runs");
    let text = text_of(&result);
    assert!(text.contains("LINGXI_TEST_MARKER=42"), "{text}");
    assert!(
        text.contains("PATH="),
        "whitelisted PATH passes through: {text}"
    );
    assert!(
        !text.contains("super-secret-token-value") && !text.contains("LINGXI_SECRET_TOKEN"),
        "the server's secrets are NEVER inherited: {text}"
    );
    teardown(&h).await;
}

// ── already-exited termination + spawn failure honesty ─────────────────────

#[tokio::test]
async fn terminate_after_natural_exit_is_already_terminal_and_spawn_failures_are_loud() {
    let h = process_harness(RunIdCaptureProvider::new(vec![final_turn("term-edge")])).await;
    let ctx = kernel_ctx("sess_local_edge", "run_edge");
    // Natural exit first.
    let result = call_process_tool(
        &h,
        &ctx,
        "call-exit0",
        &h.exec_target,
        json!({"argv": ["/bin/sleep", "1"]}),
    )
    .await
    .expect("sleep 1 completes");
    match status_of(&result) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 0),
        other => panic!("exit 0: {other:?}"),
    }
    assert!(h.supervisor.live_handles().is_empty(), "settled");

    // A record for an already-exited process: terminating it is a NO-OP
    // AlreadyTerminal receipt (nothing is killed — the anti-PID-recycling
    // rule). Drive a short one-shot locally, keep its handle, let it
    // settle, then terminate the retained record.
    let retained_ctx = kernel_ctx("sess_local_edge", "run_edge2");
    let mut retained_fut = Box::pin(call_process_tool(
        &h,
        &retained_ctx,
        "call-retained",
        &h.exec_target,
        json!({"argv": ["/bin/sleep", "2"]}),
    ));
    let retained = loop {
        tokio::select! {
            settled = &mut retained_fut => {
                panic!("the retained one-shot settled before the probe: {:?}",
                    settled.map(|_| ()))
            }
            _ = tokio::time::sleep(Duration::from_millis(15)) => {
                if let Some(id) = h.supervisor.live_handles().first().cloned() {
                    break id;
                }
            }
        }
    };
    let _ = retained_fut.await.expect("the retained one-shot settles");
    let receipt = h
        .supervisor
        .terminate(&retained, TerminationReason::Close)
        .await;
    match receipt.outcome {
        TerminationOutcome::AlreadyTerminal(RecordPhase::Exited { fact }) => {
            assert_eq!(fact.status_code(), 0);
        }
        other => panic!("already-terminal receipt: {other:?}"),
    }
    let snapshot = h.supervisor.record(&retained).unwrap();
    assert!(
        !snapshot.audit.iter().any(|e| e.kind == "killpg_sent"),
        "no kill was ever sent for an exited record: {snapshot:?}"
    );

    // Spawn failure: a nonexistent binary refuses loudly.
    let missing = call_process_tool(
        &h,
        &ctx,
        "call-missing",
        &h.exec_target,
        json!({"argv": ["/nonexistent/definitely-not-a-binary"]}),
    )
    .await
    .expect("spawn failure resolves as a tool failure");
    let err = error_text(&missing);
    assert!(
        err.contains("EXEC_SPAWN_FAILED"),
        "spawn failure vocabulary: {err}"
    );
    // Spawn failure: a nonexistent cwd carries the incumbent's hint.
    let bad_cwd = call_process_tool(
        &h,
        &ctx,
        "call-badcwd",
        &h.exec_target,
        json!({
            "argv": ["/bin/pwd"],
            "workdir": h.ws.join("does-not-exist-and-never-will").display().to_string()
        }),
    )
    .await
    .expect("bad cwd resolves as a tool failure");
    let err = error_text(&bad_cwd);
    assert!(
        err.contains("working directory does not exist"),
        "cwd hint: {err}"
    );
    // Invalid params: neither cmd nor argv.
    let neither = call_process_tool(
        &h,
        &ctx,
        "call-neither",
        &h.exec_target,
        json!({"timeout_seconds": 5}),
    )
    .await
    .expect("invalid params resolve as a tool failure");
    assert!(
        error_text(&neither).contains("EXEC_COMMAND_INVALID_PARAMS"),
        "{}",
        error_text(&neither)
    );
    teardown(&h).await;
}
