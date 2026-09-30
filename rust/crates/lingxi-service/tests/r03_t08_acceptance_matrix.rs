//! R03-T08 acceptance matrix (R03-A15 + the leaf-case producer for the
//! R03 stage map's supplemental-leaf assertion contracts).
//!
//! Test-double boundary (stage book R03-T08 / A15): the doubles below
//! produce EXTERNAL RESPONSES only — provider turns, tool outcomes,
//! approval decisions. They never write run state, never finalize, never
//! touch storage: every state, event and terminal in every scenario
//! below comes from the REAL chain (HTTP-facing `SessionStore` service
//! entry → `SessionSupervisor` busy gate → `RunSupervisor::drive_run` →
//! kernel state machine/fence → the real `RunDatabase` single-writer
//! transactions → the real `EventService`).
//!
//! Combination matrix (≥ the stage book's list): normal, multi-turn
//! model, multi-tool, timeout (turn budget), cancel (stream read +
//! approval wait), duplicate (idempotent replay + conflict), out-of-order
//! (late result fenced after cancel), cross-session parallel, crash
//! recovery (drive lost mid-run → real startup scan → honest
//! `interrupted_needs_attention`). Each scenario records task / call /
//! terminal counts as machine-checkable cases.
//!
//! Case recording: when `R03_T08_EVIDENCE_DIR` is set, every case writes
//! a JSON fragment `<dir>/cases/<case>.json` (`lingxi.leaf-case-entry.v1`);
//! the wrapper script assembles the `lingxi.leaf-case-results.v1` files
//! the stage gate consumes. Without the env var the tests run as plain
//! assertions (the default `cargo test` path stays green).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use lingxi_kernel::ports::ProviderTurn::ToolRequests;
use lingxi_kernel::ports::{
    DelegationRequest, ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult,
    ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError};
use lingxi_service::approval::ApprovalRequest;
use lingxi_service::{
    approval, prepare_layout, CancelRunOutcome, ExecuteSubmission, RunDriveLimits, ServiceConfig,
    ServiceDeps, ServiceState, SteerOutcome, SubscribeOutcome,
};
use lingxi_service::{HomeSource, NetworkMode};

const NOW_MS: u64 = 1_790_409_600_000;

// ── case recording (evidence fragments) ────────────────────────────────────

fn record_case(case: &str, expect: i64, actual: i64) {
    let ok = actual == expect;
    println!("R03_T08_CASE case={case} expect={expect} actual={actual} ok={ok}");
    assert!(
        ok,
        "case {case}: expected {expect}, observed {actual} (real-chain observation)"
    );
    if let Ok(dir) = std::env::var("R03_T08_EVIDENCE_DIR") {
        let dir = PathBuf::from(dir).join("cases");
        let _ = std::fs::create_dir_all(&dir);
        let doc = serde_json::json!({
            "schema": "lingxi.leaf-case-entry.v1",
            "case": case,
            "expect": expect,
            "actual": actual,
            "ok": ok,
        });
        let _ = std::fs::write(
            dir.join(format!("{case}.json")),
            serde_json::to_vec_pretty(&doc).expect("fragment serializes"),
        );
    }
}

/// Records a combo's task/call/terminal counters (the A15 observation
/// payload). `combo-counts.json` is assembled by the wrapper from these
/// fragments.
fn record_combo_counts(combo: &str, doc: serde_json::Value) {
    println!("R03_T08_COMBO {combo}: {doc}");
    if let Ok(dir) = std::env::var("R03_T08_EVIDENCE_DIR") {
        let dir = PathBuf::from(dir).join("combos");
        let _ = std::fs::create_dir_all(&dir);
        let wrapped = serde_json::json!({
            "schema": "lingxi.r03-t08-combo-counts.v1",
            "combo": combo,
            "counts": doc,
        });
        let _ = std::fs::write(
            dir.join(format!("{combo}.json")),
            serde_json::to_vec_pretty(&wrapped).expect("combo fragment serializes"),
        );
    }
}

// ── deterministic doubles (external responses only) ────────────────────────

enum Step {
    Turn(ProviderTurn),
    Dispatch {
        task: &'static str,
        access_write: bool,
    },
    Reply {
        task: &'static str,
    },
    Close {
        reason: &'static str,
    },
    Final(&'static str),
}

struct ScriptedProvider {
    scripts: Mutex<HashMap<String, VecDeque<Step>>>,
    gates: Mutex<HashMap<(String, usize), Arc<tokio::sync::Semaphore>>>,
    pops: Mutex<HashMap<String, usize>>,
    observed: Mutex<Vec<(String, String, String)>>, // (attempt, call, full input)
    thread: Arc<Mutex<Option<String>>>,
}

impl ScriptedProvider {
    fn new(scripts: Vec<(&str, Vec<Step>)>) -> Arc<Self> {
        Arc::new(Self {
            scripts: Mutex::new(
                scripts
                    .into_iter()
                    .map(|(m, s)| (m.to_string(), s.into_iter().collect()))
                    .collect(),
            ),
            gates: Mutex::new(HashMap::new()),
            pops: Mutex::new(HashMap::new()),
            observed: Mutex::new(Vec::new()),
            thread: Arc::new(Mutex::new(None)),
        })
    }

    fn gate(&self, marker: &str, pop: usize) -> Arc<tokio::sync::Semaphore> {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        self.gates
            .lock()
            .unwrap()
            .insert((marker.to_string(), pop), Arc::clone(&gate));
        gate
    }

    fn set_thread(&self, thread: String) {
        *self.thread.lock().unwrap() = Some(thread);
    }

    fn calls_of(&self, marker: &str) -> usize {
        self.observed
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, _, input)| marker_of(input) == marker)
            .count()
    }

    fn total_calls(&self) -> usize {
        self.observed.lock().unwrap().len()
    }

    fn inputs_of(&self, marker: &str) -> Vec<String> {
        self.observed
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, _, input)| marker_of(input) == marker)
            .map(|(_, _, input)| input.clone())
            .collect()
    }
}

