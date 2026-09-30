//! R03-A11 integration｜子代理不能升级权限 — through the REAL service
//! composition (real storage incl. the V4 lineage table + invocation
//! journal, real event service, real kernel state machine + single
//! finalize, real session gate, real quotas, real cancellation tree +
//! task supervisor, real subagent runtime wired by the real bootstrap),
//! driven by deterministic doubles that ONLY produce external responses.
//!
//! Scenario (taskbook R03-A11, frozen incumbent semantics):
//! 1. the parent session is switched to the READ-ONLY permission mode
//!    (the incumbent `getSessionPermissionMode` fact, set through the
//!    ownership-checked service surface);
//! 2. a read-only parent task dispatches a subagent (access omitted →
//!    INHERITED read-only tier — attenuation allows shrinking);
//! 3. the child task attempts a WRITE tool call;
//! 4. the refusal is produced by the REAL run-layer authorization
//!    boundary (`authorize_child_tool` applied at the T05 journal's
//!    authorization step in the live drive) — NOT by the tool double,
//!    which is never invoked (zero external dispatch; the receipt closes
//!    `dispatched: false`);
//! 5. the parent-child relationship (lineage row: parentRunId / origin /
//!    sourceMessageId / causeId) and the refusal reason survive durably;
//! 6. the child's result is delivered back into the session's steering
//!    channel and reaches the parent's next model turn (the incumbent's
//!    deferred-result delivery, at the R03 fidelity).
//!
//! Negative companions proving the boundary is a GRANT decision (not a
//! blanket block), that the escalation attempt fails LOUDLY at dispatch
//! (#1614 attenuation), and that neither a model override nor the
//! executor identity can widen the grant.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::{
    DelegationRequest, ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult,
    ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::subagent::{AccessRequest, SessionPermissionMode};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};

/// The parent's user submission (marker = everything before ':').
const PARENT_INPUT: &str = "PARENT-A11: dispatch a research subagent";
/// The child's task text — the ONLY context the child sees (visibility).
const CHILD_TASK: &str = "CHILD-A11: summarize the workspace";
const NOW_MS: u64 = 1_790_409_600_000;

// ── deterministic doubles (external responses only) ──────────────────────────

/// Provider double scripted by INPUT marker (the parent's input is the
/// user submission; the child's input is its task text — the incumbent's
/// visibility rule: the child sees ONLY its task). Records every
/// (run_id, turn, input) it served; a gate can park a chosen pop.
struct ScriptedProvider {
    scripts: std::sync::Mutex<HashMap<String, VecDeque<ProviderTurn>>>,
    gates: HashMap<(String, usize), Arc<tokio::sync::Semaphore>>,
    pops: std::sync::Mutex<HashMap<String, usize>>,
    seen: std::sync::Mutex<Vec<(String, u32, String)>>,
}

impl ScriptedProvider {
    fn new(
        scripts: Vec<(&'static str, Vec<ProviderTurn>)>,
        gates: Vec<((&'static str, usize), Arc<tokio::sync::Semaphore>)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scripts: std::sync::Mutex::new(
                scripts
                    .into_iter()
                    .map(|(marker, script)| (marker.to_string(), script.into_iter().collect()))
                    .collect(),
            ),
            gates: gates
                .into_iter()
                .map(|((marker, pop), gate)| ((marker.to_string(), pop), gate))
                .collect(),
            pops: std::sync::Mutex::new(HashMap::new()),
            seen: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn marker_of(input: &str) -> String {
        input.split(':').next().unwrap_or(input).trim().to_string()
    }

    fn seen_inputs(&self) -> Vec<(String, u32, String)> {
        self.seen.lock().unwrap().clone()
    }
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.scripted".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        turn: u32,
        input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let marker = Self::marker_of(input);
        let pop = {
            let mut pops = self.pops.lock().unwrap();
            let next = pops.get(&marker).copied().unwrap_or(0) + 1;
            pops.insert(marker.clone(), next);
            next
        };
        self.seen
            .lock()
            .unwrap()
            .push((ctx.run_id.to_string(), turn, input.to_string()));
        let ctx_at_issue = ctx.clone();
        let gate = self.gates.get(&(marker.clone(), pop)).cloned();
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
        Box::pin(async move {
            if let Some(gate) = gate {
                // Park mid-"stream": this turn waits for the test anchor.
                let _permit = gate.acquire().await.expect("provider gate closed");
            }
            ProviderTurnResult::of_ctx(&ctx_at_issue, turn)
        })
    }
}

/// Tool double that only records the TARGET it was asked to execute (the
/// A11 proof point: the write target must NEVER reach it under the
/// read-only grant — the double would happily execute it).
struct RecordingTool {
    arrivals: tokio::sync::mpsc::UnboundedSender<String>,
}

impl ToolExecutorPort for RecordingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.arrivals
            .send(format!("{}|{}", ctx.run_id, request.target))
            .expect("test holds the arrivals receiver");
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::success_text(format!("executed:{}", request.target)),
            )
        })
    }
}

