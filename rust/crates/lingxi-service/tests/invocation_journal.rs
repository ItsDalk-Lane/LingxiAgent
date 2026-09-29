//! R03-T05 integration: the side-effect INVOCATION JOURNAL and the
//! crash-recovery classification through the REAL service composition
//! (real storage port incl. migration V3, real event service, real kernel
//! state machine and single finalize, real run driver with the journal
//! write order intent→…→started BEFORE the external execution and the
//! receipt AFTER it), driven by deterministic file-backed external-system
//! doubles.
//!
//! Acceptance scenarios (taskbook R03 §6):
//!
//! - R03-A09 副作用后崩溃不重复执行: the external counter has been
//!   incremented but the result was never committed locally (the driver is
//!   killed between the external execution and the receipt write). After
//!   the restart the tool invocation count does NOT automatically grow,
//!   and the receipt reads UNKNOWN.
//! - R03-A10 可验证幂等恢复: the tool supports a fixed idempotency key;
//!   after an interruption before the response, recovery resumes WITH THE
//!   SAME KEY — the external operation is not performed twice and the
//!   invocation ends once the result is verified.
//! - The drive-chain lifecycle: prepared → authorized → started close with
//!   receipts on the live path (including an approval-rejected call that
//!   closes as a never-dispatched failure).
//!
//! Crash boundary (dispatch-sanctioned form): the "kill" aborts the real
//! driving future at its await point and re-opens the database with a
//! FRESH ServiceState over the same data root — the persistence boundary
//! is real (the journal intent/started writes committed BEFORE the
//! external execution; the receipt never committed). The external system
//! is a controlled local double backed by files under an isolated temp
//! directory; nothing is sent anywhere.
//!
//! Test-double boundary: the provider/tool doubles only produce external
//! responses and park; every journal write, phase decision, fence check
//! and recovery classification belongs to the real driver + kernel +
//! storage port.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::invocation::{RecoveryDecision, ToolRecoveryCapability};
use lingxi_kernel::ports::{
    InvocationPhase, InvocationReceipt, ProviderDescriptor, ProviderTurnResult, ReceiptOutcome,
    StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::invocations::{
    recover_run_invocations, RecoveryCapabilitySource, RunRecoveryReport,
};
use lingxi_service::{
    approval, invocations::ConservativeCapabilities, ExecuteSubmission, Principal, PrincipalKind,
};
use lingxi_service::{HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState};

// ── the controlled local external systems (file-backed doubles) ─────────────

/// A file-backed external system under an isolated temp directory. The
/// request log is the durable evidence: one JSON line per request the
/// double received, carrying the presented idempotency key, whether the
/// operation was EXECUTED and the resulting digest. `state.json` maps
/// idempotency key → digest (the idempotent variant's dedup memory).
struct ExternalDouble {
    dir: PathBuf,
    /// Whether the presented idempotency key is honored (dedup); a
    /// non-idempotent external system (message send / payment analog)
    /// executes every request it receives.
    honors_key: bool,
    requests: std::sync::Mutex<Vec<serde_json::Value>>,
}

struct RequestRecord {
    key: String,
    executed: bool,
    digest: String,
}

impl ExternalDouble {
    fn new(dir: &Path, honors_key: bool) -> Arc<Self> {
        std::fs::create_dir_all(dir).expect("mkdir external dir");
        Arc::new(Self {
            dir: dir.to_path_buf(),
            honors_key,
            requests: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join("requests.log")
    }

    /// Performs (or dedups) one external operation. Durable BEFORE it
    /// returns: the request line and (for executions) the key→digest state
    /// are written to disk first, mirroring a real external system whose
    /// effect survives a client crash.
    fn perform(&self, key: &str) -> RequestRecord {
        if self.honors_key {
            let state = self.read_state();
            if let Some(digest) = state.get(key).and_then(|v| v.as_str()) {
                let record = RequestRecord {
                    key: key.to_string(),
                    executed: false,
                    digest: digest.to_string(),
                };
                self.append_request(&record);
                return record;
            }
        }
        // A fresh execution: counter increments by exactly one, the
        // outcome is durably recorded under the key, then the request line
        // lands.
        let executed_so_far = self.executed_count();
        let digest = format!("digest-{}-{}", executed_so_far + 1, key);
        if self.honors_key {
            let mut state = self.read_state();
            state.insert(key.to_string(), serde_json::Value::String(digest.clone()));
            let path = self.dir.join("state.json");
            std::fs::write(&path, serde_json::to_vec(&state).expect("state json"))
                .expect("durable external state");
        }
        let record = RequestRecord {
            key: key.to_string(),
            executed: true,
            digest,
        };
        self.append_request(&record);
        record
    }

    fn append_request(&self, record: &RequestRecord) {
        let line = serde_json::json!({
            "key": record.key,
            "executed": record.executed,
            "digest": record.digest,
        });
        self.requests.lock().unwrap().push(line.clone());
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())
            .expect("open request log");
        writeln!(file, "{}", line).expect("append request log");
    }

    fn read_state(&self) -> serde_json::Map<String, serde_json::Value> {
        let path = self.dir.join("state.json");
        std::fs::read(&path)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default()
    }

    /// How many operations were actually EXECUTED (the external counter).
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

/// Tool double backed by [`ExternalDouble`]. After performing the external
/// operation it SIGNALS the test and parks forever: the external effect is
/// durable while the response never reaches the driver — exactly the
/// crash window between the external execution and the local receipt.
/// `fired: None` (the plain lifecycle suite) skips the signal.
struct ParkingExternalTool {
    external: Arc<ExternalDouble>,
    fired: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    gate: Arc<tokio::sync::Semaphore>,
    /// When false (the plain lifecycle suite) the double returns normally
    /// instead of parking.
    park: bool,
    fail: bool,
}

impl ToolExecutorPort for ParkingExternalTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let ctx_at_issue = ctx.clone();
        // The presented idempotency key: the durable call identity.
        let key = call.to_string();
        let record = self.external.perform(&key);
        if let Some(fired) = &self.fired {
            fired.send(key).expect("test holds the fired receiver");
        }
        let park = self.park;
        let fail = self.fail;
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            if park {
                // "在响应前中断": the external operation already happened;
                // the response never arrives (the process dies first).
                let _permit = gate.acquire().await.expect("gate closed at teardown");
            }
            let outcome = if fail {
                ToolOutcome::Failed {
                    error: ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        "external refused",
                        false,
                    ),
                }
            } else {
                ToolOutcome::Success {
                    content_digest: record.digest,
                }
            };
            ToolExecutionResult::of_ctx(&ctx_at_issue, outcome)
        })
    }
}

