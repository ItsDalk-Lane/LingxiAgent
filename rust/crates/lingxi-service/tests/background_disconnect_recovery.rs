//! R03-A12 integration｜断线不等于取消 — through the REAL service
//! composition (real storage, real events, real kernel state machine +
//! single finalize, real session gate + requestId dedup, real task
//! supervisor), driven by deterministic doubles that ONLY produce
//! external responses.
//!
//! Scenario (taskbook R03-A12):
//! 1. a task is submitted under the BACKGROUND policy
//!    (`execute_background_for`): admission is the exact foreground chain
//!    (ownership → busy gate → requestId dedup), then the drive is
//!    spawned as a DETACHED supervised task of the SAME run supervisor —
//!    its lifetime is decoupled from the caller's connection;
//! 2. the client "disconnects" (the submission call has already returned;
//!    the drive task is structurally owned by NO caller — asserted
//!    against the task registry — and the contrast arm below shows a
//!    DROPPED foreground drive dying while the background one lives);
//! 3. the client reconnects: re-submitting the SAME requestId with the
//!    SAME content is an idempotent REPLAY (the original run id returns,
//!    nothing re-executed — 不重复提交); the run continued under its
//!    original policy and is queryable (durable status + terminal event
//!    + the events page surface);
//! 4. the same id with CHANGED content is the loud T04 conflict;
//! 5. the minimal service-exit hook: the background registry's bounded
//!    drain reports live drives as unconfirmed (never a fake quiet).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult, ToolExecutorPort,
    ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::{
    prepare_layout, ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState, SessionExecuteError,
};

const BG_INPUT: &str = "BG-A12: long-running background work";
const NOW_MS: u64 = 1_790_409_600_000;

// ── deterministic doubles ────────────────────────────────────────────────────

struct ScriptedProvider {
    scripts: std::sync::Mutex<HashMap<String, VecDeque<ProviderTurn>>>,
}

