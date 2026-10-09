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

use lingxi_kernel::model_exchange::{ExchangeItem, ModelTurnInput, RequestedToolCall, TurnOrigin};
use lingxi_kernel::ports::{
    CommittedOutcome, InvocationIntent, InvocationPhase, InvocationReceipt, KeyEvent,
    LateResultReason, ModelTurnDelta, ReceiptOutcome, ResultFence, StaleResultFact, StorageError,
    StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, TurnDeltaSink,
    TurnDeltaSinkClosed, TurnProviderPort,
};
use lingxi_kernel::subagent::{
    authorize_child_tool, authorize_child_tool_with_registry_id, RunLineage, ToolAccessTier,
    ToolAuthorization,
};
use lingxi_kernel::{
    attempt_id, model_call_id, tool_call_id, FailureCause, NoFinalCause, QuotaResource, RunFinish,
    RunStateMachine,
};
use lingxi_protocol::{
    AssistantSegmentDeltaPayload, AssistantSegmentEndPayload, AssistantSegmentStartPayload,
    ContentBlock, ErrorCode, EventId, EventPayload, KnownEventPayload, ModelCallCompletedPayload,
    ModelCallDeltaPayload, ModelCallId, ModelCallStartedPayload, ProtocolError, RunId,
    RunStateChangedPayload, RunStatus, ToolCallCompletedPayload, ToolCallDescriptor,
    ToolCallStartedPayload, ToolResultStatus, ToolResultWire,
};

use crate::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest};
use crate::cancel::{
    CancelBudget, CancelPhase, CancelPolicy, CancelRegistry, CancelScope, FireOutcome,
    RunCancelEntry, TerminalAdjudication,
};
use crate::events::EventService;
use crate::quotas::QuotaManager;
use crate::session_supervisor::SteeringInbox;
use crate::streaming_norm::{DeltaNormalizer, NormEvent};
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
    /// R05-T05: one initial attempt + up to two bounded retries (the
    /// pre-registered `transport_retry_max_attempts`).
    pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
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

/// R05-T05 (R05_BASELINE §8): the per-model-call wall-clock tuning. The
/// TOTAL budget one logical model call may spend spans ALL of its attempts
/// (queue wait + credential refresh + retry backoff + network + streaming);
/// the retry backoff is `base × 2^(failed_attempt-1)` capped. Injected
/// through `ServiceDeps` (the composition root) and validated like every
/// other drive bound — degenerate values are loud errors, never clamped
/// into silent shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCallTuning {
    /// `model_call_total_budget_ms` (300 000).
    pub total_budget: Duration,
    /// `retry_backoff_base_ms` (500) — the first retry's delay.
    pub retry_backoff_base: Duration,
    /// `retry_backoff_cap_ms` (8 000) — the ceiling of the doubling.
    pub retry_backoff_cap: Duration,
}

impl ModelCallTuning {
    pub const DEFAULT_TOTAL_BUDGET_MS: u64 = 300_000;
    pub const DEFAULT_RETRY_BACKOFF_BASE_MS: u64 = 500;
    pub const DEFAULT_RETRY_BACKOFF_CAP_MS: u64 = 8_000;
    /// Hard upper bounds so a degenerate injection cannot turn the retry
    /// policy unbounded.
    pub const ABSOLUTE_MAX_TOTAL_BUDGET_MS: u64 = 3_600_000;
    pub const ABSOLUTE_MAX_BACKOFF_MS: u64 = 300_000;

    pub fn validate(&self) -> Result<(), StorageError> {
        let invalid = |what: &str, detail: String| StorageError::InvalidRequest {
            detail: format!("{what}: {detail}"),
        };
        let budget_ms = u64::try_from(self.total_budget.as_millis()).unwrap_or(u64::MAX);
        if budget_ms == 0 || budget_ms > Self::ABSOLUTE_MAX_TOTAL_BUDGET_MS {
            return Err(invalid(
                "model_call_total_budget_ms",
                format!(
                    "must be in 1..={}, got {budget_ms}",
                    Self::ABSOLUTE_MAX_TOTAL_BUDGET_MS
                ),
            ));
        }
        let base_ms = u64::try_from(self.retry_backoff_base.as_millis()).unwrap_or(u64::MAX);
        let cap_ms = u64::try_from(self.retry_backoff_cap.as_millis()).unwrap_or(u64::MAX);
        if cap_ms > Self::ABSOLUTE_MAX_BACKOFF_MS {
            return Err(invalid(
                "retry_backoff_cap_ms",
                format!("must be <= {}, got {cap_ms}", Self::ABSOLUTE_MAX_BACKOFF_MS),
            ));
        }
        if base_ms > cap_ms {
            return Err(invalid(
                "retry_backoff_base_ms",
                format!("must be <= the cap ({cap_ms}), got {base_ms}"),
            ));
        }
        Ok(())
    }

    /// The delay after attempt `failed_attempt` failed (1-based): base ×
    /// 2^(failed_attempt-1), capped (attempt 1's failure → the base delay).
    pub fn backoff_delay(&self, failed_attempt: u32) -> Duration {
        let shift = failed_attempt.saturating_sub(1).min(20);
        let base_ms = u64::try_from(self.retry_backoff_base.as_millis()).unwrap_or(u64::MAX);
        let scaled = base_ms.saturating_mul(1u64 << shift);
        Duration::from_millis(
            scaled.min(u64::try_from(self.retry_backoff_cap.as_millis()).unwrap_or(u64::MAX)),
        )
    }
}

impl Default for ModelCallTuning {
    fn default() -> Self {
        Self {
            total_budget: Duration::from_millis(Self::DEFAULT_TOTAL_BUDGET_MS),
            retry_backoff_base: Duration::from_millis(Self::DEFAULT_RETRY_BACKOFF_BASE_MS),
            retry_backoff_cap: Duration::from_millis(Self::DEFAULT_RETRY_BACKOFF_CAP_MS),
        }
    }
}

/// R05-T04 (D7): the live-delta channel bound of one model call
/// (`turn_stream_max_live_deltas`). A full channel applies backpressure to
/// the provider's stream read (the sink's `emit` awaits capacity) — deltas
/// are never dropped, never buffered without bound.
pub const TURN_STREAM_LIVE_DELTA_CAP: usize = 1024;

/// R05-T04: the stream idle timeout — the R05_BASELINE §8 pre-registered
/// `http_idle_stream_timeout_ms` (60 s). Enforced by the driver on the
/// delta-drain arm: a stream that delivers no delta (and no terminal) for
/// this long is a STALLED stream — the call scope is cancelled (dropping
/// the provider's socket read at its await point, D8) and the turn
/// settles as a retryable upstream failure. This one enforcement point
/// covers the whole read chain (connect → first byte → inter-frame gaps):
/// none of those phases can park a run without bound.
pub const DEFAULT_STREAM_IDLE_TIMEOUT_MS: u64 = 60_000;

/// The D7 sink: the driver-side end of one model call's live-delta channel.
/// The sink moves INTO the supervised model-call future, so the sender
/// drops exactly when the provider turn resolves (the driver's receiver
/// then drains to `None` — the clean end-of-stream signal).
struct ChannelDeltaSink {
    tx: tokio::sync::mpsc::Sender<ModelTurnDelta>,
}

/// The per-call usage facts of ONE resolved provider turn (R05-T07): the
/// wire projection, the richer ledger fact, the serving route's identity
/// and the physical request count. Carried together so the completed
/// event's `usage` and the ledger row can never disagree.
///
/// R05 RR1 F21: the settlement `outcome`, the host-observed start
/// timestamp and (for a tool-batch turn) the emitted tool call ids ride
/// the same facts — one construction point, one ledger row, no second
/// source of truth.
#[derive(Debug, Clone)]
pub struct ModelCallUsageFacts {
    pub usage: Option<lingxi_protocol::UsageRecord>,
    pub usage_report: lingxi_kernel::usage::ReportedUsage,
    pub served_by: Option<lingxi_kernel::ports::ProviderDescriptor>,
    pub served_protocol: Option<String>,
    /// R05 RR1 F38: `None` = the attempts count is UNKNOWN — the call's
    /// future was dropped before settling (the cancellation race), so no
    /// observation of how many physical requests left the process
    /// exists. A resolved turn always carries `Some(count)`.
    pub transport_attempts: Option<u32>,
    pub outcome: lingxi_kernel::usage::CallOutcome,
    pub started_at_unix_ms: Option<u64>,
    /// The host-minted tool call ids this turn's batch emitted (the
    /// ToolRequests arm fills them before persisting — the parent-side
    /// JOIN listing of the ledger).
    pub emitted_tool_calls: Vec<String>,
}

/// The R05 RR1 F09 turn origin recorded on exchange items: the serving
/// identity of the call that produced the turn (provider+model; the
/// operation is dropped — replay authorization is source identity, not
/// purpose). `None` when no identity was reported (a double): turns whose
/// content carries same-family opaque state then render-refuse downstream,
/// never silently forward.
fn turn_origin_of(facts: &ModelCallUsageFacts) -> Option<TurnOrigin> {
    facts.served_by.as_ref().map(|served| TurnOrigin {
        provider: served.provider.clone(),
        model: served.model.clone(),
    })
}

/// Builds the usage-ledger row of one driver model call from its settle
/// facts (the single construction point both the completed-event path and
/// the R05 RR1 F38 cancelled-in-flight row share — one source, no drift).
fn model_call_usage_record_of(
    ctx: &lingxi_kernel::RunContext,
    call: &ModelCallId,
    facts: &ModelCallUsageFacts,
    ledger: &UsageLedgerContext,
    now_ms: u64,
) -> lingxi_kernel::usage::ModelCallUsageRecord {
    let served = facts.served_by.clone().unwrap_or_else(|| {
        // Unreachable on the driver path (the facts always carry the
        // descriptor fallback); kept honest rather than fabricating.
        lingxi_kernel::ports::ProviderDescriptor {
            provider: "unreported".to_string(),
            model: "unreported".to_string(),
            operation: "chat".to_string(),
        }
    });
    lingxi_kernel::usage::ModelCallUsageRecord {
        session_id: Some(ctx.session_id.to_string()),
        run_id: Some(ctx.run_id.to_string()),
        attempt: Some(ctx.attempt.to_string()),
        model_call_id: call.as_str().to_string(),
        purpose: "chat".to_string(),
        origin: ledger.origin.to_string(),
        parent_run_id: ledger.parent_run_id.clone(),
        cause_ref: ledger.cause_ref.clone(),
        parent_tool_call_id: None,
        provider: served.provider.clone(),
        model: served.model.clone(),
        protocol: facts
            .served_protocol
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        usage: match facts.usage_report.clone() {
            lingxi_kernel::usage::ReportedUsage::Known(usage) => Some(usage),
            _ => None,
        },
        invalid_detail: match &facts.usage_report {
            lingxi_kernel::usage::ReportedUsage::Invalid { detail } => Some(detail.clone()),
            _ => None,
        },
        transport_attempts: facts.transport_attempts,
        // R05 RR1 F21: the call's settlement state, its host-observed
        // timing window and (for a tool batch) the emitted tool call
        // ids — the ledger's parent-side JOIN listing.
        outcome: facts.outcome,
        started_at_unix_ms: facts.started_at_unix_ms,
        settled_at_unix_ms: Some(now_ms),
        emitted_tool_calls: facts.emitted_tool_calls.clone(),
        // T07-C08: no price source exists in this stage's config —
        // cost stays explicitly unknown, never invented.
        cost_basis: None,
    }
}

