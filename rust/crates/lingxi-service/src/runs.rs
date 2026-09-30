//! Run lifecycle supervision (R03-T01): the real run chain
//! queued → running → (model turns / tool turns / retries) → ONE finalize.
//!
//! This module is the "RunSupervisor" owner of target-contract §3/§4 ("一次
//! 用户任务及终态 | RunSupervisor＋运行事务"): it drives one user task
//! through the kernel state machine, the storage port's single finalize
//! transaction and the event service's post-commit publication.
//!
//! Identity layers (R03-T01 step 2 — a user task and a model request are
//! never conflated):
//! - `SessionId`: the conversation the run belongs to.
//! - `RunId`: fixed when the task is created (the storage allocator mints
//!   it); a provider reconnect NEVER mints a new run.
//! - `AttemptId`: `{run}#a{n}`, incremented per retry on the SAME run.
//! - `ModelCallId` / `ToolCallId`: minted here per call, independent of the
//!   run/attempt identity. One run typically has several model calls; a
//!   model call or tool call ending is NOT the run ending.
//!
//! Test-double boundary: [`TurnProviderPort`] / [`ToolExecutorPort`]
//! implementations only produce external responses; every state decision
//! (which turn ends the run, the outcome contract, the single finalize) is
//! made HERE against the kernel state machine, and every durable fact is
//! written through [`StoragePort`]. Doubles never write state and never
//! finalize a run.
//!
//! R03-T04 result fence (this stage): every asynchronous return carries a
//! [`ResultFence`](lingxi_kernel::ports::ResultFence) (run/attempt/
//! generation) and the driver verifies it BEFORE writing state — not only
//! when the request was issued. A result naming a superseded attempt,
//! another run/generation, or one whose cancellation was observed first, is
//! refused for state purposes and recorded as an audit-only stale fact
//! (`record_stale_result`); old results never pollute the next attempt,
//! the next run or a settled session.
//!
//! R03-T05 invocation journal (this stage): every tool call is journaled
//! as a receipt bound to the owner facts, run/attempt/generation, target,
//! argument digest and idempotency key. The write ORDER is the contract:
//! the intent (`prepared` → `authorized` → `started`) is durable BEFORE
//! the external execution is dispatched; the receipt (external response /
//! dedup identifier) lands AFTER it. A crash between the two leaves the
//! entry at `started` with no receipt — the recovery classification
//! (R03-T05) reads exactly that as UNKNOWN and never lets a non-idempotent
//! side effect be blindly retried.

use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::ports::{
    CommittedOutcome, InvocationIntent, InvocationPhase, InvocationReceipt, KeyEvent,
    LateResultReason, ReceiptOutcome, ResultFence, StaleResultFact, StorageError, StoragePort,
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::subagent::{
    authorize_child_tool, RunLineage, ToolAccessTier, ToolAuthorization,
};
use lingxi_kernel::{
    attempt_id, model_call_id, tool_call_id, FailureCause, NoFinalCause, QuotaResource, RunFinish,
    RunStateMachine,
};
use lingxi_protocol::{
    ErrorCode, EventId, EventPayload, KnownEventPayload, ModelCallCompletedPayload, ModelCallId,
    ModelCallStartedPayload, ProtocolError, RunId, RunStateChangedPayload, RunStatus,
    ToolCallCompletedPayload, ToolCallDescriptor, ToolCallStartedPayload, ToolResultStatus,
    ToolResultWire,
};

use crate::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest};
use crate::cancel::{
    CancelBudget, CancelPhase, CancelPolicy, CancelRegistry, CancelScope, FireOutcome,
    RunCancelEntry, TerminalAdjudication,
};
use crate::events::EventService;
use crate::quotas::QuotaManager;
use crate::session_supervisor::SteeringInbox;
use crate::task_supervisor::{TaskExit, TaskSupervisor};

/// Hard bounds of one driven run (R03-T01; injected through `ServiceDeps`).
/// Both bounds are loud: hitting `max_model_turns` FAILS the run with
/// `failed.turn_budget_exceeded` — a budget limit is never silently
/// absorbed into a "completed" outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunDriveLimits {
    /// Maximum model turns one run may consume (>= 1).
    pub max_model_turns: u32,
    /// Maximum attempts (initial + retries) one run may open (>= 1).
    pub max_attempts: u32,
}

impl RunDriveLimits {
    pub const DEFAULT_MAX_MODEL_TURNS: u32 = 32;
    pub const DEFAULT_MAX_ATTEMPTS: u32 = 2;
    /// Hard upper bounds so a degenerate config cannot turn the loop
    /// unbounded through the injection surface.
    pub const ABSOLUTE_MAX_MODEL_TURNS: u32 = 256;
    pub const ABSOLUTE_MAX_ATTEMPTS: u32 = 8;

    pub fn validate(&self) -> Result<(), StorageError> {
        let invalid = |what: &str, value: u32, lo: u32, hi: u32| StorageError::InvalidRequest {
            detail: format!("{what} must be in {lo}..={hi}, got {value}"),
        };
        if self.max_model_turns == 0 || self.max_model_turns > Self::ABSOLUTE_MAX_MODEL_TURNS {
            return Err(invalid(
                "max_model_turns",
                self.max_model_turns,
                1,
                Self::ABSOLUTE_MAX_MODEL_TURNS,
            ));
        }
        if self.max_attempts == 0 || self.max_attempts > Self::ABSOLUTE_MAX_ATTEMPTS {
            return Err(invalid(
                "max_attempts",
                self.max_attempts,
                1,
                Self::ABSOLUTE_MAX_ATTEMPTS,
            ));
        }
        Ok(())
    }
}

impl Default for RunDriveLimits {
    fn default() -> Self {
        Self {
            max_model_turns: Self::DEFAULT_MAX_MODEL_TURNS,
            max_attempts: Self::DEFAULT_MAX_ATTEMPTS,
        }
    }
}

/// The R03-T06 drive authorization: WHAT this run's tool calls may do,
/// plus the run's four-part lineage identity. Set at submission/dispatch
/// time by the REAL surfaces (the session execute path for user runs, the
/// subagent runtime for child runs) and bound to the run for its whole
/// lifetime — neither a model swap nor an executor swap can widen it.
#[derive(Debug, Clone, PartialEq)]
pub struct DriveAuthorization {
    /// parentRunId / origin / sourceMessageId / causeId — durably recorded
    /// right after the run row is created.
    pub lineage: RunLineage,
    pub grant: RunGrant,
}

/// The grant shape of one driven run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunGrant {
    /// A user-submitted run: the run-layer authorization context itself
    /// authorizes every target (quota-admitted, authenticated — the T05
    /// semantics; R04's policy gateway replaces this decision point).
    Full,
    /// A subagent child run: EVERY tool target is authorized against the
    /// ATTENUATED tier (parent grant ∩ subagent tool scope — the real
    /// authorization boundary of R03-A11). The tier is fixed at dispatch;
    /// the blocklist applies regardless of tier.
    Subagent { tier: ToolAccessTier },
}

impl DriveAuthorization {
    /// The authorization of a plain user submission (origin=user; the
    /// cause anchor is the explicit requestId when present).
    ///
    /// R03 RR2/F05-01 contract: `request_id` must be the CANONICAL id —
    /// the two submission surfaces canonicalize ONCE at their boundary
    /// ([`crate::sessions`]) and hand the same fact to the dedup key, this
    /// authorization and the cross-restart lookup. The debug fence below
    /// turns a raw/padded id slipping in through any other caller into an
    /// immediate dev/test failure instead of a durable-anchor divergence
    /// (the cause_id must be built from the same fact as every other
    /// identity consumer).
    pub fn user_submission(request_id: Option<&str>) -> Self {
        if let Some(id) = request_id {
            debug_assert!(
                matches!(crate::dedup::validate_request_id(id).as_deref(), Ok(canonical) if canonical == id),
                "DriveAuthorization::user_submission received a NON-CANONICAL request id {id:?} \
                 — canonicalize at the admission boundary; the lineage anchor must share the \
                 same identity fact as the dedup key"
            );
        }
        Self {
            lineage: RunLineage::user_submission(request_id),
            grant: RunGrant::Full,
        }
    }
}

/// Failure of the run drive (everything here is loud; none is a silent
/// degradation of the run's outcome).
#[derive(Debug, Clone, PartialEq)]
pub enum DriveError {
    /// The storage port refused or failed; no visible success was produced.
    Storage(StorageError),
    /// Driver-internal invariant violation (impossible in a healthy build;
    /// surfaced as an internal storage error by the callers).
    Internal(String),
}

impl From<StorageError> for DriveError {
    fn from(err: StorageError) -> Self {
        DriveError::Storage(err)
    }
}

/// The R03-T04 result fence: the write-side verdict for every asynchronous
/// provider/tool return. `Current` admits the result into the state
/// machine; `Stale` refuses it for state purposes (audit-only) — a late
/// result never pollutes the current attempt, a later run or a settled
/// session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FenceVerdict {
    Current,
    Stale(LateResultReason),
}

/// RAII registration of one live run against the cancellation registry
/// (R03-T03): covers EVERY exit path of the drive. `disarm()` is called
/// exactly when the single finalize committed (or the no-provider early
/// finalize returned) — every other exit (storage error, dropped driving
/// future, panic unwind) fires the cancellation TREE so linked children
/// never outlive an abandoned run, marks the phase `Abandoned` and leaves
/// the durable run row honest (active; recovery classification R03-T07).
struct RegistrationGuard<'a> {
    registry: &'a CancelRegistry,
    run_id: RunId,
    entry: Arc<RunCancelEntry>,
    disarmed: bool,
}

impl RegistrationGuard<'_> {
    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for RegistrationGuard<'_> {
    fn drop(&mut self) {
        if !self.disarmed {
            // The driving future is going away WITHOUT a finalize: cancel
            // the tree (linked children drop at their await points) and
            // record the honest abandoned phase. The durable run row keeps
            // its last durable state — nothing fabricates a terminal.
            self.entry.scope.cancel("driver exited without finalize");
            self.entry.advance_phase(CancelPhase::Abandoned {
                reason: "driver exited without a finalize (dropped request, error or \
                         crash); linked children were cancelled through the tree; the \
                         durable run row stays active — recovery classification is \
                         R03-T07"
                    .to_string(),
            });
            tracing::warn!(
                run_id = %self.run_id,
                "run driver exited without a finalize; cancellation tree fired, \
                 registry entry removed, durable row left honest (R03-T07 owns recovery)"
            );
        }
        self.registry.deregister(self.run_id.as_str());
    }
}

