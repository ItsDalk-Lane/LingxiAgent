//! R04-T04 acceptance: native read/write/edit file tools and resource
//! authorization (R04-A07 / R04-A08 + the adversarial additions).
//!
//! Everything here runs against the REAL composition root
//! (`ServiceState::bootstrap_with_deps`) with the REAL run driver, the
//! REAL journal, the REAL registry, the REAL gateway and the REAL
//! ApprovalService — and the three REAL native file executors
//! (`lingxi_service::filetools`) registered through
//! `register_core_file_tools`. All filesystem operations hit a REAL
//! isolated temp-dir test root with authorized roots and inside/outside
//! sentinels (no test double touches the filesystem layer).
//!
//! Test-double boundary (the R04 scope matrix `test_double_boundary`):
//! - `StepsProvider`/`MarkerScriptedProvider` (`TurnProviderPort`) —
//!   external model responses ONLY; the `UserWrites` steps play the
//!   USER (an external file mutator), never a system component.
//! - The authenticated approver is the test calling the real
//!   `ApprovalService` surface.
//! - The `MutationTestHook` is the module's documented test-only seam
//!   (fires inside the mutation window); no production logic is mocked.
//!
//! Scenarios:
//! - `r04_a07_*` — concurrent edit does not overwrite the user's change
//!   (read v1 → user writes v2 → edit-per-v1 conflicts, v2 kept,
//!   re-read + retry succeeds) on the FULL real chain.
//! - `r04_a08_*` — symlinks cannot escape the authorized roots (read/
//!   write/edit judged on the REAL target; zero side effects outside).
//! - read semantics: pagination, truncation notices, offset errors,
//!   duplicate-read stub, BOM, binary/non-UTF-8 honesty.
//! - edit semantics: exact/fuzzy match, duplicate occurrences, overlap,
//!   empty oldText, no-change, CRLF/BOM preservation, ENOENT.
//! - write semantics: create/overwrite, change-log records, stale
//!   conflicts, atomic temp+rename with zero residue.
//! - TOCTOU: a mid-mutation swap is a conflict (FILE_CHANGED_DURING_
//!   MUTATION), never a lost update.
//! - approval records bind the REAL file scope; the T03 OBS-1 alias
//!   resolution leg runs through the real gateway to the real executor.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::ToolOutcome;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolExecutionResult,
    ToolRequest, TurnProviderPort,
};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetId};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, ToolCallId};
use lingxi_service::approval_service::{Answer, ApprovalService};
use lingxi_service::filetools::{
    register_core_file_tools, CoreFileTools, FileChangeLog, FileModificationRecord, FileTools,
    FILE_CHANGED_DURING_MUTATION, FILE_STALE_SINCE_READ,
};
use lingxi_service::resourceaccess::{ResourceAccess, ResourceOp};
use lingxi_service::toolgateway::{
    InvocationPermissionContext, PreparedInvocationHandle, ToolInvocationGateway,
};
use lingxi_service::{
    approval, prepare_layout, ServiceConfig, ServiceDeps, ServiceState, LOCAL_OWNER_USER_ID,
};
use lingxi_service::{HomeSource, NetworkMode};
use serde_json::json;

const NOW_MS: u64 = 1_790_409_600_000;

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

// ── doubles (external stand-ins only) ──────────────────────────────────────

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

fn tool_request(target: &str, args: serde_json::Value) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        requests: vec![
            ToolRequest::from_effective_arguments(target, args, &budget())
                .expect("effective request"),
        ],
    }
}

/// One scripted step of a run: a model turn, or the USER writing a file
/// between turns (an external mutator — never a system component).
enum Step {
    Tool(ProviderTurn),
    UserWrites(PathBuf, Vec<u8>),
}

struct StepsProvider {
    steps: std::sync::Mutex<VecDeque<Step>>,
}

impl StepsProvider {
    fn new(steps: Vec<Step>) -> Arc<Self> {
        Arc::new(Self {
            steps: std::sync::Mutex::new(steps.into_iter().collect()),
        })
    }
}

impl TurnProviderPort for StepsProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
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
        loop {
            let next = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Step::Tool(final_turn("done")));
            match next {
                Step::Tool(turn) => {
                    let ctx_at_issue = ctx.clone();
                    return Box::pin(
                        async move { ProviderTurnResult::of_ctx(&ctx_at_issue, turn) },
                    );
                }
                Step::UserWrites(path, bytes) => {
                    std::fs::write(&path, bytes).expect("the user's external write lands");
                }
            }
        }
    }
}

/// Captures the explicit modification records (the checkpoint/rewind
/// interface) for assertions.
#[derive(Default)]
struct ChangeLogCapture {
    records: std::sync::Mutex<Vec<FileModificationRecord>>,
}

impl FileChangeLog for ChangeLogCapture {
    fn record(&self, record: FileModificationRecord) {
        self.records.lock().unwrap().push(record);
    }
}

// ── composition-root harness ───────────────────────────────────────────────

struct Harness {
    state: ServiceState,
    gateway: Arc<ToolInvocationGateway>,
    approvals: Arc<ApprovalService>,
    access: Arc<ResourceAccess>,
    core: CoreFileTools,
    changes: Arc<ChangeLogCapture>,
    root: PathBuf,
    ws: PathBuf,
    home: PathBuf,
}

async fn teardown(harness: &Harness) {
    harness
        .state
        .storage()
        .close()
        .await
        .expect("close storage");
    let _ = std::fs::remove_dir_all(&harness.home);
    let _ = std::fs::remove_dir_all(&harness.root);
}

/// The full harness: the three REAL file tools registered on the REAL
/// gateway, with the workspace as the tools' cwd and the external read
/// root granted to the kernel principal of the run.
async fn file_harness(provider: Arc<dyn TurnProviderPort>) -> Harness {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "lingxi-r04t04-root-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        seq
    ));
    let home = std::env::temp_dir().join(format!(
        "lingxi-r04t04-home-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        seq
    ));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&home);
    let ws = root.join("ws");
    let ext = root.join("ext");
    let restricted = root.join("restricted");
    std::fs::create_dir_all(&ws).expect("workspace");
    std::fs::create_dir_all(&ext).expect("external read root");
    std::fs::create_dir_all(&restricted).expect("restricted area");
    std::fs::write(restricted.join("secret.txt"), b"TOP SECRET SENTINEL").expect("sentinel");
    std::fs::write(ext.join("readable.txt"), b"external readable doc").expect("readable");

    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");
    let registry = Arc::new(ToolRegistry::new());
    let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let approvals = Arc::new(ApprovalService::new(Arc::new(
        lingxi_service::inject::SystemClock,
    )));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(lingxi_service::inject::SystemClock),
        budget(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    let changes = Arc::new(ChangeLogCapture::default());
    let core = register_core_file_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&access),
        ws.clone(),
        Arc::new(lingxi_service::inject::SystemClock),
        Arc::clone(&changes) as Arc<dyn FileChangeLog>,
        &budget(),
    );
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_gateway: Some(Arc::clone(&gateway)),
        approval_gate: Some(Arc::clone(&approvals) as Arc<dyn approval::ApprovalGate>),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    Harness {
        state,
        gateway,
        approvals,
        access,
        core,
        changes,
        root,
        ws,
        home,
    }
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
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
        web_session_id: None,
        credential_id: None,
    }
}

