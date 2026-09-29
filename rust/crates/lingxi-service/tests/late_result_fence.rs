//! R03-T04 integration: the attempt/stream RESULT FENCE through the REAL
//! service composition (real storage port incl. the audit-only
//! stale-result transaction, real event service, real kernel state
//! machine and single finalize, real run driver with the write-side fence
//! check, real cancellation tree), driven by deterministic gated doubles.
//!
//! Acceptance scenarios (taskbook R03 §4):
//!
//!
//! - R03-A07 旧结果不能复活任务 (cross-attempt + next-run variants): a
//!   late result from attempt1 — delivered after the run cancelled or
//!   after a retry opened attempt2 / the next Run started — is recorded as
//!   a STALE AUDIT FACT only. It never appends to the current attempt's
//!   stream, never becomes the run's final message and never flips a
//!   settled run's terminal state.
//! - The write-side half of the fence: an in-flight provider/tool result
//!   whose identity fence names a superseded attempt (or a foreign
//!   generation) is refused before ANY state write, audited, and — on a
//!   live run — settles loudly (a fenced model result = provider failure;
//!   a fenced tool result = the started call records Unknown, never a
//!   fabricated success).
//! - 历史重连只订阅: resubscribing with an EventService cursor replays
//!   durable events and starts NOTHING — no new run, no provider call.
//!
//! Test-double boundary (R03 test map): the fenced/gated provider and tool
//! doubles below only produce external responses (optionally carrying a
//! WRONG identity fence, exactly what a deferred/raced adapter delivery
//! looks like); every fence decision, audit write and state transition
//! belongs to the real driver + kernel + storage port.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurnResult, ResultFence, StorageError, StoragePort,
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::{ExecuteSubmission, Principal, PrincipalKind};
use lingxi_service::{HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState};

// ── deterministic doubles ────────────────────────────────────────────────────

/// One scripted provider turn, optionally tagged with a WRONG identity
/// fence (the deferred/raced-adapter delivery the driver must fence) and
/// optionally gated (parks inside the "stream read").
struct ScriptedStep {
    turn: lingxi_kernel::ports::ProviderTurn,
    /// When set, the result carries this CLAIMED fence instead of the
    /// context it was issued under.
    fence_override: Option<ResultFence>,
    gate: Option<Arc<tokio::sync::Semaphore>>,
}

/// Scripted per-session provider: pops one step per model call.
struct FencedProvider {
    scripts: std::sync::Mutex<std::collections::HashMap<String, VecDeque<ScriptedStep>>>,
    arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
    next_pop: std::sync::Mutex<std::collections::HashMap<String, usize>>,
}

impl FencedProvider {
    fn new(
        scripts: Vec<(&'static str, Vec<ScriptedStep>)>,
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scripts: std::sync::Mutex::new(
                scripts
                    .into_iter()
                    .map(|(session, steps)| (session.to_string(), steps.into_iter().collect()))
                    .collect(),
            ),
            arrivals,
            next_pop: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn step(
        turn: lingxi_kernel::ports::ProviderTurn,
    ) -> (ScriptedStep, Option<Arc<tokio::sync::Semaphore>>) {
        let step = ScriptedStep {
            turn,
            fence_override: None,
            gate: None,
        };
        (step, None)
    }

    fn gated_step(
        turn: lingxi_kernel::ports::ProviderTurn,
        gate: Arc<tokio::sync::Semaphore>,
    ) -> ScriptedStep {
        ScriptedStep {
            turn,
            fence_override: None,
            gate: Some(gate),
        }
    }
}

impl TurnProviderPort for FencedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.fenced".to_string(),
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
        let step = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&session)
            .and_then(|queue| queue.pop_front())
            .unwrap_or_else(|| ScriptedStep {
                turn: lingxi_kernel::ports::ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        "script exhausted",
                        false,
                    ),
                    retryable: false,
                },
                fence_override: None,
                gate: None,
            });
        Box::pin(async move {
            if let Some(gate) = step.gate {
                // Park mid-"stream": the run is reading the network.
                let _permit = gate.acquire().await.expect("gate semaphore closed");
            }
            match step.fence_override {
                // The CLAIMED fence wins — this is the late/raced delivery
                // shape the driver must fence.
                Some(fence) => ProviderTurnResult {
                    fence,
                    turn: step.turn,
                },
                None => ProviderTurnResult::of_ctx(&ctx_at_issue, step.turn),
            }
        })
    }
}

