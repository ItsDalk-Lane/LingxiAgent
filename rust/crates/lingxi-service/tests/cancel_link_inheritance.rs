//! R03 repair G01 / F01 regression: the cancellation TREE's child-run
//! LINKING and post-cancellation INHERITANCE (R03-FIX-F01 C01/C02/C03/
//! C04/C05).
//!
//! What these tests pin (the adversarial-audit counterexamples, executed
//! through the REAL components — real CancelRegistry/CancelScope/Task
//! wiring for the tree level, the REAL service composition for the
//! subagent-timeout consumption chain):
//! - C01: a run root linked under a parent (`run_root_under` /
//!   `register_linked` / `child` mixed, different depths) RECEIVES the
//!   parent's cancellation and its waiters wake in bounded time.
//! - C02: a node created AFTER the parent was already cancelled INHERITS
//!   the cancellation (first reason + moment) — no new external work may
//!   start under a cancelled subtree.
//! - C03: registration racing cancellation misses NO node (every
//!   successfully attributed node ends cancelled; the first reason of the
//!   root is never lost), including nodes joined after the traversal.
//! - C04: a REAL subagent child whose timeout fires while parked in a
//!   provider wait / an approval wait / a quota-queue wait settles
//!   `cancelled` in bounded time (the inner drive observes the timeout's
//!   scope cancellation through the linked tree).
//! - C05: isolation (an unrelated root keeps running; a child's
//!   cancellation never ascends) and first-reason stability under
//!   repeated and concurrent cancellations.
//!
//! Test-double boundary: the provider/tool/approval doubles below only
//! produce external responses at test-chosen moments; every cancellation
//! verdict, phase and durable state belongs to the real runtime.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_kernel::ports::{
    DelegationRequest, ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult,
    ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::{
    cancel, prepare_layout, CancelPolicy, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};

const NOW_MS: u64 = 1_790_409_600_000;

// ── doubles (external responses only) ────────────────────────────────────────

enum Step {
    Turn(ProviderTurn),
    Dispatch { task: &'static str },
}

struct ScriptedProvider {
    scripts: Mutex<HashMap<String, VecDeque<Step>>>,
    gates: HashMap<(String, usize), Arc<tokio::sync::Semaphore>>,
    pops: Mutex<HashMap<String, usize>>,
    /// How many times next_turn was ENTERED per marker (adapter call count).
    calls: Mutex<HashMap<String, usize>>,
}

impl ScriptedProvider {
    fn new(
        scripts: Vec<(&'static str, Vec<Step>)>,
        gates: Vec<((&'static str, usize), Arc<tokio::sync::Semaphore>)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scripts: Mutex::new(
                scripts
                    .into_iter()
                    .map(|(marker, script)| (marker.to_string(), script.into_iter().collect()))
                    .collect(),
            ),
            gates: gates
                .into_iter()
                .map(|((marker, pop), gate)| ((marker.to_string(), pop), gate))
                .collect(),
            pops: Mutex::new(HashMap::new()),
            calls: Mutex::new(HashMap::new()),
        })
    }

    fn marker_of(input: &str) -> String {
        input.split(':').next().unwrap_or(input).trim().to_string()
    }
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.scripted".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        _turn: u32,
        input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let marker = Self::marker_of(input);
        {
            let mut calls = self.calls.lock().unwrap();
            *calls.entry(marker.clone()).or_insert(0) += 1;
        }
        let pop = {
            let mut pops = self.pops.lock().unwrap();
            let next = pops.get(&marker).copied().unwrap_or(0) + 1;
            pops.insert(marker.clone(), next);
            next
        };
        let gate = self.gates.get(&(marker.clone(), pop)).cloned();
        let ctx_at_issue = ctx.clone();
        let step = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&marker)
            .and_then(|queue| queue.pop_front());
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await.expect("provider gate closed");
            }
            let turn = match step {
                Some(Step::Turn(turn)) => turn,
                Some(Step::Dispatch { task }) => ProviderTurn::ToolRequests {
                    requests: vec![ToolRequest {
                        target: "subagent".to_string(),
                        args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({
                            "task": task,
                        })),
                        args_summary: Some(format!("subagent: {task}")),
                        delegation: Some(DelegationRequest {
                            task: task.to_string(),
                            access: None,
                            label: None,
                            agent_id: None,
                            model: None,
                            thread_id: None,
                        }),
                    }],
                },
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

