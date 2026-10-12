//! R03 repair G03/F04 integration (`tool_receipt_unknown`): a DISPATCHED
//! tool invocation whose supervised child ends WITHOUT delivering an
//! outcome (panic / forced abort / lost supervision channel) journals as
//! UNKNOWN — never a fabricated confirmed failure, never a success —
//! while the trusted negative facts keep their honest classes:
//!
//! - a NEVER-DISPATCHED refusal (the real approval boundary) closes
//!   `failed` with `dispatched=false` and zero external requests;
//! - a REAL external failure receipt (the external system answered with
//!   a structured failure) stays a trusted `failed` with
//!   `dispatched=true` (never swept into unknown by the fix);
//! - the dispatched-no-receipt runner anomalies stay `unknown` in BOTH
//!   observable worlds (the external side effect happened / did not);
//! - recovery — and REPEATED recovery — never re-executes an unknown
//!   side effect, keeps the unknown visible (in-process and at the
//!   startup scan), and the idempotent control case verifies with the
//!   ORIGINAL key only.
//!
//! Test-double boundary: the tool double only produces the controlled
//! EXTERNAL side effect (a file-backed request log + counter + dedup
//! state under an isolated temp dir) and the controlled runner faults
//! (panics before/after the external action). Every journal write, the
//! supervision exit classification, the fence, the recovery scan and
//! the real SQLite storage chain belong to the production code under
//! test — nothing of that is mocked.
//!
//! Evidence: set `R03_G03_EVIDENCE=<path>.json` to collect the per-case
//! facts (counter values, journal phases, decisions, categories).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::invocation::{RecoveryClass, RecoveryDecision, ToolRecoveryCapability};
use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    InvocationPhase, ProviderDescriptor, ProviderTurnResult, ReceiptOutcome, StoragePort,
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, TurnDeltaSink,
    TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::invocations::{
    classify_run_invocations, recover_run_invocations, RecoveryCapabilitySource,
};
use lingxi_service::{
    approval, invocations::ConservativeCapabilities, ExecuteSubmission, Principal, PrincipalKind,
};
use lingxi_service::{HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState};

// ── the controlled external system (file-backed) ─────────────────────────────

/// Durable external side-effect double: one JSON line per RECEIVED request
/// (`key`, `executed`, `digest`); `state.json` maps key → digest when the
/// key is honored (the idempotent control's dedup memory). The
/// `executed_count` is the external counter the cases assert on.
struct ExternalSystem {
    dir: PathBuf,
    honors_key: bool,
    requests: std::sync::Mutex<Vec<serde_json::Value>>,
}

impl ExternalSystem {
    fn new(dir: &Path, honors_key: bool) -> Arc<Self> {
        std::fs::create_dir_all(dir).expect("mkdir external dir");
        Arc::new(Self {
            dir: dir.to_path_buf(),
            honors_key,
            requests: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// Performs (or dedups) one external operation, durably BEFORE it
    /// returns — the effect survives any local crash/panic, exactly like
    /// a real external system.
    fn perform(&self, key: &str) -> String {
        if self.honors_key {
            let state = self.read_state();
            if let Some(digest) = state.get(key).and_then(|v| v.as_str()) {
                self.append(key, false, digest);
                return digest.to_string();
            }
        }
        let executed_so_far = self.executed_count();
        let digest = format!("digest-{}-{}", executed_so_far + 1, key);
        if self.honors_key {
            let mut state = self.read_state();
            state.insert(key.to_string(), serde_json::Value::String(digest.clone()));
            let path = self.dir.join("state.json");
            std::fs::write(&path, serde_json::to_vec(&state).expect("state json"))
                .expect("durable external state");
        }
        self.append(key, true, &digest);
        digest
    }

    fn append(&self, key: &str, executed: bool, digest: &str) {
        let line = serde_json::json!({
            "key": key,
            "executed": executed,
            "digest": digest,
        });
        self.requests.lock().unwrap().push(line.clone());
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("requests.log"))
            .expect("open request log");
        writeln!(file, "{line}").expect("append request log");
    }

    fn read_state(&self) -> serde_json::Map<String, serde_json::Value> {
        std::fs::read(self.dir.join("state.json"))
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default()
    }

    /// The external counter: operations actually EXECUTED.
    fn executed_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.get("executed").and_then(|v| v.as_bool()) == Some(true))
            .count()
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

/// The controlled runner fault per tool target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// The runner dies BEFORE the external action: nothing external
    /// happened and no result exists ("外部未动作且结果未知").
    PanicBefore,
    /// The runner dies AFTER the durable external action: the side effect
    /// happened and the response is lost ("先外部成功再丢响应").
    PanicAfter,
    /// The external system EXECUTES and answers with a structured
    /// failure — a trustworthy external failure receipt.
    ExternalFail,
    /// The external system executes and succeeds.
    Succeed,
}

/// Tool double: the executor port face of the controlled external system.
/// The panic faults are raised INSIDE the supervised child future (the
/// PanicGuard of the real TaskSupervisor contains them — the containment
/// itself is G01 behavior under test, not mocked away).
struct FaultTool {
    external: Arc<ExternalSystem>,
    faults: std::sync::Mutex<HashMap<String, Fault>>,
    fired: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

impl FaultTool {
    /// Ends the controlled interruption for one target: later executions
    /// answer normally (the crash window is over — the A10 "resumed call
    /// must ANSWER" shape; the external system still dedups by key).
    fn disarm(&self, target: &str) {
        self.faults
            .lock()
            .unwrap()
            .insert(target.to_string(), Fault::Succeed);
    }
}

impl ToolExecutorPort for FaultTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let target = request.target.clone();
        let fault = *self
            .faults
            .lock()
            .unwrap()
            .get(&target)
            .expect("every scripted target has a fault");
        let key = call.to_string();
        let external = Arc::clone(&self.external);
        let fired = self.fired.clone();
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            match fault {
                Fault::PanicBefore => {
                    panic!(
                        "F04 controlled fault: runner died BEFORE the external action \
                         (target {target}, key {key})"
                    );
                }
                Fault::PanicAfter => {
                    let digest = external.perform(&key);
                    if let Some(fired) = fired {
                        let _ = fired.send(key.clone());
                    }
                    panic!(
                        "F04 controlled fault: runner died AFTER the external action \
                         (target {target}, key {key}, digest {digest})"
                    );
                }
                Fault::ExternalFail => {
                    external.perform(&key);
                    if let Some(fired) = fired {
                        let _ = fired.send(key.clone());
                    }
                    ToolExecutionResult::of_ctx(
                        &ctx_at_issue,
                        ToolOutcome::Failed {
                            error: ProtocolError::new(
                                ErrorCode::UpstreamUnavailable,
                                format!("external system refused the {target} operation"),
                                false,
                            ),
                        },
                    )
                }
                Fault::Succeed => {
                    let digest = external.perform(&key);
                    if let Some(fired) = fired {
                        let _ = fired.send(key.clone());
                    }
                    ToolExecutionResult::of_ctx(&ctx_at_issue, ToolOutcome::success_text(digest))
                }
            }
        })
    }
}

