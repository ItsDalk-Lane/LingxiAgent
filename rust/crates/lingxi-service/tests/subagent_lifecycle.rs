//! R03-T06 integration: the subagent thread LIFECYCLE — dispatch →
//! reply (continuation) → close — mapped onto child runs of the same
//! supervisor, with the incumbent's validations (open thread of the SAME
//! session; busy threads refuse continuation; closed threads refuse
//! everything) and the lineage of continuation runs (parentRunId = the
//! run whose subagent_reply tool call dispatched them).
//!
//! Test-double boundary: the provider/tool doubles below only produce
//! external responses; every state decision belongs to the real
//! RunSupervisor + subagent runtime + storage.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use lingxi_kernel::ports::{
    DelegationRequest, ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult,
    ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, ToolCallId,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};

const PARENT_INPUT: &str = "LIFE: parent run";
const CHILD_A_TASK: &str = "CHILD-A: first task";
const CHILD_B_TASK: &str = "CHILD-B: continuation task";
const NOW_MS: u64 = 1_790_409_600_000;

// ── doubles ──────────────────────────────────────────────────────────────────

/// One scripted turn: a fixed provider turn, or a DELEGATION resolved at
/// call time against the thread the test observed (the dispatch result
/// carries the thread id to the model; the double receives it through
/// the shared slot the test fills between gated turns).
enum Step {
    Turn(ProviderTurn),
    Dispatch {
        task: &'static str,
    },
    Reply {
        task: &'static str,
    },
    Close {
        reason: &'static str,
    },
    ReplyToThread {
        thread_id: String,
        task: &'static str,
    },
}

/// Shared slot: the thread id of the dispatched subagent (filled by the
/// test once the runtime registry shows it).
#[derive(Default)]
struct ThreadSlot {
    thread: Mutex<Option<String>>,
}

impl ThreadSlot {
    fn set(&self, thread: String) {
        *self.thread.lock().unwrap() = Some(thread);
    }

    fn get(&self) -> Option<String> {
        self.thread.lock().unwrap().clone()
    }
}

struct ScriptedProvider {
    scripts: Mutex<HashMap<String, VecDeque<Step>>>,
    slot: Arc<ThreadSlot>,
    gates: HashMap<(String, usize), Arc<tokio::sync::Semaphore>>,
    pops: Mutex<HashMap<String, usize>>,
}

impl ScriptedProvider {
    fn new(
        scripts: Vec<(&'static str, Vec<Step>)>,
        slot: Arc<ThreadSlot>,
        gates: Vec<((&'static str, usize), Arc<tokio::sync::Semaphore>)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scripts: Mutex::new(
                scripts
                    .into_iter()
                    .map(|(marker, script)| (marker.to_string(), script.into_iter().collect()))
                    .collect(),
            ),
            slot,
            gates: gates
                .into_iter()
                .map(|((marker, pop), gate)| ((marker.to_string(), pop), gate))
                .collect(),
            pops: Mutex::new(HashMap::new()),
        })
    }

    fn marker_of(input: &str) -> String {
        input.split(':').next().unwrap_or(input).trim().to_string()
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
        _turn: u32,
        input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let marker = Self::marker_of(input);
        let pop = {
            let mut pops = self.pops.lock().unwrap();
            let next = pops.get(&marker).copied().unwrap_or(0) + 1;
            pops.insert(marker.clone(), next);
            next
        };
        let gate = self.gates.get(&(marker.clone(), pop)).cloned();
        let ctx_at_issue = ctx.clone();
        let step = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&marker)
            .and_then(|queue| queue.pop_front());
        // The thread-id resolution happens AFTER the gate (the parked
        // turn must observe the state the test arranged while it waits).
        let slot = Arc::clone(&self.slot);
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await.expect("provider gate closed");
            }
            let turn = match step {
                Some(Step::Turn(turn)) => turn,
                Some(Step::Dispatch { task }) => delegation_step_static("subagent", task, None),
                Some(Step::Reply { task }) => {
                    let thread_id = slot.get().expect("the test filled the thread slot");
                    delegation_step_static("subagent_reply", task, Some(thread_id))
                }
                Some(Step::Close { reason }) => {
                    let thread_id = slot.get().expect("the test filled the thread slot");
                    delegation_step_static("subagent_close", reason, Some(thread_id))
                }
                Some(Step::ReplyToThread { thread_id, task }) => {
                    delegation_step_static("subagent_reply", task, Some(thread_id))
                }
                None => ProviderTurn::Failed {
                    error: ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        "script exhausted",
                        false,
                    ),
                    retryable: false,
                },
            };
            ProviderTurnResult::of_ctx(&ctx_at_issue, turn)
        })
    }
}

/// Free function form of the delegation-request builder (usable inside
/// the pinned future without borrowing the double).
fn delegation_step_static(
    target: &'static str,
    task: &str,
    thread_id: Option<String>,
) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        requests: vec![ToolRequest {
            target: target.to_string(),
            args_digest: lingxi_protocol::digest_arguments(&serde_json::json!({
                "task": task,
            })),
            args_summary: Some(format!("{target}: {task}")),
            delegation: Some(DelegationRequest {
                task: task.to_string(),
                access: None,
                label: None,
                agent_id: None,
                model: None,
                thread_id,
            }),
        }],
    }
}

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
                ToolOutcome::Success {
                    content_digest: format!("executed:{}", request.target),
                },
            )
        })
    }
}