fn kernel_ctx(session: &str, run: &str) -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(session.to_string()),
        run_id: lingxi_protocol::RunId::new(run.to_string()),
        attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}

fn user_operate() -> InvocationPermissionContext {
    InvocationPermissionContext::UserSession {
        mode: SessionPermissionMode::Operate,
    }
}

/// Direct gateway call (prepare → execute_prepared) — the same two steps
/// the run driver performs, minus the driving loop.
async fn call_file_tool(
    h: &Harness,
    ctx: &RunContext,
    call: &str,
    target: &ToolTargetId,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, lingxi_service::toolgateway::GatewayRefusal> {
    let request = ToolRequest::from_effective_arguments(target.as_str(), args, &budget())
        .expect("effective request");
    let call_id = ToolCallId::new(call.to_string());
    // Both stages surface their refusal: a prepare-time resource/policy
    // refusal and an execute-time re-check refusal are the same
    // zero-dispatch outcome class.
    let prepared = h.gateway.prepare_from_request(
        ctx,
        lingxi_service::toolgateway::CallerSurface::UserRun,
        "agent",
        user_operate(),
        &call_id,
        &request,
    )?;
    h.gateway
        .execute_prepared(ctx, &call_id, &prepared.handle)
        .await
}

fn text_of(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn error_text(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Failed { error } => error.message.clone(),
        other => panic!("expected a failed outcome, got {other:?}"),
    }
}

/// No `.lingxi-write-*` temp residue anywhere under the test root.
fn assert_no_temp_residue(root: &Path) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read_dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if entry
                .file_name()
                .to_string_lossy()
                .starts_with(".lingxi-write-")
            {
                panic!("temp residue left behind: {path:?}");
            }
        }
    }
}

// ── R04-A07: concurrent edit does not overwrite the user's change ──────────

/// The FULL real chain: the tool reads version 1; the USER (an external
/// mutator between turns) writes version 2; the tool's edit prepared
/// against version 1 returns the FILE_STALE_SINCE_READ conflict with
/// version 2 intact; an authorized re-read registers the new
/// fingerprint and the retried edit succeeds on top of version 2.
#[tokio::test]
async fn r04_a07_concurrent_edit_never_overwrites_the_user_version() {
    let file = "notes.txt";
    let ws_file = |h: &Harness| h.ws.join(file);
    let v1 = b"alpha\nbeta\ngamma\n".to_vec();
    let v2 = b"alpha\nBETA-v2\ngamma\n".to_vec();
    let provider = StepsProvider::new(vec![
        Step::Tool(tool_request("tool:first-party:read", json!({"path": file}))),
        Step::UserWrites(PathBuf::from("placeholder"), Vec::new()), // replaced below
        Step::Tool(tool_request(
            "tool:first-party:edit",
            json!({"path": file, "edits": [{"oldText": "beta", "newText": "beta-edited"}]}),
        )),
        Step::Tool(tool_request("tool:first-party:read", json!({"path": file}))),
        Step::Tool(tool_request(
            "tool:first-party:edit",
            json!({"path": file, "edits": [{"oldText": "BETA-v2", "newText": "BETA-final"}]}),
        )),
    ]);
    let harness = file_harness(Arc::clone(&provider) as Arc<dyn TurnProviderPort>).await;
    std::fs::write(ws_file(&harness), &v1).expect("v1");
    // Rebind the user step now that the workspace exists.
    provider
        .steps
        .lock()
        .unwrap()
        .get_mut(1)
        .map(|step| {
            if let Step::UserWrites(path, bytes) = step {
                *path = ws_file(&harness);
                *bytes = v2.clone();
            }
        })
        .expect("user step present");

    let principal = owner_principal();
    harness
        .state
        .sessions()
        .set_permission_mode_for(
            &principal,
            "sess_local_alpha",
            SessionPermissionMode::Operate,
        )
        .await
        .expect("operate mode (and the session record)");
    let run_id = harness
        .state
        .sessions()
        .execute_for(
            harness.state.storage().as_ref(),
            harness.state.events(),
            harness.state.runs(),
            &principal,
            "sess_local_alpha",
            "A07: staged edit with a user change in between",
            NOW_MS,
        )
        .await
        .expect("the run drives to completion")
        .run_id;

    // Journal: read(v1) SUCCEEDED, edit-per-v1 FAILED with the stale
    // conflict AND dispatched=true (the tool RAN and refused — never a
    // silent skip), re-read SUCCEEDED, retried edit SUCCEEDED.
    let journal = harness
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 4, "{journal:?}");
    assert_eq!(
        journal[0].receipt.as_ref().expect("receipt 0").outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );
    let conflicting = journal[1].receipt.as_ref().expect("receipt 1");
    assert_eq!(
        conflicting.outcome,
        lingxi_kernel::ports::ReceiptOutcome::Failed
    );
    assert!(conflicting.dispatched, "the tool executed and refused");
    assert!(
        conflicting.detail.contains(FILE_STALE_SINCE_READ),
        "the conflict is the incumbent staleness refusal: {}",
        conflicting.detail
    );
    assert_eq!(
        journal[2].receipt.as_ref().expect("receipt 2").outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );
    assert_eq!(
        journal[3].receipt.as_ref().expect("receipt 3").outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );

    // The user's version 2 was never lost: the final file is the retried
    // edit ON TOP of v2 (BETA-v2 -> BETA-final), not beta-edited.
    let final_content = std::fs::read(ws_file(&harness)).expect("final file");
    assert_eq!(
        String::from_utf8_lossy(&final_content),
        "alpha\nBETA-final\ngamma\n"
    );
    assert_no_temp_residue(&harness.root);
    teardown(&harness).await;
}

// ── R04-A08: symlinks cannot escape the authorized roots ───────────────────

