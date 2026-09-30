//! STAGE-REVIEWER-R03-RR1 — independent counterexample re-tests (2026-09-30).
//!
//! Written from scratch by the fresh stage reviewer against the FINAL
//! candidate `ebcbcad1b9fa1c621b315dd33b58f8a2e01ab9eb`. These are NOT the
//! repair suites: they are the reviewer's own minimal reproductions of the
//! seven main counterexamples (dispatch §2) plus ≥6 additional
//! adversarial variations sampled across different F-IDs (dispatch §3.2),
//! driven through the REAL service entry points (SessionStore admission /
//! execute / steer / cancel, RunSupervisor::drive_run, TaskSupervisor,
//! CancelRegistry, real SQLite storage) with doubles that only produce
//! external responses or controlled side effects.
//!
//! This file lives in the reviewer's evidence root; it is compiled inside
//! an isolated /tmp copy of the rust workspace (the repository's product /
//! test / config / gate trees are untouched).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::{
    CommittedOutcome, DelegationRequest, InvocationIntent, InvocationPhase, InvocationReceipt,
    KeyEvent, ProviderDescriptor, ProviderTurn, ProviderTurnResult, RunOutcome, StorageError,
    StoragePort, StaleResultFact, ToolExecutionResult, ToolExecutorPort, ToolOutcome,
    ToolRequest, TurnProviderPort,
};
use lingxi_kernel::ports::{InvocationJournalEntry, RunRecord};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, EventPayload, KnownEventPayload, ModelCallId, NormalizedMessage, ProtocolError,
    RunId, ToolCallId,
};
use lingxi_service::{
    approval, cancel, invocations, prepare_layout, task_supervisor, CancelPolicy, ExecuteSubmission,
    HomeSource, NetworkMode, Principal, PrincipalKind, ServiceConfig, ServiceDeps, ServiceState,
    SessionConcurrencyLimits, SessionExecuteError, SteerOutcome,
};

const NOW_MS: u64 = 1_790_409_600_000;

// ════════════════════════════════ doubles ══════════════════════════════════

/// The reviewer's own scripted provider: keys scripts/gates by the input's
/// MARKER (text before the first ':'), records EVERY turn's full input and
/// signals arrivals (marker, pop) through a channel before any parking.
struct RecProvider {
    scripts: Mutex<HashMap<String, VecDeque<Step>>>,
    gates: HashMap<(String, usize), Arc<tokio::sync::Semaphore>>,
    observed: Mutex<Vec<(String, String, usize, String)>>,
    next_pop: Mutex<HashMap<String, usize>>,
    arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
    entered: Mutex<HashMap<String, usize>>,
}

enum Step {
    Turn(ProviderTurn),
    Dispatch { task: String },
}

impl RecProvider {
    fn new(
        scripts: Vec<(String, Vec<Step>)>,
        gates: Vec<((String, usize), Arc<tokio::sync::Semaphore>)>,
    ) -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<(String, usize)>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Arc::new(Self {
                scripts: Mutex::new(
                    scripts
                        .into_iter()
                        .map(|(m, s)| (m, s.into_iter().collect()))
                        .collect(),
                ),
                gates: gates.into_iter().map(|((m, p), g)| ((m, p), g)).collect(),
                observed: Mutex::new(Vec::new()),
                next_pop: Mutex::new(HashMap::new()),
                arrivals: tx,
                entered: Mutex::new(HashMap::new()),
            }),
            rx,
        )
    }

    fn marker_of(input: &str) -> String {
        input.split(':').next().unwrap_or(input).trim().to_string()
    }

    fn observed(&self) -> Vec<(String, String, usize, String)> {
        self.observed.lock().unwrap().clone()
    }

    /// inputs recorded for one marker, in pop order.
    fn inputs_of(&self, marker: &str) -> Vec<String> {
        self.observed()
            .into_iter()
            .filter(|(m, _, _, _)| m == marker)
            .map(|(_, _, _, input)| input)
            .collect()
    }

    /// how many recorded turn inputs contain a needle (exactly-once probes).
    fn occurrences_of(&self, needle: &str) -> usize {
        self.observed()
            .into_iter()
            .filter(|(_, _, _, input)| input.contains(needle))
            .count()
    }

    fn entered_count(&self, marker: &str) -> usize {
        self.entered.lock().unwrap().get(marker).copied().unwrap_or(0)
    }
}

impl TurnProviderPort for RecProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.rr1".to_string(),
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
        {
            let mut entered = self.entered.lock().unwrap();
            *entered.entry(marker.clone()).or_insert(0) += 1;
        }
        let pop = {
            let mut pops = self.next_pop.lock().unwrap();
            let next = pops.get(&marker).copied().unwrap_or(0) + 1;
            pops.insert(marker.clone(), next);
            next
        };
        self.arrivals.send((marker.clone(), pop)).expect("rx held");
        self.observed
            .lock()
            .unwrap()
            .push((marker.clone(), ctx.run_id.to_string(), pop, input.to_string()));
        let gate = self.gates.get(&(marker.clone(), pop)).cloned();
        let step = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&marker)
            .and_then(|q| q.pop_front());
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _ = gate.acquire().await.expect("provider gate closed");
            }
            let turn = match step {
                Some(Step::Turn(t)) => t,
                Some(Step::Dispatch { task }) => ProviderTurn::ToolRequests {
                    requests: vec![ToolRequest {
                        target: "subagent".to_string(),
                        args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({
                            "task": task,
                        })),
                        args_summary: Some(format!("subagent: {task}")),
                        delegation: Some(DelegationRequest {
                            task,
                            access: None,
                            label: None,
                            agent_id: None,
                            model: None,
                            thread_id: None,
                        }),
                    }],
                },
                None => ProviderTurn::Failed {
                    error: ProtocolError::new(
                        lingxi_protocol::ErrorCode::UpstreamUnavailable,
                        "rr1: script exhausted",
                        false,
                    ),
                    retryable: false,
                },
            };
            ProviderTurnResult::of_ctx(&ctx_at_issue, turn)
        })
    }
}

/// Tool double that increments an EXTERNAL counter (a real file write +
/// an atomic), then panics before returning any outcome.
struct SideEffectThenPanicTool {
    counter: Arc<AtomicUsize>,
    file: PathBuf,
}

impl ToolExecutorPort for SideEffectThenPanicTool {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        // The external side effect: durable, independent of this process.
        let n = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        std::fs::write(&self.file, format!("side-effect #{n}")).expect("external write");
        Box::pin(async { panic!("rr1: tool runner died AFTER the external side effect") })
    }
}

/// Tool double returning a TRUSTED external failure receipt (kept for the
/// crosstalk-free vocabulary; V4 uses its own recorder inline).
struct TrustedFailTool;

impl ToolExecutorPort for TrustedFailTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::Failed {
                    error: ProtocolError::new(
                        lingxi_protocol::ErrorCode::UpstreamUnavailable,
                        "rr1: external system refused (trusted failure)",
                        false,
                    ),
                },
            )
        })
    }
}

#[allow(dead_code)]
fn _trusted_fail_tool_type_assert(_t: &TrustedFailTool) {}

/// Approval double: returns scripted decisions in order, parks later asks.
struct ScriptedGate {
    scripted: Mutex<VecDeque<approval::ApprovalDecision>>,
    asks: Arc<AtomicUsize>,
}

impl ScriptedGate {
    fn with(decisions: Vec<approval::ApprovalDecision>) -> Arc<Self> {
        Arc::new(Self {
            scripted: Mutex::new(decisions.into()),
            asks: Arc::new(AtomicUsize::new(0)),
        })
    }
}

impl approval::ApprovalGate for ScriptedGate {
    fn request<'a>(
        &'a self,
        _ctx: &'a RunContext,
        _req: &'a approval::ApprovalRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = approval::ApprovalDecision> + Send + 'a>> {
        self.asks.fetch_add(1, Ordering::SeqCst);
        let decision = self.scripted.lock().unwrap().pop_front();
        Box::pin(async move {
            match decision {
                Some(d) => d,
                None => approval::ApprovalDecision::Aborted,
            }
        })
    }
}

// ── deterministic park points on the REAL storage chain ─────────────────────

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
        self.arrived.acquire().await.expect("arrival").forget();
    }

    fn release(&self) {
        self.go.add_permits(1);
    }

    async fn arrive_and_wait(&self) {
        self.arrived.add_permits(1);
        self.go.acquire().await.expect("go").forget();
    }
}

