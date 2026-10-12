//! R03 repair G06/F07 integration｜后台 steering 消费通道 — through the
//! REAL service composition (real storage port, real event service, real
//! kernel state machine + single finalize, real session gate + session
//! lease + steering inbox, real background drive registry + detached
//! supervised drive), driven by deterministic doubles that ONLY produce
//! external responses and RECORD each provider turn's actual input.
//!
//! Test-double boundary: the gated provider below never owns run state —
//! it parks at test-chosen moments and records (session, run_id, pop,
//! input). Every state decision (admission, busy gate, steering
//! accept/miss, drive, cancellation, settle) belongs to the real
//! `SessionStore` / `SessionSupervisor` / `RunSupervisor` chain.
//!
//! Cases (R03-FIX-F07-C01..C04):
//! - C01: a steer `Accepted` while a BACKGROUND run is driving reaches the
//!   NEXT model turn's input, exactly once (adversarial: multiple steers,
//!   different submission orders, across several rounds — no re-drain).
//! - C02: the steered text stays with the AUTHORIZED session/run (two
//!   parallel background sessions; adversarial: cancel then a quick new
//!   run of the same session — the leftover is retained for the session's
//!   next run per the frozen contract, never delivered to another session
//!   or claimed by the cancelled run).
//! - C03: steering that misses the consumption point (run cancelled or
//!   already past its last drain) is RETAINED + observable, never claimed
//!   as executed, never auto-triggers a task; mid-flight steering IS
//!   consumed by its own run and must NOT leak into the session's next
//!   run.
//! - C04: the bounded steering inbox holds the SAME contract for the
//!   foreground and background entries (in-capacity texts really
//!   consumed; over-capacity is a loud refusal that pollutes nothing).

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
    prepare_layout, ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState, SessionConcurrencyLimits, SessionExecuteError, SteerOutcome,
};

const NOW_MS: u64 = 1_790_409_600_000;

// ── deterministic gated double (records every turn's actual input) ──────────

struct GatedProvider {
    scripts: std::sync::Mutex<std::collections::HashMap<String, VecDeque<ProviderTurn>>>,
    arrivals: tokio::sync::mpsc::UnboundedSender<(String, String, usize)>,
    gates: std::collections::HashMap<(String, usize), Arc<tokio::sync::Semaphore>>,
    observed: std::sync::Mutex<Vec<(String, String, usize, String)>>,
    next_pop: std::sync::Mutex<std::collections::HashMap<String, usize>>,
}

impl GatedProvider {
    fn new(
        scripts: Vec<(&'static str, Vec<ProviderTurn>)>,
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, String, usize)>,
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

    /// (session, run_id, pop, input) — the scheduling trace.
    fn observed(&self) -> Vec<(String, String, usize, String)> {
        self.observed.lock().unwrap().clone()
    }

    /// The recorded inputs of ONE session, in pop order.
    fn inputs_of(&self, session: &str) -> Vec<String> {
        self.observed()
            .into_iter()
            .filter(|(sess, _, _, _)| sess == session)
            .map(|(_, _, _, input)| input)
            .collect()
    }

    /// How many times a marker text appears across ALL recorded turn
    /// inputs (every session, every run) — the exactly-once probe.
    fn occurrences_of(&self, marker: &str) -> usize {
        self.observed()
            .into_iter()
            .filter(|(_, _, _, input)| input.contains(marker))
            .count()
    }

    /// The gate handle of one (session, pop) parking point.
    fn gate_of(&self, session: &str, pop: usize) -> Arc<tokio::sync::Semaphore> {
        self.gates
            .get(&(session.to_string(), pop))
            .cloned()
            .expect("gate exists for (session, pop)")
    }
}

impl TurnProviderPort for GatedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.gated.bg-steer".to_string(),
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
        let run = ctx.run_id.to_string();
        let pop = {
            let mut counters = self.next_pop.lock().unwrap();
            let next = counters.get(&session).copied().unwrap_or(0) + 1;
            counters.insert(session.clone(), next);
            next
        };
        self.arrivals
            .send((session.clone(), run.clone(), pop))
            .expect("test holds the arrivals receiver");
        self.observed
            .lock()
            .unwrap()
            .push((session, run, pop, input.to_string()));
        let ctx_at_issue = ctx.clone();
        let gate = self.gates.get(&(ctx.session_id.to_string(), pop)).cloned();
        let turn = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(ctx.session_id.as_str())
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

/// Immediate-success tool double (never requested by these scripts; kept
/// so the composition matches the other integration suites).
struct ImmediateTool;

impl ToolExecutorPort for ImmediateTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::success_text("unused".to_string()),
            )
        })
    }
}