/// The supervisor: injected provider/tool doubles plus the drive bounds,
/// the admission quotas and the R03-T03 cancellation runtime (the run
/// registry of the cancellation tree, the task supervisor and the cleanup
/// deadline policy). Stateless per run — all per-run state lives in
/// [`Drive`] locals and the registry entry, so concurrent executes share
/// one supervisor safely.
pub struct RunSupervisor {
    provider: Option<Arc<dyn TurnProviderPort>>,
    tools: Option<Arc<dyn ToolExecutorPort>>,
    limits: RunDriveLimits,
    quotas: QuotaManager,
    cancel: CancelRegistry,
    tasks: Arc<TaskSupervisor>,
    cancel_policy: CancelPolicy,
    approval: Option<Arc<dyn ApprovalGate>>,
    /// R03-T06: the subagent launcher (child-run dispatch/reply/close),
    /// held as a weak TRAIT OBJECT — see `subagents::SubagentLauncher`
    /// for why the indirection exists. `None` (the no-provider test
    /// wiring) makes a delegation request a loud tool failure, never a
    /// silent no-op.
    subagents: Option<std::sync::Weak<dyn crate::subagents::SubagentLauncher>>,
}

impl std::fmt::Debug for RunSupervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunSupervisor")
            .field("provider", &self.provider.as_ref().map(|_| "injected"))
            .field("tools", &self.tools.as_ref().map(|_| "injected"))
            .field("limits", &self.limits)
            .field("quotas", &self.quotas)
            .field("cancel_policy", &self.cancel_policy)
            .field("approval_gate", &self.approval.as_ref().map(|_| "injected"))
            .field(
                "subagent_launcher",
                &self.subagents.as_ref().map(|_| "bound"),
            )
            .finish()
    }
}

impl RunSupervisor {
    /// Production wiring until R05 registers real providers: no provider,
    /// no tools, no approval gate. Runs driven by this supervisor complete
    /// WITHOUT model content (`completed.no_final.no_provider_configured`)
    /// — an explicit outcome, never a fabricated reply. The default
    /// (bounded, loud) quota and cancellation policies apply.
    pub fn without_provider() -> Self {
        let cancel_policy = CancelPolicy::default();
        Self {
            provider: None,
            tools: None,
            limits: RunDriveLimits::default(),
            quotas: QuotaManager::new(crate::quotas::QuotaLimits::default()),
            cancel: CancelRegistry::new(),
            tasks: Arc::new(TaskSupervisor::with_cooperative_grace(
                cancel_policy.supervised_task_cap,
                Duration::from_millis(cancel_policy.cleanup_grace_ms),
            )),
            cancel_policy,
            approval: None,
            subagents: None,
        }
    }

    /// Full injection (tests drive deterministic doubles through the REAL
    /// chain; R05/R04 replace the doubles with real adapters). Degenerate
    /// drive bounds AND degenerate quota/cancel policies are loud errors.
    pub fn new(
        provider: Option<Arc<dyn TurnProviderPort>>,
        tools: Option<Arc<dyn ToolExecutorPort>>,
        limits: RunDriveLimits,
        quotas: QuotaManager,
        cancel_policy: CancelPolicy,
        approval: Option<Arc<dyn ApprovalGate>>,
        subagent_launcher: Option<std::sync::Weak<dyn crate::subagents::SubagentLauncher>>,
    ) -> Result<Self, StorageError> {
        limits.validate()?;
        quotas.limits().validate()?;
        cancel_policy.validate()?;
        Ok(Self {
            provider,
            tools,
            limits,
            quotas,
            cancel: CancelRegistry::new(),
            // R03 repair G01/F02: run-level children share the cleanup
            // policy's grace as their cooperative-cancel window (one
            // anchor with the owner's drain budget).
            tasks: Arc::new(TaskSupervisor::with_cooperative_grace(
                cancel_policy.supervised_task_cap,
                Duration::from_millis(cancel_policy.cleanup_grace_ms),
            )),
            cancel_policy,
            approval,
            subagents: subagent_launcher,
        })
    }

    pub fn limits(&self) -> &RunDriveLimits {
        &self.limits
    }

    /// The admission-quota manager (observability / acceptance assertions).
    pub fn quotas(&self) -> &QuotaManager {
        &self.quotas
    }

    pub fn provider_configured(&self) -> bool {
        self.provider.is_some()
    }

    /// The cancellation-tree registry (R03-T03): resolves live runs and
    /// their phases.
    pub fn cancel_registry(&self) -> &CancelRegistry {
        &self.cancel
    }

    /// The task supervisor (R03-T03): supervised children, recoverable
    /// handles and exit results.
    pub fn task_supervisor(&self) -> &Arc<TaskSupervisor> {
        &self.tasks
    }

    /// The cleanup deadline policy in force.
    pub fn cancel_policy(&self) -> &CancelPolicy {
        &self.cancel_policy
    }

    /// Whether an approval gate is wired (the minimal R03-T03 interface;
    /// production default until R04: none).
    pub fn approval_gate_configured(&self) -> bool {
        self.approval.is_some()
    }

    /// Whether the subagent launcher is wired (R03-T06 observability).
    pub fn subagent_launcher_configured(&self) -> bool {
        self.subagents.is_some()
    }

    /// The user-facing cancellation entry (R03-T03): fires the run's
    /// cancellation TREE — the durable `cancelling` leg, the child
    /// cleanup and the single `cancelled` finalize are driven by the
    /// run's own driver when it observes the request.
    pub fn cancel_run(&self, run_id: &str, reason: &str) -> FireOutcome {
        self.cancel.fire(run_id, reason)
    }

    /// The live cancellation phase of one run (`None` when no live run is
    /// registered — query the durable status separately).
    pub fn cancel_phase(&self, run_id: &str) -> Option<CancelPhase> {
        self.cancel.get(run_id).map(|entry| entry.phase())
    }

    /// The ROOT cancellation scope of one live run (supervision queries —
    /// e.g. linking a demonstrative child run to the parent's tree).
    pub fn run_scope(&self, run_id: &str) -> Option<CancelScope> {
        self.cancel.get(run_id).map(|entry| entry.scope.clone())
    }

    /// One layered admission acquisition racing the run's cancellation
    /// (R03-T03: a queued wait must EXIT on cancel — the T02 WaitGuard
    /// returns the queue place on drop). `Err(())` = cancelled;
    /// `Ok(None)` = quota refused (the run fails loudly with
    /// `failed.quota_exhausted.*`).
    async fn acquire_or_break(
        &self,
        resource: &QuotaResource,
        agent_id: &str,
        session_id: &str,
        run_id: &RunId,
        scope: &CancelScope,
    ) -> Result<Option<crate::quotas::QuotaPermit>, ()> {
        tokio::select! {
            biased;
            _ = scope.cancelled() => Err(()),
            admitted = self.quotas.acquire(*resource, agent_id, session_id) => {
                match admitted {
                    Ok(permit) => Ok(Some(permit)),
                    Err(failure) => {
                        tracing::warn!(
                            run_id = %run_id,
                            resource = ?resource,
                            error = %failure,
                            "admission quota refused the call: the run will settle as \
                             failed.quota_exhausted (never an unbounded wait, never a fake success)"
                        );
                        Ok(None)
                    }
                }
            }
        }
    }

    /// The R03-T04 write-side fence verdict for one asynchronous return.
    fn fence_verdict(
        &self,
        root: &CancelScope,
        fence: &ResultFence,
        ctx: &lingxi_kernel::RunContext,
    ) -> FenceVerdict {
        if !fence.matches_ctx(ctx) {
            FenceVerdict::Stale(LateResultReason::FenceMismatch)
        } else if root.is_cancelled() {
            // "在写状态前核对": the result carries the right identity, but
            // the cancellation was observed before the state write — the
            // completed content is stale for a run that is stopping.
            FenceVerdict::Stale(LateResultReason::CancelledBeforeWrite)
        } else {
            FenceVerdict::Current
        }
    }

    /// Records one refused late result as an AUDIT-ONLY durable fact
    /// (R03-T04: 拒写但留审计痕迹). The audit context carries the CLAIMED
    /// identity triple from the fence plus the driving principal/session —
    /// it proves WHAT ARRIVED LATE, never that the claim was legitimate,
    /// and it never advances any run's state.
    ///
    /// An audit-write failure is an explicit, loudly-logged degradation:
    /// the fence itself already held (the result is refused either way);
    /// the run's own state writes surface their own storage errors.
    async fn audit_late_result<P: StoragePort>(
        &self,
        port: &P,
        ctx: &lingxi_kernel::RunContext,
        fence: &ResultFence,
        reason: LateResultReason,
        refused_event_types: Vec<String>,
        now_ms: u64,
    ) {
        let audit_ctx = lingxi_kernel::RunContext {
            principal: ctx.principal.clone(),
            session_id: ctx.session_id.clone(),
            run_id: fence.run_id.clone(),
            attempt: fence.attempt.clone(),
            generation: fence.generation,
        };
        tracing::warn!(
            run_id = %audit_ctx.run_id,
            attempt = %audit_ctx.attempt,
            current_attempt = %ctx.attempt,
            reason = reason.name(),
            refused = ?refused_event_types,
            "late asynchronous result fenced: refused for state purposes, audited only \
             (never appended to the current attempt, never resurrecting a settled run)"
        );
        let fact = StaleResultFact {
            reason,
            refused_event_types,
        };
        if let Err(err) = port.record_stale_result(&audit_ctx, fact, now_ms).await {
            tracing::error!(
                run_id = %audit_ctx.run_id,
                attempt = %audit_ctx.attempt,
                error = %err,
                "stale-result AUDIT write failed (explicit degradation: the fence itself \
                 held — the result stays refused — but the durable audit trace for this \
                 late result is missing)"
            );
        }
    }

