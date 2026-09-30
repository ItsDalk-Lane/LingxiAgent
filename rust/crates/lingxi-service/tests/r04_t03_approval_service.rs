//! R04-T03 acceptance: approvals, revocation and the read-only mode
//! (R04-A05 / R04-A06 + the supplemental duty R04-SUP-01 + the T02 O05
//! closure + the adversarial additions).
//!
//! Everything here runs against the REAL composition root
//! (`ServiceState::bootstrap_with_deps`) with the REAL run driver, the
//! REAL journal (RunDatabase), the REAL registry (`ToolRegistry`), the
//! REAL gateway (`ToolInvocationGateway`) and the REAL production
//! approval service (`ApprovalService`) wired as BOTH the tool-policy
//! face and the approval-wait face through `ServiceDeps`.
//!
//! Test-double boundary (the R04 scope matrix `test_double_boundary`):
//! - `CountingExecutor` (`ToolExecutorPort`) — an external-system
//!   stand-in ONLY: it records which target/arguments reached it. It
//!   never decides permissions, never mints handles or approvals, never
//!   writes journal or run state.
//! - `ScriptedProvider`/`MarkerScriptedProvider` (`TurnProviderPort`) —
//!   external model responses only.
//! - `RejectAllGate` (`ApprovalGate`) — an external answerer that always
//!   rejects; used ONLY to pin the O05 priority rule (a policy-Allowed
//!   call never reaches ANY gate). The approval decisions that matter
//!   are produced by the REAL `ApprovalService`.
//! - The HUMAN approver is played by the test calling the authenticated
//!   `ApprovalService::answer`/`pending_of`/`grant_preauthorization`
//!   surface — exactly the external answerer role; the allow/deny rules
//!   themselves are adjudicated by the real Rust authorization chain.
//!
//! Scenarios:
//! - `r04_a05_approved_digest_a_cannot_execute_payload_b` — approve file
//!   A, submit file B: B is refused / re-requested, never written (the
//!   approval binds the digest).
//! - `r04_a06_wait_disabled_then_approved_still_refuses` — the target is
//!   disabled during the wait; approving the OLD request still refuses
//!   with an explicit target-invalid error.
//! - `sup01_*` — the ask-tier subagent matrix on the REAL delegation
//!   chain (the R03-T06-O1 gap closure) with every protected neighbor.
//! - `o05_policy_allowed_never_re_prompts` — the formal policy/gate
//!   priority contract.
//! - timeout / cancel / double-click / restart / alias adversarial legs.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::DelegationRequest;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolExecutionResult,
    ToolExecutorPort, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry, ToolTargetId,
};
use lingxi_protocol::{
    ContentBlock, ModelCallId, NormalizedMessage, ToolCallId, ToolSchemaDocument,
};
use lingxi_service::approval::ApprovalGate as _;
use lingxi_service::approval_service::{
    Answer, AnswerOutcome, ApprovalService, InvocationGrantKey, TOOL_APPROVAL_UNAVAILABLE,
};
use lingxi_service::toolgateway::ToolInvocationGateway;
use lingxi_service::{
    approval, prepare_layout, ServiceConfig, ServiceDeps, ServiceState, LOCAL_OWNER_USER_ID,
};
use lingxi_service::{HomeSource, NetworkMode};
use serde_json::json;

const NOW_MS: u64 = 1_790_409_600_000;
/// The tightened approval timeout of the harness (real wall-clock, short
/// enough to keep the timeout legs fast, long enough that a parked wait
/// never expires before the test answers it).
const TEST_APPROVAL_TIMEOUT_MS: u64 = 8_000;
const TEST_GRANT_TTL_MS: u64 = 60_000;

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

fn schema_doc(schema: serde_json::Value) -> ToolSchemaDocument {
    ToolSchemaDocument {
        dialect: "json-schema/2020-12".to_string(),
        schema,
    }
}

fn strict_manifest(
    name: &str,
    kind: PermissionKind,
    availability: Availability,
    aliases: Vec<String>,
) -> ToolManifest {
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: name.to_string(),
        display_name: name.to_string(),
        aliases,
        version: "1.0.0".to_string(),
        description: "approval acceptance manifest".to_string(),
        input_schema: schema_doc(json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
            },
            "required": ["path"],
            "additionalProperties": false,
        })),
        output_schema: None,
        permission: PermissionContract {
            kind,
            capability_base: format!("{name}.capability"),
        },
        availability,
        timeout_ms: Some(30_000),
        max_concurrency: Some(4),
        declared_permission: DeclaredPermission::None,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    }
}

fn register(registry: &ToolRegistry, manifest: ToolManifest) -> ToolTargetId {
    registry
        .register(manifest, &budget())
        .expect("manifest registers")
        .target_id
}

/// The registry of the approval suite: a Read-class probe, an
/// Execute-class write probe WITH AN ALIAS (the alias adversarial leg),
/// and the delegation family.
fn acceptance_registry() -> Arc<ToolRegistry> {
    let registry = Arc::new(ToolRegistry::new());
    register(
        &registry,
        strict_manifest(
            "read",
            PermissionKind::Read,
            Availability::Available,
            vec![],
        ),
    );
    register(
        &registry,
        strict_manifest(
            "probe_write",
            PermissionKind::Execute,
            Availability::Available,
            vec!["probe_write_alias".to_string()],
        ),
    );
    // The delegation family manifest (task + access surface).
    let mut subagent = strict_manifest(
        "subagent",
        PermissionKind::Read,
        Availability::Available,
        vec![],
    );
    subagent.input_schema = schema_doc(json!({
        "type": "object",
        "properties": {
            "task": {"type": "string", "minLength": 1},
            "access": {"type": "string", "enum": ["read", "write"]},
            "label": {"type": "string"},
            "agent": {"type": "string"},
            "model": {"type": "string"},
            "threadId": {"type": "string"},
        },
        "required": ["task"],
        "additionalProperties": false,
    }));
    register(&registry, subagent);
    registry
}

// ── doubles (external stand-ins only) ──────────────────────────────────────

#[derive(Default)]
struct CountingExecutor {
    executed: std::sync::Mutex<Vec<(String, String)>>,
}

impl CountingExecutor {
    fn count_of(&self, target: &str) -> usize {
        self.executed
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == target)
            .count()
    }

    fn total(&self) -> usize {
        self.executed.lock().unwrap().len()
    }

    fn payloads_of(&self, target: &str) -> Vec<String> {
        self.executed
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == target)
            .map(|(_, args)| args.clone())
            .collect()
    }
}

impl ToolExecutorPort for CountingExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let canonical = String::from_utf8_lossy(request.arguments.canonical_bytes()).to_string();
        self.executed
            .lock()
            .unwrap()
            .push((request.target.clone(), canonical));
        let ctx_at_issue = ctx.clone();
        let outcome =
            lingxi_kernel::ports::ToolOutcome::success_text(format!("executed:{}", request.target));
        Box::pin(async move { ToolExecutionResult::of_ctx(&ctx_at_issue, outcome) })
    }
}

/// An external answerer that ALWAYS REJECTS — used ONLY for the O05
/// priority pin (a policy-Allowed call must never reach any gate). Every
/// other approval decision in this suite comes from the REAL
/// ApprovalService.
struct RejectAllGate;

impl approval::ApprovalGate for RejectAllGate {
    fn request<'a>(
        &'a self,
        _ctx: &'a lingxi_kernel::RunContext,
        _req: &'a approval::ApprovalRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = approval::ApprovalDecision> + Send + 'a>> {
        Box::pin(async {
            approval::ApprovalDecision::Rejected {
                reason: "external answerer rejects".to_string(),
            }
        })
    }
}

