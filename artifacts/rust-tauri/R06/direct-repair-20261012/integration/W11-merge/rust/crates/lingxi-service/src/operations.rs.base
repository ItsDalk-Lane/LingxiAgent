//! R05-T06 §29.9: the service-side `OperationService` — the REAL Rust
//! operation entry for the non-chat model plane (embedding / rerank /
//! image / video / speech / transcription).
//!
//! What this layer owns (the spec's "模型选择、凭证、网络请求生命周期、
//! 预算和usage仍由Rust持有"):
//! - route resolution through the SAME [`ModelGatewayPort`] as chat (every
//!   operation resolves its OWN binding; an unconfigured operation is the
//!   loud `RouteNotConfigured`, never the chat route's share);
//! - credential material through the SAME [`ProviderCredentialPort`] (no
//!   caller ever sees material; error strings cross the boundary scrubbed
//!   by the provider layer);
//! - admission through the SAME [`QuotaManager`] Model permits as the main
//!   loop and the worker callbacks (C06/C12: an operation call never
//!   bypasses the model concurrency budget);
//! - deadline: every outbound call carries the caller's absolute deadline;
//!   the dialects and the dispatcher enforce it segment-wise;
//! - media PRODUCTS settle only through the egress guard (C11B): URL
//!   products download without credentials, no redirects, guarded IP
//!   policy, mid-read byte caps — then register on disk with a sha256
//!   before anything is reported Completed (C12: a job id is never a
//!   product; `Completed` follows a real, verified, registered file);
//! - the async state machines stay HONEST: `submit_video` proves only job
//!   acceptance; `query_*` reports Generating/Completed/Failed; agnes has
//!   no remote cancel — a local cancel states `remote_cancel:
//!   "unsupported"` and never masquerades as a remote revoke (§29.10).
//!
//! What this layer deliberately does NOT own: media-task persistence and
//! the management HTTP surface are R07 business scope (registered); usage
//! AGGREGATION is T07 (the raw per-answer usage objects ride the outcomes
//! unchanged).
//!
//! `system-speech` (macOS `/usr/bin/say`) is dispatched HERE per §29.13 —
//! argv-formed, no shell, the §29.13 120 s process cap bounded by the
//! caller's remaining budget, and (R05 RR1 F20) spawned through the SAME
//! supervised process plane as exec: process group, reaper, RAII
//! ownership guard, proven-ownership killpg termination, honest exit
//! facts, and intermediate-output cleanup on every non-delivering path.
//! `system-speech-recognition` resolves loudly
//! to `ProtocolNotImplemented` (the Swift/TCC helper belongs to the
//! desktop host's authorization surface; a bare service process cannot
//! hold it — an explicit refusal, not a placeholder).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lingxi_adapters::models::credentials::ProviderCredentialPort;
use lingxi_adapters::models::dispatch::HttpTimeouts;
use lingxi_adapters::models::egress::EgressGuard;
use lingxi_adapters::models::gateway::ConfigModelGateway;
use lingxi_adapters::models::network::NetworkPlane;
use lingxi_adapters::models::operations::{
    self as dialects,
    image::ImageRequest,
    rerank::RerankRequest,
    speech::SpeechRequest,
    transcribe::TranscriptionRequest,
    video::{AgnesVideoQuery, VideoRequest},
    ImageSubmitOutcome, MediaProductRef, OperationDispatcher, TaskPollOutcome,
};
use lingxi_kernel::model_exchange::{
    MediaGenerationKind, ModelGatewayError, ModelGatewayPort, ModelOperation, ModelRouteRequest,
    ProtocolFamily, ResolvedModelRoute,
};
use lingxi_kernel::usage::{CallOutcome, ReportedUsage};
use lingxi_protocol::{ErrorCode, ProtocolError};
use sha2::Digest as _;

use crate::quotas::{QuotaManager, QuotaResource};

/// The failure shape of one operation call (scrubbed at the provider
/// layer before it gets here — no credential material rides it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationFailure {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl std::fmt::Display for OperationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OperationFailure {}

impl OperationFailure {
    fn from_protocol(error: ProtocolError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        }
    }

    fn from_pair((error, _retryable): (ProtocolError, bool)) -> Self {
        Self::from_protocol(error)
    }
}

/// One registered media product: real bytes on disk under the service's
/// product root, with the digest of what was actually written (C03: no
/// ghost registrations — the record exists only after the file does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredProduct {
    pub resource_id: String,
    pub path: PathBuf,
    pub mime: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// The honest image state machine: submit settles synchronously (the
/// fake-async families) or accepts an async task (dashscope).
#[derive(Debug, Clone)]
pub enum ImageGenerationOutcome {
    Done { products: Vec<RegisteredProduct> },
    Pending { task_id: String },
}

/// One image-task poll: honestly generating, completed with REGISTERED
/// products, or failed with the provider's reason.
#[derive(Debug, Clone)]
pub enum MediaTaskStatus {
    Generating,
    Completed {
        products: Vec<RegisteredProduct>,
    },
    Failed {
        reason: String,
    },
    /// The HOST stopped tracking the task (a local cancel). The remote job
    /// was NOT revoked — `remote_cancel` states the provider fact
    /// (`"unsupported"` for agnes, §29.10); nothing here claims a remote
    /// effect that did not happen.
    CancelledLocally {
        remote_cancel: &'static str,
    },
}

/// The acceptance receipt of one async media submit — proof ONLY that the
/// provider accepted the job, never that a product exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaJobAccepted {
    pub task_id: String,
    pub provider_task_id: String,
    /// The provider-side cancel capability fact (C12).
    pub remote_cancel: &'static str,
}

/// Host-read audio for transcription (the caller read it through
/// [`ResourceAccess`]-authorized means; bounded host-side).
#[derive(Debug, Clone)]
pub struct HostAudio {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub filename: String,
}

/// The audio-input size cap (25 MiB — the openai-transcriptions limit the
/// incumbent surfaces; every family reads the same bound).
pub const AUDIO_INPUT_MAX_BYTES: usize = 25 * 1024 * 1024;
/// The reference-image input cap (20 MiB per image).
pub const IMAGE_INPUT_MAX_BYTES: usize = 20 * 1024 * 1024;
/// The system-speech process cap (§29.13) — the OUTER bound of one say
/// synthesis; the effective cap is the smaller of this and the caller's
/// remaining budget.
pub const SYSTEM_SPEECH_TIMEOUT_MS: u64 = 120_000;
/// The system-speech termination-tail bound (§29.13's grace): the
/// operations-lane supervisor's `cleanup_timeout` — the bounded wait for
/// the killed process's exit confirmation before the stop is declared
/// unconfirmed (RR1 F20: the supervisor's proven-ownership killpg chain
/// replaces the removed local TERM/KILL ladder — one cancel system).
pub const SYSTEM_SPEECH_KILL_GRACE_MS: u64 = 1_500;

/// The service-side operation entry (§29.9).
pub struct OperationService {
    gateway: Arc<ConfigModelGateway>,
    credentials: Arc<dyn ProviderCredentialPort>,
    quotas: Arc<QuotaManager>,
    dispatcher: OperationDispatcher,
    egress: Arc<EgressGuard>,
    products_root: PathBuf,
    /// The local tracking registry of async media tasks (in-memory by
    /// design: media-task persistence is the registered R07 business
    /// surface; R05 RR1 F18 keeps the host records unambiguous and
    /// bounded).
    tasks: std::sync::Mutex<BTreeMap<String, Arc<MediaTaskRecord>>>,
    /// R05-T07: the usage-ledger sink — one row per operation-plane
    /// provider request. `None` keeps the pre-T07 shape (no accounting
    /// surface); the bootstrap installs the ledger sink.
    usage_sink: Option<Arc<dyn OperationUsageSink>>,
    /// R05 RR1 F20: the supervised process plane for the macOS
    /// system-speech lane (`/usr/bin/say` spawns through the SAME
    /// ProcessSupervisor discipline as exec — process group, reaper,
    /// TERM/KILL ladder, RAII ownership). `None` keeps the operation
    /// service usable for the network families; the system-speech lane
    /// then REFUSES loudly (fail-closed) instead of spawning an
    /// unsupervised child.
    process_supervisor: Option<Arc<crate::procsupervisor::ProcessSupervisor>>,
}

/// R05 RR1 F21: the OPTIONAL call context an operation caller can carry —
/// a session-scoped caller (R06's in-session embedding/aux calls) passes
/// its real session/run/attempt and the causal anchor of the call; a
/// plane-originated caller passes `None` and the row stays the legal
/// independent root (internal accounting, invisible to owner-scoped
/// queries). The context is a HOST-provided fact — never derived from the
/// request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationCallContext {
    pub session_id: String,
    pub run_id: String,
    pub attempt: Option<String>,
    /// The causal anchor inside the run (e.g. the tool call id that
    /// triggered this operation).
    pub cause_ref: Option<String>,
}

/// R05-T07: the usage fact of ONE settled operation-plane request. The
/// operation plane has no run/session of its own — the row is internal
/// accounting (owner-scoped queries never return it).
pub struct OperationUsageFact {
    pub operation: String,
    pub provider: String,
    pub model: String,
    pub protocol: String,
    pub usage_report: lingxi_kernel::usage::ReportedUsage,
    /// R05 RR1 F20: the REAL count of physical provider HTTP requests
    /// behind this fact. The network families send exactly one today; the
    /// LOCAL system-speech lane sends NONE (a local process is not a
    /// provider HTTP call — recording 1 would fabricate billable wire
    /// traffic that never happened). R05 RR1 F21: a PRE-SEND refusal
    /// (queue timeout under the total budget) also records 0 — not-sent is
    /// never a fabricated 1.
    pub transport_attempts: u32,
    /// R05 RR1 F21: the settlement outcome of the operation call.
    pub outcome: lingxi_kernel::usage::CallOutcome,
    pub started_at_unix_ms: Option<u64>,
    pub settled_at_unix_ms: Option<u64>,
    /// R05 RR1 F21: the caller's context when one was carried (`None`
    /// keeps the plane-root shape: session/run-less internal accounting).
    pub context: Option<OperationCallContext>,
}

/// R05-T07: where operation-plane usage rows land. A failure is LOUD (the
/// caller surfaces it — an accounting failure never masquerades as an
/// operation success).
pub trait OperationUsageSink: Send + Sync {
    fn record<'a>(
        &'a self,
        fact: OperationUsageFact,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::StorageError>>
                + Send
                + 'a,
        >,
    >;
}