// ── harness ──────────────────────────────────────────────────────────────────

fn gate() -> Arc<tokio::sync::Semaphore> {
    Arc::new(tokio::sync::Semaphore::new(0))
}

fn continue_turn(note: &str) -> ProviderTurn {
    ProviderTurn::Continue {
        process_note: note.to_string(),
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

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03-g06-f07-{tag}-{}-{}",
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

async fn boot(
    tag: &str,
    provider: Arc<GatedProvider>,
    session_concurrency: SessionConcurrencyLimits,
) -> (ServiceState, PathBuf) {
    let home = synthetic_home(tag);
    let layout = prepare_layout(&home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
        tool_executor: Some(Arc::new(ImmediateTool) as Arc<dyn ToolExecutorPort>),
        session_concurrency,
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    (state, home)
}

async fn teardown(state: &ServiceState, home: &std::path::Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

/// One background submission through the REAL admission chain.
async fn submit_background(
    state: &ServiceState,
    session: &str,
    input: &str,
    request_id: &str,
) -> Result<lingxi_service::ExecuteAccepted, SessionExecuteError> {
    let submission = ExecuteSubmission {
        input,
        request_id: Some(request_id),
    };
    let storage = Arc::clone(state.storage());
    let events = Arc::clone(state.events());
    let runs = Arc::clone(state.runs());
    let background = Arc::clone(state.background());
    state
        .sessions()
        .execute_background_for(
            &storage,
            &events,
            &runs,
            &background,
            &owner_principal(),
            session,
            &submission,
            NOW_MS,
        )
        .await
}

/// One foreground submission (the contrast arm of C04 and the quick-new-run
/// arm of C02/C03), driven in its own task exactly like the other suites.
fn submit_foreground(
    state: &ServiceState,
    session: &str,
    input: &str,
) -> tokio::task::JoinHandle<Result<lingxi_service::ExecuteAccepted, SessionExecuteError>> {
    let state = state.clone();
    let session = session.to_string();
    let input = input.to_string();
    tokio::spawn(async move {
        let storage = Arc::clone(state.storage());
        state
            .sessions()
            .execute_for(
                storage.as_ref(),
                state.events(),
                state.runs(),
                &owner_principal(),
                &session,
                &input,
                NOW_MS,
            )
            .await
    })
}

async fn steer(
    state: &ServiceState,
    session: &str,
    text: &str,
) -> Result<SteerOutcome, SessionExecuteError> {
    state
        .sessions()
        .steer_for(&owner_principal(), session, text)
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

async fn run_count(state: &ServiceState, session: &str) -> usize {
    query_text(
        state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        session,
    )
    .await
    .expect("count query")
    .parse::<usize>()
    .expect("numeric count")
}

/// Waits for the (session, pop) arrival, skipping interleaved arrivals of
/// other sessions (deterministic under the per-turn gates).
async fn await_arrival(
    arrivals: &mut tokio::sync::mpsc::UnboundedReceiver<(String, String, usize)>,
    session: &str,
    pop: usize,
) -> String {
    loop {
        let (sess, run, got) = arrivals.recv().await.expect("arrivals channel open");
        if sess == session && got == pop {
            return run;
        }
    }
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

async fn wait_background_settled(state: &ServiceState) {
    wait_until("the background drives settle", || {
        state.background().live_ids().is_empty()
    })
    .await;
}

/// Test evidence sink (the R02 env-var pattern): when R03_G06_EVIDENCE
/// points at a JSON file, each test appends its machine record.
fn write_evidence(key: &str, value: serde_json::Value) {
    if let Ok(path) = std::env::var("R03_G06_EVIDENCE") {
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

// ── C01: Accepted steering reaches the background run's next turn ───────────

/// R03-FIX-F07-C01: a real background drive parks in its first model turn
/// (the session stays busy); `steer_for` answers `Accepted`; releasing the
/// first turn lets the drive continue — the NEXT model turn's input must
/// carry the steered text, exactly once.
#[tokio::test]
async fn c01_background_accepted_steering_reaches_next_turn_exactly_once() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let g1 = gate();
    let g2 = gate();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                continue_turn("working on it"),
                final_turn("done after steering"),
            ],
        )],
        arrivals_tx,
        vec![
            (("sess_local_alpha", 1), g1.clone()),
            (("sess_local_alpha", 2), g2.clone()),
        ],
    );
    let (state, home) = boot("c01", provider.clone(), SessionConcurrencyLimits::default()).await;

    // The background drive parks inside its first model turn.
    let accepted = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C01: please continue",
        "bg-steer-c01",
    )
    .await
    .expect("background admission");
    let run_id = accepted.run_id.clone();
    let arrived_run = await_arrival(&mut arrivals, "sess_local_alpha", 1).await;
    assert_eq!(arrived_run, run_id, "turn 1 belongs to the background run");
    assert!(
        state
            .sessions()
            .session_supervisor()
            .is_busy("sess_local_alpha"),
        "the session stays busy while the background drive runs"
    );

    // Steering accepted into the running turn (loop NOT interrupted).
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-C01-focus-on-config")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );

    // Release turn 1 → the drive continues into turn 2.
    g1.add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 2).await;
    let inputs = provider.inputs_of("sess_local_alpha");
    assert_eq!(inputs.len(), 2);
    assert_eq!(
        inputs[0], "BG-C01: please continue",
        "turn 1 saw the plain input (no steering yet)"
    );
    assert!(
        inputs[1].contains("[steering]") && inputs[1].contains("STEER-C01-focus-on-config"),
        "turn 2 must carry the steered text: {}",
        inputs[1]
    );

    // Settle and prove exactly-once across every recorded turn.
    g2.add_permits(1);
    wait_background_settled(&state).await;
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(
        provider.occurrences_of("STEER-C01-focus-on-config"),
        1,
        "the steered text reaches exactly one model turn"
    );
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0,
        "nothing is left pending after the drain"
    );

    write_evidence(
        "c01",
        serde_json::json!({
            "runId": run_id,
            "steerOutcome": "Accepted",
            "turnInputs": provider.inputs_of("sess_local_alpha"),
            "occurrences": provider.occurrences_of("STEER-C01-focus-on-config"),
        }),
    );
    teardown(&state, &home).await;
}