// ── scripted provider (the late_result_fence shape) ──────────────────────────

struct ScriptedStep {
    turn: lingxi_kernel::ports::ProviderTurn,
}

struct ScriptedProvider {
    scripts: std::sync::Mutex<std::collections::HashMap<String, VecDeque<ScriptedStep>>>,
    arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
    next_pop: std::sync::Mutex<std::collections::HashMap<String, usize>>,
}

impl ScriptedProvider {
    fn new(
        scripts: Vec<(&'static str, Vec<lingxi_kernel::ports::ProviderTurn>)>,
        arrivals: tokio::sync::mpsc::UnboundedSender<(String, usize)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scripts: std::sync::Mutex::new(
                scripts
                    .into_iter()
                    .map(|(session, steps)| {
                        (
                            session.to_string(),
                            steps
                                .into_iter()
                                .map(|turn| ScriptedStep { turn })
                                .collect(),
                        )
                    })
                    .collect(),
            ),
            arrivals,
            next_pop: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.journal".to_string(),
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
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let session = ctx.session_id.to_string();
        let pop = {
            let mut counters = self.next_pop.lock().unwrap();
            let next = counters.get(&session).copied().unwrap_or(0) + 1;
            counters.insert(session.clone(), next);
            next
        };
        self.arrivals
            .send((session.clone(), pop))
            .expect("test holds the arrivals receiver");
        let ctx_at_issue = ctx.clone();
        let step = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&session)
            .and_then(|queue| queue.pop_front())
            .unwrap_or_else(|| ScriptedStep {
                turn: lingxi_kernel::ports::ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        "script exhausted",
                        false,
                    ),
                    retryable: false,
                },
            });
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, step.turn) })
    }
}

