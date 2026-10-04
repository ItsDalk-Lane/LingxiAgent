//! R03 repair G04/F05 integration: the admission/dedup consistency the
//! adversarial audit demands (dedup entries must not solidify before the
//! submission is REALLY admitted; failed retries must never replay a run
//! that does not exist).
//!
//! Five cases (R03-FIX-F05-C01..C05) through the REAL service composition
//! (real SQLite run database, real session gate + requestId dedup, real
//! run supervisor + task supervisor, real single-finalize chain). The
//! doubles only produce external responses and controlled external side
//! effects (an append-only counter file that survives "restarts"); the
//! storage-port decorators below delegate EVERY write to the real
//! database and inject only the controlled fault the audit asks for
//! (start-transaction failure / a deterministic park between reservation
//! and durable start) — the same GatedPort pattern the accepted
//! recovery_crash_points suite uses.
//!
//! Cross-cutting assertion vocabulary (what "no ghost" means here): a
//! SUCCESSFUL submission response is only legal when its run id has a
//! durable row; a submission that failed before any durable fact may be
//! retried under the SAME id and must then really start (or explicitly
//! recover an existing valid record) — never replay a nonexistent run.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    CommittedOutcome, InvocationIntent, InvocationPhase, InvocationReceipt, KeyEvent,
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, RunOutcome, StaleResultFact,
    StorageError, StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest,
    TurnDeltaSink, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, RunId, ToolCallId,
};

use lingxi_service::{
    cancel::CancelPolicy, prepare_layout, ExecuteAccepted, ExecuteSubmission, HomeSource,
    NetworkMode, ServiceConfig, ServiceDeps, ServiceState, SessionExecuteError,
};

const NOW_MS: u64 = 1_790_409_600_000;

// ── deterministic doubles (external responses + external side effects) ──────

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
            provider: "stub.g04".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        input: &'a ModelTurnInput,

        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let input = input.submission.as_str();
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

/// Tool double whose ONE observable external side effect is an append to
/// a counter FILE (independent of any service state — it survives
/// "restarts", which is exactly what C04/C05 observe). The arrival
/// channel lets tests synchronize on "the external action happened".
struct CountingFileTool {
    counter: PathBuf,
    arrivals: tokio::sync::mpsc::UnboundedSender<String>,
}

impl ToolExecutorPort for CountingFileTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let _ = self
            .arrivals
            .send(format!("{}|{}", ctx.run_id, request.target));
        let counter = self.counter.clone();
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&counter)
                .expect("open counter file");
            writeln!(file, "external+1").expect("append external effect");
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::success_text("counter+1".to_string()),
            )
        })
    }
}