/// R03-FIX-F07-C01 adversarial: several steers, submitted in different
/// orders, across several rounds of the SAME background run — each text
/// drains exactly once, at the first turn AFTER its acceptance, in
/// submission order; later turns never re-drain earlier texts.
#[tokio::test]
async fn c01_adversarial_multi_steer_orders_and_rounds_no_redrain() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let gates: Vec<Arc<tokio::sync::Semaphore>> = vec![gate(), gate(), gate(), gate()];
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                continue_turn("round 1"),
                continue_turn("round 2"),
                continue_turn("round 3"),
                final_turn("done"),
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
    let (state, home) = boot(
        "c01-adv",
        provider.clone(),
        SessionConcurrencyLimits::default(),
    )
    .await;

    let accepted = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C01-ADV: loop several rounds",
        "bg-steer-c01-adv",
    )
    .await
    .expect("background admission");
    let run_id = accepted.run_id.clone();
    await_arrival(&mut arrivals, "sess_local_alpha", 1).await;

    // Round 1 → 2: two steers, submission order preserved in the join.
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-ADV-first")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-ADV-second")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    gates[0].add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 2).await;

    // Round 2 → 3: a NEW pair, submitted in reverse-of-sorted order to
    // prove the join follows submission order, not content order.
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-ADV-zulu")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-ADV-yankee")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    gates[1].add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 3).await;

    gates[2].add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 4).await;
    gates[3].add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &run_id).await.0, "completed");

    let inputs = provider.inputs_of("sess_local_alpha");
    assert_eq!(inputs.len(), 4);
    assert!(
        inputs[1].contains("[steering]\nSTEER-ADV-first\nSTEER-ADV-second"),
        "round 2 drains both round-1 steers in submission order: {}",
        inputs[1]
    );
    assert!(
        inputs[2].contains("[steering]\nSTEER-ADV-zulu\nSTEER-ADV-yankee"),
        "round 3 drains only the round-2 steers, in submission order: {}",
        inputs[2]
    );
    assert!(
        !inputs[2].contains("STEER-ADV-first") && !inputs[2].contains("STEER-ADV-second"),
        "an earlier-drained text never re-drains: {}",
        inputs[2]
    );
    assert!(
        !inputs[3].contains("[steering]"),
        "the final turn drains nothing new: {}",
        inputs[3]
    );
    for marker in [
        "STEER-ADV-first",
        "STEER-ADV-second",
        "STEER-ADV-zulu",
        "STEER-ADV-yankee",
    ] {
        assert_eq!(
            provider.occurrences_of(marker),
            1,
            "{marker} must reach exactly one model turn"
        );
    }
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0
    );

    write_evidence(
        "c01_adversarial",
        serde_json::json!({
            "runId": run_id,
            "turnInputs": provider.inputs_of("sess_local_alpha"),
            "occurrences": {
                "first": provider.occurrences_of("STEER-ADV-first"),
                "second": provider.occurrences_of("STEER-ADV-second"),
                "zulu": provider.occurrences_of("STEER-ADV-zulu"),
                "yankee": provider.occurrences_of("STEER-ADV-yankee"),
            },
        }),
    );
    teardown(&state, &home).await;
}

