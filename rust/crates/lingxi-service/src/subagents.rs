//! Sub-agent runtime (R03-T06 deliverable 子代理运行适配): the dispatch /
//! reply / close surface that maps the incumbent's `subagent`,
//! `subagent_reply` and `subagent_close` tools onto CHILD RUNS of the SAME
//! run supervisor.
//!
//! Frozen incumbent semantics this maps (read from the Node production
//! stack before mapping — no new interaction was invented):
//! - **Dispatch is fire-and-forget**
//!   (`lib/tools/subagent-tool.ts`): the parent's tool call returns
//!   immediately with the child identity; the child keeps running in the
//!   background and its result is DELIVERED BACK through the session's
//!   steering channel (the incumbent's DeferredResultStore delivery
//!   intent `trigger_parent_turn` — at the R03 fidelity: a busy session's
//!   running turn drains it before its next model call; an idle session
//!   RETAINS it for the next run's first model call; actively triggering
//!   a new parent turn is R06/R07).
//! - **Visibility**: the child sees ONLY its task text — the incumbent's
//!   rule that a subagent cannot see the parent conversation history
//!   unless it is included in the task. The parent-child relationship is
//!   durable in the lineage row instead.
//! - **Attenuation at dispatch**
//!   (`resolveSubagentToolAccess` / `resolvePermissionMode`): explicit
//!   `access` wins, else inherit the parent session's permission mode; a
//!   `write` request under a read-only parent is a LOUD refusal
//!   (`SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY`, issue #1614).
//! - **Concurrency** (per `createSubagentTool` closure): 10 per session,
//!   20 process-wide, loud refusals; a 30-minute timeout anchored at the
//!   child's ACTUAL start.
//! - **Reply continues an OPEN thread of the SAME session**
//!   (`subagent_reply` validations); **close** requires open + same
//!   session + not busy (`subagent_close` validations).
//! - **Parent cancellation reaches the child**
//!   (STATE_TRANSITIONS T8 `abortByParentSession`): the child run's
//!   cancellation root is linked UNDER the parent's scope, so cancelling
//!   the parent settles the child too (the child's own drive walks its
//!   four-phase cancellation and single finalize).
//!
//! R03 boundary honesty: the incumbent persists threads in a durable
//! store; the R03 thread registry is PROCESS-MEMORY and bounded (the
//! durable run rows + lineage carry the facts that survive restarts;
//! durable thread continuation is R06/R07). A busy thread's reply is a
//! loud `ThreadBusy` refusal here — the incumbent queues serialized
//! continuations (`runSerialized`); a background reply queue is deferred
//! with the R07 entry work.
//!
//! No second scheduler: the child run goes through the SAME
//! [`RunSupervisor::drive_run`] chain (kernel state machine, quotas,
//! journal, single finalize). Bridge/cron entries in R07 reuse
//! [`crate::background`] / the lineage surface — they must not build
//! another one.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use lingxi_adapters::storage::subagent_threads as thread_store;
use lingxi_kernel::model_exchange::ExchangeItem;
use lingxi_kernel::ports::{DelegationRequest, StorageError};
use lingxi_kernel::subagent::{
    resolve_subagent_access, AccessRequest, RunLineage, RunOrigin, SubagentAccessDenied,
    SubagentPolicy, ToolAccessTier,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ErrorCode, ModelCallId, ProtocolError, RunId, ToolCallId};
use serde_json::{json, Value};

use crate::cancel::CancelScope;
use crate::events::EventService;
use crate::runs::{DriveAuthorization, DriveError, RunGrant, RunSupervisor};
use crate::session_supervisor::SessionSupervisor;
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::RunFinish;

/// The launcher interface the run driver consumes (R03-T06). A TRAIT
/// OBJECT on purpose: the concrete [`SubagentRuntime`] holds a
/// `Weak<RunSupervisor>` back-reference while the supervisor holds a
/// `Weak<dyn SubagentLauncher>` — a concrete edge on either side would
/// make the pair's `Send`/`Sync` auto traits circular (the compiler
/// reports exactly that); the trait object's declared `Send + Sync`
/// breaks the propagation without any unsafe impl.
pub trait SubagentLauncher: Send + Sync {
    fn flush_closeouts<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
    /// Fresh dispatch (`subagent` target).
    fn dispatch(
        self: Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LaunchedChild, SubagentDispatchError>> + Send>>;
    /// Continuation (`subagent_reply` target).
    fn reply(
        self: Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LaunchedChild, SubagentDispatchError>> + Send>>;
    /// Close (`subagent_close` target) — synchronous, no run is created.
    fn close(
        self: Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ClosedThread, SubagentDispatchError>> + Send>>;
    fn list(
        self: Arc<Self>,
        parent: ParentRunFacts,
    ) -> Pin<Box<dyn Future<Output = Result<Value, SubagentDispatchError>> + Send>>;
    fn load_history<'a>(
        &'a self,
        ctx: &'a RunContext,
        agent: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ExchangeItem>, StorageError>> + Send + 'a>>;
    fn record_failure<'a>(
        &'a self,
        ctx: &'a RunContext,
        agent: &'a str,
        error: &'a ProtocolError,
    ) -> Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + 'a>>;
    fn record_history<'a>(
        &'a self,
        ctx: &'a RunContext,
        agent: &'a str,
        items: &'a [ExchangeItem],
        replace: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + 'a>>;
}

impl SubagentLauncher for SubagentRuntime {
    fn flush_closeouts<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + 'a>> {
        Box::pin(async move { SubagentRuntime::flush_closeouts(self).await })
    }
    fn dispatch(
        self: Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LaunchedChild, SubagentDispatchError>> + Send>> {
        Box::pin(async move { Self::dispatch_child(&self, parent, request).await })
    }

    fn reply(
        self: Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LaunchedChild, SubagentDispatchError>> + Send>> {
        Box::pin(async move { Self::reply_child(&self, parent, request).await })
    }

    fn close(
        self: Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ClosedThread, SubagentDispatchError>> + Send>> {
        Box::pin(async move { SubagentRuntime::close(&self, &parent, &request).await })
    }
    fn list(
        self: Arc<Self>,
        parent: ParentRunFacts,
    ) -> Pin<Box<dyn Future<Output = Result<Value, SubagentDispatchError>> + Send>> {
        Box::pin(async move { self.list_candidates(&parent).await })
    }
    fn load_history<'a>(
        &'a self,
        ctx: &'a RunContext,
        agent: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ExchangeItem>, StorageError>> + Send + 'a>> {
        Box::pin(async move {
            self.initialize().await?;
            if !thread_store::has_history(&self.storage, ctx.run_id.as_str()).await? {
                return Ok(Vec::new());
            }
            let raw = thread_store::load_history(&self.storage, ctx, agent).await?;
            let history = raw
                .iter()
                .map(crate::model_history::decode_item)
                .collect::<Result<Vec<_>, _>>()?;
            // 工具正文必须与原Run正式模型投影一致；不把界面卡片或任意档案当成历史。
            for item in &history {
                if let ExchangeItem::ToolResult { tool_call_id, .. } = item {
                    let projection=self.storage.query_one_text(
                        "SELECT e.payload_json FROM key_events e JOIN subagent_history_v1 h ON h.run_id=e.run_id WHERE e.event_type='tool_result_model_projection' AND json_extract(e.payload_json,'$.toolCallId')=?1 AND h.thread_id=(SELECT thread_id FROM subagent_history_v1 WHERE run_id=?2)",
                        vec![tool_call_id.to_string(),ctx.run_id.to_string()],
                    ).await?.ok_or_else(||StorageError::Corrupted{detail:"subagent tool history missing authoritative projection".into()})?;
                    let projection: Value =
                        serde_json::from_str(&projection).map_err(|e| StorageError::Corrupted {
                            detail: e.to_string(),
                        })?;
                    if crate::model_history::projected_tool_item(&projection)? != *item {
                        return Err(StorageError::Corrupted {
                            detail: "subagent tool history differs from authoritative projection"
                                .into(),
                        });
                    }
                }
                if let ExchangeItem::AssistantTurn { tool_calls, .. } = item {
                    for call in tool_calls {
                        let binding = call.declaration_binding.as_ref().ok_or_else(|| {
                            StorageError::Corrupted {
                                detail: "subagent history missing declaration".into(),
                            }
                        })?;
                        let target = binding
                            .execution
                            .as_ref()
                            .map(|e| e.target.as_str())
                            .unwrap_or(call.target.as_str());
                        let digest = binding
                            .execution
                            .as_ref()
                            .map(|e| e.args_digest.hex.as_str())
                            .unwrap_or(call.args_digest.hex.as_str());
                        let fact=self.storage.query_one_text(
                            "SELECT j.journal_id FROM invocation_journal j JOIN subagent_history_v1 h ON h.run_id=j.run_id WHERE j.journal_id=?1 AND j.target=?2 AND j.args_digest=?3 AND h.thread_id=(SELECT thread_id FROM subagent_history_v1 WHERE run_id=?4)",
                            vec![call.tool_call_id.to_string(),target.into(),digest.into(),ctx.run_id.to_string()],
                        ).await?;
                        if fact.is_none() {
                            return Err(StorageError::Corrupted {
                                detail: "subagent request history differs from original journal"
                                    .into(),
                            });
                        }
                    }
                }
            }
            crate::model_history::validate_pairs(&history)?;
            Ok(history)
        })
    }
    fn record_failure<'a>(
        &'a self,
        ctx: &'a RunContext,
        agent: &'a str,
        error: &'a ProtocolError,
    ) -> Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + 'a>> {
        Box::pin(async move {
            if !thread_store::has_history(&self.storage, ctx.run_id.as_str()).await? {
                return Ok(());
            }
            thread_store::record_failure(
                &self.storage,
                ctx,
                agent,
                &format!("{}: {}", error.code.wire_name(), error.message),
            )
            .await
        })
    }
    fn record_history<'a>(
        &'a self,
        ctx: &'a RunContext,
        agent: &'a str,
        items: &'a [ExchangeItem],
        replace: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + 'a>> {
        Box::pin(async move {
            if !thread_store::has_history(&self.storage, ctx.run_id.as_str()).await? {
                return Ok(());
            }
            let raw = items
                .iter()
                .map(crate::model_history::encode_item)
                .collect::<Result<Vec<_>, _>>()?;
            thread_store::record_history(&self.storage, ctx, agent, raw, replace).await
        })
    }
}

