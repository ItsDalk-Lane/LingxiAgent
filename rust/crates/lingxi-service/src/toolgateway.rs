//! The unified tool invocation gateway (R04-T02).
//!
//! Every tool execution on the Rust stack funnels through THIS boundary:
//! path differences (resident vs on-demand catalog, user run vs subagent
//! child run, delegation dispatch, a background-driven run) must never
//! change the authorization conclusion for the same principal, target and
//! arguments (R04-A03), and nothing a MODEL writes into `arguments` can
//! mint identity, capability, prepared state or approval (R04-A04).
//!
//! # Ownership inside the frozen R03 chain (R04-SUP-02)
//!
//! The gateway deliberately owns ONLY the stages the R03 handoff left to
//! R04 — it never opens a second authorization face and never writes a
//! second receipt:
//!
//! - **Journal**: the RUN DRIVER remains the single writer of the
//!   invocation journal (`record_invocation_intent(prepared) →
//!   advance_invocation(authorized) → advance_invocation(started) →
//!   execute → record_invocation_receipt`). The gateway holds no storage
//!   port at all; a refusal it returns becomes a never-dispatched
//!   `failed` receipt written by the DRIVER, exactly like the R03-T06
//!   authorization denials.
//! - **Authorization decision point**: the driver's application of
//!   [`crate::runs::RunGrant`] at the journal's `authorized` step (Full →
//!   direct; Subagent{tier} → kernel `authorize_child_tool`) is untouched.
//!   The gateway contains NO grant logic. Its [`ToolPolicyPort`] is the
//!   工具策略 layer of the layered defense the master prompt §4.2 allows
//!   ("入口认证、工具策略、系统沙盒分层防御…最终有效权限取约束交集"):
//!   both layers must allow — neither can widen the other.
//! - **Preparation order**: target resolution / availability / generation
//!   / schema-validated effective arguments / policy adjudication all
//!   happen when the driver calls [`ToolInvocationGateway::prepare`],
//!   BEFORE the journal's `started` write — a policy refusal or a
//!   needs-approval wait can therefore never leave a `started`-without-
//!   dispatch entry behind (that phase is reserved for a real external
//!   dispatch, which is what makes its crash window honestly UNKNOWN).
//!
//! # PreparedInvocation is server-generated and unforgeable
//!
//! [`PreparedInvocation`] records are minted by THIS gateway from a
//! [`lingxi_kernel::RunContext`] handed over by a trusted entry (the run
//! driver / the service composition root). They bind principal, session,
//! run, attempt, generation, agent, entry, target id + both generations,
//! the effective arguments and their trusted digest, the capability, the
//! tool call id and an expiry. The model never sees the binding inputs:
//! the principal comes from the run context, the arguments are
//! re-validated against the CURRENT registry schema, and the handle is an
//! opaque CSPRNG nonce. At execution time ([`Self::execute_prepared`])
//! every bound fact is RE-VERIFIED against the live context, the current
//! registry state and the clock; the handle is single-use (one concurrent
//! spend wins, the loser is refused), and the executor receives the
//! SERVER-BOUND effective arguments, never re-derived wire data.
//!
//! # Entry inventory (real, not invented)
//!
//! [`CallerSurface`] enumerates the tool-call entry surfaces that ACTUALLY
//! exist on the Rust stack today. MCP / plugin-worker / developer-HTTP
//! routes do not exist yet (R04-T07 registers them through the same
//! request shape); inventing them here would violate the fail-closed
//! honesty rule. Background submissions
//! ([`crate::background`]) drive runs through the SAME
//! [`crate::runs::RunSupervisor`] chain, so a background-driven tool call
//! enters as its run's surface (user or subagent) — the submission surface
//! differs, the tool path does not.
//!
//! # Executor exposure
//!
//! The per-target executors bound via [`Self::bind_executor`] are PRIVATE.
//! The only dispatch path is [`Self::dispatch_executor`] behind a verified
//! [`PreparedInvocation`]; there is deliberately NO `ToolExecutorPort`
//! impl on the gateway (a plain `execute(ctx, call, request)` would be a
//! prepare-less bypass). `xtask check-boundaries` (check B1) statically
//! rejects any business source file touching the protected symbols
//! outside the whitelisted call sites.

use std::collections::HashMap;
use std::sync::Arc;

use lingxi_kernel::ports::{ToolExecutionResult, ToolExecutorPort, ToolRequest};
use lingxi_kernel::toolcatalog::{
    EffectiveArguments, PermissionKind, SchemaBudget, ToolRegistry, ToolTargetId, ToolTargetRef,
};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId};

use crate::inject::ServiceClock;

/// Default lifetime of a [`PreparedInvocation`] (the incumbent's confirm
/// timeout scale — a prepared call must not outlive its caller's context
/// by much; R04-T03's approval lifecycle owns the full policy).
pub const DEFAULT_PREPARED_TTL_MS: u64 = 5 * 60 * 1000;

/// Hard cap of simultaneously LIVE (unconsumed) prepared invocations.
/// A full registry is a loud refusal — never an unbounded backlog and
/// never a silent eviction that would turn a live handle into a surprise
/// refusal without diagnosis.
pub const DEFAULT_LIVE_PREPARED_CAP: usize = 1024;

/// What the trusted entry knows about where a tool call comes from. The
/// gateway derives the recorded [`InvocationEntry`] from this surface plus
/// the target's registered availability.
///
/// Real surfaces on today's Rust stack (invent nothing):
/// - **UserRun** — a tool request inside a user-submitted run
///   ([`RunGrant::Full`]); the resident/on-demand split comes from the
///   manifest's availability.
/// - **SubagentRun** — a tool request inside a subagent CHILD run
///   ([`RunGrant::Subagent`]); the parent-child attenuation is applied by
///   the driver's authorization step, not here.
/// - **DelegationDispatch** — the `subagent` / `subagent_reply` /
///   `subagent_close` family dispatched from a run's tool loop (the
///   special branch in the driver); the gateway gives it the SAME target
///   identity / availability / parameter / policy checks as every other
///   target — a special run mechanism is not a gateway bypass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerSurface {
    UserRun,
    SubagentRun,
    DelegationDispatch,
}

/// The recorded entry of one prepared invocation (what the entry×permission
/// matrix is keyed on). `Direct`/`OnDemand` distinguish the resident core
/// surface from the on-demand catalog surface of a USER run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationEntry {
    Direct,
    OnDemand,
    Subagent,
    Delegation,
}

impl InvocationEntry {
    pub fn wire_name(self) -> &'static str {
        match self {
            InvocationEntry::Direct => "direct",
            InvocationEntry::OnDemand => "on-demand",
            InvocationEntry::Subagent => "subagent",
            InvocationEntry::Delegation => "delegation",
        }
    }
}

/// The permission context one invocation is adjudicated under
/// (R04-T03): the trusted entry's snapshot of the session permission
/// facts the tool-policy face maps onto verdicts. Built by the DRIVER
/// from its [`crate::runs::DriveAuthorization`] — never from model
/// `arguments`.
///
/// - **UserSession** — a user run's session permission mode
///   (`operate`/`ask`/`read_only`, the incumbent's
///   `getSessionPermissionMode`), snapshotted at submission.
/// - **Subagent** — a subagent child run's ATTENUATED tier (fixed at
///   dispatch, `resolve_subagent_access`) plus the PARENT session mode
///   it inherited. The tier alone cannot express the ask inheritance
///   (the R03-T06-O1 gap): an operate-tier child of an ask parent is
///   exactly the case whose write-class calls the incumbent answers
///   with `deny_on_prompt` + `allowHumanApproval:false` →
///   `TOOL_APPROVAL_UNAVAILABLE` — the policy face reproduces that
///   verdict from THIS pair (see [`crate::approval_service`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationPermissionContext {
    UserSession {
        mode: lingxi_kernel::subagent::SessionPermissionMode,
    },
    Subagent {
        tier: lingxi_kernel::subagent::ToolAccessTier,
        parent_mode: lingxi_kernel::subagent::SessionPermissionMode,
    },
}