// ── C02: identity isolation across sessions and runs ────────────────────────

/// R03-FIX-F07-C02: two parallel background sessions; steering accepted
/// for ONE of them reaches only that session's run — never the other
/// session's turns and never the other session's LATER runs.
#[tokio::test]
async fn c02_steering_stays_in_the_authorized_session_across_parallel_runs() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![
            (
                "sess_local_alpha",
                vec![continue_turn("alpha turn"), final_turn("alpha done")],
            ),
            (
                "sess_local_beta",
                vec![
                    continue_turn("beta run one"),
                    final_turn("beta one done"),
                    continue_turn("beta run two"),
                    final_turn("beta two done"),
                ],
            ),
        ],
        arrivals_tx,
        vec![
            (("sess_local_alpha", 1), gate()),
            (("sess_local_alpha", 2), gate()),
            (("sess_local_beta", 1), gate()),
            (("sess_local_beta", 2), gate()),
            (("sess_local_beta", 3), gate()),
            (("sess_local_beta", 4), gate()),
        ],
    );
    let (state, home) = boot("c02", provider.clone(), SessionConcurrencyLimits::default()).await;

    let alpha_run = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C02-A: authorized target",
        "bg-steer-c02-a",
    )
    .await
    .expect("alpha background admission")
    .run_id;
    let beta_run = submit_background(
        &state,
        "sess_local_beta",
        "BG-C02-B: bystander",
        "bg-steer-c02-b",
    )
    .await
    .expect("beta background admission")
    .run_id;
    let alpha_turn1_run = await_arrival(&mut arrivals, "sess_local_alpha", 1).await;
    let beta_turn1_run = await_arrival(&mut arrivals, "sess_local_beta", 1).await;
    assert_eq!(alpha_turn1_run, alpha_run);
    assert_eq!(beta_turn1_run, beta_run);

    // Steering ONLY for the authorized session's running turn.
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-C02-only-alpha")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );

    provider.gate_of("sess_local_alpha", 1).add_permits(1);
    provider.gate_of("sess_local_beta", 1).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 2).await;
    await_arrival(&mut arrivals, "sess_local_beta", 2).await;

    let alpha_inputs = provider.inputs_of("sess_local_alpha");
    let beta_inputs = provider.inputs_of("sess_local_beta");
    assert!(
        alpha_inputs[1].contains("[steering]") && alpha_inputs[1].contains("STEER-C02-only-alpha"),
        "the authorized session's next turn carries the steer: {}",
        alpha_inputs[1]
    );
    assert!(
        !beta_inputs
            .iter()
            .any(|input| input.contains("STEER-C02-only-alpha")),
        "the bystander session never sees the steer: {beta_inputs:?}"
    );

    // Settle both; then an UNRELATED next run of the bystander session
    // must still not receive the other session's steering.
    provider.gate_of("sess_local_alpha", 2).add_permits(1);
    provider.gate_of("sess_local_beta", 2).add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &alpha_run).await.0, "completed");
    assert_eq!(run_status(&state, &beta_run).await.0, "completed");

    let beta_second = submit_background(
        &state,
        "sess_local_beta",
        "BG-C02-B2: unrelated new task",
        "bg-steer-c02-b2",
    )
    .await
    .expect("beta second background admission")
    .run_id;
    await_arrival(&mut arrivals, "sess_local_beta", 3).await;
    let beta_inputs = provider.inputs_of("sess_local_beta");
    assert!(
        !beta_inputs
            .iter()
            .any(|input| input.contains("STEER-C02-only-alpha")),
        "the unrelated next run carries no foreign steering: {beta_inputs:?}"
    );
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0,
        "the steer was fully consumed by its own session"
    );

    provider.gate_of("sess_local_beta", 3).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_beta", 4).await;
    provider.gate_of("sess_local_beta", 4).add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &beta_second).await.0, "completed");

    write_evidence(
        "c02",
        serde_json::json!({
            "alphaRun": alpha_run,
            "betaRun": beta_run,
            "betaSecondRun": beta_second,
            "alphaInputs": provider.inputs_of("sess_local_alpha"),
            "betaInputs": provider.inputs_of("sess_local_beta"),
            "occurrencesOfSteer": provider.occurrences_of("STEER-C02-only-alpha"),
        }),
    );
    teardown(&state, &home).await;
}

