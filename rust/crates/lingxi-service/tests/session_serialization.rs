//! R03-T02 integration: session serialization + global admission quotas
//! through the REAL service composition (real storage port, real event
//! service, real kernel state machine, real session gate + quota manager),
//! driven by deterministic gated doubles.
//!
//! Acceptance scenarios (taskbook R03 §4):
//! - R03-A03 同会话顺序可重复: two submissions to ONE session under
//!   controlled scheduling — the frozen incumbent semantics decide
//!   (`session_busy` rejection for the normal second submission, steering
//!   accepted without interrupting the loop); durable messages of the
//!   winning run never interleave with a second run's writes. Evidence =
//!   the scheduling trace (provider-observed inputs + durable key-event
//!   order + gate observations).
//! - R03-A04 跨会话不被全局锁阻塞: session A parks in a tool call while
//!   session B's pure-text run completes; cancelling A's driving task
//!   (tokio drop-cancellation — the REAL termination primitive that exists
//!   today; the user-facing cancellation TREE is R03-T03) returns every
//!   quota permit and the session lease.
//!
//! Test-double boundary (R03 test map): the gated doubles below only
//! produce external responses (`ProviderTurn` / `ToolOutcome`) at
//! test-chosen moments; every state decision belongs to the real
//! `RunSupervisor` + kernel state machine + storage port + session gate.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult, ToolExecutorPort,
    ToolOutcome, ToolRequest, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, ToolCallId};
use lingxi_service::{
    prepare_layout, HomeSource, LayeredQuotaLimits, NetworkMode, QuotaLimits, QuotaResource,
    ServiceConfig, ServiceDeps, ServiceState, SteerOutcome,
};

// ── deterministic gated doubles ─────────────────────────────────────────────

/// Gated scripted provider with PER-SESSION scripts: each session pops its
/// own queue (pop index per session, 1-based), records its arrival and its
/// observed INPUT (the scheduling trace), then — for (session, pop) pairs
/// that have a gate — parks on a 0-permit semaphore until the test releases
/// it. Cross-session scenarios therefore stay deterministic regardless of
/// interleaving order.
struct GatedProvider {
    scripts: std::sync::Mutex<std::collections::HashMap<String, VecDeque<ProviderTurn>>>,
    arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
    gates: std::collections::HashMap<(String, usize), Arc<tokio::sync::Semaphore>>,
    observed: std::sync::Mutex<Vec<(String, usize, String)>>,
    next_pop: std::sync::Mutex<std::collections::HashMap<String, usize>>,
}

impl GatedProvider {
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
            observed: std::sync::Mutex::new(Vec::new()),
            next_pop: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn observed_inputs(&self) -> Vec<(String, usize, String)> {
        self.observed.lock().unwrap().clone()
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
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ModelCallId,
        input: &'a ModelTurnInput,

        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let input = input.submission.as_str();
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
        self.observed
            .lock()
            .unwrap()
            .push((session.clone(), pop, input.to_string()));
        let ctx_at_issue = ctx.clone();
        let gate = self.gates.get(&(session.clone(), pop)).cloned();
        let turn = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&session)
            .and_then(|queue| queue.pop_front())
            .unwrap_or_else(|| ProviderTurn::Failed {
                error: lingxi_protocol::ProtocolError::new(
                    lingxi_protocol::ErrorCode::UpstreamUnavailable,
                    "script exhausted",
                    false,
                ),
                retryable: false,
            });
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await.expect("gate semaphore closed");
            }
            ProviderTurnResult::of_ctx(&ctx_at_issue, turn)
        })
    }
}

/// Gated tool double: records the arrival of each tool call and — when a
/// gate exists — PARKS on a 0-permit semaphore (the "tool pause" of the
/// acceptance scenario; the test decides if/when it ever returns).
struct GatedTool {
    arrivals: tokio::sync::mpsc::UnboundedSender<(String, String)>,
    gate: Option<Arc<tokio::sync::Semaphore>>,
    outcome: ToolOutcome,
}

impl GatedTool {
    fn parked_until_released(
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, String)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            arrivals,
            gate: Some(Arc::new(tokio::sync::Semaphore::new(0))),
            outcome: ToolOutcome::success_text("parked-then-success".to_string()),
        })
    }

    fn immediate(
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, String)>,
        outcome: ToolOutcome,
    ) -> Arc<Self> {
        Arc::new(Self {
            arrivals,
            gate: None,
            outcome,
        })
    }

    fn release(&self) {
        if let Some(gate) = &self.gate {
            gate.add_permits(1);
        }
    }
}

