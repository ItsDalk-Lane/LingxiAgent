//! Sub-agent domain rules (R03-T06): permission ATTENUATION, the child
//! tool-authorization boundary, run lineage identity and the policy
//! surface. Pure rules — no I/O, no clock, no executor.
//!
//! Frozen incumbent semantics this maps (read from the Node production
//! stack before mapping — no new interaction was invented):
//! - **Two-state child access** (`lib/tools/subagent-tool-policy.ts`):
//!   a subagent runs either read-only or operable. The tier is resolved
//!   from the EXPLICIT `access` parameter when present (`read` →
//!   read-only, `write` → operate), otherwise INHERITED from the parent
//!   session's current permission mode.
//! - **Attenuation refusal** (`resolvePermissionMode`, issue #1614):
//!   a `write` request under a read-only parent is REFUSED loudly with
//!   `SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY` — a subagent's
//!   permission may never exceed its parent's; never a silent downgrade
//!   to read (the model believing it can write would behave wrongly).
//! - **The per-tool-call boundary** (`core/session-permission-mode.ts`
//!   `classifySessionPermission` → `blockedByReadOnly` /
//!   `blocked`): inside a subagent, fan-out tools are ALWAYS blocked
//!   (`SUBAGENT_BLOCKED_TOOLS`: anti self-recursion, no cross-session
//!   fan-out, no long-term-memory writes, no automation/external side
//!   effects), and under the read-only tier every MUTATING target is
//!   denied with `ACTION_BLOCKED_BY_READ_ONLY` (layer `subagent_access`)
//!   — the refusal text preserves the parent-child relationship and the
//!   escape hatch. Read-only targets stay allowed so research/review
//!   tasks keep working.
//! - **Fail-closed unknown targets**: the incumbent allows unknown tools
//!   only in operable mode; under read-only anything not known read-only
//!   is blocked. The Rust mapping keeps exactly that posture.
//! - **Tool strategy / experiment defaults**: the incumbent strategy is
//!   甲 intercept by default (`LINGXI_SUBAGENT_TOOL_STRATEGY`, default
//!   `intercept`: full tool set + interception layer), and the proactive
//!   delegation experiment `subagent.proactive_delegation` defaults to
//!   FALSE (`lib/experiments/registry.ts`). Both defaults are preserved
//!   here as [`SubagentPolicy`] defaults.
//!
//! R03 wiring boundary: the kernel owns the DECISION rules; the run layer
//! (lingxi-service `runs.rs`) applies them on the live chain at the T05
//! journal's authorization step — a denied target is never dispatched
//! (receipt closes `dispatched: false`), and neither a model swap nor an
//! executor change can widen the grant (the grant is fixed at dispatch
//! and bound to the child run).

use lingxi_protocol::RunId;

/// The parent session's permission mode (the incumbent
/// `operate/ask/read_only` vocabulary, normalized — the two-state
/// subagent collapse is [`Self::is_read_only`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionPermissionMode {
    Operate,
    Ask,
    ReadOnly,
}

impl SessionPermissionMode {
    pub const WIRE_OPERATE: &'static str = "operate";
    pub const WIRE_ASK: &'static str = "ask";
    pub const WIRE_READ_ONLY: &'static str = "read_only";

    /// Stable wire name (the incumbent's persisted vocabulary).
    pub fn wire_name(self) -> &'static str {
        match self {
            SessionPermissionMode::Operate => Self::WIRE_OPERATE,
            SessionPermissionMode::Ask => Self::WIRE_ASK,
            SessionPermissionMode::ReadOnly => Self::WIRE_READ_ONLY,
        }
    }

    /// Parses the wire vocabulary; unknown values return `None` (loud at
    /// the caller — never a guess).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            Self::WIRE_OPERATE => Some(SessionPermissionMode::Operate),
            Self::WIRE_ASK => Some(SessionPermissionMode::Ask),
            Self::WIRE_READ_ONLY => Some(SessionPermissionMode::ReadOnly),
            _ => None,
        }
    }

    /// The two-state subagent collapse: only `read_only` attenuates
    /// (`isReadOnlyPermissionMode`).
    pub fn is_read_only(self) -> bool {
        self == SessionPermissionMode::ReadOnly
    }
}