/// The run-level ledger context of a drive (R05-T07-C02): the causal
/// parentage of THIS run's model calls, derived from the drive
/// authorization's lineage — parent/origin/cause are creation facts, never
/// re-derived from timing.
#[derive(Debug, Clone)]
pub(crate) struct UsageLedgerContext {
    pub origin: &'static str,
    pub parent_run_id: Option<String>,
    pub cause_ref: Option<String>,
}

impl UsageLedgerContext {
    pub(crate) fn of_authorization(authorization: &DriveAuthorization) -> Self {
        Self {
            origin: authorization.lineage.origin.wire_name(),
            parent_run_id: authorization
                .lineage
                .parent_run_id
                .as_ref()
                .map(|run| run.to_string()),
            cause_ref: authorization.lineage.cause_id.clone(),
        }
    }
}

impl TurnDeltaSink for ChannelDeltaSink {
    fn emit<'a>(
        &'a self,
        delta: ModelTurnDelta,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>,
    > {
        Box::pin(async move {
            // A full channel AWAITS (backpressure onto the provider's
            // stream read — never a try_send drop); a closed channel means
            // the driver is gone and the provider must wind down.
            self.tx.send(delta).await.map_err(|_| TurnDeltaSinkClosed)
        })
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
    /// The session permission mode this run's tool-policy plane
    /// adjudicates under (R04-T03): the parent/user session's
    /// `operate`/`ask`/`read_only` mode, SNAPSHOTTED once at
    /// submission/dispatch. For a user run this is the session's mode;
    /// for a subagent child run it is the PARENT mode the tier inherited
    /// from (the pair `{grant tier, session_mode}` is what reproduces the
    /// incumbent's ask-tier `deny_on_prompt` semantics — R04-SUP-01).
    ///
    /// Mapping decision (documented, deliberate): the incumbent reads
    /// the mode per tool call at the executor boundary; the Rust run
    /// layer binds it at run admission — a mode switch mid-run takes
    /// effect for the runs admitted AFTER it, not retroactively for a
    /// run already driving. This keeps one run's policy adjudication a
    /// single stable fact (no mid-run TOCTOU between prepare and the
    /// kernel authorization step); the interactive surface re-reads the
    /// mode on every new submission through the same snapshot rule.
    pub session_mode: lingxi_kernel::subagent::SessionPermissionMode,
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
    pub fn user_submission(
        request_id: Option<&str>,
        session_mode: lingxi_kernel::subagent::SessionPermissionMode,
    ) -> Self {
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
            session_mode,
        }
    }

