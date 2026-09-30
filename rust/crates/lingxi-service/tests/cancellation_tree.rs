//! R03-T03 integration: the cancellation TREE and subtask supervision
//! through the REAL service composition (real storage port incl. the new
//! non-terminal state-change transaction, real event service, real
//! kernel state machine + single finalize, real session gate, real quota
//! manager, real cancellation registry + task supervisor), driven by
//! deterministic gated doubles.
//!
//! Acceptance scenarios (taskbook R03 §4):
//! - R03-A05 取消覆盖等待态 (parameterized): one test drives the run into
//!   EACH of the three wait states — the admission QUEUE wait, the
//!   APPROVAL wait (the minimal R03-T03 interface; the full gateway is
//!   R04) and the STREAM read — sends ONE cancellation through the real
//!   ownership-checked surface and asserts: the wait EXITS, the run
//!   settles `cancelled` through the single finalize (durable
//!   `cancelling` leg first), every quota permit RETURNS and NO
//!   additional tool call happens.
//! - R03-A06 独立任务不被误杀: a parent run (its supervised model call) +
//!   a demonstrative child run linked to the parent's tree + an
//!   UNRELATED detached background task all run together; cancelling the
//!   parent stops the parent's work AND the linked child, while the
//!   unrelated background keeps running to completion.
//!
//! Honest-boundary companions (taskbook R03-T03 steps 2/4/5):
//! - a non-cooperating tool child (never yields) is REPORTED unconfirmed
//!   — the cleanup never claims quiet it did not observe;
//! - a panicking tool child is supervised: contained at the task
//!   boundary, recorded as a tool failure, the run settles loudly;
//! - the cancellation surface is diagnosable for unknown runs,
//!   already-terminal runs, cross-principal requests and
//!   active-but-driverless (dangling, restart-shaped) rows;
//! - the approval wait's approve/reject legs (zero executions on
//!   rejection) and the state-change transaction's loud rejections.
//!
//! Test-double boundary (R03 test map): the gated provider/tool/approval
//! doubles below only produce external responses at test-chosen moments;
//! every state decision belongs to the real RunSupervisor + kernel state
//! machine + storage port + cancellation tree + task supervisor.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolExecutionResult,
    ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::{
    approval, cancel, prepare_layout, CancelPolicy, HomeSource, LayeredQuotaLimits, NetworkMode,
    QuotaLimits, QuotaResource, ServiceConfig, ServiceDeps, ServiceState,
};

// ── deterministic doubles ────────────────────────────────────────────────────

/// Scripted per-session provider with optional per-(session, pop) gates:
/// a gated pop parks INSIDE next_turn — the "network stream read" wait.
struct GatedProvider {
    scripts: std::sync::Mutex<std::collections::HashMap<String, VecDeque<ProviderTurn>>>,
    arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
    gates: std::collections::HashMap<(String, usize), Arc<tokio::sync::Semaphore>>,
    next_pop: std::sync::Mutex<std::collections::HashMap<String, usize>>,
}

impl GatedProvider {
    #[allow(clippy::too_many_arguments)]
    fn new(
        scripts: Vec<(&'static str, Vec<ProviderTurn>)>,
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
        gates: Vec<((&'static str, usize), Arc<tokio::sync::Semaphore>)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scripts: std::sync::Mutex::new(
                scripts
                    .into_iter()
                    .map(|(session, script)| (session.to_string(), script.into_iter().collect()))
                    .collect(),
            ),
            arrivals,
            gates: gates
                .into_iter()
                .map(|((session, pop), gate)| ((session.to_string(), pop), gate))
                .collect(),
            next_pop: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }
}

impl TurnProviderPort for GatedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.gated".to_string(),
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
        let session = ctx.session_id.to_string();
        let pop = {
            let mut counters = self.next_pop.lock().unwrap();
            let next = counters.get(&session).copied().unwrap_or(0) + 1;
            counters.insert(session.clone(), next);
            next
        };
        self.arrivals
            .send((session.clone(), pop))
            .expect("test holds the arrivals receiver");
        let ctx_at_issue = ctx.clone();
        let gate = self.gates.get(&(session.clone(), pop)).cloned();
        let turn = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&session)
            .and_then(|queue| queue.pop_front())
            .unwrap_or_else(|| ProviderTurn::Failed {
                error: ProtocolError::new(
                    ErrorCode::UpstreamUnavailable,
                    "script exhausted",
                    false,
                ),
                retryable: false,
            });
        Box::pin(async move {
            if let Some(gate) = gate {
                // Park mid-"stream": the run is reading the network.
                let _permit = gate.acquire().await.expect("gate semaphore closed");
            }
            ProviderTurnResult::of_ctx(&ctx_at_issue, turn)
        })
    }
}

/// What the tool double does per call.
enum ToolBehavior {
    /// Park on a 0-permit semaphore (the tool I/O wait).
    ParkedUntilReleased,
    /// Return this outcome immediately.
    Immediate(ToolOutcome),
    /// Panic inside the call (supervision proof).
    Panic(&'static str),
    /// Never yield at all — a truly non-cooperating child (bounded by
    /// self-exit so the test itself terminates).
    BlockNeverYield { for_ms: u64 },
}

struct GatedTool {
    arrivals: tokio::sync::mpsc::UnboundedSender<(String, String)>,
    behavior: std::sync::Mutex<VecDeque<ToolBehavior>>,
    gate: Option<Arc<tokio::sync::Semaphore>>,
}

impl GatedTool {
    fn with_queue(
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, String)>,
        behavior: Vec<ToolBehavior>,
    ) -> Arc<Self> {
        let has_gate = behavior
            .iter()
            .any(|b| matches!(b, ToolBehavior::ParkedUntilReleased));
        Arc::new(Self {
            arrivals,
            behavior: std::sync::Mutex::new(behavior.into()),
            gate: has_gate.then(|| Arc::new(tokio::sync::Semaphore::new(0))),
        })
    }

    fn immediate(
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, String)>,
        outcome: ToolOutcome,
    ) -> Arc<Self> {
        Self::with_queue(arrivals, vec![ToolBehavior::Immediate(outcome)])
    }

    fn release(&self) {
        if let Some(gate) = &self.gate {
            gate.add_permits(1);
        }
    }

    fn drain_behavior(&self) -> ToolBehavior {
        let mut queue = self.behavior.lock().unwrap();
        queue
            .pop_front()
            .unwrap_or(ToolBehavior::Immediate(ToolOutcome::success_text(
                "default".to_string(),
            )))
    }
}

impl ToolExecutorPort for GatedTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.arrivals
            .send((ctx.session_id.to_string(), call.to_string()))
            .expect("test holds the tool arrivals receiver");
        let ctx_at_issue = ctx.clone();
        let behavior = self.drain_behavior();
        let gate = self.gate.clone();
        Box::pin(async move {
            let outcome = match behavior {
                ToolBehavior::ParkedUntilReleased => {
                    let gate = gate.expect("parked behavior implies a gate");
                    let _permit = gate.acquire().await.expect("tool gate closed");
                    ToolOutcome::success_text("parked-then-success".to_string())
                }
                ToolBehavior::Immediate(outcome) => outcome,
                ToolBehavior::Panic(message) => panic!("{message}"),
                ToolBehavior::BlockNeverYield { for_ms } => {
                    // Never awaits: the cancellation drop cannot land.
                    let deadline = std::time::Instant::now() + Duration::from_millis(for_ms);
                    while std::time::Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    ToolOutcome::success_text("blocked-through".to_string())
                }
            };
            ToolExecutionResult::of_ctx(&ctx_at_issue, outcome)
        })
    }
}

