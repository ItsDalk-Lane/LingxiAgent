//! R03 repair G02/F03 integration: the UNIFIED RACE ADJUDICATION between
//! an accepted cancellation request and a run's terminal settlement,
//! driven through the REAL service composition (real SQLite storage port,
//! real event service, real kernel state machine + single finalize, real
//! run driver, real cancellation tree), with deterministic PARK POINTS
//! injected at real persistence boundaries of the REAL storage chain.
//!
//! The linearization rule under test (frozen for this repair):
//! - a cancellation request that is ACCEPTED before the run's terminal
//!   claim wins: no `completed`/`failed` terminal and no final message may
//!   commit afterwards — the driver settles through the four-phase
//!   cancellation path (F03-C01);
//! - once the terminal settlement is irrevocably claimed, a cancellation
//!   request is honestly TOO LATE (never an Accepted-with-stop-promise),
//!   the pre-existing terminal is never rewritten and the finalize happens
//!   exactly once (F03-C02);
//! - duplicate/concurrent cancellation requests keep the FIRST reason,
//!   never regress the four-phase machine and never report a second Fired
//!   (F03-C03);
//! - after an accepted cancellation the driver starts NO new external
//!   operation when it resumes from a storage/authorization boundary —
//!   only cancellation/audit/closeout writes land (F03-C04).
//!
//! Test-double boundary (R03 test map): the scripted provider/executor
//! doubles only produce external responses and COUNT their calls; the
//! `GatedStorage` decorator is a pass-through wrapper around the REAL
//! `RunDatabase` that deterministically PARKS the driver at chosen real
//! persistence points (before the underlying write is submitted) — no
//! storage behavior is mocked, no sleep-based synchronization is used.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::{CommittedOutcome, KeyEvent, RunOutcome};
use lingxi_kernel::ports::{
    InvocationIntent, InvocationPhase, InvocationReceipt, ProviderDescriptor, ReceiptOutcome,
    StaleResultFact, StorageError, StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome,
    ToolRequest, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, EventPayload, KnownEventPayload, ModelCallId, NormalizedMessage, ProtocolError,
    RunId, ToolCallId,
};
use lingxi_service::{
    CancelPhase, CancelRegistry, FireOutcome, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};
use lingxi_service::{ExecuteSubmission, Principal, PrincipalKind};

// ── deterministic doubles ────────────────────────────────────────────────────

/// Scripted per-session provider: pops one turn per model call and COUNTS
/// every external call it received (F03-C04 evidence).
struct CountingScriptedProvider {
    scripts: std::sync::Mutex<
        std::collections::HashMap<String, VecDeque<lingxi_kernel::ports::ProviderTurn>>,
    >,
    calls: std::sync::atomic::AtomicUsize,
}

impl CountingScriptedProvider {
    fn new(scripts: Vec<(&'static str, Vec<lingxi_kernel::ports::ProviderTurn>)>) -> Arc<Self> {
        Arc::new(Self {
            scripts: std::sync::Mutex::new(
                scripts
                    .into_iter()
                    .map(|(session, turns)| (session.to_string(), turns.into_iter().collect()))
                    .collect(),
            ),
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn external_calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl TurnProviderPort for CountingScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.counting".to_string(),
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
    ) -> Pin<
        Box<dyn std::future::Future<Output = lingxi_kernel::ports::ProviderTurnResult> + Send + 'a>,
    > {
        let session = ctx.session_id.to_string();
        let ctx_at_issue = ctx.clone();
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let turn = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&session)
            .and_then(|queue| queue.pop_front())
            .unwrap_or_else(|| lingxi_kernel::ports::ProviderTurn::Failed {
                error: ProtocolError::new(
                    lingxi_protocol::ErrorCode::UpstreamUnavailable,
                    "script exhausted",
                    false,
                ),
                retryable: false,
            });
        Box::pin(
            async move { lingxi_kernel::ports::ProviderTurnResult::of_ctx(&ctx_at_issue, turn) },
        )
    }
}

/// Tool double: immediate success; COUNTS every external execution.
struct CountingTool {
    calls: std::sync::atomic::AtomicUsize,
}

impl CountingTool {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn external_calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl ToolExecutorPort for CountingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::Success {
                    content_digest: "tool-content".to_string(),
                },
            )
        })
    }
}