    /// The gateway permission context of this run's invocations
    /// (R04-T03): derived from the grant + the session-mode snapshot the
    /// run was admitted with — a trusted driver fact, never model data.
    pub fn invocation_permission_context(&self) -> crate::toolgateway::InvocationPermissionContext {
        match self.grant {
            RunGrant::Full => crate::toolgateway::InvocationPermissionContext::UserSession {
                mode: self.session_mode,
            },
            RunGrant::Subagent { tier } => {
                crate::toolgateway::InvocationPermissionContext::Subagent {
                    tier,
                    parent_mode: self.session_mode,
                }
            }
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
    quotas: Arc<QuotaManager>,
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
    /// R04-T02: the unified tool invocation gateway. When `Some`, EVERY
    /// tool request of every driven run is PREPARED through it before the
    /// journal's authorization step (target identity / availability /
    /// generations / current-schema arguments / policy verdict — a
    /// refusal or an approval requirement never leaves a `started`
    /// entry) and dispatched ONLY through its re-verified, single-use
    /// [`crate::toolgateway::PreparedInvocation`] handle after `started`.
    /// `None` keeps the R03 shape exactly (the raw `tools` port). The
    /// gateway holds NO grant logic: the RunGrant application below stays
    /// THE authorization decision point (R04-SUP-02) — the gateway's
    /// policy port is the layered tool-policy face whose verdict
    /// intersects with it.
    tool_gateway: Option<Arc<crate::toolgateway::ToolInvocationGateway>>,
    /// R05-T04: the stream idle timeout enforced on the live-delta drain
    /// (see [`DEFAULT_STREAM_IDLE_TIMEOUT_MS`]). A builder knob exists so
    /// tests can shorten it without waiting out the production bound.
    stream_idle_timeout: Duration,
    /// R05-T05: the per-call wall-clock tuning (total budget across all
    /// attempts of one logical call + the retry backoff shape).
    model_call_tuning: ModelCallTuning,
    /// R06-T01: the session-scoped context compiler. When `Some`, the
    /// system text of EVERY turn's [`ModelTurnInput`] is the render of the
    /// session's frozen [`crate::context_compiler::CompiledContext`] — the
    /// SAME artifact the observation endpoint serves (one build result,
    /// no second prompt reconstruction). `None` (the no-compiler test
    /// wiring) keeps the pre-R06 shape exactly (`system_prompt: None`).
    context_compiler: Option<Arc<crate::context_compiler::ContextCompilerService>>,
    /// R06-T02: the mid-run compaction service. When `Some`, every turn
    /// boundary runs the incumbent trigger evaluation (real usage + tail
    /// estimate against the chat route's declared window; FORCE 80% /
    /// reserve line) and, on FIRE, compacts the live exchange through the
    /// Summarize auxiliary slot (cache-preserving request shape). A
    /// compaction failure NEVER interrupts the run — the original
    /// exchange continues (the incumbent "compaction never throws"
    /// semantics) with a loud ledger row + warn. `None` (test wiring)
    /// keeps the pre-R06-T02 shape exactly.
    compaction: Option<Arc<crate::compaction::CompactionService>>,
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
            .field(
                "tool_gateway",
                &self.tool_gateway.as_ref().map(|_| "injected"),
            )
            .field("model_call_tuning", &self.model_call_tuning)
            .field(
                "context_compiler",
                &self.context_compiler.as_ref().map(|_| "bound"),
            )
            .field("compaction", &self.compaction.as_ref().map(|_| "bound"))
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
            quotas: Arc::new(QuotaManager::new(crate::quotas::QuotaLimits::default())),
            cancel: CancelRegistry::new(),
            tasks: Arc::new(TaskSupervisor::with_cooperative_grace(
                cancel_policy.supervised_task_cap,
                Duration::from_millis(cancel_policy.cleanup_grace_ms),
            )),
            cancel_policy,
            approval: None,
            subagents: None,
            tool_gateway: None,
            stream_idle_timeout: Duration::from_millis(DEFAULT_STREAM_IDLE_TIMEOUT_MS),
            model_call_tuning: ModelCallTuning::default(),
            context_compiler: None,
            compaction: None,
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
            quotas: Arc::new(quotas),
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
            tool_gateway: None,
            stream_idle_timeout: Duration::from_millis(DEFAULT_STREAM_IDLE_TIMEOUT_MS),
            model_call_tuning: ModelCallTuning::default(),
            context_compiler: None,
            compaction: None,
        })
    }

    /// R04-T02: binds the unified tool invocation gateway (builder-style;
    /// the composition root calls this after [`Self::new`]). Binding a
    /// gateway makes it the ONLY execution path of this supervisor — the
    /// raw `tools` port is no longer consulted for tool dispatch on this
    /// wiring (no dual path). See the field docs for the contract.
    pub fn with_tool_gateway(
        mut self,
        gateway: Option<Arc<crate::toolgateway::ToolInvocationGateway>>,
    ) -> Self {
        self.tool_gateway = gateway;
        self
    }

    /// R05-T04 test seam: overrides the stream idle timeout (production
    /// keeps the pre-registered [`DEFAULT_STREAM_IDLE_TIMEOUT_MS`]). The
    /// composition root wires it from `ServiceDeps.stream_idle_timeout`
    /// (R05-T05 F-03).
    pub fn with_stream_idle_timeout(mut self, timeout: Duration) -> Self {
        self.stream_idle_timeout = timeout;
        self
    }

    /// R05-T05: injects the per-call wall-clock tuning (the composition
    /// root passes `ServiceDeps.model_call_tuning`; tests shorten the
    /// windows). Degenerate values are a loud error, never clamped.
    pub fn with_model_call_tuning(mut self, tuning: ModelCallTuning) -> Result<Self, StorageError> {
        tuning.validate()?;
        self.model_call_tuning = tuning;
        Ok(self)
    }

    /// R06-T01: binds the session-scoped context compiler (builder-style;
    /// the composition root calls this after [`Self::new`]). Binding it
    /// makes its compiled artifact the ONLY system-prompt source of every
    /// driven run of this supervisor (no dual prompt path).
    pub fn with_context_compiler(
        mut self,
        compiler: Option<Arc<crate::context_compiler::ContextCompilerService>>,
    ) -> Self {
        self.context_compiler = compiler;
        self
    }

    /// R06-T02: binds the mid-run compaction service (builder-style; the
    /// composition root calls this after [`Self::new`]). Binding it makes
    /// every driven run's turn boundary run the incumbent trigger
    /// evaluation; `None` keeps the pre-R06-T02 shape exactly (no
    /// compaction check at all).
    pub fn with_compaction(
        mut self,
        compaction: Option<Arc<crate::compaction::CompactionService>>,
    ) -> Self {
        self.compaction = compaction;
        self
    }

    pub fn limits(&self) -> &RunDriveLimits {
        &self.limits
    }

    /// The admission-quota manager (observability / acceptance assertions).
    pub fn quotas(&self) -> &QuotaManager {
        &self.quotas
    }

    /// R05-T06 (C06): a shared handle to the SAME quota manager, so the
    /// worker-callback model port (`workermodel::GatewayWorkerModel`)
    /// admits every callback through the identical global/agent/session
    /// lanes as the main model loop — one budget, no side channel.
    pub fn quotas_shared(&self) -> Arc<QuotaManager> {
        Arc::clone(&self.quotas)
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
    ///
    /// R05 RR1 F16: when the call carries an ABSOLUTE deadline, the QUEUE
    /// WAIT races the REMAINING budget too — the wait exits the moment the
    /// deadline passes (the caller re-checks the clock to classify its
    /// `Ok(None)` as the honest `budget_exceeded` instead of waiting the
    /// quota manager's full `wait_timeout_ms` on an already-spent call).
    async fn acquire_or_break(
        &self,
        resource: &QuotaResource,
        agent_id: &str,
        session_id: &str,
        run_id: &RunId,
        scope: &CancelScope,
        deadline_unix_ms: Option<u64>,
    ) -> Result<Option<crate::quotas::QuotaPermit>, ()> {
        let budget_sleep = async {
            match lingxi_adapters::models::dispatch::remaining_budget_ms(deadline_unix_ms) {
                Some(remaining) => {
                    tokio::time::sleep(std::time::Duration::from_millis(remaining)).await
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            biased;
            _ = scope.cancelled() => Err(()),
            _ = budget_sleep => Ok(None),
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

        // R06-T01 (R06-A01 同源): compile the session's frozen context
        // artifact ONCE per (session, subagent shape) — after the provider
        // check, so the no-provider path stays byte-identical to the pre-R06
        // shape; before the first model call, so EVERY turn of this run
        // sends the SAME render the observation endpoint serves. A compile
        // failure is a LOUD non-retryable run failure (the adjudicated
        // finalize mirrors the no-provider early return above), never a
        // silently degraded prompt.
        let system_prompt = match self.context_compiler.as_ref() {
            Some(compiler) => {
                let for_subagent = matches!(authorization.grant, RunGrant::Subagent { .. });
                match compiler.compiled_for_run(session_id, agent_id, for_subagent, now_ms) {
                    Ok(compiled) => Some(compiled.render().to_string()),
                    Err(failure) => {
                        let finish = RunFinish::Failed {
                            cause: FailureCause::ProviderFailed {
                                code: format!("context_compile: {failure}"),
                                retryable: false,
                            },
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
                    }
                }
            }
            None => None,
        };

        let mut turn: u32 = 0;
        let mut tool_call_seq: u32 = 0;
        let mut saw_tool_failure = false;
        let mut saw_process_content = false;
        // R05-T01: the run's typed model exchange (assistant turns with
        // their requested tool calls interleaved with the REAL tool
        // outcomes) — the `prior` half of every next turn's
        // [`ModelTurnInput`]. R05-T03 (C12): a retried attempt CONTINUES
        // this exchange — the confirmed assistant/tool exchanges of the
        // failed attempt are real, already-persisted history the retry's
        // first call must see (never a rebuild from the bare submission,
        // never a re-executed write); only the failed model call itself
        // leaves no assistant turn behind.
        let mut exchange: Vec<ExchangeItem> = Vec::new();
        // R06-T02: the mid-run compaction trigger state (the incumbent
        // `session-compaction-runtime` facts). `last_turn_usage` = the last
        // RESOLVED model call's real usage (the trigger's primary signal —
        // never an estimate; `None` after a successful compaction mirrors
        // the incumbent retrigger guard). `tail_from` = the exchange index
        // right after the last assistant turn — the tool results beyond it
        // are the tail that entered the context AFTER that usage fact.
        let mut last_turn_usage: Option<lingxi_kernel::usage::ModelCallUsage> = None;
        let mut tail_from: usize = 0;
        let mut compaction_seq: u32 = 0;
        // R05-T07: the run's ledger context — parentage is a creation fact
        // from the authorization's lineage, captured ONCE (never guessed
        // from wall-clock proximity).
        let usage_ledger = UsageLedgerContext::of_authorization(&authorization);
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
        // R05-T01: every tool outcome that closes a tool-loop iteration —
        // all four [`ToolOutcome`] states, including every zero-dispatch
        // refusal — joins the run's exchange history, so the NEXT model
        // call's typed input carries the REAL outcome (never a guess).
        macro_rules! record_tool_result {
            ($call_id:expr, $request:expr, $outcome:expr) => {
                exchange.push(ExchangeItem::ToolResult {
                    tool_call_id: $call_id.clone(),
                    provider_call_id: $request.provider_call_id.clone(),
                    outcome: $outcome.clone(),
                })
            };
        }
        // R05-T05: the per-call TOTAL wall-clock budget
        // (`model_call_total_budget_ms`) of the CURRENT logical model call
        // as a real unix-ms deadline. Established ONCE per logical call and
        // SHARED across its retries (the retry `continue` keeps it); the
        // conversation-moving arms (ToolRequests / Continue) reset it to
        // None so the NEXT logical call opens a fresh budget. The value
        // travels to the adapter as `ModelTurnInput::deadline_unix_ms` and
        // is compared against the SAME real clock there — it deliberately
        // does NOT ride the injected `now_ms` test clock.
        let mut call_deadline: Option<u64> = None;
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
            let mut submission = input.to_string();
            if let Some(inbox) = steering {
                if let Some(steered) = inbox.drain_joined() {
                    submission = format!("{input}\n\n[steering]\n{steered}");
                }
            }
            let call = model_call_id(&run_id, turn);
            // R05 RR1 F21: the call's host-observed start moment (real
            // wall clock — the same convention as `call_deadline`, not
            // the injected test clock).
            let turn_started_unix_ms = lingxi_adapters::models::dispatch::unix_ms_now();
            // R05-T01 (C08): the tool declaration snapshot is rebuilt from
            // the LIVE registry at every send — the adapter declares exactly
            // this set to the provider, never a cached or fabricated one. A
            // build failure (a wire-name conflict) is a LOUD run failure,
            // never a silently narrowed tool list. Without a gateway wired
            // (the R03 double wiring) the snapshot is explicitly empty.
            let tool_snapshot = match self.tool_gateway.as_ref() {
                Some(gateway) => {
                    match lingxi_kernel::model_exchange::ToolDeclarationSnapshot::from_registry(
                        gateway.registry(),
                    ) {
                        Ok(snapshot) => snapshot,
                        Err(err) => {
                            break RunFinish::Failed {
                                cause: FailureCause::ProviderFailed {
                                    code: format!("tool_declaration_snapshot: {err}"),
                                    retryable: false,
                                },
                            };
                        }
                    }
                }
                None => lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
            };
            // R06-T02: the mid-run compaction check at the turn boundary
            // (the incumbent `compactIfNeeded` position: after the tool
            // snapshot exists — the compaction request carries it — and
            // before the call budget opens). Trigger = the incumbent
            // FORCE/reserve lines on the REAL last-call usage plus the
            // tail estimate; a successful compaction REPLACES the live
            // exchange with [summary item] + retained suffix (the summary
            // rides as ordinary user-role history, never as a system
            // instruction) and resets the retrigger guard; a failure keeps
            // the original exchange and the run continues (the incumbent
            // "compaction never throws" semantics — the loud ledger row
            // and warn already landed inside the service).
            if let Some(compaction) = self.compaction.clone() {
                match compaction
                    .maybe_compact_mid_run(
                        &ctx,
                        port,
                        agent_id,
                        usage_ledger.origin,
                        usage_ledger.parent_run_id.clone(),
                        usage_ledger.cause_ref.clone(),
                        &crate::compaction::MidRunCompactionInput {
                            exchange: &exchange,
                            system_prompt: system_prompt.as_deref(),
                            submission: &submission,
                            tools: &tool_snapshot,
                            last_usage: last_turn_usage.as_ref(),
                            tail_from,
                        },
                        &root,
                        compaction_seq,
                    )
                    .await?
                {
                    crate::compaction::CompactionOutcome::Compacted {
                        exchange: compacted,
                        plan,
                        report,
                    } => {
                        tracing::info!(
                            run_id = %run_id,
                            turn,
                            cut_index = plan.cut_index,
                            summarized_tokens = plan.summarized_tokens,
                            retained_tokens = plan.retained_tokens,
                            context_tokens = report.context_tokens,
                            context_window = report.context_window,
                            "mid-run compaction applied"
                        );
                        exchange = compacted;
                        // The incumbent retrigger guard holds WITHOUT
                        // clearing `last_turn_usage` here: the next read at
                        // this checkpoint only ever sees the usage of a
                        // call that settled AFTER the compaction (every
                        // path from here re-assigns it at settle time), so
                        // the pre-compaction usage can never re-fire.
                        tail_from = exchange.len();
                        compaction_seq += 1;
                    }
                    crate::compaction::CompactionOutcome::HardTruncated {
                        exchange: truncated,
                        plan,
                        report,
                    } => {
                        // 现役硬截断兜底（摘要请求自身超窗）：与 Compacted
                        // 同一驱动动作，但显式标注降级——产物摘要是标记
                        // 文本，不是模型摘要。
                        tracing::warn!(
                            run_id = %run_id,
                            turn,
                            cut_index = plan.cut_index,
                            summarized_tokens = plan.summarized_tokens,
                            retained_tokens = plan.retained_tokens,
                            context_tokens = report.context_tokens,
                            context_window = report.context_window,
                            "mid-run compaction DEGRADED to honest hard-truncation \
                             (the summarize request would exceed the summary model window)"
                        );
                        exchange = truncated;
                        tail_from = exchange.len();
                        compaction_seq += 1;
                    }
                    crate::compaction::CompactionOutcome::Failed { .. }
                    | crate::compaction::CompactionOutcome::NotTriggered => {}
                    crate::compaction::CompactionOutcome::Cancelled => {
                        // The cancellation won mid-compaction — the next
                        // loop-top gate settles the run through the ONE
                        // cancellation path (no second settle shape here).
                        continue 'turns;
                    }
                }
            }
            // R05-T05: establish the call's deadline once (see the
            // `call_deadline` declaration); a retrying iteration reuses the
            // shared deadline.
            if call_deadline.is_none() {
                call_deadline = Some(
                    lingxi_adapters::models::dispatch::unix_ms_now().saturating_add(
                        u64::try_from(self.model_call_tuning.total_budget.as_millis())
                            .unwrap_or(u64::MAX),
                    ),
                );
            }
            // The typed per-turn exchange (R05-T01): submission + complete
            // prior exchange of this run + the send-time tool snapshot.
            // R06-T01: `system_prompt` is the render of the session's frozen
            // context artifact when the compiler is wired (one build result
            // for the sent text AND the observation — R06-A01); without a
            // compiler wired it stays None (the pre-R06 test wiring — every
            // adapter omits the family system slot on None, codex uses its
            // documented default instruction).
            let turn_input = ModelTurnInput {
                submission,
                system_prompt: system_prompt.clone(),
                turn,
                prior: exchange.clone(),
                tools: tool_snapshot,
                deadline_unix_ms: call_deadline,
                // R05-T06: the chat path never carries aux-slot image
                // inputs or a per-call output-token bound (the family
                // defaults hold) — the incumbent wire is unchanged.
                images: Vec::new(),
                max_output_tokens: None,
            };
            // R05-T05: an already-exhausted call budget fails BEFORE the
            // queue wait opens — a loud non-retryable `budget_exceeded`
            // (another attempt under the same deadline starts already
            // spent, so the budget failure is never retryable).
            if lingxi_adapters::models::dispatch::remaining_budget_ms(call_deadline) == Some(0) {
                break RunFinish::Failed {
                    cause: FailureCause::ProviderFailed {
                        code: ErrorCode::BudgetExceeded.wire_name().to_string(),
                        retryable: false,
                    },
                };
            }
            // Admission (R03-T02): one model-call permit (global → agent →
            // session) held across the model I/O only. A quota failure is
            // a LOUD run failure — never an unbounded wait, never a fake
            // success. R03-T03: the QUEUED wait itself exits on cancel.
            // The permit sits in an Option so the in-loop terminal paths
            // (cancel / child-exit failure / stream-idle) can RELEASE it
            // before their settle awaits without a double-move (R05-T04:
            // the delta drain loop outlives any single drop point).
            let mut model_permit = match self
                .acquire_or_break(
                    &QuotaResource::Model,
                    agent_id,
                    quota_session_lane,
                    &run_id,
                    &root,
                    call_deadline,
                )
                .await
            {
                Ok(Some(permit)) => Some(permit),
                Ok(None) => {
                    // R05 RR1 F16: distinguish the deadline race from a
                    // genuine quota refusal by a fresh clock read — a
                    // budget spent IN THE QUEUE settles as the honest
                    // non-retryable budget failure (same shape as the
                    // pre-send check above), never a wait for the quota
                    // manager's own timeout on an already-spent call.
                    if lingxi_adapters::models::dispatch::remaining_budget_ms(call_deadline)
                        == Some(0)
                    {
                        break RunFinish::Failed {
                            cause: FailureCause::ProviderFailed {
                                code: ErrorCode::BudgetExceeded.wire_name().to_string(),
                                retryable: false,
                            },
                        };
                    }
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
            let provider_clone = Arc::clone(&provider);
            // R05-T04 (D6/D7): the live-delta channel of this call. The sink
            // moves INTO the supervised child future, so the sender drops
            // exactly when the turn resolves — the driver's receiver then
            // drains to None (the clean end-of-stream signal). A full
            // channel backpressures the provider's socket read (the sink's
            // emit awaits capacity); deltas are never dropped.
            let (delta_tx, mut delta_rx) =
                tokio::sync::mpsc::channel::<ModelTurnDelta>(TURN_STREAM_LIVE_DELTA_CAP);
            let model_child = self
                .tasks
                .spawn_linked(
                    run_id.as_str(),
                    &call_scope,
                    format!("model_call:{}", call.as_str()),
                    async move {
                        let sink = ChannelDeltaSink { tx: delta_tx };
                        provider_clone
                            .next_turn(&call_ctx, &call_id, &turn_input, &sink)
                            .await
                    },
                )
                .map_err(|rejected| DriveError::Storage(rejected.into()))?;
            // D6: `model_call_started` is durable BEFORE the first delta —
            // one call's event order is start → deltas → completed. The
            // started fact carries the driver-moment descriptor (a mid-call
            // reload micro-window vs the result's `served_by` is documented
            // in R05_INTERFACE_EVOLUTION).
            self.persist_model_call_started(
                port,
                events,
                &ctx,
                &call,
                &provider.descriptor(),
                now_ms,
            )
            .await?;
            let mut normalizer = DeltaNormalizer::new(turn);
            let mut delta_event_seq: u64 = 0;
            let mut model_wait = std::pin::pin!(model_child.wait());
            let provider_turn: lingxi_kernel::ports::ProviderTurnResult = 'call: loop {
                tokio::select! {
                    biased;
                    _ = root.cancelled() => {
                        drop(model_permit.take());
                        // R05 RR1 F38: the in-flight call may already have
                        // left the process — a possibly-billable request
                        // NEVER vanishes with the cancellation fence. The
                        // accounting row (outcome=cancelled, usage unknown,
                        // attempts UNKNOWN — the dropped provider future
                        // proves nothing about either fact) lands BEFORE
                        // the cancellation settle. The dispatch-moment
                        // descriptor is the honest identity: the result's
                        // `served_by` cannot exist, the turn never resolved.
                        // NO `model_call_completed` event: the A09/C16
                        // discipline keeps a cancelled call's started row
                        // unclosed (the run's own cancelled terminal closes
                        // the story) — the LEDGER row is the accounting
                        // fact, never a fabricated completion.
                        self.persist_model_call_cancelled_in_flight(
                            port,
                            &ctx,
                            &call,
                            &ModelCallUsageFacts {
                                usage: None,
                                usage_report:
                                    lingxi_kernel::usage::ReportedUsage::Unknown,
                                served_by: Some(provider.descriptor()),
                                served_protocol: None,
                                transport_attempts: None,
                                outcome:
                                    lingxi_kernel::usage::CallOutcome::Cancelled,
                                started_at_unix_ms: Some(turn_started_unix_ms),
                                emitted_tool_calls: Vec::new(),
                            },
                            &usage_ledger,
                            now_ms,
                        )
                        .await?;
                        // The receiver and the pinned wait drop on the way
                        // out: the provider's next emit fails closed and the
                        // scope ends the child at its await point (D8 — no
                        // wire-level cancel protocol).
                        let reason = root.reason().unwrap_or_else(|| "cancelled".to_string());
                        let finish = self
                            .settle_cancellation(port, events, &ctx, &entry, live_status, reason, now_ms)
                            .await?;
                        guard.disarm();
                        return Ok(finish);
                    }
                    exit = &mut model_wait => {
                        // The child resolved: drain every delta it emitted
                        // BEFORE its terminal — the receiver ends at None
                        // because the sender dropped with the child future.
                        let mut tail = Vec::new();
                        while let Some(delta) = delta_rx.recv().await {
                            tail.push(delta);
                        }
                        self.persist_delta_batch(
                            port,
                            events,
                            &ctx,
                            &call,
                            &mut normalizer,
                            &mut delta_event_seq,
                            &tail,
                            &mut saw_process_content,
                            now_ms,
                        )
                        .await?;
                        break 'call match exit {
                            Ok(turn) => turn,
                            Err(task_exit) => {
                                drop(model_permit.take());
                                // Supervision return (R03-T03 step 4): a child
                                // panic/abort/error is a LOUD provider failure —
                                // never a silent drop, never a fake reply.
                                tracing::error!(
                                    run_id = %run_id,
                                    model_call = %call,
                                    exit = task_exit.name(),
                                    "supervised model-call child ended without a turn; failing loudly"
                                );
                                break 'turns RunFinish::Failed {
                                    cause: FailureCause::ProviderFailed {
                                        code: format!(
                                            "model_call_child_{}",
                                            task_exit.name()
                                        ),
                                        retryable: false,
                                    },
                                };
                            }
                        };
                    }
                    tick = tokio::time::timeout(self.stream_idle_timeout, delta_rx.recv()) => {
                        match tick {
                            Ok(Some(first)) => {
                                // D6: ONE persist per drain batch — every
                                // immediately-available delta joins this
                                // transaction (no lock is held across the
                                // awaits; the storage port serializes).
                                let mut batch = vec![first];
                                while let Ok(delta) = delta_rx.try_recv() {
                                    batch.push(delta);
                                }
                                self.persist_delta_batch(
                                    port,
                                    events,
                                    &ctx,
                                    &call,
                                    &mut normalizer,
                                    &mut delta_event_seq,
                                    &batch,
                                    &mut saw_process_content,
                                    now_ms,
                                )
                                .await?;
                            }
                            Ok(None) => {
                                // The sender dropped without the exit arm
                                // firing first: the child future is gone and
                                // its result is delivered or in flight —
                                // take it (bounded: a cancelled/ended child
                                // resolves promptly).
                                let exit = (&mut model_wait).await;
                                break 'call match exit {
                                    Ok(turn) => turn,
                                    Err(task_exit) => {
                                        drop(model_permit.take());
                                        tracing::error!(
                                            run_id = %run_id,
                                            model_call = %call,
                                            exit = task_exit.name(),
                                            "supervised model-call child ended without a turn; failing loudly"
                                        );
                                        break 'turns RunFinish::Failed {
                                            cause: FailureCause::ProviderFailed {
                                                code: format!(
                                                    "model_call_child_{}",
                                                    task_exit.name()
                                                ),
                                                retryable: false,
                                            },
                                        };
                                    }
                                };
                            }
                            Err(_elapsed) => {
                                // The pre-registered stream idle bound: a
                                // stream that delivers nothing for this long
                                // is STALLED. Cancel the call scope (the
                                // provider's socket read drops at its await
                                // point, D8), reap the child, and settle the
                                // CALL as a retryable upstream failure — the
                                // run's attempt policy decides; the partial
                                // deltas already persisted stay in the log.
                                call_scope.cancel("stream idle timeout");
                                let _ = (&mut model_wait).await;
                                drop(model_permit.take());
                                break 'call lingxi_kernel::ports::ProviderTurnResult::of_ctx(
                                    &ctx,
                                    lingxi_kernel::ports::ProviderTurn::Failed {
                                        error: ProtocolError::new(
                                            ErrorCode::UpstreamUnavailable,
                                            format!(
                                                "model stream idle for {} ms (pre-registered \
                                                 http_idle_stream_timeout_ms): the stream is \
                                                 stalled; the call fails retryable",
                                                self.stream_idle_timeout.as_millis()
                                            ),
                                            true,
                                        ),
                                        retryable: true,
                                    },
                                );
                            }
                        }
                    }
                }
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
                        // R05 RR1 F38: the CONTENT is refused, the ACCOUNTING
                        // is not — the turn DID settle, so its row carries
                        // the REAL observed facts (usage, physical attempts,
                        // resolved identity) with outcome=cancelled, landing
                        // before the cancellation settle (the physical
                        // requests already happened; a fence never erases a
                        // billable fact — F-WU02). No completed event (the
                        // A09/C16 cancelled-call discipline).
                        self.persist_model_call_cancelled_in_flight(
                            port,
                            &ctx,
                            &call,
                            &ModelCallUsageFacts {
                                usage: provider_turn.usage.clone(),
                                usage_report: provider_turn.usage_report.clone(),
                                served_by: provider_turn
                                    .served_by
                                    .clone()
                                    .or_else(|| Some(provider.descriptor())),
                                served_protocol: provider_turn.served_protocol.clone(),
                                transport_attempts: Some(provider_turn.transport_attempts),
                                outcome: lingxi_kernel::usage::CallOutcome::Cancelled,
                                started_at_unix_ms: Some(turn_started_unix_ms),
                                emitted_tool_calls: Vec::new(),
                            },
                            &usage_ledger,
                            now_ms,
                        )
                        .await?;
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
            // R05-T04 (A09): the call's terminal closes the delta chain
            // BEFORE the completed fact — the tag parsers flush into the
            // current segments and the open segments end, all persisted
            // ahead of `model_call_completed`.
            //
            // R05 RR1 F12: the text segment's phase resolution needs the
            // turn's TERMINAL classification, so the turn is inspected
            // here (before the consuming match) — `final_answer` is only
            // confirmed when the provider turn is a real final carrying
            // visible answer text AFTER normalization (F13); anything else
            // (tools / process-only / failure) closes the text segment as
            // `unresolved`, never a guessed final_answer.
            //
            // R06-T02: the compaction trigger's primary signal — the last
            // RESOLVED call's real usage. Unknown/Invalid stays `None`
            // (the incumbent `!message.usage → no trigger` door; an
            // estimate never substitutes).
            // R06-T02 FIX-08（N-2）：现役触发门 `stopReason∉{error,aborted}`
            // ——失败 turn 的 Known usage 不捕获（失败调用的 usage 不触发
            // 压缩；aborted 臂在 Rust 由取消面结算，到不了这里）。
            last_turn_usage = match (&provider_result.turn, &provider_result.usage_report) {
                (lingxi_kernel::ports::ProviderTurn::Failed { .. }, _) => None,
                (_, lingxi_kernel::usage::ReportedUsage::Known(usage)) => Some(usage.clone()),
                _ => None,
            };
            let provider_turn = provider_result.turn;
            let normalized_final = match &provider_turn {
                lingxi_kernel::ports::ProviderTurn::Final { message } => {
                    let normalized =
                        crate::streaming_norm::normalize_final_message(message.clone());
                    if crate::streaming_norm::message_has_visible_text(&normalized) {
                        Some(normalized)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let finish_events = normalizer.finish(normalized_final.is_some());
            self.persist_norm_events(
                port,
                events,
                &ctx,
                &call,
                &mut delta_event_seq,
                finish_events,
                now_ms,
            )
            .await?;
            debug_assert!(
                delta_rx.try_recv().is_err(),
                "every delta of the resolved call was drained before its terminal persist"
            );
            // R05-T01: the provider-reported token usage (when the protocol
            // reports one) rides the call's `model_call_completed` fact;
            // None stays None — never a fabricated estimate.
            // R05-T07: the richer ledger facts travel with it — the
            // component-level usage fact (provenance + invalid-vs-
            // unknown), the protocol family that served the call and the
            // PHYSICAL request count (a 401-refresh resend is 2).
            let mut usage_facts = ModelCallUsageFacts {
                usage: provider_result.usage.clone(),
                usage_report: provider_result.usage_report.clone(),
                // The result's served route wins; a double that reports
                // none falls back to the port descriptor (exact for a
                // deterministic double).
                served_by: provider_result
                    .served_by
                    .clone()
                    .or_else(|| Some(provider.descriptor())),
                served_protocol: provider_result.served_protocol.clone(),
                // A resolved turn always observed its physical count (F38:
                // None is exclusively the dropped-before-settlement shape).
                transport_attempts: Some(provider_result.transport_attempts),
                // R05 RR1 F21: the outcome of the call — a Failed turn is
                // a failed call even when its usage is known; every other
                // terminal (Final/ToolRequests/Continue/Empty) settled
                // the call itself successfully.
                outcome: if matches!(
                    provider_turn,
                    lingxi_kernel::ports::ProviderTurn::Failed { .. }
                ) {
                    lingxi_kernel::usage::CallOutcome::Failed
                } else {
                    lingxi_kernel::usage::CallOutcome::Succeeded
                },
                started_at_unix_ms: Some(turn_started_unix_ms),
                emitted_tool_calls: Vec::new(),
            };
            match provider_turn {
                lingxi_kernel::ports::ProviderTurn::Final { message } => {
                    if message.content.is_empty() {
                        // "Final" with zero content blocks is the empty
                        // reply — no empty final message is committed. The
                        // call's facts still CLOSE (R05-T04: the started row
                        // is durable since the call's dispatch, so the
                        // completed event must follow — an unclosed started
                        // row is a recovery false-positive).
                        self.persist_model_call_completed(
                            port,
                            events,
                            &ctx,
                            &call,
                            &usage_facts,
                            &usage_ledger,
                            now_ms,
                        )
                        .await?;
                        break finish_no_final(saw_tool_failure, saw_process_content);
                    }
                    // R05 RR1 F12/F13: the committed final message is the
                    // NORMALIZED projection (the same scanner the live
                    // delta chain uses) — think-family tags structure as
                    // reasoning, mood-family content never returns as
                    // displayable body, fenced/escaped literals stay text.
                    // A final whose blocks carry NO visible answer text
                    // after normalization (mood-only body, reasoning-only
                    // content that slipped past an adapter) is process
                    // content: no final message is committed and the run
                    // settles the honest no-final terminal.
                    let Some(message) = normalized_final else {
                        self.persist_model_call_completed(
                            port,
                            events,
                            &ctx,
                            &call,
                            &usage_facts,
                            &usage_ledger,
                            now_ms,
                        )
                        .await?;
                        saw_process_content = true;
                        break finish_no_final(saw_tool_failure, saw_process_content);
                    };
                    self.persist_model_call_completed(
                        port,
                        events,
                        &ctx,
                        &call,
                        &usage_facts,
                        &usage_ledger,
                        now_ms,
                    )
                    .await?;
                    break RunFinish::CompletedWithFinal { message };
                }
                lingxi_kernel::ports::ProviderTurn::ToolRequests { requests, content } => {
                    // R05-T05: the turn RESOLVED (the conversation moves to
                    // tool execution) — the next loop iteration is a new
                    // logical model call with a fresh total budget.
                    call_deadline = None;
                    if requests.is_empty() {
                        // A tool-request turn with zero calls is a protocol
                        // violation of the double/adapter, not "no tools
                        // needed": loud failure. The call's facts close
                        // first (R05-T04: the started row is durable since
                        // dispatch).
                        self.persist_model_call_completed(
                            port,
                            events,
                            &ctx,
                            &call,
                            &usage_facts,
                            &usage_ledger,
                            now_ms,
                        )
                        .await?;
                        break RunFinish::Failed {
                            cause: FailureCause::ProviderFailed {
                                code: "empty_tool_request_list".to_string(),
                                retryable: false,
                            },
                        };
                    }
                    // R05-T01: pre-mint the tool call ids of EVERY request of
                    // this turn before any execution (minting order ==
                    // execution order, so the tc0001.. sequence is exactly
                    // the one the per-iteration minting produced before).
                    // The exchange history's assistant turn pairs each
                    // request with the host identity the driver binds it to
                    // PLUS the provider's own correlation id.
                    let planned: Vec<(lingxi_protocol::ToolCallId, RequestedToolCall)> = requests
                        .iter()
                        .map(|request| {
                            tool_call_seq += 1;
                            let planned_id = tool_call_id(&run_id, tool_call_seq);
                            (
                                planned_id.clone(),
                                RequestedToolCall {
                                    tool_call_id: planned_id,
                                    provider_call_id: request.provider_call_id.clone(),
                                    target: request.target.clone(),
                                    arguments: request.arguments.clone(),
                                    args_digest: request.args_digest.clone(),
                                    args_summary: request.args_summary.clone(),
                                },
                            )
                        })
                        .collect();
                    // R05 RR1 F21: the parent-side JOIN listing — this
                    // call's row names the tool call ids its batch emitted
                    // so a worker-callback child (parent_tool_call_id) can
                    // reach its parent MODEL call row inside the ledger.
                    usage_facts.emitted_tool_calls = planned
                        .iter()
                        .map(|(id, _)| id.as_str().to_string())
                        .collect();
                    exchange.push(ExchangeItem::AssistantTurn {
                        call: call.clone(),
                        content,
                        tool_calls: planned
                            .iter()
                            .map(|(_, requested)| requested.clone())
                            .collect(),
                        // R05 RR1 F09: the turn's provider-opaque state is
                        // bound to the provider/model that served it — the
                        // renderer refuses to forward it onto any other
                        // route identity.
                        origin: turn_origin_of(&usage_facts),
                    });
                    // R06-T02: the tail of the compaction trigger restarts
                    // after every assistant turn — only the tool results
                    // beyond this point count as post-usage context.
                    tail_from = exchange.len();
                    self.persist_model_call_completed(
                        port,
                        events,
                        &ctx,
                        &call,
                        &usage_facts,
                        &usage_ledger,
                        now_ms,
                    )
                    .await?;
                    // F03-C04: the model-event storage boundary — a
                    // cancellation accepted during the persist ends the
                    // turn loop here (no tool admission, no intent write).
                    gate_cancel!();
                    // R04-T02: with a gateway wired the gateway itself
                    // holds the executors (the raw port is not consulted
                    // on this wiring); without one the R03 shape stands.
                    let tools = if self.tool_gateway.is_some() {
                        None
                    } else {
                        let Some(tools) = self.tools.clone() else {
                            break RunFinish::Failed {
                                cause: FailureCause::ToolExecutorUnavailable,
                            };
                        };
                        Some(tools)
                    };
                    let tool_gateway = self.tool_gateway.clone();
                    // ── R05 RR1 F11: whole-batch admission BEFORE the first
                    //     side effect ──
                    // A model tool turn is admitted as ONE batch: protocol
                    // completeness and per-request shapes were settled by
                    // the adapter; HERE the driver confirms the batch-level
                    // facts that no per-request check covers — (a) the
                    // provider call ids are unambiguous within THIS model
                    // call (defense in depth over the adapters' admission:
                    // a direct TurnProviderPort double bypasses it), and
                    // (b) EVERY request passes the static admission (digest
                    // gate + gateway target/availability/generations/
                    // CURRENT-schema/policy verdict — the pure validation
                    // half of `prepare`, no record minted).
                    //
                    // A batch that fails ANY of this executes NOTHING: each
                    // request closes as a never-dispatched failure and the
                    // structured refusals travel back to the model (which
                    // can correct and retry). An identical same-id re-send
                    // collapses to its first occurrence (one execution, one
                    // result under the shared id). This pre-validation does
                    // NOT replace the immediate authorization: the
                    // per-request execution below still runs the digest
                    // gate and the FULL gateway prepare (plus permission,
                    // approval, cancellation and generation re-checks) at
                    // the moment of dispatch.
                    let mut batch_collapsed = vec![false; requests.len()];
                    // Per-slot refusal: the structured outcome (rides to the
                    // model with its OWN error code) + the journal receipt
                    // detail (keeps the incumbent per-refusal diagnostic
                    // vocabulary the R04 receipts pinned).
                    let mut batch_refusal: Vec<Option<(ToolOutcome, String)>> =
                        vec![None; requests.len()];
                    let mut batch_failed = false;
                    {
                        let mut seen: std::collections::HashMap<&str, (usize, &str, &str)> =
                            std::collections::HashMap::new();
                        for (index, request) in requests.iter().enumerate() {
                            let Some(id) = request.provider_call_id.as_deref() else {
                                continue;
                            };
                            let shape = (request.target.as_str(), request.args_digest.hex.as_str());
                            match seen.get(id) {
                                None => {
                                    seen.insert(id, (index, shape.0, shape.1));
                                }
                                Some(&(_, first_target, first_digest))
                                    if first_target == shape.0 && first_digest == shape.1 =>
                                {
                                    // Identical completed re-send under the
                                    // same id: no new information — the first
                                    // occurrence executes once.
                                    batch_collapsed[index] = true;
                                }
                                Some(&(first, ..)) => {
                                    let outcome = ToolOutcome::Failed {
                                        error: ProtocolError::new(
                                            ErrorCode::InvalidMessage,
                                            format!(
                                                "the model turn carries the provider call id \
                                                 {id:?} twice with DIFFERENT shapes (request #{} \
                                                 vs #{}): the batch is ambiguous — nothing is \
                                                 admitted or dispatched",
                                                first + 1,
                                                index + 1
                                            ),
                                            false,
                                        ),
                                    };
                                    let detail = "not dispatched: whole-batch admission refusal \
                                         (duplicate provider call id with conflicting shapes)"
                                        .to_string();
                                    for slot in batch_refusal.iter_mut() {
                                        *slot = Some((outcome.clone(), detail.clone()));
                                    }
                                    batch_failed = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !batch_failed {
                        let mut own_refusals: Vec<Option<(ToolOutcome, String)>> =
                            vec![None; requests.len()];
                        for (index, request) in requests.iter().enumerate() {
                            if batch_collapsed[index] {
                                continue;
                            }
                            let call_id = planned[index].0.clone();
                            if !request.digest_matches_arguments() {
                                own_refusals[index] = Some((
                                    ToolOutcome::Failed {
                                        error: ProtocolError::new(
                                            ErrorCode::InvalidMessage,
                                            format!(
                                                "args_digest mismatch (declared {}, computed \
                                                 {}): forged or adulterated request; not \
                                                 dispatched",
                                                request.args_digest.hex,
                                                request.arguments.digest().hex
                                            ),
                                            false,
                                        ),
                                    },
                                    format!(
                                        "not dispatched: args_digest mismatch (declared {}, \
                                         computed {})",
                                        request.args_digest.hex,
                                        request.arguments.digest().hex
                                    ),
                                ));
                                continue;
                            }
                            let Some(gateway) = tool_gateway.as_ref() else {
                                continue;
                            };
                            let surface = if request.delegation.is_some() {
                                crate::toolgateway::CallerSurface::DelegationDispatch
                            } else if matches!(authorization.grant, RunGrant::Subagent { .. }) {
                                crate::toolgateway::CallerSurface::SubagentRun
                            } else {
                                crate::toolgateway::CallerSurface::UserRun
                            };
                            let permission = authorization.invocation_permission_context();
                            if let Err(refusal) = gateway.validate_from_request(
                                &ctx, surface, agent_id, permission, &call_id, request,
                            ) {
                                // The refusal keeps its OWN structured code
                                // and diagnostic (the model must see the
                                // real reason: policy denial, schema
                                // violation, unknown target, ...).
                                own_refusals[index] = Some((
                                    ToolOutcome::Failed {
                                        error: refusal.to_tool_error(),
                                    },
                                    format!(
                                        "not dispatched: gateway refused the preparation \
                                         ({refusal})"
                                    ),
                                ));
                            }
                        }
                        let failed: Vec<usize> = own_refusals
                            .iter()
                            .enumerate()
                            .filter_map(|(i, r)| r.is_some().then_some(i))
                            .collect();
                        if !failed.is_empty() {
                            let failed_list = failed
                                .iter()
                                .map(|i| format!("#{}", i + 1))
                                .collect::<Vec<_>>()
                                .join(", ");
                            let sibling = (
                                ToolOutcome::Failed {
                                    error: ProtocolError::new(
                                        ErrorCode::InvalidMessage,
                                        format!(
                                            "another request of this model turn ({failed_list}) \
                                             failed the batch admission: the whole batch stays \
                                             at zero dispatch — correct the request(s) and retry"
                                        ),
                                        false,
                                    ),
                                },
                                format!(
                                    "not dispatched: whole-batch admission refusal (another \
                                     request of this model turn ({failed_list}) failed the \
                                     batch admission)"
                                ),
                            );
                            for (index, refusal) in own_refusals.into_iter().enumerate() {
                                batch_refusal[index] =
                                    Some(refusal.unwrap_or_else(|| sibling.clone()));
                            }
                        }
                    }
                    for (index, (request, (planned_id, _))) in
                        requests.iter().zip(planned.iter()).enumerate()
                    {
                        // F03-C04: each tool-loop iteration re-checks — a
                        // cancellation accepted during the previous
                        // iteration's receipt/event writes stops the loop
                        // before any new admission or intent.
                        gate_cancel!();
                        let call_id = planned_id.clone();
                        if batch_collapsed[index] {
                            // The identical re-send of an earlier request:
                            // its result is the first occurrence's result —
                            // no second journal entry, no second execution.
                            continue;
                        }
                        if let Some((outcome, receipt_detail)) = &batch_refusal[index] {
                            tracing::warn!(
                                run_id = %run_id,
                                tool_call = %call_id,
                                target = request.target,
                                "whole-batch admission refused this model tool turn (zero \
                                 dispatch); the journal closes a never-dispatched failure"
                            );
                            let trusted_digest = request.arguments.digest();
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
                            port.record_invocation_receipt(
                                &ctx,
                                &call_id,
                                InvocationReceipt {
                                    outcome: ReceiptOutcome::Failed,
                                    detail: receipt_detail.clone(),
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
                                Some(outcome),
                                now_ms,
                            )
                            .await?;
                            record_tool_result!(call_id, request, outcome.clone());
                            continue;
                        }
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
                                // The tool lane has no model-call budget;
                                // its queue wait is cancellation-bounded
                                // (R03-T03 semantics, unchanged).
                                None,
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
                            record_tool_result!(call_id, request, outcome);
                            drop(tool_permit);
                            continue;
                        }
                        // ── R04-T02: the unified gateway PREPARE stage ──
                        // Target identity / availability / generations /
                        // CURRENT-schema arguments / the tool-policy
                        // verdict all settle HERE, before the intent is
                        // journaled and long before any `started` write:
                        // a refusal or an approval requirement can never
                        // leave a started-without-dispatch entry behind.
                        // The principal facts come from the driver's
                        // trusted context — never from the model's
                        // arguments (R04-A04).
                        let prepared = match tool_gateway.as_ref() {
                            Some(gateway) => {
                                let surface = if request.delegation.is_some() {
                                    crate::toolgateway::CallerSurface::DelegationDispatch
                                } else if matches!(authorization.grant, RunGrant::Subagent { .. }) {
                                    crate::toolgateway::CallerSurface::SubagentRun
                                } else {
                                    crate::toolgateway::CallerSurface::UserRun
                                };
                                // R04-T03: the permission context is the
                                // driver's trusted snapshot (grant tier +
                                // session mode) — the model never supplies
                                // it.
                                let permission = authorization.invocation_permission_context();
                                match gateway.prepare_from_request(
                                    &ctx, surface, agent_id, permission, &call_id, request,
                                ) {
                                    Ok(prepared) => Some(prepared),
                                    Err(refusal) => {
                                        tracing::warn!(
                                            run_id = %run_id,
                                            tool_call = %call_id,
                                            target = request.target,
                                            code = refusal.code(),
                                            "tool gateway refused the preparation (zero \
                                             dispatch); the journal closes a never-dispatched \
                                             failure"
                                        );
                                        // Same write-order shape as the
                                        // digest gate above: the intent
                                        // lands FIRST (a receipt may only
                                        // close an entry that exists),
                                        // binding the trusted digest.
                                        port.record_invocation_intent(
                                            &ctx,
                                            InvocationIntent {
                                                journal_id: call_id.clone(),
                                                target: request.target.clone(),
                                                args_digest: request.arguments.digest().hex,
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
                                            error: refusal.to_tool_error(),
                                        };
                                        port.record_invocation_receipt(
                                            &ctx,
                                            &call_id,
                                            InvocationReceipt {
                                                outcome: ReceiptOutcome::Failed,
                                                detail: format!(
                                                    "not dispatched: gateway refused the \
                                                     preparation ({refusal})"
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
                                        record_tool_result!(call_id, request, outcome);
                                        drop(tool_permit);
                                        continue;
                                    }
                                }
                            }
                            None => None,
                        };
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
                        // R04-T02-R1-F01 repair: on the gateway wiring
                        // the kernel vocabularies (the anti-recursion
                        // blocklist AND the read-only allow-list) judge
                        // the prepared LOCAL NAME as the AUTHORITY — the
                        // registry target id is matched against the
                        // blocklist only, as depth defense. The
                        // pre-repair order judged the namespaced id
                        // against the bare-name read-only allow-list
                        // FIRST, which blanket-denied every registered
                        // Read-class target for a read-only child (the
                        // local-name re-check was unreachable) — an
                        // unregistered regression of the protected
                        // "explicit read attenuation keeps the research
                        // surface open" semantics. Both protections hold
                        // at once: write/unknown local names stay denied
                        // under the read-only tier, and the delegation
                        // family stays blocked through EITHER vocabulary.
                        // Without a gateway the R03 bare-name judgment
                        // stands unchanged.
                        let authorization_decision = match authorization.grant {
                            RunGrant::Full => ToolAuthorization::Allowed,
                            RunGrant::Subagent { tier } => match prepared.as_ref() {
                                Some(prepared) => authorize_child_tool_with_registry_id(
                                    tier,
                                    prepared.local_name.as_str(),
                                    request.target.as_str(),
                                ),
                                None => authorize_child_tool(tier, request.target.as_str()),
                            },
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
                            record_tool_result!(call_id, request, outcome);
                            drop(tool_permit);
                            continue;
                        }
                        // ── the approval requirement (R04-T02) ──
                        // On the gateway wiring the requirement comes from
                        // the ADJUDICATED POLICY VERDICT: `Allowed` means
                        // the configured policy service already allowed —
                        // asking a human again would be a duplicate prompt
                        // (master prompt §4.2: no double prompting), so the
                        // call advances to authorized directly.
                        // `NeedsApproval` routes to the approval surface;
                        // when NONE is wired the call is REFUSED with zero
                        // dispatch (the incumbent's TOOL_APPROVAL_UNAVAILABLE
                        // posture — an unconfigured mechanism never
                        // auto-allows an execution-class call). Without a
                        // gateway the R03 shape stands: a wired gate is
                        // always asked, no gate means the run-layer
                        // authorization context itself authorizes.
                        let approval_requirement: Option<String> =
                            if let Some(prepared) = prepared.as_ref() {
                                match &prepared.policy {
                                    crate::toolgateway::PolicyVerdict::NeedsApproval { reason } => {
                                        Some(reason.clone())
                                    }
                                    _ => None,
                                }
                            } else if self.approval.is_some() {
                                Some("approval gate wired (R03 minimal interface)".to_string())
                            } else {
                                None
                            };
                        match approval_requirement {
                            None => {
                                port.advance_invocation(
                                    &ctx,
                                    &call_id,
                                    InvocationPhase::Authorized,
                                    now_ms,
                                )
                                .await
                                .map_err(DriveError::Storage)?;
                            }
                            Some(requirement) => {
                                // No approval surface is wired at all: the
                                // requirement is real but cannot be served —
                                // a structured, never-dispatched refusal.
                                let Some(gate) = self.approval.clone() else {
                                    tracing::warn!(
                                        run_id = %run_id,
                                        tool_call = %call_id,
                                        target = request.target,
                                        "tool invocation needs approval but no approval \
                                         surface is wired: TOOL_APPROVAL_UNAVAILABLE (zero \
                                         dispatch, never an auto-allow)"
                                    );
                                    saw_tool_failure = true;
                                    saw_process_content = true;
                                    let outcome = ToolOutcome::Failed {
                                        error: ProtocolError::new(
                                            ErrorCode::Forbidden,
                                            format!(
                                                "tool approval unavailable: this invocation \
                                                 needs approval ({requirement}) but no \
                                                 approval surface is configured; switch the \
                                                 session to a mode that can prompt and retry — \
                                                 the action was not run"
                                            ),
                                            false,
                                        ),
                                    };
                                    port.record_invocation_receipt(
                                        &ctx,
                                        &call_id,
                                        InvocationReceipt {
                                            outcome: ReceiptOutcome::Failed,
                                            detail: format!(
                                                "not dispatched: approval required but \
                                                 unavailable: {requirement}"
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
                                    record_tool_result!(call_id, request, outcome);
                                    drop(tool_permit);
                                    continue;
                                };
                                let _ = requirement;
                                // ── approval wait (R03-T03 interface) ──
                                // F03-C04: asking a human is an external
                                // interaction too — a cancellation accepted
                                // at the authorization boundary must not
                                // open a new approval round trip.
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
                                // R04-T03: the approver sees the SHAPE-ONLY
                                // summary — derived from the wire
                                // arguments when the request carried none
                                // (values never leak into the approval
                                // surface; `summarize_arguments` is keys
                                // + type tags only).
                                let gate_req = ApprovalRequest {
                                    tool_call_id: call_id.clone(),
                                    target: request.target.clone(),
                                    args_digest: request.args_digest.hex.clone(),
                                    args_summary: request.args_summary.clone().or_else(|| {
                                        Some(lingxi_kernel::toolcatalog::summarize_arguments(
                                            &request.arguments,
                                        ))
                                    }),
                                    // R04-T04: the approval RECORD binds the
                                    // REAL resource scopes the gateway
                                    // derived at preparation (canonical
                                    // authorized paths + operations) — the
                                    // approver approves exactly this scope.
                                    resources: prepared
                                        .as_ref()
                                        .map(|prepared| prepared.resources.clone())
                                        .unwrap_or_default(),
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
                                    ApprovalDecision::Rejected { .. }
                                    | ApprovalDecision::Aborted => {
                                        // ZERO executions — the rejection is a
                                        // recorded tool failure, not a silent
                                        // skip (frozen incumbent: the wrapper
                                        // returns toolError with 执行 0 次). The
                                        // journal closes the invocation as a
                                        // never-dispatched failure.
                                        let reason = match decision {
                                            ApprovalDecision::Rejected { reason } => reason,
                                            ApprovalDecision::Aborted => {
                                                "approval aborted".to_string()
                                            }
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
                                        record_tool_result!(call_id, request, outcome);
                                        drop(tool_permit);
                                        continue;
                                    }
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
                            // R04-T02: on the gateway wiring the request's
                            // target is the REGISTRY TARGET ID; the family
                            // match uses the prepared LOCAL NAME (the same
                            // name the catalog registered), falling back to
                            // the raw target for the legacy wiring. The
                            // delegation family went through the SAME
                            // gateway prepare (identity / availability /
                            // generations / current-schema arguments /
                            // policy) as every other target above — a
                            // special run mechanism is not a gateway bypass.
                            // R04-T02-R1-F01 same-root-cause closure: the
                            // family match by LOCAL NAME is restricted to
                            // FIRST-PARTY targets — a plugin/MCP-origin
                            // registration whose local name collides with
                            // the family is a DIFFERENT target and must
                            // take the loud InvalidTarget refusal below,
                            // never the real child-run launcher. The legacy
                            // wiring (no registry) keeps the bare-name
                            // family exactly as before.
                            let delegation_target: &str = prepared
                                .as_ref()
                                .map(|p| p.local_name.as_str())
                                .unwrap_or(request.target.as_str());
                            let delegation_first_party = match prepared.as_ref() {
                                Some(prepared) => {
                                    matches!(
                                        prepared.origin,
                                        lingxi_kernel::toolcatalog::ToolOrigin::FirstParty
                                    )
                                }
                                None => true,
                            };
                            // `subagent_close` creates no run: its success
                            // carries the closed thread id as the content
                            // digest, its refusals take the shared failure
                            // shape below.
                            let launched: Result<
                                Option<String>,
                                crate::subagents::SubagentDispatchError,
                            > = if delegation_target == "subagent_close" && delegation_first_party {
                                match launcher
                                    .as_ref()
                                    .map(|launcher| launcher.close(&parent, &delegation))
                                {
                                    Some(Ok(closed)) => Ok(Some(closed.thread_id)),
                                    Some(Err(err)) => Err(err),
                                    None => Err(crate::subagents::SubagentDispatchError::NotBound),
                                }
                            } else if delegation_first_party {
                                let Some(launcher) = launcher else {
                                    return Err(DriveError::Internal(
                                        "subagent launcher not bound in this supervisor"
                                            .to_string(),
                                    ));
                                };
                                match delegation_target {
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
                            } else {
                                // A delegation payload on a NON-first-party
                                // target is the same protocol violation —
                                // the local-name collision does not make a
                                // plugin/MCP tool the child-run mechanism.
                                Err(crate::subagents::SubagentDispatchError::InvalidTarget(
                                    delegation_target.to_string(),
                                ))
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
                            record_tool_result!(call_id, request, outcome);
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
                        // R04-T02: on the gateway wiring the dispatch is
                        // the gateway's re-verified, single-use
                        // `execute_prepared` (the ONLY path to the bound
                        // executors); its Err(refusal) means ZERO dispatch
                        // and is journaled as a never-dispatched failure
                        // below. The legacy wiring keeps the raw port
                        // shape byte-for-byte.
                        let tool_scope = root.child(
                            format!("tool_call:{}", call_id.as_str()),
                            crate::cancel::ScopeKind::ToolCall,
                        );
                        let tool_ctx = ctx.clone();
                        let exec_call_id = call_id.clone();
                        let tool_child = match tool_gateway.as_ref() {
                            Some(gateway) => {
                                let gateway = Arc::clone(gateway);
                                let handle = prepared
                                    .as_ref()
                                    .expect("the gateway wiring prepared this call")
                                    .handle
                                    .clone();
                                self.tasks
                                    .spawn_linked(
                                        run_id.as_str(),
                                        &tool_scope,
                                        format!("tool_call:{}", call_id.as_str()),
                                        async move {
                                            gateway
                                                .execute_prepared(&tool_ctx, &exec_call_id, &handle)
                                                .await
                                        },
                                    )
                                    .map_err(|rejected| DriveError::Storage(rejected.into()))?
                            }
                            None => {
                                let tools_clone = Arc::clone(tools.as_ref().expect(
                                    "the legacy wiring checked the executor at turn entry",
                                ));
                                let tool_request = request.clone();
                                self.tasks
                                    .spawn_linked(
                                        run_id.as_str(),
                                        &tool_scope,
                                        format!("tool_call:{}", call_id.as_str()),
                                        async move {
                                            Ok(tools_clone
                                                .execute(&tool_ctx, &exec_call_id, &tool_request)
                                                .await)
                                        },
                                    )
                                    .map_err(|rejected| DriveError::Storage(rejected.into()))?
                            }
                        };
                        let executed: Result<
                            ToolExecutionResult,
                            crate::toolgateway::GatewayRefusal,
                        > = tokio::select! {
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
                                    Ok(ToolExecutionResult::of_ctx(
                                        &ctx,
                                        ToolOutcome::Unknown { reason },
                                    ))
                                }
                            },
                        };
                        // R04-T02: a gateway execution-time refusal (handle
                        // spent/expired/foreign, target disabled or updated
                        // between prepare and execute, no bound executor)
                        // dispatched NOTHING: the journal closes a
                        // never-dispatched failure with the refusal's own
                        // vocabulary — never a dispatched-looking receipt.
                        let outcome = match executed {
                            Err(refusal) => {
                                tracing::warn!(
                                    run_id = %run_id,
                                    tool_call = %call_id,
                                    target = request.target,
                                    code = refusal.code(),
                                    "tool gateway refused the execution (zero dispatch)"
                                );
                                saw_tool_failure = true;
                                saw_process_content = true;
                                let outcome = ToolOutcome::Failed {
                                    error: refusal.to_tool_error(),
                                };
                                port.record_invocation_receipt(
                                    &ctx,
                                    &call_id,
                                    InvocationReceipt {
                                        outcome: ReceiptOutcome::Failed,
                                        detail: format!(
                                            "not dispatched: gateway refused the execution \
                                             ({refusal})"
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
                                record_tool_result!(call_id, request, outcome);
                                drop(tool_permit);
                                continue;
                            }
                            Ok(result) => {
                                // R03-T04 result fence: same write-side
                                // identity check as model calls. A fenced
                                // tool result is audited; a live run records
                                // the started call as Unknown (the receipt
                                // is not trustworthy — never retried
                                // blindly, never a success).
                                match self.fence_verdict(&root, &result.fence, &ctx) {
                                    FenceVerdict::Current => result.outcome,
                                    FenceVerdict::Stale(reason) => {
                                        self.audit_late_result(
                                            port,
                                            &ctx,
                                            &result.fence,
                                            reason,
                                            vec!["tool_call_result".to_string()],
                                            now_ms,
                                        )
                                        .await;
                                        if root.is_cancelled() {
                                            drop(tool_permit);
                                            let cancel_reason = root
                                                .reason()
                                                .unwrap_or_else(|| "cancelled".to_string());
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
                        record_tool_result!(call_id, request, outcome);
                        drop(tool_permit);
                    }
                }
                lingxi_kernel::ports::ProviderTurn::Continue { process_note } => {
                    // R05-T05: a process-only turn RESOLVED — the next loop
                    // iteration is a new logical model call with a fresh
                    // total budget.
                    call_deadline = None;
                    saw_process_content = true;
                    // R05-T01: a process-only turn is still an exchange item
                    // — the next turn's typed input carries the reasoning
                    // block (never dropped silently).
                    exchange.push(ExchangeItem::AssistantTurn {
                        call: call.clone(),
                        content: vec![ContentBlock::Reasoning { text: process_note }],
                        tool_calls: Vec::new(),
                        origin: turn_origin_of(&usage_facts),
                    });
                    // R06-T02: the tail of the compaction trigger restarts
                    // after every assistant turn (a process-only turn too).
                    tail_from = exchange.len();
                    self.persist_model_call_completed(
                        port,
                        events,
                        &ctx,
                        &call,
                        &usage_facts,
                        &usage_ledger,
                        now_ms,
                    )
                    .await?;
                }
                lingxi_kernel::ports::ProviderTurn::Empty { .. } => {
                    self.persist_model_call_completed(
                        port,
                        events,
                        &ctx,
                        &call,
                        &usage_facts,
                        &usage_ledger,
                        now_ms,
                    )
                    .await?;
                    break finish_no_final(saw_tool_failure, saw_process_content);
                }
                lingxi_kernel::ports::ProviderTurn::Failed { error, retryable } => {
                    self.persist_model_call_completed(
                        port,
                        events,
                        &ctx,
                        &call,
                        &usage_facts,
                        &usage_ledger,
                        now_ms,
                    )
                    .await?;
                    // R05-T05: a 429's `Retry-After` hint (carried in the
                    // error details by the dispatch layer) OVERRIDES the
                    // computed backoff; the call's shared deadline VETOES
                    // both — a retry whose backoff cannot finish inside the
                    // budget settles as the ORIGINAL failure, never as a
                    // sleep into a guaranteed expiry.
                    let retry_after_ms = error
                        .details
                        .as_ref()
                        .and_then(|details| {
                            details.get(lingxi_adapters::models::dispatch::RETRY_AFTER_MS_DETAIL)
                        })
                        .and_then(serde_json::Value::as_u64);
                    if retryable && attempt_seq < self.limits.max_attempts {
                        // Retry = NEW ATTEMPT on the SAME run (the run id is
                        // fixed at creation; a provider reconnect never
                        // becomes a new user task). R05-T03 (C12): the retry
                        // KEEPS the run's confirmed exchange — every tool
                        // exchange that already executed is durable fact and
                        // real model history, so the retried attempt's first
                        // call receives it (a retry never rebuilds from the
                        // bare submission and never re-executes a confirmed
                        // write). The FAILED model call itself pushed no
                        // assistant turn, so nothing fabricated enters.
                        let delay_ms = retry_after_ms.unwrap_or_else(|| {
                            u64::try_from(
                                self.model_call_tuning
                                    .backoff_delay(attempt_seq)
                                    .as_millis(),
                            )
                            .unwrap_or(u64::MAX)
                        });
                        let vetoed = call_deadline.is_some_and(|deadline| {
                            lingxi_adapters::models::dispatch::unix_ms_now()
                                .saturating_add(delay_ms)
                                > deadline
                        });
                        if vetoed {
                            break RunFinish::Failed {
                                cause: FailureCause::ProviderFailed {
                                    code: error.code.wire_name().to_string(),
                                    retryable,
                                },
                            };
                        }
                        // The backoff sleep is a cancel-aware select arm: a
                        // cancellation accepted mid-backoff settles through
                        // the four-phase path NOW, never after the
                        // remaining delay. The quota permit is already
                        // released (the call's I/O ended above), so the
                        // sleep blocks no other run.
                        if delay_ms > 0 {
                            tokio::select! {
                                biased;
                                _ = root.cancelled() => {
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
                                _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => {}
                            }
                        }
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

    /// R05-T04 (D6): persists `model_call_started` for one call — durable
    /// BEFORE the first delta of the call is written, so one call's durable
    /// event order is start → deltas → completed. The descriptor is the
    /// driver-moment provider identity (the result-side `served_by` cannot
    /// exist yet; the reload micro-window is documented in
    /// R05_INTERFACE_EVOLUTION).
    async fn persist_model_call_started<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        call: &ModelCallId,
        descriptor: &lingxi_kernel::ports::ProviderDescriptor,
        now_ms: u64,
    ) -> Result<(), DriveError> {
        let run_id = ctx.run_id.to_string();
        let committed = port
            .record_run_events(
                ctx,
                vec![KeyEvent {
                    event_id: EventId::new(format!("{run_id}-{}-start", call.as_str())),
                    payload: EventPayload::Known(KnownEventPayload::ModelCallStarted(
                        ModelCallStartedPayload {
                            model_call_id: call.clone(),
                            provider: descriptor.provider.clone(),
                            model: descriptor.model.clone(),
                            operation: descriptor.operation.clone(),
                        },
                    )),
                }],
                now_ms,
            )
            .await
            .map_err(DriveError::Storage)?;
        events.publish_committed(&committed.events);
        Ok(())
    }

    /// R05-T04/T07: persists `model_call_completed` for one call — the
    /// closing fact of the started row, carrying the usage the provider
    /// ACTUALLY reported (None = the protocol/double reported none — never
    /// a fabricated estimate). Written only after every delta of the call
    /// is durable (A09).
    ///
    /// R05-T07 (C10): the usage-ledger row commits FIRST — the completion
    /// event is only recorded and published after the accounting fact is
    /// durable (a DB failure never publishes success; the idempotent
    /// ledger replay makes the driver's retry safe).
    #[allow(clippy::too_many_arguments)]
    async fn persist_model_call_completed<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        call: &ModelCallId,
        facts: &ModelCallUsageFacts,
        ledger: &UsageLedgerContext,
        now_ms: u64,
    ) -> Result<(), DriveError> {
        let run_id = ctx.run_id.to_string();
        // The wire projection and the ledger fact must agree: a Known
        // report projects its own wire record; anything else keeps the
        // legacy projection (doubles) or None.
        let usage = match &facts.usage_report {
            lingxi_kernel::usage::ReportedUsage::Known(usage) => usage.wire_record(),
            _ => facts.usage.clone(),
        };
        let record = model_call_usage_record_of(ctx, call, facts, ledger, now_ms);
        port.record_model_call_usage(record, now_ms)
            .await
            .map_err(DriveError::Storage)?;
        let committed = port
            .record_run_events(
                ctx,
                vec![KeyEvent {
                    event_id: EventId::new(format!("{run_id}-{}-done", call.as_str())),
                    payload: EventPayload::Known(KnownEventPayload::ModelCallCompleted(
                        ModelCallCompletedPayload {
                            model_call_id: call.clone(),
                            usage,
                        },
                    )),
                }],
                now_ms,
            )
            .await
            .map_err(DriveError::Storage)?;
        events.publish_committed(&committed.events);
        Ok(())
    }

    /// R05 RR1 F38: persists ONLY the usage-ledger row of a model call
    /// that was IN FLIGHT when the run's cancellation fence won — no
    /// `model_call_completed` run event. The A09/C16 discipline (a
    /// cancelled call is never closed with a fabricated completed event;
    /// the run's own `cancelled` terminal closes the story) stays exactly
    /// as pinned, while the ACCOUNTING fact — the request may already
    /// have left the process — must not vanish with the fence (F-WU02:
    /// a normal cancellation never hides behind the crash-window
    /// registration). The row states `outcome=cancelled` and exactly what
    /// the driver could observe (see the call sites).
    async fn persist_model_call_cancelled_in_flight<P: StoragePort>(
        &self,
        port: &P,
        ctx: &lingxi_kernel::RunContext,
        call: &ModelCallId,
        facts: &ModelCallUsageFacts,
        ledger: &UsageLedgerContext,
        now_ms: u64,
    ) -> Result<(), DriveError> {
        let record = model_call_usage_record_of(ctx, call, facts, ledger, now_ms);
        port.record_model_call_usage(record, now_ms)
            .await
            .map_err(DriveError::Storage)?;
        Ok(())
    }

    /// R05-T04 (D6): normalizes one drained delta batch through the
    /// think/mood chain and persists the resulting wire events in ONE
    /// transaction, publishing strictly after the commit. Delta event ids
    /// are `{run_id}-{call}-ev{n}` with a per-call local sequence. A
    /// reasoning fragment marks the run's process-content observation
    /// (streamed reasoning IS process content — the no-final cause
    /// classification stays honest when the terminal itself is empty).
    #[allow(clippy::too_many_arguments)]
    async fn persist_delta_batch<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        call: &ModelCallId,
        normalizer: &mut DeltaNormalizer,
        event_seq: &mut u64,
        deltas: &[ModelTurnDelta],
        saw_process_content: &mut bool,
        now_ms: u64,
    ) -> Result<(), DriveError> {
        let mut norm_events = Vec::new();
        for delta in deltas {
            if matches!(delta, ModelTurnDelta::Reasoning(_)) {
                *saw_process_content = true;
            }
            norm_events.extend(normalizer.feed(delta));
        }
        self.persist_norm_events(port, events, ctx, call, event_seq, norm_events, now_ms)
            .await
    }

    /// Persists one normalized event batch in ONE `record_run_events`
    /// transaction and publishes strictly after the commit (a subscriber
    /// never sees a delta that is not durable). Storage failures are loud
    /// [`DriveError::Storage`]s — a delta that could not persist never
    /// reaches subscribers.
    // Same explicit-dependency-passing shape as `drive_run`.
    #[allow(clippy::too_many_arguments)]
    async fn persist_norm_events<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        ctx: &lingxi_kernel::RunContext,
        call: &ModelCallId,
        event_seq: &mut u64,
        norm_events: Vec<NormEvent>,
        now_ms: u64,
    ) -> Result<(), DriveError> {
        if norm_events.is_empty() {
            return Ok(());
        }
        let run_id = ctx.run_id.to_string();
        let mut key_events = Vec::with_capacity(norm_events.len());
        for event in norm_events {
            *event_seq += 1;
            key_events.push(KeyEvent {
                event_id: EventId::new(format!("{run_id}-{}-ev{}", call.as_str(), *event_seq)),
                payload: EventPayload::Known(norm_event_payload(call, event)),
            });
        }
        let committed = port
            .record_run_events(ctx, key_events, now_ms)
            .await
            .map_err(DriveError::Storage)?;
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

/// Maps one normalized delta event onto the frozen wire payload (R05-T04
/// D5: `model_call_delta` carries the per-phase fragment; the segment
/// events carry the same text under the segment identity).
fn norm_event_payload(call: &ModelCallId, event: NormEvent) -> KnownEventPayload {
    match event {
        NormEvent::ModelDelta { phase, delta } => {
            KnownEventPayload::ModelCallDelta(ModelCallDeltaPayload {
                model_call_id: call.clone(),
                phase,
                delta,
            })
        }
        NormEvent::SegmentStart {
            segment_id,
            kind,
            phase,
        } => KnownEventPayload::AssistantSegmentStart(AssistantSegmentStartPayload {
            segment_id,
            kind,
            semantic_phase: phase,
        }),
        NormEvent::SegmentDelta {
            segment_id,
            delta,
            phase,
        } => KnownEventPayload::AssistantSegmentDelta(AssistantSegmentDeltaPayload {
            segment_id,
            delta,
            semantic_phase: phase,
        }),
        NormEvent::SegmentEnd { segment_id, phase } => {
            KnownEventPayload::AssistantSegmentEnd(AssistantSegmentEndPayload {
                segment_id,
                semantic_phase: phase,
            })
        }
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
            detail: match result.status.as_deref() {
                Some(lingxi_kernel::ports::ToolRunStatus::Exited { code }) => format!(
                    "external content digest {} (exit {code})",
                    result.content_digest
                ),
                Some(lingxi_kernel::ports::ToolRunStatus::Running { handle }) => format!(
                    "external content digest {} (still running, handle {handle})",
                    result.content_digest
                ),
                // R04-RR1-F03: an unconfirmed stop is journaled as what
                // it is — the receipt never reads as an observed exit.
                Some(lingxi_kernel::ports::ToolRunStatus::StopUnconfirmed { handle, detail }) => {
                    format!(
                        "external content digest {} (stop unconfirmed, handle {handle}: {detail})",
                        result.content_digest
                    )
                }
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
                status: Some(Box::new(lingxi_kernel::ports::ToolRunStatus::Exited {
                    code: 3,
                })),
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