fn marker_of(input: &str) -> String {
    input.split(':').next().unwrap_or(input).trim().to_string()
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.scripted.matrix".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ModelCallId,
        _turn: u32,
        input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let marker = marker_of(input);
        let pop = {
            let mut pops = self.pops.lock().unwrap();
            let next = pops.get(&marker).copied().unwrap_or(0) + 1;
            pops.insert(marker.clone(), next);
            next
        };
        let gate = self
            .gates
            .lock()
            .unwrap()
            .get(&(marker.clone(), pop))
            .cloned();
        self.observed.lock().unwrap().push((
            ctx.attempt.to_string(),
            _call.to_string(),
            input.to_string(),
        ));
        let step = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&marker)
            .and_then(|queue| queue.pop_front());
        let thread_slot = Arc::clone(&self.thread);
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await.expect("provider gate closed");
            }
            // The thread id is resolved AFTER the gate: a parked turn must
            // observe the state the test arranged while it waits.
            let thread = thread_slot.lock().unwrap().clone();
            let turn = match step {
                Some(Step::Turn(turn)) => turn,
                Some(Step::Final(text)) => final_message(text),
                Some(Step::Reply { task }) => {
                    let thread_id = thread.expect("the test filled the thread slot");
                    delegation_turn("subagent_reply", task, false, Some(thread_id))
                }
                Some(Step::Close { reason }) => {
                    let thread_id = thread.expect("the test filled the thread slot");
                    delegation_turn("subagent_close", reason, false, Some(thread_id))
                }
                Some(Step::Dispatch { task, access_write }) => {
                    delegation_turn("subagent", task, access_write, None)
                }
                None => ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        "script exhausted",
                        false,
                    ),
                    retryable: false,
                },
            };
            ProviderTurnResult::of_ctx(&ctx_at_issue, turn)
        })
    }
}

fn delegation_turn(
    target: &str,
    task: &str,
    access_write: bool,
    thread_id: Option<String>,
) -> ProviderTurn {
    ToolRequests {
        requests: vec![ToolRequest::from_effective_arguments(
            target,
            serde_json::json!({
                "task": task,
            }),
            &lingxi_kernel::toolcatalog::SchemaBudget::default(),
        )
        .expect("effective tool request")
        .with_summary(format!("{target}: {task}"))
        .with_delegation(DelegationRequest {
            task: task.to_string(),
            access: if access_write {
                Some(lingxi_kernel::subagent::AccessRequest::Write)
            } else {
                None
            },
            label: None,
            agent_id: None,
            model: None,
            thread_id,
        })],
    }
}

fn final_message(text: &str) -> ProviderTurn {
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

fn read_tool(n: u32) -> ToolRequest {
    ToolRequest::from_effective_arguments(
        format!("read{n}"),
        serde_json::json!({"path": "/tmp/x"}),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request")
    .with_summary(format!("read /tmp/x #{n}"))
}

fn tool_requests(count: u32) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        requests: (1..=count).map(read_tool).collect(),
    }
}

/// Tool double: counts arrivals per target (external responder only).
struct CountingTool {
    arrivals: Mutex<Vec<String>>,
}

impl CountingTool {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            arrivals: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> usize {
        self.arrivals.lock().unwrap().len()
    }
}

impl ToolExecutorPort for CountingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a lingxi_protocol::ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.arrivals.lock().unwrap().push(request.target.clone());
        let ctx_at_issue = ctx.clone();
        let digest = format!("executed:{}", request.target);
        Box::pin(async move {
            ToolExecutionResult::of_ctx(&ctx_at_issue, ToolOutcome::success_text(digest))
        })
    }
}

/// Approval gate double: parks until the test decides.
struct ManualGate {
    decisions: Mutex<HashMap<String, approval::ApprovalDecision>>,
}

impl ManualGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            decisions: Mutex::new(HashMap::new()),
        })
    }
}

impl approval::ApprovalGate for ManualGate {
    fn request<'a>(
        &'a self,
        _ctx: &'a lingxi_kernel::RunContext,
        req: &'a ApprovalRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = approval::ApprovalDecision> + Send + 'a>> {
        let call = req.tool_call_id.to_string();
        Box::pin(async move {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Some(decision) = self.decisions.lock().unwrap().get(&call).cloned() {
                    return decision;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "manual gate decision never arrived for {call}"
                );
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
        })
    }
}

// ── harness ────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t08-{tag}-{}-{}",
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

async fn execute(state: &ServiceState, session: &str, input: &str) -> String {
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
            NOW_MS,
        )
        .await
        .expect("execute accepted")
        .run_id
}

async fn run_status(state: &ServiceState, run_id: &str) -> String {
    state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query")
        .expect("run row")
}

async fn run_reason(state: &ServiceState, run_id: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query reason")
}

async fn count_events(state: &ServiceState, run_id: &str, event_type: &str) -> i64 {
    state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = ?2",
            vec![run_id.to_string(), event_type.to_string()],
        )
        .await
        .expect("count query")
        .and_then(|v| v.parse().ok())
        .expect("numeric count")
}

async fn session_run_count(state: &ServiceState, session: &str) -> i64 {
    state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
            vec![session.to_string()],
        )
        .await
        .expect("count query")
        .and_then(|v| v.parse().ok())
        .expect("numeric count")
}

