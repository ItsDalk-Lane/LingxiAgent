//! R03 repair G01 / F02 regression: the subagent child-run CLOSEOUT —
//! concurrency lanes, thread busy state, durable terminal, supervision
//! registration and background registries must all return to baseline
//! IN THE SAME PROCESS for ordinary cancellations (R03-FIX-F02
//! C01/C02/C03/C04/C05).
//!
//! The acceptance rule under test (adversarial audit F02): a healthy
//! service's ordinary parent cancellation must settle the child's durable
//! row (`cancelled`), clear the thread's busy flag, return the
//! active-per-session/global counts AND clear the supervision entries —
//! WITHOUT a restart and WITHOUT the startup scan. Repeating beyond the
//! configured concurrency cap must still admit a final normal child.
//! Refusal windows (supervisor cap, thread registry cap, pre-provider
//! cancellation), abnormal endings (provider panic) and the drain-timeout
//! / background-panic supervision paths must all leave ZERO permanent
//! reservations or ghost Running entries.
//!
//! Test-double boundary: the provider/tool doubles below only produce
//! external responses (or controlled panics) at test-chosen moments;
//! every closeout decision, durable state and registry verdict belongs to
//! the real runtime (RunSupervisor, SubagentRuntime, TaskSupervisor,
//! BackgroundDriveRegistry, real SQLite storage).

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
    cancel, prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
    TaskExit,
};

const NOW_MS: u64 = 1_790_409_600_000;

// ── doubles (external responses only) ────────────────────────────────────────

enum Step {
    Turn(ProviderTurn),
    Dispatch { task: &'static str },
    Reply { task: &'static str },
    Panic(&'static str),
}

struct ScriptedProvider {
    scripts: Mutex<HashMap<String, VecDeque<Step>>>,
    gates: HashMap<(String, usize), Arc<tokio::sync::Semaphore>>,
    pops: Mutex<HashMap<String, usize>>,
    thread: Arc<Mutex<Option<String>>>,
}

impl ScriptedProvider {
    fn new(
        scripts: Vec<(String, Vec<Step>)>,
        gates: Vec<((String, usize), Arc<tokio::sync::Semaphore>)>,
        thread: Arc<Mutex<Option<String>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scripts: Mutex::new(
                scripts
                    .into_iter()
                    .map(|(marker, script)| (marker, script.into_iter().collect()))
                    .collect(),
            ),
            gates: gates.into_iter().collect(),
            pops: Mutex::new(HashMap::new()),
            thread,
        })
    }

    fn marker_of(input: &str) -> String {
        input.split(':').next().unwrap_or(input).trim().to_string()
    }

    fn set_thread(&self, thread: String) {
        *self.thread.lock().unwrap() = Some(thread);
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
        let thread = Arc::clone(&self.thread);
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await.expect("provider gate closed");
            }
            let turn = match step {
                Some(Step::Turn(turn)) => turn,
                Some(Step::Dispatch { task }) => delegation_turn("subagent", task, None),
                Some(Step::Reply { task }) => {
                    let thread_id = thread.lock().unwrap().clone();
                    delegation_turn("subagent_reply", task, thread_id)
                }
                Some(Step::Panic(message)) => panic!("{message}"),
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

fn delegation_turn(target: &'static str, task: &str, thread_id: Option<String>) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        requests: vec![ToolRequest {
            target: target.to_string(),
            args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({
                "task": task,
            })),
            args_summary: Some(format!("{target}: {task}")),
            delegation: Some(DelegationRequest {
                task: task.to_string(),
                access: None,
                label: None,
                agent_id: None,
                model: None,
                thread_id,
            }),
        }],
    }
}

/// Tool double: parks on a 0-permit gate until released, recording every
/// arrival (external execution evidence) per run.
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

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03g01-f02-{tag}-{}-{}",
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

/// Bounded wait for one run's DURABLE status to reach `expected` (each
/// poll awaits the REAL storage queue — a true read of the database).
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