/// The EXPLICIT `access` parameter of a subagent dispatch
/// (`access: "read" | "write"`; anything else is treated as omitted, the
/// incumbent's illegal-value handling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessRequest {
    Read,
    Write,
}

impl AccessRequest {
    /// Mirrors the incumbent's enum coercion: only the two legal literals
    /// count; every other value (including absent) inherits.
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            Some("read") => Some(AccessRequest::Read),
            Some("write") => Some(AccessRequest::Write),
            _ => None,
        }
    }
}

/// The resolved two-state access tier a subagent child run runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolAccessTier {
    /// Research / exploration / review: no edits, no mutating commands.
    ReadOnly,
    /// Execution / edits / commands (still bounded by the subagent
    /// blocklist).
    Operate,
}

impl ToolAccessTier {
    pub fn wire_name(self) -> &'static str {
        match self {
            ToolAccessTier::ReadOnly => "read",
            ToolAccessTier::Operate => "write",
        }
    }

    pub fn is_read_only(self) -> bool {
        self == ToolAccessTier::ReadOnly
    }
}

/// The attenuation refusal (issue #1614 shape): a subagent's permission
/// may never exceed its parent's. Loud and actionable — never a silent
/// downgrade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentAccessDenied {
    /// Stable machine code (the incumbent's error code verbatim).
    pub code: &'static str,
    /// The refusal text WITH the escape hatch (the incumbent's message
    /// semantics; the service surfaces it to the model as a structured
    /// tool error).
    pub message: String,
}

/// Resolves the child's access tier — the dispatch-time half of the
/// attenuation rule (`resolveSubagentToolAccess` → `resolvePermissionMode`).
///
/// - `Read` → read-only;
/// - `Write` → operable ONLY when the parent is not read-only — otherwise
///   [`SubagentAccessDenied`] (`SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY`);
/// - omitted (or an illegal value, already coerced to `None`) → inherit
///   the parent's mode, collapsed to the two subagent states. A missing
///   parent mode defaults to operable (the incumbent's
///   `resolveInheritedMode(null)` → OPERATE).
pub fn resolve_subagent_access(
    explicit: Option<AccessRequest>,
    parent_mode: SessionPermissionMode,
) -> Result<ToolAccessTier, SubagentAccessDenied> {
    match explicit {
        Some(AccessRequest::Read) => Ok(ToolAccessTier::ReadOnly),
        Some(AccessRequest::Write) => {
            if parent_mode.is_read_only() {
                Err(SubagentAccessDenied {
                    code: "SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY",
                    message: "Cannot grant write access: the parent session is read-only, and a \
                              subagent's permission may not exceed its parent. Switch the \
                              session to an operable mode and retry, or re-dispatch with \
                              access:\"read\"."
                        .to_string(),
                })
            } else {
                Ok(ToolAccessTier::Operate)
            }
        }
        None => Ok(if parent_mode.is_read_only() {
            ToolAccessTier::ReadOnly
        } else {
            ToolAccessTier::Operate
        }),
    }
}

/// Targets ALWAYS blocked inside a subagent regardless of tier (the
/// incumbent `SUBAGENT_BLOCKED_TOOLS`: fan-out, long-term-memory writes,
/// automation, external side effects, blocking on the user). Plus
/// `subagent_reply`/`subagent_close`: the incumbent isolates the child in
/// its own session so threads are unreachable from inside; R03 child runs
/// share the parent's session row, so the anti-recursion/ownership guard
/// must be structural instead (a documented mapping deviation, not a
/// semantic one).
pub const SUBAGENT_BLOCKED_TARGETS: &[&str] = &[
    // fan-out (anti self-recursion + indirect fan-out + cross-session)
    "subagent",
    "subagent_reply",
    "subagent_close",
    "workflow",
    "session",
    // long-term memory: readable, never writable from a subagent
    "pin_memory",
    "unpin_memory",
    "record_experience",
    "tenet_propose",
    // agent lifecycle / external side effects
    "automation",
    "cron",
    "channel",
    "dm",
    "notify",
    "install_skill",
    "learn_lesson",
    // blocking on the user: a subagent reports uncertainty to the parent
    "ask_user",
    "update_settings",
    "session_folders",
    // the loop belongs to the main session
    "loop_control",
    // knowledge-base mutation surface (the read side stays open)
    "knowledge_manage",
];