impl InvocationPermissionContext {
    pub fn wire_kind(&self) -> &'static str {
        match self {
            InvocationPermissionContext::UserSession { .. } => "user_session",
            InvocationPermissionContext::Subagent { .. } => "subagent",
        }
    }
}

/// The unified request every tool-call entry converts into (R04-T02
/// 怎么做 1). Built by TRUSTED entry code from a [`RunContext`] it owns —
/// the principal facts NEVER come from model `arguments` (that is the
/// whole point of R04-A04).
#[derive(Debug, Clone, PartialEq)]
pub struct InvocationRequest {
    pub principal: Principal,
    pub session_id: String,
    pub run_id: String,
    pub attempt: String,
    pub generation: u64,
    pub agent_id: String,
    pub surface: CallerSurface,
    /// The permission context the policy face adjudicates under
    /// (R04-T03): the driver's trusted snapshot, never model data.
    pub permission: InvocationPermissionContext,
    /// Registry reference (target id / name / source+name).
    pub reference: ToolTargetRef,
    /// The caller's pinned catalog generation, when it holds one.
    pub pin: Option<lingxi_kernel::toolcatalog::CatalogPin>,
    /// RAW (model-provided) arguments; the gateway validates + normalizes
    /// them against the CURRENT schema itself.
    pub raw_arguments: serde_json::Value,
    pub tool_call_id: ToolCallId,
}

impl InvocationRequest {
    /// Builds the unified request from the facts a trusted entry holds.
    /// This is the ONLY constructor: there is no "from JSON" path, so a
    /// model payload can never populate the identity fields.
    #[allow(clippy::too_many_arguments)]
    pub fn from_trusted_entry(
        ctx: &RunContext,
        surface: CallerSurface,
        agent_id: &str,
        permission: InvocationPermissionContext,
        reference: ToolTargetRef,
        pin: Option<lingxi_kernel::toolcatalog::CatalogPin>,
        raw_arguments: serde_json::Value,
        tool_call_id: ToolCallId,
    ) -> Self {
        Self {
            principal: ctx.principal.clone(),
            session_id: ctx.session_id.to_string(),
            run_id: ctx.run_id.to_string(),
            attempt: ctx.attempt.to_string(),
            generation: ctx.generation,
            agent_id: agent_id.to_string(),
            surface,
            permission,
            reference,
            pin,
            raw_arguments,
            tool_call_id,
        }
    }
}

/// The policy verdict for one prepared invocation. [`PolicyVerdict::Denied`]
/// never yields a prepared handle (prepare refuses); it appears in the
/// vocabulary because the PORT may return it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    /// The configured policy service adjudicated ALLOW. (The driver's
    /// RunGrant layer still applies — this is a layer, not a bypass.)
    Allowed,
    /// The policy cannot safely decide this itself: a human approval is
    /// required before any dispatch. When no approval surface is wired the
    /// DRIVER closes the call as a never-dispatched refusal
    /// (TOOL_APPROVAL_UNAVAILABLE semantics) — never an auto-Full pass.
    NeedsApproval { reason: String },
    /// The policy refused outright.
    Denied {
        code: String,
        layer: String,
        message: String,
    },
}

/// The input the policy service adjudicates on. Identity facts come from
/// the trusted entry; the digest is the trusted-boundary digest of the
/// EFFECTIVE arguments (what an approval binds to); the permission
/// context is the driver's trusted session-mode snapshot (R04-T03).
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyAdjudicationInput {
    pub entry: InvocationEntry,
    pub target_id: String,
    pub local_name: String,
    pub permission_kind: PermissionKind,
    pub capability_base: String,
    pub args_digest_hex: String,
    pub principal_kind: &'static str,
    pub principal_subject: String,
    pub session_id: String,
    pub run_id: String,
    pub agent_id: String,
    /// The permission context of the invocation (R04-T03): user session
    /// mode, or the subagent tier + inherited parent mode.
    pub permission_context: InvocationPermissionContext,
}

/// The tool-policy service port (R04-T02 怎么做 3). R04-T03's ApprovalService
/// is the real production implementation; until it is wired the
/// fail-closed default ([`FailClosedPolicy`]) applies. The port is
/// synchronous and pure on its input: it may only LOOK at facts, never
/// mint new ones.
pub trait ToolPolicyPort: Send + Sync {
    fn adjudicate(&self, input: &PolicyAdjudicationInput) -> PolicyVerdict;
}

/// The production-default policy source when NO policy service is wired:
/// refuse what it cannot safely decide — never an automatic Full pass.
///
/// - Read-class contracts (information tools, allowed in every incumbent
///   mode) are safe to allow on the contract alone.
/// - Execute-class and argument-aware contracts carry real side effects
///   the unconfigured mechanism CANNOT adjudicate → explicit
///   [`PolicyVerdict::NeedsApproval`] carrying the honest reason; without
///   an approval surface the driver refuses the call with zero dispatch,
///   with one it parks in `waiting_approval` (R03-T03 leg).
pub struct FailClosedPolicy;

impl ToolPolicyPort for FailClosedPolicy {
    fn adjudicate(&self, input: &PolicyAdjudicationInput) -> PolicyVerdict {
        match input.permission_kind {
            PermissionKind::Read => PolicyVerdict::Allowed,
            PermissionKind::Execute
            | PermissionKind::ArgumentAwareFile
            | PermissionKind::ArgumentAwareSessionFolders => PolicyVerdict::NeedsApproval {
                reason: format!(
                    "no tool policy service is configured; the gateway cannot safely \
                     adjudicate an execution-class invocation of {} ({}) and never \
                     auto-allows it",
                    input.target_id, input.capability_base
                ),
            },
        }
    }
}

/// The loud refusal vocabulary of the gateway boundary. Every variant is a
/// caller-visible, machine-diagnosable zero-dispatch outcome; none is ever
/// degraded into success.
#[derive(Debug, Clone, PartialEq)]
pub enum GatewayRefusal {
    /// The requested target does not exist in the registry.
    TargetNotRegistered { reference: String },
    /// The target exists but is not callable (disabled / future / …).
    TargetNotCallable {
        target_id: String,
        availability: String,
        reason: String,
    },
    /// The caller's pinned catalog generation is stale.
    StaleCatalog {
        held_generation: u64,
        current_generation: u64,
        target_id: String,
    },
    /// The arguments violate the target's current schema (each violation
    /// named by the trusted boundary).
    ArgumentsInvalid { violations: Vec<String> },
    /// The wire request's declared digest does not match the arguments the
    /// gateway holds (defense in depth over the driver's R04-T01 gate).
    DigestMismatch { declared: String, computed: String },
    /// The configured policy refused the invocation outright.
    PolicyDenied {
        code: String,
        layer: String,
        message: String,
    },
    /// The system CSPRNG could not mint a prepared-handle nonce (entropy
    /// source failure). R04-T02-R1-O04: this is NOT a capacity condition —
    /// the dedicated code keeps the diagnostic honest (the pre-repair
    /// mapping reported `gateway_prepared_registry_full`, misleading
    /// capacity triage). Loud and safe either way: the preparation is
    /// refused, never degraded into a guessable handle.
    HandleMintFailed { source: String },
    /// The prepared-handle registry is at its cap.
    PreparedRegistryFull { cap: usize },
    /// Unknown handle (never minted, already consumed, or expired+evicted).
    PreparedHandleUnknown { handle: String },
    /// The handle was valid but already consumed (single use).
    PreparedHandleConsumed { handle: String },
    /// The handle's TTL expired.
    PreparedHandleExpired {
        handle: String,
        expired_at_unix_ms: u64,
        now_unix_ms: u64,
    },
    /// The executing context does not match the identity the handle was
    /// bound to (cross agent/session/run/attempt/generation reuse).
    IdentityMismatch { bound: String, presented: String },
    /// The target changed between prepare and execute (unregistered,
    /// disabled, or advanced to a new generation).
    TargetChanged { target_id: String, detail: String },
    /// The target has no bound executor (capability honestly not wired).
    NoExecutorBound { target_id: String },
}

