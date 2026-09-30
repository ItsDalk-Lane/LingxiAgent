//! R03 repair G04/F05 ADVERSARIAL selfcheck: the attack windows the
//! dispatch names, beyond the base admission_dedup_consistency cases —
//! failures injected BETWEEN the capacity check and the spawn, response
//! loss INSIDE the admission window (the lazy unverified resolution
//! through the real store), cancellation racing the admission window,
//! cross-principal/session isolation while a key is mid-admission,
//! response loss before ANY external effect, and a restart whose earlier
//! run holds UNKNOWN side effects (killed mid-drive).
//!
//! Same tier as the base suite: REAL composition root, REAL SQLite store,
//! REAL supervisor/gate/dedup; the doubles only produce external
//! responses/effects and the one controlled fault (a deterministic park
//! inside record_run_started — the accepted recovery_crash_points
//! GatedPort pattern; every write delegates to the real database).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use lingxi_kernel::ports::{
    CommittedOutcome, InvocationIntent, InvocationPhase, InvocationReceipt, KeyEvent,
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, RunOutcome, StaleResultFact,
    StorageError, StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest,
    TurnProviderPort,
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
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.g04adv".to_string(),
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
        let marker = input.split(':').next().unwrap_or(input).trim().to_string();
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

/// Tool double that parks BEFORE its external effect (on a 0-permit
/// semaphore) — the "drive sits at external I/O, nothing happened yet"
/// shape.
struct PreEffectGatedTool {
    counter: PathBuf,
    gate: Arc<tokio::sync::Semaphore>,
}

impl ToolExecutorPort for PreEffectGatedTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let counter = self.counter.clone();
        let gate = Arc::clone(&self.gate);
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            let _permit = gate.acquire().await.expect("tool gate closed");
            append_external(&counter);
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::success_text("counter+1".to_string()),
            )
        })
    }
}

/// Tool double that performs its external effect FIRST and then parks
/// waiting for the external system's response — the "external action
/// confirmed, outcome pending" shape a killed process leaves behind.
struct PostEffectParkedTool {
    counter: PathBuf,
    gate: Arc<tokio::sync::Semaphore>,
}

impl ToolExecutorPort for PostEffectParkedTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let counter = self.counter.clone();
        let gate = Arc::clone(&self.gate);
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            append_external(&counter);
            let _permit = gate.acquire().await.expect("tool gate closed");
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::success_text("counter+1".to_string()),
            )
        })
    }
}

fn append_external(counter: &Path) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(counter)
        .expect("open counter file");
    writeln!(file, "external+1").expect("append external effect");
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

fn scripted_steps() -> Vec<ProviderTurn> {
    vec![
        ProviderTurn::ToolRequests {
            requests: vec![tool_request()],
        },
        final_turn("g04adv done"),
    ]
}

fn final_only() -> Vec<ProviderTurn> {
    vec![final_turn("g04adv done without tools")]
}