/// R03-FIX-F07-C02 adversarial + C03 cancel boundary: steering accepted
/// for a background run that is then CANCELLED before its consumption
/// point is (a) never delivered into the cancelled run's turns, (b)
/// retained + observable for the SESSION's next run (the frozen
/// recipient), (c) never delivered to another session, and (d) never the
/// trigger of a new task — the next run exists only because it is
/// submitted.
#[tokio::test]
async fn c02_adversarial_cancel_then_quick_new_run_no_misattribution() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![
            // The provider pops per SESSION across runs: the cancelled run
            // consumes exactly ONE pop (turn 1); the quick new run starts
            // at pop 2 — its first turn carries the retained steering.
            (
                "sess_local_alpha",
                vec![
                    continue_turn("cancelled run turn"),
                    continue_turn("new run turn"),
                    final_turn("new run done"),
                ],
            ),
            (
                "sess_local_beta",
                vec![continue_turn("beta turn"), final_turn("beta done")],
            ),
        ],
        arrivals_tx,
        vec![
            (("sess_local_alpha", 1), gate()),
            (("sess_local_alpha", 2), gate()),
            (("sess_local_alpha", 3), gate()),
            (("sess_local_beta", 1), gate()),
            (("sess_local_beta", 2), gate()),
        ],
    );
    let (state, home) = boot(
        "c02-adv",
        provider.clone(),
        SessionConcurrencyLimits::default(),
    )
    .await;

    // The soon-to-be-cancelled background run parks in turn 1.
    let cancelled_run = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C02ADV: will be cancelled",
        "bg-steer-c02adv",
    )
    .await
    .expect("background admission")
    .run_id;
    await_arrival(&mut arrivals, "sess_local_alpha", 1).await;

    // Steering accepted while the run is live; the run is then cancelled
    // BEFORE the text could reach any model turn.
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-C02ADV-during-cancelled")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    let storage = Arc::clone(state.storage());
    match state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &owner_principal(),
            &cancelled_run,
        )
        .await
    {
        Ok(lingxi_service::CancelRunOutcome::Accepted { run_id, .. }) => {
            assert_eq!(run_id, cancelled_run);
        }
        other => panic!("expected cancel Accepted, got {other:?}"),
    }
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &cancelled_run).await.0, "cancelled");

    // (a) the cancelled run never consumed the text…
    assert!(
        !provider
            .inputs_of("sess_local_alpha")
            .iter()
            .any(|input| input.contains("STEER-C02ADV-during-cancelled")),
        "the cancelled run's turns carry no steering: {:?}",
        provider.inputs_of("sess_local_alpha")
    );
    // (b) …it is retained + observable for the session's next run.
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        1,
        "the unconsumed steer is retained (never dropped, never claimed)"
    );
    // (d) steering itself triggered no new task…
    assert_eq!(
        run_count(&state, "sess_local_alpha").await,
        1,
        "no new run appeared without a submission"
    );
    // …and an idle session misses steering (the frozen fallback).
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-C02ADV-idle")
            .await
            .unwrap(),
        SteerOutcome::Miss
    );

    // The quick new run (foreground entry — cross-entry consistency): its
    // FIRST model call receives the retained steering (same session — the
    // frozen recipient), never another session's turns.
    let second = submit_foreground(&state, "sess_local_alpha", "FG-C02ADV: quick new run");
    let new_run = await_arrival(&mut arrivals, "sess_local_alpha", 2).await;
    let new_run_first_input = provider.inputs_of("sess_local_alpha")[1].clone();
    assert!(
        new_run_first_input.contains("[steering]")
            && new_run_first_input.contains("STEER-C02ADV-during-cancelled"),
        "the session's next run receives the retained steer at its first turn: {new_run_first_input}"
    );
    assert!(
        !new_run_first_input.contains("STEER-C02ADV-idle"),
        "the idle miss stored nothing: {new_run_first_input}"
    );
    provider.gate_of("sess_local_alpha", 2).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 3).await;
    provider.gate_of("sess_local_alpha", 3).add_permits(1);
    let accepted = second.await.unwrap().expect("new run settles");
    assert_eq!(accepted.run_id, new_run);
    assert_eq!(run_status(&state, &new_run).await.0, "completed");
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0
    );
    // (c) another session stays untouched end-to-end.
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_beta"),
        0
    );

    write_evidence(
        "c02_adversarial",
        serde_json::json!({
            "cancelledRun": cancelled_run,
            "newRun": new_run,
            "alphaInputs": provider.inputs_of("sess_local_alpha"),
            "steeringPendingAfterCancel": 1,
            "runCountAfterCancel": 1,
        }),
    );
    teardown(&state, &home).await;
}

// ── C03: the consumption-point boundaries ───────────────────────────────────

