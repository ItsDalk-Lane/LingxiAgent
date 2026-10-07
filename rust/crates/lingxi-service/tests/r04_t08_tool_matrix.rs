//! R04-T08 acceptance: the full tool matrix — tool families × permission
//! contexts × call routes × lifecycle states — plus the two base
//! scenarios R04-A15 (an empty/invalid product is never a success) and
//! R04-A16 (a disabled state covers EVERY route), and the unified
//! Success/Failed/Cancelled/Unknown outcome semantics with their journal
//! receipts.
//!
//! Everything runs against the REAL composition surfaces: the REAL
//! `ToolRegistry`/`ToolInvocationGateway` (T01/T02), the REAL
//! ApprovalService policy face (T03), the REAL native file tools /
//! process tools (T04/T05), the REAL rmcp stdio bridge (T07) and the
//! REAL worker RPC (T07) — all driven through `bootstrap_with_deps` run
//! chains where a run is involved. Test doubles only supply EXTERNAL
//! responses: the `StepsProvider` plays the (R05) model, the fixture
//! child processes play external workers/MCP servers, and the two tiny
//! inline executors below play external tools whose DELIVERED REFERENCES
//! the gateway's registration audit must judge. No double replaces the
//! gateway, the policy, the registry, the journal or the driver.
//!
//! Evidence output: when `R04_T08_EVIDENCE_DIR` is set, every pinned
//! case writes a fragment under `<dir>/cases/<case>.json` (schema
//! `lingxi.leaf-case-results.v1` is assembled by the producer script
//! `scripts/rust-tauri/r04_t08_matrix.sh`); each fragment is ASSERTED
//! in-process first — packaging never invents an observation.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolExecutionResult,
    ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess, TurnDeltaSink, TurnProviderPort,
};
use lingxi_kernel::subagent::{SessionPermissionMode, ToolAccessTier};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry, ToolTargetId, ToolTargetRef,
};
use lingxi_kernel::Principal;
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    AttemptId, ContentBlock, ModelCallId, NormalizedMessage, ResourceKind, ResourceRef, RunId,
    SessionId, ToolCallId, ToolSchemaDocument,
};
use lingxi_service::approval_service::ApprovalService;
use lingxi_service::artifactverify::ClaimedFileContract;
use lingxi_service::mcpbridge::{register_mcp_server, McpEndpoint, McpServer};
use lingxi_service::procsupervisor::{ProcessSupervisor, SupervisorLimits};
use lingxi_service::toolgateway::{
    CallerSurface, InvocationPermissionContext, PolicyVerdict, ToolInvocationGateway,
};
use lingxi_service::workerrpc::{
    register_worker_tool, UnconfiguredWorkerModel, WorkerLimits, WorkerModelPort, WorkerRuntime,
    WorkerToolSpec,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::json;

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

fn fixture_exe() -> &'static str {
    env!("CARGO_BIN_EXE_r04_t07_fixture")
}

fn unique_dir(label: &str) -> PathBuf {
    static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);
    let seq = DIR_SEQ.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!(
        "r04t08-{label}-{}-{}-{seq}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).expect("test dir");
    base
}

fn unique_suffix() -> String {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    )
}

// ── evidence fragments ──────────────────────────────────────────────────────

/// Records one pinned case: asserts the observation in-process, then
/// (when the producer set `R04_T08_EVIDENCE_DIR`) writes the fragment the
/// stage map's assertion contracts consume.
fn record_case(case: &str, expect: i64, actual: i64) {
    assert_eq!(
        actual, expect,
        "case {case}: pinned expectation {expect} did not hold (observed {actual})"
    );
    let Ok(dir) = std::env::var("R04_T08_EVIDENCE_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir).join("cases");
    std::fs::create_dir_all(&dir).expect("evidence cases dir");
    let fragment = serde_json::json!({
        "case": case,
        "expect": expect,
        "actual": actual,
        "ok": actual == expect,
    });
    std::fs::write(
        dir.join(format!("{case}.json")),
        serde_json::to_string(&fragment).expect("serializes"),
    )
    .expect("fragment written");
}

// ── the provider double (R05 boundary stand-in: external responses only) ───

struct StepsProvider {
    steps: Mutex<VecDeque<ProviderTurn>>,
    latest_run_id: Mutex<Option<String>>,
}

impl StepsProvider {
    fn new(steps: Vec<ProviderTurn>) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(steps.into()),
            latest_run_id: Mutex::new(None),
        })
    }

    /// The run id of the most recent turn (the trusted driver context —
    /// never model data).
    fn run_id(&self) -> Option<String> {
        self.latest_run_id.lock().unwrap().clone()
    }
}

impl TurnProviderPort for StepsProvider {
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
        _input: &'a ModelTurnInput,

        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let next = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| final_turn("done"));
        *self.latest_run_id.lock().unwrap() = Some(ctx.run_id.to_string());
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

fn tool_turn(target: &str, args: serde_json::Value) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        content: Vec::new(),
        requests: vec![
            ToolRequest::from_effective_arguments(target, args, &budget())
                .expect("effective request"),
        ],
    }
}

// ── the harness ─────────────────────────────────────────────────────────────

struct Harness {
    state: ServiceState,
    gateway: Arc<ToolInvocationGateway>,
    registry: Arc<ToolRegistry>,
    access: Arc<lingxi_service::ResourceAccess>,
    approvals: Arc<ApprovalService>,
    supervisor: Arc<ProcessSupervisor>,
    ws: PathBuf,
    restricted: PathBuf,
    provider: Arc<StepsProvider>,
}

impl Harness {
    /// The kernel context of a direct gateway caller.
    fn ctx(&self, tag: &str) -> RunContext {
        RunContext {
            principal: Principal::LocalUser,
            session_id: SessionId::new(format!("sess_t08_{tag}")),
            run_id: RunId::new(format!("run-{}-{tag}", unique_suffix())),
            attempt: AttemptId::new("a-1".to_string()),
            generation: 1,
        }
    }
}

async fn matrix_harness(label: &str) -> Harness {
    let root = unique_dir(label);
    let ws = root.join("ws");
    let restricted = root.join("restricted");
    std::fs::create_dir_all(&ws).expect("ws");
    std::fs::create_dir_all(&restricted).expect("restricted");
    let home_dir = unique_dir("home");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home_dir.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home_dir).expect("layout");
    let registry = Arc::new(ToolRegistry::new());
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
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
    let clock: Arc<dyn lingxi_service::inject::ServiceClock> =
        Arc::new(lingxi_service::inject::SystemClock);
    // The native file tools (T04) — read/write/edit behind the gateway
    // with their resource extractors.
    lingxi_service::filetools::register_core_file_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&access),
        ws.clone(),
        Arc::clone(&clock),
        Arc::new(lingxi_service::filetools::NoopChangeLog),
        &budget(),
    );
    // The native process tools (T05) — exec_command + write_stdin behind
    // the gateway with the cwd resource extractor (unsandboxed T05 shape;
    // the sandbox face has its own suite).
    let supervisor = Arc::new(
        ProcessSupervisor::new(
            Arc::clone(&clock),
            SupervisorLimits::with_spill_dir(root.join("spill")),
        )
        .expect("supervisor"),
    );
    lingxi_service::exectools::register_process_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&supervisor),
        Arc::clone(&access),
        ws.clone(),
        None,
        Arc::clone(&clock),
        &budget(),
    );
    let provider = StepsProvider::new(vec![]);
    let deps = ServiceDeps {
        turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
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
        registry,
        access,
        approvals,
        supervisor,
        ws,
        restricted,
        provider,
    }
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
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

/// A direct two-phase gateway call (prepare → execute) for one target.
async fn gateway_call(
    h: &Harness,
    ctx: &RunContext,
    surface: CallerSurface,
    permission: InvocationPermissionContext,
    target: &str,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, lingxi_service::toolgateway::GatewayRefusal> {
    let request =
        ToolRequest::from_effective_arguments(target, args, &budget()).expect("effective request");
    let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
    let prepared = h
        .gateway
        .prepare_from_request(ctx, surface, "agent", permission, &call_id, &request)?;
    h.gateway
        .execute_prepared(ctx, &call_id, &prepared.handle)
        .await
}

fn user_mode(mode: SessionPermissionMode) -> InvocationPermissionContext {
    InvocationPermissionContext::UserSession { mode }
}

fn subagent_face(
    tier: ToolAccessTier,
    parent: SessionPermissionMode,
) -> InvocationPermissionContext {
    InvocationPermissionContext::Subagent {
        tier,
        parent_mode: parent,
    }
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
        ToolOutcome::Failed { error } => format!("FAILED: {}", error.message),
        other => format!("{other:?}"),
    }
}

