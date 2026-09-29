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
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use lingxi_kernel::ports::{DelegationRequest, StorageError};
use lingxi_kernel::subagent::{
    resolve_subagent_access, AccessRequest, RunLineage, RunOrigin, SubagentAccessDenied,
    SubagentPolicy, ToolAccessTier,
};
use lingxi_protocol::{ErrorCode, ModelCallId, ProtocolError, RunId, ToolCallId};

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
        &self,
        parent: &ParentRunFacts,
        request: &DelegationRequest,
    ) -> Result<ClosedThread, SubagentDispatchError>;
}

impl SubagentLauncher for SubagentRuntime {
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
        &self,
        parent: &ParentRunFacts,
        request: &DelegationRequest,
    ) -> Result<ClosedThread, SubagentDispatchError> {
        SubagentRuntime::close(self, parent, request)
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
}

/// Identity of one successfully closed thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedThread {
    pub thread_id: String,
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

    /// Dispatches a FRESH subagent child run (`subagent` tool): resolve
    /// the attenuated tier (explicit access > inherited parent mode),
    /// enforce the concurrency caps, allocate the child run, link it
    /// under the parent's cancellation tree and spawn the drive. Returns
    /// immediately (fire-and-forget); the result is delivered through the
    /// session's steering channel when the child settles.
    pub async fn dispatch_child(
        self: &Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
    ) -> Result<LaunchedChild, SubagentDispatchError> {
        if request.task.trim().is_empty() {
            return Err(SubagentDispatchError::InvalidTarget(
                "subagent dispatch requires a non-empty task".to_string(),
            ));
        }
        let mode = self.sessions.permission_mode(&parent.session_id);
        let tier = resolve_subagent_access(request.access, mode)
            .map_err(SubagentDispatchError::AccessDenied)?;
        self.spawn_child(parent, request, tier, None).await
    }

    /// Continues an OPEN thread of the SAME session (`subagent_reply`
    /// tool): validates the thread, resolves the tier (explicit access >
    /// the thread's recorded tier > inherited parent mode — always
    /// re-attenuated against the CURRENT parent mode) and dispatches a
    /// continuation child run.
    pub async fn reply_child(
        self: &Arc<Self>,
        parent: ParentRunFacts,
        request: DelegationRequest,
    ) -> Result<LaunchedChild, SubagentDispatchError> {
        let thread_id = request
            .thread_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_default();
        let mode = self.sessions.permission_mode(&parent.session_id);
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
        self.spawn_child(parent, request, tier, Some(thread_id))
            .await
    }

    /// Closes an OPEN, non-busy thread of the SAME session
    /// (`subagent_close` tool). No run is created; the closing reason (the
    /// delegation `task` text, when non-empty) becomes the thread's
    /// recorded summary anchor.
    pub fn close(
        &self,
        parent: &ParentRunFacts,
        request: &DelegationRequest,
    ) -> Result<ClosedThread, SubagentDispatchError> {
        let thread_id = request
            .thread_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_default();
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
        existing_thread: Option<String>,
    ) -> Result<LaunchedChild, SubagentDispatchError> {
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
        let grant = DriveAuthorization {
            lineage,
            grant: RunGrant::Subagent { tier },
        };
        let storage = Arc::clone(&self.storage);
        let events = Arc::clone(&self.events);
        let sessions = Arc::clone(&self.sessions);
        let principal = parent.principal.clone();
        let session_id = parent.session_id.clone();
        // The child's concurrency lanes are its OWN (the incumbent's
        // isolated-session semantics): one agent lane and one session
        // lane per subagent thread, so a parked parent holding the
        // session's model lane can never starve its child (and vice
        // versa); total subagent concurrency stays bounded by the
        // runtime's per-session/global caps on top.
        let agent_lane = format!(
            "subagent:{}",
            request
                .agent_id
                .clone()
                .unwrap_or_else(|| parent.agent_id.clone())
        );
        let quota_session_lane = format!("{}::subagent::{}", parent.session_id, thread_id);
        let task_input = request.task.clone();
        let now_ms = parent.now_ms;
        let child_run = child_run_id.clone();
        let thread_delivery = thread_id.clone();
        let timeout = Duration::from_millis(self.policy.timeout_ms);
        let drive_supervisor = Arc::clone(&supervisor);
        let drive_scope = child_scope.clone();
        let spawn_result = supervisor.task_supervisor().spawn_linked(
            parent.parent_run_id.as_str(),
            &child_scope,
            format!("child_run:{child_run_id}"),
            async move {
                let closeout = closeout_for_spawn;
                let drive = drive_supervisor.drive_run(
                    storage.as_ref(),
                    events.as_ref(),
                    &principal,
                    &session_id,
                    &agent_lane,
                    &child_run,
                    &task_input,
                    1,
                    now_ms,
                    None,
                    Some(&drive_scope),
                    grant,
                    &quota_session_lane,
                );
                // Timeout anchored at the child's ACTUAL start (the
                // incumbent's timer placement). On expiry the child
                // scope fires and the drive settles `cancelled`
                // through its OWN four-phase path — the future is not
                // dropped mid-flight.
                let mut drive = Box::pin(drive);
                let finish = match tokio::time::timeout_at(
                    tokio::time::Instant::now() + timeout,
                    drive.as_mut(),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        drive_scope.cancel("subagent timeout");
                        drive.await
                    }
                };
                // Delivery + bookkeeping (every finish path). The closeout
                // guard is disarmed by `complete`; a panic or drop before
                // this point closes out abnormally instead (exactly-once
                // either way).
                closeout.complete(&finish);
                if let Err(err) = sessions.deliver_retained(
                    &session_id,
                    &delivery_text(&thread_delivery, &child_run, &finish),
                ) {
                    // Loud, never silent: the run's own durable events
                    // remain the authoritative record; the thread
                    // snapshot keeps the outcome queryable.
                    tracing::error!(
                        session_id = %session_id,
                        thread_id = %thread_delivery,
                        run_id = %child_run,
                        error = %err,
                        "subagent result delivery into the steering channel was refused \
                         (bounded inbox full); the outcome stays queryable through the \
                         run events and the thread snapshot"
                    );
                }
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
                self.rollback_thread(&thread_id, &existing_thread);
                return Err(SubagentDispatchError::SpawnRefused { cap: rejected.cap });
            }
        }
        Ok(LaunchedChild {
            child_run_id,
            thread_id,
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
            self.runtime.note_child_finished(
                &self.session_id,
                &self.thread_id,
                &self.child_run_id,
                &Err(DriveError::Internal(
                    "child run ended without delivering a result (dropped or panicked before \
                     its completion tail)"
                        .to_string(),
                )),
            );
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
