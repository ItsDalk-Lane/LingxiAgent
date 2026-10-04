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
//! argv-formed, no shell, a 120 s process cap, SIGTERM → 1.5 s → SIGKILL,
//! a non-empty product check. `system-speech-recognition` resolves loudly
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
use lingxi_kernel::usage::ReportedUsage;
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
/// The system-speech process cap (§29.13).
pub const SYSTEM_SPEECH_TIMEOUT_MS: u64 = 120_000;
/// The system-speech SIGTERM→SIGKILL grace (§29.13).
pub const SYSTEM_SPEECH_KILL_GRACE_MS: u64 = 1_500;

/// The service-side operation entry (§29.9).
pub struct OperationService {
    gateway: Arc<ConfigModelGateway>,
    credentials: Arc<dyn ProviderCredentialPort>,
    quotas: Arc<QuotaManager>,
    dispatcher: OperationDispatcher,
    egress: Arc<EgressGuard>,
    products_root: PathBuf,
    /// The local tracking state of async media tasks (in-memory by design:
    /// media-task persistence is the registered R07 business surface).
    tasks: std::sync::Mutex<BTreeMap<String, LocalTaskState>>,
    /// R05-T07: the usage-ledger sink — one row per operation-plane
    /// provider request. `None` keeps the pre-T07 shape (no accounting
    /// surface); the bootstrap installs the ledger sink.
    usage_sink: Option<Arc<dyn OperationUsageSink>>,
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
            let record = lingxi_kernel::usage::ModelCallUsageRecord {
                session_id: None,
                run_id: None,
                attempt: None,
                model_call_id,
                purpose: fact.operation.clone(),
                origin: "operation".to_string(),
                parent_run_id: None,
                cause_ref: None,
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
                // The operation plane sends exactly one physical request
                // per call today (no refresh-retry on this dispatcher).
                transport_attempts: 1,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalTaskState {
    Tracking,
    CancelledLocally,
}

/// The per-family remote-cancel capability fact (only agnes exists today;
/// it exposes no cancel API).
fn remote_cancel_fact(protocol: ProtocolFamily) -> &'static str {
    match protocol {
        ProtocolFamily::AgnesVideos => "unsupported",
        _ => "unsupported",
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
        std::fs::create_dir_all(&products_root).map_err(|err| OperationFailure {
            code: ErrorCode::Internal,
            message: format!(
                "cannot create the media product root {}: {err}",
                products_root.display()
            ),
            retryable: false,
        })?;
        let egress = EgressGuard::from_provider_endpoints(&provider_endpoints)
            .map_err(OperationFailure::from_protocol)?;
        let dispatcher = OperationDispatcher::with_timeouts(HttpTimeouts::default())
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
        })
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
    async fn record_operation_usage(
        &self,
        operation: &str,
        route: &ResolvedModelRoute,
        usage_report: lingxi_kernel::usage::ReportedUsage,
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
        })
        .await
        .map_err(|failure| OperationFailure {
            code: ErrorCode::Internal,
            message: format!("the usage ledger refused the {operation} accounting row: {failure}"),
            retryable: false,
        })
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
    /// as the chat loop and the worker callbacks).
    async fn admit(&self, lane: &str) -> Result<crate::quotas::QuotaPermit, OperationFailure> {
        self.quotas
            .acquire(QuotaResource::Model, lane, "model-operations")
            .await
            .map_err(|failure| OperationFailure {
                code: ErrorCode::BudgetExceeded,
                message: format!("model-plane admission refused: {failure}"),
                retryable: false,
            })
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

    // ── embedding / rerank (the text operation plane) ────────────────────

    /// One embedding call: validate → route → admit → dispatch → parse.
    pub async fn embed(
        &self,
        request: dialects::embedding::EmbeddingRequest,
        deadline_unix_ms: Option<u64>,
    ) -> Result<dialects::embedding::EmbeddingOutcome, OperationFailure> {
        dialects::embedding::validate_embedding_request(&request)
            .map_err(OperationFailure::from_protocol)?;
        let (route, auth) = self.resolve(ModelOperation::Embedding).await?;
        let _permit = self.admit("operations:embedding").await?;
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
                self.record_operation_usage("embedding", &route, ReportedUsage::Unknown)
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
                self.record_operation_usage("embedding", &route, report)
                    .await?;
                Ok(outcome)
            }
            Err(failure) => {
                self.record_operation_usage("embedding", &route, ReportedUsage::Unknown)
                    .await?;
                Err(failure)
            }
        }
    }

    /// One rerank call: validate → route → admit → dispatch → parse.
    pub async fn rerank(
        &self,
        request: RerankRequest,
        deadline_unix_ms: Option<u64>,
    ) -> Result<dialects::rerank::RerankOutcome, OperationFailure> {
        let top_n = dialects::rerank::validate_rerank_request(&request)
            .map_err(OperationFailure::from_protocol)?;
        let (route, auth) = self.resolve(ModelOperation::Rerank).await?;
        let _permit = self.admit("operations:rerank").await?;
        let plan = dialects::rerank::build_rerank(&route, &auth, &request)
            .map_err(OperationFailure::from_protocol)?;
        let body = match self
            .dispatcher
            .execute_json(&plan, deadline_unix_ms, &auth)
            .await
        {
            Ok(body) => body,
            Err(pair) => {
                self.record_operation_usage("rerank", &route, ReportedUsage::Unknown)
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
                self.record_operation_usage("rerank", &route, report)
                    .await?;
                Ok(outcome)
            }
            Err(failure) => {
                self.record_operation_usage("rerank", &route, ReportedUsage::Unknown)
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
        let _permit = self.admit("operations:image").await?;
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
                self.record_operation_usage("image", &route, ReportedUsage::Unknown)
                    .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        // R05-T07: the request fact is recorded once the response arrived
        // (per-item billing families report no token usage on the wire).
        self.record_operation_usage("image", &route, ReportedUsage::Unknown)
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
                self.tasks
                    .lock()
                    .expect("media task registry")
                    .insert(task_id.clone(), LocalTaskState::Tracking);
                Ok(ImageGenerationOutcome::Pending { task_id })
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
                self.record_operation_usage("image", route, ReportedUsage::Unknown)
                    .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        self.record_operation_usage("image", route, ReportedUsage::Unknown)
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
    /// out only while the task is locally tracked; a Completed answer
    /// follows download + verify + registration (a job id is never a
    /// product).
    pub async fn query_media_task(
        &self,
        task_id: &str,
        deadline_unix_ms: Option<u64>,
    ) -> Result<MediaTaskStatus, OperationFailure> {
        // The task's route family is re-derived from the CURRENT image
        // binding (the registry keys are opaque to the caller).
        let (route, auth) = self
            .resolve(ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Image,
            })
            .await?;
        {
            let tasks = self.tasks.lock().expect("media task registry");
            match tasks.get(task_id) {
                None => {
                    return Err(OperationFailure {
                        code: ErrorCode::InvalidMessage,
                        message: format!(
                            "media task {task_id:?} is not tracked by this service instance \
                             (a task id from another process or a fabricated id is not a \
                             pollable fact)"
                        ),
                        retryable: false,
                    });
                }
                Some(LocalTaskState::CancelledLocally) => {
                    return Ok(MediaTaskStatus::CancelledLocally {
                        remote_cancel: remote_cancel_fact(route.protocol),
                    });
                }
                Some(LocalTaskState::Tracking) => {}
            }
        }
        let _permit = self.admit("operations:image-query").await?;
        let plan = dialects::image::build_dashscope_image_query(&route, &auth, task_id);
        let body = self
            .dispatcher
            .execute_json(&plan, deadline_unix_ms, &auth)
            .await
            .map_err(OperationFailure::from_pair)?;
        let outcome = dialects::image::parse_dashscope_image_query(&body)
            .map_err(OperationFailure::from_protocol)?;
        self.settle_poll("image", task_id, outcome, deadline_unix_ms)
            .await
    }

    /// Maps ONE parsed poll outcome onto the honest state machine,
    /// retiring the local tracking entry on terminal states.
    async fn settle_poll(
        &self,
        kind: &str,
        task_id: &str,
        outcome: TaskPollOutcome,
        deadline_unix_ms: Option<u64>,
    ) -> Result<MediaTaskStatus, OperationFailure> {
        match outcome {
            TaskPollOutcome::Pending => Ok(MediaTaskStatus::Generating),
            TaskPollOutcome::Failed { reason } => {
                self.tasks
                    .lock()
                    .expect("media task registry")
                    .remove(task_id);
                Ok(MediaTaskStatus::Failed { reason })
            }
            TaskPollOutcome::Done { products } => {
                // Completed ONLY after download + verify + registration.
                let products = self
                    .settle_products(kind, products, deadline_unix_ms)
                    .await?;
                self.tasks
                    .lock()
                    .expect("media task registry")
                    .remove(task_id);
                Ok(MediaTaskStatus::Completed { products })
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
        let _permit = self.admit("operations:video").await?;
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
                self.record_operation_usage("video", &route, ReportedUsage::Unknown)
                    .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        self.record_operation_usage("video", &route, ReportedUsage::Unknown)
            .await?;
        let outcome =
            dialects::video::parse_video_submit(&body).map_err(OperationFailure::from_protocol)?;
        self.tasks
            .lock()
            .expect("media task registry")
            .insert(outcome.task_id.clone(), LocalTaskState::Tracking);
        Ok(MediaJobAccepted {
            task_id: outcome.task_id,
            provider_task_id: outcome.provider_task_id,
            remote_cancel: remote_cancel_fact(route.protocol),
        })
    }

    /// Polls one agnes video task: Generating / Failed / Completed (only
    /// after the video bytes downloaded through the egress guard,
    /// verified non-empty, and registered).
    pub async fn query_video(
        &self,
        task_id: &str,
        deadline_unix_ms: Option<u64>,
    ) -> Result<MediaTaskStatus, OperationFailure> {
        let (route, auth) = self
            .resolve(ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Video,
            })
            .await?;
        {
            let tasks = self.tasks.lock().expect("media task registry");
            match tasks.get(task_id) {
                None => {
                    return Err(OperationFailure {
                        code: ErrorCode::InvalidMessage,
                        message: format!(
                            "video task {task_id:?} is not tracked by this service instance"
                        ),
                        retryable: false,
                    });
                }
                Some(LocalTaskState::CancelledLocally) => {
                    return Ok(MediaTaskStatus::CancelledLocally {
                        remote_cancel: remote_cancel_fact(route.protocol),
                    });
                }
                Some(LocalTaskState::Tracking) => {}
            }
        }
        let _permit = self.admit("operations:video-query").await?;
        let query = AgnesVideoQuery {
            task_id: task_id.to_string(),
            legacy_task_id: None,
            model_name: None,
        };
        let outcome = dialects::video::execute_agnes_video_query(
            &self.dispatcher,
            &route,
            &auth,
            &query,
            deadline_unix_ms,
        )
        .await
        .map_err(OperationFailure::from_pair)?;
        self.settle_poll("video", task_id, outcome, deadline_unix_ms)
            .await
    }

    /// Cancels the LOCAL tracking of one async media task. The provider
    /// fact rides the answer: agnes exposes no remote cancel, so the
    /// return states `remote_cancel: "unsupported"` — the remote job (if
    /// still running) keeps running; nothing here claims a remote revoke.
    pub fn cancel_media_task(&self, task_id: &str) -> Result<MediaTaskStatus, OperationFailure> {
        let mut tasks = self.tasks.lock().expect("media task registry");
        match tasks.get_mut(task_id) {
            None => Err(OperationFailure {
                code: ErrorCode::InvalidMessage,
                message: format!("media task {task_id:?} is not tracked by this service instance"),
                retryable: false,
            }),
            Some(state) => {
                *state = LocalTaskState::CancelledLocally;
                Ok(MediaTaskStatus::CancelledLocally {
                    remote_cancel: "unsupported",
                })
            }
        }
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
        // §29.13: system-speech is the macOS /usr/bin/say platform lane.
        if route.protocol == ProtocolFamily::SystemSpeech {
            return self.system_speech(&request, deadline_unix_ms).await;
        }
        let _permit = self.admit("operations:speech").await?;
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
                        self.record_operation_usage("speech", &route, ReportedUsage::Unknown)
                            .await?;
                        return Err(OperationFailure::from_pair(pair));
                    }
                };
                self.record_operation_usage("speech", &route, ReportedUsage::Unknown)
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
                        self.record_operation_usage("speech", &route, ReportedUsage::Unknown)
                            .await?;
                        return Err(OperationFailure::from_pair(pair));
                    }
                };
                self.record_operation_usage("speech", &route, ReportedUsage::Unknown)
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
                        self.record_operation_usage("speech", &route, ReportedUsage::Unknown)
                            .await?;
                        return Err(OperationFailure::from_pair(pair));
                    }
                };
                self.record_operation_usage("speech", &route, ReportedUsage::Unknown)
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

    /// The §29.13 platform lane: `/usr/bin/say`, argv-formed, no shell,
    /// 120 s cap, SIGTERM → 1.5 s → SIGKILL, a non-empty product check.
    /// macOS only — any other platform is the loud refusal, never a
    /// silent substitute synthesizer.
    async fn system_speech(
        &self,
        request: &SpeechRequest,
        _deadline_unix_ms: Option<u64>,
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
        let mut entropy = [0u8; 6];
        getrandom::getrandom(&mut entropy).map_err(|err| OperationFailure {
            code: ErrorCode::Internal,
            message: format!("system-speech output entropy failed: {err}"),
            retryable: false,
        })?;
        let tag: String = entropy.iter().map(|b| format!("{b:02x}")).collect();
        let out_path = self.products_root.join(format!("speech-say-{}.m4a", tag));
        // The incumbent's only argv mapping: -o out [-v voice] [-r wpm]
        // input — no shell, no user-controlled flags (voice/rate are
        // host-validated fields, the input rides LAST).
        let mut argv = vec!["-o".to_string(), out_path.display().to_string()];
        if let Some(voice) = request
            .voice
            .as_deref()
            .map(str::trim)
            .filter(|voice| !voice.is_empty())
        {
            argv.push("-v".to_string());
            argv.push(voice.to_string());
        }
        argv.push(input.to_string());
        let mut command = tokio::process::Command::new("/usr/bin/say");
        command
            .args(&argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(|err| OperationFailure {
            code: ErrorCode::UpstreamUnavailable,
            message: format!("cannot spawn /usr/bin/say: {err}"),
            retryable: false,
        })?;
        let deadline = std::time::Duration::from_millis(SYSTEM_SPEECH_TIMEOUT_MS);
        let status = match tokio::time::timeout(deadline, child.wait()).await {
            Ok(status) => status.map_err(|err| OperationFailure {
                code: ErrorCode::UpstreamUnavailable,
                message: format!("system speech process failed: {err}"),
                retryable: false,
            })?,
            Err(_) => {
                // The §29.13 kill discipline: SIGTERM, then SIGKILL after
                // the grace; the failure says TIMEOUT explicitly. The
                // backstop kill is fire-and-forget — the process is not
                // reaped again beyond the grace wait (a stuck reap is not
                // masked into a success).
                #[cfg(unix)]
                {
                    if let Some(pid) = child.id() {
                        unsafe {
                            libc::kill(pid as i32, libc::SIGTERM);
                        }
                    }
                }
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(SYSTEM_SPEECH_KILL_GRACE_MS),
                    child.wait(),
                )
                .await;
                let _ = child.start_kill();
                return Err(OperationFailure {
                    code: ErrorCode::UpstreamUnavailable,
                    message: format!(
                        "system speech timed out (process cap {SYSTEM_SPEECH_TIMEOUT_MS} ms; \
                         SIGTERM→SIGKILL applied)"
                    ),
                    retryable: false,
                });
            }
        };
        if !status.success() {
            return Err(OperationFailure {
                code: ErrorCode::UpstreamUnavailable,
                message: format!("system speech process exited {status}"),
                retryable: false,
            });
        }
        let bytes = std::fs::read(&out_path).map_err(|err| OperationFailure {
            code: ErrorCode::UpstreamUnavailable,
            message: format!("system speech produced no output: {err}"),
            retryable: false,
        })?;
        let product = self.register_product("speech", &bytes, "audio/mp4")?;
        // The intermediate say output IS the registered product file;
        // remove the duplicate only if the paths differ (they do by
        // construction — the registered copy is digest-named).
        if product.path != out_path {
            let _ = std::fs::remove_file(&out_path);
        }
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
        let _permit = self.admit("operations:transcribe").await?;
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
                self.record_operation_usage("transcribe", &route, ReportedUsage::Unknown)
                    .await?;
                return Err(OperationFailure::from_pair(pair));
            }
        };
        self.record_operation_usage("transcribe", &route, ReportedUsage::Unknown)
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

