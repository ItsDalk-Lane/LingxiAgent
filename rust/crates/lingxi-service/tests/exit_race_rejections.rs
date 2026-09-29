//! R03-T07 permanent tests — R03-A14 退出不接收新任务 (the exit-window
//! race), through the REAL composition root and (for the transport half)
//! the REAL HTTP surface.
//!
//! Scenario shape (the acceptance's own words):
//! - 前置: shutdown 已开始 — the test closes the process's submission
//!   intake through the SAME one-way gate the serving binary's signal
//!   future closes at signal time (`ServiceState::submission_intake`),
//!   with an EXISTING background task parked at its tool I/O (an already
//!   admitted run the exit must settle by policy, not drop).
//! - 操作: 并发提交新任务 — a concurrent wave of fresh submissions on
//!   both surfaces (foreground service-level, background spawn surface,
//!   and — in the transport test — real HTTP POSTs) while the exit
//!   proceeds.
//! - 通过条件: every fresh submission is REFUSED; the existing task
//!   settles through its own single finalize (cancelled via the exit's
//!   task-type cancellation) within the bounded budget; the residue
//!   report is honest.
//!
//! Test doubles: the provider/tool doubles only produce external responses
//! and park (the T05 double boundary); the intake gate, admission chain,
//! cancel tree, drain budget and finalize belong to the real service.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurnResult, ToolExecutionResult, ToolExecutorPort, ToolOutcome,
    ToolRequest, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::shutdown::{graceful_shutdown, ShutdownBudget, WsShutdown};
use lingxi_service::{
    prepare_layout, run, ExecuteSubmission, ServiceDeps, ServiceState, SessionExecuteError,
};

#[path = "recovery_support/harness.rs"]
mod harness;

use harness::{config_for, owner_principal, synthetic_home};

// ── the doubles (response production + parking only) ─────────────────────────

struct OneToolThenFinalProvider;

impl TurnProviderPort for OneToolThenFinalProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.exit-race".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        turn: u32,
        _input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            let out = if turn == 1 {
                lingxi_kernel::ports::ProviderTurn::ToolRequests {
                    requests: vec![ToolRequest {
                        target: "park.tool".to_string(),
                        args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({
                            "target": "park.tool", "payload": "fixed",
                        })),
                        args_summary: Some("parking tool".to_string()),
                        delegation: None,
                    }],
                }
            } else {
                lingxi_kernel::ports::ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "unreachable while parked".to_string(),
                        }],
                        model_call_id: None,
                    },
                }
            };
            ProviderTurnResult::of_ctx(&ctx_at_issue, out)
        })
    }
}

/// Parks at a 0-permit gate after recording the arrival (the run's tool
/// I/O boundary — where the exit's cancellation must reach it).
struct ParkingTool {
    gate: Arc<tokio::sync::Semaphore>,
    arrivals: std::sync::Mutex<Vec<String>>,
}

impl ToolExecutorPort for ParkingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.arrivals.lock().unwrap().push(call.to_string());
        let ctx_at_issue = ctx.clone();
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            let _permit = gate.acquire().await;
            let outcome = ToolOutcome::Failed {
                error: ProtocolError::new(ErrorCode::UpstreamUnavailable, "unparked", false),
            };
            ToolExecutionResult::of_ctx(&ctx_at_issue, outcome)
        })
    }
}

fn parked_deps(gate: Arc<tokio::sync::Semaphore>) -> ServiceDeps {
    ServiceDeps {
        turn_provider: Some(Arc::new(OneToolThenFinalProvider)),
        tool_executor: Some(Arc::new(ParkingTool {
            gate,
            arrivals: std::sync::Mutex::new(Vec::new()),
        }) as Arc<dyn ToolExecutorPort>),
        ..ServiceDeps::default()
    }
}