/// The production sink: one `model_call_usage` row per operation request,
/// minted with a plane-unique call id (`op-{operation}-{unix_ms}-{seq}`).
pub struct LedgerOperationUsageSink {
    storage: Arc<lingxi_adapters::storage::RunDatabase>,
    clock: Arc<dyn crate::inject::ServiceClock>,
}

impl LedgerOperationUsageSink {
    pub fn new(
        storage: Arc<lingxi_adapters::storage::RunDatabase>,
        clock: Arc<dyn crate::inject::ServiceClock>,
    ) -> Self {
        Self { storage, clock }
    }
}

impl OperationUsageSink for LedgerOperationUsageSink {
    fn record<'a>(
        &'a self,
        fact: OperationUsageFact,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::StorageError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            use std::sync::atomic::Ordering;
            static SINK_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let model_call_id = format!(
                "op-{}-{}-{}",
                fact.operation,
                self.clock.now_unix_ms(),
                SINK_SEQ.fetch_add(1, Ordering::Relaxed)
            );
            // R05 RR1 F21: a carried context makes the row a session-scoped
            // accounting fact (joinable to its run); a None context keeps
            // the plane-root shape (session-less internal accounting).
            let (session_id, run_id, attempt, cause_ref) = match &fact.context {
                Some(context) => (
                    Some(context.session_id.clone()),
                    Some(context.run_id.clone()),
                    context.attempt.clone(),
                    context.cause_ref.clone(),
                ),
                None => (None, None, None, None),
            };
            let record = lingxi_kernel::usage::ModelCallUsageRecord {
                session_id,
                run_id,
                attempt,
                model_call_id,
                purpose: fact.operation.clone(),
                origin: "operation".to_string(),
                parent_run_id: None,
                cause_ref,
                parent_tool_call_id: None,
                provider: fact.provider,
                model: fact.model,
                protocol: fact.protocol,
                usage: match fact.usage_report.clone() {
                    lingxi_kernel::usage::ReportedUsage::Known(usage) => Some(usage),
                    _ => None,
                },
                invalid_detail: match &fact.usage_report {
                    lingxi_kernel::usage::ReportedUsage::Invalid { detail } => Some(detail.clone()),
                    _ => None,
                },
                // The fact's REAL physical wire count (RR1 F20: the local
                // system-speech lane records 0; the network families
                // record 1 today — no refresh-retry on this dispatcher).
                // RR1 F21: a pre-send refusal records 0 (not-sent). F38:
                // every operation fact settles within its own dispatch —
                // the count is always observed (Some), `None` is reserved
                // for dropped-before-settlement calls.
                transport_attempts: Some(fact.transport_attempts),
                outcome: fact.outcome,
                started_at_unix_ms: fact.started_at_unix_ms,
                settled_at_unix_ms: fact.settled_at_unix_ms,
                emitted_tool_calls: Vec::new(),
                // T07-C08: media/ASR bill per item, not per token, and no
                // price source exists in this stage — cost stays unknown.
                cost_basis: None,
            };
            lingxi_kernel::ports::StoragePort::record_model_call_usage(
                self.storage.as_ref(),
                record,
                self.clock.now_unix_ms(),
            )
            .await
        })
    }
}

/// R05 RR1 F18: the kind of one tracked async media task — a task id is
/// only ever polled through the entry of ITS kind (a video id is not an
/// image task id, even when the bare strings coincide).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaTaskKind {
    Image,
    Video,
}

/// R05 RR1 F18/F19: the host-owned, unambiguous record of ONE async media
/// task. It keeps BOTH identities — the HOST TRACKER (the id the caller
/// polls and cancels with) and the PROVIDER job id (what the provider's
/// poll endpoint expects) plus the distinct legacy fallback id — together
/// with the SUBMIT-TIME route identity (provider/model/protocol/endpoint/
/// credential reference/config generation, no credential material), so an
/// old task can never be re-routed onto a new provider by a hot reload.
#[derive(Debug)]
pub struct MediaTaskRecord {
    pub kind: MediaTaskKind,
    /// The id the caller uses (disambiguated when a live task already
    /// holds the provider's bare id).
    pub host_tracker: String,
    /// The provider-facing job id (agnes `video_id=…`; the dashscope task
    /// id).
    pub provider_job_id: String,
    /// The distinct legacy fallback id when the provider distinguishes
    /// them (the incumbent's `{ taskId, providerTaskId }`).
    pub legacy_job_id: Option<String>,
    /// The model name the submit-time route carried (the incumbent's
    /// `model_name` poll parameter).
    pub model_name: String,
    /// The submit-time route identity — polls re-resolve credentials
    /// THROUGH THIS ROUTE, never the current binding.
    pub route: ResolvedModelRoute,
    /// Serializes the settle phase of concurrent polls (one delivery).
    settle: tokio::sync::Mutex<()>,
    /// The fence + terminal receipt (guarded; linearizes cancel vs
    /// completion).
    state: std::sync::Mutex<MediaTaskState>,
}

#[derive(Debug, Clone)]
pub enum MediaTaskState {
    Tracking,
    CancelledLocally,
    /// The terminal receipt: registered products, delivered exactly once.
    Completed(Vec<RegisteredProduct>),
    Failed(String),
}

impl MediaTaskState {
    fn is_live(&self) -> bool {
        matches!(self, MediaTaskState::Tracking)
    }
}

impl MediaTaskRecord {
    fn new(
        kind: MediaTaskKind,
        bare_tracker: String,
        provider_job_id: String,
        legacy_job_id: Option<String>,
        route: ResolvedModelRoute,
    ) -> Self {
        Self {
            kind,
            host_tracker: bare_tracker,
            provider_job_id,
            legacy_job_id,
            model_name: route.model.clone(),
            route,
            settle: tokio::sync::Mutex::new(()),
            state: std::sync::Mutex::new(MediaTaskState::Tracking),
        }
    }

    /// The current status WITHOUT any transition (the read-only fence
    /// check every await boundary performs).
    fn status(&self) -> MediaTaskStatus {
        match &*self.state.lock().expect("media task state") {
            MediaTaskState::Tracking => MediaTaskStatus::Generating,
            MediaTaskState::CancelledLocally => MediaTaskStatus::CancelledLocally {
                remote_cancel: remote_cancel_fact(self.route.protocol),
            },
            MediaTaskState::Completed(products) => MediaTaskStatus::Completed {
                products: products.clone(),
            },
            MediaTaskState::Failed(reason) => MediaTaskStatus::Failed {
                reason: reason.clone(),
            },
        }
    }
}

/// The bound on tracked (live + receipt-holding) task records: the host
/// registry is bounded memory. At the cap, terminal records are evicted
/// first; a registry full of LIVE tasks refuses new submits loudly (a
/// submit that cannot be tracked honestly is not accepted silently).
pub const MEDIA_TASK_REGISTRY_CAP: usize = 128;

/// The per-family remote-cancel capability fact (only agnes exists today;
/// it exposes no cancel API).
fn remote_cancel_fact(protocol: ProtocolFamily) -> &'static str {
    match protocol {
        ProtocolFamily::AgnesVideos => "unsupported",
        _ => "unsupported",
    }
}

/// Projects a guarded task state into the public status (RR1 F18/F19).
fn current_status(state: &MediaTaskState, protocol: &ProtocolFamily) -> MediaTaskStatus {
    match state {
        MediaTaskState::Tracking => MediaTaskStatus::Generating,
        MediaTaskState::CancelledLocally => MediaTaskStatus::CancelledLocally {
            remote_cancel: remote_cancel_fact(*protocol),
        },
        MediaTaskState::Completed(products) => MediaTaskStatus::Completed {
            products: products.clone(),
        },
        MediaTaskState::Failed(reason) => MediaTaskStatus::Failed {
            reason: reason.clone(),
        },
    }
}

impl OperationService {
    /// Builds the service. `provider_endpoints` seeds the egress allowlist
    /// (URL products may only leave through same-origin or public-https
    /// destinations); `products_root` is where registered products land.
    pub fn new(
        gateway: Arc<ConfigModelGateway>,
        credentials: Arc<dyn ProviderCredentialPort>,
        quotas: Arc<QuotaManager>,
        provider_endpoints: Vec<String>,
        products_root: PathBuf,
    ) -> Result<Self, OperationFailure> {
        Self::new_with_network(
            gateway,
            credentials,
            quotas,
            provider_endpoints,
            products_root,
            None,
        )
    }

    /// R05 RR1 F14: [`Self::new`] with the model plane's shared network
    /// policy — the operation plane's dispatcher AND the egress download
    /// client route under the SAME frozen proxy / NO_PROXY / explicit-CA
    /// policy as the chat loop (`None` keeps the pre-F14 direct behavior).
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_network(
        gateway: Arc<ConfigModelGateway>,
        credentials: Arc<dyn ProviderCredentialPort>,
        quotas: Arc<QuotaManager>,
        provider_endpoints: Vec<String>,
        products_root: PathBuf,
        network: Option<std::sync::Arc<NetworkPlane>>,
    ) -> Result<Self, OperationFailure> {
        std::fs::create_dir_all(&products_root).map_err(|err| OperationFailure {
            code: ErrorCode::Internal,
            message: format!(
                "cannot create the media product root {}: {err}",
                products_root.display()
            ),
            retryable: false,
        })?;
        let egress = match &network {
            Some(plane) => {
                EgressGuard::from_provider_endpoints_and_network(&provider_endpoints, plane.clone())
            }
            None => EgressGuard::from_provider_endpoints(&provider_endpoints),
        }
        .map_err(OperationFailure::from_protocol)?;
        let dispatcher = match &network {
            Some(plane) => OperationDispatcher::with_timeouts_and_network(
                HttpTimeouts::default(),
                plane.clone(),
            ),
            None => OperationDispatcher::with_timeouts(HttpTimeouts::default()),
        }
        .map_err(OperationFailure::from_protocol)?;
        Ok(Self {
            gateway,
            credentials,
            quotas,
            dispatcher,
            egress: Arc::new(egress),
            products_root,
            tasks: std::sync::Mutex::new(BTreeMap::new()),
            usage_sink: None,
            process_supervisor: None,
        })
    }