fn error_code_of(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Failed { error } => error
            .details
            .as_ref()
            .and_then(|details| details.get("code"))
            .and_then(|code| code.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

/// Drives one full RUN CHAIN (the real driver: admission → journal →
/// authorization → approval → gateway dispatch) whose model requests the
/// given tool calls, on the shared local session.
async fn run_chain(
    h: &Harness,
    steps: Vec<ProviderTurn>,
    prompt: &str,
) -> lingxi_service::sessions::ExecuteAccepted {
    for step in steps {
        h.provider.steps.lock().unwrap().push_back(step);
    }
    let principal = owner_principal();
    h.state
        .sessions()
        .execute_for(
            h.state.storage().as_ref(),
            h.state.events(),
            h.state.runs(),
            &principal,
            "sess_local_alpha",
            prompt,
            1_000,
        )
        .await
        .expect("run settles")
}

async fn journal_of(
    h: &Harness,
    run_id: &str,
) -> Vec<lingxi_kernel::ports::InvocationJournalEntry> {
    h.state
        .storage()
        .load_invocation_journal(&RunId::new(run_id.to_string()))
        .await
        .expect("journal")
}

async fn set_mode(h: &Harness, mode: SessionPermissionMode) {
    let principal = owner_principal();
    h.state
        .sessions()
        .set_permission_mode_for(&principal, "sess_local_alpha", mode)
        .await
        .expect("mode set");
}

/// Registers a worker tool using the fixture binary in `mode` (the
/// external worker double) with an optional claimed-file contract.
async fn register_fixture_worker(
    h: &Harness,
    runtime: &Arc<WorkerRuntime>,
    mode: &str,
    extra: &str,
    contract: Option<ClaimedFileContract>,
) -> ToolTargetId {
    register_fixture_worker_input(h, runtime, mode, extra, contract, "input.txt").await
}

/// The worker registration with an explicit input file name (the A15
/// adversarial modes sabotage their OWN deliverable, so each leg needs a
/// fresh input).
async fn register_fixture_worker_input(
    h: &Harness,
    runtime: &Arc<WorkerRuntime>,
    mode: &str,
    extra: &str,
    contract: Option<ClaimedFileContract>,
    input_name: &str,
) -> ToolTargetId {
    let input = h.ws.join(input_name);
    std::fs::write(&input, "granted-content\n").expect("input file");
    let local_name = format!("t08w_{mode}_{}", unique_suffix());
    let mut argv = vec![fixture_exe().to_string(), mode.to_string()];
    if !extra.is_empty() {
        argv.push(extra.to_string());
    }
    let spec = WorkerToolSpec {
        plugin_id: "t08worker".to_string(),
        op: "probe".to_string(),
        local_name: local_name.clone(),
        description: format!("T08 matrix worker ({mode})"),
        input_schema: json!({
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
            "additionalProperties": false
        }),
        path_args: vec!["input".to_string()],
        argv,
        env: BTreeMap::new(),
        cwd: h.ws.clone(),
        model: Arc::new(UnconfiguredWorkerModel) as Arc<dyn WorkerModelPort>,
        // R05-T06 (C09): this matrix's workers never issue model
        // callbacks — the granted purpose set is honestly empty.
        allowed_model_purposes: Vec::new(),
        claimed_file_contract: contract,
    };
    register_worker_tool(
        &h.registry,
        h.gateway.as_ref(),
        runtime,
        &h.access,
        &budget(),
        spec,
    )
    .expect("worker tool registers")
    .target
}

/// A plain callable manifest for the inline executors.
fn callable_manifest(name: &str, aliases: Vec<String>) -> ToolManifest {
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: name.to_string(),
        display_name: name.to_string(),
        aliases,
        version: "1.0.0".to_string(),
        description: format!("T08 matrix probe tool {name}"),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema: json!({"type": "object", "additionalProperties": false}),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::Execute,
            capability_base: format!("{name}.probe"),
        },
        availability: Availability::Available,
        timeout_ms: Some(30_000),
        max_concurrency: Some(2),
        declared_permission: DeclaredPermission::Execute,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    }
}

/// An inline executor playing an EXTERNAL tool that "delivers" the refs it
/// is constructed with — the thing the GATEWAY's registration audit must
/// judge (the audit is the code under test; the executor is its input).
struct FixedDeliveryExecutor {
    text: String,
    refs: Vec<ResourceRef>,
}

impl ToolExecutorPort for FixedDeliveryExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let mut success = ToolSuccess::text(self.text.clone());
        success.resource_refs = self.refs.clone();
        Box::pin(async move {
            ToolExecutionResult::of_ctx(ctx, ToolOutcome::Success { result: success })
        })
    }
}

fn file_ref(path: &Path) -> ResourceRef {
    ResourceRef {
        resource_id: lingxi_protocol::ResourceId::new(format!("t08ref:{}", path.display())),
        kind: ResourceKind::Artifact,
        display_name: Some(path.display().to_string()),
        uri: Some(format!("file://{}", path.display())),
        digest: None,
        size_bytes: None,
    }
}