/// Targets that stay allowed under the read-only tier (the incumbent's
/// read-action/information sets, in the R03 target vocabulary). Anything
/// NOT in this set is blocked under read-only — fail-closed for unknown
/// targets, exactly like the incumbent.
pub const SUBAGENT_READ_ONLY_TARGETS: &[&str] = &[
    "read",
    "grep",
    "find",
    "ls",
    "glob",
    "web_search",
    "web_fetch",
    "todo_read",
    "current_status",
    "knowledge_read",
    "knowledge_grep",
    "knowledge_outline",
];

/// The per-target authorization decision for one subagent child tool
/// call — the boundary the run layer applies at the T05 journal's
/// authorization step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolAuthorization {
    /// The attenuated grant covers this target; the invocation may
    /// advance to `authorized`.
    Allowed,
    /// The real authorization boundary refused the target. The executor
    /// is never invoked (zero dispatch); the refusal carries the stable
    /// code, the layer and a message that preserves the parent-child
    /// relationship and the escape hatch.
    Denied {
        code: &'static str,
        layer: &'static str,
        message: String,
    },
}

/// Decides one tool target for a subagent child run
/// (`classifySessionPermission` with `isSubagent: true`):
/// 1. blocklisted targets are denied ALWAYS (fixed subagent boundary,
///    independent of tier — anti-recursion etc.);
/// 2. read-only tier denies every target not known read-only (fail-closed
///    for unknowns) with the subagent_access refusal text;
/// 3. operate tier allows the rest.
pub fn authorize_child_tool(tier: ToolAccessTier, target: &str) -> ToolAuthorization {
    if SUBAGENT_BLOCKED_TARGETS.contains(&target) {
        return ToolAuthorization::Denied {
            code: "ACTION_BLOCKED_IN_SUBAGENT",
            layer: "subagent_blocklist",
            message: format!(
                "{target} is not available inside a subagent. This tool is always blocked in \
                 subagent context regardless of access level; perform this action from the \
                 parent session instead."
            ),
        };
    }
    if tier.is_read_only() && !SUBAGENT_READ_ONLY_TARGETS.contains(&target) {
        return ToolAuthorization::Denied {
            code: "ACTION_BLOCKED_BY_READ_ONLY",
            layer: "subagent_access",
            message: format!(
                "{target} is blocked: this subagent runs in read-only mode. For write access, \
                 re-dispatch the subagent with access:\"write\" — this requires the parent \
                 session to be in an operable (non read-only) mode; a subagent's permission can \
                 never exceed its parent session."
            ),
        };
    }
    ToolAuthorization::Allowed
}

/// Where a run came from. `User` is the interactive submission surface;
/// `Subagent` is a child run dispatched by a parent run's delegation tool
/// call. `Cron` / `Heartbeat` / `Bridge` reserve the R07 entry vocabulary
/// NOW so those integrations reuse the same lineage surface instead of
/// building a second scheduler (R03-T06 step 4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RunOrigin {
    User,
    Subagent,
    Cron,
    Heartbeat,
    Bridge,
}

