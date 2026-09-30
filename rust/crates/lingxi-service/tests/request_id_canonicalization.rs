//! R03 RR2 / F05-01 integration: the CANONICAL requestId contract — one
//! normalized id fact shared by the in-memory dedup key, the run
//! authorization/lineage anchor, the durable cross-restart lookup and the
//! error echo, plus the minimal compatible read for PRE-FIX cause_id rows
//! (and the explicit ambiguity refusal when several historical raw ids
//! normalize to one logical key).
//!
//! Same tier as the G04 suites: REAL composition root (`boot_*`), REAL
//! SQLite run database, REAL supervisor/gate/dedup; the doubles only
//! produce external responses and the controlled external side effect (an
//! append-only counter FILE that survives "restarts"). "Restart" is
//! `state.storage().close()` + drop + a fresh boot on the SAME data root —
//! the in-process registries reset with it.
//!
//! Case map (R03-RR2-F05-C01..C05; C06 belongs to the stage gate):
//! - C01 control group — a PLAIN (no whitespace) id must keep the frozen
//!   G04/F05 cross-restart contract (refusal naming the earlier run, no
//!   re-execution, new ids stay fresh, changed content never overwrites).
//! - C02 the defect's counterexample — " req-42 " is legally accepted in
//!   process A; after a restart BOTH the raw " req-42 " and the canonical
//!   "req-42" retries must resolve to the SAME logical request (no second
//!   effective execution, no second external effect), across the
//!   foreground/background × confirmed/unknown-side-effect matrix.
//! - C03 whitespace/boundary variants — Tab, CRLF, U+3000, U+00A0 and the
//!   length boundary normalize through the REAL `validate_request_id`
//!   rule; one canonical id across admit→persist→restart lookup; illegal
//!   ids are refused before any side effect; normalization never widens
//!   acceptance or truncates content.
//! - C04 concurrency/isolation/compensation — two raw ids that normalize
//!   to ONE key share a single admission namespace (in-flight refusal,
//!   content conflict, capacity refusal, start-transaction failure
//!   compensation); cross-principal/session namespaces stay separate; no
//!   ghost replay.
//! - C05 legacy compatibility — cause_id rows written by the PRE-FIX
//!   build (raw, un-normalized) are still found by the logical key (or
//!   refused as an explicit ambiguity when several raw forms collide);
//!   the frozen lineage rows are never rewritten and nothing is silently
//!   executed as a fresh task.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use lingxi_kernel::ports::{
    CommittedOutcome, InvocationIntent, InvocationPhase, InvocationReceipt, KeyEvent,
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, RunOutcome, StaleResultFact,
    StorageError, StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest,
    TurnProviderPort,
};
use lingxi_kernel::subagent::{RunLineage, RunOrigin};
use lingxi_kernel::{Principal as KernelPrincipal, RunContext};
use lingxi_protocol::{
    ContentBlock, ErrorCode, EventId, EventPayload, KnownEventPayload, ModelCallId,
    NormalizedMessage, ProtocolError, RunId, RunStateChangedPayload, SessionId, ToolCallId,
};

use lingxi_service::{
    cancel::CancelPolicy, prepare_layout, ExecuteAccepted, ExecuteSubmission, HomeSource,
    NetworkMode, ServiceConfig, ServiceDeps, ServiceState, SessionExecuteError,
};

const NOW_MS: u64 = 1_790_409_600_000;
const SESSION: &str = "sess_local_alpha";

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
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.rr2f05".to_string(),
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

/// Tool double whose ONE observable external side effect is an append to a
/// counter FILE (it survives "restarts"). Completes immediately — the
/// CONFIRMED side-effect shape.
struct CountingFileTool {
    counter: PathBuf,
}

impl ToolExecutorPort for CountingFileTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let counter = self.counter.clone();
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            append_external(&counter);
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::Success {
                    content_digest: "counter+1".to_string(),
                },
            )
        })
    }
}

/// Tool double that performs its external effect FIRST and then parks on a
/// 0-permit semaphore — the "external action confirmed, outcome UNKNOWN"
/// shape a killed process leaves behind.
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
                ToolOutcome::Success {
                    content_digest: "counter+1".to_string(),
                },
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
    ToolRequest {
        target: "counter.write".to_string(),
        args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({"op": "bump"})),
        args_summary: Some("counter bump".to_string()),
        delegation: None,
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

/// ONE external tool call, then the final turn (a confirmed side effect).
fn scripted_steps() -> Vec<ProviderTurn> {
    vec![
        ProviderTurn::ToolRequests {
            requests: vec![tool_request()],
        },
        final_turn("rr2f05 done"),
    ]
}

fn final_only() -> Vec<ProviderTurn> {
    vec![final_turn("rr2f05 done without tools")]
}