impl GatewayRefusal {
    /// Stable machine-readable code (audit/evidence vocabulary).
    pub fn code(&self) -> &'static str {
        match self {
            GatewayRefusal::TargetNotRegistered { .. } => "gateway_target_not_registered",
            GatewayRefusal::TargetNotCallable { .. } => "gateway_target_not_callable",
            GatewayRefusal::StaleCatalog { .. } => "gateway_stale_catalog",
            GatewayRefusal::ArgumentsInvalid { .. } => "gateway_arguments_invalid",
            GatewayRefusal::DigestMismatch { .. } => "gateway_digest_mismatch",
            GatewayRefusal::PolicyDenied { .. } => "gateway_policy_denied",
            GatewayRefusal::HandleMintFailed { .. } => "gateway_prepared_handle_mint_failed",
            GatewayRefusal::PreparedRegistryFull { .. } => "gateway_prepared_registry_full",
            GatewayRefusal::PreparedHandleUnknown { .. } => "gateway_prepared_handle_unknown",
            GatewayRefusal::PreparedHandleConsumed { .. } => "gateway_prepared_handle_consumed",
            GatewayRefusal::PreparedHandleExpired { .. } => "gateway_prepared_handle_expired",
            GatewayRefusal::IdentityMismatch { .. } => "gateway_identity_mismatch",
            GatewayRefusal::TargetChanged { .. } => "gateway_target_changed",
            GatewayRefusal::NoExecutorBound { .. } => "gateway_no_executor_bound",
        }
    }

    /// The structured tool error the model sees (the driver embeds it in a
    /// never-dispatched failed receipt).
    pub fn to_tool_error(&self) -> ProtocolError {
        let (code, message) = match self {
            GatewayRefusal::TargetNotRegistered { reference } => (
                ErrorCode::NotFound,
                format!("tool target {reference:?} is not registered on this gateway"),
            ),
            GatewayRefusal::TargetNotCallable {
                target_id,
                availability,
                reason,
            } => (
                ErrorCode::Forbidden,
                format!("tool target {target_id} is {availability} and not callable: {reason}"),
            ),
            GatewayRefusal::StaleCatalog {
                held_generation,
                current_generation,
                target_id,
            } => (
                ErrorCode::Conflict,
                format!(
                    "stale catalog: request pinned at generation {held_generation} but the \
                     registry is at {current_generation} (target {target_id}); re-describe and \
                     re-prepare against a fresh snapshot"
                ),
            ),
            GatewayRefusal::ArgumentsInvalid { violations } => (
                ErrorCode::InvalidMessage,
                format!(
                    "tool arguments violate the current schema: {}",
                    violations.join("; ")
                ),
            ),
            GatewayRefusal::DigestMismatch { declared, computed } => (
                ErrorCode::InvalidMessage,
                format!(
                    "tool request args_digest {declared} does not match its arguments \
                     (computed {computed}); forged or adulterated request — not dispatched"
                ),
            ),
            GatewayRefusal::PolicyDenied {
                code,
                layer,
                message,
            } => (
                ErrorCode::Forbidden,
                format!("tool policy denied: {code} [{layer}]: {message}"),
            ),
            GatewayRefusal::HandleMintFailed { source } => (
                ErrorCode::UpstreamUnavailable,
                format!(
                    "the system entropy source failed while minting the prepared-invocation \
                     handle nonce ({source}); the preparation is refused — a guessable handle \
                     would be a forgery surface, so this is never degraded"
                ),
            ),
            GatewayRefusal::PreparedRegistryFull { cap } => (
                ErrorCode::BudgetExceeded,
                format!("prepared-invocation registry is at its cap ({cap})"),
            ),
            GatewayRefusal::PreparedHandleUnknown { handle } => (
                ErrorCode::Forbidden,
                format!(
                    "prepared invocation handle is unknown (never issued, consumed or \
                     expired): {handle}"
                ),
            ),
            GatewayRefusal::PreparedHandleConsumed { handle } => (
                ErrorCode::Conflict,
                format!("prepared invocation handle was already used exactly once: {handle}"),
            ),
            GatewayRefusal::PreparedHandleExpired {
                handle,
                expired_at_unix_ms,
                now_unix_ms,
            } => (
                ErrorCode::Forbidden,
                format!(
                    "prepared invocation handle expired at {expired_at_unix_ms} (now \
                     {now_unix_ms}): {handle}"
                ),
            ),
            GatewayRefusal::IdentityMismatch { bound, presented } => (
                ErrorCode::Forbidden,
                format!(
                    "prepared invocation identity mismatch: bound to {bound}, presented {presented}"
                ),
            ),
            GatewayRefusal::TargetChanged { target_id, detail } => (
                ErrorCode::Conflict,
                format!(
                    "tool target {target_id} changed after preparation and the invocation \
                     is refused: {detail}"
                ),
            ),
            GatewayRefusal::NoExecutorBound { target_id } => (
                ErrorCode::UpstreamUnavailable,
                format!(
                    "tool target {target_id} has no executor bound on this gateway; the \
                     capability is not wired (refused, never silently skipped)"
                ),
            ),
        };
        ProtocolError::new(code, message, false)
    }
}

impl std::fmt::Display for GatewayRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.to_tool_error().message)
    }
}

impl std::error::Error for GatewayRefusal {}

/// Opaque handle of one prepared invocation. It is a CSPRNG nonce minted
/// by the gateway; the ONLY way to obtain one is [`Self::prepare`], and
/// the only way to spend one is [`Self::execute_prepared`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PreparedInvocationHandle(String);