fn tool_request(target: &str) -> ToolRequest {
    ToolRequest {
        target: target.to_string(),
        args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({
            "target": target, "payload": "fixed"
        })),
        args_summary: Some(format!("{target} fixed payload")),
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

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t05-{tag}-{}-{}",
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

async fn run_status(state: &ServiceState, run_id: &str) -> String {
    query_text(state, "SELECT status FROM runs WHERE run_id = ?1", run_id)
        .await
        .expect("run row exists")
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
    if let Ok(path) = std::env::var("R03_T05_EVIDENCE") {
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

/// The capability source the A10 recovery uses: the ledger double VERIFIABLY
/// honors the idempotency key (its dedup behavior is asserted by the test
/// itself); every other target stays conservative.
struct LedgerCapabilities;

impl RecoveryCapabilitySource for LedgerCapabilities {
    fn capability_of(&self, target: &str) -> ToolRecoveryCapability {
        if target == "ledger.double" {
            ToolRecoveryCapability {
                honors_idempotency_key: true,
                ..ToolRecoveryCapability::CONSERVATIVE
            }
        } else {
            ToolRecoveryCapability::CONSERVATIVE
        }
    }
}

// ── R03-A09: 副作用后崩溃不重复执行 ─────────────────────────────────────────

/// The non-idempotent external counter has EXECUTED the operation (its
/// request log and count prove it) but the driver dies before the receipt
/// commit. After the restart:
/// - nothing re-executes (the external counter never grows — restart
///   alone does not re-drive tools, and the recovery decision for a
///   non-idempotent unknown is needs-attention, never a retry);
/// - the receipt reads UNKNOWN (the honest durable verdict);
/// - the run row stays honest (active; the full startup-scan coordinator
///   is R03-T07).
#[tokio::test(flavor = "current_thread")]
async fn r03_a09_crash_after_side_effect_does_not_reexecute_and_receipt_is_unknown() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (fired_tx, mut fired) = tokio::sync::mpsc::unbounded_channel();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let external_dir = synthetic_home("a09-external");
    let external = ExternalDouble::new(&external_dir, false);
    let provider = ScriptedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                lingxi_kernel::ports::ProviderTurn::ToolRequests {
                    requests: vec![tool_request("notify.double")],
                },
                lingxi_kernel::ports::ProviderTurn::Final {
                    message: assistant_final("never reached before the crash"),
                },
            ],
        )],
        arrivals_tx,
    );
    let tools: Arc<dyn ToolExecutorPort> = Arc::new(ParkingExternalTool {
        external: Arc::clone(&external),
        fired: Some(fired_tx),
        gate: Arc::clone(&gate),
        park: true,
        fail: false,
    });
    let (state, home) = boot(
        "a09",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(tools),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Drive until the external operation has executed and the tool parks.
    let task = tokio::spawn(execute_plain(
        state.clone(),
        "sess_local_alpha",
        "a09 crash window",
    ));
    assert_eq!(
        arrivals.recv().await.expect("turn1"),
        ("sess_local_alpha".to_string(), 1)
    );
    let fired_key = fired.recv().await.expect("external executed");
    assert_eq!(
        external.executed_count(),
        1,
        "the external counter increased ONCE"
    );
    assert_eq!(external.request_count(), 1);
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
    assert_eq!(fired_key, format!("{run_id}-tc0001"));

    // The crash window, witnessed BEFORE the kill: the journal holds the
    // durable intent at `started` (committed BEFORE the external dispatch)
    // and NO receipt.
    let journal_before = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal load");
    assert_eq!(journal_before.len(), 1);
    assert_eq!(journal_before[0].phase, InvocationPhase::Started);
    assert!(journal_before[0].receipt.is_none(), "no receipt committed");
    assert_eq!(
        journal_before[0].idempotency_key.as_deref(),
        Some(fired_key.as_str())
    );
    assert_eq!(journal_before[0].target, "notify.double");

    // THE KILL: the driving future is aborted at its await point (the
    // tool response never arrives); the registration guard fires the tree;
    // the durable run row stays honest.
    task.abort();
    assert!(
        wait_until(500, || {
            state.runs().cancel_registry().get(&run_id).is_none()
        })
        .await,
        "the abandoned run deregistered (guard fired)"
    );
    state.storage().close().await.expect("close storage");
    drop(state);

    // THE RESTART: a fresh ServiceState over the same data root. Nothing
    // auto-drives (no startup-scan coordinator exists until R03-T07), and
    // nothing in the T05 recovery pass re-executes anything — assert the
    // counter is unchanged across the restart BEFORE and AFTER the pass.
    let layout2 = lingxi_service::prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout2)
        .await
        .expect("bootstrap 2 (production-shaped: no doubles configured)");
    assert_eq!(external.executed_count(), 1, "restart executed nothing");
    let status = run_status(&state2, &run_id).await;
    assert_eq!(
        status, "running",
        "the run row stays honest (dangling active)"
    );

    // The T05 recovery pass: classify + persist the unknown verdict.
    let report: RunRecoveryReport = recover_run_invocations(
        state2.storage().as_ref(),
        &run_id,
        &ConservativeCapabilities,
        1_790_409_700_000,
    )
    .await
    .expect("recovery pass");
    assert_eq!(report.entries.len(), 1);
    let entry = &report.entries[0];
    assert_eq!(entry.target, "notify.double");
    assert!(
        matches!(entry.decision, RecoveryDecision::NeedsAttention { .. }),
        "a non-idempotent unknown side effect must NEVER auto-retry: {:?}",
        entry.decision
    );
    assert!(entry.unknown_verdict_persisted, "the verdict was persisted");
    // The receipt now reads UNKNOWN — the honest durable classification.
    let journal_after = state2
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal reload");
    assert_eq!(journal_after[0].phase, InvocationPhase::Unknown);
    assert_eq!(
        journal_after[0].receipt.as_ref().expect("verdict").outcome,
        ReceiptOutcome::Unknown
    );
    assert_eq!(
        external.executed_count(),
        1,
        "the tool invocation total did NOT grow after recovery"
    );
    assert_eq!(external.request_count(), 1);

    write_evidence(
        "r03_a09_unknown_receipt",
        serde_json::json!({
            "run_id": run_id,
            "target": "notify.double",
            "external_executed_count": external.executed_count(),
            "external_request_count": external.request_count(),
            "journal_phase_before_kill": "started",
            "receipt_outcome_after_recovery": "unknown",
            "decision": entry.decision.name(),
            "run_status_after_restart": status,
        }),
    );
    println!(
        "R03_A09_TRACE: run={run_id} external_executions={} journal=started→unknown \
         decision={} run_row={}",
        external.executed_count(),
        entry.decision.name(),
        status
    );
    state2.storage().close().await.expect("close 2");
    gate.add_permits(1);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}