/// 只对真实first-party入口从实际参数提取委派，不让模型自填权限事实。
pub(crate) fn request_delegation(
    request: &lingxi_kernel::ports::ToolRequest,
) -> Option<DelegationRequest> {
    let family = request.target.strip_prefix("tool:first-party:")?;
    if !matches!(family, "subagent" | "subagent_reply" | "subagent_close") {
        return None;
    }
    let args = request.arguments.as_value();
    Some(DelegationRequest {
        task: if family == "subagent_close" {
            args["reason"]
                .as_str()
                .or_else(|| args["task"].as_str())
                .unwrap_or_default()
        } else {
            args["task"].as_str().unwrap_or_default()
        }
        .into(),
        access: AccessRequest::parse(args["access"].as_str()),
        label: args["label"]
            .as_str()
            .or_else(|| args["instance"].as_str())
            .map(str::to_string),
        agent_id: args["agent"].as_str().map(str::to_string),
        model: args["model"].as_str().map(str::to_string),
        thread_id: args["threadId"].as_str().map(str::to_string),
    })
}

/// 正式模型目录复用Node的参数形状；执行由统一Run驱动接管。
pub fn register_tools(
    registry: &lingxi_kernel::toolcatalog::ToolRegistry,
    gateway: &crate::toolgateway::ToolInvocationGateway,
    budget: &lingxi_kernel::toolcatalog::SchemaBudget,
) {
    use lingxi_kernel::toolcatalog::*;
    for (name, description, schema) in [
        (
            "subagent",
            "创建可续接子任务，立即返回taskId/threadId；结果回父会话。agent为?或list时读取真实候选。",
            json!({"type":"object","properties":{"task":{"type":"string"},"agent":{"type":"string"},"model":{"type":"string"},"label":{"type":"string"},"instance":{"type":"string"},"access":{"type":"string","enum":["read","write"]}},"required":["task"],"additionalProperties":false}),
        ),
        (
            "subagent_reply",
            "给当前父会话的开放子线程追加任务，续接该线程真实历史。",
            json!({"type":"object","properties":{"threadId":{"type":"string","minLength":1},"task":{"type":"string","minLength":1},"access":{"type":"string","enum":["read","write"]}},"required":["threadId","task"],"additionalProperties":false}),
        ),
        (
            "subagent_close",
            "关闭当前父会话指定的空闲开放子线程，返回最终closed状态。",
            json!({"type":"object","properties":{"threadId":{"type":"string","minLength":1},"reason":{"type":"string"}},"required":["threadId"],"additionalProperties":false}),
        ),
    ] {
        let registered = registry
            .register(
                ToolManifest {
                    origin: ToolOrigin::FirstParty,
                    local_name: name.into(),
                    display_name: name.into(),
                    aliases: Vec::new(),
                    version: "1.0.0".into(),
                    description: description.into(),
                    input_schema: lingxi_protocol::ToolSchemaDocument {
                        dialect: "json-schema/2020-12".into(),
                        schema,
                    },
                    output_schema: None,
                    permission: PermissionContract {
                        kind: PermissionKind::Read,
                        capability_base: format!("{name}.capability"),
                    },
                    availability: Availability::Available,
                    timeout_ms: Some(30_000),
                    max_concurrency: Some(4),
                    declared_permission: DeclaredPermission::None,
                    recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
                },
                budget,
            )
            .expect("子代理工具声明登记失败");
        gateway.bind_executor(
            registered.target_id,
            Arc::new(DelegationOnly),
            "子任务只通过正式Run驱动执行",
        );
    }
}

struct DelegationOnly;
impl lingxi_kernel::ports::ToolExecutorPort for DelegationOnly {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a lingxi_kernel::ports::ToolRequest,
    ) -> Pin<Box<dyn Future<Output = lingxi_kernel::ports::ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            lingxi_kernel::ports::ToolExecutionResult::of_ctx(
                ctx,
                lingxi_kernel::ports::ToolOutcome::Failed {
                    error: ProtocolError::new(
                        ErrorCode::Internal,
                        "subagent request did not reach the authoritative delegation driver",
                        false,
                    ),
                },
            )
        })
    }
}

/// The facts of the parent run a child is dispatched from (built by the
/// run driver at the delegation tool call; carries the lineage anchors).
pub struct ParentRunFacts {
    pub principal: lingxi_kernel::Principal,
    pub session_id: String,
    pub agent_id: String,
    pub parent_run_id: RunId,
    /// The parent run's ROOT cancellation scope (the child links under
    /// it — abortByParentSession semantics).
    pub parent_scope: CancelScope,
    /// The parent's model call that emitted the delegation request (the
    /// source message anchor).
    pub source_model_call: ModelCallId,
    /// The parent's delegation tool call (the cause anchor).
    pub cause_tool_call: ToolCallId,
    pub now_ms: u64,
}

/// Identity of one successfully launched child run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedChild {
    pub child_run_id: String,
    pub thread_id: String,
    pub status: String,
    pub progress_error: Option<String>,
}