    /// Drives ONE run from creation to its single finalize (R03-T01) under
    /// the R03-T03 cancellation tree.
    ///
    /// Callers (the session execute surface) have already done the session
    /// lookup and ownership check; `run_id` was allocated by the storage
    /// backend's atomic allocator (fixed at task creation).
    ///
    /// R03-T06 wiring: `quota_session_lane` separates the CONCURRENCY
    /// lane from the durable session binding — user submissions pass the
    /// session id itself (identical semantics); a subagent child run
    /// passes its own isolated lane (the incumbent isolates subagents in
    /// their own sessions, so a parked parent holding the session's model
    /// lane can never starve its child, and vice versa). The durable
    /// facts still bind to `session_id`.
    ///
    /// R03-T02 wiring: `agent_id` scopes the per-agent quota lanes,
    /// `steering` is the session's steering channel (drained BEFORE each
    /// provider turn — steered text reaches the NEXT model call and never
    /// interrupts the loop; frozen incumbent semantics). Every model/tool
    /// call acquires its global/agent/session admission permit for the
    /// duration of the I/O only — never while holding any supervisor lock,
    /// and never waiting without a bound (quota failures settle the run
    /// LOUDLY with `failed.quota_exhausted.*`).
    ///
    /// R03-T03 wiring: the run registers a ROOT scope in the cancellation
    /// registry; every model call, approval wait and tool call is a
    /// SUPERVISED child of that scope (owner + recoverable handle + exit
    /// result — no fire-and-forget), so a cancellation request fires the
    /// tree and the driver then walks the four phases (requested →
    /// cleaning → confirmed / unconfirmed), persists the durable
    /// `cancelling` leg and settles `cancelled` through the SAME single
    /// finalize. If the driving future itself disappears before any
    /// finalize (dropped request, crash), the guard cancels the tree,
    /// marks the entry `Abandoned` and leaves the durable run row honest
    /// (its recovery classification is R03-T07).
    ///
    /// The chain:
    /// 1. validate the creation transition queued→running through the
    ///    kernel state machine, then persist the durable start (run row +
    ///    first attempt row + `run_state_changed` key event) and publish
    ///    strictly after the commit;
    /// 2. drive model turns (each with its OWN ModelCallId; tool requests
    ///    execute through the tool port with their OWN ToolCallIds; a
    ///    retryable provider failure opens a NEW ATTEMPT on the SAME run);
    /// 3. settle exactly once through [`Self::finalize_settlement`].
    #[allow(clippy::too_many_arguments)]
    pub async fn drive_run<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        principal: &lingxi_kernel::Principal,
        session_id: &str,
        agent_id: &str,
        run_id: &str,
        input: &str,
        generation: u64,
        now_ms: u64,
        steering: Option<&SteeringInbox>,
        parent_scope: Option<&CancelScope>,
        authorization: DriveAuthorization,
        admission: Option<&crate::dedup::AdmissionBinding>,
        quota_session_lane: &str,
    ) -> Result<RunFinish, DriveError> {
        let run_id = RunId::new(run_id.to_string());
        // R03-T03: register the run's root scope + phase; the guard covers
        // EVERY exit path below (finalize disarm, error, drop).
        // R03-T06: a subagent child run registers its root UNDER the
        // parent's scope — cancelling the parent propagates into this
        // run's own tree (abortByParentSession semantics), never the
        // reverse.
        let entry = match parent_scope {
            None => self.cancel.register(run_id.as_str()),
            Some(parent) => self.cancel.register_linked(run_id.as_str(), parent),
        };
        let mut guard = RegistrationGuard {
            registry: &self.cancel,
            run_id: run_id.clone(),
            entry: Arc::clone(&entry),
            disarmed: false,
        };
        let root = entry.scope.clone();
        // 1) Creation transition: the kernel state machine is consulted on
        //    the live chain (the storage transaction re-checks it under the
        //    write lock — this is the early, diagnosable half).
        RunStateMachine::transition(RunStatus::Queued, RunStatus::Running).map_err(|err| {
            DriveError::Internal(format!(
                "creation transition queued->running rejected by the kernel state machine: {}",
                err.reason
            ))
        })?;

        let mut attempt_seq: u32 = 1;
        let mut ctx = lingxi_kernel::RunContext {
            principal: principal.clone(),
            session_id: lingxi_protocol::SessionId::new(session_id.to_string()),
            run_id: run_id.clone(),
            attempt: attempt_id(&run_id, attempt_seq),
            generation,
        };
        let started: CommittedOutcome = port
            .record_run_started(&ctx, now_ms)
            .await
            .map_err(DriveError::Storage)?;
        // R03 repair G04/F05 — THE PROMISE POINT: the durable run-start
        // commit is the moment an explicit requestId (when the submission
        // carried one) starts promising THIS run's identity. From here the
        // id→run binding is durably admitted: replays are real, and no
        // later failure — a lost response, a mid-drive storage error, a
        // dropped drive — may retract it (the run row is a durable fact;
        // before this point nothing external can have happened, which is
        // what makes the caller's provably-not-started retraction safe).
        if let Some(binding) = admission {
            binding.commit_durable();
        }
        events.publish_committed(&started.events);
        // R03-T06: the four-part lineage (parentRunId/origin/
        // sourceMessageId/causeId) is durably recorded right after the run
        // row exists — the parent-child relationship survives every later
        // state of the run, including refusals and crashes.
        port.record_run_lineage(&ctx, authorization.lineage.clone(), now_ms)
            .await
            .map_err(DriveError::Storage)?;

        // 2) Model turns. No provider configured: explicit no-content
        //    completion (never a fake reply). The early close goes through
        //    the SAME adjudicated finalize as every other terminal (R03
        //    repair G02/F03: a cancellation accepted at the boundary wins).
        let Some(provider) = self.provider.clone() else {
            let finish = RunFinish::CompletedWithoutFinal {
                cause: NoFinalCause::NoProviderConfigured,
            };
            let finish = self
                .adjudicated_finalize(
                    port,
                    events,
                    &ctx,
                    &entry,
                    RunStatus::Running,
                    finish,
                    now_ms,
                )
                .await?;
            guard.disarm();
            return Ok(finish);
        };

        let descriptor = provider.descriptor();
        let mut turn: u32 = 0;
        let mut tool_call_seq: u32 = 0;
        let mut saw_tool_failure = false;
        let mut saw_process_content = false;
        // The driver-observed live status (the durable leg of the
        // waiting_approval round trip; the cancelling entry uses it as the
        // `from` of the two-phase cancellation).
        let mut live_status = RunStatus::Running;
        // R03 repair G02/F03 (C04): the dispatch-boundary gate. Every
        // point below where the driver RESUMES from an await and is about
        // to start a NEW external operation (model call, tool execution,
        // subagent dispatch — or the writes that lead into them) re-checks
        // the tree: a cancellation accepted while the driver was parked
        // must result in the four-phase cancellation settle, never in a
        // new external call. This is NOT the terminal-race fix — the
        // cancel-vs-terminal adjudication is the atomic
        // [`Self::adjudicated_finalize`] claim — it is the 执行前重检撤销
        // leg of the taskbook §5 tool contract.
        macro_rules! gate_cancel {
            () => {
                if root.is_cancelled() {
                    let reason = root.reason().unwrap_or_else(|| "cancelled".to_string());
                    let finish = self
                        .settle_cancellation(
                            port,
                            events,
                            &ctx,
                            &entry,
                            live_status,
                            reason,
                            now_ms,
                        )
                        .await?;
                    guard.disarm();
                    return Ok(finish);
                }
            };
        }
        let finish = 'turns: loop {
            gate_cancel!();
            turn += 1;
            if turn > self.limits.max_model_turns {
                break RunFinish::Failed {
                    cause: FailureCause::TurnBudgetExceeded {
                        max_turns: self.limits.max_model_turns,
                    },
                };
            }
            // Steering (frozen incumbent semantics): drain the session's
            // steering channel BEFORE the turn — steered text reaches this
            // model call's input; the loop itself is never interrupted.
            let mut turn_input = input.to_string();
            if let Some(inbox) = steering {
                if let Some(steered) = inbox.drain_joined() {
                    turn_input = format!("{input}\n\n[steering]\n{steered}");
                }
            }
            let call = model_call_id(&run_id, turn);
            // Admission (R03-T02): one model-call permit (global → agent →
            // session) held across the model I/O only. A quota failure is
            // a LOUD run failure — never an unbounded wait, never a fake
            // success. R03-T03: the QUEUED wait itself exits on cancel.
            let model_permit = match self
                .acquire_or_break(
                    &QuotaResource::Model,
                    agent_id,
                    quota_session_lane,
                    &run_id,
                    &root,
                )
                .await
            {
                Ok(Some(permit)) => permit,
                Ok(None) => {
                    break RunFinish::Failed {
                        cause: FailureCause::QuotaExhausted {
                            resource: QuotaResource::Model,
                        },
                    };
                }
                Err(()) => {
                    let reason = root.reason().unwrap_or_else(|| "cancelled".to_string());
                    let finish = self
                        .settle_cancellation(
                            port,
                            events,
                            &ctx,
                            &entry,
                            live_status,
                            reason,
                            now_ms,
                        )
                        .await?;
                    guard.disarm();
                    return Ok(finish);
                }
            };
            // R03-T03: the model call (a network-stream read in R05
            // terms) runs as a SUPERVISED child of the run's tree —
            // cancellation drops the provider future at its await point.
            // F03-C04: re-check the tree before the dispatch itself — a
            // cancellation accepted while the driver waited for the
            // admission permit must not turn into a new provider call.
            gate_cancel!();
            let call_scope = root.child(
                format!("model_call:{}", call.as_str()),
                crate::cancel::ScopeKind::ModelCall,
            );
            let call_ctx = ctx.clone();
            let call_id = call.clone();
            let stream_input = turn_input.clone();
            let provider_clone = Arc::clone(&provider);
            let model_child = self
                .tasks
                .spawn_linked(
                    run_id.as_str(),
                    &call_scope,
                    format!("model_call:{}", call.as_str()),
                    async move {
                        provider_clone
                            .next_turn(&call_ctx, &call_id, turn, &stream_input)
                            .await
                    },
                )
                .map_err(|rejected| DriveError::Storage(rejected.into()))?;
            let provider_turn = tokio::select! {
                biased;
                _ = root.cancelled() => {
                    drop(model_permit);
                    let reason = root.reason().unwrap_or_else(|| "cancelled".to_string());
                    let finish = self
                        .settle_cancellation(port, events, &ctx, &entry, live_status, reason, now_ms)
                        .await?;
                    guard.disarm();
                    return Ok(finish);
                }
                exit = model_child.wait() => match exit {
                    Ok(turn) => turn,
                    Err(task_exit) => {
                        drop(model_permit);
                        // Supervision return (R03-T03 step 4): a child
                        // panic/abort/error is a LOUD provider failure —
                        // never a silent drop, never a fake reply.
                        tracing::error!(
                            run_id = %run_id,
                            model_call = %call,
                            exit = task_exit.name(),
                            "supervised model-call child ended without a turn; failing loudly"
                        );
                        break RunFinish::Failed {
                            cause: FailureCause::ProviderFailed {
                                code: format!(
                                    "model_call_child_{}",
                                    task_exit.name()
                                ),
                                retryable: false,
                            },
                        };
                    }
                },
            };
            drop(model_permit);
            // R03-T04 result fence: verify the asynchronous return's
            // identity BEFORE writing any state (not only when the request
            // was issued). A result naming another run/attempt/generation,
            // or one whose cancellation was observed first, is audited as a
            // stale fact and never appended to the run's stream.
            let provider_result = match self.fence_verdict(&root, &provider_turn.fence, &ctx) {
                FenceVerdict::Current => provider_turn,
                FenceVerdict::Stale(reason) => {
                    self.audit_late_result(
                        port,
                        &ctx,
                        &provider_turn.fence,
                        reason,
                        vec!["model_call_result".to_string()],
                        now_ms,
                    )
                    .await;
                    if root.is_cancelled() {
                        // The cancellation won the write race: the completed
                        // turn is stale, the run settles through the cancel
                        // path and the turn's content never lands.
                        let cancel_reason =
                            root.reason().unwrap_or_else(|| "cancelled".to_string());
                        let finish = self
                            .settle_cancellation(
                                port,
                                events,
                                &ctx,
                                &entry,
                                live_status,
                                cancel_reason,
                                now_ms,
                            )
                            .await?;
                        guard.disarm();
                        return Ok(finish);
                    }
                    // Fence mismatch on a live run = an adapter delivering a
                    // result under the WRONG identity: a loud provider
                    // failure, never a silent skip.
                    break RunFinish::Failed {
                        cause: FailureCause::ProviderFailed {
                            code: format!("late_result_fenced.{}", reason.name()),
                            retryable: false,
                        },
                    };
                }
            };
            let provider_turn = provider_result.turn;
            match provider_turn {
                lingxi_kernel::ports::ProviderTurn::Final { message } => {
                    if message.content.is_empty() {
                        // "Final" with zero content blocks is the empty
                        // reply — no empty final message is committed.
                        break finish_no_final(saw_tool_failure, saw_process_content);
                    }
                    self.persist_model_call(
                        port,
                        events,
                        &ctx,
                        &call,
                        &descriptor,
                        "final_answer",
                        now_ms,
                    )
                    .await?;
                    break RunFinish::CompletedWithFinal { message };
                }
                lingxi_kernel::ports::ProviderTurn::ToolRequests { requests } => {
                    if requests.is_empty() {
                        // A tool-request turn with zero calls is a protocol
                        // violation of the double/adapter, not "no tools
                        // needed": loud failure.
                        break RunFinish::Failed {
                            cause: FailureCause::ProviderFailed {
                                code: "empty_tool_request_list".to_string(),
                                retryable: false,
                            },
                        };
                    }
                    self.persist_model_call(
                        port,
                        events,
                        &ctx,
                        &call,
                        &descriptor,
                        "tool_request",
                        now_ms,
                    )
                    .await?;
                    // F03-C04: the model-event storage boundary — a
                    // cancellation accepted during the persist ends the
                    // turn loop here (no tool admission, no intent write).
                    gate_cancel!();
                    let Some(tools) = self.tools.clone() else {
                        break RunFinish::Failed {
                            cause: FailureCause::ToolExecutorUnavailable,
                        };
                    };
                    for request in &requests {
                        // F03-C04: each tool-loop iteration re-checks — a
                        // cancellation accepted during the previous
                        // iteration's receipt/event writes stops the loop
                        // before any new admission or intent.
                        gate_cancel!();
                        tool_call_seq += 1;
                        let call_id = tool_call_id(&run_id, tool_call_seq);
                        // Admission (R03-T02): one tool-call permit per
                        // call, held across the approval wait AND the tool
                        // I/O (the incumbent registers the execution for
                        // its whole lifetime, approval wait included).
                        let tool_permit = match self
                            .acquire_or_break(
                                &QuotaResource::Tool,
                                agent_id,
                                quota_session_lane,
                                &run_id,
                                &root,
                            )
                            .await
                        {
                            Ok(Some(permit)) => permit,
                            Ok(None) => {
                                break 'turns RunFinish::Failed {
                                    cause: FailureCause::QuotaExhausted {
                                        resource: QuotaResource::Tool,
                                    },
                                };
                            }
                            Err(()) => {
                                let reason =
                                    root.reason().unwrap_or_else(|| "cancelled".to_string());
                                let finish = self
                                    .settle_cancellation(
                                        port,
                                        events,
                                        &ctx,
                                        &entry,
                                        live_status,
                                        reason,
                                        now_ms,
                                    )
                                    .await?;
                                guard.disarm();
                                return Ok(finish);
                            }
                        };
                        // F03-C04: re-check after the admission wait — a
                        // cancellation accepted while queued for the tool
                        // permit must not write a new invocation intent.
                        gate_cancel!();
                        // ── R04-T01: the anti-forgery argument gate ──
                        // The request must carry the COMPLETE effective
                        // arguments whose canonical digest IS args_digest.
                        // A provider/adapter that self-fills a digest for
                        // OTHER arguments is a protocol violation: the
                        // journal records the TRUSTED digest (computed
                        // from the arguments this driver actually holds)
                        // and the call is closed as a never-dispatched
                        // failure — zero side effects, structured refusal
                        // back to the model.
                        if !request.digest_matches_arguments() {
                            let trusted_digest = request.arguments.digest();
                            tracing::warn!(
                                run_id = %run_id,
                                tool_call = %call_id,
                                target = request.target,
                                declared = request.args_digest.hex,
                                computed = trusted_digest.hex,
                                "tool request argument digest mismatch: refusing (zero \
                                 dispatch); the journal binds the trusted digest"
                            );
                            port.record_invocation_intent(
                                &ctx,
                                InvocationIntent {
                                    journal_id: call_id.clone(),
                                    target: request.target.clone(),
                                    args_digest: trusted_digest.hex.clone(),
                                    args_summary: request.args_summary.clone(),
                                    idempotency_key: Some(call_id.to_string()),
                                },
                                now_ms,
                            )
                            .await
                            .map_err(DriveError::Storage)?;
                            saw_tool_failure = true;
                            saw_process_content = true;
                            let outcome = ToolOutcome::Failed {
                                error: ProtocolError::new(
                                    ErrorCode::InvalidMessage,
                                    "tool request args_digest does not match its arguments \
                                     (forged or adulterated request); not dispatched"
                                        .to_string(),
                                    false,
                                ),
                            };
                            port.record_invocation_receipt(
                                &ctx,
                                &call_id,
                                InvocationReceipt {
                                    outcome: ReceiptOutcome::Failed,
                                    detail: format!(
                                        "not dispatched: args_digest mismatch (declared {}, \
                                         computed {})",
                                        request.args_digest.hex, trusted_digest.hex
                                    ),
                                    dedup_id: None,
                                    dispatched: false,
                                },
                                now_ms,
                            )
                            .await
                            .map_err(DriveError::Storage)?;
                            self.persist_tool_event(
                                port,
                                events,
                                &ctx,
                                &call_id,
                                request,
                                Some(&outcome),
                                now_ms,
                            )
                            .await?;
                            drop(tool_permit);
                            continue;
                        }
                        // R03-T05: the invocation INTENT is durable BEFORE
                        // anything else — no external execution may ever be
                        // dispatched without its prepared receipt on disk.
                        // The idempotency key is derived from the durable
                        // call identity (stable across restarts); whether
                        // the external system HONORS it is a per-tool
                        // recovery capability, not a claim made here.
                        port.record_invocation_intent(
                            &ctx,
                            InvocationIntent {
                                journal_id: call_id.clone(),
                                target: request.target.clone(),
                                args_digest: request.args_digest.hex.clone(),
                                args_summary: request.args_summary.clone(),
                                idempotency_key: Some(call_id.to_string()),
                            },
                            now_ms,
                        )
                        .await
                        .map_err(DriveError::Storage)?;
                        self.persist_tool_event(
                            port, events, &ctx, &call_id, request, None, now_ms,
                        )
                        .await?;
                        // F03-C04: the storage/authorization boundary —
                        // the writes above are durable audit facts, but a
                        // cancellation accepted while they committed ends
                        // the loop here: no authorization, no approval
                        // round trip, no dispatch.
                        gate_cancel!();
                        // ── R03-T06: the run-layer authorization boundary ──
                        // For a user run (Full grant) the T05 semantics
                        // stand unchanged: the run-layer context itself
                        // authorizes. For a subagent child run EVERY
                        // target is authorized against the attenuated
                        // grant — the refusal below is produced by THIS
                        // real boundary (not by any executor double), so
                        // the executor is never invoked (zero dispatch),
                        // the journal closes the invocation as a
                        // never-dispatched failure carrying the reason,
                        // and the model sees the structured refusal.
                        let authorization_decision = match authorization.grant {
                            RunGrant::Full => ToolAuthorization::Allowed,
                            RunGrant::Subagent { tier } => {
                                authorize_child_tool(tier, &request.target)
                            }
                        };
                        if let ToolAuthorization::Denied {
                            code,
                            layer,
                            message,
                        } = authorization_decision
                        {
                            tracing::warn!(
                                run_id = %run_id,
                                tool_call = %call_id,
                                target = %request.target,
                                code = code,
                                layer = layer,
                                grant = ?authorization.grant,
                                "tool target refused by the run-layer authorization boundary: \
                                 never dispatched (parent-child relationship and reason are \
                                 preserved in the lineage row and the receipt)"
                            );
                            saw_tool_failure = true;
                            saw_process_content = true;
                            let outcome = ToolOutcome::Failed {
                                error: ProtocolError::new(
                                    ErrorCode::Forbidden,
                                    format!("{code} [{layer}]: {message}"),
                                    false,
                                ),
                            };
                            port.record_invocation_receipt(
                                &ctx,
                                &call_id,
                                InvocationReceipt {
                                    outcome: ReceiptOutcome::Failed,
                                    detail: format!(
                                        "not dispatched: authorization denied ({code} \
                                         [{layer}]): {message}"
                                    ),
                                    dedup_id: None,
                                    dispatched: false,
                                },
                                now_ms,
                            )
                            .await
                            .map_err(DriveError::Storage)?;
                            self.persist_tool_event(
                                port,
                                events,
                                &ctx,
                                &call_id,
                                request,
                                Some(&outcome),
                                now_ms,
                            )
                            .await?;
                            drop(tool_permit);
                            continue;
                        }
                        // No approval gate configured: the R03 run-layer
                        // authorization context itself authorizes the call
                        // (quota-admitted, authenticated run). R04's policy
                        // gateway replaces this decision point.
                        if self.approval.is_none() {
                            port.advance_invocation(
                                &ctx,
                                &call_id,
                                InvocationPhase::Authorized,
                                now_ms,
                            )
                            .await
                            .map_err(DriveError::Storage)?;
                        }
                        // ── approval wait (R03-T03 minimal interface) ──
                        if let Some(gate) = self.approval.clone() {
                            // F03-C04: asking a human is an external
                            // interaction too — a cancellation accepted at
                            // the authorization boundary must not open a
                            // new approval round trip.
                            gate_cancel!();
                            live_status = RunStatus::WaitingApproval;
                            self.persist_state_change(
                                port,
                                events,
                                &ctx,
                                RunStatus::Running,
                                RunStatus::WaitingApproval,
                                Some("approval_required".to_string()),
                                now_ms,
                            )
                            .await?;
                            let gate_scope = root.child(
                                format!("approval:{}", call_id.as_str()),
                                crate::cancel::ScopeKind::ToolCall,
                            );
                            let gate_req = ApprovalRequest {
                                tool_call_id: call_id.clone(),
                                target: request.target.clone(),
                                args_digest: request.args_digest.hex.clone(),
                            };
                            let gate_ctx = ctx.clone();
                            let gate_child = self
                                .tasks
                                .spawn_linked(
                                    run_id.as_str(),
                                    &gate_scope,
                                    format!("approval:{}", call_id.as_str()),
                                    async move { gate.request(&gate_ctx, &gate_req).await },
                                )
                                .map_err(|rejected| DriveError::Storage(rejected.into()))?;
                            let decision = tokio::select! {
                                biased;
                                _ = root.cancelled() => {
                                    drop(tool_permit);
                                    let reason =
                                        root.reason().unwrap_or_else(|| "cancelled".to_string());
                                    let finish = self
                                        .settle_cancellation(
                                            port,
                                            events,
                                            &ctx,
                                            &entry,
                                            live_status,
                                            reason,
                                            now_ms,
                                        )
                                        .await?;
                                    guard.disarm();
                                    return Ok(finish);
                                }
                                exit = gate_child.wait() => match exit {
                                    Ok(decision) => decision,
                                    Err(task_exit) => {
                                        tracing::error!(
                                            run_id = %run_id,
                                            tool_call = %call_id,
                                            exit = task_exit.name(),
                                            "approval gate child ended without a decision; \
                                             treating as rejected (zero executions)"
                                        );
                                        ApprovalDecision::Aborted
                                    }
                                },
                            };
                            // Back to running BEFORE any tool execution (or
                            // rejection): the approval wait is over.
                            live_status = RunStatus::Running;
                            self.persist_state_change(
                                port,
                                events,
                                &ctx,
                                RunStatus::WaitingApproval,
                                RunStatus::Running,
                                Some("approval_resolved".to_string()),
                                now_ms,
                            )
                            .await?;
                            match decision {
                                ApprovalDecision::Approved => {
                                    // R03-T05: the approval RESOLVED — the
                                    // receipt advances to authorized (still
                                    // before any external execution).
                                    port.advance_invocation(
                                        &ctx,
                                        &call_id,
                                        InvocationPhase::Authorized,
                                        now_ms,
                                    )
                                    .await
                                    .map_err(DriveError::Storage)?;
                                }
                                ApprovalDecision::Rejected { .. } | ApprovalDecision::Aborted => {
                                    // ZERO executions — the rejection is a
                                    // recorded tool failure, not a silent
                                    // skip (frozen incumbent: the wrapper
                                    // returns toolError with 执行 0 次). The
                                    // journal closes the invocation as a
                                    // never-dispatched failure.
                                    let reason = match decision {
                                        ApprovalDecision::Rejected { reason } => reason,
                                        ApprovalDecision::Aborted => "approval aborted".to_string(),
                                        ApprovalDecision::Approved => unreachable!(),
                                    };
                                    saw_tool_failure = true;
                                    saw_process_content = true;
                                    let outcome = ToolOutcome::Failed {
                                        error: ProtocolError::new(
                                            ErrorCode::Forbidden,
                                            format!("tool request not approved: {reason}"),
                                            false,
                                        ),
                                    };
                                    port.record_invocation_receipt(
                                        &ctx,
                                        &call_id,
                                        InvocationReceipt {
                                            outcome: ReceiptOutcome::Failed,
                                            detail: format!("not dispatched: {reason}"),
                                            dedup_id: None,
                                            dispatched: false,
                                        },
                                        now_ms,
                                    )
                                    .await
                                    .map_err(DriveError::Storage)?;
                                    self.persist_tool_event(
                                        port,
                                        events,
                                        &ctx,
                                        &call_id,
                                        request,
                                        Some(&outcome),
                                        now_ms,
                                    )
                                    .await?;
                                    drop(tool_permit);
                                    continue;
                                }
                            }
                        }
                        // ── R03-T06: subagent-family delegation targets ──
                        // A delegation dispatches a CHILD RUN through the
                        // SAME supervisor (linked to this run's
                        // cancellation tree; fire-and-forget for the
                        // parent) instead of an external tool execution.
                        // The parent's tool call returns the child
                        // identity immediately; the child's result is
                        // delivered back through the session's steering
                        // channel (the incumbent's deferred-result
                        // "trigger_parent_turn" delivery, at the R03
                        // fidelity).
                        // F03-C04: the child-run dispatch boundary — a
                        // cancellation accepted at the authorization or
                        // approval boundary above must not spawn a new
                        // child run.
                        gate_cancel!();
                        if let Some(delegation) = request.delegation.clone() {
                            // The dispatch IS the side effect: `started`
                            // is durable before it (same write-order
                            // contract as any external execution).
                            port.advance_invocation(
                                &ctx,
                                &call_id,
                                InvocationPhase::Started,
                                now_ms,
                            )
                            .await
                            .map_err(DriveError::Storage)?;
                            let parent = crate::subagents::ParentRunFacts {
                                principal: ctx.principal.clone(),
                                session_id: ctx.session_id.to_string(),
                                agent_id: agent_id.to_string(),
                                parent_run_id: run_id.clone(),
                                parent_scope: root.clone(),
                                source_model_call: call.clone(),
                                cause_tool_call: call_id.clone(),
                                now_ms,
                            };
                            let launcher =
                                self.subagents.as_ref().and_then(std::sync::Weak::upgrade);
                            // `subagent_close` creates no run: its success
                            // carries the closed thread id as the content
                            // digest, its refusals take the shared failure
                            // shape below.
                            let launched: Result<
                                Option<String>,
                                crate::subagents::SubagentDispatchError,
                            > = if request.target == "subagent_close" {
                                match launcher
                                    .as_ref()
                                    .map(|launcher| launcher.close(&parent, &delegation))
                                {
                                    Some(Ok(closed)) => Ok(Some(closed.thread_id)),
                                    Some(Err(err)) => Err(err),
                                    None => Err(crate::subagents::SubagentDispatchError::NotBound),
                                }
                            } else {
                                let Some(launcher) = launcher else {
                                    return Err(DriveError::Internal(
                                        "subagent launcher not bound in this supervisor"
                                            .to_string(),
                                    ));
                                };
                                match request.target.as_str() {
                                    "subagent" => launcher
                                        .dispatch(parent, delegation)
                                        .await
                                        .map(|child| Some(child.child_run_id)),
                                    "subagent_reply" => launcher
                                        .reply(parent, delegation)
                                        .await
                                        .map(|child| Some(child.child_run_id)),
                                    // A delegation payload on a
                                    // non-subagent-family target is a
                                    // protocol violation of the
                                    // double/adapter: loud, never executed.
                                    other => {
                                        Err(crate::subagents::SubagentDispatchError::InvalidTarget(
                                            other.to_string(),
                                        ))
                                    }
                                }
                            };
                            let outcome = match launched {
                                Ok(Some(identity)) => {
                                    // R04-T01: the delegation success is a
                                    // CONSUMABLE structured result — the
                                    // child run / thread identity is real
                                    // content (its derived digest is the
                                    // receipt's dedup id, as before).
                                    ToolOutcome::success_text(identity)
                                }
                                Ok(None) => unreachable!("every delegation success carries an id"),
                                Err(err) => {
                                    tracing::warn!(
                                        run_id = %run_id,
                                        tool_call = %call_id,
                                        target = %request.target,
                                        error = %err,
                                        "subagent delegation refused; recorded as a tool \
                                         failure (zero child runs created on refusal paths)"
                                    );
                                    ToolOutcome::Failed {
                                        error: err.tool_error(),
                                    }
                                }
                            };
                            let failed = matches!(outcome, ToolOutcome::Failed { .. });
                            // R03 repair G03/F04 (same family): a delegation
                            // REFUSAL is a known never-dispatched negative
                            // fact — every `SubagentDispatchError` leaves
                            // ZERO child runs created. The receipt keeps
                            // `dispatched=false` so the three fact classes
                            // (never dispatched / trusted external failure /
                            // runner failure without an external result)
                            // stay distinguishable in the journal.
                            let receipt = match &outcome {
                                ToolOutcome::Success { .. } => journal_receipt_of(&outcome),
                                ToolOutcome::Failed { error } => InvocationReceipt {
                                    outcome: ReceiptOutcome::Failed,
                                    detail: format!(
                                        "not dispatched: {}: {}",
                                        error.code.wire_name(),
                                        error.message
                                    ),
                                    dedup_id: None,
                                    dispatched: false,
                                },
                                ToolOutcome::Cancelled | ToolOutcome::Unknown { .. } => {
                                    unreachable!("delegation maps success or a refusal only")
                                }
                            };
                            port.record_invocation_receipt(&ctx, &call_id, receipt, now_ms)
                                .await
                                .map_err(DriveError::Storage)?;
                            self.persist_tool_event(
                                port,
                                events,
                                &ctx,
                                &call_id,
                                request,
                                Some(&outcome),
                                now_ms,
                            )
                            .await?;
                            if failed {
                                saw_tool_failure = true;
                            }
                            saw_process_content = true;
                            drop(tool_permit);
                            continue;
                        }
                        // R03-T05: `started` is durable BEFORE the external
                        // execution is dispatched. This write is what makes
                        // a crash between the external execution and the
                        // receipt commit classifiable as UNKNOWN (已执行但
                        // 回执未持久化) — never as "not executed", never as
                        // "succeeded".
                        port.advance_invocation(&ctx, &call_id, InvocationPhase::Started, now_ms)
                            .await
                            .map_err(DriveError::Storage)?;
                        // F03-C04: the dispatch boundary itself — a
                        // cancellation accepted during the `started` write
                        // must not become an external tool execution (the
                        // journal entry stays at `started`; recovery
                        // classifies it as unobserved).
                        gate_cancel!();
                        // ── supervised tool execution ──
                        let tool_scope = root.child(
                            format!("tool_call:{}", call_id.as_str()),
                            crate::cancel::ScopeKind::ToolCall,
                        );
                        let tool_ctx = ctx.clone();
                        let tool_request = request.clone();
                        let exec_call_id = call_id.clone();
                        let tools_clone = Arc::clone(&tools);
                        let tool_child = self
                            .tasks
                            .spawn_linked(
                                run_id.as_str(),
                                &tool_scope,
                                format!("tool_call:{}", call_id.as_str()),
                                async move {
                                    tools_clone
                                        .execute(&tool_ctx, &exec_call_id, &tool_request)
                                        .await
                                },
                            )
                            .map_err(|rejected| DriveError::Storage(rejected.into()))?;
                        let outcome = tokio::select! {
                            biased;
                            _ = root.cancelled() => {
                                drop(tool_permit);
                                let reason =
                                    root.reason().unwrap_or_else(|| "cancelled".to_string());
                                let finish = self
                                    .settle_cancellation(
                                        port,
                                        events,
                                        &ctx,
                                        &entry,
                                        live_status,
                                        reason,
                                        now_ms,
                                    )
                                    .await?;
                                guard.disarm();
                                return Ok(finish);
                            }
                            exit = tool_child.wait() => match exit {
                                Ok(result) => result,
                                Err(task_exit) => {
                                    // Supervision return (R03 repair
                                    // G03/F04): the invocation WAS
                                    // dispatched (`started` is durable) and
                                    // the supervised child ended WITHOUT
                                    // delivering an outcome. A panic, a
                                    // forced abort or a lost supervision
                                    // channel proves NOTHING about the
                                    // external operation — the runner-level
                                    // failure is recorded separately
                                    // (log + receipt detail, never
                                    // masquerading as an external result)
                                    // and the invocation journals as
                                    // UNKNOWN: never a fabricated confirmed
                                    // failure, never a success. The run
                                    // continues; the tool partial-failure
                                    // vocabulary settles it.
                                    let reason =
                                        unobserved_tool_exit_reason(&task_exit);
                                    tracing::error!(
                                        run_id = %run_id,
                                        tool_call = %call_id,
                                        exit = task_exit.name(),
                                        "supervised tool child ended without an outcome; \
                                         the external outcome is unobserved — journaling \
                                         unknown, never a fabricated failure"
                                    );
                                    ToolExecutionResult::of_ctx(
                                        &ctx,
                                        ToolOutcome::Unknown { reason },
                                    )
                                }
                            },
                        };
                        // R03-T04 result fence: same write-side identity
                        // check as model calls. A fenced tool result is
                        // audited; a live run records the started call as
                        // Unknown (the receipt is not trustworthy — never
                        // retried blindly, never a success).
                        let outcome = match self.fence_verdict(&root, &outcome.fence, &ctx) {
                            FenceVerdict::Current => outcome.outcome,
                            FenceVerdict::Stale(reason) => {
                                self.audit_late_result(
                                    port,
                                    &ctx,
                                    &outcome.fence,
                                    reason,
                                    vec!["tool_call_result".to_string()],
                                    now_ms,
                                )
                                .await;
                                if root.is_cancelled() {
                                    drop(tool_permit);
                                    let cancel_reason =
                                        root.reason().unwrap_or_else(|| "cancelled".to_string());
                                    let finish = self
                                        .settle_cancellation(
                                            port,
                                            events,
                                            &ctx,
                                            &entry,
                                            live_status,
                                            cancel_reason,
                                            now_ms,
                                        )
                                        .await?;
                                    guard.disarm();
                                    return Ok(finish);
                                }
                                ToolOutcome::Unknown {
                                    reason: format!(
                                        "tool result fenced as stale ({})",
                                        reason.name()
                                    ),
                                }
                            }
                        };
                        if matches!(
                            outcome,
                            ToolOutcome::Failed { .. }
                                | ToolOutcome::Cancelled
                                | ToolOutcome::Unknown { .. }
                        ) {
                            saw_tool_failure = true;
                        }
                        saw_process_content = true;
                        // R03-T05: the receipt (external response / dedup
                        // identifier) is durable AFTER the execution — no
                        // cross-system atomic transaction is claimed. This
                        // lands BEFORE the stream event: a crash between
                        // them leaves the receipt as the recovery evidence.
                        port.record_invocation_receipt(
                            &ctx,
                            &call_id,
                            journal_receipt_of(&outcome),
                            now_ms,
                        )
                        .await
                        .map_err(DriveError::Storage)?;
                        self.persist_tool_event(
                            port,
                            events,
                            &ctx,
                            &call_id,
                            request,
                            Some(&outcome),
                            now_ms,
                        )
                        .await?;
                        drop(tool_permit);
                    }
                }
                lingxi_kernel::ports::ProviderTurn::Continue { .. } => {
                    saw_process_content = true;
                    self.persist_model_call(
                        port,
                        events,
                        &ctx,
                        &call,
                        &descriptor,
                        "process_only",
                        now_ms,
                    )
                    .await?;
                }
                lingxi_kernel::ports::ProviderTurn::Empty { .. } => {
                    self.persist_model_call(
                        port,
                        events,
                        &ctx,
                        &call,
                        &descriptor,
                        "empty_reply",
                        now_ms,
                    )
                    .await?;
                    break finish_no_final(saw_tool_failure, saw_process_content);
                }
                lingxi_kernel::ports::ProviderTurn::Failed { error, retryable } => {
                    self.persist_model_call(
                        port,
                        events,
                        &ctx,
                        &call,
                        &descriptor,
                        "failed",
                        now_ms,
                    )
                    .await?;
                    if retryable && attempt_seq < self.limits.max_attempts {
                        // Retry = NEW ATTEMPT on the SAME run (the run id is
                        // fixed at creation; a provider reconnect never
                        // becomes a new user task).
                        attempt_seq += 1;
                        ctx.attempt = attempt_id(&run_id, attempt_seq);
                        port.record_attempt_started(&ctx, now_ms)
                            .await
                            .map_err(DriveError::Storage)?;
                        continue;
                    }
                    break RunFinish::Failed {
                        cause: FailureCause::ProviderFailed {
                            code: error.code.wire_name().to_string(),
                            retryable,
                        },
                    };
                }
            }
        };

        // 3) Exactly one finalize, through the single settlement path —
        //    ADJUDICATED against any cancellation accepted while the loop
        //    was driving (R03 repair G02/F03): the claim and the commit
        //    happen in that order with no re-check gap in between.
        let finish = self
            .adjudicated_finalize(port, events, &ctx, &entry, live_status, finish, now_ms)
            .await?;
        guard.disarm();
        Ok(finish)
    }

    /// R03 repair G02/F03 — the UNIFIED cancel-vs-terminal adjudication
    /// for every NON-cancellation terminal a driver can settle
    /// (completed/failed, including the no-provider early close). This is
    /// the one place the race is decided, and it is atomic:
    ///
    /// - [`TerminalAdjudication::Claimed`] — no cancellation had been
    ///   accepted when the claim was taken (the entry's phase mutex is
    ///   the linearization point; the claim is recorded BEFORE the
    ///   finalize's first await). The finalize then commits exactly once
    ///   and a cancellation arriving during it reads
    ///   [`FireOutcome::TooLate`] — never an Accepted-with-stop-promise.
    /// - [`TerminalAdjudication::CancelledBy`] — a cancellation was
    ///   accepted first: the intended terminal is DIVERTED to the
    ///   four-phase cancellation settle. No `completed`/`failed` and no
    ///   final message commit after an accepted cancellation, whatever
    ///   awaits sat between the last fence check and here (the fix is
    ///   deliberately NOT "one more is_cancelled before the last await" —
    ///   that would still leave the check→commit window open).
    // Same explicit-dependency-passing shape as `drive_run`.
    #[allow(clippy::too_many_arguments)]
    async fn adjudicated_finalize<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        entry: &Arc<RunCancelEntry>,
        live_status: RunStatus,
        finish: RunFinish,
        now_ms: u64,
    ) -> Result<RunFinish, DriveError> {
        let superseded = finish.terminal_reason();
        match entry.claim_terminal(&superseded) {
            TerminalAdjudication::Claimed => {
                self.finalize_settlement(port, events, ctx, live_status, finish, now_ms)
                    .await
            }
            TerminalAdjudication::CancelledBy { reason } => {
                tracing::info!(
                    run_id = %ctx.run_id,
                    superseded_terminal = %superseded,
                    cancel_reason = %reason,
                    "cancellation accepted before the terminal claim: the intended terminal \
                     is diverted to the four-phase cancellation settle (no completed/failed \
                     and no final message commit after an accepted cancellation)"
                );
                let mut settled = self
                    .settle_cancellation(port, events, ctx, entry, live_status, reason, now_ms)
                    .await?;
                if let RunFinish::Cancelled { detail } = &mut settled {
                    // The superseded in-flight terminal stays diagnosable
                    // in the returned verdict (audit note, never a second
                    // terminal — the durable terminal is `cancelled`).
                    detail.push_str(&format!("; superseded in-flight terminal: {superseded}"));
                }
                Ok(settled)
            }
        }
    }

    /// The R03-T03 cancellation flow, phases 2–4 + the single finalize:
    /// persist the durable `cancelling` leg, drain the run's supervised
    /// children under the anchored cleanup budget, then settle
    /// `cancelled` through the ONE finalize path (from `cancelling`, per
    /// the kernel's two-phase contract). Quotas return by RAII as the
    /// caller's permits drop on the way out.
    // Same explicit-dependency-passing shape as `drive_run` (port/events
    // are caller-owned); the parameter set is inherent to it.
    #[allow(clippy::too_many_arguments)]
    async fn settle_cancellation<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        entry: &Arc<RunCancelEntry>,
        live_status: RunStatus,
        reason: String,
        now_ms: u64,
    ) -> Result<RunFinish, DriveError> {
        // Phase 2 — 开始清理. The budget is anchored at the REQUEST
        // moment (a late-observed cancellation keeps only what remains).
        let requested_at = entry
            .scope
            .cancelled_at()
            .unwrap_or_else(std::time::Instant::now);
        entry.advance_phase(CancelPhase::Cleaning {
            reason: reason.clone(),
        });
        self.persist_state_change(
            port,
            events,
            ctx,
            live_status,
            RunStatus::Cancelling,
            Some(format!("cancelled:{reason}")),
            now_ms,
        )
        .await?;
        let budget = CancelBudget::new(
            requested_at,
            Duration::from_millis(self.cancel_policy.cleanup_grace_ms),
        );
        // Confirmed-stopped children = the ones the TREE already ended
        // (observed exits in the registry — they self-confirmed through
        // the scope drop) UNION the ones the bounded drain joined in
        // time. The drain reaps its own confirms, so the snapshot is
        // taken BEFORE it runs.
        let tree_confirmed: Vec<String> = self
            .tasks
            .tasks()
            .iter()
            .filter(|task| {
                task.run_id.as_deref() == Some(ctx.run_id.as_str()) && task.exit.is_some()
            })
            .map(|task| task.describe())
            .collect();
        let report = self.tasks.drain_run(ctx.run_id.as_str(), budget).await;
        let mut confirmed = tree_confirmed;
        confirmed.extend(report.confirmed.iter().map(|t| t.describe()));
        let unconfirmed: Vec<String> = report.unconfirmed.iter().map(|t| t.describe()).collect();
        let finish = if report.all_quiet() {
            entry.advance_phase(CancelPhase::ConfirmedTerminated {
                reason: reason.clone(),
                confirmed: confirmed.clone(),
            });
            RunFinish::Cancelled {
                detail: format!(
                    "{reason}; all {} supervised children confirmed exit within the \
                     cleanup budget",
                    confirmed.len()
                ),
            }
        } else {
            // Phase 4 — 无法确认停止: report the un-cleaned items; never
            // claim quiet. External actions the children already performed
            // are NOT promised rolled back.
            tracing::warn!(
                run_id = %ctx.run_id,
                unconfirmed = ?unconfirmed,
                budget_ms = self.cancel_policy.cleanup_grace_ms,
                "cancellation cleanup deadline reached with live children — \
                 reporting them; already-performed external actions are not \
                 promised rolled back"
            );
            entry.advance_phase(CancelPhase::StopUnconfirmed {
                reason: reason.clone(),
                confirmed: confirmed.clone(),
                unconfirmed: unconfirmed.clone(),
            });
            RunFinish::Cancelled {
                detail: format!(
                    "{reason}; cleanup budget expired with {} unconfirmed children: {}; \
                     external actions already performed are not promised rolled back",
                    unconfirmed.len(),
                    unconfirmed.join(", ")
                ),
            }
        };
        // Phase 3 — the single finalize from the durable `cancelling` leg.
        self.finalize_settlement(port, events, ctx, RunStatus::Cancelling, finish, now_ms)
            .await
    }

    /// Persists one NON-TERMINAL run phase change (waiting_approval round
    /// trips and the durable cancelling leg) and publishes after commit.
    #[allow(clippy::too_many_arguments)]
    async fn persist_state_change<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        from: RunStatus,
        to: RunStatus,
        reason: Option<String>,
        now_ms: u64,
    ) -> Result<(), DriveError> {
        let committed = port
            .record_run_state_change(ctx, from, to, reason, now_ms)
            .await
            .map_err(DriveError::Storage)?;
        events.publish_committed(&committed.events);
        Ok(())
    }

    /// The single finalize path (R03-T01 step 3). Builds the terminal
    /// [`RunOutcome`](lingxi_kernel::ports::RunOutcome) from the kernel
    /// outcome contract, persists status + key events (+ final message) in
    /// ONE storage transaction and publishes strictly after the commit.
    ///
    /// `from` is the run's live (driver-observed) status — the storage
    /// transaction remains the authority and re-validates the transition
    /// under the write lock. Identical duplicate submissions of the SAME
    /// settlement replay idempotently (the storage transaction decides via
    /// the kernel's [`RunStateMachine::finalize`]); conflicting settlements
    /// are loud [`StorageError::Conflict`]s. Public so cancel/recovery
    /// surfaces (R03-T03/T07) settle through the SAME path.
    pub async fn finalize_settlement<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        from: RunStatus,
        finish: RunFinish,
        now_ms: u64,
    ) -> Result<RunFinish, DriveError> {
        let run_id = ctx.run_id.clone();
        let to = finish.status();
        let reason = finish.terminal_reason();
        let final_message = finish.final_message().cloned();
        let outcome = lingxi_kernel::ports::RunOutcome {
            status: to,
            reason: Some(reason.clone()),
            key_events: vec![KeyEvent {
                event_id: EventId::new(format!("{run_id}-done")),
                payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                    RunStateChangedPayload {
                        from,
                        to,
                        reason: Some(reason),
                    },
                )),
            }],
            final_message,
        };
        let committed = port
            .commit_run_outcome(ctx, outcome, now_ms)
            .await
            .map_err(DriveError::Storage)?;
        events.publish_committed(&committed.events);
        if !committed.newly_committed {
            tracing::info!(
                run_id = %run_id,
                status = %to.wire_name(),
                "finalize replayed idempotently (settlement already durable)"
            );
        }
        Ok(finish)
    }

    /// Persists one model call's facts as key events in ONE transaction:
    /// `model_call_started` + `model_call_completed` bound to the CURRENT
    /// attempt (R03-A01 database evidence). Mid-call crash recovery is NOT
    /// claimed here — the invocation journal / recovery coordinator are
    /// R03-T05/T07.
    // Explicit dependency passing (port/events) plus the call identity and
    // clock: the parameter set is inherent to the injected-port style.
    #[allow(clippy::too_many_arguments)]
    async fn persist_model_call<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        call: &ModelCallId,
        descriptor: &lingxi_kernel::ports::ProviderDescriptor,
        finish_kind: &'static str,
        now_ms: u64,
    ) -> Result<(), DriveError> {
        let run_id = ctx.run_id.to_string();
        let key_events = vec![
            KeyEvent {
                event_id: EventId::new(format!("{run_id}-{}-start", call.as_str())),
                payload: EventPayload::Known(KnownEventPayload::ModelCallStarted(
                    ModelCallStartedPayload {
                        model_call_id: call.clone(),
                        provider: descriptor.provider.clone(),
                        model: descriptor.model.clone(),
                        operation: descriptor.operation.clone(),
                    },
                )),
            },
            KeyEvent {
                event_id: EventId::new(format!("{run_id}-{}-done", call.as_str())),
                payload: EventPayload::Known(KnownEventPayload::ModelCallCompleted(
                    ModelCallCompletedPayload {
                        model_call_id: call.clone(),
                        // Usage is filled by real providers (R05); a
                        // deterministic double never invents token counts.
                        usage: None,
                    },
                )),
            },
        ];
        let committed = port
            .record_run_events(ctx, key_events, now_ms)
            .await
            .map_err(DriveError::Storage)?;
        debug_assert_eq!(
            committed.events.len(),
            2,
            "record_run_events stages exactly the submitted events"
        );
        let _ = finish_kind; // diagnostic anchor (the event pair is the fact)
        events.publish_committed(&committed.events);
        Ok(())
    }

    /// Persists one tool call's started (outcome `None`) or completed fact
    /// as a key event in one transaction.
    #[allow(clippy::too_many_arguments)]
    async fn persist_tool_event<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        call: &lingxi_protocol::ToolCallId,
        request: &ToolRequest,
        outcome: Option<&ToolOutcome>,
        now_ms: u64,
    ) -> Result<(), DriveError> {
        let run_id = ctx.run_id.to_string();
        let payload = match outcome {
            None => KnownEventPayload::ToolCallStarted(ToolCallStartedPayload {
                tool_call: ToolCallDescriptor {
                    tool_call_id: call.clone(),
                    target: request.target.clone(),
                    args_digest: request.args_digest.clone(),
                    args_summary: request.args_summary.clone(),
                },
            }),
            Some(outcome) => KnownEventPayload::ToolCallCompleted(ToolCallCompletedPayload {
                tool_call_id: call.clone(),
                result: tool_result_wire(outcome),
            }),
        };
        let suffix = if outcome.is_some() { "done" } else { "start" };
        let committed = port
            .record_run_events(
                ctx,
                vec![KeyEvent {
                    event_id: EventId::new(format!("{run_id}-{}-{suffix}", call.as_str())),
                    payload: EventPayload::Known(payload),
                }],
                now_ms,
            )
            .await
            .map_err(DriveError::Storage)?;
        events.publish_committed(&committed.events);
        Ok(())
    }
}