    /// R05 RR1 F20: installs the supervised process plane (the
    /// system-speech lane's spawn/ownership discipline).
    pub fn with_process_supervisor(
        mut self,
        supervisor: Arc<crate::procsupervisor::ProcessSupervisor>,
    ) -> Self {
        self.process_supervisor = Some(supervisor);
        self
    }

    /// R05-T07: installs the usage-ledger sink (the bootstrap wiring; the
    /// pre-T07 no-sink shape stays available for narrow test doubles).
    pub fn with_usage_sink(mut self, sink: Arc<dyn OperationUsageSink>) -> Self {
        self.usage_sink = Some(sink);
        self
    }

    /// R05-T07: records ONE operation-plane request's usage fact. Unknown
    /// stays unknown (never zero); a sink failure is a loud
    /// [`OperationFailure`] (accounting never masquerades as success).
    /// R05 RR1 F20: `transport_attempts` states the REAL physical wire
    /// count (the local system-speech lane records 0 — it makes no
    /// provider HTTP request). R05 RR1 F21: the row carries the call's
    /// outcome, timing and (when the caller carried one) its session/run
    /// context.
    async fn record_operation_usage(
        &self,
        operation: &str,
        route: &ResolvedModelRoute,
        usage_report: lingxi_kernel::usage::ReportedUsage,
        context: Option<&OperationCallContext>,
        outcome: lingxi_kernel::usage::CallOutcome,
        started_at_unix_ms: Option<u64>,
    ) -> Result<(), OperationFailure> {
        self.record_operation_usage_with_attempts(
            operation,
            route,
            usage_report,
            1,
            context,
            outcome,
            started_at_unix_ms,
        )
        .await
    }

    /// The transport-attempt-explicit form (RR1 F20: the local lane
    /// records its ZERO physical HTTP requests through here; RR1 F21: a
    /// pre-send refusal records its not-sent 0 through here too).
    #[allow(clippy::too_many_arguments)]
    async fn record_operation_usage_with_attempts(
        &self,
        operation: &str,
        route: &ResolvedModelRoute,
        usage_report: lingxi_kernel::usage::ReportedUsage,
        transport_attempts: u32,
        context: Option<&OperationCallContext>,
        outcome: lingxi_kernel::usage::CallOutcome,
        started_at_unix_ms: Option<u64>,
    ) -> Result<(), OperationFailure> {
        let Some(sink) = self.usage_sink.as_ref() else {
            return Ok(());
        };
        sink.record(OperationUsageFact {
            operation: operation.to_string(),
            provider: route.provider.clone(),
            model: route.model.clone(),
            protocol: route.protocol.config_name().to_string(),
            usage_report,
            transport_attempts,
            outcome,
            started_at_unix_ms,
            settled_at_unix_ms: Some(lingxi_adapters::models::dispatch::unix_ms_now()),
            context: context.cloned(),
        })
        .await
        .map_err(|failure| OperationFailure {
            code: ErrorCode::Internal,
            message: format!("the usage ledger refused the {operation} accounting row: {failure}"),
            retryable: false,
        })
    }

    /// R05 RR1 F21: records the NOT-SENT accounting row of an operation
    /// invocation that was accepted, routed and then refused BEFORE
    /// anything left the process (the queue wait outlived the call's total
    /// budget). The invocation is a real fact that settled pre-send — its
    /// row states 0 transport attempts (never the fabricated 1) and an
    /// unknown usage (never zero). A ledger failure here surfaces loudly
    /// (the accounting row is part of the invocation's settlement).
    async fn record_operation_not_sent(
        &self,
        operation: &str,
        route: &ResolvedModelRoute,
        context: Option<&OperationCallContext>,
        started_at_unix_ms: Option<u64>,
    ) -> Result<(), OperationFailure> {
        self.record_operation_usage_with_attempts(
            operation,
            route,
            lingxi_kernel::usage::ReportedUsage::Unknown,
            0,
            context,
            lingxi_kernel::usage::CallOutcome::Failed,
            started_at_unix_ms,
        )
        .await
    }

    // ── the shared discipline ────────────────────────────────────────────