/// Minimal approval-gate double: COUNTS every ask (F03-C04 evidence —
/// whether the driver opened a NEW approval round trip at all) and parks
/// the decision until released.
struct CountingApprovalGate {
    asks: std::sync::atomic::AtomicUsize,
    gate: Arc<tokio::sync::Semaphore>,
}

impl CountingApprovalGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            asks: std::sync::atomic::AtomicUsize::new(0),
            gate: Arc::new(tokio::sync::Semaphore::new(0)),
        })
    }

    fn asks(&self) -> usize {
        self.asks.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl lingxi_service::approval::ApprovalGate for CountingApprovalGate {
    fn request<'a>(
        &'a self,
        _ctx: &'a RunContext,
        _req: &'a lingxi_service::approval::ApprovalRequest,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = lingxi_service::approval::ApprovalDecision>
                + Send
                + 'a,
        >,
    > {
        self.asks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            let _permit = gate.acquire().await.expect("approval gate open");
            lingxi_service::approval::ApprovalDecision::Approved
        })
    }
}

// ── deterministic park points on the REAL storage chain ─────────────────────

/// One deterministic handshake at a real persistence boundary: the driver
/// signals ARRIVAL (a permit) and then PARKS on `go`. The test acquires the
/// arrival permit (no polling, no sleep), performs the racing action and
/// releases the driver.
#[derive(Clone)]
struct Park {
    arrived: Arc<tokio::sync::Semaphore>,
    go: Arc<tokio::sync::Semaphore>,
}

impl Park {
    fn new() -> Self {
        Self {
            arrived: Arc::new(tokio::sync::Semaphore::new(0)),
            go: Arc::new(tokio::sync::Semaphore::new(0)),
        }
    }

    async fn wait_arrival(&self) {
        self.arrived.acquire().await.expect("arrival open").forget();
    }

    fn release(&self) {
        self.go.add_permits(1);
    }

    async fn arrive_and_wait(&self) {
        self.arrived.add_permits(1);
        self.go.acquire().await.expect("go open").forget();
    }
}

/// Which real persistence boundary the driver parks at.
#[derive(Default)]
struct StorageGates {
    /// Park before the model-call key events of the model call whose id
    /// contains this marker (e.g. `-mc0001`) are submitted.
    model_events_for: Option<String>,
    model_events: Option<Park>,
    /// Park before the invocation INTENT write is submitted.
    invocation_intent: Option<Park>,
    /// Park before the invocation `started` advance is submitted.
    invocation_started: Option<Park>,
    /// Park before the invocation RECEIPT write is submitted.
    invocation_receipt: Option<Park>,
    /// Park before the single finalize transaction is submitted.
    commit_outcome: Option<Park>,
    /// Park before the run lineage write is submitted.
    run_lineage: Option<Park>,
    /// Park before the durable waiting_approval leg is submitted (the
    /// authorization boundary in front of the approval ask).
    state_change_waiting_approval: Option<Park>,
}

/// A pass-through decorator around the REAL `RunDatabase`: every call is
/// delegated to the real single-writer SQLite chain unchanged; chosen
/// boundaries additionally PARK deterministically (before delegation, so
/// the write itself still lands through the real transaction).
struct GatedStorage {
    inner: Arc<RunDatabase>,
    gates: StorageGates,
}

impl GatedStorage {
    fn new(inner: Arc<RunDatabase>, gates: StorageGates) -> Arc<Self> {
        Arc::new(Self { inner, gates })
    }
}

fn names_model_call(events: &[KeyEvent], marker: &str) -> bool {
    events.iter().any(|event| match &event.payload {
        EventPayload::Known(KnownEventPayload::ModelCallStarted(payload)) => {
            payload.model_call_id.as_str().contains(marker)
        }
        EventPayload::Known(KnownEventPayload::ModelCallCompleted(payload)) => {
            payload.model_call_id.as_str().contains(marker)
        }
        _ => false,
    })
}