/// R03-A14 service level: after the intake closes, a CONCURRENT wave of
/// fresh submissions (foreground + background surfaces, busy AND idle
/// sessions, plain and explicit-requestId) is refused with
/// `ShuttingDown`; an idempotent REPLAY of the already-admitted run still
/// answers; the EXISTING parked run settles `cancelled` through its own
/// finalize inside the exit's cancellation+drain; the report is honest.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_window_rejects_concurrent_submissions_and_settles_existing_tasks() {
    let home = synthetic_home("exit-race");
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap_with_deps(
        config_for(&home),
        &layout,
        parked_deps(Arc::clone(&gate)),
    )
    .await
    .expect("bootstrap");

    // The EXISTING task: a background submission (requestId "exit-1") that
    // parks at its tool I/O — an admitted run with no client attached.
    let storage = Arc::clone(state.storage());
    let accepted = state
        .sessions()
        .execute_background_for(
            &storage,
            state.events(),
            state.runs(),
            state.background(),
            &owner_principal(),
            "sess_local_alpha",
            &ExecuteSubmission {
                input: "existing task parked at its tool",
                request_id: Some("exit-1"),
            },
            1_790_409_600_000,
        )
        .await
        .expect("background submission accepted before the exit");
    let run_id = accepted.run_id.clone();
    // Wait until the drive is parked at its tool AND its run row is
    // committed (the registry entry alone can briefly precede the row).
    let drive_ready = {
        let deadline = std::time::Instant::now() + Duration::from_millis(2_000);
        loop {
            let row_committed = state.storage().total_runs().await.unwrap_or(0) == 1;
            let drive_live = state.background().live_ids().iter().any(|id| id == &run_id);
            if row_committed && drive_live {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    assert!(
        drive_ready,
        "the background drive is live, parked at its tool, and its run row is committed"
    );
    let runs_before = state
        .storage()
        .total_runs()
        .await
        .expect("total runs before");

    // shutdown 已开始: the signal-time intake closure (the binary's
    // shutdown future performs exactly this call, first of the exit).
    state.submission_intake().close();
    assert!(state.submission_intake().is_closed());

    // 并发提交新任务: 10 concurrent fresh submissions across BOTH sessions
    // and BOTH surfaces, mixed plain / explicit-id / background.
    let owner = owner_principal();
    let mut tasks = Vec::new();
    for n in 0..10usize {
        let state = state.clone();
        let owner = owner.clone();
        let storage = Arc::clone(&storage);
        tasks.push(tokio::spawn(async move {
            let session = if n % 2 == 0 {
                "sess_local_alpha"
            } else {
                "sess_local_beta"
            };
            let text = format!("exit-race-{n}");
            let submission = if n % 3 == 0 {
                ExecuteSubmission {
                    input: &text,
                    request_id: Some("fresh-exit-race"),
                }
            } else {
                ExecuteSubmission::plain(&text)
            };
            if n % 5 == 0 {
                state
                    .sessions()
                    .execute_background_for(
                        &storage,
                        state.events(),
                        state.runs(),
                        state.background(),
                        &owner,
                        session,
                        &submission,
                        1_790_409_601_000,
                    )
                    .await
            } else {
                state
                    .sessions()
                    .execute_submission_for(
                        storage.as_ref(),
                        state.events(),
                        state.runs(),
                        &owner,
                        session,
                        &submission,
                        1_790_409_601_000,
                    )
                    .await
            }
        }));
    }
    let mut refused = 0usize;
    for t in tasks {
        match t.await.unwrap() {
            Err(SessionExecuteError::ShuttingDown) => refused += 1,
            other => panic!("every fresh submission must be refused, got {other:?}"),
        }
    }
    assert_eq!(refused, 10);
    assert_eq!(
        state.storage().total_runs().await.expect("runs after"),
        runs_before,
        "the refused wave allocated NO run and wrote NOTHING"
    );

    // An idempotent REPLAY of the already-admitted task still answers (it
    // is a query about an EXISTING task, not a new submission).
    let replayed = state
        .sessions()
        .execute_background_for(
            &storage,
            state.events(),
            state.runs(),
            state.background(),
            &owner,
            "sess_local_alpha",
            &ExecuteSubmission {
                input: "existing task parked at its tool",
                request_id: Some("exit-1"),
            },
            1_790_409_602_000,
        )
        .await
        .expect("the replay of the admitted task is answered");
    assert!(replayed.replayed);
    assert_eq!(replayed.run_id, run_id);

    // The exit sequence itself, with a SEPARATE concurrently firing wave:
    // submissions keep arriving WHILE the drain runs and are still refused.
    let racing_state = state.clone();
    let racing_owner = owner.clone();
    let racing_storage = Arc::clone(&storage);
    let racer = tokio::spawn(async move {
        let mut refused_during_drain = 0usize;
        for n in 0..4usize {
            let text = format!("during-drain-{n}");
            let submission = ExecuteSubmission::plain(&text);
            match racing_state
                .sessions()
                .execute_submission_for(
                    racing_storage.as_ref(),
                    racing_state.events(),
                    racing_state.runs(),
                    &racing_owner,
                    "sess_local_beta",
                    &submission,
                    1_790_409_603_000,
                )
                .await
            {
                Err(SessionExecuteError::ShuttingDown) => refused_during_drain += 1,
                other => panic!("during-drain submission must be refused, got {other:?}"),
            }
        }
        refused_during_drain
    });

    // 已有任务按策略收束: the coordinator requests the cancellation of the
    // live drive (task-type cancellation), which settles it through its OWN
    // single finalize; the bounded drain then confirms it.
    let db_path = state.storage().db_path().to_path_buf();
    let guard = published_guard(&home);
    let ws = Arc::new(WsShutdown::new());
    let budget = ShutdownBudget::new(std::time::Instant::now(), Duration::from_secs(5));
    let report = graceful_shutdown(
        state.storage(),
        &ws,
        state.background(),
        state.runs(),
        guard,
        false,
        budget,
    )
    .await;
    assert_eq!(report.exit_code(), 0, "{report:?}");
    assert_eq!(
        report.background_cancel_requested,
        vec![run_id.clone()],
        "the exit requested the live drive's cancellation"
    );
    assert!(
        report.background_unconfirmed.is_empty(),
        "the parked run settled within the budget: {report:?}"
    );
    assert_eq!(racer.await.unwrap(), 4, "the during-drain wave was refused");

    // The durable outcome (read from the closed database file — the exit
    // closed the service's own handle): the existing run settled
    // `cancelled` through its own finalize.
    let disk = rusqlite::Connection::open(&db_path).expect("reopen after close");
    let status: String = disk
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [&run_id],
            |row| row.get(0),
        )
        .expect("run row");
    assert_eq!(status, "cancelled");
    let reason: String = disk
        .query_row(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            [&run_id],
            |row| row.get(0),
        )
        .expect("terminal reason");
    assert_eq!(reason, "cancelled.requested");
    drop(disk);

    harness::write_evidence(
        "r03_a14_exit_window",
        serde_json::json!({
            "existing_run": run_id,
            "fresh_refused_before_drain": refused,
            "fresh_refused_during_drain": 4,
            "replay_answered": true,
            "existing_run_terminal": "cancelled",
            "exit_code": report.exit_code(),
            "background_unconfirmed": report.background_unconfirmed,
        }),
    );
    println!(
        "R03_A14_TRACE: existing_run={run_id} refused_before={refused} refused_during_drain=4 \
         replay=answered terminal=cancelled exit_code={}",
        report.exit_code()
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// R03-A14 transport level: a real HTTP POST to the execute route AFTER
/// the intake closes answers 503 with the stable `shutting_down` reason
/// and `service.shutting_down` cause — the exit window is closed at the
/// real admission boundary, not only in-process.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_execute_is_refused_with_503_while_shutting_down() {
    let home = synthetic_home("exit-http");
    let layout = prepare_layout(&home).expect("layout");
    let state =
        ServiceState::bootstrap_with_deps(config_for(&home), &layout, ServiceDeps::default())
            .await
            .expect("bootstrap");
    // The SAME state serves and gets its intake closed (ServiceState is a
    // clone-sharing handle — the intake Arc is shared with the serving
    // task's copy).
    let serving = state.clone();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(run(
        serving,
        async {
            let _ = stop_rx.await;
        },
        |addr| {
            let _ = ready_tx.send(addr);
        },
        None,
    ));
    let addr = ready_rx.await.expect("service ready");
    let token = local_token(&home);

    // Sanity: while the intake is OPEN the route serves submissions.
    let (status_ok, body_ok) = http_post_execute(addr, &token, "before the exit").await;
    assert_eq!(status_ok, 200, "{body_ok}");

    // shutdown 已开始 — the signal-time closure (the binary's shutdown
    // future performs exactly this call before any drain).
    state.submission_intake().close();

    // 并发提交新任务 (three concurrent real POSTs through the transport).
    let mut posts = Vec::new();
    for n in 0..3usize {
        let token = token.clone();
        posts.push(tokio::spawn(async move {
            http_post_execute(addr, &token, &format!("during the exit {n}")).await
        }));
    }
    for post in posts {
        let (status, body) = post.await.unwrap();
        assert_eq!(
            status, 503,
            "the exit window refuses fresh submissions: {body}"
        );
        assert!(body.contains("shutting_down"), "{body}");
        assert!(body.contains("service.shutting_down"), "{body}");
        assert!(
            body.contains("\"retryable\":true"),
            "the refusal is retryable against the next instance: {body}"
        );
    }

    stop_tx.send(()).expect("stop server");
    tokio::time::timeout(Duration::from_secs(10), handle)
        .await
        .expect("server stops")
        .expect("join")
        .expect("clean serve");
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&home);
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn local_token(home: &std::path::Path) -> String {
    let raw = std::fs::read_to_string(home.join("lingxi-service").join("local-token.json"))
        .expect("local token file exists");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("token json");
    value["token"].as_str().expect("token string").to_string()
}

async fn http_post_execute(addr: SocketAddr, token: &str, input: &str) -> (u16, String) {
    let body = serde_json::json!({ "input": input }).to_string();
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let request = format!(
        "POST /lingxi/v1/sessions/sess_local_alpha/execute HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Connection: close\r\n\
         Authorization: Bearer {token}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read response");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status in head: {head}"));
    (status, body.to_string())
}

/// Acquires the instance lock of a synthetic home and publishes the guard
/// for the shutdown coordinator's record-cleanup phase.
fn published_guard(home: &std::path::Path) -> lingxi_service::instance::InstanceGuard {
    let layout = prepare_layout(home).expect("layout");
    let (mut guard, stale) = lingxi_service::acquire(&layout).expect("acquire");
    assert!(stale.is_none(), "no stale record in a synthetic home");
    guard
        .publish("127.0.0.1:9".parse().unwrap())
        .expect("publish");
    guard
}