    /// Resolves the route + applicable credential for one operation (the
    /// single entry every public method shares — no caller-side routing).
    async fn resolve(
        &self,
        operation: ModelOperation,
    ) -> Result<
        (
            ResolvedModelRoute,
            lingxi_adapters::models::credentials::ApplicableAuth,
        ),
        OperationFailure,
    > {
        let route = self
            .gateway
            .resolve_route(&ModelRouteRequest::for_operation(operation))
            .map_err(|error| {
                let code = match &error {
                    ModelGatewayError::UnknownProvider { .. }
                    | ModelGatewayError::RouteNotConfigured { .. } => ErrorCode::NotFound,
                    ModelGatewayError::MissingCredential { .. } => ErrorCode::Unauthorized,
                    ModelGatewayError::ModelPinRequiresProvider { .. }
                    | ModelGatewayError::OperationUnsupportedByProvider { .. }
                    | ModelGatewayError::CapabilityUnsupported { .. }
                    | ModelGatewayError::ProtocolNotImplemented { .. } => ErrorCode::InvalidMessage,
                };
                OperationFailure {
                    code,
                    message: format!("{error}"),
                    retryable: false,
                }
            })?;
        // system-speech-recognition is the §29.13 platform-honesty refusal.
        if route.protocol == ProtocolFamily::SystemSpeechRecognition {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: "system-speech-recognition is not implemented by the Rust service: \
                          the Swift/TCC helper belongs to the desktop host's authorization \
                          surface, which a bare service process cannot hold (an explicit \
                          refusal — use a provider ASR family)"
                    .to_string(),
                retryable: false,
            });
        }
        let auth = self
            .credentials
            .resolve(&route)
            .await
            .map_err(|error| OperationFailure {
                code: ErrorCode::Unauthorized,
                message: format!("{error}"),
                retryable: false,
            })?;
        Ok((route, auth))
    }

    /// Acquires the model-plane admission permit (the SAME quota manager
    /// as the chat loop and the worker callbacks). R05 RR1 F16: the QUEUE
    /// WAIT itself is under the call's ABSOLUTE deadline — the acquire
    /// races the remaining budget and an exhausted deadline surfaces as
    /// the honest, non-retryable `BudgetExceeded` (the pre-fix shape
    /// waited the quota manager's full `wait_timeout_ms` even when the
    /// call's total budget had already passed, then reported the expiry
    /// only after the permit finally moved).
    async fn admit(
        &self,
        lane: &str,
        deadline_unix_ms: Option<u64>,
    ) -> Result<crate::quotas::QuotaPermit, OperationFailure> {
        let budget_exceeded = || OperationFailure {
            code: ErrorCode::BudgetExceeded,
            message: "model-plane admission wait outlived the call's total budget \
                      (deadline_unix_ms reached while queued for a model permit); not \
                      retryable under the same budget"
                .to_string(),
            retryable: false,
        };
        let acquire = self
            .quotas
            .acquire(QuotaResource::Model, lane, "model-operations");
        let permit = match lingxi_adapters::models::dispatch::remaining_budget_ms(deadline_unix_ms)
        {
            Some(remaining) => {
                match tokio::time::timeout(std::time::Duration::from_millis(remaining), acquire)
                    .await
                {
                    Ok(Ok(permit)) => permit,
                    Ok(Err(failure)) => {
                        return Err(OperationFailure {
                            code: ErrorCode::BudgetExceeded,
                            message: format!("model-plane admission refused: {failure}"),
                            retryable: false,
                        });
                    }
                    Err(_) => return Err(budget_exceeded()),
                }
            }
            None => match acquire.await {
                Ok(permit) => permit,
                Err(failure) => {
                    return Err(OperationFailure {
                        code: ErrorCode::BudgetExceeded,
                        message: format!("model-plane admission refused: {failure}"),
                        retryable: false,
                    });
                }
            },
        };
        Ok(permit)
    }

    /// Registers product bytes on disk (C03: the record exists only after
    /// the file does — a registered product is a real file with the digest
    /// of exactly what was written).
    fn register_product(
        &self,
        kind: &str,
        bytes: &[u8],
        mime: &str,
    ) -> Result<RegisteredProduct, OperationFailure> {
        if bytes.is_empty() {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: format!(
                    "a {kind} product arrived EMPTY; refusing to register a ghost file"
                ),
                retryable: false,
            });
        }
        let mut entropy = [0u8; 8];
        getrandom::getrandom(&mut entropy).map_err(|err| OperationFailure {
            code: ErrorCode::Internal,
            message: format!("product id entropy failed: {err}"),
            retryable: false,
        })?;
        let resource_id = format!(
            "{kind}-{}-{}",
            lingxi_adapters::models::dispatch::unix_ms_now(),
            entropy
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let path = self.products_root.join(&resource_id);
        std::fs::write(&path, bytes).map_err(|err| OperationFailure {
            code: ErrorCode::Internal,
            message: format!(
                "cannot persist the {kind} product at {}: {err}",
                path.display()
            ),
            retryable: false,
        })?;
        let sha256 = sha2::Sha256::digest(bytes);
        Ok(RegisteredProduct {
            resource_id,
            path,
            mime: mime.to_string(),
            size_bytes: bytes.len() as u64,
            sha256: entropy_to_hex(&sha256),
        })
    }

    /// Settles ONE media product reference into a registered product:
    /// bytes register directly; a URL downloads through the egress guard
    /// (no credentials, guarded policy, mid-read cap) and registers.
    async fn settle_product(
        &self,
        kind: &str,
        product: MediaProductRef,
        deadline_unix_ms: Option<u64>,
    ) -> Result<RegisteredProduct, OperationFailure> {
        match product {
            MediaProductRef::Bytes { bytes, mime } => self.register_product(kind, &bytes, &mime),
            MediaProductRef::Url { url, mime_hint } => {
                let (bytes, content_type) = self
                    .egress
                    .download(&url, deadline_unix_ms)
                    .await
                    .map_err(OperationFailure::from_pair)?;
                let mime = if content_type.is_empty() {
                    mime_hint.unwrap_or_else(|| "application/octet-stream".to_string())
                } else {
                    content_type
                };
                self.register_product(kind, &bytes, &mime)
            }
        }
    }

    async fn settle_products(
        &self,
        kind: &str,
        products: Vec<MediaProductRef>,
        deadline_unix_ms: Option<u64>,
    ) -> Result<Vec<RegisteredProduct>, OperationFailure> {
        let mut out = Vec::with_capacity(products.len());
        for product in products {
            out.push(self.settle_product(kind, product, deadline_unix_ms).await?);
        }
        Ok(out)
    }

    /// Atomically swaps the egress allowlist to the CURRENT provider
    /// endpoints (the management reload surface calls this right after the
    /// gateway swap — same-origin egress follows the live config).
    pub fn refresh_egress_origins(&self, endpoints: &[String]) -> Result<(), ProtocolError> {
        self.egress.set_provider_endpoints(endpoints)
    }

    /// R05 RR1 F18: registers one tracked media task and returns the HOST
    /// TRACKER id the caller polls/cancels with. A live task already
    /// holding the provider's bare tracker id forces a disambiguated
    /// tracker (`{bare}-{n}`) — two live tasks never share one record;
    /// terminal records at the registry cap are evicted first; a registry
    /// full of LIVE tasks refuses the submit loudly (zero side effects
    /// after the provider accepted… tracked honestly: the refusal names
    /// the registry fact, the caller still holds the provider receipt).
    fn track_media_task(
        &self,
        kind: MediaTaskKind,
        bare_tracker: String,
        provider_job_id: String,
        legacy_job_id: Option<String>,
        route: ResolvedModelRoute,
    ) -> Result<String, OperationFailure> {
        let mut tasks = self.tasks.lock().expect("media task registry");
        if tasks.len() >= MEDIA_TASK_REGISTRY_CAP {
            // Evict one terminal record (receipts are replayable only
            // while tracked; eviction is honest — a later query reports
            // the task as no longer tracked by this instance).
            if let Some(evict) = tasks
                .iter()
                .find(|(_, record)| !record.state.lock().expect("media task state").is_live())
                .map(|(id, _)| id.clone())
            {
                tasks.remove(&evict);
            } else {
                return Err(OperationFailure {
                    code: ErrorCode::BudgetExceeded,
                    message: format!(
                        "the media task registry is at its cap of {MEDIA_TASK_REGISTRY_CAP} \
                         LIVE tasks; refusing to track another task without a record — the \
                         provider's job may exist; retry after tasks settle"
                    ),
                    retryable: true,
                });
            }
        }
        // The bare tracker id is the host tracker when free (or held only
        // by a terminal record); a LIVE collision mints a disambiguated
        // tracker.
        let mut tracker = bare_tracker.clone();
        let mut attempt = 1usize;
        loop {
            let live_collision = tasks
                .get(&tracker)
                .is_some_and(|record| record.state.lock().expect("media task state").is_live());
            if !live_collision {
                tasks.insert(
                    tracker.clone(),
                    Arc::new(MediaTaskRecord::new(
                        kind,
                        tracker.clone(),
                        provider_job_id,
                        legacy_job_id,
                        route,
                    )),
                );
                return Ok(tracker);
            }
            attempt += 1;
            tracker = format!("{bare_tracker}-{attempt}");
        }
    }

    /// The shared lookup: kind must match, the record must exist — a task
    /// id from another kind/process/fabrication is refused honestly.
    fn tracked_task(
        &self,
        task_id: &str,
        kind: MediaTaskKind,
    ) -> Result<Arc<MediaTaskRecord>, OperationFailure> {
        self.tasks
            .lock()
            .expect("media task registry")
            .get(task_id)
            .cloned()
            .filter(|record| record.kind == kind)
            .ok_or_else(|| OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: format!(
                    "media task {task_id:?} is not tracked by this service instance as a \
                     {} task (a task id from another process, another operation kind, or a \
                     fabricated id is not a pollable fact)",
                    match kind {
                        MediaTaskKind::Image => "image",
                        MediaTaskKind::Video => "video",
                    }
                ),
                retryable: false,
            })
    }

    // ── embedding / rerank (the text operation plane) ────────────────────

    /// One embedding call: validate → route → admit → dispatch → parse.
    /// R05 RR1 F21: an OPTIONAL [`OperationCallContext`] carries the
    /// caller's real session/run/causal anchor (a plane-originated caller
    /// passes `None` — the legal independent root).
    pub async fn embed(
        &self,
        request: dialects::embedding::EmbeddingRequest,
        deadline_unix_ms: Option<u64>,
        context: Option<&OperationCallContext>,
    ) -> Result<dialects::embedding::EmbeddingOutcome, OperationFailure> {
        let started_at = lingxi_adapters::models::dispatch::unix_ms_now();
        dialects::embedding::validate_embedding_request(&request)
            .map_err(OperationFailure::from_protocol)?;
        let (route, auth) = self.resolve(ModelOperation::Embedding).await?;
        let _permit = match self.admit("operations:embedding", deadline_unix_ms).await {
            Ok(permit) => permit,
            Err(refusal) => {
                // R05 RR1 F21: the invocation really happened and settled
                // pre-send — the not-sent row (0 attempts, unknown usage)
                // is part of its settlement, never a vanished fact.
                self.record_operation_not_sent("embedding", &route, context, Some(started_at))
                    .await?;
                return Err(refusal);
            }
        };
        let plan = dialects::embedding::build_embedding(&route, &auth, &request)
            .map_err(OperationFailure::from_protocol)?;
        let body = match self
            .dispatcher
            .execute_json(&plan, deadline_unix_ms, &auth)
            .await
        {
            Ok(body) => body,
            Err(pair) => {
                // R05-T07 (C03): a possibly-billable request with no usable
                // usage still leaves its accounting row (unknown ≠ 0).
                self.record_operation_usage(
                    "embedding",
                    &route,
                    ReportedUsage::Unknown,
                    context,
                    CallOutcome::Failed,
                    Some(started_at),
                )
                .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        match dialects::embedding::parse_embedding(route.protocol, &request, &body)
            .map_err(OperationFailure::from_protocol)
        {
            Ok(outcome) => {
                let report = match outcome.usage.as_ref() {
                    Some(usage) => {
                        match lingxi_adapters::models::usage::decode_operation_usage(usage) {
                            lingxi_adapters::models::usage::UsageDecode::Usage(fact) => {
                                ReportedUsage::Known(fact)
                            }
                            lingxi_adapters::models::usage::UsageDecode::Invalid { detail } => {
                                ReportedUsage::Invalid { detail }
                            }
                            lingxi_adapters::models::usage::UsageDecode::Absent => {
                                ReportedUsage::Unknown
                            }
                        }
                    }
                    None => ReportedUsage::Unknown,
                };
                self.record_operation_usage(
                    "embedding",
                    &route,
                    report,
                    context,
                    CallOutcome::Succeeded,
                    Some(started_at),
                )
                .await?;
                Ok(outcome)
            }
            Err(failure) => {
                self.record_operation_usage(
                    "embedding",
                    &route,
                    ReportedUsage::Unknown,
                    context,
                    CallOutcome::Failed,
                    Some(started_at),
                )
                .await?;
                Err(failure)
            }
        }
    }

    /// One rerank call: validate → route → admit → dispatch → parse.
    /// R05 RR1 F21: see [`Self::embed`] for the optional call context.
    pub async fn rerank(
        &self,
        request: RerankRequest,
        deadline_unix_ms: Option<u64>,
        context: Option<&OperationCallContext>,
    ) -> Result<dialects::rerank::RerankOutcome, OperationFailure> {
        let started_at = lingxi_adapters::models::dispatch::unix_ms_now();
        let top_n = dialects::rerank::validate_rerank_request(&request)
            .map_err(OperationFailure::from_protocol)?;
        let (route, auth) = self.resolve(ModelOperation::Rerank).await?;
        let _permit = match self.admit("operations:rerank", deadline_unix_ms).await {
            Ok(permit) => permit,
            Err(refusal) => {
                self.record_operation_not_sent("rerank", &route, context, Some(started_at))
                    .await?;
                return Err(refusal);
            }
        };
        let plan = dialects::rerank::build_rerank(&route, &auth, &request)
            .map_err(OperationFailure::from_protocol)?;
        let body = match self
            .dispatcher
            .execute_json(&plan, deadline_unix_ms, &auth)
            .await
        {
            Ok(body) => body,
            Err(pair) => {
                self.record_operation_usage(
                    "rerank",
                    &route,
                    ReportedUsage::Unknown,
                    context,
                    CallOutcome::Failed,
                    Some(started_at),
                )
                .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        match dialects::rerank::parse_rerank(route.protocol, top_n, request.documents.len(), &body)
            .map_err(OperationFailure::from_protocol)
        {
            Ok(outcome) => {
                let report = match outcome.usage.as_ref() {
                    Some(usage) => {
                        match lingxi_adapters::models::usage::decode_operation_usage(usage) {
                            lingxi_adapters::models::usage::UsageDecode::Usage(fact) => {
                                ReportedUsage::Known(fact)
                            }
                            lingxi_adapters::models::usage::UsageDecode::Invalid { detail } => {
                                ReportedUsage::Invalid { detail }
                            }
                            lingxi_adapters::models::usage::UsageDecode::Absent => {
                                ReportedUsage::Unknown
                            }
                        }
                    }
                    None => ReportedUsage::Unknown,
                };
                self.record_operation_usage(
                    "rerank",
                    &route,
                    report,
                    context,
                    CallOutcome::Succeeded,
                    Some(started_at),
                )
                .await?;
                Ok(outcome)
            }
            Err(failure) => {
                self.record_operation_usage(
                    "rerank",
                    &route,
                    ReportedUsage::Unknown,
                    context,
                    CallOutcome::Failed,
                    Some(started_at),
                )
                .await?;
                Err(failure)
            }
        }
    }

    // ── image generation (the honest submit/query state machine) ────────

    /// One image submit. Synchronous families settle to `Done` with
    /// REGISTERED products; dashscope accepts a task (`Pending` — the
    /// caller polls [`Self::query_media_task`]).
    pub async fn generate_image(
        &self,
        request: ImageRequest,
        deadline_unix_ms: Option<u64>,
    ) -> Result<ImageGenerationOutcome, OperationFailure> {
        let (route, auth) = self
            .resolve(ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Image,
            })
            .await?;
        let _permit = self.admit("operations:image", deadline_unix_ms).await?;
        if route.protocol == ProtocolFamily::OpenAiCodexResponsesImage {
            return self
                .codex_image(&route, &auth, &request, deadline_unix_ms)
                .await;
        }
        let plan =
            dialects::image::build_image_submit(&route, &auth, &request, &serde_json::Value::Null)
                .map_err(OperationFailure::from_protocol)?;
        let body = match self
            .dispatcher
            .execute_json(&plan, deadline_unix_ms, &auth)
            .await
        {
            Ok(body) => body,
            Err(pair) => {
                // Image families bill per item and carry no token usage —
                // the request fact still lands (unknown ≠ 0).
                self.record_operation_usage(
                    "image",
                    &route,
                    ReportedUsage::Unknown,
                    None,
                    CallOutcome::Failed,
                    None,
                )
                .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        // R05-T07: the request fact is recorded once the response arrived
        // (per-item billing families report no token usage on the wire).
        self.record_operation_usage(
            "image",
            &route,
            ReportedUsage::Unknown,
            None,
            CallOutcome::Succeeded,
            None,
        )
        .await?;
        match dialects::image::parse_image_submit(&route, &request, &serde_json::Value::Null, &body)
            .map_err(OperationFailure::from_protocol)?
        {
            ImageSubmitOutcome::Done { products } => {
                let products = self
                    .settle_products("image", products, deadline_unix_ms)
                    .await?;
                Ok(ImageGenerationOutcome::Done { products })
            }
            ImageSubmitOutcome::Pending { task_id } => {
                // RR1 F18: track the task with its submit-time route
                // identity; the returned id is the HOST TRACKER (the
                // provider's bare id unless a live task already holds it).
                let host_tracker = self.track_media_task(
                    MediaTaskKind::Image,
                    task_id.clone(),
                    task_id.clone(),
                    None,
                    route,
                )?;
                Ok(ImageGenerationOutcome::Pending {
                    task_id: host_tracker,
                })
            }
        }
    }

    /// The codex image family answers SSE, not JSON (the bounded aggregate
    /// text parses through the dedicated dialect entry).
    async fn codex_image(
        &self,
        route: &ResolvedModelRoute,
        auth: &lingxi_adapters::models::credentials::ApplicableAuth,
        request: &ImageRequest,
        deadline_unix_ms: Option<u64>,
    ) -> Result<ImageGenerationOutcome, OperationFailure> {
        let plan =
            dialects::image::build_image_submit(route, auth, request, &serde_json::Value::Null)
                .map_err(OperationFailure::from_protocol)?;
        let (bytes, _content_type) = match self
            .dispatcher
            .execute_bytes(&plan, deadline_unix_ms, auth)
            .await
        {
            Ok(done) => done,
            Err(pair) => {
                self.record_operation_usage(
                    "image",
                    route,
                    ReportedUsage::Unknown,
                    None,
                    CallOutcome::Failed,
                    None,
                )
                .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        self.record_operation_usage(
            "image",
            route,
            ReportedUsage::Unknown,
            None,
            CallOutcome::Succeeded,
            None,
        )
        .await?;
        let aggregate = String::from_utf8_lossy(&bytes).into_owned();
        let products = dialects::image::parse_openai_codex_image_sse(
            request,
            &serde_json::Value::Null,
            &aggregate,
        )
        .map_err(OperationFailure::from_protocol)?;
        let products = self
            .settle_products("image", products, deadline_unix_ms)
            .await?;
        Ok(ImageGenerationOutcome::Done { products })
    }

    /// Polls ONE async media task (dashscope image today): the poll goes
    /// out only while the task is locally tracked, against the task's
    /// SUBMIT-TIME route (an old task never re-routes onto a new
    /// provider); a Completed answer follows download + verify +
    /// registration behind the cancellation fence (a job id is never a
    /// product).
    pub async fn query_media_task(
        &self,
        task_id: &str,
        deadline_unix_ms: Option<u64>,
    ) -> Result<MediaTaskStatus, OperationFailure> {
        let record = self.tracked_task(task_id, MediaTaskKind::Image)?;
        if !matches!(
            *record.state.lock().expect("media task state"),
            MediaTaskState::Tracking
        ) {
            return Ok(record.status());
        }
        let _permit = self
            .admit("operations:image-query", deadline_unix_ms)
            .await?;
        // RR1 F18: credentials resolve THROUGH THE STORED ROUTE — the
        // submitting provider's fact, never the current binding.
        let auth = self
            .credentials
            .resolve(&record.route)
            .await
            .map_err(|error| OperationFailure {
                code: ErrorCode::Unauthorized,
                message: format!(
                    "the image task's submitting provider \
                     ({}/{}) can no longer supply credentials: {error}",
                    record.route.provider, record.route.endpoint
                ),
                retryable: false,
            })?;
        let plan = dialects::image::build_dashscope_image_query(
            &record.route,
            &auth,
            &record.provider_job_id,
        );
        let body = self
            .dispatcher
            .execute_json(&plan, deadline_unix_ms, &auth)
            .await
            .map_err(OperationFailure::from_pair)?;
        let outcome = dialects::image::parse_dashscope_image_query(&body)
            .map_err(OperationFailure::from_protocol)?;
        self.settle_poll("image", &record, outcome, deadline_unix_ms)
            .await
    }

    /// Maps ONE parsed poll outcome onto the honest state machine behind
    /// the R05 RR1 F19 cancellation fence: every await boundary (the poll
    /// response, the download, the registration) re-checks the task
    /// state; the terminal COMMIT (cancel vs complete) is a single
    /// linearized transition; concurrent polls serialize on the task's
    /// settle mutex and deliver the SAME receipt exactly once.
    async fn settle_poll(
        &self,
        kind: &str,
        record: &Arc<MediaTaskRecord>,
        outcome: TaskPollOutcome,
        deadline_unix_ms: Option<u64>,
    ) -> Result<MediaTaskStatus, OperationFailure> {
        match outcome {
            TaskPollOutcome::Pending => Ok(MediaTaskStatus::Generating),
            TaskPollOutcome::Failed { reason } => {
                // A late failure never resurrects a settled task: the
                // transition only lands while still Tracking.
                let mut state = record.state.lock().expect("media task state");
                if state.is_live() {
                    *state = MediaTaskState::Failed(reason);
                }
                Ok(current_status(&state, &record.route.protocol))
            }
            TaskPollOutcome::Done { products } => {
                // One poll settles at a time; the second concurrent poll
                // finds the committed receipt and replays it.
                let _settle = record.settle.lock().await;
                // Fence 1 — the poll RESPONSE is in: an acknowledged
                // cancellation wins before any download starts.
                {
                    let state = record.state.lock().expect("media task state");
                    if !state.is_live() {
                        return Ok(current_status(&state, &record.route.protocol));
                    }
                }
                // Download every product into memory (no disk writes yet).
                let mut staged = Vec::with_capacity(products.len());
                for product in products {
                    staged.push(match product {
                        MediaProductRef::Bytes { bytes, mime } => (bytes, mime),
                        MediaProductRef::Url { url, mime_hint } => {
                            let (bytes, content_type) = self
                                .egress
                                .download(&url, deadline_unix_ms)
                                .await
                                .map_err(OperationFailure::from_pair)?;
                            let mime = if content_type.is_empty() {
                                mime_hint.unwrap_or_else(|| "application/octet-stream".to_string())
                            } else {
                                content_type
                            };
                            (bytes, mime)
                        }
                    });
                }
                // Fence 2 — the DOWNLOAD completed: re-check before any
                // disk write.
                {
                    let state = record.state.lock().expect("media task state");
                    if !state.is_live() {
                        return Ok(current_status(&state, &record.route.protocol));
                    }
                }
                let mut registered = Vec::with_capacity(staged.len());
                for (bytes, mime) in staged {
                    match self.register_product(kind, &bytes, &mime) {
                        Ok(product) => registered.push(product),
                        Err(failure) => {
                            // A partial registration is cleaned up — no
                            // half-registered artifacts remain.
                            for product in &registered {
                                let _ = std::fs::remove_file(&product.path);
                            }
                            return Err(failure);
                        }
                    }
                }
                // Fence 3 — the COMMIT: cancel and completion linearize on
                // this one transition. A cancellation that arrived during
                // the registration wins: the files are cleaned and the
                // cancellation stands.
                let mut state = record.state.lock().expect("media task state");
                if !state.is_live() {
                    drop(state);
                    for product in &registered {
                        let _ = std::fs::remove_file(&product.path);
                    }
                    let state = record.state.lock().expect("media task state");
                    return Ok(current_status(&state, &record.route.protocol));
                }
                *state = MediaTaskState::Completed(registered);
                Ok(current_status(&state, &record.route.protocol))
            }
        }
    }

    // ── video (agnes): submit proves acceptance; query settles ──────────

    /// One agnes video submit — the receipt proves ONLY job acceptance.
    pub async fn submit_video(
        &self,
        request: VideoRequest,
        deadline_unix_ms: Option<u64>,
    ) -> Result<MediaJobAccepted, OperationFailure> {
        let (route, auth) = self
            .resolve(ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Video,
            })
            .await?;
        let _permit = self.admit("operations:video", deadline_unix_ms).await?;
        let plan =
            dialects::video::build_video_submit(&route, &auth, &request, &serde_json::Value::Null)
                .map_err(OperationFailure::from_protocol)?;
        let body = match self
            .dispatcher
            .execute_json(&plan, deadline_unix_ms, &auth)
            .await
        {
            Ok(body) => body,
            Err(pair) => {
                self.record_operation_usage(
                    "video",
                    &route,
                    ReportedUsage::Unknown,
                    None,
                    CallOutcome::Failed,
                    None,
                )
                .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        self.record_operation_usage(
            "video",
            &route,
            ReportedUsage::Unknown,
            None,
            CallOutcome::Succeeded,
            None,
        )
        .await?;
        let outcome =
            dialects::video::parse_video_submit(&body).map_err(OperationFailure::from_protocol)?;
        // RR1 F18: keep BOTH identities — the host tracker (returned to
        // the caller) and the provider job id (what `video_id=` expects),
        // with the distinct tracker as the legacy fallback — plus the
        // submit-time route identity for every later poll.
        let legacy_job_id =
            (outcome.task_id != outcome.provider_task_id).then(|| outcome.task_id.clone());
        let host_tracker = self.track_media_task(
            MediaTaskKind::Video,
            outcome.task_id.clone(),
            outcome.provider_task_id.clone(),
            legacy_job_id,
            route.clone(),
        )?;
        Ok(MediaJobAccepted {
            task_id: host_tracker,
            provider_task_id: outcome.provider_task_id,
            remote_cancel: remote_cancel_fact(route.protocol),
        })
    }

    /// Polls one agnes video task: Generating / Failed / Completed (only
    /// after the video bytes downloaded through the egress guard,
    /// verified non-empty, and registered behind the cancellation fence).
    /// The poll uses the task's STORED identity — the provider job id for
    /// the primary query, the distinct host tracker as the legacy
    /// fallback, the submit-time model name — never the current binding.
    pub async fn query_video(
        &self,
        task_id: &str,
        deadline_unix_ms: Option<u64>,
    ) -> Result<MediaTaskStatus, OperationFailure> {
        let record = self.tracked_task(task_id, MediaTaskKind::Video)?;
        if !matches!(
            *record.state.lock().expect("media task state"),
            MediaTaskState::Tracking
        ) {
            return Ok(record.status());
        }
        let _permit = self
            .admit("operations:video-query", deadline_unix_ms)
            .await?;
        // RR1 F18: credentials resolve THROUGH THE STORED ROUTE.
        let auth = self
            .credentials
            .resolve(&record.route)
            .await
            .map_err(|error| OperationFailure {
                code: ErrorCode::Unauthorized,
                message: format!(
                    "the video task's submitting provider \
                     ({}/{}) can no longer supply credentials: {error}",
                    record.route.provider, record.route.endpoint
                ),
                retryable: false,
            })?;
        let query = AgnesVideoQuery {
            task_id: record.provider_job_id.clone(),
            legacy_task_id: record.legacy_job_id.clone(),
            model_name: Some(record.model_name.clone()),
        };
        let outcome = dialects::video::execute_agnes_video_query(
            &self.dispatcher,
            &record.route,
            &auth,
            &query,
            deadline_unix_ms,
        )
        .await
        .map_err(OperationFailure::from_pair)?;
        self.settle_poll("video", &record, outcome, deadline_unix_ms)
            .await
    }

    /// Cancels the LOCAL tracking of one async media task. The provider
    /// fact rides the answer: agnes exposes no remote cancel, so the
    /// return states `remote_cancel: "unsupported"` — the remote job (if
    /// still running) keeps running; nothing here claims a remote revoke.
    /// R05 RR1 F19: the transition is the SAME linearized state a settling
    /// poll commits through — a cancellation acknowledged here fences
    /// every later boundary of an in-flight poll; a task that already
    /// COMMITTED a completion keeps its delivered fact (the answer reports
    /// it, never a fabricated cancellation).
    pub fn cancel_media_task(&self, task_id: &str) -> Result<MediaTaskStatus, OperationFailure> {
        let record = {
            let tasks = self.tasks.lock().expect("media task registry");
            tasks
                .get(task_id)
                .cloned()
                .ok_or_else(|| OperationFailure {
                    code: ErrorCode::InvalidMessage,
                    message: format!(
                        "media task {task_id:?} is not tracked by this service instance"
                    ),
                    retryable: false,
                })?
        };
        let mut state = record.state.lock().expect("media task state");
        if state.is_live() {
            *state = MediaTaskState::CancelledLocally;
        }
        Ok(current_status(&state, &record.route.protocol))
    }

    // ── speech synthesis (openai / minimax / dashscope / system) ────────

    /// One speech-synthesis call settled to a REGISTERED audio product.
    pub async fn synthesize_speech(
        &self,
        request: SpeechRequest,
        deadline_unix_ms: Option<u64>,
    ) -> Result<RegisteredProduct, OperationFailure> {
        let (route, auth) = self
            .resolve(ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Speech,
            })
            .await?;
        // R05 RR1 F20: the system-speech platform lane sits UNDER the same
        // unified admission permit as every network family (the pre-fix
        // branch dispatched it BEFORE admission — a queued speech call
        // bypassed the model concurrency budget entirely).
        let _permit = self.admit("operations:speech", deadline_unix_ms).await?;
        // §29.13: system-speech is the macOS /usr/bin/say platform lane.
        if route.protocol == ProtocolFamily::SystemSpeech {
            let product = self
                .system_speech(&route, &request, deadline_unix_ms)
                .await?;
            return Ok(product);
        }
        let plan = dialects::speech::build_speech(&route, &auth, &request)
            .map_err(OperationFailure::from_protocol)?;
        match route.protocol {
            ProtocolFamily::OpenAiAudioSpeech => {
                let (bytes, _content_type) = match self
                    .dispatcher
                    .execute_bytes(&plan, deadline_unix_ms, &auth)
                    .await
                {
                    Ok(done) => done,
                    Err(pair) => {
                        self.record_operation_usage(
                            "speech",
                            &route,
                            ReportedUsage::Unknown,
                            None,
                            CallOutcome::Failed,
                            None,
                        )
                        .await?;
                        return Err(OperationFailure::from_pair(pair));
                    }
                };
                self.record_operation_usage(
                    "speech",
                    &route,
                    ReportedUsage::Unknown,
                    None,
                    CallOutcome::Succeeded,
                    None,
                )
                .await?;
                let mime = dialects::speech::openai_speech_mime_for_format(
                    &dialects::speech::effective_openai_speech_format(request.format.as_deref()),
                )
                .to_string();
                self.register_product("speech", &bytes, &mime)
            }
            ProtocolFamily::MinimaxTts => {
                let body = match self
                    .dispatcher
                    .execute_json(&plan, deadline_unix_ms, &auth)
                    .await
                {
                    Ok(body) => body,
                    Err(pair) => {
                        self.record_operation_usage(
                            "speech",
                            &route,
                            ReportedUsage::Unknown,
                            None,
                            CallOutcome::Failed,
                            None,
                        )
                        .await?;
                        return Err(OperationFailure::from_pair(pair));
                    }
                };
                self.record_operation_usage(
                    "speech",
                    &route,
                    ReportedUsage::Unknown,
                    None,
                    CallOutcome::Succeeded,
                    None,
                )
                .await?;
                let product =
                    dialects::speech::parse_minimax_speech(request.format.as_deref(), &body)
                        .map_err(OperationFailure::from_protocol)?;
                match product {
                    MediaProductRef::Bytes { bytes, mime } => {
                        self.register_product("speech", &bytes, &mime)
                    }
                    MediaProductRef::Url { .. } => Err(OperationFailure {
                        code: ErrorCode::InvalidMessage,
                        message: "minimax speech parse returned a URL product (a dialect \
                                  invariant broke; refusing)"
                            .to_string(),
                        retryable: false,
                    }),
                }
            }
            ProtocolFamily::DashscopeQwenTts => {
                let body = match self
                    .dispatcher
                    .execute_json(&plan, deadline_unix_ms, &auth)
                    .await
                {
                    Ok(body) => body,
                    Err(pair) => {
                        self.record_operation_usage(
                            "speech",
                            &route,
                            ReportedUsage::Unknown,
                            None,
                            CallOutcome::Failed,
                            None,
                        )
                        .await?;
                        return Err(OperationFailure::from_pair(pair));
                    }
                };
                self.record_operation_usage(
                    "speech",
                    &route,
                    ReportedUsage::Unknown,
                    None,
                    CallOutcome::Succeeded,
                    None,
                )
                .await?;
                let product = dialects::speech::parse_dashscope_speech(&body)
                    .map_err(OperationFailure::from_protocol)?;
                self.settle_product("speech", product, deadline_unix_ms)
                    .await
            }
            other => Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: format!(
                    "protocol family {} does not serve speech (wiring bug)",
                    other.config_name()
                ),
                retryable: false,
            }),
        }
    }

    /// The §29.13 platform lane: `/usr/bin/say`, argv-formed, no shell.
    /// R05 RR1 F20: the lane runs UNDER the caller's absolute deadline
    /// (the effective cap is the SMALLER of the remaining budget and the
    /// §29.13 120 s process cap) and spawns through the SUPERVISED
    /// process plane — process group, reaper, `ProcessOwnershipGuard`
    /// (a dropped caller future terminates the process through the
    /// supervisor's chain; tokio `kill_on_drop` is never the guarantee),
    /// the TERM→grace→KILL ladder on the deadline, honest exit facts, and
    /// intermediate-output cleanup on EVERY non-delivering path. The
    /// accounting fact lands with ZERO transport attempts (a local
    /// process is not a provider HTTP call). macOS only — any other
    /// platform is the loud refusal, never a silent substitute
    /// synthesizer; an operation service without the supervised process
    /// plane refuses the lane fail-closed rather than spawning an
    /// unsupervised child.
    async fn system_speech(
        &self,
        route: &ResolvedModelRoute,
        request: &SpeechRequest,
        deadline_unix_ms: Option<u64>,
    ) -> Result<RegisteredProduct, OperationFailure> {
        if !cfg!(target_os = "macos") {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: "system speech is only available on macOS".to_string(),
                retryable: false,
            });
        }
        let input = request.text.trim();
        if input.is_empty() {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: "prompt is required".to_string(),
                retryable: false,
            });
        }
        let Some(supervisor) = self.process_supervisor.as_ref() else {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: "system speech requires the supervised process plane \
                          (ProcessSupervisor); this operation service was built without one — \
                          refusing to spawn an unsupervised local process"
                    .to_string(),
                retryable: false,
            });
        };
        // The effective cap: the caller's remaining budget, never above the
        // §29.13 process cap; an exhausted budget refuses BEFORE spawning.
        let remaining = lingxi_adapters::models::dispatch::remaining_budget_ms(deadline_unix_ms);
        let effective_timeout_ms = match remaining {
            Some(0) => {
                return Err(OperationFailure {
                    code: ErrorCode::BudgetExceeded,
                    message: "system speech outlived the call's total budget before the \
                              local process started"
                        .to_string(),
                    retryable: false,
                });
            }
            remaining => remaining
                .unwrap_or(SYSTEM_SPEECH_TIMEOUT_MS)
                .min(SYSTEM_SPEECH_TIMEOUT_MS),
        };
        let mut entropy = [0u8; 6];
        getrandom::getrandom(&mut entropy).map_err(|err| OperationFailure {
            code: ErrorCode::Internal,
            message: format!("system-speech output entropy failed: {err}"),
            retryable: false,
        })?;
        let tag: String = entropy.iter().map(|b| format!("{b:02x}")).collect();
        let out_path = self.products_root.join(format!("speech-say-{}.m4a", tag));
        // The RAII cleanup of the intermediate say output: armed until the
        // product is REGISTERED (a delivered product is digest-named under
        // the same root; the intermediate file never survives any
        // non-delivering path — timeout, failure, or caller drop).
        struct SayOutputGuard<'a> {
            path: &'a Path,
            armed: bool,
        }
        impl Drop for SayOutputGuard<'_> {
            fn drop(&mut self) {
                if self.armed {
                    let _ = std::fs::remove_file(self.path);
                }
            }
        }
        // Held to the end of the call: the drop removes the intermediate
        // file on EVERY non-delivering path (failure, timeout, caller
        // drop) and the duplicate left by a successful registration.
        #[allow(unused_mut)]
        let mut output_guard = SayOutputGuard {
            path: &out_path,
            armed: true,
        };
        output_guard.armed = true;
        // The incumbent's only argv mapping: -o out [-v voice] [-r wpm]
        // input — no shell, no user-controlled flags (voice/rate are
        // host-validated fields). RR1 F20 hardening: the `--` end-of-
        // options separator rides BEFORE the input — on this platform a
        // message beginning with `-o…` would otherwise be parsed as the
        // say OPTION and redirect the output (verified: `say -o A
        // "-oB"` writes B) — user text is never argv options.
        let mut argv = vec!["/usr/bin/say".to_string(), "-o".to_string()];
        argv.push(out_path.display().to_string());
        if let Some(voice) = request
            .voice
            .as_deref()
            .map(str::trim)
            .filter(|voice| !voice.is_empty())
        {
            argv.push("-v".to_string());
            argv.push(voice.to_string());
        }
        argv.push("--".to_string());
        argv.push(input.to_string());
        let owner = crate::procsupervisor::ProcessOwner {
            principal_kind: "local_user".to_string(),
            principal_subject: "lingxi-service".to_string(),
            session_id: "operations".to_string(),
            run_id: "system-speech".to_string(),
            tool_call_id: lingxi_protocol::ToolCallId::new(format!(
                "system-speech-{}-{tag}",
                lingxi_adapters::models::dispatch::unix_ms_now()
            )),
        };
        let spec = crate::procsupervisor::SpawnSpec {
            argv,
            cwd: self.products_root.clone(),
            env: BTreeMap::new(),
            owner,
            kind: crate::procsupervisor::ProcessKind::OneShot,
            cols: 80,
            rows: 24,
        };
        let spawned = match supervisor.spawn(spec).await {
            Ok(spawned) => spawned,
            Err(failure @ crate::procsupervisor::SpawnFailure::RegistryFull) => {
                return Err(OperationFailure {
                    code: ErrorCode::BudgetExceeded,
                    message: format!(
                        "the supervised process plane is at capacity; system speech was \
                         never dispatched: {failure}"
                    ),
                    retryable: true,
                });
            }
            Err(failure) => {
                return Err(OperationFailure {
                    code: ErrorCode::UpstreamUnavailable,
                    message: format!("cannot spawn /usr/bin/say: {failure}"),
                    retryable: false,
                });
            }
        };
        // Process ownership: the guard terminates the process through the
        // supervisor chain when THIS future is dropped (caller cancel);
        // disarmed once the phase settles below.
        let mut guard = crate::exectools::ProcessOwnershipGuard::new(
            Arc::clone(supervisor),
            spawned.id.clone(),
        );
        let timeout_at =
            tokio::time::Instant::now() + std::time::Duration::from_millis(effective_timeout_ms);
        let phase = tokio::select! {
            biased;
            phase = supervisor.wait_terminal(&spawned.id) => phase,
            _ = tokio::time::sleep_until(timeout_at) => {
                // The §29.13 ladder through the supervisor: TERM → grace →
                // KILL on the process GROUP, honest receipt.
                let receipt = supervisor
                    .terminate(&spawned.id, crate::procsupervisor::TerminationReason::Timeout)
                    .await;
                use crate::procsupervisor::TerminationOutcome;
                let detail = match receipt.outcome {
                    TerminationOutcome::Terminated { fact, .. } => {
                        format!("process terminated ({})", fact.describe())
                    }
                    TerminationOutcome::AlreadyTerminal(phase) => {
                        format!("process had already ended ({phase:?})")
                    }
                    TerminationOutcome::CleanupTimedOut => {
                        "process cleanup timed out (stop unconfirmed)".to_string()
                    }
                    TerminationOutcome::UnknownProcess => {
                        "the process record vanished".to_string()
                    }
                };
                let _ = self
                    .record_operation_usage_with_attempts(
                        "speech",
                        route,
                        ReportedUsage::Unknown,
                        0,
                        None,
                        CallOutcome::Failed,
                        None,
                    )
                    .await;
                return Err(OperationFailure {
                    code: ErrorCode::BudgetExceeded,
                    message: format!(
                        "system speech timed out (cap {effective_timeout_ms} ms from the \
                         call's remaining budget; SIGTERM→SIGKILL applied): {detail}"
                    ),
                    retryable: false,
                });
            }
        };
        guard.disarm();
        let phase = match phase {
            Some(phase) => phase,
            None => {
                return Err(OperationFailure {
                    code: ErrorCode::Internal,
                    message: "the system-speech process record vanished while waiting for \
                              the exit"
                        .to_string(),
                    retryable: false,
                });
            }
        };
        let phase_fact = |phase: &crate::procsupervisor::RecordPhase| match phase {
            crate::procsupervisor::RecordPhase::Running => "still running".to_string(),
            crate::procsupervisor::RecordPhase::Terminating { .. } => "terminating".to_string(),
            crate::procsupervisor::RecordPhase::Exited { fact }
            | crate::procsupervisor::RecordPhase::Terminated { fact, .. } => fact.describe(),
            crate::procsupervisor::RecordPhase::CleanupTimedOut { .. } => {
                "stop unconfirmed (cleanup timed out)".to_string()
            }
        };
        let exit_ok = matches!(
            &phase,
            crate::procsupervisor::RecordPhase::Exited {
                fact: crate::procsupervisor::ExitFact::Code(0),
            }
        ) || matches!(
            &phase,
            crate::procsupervisor::RecordPhase::Terminated {
                fact: crate::procsupervisor::ExitFact::Code(0),
                ..
            }
        );
        if !exit_ok {
            let output_hint = supervisor
                .output_snapshot(&spawned.id)
                .map(|snapshot| String::from_utf8_lossy(&snapshot.window).trim().to_string())
                .unwrap_or_default();
            let _ = self
                .record_operation_usage_with_attempts(
                    "speech",
                    route,
                    ReportedUsage::Unknown,
                    0,
                    None,
                    CallOutcome::Failed,
                    None,
                )
                .await;
            return Err(OperationFailure {
                code: ErrorCode::UpstreamUnavailable,
                message: format!(
                    "system speech process did not deliver ({}{}{}); the intermediate \
                     output was removed",
                    phase_fact(&phase),
                    if output_hint.is_empty() {
                        String::new()
                    } else {
                        format!("; say output: {output_hint}")
                    },
                    ""
                ),
                retryable: false,
            });
        }
        let bytes = std::fs::read(&out_path).map_err(|err| OperationFailure {
            code: ErrorCode::UpstreamUnavailable,
            message: format!("system speech produced no output: {err}"),
            retryable: false,
        })?;
        let product = self.register_product("speech", &bytes, "audio/mp4")?;
        // The intermediate say output was the SOURCE of the registered
        // (digest-named) product; the guard removes the duplicate on drop.
        let _ = self
            .record_operation_usage_with_attempts(
                "speech",
                route,
                ReportedUsage::Unknown,
                0,
                None,
                CallOutcome::Succeeded,
                None,
            )
            .await;
        Ok(product)
    }

    // ── transcription (ASR) ──────────────────────────────────────────────

    /// One transcription call over host-read audio. The volcengine-bigasr
    /// status header check fires on the RESPONSE headers; the text ladder
    /// is the dialect's.
    pub async fn transcribe(
        &self,
        audio: HostAudio,
        language: Option<String>,
        deadline_unix_ms: Option<u64>,
    ) -> Result<dialects::transcribe::TranscriptionOutcome, OperationFailure> {
        if audio.bytes.is_empty() {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: "transcription audio is empty (nothing to recognize)".to_string(),
                retryable: false,
            });
        }
        if audio.bytes.len() > AUDIO_INPUT_MAX_BYTES {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: format!(
                    "transcription audio exceeds the {} byte input cap; refused, never \
                     truncated",
                    AUDIO_INPUT_MAX_BYTES
                ),
                retryable: false,
            });
        }
        let (route, auth) = self.resolve(ModelOperation::SpeechRecognition).await?;
        let _permit = self
            .admit("operations:transcribe", deadline_unix_ms)
            .await?;
        let request = TranscriptionRequest {
            audio: audio.bytes,
            mime: audio.mime,
            filename: audio.filename,
            language,
        };
        let plan = dialects::transcribe::build_transcription(&route, &auth, &request)
            .map_err(OperationFailure::from_protocol)?;
        let (body, headers) = match self
            .dispatcher
            .execute_json_full(&plan, deadline_unix_ms, &auth)
            .await
        {
            Ok(done) => done,
            Err(pair) => {
                self.record_operation_usage(
                    "transcribe",
                    &route,
                    ReportedUsage::Unknown,
                    None,
                    CallOutcome::Failed,
                    None,
                )
                .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        self.record_operation_usage(
            "transcribe",
            &route,
            ReportedUsage::Unknown,
            None,
            CallOutcome::Succeeded,
            None,
        )
        .await?;
        dialects::transcribe::bigasr_status_ok(&headers)
            .map_err(OperationFailure::from_protocol)?;
        dialects::transcribe::parse_transcription(route.protocol, &body)
            .map_err(OperationFailure::from_protocol)
    }
}

