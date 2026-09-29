//! lingxi-service — Rust 独立服务与组合根（R02-T01 建立，R02-T02 扩展配置/
//! 路径/实例，R02-T03 加入 HTTP/WS 认证与资源范围）。
//!
//! 契约锚点（任务书 `02_目标架构与强制契约.md` §1/§2，R02-T01/T02/T03）：
//! - 本 crate 是进程组合根：配置、传输（HTTP+WS）与 port 注入都在这里发生；
//!   领域（lingxi-kernel）不直接访问 HTTP、UI、环境变量或数据库。
//!   由 `docs/rust-tauri/R01/r01_t01_check_ownership.py` 对
//!   `docs/rust-tauri/R01/DEPENDENCY_RULES.json` 机械执法
//!   （DEP-07 全 workspace 桌面禁令 + DEP-08 kernel 禁基础设施依赖与
//!   `std::env`/`env::var` 源码 token——环境变量读取只在组合根发生）。
//! - 数据根优先级（R02-T02，模块 [`config`]）：
//!   `--test-mode` > `--home` > `LINGXI_HOME` > `--config` 文件 `home`；
//!   无来源即拒启，不存在静默落进真实用户目录的默认值。
//!   规范化目录/权限/原子写见 [`paths`]；实例身份与本地单写者锁见
//!   [`instance`]（锁是唯一存活权威，PID 只作诊断，绝不参与判定）。
//! - 认证与资源范围（R02-T03，模块 [`auth`]/[`transport`]/[`ws`]）：
//!   从第一条业务路由起先过传输守卫（Host/Origin/限速）再过共享认证授权
//!   服务；loopback 是默认网络形态，LAN 仅显式 `--network-mode lan`；
//!   因为是 127.0.0.1 就免认证是被禁止的。身份只在 [`auth::AuthService`]
//!   的令牌验证边界创建，业务载荷里的身份字段一律不可信。
//! - 版本事实单一来源：wire 协议版本与 data epoch 取自 lingxi-protocol
//!   常量，本 crate 不复制第二份（实例记录快照同一来源）。

pub mod approval;
pub mod auth;
pub mod cancel;
pub mod config;
pub mod dedup;
pub mod epoch;
pub mod events;
pub mod inject;
pub mod instance;
pub mod limits;
pub mod logging;
mod management;
pub mod paths;
pub mod quotas;
pub mod redaction;
pub mod runs;
mod security_audit;
pub mod serve;
pub mod session_supervisor;
pub mod sessions;
pub mod shutdown;
mod static_web;
pub mod task_supervisor;
pub mod transport;
pub mod ws;

pub use approval::{ApprovalDecision, ApprovalGate, ApprovalRequest};
pub use auth::{
    authorize as authorize_route, classify_route, scope_allows, AuthDenial, AuthService,
    AuthSetupError, AuthzDenial, CredentialKind, IssuedDeviceCredential, Principal, PrincipalKind,
    RoutePolicy, TrustState, LOCAL_OWNER_USER_ID,
};
pub use cancel::{
    CancelBudget, CancelPhase, CancelPolicy, CancelRegistry, CancelScope, FireOutcome,
    RunCancelEntry, ScopeKind,
};
pub use config::{
    parse_cli, read_config_home, resolve_effective_home, CliOptions, ConfigError, HomeSource,
    IgnoredHomeSource, ResolvedHome, HOME_ENV_VAR,
};
pub use dedup::{
    DedupDecision, DedupKey, DedupRegistryFull, SubmissionDedup, DEFAULT_DEDUP_CAP,
    MAX_REQUEST_ID_LEN,
};
pub use epoch::{
    coordinate_data_epoch_startup, read_journal, read_stamp, render_block, EpochGateBlock,
    EpochStamp, GateDecision, JournalRead, StampFormat, StampRead, TransitionJournal,
    EPOCH_BLOCKED_MARKER, EPOCH_TRANSITION_INCOMPLETE_MARKER, JOURNAL_FILE_NAME, STAMP_FILE_NAME,
};
pub use events::{
    control_frame_of_detach, control_snapshot_required_json, control_subscribed_json, DetachReason,
    EventCut, EventHub, EventLimits, EventService, HubStats, SnapshotRequired, SubscribeCursor,
    SubscribeOutcome, SubscribePageError, SubscribeReject, SubscriberCapKind, SubscriberStats,
    SubscriptionFrame, SubscriptionGuard,
};
pub use inject::{
    ManualClock, RandomRequestIdGen, RequestIdGen, SequentialRequestIdGen, ServiceClock,
    SystemClock,
};
pub use instance::{
    acquire, probe_peer, InstanceGuard, InstanceIdentity, InstanceLockError, InstanceRecord,
    PeerProbe, SINGLE_WRITER_BLOCKED_MARKER, STALE_RECORD_MARKER,
};
pub use lingxi_adapters::storage::StoreOptions;
pub use logging::{
    init_tracing, LogRotationConfig, LogRouter, DEFAULT_LOG_MAX_BYTES, DEFAULT_LOG_MAX_FILES,
};
pub use paths::{prepare_layout, DataRootLayout};
pub use quotas::{
    LayeredQuotaLimits, QuotaFailure, QuotaFailureKind, QuotaLayer, QuotaLimits, QuotaManager,
    QuotaPermit, QuotaResource,
};
pub use redaction::{redact_line, redact_text};
pub use runs::{DriveError, RunDriveLimits, RunSupervisor};
pub use session_supervisor::{
    BusyGateError, SessionConcurrencyLimits, SessionLease, SessionSupervisor, SteerError,
    SteerOutcome, SteeringInbox, SubmissionKind,
};
pub use sessions::{
    CancelRunOutcome, ExecuteAccepted, ExecuteRequest, ExecuteSubmission, RunSummary,
    SessionAccess, SessionBackend, SessionExecuteError, SessionFacts, SessionStore, SessionView,
};
pub use shutdown::{
    graceful_shutdown, WsSessionGuard, WsShutdown, DEFAULT_SHUTDOWN_TIMEOUT_MS,
    SHUTDOWN_TIMEOUT_MARKER,
};
pub use task_supervisor::{
    ChildHandle, CleanupReport, SpawnRejected, TaskExit, TaskKind, TaskRef, TaskSupervisor,
};
pub use transport::{check_origin, infer_connection_kind, ConnectionKind, NetworkMode};
pub use ws::{
    WsTicketService, WS_CLOSE_FORBIDDEN, WS_CLOSE_TRY_AGAIN_LATER, WS_CLOSE_UNAUTHORIZED,
};

use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lingxi_adapters::storage::{RunDatabase, RUNS_DB_FILE_NAME};
use lingxi_kernel::ports::StorageError;
use lingxi_protocol::handshake::{
    negotiate_protocol, ClientHello, ServerHello, WIRE_PROTOCOL_MAX_SUPPORTED,
    WIRE_PROTOCOL_MIN_SUPPORTED, WIRE_PROTOCOL_NAME,
};
use lingxi_protocol::{canon, ContractVersions, ErrorCode, ProtocolError};
use serde::Serialize;

/// Server identity for diagnostics (handshake `serverKind` vocabulary).
pub const SERVER_KIND: &str = "lingxi-service";

/// Service implementation version (informational; the wire protocol version
/// axis lives in [`lingxi_protocol::handshake`]).
pub fn server_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Explicit, fully-resolved service configuration.
///
/// `data_home` is the effective root chosen by the documented precedence
/// ([`config::resolve_effective_home`]); `home_source` records where it
/// came from so the safe log and readiness line can always answer "why is
/// this the root". The invariant that survives every stage: the effective
/// root is explicit here, never a silent default into a real user
/// directory.
///
/// `network_mode` (R02-T03): the default is loopback; LAN exposure only
/// through the explicit `--network-mode lan` flag, and a loopback-mode
/// configuration with a non-loopback bind is a loud error, never a silent
/// LAN bind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    /// Socket address to listen on. Defaults to loopback with an ephemeral
    /// port; LAN binding requires explicit `--network-mode lan`.
    pub bind_addr: SocketAddr,
    /// Absolute path of the isolated service data root (as given by the
    /// winning source; canonicalization happens in [`paths::prepare_layout`]).
    pub data_home: std::path::PathBuf,
    /// Which source supplied `data_home` (diagnostics; see [`HomeSource`]).
    pub home_source: HomeSource,
    /// Network exposure: loopback (default) or explicitly configured LAN.
    pub network_mode: NetworkMode,
    /// Graceful-shutdown deadline per phase in milliseconds (R02-T06);
    /// CLI `--shutdown-timeout-ms` or the production default.
    pub shutdown_timeout_ms: u64,
}

impl ServiceConfig {
    /// Default bind target: loopback with an ephemeral port.
    pub const DEFAULT_BIND: &'static str = "127.0.0.1:0";

    /// Assembles a configuration from already-parsed CLI options plus the
    /// injected environment value. The binary passes
    /// `std::env::var(HOME_ENV_VAR).ok().as_deref()` and
    /// `std::env::temp_dir()`; tests inject synthetic values — the library
    /// itself never touches process state (so precedence is unit-testable
    /// without polluting the outer shell).
    pub fn from_sources(
        cli: &CliOptions,
        env_home: Option<&str>,
        temp_base: &std::path::Path,
    ) -> Result<Self, ConfigError> {
        let resolved = resolve_effective_home(cli, env_home, temp_base)?;
        let data_home = resolved.path;
        if !data_home.is_absolute() {
            return Err(ConfigError::RelativeHome { value: data_home });
        }
        let stored_network = management::read_persisted_network(&data_home)
            .map_err(|detail| ConfigError::StoredNetworkInvalid { detail })?;
        let stored_mode = stored_network
            .as_ref()
            .and_then(|network| NetworkMode::parse(&network.mode));
        let network_mode = cli
            .network_mode
            .as_deref()
            .map(|value| {
                NetworkMode::parse(value).ok_or(ConfigError::BadNetworkMode {
                    value: value.to_string(),
                })
            })
            .transpose()?
            .or(stored_mode)
            .unwrap_or(NetworkMode::Loopback);
        let stored_bind = stored_network
            .as_ref()
            .map(|network| {
                network
                    .listen_host
                    .parse::<std::net::IpAddr>()
                    .map(|ip| SocketAddr::new(ip, network.listen_port).to_string())
                    .map_err(|err| ConfigError::StoredNetworkInvalid {
                        detail: format!("invalid saved listenHost: {err}"),
                    })
            })
            .transpose()?;
        let bind_raw = cli.bind.clone().unwrap_or_else(|| {
            match stored_bind
                .as_ref()
                .filter(|_| cli.network_mode.is_none() || stored_mode == Some(network_mode))
            {
                Some(bind) => bind.clone(),
                None => Self::DEFAULT_BIND.to_string(),
            }
        });
        let bind_addr: SocketAddr =
            bind_raw
                .parse::<SocketAddr>()
                .map_err(|source| ConfigError::BadBind {
                    value: bind_raw.clone(),
                    source: source.to_string(),
                })?;
        if network_mode == NetworkMode::Loopback && !bind_addr.ip().is_loopback() {
            return Err(ConfigError::NetworkModeBindMismatch {
                bind: bind_addr,
                mode: NetworkMode::Loopback,
            });
        }
        let shutdown_timeout_ms = cli
            .shutdown_timeout_ms
            .unwrap_or(shutdown::DEFAULT_SHUTDOWN_TIMEOUT_MS);
        Ok(Self {
            bind_addr,
            data_home,
            home_source: resolved.source,
            network_mode,
            shutdown_timeout_ms,
        })
    }

    /// Backwards-compatible CLI-args constructor (R02-T01 surface):
    /// resolves strictly from CLI arguments only, env = None.
    pub fn from_cli_args<I, S>(args: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let cli = parse_cli(args)?;
        Self::from_sources(&cli, None, &std::env::temp_dir())
    }

    /// Creates the data root if it does not exist yet. Fails loudly when the
    /// path exists as a non-directory or cannot be created — never a silent
    /// fallback to some other location.
    pub fn prepare_data_home(&self) -> Result<(), ConfigError> {
        if self.data_home.parent().is_none() {
            return Err(ConfigError::HomeIsFilesystemRoot {
                value: self.data_home.clone(),
            });
        }
        if self.data_home.exists() && !self.data_home.is_dir() {
            return Err(ConfigError::HomeIsNotADirectory {
                value: self.data_home.clone(),
            });
        }
        let new_home = !self.data_home.exists();
        if new_home {
            #[cfg(windows)]
            paths::ensure_private_dir(&self.data_home)?;
            #[cfg(not(windows))]
            std::fs::create_dir_all(&self.data_home).map_err(|source| {
                ConfigError::HomeCreateFailed {
                    value: self.data_home.clone(),
                    source: source.to_string(),
                }
            })?;
        }
        #[cfg(windows)]
        {
            let canonical = std::fs::canonicalize(&self.data_home).map_err(|source| {
                ConfigError::HomeCreateFailed {
                    value: self.data_home.clone(),
                    source: source.to_string(),
                }
            })?;
            if canonical.parent().is_none() {
                return Err(ConfigError::HomeIsFilesystemRoot { value: canonical });
            }
            if new_home {
                paths::ensure_private_dir(&canonical)?;
            } else {
                let _guard =
                    lingxi_adapters::storage::windows_acl::require_private_directory(&canonical)
                        .map_err(|source| ConfigError::HomeCreateFailed {
                            value: canonical.clone(),
                            source: source.to_string(),
                        })?;
            }
        }
        Ok(())
    }
}

/// Health check response (transport surface, not part of the frozen
/// `lingxi.wire` v1 vocabulary; values are sourced from the single
/// authorities, never duplicated here). Deliberately minimal: no paths, no
/// instance ids, no configuration echo (R02-T03 hard requirement).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: &'static str,
    pub server_kind: &'static str,
    pub server_version: &'static str,
    pub wire_protocol_min: u32,
    pub wire_protocol_max: u32,
    pub data_epoch: u32,
}

/// Live service state shared by handlers: the injected port implementations
/// (auth / tickets / storage / sessions / events / limits) live here.
#[derive(Clone)]
pub struct ServiceState {
    config: Arc<ServiceConfig>,
    auth: Arc<AuthService>,
    management: Arc<management::ManagementState>,
    static_web: Arc<static_web::StaticWebConfig>,
    bound_addr: Arc<std::sync::RwLock<Option<SocketAddr>>>,
    tickets: Arc<WsTicketService>,
    storage: Arc<RunDatabase>,
    sessions: Arc<sessions::SessionStore>,
    events: Arc<events::EventService>,
    rate: Arc<limits::RateLimiter>,
    ws_conns: Arc<limits::WsConnectionCounter>,
    /// HTTP admission gate (R02 stage-repair R1 / F07): hard in-flight cap
    /// + per-request budget at the outer edge.
    admission: Arc<limits::HttpAdmission>,
    /// CONNECTION admission gate (R02 stage-repair R2 / R2-F04): the hard
    /// cap enforced at the accept edge, so sockets waiting for their
    /// request headers are counted too (the request-level gate only runs
    /// after complete headers arrived). The cap is 2× the request
    /// in-flight cap — see bootstrap for the layering argument.
    conn_admission: Arc<limits::ConnectionAdmission>,
    /// Managed-task shutdown broadcast for active WS sessions (R02-T06).
    ws_shutdown: Arc<shutdown::WsShutdown>,
    /// Injectable clock (R02-T07): rate limiting, ticket expiry, execute
    /// timestamps all read through this surface.
    clock: Arc<dyn ServiceClock>,
    /// Injectable per-request id source (R02-T07).
    request_ids: Arc<dyn RequestIdGen>,
    /// Run lifecycle supervisor (R03-T01): drives every execute through
    /// the real queued→running→…→single-finalize chain.
    runs: Arc<runs::RunSupervisor>,
}