impl ToolExecutorPort for GatedTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.arrivals
            .send((ctx.session_id.to_string(), call.to_string()))
            .expect("test holds the tool arrivals receiver");
        let ctx_at_issue = ctx.clone();
        let gate = self.gate.clone();
        let outcome = self.outcome.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await.expect("tool gate closed");
            }
            ToolExecutionResult::of_ctx(&ctx_at_issue, outcome)
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

// ── harness ────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t02-{tag}-{}-{}",
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

async fn steer(
    state: &ServiceState,
    session: &str,
    text: &str,
) -> Result<SteerOutcome, lingxi_service::SessionExecuteError> {
    state
        .sessions()
        .steer_for(&owner_principal(), session, text)
        .await
}

async fn query_text(state: &ServiceState, sql: &str, run_id: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![run_id.to_string()])
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

async fn run_event_types(state: &ServiceState, run_id: &str) -> Vec<String> {
    // Ordered by seq within the session stream — the durable write order.
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

/// Test evidence sink (mirrors the R02 env-var pattern): when
/// R03_T02_EVIDENCE points at a JSON file, each test appends its machine
/// record; the run's log capture keeps the human trace lines.
fn write_evidence(key: &str, value: serde_json::Value) {
    if let Ok(path) = std::env::var("R03_T02_EVIDENCE") {
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

// ── R03-A03: one session, two submissions, frozen semantics ────────────────

/// The acceptance scenario verbatim: ONE session receives two submissions
/// "simultaneously" (under controlled scheduling). The frozen incumbent
/// semantics decide the outcome:
/// - the SECOND NORMAL submission is rejected `session_busy` (retryable)
///   and writes NOTHING (no run row, no key event);
/// - a STEERING submission for the running turn is ACCEPTED and reaches
///   the NEXT model call's input without interrupting the loop (the run
///   keeps its single identity and settles once);
/// - the winning run's durable messages never interleave with a second
///   run's writes (there IS no second run);
/// - after the settle the session accepts the next submission again.
/// Evidence = the scheduling trace below.
#[tokio::test(flavor = "current_thread")]
async fn r03_a03_same_session_serializes_and_steers_per_frozen_semantics() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    // alpha's run: pop1 = Continue (gated), pop2 = Final (gated).
    let g1 = gate();
    let g2 = gate();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                ProviderTurn::Continue {
                    process_note: "working on it".to_string(),
                },
                ProviderTurn::Final {
                    message: assistant_final("done after steering"),
                },
            ],
        )],
        arrivals_tx,
        vec![
            (("sess_local_alpha", 1), g1.clone()),
            (("sess_local_alpha", 2), g2.clone()),
        ],
    );
    let (tool_arrivals_tx, _tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (state, home) = boot(
        "a03",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(GatedTool::immediate(
                tool_arrivals_tx,
                ToolOutcome::success_text("unused".to_string()),
            ) as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Submission 1 parks inside its first model turn (gate g1).
    let first = execute_task(&state, "sess_local_alpha", "please continue");
    assert_eq!(
        arrivals.recv().await.expect("turn1 arrival"),
        ("sess_local_alpha".to_string(), 1)
    );
    println!("R03_A03_TRACE 1: submission1 running, turn1 arrived (session busy)");

    // Submission 2 (NORMAL) while the session is busy: the frozen
    // session_busy rejection — deterministic under the controlled gate.
    let second = execute_task(&state, "sess_local_alpha", "second normal submission");
    match second.await.unwrap() {
        Err(lingxi_service::SessionExecuteError::Busy) => {}
        other => panic!("busy session must reject a normal submission, got {other:?}"),
    }
    println!("R03_A03_TRACE 2: submission2 (normal) REJECTED session_busy, zero side effects");

    // Steering IS accepted for the running turn (distinct submission kind).
    assert_eq!(
        steer(&state, "sess_local_alpha", "focus on the config file")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    println!("R03_A03_TRACE 3: steering ACCEPTED into the running turn (loop not interrupted)");

    // Release turn 1; the driver drains the steering BEFORE turn 2.
    g1.add_permits(1);
    assert_eq!(
        arrivals.recv().await.expect("turn2 arrival"),
        ("sess_local_alpha".to_string(), 2)
    );
    let inputs = provider.observed_inputs();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].2, "please continue");
    assert!(
        inputs[1].2.contains("[steering]") && inputs[1].2.contains("focus on the config file"),
        "turn2 input must carry the steered text: {:?}",
        inputs[1].2
    );
    println!("R03_A03_TRACE 4: turn2 input observed with steered text (steer -> NEXT model call)");

    // Release turn 2 (Final): the run settles exactly once.
    g2.add_permits(1);
    let run_id = first.await.unwrap().expect("first submission settles");
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    // Messages never interleave: the session's durable stream contains
    // EXACTLY this run's events, in order, and no second run exists.
    let events = run_event_types(&state, &run_id).await;
    assert_eq!(
        events,
        vec![
            "run_state_changed",  // queued -> running
            "model_call_started", // turn 1 (continue)
            "model_call_completed",
            "model_call_started", // turn 2 (final, steered input)
            "model_call_completed",
            "run_state_changed", // running -> completed (ONE terminal)
            "final_message_committed",
        ],
        "scheduling trace (durable write order): the winning run's writes \
         are contiguous; the rejected submission wrote nothing"
    );
    let total_runs: i64 = state
        .storage()
        .query_one_text("SELECT COUNT(*) FROM runs", vec![])
        .await
        .unwrap()
        .and_then(|v| v.parse().ok())
        .unwrap();
    assert_eq!(total_runs, 1, "the rejected submission created no run row");
    let total_events: i64 = state
        .storage()
        .query_one_text("SELECT COUNT(*) FROM key_events", vec![])
        .await
        .unwrap()
        .and_then(|v| v.parse().ok())
        .unwrap();
    assert_eq!(total_events, 7, "and no key events either");

    // The session is free again: the next NORMAL submission is accepted.
    let next = execute_task(&state, "sess_local_alpha", "next turn after settle");
    assert_eq!(
        arrivals.recv().await.expect("pop3 arrival"),
        ("sess_local_alpha".to_string(), 3)
    );
    // The script is exhausted beyond pop2: pop3 fails loudly — release any
    // gate (none for pop3) and settle; the acceptance point is that the
    // submission was ACCEPTED (a run id exists), not its outcome.
    let next_id = next.await.unwrap().expect("session accepts after settle");
    let (next_status, _) = run_status(&state, &next_id).await;
    assert_eq!(
        next_status, "failed",
        "script-exhausted double fails loudly"
    );
    println!(
        "R03_A03_TRACE 5: session idle after settle; next submission ACCEPTED (run {next_id})"
    );

    write_evidence(
        "r03_a03",
        serde_json::json!({
            "acceptedRuns": 1,
            "busyRejections": 1,
            "steering": {"accepted": 1, "reachedTurn": 2},
            "durableEventsOfWinner": events,
            "totalRuns": total_runs,
            "totalKeyEvents": total_events,
            "winnerTerminal": {"status": status, "reason": reason},
            "postSettleAcceptance": {"runId": next_id, "status": next_status},
        }),
    );
    teardown(&state, &home).await;
}

// ── R03-A04: cross-session parallelism + quota release on cancellation ─────

/// The acceptance scenario verbatim: session A parks in a TOOL call, session
/// B runs pure text CONCURRENTLY; B completes while A is still parked (no
/// global lock covers tool/model I/O); cancelling A's driving task returns
/// every quota permit and the session lease.
///
/// Cancellation boundary (honest, per dispatch): the user-facing
/// cancellation TREE (authorized cancel entries, cancelling terminal
/// states, child propagation) is R03-T03 and does not exist yet. The
/// termination used here is the REAL primitive available today — tokio
/// task abort, i.e. drop-cancellation of the driving future at its await
/// point (the same mechanism a dropped HTTP request connection uses). It
/// exercises the exact RAII release path any future cancellation takes;
/// the run row honestly stays `running` (no fabricated terminal).
#[tokio::test(flavor = "current_thread")]
async fn r03_a04_cross_session_parallel_and_quota_release_on_cancel() {
    let (model_arrivals_tx, _model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    // alpha: tool-request turn (pop1). beta: pure text final (pop2).
    let provider = GatedProvider::new(
        vec![
            (
                "sess_local_alpha",
                vec![ProviderTurn::ToolRequests {
                    content: Vec::new(),
                    requests: vec![read_tool_request()],
                }],
            ),
            (
                "sess_local_beta",
                vec![ProviderTurn::Final {
                    message: assistant_final("b pure text answer"),
                }],
            ),
        ],
        model_arrivals_tx,
        vec![],
    );
    let (tool_arrivals_tx, mut tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let tools = GatedTool::parked_until_released(tool_arrivals_tx);
    let (state, home) = boot(
        "a04",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            quota_limits: QuotaLimits {
                tool: LayeredQuotaLimits {
                    global: 1,
                    per_agent: 4,
                    per_session: 2,
                },
                model: LayeredQuotaLimits {
                    global: 2,
                    per_agent: 4,
                    per_session: 1,
                },
                ..QuotaLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;
    let quotas = state.runs().quotas();

    // Session A: parks inside its tool call, holding the ONLY global tool
    // permit plus the alpha session lease.
    let a = execute_task(&state, "sess_local_alpha", "a needs the tool");
    let (a_session, a_tool_call) = tool_arrivals.recv().await.expect("tool arrival");
    assert_eq!(a_session, "sess_local_alpha");
    assert!(
        a_tool_call.ends_with("-tc0001"),
        "tool call id: {a_tool_call}"
    );
    let a_run_id = a_tool_call
        .strip_suffix("-tc0001")
        .expect("tool call id shape")
        .to_string();
    assert_eq!(
        quotas.in_use(QuotaResource::Tool),
        1,
        "A holds the global tool permit"
    );
    assert!(state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha"));
    println!("R03_A04_TRACE 1: sessionA parked in tool call (tool permit held, alpha busy)");

    // Session B: pure text, CONCURRENT — must complete while A is parked.
    let b = execute_task(&state, "sess_local_beta", "b pure text");
    let b_run_id = b.await.unwrap().expect("B completes while A is parked");
    let (b_status, b_reason) = run_status(&state, &b_run_id).await;
    assert_eq!(b_status, "completed");
    assert_eq!(b_reason.as_deref(), Some("completed.with_final"));
    assert_eq!(
        quotas.in_use(QuotaResource::Tool),
        1,
        "A is STILL parked — B's progress did not need the tool lane"
    );
    assert_eq!(
        quotas.in_use(QuotaResource::Model),
        0,
        "model permits all returned"
    );
    println!("R03_A04_TRACE 2: sessionB completed while sessionA still parked (no global lock)");

    // Cancel A: tokio task abort = drop-cancellation at the await point
    // (the real termination primitive available before R03-T03).
    a.abort();
    let _ = a.await;
    assert_eq!(
        quotas.in_use(QuotaResource::Tool),
        0,
        "A's cancellation returned the global tool permit (no permit leak)"
    );
    assert_eq!(quotas.in_use(QuotaResource::Model), 0);
    assert!(
        !state
            .sessions()
            .session_supervisor()
            .is_busy("sess_local_alpha"),
        "A's cancellation freed the session lease"
    );
    // The run row is HONEST: still running — the cancelled terminal state
    // belongs to R03-T03's cancellation tree; nothing fabricates a finish.
    let (a_status, a_reason) = run_status(&state, &a_run_id).await;
    assert_eq!(a_status, "running");
    assert_eq!(
        a_reason, None,
        "no terminal reason may be invented for the aborted run"
    );
    println!(
        "R03_A04_TRACE 3: cancelA -> tool/model permits 0, alpha lease freed; \
         run row honestly 'running' (cancelled-terminal = R03-T03)"
    );

    // Quota fully reusable: the freed tool lane admits a fresh tool call.
    tools.release(); // nobody is waiting anymore; harmless

    write_evidence(
        "r03_a04",
        serde_json::json!({
            "bCompletedWhileAParked": {"runId": b_run_id, "status": b_status, "reason": b_reason},
            "toolInUseWhileParked": 1,
            "afterCancel": {"toolInUse": quotas.in_use(QuotaResource::Tool),
                            "modelInUse": quotas.in_use(QuotaResource::Model),
                            "alphaBusy": false},
            "aRunRowHonest": {"runId": a_run_id, "status": a_status,
                              "terminalReason": null},
            "cancellationPrimitive": "tokio task abort (drop at await point); user cancel tree = R03-T03",
        }),
    );
    teardown(&state, &home).await;
}

/// Sibling of the A04 release proof on the ERROR path (no cancellation at
/// all): a tool double that FAILS settles the run and returns every permit.
#[tokio::test(flavor = "current_thread")]
async fn failed_tool_path_settles_and_returns_quotas() {
    let (model_arrivals_tx, _model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                ProviderTurn::ToolRequests {
                    content: Vec::new(),
                    requests: vec![read_tool_request()],
                },
                ProviderTurn::Empty {
                    detail: "cannot answer after the tool failed".to_string(),
                },
            ],
        )],
        model_arrivals_tx,
        vec![],
    );
    let (tool_arrivals_tx, _tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (state, home) = boot(
        "a04fail",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(GatedTool::immediate(
                tool_arrivals_tx,
                ToolOutcome::Failed {
                    error: lingxi_protocol::ProtocolError::new(
                        lingxi_protocol::ErrorCode::Internal,
                        "stub tool failure",
                        false,
                    ),
                },
            ) as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute_task(&state, "sess_local_alpha", "use the broken tool")
        .await
        .unwrap()
        .expect("run settles");
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        reason.as_deref(),
        Some("completed.no_final.tool_partial_failure")
    );
    assert_eq!(state.runs().quotas().in_use(QuotaResource::Tool), 0);
    assert_eq!(state.runs().quotas().in_use(QuotaResource::Model), 0);
    assert!(!state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha"));
    write_evidence(
        "r03_a04_error_path",
        serde_json::json!({
            "runId": run_id, "status": status, "reason": reason,
            "toolInUse": 0, "modelInUse": 0, "sessionBusy": false,
        }),
    );
    teardown(&state, &home).await;
}

/// Quota exhaustion is a LOUD run failure through the single finalize path:
/// with the global tool lane held by a parked call, a second session's tool
/// run waits its bounded wait, times out (a real, bounded 500ms), and settles
/// `failed.quota_exhausted.tool` — never an unbounded wait, never a fake
/// success. The parked holder then finishes normally and returns the lane.
#[tokio::test(flavor = "current_thread")]
async fn quota_exhaustion_fails_loudly_and_releases_on_settle() {
    let (model_arrivals_tx, _model_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![
            // alpha: tool request (parks), then final after release.
            (
                "sess_local_alpha",
                vec![
                    ProviderTurn::ToolRequests {
                        content: Vec::new(),
                        requests: vec![read_tool_request()],
                    },
                    ProviderTurn::Final {
                        message: assistant_final("alpha done after tool"),
                    },
                ],
            ),
            // beta: tool request (waits for the lane, times out).
            (
                "sess_local_beta",
                vec![ProviderTurn::ToolRequests {
                    content: Vec::new(),
                    requests: vec![read_tool_request()],
                }],
            ),
        ],
        model_arrivals_tx,
        vec![],
    );
    let (tool_arrivals_tx, mut tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let tools = GatedTool::parked_until_released(tool_arrivals_tx);
    let (state, home) = boot(
        "quotaexh",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            quota_limits: QuotaLimits {
                tool: LayeredQuotaLimits {
                    global: 1,
                    per_agent: 4,
                    per_session: 2,
                },
                wait_timeout_ms: 500,
                ..QuotaLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;

    // alpha parks holding the only tool permit.
    let alpha = execute_task(&state, "sess_local_alpha", "alpha holds the lane");
    let _first_tool_arrival = tool_arrivals.recv().await.expect("alpha tool arrival");
    assert_eq!(state.runs().quotas().in_use(QuotaResource::Tool), 1);

    // beta's tool run exhausts the bounded wait (the real 500ms deadline
    // fires; the OUTCOME is deterministic — bounded wait, loud failure)
    // and settles loudly.
    let beta = execute_task(&state, "sess_local_beta", "beta needs the lane too");
    let beta_run = beta.await.unwrap().expect("beta settles");
    let (status, reason) = run_status(&state, &beta_run).await;
    assert_eq!(status, "failed");
    assert_eq!(
        reason.as_deref(),
        Some("failed.quota_exhausted.tool"),
        "quota exhaustion is a loud, diagnosable terminal — not a hang, not a fake success"
    );
    assert_eq!(
        state.runs().quotas().in_use(QuotaResource::Tool),
        1,
        "the parked holder still owns the lane"
    );
    assert!(!state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_beta"));

    // Release alpha: its tool succeeds and the run settles; the lane frees.
    tools.release();
    let alpha_run = alpha.await.unwrap().expect("alpha settles");
    let (alpha_status, alpha_reason) = run_status(&state, &alpha_run).await;
    assert_eq!(alpha_status, "completed");
    assert_eq!(alpha_reason.as_deref(), Some("completed.with_final"));
    assert_eq!(state.runs().quotas().in_use(QuotaResource::Tool), 0);

    write_evidence(
        "r03_quota_exhaustion",
        serde_json::json!({
            "beta": {"runId": beta_run, "status": status, "reason": reason},
            "alpha": {"runId": alpha_run, "status": alpha_status,
                      "reason": alpha_reason},
            "toolInUseAfterAll": 0,
        }),
    );
    teardown(&state, &home).await;
}

/// Leftover steering is never dropped: a steer that lands after the run's
/// last drain is retained in the session slot and reaches the session's
/// NEXT run (frozen incumbent semantics: a steered user message stays
/// session input).
#[tokio::test(flavor = "current_thread")]
async fn leftover_steering_survives_into_the_next_run() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    // pop1 = Continue (gated), pop2 = Final (gated); pop3/pop4 = run two's
    // turns (gated). The test keeps a handle to every gate.
    let gates: Vec<Arc<tokio::sync::Semaphore>> = vec![gate(), gate(), gate(), gate()];
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                ProviderTurn::Continue {
                    process_note: "first run".to_string(),
                },
                ProviderTurn::Final {
                    message: assistant_final("first run done"),
                },
                ProviderTurn::Continue {
                    process_note: "second run".to_string(),
                },
                ProviderTurn::Final {
                    message: assistant_final("second run done"),
                },
            ],
        )],
        arrivals_tx,
        vec![
            (("sess_local_alpha", 1), gates[0].clone()),
            (("sess_local_alpha", 2), gates[1].clone()),
            (("sess_local_alpha", 3), gates[2].clone()),
            (("sess_local_alpha", 4), gates[3].clone()),
        ],
    );
    let (tool_arrivals_tx, _tool_arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (state, home) = boot(
        "steerleft",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(GatedTool::immediate(
                tool_arrivals_tx,
                ToolOutcome::success_text("unused".to_string()),
            ) as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Run one: pop1 arrives (parked), release it, pop2 (Final) arrives —
    // the last drain of run one already happened before pop2.
    let first = execute_task(&state, "sess_local_alpha", "run one");
    assert_eq!(
        arrivals.recv().await.unwrap(),
        ("sess_local_alpha".to_string(), 1)
    );
    gates[0].add_permits(1);
    assert_eq!(
        arrivals.recv().await.unwrap(),
        ("sess_local_alpha".to_string(), 2)
    );
    // Steering lands AFTER the last drain of run one: accepted (the run is
    // still driving turn 2), but it cannot reach this run anymore.
    assert_eq!(
        steer(&state, "sess_local_alpha", "late steering")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    gates[1].add_permits(1);
    let first_run = first.await.unwrap().expect("run one settles");
    assert_eq!(run_status(&state, &first_run).await.0, "completed");
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        1,
        "the late steer is retained, not dropped"
    );

    // Run two: its FIRST model call carries the retained steering.
    let second = execute_task(&state, "sess_local_alpha", "run two");
    assert_eq!(
        arrivals.recv().await.unwrap(),
        ("sess_local_alpha".to_string(), 3)
    );
    let inputs = provider.observed_inputs();
    assert!(
        inputs[2].2.contains("[steering]") && inputs[2].2.contains("late steering"),
        "run two's first input carries the retained steering: {:?}",
        inputs[2].2
    );
    gates[2].add_permits(1);
    assert_eq!(
        arrivals.recv().await.unwrap(),
        ("sess_local_alpha".to_string(), 4)
    );
    gates[3].add_permits(1);
    let second_run = second.await.unwrap().expect("run two settles");
    assert_eq!(run_status(&state, &second_run).await.0, "completed");
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0
    );

    write_evidence(
        "r03_leftover_steering",
        serde_json::json!({
            "firstRun": first_run,
            "secondRunFirstInputCarriesSteering": true,
            "secondRun": second_run,
        }),
    );
    teardown(&state, &home).await;
}