/// Bounded wait for a subagent child run NEWER than every id in
/// `older_than` (successive runs on one thread are distinguished).
async fn wait_for_next_subagent_child(state: &ServiceState, older_than: &[&str]) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(child) = latest_subagent_child_of(state).await {
            if !older_than.iter().any(|old| *old == child) {
                return child;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the next subagent child run row"
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

fn spawn_execute(
    state: &ServiceState,
    session: &'static str,
    input: String,
) -> tokio::task::JoinHandle<String> {
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
                &input,
                NOW_MS,
            )
            .await
            .expect("run settles")
            .run_id
    })
}

async fn cancel_parent(state: &ServiceState, run_id: &str) {
    let storage = Arc::clone(state.storage());
    let outcome = state
        .sessions()
        .cancel_run_for(storage.as_ref(), state.runs(), &owner_principal(), run_id)
        .await
        .expect("cancel resolves");
    assert!(matches!(
        outcome,
        lingxi_service::CancelRunOutcome::Accepted { .. }
    ));
}

fn busy_thread_of(
    state: &ServiceState,
    session: &str,
) -> Option<lingxi_service::subagents::ThreadSnapshot> {
    state
        .subagents()
        .threads_of(session)
        .into_iter()
        .find(|thread| thread.busy)
}

// ── F02-C01: same-process repeated parent cancels, then a normal child ───────

/// Small per-session cap (2); THREE cancel rounds (beyond the cap), each
/// proving the full in-process closeout — durable `cancelled`, busy
/// cleared, counts back to baseline, supervision entries cleared — then a
/// final NORMAL child succeeds. No restart, no startup scan anywhere in
/// this test.
#[tokio::test]
async fn parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap() {
    let subagent_policy = lingxi_kernel::subagent::SubagentPolicy {
        per_session_limit: 2,
        global_limit: 4,
        timeout_ms: 30 * 60 * 1000,
        thread_registry_cap: 64,
        ..lingxi_kernel::subagent::SubagentPolicy::default()
    };
    let thread_slot = Arc::new(Mutex::new(None::<String>));
    // Three gated cancel rounds + one normal final round.
    let mut scripts: Vec<(String, Vec<Step>)> = Vec::new();
    let mut gates: Vec<((String, usize), Arc<tokio::sync::Semaphore>)> = Vec::new();
    for round in 1..=3 {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let parent_gate = Arc::new(tokio::sync::Semaphore::new(0));
        scripts.push((
            format!("P{round}"),
            vec![
                Step::Dispatch {
                    task: Box::leak(format!("KID{round}: park mid-provider").into_boxed_str()),
                },
                // The parent parks mid-stream AFTER the dispatch so it is
                // still running (cancellable) when the test acts.
                Step::Turn(ProviderTurn::Continue {
                    process_note: "parent parked mid-stream".to_string(),
                }),
            ],
        ));
        gates.push(((format!("P{round}"), 2usize), Arc::clone(&parent_gate)));
        scripts.push((
            format!("KID{round}"),
            vec![
                Step::Turn(ProviderTurn::Continue {
                    process_note: "child parked".to_string(),
                }),
                Step::Turn(final_turn("child late")),
            ],
        ));
        gates.push(((format!("KID{round}"), 1usize), Arc::clone(&gate)));
    }
    scripts.push((
        "PF".to_string(),
        vec![
            Step::Dispatch {
                task: "KIDF: finish quickly",
            },
            Step::Turn(final_turn("parent final done")),
        ],
    ));
    scripts.push((
        "KIDF".to_string(),
        vec![Step::Turn(final_turn("child final done"))],
    ));
    let provider = ScriptedProvider::new(scripts, gates, Arc::clone(&thread_slot));
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "c01-rounds",
        ServiceDeps {
            turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
            tool_executor: Some(Arc::clone(&tools) as Arc<dyn ToolExecutorPort>),
            subagent_policy,
            ..ServiceDeps::default()
        },
    )
    .await;

    for round in 1..=3 {
        let parent = spawn_execute(&state, "sess_local_alpha", format!("P{round}: dispatch"));
        wait_until("the child is dispatched and busy", || {
            busy_thread_of(&state, "sess_local_alpha").is_some()
        })
        .await;
        let child = wait_for_subagent_child(&state).await;
        let parent_run = query_text(
            &state,
            "SELECT run_id FROM runs WHERE session_id = ?1 AND run_id NOT IN (SELECT run_id \
             FROM run_lineage WHERE origin = 'subagent') ORDER BY rowid DESC LIMIT 1",
            "sess_local_alpha",
        )
        .await
        .expect("the parent run row exists");

        // Ordinary parent cancellation — same process, no restart.
        cancel_parent(&state, &parent_run).await;
        let settled = parent.await.expect("parent settles");
        assert_eq!(settled, parent_run);
        assert_eq!(
            run_status(&state, &parent_run).await.as_deref(),
            Some("cancelled"),
            "round {round}: the parent settles cancelled"
        );

        // THE F02 closeout: the child's durable row settles `cancelled` IN
        // THIS PROCESS (never left active for a startup scan).
        wait_status(
            &state,
            &child,
            "cancelled",
            &format!("round {round}: the child settles cancelled in-process"),
        )
        .await;
        // Busy cleared + last status recorded; concurrency lanes back to
        // baseline.
        wait_until("round {round}: the thread frees", || {
            busy_thread_of(&state, "sess_local_alpha").is_none()
        })
        .await;
        let snapshot = state
            .subagents()
            .threads_of("sess_local_alpha")
            .into_iter()
            .find(|thread| thread.child_run_id.as_deref() == Some(child.as_str()))
            .expect("the thread stays queryable");
        assert!(!snapshot.busy);
        assert_eq!(
            snapshot.last_run_status.as_deref(),
            Some("cancelled"),
            "round {round}: the thread records the child terminal"
        );
        let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
        assert_eq!(
            (per_session, global),
            (0, 0),
            "round {round}: concurrency lanes return to baseline"
        );
        // The cancelled parent's supervised children all confirmed.
        assert!(
            state
                .runs()
                .task_supervisor()
                .live_children_of(&parent_run)
                .is_empty(),
            "round {round}: no supervision ghost entries"
        );
    }

    // Beyond-cap proof: after THREE cancel rounds under a per-session cap
    // of 2, a fresh NORMAL child still dispatches and completes.
    let parent = spawn_execute(&state, "sess_local_alpha", "PF: final round".to_string());
    let parent_run = parent.await.expect("final parent settles");
    assert_eq!(
        run_status(&state, &parent_run).await.as_deref(),
        Some("completed")
    );
    let final_child = latest_subagent_child_of(&state).await.expect("final child");
    wait_status(
        &state,
        &final_child,
        "completed",
        "the final child completes",
    )
    .await;
    wait_until("the final thread frees", || {
        busy_thread_of(&state, "sess_local_alpha").is_none()
    })
    .await;
    let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
    assert_eq!((per_session, global), (0, 0));
    teardown(&state, &home).await;
}