fn entropy_to_hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// R05 RR1 F17: reads the file the authorization ACTUALLY scoped — the
/// canonical `ResourceScope.path`, opened through the same no-follow
/// dirfd discipline as the file tools (a component swapped to a symlink
/// after authorization re-resolves the REAL target and re-authorizes it,
/// bounded churn) — with the size cap enforced DURING the read (at most
/// `max_bytes + 1` bytes ever leave the file), never as a full read
/// followed by a refusal. Non-regular sources (FIFOs, devices) are
/// refused outright: an attachment read is a bounded regular-file read.
fn read_authorized_bytes_bounded(
    access: &crate::resourceaccess::ResourceAccess,
    ctx: &lingxi_kernel::RunContext,
    raw_path: &Path,
    cwd: &Path,
    max_bytes: usize,
    what: &str,
) -> Result<Vec<u8>, OperationFailure> {
    const MAX_SYMLINK_CHURN: usize = 8;
    let mut churn = 0usize;
    loop {
        let scope = access
            .authorize(
                ctx.principal.storage_kind(),
                &ctx.principal.storage_subject(),
                ctx.session_id.as_str(),
                raw_path,
                cwd,
                crate::resourceaccess::ResourceOp::Read,
            )
            .map_err(|refusal| OperationFailure {
                code: ErrorCode::Forbidden,
                message: refusal.message.clone(),
                retryable: false,
            })?;
        // Open the CANONICAL authorized path — never the raw argument
        // (which is only meaningful against `cwd`, not the process cwd).
        let parent = scope
            .path
            .parent()
            .unwrap_or_else(|| Path::new("/"))
            .to_path_buf();
        let name = scope
            .path
            .file_name()
            .ok_or_else(|| OperationFailure {
                code: ErrorCode::Forbidden,
                message: format!(
                    "the authorized {what} scope {} has no file name; refusing",
                    scope.path.display()
                ),
                retryable: false,
            })?
            .to_os_string();
        let dir = match crate::filetools::fsio::DirFd::open(&parent) {
            Ok(dir) => dir,
            Err(err) if crate::filetools::fsio::is_symlink_swap(&err) => {
                churn += 1;
                if churn > MAX_SYMLINK_CHURN {
                    return Err(OperationFailure {
                        code: ErrorCode::Forbidden,
                        message: format!(
                            "the authorized {what} path's components kept changing while \
                             opening it (symlink churn); refusing after {churn} re-resolutions"
                        ),
                        retryable: false,
                    });
                }
                continue;
            }
            Err(err) => {
                return Err(OperationFailure {
                    code: ErrorCode::InvalidMessage,
                    message: format!(
                        "cannot open the authorized {what} {}: {err}",
                        scope.path.display()
                    ),
                    retryable: false,
                });
            }
        };
        let file = match dir.open_attachment_nofollow(&name) {
            Some(Ok(file)) => file,
            None => {
                return Err(OperationFailure {
                    code: ErrorCode::InvalidMessage,
                    message: format!(
                        "cannot read the authorized {what} {}: the file vanished after \
                         authorization",
                        scope.path.display()
                    ),
                    retryable: false,
                });
            }
            Some(Err(err)) => {
                let swap = crate::filetools::fsio::is_symlink_swap(&err);
                if !swap {
                    return Err(OperationFailure {
                        code: ErrorCode::InvalidMessage,
                        message: format!(
                            "cannot read the authorized {what} {}: {err}",
                            scope.path.display()
                        ),
                        retryable: false,
                    });
                }
                churn += 1;
                if churn > MAX_SYMLINK_CHURN {
                    return Err(OperationFailure {
                        code: ErrorCode::Forbidden,
                        message: format!(
                            "the authorized {what} target kept being swapped to a symlink; \
                             refusing after {churn} re-resolutions"
                        ),
                        retryable: false,
                    });
                }
                // A leaf swapped to a symlink AFTER authorization: loop to
                // re-resolve the REAL target and re-authorize it.
                continue;
            }
        };
        // The read is bounded at the HANDLE: a non-regular source refuses,
        // and at most `max_bytes + 1` bytes are ever read (the +1 proves
        // the cap without trusting a pre-read stat on a growing file).
        let metadata = file.metadata().map_err(|err| OperationFailure {
            code: ErrorCode::InvalidMessage,
            message: format!(
                "cannot stat the authorized {what} {}: {err}",
                scope.path.display()
            ),
            retryable: false,
        })?;
        if !metadata.is_file() {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: format!(
                    "the authorized {what} {} is not a regular file; an attachment read \
                     is a bounded regular-file read, refusing instead of reading a \
                     non-regular source",
                    scope.path.display()
                ),
                retryable: false,
            });
        }
        let mut file = file;
        use std::io::Read as _;
        let mut bytes = Vec::new();
        let read = std::io::Read::take(
            std::io::BufReader::new(file.by_ref()),
            (max_bytes + 1) as u64,
        )
        .read_to_end(&mut bytes)
        .map_err(|err| OperationFailure {
            code: ErrorCode::InvalidMessage,
            message: format!(
                "cannot read the authorized {what} {}: {err}",
                scope.path.display()
            ),
            retryable: false,
        })?;
        if read > max_bytes {
            return Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: format!(
                    "{what} exceeds the {max_bytes} byte input cap; refused, never truncated"
                ),
                retryable: false,
            });
        }
        return Ok(bytes);
    }
}