// ── providers ────────────────────────────────────────────────────────────────

/// Scripted provider: fixed turns for one session, announcing each arrival.
struct ScriptedProvider {
    turns: Vec<lingxi_kernel::ports::ProviderTurn>,
    arrivals: tokio::sync::mpsc::UnboundedSender<usize>,
    next: std::sync::Mutex<usize>,
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.g03".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        _input: &'a ModelTurnInput,

        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        assert_eq!(
            ctx.session_id.to_string(),
            "sess_local_alpha",
            "one scripted session"
        );
        let step = {
            let mut next = self.next.lock().unwrap();
            *next += 1;
            *next
        };
        let _ = self.arrivals.send(step);
        let ctx_at_issue = ctx.clone();
        let turn = self.turns.get(step - 1).cloned().unwrap_or_else(|| {
            lingxi_kernel::ports::ProviderTurn::Failed {
                error: ProtocolError::new(
                    ErrorCode::UpstreamUnavailable,
                    "script exhausted",
                    false,
                ),
                retryable: false,
            }
        });
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, turn) })
    }
}

/// Provider that emits ONE tool-request turn, then PARKS forever on a
/// semaphore: the run is left dangling-active for the recovery cases
/// (the park is the controlled "process died mid-run" shape; the drive
/// is aborted by the test right after the receipt under test commits).
struct ParkAfterToolsProvider {
    requests: Vec<ToolRequest>,
    gate: Arc<tokio::sync::Semaphore>,
    arrivals: tokio::sync::mpsc::UnboundedSender<usize>,
    next: std::sync::Mutex<usize>,
}

impl TurnProviderPort for ParkAfterToolsProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.g03.park".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        _input: &'a ModelTurnInput,

        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        assert_eq!(
            ctx.session_id.to_string(),
            "sess_local_alpha",
            "one scripted session"
        );
        let step = {
            let mut next = self.next.lock().unwrap();
            *next += 1;
            *next
        };
        let _ = self.arrivals.send(step);
        let ctx_at_issue = ctx.clone();
        let requests = self.requests.clone();
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            if step == 1 {
                return ProviderTurnResult::of_ctx(
                    &ctx_at_issue,
                    lingxi_kernel::ports::ProviderTurn::ToolRequests {
                        requests,
                        content: Vec::new(),
                    },
                );
            }
            // Turn 2+ parks: the process "dies" here from the run's view.
            let _permit = gate.acquire().await;
            ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                lingxi_kernel::ports::ProviderTurn::Final {
                    message: assistant_final("never reached"),
                },
            )
        })
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn tool_request(target: &str) -> ToolRequest {
    ToolRequest::from_effective_arguments(
        target,
        serde_json::json!({
            "target": target, "payload": "fixed",
        }),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request")
    .with_summary(format!("{target} fixed payload"))
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
        "lingxi-r03g03-{tag}-{}-{}",
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

async fn execute_plain(
    state: ServiceState,
    session: &'static str,
    input: &'static str,
) -> Result<String, lingxi_service::SessionExecuteError> {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_submission_for(
            storage.as_ref(),
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

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
}

async fn wait_until(max_ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(max_ms);
    while !cond() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    true
}

fn write_evidence(key: &str, value: serde_json::Value) {
    if let Ok(path) = std::env::var("R03_G03_EVIDENCE") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut doc = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(map) = doc.as_object_mut() {
            map.insert(key.to_string(), value);
        }
        let _ = std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap());
    }
}

async fn load_journal(
    state: &ServiceState,
    run_id: &str,
) -> Vec<lingxi_kernel::ports::InvocationJournalEntry> {
    state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.to_string()))
        .await
        .expect("journal load")
}

