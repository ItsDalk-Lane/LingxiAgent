//! R05-T05 service-level evidence: the run driver's bounded retry
//! discipline — total call budget, Retry-After override, cancel-aware
//! backoff, attempt budget — driven through the REAL service composition
//! (real storage, real event service, real quota manager, real
//! cancellation tree) with a deterministic provider DOUBLE at the
//! `TurnProviderPort` seam (the double only produces external responses;
//! every state decision belongs to the supervisor).
//!
//! Coverage map (appendix-B R05-T05 checkpoints):
//! - C03 (bounded 429/5xx): a 429's `Retry-After` hint (carried in the
//!   error details by the dispatch layer) OVERRIDES the computed backoff;
//!   without a hint the computed `base × 2^(n-1)` capped schedule applies
//!   (pinned as a pure function).
//! - C05/C07 (no blind resend): a retry whose delay cannot finish inside
//!   the call's shared total budget is VETOED — the run settles as the
//!   original failure, never as a sleep into a guaranteed expiry; the
//!   default attempt budget is the pre-registered 3.
//! - C08 (cancel covers the backoff wait): a cancellation accepted while
//!   the driver sleeps between attempts settles NOW through the
//!   four-phase path, never after the remaining delay.
//! - Concurrency honesty: the model-call quota permit is NOT held across
//!   the backoff sleep — a second run in the SAME session lane is
//!   admitted while the first run backs off.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError};
use lingxi_service::runs::ModelCallTuning;
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, RunDriveLimits, ServiceConfig, ServiceDeps,
    ServiceState,
};

// ── deterministic provider double (test-only; never in production wiring) ───

/// Pops turns from a fixed script; records (attempt, unix-ms) of every
/// call it was asked under. An exhausted script fails loudly.
struct ScriptedProvider {
    script: std::sync::Mutex<VecDeque<ProviderTurn>>,
    observed: std::sync::Mutex<Vec<(String, u64)>>,
    priors: std::sync::Mutex<Vec<Vec<String>>>,
}