fn final_turn(text: &str) -> Step {
    Step::Turn(ProviderTurn::Final {
        message: NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            model_call_id: None,
        },
    })
}

// ── harness ──────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t06-life-{tag}-{}-{}",
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

async fn boot(tag: &str, provider: Arc<ScriptedProvider>) -> (ServiceState, PathBuf) {
    let home = synthetic_home(tag);
    let layout = prepare_layout(&home).expect("layout");
    let (tool_tx, _tool_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(Arc::new(RecordingTool { arrivals: tool_tx })),
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

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
}

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

fn thread_settled(state: &ServiceState) -> bool {
    state
        .subagents()
        .threads_of("sess_local_alpha")
        .iter()
        .any(|thread| !thread.busy && thread.last_run_status.is_some())
}

// ── dispatch → reply → close through the REAL driver chain ───────────────────

#[tokio::test]
async fn dispatch_reply_close_round_trip_on_the_real_chain() {
    let slot = Arc::new(ThreadSlot::default());
    // Deterministic ordering anchors: the parent's reply turn waits for
    // the test to observe the settled child A; the close turn waits for
    // the continuation child B to settle (a close of a BUSY thread is a
    // loud refusal — see the busy test below).
    let reply_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let close_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(
        vec![
            (
                "LIFE",
                vec![
                    Step::Dispatch { task: CHILD_A_TASK },
                    Step::Reply { task: CHILD_B_TASK },
                    Step::Close {
                        reason: "no longer needed",
                    },
                    final_turn("parent done"),
                ],
            ),
            ("CHILD-A", vec![final_turn("child A done")]),
            ("CHILD-B", vec![final_turn("child B done")]),
        ],
        Arc::clone(&slot),
        vec![
            (("LIFE", 2), Arc::clone(&reply_gate)),
            (("LIFE", 3), Arc::clone(&close_gate)),
        ],
    );
    let (state, home) = boot("roundtrip", Arc::clone(&provider)).await;

    // The parent runs in its own task (its middle turns are gated).
    let state_for_task = state.clone();
    let parent = tokio::spawn(async move {
        let storage = Arc::clone(state_for_task.storage());
        state_for_task
            .sessions()
            .execute_for(
                storage.as_ref(),
                state_for_task.events(),
                state_for_task.runs(),
                &owner_principal(),
                "sess_local_alpha",
                "LIFE: parent run",
                NOW_MS,
            )
            .await
            .expect("the parent settles (dispatch → reply → close → final)")
            .run_id
    });

    // Child A settles; the thread is open, idle, and queryable.
    wait_until("child A settles", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| !thread.busy && thread.last_run_status.is_some())
    })
    .await;
    let thread_id = state
        .subagents()
        .threads_of("sess_local_alpha")
        .into_iter()
        .next()
        .expect("the thread exists")
        .thread_id;
    assert!(!thread_id.is_empty());
    slot.set(thread_id);
    reply_gate.add_permits(1);

    // The continuation child B settles on the SAME thread.
    wait_until("child B settles", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| !thread.busy)
    })
    .await;
    // (child A already satisfies !busy — wait for the SECOND run of the
    // thread by counting runs, then for the thread to be idle.)
    wait_until("two child runs exist", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| {
                thread
                    .child_run_id
                    .as_deref()
                    .is_some_and(|id| !id.is_empty())
            })
    })
    .await;
    close_gate.add_permits(1);

    let parent_run = parent.await.expect("the parent task");
    let _ = parent_run;

    // The thread settled with the CONTINUATION child and is CLOSED.
    let snapshot = state
        .subagents()
        .threads_of("sess_local_alpha")
        .into_iter()
        .next()
        .expect("the thread exists");
    assert_eq!(snapshot.status.wire_name(), "closed");
    assert!(!snapshot.busy);
    // The close reason becomes the thread's recorded summary anchor.
    assert_eq!(
        snapshot.last_run_status.as_deref(),
        Some("closed: no longer needed")
    );

    // Three runs total: the parent + two child runs of the SAME thread.
    let runs = query_text(
        &state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        "sess_local_alpha",
    )
    .await
    .expect("count");
    assert_eq!(runs.as_str(), "3", "parent + dispatch child + reply child");

    // The CONTINUATION child's lineage: parentRunId = the parent run
    // (whose subagent_reply dispatched it), origin = subagent.
    let continuation = snapshot
        .child_run_id
        .expect("the thread names its last run");
    let lineage = state
        .sessions()
        .run_lineage_for(state.storage().as_ref(), &owner_principal(), &continuation)
        .await
        .expect("lineage query")
        .expect("the continuation's lineage row exists");
    assert_eq!(
        lineage.parent_run_id.map(|p| p.to_string()).as_deref(),
        Some(parent_run.as_str()),
        "the continuation is parented by the run that replied"
    );
    assert_eq!(lineage.origin.wire_name(), "subagent");
    let cause = lineage.cause_id.expect("causeId set");
    assert!(
        cause.starts_with(&format!("{parent_run}-tc")),
        "the continuation's cause anchors the parent's reply tool call: {cause}"
    );
    teardown(&state, &home).await;
}

