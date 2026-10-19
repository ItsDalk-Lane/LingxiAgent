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

use lingxi_kernel::ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest};
use lingxi_kernel::toolcatalog::{
    CatalogPin, EffectiveArguments, PermissionKind, SchemaBudget, ToolRegistry, ToolTargetId,
    ToolTargetRef,
};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId};

use crate::inject::ServiceClock;
use crate::resourceaccess::ResourceScope;

thread_local! {
    static PREPARE_MCP_FOREGROUND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PREPARE_SETTINGS_PRINCIPAL: std::cell::RefCell<Option<crate::auth::Principal>> = const { std::cell::RefCell::new(None) };
}

/// 只在同步prepare期间携带真实入队身份；执行时使用准备记录的冻结副本。
pub(crate) fn with_settings_principal<T>(
    principal: Option<crate::auth::Principal>,
    action: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<crate::auth::Principal>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PREPARE_SETTINGS_PRINCIPAL.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(PREPARE_SETTINGS_PRINCIPAL.with(|slot| slot.replace(principal)));
    action()
}

/// 仅由前台会话受理链冻结；后台与子代理不能继承临时会话能力。
pub(crate) fn with_mcp_foreground<T>(foreground: bool, action: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            PREPARE_MCP_FOREGROUND.with(|slot| slot.set(self.0));
        }
    }
    let _restore = Restore(PREPARE_MCP_FOREGROUND.with(|slot| slot.replace(foreground)));
    action()
}

/// 监督器在真实工具子任务内注入生命周期；不从 arguments 读取。
#[derive(Clone)]
pub(crate) struct TrustedToolLifetime {
    pub workspace: Option<std::path::PathBuf>,
    pub cancel: crate::cancel::CancelScope,
    pub deadline_unix_ms: u64,
    pub budget_ms: u64,
}
/// 仅由实际调度点构造，绑定网关冻结的调用身份与设置归属。
#[derive(Clone)]
pub(crate) struct TrustedSettingsContext {
    principal: Principal,
    settings_principal: Option<crate::auth::Principal>,
    session_id: String,
    run_id: String,
    attempt: String,
    call_id: String,
    agent_id: String,
    permission: InvocationPermissionContext,
}
impl TrustedSettingsContext {
    pub(crate) fn settings_principal(&self) -> Option<&crate::auth::Principal> {
        self.settings_principal.as_ref()
    }
    pub(crate) fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub(crate) fn permission(&self) -> InvocationPermissionContext {
        self.permission
    }
}
tokio::task_local! {
    pub(crate) static TOOL_LIFETIME: TrustedToolLifetime;
    static KNOWLEDGE_RUNTIME: crate::operations::EmbeddingCallRuntime;
    static REVIEW_SCOPE: Option<serde_json::Value>;
    static EXECUTION_RESOURCES: Vec<ResourceScope>;
    static PREPARE_WORKSPACE: Option<std::path::PathBuf>;
    pub(crate) static WORKFLOW_SCOPE: Option<crate::runs::WorkflowNodeScope>;
    static SETTINGS_CONTEXT: TrustedSettingsContext;
}

pub(crate) fn trusted_settings_context(
    ctx: &RunContext,
    call: &ToolCallId,
) -> Result<TrustedSettingsContext, String> {
    SETTINGS_CONTEXT
        .try_with(Clone::clone)
        .map_err(|_| "设置工具缺少网关冻结的调用范围".to_string())
        .and_then(|bound| {
            if bound.principal != ctx.principal
                || bound.session_id != ctx.session_id.as_str()
                || bound.run_id != ctx.run_id.as_str()
                || bound.attempt != ctx.attempt.to_string()
                || bound.call_id != call.as_str()
            {
                return Err("设置工具的实际调用身份不一致".into());
            }
            Ok(bound)
        })
}

/// 准备和执行都使用实际运行冻结的工作区，模型不能通过参数替换默认目录。
pub(crate) fn workspace() -> Option<std::path::PathBuf> {
    PREPARE_WORKSPACE
        .try_with(Clone::clone)
        .ok()
        .flatten()
        .or_else(|| {
            TOOL_LIFETIME
                .try_with(|value| value.workspace.clone())
                .ok()
                .flatten()
        })
}
pub(crate) fn with_workspace<T>(
    workspace: Option<std::path::PathBuf>,
    action: impl FnOnce() -> T,
) -> T {
    PREPARE_WORKSPACE.sync_scope(workspace, action)
}

/// 工作流范围来自服务批准的节点，模型无法通过工具参数扩大。
pub(crate) fn workflow_scope() -> Option<crate::runs::WorkflowNodeScope> {
    WORKFLOW_SCOPE.try_with(Clone::clone).ok().flatten()
}

pub(crate) fn with_workflow_scope<T>(
    scope: Option<crate::runs::WorkflowNodeScope>,
    action: impl FnOnce() -> T,
) -> T {
    WORKFLOW_SCOPE.sync_scope(scope, action)
}

/// 删除执行端只能消费网关已冻结并复核的范围，模型参数不能铸造该范围。
pub(crate) fn review_scope() -> Option<serde_json::Value> {
    REVIEW_SCOPE.try_with(Clone::clone).ok().flatten()
}
/// 执行端只比较网关已绑定的真实来源，不能把审批后的新挂载根当成旧目标。
pub(crate) fn require_bound_resource(
    path: &std::path::Path,
    op: crate::resourceaccess::ResourceOp,
) -> Result<(), String> {
    if EXECUTION_RESOURCES
        .try_with(|scopes| {
            scopes
                .iter()
                .any(|scope| scope.path == path && scope.op == op)
        })
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err("resource differs from the gateway-bound approval scope".into())
    }
}
/// 确认范围来自可信调用身份，模型不能指定待审会话或助手。
pub struct ReviewInput<'a> {
    pub agent_id: &'a str,
    pub session_id: &'a str,
    pub principal_kind: &'a str,
    pub principal_subject: &'a str,
}
pub type ReviewScopeExtractor = Arc<
    dyn Fn(&ReviewInput<'_>, &EffectiveArguments) -> Result<serde_json::Value, String>
        + Send
        + Sync,
>;

pub(crate) fn knowledge_runtime(
    ctx: &RunContext,
    call: &ToolCallId,
) -> Result<crate::operations::EmbeddingCallRuntime, ProtocolError> {
    KNOWLEDGE_RUNTIME
        .try_with(Clone::clone)
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::Unauthorized,
                "knowledge embedding requires a trusted gateway context",
                false,
            )
        })
        .and_then(|runtime| {
            if runtime.ownership.session_id.as_deref() != Some(ctx.session_id.as_str())
                || runtime.ownership.run_id.as_deref() != Some(ctx.run_id.as_str())
                || runtime.ownership.attempt.as_deref() != Some(ctx.attempt.as_str())
                || runtime.ownership.parent_tool_call_id.as_deref() != Some(call.as_str())
                || runtime.ownership.principal_subject != ctx.principal.storage_subject()
            {
                return Err(ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "knowledge embedding owner mismatch",
                    false,
                ));
            }
            Ok(runtime)
        })
}

/// The trusted facts a resource-scope derivation may see (R04-T04). All
/// of them come from the trusted entry's [`RunContext`] — a model
/// payload can never populate this input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceExtractionInput {
    pub principal_kind: &'static str,
    pub principal_subject: String,
    pub session_id: String,
    pub run_id: String,
}

/// Derives the REAL resource scopes of one invocation from its EFFECTIVE
/// arguments (`资源规范化` in the frozen invocation chain — between
/// argument validation and the permission adjudication). For the file
/// tools this resolves and AUTHORIZES the canonical path
/// ([`crate::resourceaccess::ResourceAccess::authorize`]); an
/// unauthorized scope is a loud preparation refusal, never a value the
/// approver is asked to bless.
pub type ResourceExtractor = std::sync::Arc<
    dyn Fn(&ResourceExtractionInput, &EffectiveArguments) -> Result<Vec<ResourceScope>, String>
        + Send
        + Sync,