/// The capability source of the idempotent control case: the ledger
/// double VERIFIABLY honors the idempotency key (its dedup behavior is
/// asserted by the test itself); everything else stays conservative.
struct IdemLedgerCapabilities;

impl RecoveryCapabilitySource for IdemLedgerCapabilities {
    fn capability_of(&self, target: &str) -> ToolRecoveryCapability {
        if target == "ledger.idem" {
            ToolRecoveryCapability {
                honors_idempotency_key: true,
                ..ToolRecoveryCapability::CONSERVATIVE
            }
        } else {
            ToolRecoveryCapability::CONSERVATIVE
        }
    }
}

// ── F04-C01: 副作用后 panic 保留 Unknown ─────────────────────────────────────

/// The controlled external counter starts at 0; the tool adds 1, then the
/// runner panics BEFORE any receipt exists. Through the REAL supervisor
/// and journal: the external counter reads 1; the receipt is UNKNOWN
/// (not a confirmed failure, not a success); the error source is
/// diagnosable (the TaskExit panic payload lands in the receipt detail);
/// the idempotency key survives for later verification; and the run's
/// own terminal does not mask the unclear call (the run completes with
/// its final answer while the invocation stays unknown).
#[tokio::test(flavor = "current_thread")]
async fn f04_c01_panic_after_side_effect_journals_unknown_not_confirmed_failure() {
    let (arrivals_tx, arrivals_rx) = tokio::sync::mpsc::unbounded_channel();
    let _hold_arrivals = arrivals_rx;
    let (fired_tx, mut fired) = tokio::sync::mpsc::unbounded_channel();
    let external_dir = synthetic_home("c01-external");
    let external = ExternalSystem::new(&external_dir, false);
    let provider = Arc::new(ScriptedProvider {
        turns: vec![
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                content: Vec::new(),
                requests: vec![tool_request("notify.fault")],
            },
            lingxi_kernel::ports::ProviderTurn::Final {
                message: assistant_final("final after the panicking call"),
            },
        ],
        arrivals: arrivals_tx,
        next: std::sync::Mutex::new(0),
    });
    let (state, home) = boot(
        "c01-after",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(Arc::new(FaultTool {
                external: Arc::clone(&external),
                faults: std::sync::Mutex::new(HashMap::from([(
                    "notify.fault".to_string(),
                    Fault::PanicAfter,
                )])),
                fired: Some(fired_tx),
            })),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_plain(state.clone(), "sess_local_alpha", "c01 panic after")
        .await
        .expect("the run survives the panicking tool call");
    let executed_key = fired.recv().await.expect("external executed");
    assert_eq!(executed_key, format!("{run_id}-tc0001"));

    // The external side effect HAPPENED: counter 1.
    assert_eq!(
        external.executed_count(),
        1,
        "the side effect fired exactly once (C01: 外部计数=1)"
    );
    assert_eq!(external.request_count(), 1);

    // The journal receipt is UNKNOWN — never a confirmed failure.
    let journal = load_journal(&state, &run_id).await;
    assert_eq!(journal.len(), 1);
    assert_eq!(
        journal[0].phase,
        InvocationPhase::Unknown,
        "a dispatched-no-receipt panic must journal unknown, got {:?}",
        journal[0].phase
    );
    let receipt = journal[0].receipt.as_ref().expect("receipt");
    assert_eq!(receipt.outcome, ReceiptOutcome::Unknown);
    assert!(
        receipt.dispatched,
        "the execution WAS dispatched (started is durable)"
    );
    assert!(
        receipt.detail.contains("panicked")
            && receipt
                .detail
                .contains("F04 controlled fault: runner died AFTER"),
        "the runner-level error source is diagnosable in the receipt: {}",
        receipt.detail
    );
    assert!(
        receipt.dedup_id.is_none(),
        "no fabricated external receipt (no invented dedup id)"
    );
    // 幂等键信息保留: the recovery's verification surface still has the key.
    assert_eq!(
        journal[0].idempotency_key.as_deref(),
        Some(format!("{run_id}-tc0001").as_str())
    );

    // The recovery classification: Unknown → conservative needs-attention,
    // NOT ConfirmedFailed/ConfirmedSettled.
    let report =
        classify_run_invocations(state.storage().as_ref(), &run_id, &ConservativeCapabilities)
            .await
            .expect("classify");
    assert_eq!(report.entries[0].class, RecoveryClass::Unknown);
    assert!(
        matches!(
            report.entries[0].decision,
            RecoveryDecision::NeedsAttention { .. }
        ),
        "decision: {:?}",
        report.entries[0].decision
    );

    // The run's own terminal does not mask the unclear call result.
    let (status, reason) = (
        query_text(&state, "SELECT status FROM runs WHERE run_id = ?1", &run_id)
            .await
            .expect("run row"),
        query_text(
            &state,
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            &run_id,
        )
        .await
        .expect("terminal reason"),
    );
    assert_eq!(status, "completed");
    assert_eq!(reason, "completed.with_final");
    // The current-process presentation: the tool event carries the
    // unknown status (contract §5 vocabulary).
    let tool_event = query_text(
        &state,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
         'tool_call_completed'",
        &run_id,
    )
    .await
    .expect("completed tool event");
    assert!(
        tool_event.contains("\"status\":\"unknown\""),
        "the model sees the unknown status: {tool_event}"
    );

    write_evidence(
        "f04_c01_panic_after",
        serde_json::json!({
            "run_id": run_id,
            "external_executed_count": external.executed_count(),
            "journal_phase": journal[0].phase.wire_name(),
            "receipt_outcome": receipt.outcome.wire_name(),
            "receipt_dispatched": receipt.dispatched,
            "receipt_detail": receipt.detail,
            "idempotency_key": journal[0].idempotency_key,
            "recovery_class": format!("{:?}", report.entries[0].class),
            "decision": report.entries[0].decision.name(),
            "run_status": status,
        }),
    );
    state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}