/// Reads one host audio file for transcription through the authorized
/// access surface (C03: authorized resource reading; the size cap refuses
/// loudly, never truncates).
pub fn read_host_audio(
    access: &crate::resourceaccess::ResourceAccess,
    ctx: &lingxi_kernel::RunContext,
    path: &Path,
    cwd: &Path,
) -> Result<HostAudio, OperationFailure> {
    let _scope = access
        .authorize(
            ctx.principal.storage_kind(),
            &ctx.principal.storage_subject(),
            ctx.session_id.as_str(),
            path,
            cwd,
            crate::resourceaccess::ResourceOp::Read,
        )
        .map_err(|refusal| OperationFailure {
            code: ErrorCode::Forbidden,
            message: refusal.message.clone(),
            retryable: false,
        })?;
    let bytes = std::fs::read(path).map_err(|err| OperationFailure {
        code: ErrorCode::InvalidMessage,
        message: format!("cannot read the audio file {}: {err}", path.display()),
        retryable: false,
    })?;
    if bytes.len() > AUDIO_INPUT_MAX_BYTES {
        return Err(OperationFailure {
            code: ErrorCode::InvalidMessage,
            message: format!(
                "transcription audio exceeds the {} byte input cap; refused, never truncated",
                AUDIO_INPUT_MAX_BYTES
            ),
            retryable: false,
        });
    }
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "audio.wav".to_string());
    let mime = audio_mime_for_extension(
        path.extension()
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
    let _scope = access
        .authorize(
            ctx.principal.storage_kind(),
            &ctx.principal.storage_subject(),
            ctx.session_id.as_str(),
            path,
            cwd,
            crate::resourceaccess::ResourceOp::Read,
        )
        .map_err(|refusal| OperationFailure {
            code: ErrorCode::Forbidden,
            message: refusal.message.clone(),
            retryable: false,
        })?;
    let bytes = std::fs::read(path).map_err(|err| OperationFailure {
        code: ErrorCode::InvalidMessage,
        message: format!("cannot read the reference image {}: {err}", path.display()),
        retryable: false,
    })?;
    if bytes.len() > IMAGE_INPUT_MAX_BYTES {
        return Err(OperationFailure {
            code: ErrorCode::InvalidMessage,
            message: format!(
                "reference image exceeds the {} byte input cap; refused, never truncated",
                IMAGE_INPUT_MAX_BYTES
            ),
            retryable: false,
        });
    }
    let mime = match path
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
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "reference.png".to_string());
    Ok(lingxi_adapters::models::operations::ImageReference::Bytes {
        bytes,
        mime,
        filename,
    })
}