impl LaunchedChild {
    pub fn result_text(&self) -> String {
        json!({"taskId":self.child_run_id,"childRunId":self.child_run_id,"threadId":self.thread_id,"status":self.status,"progressError":self.progress_error}).to_string()
    }
}

/// Identity of one successfully closed thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedThread {
    pub thread_id: String,
}

impl ClosedThread {
    pub fn result_text(&self) -> String {
        json!({"threadId":self.thread_id,"streamStatus":"closed","status":"closed"}).to_string()
    }
}

/// Dispatch/reply/close refusal. Every variant is loud and maps onto a
/// structured tool failure for the parent's model — none is a silent
/// no-op, and every refusal path leaves ZERO child runs created.
#[derive(Debug, Clone, PartialEq)]
pub enum SubagentDispatchError {
    /// The dispatch-time attenuation refusal (#1614).
    AccessDenied(SubagentAccessDenied),
    /// Per-session concurrent subagent cap (incumbent 10).
    SessionLimit {
        max: usize,
    },
    /// Process-wide concurrent subagent cap (incumbent 20).
    GlobalLimit {
        max: usize,
    },
    ThreadNotFound {
        thread_id: String,
    },
    ThreadNotOpen {
        thread_id: String,
    },
    ThreadNotInSession {
        thread_id: String,
        session_id: String,
    },
    ThreadBusy {
        thread_id: String,
    },
    ThreadRegistryFull {
        cap: usize,
    },
    /// A delegation payload on a non-subagent-family target.
    InvalidTarget(String),
    /// The runtime was never bound to a run supervisor (wiring error).
    NotBound,
    Storage(StorageError),
    /// The supervised-task registry refused the child spawn.
    SpawnRefused {
        cap: usize,
    },
    AgentUnavailable {
        agent: String,
        detail: String,
    },
}

impl SubagentDispatchError {
    /// The structured tool error the parent's model sees (mirrors the
    /// incumbent's `toolError` with a stable `errorCode`).
    pub fn tool_error(&self) -> ProtocolError {
        match self {
            SubagentDispatchError::AccessDenied(denial) => ProtocolError::new(
                ErrorCode::Forbidden,
                format!("{}: {}", denial.code, denial.message),
                false,
            ),
            SubagentDispatchError::SessionLimit { max } => ProtocolError::new(
                ErrorCode::BudgetExceeded,
                format!("subagent per-session concurrency limit reached ({max})"),
                false,
            ),
            SubagentDispatchError::GlobalLimit { max } => ProtocolError::new(
                ErrorCode::BudgetExceeded,
                format!("subagent global concurrency limit reached ({max})"),
                false,
            ),
            SubagentDispatchError::ThreadNotFound { thread_id } => ProtocolError::new(
                ErrorCode::NotFound,
                format!("Unknown subagent thread: {thread_id}"),
                false,
            ),
            SubagentDispatchError::ThreadNotOpen { thread_id } => ProtocolError::new(
                ErrorCode::Conflict,
                format!("Subagent thread is not open: {thread_id}"),
                false,
            ),
            SubagentDispatchError::ThreadNotInSession {
                thread_id,
                session_id,
            } => ProtocolError::new(
                ErrorCode::Forbidden,
                format!("Subagent thread {thread_id} does not belong to session {session_id}"),
                false,
            ),
            SubagentDispatchError::ThreadBusy { thread_id } => ProtocolError::new(
                ErrorCode::Conflict,
                format!("Subagent thread is busy: {thread_id}"),
                false,
            ),
            SubagentDispatchError::ThreadRegistryFull { cap } => ProtocolError::new(
                ErrorCode::BudgetExceeded,
                format!("subagent thread registry is at its cap ({cap})"),
                false,
            ),
            SubagentDispatchError::InvalidTarget(target) => ProtocolError::new(
                ErrorCode::InvalidMessage,
                format!("target {target:?} does not accept a delegation payload"),
                false,
            ),
            SubagentDispatchError::NotBound => ProtocolError::new(
                ErrorCode::Internal,
                "subagent runtime is not bound to a run supervisor",
                false,
            ),
            SubagentDispatchError::Storage(err) => {
                ProtocolError::new(ErrorCode::Internal, err.to_string(), false)
            }
            SubagentDispatchError::SpawnRefused { cap } => ProtocolError::new(
                ErrorCode::Internal,
                format!("supervised-task registry at its cap ({cap}); child spawn refused"),
                false,
            ),
            SubagentDispatchError::AgentUnavailable { agent, detail } => ProtocolError::new(
                ErrorCode::NotFound,
                format!("Subagent candidate unavailable: {agent}: {detail}"),
                false,
            ),
        }
    }
}

impl std::fmt::Display for SubagentDispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubagentDispatchError::AccessDenied(denial) => write!(f, "{}", denial.message),
            SubagentDispatchError::SessionLimit { max } => {
                write!(f, "subagent per-session limit reached ({max})")
            }
            SubagentDispatchError::GlobalLimit { max } => {
                write!(f, "subagent global limit reached ({max})")
            }
            SubagentDispatchError::ThreadNotFound { thread_id } => {
                write!(f, "unknown subagent thread {thread_id}")
            }
            SubagentDispatchError::ThreadNotOpen { thread_id } => {
                write!(f, "subagent thread {thread_id} is not open")
            }
            SubagentDispatchError::ThreadNotInSession {
                thread_id,
                session_id,
            } => write!(
                f,
                "subagent thread {thread_id} is not in session {session_id}"
            ),
            SubagentDispatchError::ThreadBusy { thread_id } => {
                write!(f, "subagent thread {thread_id} is busy")
            }
            SubagentDispatchError::ThreadRegistryFull { cap } => {
                write!(f, "subagent thread registry full ({cap})")
            }
            SubagentDispatchError::InvalidTarget(target) => {
                write!(f, "target {target:?} does not accept a delegation payload")
            }
            SubagentDispatchError::NotBound => {
                write!(f, "subagent runtime is not bound to a run supervisor")
            }
            SubagentDispatchError::Storage(err) => write!(f, "storage failure: {err}"),
            SubagentDispatchError::SpawnRefused { cap } => {
                write!(
                    f,
                    "supervised-task registry cap {cap} refused the child spawn"
                )
            }
            SubagentDispatchError::AgentUnavailable { agent, detail } => {
                write!(f, "subagent candidate {agent} unavailable: {detail}")
            }
        }
    }
}

/// Status of one subagent thread (the incumbent's open/closed vocabulary;
/// finished runs leave the thread OPEN for continuation — `finishRun
/// {close: false}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadStatus {
    Open,
    Closed,
}

impl ThreadStatus {
    pub fn wire_name(self) -> &'static str {
        match self {
            ThreadStatus::Open => "open",
            ThreadStatus::Closed => "closed",
        }
    }
}

/// One tracked subagent thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentThread {
    pub thread_id: String,
    pub parent_session_id: String,
    pub label: Option<String>,
    /// The tier the last dispatch ran under (a reply reuses it unless the
    /// caller passes an explicit access).
    pub tier: ToolAccessTier,
    /// Whether a child run of this thread is currently executing.
    pub busy: bool,
    /// The most recent child run id of this thread.
    pub child_run_id: Option<String>,
    pub status: ThreadStatus,
    /// Terminal status word of the most recent child run
    /// (resolved/failed/cancelled...), for status queries.
    pub last_run_status: Option<String>,
}

/// Read-only projection for status queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSnapshot {
    pub thread_id: String,
    pub label: Option<String>,
    pub tier: ToolAccessTier,
    pub busy: bool,
    pub child_run_id: Option<String>,
    pub status: ThreadStatus,
    pub last_run_status: Option<String>,
}

