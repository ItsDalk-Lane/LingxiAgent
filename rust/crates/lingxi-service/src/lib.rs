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

pub mod auth;
pub mod config;
pub mod instance;
pub mod limits;
pub mod paths;
pub mod sessions;
pub mod transport;
pub mod ws;

pub use auth::{
    authorize as authorize_route, classify_route, scope_allows, AuthDenial, AuthService,
    AuthSetupError, AuthzDenial, CredentialKind, IssuedDeviceCredential, Principal, PrincipalKind,
    RoutePolicy, TrustState, LOCAL_OWNER_USER_ID,
};
pub use config::{
    parse_cli, read_config_home, resolve_effective_home, CliOptions, ConfigError, HomeSource,
    IgnoredHomeSource, ResolvedHome, HOME_ENV_VAR,
};
pub use instance::{
    acquire, probe_peer, InstanceGuard, InstanceIdentity, InstanceLockError, InstanceRecord,
    PeerProbe, SINGLE_WRITER_BLOCKED_MARKER, STALE_RECORD_MARKER,
};
pub use paths::{prepare_layout, DataRootLayout};
pub use sessions::{
    ExecuteAccepted, ExecuteRequest, RunSummary, SessionAccess, SessionBackend,
    SessionExecuteError, SessionFacts, SessionStore, SessionView,
};
pub use transport::{check_origin, infer_connection_kind, ConnectionKind, NetworkMode};
pub use ws::{WsTicketService, WS_CLOSE_FORBIDDEN, WS_CLOSE_UNAUTHORIZED};

use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lingxi_adapters::storage::{RunDatabase, StoreOptions, RUNS_DB_FILE_NAME};
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
        let bind_raw = cli
            .bind
            .clone()
            .unwrap_or_else(|| Self::DEFAULT_BIND.to_string());
        let bind_addr: SocketAddr =
            bind_raw
                .parse::<SocketAddr>()
                .map_err(|source| ConfigError::BadBind {
                    value: bind_raw.clone(),
                    source: source.to_string(),
                })?;
        let data_home = resolved.path;
        if !data_home.is_absolute() {
            return Err(ConfigError::RelativeHome { value: data_home });
        }
        let network_mode = cli
            .network_mode
            .as_deref()
            .map(|value| {
                NetworkMode::parse(value).ok_or(ConfigError::BadNetworkMode {
                    value: value.to_string(),
                })
            })
            .transpose()?
            .unwrap_or(NetworkMode::Loopback);
        if network_mode == NetworkMode::Loopback && !bind_addr.ip().is_loopback() {
            return Err(ConfigError::NetworkModeBindMismatch {
                bind: bind_addr,
                mode: NetworkMode::Loopback,
            });
        }
        Ok(Self {
            bind_addr,
            data_home,
            home_source: resolved.source,
            network_mode,
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
        if self.data_home.exists() && !self.data_home.is_dir() {
            return Err(ConfigError::HomeIsNotADirectory {
                value: self.data_home.clone(),
            });
        }
        if !self.data_home.exists() {
            std::fs::create_dir_all(&self.data_home).map_err(|source| {
                ConfigError::HomeCreateFailed {
                    value: self.data_home.clone(),
                    source: source.to_string(),
                }
            })?;
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
/// (auth / tickets / storage / sessions / limits) live here — events
/// arrive with R02-T05.
#[derive(Clone)]
pub struct ServiceState {
    config: Arc<ServiceConfig>,
    auth: Arc<AuthService>,
    tickets: Arc<WsTicketService>,
    storage: Arc<RunDatabase>,
    sessions: Arc<sessions::SessionStore>,
    rate: Arc<limits::RateLimiter>,
    ws_conns: Arc<limits::WsConnectionCounter>,
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
        let identity = instance::InstanceIdentity::generate();
        let auth = AuthService::bootstrap(layout, &identity.instance_id)
            .map_err(ServiceStartupError::Auth)?;
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
        let storage = RunDatabase::open(&db_path, store_options)
            .await
            .map_err(ServiceStartupError::Storage)?;
        storage
            .ensure_session_seed(sessions::SessionStore::seed_rows(auth::now_unix_ms()))
            .await
            .map_err(ServiceStartupError::Storage)?;
        let session_store = sessions::SessionStore::new(storage.clone());
        Ok(Self {
            config: Arc::new(config),
            auth: Arc::new(auth),
            tickets: Arc::new(WsTicketService::new(
                ws_ticket_ttl_ms,
                ws::DEFAULT_WS_MAX_TICKETS,
            )),
            storage: Arc::new(storage),
            sessions: Arc::new(session_store),
            rate: Arc::new(limits::RateLimiter::new(rate_window_ms, rate_max)),
            ws_conns: Arc::new(limits::WsConnectionCounter::new(ws_max_connections)),
        })
    }

    pub fn config(&self) -> &ServiceConfig {
        &self.config
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

    /// The injected storage port implementation (composition root handle
    /// for the close/checkpoint path).
    pub fn storage(&self) -> &Arc<RunDatabase> {
        &self.storage
    }

    pub fn ws_connection_count(&self) -> usize {
        self.ws_conns.current()
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
pub struct EndpointError {
    status: StatusCode,
    error: ProtocolError,
}

impl EndpointError {
    fn new(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            error: ProtocolError::new(code, message, false),
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
    }

    pub fn forbidden(reason: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            "not allowed for this principal",
        )
        .with_reason(reason)
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
    }

    pub fn invalid_transport(reason: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            "transport policy rejected the request",
        )
        .with_reason(reason)
    }

    pub fn rate_limited() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::Forbidden,
            "rate limit exceeded",
        )
        .with_reason("rate_limited")
    }

    pub fn payload_too_large(detail: String) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::InvalidMessage,
            format!("request body exceeds the configured limit: {detail}"),
        )
        .with_reason("body_limit_exceeded")
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
    }

    /// Maps a storage-port failure (R02-A07: no fake success). Retryable
    /// backpressure (bounded queue full / busy) answers 503; every other
    /// storage failure is a 500. Both carry the machine reason.
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
        );
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
}

