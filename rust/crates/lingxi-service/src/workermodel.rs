//! R05-T06: the production [`WorkerModelPort`] — worker model callbacks
//! served through the REAL model plane (R05 §29.5).
//!
//! One callback = one auxiliary-slot call through the shared
//! [`AuxiliaryExecutor`] (the five real chat protocol families), admitted
//! through the SAME [`QuotaManager`] as the main model loop (C06: the
//! structural precondition is that the run driver releases its model
//! permit after the HTTP turn resolves and before tool execution — the
//! callback's acquire can then succeed; the negative leg pins that a
//! held permit makes the callback's bounded wait time out instead of
//! deadlocking silently).
//!
//! Honesty rules:
//! - `purpose` is a REQUEST, never an authority: the executor's
//!   per-worker whitelist already refused ungranted purposes (C09); this
//!   port maps the granted string to a slot through the host's own
//!   [`AuxiliarySlot::from_purpose`] and refuses anything unknown
//!   (defense in depth — never a guessed slot).
//! - the worker's agent lane is `worker:{plugin_id}` (the callback's
//!   real principal), the session lane the run's session.
//! - the quota wait is bounded by `min(remaining invocation deadline,
//!   quota wait_timeout)`; a quota refusal or a deadline-clamped wait is
//!   a `BudgetExceeded` refusal, never a hang.
//! - every error string reaching the worker comes from the provider
//!   layer, which scrubs in-play credential material before classifying
//!   (the port itself never sees material — C10's credential leg).
//! - the callback's correlation id is `aux-{slot}-{invocation}-{cb_id}`:
//!   a trace joins the child call to its parent worker invocation (C04).
//! - R05-T07: the settled callback's usage/trace fact is written to the
//!   USAGE LEDGER through the real storage port BEFORE the reply returns
//!   to the worker — a ledger failure is a loud callback refusal, never a
//!   silently lost billable request.

use std::pin::Pin;
use std::sync::Arc;

use lingxi_adapters::models::auxiliary::{AuxiliaryExecutor, AuxiliaryRequest};
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::model_exchange::AuxiliarySlot;
use lingxi_kernel::ports::StoragePort;
use lingxi_kernel::RunContext;
use lingxi_protocol::ModelCallId;

use crate::quotas::{QuotaManager, QuotaResource};
use crate::workerrpc::{WorkerModelPort, WorkerModelRefusal, WorkerModelReply, WorkerModelRequest};

/// The correlation fact of ONE settled worker model callback (C04 + T07):
/// the parent invocation + cb_id, the requested purpose, the host-resolved
/// slot, the RESOLVED route identity (never a claimed one) and the richer
/// usage fact the provider actually reported.
#[derive(Debug, Clone)]
pub struct WorkerCallbackTrace {
    // ── the ledger row's identity (T07-C02: parentage from the driver's
    //    context, never from timing) ──
    pub session_id: String,
    pub run_id: String,
    pub attempt: String,
    pub model_call_id: String,
    // ── the callback correlation (C04) ──
    pub invocation: String,
    pub cb_id: String,
    pub purpose: String,
    pub slot: &'static str,
    // ── the resolved route + usage fact (T07) ──
    pub provider: String,
    pub model: String,
    pub protocol: Option<String>,
    pub usage_report: lingxi_kernel::usage::ReportedUsage,
    pub transport_attempts: u32,
}

/// Observability port for settled worker callbacks. R05-T07: the
/// production implementation is [`LedgerWorkerCallbackTrace`] (the usage
/// ledger through the real storage port); tests install a recording
/// implementation to pin parent/child correlation (C04).
pub trait WorkerCallbackTracePort: Send + Sync {
    fn record<'a>(
        &'a self,
        trace: WorkerCallbackTrace,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::StorageError>>
                + Send
                + 'a,
        >,
    >;
}

/// The default trace sink: records nothing (the pre-T07 wiring; the
/// bootstrap installs the ledger trace instead).
pub struct NoopWorkerCallbackTrace;

impl WorkerCallbackTracePort for NoopWorkerCallbackTrace {
    fn record<'a>(
        &'a self,
        _trace: WorkerCallbackTrace,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::StorageError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(()) })
    }
}

/// R05-T07: the production trace sink — one usage-ledger row per settled
/// worker callback. The row's causal anchor is the invocation (cause_ref)
/// under the driving run; the origin vocabulary names the entry surface.
pub struct LedgerWorkerCallbackTrace {
    storage: Arc<RunDatabase>,
    clock: std::sync::Arc<dyn crate::inject::ServiceClock>,
}

impl LedgerWorkerCallbackTrace {
    pub fn new(
        storage: Arc<RunDatabase>,
        clock: std::sync::Arc<dyn crate::inject::ServiceClock>,
    ) -> Self {
        Self { storage, clock }
    }
}

impl WorkerCallbackTracePort for LedgerWorkerCallbackTrace {
    fn record<'a>(
        &'a self,
        trace: WorkerCallbackTrace,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::StorageError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let record = lingxi_kernel::usage::ModelCallUsageRecord {
                session_id: Some(trace.session_id),
                run_id: Some(trace.run_id),
                attempt: Some(trace.attempt),
                model_call_id: trace.model_call_id,
                purpose: format!("auxiliary.{}", trace.slot),
                origin: "worker-callback".to_string(),
                // The callback is caused by the invocation inside the SAME
                // run — the cause ref is the anchor, not a second run.
                parent_run_id: None,
                cause_ref: Some(trace.invocation),
                provider: trace.provider,
                model: trace.model,
                protocol: trace.protocol.unwrap_or_else(|| "unknown".to_string()),
                usage: match trace.usage_report.clone() {
                    lingxi_kernel::usage::ReportedUsage::Known(usage) => Some(usage),
                    _ => None,
                },
                invalid_detail: match &trace.usage_report {
                    lingxi_kernel::usage::ReportedUsage::Invalid { detail } => Some(detail.clone()),
                    _ => None,
                },
                transport_attempts: trace.transport_attempts,
                // T07-C08: no price source exists in this stage — cost
                // stays explicitly unknown.
                cost_basis: None,
            };
            self.storage
                .record_model_call_usage(record, self.clock.now_unix_ms())
                .await
        })
    }
}