/// The reviewer's own pass-through decorator over the real RunDatabase:
/// parks before chosen boundaries (arrival → test acts → release → the
/// write still goes through the real single-writer chain), and can fail
/// the FIRST run-start once (the F05-C02 window).
struct GatedStore {
    inner: Arc<RunDatabase>,
    park_model_events: Option<Park>,
    park_commit_outcome: Option<Park>,
    fail_first_run_start: AtomicBool,
}

impl GatedStore {
    fn new(inner: Arc<RunDatabase>) -> Arc<Self> {
        Self::build(inner, None, None)
    }

    fn build(
        inner: Arc<RunDatabase>,
        park_model_events: Option<Park>,
        park_commit_outcome: Option<Park>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner,
            park_model_events,
            park_commit_outcome,
            fail_first_run_start: AtomicBool::new(false),
        })
    }
}

fn names_mc0001(events: &[KeyEvent]) -> bool {
    events.iter().any(|event| match &event.payload {
        EventPayload::Known(KnownEventPayload::ModelCallStarted(p)) => {
            p.model_call_id.as_str().contains("-mc0001")
        }
        EventPayload::Known(KnownEventPayload::ModelCallCompleted(p)) => {
            p.model_call_id.as_str().contains("-mc0001")
        }
        _ => false,
    })
}