fn external_count(counter: &Path) -> usize {
    std::fs::read_to_string(counter)
        .map(|raw| raw.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

/// Parks INSIDE record_run_started (before delegating to the real
/// database) until released, and records the parked run id — the
/// deterministic reservation→durable-start window.
struct ParkingStartPort {
    inner: Arc<lingxi_adapters::storage::RunDatabase>,
    release: Arc<AtomicBool>,
    parked: Arc<AtomicBool>,
    parked_run_id: std::sync::Mutex<Option<String>>,
}

impl ParkingStartPort {
    async fn start_gate(&self, ctx: &RunContext) -> Result<(), StorageError> {
        *self.parked_run_id.lock().unwrap() = Some(ctx.run_id.to_string());
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

macro_rules! delegate_storage_port {
    ($ty:ty) => {
        impl StoragePort for $ty {
            async fn record_run_started(
                &self,
                ctx: &RunContext,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                self.start_gate(ctx).await?;
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

delegate_storage_port!(ParkingStartPort);

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03-g04adv-{tag}-{}-{}",
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
        "lingxi-r03-g04adv-counter-{tag}-{}-{}.txt",
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

/// A same-user DEVICE principal: a DIFFERENT trusted principal kind that
/// still owns the seeded sessions (the isolation control arm).
fn device_principal_of_local_user() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_device_local".to_string(),
        kind: lingxi_service::PrincipalKind::Device,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: Some("device_a".to_string()),
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Lan,
        credential_kind: lingxi_service::CredentialKind::DeviceCredential,
        trust_state: lingxi_service::TrustState::Lan,
        scopes: vec!["chat".to_string()],
    }
}

async fn boot_on(
    home: &Path,
    provider: Arc<ScriptedProvider>,
    tool: Option<Arc<dyn lingxi_kernel::ports::ToolExecutorPort>>,
    cancel_policy: CancelPolicy,
) -> ServiceState {
    let layout = prepare_layout(home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: tool,
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
    principal: &lingxi_service::Principal,
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
            principal,
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

// ── C01 adversarial: the spawn window + retraction accounting ───────────────

/// 攻击窗口：后台注册表容量检查（1024，未触及）与真实 TaskSupervisor
/// spawn 之间注入失败（受监督任务容量先被真实占满——不是最早的注册表
/// 拒绝）；撤销后同 key 改内容重试是全新受理（被撤销的预留不残留
/// digest 冲突），无关 key 不受撤销影响（注册表/容量记账完好）。
#[tokio::test]
async fn adv_c01_spawn_window_failure_and_retraction_leave_no_sticky_state() {
    let counter = counter_path("adv-c1");
    let provider = ScriptedProvider::new(vec![
        ("ADV1A", scripted_steps()),
        ("ADV1B", scripted_steps()),
        ("ADV1C", final_only()),
    ]);
    let tool = Some(Arc::new(PreEffectGatedTool {
        counter: counter.clone(),
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
    }) as Arc<dyn lingxi_kernel::ports::ToolExecutorPort>);
    let cancel_policy = CancelPolicy {
        supervised_task_cap: 4,
        ..CancelPolicy::default()
    };
    let home = synthetic_home("adv-c1");
    let state = boot_on(&home, provider, tool, cancel_policy).await;

    // 占满真实受监督容量（后台注册表检查通过、spawn 拒绝的窗口）。
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let mut fillers = Vec::new();
    for n in 0..4 {
        let gate = Arc::clone(&gate);
        fillers.push(
            state
                .runs()
                .task_supervisor()
                .spawn_detached(format!("adv-c01-filler-{n}"), async move {
                    let _ = gate.acquire().await;
                })
                .expect("filler spawn"),
        );
    }
    match submit_bg(&state, "sess_local_alpha", "ADV1A: bg", "adv-c1").await {
        Err(SessionExecuteError::BackgroundRegistryFull { cap }) => assert_eq!(cap, 4),
        other => panic!("spawn-window failure must be the loud refusal, got {other:?}"),
    }
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 0);
    assert_eq!(external_count(&counter), 0);

    // 释放容量：同 key 改内容重试 → 被撤销的预留不残留 digest 冲突，
    // 全新受理并真实启动（驱动停在受控外部等待上——run 行真实存在）。
    gate.add_permits(4);
    for filler in fillers {
        filler.wait().await.expect("filler exits");
    }
    let retry = submit_bg(&state, "sess_local_alpha", "ADV1B: changed", "adv-c1")
        .await
        .expect("changed-content retry after retraction is a FRESH admission");
    assert!(!retry.replayed);
    wait_until("the background drive is live with a REAL run row", || {
        state.background().live_ids() == vec![retry.run_id.clone()]
    })
    .await;
    assert_eq!(
        run_status(&state, &retry.run_id).await.as_deref(),
        Some("running"),
        "no ghost: the retried run id has a durable row"
    );
    // 取消收束该驱动（它停在 0 许可工具闸门上）。
    let storage = Arc::clone(state.storage());
    let _ = state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &owner_principal(),
            &retry.run_id,
        )
        .await;
    wait_until("the cancelled background drive settles", || {
        state.background().live_ids().is_empty()
    })
    .await;
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);
    assert_eq!(
        external_count(&counter),
        0,
        "the parked tool never executed"
    );

    // 无关 key 不受撤销影响：正常受理并完整结算（无工具脚本）。
    let other = submit_bg(
        &state,
        "sess_local_alpha",
        "ADV1C: unrelated",
        "adv-c1-other",
    )
    .await
    .expect("an unrelated id is unaffected by the retraction");
    assert!(!other.replayed);
    wait_until("the unrelated background run settles", || {
        state.background().live_ids().is_empty()
    })
    .await;
    assert_eq!(
        run_status(&state, &other.run_id).await.as_deref(),
        Some("completed")
    );

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── C02 adversarial: response lost INSIDE the window + cancel racing it ─────

/// 攻击窗口：预留与持久受理之间（record_run_started 内）请求 future 被
/// 丢弃（响应丢失）→ 绑定 Drop 留下 Unverified；同 key 重试在真实存储上
/// 懒解决（行不存在 → 安全撤销 → 全新受理，最终只一个有效执行）。同窗
/// 口内对该 run 的取消请求是诚实的 NotFound（没有任何可取消事实）。
#[tokio::test]
async fn adv_c02_response_lost_inside_start_window_resolves_lazily() {
    let counter = counter_path("adv-c2");
    // final-only script: the retried run completes deterministically (the
    // case is about the ADMISSION lifecycle, not the drive's turns).
    let provider = ScriptedProvider::new(vec![("ADV2", final_only())]);
    let tool = Some(Arc::new(PreEffectGatedTool {
        counter: counter.clone(),
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
    }) as Arc<dyn lingxi_kernel::ports::ToolExecutorPort>);
    let home = synthetic_home("adv-c2");
    let state = boot_on(&home, provider, tool, CancelPolicy::default()).await;

    let parked_port = Arc::new(ParkingStartPort {
        inner: Arc::clone(state.storage()),
        release: Arc::new(AtomicBool::new(false)),
        parked: Arc::new(AtomicBool::new(false)),
        parked_run_id: std::sync::Mutex::new(None),
    });
    let state_a = state.clone();
    let port_a = Arc::clone(&parked_port);
    let first = tokio::spawn(async move {
        submit_fg(
            &state_a,
            port_a.as_ref(),
            &owner_principal(),
            "sess_local_alpha",
            "ADV2: x",
            "adv-c2",
        )
        .await
    });
    wait_until(
        "the first submission parks between reservation and durable start",
        || parked_port.parked.load(Ordering::SeqCst),
    )
    .await;

    // 取消在受理窗口：该 run 没有任何持久事实 → 诚实的 NotFound。
    let parked_run_id = parked_port
        .parked_run_id
        .lock()
        .unwrap()
        .clone()
        .expect("the parked run id is recorded");
    let storage = Arc::clone(state.storage());
    match state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &owner_principal(),
            &parked_run_id,
        )
        .await
    {
        Err(SessionExecuteError::NotFound) => {}
        other => panic!("cancel inside the admission window must be NotFound, got {other:?}"),
    }

    // 响应丢失：请求 future 被丢弃（无 verdict → Unverified 保留绑定）。
    first.abort();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 0);

    // 同 key 重试：Unverified 在真实存储上懒解决（行不存在 → 安全撤
    // 销 → 全新受理 → 真实执行；不是幽灵 replay）。
    let retry = submit_fg(
        &state,
        state.storage().as_ref(),
        &owner_principal(),
        "sess_local_alpha",
        "ADV2: x",
        "adv-c2",
    )
    .await
    .expect("the retry resolves the unverified reservation and really starts");
    assert!(!retry.replayed, "the resolution is a fresh real start");
    assert_eq!(
        run_status(&state, &retry.run_id).await.as_deref(),
        Some("completed"),
        "the retry's run id has a durable row (settled for real)"
    );
    assert_ne!(
        retry.run_id, parked_run_id,
        "the vanished reservation is not resurrected"
    );
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);
    assert_eq!(external_count(&counter), 0, "nothing executed twice");

    // 结算后的同 key 重试：幂等 replay 指向同一真实运行。
    let replayed = submit_fg(
        &state,
        state.storage().as_ref(),
        &owner_principal(),
        "sess_local_alpha",
        "ADV2: x",
        "adv-c2",
    )
    .await
    .expect("post-settle retry replays");
    assert!(replayed.replayed);
    assert_eq!(replayed.run_id, retry.run_id);
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── C03 adversarial: isolation controls while a key is mid-admission ────────

/// 攻击窗口：alpha 会话的同 key 受理停在预留/提交之间时——另一会话
/// （同主体）与另一主体（同会话）用相同 key 各自全新受理（命名空间隔
/// 离对照）；alpha 的窗口结束后本命名空间仍然只有一个有效受理。
#[tokio::test]
async fn adv_c03_isolation_controls_hold_while_a_key_is_mid_admission() {
    let counter = counter_path("adv-c3");
    let provider = ScriptedProvider::new(vec![
        ("ADV3A", final_only()),
        ("ADV3B", final_only()),
        ("ADV3C", final_only()),
    ]);
    let tool = Some(Arc::new(PreEffectGatedTool {
        counter: counter.clone(),
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
    }) as Arc<dyn lingxi_kernel::ports::ToolExecutorPort>);
    let home = synthetic_home("adv-c3");
    let state = boot_on(&home, provider, tool, CancelPolicy::default()).await;

    let parked_port = Arc::new(ParkingStartPort {
        inner: Arc::clone(state.storage()),
        release: Arc::new(AtomicBool::new(false)),
        parked: Arc::new(AtomicBool::new(false)),
        parked_run_id: std::sync::Mutex::new(None),
    });
    let state_a = state.clone();
    let port_a = Arc::clone(&parked_port);
    let first = tokio::spawn(async move {
        submit_fg(
            &state_a,
            port_a.as_ref(),
            &owner_principal(),
            "sess_local_alpha",
            "ADV3A: x",
            "adv-shared",
        )
        .await
    });
    wait_until(
        "alpha's admission parks between reservation and commit",
        || parked_port.parked.load(Ordering::SeqCst),
    )
    .await;
    // 同 key 在 alpha 上并发：显式 in-flight（不暴露半提交假结果）。
    match submit_fg(
        &state,
        parked_port.as_ref(),
        &owner_principal(),
        "sess_local_alpha",
        "ADV3A: x",
        "adv-shared",
    )
    .await
    {
        Err(SessionExecuteError::AdmissionInFlight { request_id }) => {
            assert_eq!(request_id, "adv-shared")
        }
        other => panic!("alpha duplicate must be in-flight, got {other:?}"),
    }

    // 隔离对照 1：另一会话、同主体、同 key → 自己的命名空间，全新受理。
    let beta = submit_fg(
        &state,
        state.storage().as_ref(),
        &owner_principal(),
        "sess_local_beta",
        "ADV3B: x",
        "adv-shared",
    )
    .await
    .expect("another session owns its id namespace");
    assert!(!beta.replayed);

    // 释放 alpha 的窗口：首个提交真实受理（同会话的 busy 串行化在此
    // 期间对任何主体都成立——包括 device，这是 R03-T02 的冻结语义）。
    parked_port.release.store(true, Ordering::SeqCst);
    let alpha = first.await.expect("task").expect("alpha completes");
    assert_eq!(
        session_runs(&state, "sess_local_alpha").await,
        1,
        "the owner namespace holds exactly one run"
    );
    assert_eq!(session_runs(&state, "sess_local_beta").await, 1);
    // alpha 侧同 key 重试（owner）现在 replay 真实运行。
    let replay = submit_fg(
        &state,
        state.storage().as_ref(),
        &owner_principal(),
        "sess_local_alpha",
        "ADV3A: x",
        "adv-shared",
    )
    .await
    .expect("owner replay after settle");
    assert!(replay.replayed);
    assert_eq!(replay.run_id, alpha.run_id);
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);

    // 隔离对照 2（结算后）：另一主体（同用户 device）、同会话、同 key
    // → 自己的命名空间，全新受理（不 replay owner 的运行、不冲突）。
    let device = submit_fg(
        &state,
        state.storage().as_ref(),
        &device_principal_of_local_user(),
        "sess_local_alpha",
        "ADV3C: x",
        "adv-shared",
    )
    .await
    .expect("another principal kind owns its id namespace");
    assert!(!device.replayed);
    assert_ne!(device.run_id, alpha.run_id);
    assert_ne!(device.run_id, beta.run_id);
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 2);

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── C04 adversarial: response lost before ANY external effect ───────────────

/// 攻击窗口：run 行已持久（绑定已承诺）但外部动作尚未发生时响应丢
/// 失（journal 提交前的丢响应形态）→ 同 key 重试 replay 真实的
/// dangling-active 运行——不是幽灵、不重复受理、外部动作总数保持 0；
/// 该真实运行可被取消面诚实管理（DanglingActive）。
#[tokio::test]
async fn adv_c04_response_lost_before_any_external_effect_replays_real_run() {
    let counter = counter_path("adv-c4");
    let provider = ScriptedProvider::new(vec![("ADV4", scripted_steps())]);
    let tool = Some(Arc::new(PreEffectGatedTool {
        counter: counter.clone(),
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
    }) as Arc<dyn lingxi_kernel::ports::ToolExecutorPort>);
    let home = synthetic_home("adv-c4");
    let state = boot_on(&home, provider, tool, CancelPolicy::default()).await;

    let state_a = state.clone();
    let inflight = tokio::spawn(async move {
        submit_fg(
            &state_a,
            state_a.storage().as_ref(),
            &owner_principal(),
            "sess_local_alpha",
            "ADV4: x",
            "adv-c4",
        )
        .await
    });
    // run 行出现（持久受理、绑定已承诺）但工具停在 0 许可闸门上——外
    // 部动作尚未发生。
    let run_id = loop {
        let row = query_text(
            &state,
            "SELECT run_id FROM runs WHERE session_id = ?1",
            "sess_local_alpha",
        )
        .await;
        if let Some(run_id) = row {
            break run_id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    };
    assert_eq!(external_count(&counter), 0, "no external effect yet");
    inflight.abort(); // 响应在外部动作之前丢失（drive 一并消亡）
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    // 同 key 重试：replay 真实运行（行存在、可查询），不新增执行。
    let retry = submit_fg(
        &state,
        state.storage().as_ref(),
        &owner_principal(),
        "sess_local_alpha",
        "ADV4: x",
        "adv-c4",
    )
    .await
    .expect("the retry replays the REAL durably-admitted run");
    assert!(retry.replayed);
    assert_eq!(retry.run_id, run_id);
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);
    assert_eq!(
        external_count(&counter),
        0,
        "still no external effect — nothing re-executed"
    );
    assert_eq!(
        run_status(&state, &run_id).await.as_deref(),
        Some("running"),
        "the replayed run is the honest dangling-active row (recovery owns it)"
    );

    // 真实可管理性对照：取消该运行得到诚实的 DanglingActive（无活驱
    // 动），而不是伪造终态。
    let storage = Arc::clone(state.storage());
    match state
        .sessions()
        .cancel_run_for(storage.as_ref(), state.runs(), &owner_principal(), &run_id)
        .await
    {
        Ok(lingxi_service::CancelRunOutcome::DanglingActive { run_id: id, .. }) => {
            assert_eq!(id, run_id)
        }
        other => panic!("the dangling run reports DanglingActive, got {other:?}"),
    }

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── C05 adversarial: restart with UNKNOWN side effects ─────────────────────

/// 攻击窗口：进程 A 的运行停在工具 I/O（外部动作已 +1、journal
/// started、run 仍 active——典型的未知副作用崩溃形态）→ 进程死亡 → 进程
/// B 在同一数据根上重启（恢复扫描诚实收束为 interrupted）→ 同 key 提交
/// 得到显式拒绝并指向该运行（不盲重做、外部计数不增）；新 key 正常
/// 受理。不声称对任意外部系统 exactly-once。
#[tokio::test]
async fn adv_c05_restart_with_unknown_side_effects_refuses_explicitly() {
    let home = synthetic_home("adv-c5");
    let counter = counter_path("adv-c5");

    let tool_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider_a = ScriptedProvider::new(vec![("ADV5", scripted_steps())]);
    let tool_a = Arc::new(PostEffectParkedTool {
        counter: counter.clone(),
        gate: Arc::clone(&tool_gate),
    }) as Arc<dyn lingxi_kernel::ports::ToolExecutorPort>;
    let state_a = boot_on(&home, provider_a, Some(tool_a), CancelPolicy::default()).await;

    // 前台提交（驻留任务）：外部 +1 已发生、journal started、驱动停在
    // 外部系统响应上、run active。
    let state_for_task = state_a.clone();
    let task = tokio::spawn(async move {
        submit_fg(
            &state_for_task,
            state_for_task.storage().as_ref(),
            &owner_principal(),
            "sess_local_alpha",
            "ADV5: x",
            "adv-c5",
        )
        .await
    });
    wait_until(
        "the external side effect happened (unknown-outcome shape)",
        || external_count(&counter) == 1,
    )
    .await;
    let run_id = query_text(
        &state_a,
        "SELECT run_id FROM runs WHERE session_id = ?1",
        "sess_local_alpha",
    )
    .await
    .expect("the active run row exists");
    assert_eq!(
        run_status(&state_a, &run_id).await.as_deref(),
        Some("running")
    );
    // 进程死亡：关闭存储（驱动任务永久停在外部等待上，不再产生写）。
    state_a.storage().close().await.expect("close A");
    drop(state_a);

    // 进程 B：同一数据根上的全新服务（bootstrap 运行恢复扫描）。
    let provider_b = ScriptedProvider::new(vec![("ADV5", final_only())]);
    let state_b = boot_on(&home, provider_b, None, CancelPolicy::default()).await;
    let report = state_b.recovery_report().expect("the startup scan ran");
    assert_eq!(report.scanned, 1, "the dangling-active run was scanned");
    assert!(
        report.outcomes.iter().any(|o| o.run_id == run_id
            && o.settlement
                == lingxi_service::recovery::RecoverySettlement::Written {
                    target: lingxi_protocol::RunStatus::InterruptedNeedsAttention,
                    newly_committed: true,
                }),
        "the killed run settles interrupted_needs_attention (no fabricated success): {:?}",
        report.outcomes
    );

    // 同 key 重试：显式拒绝并指向既有运行——不盲重做。
    match submit_fg(
        &state_b,
        state_b.storage().as_ref(),
        &owner_principal(),
        "sess_local_alpha",
        "ADV5: x",
        "adv-c5",
    )
    .await
    {
        Err(SessionExecuteError::RequestIdBoundToEarlierRun {
            request_id,
            run_id: bound,
        }) => {
            assert_eq!(request_id, "adv-c5");
            assert_eq!(bound, run_id, "the refusal names the earlier run");
        }
        other => panic!("post-restart same-key must be an explicit refusal, got {other:?}"),
    }
    assert_eq!(session_runs(&state_b, "sess_local_alpha").await, 1);
    assert_eq!(
        external_count(&counter),
        1,
        "no blind re-execution of unknown outcomes"
    );

    // 新 key：正常受理（全新验证的提交）。
    let fresh = submit_fg(
        &state_b,
        state_b.storage().as_ref(),
        &owner_principal(),
        "sess_local_alpha",
        "ADV5: x",
        "adv-c5-new",
    )
    .await
    .expect("a NEW id is a fresh validated submission after restart");
    assert!(!fresh.replayed);
    assert!(run_status(&state_b, &fresh.run_id).await.is_some());
    assert_eq!(
        run_status(&state_b, &fresh.run_id).await.as_deref(),
        Some("completed"),
        "the fresh post-restart run settles for real"
    );

    // 悬挂的进程 A 驱动任务不再被 poll；清理。
    task.abort();
    teardown(&state_b, &home).await;
    let _ = std::fs::remove_file(&counter);
}