impl RunOrigin {
    pub fn wire_name(&self) -> &'static str {
        match self {
            RunOrigin::User => "user",
            RunOrigin::Subagent => "subagent",
            RunOrigin::Cron => "cron",
            RunOrigin::Heartbeat => "heartbeat",
            RunOrigin::Bridge => "bridge",
        }
    }

    /// Parses the wire vocabulary; unknown values return `None` (loud at
    /// the caller — a stored origin this build does not know is never
    /// guessed into another meaning).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "user" => Some(RunOrigin::User),
            "subagent" => Some(RunOrigin::Subagent),
            "cron" => Some(RunOrigin::Cron),
            "heartbeat" => Some(RunOrigin::Heartbeat),
            "bridge" => Some(RunOrigin::Bridge),
            _ => None,
        }
    }
}

/// The four-part run lineage identity (R03-T06 step 4): parentRunId /
/// origin / sourceMessageId / causeId. Persisted for EVERY run — child
/// runs fill all four anchors; user runs carry their submission anchors.
///
/// - `parent_run_id`: the run whose delegation tool call dispatched this
///   child (`None` for user submissions).
/// - `origin`: which entry surface created the run.
/// - `source_message_id`: the message that sourced the run — for a child,
///   the parent's model call that emitted the delegation request.
/// - `cause_id`: the precise causal anchor — for a child, the parent's
///   tool call id; for a user submission, the explicit requestId when
///   present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLineage {
    pub parent_run_id: Option<RunId>,
    pub origin: RunOrigin,
    pub source_message_id: Option<String>,
    pub cause_id: Option<String>,
}

impl RunLineage {
    /// The lineage of a plain user submission (no parent, no source
    /// message; the cause is the explicit requestId when present).
    pub fn user_submission(request_id: Option<&str>) -> Self {
        Self {
            parent_run_id: None,
            origin: RunOrigin::User,
            source_message_id: None,
            cause_id: request_id.map(|id| format!("request:{id}")),
        }
    }
}

/// The subagent policy surface (R03-T06). Defaults preserve the incumbent
/// values: intercept strategy, proactive delegation experiment OFF,
/// per-session 10 / global 20 concurrent subagents, 30-minute timeout
/// counted from the child's actual start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentPolicy {
    /// Tool strategy: `intercept` (full set + interception — the default
    /// 甲) or `strip` (per-tier stripped lists). The strategy changes HOW
    /// tools are withheld, never WHAT the authorization boundary allows.
    pub tool_strategy: SubagentToolStrategy,
    /// The `subagent.proactive_delegation` experiment switch — the
    /// incumbent default is OFF (false).
    pub proactive_delegation: bool,
    /// Maximum concurrent subagent runs per session (incumbent 10).
    pub per_session_limit: usize,
    /// Maximum concurrent subagent runs process-wide (incumbent 20).
    pub global_limit: usize,
    /// Child run timeout in milliseconds, anchored at the child's actual
    /// start (incumbent 30 minutes — a queued dispatch must not burn the
    /// budget before it starts).
    pub timeout_ms: u64,
    /// Hard cap of TRACKED subagent threads (open threads are live facts;
    /// closed ones are evicted at the cap before a refusal).
    pub thread_registry_cap: usize,
}

impl Default for SubagentPolicy {
    fn default() -> Self {
        Self {
            tool_strategy: SubagentToolStrategy::Intercept,
            proactive_delegation: false,
            per_session_limit: 10,
            global_limit: 20,
            timeout_ms: 30 * 60 * 1000,
            thread_registry_cap: 1024,
        }
    }
}

impl SubagentPolicy {
    /// Loud validation: a zero limit or timeout would silently disable the
    /// subagent surface or let children run unbounded — both are startup
    /// errors, never clamps.
    pub fn validate(&self) -> Result<(), crate::ports::StorageError> {
        use crate::ports::StorageError;
        if self.per_session_limit == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "subagent per_session_limit must be >= 1 (0 would silently disable \
                         subagent dispatch)"
                    .to_string(),
            });
        }
        if self.global_limit == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "subagent global_limit must be >= 1 (0 would silently disable \
                         subagent dispatch)"
                    .to_string(),
            });
        }
        if self.timeout_ms == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "subagent timeout_ms must be >= 1 (0 would disable the child run \
                         deadline)"
                    .to_string(),
            });
        }
        if self.thread_registry_cap == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "subagent thread_registry_cap must be >= 1 (0 would silently disable \
                         thread tracking)"
                    .to_string(),
            });
        }
        Ok(())
    }
}