impl StoragePort for GatedStore {
    fn record_run_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        let fail = self
            .fail_first_run_start
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        async move {
            if fail {
                return Err(StorageError::Io {
                    detail: "rr1: injected first run-start failure".to_string(),
                });
            }
            inner.record_run_started(&ctx, now_unix_ms).await
        }
    }

    fn commit_run_outcome(
        &self,
        ctx: &RunContext,
        outcome: RunOutcome,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let park = self.park_commit_outcome.clone();
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
    ) -> impl std::future::Future<Output = Result<Option<RunRecord>, StorageError>> + Send {
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
        let park = self.park_model_events.clone();
        let hit = names_mc0001(&events);
        let ctx = ctx.clone();
        async move {
            if hit {
                if let Some(park) = park {
                    park.arrive_and_wait().await;
                }
            }
            inner.record_run_events(&ctx, events, now_unix_ms).await
        }
    }

    fn record_stale_result(
        &self,
        ctx: &RunContext,
        fact: StaleResultFact,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        async move { inner.record_stale_result(&ctx, fact, now_unix_ms).await }
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
        let ctx = ctx.clone();
        async move {
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
        let ctx = ctx.clone();
        async move { inner.record_invocation_intent(&ctx, intent, now_unix_ms).await }
    }

    fn advance_invocation(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        phase: InvocationPhase,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        let journal_id = journal_id.clone();
        async move { inner.advance_invocation(&ctx, &journal_id, phase, now_unix_ms).await }
    }

    fn record_invocation_receipt(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        receipt: InvocationReceipt,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send {
        let inner = Arc::clone(&self.inner);
        let ctx = ctx.clone();
        let journal_id = journal_id.clone();
        async move {
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
        async move { inner.record_invocation_unknown(&journal_id, detail, now_unix_ms).await }
    }

    fn load_invocation_journal(
        &self,
        run_id: &RunId,
    ) -> impl std::future::Future<Output = Result<Vec<InvocationJournalEntry>, StorageError>> + Send
    {
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
        let ctx = ctx.clone();
        async move { inner.record_run_lineage(&ctx, lineage, now_unix_ms).await }
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

// ════════════════════════════════ harness ══════════════════════════════════

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-rr1-stagereview-{tag}-{}-{}",
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
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("addr"),
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

fn deps_for(
    provider: Arc<RecProvider>,
    tools: Option<Arc<dyn ToolExecutorPort>>,
    approval_gate: Option<Arc<dyn approval::ApprovalGate>>,
    subagent_policy: lingxi_kernel::subagent::SubagentPolicy,
    cancel_policy: CancelPolicy,
    session_concurrency: SessionConcurrencyLimits,
) -> ServiceDeps {
    ServiceDeps {
        turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
        tool_executor: tools,
        approval_gate,
        subagent_policy,
        cancel_policy,
        session_concurrency,
        ..ServiceDeps::default()
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

async fn q(state: &ServiceState, sql: &str, args: Vec<String>) -> Option<String> {
    state.storage().query_one_text(sql, args).await.expect("query")
}

async fn run_status(state: &ServiceState, run_id: &str) -> Option<String> {
    q(
        state,
        "SELECT status FROM runs WHERE run_id = ?1",
        vec![run_id.to_string()],
    )
    .await
}

async fn runs_count(state: &ServiceState) -> usize {
    q(
        state,
        "SELECT CAST(COUNT(*) AS TEXT) FROM runs",
        vec![],
    )
    .await
    .expect("count")
    .parse::<usize>()
    .expect("number")
}

async fn latest_run_of(state: &ServiceState, session: &str) -> Option<String> {
    q(
        state,
        "SELECT run_id FROM runs WHERE session_id = ?1 ORDER BY rowid DESC LIMIT 1",
        vec![session.to_string()],
    )
    .await
}

/// The latest USER-SUBMITTED run of a session (subagent children share the
/// session id; they are excluded through the lineage table).
async fn latest_user_run_of(state: &ServiceState, session: &str) -> Option<String> {
    q(
        state,
        "SELECT r.run_id FROM runs r WHERE r.session_id = ?1 AND r.run_id NOT IN \
         (SELECT run_id FROM run_lineage WHERE origin = 'subagent') \
         ORDER BY r.rowid DESC LIMIT 1",
        vec![session.to_string()],
    )
    .await
}

async fn final_message_count(state: &ServiceState, run_id: &str) -> usize {
    q(
        state,
        "SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE run_id = ?1",
        vec![run_id.to_string()],
    )
    .await
    .expect("count")
    .parse::<usize>()
    .expect("number")
}

async fn journal_row(state: &ServiceState, journal_id: &str) -> Option<(String, Option<i64>)> {
    // (receipt_outcome, dispatched)
    state
        .storage()
        .query_one_text(
            "SELECT receipt_outcome || '|' || COALESCE(CAST(dispatched AS TEXT), 'NULL') FROM \
             invocation_journal WHERE journal_id = ?1",
            vec![journal_id.to_string()],
        )
        .await
        .expect("journal query")
        .map(|joined| {
            let mut parts = joined.split('|');
            let outcome = parts.next().unwrap_or("").to_string();
            let dispatched = parts
                .next()
                .and_then(|d| d.parse::<i64>().ok());
            (outcome, dispatched)
        })
}

async fn wait_until(what: &str, mut probe: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while !probe() {
        assert!(
            std::time::Instant::now() < deadline,
            "rr1: timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// Bounded wait for a durable status (real DB poll).
async fn await_status(state: &ServiceState, run_id: &str, expected: &str, what: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        if run_status(state, run_id).await.as_deref() == Some(expected) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "rr1: timed out waiting for {what} ({run_id} -> {expected})"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

fn submit_foreground(
    state: &ServiceState,
    session: &str,
    input: &str,
) -> tokio::task::JoinHandle<Result<lingxi_service::ExecuteAccepted, SessionExecuteError>> {
    let state = state.clone();
    let session = session.to_string();
    let input = input.to_string();
    tokio::spawn(async move {
        let storage = Arc::clone(state.storage());
        state
            .sessions()
            .execute_for(
                storage.as_ref(),
                state.events(),
                state.runs(),
                &owner_principal(),
                &session,
                &input,
                NOW_MS,
            )
            .await
    })
}

fn submit_foreground_keyed(
    state: &ServiceState,
    session: &str,
    input: &str,
    request_id: &str,
) -> tokio::task::JoinHandle<Result<lingxi_service::ExecuteAccepted, SessionExecuteError>> {
    let state = state.clone();
    let session = session.to_string();
    let input = input.to_string();
    let request_id = request_id.to_string();
    tokio::spawn(async move {
        let storage = Arc::clone(state.storage());
        let submission = ExecuteSubmission {
            input: &input,
            request_id: Some(&request_id),
        };
        state
            .sessions()
            .execute_submission_for(
                storage.as_ref(),
                state.events(),
                state.runs(),
                &owner_principal(),
                &session,
                &submission,
                NOW_MS,
            )
            .await
    })
}

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

fn turn_final(text: &str) -> Step {
    Step::Turn(ProviderTurn::Final {
        message: NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text { text: text.to_string() }],
            model_call_id: None,
        },
    })
}

fn turn_continue(note: &str) -> Step {
    Step::Turn(ProviderTurn::Continue {
        process_note: note.to_string(),
    })
}

fn tool_request() -> ToolRequest {
    ToolRequest {
        target: "read".to_string(),
        args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({"path": "/tmp/rr1"})),
        args_summary: Some("read /tmp/rr1".to_string()),
        delegation: None,
    }
}

fn step_tool_request() -> Step {
    Step::Turn(ProviderTurn::ToolRequests {
        requests: vec![tool_request()],
    })
}

fn gate() -> Arc<tokio::sync::Semaphore> {
    Arc::new(tokio::sync::Semaphore::new(0))
}

async fn cancel_run(
    state: &ServiceState,
    run_id: &str,
) -> Result<lingxi_service::CancelRunOutcome, SessionExecuteError> {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .cancel_run_for(
            storage.as_ref(),
            state.runs(),
            &owner_principal(),
            run_id,
        )
        .await
}

// ════════════════ MAIN 1 — F01: tree links + post-cancel creation ═══════════

/// Main counterexample ①: real CancelRegistry/CancelScope —
/// `run_root_under`/`register_linked` chains receive the parent's
/// cancellation at every depth; a parent cancelled FIRST and THEN given
/// children (scope child / run_root_under / register_linked) has them
/// INHERIT the cancellation, and a real supervised dispatch under the
/// cancelled tree performs ZERO adapter calls.
#[tokio::test]
async fn rr1_main_f01_linked_tree_and_post_cancel_creation_adapter_zero() {
    let registry = cancel::CancelRegistry::new();
    let parent = registry.register("rr1-p");
    let child = registry.register_linked("rr1-c", &parent.scope);
    let grand = cancel::CancelScope::run_root_under("rr1-g", &child.scope);
    let model = grand.child("rr1-g-mc".to_string(), cancel::ScopeKind::ModelCall);

    assert!(!child.scope.is_cancelled());
    assert!(parent.scope.cancel("user"));
    for (name, c) in [
        ("child", child.scope.is_cancelled()),
        ("grand", grand.is_cancelled()),
        ("model", model.is_cancelled()),
    ] {
        assert!(c, "parent cancel must reach {name}");
    }
    for (name, w) in [
        ("child", child.scope.cancelled()),
        ("grand", grand.cancelled()),
        ("model", model.cancelled()),
    ] {
        tokio::time::timeout(Duration::from_millis(250), w)
            .await
            .unwrap_or_else(|_| panic!("{name} waiter wakes"));
    }
    assert_eq!(grand.reason().as_deref(), Some("user"));

    // Parent cancelled FIRST, then children of every construction entry.
    let registry2 = cancel::CancelRegistry::new();
    let p2 = registry2.register("rr1-p2");
    assert!(p2.scope.cancel("early-bird"));
    let first_at = p2.scope.cancelled_at().expect("moment");
    let late_scope = p2.scope.child("rr1-late-tc".to_string(), cancel::ScopeKind::ToolCall);
    assert!(late_scope.is_cancelled());
    assert_eq!(late_scope.reason().as_deref(), Some("early-bird"));
    assert_eq!(late_scope.cancelled_at(), Some(first_at));
    let late_root = cancel::CancelScope::run_root_under("rr1-late-root", &p2.scope);
    assert!(late_root.is_cancelled());
    let late_entry = registry2.register_linked("rr1-late-entry", &p2.scope);
    assert!(late_entry.scope.is_cancelled());
    match late_entry.phase() {
        cancel::CancelPhase::Requested { reason } => assert_eq!(reason, "early-bird"),
        other => panic!("inherited entry must start Requested, got {other:?}"),
    }

    // ZERO external adapter calls under the cancelled tree: a REAL
    // supervised dispatch whose future would call the adapter.
    let supervisor = Arc::new(task_supervisor::TaskSupervisor::new(8));
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = Arc::clone(&calls);
    let handle = supervisor
        .spawn_linked(
            "rr1-p2",
            &late_root,
            "model_call:rr1-never-dispatched".to_string(),
            async move {
                calls2.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<()>().await;
                #[allow(unreachable_code)]
                ()
            },
        )
        .expect("spawn");
    let exit = handle.wait().await.expect_err("must not run");
    assert_eq!(exit, task_supervisor::TaskExit::Aborted);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "adapter call count must be 0");
}

// ═════ MAIN 2 — F02: same-process repeated parent cancels beyond cap ════════

/// Main counterexample ②: same process, NO restart, NO startup_scan —
/// with per-session/global subagent caps of 2, FOUR rounds of
/// parent-cancel (each parking a child subagent) must return the busy
/// flag, the active counts, the durable rows and the supervisor registry
/// to baseline every time; a final NORMAL dispatch then succeeds.
#[tokio::test]
async fn rr1_main_f02_same_process_parent_cancels_beyond_cap_full_closeout() {
    let (provider, _arrivals) = RecProvider::new(
        vec![
            (
                "PA0".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID0: park".to_string(),
                    },
                    turn_continue("parent parks after dispatch"),
                ],
            ),
            (
                "KID0".to_string(),
                vec![turn_continue("child parked"), turn_final("never")],
            ),
            (
                "PA1".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID1: park".to_string(),
                    },
                    turn_continue("parent parks after dispatch"),
                ],
            ),
            (
                "KID1".to_string(),
                vec![turn_continue("child parked"), turn_final("never")],
            ),
            (
                "PA2".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID2: park".to_string(),
                    },
                    turn_continue("parent parks after dispatch"),
                ],
            ),
            (
                "KID2".to_string(),
                vec![turn_continue("child parked"), turn_final("never")],
            ),
            (
                "PA3".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KID3: park".to_string(),
                    },
                    turn_continue("parent parks after dispatch"),
                ],
            ),
            (
                "KID3".to_string(),
                vec![turn_continue("child parked"), turn_final("never")],
            ),
            (
                "PAF".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KIDOK: quick".to_string(),
                    },
                    turn_final("parent final"),
                ],
            ),
            ("KIDOK".to_string(), vec![turn_final("child done")]),
        ],
        vec![
            (("PA0".to_string(), 2), gate()),
            (("PA1".to_string(), 2), gate()),
            (("PA2".to_string(), 2), gate()),
            (("PA3".to_string(), 2), gate()),
            (("KID0".to_string(), 1), gate()),
            (("KID1".to_string(), 1), gate()),
            (("KID2".to_string(), 1), gate()),
            (("KID3".to_string(), 1), gate()),
        ],
    );
    let policy = lingxi_kernel::subagent::SubagentPolicy {
        per_session_limit: 2,
        global_limit: 2,
        timeout_ms: 60_000,
        ..lingxi_kernel::subagent::SubagentPolicy::default()
    };
    let (state, home) = boot(
        "f02-main",
        deps_for(
            Arc::clone(&provider),
            // A configured executor is required for any ToolRequests turn
            // (the delegation branch itself never calls it here).
            Some(Arc::new(TrustedFailTool) as Arc<dyn ToolExecutorPort>),
            None,
            policy,
            CancelPolicy::default(),
            SessionConcurrencyLimits::default(),
        ),
    )
    .await;

    for round in 0..4 {
        let parent_task = submit_foreground(&state, "sess_local_alpha", &format!("PA{round}: go"));
        // Wait for the child to be dispatched and parked.
        wait_until("child arrival", || {
            provider
                .observed()
                .iter()
                .any(|(m, _, _, _)| m == &format!("KID{round}"))
        })
        .await;
        wait_until("thread busy", || {
            state
                .subagents()
                .threads_of("sess_local_alpha")
                .iter()
                .any(|t| t.busy)
        })
        .await;
        let child = q(
            &state,
            "SELECT run_id FROM run_lineage WHERE origin = 'subagent' ORDER BY rowid DESC LIMIT 1",
            vec![],
        )
        .await
        .expect("child row");
        let parent_run = latest_user_run_of(&state, "sess_local_alpha")
            .await
            .expect("parent row");

        // Cancel the PARENT: the linked tree must settle both, in THIS
        // process, without any restart or startup scan.
        match cancel_run(&state, &parent_run).await {
            Ok(lingxi_service::CancelRunOutcome::Accepted { .. }) => {}
            other => panic!("round {round}: cancel must be Accepted, got {other:?}"),
        }
        parent_task.await.expect("join").expect("parent settles");
        await_status(&state, &parent_run, "cancelled", "parent cancelled").await;
        await_status(&state, &child, "cancelled", "child cancelled").await;
        wait_until("thread frees", || {
            !state
                .subagents()
                .threads_of("sess_local_alpha")
                .iter()
                .any(|t| t.busy)
        })
        .await;
        let (per_session, global) = state.subagents().active_counts("sess_local_alpha");
        assert_eq!((per_session, global), (0, 0), "round {round}: counts to baseline");
        wait_until("supervisor reclaims parent children", || {
            state
                .runs()
                .task_supervisor()
                .tasks()
                .into_iter()
                .filter(|t| t.run_id.as_deref() == Some(parent_run.as_str()))
                .all(|t| t.exit.is_some())
        })
        .await;
        let threads = state.subagents().threads_of("sess_local_alpha");
        let thread = threads
            .iter()
            .find(|t| t.child_run_id.as_deref() == Some(child.as_str()))
            .expect("thread snapshot");
        assert!(!thread.busy, "round {round}: busy cleared");
        assert_eq!(
            thread.last_run_status.as_deref(),
            Some("cancelled"),
            "round {round}: legal terminal recorded"
        );
    }

    // One more NORMAL dispatch succeeds (caps truly returned).
    let final_task = submit_foreground(&state, "sess_local_alpha", "PAF: finish");
    final_task.await.expect("join").expect("final parent ok");
    let child = q(
        &state,
        "SELECT run_id FROM run_lineage WHERE origin = 'subagent' ORDER BY rowid DESC LIMIT 1",
        vec![],
    )
    .await
    .expect("child row");
    await_status(&state, &child, "completed", "normal child completes").await;
    teardown(&state, &home).await;
}