/// R03-FIX-F07-C03 (the leak leg): steering accepted MID-FLIGHT is
/// consumed by ITS OWN run's next turn — after the run settles nothing is
/// left pending, so a later (unrelated) task of the same session starts
/// CLEAN. Before the fix the background run never drained: the text stayed
/// pending and silently leaked into the session's NEXT run's first turn.
#[tokio::test]
async fn c03_midflight_steering_is_consumed_not_leaked_into_next_run() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                continue_turn("first run turn"),
                final_turn("first run done"),
                continue_turn("second run turn"),
                final_turn("second run done"),
            ],
        )],
        arrivals_tx,
        vec![
            (("sess_local_alpha", 1), gate()),
            (("sess_local_alpha", 2), gate()),
            (("sess_local_alpha", 3), gate()),
            (("sess_local_alpha", 4), gate()),
        ],
    );
    let (state, home) = boot(
        "c03-leak",
        provider.clone(),
        SessionConcurrencyLimits::default(),
    )
    .await;

    let first = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C03: first task",
        "bg-steer-c03-first",
    )
    .await
    .expect("background admission")
    .run_id;
    await_arrival(&mut arrivals, "sess_local_alpha", 1).await;

    // Mid-flight steer: the run is STILL driving (turn 1 parked).
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-C03-midflight")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    provider.gate_of("sess_local_alpha", 1).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 2).await;
    provider.gate_of("sess_local_alpha", 2).add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &first).await.0, "completed");

    let first_inputs = provider.inputs_of("sess_local_alpha");
    assert!(
        first_inputs[1].contains("[steering]") && first_inputs[1].contains("STEER-C03-midflight"),
        "the run consumed its own steer at its next turn: {}",
        first_inputs[1]
    );
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0,
        "consumed in full — nothing may leak past the run that accepted it"
    );

    // The UNRELATED next task of the same session starts clean.
    let second = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C03: second unrelated task",
        "bg-steer-c03-second",
    )
    .await
    .expect("second background admission")
    .run_id;
    await_arrival(&mut arrivals, "sess_local_alpha", 3).await;
    let all_inputs = provider.inputs_of("sess_local_alpha");
    // The second run's turns are pops 3.. (the first run consumed pops 1–2).
    let second_run_inputs = &all_inputs[2..];
    assert!(
        !second_run_inputs
            .iter()
            .any(|input| input.contains("STEER-C03-midflight")),
        "no residual steering leaks into the next task: {second_run_inputs:?}"
    );
    provider.gate_of("sess_local_alpha", 3).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 4).await;
    provider.gate_of("sess_local_alpha", 4).add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &second).await.0, "completed");

    write_evidence(
        "c03_leak",
        serde_json::json!({
            "firstRun": first,
            "secondRun": second,
            "turnInputs": provider.inputs_of("sess_local_alpha"),
            "occurrences": provider.occurrences_of("STEER-C03-midflight"),
        }),
    );
    teardown(&state, &home).await;
}

/// R03-FIX-F07-C03 (the retention leg): a steer that MISSES the run's
/// last drain (accepted while the final turn is already in flight) is
/// retained + observable for the session's next run — never claimed as
/// executed by the finished run, never dropped, and never a trigger.
#[tokio::test]
async fn c03_too_late_steering_retained_not_claimed_no_auto_trigger() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                continue_turn("first run turn"),
                final_turn("first run done"),
                continue_turn("second run turn"),
                final_turn("second run done"),
            ],
        )],
        arrivals_tx,
        vec![
            (("sess_local_alpha", 1), gate()),
            (("sess_local_alpha", 2), gate()),
            (("sess_local_alpha", 3), gate()),
            (("sess_local_alpha", 4), gate()),
        ],
    );
    let (state, home) = boot(
        "c03-late",
        provider.clone(),
        SessionConcurrencyLimits::default(),
    )
    .await;

    let first = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C03LATE: first task",
        "bg-steer-c03late",
    )
    .await
    .expect("background admission")
    .run_id;
    await_arrival(&mut arrivals, "sess_local_alpha", 1).await;
    provider.gate_of("sess_local_alpha", 1).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 2).await;

    // The final turn is in flight: the run's last drain already happened.
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-C03LATE-too-late")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    provider.gate_of("sess_local_alpha", 2).add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(
        run_status(&state, &first).await,
        (
            "completed".to_string(),
            Some("completed.with_final".to_string())
        ),
        "the run settles its own terminal (no fabricated claim about the steer)"
    );
    assert!(
        !provider
            .inputs_of("sess_local_alpha")
            .iter()
            .any(|input| input.contains("STEER-C03LATE-too-late")),
        "the finished run never claims the missed steer as executed"
    );
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        1,
        "the missed steer is retained (observable), not dropped"
    );
    assert_eq!(
        run_count(&state, "sess_local_alpha").await,
        1,
        "the retained steer triggers no new task by itself"
    );

    // The session's next run carries it at its FIRST model call.
    let second = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C03LATE: second task",
        "bg-steer-c03late-2",
    )
    .await
    .expect("second background admission")
    .run_id;
    await_arrival(&mut arrivals, "sess_local_alpha", 3).await;
    let inputs = provider.inputs_of("sess_local_alpha");
    assert!(
        inputs[2].contains("[steering]") && inputs[2].contains("STEER-C03LATE-too-late"),
        "the next run's first turn receives the retained steer: {}",
        inputs[2]
    );
    provider.gate_of("sess_local_alpha", 3).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 4).await;
    provider.gate_of("sess_local_alpha", 4).add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &second).await.0, "completed");
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0
    );

    write_evidence(
        "c03_retention",
        serde_json::json!({
            "firstRun": first,
            "secondRun": second,
            "turnInputs": provider.inputs_of("sess_local_alpha"),
            "pendingAfterFirstRun": 1,
        }),
    );
    teardown(&state, &home).await;
}