/// Tool double: immediate success, optionally carrying a CLAIMED fence
/// (wrong generation / older attempt) instead of the issued context.
struct FencedTool {
    fence_override: Option<ResultFence>,
    calls: std::sync::Mutex<Vec<String>>,
}

impl ToolExecutorPort for FencedTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.calls.lock().unwrap().push(call.to_string());
        let ctx_at_issue = ctx.clone();
        let fence_override = self.fence_override.clone();
        Box::pin(async move {
            let outcome = ToolOutcome::Success {
                content_digest: "tool-content".to_string(),
            };
            match fence_override {
                Some(fence) => ToolExecutionResult { fence, outcome },
                None => ToolExecutionResult::of_ctx(&ctx_at_issue, outcome),
            }
        })
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

fn retryable_failure() -> lingxi_kernel::ports::ProviderTurn {
    lingxi_kernel::ports::ProviderTurn::Failed {
        error: ProtocolError::new(ErrorCode::UpstreamUnavailable, "transient", false),
        retryable: true,
    }
}

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t04-{tag}-{}-{}",
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

fn owner_principal() -> Principal {
    Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: PrincipalKind::LocalUser,
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
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    (state, home)
}

async fn teardown(state: &ServiceState, home: &std::path::Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

/// Executes through the REAL session surface (plain submission shape).
/// Takes an owned clone so it can be spawned (`ServiceState` is a cheap
/// `Arc` clone — the same pattern the cancellation-tree suite uses).
async fn execute_plain(
    state: ServiceState,
    session: &'static str,
    input: &'static str,
) -> Result<String, lingxi_service::SessionExecuteError> {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_submission_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            session,
            &ExecuteSubmission::plain(input),
            1_790_409_600_000,
        )
        .await
        .map(|accepted| accepted.run_id)
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

async fn runs_of_session(state: &ServiceState, session: &str) -> i64 {
    query_text(
        state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        session,
    )
    .await
    .and_then(|v| v.parse().ok())
    .unwrap_or(0)
}

async fn run_event_count(state: &ServiceState, run_id: &str) -> i64 {
    query_text(
        state,
        "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
        run_id,
    )
    .await
    .and_then(|v| v.parse().ok())
    .unwrap_or(0)
}

async fn run_message_count(state: &ServiceState, run_id: &str) -> i64 {
    query_text(
        state,
        "SELECT COUNT(*) FROM messages WHERE run_id = ?1",
        run_id,
    )
    .await
    .and_then(|v| v.parse().ok())
    .unwrap_or(0)
}

/// Latest audit row of one run: (reason, attempt, refused_event_types).
async fn latest_audit(state: &ServiceState, run_id: &str) -> Option<(String, String, String)> {
    let raw = query_text(
        state,
        "SELECT reason || '|' || COALESCE(attempt,'') || '|' || refused_event_types FROM \
         stale_result_audit WHERE run_id = ?1 ORDER BY audit_id DESC LIMIT 1",
        run_id,
    )
    .await?;
    let mut parts = raw.split('|');
    let reason = parts.next()?.to_string();
    let attempt = parts.next().unwrap_or("").to_string();
    let refused = parts.next().unwrap_or("").to_string();
    Some((reason, attempt, refused))
}

async fn audit_count(state: &ServiceState, run_id: &str) -> i64 {
    query_text(
        state,
        "SELECT COUNT(*) FROM stale_result_audit WHERE run_id = ?1",
        run_id,
    )
    .await
    .and_then(|v| v.parse().ok())
    .unwrap_or(0)
}

/// The out-of-band delivery surface: a LATE result arriving through the
/// storage port with the CLAIMED (stale) identity — exactly what a
/// deferred adapter delivery or a replayed client submission looks like.
async fn deliver_late_model_result(
    state: &ServiceState,
    run_id: &str,
    attempt_seq: u32,
    late_call_suffix: &str,
    now_ms: u64,
) -> Result<lingxi_kernel::ports::CommittedOutcome, StorageError> {
    let run = lingxi_protocol::RunId::new(run_id.to_string());
    let ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: run.clone(),
        attempt: lingxi_kernel::attempt_id(&run, attempt_seq),
        generation: 1,
    };
    let event = lingxi_kernel::ports::KeyEvent {
        event_id: lingxi_protocol::EventId::new(format!("{run_id}-{late_call_suffix}-done")),
        payload: lingxi_protocol::EventPayload::Known(
            lingxi_protocol::KnownEventPayload::ModelCallCompleted(
                lingxi_protocol::ModelCallCompletedPayload {
                    model_call_id: ModelCallId::new(format!("{run_id}-{late_call_suffix}")),
                    usage: None,
                },
            ),
        ),
    };
    state
        .storage()
        .record_run_events(&ctx, vec![event], now_ms)
        .await
}

