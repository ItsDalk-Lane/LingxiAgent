//! R03-T01 / R03-A01 integration: the REAL run lifecycle through the real
//! service composition (real storage port, real event service, real kernel
//! state machine), driven by deterministic provider/tool DOUBLES injected
//! through `ServiceDeps`.
//!
//! Test-double boundary (R03 test map): the doubles below only produce
//! external responses (`ProviderTurn` / `ToolOutcome`). Every state
//! decision — which turn ends the run, the outcome contract, the single
//! finalize — belongs to `RunSupervisor` + the kernel state machine + the
//! storage port. The doubles write no state and never finalize.

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
use lingxi_protocol::{
    AttemptId, ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, RunId,
    ToolCallId,
};
use lingxi_service::{
    prepare_layout, RunDriveLimits, RunSupervisor, ServiceConfig, ServiceDeps, ServiceState,
    SubscribeOutcome,
};
use lingxi_service::{HomeSource, NetworkMode};

// ── deterministic doubles (test-only; never in production wiring) ──────────

/// Scripted provider: pops turns from a fixed script; records the
/// (attempt, model-call) identity it was asked under. When the script is
/// exhausted it fails loudly (a run never "succeeds" by double accident).
struct ScriptedProvider {
    script: std::sync::Mutex<VecDeque<ProviderTurn>>,
    observed: std::sync::Mutex<Vec<(String, String)>>,
}

impl ScriptedProvider {
    fn new(script: Vec<ProviderTurn>) -> Arc<Self> {
        Arc::new(Self {
            script: std::sync::Mutex::new(script.into_iter().collect()),
            observed: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn observed_attempts(&self) -> Vec<String> {
        self.observed
            .lock()
            .unwrap()
            .iter()
            .map(|(attempt, _)| attempt.clone())
            .collect()
    }

    fn observed_call_ids(&self) -> Vec<String> {
        self.observed
            .lock()
            .unwrap()
            .iter()
            .map(|(_, call)| call.clone())
            .collect()
    }
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,

        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.observed
            .lock()
            .unwrap()
            .push((ctx.attempt.to_string(), call.to_string()));
        let ctx_at_issue = ctx.clone();
        let turn =
            self.script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        "script exhausted",
                        false,
                    ),
                    retryable: false,
                });
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, turn) })
    }
}

/// Scripted tool double: every request returns the configured outcome.
struct ToolDouble {
    outcome: ToolOutcome,
    calls: std::sync::Mutex<Vec<(String, String)>>,
}