impl std::fmt::Debug for ServiceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceState")
            .field("config", &self.config)
            .field("db_path", &self.storage.db_path())
            .finish_non_exhaustive()
    }
}

/// Failures of the pre-serve bootstrap (auth store / run database
/// preparation). Storage failures are loud: a database that cannot open,
/// is newer than this build, or has tampered migration receipts refuses
/// to serve (R02-T04).
#[derive(Debug)]
pub enum ServiceStartupError {
    Auth(AuthSetupError),
    Storage(StorageError),
}

impl fmt::Display for ServiceStartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth(err) => write!(f, "auth bootstrap failed: {err}"),
            Self::Storage(err) => write!(f, "run database bootstrap failed: {err}"),
        }
    }
}

impl std::error::Error for ServiceStartupError {}

/// Explicit dependency set of the composition root (R02-T07): the resource
/// limits AND the injectable clock / request-id generator. `Default` is
/// the production wiring (system clock, OS-random request ids, the
/// documented limit defaults); tests inject a `ManualClock` /
/// `SequentialRequestIdGen` plus small caps and prove ordering without
/// sleeps.
#[derive(Clone)]
pub struct ServiceDeps {
    /// WS upgrade-ticket lifetime (R02-T03).
    pub ws_ticket_ttl_ms: u64,
    /// Per-peer HTTP rate-limit window (R02-T03).
    pub rate_window_ms: u64,
    /// Per-peer HTTP rate budget per window (R02-T03).
    pub rate_max: u32,
    /// Concurrently-upgraded WS connection ceiling (R02-T03; this is also
    /// the bound of the managed-task registry — every WS session task is
    /// spawned only after a slot was acquired, so the registry can never
    /// outgrow it).
    pub ws_max_connections: usize,
    /// Pending WS ticket registry bound (R02-T03).
    pub ws_max_tickets: usize,
    /// Bounded single-writer DB queue knobs (R02-T04). `queue_capacity`
    /// bounds pending jobs and `queue_wait_timeout_ms` bounds how long one
    /// submission may WAIT for capacity (R02 stage-repair R1 / F07): both
    /// edges surface `StorageError::QueueFull` → explicit 503
    /// backpressure; no unbounded waiter exists anywhere.
    pub store_options: StoreOptions,
    /// Event-surface flow control and registry caps (R02-T05 + R02-T07).
    pub event_limits: EventLimits,
    /// Hard cap of concurrently in-flight HTTP requests (R02 stage-repair
    /// R1 / F07): beyond it, admission rejects with 503 instead of letting
    /// requests pile up at the accept/handler boundary.
    pub http_max_in_flight: usize,
    /// Per-request wall-clock budget in milliseconds (F07): a request whose
    /// handling (body read included) exceeds it is cancelled and answered
    /// 408 — a slow-body client cannot hold a slot past the deadline.
    pub http_request_budget_ms: u64,
    /// Injectable clock (R02-T07).
    pub clock: std::sync::Arc<dyn ServiceClock>,
    /// Injectable per-request id source (R02-T07).
    pub request_ids: std::sync::Arc<dyn RequestIdGen>,
    /// Injectable model-turn source (R03-T01). `None` (the production
    /// default until R05 registers real providers) means runs complete
    /// with the explicit `completed.no_final.no_provider_configured`
    /// outcome — never a fabricated model reply. Deterministic doubles are
    /// injected by tests through the REAL chain; they produce responses
    /// only and never own run state.
    pub turn_provider: Option<std::sync::Arc<dyn lingxi_kernel::ports::TurnProviderPort>>,
    /// Injectable tool executor (R03-T01 minimal port; R04's unified
    /// gateway replaces doubles behind the same shape).
    pub tool_executor: Option<std::sync::Arc<dyn lingxi_kernel::ports::ToolExecutorPort>>,
    /// Run lifecycle bounds (R03-T01): hard, loud limits on model turns
    /// and attempts per run.
    pub run_limits: runs::RunDriveLimits,
    /// Global/agent/session model & tool admission quotas (R03-T02): a
    /// degenerate value is a loud startup error, never a silent clamp.
    pub quota_limits: quotas::QuotaLimits,
    /// Session serialization policy (R03-T02): busy-gate registry cap and
    /// the bounded steering inbox.
    pub session_concurrency: session_supervisor::SessionConcurrencyLimits,
    /// Cancellation cleanup policy (R03-T03): the bounded cleanup budget
    /// anchored at the cancel request plus the supervised-task registry
    /// cap. Degenerate values are loud startup errors.
    pub cancel_policy: cancel::CancelPolicy,
    /// Minimal approval-wait gate (R03-T03): `None` (the production
    /// default until R04 ships the full tool-policy gateway) means no
    /// run ever parks in `waiting_approval`. Tests inject a deterministic
    /// double to drive the REAL waiting_approval legs; the double only
    /// decides approvals and never owns run state.
    pub approval_gate: Option<std::sync::Arc<dyn approval::ApprovalGate>>,
}

impl Default for ServiceDeps {
    fn default() -> Self {
        Self {
            ws_ticket_ttl_ms: ws::DEFAULT_WS_TICKET_TTL_MS,
            rate_window_ms: limits::DEFAULT_HTTP_RATE_WINDOW_MS,
            rate_max: limits::DEFAULT_HTTP_RATE_MAX,
            ws_max_connections: limits::DEFAULT_WS_MAX_CONNECTIONS,
            ws_max_tickets: ws::DEFAULT_WS_MAX_TICKETS,
            store_options: StoreOptions::default(),
            event_limits: EventLimits::default(),
            http_max_in_flight: limits::DEFAULT_HTTP_MAX_IN_FLIGHT,
            http_request_budget_ms: limits::DEFAULT_HTTP_REQUEST_BUDGET_MS,
            clock: std::sync::Arc::new(SystemClock),
            request_ids: std::sync::Arc::new(RandomRequestIdGen),
            turn_provider: None,
            tool_executor: None,
            run_limits: runs::RunDriveLimits::default(),
            quota_limits: quotas::QuotaLimits::default(),
            session_concurrency: session_supervisor::SessionConcurrencyLimits::default(),
            cancel_policy: cancel::CancelPolicy::default(),
            approval_gate: None,
        }
    }
}

impl std::fmt::Debug for ServiceDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceDeps")
            .field("ws_ticket_ttl_ms", &self.ws_ticket_ttl_ms)
            .field("rate_window_ms", &self.rate_window_ms)
            .field("rate_max", &self.rate_max)
            .field("ws_max_connections", &self.ws_max_connections)
            .field("ws_max_tickets", &self.ws_max_tickets)
            .field("store_options", &self.store_options)
            .field("event_limits", &self.event_limits)
            .field("http_max_in_flight", &self.http_max_in_flight)
            .field("http_request_budget_ms", &self.http_request_budget_ms)
            .field("clock", &"Arc<dyn ServiceClock>")
            .field("request_ids", &"Arc<dyn RequestIdGen>")
            .field(
                "turn_provider",
                &self.turn_provider.as_ref().map(|_| "injected"),
            )
            .field(
                "tool_executor",
                &self.tool_executor.as_ref().map(|_| "injected"),
            )
            .field("run_limits", &self.run_limits)
            .field("quota_limits", &self.quota_limits)
            .field("session_concurrency", &self.session_concurrency)
            .field("cancel_policy", &self.cancel_policy)
            .field(
                "approval_gate",
                &self.approval_gate.as_ref().map(|_| "injected"),
            )
            .finish()
    }
}

/// Validates the injected resource knobs of the composition root (R02
/// stage-repair R3 / R3-F02): the binary's CLI layer rejects out-of-range
/// values at parse time, and THIS layer rejects them for every other
/// caller of `bootstrap_with_deps` with the same loudness (the binary maps
/// a startup error to exit 2). Nothing downstream may clamp, wrap,
/// truncate or saturate a degenerate value silently — the pre-fix
/// constructors' `.max(1)` fallbacks and the `saturating_mul(2)`
/// connection-cap derivation are exactly the failure modes this replaces.
fn validate_resource_deps(deps: &ServiceDeps) -> Result<(), ServiceStartupError> {
    fn invalid(detail: String) -> ServiceStartupError {
        ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
    }
    if deps.rate_window_ms == 0 {
        // RateLimiter compares elapsed >= window: a 0 window resets on
        // every request (fail-open, unlimited) — never a real config.
        return Err(invalid(
            "rate_window_ms must be >= 1 (0 would reset the rate window on \
             every request, silently disabling the limit)"
                .to_string(),
        ));
    }
    if deps.rate_max == 0 {
        return Err(invalid(
            "rate_max must be >= 1 (0 would answer every request 429 — the \
             exact state the pre-fix `as u32` truncation of 4294967296 \
             produced)"
                .to_string(),
        ));
    }
    if deps.ws_max_connections == 0 {
        return Err(invalid(
            "ws_max_connections must be >= 1 (0 would reject every WebSocket \
             upgrade)"
                .to_string(),
        ));
    }
    if deps.ws_max_tickets == 0 {
        return Err(invalid(
            "ws_max_tickets must be >= 1 (0 would reject every ticket issue)".to_string(),
        ));
    }
    if deps.ws_ticket_ttl_ms == 0 {
        return Err(invalid(
            "ws_ticket_ttl_ms must be >= 1 (0 would expire every ticket at \
             issue)"
                .to_string(),
        ));
    }
    if deps.http_max_in_flight == 0
        || deps.http_max_in_flight as u64 > crate::config::MAX_HTTP_MAX_IN_FLIGHT
    {
        return Err(invalid(format!(
            "http_max_in_flight must be in 1..={} (the connection cap is \
             derived as 2x this value; the bound keeps that CHECKED \
             multiplication overflow-free), got {}",
            crate::config::MAX_HTTP_MAX_IN_FLIGHT,
            deps.http_max_in_flight
        )));
    }
    if deps.http_request_budget_ms == 0
        || deps.http_request_budget_ms > crate::config::MAX_TIME_BUDGET_MS
    {
        return Err(invalid(format!(
            "http_request_budget_ms must be in 1..={} (30 days — larger \
             deadlines risk overflowing platform monotonic-clock arithmetic \
             in the header/request timers), got {}",
            crate::config::MAX_TIME_BUDGET_MS,
            deps.http_request_budget_ms
        )));
    }
    // R03-T02: the admission quotas and the session concurrency policy
    // validate with the same loudness (their own validators name the
    // exact degenerate knob).
    deps.quota_limits
        .validate()
        .map_err(ServiceStartupError::Storage)?;
    deps.session_concurrency
        .validate()
        .map_err(ServiceStartupError::Storage)?;
    // R03-T03: the cancellation cleanup policy (bounded budget +
    // supervised-task registry cap) validates the same way.
    deps.cancel_policy
        .validate()
        .map_err(ServiceStartupError::Storage)?;
    Ok(())
}

impl ServiceState {
    /// Prepares the full runtime state with the DEFAULT limits: loopback
    /// token (rotated per start, owner-only file), device registries,
    /// ticket service, seeded session store and the limiters. Every router
    /// built after this point enforces authentication from the first
    /// business route — there is no auth-less construction path in
    /// production code.
    pub async fn bootstrap(
        config: ServiceConfig,
        layout: &DataRootLayout,
    ) -> Result<Self, ServiceStartupError> {
        Self::bootstrap_with_limits(
            config,
            layout,
            ws::DEFAULT_WS_TICKET_TTL_MS,
            limits::DEFAULT_HTTP_RATE_WINDOW_MS,
            limits::DEFAULT_HTTP_RATE_MAX,
            limits::DEFAULT_WS_MAX_CONNECTIONS,
        )
        .await
    }

    /// Same as [`ServiceState::bootstrap`] with injectable limits (tests
    /// drive small TTLs/budgets instead of waiting real time; the limit
    /// logic itself is identical).
    pub async fn bootstrap_with_limits(
        config: ServiceConfig,
        layout: &DataRootLayout,
        ws_ticket_ttl_ms: u64,
        rate_window_ms: u64,
        rate_max: u32,
        ws_max_connections: usize,
    ) -> Result<Self, ServiceStartupError> {
        Self::bootstrap_with_limits_and_store(
            config,
            layout,
            ws_ticket_ttl_ms,
            rate_window_ms,
            rate_max,
            ws_max_connections,
            StoreOptions::default(),
        )
        .await
    }

    /// Same as [`ServiceState::bootstrap_with_limits`] with injectable
    /// storage options (tests drive small queue bounds; production keeps
    /// the defaults).
    pub async fn bootstrap_with_limits_and_store(
        config: ServiceConfig,
        layout: &DataRootLayout,
        ws_ticket_ttl_ms: u64,
        rate_window_ms: u64,
        rate_max: u32,
        ws_max_connections: usize,
        store_options: StoreOptions,
    ) -> Result<Self, ServiceStartupError> {
        Self::bootstrap_with_deps(
            config,
            layout,
            ServiceDeps {
                ws_ticket_ttl_ms,
                rate_window_ms,
                rate_max,
                ws_max_connections,
                store_options,
                ..ServiceDeps::default()
            },
        )
        .await
    }

    /// Full-injection constructor (R02-T07): every limit and the clock /
    /// request-id source are explicit. There is no other construction
    /// path — the default-facing constructors delegate here.
    pub async fn bootstrap_with_deps(
        config: ServiceConfig,
        layout: &DataRootLayout,
        deps: ServiceDeps,
    ) -> Result<Self, ServiceStartupError> {
        Self::bootstrap_with_instance_identity(
            config,
            layout,
            deps,
            instance::InstanceIdentity::generate(),
        )
        .await
    }

    /// 二进制入口必须沿用已取得独占锁的实例身份，令实例记录和本机令牌指向同一次启动。
    pub async fn bootstrap_with_locked_instance(
        config: ServiceConfig,
        layout: &DataRootLayout,
        deps: ServiceDeps,
        guard: &InstanceGuard,
    ) -> Result<Self, ServiceStartupError> {
        let configured_home = std::fs::canonicalize(&config.data_home).map_err(|err| {
            ServiceStartupError::Storage(StorageError::Io {
                detail: format!("cannot resolve service data home: {err}"),
            })
        })?;
        if guard.layout().home != layout.home || configured_home != layout.home {
            return Err(ServiceStartupError::Storage(StorageError::InvalidRequest {
                detail: "instance guard, service config and layout refer to different data homes"
                    .into(),
            }));
        }
        Self::bootstrap_with_instance_identity(config, layout, deps, guard.identity().clone()).await
    }