/// Resolves the explicit no-final cause from what the run actually
/// observed (R03-T01 step 4): tool partial failure > process-only content >
/// empty reply. None of them fabricates a final answer.
fn finish_no_final(saw_tool_failure: bool, saw_process_content: bool) -> RunFinish {
    let cause = if saw_tool_failure {
        NoFinalCause::ToolPartialFailure
    } else if saw_process_content {
        NoFinalCause::ProcessOnly
    } else {
        NoFinalCause::EmptyReply
    };
    RunFinish::CompletedWithoutFinal { cause }
}

/// Renders one supervised tool-child exit that arrived WITHOUT a tool
/// outcome into the diagnosable reason of the honest UNKNOWN receipt
/// (R03 repair G03/F04). Consumes the REAL [`TaskExit`] variant (never a
/// string guess — the G01 supervision vocabulary is the source of
/// truth): every runner-level anomaly class keeps its own diagnostic,
/// and none of them pretends to be an external result.
fn unobserved_tool_exit_reason(exit: &TaskExit) -> String {
    match exit {
        TaskExit::Panicked(payload) => format!(
            "tool executor panicked after dispatch (panic: {payload}); the external \
             outcome is unobserved"
        ),
        TaskExit::Aborted => "tool executor dropped at an await point after dispatch \
             (supervised abort); the external outcome is unobserved"
            .to_string(),
        TaskExit::Failed(detail) => format!(
            "tool supervision channel lost after dispatch ({detail}); the external \
             outcome is unobserved"
        ),
        TaskExit::Completed => "supervision reported completion without delivering an \
             outcome (internal anomaly); the external outcome is unobserved"
            .to_string(),
    }
}

