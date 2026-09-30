//! STAGE-REVIEWER-R03-RR2 — INDEPENDENT retest (NOT the executor's suite,
//! NOT the R1 reviewer's retest; own ids, own whitespace variants, own
//! control arms).
//!
//! Written by the fresh RR2 STAGE reviewer to re-verify R03-RR2-F05-01 on
//! the final candidate f4d4b1666 through the REAL public surfaces (real
//! composition root, real SQLite store, real supervisor/gate/dedup; the
//! only doubles are the scripted provider and the external side-effect
//! counter FILE). Error variants are matched by RUNTIME Debug-string
//! containment so the SAME file compiles on the fixed candidate AND on the
//! unmodified RR2 baseline c96f7cc6 (where it must go RED).
//!
//! Chain (a): padded first id " req-9x " admitted through the real
//! FOREGROUND surface -> external count +1 -> storage close + drop + fresh
//! boot on the SAME data root -> BOTH the raw " req-9x " and the canonical
//! "req-9x" retries resolve to the SAME earlier run (explicit refusal),
//! no second execution, no second external effect, canonical durable
//! anchor; a BRAND-NEW id is still admitted after the restart (no
//! over-blocking).
//! Chain (b): two PRE-FIX cause_id rows whose raw ids normalize to one
//! logical key ("request: req-9x " and "request:\r\nreq-9x\r\n") -> every
//! spelling of the logical key refuses as an EXPLICIT ambiguity naming
//! EVERY run (never a silent pick, never a fresh execution); the frozen
//! rows stay verbatim.
//! Chain (c): namespace isolation — the same logical id " iso-7 " under
//! another session (same principal) and under another principal kind
//! (device of the same user) is a SEPARATE namespace; after the restart
//! each namespace's refusal names ITS OWN run (raw/canonical/tab-padded
//! retries mixed), never a foreign one.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::{
    KeyEvent, ProviderDescriptor, ProviderTurn, ProviderTurnResult, RunOutcome, StoragePort,
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::subagent::{RunLineage, RunOrigin};
use lingxi_kernel::{Principal as KernelPrincipal, RunContext};
use lingxi_protocol::{
    ContentBlock, ErrorCode, EventId, EventPayload, KnownEventPayload, ModelCallId,
    NormalizedMessage, ProtocolError, RunId, SessionId, ToolCallId,
};

use lingxi_service::{
    cancel::CancelPolicy, prepare_layout, ExecuteSubmission, HomeSource, NetworkMode,
    ServiceConfig, ServiceDeps, ServiceState,
};

const NOW_MS: u64 = 1_790_409_600_000;

// ── doubles: scripted provider + external counter file ──────────────────────

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
            provider: "stub.srv2_stage".to_string(),
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

fn append_external(counter: &Path) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(counter)
        .expect("open counter file");
    writeln!(file, "external+1").expect("append external effect");
}

fn external_count(counter: &Path) -> usize {
    std::fs::read_to_string(counter)
        .map(|raw| raw.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

fn tool_request() -> ToolRequest {
    ToolRequest {
        target: "counter.write".to_string(),
        args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({"op": "bump"})),
        args_summary: Some("counter bump".to_string()),
        delegation: None,
    }
}

fn scripted_steps() -> Vec<ProviderTurn> {
    vec![
        ProviderTurn::ToolRequests {
            requests: vec![tool_request()],
        },
        ProviderTurn::Final {
            message: NormalizedMessage {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text {
                    text: "srv2 stage done".to_string(),
                }],
                model_call_id: None,
            },
        },
    ]
}

// ── harness ──────────────────────────────────────────────────────────────────

fn temp_tag(tag: &str) -> String {
    format!(
        "lingxi-srv2-stage-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
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
        principal_id: "principal_local_srv2".to_string(),
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

/// A DEVICE principal of the SAME local user — a different owner kind,
/// therefore a DIFFERENT durable id namespace.
fn device_principal_of_local_user() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_device_srv2".to_string(),
        kind: lingxi_service::PrincipalKind::Device,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: Some("device_srv2".to_string()),
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Lan,
        credential_kind: lingxi_service::CredentialKind::DeviceCredential,
        trust_state: lingxi_service::TrustState::Lan,
        scopes: vec!["chat".to_string()],
    }
}

async fn boot(home: &Path, provider: Arc<ScriptedProvider>, counter: &Path) -> ServiceState {
    let layout = prepare_layout(home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(Arc::new(CountingFileTool {
            counter: counter.to_path_buf(),
        })),
        cancel_policy: CancelPolicy::default(),
        ..ServiceDeps::default()
    };
    ServiceState::bootstrap_with_deps(config_for(home), &layout, deps)
        .await
        .expect("bootstrap")
}