impl PreparedInvocationHandle {
    /// Parses a handle STRING for lookup/attack-test purposes. Parsing
    /// never validates anything — an unknown string simply fails the
    /// registry lookup with [`GatewayRefusal::PreparedHandleUnknown`].
    pub fn parse(raw: &str) -> Self {
        Self(raw.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The caller-visible view of one prepared invocation. The full binding
/// record stays server-side; this view carries exactly what the driver
/// needs to decide (policy verdict, local name for the child-tool
/// blocklist, digest) plus the opaque handle.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedInvocation {
    pub handle: PreparedInvocationHandle,
    pub entry: InvocationEntry,
    pub target_id: ToolTargetId,
    /// The target's origin (first-party / plugin / mcp). The run driver's
    /// delegation-family routing (R04-T02-R1-F01 same-root-cause closure)
    /// matches by LOCAL NAME and must restrict the match to FIRST-PARTY
    /// targets: a plugin/MCP-origin registration whose local name
    /// collides with the delegation family is a different target and may
    /// never be routed into the child-run launcher.
    pub origin: lingxi_kernel::toolcatalog::ToolOrigin,
    pub local_name: String,
    pub permission_kind: PermissionKind,
    pub capability_base: String,
    pub args_digest_hex: String,
    pub target_generation: u64,
    pub catalog_generation: u64,
    pub expires_at_unix_ms: u64,
    /// The adjudicated policy: `Allowed`, or `NeedsApproval` (the driver
    /// routes it through the approval surface; without one it refuses).
    pub policy: PolicyVerdict,
}

/// Server-side binding record (never handed to the model). The
/// caller-visible [`PreparedInvocation`] view carries the decision facts
/// (local name, policy verdict); this record carries only what
/// re-verification and dispatch consume.
#[derive(Debug, Clone)]
struct PreparedRecord {
    principal_kind: &'static str,
    principal_subject: String,
    session_id: String,
    run_id: String,
    attempt: String,
    generation: u64,
    agent_id: String,
    entry: InvocationEntry,
    /// The permission context the invocation was adjudicated under
    /// (R04-T03 audit fact — the verdict is already taken; the record
    /// keeps what it was taken under).
    permission: InvocationPermissionContext,
    target_id: ToolTargetId,
    permission_kind: PermissionKind,
    capability_base: String,
    target_generation: u64,
    catalog_generation: u64,
    effective: EffectiveArguments,
    args_digest_hex: String,
    tool_call_id: ToolCallId,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
}

/// One bound executor + the registrar's reason (audit: why this executor
/// may serve this target through the gateway).
#[derive(Clone)]
struct ExecutorBinding {
    executor: Arc<dyn ToolExecutorPort>,
    reason: String,
}

struct PreparedState {
    record: PreparedRecord,
}

/// The live-preapred registry's bookkeeping: the LIVE (unconsumed,
/// unexpired) records plus a bounded ring of recently CONSUMED handles.
/// The ring keeps the `gateway_prepared_handle_consumed` diagnostic for
/// replay presentations while guaranteeing bounded memory: consumed and
/// expired entries are reclaimed (consumed → ring, expired → dropped)
/// under cap pressure, never left to grow a long-lived process without
/// bound.
const CONSUMED_RING_CAP: usize = 4096;

/// The unified tool invocation gateway. Interior-mutable, `Send + Sync`,
/// owned by the service composition root and injected into the run
/// supervisor ([`crate::runs::RunSupervisor`]).
pub struct ToolInvocationGateway {
    registry: Arc<ToolRegistry>,
    budget: SchemaBudget,
    policy: Arc<dyn ToolPolicyPort>,
    executors: std::sync::Mutex<std::collections::BTreeMap<ToolTargetId, ExecutorBinding>>,
    prepared: std::sync::Mutex<HashMap<String, PreparedState>>,
    /// Bounded ring of CONSUMED handles (single-use bookkeeping without
    /// unbounded retention).
    consumed_ring: std::sync::Mutex<std::collections::VecDeque<String>>,
    clock: Arc<dyn ServiceClock>,
    prepared_ttl_ms: u64,
    live_prepared_cap: usize,
}

impl std::fmt::Debug for ToolInvocationGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolInvocationGateway")
            .field("prepared_ttl_ms", &self.prepared_ttl_ms)
            .field("live_prepared_cap", &self.live_prepared_cap)
            .field("policy", &"Arc<dyn ToolPolicyPort>")
            .field("registry", &"Arc<ToolRegistry>")
            .finish_non_exhaustive()
    }
}

impl ToolInvocationGateway {
    /// Full construction. The policy port is REQUIRED — wiring without a
    /// policy decision source is impossible by type; callers that have no
    /// configured policy service pass [`FailClosedPolicy`] (the honest
    /// default, never an auto-allow).
    pub fn new(
        registry: Arc<ToolRegistry>,
        policy: Arc<dyn ToolPolicyPort>,
        clock: Arc<dyn ServiceClock>,
        budget: SchemaBudget,
        prepared_ttl_ms: u64,
        live_prepared_cap: usize,
    ) -> Self {
        Self {
            registry,
            budget,
            policy,
            executors: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            prepared: std::sync::Mutex::new(HashMap::new()),
            consumed_ring: std::sync::Mutex::new(std::collections::VecDeque::new()),
            clock,
            prepared_ttl_ms,
            live_prepared_cap,
        }
    }