    async fn bootstrap_with_instance_identity(
        config: ServiceConfig,
        layout: &DataRootLayout,
        deps: ServiceDeps,
        identity: InstanceIdentity,
    ) -> Result<Self, ServiceStartupError> {
        deps.event_limits
            .validate()
            .map_err(ServiceStartupError::Storage)?;
        deps.store_options
            .validate()
            .map_err(ServiceStartupError::Storage)?;
        validate_resource_deps(&deps)?;
        let auth = AuthService::bootstrap(layout, &identity.instance_id)
            .map_err(ServiceStartupError::Auth)?;
        let management = management::ManagementState::open(layout, &config)
            .map_err(|detail| ServiceStartupError::Storage(StorageError::Io { detail }))?;
        // R02-T04: open the run/message database inside the private runtime
        // dir ({home}/lingxi-service/data/runs.db — fixed names, never
        // user-derived), run migrations and seed the synthetic sessions.
        // Failure is a startup error (exit 2 in the binary): never an
        // in-memory degraded fallback.
        let data_dir = layout.runtime_dir.join("data");
        paths::ensure_private_dir(&data_dir).map_err(|err| {
            ServiceStartupError::Storage(StorageError::Io {
                detail: format!("cannot prepare data dir {}: {err}", data_dir.display()),
            })
        })?;
        let db_path = data_dir.join(RUNS_DB_FILE_NAME);
        let storage = RunDatabase::open(&db_path, deps.store_options.clone())
            .await
            .map_err(ServiceStartupError::Storage)?;
        storage
            .ensure_session_seed(sessions::SessionStore::seed_rows(deps.clock.now_unix_ms()))
            .await
            .map_err(ServiceStartupError::Storage)?;
        let storage = Arc::new(storage);
        // R03-T02: the session surface owns the per-session serialization
        // gate (busy `session_busy` rejection + bounded steering inbox);
        // the policy comes from the deps.
        let session_store =
            sessions::SessionStore::with_concurrency((*storage).clone(), deps.session_concurrency);
        let session_arc = Arc::new(session_store);
        // R02-T05: the event subscription service over the same durable
        // log (snapshot/cursor protocol + post-commit publication hub).
        // Limits come from the deps; overflow/flow-control tests inject
        // smaller ones through the same path (the hub semantics are
        // identical).
        let events = events::EventService::new(
            Arc::clone(&storage),
            Arc::clone(&session_arc),
            deps.event_limits.clone(),
        )
        .map_err(ServiceStartupError::Storage)?;
        // R3-F02: the 2x connection-cap derivation is a CHECKED
        // multiplication. The CLI parse bound (1..=usize::MAX/2) and
        // validate_resource_deps above make the overflow branch unreachable
        // from any supported input — but if it ever were reached, the
        // failure is loud HERE instead of the pre-fix `saturating_mul(2)`
        // silently pinning the cap at usize::MAX.
        let connection_cap = deps.http_max_in_flight.checked_mul(2).ok_or_else(|| {
            ServiceStartupError::Storage(StorageError::InvalidRequest {
                detail: format!(
                    "http_max_in_flight {} overflows the 2x connection-cap \
                     derivation (supported range 1..={})",
                    deps.http_max_in_flight,
                    crate::config::MAX_HTTP_MAX_IN_FLIGHT
                ),
            })
        })?;
        // R03-T01: the run lifecycle supervisor. Degenerate bounds are a
        // loud startup error (never clamped silently); a missing provider is
        // EXPLICIT (no-provider outcome), never a fabricated reply.
        // R03-T02: the same construction now takes the admission-quota
        // manager (global/agent/session model & tool lanes).
        // R03-T03: plus the cancellation runtime (registry + task
        // supervisor + cleanup policy) and the minimal approval gate.
        let runs = runs::RunSupervisor::new(
            deps.turn_provider.clone(),
            deps.tool_executor.clone(),
            deps.run_limits,
            quotas::QuotaManager::new(deps.quota_limits),
            deps.cancel_policy,
            deps.approval_gate.clone(),
        )
        .map_err(ServiceStartupError::Storage)?;
        Ok(Self {
            config: Arc::new(config),
            auth: Arc::new(auth),
            management: Arc::new(management),
            static_web: Arc::new(static_web::StaticWebConfig::resolve()),
            bound_addr: Arc::new(std::sync::RwLock::new(None)),
            tickets: Arc::new(WsTicketService::new(
                deps.ws_ticket_ttl_ms,
                deps.ws_max_tickets,
            )),
            storage,
            sessions: session_arc,
            events: Arc::new(events),
            rate: Arc::new(limits::RateLimiter::new(deps.rate_window_ms, deps.rate_max)),
            ws_conns: Arc::new(limits::WsConnectionCounter::new(deps.ws_max_connections)),
            admission: Arc::new(limits::HttpAdmission::new(
                deps.http_max_in_flight,
                deps.http_request_budget_ms,
            )),
            // R2-F04: the CONNECTION cap must be strictly larger than the
            // request in-flight cap — admission layers only work when the
            // outer layer is looser. Every in-flight request holds one
            // connection, so an EQUAL cap lets N slow-body holders consume
            // every connection slot: the request-level 503 would become
            // unreachable and new connections would die at the accept edge
            // instead. 2× leaves exactly the in-flight set's worth of room
            // for arriving (header-wait) and keep-alive-idle sockets, so
            // the request gate stays the policy voice while the transport
            // still hard-bounds every open socket.
            conn_admission: Arc::new(limits::ConnectionAdmission::new(connection_cap)),
            ws_shutdown: Arc::new(shutdown::WsShutdown::new()),
            clock: deps.clock,
            request_ids: deps.request_ids,
            runs: Arc::new(runs),
        })
    }

    pub fn config(&self) -> &ServiceConfig {
        &self.config
    }

    fn actual_port(&self) -> u16 {
        self.bound_addr
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .map(|addr| addr.port())
            .unwrap_or(self.config.bind_addr.port())
    }

    pub fn auth(&self) -> &AuthService {
        &self.auth
    }

    pub fn tickets(&self) -> &WsTicketService {
        &self.tickets
    }

    pub fn sessions(&self) -> &sessions::SessionStore {
        &self.sessions
    }

    /// The injected event subscription service (R02-T05: snapshot/cursor
    /// protocol + post-commit publication hub).
    pub fn events(&self) -> &Arc<events::EventService> {
        &self.events
    }

    /// The injected storage port implementation (composition root handle
    /// for the close/checkpoint path).
    pub fn storage(&self) -> &Arc<RunDatabase> {
        &self.storage
    }

    /// The managed-task shutdown handle (R02-T06): the binary subscribes
    /// once for the shutdown broadcast; every WS session task subscribes
    /// per connection.
    pub fn ws_shutdown(&self) -> Arc<shutdown::WsShutdown> {
        Arc::clone(&self.ws_shutdown)
    }

    /// The injected clock (R02-T07): every served-path timestamp reads
    /// through this surface, so tests drive time instead of sleeping.
    pub fn clock(&self) -> &Arc<dyn ServiceClock> {
        &self.clock
    }

    /// The injected request-id source (R02-T07).
    pub fn request_ids(&self) -> &Arc<dyn RequestIdGen> {
        &self.request_ids
    }

    /// The run lifecycle supervisor (R03-T01): the single owner of the
    /// queued→running→…→finalize chain behind every execute.
    pub fn runs(&self) -> &Arc<runs::RunSupervisor> {
        &self.runs
    }

    pub fn ws_connection_count(&self) -> usize {
        self.ws_conns.current()
    }

    /// The HTTP admission gate (F07): tests observe the in-flight count
    /// through the same instance the router serves.
    pub fn admission(&self) -> &Arc<limits::HttpAdmission> {
        &self.admission
    }

    /// The CONNECTION admission gate (R2-F04): tests observe the open
    /// connection count through the same instance the serve loop enforces.
    pub fn connection_admission(&self) -> &Arc<limits::ConnectionAdmission> {
        &self.conn_admission
    }

    /// Arc handle to the WS connection counter (needed to acquire a
    /// `'static` slot guard inside the upgrade task).
    fn ws_conns_arc(&self) -> &Arc<limits::WsConnectionCounter> {
        &self.ws_conns
    }
}

/// Builds the health payload from the single version authorities.
fn health_payload() -> HealthResponse {
    HealthResponse {
        status: "ok",
        server_kind: SERVER_KIND,
        server_version: server_version(),
        wire_protocol_min: WIRE_PROTOCOL_MIN_SUPPORTED,
        wire_protocol_max: WIRE_PROTOCOL_MAX_SUPPORTED,
        data_epoch: ContractVersions::R00_BASELINE.data_epoch,
    }
}

// ── Error surface (protocol error model over HTTP statuses) ────────────────

/// Endpoint error: an HTTP status carrying the frozen `ProtocolError`
/// structure, with machine reasons in `details` (mirrors the incumbent
/// `{error, reason}` vocabulary without inventing a second error model).
///
/// R02-T07 structured-error contract: every outward error carries
/// - `code` (frozen protocol vocabulary),
/// - `retryable` (advisory for clients), and
/// - `details.causeId` — a stable, secret-free cause identifier
///   (`domain.cause` vocabulary, e.g. `auth.missing_credential`,
///   `storage.queue_full`, `events.subscriber_limit`) that ties the outward
///   error to its diagnostic trail. The envelope itself stays closed (the
///   frozen struct is not modified — `causeId` rides in `details`).
///
/// The error-enrichment middleware adds `details.requestId` and passes the
/// `message` through [`redaction::redact_text`] with the data home, so an
/// outward error can never echo a token, the raw request authentication
/// header or a local secret path.
pub struct EndpointError {
    status: StatusCode,
    error: ProtocolError,
    cause_id: String,
}

impl EndpointError {
    fn new(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            error: ProtocolError::new(code, message, false),
            cause_id: code.wire_name().to_string(),
        }
    }

    fn with_reason(mut self, reason: &str) -> Self {
        let details = self.error.details.get_or_insert_with(serde_json::Map::new);
        details.insert(
            "reason".to_string(),
            serde_json::Value::String(reason.to_string()),
        );
        self
    }

    /// Sets the structured cause identifier (`details.causeId`).
    pub fn with_cause(mut self, cause_id: impl Into<String>) -> Self {
        self.cause_id = cause_id.into();
        self
    }

    fn with_required_scope(mut self, scope: &str) -> Self {
        let details = self.error.details.get_or_insert_with(serde_json::Map::new);
        details.insert(
            "requiredScope".to_string(),
            serde_json::Value::String(scope.to_string()),
        );
        self
    }

    pub fn unauthorized(reason: &str) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            ErrorCode::Unauthorized,
            "authentication required",
        )
        .with_reason(reason)
        .with_cause(format!("auth.{reason}"))
    }

    pub fn forbidden(reason: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            "not allowed for this principal",
        )
        .with_reason(reason)
        .with_cause(format!("authz.{reason}"))
    }

    pub fn local_only() -> Self {
        Self::forbidden("local_owner_required")
    }

    pub fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            ErrorCode::NotFound,
            "resource not found",
        )
        .with_cause("resource.not_found")
    }

    pub fn invalid_transport(reason: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            "transport policy rejected the request",
        )
        .with_reason(reason)
        .with_cause(format!("transport.{reason}"))
    }

    pub fn rate_limited() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::Forbidden,
            "rate limit exceeded",
        )
        .with_reason("rate_limited")
        .with_cause("transport.rate_limited")
    }

    /// The rate-limiter peer REGISTRY is at its hard cap and this peer is
    /// not tracked (F07): a distinct-peer flood is a service-protection
    /// condition (503, retryable), not the peer's own window budget (429).
    pub fn rate_registry_full() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::BudgetExceeded,
            "rate-limiter peer registry is full",
        )
        .with_reason("rate_registry_full")
        .with_cause("transport.rate_registry_full")
    }

    /// HTTP admission cap reached (F07): too many in-flight requests.
    pub fn http_in_flight_limit() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::BudgetExceeded,
            "too many in-flight requests",
        )
        .with_reason("http_in_flight_limit")
        .with_cause("transport.http_in_flight_limit")
    }

    /// The request exceeded its wall-clock budget (F07): the handler
    /// (including the body read) was cancelled instead of holding the
    /// connection/admission slot without a bound.
    pub fn request_timeout() -> Self {
        Self::new(
            StatusCode::REQUEST_TIMEOUT,
            ErrorCode::BudgetExceeded,
            "request exceeded its time budget",
        )
        .with_reason("request_timeout")
        .with_cause("transport.request_timeout")
    }

    pub fn payload_too_large(detail: String) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::InvalidMessage,
            format!("request body exceeds the configured limit: {detail}"),
        )
        .with_reason("body_limit_exceeded")
        .with_cause("transport.body_limit_exceeded")
    }

    /// Maps an axum `Json` rejection: body-limit failures keep their 413,
    /// everything else is a 400 invalid_message.
    pub fn from_json_rejection(rejection: &axum::extract::rejection::JsonRejection) -> Self {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            Self::payload_too_large(rejection.body_text())
        } else {
            Self::invalid_message(rejection.body_text())
        }
    }

    pub fn invalid_message(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, ErrorCode::InvalidMessage, message)
            .with_cause("request.invalid_message")
    }

    /// The session already owns a running task (R03-T02, frozen incumbent
    /// `session_busy` gate): 409 + the stable `session_busy` reason and
    /// `retryable: true` — the adopted queueing behavior is the CLIENT's
    /// retry (or steering the running turn), never a server-side queue of
    /// normal inputs.
    pub fn session_busy() -> Self {
        let mut out = Self::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            "session is already running a task",
        );
        out.error.retryable = true;
        out.with_reason("session_busy")
            .with_cause("session.session_busy")
    }

    /// The tracked-session registry is at its hard cap (R03-T02 service
    /// protection; distinct from the session's own busy state).
    pub fn session_registry_full() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::BudgetExceeded,
            "session concurrency registry is full",
        )
        .with_reason("session_registry_full")
        .with_cause("session.registry_full")
    }

    /// R03-A08: an explicit requestId was resubmitted with CHANGED content.
    /// The recorded execution is not reused and no new execution started —
    /// the client must resolve the conflict (new id, or the original
    /// content); retrying the same id/content change cannot succeed.
    pub fn request_id_conflict(request_id: &str) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!("requestId was already accepted with different content: {request_id}"),
        )
        .with_reason("request_id_conflict")
        .with_cause("session.request_id_conflict")
    }

    /// The idempotency-key registry is at its hard cap (R03-T04 service
    /// protection; retryable backpressure).
    pub fn idempotency_registry_full() -> Self {
        let mut out = Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::BudgetExceeded,
            "idempotency-key registry is full",
        );
        out.error.retryable = true;
        out.with_reason("idempotency_registry_full")
            .with_cause("session.idempotency_registry_full")
    }

    /// Stable `storage.<cause>` identifier for a storage-port failure.
    /// Part of the R02-T07 structured-error vocabulary (shared with the WS
    /// surface), never carries a path or SQLite text on its own.
    pub fn storage_cause_id(err: &StorageError) -> String {
        format!(
            "storage.{}",
            match err {
                StorageError::QueueFull => "queue_full",
                StorageError::QueueClosed => "queue_closed",
                StorageError::Busy { .. } => "busy",
                StorageError::DiskFull { .. } => "disk_full",
                StorageError::Io { .. } => "io",
                StorageError::Conflict { .. } => "conflict",
                StorageError::DatabaseTooNew { .. } => "database_too_new",
                StorageError::SchemaTampered { .. } => "schema_tampered",
                StorageError::Corrupted { .. } => "corrupted",
                StorageError::InvalidRequest { .. } => "invalid_request",
                StorageError::RunIdExhausted { .. } => "run_id_exhausted",
                StorageError::Internal { .. } => "internal",
            }
        )
    }

    /// Maps a storage-port failure (R02-A07: no fake success). Retryable
    /// backpressure (bounded queue full / busy) answers 503; every other
    /// storage failure is a 500. Both carry the machine reason AND the
    /// structured causeId.
    pub fn storage(err: &StorageError) -> Self {
        let status = if err.retryable() {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        let mut out = Self::new(
            status,
            ErrorCode::Internal,
            format!("run database operation failed: {err}"),
        )
        .with_cause(Self::storage_cause_id(err));
        let details = out.error.details.get_or_insert_with(serde_json::Map::new);
        details.insert(
            "reason".to_string(),
            serde_json::Value::String(match err {
                StorageError::QueueFull => "db_queue_full".to_string(),
                StorageError::Busy { .. } => "db_busy".to_string(),
                StorageError::DiskFull { .. } => "db_disk_full".to_string(),
                _ => "db_failure".to_string(),
            }),
        );
        details.insert(
            "retryable".to_string(),
            serde_json::Value::Bool(err.retryable()),
        );
        out
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The structured cause identifier (`details.causeId`).
    pub fn cause_id(&self) -> &str {
        &self.cause_id
    }
}

impl IntoResponse for EndpointError {
    fn into_response(mut self) -> Response {
        // Single choke point for the structured cause: every outward error
        // body carries details.causeId (secret-free by vocabulary).
        let details = self.error.details.get_or_insert_with(serde_json::Map::new);
        details
            .entry("causeId".to_string())
            .or_insert_with(|| serde_json::Value::String(self.cause_id.clone()));
        let body = serde_json::to_value(&self.error).unwrap_or_else(|err| {
            // Serializing this plain struct cannot fail; if it ever does,
            // answer with a minimal valid protocol error instead of a
            // malformed body.
            tracing::error!(%err, "cannot serialize endpoint error");
            serde_json::json!({
                "code": "internal",
                "message": "error serialization failed",
                "retryable": false,
                "details": { "causeId": "internal.error" }
            })
        });
        (self.status, Json(body)).into_response()
    }
}

// ── Middleware: transport guard then authentication/authorization ───────────

/// Machine-readable stderr marker for transport rejections (evidence
/// contract for the acceptance matrix; never contains token material).
pub const TRANSPORT_REJECTED_MARKER: &str = "LINGXI_TRANSPORT_REJECTED";
/// Machine-readable stderr marker for authentication/authorization
/// rejections.
pub const AUTH_REJECTED_MARKER: &str = "LINGXI_AUTH_REJECTED";
/// Machine-readable stderr marker for a log-file write failure (explicit
/// degradation notice; see [`logging::LogRouter`]).
pub const LOG_WRITE_FAILED_MARKER: &str = "LINGXI_SERVICE_LOG_WRITE_FAILED";

/// Per-request correlation id (R02-T07): minted by the outermost
/// middleware, carried in request extensions, rejection marker lines and
/// error response `details.requestId` — the anchor that ties any
/// diagnostic line back to its request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(pub String);

/// HTTP admission gate (R02 stage-repair R1 / F07): runs just inside the
/// request-id middleware so every request — public or authenticated — is
/// bounded BEFORE any further work:
/// 1. hard in-flight cap: beyond `http_max_in_flight` the request is
///    rejected 503 `http_in_flight_limit` instead of piling up at the
///    accept/handler boundary;
/// 2. per-request wall-clock budget: handling (the body read included) is
///    wrapped in a timeout — on expiry the handler future is cancelled
///    (dropped) and the client gets 408 `request_timeout`, so a slow-body
///    client cannot occupy a connection/slot past the deadline.
///
/// Both rejections carry the transport marker line (evidence contract) and
/// ride through error enrichment for requestId + redaction like any other
/// rejection.
async fn http_admission(
    State(state): State<ServiceState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let request_id = req
        .extensions()
        .get::<RequestId>()
        .map(|r| r.0.clone())
        .unwrap_or_default();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let reject = |status: StatusCode, reason: &'static str, err: EndpointError| {
        eprintln!(
            "{}",
            redact_line(&format!(
                "{TRANSPORT_REJECTED_MARKER} request_id={request_id} method={method} path={path} \
                 status={} reason={reason} origin={} host={host} remote={remote}",
                status.as_u16(),
                origin.as_deref().unwrap_or("absent"),
            ))
        );
        err.into_response()
    };
    let Some(slot) = state.admission.acquire() else {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "http_in_flight_limit",
            EndpointError::http_in_flight_limit(),
        );
    };
    let budget = state.admission.request_budget();
    match tokio::time::timeout(budget, next.run(req)).await {
        Ok(response) => {
            drop(slot);
            response
        }
        Err(_elapsed) => {
            // The handler future was dropped = the request work (body read
            // included) is cancelled; the slot releases here.
            drop(slot);
            reject(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                EndpointError::request_timeout(),
            )
        }
    }
}