// ── C04: the bounded inbox holds for BOTH entries ────────────────────────────

/// R03-FIX-F07-C04: with a tiny steering-inbox capacity, BOTH the
/// background and the foreground entries consume the in-capacity texts
/// for real and refuse the overflow LOUDLY (identical contract, identical
/// gate — the entry changes nothing).
#[tokio::test]
async fn c04_bounded_inbox_same_contract_foreground_and_background() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![
            (
                "sess_local_beta",
                vec![continue_turn("bg turn"), final_turn("bg done")],
            ),
            (
                "sess_local_alpha",
                vec![continue_turn("fg turn"), final_turn("fg done")],
            ),
        ],
        arrivals_tx,
        vec![
            (("sess_local_beta", 1), gate()),
            (("sess_local_beta", 2), gate()),
            (("sess_local_alpha", 1), gate()),
            (("sess_local_alpha", 2), gate()),
        ],
    );
    let (state, home) = boot(
        "c04",
        provider.clone(),
        SessionConcurrencyLimits {
            steering_inbox_capacity: 2,
            registry_cap: SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
        },
    )
    .await;

    // Background leg.
    let bg_run = submit_background(
        &state,
        "sess_local_beta",
        "BG-C04: bounded background",
        "bg-steer-c04",
    )
    .await
    .expect("background admission")
    .run_id;
    await_arrival(&mut arrivals, "sess_local_beta", 1).await;
    assert_eq!(
        steer(&state, "sess_local_beta", "STEER-C04BG-one")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    assert_eq!(
        steer(&state, "sess_local_beta", "STEER-C04BG-two")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    match steer(&state, "sess_local_beta", "STEER-C04BG-overflow").await {
        Err(SessionExecuteError::SteeringInboxFull) => {}
        other => panic!("background overflow must be a loud SteeringInboxFull, got {other:?}"),
    }
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_beta"),
        2,
        "the refused text pollutes nothing (exactly the two accepted remain)"
    );
    provider.gate_of("sess_local_beta", 1).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_beta", 2).await;
    let bg_inputs = provider.inputs_of("sess_local_beta");
    assert!(
        bg_inputs[1].contains("STEER-C04BG-one") && bg_inputs[1].contains("STEER-C04BG-two"),
        "in-capacity steering is really consumed by the background run: {}",
        bg_inputs[1]
    );
    assert!(
        !bg_inputs
            .iter()
            .any(|input| input.contains("STEER-C04BG-overflow")),
        "the refused text never reaches a model turn: {bg_inputs:?}"
    );
    provider.gate_of("sess_local_beta", 2).add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &bg_run).await.0, "completed");
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_beta"),
        0
    );

    // Foreground leg — the SAME tiny capacity, the SAME gate.
    let fg = submit_foreground(&state, "sess_local_alpha", "FG-C04: bounded foreground");
    await_arrival(&mut arrivals, "sess_local_alpha", 1).await;
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-C04FG-one")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    assert_eq!(
        steer(&state, "sess_local_alpha", "STEER-C04FG-two")
            .await
            .unwrap(),
        SteerOutcome::Accepted
    );
    match steer(&state, "sess_local_alpha", "STEER-C04FG-overflow").await {
        Err(SessionExecuteError::SteeringInboxFull) => {}
        other => panic!("foreground overflow must be a loud SteeringInboxFull, got {other:?}"),
    }
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        2
    );
    provider.gate_of("sess_local_alpha", 1).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 2).await;
    let fg_inputs = provider.inputs_of("sess_local_alpha");
    assert!(
        fg_inputs[1].contains("STEER-C04FG-one") && fg_inputs[1].contains("STEER-C04FG-two"),
        "in-capacity steering is really consumed by the foreground run: {}",
        fg_inputs[1]
    );
    assert!(
        !fg_inputs
            .iter()
            .any(|input| input.contains("STEER-C04FG-overflow")),
        "the refused text never reaches a model turn: {fg_inputs:?}"
    );
    provider.gate_of("sess_local_alpha", 2).add_permits(1);
    let fg_accepted = fg.await.unwrap().expect("foreground run settles");
    assert_eq!(run_status(&state, &fg_accepted.run_id).await.0, "completed");
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0
    );

    write_evidence(
        "c04",
        serde_json::json!({
            "backgroundRun": bg_run,
            "foregroundRun": fg_accepted.run_id,
            "backgroundInputs": provider.inputs_of("sess_local_beta"),
            "foregroundInputs": provider.inputs_of("sess_local_alpha"),
            "capacity": 2,
        }),
    );
    teardown(&state, &home).await;
}