async fn submit_fg_as(
    state: &ServiceState,
    principal: &lingxi_service::Principal,
    session: &str,
    input: &str,
    request_id: &str,
) -> Result<lingxi_service::ExecuteAccepted, lingxi_service::SessionExecuteError> {
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

/// The refusal-shape probe: returns the Debug string of a NON-acceptance,
/// or panics with the blind-re-execution evidence when the retry was
/// FRESHLY accepted (the defect's exact observable).
async fn must_be_refused(
    state: &ServiceState,
    principal: &lingxi_service::Principal,
    session: &str,
    input: &str,
    request_id: &str,
    what: &str,
) -> String {
    match submit_fg_as(state, principal, session, input, request_id).await {
        Err(err) => format!("{err:?}"),
        Ok(accepted) => panic!(
            "{what}: retry of {request_id:?} was FRESHLY accepted ({accepted:?}) — \
             the silent blind re-execution of R03-RR2-F05-01"
        ),
    }
}

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
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

async fn run_status(state: &ServiceState, run_id: &str) -> Option<String> {
    query_text(state, "SELECT status FROM runs WHERE run_id = ?1", run_id).await
}

async fn cause_id_of(state: &ServiceState, run_id: &str) -> Option<String> {
    query_text(
        state,
        "SELECT cause_id FROM run_lineage WHERE run_id = ?1",
        run_id,
    )
    .await
}

fn legacy_ctx(run_id: &str, session: &str) -> RunContext {
    let run = RunId::new(run_id.to_string());
    RunContext {
        principal: KernelPrincipal::LocalUser,
        session_id: SessionId::new(session.to_string()),
        attempt: lingxi_kernel::attempt_id(&run, 1),
        run_id: run,
        generation: 1,
    }
}

/// Seeds a PRE-FIX user-run lineage row through the REAL storage API
/// (record_run_started + record_run_lineage with the RAW cause_id the
/// baseline build wrote), settled completed.
async fn seed_legacy_row(state: &ServiceState, run_id: &str, raw_cause_id: &str, session: &str) {
    state
        .storage()
        .record_run_started(&legacy_ctx(run_id, session), NOW_MS)
        .await
        .expect("legacy run started");
    state
        .storage()
        .record_run_lineage(
            &legacy_ctx(run_id, session),
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
    let ctx = legacy_ctx(run_id, session);
    state
        .storage()
        .commit_run_outcome(
            &ctx,
            RunOutcome {
                status: lingxi_protocol::RunStatus::Completed,
                reason: Some("completed.srv2_legacy_fixture".to_string()),
                key_events: vec![KeyEvent {
                    event_id: EventId::new(format!("{run_id}-done")),
                    payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                        lingxi_protocol::RunStateChangedPayload {
                            from: lingxi_protocol::RunStatus::Running,
                            to: lingxi_protocol::RunStatus::Completed,
                            reason: Some("completed.srv2_legacy_fixture".to_string()),
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

/// "Restart": close the real store, drop the service state, boot a FRESH
/// service on the SAME data root (in-process registries die with it).
async fn restart(
    home: &Path,
    state: ServiceState,
    provider: Arc<ScriptedProvider>,
    counter: &Path,
) -> ServiceState {
    state.storage().close().await.expect("close storage");
    drop(state);
    boot(home, provider, counter).await
}

// ── chain (a): padded id, restart, raw + canonical retries, fresh control ────

#[tokio::test]
async fn srv2_a_padded_id_raw_and_canonical_retries_bind_the_one_logical_request() {
    let tag = temp_tag("a");
    let home = std::env::temp_dir().join(format!("{tag}-home"));
    let counter = std::env::temp_dir().join(format!("{tag}-counter.txt"));
    let sess = "sess_local_alpha";

    // Process A: the padded id " req-9x " is legally accepted through the
    // REAL foreground surface; ONE external effect; the run completes.
    let provider_a = ScriptedProvider::new(vec![("SRV2A", scripted_steps())]);
    let state_a = boot(&home, provider_a, &counter).await;
    let first =
        submit_fg_as(&state_a, &owner_principal(), sess, "SRV2A: once", " req-9x ")
            .await
            .expect("the padded id must be legally accepted in process A");
    assert_eq!(
        run_status(&state_a, &first.run_id).await.as_deref(),
        Some("completed")
    );
    assert_eq!(external_count(&counter), 1, "exactly one external effect");
    assert_eq!(session_runs(&state_a, sess).await, 1);

    // Restart on the same data root.
    let provider_b =
        ScriptedProvider::new(vec![("SRV2A", scripted_steps()), ("SRV2FRESH", scripted_steps())]);
    let state_b = restart(&home, state_a, provider_b, &counter).await;

    // The RAW retry must be a refusal naming process A's run.
    let dbg_raw = must_be_refused(
        &state_b,
        &owner_principal(),
        sess,
        "SRV2A: once",
        " req-9x ",
        "raw padded retry",
    )
    .await;
    assert!(
        dbg_raw.contains("RequestIdBoundToEarlierRun"),
        "raw retry refusal shape must be RequestIdBoundToEarlierRun, got {dbg_raw}"
    );
    assert!(
        dbg_raw.contains(&first.run_id),
        "the refusal must NAME process A's run {}, got {dbg_raw}",
        first.run_id
    );
    assert!(
        !dbg_raw.contains("RequestIdBoundAmbiguous"),
        "a single binding must not be reported ambiguous: {dbg_raw}"
    );

    // The CANONICAL retry must resolve to the SAME logical request.
    let dbg_canonical = must_be_refused(
        &state_b,
        &owner_principal(),
        sess,
        "SRV2A: once",
        "req-9x",
        "canonical retry",
    )
    .await;
    assert!(
        dbg_canonical.contains("RequestIdBoundToEarlierRun")
            && dbg_canonical.contains(&first.run_id),
        "canonical retry must bind the SAME earlier run, got {dbg_canonical}"
    );

    // No second effective execution / external effect; canonical echo.
    assert!(
        dbg_raw.contains("request_id: \"req-9x\""),
        "the refusal must echo the CANONICAL id, got {dbg_raw}"
    );
    assert_eq!(session_runs(&state_b, sess).await, 1);
    assert_eq!(external_count(&counter), 1, "the counter must not grow");

    // The durable anchor is built from the CANONICAL id.
    assert_eq!(
        cause_id_of(&state_b, &first.run_id).await.as_deref(),
        Some("request:req-9x"),
        "the durable anchor must be the canonical form"
    );

    // Over-blocking control: a BRAND-NEW id is still admitted after the
    // restart and completes normally (a blanket-refusal is not a fix).
    let fresh = submit_fg_as(
        &state_b,
        &owner_principal(),
        sess,
        "SRV2FRESH: once",
        "req-fresh-9z",
    )
    .await
    .expect("a brand-new id must stay admissible after the restart");
    assert_eq!(
        run_status(&state_b, &fresh.run_id).await.as_deref(),
        Some("completed")
    );
    assert_eq!(external_count(&counter), 2);
    assert_eq!(session_runs(&state_b, sess).await, 2);

    state_b.storage().close().await.expect("close B");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_file(&counter);
}

// ── chain (b): colliding pre-fix cause_id rows -> explicit ambiguity ────────

#[tokio::test]
async fn srv2_b_colliding_legacy_rows_refuse_as_explicit_ambiguity_never_a_pick() {
    let tag = temp_tag("b");
    let home = std::env::temp_dir().join(format!("{tag}-home"));
    let counter = std::env::temp_dir().join(format!("{tag}-counter.txt"));
    let sess = "sess_local_alpha";

    // Process A seeds TWO pre-fix rows whose raw ids normalize to ONE
    // logical key (the frozen duplicate-execution shape the baseline
    // could leave behind: padded and CRLF-wrapped), then dies.
    let state_a = boot(&home, ScriptedProvider::new(vec![]), &counter).await;
    seed_legacy_row(&state_a, "srv2_legacy_a", "request: req-9x ", sess).await;
    seed_legacy_row(&state_a, "srv2_legacy_b", "request:\r\nreq-9x\r\n", sess).await;
    let state_b = restart(&home, state_a, ScriptedProvider::new(vec![]), &counter).await;

    // Every spelling of the logical key must be refused as an EXPLICIT
    // AMBIGUITY naming EVERY bound run — never a silent pick of one
    // binding, never a fresh execution.
    for retry_id in ["req-9x", " req-9x ", "\r\nreq-9x\r\n"] {
        let dbg = must_be_refused(
            &state_b,
            &owner_principal(),
            sess,
            "SRV2B: once",
            retry_id,
            "colliding-legacy retry",
        )
        .await;
        assert!(
            dbg.contains("RequestIdBoundAmbiguous"),
            "colliding legacy rows must refuse as an EXPLICIT ambiguity (retry {retry_id:?}), \
             got {dbg}"
        );
        assert!(
            !dbg.contains("RequestIdBoundToEarlierRun"),
            "the ambiguous refusal must not degrade into a single-run pick: {dbg}"
        );
        assert!(
            dbg.contains("srv2_legacy_a") && dbg.contains("srv2_legacy_b"),
            "the ambiguity must name EVERY bound run (retry {retry_id:?}), got {dbg}"
        );
    }

    // Nothing re-executed; nothing rewritten; both frozen rows verbatim.
    assert_eq!(session_runs(&state_b, sess).await, 2);
    assert_eq!(external_count(&counter), 0);
    assert_eq!(
        cause_id_of(&state_b, "srv2_legacy_a").await.as_deref(),
        Some("request: req-9x "),
        "the frozen legacy row must not be rewritten"
    );
    assert_eq!(
        cause_id_of(&state_b, "srv2_legacy_b").await.as_deref(),
        Some("request:\r\nreq-9x\r\n"),
        "the frozen legacy row must not be rewritten"
    );

    state_b.storage().close().await.expect("close B");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_file(&counter);
}

// ── chain (c): namespace isolation of the same logical id ───────────────────

#[tokio::test]
async fn srv2_c_same_logical_id_never_crosses_session_or_principal_namespaces() {
    let tag = temp_tag("c");
    let home = std::env::temp_dir().join(format!("{tag}-home"));
    let counter = std::env::temp_dir().join(format!("{tag}-counter.txt"));

    // Process A: the SAME padded logical id " iso-7 " is admitted in THREE
    // independent namespaces — owner's session X, owner's session Y, and a
    // device principal of the same user in session X. Each is its OWN
    // admission (a fresh run, a real external effect).
    let provider_a = ScriptedProvider::new(vec![
        ("SRV2CX", scripted_steps()),
        ("SRV2CY", scripted_steps()),
        ("SRV2CD", scripted_steps()),
    ]);
    let state_a = boot(&home, provider_a, &counter).await;
    let owner = owner_principal();
    let device = device_principal_of_local_user();

    let rx = submit_fg_as(&state_a, &owner, "sess_local_alpha", "SRV2CX: once", " iso-7 ")
        .await
        .expect("owner session X accepts the padded id");
    let ry = submit_fg_as(&state_a, &owner, "sess_local_beta", "SRV2CY: once", " iso-7 ")
        .await
        .expect("owner session Y owns its OWN id namespace — fresh admission");
    assert!(
        !ry.replayed && ry.run_id != rx.run_id,
        "another session must not replay/conflict owner session X's run"
    );
    let rd = submit_fg_as(&state_a, &device, "sess_local_alpha", "SRV2CD: once", " iso-7 ")
        .await
        .expect("a device principal owns its OWN id namespace — fresh admission");
    assert!(
        !rd.replayed && rd.run_id != rx.run_id,
        "another principal kind must not replay/conflict the local owner's run"
    );
    assert_eq!(external_count(&counter), 3);
    assert_eq!(session_runs(&state_a, "sess_local_alpha").await, 2);

    // Restart: each namespace's same-key retry (mixed raw / canonical /
    // tab-padded spellings) must be refused naming ITS OWN run — never a
    // foreign namespace's run.
    let provider_b = ScriptedProvider::new(vec![
        ("SRV2CX", scripted_steps()),
        ("SRV2CY", scripted_steps()),
        ("SRV2CD", scripted_steps()),
    ]);
    let state_b = restart(&home, state_a, provider_b, &counter).await;

    let dbg = must_be_refused(
        &state_b,
        &owner,
        "sess_local_alpha",
        "SRV2CX: once",
        " iso-7 ",
        "owner session X retry",
    )
    .await;
    assert!(
        dbg.contains("RequestIdBoundToEarlierRun") && dbg.contains(&rx.run_id),
        "owner session X's refusal must name ITS OWN run {}, got {dbg}",
        rx.run_id
    );
    assert!(
        !dbg.contains(&rd.run_id),
        "owner session X must never see the device-principal binding: {dbg}"
    );

    let dbg = must_be_refused(
        &state_b,
        &owner,
        "sess_local_beta",
        "SRV2CY: once",
        "iso-7",
        "owner session Y retry",
    )
    .await;
    assert!(
        dbg.contains("RequestIdBoundToEarlierRun") && dbg.contains(&ry.run_id),
        "owner session Y's refusal must name ITS OWN run {}, got {dbg}",
        ry.run_id
    );
    assert!(
        !dbg.contains(&rx.run_id),
        "session Y must never see session X's binding: {dbg}"
    );

    let dbg = must_be_refused(
        &state_b,
        &device,
        "sess_local_alpha",
        "SRV2CD: once",
        "\tiso-7\t",
        "device principal retry",
    )
    .await;
    assert!(
        dbg.contains("RequestIdBoundToEarlierRun") && dbg.contains(&rd.run_id),
        "the device principal's refusal must name ITS OWN run {}, got {dbg}",
        rd.run_id
    );
    assert!(
        !dbg.contains(&rx.run_id),
        "the device namespace must never see the local owner's binding: {dbg}"
    );

    // No cross-namespace execution happened on the retries.
    assert_eq!(external_count(&counter), 3, "no namespace re-executed");
    assert_eq!(session_runs(&state_b, "sess_local_alpha").await, 2);
    assert_eq!(session_runs(&state_b, "sess_local_beta").await, 1);

    state_b.storage().close().await.expect("close B");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_file(&counter);
}
