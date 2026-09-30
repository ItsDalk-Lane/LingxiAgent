//! R04-T06 acceptance: cross-platform sandbox and escape protection
//! (R04-A11 / R04-A12 + the adversarial additions).
//!
//! Everything here runs against the REAL composition root
//! (`ServiceState::bootstrap_with_deps`), the REAL registry/gateway, the
//! REAL T05 `ProcessSupervisor` (POSIX process groups) and the REAL
//! [`lingxi_service::sandbox`] face bound through
//! `register_process_tools`. Every allow/deny assertion is a REAL OS
//! observation:
//!
//! - filesystem boundaries are probed by REAL commands writing/reading
//!   REAL sentinel files inside and OUTSIDE the authorized roots
//!   (created by this suite; never user files);
//! - network boundaries are probed against a loopback service THIS TEST
//!   creates and registers (ephemeral 127.0.0.1 port; no other target);
//! - the environment boundary is probed with a planted server secret;
//! - helper trust is probed with a REAL impostor helper (`exec "$@"`) —
//!   had the executor fallen back to a bare run, the sentinel WOULD have
//!   been modified; the assertion proves it was not.
//!
//! Test-double boundary: the turn provider is the suite's scripted
//! external model; no double replaces the sandbox, the gateway, the
//! policy derivation, the supervisor or the executor.
//!
//! Platform note: the seatbelt legs run on the macOS host (the frozen
//! real-machine verification). The unsupported-backend legs run
//! everywhere and pin the fail-closed shape of platforms whose incumbent
//! helper is not ported (Windows restricted-token; R03-WINDOWS-R09-R10
//! deferral).

use std::collections::VecDeque;
use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult, ToolOutcome,
    ToolRequest, TurnProviderPort,
};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetId};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, ToolCallId};
use lingxi_service::approval_service::ApprovalService;
use lingxi_service::exectools::register_process_tools;
use lingxi_service::procsupervisor::{
    ProcessSupervisor, SpawnSpec, SupervisorLimits, TerminationReason,
};
use lingxi_service::resourceaccess::ResourceAccess;
use lingxi_service::sandbox::{
    platform_sandbox, SandboxCommandRequest, SandboxNetworkPolicy, SandboxNetworkRequest,
    SandboxPolicy, SandboxPolicyInput, SandboxPort, SeatbeltSandbox, UnsupportedSandbox,
};
use lingxi_service::toolgateway::{
    GatewayRefusal, InvocationPermissionContext, ToolInvocationGateway,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::json;

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

// ── the scripted external model (double boundary: responses only) ─────────

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

struct IdleProvider {
    steps: std::sync::Mutex<VecDeque<ProviderTurn>>,
}

impl IdleProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            steps: std::sync::Mutex::new(VecDeque::new()),
        })
    }
}

impl TurnProviderPort for IdleProvider {
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

// ── harness ────────────────────────────────────────────────────────────────

/// A unique test directory (pid + nanos + sequence — never a shared /tmp
/// wildcard). The base is cargo's `CARGO_TARGET_TMPDIR` (repo-local build
/// scratch): the OS `$TMPDIR` and `/private/tmp` are legitimately writable
/// under the incumbent sandbox contract, so rooting the test tree there
/// would make every allow/deny contrast degenerate — the only things the
/// sandbox should allow here are the roots the frozen policy names.
fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "lingxi-r04t06-{tag}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        seq
    ))
}

struct Harness {
    #[allow(dead_code)]
    state: ServiceState,
    gateway: Arc<ToolInvocationGateway>,
    supervisor: Arc<ProcessSupervisor>,
    exec_target: ToolTargetId,
    #[allow(dead_code)]
    sandbox: Arc<dyn SandboxPort>,
    root: PathBuf,
    ws: PathBuf,
    home: PathBuf,
    /// The bootstrap data home (service storage) — removed at teardown.
    bootstrap_home: PathBuf,
    #[allow(dead_code)]
    agent_dir: PathBuf,
    restricted: PathBuf,
}

impl Harness {
    fn sentinel(&self) -> PathBuf {
        self.restricted.join("sentinel.txt")
    }
}

async fn teardown(h: &Harness) {
    let _ = h.supervisor.shutdown_all().await;
    h.state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&h.root);
    let _ = std::fs::remove_dir_all(&h.restricted);
    let _ = std::fs::remove_dir_all(&h.bootstrap_home);
}