// ═════ MAIN 3 — F03: final-persistence race, single winner ══════════════════

/// Main counterexample ③, arm A — Provider already returned Final; the
/// model-event persistence is parked; the cancellation returns ACCEPTED;
/// after release there is NO completed status and NO final message. Arm B
/// — the finalize commit itself is parked: a cancel during that window is
/// honestly TooLate and the run completes with its final message intact.
#[tokio::test]
async fn rr1_main_f03_final_persistence_cancel_vs_complete_single_winner() {
    // ── Arm A: cancel accepted during model-event persistence wins ──
    {
        let (provider, _arrivals) = RecProvider::new(
            vec![("F3A".to_string(), vec![turn_final("the answer")])],
            vec![],
        );
        let (state, home) = boot(
            "f03-arm-a",
            deps_for(
                Arc::clone(&provider),
                None,
                None,
                lingxi_kernel::subagent::SubagentPolicy::default(),
                CancelPolicy::default(),
                SessionConcurrencyLimits::default(),
            ),
        )
        .await;
        let store = GatedStore::build(
            Arc::clone(state.storage()),
            Some(Park::new()),
            None,
        );
        let park = store.park_model_events.clone().unwrap();

        let state_for_task = state.clone();
        let store_ref = Arc::clone(&store);
        let task = tokio::spawn(async move {
            state_for_task
                .sessions()
                .execute_for(
                    store_ref.as_ref(),
                    state_for_task.events(),
                    state_for_task.runs(),
                    &owner_principal(),
                    "sess_local_alpha",
                    "F3A: race the final persistence",
                    NOW_MS,
                )
                .await
                .expect("arm A settles")
        });
        park.wait_arrival().await;
        let run_id = latest_run_of(&state, "sess_local_alpha")
            .await
            .expect("run row");
        match cancel_run(&state, &run_id).await {
            Ok(lingxi_service::CancelRunOutcome::Accepted { .. }) => {}
            other => panic!("arm A: cancel must be Accepted, got {other:?}"),
        }
        park.release();
        task.await.expect("arm A join");
        assert_eq!(
            run_status(&state, &run_id).await.as_deref(),
            Some("cancelled"),
            "arm A: NO completed after an accepted cancel"
        );
        assert_eq!(
            final_message_count(&state, &run_id).await,
            0,
            "arm A: no final message committed"
        );
        assert_eq!(provider.entered_count("F3A"), 1, "no second provider call");
        teardown(&state, &home).await;
    }

    // ── Arm B: the terminal claim already won → cancel is TooLate ──
    {
        let (provider, _arrivals) = RecProvider::new(
            vec![("F3B".to_string(), vec![turn_final("the real answer")])],
            vec![],
        );
        let (state, home) = boot(
            "f03-arm-b",
            deps_for(
                Arc::clone(&provider),
                None,
                None,
                lingxi_kernel::subagent::SubagentPolicy::default(),
                CancelPolicy::default(),
                SessionConcurrencyLimits::default(),
            ),
        )
        .await;
        let store = GatedStore::build(
            Arc::clone(state.storage()),
            None,
            Some(Park::new()),
        );
        let park = store.park_commit_outcome.clone().unwrap();

        let state_for_task = state.clone();
        let store_ref = Arc::clone(&store);
        let task = tokio::spawn(async move {
            state_for_task
                .sessions()
                .execute_for(
                    store_ref.as_ref(),
                    state_for_task.events(),
                    state_for_task.runs(),
                    &owner_principal(),
                    "sess_local_alpha",
                    "F3B: claim first",
                    NOW_MS,
                )
                .await
                .expect("arm B settles")
        });
        park.wait_arrival().await;
        let run_id = latest_run_of(&state, "sess_local_alpha")
            .await
            .expect("run row");
        match cancel_run(&state, &run_id).await {
            Ok(lingxi_service::CancelRunOutcome::TooLate { .. }) => {}
            other => panic!("arm B: cancel must be TooLate, got {other:?}"),
        }
        park.release();
        task.await.expect("arm B join");
        assert_eq!(
            run_status(&state, &run_id).await.as_deref(),
            Some("completed"),
            "arm B: the claimed terminal completes"
        );
        assert!(
            final_message_count(&state, &run_id).await >= 1,
            "arm B: the final message IS committed"
        );
        match cancel_run(&state, &run_id).await {
            Ok(lingxi_service::CancelRunOutcome::AlreadyTerminal { .. }) => {}
            other => panic!("arm B: post-terminal cancel, got {other:?}"),
        }
        teardown(&state, &home).await;
    }
}

// ═════ MAIN 4 — F04: side effect then panic → Unknown, no redo ══════════════

/// Main counterexample ④: an external counter is incremented by the tool
/// and the tool then PANICS: the durable journal reads UNKNOWN (not
/// failed/succeeded) with dispatched=1, and TWO recovery passes over the
/// journal never re-execute (counter stays 1, decision is
/// needs_attention under the conservative capabilities — never a blind
/// retry).
#[tokio::test]
async fn rr1_main_f04_side_effect_then_panic_unknown_and_recovery_no_redo() {
    let (provider, _arrivals) = RecProvider::new(
        vec![
            ("F4".to_string(), vec![step_tool_request(), turn_final("after tool")]),
        ],
        vec![],
    );
    let counter = Arc::new(AtomicUsize::new(0));
    let effect_file = synthetic_home("f04-external") .join("external-effect.txt");
    std::fs::create_dir_all(effect_file.parent().unwrap()).expect("dirs");
    let tools = Arc::new(SideEffectThenPanicTool {
        counter: Arc::clone(&counter),
        file: effect_file.clone(),
    });
    let (state, home) = boot(
        "f04-main",
        deps_for(
            Arc::clone(&provider),
            Some(tools as Arc<dyn ToolExecutorPort>),
            None,
            lingxi_kernel::subagent::SubagentPolicy::default(),
            CancelPolicy::default(),
            SessionConcurrencyLimits::default(),
        ),
    )
    .await;

    let accepted = submit_foreground(&state, "sess_local_alpha", "F4: run")
        .await
        .expect("join")
        .expect("run settles (tool partial failure vocabulary)");
    await_status(&state, &accepted.run_id, "completed", "run settles").await;

    assert_eq!(counter.load(Ordering::SeqCst), 1, "exactly ONE external side effect");
    assert!(effect_file.exists(), "the external effect file exists");

    let journal_id = format!("{}-tc0001", accepted.run_id);
    let (outcome, dispatched) = journal_row(&state, &journal_id)
        .await
        .expect("journal entry exists");
    assert_eq!(outcome.as_str(), "unknown", "panic after dispatch journals UNKNOWN");
    assert_eq!(dispatched, Some(1), "the dispatch fact is preserved");

    // Two recovery passes over the REAL journal: no redo, idempotent.
    let caps = invocations::ConservativeCapabilities;
    let report1 = invocations::recover_run_invocations(
        state.storage().as_ref(),
        &accepted.run_id,
        &caps,
        NOW_MS + 1,
    )
    .await
    .expect("recovery pass 1");
    let report2 = invocations::recover_run_invocations(
        state.storage().as_ref(),
        &accepted.run_id,
        &caps,
        NOW_MS + 2,
    )
    .await
    .expect("recovery pass 2");
    for (name, report) in [("pass1", &report1), ("pass2", &report2)] {
        assert_eq!(report.entries.len(), 1, "{name}: one journal entry");
        let entry = &report.entries[0];
        assert_eq!(
            entry.decision.name(),
            "needs_attention",
            "{name}: conservative unknown never blindly retried ({:?})",
            entry.decision
        );
    }
    assert_eq!(counter.load(Ordering::SeqCst), 1, "no redo after two recovery passes");
    let (outcome, dispatched) = journal_row(&state, &journal_id)
        .await
        .expect("journal entry still exists");
    assert_eq!(outcome.as_str(), "unknown");
    assert_eq!(dispatched, Some(1));
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(effect_file.parent().unwrap());
}