/// Outermost middleware (R02-T07): mints the request id, logs one
/// structured completion line per request, and enriches every error
/// response body with `details.requestId` plus the REDACTED message (the
/// data home is replaced, so a storage/auth error can never leak a local
/// secret path, a token or request credential material).
async fn error_enrichment(
    State(state): State<ServiceState>,
    mut req: Request,
    next: Next,
) -> Response {
    let request_id = state.request_ids.next_request_id();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    req.extensions_mut().insert(RequestId(request_id.clone()));
    let mut response = next.run(req).await;
    let status = response.status();
    // Liveness probes (peer-probe health checks) are infrastructure noise:
    // logging each one would append to the probee's home log on every
    // single-writer collision check (T02's tree-hash contract observes the
    // home must not change across a rejected second instance), and health
    // probes carry no business correlation worth a line. Everything else
    // gets one structured completion line.
    if path != "/lingxi/v1/health" {
        tracing::info!(
            request_id = %request_id,
            method = %method,
            path = %path,
            status = status.as_u16(),
            "request handled"
        );
    }
    if status.is_client_error() || status.is_server_error() {
        let (mut parts, body) = response.into_parts();
        let bytes = match axum::body::to_bytes(body, limits::DEFAULT_BODY_LIMIT_BYTES).await {
            Ok(bytes) => bytes,
            Err(err) => {
                // A body that cannot even be buffered is not silently
                // passed through: replace it with a minimal valid error
                // (the client keeps a well-formed response; the incident
                // is logged).
                tracing::error!(%err, request_id = %request_id, "cannot buffer error response body");
                let fallback = serde_json::json!({
                    "code": "internal",
                    "message": "error response could not be delivered",
                    "retryable": false,
                    "details": {
                        "causeId": "internal.error_response_undeliverable",
                        "requestId": request_id,
                    }
                });
                parts.headers.remove(header::CONTENT_LENGTH);
                parts.headers.insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                return Response::from_parts(
                    parts,
                    axum::body::Body::from(serde_json::to_string(&fallback).unwrap_or_default()),
                );
            }
        };
        match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(mut value) if value.get("code").is_some() && value.is_object() => {
                let object = value.as_object_mut().expect("checked is_object");
                // The message passes the redactor WITH the data home: no
                // local secret path (and no secret-shaped material) can
                // leave in an error body.
                if let Some(message) = object.get("message").and_then(|m| m.as_str()) {
                    let redacted = redact_text(message, Some(state.config().data_home.as_path()));
                    object.insert("message".to_string(), serde_json::Value::String(redacted));
                }
                let details = object
                    .entry("details")
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                if let Some(details) = details.as_object_mut() {
                    details
                        .entry("requestId".to_string())
                        .or_insert_with(|| serde_json::Value::String(request_id.clone()));
                }
                let serialized = serde_json::to_string(&value).unwrap_or_else(|err| {
                    tracing::error!(%err, "cannot serialize enriched error");
                    String::from(
                        "{\"code\":\"internal\",\"message\":\"error \
                        serialization failed\",\"retryable\":false}",
                    )
                });
                parts.headers.remove(header::CONTENT_LENGTH);
                parts.headers.insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                response = Response::from_parts(parts, axum::body::Body::from(serialized));
            }
            _ => {
                // Not a protocol-error body (e.g. a plain axum rejection):
                // pass through unchanged.
                response = Response::from_parts(parts, axum::body::Body::from(bytes));
            }
        }
    }
    response
}

fn browser_cors_response(
    mut response: Response,
    origin: Option<&str>,
    origin_allowed: bool,
) -> Response {
    let Some(origin) = origin else {
        return response;
    };
    if !origin_allowed {
        return response;
    }
    // transport_guard 只在来源通过白名单后调用这里；凭据响应不允许通配来源。
    let Ok(origin_value) = HeaderValue::from_str(origin) else {
        return response;
    };
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin_value);
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, Authorization, If-None-Match"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    response
}

async fn transport_guard(
    State(state): State<ServiceState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let request_id = req
        .extensions()
        .get::<RequestId>()
        .map(|r| r.0.clone())
        .unwrap_or_default();
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let secure = req
        .extensions()
        .get::<serve::SecureTransport>()
        .is_some_and(|value| value.0);
    // 只有实际运行中的本机网卡地址可作为局域网页面来源；网卡枚举失败时拒绝该来源。
    let local_ips = if state.config.network_mode == NetworkMode::Lan
        && check_origin(origin.as_deref()) == transport::OriginVerdict::Forbidden
    {
        if_addrs::get_if_addrs()
            .map(|interfaces| {
                interfaces
                    .into_iter()
                    .filter(|interface| interface.is_oper_up() && !interface.is_link_local())
                    .map(|interface| interface.ip())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let public_base_url = state.management.public_base_url();
    let origin_verdict = transport::check_service_origin(
        origin.as_deref(),
        &host,
        transport::ServiceOriginContext {
            mode: state.config.network_mode,
            actual_port: state.actual_port(),
            secure,
            bind_ip: state.config.bind_addr.ip(),
            local_ips: &local_ips,
            public_base_url: public_base_url.as_deref(),
        },
    );

    let reject = |status: StatusCode, reason: &str, resp: EndpointError| {
        // The marker line passes the redactor before printing: the marker
        // vocabulary itself is secret-free, but any future field added
        // here is structurally guarded.
        eprintln!(
            "{}",
            redact_line(&format!(
                "{TRANSPORT_REJECTED_MARKER} request_id={request_id} method={method} path={path} \
                 status={} reason={reason} origin={} host={host} remote={remote}",
                status.as_u16(),
                origin.as_deref().unwrap_or("absent"),
            ))
        );
        browser_cors_response(
            resp.into_response(),
            origin.as_deref(),
            origin_verdict == transport::OriginVerdict::Allowed,
        )
    };

    // Origin policy runs for EVERY route (public included): a browser that
    // speaks a foreign origin may not touch even the health surface.
    match origin_verdict {
        transport::OriginVerdict::Allowed | transport::OriginVerdict::Absent => {}
        transport::OriginVerdict::Forbidden => {
            return reject(
                StatusCode::FORBIDDEN,
                "bad_origin",
                EndpointError::invalid_transport("bad_origin"),
            );
        }
    }

    // Connection-kind inference (Host must agree with the network mode).
    let connection_kind =
        match infer_connection_kind(&host, Some(remote), state.config.network_mode) {
            Ok(kind) => kind,
            Err(rejection) => {
                let reason = rejection.reason_code();
                return reject(
                    StatusCode::FORBIDDEN,
                    reason,
                    EndpointError::invalid_transport(reason),
                );
            }
        };

    // Rate limit (per remote peer, fixed window) — driven by the injected
    // clock (R02-T07). The verdict distinguishes the peer's own window
    // budget (429) from the peer-REGISTRY hard cap (503, F07).
    match state.rate.check(remote.ip(), state.clock.now_unix_ms()) {
        limits::RateVerdict::Allowed => {}
        limits::RateVerdict::OverBudget => {
            return reject(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                EndpointError::rate_limited(),
            );
        }
        limits::RateVerdict::RegistryFull => {
            return reject(
                StatusCode::SERVICE_UNAVAILABLE,
                "rate_registry_full",
                EndpointError::rate_registry_full(),
            );
        }
    }

    // 浏览器的跨域预检必须在身份认证前结束，实际请求仍走完整鉴权链。
    if method == "OPTIONS" && origin.is_some() {
        return browser_cors_response(
            StatusCode::NO_CONTENT.into_response(),
            origin.as_deref(),
            true,
        );
    }

    let mut req = req;
    req.extensions_mut().insert(connection_kind);
    req.extensions_mut().insert(remote);
    browser_cors_response(next.run(req).await, origin.as_deref(), true)
}

/// 设备凭证与浏览器会话共用一处“仍有效”判断。WebSession 的内部会话号
/// 只留在服务内存；退出、过期或存储不可读都不能沿用已经签发的 WS 票据。
fn validate_live_principal(
    state: &ServiceState,
    principal: &Principal,
    secure_transport: bool,
) -> Result<(), AuthDenial> {
    state.auth.validate_principal(principal)?;
    if principal.credential_kind != CredentialKind::WebSession {
        return Ok(());
    }
    let reason = match state.management.web_session_current(
        principal,
        state.clock.now_unix_ms(),
        secure_transport,
    ) {
        Ok(true) => return Ok(()),
        Ok(false) => "invalid_credential",
        Err(_) => "web_session_registry_unavailable",
    };
    Err(AuthDenial {
        reason,
        credential_source: Some("web_session"),
        connection_kind: match principal.connection_kind {
            auth::ConnectionKindSerde::Local => ConnectionKind::Local,
            auth::ConnectionKindSerde::Lan => ConnectionKind::Lan,
        },
    })
}

async fn auth_guard(State(state): State<ServiceState>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let request_id = req
        .extensions()
        .get::<RequestId>()
        .map(|r| r.0.clone())
        .unwrap_or_default();
    if classify_route(&method, &path) == RoutePolicy::Public {
        return next.run(req).await;
    }

    let authorization = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let query = req.uri().query().unwrap_or("").to_string();
    let secure_transport = req
        .extensions()
        .get::<serve::SecureTransport>()
        .is_some_and(|value| value.0);
    let Some(connection_kind) = req.extensions().get::<ConnectionKind>().copied() else {
        // Unreachable when the transport guard is layered (it always
        // inserts the kind); fail closed rather than guess.
        eprintln!(
            "{}",
            redact_line(&format!(
                "{AUTH_REJECTED_MARKER} request_id={request_id} method={method} path={path} \
                 status=500 reason=missing_transport_context"
            ))
        );
        return EndpointError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            "transport context missing",
        )
        .with_cause("internal.missing_transport_context")
        .into_response();
    };

    let now_ms = state.clock.now_unix_ms();
    let principal = if path == "/lingxi/v1/ws" {
        // WS upgrade credentials: one-shot ticket, or bearer/query token
        // through the same authenticate call. The consumed/verified
        // principal rides on with the upgraded socket.
        let remote = req
            .extensions()
            .get::<SocketAddr>()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let auth_result = match ws::ws_credential_from(authorization.as_deref(), &query) {
            Some(ws::WsCredential::Ticket(ticket)) => state
                .tickets
                .consume(
                    &ticket,
                    connection_kind,
                    secure_transport,
                    "/lingxi/v1/ws",
                    now_ms,
                )
                .ok_or(AuthDenial {
                    reason: "invalid_ws_ticket",
                    credential_source: Some("ws_ticket"),
                    connection_kind,
                })
                .and_then(|principal| {
                    validate_live_principal(&state, &principal, secure_transport)?;
                    Ok((principal, "ws_ticket"))
                }),
            Some(ws::WsCredential::Bearer(token)) => state
                .auth
                .authenticate(
                    Some(&format!("Bearer {token}")),
                    None,
                    false,
                    connection_kind,
                )
                .map(|principal| (principal, "authorization")),
            Some(ws::WsCredential::QueryToken(token)) => state
                .auth
                .authenticate(None, Some(&token), true, connection_kind)
                .map(|principal| (principal, "query")),
            None => Err(AuthDenial {
                reason: "missing_credential",
                credential_source: Some("none"),
                connection_kind,
            }),
        };
        match auth_result {
            Ok((principal, source)) => {
                tracing::debug!(%source, request_id = %request_id, "ws upgrade authenticated");
                principal
            }
            Err(denial) => {
                let registry_failure = matches!(
                    denial.reason,
                    "auth_registry_unavailable" | "web_session_registry_unavailable"
                );
                eprintln!(
                    "{}",
                    redact_line(&format!(
                        "{AUTH_REJECTED_MARKER} request_id={request_id} method={method} path={path} \
                         status={} reason={} remote={remote}",
                        if registry_failure { 500 } else { 401 },
                        denial.reason
                    ))
                );
                if registry_failure {
                    return EndpointError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ErrorCode::Internal,
                        "credential registry unavailable",
                    )
                    .with_reason(if denial.reason == "web_session_registry_unavailable" {
                        "web_session_registry_failure"
                    } else {
                        "device_registry_failure"
                    })
                    .with_cause(if denial.reason == "web_session_registry_unavailable" {
                        "auth.web_session_registry_failure"
                    } else {
                        "auth.device_registry_failure"
                    })
                    .into_response();
                }
                return EndpointError::unauthorized(denial.reason).into_response();
            }
        }
    } else {
        let query_token = ws::parse_query_pairs(&query)
            .into_iter()
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value);
        let cookie_principal = if authorization.is_none() && query_token.is_none() {
            let cookie = req
                .headers()
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok());
            match state
                .management
                .authenticate_cookie(cookie, now_ms, secure_transport)
            {
                Ok(Some(principal)) if principal.connection_kind == connection_kind.into() => {
                    match validate_live_principal(&state, &principal, secure_transport) {
                        Ok(()) => Some(principal),
                        Err(denial) if denial.reason == "web_session_registry_unavailable" => {
                            return EndpointError::new(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                ErrorCode::Internal,
                                "credential registry unavailable",
                            )
                            .with_reason("web_session_registry_failure")
                            .with_cause("auth.web_session_registry_failure")
                            .into_response()
                        }
                        Err(_) => None,
                    }
                }
                Ok(_) => None,
                Err(err) => return err.into_response(),
            }
        } else {
            None
        };
        let authentication = match cookie_principal {
            Some(principal) => Ok(principal),
            None => state.auth.authenticate(
                authorization.as_deref(),
                query_token.as_deref(),
                true,
                connection_kind,
            ),
        };
        match authentication {
            Ok(principal) => principal,
            Err(denial) => {
                if denial.reason == "auth_registry_unavailable" {
                    return EndpointError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ErrorCode::Internal,
                        "credential registry unavailable",
                    )
                    .with_reason("device_registry_failure")
                    .with_cause("auth.device_registry_failure")
                    .into_response();
                }
                // R14-F01 (R02 stage-repair R14): the R00 supplemental leaf
                // R00-T02-LA-5816DA563ED8 pins the ORIGINAL assertion for
                // POST /lingxi/v1/ws-ticket — "无主体返回 403", captured from
                // the incumbent (`server/routes/ws-auth.ts` answers a missing
                // principal with 403 missing_principal; `server/http/
                // request-principal.ts` denies failed authentication with
                // 403). The ticket-issuance route keeps that
                // authorization-shaped denial (403) for an unauthenticated
                // caller so the R00 leaf assertion holds verbatim; every
                // other route keeps the 401 authentication semantics the
                // R02 stage matrix (A05) asserts.
                let ticket_route = path == "/lingxi/v1/ws-ticket" && method == "POST";
                let (status, err) = if ticket_route {
                    (
                        StatusCode::FORBIDDEN,
                        EndpointError::forbidden(denial.reason),
                    )
                } else {
                    (
                        StatusCode::UNAUTHORIZED,
                        EndpointError::unauthorized(denial.reason),
                    )
                };
                eprintln!(
                    "{}",
                    redact_line(&format!(
                        "{AUTH_REJECTED_MARKER} request_id={request_id} method={method} path={path} \
                         status={status} reason={} remote={}",
                        denial.reason,
                        req.extensions()
                            .get::<SocketAddr>()
                            .map(|a| a.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    ))
                );
                return err.into_response();
            }
        }
    };

    // Route-level authorization (shared with the WS path: same table).
    if let Err(denial) = authorize_route(&method, &path, Some(&principal)) {
        eprintln!(
            "{}",
            redact_line(&format!(
                "{AUTH_REJECTED_MARKER} request_id={request_id} method={method} path={path} \
                 status={} reason={} remote={}",
                denial.status,
                denial.reason,
                req.extensions()
                    .get::<SocketAddr>()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ))
        );
        let mut err = EndpointError::forbidden(denial.reason);
        if let Some(scope) = denial.required_scope {
            err = err.with_required_scope(scope);
        }
        return err.into_response();
    }

    let mut req = req;
    req.extensions_mut().insert(principal);
    next.run(req).await
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn health() -> Json<HealthResponse> {
    Json(health_payload())
}

