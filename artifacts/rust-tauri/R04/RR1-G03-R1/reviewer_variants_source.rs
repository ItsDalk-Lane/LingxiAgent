//! REVIEWER-R04-RR1-G03-R01 adversarial variants (my own parameters,
//! distinct from the executor's): different window geometry, repeated
//! polls with mixed chars, the late-exit-between-deadline-and-second-
//! call race, my own control group, and the late-observation-does-not-
//! upgrade-the-journal check.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-g03r1v-{tag}-{}-{}-{}",
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
        session_id: "sess_g03v".to_string(),
        run_id: "run_g03v".to_string(),
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

fn text_of(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
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

fn process_alive(pid: i32) -> bool {
    // SAFETY: signal-0 liveness probe on a test-created process.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn kill_owned(pid: i32) {
    // SAFETY: SIGKILL a process this test created (exact pid).
    unsafe {
        libc::kill(pid, libc::SIGKILL);
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

/// V1 (C01 variant): a different window geometry — child life 0.8s,
/// grace 2.5s, timeout 1.5s (inside [0.8, 3.3] with 0.7s/1.8s margins),
/// cleanup 2ms. The unconfirmed state must stay queryable, the
/// never-signalled holder alive, the slot held, and the deferred real
/// exit later recorded with the first cause before the slot returns
/// exactly once.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn v1_c01_variant_wider_window_stays_unconfirmed_then_real_exit() {
    let ws = test_dir("v1-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("v1");
    limits.cleanup_timeout = Duration::from_millis(2);
    limits.stdio_grace = Duration::from_millis(2500);
    limits.live_cap = 1;
    let supervisor = supervisor_with(limits);
    let tools = tools_over(&supervisor, &ws);
    let ctx = kernel_ctx("sess_v1", "run_v1");
    let call = ToolCallId::new("call_v1".to_string());
    let outcome = tools
        .run_exec_command(
            &ctx,
            &call,
            &json!({
                "cmd": format!(
                    "( sleep 300 ) & printf '%s\\n' \"$!\" > {}; echo V1_MARKER; sleep 0.8",
                    pid_file.display()
                ),
                "timeout_seconds": 2
            }),
        )
        .await;
    match status_of(&outcome) {
        Some(ToolRunStatus::StopUnconfirmed { handle, detail }) => {
            assert!(detail.contains("not observed"), "{detail}");
            assert_eq!(
                handle,
                supervisor
                    .retained_record_ids()
                    .into_iter()
                    .next()
                    .expect("retained")
                    .to_string(),
                "the original handle"
            );
        }
        other => panic!("V1: the honest unconfirmed status: {other:?}"),
    }
    let text = text_of(&outcome);
    assert!(text.contains("V1_MARKER"), "{text}");
    assert!(text.contains("unconfirmed"), "{text}");
    assert!(!text.contains("exited with code"), "{text}");
    let grandchild = read_pid_file(&pid_file).await;
    assert!(process_alive(grandchild), "no signal was ever sent");
    // The slot stays held while unconfirmed.
    let refusal = supervisor
        .spawn(oneshot_spec("v1-refusal", vec!["/usr/bin/true".to_string()], &ws))
        .await;
    assert!(
        matches!(refusal, Err(SpawnFailure::RegistryFull)),
        "{refusal:?}"
    );
    // The deferred REAL exit with the first cause.
    let handle = supervisor
        .retained_record_ids()
        .into_iter()
        .next()
        .expect("retained");
    let terminal = wait_until("deferred real exit", Duration::from_secs(6), || {
        let snap = supervisor.record(&handle)?;
        match &snap.phase {
            RecordPhase::Terminated {
                reason: TerminationReason::Timeout,
                fact: ExitFact::Code(0),
            } => Some(snap.audit.iter().map(|e| e.kind.to_string()).collect::<Vec<_>>()),
            _ => None,
        }
    })
    .await;
    assert!(
        terminal.iter().any(|k| k == "exit_observed"),
        "{terminal:?}"
    );
    // Slot returns exactly once.
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match supervisor
            .spawn(oneshot_spec("v1-probe", vec!["/usr/bin/true".to_string()], &ws))
            .await
        {
            Ok(spawned) => {
                let _ = supervisor.wait_terminal(&spawned.id).await;
                break;
            }
            Err(SpawnFailure::RegistryFull) => assert!(
                Instant::now() < end,
                "the live slot was never returned"
            ),
            Err(other) => panic!("unexpected: {other:?}"),
        }
    }
    kill_owned(grandchild);
    let _ = std::fs::remove_dir_all(&ws);
}

/// V2 (C02 variant): PTY unconfirmed window via the sanctioned
/// observation-delay injection with MY OWN budgets (cleanup 100ms,
/// grace 400ms); empty-char polls AND a non-empty-char poll must agree
/// with the text; a foreign session is still refused.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn v2_c02_variant_repeated_mixed_polls_and_foreign_refusal() {
    let ws = test_dir("v2-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let mut limits = limits_at("v2");
    limits.cleanup_timeout = Duration::from_millis(100);
    limits.stdio_grace = Duration::from_millis(400);
    let supervisor = supervisor_with(limits);
    supervisor.arm_reap_fault_for_verification(ReapFaultPoint::DelayObservationNext);
    let tools = tools_over(&supervisor, &ws);
    let ctx = kernel_ctx("sess_v2", "run_v2");
    let call = ToolCallId::new("call_v2".to_string());
    let started = tools
        .run_exec_command(
            &ctx,
            &call,
            &json!({"cmd": "echo V2_MARKER; sleep 300", "tty": true}),
        )
        .await;
    let handle = match status_of(&started) {
        Some(ToolRunStatus::Running { handle }) => handle,
        other => panic!("running start: {other:?}"),
    };
    let parsed = ProcessHandleId::parse(&handle).expect("minted");
    let receipt = supervisor
        .terminate(&parsed, TerminationReason::Close)
        .await;
    assert!(
        matches!(receipt.outcome, TerminationOutcome::CleanupTimedOut),
        "{:?}",
        receipt.outcome
    );
    // Mixed poll rounds: three empty, one with chars (a write attempt on
    // the unconfirmed handle), each honest.
    for round in 0..4 {
        let args = if round == 3 {
            json!({"process_id": handle, "chars": "x"})
        } else {
            json!({"process_id": handle})
        };
        let poll = tools.run_write_stdin(&ctx, &args).await;
        match &poll {
            ToolOutcome::Success { .. } => match status_of(&poll) {
                Some(ToolRunStatus::StopUnconfirmed { handle: h, .. }) => assert_eq!(h, handle),
                other => panic!("round {round} stays unconfirmed: {other:?}"),
            },
            // A write attempt on the unconfirmed terminal is refused with
            // the REAL phase echoed — an honest error, never a fabricated
            // exit claim.
            ToolOutcome::Failed { error } => {
                assert_eq!(error.code, lingxi_protocol::ErrorCode::Conflict, "{error}");
                assert!(
                    error.message.contains("CleanupTimedOut"),
                    "the refusal names the real phase: {error}"
                );
            }
            other => panic!("round {round}: {other:?}"),
        }
        if let ToolOutcome::Success { .. } = &poll {
            let text = text_of(&poll);
            assert!(text.contains("cleanup_timed_out"), "{text}");
            assert!(text.contains("unconfirmed") || text.contains("not observed"), "{text}");
            assert!(text.contains(&handle), "{text}");
        }
    }
    // Foreign session: refused before any status is produced.
    let foreign = kernel_ctx("sess_v2_foreign", "run_v2_foreign");
    match tools
        .run_write_stdin(&foreign, &json!({"process_id": handle}))
        .await
    {
        ToolOutcome::Failed { error } => {
            assert_eq!(error.code, lingxi_protocol::ErrorCode::Forbidden);
        }
        other => panic!("foreign refusal: {other:?}"),
    }
    // Exactly one honest expiry receipt, still unconfirmed.
    let snap = supervisor.record(&parsed).expect("queryable");
    let kinds = snap.audit.iter().map(|e| e.kind.to_string()).collect::<Vec<_>>();
    assert_eq!(
        kinds.iter().filter(|k| *k == "cleanup_timed_out").count(),
        1,
        "{kinds:?}"
    );
    assert!(!kinds.iter().any(|k| k == "exit_observed"), "{kinds:?}");
    let _ = std::fs::remove_dir_all(&ws);
}

/// V3 (C03 adversarial): the real exit arrives BETWEEN the first
/// terminator's deadline and the second call — the second terminate
/// must echo the REAL fact and the FIRST cause, never fabricate, and
/// the live slot must return exactly once.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn v3_c03_variant_real_exit_between_deadline_and_second_call() {
    let ws = test_dir("v3-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let pid_file = ws.join("grandchild.pid");
    let mut limits = limits_at("v3");
    limits.cleanup_timeout = Duration::from_millis(2);
    limits.stdio_grace = Duration::from_millis(400);
    limits.live_cap = 1;
    let supervisor = supervisor_with(limits);
    let spawned = supervisor
        .spawn(oneshot_spec(
            "v3",
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!(
                    "( sleep 300 ) & printf '%s\\n' \"$!\" > {}; echo V3_MARKER; sleep 0.5",
                    pid_file.display()
                ),
            ],
            &ws,
        ))
        .await
        .expect("spawn");
    wait_until("drain window", Duration::from_secs(5), || {
        let snap = supervisor.record(&spawned.id)?;
        (snap.child_reaped && !snap.phase.is_terminal()).then_some(snap)
    })
    .await;
    let r1 = supervisor
        .terminate(&spawned.id, TerminationReason::Timeout)
        .await;
    assert!(
        matches!(r1.outcome, TerminationOutcome::CleanupTimedOut),
        "{:?}",
        r1.outcome
    );
    // The real exit lands here (grace 400ms), BEFORE the second call.
    wait_until("deferred real exit", Duration::from_secs(5), || {
        let snap = supervisor.record(&spawned.id)?;
        matches!(
            &snap.phase,
            RecordPhase::Terminated {
                reason: TerminationReason::Timeout,
                fact: ExitFact::Code(0)
            }
        )
        .then_some(())
    })
    .await;
    // The second call AFTER the confirmation: echo the real fact and the
    // first cause.
    let r2 = supervisor
        .terminate(&spawned.id, TerminationReason::Close)
        .await;
    match &r2.outcome {
        TerminationOutcome::AlreadyTerminal(RecordPhase::Terminated { reason, fact }) => {
            assert!(matches!(reason, TerminationReason::Timeout), "{reason:?}");
            assert!(matches!(fact, ExitFact::Code(0)), "{fact:?}");
        }
        other => panic!("V3: the confirmed record echoes reality: {other:?}"),
    }
    // The slot returned exactly once (the settle happened at the
    // deferred observation, not before).
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match supervisor
            .spawn(oneshot_spec("v3-probe", vec!["/usr/bin/true".to_string()], &ws))
            .await
        {
            Ok(spawned) => {
                let _ = supervisor.wait_terminal(&spawned.id).await;
                break;
            }
            Err(SpawnFailure::RegistryFull) => assert!(
                Instant::now() < end,
                "the live slot was never returned after the confirmation"
            ),
            Err(other) => panic!("unexpected: {other:?}"),
        }
    }
    let grandchild = read_pid_file(&pid_file).await;
    kill_owned(grandchild);
    let _ = std::fs::remove_dir_all(&ws);
}

