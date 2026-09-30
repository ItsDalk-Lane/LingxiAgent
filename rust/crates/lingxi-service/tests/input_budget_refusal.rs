//! R03 repair G05/F06 integration, case R03-FIX-F06-C03: a submission
//! input over the OFFICIAL budget is refused LOUDLY at admission — an
//! accurate `input_too_large` error naming the byte counts — BEFORE any
//! side effect: no run id is allocated, no run row / `started` write
//! happens, no model call and no tool dispatch occur, and the refusal
//! leaves no id→run binding (a later legal retry under the same id
//! admits fresh — no ghost). The counterpart pin: an input that is only
//! "too long for a LOG summary" (past any historical log bound, far
//! inside the execution budget) must NOT be refused.
//!
//! The official budget is `limits::MAX_SUBMISSION_INPUT_BYTES`, aligned
//! with the transport budget (`DEFAULT_BODY_LIMIT_BYTES`, the 1 MiB
//! HTTP body / WS frame limit the framework already enforces with 413
//! before any handler runs). The service-level check is the loud,
//! admission-time defense-in-depth leg of that same budget — never a
//! silent truncation.
//!
//! This file references `SessionExecuteError::InputTooLarge` and
//! therefore cannot compile against the pre-fix source (no loud refusal
//! existed there — over-budget input was silently executed truncated).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult, ToolExecutorPort,
    ToolOutcome, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};

use lingxi_service::{
    limits, prepare_layout, ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState, SessionExecuteError,
};

const NOW_MS: u64 = 1_790_409_600_000;

// ── the recording double (records actual inputs; fixed replies) ─────────────

struct RecordingProvider {
    inputs: std::sync::Mutex<Vec<String>>,
}

impl RecordingProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inputs: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn call_count(&self) -> usize {
        self.inputs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

impl TurnProviderPort for RecordingProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.g05-budget".to_string(),
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
        self.inputs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(input.to_string());
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "stub-final (fixed; carries no input information)".to_string(),
                        }],
                        model_call_id: None,
                    },
                },
            )
        })
    }
}

struct CountingTool {
    dispatches: std::sync::atomic::AtomicUsize,
}