/// Bounded deterministic wait (1ms polls under a hard deadline — a real,
/// bounded wait; no test-util time control).
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

/// Test evidence sink (the established R02/R03 env-var pattern).
fn write_evidence(key: &str, value: serde_json::Value) {
    if let Ok(path) = std::env::var("R03_T04_EVIDENCE") {
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

// ── R03-A07 (variant 1): attempt1's late result vs the CURRENT attempt ───────

/// attempt1's retryable failure opens attempt2; while attempt2 is parked in
/// the stream read, attempt1's LATE result is delivered out-of-band through
/// the storage port. It must be refused for state purposes and audited as
/// a stale fact; the run's stream must be byte-unchanged; when attempt2
/// then completes, its final answer is the ONLY body the run commits.
#[tokio::test(flavor = "current_thread")]
async fn r03_a07_late_attempt1_result_vs_current_attempt_is_stale_only() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let attempt2_gate = gate();
    let provider = FencedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                FencedProvider::step(retryable_failure()).0,
                FencedProvider::gated_step(
                    lingxi_kernel::ports::ProviderTurn::Final {
                        message: assistant_final("attempt2 final answer"),
                    },
                    Arc::clone(&attempt2_gate),
                ),
            ],
        )],
        arrivals_tx,
    );
    let (state, home) = boot(
        "a07-attempt",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Drive: pop1 (attempt1) fails retryably → attempt2 opens and parks.
    let task = tokio::spawn(execute_plain(
        state.clone(),
        "sess_local_alpha",
        "a07 attempt case",
    ));
    assert_eq!(
        arrivals.recv().await.expect("attempt1 turn"),
        ("sess_local_alpha".to_string(), 1)
    );
    assert_eq!(
        arrivals.recv().await.expect("attempt2 turn"),
        ("sess_local_alpha".to_string(), 2)
    );
    let run_id = loop {
        // The run row exists once the drive committed its start.
        let id = query_text(
            &state,
            "SELECT run_id FROM runs WHERE session_id = ?1",
            "sess_local_alpha",
        )
        .await;
        if let Some(id) = id {
            break id;
        }
        tokio::task::yield_now().await;
    };
    // Attempt2 is parked in the stream read: attempt1 is now superseded.
    assert!(
        wait_until(500, || {
            state.runs().cancel_registry().get(&run_id).is_some()
        })
        .await,
        "run registered live"
    );
    let events_before = run_event_count(&state, &run_id).await;
    let messages_before = run_message_count(&state, &run_id).await;

    // THE LATE RESULT of attempt1, delivered out-of-band.
    match deliver_late_model_result(&state, &run_id, 1, "mc0009", 1_790_409_600_100).await {
        Err(StorageError::Conflict { detail }) => {
            assert!(
                detail.contains("no longer the current attempt"),
                "detail: {detail}"
            );
        }
        other => panic!("late attempt1 result must conflict, got {other:?}"),
    }

    // Audit-only: exactly one stale row naming attempt1; nothing else moved.
    assert_eq!(audit_count(&state, &run_id).await, 1);
    let (reason, attempt, refused) = latest_audit(&state, &run_id).await.expect("audit row");
    assert_eq!(reason, "attempt_stale");
    assert_eq!(attempt, format!("{run_id}#a1"));
    assert!(
        refused.contains("model_call_completed"),
        "refused: {refused}"
    );
    assert_eq!(
        run_event_count(&state, &run_id).await,
        events_before,
        "the refused delivery appended NOTHING to the stream"
    );
    assert_eq!(
        run_message_count(&state, &run_id).await,
        messages_before,
        "no message row appeared"
    );
    // A duplicate late delivery is again refused AND again audited
    // (duplicate form of the property's stale class).
    assert!(
        deliver_late_model_result(&state, &run_id, 1, "mc0009", 1_790_409_600_110)
            .await
            .is_err()
    );
    assert_eq!(audit_count(&state, &run_id).await, 2);

    // Release attempt2: its final answer is the run's ONLY body.
    attempt2_gate.add_permits(1);
    let accepted_run = task.await.expect("task").expect("settles");
    assert_eq!(accepted_run, run_id);
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(run_message_count(&state, &run_id).await, 1);
    let body = query_text(
        &state,
        "SELECT content_json FROM messages WHERE run_id = ?1",
        &run_id,
    )
    .await
    .expect("final message row");
    assert!(
        body.contains("attempt2 final answer"),
        "final body must be attempt2's: {body}"
    );
    assert!(
        !body.contains("mc0009"),
        "the late attempt1 result never became content"
    );
    let late_leak = query_text(
        &state,
        "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND payload_json LIKE '%mc0009%'",
        &run_id,
    )
    .await
    .unwrap_or_else(|| "0".to_string());
    assert_eq!(late_leak, "0", "the late result never entered the stream");

    write_evidence(
        "r03_a07_attempt_stale",
        serde_json::json!({
            "run_id": run_id,
            "audit_reason": "attempt_stale",
            "late_attempt": format!("{run_id}#a1"),
            "final_reason": "completed.with_final",
            "late_event_leak_count": late_leak,
        }),
    );
    println!("R03_A07_TRACE attempt_stale: run={run_id} audit=attempt_stale final=completed.with_final leak=0");
    teardown(&state, &home).await;
}