/// Creates the isolated test root: `ws` (the authorized workspace) and
/// `home`/`home/agents/hana` (the Lingxi home tree with `auth.json`) under
/// the caller's root, plus the `restricted` sentinel area OUTSIDE every
/// writable root. The restricted area deliberately lives under cargo's
/// `CARGO_TARGET_TMPDIR` (build-artifact scratch, NOT the OS `$TMPDIR`):
/// the incumbent sandbox contract allows writes to `/private/tmp` and
/// `$TMPDIR`, so a "restricted" sentinel placed under `$TMPDIR` would be
/// legitimately writable — the escape probe must sit outside that set.
fn make_root(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let ws = root.join("ws");
    let restricted = root.join("restricted");
    let home = root.join("home");
    let agent_dir = home.join("agents/hana");
    std::fs::create_dir_all(&ws).expect("ws");
    std::fs::create_dir_all(&restricted).expect("restricted");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    std::fs::create_dir_all(ws.join(".git")).expect("ws/.git (protected probe)");
    std::fs::write(restricted.join("sentinel.txt"), b"TOP SECRET SENTINEL").expect("sentinel");
    std::fs::write(home.join("auth.json"), b"AUTH-SECRET-7f3a").expect("auth.json secret");
    (ws, restricted, home, agent_dir)
}

fn sandbox_policy(home: &Path, agent_dir: &Path, ws: &Path) -> SandboxPolicy {
    SandboxPolicy::derive(&SandboxPolicyInput {
        lingxi_home: home.to_path_buf(),
        agent_dir: agent_dir.to_path_buf(),
        workspace_roots: vec![ws.to_path_buf()],
        runtime_writable_paths: vec![],
        network: SandboxNetworkPolicy::Allowed,
    })
    .expect("policy derives")
}

