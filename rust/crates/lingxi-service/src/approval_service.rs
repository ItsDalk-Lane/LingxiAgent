//! The production approval service (R04-T03): permission-mode mapping,
//! approval records and revocation — "approve the SPECIFIC action, never
//! an unrestricted follow-on right".
//!
//! This ONE type is both faces the R04 chain needs:
//!
//! 1. **The tool-policy face** ([`ToolPolicyPort`]): maps the incumbent's
//!    session permission vocabulary (`operate` / `ask` / `read_only`,
//!    `core/session-permission-mode.ts`) onto Rust-side verdicts. This is
//!    where the R03-T06-O1 gap (R04-SUP-01) closes: an ask-tier subagent
//!    child run (`access` omitted, or an explicit `write` request that
//!    INHERITS the ask parent under the incumbent's
//!    `resolvePermissionMode`) cannot have its write-class calls collapsed
//!    into an operate pass — the incumbent answers them through the
//!    executor-layer `approvalPolicy: "deny_on_prompt"` +
//!    `allowHumanApproval: false` (fixed for every subagent dispatch,
//!    `lib/tools/subagent-tool.ts`), which surfaces as the structured
//!    `TOOL_APPROVAL_UNAVAILABLE` refusal. The Rust mapping reproduces
//!    exactly that: **Denied { TOOL_APPROVAL_UNAVAILABLE }** — never an
//!    auto-approve, never an unbounded wait, never a bare dispatch.
//!
//! 2. **The approval-wait face** ([`ApprovalGate`]): the driver parks a
//!    NeedsApproval call in `waiting_approval` and asks THIS service. The
//!    service mints an approval record bound to principal, session, run,
//!    attempt, generation, tool call id, target, the canonical argument
//!    digest, a deadline and a use budget; an AUTHENTICATED external
//!    answerer resolves it ([`ApprovalService::answer`]); a timeout or a
//!    dropped wait settles it deterministically; a LATE answer after
//!    settle/cancel/restart NEVER resurrects the call.
//!
//! # Policy vs gate priority (the R04-T02 O05 closure)
//!
//! Formal adjudication (pinned by tests here and in
//! `tests/r04_t03_approval_service.rs`): **the policy face is the single
//! source of the approval REQUIREMENT.** A call the configured policy
//! adjudicated `Allowed` advances to `authorized` WITHOUT consulting the
//! approval gate again (the master prompt §4.2 "不重复弹审批" — asking a
//! human twice for a call a configured policy service already allowed is
//! a duplicate prompt); a call adjudicated `NeedsApproval` waits on the
//! gate; when no approval surface is wired the driver closes it as the
//! never-dispatched `TOOL_APPROVAL_UNAVAILABLE` refusal (T02 semantics,
//! unchanged). The R03 minimal gate remains consulted ONLY on the
//! no-gateway legacy wiring, byte-for-byte as before.
//!
//! # Approval records are single-use, bound and revocation-aware
//!
//! - A record binds the FULL invocation identity: principal × session ×
//!   run × attempt × generation × tool call id × target × canonical
//!   args digest. Another call id, another digest, another session —
//!   nothing about a second request can spend the first one's approval.
//! - "Approve file A, submit file B" (R04-A05) cannot execute B: the
//!   approval is bound to A's digest. Session-scoped PRE-AUTHORIZATION
//!   follows the incumbent's real contract
//!   (`preAuthorizedInvocationCapabilities`): the whole key is the exact
//!   invocation (target + capability + canonical digest) — a grant never
//!   widens past it, and different parameters simply do not match.
//! - Answering twice (double click) is deterministic: the second answer
//!   observes the first settlement and changes nothing.
//! - A wait dropped by the cancellation tree marks its record aborted;
//!   the late answer is refused (the incumbent's ConfirmStore
//!   `abortBySession` + late-decision-returns-false).
//! - The dispatch-time re-adjudication ("真正派发前重新裁决可用性、身份
//!   和授权") lives in the gateway's `execute_prepared` (registry
//!   re-verification, identity re-verification, single-use spend) — the
//!   approval service never re-derives it; disabling/uninstalling the
//!   target during a wait therefore still refuses the OLD approved
//!   request with an explicit target-invalid error (R04-A06).
//! - **Revocation vs started**: a revocation (disable/uninstall/permission
//!   revision) that lands BEFORE the external dispatch always wins (zero
//!   dispatch); once the executor has been entered the external side
//!   effect is beyond recall and NOTHING here claims to undo it — the
//!   honest boundary the taskbook fixes ("已经发生的外部副作用不声称可撤销").
//!
//! # Persistence rule (deliberate, honest)
//!
//! Approval records and pre-authorizations are IN-MEMORY for this
//! process. A restart drops them all: an old approval id answers
//! `Refused` (unknown on this service), an old pre-authorization no
//! longer matches, and a replayed prepared handle is unknown to the
//! fresh gateway registry. Old approvals are therefore INVALIDATED (not
//! continued) across restarts — the explicit rule the taskbook requires
//! ("不能默认延续"); durable approval state is R06 session-state scope,
//! not silently faked here.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::subagent::{SessionPermissionMode, ToolAccessTier};
use lingxi_kernel::RunContext;
use lingxi_protocol::ToolCallId;