#[derive(Default)]
struct RuntimeState {
    active_per_session: HashMap<String, usize>,
    active_global: usize,
    threads: HashMap<String, SubagentThread>,
    /// Child runs whose completion bookkeeping already ran (R03 repair
    /// G01/F02: exactly-once accounting — a late or duplicated completion
    /// callback for an already-accounted run is a diagnosable no-op, never
    /// a second cap decrement and never a busy-clear for a newer run).
    /// Bounded (the oldest id drops first — a diagnostic ring, not state).
    accounted_children: std::collections::VecDeque<String>,
}

/// Bound of the accounted-completion ring (matches the other bounded
/// registries' vocabulary).
const ACCOUNTED_CHILDREN_CAP: usize = 1024;

/// The subagent runtime. Constructed by the composition root with the
/// single storage/events/session-state instances; the run supervisor is
/// bound AFTER construction (a `Weak` — the child tasks hold `Arc`s of
/// the supervisor only for their own lifetime, so no reference cycle
/// keeps a supervisor alive).
pub struct SubagentRuntime {
    storage: Arc<RunDatabase>,
    events: Arc<EventService>,
    sessions: Arc<SessionSupervisor>,
    policy: SubagentPolicy,
    supervisor: OnceLock<Weak<RunSupervisor>>,
    state: Mutex<RuntimeState>,
    agents_root: OnceLock<PathBuf>,
    storage_ready: tokio::sync::OnceCell<()>,
    pending_closeouts: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl std::fmt::Debug for SubagentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubagentRuntime")
            .field("policy", &self.policy)
            .field("bound", &self.supervisor.get().is_some())
            .finish_non_exhaustive()
    }
}