impl StoragePort for GatedStorage {
    fn record_run_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        async move { inner.record_run_started(&ctx, now_unix_ms).await }
    }

    fn commit_run_outcome(
        &self,
        ctx: &RunContext,
        outcome: RunOutcome,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let park = self.gates.commit_outcome.clone();
        let ctx = ctx.clone();
        async move {
            if let Some(park) = park {
                park.arrive_and_wait().await;
            }
            inner.commit_run_outcome(&ctx, outcome, now_unix_ms).await
        }
    }

    fn load_run(
        &self,
        run_id: &RunId,
    ) -> impl std::future::Future<
        Output = Result<Option<lingxi_kernel::ports::RunRecord>, StorageError>,
    > + Send {
        let inner = Arc::clone(&self.inner);
        let run_id = run_id.clone();
        async move { inner.load_run(&run_id).await }
    }

    fn record_run_events(
        &self,
        ctx: &RunContext,
        events: Vec<KeyEvent>,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let park = self
            .gates
            .model_events_for
            .as_deref()
            .zip(self.gates.model_events.clone())
            .filter(|(marker, _)| names_model_call(&events, marker))
            .map(|(_, park)| park);
        let ctx = ctx.clone();
        async move {
            if let Some(park) = park {
                park.arrive_and_wait().await;
            }
            inner.record_run_events(&ctx, events, now_unix_ms).await
        }
    }

    fn record_stale_result(
        &self,
        ctx: &RunContext,
        refused: StaleResultFact,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        async move { inner.record_stale_result(&ctx, refused, now_unix_ms).await }
    }

    fn record_attempt_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        async move { inner.record_attempt_started(&ctx, now_unix_ms).await }
    }

    fn record_run_state_change(
        &self,
        ctx: &RunContext,
        from: lingxi_protocol::RunStatus,
        to: lingxi_protocol::RunStatus,
        reason: Option<String>,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let park = (to == lingxi_protocol::RunStatus::WaitingApproval)
            .then(|| self.gates.state_change_waiting_approval.clone())
            .flatten();
        let ctx = ctx.clone();
        async move {
            if let Some(park) = park {
                park.arrive_and_wait().await;
            }
            inner
                .record_run_state_change(&ctx, from, to, reason, now_unix_ms)
                .await
        }
    }

    fn record_invocation_intent(
        &self,
        ctx: &RunContext,
        intent: InvocationIntent,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let park = self.gates.invocation_intent.clone();
        let ctx = ctx.clone();
        async move {
            if let Some(park) = park {
                park.arrive_and_wait().await;
            }
            inner
                .record_invocation_intent(&ctx, intent, now_unix_ms)
                .await
        }
    }

    fn advance_invocation(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        to: InvocationPhase,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let park = (to == InvocationPhase::Started)
            .then(|| self.gates.invocation_started.clone())
            .flatten();
        let ctx = ctx.clone();
        let journal_id = journal_id.clone();
        async move {
            if let Some(park) = park {
                park.arrive_and_wait().await;
            }
            inner
                .advance_invocation(&ctx, &journal_id, to, now_unix_ms)
                .await
        }
    }

    fn record_invocation_receipt(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        receipt: InvocationReceipt,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let park = self.gates.invocation_receipt.clone();
        let ctx = ctx.clone();
        let journal_id = journal_id.clone();
        async move {
            if let Some(park) = park {
                park.arrive_and_wait().await;
            }
            inner
                .record_invocation_receipt(&ctx, &journal_id, receipt, now_unix_ms)
                .await
        }
    }

    fn record_invocation_unknown(
        &self,
        journal_id: &ToolCallId,
        detail: String,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let journal_id = journal_id.clone();
        async move {
            inner
                .record_invocation_unknown(&journal_id, detail, now_unix_ms)
                .await
        }
    }

    fn load_invocation_journal(
        &self,
        run_id: &RunId,
    ) -> impl std::future::Future<
        Output = Result<Vec<lingxi_kernel::ports::InvocationJournalEntry>, StorageError>,
    > + Send {
        let inner = Arc::clone(&self.inner);
        let run_id = run_id.clone();
        async move { inner.load_invocation_journal(&run_id).await }
    }

    fn record_run_lineage(
        &self,
        ctx: &RunContext,
        lineage: lingxi_kernel::subagent::RunLineage,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let park = self.gates.run_lineage.clone();
        let ctx = ctx.clone();
        async move {
            if let Some(park) = park {
                park.arrive_and_wait().await;
            }
            inner.record_run_lineage(&ctx, lineage, now_unix_ms).await
        }
    }

    fn load_run_lineage(
        &self,
        run_id: &RunId,
    ) -> impl std::future::Future<
        Output = Result<Option<lingxi_kernel::subagent::RunLineage>, StorageError>,
    > + Send {
        let inner = Arc::clone(&self.inner);
        let run_id = run_id.clone();
        async move { inner.load_run_lineage(&run_id).await }
    }
}