// ── R03-A07 (variant 2): late result vs a CANCELLED run and the NEXT run ────

/// attempt1's run is CANCELLED (settled terminal) and the NEXT Run of the
/// same session has started; only then is attempt1's result delivered. It
/// must be stale-audited against the cancelled run and must leave the new
/// run byte-untouched (cross-attempt + cross-run assertion).
#[tokio::test(flavor = "current_thread")]
async fn r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let run1_gate = gate();
    let provider = FencedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                FencedProvider::gated_step(
                    lingxi_kernel::ports::ProviderTurn::Final {
                        message: assistant_final("run1 attempt1 answer (late)"),
                    },
                    Arc::clone(&run1_gate),
                ),
                FencedProvider::step(lingxi_kernel::ports::ProviderTurn::Final {
                    message: assistant_final("run2 fresh answer"),
                })
                .0,
            ],
        )],
        arrivals_tx,
    );
    let (state, home) = boot(
        "a07-nextrun",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Run1 parks in the stream read; cancel settles it `cancelled`.
    let run1_task = tokio::spawn(execute_plain(
        state.clone(),
        "sess_local_alpha",
        "a07 next-run case",
    ));
    assert_eq!(
        arrivals.recv().await.expect("run1 turn"),
        ("sess_local_alpha".to_string(), 1)
    );
    let run1 = loop {
        let id = query_text(
            &state,
            "SELECT run_id FROM runs WHERE session_id = ?1",
            "sess_local_alpha",
        )
        .await;
        if let Some(id) = id {
            break id;
        }
        tokio::task::yield_now().await;
    };
    match cancel_run(&state, &run1).await.expect("cancel") {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("cancel must be accepted, got {other:?}"),
    }
    run1_task.await.expect("run1 task").expect("run1 settles");
    let (status1, reason1) = run_status(&state, &run1).await;
    assert_eq!(status1, "cancelled");
    assert_eq!(reason1.as_deref(), Some("cancelled.requested"));

    // The NEXT Run of the same session starts and completes.
    let run2 = execute_plain(
        state.clone(),
        "sess_local_alpha",
        "a07 next-run second submission",
    )
    .await
    .expect("run2 settles");
    assert_ne!(run1, run2, "the next run is a NEW run");
    let (status2, reason2) = run_status(&state, &run2).await;
    assert_eq!(status2, "completed");
    assert_eq!(reason2.as_deref(), Some("completed.with_final"));
    let run2_events_before = run_event_count(&state, &run2).await;
    let run2_messages_before = run_message_count(&state, &run2).await;

    // THE LATE RESULT of run1/attempt1, delivered after everything moved on.
    match deliver_late_model_result(&state, &run1, 1, "mc0007", 1_790_409_600_200).await {
        Err(StorageError::Conflict { detail }) => {
            assert!(detail.contains("already terminal"), "detail: {detail}");
        }
        other => panic!("late result must conflict, got {other:?}"),
    }

    // Audit-only, bound to the CANCELLED run; the new run is untouched.
    assert_eq!(audit_count(&state, &run1).await, 1);
    let (audit_reason, audit_attempt, refused) =
        latest_audit(&state, &run1).await.expect("audit row");
    assert_eq!(audit_reason, "run_terminal");
    assert_eq!(audit_attempt, format!("{run1}#a1"));
    assert!(refused.contains("model_call_completed"));
    let (status1_after, reason1_after) = run_status(&state, &run1).await;
    assert_eq!(
        (status1_after.clone(), reason1_after),
        (status1, reason1),
        "the settled run never flips"
    );
    assert_eq!(
        run_message_count(&state, &run1).await,
        0,
        "a cancelled run gains no final body from a late result"
    );
    assert_eq!(
        run_event_count(&state, &run2).await,
        run2_events_before,
        "the new run's stream is untouched"
    );
    assert_eq!(run_message_count(&state, &run2).await, run2_messages_before);
    let body2 = query_text(
        &state,
        "SELECT content_json FROM messages WHERE run_id = ?1",
        &run2,
    )
    .await
    .expect("run2 final");
    assert!(body2.contains("run2 fresh answer"));
    assert!(!body2.contains("run1 attempt1"));
    let cross_leak = query_text(
        &state,
        "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND payload_json LIKE '%mc0007%'",
        &run2,
    )
    .await
    .unwrap_or_else(|| "0".to_string());
    assert_eq!(cross_leak, "0", "the late result never reached the new run");

    write_evidence(
        "r03_a07_run_terminal",
        serde_json::json!({
            "cancelled_run": run1,
            "next_run": run2,
            "audit_reason": audit_reason,
            "run1_status_after": status1_after.clone(),
            "run2_status": status2,
            "cross_run_leak_count": cross_leak,
        }),
    );
    println!(
        "R03_A07_TRACE run_terminal: cancelled={run1} next={run2} audit=run_terminal cross_leak=0"
    );
    teardown(&state, &home).await;
}