// ═════ MAIN 5a — F05: refused background dispatch → retry really starts ═════

/// Main counterexample ⑤, arm A: the supervisor registry is FULL (two
/// parked subagent children hold all four slots), so a background
/// submission with requestId=BGK1 fails loudly and leaves NO run row; after
/// the slots are released, the SAME key retry starts a REAL run (no ghost
/// replay of a nonexistent run, no second execution).
#[tokio::test]
async fn rr1_main_f05a_background_spawn_refusal_retry_starts_for_real() {
    let kid1 = gate();
    let kid2 = gate();
    let (provider, _arrivals) = RecProvider::new(
        vec![
            (
                "P1".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KIDP1: park".to_string(),
                    },
                    turn_final("p1 done"),
                ],
            ),
            ("KIDP1".to_string(), vec![turn_continue("kid1 parked"), turn_final("k1")]),
            (
                "P2".to_string(),
                vec![
                    Step::Dispatch {
                        task: "KIDP2: park".to_string(),
                    },
                    turn_final("p2 done"),
                ],
            ),
            ("KIDP2".to_string(), vec![turn_continue("kid2 parked"), turn_final("k2")]),
            ("BGT".to_string(), vec![turn_final("background done")]),
        ],
        vec![
            (("KIDP1".to_string(), 1), Arc::clone(&kid1)),
            (("KIDP2".to_string(), 1), Arc::clone(&kid2)),
        ],
    );
    // Cap 4: each parked child holds 2 slots (its drive + its model call).
    let policy = lingxi_kernel::subagent::SubagentPolicy {
        timeout_ms: 60_000,
        ..lingxi_kernel::subagent::SubagentPolicy::default()
    };
    let (state, home) = boot(
        "f05a-main",
        deps_for(
            Arc::clone(&provider),
            // A configured executor is required for the ToolRequests turn
            // (the delegation path never dispatches to it).
            Some(Arc::new(TrustedFailTool) as Arc<dyn ToolExecutorPort>),
            None,
            policy,
            CancelPolicy {
                supervised_task_cap: 4,
                ..CancelPolicy::default()
            },
            SessionConcurrencyLimits::default(),
        ),
    )
    .await;

    // Fill every supervised slot with two parked subagent children.
    submit_foreground(&state, "sess_local_alpha", "P1: go")
        .await
        .expect("join p1")
        .expect("p1 ok");
    wait_until("kid1 parked", || {
        provider
            .observed()
            .iter()
            .any(|(m, _, _, _)| m == "KIDP1")
    })
    .await;
    submit_foreground(&state, "sess_local_alpha", "P2: go")
        .await
        .expect("join p2")
        .expect("p2 ok");
    wait_until("kid2 parked", || {
        provider
            .observed()
            .iter()
            .any(|(m, _, _, _)| m == "KIDP2")
    })
    .await;
    wait_until("all 4 supervisor slots live", || {
        state
            .runs()
            .task_supervisor()
            .tasks()
            .into_iter()
            .filter(|t| t.exit.is_none())
            .count()
            >= 4
    })
    .await;
    let count_before = runs_count(&state).await;

    // The refused submission: loud failure, zero rows, zero provider calls.
    let refused = submit_background(&state, "sess_local_alpha", "BGT: work", "BGK1").await;
    match &refused {
        Err(SessionExecuteError::BackgroundRegistryFull { cap }) => assert_eq!(*cap, 4),
        other => panic!("expected BackgroundRegistryFull, got {other:?}"),
    }
    assert_eq!(
        runs_count(&state).await,
        count_before,
        "no ghost run row from the refused submission"
    );
    assert_eq!(
        provider.entered_count("BGT"),
        0,
        "zero external work for the refused submission"
    );

    // Release the slots; the SAME key retry must start a REAL run.
    kid1.add_permits(1);
    kid2.add_permits(1);
    wait_until("slots drain", || {
        state
            .runs()
            .task_supervisor()
            .tasks()
            .into_iter()
            .filter(|t| t.exit.is_none())
            .count()
            == 0
    })
    .await;
    let retried = submit_background(&state, "sess_local_alpha", "BGT: work", "BGK1")
        .await
        .expect("retry starts a real run");
    assert!(!retried.replayed, "the retry is a fresh real admission");
    await_status(&state, &retried.run_id, "completed", "retried run completes").await;
    assert_eq!(provider.entered_count("BGT"), 1, "exactly one real execution");
    assert_eq!(runs_count(&state).await, count_before + 1);
    teardown(&state, &home).await;
}

// ═════ MAIN 5b — F05: run-start persistence failure → fresh re-admission ═══

/// Main counterexample ⑤, arm B: the durable run-start transaction fails
/// (injected) AFTER the id reservation: the submission fails loudly, the
/// reservation is compensated (no fake admission), and the same-key retry
/// executes exactly once.
#[tokio::test]
async fn rr1_main_f05b_foreground_start_persistence_failure_retry_fresh() {
    let (provider, _arrivals) = RecProvider::new(
        vec![("F5B".to_string(), vec![turn_final("ok")])],
        vec![],
    );
    let (state, home) = boot(
        "f05b-main",
        deps_for(
            Arc::clone(&provider),
            None,
            None,
            lingxi_kernel::subagent::SubagentPolicy::default(),
            CancelPolicy::default(),
            SessionConcurrencyLimits::default(),
        ),
    )
    .await;
    let store = GatedStore::new(Arc::clone(state.storage()));
    store.fail_first_run_start.store(true, Ordering::SeqCst);

    let count_before = runs_count(&state).await;
    let state_for_task = state.clone();
    let store_ref = Arc::clone(&store);
    let first = tokio::spawn(async move {
        let submission = ExecuteSubmission {
            input: "F5B: try",
            request_id: Some("FGK1"),
        };
        state_for_task
            .sessions()
            .execute_submission_for(
                store_ref.as_ref(),
                state_for_task.events(),
                state_for_task.runs(),
                &owner_principal(),
                "sess_local_alpha",
                &submission,
                NOW_MS,
            )
            .await
    })
    .await
    .expect("join");
    assert!(first.is_err(), "the failed run-start surfaces loudly");
    assert_eq!(
        runs_count(&state).await,
        count_before,
        "no half-committed run row"
    );
    assert_eq!(provider.entered_count("F5B"), 0, "no external work");

    // Same key, storage healthy: exactly one real execution.
    let retried = submit_foreground_keyed(&state, "sess_local_alpha", "F5B: try", "FGK1")
        .await
        .expect("join")
        .expect("retry admitted");
    assert!(!retried.replayed);
    await_status(&state, &retried.run_id, "completed", "retry completes").await;
    assert_eq!(provider.entered_count("F5B"), 1);
    assert_eq!(runs_count(&state).await, count_before + 1);
    teardown(&state, &home).await;
}

// ═════ MAIN 6 — F06: >2000-char payloads arrive complete (both entries) ════