/// Maps a tool outcome onto the durable invocation RECEIPT (R03-T05). A
/// success's derived content digest is the dedup identifier the external
/// system made available; the process run/exit status travels in the
/// detail when present (the receipt is a bounded diagnostic, the
/// structured result in the stream event carries the full payload); a
/// cancelled outcome says the executor STOPPED WAITING — it does not
/// prove the external operation did not complete, so it journals as
/// Unknown (never a fabricated failure); a fenced/unobserved result is
/// Unknown by construction. The receipt records what the external system
/// returned — no cross-system atomicity.
///
/// R03 repair G03/F04: the `Failed` arm is only reachable from outcomes
/// the EXECUTOR returned (a trustworthy external failure receipt). A
/// supervised child that ended WITHOUT an outcome never reaches this
/// mapping as `Failed` — the driver journals it through
/// [`unobserved_tool_exit_reason`] as Unknown instead.
fn journal_receipt_of(outcome: &ToolOutcome) -> InvocationReceipt {
    match outcome {
        ToolOutcome::Success { result } => InvocationReceipt {
            outcome: ReceiptOutcome::Succeeded,
            detail: match &result.status {
                Some(lingxi_kernel::ports::ToolRunStatus::Exited { code }) => format!(
                    "external content digest {} (exit {code})",
                    result.content_digest
                ),
                Some(lingxi_kernel::ports::ToolRunStatus::Running { handle }) => format!(
                    "external content digest {} (still running, handle {handle})",
                    result.content_digest
                ),
                None => format!("external content digest {}", result.content_digest),
            },
            dedup_id: Some(result.content_digest.clone()),
            dispatched: true,
        },
        ToolOutcome::Failed { error } => InvocationReceipt {
            outcome: ReceiptOutcome::Failed,
            detail: format!("{}: {}", error.code.wire_name(), error.message),
            dedup_id: None,
            dispatched: true,
        },
        ToolOutcome::Cancelled => InvocationReceipt {
            outcome: ReceiptOutcome::Unknown,
            detail: "executor stopped waiting (cancelled); external outcome unobserved".to_string(),
            dedup_id: None,
            dispatched: true,
        },
        ToolOutcome::Unknown { reason } => InvocationReceipt {
            outcome: ReceiptOutcome::Unknown,
            detail: reason.clone(),
            dedup_id: None,
            dispatched: true,
        },
    }
}