async fn wait_until<T>(what: &str, deadline: Duration, probe: impl Fn() -> Option<T>) -> T {
    let end = std::time::Instant::now() + deadline;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(
            std::time::Instant::now() < end,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

// ── matrix cell 1: tool families × permission contexts × routes ────────────

#[tokio::test(flavor = "multi_thread")]
async fn matrix_tools_x_permission_x_entry_consistency() {
    let h = matrix_harness("matrix-perm").await;
    // Register the MCP stdio server (the on-demand/Deferred route family
    // + the MCP mechanism face) and a worker (the plugin route family).
    let mcp_server = McpServer::new(
        "t08stdio",
        McpEndpoint::Stdio {
            command: fixture_exe().to_string(),
            args: vec!["--mcp-stdio-server".to_string()],
            env: BTreeMap::new(),
            cwd: Some(h.ws.clone()),
        },
    );
    let registered = register_mcp_server(
        Arc::clone(&mcp_server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("mcp registers");
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );
    let worker_target = register_fixture_worker(&h, &runtime, "ok", "", None).await;
    let mcp_target = registered
        .targets
        .iter()
        .find(|t| t.as_str().ends_with(":env_report"))
        .expect("env_report listed")
        .clone();

    // The seven REAL tool families on the Rust stack (the matrix's tool
    // axis — everything below judges the same cells for each of them).
    let families: Vec<(&str, ToolTargetId, PermissionKind)> = vec![
        ("read", read_target(&h), PermissionKind::Read),
        ("write", write_target(&h), PermissionKind::Execute),
        ("edit", edit_target(&h), PermissionKind::Execute),
        ("exec_command", exec_target(&h), PermissionKind::Execute),
        (
            "write_stdin",
            write_stdin_target(&h),
            PermissionKind::Execute,
        ),
        ("mcp", mcp_target.clone(), PermissionKind::Execute),
        ("worker", worker_target.clone(), PermissionKind::Execute),
    ];
    record_case("matrix-tool-family-count", 7, families.len() as i64);

    let args_of = |family: &str| match family {
        "read" => json!({"path": "input.txt"}),
        "write" => json!({"path": "perm-write.txt", "content": "x"}),
        "edit" => {
            json!({"path": "input.txt", "edits": [{"oldText": "granted", "newText": "GRANTED"}]})
        }
        "exec_command" => json!({"argv": ["/bin/echo", "matrix"]}),
        "write_stdin" => json!({"process_id": "proc:0000000000000000000000000000dead"}),
        "mcp" => json!({}),
        "worker" => json!({"input": "input.txt"}),
        other => panic!("unknown family {other}"),
    };

    #[derive(Debug, PartialEq)]
    enum Verdict {
        Allowed,
        NeedsApproval,
        /// Any outright policy denial (read_only's
        /// ACTION_BLOCKED_BY_READ_ONLY and the ask-subagent's
        /// TOOL_APPROVAL_UNAVAILABLE are both denials — the specific
        /// vocabulary is asserted per-cell below).
        Denied,
    }
    let verdict_of = |target: &str, args: serde_json::Value, permission| {
        let ctx = h.ctx("cell");
        let request =
            ToolRequest::from_effective_arguments(target, args, &budget()).expect("request");
        let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
        match h.gateway.prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            permission,
            &call_id,
            &request,
        ) {
            Ok(prepared) => match prepared.policy {
                PolicyVerdict::Allowed => Ok(Verdict::Allowed),
                PolicyVerdict::NeedsApproval { .. } => Ok(Verdict::NeedsApproval),
                PolicyVerdict::Denied { .. } => Ok(Verdict::Denied),
            },
            Err(lingxi_service::toolgateway::GatewayRefusal::PolicyDenied { .. }) => {
                Ok(Verdict::Denied)
            }
            Err(other) => Err(format!("{other:?}")),
        }
    };

    let mut cells_ok = true;
    for (family, target, kind) in &families {
        let args = args_of(family);
        // operate: every family is allowed.
        match verdict_of(
            target.as_str(),
            args.clone(),
            user_mode(SessionPermissionMode::Operate),
        ) {
            Ok(Verdict::Allowed) => {}
            other => {
                cells_ok = false;
                eprintln!("{family}/operate: {other:?}");
            }
        }
        // ask: Execute-class needs approval; Read never prompts.
        let expect_ask = if *kind == PermissionKind::Read {
            Verdict::Allowed
        } else {
            Verdict::NeedsApproval
        };
        match verdict_of(
            target.as_str(),
            args.clone(),
            user_mode(SessionPermissionMode::Ask),
        ) {
            Ok(v) if v == expect_ask => {}
            other => {
                cells_ok = false;
                eprintln!("{family}/ask: {other:?} (expected {expect_ask:?})");
            }
        }
        // read_only (user session): Execute-class denied with the
        // read-only vocabulary; Read still allowed.
        let expect_ro = if *kind == PermissionKind::Read {
            Verdict::Allowed
        } else {
            Verdict::Denied
        };
        match verdict_of(
            target.as_str(),
            args.clone(),
            user_mode(SessionPermissionMode::ReadOnly),
        ) {
            Ok(v) if v == expect_ro => {}
            other => {
                cells_ok = false;
                eprintln!("{family}/read_only: {other:?} (expected {expect_ro:?})");
            }
        }
        // subagent read-tier at the POLICY face: the face itself allows
        // (the subagent tier's write-class attenuation is enforced by the
        // run driver's kernel authorization at dispatch — the T02/T03
        // suites pin that leg); every kind is therefore Allowed here.
        match verdict_of(
            target.as_str(),
            args.clone(),
            subagent_face(ToolAccessTier::ReadOnly, SessionPermissionMode::Operate),
        ) {
            Ok(Verdict::Allowed) => {}
            other => {
                cells_ok = false;
                eprintln!("{family}/subagent-read: {other:?} (expected Allowed)");
            }
        }
        // subagent operate-tier of an ASK parent (the SUP-01 gap): a
        // write-class call is the structured TOOL_APPROVAL_UNAVAILABLE
        // DENIAL — never an operate collapse, never an auto-approve,
        // never an unbounded wait; Read stays allowed.
        let expect_ask_sub = if *kind == PermissionKind::Read {
            Verdict::Allowed
        } else {
            Verdict::Denied
        };
        match verdict_of(
            target.as_str(),
            args.clone(),
            subagent_face(ToolAccessTier::Operate, SessionPermissionMode::Ask),
        ) {
            Ok(v) if v == expect_ask_sub => {}
            other => {
                cells_ok = false;
                eprintln!(
                    "{family}/subagent-write-of-ask: {other:?} (expected {expect_ask_sub:?})"
                );
            }
        }
    }
    record_case("matrix-permission-consistency", 1, i64::from(cells_ok));

    // The SUP-01 refusal itself carries the structured vocabulary (one
    // representative cell, pinned by name for the stage map). The full
    // denial MESSAGE is only visible on a direct prepare (the verdict
    // view collapses it to Denied).
    let sup01_ctx = h.ctx("sup01");
    let sup01_request = ToolRequest::from_effective_arguments(
        worker_target.as_str(),
        json!({"input": "input.txt"}),
        &budget(),
    )
    .expect("request");
    let sup01_call = ToolCallId::new(format!("call-{}", unique_suffix()));
    let sup01 = match h.gateway.prepare_from_request(
        &sup01_ctx,
        CallerSurface::UserRun,
        "agent",
        subagent_face(ToolAccessTier::Operate, SessionPermissionMode::Ask),
        &sup01_call,
        &sup01_request,
    ) {
        Err(lingxi_service::toolgateway::GatewayRefusal::PolicyDenied {
            code, message, ..
        }) => {
            code.contains("TOOL_APPROVAL_UNAVAILABLE")
                && message.contains("deny_on_prompt")
                && message.contains("allowHumanApproval")
                && message.contains("the action was not run")
        }
        _ => false,
    };
    record_case("sup01-ask-subagent-write-refused", 1, i64::from(sup01));

    // Route axis: the SAME (principal, target, args) authorized and
    // executed identically through the DIRECT gateway route and the FULL
    // RUN CHAIN route (background submissions drive the same run surface;
    // the delegation family and subagent child runs have their own T02
    // suites — the entry inventory is registered in the ledger).
    let ctx = h.ctx("route");
    let direct = gateway_call(
        &h,
        &ctx,
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        read_target(&h).as_str(),
        json!({"path": "input.txt"}),
    )
    .await
    .expect("direct route executes");
    let direct_ok = matches!(direct.outcome, ToolOutcome::Success { .. });

    set_mode(&h, SessionPermissionMode::Operate).await;
    let run = run_chain(
        &h,
        vec![tool_turn(
            read_target(&h).as_str(),
            json!({"path": "input.txt"}),
        )],
        "route axis: run-chain read",
    )
    .await;
    let journal = journal_of(&h, &run.run_id).await;
    let chain_ok = journal.len() == 1
        && journal[0]
            .receipt
            .as_ref()
            .is_some_and(|r| r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded)
        && text_of(&direct).contains("granted-content");
    record_case(
        "matrix-route-consistency",
        1,
        i64::from(direct_ok && chain_ok),
    );
}

fn read_target(h: &Harness) -> ToolTargetId {
    h.registry
        .resolve(&ToolTargetRef::ByName {
            name: "read".to_string(),
        })
        .expect("read registered")
}

fn write_target(h: &Harness) -> ToolTargetId {
    h.registry
        .resolve(&ToolTargetRef::ByName {
            name: "write".to_string(),
        })
        .expect("write registered")
}

fn edit_target(h: &Harness) -> ToolTargetId {
    h.registry
        .resolve(&ToolTargetRef::ByName {
            name: "edit".to_string(),
        })
        .expect("edit registered")
}

fn exec_target(h: &Harness) -> ToolTargetId {
    h.registry
        .resolve(&ToolTargetRef::ByName {
            name: "exec_command".to_string(),
        })
        .expect("exec_command registered")
}

fn write_stdin_target(h: &Harness) -> ToolTargetId {
    h.registry
        .resolve(&ToolTargetRef::ByName {
            name: "write_stdin".to_string(),
        })
        .expect("write_stdin registered")
}

// ── matrix cell 2: unmigrated capabilities stay honest in the catalog ──────

/// The on-demand/file-family and dev tool surfaces whose EXECUTION bodies
/// belong to later stages: the R04 catalog face must express them as
/// `Availability::Future` (discoverable, NEVER callable, never presented
/// as available) — the honest-capability share the master prompt §3.2
/// demands ("未迁移能力准确标识并保留期限").
#[tokio::test(flavor = "multi_thread")]
async fn matrix_future_tools_honest_in_catalog() {
    let h = matrix_harness("matrix-future").await;
    let unmigrated = [
        "file",
        "find",
        "grep",
        "ls",
        "materialize",
        "stage_files",
        "ast_edit",
        "ast_grep",
        "lsp",
        "run_code",
        "security_scan",
    ];
    for name in unmigrated {
        let mut manifest = callable_manifest(name, Vec::new());
        manifest.availability = Availability::Future {
            reason: format!(
                "the {name} execution body belongs to a later stage (R06/R07); R04 delivers \
                 the catalog/gateway mechanism and refuses to fake availability"
            ),
        };
        manifest.permission.kind = PermissionKind::Read;
        let receipt = h.registry.register(manifest, &budget()).expect("registers");
        // Discoverable: search finds it and the description names the
        // honest availability.
        let found = h
            .registry
            .search(name)
            .into_iter()
            .any(|listing| listing.target_id == receipt.target_id);
        let full = h
            .registry
            .describe_full(&receipt.target_id)
            .expect("describe");
        let availability_honest = full.listing.availability
            == Availability::Future {
                reason: String::new(),
            }
            || matches!(full.listing.availability, Availability::Future { .. });
        // NOT callable: a prepare is refused with the not-callable
        // vocabulary — zero dispatch, no executor needed at all.
        let ctx = h.ctx("future");
        let request =
            ToolRequest::from_effective_arguments(receipt.target_id.as_str(), json!({}), &budget())
                .expect("request");
        let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
        let refused = matches!(
            h.gateway.prepare_from_request(
                &ctx,
                CallerSurface::UserRun,
                "agent",
                user_mode(SessionPermissionMode::Operate),
                &call_id,
                &request,
            ),
            Err(lingxi_service::toolgateway::GatewayRefusal::TargetNotCallable { .. })
        );
        let held = found && availability_honest && refused;
        record_case(
            &format!("future-tool-shape-{name}-discoverable-not-callable"),
            1,
            i64::from(held),
        );
    }
}

// ── R04-A15: empty/invalid products never register as success ──────────────

#[tokio::test(flavor = "multi_thread")]
async fn a15_empty_and_invalid_products_never_register() {
    let h = matrix_harness("a15-claims").await;
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );

    // Contrast FIRST: an honest claim inside the grant whose file exists
    // registers a REAL delivery (existence-verified ResourceRef + a
    // Success that survives the gateway's registration audit).
    let honest =
        register_fixture_worker_input(&h, &runtime, "claim_ok", "", None, "a15-input.txt").await;
    let ok = gateway_call(
        &h,
        &h.ctx("a15-ok"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        honest.as_str(),
        json!({"input": "a15-input.txt"}),
    )
    .await
    .expect("honest claim executes");
    let ok_held = match &ok.outcome {
        ToolOutcome::Success { result } => {
            result.resource_refs.len() == 1
                && result.resource_refs[0]
                    .uri
                    .as_deref()
                    .is_some_and(|uri| uri.ends_with("a15-input.txt"))
                && result.resource_refs[0].size_bytes == Some("granted-content\n".len() as u64)
        }
        _ => false,
    };
    record_case("a15-real-delivery-registered", 1, i64::from(ok_held));

    // Out-of-grant claim: refused, nothing minted, the restricted
    // sentinel untouched.
    let sentinel = h.restricted.join("sentinel.txt");
    std::fs::write(&sentinel, "SENTINEL\n").expect("sentinel");
    let outside = register_fixture_worker(
        &h,
        &runtime,
        "claim_outside",
        sentinel.to_str().unwrap_or_default(),
        None,
    )
    .await;
    let refused = gateway_call(
        &h,
        &h.ctx("a15-outside"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        outside.as_str(),
        json!({"input": "input.txt"}),
    )
    .await
    .expect("claim executes; verification converts");
    let outside_held = matches!(&refused.outcome, ToolOutcome::Failed { .. })
        && error_code_of(&refused) == "worker_claimed_unauthorized_path"
        && std::fs::read(&sentinel).expect("sentinel") == b"SENTINEL\n";
    record_case("a15-out-of-grant-claim-refused", 1, i64::from(outside_held));

    // Missing product: the worker DELETES its own granted deliverable,
    // then claims it — success text with a vanished in-grant file.
    let missing = register_fixture_worker_input(
        &h,
        &runtime,
        "claim_missing",
        "",
        None,
        "a15-missing-input.txt",
    )
    .await;
    let missing_result = gateway_call(
        &h,
        &h.ctx("a15-missing"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        missing.as_str(),
        json!({"input": "a15-missing-input.txt"}),
    )
    .await
    .expect("claim executes; verification converts");
    let missing_held = matches!(&missing_result.outcome, ToolOutcome::Failed { .. })
        && error_code_of(&missing_result) == "worker_claimed_artifact_invalid"
        && text_of(&missing_result).contains("artifact_missing")
        && text_of(&missing_result).contains("nothing is registered as delivered");
    if !missing_held {
        eprintln!(
            "a15-missing debug: code={:?} text={}",
            error_code_of(&missing_result),
            text_of(&missing_result)
        );
    }
    record_case("a15-missing-claim-refused", 1, i64::from(missing_held));

    // Directory claim: the worker REPLACES its deliverable with a
    // directory and claims the same path — a directory is not a file
    // deliverable (the pre-A15 `exists()`-only check would have minted
    // it).
    let dir_claim =
        register_fixture_worker_input(&h, &runtime, "claim_dir", "", None, "a15-dir-input.txt")
            .await;
    let dir_result = gateway_call(
        &h,
        &h.ctx("a15-dir"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        dir_claim.as_str(),
        json!({"input": "a15-dir-input.txt"}),
    )
    .await
    .expect("claim executes; verification converts");
    let dir_held = matches!(&dir_result.outcome, ToolOutcome::Failed { .. })
        && text_of(&dir_result).contains("artifact_not_a_regular_file");
    record_case("a15-directory-claim-refused", 1, i64::from(dir_held));

    // Structure condition: the tool declares a JSON claimed-file contract
    // and the worker OVERWRITES its deliverable with non-JSON bytes,
    // then claims it.
    let contract = ClaimedFileContract {
        min_bytes: 1,
        format: lingxi_service::artifactverify::ClaimedFileFormat::Json,
    };
    let structural = register_fixture_worker_input(
        &h,
        &runtime,
        "claim_bad_json",
        "",
        Some(contract),
        "a15-json-input.txt",
    )
    .await;
    let structural_result = gateway_call(
        &h,
        &h.ctx("a15-json"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        structural.as_str(),
        json!({"input": "a15-json-input.txt"}),
    )
    .await
    .expect("claim executes; verification converts");
    let structural_held = matches!(&structural_result.outcome, ToolOutcome::Failed { .. })
        && text_of(&structural_result).contains("artifact_content_condition_failed")
        && text_of(&structural_result).contains("not valid JSON");
    record_case(
        "a15-structure-violation-refused",
        1,
        i64::from(structural_held),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a15_gateway_registration_audit_converts_fake_refs() {
    let h = matrix_harness("a15-audit").await;
    // A REAL file delivered through a SUCCESS must pass the audit.
    let real = h.ws.join("audit-real.txt");
    std::fs::write(&real, "real\n").expect("real");
    let real_target = h
        .registry
        .register(callable_manifest("t08_audit_real", Vec::new()), &budget())
        .expect("registers")
        .target_id;
    h.gateway.bind_executor(
        real_target.clone(),
        Arc::new(FixedDeliveryExecutor {
            text: "delivered".to_string(),
            refs: vec![file_ref(&real)],
        }),
        "R04-T08 audit probe executor (real delivery)",
    );
    let ok = gateway_call(
        &h,
        &h.ctx("audit-real"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        real_target.as_str(),
        json!({}),
    )
    .await
    .expect("real delivery executes");
    let real_held =
        matches!(&ok.outcome, ToolOutcome::Success { result } if result.resource_refs.len() == 1);
    record_case("a15-gateway-audit-real-ref-passes", 1, i64::from(real_held));

    // A SUCCESS claiming a file that does NOT exist: the executor ran
    // (dispatched), but the registration audit converts the outcome to an
    // explicit FAILED that states the dispatch happened, side effects may
    // exist, and the file is NOT registered as a valid deliverable.
    let ghost = h.ws.join("audit-ghost.txt");
    let ghost_target = h
        .registry
        .register(callable_manifest("t08_audit_ghost", Vec::new()), &budget())
        .expect("registers")
        .target_id;
    h.gateway.bind_executor(
        ghost_target.clone(),
        Arc::new(FixedDeliveryExecutor {
            text: "pretending".to_string(),
            refs: vec![file_ref(&ghost)],
        }),
        "R04-T08 audit probe executor (fake delivery)",
    );
    let fake = gateway_call(
        &h,
        &h.ctx("audit-ghost"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        ghost_target.as_str(),
        json!({}),
    )
    .await
    .expect("fake delivery executes; the audit converts");
    let fake_held = matches!(&fake.outcome, ToolOutcome::Failed { .. })
        && error_code_of(&fake) == "gateway_artifact_verification_failed"
        && text_of(&fake).contains("execution WAS dispatched")
        && text_of(&fake).contains("NOT registered as a valid deliverable")
        && text_of(&fake).contains("artifact_missing");
    record_case("a15-gateway-audit-fake-ref-failed", 1, i64::from(fake_held));

    // A directory URI is equally not a file delivery.
    let dir_target = h
        .registry
        .register(callable_manifest("t08_audit_dir", Vec::new()), &budget())
        .expect("registers")
        .target_id;
    h.gateway.bind_executor(
        dir_target.clone(),
        Arc::new(FixedDeliveryExecutor {
            text: "directory claim".to_string(),
            refs: vec![file_ref(&h.ws)],
        }),
        "R04-T08 audit probe executor (directory claim)",
    );
    let dir = gateway_call(
        &h,
        &h.ctx("audit-dir"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        dir_target.as_str(),
        json!({}),
    )
    .await
    .expect("directory claim executes; the audit converts");
    let dir_held = matches!(&dir.outcome, ToolOutcome::Failed { .. })
        && error_code_of(&dir) == "gateway_artifact_verification_failed"
        && text_of(&dir).contains("artifact_not_a_regular_file");
    record_case(
        "a15-gateway-audit-directory-ref-failed",
        1,
        i64::from(dir_held),
    );
}

// ── R04-A16: a disabled state covers EVERY route ───────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn a16_disabled_state_covers_every_route() {
    let h = matrix_harness("a16").await;
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );
    let worker_target = register_fixture_worker(&h, &runtime, "ok", "", None).await;

    // An ALIAS-carrying probe target (the alias route of the matrix).
    let alias_target = h
        .registry
        .register(
            callable_manifest("t16_probe", vec!["t16_alias".to_string()]),
            &budget(),
        )
        .expect("registers")
        .target_id;
    h.gateway.bind_executor(
        alias_target.clone(),
        Arc::new(FixedDeliveryExecutor {
            text: "probe-ok".to_string(),
            refs: Vec::new(),
        }),
        "R04-T08 A16 alias probe executor",
    );

    // 1) REAL HISTORY FIRST: the worker executes for real on a run chain.
    // These receipts must SURVIVE the disable below (history is never
    // deleted to hide facts).
    set_mode(&h, SessionPermissionMode::Operate).await;
    let historical = run_chain(
        &h,
        vec![tool_turn(
            worker_target.as_str(),
            json!({"input": "input.txt"}),
        )],
        "A16: historical execution before the disable",
    )
    .await;
    let history = journal_of(&h, &historical.run_id).await;
    assert_eq!(history.len(), 1, "{history:?}");
    assert_eq!(
        history[0].receipt.as_ref().expect("receipt").outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );
    assert!(history[0].receipt.as_ref().expect("receipt").dispatched);

    // 2) CACHE the description snapshot AND a prepared handle for BOTH
    // targets (the "cached description + handle" precondition), plus the
    // pre-disable catalog generation for the pinned route.
    let pin = h.registry.snapshot().catalog_generation;
    let _cached_desc = h
        .registry
        .describe_full(&worker_target)
        .expect("description cached");
    let cached_handle = prepare_cached(
        &h,
        &worker_target,
        json!({"input": "input.txt"}),
        "a16-cache",
    )
    .expect("handle cached");
    let alias_handle =
        prepare_cached(&h, &alias_target, json!({}), "a16-alias").expect("handle cached");

    // 3) DISABLE both targets.
    h.registry
        .set_availability(
            &worker_target,
            Availability::Disabled {
                reason: "R04-A16 matrix disable".to_string(),
            },
        )
        .expect("disable");
    h.registry
        .set_availability(
            &alias_target,
            Availability::Disabled {
                reason: "R04-A16 matrix disable".to_string(),
            },
        )
        .expect("disable");

    let mut holes = 0i64;

    // Route A: a FRESH prepare (the direct gateway route).
    match prepare_cached(
        &h,
        &worker_target,
        json!({"input": "input.txt"}),
        "a16-fresh",
    ) {
        Err(lingxi_service::toolgateway::GatewayRefusal::TargetNotCallable { .. })
        | Err(lingxi_service::toolgateway::GatewayRefusal::StaleCatalog { .. }) => {}
        other => {
            holes += 1;
            eprintln!("fresh prepare after disable: refused={:?}", other.is_err());
        }
    }
    // Route B: the CACHED handle is refused at execute time (the target
    // changed under the cached description).
    match h
        .gateway
        .execute_prepared(
            &cached_handle.ctx,
            &cached_handle.call_id,
            &cached_handle.handle,
        )
        .await
    {
        Err(lingxi_service::toolgateway::GatewayRefusal::TargetChanged { .. }) => {}
        other => {
            holes += 1;
            eprintln!("cached handle after disable: {other:?}");
        }
    }
    // Route C: the ALIAS route (ByName resolution) is equally refused.
    let alias_prepare = {
        let ctx = h.ctx("a16-alias-name");
        let request = ToolRequest::from_effective_arguments("t16_alias", json!({}), &budget())
            .expect("request");
        let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
        h.gateway.prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            user_mode(SessionPermissionMode::Operate),
            &call_id,
            &request,
        )
    };
    let alias_route_refused = matches!(
        alias_prepare,
        Err(lingxi_service::toolgateway::GatewayRefusal::TargetNotCallable { .. })
            | Err(lingxi_service::toolgateway::GatewayRefusal::TargetNotRegistered { .. })
            | Err(lingxi_service::toolgateway::GatewayRefusal::StaleCatalog { .. })
    );
    if !alias_route_refused {
        holes += 1;
        eprintln!("alias route after disable: {:?}", alias_prepare.map(|_| ()));
    }
    // Route D: the cached ALIAS handle dies the same way.
    match h
        .gateway
        .execute_prepared(
            &alias_handle.ctx,
            &alias_handle.call_id,
            &alias_handle.handle,
        )
        .await
    {
        Err(lingxi_service::toolgateway::GatewayRefusal::TargetChanged { .. }) => {}
        other => {
            holes += 1;
            eprintln!("cached alias handle after disable: {other:?}");
        }
    }
    // Route E: the FULL RUN CHAIN — the journal closes a never-dispatched
    // failure (a dispatched receipt here would be a hole).
    let after = run_chain(
        &h,
        vec![tool_turn(
            worker_target.as_str(),
            json!({"input": "input.txt"}),
        )],
        "A16: run chain after the disable",
    )
    .await;
    let after_journal = journal_of(&h, &after.run_id).await;
    assert_eq!(after_journal.len(), 1, "{after_journal:?}");
    let receipt = after_journal[0].receipt.as_ref().expect("receipt");
    assert_eq!(
        receipt.outcome,
        lingxi_kernel::ports::ReceiptOutcome::Failed,
        "{after_journal:?}"
    );
    if receipt.dispatched {
        holes += 1;
    }
    // Route F: a PINNED old-generation request (the disable bumped the
    // catalog generation — the stale-catalog gate refuses it).
    let pinned = h.gateway.prepare(
        lingxi_service::toolgateway::InvocationRequest::from_trusted_entry(
            &h.ctx("a16-pin"),
            CallerSurface::UserRun,
            "agent",
            user_mode(SessionPermissionMode::Operate),
            ToolTargetRef::ByTargetId {
                target_id: worker_target.clone(),
            },
            Some(lingxi_kernel::toolcatalog::CatalogPin {
                catalog_generation: pin,
            }),
            json!({"input": "input.txt"}),
            ToolCallId::new(format!("call-{}", unique_suffix())),
        ),
    );
    match pinned {
        Err(lingxi_service::toolgateway::GatewayRefusal::StaleCatalog { .. })
        | Err(lingxi_service::toolgateway::GatewayRefusal::TargetNotCallable { .. }) => {}
        other => {
            holes += 1;
            eprintln!("pinned generation after disable: {other:?}");
        }
    }

    record_case("matrix-lifecycle-disable-holes", 0, holes);
    // The historical receipts SURVIVED the disable (no deletion cover-up).
    let history_after = journal_of(&h, &historical.run_id).await;
    let preserved = history_after.len() == 1
        && history_after[0].receipt.as_ref().is_some_and(|r| {
            r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded && r.dispatched
        });
    record_case("a16-history-preserved", 1, i64::from(preserved));
    record_case("a16-alias-route-covered", 1, i64::from(alias_route_refused));

    // 4) UNINSTALL leg: re-enable, cache a fresh handle, uninstall — no
    // route executes the removed target.
    h.registry
        .set_availability(&worker_target, Availability::Deferred)
        .expect("re-enable");
    let fresh = prepare_cached(
        &h,
        &worker_target,
        json!({"input": "input.txt"}),
        "a16-uninst",
    )
    .expect("fresh handle after re-enable");
    h.registry.uninstall(&worker_target).expect("uninstall");
    let mut uninstall_holes = 0i64;
    if h.registry
        .resolve(&ToolTargetRef::ByTargetId {
            target_id: worker_target.clone(),
        })
        .is_ok()
    {
        uninstall_holes += 1;
    }
    if h.gateway
        .execute_prepared(&fresh.ctx, &fresh.call_id, &fresh.handle)
        .await
        .is_ok()
    {
        uninstall_holes += 1;
    }
    record_case("matrix-lifecycle-uninstall-holes", 0, uninstall_holes);

    // 5) GENERATION leg: register a fresh callable, cache a handle, then
    // UPDATE the manifest — the cached handle must die with TargetChanged
    // (the old description never points at the new meaning).
    let gen_target = h
        .registry
        .register(callable_manifest("t16_gen", Vec::new()), &budget())
        .expect("registers")
        .target_id;
    h.gateway.bind_executor(
        gen_target.clone(),
        Arc::new(FixedDeliveryExecutor {
            text: "gen".to_string(),
            refs: Vec::new(),
        }),
        "R04-T08 A16 generation probe executor",
    );
    let gen_handle = prepare_cached(&h, &gen_target, json!({}), "a16-gen").expect("handle");
    let mut updated = callable_manifest("t16_gen", Vec::new());
    updated.description = "R04-T08 A16 generation bump (schema meaning changed)".to_string();
    updated.input_schema.schema = json!({
        "type": "object",
        "properties": {"required_now": {"type": "string"}},
        "required": ["required_now"],
        "additionalProperties": false,
    });
    h.registry
        .update(&gen_target, updated, &budget())
        .expect("update bumps the generation");
    let mut generation_holes = 0i64;
    match h
        .gateway
        .execute_prepared(&gen_handle.ctx, &gen_handle.call_id, &gen_handle.handle)
        .await
    {
        Err(lingxi_service::toolgateway::GatewayRefusal::TargetChanged { .. }) => {}
        other => {
            generation_holes += 1;
            eprintln!("cached handle after generation bump: {other:?}");
        }
    }
    record_case("matrix-lifecycle-generation-refusals", 0, generation_holes);
}

/// A16 routes can replay the pair precisely.
struct CachedHandle {
    handle: lingxi_service::toolgateway::PreparedInvocationHandle,
    call_id: ToolCallId,
    ctx: RunContext,
}

/// Prepares and returns the handle together with its binding facts.
fn prepare_cached(
    h: &Harness,
    target: &ToolTargetId,
    args: serde_json::Value,
    tag: &str,
) -> Result<CachedHandle, lingxi_service::toolgateway::GatewayRefusal> {
    let ctx = h.ctx(tag);
    let request =
        ToolRequest::from_effective_arguments(target.as_str(), args, &budget()).expect("request");
    let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
    let prepared = h.gateway.prepare_from_request(
        &ctx,
        CallerSurface::UserRun,
        "agent",
        user_mode(SessionPermissionMode::Operate),
        &call_id,
        &request,
    )?;
    Ok(CachedHandle {
        handle: prepared.handle,
        call_id,
        ctx,
    })
}

// ── the unified outcome semantics and their receipts ───────────────────────

/// One run chain exercising the four outcome classes end to end, with the
/// journal receipts and per-call authorization independence pinned as
/// cases (the semantics share of the leaf contracts).
#[tokio::test(flavor = "multi_thread")]
async fn outcome_semantics_and_receipts_unified() {
    let h = matrix_harness("semantics").await;
    let runtime = WorkerRuntime::new(
        WorkerLimits {
            deadline_ms: 500,
            ..WorkerLimits::default()
        },
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );
    set_mode(&h, SessionPermissionMode::Operate).await;

    // SUCCESS: the native read through the full chain — a dispatched,
    // succeeded receipt whose durable wire event carries the REAL content.
    std::fs::write(h.ws.join("sem.txt"), "semantics payload\n").expect("file");
    let ok_run = run_chain(
        &h,
        vec![tool_turn(
            read_target(&h).as_str(),
            json!({"path": "sem.txt"}),
        )],
        "semantics: successful read",
    )
    .await;
    let ok_journal = journal_of(&h, &ok_run.run_id).await;
    let ok_held = ok_journal.len() == 1
        && ok_journal[0].receipt.as_ref().is_some_and(|r| {
            r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded && r.dispatched
        });
    record_case(
        "semantics-success-receipt-dispatched",
        1,
        i64::from(ok_held),
    );

    // FAILED (never dispatched): a write outside the resource scope —
    // the gateway refuses at prepare; the receipt is failed with
    // dispatched=false and no file appears.
    let deny_run = run_chain(
        &h,
        vec![tool_turn(
            write_target(&h).as_str(),
            json!({"path": "../restricted/sem-escape.txt", "content": "no"}),
        )],
        "semantics: refused write",
    )
    .await;
    let deny_journal = journal_of(&h, &deny_run.run_id).await;
    let deny_held = deny_journal.len() == 1
        && deny_journal[0].receipt.as_ref().is_some_and(|r| {
            r.outcome == lingxi_kernel::ports::ReceiptOutcome::Failed && !r.dispatched
        })
        && !h.restricted.join("sem-escape.txt").exists();
    record_case(
        "semantics-failed-never-dispatched-receipt",
        1,
        i64::from(deny_held),
    );

    // UNKNOWN: a worker that exceeds its deadline — the invocation WAS
    // dispatched, the outcome is unconfirmed, the receipt says unknown
    // and nothing retried it (exactly one journal entry).
    let hang_target = register_fixture_worker(&h, &runtime, "hang", "", None).await;
    let unknown_run = run_chain(
        &h,
        vec![tool_turn(
            hang_target.as_str(),
            json!({"input": "input.txt"}),
        )],
        "semantics: unknown worker outcome",
    )
    .await;
    let unknown_journal = journal_of(&h, &unknown_run.run_id).await;
    let unknown_held = unknown_journal.len() == 1
        && unknown_journal[0].receipt.as_ref().is_some_and(|r| {
            r.outcome == lingxi_kernel::ports::ReceiptOutcome::Unknown && r.dispatched
        });
    record_case(
        "semantics-unknown-receipt-honest",
        1,
        i64::from(unknown_held),
    );

    // CANCELLED: a user cancel during a long-running exec — the run
    // settles cancelled, and the tool entry is left WITHOUT a fabricated
    // receipt (started-only, or a failed/unknown receipt that says the
    // truth — never a success).
    let cancel_run_handle = {
        let steps = vec![tool_turn(
            exec_target(&h).as_str(),
            json!({"cmd": "sleep 300"}),
        )];
        let state = h.state.clone();
        let principal = owner_principal();
        for step in steps {
            h.provider.steps.lock().unwrap().push_back(step);
        }
        tokio::spawn(async move {
            state
                .sessions()
                .execute_for(
                    state.storage().as_ref(),
                    state.events(),
                    state.runs(),
                    &principal,
                    "sess_local_alpha",
                    "semantics: cancel during exec",
                    1_000,
                )
                .await
                .expect("run settles")
        })
    };
    let live = wait_until("live process record", Duration::from_secs(20), || {
        h.supervisor.live_handles().first().cloned()
    })
    .await;
    let _ = live;
    // The run id arrives through the durable run registry (the newest
    // running run of the shared session).
    let run_id = wait_until("run id visible", Duration::from_secs(20), || {
        h.provider.run_id()
    })
    .await;
    let fired = h
        .state
        .runs()
        .cancel_run(&run_id, "user cancelled the semantics probe");
    assert!(
        matches!(
            fired,
            lingxi_service::cancel::FireOutcome::Fired
                | lingxi_service::cancel::FireOutcome::AlreadyCancelling
        ),
        "{fired:?}"
    );
    let cancelled_outcome = cancel_run_handle.await.expect("run settles");
    let cancel_journal = journal_of(&h, &cancelled_outcome.run_id).await;
    let cancel_held = cancel_journal.len() == 1
        && !cancel_journal[0]
            .receipt
            .as_ref()
            .is_some_and(|r| r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded);
    record_case(
        "semantics-cancel-leaves-no-fabricated-receipt",
        1,
        i64::from(cancel_held),
    );
    let _ = run_id;

    // Per-call authorization independence (the run_tools share): in the
    // SAME read_only context a read dispatches while a write is refused —
    // each call is judged on its own target+arguments.
    let ctx_ro = h.ctx("independent");
    let read_ro = gateway_call(
        &h,
        &ctx_ro,
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::ReadOnly),
        read_target(&h).as_str(),
        json!({"path": "sem.txt"}),
    )
    .await;
    let write_ro = gateway_call(
        &h,
        &ctx_ro,
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::ReadOnly),
        write_target(&h).as_str(),
        json!({"path": "ro-write.txt", "content": "x"}),
    )
    .await;
    let independent = matches!(
        read_ro.expect("read under read_only").outcome,
        ToolOutcome::Success { .. }
    ) && matches!(
        write_ro.map(|result| result.outcome),
        Err(_) | Ok(ToolOutcome::Failed { .. }) | Ok(ToolOutcome::Unknown { .. })
    );
    record_case(
        "gateway-each-call-independent-permission",
        1,
        i64::from(independent),
    );
}

// ── the real-file share cases (read/write/edit through the chain) ──────────

#[tokio::test(flavor = "multi_thread")]
async fn native_tool_share_cases_on_the_real_chain() {
    let h = matrix_harness("native-share").await;
    set_mode(&h, SessionPermissionMode::Operate).await;

    // read: real bytes through the full chain.
    std::fs::write(h.ws.join("share.txt"), "alpha\nbeta\ngamma\n").expect("file");
    let read_run = run_chain(
        &h,
        vec![tool_turn(
            read_target(&h).as_str(),
            json!({"path": "share.txt"}),
        )],
        "share: read",
    )
    .await;
    let read_journal = journal_of(&h, &read_run.run_id).await;
    let read_ok = read_journal.len() == 1
        && read_journal[0]
            .receipt
            .as_ref()
            .is_some_and(|r| r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded);
    record_case("tool-read-real-chain", 1, i64::from(read_ok));

    // write: a real file lands (and its ResourceRef survives the
    // registration audit).
    let write_run = run_chain(
        &h,
        vec![tool_turn(
            write_target(&h).as_str(),
            json!({"path": "written.txt", "content": "written by the chain"}),
        )],
        "share: write",
    )
    .await;
    let write_journal = journal_of(&h, &write_run.run_id).await;
    let write_ok = write_journal.len() == 1
        && write_journal[0]
            .receipt
            .as_ref()
            .is_some_and(|r| r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded)
        && std::fs::read_to_string(h.ws.join("written.txt"))
            .is_ok_and(|content| content == "written by the chain");
    record_case("tool-write-real-chain", 1, i64::from(write_ok));

    // edit: the OBS-1-aligned fuzzy semantics through the chain (one
    // smart-quote variant + one exact duplicate contrast is already the
    // T04 suite; here the single-occurrence edit succeeds).
    let edit_run = run_chain(
        &h,
        vec![tool_turn(
            edit_target(&h).as_str(),
            json!({"path": "share.txt", "edits": [{"oldText": "beta", "newText": "BETA"}]}),
        )],
        "share: edit",
    )
    .await;
    let edit_journal = journal_of(&h, &edit_run.run_id).await;
    let edit_ok = edit_journal.len() == 1
        && edit_journal[0]
            .receipt
            .as_ref()
            .is_some_and(|r| r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded)
        && std::fs::read_to_string(h.ws.join("share.txt"))
            .is_ok_and(|content| content == "alpha\nBETA\ngamma\n");
    record_case("tool-edit-real-chain", 1, i64::from(edit_ok));

    // edit conflict: the version changes under a prepared read — the
    // user's newer version survives untouched (A07 share, direct route).
    let ctx = h.ctx("conflict");
    std::fs::write(h.ws.join("conflict.txt"), "version-1\n").expect("v1");
    let read_first = gateway_call(
        &h,
        &ctx,
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        read_target(&h).as_str(),
        json!({"path": "conflict.txt"}),
    )
    .await
    .expect("read v1");
    assert!(matches!(read_first.outcome, ToolOutcome::Success { .. }));
    std::fs::write(h.ws.join("conflict.txt"), "version-2-user\n").expect("user edit");
    let stale = gateway_call(
        &h,
        &ctx,
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        edit_target(&h).as_str(),
        json!({"path": "conflict.txt", "edits": [{"oldText": "version-1", "newText": "x"}]}),
    )
    .await
    .expect("stale edit is a tool error");
    let conflict_ok = text_of(&stale).contains("FILE_STALE_SINCE_READ")
        && std::fs::read_to_string(h.ws.join("conflict.txt")).expect("v2 kept")
            == "version-2-user\n";
    record_case(
        "tool-edit-conflict-preserves-user-version",
        1,
        i64::from(conflict_ok),
    );

    // exec_command: a real process through the full chain.
    let exec_run = run_chain(
        &h,
        vec![tool_turn(
            exec_target(&h).as_str(),
            json!({"argv": ["/bin/sh", "-c", "printf matrix-exec"]}),
        )],
        "share: exec",
    )
    .await;
    let exec_journal = journal_of(&h, &exec_run.run_id).await;
    let exec_ok = exec_journal.len() == 1
        && exec_journal[0]
            .receipt
            .as_ref()
            .is_some_and(|r| r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded);
    record_case("tool-exec-command-real-chain", 1, i64::from(exec_ok));
}

// ── the PTY / write_stdin / terminal share cases ───────────────────────────

/// How many times one sent marker becomes observable in the terminal's
/// transcript: the terminal runs with the DEFAULT line discipline
/// (canonical + ECHO — the spawn path sets no raw mode), so a written
/// marker is reflected TWICE, in stream order: the kernel's input echo
/// (emitted at write time) and the child `cat`'s loopback copy (written
/// only once the child is scheduled to read the line and echo it back).
const MARKER_OBSERVABLE_COPIES: usize = 2;

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// Sends one marker to the terminal and polls (empty-char writes are
/// pure reads) until the marker's output has FULLY settled — real PTY
/// timing under a bounded deadline, never a fixed sleep.
///
/// Readiness barrier: this helper returns only after EVERY observable
/// copy of the marker has been delivered (`MARKER_OBSERVABLE_COPIES`),
/// not after the first occurrence. The single sequential PTY read loop
/// appends transcript bytes in stream order, the echo of a line is
/// emitted before that line can become readable to the child, and a
/// delivery of this pure-ASCII stream consumes every byte present — so
/// once the LAST copy (the child's) is observed, every byte this write
/// can ever produce has been appended AND consumed: the cursor is
/// provably past the whole marker. Exiting at the FIRST observed copy
/// instead leaves the second copy in flight under load, and it then
/// legitimately lands in the NEXT send's delivery window — exactly the
/// snapshot-case flake FINAL-03 recorded (the OLD marker reappearing in
/// the new snapshot, `observed 0`): a barrier defect in this harness,
/// not a cursor defect in the tool.
async fn send_and_expect(h: &Harness, handle: &str, marker: &str) -> String {
    let first = gateway_call(
        h,
        &h.ctx("tty"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        write_stdin_target(h).as_str(),
        json!({"process_id": handle, "chars": format!("{marker}\n")}),
    )
    .await
    .expect("write_stdin continues");
    let mut collected = text_of(&first);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while count_occurrences(&collected, marker) < MARKER_OBSERVABLE_COPIES {
        assert!(
            std::time::Instant::now() < deadline,
            "marker {marker} never settled (expected {MARKER_OBSERVABLE_COPIES} observable \
             copies: line-discipline echo + child loopback); collected so far: {collected}"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
        let poll = gateway_call(
            h,
            &h.ctx("tty"),
            CallerSurface::UserRun,
            user_mode(SessionPermissionMode::Operate),
            write_stdin_target(h).as_str(),
            json!({"process_id": handle, "chars": ""}),
        )
        .await
        .expect("poll continues");
        collected.push_str(&text_of(&poll));
    }
    collected
}

#[tokio::test(flavor = "multi_thread")]
async fn terminal_family_share_cases() {
    let h = matrix_harness("terminal").await;
    set_mode(&h, SessionPermissionMode::Operate).await;

    // A persistent terminal: exec_command tty=true returns a RUNNING
    // handle (started, never a completion claim); write_stdin continues
    // the SAME terminal and delivers the NEW output since its cursor
    // (the tail/snapshot mechanism); a foreign session's write is
    // refused; closing terminates the terminal.
    let start = gateway_call(
        &h,
        &h.ctx("tty"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        exec_target(&h).as_str(),
        json!({"cmd": "cat", "tty": true}),
    )
    .await
    .expect("terminal starts");
    let handle = match &start.outcome {
        ToolOutcome::Success { result } => match result.status.as_deref() {
            Some(lingxi_kernel::ports::ToolRunStatus::Running { handle }) => handle.clone(),
            other => panic!("a tty start must be Running, got {other:?}"),
        },
        other => panic!("terminal start failed: {other:?}"),
    };

    // Continuation + cursor delivery (tail semantics): send the marker,
    // then poll the terminal (empty writes = pure reads) until the echo
    // is delivered — real PTY timing, bounded by the deadline.
    let tail_output = send_and_expect(&h, &handle, "TAIL-MARKER").await;
    let tail_ok = tail_output.contains("TAIL-MARKER");
    record_case("terminal-tail-cursor-continuation", 1, i64::from(tail_ok));

    // A SECOND marker delivers ONLY the new output (the cursor advanced
    // — the snapshot is the current transcript tail, not a replay).
    let snapshot_output = send_and_expect(&h, &handle, "SNAPSHOT-MARKER").await;
    let snapshot_ok =
        snapshot_output.contains("SNAPSHOT-MARKER") && !snapshot_output.contains("TAIL-MARKER");
    record_case(
        "terminal-snapshot-current-transcript",
        1,
        i64::from(snapshot_ok),
    );

    // A FOREIGN session's write_stdin is refused (ownership), and it
    // writes nothing into the terminal.
    let foreign = gateway_call(
        &h,
        &h.ctx("foreign"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        write_stdin_target(&h).as_str(),
        json!({"process_id": handle, "chars": "FOREIGN\n"}),
    )
    .await
    .expect("foreign write is refused loudly");
    let foreign_refused = matches!(&foreign.outcome, ToolOutcome::Failed { .. })
        && text_of(&foreign).contains("WRITE_STDIN");
    record_case(
        "tool-write-stdin-foreign-writes",
        0,
        i64::from(!foreign_refused),
    );
    let _ = foreign_refused;

    // The successful continuations above are the write_stdin share.
    record_case(
        "tool-write-stdin-continuation",
        1,
        i64::from(tail_ok && snapshot_ok),
    );

    // Closing the terminal terminates it (bounded, observable).
    let parsed =
        lingxi_service::procsupervisor::ProcessHandleId::parse(&handle).expect("handle parses");
    h.supervisor.terminate_detached(
        &parsed,
        lingxi_service::procsupervisor::TerminationReason::Close,
    );
    wait_until("terminal record settles", Duration::from_secs(10), || {
        let phase = h.supervisor.phase_of(&parsed)?;
        matches!(
            phase,
            lingxi_service::procsupervisor::RecordPhase::Terminated { .. }
                | lingxi_service::procsupervisor::RecordPhase::Exited { .. }
        )
        .then_some(())
    })
    .await;
    // After the close, a late write fails honestly.
    let late = gateway_call(
        &h,
        &h.ctx("tty"),
        CallerSurface::UserRun,
        user_mode(SessionPermissionMode::Operate),
        write_stdin_target(&h).as_str(),
        json!({"process_id": handle, "chars": "late\n"}),
    )
    .await
    .expect("late write resolves");
    let close_ok = matches!(&late.outcome, ToolOutcome::Failed { .. })
        && text_of(&late).contains("not running");
    record_case("terminal-close-stops-terminal", 1, i64::from(close_ok));

    // exec cancel: the process-tree cleanup on a run cancel (the light
    // A09 share — full-tree version lives in the T05 suite).
    let cancel_steps = vec![tool_turn(
        exec_target(&h).as_str(),
        json!({"cmd": "sleep 300"}),
    )];
    for step in cancel_steps {
        h.provider.steps.lock().unwrap().push_back(step);
    }
    let state = h.state.clone();
    let principal = owner_principal();
    let run_handle = tokio::spawn(async move {
        state
            .sessions()
            .execute_for(
                state.storage().as_ref(),
                state.events(),
                state.runs(),
                &principal,
                "sess_local_alpha",
                "terminal share: cancel during exec",
                1_000,
            )
            .await
            .expect("run settles")
    });
    let live_id = wait_until("live record", Duration::from_secs(20), || {
        h.supervisor.live_handles().first().cloned()
    })
    .await;
    let cancel_run_id = wait_until("run id", Duration::from_secs(20), || h.provider.run_id()).await;
    let _ = live_id;
    let fired = h
        .state
        .runs()
        .cancel_run(&cancel_run_id, "user cancelled the exec");
    assert!(
        matches!(
            fired,
            lingxi_service::cancel::FireOutcome::Fired
                | lingxi_service::cancel::FireOutcome::AlreadyCancelling
        ),
        "{fired:?}"
    );
    let cancelled = run_handle.await.expect("settles");
    let _ = cancelled;
    let cleanup_ok = wait_until("no live processes", Duration::from_secs(10), || {
        h.supervisor.live_handles().is_empty().then_some(true)
    })
    .await;
    record_case("tool-exec-cancel-cleanup", 1, i64::from(cleanup_ok));
}

// ── the MCP mechanism face (connector-family share cases) ──────────────────

#[tokio::test(flavor = "multi_thread")]
async fn mcp_mechanism_face_share_cases() {
    let h = matrix_harness("mcp-face").await;
    let server = McpServer::new(
        "t08face",
        McpEndpoint::Stdio {
            command: fixture_exe().to_string(),
            args: vec!["--mcp-stdio-server".to_string()],
            env: BTreeMap::new(),
            cwd: Some(h.ws.clone()),
        },
    );
    // Register (the connector composition entry): a REAL initialize
    // handshake, a negotiated protocol, and the listing synced into the
    // catalog.
    let registered = register_mcp_server(
        Arc::clone(&server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("registers");
    let handshake_ok = server.connect_count() == 1
        && registered.sync.negotiated_protocol.starts_with("20")
        && !registered.targets.is_empty();
    record_case(
        "mcp-connector-register-handshake",
        1,
        i64::from(handshake_ok),
    );
    let sync_ok = registered.sync.registered >= 1
        && server.is_connected()
        && registered
            .targets
            .iter()
            .any(|target| target.as_str().ends_with(":env_report"));
    record_case("mcp-connector-catalog-sync", 1, i64::from(sync_ok));

    let target = registered
        .targets
        .iter()
        .find(|t| t.as_str().ends_with(":env_report"))
        .expect("env_report")
        .clone();

    // describe: the real namespaced identity + the Execute permission
    // contract (annotations never soften it).
    let full = h.registry.describe_full(&target).expect("describe");
    let describe_ok =
        full.listing.target_id == target && matches!(full.listing.origin, ToolOrigin::Mcp { .. });
    record_case("mcp-describe-real-identity", 1, i64::from(describe_ok));

    // search: the namespaced catalog query finds it.
    let found = h
        .registry
        .search("env_report")
        .into_iter()
        .any(|listing| listing.target_id == target);
    record_case("mcp-search-namespaced", 1, i64::from(found));

    // The catalog face lists MCP tools with their permission contract
    // (the data face the settings surfaces consume).
    let snapshot = h.registry.snapshot();
    let listed_with_permission = snapshot
        .tools
        .iter()
        .any(|listing| listing.target_id == target);
    record_case(
        "catalog-face-lists-mcp-tools-with-permission",
        1,
        i64::from(listed_with_permission),
    );

    // The T03 permission face on the MCP tool: operate allows, ask needs
    // approval, read_only denies with the read-only vocabulary.
    let verdict_of = |permission| {
        let ctx = h.ctx("mcp-verdict");
        let request = ToolRequest::from_effective_arguments(target.as_str(), json!({}), &budget())
            .expect("request");
        let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
        h.gateway
            .prepare_from_request(
                &ctx,
                CallerSurface::UserRun,
                "agent",
                permission,
                &call_id,
                &request,
            )
            .map(|prepared| match prepared.policy {
                PolicyVerdict::Allowed => "allowed",
                PolicyVerdict::NeedsApproval { .. } => "needs-approval",
                PolicyVerdict::Denied { .. } => "denied",
            })
            .map_err(|refusal| refusal.code().to_string())
    };
    let face_ok = verdict_of(user_mode(SessionPermissionMode::Operate)) == Ok("allowed")
        && verdict_of(user_mode(SessionPermissionMode::Ask)) == Ok("needs-approval")
        && verdict_of(user_mode(SessionPermissionMode::ReadOnly))
            == Err("gateway_policy_denied".to_string());
    record_case("mcp-tool-permission-face", 1, i64::from(face_ok));

    // The full-chain call succeeds with the server's REAL content (the
    // provider only requested it; the content comes from the process).
    set_mode(&h, SessionPermissionMode::Operate).await;
    let run = run_chain(
        &h,
        vec![tool_turn(target.as_str(), json!({}))],
        "mcp: chain call",
    )
    .await;
    let journal = journal_of(&h, &run.run_id).await;
    let call_ok = journal.len() == 1
        && journal[0].receipt.as_ref().is_some_and(|r| {
            r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded && r.dispatched
        });
    record_case("mcp-tool-call-full-chain", 1, i64::from(call_ok));
}

// ── the approval-face share cases ──────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn approval_face_share_cases() {
    let h = matrix_harness("approval-face").await;
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );
    let worker_target = register_fixture_worker(&h, &runtime, "ok", "", None).await;

    // The three user modes are settable and observable on the real
    // session surface (the permission-mode data face).
    let mut modes_ok = true;
    for mode in [
        SessionPermissionMode::Operate,
        SessionPermissionMode::Ask,
        SessionPermissionMode::ReadOnly,
    ] {
        set_mode(&h, mode).await;
        let observed = h
            .state
            .sessions()
            .session_supervisor()
            .permission_mode("sess_local_alpha");
        if observed != mode {
            modes_ok = false;
        }
    }
    record_case("permission-face-modes-verifiable", 1, i64::from(modes_ok));

    // ASK: park → answer(Approve) → exactly ONE execution.
    set_mode(&h, SessionPermissionMode::Ask).await;
    let run_handle = {
        let steps = vec![tool_turn(
            worker_target.as_str(),
            json!({"input": "input.txt"}),
        )];
        for step in steps {
            h.provider.steps.lock().unwrap().push_back(step);
        }
        let state = h.state.clone();
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
                    "approval share: ask park",
                    1_000,
                )
                .await
                .expect("run settles")
        })
    };
    let pending = wait_until("pending approval", Duration::from_secs(20), || {
        let ctx = RunContext {
            principal: Principal::LocalUser,
            session_id: SessionId::new("sess_local_alpha".to_string()),
            run_id: RunId::new("pending-probe".to_string()),
            attempt: AttemptId::new("a-1".to_string()),
            generation: 1,
        };
        h.approvals
            .pending_of(&ctx, "sess_local_alpha")
            .into_iter()
            .find(|view| view.target == worker_target.as_str())
    })
    .await;
    let answer_ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess_local_alpha".to_string()),
        run_id: RunId::new("answerer".to_string()),
        attempt: AttemptId::new("a-1".to_string()),
        generation: 1,
    };
    let answered = h.approvals.answer(
        &answer_ctx,
        "sess_local_alpha",
        &pending.approval_id,
        lingxi_service::approval_service::Answer::Approve,
    );
    assert!(answered.settled());
    // The duplicate click is the deterministic AlreadySettled no-op.
    let duplicate = h.approvals.answer(
        &answer_ctx,
        "sess_local_alpha",
        &pending.approval_id,
        lingxi_service::approval_service::Answer::Approve,
    );
    let duplicate_ok = matches!(
        duplicate,
        lingxi_service::approval_service::AnswerOutcome::AlreadySettled(_)
    );
    let settled = run_handle.await.expect("run completes");
    let journal = journal_of(&h, &settled.run_id).await;
    let executed_once = journal.len() == 1
        && journal[0].receipt.as_ref().is_some_and(|r| {
            r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded && r.dispatched
        });
    record_case("approval-answer-executes-once", 1, i64::from(executed_once));
    record_case("approval-duplicate-idempotent", 1, i64::from(duplicate_ok));

    // REJECT: zero executions, never a silent skip.
    set_mode(&h, SessionPermissionMode::Ask).await;
    let reject_run = {
        let steps = vec![tool_turn(
            worker_target.as_str(),
            json!({"input": "input.txt"}),
        )];
        for step in steps {
            h.provider.steps.lock().unwrap().push_back(step);
        }
        let state = h.state.clone();
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
                    "approval share: reject",
                    1_000,
                )
                .await
                .expect("run settles")
        })
    };
    let pending2 = wait_until("second pending", Duration::from_secs(20), || {
        let ctx = RunContext {
            principal: Principal::LocalUser,
            session_id: SessionId::new("sess_local_alpha".to_string()),
            run_id: RunId::new("pending-probe".to_string()),
            attempt: AttemptId::new("a-1".to_string()),
            generation: 1,
        };
        h.approvals
            .pending_of(&ctx, "sess_local_alpha")
            .into_iter()
            .find(|view| {
                view.target == worker_target.as_str() && view.approval_id != pending.approval_id
            })
    })
    .await;
    let rejected = h.approvals.answer(
        &answer_ctx,
        "sess_local_alpha",
        &pending2.approval_id,
        lingxi_service::approval_service::Answer::Reject {
            reason: "not wanted".to_string(),
        },
    );
    assert!(rejected.settled());
    let reject_run = reject_run.await.expect("settles");
    let reject_journal = journal_of(&h, &reject_run.run_id).await;
    let dispatched_count = reject_journal
        .iter()
        .filter(|entry| entry.receipt.as_ref().is_some_and(|r| r.dispatched))
        .count() as i64;
    record_case("approval-reject-zero-dispatch", 0, dispatched_count);

    // PREAUTHORIZATION is single-use and session-scoped (the one-shot
    // MCP capability grant mechanism).
    let grant_ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess_local_alpha".to_string()),
        run_id: RunId::new("grant-holder".to_string()),
        attempt: AttemptId::new("a-1".to_string()),
        generation: 1,
    };
    let other_session_ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess_other".to_string()),
        run_id: RunId::new("grant-other".to_string()),
        attempt: AttemptId::new("a-1".to_string()),
        generation: 1,
    };
    let request = ToolRequest::from_effective_arguments(
        worker_target.as_str(),
        json!({"input": "input.txt"}),
        &budget(),
    )
    .expect("request");
    let listing = h.registry.describe(&worker_target).expect("describe");
    let key = lingxi_service::approval_service::InvocationGrantKey {
        target_id: worker_target.as_str().to_string(),
        capability_base: listing.permission.capability_base.clone(),
        args_digest_hex: request.arguments.digest().hex.clone(),
    };
    let granted =
        h.approvals
            .grant_preauthorization(&grant_ctx, "sess_local_alpha", key.clone(), 1, 60_000);
    // CROSS-SESSION probe (the "scoped" half of the case name, made
    // explicit): the SAME target + canonical digest prepared from ANOTHER
    // session in ask mode must NOT see sess_local_alpha's grant — it parks
    // at the approval surface instead of being auto-allowed, and it does
    // not spend the grant either (the single use below still works).
    let foreign_request = ToolRequest::from_effective_arguments(
        worker_target.as_str(),
        json!({"input": "input.txt"}),
        &budget(),
    )
    .expect("request");
    let foreign_verdict = h
        .gateway
        .prepare_from_request(
            &other_session_ctx,
            CallerSurface::UserRun,
            "agent",
            user_mode(SessionPermissionMode::Ask),
            &ToolCallId::new(format!("call-{}", unique_suffix())),
            &foreign_request,
        )
        .map(|prepared| prepared.policy);
    let foreign_parks = matches!(foreign_verdict, Ok(PolicyVerdict::NeedsApproval { .. }));
    // The single use is spent through the REAL gate: an ask-mode run of the
    // SAME invocation answers from the grant (exactly one execution);
    // a second run opens a fresh pending (the grant is exhausted) and a
    // DIFFERENT session never sees the grant at all.
    set_mode(&h, SessionPermissionMode::Ask).await;
    let first = run_chain(
        &h,
        vec![tool_turn(
            worker_target.as_str(),
            json!({"input": "input.txt"}),
        )],
        "preauth: granted invocation",
    )
    .await;
    let first_journal = journal_of(&h, &first.run_id).await;
    let spent_once = first_journal.len() == 1
        && first_journal[0].receipt.as_ref().is_some_and(|r| {
            r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded && r.dispatched
        });
    let second = run_chain(
        &h,
        vec![tool_turn(
            worker_target.as_str(),
            json!({"input": "input.txt"}),
        )],
        "preauth: exhausted grant",
    )
    .await;
    let second_journal = journal_of(&h, &second.run_id).await;
    let second_parked = second_journal.is_empty()
        || second_journal[0]
            .receipt
            .as_ref()
            .is_none_or(|r| !r.dispatched);
    let scoped = granted.is_ok() && foreign_parks && spent_once && second_parked;
    record_case(
        "preauthorization-single-session-scoped",
        1,
        i64::from(scoped),
    );
}