/// Minimal approval-gate double (the R03-T03 test interface): each
/// request registers a bounded pending entry and parks for the decision;
/// DROPPING the request future (what the cancellation tree does) removes
/// the pending entry — a LATE decision then returns false, mirroring the
/// incumbent ConfirmStore (abort clears the timer; a late approve never
/// resurrects the call).
struct ManualGate {
    pendings: std::sync::Mutex<
        std::collections::HashMap<String, tokio::sync::oneshot::Sender<approval::ApprovalDecision>>,
    >,
    arrivals: tokio::sync::mpsc::UnboundedSender<String>,
    decisions: std::sync::Mutex<VecDeque<approval::ApprovalDecision>>,
}

/// Drop guard of one pending approval: removes the pending entry when
/// the request future is dropped without a decision (the cancellation
/// path) — the incumbent's clearTimeout-on-abort.
struct PendingGuard<'g> {
    gate: &'g ManualGate,
    call_id: String,
    armed: bool,
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.gate.pendings.lock().unwrap().remove(&self.call_id);
        }
    }
}

impl ManualGate {
    fn parking(arrivals: tokio::sync::mpsc::UnboundedSender<String>) -> Arc<Self> {
        Arc::new(Self {
            pendings: std::sync::Mutex::new(std::collections::HashMap::new()),
            arrivals,
            decisions: std::sync::Mutex::new(VecDeque::new()),
        })
    }

    fn scripted(
        arrivals: tokio::sync::mpsc::UnboundedSender<String>,
        decisions: Vec<approval::ApprovalDecision>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pendings: std::sync::Mutex::new(std::collections::HashMap::new()),
            arrivals,
            decisions: std::sync::Mutex::new(decisions.into()),
        })
    }

    fn pending_count(&self) -> usize {
        self.pendings.lock().unwrap().len()
    }

    /// Delivers a decision to a pending request. Returns false when the
    /// request is no longer pending (aborted/dropped) — the late-decision
    /// refusal of the incumbent.
    fn decide(&self, call_id: &str, decision: approval::ApprovalDecision) -> bool {
        let sender = self.pendings.lock().unwrap().remove(call_id);
        match sender {
            Some(sender) => sender.send(decision).is_ok(),
            None => false,
        }
    }
}

impl approval::ApprovalGate for ManualGate {
    fn request<'a>(
        &'a self,
        _ctx: &'a RunContext,
        req: &'a approval::ApprovalRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = approval::ApprovalDecision> + Send + 'a>> {
        self.arrivals
            .send(req.tool_call_id.to_string())
            .expect("test holds the approval arrivals receiver");
        let scripted = self.decisions.lock().unwrap().pop_front();
        let call_id = req.tool_call_id.to_string();
        Box::pin(async move {
            match scripted {
                Some(decision) => decision,
                None => {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    self.pendings.lock().unwrap().insert(call_id.clone(), tx);
                    let mut guard = PendingGuard {
                        gate: self,
                        call_id,
                        armed: true,
                    };
                    match rx.await {
                        Ok(decision) => {
                            guard.armed = false;
                            decision
                        }
                        Err(_dropped) => approval::ApprovalDecision::Aborted,
                    }
                }
            }
        })
    }
}

fn read_tool_request() -> ToolRequest {
    ToolRequest::from_effective_arguments(
        "read",
        serde_json::json!({"path": "/tmp/x"}),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request")
    .with_summary("read /tmp/x")
}

fn assistant_final(text: &str) -> NormalizedMessage {
    NormalizedMessage {
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        model_call_id: None,
    }
}

fn gate() -> Arc<tokio::sync::Semaphore> {
    Arc::new(tokio::sync::Semaphore::new(0))
}

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t03-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn config_for(home: &std::path::Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
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
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
}

/// A device principal of a DIFFERENT user (cross-principal boundary).
fn foreign_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_device_foreign".to_string(),
        kind: lingxi_service::PrincipalKind::Device,
        user_id: Some("user_foreign".to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: Some("dev_foreign".to_string()),
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Lan,
        credential_kind: lingxi_service::CredentialKind::DeviceCredential,
        trust_state: lingxi_service::TrustState::Lan,
        scopes: vec!["chat".to_string()],
    }
}

async fn boot(tag: &str, deps: ServiceDeps) -> (ServiceState, PathBuf) {
    let home = synthetic_home(tag);
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    (state, home)
}

async fn teardown(state: &ServiceState, home: &std::path::Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

fn execute_task(
    state: &ServiceState,
    session: &'static str,
    input: &'static str,
) -> tokio::task::JoinHandle<Result<String, lingxi_service::SessionExecuteError>> {
    let state = state.clone();
    tokio::spawn(async move {
        let storage = Arc::clone(state.storage());
        state
            .sessions()
            .execute_for(
                storage.as_ref(),
                state.events(),
                state.runs(),
                &owner_principal(),
                session,
                input,
                1_790_409_600_000,
            )
            .await
            .map(|accepted| accepted.run_id)
    })
}

async fn cancel_run(
    state: &ServiceState,
    run_id: &str,
) -> Result<lingxi_service::CancelRunOutcome, lingxi_service::SessionExecuteError> {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .cancel_run_for(storage.as_ref(), state.runs(), &owner_principal(), run_id)
        .await
}

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
}

async fn run_status(state: &ServiceState, run_id: &str) -> (String, Option<String>) {
    let status = query_text(state, "SELECT status FROM runs WHERE run_id = ?1", run_id)
        .await
        .expect("run row exists");
    let reason = query_text(
        state,
        "SELECT terminal_reason FROM runs WHERE run_id = ?1",
        run_id,
    )
    .await;
    (status, reason)
}

async fn run_of_session(state: &ServiceState, session: &str) -> String {
    state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE session_id = ?1",
            vec![session.to_string()],
        )
        .await
        .expect("query runs")
        .expect("the session's run row exists")
}

async fn run_event_types(state: &ServiceState, run_id: &str) -> Vec<String> {
    let raw = state
        .storage()
        .query_one_text(
            "SELECT GROUP_CONCAT(t.event_type, ',') FROM (SELECT event_type FROM key_events \
             WHERE run_id = ?1 ORDER BY seq) AS t",
            vec![run_id.to_string()],
        )
        .await
        .expect("event order query");
    raw.map(|joined| joined.split(',').map(str::to_string).collect())
        .unwrap_or_default()
}

async fn count_tool_calls(state: &ServiceState, run_id: &str) -> i64 {
    state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = \
             'tool_call_started'",
            vec![run_id.to_string()],
        )
        .await
        .unwrap()
        .and_then(|v| v.parse().ok())
        .unwrap()
}

/// Bounded deterministic wait: polls a synchronous condition with 1ms
/// sleeps under a hard deadline (a real, bounded wait — never an
/// unbounded retry loop).
async fn wait_until(max_ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(max_ms);
    while !cond() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    true
}

/// Test evidence sink (mirrors the R02/R03 env-var pattern).
fn write_evidence(key: &str, value: serde_json::Value) {
    if let Ok(path) = std::env::var("R03_T03_EVIDENCE") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut doc = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(map) = doc.as_object_mut() {
            map.insert(key.to_string(), value);
        }
        let _ = std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap());
    }
}