// ── F02-C02: refusal windows and pre-external-action cancellation ────────────

/// Thread-registry cap refusal (the "登记线程" window): the refusal happens
/// before any reservation — zero busy residue, zero active rows, zero
/// child runs — and the FIRST thread keeps running untouched.
#[tokio::test]
async fn thread_registry_refusal_leaves_no_permanent_reservations() {
    let thread_slot = Arc::new(Mutex::new(None::<String>));
    let first_child_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(
        vec![
            (
                "P1".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID1: hold the only thread slot",
                    },
                    Step::Turn(final_turn("parent 1 done")),
                ],
            ),
            (
                "KID1".to_string(),
                vec![
                    Step::Turn(ProviderTurn::Continue {
                        process_note: "holds the slot".to_string(),
                    }),
                    Step::Turn(final_turn("late")),
                ],
            ),
            (
                "P2".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID2: refused by the registry cap",
                    },
                    Step::Turn(final_turn("parent 2 saw the refusal")),
                ],
            ),
        ],
        vec![(("KID1".to_string(), 1), Arc::clone(&first_child_gate))],
        Arc::clone(&thread_slot),
    );
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "c02-registry",
        ServiceDeps {
            turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
            tool_executor: Some(Arc::clone(&tools) as Arc<dyn ToolExecutorPort>),
            subagent_policy: lingxi_kernel::subagent::SubagentPolicy {
                thread_registry_cap: 1,
                ..lingxi_kernel::subagent::SubagentPolicy::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;

    // Session A: first dispatch creates the only thread (parks).
    let parent_a = spawn_execute(&state, "sess_local_alpha", "P1: dispatch".to_string());
    wait_until("the first child is busy", || {
        busy_thread_of(&state, "sess_local_alpha").is_some()
    })
    .await;
    // Session B: the registry is full of OPEN threads → ThreadRegistryFull.
    let parent_b = spawn_execute(&state, "sess_local_beta", "P2: dispatch".to_string());
    let parent_b_run = parent_b.await.expect("parent B settles with the refusal");
    assert_eq!(
        run_status(&state, &parent_b_run).await.as_deref(),
        Some("completed")
    );
    // Zero residue for the REFUSING session: no thread, no counts, no run.
    assert!(state.subagents().threads_of("sess_local_beta").is_empty());
    let (per_session, _global) = state.subagents().active_counts("sess_local_beta");
    assert_eq!(per_session, 0);
    let refused_children = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM run_lineage WHERE origin = 'subagent'",
            vec![],
        )
        .await
        .expect("lineage count");
    assert_eq!(
        refused_children.as_deref(),
        Some("1"),
        "exactly ONE subagent child exists (the refusing dispatch created zero)"
    );
    // The refusal is a recorded tool failure for parent B's model.
    let refusal = query_text(
        &state,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
         'tool_call_completed'",
        &parent_b_run,
    )
    .await
    .expect("the refused dispatch has a completed tool event");
    assert!(
        refusal.contains("registry"),
        "the registry-cap refusal reached the model: {refusal}"
    );
    // The FIRST thread keeps its busy state (its child still runs).
    assert!(busy_thread_of(&state, "sess_local_alpha").is_some());

    // Release the parked first child; the fixture tears down cleanly.
    first_child_gate.add_permits(1);
    let parent_a_run = parent_a.await.expect("parent A settles");
    assert_eq!(
        run_status(&state, &parent_a_run).await.as_deref(),
        Some("completed")
    );
    teardown(&state, &home).await;
}