impl ScriptedProvider {
    fn new(scripts: Vec<(&'static str, Vec<ProviderTurn>)>) -> Arc<Self> {
        Arc::new(Self {
            scripts: std::sync::Mutex::new(
                scripts
                    .into_iter()
                    .map(|(marker, script)| (marker.to_string(), script.into_iter().collect()))
                    .collect(),
            ),
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
        let ctx_at_issue = ctx.clone();
        let turn = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&marker)
            .and_then(|queue| queue.pop_front())
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

/// Tool double that parks on a 0-permit semaphore until released (the
/// long external I/O the background run waits through).
struct ParkingTool {
    gate: Arc<tokio::sync::Semaphore>,
    arrivals: tokio::sync::mpsc::UnboundedSender<String>,
}

impl ToolExecutorPort for ParkingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.arrivals
            .send(format!("{}|{}", ctx.run_id, request.target))
            .expect("test holds the arrivals receiver");
        let ctx_at_issue = ctx.clone();
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            let _permit = gate.acquire().await.expect("tool gate closed");
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::success_text("parked-then-success".to_string()),
            )
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
        "lingxi-r03t06-a12-{tag}-{}-{}",
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
    provider: Arc<ScriptedProvider>,
    tool: Arc<ParkingTool>,
) -> (ServiceState, PathBuf) {
    let home = synthetic_home(tag);
    let layout = prepare_layout(&home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(tool),
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

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
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

// ── R03-A12: the acceptance scenario ─────────────────────────────────────────

/// 任务按后台策略运行；断开并重连客户端 → 不重复提交；任务按原策略继续
/// 且可查询；前台断连的对照组证明差异（断线≠取消 仅对后台策略成立）。
#[tokio::test]
async fn r03_a12_background_run_survives_disconnect_reconnect_replays_and_is_queryable() {
    let (arrival_tx, mut arrival_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tool_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(vec![(
        "BG-A12",
        vec![
            ProviderTurn::ToolRequests {
                requests: vec![read_tool_request()],
            },
            final_turn("background work finished"),
        ],
    )]);
    let tool = Arc::new(ParkingTool {
        gate: Arc::clone(&tool_gate),
        arrivals: arrival_tx,
    });
    let (state, home) = boot("main", provider, tool).await;

    // 1) Background submission with an explicit requestId: accepted
    //    IMMEDIATELY (the drive is detached).
    let accepted = submit_background(&state, "sess_local_alpha", BG_INPUT, "reconn-1")
        .await
        .expect("background admission");
    assert!(!accepted.replayed, "fresh submission");
    let run_id = accepted.run_id.clone();

    // 2) The run is live and its drive is owned by NO caller: a detached
    //    supervised task of the SAME supervisor (owner None), plus the
    //    registry entry — the "client connection" cannot reach it.
    wait_until("the background drive reaches its tool I/O", || {
        arrival_rx
            .try_recv()
            .map(|arrival| arrival.starts_with(&run_id))
            .unwrap_or(false)
    })
    .await;
    let detached_owned = state
        .runs()
        .task_supervisor()
        .tasks()
        .into_iter()
        .any(|task| task.run_id.is_none() && task.label == format!("background_drive:{run_id}"));
    assert!(
        detached_owned,
        "the background drive is a DETACHED supervised task (no run/client owns it)"
    );
    assert_eq!(state.background().live_ids(), vec![run_id.clone()]);

    // The run keeps its original policy while "disconnected" (no caller
    // exists at all): status is durably running mid-flight.
    let status = query_text(&state, "SELECT status FROM runs WHERE run_id = ?1", &run_id)
        .await
        .expect("the background run row exists");
    assert_eq!(status, "running");

    // 3) Reconnect: the SAME requestId with the SAME content is an
    //    idempotent REPLAY — the original run id, nothing re-executed.
    let replay = submit_background(&state, "sess_local_alpha", BG_INPUT, "reconn-1")
        .await
        .expect("reconnect submission");
    assert!(replay.replayed, "the reconnect replays idempotently");
    assert_eq!(replay.run_id, run_id, "the original run id returns");
    let run_count = query_text(
        &state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        "sess_local_alpha",
    )
    .await
    .expect("count query");
    assert_eq!(run_count, "1", "不重复提交: no second run was created");
    // The parked tool saw exactly ONE arrival (no re-execution).
    assert!(arrival_rx.try_recv().is_err(), "no duplicate tool dispatch");

    // 4) The same id with CHANGED content is the loud T04 conflict.
    match submit_background(&state, "sess_local_alpha", "CHANGED content", "reconn-1").await {
        Err(SessionExecuteError::DuplicateRequestConflict { request_id, .. }) => {
            assert_eq!(request_id, "reconn-1");
        }
        other => panic!("expected DuplicateRequestConflict, got {other:?}"),
    }

    // 5) The run continues under its ORIGINAL policy: release the parked
    //    tool and the SAME drive settles through the single finalize.
    tool_gate.add_permits(1);
    wait_until("the background run settles", || {
        state.background().live_ids().is_empty()
    })
    .await;
    let (status, reason) = (
        query_text(&state, "SELECT status FROM runs WHERE run_id = ?1", &run_id).await,
        query_text(
            &state,
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            &run_id,
        )
        .await,
    );
    assert_eq!(status.as_deref(), Some("completed"));
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    // 6) Queryable: the durable terminal event exists AND the events page
    //    surface serves the run's stream to the reconnecting client.
    let terminal_event = query_text(
        &state,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
         'run_state_changed' AND payload_json LIKE '%completed.with_final%'",
        &run_id,
    )
    .await
    .expect("the terminal event is durable");
    assert!(terminal_event.contains("completed"));
    let page = state
        .events()
        .events_page(&owner_principal(), "sess_local_alpha", None, Some(50))
        .await
        .expect("events page query");
    assert!(
        page.events
            .iter()
            .any(|envelope| envelope.event_type == "run_state_changed"),
        "the events page serves the settled run's stream"
    );

    teardown(&state, &home).await;
}

// ── companion: the foreground contrast (a DROPPED caller dies) ───────────────

/// The DIFFERENCE the taskbook asks to define: a FOREGROUND submission
/// whose driving future disappears (the disconnect shape) ends as an
/// honestly abandoned active row — while the background policy (above)
/// survives the same event. 断线≠取消 is a property of the background
/// submission policy, not of every run.
#[tokio::test]
async fn foreground_drive_disappearing_is_abandoned_not_cancelled() {
    let (arrival_tx, mut arrival_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tool_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(vec![(
        "FG-A12",
        vec![
            ProviderTurn::ToolRequests {
                requests: vec![read_tool_request()],
            },
            final_turn("never reached"),
        ],
    )]);
    let tool = Arc::new(ParkingTool {
        gate: Arc::clone(&tool_gate),
        arrivals: arrival_tx,
    });
    let (state, home) = boot("foreground", provider, tool).await;

    // A foreground submission in its OWN task (the request future).
    let state_for_task = state.clone();
    let foreground = tokio::spawn(async move {
        let storage = Arc::clone(state_for_task.storage());
        state_for_task
            .sessions()
            .execute_for(
                storage.as_ref(),
                state_for_task.events(),
                state_for_task.runs(),
                &owner_principal(),
                "sess_local_beta",
                "FG-A12: foreground run",
                NOW_MS,
            )
            .await
    });
    // The drive reaches its parked tool I/O…
    wait_until("the foreground drive reaches its tool I/O", || {
        arrival_rx
            .try_recv()
            .map(|arrival| arrival.contains("read"))
            .unwrap_or(false)
    })
    .await;
    // …then the client connection DROPS (the request future vanishes).
    foreground.abort();
    wait_until("the abandoned drive is registered", || {
        state
            .runs()
            .task_supervisor()
            .tasks()
            .iter()
            .any(|task| task.exit.is_some())
    })
    .await;
    // The durable row stays honestly ACTIVE (no fabricated terminal);
    // the abandoned verdict is queryable through the cancellation
    // registry — the recovery classification is R03-T07's.
    let run_id = query_text(
        &state,
        "SELECT run_id FROM runs WHERE session_id = ?1",
        "sess_local_beta",
    )
    .await
    .expect("the foreground run row exists");
    let status = query_text(&state, "SELECT status FROM runs WHERE run_id = ?1", &run_id).await;
    assert_eq!(
        status.as_deref(),
        Some("running"),
        "a dropped foreground drive leaves an honestly active row (no fabricated terminal)"
    );
    let verdict = state.runs().cancel_registry().recent_verdict(&run_id);
    assert!(
        matches!(verdict, Some(lingxi_service::CancelPhase::Abandoned { .. })),
        "the abandoned verdict is queryable: {verdict:?}"
    );

    // Release the parked tool so the aborted child future can finish and
    // the fixture tears down cleanly.
    tool_gate.add_permits(1);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    teardown(&state, &home).await;
}

// ── companion: the minimal exit hook reports live drives honestly ───────────

/// The service-exit difference: the background registry's bounded drain
/// reports a still-running drive as UNCONFIRMED (never a fake quiet);
/// the full per-task exit policy is R03-T07.
#[tokio::test]
async fn exit_hook_drain_reports_live_drives_as_unconfirmed() {
    let (arrival_tx, mut arrival_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tool_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(vec![(
        "BG-A12",
        vec![
            ProviderTurn::ToolRequests {
                requests: vec![read_tool_request()],
            },
            final_turn("settled after the drain window"),
        ],
    )]);
    let tool = Arc::new(ParkingTool {
        gate: Arc::clone(&tool_gate),
        arrivals: arrival_tx,
    });
    let (state, home) = boot("exit-hook", provider, tool).await;

    let accepted = submit_background(&state, "sess_local_alpha", BG_INPUT, "exit-1")
        .await
        .expect("background admission");
    let run_id = accepted.run_id.clone();
    wait_until("the background drive reaches its tool I/O", || {
        arrival_rx
            .try_recv()
            .map(|arrival| arrival.starts_with(&run_id))
            .unwrap_or(false)
    })
    .await;

    // The exit hook under a tiny budget: the live drive is reported
    // unconfirmed (the process exit would bound it; recovery is T07).
    let report = state
        .background()
        .drain_within(std::time::Duration::from_millis(30))
        .await;
    assert_eq!(report.confirmed, Vec::<String>::new());
    assert_eq!(report.unconfirmed, vec![run_id.clone()]);

    // The reported drive was NOT cancelled by the report: it keeps
    // running under its original policy and settles when released. (The
    // registry map was emptied by the drain — observe the supervised
    // task's own exit instead.)
    tool_gate.add_permits(1);
    let drive_label = format!("background_drive:{run_id}");
    wait_until("the background run settles after the drain window", || {
        state
            .runs()
            .task_supervisor()
            .tasks()
            .iter()
            .any(|task| task.label == drive_label && task.exit.is_some())
    })
    .await;
    let status = query_text(&state, "SELECT status FROM runs WHERE run_id = ?1", &run_id).await;
    assert_eq!(status.as_deref(), Some("completed"));
    teardown(&state, &home).await;
}