impl SubagentRuntime {
    pub fn new(
        storage: Arc<RunDatabase>,
        events: Arc<EventService>,
        sessions: Arc<SessionSupervisor>,
        policy: SubagentPolicy,
    ) -> Arc<Self> {
        Arc::new(Self {
            storage,
            events,
            sessions,
            policy,
            supervisor: OnceLock::new(),
            state: Mutex::new(RuntimeState::default()),
            agents_root: OnceLock::new(),
            storage_ready: tokio::sync::OnceCell::new(),
            pending_closeouts: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Binds the run supervisor children are driven through (bootstrap,
    /// after the supervisor is constructed). Binding twice is a loud
    /// wiring error.
    pub fn bind_supervisor(&self, supervisor: Weak<RunSupervisor>) {
        if self.supervisor.set(supervisor).is_err() {
            tracing::error!("subagent runtime bound twice — the second binding was ignored");
        }
    }

    pub fn policy(&self) -> &SubagentPolicy {
        &self.policy
    }

    pub fn bind_agents_root(&self, root: PathBuf) {
        if self.agents_root.set(root).is_err() {
            tracing::error!("subagent agent directory bound twice");
        }
    }

    pub async fn initialize(&self) -> Result<(), StorageError> {
        self.storage_ready
            .get_or_try_init(|| thread_store::initialize(&self.storage))
            .await?;
        flush_abnormal_closeouts(&self.storage, &self.pending_closeouts).await
    }

    pub async fn flush_closeouts(&self) -> Result<(), StorageError> {
        flush_abnormal_closeouts(&self.storage, &self.pending_closeouts).await
    }

    async fn validate_parent(&self, parent: &ParentRunFacts) -> Result<(), SubagentDispatchError> {
        let actual=self.storage.query_one_text(
            "SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND s.agent_id=?5",
            vec![parent.parent_run_id.to_string(),parent.session_id.clone(),parent.principal.storage_kind().into(),parent.principal.storage_subject(),parent.agent_id.clone()],
        ).await.map_err(SubagentDispatchError::Storage)?;
        if actual.is_none() {
            return Err(SubagentDispatchError::Storage(
                StorageError::InvalidRequest {
                    detail: "subagent_parent_ownership_mismatch".into(),
                },
            ));
        }
        Ok(())
    }

    fn candidates(&self, current: &str) -> Result<Vec<Value>, SubagentDispatchError> {
        let root = self
            .agents_root
            .get()
            .ok_or(SubagentDispatchError::NotBound)?;
        let mut candidates = Vec::new();
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(candidates),
            Err(e) => {
                return Err(SubagentDispatchError::AgentUnavailable {
                    agent: "list".into(),
                    detail: e.to_string(),
                });
            }
        };
        let canonical_root =
            std::fs::canonicalize(root).map_err(|e| SubagentDispatchError::AgentUnavailable {
                agent: "list".into(),
                detail: e.to_string(),
            })?;
        for entry in entries {
            let entry = entry.map_err(|e| SubagentDispatchError::AgentUnavailable {
                agent: "list".into(),
                detail: e.to_string(),
            })?;
            let id = entry.file_name().to_string_lossy().to_string();
            if id == current || !entry.path().join("config.yaml").is_file() {
                continue;
            }
            // 先验证正式agent身份，再核实际文件归属；不直接读取模型提供的路径。
            let roots = lingxi_adapters::storage::agent_memory::resolve_agent_roots(root, &id)
                .map_err(|e| SubagentDispatchError::AgentUnavailable {
                    agent: id.clone(),
                    detail: e.to_string(),
                })?;
            let config_path =
                std::fs::canonicalize(roots.agent_dir.join("config.yaml")).map_err(|e| {
                    SubagentDispatchError::AgentUnavailable {
                        agent: id.clone(),
                        detail: e.to_string(),
                    }
                })?;
            if !config_path.starts_with(&canonical_root) {
                return Err(SubagentDispatchError::AgentUnavailable {
                    agent: id.clone(),
                    detail: "agent config escapes actual roster root".into(),
                });
            }
            let scan =
                lingxi_adapters::storage::agent_memory::config::read_agent_config(&config_path)
                    .and_then(|doc| doc.scan())
                    .map_err(|e| SubagentDispatchError::AgentUnavailable {
                        agent: id.clone(),
                        detail: e.to_string(),
                    })?;
            candidates.push(json!({"id":id,"name":scan.agent_name.unwrap_or_else(||id.clone())}));
        }
        candidates.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        Ok(candidates)
    }

    pub async fn list_candidates(
        &self,
        parent: &ParentRunFacts,
    ) -> Result<Value, SubagentDispatchError> {
        self.initialize()
            .await
            .map_err(SubagentDispatchError::Storage)?;
        self.validate_parent(parent).await?;
        let agents = self.candidates(&parent.agent_id)?;
        let tasks = thread_store::progress(
            &self.storage,
            &parent.session_id,
            parent.principal.storage_kind(),
            &parent.principal.storage_subject(),
        )
        .await
        .map_err(SubagentDispatchError::Storage)?;
        Ok(
            json!({"agents":agents,"noCandidates":agents.is_empty(),"reason":if agents.is_empty(){Some("No other available agents")}else{None},"tasks":tasks}),
        )
    }

    async fn restore_thread(&self, id: &str) -> Result<Value, SubagentDispatchError> {
        self.initialize()
            .await
            .map_err(SubagentDispatchError::Storage)?;
        let record = thread_store::read_thread(&self.storage, id)
            .await
            .map_err(SubagentDispatchError::Storage)?
            .ok_or_else(|| SubagentDispatchError::ThreadNotFound {
                thread_id: id.into(),
            })?;
        let required = |key: &str| {
            record[key].as_str().map(str::to_string).ok_or_else(|| {
                SubagentDispatchError::Storage(StorageError::Corrupted {
                    detail: format!("subagent thread missing {key}"),
                })
            })
        };
        let thread = SubagentThread {
            thread_id: required("threadId")?,
            parent_session_id: required("parentSessionId")?,
            label: record["label"].as_str().map(str::to_string),
            tier: match record["tier"].as_str() {
                Some("read") => ToolAccessTier::ReadOnly,
                Some("write") => ToolAccessTier::Operate,
                _ => {
                    return Err(SubagentDispatchError::Storage(StorageError::Corrupted {
                        detail: "subagent thread invalid tier".into(),
                    }));
                }
            },
            busy: record["busy"].as_bool().ok_or_else(|| {
                SubagentDispatchError::Storage(StorageError::Corrupted {
                    detail: "subagent thread missing busy".into(),
                })
            })?,
            child_run_id: Some(required("childRunId")?),
            status: match record["streamStatus"].as_str() {
                Some("open") => ThreadStatus::Open,
                Some("closed") => ThreadStatus::Closed,
                _ => {
                    return Err(SubagentDispatchError::Storage(StorageError::Corrupted {
                        detail: "subagent thread invalid status".into(),
                    }));
                }
            },
            last_run_status: record["lastRunStatus"].as_str().map(str::to_string),
        };
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .threads
            .insert(id.into(), thread);
        Ok(record)
    }

    /// Dispatches a FRESH subagent child run (`subagent` tool): resolve
    /// the attenuated tier (explicit access > inherited parent mode),
    /// enforce the concurrency caps, allocate the child run, link it
    /// under the parent's cancellation tree and spawn the drive. Returns
    /// immediately (fire-and-forget); the result is delivered through the
    /// session's steering channel when the child settles.
    pub async fn dispatch_child(
        self: &Arc<Self>,
        parent: ParentRunFacts,
        mut request: DelegationRequest,
    ) -> Result<LaunchedChild, SubagentDispatchError> {
        self.initialize()
            .await
            .map_err(SubagentDispatchError::Storage)?;
        self.validate_parent(&parent).await?;
        if let Some(agent) = request
            .agent_id
            .as_deref()
            .filter(|id| *id != parent.agent_id)
        {
            let candidates = self.candidates(&parent.agent_id)?;
            let selected = candidates
                .iter()
                .find(|a| a["id"] == agent)
                .or_else(|| {
                    let names: Vec<_> = candidates.iter().filter(|a| a["name"] == agent).collect();
                    if names.len() == 1 {
                        Some(names[0])
                    } else {
                        None
                    }
                })
                .ok_or_else(|| SubagentDispatchError::AgentUnavailable {
                    agent: agent.into(),
                    detail: "not present or name ambiguous in the actual roster".into(),
                })?;
            request.agent_id = Some(selected["id"].as_str().unwrap().into());
        }
        if request.task.trim().is_empty() {
            return Err(SubagentDispatchError::InvalidTarget(
                "subagent dispatch requires a non-empty task".to_string(),
            ));
        }
        let mode = self.sessions.permission_mode(&parent.session_id);
        let tier = resolve_subagent_access(request.access, mode)
            .map_err(SubagentDispatchError::AccessDenied)?;
        self.spawn_child(parent, request, tier, mode, None).await
    }

    /// Continues an OPEN thread of the SAME session (`subagent_reply`
    /// tool): validates the thread, resolves the tier (explicit access >
    /// the thread's recorded tier > inherited parent mode — always
    /// re-attenuated against the CURRENT parent mode) and dispatches a
    /// continuation child run.
    pub async fn reply_child(
        self: &Arc<Self>,
        parent: ParentRunFacts,
        mut request: DelegationRequest,
    ) -> Result<LaunchedChild, SubagentDispatchError> {
        let thread_id = request
            .thread_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_default();
        let mode = self.sessions.permission_mode(&parent.session_id);
        self.validate_parent(&parent).await?;
        let record = self.restore_thread(&thread_id).await?;
        if record["ownerKind"] != parent.principal.storage_kind()
            || record["ownerSubject"] != parent.principal.storage_subject()
        {
            return Err(SubagentDispatchError::ThreadNotInSession {
                thread_id,
                session_id: parent.session_id.clone(),
            });
        }
        request.agent_id = record["agentId"].as_str().map(str::to_string);
        if request.task.trim().is_empty() {
            return Err(SubagentDispatchError::InvalidTarget(
                "subagent reply requires non-empty task".into(),
            ));
        }
        // Validate + reserve the thread under one lock.
        let thread_tier = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(thread) = state.threads.get_mut(&thread_id) else {
                return Err(SubagentDispatchError::ThreadNotFound { thread_id });
            };
            if thread.parent_session_id != parent.session_id {
                return Err(SubagentDispatchError::ThreadNotInSession {
                    thread_id,
                    session_id: parent.session_id.clone(),
                });
            }
            if thread.status != ThreadStatus::Open {
                return Err(SubagentDispatchError::ThreadNotOpen { thread_id });
            }
            if thread.busy {
                return Err(SubagentDispatchError::ThreadBusy { thread_id });
            }
            thread.tier
        };
        // Tier: explicit > the thread's recorded tier > inherit — then the
        // SAME attenuation check against the current parent mode.
        let effective_access = request.access.or(match thread_tier {
            ToolAccessTier::ReadOnly => Some(AccessRequest::Read),
            ToolAccessTier::Operate => Some(AccessRequest::Write),
        });
        let tier = resolve_subagent_access(effective_access, mode)
            .map_err(SubagentDispatchError::AccessDenied)?;
        self.spawn_child(parent, request, tier, mode, Some(thread_id))
            .await
    }

    /// Closes an OPEN, non-busy thread of the SAME session
    /// (`subagent_close` tool). No run is created; the closing reason (the
    /// delegation `task` text, when non-empty) becomes the thread's
    /// recorded summary anchor.
    pub async fn close(
        &self,
        parent: &ParentRunFacts,
        request: &DelegationRequest,
    ) -> Result<ClosedThread, SubagentDispatchError> {
        let thread_id = request
            .thread_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_default();
        self.validate_parent(parent).await?;
        let record = self.restore_thread(&thread_id).await?;
        if record["ownerKind"] != parent.principal.storage_kind()
            || record["ownerSubject"] != parent.principal.storage_subject()
        {
            return Err(SubagentDispatchError::ThreadNotInSession {
                thread_id,
                session_id: parent.session_id.clone(),
            });
        }
        {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(thread) = state.threads.get(&thread_id) else {
                return Err(SubagentDispatchError::ThreadNotFound { thread_id });
            };
            if thread.parent_session_id != parent.session_id {
                return Err(SubagentDispatchError::ThreadNotInSession {
                    thread_id,
                    session_id: parent.session_id.clone(),
                });
            }
            if thread.status != ThreadStatus::Open {
                return Err(SubagentDispatchError::ThreadNotOpen { thread_id });
            }
            if thread.busy {
                return Err(SubagentDispatchError::ThreadBusy { thread_id });
            }
        }
        thread_store::close(
            &self.storage,
            &thread_id,
            &parent.session_id,
            parent.principal.storage_kind(),
            &parent.principal.storage_subject(),
            &request.task,
        )
        .await
        .map_err(SubagentDispatchError::Storage)?;
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let thread = state
            .threads
            .get_mut(&thread_id)
            .expect("closed thread remains registered");
        thread.status = ThreadStatus::Closed;
        if !request.task.trim().is_empty() {
            thread.last_run_status = Some(format!("closed: {}", request.task.trim()));
        }
        Ok(ClosedThread { thread_id })
    }

    /// Open threads of one session (the `current_status`-style query
    /// surface for tests/evidence).
    pub fn threads_of(&self, session_id: &str) -> Vec<ThreadSnapshot> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .threads
            .values()
            .filter(|thread| thread.parent_session_id == session_id)
            .map(|thread| ThreadSnapshot {
                thread_id: thread.thread_id.clone(),
                label: thread.label.clone(),
                tier: thread.tier,
                busy: thread.busy,
                child_run_id: thread.child_run_id.clone(),
                status: thread.status,
                last_run_status: thread.last_run_status.clone(),
            })
            .collect()
    }

    /// Live concurrency counts (per session / global) for queries.
    pub fn active_counts(&self, session_id: &str) -> (usize, usize) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            state
                .active_per_session
                .get(session_id)
                .copied()
                .unwrap_or(0),
            state.active_global,
        )
    }

    /// The shared spawn path of dispatch and reply.
    async fn spawn_child(
        self: &Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
        tier: ToolAccessTier,
        parent_mode: lingxi_kernel::subagent::SessionPermissionMode,
        existing_thread: Option<String>,
    ) -> Result<LaunchedChild, SubagentDispatchError> {
        // 先核实际父运行和主体，活父只继承它自己发出的许可及取消树节点。
        let supervisor = self
            .supervisor
            .get()
            .and_then(Weak::upgrade)
            .ok_or(SubagentDispatchError::NotBound)?;
        let actual_parent = self.storage.query_one_text(
            "SELECT status FROM runs WHERE run_id=?1 AND session_id=?2 AND owner_kind=?3 AND owner_subject=?4",
            vec![parent.parent_run_id.to_string(),parent.session_id.clone(),parent.principal.storage_kind().into(),parent.principal.storage_subject()],
        ).await.map_err(SubagentDispatchError::Storage)?;
        if actual_parent.is_none() {
            return Err(SubagentDispatchError::Storage(StorageError::Conflict {
                detail: "subagent_parent_ownership_mismatch".into(),
            }));
        }
        let gate = self.storage.session_admission();
        let inherited = supervisor
            .task_supervisor()
            .parent_run_permit(
                parent.parent_run_id.as_str(),
                &parent.session_id,
                &parent.agent_id,
                &parent.principal,
                gate,
            )
            .map_err(SubagentDispatchError::Storage)?;
        let parent_permit = match inherited {
            Some(permit) => {
                if !supervisor
                    .run_scope(parent.parent_run_id.as_str())
                    .is_some_and(|scope| scope.same_scope(&parent.parent_scope))
                {
                    return Err(SubagentDispatchError::Storage(StorageError::Conflict {
                        detail: "subagent_parent_scope_mismatch".into(),
                    }));
                }
                permit
            }
            None => {
                if !matches!(
                    actual_parent.as_deref(),
                    Some("completed" | "failed" | "cancelled" | "interrupted")
                ) {
                    return Err(SubagentDispatchError::Storage(StorageError::Conflict {
                        detail: "subagent_parent_not_settled_or_admitted".into(),
                    }));
                }
                Arc::new(
                    gate.try_acquire(&[&parent.session_id], usize::MAX)
                        .map_err(|_| {
                            SubagentDispatchError::Storage(StorageError::Conflict {
                                detail: "session_busy".into(),
                            })
                        })?,
                )
            }
        };
        // 1) Concurrency caps + thread reservation (one lock; rolled back
        //    on every failure path below).
        let session_id_for_rollback = parent.session_id.clone();
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let per_session = state
                .active_per_session
                .get(&parent.session_id)
                .copied()
                .unwrap_or(0);
            if per_session >= self.policy.per_session_limit {
                return Err(SubagentDispatchError::SessionLimit {
                    max: self.policy.per_session_limit,
                });
            }
            if state.active_global >= self.policy.global_limit {
                return Err(SubagentDispatchError::GlobalLimit {
                    max: self.policy.global_limit,
                });
            }
            // Closed threads are evicted at the registry cap before a
            // refusal (open threads are live facts).
            if existing_thread.is_none() && state.threads.len() >= self.policy.thread_registry_cap {
                state
                    .threads
                    .retain(|_, thread| thread.status == ThreadStatus::Open || thread.busy);
                if state.threads.len() >= self.policy.thread_registry_cap {
                    return Err(SubagentDispatchError::ThreadRegistryFull {
                        cap: self.policy.thread_registry_cap,
                    });
                }
            }
            state
                .active_per_session
                .insert(parent.session_id.clone(), per_session + 1);
            state.active_global += 1;
        }
        let rollback_caps = |runtime: &Self| {
            let mut state = runtime
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(count) = state.active_per_session.get_mut(&session_id_for_rollback) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    state.active_per_session.remove(&session_id_for_rollback);
                }
            }
            state.active_global = state.active_global.saturating_sub(1);
        };

        // 2) The child run id (the storage allocator is the only
        //    legitimate source of run ids).
        let child_run_id = match self.storage.allocate_run_id(parent.now_ms) {
            Ok(id) => id,
            Err(err) => {
                rollback_caps(self);
                return Err(SubagentDispatchError::Storage(err));
            }
        };
        let thread_id = existing_thread
            .clone()
            .unwrap_or_else(|| child_run_id.clone());

        let agent_identity = request
            .agent_id
            .clone()
            .unwrap_or_else(|| parent.agent_id.clone());
        let reserved = json!({"threadId":thread_id,"parentSessionId":parent.session_id,
            "ownerKind":parent.principal.storage_kind(),"ownerSubject":parent.principal.storage_subject(),
            "agentId":agent_identity,"label":request.label,"tier":tier.wire_name(),
            "childRunId":child_run_id,"streamStatus":"open","busy":true,
            "lastRunStatus":null,"result":null,"error":null});
        let previous = match thread_store::reserve(
            &self.storage,
            reserved,
            parent.parent_run_id.as_str(),
            &request.task,
            existing_thread.is_some(),
        )
        .await
        {
            Ok(previous) => previous,
            Err(error) => {
                rollback_caps(self);
                return Err(SubagentDispatchError::Storage(error));
            }
        };

        // 3) Thread record (new or continued).
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match state.threads.get_mut(&thread_id) {
                Some(thread) => {
                    thread.busy = true;
                    thread.tier = tier;
                    thread.child_run_id = Some(child_run_id.clone());
                }
                None => {
                    state.threads.insert(
                        thread_id.clone(),
                        SubagentThread {
                            thread_id: thread_id.clone(),
                            parent_session_id: parent.session_id.clone(),
                            label: request.label.clone(),
                            tier,
                            busy: true,
                            child_run_id: Some(child_run_id.clone()),
                            status: ThreadStatus::Open,
                            last_run_status: None,
                        },
                    );
                }
            }
        }

        // 4) The child lineage: parentRunId / origin=subagent /
        //    sourceMessageId / causeId — durably recorded by the child's
        //    own drive right after its run row is created.
        let lineage = RunLineage {
            parent_run_id: Some(parent.parent_run_id.clone()),
            origin: RunOrigin::Subagent,
            source_message_id: Some(parent.source_model_call.to_string()),
            cause_id: Some(parent.cause_tool_call.to_string()),
        };

        // 5) The closeout guard (R03 repair G01/F02): from HERE to the
        //    end of the child's lifetime, exactly one completion
        //    accounting exists — the normal tail disarms it; every other
        //    ending (drop at a cancellation last resort, panic, never
        //    first-polled) closes out abnormally through Drop.
        let closeout = ChildCloseout::new(
            Arc::clone(self),
            session_id_for_rollback.clone(),
            thread_id.clone(),
            child_run_id.clone(),
        );
        // A clone travels into the child future; this frame's clone covers
        // the pre-spawn refusal branch below (both share ONE done flag).
        let closeout_for_spawn = Arc::clone(&closeout);

        // 6) The supervisor (bound by the composition root).
        let supervisor = match self.supervisor.get().and_then(Weak::upgrade) {
            Some(supervisor) => supervisor,
            None => {
                closeout.disarm();
                rollback_caps(self);
                thread_store::rollback(&self.storage, &thread_id, &child_run_id, previous)
                    .await
                    .map_err(SubagentDispatchError::Storage)?;
                self.rollback_thread(&thread_id, &existing_thread);
                return Err(SubagentDispatchError::NotBound);
            }
        };

        // 6) The child drive: linked to the parent's cancellation tree,
        //    driven through the SAME supervisor chain, bounded by the
        //    child timeout (anchored HERE — the actual start), and its
        //    result delivered into the session's steering channel.
        let child_scope = parent.parent_scope.child(
            format!("child_run:{child_run_id}"),
            crate::cancel::ScopeKind::ChildRun,
        );
        // R04-T03: the child's drive authorization snapshots the PARENT
        // session mode it inherited from — the {tier, parent_mode} pair
        // is what the tool-policy plane needs to reproduce the
        // incumbent's ask-tier deny_on_prompt semantics (R04-SUP-01).
        let grant = DriveAuthorization {
            knowledge_selection: None,
            lineage,
            grant: RunGrant::Subagent { tier },
            session_mode: parent_mode,
        };
        let storage = Arc::clone(&self.storage);
        let events = Arc::clone(&self.events);
        let principal = parent.principal.clone();
        let session_id = parent.session_id.clone();
        // R06-T08 fix (the behavior evaluation surfaced this): the value
        // passed to `drive_run.agent_id` is the AGENT IDENTITY — the T01
        // context compiler resolves the agent's persona/memory materials
        // with it (and the tool gateway + usage ledger attribute the
        // child's invocations to it), so it MUST be the real serving
        // agent (`request.agent_id` falling back to the dispatching
        // parent's agent). A child run of agent "lingxi" compiles the
        // same trimmed subagent context the incumbent serves — never a
        // loud compile failure for a nonexistent "subagent:<id>" agent.
        // The child's CONCURRENCY isolation is carried by the lane
        // parameters, not by the identity: `quota_session_lane` stays
        // unique per subagent thread (the incumbent's isolated-session
        // semantics — a parked parent holding the session's model lane
        // can never starve its child), and the per-thread session lane
        // plus the global/process caps bound total subagent concurrency.
        let quota_session_lane = format!(
            "{}::subagent::{}::{}",
            parent.session_id, agent_identity, thread_id
        );
        let task_input = request.task.clone();
        let now_ms = parent.now_ms;
        let child_run = child_run_id.clone();
        let timeout = Duration::from_millis(self.policy.timeout_ms);
        let drive_supervisor = Arc::clone(&supervisor);
        let drive_scope = child_scope.clone();
        let spawn_result = supervisor.task_supervisor().spawn_linked(
            parent.parent_run_id.as_str(),
            &child_scope,
            format!("child_run:{child_run_id}"),
            async move {
                let _run_permit = drive_supervisor
                    .task_supervisor()
                    .retain_authorized_run_permit(
                        &child_run,
                        &session_id,
                        &agent_identity,
                        &principal,
                        &grant,
                        Arc::clone(&parent_permit),
                    );
                // 绑定失败不启动业务，也不把另一run的登记覆盖。
                if let Err(error) = &_run_permit {
                    closeout_for_spawn
                        .complete_and_deliver(&Err(DriveError::Storage(error.clone())))
                        .await;
                    return;
                }
                let _parent_permit = parent_permit;
                let closeout = closeout_for_spawn;
                let drive = drive_supervisor.drive_run(
                    storage.as_ref(),
                    events.as_ref(),
                    &principal,
                    &session_id,
                    &agent_identity,
                    &child_run,
                    &task_input,
                    1,
                    now_ms,
                    None,
                    Some(&drive_scope),
                    grant,
                    // Child runs carry no submission requestId — there is
                    // no id→run binding to commit (R03 repair G04/F05).
                    None,
                    &quota_session_lane,
                );
                // Timeout anchored at the child's ACTUAL start (the
                // incumbent's timer placement). On expiry the child
                // scope fires and the drive settles `cancelled`
                // through its OWN four-phase path — the future is not
                // dropped mid-flight.
                let mut drive = Box::pin(CatchChild {
                    future: Box::pin(drive),
                });
                let finish = match tokio::time::timeout_at(
                    tokio::time::Instant::now() + timeout,
                    drive.as_mut(),
                )
                .await
                {
                    Ok(Ok(result)) => result,
                    Ok(Err(reason)) => {
                        Err(DriveError::Internal(format!("subagent panic: {reason}")))
                    }
                    Err(_elapsed) => {
                        drive_scope.cancel("subagent timeout");
                        match drive.as_mut().await {
                            Ok(result) => result,
                            Err(reason) => {
                                Err(DriveError::Internal(format!("subagent panic: {reason}")))
                            }
                        }
                    }
                };
                drop(drive);
                // Delivery + bookkeeping (every finish path). The closeout
                // guard is disarmed by `complete`; a panic or drop before
                // this point closes out abnormally instead (exactly-once
                // either way).
                closeout.complete_and_deliver(&finish).await;
            },
        );
        match spawn_result {
            Ok(handle) => {
                // Fire-and-forget: the child's exit result stays
                // recoverable in the supervised registry (bounded; reaped
                // at pressure) — and its cancellation path runs through
                // the linked scope + the run's own drive.
                drop(handle);
            }
            Err(rejected) => {
                closeout.disarm();
                rollback_caps(self);
                thread_store::rollback(&self.storage, &thread_id, &child_run_id, previous)
                    .await
                    .map_err(SubagentDispatchError::Storage)?;
                self.rollback_thread(&thread_id, &existing_thread);
                if existing_thread.is_some() {
                    self.restore_thread(&thread_id).await?;
                }
                return Err(SubagentDispatchError::SpawnRefused { cap: rejected.cap });
            }
        }
        // 已成功派发后，状态读取失败不能谎报“零子任务”；明确保留已接受身份与观测失败。
        let (status, progress_error) = match self
            .storage
            .query_one_text(
                "SELECT status FROM runs WHERE run_id=?1",
                vec![child_run_id.clone()],
            )
            .await
        {
            Ok(Some(status)) => (status, None),
            Ok(None) => ("queued".into(), None),
            Err(error) => ("unknown".into(), Some(error.to_string())),
        };
        Ok(LaunchedChild {
            child_run_id,
            thread_id,
            status,
            progress_error,
        })
    }

    fn rollback_thread(&self, thread_id: &str, existing: &Option<String>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if existing.is_some() {
            if let Some(thread) = state.threads.get_mut(thread_id) {
                thread.busy = false;
            }
        } else {
            state.threads.remove(thread_id);
        }
    }

    /// The child task's completion bookkeeping: caps decrement, thread
    /// busy=false + last status, thread stays OPEN for continuation
    /// (`finishRun {close: false}`).
    ///
    /// R03 repair G01/F02 hardening:
    /// - **Exactly-once**: a completion is accounted ONCE per child run id
    ///   (the bounded `accounted_children` ring) — a duplicated or late
    ///   re-delivery is a logged no-op, never a second cap decrement.
    /// - **Identity fence**: only the thread's CURRENT child run may clear
    ///   its busy flag and record its status — a late completion carrying
    ///   a SUPERSEDED child_run_id can never clear the busy of a newer
    ///   run on the same thread.
    pub fn note_child_finished(
        &self,
        session_id: &str,
        thread_id: &str,
        child_run_id: &str,
        finish: &Result<RunFinish, DriveError>,
    ) {
        let status_word = match finish {
            Ok(finish) => finish.status().wire_name().to_string(),
            Err(_) => "failed".to_string(),
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state
            .accounted_children
            .iter()
            .any(|accounted| accounted == child_run_id)
        {
            tracing::warn!(
                session_id = %session_id,
                thread_id = %thread_id,
                child_run_id = %child_run_id,
                "duplicate child completion callback ignored (already accounted — exactly-once \
                 bookkeeping)"
            );
            return;
        }
        if state.accounted_children.len() >= ACCOUNTED_CHILDREN_CAP {
            state.accounted_children.pop_front();
        }
        state.accounted_children.push_back(child_run_id.to_string());
        if let Some(count) = state.active_per_session.get_mut(session_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                state.active_per_session.remove(session_id);
            }
        }
        state.active_global = state.active_global.saturating_sub(1);
        if let Some(thread) = state.threads.get_mut(thread_id) {
            if thread.child_run_id.as_deref() == Some(child_run_id) {
                thread.busy = false;
                thread.last_run_status = Some(status_word);
            } else {
                tracing::warn!(
                    session_id = %session_id,
                    thread_id = %thread_id,
                    stale_child_run_id = %child_run_id,
                    current_child_run_id = ?thread.child_run_id,
                    "late completion for a SUPERSEDED child run: caps returned, the current \
                     run's busy is untouched (identity fence)"
                );
            }
        }
    }
}