// ── R03-A10: 可验证幂等恢复 ──────────────────────────────────────────────────

/// The external system honors the idempotency key (its dedup memory maps
/// key → recorded outcome). The driver dies after the external execution
/// but before the response. After the restart, the recovery decision for
/// the unknown entry is RESUME WITH THE SAME KEY; the re-invocation
/// presents the key, the external system does NOT perform the operation
/// again (returns the recorded outcome), the verified receipt settles the
/// entry and the recovery ends — one external operation in total.
#[tokio::test(flavor = "current_thread")]
async fn r03_a10_idempotent_key_resume_does_not_duplicate_the_external_operation() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (fired_tx, mut fired) = tokio::sync::mpsc::unbounded_channel();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let external_dir = synthetic_home("a10-external");
    let external = ExternalDouble::new(&external_dir, true);
    let provider = ScriptedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                lingxi_kernel::ports::ProviderTurn::ToolRequests {
                    requests: vec![tool_request("ledger.double")],
                },
                lingxi_kernel::ports::ProviderTurn::Final {
                    message: assistant_final("never reached before the interruption"),
                },
            ],
        )],
        arrivals_tx,
    );
    let tools = Arc::new(ParkingExternalTool {
        external: Arc::clone(&external),
        fired: Some(fired_tx),
        gate: Arc::clone(&gate),
        park: true,
        fail: false,
    });
    let (state, home) = boot(
        "a10",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(tools.clone() as Arc<dyn ToolExecutorPort>),
            ..ServiceDeps::default()
        },
    )
    .await;

    // Run 1: the external ledger executes under the key, then the response
    // is interrupted (the double parks; the driver dies).
    let task = tokio::spawn(execute_plain(
        state.clone(),
        "sess_local_alpha",
        "a10 idempotent crash window",
    ));
    assert_eq!(
        arrivals.recv().await.expect("turn1"),
        ("sess_local_alpha".to_string(), 1)
    );
    let key = fired.recv().await.expect("external executed");
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
    let journal_before = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal_before.len(), 1);
    assert_eq!(journal_before[0].phase, InvocationPhase::Started);
    assert!(journal_before[0].receipt.is_none());

    task.abort();
    assert!(
        wait_until(500, || {
            state.runs().cancel_registry().get(&run_id).is_none()
        })
        .await
    );
    state.storage().close().await.expect("close");
    drop(state);

    // Restart and run the recovery pass under the VERIFIED capability
    // (the ledger double honors the key).
    let layout2 = lingxi_service::prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout2)
        .await
        .expect("bootstrap 2");
    let report = recover_run_invocations(
        state2.storage().as_ref(),
        &run_id,
        &LedgerCapabilities,
        1_790_409_700_000,
    )
    .await
    .expect("recovery pass");
    assert_eq!(report.entries.len(), 1);
    let decision = report.entries[0].decision.clone();
    match &decision {
        RecoveryDecision::UnknownResumeWithIdempotencyKey {
            key: resume_key, ..
        } => {
            assert_eq!(resume_key, &key, "the SAME key must be presented");
        }
        other => panic!("expected resume-with-key, got {other:?}"),
    }

    // The resume: re-invoke the tool through the SAME executor port with
    // the recovered context and call identity (the R03-T07 coordinator
    // will drive this from the decision; the port call itself is what it
    // dispatches). The interruption is over, so the resumed call must
    // ANSWER — the gate is released first (the original parked child was
    // already dropped by the cancellation tree). The external ledger
    // dedups — no second execution.
    gate.add_permits(1);
    let entry = &state2
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal")[0];
    let run = lingxi_protocol::RunId::new(run_id.clone());
    let recovered_ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
        run_id: run.clone(),
        attempt: lingxi_protocol::AttemptId::new(entry.attempt.clone()),
        generation: entry.generation,
    };
    let call = ToolCallId::new(entry.journal_id.clone());
    let request = ToolRequest {
        target: entry.target.clone(),
        args_digest: lingxi_protocol::ArgsDigest {
            algorithm: "sha256".to_string(),
            canonicalization: "jcs".to_string(),
            hex: entry.args_digest.clone(),
        },
        args_summary: entry.args_summary.clone(),
    };
    let resumed = tools.execute(&recovered_ctx, &call, &request).await;
    assert!(resumed.fence.matches_ctx(&recovered_ctx));
    let digest = match resumed.outcome {
        ToolOutcome::Success { content_digest } => content_digest,
        other => panic!("the deduped resume must return the recorded outcome, got {other:?}"),
    };
    // The dedup evidence: TWO requests (original + resume), ONE execution.
    assert_eq!(
        external.request_count(),
        2,
        "the resume re-issued the request"
    );
    assert_eq!(
        external.executed_count(),
        1,
        "the external operation was NOT performed twice"
    );
    // 核验: the deduped response equals the originally recorded outcome.
    let state_map = external.read_state();
    let recorded = state_map
        .get(&key)
        .and_then(|v| v.as_str())
        .expect("the key's recorded outcome");
    assert_eq!(
        &digest, recorded,
        "verified: the dedup result matches the record"
    );

    // The verified receipt settles the formerly-unknown entry and the
    // recovery ends (the journal is closed with the dedup identifier).
    state2
        .storage()
        .record_invocation_receipt(
            &recovered_ctx,
            &call,
            InvocationReceipt {
                outcome: ReceiptOutcome::Succeeded,
                detail: format!(
                    "resume with idempotency key {key}: deduped to the recorded outcome"
                ),
                dedup_id: Some(key.clone()),
                dispatched: true,
            },
            1_790_409_800_000,
        )
        .await
        .expect("verified receipt settles the unknown");
    let settled = state2
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("final journal");
    assert_eq!(settled[0].phase, InvocationPhase::Succeeded);
    assert_eq!(
        settled[0]
            .receipt
            .as_ref()
            .expect("receipt")
            .dedup_id
            .as_deref(),
        Some(key.as_str())
    );
    assert_eq!(external.executed_count(), 1, "still exactly one execution");

    write_evidence(
        "r03_a10_idempotent_resume",
        serde_json::json!({
            "run_id": run_id,
            "target": "ledger.double",
            "idempotency_key": key,
            "external_request_count": external.request_count(),
            "external_executed_count": external.executed_count(),
            "decision": decision.name(),
            "verified_digest": digest,
            "final_phase": "succeeded",
        }),
    );
    println!(
        "R03_A10_TRACE: run={run_id} key={key} requests={} executed={} decision={} phase=succeeded",
        external.request_count(),
        external.executed_count(),
        decision.name()
    );
    state2.storage().close().await.expect("close 2");
    gate.add_permits(1);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}