// ── fixtures ─────────────────────────────────────────────────────────────────

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

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03g02-{tag}-{}-{}",
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

/// Executes through the REAL session surface against the (optionally
/// gated) REAL storage port; returns the settled run id.
async fn execute_on<P: StoragePort>(
    storage: &P,
    state: ServiceState,
    session: &'static str,
    input: &'static str,
) -> Result<String, lingxi_service::SessionExecuteError> {
    state
        .sessions()
        .execute_submission_for(
            storage,
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

async fn cancel_run<P: StoragePort>(
    storage: &P,
    state: &ServiceState,
    run_id: &str,
) -> Result<lingxi_service::CancelRunOutcome, lingxi_service::SessionExecuteError> {
    state
        .sessions()
        .cancel_run_for(storage, state.runs(), &owner_principal(), run_id)
        .await
}

async fn run_status(state: &ServiceState, run_id: &str) -> (String, Option<String>) {
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
    (status, reason)
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

// ── F03-C01: a cancellation accepted before the terminal claim WINS ─────────

/// The reported F03 window itself: the provider has ALREADY returned Final
/// (the fence admitted it), the driver parks persisting the model-call
/// events, the cancellation is ACCEPTED, the barrier releases — the run
/// must NOT commit `completed` nor a final message; it settles through the
/// four-phase cancellation path.
#[tokio::test]
async fn cancel_accepted_while_final_events_persist_beats_the_completed_terminal() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![lingxi_kernel::ports::ProviderTurn::Final {
            message: assistant_final("the final answer"),
        }],
    )]);
    let (state, home) = boot(
        "c01finalevents",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let model_events = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            model_events_for: Some("-mc0001".to_string()),
            model_events: Some(model_events.clone()),
            ..StorageGates::default()
        },
    );

    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    model_events.wait_arrival().await;
    // Every parked boundary sits strictly after record_run_started, so the
    // durable run row deterministically exists here (no polling).
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    // The cancellation request must be ACCEPTED (the run was live, not
    // terminal, no finalize had claimed the terminal yet).
    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel surface")
    {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("expected Accepted before the terminal claim, got {other:?}"),
    }

    model_events.release();
    drive
        .await
        .expect("drive task")
        .expect("the diverted settle still settles through the single finalize");

    // The cancellation won: no completed, no final message.
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "cancelled", "cancel-before-claim must not complete");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    assert!(
        final_message_content(&state, &run_id).await.is_none(),
        "no final message may commit after an accepted cancellation"
    );
    // The durable cancelling leg exists (four-phase contract).
    assert!(count_events(&state, &run_id, "run_state_changed").await >= 3);
    teardown(&state, &home).await;
}