/// V4 (C04 controls, my own): exit 0, exit 7 and an OBSERVED SIGKILL —
/// each stated EXACTLY, never degraded to Unknown/StopUnconfirmed; the
/// observed-kill text carries no "[stop unconfirmed]" line.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn v4_controls_exit0_exit7_observed_kill_stay_exact() {
    let ws = test_dir("v4-ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let ctx = kernel_ctx("sess_v4", "run_v4");
    let mut kill_limits = limits_at("v4kill");
    kill_limits.cleanup_timeout = Duration::from_secs(2);
    kill_limits.stdio_grace = Duration::from_millis(150);
    let kill_supervisor = supervisor_with(kill_limits);
    let kill_tools = tools_over(&kill_supervisor, &ws);

    // exit 0
    let ok = kill_tools
        .run_exec_command(&ctx, &ToolCallId::new("c0".to_string()), &json!({"cmd": "echo V4_OK"}))
        .await;
    assert!(matches!(status_of(&ok), Some(ToolRunStatus::Exited { code: 0 })), "{:?}", status_of(&ok));
    // exit 7
    let seven = kill_tools
        .run_exec_command(&ctx, &ToolCallId::new("c7".to_string()), &json!({"cmd": "exit 7"}))
        .await;
    assert!(matches!(status_of(&seven), Some(ToolRunStatus::Exited { code: 7 })), "{:?}", status_of(&seven));
    let text7 = text_of(&seven);
    assert!(text7.contains("Command exited with code 7"), "{text7}");
    assert!(!text7.contains("stop unconfirmed"), "{text7}");
    // observed SIGKILL -> legitimate 137
    let killed = kill_tools
        .run_exec_command(
            &ctx,
            &ToolCallId::new("ck".to_string()),
            &json!({"cmd": "sleep 300", "timeout_seconds": 1}),
        )
        .await;
    match status_of(&killed) {
        Some(ToolRunStatus::Exited { code }) => assert_eq!(code, 137),
        other => panic!("the observed kill stays an exact exit: {other:?}"),
    }
    let textk = text_of(&killed);
    assert!(textk.contains("Command timed out after 1 seconds"), "{textk}");
    assert!(!textk.contains("stop unconfirmed"), "{textk}");
    let snap = kill_supervisor
        .record(
            &kill_supervisor
                .retained_record_ids()
                .into_iter()
                .next()
                .expect("retained"),
        )
        .expect("record");
    assert!(
        snap.audit.iter().any(|e| e.kind == "exit_observed"),
        "the 137 is observation-backed"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

// ── V5: the late real observation must NOT upgrade the cancelled run's
// journal (the two facts stay separate on BOTH timelines).

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
            .unwrap_or_else(|| final_turn("v5 done"));
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

/// V5 (C05 variant): after the run is cancelled with the process stop
/// unconfirmed, the journal holds Started-without-receipt; when the
/// DEFERRED real observation later lands on the process record, the
/// journal must NOT be rewritten into a success — the run's control
/// flow and the process's external fact stay separate on both ends of
/// the timeline.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn v5_c05_variant_late_observation_does_not_upgrade_the_cancelled_run_journal() {
    let mut limits = limits_at("v5");
    limits.cleanup_timeout = Duration::from_millis(40);
    limits.stdio_grace = Duration::from_millis(150);
    let provider = RunIdCaptureProvider::new(vec![
        tool_request_turn(
            "tool:first-party:exec_command",
            json!({"cmd": "echo V5_MARKER; sleep 300"}),
        ),
        final_turn("v5 done"),
    ]);
    // harness
    let home = test_dir("v5-home");
    let root = test_dir("v5-root");
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
    supervisor.arm_reap_fault_for_verification(ReapFaultPoint::DelayObservationNext);
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
        turn_provider: Some(provider.clone()),
        tool_gateway: Some(Arc::clone(&gateway)),
        approval_gate: Some(Arc::clone(&approvals) as Arc<dyn approval::ApprovalGate>),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");

    let run_driver = {
        let state = state.clone();
        let principal = owner_principal();
        tokio::spawn(async move {
            state
                .sessions()
                .execute_for(
                    state.storage().as_ref(),
                    state.events(),
                    state.runs(),
                    &principal,
                    "sess_local_alpha",
                    "V5 cancel leg",
                    1_790_409_600_000,
                )
                .await
                .expect("user run drives")
                .run_id
        })
    };
    let handle = wait_until("live process record", Duration::from_secs(20), || {
        supervisor.live_handles().first().cloned()
    })
    .await;
    let run_id = wait_until("trusted run id", Duration::from_secs(20), || {
        provider.run_id()
    })
    .await;
    let fired = state
        .runs()
        .cancel_run(run_id.as_str(), "v5 reviewer cancel");
    assert!(
        matches!(
            fired,
            lingxi_service::cancel::FireOutcome::Fired
                | lingxi_service::cancel::FireOutcome::AlreadyCancelling
        ),
        "{fired:?}"
    );
    let settled = run_driver.await.expect("run settles");
    assert_eq!(settled, run_id);
    let journal_before: Vec<InvocationJournalEntry> = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal");
    let entry_before = journal_before
        .iter()
        .find(|e| e.target.ends_with("exec_command"))
        .expect("exec entry");
    assert!(
        matches!(entry_before.phase, InvocationPhase::Started),
        "{:?}",
        entry_before.phase
    );
    assert!(entry_before.receipt.is_none());
    // The deferred real observation lands on the PROCESS record.
    wait_until("deferred real exit", Duration::from_secs(6), || {
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
    // THE ADDED CHECK: the journal was NOT rewritten into a success.
    let journal_after: Vec<InvocationJournalEntry> = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal reload");
    let entry_after = journal_after
        .iter()
        .find(|e| e.target.ends_with("exec_command"))
        .expect("exec entry after");
    assert!(
        matches!(entry_after.phase, InvocationPhase::Started),
        "the late observation did not upgrade the run's journal: {:?}",
        entry_after.phase
    );
    assert!(
        entry_after.receipt.is_none(),
        "no receipt materialized for the cancelled call after the late observation"
    );
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&root);
}

/// V6 (journal, success direction): a COMPLETED tool call whose process
/// stop is unconfirmed journals the honest receipt (Succeeded with a
/// "stop unconfirmed" detail that never reads as an exit); when the
/// deferred real observation later lands on the process record, the
/// journal is NOT rewritten — the written receipt stays the unconfirmed
/// one, and no late "exit" wording appears.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn v6_journal_success_stop_unconfirmed_not_upgraded_by_late_observation() {
    let mut limits = limits_at("v6");
    limits.cleanup_timeout = Duration::from_millis(2);
    limits.stdio_grace = Duration::from_millis(1500);
    let ws_dir = test_dir("v6-ws");
    let pid_file = ws_dir.join("grandchild.pid");
    std::fs::create_dir_all(&ws_dir).expect("cmd cwd");
    let provider = RunIdCaptureProvider::new(vec![
        tool_request_turn(
            "tool:first-party:exec_command",
            json!({
                "cmd": format!(
                    "( sleep 300 ) & printf '%s\\n' \"$!\" > {}; echo V6_MARKER; sleep 0.5",
                    pid_file.display()
                ),
                "timeout_seconds": 1
            }),
        ),
        final_turn("v6 done"),
    ]);
    let home = test_dir("v6-home");
    let root = test_dir("v6-root");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("workspace");
    std::fs::create_dir_all(&ws_dir).expect("cmd cwd");
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
        turn_provider: Some(provider.clone()),
        tool_gateway: Some(Arc::clone(&gateway)),
        approval_gate: Some(Arc::clone(&approvals) as Arc<dyn approval::ApprovalGate>),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let principal = owner_principal();
    let run_id = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal,
            "sess_local_alpha",
            "V6 unconfirmed receipt leg",
            1_790_409_600_000,
        )
        .await
        .expect("run drives")
        .run_id
        .to_string();
    let handle = supervisor
        .live_handles()
        .into_iter()
        .next()
        .or_else(|| {
            supervisor
                .retained_record_ids()
                .into_iter()
                .next()
        })
        .expect("a process record exists");
    // The receipt exists and is the honest unconfirmed one.
    let journal: Vec<InvocationJournalEntry> = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal");
    let entry = journal
        .iter()
        .find(|e| e.target.ends_with("exec_command"))
        .expect("exec entry");
    let receipt = entry.receipt.as_ref().expect("the run completed");
    assert!(
        matches!(receipt.outcome, lingxi_kernel::ports::ReceiptOutcome::Succeeded),
        "the TOOL call completed: {:?}",
        receipt.outcome
    );
    let detail_before = receipt.detail.clone();
    assert!(
        detail_before.contains("stop unconfirmed"),
        "the detail states the unconfirmed fact: {detail_before}"
    );
    // The Exited arm's format is "(exit {code})" — none may appear.
    assert!(
        !detail_before.contains("(exit "),
        "the detail never reads as an observed exit: {detail_before}"
    );
    // The deferred real observation lands on the PROCESS record.
    wait_until("deferred real exit", Duration::from_secs(6), || {
        let snap = supervisor.record(&handle)?;
        matches!(
            &snap.phase,
            RecordPhase::Terminated {
                reason: TerminationReason::Timeout,
                fact: ExitFact::Code(0)
            }
        )
        .then_some(())
    })
    .await;
    // The journal was NOT rewritten.
    let journal_after: Vec<InvocationJournalEntry> = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal reload");
    let entry_after = journal_after
        .iter()
        .find(|e| e.target.ends_with("exec_command"))
        .expect("exec entry after");
    let receipt_after = entry_after.receipt.as_ref().expect("receipt persists");
    assert_eq!(
        receipt_after.detail, detail_before,
        "the late observation did not rewrite the written receipt"
    );
    let grandchild = read_pid_file(&pid_file).await;
    kill_owned(grandchild);
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&ws_dir);
}