async fn me(
    State(state): State<ServiceState>,
    axum::Extension(mut principal): axum::Extension<Principal>,
) -> Response {
    let (server_id, studio_id) = state.management.identity_ids();
    principal.server_node_id.get_or_insert(server_id.clone());
    principal.studio_id.get_or_insert(studio_id.clone());
    let mut capabilities = std::collections::BTreeSet::new();
    for scope in &principal.scopes {
        capabilities.insert(scope.as_str());
        if let Some(namespace) = scope.split('.').next() {
            capabilities.insert(namespace);
        }
    }
    let mut view = serde_json::to_value(&principal).expect("Principal serialization is infallible");
    if let Some(object) = view.as_object_mut() {
        object.insert("version".into(), serde_json::json!(server_version()));
        object.insert("serverVersion".into(), serde_json::json!(server_version()));
        object.insert("serverNodeKind".into(), serde_json::json!(SERVER_KIND));
        object.insert("serverId".into(), serde_json::json!(server_id));
        object.insert("capabilities".into(), serde_json::json!(capabilities));
        object.insert("principal".into(), serde_json::json!(principal));
    }
    (StatusCode::OK, Json(view)).into_response()
}

async fn list_sessions(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
) -> Response {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Body {
        sessions: Vec<sessions::SessionView>,
    }
    let sessions = match state.sessions.list_for(&principal).await {
        Ok(views) => views,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    (StatusCode::OK, Json(Body { sessions })).into_response()
}

async fn get_session(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    match state.sessions.get_for(&principal, &session_id).await {
        Ok(sessions::SessionAccess::Ok(facts)) => {
            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            struct Body {
                session_id: String,
                agent_id: String,
                owner_user_id: String,
                title: String,
                run_count: u64,
                last_runs: Vec<sessions::RunSummary>,
            }
            let body = Body {
                session_id: facts.session_id,
                agent_id: facts.agent_id,
                owner_user_id: facts.owner_user_id,
                title: facts.title,
                run_count: facts.run_count,
                last_runs: facts.last_runs,
            };
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(sessions::SessionAccess::NotFound) => EndpointError::not_found().into_response(),
        Ok(sessions::SessionAccess::Forbidden) => {
            EndpointError::forbidden("cross_principal_access").into_response()
        }
        Err(err) => EndpointError::storage(&err).into_response(),
    }
}

/// `GET /lingxi/v1/sessions/{id}/events` — snapshot / cursor continuation
/// page of the session's event stream (R02-T05). Transport DTO reusing the
/// protocol `Page` semantics (`items`/`nextCursor`/`snapshotSeq` pin the
/// boundary the following subscription joins at) plus the stream id.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventsPageBody {
    stream_id: String,
    mode: &'static str,
    from_seq: lingxi_protocol::Seq,
    snapshot_seq: lingxi_protocol::Seq,
    items: Vec<lingxi_protocol::EventEnvelope>,
    next_cursor: Option<lingxi_protocol::Cursor>,
}

async fn session_events_page(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    // Query params: strict parse (unknown keys are hard errors, mirroring
    // the closed-struct policy of the JSON bodies).
    let pairs = ws::parse_query_pairs(query.as_deref().unwrap_or(""));
    let mut cursor = None;
    let mut limit = None;
    for (key, value) in pairs {
        match key.as_str() {
            "cursor" => {
                if cursor
                    .replace(lingxi_protocol::Cursor::new(value))
                    .is_some()
                {
                    return EndpointError::invalid_message("duplicate cursor query parameter")
                        .into_response();
                }
            }
            "limit" => match value.parse::<u32>() {
                Ok(parsed) => limit = Some(parsed),
                Err(_) => {
                    return EndpointError::invalid_message(
                        "limit must be a non-negative decimal integer",
                    )
                    .into_response();
                }
            },
            other => {
                return EndpointError::invalid_message(format!(
                    "unknown query parameter {other:?}"
                ))
                .into_response();
            }
        }
    }
    match state
        .events
        .events_page(&principal, &session_id, cursor, limit)
        .await
    {
        Ok(cut) => (
            StatusCode::OK,
            Json(EventsPageBody {
                stream_id: cut.stream_id,
                mode: cut.mode,
                from_seq: cut.from_seq,
                snapshot_seq: cut.snapshot_seq,
                items: cut.events,
                next_cursor: cut.next_cursor,
            }),
        )
            .into_response(),
        Err(events::SubscribePageError::CursorExpired(required)) => {
            // The explicit rebuild directive (R02-A10): the client refetches
            // a snapshot and resubscribes — never an empty page, never a
            // silent replay.
            let mut err = EndpointError::new(
                StatusCode::CONFLICT,
                ErrorCode::CursorExpired,
                "cursor predates the retained event floor; refetch a snapshot",
            )
            .with_cause("events.cursor_expired");
            let details = err.error.details.get_or_insert_with(serde_json::Map::new);
            details.insert(
                "reason".to_string(),
                serde_json::Value::String("snapshot_required".to_string()),
            );
            // floorSeq mirrors the WS control frame: present only when the
            // stream still retains events (a purge-emptied stream has no
            // floor to name — the directive itself is the instruction).
            if let Some(floor) = required.floor {
                details.insert(
                    "floorSeq".to_string(),
                    serde_json::Value::String(floor.to_wire_string()),
                );
            }
            details.insert(
                "streamId".to_string(),
                serde_json::Value::String(required.stream_id),
            );
            err.into_response()
        }
        Err(events::SubscribePageError::InvalidLimit { requested, max }) => {
            EndpointError::invalid_message(format!("limit {requested} out of range 1..={max}"))
                .with_cause("request.invalid_limit")
                .into_response()
        }
        Err(events::SubscribePageError::Reject(reject)) => match &reject {
            events::SubscribeReject::StreamNotFound { .. } => EndpointError::not_found()
                .with_cause("events.stream_not_found")
                .into_response(),
            events::SubscribeReject::Forbidden { .. } => {
                EndpointError::forbidden("cross_principal_access")
                    .with_cause("events.cross_principal_access")
                    .into_response()
            }
            events::SubscribeReject::MalformedCursor { detail } => {
                EndpointError::invalid_message(format!("malformed cursor: {detail}"))
                    .with_cause("events.malformed_cursor")
                    .into_response()
            }
            events::SubscribeReject::StaleStreamCursor {
                stream_id,
                cursor_stream,
            } => EndpointError::invalid_message(format!(
                "cursor was issued for stream {cursor_stream:?}, not {stream_id:?} \
                 (stale stream cursor)"
            ))
            .with_cause("events.stale_stream_cursor")
            .into_response(),
            events::SubscribeReject::FutureCursor { seq, head, .. } => {
                EndpointError::invalid_message(format!(
                    "cursor seq {} is beyond the committed head {} (future cursor)",
                    seq, head
                ))
                .with_cause("events.future_cursor")
                .into_response()
            }
            // Bounded subscriber registry (R02-T07): an explicit 503, the
            // caller retries later — never a silent admission.
            events::SubscribeReject::SubscriberLimit { scope, limit, .. } => EndpointError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::BudgetExceeded,
                format!("event subscriber limit reached ({scope} cap {limit})"),
            )
            .with_reason("subscriber_limit")
            .with_cause("events.subscriber_limit")
            .into_response(),
            events::SubscribeReject::Storage(err) => EndpointError::storage(err).into_response(),
        },
    }
}

async fn execute_session(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<sessions::ExecuteRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(json) => json,
        Err(rejection) => {
            return EndpointError::from_json_rejection(&rejection).into_response();
        }
    };
    let storage = Arc::clone(state.storage());
    // R03-T04/A08: the submission carries the OPTIONAL explicit requestId
    // (idempotency key). Validation of its SHAPE happens here (a 400 is a
    // client error); the service surface re-validates on its own path.
    let validated_request_id = match &request.request_id {
        None => None,
        Some(raw) => match crate::dedup::validate_request_id(raw) {
            Ok(validated) => Some(validated),
            Err(detail) => {
                return EndpointError::invalid_message(format!("requestId invalid: {detail}"))
                    .into_response();
            }
        },
    };
    let submission = sessions::ExecuteSubmission {
        input: &request.input,
        request_id: validated_request_id.as_deref(),
    };
    match state
        .sessions
        .execute_submission_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &principal,
            &session_id,
            &submission,
            state.clock.now_unix_ms(),
        )
        .await
    {
        Ok(accepted) => (StatusCode::OK, Json(accepted)).into_response(),
        Err(sessions::SessionExecuteError::NotFound) => EndpointError::not_found().into_response(),
        Err(sessions::SessionExecuteError::Forbidden) => {
            EndpointError::forbidden("cross_principal_access").into_response()
        }
        Err(sessions::SessionExecuteError::Busy) => EndpointError::session_busy().into_response(),
        Err(sessions::SessionExecuteError::SessionRegistryFull) => {
            EndpointError::session_registry_full().into_response()
        }
        Err(sessions::SessionExecuteError::DuplicateRequestConflict { request_id, .. }) => {
            EndpointError::request_id_conflict(&request_id).into_response()
        }
        Err(sessions::SessionExecuteError::IdempotencyRegistryFull { .. }) => {
            EndpointError::idempotency_registry_full().into_response()
        }
        Err(sessions::SessionExecuteError::InvalidRequestId { detail }) => {
            EndpointError::invalid_message(format!("requestId invalid: {detail}")).into_response()
        }
        Err(sessions::SessionExecuteError::SteeringInboxFull) => {
            // Not reachable from the execute route (steering is a separate
            // service-surface call); mapped anyway so the surface stays
            // closed and loud if a future route wires it.
            EndpointError::new(
                StatusCode::CONFLICT,
                ErrorCode::BudgetExceeded,
                "session steering inbox is full",
            )
            .with_reason("steering_inbox_full")
            .with_cause("session.steering_inbox_full")
            .into_response()
        }
        Err(sessions::SessionExecuteError::Storage(err)) => {
            EndpointError::storage(&err).into_response()
        }
    }
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IssueDeviceCredentialRequest {
    user_id: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    expires_at_unix_ms: Option<u64>,
}