// ── R03-A05: parameterized cancellation across the three wait states ────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitState {
    /// Waiting for an admission quota slot (the queued wait).
    QueueWait,
    /// Waiting for an approval decision (the minimal gate).
    ApprovalWait,
    /// Waiting inside the provider stream read.
    StreamRead,
}

impl WaitState {
    fn name(self) -> &'static str {
        match self {
            WaitState::QueueWait => "queue_wait",
            WaitState::ApprovalWait => "approval_wait",
            WaitState::StreamRead => "stream_read",
        }
    }
}

/// The acceptance scenario, parameterized over the three wait states:
/// the run parks in the chosen wait, ONE cancellation arrives through
/// the real ownership-checked surface, and the assertions prove the wait
/// exited, the quotas returned, no additional tool call happened, and
/// the run settled `cancelled` through the single finalize with the
/// durable two-phase legs.
#[tokio::test(flavor = "current_thread")]
async fn r03_a05_cancel_exits_each_wait_state_and_returns_quotas() {
    for state_kind in [
        WaitState::QueueWait,
        WaitState::ApprovalWait,
        WaitState::StreamRead,
    ] {
        println!("R03_A05_TRACE === parameter: {} ===", state_kind.name());
        a05_case(state_kind).await;
    }
}

async fn a05_case(state_kind: WaitState) {
    let tag = format!("a05-{}", state_kind.name());
    let (model_arrivals_tx, mut model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (tool_arrivals_tx, tool_arrivals_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut tool_arrivals = tool_arrivals_rx;
    let (approval_arrivals_tx, mut approval_arrivals) = tokio::sync::mpsc::unbounded_channel();

    // Per-case trailers: the unrelated lane holder (queue case) and the
    // gate double (approval case), asserted after the cancellation.
    /// The queue case's unrelated lane holder (task + tool double).
    type HolderPair = (
        tokio::task::JoinHandle<Result<String, lingxi_service::SessionExecuteError>>,
        Arc<GatedTool>,
    );
    let mut other_holder: Option<HolderPair> = None;
    let mut gate_double: Option<Arc<ManualGate>> = None;

    let (state, home) = match state_kind {
        WaitState::QueueWait => {
            // beta holds the ONLY global tool slot (parked tool call);
            // alpha's tool call parks in the admission QUEUE.
            let provider = GatedProvider::new(
                vec![
                    (
                        "sess_local_alpha",
                        vec![ProviderTurn::ToolRequests {
                            requests: vec![read_tool_request()],
                        }],
                    ),
                    (
                        "sess_local_beta",
                        vec![
                            ProviderTurn::ToolRequests {
                                requests: vec![read_tool_request()],
                            },
                            ProviderTurn::Final {
                                message: assistant_final("beta done after release"),
                            },
                        ],
                    ),
                ],
                model_arrivals_tx,
                vec![],
            );
            let tools =
                GatedTool::with_queue(tool_arrivals_tx, vec![ToolBehavior::ParkedUntilReleased]);
            let (state, home) = boot(
                &tag,
                ServiceDeps {
                    turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
                    tool_executor: Some(Arc::clone(&tools) as Arc<dyn ToolExecutorPort>),
                    quota_limits: QuotaLimits {
                        tool: LayeredQuotaLimits {
                            global: 1,
                            per_agent: 4,
                            per_session: 2,
                        },
                        ..QuotaLimits::default()
                    },
                    ..ServiceDeps::default()
                },
            )
            .await;
            let beta = execute_task(&state, "sess_local_beta", "beta holds the tool lane");
            assert_eq!(
                model_arrivals.recv().await.expect("beta turn1"),
                ("sess_local_beta".to_string(), 1)
            );
            assert!(
                wait_until(500, || {
                    state.runs().quotas().in_use(QuotaResource::Tool) == 1
                })
                .await,
                "beta holds the only tool permit"
            );
            other_holder = Some((beta, tools));
            (state, home)
        }
        WaitState::ApprovalWait => {
            // alpha's tool request parks at the APPROVAL gate (minimal
            // R03-T03 interface; the tool double would only run after an
            // approval that never comes).
            let provider = GatedProvider::new(
                vec![(
                    "sess_local_alpha",
                    vec![
                        ProviderTurn::ToolRequests {
                            requests: vec![read_tool_request()],
                        },
                        ProviderTurn::Final {
                            message: assistant_final("unreachable after cancel"),
                        },
                    ],
                )],
                model_arrivals_tx,
                vec![],
            );
            let tools = GatedTool::immediate(
                tool_arrivals_tx,
                ToolOutcome::success_text("never-reached".to_string()),
            );
            let gate = ManualGate::parking(approval_arrivals_tx);
            gate_double = Some(Arc::clone(&gate));
            boot(
                &tag,
                ServiceDeps {
                    turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
                    tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
                    approval_gate: Some(gate as Arc<dyn approval::ApprovalGate>),
                    ..ServiceDeps::default()
                },
            )
            .await
        }
        WaitState::StreamRead => {
            // alpha's FIRST model turn parks inside the stream read; the
            // script would request a tool on turn 2 — proving no tool
            // call happens after the cancellation.
            let stream_gate = gate();
            let provider = GatedProvider::new(
                vec![(
                    "sess_local_alpha",
                    vec![
                        ProviderTurn::Continue {
                            process_note: "reading the stream".to_string(),
                        },
                        ProviderTurn::ToolRequests {
                            requests: vec![read_tool_request()],
                        },
                    ],
                )],
                model_arrivals_tx,
                vec![(("sess_local_alpha", 1), stream_gate)],
            );
            let tools = GatedTool::immediate(
                tool_arrivals_tx,
                ToolOutcome::success_text("never-reached".to_string()),
            );
            boot(
                &tag,
                ServiceDeps {
                    turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
                    tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
                    ..ServiceDeps::default()
                },
            )
            .await
        }
    };
    let quotas = state.runs().quotas();

    // Drive the target run into the chosen wait state.
    let task = execute_task(&state, "sess_local_alpha", "park me");
    let run_id = match state_kind {
        WaitState::QueueWait => {
            // alpha reaches its tool admission and parks in the QUEUE
            // behind beta.
            assert_eq!(
                model_arrivals.recv().await.expect("alpha turn1"),
                ("sess_local_alpha".to_string(), 1)
            );
            assert!(
                wait_until(500, || quotas.waiting(QuotaResource::Tool) == 1).await,
                "alpha's tool call waits in the bounded admission queue"
            );
            run_of_session(&state, "sess_local_alpha").await
        }
        WaitState::ApprovalWait => {
            assert_eq!(
                model_arrivals.recv().await.expect("alpha turn1"),
                ("sess_local_alpha".to_string(), 1)
            );
            let approval_call = approval_arrivals
                .recv()
                .await
                .expect("approval request arrives");
            let run_id = run_of_session(&state, "sess_local_alpha").await;
            // The run is DURABLY in waiting_approval before the gate parks.
            let (status, _) = run_status(&state, &run_id).await;
            assert_eq!(status, "waiting_approval", "durable waiting_approval leg");
            assert_eq!(
                quotas.in_use(QuotaResource::Tool),
                1,
                "the tool permit is held across the approval wait"
            );
            assert!(approval_call.ends_with("-tc0001"));
            assert_eq!(gate_double.as_ref().unwrap().pending_count(), 1);
            run_id
        }
        WaitState::StreamRead => {
            assert_eq!(
                model_arrivals.recv().await.expect("alpha turn1"),
                ("sess_local_alpha".to_string(), 1)
            );
            run_of_session(&state, "sess_local_alpha").await
        }
    };
    println!(
        "R03_A05_TRACE [{}] 1: run parked in {} (run {run_id})",
        state_kind.name(),
        state_kind.name()
    );

    // ONE cancellation through the real ownership-checked surface.
    let outcome = cancel_run(&state, &run_id).await.expect("cancel accepted");
    match outcome {
        lingxi_service::CancelRunOutcome::Accepted { phase, .. } => {
            assert!(
                phase.cancel_requested(),
                "phase snapshot at request time: {:?}",
                phase
            );
        }
        other => panic!("cancellation must be accepted, got {other:?}"),
    }
    println!(
        "R03_A05_TRACE [{}] 2: cancellation fired (phase=requested)",
        state_kind.name()
    );

    // The wait EXITS and the run settles `cancelled` through the single
    // finalize (durable cancelling leg first).
    let settled_id = task
        .await
        .expect("drive task")
        .expect("run settles (cancelled)");
    assert_eq!(settled_id, run_id);
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));

    // Quotas returned, per case.
    match state_kind {
        WaitState::QueueWait => {
            // alpha never held the tool slot; its queue place is gone.
            assert_eq!(quotas.waiting(QuotaResource::Tool), 0);
            assert_eq!(quotas.in_use(QuotaResource::Model), 0);
            // beta still holds the lane — the cancellation did not touch
            // the unrelated holder.
            assert_eq!(quotas.in_use(QuotaResource::Tool), 1);
        }
        WaitState::ApprovalWait => {
            // The permit held across the approval wait was returned.
            assert_eq!(quotas.in_use(QuotaResource::Tool), 0);
            assert_eq!(quotas.in_use(QuotaResource::Model), 0);
        }
        WaitState::StreamRead => {
            assert_eq!(quotas.in_use(QuotaResource::Model), 0);
            assert_eq!(quotas.in_use(QuotaResource::Tool), 0);
        }
    }

    // NO additional tool EXECUTION for the cancelled run: the tool double
    // was never invoked (the queue case's unrelated holder may have its
    // own arrival in the shared channel — only alpha-tagged executions
    // count). The approval case legitimately persists a
    // `tool_call_started` FACT (the wait covers the call) while the tool
    // itself never runs — executions, not events, are the acceptance's
    // "不额外调用工具".
    let mut alpha_tool_executions = 0usize;
    while let Ok((session, _call)) = tool_arrivals.try_recv() {
        if session == "sess_local_alpha" {
            alpha_tool_executions += 1;
        }
    }
    assert_eq!(
        alpha_tool_executions, 0,
        "no additional tool execution after the cancellation"
    );

    // Durable event order per case (the two-phase cancellation legs).
    let events = run_event_types(&state, &run_id).await;
    match state_kind {
        WaitState::QueueWait => assert_eq!(
            events,
            vec![
                "run_state_changed",  // queued -> running
                "model_call_started", // turn 1 (tool request turn)
                "model_call_completed",
                "run_state_changed", // running -> cancelling
                "run_state_changed", // cancelling -> cancelled
            ],
            "queue-wait cancellation durable order"
        ),
        WaitState::ApprovalWait => assert_eq!(
            events,
            vec![
                "run_state_changed",  // queued -> running
                "model_call_started", // tool request turn
                "model_call_completed",
                "tool_call_started", // the approval wait covers this call
                "run_state_changed", // running -> waiting_approval
                "run_state_changed", // waiting_approval -> cancelling
                "run_state_changed", // cancelling -> cancelled
            ],
            "approval-wait cancellation durable order"
        ),
        WaitState::StreamRead => assert_eq!(
            events,
            vec![
                "run_state_changed", // queued -> running
                "run_state_changed", // running -> cancelling (mid-stream)
                "run_state_changed", // cancelling -> cancelled
            ],
            "stream-read cancellation durable order"
        ),
    }
    // The four-phase machine's terminal verdict stays queryable after the
    // drive ended: every cooperating case reached ConfirmedTerminated.
    // The stream/approval cases had a live supervised child (the model
    // call / the approval wait) that the tree stopped and the verdict
    // names; the queue case had NO live child (the run never entered its
    // tool call — it was waiting for admission), so its confirmed list is
    // honestly empty.
    match state.runs().cancel_registry().recent_verdict(&run_id) {
        Some(cancel::CancelPhase::ConfirmedTerminated { confirmed, .. }) => match state_kind {
            WaitState::QueueWait => assert!(
                confirmed.is_empty(),
                "no live child to stop in the queue case: {confirmed:?}"
            ),
            WaitState::ApprovalWait => assert!(
                confirmed.iter().any(|c| c.contains("approval:")),
                "the approval wait is among the confirmed children: {confirmed:?}"
            ),
            WaitState::StreamRead => assert!(
                confirmed.iter().any(|c| c.contains("model_call:")),
                "the stream read is among the confirmed children: {confirmed:?}"
            ),
        },
        other => panic!(
            "[{}] expected ConfirmedTerminated verdict, got {other:?}",
            state_kind.name()
        ),
    }
    println!(
        "R03_A05_TRACE [{}] 3: cancelled terminal + quotas returned + zero tool calls \
         (verdict=confirmed_terminated)",
        state_kind.name()
    );

    // Case trailers.
    match state_kind {
        WaitState::QueueWait => {
            // Release the unrelated holder: it completes normally; the
            // lane then returns to zero (full quota restoration).
            let (beta, tools) = other_holder.take().expect("queue trailer");
            tools.release();
            let beta_run = beta.await.unwrap().expect("beta settles");
            let (beta_status, beta_reason) = run_status(&state, &beta_run).await;
            assert_eq!(beta_status, "completed");
            assert_eq!(beta_reason.as_deref(), Some("completed.with_final"));
            assert_eq!(quotas.in_use(QuotaResource::Tool), 0);
            write_evidence(
                &format!("r03_a05_{}", state_kind.name()),
                serde_json::json!({
                    "waitState": state_kind.name(),
                    "runId": run_id,
                    "terminal": {"status": status, "reason": reason},
                    "durableEvents": events,
                    "toolExecutionsAfterCancel": alpha_tool_executions,
                    "unrelatedHolder": {"runId": beta_run, "status": beta_status,
                                         "reason": beta_reason},
                    "quotaInUseEnd": {"model": quotas.in_use(QuotaResource::Model),
                                       "tool": quotas.in_use(QuotaResource::Tool)},
                }),
            );
        }
        WaitState::ApprovalWait => {
            // A LATE approval decision is refused: the pending entry was
            // resolved away when the request future was dropped.
            let gate = gate_double.as_ref().unwrap();
            assert_eq!(
                gate.pending_count(),
                0,
                "the pending entry was cleaned on drop"
            );
            let call_id = format!("{run_id}-tc0001");
            assert!(
                !gate.decide(&call_id, approval::ApprovalDecision::Approved),
                "a late approve after the cancellation never lands"
            );
            write_evidence(
                &format!("r03_a05_{}", state_kind.name()),
                serde_json::json!({
                    "waitState": state_kind.name(),
                    "runId": run_id,
                    "terminal": {"status": status, "reason": reason},
                    "durableEvents": events,
                    "toolExecutionsAfterCancel": alpha_tool_executions,
                    "lateApproveRefused": true,
                    "quotaInUseEnd": {"model": quotas.in_use(QuotaResource::Model),
                                       "tool": quotas.in_use(QuotaResource::Tool)},
                }),
            );
        }
        WaitState::StreamRead => {
            write_evidence(
                &format!("r03_a05_{}", state_kind.name()),
                serde_json::json!({
                    "waitState": state_kind.name(),
                    "runId": run_id,
                    "terminal": {"status": status, "reason": reason},
                    "durableEvents": events,
                    "toolExecutionsAfterCancel": alpha_tool_executions,
                    "quotaInUseEnd": {"model": quotas.in_use(QuotaResource::Model),
                                       "tool": quotas.in_use(QuotaResource::Tool)},
                }),
            );
        }
    }
    teardown(&state, &home).await;
}