/// Main counterexample ⑥: inputs of 1999/2000/2001/2600+ CHARS (Unicode +
/// CRLF + a tail marker that reverses the head) arrive at the provider
/// BYTE-IDENTICAL through BOTH the foreground and background entries.
#[tokio::test]
async fn rr1_main_f06_in_budget_full_input_arrives_foreground_and_background() {
    let (provider, _arrivals) = RecProvider::new(
        vec![
            ("L1999".to_string(), vec![turn_final("ok")]),
            ("L2000".to_string(), vec![turn_final("ok")]),
            ("L2001".to_string(), vec![turn_final("ok")]),
            ("L2600".to_string(), vec![turn_final("ok")]),
            ("LBG".to_string(), vec![turn_final("ok bg")]),
        ],
        vec![],
    );
    let (state, home) = boot(
        "f06-main",
        deps_for(
            Arc::clone(&provider),
            None,
            None,
            lingxi_kernel::subagent::SubagentPolicy::default(),
            CancelPolicy::default(),
            SessionConcurrencyLimits::default(),
        ),
    )
        .await;

    let build = |marker: &str, target_chars: usize| -> String {
        // Unicode + CRLF mixed body, EXACT char count, with a decisive
        // tail marker AFTER the 2000-char boundary for target > 2000.
        let tail = format!("TAIL_MARKER_{marker}_必须完整到达✓");
        let tail_chars = tail.chars().count();
        let unit: Vec<char> = "数据✅ab\r\n".chars().collect();
        let mut s: Vec<char> = format!("{marker}: ").chars().collect();
        while s.len() + tail_chars + unit.len() <= target_chars {
            s.extend_from_slice(&unit);
        }
        while s.len() + tail_chars < target_chars {
            s.push('x');
        }
        s.extend(tail.chars());
        s.into_iter().collect()
    };

    for (marker, target) in [
        ("L1999", 1999usize),
        ("L2000", 2000usize),
        ("L2001", 2001usize),
        ("L2600", 2600usize),
    ] {
        let input = build(marker, target);
        assert_eq!(input.chars().count(), target, "{marker} exact char count");
        submit_foreground(&state, "sess_local_alpha", &input)
            .await
            .expect("join")
            .unwrap_or_else(|e| panic!("{marker} foreground ok: {e:?}"));
        let recorded = provider.inputs_of(marker);
        assert_eq!(
            recorded.last().map(String::as_str),
            Some(input.as_str()),
            "{marker}: the provider received the FULL byte-identical input"
        );
    }

    // The background entry executes the same full payload.
    let bg_input = build("LBG", 2600);
    let accepted = submit_background(&state, "sess_local_beta", &bg_input, "BG-F06")
        .await
        .expect("background ok");
    await_status(&state, &accepted.run_id, "completed", "bg completes").await;
    let recorded = provider.inputs_of("LBG");
    assert_eq!(
        recorded.last().map(String::as_str),
        Some(bg_input.as_str()),
        "background: the provider received the FULL byte-identical input"
    );
    teardown(&state, &home).await;
}

// ═════ MAIN 7 — F07: background steer consumed exactly once, no crosstalk ══

/// Main counterexample ⑦: two background runs park at their first model
/// turn; a steer on session A returns Accepted; after release, A's NEXT
/// turn's input contains the steered text EXACTLY ONCE, B's never does,
/// and a later new run of A does not receive the already-drained text.
#[tokio::test]
async fn rr1_main_f07_background_steer_consumed_once_no_crosstalk() {
    let gate_a1 = gate();
    let gate_b1 = gate();
    let (provider, _arrivals) = RecProvider::new(
        vec![
            (
                "BGA".to_string(),
                vec![turn_continue("a turn1"), turn_final("a done")],
            ),
            (
                "BGB".to_string(),
                vec![turn_continue("b turn1"), turn_final("b done")],
            ),
            ("AFTER".to_string(), vec![turn_final("after done")]),
        ],
        vec![
            (("BGA".to_string(), 1), Arc::clone(&gate_a1)),
            (("BGB".to_string(), 1), Arc::clone(&gate_b1)),
        ],
    );
    let (state, home) = boot(
        "f07-main",
        deps_for(
            Arc::clone(&provider),
            None,
            None,
            lingxi_kernel::subagent::SubagentPolicy::default(),
            CancelPolicy::default(),
            SessionConcurrencyLimits::default(),
        ),
    )
    .await;

    let a = submit_background(&state, "sess_local_alpha", "BGA: run", "F7A")
        .await
        .expect("bg a");
    let b = submit_background(&state, "sess_local_beta", "BGB: run", "F7B")
        .await
        .expect("bg b");
    wait_until("both parked at turn 1", || {
        provider.inputs_of("BGA").len() == 1 && provider.inputs_of("BGB").len() == 1
    })
    .await;

    let steer_text = "STEER-追加-9Q2-TARGET";
    match state
        .sessions()
        .steer_for(&owner_principal(), "sess_local_alpha", steer_text)
        .await
    {
        Ok(SteerOutcome::Accepted) => {}
        other => panic!("steer must be Accepted, got {other:?}"),
    }

    gate_a1.add_permits(1); // release A's first turn → Continue
    wait_until("A's second turn arrived", || provider.inputs_of("BGA").len() == 2).await;
    let turn2 = provider.inputs_of("BGA")[1].clone();
    assert!(
        turn2.contains(steer_text),
        "the steered text reaches the NEXT model turn:\n{turn2}"
    );
    assert_eq!(
        turn2.matches(steer_text).count(),
        1,
        "the steered text is consumed EXACTLY once"
    );
    await_status(&state, &a.run_id, "completed", "A completes").await;

    // No crosstalk to B, ever.
    gate_b1.add_permits(1);
    await_status(&state, &b.run_id, "completed", "B completes").await;
    assert_eq!(
        provider.occurrences_of(steer_text),
        1,
        "the steered text appears in exactly ONE recorded turn input"
    );

    // A later NEW run of the same session does not receive the drained text.
    submit_foreground(&state, "sess_local_alpha", "AFTER: fresh")
        .await
        .expect("join")
        .expect("after ok");
    assert_eq!(
        provider.occurrences_of(steer_text),
        1,
        "the drained steering never leaks into a later run"
    );
    teardown(&state, &home).await;
}

// ═══════════ VARIANTS — ≥6 different F-IDs (dispatch §3.2) ═════════════════

/// V1 (F01-C03): barrier-started registration racing a cancellation —
/// every successfully attributed node (incl. multi-level and post-
/// traversal joiners) is cancelled and the root's first reason survives.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rr1_var_f01c_registration_vs_cancellation_race_barrier() {
    let registry = cancel::CancelRegistry::new();
    let parent = registry.register("rr1-race");
    let barrier = Arc::new(std::sync::Barrier::new(7));
    let scope = parent.scope.clone();

    let mut joins = Vec::new();
    for racer in 0..6 {
        let barrier = Arc::clone(&barrier);
        let scope = scope.clone();
        joins.push(tokio::task::spawn_blocking(move || {
            barrier.wait();
            let mut leaves = Vec::new();
            for n in 0..3 {
                let child = scope.child(
                    format!("rr1-r{racer}-{n}"),
                    cancel::ScopeKind::ToolCall,
                );
                let _grand = child.child(
                    format!("rr1-r{racer}-{n}-g"),
                    cancel::ScopeKind::ModelCall,
                );
                leaves.push(child);
            }
            leaves
        }));
    }
    barrier.wait();
    assert!(parent.scope.cancel("rr1-first-reason"));
    let mut total = 0usize;
    let mut all = true;
    for join in joins {
        for leaf in join.await.expect("racer") {
            total += 1;
            all &= leaf.is_cancelled();
        }
    }
    assert_eq!(total, 18);
    assert!(all, "no successfully attributed node escapes cancellation");
    assert_eq!(
        parent.scope.reason().as_deref(),
        Some("rr1-first-reason"),
        "first reason never overwritten by the racers"
    );
    let late = parent.scope.child("rr1-race-late".to_string(), cancel::ScopeKind::ToolCall);
    assert!(late.is_cancelled());
}

/// V2 (F02-C02): the never-first-polled window — a tree that fires BEFORE
/// the wrapper polls the child still records the supervised abort and
/// performs ZERO adapter calls.
#[tokio::test(flavor = "current_thread")]
async fn rr1_var_f02b_never_first_polled_closeout() {
    let supervisor = Arc::new(task_supervisor::TaskSupervisor::new(4));
    let root = cancel::CancelScope::run_root("rr1-prepoll");
    let scope = root.child("rr1-prepoll-tc".to_string(), cancel::ScopeKind::ToolCall);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = Arc::clone(&calls);
    let handle = supervisor
        .spawn_linked(
            "rr1-prepoll",
            &scope,
            "tool_call:rr1-never-polled".to_string(),
            async move {
                calls2.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<()>().await;
                #[allow(unreachable_code)]
                ()
            },
        )
        .expect("spawn");
    root.cancel("before-poll");
    let exit = handle.wait().await.expect_err("aborted before first poll");
    assert_eq!(exit, task_supervisor::TaskExit::Aborted);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "zero adapter calls");
}