impl ToolDouble {
    fn succeeding() -> Arc<Self> {
        Arc::new(Self {
            outcome: ToolOutcome::success_text("deadbeef".to_string()),
            calls: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn failing() -> Arc<Self> {
        Arc::new(Self {
            outcome: ToolOutcome::Failed {
                error: ProtocolError::new(ErrorCode::Internal, "stub tool failure", false),
            },
            calls: std::sync::Mutex::new(Vec::new()),
        })
    }
}

impl ToolExecutorPort for ToolDouble {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.calls
            .lock()
            .unwrap()
            .push((ctx.attempt.to_string(), call.to_string()));
        let ctx_at_issue = ctx.clone();
        let outcome = self.outcome.clone();
        Box::pin(async move { ToolExecutionResult::of_ctx(&ctx_at_issue, outcome) })
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

fn assistant_final(text: &str, call: &str) -> NormalizedMessage {
    NormalizedMessage {
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        model_call_id: Some(ModelCallId::new(call.to_string())),
    }
}

// ── harness ────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t01-{tag}-{}-{}",
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

async fn execute(state: &ServiceState, input: &str) -> String {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            input,
            1_790_409_600_000,
        )
        .await
        .expect("execute accepted")
        .run_id
}

/// Durable events of one run straight from the authority (key_events).
async fn run_events(state: &ServiceState, run_id: &str) -> Vec<lingxi_protocol::EventEnvelope> {
    use lingxi_kernel::ports::EventStorePort as _;
    state
        .storage()
        .stream_events_after("sess_local_alpha", lingxi_protocol::Seq::new(0), 10_000)
        .await
        .expect("authority read")
        .into_iter()
        .filter(|e| e.run_id.as_ref().map(|r| r.as_str()) == Some(run_id))
        .collect()
}

async fn run_row(state: &ServiceState, run_id: &str) -> (String, Option<String>, i64) {
    let status = state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query status")
        .expect("run row");
    let reason = state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query reason");
    let attempts: i64 = state
        .storage()
        .query_one_text(
            "SELECT attempt_count FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query attempts")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);
    (status, reason, attempts)
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

async fn final_message_content(state: &ServiceState, run_id: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(
            "SELECT content_json FROM messages WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("final message query")
}

// ── R03-A01: three model calls, ONE task terminal ─────────────────────────

/// The acceptance scenario verbatim: the double drives tool-request →
/// continue → final-reply. One Run, three ModelCalls, a single task
/// terminal; the END of an intermediate model call is never the end of the
/// run. Evidence = durable events + database rows (isolated local
/// environment, deterministic doubles).
#[tokio::test]
async fn r03_a01_three_model_calls_produce_exactly_one_task_terminal() {
    let provider = ScriptedProvider::new(vec![
        ProviderTurn::ToolRequests {
            content: Vec::new(),
            requests: vec![read_tool_request()],
        },
        ProviderTurn::Continue {
            process_note: "reasoning about tool output".to_string(),
        },
        ProviderTurn::Final {
            message: assistant_final("final answer after tool", "mc-3"),
        },
    ]);
    let tools = ToolDouble::succeeding();
    let (state, home) = boot(
        "a01",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Subscribe BEFORE the run: the live hub must also see the whole chain.
    let subscription = match state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("subscribe")
    {
        SubscribeOutcome::Started { subscription, .. } => subscription,
        SubscribeOutcome::RequiresSnapshot(_) => panic!("fresh stream needs no snapshot"),
    };

    let run_id = execute(&state, "please use the read tool").await;

    // One run, completed, with a committed final message.
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(attempts, 1, "no retry happened: one attempt");

    // Exactly three model calls, each with its OWN id, all under attempt #a1.
    assert_eq!(
        count_events(&state, &run_id, "model_call_started").await,
        3,
        "three model calls started"
    );
    assert_eq!(
        count_events(&state, &run_id, "model_call_completed").await,
        3,
        "three model calls completed"
    );
    let expected_calls: Vec<String> = (1..=3).map(|n| format!("{run_id}-mc{n:04}")).collect();
    assert_eq!(provider.observed_call_ids(), expected_calls);
    assert!(provider
        .observed_attempts()
        .iter()
        .all(|a| a == &format!("{run_id}#a1")));

    // The tool call has its own identity layer.
    assert_eq!(count_events(&state, &run_id, "tool_call_started").await, 1);
    assert_eq!(
        count_events(&state, &run_id, "tool_call_completed").await,
        1
    );
    assert_eq!(
        tools.calls.lock().unwrap()[0].1,
        format!("{run_id}-tc0001"),
        "tool call id is minted by the driver, independent of run/attempt"
    );

    // ONE task terminal: exactly one terminal run_state_changed, and it is
    // the LAST durable event of the run — the ends of model calls 1 and 2
    // never finalized anything.
    let events = run_events(&state, &run_id).await;
    let terminal_positions: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            matches!(
                e.payload,
                lingxi_protocol::EventPayload::Known(
                    lingxi_protocol::KnownEventPayload::RunStateChanged(ref p)
                ) if p.to.is_terminal()
            )
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        terminal_positions,
        vec![events.len() - 2],
        "exactly one terminal run_state_changed, ordered just before the final message event: \
         positions {terminal_positions:?} of {} events",
        events.len()
    );
    if let lingxi_protocol::EventPayload::Known(
        lingxi_protocol::KnownEventPayload::RunStateChanged(ref p),
    ) = events[events.len() - 2].payload
    {
        assert_eq!(p.from, lingxi_protocol::RunStatus::Running);
        assert_eq!(p.to, lingxi_protocol::RunStatus::Completed);
    } else {
        panic!("second-to-last event must be the terminal transition");
    }
    // Final message event is the very last durable fact, and the message
    // row exists with the model call attribution.
    assert_eq!(
        events.last().unwrap().payload.event_type(),
        "final_message_committed"
    );
    let content = final_message_content(&state, &run_id)
        .await
        .expect("final message row exists");
    assert!(content.contains("final answer after tool"), "{content}");

    // Full durable event chain of the run (ordering witness):
    // start, mc1 started/done, tool start/done, mc2 started/done,
    // mc3 started/done, terminal, final message.
    let types: Vec<&str> = events.iter().map(|e| e.payload.event_type()).collect();
    assert_eq!(
        types,
        vec![
            "run_state_changed", // queued -> running (creation)
            "model_call_started",
            "model_call_completed",
            "tool_call_started",
            "tool_call_completed",
            "model_call_started",
            "model_call_completed",
            "model_call_started",
            "model_call_completed",
            "run_state_changed", // running -> completed (the ONE terminal)
            "final_message_committed",
        ],
        "middle model-call ends are process facts, never task terminals"
    );

    // Live hub observation: every durable event of this run was published
    // post-commit (11 frames).
    let mut live = 0;
    while subscription
        .mailbox()
        .try_recv()
        .map(|f| matches!(f, lingxi_service::SubscriptionFrame::Event(_)))
        .unwrap_or(false)
    {
        live += 1;
    }
    assert_eq!(live, 11, "hub delivered the whole committed chain");

    teardown(&state, &home).await;
}

// ── R03-T01 step 2: retry = new attempt on the SAME run ───────────────────

#[tokio::test]
async fn retryable_provider_failure_reopens_attempt_on_the_same_run() {
    let provider = ScriptedProvider::new(vec![
        ProviderTurn::Failed {
            error: ProtocolError::new(ErrorCode::UpstreamUnavailable, "connection reset", true),
            retryable: true,
        },
        ProviderTurn::Final {
            message: assistant_final("recovered on attempt two", "mc-2"),
        },
    ]);
    let (state, home) = boot(
        "retry",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: None,
            run_limits: RunDriveLimits {
                max_model_turns: 8,
                max_attempts: 2,
            },
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute(&state, "flaky request").await;

    // ONE run (the run id is fixed at creation; a provider reconnect never
    // mints a second user task), TWO attempts.
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(attempts, 2, "the retry opened attempt #a2 on the same run");
    let total_runs: i64 = state
        .storage()
        .query_one_text("SELECT COUNT(*) FROM runs", vec![])
        .await
        .unwrap()
        .and_then(|v| v.parse().ok())
        .unwrap();
    assert_eq!(total_runs, 1, "no second run row was created");

    // The two attempts are durable facts.
    for attempt in ["a1", "a2"] {
        let found = state
            .storage()
            .query_one_text(
                "SELECT COUNT(*) FROM run_attempts WHERE run_id = ?1 AND attempt = ?2",
                vec![run_id.clone(), format!("{run_id}#{attempt}")],
            )
            .await
            .unwrap()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap();
        assert_eq!(found, 1, "attempt {attempt} must have its own row");
    }

    // Model call 1 ran under attempt #a1, model call 2 under attempt #a2 —
    // results never cross attempts.
    assert_eq!(
        provider.observed_attempts(),
        vec![format!("{run_id}#a1"), format!("{run_id}#a2")],
        "the driver advanced the attempt identity in place"
    );
    let a1_events = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND attempt = ?2",
            vec![run_id.clone(), format!("{run_id}#a1")],
        )
        .await
        .unwrap()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap();
    let a2_events = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND attempt = ?2",
            vec![run_id.clone(), format!("{run_id}#a2")],
        )
        .await
        .unwrap()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap();
    assert_eq!(
        (a1_events, a2_events),
        (3, 4),
        "a1: start + mc1 started/done; a2: mc2 started/done + terminal + final message"
    );

    teardown(&state, &home).await;
}

#[tokio::test]
async fn non_retryable_failure_settles_without_extra_attempts() {
    let provider = ScriptedProvider::new(vec![ProviderTurn::Failed {
        error: ProtocolError::new(ErrorCode::Forbidden, "model refused", false),
        retryable: false,
    }]);
    let (state, home) = boot(
        "nonretry",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "denied").await;
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(attempts, 1);
    assert!(final_message_content(&state, &run_id).await.is_none());
    teardown(&state, &home).await;
}

#[tokio::test]
async fn retry_budget_exhaustion_fails_loudly_with_attempt_count_honest() {
    // Every turn fails retryably; max_attempts = 2 => one retry, then a
    // loud provider_error (never an infinite retry loop, never a fake
    // success).
    let provider = ScriptedProvider::new(vec![
        ProviderTurn::Failed {
            error: ProtocolError::new(ErrorCode::UpstreamUnavailable, "flaky 1", true),
            retryable: true,
        },
        ProviderTurn::Failed {
            error: ProtocolError::new(ErrorCode::UpstreamUnavailable, "flaky 2", true),
            retryable: true,
        },
        ProviderTurn::Failed {
            error: ProtocolError::new(ErrorCode::UpstreamUnavailable, "flaky 3", true),
            retryable: true,
        },
    ]);
    let (state, home) = boot(
        "retrybudget",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            run_limits: RunDriveLimits {
                max_model_turns: 8,
                max_attempts: 2,
            },
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "always flaky").await;
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(attempts, 2, "attempt budget is the retry bound");
    teardown(&state, &home).await;
}

// ── R03-T01 step 4: outcome contract (no fabricated finals) ────────────────

#[tokio::test]
async fn empty_reply_completes_without_final_and_names_the_cause() {
    let provider = ScriptedProvider::new(vec![ProviderTurn::Empty {
        detail: "zero content blocks".to_string(),
    }]);
    let (state, home) = boot(
        "empty",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "say nothing").await;
    let (status, reason, _) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.no_final.empty_reply"));
    assert!(
        final_message_content(&state, &run_id).await.is_none(),
        "no empty final message may be committed"
    );
    assert_eq!(
        count_events(&state, &run_id, "final_message_committed").await,
        0
    );
    teardown(&state, &home).await;
}

#[tokio::test]
async fn process_only_run_completes_without_final_and_names_the_cause() {
    let provider = ScriptedProvider::new(vec![
        ProviderTurn::ToolRequests {
            content: Vec::new(),
            requests: vec![read_tool_request()],
        },
        ProviderTurn::Empty {
            detail: "nothing more to say".to_string(),
        },
    ]);
    let (state, home) = boot(
        "processonly",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(ToolDouble::succeeding() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "do the thing then go quiet").await;
    let (status, reason, _) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.no_final.process_only"));
    assert!(final_message_content(&state, &run_id).await.is_none());
    teardown(&state, &home).await;
}

#[tokio::test]
async fn tool_partial_failure_has_its_own_outcome_not_a_fabricated_answer() {
    let provider = ScriptedProvider::new(vec![
        ProviderTurn::ToolRequests {
            content: Vec::new(),
            requests: vec![read_tool_request()],
        },
        ProviderTurn::Empty {
            detail: "cannot answer after tool failure".to_string(),
        },
    ]);
    let (state, home) = boot(
        "toolfail",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(ToolDouble::failing() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "use the broken tool").await;
    let (status, reason, _) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        reason.as_deref(),
        Some("completed.no_final.tool_partial_failure")
    );
    assert!(final_message_content(&state, &run_id).await.is_none());
    // The failed tool result is a durable fact with the structured status.
    let failed_result = state
        .storage()
        .query_one_text(
            "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = 'tool_call_completed'",
            vec![run_id],
        )
        .await
        .expect("tool completed event");
    let failed_result = failed_result.expect("tool completed payload");
    assert!(failed_result.contains("\"failed\""), "{failed_result}");
    teardown(&state, &home).await;
}

#[tokio::test]
async fn turn_budget_exhaustion_fails_loudly_never_fake_completes() {
    let provider = ScriptedProvider::new(
        std::iter::repeat_n(
            ProviderTurn::Continue {
                process_note: "still thinking".to_string(),
            },
            16,
        )
        .collect(),
    );
    let (state, home) = boot(
        "budget",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            run_limits: RunDriveLimits {
                max_model_turns: 3,
                max_attempts: 1,
            },
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "keep going forever").await;
    let (status, reason, _) = run_row(&state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.turn_budget_exceeded"));
    assert!(final_message_content(&state, &run_id).await.is_none());
    teardown(&state, &home).await;
}

#[tokio::test]
async fn tool_request_without_executor_is_a_loud_failure() {
    let provider = ScriptedProvider::new(vec![ProviderTurn::ToolRequests {
        content: Vec::new(),
        requests: vec![read_tool_request()],
    }]);
    let (state, home) = boot(
        "notools",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: None,
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "need a tool").await;
    let (status, reason, _) = run_row(&state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.tool_executor_unavailable"));
    teardown(&state, &home).await;
}

/// The production default wiring until R05: NO provider configured. The run
/// completes WITHOUT model content and names the cause explicitly — never a
/// fabricated reply. This is also the R02 compatibility surface: exactly
/// two key events per run (start + terminal), so the R02 regression suites
/// (execute_concurrency 2-events-per-run, persistence read-back) keep their
/// public invariant.
#[tokio::test]
async fn no_provider_configuration_is_explicit_and_r02_compatible() {
    let (state, home) = boot("noprovider", ServiceDeps::default()).await;
    assert!(!state.runs().provider_configured());

    let run_id = execute(&state, "hello").await;
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        reason.as_deref(),
        Some("completed.no_final.no_provider_configured")
    );
    assert_eq!(attempts, 1);
    assert!(final_message_content(&state, &run_id).await.is_none());
    let total: i64 = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            vec![run_id],
        )
        .await
        .unwrap()
        .and_then(|v| v.parse().ok())
        .unwrap();
    assert_eq!(total, 2, "start + terminal: no model content is invented");
    teardown(&state, &home).await;
}

// ── R03-A02 service-level half: the finalize path settles exactly once ─────

/// Duplicate identical settlements replay idempotently and conflicting
/// settlements are diagnosed — through the SAME public finalize path the
/// driver uses (`RunSupervisor::finalize_settlement`). The property-style
/// randomized matrix against the REAL database lives in
/// lingxi-adapters/tests/run_finalize_property.rs (R03-A02).
#[tokio::test]
async fn duplicate_finalize_replays_and_conflicting_finalize_is_diagnosed() {
    let (state, home) = boot("a02svc", ServiceDeps::default()).await;
    let run_id = execute(&state, "settle me").await;

    // Rebuild the run context exactly as the driver holds it.
    let run_id_parsed = RunId::new(run_id.clone());
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: run_id_parsed.clone(),
        attempt: AttemptId::new(format!("{run_id}#a1")),
        generation: 1,
    };
    let supervisor = RunSupervisor::without_provider();

    // Duplicate of the settled outcome (identical status/reason/message):
    // idempotent — the caller gets the finish back, nothing re-settles.
    let events_before: i64 = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .unwrap()
        .and_then(|v| v.parse().ok())
        .unwrap();
    let duplicate = supervisor
        .finalize_settlement(
            state.storage().as_ref(),
            state.events(),
            &ctx,
            lingxi_protocol::RunStatus::Running,
            lingxi_kernel::RunFinish::CompletedWithoutFinal {
                cause: lingxi_kernel::NoFinalCause::NoProviderConfigured,
            },
            1_790_409_600_001,
        )
        .await
        .expect("identical duplicate settles idempotently");
    assert_eq!(
        duplicate.terminal_reason(),
        "completed.no_final.no_provider_configured"
    );
    let events_after: i64 = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            vec![run_id.clone()],
        )
        .await
        .unwrap()
        .and_then(|v| v.parse().ok())
        .unwrap();
    assert_eq!(
        events_before, events_after,
        "an idempotent replay writes nothing new"
    );

    // A conflicting settlement (different outcome) is a loud conflict.
    use lingxi_service::runs::DriveError;
    let conflict = supervisor
        .finalize_settlement(
            state.storage().as_ref(),
            state.events(),
            &ctx,
            lingxi_protocol::RunStatus::Running,
            lingxi_kernel::RunFinish::Failed {
                cause: lingxi_kernel::FailureCause::ProviderFailed {
                    code: "internal".to_string(),
                    retryable: false,
                },
            },
            1_790_409_600_002,
        )
        .await
        .expect_err("conflicting settlement must be rejected");
    match conflict {
        DriveError::Storage(lingxi_kernel::ports::StorageError::Conflict { detail }) => {
            assert!(detail.contains("already terminal"), "{detail}");
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
    // And the terminal state never flipped.
    let (status, reason, _) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        reason.as_deref(),
        Some("completed.no_final.no_provider_configured")
    );
    teardown(&state, &home).await;
}

/// Terminal runs never gain attempts or mid-run events (identity floor of
/// "迟到结果不得跨 attempt 写入"; the full generation fence is R03-T04).
#[tokio::test]
async fn terminal_run_rejects_late_attempts_and_events_loudly() {
    let (state, home) = boot("late", ServiceDeps::default()).await;
    let run_id = execute(&state, "finished").await;
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: RunId::new(run_id.clone()),
        attempt: AttemptId::new(format!("{run_id}#a2")),
        generation: 1,
    };
    use lingxi_kernel::ports::StoragePort as _;
    match state
        .storage()
        .record_attempt_started(&ctx, 1_790_409_600_003)
        .await
    {
        Err(lingxi_kernel::ports::StorageError::Conflict { detail }) => {
            assert!(detail.contains("never gains attempts"), "{detail}");
        }
        other => panic!("late attempt must conflict, got {other:?}"),
    }
    let event = lingxi_kernel::ports::KeyEvent {
        event_id: lingxi_protocol::EventId::new(format!("{run_id}-late")),
        payload: lingxi_protocol::EventPayload::Known(
            lingxi_protocol::KnownEventPayload::ModelCallCompleted(
                lingxi_protocol::ModelCallCompletedPayload {
                    model_call_id: ModelCallId::new(format!("{run_id}-mc9999")),
                    usage: None,
                },
            ),
        ),
    };
    match state
        .storage()
        .record_run_events(&ctx, vec![event], 1_790_409_600_004)
        .await
    {
        Err(lingxi_kernel::ports::StorageError::Conflict { detail }) => {
            assert!(detail.contains("already terminal"), "{detail}");
        }
        other => panic!("late event must conflict, got {other:?}"),
    }
    // Events for an attempt that never started are refused even while the
    // run is still ACTIVE: open a real run through the storage port and
    // leave it un-finalized.
    use lingxi_adapters::storage::RunDatabase;
    let run2 =
        RunDatabase::allocate_run_id(state.storage(), 1_790_409_600_006).expect("allocate run id");
    let ctx_start = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: RunId::new(run2.clone()),
        attempt: lingxi_kernel::attempt_id(&RunId::new(run2.clone()), 1),
        generation: 1,
    };
    state
        .storage()
        .record_run_started(&ctx_start, 1_790_409_600_006)
        .await
        .expect("start a live run");
    let ctx2 = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: RunId::new(run2.clone()),
        attempt: AttemptId::new(format!("{run2}#a7")),
        generation: 1,
    };
    match state
        .storage()
        .record_run_events(
            &ctx2,
            vec![lingxi_kernel::ports::KeyEvent {
                event_id: lingxi_protocol::EventId::new(format!("{run2}-ghost")),
                payload: lingxi_protocol::EventPayload::Known(
                    lingxi_protocol::KnownEventPayload::ModelCallCompleted(
                        lingxi_protocol::ModelCallCompletedPayload {
                            model_call_id: ModelCallId::new(format!("{run2}-mc0001")),
                            usage: None,
                        },
                    ),
                ),
            }],
            1_790_409_600_005,
        )
        .await
    {
        Err(lingxi_kernel::ports::StorageError::Conflict { detail }) => {
            assert!(detail.contains("never opened"), "{detail}");
        }
        other => panic!("ghost attempt must conflict, got {other:?}"),
    }
    teardown(&state, &home).await;
}