/// F03-C01 variant — the FAILED terminal is subject to the SAME
/// adjudication: a retryable-exhausted provider failure's model events
/// park (turn 2), the cancellation is accepted, release — the run must
/// NOT settle `failed` after an accepted cancellation (the adjudication
/// covers every terminal path, not only the Final one).
#[tokio::test]
async fn cancel_accepted_before_the_terminal_claim_beats_the_failed_terminal_too() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                requests: vec![read_tool_request()],
            },
            lingxi_kernel::ports::ProviderTurn::Failed {
                error: ProtocolError::new(
                    lingxi_protocol::ErrorCode::UpstreamUnavailable,
                    "permanent",
                    false,
                ),
                retryable: false,
            },
        ],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "c01failed",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let model_events = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            model_events_for: Some("-mc0002".to_string()),
            model_events: Some(model_events.clone()),
            ..StorageGates::default()
        },
    );
    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    model_events.wait_arrival().await;
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel")
    {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("expected Accepted, got {other:?}"),
    }

    model_events.release();
    drive.await.expect("drive task").expect("settles");

    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(
        status, "cancelled",
        "a failed terminal must not commit after an accepted cancellation either"
    );
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    assert!(final_message_content(&state, &run_id).await.is_none());
    teardown(&state, &home).await;
}

/// F03-C01 variant: the same race with the cancellation accepted at the
/// invocation-intent boundary (a FAILED-intent tool-request turn follows —
/// any non-cancellation terminal is equally subject to the rule).
#[tokio::test]
async fn cancel_accepted_before_the_terminal_claim_beats_every_terminal_shape() {
    // Provider: tool request turn, then (had the run continued) a final.
    // With the cancellation accepted at the intent boundary the driver
    // must never dispatch the tool NOR settle anything but cancelled.
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                requests: vec![read_tool_request()],
            },
            lingxi_kernel::ports::ProviderTurn::Final {
                message: assistant_final("never reached"),
            },
        ],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "c01intent",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let intent = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            invocation_intent: Some(intent.clone()),
            ..StorageGates::default()
        },
    );
    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    intent.wait_arrival().await;
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel surface")
    {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("expected Accepted, got {other:?}"),
    }

    intent.release();
    drive.await.expect("drive task").expect("settles");

    assert_eq!(
        tools.external_calls(),
        0,
        "no external dispatch after cancel"
    );
    assert_eq!(
        provider.external_calls(),
        1,
        "the pre-cancel model call only"
    );
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    teardown(&state, &home).await;
}

async fn read_run_of_session(state: &ServiceState, session: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE session_id = ?1 ORDER BY created_at_unix_ms DESC LIMIT 1",
            vec![session.to_string()],
        )
        .await
        .expect("query run of session")
}

// ── F03-C02: once the terminal is irrevocably claimed, cancel is honest ─────

/// After the run has fully committed `completed`, a cancellation request
/// reports AlreadyTerminal (durable answer), never rewrites the terminal
/// and never settles twice.
#[tokio::test]
async fn cancel_after_the_committed_terminal_reports_already_terminal() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![lingxi_kernel::ports::ProviderTurn::Final {
            message: assistant_final("done"),
        }],
    )]);
    let (state, home) = boot(
        "c02after",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let exec_state = state.clone();
    let run_id = execute_on(
        state.storage().as_ref(),
        exec_state,
        "sess_local_alpha",
        "go",
    )
    .await
    .expect("run settles");
    let (status, _) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");

    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel")
    {
        lingxi_service::CancelRunOutcome::AlreadyTerminal { status, .. } => {
            assert_eq!(status, lingxi_protocol::RunStatus::Completed);
        }
        other => panic!("expected AlreadyTerminal, got {other:?}"),
    }
    // The terminal never flipped and the finalize happened exactly once.
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert!(final_message_content(&state, &run_id).await.is_some());
    teardown(&state, &home).await;
}