use crate::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest};
use crate::inject::ServiceClock;
use crate::toolgateway::{
    InvocationPermissionContext, PolicyAdjudicationInput, PolicyVerdict, ToolPolicyPort,
};

/// The incumbent's structured refusal code for "approval is required but
/// the approval surface cannot serve this caller"
/// (`lib/tools/session-permission-wrapper.ts` `toolApprovalUnavailable`).
pub const TOOL_APPROVAL_UNAVAILABLE: &str = "TOOL_APPROVAL_UNAVAILABLE";

/// The incumbent's read-only refusal code (`classifySessionPermission`
/// → `blockedByReadOnly`).
pub const ACTION_BLOCKED_BY_READ_ONLY: &str = "ACTION_BLOCKED_BY_READ_ONLY";

/// Default bounded wait of one approval request — the incumbent's
/// ConfirmStore DEFAULT_TIMEOUT (5 minutes, `lib/confirm-store.ts:11`).
pub const DEFAULT_APPROVAL_TIMEOUT_MS: u64 = 5 * 60 * 1000;

/// Bounded retention of SETTLED approval records: late answers and replay
/// presentations keep their deterministic diagnostic without unbounded
/// memory in a long-lived process.
pub const SETTLED_RING_CAP: usize = 4096;

/// Bounded cap of simultaneously PENDING approvals: a full registry is a
/// loud refusal (the caller's wait returns `Aborted`), never an
/// unbounded backlog.
pub const DEFAULT_PENDING_CAP: usize = 1024;

/// Bounded cap of live session-scoped pre-authorizations.
pub const DEFAULT_PREAUTHORIZED_CAP: usize = 1024;

/// The approval-policy vocabulary the incumbent persists
/// (`SESSION_APPROVAL_POLICIES`: interactive / deny_on_prompt / never).
/// Derived here from the session mode + the subagent's fixed
/// unattended posture (the `resolveSessionApprovalPolicy` mapping):
/// - operate → Never (the mode itself authorizes side effects);
/// - ask (a human is reachable) → Interactive;
/// - ask inside a subagent (`allowHumanApproval: false`, fixed) →
///   DenyOnPrompt — a prompt that can never be answered becomes the
///   structured TOOL_APPROVAL_UNAVAILABLE refusal, never a wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    Interactive,
    DenyOnPrompt,
    Never,
}

impl ApprovalPolicy {
    pub fn wire_name(self) -> &'static str {
        match self {
            ApprovalPolicy::Interactive => "interactive",
            ApprovalPolicy::DenyOnPrompt => "deny_on_prompt",
            ApprovalPolicy::Never => "never",
        }
    }
}

/// Resolves the approval policy of one invocation context — the
/// `resolveSessionApprovalPolicy` mapping on the Rust vocabulary (the
/// incumbent's `auto` tier is not part of the frozen Rust session-mode
/// set; the reachable user tiers map directly).
pub fn resolve_approval_policy(context: &InvocationPermissionContext) -> ApprovalPolicy {
    match context {
        InvocationPermissionContext::UserSession { mode } => match mode {
            SessionPermissionMode::Operate => ApprovalPolicy::Never,
            SessionPermissionMode::Ask => ApprovalPolicy::Interactive,
            SessionPermissionMode::ReadOnly => ApprovalPolicy::Never,
        },
        InvocationPermissionContext::Subagent { .. } => {
            // allowHumanApproval: false is FIXED for every subagent
            // dispatch (the incumbent passes it unconditionally); a
            // subagent never reaches a human, so its ask inheritance is
            // deny_on_prompt — the SUP-01 semantics.
            ApprovalPolicy::DenyOnPrompt
        }
    }
}

/// The exact invocation a pre-authorization covers — the whole key, per
/// the incumbent contract ("the capability string is the whole key, so a
/// grant never widens past the exact invocation it was issued for"):
/// target id + capability base + canonical argument digest, scoped to
/// one principal and one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationGrantKey {
    pub target_id: String,
    pub capability_base: String,
    pub args_digest_hex: String,
}

/// One session-scoped pre-authorization granted by an explicit user
/// decision. Single-use by default (the user approved ONE action); a
/// larger budget is an explicit parameter, never a default.
#[derive(Debug, Clone)]
struct PreAuthorization {
    principal_kind: &'static str,
    principal_subject: String,
    session_id: String,
    key: InvocationGrantKey,
    remaining_uses: u32,
    granted_at_unix_ms: u64,
    expires_at_unix_ms: u64,
}

/// The answer of an authenticated external approver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Approve,
    Reject { reason: String },
}

/// The deterministic outcome of an answer attempt. Late and duplicate
/// answers NEVER change a settled record and NEVER resurrect anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerOutcome {
    /// The answer settled the pending record (its wait resolves now).
    Settled(Answer),
    /// The record was already settled by an earlier answer (double
    /// click): the FIRST decision stands, verbatim.
    AlreadySettled(Answer),
    /// The record is no longer answerable (aborted by a dropped wait /
    /// cancellation, expired, or unknown — e.g. minted before a
    /// restart). The call it belonged to was never executed and cannot
    /// be resurrected.
    Refused { reason: String },
}

impl AnswerOutcome {
    pub fn settled(&self) -> bool {
        matches!(self, AnswerOutcome::Settled(_))
    }
}