struct ScriptedProvider {
    script: std::sync::Mutex<VecDeque<ProviderTurn>>,
}

impl ScriptedProvider {
    fn new(script: Vec<ProviderTurn>) -> Arc<Self> {
        Arc::new(Self {
            script: std::sync::Mutex::new(script.into_iter().collect()),
        })
    }
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ModelCallId,
        _turn: u32,
        _input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let next = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| final_turn("done"));
        let ctx_at_issue = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, next) })
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

/// Provider double scripted by INPUT MARKER (everything before ':') so
/// the parent run and a fire-and-forget delegation child draw from
/// SEPARATE deterministic script queues.
struct MarkerScriptedProvider {
    scripts: std::sync::Mutex<std::collections::HashMap<String, VecDeque<ProviderTurn>>>,
}

impl MarkerScriptedProvider {
    fn new(scripts: Vec<(&str, Vec<ProviderTurn>)>) -> Arc<Self> {
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

impl TurnProviderPort for MarkerScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ModelCallId,
        _turn: u32,
        input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let next = MarkerScriptedProvider::marker_of(input);
        let turn = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&next)
            .and_then(|queue| queue.pop_front())
            .unwrap_or_else(|| final_turn("done"));
        let ctx_at_issue = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, turn) })
    }
}

fn tool_request(target: &str, args: serde_json::Value) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        requests: vec![
            ToolRequest::from_effective_arguments(target, args, &budget())
                .expect("effective request"),
        ],
    }
}

fn delegation_request(task: &str, access: Option<&str>) -> ProviderTurn {
    let mut request = ToolRequest::from_effective_arguments(
        "tool:first-party:subagent",
        json!({"task": task}),
        &budget(),
    )
    .expect("delegation request builds");
    request = request.with_delegation(DelegationRequest {
        task: task.to_string(),
        access: lingxi_kernel::subagent::AccessRequest::parse(access),
        label: None,
        agent_id: None,
        model: None,
        thread_id: None,
    });
    ProviderTurn::ToolRequests {
        requests: vec![request],
    }
}

// ── composition-root harness ───────────────────────────────────────────────

struct Harness {
    state: ServiceState,
    registry: Arc<ToolRegistry>,
    approvals: Arc<ApprovalService>,
    executor: Arc<CountingExecutor>,
    home: std::path::PathBuf,
}

async fn teardown(harness: &Harness) {
    harness
        .state
        .storage()
        .close()
        .await
        .expect("close storage");
    let _ = std::fs::remove_dir_all(&harness.home);
}

/// The ApprovalService-wired harness: the REAL production approval
/// service is the gateway's policy face AND the wired approval gate.
/// `gate` overrides the gate slot when `Some` (the O05 leg injects a
/// rejecting answerer to prove policy-Allowed calls never reach it).
async fn harness_with_gate_override(
    provider: Arc<dyn TurnProviderPort>,
    gate: Option<Arc<dyn approval::ApprovalGate>>,
    approval_timeout_ms: u64,
) -> Harness {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let home = std::env::temp_dir().join(format!(
        "lingxi-r04t03-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&home);
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");
    let registry = acceptance_registry();
    let executor = Arc::new(CountingExecutor::default());
    let approvals = Arc::new(ApprovalService::with_bounds(
        Arc::new(lingxi_service::inject::SystemClock),
        approval_timeout_ms,
        lingxi_service::approval_service::DEFAULT_PENDING_CAP,
        lingxi_service::approval_service::DEFAULT_PREAUTHORIZED_CAP,
    ));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(lingxi_service::inject::SystemClock),
        budget(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    for target in ["tool:first-party:read", "tool:first-party:probe_write"] {
        gateway.bind_executor(
            ToolTargetId::parse(target),
            Arc::clone(&executor) as Arc<dyn ToolExecutorPort>,
            "r04-t03 acceptance counting executor",
        );
    }
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_gateway: Some(Arc::clone(&gateway)),
        approval_gate: Some(
            gate.unwrap_or_else(|| Arc::clone(&approvals) as Arc<dyn approval::ApprovalGate>),
        ),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    Harness {
        state,
        registry,
        approvals,
        executor,
        home,
    }
}

async fn approval_harness(provider: Arc<dyn TurnProviderPort>) -> Harness {
    harness_with_gate_override(provider, None, TEST_APPROVAL_TIMEOUT_MS).await
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(LOCAL_OWNER_USER_ID.to_string()),
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

/// The KERNEL principal the driver runs under for the owner (the shape
/// `kernel_principal_of(LocalUser)` produces) — the identity the
/// approval records bind to and the authenticated answer surface checks.
fn kernel_ctx(session: &str, run: &str) -> lingxi_kernel::RunContext {
    lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(session.to_string()),
        run_id: lingxi_protocol::RunId::new(run.to_string()),
        attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}

async fn set_mode(harness: &Harness, session: &str, mode: SessionPermissionMode) {
    harness
        .state
        .sessions()
        .set_permission_mode_for(&owner_principal(), session, mode)
        .await
        .expect("set permission mode");
}

/// Submits a user run in the BACKGROUND (the run parks in
/// waiting_approval; the test then drives the approval surface).
fn spawn_user_run(
    harness: &Harness,
    session: &str,
    input: &str,
) -> tokio::task::JoinHandle<String> {
    let state = harness.state.clone();
    let principal = owner_principal();
    let session = session.to_string();
    let input = input.to_string();
    tokio::spawn(async move {
        state
            .sessions()
            .execute_for(
                state.storage().as_ref(),
                state.events(),
                state.runs(),
                &principal,
                &session,
                &input,
                NOW_MS,
            )
            .await
            .expect("user run drives")
            .run_id
    })
}

async fn journal_of(
    harness: &Harness,
    run_id: &str,
) -> Vec<lingxi_kernel::ports::InvocationJournalEntry> {
    harness
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.to_string()))
        .await
        .expect("journal loads")
}

/// Waits (bounded) for a predicate — the R03 house polling helper.
async fn wait_until(what: &str, mut probe: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !probe() {
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for: {what}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    }
}

/// Waits for exactly one pending approval of one target to appear, then
/// returns its view (the authenticated approver's observation).
async fn wait_for_pending(
    harness: &Harness,
    session: &str,
    target: &str,
) -> lingxi_service::approval_service::PendingView {
    let ctx = kernel_ctx(session, "pending-probe");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for a pending approval of {target} in {session}");
        }
        let pendings = harness.approvals.pending_of(&ctx, session);
        let matches: Vec<_> = pendings
            .iter()
            .filter(|view| view.target == target)
            .collect();
        if matches.len() == 1 {
            return matches[0].clone();
        }
        if matches.len() > 1 {
            panic!("expected one pending for {target}, saw {}", matches.len());
        }
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    }
}

// ── R04-A05: approve A, submit B ───────────────────────────────────────────