/// Workspace links pointing OUT of the authorized roots (to the
/// restricted sentinel and to the read-only external root) are judged by
/// their REAL targets: the restricted leak is refused for read/write/
/// edit with zero side effects; the read-granted external document
/// reads fine but never writes; an in-workspace link operates on its
/// real target.
#[tokio::test]
async fn r04_a08_symlinks_are_judged_by_their_real_targets() {
    let harness = file_harness(StepsProvider::new(vec![])).await;
    // Links INSIDE the workspace pointing to real targets outside/inside.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../restricted/secret.txt", harness.ws.join("leak.txt"))
            .expect("leak link");
        std::os::unix::fs::symlink("../ext/readable.txt", harness.ws.join("doc-link.txt"))
            .expect("doc link");
        std::fs::write(harness.ws.join("real-inner.txt"), b"inner doc").expect("inner");
        std::os::unix::fs::symlink("real-inner.txt", harness.ws.join("inner-link.txt"))
            .expect("inner link");
    }
    let ctx = kernel_ctx("sess-a08", "run-a08");
    // Grant the external root as READ to exactly this principal+session.
    harness
        .access
        .grant_root(
            ctx.principal.storage_kind(),
            &ctx.principal.storage_subject(),
            &ctx.session_id.to_string(),
            &harness.root.join("ext"),
            ResourceOp::Read,
        )
        .expect("read grant");

    // 1) The restricted leak: read/write/edit ALL refused at the
    //    resource boundary (prepare-time, zero dispatch).
    for (call, target, args) in [
        (
            "read-leak",
            harness.core.read_target.clone(),
            json!({"path": "leak.txt"}),
        ),
        (
            "write-leak",
            harness.core.write_target.clone(),
            json!({"path": "leak.txt", "content": "EXFILTRATED"}),
        ),
        (
            "edit-leak",
            harness.core.edit_target.clone(),
            json!({"path": "leak.txt", "edits": [{"oldText": "SENTINEL", "newText": "PWNED"}]}),
        ),
    ] {
        match call_file_tool(&harness, &ctx, call, &target, args).await {
            Err(refusal) => {
                assert_eq!(
                    refusal.code(),
                    "gateway_resource_scope_denied",
                    "{call}: {refusal}"
                );
                assert!(
                    refusal.to_tool_error().message.contains("authorized root"),
                    "{call}: {}",
                    refusal.to_tool_error().message
                );
            }
            Ok(result) => panic!("{call} must be refused, got {result:?}"),
        }
    }
    // Zero side effects: the sentinel is byte-identical, no new files in
    // the restricted area, no temp residue.
    let sentinel = std::fs::read(harness.root.join("restricted/secret.txt")).expect("sentinel");
    assert_eq!(sentinel, b"TOP SECRET SENTINEL");
    assert_eq!(
        std::fs::read_dir(harness.root.join("restricted"))
            .expect("restricted dir")
            .count(),
        1,
        "the restricted area gained no files"
    );
    assert_no_temp_residue(&harness.root);

    // 2) The read-granted external document: readable through the link
    //    (its REAL target is inside the granted root)…
    let read = call_file_tool(
        &harness,
        &ctx,
        "read-doc-link",
        &harness.core.read_target,
        json!({"path": "doc-link.txt"}),
    )
    .await
    .expect("the real target is read-authorized");
    assert!(
        text_of(&read).contains("external readable doc"),
        "{}",
        text_of(&read)
    );
    // …but a write through it is outside the write scope.
    match call_file_tool(
        &harness,
        &ctx,
        "write-doc-link",
        &harness.core.write_target,
        json!({"path": "doc-link.txt", "content": "overwritten"}),
    )
    .await
    {
        Err(refusal) => {
            assert_eq!(refusal.code(), "gateway_resource_scope_denied");
            assert!(
                refusal
                    .to_tool_error()
                    .message
                    .contains("authorized for reads only"),
                "{}",
                refusal.to_tool_error().message
            );
        }
        Ok(result) => panic!("write through the read-granted link must be refused: {result:?}"),
    }
    assert_eq!(
        std::fs::read(harness.root.join("ext/readable.txt")).expect("doc"),
        b"external readable doc"
    );

    // 3) An in-workspace link operates on its REAL target.
    let read = call_file_tool(
        &harness,
        &ctx,
        "read-inner-link",
        &harness.core.read_target,
        json!({"path": "inner-link.txt"}),
    )
    .await
    .expect("the real target is inside the workspace");
    assert!(text_of(&read).contains("inner doc"));
    let write = call_file_tool(
        &harness,
        &ctx,
        "write-inner-link",
        &harness.core.write_target,
        json!({"path": "inner-link.txt", "content": "written via link"}),
    )
    .await
    .expect("in-workspace link writes its real target");
    assert!(text_of(&write).contains("Successfully wrote"));
    assert_eq!(
        std::fs::read(harness.ws.join("real-inner.txt")).expect("real target"),
        b"written via link"
    );
    // The link itself is untouched (still a link to the same file).
    assert!(std::fs::symlink_metadata(harness.ws.join("inner-link.txt"))
        .expect("link")
        .file_type()
        .is_symlink());
    teardown(&harness).await;
}

/// The same refusal on the FULL run chain (the driver journals a
/// never-dispatched failure for the resource refusal).
#[tokio::test]
async fn r04_a08_resource_refusal_on_the_full_chain_is_never_dispatched() {
    let harness = file_harness(StepsProvider::new(vec![
        Step::Tool(tool_request(
            "tool:first-party:read",
            json!({"path": "leak.txt"}),
        )),
        Step::Tool(final_turn("done")),
    ]))
    .await;
    #[cfg(unix)]
    std::os::unix::fs::symlink("../restricted/secret.txt", harness.ws.join("leak.txt"))
        .expect("leak link");
    let principal = owner_principal();
    harness
        .state
        .sessions()
        .set_permission_mode_for(
            &principal,
            "sess_local_alpha",
            SessionPermissionMode::Operate,
        )
        .await
        .expect("operate mode (and the session record)");
    let run_id = harness
        .state
        .sessions()
        .execute_for(
            harness.state.storage().as_ref(),
            harness.state.events(),
            harness.state.runs(),
            &principal,
            "sess_local_alpha",
            "A08: read through the leak link",
            NOW_MS,
        )
        .await
        .expect("the run completes (the refusal is a tool failure)")
        .run_id;
    let journal = harness
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1);
    let receipt = journal[0].receipt.as_ref().expect("receipt");
    assert_eq!(
        receipt.outcome,
        lingxi_kernel::ports::ReceiptOutcome::Failed
    );
    assert!(!receipt.dispatched, "a resource refusal never dispatches");
    assert!(
        receipt.detail.contains("gateway_resource_scope_denied"),
        "{}",
        receipt.detail
    );
    assert_eq!(
        std::fs::read(harness.root.join("restricted/secret.txt")).expect("sentinel"),
        b"TOP SECRET SENTINEL"
    );
    teardown(&harness).await;
}