/// The caller-visible view of one pending approval (the minimal
/// authenticated interaction surface — a real UI/bridge face wraps this
/// in R06+; it never carries the server-side binding inputs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingView {
    pub approval_id: String,
    pub target: String,
    /// The run the parked call belongs to (the approver's audit anchor).
    pub run_id: String,
    pub args_summary: Option<String>,
    pub waiting_since_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

/// State of one approval record.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordState {
    Pending,
    Approved,
    Rejected {
        reason: String,
    },
    /// The wait was dropped (cancellation tree / driver exit) — a late
    /// answer is refused, the call was never executed.
    Aborted,
    Expired,
}

/// One approval record: everything an approval binds. Never handed to
/// the model; the pending VIEW is the only external projection.
#[derive(Debug, Clone)]
struct ApprovalRecord {
    approval_id: String,
    principal_kind: &'static str,
    principal_subject: String,
    session_id: String,
    run_id: String,
    attempt: String,
    generation: u64,
    tool_call_id: ToolCallId,
    target: String,
    args_digest_hex: String,
    args_summary: Option<String>,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    state: RecordState,
    /// Wakes the parked wait when an answer/abort settles the record.
    notify: Arc<tokio::sync::Notify>,
}

/// Errors of the pre-authorization surface (loud, never silent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantError {
    /// The pre-authorization registry is at its cap.
    RegistryFull { cap: usize },
    /// A pre-authorization with a zero use budget or zero TTL is not a
    /// grant — refusing it loudly beats recording a no-op.
    Degenerate { detail: String },
}

impl std::fmt::Display for GrantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantError::RegistryFull { cap } => {
                write!(f, "pre-authorization registry is at its cap ({cap})")
            }
            GrantError::Degenerate { detail } => write!(f, "degenerate grant refused: {detail}"),
        }
    }
}

impl std::error::Error for GrantError {}

#[derive(Default)]
struct ApprovalState {
    records: HashMap<String, ApprovalRecord>,
    settled_ring: std::collections::VecDeque<String>,
    preauthorized: Vec<PreAuthorization>,
}

/// The production approval service. One instance serves as the tool
/// policy face AND the approval-wait face of the wired gateway chain
/// (inject the same `Arc` into `ToolInvocationGateway::new` as the
/// policy and into `ServiceDeps::approval_gate` as the gate).
pub struct ApprovalService {
    state: std::sync::Mutex<ApprovalState>,
    clock: Arc<dyn ServiceClock>,
    approval_timeout_ms: u64,
    pending_cap: usize,
    preauthorized_cap: usize,
}

impl std::fmt::Debug for ApprovalService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalService")
            .field("approval_timeout_ms", &self.approval_timeout_ms)
            .field("pending_cap", &self.pending_cap)
            .field("preauthorized_cap", &self.preauthorized_cap)
            .finish_non_exhaustive()
    }
}

impl ApprovalService {
    /// Full construction with the incumbent's default bounds.
    pub fn new(clock: Arc<dyn ServiceClock>) -> Self {
        Self::with_bounds(
            clock,
            DEFAULT_APPROVAL_TIMEOUT_MS,
            DEFAULT_PENDING_CAP,
            DEFAULT_PREAUTHORIZED_CAP,
        )
    }

    /// Full construction with explicit bounds (tests tighten the timeout
    /// to keep the timeout legs deterministic and fast).
    pub fn with_bounds(
        clock: Arc<dyn ServiceClock>,
        approval_timeout_ms: u64,
        pending_cap: usize,
        preauthorized_cap: usize,
    ) -> Self {
        assert!(
            approval_timeout_ms >= 1,
            "approval_timeout_ms must be >= 1 (0 would disable the bounded wait)"
        );
        assert!(
            pending_cap >= 1 && preauthorized_cap >= 1,
            "approval caps must be >= 1 (0 would silently disable the approval surface)"
        );
        Self {
            state: std::sync::Mutex::new(ApprovalState::default()),
            clock,
            approval_timeout_ms,
            pending_cap,
            preauthorized_cap,
        }
    }