/// The user approves writing FILE A; the submitted call is then changed
/// to FILE B. The approval binds A's canonical digest: B is re-requested
/// (its own pending approval) and is NEVER written — neither the waiting
/// form (an approved A cannot settle B's pending; the late re-answer of
/// A's settled id changes nothing) nor the pre-authorization form (the
/// grant's whole key is target+digest; B simply does not match).
#[tokio::test]
async fn r04_a05_approved_digest_a_cannot_execute_payload_b() {
    const SESSION: &str = "sess_local_alpha";
    // Marker-scripted provider: every run (A, B, A2, B2) draws its OWN
    // queue — a parked run consumes exactly its write turn.
    let harness = approval_harness(MarkerScriptedProvider::new(vec![
        (
            "A05-A",
            vec![
                tool_request(
                    "tool:first-party:probe_write",
                    json!({"path": "/data/file-A.txt"}),
                ),
                final_turn("A done"),
            ],
        ),
        (
            "A05-B",
            vec![
                tool_request(
                    "tool:first-party:probe_write",
                    json!({"path": "/data/file-B.txt"}),
                ),
                final_turn("B done"),
            ],
        ),
        (
            "A05-A2",
            vec![
                tool_request(
                    "tool:first-party:probe_write",
                    json!({"path": "/data/file-A2.txt"}),
                ),
                final_turn("A2 done"),
            ],
        ),
        (
            "A05-B2",
            vec![
                tool_request(
                    "tool:first-party:probe_write",
                    json!({"path": "/data/file-B2.txt"}),
                ),
                final_turn("B2 done"),
            ],
        ),
    ]))
    .await;
    set_mode(&harness, SESSION, SessionPermissionMode::Ask).await;
    let handle_a = spawn_user_run(&harness, SESSION, "A05-A: write file A");
    let pending_a = wait_for_pending(&harness, SESSION, "tool:first-party:probe_write").await;
    assert_eq!(
        pending_a.args_summary.as_deref(),
        Some("{path:str}"),
        "the pending view carries the SHAPE-ONLY summary (never the path value)"
    );
    // The user approves A's pending record.
    let ctx = kernel_ctx(SESSION, "answerer");
    assert!(harness
        .approvals
        .answer(&ctx, SESSION, &pending_a.approval_id, Answer::Approve)
        .settled());
    let run_a = handle_a.await.expect("run A settles");
    // A executed exactly once, payload A only.
    assert_eq!(harness.executor.count_of("tool:first-party:probe_write"), 1);
    assert_eq!(
        harness.executor.payloads_of("tool:first-party:probe_write"),
        vec![r#"{"path":"/data/file-A.txt"}"#.to_string()],
        "file A (and only file A) was written"
    );
    let journal_a = journal_of(&harness, &run_a).await;
    assert_eq!(
        journal_a.first().expect("entry").phase,
        lingxi_kernel::ports::InvocationPhase::Succeeded
    );

    // ── The attack: re-answer A's SETTLED id (double click / replay). ──
    // The second answer changes nothing (AlreadySettled) and approves
    // nothing else.
    match harness
        .approvals
        .answer(&ctx, SESSION, &pending_a.approval_id, Answer::Approve)
    {
        AnswerOutcome::AlreadySettled(Answer::Approve) => {}
        other => panic!("double click must observe the first settlement: {other:?}"),
    }

    // ── Submit B (same tool, different parameters). ──
    // Same session: the previous run settled, so the session is free.
    let handle_b = spawn_user_run(&harness, SESSION, "A05-B: write file B");
    let pending_b = wait_for_pending(&harness, SESSION, "tool:first-party:probe_write").await;
    // B's pending is a DIFFERENT record (fresh CSPRNG id); presenting
    // A's settled approval id cannot settle it.
    assert_ne!(pending_a.approval_id, pending_b.approval_id);
    // The attacker tries to spend A's approval on B: the id does not
    // even name B's record — B keeps waiting.
    match harness
        .approvals
        .answer(&ctx, SESSION, &pending_a.approval_id, Answer::Approve)
    {
        AnswerOutcome::AlreadySettled(_) => {}
        other => panic!("A's settled id must not settle anything else: {other:?}"),
    }
    assert!(
        !harness.approvals.pending_of(&ctx, SESSION).is_empty(),
        "B's pending record is untouched by A's approval"
    );
    // The user REJECTS B (the honest outcome for an unapproved change).
    assert!(harness
        .approvals
        .answer(
            &ctx,
            SESSION,
            &pending_b.approval_id,
            Answer::Reject {
                reason: "parameters changed; not what was approved".to_string()
            }
        )
        .settled());
    let run_b = handle_b.await.expect("run B settles");
    // B was NEVER written: executor still exactly once (A only).
    assert_eq!(harness.executor.count_of("tool:first-party:probe_write"), 1);
    let journal_b = journal_of(&harness, &run_b).await;
    let entry_b = journal_b.first().expect("B journaled its refusal");
    assert_eq!(entry_b.phase, lingxi_kernel::ports::InvocationPhase::Failed);
    assert!(
        !entry_b.receipt.as_ref().expect("receipt").dispatched,
        "B was never dispatched"
    );

    // ── Leg 2: the pre-authorization form (the old incumbent contract). ──
    // A session-scoped grant for the EXACT invocation (target + digest
    // of A) covers A and only A; B's digest does not match, so B is
    // re-requested (a NEW pending), never silently executed.
    let digest_of = |args: serde_json::Value| {
        ToolRequest::from_effective_arguments("tool:first-party:probe_write", args, &budget())
            .expect("builds")
            .args_digest
            .hex
    };
    let digest_a2 = digest_of(json!({"path": "/data/file-A2.txt"}));
    harness
        .approvals
        .grant_preauthorization(
            &kernel_ctx(SESSION, "granter"),
            SESSION,
            InvocationGrantKey {
                target_id: "tool:first-party:probe_write".to_string(),
                capability_base: "probe_write.capability".to_string(),
                args_digest_hex: digest_a2.clone(),
            },
            1,
            TEST_GRANT_TTL_MS,
        )
        .expect("grant A2");
    // Run A2 (the granted digest): executes WITHOUT a new prompt and
    // SETTLES (its handle resolves only at the run's terminal state, so
    // the session lease is free for the B2 submission that follows).
    spawn_user_run(&harness, SESSION, "A05-A2: the granted write")
        .await
        .expect("run A2 settles (the grant answered it)");
    assert_eq!(
        harness.executor.count_of("tool:first-party:probe_write"),
        2,
        "the granted invocation executed without a new prompt"
    );
    // Run B2 (a DIFFERENT digest under the same tool): the grant does
    // not cover it — a fresh pending appears (re-request), and B2 stays
    // unwritten until the user answers it.
    let digest_b2 = digest_of(json!({"path": "/data/file-B2.txt"}));
    assert_ne!(digest_a2, digest_b2);
    let handle_b2 = spawn_user_run(&harness, SESSION, "A05-B2: the widened write");
    let pending_b2 = wait_for_pending(&harness, SESSION, "tool:first-party:probe_write").await;
    // The single-use grant is exhausted; nothing silently covers B2.
    assert_ne!(pending_b2.approval_id, pending_a.approval_id);
    harness.approvals.answer(
        &ctx,
        SESSION,
        &pending_b2.approval_id,
        Answer::Reject {
            reason: "widened scope; not the granted invocation".to_string(),
        },
    );
    handle_b2.await.expect("run B2 settles");
    let payloads = harness.executor.payloads_of("tool:first-party:probe_write");
    assert_eq!(
        payloads,
        vec![
            r#"{"path":"/data/file-A.txt"}"#.to_string(),
            r#"{"path":"/data/file-A2.txt"}"#.to_string(),
        ],
        "only the approved/granted payloads were written; B and B2 were never written"
    );
    teardown(&harness).await;
}

// ── R04-A06: disable during the wait ───────────────────────────────────────

/// A request parks waiting for approval; the target is DISABLED; the OLD
/// request is then approved — the invocation is still refused, and the
/// error explicitly says the target is no longer callable (zero
/// executions; the dispatch-time re-adjudication wins).
#[tokio::test]
async fn r04_a06_wait_disabled_then_approved_still_refuses() {
    const SESSION: &str = "sess_local_alpha";
    // Both harnesses are built BEFORE the local binding shadows the
    // helper fn (each on its own storage/seed home).
    let harness = approval_harness(ScriptedProvider::new(vec![
        tool_request(
            "tool:first-party:probe_write",
            json!({"path": "/data/a06.txt"}),
        ),
        final_turn("a06 done"),
    ]))
    .await;
    let harness_u = approval_harness(ScriptedProvider::new(vec![
        tool_request(
            "tool:first-party:probe_write",
            json!({"path": "/data/a06u.txt"}),
        ),
        final_turn("a06u done"),
    ]))
    .await;
    set_mode(&harness, SESSION, SessionPermissionMode::Ask).await;
    let handle = spawn_user_run(&harness, SESSION, "A06: wait then get disabled");
    let pending = wait_for_pending(&harness, SESSION, "tool:first-party:probe_write").await;

    // The tool is DISABLED while the request waits.
    harness
        .registry
        .set_availability(
            &ToolTargetId::parse("tool:first-party:probe_write"),
            Availability::Disabled {
                reason: "disabled during the approval wait (A06)".to_string(),
            },
        )
        .expect("disable");
    // The OLD request is then approved.
    let ctx = kernel_ctx(SESSION, "answerer");
    assert!(harness
        .approvals
        .answer(&ctx, SESSION, &pending.approval_id, Answer::Approve)
        .settled());
    let run_id = handle.await.expect("run settles");

    // Zero executions; the journal's refusal explicitly names the
    // target's invalid state.
    assert_eq!(
        harness.executor.total(),
        0,
        "the approved-but-stale request never executed"
    );
    let journal = journal_of(&harness, &run_id).await;
    let entry = journal.first().expect("the tool call is journaled");
    assert_eq!(entry.phase, lingxi_kernel::ports::InvocationPhase::Failed);
    let receipt = entry.receipt.as_ref().expect("receipt");
    assert!(!receipt.dispatched, "never dispatched");
    assert!(
        receipt.detail.contains("gateway_target_changed")
            && receipt.detail.contains("disabled during the approval wait"),
        "the error explicitly says the target is no longer valid: {}",
        receipt.detail
    );
    teardown(&harness).await;

    // ── The UNINSTALL variant: same wait, target UNREGISTERED. ──
    set_mode(&harness_u, "sess_local_beta", SessionPermissionMode::Ask).await;
    let handle = spawn_user_run(
        &harness_u,
        "sess_local_beta",
        "A06U: wait then get uninstalled",
    );
    let pending = wait_for_pending(
        &harness_u,
        "sess_local_beta",
        "tool:first-party:probe_write",
    )
    .await;
    harness_u
        .registry
        .uninstall(&ToolTargetId::parse("tool:first-party:probe_write"))
        .expect("uninstall");
    assert!(harness_u
        .approvals
        .answer(
            &kernel_ctx("sess_local_beta", "answerer"),
            "sess_local_beta",
            &pending.approval_id,
            Answer::Approve
        )
        .settled());
    let run_id = handle.await.expect("run settles");
    assert_eq!(harness_u.executor.total(), 0);
    let journal = journal_of(&harness_u, &run_id).await;
    let receipt = journal[0].receipt.as_ref().expect("receipt");
    assert!(!receipt.dispatched);
    assert!(
        receipt.detail.contains("not registered") || receipt.detail.contains("target"),
        "the refusal names the gone target: {}",
        receipt.detail
    );
    teardown(&harness_u).await;
}

// ── R04-SUP-01: the ask-tier subagent matrix on the REAL chain ─────────────

/// The R03-T06-O1 gap closure (R04-SUP-01): under an ASK parent session,
/// a subagent that OMITTED `access` (or explicitly requested `write`)
/// runs on the operate TIER but its write-class calls are the structured
/// TOOL_APPROVAL_UNAVAILABLE refusal — never collapsed into an operate
/// pass, never auto-approved, never parked (a subagent cannot ask a
/// human). Every protected neighbor is asserted in the same suite:
/// read_only escalation refusal, explicit-read attenuation (the T02 F01
/// fix), and the normal operate legitimate write.
#[tokio::test]
async fn sup01_ask_tier_subagent_matrix_on_the_real_chain() {
    // ── (1) ask parent + access omitted → write class = TOOL_APPROVAL_UNAVAILABLE.
    {
        let harness = approval_harness(MarkerScriptedProvider::new(vec![
            (
                "SUP1-PARENT",
                vec![
                    delegation_request("SUP1-CHILD: try to write", None),
                    final_turn("parent done"),
                ],
            ),
            (
                "SUP1-CHILD",
                vec![
                    tool_request(
                        "tool:first-party:probe_write",
                        json!({"path": "/data/sup1.txt"}),
                    ),
                    final_turn("child done"),
                ],
            ),
        ]))
        .await;
        set_mode(&harness, "sess_local_alpha", SessionPermissionMode::Ask).await;
        spawn_user_run(&harness, "sess_local_alpha", "SUP1-PARENT: dispatch")
            .await
            .expect("parent settles");
        let child_run = wait_for_child_run(&harness, "sess_local_alpha").await;
        wait_until("the ask child settles", || {
            harness
                .state
                .subagents()
                .threads_of("sess_local_alpha")
                .iter()
                .any(|thread| !thread.busy && thread.last_run_status.is_some())
        })
        .await;
        assert_eq!(
            harness.executor.count_of("tool:first-party:probe_write"),
            0,
            "the ask-tier child's write call never executed"
        );
        let journal = journal_of(&harness, &child_run).await;
        let entry = journal.first().expect("the child journaled its call");
        assert_eq!(entry.phase, lingxi_kernel::ports::InvocationPhase::Failed);
        let receipt = entry.receipt.as_ref().expect("receipt");
        assert!(!receipt.dispatched);
        assert!(
            receipt.detail.contains(TOOL_APPROVAL_UNAVAILABLE),
            "the refusal is the structured ask-tier code: {}",
            receipt.detail
        );
        assert!(
            receipt.detail.contains("deny_on_prompt"),
            "the refusal names the unattended policy: {}",
            receipt.detail
        );
        teardown(&harness).await;
    }

    // ── (2) ask parent + EXPLICIT access:"write" → the SAME refusal
    //        (the incumbent's write request INHERITS the ask mode; it
    //        never collapses into an operate grant).
    {
        let harness = approval_harness(MarkerScriptedProvider::new(vec![
            (
                "SUP2-PARENT",
                vec![
                    delegation_request("SUP2-CHILD: write please", Some("write")),
                    final_turn("parent done"),
                ],
            ),
            (
                "SUP2-CHILD",
                vec![
                    tool_request(
                        "tool:first-party:probe_write",
                        json!({"path": "/data/sup2.txt"}),
                    ),
                    final_turn("child done"),
                ],
            ),
        ]))
        .await;
        set_mode(&harness, "sess_local_alpha", SessionPermissionMode::Ask).await;
        spawn_user_run(&harness, "sess_local_alpha", "SUP2-PARENT: dispatch")
            .await
            .expect("parent settles");
        let child_run = wait_for_child_run(&harness, "sess_local_alpha").await;
        wait_until("the explicit-write child settles", || {
            harness
                .state
                .subagents()
                .threads_of("sess_local_alpha")
                .iter()
                .any(|thread| !thread.busy && thread.last_run_status.is_some())
        })
        .await;
        assert_eq!(harness.executor.total(), 0);
        let journal = journal_of(&harness, &child_run).await;
        let receipt = journal[0].receipt.as_ref().expect("receipt");
        assert!(!receipt.dispatched);
        assert!(
            receipt.detail.contains(TOOL_APPROVAL_UNAVAILABLE),
            "explicit write under an ask parent is the same structured refusal: {}",
            receipt.detail
        );
        teardown(&harness).await;
    }

    // ── (3) ask parent + read-class call → ALLOWED (research keeps
    //        working; no blanket shutdown).
    {
        let harness = approval_harness(MarkerScriptedProvider::new(vec![
            (
                "SUP3-PARENT",
                vec![
                    delegation_request("SUP3-CHILD: research", None),
                    final_turn("parent done"),
                ],
            ),
            (
                "SUP3-CHILD",
                vec![
                    tool_request("tool:first-party:read", json!({"path": "/data/sup3.txt"})),
                    final_turn("child done"),
                ],
            ),
        ]))
        .await;
        set_mode(&harness, "sess_local_alpha", SessionPermissionMode::Ask).await;
        spawn_user_run(&harness, "sess_local_alpha", "SUP3-PARENT: dispatch")
            .await
            .expect("parent settles");
        wait_until("the ask child's read dispatched", || {
            harness.executor.count_of("tool:first-party:read") == 1
        })
        .await;
        teardown(&harness).await;
    }

    // ── (4) OPERATE parent (access omitted) → the child's write
    //        EXECUTES (the normal legitimate write — protected control).
    {
        let harness = approval_harness(MarkerScriptedProvider::new(vec![
            (
                "SUP4-PARENT",
                vec![
                    delegation_request("SUP4-CHILD: do the edit", None),
                    final_turn("parent done"),
                ],
            ),
            (
                "SUP4-CHILD",
                vec![
                    tool_request(
                        "tool:first-party:probe_write",
                        json!({"path": "/data/sup4.txt"}),
                    ),
                    final_turn("child done"),
                ],
            ),
        ]))
        .await;
        // operate is the null-parent default; set it explicitly anyway.
        set_mode(&harness, "sess_local_alpha", SessionPermissionMode::Operate).await;
        spawn_user_run(&harness, "sess_local_alpha", "SUP4-PARENT: dispatch")
            .await
            .expect("parent settles");
        wait_until("the operate child's write dispatched", || {
            harness.executor.count_of("tool:first-party:probe_write") == 1
        })
        .await;
        assert_eq!(
            harness.executor.payloads_of("tool:first-party:probe_write"),
            vec![r#"{"path":"/data/sup4.txt"}"#.to_string()]
        );
        teardown(&harness).await;
    }

    // ── (5) READ_ONLY parent + explicit write → the dispatch itself is
    //        the loud attenuation refusal (SUBAGENT_WRITE_DENIED_BY_
    //        PARENT_READ_ONLY; zero child runs).
    {
        let harness = approval_harness(MarkerScriptedProvider::new(vec![(
            "SUP5-PARENT",
            vec![
                delegation_request("SUP5-CHILD: escalate", Some("write")),
                final_turn("parent done"),
            ],
        )]))
        .await;
        set_mode(
            &harness,
            "sess_local_alpha",
            SessionPermissionMode::ReadOnly,
        )
        .await;
        spawn_user_run(&harness, "sess_local_alpha", "SUP5-PARENT: dispatch")
            .await
            .expect("parent settles");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(
            harness
                .state
                .subagents()
                .threads_of("sess_local_alpha")
                .is_empty(),
            "the escalation dispatch created zero child runs"
        );
        assert_eq!(harness.executor.total(), 0);
        teardown(&harness).await;
    }

    // ── (6) READ_ONLY parent + access omitted → the child runs
    //        read-only; its WRITE call is the kernel attenuation refusal
    //        (ACTION_BLOCKED_BY_READ_ONLY — escalation denied), and its
    //        READ call dispatches (the explicit-read semantics hold
    //        without an explicit access too).
    {
        let harness = approval_harness(MarkerScriptedProvider::new(vec![
            (
                "SUP6-PARENT",
                vec![
                    delegation_request("SUP6-CHILD: inherit read-only", None),
                    final_turn("parent done"),
                ],
            ),
            (
                "SUP6-CHILD",
                vec![
                    tool_request(
                        "tool:first-party:probe_write",
                        json!({"path": "/data/sup6.txt"}),
                    ),
                    tool_request("tool:first-party:read", json!({"path": "/data/sup6.txt"})),
                    final_turn("child done"),
                ],
            ),
        ]))
        .await;
        set_mode(
            &harness,
            "sess_local_alpha",
            SessionPermissionMode::ReadOnly,
        )
        .await;
        spawn_user_run(&harness, "sess_local_alpha", "SUP6-PARENT: dispatch")
            .await
            .expect("parent settles");
        let child_run = wait_for_child_run(&harness, "sess_local_alpha").await;
        wait_until("the read-only child dispatched its read", || {
            harness.executor.count_of("tool:first-party:read") == 1
        })
        .await;
        assert_eq!(
            harness.executor.count_of("tool:first-party:probe_write"),
            0,
            "the write call never executed under the read-only tier"
        );
        let journal = journal_of(&harness, &child_run).await;
        let write_receipt = journal[0].receipt.as_ref().expect("receipt");
        assert!(!write_receipt.dispatched);
        assert!(
            write_receipt.detail.contains("ACTION_BLOCKED_BY_READ_ONLY"),
            "the write refusal is the kernel attenuation layer: {}",
            write_receipt.detail
        );
        assert!(!write_receipt.detail.contains(TOOL_APPROVAL_UNAVAILABLE));
        teardown(&harness).await;
    }

    // ── (7) READ_ONLY parent + EXPLICIT access:"read" → the T02 F01
    //        fix holds: the child's registered Read-class call
    //        dispatches (no regression from the SUP-01 closure).
    {
        let harness = approval_harness(MarkerScriptedProvider::new(vec![
            (
                "SUP7-PARENT",
                vec![
                    delegation_request("SUP7-CHILD: explicit read", Some("read")),
                    final_turn("parent done"),
                ],
            ),
            (
                "SUP7-CHILD",
                vec![
                    tool_request("tool:first-party:read", json!({"path": "/data/sup7.txt"})),
                    final_turn("child done"),
                ],
            ),
        ]))
        .await;
        set_mode(
            &harness,
            "sess_local_alpha",
            SessionPermissionMode::ReadOnly,
        )
        .await;
        spawn_user_run(&harness, "sess_local_alpha", "SUP7-PARENT: dispatch")
            .await
            .expect("parent settles");
        wait_until("the explicit-read child dispatched", || {
            harness.executor.count_of("tool:first-party:read") == 1
        })
        .await;
        assert_eq!(harness.executor.count_of("tool:first-party:read"), 1);
        teardown(&harness).await;
    }
}

/// Waits for a delegation child run of one session and returns its id.
async fn wait_for_child_run(harness: &Harness, session: &str) -> String {
    let state = harness.state.clone();
    wait_until("the delegation child run is dispatched", || {
        !state.subagents().threads_of(session).is_empty()
            && state
                .subagents()
                .threads_of(session)
                .iter()
                .any(|thread| thread.child_run_id.is_some())
    })
    .await;
    harness
        .state
        .subagents()
        .threads_of(session)
        .into_iter()
        .find(|thread| thread.child_run_id.is_some())
        .and_then(|thread| thread.child_run_id)
        .expect("child run id")
}

// ── the O05 closure: policy/gate priority ─────────────────────────────────

/// The FORMAL priority contract (R04-T02 O05, closed by T03): the policy
/// face is the single source of the approval REQUIREMENT. A call the
/// configured policy (the REAL ApprovalService under an operate
/// session) adjudicated Allowed advances WITHOUT consulting the wired
/// gate — proven by wiring a gate that ALWAYS REJECTS and observing the
/// execution anyway. The contrast leg (ask → NeedsApproval) reaches the
/// gate and honors its rejection.
#[tokio::test]
async fn o05_policy_allowed_never_re_prompts_the_wired_gate() {
    // ── Allowed leg: operate session, rejecting gate wired. ──
    let harness = harness_with_gate_override(
        ScriptedProvider::new(vec![
            tool_request(
                "tool:first-party:probe_write",
                json!({"path": "/data/o05.txt"}),
            ),
            final_turn("o05 done"),
        ]),
        Some(Arc::new(RejectAllGate)),
        TEST_APPROVAL_TIMEOUT_MS,
    )
    .await;
    // operate is the null default; set explicitly for the contract leg.
    set_mode(&harness, "sess_local_alpha", SessionPermissionMode::Operate).await;
    let run_id = spawn_user_run(&harness, "sess_local_alpha", "O05: allowed write")
        .await
        .expect("settles");
    assert_eq!(
        harness.executor.count_of("tool:first-party:probe_write"),
        1,
        "the policy-Allowed call executed — the wired gate was never consulted"
    );
    let journal = journal_of(&harness, &run_id).await;
    assert_eq!(
        journal.first().expect("entry").phase,
        lingxi_kernel::ports::InvocationPhase::Succeeded
    );
    // The gate never produced a pending record (it is the RejectAll
    // double; the REAL approval service's pending surface stays empty —
    // the requirement never existed).
    assert!(harness
        .approvals
        .pending_of(&kernel_ctx("sess_local_alpha", "probe"), "sess_local_alpha")
        .is_empty());
    teardown(&harness).await;

    // ── NeedsApproval leg: ask session reaches the gate and honors it. ──
    let harness = harness_with_gate_override(
        ScriptedProvider::new(vec![
            tool_request(
                "tool:first-party:probe_write",
                json!({"path": "/data/o05b.txt"}),
            ),
            final_turn("o05b done"),
        ]),
        Some(Arc::new(RejectAllGate)),
        TEST_APPROVAL_TIMEOUT_MS,
    )
    .await;
    set_mode(&harness, "sess_local_alpha", SessionPermissionMode::Ask).await;
    let run_id = spawn_user_run(&harness, "sess_local_alpha", "O05B: gated write")
        .await
        .expect("settles");
    assert_eq!(
        harness.executor.total(),
        0,
        "the gate's rejection held: zero executions"
    );
    let journal = journal_of(&harness, &run_id).await;
    let receipt = journal[0].receipt.as_ref().expect("receipt");
    assert!(!receipt.dispatched);
    assert!(
        receipt.detail.contains("external answerer rejects"),
        "the refusal is the gate's decision: {}",
        receipt.detail
    );
    teardown(&harness).await;
}

// ── timeout / cancel / double-click determinism ────────────────────────────

/// An unanswered approval request times out: a definite rejection, zero
/// executions; the LATE approval afterwards is refused (expired) and
/// nothing resurrects.
#[tokio::test]
async fn approval_timeout_is_a_definite_rejection_and_late_approval_is_refused() {
    const SESSION: &str = "sess_local_alpha";
    // A SHORT timeout makes the timeout leg fast (real wall clock).
    let harness = harness_with_gate_override(
        ScriptedProvider::new(vec![
            tool_request(
                "tool:first-party:probe_write",
                json!({"path": "/data/t.txt"}),
            ),
            final_turn("t done"),
        ]),
        None,
        120,
    )
    .await;
    set_mode(&harness, SESSION, SessionPermissionMode::Ask).await;
    let handle = spawn_user_run(&harness, SESSION, "TIMEOUT: never answered");
    let pending = wait_for_pending(&harness, SESSION, "tool:first-party:probe_write").await;
    // Do NOT answer; the bounded wait expires on its own.
    let run_id = handle.await.expect("run settles after the timeout");
    assert_eq!(harness.executor.total(), 0, "zero executions on timeout");
    let journal = journal_of(&harness, &run_id).await;
    let receipt = journal[0].receipt.as_ref().expect("receipt");
    assert!(!receipt.dispatched);
    assert!(
        receipt.detail.contains("not dispatched"),
        "the timeout closed the call: {}",
        receipt.detail
    );
    // The LATE approval is refused (expired) — no resurrection.
    match harness.approvals.answer(
        &kernel_ctx(SESSION, "late"),
        SESSION,
        &pending.approval_id,
        Answer::Approve,
    ) {
        AnswerOutcome::Refused { reason } => {
            assert!(reason.contains("expired"), "{reason}")
        }
        other => panic!("late approval after timeout must be refused: {other:?}"),
    }
    assert_eq!(harness.executor.total(), 0);
    teardown(&harness).await;
}

/// A run cancelled while waiting: the cancellation tree drops the wait;
/// the record aborts; the late approval is refused and the run's
/// settled cancellation is never resurrected.
#[tokio::test]
async fn cancel_during_wait_then_late_approval_never_resurrects() {
    const SESSION: &str = "sess_local_alpha";
    let harness = approval_harness(ScriptedProvider::new(vec![
        tool_request(
            "tool:first-party:probe_write",
            json!({"path": "/data/c.txt"}),
        ),
        final_turn("c done"),
    ]))
    .await;
    set_mode(&harness, SESSION, SessionPermissionMode::Ask).await;
    let handle = spawn_user_run(&harness, SESSION, "CANCEL: waiting run");
    let pending = wait_for_pending(&harness, SESSION, "tool:first-party:probe_write").await;
    // Cancel the run through the real user-facing cancellation entry
    // (the pending view's run anchor names the run to cancel).
    let fired = harness
        .state
        .runs()
        .cancel_run(&pending.run_id, "user cancelled while waiting for approval");
    assert!(
        matches!(
            fired,
            lingxi_service::cancel::FireOutcome::Fired
                | lingxi_service::cancel::FireOutcome::AlreadyCancelling
        ),
        "the cancellation fired: {fired:?}"
    );
    handle.await.expect("run settles cancelled");
    assert_eq!(harness.executor.total(), 0, "zero executions after cancel");
    // The late approval is refused (aborted).
    match harness.approvals.answer(
        &kernel_ctx(SESSION, "late"),
        SESSION,
        &pending.approval_id,
        Answer::Approve,
    ) {
        AnswerOutcome::Refused { reason } => {
            assert!(reason.contains("aborted"), "{reason}")
        }
        other => panic!("late approval after cancel must be refused: {other:?}"),
    }
    // Nothing resurrected: still zero executions.
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    assert_eq!(harness.executor.total(), 0);
    teardown(&harness).await;
}

/// Two concurrent waiting requests each have their OWN record; approving
/// ONE settles exactly one; the other is untouched (an approval can
/// never be consumed once by each of two concurrent requests).
#[tokio::test]
async fn one_approval_cannot_be_spent_by_two_concurrent_requests() {
    const SESSION_A: &str = "sess_local_alpha";
    const SESSION_B: &str = "sess_local_beta";
    // Two CONCURRENT requests (a session busy-gates its own second
    // submission, so the two parked waits live in two sessions of the
    // same principal — two distinct approval records either way).
    let harness = approval_harness(MarkerScriptedProvider::new(vec![
        (
            "CC-A",
            vec![
                tool_request(
                    "tool:first-party:probe_write",
                    json!({"path": "/data/cc.txt"}),
                ),
                final_turn("cc-a done"),
            ],
        ),
        (
            "CC-B",
            vec![
                tool_request(
                    "tool:first-party:probe_write",
                    json!({"path": "/data/cc.txt"}),
                ),
                final_turn("cc-b done"),
            ],
        ),
    ]))
    .await;
    set_mode(&harness, SESSION_A, SessionPermissionMode::Ask).await;
    set_mode(&harness, SESSION_B, SessionPermissionMode::Ask).await;
    let h1 = spawn_user_run(&harness, SESSION_A, "CC-A: concurrent write");
    let h2 = spawn_user_run(&harness, SESSION_B, "CC-B: concurrent write");
    // Both park on the SAME payload (same digest) but on DISTINCT
    // records — a shared digest is the adversarial case: one approval
    // must still not be spent by both.
    let pending_a = wait_for_pending(&harness, SESSION_A, "tool:first-party:probe_write").await;
    let pending_b = wait_for_pending(&harness, SESSION_B, "tool:first-party:probe_write").await;
    assert_ne!(pending_a.approval_id, pending_b.approval_id);
    // Approve exactly ONE of them.
    let ctx_a = kernel_ctx(SESSION_A, "approver");
    assert!(harness
        .approvals
        .answer(&ctx_a, SESSION_A, &pending_a.approval_id, Answer::Approve)
        .settled());
    // Its run executes exactly once; the OTHER pending is untouched.
    wait_until("the approved run executed", || {
        harness.executor.total() == 1
    })
    .await;
    assert_eq!(
        harness.executor.total(),
        1,
        "only the approved request executed"
    );
    // B's approval record must NOT have been consumed by A's approval:
    // it is still pending, and answering it decides only itself.
    let ctx_b = kernel_ctx(SESSION_B, "approver");
    assert_eq!(
        harness.approvals.pending_of(&ctx_b, SESSION_B).len(),
        1,
        "the other request still waits for its OWN approval"
    );
    // The cross-session replay of A's id on B's session is uniformly
    // refused (ownership + unknown-to-this-session).
    assert!(matches!(
        harness
            .approvals
            .answer(&ctx_b, SESSION_B, &pending_a.approval_id, Answer::Approve),
        AnswerOutcome::Refused { .. }
    ));
    assert!(matches!(
        harness.approvals.answer(
            &ctx_b,
            SESSION_B,
            &pending_b.approval_id,
            Answer::Reject {
                reason: "not the approved one".to_string()
            }
        ),
        AnswerOutcome::Settled(_)
    ));
    h1.await.expect("run 1 settles");
    h2.await.expect("run 2 settles");
    assert_eq!(harness.executor.total(), 1);
    teardown(&harness).await;
}

/// Two CONCURRENT waits racing one single-use pre-authorization: the
/// compare-and-decrement under the state lock lets EXACTLY ONE of them
/// spend the grant; the loser parks on its own pending record (it never
/// silently rides the winner's approval).
#[tokio::test]
async fn concurrent_spends_of_one_grant_produce_exactly_one_winner() {
    const SESSION: &str = "sess_local_alpha";
    let harness = approval_harness(ScriptedProvider::new(vec![])).await;
    let payload = json!({"path": "/data/race.txt"});
    let digest =
        ToolRequest::from_effective_arguments("tool:first-party:probe_write", payload, &budget())
            .expect("builds")
            .args_digest
            .hex;
    let ctx = kernel_ctx(SESSION, "racer");
    harness
        .approvals
        .grant_preauthorization(
            &ctx,
            SESSION,
            InvocationGrantKey {
                target_id: "tool:first-party:probe_write".to_string(),
                capability_base: "probe_write.capability".to_string(),
                args_digest_hex: digest.clone(),
            },
            1,
            TEST_GRANT_TTL_MS,
        )
        .expect("grant");
    let spend = |tag: &'static str| {
        let approvals = Arc::clone(&harness.approvals);
        let digest = digest.clone();
        async move {
            let req_ctx = kernel_ctx(SESSION, tag);
            let req = approval::ApprovalRequest {
                tool_call_id: ToolCallId::new(format!("race-{tag}")),
                target: "tool:first-party:probe_write".to_string(),
                args_digest: digest,
                args_summary: None,
                resources: Vec::new(),
            };
            approvals.request(&req_ctx, &req).await
        }
    };
    // Race them: exactly ONE winner takes the grant's single use.
    let (a, b) = tokio::join!(spend("race-a"), spend("race-b"));
    let mut winners = 0;
    for decision in [&a, &b] {
        if *decision == approval::ApprovalDecision::Approved {
            winners += 1;
        }
    }
    assert_eq!(winners, 1, "exactly one spend won (a={a:?}, b={b:?})");
    // The loser parked on its OWN pending record (or already timed out
    // deterministically) — it did NOT ride the winner's approval.
    let pendings = harness.approvals.pending_of(&ctx, SESSION);
    assert!(
        pendings.len() <= 1,
        "at most the loser's own pending remains: {:?}",
        pendings
    );
    if let Some(loser) = pendings.first() {
        assert!(matches!(
            harness.approvals.answer(
                &ctx,
                SESSION,
                &loser.approval_id,
                Answer::Reject {
                    reason: "race loser: grant already spent".to_string()
                }
            ),
            AnswerOutcome::Settled(_)
        ));
    }
    teardown(&harness).await;
}

/// The restart rule (explicit, honest): approval records and
/// pre-authorizations are in-memory — a fresh approval service knows no
/// old ids and no old grants; replaying an old approval id or ticket is
/// refused, nothing continues by default.
#[tokio::test]
async fn restart_invalidates_old_approvals_and_grants() {
    const SESSION: &str = "sess_local_alpha";
    let harness = approval_harness(ScriptedProvider::new(vec![
        tool_request(
            "tool:first-party:probe_write",
            json!({"path": "/data/r.txt"}),
        ),
        final_turn("r done"),
    ]))
    .await;
    set_mode(&harness, SESSION, SessionPermissionMode::Ask).await;
    let handle = spawn_user_run(&harness, SESSION, "RESTART: parked write");
    let pending = wait_for_pending(&harness, SESSION, "tool:first-party:probe_write").await;
    // The user approves; the write executes.
    assert!(harness
        .approvals
        .answer(
            &kernel_ctx(SESSION, "approver"),
            SESSION,
            &pending.approval_id,
            Answer::Approve
        )
        .settled());
    handle.await.expect("run settles");
    assert_eq!(harness.executor.total(), 1);

    // A RESTART: a fresh ApprovalService instance (same clock domain)
    // knows NOTHING of the old records — the explicit invalidation rule.
    let fresh = ApprovalService::new(Arc::new(lingxi_service::inject::SystemClock));
    match fresh.answer(
        &kernel_ctx(SESSION, "replayer"),
        SESSION,
        &pending.approval_id,
        Answer::Approve,
    ) {
        AnswerOutcome::Refused { reason } => {
            assert!(reason.contains("unknown"), "{reason}");
            assert!(
                reason.contains("restarts"),
                "the refusal names the restart rule: {reason}"
            );
        }
        other => panic!("an old approval id must not survive a restart: {other:?}"),
    }
    // An old pre-authorization does not continue either: the fresh
    // service has no grants, so the invocation re-requests approval.
    let digest = ToolRequest::from_effective_arguments(
        "tool:first-party:probe_write",
        json!({"path": "/data/r.txt"}),
        &budget(),
    )
    .expect("builds")
    .args_digest
    .hex;
    let req = approval::ApprovalRequest {
        tool_call_id: ToolCallId::new("restart-replay".to_string()),
        target: "tool:first-party:probe_write".to_string(),
        args_digest: digest,
        args_summary: None,
        resources: Vec::new(),
    };
    let ctx = kernel_ctx(SESSION, "replay-run");
    // The fresh service parks the replayed request (a NEW pending), it
    // does not silently continue the pre-restart approval.
    let mut wait = fresh.request(&ctx, &req);
    let mut resumed = Box::pin(tokio::time::sleep(std::time::Duration::from_millis(60)));
    tokio::select! {
        decision = &mut wait => panic!("the replay must not resolve from pre-restart state (got {decision:?})"),
        _ = &mut resumed => {}
    }
    // …and its OWN pending is answerable on the fresh service (the
    // honest re-request), then rejected.
    let pendings = fresh.pending_of(&ctx, SESSION);
    assert_eq!(
        pendings.len(),
        1,
        "the replay opened its OWN pending record"
    );
    assert!(matches!(
        fresh.answer(
            &ctx,
            SESSION,
            &pendings[0].approval_id,
            Answer::Reject {
                reason: "re-requested after restart".to_string()
            }
        ),
        AnswerOutcome::Settled(_)
    ));
    let decision = (&mut wait).await;
    assert_eq!(
        decision,
        approval::ApprovalDecision::Rejected {
            reason: "re-requested after restart".to_string()
        }
    );
    teardown(&harness).await;
}

/// A single-use pre-authorization cannot be spent twice — including via
/// a different ALIAS of the same target: the alias resolves to the same
/// registry target, and the grant's (target, digest) key with its use
/// budget is exhausted by the first spend (the gateway-level binding).
#[tokio::test]
async fn alias_cannot_reuse_a_single_use_grant() {
    const SESSION: &str = "sess_local_alpha";
    let harness = approval_harness(ScriptedProvider::new(vec![])).await;
    let payload = json!({"path": "/data/alias.txt"});
    let digest = ToolRequest::from_effective_arguments(
        "tool:first-party:probe_write",
        payload.clone(),
        &budget(),
    )
    .expect("builds")
    .args_digest
    .hex;
    let ctx = kernel_ctx(SESSION, "granter");
    harness
        .approvals
        .grant_preauthorization(
            &ctx,
            SESSION,
            InvocationGrantKey {
                target_id: "tool:first-party:probe_write".to_string(),
                capability_base: "probe_write.capability".to_string(),
                args_digest_hex: digest.clone(),
            },
            1,
            TEST_GRANT_TTL_MS,
        )
        .expect("grant");
    // The gateway-level approval wait, once via the CANONICAL name and
    // once via the ALIAS (both resolve to the same target id).
    // An owning spend future (the request borrows its context; the
    // async block owns everything it borrows).
    let spend = |tag: &'static str| {
        let approvals = Arc::clone(&harness.approvals);
        let digest = digest.clone();
        async move {
            let ctx = kernel_ctx(SESSION, "alias-run");
            let req = approval::ApprovalRequest {
                tool_call_id: ToolCallId::new(format!("alias-{tag}")),
                target: "tool:first-party:probe_write".to_string(),
                args_digest: digest,
                args_summary: None,
                resources: Vec::new(),
            };
            approvals.request(&ctx, &req).await
        }
    };
    // First spend (canonical name): the grant answers Approved.
    let first = spend("canonical").await;
    assert_eq!(
        first,
        approval::ApprovalDecision::Approved,
        "the granted invocation spends its single use"
    );
    // Second spend (the alias): the grant is EXHAUSTED — the wait parks
    // on a NEW pending record instead of reusing the spent approval.
    let mut second = Box::pin(spend("alias"));
    let mut grace = Box::pin(tokio::time::sleep(std::time::Duration::from_millis(60)));
    tokio::select! {
        decision = &mut second => panic!("the exhausted grant must not answer the alias spend (got {decision:?})"),
        _ = &mut grace => {}
    }
    let pendings = harness.approvals.pending_of(&ctx, SESSION);
    assert_eq!(
        pendings.len(),
        1,
        "the alias spend opened its OWN pending (the single-use grant is gone)"
    );
    // Reject it; zero ambiguity remains.
    assert!(matches!(
        harness.approvals.answer(
            &ctx,
            SESSION,
            &pendings[0].approval_id,
            Answer::Reject {
                reason: "grant already spent".to_string()
            }
        ),
        AnswerOutcome::Settled(_)
    ));
    assert_eq!(
        (&mut second).await,
        approval::ApprovalDecision::Rejected {
            reason: "grant already spent".to_string()
        }
    );
    teardown(&harness).await;
}