fn tool_request() -> ToolRequest {
    ToolRequest::from_effective_arguments(
        "counter.write",
        serde_json::json!({"op": "bump"}),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request")
    .with_summary("counter bump")
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

/// The turn script every case uses: ONE external tool call, then the
/// final turn.
fn scripted_steps() -> Vec<ProviderTurn> {
    vec![
        ProviderTurn::ToolRequests {
            content: Vec::new(),
            requests: vec![tool_request()],
        },
        final_turn("g04 done"),
    ]
}

/// The marker-keyed script for one case.
fn scripted(marker: &'static str) -> Vec<(&'static str, Vec<ProviderTurn>)> {
    vec![(marker, scripted_steps())]
}

fn external_count(counter: &Path) -> usize {
    std::fs::read_to_string(counter)
        .map(|raw| raw.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

// ── storage-port decorators (controlled fault points; everything else
//    delegates to the REAL database — the recovery_crash_points pattern) ─────

/// Injects a failure of the run-start transaction BEFORE it is submitted
/// to the real database (C02: 起始持久化失败补偿); reads (load_run) and
/// every later write delegate untouched, so the post-failure verification
/// and the retry run against the real store.
struct FailingStartPort {
    inner: Arc<lingxi_adapters::storage::RunDatabase>,
    remaining_start_failures: AtomicUsize,
}

/// Parks INSIDE record_run_started (before delegating) until released —
/// the deterministic "between reservation and durable commit" window C03
/// interleaves a concurrent duplicate into.
struct ParkingStartPort {
    inner: Arc<lingxi_adapters::storage::RunDatabase>,
    release: Arc<AtomicBool>,
    parked: Arc<AtomicBool>,
}

macro_rules! delegate_storage_port {
    ($ty:ty) => {
        impl StoragePort for $ty {
            async fn record_run_started(
                &self,
                ctx: &RunContext,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                self.start_gate().await?;
                self.inner.record_run_started(ctx, now_unix_ms).await
            }
            async fn commit_run_outcome(
                &self,
                ctx: &RunContext,
                outcome: RunOutcome,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                self.inner
                    .commit_run_outcome(ctx, outcome, now_unix_ms)
                    .await
            }
            async fn load_run(
                &self,
                run_id: &RunId,
            ) -> Result<Option<lingxi_kernel::ports::RunRecord>, StorageError> {
                self.inner.load_run(run_id).await
            }
            async fn record_run_events(
                &self,
                ctx: &RunContext,
                events: Vec<KeyEvent>,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                self.inner.record_run_events(ctx, events, now_unix_ms).await
            }
            async fn record_stale_result(
                &self,
                ctx: &RunContext,
                refused: StaleResultFact,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                self.inner
                    .record_stale_result(ctx, refused, now_unix_ms)
                    .await
            }
            async fn record_attempt_started(
                &self,
                ctx: &RunContext,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                self.inner.record_attempt_started(ctx, now_unix_ms).await
            }
            async fn record_model_call_usage(
                &self,
                record: lingxi_kernel::usage::ModelCallUsageRecord,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                self.inner
                    .record_model_call_usage(record, now_unix_ms)
                    .await
            }
            async fn query_model_call_usage(
                &self,
                query: lingxi_kernel::usage::ModelUsageQuery,
            ) -> Result<Vec<lingxi_kernel::usage::ModelCallUsageRecord>, StorageError> {
                self.inner.query_model_call_usage(query).await
            }
            async fn record_run_state_change(
                &self,
                ctx: &RunContext,
                from: lingxi_protocol::RunStatus,
                to: lingxi_protocol::RunStatus,
                reason: Option<String>,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                self.inner
                    .record_run_state_change(ctx, from, to, reason, now_unix_ms)
                    .await
            }
            async fn record_invocation_intent(
                &self,
                ctx: &RunContext,
                intent: InvocationIntent,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                self.inner
                    .record_invocation_intent(ctx, intent, now_unix_ms)
                    .await
            }
            async fn advance_invocation(
                &self,
                ctx: &RunContext,
                journal_id: &ToolCallId,
                to: InvocationPhase,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                self.inner
                    .advance_invocation(ctx, journal_id, to, now_unix_ms)
                    .await
            }
            async fn record_invocation_receipt(
                &self,
                ctx: &RunContext,
                journal_id: &ToolCallId,
                receipt: InvocationReceipt,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                self.inner
                    .record_invocation_receipt(ctx, journal_id, receipt, now_unix_ms)
                    .await
            }
            async fn record_invocation_unknown(
                &self,
                journal_id: &ToolCallId,
                detail: String,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                self.inner
                    .record_invocation_unknown(journal_id, detail, now_unix_ms)
                    .await
            }
            async fn load_invocation_journal(
                &self,
                run_id: &RunId,
            ) -> Result<Vec<lingxi_kernel::ports::InvocationJournalEntry>, StorageError> {
                self.inner.load_invocation_journal(run_id).await
            }
            async fn record_run_lineage(
                &self,
                ctx: &RunContext,
                lineage: lingxi_kernel::subagent::RunLineage,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                self.inner
                    .record_run_lineage(ctx, lineage, now_unix_ms)
                    .await
            }
            async fn load_run_lineage(
                &self,
                run_id: &RunId,
            ) -> Result<Option<lingxi_kernel::subagent::RunLineage>, StorageError> {
                self.inner.load_run_lineage(run_id).await
            }
        }
    };
}

impl FailingStartPort {
    async fn start_gate(&self) -> Result<(), StorageError> {
        if self.remaining_start_failures.load(Ordering::SeqCst) > 0 {
            self.remaining_start_failures.fetch_sub(1, Ordering::SeqCst);
            return Err(StorageError::Io {
                detail: "injected run-start transaction failure (G04/F05 C02: before submit)"
                    .to_string(),
            });
        }
        Ok(())
    }
}

impl ParkingStartPort {
    async fn start_gate(&self) -> Result<(), StorageError> {
        self.parked.store(true, Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !self.release.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "parked start never released"
            );
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        Ok(())
    }
}

delegate_storage_port!(FailingStartPort);
delegate_storage_port!(ParkingStartPort);

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03-g04-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn counter_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-r03-g04-counter-{tag}-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
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
    provider: Arc<ScriptedProvider>,
    tool: Arc<CountingFileTool>,
    cancel_policy: CancelPolicy,
) -> ServiceState {
    let layout = prepare_layout(home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(tool),
        cancel_policy,
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

async fn submit_fg<P: StoragePort>(
    state: &ServiceState,
    port: &P,
    session: &'static str,
    input: &str,
    request_id: &str,
) -> Result<ExecuteAccepted, SessionExecuteError> {
    let submission = ExecuteSubmission {
        input,
        request_id: Some(request_id),
    };
    state
        .sessions()
        .execute_submission_for(
            port,
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
) -> Result<ExecuteAccepted, SessionExecuteError> {
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

async fn session_runs(state: &ServiceState, session: &str) -> usize {
    query_text(
        state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        session,
    )
    .await
    .and_then(|v| v.parse().ok())
    .unwrap_or(0)
}

async fn wait_until(what: &str, mut probe: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !probe() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

// ── R03-FIX-F05-C01: 后台 spawn 拒绝后无幽灵 Replay ─────────────────────────

/// 占满真实 TaskSupervisor 测试容量（真实受监督任务），提交带新
/// requestId 的后台任务 → 显式拒绝；释放容量后同 key 重试 → 必须真实
/// 启动（响应的 run id 有 durable 行、外部动作恰好发生一次），不能返回
/// 不存在的 Run。
#[tokio::test]
async fn r03_f05_c01_background_spawn_rejection_leaves_no_ghost_replay() {
    let counter = counter_path("c01");
    let (arrival_tx, _arrival_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let provider = ScriptedProvider::new(scripted("C01"));
    let tool = Arc::new(CountingFileTool {
        counter: counter.clone(),
        arrivals: arrival_tx,
    });
    // A small REAL supervised-task cap (same semantics, tiny occupancy):
    // the spawn rejection below is the real capacity gate, not a stub.
    let cancel_policy = CancelPolicy {
        supervised_task_cap: 4,
        ..CancelPolicy::default()
    };
    let home = synthetic_home("c01");
    let state = boot_on(&home, provider, tool, cancel_policy).await;

    // 占满容量：4 个真实受监督任务停在受控外部等待上。
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let mut fillers = Vec::new();
    for n in 0..4 {
        let gate = Arc::clone(&gate);
        fillers.push(
            state
                .runs()
                .task_supervisor()
                .spawn_detached(format!("c01-filler-{n}"), async move {
                    let _ = gate.acquire().await;
                })
                .expect("filler spawn"),
        );
    }

    // 首次提交：显式未受理（真实容量拒绝；不得伪装成成功）。
    match submit_bg(&state, "sess_local_alpha", "C01: bg work", "c01-k").await {
        Err(SessionExecuteError::BackgroundRegistryFull { cap }) => assert_eq!(cap, 4),
        other => panic!("first submission must be an explicit refusal, got {other:?}"),
    }
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 0);
    assert_eq!(external_count(&counter), 0, "nothing executed");

    // 释放容量（后台注册表容量检查与真实 spawn 之间的拒绝窗口已被覆盖：
    // 拒绝来自 spawn_detached 的容量门）。
    gate.add_permits(4);
    for filler in fillers {
        filler.wait().await.expect("filler exits cleanly");
    }

    // 同 key 重试：真实启动或明确恢复既有有效记录 — 不能返回不存在 Run。
    let retry = submit_bg(&state, "sess_local_alpha", "C01: bg work", "c01-k").await;
    let accepted = retry.expect("retry after capacity release starts for real");
    assert!(
        run_status(&state, &accepted.run_id).await.is_some(),
        "no ghost replay: the accepted run id must have a durable row ({})",
        accepted.run_id
    );
    wait_until("the external effect happens exactly once", || {
        external_count(&counter) == 1
    })
    .await;
    wait_until("the background drive settles", || {
        state.background().live_ids().is_empty()
    })
    .await;
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);
    assert_eq!(external_count(&counter), 1);

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── R03-FIX-F05-C02: 运行起始持久化失败补偿 ────────────────────────────────

/// 真实存储起始事务提交前注入故障 → 前台带 key 提交失败；恢复存储后同
/// key 重试：不残留假受理、安全重新受理、最终只一个有效执行。
#[tokio::test]
async fn r03_f05_c02_start_transaction_failure_is_compensated() {
    let counter = counter_path("c02");
    let (arrival_tx, _arrival_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let provider = ScriptedProvider::new(scripted("C02"));
    let tool = Arc::new(CountingFileTool {
        counter: counter.clone(),
        arrivals: arrival_tx,
    });
    let home = synthetic_home("c02");
    let state = boot_on(&home, provider, tool, CancelPolicy::default()).await;

    let failing = FailingStartPort {
        inner: Arc::clone(state.storage()),
        remaining_start_failures: AtomicUsize::new(1),
    };

    // 前台带 key 提交：起始事务失败 → 显式错误（无可见成功）。
    match submit_fg(
        &state,
        &failing,
        "sess_local_alpha",
        "C02: start me",
        "c02-k",
    )
    .await
    {
        Err(SessionExecuteError::Storage(StorageError::Io { detail })) => {
            assert!(detail.contains("injected"), "detail: {detail}");
        }
        other => panic!("start-transaction failure must surface loudly, got {other:?}"),
    }
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 0);
    assert_eq!(external_count(&counter), 0, "no external action may happen");

    // 存储恢复后同 key 重试：安全重新受理；成功响应必须对应真实运行行。
    let retry = submit_fg(
        &state,
        state.storage().as_ref(),
        "sess_local_alpha",
        "C02: start me",
        "c02-k",
    )
    .await
    .expect("retry after recovery re-admits for real");
    assert!(
        run_status(&state, &retry.run_id).await.is_some(),
        "no ghost replay: the retried run id must have a durable row ({})",
        retry.run_id
    );
    assert_eq!(
        session_runs(&state, "sess_local_alpha").await,
        1,
        "最终只一个有效执行"
    );
    assert_eq!(external_count(&counter), 1);

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── R03-FIX-F05-C03: 并发同 key 和异内容 ────────────────────────────────────

/// 同主体会话两个同 key 请求在预留/提交之间交错：同内容重试只有一个
/// 有效受理（不得暴露半提交假结果）；不同内容明确冲突；结算后重试是
/// 指向同一真实运行的幂等 replay。
#[tokio::test]
async fn r03_f05_c03_concurrent_same_key_between_reservation_and_commit() {
    let counter = counter_path("c03");
    let (arrival_tx, _arrival_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let provider = ScriptedProvider::new(scripted("C03"));
    let tool = Arc::new(CountingFileTool {
        counter: counter.clone(),
        arrivals: arrival_tx,
    });
    let home = synthetic_home("c03");
    let state = boot_on(&home, provider, tool, CancelPolicy::default()).await;

    let parked_port = Arc::new(ParkingStartPort {
        inner: Arc::clone(state.storage()),
        release: Arc::new(AtomicBool::new(false)),
        parked: Arc::new(AtomicBool::new(false)),
    });

    // 首个提交：停在预留与持久受理之间。
    let state_a = state.clone();
    let port_a = Arc::clone(&parked_port);
    let first = tokio::spawn(async move {
        submit_fg(
            &state_a,
            port_a.as_ref(),
            "sess_local_alpha",
            "C03: concurrent",
            "c03-k",
        )
        .await
    });
    wait_until(
        "the first submission parks between reservation and durable start",
        || parked_port.parked.load(Ordering::SeqCst),
    )
    .await;

    // 同 key 同内容的并发重试：成功响应必须对应真实运行行（不得暴露
    // 半提交假结果）；显式拒绝也是合法契约。
    match submit_fg(
        &state,
        parked_port.as_ref(),
        "sess_local_alpha",
        "C03: concurrent",
        "c03-k",
    )
    .await
    {
        Ok(accepted) => {
            assert!(
                run_status(&state, &accepted.run_id).await.is_some(),
                "no half-committed fake result: {} has no durable row",
                accepted.run_id
            );
        }
        Err(_) => { /* explicit refusal — the retryable in-flight contract */ }
    }
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 0);

    // 同 key 不同内容：明确冲突（无论受理处于哪个相位）。
    match submit_fg(
        &state,
        parked_port.as_ref(),
        "sess_local_alpha",
        "C03: CHANGED content",
        "c03-k",
    )
    .await
    {
        Err(SessionExecuteError::DuplicateRequestConflict { request_id, .. }) => {
            assert_eq!(request_id, "c03-k");
        }
        other => panic!("different content under the same key must conflict, got {other:?}"),
    }

    // 释放窗口：首个提交真实受理并结算。
    parked_port.release.store(true, Ordering::SeqCst);
    let first = first
        .await
        .expect("first task")
        .expect("first submission completes for real");
    assert!(run_status(&state, &first.run_id).await.is_some());
    assert_eq!(external_count(&counter), 1);

    // 结算后同 key 同内容重试：幂等 replay，指向同一真实运行。
    let replay = submit_fg(
        &state,
        state.storage().as_ref(),
        "sess_local_alpha",
        "C03: concurrent",
        "c03-k",
    )
    .await
    .expect("post-settle retry replays");
    assert!(replay.replayed);
    assert_eq!(replay.run_id, first.run_id);
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);
    assert_eq!(external_count(&counter), 1);

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── R03-FIX-F05-C04: 副作用之后响应失败不删绑定 ────────────────────────────

/// 外部动作已 +1 后响应丢失（journal 提交前/后两态）→ 同 key 重试：不
/// 重复执行、返回既有（可核实的）运行。该用例同时钉住修复红线：任何
/// 错误路径都不得清掉已经持久受理的绑定。
#[tokio::test]
async fn r03_f05_c04_post_side_effect_response_loss_never_reexecutes() {
    let counter = counter_path("c04");
    let (arrival_tx, mut arrival_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let provider =
        ScriptedProvider::new(vec![("C04A", scripted_steps()), ("C04B", scripted_steps())]);
    let tool = Arc::new(CountingFileTool {
        counter: counter.clone(),
        arrivals: arrival_tx,
    });
    let home = synthetic_home("c04");
    let state = boot_on(&home, provider, tool, CancelPolicy::default()).await;

    // 变体 A（journal started 已提交、receipt 可能未提交——响应在副作用
    // 之后、任务结算之前丢失）：前台请求 future 被丢弃。
    let state_a = state.clone();
    let inflight = tokio::spawn(async move {
        submit_fg(
            &state_a,
            state_a.storage().as_ref(),
            "sess_local_alpha",
            "C04A: inflight",
            "c04-a",
        )
        .await
    });
    wait_until(
        "the external side effect happened (journal started committed)",
        || external_count(&counter) == 1,
    )
    .await;
    assert!(
        arrival_rx.try_recv().is_ok(),
        "the tool arrival is observed before the response is lost"
    );
    inflight.abort(); // 响应丢失
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);

    // 同 key 重试：返回既有（可核实的）运行，不重复执行。
    let retry_a = submit_fg(
        &state,
        state.storage().as_ref(),
        "sess_local_alpha",
        "C04A: inflight",
        "c04-a",
    )
    .await
    .expect("retry after response loss replays the real run");
    assert!(retry_a.replayed);
    assert!(run_status(&state, &retry_a.run_id).await.is_some());
    assert_eq!(external_count(&counter), 1, "不因遇错清理绑定而重复执行");
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);

    // 变体 B（journal receipt 已提交、run 已结算——响应在一切事实之后
    // 丢失）：直接丢弃首次响应，同 key 重试。
    let _settled = submit_fg(
        &state,
        state.storage().as_ref(),
        "sess_local_alpha",
        "C04B: settled",
        "c04-b",
    )
    .await
    .expect("settles durably (response discarded by the client)");
    assert_eq!(external_count(&counter), 2);
    let retry_b = submit_fg(
        &state,
        state.storage().as_ref(),
        "sess_local_alpha",
        "C04B: settled",
        "c04-b",
    )
    .await
    .expect("retry after settle replays");
    assert!(retry_b.replayed);
    assert_eq!(
        external_count(&counter),
        2,
        "settled results are returned, not re-executed"
    );
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 2);

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── R03-FIX-F05-C05: 重启重试的安全契约 ────────────────────────────────────

/// 任务有确认副作用、客户端仍持原 key → 重启后同 key 提交：按明确契约
/// 恢复绑定/拒绝（不得静默忽略已有事实后盲重做）；同测新 key 正常受理
/// 与同 key 改内容；不伪称 exactly-once。
#[tokio::test]
async fn r03_f05_c05_restart_same_key_is_an_explicit_contract_not_a_blind_redo() {
    let home = synthetic_home("c05");
    let counter = counter_path("c05");

    // 进程 A：带 key 提交 → 外部副作用 +1、运行完成。
    let (arrival_tx, _arrival_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let provider_a = ScriptedProvider::new(scripted("C05"));
    let tool_a = Arc::new(CountingFileTool {
        counter: counter.clone(),
        arrivals: arrival_tx,
    });
    let state_a = boot_on(&home, provider_a, tool_a, CancelPolicy::default()).await;
    let first = submit_fg(
        &state_a,
        state_a.storage().as_ref(),
        "sess_local_alpha",
        "C05: once",
        "c05-k",
    )
    .await
    .expect("accepted in process A");
    assert_eq!(
        run_status(&state_a, &first.run_id).await.as_deref(),
        Some("completed")
    );
    assert_eq!(external_count(&counter), 1);
    state_a.storage().close().await.expect("close A");
    drop(state_a);

    // 进程 B：同一数据根上的全新服务（真实 bootstrap；内存去重表随进程
    // 消失，持久事实仍在）。
    let (arrival_tx_b, _arrival_rx_b) = tokio::sync::mpsc::unbounded_channel::<String>();
    let provider_b = ScriptedProvider::new(scripted("C05"));
    let tool_b = Arc::new(CountingFileTool {
        counter: counter.clone(),
        arrivals: arrival_tx_b,
    });
    let state_b = boot_on(&home, provider_b, tool_b, CancelPolicy::default()).await;
    let runs_before = session_runs(&state_b, "sess_local_alpha").await;
    let ext_before = external_count(&counter);
    assert_eq!(runs_before, 1);
    assert_eq!(ext_before, 1);

    // 同 key 同内容：明确契约 —— 恢复绑定（必须是既有运行）或显式拒绝；
    // 不能静默当作全新请求重做。
    match submit_fg(
        &state_b,
        state_b.storage().as_ref(),
        "sess_local_alpha",
        "C05: once",
        "c05-k",
    )
    .await
    {
        Ok(accepted) => {
            assert!(
                accepted.replayed,
                "a post-restart binding recovery must be an explicit replay"
            );
            assert_eq!(
                accepted.run_id, first.run_id,
                "the recovered binding must be the EXISTING run"
            );
        }
        Err(_) => { /* an explicit refusal naming the earlier run is the contract */ }
    }
    assert_eq!(
        session_runs(&state_b, "sess_local_alpha").await,
        runs_before,
        "no blind redo after restart"
    );
    assert_eq!(
        external_count(&counter),
        ext_before,
        "no second external effect"
    );

    // 新 key：正常受理（真实新执行）。
    let fresh = submit_fg(
        &state_b,
        state_b.storage().as_ref(),
        "sess_local_alpha",
        "C05: once",
        "c05-new",
    )
    .await
    .expect("a NEW id is a fresh validated submission");
    assert!(!fresh.replayed);
    assert!(run_status(&state_b, &fresh.run_id).await.is_some());
    let runs_mid = session_runs(&state_b, "sess_local_alpha").await;
    let ext_mid = external_count(&counter);
    assert_eq!(runs_mid, runs_before + 1);
    assert_eq!(ext_mid, ext_before + 1);

    // 同 key 改内容：同样不得静默重做。
    match submit_fg(
        &state_b,
        state_b.storage().as_ref(),
        "sess_local_alpha",
        "C05: CHANGED content",
        "c05-k",
    )
    .await
    {
        Ok(accepted) => assert_eq!(
            accepted.run_id, first.run_id,
            "same key changed content must not silently start a NEW execution"
        ),
        Err(_) => { /* explicit refusal/conflict — the documented contract */ }
    }
    assert_eq!(session_runs(&state_b, "sess_local_alpha").await, runs_mid);
    assert_eq!(external_count(&counter), ext_mid);

    teardown(&state_b, &home).await;
    let _ = std::fs::remove_file(&counter);
}