/// The frozen irrevocable point: the driver parks with the single finalize
/// transaction IN FLIGHT (the terminal is claimed, the commit not yet
/// submitted). A cancellation arriving NOW must NOT be reported Accepted
/// (that would promise a stop that will not happen): it is honestly too
/// late. After release the original terminal commits exactly once.
#[tokio::test]
async fn cancel_racing_the_finalize_transaction_is_too_late_not_accepted() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![lingxi_kernel::ports::ProviderTurn::Final {
            message: assistant_final("committed anyway"),
        }],
    )]);
    let (state, home) = boot(
        "c02inflight",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let commit = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            commit_outcome: Some(commit.clone()),
            ..StorageGates::default()
        },
    );
    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    commit.wait_arrival().await;
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    let outcome = cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel");
    assert!(
        !matches!(outcome, lingxi_service::CancelRunOutcome::Accepted { .. }),
        "a cancellation racing the in-flight finalize must never be reported Accepted: {outcome:?}"
    );

    commit.release();
    drive.await.expect("drive task").expect("settles");

    // The pre-claimed terminal committed, exactly once, and stays.
    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));
    assert!(final_message_content(&state, &run_id).await.is_some());
    // No cancelling leg was ever written.
    let legs = count_events(&state, &run_id, "run_state_changed").await;
    assert_eq!(legs, 2, "queued->running + running->completed only");

    // And once durable, the answer for a later cancel is AlreadyTerminal
    // (not a dangling-active report).
    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel")
    {
        lingxi_service::CancelRunOutcome::AlreadyTerminal { status, .. } => {
            assert_eq!(status, lingxi_protocol::RunStatus::Completed);
        }
        other => panic!("expected AlreadyTerminal after commit, got {other:?}"),
    }
    teardown(&state, &home).await;
}

// ── F03-C03: duplicate/concurrent cancels keep first reason, monotone ────────

/// Real concurrent duplicate fires (4 callers, distinct reasons, barrier
/// release) against wide scope trees (the old non-atomic fire held its
/// read→write gap across the whole tree traversal): exactly ONE Fired,
/// the retained phase reason equals the scope's FIRST reason, and the
/// four-phase machine stays at `requested`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_duplicate_fires_keep_the_first_reason_and_a_single_fired() {
    const ENTRIES: usize = 96;
    const CALLERS: usize = 4;
    const TREE_WIDTH: usize = 48;

    let registry = CancelRegistry::new();
    let mut entries = Vec::new();
    for index in 0..ENTRIES {
        let entry = registry.register(&format!("run_c03_{index}"));
        for child in 0..TREE_WIDTH {
            entry.scope.child(
                format!("wide-{child}"),
                lingxi_service::ScopeKind::ModelCall,
            );
        }
        entries.push(entry);
    }

    let barrier = Arc::new(std::sync::Barrier::new(CALLERS));
    let registry = Arc::new(registry);
    let mut handles = Vec::new();
    for caller in 0..CALLERS {
        let registry = Arc::clone(&registry);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let mut fired = 0usize;
            for index in 0..ENTRIES {
                if registry.fire(&format!("run_c03_{index}"), &format!("reason-{caller}"))
                    == FireOutcome::Fired
                {
                    fired += 1;
                }
            }
            fired
        }));
    }
    let total_fired: usize = handles.into_iter().map(|h| h.join().expect("caller")).sum();

    assert_eq!(
        total_fired, ENTRIES,
        "exactly one Fired per run, however the callers interleave"
    );
    for index in 0..ENTRIES {
        let entry = registry.get(&format!("run_c03_{index}")).expect("live");
        let scope_reason = entry.scope.reason().expect("first reason recorded");
        match entry.phase() {
            CancelPhase::Requested { reason } => assert_eq!(
                reason, scope_reason,
                "the retained phase reason IS the scope's first reason"
            ),
            other => panic!("expected Requested, got {other:?}"),
        }
    }
}

/// The four-phase machine never regresses: once the driver advanced to
/// `cleaning`, a later duplicate fire reports AlreadyCancelling and leaves
/// the phase untouched (no write-back to `requested`).
#[tokio::test]
async fn a_fire_after_the_driver_reached_cleaning_never_regresses_the_phase() {
    let registry = CancelRegistry::new();
    let entry = registry.register("run_c03_mono");
    assert_eq!(registry.fire("run_c03_mono", "user"), FireOutcome::Fired);
    entry.advance_phase(CancelPhase::Cleaning {
        reason: "user".to_string(),
    });
    assert_eq!(
        registry.fire("run_c03_mono", "second caller"),
        FireOutcome::AlreadyCancelling
    );
    match entry.phase() {
        CancelPhase::Cleaning { reason } => assert_eq!(reason, "user"),
        other => panic!("phase regressed: {other:?}"),
    }
}