// ── drive-chain lifecycle: the write order on the live path ──────────────────

/// A normal run with two tool calls (one success, one external failure)
/// journals BOTH invocations through the real write order
/// prepared→authorized→started→receipt, and an approval-REJECTED call
/// closes as a never-dispatched failure (dispatched=false). The receipts
/// bind owner/run/attempt/generation, target, args digest and key.
#[tokio::test(flavor = "current_thread")]
async fn journal_lifecycle_progresses_and_closes_receipts_on_the_live_chain() {
    let (arrivals_tx, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let external_dir = synthetic_home("lifecycle-external");
    // Two sequential calls against one non-parking external double: the
    // first succeeds, the second is driven through a FAILING wrapper.
    let external = ExternalDouble::new(&external_dir, false);

    struct FlakyOnSecond {
        inner: Arc<ParkingExternalTool>,
    }
    impl ToolExecutorPort for FlakyOnSecond {
        fn execute<'a>(
            &'a self,
            ctx: &'a RunContext,
            call: &'a ToolCallId,
            request: &'a ToolRequest,
        ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
            let seq: u32 = call
                .to_string()
                .rsplit("tc")
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let inner = self.inner.execute(ctx, call, request);
            let ctx_at_issue = ctx.clone();
            Box::pin(async move {
                let result = inner.await;
                if seq == 2 {
                    return ToolExecutionResult::of_ctx(
                        &ctx_at_issue,
                        ToolOutcome::Failed {
                            error: ProtocolError::new(
                                ErrorCode::UpstreamUnavailable,
                                "external refused on the second call",
                                false,
                            ),
                        },
                    );
                }
                result
            })
        }
    }
    let base = Arc::new(ParkingExternalTool {
        external: Arc::clone(&external),
        fired: None,
        gate,
        park: false,
        fail: false,
    });
    let tools: Arc<dyn ToolExecutorPort> = Arc::new(FlakyOnSecond { inner: base });

    // The approval gate: APPROVES the first call, REJECTS the third.
    struct ApproveFirstRejectThird;
    impl approval::ApprovalGate for ApproveFirstRejectThird {
        fn request<'a>(
            &'a self,
            _ctx: &'a RunContext,
            req: &'a approval::ApprovalRequest,
        ) -> Pin<Box<dyn std::future::Future<Output = approval::ApprovalDecision> + Send + 'a>>
        {
            let seq: u32 = req
                .tool_call_id
                .to_string()
                .rsplit("tc")
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            Box::pin(async move {
                if seq == 3 {
                    approval::ApprovalDecision::Rejected {
                        reason: "policy refuses the third call".to_string(),
                    }
                } else {
                    approval::ApprovalDecision::Approved
                }
            })
        }
    }

    let provider = ScriptedProvider::new(
        vec![(
            "sess_local_alpha",
            vec![
                lingxi_kernel::ports::ProviderTurn::ToolRequests {
                    requests: vec![tool_request("notify.double"), tool_request("notify.double")],
                },
                lingxi_kernel::ports::ProviderTurn::ToolRequests {
                    requests: vec![tool_request("notify.double")],
                },
                lingxi_kernel::ports::ProviderTurn::Final {
                    message: assistant_final("final after three tool calls"),
                },
            ],
        )],
        arrivals_tx,
    );
    let (state, home) = boot(
        "lifecycle",
        ServiceDeps {
            turn_provider: Some(provider),
            tool_executor: Some(tools),
            approval_gate: Some(
                Arc::new(ApproveFirstRejectThird) as Arc<dyn approval::ApprovalGate>
            ),
            ..ServiceDeps::default()
        },
    )
    .await;

    let run_id = execute_plain(state.clone(), "sess_local_alpha", "lifecycle probe")
        .await
        .expect("settles");
    for expected in 1..=3 {
        assert_eq!(
            arrivals.recv().await.expect("turn arrivals"),
            ("sess_local_alpha".to_string(), expected)
        );
    }

    let journal = state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 3, "one entry per tool call, oldest first");
    for (index, entry) in journal.iter().enumerate() {
        let seq = index + 1;
        assert_eq!(entry.journal_id, format!("{run_id}-tc{seq:04}"));
        assert_eq!(entry.run_id, run_id);
        assert_eq!(entry.attempt, format!("{run_id}#a1"));
        assert_eq!(entry.generation, 1);
        assert_eq!(entry.owner_kind, "local_user");
        assert_eq!(entry.owner_subject, "user_local");
        assert_eq!(entry.target, "notify.double");
        assert!(
            entry.args_digest.starts_with("sha256") || !entry.args_digest.is_empty(),
            "the normalized args digest is bound"
        );
        assert_eq!(
            entry.idempotency_key.as_deref(),
            Some(format!("{run_id}-tc{seq:04}").as_str())
        );
    }
    // tc0001: approved → executed → succeeded with the dedup identifier.
    assert_eq!(journal[0].phase, InvocationPhase::Succeeded);
    assert_eq!(
        journal[0].receipt.as_ref().unwrap().outcome,
        ReceiptOutcome::Succeeded
    );
    assert!(journal[0].receipt.as_ref().unwrap().dispatched);
    assert!(journal[0].receipt.as_ref().unwrap().dedup_id.is_some());
    // tc0002: approved → executed → external failure closed as failed.
    assert_eq!(journal[1].phase, InvocationPhase::Failed);
    assert_eq!(
        journal[1].receipt.as_ref().unwrap().outcome,
        ReceiptOutcome::Failed
    );
    assert!(journal[1].receipt.as_ref().unwrap().dispatched);
    // tc0003: REJECTED before any dispatch — closed as a never-dispatched
    // failure (执行 0 次).
    assert_eq!(journal[2].phase, InvocationPhase::Failed);
    let rejected = journal[2].receipt.as_ref().unwrap();
    assert_eq!(rejected.outcome, ReceiptOutcome::Failed);
    assert!(!rejected.dispatched, "a rejected approval never dispatched");
    assert!(rejected.detail.contains("not dispatched"));
    // The external double executed exactly the two approved calls.
    assert_eq!(external.executed_count(), 2);
    // The run settles through the single finalize WITH its real final
    // answer (the failed tool calls are tool facts the run survives; a
    // final message exists, so the outcome is with_final, not the
    // no-final tool-partial-failure vocabulary).
    let (status, reason) = (
        run_status(&state, &run_id).await,
        query_text(
            &state,
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            &run_id,
        )
        .await,
    );
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    write_evidence(
        "r03_t05_lifecycle",
        serde_json::json!({
            "run_id": run_id,
            "entries": journal
                .iter()
                .map(|e| serde_json::json!({
                    "journal_id": e.journal_id,
                    "phase": e.phase.wire_name(),
                    "dispatched": e.receipt.as_ref().map(|r| r.dispatched),
                    "outcome": e.receipt.as_ref().map(|r| r.outcome.wire_name()),
                }))
                .collect::<Vec<_>>(),
            "external_executed_count": external.executed_count(),
        }),
    );
    println!(
        "R03_T05_LIFECYCLE_TRACE: run={run_id} entries=3 phases=succeeded,failed,failed(rejected) \
         external_executions={}",
        external.executed_count()
    );
    state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&external_dir);
}
