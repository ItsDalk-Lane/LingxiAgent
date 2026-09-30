//! Minimal approval-wait surface (R03-T03): the smallest honest interface
//! that lets a real run enter `waiting_approval` and park there — the
//! FULL tool-policy approval gateway is R04 and is deliberately NOT built
//! here (dispatch boundary: "waiting_approval 进入路径允许最小测试接口
//! （替身工具请求审批），不做 R04 完整审批网关").
//!
//! Frozen incumbent semantics this mirrors (read from the Node stack
//! before mapping): a tool request that needs approval parks the run in
//! an approval wait; the pending entry is BOUNDED in time (ConfirmStore
//! DEFAULT_TIMEOUT = 5min, `lib/confirm-store.ts:11`); a session
//! cancellation resolves every pending approval as `{action:"aborted"}`
//! and the wrapper returns a tool error with ZERO executions; a LATE
//! approve after abort returns false (never resurrects the call).
//!
//! The Rust mapping keeps exactly that shape:
//! - the run driver asks the injected gate BEFORE executing the tool
//!   call (the gate decides whether approval is required at all);
//! - [`ApprovalDecision::Aborted`] (or the request future being dropped
//!   by the cancellation tree — mirrors `abortBySession` +
//!   `clearTimeout`) means the tool is NEVER executed;
//! - a late decision after cancellation is simply never observed: the
//!   cancelled run settles and cannot be resurrected (R03-A07's fence).
//!
//! Production default: NO gate is configured (`ServiceDeps::default`) —
//! until R04 ships the real policy gateway no production run ever parks
//! in `waiting_approval`; the state machine leg exists (kernel) and this
//! minimal interface exercises it through the REAL drive chain in tests
//! only.

use std::pin::Pin;

use lingxi_kernel::RunContext;
use lingxi_protocol::ToolCallId;

use crate::cancel::ScopeKind;
use crate::task_supervisor::TaskKind;

/// One approval request the gate resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub tool_call_id: ToolCallId,
    pub target: String,
    pub args_digest: String,
    /// The shape-only argument summary (R04-T03: keys + type tags, never
    /// values — `summarize_arguments`) so the authenticated approver
    /// surface can show WHAT is being approved without leaking content.
    pub args_summary: Option<String>,
}

/// The decision of one approval wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Approved: the run returns to `running` and executes the call.
    Approved,
    /// Rejected: the run returns to `running`, records the tool failure
    /// and NEVER executes the call.
    Rejected { reason: String },
    /// Aborted (the gate itself gave up — bounded wait, session abort…):
    /// same handling as a rejection with the abort reason; ZERO
    /// executions either way.
    Aborted,
}

/// Contract of the minimal approval gate (R03-T03 test interface; R04
/// replaces implementations behind the same idea with the full policy
/// gateway).
///
/// Cancellation contract: when the caller's cancellation tree fires, the
/// request future is DROPPED at its await point — implementations MUST
/// resolve their pending state on drop exactly like the incumbent's
/// ConfirmStore does on `abortBySession` (clear the bound timer, mark
/// the pending entry aborted so a LATE decision returns false). A
/// dropped request is an aborted request; the tool call is not executed.
pub trait ApprovalGate: Send + Sync {
    /// Decides one tool request. `req` carries the identity the decision
    /// binds to (tool call id + target + args digest).
    fn request<'a>(
        &'a self,
        ctx: &'a RunContext,
        req: &'a ApprovalRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ApprovalDecision> + Send + 'a>>;
}

/// The supervision kind label of the approval wait (used when the driver
/// spawns the wait as a supervised child of the run's tree).
pub fn approval_wait_kind() -> TaskKind {
    TaskKind::from(ScopeKind::ToolCall)
}