/// V3 (F03-C03): concurrent duplicate cancels with DIFFERENT reasons —
/// exactly one Fired, the first reason is the scope's reason, the phase
/// never regresses and a terminal claim after the cleaning leg is
/// diverted with the FIRST reason.
#[test]
fn rr1_var_f03c_concurrent_duplicate_cancel_first_reason_no_regress() {
    let registry = cancel::CancelRegistry::new();
    let entry = registry.register("rr1-dup");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let e1 = Arc::clone(&entry);
    let e2 = Arc::clone(&entry);
    let b1 = Arc::clone(&barrier);
    let b2 = Arc::clone(&barrier);
    let t1 = std::thread::spawn(move || {
        b1.wait();
        e1.fire("reason-AAA")
    });
    let t2 = std::thread::spawn(move || {
        b2.wait();
        e2.fire("reason-BBB")
    });
    let o1 = t1.join().expect("t1");
    let o2 = t2.join().expect("t2");
    let outcomes = [o1, o2];
    let fired = outcomes
        .iter()
        .filter(|o| **o == cancel::FireOutcome::Fired)
        .count();
    let already = outcomes
        .iter()
        .filter(|o| **o == cancel::FireOutcome::AlreadyCancelling)
        .count();
    assert_eq!(fired, 1, "exactly one Fired");
    assert_eq!(already, 1, "the other is AlreadyCancelling");
    let scope_reason = entry.scope.reason().expect("reason kept");
    assert!(
        scope_reason == "reason-AAA" || scope_reason == "reason-BBB",
        "first reason is one of the racers"
    );
    match entry.phase() {
        cancel::CancelPhase::Requested { reason } => assert_eq!(reason, scope_reason),
        other => panic!("phase is Requested, got {other:?}"),
    }
    // No phase regression: a late fire during cleaning writes nothing back.
    entry.advance_phase(cancel::CancelPhase::Cleaning {
        reason: scope_reason.clone(),
    });
    assert_eq!(
        entry.fire("late-duplicate"),
        cancel::FireOutcome::AlreadyCancelling
    );
    match entry.phase() {
        cancel::CancelPhase::Cleaning { reason } => assert_eq!(reason, scope_reason),
        other => panic!("phase regressed: {other:?}"),
    }
    match entry.claim_terminal("completed.with_final") {
        cancel::TerminalAdjudication::CancelledBy { reason } => assert_eq!(reason, scope_reason),
        other => panic!("claim diverted with first reason, got {other:?}"),
    }
}

/// V4 (F04-C03): the three durable negative facts stay distinguishable —
/// a pre-authorization refusal keeps dispatched=false, a TRUSTED external
/// failure keeps dispatched=true+failed, and the runner panic keeps
/// unknown (main 4).
#[tokio::test]
async fn rr1_var_f04c_trusted_negatives_dispatched_false_vs_true() {
    let (provider, _arrivals) = RecProvider::new(
        vec![
            (
                "V4R".to_string(),
                vec![step_tool_request(), turn_final("after rejection")],
            ),
            (
                "V4F".to_string(),
                vec![step_tool_request(), turn_final("after trusted failure")],
            ),
        ],
        vec![],
    );
    // The FIRST approval ask belongs to V4R (rejected); the second ask —
    // V4F's — must be APPROVED so the executor really runs and returns the
    // trusted external failure.
    let gate = ScriptedGate::with(vec![
        approval::ApprovalDecision::Rejected {
            reason: "rr1: user said no".to_string(),
        },
        approval::ApprovalDecision::Approved,
    ]);
    struct DispatchRecorderTool {
        arrivals: Arc<AtomicUsize>,
    }
    impl ToolExecutorPort for DispatchRecorderTool {
        fn execute<'a>(
            &'a self,
            ctx: &'a RunContext,
            _c: &'a ToolCallId,
            _r: &'a ToolRequest,
        ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
            self.arrivals.fetch_add(1, Ordering::SeqCst);
            let ctx_at_issue = ctx.clone();
            Box::pin(async move {
                ToolExecutionResult::of_ctx(
                    &ctx_at_issue,
                    ToolOutcome::Failed {
                        error: ProtocolError::new(
                            lingxi_protocol::ErrorCode::UpstreamUnavailable,
                            "rr1: trusted external failure",
                            false,
                        ),
                    },
                )
            })
        }
    }
    let dispatched = Arc::new(AtomicUsize::new(0));
    let tools = Arc::new(DispatchRecorderTool {
        arrivals: Arc::clone(&dispatched),
    });
    let (state, home) = boot(
        "f04c-var",
        deps_for(
            Arc::clone(&provider),
            Some(tools as Arc<dyn ToolExecutorPort>),
            Some(gate as Arc<dyn approval::ApprovalGate>),
            lingxi_kernel::subagent::SubagentPolicy::default(),
            CancelPolicy::default(),
            SessionConcurrencyLimits::default(),
        ),
    )
    .await;

    // (a) approval rejected BEFORE dispatch: failed + dispatched=false.
    let rejected = submit_foreground(&state, "sess_local_alpha", "V4R: run")
        .await
        .expect("join")
        .expect("settles");
    await_status(&state, &rejected.run_id, "completed", "rejected run settles").await;
    let (outcome, dispatched_flag) = journal_row(&state, &format!("{}-tc0001", rejected.run_id))
        .await
        .expect("journal");
    assert_eq!(outcome.as_str(), "failed");
    assert_eq!(dispatched_flag, Some(0), "authorization refusal was never dispatched");

    // (b) trusted external failure receipt: failed + dispatched=true.
    let trusted = submit_foreground(&state, "sess_local_alpha", "V4F: run")
        .await
        .expect("join")
        .expect("settles");
    await_status(&state, &trusted.run_id, "completed", "trusted-fail run settles").await;
    let (outcome, dispatched_flag) = journal_row(&state, &format!("{}-tc0001", trusted.run_id))
        .await
        .expect("journal");
    assert_eq!(outcome.as_str(), "failed");
    assert_eq!(dispatched_flag, Some(1), "the real external failure IS dispatched");
    assert_eq!(dispatched.load(Ordering::SeqCst), 1, "executor ran exactly once (V4F)");
    teardown(&state, &home).await;
}

/// V5 (F05-C03): same key + changed content conflicts loudly (nothing
/// re-executed, no new run); the SAME key + content in a DIFFERENT session
/// is a fresh independent admission (the id namespace is per session).
#[tokio::test]
async fn rr1_var_f05c_same_key_conflict_and_cross_session_namespace() {
    let (provider, _arrivals) = RecProvider::new(
        vec![
            ("V5A".to_string(), vec![turn_final("a")]),
            ("V5B".to_string(), vec![turn_final("b")]),
        ],
        vec![],
    );
    let (state, home) = boot(
        "f05c-var",
        deps_for(
            Arc::clone(&provider),
            None,
            None,
            lingxi_kernel::subagent::SubagentPolicy::default(),
            CancelPolicy::default(),
            SessionConcurrencyLimits::default(),
        ),
    )
    .await;

    let first = submit_foreground_keyed(&state, "sess_local_alpha", "V5A: content", "K5")
        .await
        .expect("join")
        .expect("first ok");
    let count = runs_count(&state).await;

    // Same key, CHANGED content → explicit conflict, zero new runs.
    match submit_foreground_keyed(&state, "sess_local_alpha", "V5B: DIFFERENT", "K5")
        .await
        .expect("join")
    {
        Err(SessionExecuteError::DuplicateRequestConflict { request_id, .. }) => {
            assert_eq!(request_id, "K5")
        }
        other => panic!("expected conflict, got {other:?}"),
    }
    assert_eq!(runs_count(&state).await, count, "conflict adds no run");
    assert_eq!(provider.entered_count("V5B"), 0, "conflict executes nothing");

    // Same key + SAME content on the OTHER session: an independent
    // namespace — a fresh real admission.
    let beta = submit_foreground_keyed(&state, "sess_local_beta", "V5A: content", "K5")
        .await
        .expect("join")
        .expect("beta fresh admission");
    assert!(!beta.replayed);
    assert_ne!(beta.run_id, first.run_id);
    assert_eq!(provider.entered_count("V5A"), 2, "both sessions really ran");
    teardown(&state, &home).await;
}