/// Cancellation landing while the child is parked in the MODEL admission
/// queue (before its first provider call): the child settles `cancelled`
/// with ZERO provider calls for it — the pre-external-action window of an
/// inherited cancellation.
#[tokio::test(flavor = "current_thread")]
async fn parent_cancel_before_the_childs_first_provider_call_settles_with_zero_calls() {
    let thread_slot = Arc::new(Mutex::new(None::<String>));
    let parent_turn2_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(
        vec![
            (
                "P".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID: queued behind the parent's parked turn",
                    },
                    Step::Turn(ProviderTurn::Continue {
                        process_note: "parent parks holding the model lane".to_string(),
                    }),
                    Step::Turn(final_turn("parent would finish")),
                ],
            ),
            (
                "KID".to_string(),
                vec![Step::Turn(final_turn("child never runs"))],
            ),
        ],
        vec![(("P".to_string(), 2), Arc::clone(&parent_turn2_gate))],
        Arc::clone(&thread_slot),
    );
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "c02-preprovider",
        ServiceDeps {
            turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
            tool_executor: Some(Arc::clone(&tools) as Arc<dyn ToolExecutorPort>),
            quota_limits: lingxi_service::QuotaLimits {
                model: lingxi_service::LayeredQuotaLimits {
                    global: 1,
                    per_agent: 4,
                    per_session: 4,
                },
                ..lingxi_service::QuotaLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;

    let parent = spawn_execute(&state, "sess_local_alpha", "P: dispatch".to_string());
    // Turn 1 dispatches; turn 2 parks HOLDING the only model permit; the
    // child's first model call parks in the admission queue.
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
    let parent_run = query_text(
        &state,
        "SELECT run_id FROM runs WHERE session_id = ?1 AND run_id NOT IN (SELECT run_id \
         FROM run_lineage WHERE origin = 'subagent')",
        "sess_local_alpha",
    )
    .await
    .expect("parent row");

    // Cancel the parent: the queue wait must exit, the child must settle
    // `cancelled` WITHOUT its provider ever being called.
    cancel_parent(&state, &parent_run).await;
    let settled = parent.await.expect("parent settles cancelled");
    assert_eq!(settled, parent_run);
    wait_status(
        &state,
        &child,
        "cancelled",
        "the child settles cancelled before its first provider call",
    )
    .await;
    // The child's provider was NEVER entered (the scripted KID marker
    // never consumed a turn) — zero external model action for the child.
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
        "the cancelled-before-start child never called the provider"
    );
    wait_until("the thread frees", || {
        busy_thread_of(&state, "sess_local_alpha").is_none()
    })
    .await;
    let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
    assert_eq!((per_session, global), (0, 0));
    assert!(
        tools.arrivals.lock().unwrap().is_empty(),
        "ZERO tool executions through the whole scenario"
    );
    teardown(&state, &home).await;
}