// ── adversarial additions ───────────────────────────────────────────────────

/// String-prefix siblings, parent-directory switches through symlinks,
/// and the legitimate contrasts (the refusal posture is not "reject
/// everything": paths inside the roots work).
#[tokio::test]
async fn adversarial_prefix_parent_switch_and_unicode_names() {
    let harness = file_harness(StepsProvider::new(vec![])).await;
    let ctx = kernel_ctx("sess-adv", "run-adv");
    // 1) A sibling whose name SHARES THE PREFIX of the workspace.
    let sibling = harness.root.join("ws-secret");
    std::fs::create_dir_all(&sibling).expect("sibling");
    std::fs::write(sibling.join("escape.txt"), b"nope").expect("escape");
    match call_file_tool(
        &harness,
        &ctx,
        "write-prefix-sibling",
        &harness.core.write_target,
        json!({"path": "../ws-secret/escape.txt", "content": "escaped"}),
    )
    .await
    {
        Err(refusal) => assert_eq!(refusal.code(), "gateway_resource_scope_denied"),
        Ok(result) => panic!("prefix sibling must be refused: {result:?}"),
    }
    assert_eq!(
        std::fs::read(sibling.join("escape.txt")).expect("untouched"),
        b"nope"
    );
    // The sibling directory must not have gained anything and the
    // workspace escape must not have created parents outside.
    assert!(!harness.ws.join("../ws-secret/deep").exists());

    // 2) A parent-directory switch: a directory INSIDE the workspace
    //    that is a symlink to the restricted area.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../restricted", harness.ws.join("switchdir"))
            .expect("switch dir");
        match call_file_tool(
            &harness,
            &ctx,
            "read-parent-switch",
            &harness.core.read_target,
            json!({"path": "switchdir/secret.txt"}),
        )
        .await
        {
            Err(refusal) => {
                assert_eq!(refusal.code(), "gateway_resource_scope_denied");
                assert!(
                    refusal.to_tool_error().message.contains("authorized root"),
                    "{}",
                    refusal.to_tool_error().message
                );
            }
            Ok(result) => panic!("parent switch must be refused: {result:?}"),
        }
    }

    // 3) Spaces / Chinese / Unicode / emoji filenames round-trip.
    let unicode_name = "文件 名 称 📄 with spaces.txt";
    let write = call_file_tool(
        &harness,
        &ctx,
        "write-unicode",
        &harness.core.write_target,
        json!({"path": unicode_name, "content": "你好 world 🌍"}),
    )
    .await
    .expect("unicode filename writes");
    assert!(text_of(&write).contains("Successfully wrote"));
    let read = call_file_tool(
        &harness,
        &ctx,
        "read-unicode",
        &harness.core.read_target,
        json!({"path": unicode_name}),
    )
    .await
    .expect("unicode filename reads");
    assert_eq!(text_of(&read), "你好 world 🌍");
    let edit = call_file_tool(
        &harness,
        &ctx,
        "edit-unicode",
        &harness.core.edit_target,
        json!({"path": unicode_name, "edits": [{"oldText": "你好", "newText": "世界"}]}),
    )
    .await
    .expect("unicode filename edits");
    assert!(
        text_of(&edit).contains("Successfully replaced 1 block(s)"),
        "{}",
        text_of(&edit)
    );
    assert_eq!(
        std::fs::read_to_string(harness.ws.join(unicode_name)).expect("edited"),
        "世界 world 🌍"
    );
    assert_no_temp_residue(&harness.root);
    teardown(&harness).await;
}