impl ScriptedProvider {
    fn new(script: Vec<ProviderTurn>) -> Arc<Self> {
        Arc::new(Self {
            script: std::sync::Mutex::new(script.into_iter().collect()),
            observed: std::sync::Mutex::new(Vec::new()),
            priors: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn observed(&self) -> Vec<(String, u64)> {
        self.observed.lock().unwrap().clone()
    }

    fn call_count(&self) -> usize {
        self.observed.lock().unwrap().len()
    }

    /// Per call, the shape of the prior exchange it was sent (C07: the
    /// retried call must carry the confirmed tool result).
    fn priors(&self) -> Vec<Vec<String>> {
        self.priors.lock().unwrap().clone()
    }
}

fn describe_prior(input: &ModelTurnInput) -> Vec<String> {
    input
        .prior
        .iter()
        .map(|item| match item {
            lingxi_kernel::model_exchange::ExchangeItem::AssistantTurn { .. } => {
                "assistant".to_string()
            }
            lingxi_kernel::model_exchange::ExchangeItem::ToolResult {
                tool_call_id,
                outcome,
                ..
            } => {
                let detail = match outcome {
                    lingxi_kernel::ports::ToolOutcome::Success { result } => {
                        format!("success:{}", result.content.len())
                    }
                    lingxi_kernel::ports::ToolOutcome::Failed { error } => {
                        format!("failed:{}", error.code.wire_name())
                    }
                    lingxi_kernel::ports::ToolOutcome::Cancelled => "cancelled".to_string(),
                    lingxi_kernel::ports::ToolOutcome::Unknown { reason } => {
                        format!("unknown:{reason}")
                    }
                };
                format!("tool_result:{tool_call_id}:{detail}")
            }
        })
        .collect()
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
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
        _call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.observed
            .lock()
            .unwrap()
            .push((ctx.attempt.to_string(), unix_ms()));
        self.priors.lock().unwrap().push(describe_prior(input));
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

fn retryable_upstream(detail: &str) -> ProviderTurn {
    ProviderTurn::Failed {
        error: ProtocolError::new(ErrorCode::UpstreamUnavailable, detail.to_string(), true),
        retryable: true,
    }
}

/// A 429-shaped failure as the dispatch layer hands it to the driver:
/// `budget_exceeded`, retryable, the `Retry-After` hint in the details.
fn rate_limited(retry_after_ms: u64) -> ProviderTurn {
    let error = ProtocolError::new(
        ErrorCode::BudgetExceeded,
        "provider returned HTTP 429 Too Many Requests".to_string(),
        true,
    )
    .with_details(serde_json::Map::from_iter([(
        lingxi_adapters::models::dispatch::RETRY_AFTER_MS_DETAIL.to_string(),
        serde_json::Value::from(retry_after_ms),
    )]));
    ProviderTurn::Failed {
        error,
        retryable: true,
    }
}

fn final_text(text: &str) -> ProviderTurn {
    ProviderTurn::Final {
        message: NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            model_call_id: Some(ModelCallId::new("mc-final")),
        },
    }
}

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t05-{tag}-{}-{}",
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

async fn execute_on(state: ServiceState, session: &'static str, input: &'static str) -> String {
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
        .expect("execute accepted")
        .run_id
}

async fn execute(state: &ServiceState, input: &'static str) -> String {
    execute_on(state.clone(), "sess_local_alpha", input).await
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

fn tuning(total_budget_ms: u64, backoff_base_ms: u64, backoff_cap_ms: u64) -> ModelCallTuning {
    ModelCallTuning {
        total_budget: Duration::from_millis(total_budget_ms),
        retry_backoff_base: Duration::from_millis(backoff_base_ms),
        retry_backoff_cap: Duration::from_millis(backoff_cap_ms),
    }
}

// ── C03: the bounded-retry schedule ─────────────────────────────────────────

#[test]
fn computed_backoff_doubles_from_the_base_to_the_cap() {
    let schedule = tuning(300_000, 500, 8_000);
    let delays: Vec<u64> = (1..=6)
        .map(|failed| schedule.backoff_delay(failed).as_millis() as u64)
        .collect();
    assert_eq!(
        delays,
        vec![500, 1_000, 2_000, 4_000, 8_000, 8_000],
        "base × 2^(failed_attempt-1), capped"
    );
    // The pre-registered defaults (R05_BASELINE §8).
    let defaults = ModelCallTuning::default();
    assert_eq!(defaults.total_budget.as_millis(), 300_000);
    assert_eq!(defaults.retry_backoff_base.as_millis(), 500);
    assert_eq!(defaults.retry_backoff_cap.as_millis(), 8_000);
    defaults.validate().expect("the defaults are valid");
    // Degenerate injections are loud, never clamped.
    assert!(tuning(0, 500, 8_000).validate().is_err());
    assert!(tuning(3_600_001, 500, 8_000).validate().is_err());
    assert!(tuning(300_000, 9_000, 8_000).validate().is_err());
    assert!(tuning(300_000, 500, 300_001).validate().is_err());
}

#[test]
fn the_default_attempt_budget_is_the_preregistered_three() {
    assert_eq!(RunDriveLimits::DEFAULT_MAX_ATTEMPTS, 3);
}

#[tokio::test]
async fn retry_after_hint_overrides_the_computed_backoff() {
    // Computed backoff would be 30 s; the provider's 400 ms hint wins.
    let provider = ScriptedProvider::new(vec![
        rate_limited(400),
        final_text("recovered after the hinted wait"),
    ]);
    let (state, home) = boot(
        "retry-after",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            model_call_tuning: tuning(300_000, 30_000, 30_000),
            ..ServiceDeps::default()
        },
    )
    .await;
    let started = std::time::Instant::now();
    let run_id = execute(&state, "rate limited once").await;
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(attempts, 2, "the hinted retry reopened attempt #a2");
    let observed = provider.observed();
    assert_eq!(observed.len(), 2);
    let gap = observed[1].1 - observed[0].1;
    assert!(
        gap >= 400,
        "the Retry-After hint floors the wait, got {gap} ms"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the 30 s computed backoff did NOT apply (the hint overrode it)"
    );
    teardown(&state, &home).await;
}

#[tokio::test]
async fn computed_backoff_applies_without_a_hint() {
    // No detail on the error: the 700 ms base schedule applies (floored —
    // timing floors are the assertion, never exact sleeps).
    let provider = ScriptedProvider::new(vec![
        retryable_upstream("flaky, no hint"),
        final_text("recovered on the computed schedule"),
    ]);
    let (state, home) = boot(
        "computed-backoff",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            model_call_tuning: tuning(300_000, 700, 8_000),
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "flaky once").await;
    let (status, _, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(attempts, 2);
    let observed = provider.observed();
    let gap = observed[1].1 - observed[0].1;
    assert!(
        gap >= 700,
        "the computed base delay floors the retry, got {gap} ms"
    );
    teardown(&state, &home).await;
}

// ── C05/C07: the budget veto (no blind resend) ──────────────────────────────

#[tokio::test]
async fn a_retry_that_cannot_fit_the_call_budget_is_vetoed_not_slept_into() {
    // Total budget 1.5 s, first backoff 10 s: the retry could only start
    // already spent. The run must settle as the ORIGINAL failure after ONE
    // attempt — never sleep 10 s into a guaranteed expiry.
    let provider = ScriptedProvider::new(vec![
        retryable_upstream("flaky under a tight budget"),
        final_text("must never be reached"),
    ]);
    let (state, home) = boot(
        "budget-veto",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            model_call_tuning: tuning(1_500, 10_000, 10_000),
            ..ServiceDeps::default()
        },
    )
    .await;
    let started = std::time::Instant::now();
    let run_id = execute(&state, "tight budget").await;
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "failed");
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        attempts, 1,
        "the veto settled on the original failure — no blind retry"
    );
    assert_eq!(
        provider.call_count(),
        1,
        "the vetoed retry never reached the provider"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the run did NOT sleep the vetoed 10 s backoff"
    );
    teardown(&state, &home).await;
}

// ── C08: cancellation covers the backoff wait ────────────────────────────────

#[tokio::test]
async fn cancellation_during_backoff_settles_now_not_after_the_delay() {
    // First attempt fails retryably; the backoff is 60 s. The cancellation
    // lands mid-backoff and must settle the run NOW.
    let provider = ScriptedProvider::new(vec![
        retryable_upstream("flaky, then a long backoff"),
        final_text("must never be reached"),
    ]);
    let (state, home) = boot(
        "cancel-backoff",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            model_call_tuning: tuning(300_000, 60_000, 60_000),
            ..ServiceDeps::default()
        },
    )
    .await;
    let drive_state = state.clone();
    let drive =
        tokio::spawn(
            async move { execute_on(drive_state, "sess_local_alpha", "slow retry").await },
        );
    // Wait until the first attempt actually failed (the driver is now in
    // the 60 s backoff sleep).
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while provider.call_count() < 1 {
        assert!(
            std::time::Instant::now() < deadline,
            "attempt one never ran"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await; // settle into the sleep arm
    let run_id = {
        // The run id is observable from the provider's attempt records.
        let attempt = &provider.observed()[0].0;
        attempt
            .split('#')
            .next()
            .expect("run id prefix")
            .to_string()
    };
    let cancel_started = std::time::Instant::now();
    let outcome = state
        .sessions()
        .cancel_run_for(
            state.storage().as_ref(),
            state.runs(),
            &owner_principal(),
            &run_id,
        )
        .await
        .expect("cancel surface");
    assert!(
        matches!(outcome, lingxi_service::CancelRunOutcome::Accepted { .. }),
        "the live run accepted the cancellation, got {outcome:?}"
    );
    let driven = tokio::time::timeout(Duration::from_secs(10), drive)
        .await
        .expect("the run settled NOW, not after the 60 s backoff")
        .expect("drive task");
    assert_eq!(driven, run_id);
    assert!(
        cancel_started.elapsed() < Duration::from_secs(5),
        "cancel-during-backoff settled promptly"
    );
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    assert_eq!(attempts, 1, "no retry attempt opened after the cancel");
    assert_eq!(provider.call_count(), 1, "the provider was never re-called");
    teardown(&state, &home).await;
}

// ── concurrency honesty: the backoff sleep holds no model permit ─────────────

#[tokio::test]
async fn the_backoff_sleep_holds_no_model_call_permit() {
    // The GLOBAL model lane admits ONE concurrent call. Run A fails
    // retryably and backs off 2.5 s; run B (a different session — the
    // session busy gate is a separate contract) must be admitted and
    // complete while A sleeps: the permit is released with the failed
    // call's I/O, never held across the backoff.
    let provider = ScriptedProvider::new(vec![
        retryable_upstream("A flaky once"),
        final_text("first success"),
        final_text("second success"),
    ]);
    let (state, home) = boot(
        "permit-backoff",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            model_call_tuning: tuning(300_000, 2_500, 2_500),
            quota_limits: lingxi_service::quotas::QuotaLimits {
                model: lingxi_service::quotas::LayeredQuotaLimits {
                    global: 1,
                    per_agent: 4,
                    per_session: 1,
                },
                ..lingxi_service::quotas::QuotaLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await;
    let drive_state = state.clone();
    let drive_a =
        tokio::spawn(async move { execute_on(drive_state, "sess_local_alpha", "run A").await });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while provider.call_count() < 1 {
        assert!(std::time::Instant::now() < deadline, "run A never started");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // A is now inside its 2.5 s backoff (its first call just failed).
    let a_first_at = provider.observed()[0].1;
    let run_b = tokio::time::timeout(
        Duration::from_secs(10),
        execute_on(state.clone(), "sess_local_beta", "run B"),
    )
    .await
    .expect("run B settled (admitted during A's backoff — no permit held across the sleep)");
    let run_a = tokio::time::timeout(Duration::from_secs(15), drive_a)
        .await
        .expect("run A settled")
        .expect("drive A");
    // Attribute the provider calls by run-id prefix of the attempt ids.
    let observed = provider.observed();
    let a_calls: Vec<&(String, u64)> = observed
        .iter()
        .filter(|(attempt, _)| attempt.starts_with(&run_a))
        .collect();
    let b_calls: Vec<&(String, u64)> = observed
        .iter()
        .filter(|(attempt, _)| attempt.starts_with(&run_b))
        .collect();
    assert_eq!(a_calls.len(), 2, "A failed once and retried: {observed:?}");
    assert_eq!(b_calls.len(), 1, "B completed on one call: {observed:?}");
    assert!(
        b_calls[0].1 < a_first_at + 2_500,
        "B's model call started while A's backoff was still running"
    );
    let (status_a, _, attempts_a) = run_row(&state, &run_a).await;
    let (status_b, _, _) = run_row(&state, &run_b).await;
    assert_eq!(status_a, "completed");
    assert_eq!(attempts_a, 2);
    assert_eq!(status_b, "completed");
    teardown(&state, &home).await;
}

// ── C07: a retry after a confirmed tool side effect never re-executes it ─────

/// Counting tool double: every execution is recorded (the side-effect
/// counter of the C07 proof).
struct CountingTool {
    calls: std::sync::Mutex<Vec<String>>,
}

impl lingxi_kernel::ports::ToolExecutorPort for CountingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a lingxi_protocol::ToolCallId,
        request: &'a lingxi_kernel::ports::ToolRequest,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = lingxi_kernel::ports::ToolExecutionResult> + Send + 'a,
        >,
    > {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:{}", ctx.attempt, request.target));
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            lingxi_kernel::ports::ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                lingxi_kernel::ports::ToolOutcome::success_text(
                    "confirmed-write-content".to_string(),
                ),
            )
        })
    }
}

/// C07: turn 1 requests a tool, the tool's (counted, confirmed) side effect
/// lands, turn 2's model call fails retryably. The retry is a NEW ATTEMPT on
/// the same run that MUST: (a) not re-execute the confirmed tool (the
/// counter stays at 1), (b) receive the confirmed tool result in its prior
/// exchange (the continuation is honest history, never a rebuilt-bare
/// submission), and (c) complete cleanly.
#[tokio::test]
async fn a_retry_after_a_confirmed_tool_effect_never_re_executes_it() {
    let tool_request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        "write",
        serde_json::json!({"path": "/tmp/c07", "content": "x"}),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request");
    let provider = ScriptedProvider::new(vec![
        ProviderTurn::ToolRequests {
            content: Vec::new(),
            requests: vec![tool_request],
        },
        retryable_upstream("the post-tool model call failed"),
        final_text("done after the clean retry"),
    ]);
    let tool = Arc::new(CountingTool {
        calls: std::sync::Mutex::new(Vec::new()),
    });
    let (state, home) = boot(
        "c07-retry",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tool.clone() as Arc<dyn lingxi_kernel::ports::ToolExecutorPort>),
            model_call_tuning: tuning(300_000, 200, 8_000),
            ..ServiceDeps::default()
        },
    )
    .await;
    let run_id = execute(&state, "write then continue").await;
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert_eq!(attempts, 2, "the retry opened attempt #a2 on the same run");
    // (a) the confirmed side effect ran EXACTLY once, on attempt #a1.
    let calls = tool.calls.lock().unwrap().clone();
    assert_eq!(
        calls.as_slice(),
        &[format!("{run_id}#a1:write")],
        "the confirmed tool was never re-executed by the retry"
    );
    // (b) the retried call (attempt #a2) received the confirmed tool
    // result in its prior exchange — the continuation carries the real
    // outcome, never a guess.
    let priors = provider.priors();
    assert_eq!(priors.len(), 3, "tool turn + failed turn + retried turn");
    assert!(
        priors[2]
            .iter()
            .any(|item| item.contains("tool_result:") && item.contains("success:1")),
        "the a2 retry carried the confirmed tool result: {:?}",
        priors[2]
    );
    assert!(
        priors[2].iter().any(|item| item == "assistant"),
        "the a2 retry carried the assistant tool-request turn: {:?}",
        priors[2]
    );
    teardown(&state, &home).await;
}