/// R03-FIX-F07-C04 adversarial: CONCURRENT steering submissions (not the
/// sequential leg above) racing the tiny capacity from parallel tasks —
/// exactly `capacity` texts are Accepted, the rest are the SAME loud
/// refusal, nothing is lost, duplicated or queued past the bound, and
/// every accepted text is drained exactly once by the run.
#[tokio::test]
async fn c04_adversarial_concurrent_steers_respect_the_bound() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = GatedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![continue_turn("turn"), final_turn("done")],
        )],
        arrivals_tx,
        vec![
            (("sess_local_alpha", 1), gate()),
            (("sess_local_alpha", 2), gate()),
        ],
    );
    let (state, home) = boot(
        "c04-conc",
        provider.clone(),
        SessionConcurrencyLimits {
            steering_inbox_capacity: 2,
            registry_cap: SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
        },
    )
    .await;

    let bg_run = submit_background(
        &state,
        "sess_local_alpha",
        "BG-C04CONC: concurrent steers",
        "bg-steer-c04conc",
    )
    .await
    .expect("background admission")
    .run_id;
    await_arrival(&mut arrivals, "sess_local_alpha", 1).await;

    // Six CONCURRENT steering calls racing the capacity of two.
    let texts: Vec<String> = (0..6).map(|i| format!("STEER-C04CONC-{i}")).collect();
    let mut calls = Vec::new();
    for text in &texts {
        let state = state.clone();
        let text = text.clone();
        calls.push(tokio::spawn(async move {
            steer(&state, "sess_local_alpha", &text).await
        }));
    }
    let mut accepted = Vec::new();
    let mut refused = 0usize;
    for call in calls {
        match call.await.unwrap() {
            Ok(SteerOutcome::Accepted) => accepted.push(()),
            Ok(SteerOutcome::Miss) => panic!("the session is busy — a Miss would be wrong"),
            Err(SessionExecuteError::SteeringInboxFull) => refused += 1,
            other => panic!("unexpected steer outcome: {other:?}"),
        }
    }
    assert_eq!(
        accepted.len(),
        2,
        "exactly the capacity is accepted under concurrency"
    );
    assert_eq!(refused, 4, "every over-capacity text is the loud refusal");
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        2,
        "the bound holds: no queue growth past the capacity"
    );

    provider.gate_of("sess_local_alpha", 1).add_permits(1);
    await_arrival(&mut arrivals, "sess_local_alpha", 2).await;
    let inputs = provider.inputs_of("sess_local_alpha");
    let accepted_in_turn2: Vec<&String> = texts
        .iter()
        .filter(|text| inputs[1].contains(text.as_str()))
        .collect();
    assert_eq!(
        accepted_in_turn2.len(),
        2,
        "exactly the two accepted texts reach the next turn (no loss, no duplication): {}",
        inputs[1]
    );
    provider.gate_of("sess_local_alpha", 2).add_permits(1);
    wait_background_settled(&state).await;
    assert_eq!(run_status(&state, &bg_run).await.0, "completed");
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0
    );

    write_evidence(
        "c04_concurrent",
        serde_json::json!({
            "backgroundRun": bg_run,
            "capacity": 2,
            "concurrentCalls": 6,
            "accepted": accepted.len(),
            "refused": refused,
            "turnInputs": provider.inputs_of("sess_local_alpha"),
        }),
    );
    teardown(&state, &home).await;
}