/// CRLF files, long tails, duplicate matches, invalid inputs with
/// legitimate contrasts — the incumbent read/edit error vocabulary on
/// real files.
#[tokio::test]
async fn adversarial_read_pagination_truncation_and_edit_vocabulary() {
    let harness = file_harness(StepsProvider::new(vec![])).await;
    let ctx = kernel_ctx("sess-sem", "run-sem");

    // read: limit + the remaining-lines notice; the contrast without a
    // limit returns everything.
    std::fs::write(
        harness.ws.join("ten.txt"),
        "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\n",
    )
    .expect("ten");
    let limited = call_file_tool(
        &harness,
        &ctx,
        "read-limit",
        &harness.core.read_target,
        json!({"path": "ten.txt", "offset": 2, "limit": 3}),
    )
    .await
    .expect("limited read");
    assert_eq!(
        text_of(&limited),
        "l2\nl3\nl4\n\n[7 more lines in file. Use offset=5 to continue.]"
    );
    let full = call_file_tool(
        &harness,
        &ctx,
        "read-full",
        &harness.core.read_target,
        json!({"path": "ten.txt"}),
    )
    .await
    .expect("full read");
    assert!(text_of(&full).starts_with("l1\nl2\n"));
    assert!(!text_of(&full).contains("more lines"));

    // read: offset past EOF is the incumbent's explicit error (with the
    // valid-offset contrast above).
    match call_file_tool(
        &harness,
        &ctx,
        "read-past-eof",
        &harness.core.read_target,
        json!({"path": "ten.txt", "offset": 12}),
    )
    .await
    {
        Ok(result) => assert!(
            error_text(&result).contains("Offset 12 is beyond end of file (11 lines total)"),
            "{}",
            error_text(&result)
        ),
        Err(refusal) => panic!("offset past EOF is a tool error, not a refusal: {refusal}"),
    }

    // read: truncation by lines (2500 lines) with the continuation
    // notice, and offset continuation.
    let long: String = (1..=2500)
        .map(|n| format!("line-{n}\n"))
        .collect::<String>();
    std::fs::write(harness.ws.join("long.txt"), &long).expect("long");
    let truncated = call_file_tool(
        &harness,
        &ctx,
        "read-long",
        &harness.core.read_target,
        json!({"path": "long.txt"}),
    )
    .await
    .expect("long read");
    assert!(
        text_of(&truncated)
            .ends_with("[Showing lines 1-2000 of 2501. Use offset=2001 to continue.]"),
        "tail: {}",
        text_of(&truncated)
            .chars()
            .rev()
            .take(120)
            .collect::<String>()
    );
    match &truncated.outcome {
        ToolOutcome::Success { result } => assert!(result.truncated, "the flag is set"),
        other => panic!("{other:?}"),
    }
    let continuation = call_file_tool(
        &harness,
        &ctx,
        "read-long-cont",
        &harness.core.read_target,
        json!({"path": "long.txt", "offset": 2001}),
    )
    .await
    .expect("continuation read");
    assert!(
        text_of(&continuation).starts_with("line-2001"),
        "{}",
        text_of(&continuation).chars().take(60).collect::<String>()
    );
    match &continuation.outcome {
        ToolOutcome::Success { result } => assert!(!result.truncated),
        other => panic!("{other:?}"),
    }

    // read: a single line beyond the byte limit gets the honest
    // single-line notice (adapted from the incumbent's bash fallback).
    let huge_line = "x".repeat(60 * 1024);
    std::fs::write(harness.ws.join("wide.txt"), &huge_line).expect("wide");
    let wide = call_file_tool(
        &harness,
        &ctx,
        "read-wide",
        &harness.core.read_target,
        json!({"path": "wide.txt"}),
    )
    .await
    .expect("wide read");
    assert!(
        text_of(&wide).starts_with("[Line 1 is 60.0KB, exceeds 50.0KB limit"),
        "{}",
        text_of(&wide).chars().take(80).collect::<String>()
    );

    // read: duplicate ADJACENT read returns the incumbent stub (the
    // registry remembers only the LAST read — the plain read immediately
    // before the identical one is what deduplicates).
    let _plain = call_file_tool(
        &harness,
        &ctx,
        "read-plain-before-dup",
        &harness.core.read_target,
        json!({"path": "ten.txt"}),
    )
    .await
    .expect("plain read");
    let dup = call_file_tool(
        &harness,
        &ctx,
        "read-dup",
        &harness.core.read_target,
        json!({"path": "ten.txt"}),
    )
    .await
    .expect("duplicate read");
    assert!(
        text_of(&dup).starts_with("[Duplicate read: ten.txt is unchanged"),
        "{}",
        text_of(&dup)
    );

    // read: BOM stripped; binary/non-UTF-8 reported honestly.
    std::fs::write(harness.ws.join("bom.txt"), b"\xEF\xBB\xBFbom content").expect("bom");
    let bom_read = call_file_tool(
        &harness,
        &ctx,
        "read-bom",
        &harness.core.read_target,
        json!({"path": "bom.txt"}),
    )
    .await
    .expect("bom read");
    assert_eq!(text_of(&bom_read), "bom content");
    std::fs::write(harness.ws.join("blob.bin"), [0x00, 0xFF, 0xFE, 0x01, 0x02]).expect("blob");
    let binary = call_file_tool(
        &harness,
        &ctx,
        "read-binary",
        &harness.core.read_target,
        json!({"path": "blob.bin"}),
    )
    .await
    .expect("binary read");
    assert!(
        text_of(&binary).contains("5 bytes of binary or non-UTF-8 content"),
        "{}",
        text_of(&binary)
    );

    // edit: duplicate occurrences refuse with the incumbent text and
    // leave the file untouched; the UNIQUE contrast succeeds.
    std::fs::write(harness.ws.join("dup.txt"), "same same different same\n").expect("dup");
    let duplicate = call_file_tool(
        &harness,
        &ctx,
        "edit-duplicate",
        &harness.core.edit_target,
        json!({"path": "dup.txt", "edits": [{"oldText": "same", "newText": "other"}]}),
    )
    .await
    .expect("duplicate refusal is a tool error");
    assert!(
        error_text(&duplicate).contains("Found 3 occurrences of the text in dup.txt"),
        "{}",
        error_text(&duplicate)
    );
    assert_eq!(
        std::fs::read_to_string(harness.ws.join("dup.txt")).expect("untouched"),
        "same same different same\n"
    );
    let unique = call_file_tool(
        &harness,
        &ctx,
        "edit-unique",
        &harness.core.edit_target,
        json!({"path": "dup.txt", "edits": [{"oldText": "different", "newText": "DISTINCT"}]}),
    )
    .await
    .expect("unique edit succeeds");
    assert!(text_of(&unique).contains("Successfully replaced 1 block(s)"));

    // edit: not-found / empty oldText / overlap / no-change, each with
    // the file untouched.
    std::fs::write(harness.ws.join("vocab.txt"), "abcdef\n").expect("vocab");
    let not_found = call_file_tool(
        &harness,
        &ctx,
        "edit-not-found",
        &harness.core.edit_target,
        json!({"path": "vocab.txt", "edits": [{"oldText": "zzz", "newText": "y"}]}),
    )
    .await
    .expect("not found is a tool error");
    assert!(
        error_text(&not_found).contains(
            "Could not find the exact text in vocab.txt. The old text must match exactly"
        ),
        "{}",
        error_text(&not_found)
    );
    let empty = call_file_tool(
        &harness,
        &ctx,
        "edit-empty",
        &harness.core.edit_target,
        json!({"path": "vocab.txt", "edits": [{"oldText": "", "newText": "y"}]}),
    )
    .await;
    match empty {
        Ok(result) => assert!(
            error_text(&result).contains("oldText must not be empty in vocab.txt"),
            "{}",
            error_text(&result)
        ),
        Err(refusal) => panic!("empty oldText is a tool error, not a refusal: {refusal}"),
    }
    let overlap = call_file_tool(
        &harness,
        &ctx,
        "edit-overlap",
        &harness.core.edit_target,
        json!({"path": "vocab.txt", "edits": [
            {"oldText": "abcde", "newText": "X"},
            {"oldText": "cdef", "newText": "Y"},
        ]}),
    )
    .await
    .expect("overlap is a tool error");
    assert!(
        error_text(&overlap).contains("edits[0] and edits[1] overlap in vocab.txt"),
        "{}",
        error_text(&overlap)
    );
    let no_change = call_file_tool(
        &harness,
        &ctx,
        "edit-nochange",
        &harness.core.edit_target,
        json!({"path": "vocab.txt", "edits": [{"oldText": "abc", "newText": "abc"}]}),
    )
    .await
    .expect("no-change is a tool error");
    assert!(
        error_text(&no_change).contains("No changes made to vocab.txt"),
        "{}",
        error_text(&no_change)
    );
    assert_eq!(
        std::fs::read_to_string(harness.ws.join("vocab.txt")).expect("untouched"),
        "abcdef\n"
    );

    // edit: missing file keeps the incumbent ENOENT phrasing.
    let missing = call_file_tool(
        &harness,
        &ctx,
        "edit-missing",
        &harness.core.edit_target,
        json!({"path": "nope.txt", "edits": [{"oldText": "a", "newText": "b"}]}),
    )
    .await
    .expect("missing file is a tool error");
    assert!(
        error_text(&missing).contains("Could not edit file: nope.txt. Error code: ENOENT."),
        "{}",
        error_text(&missing)
    );

    // edit: CRLF preserved end-to-end; the fuzzy layer matches smart
    // quotes; a BOM survives an edit.
    std::fs::write(harness.ws.join("crlf.txt"), b"a\r\nb\r\nc\r\n").expect("crlf");
    let crlf = call_file_tool(
        &harness,
        &ctx,
        "edit-crlf",
        &harness.core.edit_target,
        json!({"path": "crlf.txt", "edits": [{"oldText": "b", "newText": "B"}]}),
    )
    .await
    .expect("crlf edit");
    assert!(text_of(&crlf).contains("Successfully replaced 1 block(s)"));
    assert_eq!(
        std::fs::read(harness.ws.join("crlf.txt")).expect("endings preserved"),
        b"a\r\nB\r\nc\r\n"
    );
    std::fs::write(harness.ws.join("smart.txt"), "it’s “quoted” — dash\n").expect("smart");
    let fuzzy = call_file_tool(
        &harness,
        &ctx,
        "edit-fuzzy",
        &harness.core.edit_target,
        json!({"path": "smart.txt", "edits": [{"oldText": "it's \"quoted\" - dash", "newText": "plain ascii"}]}),
    )
    .await
    .expect("fuzzy edit matches the typographic variants");
    assert!(text_of(&fuzzy).contains("Successfully replaced 1 block(s)"));
    assert_eq!(
        std::fs::read_to_string(harness.ws.join("smart.txt")).expect("replaced"),
        "plain ascii\n"
    );
    std::fs::write(harness.ws.join("bomedit.txt"), b"\xEF\xBB\xBFkeep me\n").expect("bom edit");
    let bom_edit = call_file_tool(
        &harness,
        &ctx,
        "edit-bom",
        &harness.core.edit_target,
        json!({"path": "bomedit.txt", "edits": [{"oldText": "keep", "newText": "KEEP"}]}),
    )
    .await
    .expect("bom edit");
    assert!(text_of(&bom_edit).contains("Successfully replaced 1 block(s)"));
    assert_eq!(
        std::fs::read(harness.ws.join("bomedit.txt")).expect("bom preserved"),
        b"\xEF\xBB\xBFKEEP me\n"
    );

    // edit: multiple disjoint edits in one call (the incumbent contract).
    std::fs::write(harness.ws.join("multi.txt"), "one\ntwo\nthree\n").expect("multi");
    let multi = call_file_tool(
        &harness,
        &ctx,
        "edit-multi",
        &harness.core.edit_target,
        json!({"path": "multi.txt", "edits": [
            {"oldText": "one", "newText": "ONE"},
            {"oldText": "three", "newText": "THREE"},
        ]}),
    )
    .await
    .expect("multi edit");
    assert!(
        text_of(&multi).contains("Successfully replaced 2 block(s) in multi.txt"),
        "{}",
        text_of(&multi)
    );
    assert_eq!(
        std::fs::read_to_string(harness.ws.join("multi.txt")).expect("multi"),
        "ONE\ntwo\nTHREE\n"
    );

    // Schema-level invalid input with the legitimate contrast: unknown
    // keys are refused at the boundary, valid calls execute.
    match call_file_tool(
        &harness,
        &ctx,
        "read-unknown-key",
        &harness.core.read_target,
        json!({"path": "ten.txt", "surprise": true}),
    )
    .await
    {
        Err(refusal) => assert_eq!(refusal.code(), "gateway_arguments_invalid"),
        Ok(result) => panic!("unknown keys must be refused: {result:?}"),
    }
    assert!(call_file_tool(
        &harness,
        &ctx,
        "read-valid-contrast",
        &harness.core.read_target,
        json!({"path": "ten.txt"}),
    )
    .await
    .is_ok());
    assert_no_temp_residue(&harness.root);
    teardown(&harness).await;
}