// ── C01: the idle-stream bound, pinned through the injection seam (F-03) ─────

/// A provider double that emits ONE text delta per call and then stalls
/// FOREVER — it ignores every deadline and never produces a terminal; the
/// driver's idle arm is the only exit.
struct StallAfterFirstDeltaProvider {
    observed: std::sync::Mutex<Vec<(String, u64)>>,
}

impl StallAfterFirstDeltaProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            observed: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn observed(&self) -> Vec<(String, u64)> {
        self.observed.lock().unwrap().clone()
    }
}

impl TurnProviderPort for StallAfterFirstDeltaProvider {
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
        _call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.observed
            .lock()
            .unwrap()
            .push((ctx.attempt.to_string(), unix_ms()));
        Box::pin(async move {
            let _ = deltas
                .emit(lingxi_kernel::ports::ModelTurnDelta::Text(
                    "partial-before-the-stall".to_string(),
                ))
                .await;
            // Stalled forever: no further delta, no terminal. A driver
            // without the idle arm would park here indefinitely (the
            // reviewer's idle-probe proved the production 60 s signature
            // the slow way: 181.5 s / 3 attempts).
            std::future::pending::<ProviderTurnResult>().await
        })
    }
}

async fn count_events(state: &ServiceState, run_id: &str, event_type: &str) -> i64 {
    state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = ?2",
            vec![run_id.to_string(), event_type.to_string()],
        )
        .await
        .expect("count events")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1)
}