    // ── the tool-policy face (R04-SUP-01 closure) ─────────────────────────
    //
    // Adjudication matrix (the `classifySessionPermission` +
    // `resolveSessionApprovalPolicy` mapping on the Rust vocabulary):
    //
    // | context                          | Read class | Execute class |
    // |----------------------------------|------------|---------------|
    // | user `operate`                   | Allowed    | Allowed (policy Never) |
    // | user `ask`                       | Allowed    | NeedsApproval (Interactive) |
    // | user `read_only`                 | Allowed    | Denied ACTION_BLOCKED_BY_READ_ONLY |
    // | subagent read-only tier          | Allowed    | Allowed here; the KERNEL authorization step denies write/unknown local names (the single authorization point, R04-SUP-02) |
    // | subagent operate tier, parent operate | Allowed | Allowed (normal legitimate write) |
    // | subagent operate tier, parent ask | Allowed   | **Denied TOOL_APPROVAL_UNAVAILABLE** (SUP-01) |
    // | subagent operate tier, parent read_only | Allowed | Denied ACTION_BLOCKED_BY_READ_ONLY (unreachable via resolve_subagent_access; kept as defense in the safe direction) |
    //
    // The read-only-tier Execute row deliberately defers to the kernel
    // judgment point: the R03-T06 authorization step is THE single
    // authorization decision point (R04-SUP-02) — duplicating its
    // vocabulary here could only drift, and a policy `Allowed` still
    // lands on the kernel's refusal before anything is dispatched.
    pub fn adjudicate(&self, input: &PolicyAdjudicationInput) -> PolicyVerdict {
        use lingxi_kernel::toolcatalog::PermissionKind;
        match input.permission_context {
            InvocationPermissionContext::UserSession { mode } => match mode {
                SessionPermissionMode::Operate => PolicyVerdict::Allowed,
                SessionPermissionMode::Ask => match input.permission_kind {
                    PermissionKind::Read => PolicyVerdict::Allowed,
                    _ => PolicyVerdict::NeedsApproval {
                        reason: format!(
                            "session permission mode is ask: {} ({} class) requires a human \
                             approval before dispatch",
                            input.target_id,
                            input.permission_kind.wire_name()
                        ),
                    },
                },
                SessionPermissionMode::ReadOnly => match input.permission_kind {
                    PermissionKind::Read => PolicyVerdict::Allowed,
                    _ => PolicyVerdict::Denied {
                        code: ACTION_BLOCKED_BY_READ_ONLY.to_string(),
                        layer: "session".to_string(),
                        message: format!(
                            "{} is blocked: this session is in read-only mode. Switch the \
                             session permission mode out of read_only to use this tool.",
                            input.local_name
                        ),
                    },
                },
            },
            InvocationPermissionContext::Subagent { tier, parent_mode } => {
                match (tier, parent_mode) {
                    (ToolAccessTier::ReadOnly, _)
                    | (ToolAccessTier::Operate, SessionPermissionMode::Operate) => {
                        PolicyVerdict::Allowed
                    }
                    (ToolAccessTier::Operate, SessionPermissionMode::Ask) => {
                        match input.permission_kind {
                            // Read-class calls stay allowed under the ask
                            // inheritance (research/review work keeps
                            // working; the kernel read-only list is not in
                            // play on the operate tier).
                            PermissionKind::Read => PolicyVerdict::Allowed,
                            // SUP-01: the write-class call of an ask-tier
                            // subagent is the structured refusal — never an
                            // operate collapse, never an auto-approve,
                            // never an unbounded wait (deny_on_prompt with
                            // no human reachable).
                            _ => PolicyVerdict::Denied {
                                code: TOOL_APPROVAL_UNAVAILABLE.to_string(),
                                layer: "approval_policy".to_string(),
                                message: format!(
                                    "tool approval unavailable: {} ({} class) needs approval but \
                                 this subagent runs unattended (approvalPolicy deny_on_prompt, \
                                 allowHumanApproval false — inherited from the parent session's \
                                 ask mode); the action was not run",
                                    input.target_id,
                                    input.permission_kind.wire_name()
                                ),
                            },
                        }
                    }
                    // Unreachable through resolve_subagent_access (a write
                    // request under a read-only parent is refused at
                    // dispatch; an omitted access inherits read-only). If a
                    // future wiring ever produces it, deny in the safe
                    // direction instead of allowing a write under a
                    // read-only parent.
                    (ToolAccessTier::Operate, SessionPermissionMode::ReadOnly) => {
                        PolicyVerdict::Denied {
                            code: ACTION_BLOCKED_BY_READ_ONLY.to_string(),
                            layer: "subagent_access".to_string(),
                            message: format!(
                                "{} is blocked: the parent session is read-only; a subagent's \
                             permission can never exceed its parent session.",
                                input.local_name
                            ),
                        }
                    }
                }
            }
        }
    }

    // ── the authenticated minimal answer surface ───────────────────────────

    /// The pending approvals of one session, visible to its owner only
    /// (the authenticated approver surface — R06+ wraps this in the real
    /// UI/bridge; nothing here leaks another session's records).
    pub fn pending_of(&self, principal: &RunContext, session_id: &str) -> Vec<PendingView> {
        let state = self.lock_state();
        let mut views: Vec<PendingView> = state
            .records
            .values()
            .filter(|record| {
                record.state == RecordState::Pending
                    && record.session_id == session_id
                    && record.principal_kind == principal.principal.storage_kind()
                    && record.principal_subject == principal.principal.storage_subject()
            })
            .map(|record| PendingView {
                approval_id: record.approval_id.clone(),
                target: record.target.clone(),
                run_id: record.run_id.clone(),
                args_summary: record.args_summary.clone(),
                waiting_since_unix_ms: record.created_at_unix_ms,
                expires_at_unix_ms: record.expires_at_unix_ms,
            })
            .collect();
        views.sort_by(|a, b| a.approval_id.cmp(&b.approval_id));
        views
    }