/// Write semantics: parent creation, explicit modification records (the
/// checkpoint/rewind interface), resource refs with verified existence,
/// the stale conflict, and zero temp residue — with a mid-test conflict
/// leaving the user's content intact.
#[tokio::test]
async fn adversarial_write_records_conflicts_and_atomicity() {
    let harness = file_harness(StepsProvider::new(vec![])).await;
    let ctx = kernel_ctx("sess-write", "run-write");

    // New nested file: parents created, record Created with before=None,
    // a resource ref whose existence and digest are real.
    let created = call_file_tool(
        &harness,
        &ctx,
        "write-new",
        &harness.core.write_target,
        json!({"path": "deep/nested/new.txt", "content": "created"}),
    )
    .await
    .expect("nested create");
    assert_eq!(
        text_of(&created),
        "Successfully wrote to deep/nested/new.txt"
    );
    let records = harness.changes.records.lock().unwrap().clone();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].operation.wire_name(), "created");
    assert!(records[0].before.is_none());
    assert_eq!(records[0].after.size_bytes, 7);
    assert!(records[0].after.sha256_hex.is_some());
    match &created.outcome {
        ToolOutcome::Success { result } => {
            assert_eq!(result.resource_refs.len(), 1);
            let reference = &result.resource_refs[0];
            assert_eq!(reference.size_bytes, Some(7));
            let digest = reference.digest.as_ref().expect("digest");
            assert_eq!(digest.algorithm, "sha256");
            // The referenced file REALLY exists with that content.
            let uri = reference.uri.as_deref().expect("uri");
            let path = uri.strip_prefix("file://").expect("file uri");
            assert!(Path::new(path).is_file());
        }
        other => panic!("{other:?}"),
    }

    // Overwrite after a read: Modified with the before-version digest.
    let _read = call_file_tool(
        &harness,
        &ctx,
        "read-before-overwrite",
        &harness.core.read_target,
        json!({"path": "deep/nested/new.txt"}),
    )
    .await
    .expect("read");
    let modified = call_file_tool(
        &harness,
        &ctx,
        "write-overwrite",
        &harness.core.write_target,
        json!({"path": "deep/nested/new.txt", "content": "modified!"}),
    )
    .await
    .expect("overwrite");
    assert!(text_of(&modified).contains("Successfully wrote to"));
    let records = harness.changes.records.lock().unwrap().clone();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].operation.wire_name(), "modified");
    let before = records[1].before.as_ref().expect("before version");
    assert_eq!(
        before.sha256_hex.as_deref(),
        records[0].after.sha256_hex.as_deref()
    );
    assert_eq!(
        std::fs::read_to_string(harness.ws.join("deep/nested/new.txt")).expect("content"),
        "modified!"
    );

    // The stale conflict on write: read v1, the user writes v2, the
    // tool's write refuses and v2 stays.
    std::fs::write(harness.ws.join("stale.txt"), "v1").expect("v1");
    let _read = call_file_tool(
        &harness,
        &ctx,
        "read-stale",
        &harness.core.read_target,
        json!({"path": "stale.txt"}),
    )
    .await
    .expect("read v1");
    std::fs::write(harness.ws.join("stale.txt"), "USER VERSION 2").expect("user v2");
    let conflicted = call_file_tool(
        &harness,
        &ctx,
        "write-stale",
        &harness.core.write_target,
        json!({"path": "stale.txt", "content": "tool overwrite"}),
    )
    .await
    .expect("the stale conflict is a tool error");
    assert!(
        error_text(&conflicted).contains(FILE_STALE_SINCE_READ),
        "{}",
        error_text(&conflicted)
    );
    assert!(
        error_text(&conflicted).contains("Re-read the file, then retry this change"),
        "{}",
        error_text(&conflicted)
    );
    assert_eq!(
        std::fs::read_to_string(harness.ws.join("stale.txt")).expect("user content kept"),
        "USER VERSION 2"
    );
    // No change record for the refused write.
    let records = harness.changes.records.lock().unwrap().clone();
    assert_eq!(records.len(), 2, "failed writes record nothing");

    assert_no_temp_residue(&harness.root);
    teardown(&harness).await;
}