/// Recover the synthetic home of a booted state (the DB path embeds it).
/// (Kept for harness symmetry; tests normally keep the `home` returned
/// by [`boot`].)
#[allow(dead_code)]
async fn home_of(state: &ServiceState) -> PathBuf {
    let db = state.storage().db_path().to_path_buf();
    // {home}/lingxi-service/data/runs.db -> {home}
    db.ancestors()
        .nth(3)
        .map(|p| p.to_path_buf())
        .expect("synthetic home from db path")
}

/// The queue-wait companion (kept as its own test for a clean
/// two-session matrix): the unrelated lane holder completes normally
/// AFTER the queue-waiting run was cancelled.
#[tokio::test(flavor = "current_thread")]
async fn queue_wait_cancel_leaves_the_unrelated_holder_untouched() {
    let (model_arrivals_tx, mut model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (tool_arrivals_tx, mut tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![
            (
                "sess_local_alpha",
                vec![ProviderTurn::ToolRequests {
                    requests: vec![read_tool_request()],
                }],
            ),
            (
                "sess_local_beta",
                vec![
                    ProviderTurn::ToolRequests {
                        requests: vec![read_tool_request()],
                    },
                    ProviderTurn::Final {
                        message: assistant_final("beta completes after the cancel"),
                    },
                ],
            ),
        ],
        model_arrivals_tx,
        vec![],
    );
    let tools = GatedTool::with_queue(
        tool_arrivals_tx,
        vec![
            ToolBehavior::ParkedUntilReleased,
            ToolBehavior::Immediate(ToolOutcome::success_text("beta-tool-ok".to_string())),
        ],
    );
    let (state, home) = boot(
        "a05q2",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(Arc::clone(&tools) as Arc<dyn ToolExecutorPort>),
            quota_limits: QuotaLimits {
                tool: LayeredQuotaLimits {
                    global: 1,
                    per_agent: 4,
                    per_session: 2,
                },
                ..QuotaLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;
    let quotas = state.runs().quotas();

    // beta parks holding the lane; alpha queues behind it.
    let beta = execute_task(&state, "sess_local_beta", "beta holds the lane");
    assert_eq!(
        model_arrivals.recv().await.expect("beta turn1"),
        ("sess_local_beta".to_string(), 1)
    );
    let (beta_session, _beta_call) = tool_arrivals.recv().await.expect("beta tool starts");
    assert_eq!(beta_session, "sess_local_beta");
    assert!(
        wait_until(500, || quotas.in_use(QuotaResource::Tool) == 1).await,
        "beta holds the lane"
    );
    let alpha = execute_task(&state, "sess_local_alpha", "alpha queues");
    assert_eq!(
        model_arrivals.recv().await.expect("alpha turn1"),
        ("sess_local_alpha".to_string(), 1)
    );
    assert!(
        wait_until(500, || quotas.waiting(QuotaResource::Tool) == 1).await,
        "alpha queued"
    );
    let alpha_run = run_of_session(&state, "sess_local_alpha").await;

    // Cancel ONLY the queue-waiting run.
    cancel_run(&state, &alpha_run).await.expect("accepted");
    let alpha_run = alpha.await.unwrap().expect("alpha settles");
    let (status, reason) = run_status(&state, &alpha_run).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    assert_eq!(quotas.waiting(QuotaResource::Tool), 0, "queue place freed");
    assert_eq!(quotas.in_use(QuotaResource::Tool), 1, "beta still holds");
    assert_eq!(
        count_tool_calls(&state, &alpha_run).await,
        0,
        "the cancelled run never called the tool"
    );

    // Release beta: it completes normally; the lane returns.
    tools.release();
    let beta_run = beta.await.unwrap().expect("beta settles");
    let (beta_status, beta_reason) = run_status(&state, &beta_run).await;
    assert_eq!(beta_status, "completed");
    assert_eq!(beta_reason.as_deref(), Some("completed.with_final"));
    assert_eq!(quotas.in_use(QuotaResource::Tool), 0);

    write_evidence(
        "r03_a05_queue_companion",
        serde_json::json!({
            "cancelledRun": {"runId": alpha_run, "status": status, "reason": reason},
            "unrelatedHolder": {"runId": beta_run, "status": beta_status,
                                 "reason": beta_reason},
            "toolInUseAfterAll": quotas.in_use(QuotaResource::Tool),
        }),
    );
    teardown(&state, &home).await;
}

// ── R03-A06: cancel the parent, spare the unrelated background ──────────────

/// The acceptance scenario verbatim: the parent task (its supervised
/// model call) + a demonstrative CHILD RUN linked to the parent's tree +
/// an UNRELATED background task run together; cancelling the parent
/// stops the parent's work and the linked child, while the unrelated
/// background keeps running to completion.
#[tokio::test(flavor = "current_thread")]
async fn r03_a06_cancel_parent_spares_unrelated_background() {
    let (model_arrivals_tx, mut model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let stream_gate = gate();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                ProviderTurn::Continue {
                    process_note: "parent stream".to_string(),
                },
                ProviderTurn::Final {
                    message: assistant_final("parent would finish"),
                },
            ],
        )],
        model_arrivals_tx,
        vec![(("sess_local_alpha", 1), stream_gate)],
    );
    let (tool_arrivals_tx, _tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let tools = GatedTool::immediate(
        tool_arrivals_tx,
        ToolOutcome::success_text("unused".to_string()),
    );
    let (state, home) = boot(
        "a06",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Parent run: parks reading its first stream.
    let parent = execute_task(&state, "sess_local_alpha", "parent task");
    assert_eq!(
        model_arrivals.recv().await.expect("parent turn1"),
        ("sess_local_alpha".to_string(), 1)
    );
    let parent_run = run_of_session(&state, "sess_local_alpha").await;
    let parent_scope = state
        .runs()
        .run_scope(&parent_run)
        .expect("live parent scope");

    // Demonstrative CHILD RUN linked to the parent's tree (the real
    // subagent surface is R03-T06; this is the supervised child-run link
    // of the cancellation tree). R03 repair G01/F02: run-level children
    // observe the tree cooperatively — this one parks until the tree
    // fires, then finishes its own teardown (the exact shape a child
    // run's drive presents today).
    let child_scope = parent_scope.child("child_run:demo".to_string(), cancel::ScopeKind::ChildRun);
    let child_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let child_flag = Arc::clone(&child_started);
    let child_wait_scope = child_scope.clone();
    let child_handle = state
        .runs()
        .task_supervisor()
        .spawn_linked(
            &parent_run,
            &child_scope,
            "child_run:demo".to_string(),
            async move {
                child_flag.store(true, std::sync::atomic::Ordering::Release);
                // Parks until the tree fires, then completes its own
                // teardown (bounded by the cooperative window).
                child_wait_scope.cancelled().await;
            },
        )
        .expect("child run spawn");
    assert!(
        wait_until(500, || child_started
            .load(std::sync::atomic::Ordering::Acquire))
        .await,
        "child run started"
    );

    // UNRELATED background task: detached (no run scope) — keeps ticking
    // no matter what happens to the parent.
    let ticks = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let tick_counter = Arc::clone(&ticks);
    let background = state
        .runs()
        .task_supervisor()
        .spawn_detached("background:heartbeat".to_string(), async move {
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(2)).await;
                tick_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            "background done"
        })
        .expect("background spawn");

    // Supervision relations BEFORE the cancellation: the parent's live
    // children are its model call + the child run; the background is
    // owned by nobody.
    let live = state.runs().task_supervisor().live_children_of(&parent_run);
    let labels: Vec<&str> = live.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(
        live.len(),
        2,
        "supervision: the parent owns its model call + the child run: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("model_call:")),
        "the supervised model call is a live child: {labels:?}"
    );
    assert!(
        labels.contains(&"child_run:demo"),
        "the child run is a live child: {labels:?}"
    );
    let background_ref = state
        .runs()
        .task_supervisor()
        .tasks()
        .into_iter()
        .find(|t| t.label == "background:heartbeat")
        .expect("background task registered");
    assert!(
        background_ref.run_id.is_none(),
        "background has no run owner"
    );
    assert!(background_ref.exit.is_none(), "background still running");
    // Let the background prove ACTIVITY before the cancellation lands.
    assert!(
        wait_until(500, || ticks.load(std::sync::atomic::Ordering::Acquire)
            >= 1)
        .await,
        "the background task is demonstrably active"
    );
    println!(
        "R03_A06_TRACE 1: parent({parent_run}) + child_run + background all live \
         (children: {labels:?})"
    );

    // Cancel the PARENT through the real surface.
    let outcome = cancel_run(&state, &parent_run).await.expect("accepted");
    assert!(matches!(
        outcome,
        lingxi_service::CancelRunOutcome::Accepted { .. }
    ));

    // The parent settles cancelled; the four-phase machine reached its
    // terminal phase with BOTH children confirmed.
    let settled = parent.await.unwrap().expect("parent settles");
    assert_eq!(settled, parent_run);
    let (status, reason) = run_status(&state, &parent_run).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    // The child run exited through the tree (its supervision entry
    // confirmed by the parent's bounded cleanup drain).
    let live_after = state.runs().task_supervisor().live_children_of(&parent_run);
    assert!(
        live_after.is_empty(),
        "all parent children stopped: {live_after:?}"
    );
    child_handle
        .wait()
        .await
        .expect("child finished its own teardown");
    println!("R03_A06_TRACE 2: parent cancelled, child_run stopped through the tree");

    // The UNRELATED background CONTINUES: it is still running right now
    // and completes its full tick budget afterwards.
    let background_still_running = state
        .runs()
        .task_supervisor()
        .tasks()
        .into_iter()
        .any(|t| t.label == "background:heartbeat" && t.exit.is_none());
    assert!(
        background_still_running,
        "the unrelated background task survived the parent cancellation"
    );
    let ticks_at_cancel = ticks.load(std::sync::atomic::Ordering::Acquire);
    let background_value = background.wait().await.expect("background completes");
    assert_eq!(background_value, "background done");
    assert_eq!(ticks.load(std::sync::atomic::Ordering::Acquire), 20);
    assert!(ticks_at_cancel > 0, "it was alive before the cancellation");
    // The four-phase verdict of the settled parent: BOTH children (its
    // model call + the child run) confirmed — the unrelated background is
    // not among them (it was never the parent's child).
    match state.runs().cancel_registry().recent_verdict(&parent_run) {
        Some(cancel::CancelPhase::ConfirmedTerminated { confirmed, .. }) => {
            assert_eq!(confirmed.len(), 2, "both children confirmed: {confirmed:?}");
            assert!(
                confirmed.iter().any(|c| c.contains("child_run:demo")),
                "the child run's confirmation is in the verdict: {confirmed:?}"
            );
            assert!(
                !confirmed.iter().any(|c| c.contains("background")),
                "the unrelated background is NOT a cancelled child: {confirmed:?}"
            );
        }
        other => panic!("expected ConfirmedTerminated verdict, got {other:?}"),
    }
    println!(
        "R03_A06_TRACE 3: background completed all 20 ticks after the cancel \
         (ticks at cancel: {ticks_at_cancel})"
    );

    write_evidence(
        "r03_a06",
        serde_json::json!({
            "parentRun": {"runId": parent_run, "status": status, "reason": reason},
            "childrenConfirmed": 2,
            "childRunExit": "completed (cooperative teardown)",
            "background": {"survivedCancel": background_still_running,
                           "ticksAtCancel": ticks_at_cancel,
                           "ticksTotal": ticks.load(std::sync::atomic::Ordering::Acquire)},
        }),
    );
    teardown(&state, &home).await;
}

// ── honest boundaries ────────────────────────────────────────────────────────

/// A truly non-cooperating tool child (never yields) cannot confirm its
/// stop within the cleanup budget: the drain reports it unconfirmed and
/// the registry keeps the entry (no exit recorded) — never a fake quiet.
/// The run itself still settles `cancelled` (the cancellation is not
/// revoked by an uncooperative child).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn noncooperating_child_is_reported_unconfirmed_not_fake_quiet() {
    let (model_arrivals_tx, mut model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![ProviderTurn::ToolRequests {
                requests: vec![read_tool_request()],
            }],
        )],
        model_arrivals_tx,
        vec![],
    );
    let (tool_arrivals_tx, mut tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let tools = GatedTool::with_queue(
        tool_arrivals_tx,
        vec![ToolBehavior::BlockNeverYield { for_ms: 300 }],
    );
    let (state, home) = boot(
        "unconf",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
            cancel_policy: CancelPolicy {
                cleanup_grace_ms: 50,
                ..CancelPolicy::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;

    let task = execute_task(&state, "sess_local_alpha", "uses a blocking tool");
    assert_eq!(
        model_arrivals.recv().await.expect("turn1"),
        ("sess_local_alpha".to_string(), 1)
    );
    let run_id = run_of_session(&state, "sess_local_alpha").await;
    // The blocking tool child is live before we cancel (its arrival is
    // the deterministic anchor).
    let (session, call) = tool_arrivals.recv().await.expect("blocking tool started");
    assert_eq!(session, "sess_local_alpha");
    assert!(call.ends_with("-tc0001"));

    cancel_run(&state, &run_id).await.expect("accepted");
    let settled = task.await.unwrap().expect("settles");
    let (status, reason) = run_status(&state, &settled).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    // The unconfirmed child stays REPORTED in the supervisor registry:
    // no exit was ever recorded for it within the budget.
    let unconfirmed = state
        .runs()
        .task_supervisor()
        .tasks()
        .into_iter()
        .filter(|t| t.run_id.as_deref() == Some(settled.as_str()))
        .collect::<Vec<_>>();
    assert!(
        !unconfirmed.is_empty(),
        "the unconfirmed child stays reported in the supervisor registry"
    );
    assert!(
        unconfirmed.iter().all(|t| t.exit.is_none()),
        "the non-cooperating child has no confirmed exit: {unconfirmed:?}"
    );
    // The four-phase verdict names it: StopUnconfirmed — never a fake
    // quiet, and the cancellation itself still settled the run.
    match state.runs().cancel_registry().recent_verdict(&settled) {
        Some(cancel::CancelPhase::StopUnconfirmed {
            unconfirmed: names, ..
        }) => {
            assert_eq!(names.len(), 1, "the unconfirmed list: {names:?}");
            assert!(
                names[0].contains("tool_call:"),
                "the blocking tool child is the reported item: {names:?}"
            );
        }
        other => panic!("expected StopUnconfirmed verdict, got {other:?}"),
    }
    write_evidence(
        "r03_t03_unconfirmed",
        serde_json::json!({
            "runId": settled,
            "terminal": {"status": status, "reason": reason},
            "unconfirmedChildren": unconfirmed.len(),
            "verdict": "stop_unconfirmed",
            "cleanupBudgetMs": 50,
        }),
    );
    teardown(&state, &home).await;
}

/// A panicking tool child is SUPERVISED: the panic is contained at the
/// task boundary, the run records the tool failure and settles loudly
/// (`completed.no_final.tool_partial_failure`) — the request task never
/// crashes.
#[tokio::test(flavor = "current_thread")]
async fn panicking_tool_child_is_supervised_and_the_run_settles_loudly() {
    let (model_arrivals_tx, _model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                ProviderTurn::ToolRequests {
                    requests: vec![read_tool_request()],
                },
                ProviderTurn::Empty {
                    detail: "model saw the tool failure".to_string(),
                },
            ],
        )],
        model_arrivals_tx,
        vec![],
    );
    let (tool_arrivals_tx, _tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let tools = GatedTool::with_queue(
        tool_arrivals_tx,
        vec![ToolBehavior::Panic("tool double exploded")],
    );
    let (state, home) = boot(
        "panic",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_task(&state, "sess_local_alpha", "use the panicking tool")
        .await
        .unwrap()
        .expect("the panic was contained — the run settles");
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        reason.as_deref(),
        Some("completed.no_final.tool_partial_failure"),
        "the supervised panic settles as a loud tool failure"
    );
    // The failure fact is durable (tool_call_completed with the error).
    let events = run_event_types(&state, &run_id).await;
    assert_eq!(
        events,
        vec![
            "run_state_changed",
            "model_call_started",
            "model_call_completed",
            "tool_call_started",
            "tool_call_completed",
            "model_call_started",
            "model_call_completed",
            "run_state_changed",
        ],
        "the panic became a recorded tool failure, not a crash"
    );
    write_evidence(
        "r03_t03_panic_supervision",
        serde_json::json!({
            "runId": run_id,
            "terminal": {"status": status, "reason": reason},
            "contained": true,
        }),
    );
    teardown(&state, &home).await;
}