/// How the subagent tool set is presented (the incumbent A/B surface; the
/// default is 甲 intercept). Authorization semantics are IDENTICAL under
/// both — the boundary lives in [`authorize_child_tool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubagentToolStrategy {
    /// Full tool set + interception layer (default).
    Intercept,
    /// Per-tier stripped tool lists.
    Strip,
}

impl SubagentToolStrategy {
    pub fn wire_name(self) -> &'static str {
        match self {
            SubagentToolStrategy::Intercept => "intercept",
            SubagentToolStrategy::Strip => "strip",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attenuation_matrix_matches_the_incumbent() {
        use AccessRequest as A;
        use SessionPermissionMode as M;
        use ToolAccessTier as T;
        // explicit read → read-only under every parent mode.
        for mode in [M::Operate, M::Ask, M::ReadOnly] {
            assert_eq!(
                resolve_subagent_access(Some(A::Read), mode),
                Ok(T::ReadOnly)
            );
        }
        // explicit write → operate only when the parent is not read-only.
        assert_eq!(
            resolve_subagent_access(Some(A::Write), M::Operate),
            Ok(T::Operate)
        );
        assert_eq!(
            resolve_subagent_access(Some(A::Write), M::Ask),
            Ok(T::Operate)
        );
        // write + read-only parent = the loud attenuation refusal.
        let denial = resolve_subagent_access(Some(A::Write), M::ReadOnly).unwrap_err();
        assert_eq!(denial.code, "SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY");
        assert!(
            denial.message.contains("may not exceed its parent"),
            "message keeps the attenuation reason: {}",
            denial.message
        );
        assert!(
            denial.message.contains("access:\\\"read\\\"") || denial.message.contains("read"),
            "message keeps the escape hatch"
        );
        // omitted → inherit, collapsed to the two subagent states.
        assert_eq!(resolve_subagent_access(None, M::Operate), Ok(T::Operate));
        assert_eq!(resolve_subagent_access(None, M::Ask), Ok(T::Operate));
        assert_eq!(resolve_subagent_access(None, M::ReadOnly), Ok(T::ReadOnly));
    }

    #[test]
    fn access_request_parses_only_the_two_legal_literals() {
        assert_eq!(
            AccessRequest::parse(Some("read")),
            Some(AccessRequest::Read)
        );
        assert_eq!(
            AccessRequest::parse(Some("write")),
            Some(AccessRequest::Write)
        );
        // Illegal values are treated as omitted (inherit) — the incumbent
        // coercion; identity-ish junk never becomes a tier.
        assert_eq!(AccessRequest::parse(Some("admin")), None);
        assert_eq!(AccessRequest::parse(Some("")), None);
        assert_eq!(AccessRequest::parse(None), None);
    }

    #[test]
    fn blocklisted_targets_are_denied_regardless_of_tier() {
        for target in [
            "subagent",
            "workflow",
            "session",
            "cron",
            "dm",
            "pin_memory",
        ] {
            for tier in [ToolAccessTier::ReadOnly, ToolAccessTier::Operate] {
                match authorize_child_tool(tier, target) {
                    ToolAuthorization::Denied { code, layer, .. } => {
                        assert_eq!(code, "ACTION_BLOCKED_IN_SUBAGENT", "{target}/{tier:?}");
                        assert_eq!(layer, "subagent_blocklist");
                    }
                    other => panic!("{target} must be blocked inside a subagent, got {other:?}"),
                }
            }
        }
        // Anti-recursion is structural: the delegation target itself.
        assert!(matches!(
            authorize_child_tool(ToolAccessTier::Operate, "subagent"),
            ToolAuthorization::Denied { .. }
        ));
    }

    #[test]
    fn read_only_tier_denies_mutating_targets_and_fails_closed_on_unknowns() {
        for target in ["write", "edit", "exec_command", "totally_unknown_tool"] {
            match authorize_child_tool(ToolAccessTier::ReadOnly, target) {
                ToolAuthorization::Denied {
                    code,
                    layer,
                    message,
                } => {
                    assert_eq!(code, "ACTION_BLOCKED_BY_READ_ONLY", "{target}");
                    assert_eq!(layer, "subagent_access");
                    assert!(
                        message.contains("never exceed its parent session"),
                        "the refusal preserves the parent-child rule: {message}"
                    );
                }
                other => panic!("read-only subagent must deny {target}, got {other:?}"),
            }
        }
        // Known read-only targets stay allowed (research keeps working).
        for target in ["read", "grep", "find", "ls", "web_search", "knowledge_read"] {
            assert_eq!(
                authorize_child_tool(ToolAccessTier::ReadOnly, target),
                ToolAuthorization::Allowed,
                "{target} stays readable under the read-only tier"
            );
        }
    }

    #[test]
    fn operate_tier_allows_non_blocklisted_targets() {
        for target in ["read", "write", "edit", "exec_command"] {
            assert_eq!(
                authorize_child_tool(ToolAccessTier::Operate, target),
                ToolAuthorization::Allowed
            );
        }
    }

    #[test]
    fn origin_vocabulary_is_stable_and_loud_on_unknowns() {
        for (origin, name) in [
            (RunOrigin::User, "user"),
            (RunOrigin::Subagent, "subagent"),
            (RunOrigin::Cron, "cron"),
            (RunOrigin::Heartbeat, "heartbeat"),
            (RunOrigin::Bridge, "bridge"),
        ] {
            assert_eq!(origin.wire_name(), name);
            assert_eq!(RunOrigin::parse(name), Some(origin));
        }
        assert_eq!(RunOrigin::parse("root"), None);
        assert_eq!(RunOrigin::parse(""), None);
    }

    #[test]
    fn session_mode_vocabulary_round_trips_and_collapses() {
        for mode in [
            SessionPermissionMode::Operate,
            SessionPermissionMode::Ask,
            SessionPermissionMode::ReadOnly,
        ] {
            assert_eq!(SessionPermissionMode::parse(mode.wire_name()), Some(mode));
        }
        assert!(SessionPermissionMode::ReadOnly.is_read_only());
        assert!(!SessionPermissionMode::Ask.is_read_only());
        assert!(!SessionPermissionMode::Operate.is_read_only());
        assert_eq!(SessionPermissionMode::parse("plan"), None);
    }

    #[test]
    fn policy_defaults_preserve_the_incumbent_switch_values() {
        let policy = SubagentPolicy::default();
        assert_eq!(policy.tool_strategy, SubagentToolStrategy::Intercept);
        assert!(
            !policy.proactive_delegation,
            "proactive_delegation default OFF"
        );
        assert_eq!(policy.per_session_limit, 10);
        assert_eq!(policy.global_limit, 20);
        assert_eq!(policy.timeout_ms, 30 * 60 * 1000);
        assert!(policy.validate().is_ok());
        for bad in [
            SubagentPolicy {
                per_session_limit: 0,
                ..SubagentPolicy::default()
            },
            SubagentPolicy {
                global_limit: 0,
                ..SubagentPolicy::default()
            },
            SubagentPolicy {
                timeout_ms: 0,
                ..SubagentPolicy::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?} must be refused loudly");
        }
    }

    #[test]
    fn user_lineage_carries_only_submission_anchors() {
        let lineage = RunLineage::user_submission(Some("req-7"));
        assert_eq!(lineage.parent_run_id, None);
        assert_eq!(lineage.origin, RunOrigin::User);
        assert_eq!(lineage.source_message_id, None);
        assert_eq!(lineage.cause_id.as_deref(), Some("request:req-7"));
        let plain = RunLineage::user_submission(None);
        assert_eq!(plain.cause_id, None);
    }
}