/// V6 (F05-C05): the cross-restart same-key contract — after a REAL
/// service restart on the same data home (fresh in-process registries,
/// same durable store), the same key is refused EXPLICITLY naming the
/// earlier run, while a NEW key admits normally.
#[tokio::test]
async fn rr1_var_f05e_restart_same_key_explicit_binding() {
    let (provider, _arrivals) = RecProvider::new(
        vec![
            ("V6".to_string(), vec![turn_final("life-1 done")]),
            ("V6N".to_string(), vec![turn_final("life-2 fresh")]),
        ],
        vec![],
    );
    let home = synthetic_home("f05e-var");
    let layout = prepare_layout(&home).expect("layout");
    let deps = deps_for(
        Arc::clone(&provider),
        None,
        None,
        lingxi_kernel::subagent::SubagentPolicy::default(),
        CancelPolicy::default(),
        SessionConcurrencyLimits::default(),
    );
    let state1 = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps.clone())
        .await
        .expect("life-1 bootstrap");
    let first = submit_foreground_keyed(&state1, "sess_local_alpha", "V6: has effects", "RK1")
        .await
        .expect("join")
        .expect("life-1 ok");
    await_status(&state1, &first.run_id, "completed", "life-1 completes").await;
    state1.storage().close().await.expect("close life-1");

    // "Restart": a brand-new service instance, same durable home.
    let state2 = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("life-2 bootstrap");
    match submit_foreground_keyed(&state2, "sess_local_alpha", "V6: has effects", "RK1")
        .await
        .expect("join")
    {
        Err(SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
            assert_eq!(request_id, "RK1");
            assert_eq!(run_id, first.run_id, "the refusal names the EARLIER run");
        }
        other => panic!("restart same-key must be an explicit refusal, got {other:?}"),
    }
    let fresh = submit_foreground_keyed(&state2, "sess_local_alpha", "V6N: fresh key", "RK2")
        .await
        .expect("join")
        .expect("new key admits");
    await_status(&state2, &fresh.run_id, "completed", "life-2 fresh completes").await;
    state2.storage().close().await.expect("close life-2");
    let _ = std::fs::remove_dir_all(&home);
}

/// V7 (F06-C03): an over-budget input is refused EXPLICITLY with zero
/// acceptance and zero side effects on BOTH entries (and the provider is
/// never entered).
#[tokio::test]
async fn rr1_var_f06c_over_budget_explicit_refusal_zero_side_effects() {
    let (provider, _arrivals) = RecProvider::new(
        vec![("TOOBIG".to_string(), vec![turn_final("never")])],
        vec![],
    );
    let (state, home) = boot(
        "f06c-var",
        deps_for(
            Arc::clone(&provider),
            None,
            None,
            lingxi_kernel::subagent::SubagentPolicy::default(),
            CancelPolicy::default(),
            SessionConcurrencyLimits::default(),
        ),
    )
    .await;
    let limit = lingxi_service::limits::MAX_SUBMISSION_INPUT_BYTES;
    let mut oversized = String::from("TOOBIG: ");
    while oversized.len() <= limit {
        oversized.push_str(&"x".repeat(4096));
    }
    assert!(oversized.len() > limit);
    let count = runs_count(&state).await;

    match submit_foreground(&state, "sess_local_alpha", &oversized)
        .await
        .expect("join")
    {
        Err(SessionExecuteError::InputTooLarge { bytes, limit_bytes }) => {
            assert_eq!(bytes, oversized.len());
            assert_eq!(limit_bytes, limit);
        }
        other => panic!("foreground over-budget refusal, got {other:?}"),
    }
    match submit_background(&state, "sess_local_beta", &oversized, "BG-TOOBIG").await {
        Err(SessionExecuteError::InputTooLarge { .. }) => {}
        other => panic!("background over-budget refusal, got {other:?}"),
    }
    assert_eq!(runs_count(&state).await, count, "zero run rows created");
    assert_eq!(provider.entered_count("TOOBIG"), 0, "provider never entered");
    teardown(&state, &home).await;
}

/// V8 (F07-C04): a tiny steering inbox holds the SAME contract for the
/// foreground and background entries: in-capacity texts are really
/// consumed, the over-capacity one is a loud refusal that pollutes
/// nothing.
#[tokio::test]
async fn rr1_var_f07d_steering_capacity_foreground_background_same_contract() {
    let gate_f = gate();
    let gate_bg = gate();
    let (provider, _arrivals) = RecProvider::new(
        vec![
            ("CAPF".to_string(), vec![turn_continue("f1"), turn_final("f done")]),
            ("CAPBG".to_string(), vec![turn_continue("bg1"), turn_final("bg done")]),
        ],
        vec![
            (("CAPF".to_string(), 1), Arc::clone(&gate_f)),
            (("CAPBG".to_string(), 1), Arc::clone(&gate_bg)),
        ],
    );
    let (state, home) = boot(
        "f07d-var",
        deps_for(
            Arc::clone(&provider),
            None,
            None,
            lingxi_kernel::subagent::SubagentPolicy::default(),
            CancelPolicy::default(),
            SessionConcurrencyLimits {
                steering_inbox_capacity: 2,
                ..SessionConcurrencyLimits::default()
            },
        ),
    )
    .await;

    // Foreground arm.
    let f = submit_foreground(&state, "sess_local_alpha", "CAPF: run");
    wait_until("fg parked", || provider.inputs_of("CAPF").len() == 1).await;
    assert_eq!(
        state
            .sessions()
            .steer_for(&owner_principal(), "sess_local_alpha", "CAP-ONE-壹")
            .await
            .expect("steer 1"),
        SteerOutcome::Accepted
    );
    assert_eq!(
        state
            .sessions()
            .steer_for(&owner_principal(), "sess_local_alpha", "CAP-TWO-贰")
            .await
            .expect("steer 2"),
        SteerOutcome::Accepted
    );
    match state
        .sessions()
        .steer_for(&owner_principal(), "sess_local_alpha", "CAP-THREE-叁")
        .await
    {
        Err(SessionExecuteError::SteeringInboxFull) => {}
        other => panic!("3rd steer over capacity must refuse loudly, got {other:?}"),
    }
    gate_f.add_permits(1);
    let accepted = f.await.expect("join").expect("fg ok");
    await_status(&state, &accepted.run_id, "completed", "fg completes").await;
    let turn2 = provider.inputs_of("CAPF")[1].clone();
    assert!(turn2.contains("CAP-ONE-壹") && turn2.contains("CAP-TWO-贰"), "{turn2}");
    assert!(!turn2.contains("CAP-THREE-叁"), "the refused text never polluted the queue");

    // Background arm: the same contract through the other entry.
    let bg = submit_background(&state, "sess_local_beta", "CAPBG: run", "F7CAP")
        .await
        .expect("bg ok");
    wait_until("bg parked", || provider.inputs_of("CAPBG").len() == 1).await;
    assert_eq!(
        state
            .sessions()
            .steer_for(&owner_principal(), "sess_local_beta", "BG-CAP-ONE-甲")
            .await
            .expect("bg steer 1"),
        SteerOutcome::Accepted
    );
    match state
        .sessions()
        .steer_for(&owner_principal(), "sess_local_beta", "BG-CAP-TWO-乙")
        .await
    {
        // capacity is 2: the queued one counts — this second text is
        // within capacity and must be accepted.
        Ok(SteerOutcome::Accepted) => {}
        other => panic!("2nd steer within capacity, got {other:?}"),
    }
    match state
        .sessions()
        .steer_for(&owner_principal(), "sess_local_beta", "BG-CAP-OVER2-丙")
        .await
    {
        Err(SessionExecuteError::SteeringInboxFull) => {}
        other => panic!("bg 3rd steer must refuse, got {other:?}"),
    }
    gate_bg.add_permits(1);
    await_status(&state, &bg.run_id, "completed", "bg completes").await;
    let bg_turn2 = provider.inputs_of("CAPBG")[1].clone();
    assert!(
        bg_turn2.contains("BG-CAP-ONE-甲") && bg_turn2.contains("BG-CAP-TWO-乙"),
        "{bg_turn2}"
    );
    assert!(
        !bg_turn2.contains("BG-CAP-OVER2-丙"),
        "the over-capacity text never leaked"
    );
    teardown(&state, &home).await;
}

// reviewer note: the counter/effect-file cleanups live inside their tests.