    /// Binds the executor that serves one target THROUGH the gateway. The
    /// binding is private: business code can only reach it via a verified
    /// [`PreparedInvocation`].
    pub fn bind_executor(
        &self,
        target_id: ToolTargetId,
        executor: Arc<dyn ToolExecutorPort>,
        reason: &str,
    ) {
        self.executors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                target_id,
                ExecutorBinding {
                    executor,
                    reason: reason.to_string(),
                },
            );
    }

    /// Observability: the registered reasons of the bound executors.
    pub fn executor_binding_reasons(&self) -> Vec<(String, String)> {
        self.executors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(id, binding)| (id.as_str().to_string(), binding.reason.clone()))
            .collect()
    }

    /// The unified PREPARE stage: target resolution → availability →
    /// generation pin → schema validation + normalization → trusted
    /// digest → policy adjudication → server-side prepared record.
    ///
    /// Returns a refusal (zero dispatch, the driver journals a
    /// never-dispatched failure) or the prepared view. A
    /// [`PolicyVerdict::NeedsApproval`] PREPARES successfully — the
    /// approval wait belongs to the driver's approval surface, before any
    /// `started` write.
    pub fn prepare(
        &self,
        request: InvocationRequest,
    ) -> Result<PreparedInvocation, GatewayRefusal> {
        let now = self.clock.now_unix_ms();
        let prepared_tool_call = self
            .registry
            .prepare_invocation(
                &request.reference,
                request.pin,
                &request.raw_arguments,
                &self.budget,
            )
            .map_err(map_catalog_error)?;
        // Defense in depth over the driver's R04-T01 gate: the wire digest
        // (when the caller compared one) must equal the digest the trusted
        // boundary just derived.
        let args_digest_hex = prepared_tool_call.args_digest.hex.clone();
        let entry = match request.surface {
            CallerSurface::DelegationDispatch => InvocationEntry::Delegation,
            CallerSurface::SubagentRun => InvocationEntry::Subagent,
            CallerSurface::UserRun => {
                // The resident/on-demand split is the manifest's own
                // availability classification.
                match self.registry.describe(&prepared_tool_call.target_id) {
                    Ok(listing) => {
                        if listing.availability
                            == lingxi_kernel::toolcatalog::Availability::Deferred
                        {
                            InvocationEntry::OnDemand
                        } else {
                            InvocationEntry::Direct
                        }
                    }
                    Err(err) => return Err(map_catalog_error(err)),
                }
            }
        };
        let policy_input = PolicyAdjudicationInput {
            entry,
            target_id: prepared_tool_call.target_id.as_str().to_string(),
            local_name: prepared_tool_call.local_name.clone(),
            permission_kind: prepared_tool_call.permission.kind,
            capability_base: prepared_tool_call.permission.capability_base.clone(),
            args_digest_hex: args_digest_hex.clone(),
            principal_kind: request.principal.storage_kind(),
            principal_subject: request.principal.storage_subject(),
            session_id: request.session_id.clone(),
            run_id: request.run_id.clone(),
            agent_id: request.agent_id.clone(),
            permission_context: request.permission,
        };
        let policy = match self.policy.adjudicate(&policy_input) {
            PolicyVerdict::Denied {
                code,
                layer,
                message,
            } => {
                return Err(GatewayRefusal::PolicyDenied {
                    code,
                    layer,
                    message,
                });
            }
            verdict => verdict,
        };
        let expires_at = now.saturating_add(self.prepared_ttl_ms);
        let handle_nonce = match crate::auth::hex_random_public(16) {
            Ok(hex) => format!("prep:{hex}"),
            // The system CSPRNG failing must never degrade into a GUESSABLE
            // handle: refuse the preparation loudly instead. R04-T02-R1-O04:
            // the refusal carries its OWN code (an entropy failure is not a
            // capacity condition — the registry may be empty).
            Err(err) => {
                tracing::error!(
                    error = %err,
                    "prepared-invocation handle nonce could not be minted; refusing the \
                     preparation (a guessable handle would be a forgery surface)"
                );
                return Err(GatewayRefusal::HandleMintFailed {
                    source: err.to_string(),
                });
            }
        };
        let record = PreparedRecord {
            principal_kind: request.principal.storage_kind(),
            principal_subject: request.principal.storage_subject(),
            session_id: request.session_id,
            run_id: request.run_id,
            attempt: request.attempt,
            generation: request.generation,
            agent_id: request.agent_id,
            entry,
            permission: request.permission,
            target_id: prepared_tool_call.target_id.clone(),
            permission_kind: prepared_tool_call.permission.kind,
            capability_base: prepared_tool_call.permission.capability_base.clone(),
            target_generation: prepared_tool_call.target_generation,
            catalog_generation: prepared_tool_call.catalog_generation,
            effective: prepared_tool_call.effective,
            args_digest_hex: args_digest_hex.clone(),
            tool_call_id: request.tool_call_id,
            created_at_unix_ms: now,
            expires_at_unix_ms: expires_at,
        };
        let mut prepared = self
            .prepared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Bounded-memory reclamation under cap pressure: consumed entries
        // are ALREADY reclaimed at consumption (removed from the map into
        // the bounded ring), so the only accumulation risk is EXPIRED
        // never-presented handles — they drop here. The sweep runs ONLY
        // when the map is at capacity, so the expiry diagnostic is
        // normally kept for honest refusal messages.
        if prepared.len() >= self.live_prepared_cap {
            let now_for_sweep = now;
            prepared.retain(|_, state| now_for_sweep <= state.record.expires_at_unix_ms);
        }
        if prepared.len() >= self.live_prepared_cap {
            return Err(GatewayRefusal::PreparedRegistryFull {
                cap: self.live_prepared_cap,
            });
        }
        prepared.insert(handle_nonce.clone(), PreparedState { record });
        Ok(PreparedInvocation {
            handle: PreparedInvocationHandle(handle_nonce),
            entry,
            target_id: prepared_tool_call.target_id,
            origin: prepared_tool_call.origin,
            local_name: prepared_tool_call.local_name,
            permission_kind: prepared_tool_call.permission.kind,
            capability_base: prepared_tool_call.permission.capability_base,
            args_digest_hex,
            target_generation: prepared_tool_call.target_generation,
            catalog_generation: prepared_tool_call.catalog_generation,
            expires_at_unix_ms: expires_at,
            policy,
        })
    }

    /// Prepares from the DRIVER's request shape: the registry reference is
    /// the request's target id, the "raw" arguments are the request's
    /// effective arguments (re-validated against the CURRENT schema — an
    /// arguments object that slipped past a schema change is refused
    /// here), and the wire digest must be the digest of the wire
    /// arguments.
    ///
    /// Digest semantics (deliberate): the anti-forgery comparison covers
    /// the WIRE pair only (`args_digest` == digest of the arguments this
    /// gateway holds — defense in depth over the driver's R04-T01 gate).
    /// The PREPARED digest is NOT compared against the wire digest:
    /// normalization may legitimately fill schema DEFAULTS, and the
    /// trusted-boundary-filled effective arguments (what the executor
    /// receives and what the record binds) then hash differently from the
    /// raw wire payload — that is the boundary doing its job, not a
    /// forgery.
    pub fn prepare_from_request(
        &self,
        ctx: &RunContext,
        surface: CallerSurface,
        agent_id: &str,
        permission: InvocationPermissionContext,
        call_id: &ToolCallId,
        request: &ToolRequest,
    ) -> Result<PreparedInvocation, GatewayRefusal> {
        if !request.digest_matches_arguments() {
            return Err(GatewayRefusal::DigestMismatch {
                declared: request.args_digest.hex.clone(),
                computed: request.arguments.digest().hex,
            });
        }
        let reference = ToolTargetRef::ByTargetId {
            target_id: ToolTargetId::parse(&request.target),
        };
        let unified = InvocationRequest::from_trusted_entry(
            ctx,
            surface,
            agent_id,
            permission,
            reference,
            None,
            request.arguments.as_value().clone(),
            call_id.clone(),
        );
        self.prepare(unified)
    }

    /// The single EXECUTION stage. Re-verifies EVERY bound fact against
    /// the live context and registry, marks the handle consumed exactly
    /// once, then dispatches to the PRIVATE executor binding with the
    /// SERVER-BOUND effective arguments (never re-derived wire data).
    ///
    /// `Err(refusal)` means ZERO dispatch happened — the driver journals a
    /// never-dispatched failed receipt; `Ok(result)` carries the
    /// executor's own outcome (its `Failed` means the external system
    /// failed AFTER dispatch, exactly like the R03 shape).
    pub async fn execute_prepared(
        &self,
        ctx: &RunContext,
        call_id: &ToolCallId,
        handle: &PreparedInvocationHandle,
    ) -> Result<ToolExecutionResult, GatewayRefusal> {
        // 1) Verify-then-consume under ONE lock: identity, expiry and
        //    single-use are decided atomically, in that order. Verifying
        //    identity BEFORE consuming means a hostile/foreign presentation
        //    cannot burn the legitimate handle (a denial of the real
        //    execution), while the second concurrent spend of a valid
        //    handle still loses deterministically.
        let record = {
            let mut prepared = self
                .prepared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(state) = prepared.get_mut(handle.as_str()) else {
                // Not live: a recently consumed handle reports the
                // single-use diagnostic; anything else is unknown.
                let consumed = self
                    .consumed_ring
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .contains(&handle.as_str().to_string());
                return Err(if consumed {
                    GatewayRefusal::PreparedHandleConsumed {
                        handle: handle.as_str().to_string(),
                    }
                } else {
                    GatewayRefusal::PreparedHandleUnknown {
                        handle: handle.as_str().to_string(),
                    }
                });
            };
            let now = self.clock.now_unix_ms();
            if now > state.record.expires_at_unix_ms {
                return Err(GatewayRefusal::PreparedHandleExpired {
                    handle: handle.as_str().to_string(),
                    expired_at_unix_ms: state.record.expires_at_unix_ms,
                    now_unix_ms: now,
                });
            }
            // 2) Identity re-verification: the handle is bound to exactly
            //    one (principal, session, run, attempt, generation, agent,
            //    call) — a cross agent/session/run/attempt/call
            //    presentation is a forgery, never a feature.
            let record = &state.record;
            let identity_ok = record.principal_kind == ctx.principal.storage_kind()
                && record.principal_subject == ctx.principal.storage_subject()
                && record.session_id == ctx.session_id.to_string()
                && record.run_id == ctx.run_id.to_string()
                && record.attempt == ctx.attempt.to_string()
                && record.generation == ctx.generation
                && record.tool_call_id == *call_id;
            if !identity_ok {
                let bound = format!(
                    "principal={}/{} session={} run={} attempt={} generation={} agent={} \
                     call={}",
                    record.principal_kind,
                    record.principal_subject,
                    record.session_id,
                    record.run_id,
                    record.attempt,
                    record.generation,
                    record.agent_id,
                    record.tool_call_id,
                );
                let presented = format!(
                    "principal={}/{} session={} run={} attempt={} generation={} agent={} \
                     call={}",
                    ctx.principal.storage_kind(),
                    ctx.principal.storage_subject(),
                    ctx.session_id,
                    ctx.run_id,
                    ctx.attempt,
                    ctx.generation,
                    "(live)",
                    call_id,
                );
                tracing::warn!(
                    handle = handle.as_str(),
                    bound = %bound,
                    presented = %presented,
                    "prepared invocation identity mismatch: refused WITHOUT consuming the \
                     handle (cross agent/session/run handle reuse is a forgery, never a \
                     feature; the legitimate owner may still spend it)"
                );
                return Err(GatewayRefusal::IdentityMismatch { bound, presented });
            }
            // Consume: the record leaves the live map immediately and its
            // handle enters the bounded consumed ring — single use with
            // bounded memory (a long-lived process never accumulates spent
            // records).
            let record = state.record.clone();
            prepared.remove(handle.as_str());
            let mut consumed_ring = self
                .consumed_ring
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if consumed_ring.len() >= CONSUMED_RING_CAP {
                consumed_ring.pop_front();
            }
            consumed_ring.push_back(handle.as_str().to_string());
            record
        };
        // 3) Registry re-verification: the target must still exist, still
        //    be callable, and still be at the SAME target generation (an
        //    update / disable / uninstall between prepare and execute
        //    invalidates the binding — the old description never points at
        //    a new meaning).
        let listing = self
            .registry
            .describe(&record.target_id)
            .map_err(map_catalog_error)?;
        if !listing.availability.callable() {
            return Err(GatewayRefusal::TargetChanged {
                target_id: record.target_id.as_str().to_string(),
                detail: format!(
                    "availability is now {} ({})",
                    listing.availability.wire_name(),
                    listing
                        .availability
                        .refusal_reason_display()
                        .unwrap_or_default()
                ),
            });
        }
        if listing.target_generation != record.target_generation {
            return Err(GatewayRefusal::TargetChanged {
                target_id: record.target_id.as_str().to_string(),
                detail: format!(
                    "target generation moved from {} to {}",
                    record.target_generation, listing.target_generation
                ),
            });
        }
        // 4) The bound executor must exist (fail-closed on unwired
        //    capabilities, never a silent skip).
        let binding = {
            let executors = self
                .executors
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            executors.get(&record.target_id).cloned()
        };
        let Some(binding) = binding else {
            return Err(GatewayRefusal::NoExecutorBound {
                target_id: record.target_id.as_str().to_string(),
            });
        };
        // 5) Dispatch with the SERVER-BOUND arguments.
        let request = ToolRequest {
            target: record.target_id.as_str().to_string(),
            arguments: record.effective.clone(),
            args_digest: digest_of_hex(&record.args_digest_hex),
            args_summary: Some(lingxi_kernel::toolcatalog::summarize_arguments(
                &record.effective,
            )),
            delegation: None,
        };
        tracing::info!(
            target = %record.target_id,
            entry = record.entry.wire_name(),
            permission_context = record.permission.wire_kind(),
            capability = %record.capability_base,
            permission_kind = record.permission_kind.wire_name(),
            target_generation = record.target_generation,
            catalog_generation = record.catalog_generation,
            prepared_at_unix_ms = record.created_at_unix_ms,
            binding_reason = %binding.reason,
            "gateway dispatch: every re-verification passed; dispatching the server-bound \
             effective arguments to the bound executor"
        );
        self.dispatch_executor(ctx, call_id, &request, &binding.executor)
            .await
    }

    /// The PROTECTED dispatch path — the only place a business flow may
    /// hand a request to a bound executor, and only with a record that
    /// passed every re-verification above. Static boundary check B1
    /// rejects any other source file referencing this symbol.
    async fn dispatch_executor(
        &self,
        ctx: &RunContext,
        call_id: &ToolCallId,
        request: &ToolRequest,
        executor: &Arc<dyn ToolExecutorPort>,
    ) -> Result<ToolExecutionResult, GatewayRefusal> {
        let outcome = executor.execute(ctx, call_id, request).await;
        Ok(outcome)
    }
}