/// Adversarial C01: the panic fires BEFORE the external confirmation —
/// nothing external happened and no result exists. The local receipt
/// still must not fabricate either direction: UNKNOWN, diagnosable, no
/// dedup id, counter 0.
#[tokio::test(flavor = "current_thread")]
async fn f04_c01_adv_panic_before_side_effect_is_still_unknown() {
    let (arrivals_tx, arrivals_rx) = tokio::sync::mpsc::unbounded_channel();
    let _hold_arrivals = arrivals_rx;
    let (fired_tx, mut fired) = tokio::sync::mpsc::unbounded_channel();
    let external_dir = synthetic_home("c01b-external");
    let external = ExternalSystem::new(&external_dir, false);
    let provider = Arc::new(ScriptedProvider {
        turns: vec![
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                content: Vec::new(),
                requests: vec![tool_request("notify.fault")],
            },
            lingxi_kernel::ports::ProviderTurn::Final {
                message: assistant_final("final after the early panic"),
            },
        ],
        arrivals: arrivals_tx,
        next: std::sync::Mutex::new(0),
    });
    let (state, home) = boot(
        "c01-before",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(Arc::new(FaultTool {
                external: Arc::clone(&external),
                faults: std::sync::Mutex::new(HashMap::from([(
                    "notify.fault".to_string(),
                    Fault::PanicBefore,
                )])),
                fired: Some(fired_tx),
            })),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_plain(state.clone(), "sess_local_alpha", "c01 panic before")
        .await
        .expect("run settles");
    assert!(
        fired.try_recv().is_err(),
        "the external system was never reached"
    );
    assert_eq!(
        external.executed_count(),
        0,
        "外部未动作: the counter never moved"
    );

    let journal = load_journal(&state, &run_id).await;
    assert_eq!(journal.len(), 1);
    assert_eq!(
        journal[0].phase,
        InvocationPhase::Unknown,
        "外部未动作且结果未知 still journals unknown (never a fabricated receipt): {:?}",
        journal[0].phase
    );
    let receipt = journal[0].receipt.as_ref().expect("receipt");
    assert_eq!(receipt.outcome, ReceiptOutcome::Unknown);
    assert!(receipt.dispatched);
    assert!(
        receipt.detail.contains("panicked")
            && receipt
                .detail
                .contains("F04 controlled fault: runner died BEFORE"),
        "diagnosable: {}",
        receipt.detail
    );
    assert!(receipt.dedup_id.is_none());

    let report =
        classify_run_invocations(state.storage().as_ref(), &run_id, &ConservativeCapabilities)
            .await
            .expect("classify");
    assert_eq!(report.entries[0].class, RecoveryClass::Unknown);

    write_evidence(
        "f04_c01_adv_panic_before",
        serde_json::json!({
            "run_id": run_id,
            "external_executed_count": external.executed_count(),
            "journal_phase": journal[0].phase.wire_name(),
            "receipt_outcome": receipt.outcome.wire_name(),
            "receipt_detail": receipt.detail,
        }),
    );
    state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}

// ── F04-C02: 派发后无结果各类异常一致 ────────────────────────────────────────