async fn final_message_of(state: &ServiceState, run_id: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(
            "SELECT content_json FROM messages WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("final message query")
}

async fn wait_until(what: &str, mut probe: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !probe() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

/// Polls the durable run row (the AUTHORITY) until it reaches `status`.
async fn wait_run_status(state: &ServiceState, run_id: &str, status: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let current = state
            .storage()
            .query_one_text(
                "SELECT status FROM runs WHERE run_id = ?1",
                vec![run_id.to_string()],
            )
            .await
            .expect("status query")
            .expect("run row");
        if current == status {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "run {run_id} never reached status {status} (last: {current})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}

fn deps(provider: Arc<ScriptedProvider>, tools: Arc<CountingTool>) -> ServiceDeps {
    ServiceDeps {
        turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
        tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
        ..ServiceDeps::default()
    }
}

// ── combo: normal ───────────────────────────────────────────────────────────

#[tokio::test]
async fn combo_normal_completes_with_final() {
    let provider = ScriptedProvider::new(vec![("NORM", vec![Step::Final("done")])]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "combo-normal",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let subscription = subscribe(&state).await;
    let run = execute(&state, "sess_local_alpha", "NORM: hello").await;

    assert_eq!(run_status(&state, &run).await, "completed");
    assert_eq!(
        run_reason(&state, &run).await.as_deref(),
        Some("completed.with_final")
    );
    assert!(final_message_of(&state, &run).await.is_some());
    record_case("combo-normal-run-completed", 1, 1);
    record_case(
        "combo-events-subscriber-saw-terminal",
        1,
        subscriber_saw_terminal(&subscription).await as i64,
    );
    record_combo_counts(
        "normal",
        serde_json::json!({
            "runs": 1,
            "model_calls": provider.calls_of("NORM"),
            "tool_calls": tools.calls(),
            "terminal_events": count_events(&state, &run, "run_state_changed").await,
            "final_status": "completed",
        }),
    );
    teardown(&state, &home).await;
}

async fn subscribe(state: &ServiceState) -> lingxi_service::SubscriptionGuard {
    match state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("subscribe")
    {
        SubscribeOutcome::Started { subscription, .. } => subscription,
        SubscribeOutcome::RequiresSnapshot(_) => panic!("fresh stream needs no snapshot"),
    }
}

async fn subscriber_saw_terminal(subscription: &lingxi_service::SubscriptionGuard) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let mut saw = false;
        while let Some(frame) = subscription.mailbox().try_recv() {
            if let lingxi_service::SubscriptionFrame::Event(envelope) = frame {
                if let lingxi_protocol::EventPayload::Known(
                    lingxi_protocol::KnownEventPayload::RunStateChanged(ref payload),
                ) = envelope.payload
                {
                    if payload.to.is_terminal() {
                        saw = true;
                    }
                }
            }
        }
        if saw {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    false
}

// ── combo: multi-turn model (three calls, ONE terminal) ─────────────────────

#[tokio::test]
async fn combo_multi_turn_model_one_terminal() {
    let provider = ScriptedProvider::new(vec![(
        "MULTI",
        vec![
            Step::Turn(tool_requests(1)),
            Step::Turn(ProviderTurn::Continue {
                process_note: "thinking".to_string(),
            }),
            Step::Final("after three calls"),
        ],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "combo-multi-turn",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let run = execute(&state, "sess_local_alpha", "MULTI: go").await;

    assert_eq!(run_status(&state, &run).await, "completed");
    assert_eq!(count_events(&state, &run, "model_call_started").await, 3);
    assert_eq!(count_events(&state, &run, "model_call_completed").await, 3);
    assert_eq!(
        count_terminal_events(&state, &run).await,
        1,
        "model-call ends never finalize the task"
    );
    record_case("combo-multi-turn-one-terminal", 1, 1);
    record_combo_counts(
        "multi-turn-model",
        serde_json::json!({
            "runs": 1,
            "model_calls": provider.calls_of("MULTI"),
            "tool_calls": tools.calls(),
            "terminal_events": count_terminal_events(&state, &run).await,
            "final_status": "completed",
        }),
    );
    teardown(&state, &home).await;
}

async fn count_terminal_events(state: &ServiceState, run_id: &str) -> i64 {
    // A run_state_changed whose `to` is one of the four terminal wire
    // names (exactly what the single finalize path commits).
    state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'run_state_changed' \
             AND (payload_json LIKE '%\"to\":\"completed\"%' \
               OR payload_json LIKE '%\"to\":\"failed\"%' \
               OR payload_json LIKE '%\"to\":\"cancelled\"%' \
               OR payload_json LIKE '%\"to\":\"interrupted_needs_attention\"%')",
            vec![run_id.to_string()],
        )
        .await
        .expect("count")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

// ── combo: multi-tool ───────────────────────────────────────────────────────

#[tokio::test]
async fn combo_multi_tool_three_calls() {
    let provider = ScriptedProvider::new(vec![(
        "TOOLS",
        vec![Step::Turn(tool_requests(3)), Step::Final("tools done")],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "combo-multi-tool",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let run = execute(&state, "sess_local_alpha", "TOOLS: run three").await;

    assert_eq!(run_status(&state, &run).await, "completed");
    assert_eq!(count_events(&state, &run, "tool_call_started").await, 3);
    assert_eq!(count_events(&state, &run, "tool_call_completed").await, 3);
    record_case("combo-multi-tool-three-calls", 3, tools.calls() as i64);
    record_combo_counts(
        "multi-tool",
        serde_json::json!({
            "runs": 1,
            "model_calls": provider.calls_of("TOOLS"),
            "tool_calls": tools.calls(),
            "terminal_events": count_terminal_events(&state, &run).await,
            "final_status": "completed",
        }),
    );
    teardown(&state, &home).await;
}

// ── combo: timeout (turn budget exhausted → loud failure) ───────────────────

#[tokio::test]
async fn combo_timeout_budget_is_a_loud_failure() {
    let provider = ScriptedProvider::new(vec![(
        "LOOP",
        vec![
            Step::Turn(ProviderTurn::Continue {
                process_note: "1".to_string(),
            }),
            Step::Turn(ProviderTurn::Continue {
                process_note: "2".to_string(),
            }),
            Step::Turn(ProviderTurn::Continue {
                process_note: "3".to_string(),
            }),
        ],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "combo-timeout",
        ServiceDeps {
            run_limits: RunDriveLimits {
                max_model_turns: 3,
                max_attempts: 1,
            },
            ..deps(Arc::clone(&provider), Arc::clone(&tools))
        },
    )
    .await;

    let run = execute(&state, "sess_local_alpha", "LOOP: never ends").await;

    assert_eq!(run_status(&state, &run).await, "failed");
    assert_eq!(
        run_reason(&state, &run).await.as_deref(),
        Some("failed.turn_budget_exceeded"),
        "a budget limit is never silently completed"
    );
    assert!(final_message_of(&state, &run).await.is_none());
    record_case("combo-timeout-budget-loud-failure", 1, 1);
    record_combo_counts(
        "timeout-budget",
        serde_json::json!({
            "runs": 1,
            "model_calls": provider.calls_of("LOOP"),
            "tool_calls": tools.calls(),
            "terminal_events": count_terminal_events(&state, &run).await,
            "final_status": "failed",
            "final_reason": "failed.turn_budget_exceeded",
        }),
    );
    teardown(&state, &home).await;
}

// ── combo: cancel during stream read ────────────────────────────────────────

#[tokio::test]
async fn combo_cancel_during_stream_read() {
    let provider = ScriptedProvider::new(vec![("CXL", vec![Step::Final("never lands")])]);
    let gate = provider.gate("CXL", 1);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "combo-cancel-stream",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let state_for_task = state.clone();
    let task =
        tokio::spawn(
            async move { execute(&state_for_task, "sess_local_alpha", "CXL: park").await },
        );
    wait_until("model call 1 started", || provider.calls_of("CXL") >= 1).await;

    let storage = Arc::clone(state.storage());
    let run = current_run_of(&state, "sess_local_alpha").await;
    let outcome = state
        .sessions()
        .cancel_run_for(storage.as_ref(), state.runs(), &owner_principal(), &run)
        .await
        .expect("cancel resolves");
    assert!(matches!(outcome, CancelRunOutcome::Accepted { .. }));
    gate.add_permits(1); // the parked turn completes LATE (out-of-order form)
    let run_from_task = task.await.expect("cancelled run settles");
    assert_eq!(run_from_task, run);

    assert_eq!(run_status(&state, &run).await, "cancelled");
    assert!(final_message_of(&state, &run).await.is_none());
    let completed = count_events(&state, &run, "model_call_completed").await;
    assert_eq!(
        completed, 0,
        "the late turn never lands in the cancelled run's stream"
    );
    let audit = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM stale_result_audit WHERE run_id = ?1",
            vec![run.clone()],
        )
        .await
        .expect("audit count")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    // The out-of-order combo's pinned invariant: the late result NEVER
    // pollutes the stream (no completed model events, no final message).
    // The audit row itself is the port-level fence's contract, proven by
    // late_result_fence*; here it is recorded as an observation only.
    record_case("combo-cancel-stream-terminal-cancelled", 1, 1);
    record_case("combo-out-of-order-late-result-fenced", 1, 1);
    record_case("cancel-live-run-accepted-terminal-cancelled", 1, 1);
    record_combo_counts(
        "cancel-stream-read",
        serde_json::json!({
            "runs": 1,
            "model_calls": provider.calls_of("CXL"),
            "tool_calls": tools.calls(),
            "terminal_events": count_terminal_events(&state, &run).await,
            "final_status": "cancelled",
            "model_call_completed_after_cancel": completed,
            "late_result_audit_rows": audit,
        }),
    );
    teardown(&state, &home).await;
}

async fn wait_for_first_run(state: &ServiceState, session: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(run) = state
            .storage()
            .query_one_text(
                "SELECT run_id FROM runs WHERE session_id = ?1 ORDER BY rowid DESC LIMIT 1",
                vec![session.to_string()],
            )
            .await
            .expect("run query")
        {
            return run;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no run row ever appeared for {session}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}

async fn current_run_of(state: &ServiceState, session: &str) -> String {
    state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE session_id = ?1 ORDER BY rowid DESC LIMIT 1",
            vec![session.to_string()],
        )
        .await
        .expect("run query")
        .expect("at least one run row")
}

// ── combo: cancel during approval wait ──────────────────────────────────────

#[tokio::test]
async fn combo_cancel_during_approval_wait() {
    let provider = ScriptedProvider::new(vec![(
        "APR",
        vec![Step::Turn(tool_requests(1)), Step::Final("after approval")],
    )]);
    let tools = CountingTool::new();
    let gate = ManualGate::new();
    let (state, home) = boot(
        "combo-cancel-approval",
        ServiceDeps {
            approval_gate: Some(gate as Arc<dyn approval::ApprovalGate>),
            ..deps(Arc::clone(&provider), Arc::clone(&tools))
        },
    )
    .await;

    let state_for_task = state.clone();
    let task = tokio::spawn(async move {
        execute(&state_for_task, "sess_local_alpha", "APR: needs approval").await
    });
    let run = wait_for_first_run(&state, "sess_local_alpha").await;
    // The run is DURABLY in waiting_approval before the gate parks
    // (poll the authority — the run row itself).
    wait_run_status(&state, &run, "waiting_approval").await;

    let storage = Arc::clone(state.storage());
    let outcome = state
        .sessions()
        .cancel_run_for(storage.as_ref(), state.runs(), &owner_principal(), &run)
        .await
        .expect("cancel resolves");
    assert!(matches!(outcome, CancelRunOutcome::Accepted { .. }));
    task.await.expect("cancelled run settles");

    assert_eq!(run_status(&state, &run).await, "cancelled");
    assert_eq!(
        tools.calls(),
        0,
        "cancellation during approval wait executes ZERO tools"
    );
    record_case("cancel-waiting-approval-cancels-cleanly", 1, 1);
    record_combo_counts(
        "cancel-approval-wait",
        serde_json::json!({
            "runs": 1,
            "model_calls": provider.calls_of("APR"),
            "tool_calls": tools.calls(),
            "terminal_events": count_terminal_events(&state, &run).await,
            "final_status": "cancelled",
        }),
    );
    teardown(&state, &home).await;
}

// ── combo: duplicate submissions ────────────────────────────────────────────

#[tokio::test]
async fn combo_duplicate_request_id_replays_and_conflicts() {
    let provider = ScriptedProvider::new(vec![("DUP", vec![Step::Final("once")])]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "combo-duplicate",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let storage = Arc::clone(state.storage());
    let first = state
        .sessions()
        .execute_submission_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            &ExecuteSubmission {
                input: "DUP: idempotent",
                request_id: Some("req-dup-1"),
            },
            NOW_MS,
        )
        .await
        .expect("first submission accepted");

    let replay = state
        .sessions()
        .execute_submission_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            &ExecuteSubmission {
                input: "DUP: idempotent",
                request_id: Some("req-dup-1"),
            },
            NOW_MS + 1,
        )
        .await
        .expect("identical duplicate replays");
    assert!(replay.replayed, "the duplicate is an idempotent replay");
    assert_eq!(replay.run_id, first.run_id);
    assert_eq!(session_run_count(&state, "sess_local_alpha").await, 1);

    let conflict = state
        .sessions()
        .execute_submission_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            &ExecuteSubmission {
                input: "DUP: CHANGED CONTENT under the same id",
                request_id: Some("req-dup-1"),
            },
            NOW_MS + 2,
        )
        .await
        .expect_err("same id + changed content conflicts");
    assert!(matches!(
        conflict,
        lingxi_service::SessionExecuteError::DuplicateRequestConflict { .. }
    ));
    assert_eq!(
        session_run_count(&state, "sess_local_alpha").await,
        1,
        "the conflict neither reused nor started an execution"
    );
    record_case("combo-duplicate-replay-no-second-run", 1, 1);
    record_case("combo-duplicate-conflict-rejected", 1, 1);
    record_combo_counts(
        "duplicate-submission",
        serde_json::json!({
            "runs": session_run_count(&state, "sess_local_alpha").await,
            "model_calls": provider.calls_of("DUP"),
            "tool_calls": tools.calls(),
            "terminal_events": count_terminal_events(&state, &first.run_id).await,
            "final_status": "completed",
            "idempotent_replays": 1,
            "diagnosed_conflicts": 1,
        }),
    );
    teardown(&state, &home).await;
}

// ── combo: cross-session parallel ───────────────────────────────────────────

#[tokio::test]
async fn combo_cross_session_parallel_both_complete() {
    let provider = ScriptedProvider::new(vec![
        ("ALPHA", vec![Step::Final("alpha done")]),
        ("BETA", vec![Step::Final("beta done")]),
    ]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "combo-cross-session",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let alpha_state = state.clone();
    let beta_state = state.clone();
    let (alpha, beta) = tokio::join!(
        async move { execute(&alpha_state, "sess_local_alpha", "ALPHA: mine").await },
        async move { execute(&beta_state, "sess_local_beta", "BETA: mine").await },
    );
    assert_eq!(run_status(&state, &alpha).await, "completed");
    assert_eq!(run_status(&state, &beta).await, "completed");
    assert_eq!(session_run_count(&state, "sess_local_alpha").await, 1);
    assert_eq!(session_run_count(&state, "sess_local_beta").await, 1);
    record_case("combo-cross-session-both-complete", 2, 2);
    record_combo_counts(
        "cross-session-parallel",
        serde_json::json!({
            "runs": session_run_count(&state, "sess_local_alpha").await
                + session_run_count(&state, "sess_local_beta").await,
            "model_calls": provider.total_calls(),
            "tool_calls": tools.calls(),
            "final_status_alpha": "completed",
            "final_status_beta": "completed",
        }),
    );
    teardown(&state, &home).await;
}

// ── combo: crash recovery (drive lost mid-run → real startup scan) ─────────

#[tokio::test]
async fn combo_crash_recovery_is_honestly_interrupted() {
    // Phase 1: a run parks mid-model-call; the driving future is LOST
    // (process-loss boundary; the binary-level SIGKILL form is covered by
    // recovery_crash_points.rs — this matrix combo exercises the REAL
    // restart scan on the same durable facts).
    let provider = ScriptedProvider::new(vec![("CRASH", vec![Step::Final("never returned")])]);
    let _gate = provider.gate("CRASH", 1);
    let tools = CountingTool::new();
    let home = synthetic_home("combo-crash");
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap_with_deps(
        config_for(&home),
        &layout,
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await
    .expect("bootstrap");

    let state_for_task = state.clone();
    let task = tokio::spawn(async move {
        execute(&state_for_task, "sess_local_alpha", "CRASH: lose me").await
    });
    wait_until("model call 1 started", || provider.calls_of("CRASH") >= 1).await;
    let run = current_run_of(&state, "sess_local_alpha").await;
    task.abort(); // the driver disappears WITHOUT finalize (process lost)
                  // Storage closes WITHOUT a graceful shutdown: the active run row and
                  // its journal facts stay exactly as a crashed process left them.
    state
        .storage()
        .close()
        .await
        .expect("close lost-process storage");

    // Phase 2: a fresh production-form bootstrap runs the REAL startup
    // recovery scan over the surviving durable facts.
    let layout2 = prepare_layout(&home).expect("layout again");
    let state2 =
        ServiceState::bootstrap_with_deps(config_for(&home), &layout2, ServiceDeps::default())
            .await
            .expect("restart bootstrap with recovery scan");

    assert_eq!(
        run_status(&state2, &run).await,
        "interrupted_needs_attention",
        "a lost active run is honestly interrupted, never blank and never fake success"
    );
    assert!(
        final_message_of(&state2, &run).await.is_none(),
        "no model final reply is ever fabricated by recovery"
    );
    let report = state2.recovery_report().expect("scan report recorded");
    assert!(report.scanned >= 1);
    record_case("combo-crash-recovery-honest-interrupted", 1, 1);
    record_combo_counts(
        "crash-recovery",
        serde_json::json!({
            "runs_scanned": report.scanned,
            "model_calls": provider.calls_of("CRASH"),
            "tool_calls": tools.calls(),
            "final_status": "interrupted_needs_attention",
            "final_message_rows": 0,
            "form": "drive-lost + real startup scan (SIGKILL form: recovery_crash_points.rs)",
        }),
    );
    teardown(&state2, &home).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Leaf-matrix scenarios (the R03 stage map's supplemental-leaf case
// producer): each scenario drives the REAL chain and records the pinned
// case values. Doubles still only produce external responses.
// ═══════════════════════════════════════════════════════════════════════════

// ── steering semantics (frozen incumbent behavior) ──────────────────────────

#[tokio::test]
async fn leaf_steer_semantics_busy_miss_and_drain() {
    let provider = ScriptedProvider::new(vec![(
        "STEER",
        vec![
            Step::Turn(ProviderTurn::Continue {
                process_note: "first turn".to_string(),
            }),
            Step::Final("steered answer"),
        ],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "leaf-steer",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;
    let gate = provider.gate("STEER", 1);

    let state_for_task = state.clone();
    let task =
        tokio::spawn(
            async move { execute(&state_for_task, "sess_local_alpha", "STEER: base").await },
        );
    wait_until("first turn started", || provider.calls_of("STEER") >= 1).await;

    // Busy session: the steering text is accepted into the bounded inbox.
    let busy = state
        .sessions()
        .steer_for(
            &owner_principal(),
            "sess_local_alpha",
            "consider this instead",
        )
        .await
        .expect("steer resolves");
    assert_eq!(busy, SteerOutcome::Accepted);
    record_case("steer-busy-accepted", 1, 1);

    gate.add_permits(1);
    let run = task.await.expect("steered run settles");

    // The steering text was drained into the NEXT model call's input.
    let inputs = provider.inputs_of("STEER");
    assert!(
        inputs.iter().any(|input| input.contains("[steering]")),
        "the drained steering text must reach the next model input: {inputs:?}"
    );
    record_case("steer-text-drained-into-model-input", 1, 1);
    assert_eq!(run_status(&state, &run).await, "completed");

    // Idle session: steering misses — the frozen fallback is a normal
    // submission by the CALLER (the service honestly reports the miss).
    let idle = state
        .sessions()
        .steer_for(&owner_principal(), "sess_local_alpha", "no live stream")
        .await
        .expect("steer resolves");
    assert_eq!(idle, SteerOutcome::Miss);
    record_case("steer-idle-miss", 1, 1);
    record_combo_counts(
        "leaf-steer",
        serde_json::json!({
            "runs": session_run_count(&state, "sess_local_alpha").await,
            "model_calls": provider.calls_of("STEER"),
            "tool_calls": tools.calls(),
            "final_status": "completed",
        }),
    );
    teardown(&state, &home).await;
}

// ── cancel outcome shapes (aborted / already stopped / not found) ────────────

#[tokio::test]
async fn leaf_cancel_outcome_shapes() {
    let provider = ScriptedProvider::new(vec![("CXL2", vec![Step::Final("done")])]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "leaf-cancel-outcomes",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let run = execute(&state, "sess_local_alpha", "CXL2: completes").await;
    assert_eq!(run_status(&state, &run).await, "completed");

    let storage = Arc::clone(state.storage());
    let terminal = state
        .sessions()
        .cancel_run_for(storage.as_ref(), state.runs(), &owner_principal(), &run)
        .await
        .expect("cancel of a settled run resolves");
    assert!(matches!(terminal, CancelRunOutcome::AlreadyTerminal { .. }));
    record_case("cancel-terminal-run-already-terminal", 1, 1);
    assert_eq!(run_status(&state, &run).await, "completed");

    let missing = state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &owner_principal(),
            "run_does_not_exist",
        )
        .await
        .expect_err("unknown run id");
    assert!(matches!(
        missing,
        lingxi_service::SessionExecuteError::NotFound
    ));
    record_case("cancel-unknown-run-not-found", 1, 1);
    teardown(&state, &home).await;
}

// ── subagent family: dispatch → reply → close with lineage ──────────────────

#[tokio::test]
async fn leaf_subagent_dispatch_reply_close() {
    let provider = ScriptedProvider::new(vec![
        (
            "LIFE",
            vec![
                Step::Dispatch {
                    task: "CHILD-A: research the fence",
                    access_write: false,
                },
                Step::Reply {
                    task: "CHILD-B: continue the research",
                },
                Step::Close {
                    reason: "wrapped up",
                },
                Step::Final("parent done"),
            ],
        ),
        ("CHILD-A", vec![Step::Final("child A done")]),
        ("CHILD-B", vec![Step::Final("child B done")]),
    ]);
    // Deterministic ordering anchors (the subagent_lifecycle precedent):
    // the parent's reply turn waits for child A to settle; the close turn
    // waits for the continuation child to settle.
    let reply_gate = provider.gate("LIFE", 2);
    let close_gate = provider.gate("LIFE", 3);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "leaf-subagent-family",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let state_for_task = state.clone();
    let task = tokio::spawn(async move {
        execute(&state_for_task, "sess_local_alpha", "LIFE: delegate").await
    });

    // Child A settles on its own thread.
    wait_until("child A thread settles", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| !thread.busy && thread.last_run_status.is_some())
    })
    .await;
    let snapshot = state
        .subagents()
        .threads_of("sess_local_alpha")
        .into_iter()
        .next()
        .expect("the thread exists");
    let child_a = snapshot.child_run_id.expect("the thread names its run");
    assert_eq!(
        run_status(&state, &child_a).await,
        "completed",
        "the dispatched child completes its own task"
    );
    let lineage = state
        .sessions()
        .run_lineage_for(state.storage().as_ref(), &owner_principal(), &child_a)
        .await
        .expect("lineage query")
        .expect("dispatch lineage exists");
    let parent_run = lineage
        .parent_run_id
        .map(|p| p.to_string())
        .expect("parentRunId recorded");
    assert_eq!(lineage.origin.wire_name(), "subagent");
    record_case("subagent-dispatch-child-lineage-recorded", 1, 1);

    provider.set_thread(snapshot.thread_id.clone());
    reply_gate.add_permits(1);
    wait_until("continuation child settles", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| {
                thread
                    .child_run_id
                    .as_deref()
                    .is_some_and(|id| id != child_a)
            })
    })
    .await;
    record_case("subagent-reply-continues-thread", 1, 1);
    close_gate.add_permits(1);

    let parent = task.await.expect("parent settles");
    assert_eq!(
        parent_run, parent,
        "the lineage's parent is the driving run"
    );

    let closed = state
        .subagents()
        .threads_of("sess_local_alpha")
        .into_iter()
        .next()
        .expect("the thread exists");
    assert_eq!(closed.status.wire_name(), "closed");
    record_case("subagent-close-thread-closed-status", 1, 1);
    teardown(&state, &home).await;
}

// ── parent cancel stops the linked child ────────────────────────────────────

#[tokio::test]
async fn leaf_parent_cancel_stops_child() {
    let provider = ScriptedProvider::new(vec![
        (
            "LIFE",
            vec![
                Step::Dispatch {
                    task: "LONGCHILD: long child task",
                    access_write: false,
                },
                Step::Final("parent after child"),
            ],
        ),
        (
            "LONGCHILD",
            vec![
                Step::Turn(ProviderTurn::Continue {
                    process_note: "child working".to_string(),
                }),
                Step::Final("child late"),
            ],
        ),
    ]);
    let parent_gate = provider.gate("LIFE", 2); // the parent parks after dispatch
    let child_gate = provider.gate("LONGCHILD", 1);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "leaf-parent-cancel",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let state_for_task = state.clone();
    let task = tokio::spawn(async move {
        execute(&state_for_task, "sess_local_alpha", "LIFE: cancel me").await
    });
    wait_until("child started", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| thread.busy)
    })
    .await;
    let child = current_child_of(&state).await;
    let parent = current_parent_of(&state, &child).await;

    let storage = Arc::clone(state.storage());
    let outcome = state
        .sessions()
        .cancel_run_for(storage.as_ref(), state.runs(), &owner_principal(), &parent)
        .await
        .expect("parent cancel resolves");
    assert!(matches!(outcome, CancelRunOutcome::Accepted { .. }));
    parent_gate.add_permits(1); // the parent's parked turn completes LATE too
    task.await.expect("parent settles cancelled");
    child_gate.add_permits(1); // the child's parked turn completes LATE

    assert_eq!(run_status(&state, &parent).await, "cancelled");

    // The child TASK stopped with the parent (the supervision tree
    // aborted it) — and since R03 repair G01/F02 its DURABLE row settles
    // `cancelled` IN THIS PROCESS: the linked wrapper opens a bounded
    // cooperative window so the child's own drive walks its four-phase
    // cancellation and its single finalize (no restart, no startup scan
    // needed for an ordinary parent cancellation).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let observed = run_status(&state, &child).await;
        if observed == "cancelled" {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the tree-cancelled child must settle cancelled in-process (last: {observed})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(final_message_of(&state, &child).await.is_none());
    // The thread registry's busy flag cleared and the concurrency lanes
    // returned to baseline in the same process.
    wait_until("the child's thread frees", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .all(|thread| !thread.busy)
    })
    .await;
    let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
    assert_eq!((per_session, global), (0, 0));
    // The child TASK stopped: the parent's bounded cleanup confirmed all
    // of its supervised children (the A06 supervision signal).
    let live_children = state.runs().task_supervisor().live_children_of(&parent);
    assert!(
        live_children.is_empty(),
        "all supervised children of the cancelled parent stopped: {live_children:?}"
    );
    record_case("parent-cancel-stops-child-run", 1, 1);

    // A fresh bootstrap's REAL recovery scan must NOT touch the already
    // terminal row (终态不复活): the same-process closeout made the
    // restart path a pure observer of an existing terminal.
    state.storage().close().await.expect("close storage");
    let layout2 = prepare_layout(&home).expect("layout again");
    let state2 =
        ServiceState::bootstrap_with_deps(config_for(&home), &layout2, ServiceDeps::default())
            .await
            .expect("restart bootstrap with recovery scan");
    let child_after = run_status(&state2, &child).await;
    assert_eq!(
        child_after, "cancelled",
        "the in-process cancelled child row is respected by the recovery scan \
            (observed before restart: cancelled)"
    );
    assert!(final_message_of(&state2, &child).await.is_none());
    record_combo_counts(
        "leaf-parent-cancel",
        serde_json::json!({
            "parent_final_status": "cancelled",
            "child_status_before_restart": "cancelled",
            "child_status_after_recovery_scan": child_after,
            "child_final_message_rows": 0,
            "note": "child run closed out IN-PROCESS by the cooperative-cancel window \
                (durable cancelled + busy cleared + lanes returned); the restart scan only \
                observes the existing terminal — ordinary cancellation never needs a restart",
        }),
    );
    teardown(&state2, &home).await;
}

async fn current_child_of(state: &ServiceState) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(child) = state
            .storage()
            .query_one_text(
                "SELECT run_id FROM run_lineage WHERE origin = 'subagent' ORDER BY rowid DESC LIMIT 1",
                vec![],
            )
            .await
            .expect("lineage query")
        {
            return child;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no subagent child lineage row ever appeared"
        );
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}

async fn current_parent_of(state: &ServiceState, child: &str) -> String {
    state
        .storage()
        .query_one_text(
            "SELECT parent_run_id FROM run_lineage WHERE run_id = ?1",
            vec![child.to_string()],
        )
        .await
        .expect("parent query")
        .expect("parent recorded")
}

// ── cancelling a child directly leaves the parent in charge ─────────────────

#[tokio::test]
async fn leaf_child_cancel_stops_only_the_child() {
    let provider = ScriptedProvider::new(vec![
        (
            "LIFE",
            vec![
                Step::Dispatch {
                    task: "STOPME: child to stop",
                    access_write: false,
                },
                Step::Final("parent continues"),
            ],
        ),
        (
            "STOPME",
            vec![
                Step::Turn(ProviderTurn::Continue {
                    process_note: "working".to_string(),
                }),
                Step::Final("child late"),
            ],
        ),
    ]);
    let parent_gate = provider.gate("LIFE", 2); // the parent stays live
    let child_gate = provider.gate("STOPME", 1);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "leaf-child-cancel",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let state_for_task = state.clone();
    let task = tokio::spawn(async move {
        execute(
            &state_for_task,
            "sess_local_alpha",
            "LIFE: dispatch then final",
        )
        .await
    });
    wait_until("child busy", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| thread.busy)
    })
    .await;
    let child = current_child_of(&state).await;

    let storage = Arc::clone(state.storage());
    let outcome = state
        .sessions()
        .cancel_run_for(storage.as_ref(), state.runs(), &owner_principal(), &child)
        .await
        .expect("child cancel resolves");
    assert!(matches!(outcome, CancelRunOutcome::Accepted { .. }));
    child_gate.add_permits(1);
    parent_gate.add_permits(1);
    let parent = task.await.expect("parent still settles");

    assert_eq!(run_status(&state, &child).await, "cancelled");
    assert_eq!(
        run_status(&state, &parent).await,
        "completed",
        "stopping the child does not kill the parent task"
    );
    record_case("subagent-child-cancel-stops-child-only", 1, 1);
    teardown(&state, &home).await;
}

// ── escalation denied: a read-only parent cannot dispatch a write child ─────

#[tokio::test]
async fn leaf_escalation_denied_zero_children() {
    let provider = ScriptedProvider::new(vec![(
        "ESC",
        vec![
            Step::Dispatch {
                task: "try to write",
                access_write: true,
            },
            Step::Final("parent saw the refusal"),
        ],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "leaf-escalation",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    // The default subagent policy: proactive delegation OFF (the frozen
    // experiment default) — pinned as a case before the escalation probe.
    let default_off = !ServiceDeps::default().subagent_policy.proactive_delegation;
    record_case(
        "subagent-policy-proactive-delegation-default-off",
        1,
        default_off as i64,
    );

    state
        .sessions()
        .set_permission_mode_for(
            &owner_principal(),
            "sess_local_alpha",
            lingxi_kernel::subagent::SessionPermissionMode::ReadOnly,
        )
        .await
        .expect("set read-only mode");

    let run = execute(&state, "sess_local_alpha", "ESC: escalate").await;
    assert_eq!(run_status(&state, &run).await, "completed");

    let child_runs = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM run_lineage WHERE origin = 'subagent'",
            vec![],
        )
        .await
        .expect("count children")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(-1);
    record_case(
        "subagent-escalation-write-denied-zero-children",
        0,
        child_runs,
    );
    assert_eq!(
        session_run_count(&state, "sess_local_alpha").await,
        1,
        "the denied escalation created ZERO child runs"
    );
    teardown(&state, &home).await;
}

// ── background runs: decoupled completion + survival of unrelated cancels ───

#[tokio::test]
async fn leaf_background_completes_decoupled_and_survives_cancels() {
    let provider = ScriptedProvider::new(vec![
        ("BG", vec![Step::Final("background done")]),
        (
            "FG",
            vec![
                Step::Turn(ProviderTurn::Continue {
                    process_note: "parked".to_string(),
                }),
                Step::Final("fg late"),
            ],
        ),
    ]);
    let fg_gate = provider.gate("FG", 1);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "leaf-background",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    // A foreground run on alpha parks mid-turn.
    let state_for_task = state.clone();
    let fg = tokio::spawn(async move {
        execute(&state_for_task, "sess_local_alpha", "FG: will be cancelled").await
    });
    wait_until("foreground parked", || provider.calls_of("FG") >= 1).await;

    // A background run on beta (same supervisor, detached drive).
    let storage = Arc::clone(state.storage());
    let events = Arc::clone(state.events());
    let runs = Arc::clone(state.runs());
    let background = Arc::clone(state.background());
    let accepted = state
        .sessions()
        .execute_background_for(
            &storage,
            &events,
            &runs,
            &background,
            &owner_principal(),
            "sess_local_beta",
            &ExecuteSubmission {
                input: "BG: decoupled",
                request_id: None,
            },
            NOW_MS,
        )
        .await
        .expect("background submission accepted");
    let bg_run = accepted.run_id.clone();

    // Cancel the foreground run; the background drive is NOT in its tree.
    let fg_run = current_run_of(&state, "sess_local_alpha").await;
    let storage2 = Arc::clone(state.storage());
    let outcome = state
        .sessions()
        .cancel_run_for(storage2.as_ref(), state.runs(), &owner_principal(), &fg_run)
        .await
        .expect("foreground cancel resolves");
    assert!(matches!(outcome, CancelRunOutcome::Accepted { .. }));
    fg.await.expect("foreground settles");
    fg_gate.add_permits(1);

    // The background run completes on its own (decoupled lifetime).
    wait_run_status(&state, &bg_run, "completed").await;
    let report = state
        .background()
        .drain_within(std::time::Duration::from_secs(5))
        .await;
    assert!(
        report.confirmed.contains(&bg_run),
        "the drained background drive confirmed its run"
    );
    record_case("background-run-completes-decoupled", 1, 1);
    record_case("background-survives-unrelated-cancel", 1, 1);
    assert_eq!(run_status(&state, &fg_run).await, "cancelled");
    teardown(&state, &home).await;
}

// ── reconnect resume: subscribe-only, never a model restart ─────────────────

#[tokio::test]
async fn leaf_reconnect_resume_never_restarts_the_model() {
    let provider = ScriptedProvider::new(vec![("RES", vec![Step::Final("resumable")])]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "leaf-reconnect",
        deps(Arc::clone(&provider), Arc::clone(&tools)),
    )
    .await;

    let (cut, first) = match state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("first subscribe")
    {
        SubscribeOutcome::Started { cut, subscription } => (cut, subscription),
        SubscribeOutcome::RequiresSnapshot(_) => panic!("fresh stream needs no snapshot"),
    };
    let run = execute(&state, "sess_local_alpha", "RES: one call").await;
    assert_eq!(run_status(&state, &run).await, "completed");
    let calls_before = provider.calls_of("RES");
    let runs_before = session_run_count(&state, "sess_local_alpha").await;
    assert!(calls_before >= 1);

    // "Reconnect": a NEW subscription with a valid cursor — the R02
    // snapshot/resume surface. It replays the missed events AFTER the
    // cursor and NEVER restarts the model.
    let cursor = lingxi_service::events::SubscribeCursor {
        stream_id: "sess_local_alpha".to_string(),
        seq: cut.from_seq,
    }
    .encode();
    let (resume_cut, reconnect) = match state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", Some(cursor))
        .await
        .expect("reconnect subscribe")
    {
        SubscribeOutcome::Started { cut, subscription } => (cut, subscription),
        SubscribeOutcome::RequiresSnapshot(_) => panic!("valid cursor resumes"),
    };
    // The resume CUT itself carries the replayed events (snapshot/resume
    // contract): the missed terminal must already be in it.
    assert!(
        resume_cut.events.iter().any(|envelope| {
            envelope.run_id.as_ref().map(|r| r.as_str()) == Some(run.as_str())
                && matches!(
                    envelope.payload,
                    lingxi_protocol::EventPayload::Known(
                        lingxi_protocol::KnownEventPayload::RunStateChanged(ref payload)
                    ) if payload.to.is_terminal()
                )
        }),
        "the resume cut replays the missed terminal event"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut replayed_terminal = resume_cut.events.iter().any(|envelope| {
        envelope.run_id.as_ref().map(|r| r.as_str()) == Some(run.as_str())
            && matches!(
                envelope.payload,
                lingxi_protocol::EventPayload::Known(
                    lingxi_protocol::KnownEventPayload::RunStateChanged(ref payload)
                ) if payload.to.is_terminal()
            )
    });
    while std::time::Instant::now() < deadline {
        while let Some(frame) = reconnect.mailbox().try_recv() {
            if let lingxi_service::SubscriptionFrame::Event(envelope) = frame {
                if envelope.run_id.as_ref().map(|r| r.as_str()) == Some(run.as_str())
                    && matches!(
                        envelope.payload,
                        lingxi_protocol::EventPayload::Known(
                            lingxi_protocol::KnownEventPayload::RunStateChanged(ref payload)
                        ) if payload.to.is_terminal()
                    )
                {
                    replayed_terminal = true;
                }
            }
        }
        if replayed_terminal {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(replayed_terminal, "the resumed stream replays the terminal");
    drop(first);
    assert_eq!(
        provider.calls_of("RES"),
        calls_before,
        "reconnect replays events ONLY — no model call is ever restarted"
    );
    assert_eq!(
        session_run_count(&state, "sess_local_alpha").await,
        runs_before
    );
    record_case("reconnect-resume-no-model-restart", 1, 1);
    teardown(&state, &home).await;
}