impl IntoResponse for EndpointError {
    fn into_response(self) -> Response {
        let body = serde_json::to_value(&self.error).unwrap_or_else(|err| {
            // Serializing this plain struct cannot fail; if it ever does,
            // answer with a minimal valid protocol error instead of a
            // malformed body.
            tracing::error!(%err, "cannot serialize endpoint error");
            serde_json::json!({
                "code": "internal",
                "message": "error serialization failed",
                "retryable": false
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

async fn transport_guard(
    State(state): State<ServiceState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
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

    let reject = |status: StatusCode, reason: &str, resp: EndpointError| {
        eprintln!(
            "{TRANSPORT_REJECTED_MARKER} method={method} path={path} status={} reason={reason} \
             origin={} host={host} remote={remote}",
            status.as_u16(),
            origin.as_deref().unwrap_or("absent"),
        );
        resp
    };

    // Origin policy runs for EVERY route (public included): a browser that
    // speaks a foreign origin may not touch even the health surface.
    match check_origin(origin.as_deref()) {
        transport::OriginVerdict::Allowed | transport::OriginVerdict::Absent => {}
        transport::OriginVerdict::Forbidden => {
            return reject(
                StatusCode::FORBIDDEN,
                "bad_origin",
                EndpointError::invalid_transport("bad_origin"),
            )
            .into_response();
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
                )
                .into_response();
            }
        };

    // Rate limit (per remote peer, fixed window).
    if !state.rate.check(remote.ip(), auth::now_unix_ms()) {
        return reject(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            EndpointError::rate_limited(),
        )
        .into_response();
    }

    let mut req = req;
    req.extensions_mut().insert(connection_kind);
    req.extensions_mut().insert(remote);
    next.run(req).await
}

async fn auth_guard(State(state): State<ServiceState>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    if classify_route(&method, &path) == RoutePolicy::Public {
        return next.run(req).await;
    }

    let authorization = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let query = req.uri().query().unwrap_or("").to_string();
    let Some(connection_kind) = req.extensions().get::<ConnectionKind>().copied() else {
        // Unreachable when the transport guard is layered (it always
        // inserts the kind); fail closed rather than guess.
        eprintln!("{AUTH_REJECTED_MARKER} method={method} path={path} status=500 reason=missing_transport_context");
        return EndpointError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            "transport context missing",
        )
        .into_response();
    };

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
                    "/lingxi/v1/ws",
                    auth::now_unix_ms(),
                )
                .ok_or(AuthDenial {
                    reason: "invalid_ws_ticket",
                    credential_source: Some("ws_ticket"),
                    connection_kind,
                })
                .map(|principal| (principal, "ws_ticket")),
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
                tracing::debug!(%source, "ws upgrade authenticated");
                principal
            }
            Err(denial) => {
                eprintln!(
                    "{AUTH_REJECTED_MARKER} method={method} path={path} status=401 reason={} remote={remote}",
                    denial.reason
                );
                return EndpointError::unauthorized(denial.reason).into_response();
            }
        }
    } else {
        let query_token = ws::parse_query_pairs(&query)
            .into_iter()
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value);
        match state.auth.authenticate(
            authorization.as_deref(),
            query_token.as_deref(),
            true,
            connection_kind,
        ) {
            Ok(principal) => principal,
            Err(denial) => {
                eprintln!(
                    "{AUTH_REJECTED_MARKER} method={method} path={path} status=401 reason={} \
                     remote={}",
                    denial.reason,
                    req.extensions()
                        .get::<SocketAddr>()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                );
                return EndpointError::unauthorized(denial.reason).into_response();
            }
        }
    };

    // Route-level authorization (shared with the WS path: same table).
    if let Err(denial) = authorize_route(&method, &path, Some(&principal)) {
        eprintln!(
            "{AUTH_REJECTED_MARKER} method={method} path={path} status={} reason={} \
             remote={}",
            denial.status,
            denial.reason,
            req.extensions()
                .get::<SocketAddr>()
                .map(|a| a.to_string())
                .unwrap_or_else(|| "unknown".to_string())
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

async fn me(axum::Extension(principal): axum::Extension<Principal>) -> Response {
    // Server-computed identity echo: proves the principal comes from the
    // auth chain. Any client-supplied identity fields were already ignored
    // (the ExecuteRequest parser rejects unknown fields outright).
    (StatusCode::OK, Json(principal)).into_response()
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
    match state
        .sessions
        .execute_for(
            storage.as_ref(),
            &principal,
            &session_id,
            &request.input,
            auth::now_unix_ms(),
        )
        .await
    {
        Ok(accepted) => (StatusCode::OK, Json(accepted)).into_response(),
        Err(sessions::SessionExecuteError::NotFound) => EndpointError::not_found().into_response(),
        Err(sessions::SessionExecuteError::Forbidden) => {
            EndpointError::forbidden("cross_principal_access").into_response()
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
    State(state): State<ServiceState>,
) -> Response {
    let issued = state.tickets.issue(
        principal,
        connection_kind,
        "/lingxi/v1/ws",
        auth::now_unix_ms(),
    );
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
    ws_upgrade: Result<ws::WsUpgrade, ws::WsUpgradeRejection>,
) -> Response {
    let upgrade = match ws_upgrade {
        Ok(upgrade) => upgrade,
        Err(rejection) => {
            eprintln!(
                "{TRANSPORT_REJECTED_MARKER} method=GET path=/lingxi/v1/ws status={} reason={}",
                rejection.status, rejection.reason
            );
            return EndpointError::invalid_transport(rejection.reason).into_response();
        }
    };
    // Connection ceiling BEFORE the 101: a refused upgrade must not count.
    let Some(slot) = state.ws_conns_arc().acquire() else {
        return EndpointError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::BudgetExceeded,
            "websocket connection limit reached",
        )
        .with_reason("ws_connection_limit")
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
                run_ws_session(io, state_for_task, principal, slot).await;
            }
            Err(err) => {
                tracing::warn!(%err, "websocket upgrade failed after 101");
            }
        }
    });
    response
}