/// The cancellation surface is diagnosable: unknown runs, terminal runs,
/// foreign principals and restart-shaped (driverless) active rows each
/// get their own explicit outcome — nothing is silently absorbed.
#[tokio::test(flavor = "current_thread")]
async fn cancel_surface_is_diagnosable_for_unknown_terminal_foreign_and_dangling() {
    let (model_arrivals_tx, mut model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![
            (
                "sess_local_alpha",
                vec![ProviderTurn::Continue {
                    process_note: "parks".to_string(),
                }],
            ),
            (
                "sess_local_beta",
                vec![ProviderTurn::Final {
                    message: assistant_final("beta completes"),
                }],
            ),
        ],
        model_arrivals_tx,
        vec![(("sess_local_alpha", 1), gate())],
    );
    let (tool_arrivals_tx, _tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let tools = GatedTool::immediate(
        tool_arrivals_tx,
        ToolOutcome::success_text("unused".to_string()),
    );
    let (state, home) = boot(
        "diag",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let storage = Arc::clone(state.storage());

    // Unknown run id: NotFound (nothing to cancel).
    let unknown = state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &owner_principal(),
            "run_ffffffffffffffff_ffffff",
        )
        .await;
    assert!(matches!(
        unknown,
        Err(lingxi_service::SessionExecuteError::NotFound)
    ));

    // A completed run: AlreadyTerminal (the incumbent's diagnosable
    // no-op false).
    let beta = execute_task(&state, "sess_local_beta", "finish");
    let beta_run = beta.await.unwrap().expect("beta settles (durable)");
    match state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &owner_principal(),
            &beta_run,
        )
        .await
        .unwrap()
    {
        lingxi_service::CancelRunOutcome::AlreadyTerminal { status, .. } => {
            assert_eq!(status, lingxi_protocol::RunStatus::Completed);
        }
        other => panic!("terminal run must be AlreadyTerminal, got {other:?}"),
    }

    // A foreign principal may not cancel the local owner's run.
    let alpha = execute_task(&state, "sess_local_alpha", "parks");
    // (beta's turn arrival is still queued ahead of alpha's — FIFO.)
    let mut saw_alpha = false;
    for _ in 0..2 {
        let (session, _pop) = model_arrivals.recv().await.expect("turn arrival");
        if session == "sess_local_alpha" {
            saw_alpha = true;
        }
    }
    assert!(saw_alpha, "alpha's turn1 arrival observed");
    let alpha_run = run_of_session(&state, "sess_local_alpha").await;
    let foreign = state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &foreign_principal(),
            &alpha_run,
        )
        .await;
    assert!(
        matches!(foreign, Err(lingxi_service::SessionExecuteError::Forbidden)),
        "cross-principal cancellation is forbidden"
    );

    // Dangling active row: the driving future disappears without a
    // finalize (tokio abort — the same drop a dead transport performs).
    // The guard cancels the tree, marks the entry abandoned and leaves
    // the durable row ACTIVE; the surface reports the explainable
    // dangling state (recovery classification = R03-T07).
    alpha.abort();
    let _ = alpha.await;
    let (alpha_status, alpha_reason) = run_status(&state, &alpha_run).await;
    assert_eq!(
        alpha_status, "running",
        "no fabricated terminal after abandonment"
    );
    assert_eq!(alpha_reason, None);
    assert!(
        state.runs().cancel_registry().get(&alpha_run).is_none(),
        "the abandoned entry left the live registry"
    );
    // The guard's honest verdict: Abandoned (tree fired, no finalize, the
    // durable row stays active for the R03-T07 startup scan).
    match state.runs().cancel_registry().recent_verdict(&alpha_run) {
        Some(cancel::CancelPhase::Abandoned { .. }) => {}
        other => panic!("expected Abandoned verdict, got {other:?}"),
    }
    match state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &owner_principal(),
            &alpha_run,
        )
        .await
        .unwrap()
    {
        lingxi_service::CancelRunOutcome::DanglingActive { status, detail, .. } => {
            assert_eq!(status, lingxi_protocol::RunStatus::Running);
            assert!(
                detail.contains("R03-T07"),
                "the dangling detail names the recovery owner: {detail}"
            );
        }
        other => panic!("driverless active row must be DanglingActive, got {other:?}"),
    }

    write_evidence(
        "r03_t03_cancel_surface",
        serde_json::json!({
            "unknownRun": "NotFound",
            "terminalRun": "AlreadyTerminal(completed)",
            "foreignPrincipal": "Forbidden",
            "danglingActive": {"runId": alpha_run, "durableStatus": alpha_status,
                               "verdict": "abandoned",
                               "recoveryOwner": "R03-T07"},
        }),
    );
    teardown(&state, &home).await;
}