/// Rebuilds the protocol digest value from the stored hex (the canonical
/// algorithm/canonicalization ids are the boundary's fixed vocabulary).
fn digest_of_hex(hex: &str) -> lingxi_protocol::ArgsDigest {
    lingxi_protocol::ArgsDigest {
        algorithm: "sha256".to_string(),
        canonicalization: lingxi_protocol::canon::CANONICALIZATION_ID.to_string(),
        hex: hex.to_string(),
    }
}

fn map_catalog_error(err: lingxi_kernel::toolcatalog::ToolCatalogError) -> GatewayRefusal {
    use lingxi_kernel::toolcatalog::ToolCatalogError as E;
    match err {
        E::TargetNotFound { reference } => GatewayRefusal::TargetNotRegistered { reference },
        E::TargetNotCallable {
            target_id,
            availability,
            reason,
        } => GatewayRefusal::TargetNotCallable {
            target_id,
            availability: availability.to_string(),
            reason,
        },
        E::StaleCatalog {
            held_generation,
            current_generation,
            target_id,
        } => GatewayRefusal::StaleCatalog {
            held_generation,
            current_generation,
            target_id,
        },
        E::ArgumentsInvalid { violations } => GatewayRefusal::ArgumentsInvalid { violations },
        E::ArgumentsNotObject => GatewayRefusal::ArgumentsInvalid {
            violations: vec!["arguments must be a JSON object".to_string()],
        },
        E::ArgumentsBudgetExceeded { bound, found, max } => GatewayRefusal::ArgumentsInvalid {
            violations: vec![format!("budget {bound} exceeded: {found} > {max}")],
        },
        E::ArgumentsNotSafeInteger { at, found } => GatewayRefusal::ArgumentsInvalid {
            violations: vec![format!("unsafe integer at {at}: {found}")],
        },
        other => GatewayRefusal::ArgumentsInvalid {
            violations: vec![format!("catalog refused the preparation: {other}")],
        },
    }
}

// A tiny Display helper for Availability reasons without widening the
// kernel API surface for diagnostics.
trait AvailabilityReasonDisplay {
    fn refusal_reason_display(&self) -> Option<String>;
}