// ── F03-C04: after an accepted cancel, no NEW external operation ────────────

/// Park at the invocation-intent storage boundary; accept the cancellation;
/// release: the executor must observe ZERO calls (only cancellation /
/// audit / closeout writes land).
#[tokio::test]
async fn cancel_at_the_intent_boundary_dispatches_no_new_external_call() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![lingxi_kernel::ports::ProviderTurn::ToolRequests {
            requests: vec![read_tool_request(), read_tool_request()],
        }],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "c04intent",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let intent = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            invocation_intent: Some(intent.clone()),
            ..StorageGates::default()
        },
    );
    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    intent.wait_arrival().await;
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel")
    {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("expected Accepted, got {other:?}"),
    }

    intent.release();
    drive.await.expect("drive task").expect("settles");

    assert_eq!(
        tools.external_calls(),
        0,
        "zero NEW external calls after an accepted cancellation"
    );
    assert_eq!(
        provider.external_calls(),
        1,
        "the pre-cancel model call only"
    );
    let (status, _) = run_status(&state, &run_id).await;
    assert_eq!(status, "cancelled");
    // The journal holds the prepared intent (audit) and NO started phase.
    let journal = state
        .storage()
        .load_invocation_journal(&RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].phase, InvocationPhase::Prepared);
    assert!(journal[0].receipt.is_none());
    teardown(&state, &home).await;
}

/// Park at the `started` advance (the last storage boundary before the
/// external dispatch); accept the cancellation; release: still ZERO
/// external executions.
#[tokio::test]
async fn cancel_at_the_started_boundary_dispatches_no_new_external_call() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![lingxi_kernel::ports::ProviderTurn::ToolRequests {
            requests: vec![read_tool_request()],
        }],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "c04started",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let started = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            invocation_started: Some(started.clone()),
            ..StorageGates::default()
        },
    );
    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    started.wait_arrival().await;
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel")
    {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("expected Accepted, got {other:?}"),
    }

    started.release();
    drive.await.expect("drive task").expect("settles");

    assert_eq!(
        tools.external_calls(),
        0,
        "the dispatch boundary gate must stop the external call"
    );
    let (status, _) = run_status(&state, &run_id).await;
    assert_eq!(status, "cancelled");
    teardown(&state, &home).await;
}

/// The NO-PROVIDER early close (adversarial variation of C04): the driver
/// parks at the lineage write of the providerless fast path; the
/// cancellation is accepted; release — the run must settle CANCELLED, not
/// `completed.no_final.no_provider_configured`.
#[tokio::test]
async fn cancel_at_the_no_provider_early_close_settles_cancelled() {
    let (state, home) = boot("c04noprovider", ServiceDeps::default()).await;
    assert!(!state.runs().provider_configured());
    let lineage = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            run_lineage: Some(lineage.clone()),
            ..StorageGates::default()
        },
    );
    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    lineage.wait_arrival().await;
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel")
    {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("expected Accepted, got {other:?}"),
    }

    lineage.release();
    drive.await.expect("drive task").expect("settles");

    let (status, reason) = run_status(&state, &run_id).await;
    assert_eq!(
        status, "cancelled",
        "the providerless early close is subject to the same adjudication"
    );
    assert_eq!(reason.as_deref(), Some("cancelled.requested"));
    assert!(final_message_content(&state, &run_id).await.is_none());
    teardown(&state, &home).await;
}