async fn run_ws_session<IO>(
    mut io: IO,
    state: ServiceState,
    principal: Principal,
    _slot: limits::WsConnectionGuard,
) where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // 1. lingxi.wire handshake: first text frame must be a ClientHello.
    match ws::read_ws_frame(&mut io).await {
        Ok(Some(ws::WsFrame::Text(bytes))) => {
            let hello: ClientHello = match serde_json::from_slice(&bytes) {
                Ok(hello) => hello,
                Err(err) => {
                    let error = ProtocolError::new(
                        ErrorCode::InvalidMessage,
                        format!("first frame is not a valid ClientHello: {err}"),
                        false,
                    );
                    let _ = ws::write_ws_text(&mut io, &canon::canonical_bytes(&error)).await;
                    let _ = ws::write_ws_close(
                        &mut io,
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
                        ws::write_ws_text(&mut io, &canon::canonical_bytes(&server_hello)).await
                    {
                        tracing::warn!(%err, "cannot send ServerHello");
                        return;
                    }
                }
                Err(error) => {
                    let _ = ws::write_ws_text(&mut io, &canon::canonical_bytes(&error)).await;
                    let _ = ws::write_ws_close(
                        &mut io,
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
                &mut io,
                ws::WS_CLOSE_INVALID_MESSAGE,
                "first frame must be a text ClientHello",
            )
            .await;
            return;
        }
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(%err, "ws handshake read failed");
            return;
        }
    }

    // 2. Request loop: per-message authorization through the SAME session
    //    ownership rule as the HTTP read endpoint.
    loop {
        match ws::read_ws_frame(&mut io).await {
            Ok(Some(ws::WsFrame::Text(bytes))) => {
                let request: ws::WsClientRequest = match serde_json::from_slice(&bytes) {
                    Ok(request) => request,
                    Err(err) => {
                        let error = ProtocolError::new(
                            ErrorCode::InvalidMessage,
                            format!("frame is not a valid ws request: {err}"),
                            false,
                        );
                        let _ = ws::write_ws_text(&mut io, &canon::canonical_bytes(&error)).await;
                        let _ = ws::write_ws_close(
                            &mut io,
                            ws::WS_CLOSE_INVALID_MESSAGE,
                            "invalid_message",
                        )
                        .await;
                        return;
                    }
                };
                let ws::WsClientRequest::SessionRead { session_id } = request;
                let access = state.sessions.get_for(&principal, &session_id).await;
                match access {
                    Ok(sessions::SessionAccess::Ok(facts)) => {
                        let message = ws::WsServerMessage::SessionReadResult {
                            session_id: facts.session_id,
                            run_count: facts.run_count,
                        };
                        if let Err(err) =
                            ws::write_ws_text(&mut io, &canon::canonical_bytes(&message)).await
                        {
                            tracing::warn!(%err, "cannot send ws reply");
                            return;
                        }
                    }
                    Ok(sessions::SessionAccess::NotFound) => {
                        let error =
                            ProtocolError::new(ErrorCode::NotFound, "session not found", false);
                        let _ = ws::write_ws_text(&mut io, &canon::canonical_bytes(&error)).await;
                        let _ =
                            ws::write_ws_close(&mut io, ws::WS_CLOSE_NOT_FOUND, "not_found").await;
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
                        let _ = ws::write_ws_text(&mut io, &canon::canonical_bytes(&error)).await;
                        let _ =
                            ws::write_ws_close(&mut io, ws::WS_CLOSE_FORBIDDEN, "forbidden").await;
                        return;
                    }
                    Err(err) => {
                        let error = ProtocolError::new(
                            ErrorCode::Internal,
                            format!("run database read failed: {err}"),
                            false,
                        );
                        let _ = ws::write_ws_text(&mut io, &canon::canonical_bytes(&error)).await;
                        let _ = ws::write_ws_close(&mut io, 1011, "internal").await;
                        return;
                    }
                }
            }
            Ok(Some(ws::WsFrame::Ping(payload))) => {
                if let Err(err) = ws::write_ws_frame(&mut io, 0xA, &payload).await {
                    tracing::warn!(%err, "cannot send pong");
                    return;
                }
            }
            Ok(Some(ws::WsFrame::Pong)) => {}
            Ok(Some(ws::WsFrame::Close(code, reason))) => {
                let _ = ws::write_ws_close(&mut io, code, &reason).await;
                return;
            }
            Ok(None) => return,
            Err(err) => {
                tracing::warn!(%err, "ws session read failed");
                return;
            }
        }
    }
}

/// Builds the full HTTP router with all injected state: public health plus
/// the authenticated business surface, behind the shared transport guard
/// and authentication/authorization middleware.
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
        .route("/lingxi/v1/ws-ticket", post(ws_ticket))
        .route(
            "/lingxi/v1/devices/credentials",
            post(issue_device_credential),
        )
        .route("/lingxi/v1/ws", get(ws_handler))
        .layer(axum::extract::DefaultBodyLimit::max(
            limits::DEFAULT_BODY_LIMIT_BYTES,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), auth_guard))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            transport_guard,
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

/// Binds, reports the concrete local address through `on_ready`, and serves
/// until `shutdown` resolves (SIGINT/SIGTERM in the binary; a test-controlled
/// future in the harness). In-flight requests drain before returning.
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
) -> Result<(), ServiceError>
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
    on_ready(local);

    let server = axum::serve(
        listener,
        build_router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown);
    server
        .await
        .map_err(|source| ServiceError::Serve { source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
}