/// The two OBSERVABLE worlds of a dispatched call whose response is lost
/// — the external action happened, vs it never acted — classify
/// IDENTICALLY as unknown in one run, with the external counter proving
/// which one actually executed. No anomaly kind leaks into
/// failed/succeeded.
#[tokio::test(flavor = "current_thread")]
async fn f04_c02_dispatched_no_result_variants_classify_consistently_unknown() {
    let (arrivals_tx, arrivals_rx) = tokio::sync::mpsc::unbounded_channel();
    let _hold_arrivals = arrivals_rx;
    let external_dir = synthetic_home("c02-external");
    let external = ExternalSystem::new(&external_dir, false);
    let provider = Arc::new(ScriptedProvider {
        turns: vec![
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                content: Vec::new(),
                requests: vec![tool_request("lost.after"), tool_request("lost.before")],
            },
            lingxi_kernel::ports::ProviderTurn::Final {
                message: assistant_final("final after both lost responses"),
            },
        ],
        arrivals: arrivals_tx,
        next: std::sync::Mutex::new(0),
    });
    let (state, home) = boot(
        "c02",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(Arc::new(FaultTool {
                external: Arc::clone(&external),
                faults: std::sync::Mutex::new(HashMap::from([
                    ("lost.after".to_string(), Fault::PanicAfter),
                    ("lost.before".to_string(), Fault::PanicBefore),
                ])),
                fired: None,
            })),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_plain(state.clone(), "sess_local_alpha", "c02 both worlds")
        .await
        .expect("run settles");

    let journal = load_journal(&state, &run_id).await;
    assert_eq!(journal.len(), 2);
    let classification_table: Vec<serde_json::Value> = journal
        .iter()
        .map(|entry| {
            let receipt = entry.receipt.as_ref().expect("receipt");
            serde_json::json!({
                "journal_id": entry.journal_id,
                "target": entry.target,
                "phase": entry.phase.wire_name(),
                "outcome": receipt.outcome.wire_name(),
                "dispatched": receipt.dispatched,
                "detail": receipt.detail,
            })
        })
        .collect();
    for (index, entry) in journal.iter().enumerate() {
        assert_eq!(
            entry.phase,
            InvocationPhase::Unknown,
            "entry {index} ({}) must classify unknown: {:?}",
            entry.target,
            entry.phase
        );
        let receipt = entry.receipt.as_ref().expect("receipt");
        assert_eq!(receipt.outcome, ReceiptOutcome::Unknown);
        assert!(receipt.dispatched);
        assert!(receipt.dedup_id.is_none());
    }
    // Exactly ONE external execution — the after-variant performed, the
    // before-variant never reached the external system.
    assert_eq!(external.executed_count(), 1);
    assert_eq!(external.request_count(), 1);

    let report =
        classify_run_invocations(state.storage().as_ref(), &run_id, &ConservativeCapabilities)
            .await
            .expect("classify");
    assert!(report
        .entries
        .iter()
        .all(|e| e.class == RecoveryClass::Unknown));

    write_evidence(
        "f04_c02_classification_table",
        serde_json::json!({
            "run_id": run_id,
            "external_executed_count": external.executed_count(),
            "entries": classification_table,
        }),
    );
    state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}

// ── F04-C03: 可信负面事实不丢失 ──────────────────────────────────────────────