>;

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
    /// 后台自动化：Ask 对应自动审核，任何档位都不能等待人工确认。
    Automation {
        mode: lingxi_kernel::subagent::SessionPermissionMode,
    },
    Bridge {
        role: lingxi_protocol::BridgeRole,
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
            InvocationPermissionContext::Automation { .. } => "automation",
            InvocationPermissionContext::Bridge { .. } => "bridge",
            InvocationPermissionContext::Subagent { .. } => "subagent",
        }
    }
}

/// 外部发送者不能操作本机屏幕；同一检查覆盖声明、直接调用和被解析后的真实目标。
pub fn bridge_forbidden_tool(name: &str, capability: &str) -> bool {
    let name = name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase();
    name.contains("computer")
        || name.contains("browser")
        || capability.starts_with("computer.")
        || capability.starts_with("browser.")
}

pub(crate) fn bridge_policy(input: &PolicyAdjudicationInput) -> PolicyVerdict {
    use lingxi_kernel::{subagent::SessionPermissionMode, toolcatalog::PermissionKind};
    let InvocationPermissionContext::Bridge { role, mode } = input.permission_context else {
        unreachable!("Bridge policy only accepts host Bridge facts")
    };
    let deny = |code: &str, message: &str| PolicyVerdict::Denied {
        code: code.into(),
        layer: "bridge".into(),
        message: message.into(),
    };
    if role == lingxi_protocol::BridgeRole::Guest {
        return deny("BRIDGE_GUEST_NO_TOOLS", "普通桥接发送者只能文本问答");
    }
    if bridge_forbidden_tool(&input.local_name, &input.capability_base) {
        return deny("BRIDGE_TOOL_FORBIDDEN", "桥接不能操作本机浏览器或屏幕");
    }
    if input.permission_kind == PermissionKind::Review {
        return deny(
            crate::approval_service::TOOL_APPROVAL_UNAVAILABLE,
            "桥接不能等待人工确认此操作",
        );
    }
    match (mode, input.permission_kind) {
        (SessionPermissionMode::ReadOnly, PermissionKind::Read)
        | (SessionPermissionMode::Ask, PermissionKind::Read)
        | (SessionPermissionMode::Operate, _) => PolicyVerdict::Allowed,
        (SessionPermissionMode::ReadOnly, _) => deny(
            crate::approval_service::ACTION_BLOCKED_BY_READ_ONLY,
            "桥接继承只读权限",
        ),
        _ => deny(
            crate::approval_service::TOOL_APPROVAL_UNAVAILABLE,
            "桥接当前权限不能确认此操作",
        ),
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

/// 网关完成当前目录与参数校验后传给策略的事实，不从模型声明中采信权限。
/// 独立于旧策略输入，保持现有策略端口与测试调用的兼容性。
#[derive(Debug, Clone, PartialEq)]
pub struct InvocationPolicyMaterial {
    pub origin: lingxi_kernel::toolcatalog::ToolOrigin,
    pub declared_permission: lingxi_kernel::toolcatalog::DeclaredPermission,
    pub effective_arguments: serde_json::Value,
    pub resources: Vec<ResourceScope>,
}

/// The tool-policy service port (R04-T02 怎么做 3). R04-T03's ApprovalService
/// is the real production implementation; until it is wired the
/// fail-closed default ([`FailClosedPolicy`]) applies. The port is
/// synchronous and pure on its input: it may only LOOK at facts, never
/// mint new ones.
pub trait ToolPolicyPort: Send + Sync {
    fn adjudicate(&self, input: &PolicyAdjudicationInput) -> PolicyVerdict;

    /// 旧实现继续使用原裁决；自动化策略可以检查已冻结的真实动作和参数。
    fn adjudicate_invocation(
        &self,
        input: &PolicyAdjudicationInput,
        _material: &InvocationPolicyMaterial,
    ) -> PolicyVerdict {
        self.adjudicate(input)
    }
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
            | PermissionKind::Review
            | PermissionKind::ArgumentAwareFile
            | PermissionKind::ArgumentAwareSessionFolders
            | PermissionKind::ArgumentAwareCheckpoint
            | PermissionKind::ArgumentAwareChannel
            | PermissionKind::ArgumentAwareBrowser
            | PermissionKind::ArgumentAwareGoal
            | PermissionKind::ArgumentAwareSettings => PolicyVerdict::NeedsApproval {
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
    /// The invocation's REAL resource scope could not be derived or is
    /// not authorized (R04-T04: `资源规范化` refuses before any approval
    /// or dispatch — an approver is never asked to bless an
    /// unauthorized path).
    ResourceScopeDenied { code: String, message: String },
    /// The resource scope changed between prepare and execute (the real
    /// target moved / was replaced by a link). Zero dispatch.
    ResourceScopeChanged {
        target_id: String,
        bound: String,
        current: String,
    },
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
            GatewayRefusal::ResourceScopeDenied { .. } => "gateway_resource_scope_denied",
            GatewayRefusal::ResourceScopeChanged { .. } => "gateway_resource_scope_changed",
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
            GatewayRefusal::ResourceScopeDenied { code, message } => (
                ErrorCode::Forbidden,
                format!("tool resource scope refused: {code}: {message}"),
            ),
            GatewayRefusal::ResourceScopeChanged {
                target_id,
                bound,
                current,
            } => (
                ErrorCode::Conflict,
                format!(
                    "tool target {target_id}: the invocation's real resource scope changed \
                     between preparation and execution (bound {bound}, now {current}); refusing \
                     with zero dispatch — re-prepare the invocation"
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
    /// The REAL resource scopes this invocation was prepared against
    /// (R04-T04: canonical authorized paths + operations). Empty for
    /// targets without a resource extractor. This is what the approval
    /// record binds (the approver approves exactly this scope) and what
    /// `execute_prepared` re-derives before dispatch.
    pub resources: Vec<ResourceScope>,
    /// 对实际匹配的删除条目生成的确认摘要。
    pub review_summary: Option<String>,
}

/// Server-side binding record (never handed to the model). The
/// caller-visible [`PreparedInvocation`] view carries the decision facts
/// (local name, policy verdict); this record carries only what
/// re-verification and dispatch consume.
#[derive(Debug, Clone)]
struct PreparedRecord {
    mcp_session_grant: Option<u64>,
    settings_principal: Option<crate::auth::Principal>,
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
    /// The REAL resource scopes bound at preparation (R04-T04): what the
    /// approval record carries and what execute-time re-derivation must
    /// reproduce.
    resources: Vec<ResourceScope>,
    review_scope: Option<serde_json::Value>,
}

/// One bound executor + the registrar's reason (audit: why this executor
/// may serve this target through the gateway) and, when the target owns
/// real resources, its resource-scope derivation (R04-T04).
#[derive(Clone)]
struct ExecutorBinding {
    executor: Arc<dyn ToolExecutorPort>,
    reason: String,
    resources: Option<ResourceExtractor>,
    review: Option<ReviewScopeExtractor>,
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
    session_capabilities: crate::mcp_session_permissions::SessionCapabilities,
    catalog_definitions:
        std::sync::Mutex<Option<Arc<crate::toolcatalog_bridge::CatalogDefinitions>>>,
    budget: SchemaBudget,
    policy: Arc<dyn ToolPolicyPort>,
    automatic_reviewer: std::sync::Mutex<Option<Arc<dyn crate::automation_review::AutoReviewPort>>>,
    automatic_reviews: std::sync::Mutex<AutomaticReviewState>,
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

// 已审阅的工具调用与其同一次检查产生的权限、范围和确认材料。
type AdmissionChecks = (
    lingxi_kernel::toolcatalog::PreparedToolCall,
    InvocationEntry,
    Vec<ResourceScope>,
    PolicyVerdict,
    Option<serde_json::Value>,
    Option<AutomaticReviewBinding>,
    Option<u64>,
);

#[derive(Clone, PartialEq, Eq)]
struct AutomaticReviewBinding {
    key: String,
    resources: Vec<ResourceScope>,
}
struct AutomaticReviewGrant {
    binding: AutomaticReviewBinding,
    expires_at_unix_ms: u64,
    consumed: bool,
}
#[derive(Default)]
struct AutomaticReviewState {
    grants: Vec<AutomaticReviewGrant>,
    in_flight: std::collections::HashSet<String>,
}
struct InFlightAutomaticReview<'a> {
    state: &'a std::sync::Mutex<AutomaticReviewState>,
    key: String,
}
impl Drop for InFlightAutomaticReview<'_> {
    fn drop(&mut self) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .in_flight
            .remove(&self.key);
    }
}

fn automatic_review_refusal(message: &str) -> GatewayRefusal {
    GatewayRefusal::PolicyDenied {
        code: crate::approval_service::TOOL_APPROVAL_UNAVAILABLE.into(),
        layer: "automation_approval".into(),
        message: message.into(),
    }
}

fn automatic_review_binding(
    request: &InvocationRequest,
    prepared: &lingxi_kernel::toolcatalog::PreparedToolCall,
    entry: InvocationEntry,
    resources: &[ResourceScope],
) -> AutomaticReviewBinding {
    let key = lingxi_protocol::digest_arguments(&serde_json::json!({
        "principalKind":request.principal.storage_kind(),
        "principalSubject":request.principal.storage_subject(),
        "sessionId":request.session_id,"runId":request.run_id,"attempt":request.attempt,
        "generation":request.generation,"agentId":request.agent_id,
        "surface":format!("{:?}",request.surface),"permission":format!("{:?}",request.permission),
        "entry":entry.wire_name(),"callId":request.tool_call_id.as_str(),
        "targetId":prepared.target_id.as_str(),"digest":prepared.args_digest.hex,
        "capability":prepared.permission.capability_base,
        "catalogGeneration":prepared.catalog_generation,"targetGeneration":prepared.target_generation,
    })).hex;
    AutomaticReviewBinding {
        key,
        resources: resources.to_vec(),
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
            session_capabilities: Default::default(),
            catalog_definitions: std::sync::Mutex::new(None),
            budget,
            policy,
            automatic_reviewer: std::sync::Mutex::new(None),
            automatic_reviews: std::sync::Mutex::new(AutomaticReviewState::default()),
            executors: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            prepared: std::sync::Mutex::new(HashMap::new()),
            consumed_ring: std::sync::Mutex::new(std::collections::VecDeque::new()),
            clock,
            prepared_ttl_ms,
            live_prepared_cap,
        }
    }

    /// 只读观测真实授权处理器阶段；默认不装配，不返回授权决定。
    pub fn set_mcp_session_grant_read_observer(
        &self,
        observer: Option<crate::mcp_session_permissions::GrantReadObserver>,
    ) {
        self.session_capabilities.set_read_observer(observer);
    }
    pub(crate) fn begin_mcp_session_grant(
        &self,
        session: &str,
    ) -> Result<
        crate::mcp_session_permissions::GrantEpoch,
        crate::mcp_session_permissions::GrantError,
    > {
        self.session_capabilities
            .begin(session, self.clock.now_unix_ms())
    }
    pub(crate) fn observe_mcp_session_grant_read(&self, session: &str) {
        self.session_capabilities.observe_read(session);
    }

    /// 审批页面只展示真实目录给出的能力，不根据工具显示名拼接权限。
    pub(crate) fn mcp_session_capability(&self, target: &str) -> Option<String> {
        let listing = self.registry.describe(&ToolTargetId::parse(target)).ok()?;
        (matches!(
            listing.origin,
            lingxi_kernel::toolcatalog::ToolOrigin::Mcp { .. }
        ) && listing.availability.callable()
            && listing.permission.kind == PermissionKind::Execute)
            .then_some(listing.permission.capability_base)
    }

    /// 只接受实际会话与目录事实；会话能力不改变目录、工具权限或永久配置。
    pub(crate) fn grant_mcp_session_permission(
        &self,
        state: &crate::ServiceState,
        principal: &crate::auth::Principal,
        row: &lingxi_adapters::storage::session_tree::SessionTreeRow,
        epoch: &crate::mcp_session_permissions::GrantEpoch,
        request: &lingxi_protocol::McpSessionPermissionRequest,
        secure_transport: bool,
    ) -> Result<(), crate::mcp_session_permissions::GrantError> {
        if !crate::sessions::SessionStore::can_access_session(
            principal,
            &row.owner_user_id,
            &row.session_id,
        ) || !principal.has_scope("sessions.write")
        {
            return Err("会话授权身份或范围不匹配".into());
        }
        let target = ToolTargetId::parse(&request.target_id);
        let listing = self
            .registry
            .describe(&target)
            .map_err(|_| "MCP工具不存在")?;
        if !matches!(
            listing.origin,
            lingxi_kernel::toolcatalog::ToolOrigin::Mcp { .. }
        ) || !listing.availability.callable()
            || listing.permission.kind != PermissionKind::Execute
            || listing.permission.capability_base != request.capability
        {
            return Err("目标并非当前可调用的精确MCP能力".into());
        }
        let derive = self
            .executors
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&target)
            .and_then(|binding| binding.review.clone())
            .ok_or("MCP工具缺少受限管理绑定")?;
        let empty = EffectiveArguments::from_value(serde_json::json!({}), &self.budget)
            .map_err(|e| e.to_string())?;
        let review_scope = derive(
            &ReviewInput {
                agent_id: &row.agent_id,
                session_id: &row.session_id,
                principal_kind: crate::sessions::kernel_principal_of(principal).storage_kind(),
                principal_subject: &crate::sessions::kernel_principal_of(principal)
                    .storage_subject(),
            },
            &empty,
        )?;
        let current = self
            .registry
            .describe(&target)
            .map_err(|_| "MCP目录已改变")?;
        if current.target_generation != listing.target_generation
            || !current.availability.callable()
        {
            return Err("MCP目录已改变，请重新确认".into());
        }
        self.session_capabilities.grant(
            crate::mcp_session_permissions::CapabilityBinding {
                principal: principal.clone(),
                session_id: row.session_id.clone(),
                agent_id: row.agent_id.clone(),
                target_id: request.target_id.clone(),
                target_generation: listing.target_generation,
                capability: request.capability.clone(),
                review_scope,
            },
            epoch,
            state,
            row,
            secure_transport,
        )
    }

    /// 服务关闭时清空且封闭内存授权表；已有执行凭据也要重新核对此闸。
    pub(crate) fn close_mcp_session_permissions(&self) {
        self.session_capabilities.close();
    }
    pub(crate) fn revoke_mcp_session_permissions(&self, session: &str) {
        self.session_capabilities.revoke_session(session);
    }

    /// 仅可信装配入口提供真实审核器，HTTP 或模型参数不能替换它。
    pub fn set_automatic_reviewer(
        &self,
        reviewer: Arc<dyn crate::automation_review::AutoReviewPort>,
    ) {
        *self
            .automatic_reviewer
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(reviewer);
    }

    pub fn attach_catalog_definitions(
        &self,
        definitions: Arc<crate::toolcatalog_bridge::CatalogDefinitions>,
    ) {
        *self
            .catalog_definitions
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(definitions);
    }
    pub fn has_catalog_definitions(&self) -> bool {
        self.catalog_definitions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }
    pub fn catalog_run_lease(
        &self,
        ctx: &RunContext,
    ) -> Option<crate::toolcatalog_bridge::CatalogRunLease> {
        self.catalog_definitions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|definitions| crate::toolcatalog_bridge::CatalogRunLease {
                definitions: Arc::clone(definitions),
                context: ctx.clone(),
            })
    }
    pub fn normalize_model_request(
        &self,
        ctx: &RunContext,
        request: &ToolRequest,
        pin: Option<CatalogPin>,
    ) -> Result<(ToolRequest, Option<CatalogPin>), GatewayRefusal> {
        let definitions = self
            .catalog_definitions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        match definitions {
            Some(definitions) => definitions.normalize(&self.registry, ctx, request, pin),
            None => Ok((request.clone(), pin)),
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
        self.bind_executor_with_resources(target_id, executor, None, reason);
    }

    /// Binds the executor WITH its resource-scope derivation (R04-T04):
    /// every preparation of this target derives + AUTHORIZES the real
    /// resource scopes before the permission adjudication (`资源规范化`
    /// precedes 权限/批准 in the frozen chain), and every execution
    /// re-derives them and must reproduce the bound scopes.
    pub fn bind_executor_with_resources(
        &self,
        target_id: ToolTargetId,
        executor: Arc<dyn ToolExecutorPort>,
        resources: Option<ResourceExtractor>,
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
                    resources,
                    review: None,
                },
            );
    }

    /// 注册业务资源的实际删除范围；范围与文件权限分别校验。
    pub fn bind_executor_with_review(
        &self,
        target_id: ToolTargetId,
        executor: Arc<dyn ToolExecutorPort>,
        review: ReviewScopeExtractor,
        reason: &str,
    ) {
        self.bind_executor(target_id.clone(), executor, reason);
        self.executors
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(&target_id)
            .expect("刚绑定的执行端")
            .review = Some(review);
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

    /// The registry this gateway resolves targets against (composition
    /// and test surfaces register additional targets through it; every
    /// EXECUTION still funnels through `prepare`/`execute_prepared`).
    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
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
        let (
            prepared_tool_call,
            entry,
            resources,
            policy,
            review_scope,
            automatic_grant,
            mcp_session_grant,
        ) = self.admission_checks(&request)?;
        let review_summary = review_scope
            .as_ref()
            .filter(|_| {
                prepared_tool_call.permission.kind
                    == lingxi_kernel::toolcatalog::PermissionKind::Review
            })
            .map(|scope| -> Result<String, GatewayRefusal> {
                if let Some(summary) = scope
                    .get("summary")
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                {
                    return Ok(summary.to_string());
                }
                // 保留既有置顶原则的逐项确认；其他工具不能被猜作删除原则，也不能使服务panic。
                let items = scope["items"].as_array().ok_or_else(|| {
                    GatewayRefusal::ResourceScopeDenied {
                        code: "review_summary_unavailable".into(),
                        message: "工具没有提供可核对的操作摘要，无法请求确认".into(),
                    }
                })?;
                Ok(format!(
                    "删除 {} 条置顶原则（助手 {}）：\n{}",
                    items.len(),
                    scope["agentId"].as_str().unwrap_or(""),
                    items
                        .iter()
                        .map(|item| format!(
                            "- {}：{}",
                            item["id"].as_str().unwrap_or(""),
                            item["content"].as_str().unwrap_or("")
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            })
            .transpose()?;
        let args_digest_hex = prepared_tool_call.args_digest.hex.clone();
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
        let settings_principal = PREPARE_SETTINGS_PRINCIPAL
            .with(|slot| slot.borrow().clone())
            .filter(|original| crate::sessions::kernel_principal_of(original) == request.principal);
        let record = PreparedRecord {
            mcp_session_grant,
            settings_principal,
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
            resources: resources.clone(),
            review_scope,
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
        // 预验可重复读取授权；只有铸造这一张执行凭据时消费，不能重复准备两张凭据。
        if let Some(binding) = automatic_grant {
            let now = self.clock.now_unix_ms();
            let mut reviews = self
                .automatic_reviews
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let grant = reviews
                .grants
                .iter_mut()
                .find(|grant| {
                    grant.binding == binding && !grant.consumed && now <= grant.expires_at_unix_ms
                })
                .ok_or_else(|| automatic_review_refusal("本次自动审核授权已失效或已消费"))?;
            grant.consumed = true;
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
            resources,
            review_summary,
        })
    }

    /// The pure ADMISSION half of [`Self::prepare`]: target resolution →
    /// availability → generation pin → schema validation + normalization →
    /// trusted digest → resource normalization → policy adjudication. No
    /// prepared record is minted and nothing is registered — the R05 RR1
    /// F11 whole-batch admission runs this for EVERY request of a model
    /// turn BEFORE the first side effect, then the per-request execution
    /// path re-runs the full `prepare` at its own moment (pre-validation
    /// never replaces the immediate authorization).
    ///
    /// Refusal honesty (R06-AR-F05): a stale-pin refusal whose reference
    /// resolves at the CURRENT generation to a target that is no longer
    /// callable surfaces that decisive fact instead. The kernel keeps its
    /// stale-first prepare contract (R04-A02: the caller's whole view is
    /// outdated); this only picks the model-facing refusal. For a
    /// revoked/disabled tool the generic "re-describe and re-prepare"
    /// advice would be a lie — no fresh snapshot makes it callable —
    /// while a mere description/schema change still reports the
    /// recoverable stale-catalog path. Uninstalled/renamed/ambiguous
    /// references keep the stale-catalog refusal (a fresh snapshot
    /// remains the caller's real next step there).
    fn refuse_with_current_callability(
        &self,
        err: lingxi_kernel::toolcatalog::ToolCatalogError,
        reference: &ToolTargetRef,
    ) -> GatewayRefusal {
        use lingxi_kernel::toolcatalog::ToolCatalogError;
        let ToolCatalogError::StaleCatalog {
            held_generation,
            current_generation,
            ..
        } = err
        else {
            return map_catalog_error(err);
        };
        let revoked_now = self.registry.resolve(reference).ok().and_then(|target_id| {
            let listing = self.registry.describe(&target_id).ok()?;
            (!listing.availability.callable()).then(|| GatewayRefusal::TargetNotCallable {
                target_id: target_id.as_str().to_string(),
                availability: listing.availability.wire_name().to_string(),
                // Keep BOTH facts in the refusal: the tool is not callable
                // AND the caller's pinned catalog view is stale (the
                // journal detail carries this Display verbatim).
                reason: format!(
                    "{}; the pinned catalog view is stale (held generation \
                     {held_generation}, registry at {current_generation})",
                    listing.availability.refusal_reason()
                ),
            })
        });
        revoked_now.unwrap_or_else(|| map_catalog_error(err))
    }

    fn admission_checks(
        &self,
        request: &InvocationRequest,
    ) -> Result<AdmissionChecks, GatewayRefusal> {
        self.admission_checks_with_review(request, false)
    }

    fn admission_checks_with_review(
        &self,
        request: &InvocationRequest,
        review_probe: bool,
    ) -> Result<AdmissionChecks, GatewayRefusal> {
        let prepared_tool_call = self
            .registry
            .prepare_invocation(
                &request.reference,
                request.pin,
                &request.raw_arguments,
                &self.budget,
            )
            .map_err(|err| self.refuse_with_current_callability(err, &request.reference))?;
        // 确认卡只代表提议，仍不能借 Read 声明绕过当前实际只读授权。
        let read_only_proposal = match request.permission {
            InvocationPermissionContext::UserSession { mode }
            | InvocationPermissionContext::Automation { mode }
            | InvocationPermissionContext::Bridge { mode, .. } => {
                mode == lingxi_kernel::subagent::SessionPermissionMode::ReadOnly
            }
            InvocationPermissionContext::Subagent { tier, parent_mode } => {
                tier == lingxi_kernel::subagent::ToolAccessTier::ReadOnly
                    || parent_mode == lingxi_kernel::subagent::SessionPermissionMode::ReadOnly
            }
        };
        if prepared_tool_call.origin == lingxi_kernel::toolcatalog::ToolOrigin::FirstParty
            && prepared_tool_call.local_name == "session"
            && matches!(
                prepared_tool_call.effective.as_value()["action"].as_str(),
                Some("send" | "create")
            )
            && read_only_proposal
        {
            return Err(GatewayRefusal::PolicyDenied {
                code: crate::approval_service::ACTION_BLOCKED_BY_READ_ONLY.into(),
                layer: "session".into(),
                message: "只读运行不能提出发送或创建会话的确认草稿".into(),
            });
        }
        let bridge_principal = matches!(
            request.principal,
            Principal::Bridge { .. } | Principal::BridgeTakeover { .. }
        );
        let bridge_context = matches!(
            request.permission,
            InvocationPermissionContext::Bridge { .. }
        );
        if bridge_context && !bridge_principal
            || bridge_principal
                && !matches!(
                    request.permission,
                    InvocationPermissionContext::Bridge { .. }
                        | InvocationPermissionContext::Subagent { .. }
                )
        {
            return Err(GatewayRefusal::PolicyDenied {
                code: "BRIDGE_IDENTITY_MISMATCH".into(),
                layer: "bridge".into(),
                message: "桥接身份与宿主授权不一致".into(),
            });
        }
        if bridge_principal
            && (matches!(
                request.permission,
                InvocationPermissionContext::Bridge {
                    role: lingxi_protocol::BridgeRole::Guest,
                    ..
                }
            ) || bridge_forbidden_tool(
                &prepared_tool_call.local_name,
                &prepared_tool_call.permission.capability_base,
            ))
        {
            return Err(GatewayRefusal::PolicyDenied {
                code: "BRIDGE_TOOL_FORBIDDEN".into(),
                layer: "bridge".into(),
                message: "桥接来源不能执行该工具".into(),
            });
        }
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
        // ── R04-T04: resource normalization (`资源规范化`) ──
        // Between argument validation and the permission adjudication
        // (the frozen §4.2 chain: 目标解析→可用性→参数校验→资源规范化→
        // 权限→批准→…). A target with a resource extractor derives the
        // REAL canonical scopes now; an unauthorized scope is a LOUD
        // preparation refusal — an approver is never asked to bless a
        // path the resource boundary would refuse, and nothing is
        // dispatched.
        let resources: Vec<ResourceScope> = {
            let executors = self
                .executors
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match executors
                .get(&prepared_tool_call.target_id)
                .and_then(|binding| binding.resources.as_ref())
            {
                Some(extractor) => {
                    let input = ResourceExtractionInput {
                        principal_kind: request.principal.storage_kind(),
                        principal_subject: request.principal.storage_subject(),
                        session_id: request.session_id.clone(),
                        run_id: request.run_id.clone(),
                    };
                    match extractor(&input, &prepared_tool_call.effective) {
                        Ok(scopes) => scopes,
                        Err(message) => {
                            tracing::warn!(
                                run_id = %request.run_id,
                                target = %prepared_tool_call.target_id,
                                "{message}"
                            );
                            return Err(GatewayRefusal::ResourceScopeDenied {
                                code: "resource_scope_refused".to_string(),
                                message,
                            });
                        }
                    }
                }
                None => Vec::new(),
            }
        };
        let review_scope = {
            let bindings = self.executors.lock().unwrap_or_else(|p| p.into_inner());
            bindings
                .get(&prepared_tool_call.target_id)
                .and_then(|b| b.review.as_ref())
                .map(|derive| {
                    derive(
                        &ReviewInput {
                            agent_id: &request.agent_id,
                            session_id: &request.session_id,
                            principal_kind: request.principal.storage_kind(),
                            principal_subject: &request.principal.storage_subject(),
                        },
                        &prepared_tool_call.effective,
                    )
                })
                .transpose()
                .map_err(|message| GatewayRefusal::ResourceScopeDenied {
                    code: "review_scope_refused".into(),
                    message,
                })?
        };
        let policy_input = PolicyAdjudicationInput {
            entry,
            target_id: prepared_tool_call.target_id.as_str().to_string(),
            local_name: prepared_tool_call.local_name.clone(),
            permission_kind: prepared_tool_call.permission.kind,
            capability_base: prepared_tool_call.permission.capability_base.clone(),
            args_digest_hex: prepared_tool_call.args_digest.hex.clone(),
            principal_kind: request.principal.storage_kind(),
            principal_subject: request.principal.storage_subject(),
            session_id: request.session_id.clone(),
            run_id: request.run_id.clone(),
            agent_id: request.agent_id.clone(),
            permission_context: request.permission,
        };
        let listing = self
            .registry
            .describe(&prepared_tool_call.target_id)
            .map_err(map_catalog_error)?;
        let material = InvocationPolicyMaterial {
            origin: prepared_tool_call.origin.clone(),
            declared_permission: listing.declared_permission,
            effective_arguments: prepared_tool_call.effective.as_value().clone(),
            resources: resources.clone(),
        };
        let mut verdict = self.policy.adjudicate_invocation(&policy_input, &material);
        let session_binding = if PREPARE_MCP_FOREGROUND.with(|slot| slot.get())
            && matches!(
                request.permission,
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Ask
                }
            )
            && request.surface == CallerSurface::UserRun
            && matches!(
                prepared_tool_call.origin,
                lingxi_kernel::toolcatalog::ToolOrigin::Mcp { .. }
            )
            && prepared_tool_call.permission.kind == PermissionKind::Execute
            && matches!(verdict, PolicyVerdict::NeedsApproval { .. })
        {
            PREPARE_SETTINGS_PRINCIPAL
                .with(|slot| slot.borrow().clone())
                .filter(|original| {
                    crate::sessions::kernel_principal_of(original) == request.principal
                })
                .and_then(|principal| {
                    review_scope.clone().map(|review_scope| {
                        crate::mcp_session_permissions::CapabilityBinding {
                            principal,
                            session_id: request.session_id.clone(),
                            agent_id: request.agent_id.clone(),
                            target_id: prepared_tool_call.target_id.to_string(),
                            target_generation: prepared_tool_call.target_generation,
                            capability: prepared_tool_call.permission.capability_base.clone(),
                            review_scope,
                        }
                    })
                })
        } else {
            None
        };
        let mcp_session_grant = session_binding.as_ref().and_then(|binding| {
            self.session_capabilities
                .find(binding, self.clock.now_unix_ms())
        });
        if mcp_session_grant.is_some() {
            verdict = PolicyVerdict::Allowed;
        }

        if matches!(
            request.permission,
            InvocationPermissionContext::Bridge { .. }
        ) {
            let hard = if matches!(
                request.permission,
                InvocationPermissionContext::Bridge {
                    role: lingxi_protocol::BridgeRole::Guest,
                    ..
                }
            ) || prepared_tool_call.permission.kind
                == lingxi_kernel::toolcatalog::PermissionKind::Review
            {
                bridge_policy(&policy_input)
            } else {
                crate::automation_permission::adjudicate(&policy_input, Some(&material))
            };
            if matches!(hard, PolicyVerdict::Denied { .. }) {
                verdict = hard;
            }
            if matches!(verdict, PolicyVerdict::NeedsApproval { .. }) {
                verdict = PolicyVerdict::Denied {
                    code: crate::approval_service::TOOL_APPROVAL_UNAVAILABLE.into(),
                    layer: "bridge".into(),
                    message: "桥接不能等待人工批准".into(),
                };
            }
        }
        let reviewable =
            crate::automation_permission::requires_automatic_review(&policy_input, &material);
        let mut automatic_grant = None;
        if reviewable
            && (matches!(&verdict,PolicyVerdict::Denied{code,..} if code==crate::approval_service::TOOL_APPROVAL_UNAVAILABLE)
                || matches!(&verdict, PolicyVerdict::NeedsApproval { .. }))
        {
            let binding = automatic_review_binding(request, &prepared_tool_call, entry, &resources);
            let now = self.clock.now_unix_ms();
            let mut reviews = self
                .automatic_reviews
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            reviews
                .grants
                .retain(|grant| now <= grant.expires_at_unix_ms);
            if reviews
                .grants
                .iter()
                .any(|grant| grant.binding == binding && !grant.consumed)
            {
                verdict = PolicyVerdict::Allowed;
                automatic_grant = Some(binding);
            } else if review_probe {
                verdict = PolicyVerdict::NeedsApproval {
                    reason: "本次动作需要真实自动审核".into(),
                };
            }
        }
        // 即使接入旧策略或未配置策略，后台也不能进入永远无人回答的等待。
        let verdict = match (request.permission, verdict) {
            (
                InvocationPermissionContext::Automation { .. },
                verdict @ PolicyVerdict::NeedsApproval { .. },
            ) if review_probe && reviewable => verdict,
            (
                InvocationPermissionContext::Automation { .. },
                PolicyVerdict::NeedsApproval { reason },
            ) => PolicyVerdict::Denied {
                code: crate::approval_service::TOOL_APPROVAL_UNAVAILABLE.into(),
                layer: "automation_approval".into(),
                message: format!("后台任务无法等待人工确认，操作未执行：{reason}"),
            },
            (_, verdict) => verdict,
        };
        let policy = match verdict {
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
        Ok((
            prepared_tool_call,
            entry,
            resources,
            policy,
            review_scope,
            automatic_grant,
            mcp_session_grant,
        ))
    }

    /// 整批预验中的异步审核入口；审核后重验原参数、权限、资源及目录代次。
    /// 只有同一调用的后续同步预验和 prepare 可以读取该次短时授权。
    #[allow(clippy::too_many_arguments)]
    pub async fn review_and_validate_from_request_pinned(
        &self,
        ctx: &RunContext,
        surface: CallerSurface,
        agent_id: &str,
        permission: InvocationPermissionContext,
        call_id: &ToolCallId,
        request: &ToolRequest,
        pin: Option<CatalogPin>,
        evidence: &[crate::automation_review::AuthorizationEvidence],
        explicit_user_authorization: Option<&str>,
    ) -> Result<(), GatewayRefusal> {
        if !request.digest_matches_arguments() {
            return Err(GatewayRefusal::DigestMismatch {
                declared: request.args_digest.hex.clone(),
                computed: request.arguments.digest().hex,
            });
        }
        let unified = InvocationRequest::from_trusted_entry(
            ctx,
            surface,
            agent_id,
            permission,
            ToolTargetRef::ByTargetId {
                target_id: ToolTargetId::parse(&request.target),
            },
            pin,
            request.arguments.as_value().clone(),
            call_id.clone(),
        );
        let (prepared, entry, resources, verdict, _, _, _) =
            self.admission_checks_with_review(&unified, true)?;
        if verdict == PolicyVerdict::Allowed {
            return Ok(());
        }
        if !matches!(
            permission,
            InvocationPermissionContext::Automation {
                mode: lingxi_kernel::subagent::SessionPermissionMode::Ask
            } | InvocationPermissionContext::Bridge {
                role: lingxi_protocol::BridgeRole::Owner,
                mode: lingxi_kernel::subagent::SessionPermissionMode::Ask
            }
        ) {
            return self.admission_checks(&unified).map(|_| ());
        }
        let binding = automatic_review_binding(&unified, &prepared, entry, &resources);
        let reviewer = self
            .automatic_reviewer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or_else(|| automatic_review_refusal("自动审核服务尚未配置，操作未执行"))?;
        {
            let now = self.clock.now_unix_ms();
            let mut reviews = self
                .automatic_reviews
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            reviews
                .grants
                .retain(|grant| now <= grant.expires_at_unix_ms);
            if reviews
                .grants
                .iter()
                .any(|grant| grant.binding == binding && grant.consumed)
            {
                return Err(automatic_review_refusal(
                    "同一工具调用的自动审核授权已经消费",
                ));
            }
            if reviews.grants.len() + reviews.in_flight.len() >= self.live_prepared_cap
                || !reviews.in_flight.insert(binding.key.clone())
            {
                return Err(automatic_review_refusal(
                    "本次自动审核正在进行或审核容量已满",
                ));
            }
        }
        let _in_flight = InFlightAutomaticReview {
            state: &self.automatic_reviews,
            key: binding.key.clone(),
        };
        let listing = self
            .registry
            .describe(&prepared.target_id)
            .map_err(map_catalog_error)?;
        let input = PolicyAdjudicationInput {
            entry,
            target_id: prepared.target_id.to_string(),
            local_name: prepared.local_name.clone(),
            permission_kind: prepared.permission.kind,
            capability_base: prepared.permission.capability_base.clone(),
            args_digest_hex: prepared.args_digest.hex.clone(),
            principal_kind: ctx.principal.storage_kind(),
            principal_subject: ctx.principal.storage_subject(),
            session_id: ctx.session_id.to_string(),
            run_id: ctx.run_id.to_string(),
            agent_id: agent_id.into(),
            permission_context: permission,
        };
        let material = InvocationPolicyMaterial {
            origin: prepared.origin.clone(),
            declared_permission: listing.declared_permission,
            effective_arguments: prepared.effective.as_value().clone(),
            resources,
        };
        if !crate::automation_permission::requires_automatic_review(&input, &material) {
            return Err(automatic_review_refusal("该动作不能通过自动审核扩大权限"));
        }
        let review = crate::automation_review::AutoReviewRequest {
            ctx: ctx.clone(),
            tool_call_id: call_id.clone(),
            input,
            material,
            evidence: evidence.to_vec(),
            explicit_user_authorization: explicit_user_authorization.map(str::to_string),
        };
        match reviewer.review(&review).await {
            crate::automation_review::AutoReviewDecision::Allowed { .. } => {}
            crate::automation_review::AutoReviewDecision::Denied { code, reason } => {
                return Err(GatewayRefusal::PolicyDenied {
                    code,
                    layer: "automation_approval".into(),
                    message: reason,
                });
            }
        }
        let (current, entry, resources, _, _, _, _) =
            self.admission_checks_with_review(&unified, true)?;
        if automatic_review_binding(&unified, &current, entry, &resources) != binding {
            return Err(automatic_review_refusal(
                "自动审核期间工具、参数或资源范围已改变，原授权不可使用",
            ));
        }
        let now = self.clock.now_unix_ms();
        self.automatic_reviews
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .grants
            .push(AutomaticReviewGrant {
                binding,
                expires_at_unix_ms: now
                    .saturating_add(self.prepared_ttl_ms.min(DEFAULT_PREPARED_TTL_MS)),
                consumed: false,
            });
        // 通过同一同步入口再验一次，不以审核结论替换原有准备链。
        self.admission_checks(&unified).map(|_| ())
    }

    /// R05 RR1 F11: the whole-batch ADMISSION probe — the pure validation
    /// half of the gateway (digest gate + target identity/availability/
    /// generations + CURRENT-schema arguments + resource scopes + policy
    /// verdict) with NO prepared record and NO registration. The driver
    /// runs this for every request of a model turn before the first side
    /// effect; a refusal here means the whole batch stays at zero
    /// dispatch. The per-request execution path still runs the FULL
    /// `prepare` at its own moment (immediate authorization is never
    /// replaced by pre-validation).
    pub fn validate_from_request(
        &self,
        ctx: &RunContext,
        surface: CallerSurface,
        agent_id: &str,
        permission: InvocationPermissionContext,
        call_id: &ToolCallId,
        request: &ToolRequest,
    ) -> Result<(), GatewayRefusal> {
        self.validate_from_request_pinned(
            ctx, surface, agent_id, permission, call_id, request, None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn validate_from_request_pinned(
        &self,
        ctx: &RunContext,
        surface: CallerSurface,
        agent_id: &str,
        permission: InvocationPermissionContext,
        call_id: &ToolCallId,
        request: &ToolRequest,
        pin: Option<CatalogPin>,
    ) -> Result<(), GatewayRefusal> {
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
            pin,
            request.arguments.as_value().clone(),
            call_id.clone(),
        );
        self.admission_checks(&unified).map(|_| ())
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
        self.prepare_from_request_pinned(ctx, surface, agent_id, permission, call_id, request, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_from_request_pinned(
        &self,
        ctx: &RunContext,
        surface: CallerSurface,
        agent_id: &str,
        permission: InvocationPermissionContext,
        call_id: &ToolCallId,
        request: &ToolRequest,
        pin: Option<CatalogPin>,
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
            pin,
            request.arguments.as_value().clone(),
            call_id.clone(),
        );
        self.prepare(unified)
    }

    /// 统一驱动退出时释放它自己的准备状态；外来ctx/call不能烧毁他人的句柄。
    pub(crate) fn discard_prepared_for_call(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        handle: &PreparedInvocationHandle,
    ) {
        let mut prepared = self.prepared.lock().unwrap_or_else(|p| p.into_inner());
        let owns = prepared.get(handle.as_str()).is_some_and(|state| {
            let record = &state.record;
            record.principal_kind == ctx.principal.storage_kind()
                && record.principal_subject == ctx.principal.storage_subject()
                && record.session_id == ctx.session_id.as_str()
                && record.run_id == ctx.run_id.as_str()
                && record.attempt == ctx.attempt.as_str()
                && record.generation == ctx.generation
                && record.tool_call_id == *call
        });
        if owns {
            prepared.remove(handle.as_str());
        }
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
        // 4b) R04-T04 — the execution-time re-check of the RESOURCE
        //     scope (`临近执行重检` covers 撤销/代次/参数/资源): the
        //     bound extractor re-derives the REAL scopes from the CURRENT
        //     filesystem and they must reproduce the prepared record's
        //     scopes (canonical path + operation). A file moved, replaced
        //     by a link, or re-authorized differently between prepare and
        //     execute is refused with zero dispatch — the approval was
        //     for the OLD real target, never for whatever is there now.
        if let Some(extractor) = binding.resources.as_ref() {
            let input = ResourceExtractionInput {
                principal_kind: record.principal_kind,
                principal_subject: record.principal_subject.clone(),
                session_id: record.session_id.clone(),
                run_id: record.run_id.clone(),
            };
            match extractor(&input, &record.effective) {
                Ok(current) => {
                    let same = current.len() == record.resources.len()
                        && current
                            .iter()
                            .zip(record.resources.iter())
                            .all(|(now, bound)| now.op == bound.op && now.path == bound.path);
                    if !same {
                        let describe = |scopes: &[ResourceScope]| {
                            scopes
                                .iter()
                                .map(|scope| {
                                    format!("{}:{}", scope.op.wire_name(), scope.path.display())
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        return Err(GatewayRefusal::ResourceScopeChanged {
                            target_id: record.target_id.as_str().to_string(),
                            bound: describe(&record.resources),
                            current: describe(&current),
                        });
                    }
                }
                Err(message) => {
                    tracing::warn!(
                        target = %record.target_id,
                        "{message}"
                    );
                    return Err(GatewayRefusal::ResourceScopeDenied {
                        code: "resource_scope_refused".to_string(),
                        message,
                    });
                }
            }
        }
        if let Some(derive) = binding.review.as_ref() {
            let current = derive(
                &ReviewInput {
                    agent_id: &record.agent_id,
                    session_id: &record.session_id,
                    principal_kind: record.principal_kind,
                    principal_subject: &record.principal_subject,
                },
                &record.effective,
            )
            .map_err(|message| GatewayRefusal::ResourceScopeDenied {
                code: "review_scope_refused".into(),
                message,
            })?;
            if record.review_scope.as_ref() != Some(&current) {
                return Err(GatewayRefusal::ResourceScopeChanged {
                    target_id: record.target_id.as_str().into(),
                    bound: "confirmed pinned-memory items".into(),
                    current: "matched items changed; confirmation required again".into(),
                });
            }
        }
        if let Some(grant) = record.mcp_session_grant {
            let principal = record
                .settings_principal
                .clone()
                .ok_or_else(|| automatic_review_refusal("会话授权缺少原始前台身份"))?;
            let session_binding = crate::mcp_session_permissions::CapabilityBinding {
                principal,
                session_id: record.session_id.clone(),
                agent_id: record.agent_id.clone(),
                target_id: record.target_id.to_string(),
                target_generation: record.target_generation,
                capability: record.capability_base.clone(),
                review_scope: record
                    .review_scope
                    .clone()
                    .unwrap_or(serde_json::Value::Null),
            };
            if !self
                .session_capabilities
                .validate(grant, &session_binding, self.clock.as_ref())
                .await
            {
                return Err(GatewayRefusal::PolicyDenied {
                    code: "mcp_session_permission_revoked".into(),
                    layer: "mcp_session".into(),
                    message: "会话临时授权已失效，工具未派发".into(),
                });
            }
            // 实际会话读取发生await；返回后不能沿用等待前的目录与权限快照。
            let current_listing = self
                .registry
                .describe(&record.target_id)
                .map_err(map_catalog_error)?;
            if !current_listing.availability.callable()
                || current_listing.target_generation != record.target_generation
            {
                return Err(GatewayRefusal::TargetChanged {
                    target_id: record.target_id.to_string(),
                    detail: "会话授权复核期间MCP目录已改变".into(),
                });
            }
            let current_review = binding
                .review
                .as_ref()
                .ok_or_else(|| automatic_review_refusal("MCP管理绑定已失效"))?(
                &ReviewInput {
                    agent_id: &record.agent_id,
                    session_id: &record.session_id,
                    principal_kind: record.principal_kind,
                    principal_subject: &record.principal_subject,
                },
                &record.effective,
            )
            .map_err(|message| GatewayRefusal::ResourceScopeDenied {
                code: "review_scope_refused".into(),
                message,
            })?;
            if record.review_scope.as_ref() != Some(&current_review) {
                return Err(automatic_review_refusal("会话授权的MCP权限范围已改变"));
            }
        }
        // 5) Dispatch with the SERVER-BOUND arguments.
        let request = ToolRequest {
            target: record.target_id.as_str().to_string(),
            arguments: record.effective.clone(),
            args_digest: digest_of_hex(&record.args_digest_hex),
            args_summary: Some(lingxi_kernel::toolcatalog::summarize_arguments(
                &record.effective,
            )),
            // The gateway re-dispatch is internal: no provider correlation
            // id applies (the caller's `ToolRequest` keeps its own).
            provider_call_id: None,
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
        let lifetime =
            TOOL_LIFETIME
                .try_with(Clone::clone)
                .unwrap_or_else(|_| TrustedToolLifetime {
                    cancel: crate::cancel::CancelScope::background_root(&format!(
                        "host-tool:{}",
                        call_id.as_str()
                    )),
                    workspace: None,
                    deadline_unix_ms: record.expires_at_unix_ms,
                    budget_ms: record
                        .expires_at_unix_ms
                        .saturating_sub(record.created_at_unix_ms),
                });
        let runtime = crate::operations::EmbeddingCallRuntime {
            ownership: crate::operations::OperationOwnership {
                principal_kind: ctx.principal.storage_kind().to_string(),
                principal_subject: ctx.principal.storage_subject(),
                agent_id: Some(record.agent_id.clone()),
                session_id: Some(ctx.session_id.to_string()),
                run_id: Some(ctx.run_id.to_string()),
                attempt: Some(ctx.attempt.to_string()),
                parent_tool_call_id: Some(call_id.to_string()),
                cause_ref: format!("tool:{}", call_id.as_str()),
                deadline_unix_ms: lifetime.deadline_unix_ms.min(record.expires_at_unix_ms),
                budget_ms: lifetime.budget_ms.min(
                    record
                        .expires_at_unix_ms
                        .saturating_sub(record.created_at_unix_ms),
                ),
            },
            cancel: lifetime.cancel,
            request_key: format!("tool:{}:{}:{}", ctx.run_id, ctx.attempt, call_id),
            manual_retry_authorized: false,
        };
        let mut executed = KNOWLEDGE_RUNTIME
            .scope(
                runtime,
                REVIEW_SCOPE.scope(
                    record.review_scope.clone(),
                    EXECUTION_RESOURCES.scope(
                        record.resources.clone(),
                        SETTINGS_CONTEXT.scope(
                            TrustedSettingsContext {
                                principal: ctx.principal.clone(),
                                settings_principal: record.settings_principal.clone(),
                                session_id: record.session_id.clone(),
                                run_id: record.run_id.clone(),
                                attempt: record.attempt.clone(),
                                call_id: call_id.to_string(),
                                agent_id: record.agent_id.clone(),
                                permission: record.permission,
                            },
                            self.dispatch_executor(ctx, call_id, &request, &binding.executor),
                        ),
                    ),
                ),
            )
            .await?;
        // 6) R04-A15 — the DELIVERY-REGISTRATION audit. The executor ran,
        //    so this is NOT a zero-dispatch refusal: a SUCCESS whose
        //    `file://` resource reference does not verify against the
        //    filesystem RIGHT NOW (missing / not a regular file) is
        //    converted to an explicit FAILED that states the dispatch
        //    happened, that side effects may exist, and that the artifact
        //    is NOT registered as a valid deliverable. "Request sent"
        //    never masquerades as "file produced" — and a vanished or
        //    bogus artifact never masquerades as a delivery either.
        if let ToolOutcome::Success { result } = &mut executed.outcome {
            if let Err(rejection) = audit_success_artifacts(result) {
                tracing::warn!(
                    target = %record.target_id,
                    code = rejection.code(),
                    "delivered artifact failed the gateway registration audit: {}",
                    rejection.message()
                );
                executed.outcome = ToolOutcome::Failed {
                    error: ProtocolError::new(
                        ErrorCode::Internal,
                        format!(
                            "delivered artifact failed verification ({}): {}; the execution WAS \
                             dispatched and side effects may exist, but this file is NOT \
                             registered as a valid deliverable",
                            rejection.code(),
                            rejection.message()
                        ),
                        false,
                    )
                    .with_details(serde_json::Map::from_iter([(
                        "code".to_string(),
                        serde_json::json!("gateway_artifact_verification_failed"),
                    )])),
                };
            }
        }
        Ok(executed)
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

/// R04-A15: audits every LOCAL-FILE resource reference a SUCCESS carries
/// (the universal facts the gateway can derive on its own — existence and
/// regular-file shape, judged at registration time). Scope/contract
/// conditions belong to the boundary that owns them (worker grants, the
/// file tools' resource extractor) and are NOT re-judged here; remote
/// URIs are verified by their own type and never forced through a
/// local-file audit.
fn audit_success_artifacts(
    result: &lingxi_kernel::ports::ToolSuccess,
) -> Result<(), crate::artifactverify::ArtifactRejection> {
    for reference in &result.resource_refs {
        let Some(uri) = reference.uri.as_deref() else {
            continue;
        };
        let Some(path_text) = crate::artifactverify::local_path_of_uri(uri) else {
            continue;
        };
        crate::artifactverify::audit_delivered_file(std::path::Path::new(path_text))?;
    }
    Ok(())
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

pub(crate) fn map_catalog_error(
    err: lingxi_kernel::toolcatalog::ToolCatalogError,
) -> GatewayRefusal {
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

/// 封面批准调用的完整输入面：固定 CoverRun 的持久层、作业与真实调用身份、
/// 准备句柄及凭证复核入口一次成组，调用方按名绑定，不靠参数位序。
pub(crate) struct CoverAuthorizationCall<'a> {
    pub db: &'a lingxi_adapters::storage::RunDatabase,
    pub job_id: &'a str,
    pub ctx: &'a RunContext,
    pub call: &'a ToolCallId,
    pub prepared: &'a PreparedInvocation,
    pub auth: &'a crate::auth::AuthService,
    pub management: &'a crate::management::ManagementState,
}

impl ToolInvocationGateway {
    /// 仅固定 CoverRun 消费用户已经明确发起的动作；不向普通模型或HTTP公开批准入口。
    /// 批准绑定当前nonce、真实调用、供应商/资源范围和付费前版本，不修改通用策略。
    pub(crate) async fn authorize_cover_prepared(
        &self,
        approval: CoverAuthorizationCall<'_>,
    ) -> Result<(), String> {
        let CoverAuthorizationCall {
            db,
            job_id,
            ctx,
            call,
            prepared,
            auth,
            management,
        } = approval;
        use lingxi_adapters::storage::beautify::{self, CoverPhase};
        let job = beautify::get(db, job_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("封面作业不存在")?;
        let mut principal: crate::Principal =
            serde_json::from_value(job.principal.clone()).map_err(|_| "封面原始认证事实损坏")?;
        principal.web_session_id = job.web_session_id.clone();
        auth.validate_principal(&principal)
            .map_err(|_| "封面发起凭证已撤销")?;
        if principal.credential_kind == crate::auth::CredentialKind::WebSession
            && !management
                .web_session_current(&principal, self.clock.now_unix_ms(), true)
                .map_err(|_| "封面网页会话无法复核")?
        {
            return Err("封面网页会话已撤销".into());
        }
        let actor = crate::sessions::kernel_principal_of(&principal);
        if (!principal.is_local_owner()
            && principal.kind != crate::auth::PrincipalKind::Device
            && principal.credential_kind != crate::auth::CredentialKind::WebSession)
            || !principal.has_scope("chat")
            || !principal.has_scope("resources.write")
            || principal.principal_id != job.principal_id
            || actor != ctx.principal
            || job.run_id.as_deref() != Some(ctx.run_id.as_str())
            || job.session_id != ctx.session_id.as_str()
            || job.admitted_write.is_none()
        {
            return Err("封面批准缺少真实用户动作与当前作用范围".into());
        }
        let record = {
            let records = self.prepared.lock().map_err(|_| "工具准备状态不可读")?;
            records
                .get(prepared.handle.as_str())
                .ok_or("封面准备句柄已失效")?
                .record
                .clone()
        };
        let (args, route) = match (job.phase, record.target_id.as_str()) {
            (CoverPhase::MediaPreparing, "tool:plugin:media:generate-image") => {
                (&job.state["mediaArgs"], Some(&job.state["mediaRoute"]))
            }
            (CoverPhase::Applying, "tool:plugin:beautify:create-cover") => {
                (&job.state["applyArgs"], None)
            }
            _ => return Err("封面阶段与工具动作不匹配".into()),
        };
        if record.principal_kind != ctx.principal.storage_kind()
            || record.principal_subject != ctx.principal.storage_subject()
            || record.session_id != ctx.session_id.as_str()
            || record.run_id != ctx.run_id.as_str()
            || record.attempt != ctx.attempt.as_str()
            || record.generation != ctx.generation
            || record.agent_id != job.agent_id
            || record.tool_call_id != *call
            || record.target_id != prepared.target_id
            || record.args_digest_hex != prepared.args_digest_hex
            || record.catalog_generation != prepared.catalog_generation
            || record.target_generation != prepared.target_generation
            || record.expires_at_unix_ms <= self.clock.now_unix_ms()
            || record.effective.as_value() != args
            || route.is_some_and(|route| {
                record.review_scope.as_ref().map(|s| &s["mediaRoute"]) != Some(route)
            })
        {
            return Err("封面批准的准备、参数、供应商或真实调用绑定已改变".into());
        }
        let resources = record
            .resources
            .iter()
            .map(|scope| {
                serde_json::json!({"op": scope.op.wire_name(), "path": scope.path.display().to_string()})
            })
            .collect::<Vec<_>>();
        let binding = serde_json::json!({"handle":prepared.handle.as_str(),"ctx":{"runId":ctx.run_id,"sessionId":ctx.session_id,"attempt":ctx.attempt,"generation":ctx.generation},"principalId":job.principal_id,"target":record.target_id.as_str(),"argsDigest":record.args_digest_hex,"catalogGeneration":record.catalog_generation,"targetGeneration":record.target_generation,"resources":resources,"reviewScope":record.review_scope,"frozenWrite":job.admitted_write});
        beautify::record_authorization(
            db,
            beautify::CoverAuthorization {
                job_id: &job.id,
                run: ctx.run_id.as_str(),
                call: call.as_str(),
                target: record.target_id.as_str(),
                digest: &record.args_digest_hex,
                binding,
                now: self.clock.now_unix_ms(),
            },
        )
        .await
        .map_err(|e| e.to_string())
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
    fn prepared_cleanup_is_bound_to_real_call_and_releases_abandoned_state() {
        let registry = Arc::new(ToolRegistry::new());
        registry
            .register(
                manifest("probe", PermissionKind::Read, Availability::Available),
                &SchemaBudget::default(),
            )
            .unwrap();
        let gw = gateway(&registry, Arc::new(FailClosedPolicy));
        let ctx = ctx_of("cleanup-run");
        let call = ToolCallId::new("cleanup-call");
        let prepared = gw
            .prepare(InvocationRequest::from_trusted_entry(
                &ctx,
                CallerSurface::UserRun,
                "agent",
                test_permission(),
                ToolTargetRef::ByName {
                    name: "probe".into(),
                },
                None,
                json!({"path":"/x"}),
                call.clone(),
            ))
            .unwrap();
        gw.discard_prepared_for_call(&ctx_of("foreign-run"), &call, &prepared.handle);
        assert!(gw
            .prepared
            .lock()
            .unwrap()
            .contains_key(prepared.handle.as_str()));
        gw.discard_prepared_for_call(&ctx, &ToolCallId::new("foreign-call"), &prepared.handle);
        assert!(gw
            .prepared
            .lock()
            .unwrap()
            .contains_key(prepared.handle.as_str()));
        gw.discard_prepared_for_call(&ctx, &call, &prepared.handle);
        assert!(!gw
            .prepared
            .lock()
            .unwrap()
            .contains_key(prepared.handle.as_str()));
        let error =
            futures_block_on(gw.execute_prepared(&ctx, &call, &prepared.handle)).unwrap_err();
        assert_eq!(error.code(), "gateway_prepared_handle_unknown");
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