/// Maps the kernel [`ToolOutcome`] onto the wire tool-result shape
/// (R04-T01: the SUCCESS arm now carries the REAL structured result —
/// actual content blocks, resource references, the truncation flag —
/// instead of a digest placeholder; consumers read the content, the
/// digest remains audit/journal data only). Unknown stays Unknown —
/// never retried, never "success".
fn tool_result_wire(outcome: &ToolOutcome) -> ToolResultWire {
    match outcome {
        ToolOutcome::Success { result } => ToolResultWire {
            status: ToolResultStatus::Success,
            content: result.content.clone(),
            resource_refs: result.resource_refs.clone(),
            truncated: result.truncated,
            error: None,
        },
        ToolOutcome::Failed { error } => ToolResultWire {
            status: ToolResultStatus::Failed,
            content: Vec::new(),
            resource_refs: Vec::new(),
            truncated: false,
            error: Some(error.clone()),
        },
        ToolOutcome::Cancelled => ToolResultWire {
            status: ToolResultStatus::Cancelled,
            content: Vec::new(),
            resource_refs: Vec::new(),
            truncated: false,
            error: None,
        },
        ToolOutcome::Unknown { reason } => ToolResultWire {
            status: ToolResultStatus::Unknown,
            content: Vec::new(),
            resource_refs: Vec::new(),
            truncated: false,
            error: Some(ProtocolError::new(
                ErrorCode::Internal,
                format!("unknown tool outcome: {reason}"),
                false,
            )),
        },
    }
}