/// The exactly-once closeout guard of one dispatched child run (R03
/// repair G01/F02): created in `spawn_child` BEFORE the supervised spawn
/// (so it exists even if the child future is never first-polled) and
/// moved INTO the child future, which disarms it on its own completion
/// tail. EVERY other ending — the future dropped at a cancellation last
/// resort, a panic anywhere in the drive or the delivery tail, an abrupt
/// task teardown — runs the abnormal closeout through `Drop`: caps
/// decrement, busy cleared, an honest `failed` status recorded. Double
/// firing is impossible (the `done` flag) and the runtime-side
/// `note_child_finished` is itself exactly-once per child run id.
struct ChildCloseout {
    runtime: Arc<SubagentRuntime>,
    session_id: String,
    thread_id: String,
    child_run_id: String,
    done: std::sync::atomic::AtomicBool,
}

impl ChildCloseout {
    fn new(
        runtime: Arc<SubagentRuntime>,
        session_id: String,
        thread_id: String,
        child_run_id: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            session_id,
            thread_id,
            child_run_id,
            done: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn done(&self) -> bool {
        self.done.load(std::sync::atomic::Ordering::Acquire)
    }

    fn mark_done(&self) {
        self.done.store(true, std::sync::atomic::Ordering::Release);
    }

    /// The normal completion tail (the drive delivered a finish): disarm
    /// the drop path and run the bookkeeping with the REAL outcome.
    fn complete(&self, finish: &Result<RunFinish, DriveError>) {
        if self.done() {
            return;
        }
        self.mark_done();
        self.runtime.note_child_finished(
            &self.session_id,
            &self.thread_id,
            &self.child_run_id,
            finish,
        );
    }

    async fn complete_and_deliver(&self, finish: &Result<RunFinish, DriveError>) {
        let status = match finish {
            Ok(f) => f.status().wire_name(),
            Err(_) => "failed",
        };
        let prior = thread_store::read_thread(&self.runtime.storage, &self.thread_id).await;
        let error = match finish {
            Ok(RunFinish::Failed { cause }) => Some(
                prior
                    .ok()
                    .flatten()
                    .and_then(|r| r["error"].as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("{cause:?}")),
            ),
            Err(e) => Some(format!("{e:?}")),
            _ => None,
        };
        let text = match error.as_deref() {
            Some(error) => format!(
                "[subagent-result thread={} run={}] failed: {error}",
                self.thread_id, self.child_run_id
            ),
            None => delivery_text(&self.thread_id, &self.child_run_id, finish),
        };
        match thread_store::finish(
            &self.runtime.storage,
            &self.thread_id,
            &self.child_run_id,
            status,
            &text,
            error.as_deref(),
        )
        .await
        {
            Ok(()) => {
                self.complete(finish);
                self.deliver(&text);
            }
            Err(error) => {
                let failed = Err(DriveError::Storage(error));
                self.complete(&failed);
                self.deliver(&delivery_text(&self.thread_id, &self.child_run_id, &failed));
            }
        }
    }

    fn deliver(&self, text: &str) {
        if let Err(error) = self
            .runtime
            .sessions
            .deliver_retained(&self.session_id, text)
        {
            tracing::error!(session_id=%self.session_id,thread_id=%self.thread_id,run_id=%self.child_run_id,error=%error,"subagent result delivery refused; durable result retained");
        }
    }

    /// Disarm WITHOUT bookkeeping — exclusively for the pre-spawn refusal
    /// paths whose explicit rollbacks (caps + thread record) already ran.
    fn disarm(&self) {
        self.mark_done();
    }
}

impl Drop for ChildCloseout {
    fn drop(&mut self) {
        if !self.done() {
            self.mark_done();
            tracing::error!(
                session_id = %self.session_id,
                thread_id = %self.thread_id,
                child_run_id = %self.child_run_id,
                "subagent child ended WITHOUT delivering a result (dropped at a cancellation \
                 last resort, panicked, or never polled) — abnormal closeout: caps returned, \
                 busy cleared, honest failed status recorded"
            );
            let reason =
                "child run ended without delivering a result (dropped before its completion tail)"
                    .to_string();
            let finish = Err(DriveError::Internal(reason.clone()));
            self.runtime.note_child_finished(
                &self.session_id,
                &self.thread_id,
                &self.child_run_id,
                &finish,
            );
            self.runtime
                .pending_closeouts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((self.thread_id.clone(), self.child_run_id.clone(), reason));
            self.deliver(&delivery_text(&self.thread_id, &self.child_run_id, &finish));
        }
    }
}

async fn flush_abnormal_closeouts(
    storage: &RunDatabase,
    pending: &Mutex<Vec<(String, String, String)>>,
) -> Result<(), StorageError> {
    let entries = std::mem::take(&mut *pending.lock().unwrap_or_else(|p| p.into_inner()));
    let mut entries = entries.into_iter();
    while let Some((thread, run, reason)) = entries.next() {
        if let Err(error) =
            thread_store::finish(storage, &thread, &run, "failed", &reason, Some(&reason)).await
        {
            let mut remaining = pending.lock().unwrap_or_else(|p| p.into_inner());
            remaining.push((thread, run, reason));
            remaining.extend(entries);
            return Err(error);
        }
    }
    Ok(())
}

// 把真实panic原因带进同一完成尾段；不交给外层吞掉为泛化失败。
struct CatchChild<F: Future> {
    future: Pin<Box<F>>,
}
impl<F: Future> Future for CatchChild<F> {
    type Output = Result<F::Output, String>;
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.get_mut().future.as_mut().poll(cx)
        })) {
            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
            Ok(std::task::Poll::Ready(value)) => std::task::Poll::Ready(Ok(value)),
            Err(payload) => std::task::Poll::Ready(Err(payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "opaque panic payload".into()))),
        }
    }
}

/// The delivery text for a finished child (bounded summary — the
/// incumbent truncates block_update summaries to 200 chars).
fn delivery_text(thread_id: &str, run_id: &str, finish: &Result<RunFinish, DriveError>) -> String {
    match finish {
        Ok(finish) => {
            let reason = finish.terminal_reason().to_string();
            let final_text: String = finish
                .final_message()
                .map(|message| {
                    message
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            lingxi_protocol::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let summary = if final_text.trim().is_empty() {
                reason
            } else {
                final_text
            };
            let bounded: String = summary.chars().take(200).collect();
            format!("[subagent-result thread={thread_id} run={run_id}] {bounded}")
        }
        Err(err) => format!("[subagent-result thread={thread_id} run={run_id}] failed: {err:?}"),
    }
}