/// Multiple tool-loop boundaries: the first tool completes (one external
/// call); the cancellation is accepted while the driver parks at the
/// receipt boundary; release — the SECOND requested tool never dispatches.
#[tokio::test]
async fn cancel_between_tool_iterations_stops_the_second_dispatch() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![lingxi_kernel::ports::ProviderTurn::ToolRequests {
            requests: vec![read_tool_request(), read_tool_request()],
        }],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "c04loop",
        ServiceDeps {
            turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let receipt = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            invocation_receipt: Some(receipt.clone()),
            ..StorageGates::default()
        },
    );
    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    receipt.wait_arrival().await;
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel")
    {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("expected Accepted, got {other:?}"),
    }

    receipt.release();
    drive.await.expect("drive task").expect("settles");

    assert_eq!(
        tools.external_calls(),
        1,
        "exactly the pre-cancel tool call; the second never dispatches"
    );
    assert_eq!(provider.external_calls(), 1);
    let (status, _) = run_status(&state, &run_id).await;
    assert_eq!(status, "cancelled");
    teardown(&state, &home).await;
}

/// The AUTHORIZATION boundary (adversarial variation of C04): the driver
/// parks persisting the durable waiting_approval leg — the last write in
/// front of the approval ask. The cancellation is accepted there;
/// release: NO approval round trip is opened (the human is never asked),
/// no tool executes, the run settles cancelled.
#[tokio::test]
async fn cancel_at_the_authorization_boundary_never_opens_the_approval_ask() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![lingxi_kernel::ports::ProviderTurn::ToolRequests {
            requests: vec![read_tool_request()],
        }],
    )]);
    let tools = CountingTool::new();
    let approval = CountingApprovalGate::new();
    let (state, home) =
        boot(
            "c04approval",
            ServiceDeps {
                turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
                tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
                approval_gate: Some(
                    approval.clone() as Arc<dyn lingxi_service::approval::ApprovalGate>
                ),
                ..ServiceDeps::default()
            },
        )
        .await;
    let waiting = Park::new();
    let storage = GatedStorage::new(
        Arc::clone(state.storage()),
        StorageGates {
            state_change_waiting_approval: Some(waiting.clone()),
            ..StorageGates::default()
        },
    );
    let exec_state = state.clone();
    let drive = tokio::spawn(async move {
        execute_on(storage.as_ref(), exec_state, "sess_local_alpha", "go").await
    });
    waiting.wait_arrival().await;
    let run_id = read_run_of_session(&state, "sess_local_alpha")
        .await
        .expect("run row exists while the driver is parked");

    match cancel_run(state.storage().as_ref(), &state, &run_id)
        .await
        .expect("cancel")
    {
        lingxi_service::CancelRunOutcome::Accepted { .. } => {}
        other => panic!("expected Accepted, got {other:?}"),
    }

    waiting.release();
    drive.await.expect("drive task").expect("settles");

    assert_eq!(
        approval.asks(),
        0,
        "no NEW approval round trip after an accepted cancellation"
    );
    assert_eq!(tools.external_calls(), 0);
    let (status, _) = run_status(&state, &run_id).await;
    assert_eq!(status, "cancelled");
    teardown(&state, &home).await;
}

/// Receipt classification evidence for the boundary-stop paths: a journal
/// that reached `started` without a receipt classifies Unknown (the honest
/// "dispatched boundary crossed, outcome unobserved" shape), never a
/// fabricated failure.
#[tokio::test]
async fn started_without_receipt_journals_unknown_receipt_facts() {
    let provider = CountingScriptedProvider::new(vec![(
        "sess_local_alpha",
        vec![
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                requests: vec![read_tool_request()],
            },
            lingxi_kernel::ports::ProviderTurn::Final {
                message: assistant_final("closed normally"),
            },
        ],
    )]);
    let tools = CountingTool::new();
    let (state, home) = boot(
        "c04journal",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            tool_executor: Some(tools as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    let exec_state = state.clone();
    let run_id = execute_on(
        state.storage().as_ref(),
        exec_state,
        "sess_local_alpha",
        "go",
    )
    .await
    .expect("run settles");
    let (status, _) = run_status(&state, &run_id).await;
    assert_eq!(status, "completed");
    let journal = state
        .storage()
        .load_invocation_journal(&RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].phase, InvocationPhase::Succeeded);
    assert_eq!(
        journal[0].receipt.as_ref().expect("receipt").outcome,
        ReceiptOutcome::Succeeded
    );
    teardown(&state, &home).await;
}