/// F-03 (REVIEW-T05 R01): the production 60 s idle-stream bound
/// (`DEFAULT_STREAM_IDLE_TIMEOUT_MS`) previously had NO fast executed
/// evidence. Through the `with_stream_idle_timeout` seam the same arm is
/// pinned in seconds: a stream that delivers one delta and then goes silent
/// is cut at the INJECTED 200 ms bound, the stalled call settles retryable
/// (the driver's idle-arm classification), the bounded attempt policy runs
/// its pre-registered course (3 attempts), and the run settles honestly
/// failed — with every attempt's partial delta durable (no rewind).
#[tokio::test]
async fn an_idle_stream_is_cut_at_the_injected_bound_and_settles_honestly() {
    let provider = StallAfterFirstDeltaProvider::new();
    let (state, home) = boot(
        "idle-stream",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            model_call_tuning: tuning(300_000, 100, 100),
            stream_idle_timeout: Some(Duration::from_millis(200)),
            ..ServiceDeps::default()
        },
    )
    .await;
    let started = std::time::Instant::now();
    let run_id = execute(&state, "one delta then silence").await;
    let (status, reason, attempts) = run_row(&state, &run_id).await;
    assert_eq!(
        status, "failed",
        "a permanently stalled stream never completes"
    );
    assert_eq!(reason.as_deref(), Some("failed.provider_error"));
    assert_eq!(
        attempts, 3,
        "the retryable idle-stall ran the pre-registered attempt budget"
    );
    let observed = provider.observed();
    assert_eq!(observed.len(), 3, "each attempt reached the provider once");
    for pair in observed.windows(2) {
        let gap = pair[1].1 - pair[0].1;
        assert!(
            gap >= 300,
            "each retry paid the 200 ms idle bound + the 100 ms backoff, got {gap} ms"
        );
    }
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the injected 200 ms bound fired, not the production 60 s one"
    );
    assert!(
        count_events(&state, &run_id, "model_call_delta").await >= 3,
        "every attempt's partial delta stayed durable (no rewind)"
    );
    teardown(&state, &home).await;
}