// ── write-side fence: in-flight results with a WRONG identity ───────────────

/// A provider result tagged with the SUPERSEDED attempt's fence arrives on
/// a live run: refused before ANY state write, audited as fence_mismatch,
/// and the run settles loudly (never a silent skip, never the stale
/// content as the body).
#[tokio::test(flavor = "current_thread")]
async fn r03_a07_driver_fences_stale_tagged_model_result_loudly() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    // A faithful model of a deferred adapter delivery: attempt1's context
    // is captured when its call is issued; when attempt2's slot asks for a
    // turn, the double returns the OLD call's result TAGGED with attempt1's
    // identity fence.
    struct DeferredDeliveryProvider {
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
        seen_attempt1: std::sync::Mutex<Option<RunContext>>,
        next_pop: std::sync::Mutex<std::collections::HashMap<String, usize>>,
    }
    impl TurnProviderPort for DeferredDeliveryProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                provider: "stub.deferred".to_string(),
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
                .send((session, pop))
                .expect("test holds the arrivals receiver");
            let issued = ctx.clone();
            if issued.attempt.to_string().ends_with("#a1") {
                *self.seen_attempt1.lock().unwrap() = Some(issued.clone());
                Box::pin(async move {
                    // Attempt1's call: fail retryably so attempt2 opens.
                    ProviderTurnResult::of_ctx(
                        &issued,
                        lingxi_kernel::ports::ProviderTurn::Failed {
                            error: ProtocolError::new(
                                ErrorCode::UpstreamUnavailable,
                                "transient",
                                false,
                            ),
                            retryable: true,
                        },
                    )
                })
            } else {
                // Attempt2's slot receives the OLD call's deferred result,
                // tagged with attempt1's identity fence.
                let stale_ctx = self
                    .seen_attempt1
                    .lock()
                    .unwrap()
                    .clone()
                    .expect("attempt1 was observed first");
                Box::pin(async move {
                    ProviderTurnResult {
                        fence: ResultFence::of_ctx(&stale_ctx),
                        turn: lingxi_kernel::ports::ProviderTurn::Final {
                            message: assistant_final("stale attempt1 content"),
                        },
                    }
                })
            }
        }
    }
    let provider = Arc::new(DeferredDeliveryProvider {
        arrivals: arrivals_tx,
        seen_attempt1: std::sync::Mutex::new(None),
        next_pop: std::sync::Mutex::new(std::collections::HashMap::new()),
    });
    let (state, home) = boot(
        "a07-fence",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_plain(state.clone(), "sess_local_alpha", "a07 fence case")
        .await
        .expect("run settles");
    assert_eq!(
        arrivals.recv().await.expect("attempt1"),
        ("sess_local_alpha".to_string(), 1)
    );
    assert_eq!(
        arrivals.recv().await.expect("attempt2"),
        ("sess_local_alpha".to_string(), 2)
    );

    // Loud failure with the fenced vocabulary; the stale content is NOWHERE.
    let (status, terminal) = run_status(&state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(terminal.as_deref(), Some("failed.provider_error"));
    assert_eq!(run_message_count(&state, &run_id).await, 0);
    // Attempt1's FAILED turn was persisted under its own (then-current)
    // attempt — mc0001 events exist; the fenced attempt2 result (mc0002)
    // never wrote anything.
    let mc2 = query_text(
        &state,
        "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND payload_json LIKE '%mc0002%'",
        &run_id,
    )
    .await
    .unwrap_or_else(|| "0".to_string());
    assert_eq!(mc2, "0", "the fenced result wrote no state");
    let (audit_reason, audit_attempt, refused) =
        latest_audit(&state, &run_id).await.expect("audit row");
    assert_eq!(audit_reason, "fence_mismatch");
    assert_eq!(audit_attempt, format!("{run_id}#a1"));
    assert!(refused.contains("model_call_result"));

    write_evidence(
        "r03_a07_fence_mismatch_model",
        serde_json::json!({
            "run_id": run_id,
            "status": status,
            "audit_reason": audit_reason,
            "fenced_attempt": audit_attempt,
            "stale_turn_events_written": mc2,
        }),
    );
    println!(
        "R03_A07_TRACE fence_mismatch(model): run={run_id} status=failed audit=fence_mismatch attempt={audit_attempt}"
    );
    teardown(&state, &home).await;
}

/// A TOOL result tagged with a foreign generation is fenced: the started
/// call records Unknown (receipt not trustworthy — never retried blindly,
/// never a success) and the stale claim is audited; the run itself
/// continues and completes with its real final answer.
#[tokio::test(flavor = "current_thread")]
async fn r03_a07_tool_result_with_stale_fence_records_unknown_and_audits() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = FencedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                FencedProvider::step(lingxi_kernel::ports::ProviderTurn::ToolRequests {
                    requests: vec![read_tool_request()],
                })
                .0,
                FencedProvider::step(lingxi_kernel::ports::ProviderTurn::Final {
                    message: assistant_final("real final after fenced tool"),
                })
                .0,
            ],
        )],
        arrivals_tx,
    );
    // Foreign generation 99: the fence names a registry generation the
    // writer does not hold → FenceMismatch.
    let tools = Arc::new(FencedTool {
        fence_override: None,
        calls: std::sync::Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "a07-toolfence",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some({
                // Wrap: override the fence at delivery time via a distinct
                // double carrying generation 99 for the CURRENT run.
                struct ForeignGenTool {
                    inner: Arc<FencedTool>,
                }
                impl ToolExecutorPort for ForeignGenTool {
                    fn execute<'a>(
                        &'a self,
                        ctx: &'a RunContext,
                        call: &'a ToolCallId,
                        request: &'a ToolRequest,
                    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>>
                    {
                        let inner = self.inner.execute(ctx, call, request);
                        Box::pin(async move {
                            let mut result = inner.await;
                            result.fence = ResultFence {
                                run_id: ctx.run_id.clone(),
                                attempt: ctx.attempt.clone(),
                                generation: 99,
                            };
                            result
                        })
                    }
                }
                Arc::new(ForeignGenTool {
                    inner: Arc::clone(&tools),
                }) as Arc<dyn ToolExecutorPort>
            }),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_plain(state.clone(), "sess_local_alpha", "a07 tool fence case")
        .await
        .expect("run settles");
    assert_eq!(
        arrivals.recv().await.expect("turn1"),
        ("sess_local_alpha".to_string(), 1)
    );
    assert_eq!(
        arrivals.recv().await.expect("turn2"),
        ("sess_local_alpha".to_string(), 2)
    );

    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    // The started tool call recorded UNKNOWN (never a fabricated success).
    let unknown_recorded = query_text(
        &state,
        "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = \
         'tool_call_completed' AND payload_json LIKE '%unknown%'",
        &run_id,
    )
    .await
    .unwrap_or_else(|| "0".to_string());
    assert_eq!(
        unknown_recorded, "1",
        "the fenced tool result must record Unknown, not success"
    );
    let success_leak = query_text(
        &state,
        "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = \
         'tool_call_completed' AND payload_json LIKE '%tool-content%'",
        &run_id,
    )
    .await
    .unwrap_or_else(|| "0".to_string());
    assert_eq!(
        success_leak, "0",
        "the stale tool content never became state"
    );
    let (audit_reason, audit_attempt, refused) =
        latest_audit(&state, &run_id).await.expect("audit row");
    assert_eq!(audit_reason, "fence_mismatch");
    assert_eq!(audit_attempt, format!("{run_id}#a1"));
    assert!(refused.contains("tool_call_result"));

    write_evidence(
        "r03_a07_fence_mismatch_tool",
        serde_json::json!({
            "run_id": run_id,
            "status": status,
            "unknown_recorded": unknown_recorded,
            "audit_reason": audit_reason,
        }),
    );
    println!("R03_A07_TRACE fence_mismatch(tool): run={run_id} unknown=1 audit=fence_mismatch");
    teardown(&state, &home).await;
}

// ── 历史重连只订阅，不重新启动模型 ──────────────────────────────────────────

/// After a run settles, a subscriber that (re)connects with an EventService
/// cursor only READS: durable events are replayed/resumed, no run is
/// created, no provider call happens (arrivals channel stays empty).
#[tokio::test(flavor = "current_thread")]
async fn reconnect_resubscribes_only_and_never_restarts_a_model() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let provider = FencedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                FencedProvider::step(lingxi_kernel::ports::ProviderTurn::Final {
                    message: assistant_final("reconnect probe answer"),
                })
                .0,
            ],
        )],
        arrivals_tx,
    );
    let (state, home) = boot(
        "reconnect",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute_plain(state.clone(), "sess_local_alpha", "reconnect probe")
        .await
        .expect("settles");
    assert_eq!(
        arrivals.recv().await.expect("the one model call"),
        ("sess_local_alpha".to_string(), 1)
    );
    let runs_before = runs_of_session(&state, "sess_local_alpha").await;

    // First subscription: a fresh snapshot cut.
    let first = state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("subscribe");
    let (cut, _guard) = match first {
        lingxi_service::SubscribeOutcome::Started { cut, subscription } => (cut, subscription),
        other => panic!("expected Started, got {other:?}"),
    };
    assert!(cut.snapshot_seq.value() > 0, "durable events were replayed");

    // Reconnect with the cut's cursor: continuation only.
    let cursor = cut.next_cursor.clone().unwrap_or_else(|| {
        lingxi_service::SubscribeCursor::encode(&lingxi_service::SubscribeCursor {
            stream_id: "sess_local_alpha".to_string(),
            seq: cut.snapshot_seq,
        })
    });
    let resumed = state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", Some(cursor))
        .await
        .expect("resubscribe");
    match resumed {
        lingxi_service::SubscribeOutcome::Started { cut, .. } => {
            assert_eq!(cut.mode, "resume", "the cursor continues the durable log");
        }
        other => panic!("expected Started on resume, got {other:?}"),
    }

    // NOTHING restarted: no new run, no provider call, status unchanged.
    assert_eq!(
        runs_of_session(&state, "sess_local_alpha").await,
        runs_before,
        "reconnect must not create runs"
    );
    assert!(
        arrivals.try_recv().is_err(),
        "reconnect must not start a model call"
    );
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(
        (status.as_str(), reason.as_deref()),
        ("completed", Some("completed.with_final"))
    );

    write_evidence(
        "reconnect_read_only",
        serde_json::json!({
            "run_id": run_id,
            "runs_before": runs_before,
            "runs_after": runs_of_session(&state, "sess_local_alpha").await,
            "provider_calls_after_reconnect": 0,
        }),
    );
    println!("R03_T04_TRACE reconnect_read_only: runs unchanged, provider calls 0");
    teardown(&state, &home).await;
}