// ── F02-C03: abnormal endings still close out ────────────────────────────────

/// The child's provider PANICS mid-run: the run records the loud failure,
/// the thread + lanes close out in-process, and the SAME thread continues
/// with a next run (reply) that succeeds.
#[tokio::test]
async fn child_provider_panic_closes_out_and_the_thread_continues() {
    let thread_slot = Arc::new(Mutex::new(None::<String>));
    let reply_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(
        vec![
            (
                "P".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID: panic in the provider",
                    },
                    Step::Reply {
                        task: "KID2: continue after the panic",
                    },
                    Step::Turn(final_turn("parent done")),
                ],
            ),
            (
                "KID".to_string(),
                vec![Step::Panic("child provider exploded")],
            ),
            (
                "KID2".to_string(),
                vec![Step::Turn(final_turn("child 2 done"))],
            ),
        ],
        vec![(("P".to_string(), 2), Arc::clone(&reply_gate))],
        Arc::clone(&thread_slot),
    );
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "c03-panic",
        ServiceDeps {
            turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
            tool_executor: Some(Arc::clone(&tools) as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    let parent = spawn_execute(&state, "sess_local_alpha", "P: dispatch".to_string());
    // The child's provider panics; the run settles loudly as failed and
    // the thread frees (in-process closeout of an abnormal ending).
    wait_until("the panicking child's thread frees", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| !thread.busy && thread.last_run_status.is_some())
    })
    .await;
    let child = latest_subagent_child_of(&state)
        .await
        .expect("the child run exists");
    wait_status(
        &state,
        &child,
        "failed",
        "the panicking child settles failed",
    )
    .await;
    let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
    assert_eq!((per_session, global), (0, 0));
    // The thread id is observable; fill the provider's slot for the reply.
    let thread_id = state
        .subagents()
        .threads_of("sess_local_alpha")
        .into_iter()
        .next()
        .expect("the thread exists")
        .thread_id;
    provider.set_thread(thread_id);
    reply_gate.add_permits(1);
    let parent_run = parent.await.expect("parent settles");
    assert_eq!(
        run_status(&state, &parent_run).await.as_deref(),
        Some("completed")
    );
    // The SAME thread continued with a SECOND child that completed.
    let continuation = latest_subagent_child_of(&state)
        .await
        .expect("second child");
    assert_ne!(continuation, child);
    wait_status(
        &state,
        &continuation,
        "completed",
        "the continuation child completes",
    )
    .await;
    wait_until("the thread frees again", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .all(|thread| !thread.busy)
    })
    .await;
    teardown(&state, &home).await;
}

// ── F02-C04: the drain-timeout abort is finally recovered ────────────────────

/// A child that outlives the cleanup budget: the drain reports it
/// UNCONFIRMED (abort requested) — and once the child ACTUALLY exits, the
/// registry entry records its exit (never a permanent Running ghost with
/// no handle) and the capacity is reusable.
#[tokio::test(flavor = "current_thread")]
async fn drain_expired_child_is_finally_reaped_after_its_actual_exit() {
    let supervisor = Arc::new(lingxi_service::TaskSupervisor::new(2));
    let root = cancel::CancelScope::run_root("run_c04");
    let scope = root.child("tool:late-exit".to_string(), cancel::ScopeKind::ToolCall);
    let handle = supervisor
        .spawn_linked(
            "run_c04",
            &scope,
            "tool_call:late-exit".to_string(),
            async {
                // Outlives the drain budget below, then finishes on its own.
                tokio::time::sleep(Duration::from_millis(250)).await;
                "late value"
            },
        )
        .expect("spawn");
    let task_id = handle.task_id();
    drop(handle); // fire-and-forget shape: the drain owns the reaping

    let budget = cancel::CancelBudget::new(std::time::Instant::now(), Duration::from_millis(30));
    let report = supervisor.drain_run("run_c04", budget).await;
    assert_eq!(
        report.unconfirmed.len(),
        1,
        "budget expiry reports unconfirmed"
    );
    assert_eq!(report.unconfirmed[0].exit, None, "requested != observed");

    // After the child ACTUALLY exits, the entry must record the exit —
    // never stay Running with no handle forever.
    wait_until("the expired child's exit is finally recorded", || {
        supervisor.exit_of(task_id).is_some()
    })
    .await;
    assert_eq!(
        supervisor.exit_of(task_id),
        Some(TaskExit::Aborted),
        "the requested abort is observed as Aborted once it lands"
    );

    // The capacity is reusable (the ended entry frees its slot at
    // pressure) and a fresh spawn of the same shape works.
    let fresh_scope = root.child("tool:fresh".to_string(), cancel::ScopeKind::ToolCall);
    let fresh = supervisor
        .spawn_linked(
            "run_c04",
            &fresh_scope,
            "tool_call:fresh".to_string(),
            async { "fresh value" },
        )
        .expect("the reaped slot admits a fresh spawn");
    assert_eq!(fresh.wait().await.expect("fresh completes"), "fresh value");
}