/// Tool double: parks on a 0-permit gate until released, recording every
/// arrival (external execution evidence).
struct ParkingTool {
    gate: Arc<tokio::sync::Semaphore>,
    arrivals: Mutex<Vec<String>>,
}

impl ToolExecutorPort for ParkingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.arrivals.lock().unwrap().push(ctx.run_id.to_string());
        let ctx_at_issue = ctx.clone();
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            let _permit = gate.acquire().await.expect("tool gate closed");
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::Success {
                    content_digest: "parked-then-success".to_string(),
                },
            )
        })
    }
}

/// Approval-gate double: a queue of PRELOADED decisions is returned in
/// order; every later request PARKS until decided. DROPPING a parked
/// request future removes the pending entry (late decisions refuse).
struct ParkingGate {
    pendings: Mutex<
        HashMap<String, tokio::sync::oneshot::Sender<lingxi_service::approval::ApprovalDecision>>,
    >,
    scripted: Mutex<VecDeque<lingxi_service::approval::ApprovalDecision>>,
}

impl ParkingGate {
    fn with_scripted(decisions: Vec<lingxi_service::approval::ApprovalDecision>) -> Arc<Self> {
        Arc::new(Self {
            pendings: Mutex::new(HashMap::new()),
            scripted: Mutex::new(decisions.into()),
        })
    }

    fn pending_count(&self) -> usize {
        self.pendings.lock().unwrap().len()
    }
}

impl lingxi_service::approval::ApprovalGate for ParkingGate {
    fn request<'a>(
        &'a self,
        _ctx: &'a RunContext,
        req: &'a lingxi_service::approval::ApprovalRequest,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = lingxi_service::approval::ApprovalDecision>
                + Send
                + 'a,
        >,
    > {
        let call_id = req.tool_call_id.to_string();
        struct Guard<'g>(&'g ParkingGate, String, bool);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                if self.2 {
                    self.0.pendings.lock().unwrap().remove(&self.1);
                }
            }
        }
        let scripted = self.scripted.lock().unwrap().pop_front();
        Box::pin(async move {
            if let Some(decision) = scripted {
                return decision;
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.pendings.lock().unwrap().insert(call_id.clone(), tx);
            let mut guard = Guard(self, call_id, true);
            match rx.await {
                Ok(decision) => {
                    guard.2 = false;
                    decision
                }
                Err(_dropped) => lingxi_service::approval::ApprovalDecision::Aborted,
            }
        })
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

fn read_tool_request() -> ToolRequest {
    ToolRequest {
        target: "read".to_string(),
        args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({"path": "/tmp/x"})),
        args_summary: Some("read /tmp/x".to_string()),
        delegation: None,
    }
}

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03g01-f01-{tag}-{}-{}",
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