/// The approval wait's normal legs (the minimal interface both ways):
/// APPROVED executes the tool and completes with a final answer;
/// REJECTED skips execution (zero tool calls) and settles as a loud
/// tool-partial-failure — both with the durable waiting_approval round
/// trip in the event order.
#[tokio::test(flavor = "current_thread")]
async fn approval_wait_round_trips_approve_and_reject_legs() {
    // ── approved leg ──
    let (model_arrivals_tx, _model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (approval_arrivals_tx, _approval_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                ProviderTurn::ToolRequests {
                    requests: vec![read_tool_request()],
                },
                ProviderTurn::Final {
                    message: assistant_final("answer after the approved tool"),
                },
            ],
        )],
        model_arrivals_tx,
        vec![],
    );
    let (tool_arrivals_tx, mut tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let tools = GatedTool::immediate(
        tool_arrivals_tx,
        ToolOutcome::success_text("approved-tool".to_string()),
    );
    let (state, home) = boot(
        "appr-ok",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
            approval_gate: Some(ManualGate::scripted(
                approval_arrivals_tx,
                vec![approval::ApprovalDecision::Approved],
            ) as Arc<dyn approval::ApprovalGate>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let approved_run = execute_task(&state, "sess_local_alpha", "approved tool")
        .await
        .unwrap()
        .expect("settles");
    let (status, reason) = run_status(&state, &approved_run).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(
        tool_arrivals
            .try_recv()
            .expect("the tool EXECUTED after approval")
            .0,
        "sess_local_alpha"
    );
    let events = run_event_types(&state, &approved_run).await;
    assert_eq!(
        events,
        vec![
            "run_state_changed",  // queued -> running
            "model_call_started", // tool request turn
            "model_call_completed",
            "tool_call_started",
            "run_state_changed", // running -> waiting_approval
            "run_state_changed", // waiting_approval -> running
            "tool_call_completed",
            "model_call_started", // final turn
            "model_call_completed",
            "run_state_changed", // running -> completed
            "final_message_committed",
        ],
        "the durable waiting_approval round trip is in the event order"
    );
    teardown(&state, &home).await;

    // ── rejected leg ──
    let (model_arrivals_tx, _model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (approval_arrivals_tx, _approval_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                ProviderTurn::ToolRequests {
                    requests: vec![read_tool_request()],
                },
                ProviderTurn::Empty {
                    detail: "nothing more after rejection".to_string(),
                },
            ],
        )],
        model_arrivals_tx,
        vec![],
    );
    let (tool_arrivals_tx, mut tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let tools = GatedTool::immediate(
        tool_arrivals_tx,
        ToolOutcome::success_text("never".to_string()),
    );
    let (state, home) = boot(
        "appr-no",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
            approval_gate: Some(ManualGate::scripted(
                approval_arrivals_tx,
                vec![approval::ApprovalDecision::Rejected {
                    reason: "policy says no".to_string(),
                }],
            ) as Arc<dyn approval::ApprovalGate>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let rejected_run = execute_task(&state, "sess_local_alpha", "rejected tool")
        .await
        .unwrap()
        .expect("settles");
    let (rejected_status, rejected_reason) = run_status(&state, &rejected_run).await;
    assert_eq!(rejected_status, "completed");
    assert_eq!(
        rejected_reason.as_deref(),
        Some("completed.no_final.tool_partial_failure"),
        "the rejection is a loud tool failure, never a silent skip"
    );
    assert!(
        tool_arrivals.try_recv().is_err(),
        "ZERO tool executions on rejection"
    );
    write_evidence(
        "r03_t03_approval_legs",
        serde_json::json!({
            "approved": {"runId": approved_run, "reason": reason,
                          "executed": true},
            "rejected": {"runId": rejected_run, "reason": rejected_reason,
                          "executed": false},
        }),
    );
    teardown(&state, &home).await;
}

/// The non-terminal state-change transaction itself (adapter level):
/// legal legs commit with the event; a stale `from` and a terminal
/// target are diagnosed loudly; the cancelled finalize works from the
/// durable `cancelling` leg.
#[tokio::test(flavor = "current_thread")]
async fn state_change_transaction_commits_legal_legs_and_diagnoses_bad_ones() {
    use lingxi_kernel::ports::{KeyEvent, RunOutcome};
    use lingxi_protocol::{
        EventId, EventPayload, KnownEventPayload, RunStateChangedPayload, RunStatus,
    };

    let home = synthetic_home("sc-txn");
    let layout = prepare_layout(&home).expect("layout");
    let state =
        ServiceState::bootstrap_with_deps(config_for(&home), &layout, ServiceDeps::default())
            .await
            .expect("bootstrap");
    let storage = Arc::clone(state.storage());
    let ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: lingxi_protocol::RunId::new("run_manual_0000000000000000_000001".to_string()),
        attempt: lingxi_protocol::AttemptId::new(
            "run_manual_0000000000000000_000001#a1".to_string(),
        ),
        generation: 1,
    };
    let started = storage
        .record_run_started(&ctx, 1_000)
        .await
        .expect("start");
    assert!(started.newly_committed);

    // Legal leg: running -> waiting_approval.
    let changed = storage
        .record_run_state_change(
            &ctx,
            RunStatus::Running,
            RunStatus::WaitingApproval,
            Some("approval_required".to_string()),
            2_000,
        )
        .await
        .expect("legal leg commits");
    assert_eq!(changed.events.len(), 1);

    // Stale `from` (the row is already waiting_approval): diagnosed.
    let stale = storage
        .record_run_state_change(&ctx, RunStatus::Running, RunStatus::Cancelling, None, 3_000)
        .await;
    assert!(matches!(
        stale,
        Err(lingxi_kernel::ports::StorageError::Conflict { .. })
    ));

    // Terminal target on this surface: refused.
    let terminal = storage
        .record_run_state_change(
            &ctx,
            RunStatus::WaitingApproval,
            RunStatus::Cancelled,
            None,
            4_000,
        )
        .await;
    assert!(
        matches!(
            terminal,
            Err(lingxi_kernel::ports::StorageError::InvalidRequest { .. })
        ),
        "terminals belong to commit_run_outcome"
    );

    // Back to running, then cancelling — the two-phase legs work from
    // waiting_approval as well.
    storage
        .record_run_state_change(
            &ctx,
            RunStatus::WaitingApproval,
            RunStatus::Running,
            Some("approval_resolved".to_string()),
            5_000,
        )
        .await
        .expect("back to running");
    storage
        .record_run_state_change(
            &ctx,
            RunStatus::Running,
            RunStatus::Cancelling,
            Some("cancelled:user".to_string()),
            6_000,
        )
        .await
        .expect("cancelling leg");
    let (status, _) = run_status(&state, ctx.run_id.as_str()).await;
    assert_eq!(status, "cancelling");
    // …and the cancelled finalize from the durable cancelling leg.
    let outcome = RunOutcome {
        status: RunStatus::Cancelled,
        reason: Some("cancelled.requested".to_string()),
        key_events: vec![KeyEvent {
            event_id: EventId::new(format!("{}-done", ctx.run_id.as_str())),
            payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                RunStateChangedPayload {
                    from: RunStatus::Cancelling,
                    to: RunStatus::Cancelled,
                    reason: Some("cancelled.requested".to_string()),
                },
            )),
        }],
        final_message: None,
    };
    storage
        .commit_run_outcome(&ctx, outcome, 7_000)
        .await
        .expect("cancelled finalize from the durable cancelling leg");
    let (status, reason) = run_status(&state, ctx.run_id.as_str()).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    teardown(&state, &home).await;
}