/// The full composition: bootstrap + registry + gateway + supervisor +
/// process tools bound behind the sandbox face. The harness REUSES the
/// caller's root — the sandbox policy and the resource layer must judge
/// the SAME workspace (constraint intersection, not two worlds).
async fn harness_with(root: &Path, sandbox: Arc<dyn SandboxPort>) -> Harness {
    let (ws, restricted, home, agent_dir) = make_root(root);
    let home_dir = test_dir("home");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home_dir.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home_dir).expect("layout");
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
        ProcessSupervisor::new(
            Arc::new(lingxi_service::inject::SystemClock),
            SupervisorLimits::with_spill_dir(test_dir("spill")),
        )
        .expect("supervisor"),
    );
    let core = register_process_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&supervisor),
        Arc::clone(&access),
        ws.clone(),
        Some(Arc::clone(&sandbox)),
        Arc::new(lingxi_service::inject::SystemClock),
        &budget(),
    );
    let deps = ServiceDeps {
        turn_provider: Some(IdleProvider::new()),
        tool_gateway: Some(Arc::clone(&gateway)),
        approval_gate: Some(
            Arc::clone(&approvals) as Arc<dyn lingxi_service::approval::ApprovalGate>
        ),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    Harness {
        state,
        gateway,
        supervisor,
        exec_target: core.exec_target,
        sandbox,
        root: root.to_path_buf(),
        ws,
        home,
        bootstrap_home: home_dir,
        agent_dir,
        restricted,
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
/// the run driver performs.
async fn call_exec(
    h: &Harness,
    ctx: &RunContext,
    call: &str,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, GatewayRefusal> {
    let request = ToolRequest::from_effective_arguments(h.exec_target.as_str(), args, &budget())
        .expect("effective request");
    let call_id = ToolCallId::new(call.to_string());
    let prepared = h.gateway.prepare_from_request(
        ctx,
        lingxi_service::toolgateway::CallerSurface::UserRun,
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
            .collect(),
        ToolOutcome::Failed { error } => format!("<failed: {}>", error.message),
        outcome => format!("<{outcome:?}>"),
    }
}

fn failed_text_of(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Failed { error } => error.message.clone(),
        outcome => panic!("expected a failed outcome, got {outcome:?}"),
    }
}

/// The real helper-backed sandbox (macOS: seatbelt at the pinned system
/// path — the production shape).
fn real_platform_sandbox(home: &Path, agent_dir: &Path, ws: &Path) -> Arc<dyn SandboxPort> {
    match platform_sandbox(sandbox_policy(home, agent_dir, ws), None) {
        Ok(port) => port,
        Err(refusal) => panic!("the platform sandbox must compose on this host: {refusal}"),
    }
}

// ── R04-A11: a missing/untrusted sandbox NEVER becomes a bare run ──────────

#[tokio::test]
async fn r04_a11_missing_helper_at_composition_is_a_loud_refusal() {
    // A test installation whose helper path does not exist: composing the
    // sandbox fails with the diagnosable HelperMissing code — there is no
    // constructor path that yields a "sandbox" that runs commands bare.
    let root = test_dir("a11-missing");
    let (ws, _restricted, home, agent_dir) = make_root(&root);
    let refusal = SeatbeltSandbox::new(
        sandbox_policy(&home, &agent_dir, &ws),
        Some(root.join("no-such-helper/sandbox-exec")),
    )
    .err()
    .expect("composition must fail");
    assert_eq!(refusal.code(), "sandbox_helper_missing");
    assert!(
        format!("{refusal}").contains("never run unsandboxed"),
        "{refusal}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn r04_a11_replaced_helper_is_refused_and_the_impostor_never_runs() {
    // The impostor WOULD happily run any command (`exec "$@"`). Two legs:
    // 1. composing against the impostor fails the trust scheme (owner is
    //    not root — a replaced/mismatched helper);
    // 2. mid-flight replacement through a legitimate-looking alias: a
    //    helper alias that passed construction is swapped for the
    //    impostor AFTER construction — the per-wrap re-verification
    //    refuses THROUGH THE REAL EXEC CHAIN, the sentinel is untouched
    //    and nothing is left running.
    let root = test_dir("a11-impostor");
    let (ws, _restricted, home, agent_dir) = make_root(&root);
    let helper_dir = root.join("helper-install");
    std::fs::create_dir_all(&helper_dir).expect("helper dir");
    let helper_path = helper_dir.join("sandbox-exec");
    let impostor = b"#!/bin/sh\nexec \"$@\"\n";

    // Leg 1: an impostor at the helper path fails composition.
    std::fs::write(&helper_path, impostor).expect("impostor");
    make_executable(&helper_path);
    let refusal = SeatbeltSandbox::new(
        sandbox_policy(&home, &agent_dir, &ws),
        Some(helper_path.clone()),
    )
    .err()
    .expect("impostor composition must fail");
    assert_eq!(refusal.code(), "sandbox_helper_untrusted");
    assert!(format!("{refusal}").contains("not root"), "{refusal}");

    // Leg 2: a legit alias (symlink → the real system helper) composes and
    // runs; swapping it for the impostor mid-flight must refuse at wrap.
    std::fs::remove_file(&helper_path).expect("clear impostor");
    std::os::unix::fs::symlink("/usr/bin/sandbox-exec", &helper_path).expect("alias link");
    let port: Arc<dyn SandboxPort> = Arc::new(
        SeatbeltSandbox::new(
            sandbox_policy(&home, &agent_dir, &ws),
            Some(helper_path.clone()),
        )
        .expect("alias composes"),
    );
    let h = harness_with(&root, port).await;
    let ctx = kernel_ctx("sess_a11", "run_a11");
    // Sanity: the alias really wraps (a contained command runs).
    let ok = call_exec(
        &h,
        &ctx,
        "call-a11-ok",
        json!({"argv": ["/bin/sh", "-c", "echo probe-ok"]}),
    )
    .await
    .expect("alias leg runs");
    assert!(text_of(&ok).contains("probe-ok"), "{}", text_of(&ok));
    assert!(
        text_of(&ok).contains("sandbox: seatbelt"),
        "{}",
        text_of(&ok)
    );

    // Mid-flight swap: alias → impostor (the "mismatched version" shape).
    std::fs::remove_file(&helper_path).expect("drop alias");
    std::fs::write(&helper_path, impostor).expect("install impostor");
    make_executable(&helper_path);
    let sentinel_before = std::fs::read(h.sentinel()).expect("sentinel readable");
    let refusal_result = call_exec(
        &h,
        &ctx,
        "call-a11-refuse",
        json!({"argv": ["/bin/sh", "-c", format!("echo hacked > {}", h.sentinel().display())]}),
    )
    .await
    .expect("the executor answers (a failed outcome, not a gateway panic)");
    let text = failed_text_of(&refusal_result);
    assert!(text.contains("EXEC_SANDBOX_REFUSED"), "{text}");
    assert!(text.contains("sandbox_helper_untrusted"), "{text}");
    // The impostor NEVER ran: the sentinel is byte-identical and no
    // process is left live (fail-closed, zero side effects).
    assert_eq!(
        std::fs::read(h.sentinel()).expect("sentinel readable"),
        sentinel_before,
        "the sentinel must be untouched — the impostor never executed"
    );
    assert!(h.supervisor.live_handles().is_empty());
    // Leg 2b: removing the helper entirely is the same fail-closed shape.
    std::fs::remove_file(&helper_path).expect("remove helper");
    let missing = call_exec(
        &h,
        &ctx,
        "call-a11-missing",
        json!({"argv": ["/bin/sh", "-c", "echo nope"]}),
    )
    .await
    .expect("executor answers");
    let missing_text = failed_text_of(&missing);
    assert!(
        missing_text.contains("EXEC_SANDBOX_REFUSED")
            && missing_text.contains("sandbox_helper_missing"),
        "{missing_text}"
    );
    teardown(&h).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn r04_a11_unsupported_backend_refuses_through_the_real_chain() {
    // The platform whose incumbent helper is not ported (Windows
    // restricted-token in R04): binding the refusing backend means every
    // must-isolate command is REFUSED at the executor — never a bare run.
    let unsupported_root = test_dir("a11-unsupported");
    let h = harness_with(
        &unsupported_root,
        Arc::new(UnsupportedSandbox::new("test: no backend")),
    )
    .await;
    let ctx = kernel_ctx("sess_a11u", "run_a11u");
    let sentinel_before = std::fs::read(h.sentinel()).expect("sentinel");
    let result = call_exec(
        &h,
        &ctx,
        "call-unsupported",
        json!({"argv": ["/bin/sh", "-c", format!("echo hacked > {}", h.sentinel().display())]}),
    )
    .await
    .expect("executor answers");
    let text = failed_text_of(&result);
    assert!(text.contains("EXEC_SANDBOX_REFUSED"), "{text}");
    assert!(text.contains("sandbox_policy_unsupported"), "{text}");
    assert_eq!(
        std::fs::read(h.sentinel()).expect("sentinel"),
        sentinel_before
    );
    assert!(h.supervisor.live_handles().is_empty());
    teardown(&h).await;
    let _ = std::fs::remove_dir_all(&unsupported_root);
}

// ── R04-A12: the isolation guarantees ACTUALLY hold ────────────────────────

#[tokio::test]
async fn r04_a12_filesystem_write_isolation_holds_with_real_sentinels() {
    let root = test_dir("a12-fs");
    let (ws, _restricted, home, agent_dir) = make_root(&root);
    let sandbox = real_platform_sandbox(&home, &agent_dir, &ws);
    let h = harness_with(&root, sandbox).await;
    let ctx = kernel_ctx("sess_a12fs", "run_a12fs");

    // Allowed contrast: writing INSIDE the workspace succeeds.
    let inside = h.ws.join("inside.txt");
    let ok = call_exec(
        &h,
        &ctx,
        "call-ok",
        json!({"argv": ["/bin/sh", "-c", format!("echo allowed-ok > {}", inside.display())]}),
    )
    .await
    .expect("allowed leg runs");
    assert_eq!(
        std::fs::read_to_string(&inside).expect("inside written"),
        "allowed-ok\n"
    );
    assert!(
        text_of(&ok).contains("sandbox: seatbelt"),
        "{}",
        text_of(&ok)
    );

    // Restricted: writing OUTSIDE every root is denied by the OS sandbox —
    // the sentinel survives byte-identical.
    let sentinel = h.sentinel();
    let denied = call_exec(
        &h,
        &ctx,
        "call-denied",
        json!({"argv": ["/bin/sh", "-c", format!("echo hacked > {}", sentinel.display())]}),
    )
    .await
    .expect("denied leg answers");
    let denied_text = text_of(&denied);
    assert!(
        denied_text.contains("Command exited with code"),
        "the write must fail: {denied_text}"
    );
    assert!(
        denied_text.contains("Operation not permitted"),
        "{denied_text}"
    );
    assert!(
        denied_text.contains("[Security] File system writes are restricted"),
        "{denied_text}"
    );
    assert_eq!(
        std::fs::read(&sentinel).expect("sentinel"),
        b"TOP SECRET SENTINEL"
    );

    // Absolute path + symlink redirection: a link INSIDE the workspace
    // pointing at the restricted file — writing through it is judged on
    // the real target and denied.
    let link = h.ws.join("escape-link.txt");
    std::os::unix::fs::symlink(h.restricted.join("sentinel.txt"), &link).expect("link");
    let via_link = call_exec(
        &h,
        &ctx,
        "call-link",
        json!({"argv": ["/bin/sh", "-c", format!("echo hacked > {}", link.display())]}),
    )
    .await
    .expect("link leg answers");
    assert!(
        text_of(&via_link).contains("Command exited with code"),
        "{}",
        text_of(&via_link)
    );
    assert_eq!(
        std::fs::read(&sentinel).expect("sentinel"),
        b"TOP SECRET SENTINEL"
    );

    // Protected path INSIDE the writable set: `ws/.git` is write-denied
    // (deny overrides allow; last match wins).
    let git_config = h.ws.join(".git/config");
    let git_denied = call_exec(
        &h,
        &ctx,
        "call-git",
        json!({"argv": ["/bin/sh", "-c", format!("echo hacked > {}", git_config.display())]}),
    )
    .await
    .expect("git leg answers");
    assert!(
        text_of(&git_denied).contains("Command exited with code"),
        "{}",
        text_of(&git_denied)
    );
    assert!(
        !git_config.exists(),
        "the protected path must not be created"
    );

    // Deny-read: the Lingxi-home secret file is unreadable INSIDE the
    // sandbox (global read-all minus the deny list).
    let auth = h.home.join("auth.json");
    let read_denied = call_exec(
        &h,
        &ctx,
        "call-auth",
        json!({"argv": ["/bin/cat", auth.display().to_string()]}),
    )
    .await
    .expect("auth leg answers");
    let auth_text = text_of(&read_denied);
    assert!(
        !auth_text.contains("AUTH-SECRET-7f3a"),
        "the secret must be unreadable: {auth_text}"
    );
    assert!(
        auth_text.contains("Command exited with code"),
        "{auth_text}"
    );

    // Temp resources: writing TMPDIR is allowed (the incumbent contract).
    let tmp_probe = std::env::temp_dir().join(format!(
        "lingxi-t06-tmpprobe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let tmp_ok = call_exec(
        &h,
        &ctx,
        "call-tmp",
        json!({"argv": ["/bin/sh", "-c", format!("echo tmp-ok > {}", tmp_probe.display())]}),
    )
    .await
    .expect("tmp leg runs");
    assert_eq!(
        std::fs::read_to_string(&tmp_probe)
            .unwrap_or_else(|_| panic!("tmp probe written; result: {}", text_of(&tmp_ok))),
        "tmp-ok\n"
    );
    let _ = std::fs::remove_file(&tmp_probe);

    // Subprocess: forking INSIDE the sandbox works (process-exec* allowed)
    // — the containment story does not break legitimate child processes.
    let sub = call_exec(
        &h,
        &ctx,
        "call-sub",
        json!({"argv": ["/bin/sh", "-c", "/bin/echo child-ok"]}),
    )
    .await
    .expect("subprocess leg runs");
    assert!(text_of(&sub).contains("child-ok"), "{}", text_of(&sub));

    assert!(h.supervisor.live_handles().is_empty());
    teardown(&h).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn r04_a12_network_isolation_against_the_registered_loopback_service() {
    // The loopback service THIS TEST creates (ephemeral 127.0.0.1 port,
    // answering `pong`); the probes touch nothing else. The accept loop is
    // nonblocking with a hard deadline + stop flag — a probe that never
    // connects can never wedge the suite.
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback bind");
    let port = listener.local_addr().expect("addr").port();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let server = std::thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while !stop_flag.load(std::sync::atomic::Ordering::Relaxed)
            && std::time::Instant::now() < deadline
        {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(true);
                    let _ = stream.write_all(b"pong");
                    let _ = stream.flush();
                }
                Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return,
            }
        }
    });

    let root = test_dir("a12-net");
    let (ws, _restricted, home, agent_dir) = make_root(&root);
    let sandbox = real_platform_sandbox(&home, &agent_dir, &ws);
    let h = harness_with(&root, Arc::clone(&sandbox)).await;
    let ctx = kernel_ctx("sess_a12net", "run_a12net");

    // Contained (the model-facing one-shot path): the connection is
    // DENIED by the sandbox — `nc` never receives the pong.
    let contained = call_exec(
        &h,
        &ctx,
        "call-net-contained",
        json!({"argv": ["/usr/bin/nc", "-w", "2", "127.0.0.1", port.to_string()]}),
    )
    .await
    .expect("contained leg answers");
    let contained_text = text_of(&contained);
    assert!(
        !contained_text.contains("pong"),
        "the contained sandbox must not reach the network: {contained_text}"
    );
    assert!(
        contained_text.contains("Command exited with code"),
        "the connection must fail loudly: {contained_text}"
    );

    // Allowed contrast through the REAL supervisor: the SAME policy with
    // the network-capable variant DOES reach the service (proving the
    // deny above is the policy, not a broken probe).
    let wrapped = sandbox
        .wrap(SandboxCommandRequest {
            argv: vec![
                "/usr/bin/nc".to_string(),
                "-w".to_string(),
                "2".to_string(),
                "127.0.0.1".to_string(),
                port.to_string(),
            ],
            network: SandboxNetworkRequest::NetworkCapable,
            cwd: Some(h.ws.clone()),
        })
        .expect("network-capable wrap");
    assert!(wrapped.network_allowed);
    let spec = SpawnSpec {
        argv: wrapped.argv,
        cwd: h.ws.clone(),
        env: lingxi_service::exectools::build_child_env(&Default::default()),
        owner: net_probe_owner(),
        kind: lingxi_service::procsupervisor::ProcessKind::OneShot,
        cols: 80,
        rows: 24,
    };
    let spawned = h.supervisor.spawn(spec).await.expect("net-capable spawn");
    let phase = h
        .supervisor
        .wait_terminal(&spawned.id)
        .await
        .expect("terminal phase");
    let snapshot = h.supervisor.output_snapshot(&spawned.id).expect("snapshot");
    let out = String::from_utf8_lossy(&snapshot.window).into_owned();
    assert!(
        out.contains("pong"),
        "the network-capable leg must reach the service: {out} (phase {phase:?})"
    );
    assert!(h.supervisor.live_handles().is_empty());

    // A network-denied policy refuses the network-capable variant outright
    // (unsupported request → fail-closed, no weaker fallback).
    let denied_policy = SandboxPolicy::derive(&SandboxPolicyInput {
        lingxi_home: home.clone(),
        agent_dir: agent_dir.clone(),
        workspace_roots: vec![ws.clone()],
        runtime_writable_paths: vec![],
        network: SandboxNetworkPolicy::Denied,
    })
    .expect("denied policy");
    let denied_sandbox = SeatbeltSandbox::new(denied_policy, None).expect("compose");
    let refusal = denied_sandbox
        .wrap(SandboxCommandRequest {
            argv: vec!["/usr/bin/nc".to_string()],
            network: SandboxNetworkRequest::NetworkCapable,
            cwd: Some(h.ws.clone()),
        })
        .unwrap_err();
    assert_eq!(refusal.code(), "sandbox_policy_unsupported");

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    server.join().expect("server thread");
    teardown(&h).await;
    let _ = std::fs::remove_dir_all(&root);
}

fn net_probe_owner() -> lingxi_service::procsupervisor::ProcessOwner {
    lingxi_service::procsupervisor::ProcessOwner {
        principal_kind: "local_user".to_string(),
        principal_subject: "principal_local".to_string(),
        session_id: "sess_a12net".to_string(),
        run_id: "run_a12net".to_string(),
        tool_call_id: ToolCallId::new("call-net-capable".to_string()),
    }
}

#[tokio::test]
async fn r04_a12_environment_whitelist_holds_through_the_sandbox_wrapper() {
    // A planted server secret must not leak into the sandboxed child; the
    // whitelisted vars and explicit entries do pass (T05 whitelist remains
    // the single env funnel — the sandbox adds no second passthrough).
    // SAFETY: test setup before any threads read the environment.
    unsafe {
        std::env::set_var("LINGXI_T06_SECRET", "t06-secret-value-do-not-leak");
    }
    let root = test_dir("a12-env");
    let (ws, _restricted, home, agent_dir) = make_root(&root);
    let sandbox = real_platform_sandbox(&home, &agent_dir, &ws);
    let h = harness_with(&root, sandbox).await;
    let ctx = kernel_ctx("sess_a12env", "run_a12env");
    let result = call_exec(
        &h,
        &ctx,
        "call-env",
        json!({
            "argv": ["/usr/bin/env"],
            "env": {"LINGXI_T06_MARKER": "42"}
        }),
    )
    .await
    .expect("env leg runs");
    let text = text_of(&result);
    assert!(text.contains("LINGXI_T06_MARKER=42"), "{text}");
    assert!(text.contains("PATH="), "{text}");
    assert!(
        !text.contains("t06-secret-value-do-not-leak") && !text.contains("LINGXI_T06_SECRET"),
        "the server secret must never reach the sandboxed child: {text}"
    );
    assert!(text.contains("sandbox: seatbelt"), "{text}");
    teardown(&h).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn r04_a12_sandboxed_process_tree_still_honors_the_cancellation_chain() {
    // The supervisor's setsid/killpg chain must keep working THROUGH the
    // sandbox wrapper (sandbox-exec execs in place, so the group survives)
    // — cancelling a sandboxed tree really kills the grandchildren.
    let root = test_dir("a12-tree");
    let (ws, _restricted, home, agent_dir) = make_root(&root);
    let sandbox = real_platform_sandbox(&home, &agent_dir, &ws);
    let h = harness_with(&root, sandbox).await;
    let ctx = kernel_ctx("sess_a12tree", "run_a12tree");
    let gc_pid_file = h.ws.join("gc.pid");
    let result = call_exec(
        &h,
        &ctx,
        "call-tree",
        json!({
            "argv": ["/bin/sh", "-c",
                     format!("sleep 30 & echo $! > {}; wait", gc_pid_file.display())],
            "timeout_seconds": 2
        }),
    )
    .await
    .expect("tree leg answers");
    // The timeout watchdog ran the supervisor's bounded termination chain
    // (killpg through the sandbox wrapper).
    let text = text_of(&result);
    assert!(text.contains("Command timed out after 2 seconds"), "{text}");
    // The grandchild published its pid before the kill; it must be GONE
    // (killpg reached it through sandbox-exec — no survivor behind the
    // sandbox wrapper).
    let gc_pid: i32 = std::fs::read_to_string(&gc_pid_file)
        .expect("the grandchild published its pid")
        .trim()
        .parse()
        .expect("pid integer");
    let mut gone = false;
    for _ in 0..20 {
        // SAFETY: probe a process this test created.
        if unsafe { libc::kill(gc_pid, 0) } != 0 {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        gone,
        "the sandboxed grandchild must be killed by the group kill"
    );
    assert!(h.supervisor.live_handles().is_empty());
    teardown(&h).await;
}

// ── adversarial additions ──────────────────────────────────────────────────

#[tokio::test]
async fn adversarial_policy_injection_is_refused_at_both_layers() {
    let root = test_dir("adv-inject");
    let (ws, _restricted, home, agent_dir) = make_root(&root);

    // Layer 1 — profile compilation: a workspace root containing a quote
    // cannot be embedded in the SBPL literal; the command is refused.
    let quote_root = root.join("we\"ird-ws");
    std::fs::create_dir_all(&quote_root).expect("quote root");
    let policy = SandboxPolicy::derive(&SandboxPolicyInput {
        lingxi_home: home.clone(),
        agent_dir: agent_dir.clone(),
        workspace_roots: vec![quote_root.clone()],
        runtime_writable_paths: vec![],
        network: SandboxNetworkPolicy::Allowed,
    })
    .expect("policy derives (lexical)");
    let sandbox = SeatbeltSandbox::new(policy, None).expect("composes");
    let refusal = sandbox
        .wrap(SandboxCommandRequest {
            argv: vec!["/bin/sh".to_string(), "-c".to_string(), "true".to_string()],
            network: SandboxNetworkRequest::Contained,
            cwd: None,
        })
        .unwrap_err();
    assert_eq!(refusal.code(), "sandbox_profile_path_unsafe");

    // Layer 2 — the model's arguments cannot smuggle sandbox parameters:
    // the frozen exec schema rejects unknown keys at PREPARE (zero
    // dispatch), so a fake `sandbox_profile` never reaches the executor.
    let good = real_platform_sandbox(&home, &agent_dir, &ws);
    let h = harness_with(&root, good).await;
    let ctx = kernel_ctx("sess_adv_inj", "run_adv_inj");
    let refusal = call_exec(
        &h,
        &ctx,
        "call-inject",
        json!({
            "argv": ["/bin/sh", "-c", "true"],
            "sandbox_profile": "(allow network*)"
        }),
    )
    .await
    .unwrap_err();
    match refusal {
        GatewayRefusal::ArgumentsInvalid { violations } => {
            assert!(
                violations.iter().any(|v| v.contains("sandbox_profile")),
                "{violations:?}"
            );
        }
        other => panic!("expected ArgumentsInvalid, got {other:?}"),
    }
    assert!(h.supervisor.live_handles().is_empty());
    teardown(&h).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn adversarial_tty_terminals_are_honestly_not_claimed_as_contained() {
    // Incumbent semantics: interactive terminals run WITHOUT the OS
    // sandbox (`sandboxed = !tty && …`). The result must never claim
    // containment — the honest state.
    let root = test_dir("adv-tty");
    let (ws, _restricted, home, agent_dir) = make_root(&root);
    let sandbox = real_platform_sandbox(&home, &agent_dir, &ws);
    let h = harness_with(&root, sandbox).await;
    let ctx = kernel_ctx("sess_adv_tty", "run_adv_tty");
    let result = call_exec(
        &h,
        &ctx,
        "call-tty",
        json!({"argv": ["/bin/sleep", "30"], "tty": true}),
    )
    .await
    .expect("tty leg starts");
    let text = text_of(&result);
    assert!(text.contains("Interactive process started"), "{text}");
    assert!(
        !text.contains("sandbox:"),
        "an unsandboxed tty start must never claim containment: {text}"
    );
    // Cleanup: close the terminal we started (exact handle).
    let handle = h
        .supervisor
        .live_handles()
        .first()
        .cloned()
        .expect("live handle");
    let _ = h
        .supervisor
        .terminate(&handle, TerminationReason::Close)
        .await;
    assert!(h.supervisor.live_handles().is_empty());
    teardown(&h).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn seatbelt_helper_trust_and_capabilities_are_recorded() {
    let root = test_dir("trust");
    let (ws, _restricted, home, agent_dir) = make_root(&root);
    let sandbox = SeatbeltSandbox::new(sandbox_policy(&home, &agent_dir, &ws), None)
        .expect("the pinned system helper composes");
    let trust = sandbox.helper_trust().expect("trust recorded");
    assert_eq!(trust.owner_uid, 0, "the system helper is root-owned");
    assert_eq!(trust.resolved, PathBuf::from("/usr/bin/sandbox-exec"));
    // The enforcement probe passed implicitly (construction succeeded).
    let caps = sandbox.capabilities();
    assert_eq!(caps.backend, "seatbelt");
    assert_eq!(caps.filesystem_write, "scoped");
    assert_eq!(caps.network, "per-policy");
    // The wrapped argv really is the helper prefix + the command (no
    // accidental bare run in the wrapped form).
    let wrapped = sandbox
        .wrap(SandboxCommandRequest {
            argv: vec!["/bin/echo".to_string(), "x".to_string()],
            network: SandboxNetworkRequest::Contained,
            cwd: None,
        })
        .expect("wrap");
    assert_eq!(wrapped.argv[0], "/usr/bin/sandbox-exec");
    assert_eq!(wrapped.argv[1], "-p");
    assert_eq!(wrapped.argv.last().unwrap(), "x");
    assert!(wrapped.argv[2].contains("(deny network*)"));
    let _ = std::fs::remove_dir_all(&root);
}

// ── helpers ────────────────────────────────────────────────────────────────

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .expect("impostor metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("impostor executable");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