/// Reads one host audio file for transcription through the authorized
/// access surface (C03: authorized resource reading; the size cap refuses
/// loudly, never truncates).
pub fn read_host_audio(
    access: &crate::resourceaccess::ResourceAccess,
    ctx: &lingxi_kernel::RunContext,
    path: &Path,
    cwd: &Path,
) -> Result<HostAudio, OperationFailure> {
    let canonical = access.resolve(path, cwd).unwrap_or_else(|| cwd.join(path));
    let bytes = read_authorized_bytes_bounded(
        access,
        ctx,
        path,
        cwd,
        AUDIO_INPUT_MAX_BYTES,
        "transcription audio",
    )?;
    let filename = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "audio.wav".to_string());
    let mime = audio_mime_for_extension(
        canonical
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default(),
    );
    Ok(HostAudio {
        bytes,
        mime,
        filename,
    })
}

/// The wire MIME of an audio extension (the incumbent table; the default
/// is `audio/wav`).
pub fn audio_mime_for_extension(extension: &str) -> String {
    match extension.to_ascii_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "webm" => "audio/webm",
        _ => "audio/wav",
    }
    .to_string()
}

/// Reads one reference image for a media generation call through the
/// authorized access surface: bounded ([`IMAGE_INPUT_MAX_BYTES`]), the
/// wire MIME derived from the path's extension per the incumbent
/// `imageMime` table (png/jpg/jpeg/webp, else `image/png`).
pub fn read_image_reference(
    access: &crate::resourceaccess::ResourceAccess,
    ctx: &lingxi_kernel::RunContext,
    path: &Path,
    cwd: &Path,
) -> Result<lingxi_adapters::models::operations::ImageReference, OperationFailure> {
    let canonical = access.resolve(path, cwd).unwrap_or_else(|| cwd.join(path));
    let bytes = read_authorized_bytes_bounded(
        access,
        ctx,
        path,
        cwd,
        IMAGE_INPUT_MAX_BYTES,
        "reference image",
    )?;
    let mime = match canonical
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
    .to_string();
    let filename = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "reference.png".to_string());
    Ok(lingxi_adapters::models::operations::ImageReference::Bytes {
        bytes,
        mime,
        filename,
    })
}