/// Three calls in one run keep THREE distinguishable fact classes:
/// 1. the REAL approval boundary rejects `reject.me` BEFORE dispatch →
///    trusted `failed` receipt with `dispatched=false` and ZERO external
///    requests (the refusal is produced by the wired boundary, not by
///    the tool double);
/// 2. `fail.external` EXECUTES externally and answers a structured
///    failure → trusted `failed` receipt with `dispatched=true`
///    (ConfirmedFailed — the fix must NOT sweep real failures into
///    unknown);
/// 3. `panic.fault` dispatches and panics → `unknown` (differs from
///    both trusted negatives).
#[tokio::test(flavor = "current_thread")]
async fn f04_c03_trusted_negatives_survive_alongside_the_unknown() {
    struct RejectOneTarget {
        target: String,
    }
    impl approval::ApprovalGate for RejectOneTarget {
        fn request<'a>(
            &'a self,
            _ctx: &'a RunContext,
            req: &'a approval::ApprovalRequest,
        ) -> Pin<Box<dyn std::future::Future<Output = approval::ApprovalDecision> + Send + 'a>>
        {
            let reject = req.target == self.target;
            Box::pin(async move {
                if reject {
                    approval::ApprovalDecision::Rejected {
                        reason: "policy refuses this target before any dispatch".to_string(),
                    }
                } else {
                    approval::ApprovalDecision::Approved
                }
            })
        }
    }

    let (arrivals_tx, arrivals_rx) = tokio::sync::mpsc::unbounded_channel();
    let _hold_arrivals = arrivals_rx;
    let external_dir = synthetic_home("c03-external");
    let external = ExternalSystem::new(&external_dir, false);
    let provider = Arc::new(ScriptedProvider {
        turns: vec![
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                content: Vec::new(),
                requests: vec![
                    tool_request("reject.me"),
                    tool_request("fail.external"),
                    tool_request("panic.fault"),
                ],
            },
            lingxi_kernel::ports::ProviderTurn::Final {
                message: assistant_final("final after the three classes"),
            },
        ],
        arrivals: arrivals_tx,
        next: std::sync::Mutex::new(0),
    });
    let (state, home) = boot(
        "c03",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(Arc::new(FaultTool {
                external: Arc::clone(&external),
                faults: std::sync::Mutex::new(HashMap::from([
                    ("reject.me".to_string(), Fault::Succeed),
                    ("fail.external".to_string(), Fault::ExternalFail),
                    ("panic.fault".to_string(), Fault::PanicAfter),
                ])),
                fired: None,
            })),
            approval_gate: Some(Arc::new(RejectOneTarget {
                target: "reject.me".to_string(),
            })),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_plain(state.clone(), "sess_local_alpha", "c03 three classes")
        .await
        .expect("run settles");

    let journal = load_journal(&state, &run_id).await;
    assert_eq!(journal.len(), 3, "one entry per call, oldest first");
    // 1) The pre-authorization rejection: never dispatched.
    let rejected = &journal[0];
    assert_eq!(rejected.target, "reject.me");
    assert_eq!(rejected.phase, InvocationPhase::Failed);
    let receipt = rejected.receipt.as_ref().expect("receipt");
    assert_eq!(receipt.outcome, ReceiptOutcome::Failed);
    assert!(
        !receipt.dispatched,
        "授权前拒绝 dispatched=false (a known never-dispatched fact)"
    );
    assert!(receipt.detail.contains("not dispatched"));
    // 2) The real external failure: a trusted dispatched failure.
    let failed = &journal[1];
    assert_eq!(failed.target, "fail.external");
    assert_eq!(failed.phase, InvocationPhase::Failed);
    let receipt = failed.receipt.as_ref().expect("receipt");
    assert_eq!(receipt.outcome, ReceiptOutcome::Failed);
    assert!(
        receipt.dispatched,
        "a real external failure receipt keeps dispatched=true"
    );
    assert!(receipt.detail.contains("upstream_unavailable"));
    // 3) The dispatched-no-receipt panic: unknown.
    let panned = &journal[2];
    assert_eq!(panned.target, "panic.fault");
    assert_eq!(panned.phase, InvocationPhase::Unknown);
    let receipt = panned.receipt.as_ref().expect("receipt");
    assert_eq!(receipt.outcome, ReceiptOutcome::Unknown);
    assert!(receipt.dispatched);

    // The zero-dispatch proof from the external system itself: only the
    // fail.external and panic.fault requests arrived (the rejection was
    // answered by the wired authorization boundary, never by the
    // executor double "reporting" a refusal).
    assert_eq!(
        external.request_count(),
        2,
        "reject.me never reached the external system"
    );
    assert_eq!(external.executed_count(), 2);

    // The classification keeps the three classes apart.
    let report =
        classify_run_invocations(state.storage().as_ref(), &run_id, &ConservativeCapabilities)
            .await
            .expect("classify");
    let decisions: Vec<&RecoveryDecision> = report.entries.iter().map(|e| &e.decision).collect();
    assert!(matches!(
        decisions[0],
        RecoveryDecision::ConfirmedSettled {
            outcome: ReceiptOutcome::Failed,
            ..
        }
    ));
    assert!(matches!(
        decisions[1],
        RecoveryDecision::ConfirmedSettled {
            outcome: ReceiptOutcome::Failed,
            ..
        }
    ));
    assert!(matches!(
        decisions[2],
        RecoveryDecision::NeedsAttention { .. }
    ));

    write_evidence(
        "f04_c03_three_classes",
        serde_json::json!({
            "run_id": run_id,
            "external_request_count": external.request_count(),
            "external_executed_count": external.executed_count(),
            "entries": journal
                .iter()
                .map(|e| serde_json::json!({
                    "journal_id": e.journal_id,
                    "target": e.target,
                    "phase": e.phase.wire_name(),
                    "outcome": e.receipt.as_ref().map(|r| r.outcome.wire_name()),
                    "dispatched": e.receipt.as_ref().map(|r| r.dispatched),
                }))
                .collect::<Vec<_>>(),
        }),
    );
    state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}

// ── F04-C04: 恢复及重复恢复不重做 Unknown ────────────────────────────────────