// ── F02-C03 adversarial: late/duplicate completions cannot corrupt ──────────

/// A LATE completion callback carrying a SUPERSEDED child_run_id cannot
/// clear the CURRENT run's busy, and a DUPLICATE completion cannot
/// double-decrement the concurrency lanes (the identity fence +
/// exactly-once accounting of the closeout).
#[tokio::test]
async fn late_and_duplicate_completions_cannot_corrupt_the_closeout() {
    let thread_slot = Arc::new(Mutex::new(None::<String>));
    let parent_park = Arc::new(tokio::sync::Semaphore::new(0));
    let kid1_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let kid3_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(
        vec![
            (
                "P1".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID1: first run (cancelled)",
                    },
                    // The parent parks mid-stream after the dispatch so it
                    // is still running (cancellable) when the test acts;
                    // unreachable after the cancellation — later runs come
                    // from FRESH parents.
                    Step::Turn(ProviderTurn::Continue {
                        process_note: "first parent parked".to_string(),
                    }),
                ],
            ),
            (
                "KID1".to_string(),
                vec![
                    Step::Turn(ProviderTurn::Continue {
                        process_note: "first child parked".to_string(),
                    }),
                    Step::Turn(final_turn("never reached")),
                ],
            ),
            (
                "P2".to_string(),
                vec![
                    Step::Reply {
                        task: "KID2: the middle run (completes)",
                    },
                    Step::Turn(final_turn("second parent done")),
                ],
            ),
            (
                "KID2".to_string(),
                vec![Step::Turn(final_turn("middle child done"))],
            ),
            (
                "P3".to_string(),
                vec![
                    Step::Reply {
                        task: "KID3: the current run",
                    },
                    // Parks after the dispatch so the thread stays busy
                    // while the attacks land.
                    Step::Turn(ProviderTurn::Continue {
                        process_note: "third parent parked".to_string(),
                    }),
                ],
            ),
            (
                "KID3".to_string(),
                vec![
                    Step::Turn(ProviderTurn::Continue {
                        process_note: "current child parked".to_string(),
                    }),
                    Step::Turn(final_turn("never reached")),
                ],
            ),
        ],
        vec![
            (("KID1".to_string(), 1), Arc::clone(&kid1_gate)),
            (("P1".to_string(), 2), Arc::clone(&parent_park)),
            (("KID3".to_string(), 1), Arc::clone(&kid3_gate)),
            (
                ("P3".to_string(), 2),
                Arc::new(tokio::sync::Semaphore::new(0)),
            ),
        ],
        Arc::clone(&thread_slot),
    );
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "c03-late-fence",
        ServiceDeps {
            turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
            tool_executor: Some(Arc::clone(&tools) as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Run 1 (KID1): dispatch, then cancel the parent — the child settles
    // cancelled IN PROCESS and the thread frees.
    let parent = spawn_execute(&state, "sess_local_alpha", "P1: dispatch".to_string());
    wait_until("the first child is busy", || {
        busy_thread_of(&state, "sess_local_alpha").is_some()
    })
    .await;
    let first_child = wait_for_subagent_child(&state).await;
    let parent_run = query_text(
        &state,
        "SELECT run_id FROM runs WHERE session_id = ?1 AND run_id NOT IN (SELECT run_id \
         FROM run_lineage WHERE origin = 'subagent')",
        "sess_local_alpha",
    )
    .await
    .expect("parent row");
    cancel_parent(&state, &parent_run).await;
    parent.await.expect("parent settles cancelled");
    wait_status(
        &state,
        &first_child,
        "cancelled",
        "the first child settles cancelled",
    )
    .await;
    wait_until("the thread frees after the cancellation", || {
        busy_thread_of(&state, "sess_local_alpha").is_none()
    })
    .await;
    assert_eq!(state.subagents().active_counts("sess_local_alpha"), (0, 0));

    // The thread id for the REPLY continuations.
    let thread_id = state
        .subagents()
        .threads_of("sess_local_alpha")
        .into_iter()
        .next()
        .expect("the thread exists")
        .thread_id;
    provider.set_thread(thread_id.clone());

    // Run 2 (KID2): a fresh parent replies — the middle child completes
    // NORMALLY (its real completion is the accounted one).
    let _second = spawn_execute(&state, "sess_local_alpha", "P2: reply".to_string());
    let middle_child = wait_for_next_subagent_child(&state, &[first_child.as_str()]).await;
    wait_status(
        &state,
        &middle_child,
        "completed",
        "the middle child completes",
    )
    .await;
    wait_until("the thread frees after the middle run", || {
        busy_thread_of(&state, "sess_local_alpha").is_none()
    })
    .await;
    assert_eq!(state.subagents().active_counts("sess_local_alpha"), (0, 0));

    // Run 3 (KID3): another reply — the CURRENT run parks, holding
    // exactly one lane pair.
    let _third = spawn_execute(&state, "sess_local_alpha", "P3: reply".to_string());
    wait_until("the current child is busy", || {
        busy_thread_of(&state, "sess_local_alpha").is_some()
    })
    .await;
    let current_child =
        wait_for_next_subagent_child(&state, &[first_child.as_str(), middle_child.as_str()]).await;
    assert_ne!(current_child, first_child);
    assert_ne!(current_child, middle_child);
    assert_eq!(state.subagents().active_counts("sess_local_alpha"), (1, 1));

    // ATTACK 1 — a LATE completion for the SUPERSEDED first run arrives
    // while the current run is busy: it must NOT clear the current run's
    // busy and must NOT touch the lane counts.
    state.subagents().note_child_finished(
        "sess_local_alpha",
        &thread_id,
        &first_child,
        &Ok(lingxi_kernel::RunFinish::Cancelled {
            detail: "late stale delivery".to_string(),
        }),
    );
    assert!(
        busy_thread_of(&state, "sess_local_alpha").is_some(),
        "the stale completion cannot clear the CURRENT run's busy"
    );
    assert_eq!(
        state.subagents().active_counts("sess_local_alpha"),
        (1, 1),
        "the stale completion cannot decrement the lanes again"
    );

    // ATTACK 2 — a DUPLICATE completion for the ALREADY-accounted middle
    // run (double fire): exactly-once accounting keeps the lanes honest.
    state.subagents().note_child_finished(
        "sess_local_alpha",
        &thread_id,
        &middle_child,
        &Ok(lingxi_kernel::RunFinish::CompletedWithoutFinal {
            cause: lingxi_kernel::NoFinalCause::EmptyReply,
        }),
    );
    assert_eq!(
        state.subagents().active_counts("sess_local_alpha"),
        (1, 1),
        "a duplicate completion is a no-op (exactly-once)"
    );
    assert!(
        busy_thread_of(&state, "sess_local_alpha").is_some(),
        "a duplicate completion cannot clear the current run's busy"
    );

    // The current run is released and closes out for real.
    kid3_gate.add_permits(1);
    wait_status(
        &state,
        &current_child,
        "completed",
        "the current child completes",
    )
    .await;
    wait_until("the thread frees for real", || {
        busy_thread_of(&state, "sess_local_alpha").is_none()
    })
    .await;
    assert_eq!(state.subagents().active_counts("sess_local_alpha"), (0, 0));
    teardown(&state, &home).await;
}

// ── F02-C05: background panic diagnosis + two-registry reclamation ──────────

/// A PANICKING detached background task: the panic is contained and its
/// exit recorded IN-BAND (queryable WITHOUT any waiter); repeated
/// panicking spawns never exhaust the registry capacity; an unrelated
/// sentinel keeps running throughout.
#[tokio::test(flavor = "current_thread")]
async fn panicking_background_exits_are_recorded_and_reaped_without_a_waiter() {
    let supervisor = Arc::new(lingxi_service::TaskSupervisor::new(3));
    let ticks = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let sentinel = supervisor
        .spawn_detached("background:sentinel".to_string(), {
            let ticks = Arc::clone(&ticks);
            async move {
                for _ in 0..10 {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                    ticks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                "sentinel done"
            }
        })
        .expect("sentinel spawn");

    let mut ids = Vec::new();
    for i in 0..3 {
        let handle = supervisor
            .spawn_detached(format!("background:boom-{i}"), async move {
                panic!("background task {i} exploded");
            })
            .expect("panicking spawn");
        let id = handle.task_id();
        drop(handle); // fire-and-forget: NO waiter ever polls this handle
                      // The panic is contained and recorded IN-BAND (queryable without
                      // any waiter) — prove it per task before the next spawn.
        wait_until("panicking background exit recorded", || {
            matches!(
                supervisor.exit_of(id),
                Some(TaskExit::Panicked(ref detail)) if detail.contains(&format!("background task {i}"))
            )
        })
        .await;
        ids.push(id);
    }
    // The unrelated sentinel kept running through every panic.
    let ticks_during = ticks.load(std::sync::atomic::Ordering::Relaxed);
    assert!(ticks_during > 0, "the sentinel was active throughout");
    assert_eq!(
        sentinel.wait().await.expect("sentinel completes"),
        "sentinel done"
    );
    // The ended entries (panics + sentinel) free their slots at pressure:
    // the capped registry still admits fresh work.
    let fresh = supervisor
        .spawn_detached("background:fresh".to_string(), async { "fresh" })
        .expect("ended entries never permanently consume capacity");
    assert_eq!(fresh.wait().await.expect("fresh completes"), "fresh");
    assert_eq!(
        supervisor
            .tasks()
            .iter()
            .filter(|task| task.exit.is_none())
            .count(),
        0,
        "no Running ghosts remain"
    );
}

/// Service-level two-registry reclamation: finished background drives
/// leave BOTH the drive registry (live_ids) and the task supervisor's
/// entry set — repeatedly, with no growth.
#[tokio::test]
async fn finished_background_drives_reap_both_registries() {
    let thread_slot = Arc::new(Mutex::new(None::<String>));
    let provider = ScriptedProvider::new(
        vec![(
            "BG".to_string(),
            vec![
                Step::Turn(final_turn("background done 1")),
                Step::Turn(final_turn("background done 2")),
                Step::Turn(final_turn("background done 3")),
            ],
        )],
        vec![],
        Arc::clone(&thread_slot),
    );
    let tools = Arc::new(ParkingTool {
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
        arrivals: Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "c05-both-tables",
        ServiceDeps {
            turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
            tool_executor: Some(Arc::clone(&tools) as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    for round in 0..3 {
        let request_id = format!("c05-round-{round}");
        let submission = lingxi_service::ExecuteSubmission {
            input: "BG: quick background run",
            request_id: Some(request_id.as_str()),
        };
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
                "sess_local_alpha",
                &submission,
                NOW_MS,
            )
            .await
            .expect("background admission");
        let run_id = accepted.run_id.clone();
        wait_status(&state, &run_id, "completed", "the background run settles").await;
        // live_ids reaps the finished drive from ITS registry…
        wait_until("the drive leaves the background registry", || {
            !state.background().live_ids().contains(&run_id)
        })
        .await;
        // …and the completion is CONSUMED: the supervised entry for the
        // drive leaves the task registry too (no second-table leak).
        let drive_label = format!("background_drive:{run_id}");
        wait_until("the supervised drive entry is reaped", || {
            !state
                .runs()
                .task_supervisor()
                .tasks()
                .iter()
                .any(|task| task.label == drive_label)
        })
        .await;
    }
    teardown(&state, &home).await;
}