async fn issue_device_credential(
    State(state): State<ServiceState>,
    body: Result<Json<IssueDeviceCredentialRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // Route policy is local_only (enforced by the auth middleware); this
    // handler is the trusted-boundary management surface, mirroring the
    // incumbent LOCAL_ONLY /api/devices/ routes.
    let Json(request) = match body {
        Ok(json) => json,
        Err(rejection) => {
            return EndpointError::from_json_rejection(&rejection).into_response();
        }
    };
    if request.user_id.trim().is_empty() {
        return EndpointError::invalid_message("userId must be a non-empty string").into_response();
    }
    let scopes: Vec<&str> = request.scopes.iter().map(String::as_str).collect();
    match state.auth.issue_device_credential(
        request.user_id.trim(),
        &scopes,
        request.expires_at_unix_ms,
    ) {
        Ok(issued) => {
            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            struct Body {
                credential_id: String,
                device_id: String,
                secret: String,
                scopes: Vec<String>,
                expires_at_unix_ms: Option<u64>,
            }
            (
                StatusCode::CREATED,
                Json(Body {
                    credential_id: issued.credential_id,
                    device_id: issued.device_id,
                    secret: issued.secret,
                    scopes: issued.scopes,
                    expires_at_unix_ms: issued.expires_at_unix_ms,
                }),
            )
                .into_response()
        }
        Err(err) => EndpointError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            format!("cannot issue device credential: {err}"),
        )
        .into_response(),
    }
}

async fn ws_ticket(
    axum::Extension(principal): axum::Extension<Principal>,
    axum::Extension(connection_kind): axum::Extension<ConnectionKind>,
    axum::Extension(secure_transport): axum::Extension<serve::SecureTransport>,
    State(state): State<ServiceState>,
) -> Response {
    // R9-F05: ticket material is system-CSPRNG-only; when the secure
    // source fails, NO ticket is issued (5xx) — never a weaker fallback.
    let issued = match state.tickets.issue(
        principal,
        connection_kind,
        secure_transport.0,
        "/lingxi/v1/ws",
        state.clock.now_unix_ms(),
    ) {
        Ok(issued) => issued,
        Err(err) => {
            return EndpointError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                format!("cannot mint ws ticket — secure random source refused: {err}"),
            )
            .into_response()
        }
    };
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Body {
        ticket: String,
        expires_at_unix_ms: u64,
    }
    (
        StatusCode::OK,
        Json(Body {
            ticket: issued.ticket,
            expires_at_unix_ms: issued.expires_at_unix_ms,
        }),
    )
        .into_response()
}

async fn ws_handler(
    State(state): State<ServiceState>,
    axum::Extension(principal): axum::Extension<Principal>,
    axum::Extension(secure_transport): axum::Extension<serve::SecureTransport>,
    request_id: Option<axum::Extension<RequestId>>,
    ws_upgrade: Result<ws::WsUpgrade, ws::WsUpgradeRejection>,
) -> Response {
    let request_id = request_id
        .map(|axum::Extension(id)| id.0)
        .unwrap_or_default();
    let upgrade = match ws_upgrade {
        Ok(upgrade) => upgrade,
        Err(rejection) => {
            eprintln!(
                "{}",
                redact_line(&format!(
                    "{TRANSPORT_REJECTED_MARKER} request_id={request_id} method=GET \
                     path=/lingxi/v1/ws status={} reason={}",
                    rejection.status, rejection.reason
                ))
            );
            return EndpointError::new(
                StatusCode::from_u16(rejection.status).unwrap_or(StatusCode::BAD_REQUEST),
                ErrorCode::InvalidMessage,
                "invalid WebSocket upgrade request",
            )
            .with_reason(rejection.reason)
            .with_cause("transport.invalid_ws_upgrade")
            .into_response();
        }
    };
    // Connection ceiling BEFORE the 101: a refused upgrade must not count.
    // This is also the managed-task registry bound: the session task is
    // spawned only after a slot was acquired (R02-T07).
    let Some(slot) = state.ws_conns_arc().acquire() else {
        return EndpointError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::BudgetExceeded,
            "websocket connection limit reached",
        )
        .with_reason("ws_connection_limit")
        .with_cause("transport.ws_connection_limit")
        .into_response();
    };
    let state_for_task = state.clone();
    let response = ws::switching_protocols_response(&upgrade.sec_websocket_key);
    tokio::spawn(async move {
        // OnUpgrade IS the future resolving to the upgraded IO; TokioIo
        // adapts hyper's IO traits to tokio's for the frame codec.
        match upgrade.on_upgrade.await {
            Ok(io) => {
                let io = hyper_util::rt::tokio::TokioIo::new(io);
                run_ws_session(io, state_for_task, principal, secure_transport.0, slot).await;
            }
            Err(err) => {
                tracing::warn!(%err, "websocket upgrade failed after 101");
            }
        }
    });
    response
}

/// 每次向已建立的 WS 连接收发业务数据前，以及空闲连接的定时检查中，
/// 重新确认设备凭证。注册簿读不到时明确报内部错误，不能伪装成凭证撤销。
async fn ensure_ws_principal_current<IO>(
    io: &mut IO,
    state: &ServiceState,
    principal: &Principal,
    secure_transport: bool,
) -> bool
where
    IO: tokio::io::AsyncWrite + Unpin,
{
    let Err(denial) = validate_live_principal(state, principal, secure_transport) else {
        return true;
    };
    let registry_failure = matches!(
        denial.reason,
        "auth_registry_unavailable" | "web_session_registry_unavailable"
    );
    let registry_cause = if denial.reason == "web_session_registry_unavailable" {
        "auth.web_session_registry_failure"
    } else {
        "auth.device_registry_failure"
    };
    let (code, message, cause, close_code, close_reason) = if registry_failure {
        (
            ErrorCode::Internal,
            "credential registry unavailable",
            registry_cause,
            1011,
            "internal",
        )
    } else {
        (
            ErrorCode::Unauthorized,
            "device credential is no longer valid",
            "auth.invalid_credential",
            WS_CLOSE_UNAUTHORIZED,
            "unauthorized",
        )
    };
    let error =
        ProtocolError::new(code, message, false).with_details(error_details(denial.reason, cause));
    let _ = ws::write_ws_text(io, &canon::canonical_bytes(&error)).await;
    let _ = ws::write_ws_close(io, close_code, close_reason).await;
    false
}