/// A call that incremented the external counter (1) but is locally
/// UNKNOWN is left behind on a dangling-active run. The startup scan —
/// run TWICE — settles it with the honest interrupted/needs-attention
/// terminal, keeps the unknown visible, persists no second verdict and
/// NEVER re-executes the side effect (the counter stays 1).
#[tokio::test(flavor = "current_thread")]
async fn f04_c04_recovery_and_repeat_recovery_do_not_redo_unknown() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (tool_fired_tx, mut tool_fired) = tokio::sync::mpsc::unbounded_channel();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let external_dir = synthetic_home("c04-external");
    let external = ExternalSystem::new(&external_dir, false);
    let provider = Arc::new(ParkAfterToolsProvider {
        requests: vec![tool_request("notify.fault")],
        gate: Arc::clone(&gate),
        arrivals: arrivals_tx,
        next: std::sync::Mutex::new(0),
    });
    let (state, home) = boot(
        "c04",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(Arc::new(FaultTool {
                external: Arc::clone(&external),
                faults: std::sync::Mutex::new(HashMap::from([(
                    "notify.fault".to_string(),
                    Fault::PanicAfter,
                )])),
                fired: Some(tool_fired_tx),
            })),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Drive until the external side effect fired and the receipt is
    // committed locally (the run then parks at its second model turn).
    let task = tokio::spawn(execute_plain(
        state.clone(),
        "sess_local_alpha",
        "c04 dangling unknown",
    ));
    assert_eq!(arrivals.recv().await.expect("turn1"), 1);
    let executed_key = tool_fired.recv().await.expect("external executed");
    assert_eq!(external.executed_count(), 1);
    let run_id = loop {
        let id = query_text(
            &state,
            "SELECT run_id FROM runs WHERE session_id = ?1",
            "sess_local_alpha",
        )
        .await;
        if let Some(id) = id {
            break id;
        }
        tokio::task::yield_now().await;
    };
    assert_eq!(executed_key, format!("{run_id}-tc0001"));
    // Wait for the durable receipt before the kill (the fact under test).
    let receipt_deadline = std::time::Instant::now() + Duration::from_millis(2_000);
    loop {
        let outcome = state
            .storage()
            .query_one_text(
                "SELECT receipt_outcome FROM invocation_journal WHERE journal_id = ?1",
                vec![format!("{run_id}-tc0001")],
            )
            .await
            .expect("poll receipt");
        if outcome.is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < receipt_deadline,
            "the receipt under test committed before the kill"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // THE KILL: the drive dies mid-run; the durable row stays active.
    task.abort();
    assert!(
        wait_until(500, || {
            state.runs().cancel_registry().get(&run_id).is_none()
        })
        .await,
        "the abandoned run deregistered"
    );
    state.storage().close().await.expect("close storage");
    drop(state);

    // RECOVERY SCAN #1 (production-shaped bootstrap, doubles absent).
    let layout2 = lingxi_service::prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout2)
        .await
        .expect("bootstrap 2");
    let scan = state2.recovery_report().expect("scan at bootstrap");
    assert_eq!(scan.scanned, 1);
    let outcome = &scan.outcomes[0];
    assert_eq!(outcome.run_id, run_id);
    assert_eq!(
        outcome.category.name(),
        "interrupted_needs_attention",
        "the unknown side effect governs the run: {}",
        outcome.category.name()
    );
    assert!(
        outcome.user_reason.contains("UNKNOWN"),
        "the unknown stays visible in the user-facing reason: {}",
        outcome.user_reason
    );
    // The in-process receipt already recorded unknown — the scan persists
    // no SECOND verdict (idempotent) and nothing re-executes.
    assert_eq!(outcome.unknown_verdicts_persisted, 0);
    assert_eq!(
        external.executed_count(),
        1,
        "no new external execution at recovery"
    );
    let status = query_text(
        &state2,
        "SELECT status FROM runs WHERE run_id = ?1",
        &run_id,
    )
    .await
    .expect("run row");
    assert_eq!(status, "interrupted_needs_attention");
    let journal = load_journal(&state2, &run_id).await;
    assert_eq!(journal[0].phase, InvocationPhase::Unknown);
    assert_eq!(
        journal[0].receipt.as_ref().expect("receipt").outcome,
        ReceiptOutcome::Unknown
    );

    // REPEATED RECOVERY #2: a direct journal pass replays idempotently.
    let pass = recover_run_invocations(
        state2.storage().as_ref(),
        &run_id,
        &ConservativeCapabilities,
        1_790_409_700_000,
    )
    .await
    .expect("recovery pass 2");
    assert_eq!(pass.entries.len(), 1);
    assert!(!pass.entries[0].unknown_verdict_persisted);
    assert_eq!(pass.entries[0].class, RecoveryClass::Unknown);
    assert!(matches!(
        pass.entries[0].decision,
        RecoveryDecision::NeedsAttention { .. }
    ));

    // REPEATED RECOVERY #3: a THIRD bootstrap re-scans nothing (the run
    // is terminal now — 终态不复活) and re-executes nothing.
    state2.storage().close().await.expect("close 2");
    drop(state2);
    let layout3 = lingxi_service::prepare_layout(&home).expect("layout 3");
    let state3 = ServiceState::bootstrap(config_for(&home), &layout3)
        .await
        .expect("bootstrap 3");
    let scan3 = state3.recovery_report().expect("scan 3");
    assert_eq!(
        scan3.scanned, 0,
        "the settled run is never re-scanned, never re-settled"
    );
    assert_eq!(
        external.executed_count(),
        1,
        "the external total never grew across three recoveries"
    );
    let journal3 = load_journal(&state3, &run_id).await;
    assert_eq!(journal3[0].phase, InvocationPhase::Unknown);

    write_evidence(
        "f04_c04_repeat_recovery",
        serde_json::json!({
            "run_id": run_id,
            "external_executed_count_final": external.executed_count(),
            "scan1_category": outcome.category.name(),
            "scan1_unknown_verdicts_persisted": outcome.unknown_verdicts_persisted,
            "scan3_scanned": scan3.scanned,
            "journal_phase_final": journal3[0].phase.wire_name(),
            "pass2_persisted": false,
        }),
    );
    state3.storage().close().await.expect("close 3");
    gate.add_permits(4);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}