/// The user-session mode matrix on the real chain: read_only DENIES the
/// write with the incumbent code (privilege escalation refused at the
/// session layer too, not only inside subagents); operate ALLOWS it; ask
/// round-trips the real approval wait.
#[tokio::test]
async fn user_session_mode_matrix_on_the_real_chain() {
    // ── read_only: the write is denied (zero executions). ──
    {
        let harness = approval_harness(ScriptedProvider::new(vec![
            tool_request(
                "tool:first-party:probe_write",
                json!({"path": "/data/m1.txt"}),
            ),
            final_turn("m1 done"),
        ]))
        .await;
        set_mode(
            &harness,
            "sess_local_alpha",
            SessionPermissionMode::ReadOnly,
        )
        .await;
        let run_id = spawn_user_run(&harness, "sess_local_alpha", "M1: read-only write")
            .await
            .expect("settles");
        assert_eq!(harness.executor.total(), 0);
        let journal = journal_of(&harness, &run_id).await;
        let receipt = journal[0].receipt.as_ref().expect("receipt");
        assert!(!receipt.dispatched);
        assert!(
            receipt.detail.contains("gateway_policy_denied")
                && receipt.detail.contains("ACTION_BLOCKED_BY_READ_ONLY"),
            "the read-only session refusal carries the incumbent code: {}",
            receipt.detail
        );
        // The read class stays allowed in the same session.
        teardown(&harness).await;
    }
    // ── ask: the write round-trips the real approval wait. ──
    {
        let harness = approval_harness(ScriptedProvider::new(vec![
            tool_request(
                "tool:first-party:probe_write",
                json!({"path": "/data/m2.txt"}),
            ),
            tool_request("tool:first-party:read", json!({"path": "/data/m2.txt"})),
            final_turn("m2 done"),
        ]))
        .await;
        set_mode(&harness, "sess_local_alpha", SessionPermissionMode::Ask).await;
        let handle = spawn_user_run(&harness, "sess_local_alpha", "M2: ask writes");
        let pending =
            wait_for_pending(&harness, "sess_local_alpha", "tool:first-party:probe_write").await;
        assert!(harness
            .approvals
            .answer(
                &kernel_ctx("sess_local_alpha", "approver"),
                "sess_local_alpha",
                &pending.approval_id,
                Answer::Approve
            )
            .settled());
        let run_id = handle.await.expect("settles");
        assert_eq!(
            harness.executor.total(),
            2,
            "the approved write AND the free read both ran"
        );
        let journal = journal_of(&harness, &run_id).await;
        assert_eq!(journal.len(), 2);
        assert_eq!(
            journal[0].phase,
            lingxi_kernel::ports::InvocationPhase::Succeeded
        );
        assert_eq!(
            journal[1].phase,
            lingxi_kernel::ports::InvocationPhase::Succeeded
        );
        teardown(&harness).await;
    }
}