#[cfg(test)]
mod rr1_f18_registry_tests {
    //! The bounded-registry invariant (F18: the host record store is
    //! bounded memory) — exercised on the REAL registry through the real
    //! tracking path (no loopback HTTP needed: tracking itself is pure
    //! host state).

    use super::*;

    fn service() -> OperationService {
        let plane_json = r#"{
            "providers": {
                "text_a": {
                    "protocol": "openai-completions",
                    "endpoint": "http://127.0.0.1:9",
                    "auth": {"kind": "apiKey", "apiKey": "sk-UNIT-SECRET"}
                }
            },
            "models": {
                "chat": {"provider": "text_a", "model": "unit-model"}
            }
        }"#;
        let plane =
            lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(plane_json)
                .expect("plane");
        let runtime_dir = std::env::temp_dir().join(format!(
            "lingxi-ops-unit-rt-{}-{}",
            std::process::id(),
            lingxi_adapters::models::dispatch::unix_ms_now()
        ));
        let products_root = runtime_dir.join("products");
        std::fs::create_dir_all(&products_root).expect("products root");
        let credentials = Arc::new(
            crate::credentials::CredentialService::bootstrap(
                &plane,
                &runtime_dir,
                Arc::new(crate::inject::SystemClock),
            )
            .expect("credentials"),
        );
        OperationService::new(
            Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane)),
            credentials,
            Arc::new(crate::quotas::QuotaManager::new(
                crate::quotas::QuotaLimits::default(),
            )),
            vec![],
            products_root,
        )
        .expect("service")
    }

    fn route_for(service: &OperationService) -> ResolvedModelRoute {
        use lingxi_kernel::model_exchange::ModelGatewayPort as _;
        // The unit plane has no video binding; the CHAT route serves as
        // the identity carrier (tracking itself never dispatches).
        service
            .gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("chat route")
    }

    #[test]
    fn registry_refuses_live_overflow_and_evicts_terminal_records() {
        let service = service();
        let route = route_for(&service);
        for seq in 0..MEDIA_TASK_REGISTRY_CAP {
            let tracker = service
                .track_media_task(
                    MediaTaskKind::Video,
                    format!("bare-{seq}"),
                    format!("provider-{seq}"),
                    None,
                    route.clone(),
                )
                .unwrap_or_else(|failure| panic!("live task {seq} tracks: {failure:?}"));
            assert_eq!(tracker, format!("bare-{seq}"));
        }
        // The registry is full of LIVE tasks: the next submit refuses.
        let refusal = service
            .track_media_task(
                MediaTaskKind::Video,
                "overflow".to_string(),
                "provider-overflow".to_string(),
                None,
                route.clone(),
            )
            .expect_err("a registry full of live tasks refuses");
        assert_eq!(refusal.code, ErrorCode::BudgetExceeded);
        assert!(
            refusal.message.contains("cap"),
            "the refusal names the registry fact: {refusal:?}"
        );
        // Settle one task (terminal receipt) — the next submit evicts the
        // terminal record and tracks.
        {
            let tasks = service.tasks.lock().expect("registry");
            let record = Arc::clone(tasks.get("bare-0").expect("tracked"));
            drop(tasks);
            *record.state.lock().expect("state") =
                MediaTaskState::Completed(vec![RegisteredProduct {
                    resource_id: "unit".to_string(),
                    path: PathBuf::from("/tmp/unit-product"),
                    mime: "video/mp4".to_string(),
                    size_bytes: 1,
                    sha256: "00".to_string(),
                }]);
        }
        let tracker = service
            .track_media_task(
                MediaTaskKind::Video,
                "after-settle".to_string(),
                "provider-after".to_string(),
                None,
                route,
            )
            .expect("a terminal record is evicted for the new live task");
        assert_eq!(tracker, "after-settle");
        assert_eq!(
            service.tasks.lock().expect("registry").len(),
            MEDIA_TASK_REGISTRY_CAP
        );
    }
}