async fn run_ws_session<IO>(
    io: IO,
    state: ServiceState,
    principal: Principal,
    secure_transport: bool,
    _slot: limits::WsConnectionGuard,
) where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // R02-T06: this session is a MANAGED TASK of the shutdown coordinator —
    // register it (open count) and watch the shutdown broadcast. The guard
    // decrements the count on every exit path of this loop.
    let mut shutdown_rx = state.ws_shutdown().subscribe();
    let _session_guard = shutdown::WsSessionGuard::new(state.ws_shutdown());
    let (reader, mut writer) = tokio::io::split(io);
    let mut frame_reader = ws::ClientFrameReader::new(reader);
    // 关停广播分支在统一关停预算内已写 close(1001) 并立即返回（进程即将
    // 退出，不再等待排空）；其余服务端主动关闭统一走会话末尾的干净收尾。
    let mut exited_by_shutdown = false;
    async {
    // 撤销可能发生在客户端静默期间，或订阅尚无新事件时；定时复核可让
    // 已升级连接在没有下一帧的情况下失效。跳过积压 tick，避免突发重读。
    let mut auth_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    auth_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // 1. lingxi.wire handshake: first text frame must be a ClientHello.
    //    R02 stage-repair R1 / F03: the handshake read races the shutdown
    //    broadcast — a client that upgrades but never sends ClientHello
    //    must not hold the transport drain past the budget.
    let first_frame = loop {
        tokio::select! {
            shutdown = shutdown_rx.changed() => {
                let _ = shutdown;
                let _ = ws::write_ws_close(&mut writer, 1001, "server_shutdown").await;
                exited_by_shutdown = true;
                return;
            }
            _ = auth_tick.tick() => {
                if !ensure_ws_principal_current(&mut writer, &state, &principal, secure_transport).await {
                    return;
                }
            }
            frame = frame_reader.recv() => break frame,
        }
    };
    if matches!(first_frame, Ok(Some(_)))
        && !ensure_ws_principal_current(&mut writer, &state, &principal, secure_transport).await
    {
        return;
    }
    match first_frame {
        Ok(Some(ws::WsFrame::Text(bytes))) => {
            let hello: ClientHello = match serde_json::from_slice(&bytes) {
                Ok(hello) => hello,
                Err(err) => {
                    let error = ProtocolError::new(
                        ErrorCode::InvalidMessage,
                        format!("first frame is not a valid ClientHello: {err}"),
                        false,
                    );
                    let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                    let _ = ws::write_ws_close(
                        &mut writer,
                        ws::WS_CLOSE_INVALID_MESSAGE,
                        "invalid_message",
                    )
                    .await;
                    return;
                }
            };
            match negotiate_protocol(&hello) {
                Ok(selected) => {
                    let server_hello = ServerHello {
                        protocol: WIRE_PROTOCOL_NAME.to_string(),
                        selected_protocol: selected,
                        wire_protocol_min: WIRE_PROTOCOL_MIN_SUPPORTED,
                        wire_protocol_max: WIRE_PROTOCOL_MAX_SUPPORTED,
                        data_epoch: ContractVersions::R00_BASELINE.data_epoch,
                        server_kind: SERVER_KIND.to_string(),
                        server_version: server_version().to_string(),
                        rejected_caps: Vec::new(),
                    };
                    if let Err(err) =
                        ws::write_ws_text(&mut writer, &canon::canonical_bytes(&server_hello)).await
                    {
                        tracing::warn!(%err, "cannot send ServerHello");
                        return;
                    }
                }
                Err(error) => {
                    let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                    let _ = ws::write_ws_close(
                        &mut writer,
                        ws::WS_CLOSE_INVALID_MESSAGE,
                        "version_incompatible",
                    )
                    .await;
                    return;
                }
            }
        }
        Ok(Some(_)) => {
            let _ = ws::write_ws_close(
                &mut writer,
                ws::WS_CLOSE_INVALID_MESSAGE,
                "first frame must be a text ClientHello",
            )
            .await;
            return;
        }
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(%err, "ws handshake read failed");
            if err.kind() == std::io::ErrorKind::InvalidData {
                let _ = ws::write_ws_close(&mut writer, 1002, "protocol_error").await;
            }
            return;
        }
    }

    // 2. Request loop with interleaved event delivery (R02-T05): inbound
    //    frames (session_read / subscribe_events / ping / close) race the
    //    active subscription's mailbox; each event frame written to the
    //    socket is a bare canonical EventEnvelope (PROTOCOL_SPEC §5),
    //    control frames carry frameKind:"control".
    let mut subscription: Option<events::SubscriptionGuard> = None;
    loop {
        let sub_mailbox = subscription.as_ref().map(|s| s.mailbox().clone());
        let sub_frame = async {
            match sub_mailbox {
                Some(mailbox) => mailbox.recv().await,
                None => std::future::pending::<Option<events::SubscriptionFrame>>().await,
            }
        };
        tokio::select! {
            shutdown = shutdown_rx.changed() => {
                // R02-T06: the coordinator broadcast a shutdown — send a
                // polite close(1001) and end the session so the drain can
                // complete within the deadline.
                let _ = shutdown;
                let _ = ws::write_ws_close(&mut writer, 1001, "server_shutdown").await;
                exited_by_shutdown = true;
                return;
            }
            _ = auth_tick.tick() => {
                if !ensure_ws_principal_current(&mut writer, &state, &principal, secure_transport).await {
                    return;
                }
            }
            incoming = frame_reader.recv() => {
                if matches!(incoming, Ok(Some(_)))
                    && !ensure_ws_principal_current(&mut writer, &state, &principal, secure_transport).await
                {
                    return;
                }
                match incoming {
                    Ok(Some(ws::WsFrame::Text(bytes))) => {
                        let request: ws::WsClientRequest = match serde_json::from_slice(&bytes) {
                            Ok(request) => request,
                            Err(err) => {
                                let error = ProtocolError::new(
                                    ErrorCode::InvalidMessage,
                                    format!("frame is not a valid ws request: {err}"),
                                    false,
                                );
                                let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                                let _ = ws::write_ws_close(
                                    &mut writer,
                                    ws::WS_CLOSE_INVALID_MESSAGE,
                                    "invalid_message",
                                )
                                .await;
                                return;
                            }
                        };
                        match request {
                            ws::WsClientRequest::SessionRead { session_id } => {
                                let access = state.sessions.get_for(&principal, &session_id).await;
                                if !ensure_ws_principal_current(
                                    &mut writer,
                                    &state,
                                    &principal,
                                    secure_transport,
                                )
                                .await
                                {
                                    return;
                                }
                                match access {
                                    Ok(sessions::SessionAccess::Ok(facts)) => {
                                        let message = ws::WsServerMessage::SessionReadResult {
                                            session_id: facts.session_id,
                                            run_count: facts.run_count,
                                        };
                                        if let Err(err) =
                                            ws::write_ws_text(&mut writer, &canon::canonical_bytes(&message)).await
                                        {
                                            tracing::warn!(%err, "cannot send ws reply");
                                            return;
                                        }
                                    }
                                    Ok(sessions::SessionAccess::NotFound) => {
                                        let error =
                                            ProtocolError::new(ErrorCode::NotFound, "session not found", false);
                                        let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                                        let _ =
                                            ws::write_ws_close(&mut writer, ws::WS_CLOSE_NOT_FOUND, "not_found").await;
                                        return;
                                    }
                                    Ok(sessions::SessionAccess::Forbidden) => {
                                        let error = ProtocolError::new(
                                            ErrorCode::Forbidden,
                                            "session belongs to another principal",
                                            false,
                                        )
                                        .with_details(serde_json::Map::from_iter([(
                                            "reason".to_string(),
                                            serde_json::Value::String("cross_principal_access".to_string()),
                                        )]));
                                        let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                                        let _ =
                                            ws::write_ws_close(&mut writer, ws::WS_CLOSE_FORBIDDEN, "forbidden").await;
                                        return;
                                    }
                                    Err(err) => {
                                        let error = ProtocolError::new(
                                            ErrorCode::Internal,
                                            format!("run database read failed: {err}"),
                                            false,
                                        );
                                        let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                                        let _ = ws::write_ws_close(&mut writer, 1011, "internal").await;
                                        return;
                                    }
                                }
                            }
                            ws::WsClientRequest::SubscribeEvents { stream_id, cursor } => {
                                if subscription.is_some() {
                                    let error = ProtocolError::new(
                                        ErrorCode::InvalidMessage,
                                        "this connection already holds an active event subscription",
                                        false,
                                    )
                                    .with_details(serde_json::Map::from_iter([(
                                        "reason".to_string(),
                                        serde_json::Value::String("already_subscribed".to_string()),
                                    )]));
                                    let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                                    let _ = ws::write_ws_close(
                                        &mut writer,
                                        ws::WS_CLOSE_INVALID_MESSAGE,
                                        "invalid_message",
                                    )
                                    .await;
                                    return;
                                }
                                match state.events.subscribe(&principal, &stream_id, cursor).await {
                                    Ok(events::SubscribeOutcome::Started { cut, subscription: sub }) => {
                                        if !ensure_ws_principal_current(
                                            &mut writer,
                                            &state,
                                            &principal,
                                            secure_transport,
                                        )
                                        .await
                                        {
                                            return;
                                        }
                                        // Control frame first (explicit boundary), then the
                                        // snapshot cut, then live frames from the mailbox.
                                        if let Err(err) = ws::write_ws_text(
                                            &mut writer,
                                            events::control_subscribed_json(&cut).as_bytes(),
                                        )
                                        .await
                                        {
                                            tracing::warn!(%err, "cannot send subscribed control frame");
                                            return;
                                        }
                                        for envelope in &cut.events {
                                            if !ensure_ws_principal_current(
                                                &mut writer,
                                                &state,
                                                &principal,
                                                secure_transport,
                                            )
                                            .await
                                            {
                                                return;
                                            }
                                            if let Err(err) =
                                                ws::write_ws_text(&mut writer, &canon::canonical_bytes(envelope)).await
                                            {
                                                tracing::warn!(%err, "cannot send snapshot event");
                                                return;
                                            }
                                        }
                                        subscription = Some(sub);
                                    }
                                    Ok(events::SubscribeOutcome::RequiresSnapshot(required)) => {
                                        if !ensure_ws_principal_current(
                                            &mut writer,
                                            &state,
                                            &principal,
                                            secure_transport,
                                        )
                                        .await
                                        {
                                            return;
                                        }
                                        // Explicit rebuild directive: NOT an error close — the
                                        // connection stays for the resubscribe-after-snapshot.
                                        if let Err(err) = ws::write_ws_text(
                                            &mut writer,
                                            events::control_snapshot_required_json(
                                                &required.stream_id,
                                                required.floor,
                                                required.reason,
                                            )
                                            .as_bytes(),
                                        )
                                        .await
                                        {
                                            tracing::warn!(%err, "cannot send snapshot_required frame");
                                            return;
                                        }
                                    }
                                    Err(reject) => {
                                        let (error, close_code, close_reason) = match &reject {
                                            events::SubscribeReject::StreamNotFound { stream_id } => (
                                                ProtocolError::new(
                                                    ErrorCode::NotFound,
                                                    format!("event stream {stream_id} not found (stale stream)"),
                                                    false,
                                                )
                                                .with_details(error_details("stream_not_found", "events.stream_not_found")),
                                                ws::WS_CLOSE_NOT_FOUND,
                                                "not_found",
                                            ),
                                            events::SubscribeReject::Forbidden { stream_id } => (
                                                ProtocolError::new(
                                                    ErrorCode::Forbidden,
                                                    format!("event stream {stream_id} belongs to another principal"),
                                                    false,
                                                )
                                                .with_details(error_details("cross_principal_access", "events.cross_principal_access")),
                                                ws::WS_CLOSE_FORBIDDEN,
                                                "forbidden",
                                            ),
                                            events::SubscribeReject::MalformedCursor { detail } => (
                                                ProtocolError::new(
                                                    ErrorCode::InvalidMessage,
                                                    format!("malformed cursor: {detail}"),
                                                    false,
                                                )
                                                .with_details(error_details("malformed_cursor", "events.malformed_cursor")),
                                                ws::WS_CLOSE_INVALID_MESSAGE,
                                                "invalid_message",
                                            ),
                                            events::SubscribeReject::StaleStreamCursor { stream_id, cursor_stream } => (
                                                ProtocolError::new(
                                                    ErrorCode::InvalidMessage,
                                                    format!(
                                                        "cursor was issued for stream {cursor_stream:?}, \
                                                         not {stream_id:?} (stale stream cursor)"
                                                    ),
                                                    false,
                                                )
                                                .with_details(error_details("stale_stream_cursor", "events.stale_stream_cursor")),
                                                ws::WS_CLOSE_INVALID_MESSAGE,
                                                "invalid_message",
                                            ),
                                            events::SubscribeReject::FutureCursor { seq, head, .. } => (
                                                ProtocolError::new(
                                                    ErrorCode::InvalidMessage,
                                                    format!(
                                                        "cursor seq {seq} is beyond the committed head \
                                                         {head} (future cursor)"
                                                    ),
                                                    false,
                                                )
                                                .with_details(error_details("future_cursor", "events.future_cursor")),
                                                ws::WS_CLOSE_INVALID_MESSAGE,
                                                "invalid_message",
                                            ),
                                            events::SubscribeReject::SubscriberLimit { scope, limit, .. } => (
                                                ProtocolError::new(
                                                    ErrorCode::BudgetExceeded,
                                                    format!(
                                                        "event subscriber limit reached \
                                                         ({scope} cap {limit})"
                                                    ),
                                                    false,
                                                )
                                                .with_details(error_details("subscriber_limit", "events.subscriber_limit")),
                                                WS_CLOSE_TRY_AGAIN_LATER,
                                                "try_again_later",
                                            ),
                                            events::SubscribeReject::Storage(err) => (
                                                ProtocolError::new(
                                                    ErrorCode::Internal,
                                                    format!("run database read failed: {err}"),
                                                    false,
                                                )
                                                .with_details(error_details(
                                                    "db_failure",
                                                    &EndpointError::storage_cause_id(err),
                                                )),
                                                1011,
                                                "internal",
                                            ),
                                        };
                                        let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                                        let _ =
                                            ws::write_ws_close(&mut writer, close_code, close_reason).await;
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Ok(Some(ws::WsFrame::Ping(payload))) => {
                        if let Err(err) = ws::write_ws_frame(&mut writer, 0xA, &payload).await {
                            tracing::warn!(%err, "cannot send pong");
                            return;
                        }
                    }
                    Ok(Some(ws::WsFrame::Pong)) => {}
                    Ok(Some(ws::WsFrame::Close(code, reason))) => {
                        let _ = ws::write_ws_close(&mut writer, code, &reason).await;
                        return;
                    }
                    Ok(None) => return,
                    Err(err) => {
                        tracing::warn!(%err, "ws session read failed");
                        if err.kind() == std::io::ErrorKind::InvalidData {
                            let _ = ws::write_ws_close(&mut writer, 1002, "protocol_error").await;
                        }
                        return;
                    }
                }
            }
            frame = sub_frame => {
                let Some(frame) = frame else {
                    // Mailbox closed (subscription dropped): stop racing it.
                    subscription = None;
                    continue;
                };
                if !ensure_ws_principal_current(&mut writer, &state, &principal, secure_transport).await {
                    return;
                }
                let payload_bytes = match &frame {
                    events::SubscriptionFrame::Event(envelope) => {
                        canon::canonical_bytes(envelope.as_ref())
                    }
                    events::SubscriptionFrame::SnapshotRequired { reason } => {
                        let stream_id = subscription
                            .as_ref()
                            .and_then(|s| state.events.hub().subscriber_stats(s.subscriber_id()))
                            .map(|stats| stats.stream_id)
                            .unwrap_or_default();
                        events::control_snapshot_required_json(
                            &stream_id,
                            None,
                            reason.wire_reason(),
                        )
                        .into_bytes()
                    }
                };
                if let Err(err) = ws::write_ws_text(&mut writer, &payload_bytes).await {
                    tracing::warn!(%err, "cannot deliver subscription frame");
                    return;
                }
                if matches!(frame, events::SubscriptionFrame::SnapshotRequired { .. }) {
                    // Detached: the client must rebuild from a snapshot and
                    // may resubscribe on this connection.
                    subscription = None;
                }
            }
        }
    }
    }
    .await;
    // 所有 return 分支先离开内部会话，再中止并等待读半边。管理层的
    // 会话计数 guard 此时仍在，故关停不会把未回收的读任务误判为已排空。
    if !exited_by_shutdown {
        drain_ws_after_server_close(&mut frame_reader, &mut shutdown_rx).await;
    }
    frame_reader.shutdown().await;
}

/// 服务端主动发送 close 帧后、最终丢弃连接前的入站排空宽限：收到
/// close 的合规客户端应尽快回送 close 或停止发送；不回送的对端最多
/// 占用这段宽限（连接槽保持有界），到期后仍按期限丢弃连接。
const WS_CLOSE_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// 服务端主动关闭的统一干净收尾（R02 收口 group 11 根因修复）。此前
/// 写完错误帧 + close 帧后各分支直接 `return`，会话任务随即丢弃
/// upgraded IO：若对端此刻还有未读入站数据（或 close 之后仍在写入），
/// 内核把这次关闭变成 RST，而对端 TCP 栈收到 RST 会连同接收队列里
/// 已经送达、尚未读走的错误帧/close 帧一起丢弃，读侧表现为
/// `ConnectionReset` —— 即 a05 撤销用例偶发失败的两条竞态形态
/// （消息边界复查 vs 1s 定时复查只是触发点不同，关闭形态同根）。
/// 修复：close 之后不立刻丢弃，继续消费并丢弃后续入站帧，直到对端
/// 回送 close（完成 close 握手）、断开、收到关停广播或宽限到期。
async fn drain_ws_after_server_close(
    frame_reader: &mut ws::ClientFrameReader,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
) {
    let deadline = tokio::time::Instant::now() + WS_CLOSE_DRAIN_GRACE;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return,
            changed = shutdown_rx.changed() => {
                // 关停广播（或 sender 消失）：预算内进程正在退出，立即让出。
                let _ = changed;
                return;
            }
            frame = frame_reader.recv() => match frame {
                // 对端 close（close 握手完成）、EOF 或读任务已结束：对端
                // 不再发送，丢弃连接不会再制造未读数据。
                Ok(Some(ws::WsFrame::Close(..))) | Ok(None) | Err(_) => return,
                // 其余帧（text/ping/pong）只消费并丢弃，不回应。
                Ok(Some(_)) => {}
            },
        }
    }
}

/// Machine reason detail block for subscribe rejections, with the
/// structured `causeId` of the R02-T07 error vocabulary.
fn error_details(reason: &str, cause_id: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([
        (
            "reason".to_string(),
            serde_json::Value::String(reason.to_string()),
        ),
        (
            "causeId".to_string(),
            serde_json::Value::String(cause_id.to_string()),
        ),
    ])
}

/// Builds the full HTTP router with all injected state: public health plus
/// the authenticated business surface. Layer order (outermost last):
/// request-id/error enrichment → HTTP admission (F07 in-flight cap +
/// per-request budget) → transport guard → auth guard → routes, with the
/// body limit applied at the extraction boundary.
pub fn build_router(state: ServiceState) -> Router {
    Router::new()
        .route("/lingxi/v1/health", get(health))
        .route("/lingxi/v1/me", get(me))
        .route("/lingxi/v1/sessions", get(list_sessions))
        .route("/lingxi/v1/sessions/{session_id}", get(get_session))
        .route(
            "/lingxi/v1/sessions/{session_id}/execute",
            post(execute_session),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/events",
            get(session_events_page),
        )
        .route("/lingxi/v1/ws-ticket", post(ws_ticket))
        .route(
            "/lingxi/v1/devices/credentials",
            post(issue_device_credential),
        )
        .route("/lingxi/v1/ws", get(ws_handler))
        .merge(management::routes())
        .merge(static_web::routes())
        .layer(axum::extract::DefaultBodyLimit::max(
            limits::DEFAULT_BODY_LIMIT_BYTES,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), auth_guard))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            transport_guard,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            http_admission,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            error_enrichment,
        ))
        .with_state(state)
}

/// Runtime errors of the serving loop.
#[derive(Debug)]
pub enum ServiceError {
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    Serve {
        source: std::io::Error,
    },
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceError::Bind { addr, source } => {
                write!(f, "cannot listen on {addr}: {source}")
            }
            ServiceError::Serve { source } => write!(f, "server failed: {source}"),
        }
    }
}

impl std::error::Error for ServiceError {}

/// Outcome of the serving loop (R02 stage-repair R1 / F03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServeOutcome {
    /// The transport drain exceeded the caller's budget: in-flight
    /// connections were abandoned instead of draining (the shutdown
    /// coordinator records the timeout; the binary's deterministic
    /// `process::exit` bounds whatever wedged). `false` = every connection
    /// drained cleanly within the budget.
    pub drain_timed_out: bool,
}

/// Binds, reports the concrete local address through `on_ready`, and serves
/// until `shutdown` resolves (SIGINT/SIGTERM in the binary; a test-controlled
/// future in the harness). In-flight requests drain before returning —
/// bounded by `drain_budget` when one is given (R02 stage-repair R1 / F03:
/// the pre-fix drain had NO deadline, so a single stuck connection — e.g. a
/// partial request body — held the process past every configured shutdown
/// timeout before any coordinator phase even started). The serve future is
/// LAZY, so the signal is consumed by the graceful-shutdown future itself
/// (which records the signal instant); a watchdog armed at that instant races
/// the drain and abandons it loudly when the budget expires — the accept loop
/// keeps polling normally while serving.
///
/// Note (R02-T02): single-writer locking and the instance record are owned
/// by the CALLER (`instance::acquire` + `InstanceGuard::publish` inside
/// `on_ready`). R02-T03: the router state (auth included) is assembled by
/// the caller via [`ServiceState::bootstrap`]; this function is
/// transport-only and never builds an auth-less router.
pub async fn run<F>(
    state: ServiceState,
    shutdown: F,
    on_ready: impl FnOnce(SocketAddr),
    drain_budget: Option<std::time::Duration>,
) -> Result<ServeOutcome, ServiceError>
where
    F: Future<Output = ()> + Send + 'static,
{
    run_with_tls(state, shutdown, on_ready, drain_budget, None).await
}

