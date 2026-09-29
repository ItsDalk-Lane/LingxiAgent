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

use std::sync::Arc;

use lingxi_kernel::ports::{
    CommittedOutcome, KeyEvent, StorageError, StoragePort, ToolExecutorPort, ToolOutcome,
    ToolRequest, TurnProviderPort,
};
use lingxi_kernel::{
    attempt_id, model_call_id, tool_call_id, FailureCause, NoFinalCause, RunFinish, RunStateMachine,
};
use lingxi_protocol::{
    ContentBlock, ErrorCode, EventId, EventPayload, KnownEventPayload, ModelCallCompletedPayload,
    ModelCallId, ModelCallStartedPayload, ProtocolError, RunId, RunStateChangedPayload, RunStatus,
    ToolCallCompletedPayload, ToolCallDescriptor, ToolCallStartedPayload, ToolResultStatus,
    ToolResultWire,
};

use crate::events::EventService;

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

/// The supervisor: injected provider/tool doubles plus the drive bounds.
/// Stateless per run — all per-run state lives in [`Drive`] locals, so
/// concurrent executes share one supervisor safely.
pub struct RunSupervisor {
    provider: Option<Arc<dyn TurnProviderPort>>,
    tools: Option<Arc<dyn ToolExecutorPort>>,
    limits: RunDriveLimits,
}

impl std::fmt::Debug for RunSupervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunSupervisor")
            .field("provider", &self.provider.as_ref().map(|_| "injected"))
            .field("tools", &self.tools.as_ref().map(|_| "injected"))
            .field("limits", &self.limits)
            .finish()
    }
}

impl RunSupervisor {
    /// Production wiring until R05 registers real providers: no provider,
    /// no tools. Runs driven by this supervisor complete WITHOUT model
    /// content (`completed.no_final.no_provider_configured`) — an explicit
    /// outcome, never a fabricated reply.
    pub fn without_provider() -> Self {
        Self {
            provider: None,
            tools: None,
            limits: RunDriveLimits::default(),
        }
    }

    /// Full injection (tests drive deterministic doubles through the REAL
    /// chain; R05/R04 replace the doubles with real adapters).
    pub fn new(
        provider: Option<Arc<dyn TurnProviderPort>>,
        tools: Option<Arc<dyn ToolExecutorPort>>,
        limits: RunDriveLimits,
    ) -> Result<Self, StorageError> {
        limits.validate()?;
        Ok(Self {
            provider,
            tools,
            limits,
        })
    }

    pub fn limits(&self) -> &RunDriveLimits {
        &self.limits
    }

    pub fn provider_configured(&self) -> bool {
        self.provider.is_some()
    }

    /// Drives ONE run from creation to its single finalize (R03-T01).
    ///
    /// Callers (the session execute surface) have already done the session
    /// lookup and ownership check; `run_id` was allocated by the storage
    /// backend's atomic allocator (fixed at task creation).
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
        run_id: &str,
        input: &str,
        generation: u64,
        now_ms: u64,
    ) -> Result<RunFinish, DriveError> {
        let run_id = RunId::new(run_id.to_string());
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
        events.publish_committed(&started.events);

        // 2) Model turns. No provider configured: explicit no-content
        //    completion (never a fake reply).
        let Some(provider) = self.provider.clone() else {
            let finish = RunFinish::CompletedWithoutFinal {
                cause: NoFinalCause::NoProviderConfigured,
            };
            return self
                .finalize_settlement(port, events, &ctx, RunStatus::Running, finish, now_ms)
                .await;
        };

        let descriptor = provider.descriptor();
        let mut turn: u32 = 0;
        let mut tool_call_seq: u32 = 0;
        let mut saw_tool_failure = false;
        let mut saw_process_content = false;
        let finish = loop {
            turn += 1;
            if turn > self.limits.max_model_turns {
                break RunFinish::Failed {
                    cause: FailureCause::TurnBudgetExceeded {
                        max_turns: self.limits.max_model_turns,
                    },
                };
            }
            let call = model_call_id(&run_id, turn);
            let provider_turn = provider.next_turn(&ctx, &call, turn, input).await;
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
                    let Some(tools) = self.tools.clone() else {
                        break RunFinish::Failed {
                            cause: FailureCause::ToolExecutorUnavailable,
                        };
                    };
                    for request in &requests {
                        tool_call_seq += 1;
                        let call_id = tool_call_id(&run_id, tool_call_seq);
                        self.persist_tool_event(
                            port, events, &ctx, &call_id, request, None, now_ms,
                        )
                        .await?;
                        let outcome = tools.execute(&ctx, &call_id, request).await;
                        if matches!(
                            outcome,
                            ToolOutcome::Failed { .. }
                                | ToolOutcome::Cancelled
                                | ToolOutcome::Unknown { .. }
                        ) {
                            saw_tool_failure = true;
                        }
                        saw_process_content = true;
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

        // 3) Exactly one finalize, through the single settlement path.
        self.finalize_settlement(port, events, &ctx, RunStatus::Running, finish, now_ms)
            .await
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

/// Maps the kernel [`ToolOutcome`] onto the wire tool-result shape. The
/// T01 mapping is minimal and honest: a double's success carries its
/// content digest as the only content block; R04's gateway owns real tool
/// result content. Unknown stays Unknown — never retried, never "success".
fn tool_result_wire(outcome: &ToolOutcome) -> ToolResultWire {
    match outcome {
        ToolOutcome::Success { content_digest } => ToolResultWire {
            status: ToolResultStatus::Success,
            content: vec![ContentBlock::Text {
                text: format!("content-digest:{content_digest}"),
            }],
            resource_refs: Vec::new(),
            truncated: false,
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
        let wire = tool_result_wire(&ToolOutcome::Success {
            content_digest: "abc".to_string(),
        });
        assert_eq!(wire.status, ToolResultStatus::Success);
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
}