#[allow(clippy::too_many_arguments)]
fn deps_for(
    provider: Arc<ScriptedProvider>,
    tools: Option<Arc<ParkingTool>>,
    approval: Option<Arc<ParkingGate>>,
    subagent_policy: lingxi_kernel::subagent::SubagentPolicy,
    cancel_policy: CancelPolicy,
    quota_limits: lingxi_service::QuotaLimits,
) -> ServiceDeps {
    ServiceDeps {
        turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
        tool_executor: tools.map(|tools| tools as Arc<dyn ToolExecutorPort>),
        approval_gate: approval.map(|gate| gate as Arc<dyn lingxi_service::approval::ApprovalGate>),
        subagent_policy,
        cancel_policy,
        quota_limits,
        ..ServiceDeps::default()
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

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
}

async fn run_status(state: &ServiceState, run_id: &str) -> Option<String> {
    query_text(state, "SELECT status FROM runs WHERE run_id = ?1", run_id).await
}

async fn latest_subagent_child_of(state: &ServiceState) -> Option<String> {
    state
        .storage()
        .query_one_text(
            "SELECT run_id FROM run_lineage WHERE origin = 'subagent' ORDER BY rowid DESC \
             LIMIT 1",
            vec![],
        )
        .await
        .expect("lineage query")
}

async fn wait_until(what: &str, mut probe: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !probe() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// Bounded wait for the latest subagent child run row (the lineage write
/// lands when the child's own drive first polls).
async fn wait_for_subagent_child(state: &ServiceState) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(child) = latest_subagent_child_of(state).await {
            return child;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the subagent child run row"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// Bounded wait for one run's DURABLE status to reach `expected` (the
/// storage queue is awaited per poll — a real read of the real database).
async fn wait_status(state: &ServiceState, run_id: &str, expected: &str, what: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if run_status(state, run_id).await.as_deref() == Some(expected) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what} (run {run_id} to become {expected})"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

fn thread_busy(state: &ServiceState) -> bool {
    state
        .subagents()
        .threads_of("sess_local_alpha")
        .iter()
        .any(|thread| thread.busy)
}

// ── F01-C01: linked run roots receive the parent's cancellation ─────────────

/// Mixed construction entries (`run_root_under`, `register_linked`,
/// `child`) at different depths: cancelling the PARENT root cancels every
/// node and every waiter wakes in bounded time; cancelling at a MIDDLE
/// depth cancels only that subtree (never ascending).
#[tokio::test]
async fn linked_run_roots_receive_parent_cancellation_across_mixed_entries() {
    let registry = cancel::CancelRegistry::new();

    // parent root (a real registry entry — the run's own root).
    let parent_entry = registry.register("run_parent");
    let parent_scope = parent_entry.scope.clone();

    // linked child run root, entry #1: through the REGISTRY (the
    // register_linked consumption chain drive_run uses).
    let child_entry = registry.register_linked("run_child", &parent_scope);

    // grandchild run root, entry #2: through run_root_under DIRECTLY under
    // the child's root.
    let grand_root = cancel::CancelScope::run_root_under("run_grand", &child_entry.scope);

    // a scope-level node under the grandchild (the model-call shape).
    let model_scope =
        grand_root.child("run_grand-mc0001".to_string(), cancel::ScopeKind::ModelCall);

    assert!(!child_entry.scope.is_cancelled());
    assert!(!grand_root.is_cancelled());
    assert!(!model_scope.is_cancelled());

    // Cancel the PARENT: every linked node must flip.
    assert!(parent_scope.cancel("user"));

    for (name, cancelled) in [
        ("child_entry", child_entry.scope.is_cancelled()),
        ("grand_root", grand_root.is_cancelled()),
        ("model_scope", model_scope.is_cancelled()),
    ] {
        assert!(cancelled, "cancelling the parent must reach {name}");
    }
    // Waiters wake in bounded time on every reached node.
    for (name, waiter) in [
        ("child", child_entry.scope.cancelled()),
        ("grand", grand_root.cancelled()),
        ("model", model_scope.cancelled()),
    ] {
        tokio::time::timeout(Duration::from_millis(250), waiter)
            .await
            .unwrap_or_else(|_| panic!("{name} waiter must wake"));
    }
    // The registry's own cancel surface agrees.
    assert!(registry.get("run_child").unwrap().scope.is_cancelled());

    // Middle-depth cancellation never ascends (a fresh tree).
    let registry2 = cancel::CancelRegistry::new();
    let p = registry2.register("run_p2");
    let c = registry2.register_linked("run_c2", &p.scope);
    let g = cancel::CancelScope::run_root_under("run_g2", &c.scope);
    c.scope.cancel("middle-level abort");
    assert!(c.scope.is_cancelled());
    assert!(
        g.is_cancelled(),
        "the middle cancel reaches its own subtree"
    );
    assert!(
        !p.scope.is_cancelled(),
        "a linked child's cancellation NEVER ascends to the parent run"
    );
}

// ── F01-C02: nodes created after the parent was cancelled inherit ────────────

/// `child`, `run_root_under` and `register_linked` created AFTER the
/// parent's cancellation INHERIT it: cancelled flag, first reason and the
/// parent's first cancellation MOMENT; the registry entry starts at the
/// `requested` phase (never Active-under-a-cancelled-scope); waiters
/// resolve immediately.
#[tokio::test]
async fn nodes_created_after_parent_cancellation_inherit_it() {
    let registry = cancel::CancelRegistry::new();
    let parent = registry.register("run_p3");
    assert!(parent.scope.cancel("user asked first"));
    let first_at = parent.scope.cancelled_at().expect("moment recorded");

    // scope-level child
    let late_child = parent
        .scope
        .child("run_p3-tc0001".to_string(), cancel::ScopeKind::ToolCall);
    assert!(
        late_child.is_cancelled(),
        "a child of an already-cancelled parent starts cancelled"
    );
    assert_eq!(late_child.reason().as_deref(), Some("user asked first"));
    assert_eq!(
        late_child.cancelled_at(),
        Some(first_at),
        "the inherited moment is the parent's FIRST cancellation moment"
    );

    // linked run root through run_root_under directly
    let late_root = cancel::CancelScope::run_root_under("run_late", &parent.scope);
    assert!(late_root.is_cancelled());
    assert_eq!(late_root.reason().as_deref(), Some("user asked first"));

    // linked run root through the registry (the drive_run path)
    let late_entry = registry.register_linked("run_late2", &parent.scope);
    assert!(late_entry.scope.is_cancelled());
    match late_entry.phase() {
        cancel::CancelPhase::Requested { reason } => {
            assert_eq!(reason, "user asked first");
        }
        other => panic!("an inherited cancellation starts the entry at Requested, got {other:?}"),
    }

    // A waiter created on the late node resolves immediately (bounded).
    tokio::time::timeout(Duration::from_millis(50), late_root.cancelled())
        .await
        .expect("late-node waiter resolves");
}

// ── F01-C03: registration racing cancellation misses no node ────────────────

/// Concurrent child registration vs cancellation (barrier-started): every
/// successfully attributed node — including ones joined AFTER the
/// traversal finished — ends cancelled; the root's first reason survives
/// every racer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registration_racing_cancellation_misses_no_node() {
    const RACERS: usize = 8;
    const LEVEL2_PER_RACER: usize = 2;

    let registry = cancel::CancelRegistry::new();
    let parent = registry.register("run_race");
    let barrier = Arc::new(std::sync::Barrier::new(RACERS + 1));

    let mut joins = Vec::new();
    for racer in 0..RACERS {
        let barrier = Arc::clone(&barrier);
        let scope = parent.scope.clone();
        joins.push(tokio::task::spawn_blocking(move || {
            barrier.wait();
            let mut created = Vec::new();
            for n in 0..LEVEL2_PER_RACER {
                let child = scope.child(format!("race-r{racer}-n{n}"), cancel::ScopeKind::ToolCall);
                // Multi-level nodes: grandchildren under the fresh child.
                let _grand = child.child(
                    format!("race-r{racer}-n{n}-g"),
                    cancel::ScopeKind::ModelCall,
                );
                created.push(child);
            }
            created
        }));
    }
    // The canceller starts with the racers (barrier-synchronized): the
    // traversal interleaves with the registrations arbitrarily.
    barrier.wait();
    assert!(parent.scope.cancel("first reason wins"));

    let mut all_cancelled = true;
    let mut total = 0usize;
    for join in joins {
        for child in join.await.expect("racer") {
            total += 1;
            all_cancelled &= child.is_cancelled();
        }
    }
    assert_eq!(total, RACERS * LEVEL2_PER_RACER);
    assert!(
        all_cancelled,
        "every node registered (before, during or after the traversal) is cancelled"
    );
    assert_eq!(
        parent.scope.reason().as_deref(),
        Some("first reason wins"),
        "the root's first reason survives the race"
    );

    // A node joined after everything settled still inherits.
    let late = parent
        .scope
        .child("race-late".to_string(), cancel::ScopeKind::ToolCall);
    assert!(late.is_cancelled());
}

// ── F01-C04: the real subagent timeout converges from each wait state ───────

/// Provider wait: the child parks mid-stream; ITS OWN timeout fires; the
/// linked tree must deliver that cancellation into the inner drive so it
/// settles `cancelled` in bounded time WITHOUT the test releasing the
/// park.
#[tokio::test]
async fn subagent_timeout_settles_a_parked_provider_wait() {
    let child_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(
        vec![
            (
                "P",
                vec![
                    Step::Dispatch {
                        task: "KID: park in the stream read",
                    },
                    Step::Turn(final_turn("parent done after dispatch")),
                ],
            ),
            (
                "KID",
                vec![
                    Step::Turn(ProviderTurn::Continue {
                        process_note: "child reading the stream".to_string(),
                    }),
                    Step::Turn(final_turn("child late")),
                ],
            ),
        ],
        vec![(("KID", 1), Arc::clone(&child_gate))],
    );
    let policy = lingxi_kernel::subagent::SubagentPolicy {
        timeout_ms: 250,
        ..lingxi_kernel::subagent::SubagentPolicy::default()
    };
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "tmo-provider",
        deps_for(
            Arc::clone(&provider),
            Some(Arc::clone(&tools)),
            None,
            policy,
            CancelPolicy::default(),
            lingxi_service::QuotaLimits::default(),
        ),
    )
    .await;

    let state_for_task = state.clone();
    let parent = tokio::spawn(async move {
        let storage = Arc::clone(state_for_task.storage());
        state_for_task
            .sessions()
            .execute_for(
                storage.as_ref(),
                state_for_task.events(),
                state_for_task.runs(),
                &owner_principal(),
                "sess_local_alpha",
                "P: dispatch",
                NOW_MS,
            )
            .await
            .expect("parent settles")
            .run_id
    });
    wait_until("the child is dispatched and busy", || thread_busy(&state)).await;
    let child = wait_for_subagent_child(&state).await;

    // The child's own timeout fires while its provider parks. The gate is
    // NEVER released in this test: only the timeout's scope cancellation
    // (delivered through the LINKED tree) can end the wait.
    wait_status(
        &state,
        &child,
        "cancelled",
        "the child settles cancelled through its timeout",
    )
    .await;
    let parent_run = parent.await.expect("parent task");
    // Full in-process closeout of the thread + concurrency lanes.
    wait_until("the thread frees", || !thread_busy(&state)).await;
    let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
    assert_eq!((per_session, global), (0, 0));
    // The parent itself completed normally (its own dispatch succeeded).
    assert_eq!(
        run_status(&state, &parent_run).await.as_deref(),
        Some("completed")
    );
    teardown(&state, &home).await;
}

/// Approval wait: the child parks at the approval gate; the timeout's
/// cancellation must exit the approval wait and settle the child
/// `cancelled` with ZERO tool executions.
#[tokio::test]
async fn subagent_timeout_settles_a_parked_approval_wait() {
    let provider = ScriptedProvider::new(
        vec![
            (
                "P",
                vec![
                    Step::Dispatch {
                        task: "KID: park at the approval gate",
                    },
                    Step::Turn(final_turn("parent done after dispatch")),
                ],
            ),
            (
                "KID",
                vec![Step::Turn(ProviderTurn::ToolRequests {
                    requests: vec![read_tool_request()],
                })],
            ),
        ],
        vec![],
    );
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    // The FIRST approval request is the parent's own delegation dispatch —
    // approve it; the CHILD's tool request then parks at the gate.
    let gate =
        ParkingGate::with_scripted(vec![lingxi_service::approval::ApprovalDecision::Approved]);
    let policy = lingxi_kernel::subagent::SubagentPolicy {
        timeout_ms: 250,
        ..lingxi_kernel::subagent::SubagentPolicy::default()
    };
    let (state, home) = boot(
        "tmo-approval",
        deps_for(
            Arc::clone(&provider),
            Some(Arc::clone(&tools)),
            Some(Arc::clone(&gate)),
            policy,
            CancelPolicy::default(),
            lingxi_service::QuotaLimits::default(),
        ),
    )
    .await;

    let state_for_task = state.clone();
    let parent = tokio::spawn(async move {
        let storage = Arc::clone(state_for_task.storage());
        state_for_task
            .sessions()
            .execute_for(
                storage.as_ref(),
                state_for_task.events(),
                state_for_task.runs(),
                &owner_principal(),
                "sess_local_alpha",
                "P: dispatch",
                NOW_MS,
            )
            .await
            .expect("parent settles")
            .run_id
    });
    wait_until("the child parks at the approval gate", || {
        gate.pending_count() == 1
    })
    .await;
    let child = latest_subagent_child_of(&state)
        .await
        .expect("the child run exists");
    assert_eq!(
        run_status(&state, &child).await.as_deref(),
        Some("waiting_approval"),
        "durable waiting_approval before the timeout"
    );

    wait_status(
        &state,
        &child,
        "cancelled",
        "the child settles cancelled through its timeout",
    )
    .await;
    let parent_run = parent.await.expect("parent task");
    let _ = parent_run;
    wait_until("the thread frees", || !thread_busy(&state)).await;
    assert_eq!(gate.pending_count(), 0, "the pending approval cleaned up");
    assert!(
        tools.arrivals.lock().unwrap().is_empty(),
        "ZERO tool executions through the approval-wait timeout"
    );
    let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
    assert_eq!((per_session, global), (0, 0));
    teardown(&state, &home).await;
}

/// Quota wait: the PARENT parks mid-stream on its SECOND turn holding the
/// only global model permit (its first turn already dispatched the child);
/// the child's first model call then parks in the bounded ADMISSION
/// QUEUE. The child's OWN timeout fires while it queues: the queue wait
/// must EXIT and the child settles `cancelled` with ZERO provider calls —
/// while the parked parent (unrelated to the child's timeout) keeps
/// holding its permit until released.
#[tokio::test(flavor = "current_thread")]
async fn subagent_timeout_settles_a_parked_quota_wait() {
    let parent_turn2_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(
        vec![
            (
                "P",
                vec![
                    Step::Dispatch {
                        task: "KID: queue for the model lane",
                    },
                    Step::Turn(ProviderTurn::Continue {
                        process_note: "parent parks holding the model lane".to_string(),
                    }),
                    Step::Turn(final_turn("parent finishes after release")),
                ],
            ),
            ("KID", vec![Step::Turn(final_turn("child never runs"))]),
        ],
        vec![(("P", 2), Arc::clone(&parent_turn2_gate))],
    );
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    let policy = lingxi_kernel::subagent::SubagentPolicy {
        timeout_ms: 400,
        ..lingxi_kernel::subagent::SubagentPolicy::default()
    };
    let quotas = lingxi_service::QuotaLimits {
        model: lingxi_service::LayeredQuotaLimits {
            global: 1,
            per_agent: 4,
            per_session: 4,
        },
        ..lingxi_service::QuotaLimits::default()
    };
    let (state, home) = boot(
        "tmo-quota",
        deps_for(
            Arc::clone(&provider),
            Some(Arc::clone(&tools)),
            None,
            policy,
            CancelPolicy::default(),
            quotas,
        ),
    )
    .await;

    let state_for_task = state.clone();
    let parent = tokio::spawn(async move {
        let storage = Arc::clone(state_for_task.storage());
        state_for_task
            .sessions()
            .execute_for(
                storage.as_ref(),
                state_for_task.events(),
                state_for_task.runs(),
                &owner_principal(),
                "sess_local_alpha",
                "P: dispatch",
                NOW_MS,
            )
            .await
            .expect("parent settles")
            .run_id
    });
    // Turn 1 dispatches the child; turn 2 parks HOLDING the only model
    // permit; the child's first model call parks in the admission queue.
    wait_until("the child queues behind the parked parent turn", || {
        state
            .runs()
            .quotas()
            .in_use(lingxi_service::QuotaResource::Model)
            == 1
            && state
                .runs()
                .quotas()
                .waiting(lingxi_service::QuotaResource::Model)
                == 1
    })
    .await;
    let child = latest_subagent_child_of(&state)
        .await
        .expect("the child run exists");

    // The child's timeout fires while it queues: the queue wait must EXIT
    // and the child settles cancelled — WITHOUT the parent being cancelled.
    wait_status(
        &state,
        &child,
        "cancelled",
        "the child settles cancelled through its timeout",
    )
    .await;
    wait_until("the thread frees", || !thread_busy(&state)).await;
    assert_eq!(
        state
            .runs()
            .quotas()
            .waiting(lingxi_service::QuotaResource::Model),
        0,
        "the child's queue place was returned"
    );
    assert_eq!(
        state
            .runs()
            .quotas()
            .in_use(lingxi_service::QuotaResource::Model),
        1,
        "the parked parent still holds its permit (a child timeout never ascends)"
    );
    let kid_events = query_text(
        &state,
        "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = \
'model_call_started'",
        &child,
    )
    .await
    .expect("child model events");
    assert_eq!(
        kid_events.as_str(),
        "0",
        "the queued child never called the provider"
    );
    let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
    assert_eq!((per_session, global), (0, 0));

    // The parked parent was untouched by the child's timeout; it completes
    // normally once released.
    parent_turn2_gate.add_permits(1);
    let parent_run = parent.await.expect("parent settles");
    assert_eq!(
        run_status(&state, &parent_run).await.as_deref(),
        Some("completed")
    );
    assert!(tools.arrivals.lock().unwrap().is_empty());
    teardown(&state, &home).await;
}

// ── F01-C05: isolation and repeated cancellation semantics ──────────────────

/// Two unrelated roots; one cancelled repeatedly (different reasons,
/// concurrent racers): the other root keeps running; a child's
/// cancellation never ascends; the first reason is never overwritten.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn isolation_and_first_reason_survive_repeated_and_concurrent_cancels() {
    let registry = cancel::CancelRegistry::new();
    let root_a = registry.register("run_iso_a");
    let root_b = registry.register("run_iso_b");

    // Repeated cancels with different reasons: only the FIRST wins.
    assert!(root_a.scope.cancel("first"));
    assert!(!root_a.scope.cancel("second"));
    assert!(!root_a.scope.cancel("third"));
    assert_eq!(root_a.scope.reason().as_deref(), Some("first"));

    // Concurrent different-reason cancellations linearize: the reason is
    // one of the racers' and stays stable afterwards.
    let scope = root_a.scope.clone();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut racers = Vec::new();
    for reason in ["race-r1", "race-r2", "race-r3"] {
        let scope = scope.clone();
        let barrier = Arc::clone(&barrier);
        racers.push(tokio::task::spawn_blocking(move || {
            barrier.wait();
            scope.cancel(reason)
        }));
    }
    for racer in racers {
        let _ = racer.await.expect("racer");
    }
    let reason = root_a.scope.reason().expect("a reason is set");
    assert!(
        ["first", "race-r1", "race-r2", "race-r3"].contains(&reason.as_str()),
        "linearized first reason: {reason}"
    );
    let frozen = reason.clone();
    root_a.scope.cancel("late overwrite attempt");
    assert_eq!(root_a.scope.reason(), Some(frozen), "the reason is frozen");

    // A cancelled CHILD of A never ascends; root B is untouched.
    let child_of_a = root_a
        .scope
        .child("iso-a-child".to_string(), cancel::ScopeKind::ToolCall);
    child_of_a.cancel("child-only abort");
    assert!(child_of_a.is_cancelled());
    assert!(
        !root_b.scope.is_cancelled(),
        "the unrelated root B keeps running"
    );
    assert_eq!(
        root_b.phase(),
        cancel::CancelPhase::Active,
        "root B never observed any cancellation"
    );
}