fn external_count(counter: &Path) -> usize {
    std::fs::read_to_string(counter)
        .map(|raw| raw.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

// ── storage-port decorators (controlled fault points; every write delegates
//    to the REAL database — the recovery_crash_points pattern) ────────────────

/// Injects a failure of the run-start transaction BEFORE it is submitted to
/// the real database (C04: 起始持久化失败补偿).
struct FailingStartPort {
    inner: Arc<lingxi_adapters::storage::RunDatabase>,
    remaining_start_failures: AtomicUsize,
}

impl FailingStartPort {
    async fn start_gate(&self) -> Result<(), StorageError> {
        if self.remaining_start_failures.load(Ordering::SeqCst) > 0 {
            self.remaining_start_failures.fetch_sub(1, Ordering::SeqCst);
            return Err(StorageError::Io {
                detail: "injected run-start transaction failure (RR2/F05 C04: before submit)"
                    .to_string(),
            });
        }
        Ok(())
    }
}

/// Parks INSIDE record_run_started (before delegating to the real database)
/// until released — the deterministic reservation→durable-start window C04
/// interleaves a concurrent colliding-raw-id duplicate into.
struct ParkingStartPort {
    inner: Arc<lingxi_adapters::storage::RunDatabase>,
    release: Arc<AtomicBool>,
    parked: Arc<AtomicBool>,
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

delegate_storage_port!(FailingStartPort);
delegate_storage_port!(ParkingStartPort);

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03-rr2f05-{tag}-{}-{}",
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
        "lingxi-r03-rr2f05-counter-{tag}-{}-{}.txt",
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
        device_id: Some("device_rr2".to_string()),
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Lan,
        credential_kind: lingxi_service::CredentialKind::DeviceCredential,
        trust_state: lingxi_service::TrustState::Lan,
        scopes: vec!["chat".to_string()],
    }
}

/// Boots with the CONFIRMED counting tool bound to `counter`.
async fn boot_counting(
    home: &Path,
    provider: Arc<ScriptedProvider>,
    counter: &Path,
    cancel_policy: CancelPolicy,
) -> ServiceState {
    let layout = prepare_layout(home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(Arc::new(CountingFileTool {
            counter: counter.to_path_buf(),
        })),
        cancel_policy,
        ..ServiceDeps::default()
    };
    ServiceState::bootstrap_with_deps(config_for(home), &layout, deps)
        .await
        .expect("bootstrap")
}

/// Boots with the UNKNOWN-outcome parked tool (external +1 then a 0-permit
/// park) bound to `counter`/`gate`.
async fn boot_parked(
    home: &Path,
    provider: Arc<ScriptedProvider>,
    counter: &Path,
    gate: Arc<tokio::sync::Semaphore>,
    cancel_policy: CancelPolicy,
) -> ServiceState {
    let layout = prepare_layout(home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(Arc::new(PostEffectParkedTool {
            counter: counter.to_path_buf(),
            gate,
        })),
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

async fn submit_fg(
    state: &ServiceState,
    input: &str,
    request_id: &str,
) -> Result<ExecuteAccepted, SessionExecuteError> {
    submit_fg_as(state, &owner_principal(), SESSION, input, request_id).await
}

/// The foreground submission through a CUSTOM storage port (the fault-
/// injection decorators above); the admission/dedup chain is fully real.
async fn submit_fg_port<P: StoragePort>(
    state: &ServiceState,
    port: &P,
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
            SESSION,
            &submission,
            NOW_MS,
        )
        .await
}

async fn submit_fg_as(
    state: &ServiceState,
    principal: &lingxi_service::Principal,
    session: &str,
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
            state.storage().as_ref(),
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
            SESSION,
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

/// The durable request anchor of one run (`run_lineage.cause_id`).
async fn cause_id_of(state: &ServiceState, run_id: &str) -> Option<String> {
    query_text(
        state,
        "SELECT cause_id FROM run_lineage WHERE run_id = ?1",
        run_id,
    )
    .await
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

/// The async-probe variant (storage reads inside the poll).
async fn wait_until_async<F, Fut>(what: &str, mut probe: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !probe().await {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

/// A RunContext for the REAL storage API (the C05 legacy-row fixture):
/// the local owner principal on the seeded session, matching what the
/// submission surface writes for `owner_principal()`.
fn legacy_ctx(run_id: &str) -> RunContext {
    let run = RunId::new(run_id.to_string());
    RunContext {
        principal: KernelPrincipal::LocalUser,
        session_id: SessionId::new(SESSION.to_string()),
        attempt: lingxi_kernel::attempt_id(&run, 1),
        run_id: run,
        generation: 1,
    }
}

/// Creates a PRE-FIX user-run lineage row through the REAL storage API:
/// `record_run_started` + `record_run_lineage` with the RAW (un-normalized)
/// cause_id the baseline build used to write. `settle` completes the run
/// (the normal-stop shape); leaving it unset keeps the row active (the
/// abnormal-death shape recovery then scans).
async fn write_legacy_request_row(
    state: &ServiceState,
    run_id: &str,
    raw_cause_id: &str,
    settle: bool,
) {
    state
        .storage()
        .record_run_started(&legacy_ctx(run_id), NOW_MS)
        .await
        .expect("legacy run started");
    state
        .storage()
        .record_run_lineage(
            &legacy_ctx(run_id),
            RunLineage {
                parent_run_id: None,
                origin: RunOrigin::User,
                source_message_id: None,
                cause_id: Some(raw_cause_id.to_string()),
            },
            NOW_MS,
        )
        .await
        .expect("legacy lineage recorded");
    if settle {
        let ctx = legacy_ctx(run_id);
        state
            .storage()
            .commit_run_outcome(
                &ctx,
                RunOutcome {
                    status: lingxi_protocol::RunStatus::Completed,
                    reason: Some("completed.legacy_fixture".to_string()),
                    key_events: vec![lingxi_kernel::ports::KeyEvent {
                        event_id: EventId::new(format!("{run_id}-done")),
                        payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                            RunStateChangedPayload {
                                from: lingxi_protocol::RunStatus::Running,
                                to: lingxi_protocol::RunStatus::Completed,
                                reason: Some("completed.legacy_fixture".to_string()),
                            },
                        )),
                    }],
                    final_message: None,
                },
                NOW_MS + 1,
            )
            .await
            .expect("legacy run settled");
    }
}

// ── R03-RR2-F05-C01: the no-whitespace control group ───────────────────────

/// 控制组：无空白 req-42 的既有跨重启契约不回退 —— 重启后原 ID 重试得到
/// 指向旧运行的显式拒绝（外部计数不增）；新 key 正常受理；同 key 改内容
/// 不能覆盖原运行；不是无条件拒绝一切。
#[tokio::test]
async fn rr2_f05_c01_plain_id_control_group_keeps_the_restart_contract() {
    let home = synthetic_home("c1");
    let counter = counter_path("c1");

    // 进程 A：无空白 key，确认副作用，正常完成（前台提交在返回前结算）。
    let provider_a = ScriptedProvider::new(vec![("C1", scripted_steps())]);
    let state_a = boot_counting(&home, provider_a, &counter, CancelPolicy::default()).await;
    let first = submit_fg(&state_a, "C1: once", "req-42")
        .await
        .expect("plain id accepted in process A");
    assert_eq!(
        run_status(&state_a, &first.run_id).await.as_deref(),
        Some("completed")
    );
    assert_eq!(external_count(&counter), 1);
    // 控制组锚点：无空白 key 的持久 cause_id 本来就是规范格式。
    assert_eq!(
        cause_id_of(&state_a, &first.run_id).await.as_deref(),
        Some("request:req-42")
    );
    state_a.storage().close().await.expect("close A");
    drop(state_a);

    // 进程 B：同数据根全新服务；原 ID 重试 → 显式拒绝并指向旧运行。
    let provider_b = ScriptedProvider::new(vec![("C1", scripted_steps())]);
    let state_b = boot_counting(&home, provider_b, &counter, CancelPolicy::default()).await;
    match submit_fg(&state_b, "C1: once", "req-42").await {
        Err(SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
            assert_eq!(request_id, "req-42");
            assert_eq!(run_id, first.run_id, "the refusal names the earlier run");
        }
        other => panic!("post-restart same-key must be an explicit refusal, got {other:?}"),
    }
    assert_eq!(session_runs(&state_b, SESSION).await, 1);
    assert_eq!(external_count(&counter), 1, "no re-execution");

    // 同 key 改内容：同样不得静默重做（拒绝/冲突皆可，绝不新执行）。
    match submit_fg(&state_b, "C1: CHANGED", "req-42").await {
        Err(_) => {}
        Ok(accepted) => panic!("changed content must not start a new run, got {accepted:?}"),
    }
    assert_eq!(session_runs(&state_b, SESSION).await, 1);
    assert_eq!(external_count(&counter), 1);

    // 新 key：正常受理并真实完成（不是无条件拒绝一切）。
    let fresh = submit_fg(&state_b, "C1: fresh", "req-43")
        .await
        .expect("a NEW id is a fresh validated submission");
    assert!(!fresh.replayed);
    assert_eq!(
        run_status(&state_b, &fresh.run_id).await.as_deref(),
        Some("completed")
    );
    assert_eq!(external_count(&counter), 2);

    teardown(&state_b, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── R03-RR2-F05-C02: the padded-id counterexample across restarts ───────────

/// 本缺陷的确定性反例：首请求 " req-42 "（当前合法接受、外部计数 +1）→
/// 同数据根新服务 → 原样 " req-42 " 与规范化 "req-42" 两种重试都必须识别
/// 同一逻辑请求：不创建第二有效执行、不重复计数（指向旧事实的显式拒绝
/// 或明确待核验均可，静默全新执行不行）。
#[tokio::test]
async fn rr2_f05_c02_padded_id_raw_and_canonical_retries_bind_one_logical_request() {
    let home = synthetic_home("c2");
    let counter = counter_path("c2");

    // 进程 A：带首尾空格的 ID 被合法接受，确认副作用 +1，正常完成。
    let provider_a = ScriptedProvider::new(vec![("C2", scripted_steps())]);
    let state_a = boot_counting(&home, provider_a, &counter, CancelPolicy::default()).await;
    let first = submit_fg(&state_a, "C2: once", " req-42 ")
        .await
        .expect("the padded id is legally accepted in process A");
    assert_eq!(
        run_status(&state_a, &first.run_id).await.as_deref(),
        Some("completed")
    );
    assert_eq!(external_count(&counter), 1);
    // 修复后：保存的 cause_id 必须是规范 ID 拼出的锚点（不是原始串）。
    assert_eq!(
        cause_id_of(&state_a, &first.run_id).await.as_deref(),
        Some("request:req-42"),
        "the durable anchor must be built from the CANONICAL id"
    );
    state_a.storage().close().await.expect("close A");
    drop(state_a);

    // 进程 B：同数据根全新服务（内存去重表随进程 A 消失）。
    let provider_b = ScriptedProvider::new(vec![("C2", scripted_steps())]);
    let state_b = boot_counting(&home, provider_b, &counter, CancelPolicy::default()).await;

    // 原样重试 " req-42 "：同一逻辑请求 → 显式拒绝指向旧运行，不盲重做。
    match submit_fg(&state_b, "C2: once", " req-42 ").await {
        Err(SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
            assert_eq!(request_id, "req-42", "the refusal echoes the CANONICAL id");
            assert_eq!(run_id, first.run_id, "the refusal names process A's run");
        }
        other => panic!("raw padded retry must resolve to the earlier run, got {other:?}"),
    }
    // 规范化重试 "req-42"：同一逻辑请求，同样不创建第二执行。
    match submit_fg(&state_b, "C2: once", "req-42").await {
        Err(SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
            assert_eq!(request_id, "req-42");
            assert_eq!(run_id, first.run_id);
        }
        other => panic!("canonical retry must resolve to the earlier run, got {other:?}"),
    }
    assert_eq!(session_runs(&state_b, SESSION).await, 1);
    assert_eq!(external_count(&counter), 1, "no second external effect");

    teardown(&state_b, &home).await;
    let _ = std::fs::remove_file(&counter);
}

/// 对抗矩阵：前台/后台 × 确认副作用/未知副作用 × 原样/规范化重试。
/// 每格独立隔离夹具（独立数据根 + 独立计数器 + 独立服务实例对）。
#[tokio::test]
async fn rr2_f05_c02_restart_matrix_foreground_background_confirmed_unknown() {
    for foreground in [true, false] {
        for confirmed in [true, false] {
            for raw_retry in [true, false] {
                run_matrix_cell(foreground, confirmed, raw_retry).await;
            }
        }
    }
}

/// One matrix cell: process A admits " req-42 " through the REAL surface
/// (foreground or background; confirmed or unknown side effect), the
/// process dies, process B boots on the SAME data root and retries the
/// logical key (raw or canonical form) — the retry must bind the earlier
/// run, never re-execute, never grow the external counter.
async fn run_matrix_cell(foreground: bool, confirmed: bool, raw_retry: bool) {
    let tag = format!("c2m-{}-{}-{}", foreground, confirmed, raw_retry);
    let home = synthetic_home(&tag);
    let counter = counter_path(&tag);
    let marker: &'static str = "C2M";

    // 进程 A：按格子选择工具形态（确认=计数即回；未知=+1 后停驻）。
    let state_a = if confirmed {
        let provider = ScriptedProvider::new(vec![(marker, scripted_steps())]);
        boot_counting(&home, provider, &counter, CancelPolicy::default()).await
    } else {
        let provider = ScriptedProvider::new(vec![(marker, scripted_steps())]);
        boot_parked(
            &home,
            provider,
            &counter,
            Arc::new(tokio::sync::Semaphore::new(0)),
            CancelPolicy::default(),
        )
        .await
    };
    let first = if foreground {
        if confirmed {
            submit_fg(&state_a, "C2M: once", " req-42 ")
                .await
                .expect("fg confirmed first submission completes")
        } else {
            // 前台 + 停驻工具：提交停在驱动内部（任务句柄持有服务克隆，
            // 进程死亡后不再被 poll）。durable run 行即本格子的旧事实。
            let state_t = state_a.clone();
            let _parked =
                tokio::spawn(async move { submit_fg(&state_t, "C2M: once", " req-42 ").await });
            wait_until("the unknown side effect happened (fg)", || {
                external_count(&counter) == 1
            })
            .await;
            parked_row(&state_a).await
        }
    } else {
        let accepted = submit_bg(&state_a, "C2M: once", " req-42 ")
            .await
            .expect("bg first submission is accepted");
        if confirmed {
            wait_until_async("the bg confirmed run settles", || {
                let state = state_a.clone();
                let run = accepted.run_id.clone();
                async move { matches!(run_status(&state, &run).await.as_deref(), Some("completed")) }
            })
            .await;
        } else {
            wait_until("the unknown side effect happened (bg)", || {
                external_count(&counter) == 1
            })
            .await;
        }
        accepted
    };
    assert_eq!(external_count(&counter), 1, "exactly one external effect");
    // 进程死亡（未知副作用格：驱动永久停在外部等待上，不再产生写）。
    state_a.storage().close().await.expect("close A");
    drop(state_a);

    // 进程 B：同数据根全新服务（重试必须被拒绝，工具形态仅为反证信号）。
    let state_b = if confirmed {
        let provider = ScriptedProvider::new(vec![(marker, scripted_steps())]);
        boot_counting(&home, provider, &counter, CancelPolicy::default()).await
    } else {
        let provider = ScriptedProvider::new(vec![(marker, scripted_steps())]);
        boot_parked(
            &home,
            provider,
            &counter,
            Arc::new(tokio::sync::Semaphore::new(0)),
            CancelPolicy::default(),
        )
        .await
    };

    let retry_id = if raw_retry { " req-42 " } else { "req-42" };
    let retry = if foreground {
        submit_fg(&state_b, "C2M: once", retry_id).await
    } else {
        submit_bg(&state_b, "C2M: once", retry_id).await
    };
    match retry {
        // 合法契约：指向旧运行的显式拒绝（或待核验形态），绝不全新执行。
        Err(SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
            assert_eq!(request_id, "req-42", "the echo is the canonical id");
            assert_eq!(run_id, first.run_id, "the refusal names process A's run");
        }
        Ok(accepted) => panic!(
            "cell fg={foreground} confirmed={confirmed} raw={raw_retry}: a fresh acceptance \
             would be a silent blind re-execution: {accepted:?}"
        ),
        Err(other) => panic!(
            "cell fg={foreground} confirmed={confirmed} raw={raw_retry}: unexpected refusal \
             shape for the same logical request: {other:?}"
        ),
    }
    assert_eq!(
        session_runs(&state_b, SESSION).await,
        1,
        "no second effective execution"
    );
    assert_eq!(external_count(&counter), 1, "the counter must not grow");

    teardown(&state_b, &home).await;
    let _ = std::fs::remove_file(&counter);
}

/// Reads process A's single run row id (the parked fg cell never returns a
/// response — the durable row is the fact).
async fn parked_row(state: &ServiceState) -> ExecuteAccepted {
    wait_until_async("the parked fg run row appears", || {
        let state = state.clone();
        async move {
            query_text(
                &state,
                "SELECT run_id FROM runs WHERE session_id = ?1",
                SESSION,
            )
            .await
            .is_some()
        }
    })
    .await;
    let run_id = query_text(
        state,
        "SELECT run_id FROM runs WHERE session_id = ?1",
        SESSION,
    )
    .await
    .expect("the parked run row exists");
    ExecuteAccepted {
        run_id,
        run_count: 1,
        replayed: false,
    }
}

// ── R03-RR2-F05-C03: whitespace/boundary variants of ONE canonical rule ─────

/// 真实 validate_request_id 接受的首尾空白变体（Tab、CRLF、U+3000、
/// U+00A0、混合、长度边界）在受理→落库→重启查询全链共享同一 canonical
/// ID；非法 ID 在副作用前明确拒绝；规范化不扩大接受集也不截断内容。
#[tokio::test]
async fn rr2_f05_c03_whitespace_variants_share_one_canonical_chain() {
    // 非法 ID 在任何副作用前被拒（全空白 / 超长 / 归一后仍超长）。
    {
        let home = synthetic_home("c3-reject");
        let counter = counter_path("c3-reject");
        let provider = ScriptedProvider::new(vec![("C3R", final_only())]);
        let state = boot_counting(&home, provider, &counter, CancelPolicy::default()).await;
        let overlong = "x".repeat(129);
        let trimmed_still_over = format!(" {}", "y".repeat(129));
        for illegal in [
            " \t\u{3000}\u{00A0}",
            overlong.as_str(),
            trimmed_still_over.as_str(),
        ] {
            match submit_fg(&state, "C3R: x", illegal).await {
                Err(SessionExecuteError::InvalidRequestId { .. }) => {}
                other => panic!("illegal id {illegal:?} must be refused loudly, got {other:?}"),
            }
        }
        assert_eq!(
            session_runs(&state, SESSION).await,
            0,
            "no run row may exist"
        );
        assert_eq!(
            external_count(&counter),
            0,
            "no side effect before the refusal"
        );
        teardown(&state, &home).await;
        let _ = std::fs::remove_file(&counter);
    }

    // 合法变体矩阵：每变体受理→检查持久锚点→重启→原样/规范重试。
    let variant_defs: Vec<(String, String)> = vec![
        ("\treq-tab\t".into(), "req-tab".into()),
        ("req-crlf\r\n".into(), "req-crlf".into()),
        ("\u{3000}req-ideo\u{3000}".into(), "req-ideo".into()),
        ("\u{00A0}req-nbsp\u{00A0}".into(), "req-nbsp".into()),
        ("\r\n \t\u{3000}req-mix \u{00A0}".into(), "req-mix".into()),
        // 长度边界：原始 129 字节（1 空白 + 128 内容），归一后 128 合法。
        (format!(" {}", "x".repeat(128)), "x".repeat(128)),
        // 内容不截断：内部空白与长度完整保留。
        (" req-inner  id ".into(), "req-inner  id".into()),
    ];
    for (idx, (raw, canonical)) in variant_defs.iter().enumerate() {
        let tag = format!("c3-v{idx}");
        let home = synthetic_home(&tag);
        let counter = counter_path(&tag);
        let expected_anchor = format!("request:{canonical}");

        // 进程 A：受理并完成（确认副作用 +1）。
        let provider_a = ScriptedProvider::new(vec![("C3V", scripted_steps())]);
        let state_a = boot_counting(&home, provider_a, &counter, CancelPolicy::default()).await;
        let first = submit_fg(&state_a, "C3V: once", raw)
            .await
            .unwrap_or_else(|e| panic!("variant {raw:?} must be accepted, got {e:?}"));
        assert_eq!(
            run_status(&state_a, &first.run_id).await.as_deref(),
            Some("completed")
        );
        // 全链同一 canonical：持久锚点由规范 ID 拼出。
        assert_eq!(
            cause_id_of(&state_a, &first.run_id).await.as_deref(),
            Some(expected_anchor.as_str()),
            "variant {raw:?} must persist the canonical anchor"
        );
        assert_eq!(external_count(&counter), 1);
        state_a.storage().close().await.expect("close A");
        drop(state_a);

        // 进程 B：原样与规范化重试都识别同一逻辑请求。
        let provider_b = ScriptedProvider::new(vec![("C3V", scripted_steps())]);
        let state_b = boot_counting(&home, provider_b, &counter, CancelPolicy::default()).await;
        for retry_id in [raw.as_str(), canonical.as_str()] {
            match submit_fg(&state_b, "C3V: once", retry_id).await {
                Err(SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
                    assert_eq!(&request_id, canonical, "the echo is the canonical id");
                    assert_eq!(run_id, first.run_id);
                }
                other => panic!(
                    "variant {raw:?} retry {retry_id:?} must bind the earlier run, got {other:?}"
                ),
            }
        }
        assert_eq!(session_runs(&state_b, SESSION).await, 1);
        assert_eq!(external_count(&counter), 1);
        teardown(&state_b, &home).await;
        let _ = std::fs::remove_file(&counter);
    }
}

// ── R03-RR2-F05-C05: pre-fix cause_id rows — compatible read ────────────────

/// 修复前旧格式行：cause_id 由原始（未规范化）ID 拼出。修复版加载后用
/// 逻辑 key 重试：旧事实仍能关联（显式拒绝指向它，绝不漏查后当新任
/// 务）；已冻结的 lineage 行不被改写；正常停止（settled 旧行）与异常重
/// 启（遗留 active 行 + 恢复扫描收束）都覆盖。
#[tokio::test]
async fn rr2_f05_c05_legacy_cause_ids_stay_linkable_across_restart_shapes() {
    // ── 形态 1：单一旧格式行（正常停止后的 completed 旧事实）。
    {
        let home = synthetic_home("c5-single");
        let counter = counter_path("c5-single");
        let provider = ScriptedProvider::new(vec![("C5S", scripted_steps())]);
        let state_a = boot_counting(&home, provider, &counter, CancelPolicy::default()).await;
        // 用真实存储 API 造出旧代码会写的值：cause_id = "request: req-42 "。
        write_legacy_request_row(&state_a, "run_legacy_c5s", "request: req-42 ", true).await;
        state_a.storage().close().await.expect("close A");
        drop(state_a);

        let provider_b = ScriptedProvider::new(vec![("C5S", scripted_steps())]);
        let state_b = boot_counting(&home, provider_b, &counter, CancelPolicy::default()).await;
        // 逻辑 key 重试：旧事实仍能关联 —— 显式拒绝指向旧运行。
        match submit_fg(&state_b, "C5S: once", "req-42").await {
            Err(SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
                assert_eq!(request_id, "req-42");
                assert_eq!(run_id, "run_legacy_c5s", "the legacy row is still linkable");
            }
            other => panic!("legacy single row must be found by the logical key, got {other:?}"),
        }
        // 不漏查后当新任务：没有第二执行、计数不增。
        assert_eq!(session_runs(&state_b, SESSION).await, 1);
        assert_eq!(external_count(&counter), 0);
        // 不改写已冻结的 lineage 行。
        assert_eq!(
            cause_id_of(&state_b, "run_legacy_c5s").await.as_deref(),
            Some("request: req-42 "),
            "the frozen legacy row must not be rewritten"
        );
        teardown(&state_b, &home).await;
        let _ = std::fs::remove_file(&counter);
    }

    // ── 形态 2：异常重启 —— 遗留 active 旧行由恢复扫描收束后仍能关联。
    {
        let home = synthetic_home("c5-abnormal");
        let counter = counter_path("c5-abnormal");
        let provider = ScriptedProvider::new(vec![("C5X", scripted_steps())]);
        let state_a = boot_counting(&home, provider, &counter, CancelPolicy::default()).await;
        // 旧行保持 active（前一个进程异常死亡的形状），不做任何收束。
        write_legacy_request_row(&state_a, "run_legacy_c5x", "request: req-42 ", false).await;
        // 异常重启：不做优雅停机排空，直接关闭存储句柄（等效进程死亡）。
        state_a.storage().close().await.expect("close A (abnormal)");
        drop(state_a);

        let provider_b = ScriptedProvider::new(vec![("C5X", scripted_steps())]);
        let state_b = boot_counting(&home, provider_b, &counter, CancelPolicy::default()).await;
        let report = state_b.recovery_report().expect("the startup scan ran");
        assert!(
            report.outcomes.iter().any(|o| o.run_id == "run_legacy_c5x"),
            "the dangling legacy row is scanned: {:?}",
            report.outcomes
        );
        match submit_fg(&state_b, "C5X: once", "req-42").await {
            Err(SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
                assert_eq!(request_id, "req-42");
                assert_eq!(run_id, "run_legacy_c5x");
            }
            other => panic!("abnormal-restart legacy row must stay linkable, got {other:?}"),
        }
        assert_eq!(external_count(&counter), 0, "nothing silently re-executed");
        teardown(&state_b, &home).await;
        let _ = std::fs::remove_file(&counter);
    }
}

// ── R03-RR2-F05-C04: concurrency / isolation / compensation on the canonical key

/// 两个原始 ID 规范化到同一 key 的进程内语义：固定预留—起始提交窗口内
/// 的同内容并发（in-flight，回显 canonical key）、不同内容冲突、容量不
/// 足拒绝后重试、起始事务失败补偿后重试；跨主体/会话命名空间不串用；
/// 成功响应都有 durable 行（无幽灵 Replay）；结算后另一原始形态重试是
/// replay（绑定不因任何路径被无条件删除）。
#[tokio::test]
async fn rr2_f05_c04_colliding_raw_ids_one_namespace_isolation_and_compensation() {
    let counter = counter_path("c4");
    let provider = ScriptedProvider::new(vec![
        ("C4A", scripted_steps()),
        ("C4B", scripted_steps()),
        ("C4C", scripted_steps()),
        ("C4D", scripted_steps()),
        ("C4E", scripted_steps()),
    ]);
    let cancel_policy = CancelPolicy {
        supervised_task_cap: 4,
        ..CancelPolicy::default()
    };
    let home = synthetic_home("c4");
    let state = boot_counting(&home, provider, &counter, cancel_policy).await;

    // (a) 固定预留—起始提交窗口：首个提交（原始形态 " pad-a "）停在
    // record_run_started 内部，canonical 形态 "pad-a" 的同内容并发不得滑入
    // 第二个受理 —— 显式 in-flight，且回显 canonical key。
    let parked_port = Arc::new(ParkingStartPort {
        inner: Arc::clone(state.storage()),
        release: Arc::new(AtomicBool::new(false)),
        parked: Arc::new(AtomicBool::new(false)),
    });
    let state_a = state.clone();
    let port_a = Arc::clone(&parked_port);
    let first = tokio::spawn(async move {
        submit_fg_port(&state_a, port_a.as_ref(), "C4A: once", " pad-a ").await
    });
    wait_until(
        "the first submission parks between reservation and durable start",
        || parked_port.parked.load(Ordering::SeqCst),
    )
    .await;
    match submit_fg_port(&state, parked_port.as_ref(), "C4A: once", "pad-a").await {
        Err(SessionExecuteError::AdmissionInFlight { request_id }) => {
            assert_eq!(request_id, "pad-a", "the echo is the CANONICAL key");
        }
        other => panic!("colliding-raw concurrent duplicate must be in-flight, got {other:?}"),
    }

    // 跨会话隔离对照（窗口内）：另一会话同 key 自己的命名空间，全新受理。
    let beta = submit_fg_as(
        &state,
        &owner_principal(),
        "sess_local_beta",
        "C4B: other session",
        " pad-a ",
    )
    .await
    .expect("another session owns its id namespace");
    assert!(!beta.replayed);
    assert_ne!(beta.run_id, "");

    // (b) 同 key（另一原始形态 "\tpad-a\t"）不同内容：明确冲突。
    match submit_fg_port(&state, parked_port.as_ref(), "C4A: CHANGED", "\tpad-a\t").await {
        Err(SessionExecuteError::DuplicateRequestConflict { request_id, .. }) => {
            assert_eq!(request_id, "pad-a", "the conflict echoes the canonical key");
        }
        other => panic!("different content must conflict on the canonical key, got {other:?}"),
    }

    // 释放窗口：首个提交真实受理并结算；本命名空间恰好一个有效受理。
    parked_port.release.store(true, Ordering::SeqCst);
    let first = first
        .await
        .expect("task")
        .expect("the parked first submission completes for real");
    assert_eq!(
        run_status(&state, &first.run_id).await.as_deref(),
        Some("completed"),
        "no ghost replay: the accepted run id has a durable row"
    );
    // beta（跨会话对照）与 first 各执行一次 —— SESSION 命名空间只有 first。
    assert_eq!(external_count(&counter), 2);
    assert_eq!(session_runs(&state, SESSION).await, 1);

    // 结算后另一原始形态重试：replay 同一真实运行（绑定未被无条件删除）。
    let replay = submit_fg(&state, "C4A: once", "\u{3000}pad-a\u{3000}")
        .await
        .expect("post-settle retry through a third raw spelling replays");
    assert!(replay.replayed);
    assert_eq!(replay.run_id, first.run_id);
    assert_eq!(external_count(&counter), 2);

    // 跨主体隔离对照（结算后）：同会话另一主体（device）同 key —— 自己
    // 的命名空间，全新受理，不 replay/不冲突 owner 的运行。
    let device = submit_fg_as(
        &state,
        &device_principal_of_local_user(),
        SESSION,
        "C4E: device",
        " pad-a ",
    )
    .await
    .expect("another principal kind owns its id namespace");
    assert!(!device.replayed);
    assert_ne!(device.run_id, first.run_id);
    assert_eq!(session_runs(&state, SESSION).await, 2);

    // (c) 起始事务失败补偿：原始形态 "\tpad-b\t" 首提失败（注入）→ 无
    // durable 事实可安全撤销；canonical 形态 "pad-b" 重试全新受理并真实
    // 启动（最终只一个有效执行）。
    let failing = FailingStartPort {
        inner: Arc::clone(state.storage()),
        remaining_start_failures: AtomicUsize::new(1),
    };
    match submit_fg_port(&state, &failing, "C4C: start me", "\tpad-b\t").await {
        Err(SessionExecuteError::Storage(StorageError::Io { detail })) => {
            assert!(detail.contains("injected"), "detail: {detail}");
        }
        other => panic!("start-transaction failure must surface loudly, got {other:?}"),
    }
    assert_eq!(
        session_runs(&state, SESSION).await,
        2,
        "no run row was written"
    );
    let retry_b = submit_fg(&state, "C4C: start me", "pad-b")
        .await
        .expect("canonical retry after the compensated failure re-admits for real");
    assert!(!retry_b.replayed);
    assert_eq!(
        run_status(&state, &retry_b.run_id).await.as_deref(),
        Some("completed"),
        "no ghost replay: the retried run id has a durable row"
    );
    assert_eq!(external_count(&counter), 4);
    assert_eq!(session_runs(&state, SESSION).await, 3);

    // (d) 容量不足：占满真实受监督容量，后台提交原始形态 " pad-c " → 显
    // 式拒绝且预留被撤销；释放容量后 canonical 形态 "pad-c" 重试是全新受
    // 理（不残留 digest 冲突）并真实执行。
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let mut fillers = Vec::new();
    for n in 0..4 {
        let gate = Arc::clone(&gate);
        fillers.push(
            state
                .runs()
                .task_supervisor()
                .spawn_detached(format!("c4-filler-{n}"), async move {
                    let _ = gate.acquire().await;
                })
                .expect("filler spawn"),
        );
    }
    match submit_bg(&state, "C4D: bg work", " pad-c ").await {
        Err(SessionExecuteError::BackgroundRegistryFull { cap }) => assert_eq!(cap, 4),
        other => panic!("capacity refusal must be loud, got {other:?}"),
    }
    assert_eq!(session_runs(&state, SESSION).await, 3);
    gate.add_permits(4);
    for filler in fillers {
        filler.wait().await.expect("filler exits cleanly");
    }
    let retry_c = submit_bg(&state, "C4D: bg work", "pad-c")
        .await
        .expect("canonical retry after the capacity release re-admits");
    assert!(
        !retry_c.replayed,
        "the retracted reservation left no sticky conflict"
    );
    wait_until_async("the background retry settles with a durable row", || {
        let state = state.clone();
        let run = retry_c.run_id.clone();
        async move { matches!(run_status(&state, &run).await.as_deref(), Some("completed")) }
    })
    .await;
    assert_eq!(external_count(&counter), 5);

    teardown(&state, &home).await;
    let _ = std::fs::remove_file(&counter);
}

// ── R03-RR2-F05-C05 (adversarial): colliding legacy rows → explicit ambiguity

/// 多个历史原始值归一到同一逻辑 key 的冲突样本：修复版加载后用任一原
/// 始/规范形态重试 → 显式歧义并列出全部绑定运行（不挑最后一条、不自
/// 动重跑）；计数不增；已冻结 lineage 行不被改写。
#[tokio::test]
async fn rr2_f05_c05_colliding_legacy_rows_refuse_as_explicit_ambiguity() {
    let home = synthetic_home("c5-ambig");
    let counter = counter_path("c5-ambig");
    let provider = ScriptedProvider::new(vec![("C5A", scripted_steps())]);
    let state_a = boot_counting(&home, provider, &counter, CancelPolicy::default()).await;
    // 两个旧代码写出的原始变体 + 一个规范格式行（修复前重复执行的
    // 真实形状），全部归一到逻辑 key "req-42"。
    write_legacy_request_row(&state_a, "run_legacy_a1", "request: req-42 ", false).await;
    write_legacy_request_row(&state_a, "run_legacy_a2", "request:\treq-42", false).await;
    write_legacy_request_row(
        &state_a,
        "run_legacy_a3",
        "request:\u{3000}req-42\u{3000}",
        false,
    )
    .await;
    state_a.storage().close().await.expect("close A");
    drop(state_a);

    let provider_b = ScriptedProvider::new(vec![("C5A", scripted_steps())]);
    let state_b = boot_counting(&home, provider_b, &counter, CancelPolicy::default()).await;
    // 逻辑 key 的任一形态（规范 / 原始）重试都是同一逻辑请求 → 显式歧义。
    for retry_id in ["req-42", " req-42 ", "\treq-42"] {
        match submit_fg(&state_b, "C5A: once", retry_id).await {
            Err(SessionExecuteError::RequestIdBoundAmbiguous {
                request_id,
                run_ids,
            }) => {
                assert_eq!(request_id, "req-42", "the echo is the canonical id");
                let mut named = run_ids;
                named.sort();
                assert_eq!(
                    named,
                    vec![
                        "run_legacy_a1".to_string(),
                        "run_legacy_a2".to_string(),
                        "run_legacy_a3".to_string(),
                    ],
                    "the ambiguity names EVERY bound run — never a silent pick (retry {retry_id:?})"
                );
            }
            other => panic!(
                "colliding legacy rows must refuse as an explicit ambiguity (retry {retry_id:?}), \
                 got {other:?}"
            ),
        }
    }
    // 不自动重跑、不挑一条、计数不增；冻结行保持原值。
    assert_eq!(session_runs(&state_b, SESSION).await, 3);
    assert_eq!(external_count(&counter), 0);
    for (run, frozen) in [
        ("run_legacy_a1", "request: req-42 "),
        ("run_legacy_a2", "request:\treq-42"),
        ("run_legacy_a3", "request:\u{3000}req-42\u{3000}"),
    ] {
        assert_eq!(
            cause_id_of(&state_b, run).await.as_deref(),
            Some(frozen),
            "the frozen legacy row {run} must not be rewritten"
        );
    }
    teardown(&state_b, &home).await;
    let _ = std::fs::remove_file(&counter);
}