/// Adversarial C04 control case: the tool has a VERIFIED idempotency
/// capability. The unknown entered through the F04 panic path (side
/// effect +1, receipt lost); recovery prescribes RESUME WITH THE
/// ORIGINAL KEY; the safe verification re-invokes through the real
/// executor with exactly that key — the external system dedups (one
/// execution in total), the verified outcome settles the entry, and a
/// SECOND verification with the same key still does not double-execute.
#[tokio::test(flavor = "current_thread")]
async fn f04_c04_adv_control_verified_idempotent_recovery_uses_only_the_original_key() {
    let (arrivals_tx, arrivals_rx) = tokio::sync::mpsc::unbounded_channel();
    let _hold_arrivals = arrivals_rx;
    let (fired_tx, mut fired) = tokio::sync::mpsc::unbounded_channel();
    let external_dir = synthetic_home("c04idem-external");
    let external = ExternalSystem::new(&external_dir, true);
    let provider = Arc::new(ScriptedProvider {
        turns: vec![
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                content: Vec::new(),
                requests: vec![tool_request("ledger.idem")],
            },
            lingxi_kernel::ports::ProviderTurn::Final {
                message: assistant_final("final after the lost receipt"),
            },
        ],
        arrivals: arrivals_tx,
        next: std::sync::Mutex::new(0),
    });
    let tools = Arc::new(FaultTool {
        external: Arc::clone(&external),
        faults: std::sync::Mutex::new(HashMap::from([(
            "ledger.idem".to_string(),
            Fault::PanicAfter,
        )])),
        fired: Some(fired_tx),
    });
    let (state, home) = boot(
        "c04-idem",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_plain(state.clone(), "sess_local_alpha", "c04 idem control")
        .await
        .expect("run settles");
    let original_key = fired.recv().await.expect("external executed");
    assert_eq!(original_key, format!("{run_id}-tc0001"));
    assert_eq!(external.executed_count(), 1);

    // The journal holds the unknown with the ORIGINAL key bound.
    let journal = load_journal(&state, &run_id).await;
    assert_eq!(journal[0].phase, InvocationPhase::Unknown);
    assert_eq!(
        journal[0].idempotency_key.as_deref(),
        Some(original_key.as_str())
    );

    // The recovery decision under the VERIFIED capability: resume with
    // the SAME key (安全核验只用原 key).
    let report =
        classify_run_invocations(state.storage().as_ref(), &run_id, &IdemLedgerCapabilities)
            .await
            .expect("classify");
    let resume_key = match &report.entries[0].decision {
        RecoveryDecision::UnknownResumeWithIdempotencyKey { key, .. } => key.clone(),
        other => panic!("expected resume-with-key, got {other:?}"),
    };
    assert_eq!(
        resume_key, original_key,
        "the verification must use the ORIGINAL journaled key"
    );

    // The safe verification: re-invoke through the REAL executor port
    // with the original call identity; the external system dedups. The
    // controlled interruption is over (the original panicking child was
    // dropped long ago), so the resumed execution must ANSWER.
    tools.disarm("ledger.idem");
    let entry = &load_journal(&state, &run_id).await[0];
    let recovered_ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: lingxi_protocol::RunId::new(run_id.clone()),
        attempt: lingxi_protocol::AttemptId::new(entry.attempt.clone()),
        generation: entry.generation,
    };
    let call = ToolCallId::new(entry.journal_id.clone());
    // R04-T01: the verification request carries the COMPLETE effective
    // arguments; the digest is derived from them by the constructor.
    let request = ToolRequest::from_effective_arguments(
        entry.target.clone(),
        serde_json::json!({"target": entry.target.clone(), "payload": "fixed"}),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request")
    .with_summary("recovery verification: original idempotency key");
    let resumed = tools.execute(&recovered_ctx, &call, &request).await;
    assert!(resumed.fence.matches_ctx(&recovered_ctx));
    // 核验 (content-level, R04-T01): the deduped verification returns the
    // recorded outcome's CONTENT, not just a digest.
    let outcome_text = match &resumed.outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .map(|block| match block {
                lingxi_protocol::ContentBlock::Text { text } => text.as_str(),
                other => panic!("expected text content, got {other:?}"),
            })
            .collect::<String>(),
        other => panic!("the deduped verification returns the recorded outcome, got {other:?}"),
    };
    assert_eq!(external.request_count(), 2, "the verification re-issued");
    assert_eq!(
        external.executed_count(),
        1,
        "the external operation was NOT performed twice"
    );
    let recorded = external
        .read_state()
        .get(&original_key)
        .and_then(|v| v.as_str())
        .expect("the recorded outcome")
        .to_string();
    assert_eq!(outcome_text, recorded, "verified against the record");

    // The verified receipt settles the formerly-unknown entry.
    state
        .storage()
        .record_invocation_receipt(
            &recovered_ctx,
            &call,
            lingxi_kernel::ports::InvocationReceipt {
                outcome: ReceiptOutcome::Succeeded,
                detail: format!(
                    "verified with the original idempotency key {original_key}: deduped"
                ),
                dedup_id: Some(original_key.clone()),
                dispatched: true,
            },
            1_790_409_800_000,
        )
        .await
        .expect("verified receipt settles");
    let settled = load_journal(&state, &run_id).await;
    assert_eq!(settled[0].phase, InvocationPhase::Succeeded);

    // A SECOND verification with the same key still cannot double-execute.
    let again = tools.execute(&recovered_ctx, &call, &request).await;
    assert!(matches!(again.outcome, ToolOutcome::Success { .. }));
    assert_eq!(external.request_count(), 3);
    assert_eq!(
        external.executed_count(),
        1,
        "repeat verification with the original key stays single-execution"
    );

    write_evidence(
        "f04_c04_adv_idem_control",
        serde_json::json!({
            "run_id": run_id,
            "original_key": original_key,
            "resume_key": resume_key,
            "external_request_count": external.request_count(),
            "external_executed_count": external.executed_count(),
            "verified_outcome_text": outcome_text,
            "final_phase": settled[0].phase.wire_name(),
        }),
    );
    state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}