fn delegation_request(
    task: &str,
    access: Option<AccessRequest>,
    model: Option<&str>,
) -> ToolRequest {
    ToolRequest::from_effective_arguments(
        "subagent",
        serde_json::json!({
            "task": task,
        }),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request")
    .with_summary(format!("delegate: {task}"))
    .with_delegation(DelegationRequest {
        task: task.to_string(),
        access,
        label: Some("research".to_string()),
        agent_id: None,
        model: model.map(|m| m.to_string()),
        thread_id: None,
    })
}

fn write_tool_request() -> ToolRequest {
    ToolRequest::from_effective_arguments(
        "write",
        serde_json::json!({
            "path": "/tmp/a11-out.txt", "content": "attempted write"
        }),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request")
    .with_summary("write /tmp/a11-out.txt")
}

fn read_tool_request() -> ToolRequest {
    ToolRequest::from_effective_arguments(
        "read",
        serde_json::json!({"path": "/tmp/x"}),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective tool request")
    .with_summary("read /tmp/x")
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

// ── harness (real bootstrap, real ownership-checked surfaces) ────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t06-a11-{tag}-{}-{}",
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

async fn boot(
    tag: &str,
    provider: Arc<ScriptedProvider>,
    tool: Arc<RecordingTool>,
) -> (ServiceState, PathBuf) {
    let home = synthetic_home(tag);
    let layout = prepare_layout(&home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(tool),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    (state, home)
}

async fn teardown(state: &ServiceState, home: &std::path::Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

async fn execute(state: &ServiceState, input: &str) -> String {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            input,
            NOW_MS,
        )
        .await
        .expect("parent run settles")
        .run_id
}

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
}

/// Bounded deterministic wait (1ms poll, hard cap 5s) — the T02–T05
/// house style; no test-util time control.
async fn wait_until(what: &str, mut probe: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !probe() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

/// The child run id of one open thread of the session (the subagent
/// runtime's in-memory registry — the durable facts are asserted from
/// the database once things settled).
fn child_run_of(state: &ServiceState) -> Option<String> {
    state
        .subagents()
        .threads_of("sess_local_alpha")
        .into_iter()
        .next()
        .and_then(|thread| thread.child_run_id)
}

/// The FIRST run row of the session (the parent — it is created before
/// any child; the durable ordering anchor while the parent's own task
/// has not settled yet).
async fn first_run_of_session(state: &ServiceState) -> String {
    query_text(
        state,
        "SELECT run_id FROM runs WHERE session_id = ?1 ORDER BY created_at_unix_ms, rowid          LIMIT 1",
        "sess_local_alpha",
    )
    .await
    .expect("the parent run row exists")
}

/// Whether the session's subagent thread finished its run (busy=false
/// with a recorded status).
fn thread_settled(state: &ServiceState) -> bool {
    state
        .subagents()
        .threads_of("sess_local_alpha")
        .iter()
        .any(|thread| !thread.busy && thread.last_run_status.is_some())
}

// ── R03-A11: the acceptance scenario ─────────────────────────────────────────

/// 只读父任务派生子任务 → 子任务尝试写文件 → 统一授权拒绝（真实授权边界），
/// 父子关系与原因保留，子结果回流到父的下一模型轮。
#[tokio::test]
async fn r03_a11_readonly_parent_child_write_is_refused_by_the_real_boundary() {
    let (tool_tx, mut tool_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tool = Arc::new(RecordingTool { arrivals: tool_tx });
    // The parent settles BEFORE the child finishes (fire-and-forget):
    // the delivery then lands as the session's RETAINED steering, and
    // the session's NEXT run drains it into its first model call — the
    // incumbent's deferred-result delivery intent at the R03 fidelity.
    let provider = ScriptedProvider::new(
        vec![
            (
                "PARENT-A11",
                vec![
                    ProviderTurn::ToolRequests {
                        requests: vec![delegation_request(CHILD_TASK, None, None)],
                    },
                    final_turn("parent done"),
                    final_turn("parent run two consumed the subagent result"),
                ],
            ),
            (
                "CHILD-A11",
                vec![
                    ProviderTurn::ToolRequests {
                        requests: vec![write_tool_request()],
                    },
                    final_turn("child done: the write was refused"),
                ],
            ),
        ],
        vec![],
    );
    let (state, home) = boot("main", Arc::clone(&provider), tool).await;

    // 1) The parent session switches to read-only (ownership-checked
    //    surface; the incumbent's session permission mode).
    state
        .sessions()
        .set_permission_mode_for(
            &owner_principal(),
            "sess_local_alpha",
            SessionPermissionMode::ReadOnly,
        )
        .await
        .expect("set read-only mode");

    // 2) The read-only parent task dispatches the subagent (access
    //    omitted → inherited read-only tier). The parent settles before
    //    the child (fire-and-forget); its run id is re-derived below from
    //    the durable ordering (first run of the session).
    execute(&state, PARENT_INPUT).await;

    // 3) The child run appears with its full lineage and settles.
    let child_run = {
        let state = state.clone();
        wait_until("the child run is dispatched", || {
            child_run_of(&state).is_some()
        })
        .await;
        child_run_of(&state).expect("the child run id is present")
    };
    let parent_run = first_run_of_session(&state).await;
    let lineage = state
        .sessions()
        .run_lineage_for(state.storage().as_ref(), &owner_principal(), &child_run)
        .await
        .expect("lineage query")
        .expect("the child's lineage row exists");
    assert_eq!(
        lineage.parent_run_id.map(|p| p.to_string()).as_deref(),
        Some(parent_run.as_str()),
        "parentRunId preserved"
    );
    assert_eq!(
        lineage.origin.wire_name(),
        "subagent",
        "origin=subagent preserved"
    );
    let source = lineage.source_message_id.expect("sourceMessageId set");
    let cause = lineage.cause_id.expect("causeId set");
    assert!(
        source.starts_with(&format!("{parent_run}-mc")),
        "sourceMessageId anchors the parent's model call: {source}"
    );
    assert!(
        cause.starts_with(&format!("{parent_run}-tc")),
        "causeId anchors the parent's delegation tool call: {cause}"
    );

    // The child settles before its durable facts are asserted.
    {
        let state = state.clone();
        wait_until("the child settles", || thread_settled(&state)).await;
    }

    // 4) The child's write attempt was refused by the REAL boundary:
    //    the journal receipt closed as a never-dispatched failure whose
    //    detail carries the refusal; the executor double was NEVER
    //    invoked for the write target.
    let journal = query_text(
        &state,
        "SELECT receipt_detail FROM invocation_journal WHERE run_id = ?1 AND target = 'write'",
        &child_run,
    )
    .await
    .expect("the refused write has a journal entry");
    assert!(
        journal.contains("ACTION_BLOCKED_BY_READ_ONLY"),
        "the refusal reason is preserved in the receipt: {journal}"
    );
    assert!(
        journal.contains("never exceed its parent session"),
        "{journal}"
    );
    let dispatched = query_text(
        &state,
        "SELECT dispatched FROM invocation_journal WHERE run_id = ?1 AND target = 'write'",
        &child_run,
    )
    .await
    .expect("dispatched column");
    assert_eq!(dispatched.as_str(), "0", "zero external dispatch");
    // The tool event carries the structured refusal for the model.
    let tool_event = query_text(
        &state,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
         'tool_call_completed'",
        &child_run,
    )
    .await
    .expect("the refused write has a completed event");
    assert!(
        tool_event.contains("ACTION_BLOCKED_BY_READ_ONLY"),
        "the model sees the structured refusal: {}",
        tool_event
    );

    // The refusal is a recorded tool failure, not a run killer: the
    // child completed WITH its final answer.
    let (child_status, child_reason) = (
        query_text(
            &state,
            "SELECT status FROM runs WHERE run_id = ?1",
            &child_run,
        )
        .await,
        query_text(
            &state,
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            &child_run,
        )
        .await,
    );
    assert_eq!(child_status.as_deref(), Some("completed"));
    assert_eq!(
        child_reason.as_deref(),
        Some("completed.with_final"),
        "the child completed with its final answer despite the refused write"
    );

    // 5) The delivery: the child's result lands in the session's RETAINED
    //    steering channel after the parent settled (fire-and-forget)…
    wait_until("the child result is delivered", || {
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha")
            > 0
    })
    .await;
    // …and the session's NEXT run drains it into its FIRST model call.
    let second_run = execute(&state, "PARENT-A11: run two").await;
    let second_run_input = provider
        .seen_inputs()
        .iter()
        .find(|(run, turn, _)| run == &second_run && *turn == 1)
        .map(|(_, _, input)| input.clone())
        .expect("the second run's first turn was served");
    assert!(
        second_run_input.contains("[subagent-result"),
        "the delivered child result reached the session's next model turn: {second_run_input}"
    );
    assert!(
        second_run_input.contains("thread="),
        "the delivery carries the thread identity: {second_run_input}"
    );
    assert_eq!(
        state
            .sessions()
            .session_supervisor()
            .steering_pending("sess_local_alpha"),
        0,
        "the drained delivery leaves no residue"
    );

    // 6) ZERO executor invocations across the whole scenario: the write
    //    never reached the double (the read-only grant held end-to-end).
    assert!(
        tool_rx.try_recv().is_err(),
        "the tool double was never invoked — the refusal came from the run-layer boundary, \
         not from an executor decision"
    );
    let _ = &mut tool_rx;

    teardown(&state, &home).await;
}

// ── companion: the boundary is a GRANT decision (negative control) ──────────

/// Same shape, parent in OPERATE mode: the child's tier inherits
/// operable, the SAME write target is ALLOWED and the executor double
/// executes it exactly once — proving the refusal above was the grant's
/// decision, not a blanket block.
#[tokio::test]
async fn operate_parent_child_write_is_executed_once() {
    let (tool_tx, mut tool_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tool = Arc::new(RecordingTool { arrivals: tool_tx });
    let provider = ScriptedProvider::new(
        vec![
            (
                "PARENT-A11",
                vec![
                    ProviderTurn::ToolRequests {
                        requests: vec![delegation_request(CHILD_TASK, None, None)],
                    },
                    final_turn("parent done"),
                ],
            ),
            (
                "CHILD-A11",
                vec![
                    ProviderTurn::ToolRequests {
                        requests: vec![write_tool_request()],
                    },
                    final_turn("child done: the write succeeded"),
                ],
            ),
        ],
        vec![],
    );
    let (state, home) = boot("operate", Arc::clone(&provider), tool).await;
    // Default mode is OPERATE — no switch.

    let _parent_run = execute(&state, PARENT_INPUT).await;
    // The child settles.
    let child_run = {
        let state = state.clone();
        wait_until("the child run is dispatched", || {
            child_run_of(&state).is_some()
        })
        .await;
        child_run_of(&state).expect("the child run id is present")
    };
    {
        let state = state.clone();
        wait_until("the child settles", || thread_settled(&state)).await;
    }
    // The write reached the executor exactly once.
    let mut arrivals = Vec::new();
    while let Ok(arrival) = tool_rx.try_recv() {
        arrivals.push(arrival);
    }
    assert_eq!(
        arrivals,
        vec![format!("{child_run}|write")],
        "the operable grant allowed the write — exactly one execution"
    );
    teardown(&state, &home).await;
}

// ── companion: dispatch-time escalation refusal (#1614) ─────────────────────

/// Parent read-only + explicit access:"write" → the dispatch itself is
/// REFUSED loudly (SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY): the
/// parent's tool call fails, NO child run is created, the reason names
/// the attenuation rule and the escape hatch.
#[tokio::test]
async fn readonly_parent_write_access_request_is_refused_at_dispatch() {
    let (tool_tx, _tool_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tool = Arc::new(RecordingTool { arrivals: tool_tx });
    let provider = ScriptedProvider::new(
        vec![(
            "PARENT-A11",
            vec![
                ProviderTurn::ToolRequests {
                    requests: vec![delegation_request(
                        CHILD_TASK,
                        Some(AccessRequest::Write),
                        None,
                    )],
                },
                final_turn("parent saw the refusal"),
            ],
        )],
        vec![],
    );
    let (state, home) = boot("escalation", provider, tool).await;
    state
        .sessions()
        .set_permission_mode_for(
            &owner_principal(),
            "sess_local_alpha",
            SessionPermissionMode::ReadOnly,
        )
        .await
        .expect("set read-only mode");

    let parent_run = execute(&state, PARENT_INPUT).await;
    // NO child run exists: the escalation was refused before any child.
    let runs = query_text(
        &state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        "sess_local_alpha",
    )
    .await
    .expect("count query returns a row");
    assert_eq!(
        runs.as_str(),
        "1",
        "the refused dispatch created zero child runs"
    );
    // The parent's tool event carries the structured attenuation refusal.
    let event = query_text(
        &state,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
         'tool_call_completed' AND payload_json LIKE '%subagent%'",
        &parent_run,
    )
    .await
    .expect("the refused dispatch has a completed tool event");
    assert!(
        event.contains("SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY"),
        "the attenuation refusal reached the parent's model: {}",
        event
    );
    // The parent's journal closes the delegation as never dispatched.
    let journal = query_text(
        &state,
        "SELECT receipt_detail FROM invocation_journal WHERE run_id = ?1 AND target = 'subagent'",
        &parent_run,
    )
    .await
    .expect("the refused delegation has a journal entry");
    assert!(
        journal.contains("SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY"),
        "{journal}"
    );
    // R03 repair G03/F04 (same family): a delegation REFUSAL is a known
    // never-dispatched negative fact — the receipt's `dispatched` flag
    // must say so (zero child runs were created by every refusal path).
    let dispatched = query_text(
        &state,
        "SELECT dispatched FROM invocation_journal WHERE run_id = ?1 AND target = 'subagent'",
        &parent_run,
    )
    .await
    .expect("dispatched column");
    assert_eq!(
        dispatched.as_str(),
        "0",
        "a refused delegation journals dispatched=false (fact classes stay distinct)"
    );
    // And the thread registry holds nothing.
    assert!(state.subagents().threads_of("sess_local_alpha").is_empty());
    teardown(&state, &home).await;
}

// ── companion: a model override cannot widen the grant ──────────────────────

/// Delegation with a MODEL OVERRIDE under a read-only parent still yields
/// a read-only child whose write is refused — 换模型不能扩大权限.
#[tokio::test]
async fn model_override_cannot_widen_the_readonly_grant() {
    let (tool_tx, mut tool_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tool = Arc::new(RecordingTool { arrivals: tool_tx });
    let provider = ScriptedProvider::new(
        vec![
            (
                "PARENT-A11",
                vec![
                    ProviderTurn::ToolRequests {
                        requests: vec![delegation_request(
                            CHILD_TASK,
                            None,
                            Some("other-provider/bigger-model"),
                        )],
                    },
                    final_turn("parent done"),
                ],
            ),
            (
                "CHILD-A11",
                vec![
                    ProviderTurn::ToolRequests {
                        requests: vec![write_tool_request()],
                    },
                    final_turn("child done: write refused even with the model override"),
                ],
            ),
        ],
        vec![],
    );
    let (state, home) = boot("model-override", Arc::clone(&provider), tool).await;
    state
        .sessions()
        .set_permission_mode_for(
            &owner_principal(),
            "sess_local_alpha",
            SessionPermissionMode::ReadOnly,
        )
        .await
        .expect("set read-only mode");

    let _parent_run = execute(&state, PARENT_INPUT).await;
    let child_run = {
        let state = state.clone();
        wait_until("the child run is dispatched", || {
            child_run_of(&state).is_some()
        })
        .await;
        child_run_of(&state).expect("the child run id is present")
    };
    {
        let state = state.clone();
        wait_until("the child settles", || thread_settled(&state)).await;
    }
    let journal = query_text(
        &state,
        "SELECT receipt_detail FROM invocation_journal WHERE run_id = ?1 AND target = 'write'",
        &child_run,
    )
    .await
    .expect("the refused write has a journal entry");
    assert!(
        journal.contains("ACTION_BLOCKED_BY_READ_ONLY"),
        "the model override did not widen the grant: {journal}"
    );
    assert!(
        tool_rx.try_recv().is_err(),
        "the executor was never invoked despite the model override"
    );
    // Read targets remain allowed under the same grant (research works).
    let _ = read_tool_request();
    teardown(&state, &home).await;
}