/// The production worker-model port: aux-slot route + real provider
/// chain + the shared admission quotas.
pub struct GatewayWorkerModel {
    auxiliary: Arc<AuxiliaryExecutor>,
    quotas: Arc<QuotaManager>,
    trace: Arc<dyn WorkerCallbackTracePort>,
}

impl GatewayWorkerModel {
    pub fn new(auxiliary: Arc<AuxiliaryExecutor>, quotas: Arc<QuotaManager>) -> Self {
        Self::with_trace(auxiliary, quotas, Arc::new(NoopWorkerCallbackTrace))
    }

    pub fn with_trace(
        auxiliary: Arc<AuxiliaryExecutor>,
        quotas: Arc<QuotaManager>,
        trace: Arc<dyn WorkerCallbackTracePort>,
    ) -> Self {
        Self {
            auxiliary,
            quotas,
            trace,
        }
    }
}

/// The remaining wall-clock budget of the invocation deadline (the
/// host-computed value the executor put on the request line), clamping
/// the quota wait so a callback never outlives its own invocation.
fn remaining_deadline_ms(deadline_unix_ms: Option<u64>) -> Option<std::time::Duration> {
    let deadline = deadline_unix_ms?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some(std::time::Duration::from_millis(
        deadline.saturating_sub(now),
    ))
}

impl WorkerModelPort for GatewayWorkerModel {
    fn complete<'a>(
        &'a self,
        ctx: &'a RunContext,
        worker: &'a str,
        invocation: &'a str,
        cb_id: &'a str,
        request: &'a WorkerModelRequest,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<WorkerModelReply, WorkerModelRefusal>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            // Defense in depth (C09): the executor's whitelist already
            // passed; an unmappable purpose is still refused, never
            // guessed onto a slot.
            let Some(slot) = AuxiliarySlot::from_purpose(&request.purpose) else {
                return Err(WorkerModelRefusal::PurposeNotGranted {
                    purpose: request.purpose.clone(),
                });
            };
            // C06: admit through the SAME quota manager as the main model
            // loop (agent lane = the callback's real principal).
            let agent_id = format!("worker:{worker}");
            let acquire =
                self.quotas
                    .acquire(QuotaResource::Model, &agent_id, ctx.session_id.as_str());
            let permit = match remaining_deadline_ms(request.deadline_unix_ms) {
                Some(remaining) => match tokio::time::timeout(remaining, acquire).await {
                    Ok(Ok(permit)) => permit,
                    Ok(Err(failure)) => {
                        return Err(WorkerModelRefusal::BudgetExceeded {
                            detail: format!("{failure}"),
                        });
                    }
                    Err(_) => {
                        return Err(WorkerModelRefusal::BudgetExceeded {
                            detail: "the admission wait outlived the invocation deadline"
                                .to_string(),
                        });
                    }
                },
                None => match acquire.await {
                    Ok(permit) => permit,
                    Err(failure) => {
                        return Err(WorkerModelRefusal::BudgetExceeded {
                            detail: format!("{failure}"),
                        });
                    }
                },
            };
            // The permit is held across the model call and released by
            // RAII on every exit path (C06).
            let _permit = permit;
            let call = ModelCallId::new(format!("aux-{}-{invocation}-{cb_id}", slot.config_key()));
            let outcome = self
                .auxiliary
                .complete(
                    ctx,
                    slot,
                    &call,
                    &AuxiliaryRequest {
                        prompt: request.prompt.clone(),
                        images: Vec::new(),
                        max_output_tokens: Some(request.max_output_tokens),
                        deadline_unix_ms: request.deadline_unix_ms,
                    },
                )
                .await
                .map_err(|failure| WorkerModelRefusal::ProviderRefused {
                    detail: failure.message,
                })?;
            // R05-T07: the usage-ledger row commits BEFORE the reply
            // returns to the worker (C10: an accounting failure is a loud
            // refusal, never a published success).
            self.trace
                .record(WorkerCallbackTrace {
                    session_id: ctx.session_id.to_string(),
                    run_id: ctx.run_id.to_string(),
                    attempt: ctx.attempt.to_string(),
                    model_call_id: call.as_str().to_string(),
                    invocation: invocation.to_string(),
                    cb_id: cb_id.to_string(),
                    purpose: request.purpose.clone(),
                    slot: slot.config_key(),
                    provider: outcome.served_by.provider.clone(),
                    model: outcome.served_by.model.clone(),
                    protocol: outcome.served_protocol.clone(),
                    usage_report: outcome.usage_report,
                    transport_attempts: outcome.transport_attempts,
                })
                .await
                .map_err(|failure| WorkerModelRefusal::ProviderRefused {
                    detail: format!(
                        "the usage ledger refused the callback's accounting row: {failure}"
                    ),
                })?;
            Ok(WorkerModelReply { text: outcome.text })
        })
    }
}