impl ToolExecutorPort for CountingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a lingxi_protocol::ToolCallId,
        _request: &'a lingxi_kernel::ports::ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.dispatches
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::Success {
                    content_digest: "noop".to_string(),
                },
            )
        })
    }
}

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03-g05-budget-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn config_for(home: &Path) -> ServiceConfig {
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

async fn boot_on(
    home: &Path,
    provider: Arc<RecordingProvider>,
    tool: Arc<CountingTool>,
) -> ServiceState {
    let layout = prepare_layout(home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(tool),
        ..ServiceDeps::default()
    };
    ServiceState::bootstrap_with_deps(config_for(home), &layout, deps)
        .await
        .expect("bootstrap")
}

async fn teardown(state: &ServiceState, home: &Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

async fn query_count(state: &ServiceState, sql: &str, arg: &str) -> usize {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

async fn session_runs(state: &ServiceState, session: &str) -> usize {
    query_count(
        state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        session,
    )
    .await
}

/// Rows that ever reached the running state (a started write): an
/// over-budget refusal must leave none.
async fn started_rows(state: &ServiceState, session: &str) -> usize {
    query_count(
        state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1 AND status != 'queued'",
        session,
    )
    .await
}

async fn submit_fg(
    state: &ServiceState,
    session: &'static str,
    input: &str,
    request_id: Option<&str>,
) -> Result<lingxi_service::ExecuteAccepted, SessionExecuteError> {
    let submission = ExecuteSubmission { input, request_id };
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_submission_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            session,
            &submission,
            NOW_MS,
        )
        .await
}

async fn submit_bg(
    state: &ServiceState,
    session: &'static str,
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

// ── R03-FIX-F06-C03 ──────────────────────────────────────────────────────────

#[tokio::test]
async fn c03_over_budget_refused_loudly_with_zero_side_effects() {
    // Configuration source: the official service-level input budget is
    // the SAME number as the transport body budget the HTTP layer
    // already enforces with 413 (one budget, two enforcement legs).
    assert_eq!(
        limits::MAX_SUBMISSION_INPUT_BYTES,
        limits::DEFAULT_BODY_LIMIT_BYTES,
        "the service-level input budget must stay aligned with the transport body budget"
    );
    let limit = limits::MAX_SUBMISSION_INPUT_BYTES;

    let home = synthetic_home("c03");
    let provider = RecordingProvider::new();
    let tool = Arc::new(CountingTool {
        dispatches: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = boot_on(&home, provider.clone(), tool.clone()).await;

    // Foreground: exactly one byte over the official budget.
    let tail_marker = "|OVER_BUDGET_TAIL_ZZ9";
    let over = format!("{}{tail_marker}", "x".repeat(limit + 1 - tail_marker.len()));
    assert_eq!(over.len(), limit + 1, "setup: exactly one byte over");
    let err = submit_fg(&state, "sess_local_alpha", &over, Some("g05-c03-fg-1"))
        .await
        .expect_err("one byte over the budget must be refused");
    match &err {
        SessionExecuteError::InputTooLarge { bytes, limit_bytes } => {
            assert_eq!(*bytes, limit + 1, "the error names the actual byte count");
            assert_eq!(*limit_bytes, limit, "the error names the budget");
        }
        other => panic!("expected InputTooLarge, got {other:?}"),
    }
    // Zero side effects: no run row at all, no started write, no model
    // call, no tool dispatch.
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 0);
    assert_eq!(started_rows(&state, "sess_local_alpha").await, 0);
    assert_eq!(provider.call_count(), 0);
    assert_eq!(tool.dispatches.load(std::sync::atomic::Ordering::SeqCst), 0);

    // Background: same refusal, same zero side effects.
    let err = submit_bg(&state, "sess_local_alpha", &over, "g05-c03-bg-1")
        .await
        .expect_err("background entry refuses over-budget input identically");
    assert!(matches!(
        err,
        SessionExecuteError::InputTooLarge { bytes, limit_bytes } if bytes == limit + 1 && limit_bytes == limit
    ));
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 0);
    assert_eq!(started_rows(&state, "sess_local_alpha").await, 0);
    assert_eq!(provider.call_count(), 0);
    assert_eq!(tool.dispatches.load(std::sync::atomic::Ordering::SeqCst), 0);

    // The refusal left NO id→run binding: a later LEGAL retry under the
    // very same explicit ids admits fresh (no ghost replay — G04
    // semantics preserved through the new check).
    let legal = format!("{}|LEGAL_TAIL_OK", "y".repeat(4096));
    let accepted = submit_fg(&state, "sess_local_alpha", &legal, Some("g05-c03-fg-1"))
        .await
        .expect("the earlier refusal bound nothing");
    assert!(!accepted.replayed);
    assert_eq!(provider.call_count(), 1, "the legal retry really executed");

    // The reverse direction: the background id also stays fresh.
    let accepted_bg = submit_bg(&state, "sess_local_alpha", &legal, "g05-c03-bg-1")
        .await
        .expect("no ghost from the refused background id");
    assert!(!accepted_bg.replayed);

    // Wait for the background drive to settle so the teardown runs
    // against a settled registry (busy gate honesty).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let status = state
            .storage()
            .query_one_text(
                "SELECT status FROM runs WHERE run_id = ?1",
                vec![accepted_bg.run_id.clone()],
            )
            .await
            .expect("status query");
        if matches!(status.as_deref(), Some("completed")) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "background settle timeout (last status: {status:?})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 2);
    teardown(&state, &home).await;
}

/// The counterpart pin: an input that is only "too long for a LOG
/// summary" (past the historical 2000-char log bound, far inside the
/// execution budget) must be ACCEPTED and executed in full — an
/// over-long log line may never become a refusal of legal input.
#[tokio::test]
async fn c03_log_summary_oversized_legal_input_is_not_misrefused() {
    let home = synthetic_home("c03-log-bound");
    let provider = RecordingProvider::new();
    let tool = Arc::new(CountingTool {
        dispatches: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = boot_on(&home, provider.clone(), tool.clone()).await;

    // 2001 chars: one past the historical log bound; plus a much longer
    // legal request (still a rounding error of the 1 MiB budget).
    for (idx, len) in [2001usize, 100_000].into_iter().enumerate() {
        let input = format!(
            "{}|TAIL_OK_{len}",
            "z".repeat(len - format!("|TAIL_OK_{len}").len())
        );
        let accepted = submit_fg(&state, "sess_local_alpha", &input, None)
            .await
            .unwrap_or_else(|err| panic!("len {len} is legal and must be admitted, got {err:?}"));
        assert!(!accepted.replayed);
        assert_eq!(
            provider.call_count(),
            idx + 1,
            "each legal submission executed exactly once"
        );
    }
    // Exactly one durable run per accepted submission — nothing was
    // silently dropped or coalesced.
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 2);
    assert_eq!(tool.dispatches.load(std::sync::atomic::Ordering::SeqCst), 0);
    teardown(&state, &home).await;
}