/// Converts a drive error into the session execute error surface.
impl From<DriveError> for crate::sessions::SessionExecuteError {
    fn from(err: DriveError) -> Self {
        match err {
            DriveError::Storage(storage) => crate::sessions::SessionExecuteError::Storage(storage),
            DriveError::Internal(detail) => {
                crate::sessions::SessionExecuteError::Storage(StorageError::Internal {
                    detail: format!("run driver invariant violated: {detail}"),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_protocol::ContentBlock;

    #[test]
    fn limits_validation_rejects_degenerate_bounds() {
        assert!(RunDriveLimits::default().validate().is_ok());
        for bad in [
            RunDriveLimits {
                max_model_turns: 0,
                max_attempts: 1,
            },
            RunDriveLimits {
                max_model_turns: RunDriveLimits::ABSOLUTE_MAX_MODEL_TURNS + 1,
                max_attempts: 1,
            },
            RunDriveLimits {
                max_model_turns: 4,
                max_attempts: 0,
            },
            RunDriveLimits {
                max_model_turns: 4,
                max_attempts: RunDriveLimits::ABSOLUTE_MAX_ATTEMPTS + 1,
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn without_provider_is_explicit_not_fabricated() {
        let supervisor = RunSupervisor::without_provider();
        assert!(!supervisor.provider_configured());
        assert_eq!(
            supervisor.limits(),
            &RunDriveLimits {
                max_model_turns: RunDriveLimits::DEFAULT_MAX_MODEL_TURNS,
                max_attempts: RunDriveLimits::DEFAULT_MAX_ATTEMPTS,
            }
        );
    }

    #[test]
    fn no_final_cause_priority_is_tool_failure_over_process_over_empty() {
        let finish = finish_no_final(true, true);
        assert_eq!(
            finish.terminal_reason(),
            "completed.no_final.tool_partial_failure"
        );
        let finish = finish_no_final(false, true);
        assert_eq!(finish.terminal_reason(), "completed.no_final.process_only");
        let finish = finish_no_final(false, false);
        assert_eq!(finish.terminal_reason(), "completed.no_final.empty_reply");
    }

    #[test]
    fn tool_outcome_maps_onto_wire_status_one_to_one() {
        let wire = tool_result_wire(&ToolOutcome::success_text("abc"));
        assert_eq!(wire.status, ToolResultStatus::Success);
        // R04-T01: the success wire carries the REAL content block, not a
        // digest placeholder.
        assert_eq!(
            wire.content,
            vec![ContentBlock::Text {
                text: "abc".to_string()
            }]
        );
        let wire = tool_result_wire(&ToolOutcome::Failed {
            error: ProtocolError::new(ErrorCode::Internal, "x", false),
        });
        assert_eq!(wire.status, ToolResultStatus::Failed);
        assert!(wire.error.is_some());
        let wire = tool_result_wire(&ToolOutcome::Cancelled);
        assert_eq!(wire.status, ToolResultStatus::Cancelled);
        let wire = tool_result_wire(&ToolOutcome::Unknown {
            reason: "receipt lost".to_string(),
        });
        assert_eq!(wire.status, ToolResultStatus::Unknown);
    }

    /// R04-T01: a success receipt keeps the R03 dedup semantics (the
    /// derived content digest) and records the process status in the
    /// detail when present.
    #[test]
    fn success_receipt_keeps_digest_dedup_and_status_detail() {
        let receipt = journal_receipt_of(&ToolOutcome::success_text("hello"));
        assert_eq!(receipt.outcome, ReceiptOutcome::Succeeded);
        assert!(receipt.dispatched);
        let expected_digest = lingxi_kernel::ports::ToolSuccess::text("hello").content_digest;
        assert_eq!(receipt.dedup_id.as_deref(), Some(expected_digest.as_str()));
        let exited = ToolOutcome::Success {
            result: lingxi_kernel::ports::ToolSuccess {
                status: Some(lingxi_kernel::ports::ToolRunStatus::Exited { code: 3 }),
                ..lingxi_kernel::ports::ToolSuccess::text("out")
            },
        };
        let receipt = journal_receipt_of(&exited);
        assert!(
            receipt.detail.contains("exit 3"),
            "detail: {}",
            receipt.detail
        );
    }

    /// R03-T05: the durable receipt mapping. Success carries the external
    /// dedup identifier (the derived content digest); a CANCELLED outcome
    /// journals as Unknown (stopping the wait does not prove the external
    /// operation did not complete); fenced/unobserved results are Unknown
    /// by construction.
    #[test]
    fn journal_receipt_maps_outcomes_onto_the_durable_receipt() {
        let receipt = journal_receipt_of(&ToolOutcome::success_text("dd-1"));
        assert_eq!(
            receipt.outcome,
            lingxi_kernel::ports::ReceiptOutcome::Succeeded
        );
        let expected_digest = lingxi_kernel::ports::ToolSuccess::text("dd-1").content_digest;
        assert_eq!(receipt.dedup_id.as_deref(), Some(expected_digest.as_str()));
        assert!(receipt.dispatched);

        let receipt = journal_receipt_of(&ToolOutcome::Failed {
            error: ProtocolError::new(ErrorCode::Forbidden, "denied", false),
        });
        assert_eq!(
            receipt.outcome,
            lingxi_kernel::ports::ReceiptOutcome::Failed
        );
        assert!(!receipt.detail.is_empty());
        assert!(receipt.dispatched);

        let receipt = journal_receipt_of(&ToolOutcome::Cancelled);
        assert_eq!(
            receipt.outcome,
            lingxi_kernel::ports::ReceiptOutcome::Unknown
        );
        assert!(receipt.detail.contains("unobserved"), "{}", receipt.detail);
        assert!(receipt.dispatched);

        let receipt = journal_receipt_of(&ToolOutcome::Unknown {
            reason: "receipt lost".to_string(),
        });
        assert_eq!(
            receipt.outcome,
            lingxi_kernel::ports::ReceiptOutcome::Unknown
        );
        assert_eq!(receipt.detail, "receipt lost");
    }

    /// R03 repair G03/F04: EVERY supervised tool-child exit that arrives
    /// WITHOUT an outcome maps to an UNKNOWN reason that (a) names the
    /// real `TaskExit` variant's anomaly class, (b) states the external
    /// outcome is unobserved, and (c) never claims an external result.
    /// No anomaly variant — panic, forced abort, lost supervision
    /// channel, or the internal completion anomaly — leaks into a
    /// confirmed-failure/success vocabulary.
    #[test]
    fn every_unobserved_tool_exit_variant_journals_unknown_diagnosably() {
        let cases: Vec<(TaskExit, &str)> = vec![
            (
                TaskExit::Panicked("tool double exploded".to_string()),
                "tool double exploded",
            ),
            (TaskExit::Aborted, "supervised abort"),
            (
                TaskExit::Failed("child ended without delivering a result".to_string()),
                "supervision channel lost",
            ),
            (TaskExit::Completed, "internal anomaly"),
        ];
        for (exit, marker) in cases {
            let reason = unobserved_tool_exit_reason(&exit);
            assert!(
                reason.contains("unobserved"),
                "every variant says the external outcome is unobserved: {reason}"
            );
            assert!(
                reason.contains(marker),
                "the variant's own anomaly class is diagnosable ({marker}): {reason}"
            );
        }
    }

    /// R03-T04 write-side fence logic (the driver's `fence_verdict`): a
    /// result is Current only when BOTH the identity triple matches the
    /// live context AND no cancellation was observed first.
    #[tokio::test]
    async fn fence_verdict_requires_identity_and_no_cancellation() {
        let supervisor = RunSupervisor::without_provider();
        let root = crate::cancel::CancelScope::run_root("run_fence_unit");
        let run_id = RunId::new("run_fence_unit".to_string());
        let ctx = lingxi_kernel::RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("s".to_string()),
            attempt: lingxi_kernel::attempt_id(&run_id, 2),
            run_id: run_id.clone(),
            generation: 1,
        };
        // Matching fence, live run → Current.
        let fence = ResultFence::of_ctx(&ctx);
        assert_eq!(
            supervisor.fence_verdict(&root, &fence, &ctx),
            FenceVerdict::Current
        );
        // Older attempt → FenceMismatch.
        let old = lingxi_kernel::RunContext {
            attempt: lingxi_kernel::attempt_id(&run_id, 1),
            ..ctx.clone()
        };
        assert_eq!(
            supervisor.fence_verdict(&root, &ResultFence::of_ctx(&old), &ctx),
            FenceVerdict::Stale(LateResultReason::FenceMismatch)
        );
        // Matching identity but the cancellation fired first →
        // CancelledBeforeWrite (the write side of the biased race).
        root.cancel("user");
        assert_eq!(
            supervisor.fence_verdict(&root, &fence, &ctx),
            FenceVerdict::Stale(LateResultReason::CancelledBeforeWrite)
        );
    }
}