/// 通过真实 TLS 握手服务请求；未配置时保持原有 HTTP 行为。
pub async fn run_with_tls<F>(
    state: ServiceState,
    shutdown: F,
    on_ready: impl FnOnce(SocketAddr),
    drain_budget: Option<std::time::Duration>,
    tls_acceptor: Option<tokio_rustls::TlsAcceptor>,
) -> Result<ServeOutcome, ServiceError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let bind_addr = state.config().bind_addr;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|source| ServiceError::Bind {
            addr: bind_addr,
            source,
        })?;
    let local = listener.local_addr().map_err(|source| ServiceError::Bind {
        addr: bind_addr,
        source,
    })?;
    *state
        .bound_addr
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(local);
    on_ready(local);

    // R2-F04: the serving loop enforces CONNECTION-level admission at the
    // accept edge (count cap) and a header-read budget per connection
    // (time cap) — axum's `serve` exposes neither, so the loop lives in
    // `crate::serve` with the axum graceful-drain semantics preserved.
    // Both budgets come from the SAME knobs as the request-level gate
    // (`--http-max-in-flight` / `--http-request-budget-ms`).
    let conn_admission = Arc::clone(state.connection_admission());
    let header_budget = state.admission().request_budget();
    let router = build_router(state);
    // The graceful-shutdown future consumes the stop signal and records the
    // signal instant; the drain budget watchdog is armed at THAT instant (the
    // unified from-signal budget — F03) and races the drain. Serving therefore
    // polls the accept loop from the start; dropping the serve future on a
    // watchdog win abandons whatever the drain could not finish.
    let (signal_tx, signal_rx) = tokio::sync::oneshot::channel::<std::time::Instant>();
    let graceful = async move {
        shutdown.await;
        let _ = signal_tx.send(std::time::Instant::now());
    };
    let server = serve::serve_with_connection_admission(
        listener,
        router,
        graceful,
        conn_admission,
        header_budget,
        tls_acceptor,
    );
    tokio::pin!(server);
    match drain_budget {
        None => server
            .await
            .map(|()| ServeOutcome {
                drain_timed_out: false,
            })
            .map_err(|source| ServiceError::Serve { source }),
        Some(budget) => {
            let watchdog = async {
                let _ = signal_rx.await;
                tokio::time::sleep(budget).await;
            };
            tokio::select! {
                biased;
                result = &mut server => result
                    .map(|()| ServeOutcome {
                        drain_timed_out: false,
                    })
                    .map_err(|source| ServiceError::Serve { source }),
                () = watchdog => {
                    eprintln!(
                        "{} phase=transport_drain budget_ms={}",
                        shutdown::SHUTDOWN_TIMEOUT_MARKER,
                        budget.as_millis()
                    );
                    tracing::error!(
                        phase = "transport_drain",
                        budget_ms = budget.as_millis() as u64,
                        "shutdown drain exceeded its budget; abandoning in-flight \
                         connections (bounded at process level by the deterministic exit)"
                    );
                    Ok(ServeOutcome {
                        drain_timed_out: true,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn storage_http_errors_keep_status_reason_and_retryability() {
        for (failure, status, reason, retryable, cause) in [
            (
                StorageError::QueueFull,
                StatusCode::SERVICE_UNAVAILABLE,
                "db_queue_full",
                true,
                "storage.queue_full",
            ),
            (
                StorageError::Busy { timeout_ms: 20 },
                StatusCode::SERVICE_UNAVAILABLE,
                "db_busy",
                true,
                "storage.busy",
            ),
            (
                StorageError::DiskFull {
                    detail: "injected write failure".to_string(),
                },
                StatusCode::SERVICE_UNAVAILABLE,
                "db_disk_full",
                true,
                "storage.disk_full",
            ),
            (
                StorageError::QueueClosed,
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_failure",
                false,
                "storage.queue_closed",
            ),
            (
                StorageError::Io {
                    detail: "injected write failure".to_string(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_failure",
                false,
                "storage.io",
            ),
        ] {
            let response = EndpointError::storage(&failure).into_response();
            assert_eq!(response.status(), status, "{failure:?}");
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .expect("read error response");
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["code"], "internal", "{failure:?}: {body}");
            assert_eq!(body["details"]["reason"], reason, "{failure:?}: {body}");
            assert_eq!(
                body["details"]["retryable"], retryable,
                "{failure:?}: {body}"
            );
            assert_eq!(body["details"]["causeId"], cause, "{failure:?}: {body}");
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn config_requires_explicit_home() {
        let err = ServiceConfig::from_cli_args(args(&[])).unwrap_err();
        assert_eq!(err, ConfigError::MissingHome);
    }

    #[test]
    fn config_rejects_relative_home() {
        let err = ServiceConfig::from_cli_args(args(&["--home", "relative/dir"])).unwrap_err();
        assert!(matches!(err, ConfigError::RelativeHome { .. }));
    }

    #[test]
    fn config_rejects_unknown_and_missing_values() {
        assert!(matches!(
            ServiceConfig::from_cli_args(args(&["--home", "/tmp/x", "--lan"])),
            Err(ConfigError::UnknownArgument { .. })
        ));
        assert!(matches!(
            ServiceConfig::from_cli_args(args(&["--home"])),
            Err(ConfigError::MissingArgumentValue { .. })
        ));
        assert!(matches!(
            ServiceConfig::from_cli_args(args(&["--bind"])),
            Err(ConfigError::MissingArgumentValue { .. })
        ));
    }

    #[test]
    fn config_parses_bind_and_home() {
        let cfg =
            ServiceConfig::from_cli_args(args(&["--bind", "127.0.0.1:8080", "--home", "/tmp/h"]))
                .unwrap();
        assert_eq!(cfg.bind_addr, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(cfg.data_home, PathBuf::from("/tmp/h"));
        assert_eq!(cfg.home_source, HomeSource::Cli);
        assert_eq!(cfg.network_mode, NetworkMode::Loopback);
    }

    #[test]
    fn config_defaults_to_loopback_ephemeral() {
        let cfg = ServiceConfig::from_cli_args(args(&["--home", "/tmp/h"])).unwrap();
        assert!(cfg.bind_addr.ip().is_loopback());
        assert_eq!(cfg.bind_addr.port(), 0);
        assert_eq!(cfg.network_mode, NetworkMode::Loopback);
    }

    #[test]
    fn config_network_mode_is_explicit_only() {
        // LAN mode requires the explicit flag.
        let cfg = ServiceConfig::from_cli_args(args(&[
            "--home",
            "/tmp/h",
            "--network-mode",
            "lan",
            "--bind",
            "0.0.0.0:8080",
        ]))
        .unwrap();
        assert_eq!(cfg.network_mode, NetworkMode::Lan);

        // Default mode + non-loopback bind is a loud error, never silent.
        let err =
            ServiceConfig::from_cli_args(args(&["--home", "/tmp/h", "--bind", "0.0.0.0:8080"]))
                .unwrap_err();
        assert!(matches!(err, ConfigError::NetworkModeBindMismatch { .. }));

        // Strict vocabulary.
        let err =
            ServiceConfig::from_cli_args(args(&["--home", "/tmp/h", "--network-mode", "internet"]))
                .unwrap_err();
        assert!(matches!(err, ConfigError::BadNetworkMode { .. }));
    }

    #[test]
    fn config_rejects_garbage_bind() {
        let err =
            ServiceConfig::from_cli_args(args(&["--bind", "not an addr", "--home", "/tmp/h"]))
                .unwrap_err();
        assert!(matches!(err, ConfigError::BadBind { .. }));
    }

    #[test]
    fn health_response_sources_versions_from_protocol() {
        // Single-source check: the constants must come from lingxi-protocol,
        // not be re-typed here (drift between the two would be a contract bug).
        assert_eq!(WIRE_PROTOCOL_MIN_SUPPORTED, 1);
        assert_eq!(WIRE_PROTOCOL_MAX_SUPPORTED, 1);
        assert_eq!(ContractVersions::R00_BASELINE.data_epoch, 1);
    }

    #[test]
    fn health_response_serializes_minimal_camel_case() {
        // The response intentionally carries no paths, no instance ids, no
        // configuration echo: minimal public surface.
        let json = serde_json::to_value(health_payload()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "status": "ok",
                "serverKind": SERVER_KIND,
                "serverVersion": server_version(),
                "wireProtocolMin": 1,
                "wireProtocolMax": 1,
                "dataEpoch": 1
            })
        );
    }

    /// R3-F02: the composition root rejects degenerate resource knobs
    /// loudly (the same exit-2 class the CLI parse layer produces) instead
    /// of clamping/saturating them silently — the pre-fix `saturating_mul`
    /// and `.max(1)` fallbacks are unreachable from any validated input.
    #[test]
    fn resource_deps_validation_is_loud_for_degenerate_knobs() {
        let base = ServiceDeps::default();
        let cases: Vec<(&str, ServiceDeps)> = vec![
            (
                "rate_window_ms 0 (would disable the limit silently)",
                ServiceDeps {
                    rate_window_ms: 0,
                    ..base.clone()
                },
            ),
            (
                "rate_max 0 (the pre-fix truncated-4294967296 state)",
                ServiceDeps {
                    rate_max: 0,
                    ..base.clone()
                },
            ),
            (
                "ws_max_connections 0",
                ServiceDeps {
                    ws_max_connections: 0,
                    ..base.clone()
                },
            ),
            (
                "ws_max_tickets 0",
                ServiceDeps {
                    ws_max_tickets: 0,
                    ..base.clone()
                },
            ),
            (
                "ws_ticket_ttl_ms 0",
                ServiceDeps {
                    ws_ticket_ttl_ms: 0,
                    ..base.clone()
                },
            ),
            (
                "http_max_in_flight 0",
                ServiceDeps {
                    http_max_in_flight: 0,
                    ..base.clone()
                },
            ),
            (
                "http_max_in_flight above the 2x derivation bound",
                ServiceDeps {
                    http_max_in_flight: usize::MAX,
                    ..base.clone()
                },
            ),
            (
                "http_request_budget_ms 0",
                ServiceDeps {
                    http_request_budget_ms: 0,
                    ..base.clone()
                },
            ),
            (
                "http_request_budget_ms above the 30-day platform-safe bound",
                ServiceDeps {
                    http_request_budget_ms: crate::config::MAX_TIME_BUDGET_MS + 1,
                    ..base.clone()
                },
            ),
            (
                "quota model global limit 0 (would disable admission silently)",
                ServiceDeps {
                    quota_limits: quotas::QuotaLimits {
                        model: quotas::LayeredQuotaLimits {
                            global: 0,
                            per_agent: 1,
                            per_session: 1,
                        },
                        ..base.quota_limits
                    },
                    ..base.clone()
                },
            ),
            (
                "quota tool per-session limit 0 (would deadlock the first tool call)",
                ServiceDeps {
                    quota_limits: quotas::QuotaLimits {
                        tool: quotas::LayeredQuotaLimits {
                            global: 1,
                            per_agent: 1,
                            per_session: 0,
                        },
                        ..base.quota_limits
                    },
                    ..base.clone()
                },
            ),
            (
                "quota wait_queue_capacity 0 (unbounded-wait disguise)",
                ServiceDeps {
                    quota_limits: quotas::QuotaLimits {
                        wait_queue_capacity: 0,
                        ..base.quota_limits
                    },
                    ..base.clone()
                },
            ),
            (
                "quota wait_timeout_ms above the platform-safe bound",
                ServiceDeps {
                    quota_limits: quotas::QuotaLimits {
                        wait_timeout_ms: crate::config::MAX_TIME_BUDGET_MS + 1,
                        ..base.quota_limits
                    },
                    ..base.clone()
                },
            ),
            (
                "session steering_inbox_capacity 0 (would silently disable steering)",
                ServiceDeps {
                    session_concurrency: session_supervisor::SessionConcurrencyLimits {
                        steering_inbox_capacity: 0,
                        registry_cap: 8,
                    },
                    ..base.clone()
                },
            ),
            (
                "session registry_cap 0 (would reject every session)",
                ServiceDeps {
                    session_concurrency: session_supervisor::SessionConcurrencyLimits {
                        steering_inbox_capacity: 8,
                        registry_cap: 0,
                    },
                    ..base.clone()
                },
            ),
        ];
        for (name, deps) in cases {
            let err = validate_resource_deps(&deps)
                .expect_err(&format!("{name} must be a loud startup error"));
            assert!(
                matches!(
                    err,
                    ServiceStartupError::Storage(StorageError::InvalidRequest { .. })
                ),
                "{name}: unexpected error class: {err}"
            );
        }
        // The production defaults are (and must stay) valid.
        validate_resource_deps(&base).expect("the production defaults are valid");
        // The boundary of the 2x derivation is valid (checked_mul cannot
        // overflow at usize::MAX/2).
        let boundary = ServiceDeps {
            http_max_in_flight: usize::MAX / 2,
            ..base.clone()
        };
        validate_resource_deps(&boundary).expect("usize::MAX/2 is the valid boundary");
    }

    // ── group 11：服务端主动 close 后的统一干净收尾（确定性单测）──────
    //
    // 集成层只能以竞态触发「close 时入站尚未读取」（服务端调度不可从
    // 黑盒对端控制），排空机制本身在这里用内存双工管道确定性验证。

    /// 写一个带掩码的客户端帧（RFC 6455 §5.1：客户端帧必须掩码）。
    async fn send_masked_client_frame(
        io: &mut tokio::io::DuplexStream,
        opcode: u8,
        payload: &[u8],
    ) {
        use tokio::io::AsyncWriteExt as _;
        let mask = [0x11_u8, 0x22, 0x33, 0x44];
        let mut out = vec![0x80 | opcode, 0x80 | payload.len() as u8];
        out.extend_from_slice(&mask);
        out.extend(
            payload
                .iter()
                .enumerate()
                .map(|(i, byte)| byte ^ mask[i % 4]),
        );
        io.write_all(&out).await.expect("write masked client frame");
    }

    #[tokio::test]
    async fn ws_drain_consumes_late_frames_and_ends_on_client_close() {
        let (mut client, server) = tokio::io::duplex(4096);
        let (reader, _writer) = tokio::io::split(server);
        let mut framed = ws::ClientFrameReader::new(reader);
        let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

        // 「客户端已发送帧但服务端尚未处理」的迟到形态：text + ping。
        send_masked_client_frame(&mut client, 0x1, b"{\"type\":\"session_read\"}").await;
        send_masked_client_frame(&mut client, 0x9, b"late-ping").await;

        let drain = tokio::spawn(async move {
            drain_ws_after_server_close(&mut framed, &mut shutdown_rx).await
        });
        // 迟到帧必须被消费丢弃，排空不得因 text/ping 提前结束。
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !drain.is_finished(),
            "drain must outlive late text/ping frames"
        );

        // 对端回送 close（close 握手完成）→ 排空立即结束。
        let close_payload = 1000_u16.to_be_bytes().to_vec();
        send_masked_client_frame(&mut client, 0x8, &close_payload).await;
        tokio::time::timeout(std::time::Duration::from_secs(2), drain)
            .await
            .expect("drain ends on client close")
            .expect("drain task join");
    }

    #[tokio::test]
    async fn ws_drain_ends_on_shutdown_broadcast_and_peer_eof() {
        use tokio::io::AsyncWriteExt as _;
        // 关停广播：进程正在退出，排空必须立即让出（不等宽限也不等 close）。
        {
            let (_client, server) = tokio::io::duplex(64);
            let (reader, _writer) = tokio::io::split(server);
            let mut framed = ws::ClientFrameReader::new(reader);
            let (tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
            let drain = tokio::time::timeout(std::time::Duration::from_secs(2), async move {
                let notify = tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    tx.send(true).expect("shutdown broadcast");
                });
                drain_ws_after_server_close(&mut framed, &mut shutdown_rx).await;
                notify.await.expect("notify task join");
            });
            drain.await.expect("drain yields on shutdown broadcast");
        }
        // 对端直接断开（EOF）：不再有入站来源，排空立即结束。
        {
            let (mut client, server) = tokio::io::duplex(64);
            let (reader, _writer) = tokio::io::split(server);
            let mut framed = ws::ClientFrameReader::new(reader);
            let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
            client.shutdown().await.expect("client half-close");
            tokio::time::timeout(std::time::Duration::from_secs(2), async move {
                drain_ws_after_server_close(&mut framed, &mut shutdown_rx).await
            })
            .await
            .expect("drain ends on peer EOF");
        }
    }
}