    /// Answers one pending approval. Ownership is enforced (the record's
    /// principal and session must match the answerer's), the first
    /// answer wins, and every later answer — double click, late after
    /// abort/expire, unknown id (e.g. minted before a restart) — gets a
    /// deterministic outcome that changes nothing.
    pub fn answer(
        &self,
        principal: &RunContext,
        session_id: &str,
        approval_id: &str,
        answer: Answer,
    ) -> AnswerOutcome {
        let mut state = self.lock_state();
        let Some(record) = state.records.get_mut(approval_id) else {
            return AnswerOutcome::Refused {
                reason: format!(
                    "approval {approval_id} is unknown on this approval service (never minted, \
                     or minted before this process started — approvals do not survive restarts)"
                ),
            };
        };
        if record.principal_kind != principal.principal.storage_kind()
            || record.principal_subject != principal.principal.storage_subject()
            || record.session_id != session_id
        {
            // An authenticated surface never even learns whether the id
            // exists for someone else — the refusal is uniform.
            return AnswerOutcome::Refused {
                reason: format!(
                    "approval {approval_id} does not belong to this principal and session"
                ),
            };
        }
        match record.state.clone() {
            RecordState::Pending => {
                record.state = match &answer {
                    Answer::Approve => RecordState::Approved,
                    Answer::Reject { reason } => RecordState::Rejected {
                        reason: reason.clone(),
                    },
                };
                record.notify.notify_waiters();
                AnswerOutcome::Settled(answer)
            }
            RecordState::Approved => AnswerOutcome::AlreadySettled(Answer::Approve),
            RecordState::Rejected { reason } => {
                AnswerOutcome::AlreadySettled(Answer::Reject { reason })
            }
            RecordState::Aborted => AnswerOutcome::Refused {
                reason: format!(
                    "approval {approval_id} was aborted (the waiting call was cancelled or its \
                     driver exited); a late approval cannot resurrect the call"
                ),
            },
            RecordState::Expired => AnswerOutcome::Refused {
                reason: format!(
                    "approval {approval_id} expired before it was answered; the call was not \
                     executed"
                ),
            },
        }
    }

    // ── the session-scoped pre-authorization surface ──────────────────────

    /// Grants a session-scoped pre-authorization for ONE exact
    /// invocation (target + capability + canonical digest) — the
    /// incumbent's explicit-user-decision surface
    /// (`preAuthorizedInvocationCapabilities`). Single-use by default;
    /// a larger budget is an explicit parameter. Bounded in time: a
    /// pre-authorization that never expired would be an unrestricted
    /// follow-on right, which is exactly what an approval must not
    /// become.
    pub fn grant_preauthorization(
        &self,
        principal: &RunContext,
        session_id: &str,
        key: InvocationGrantKey,
        max_uses: u32,
        ttl_ms: u64,
    ) -> Result<(), GrantError> {
        if max_uses == 0 {
            return Err(GrantError::Degenerate {
                detail: "max_uses must be >= 1 (a zero-use grant is a no-op, not a grant)"
                    .to_string(),
            });
        }
        if ttl_ms == 0 {
            return Err(GrantError::Degenerate {
                detail: "ttl_ms must be >= 1 (a zero-TTL grant would either never match or \
                         never expire)"
                    .to_string(),
            });
        }
        let now = self.clock.now_unix_ms();
        let mut state = self.lock_state();
        if state.preauthorized.len() >= self.preauthorized_cap {
            state
                .preauthorized
                .retain(|grant| now <= grant.expires_at_unix_ms && grant.remaining_uses > 0);
            if state.preauthorized.len() >= self.preauthorized_cap {
                return Err(GrantError::RegistryFull {
                    cap: self.preauthorized_cap,
                });
            }
        }
        state.preauthorized.push(PreAuthorization {
            principal_kind: principal.principal.storage_kind(),
            principal_subject: principal.principal.storage_subject(),
            session_id: session_id.to_string(),
            key,
            remaining_uses: max_uses,
            granted_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(ttl_ms),
        });
        Ok(())
    }

    /// Atomically spends one use of a matching pre-authorization: the
    /// compare-and-decrement under the state lock is what makes one
    /// approval impossible for two concurrent requests to each consume.
    fn spend_preauthorization(&self, principal: &RunContext, req: &ApprovalRequest) -> bool {
        let now = self.clock.now_unix_ms();
        let mut state = self.lock_state();
        let principal_kind = principal.principal.storage_kind();
        let principal_subject = principal.principal.storage_subject();
        let session_id = principal.session_id.to_string();
        state
            .preauthorized
            .iter_mut()
            // The WHOLE key must match (the incumbent contract): target,
            // capability-bound resource set (the canonical digest IS the
            // invocation's capability key at this stage — ResourceRef
            // binding extends it in R04-T04), principal and session
            // scope, still-live use budget and deadline.
            .find(|grant| {
                grant.remaining_uses > 0
                    && now <= grant.expires_at_unix_ms
                    && grant.principal_kind == principal_kind
                    && grant.principal_subject == principal_subject
                    && grant.session_id == session_id
                    && grant.key.target_id == req.target
                    && grant.key.args_digest_hex == req.args_digest
            })
            .map(|grant| {
                grant.remaining_uses -= 1;
                tracing::info!(
                    target = %req.target,
                    granted_at_unix_ms = grant.granted_at_unix_ms,
                    remaining_uses = grant.remaining_uses,
                    "pre-authorization spent for the exact invocation (target + canonical                      digest + principal + session)"
                );
                true
            })
            .unwrap_or(false)
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, ApprovalState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers a settled/approval id in the bounded ring (bounded
    /// memory; the late-answer diagnostics stay available; the record
    /// itself stays in the map until the ring evicts its id).
    fn retire_record(state: &mut ApprovalState, approval_id: &str) {
        if state.settled_ring.len() >= SETTLED_RING_CAP {
            if let Some(evicted) = state.settled_ring.pop_front() {
                state.records.remove(&evicted);
            }
        }
        state.settled_ring.push_back(approval_id.to_string());
    }
}

impl ToolPolicyPort for ApprovalService {
    fn adjudicate(&self, input: &PolicyAdjudicationInput) -> PolicyVerdict {
        ApprovalService::adjudicate(self, input)
    }
}

/// RAII guard: when the parked wait future is DROPPED (the run's
/// cancellation tree fired, the driver exited — the ApprovalGate
/// contract), the record is marked aborted so a late answer is refused
/// and can never resurrect the call.
struct AbortOnDrop<'a> {
    service: &'a ApprovalService,
    approval_id: String,
    armed: bool,
}

impl AbortOnDrop<'_> {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for AbortOnDrop<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut state = self.service.lock_state();
        if let Some(record) = state.records.get_mut(&self.approval_id) {
            if record.state == RecordState::Pending {
                record.state = RecordState::Aborted;
                record.notify.notify_waiters();
            }
        }
        ApprovalService::retire_record(&mut state, &self.approval_id);
    }
}