// ── validations: unknown thread / busy thread ────────────────────────────────

#[tokio::test]
async fn reply_validations_refuse_unknown_threads_loudly() {
    let slot = Arc::new(ThreadSlot::default());
    let provider = ScriptedProvider::new(
        vec![(
            "LIFE",
            vec![
                Step::ReplyToThread {
                    thread_id: "no-such-thread".to_string(),
                    task: "continue nothing",
                },
                final_turn("parent saw the refusal"),
            ],
        )],
        Arc::clone(&slot),
        vec![],
    );
    let (state, home) = boot("unknown-thread", Arc::clone(&provider)).await;

    let parent_run = {
        let storage = Arc::clone(state.storage());
        state
            .sessions()
            .execute_for(
                storage.as_ref(),
                state.events(),
                state.runs(),
                &owner_principal(),
                "sess_local_alpha",
                PARENT_INPUT,
                NOW_MS,
            )
            .await
            .expect("the parent settles with the recorded refusal")
            .run_id
    };

    // The refusal is a structured tool failure the model saw; no run was
    // created and no thread was tracked.
    let event = query_text(
        &state,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
         'tool_call_completed'",
        &parent_run,
    )
    .await
    .expect("the refused reply has a completed tool event");
    assert!(
        event.contains("not_found") || event.contains("Unknown subagent thread"),
        "the unknown-thread refusal reached the model: {event}"
    );
    let runs = query_text(
        &state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        "sess_local_alpha",
    )
    .await
    .expect("count");
    assert_eq!(runs.as_str(), "1", "the refused reply created zero runs");
    assert!(state.subagents().threads_of("sess_local_alpha").is_empty());
    teardown(&state, &home).await;
}

#[tokio::test]
async fn busy_thread_refuses_reply_with_a_structured_conflict() {
    // The child parks mid-turn (a slow model): the thread stays BUSY
    // while the parent's reply arrives → the loud ThreadBusy refusal
    // (the incumbent queues serialized continuations; a background reply
    // queue is deferred with the R07 entry work — R03 refuses loudly).
    let slot = Arc::new(ThreadSlot::default());
    let child_park = Arc::new(tokio::sync::Semaphore::new(0));
    let reply_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = ScriptedProvider::new(
        vec![
            (
                "LIFE",
                vec![
                    Step::Dispatch {
                        task: "CHILD-A: slow task",
                    },
                    Step::Reply { task: CHILD_B_TASK },
                    final_turn("parent saw the busy refusal"),
                ],
            ),
            ("CHILD-A", vec![final_turn("child A finally done")]),
        ],
        Arc::clone(&slot),
        vec![
            (("CHILD-A", 1), Arc::clone(&child_park)),
            (("LIFE", 2), Arc::clone(&reply_gate)),
        ],
    );
    let (state, home) = boot("busy", Arc::clone(&provider)).await;

    // The parent must run in its own task: its turn 2 (the reply) waits
    // until the test observed the busy child.
    let state_for_task = state.clone();
    let parent = tokio::spawn(async move {
        let storage = Arc::clone(state_for_task.storage());
        state_for_task
            .sessions()
            .execute_for(
                storage.as_ref(),
                state_for_task.events(),
                state_for_task.runs(),
                &owner_principal(),
                "sess_local_alpha",
                "LIFE: parent run",
                NOW_MS,
            )
            .await
            .expect("the parent settles after the busy refusal")
            .run_id
    });
    // Wait until the child is dispatched and BUSY, then arm the reply.
    wait_until("the child is busy", || {
        state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|thread| thread.busy)
    })
    .await;
    let thread_id = state
        .subagents()
        .threads_of("sess_local_alpha")
        .into_iter()
        .next()
        .expect("the thread exists")
        .thread_id;
    slot.set(thread_id);
    reply_gate.add_permits(1);
    let parent_run = parent.await.expect("parent task");
    // The busy refusal reached the model as a structured conflict.
    let event = query_text(
        &state,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
         'tool_call_completed' AND payload_json LIKE '%subagent%'",
        &parent_run,
    )
    .await
    .expect("the refused reply has a completed tool event");
    assert!(
        event.contains("conflict") || event.contains("busy"),
        "the busy refusal reached the model: {event}"
    );
    // No SECOND child was created by the refused reply.
    let runs = query_text(
        &state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        "sess_local_alpha",
    )
    .await
    .expect("count");
    assert_eq!(runs.as_str(), "2", "parent + the one busy child only");

    // Release the parked child so the fixture tears down cleanly.
    child_park.add_permits(1);
    wait_until("the child settles", || thread_settled(&state)).await;
    teardown(&state, &home).await;
}