impl AvailabilityReasonDisplay for lingxi_kernel::toolcatalog::Availability {
    fn refusal_reason_display(&self) -> Option<String> {
        match self {
            lingxi_kernel::toolcatalog::Availability::Disabled { reason } => {
                Some(format!("disabled: {reason}"))
            }
            lingxi_kernel::toolcatalog::Availability::Future { reason } => {
                Some(format!("not implemented yet: {reason}"))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::ManualClock;
    use lingxi_kernel::toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, ToolManifest, ToolOrigin,
    };
    use lingxi_protocol::AttemptId;
    use serde_json::json;

    fn clock() -> Arc<ManualClock> {
        Arc::new(ManualClock::new(1_700_000_000_000))
    }

    fn gateway(
        registry: &Arc<ToolRegistry>,
        policy: Arc<dyn ToolPolicyPort>,
    ) -> Arc<ToolInvocationGateway> {
        Arc::new(ToolInvocationGateway::new(
            Arc::clone(registry),
            policy,
            clock(),
            SchemaBudget::default(),
            DEFAULT_PREPARED_TTL_MS,
            DEFAULT_LIVE_PREPARED_CAP,
        ))
    }

    fn manifest(name: &str, kind: PermissionKind, availability: Availability) -> ToolManifest {
        ToolManifest {
            origin: ToolOrigin::FirstParty,
            local_name: name.to_string(),
            display_name: name.to_string(),
            aliases: Vec::new(),
            version: "1.0.0".to_string(),
            description: "gateway test manifest".to_string(),
            input_schema: lingxi_protocol::ToolSchemaDocument {
                dialect: "json-schema/2020-12".to_string(),
                schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "minLength": 1},
                    },
                    "required": ["path"],
                    "additionalProperties": false,
                }),
            },
            output_schema: None,
            permission: PermissionContract {
                kind,
                capability_base: format!("{name}.capability"),
            },
            availability,
            timeout_ms: Some(30_000),
            max_concurrency: Some(2),
            declared_permission: DeclaredPermission::None,
            recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
        }
    }

    /// The neutral user-session permission context of the gateway unit
    /// tests (operate — nothing here adjudicates mode semantics; that is
    /// the ApprovalService suite's job).
    fn test_permission() -> InvocationPermissionContext {
        InvocationPermissionContext::UserSession {
            mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
        }
    }

    fn ctx_of(run: &str) -> RunContext {
        RunContext {
            principal: Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("s-gw".to_string()),
            run_id: lingxi_protocol::RunId::new(run.to_string()),
            attempt: AttemptId::new(format!("{run}#a1")),
            generation: 1,
        }
    }

    #[test]
    fn entropy_failure_refusal_carries_its_own_code() {
        // R04-T02-R1-O04: the CSPRNG mint failure is NOT a capacity
        // condition — the dedicated refusal code keeps capacity triage
        // honest (the pre-repair mapping reported
        // `gateway_prepared_registry_full`, which pointed operators at the
        // cap instead of the entropy source).
        let refusal = GatewayRefusal::HandleMintFailed {
            source: "getrandom unavailable".to_string(),
        };
        assert_eq!(refusal.code(), "gateway_prepared_handle_mint_failed");
        assert_ne!(refusal.code(), "gateway_prepared_registry_full");
        assert!(
            refusal.to_tool_error().message.contains("entropy"),
            "the message names the real cause: {}",
            refusal.to_tool_error().message
        );
    }

    #[test]
    fn fail_closed_policy_allows_read_and_demands_approval_for_execute() {
        let read_input = PolicyAdjudicationInput {
            entry: InvocationEntry::Direct,
            target_id: "t".to_string(),
            local_name: "read".to_string(),
            permission_kind: PermissionKind::Read,
            capability_base: "read.read".to_string(),
            args_digest_hex: "d".to_string(),
            principal_kind: "local_user",
            principal_subject: "user_local".to_string(),
            session_id: "s".to_string(),
            run_id: "r".to_string(),
            agent_id: "a".to_string(),
            permission_context: test_permission(),
        };
        assert_eq!(
            FailClosedPolicy.adjudicate(&read_input),
            PolicyVerdict::Allowed
        );
        for kind in [
            PermissionKind::Execute,
            PermissionKind::ArgumentAwareFile,
            PermissionKind::ArgumentAwareSessionFolders,
        ] {
            let input = PolicyAdjudicationInput {
                permission_kind: kind,
                ..read_input.clone()
            };
            match FailClosedPolicy.adjudicate(&input) {
                PolicyVerdict::NeedsApproval { reason } => {
                    assert!(reason.contains("cannot safely adjudicate"), "{reason}");
                    assert!(
                        reason.contains("never"),
                        "the refusal names the posture: {reason}"
                    );
                }
                other => panic!("execute-class must demand approval, got {other:?}"),
            }
        }
    }

    #[test]
    fn prepared_handles_are_unique_and_single_use() {
        let registry = Arc::new(ToolRegistry::new());
        registry
            .register(
                manifest("probe", PermissionKind::Read, Availability::Available),
                &SchemaBudget::default(),
            )
            .expect("register");
        let gw = gateway(&registry, Arc::new(FailClosedPolicy));
        let ctx = ctx_of("run-handles");
        let call = ToolCallId::new("call-1".to_string());
        let req = InvocationRequest::from_trusted_entry(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            test_permission(),
            ToolTargetRef::ByName {
                name: "probe".to_string(),
            },
            None,
            json!({"path": "/x"}),
            call.clone(),
        );
        let p1 = gw.prepare(req.clone()).expect("prepare 1");
        let p2 = gw.prepare(req).expect("prepare 2");
        assert_ne!(p1.handle, p2.handle, "CSPRNG handles never repeat");
        // Single use: the second spend of p1's handle is refused.
        let call2 = ToolCallId::new("call-2".to_string());
        let req2 = InvocationRequest::from_trusted_entry(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            test_permission(),
            ToolTargetRef::ByName {
                name: "probe".to_string(),
            },
            None,
            json!({"path": "/x"}),
            call2.clone(),
        );
        let _p2b = gw.prepare(req2).expect("prepare for call2");
        // A handle spent under a DIFFERENT tool call id is an identity
        // mismatch (the handle binds the call it was prepared for).
        let err = futures_block_on(gw.execute_prepared(&ctx, &call2, &p1.handle))
            .expect_err("call-id binding");
        assert_eq!(err.code(), "gateway_identity_mismatch");
        // Under its OWN call id but with no executor bound: fail-closed
        // zero-dispatch (consumption already happened; a further attempt
        // is a consumed-handle refusal, proving single-use bookkeeping).
        let err = futures_block_on(gw.execute_prepared(&ctx, &call, &p1.handle))
            .expect_err("no executor bound");
        assert_eq!(err.code(), "gateway_no_executor_bound");
        let err2 = futures_block_on(gw.execute_prepared(&ctx, &call, &p1.handle))
            .expect_err("handle already consumed");
        assert_eq!(err2.code(), "gateway_prepared_handle_consumed");
    }

    #[test]
    fn expired_and_foreign_handles_are_refused() {
        let registry = Arc::new(ToolRegistry::new());
        registry
            .register(
                manifest("probe", PermissionKind::Read, Availability::Available),
                &SchemaBudget::default(),
            )
            .expect("register");
        let manual = clock();
        let gw = Arc::new(ToolInvocationGateway::new(
            Arc::clone(&registry),
            Arc::new(FailClosedPolicy),
            manual.clone(),
            SchemaBudget::default(),
            1_000,
            DEFAULT_LIVE_PREPARED_CAP,
        ));
        let ctx = ctx_of("run-exp");
        let call = ToolCallId::new("call-exp".to_string());
        let req = InvocationRequest::from_trusted_entry(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            test_permission(),
            ToolTargetRef::ByName {
                name: "probe".to_string(),
            },
            None,
            json!({"path": "/x"}),
            call.clone(),
        );
        let prepared = gw.prepare(req).expect("prepare");
        manual.advance(2_000);
        let err = futures_block_on(gw.execute_prepared(&ctx, &call, &prepared.handle))
            .expect_err("expired");
        assert_eq!(err.code(), "gateway_prepared_handle_expired");

        // Cross-run reuse: a fresh handle for run A cannot execute under
        // run B's context.
        let ctx_a = ctx_of("run-a");
        let call_a = ToolCallId::new("call-a".to_string());
        let req_a = InvocationRequest::from_trusted_entry(
            &ctx_a,
            CallerSurface::UserRun,
            "agent",
            test_permission(),
            ToolTargetRef::ByName {
                name: "probe".to_string(),
            },
            None,
            json!({"path": "/x"}),
            call_a.clone(),
        );
        let prepared_a = gw.prepare(req_a).expect("prepare a");
        let ctx_b = ctx_of("run-b");
        let call_b = ToolCallId::new("call-b".to_string());
        let err = futures_block_on(gw.execute_prepared(&ctx_b, &call_b, &prepared_a.handle))
            .expect_err("identity mismatch");
        assert_eq!(err.code(), "gateway_identity_mismatch");

        // A garbage handle string never resolves.
        let forged = PreparedInvocationHandle::parse("prep:not-a-real-nonce");
        let err = futures_block_on(gw.execute_prepared(&ctx_a, &call_a, &forged))
            .expect_err("unknown handle");
        assert_eq!(err.code(), "gateway_prepared_handle_unknown");
    }

    #[test]
    fn delegation_and_subagent_surfaces_record_their_entries() {
        let registry = Arc::new(ToolRegistry::new());
        registry
            .register(
                manifest("probe", PermissionKind::Read, Availability::Available),
                &SchemaBudget::default(),
            )
            .expect("register");
        registry
            .register(
                manifest("probe_od", PermissionKind::Read, Availability::Deferred),
                &SchemaBudget::default(),
            )
            .expect("register deferred");
        let gw = gateway(&registry, Arc::new(FailClosedPolicy));
        let ctx = ctx_of("run-entries");
        let prepare_with = |surface: CallerSurface, name: &str| {
            let call = ToolCallId::new(format!("call-{name}"));
            gw.prepare(InvocationRequest::from_trusted_entry(
                &ctx,
                surface,
                "agent",
                test_permission(),
                ToolTargetRef::ByName {
                    name: name.to_string(),
                },
                None,
                json!({"path": "/x"}),
                call,
            ))
        };
        assert_eq!(
            prepare_with(CallerSurface::UserRun, "probe")
                .expect("resident")
                .entry,
            InvocationEntry::Direct
        );
        assert_eq!(
            prepare_with(CallerSurface::UserRun, "probe_od")
                .expect("deferred")
                .entry,
            InvocationEntry::OnDemand
        );
        assert_eq!(
            prepare_with(CallerSurface::SubagentRun, "probe")
                .expect("subagent")
                .entry,
            InvocationEntry::Subagent
        );
        assert_eq!(
            prepare_with(CallerSurface::DelegationDispatch, "probe")
                .expect("delegation")
                .entry,
            InvocationEntry::Delegation
        );
    }

    #[test]
    fn consumed_records_are_reclaimed_under_cap_pressure() {
        // A tiny cap: the map must reclaim consumed records (bounded
        // memory) while the consumed-ring diagnostic keeps answering
        // replays, and preparation keeps succeeding.
        let registry = Arc::new(ToolRegistry::new());
        registry
            .register(
                manifest("probe", PermissionKind::Read, Availability::Available),
                &SchemaBudget::default(),
            )
            .expect("register");
        let gw = Arc::new(ToolInvocationGateway::new(
            Arc::clone(&registry),
            Arc::new(FailClosedPolicy),
            clock(),
            SchemaBudget::default(),
            DEFAULT_PREPARED_TTL_MS,
            4,
        ));
        let ctx = ctx_of("run-bounded");
        let mut spent = Vec::new();
        for seq in 0..10u32 {
            let call = ToolCallId::new(format!("run-bounded-tc{seq:04}"));
            let prepared = gw
                .prepare(InvocationRequest::from_trusted_entry(
                    &ctx,
                    CallerSurface::UserRun,
                    "agent",
                    test_permission(),
                    ToolTargetRef::ByName {
                        name: "probe".to_string(),
                    },
                    None,
                    json!({"path": "/x"}),
                    call.clone(),
                ))
                .expect("preparation keeps succeeding past the cap");
            // No executor bound: the presentation still consumes (one
            // execution attempt per prepared invocation) and is reclaimed.
            let _ = futures_block_on(gw.execute_prepared(&ctx, &call, &prepared.handle));
            spent.push(prepared.handle);
        }
        // Every spent handle still reports the single-use diagnostic (the
        // bounded ring), and none ever re-executes.
        for handle in &spent {
            let err = futures_block_on(gw.execute_prepared(
                &ctx,
                &ToolCallId::new("run-bounded-tc0000".to_string()),
                handle,
            ))
            .expect_err("spent handles never execute again");
            assert!(matches!(
                err.code(),
                "gateway_prepared_handle_consumed" | "gateway_identity_mismatch"
            ));
        }
    }

    #[test]
    fn schema_defaults_fill_without_digest_forgery_and_reach_the_executor() {
        use std::pin::Pin;
        // A manifest whose schema carries a DEFAULT (like the incumbent's
        // read `mode`): a wire request omitting the defaulted field
        // prepares successfully, the record binds the NORMALIZED (default-
        // filled) digest, and the executor receives the defaulted payload.
        // The wire digest only ever needs to match the WIRE arguments —
        // trusted-boundary default filling is not a forgery (adversarial
        // self-check fix: the pre-fix comparison refused every defaulted
        // call).
        let registry = Arc::new(ToolRegistry::new());
        let mut manifest = manifest("probe_def", PermissionKind::Read, Availability::Available);
        manifest.input_schema.schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "mode": {"type": "string", "enum": ["stat", "text"], "default": "text"},
            },
            "required": ["path"],
            "additionalProperties": false,
        });
        registry
            .register(manifest, &SchemaBudget::default())
            .expect("register");
        let gw = gateway(&registry, Arc::new(FailClosedPolicy));
        #[derive(Default)]
        struct Capture {
            payload: std::sync::Mutex<Option<String>>,
        }
        impl ToolExecutorPort for Capture {
            fn execute<'a>(
                &'a self,
                ctx: &'a RunContext,
                _call: &'a ToolCallId,
                request: &'a ToolRequest,
            ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>>
            {
                *self.payload.lock().unwrap() =
                    Some(String::from_utf8_lossy(request.arguments.canonical_bytes()).to_string());
                let ctx_at_issue = ctx.clone();
                Box::pin(async move {
                    ToolExecutionResult::of_ctx(
                        &ctx_at_issue,
                        lingxi_kernel::ports::ToolOutcome::success_text("ok"),
                    )
                })
            }
        }
        let capture = Arc::new(Capture::default());
        gw.bind_executor(
            ToolTargetId::parse("tool:first-party:probe_def"),
            Arc::clone(&capture) as Arc<dyn ToolExecutorPort>,
            "capture double",
        );
        let ctx = ctx_of("run-defaults");
        let call = ToolCallId::new("run-defaults-tc0001".to_string());
        // The WIRE request omits `mode` (its digest covers {"path":…}).
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:probe_def",
            json!({"path": "/data/defaults.txt"}),
            &SchemaBudget::default(),
        )
        .expect("wire request builds");
        let prepared = gw
            .prepare_from_request(
                &ctx,
                CallerSurface::UserRun,
                "agent",
                test_permission(),
                &call,
                &request,
            )
            .expect("default filling is NOT a digest forgery");
        // The PREPARED digest covers the default-filled payload…
        assert_ne!(prepared.args_digest_hex, request.args_digest.hex);
        // …and the executor receives exactly that server-bound payload.
        futures_block_on(gw.execute_prepared(&ctx, &call, &prepared.handle)).expect("executes");
        assert_eq!(
            capture.payload.lock().unwrap().as_deref(),
            Some(r#"{"mode":"text","path":"/data/defaults.txt"}"#)
        );
        // A FORGED wire pair is still refused: digest of OTHER arguments
        // smuggled into args_digest.
        let mut forged = ToolRequest::from_effective_arguments(
            "tool:first-party:probe_def",
            json!({"path": "/data/payload-B.txt"}),
            &SchemaBudget::default(),
        )
        .expect("builds");
        forged.args_digest =
            lingxi_protocol::digest_arguments(&json!({"path": "/data/summary-A.txt"}));
        match gw.prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            test_permission(),
            &call,
            &forged,
        ) {
            Err(refusal) => assert_eq!(refusal.code(), "gateway_digest_mismatch"),
            Ok(_) => panic!("the smuggled wire pair must be refused"),
        }
    }

    /// Minimal block_on for the sync unit tests here (the gateway's own
    /// futures never suspend when the executor double is immediate).
    fn futures_block_on<F: std::future::Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        unsafe fn noop_clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        unsafe fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(fut);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("gateway test future suspended unexpectedly"),
        }
    }
}