impl ApprovalGate for ApprovalService {
    /// The approval-wait face. Zero pre-execution during the wait: this
    /// future resolves a DECISION only — the executor is never touched
    /// from here; the driver dispatches (or refuses) after the journal's
    /// `authorized`/`started` writes exactly as for any other call.
    fn request<'a>(
        &'a self,
        ctx: &'a RunContext,
        req: &'a ApprovalRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ApprovalDecision> + Send + 'a>> {
        Box::pin(async move {
            // 1) A matching live pre-authorization answers immediately
            //    (one use spent atomically). The incumbent's
            //    pre-authorization contract: the exact invocation only.
            if self.spend_preauthorization(ctx, req) {
                return ApprovalDecision::Approved;
            }
            // 2) Mint the bound approval record and park.
            let now = self.clock.now_unix_ms();
            let nonce = match crate::auth::hex_random_public(16) {
                Ok(hex) => format!("approval:{hex}"),
                // The entropy source failing must never degrade into a
                // guessable approval id: refuse the wait loudly (zero
                // executions), exactly like the gateway's handle mint.
                Err(err) => {
                    tracing::error!(
                        target = %req.target,
                        error = %err,
                        "approval id nonce could not be minted; refusing the approval wait \
                         (zero executions; a guessable id would be a forgery surface)"
                    );
                    return ApprovalDecision::Aborted;
                }
            };
            let notify = Arc::new(tokio::sync::Notify::new());
            let record = ApprovalRecord {
                approval_id: nonce.clone(),
                principal_kind: ctx.principal.storage_kind(),
                principal_subject: ctx.principal.storage_subject(),
                session_id: ctx.session_id.to_string(),
                run_id: ctx.run_id.to_string(),
                attempt: ctx.attempt.to_string(),
                generation: ctx.generation,
                tool_call_id: req.tool_call_id.clone(),
                target: req.target.clone(),
                args_digest_hex: req.args_digest.clone(),
                args_summary: req.args_summary.clone(),
                created_at_unix_ms: now,
                expires_at_unix_ms: now.saturating_add(self.approval_timeout_ms),
                state: RecordState::Pending,
                notify: Arc::clone(&notify),
            };
            {
                let mut state = self.lock_state();
                // Bounded pending registry: reclaim dead records first —
                // a record whose wait is still parked is Pending and is
                // NEVER reclaimed here (its wait loop reads it below);
                // settled records past their deadline are gone.
                if state.records.len() >= self.pending_cap {
                    state.records.retain(|_, record| {
                        record.state == RecordState::Pending || now <= record.expires_at_unix_ms
                    });
                }
                if state.records.len() >= self.pending_cap {
                    tracing::error!(
                        target = %req.target,
                        cap = self.pending_cap,
                        "pending-approval registry is full: refusing the approval wait (zero \
                         executions)"
                    );
                    return ApprovalDecision::Aborted;
                }
                // The audit fact of WHAT one approval binds — read from
                // the RECORD itself (the binding IS the record): the
                // full invocation identity (principal × session × run ×
                // attempt × generation × call × target × canonical
                // digest). The approver approves exactly this, and any
                // other call id/digest/session cannot spend it.
                tracing::info!(
                    approval_id = %record.approval_id,
                    target = %record.target,
                    args_digest = %record.args_digest_hex,
                    run_id = %record.run_id,
                    attempt = %record.attempt,
                    generation = record.generation,
                    tool_call_id = %record.tool_call_id,
                    session_id = %record.session_id,
                    principal = %record.principal_subject,
                    "approval record minted: the approval binds this exact invocation"
                );
                state.records.insert(nonce.clone(), record);
            }
            let guard = AbortOnDrop {
                service: self,
                approval_id: nonce.clone(),
                armed: true,
            };
            // 3) Park until: an answer settles the record, or the
            //    bounded deadline expires (the incumbent's ConfirmStore
            //    timeout — a definite rejection, zero executions), or
            //    the future is dropped (the guard aborts the record; a
            //    late answer observes Aborted and is refused).
            let deadline = tokio::time::Instant::now()
                + std::time::Duration::from_millis(self.approval_timeout_ms);
            tokio::select! {
                _ = notify.notified() => {}
                _ = tokio::time::sleep_until(deadline) => {}
            }
            let state_after = {
                let mut state = self.lock_state();
                let Some(record) = state.records.get_mut(&nonce) else {
                    // Evicted mid-wait only by cap-pressure reclaim of a
                    // non-parked record — impossible for ours while
                    // parked; treat as aborted (safe direction, zero
                    // executions).
                    guard.disarm();
                    return ApprovalDecision::Aborted;
                };
                if record.state == RecordState::Pending {
                    record.state = RecordState::Expired;
                }
                let state_after = record.state.clone();
                ApprovalService::retire_record(&mut state, &nonce);
                state_after
            };
            guard.disarm();
            match state_after {
                RecordState::Approved => ApprovalDecision::Approved,
                RecordState::Rejected { reason } => ApprovalDecision::Rejected { reason },
                // Aborted (the wait was dropped — cancellation) resolves
                // here only when the abort raced the wake; either way
                // ZERO executions.
                RecordState::Aborted | RecordState::Expired => ApprovalDecision::Aborted,
                RecordState::Pending => unreachable!("the loop break settled the record"),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::ManualClock;
    use crate::toolgateway::InvocationEntry;
    use lingxi_kernel::toolcatalog::PermissionKind;
    use lingxi_protocol::AttemptId;

    fn input(
        context: InvocationPermissionContext,
        kind: PermissionKind,
    ) -> PolicyAdjudicationInput {
        PolicyAdjudicationInput {
            entry: InvocationEntry::Direct,
            target_id: "tool:first-party:write".to_string(),
            local_name: "write".to_string(),
            permission_kind: kind,
            capability_base: "write.capability".to_string(),
            args_digest_hex: "d".to_string(),
            principal_kind: "local_user",
            principal_subject: "user_local".to_string(),
            session_id: "s".to_string(),
            run_id: "r".to_string(),
            agent_id: "a".to_string(),
            permission_context: context,
        }
    }

    fn service() -> ApprovalService {
        ApprovalService::new(Arc::new(ManualClock::new(1_700_000_000_000)))
    }

    #[test]
    fn user_mode_matrix_matches_the_incumbent() {
        // operate: both classes allowed (policy Never).
        assert_eq!(
            service().adjudicate(&input(
                InvocationPermissionContext::UserSession {
                    mode: SessionPermissionMode::Operate
                },
                PermissionKind::Execute
            )),
            PolicyVerdict::Allowed
        );
        // ask: read allowed, execute-class needs approval (Interactive).
        assert_eq!(
            service().adjudicate(&input(
                InvocationPermissionContext::UserSession {
                    mode: SessionPermissionMode::Ask
                },
                PermissionKind::Read
            )),
            PolicyVerdict::Allowed
        );
        match service().adjudicate(&input(
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Ask,
            },
            PermissionKind::Execute,
        )) {
            PolicyVerdict::NeedsApproval { reason } => {
                assert!(reason.contains("ask"), "{reason}")
            }
            other => panic!("ask write must need approval, got {other:?}"),
        }
        // read_only: read allowed, execute denied with the incumbent code.
        match service().adjudicate(&input(
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::ReadOnly,
            },
            PermissionKind::ArgumentAwareFile,
        )) {
            PolicyVerdict::Denied { code, layer, .. } => {
                assert_eq!(code, ACTION_BLOCKED_BY_READ_ONLY);
                assert_eq!(layer, "session");
            }
            other => panic!("read_only write must be denied, got {other:?}"),
        }
    }

    #[test]
    fn sup01_ask_subagent_write_is_the_structured_refusal() {
        let ask_sub = InvocationPermissionContext::Subagent {
            tier: ToolAccessTier::Operate,
            parent_mode: SessionPermissionMode::Ask,
        };
        // Read class stays allowed (research keeps working).
        assert_eq!(
            service().adjudicate(&input(ask_sub, PermissionKind::Read)),
            PolicyVerdict::Allowed
        );
        // Every execute-class kind takes the structured refusal.
        for kind in [
            PermissionKind::Execute,
            PermissionKind::ArgumentAwareFile,
            PermissionKind::ArgumentAwareSessionFolders,
        ] {
            match service().adjudicate(&input(ask_sub, kind)) {
                PolicyVerdict::Denied {
                    code,
                    layer,
                    message,
                } => {
                    assert_eq!(code, TOOL_APPROVAL_UNAVAILABLE, "{kind:?}");
                    assert_eq!(layer, "approval_policy");
                    assert!(message.contains("deny_on_prompt"), "{message}");
                    assert!(message.contains("not run"), "{message}");
                }
                other => panic!("ask-subagent write must be refused, got {other:?}"),
            }
        }
        // Operate parent: the SAME tier + kind is allowed (normal
        // legitimate write — no blanket shutdown).
        assert_eq!(
            service().adjudicate(&input(
                InvocationPermissionContext::Subagent {
                    tier: ToolAccessTier::Operate,
                    parent_mode: SessionPermissionMode::Operate
                },
                PermissionKind::Execute
            )),
            PolicyVerdict::Allowed
        );
        // Read-only tier defers to the kernel authorization point (the
        // policy face does not re-litigate the kernel vocabulary).
        for parent in [
            SessionPermissionMode::Operate,
            SessionPermissionMode::Ask,
            SessionPermissionMode::ReadOnly,
        ] {
            assert_eq!(
                service().adjudicate(&input(
                    InvocationPermissionContext::Subagent {
                        tier: ToolAccessTier::ReadOnly,
                        parent_mode: parent
                    },
                    PermissionKind::Execute
                )),
                PolicyVerdict::Allowed,
                "the kernel authorization step owns the read-only-tier denial ({parent:?})"
            );
        }
    }

    #[test]
    fn approval_policy_resolution_follows_the_incumbent() {
        assert_eq!(
            resolve_approval_policy(&InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate
            })
            .wire_name(),
            "never"
        );
        assert_eq!(
            resolve_approval_policy(&InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Ask
            })
            .wire_name(),
            "interactive"
        );
        // EVERY subagent context is unattended (allowHumanApproval false
        // is fixed) — deny_on_prompt regardless of the inherited mode.
        for parent in [
            SessionPermissionMode::Operate,
            SessionPermissionMode::Ask,
            SessionPermissionMode::ReadOnly,
        ] {
            for tier in [ToolAccessTier::ReadOnly, ToolAccessTier::Operate] {
                assert_eq!(
                    resolve_approval_policy(&InvocationPermissionContext::Subagent {
                        tier,
                        parent_mode: parent
                    })
                    .wire_name(),
                    "deny_on_prompt"
                );
            }
        }
    }