/// The TOCTOU guard: a swap landing INSIDE the mutation window (after
/// the read, before the atomic rename) is a conflict, never a lost
/// update — exercised deterministically through the module's documented
/// test-only seam.
#[tokio::test]
async fn adversarial_mid_mutation_swap_is_a_conflict_not_a_lost_update() {
    let harness = file_harness(StepsProvider::new(vec![])).await;
    let file = harness.ws.join("swap.txt");
    std::fs::write(&file, "original\n").expect("original");
    let ctx = kernel_ctx("sess-toctou", "run-toctou");
    // The seam fires after the temp write, before the identity
    // re-verification: an external writer REPLACES the file (new inode,
    // new content) exactly in the window.
    harness
        .core
        .tools
        .set_mutation_test_hook(Some(Arc::new(move |path: &Path| {
            let temp = path.with_extension("swapper");
            std::fs::write(&temp, "SWAPPED BY EXTERNAL WRITER\n").expect("swap content");
            std::fs::rename(&temp, path).expect("atomic external replace");
        })));
    let conflicted = call_file_tool(
        &harness,
        &ctx,
        "edit-swapped",
        &harness.core.edit_target,
        json!({"path": "swap.txt", "edits": [{"oldText": "original", "newText": "EDITED"}]}),
    )
    .await
    .expect("the mid-mutation swap is a tool-level conflict");
    assert!(
        error_text(&conflicted).contains(FILE_CHANGED_DURING_MUTATION),
        "{}",
        error_text(&conflicted)
    );
    assert!(
        error_text(&conflicted).contains("Re-read the file and retry"),
        "{}",
        error_text(&conflicted)
    );
    // The EXTERNAL writer's content survives; the tool's edit did NOT
    // land and no temp residue remains.
    assert_eq!(
        std::fs::read_to_string(&file).expect("external content kept"),
        "SWAPPED BY EXTERNAL WRITER\n"
    );
    assert_no_temp_residue(&harness.root);
    // The same guard on the write path.
    let conflicted = call_file_tool(
        &harness,
        &ctx,
        "write-swapped",
        &harness.core.write_target,
        json!({"path": "swap.txt", "content": "tool write"}),
    )
    .await
    .expect("the write-path swap conflicts too");
    assert!(error_text(&conflicted).contains(FILE_CHANGED_DURING_MUTATION));
    assert_eq!(
        std::fs::read_to_string(&file).expect("external content kept"),
        "SWAPPED BY EXTERNAL WRITER\n"
    );
    // Nothing was recorded as a successful product.
    assert!(harness.changes.records.lock().unwrap().is_empty());
    assert_no_temp_residue(&harness.root);
    teardown(&harness).await;
}