    fn run_ctx(session: &str, run: &str) -> RunContext {
        RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new(session.to_string()),
            run_id: lingxi_protocol::RunId::new(run.to_string()),
            attempt: AttemptId::new(format!("{run}#a1")),
            generation: 1,
        }
    }

    #[test]
    fn preauthorization_key_discrimination_and_use_budget() {
        let service = service();
        let ctx = run_ctx("s1", "r1");
        let key_of = |target: &str, digest: &str| InvocationGrantKey {
            target_id: target.to_string(),
            capability_base: "write.capability".to_string(),
            args_digest_hex: digest.to_string(),
        };
        service
            .grant_preauthorization(
                &ctx,
                "s1",
                key_of("tool:first-party:write", "digest-A"),
                1,
                60_000,
            )
            .expect("grant");
        // The exact invocation spends exactly one use…
        let req_a = crate::approval::ApprovalRequest {
            tool_call_id: ToolCallId::new("call-1".to_string()),
            target: "tool:first-party:write".to_string(),
            args_digest: "digest-A".to_string(),
            args_summary: None,
        };
        assert!(service.spend_preauthorization(&ctx, &req_a));
        // …and the SAME invocation cannot spend it again (single use).
        assert!(!service.spend_preauthorization(&ctx, &req_a));
        // A different digest (file B) never matched in the first place.
        service
            .grant_preauthorization(
                &ctx,
                "s1",
                key_of("tool:first-party:write", "digest-A2"),
                1,
                60_000,
            )
            .expect("grant 2");
        let req_b = crate::approval::ApprovalRequest {
            tool_call_id: ToolCallId::new("call-2".to_string()),
            target: "tool:first-party:write".to_string(),
            args_digest: "digest-B".to_string(),
            args_summary: None,
        };
        assert!(!service.spend_preauthorization(&ctx, &req_b));
        // A different session/principal cannot spend another session's
        // grant.
        let ctx_other = RunContext {
            session_id: lingxi_protocol::SessionId::new("s2".to_string()),
            ..run_ctx("s1", "r2")
        };
        service
            .grant_preauthorization(
                &ctx,
                "s1",
                key_of("tool:first-party:write", "digest-A3"),
                1,
                60_000,
            )
            .expect("grant 3");
        let req_c = crate::approval::ApprovalRequest {
            tool_call_id: ToolCallId::new("call-3".to_string()),
            target: "tool:first-party:write".to_string(),
            args_digest: "digest-A3".to_string(),
            args_summary: None,
        };
        assert!(!service.spend_preauthorization(&ctx_other, &req_c));
        assert!(service.spend_preauthorization(&ctx, &req_c));
    }

    #[test]
    fn degenerate_grants_are_refused_loudly() {
        let service = service();
        let ctx = run_ctx("s1", "r1");
        let key = InvocationGrantKey {
            target_id: "t".to_string(),
            capability_base: "c".to_string(),
            args_digest_hex: "d".to_string(),
        };
        assert!(matches!(
            service.grant_preauthorization(&ctx, "s1", key.clone(), 0, 60_000),
            Err(GrantError::Degenerate { .. })
        ));
        assert!(matches!(
            service.grant_preauthorization(&ctx, "s1", key, 1, 0),
            Err(GrantError::Degenerate { .. })
        ));
    }

    #[test]
    fn answer_surface_is_ownership_checked_and_deterministic() {
        // Answer/late/duplicate semantics are exercised end-to-end in
        // the integration suite (real waiting runs); this unit pins the
        // unknown-id rule: a restart-fresh service knows no old ids.
        let service = service();
        let ctx = run_ctx("s1", "r1");
        match service.answer(&ctx, "s1", "approval:deadbeef", Answer::Approve) {
            AnswerOutcome::Refused { reason } => {
                assert!(reason.contains("unknown"), "{reason}");
                assert!(reason.contains("restarts"), "{reason}");
            }
            other => panic!("unknown id must be refused, got {other:?}"),
        }
        // Ownership: a foreign session's id is uniformly refused.
        let foreign = run_ctx("s2", "r9");
        match service.answer(&foreign, "s2", "approval:deadbeef", Answer::Approve) {
            AnswerOutcome::Refused { .. } => {}
            other => panic!("{other:?}"),
        }
    }
}