/// The approval record binds the REAL file scope (the approver sees
/// WHICH canonical file), and the T03 OBS-1 leg: an invocation that
/// genuinely resolves through an ALIAS reaches the real executor
/// through the real gateway.
#[tokio::test]
async fn approval_record_binds_real_file_scope_and_alias_resolves_to_the_executor() {
    // ask mode: the write parks for a human decision.
    let harness = file_harness(StepsProvider::new(vec![
        Step::Tool(tool_request(
            "tool:first-party:write",
            json!({"path": "approved-target.txt", "content": "approved content"}),
        )),
        Step::Tool(final_turn("done")),
    ]))
    .await;
    let session = "sess_local_alpha";
    harness
        .state
        .sessions()
        .set_permission_mode_for(&owner_principal(), session, SessionPermissionMode::Ask)
        .await
        .expect("ask mode");
    let run_handle = {
        let state = harness.state.clone();
        let principal = owner_principal();
        let session = session.to_string();
        tokio::spawn(async move {
            state
                .sessions()
                .execute_for(
                    state.storage().as_ref(),
                    state.events(),
                    state.runs(),
                    &principal,
                    &session,
                    "APPROVAL: write the approved target",
                    NOW_MS,
                )
                .await
                .expect("run drives")
                .run_id
        })
    };
    // Wait for the pending approval and assert it binds the REAL scope.
    let probe_ctx = kernel_ctx(session, "pending-probe");
    let pending = {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let pendings = harness.approvals.pending_of(&probe_ctx, session);
            if let Some(view) = pendings
                .iter()
                .find(|v| v.target == "tool:first-party:write")
            {
                break view.clone();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the write never parked for approval"
            );
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        }
    };
    assert_eq!(pending.resources.len(), 1, "{pending:?}");
    assert_eq!(pending.resources[0].op, ResourceOp::Write);
    assert!(
        pending.resources[0].path.ends_with("approved-target.txt"),
        "the approver sees the REAL canonical target: {:?}",
        pending.resources[0].path
    );
    let canonical_ws = std::fs::canonicalize(&harness.ws).expect("canonical ws");
    assert!(
        pending.resources[0].path.starts_with(&canonical_ws),
        "the canonical target is inside the workspace: {:?} (workspace {canonical_ws:?})",
        pending.resources[0].path
    );
    // Approve: the write executes on exactly that scope.
    assert!(harness
        .approvals
        .answer(&probe_ctx, session, &pending.approval_id, Answer::Approve)
        .settled());
    let run_id = run_handle.await.expect("run completes");
    assert_eq!(
        std::fs::read_to_string(harness.ws.join("approved-target.txt"))
            .expect("the approved write landed"),
        "approved content"
    );
    let journal = harness
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1);
    assert_eq!(
        journal[0].receipt.as_ref().expect("receipt").outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );

    // ── the T03 OBS-1 leg: REAL alias resolution through the gateway ──
    // A probe target WITH AN ALIAS, bound to the REAL read executor.
    // The invocation goes through ToolTargetRef::ByName(alias) — the
    // alias resolution leg that never executed in the T03 suite.
    let probe = strict_manifest("file_probe", vec!["file_probe_alias".to_string()]);
    let probe_target = harness
        .gateway
        .registry()
        .register(probe, &budget())
        .expect("probe registers")
        .target_id;
    harness.gateway.bind_executor_with_resources(
        probe_target.clone(),
        Arc::new(lingxi_service::filetools::CoreFileExecutor::new(
            Arc::clone(&harness.core.tools),
            lingxi_service::filetools::FileToolKind::Read,
        )),
        Some(FileTools::resource_extractor(
            Arc::clone(&harness.access),
            harness.ws.clone(),
            ResourceOp::Read,
        )),
        "OBS-1 alias-resolution leg bound to the REAL read executor",
    );
    std::fs::write(harness.ws.join("alias-target.txt"), "alias read content").expect("target");
    let ctx = kernel_ctx(session, "run-alias");
    let call = ToolCallId::new("alias-call".to_string());
    let prepared = harness
        .gateway
        .prepare(
            lingxi_service::toolgateway::InvocationRequest::from_trusted_entry(
                &ctx,
                lingxi_service::toolgateway::CallerSurface::UserRun,
                "agent",
                user_operate(),
                lingxi_kernel::toolcatalog::ToolTargetRef::ByName {
                    name: "file_probe_alias".to_string(),
                },
                None,
                json!({"path": "alias-target.txt"}),
                call.clone(),
            ),
        )
        .expect("the ALIAS prepares through the real gateway");
    assert_eq!(prepared.target_id, probe_target);
    assert_eq!(prepared.resources.len(), 1);
    assert!(
        prepared.resources[0].path.ends_with("alias-target.txt"),
        "{:?}",
        prepared.resources[0].path
    );
    let executed = harness
        .gateway
        .execute_prepared(&ctx, &call, &prepared.handle)
        .await
        .expect("the alias leg reaches the REAL executor");
    assert!(
        text_of(&executed).contains("alias read content"),
        "{}",
        text_of(&executed)
    );
    // The alias resolves to the SAME identity as the canonical name:
    // one spend of the alias handle, and the canonical name prepares a
    // DIFFERENT handle to the SAME target.
    let canonical = harness
        .gateway
        .prepare(
            lingxi_service::toolgateway::InvocationRequest::from_trusted_entry(
                &ctx,
                lingxi_service::toolgateway::CallerSurface::UserRun,
                "agent",
                user_operate(),
                lingxi_kernel::toolcatalog::ToolTargetRef::ByName {
                    name: "file_probe".to_string(),
                },
                None,
                json!({"path": "alias-target.txt"}),
                ToolCallId::new("canonical-call".to_string()),
            ),
        )
        .expect("canonical name prepares");
    assert_eq!(canonical.target_id, probe_target);
    // A foreign handle presentation cannot burn the canonical handle
    // (single-use binding discipline holds on the alias surface too).
    let forged = PreparedInvocationHandle::parse("prep:not-a-real-nonce");
    assert!(harness
        .gateway
        .execute_prepared(
            &ctx,
            &ToolCallId::new("canonical-call".to_string()),
            &forged
        )
        .await
        .is_err());
    teardown(&harness).await;
}

fn strict_manifest(name: &str, aliases: Vec<String>) -> lingxi_kernel::toolcatalog::ToolManifest {
    use lingxi_kernel::invocation::ToolRecoveryCapability;
    use lingxi_kernel::toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, ToolManifest, ToolOrigin,
    };
    use lingxi_protocol::ToolSchemaDocument;
    let _ = (name, &aliases);
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: name.to_string(),
        display_name: name.to_string(),
        aliases,
        version: "1.0.0".to_string(),
        description: "alias probe manifest".to_string(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string", "minLength": 1}},
                "required": ["path"],
                "additionalProperties": false,
            }),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: lingxi_kernel::toolcatalog::PermissionKind::Execute,
            capability_base: format!("{name}.capability"),
        },
        availability: Availability::Available,
        timeout_ms: Some(30_000),
        max_concurrency: Some(4),
        declared_permission: DeclaredPermission::None,
        recovery: ToolRecoveryCapability::CONSERVATIVE,
    }
}

/// Read-only session mode: the policy face denies the write BEFORE any
/// resource or dispatch (the T03 matrix applied to the real file tools).
#[tokio::test]
async fn read_only_session_denies_file_writes_at_the_policy_face() {
    let harness = file_harness(StepsProvider::new(vec![
        Step::Tool(tool_request(
            "tool:first-party:write",
            json!({"path": "blocked.txt", "content": "never"}),
        )),
        Step::Tool(tool_request(
            "tool:first-party:read",
            json!({"path": "allowed.txt"}),
        )),
        Step::Tool(final_turn("done")),
    ]))
    .await;
    std::fs::write(harness.ws.join("allowed.txt"), "readable").expect("allowed");
    let session = "sess_local_alpha";
    harness
        .state
        .sessions()
        .set_permission_mode_for(&owner_principal(), session, SessionPermissionMode::ReadOnly)
        .await
        .expect("read_only mode");
    let run_id = harness
        .state
        .sessions()
        .execute_for(
            harness.state.storage().as_ref(),
            harness.state.events(),
            harness.state.runs(),
            &owner_principal(),
            session,
            "RO: write then read",
            NOW_MS,
        )
        .await
        .expect("the run completes with the refusal")
        .run_id;
    let journal = harness
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 2);
    let write_receipt = journal[0].receipt.as_ref().expect("write receipt");
    assert_eq!(
        write_receipt.outcome,
        lingxi_kernel::ports::ReceiptOutcome::Failed
    );
    assert!(!write_receipt.dispatched);
    assert!(
        write_receipt.detail.contains("ACTION_BLOCKED_BY_READ_ONLY"),
        "{}",
        write_receipt.detail
    );
    assert!(!harness.ws.join("blocked.txt").exists(), "no side effect");
    assert_eq!(
        journal[1].receipt.as_ref().expect("read receipt").outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );
    teardown(&harness).await;
}
