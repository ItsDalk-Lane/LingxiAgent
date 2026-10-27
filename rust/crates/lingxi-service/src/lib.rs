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

pub mod agents_admin;
pub mod appevents;
pub mod approval;
pub mod approval_service;
pub mod artifactverify;
pub mod auth;
pub mod automation;
pub mod automation_activity;
pub mod automation_daily;
pub mod automation_heartbeat;
pub mod automation_import;
pub mod automation_patrol;
pub mod automation_permission;
pub mod automation_review;
pub mod automation_schedule;
pub mod automation_tools;
pub mod background;
pub mod background_tasks;
mod beautify_generation;
pub mod bridge;
mod bridge_effect_text;
mod bridge_effects;
pub mod bridge_media_pipe;
mod bridge_permission;
pub mod bridge_routes;
pub(crate) mod bridge_run_authority;
pub mod bridge_session_routes;
pub mod bridge_worker;
pub mod bridgemedia;
pub mod browser;
pub mod browser_host;
pub mod browser_routes;
pub mod browser_tools;
pub mod cancel;
pub mod cardguide;
pub mod cards;
pub mod character_cards;
pub mod chat_settings;
pub mod chat_wire;
pub mod checkpoint_store;
pub mod client_context;
pub mod coding_ast_lsp;
pub mod coding_ptc;
pub mod coding_run_code;
pub mod coding_security;
pub mod compaction;
pub mod computer_use_settings;
pub mod config;
pub mod context_compiler;
pub mod conversation_map;
pub mod conversation_routes;
pub mod conversation_runtime;
pub mod conversation_status;
pub mod conversation_tools;
pub mod conversations;
pub mod credentials;
pub mod cutover;
pub mod dedup;
pub mod desk_files;
pub mod desk_logs;
pub mod diary;
pub mod discovery_tools;
pub mod dream_service;
pub mod env_deps;
pub mod epoch;
pub mod events;
pub mod exectools;
pub mod file_safety;
pub mod filehistory;
pub mod filemeta;
pub mod filetools;
pub mod fsread;
pub mod git_panel;
pub mod goal;
pub mod goaltools;
pub mod history;
pub mod history_projection;
pub mod hooks;
pub mod host_auxiliary;
pub mod inject;
pub mod input_drafts;
pub mod instance;
pub mod interaction_store;
pub mod interactive_tools;
pub mod invocations;
pub mod knowledge;
pub mod knowledge_processors;
pub mod knowledgetools;
pub mod legacy_plugins;
pub mod limits;
pub mod logging;
pub mod loopsvc;
pub mod looptools;
pub mod loopx;
mod management;
pub mod mcp_session_permissions;
pub mod mcp_settings;
pub mod mcpbridge;
pub mod media;
pub mod media_admin;
pub mod media_generate;
pub mod media_jimeng;
pub mod media_routes;
pub mod media_tools;
pub mod memory;
pub mod memory_ticker;
pub mod memorytools;
pub mod model_configuration;
pub mod model_history;
mod mood_trajectory;
mod observability;
pub mod office;
mod office_pdf;
pub mod office_routes;
pub mod office_tools;
pub mod operations;
pub mod paths;
pub mod persona;
mod phone_run_authority;
pub mod phone_settings;
pub mod phone_tools;
pub mod preview;
pub mod procsupervisor;
pub mod terminals;
pub mod projects;
pub mod provider_probe;
pub mod provider_routes;
pub mod quotas;
pub mod read_vision;
pub mod recovery;
pub mod redaction;
pub mod resource_mounts;
pub mod resource_tools;
pub mod resource_url;
pub mod resourceaccess;
pub mod resourceio;
pub mod resources;
pub mod runs;
pub mod sandbox;
pub mod search_routes;
mod security_audit;
pub mod serve;
pub mod session_admin;
pub mod session_collab;
pub mod session_focus;
pub mod session_locator;
pub mod session_prompt;
pub mod session_supervisor;
pub mod session_tree;
pub mod session_ui;
pub mod sessionfiles;
pub mod sessions;
pub mod sessiontools;
pub mod status_tools;
pub mod settings_configuration;
pub mod settings_tools;
pub mod settings_ui;
pub mod shutdown;
pub mod skills;
pub mod slash;
pub mod stagefiles;
mod static_web;
pub mod streaming_norm;
pub mod subagents;
pub mod task_supervisor;
pub mod todos;
pub mod toolcatalog_bridge;
pub mod toolgateway;
pub mod transport;
pub mod userprefs;
pub mod web_tools;
pub mod workbench;
pub mod workermodel;
pub mod workerrpc;
pub mod workflow;
pub mod workflow_routes;
pub mod workflowjs;
pub mod workspace_ui_state;
pub mod ws;

pub use approval::{ApprovalDecision, ApprovalGate, ApprovalRequest};
pub use approval_service::{
    Answer, AnswerOutcome, ApprovalPolicy, ApprovalService, GrantError, InvocationGrantKey,
    PendingView, ACTION_BLOCKED_BY_READ_ONLY, DEFAULT_APPROVAL_TIMEOUT_MS,
    TOOL_APPROVAL_UNAVAILABLE,
};
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
    AdmissionBinding, DedupDecision, DedupKey, DedupRegistryFull, SubmissionDedup,
    DEFAULT_DEDUP_CAP, MAX_REQUEST_ID_LEN,
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
pub use exectools::{
    register_process_tools, CoreProcessExecutor, CoreProcessTools, ProcessOwnershipGuard,
    ProcessToolKind, ProcessTools, EXEC_DEFAULT_TIMEOUT_SECONDS, EXEC_MAX_TIMEOUT_SECONDS,
    EXEC_PROCESS_REGISTRY_FULL, EXEC_SPAWN_FAILED, SAFE_ENV_PASSTHROUGH,
    WRITE_STDIN_NOT_INTERACTIVE, WRITE_STDIN_NOT_OWNED, WRITE_STDIN_PROCESS_ID_REQUIRED,
    WRITE_STDIN_UNKNOWN_PROCESS,
};
pub use filetools::{
    register_core_file_tools, CoreFileExecutor, CoreFileTools, FileChangeLog, FileChangeOperation,
    FileModificationRecord, FileToolKind, FileTools, FileVersion, FreshnessRegistry, NoopChangeLog,
    FILE_CHANGED_DURING_MUTATION, FILE_STALE_SINCE_READ, RESOURCE_SYMLINK_CHURN,
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
pub use mcpbridge::{
    breakable_duplex, manifest_for_tool, refresh_mcp_server, register_mcp_server,
    sync_server_tools, validate_structured_output, BreakableRead, BreakableWrite,
    BridgeClientHandler, BridgeIo, McpBridgeError, McpCallFailure, McpEndpoint, McpServer,
    McpSyncReport, McpToolBinding, McpToolExecutor, RegisteredMcpServer,
    MCP_DEFAULT_CALL_DEADLINE_MS, MCP_MAX_LISTED_TOOLS, MCP_MAX_LIST_PAGES,
    MCP_MAX_RESULT_CONTENT_BYTES, SAFE_MCP_ENV_PASSTHROUGH,
};
pub use paths::{prepare_layout, DataRootLayout};
pub use procsupervisor::{
    ProcessSupervisor, SupervisorLimits, TerminationReason, DEFAULT_CLEANUP_TIMEOUT,
    DEFAULT_LIVE_PROCESS_CAP, DEFAULT_SPILL_CAP_BYTES, DEFAULT_STDIO_GRACE,
};
pub use quotas::{
    LayeredQuotaLimits, QuotaFailure, QuotaFailureKind, QuotaLayer, QuotaLimits, QuotaManager,
    QuotaPermit, QuotaResource,
};
use redaction::DiagnosticRequestId;
pub use redaction::{redact_line, redact_text};
pub use resourceaccess::{
    AccessRefusal, AccessRefusalCause, ResourceAccess, ResourceOp, ResourceScope,
};
pub use runs::{DriveError, RunDriveLimits, RunSupervisor};
pub use sandbox::{
    detect_sandbox_backend, platform_sandbox, BwrapSandbox, SandboxBackendKind,
    SandboxCapabilities, SandboxCommandRequest, SandboxNetworkPolicy, SandboxNetworkRequest,
    SandboxPolicy, SandboxPolicyInput, SandboxPort, SandboxRefusal, SeatbeltSandbox,
    UnsupportedSandbox, WrappedSandboxCommand, BWRAP_HELPER_DEFAULT, SEATBELT_HELPER_DEFAULT,
};
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
pub use toolgateway::{
    CallerSurface, FailClosedPolicy, GatewayRefusal, InvocationEntry, InvocationRequest,
    PolicyAdjudicationInput, PolicyVerdict, PreparedInvocation, PreparedInvocationHandle,
    ResourceExtractionInput, ResourceExtractor, ToolInvocationGateway, ToolPolicyPort,
    DEFAULT_LIVE_PREPARED_CAP, DEFAULT_PREPARED_TTL_MS,
};
pub use transport::{check_origin, infer_connection_kind, ConnectionKind, NetworkMode};
pub use workerrpc::{
    register_worker_tool, BoundedWorkerModel, RegisteredWorkerTool, UnconfiguredWorkerModel,
    WorkerCallbackLine, WorkerContentLine, WorkerFailure, WorkerGrant, WorkerLimits,
    WorkerModelPort, WorkerModelRefusal, WorkerModelReply, WorkerModelRequest, WorkerRequestLine,
    WorkerResultLine, WorkerRuntime, WorkerToolExecutor, WorkerToolSpec,
    SAFE_WORKER_ENV_PASSTHROUGH, WORKER_DEFAULT_DEADLINE_MS, WORKER_KILL_GRACE_MS,
    WORKER_MAX_CALLBACKS_PER_CALL, WORKER_MAX_LINE_BYTES, WORKER_MAX_REQUEST_BYTES,
    WORKER_RPC_PROTO,
};
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
use axum::routing::{delete, get, patch, post, put};
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

/// 最后一个服务状态所有者离开时发出停止信号，不把自持 Arc 的 worker 留活。
struct KnowledgeWorkerOwner(Arc<knowledge::KnowledgeService>);
impl Drop for KnowledgeWorkerOwner {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

/// Live service state shared by handlers: the injected port implementations
/// (auth / tickets / storage / sessions / events / limits) live here.
#[derive(Clone)]
pub struct ServiceState {
    config: Arc<ServiceConfig>,
    /// R08-T05 cutover self-check mode: bootstrapped for `--self-check`.
    /// When true the run loop (a) never starts the background writers
    /// (dream ticker, automation, bridge/conversation recovery, browsers,
    /// knowledge worker, loop recovery, cold-session cleanup) and (b)
    /// serves only the restricted self-check router (health + read-only
    /// probe; every other route is a loud 503). Coordinator-only.
    self_check: bool,
    _knowledge_worker_owner: Arc<KnowledgeWorkerOwner>,
    auth: Arc<AuthService>,
    management: Arc<management::ManagementState>,
    static_web: Arc<static_web::StaticWebConfig>,
    bound_addr: Arc<std::sync::RwLock<Option<SocketAddr>>>,
    tickets: Arc<WsTicketService>,
    storage: Arc<RunDatabase>,
    sessions: Arc<sessions::SessionStore>,
    slash: Arc<slash::SlashService>,
    chat_wire: Arc<chat_wire::ChatWire>,
    browser_host: Arc<browser_host::BrowserHost>,
    browsers: Arc<std::sync::OnceLock<Arc<browser::BrowserService>>>,
    web_tools: Arc<std::sync::OnceLock<Arc<web_tools::WebTools>>>,
    office_api: Arc<std::sync::OnceLock<Arc<office_tools::OfficeApi>>>,
    office_assets: Option<office_tools::OfficeAssets>,
    beautify: Arc<std::sync::OnceLock<Arc<beautify_generation::BeautifyService>>>,
    media: Arc<std::sync::OnceLock<Arc<media::MediaService>>>,
    automation: Arc<automation::AutomationHost>,
    bridge: Arc<bridge::BridgeHost>,
    conversations: Arc<conversation_runtime::ConversationHost>,
    conversation_files: lingxi_adapters::conversation_files::ConversationFiles,
    session_collab: Arc<std::sync::OnceLock<Arc<history::history_content::SessionCollabTools>>>,
    events: Arc<events::EventService>,
    app_events: Arc<appevents::AppEventLog>,
    /// R07-T07-δ2: 会话预算目标引擎单例（run 进账钩 / 取消钩 / goal 工具
    /// 共用；组合根处注释说明现役对应件）。
    goal_engine: Arc<goal::GoalEngine>,
    /// R07-T07-δ6: 循环面三件套（控制器/闹钟/存储；slash /loop 臂、
    /// loop_control 工具与会话列表注入共用）。
    loop_services: loopsvc::LoopServices,
    workflows: Arc<workflow::WorkflowService>,
    desk_logs: Arc<desk_logs::DeskLog>,
    session_projects: Arc<projects::SessionProjectsService>,
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
    native_processes: Option<Arc<procsupervisor::ProcessSupervisor>>,
    /// R06-T03: the session tree service (fork / retry / rewind /
    /// checkpoints / branch index) over the same storage + events.
    session_tree: Arc<session_tree::SessionService>,
    /// R06-T03: the session admin service (create / rename / pin / archive /
    /// restore / delete / cleanup / search / memory toggle).
    session_admin: Arc<session_admin::SessionAdminService>,
    /// R06-T05: the session file service (sf_ identity registry, legacy
    /// sidecar import, fork copy, reference checker, cold-cache cleanup).
    session_files: Arc<sessionfiles::SessionFileService>,
    /// R06-T05: the resource service (res_ envelope / content resolution /
    /// content ticket) over the same session-file registry.
    resources: Arc<resources::ResourceService>,
    /// R06-T05 Round 4: the ResourceIO face (kernel + local_fs /
    /// session_file / resource providers + event bus + watch registry).
    resource_io: Arc<resourceio::ResourceIoService>,
    /// R06-T05 Round 5: the HTML preview face (pv_ id / token-gated reads /
    /// asset root scoping, mirroring server/routes/html-preview.ts).
    preview: Arc<preview::PreviewService>,
    /// R06-T05 Round 5: the bridge media face (publish service API +
    /// token-gated public read, mirroring lib/bridge/media-publisher.ts +
    /// server/routes/bridge.ts:676-701).
    bridge_media: Arc<bridgemedia::BridgeMediaService>,
    /// R06-T05 Round 5: the workspace file-history face (v9
    /// file_history_snapshots 单库表 + ResourceIO 写路径捕获，D8；
    /// 对照 lib/file-history/* + server/routes/file-history.ts)。
    file_history: Arc<filehistory::FileHistoryService>,
    /// R06-T05 Round 5: the fs 直读面（授权根集合持有，对照
    /// server/routes/fs.ts 的 read / read-base64 两叶面）。
    fs_read: Arc<fsread::FsReadService>,
    /// R06-T06: the per-agent memory service.
    memory: Arc<memory::MemoryService>,
    /// R06-T07: 现役知识库服务面（笔记本/来源/摄入/检索/引用/TurnScope）。
    knowledge: Arc<knowledge::KnowledgeService>,
    /// R06-T07: 统一工具网关句柄（工具面测试/观测入口；运行链同一实例）。
    tool_gateway: Option<std::sync::Arc<toolgateway::ToolInvocationGateway>>,
    /// R06-T06: the skill service (discovery / whitelist / install audit).
    skills: Arc<skills::SkillService>,
    /// R06-T06: the persona file service.
    persona: Arc<persona::PersonaService>,
    /// R06-T03: the workspace file-rollback preference (incumbent
    /// `getRollbackFileChanges`, `core/preferences-manager.ts:322-324`).
    /// Default OFF, mirroring the incumbent; a rewind/retry that requests
    /// file restore while this is off is refused 403 `file_rollback_disabled`,
    /// never silently degraded to a no-restore rewind.
    rollback_file_changes: Arc<std::sync::atomic::AtomicBool>,
    rollback_preferences_lock: Arc<tokio::sync::Mutex<()>>,
    input_drafts: Arc<input_drafts::InputDraftsService>,
    session_tools: Arc<sessiontools::SessionTools>,
    interactions: Arc<interaction_store::InteractionStore>,
    focus_mutation: Arc<tokio::sync::Mutex<()>>,
    approvals: Option<Arc<approval_service::ApprovalService>>,
    /// R07-T06: 产物"系统打开"的注入点（None = open 路由响亮 503）。
    media_file_opener: Option<media_generate::MediaFileOpener>,
    /// Subagent runtime (R03-T06): child-run dispatch/reply/close through
    /// the same run supervisor.
    subagents: Arc<subagents::SubagentRuntime>,
    /// Background-drive registry (R03-T06): submissions whose runs outlive
    /// the client connection + the minimal service-exit hook.
    background: Arc<background::BackgroundDriveRegistry>,
    /// The submission intake gate (R03-T07/A14): open from bootstrap; the
    /// serving process closes it at SIGNAL time — the FIRST exit action —
    /// and every subsequent admission on either submission surface is
    /// refused through the shared admission chain.
    intake: Arc<shutdown::SubmissionIntake>,
    /// The report of the startup recovery scan (R03-T07), set once during
    /// bootstrap (`None` only via `Default`-shaped construction paths that
    /// bypass the scan — none exist in production).
    recovery_report: Arc<std::sync::OnceLock<recovery::RecoveryScanReport>>,
    /// The config-backed model gateway (R05-T01), present when a model
    /// plane is configured: the management reload surface swaps its
    /// snapshot atomically (in-flight calls keep their resolved snapshot).
    model_gateway: Option<Arc<lingxi_adapters::models::gateway::ConfigModelGateway>>,
    /// Where the model plane was loaded from (R05-T01) — the reload
    /// surface re-reads the SAME source, never a guessed location.
    model_configuration: Arc<model_configuration::ModelConfiguration>,
    mcp_settings: Arc<mcp_settings::McpSettingsManager>,
    cards: Arc<cards::CardCache>,
    /// The credential service (R05-T02), present exactly when the model
    /// gateway is: the SINGLE credential-material exit of the model plane
    /// (C01) — seeded from the same provider config, owning the OAuth
    /// store, the refresh coordination, revocation and handles.
    credential_service: Option<Arc<credentials::CredentialService>>,
    /// The worker model-callback port (R05-T06): present when the REAL
    /// model chain was wired (or a test injected one) — every worker
    /// tool's `spec.model` binds THIS port, so callbacks share the
    /// gateway/credential/quota discipline of the main model loop.
    worker_model: Option<Arc<dyn workerrpc::WorkerModelPort>>,
    /// The operation service (R05-T06 §29.9): the real Rust operation
    /// entry for the non-chat model plane (embedding / rerank / image /
    /// video / speech / transcription). Present exactly when the REAL
    /// model chain was wired — an injected `turn_provider` keeps the
    /// plane test-owned and the operation entry absent.
    operations: Option<Arc<operations::OperationService>>,
    /// The model plane's shared network policy plane (R05 RR1 F14):
    /// present exactly when the production model chain was wired; the
    /// management reload surface publishes new policy generations on it.
    network_plane: Option<Arc<lingxi_adapters::models::network::NetworkPlane>>,
    /// The session-scoped context compiler (R06-T01): present exactly when
    /// the composition root wired it — the run supervisor's system text
    /// AND the context-observation endpoint both read from it (one build
    /// result, R06-A01).
    context_compiler: Option<Arc<context_compiler::ContextCompilerService>>,
    /// The mid-run compaction service handle (R06 C2 fresh-compact 端点)：
    /// 与 runs supervisor 共享的同一实例；None = 未接真实模型链（pre-R06
    /// 形态），fresh-compact 端点显式拒绝而非静默无压缩。
    pub(crate) compaction: Option<Arc<compaction::CompactionService>>,
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
    /// 办公素材与解析器由可信宿主组装，客户端不能传入可执行程序或素材根。
    pub office_assets: Option<office_tools::OfficeAssets>,
    pub office_document_worker: Option<office_tools::OfficeWorkerLaunch>,
    /// 即梦外部登录仅由可信宿主显式授权，不继承个人HOME。
    pub jimeng_launch: Option<media_jimeng::JimengLaunch>,
    /// 可信注入宿主显式装配封闭根的会话文件工具；无 HTTP 或用户配置入口。
    /// 生产模型链自动装配；其余注入默认保留调用方完整工具面。
    pub assemble_session_file_tools: bool,
    /// 仅可信测试或宿主组装可注入外部浏览器端口，客户端不得选择。
    pub browser_port: Option<Arc<dyn lingxi_kernel::browser::BrowserPort>>,
    /// 仅可信宿主可替换 SDK 进程；消息与 HTTP 均不能选择程序。
    pub bridge_worker_launch: Option<bridge_worker::BridgeWorkerLaunch>,
    /// 可信宿主指定的产品素材根；测试用自身临时目录，默认仍沿现行环境/安装目录解析。
    pub product_material_dir: Option<std::path::PathBuf>,
    /// 可信宿主的确定性文件写入时序探针，生产默认关闭，无 HTTP 或配置入口。
    #[doc(hidden)]
    pub native_file_mutation_test_hook: Option<filetools::MutationTestHook>,
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
    /// 可信宿主周期注入；None沿用真实生产一小时，HTTP没有此配置入口。
    pub memory_ticker_interval: Option<std::time::Duration>,
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
    /// Per-model-call wall-clock tuning (R05-T05): the TOTAL budget one
    /// logical model call may spend across all of its attempts (spanning
    /// queue wait, credential refresh, retry backoff, network and
    /// streaming) and the bounded backoff shape between attempts. The
    /// defaults are the pre-registered R05_BASELINE §8 values; degenerate
    /// values are a loud startup error, never clamped.
    pub model_call_tuning: runs::ModelCallTuning,
    /// Stream idle bound injection (R05-T05 F-03 test seam): `None` keeps
    /// the pre-registered production bound
    /// ([`runs::DEFAULT_STREAM_IDLE_TIMEOUT_MS`]); tests pass a shortened
    /// window to prove the idle arm without waiting out the production
    /// value. Mirrors [`runs::RunSupervisor::with_stream_idle_timeout`].
    pub stream_idle_timeout: Option<std::time::Duration>,
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
    /// double to decide the REAL waiting_approval legs; the double only
    /// decides approvals and never owns run state.
    pub approval_gate: Option<std::sync::Arc<dyn approval::ApprovalGate>>,
    /// Subagent policy (R03-T06): attenuation/concurrency/timeout knobs
    /// with the incumbent defaults (intercept strategy, proactive
    /// delegation experiment OFF, 10/20 caps, 30-minute child timeout).
    pub subagent_policy: lingxi_kernel::subagent::SubagentPolicy,
    /// Recovery capability source for the startup recovery scan
    /// (R03-T07): which tool targets have VERIFIED recovery capabilities
    /// (read-only / idempotency-key / externally-verifiable). The default
    /// is conservative — every unverified target refuses automatic
    /// recovery of unknown outcomes; R04's tool registry replaces the
    /// resolution, never the default.
    pub recovery_capabilities: std::sync::Arc<dyn invocations::RecoveryCapabilitySource>,
    /// Unified tool invocation gateway (R04-T02). `None` (the production
    /// default until tool surfaces register real executors) keeps the R03
    /// behavior exactly: the raw `tool_executor` port (or the loud
    /// no-tools failure). When `Some`, EVERY tool execution of every run
    /// driven by the supervisor is prepared and dispatched through the
    /// gateway — the raw port is not consulted on the gateway wiring.
    pub tool_gateway: Option<std::sync::Arc<toolgateway::ToolInvocationGateway>>,
    /// The config-backed model gateway (R05-T01). When `Some` AND no
    /// `turn_provider` was injected, the bootstrap wires the REAL
    /// production chain: tool registry + approval service + unified
    /// invocation gateway + the core file tools (scoped to
    /// `workspace_root`) + the gateway-backed provider. Injected test
    /// doubles always win over this wiring. `None` keeps the R03/R04
    /// default shape exactly (`no_provider_configured`).
    pub model_gateway: Option<std::sync::Arc<lingxi_adapters::models::gateway::ConfigModelGateway>>,
    /// Where the model plane was loaded from (R05-T01): remembered so the
    /// management reload surface re-reads the SAME source (never a guessed
    /// location).
    pub model_plane_source: Option<config::ModelPlaneSource>,
    /// 生产空配置预装共享链但尚未激活；None 保留既有注入测试语义。
    pub model_plane_activation: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// The credential service (R05-T02). REQUIRED whenever the production
    /// chain is wired (`model_gateway` set and no injected `turn_provider`)
    /// — a configured model plane without its credential service is a loud
    /// startup error, never an anonymous-key fallback (C01).
    pub credential_service: Option<std::sync::Arc<credentials::CredentialService>>,
    /// The model plane's shared network policy plane (R05 RR1 F14): every
    /// outbound model-plane consumer (the five chat families, the
    /// operations dispatcher, the egress downloads, the OAuth transport)
    /// holds a client handle bound to it, and the management reload
    /// surface publishes new policy generations on it. `None` keeps the
    /// pre-F14 direct behavior (the isolated default).
    pub network_plane: Option<std::sync::Arc<lingxi_adapters::models::network::NetworkPlane>>,
    /// 可用的远端只读资源提供者；缺席的独立组合明确拒绝，不猜测网络能力。
    pub resource_url_provider: Option<Arc<resource_url::UrlProvider>>,
    /// The workspace root from the strict `--config` file (R05-T01) — the
    /// ONLY workspace source. When present together with a model plane,
    /// the core file tools are registered scoped to it.
    pub workspace_root: Option<std::path::PathBuf>,
    /// Injectable worker model-callback port (R05-T06 test seam). `None`
    /// lets the bootstrap build the REAL `GatewayWorkerModel` (behind the
    /// budget wrapper) when the production model chain is wired; an
    /// injected port always wins, mirroring `turn_provider`.
    pub worker_model: Option<std::sync::Arc<dyn workerrpc::WorkerModelPort>>,
    /// The config-declared single-operation worker (R05 RR1 F24). Consumed
    /// ONLY by the real model chain wiring: when the production chain is
    /// wired (and a workspace is scoped), the composition root registers
    /// this ONE controlled worker through the R04 discipline (approval,
    /// resource grants, process supervision, the trusted-worker protocol
    /// checks and the host model-callback port). A registration without
    /// the real chain — or alongside an injected `turn_provider` — is a
    /// loud startup error, never a silently skipped or test-owned plane.
    pub worker_tool_registration: Option<config::WorkerToolRegistration>,
    /// The session-scoped context compiler (R06-T01): the SINGLE source of
    /// every driven run's system text and of the context-observation
    /// endpoint (one build result — R06-A01). `None` keeps the pre-R06
    /// shape exactly (`system_prompt: None`, observation endpoint 404s).
    pub context_compiler: Option<std::sync::Arc<context_compiler::ContextCompilerService>>,
    /// The mid-run compaction service (R06-T02): when bound, every driven
    /// run's turn boundary runs the incumbent trigger evaluation and, on
    /// FIRE, compacts the live exchange through the Summarize auxiliary
    /// slot (cache-preserving request shape; failure keeps the original
    /// exchange and the run continues). `None` keeps the pre-R06-T02
    /// shape exactly (no compaction check at all).
    pub compaction_service: Option<std::sync::Arc<compaction::CompactionService>>,
    /// R07-T06: 产物"系统打开"的注入点（现役 openWithSystem）。生产默认是
    /// 平台 opener；测试注入录制替身。`None` 时 open 路由响亮 503，绝不冒充成功。
    pub media_file_opener: Option<media_generate::MediaFileOpener>,
}

/// The formal tool plane's shared handles (R05 RR1 F24): what the real
/// model chain wired for the later worker registration — the live tool
/// registry, the unified invocation gateway, the workspace-scoped resource
/// access and the workspace root itself.
type ToolPlaneHandles = (
    Arc<lingxi_kernel::toolcatalog::ToolRegistry>,
    Arc<toolgateway::ToolInvocationGateway>,
    Arc<resourceaccess::ResourceAccess>,
    std::path::PathBuf,
);

impl Default for ServiceDeps {
    fn default() -> Self {
        Self {
            office_assets: None,
            office_document_worker: None,
            jimeng_launch: None,
            assemble_session_file_tools: false,
            browser_port: None,
            bridge_worker_launch: bridge_worker::BridgeWorkerLaunch::discover(),
            product_material_dir: None,
            native_file_mutation_test_hook: None,
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
            memory_ticker_interval: None,
            request_ids: std::sync::Arc::new(RandomRequestIdGen),
            turn_provider: None,
            tool_executor: None,
            run_limits: runs::RunDriveLimits::default(),
            model_call_tuning: runs::ModelCallTuning::default(),
            stream_idle_timeout: None,
            quota_limits: quotas::QuotaLimits::default(),
            session_concurrency: session_supervisor::SessionConcurrencyLimits::default(),
            cancel_policy: cancel::CancelPolicy::default(),
            approval_gate: None,
            subagent_policy: lingxi_kernel::subagent::SubagentPolicy::default(),
            recovery_capabilities: std::sync::Arc::new(invocations::ConservativeCapabilities),
            tool_gateway: None,
            model_gateway: None,
            model_plane_source: None,
            model_plane_activation: None,
            credential_service: None,
            network_plane: None,
            resource_url_provider: None,
            workspace_root: None,
            worker_model: None,
            worker_tool_registration: None,
            context_compiler: None,
            compaction_service: None,
            media_file_opener: Some(media_generate::platform_media_file_opener()),
        }
    }
}

impl std::fmt::Debug for ServiceDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceDeps")
            .field(
                "assemble_session_file_tools",
                &self.assemble_session_file_tools,
            )
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
            .field("model_call_tuning", &self.model_call_tuning)
            .field("stream_idle_timeout", &self.stream_idle_timeout)
            .field("quota_limits", &self.quota_limits)
            .field("session_concurrency", &self.session_concurrency)
            .field("cancel_policy", &self.cancel_policy)
            .field(
                "approval_gate",
                &self.approval_gate.as_ref().map(|_| "injected"),
            )
            .field("subagent_policy", &self.subagent_policy)
            .field(
                "tool_gateway",
                &self.tool_gateway.as_ref().map(|_| "injected"),
            )
            .field(
                "model_gateway",
                &self.model_gateway.as_ref().map(|_| "injected"),
            )
            .field("model_plane_source", &self.model_plane_source)
            .field(
                "credential_service",
                &self.credential_service.as_ref().map(|_| "injected"),
            )
            .field("workspace_root", &self.workspace_root)
            .field(
                "worker_model",
                &self.worker_model.as_ref().map(|_| "injected"),
            )
            .field(
                "worker_tool_registration",
                &self
                    .worker_tool_registration
                    .as_ref()
                    .map(|w| w.local_name.as_str()),
            )
            .field(
                "context_compiler",
                &self.context_compiler.as_ref().map(|_| "bound"),
            )
            .field(
                "compaction_service",
                &self.compaction_service.as_ref().map(|_| "bound"),
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
    // R05-T05: the per-model-call wall-clock tuning (total budget + retry
    // backoff shape) validates the same way.
    deps.model_call_tuning
        .validate()
        .map_err(ServiceStartupError::Storage)?;
    // R03-T06: the subagent policy (attenuation surface defaults +
    // concurrency caps + child timeout) validates the same way.
    deps.subagent_policy
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
        let resource_url_provider =
            Some(Arc::new(resource_url::UrlProvider::new(&config.data_home)));
        Self::bootstrap_with_deps(
            config,
            layout,
            ServiceDeps {
                resource_url_provider,
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
            false,
        )
        .await
    }

    /// 二进制入口必须沿用已取得独占锁的实例身份，令实例记录和本机令牌指向同一次启动。
    /// `self_check`（R08-T05）：切割自检模式——完整启动链照常（epoch 门、
    /// 存储打开、实例锁），但知识库 worker / loop 恢复 / 冷会话清理一律
    /// 不启动，run 循环再叠加后台子系统禁启与受限路由面（写面结构性关闭）。
    /// 仅切割协调器经 `--self-check` 置位。
    pub async fn bootstrap_with_locked_instance(
        config: ServiceConfig,
        layout: &DataRootLayout,
        deps: ServiceDeps,
        guard: &InstanceGuard,
        self_check: bool,
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
        Self::bootstrap_with_instance_identity(
            config,
            layout,
            deps,
            guard.identity().clone(),
            self_check,
        )
        .await
    }

    async fn bootstrap_with_instance_identity(
        config: ServiceConfig,
        layout: &DataRootLayout,
        mut deps: ServiceDeps,
        identity: InstanceIdentity,
        self_check: bool,
    ) -> Result<Self, ServiceStartupError> {
        deps.event_limits
            .validate()
            .map_err(ServiceStartupError::Storage)?;
        deps.store_options
            .validate()
            .map_err(ServiceStartupError::Storage)?;
        validate_resource_deps(&deps)?;
        // R07-T11：旧用户插件生态已退役——数据根下残留 plugins/ 只告知
        // （不加载、不删除、不阻塞启动），处置权留给用户。
        legacy_plugins::warn_if_legacy_plugins(&layout.home);
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
            .ensure_session_seed_if_empty(sessions::SessionStore::seed_rows(deps.clock.now_unix_ms()))
            .await
            .map_err(ServiceStartupError::Storage)?;
        storage
            .bind_legacy_session_root(&config.data_home)
            .map_err(ServiceStartupError::Storage)?;
        lingxi_adapters::storage::legacy_session_files::recover_pending(&storage)
            .await
            .map_err(ServiceStartupError::Storage)?;
        let storage = Arc::new(storage);
        // R03-T02: the session surface owns the per-session serialization
        // gate (busy `session_busy` rejection + bounded steering inbox);
        // the policy comes from the deps. R03-T07: the SAME construction
        // injects the process's submission intake gate so the shutdown can
        // refuse fresh admissions through the real admission chain (A14).
        let intake = Arc::new(shutdown::SubmissionIntake::new());
        let session_store = sessions::SessionStore::with_concurrency_and_intake(
            (*storage).clone(),
            deps.session_concurrency,
            Arc::clone(&intake),
        );
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
        let events = Arc::new(events);
        // R07-T07-δ2: 会话预算目标引擎（现役 server/index.ts:972-991 的组合
        // 件）。单一实例——run 驱动的进账/时间检查钩、会话取消钩与 goal
        // 工具共用同一引擎（现役注释：侧车全量原子写，双实例会丢增量）。
        // 超支提醒恰好一次（引擎内 overBudgetNotified 守卫）：模型可见面走
        // steering 保留投递（deliver_retained，现役 deliverCustomMessage
        // display:false 的 Rust 对应），用户面走 app_events 轮询
        // （"goal_over_budget" 结构化载荷；Rust 无现役 _emitEvent
        // notification 通道，偏差登记见 T07_STATUS δ 设计定稿）。回调失败
        // 只记 tracing，不吞账本（现役 try/catch 语义）。
        let app_events = Arc::new(appevents::AppEventLog::new());
        let goal_engine = {
            let supervisor = Arc::downgrade(session_arc.session_supervisor());
            let notify_events = Arc::clone(&app_events);
            let notify_clock = Arc::clone(&deps.clock);
            Arc::new(goal::GoalEngine::new(
                config.data_home.clone(),
                Arc::new(
                    move |session_id: &str, goal: &goal::GoalState, kind: goal::OverBudgetKind| {
                        let label = match kind {
                            goal::OverBudgetKind::Tokens => format!(
                                "token budget {} exceeded (used {})",
                                goal.token_budget
                                    .map(|budget| budget.to_string())
                                    .unwrap_or_else(|| "null".to_string()),
                                goal.tokens_used
                            ),
                            goal::OverBudgetKind::Time => "time budget exceeded".to_string(),
                        };
                        let content = format!(
                            "<hana-goal-over-budget goal=\"{}\">{}. Wrap up current work and hand the decision back to the user: finish now, or ask whether to continue despite the overage. Check the goal tool status for exact numbers.</hana-goal-over-budget>",
                            goal.name, label
                        );
                        match supervisor.upgrade() {
                            Some(gate) => {
                                if let Err(error) = gate.deliver_retained(session_id, &content) {
                                    tracing::warn!(
                                        ?error,
                                        session_id,
                                        "goal over-budget steering delivery failed"
                                    );
                                }
                            }
                            None => {
                                tracing::warn!(
                                    session_id,
                                    "goal over-budget: session supervisor gone"
                                );
                            }
                        }
                        let active_ms = goal::goal_active_ms(goal, notify_clock.now_unix_ms());
                        notify_events.publish(
                            "goal_over_budget",
                            serde_json::json!({
                                "sessionId": session_id,
                                "name": goal.name.as_str(),
                                "kind": kind.wire_name(),
                                "tokensUsed": goal.tokens_used,
                                "tokenBudget": goal.token_budget,
                                "activeMs": active_ms,
                                "timeBudgetMs": goal.time_budget_ms,
                            }),
                        );
                    },
                ),
                Box::new({
                    let clock = Arc::clone(&deps.clock);
                    move || clock.now_unix_ms()
                }),
            ))
        };
        session_arc.attach_goal_engine(&goal_engine);
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
        // R05-T01: the production model plane. When a model plane is
        // configured AND no turn provider was injected (injected test
        // doubles always win), wire the REAL chain here — tool registry +
        // approval service + the unified invocation gateway + the core
        // file tools AND the R04 process tools (exec_command/write_stdin,
        // both scoped to the --config workspace, the ONLY workspace
        // source — R05 RR1 F24: the formal "四工具" of the stage gate) + the
        // gateway-backed provider. Without a configured plane every layer
        // keeps the R03/R04 default shape exactly: no provider,
        // conservative recovery, no tool executors.
        let model_gateway_for_wiring = if deps.turn_provider.is_none() {
            deps.model_gateway.clone()
        } else {
            None
        };
        // R05-T06: remember whether THIS bootstrap wired the real chain —
        // the worker model-callback port is built from it below (an
        // injected `turn_provider` keeps the whole plane test-owned).
        let wired_real_model_chain = model_gateway_for_wiring.is_some();
        // R06-T05 Round 4: the workspace-scoped resource access is built
        // ONCE at bootstrap whenever a workspace root is configured and
        // shared by the tool plane (file/exec tools) and the ResourceIO
        // face — resource-io routes get the SAME authorization gate even
        // without a model plane wired (07_diff_ledger: the workspace root
        // is now validated at bootstrap regardless of the model plane).
        let workspace_access: Option<(Arc<resourceaccess::ResourceAccess>, std::path::PathBuf)> =
            match deps.workspace_root.clone() {
                Some(workspace) => {
                    let access = Arc::new(
                        resourceaccess::ResourceAccess::new(std::slice::from_ref(&workspace))
                            .map_err(|refusal| {
                                ServiceStartupError::Storage(StorageError::InvalidRequest {
                                    detail: format!(
                                        "workspace root {} is not a usable resource root: {refusal}",
                                        workspace.display()
                                    ),
                                })
                            })?,
                    );
                    Some((access, workspace))
                }
                None => None,
            };
        if let Some((access, _)) = &workspace_access {
            for name in ["file-write-witnesses", "file-recovery-records"] {
                access
                    .protect_service_root(&config.data_home.join(name))
                    .map_err(|error| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: error.to_string(),
                        })
                    })?;
            }
        }
        if let Some((access, _)) = &workspace_access {
            access
                .protect_service_root(&layout.runtime_dir.join("agent-deletions"))
                .map_err(|error| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: error.to_string(),
                    })
                })?;
        }
        let file_safety = Arc::new(file_safety::FileSafety::new(&config.data_home));
        // 没有工作区也能读取自己刚生成的会话文件；零根策略不授予任何目录。
        let session_read_access = if workspace_access.is_none()
            && (wired_real_model_chain || deps.assemble_session_file_tools)
        {
            Some(Arc::new(resourceaccess::ResourceAccess::new(&[]).map_err(
                |error| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: error.to_string(),
                    })
                },
            )?))
        } else {
            None
        };

        // R06-T06：记忆/技能/人格服务（工具面注册模型侧工具需要它们）。
        // product_dir 复用 T01 的解析链。
        let t06_data_home = config.data_home.clone();
        let t06_product_dir = deps.product_material_dir.clone().unwrap_or_else(|| {
            context_compiler::resolve_product_dir(
                std::env::var("LINGXI_PRODUCT_DIR").ok().as_deref(),
                &std::env::current_exe().unwrap_or_else(|_| t06_data_home.join("lingxi-service")),
                &t06_data_home,
            )
        });
        let memory_service = Arc::new(memory::MemoryService::new(
            t06_data_home.clone(),
            layout.runtime_dir.clone(),
        ));
        memory_service
            .dream()
            .ticker()
            .configure(deps.memory_ticker_interval)
            .map_err(|error| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: error.to_string(),
                })
            })?;
        memory_service.recover_agent_deletions().map_err(|error| {
            ServiceStartupError::Storage(StorageError::Io {
                detail: format!("cannot recover agent deletions: {error}"),
            })
        })?;
        let rollback_file_changes = memory_service.rollback_file_changes().map_err(|err| {
            ServiceStartupError::Storage(StorageError::Io {
                detail: format!("cannot load workspace rollback preference: {err}"),
            })
        })?;
        let skills_service = Arc::new(skills::SkillService::new(
            t06_data_home.clone(),
            t06_product_dir.clone(),
        ));
        if let Some(network) = &deps.network_plane {
            skills_service.set_github_network(Arc::clone(network));
        }
        skills_service.set_clock(Arc::clone(&deps.clock));
        skills_service.set_workspace_root(deps.workspace_root.clone());
        let persona_service = Arc::new(persona::PersonaService::new(
            memory_service.agents_root(),
            t06_product_dir.clone(),
        ));
        // R06-T07: 现役知识库服务。三库（facts/index/vector）+ 快照/产物文件
        // 存储位于 data_home/knowledge；studio 归属与资源面同源（identity_ids，
        // management:1085 先于本行构造）。
        let knowledge_service = Arc::new(
            knowledge::KnowledgeService::new(&t06_data_home, management.identity_ids().1).map_err(
                |error| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: format!("knowledge service init failed: {error}"),
                    })
                },
            )?,
        );
        knowledge_service.set_url_provider(deps.resource_url_provider.clone());
        // R05 RR1 F24: the tool-plane handles the worker registration below
        // consumes (registry + gateway + access + workspace). Present
        // exactly when the real chain was wired with a scoped workspace.
        let file_history = Arc::new(filehistory::FileHistoryService::new(
            Arc::clone(&storage),
            workspace_access.as_ref().map(|(_, root)| root.clone()),
            {
                let clock = deps.clock.clone();
                move || clock.now_unix_ms()
            },
        ));
        let session_files = Arc::new(sessionfiles::SessionFileService::new(
            config.data_home.clone(),
            Arc::clone(&storage),
            {
                let clock = deps.clock.clone();
                move || clock.now_unix_ms()
            },
        ));
        let session_tree_handle = Arc::new(
            session_tree::SessionService::new(Arc::clone(&storage), Arc::clone(&events))
                .with_file_safety(
                    Arc::clone(&file_safety),
                    workspace_access
                        .as_ref()
                        .map(|(access, _)| Arc::clone(access)),
                )
                .with_session_files(Arc::clone(&session_files))
                .with_model_gateway(deps.model_gateway.clone()),
        );
        let mut tool_plane: Option<ToolPlaneHandles> = None;
        let mut read_file_tools = None;
        let mut native_processes = None;
        let mut approval_service_handle = None;
        if let Some(model_gateway) = model_gateway_for_wiring {
            // R05-T02 (C01): the production chain dispatches through the
            // credential service — the SINGLE material exit. A configured
            // plane without it is a loud startup error, never an anonymous
            // or key-from-elsewhere fallback.
            let credential_service = deps.credential_service.clone().ok_or_else(|| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: "a model plane is configured but no credential service was \
                             injected (R05-T02 C01: the credential service is the single \
                             material exit — there is no credential-free production chain)"
                        .to_string(),
                })
            })?;
            // 宿主显式注入工具网关与批准门时（带真实模型面的验收宿主），组合根必须
            // 采用同一套装配：注入网关的注册表承接全部正式工具登记，批准请求落到宿主
            // 持有的批准服务。静默另建网关会让注入的批准服务永远看不到批准条目
            // （R07-T06 媒体批准链的"外部生成须先进入正式批准"超时根因）。只注入其一
            // 会把工具执行与批准等待拆到两个服务上，是响亮启动错误而非静默降级。
            let budget = lingxi_kernel::toolcatalog::SchemaBudget::default();
            let (registry, gateway) = match (deps.tool_gateway.clone(), deps.approval_gate.clone())
            {
                (Some(injected_gateway), Some(_)) => {
                    let registry = Arc::clone(injected_gateway.registry());
                    (registry, injected_gateway)
                }
                (None, None) => {
                    let registry = Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new());
                    let approvals = Arc::new(approval_service::ApprovalService::new(Arc::clone(
                        &deps.clock,
                    )));
                    approval_service_handle = Some(Arc::clone(&approvals));
                    let gateway = Arc::new(toolgateway::ToolInvocationGateway::new(
                        Arc::clone(&registry),
                        Arc::clone(&approvals) as Arc<dyn toolgateway::ToolPolicyPort>,
                        Arc::clone(&deps.clock),
                        budget,
                        toolgateway::DEFAULT_PREPARED_TTL_MS,
                        toolgateway::DEFAULT_LIVE_PREPARED_CAP,
                    ));
                    deps.approval_gate =
                        Some(Arc::clone(&approvals) as Arc<dyn approval::ApprovalGate>);
                    (registry, gateway)
                }
                _ => {
                    return Err(ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: "an injected tool gateway and an injected approval gate must \
                                 come from the same host assembly; adopting only one would split \
                                 the approval chain across two services"
                            .into(),
                    }));
                }
            };
            // 子任务与其他正式工具共用当轮声明、参数校验和权限判断。
            subagents::register_tools(&registry, gateway.as_ref(), &budget);
            // R07-T07-δ3 goal 工具登记已移至 deps.tool_gateway 恒可达点
            // （loop_control 同位），此处不再重复注册。
            // R06 收口：hana_card_guide（Hana Interactive Card 设计手册，
            // 现役 lib/tools/card-guide-tool.ts + core/agent.ts:973 工具面；
            // 只读静态指南，无需 workspace，返回全册文本且不产卡片）。
            cardguide::register_card_guide_tool(&registry, gateway.as_ref(), &budget);
            if let Some((access, workspace)) = workspace_access.clone() {
                let core_file_tools = filetools::register_core_file_tools(
                    &registry,
                    gateway.as_ref(),
                    Arc::clone(&access),
                    workspace.clone(),
                    Arc::clone(&deps.clock),
                    Arc::new(filehistory::FileProductChangeLog::new(
                        file_history.clone(),
                        session_files.clone(),
                    )),
                    &budget,
                );
                discovery_tools::register_discovery_tools(
                    &registry,
                    gateway.as_ref(),
                    access.clone(),
                    workspace.clone(),
                    &budget,
                );
                stagefiles::register_stage_files(
                    &registry,
                    gateway.as_ref(),
                    session_files.clone(),
                    access.clone(),
                    workspace.clone(),
                    &budget,
                );
                read_file_tools = Some(Arc::clone(&core_file_tools.tools));
                core_file_tools
                    .tools
                    .set_file_safety(Arc::clone(&file_safety));
                core_file_tools
                    .tools
                    .set_mutation_test_hook(deps.native_file_mutation_test_hook.clone());
                // R05 RR1 F24 (CL-05): the FOURTH basic tool of the formal
                // gate's "四工具真实闭环" — exec_command (+ write_stdin, its
                // interactive-terminal pair) — registers through the SAME
                // R04 discipline the file tools use: the unified invocation
                // gateway (prepare → policy/approval → execute), the
                // prepare-time resource derivation (workdir must be an
                // authorized directory) and the REAL ProcessSupervisor
                // (process groups, reaper, spill files, kill receipts).
                // The sandbox face stays `None` — the R04-verified tool-
                // plane shape (the OS-sandbox lane is the separately
                // verified T06 discipline, not a silently widened default).
                let exec_supervisor = Arc::new(
                    procsupervisor::ProcessSupervisor::new(
                        Arc::clone(&deps.clock),
                        procsupervisor::SupervisorLimits::with_spill_dir(
                            layout.runtime_dir.join("exec-tool-spill"),
                        ),
                    )
                    .map_err(|err| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: format!(
                                "cannot construct the tool plane's process supervisor: {err}"
                            ),
                        })
                    })?,
                );
                let _core_process_tools = exectools::register_process_tools(
                    &registry,
                    gateway.as_ref(),
                    exec_supervisor.clone(),
                    Arc::clone(&access),
                    workspace.clone(),
                    None,
                    Arc::clone(&deps.clock),
                    &budget,
                );
                coding_run_code::register(
                    &registry,
                    gateway.as_ref(),
                    access.clone(),
                    workspace.clone(),
                    exec_supervisor.clone(),
                    deps.clock.clone(),
                    &budget,
                )
                .map_err(|detail| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
                })?;
                native_processes = Some(exec_supervisor);
                // R06-T06: the memory/skill model-side tools. Each executor is
                // BOUND to one agent at registration (the resolved primary
                // agent) — there is no cross-agent handle (A11). Registration
                // failure is a loud startup error, never a silent skip.
                let primary_agent = memory_service.resolve_primary_agent().unwrap_or_default();
                memorytools::register_memory_tools(
                    &registry,
                    gateway.as_ref(),
                    Arc::clone(&memory_service),
                    Arc::clone(&skills_service),
                    primary_agent,
                    Arc::clone(&deps.clock),
                    &budget,
                );
                // R06-T07: the knowledge model-side tools (search/read/grep/
                // outline/manage) — same R04 discipline, scope ceiling enforced
                // server-side via the frozen TurnScope (never model ids).
                knowledgetools::register_knowledge_tools(
                    &registry,
                    gateway.as_ref(),
                    Arc::clone(&knowledge_service),
                    &budget,
                );
                toolcatalog_bridge::register(&registry, gateway.as_ref(), &budget);
                // 保留默认七项目录策略，补充已注册终端工具的真实配对声明。
                registry
                    .add_model_resident(&_core_process_tools.write_stdin_target)
                    .map_err(|err| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: format!("cannot declare the terminal continuation tool: {err}"),
                        })
                    })?;
                tool_plane = Some((
                    Arc::clone(&registry),
                    Arc::clone(&gateway),
                    access,
                    workspace,
                ));
            }
            let provider = lingxi_adapters::models::provider::GatewayedProvider::new_with_network(
                model_gateway,
                credential_service
                    as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
                budget,
                // R05 RR1 F14: the five chat families' clients are bound to
                // the model plane's shared network policy (the plane was
                // built from the loaded config; `None` keeps the direct
                // default).
                deps.network_plane.clone().unwrap_or_else(
                    lingxi_adapters::models::network::NetworkPlane::direct_isolated,
                ),
            )
            .map_err(|err| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: format!("cannot construct the model provider: {err}"),
                })
            })?;
            // The recovery scan resolves capabilities from the REAL
            // registry (the file tools' verified capability rows), never
            // the conservative default, once real tools exist.
            deps.recovery_capabilities = Arc::new(invocations::RegistryCapabilities::new(registry));
            // 批准门：注入装配沿用宿主批准门（上面 match 已校验成对）；自建装配已在分支内写入。
            deps.tool_gateway = Some(gateway);
            deps.turn_provider = Some(Arc::new(provider));
        }
        if let (Some(access), Some(gateway)) =
            (session_read_access.as_ref(), deps.tool_gateway.as_ref())
        {
            let target = lingxi_kernel::toolcatalog::tool_target_id(
                &lingxi_kernel::toolcatalog::ToolOrigin::FirstParty,
                "read",
            )
            .expect("正式read目标");
            if gateway.registry().describe(&target).is_err() {
                let tools = filetools::register_read_file_tool(
                    gateway.registry(),
                    gateway,
                    access.clone(),
                    config.data_home.clone(),
                    deps.clock.clone(),
                    &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                );
                tools.set_file_safety(file_safety.clone());
                read_file_tools = Some(tools);
                // 仅生产链或明确装配的注入宿主进入这里；已有目录策略保持原样。
                if !gateway.has_catalog_definitions() {
                    toolcatalog_bridge::register(
                        gateway.registry(),
                        gateway,
                        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                    );
                }
            }
        }
        // R03-T01: the run lifecycle supervisor. Degenerate bounds are a
        // loud startup error (never clamped silently); a missing provider is
        // EXPLICIT (no-provider outcome), never a fabricated reply.
        // R03-T02: the same construction now takes the admission-quota
        // manager (global/agent/session model & tool lanes).
        // R03-T03: plus the cancellation runtime (registry + task
        // supervisor + cleanup policy) and the minimal approval gate.
        // R03-T06: plus the subagent runtime (child-run dispatch/reply/
        // close through the SAME supervisor; the supervisor back-reference
        // is bound right after construction — a Weak, so child tasks never
        // form a reference cycle keeping a supervisor alive).
        let subagent_runtime = subagents::SubagentRuntime::new(
            Arc::clone(&storage),
            Arc::clone(&events),
            Arc::clone(session_arc.session_supervisor()),
            deps.subagent_policy.clone(),
        );
        subagent_runtime.bind_agents_root(memory_service.agents_root());
        subagent_runtime
            .initialize()
            .await
            .map_err(ServiceStartupError::Storage)?;
        let runs = runs::RunSupervisor::new(
            deps.turn_provider.clone(),
            deps.tool_executor.clone(),
            deps.run_limits,
            quotas::QuotaManager::new(deps.quota_limits),
            deps.cancel_policy,
            deps.approval_gate.clone(),
            Some(std::sync::Arc::downgrade(
                &(Arc::clone(&subagent_runtime) as Arc<dyn subagents::SubagentLauncher>),
            )),
        )
        .map_err(ServiceStartupError::Storage)?
        // R04-T02: the unified tool invocation gateway. When wired, EVERY
        // tool execution of every run driven by this supervisor goes
        // through it (prepare before the journal's authorized step,
        // handle-verified dispatch after `started`); `None` keeps the R03
        // shape exactly (raw executor port / loud no-tools failure).
        .with_tool_gateway(deps.tool_gateway.clone())
        .with_settlement_clock(Arc::clone(&deps.clock))
        .with_session_admission(Arc::clone(storage.session_admission()))
        // R07-T07-δ2: 预算目标进账/时间检查钩的引擎句柄。
        .with_goal_engine(Arc::clone(&goal_engine));
        // R05-T05: the per-call wall-clock tuning (validated in
        // `validate_resource_deps`; revalidated at the builder boundary).
        let runs = runs
            .with_model_call_tuning(deps.model_call_tuning)
            .map_err(ServiceStartupError::Storage)?;
        // R05-T05 F-03 test seam: an injected stream idle bound shortens
        // the idle arm in tests; `None` keeps the pre-registered
        // production value.
        let runs = match deps.stream_idle_timeout {
            Some(timeout) => runs.with_stream_idle_timeout(timeout),
            None => runs,
        };
        // read 视觉桥使用同一供应商、认证、网络策略和模型并发许可；不绕过现有模型平面。
        if let (Some(tools), Some(gateway), Some(credentials)) = (
            read_file_tools,
            deps.model_gateway.clone(),
            deps.credential_service.clone(),
        ) {
            let auxiliary =
                lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new_with_network(
                    gateway,
                    credentials
                        as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
                    lingxi_kernel::toolcatalog::SchemaBudget::default(),
                    deps.network_plane.clone().unwrap_or_else(
                        lingxi_adapters::models::network::NetworkPlane::direct_isolated,
                    ),
                )
                .map_err(|error| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: format!("cannot construct read vision bridge: {error}"),
                    })
                })?;
            tools.set_read_vision(Arc::new(read_vision::ReadVisionService::new(
                Arc::new(auxiliary),
                runs.quotas_shared(),
                storage.clone(),
                deps.clock.clone(),
            )));
        }
        // R06-T02: the mid-run compaction service. An injected service
        // always wins (the test-seam rule); otherwise, when the real model
        // chain was wired, build the production instance over the SAME
        // gateway/credential/network plane (the Summarize auxiliary slot
        // resolves through the one ModelGateway — C07) and the SAME quota
        // manager as the chat loop (C06). Without a real model chain there
        // is no compaction at all (the pre-R06-T02 shape).
        let compaction_service = match deps.compaction_service.clone() {
            Some(injected) => Some(injected),
            None if wired_real_model_chain => {
                let gateway = deps.model_gateway.clone().ok_or_else(|| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: "the real model chain was wired without a model gateway"
                            .to_string(),
                    })
                })?;
                let credential_service = deps.credential_service.clone().ok_or_else(|| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: "the real model chain was wired without a credential service"
                            .to_string(),
                    })
                })?;
                let executor =
                    lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new_with_network(
                        Arc::clone(&gateway),
                        credential_service
                            as Arc<
                                dyn lingxi_adapters::models::credentials::ProviderCredentialPort,
                            >,
                        lingxi_kernel::toolcatalog::SchemaBudget::default(),
                        deps.network_plane.clone().unwrap_or_else(
                            lingxi_adapters::models::network::NetworkPlane::direct_isolated,
                        ),
                    )
                    .map_err(|err| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: format!(
                                "cannot construct the compaction auxiliary executor: {err}"
                            ),
                        })
                    })?;
                Some(std::sync::Arc::new(compaction::CompactionService::new(
                    std::sync::Arc::new(executor),
                    gateway,
                    runs.quotas_shared(),
                    deps.clock.clone(),
                )))
            }
            None => None,
        };
        if let Some(compaction) = &compaction_service {
            compaction
                .bind_recovery_sources(Arc::clone(&skills_service), layout.runtime_dir.clone())
                .map_err(ServiceStartupError::Storage)?;
            compaction
                .bind_agent_settings_root(config.data_home.join("agents"))
                .map_err(ServiceStartupError::Storage)?;
        }
        let runs = runs.with_compaction(compaction_service.clone());
        // R06-T06: the skill install guard — the `guard` auxiliary slot over
        // the SAME gateway/credential/network plane (fail-closed when the
        // slot is unconfigured; install-skill.ts:96-107). Only wired when the
        // real model chain exists; tests inject stubs via the service.
        if wired_real_model_chain {
            if let (Some(gateway), Some(credential_service)) =
                (deps.model_gateway.clone(), deps.credential_service.clone())
            {
                let guard_executor =
                    lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new_with_network(
                        gateway,
                        credential_service
                            as Arc<
                                dyn lingxi_adapters::models::credentials::ProviderCredentialPort,
                            >,
                        lingxi_kernel::toolcatalog::SchemaBudget::default(),
                        deps.network_plane.clone().unwrap_or_else(
                            lingxi_adapters::models::network::NetworkPlane::direct_isolated,
                        ),
                    )
                    .map_err(|err| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: format!("cannot construct the skill guard executor: {err}"),
                        })
                    })?;
                let guard_executor = Arc::new(guard_executor);
                skills_service.set_auxiliary(Arc::clone(&guard_executor));
                skills_service.set_guard(Arc::new(skills::GatewaySkillGuard::new(
                    guard_executor,
                    memory_service.resolve_primary_agent(),
                )));
            }
        }
        // R06-T06 R1 F-01: the runtime skill selection is a CONTEXT MATERIAL.
        // Production compilers resolve it through the SkillService (the single
        // owner of enable/disable, whitelist, workspace boundary and user
        // override order — incumbent session-coordinator.ts:2102-2110
        // getSkillsForAgent → skillsResult snapshot → resourceLoader.getSkills
        // proxy → pi agent-session.js:784 loadedSkills). The resolved entries
        // feed the FROZEN `skill.catalog` segment as source material — no new
        // privilege channel beyond the T01 segment registry. A compiler
        // constructed without this attach (bare test harnesses) keeps the
        // empty pre-repair shape.
        if let Some(compiler) = deps.context_compiler.as_ref() {
            compiler.set_skill_materials(std::sync::Arc::clone(&skills_service)
                as std::sync::Arc<dyn context_compiler::SkillMaterialPort>);
        }
        // R06-T01: the session-scoped context compiler — when wired, EVERY
        // driven run's system text is its artifact's render and the
        // observation endpoint serves the SAME artifact (R06-A01); `None`
        // keeps the pre-R06 shape exactly.
        let runs = runs
            .with_model_activation(deps.model_plane_activation.clone())
            .with_context_compiler(deps.context_compiler.clone())
            .with_context_session_store(Arc::clone(&storage))
            .with_session_resource_access(
                workspace_access
                    .as_ref()
                    .map(|(access, _)| Arc::clone(access))
                    // 预览（无 workspace）模式：read 工具面用的是零根
                    // session_read_access 实例（lib.rs:1838 注册）；运行时根刷新
                    // 此前只写 workspace_access 实例 → 会话 authorized_folders
                    // 永不生效（PPG-2 机制成因；a05 读腿 t09-a05-standalone-01
                    // 红实证）。接到同一实例：会话级授权目录经正规孔道生效，
                    // 不授予任何 workspace（零根策略不变）。
                    .or_else(|| session_read_access.clone()),
            )
            .with_chat_settings_home(config.data_home.clone());
        // R06-T06: the Pi extension hooks port — production ON (the
        // agent-loop-guard + tool_result guard pipeline). The loop guard's
        // block/warn behavior mirrors lib/extensions/agent-loop-guard-ext.ts;
        // tests that need the pre-T06 shape use the builder default (off).
        let runs = runs.with_extension_hooks(true);
        // R06-T03: the user-message recorder — when wired, every USER-origin
        // run's input lands in the session message tree (the write path of
        // fork/rewind); `None` keeps the pre-T03 shape exactly.
        let runs = runs.with_user_message_recorder(Some(Arc::new(
            session_tree::DbUserMessageRecorder::new(Arc::clone(&storage)),
        )));
        let runs = runs.with_model_history(Some(Arc::new(model_history::ModelHistory::new(
            Arc::clone(&storage),
        ))));
        let runs = Arc::new(runs.with_knowledge(Arc::clone(&knowledge_service)));
        if wired_real_model_chain {
            if let Some(gateway) = deps.tool_gateway.as_ref() {
                coding_ptc::register(
                    gateway.registry(),
                    gateway,
                    Arc::downgrade(&runs),
                    storage.clone(),
                    events.clone(),
                    deps.clock.clone(),
                    &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                )
                .map_err(|detail| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
                })?;
            }
        }
        let session_tools = Arc::new(
            sessiontools::SessionTools::new(sessiontools::Dependencies {
                storage: Arc::clone(&storage),
                tree: Arc::clone(&session_tree_handle),
                tasks: Arc::clone(runs.task_supervisor()),
                memory: Arc::clone(&memory_service),
                clock: Arc::clone(&deps.clock),
                intake: Arc::clone(&intake),
            })
            .map_err(|detail| ServiceStartupError::Storage(StorageError::Internal { detail }))?,
        );
        if let Some(gateway) = deps.tool_gateway.as_ref() {
            sessiontools::register(
                gateway.registry(),
                gateway,
                Arc::clone(&session_tools),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
        }
        let interactions = Arc::new(interaction_store::InteractionStore::new(
            Arc::clone(&storage),
            Arc::clone(&events),
            Arc::clone(&deps.clock),
        ));
        if let Some(gateway) = deps.tool_gateway.as_ref() {
            interactive_tools::register(
                gateway.registry(),
                gateway,
                Arc::clone(&interactions),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
        }
        subagent_runtime.bind_supervisor(Arc::downgrade(&runs));
        // R05-T06: the worker model-callback port. An injected port always
        // wins (the test-seam rule of `turn_provider`); otherwise, when the
        // real model chain was wired above, build the production
        // `GatewayWorkerModel` (aux-slot route + shared quotas) behind the
        // host-side budget wrapper (C07/C08). Without a model plane there
        // is no port here — a worker tool that still registered one gets
        // the honest `model_capability_not_configured` answer only when it
        // binds the R04 default itself.
        let worker_model: Option<Arc<dyn workerrpc::WorkerModelPort>> =
            match deps.worker_model.clone() {
                Some(injected) => Some(injected),
                None if wired_real_model_chain => {
                    let gateway = deps.model_gateway.clone().ok_or_else(|| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: "the real model chain was wired without a model gateway"
                                .to_string(),
                        })
                    })?;
                    let credential_service = deps.credential_service.clone().ok_or_else(|| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: "the real model chain was wired without a credential service"
                                .to_string(),
                        })
                    })?;
                    let executor =
                    lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new_with_network(
                        gateway,
                        credential_service
                            as Arc<
                                dyn lingxi_adapters::models::credentials::ProviderCredentialPort,
                            >,
                        lingxi_kernel::toolcatalog::SchemaBudget::default(),
                        // R05 RR1 F14: the auxiliary slots share the SAME
                        // network policy plane as the chat loop.
                        deps.network_plane.clone().unwrap_or_else(
                            lingxi_adapters::models::network::NetworkPlane::direct_isolated,
                        ),
                    )
                    .map_err(|err| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: format!("cannot construct the auxiliary executor: {err}"),
                        })
                    })?;
                    let gateway_model = workermodel::GatewayWorkerModel::with_trace(
                        Arc::new(executor),
                        runs.quotas_shared(),
                        // R05-T07: the production trace sink — every settled
                        // callback's usage/trace fact lands in the usage ledger
                        // before the worker sees its reply.
                        Arc::new(workermodel::LedgerWorkerCallbackTrace::new(
                            std::sync::Arc::clone(&storage),
                            deps.clock.clone(),
                        )),
                    );
                    Some(workerrpc::BoundedWorkerModel::new(
                        Some(Arc::new(gateway_model)),
                        workerrpc::WORKER_MAX_CALLBACKS_PER_CALL,
                        workerrpc::WORKER_CALLBACK_MAX_OUTPUT_TOKENS,
                    ))
                }
                None => None,
            };
        // R05 RR1 F24 (CL-04): the config-declared single-operation worker
        // registers on the REAL tool plane — the production call site of
        // `register_worker_tool` the frozen candidate never had. The worker
        // rides the same R04 discipline as every other tool (unified
        // gateway prepare/policy/approval, resource grants derived from
        // `path_args`, claimed-file verification, trusted-worker protocol
        // checks, bounded WorkerRuntime with process-group kills) and its
        // model callbacks go through the PRODUCTION port built above (the
        // aux-slot route + shared quotas + the usage-ledger trace). A
        // registration that cannot reach this boundary — no real chain, an
        // injected turn provider, or no scoped workspace — is a LOUD
        // startup error, never a silently skipped config entry.
        if let Some(worker_registration) = deps.worker_tool_registration.clone() {
            let (registry, gateway, access, workspace) = tool_plane.clone().ok_or_else(|| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: "a config-declared worker requires the real model chain with a \
                             scoped workspace (got neither: an injected turn_provider or a \
                             missing workspace keeps the plane test-owned/no-tool — declare \
                             providers/models/workspace or remove the workers section)"
                        .to_string(),
                })
            })?;
            let model_port = worker_model.clone().ok_or_else(|| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: "a config-declared worker requires the production worker \
                             model-callback port (an injected worker_model double cannot \
                             back a formal registration)"
                        .to_string(),
                })
            })?;
            // The declared executable and cwd must actually exist NOW — a
            // config pointing at nothing is a broken config (fail-closed,
            // never a call-time surprise).
            let argv0 = std::path::PathBuf::from(&worker_registration.argv[0]);
            let argv0_meta = std::fs::metadata(&argv0).map_err(|err| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: format!(
                        "the declared worker executable {} is not usable: {err}",
                        argv0.display()
                    ),
                })
            })?;
            if !argv0_meta.is_file() {
                return Err(ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: format!(
                        "the declared worker executable {} is not a regular file",
                        argv0.display()
                    ),
                }));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                if argv0_meta.permissions().mode() & 0o111 == 0 {
                    return Err(ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: format!(
                            "the declared worker executable {} is not executable",
                            argv0.display()
                        ),
                    }));
                }
            }
            let worker_cwd = worker_registration.cwd.clone().unwrap_or(workspace);
            let cwd_meta = std::fs::metadata(&worker_cwd).map_err(|err| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: format!(
                        "the declared worker cwd {} is not usable: {err}",
                        worker_cwd.display()
                    ),
                })
            })?;
            if !cwd_meta.is_dir() {
                return Err(ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: format!(
                        "the declared worker cwd {} is not a directory",
                        worker_cwd.display()
                    ),
                }));
            }
            let runtime = workerrpc::WorkerRuntime::new(
                workerrpc::WorkerLimits::default(),
                // The sandbox face stays `None` — the R04/R05-T06 verified
                // tool-plane shape (grant + claimed-file verification are
                // the controlled boundary; the OS-sandbox lane is not a
                // silently widened default).
                None,
                Arc::clone(&deps.clock),
            );
            let registered = workerrpc::register_worker_tool(
                &registry,
                gateway.as_ref(),
                &runtime,
                &access,
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                workerrpc::WorkerToolSpec {
                    plugin_id: "builtin".to_string(),
                    op: worker_registration.op.clone(),
                    local_name: worker_registration.local_name.clone(),
                    description: worker_registration.description.clone(),
                    input_schema: worker_registration.input_schema.clone(),
                    path_args: worker_registration.path_args.clone(),
                    argv: worker_registration.argv.clone(),
                    env: std::collections::BTreeMap::new(),
                    cwd: worker_cwd,
                    model: model_port,
                    allowed_model_purposes: worker_registration.allowed_model_purposes.clone(),
                    claimed_file_contract: None,
                },
            )
            .map_err(|err| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: format!(
                        "cannot register the config-declared worker {}: {err}",
                        worker_registration.local_name
                    ),
                })
            })?;
            // The R05 formal chain is MODEL-DRIVEN (T08-C11: 模型 → 工具 →
            // worker → callback → 最终回复), so the config-declared worker
            // must be RESIDENT — declared on the model wire like the four
            // basic tools. `register_worker_tool`'s own default is the R04
            // on-demand shape (callable via the discovery catalog, never in
            // the model-facing declaration); the explicit flip keeps that
            // R04 default intact for every other caller.
            registry
                .set_availability(
                    &registered.target,
                    lingxi_kernel::toolcatalog::Availability::Available,
                )
                .map_err(|err| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: format!(
                            "cannot make the config-declared worker {} resident: {err}",
                            worker_registration.local_name
                        ),
                    })
                })?;
            // 可用性和声明筛选相互独立，只补充刚经配置校验并注册成功的目标。
            registry
                .add_model_resident(&registered.target)
                .map_err(|err| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: format!(
                            "cannot declare the config-declared worker {}: {err}",
                            worker_registration.local_name
                        ),
                    })
                })?;
            tracing::info!(
                worker_tool = %registered.spec.local_name,
                op = %registered.spec.op,
                "config-declared single-operation worker registered on the formal tool plane"
            );
        }
        // R05-T06 §29.9: the operation service (the real Rust operation
        // entry of the non-chat plane) rides the SAME gateway, credential
        // service and quota manager — present exactly when the REAL model
        // chain was wired (an injected `turn_provider` keeps the plane
        // test-owned; the operation entry stays absent, never a second
        // parallel model authority).
        let operations_entry: Option<Arc<operations::OperationService>> = if wired_real_model_chain
        {
            let gateway = deps.model_gateway.clone().expect("checked above");
            let credential_service = deps.credential_service.clone().expect("checked above");
            let endpoints: Vec<String> = gateway
                .provider_endpoints()
                .into_iter()
                .map(|(_, endpoint)| endpoint)
                .collect();
            Some(Arc::new(
                operations::OperationService::new_with_network(
                    gateway,
                    credential_service
                        as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
                    runs.quotas_shared(),
                    endpoints,
                    layout.runtime_dir.join("media-products"),
                    // R05 RR1 F14: the operation plane's dispatcher and the
                    // egress download client share the SAME policy plane.
                    deps.network_plane.clone(),
                )
                .map_err(|failure| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest {
                        detail: format!(
                            "cannot construct the R05-T06 operation service: {failure}"
                        ),
                    })
                })?
                .with_embedding_supervisor(Arc::clone(runs.task_supervisor()))
                // R05 RR1 F20: the system-speech platform lane spawns
                // through the SAME supervised process discipline as exec
                // (process group, reaper, RAII ownership, honest kill
                // receipts). This supervisor instance serves the
                // operation lane; its termination tail carries the §29.13
                // grace.
                .with_process_supervisor(Arc::new(
                    procsupervisor::ProcessSupervisor::new(Arc::clone(&deps.clock), {
                        let mut limits = procsupervisor::SupervisorLimits::with_spill_dir(
                            layout.runtime_dir.join("media-exec-spill"),
                        );
                        limits.cleanup_timeout = std::time::Duration::from_millis(
                            operations::SYSTEM_SPEECH_KILL_GRACE_MS,
                        );
                        limits
                    })
                    .map_err(|err| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: format!(
                                "cannot construct the operation lane's process supervisor: \
                                 {err}"
                            ),
                        })
                    })?,
                ))
                // R05-T07: every operation-plane provider request lands in
                // the usage ledger before its outcome returns.
                .with_usage_sink(Arc::new(
                    operations::LedgerOperationUsageSink::new(
                        std::sync::Arc::clone(&storage),
                        deps.clock.clone(),
                    ),
                )),
            ))
        } else {
            None
        };
        // R03-T06: the background-drive registry (submission surface whose
        // runs outlive the client connection + the minimal exit hook).
        if let Some(operations) = &operations_entry {
            operations.recover_embedding_usage().await.map_err(|e| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: e.to_string(),
                })
            })?;
        }
        let task_tools = Arc::new(background_tasks::TaskTools::new(
            Arc::clone(&storage),
            Arc::downgrade(&runs),
            operations_entry.clone(),
        ));
        if let Some(gateway) = deps.tool_gateway.as_ref() {
            background_tasks::register(
                gateway.registry(),
                gateway,
                task_tools.clone(),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
        }
        let background = background::BackgroundDriveRegistry::new();
        // R03-T07: the STARTUP RECOVERY SCAN. Every non-terminal run a
        // previous process left behind is resolved NOW, before serving
        // starts: the T05 journal pass persists the honest unknown
        // verdicts, the kernel's recovery table classifies the run, and
        // the dangling-active row settles through the single finalize
        // path into an explainable `interrupted_needs_attention` (or
        // `cancelled` when a cancellation was already in flight). A
        // restart NEVER presents these runs as blank rows or fake
        // successes, and a scan that cannot classify the durable facts
        // refuses startup (loud, fail-closed — the R02 integrity stance).
        let recovery_report = Arc::new(std::sync::OnceLock::new());
        let scan = recovery::RecoveryCoordinator
            .run_startup_scan(
                storage.as_ref(),
                events.as_ref(),
                deps.recovery_capabilities.as_ref(),
                deps.clock.now_unix_ms(),
            )
            .await
            .map_err(ServiceStartupError::Storage)?;
        let _ = recovery_report.set(scan);
        // R06-T07: the knowledge ingestion/retrieval embedder — identity via the
        // ONE ModelGateway, calls via OperationService::embed (R05 credential/
        // quota/ledger chain; never a bypass). Without the real model chain the
        // embedder stays None → ingestion lands in the EXPLICIT pending_embedding
        // terminal state (禁静默降级).
        if let (Some(model_gateway), Some(operations)) =
            (deps.model_gateway.clone(), operations_entry.clone())
        {
            memory_service.set_search_embedding_plane(model_gateway.clone(), operations.clone());
            let embedder = Arc::new(knowledge::GatewayKnowledgeEmbedder::new(
                model_gateway,
                operations,
            ));
            knowledge_service.set_embedder(Some(embedder));
        }
        // R06-T05: session-file 服务的 lingxiHome 等价根（config 在下面的
        // struct literal 里被 Arc 收走，先取出）。
        let data_home = config.data_home.clone();
        // R06-T05 资源面：信封 name/studioId 与票据 payload 的 studioId
        // 与管理面身份同源（management 在 Ok(Self{...}) 中被移动，先取）。
        let studio_id = management.identity_ids().1;
        // R06-T05：资源服务共享同一会话文件服务句柄（Arc 复用，现役
        // engine 的 sessionFiles 注册表同样是全局单例）。

        let resources = Arc::new(resources::ResourceService::new(
            session_files.clone(),
            studio_id,
            // D4：票据 key 归私有运行目录（现役 security/ 的候选人映射）。
            layout.runtime_dir.clone(),
            {
                let clock = deps.clock.clone();
                move || clock.now_unix_ms()
            },
        ));
        // R06-T05 Round 4：ResourceIO 面共享同一授权闸（bootstrap 处
        // 一次构建的 workspace_access）与同一 session_file/resource
        // 注册表；watch 轮询间隔生产档，测试面注入更快档。
        let resource_io = Arc::new(resourceio::ResourceIoService::new(
            session_files.clone(),
            resources.clone(),
            data_home.clone(),
            workspace_access.clone(),
            resourceio::DEFAULT_WATCH_POLL_MS,
            {
                let clock = deps.clock.clone();
                Arc::new(move || clock.now_unix_ms()) as Arc<dyn Fn() -> u64 + Send + Sync>
            },
        ));
        // R06-T05 Round 5：HTML 预览面（无状态依赖，时钟注入与全局一致）。
        let preview = Arc::new(preview::PreviewService::new({
            let clock = deps.clock.clone();
            move || clock.now_unix_ms()
        }));
        // R06-T05 Round 5：桥媒体面。baseUrl 取现役环境变量回退
        // （engine.getBridgeMediaPublicBaseUrl?.() 无候选配置面对应物，
        // 台账申报）；白名单 = media-roots.ts 收集器的候选映射
        // （data_home + workspace + 用户主目录 + 临时根）。
        let bridge_preferences = memory_service.read_preferences_root().map_err(|detail| {
            ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
        })?;
        let bridge_public_base_url = bridge_preferences
            .pointer("/bridge/mediaPublicBaseUrl")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| std::env::var("LINGXI_BRIDGE_PUBLIC_BASE_URL").unwrap_or_default());
        let bridge_media = Arc::new(
            bridgemedia::BridgeMediaService::new(
                &bridge_public_base_url,
                &bridgemedia::collect_bridge_media_allowed_roots(
                    &data_home,
                    workspace_access.as_ref().map(|(_, root)| root.as_path()),
                ),
                {
                    let clock = deps.clock.clone();
                    move || clock.now_unix_ms()
                },
            )
            .map_err(|err| {
                ServiceStartupError::Storage(StorageError::InvalidRequest {
                    detail: format!("bridge media service: {err}"),
                })
            })?,
        );
        // R06-T05 Round 5：文件历史面（D8）。工作区根沿用已 canonical 的
        // workspace_access；ResourceIO 写路径经 set_file_history 接线捕获
        // （origin "event"；restore 路由经 OpContext.capture_origin 传
        // "restore"）。
        resource_io.set_file_history(file_history.clone());
        resource_io.set_file_safety(Arc::clone(&file_safety));
        if let Some(provider) = deps.resource_url_provider.clone() {
            resource_io.set_url_provider(provider).map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        }

        let mount_access = match workspace_access.as_ref() {
            Some((access, _)) => Arc::clone(access),
            None => {
                let access =
                    Arc::new(resourceaccess::ResourceAccess::new(&[]).map_err(|error| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: error.to_string(),
                        })
                    })?);
                for path in [
                    data_home.join("file-write-witnesses"),
                    data_home.join("file-recovery-records"),
                    layout.runtime_dir.join("agent-deletions"),
                ] {
                    access.protect_service_root(&path).map_err(|error| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: error.to_string(),
                        })
                    })?;
                }
                access
            }
        };
        resource_io
            .set_mount_provider(Arc::new(resource_mounts::MountProvider::new(
                &data_home,
                management.identity_ids().1,
                mount_access,
            )))
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;

        if let (Some(gateway), Some((access, cwd))) =
            (deps.tool_gateway.as_ref(), workspace_access.as_ref())
        {
            if wired_real_model_chain {
                coding_ast_lsp::register(
                    gateway.registry(),
                    gateway,
                    access.clone(),
                    cwd.clone(),
                    resource_io.clone(),
                    None,
                    &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                )
                .map_err(|detail| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
                })?;
                coding_security::register(
                    gateway.registry(),
                    gateway,
                    access.clone(),
                    cwd.clone(),
                    data_home.clone(),
                    resource_io.clone(),
                    None,
                    deps.clock.clone(),
                    &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                )
                .map_err(|detail| {
                    ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
                })?;
            }
            resource_tools::register(
                gateway.registry(),
                gateway,
                Arc::new(resource_tools::ResourceTools::new(
                    Arc::clone(&storage),
                    Arc::clone(&session_files),
                    Arc::clone(&resource_io),
                    Arc::clone(access),
                    cwd.clone(),
                    Arc::clone(&file_safety),
                )),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
        }

        if let (Some(gateway), Some(access)) =
            (deps.tool_gateway.as_ref(), session_read_access.as_ref())
        {
            let target = lingxi_kernel::toolcatalog::tool_target_id(
                &lingxi_kernel::toolcatalog::ToolOrigin::FirstParty,
                "materialize",
            )
            .expect("正式materialize目标");
            if gateway.registry().describe(&target).is_err() {
                resource_tools::register_materialize(
                    gateway.registry(),
                    gateway,
                    Arc::new(resource_tools::ResourceTools::new(
                        storage.clone(),
                        session_files.clone(),
                        resource_io.clone(),
                        access.clone(),
                        data_home.clone(),
                        file_safety.clone(),
                    )),
                    &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                );
            }
        }

        // R06-T05 Round 5：fs 直读面授权根（fs.ts :77-86 的候选映射：
        // data_home + workspace）。
        let fs_read = Arc::new(fsread::FsReadService::new(
            &data_home,
            workspace_access.as_ref().map(|(_, root)| root.as_path()),
        ));
        // R06：输入草稿服务先建，供 session_admin 永久删除时同步清理草稿。
        let input_drafts_handle = Arc::new(input_drafts::InputDraftsService::new(&data_home));
        let session_projects_handle = Arc::new(projects::SessionProjectsService::new(&data_home));
        let injected_browser_port = deps.browser_port.take();
        // R07-T07-δ6: 循环面组合（现役 server/index.ts:488-491 LoopStore +
        // engine.setLoopServices 挂钩）。chat_wire 提前落地：状态广播与
        // 展示气泡闭包与结构字面量共享同一实例。LoopStore::load 对损坏文件
        // 按现役语义移备份空态继续；IO 错误响亮拒绝启动。
        let chat_wire = Arc::new(chat_wire::ChatWire::new(&data_home));
        let loop_services = loopsvc::build_loop_services(
            &data_home,
            &session_arc,
            &storage,
            &events,
            &runs,
            &background,
            operations_entry.clone(),
            &chat_wire,
            &auth.scheduler_principal(),
            &deps.clock,
        )
        .map_err(|err| {
            ServiceStartupError::Storage(StorageError::Io {
                detail: format!("loop state load failed: {err}"),
            })
        })?;
        // R07-T07-δ3: goal 预算工具（现役 lib/tools/goal-tool.ts）——同一引擎
        // 单例；默认预算读 user/preferences.json 的 goal 段。原登记在模型面
        // 装配分支内，turn_provider 替身宿主（全部双组合测试）永远拿不到
        // goal 工具——与 δ6/δ7/δ8 同位移到注入/自建网关的恒可达注册点
        // （deps.tool_gateway 在 1778 行自建臂回填后两处均非空）。
        if let Some(gateway) = deps.tool_gateway.clone() {
            goaltools::register_goal_tool(
                gateway.registry(),
                gateway.as_ref(),
                Arc::clone(&goal_engine),
                config.data_home.clone(),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
        }
        // R07-T07-δ7: loop_control 工具（现役 lib/tools/loop-control-tool.ts）
        // ——与其它正式工具同一注册表/网关；注册表只可能在模型面装配后存在。
        if let Some(gateway) = deps.tool_gateway.clone() {
            looptools::register_loop_control_tool(
                gateway.registry(),
                gateway.as_ref(),
                Arc::clone(&loop_services.controller),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
        }
        // R07-T07-δ8: workflow 引擎（现役 lib/tools/workflow-tool.ts）。
        // 节点复用 subagent 的 child-run 通道，故服务持有同一
        // SubagentRuntime 花名册；工具与正式工具同一注册表/网关。派单即回，
        // 服务持有工作流监督器，统一处理取消、会话清理与退出。
        let workflow_service = Arc::new(workflow::WorkflowService::new(
            Arc::clone(&storage),
            Arc::clone(&events),
            Arc::clone(&runs),
            Arc::clone(session_arc.session_supervisor()),
            Arc::clone(&subagent_runtime),
            Arc::clone(&app_events),
            memory_service.agents_root(),
            Arc::clone(&deps.clock),
        ));
        if let Some(gateway) = deps.tool_gateway.clone() {
            workflow::register_workflow_tool(
                gateway.registry(),
                gateway.as_ref(),
                Arc::clone(&workflow_service),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
        }
        loop_services
            .attach_workflows(&workflow_service)
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        workflow_service.attach_loop_controller(&loop_services.controller);
        task_tools
            .set_workflows(Arc::downgrade(&workflow_service))
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        session_arc.attach_workflows(&workflow_service);
        let model_activation = deps
            .model_plane_activation
            .clone()
            .unwrap_or_else(|| Arc::new(std::sync::atomic::AtomicBool::new(true)));
        let management = Arc::new(management);
        let auth = Arc::new(auth);
        let model_configuration = Arc::new(
            model_configuration::ModelConfiguration::new(
                data_home.clone(),
                layout.runtime_dir.clone(),
                deps.model_plane_source.clone(),
                model_activation,
                deps.model_gateway.clone(),
                deps.credential_service.clone(),
                deps.network_plane.clone(),
                operations_entry.clone(),
                Arc::clone(&management),
                Arc::clone(&auth),
                Arc::clone(&deps.clock),
                Arc::clone(&storage),
                Arc::clone(runs.task_supervisor()),
                runs.quotas_shared(),
            )
            .map_err(|detail| ServiceStartupError::Storage(StorageError::Io { detail }))?,
        );
        let mcp_settings = mcp_settings::McpSettingsManager::new(
            Arc::clone(&memory_service),
            Arc::clone(&app_events),
            deps.tool_gateway.as_ref().map(Arc::downgrade),
        );
        mcp_settings.attach_tasks(runs.task_supervisor());
        mcp_settings.attach_network(deps.network_plane.as_ref());
        let state = Self {
            native_processes,
            _knowledge_worker_owner: Arc::new(KnowledgeWorkerOwner(knowledge_service.clone())),
            config: Arc::new(config),
            self_check,
            auth,
            management,
            static_web: Arc::new(static_web::StaticWebConfig::resolve()),
            bound_addr: Arc::new(std::sync::RwLock::new(None)),
            tickets: Arc::new(WsTicketService::new(
                deps.ws_ticket_ttl_ms,
                deps.ws_max_tickets,
            )),
            storage: Arc::clone(&storage),
            sessions: session_arc,
            slash: Arc::new(slash::SlashService::new(&data_home)),
            chat_wire: Arc::clone(&chat_wire),
            browser_host: Arc::new(browser_host::BrowserHost::default()),
            browsers: Arc::new(std::sync::OnceLock::new()),
            web_tools: Arc::new(std::sync::OnceLock::new()),
            office_api: Arc::new(std::sync::OnceLock::new()),
            office_assets: deps.office_assets.clone(),
            beautify: Arc::new(std::sync::OnceLock::new()),
            media: Arc::new(std::sync::OnceLock::new()),
            automation: Arc::new(automation::AutomationHost::default()),
            conversations: Arc::new(conversation_runtime::ConversationHost::default()),
            conversation_files: lingxi_adapters::conversation_files::ConversationFiles::new(
                data_home.clone(),
            )
            .map_err(ServiceStartupError::Storage)?,
            session_collab: Arc::new(std::sync::OnceLock::new()),
            bridge: Arc::new(bridge::BridgeHost::new(deps.bridge_worker_launch.clone())),
            session_tree: session_tree_handle,
            session_tools,
            interactions,
            focus_mutation: Arc::new(tokio::sync::Mutex::new(())),
            approvals: approval_service_handle,
            media_file_opener: deps.media_file_opener.clone(),
            session_admin: Arc::new(
                session_admin::SessionAdminService::new(Arc::clone(&storage))
                    .with_context_compiler(deps.context_compiler.clone())
                    .with_session_files(session_files.clone())
                    .with_input_drafts(input_drafts_handle.clone()),
            ),
            session_files,
            // R06-T05: 资源面（res_ 信封 / 内容解析 / 票据）。
            resources,
            // R06-T05 Round 4: ResourceIO 面。
            resource_io,
            // R06-T05 Round 5: HTML 预览面。
            preview,
            // R06-T05 Round 5: 桥媒体面。
            bridge_media,
            // R06-T05 Round 5: 文件历史面。
            file_history,
            // R06-T05 Round 5: fs 直读面。
            fs_read,
            // R06-T06：记忆/技能/人格服务。
            memory: memory_service,
            // R06-T07：现役知识库服务 + 统一工具网关句柄。
            knowledge: knowledge_service,
            tool_gateway: deps.tool_gateway.clone(),
            skills: skills_service,
            persona: persona_service,
            rollback_file_changes: Arc::new(std::sync::atomic::AtomicBool::new(
                rollback_file_changes,
            )),
            rollback_preferences_lock: Arc::new(tokio::sync::Mutex::new(())),
            input_drafts: input_drafts_handle,
            events: Arc::clone(&events),
            app_events: Arc::clone(&app_events),
            goal_engine: Arc::clone(&goal_engine),
            loop_services,
            workflows: workflow_service,
            desk_logs: Arc::new(desk_logs::DeskLog::default()),
            session_projects: session_projects_handle,
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
            runs,
            subagents: subagent_runtime,
            background,
            intake,
            recovery_report,
            model_gateway: deps.model_gateway.clone(),
            model_configuration,
            mcp_settings,
            cards: Arc::new(cards::CardCache::default()),
            credential_service: deps.credential_service.clone(),
            worker_model,
            operations: operations_entry,
            network_plane: deps.network_plane.clone(),
            context_compiler: deps.context_compiler.clone(),
            compaction: compaction_service,
        };
        let knowledge_owner = Arc::downgrade(state.knowledge());
        let (browser_port, browser_unavailable) =
            browser::production_port(&state, injected_browser_port)?;
        let browsers = browser::BrowserService::new(&state, browser_port, browser_unavailable)
            .await
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        state.session_admin().attach_browser(&browsers);
        if let Some(gateway) = state.tool_gateway() {
            state.session_admin().attach_mcp_gateway(gateway);
        }
        state.browsers.set(browsers).map_err(|_| {
            ServiceStartupError::Storage(StorageError::Internal {
                detail: "browser already initialized".into(),
            })
        })?;
        let web = web_tools::WebTools::production(&state).map_err(|detail| {
            ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
        })?;
        state.web_tools.set(web.clone()).map_err(|_| {
            ServiceStartupError::Storage(StorageError::Internal {
                detail: "web tools already initialized".into(),
            })
        })?;
        let media = media::MediaService::new(&state);
        if let Some(launch) = deps.jimeng_launch {
            let runtime = media_jimeng::JimengRuntime::new(launch, state.clock().clone()).map_err(
                |detail| ServiceStartupError::Storage(StorageError::InvalidRequest { detail }),
            )?;
            media.set_jimeng(runtime).map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        }
        task_tools
            .set_media(Arc::downgrade(&media))
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        state.session_admin().attach_workflows(state.workflows());
        state
            .session_admin()
            .attach_loop_controller(&state.loop_services().controller);
        state.session_admin().attach_media(&media);
        state.sessions().attach_media(&media);
        state.media.set(media.clone()).map_err(|_| {
            ServiceStartupError::Storage(StorageError::Internal {
                detail: "媒体服务重复初始化".into(),
            })
        })?;
        let office_api = office_tools::OfficeApi::from_state(&state);
        if let Some(launch) = deps.office_document_worker {
            office_api.set_document_worker(launch).map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        }
        state.office_api.set(office_api.clone()).map_err(|_| {
            ServiceStartupError::Storage(StorageError::Internal {
                detail: "办公服务重复初始化".into(),
            })
        })?;
        if let Some(gateway) = state.tool_gateway() {
            let budget = lingxi_kernel::toolcatalog::SchemaBudget::default();
            office_tools::register(gateway, office_api.clone(), &budget).map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
            settings_tools::register(
                gateway,
                &budget,
                settings_tools::SettingsToolDeps {
                    memory: state.memory().clone(),
                    auth: state.auth.clone(),
                    events: state.app_events().clone(),
                    media: media.clone(),
                    bridge_media: state.bridge_media().clone(),
                    models: state.wired_model_gateway().cloned(),
                    mcp: state.mcp_settings().clone(),
                },
            )
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
            media_tools::register(gateway, media, &budget).map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        }
        if let Some(gateway) = state.tool_gateway() {
            browser_tools::register(
                gateway,
                state.browsers().clone(),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            )
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
            web_tools::register(
                gateway,
                web,
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            )
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        }
        state.media().recover().await.map_err(|detail| {
            ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
        })?;
        let beautify = beautify_generation::BeautifyService::from_state(&state, &office_api);
        office_api
            .set_beautify_generation(beautify.clone())
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
        state.beautify.set(beautify.clone()).map_err(|_| {
            ServiceStartupError::Storage(StorageError::Internal {
                detail: "封面服务重复初始化".into(),
            })
        })?;
        beautify.recover().await.map_err(|detail| {
            ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
        })?;
        let collab = Arc::new(session_collab::production(&state).map_err(|detail| {
            ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
        })?);
        state.session_collab.set(collab.clone()).map_err(|_| {
            ServiceStartupError::Storage(StorageError::Internal {
                detail: "session collaboration already initialized".into(),
            })
        })?;
        if let Some(gateway) = state.tool_gateway() {
            session_collab::register(
                gateway.registry(),
                gateway,
                collab,
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
            // R08 收尾：current_status / session_folders 一方工具（此前仅有 HTTP 面）。
            status_tools::register(
                gateway.registry(),
                gateway,
                state.clone(),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            );
        }
        let import_report =
            automation_import::import_current_studio(&automation::JobApi::from_state(&state)).await;
        if import_report.blocked > 0 {
            tracing::error!(
                blocked = import_report.blocked,
                "部分旧自动任务未迁移，需检查原始资料；这些任务不会执行"
            );
        }
        state
            .automation()
            .set_import_report(serde_json::to_value(import_report).expect("导入报告可序列化"));
        if let Some(gateway) = state.tool_gateway() {
            automation_patrol::register(gateway, state.automation().patrol(), &state).map_err(
                |detail| ServiceStartupError::Storage(StorageError::Internal { detail }),
            )?;
            conversation_tools::register(
                gateway,
                conversations::ConversationApi::from_state(&state),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            )
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
            phone_tools::register(
                gateway,
                conversations::ConversationApi::from_state(&state),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            )
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
            automation_tools::register(
                gateway,
                automation::JobApi::from_state(&state),
                &lingxi_kernel::toolcatalog::SchemaBudget::default(),
            )
            .map_err(|detail| {
                ServiceStartupError::Storage(StorageError::InvalidRequest { detail })
            })?;
            if let (Some(models), Some(credentials)) = (
                state.wired_model_gateway(),
                state.wired_credential_service(),
            ) {
                let executor = Arc::new(
                    lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new_with_network(
                        models.clone(),
                        credentials.clone(),
                        lingxi_kernel::toolcatalog::SchemaBudget::default(),
                        state.wired_network_plane().cloned().unwrap_or_else(
                            lingxi_adapters::models::network::NetworkPlane::direct_isolated,
                        ),
                    )
                    .map_err(|error| {
                        ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: format!("cannot construct automation approval model: {error}"),
                        })
                    })?,
                );
                gateway.set_automatic_reviewer(Arc::new(
                    automation_review::ModelAutoReviewer::new(
                        executor,
                        state.storage().clone(),
                        state.runs().quotas_shared(),
                        state.clock().clone(),
                    ),
                ));
            }
        }
        let conversation_owner = Arc::downgrade(state.conversations());
        let bridge_owner = Arc::downgrade(state.bridge());
        let automation_owner = Arc::downgrade(state.automation());
        let mcp_owner = Arc::downgrade(state.mcp_settings());
        let automation_close_budget =
            std::time::Duration::from_millis(state.config().shutdown_timeout_ms);
        // 仅持有可撤销监听的弱停止句柄；关库先停止新空闲任务，不保活服务组合根。
        let loop_idle_stop = Arc::new(state.loop_services().idle_listener_stopper());
        state
            .storage()
            .register_before_close(Arc::new(move || {
                let knowledge_owner = knowledge_owner.clone();
                let automation_owner = automation_owner.clone();
                let mcp_owner = mcp_owner.clone();
                let bridge_owner = bridge_owner.clone();
                let conversation_owner = conversation_owner.clone();
                let loop_idle_stop = Arc::clone(&loop_idle_stop);
                Box::pin(async move {
                    loop_idle_stop();
                    let deadline = tokio::time::Instant::now() + automation_close_budget;
                    // 关库同样回收MCP；即使失败，也继续尝试原有后台清理。
                    let mcp_cleanup = if let Some(manager) = mcp_owner.upgrade() {
                        manager.begin_stop();
                        tokio::time::timeout_at(deadline, manager.shutdown())
                            .await
                            .map_err(|_| StorageError::Internal {
                                detail: "MCP cleanup unconfirmed before storage close".into(),
                            })
                            .and_then(|result| {
                                result.map_err(|error| StorageError::Internal {
                                    detail: error.to_string(),
                                })
                            })
                    } else {
                        Ok(())
                    };
                    if let Err(error) = &mcp_cleanup {
                        tracing::error!(error = %error, "MCP 关库清理未确认");
                    }
                    if let Some(host) = conversation_owner.upgrade() {
                        host.stop();
                        tokio::time::timeout_at(deadline, host.wait_stopped())
                            .await
                            .map_err(|_| StorageError::Internal {
                                detail: "conversation cleanup unconfirmed before storage close"
                                    .into(),
                            })?
                            .map_err(|detail| StorageError::Internal { detail })?;
                    }
                    if let Some(host) = automation_owner.upgrade() {
                        host.stop();
                    }
                    if let Some(host) = bridge_owner.upgrade() {
                        host.stop();
                        tokio::time::timeout_at(deadline, host.wait_stopped())
                            .await
                            .map_err(|_| StorageError::Internal {
                                detail: "Bridge cleanup unconfirmed before storage close".into(),
                            })?
                            .map_err(|detail| StorageError::Internal { detail })?;
                    }
                    if let Some(host) = automation_owner.upgrade() {
                        host.stop();
                        tokio::time::timeout_at(deadline, host.wait_stopped())
                            .await
                            .map_err(|_| StorageError::Internal {
                                detail: "automation cleanup unconfirmed before storage close"
                                    .into(),
                            })?
                            .map_err(|detail| StorageError::Internal { detail })?;
                    }
                    if let Some(knowledge) = knowledge_owner.upgrade() {
                        knowledge.wait_worker_stopped().await.map_err(|error| {
                            StorageError::Internal {
                                detail: format!("knowledge resource stop failed: {error}"),
                            }
                        })?;
                    }
                    mcp_cleanup
                })
            }))
            .map_err(ServiceStartupError::Storage)?;
        // 同进程最后 owner 的 Drop 已通知停止，但原任务必须真实退出并释放 OS 锁。
        // 有界异步等待不提前释放锁，也不把仍活任务恢复为 queued。
        // R08-T05：自检模式结构性跳过——知识库 worker 会发起摄取/写入，
        // 自检面只读，worker 永不启动（wait_worker_stopped 对未启动态即 Ok）。
        if !self_check {
            let handoff_deadline =
                tokio::time::Instant::now() + std::time::Duration::from_millis(500);
            loop {
                match state.knowledge().start_worker() {
                    Ok(()) => break,
                    Err(error)
                        if error.code == "KNOWLEDGE_CONFLICT"
                            && tokio::time::Instant::now() < handoff_deadline =>
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await
                    }
                    Err(error) => {
                        return Err(ServiceStartupError::Storage(StorageError::InvalidRequest {
                            detail: error.to_string(),
                        }))
                    }
                }
            }
        }
        // R07-T07-δ6：启动恢复（现役 server/index.ts:997 的
        // recoverAtBoot）——启动期 run 恢复扫描之后、开始服务之前：不可
        // 投递的循环标记停止，缺闹钟的活跃循环补兜底闹钟。闹钟定时器
        // 在 tokio 运行时内武装。
        // R08-T05：自检模式跳过——recover_at_boot 会写循环状态并武装闹钟
        // 定时器；自检面不允许任何写与定时器。
        if !self_check {
            state
                .runs()
                .attach_loop_controller(&state.loop_services().controller);
            state.loop_services().controller.recover_at_boot().await;
        }
        // R06-T05 D14：启动清扫一次（现役 server/index.ts:563-570
        // fire-and-forget + warn 日志；失败不阻断启动）。现役 24h
        // setInterval 不引入——常驻定时器违背 R02-T06 关停纪律，周期调度
        // 如需由服务编排层显式加。
        // R08-T05：自检模式跳过——cleanup_cold_sessions 会删除磁盘文件，
        // 自检面零删除。
        if !self_check {
            let cleanup = state.session_files().clone();
            tokio::spawn(async move {
                if let Err(err) = cleanup.cleanup_cold_sessions().await {
                    eprintln!("startup session-file cleanup failed: {err}");
                }
            });
        }
        state.mcp_settings().restore().await.map_err(|error| {
            ServiceStartupError::Storage(StorageError::InvalidRequest {
                detail: error.to_string(),
            })
        })?;
        Ok(state)
    }

    pub fn config(&self) -> &ServiceConfig {
        &self.config
    }

    /// R08-T05: whether this state was bootstrapped for cutover self-check
    /// (`--self-check`). The run loop consults it to skip background writers
    /// and to serve the restricted self-check router instead of the full
    /// business surface.
    pub fn self_check(&self) -> bool {
        self.self_check
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

    pub fn interactions(&self) -> &Arc<interaction_store::InteractionStore> {
        &self.interactions
    }

    pub fn sessions(&self) -> &sessions::SessionStore {
        &self.sessions
    }

    pub fn bridge(&self) -> &Arc<bridge::BridgeHost> {
        &self.bridge
    }

    pub fn automation(&self) -> &Arc<automation::AutomationHost> {
        &self.automation
    }

    /// R06-T03: the session tree service (fork / retry / rewind / checkpoints).
    pub fn session_tree(&self) -> &Arc<session_tree::SessionService> {
        &self.session_tree
    }

    /// R06-T03: the session admin service (rename/pin/archive/restore/
    /// delete/cleanup/search/memory).
    pub fn session_admin(&self) -> &Arc<session_admin::SessionAdminService> {
        &self.session_admin
    }

    /// R06-T05: the session file service (sf_ registry / legacy import /
    /// fork / reference checker / cold-cache cleanup).
    pub fn session_files(&self) -> &Arc<sessionfiles::SessionFileService> {
        &self.session_files
    }

    /// R06-T05: the resource service handle (res_ envelope / content /
    /// ticket routes).
    pub fn resources(&self) -> &Arc<resources::ResourceService> {
        &self.resources
    }

    /// R06-T05 Round 4: the ResourceIO face (15 route faces / event bus /
    /// watch registry).
    pub fn resource_io(&self) -> &Arc<resourceio::ResourceIoService> {
        &self.resource_io
    }

    /// R06-T05 Round 5: the HTML preview face handle.
    pub fn preview(&self) -> &Arc<preview::PreviewService> {
        &self.preview
    }

    /// R06-T05 Round 5: the bridge media face handle.
    pub fn bridge_media(&self) -> &Arc<bridgemedia::BridgeMediaService> {
        &self.bridge_media
    }

    /// R06-T05 Round 5: the workspace file-history service (D8).
    pub fn file_history(&self) -> &Arc<filehistory::FileHistoryService> {
        &self.file_history
    }

    /// R06-T05 Round 5: the fs 直读 service（授权根 + resolve）。
    pub fn fs_read(&self) -> &Arc<fsread::FsReadService> {
        &self.fs_read
    }

    /// R06-T06: the per-agent memory service (facts / tenets / compiled /
    /// experience / dream revisions / context notes).
    pub fn memory(&self) -> &Arc<memory::MemoryService> {
        &self.memory
    }

    /// R06-T07: 现役知识库服务（HTTP 面 + 工具面同一实例）。
    pub fn knowledge(&self) -> &Arc<knowledge::KnowledgeService> {
        &self.knowledge
    }

    /// R06-T07: 统一工具网关（知识工具面经同一网关执行）。
    pub fn tool_gateway(&self) -> Option<&Arc<toolgateway::ToolInvocationGateway>> {
        self.tool_gateway.as_ref()
    }

    /// R07-T06: 产物"系统打开"的注入点（生产=平台 opener，测试=录制替身）。
    pub(crate) fn media_file_opener(&self) -> Option<&media_generate::MediaFileOpener> {
        self.media_file_opener.as_ref()
    }

    /// R06-T06: the skill service (discovery / whitelist / install audit /
    /// bundles / removal).
    pub fn skills(&self) -> &Arc<skills::SkillService> {
        &self.skills
    }

    /// R06-T06: the persona file service (AGENTS.md / identity.md /
    /// public persona / avatars — the write faces of the T01 material chain).
    pub fn persona(&self) -> &Arc<persona::PersonaService> {
        &self.persona
    }

    /// R06-T03: whether workspace file rollback is enabled (incumbent
    /// `getRollbackFileChanges`; default off).
    pub fn rollback_file_changes_enabled(&self) -> bool {
        self.rollback_file_changes
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Test/evidence seam: toggle the rollback preference.
    pub fn set_rollback_file_changes(&self, enabled: bool) {
        self.rollback_file_changes
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    async fn save_rollback_file_changes(&self, enabled: bool) -> Result<(), String> {
        let _guard = self.rollback_preferences_lock.lock().await;
        self.memory
            .save_rollback_file_changes(enabled)
            .map_err(|err| err.to_string())?;
        self.rollback_file_changes
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// The injected event subscription service (R02-T05: snapshot/cursor
    /// protocol + post-commit publication hub).
    pub fn events(&self) -> &Arc<events::EventService> {
        &self.events
    }

    /// 进程内 app 级事件日志（agent-created/switched/deleted 等广播面）。
    pub fn app_events(&self) -> &Arc<appevents::AppEventLog> {
        &self.app_events
    }

    /// R07-T07-δ2: 会话预算目标引擎（goal 工具与 δ6 loop/slash 面共用）。
    pub fn goal_engine(&self) -> &Arc<goal::GoalEngine> {
        &self.goal_engine
    }

    pub fn workflows(&self) -> &Arc<workflow::WorkflowService> {
        &self.workflows
    }
    pub fn desk_logs(&self) -> &Arc<desk_logs::DeskLog> {
        &self.desk_logs
    }

    /// R07-T07-δ6: 循环面（/loop 命令、loop_control 工具、会话列表注入）。
    pub fn loop_services(&self) -> &loopsvc::LoopServices {
        &self.loop_services
    }

    /// R06 会话项目/文件夹目录服务。
    pub fn session_projects(&self) -> &Arc<projects::SessionProjectsService> {
        &self.session_projects
    }

    /// The injected storage port implementation (composition root handle
    /// for the close/checkpoint path).
    pub fn storage(&self) -> &Arc<RunDatabase> {
        &self.storage
    }

    /// The config-backed model gateway (R05-T01), when a model plane is
    /// configured — the management reload surface swaps its snapshot.
    pub fn model_gateway(
        &self,
    ) -> Option<&Arc<lingxi_adapters::models::gateway::ConfigModelGateway>> {
        self.model_gateway.as_ref().filter(|_| {
            self.model_configuration
                .activation
                .load(std::sync::atomic::Ordering::Acquire)
        })
    }

    /// Where the model plane was loaded from (R05-T01): the reload surface
    /// re-reads the SAME source.
    pub fn cards(&self) -> &Arc<cards::CardCache> {
        &self.cards
    }
    pub(crate) fn wired_model_gateway(
        &self,
    ) -> Option<&Arc<lingxi_adapters::models::gateway::ConfigModelGateway>> {
        self.model_gateway.as_ref()
    }
    pub(crate) fn wired_credential_service(&self) -> Option<&Arc<credentials::CredentialService>> {
        self.credential_service.as_ref()
    }
    pub(crate) fn wired_operations(&self) -> Option<&Arc<operations::OperationService>> {
        self.operations.as_ref()
    }
    pub(crate) fn wired_network_plane(
        &self,
    ) -> Option<&Arc<lingxi_adapters::models::network::NetworkPlane>> {
        self.network_plane.as_ref()
    }
    pub fn model_configuration(&self) -> &Arc<model_configuration::ModelConfiguration> {
        &self.model_configuration
    }

    pub fn mcp_settings(&self) -> &Arc<mcp_settings::McpSettingsManager> {
        &self.mcp_settings
    }

    pub fn model_plane_source(&self) -> Option<config::ModelPlaneSource> {
        self.model_configuration.source()
    }

    /// The credential service (R05-T02) — the single material exit of the
    /// model plane; the management surface reports its status and revokes
    /// through it.
    pub fn credential_service(&self) -> Option<&Arc<credentials::CredentialService>> {
        self.credential_service.as_ref().filter(|_| {
            self.model_configuration
                .activation
                .load(std::sync::atomic::Ordering::Acquire)
        })
    }

    /// The worker model-callback port (R05-T06) — worker tools bind THIS
    /// as their `spec.model` so callbacks share the model plane's
    /// gateway/credential/quota discipline.
    pub fn worker_model(&self) -> Option<&Arc<dyn workerrpc::WorkerModelPort>> {
        self.worker_model.as_ref()
    }

    /// The operation service (R05-T06 §29.9) — the real Rust operation
    /// entry of the non-chat model plane; present when the real model
    /// chain was wired at bootstrap.
    pub fn operations(&self) -> Option<&Arc<operations::OperationService>> {
        self.operations.as_ref().filter(|_| {
            self.model_configuration
                .activation
                .load(std::sync::atomic::Ordering::Acquire)
        })
    }

    /// The model plane's shared network policy plane (R05 RR1 F14) — the
    /// reload surface publishes new policy generations on it.
    pub fn network_plane(&self) -> Option<&Arc<lingxi_adapters::models::network::NetworkPlane>> {
        self.network_plane.as_ref().filter(|_| {
            self.model_configuration
                .activation
                .load(std::sync::atomic::Ordering::Acquire)
        })
    }

    /// The session-scoped context compiler (R06-T01) — the observation
    /// endpoint reads the SAME frozen artifacts the run supervisor sends
    /// from (one build result, R06-A01).
    pub fn context_compiler(&self) -> Option<&Arc<context_compiler::ContextCompilerService>> {
        self.context_compiler.as_ref()
    }

    /// The mid-run compaction service (R06 C2 fresh-compact)：与 runs
    /// supervisor 共享的同一实例；None = 未接真实模型链。
    pub fn compaction(&self) -> Option<&Arc<compaction::CompactionService>> {
        self.compaction.as_ref()
    }

    /// The managed-task shutdown handle (R02-T06): the binary subscribes
    /// once for the shutdown broadcast; every WS session task subscribes
    /// per connection.
    pub fn ws_shutdown(&self) -> Arc<shutdown::WsShutdown> {
        Arc::clone(&self.ws_shutdown)
    }

    /// The injected clock (R02-T07): every served-path timestamp reads
    /// through this surface, so tests drive time instead of sleeping.
    pub fn conversation_files(&self) -> &lingxi_adapters::conversation_files::ConversationFiles {
        &self.conversation_files
    }
    pub fn conversations(&self) -> &Arc<conversation_runtime::ConversationHost> {
        &self.conversations
    }
    pub fn session_collab(&self) -> Option<&Arc<history::history_content::SessionCollabTools>> {
        self.session_collab.get()
    }

    pub fn clock(&self) -> &Arc<dyn ServiceClock> {
        &self.clock
    }

    /// The injected request-id source (R02-T07).
    pub fn browsers(&self) -> &Arc<browser::BrowserService> {
        self.browsers.get().expect("浏览器服务已组装")
    }
    pub fn office_api(&self) -> Option<&Arc<office_tools::OfficeApi>> {
        self.office_api.get()
    }
    pub fn office_assets(&self) -> Option<&office_tools::OfficeAssets> {
        self.office_assets.as_ref()
    }
    pub fn beautify(&self) -> &Arc<beautify_generation::BeautifyService> {
        self.beautify.get().expect("封面服务已完成初始化")
    }
    pub fn media(&self) -> &Arc<media::MediaService> {
        self.media.get().expect("媒体服务已完成初始化")
    }
    pub fn web_tools(&self) -> &Arc<web_tools::WebTools> {
        self.web_tools.get().expect("网页工具已组装")
    }
    pub fn request_ids(&self) -> &Arc<dyn RequestIdGen> {
        &self.request_ids
    }

    /// The run lifecycle supervisor (R03-T01): the single owner of the
    /// queued→running→…→finalize chain behind every execute.
    pub fn runs(&self) -> &Arc<runs::RunSupervisor> {
        &self.runs
    }

    /// The subagent runtime (R03-T06).
    pub fn subagents(&self) -> &Arc<subagents::SubagentRuntime> {
        &self.subagents
    }

    /// The background-drive registry (R03-T06): live background drives +
    /// the minimal exit hook.
    pub fn background(&self) -> &Arc<background::BackgroundDriveRegistry> {
        &self.background
    }

    /// The submission intake gate (R03-T07/A14): the serving process closes
    /// it at SIGNAL time (the first exit action); tests close it to enter
    /// the exit race window deterministically.
    pub fn submission_intake(&self) -> Arc<shutdown::SubmissionIntake> {
        Arc::clone(&self.intake)
    }

    /// The startup recovery scan's report (R03-T07): what the scan found
    /// and how it settled it (observability / acceptance evidence).
    pub fn recovery_report(&self) -> Option<recovery::RecoveryScanReport> {
        self.recovery_report.get().cloned()
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
        data_epoch: ContractVersions::R08_CUTOVER.data_epoch,
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

    /// Attaches one extra structured detail value (R06-T03: childCount of the
    /// archive 409, mirroring the incumbent `child_sessions_present` payload).
    fn with_detail_value(mut self, key: &str, value: serde_json::Value) -> Self {
        let details = self.error.details.get_or_insert_with(serde_json::Map::new);
        details.insert(key.to_string(), value);
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

    /// R06-T05: an upload blob whose mime is outside the incumbent
    /// whitelist (`shared/{image,audio,video}-mime.ts`) — 415.
    pub fn unsupported_media_type(detail: String) -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ErrorCode::InvalidMessage,
            detail,
        )
        .with_reason("unsupported_media_type")
        .with_cause("request.unsupported_media_type")
    }

    /// R06-T05: 资源/票据面的现役错误外发（core/resource-service.ts
    /// ResourceError 与 core/resource-ticket-service.ts
    /// ResourceTicketError 共用一个 `{code, status}` 形状）——HTTP
    /// status 与现役一致，现役 code 原样进 `details.reason`（候选人
    /// protocol ErrorCode 词汇表不含资源域细目，不伪造第二套模型）。
    pub fn resource_failure(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        let error_code = match status {
            StatusCode::BAD_REQUEST => ErrorCode::InvalidMessage,
            StatusCode::FORBIDDEN => ErrorCode::Forbidden,
            StatusCode::NOT_FOUND => ErrorCode::NotFound,
            StatusCode::CONFLICT | StatusCode::GONE => ErrorCode::Conflict,
            _ => ErrorCode::Internal,
        };
        Self::new(status, error_code, message)
            .with_reason(code)
            .with_cause(format!("resource.{code}"))
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

    /// R03 repair G04/F05 (C01/C03): the explicit requestId is currently
    /// reserved by a submission still between its admission and the
    /// durable run start — nothing replayable exists yet. Retryable: the
    /// client re-sends the same id once the first submission settles (its
    /// replay then answers).
    pub fn admission_in_flight(request_id: &str) -> Self {
        let mut out = Self::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!("requestId is mid-admission (reserved, not yet durably started): {request_id}"),
        );
        out.error.retryable = true;
        out.with_reason("request_id_admission_in_flight")
            .with_cause("session.request_id_admission_in_flight")
    }

    /// R03 repair G04/F05 (C05): this requestId has no binding in THIS
    /// process, but the durable lineage anchor shows a run already bound
    /// to the same (owner, session, id) by an EARLIER process life. The
    /// safe cross-restart contract: query that run's durable outcome or
    /// resubmit under a NEW id — the service never silently re-executes a
    /// task whose earlier life may hold confirmed or unknown external
    /// effects, and never claims exactly-once over arbitrary external
    /// systems.
    pub fn request_id_bound_to_earlier_run(request_id: &str, run_id: &str) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!(
                "requestId was already bound to run {run_id} by an earlier process life; query \
                 that run's durable outcome or resubmit under a new requestId: {request_id}"
            ),
        )
        .with_reason("request_id_bound_to_earlier_run")
        .with_cause("session.request_id_bound_to_earlier_run")
    }

    /// R03 RR2/F05-01: the CANONICAL requestId resolves to MORE THAN ONE
    /// durably bound run (the frozen pre-fix shape — raw-padded lineage
    /// anchors of one logical key each bound their own run). The safe
    /// contract is an explicit ambiguity naming EVERY bound run: the
    /// client verifies those runs or resubmits under a NEW id; the service
    /// never picks one binding and never re-executes the task as fresh.
    pub fn request_id_bound_ambiguously(request_id: &str, run_ids: &[String]) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!(
                "requestId is durably bound to multiple runs ({}) by earlier process lives; \
                 verify those runs' outcomes or resubmit under a new requestId: {request_id}",
                run_ids.join(", ")
            ),
        )
        .with_reason("request_id_bound_ambiguously")
        .with_cause("session.request_id_bound_ambiguously")
    }

    /// R03-A14: the shutdown already began — the submission intake is
    /// closed and this (fresh) submission was refused at the admission
    /// chain before any side effect. Retryable against the NEXT process
    /// instance (the client's idempotent replay semantics stay intact).
    pub fn shutting_down() -> Self {
        let mut out = Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::BudgetExceeded,
            "service is shutting down; new submissions are refused",
        );
        out.error.retryable = true;
        out.with_reason("shutting_down")
            .with_cause("service.shutting_down")
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
    let request_id = DiagnosticRequestId(&request_id);
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
            request_id = %DiagnosticRequestId(&request_id),
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
                tracing::error!(%err, request_id = %DiagnosticRequestId(&request_id), "cannot buffer error response body");
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
    // 已授权浏览器必须能读取实际历史修订，供条件 GET 对账。
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("ETag"),
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
    let request_id = DiagnosticRequestId(&request_id);
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
    let request_id = DiagnosticRequestId(&request_id);
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

// ---------------------------------------------------------------- R06-T03
// session tree: fork / retry / rewind / checkpoint / branch history.
// 现役锚点：sessions.ts:1768 fork、:1558 retry、session-turn-actions.ts:189 rewind。
// 错误映射：Busy→409 session_busy、ForkDepthLimit→409、ForkTargetInvalid→400、
// CheckpointConflict→409、SessionExists→409 session_exists、
// FileRollbackDisabled→403、NotFound→404、Forbidden→403、Storage→存储映射。
// （REPAIR-R1：文件冲突不再整批 409——rewind 逐文件三档判定并如实入收据。）

fn session_operation_storage_response(err: &lingxi_kernel::ports::StorageError) -> Response {
    if let lingxi_kernel::ports::StorageError::Conflict { detail } = err {
        let reason = detail.split(':').next().unwrap_or(detail);
        if matches!(
            reason,
            "session_request_id_conflict"
                | "session_operation_unconfirmed"
                | "session_file_operation_unconfirmed"
                | "session_snapshot_changed"
                | "session_not_active"
                | "session_operation_receipt_conflict"
                | "session_operation_not_prepared"
        ) {
            return EndpointError::new(StatusCode::CONFLICT, ErrorCode::Conflict, detail.clone())
                .with_reason(reason)
                .with_cause("session.operation_conflict")
                .into_response();
        }
    }
    EndpointError::storage(err).into_response()
}

fn session_tree_error_response(err: session_tree::SessionTreeError) -> Response {
    use session_tree::SessionTreeError as E;
    match err {
        E::Busy => EndpointError::session_busy().into_response(),
        E::ForkDepthLimit => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            "session fork depth limit reached",
        )
        .with_reason("session_fork_depth_limit")
        .with_cause("session.fork_depth_limit")
        .into_response(),
        E::ForkTargetInvalid => EndpointError::invalid_message("fork target not on branch")
            .with_reason("session_fork_target_invalid")
            .into_response(),
        E::InvalidTarget { detail } => {
            EndpointError::invalid_message(format!("invalid retry/rewind target: {detail}"))
                .with_reason("session_turn_target_invalid")
                .into_response()
        }
        E::CheckpointConflict { name } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!("checkpoint {name} already exists"),
        )
        .with_reason("checkpoint_conflict")
        .with_cause("session.checkpoint_conflict")
        .into_response(),
        E::IdentityRetired { session_id } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!("session {session_id} identity is retired"),
        )
        .with_reason("session_identity_retired")
        .into_response(),
        E::SessionExists { session_id } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!("session {session_id} already exists"),
        )
        .with_reason("session_exists")
        .with_cause("session.exists")
        .into_response(),
        E::FileRollbackDisabled => {
            EndpointError::forbidden("file_rollback_disabled").into_response()
        }
        E::FileNotAuthorized { file_path } => EndpointError::forbidden(&format!(
            "checkpoint file not under session authorized folders: {file_path}"
        ))
        .into_response(),
        E::NotFound => EndpointError::not_found().into_response(),
        E::Forbidden => EndpointError::forbidden("session_forbidden").into_response(),
        E::Storage(err) => session_operation_storage_response(&err),
    }
}

/// R06-T03 写操作的统一前置闸：归属（现役 can_access）+ 生命周期（现役
/// assertManifestLifecycle "active"）。归档/已删会话拒绝树写操作；
/// 读取面（branch/checkpoints 列表）不走此闸。
/// 返回 Ok(()) 放行；Err(Response) 是已构造好的响亮失败响应。
/// Err 直接携带 axum Response 供 handler 原样 return（19 处调用点），
/// 拒绝路径非热路径，不做 Box 瘦身。
#[allow(clippy::result_large_err)]
async fn gate_session_tree_write(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Result<(), Response> {
    match state.sessions.get_for(principal, session_id).await {
        Ok(sessions::SessionAccess::Ok(_)) => {}
        Ok(sessions::SessionAccess::NotFound) => {
            return Err(EndpointError::not_found().into_response());
        }
        Ok(sessions::SessionAccess::Forbidden) => {
            return Err(EndpointError::forbidden("cross_principal_access").into_response());
        }
        Err(err) => return Err(EndpointError::storage(&err).into_response()),
    }
    let row =
        match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), session_id)
            .await
        {
            Ok(Some(row)) => row,
            Ok(None) => return Err(EndpointError::not_found().into_response()),
            Err(err) => return Err(EndpointError::storage(&err).into_response()),
        };
    if row.lifecycle != "active" {
        return Err(EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!(
                "session lifecycle is {}, tree writes require active",
                row.lifecycle
            ),
        )
        .with_reason("session_not_active")
        .with_cause("session.not_active")
        .into_response());
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ForkRequestBody {
    #[serde(default)]
    request_id: Option<String>,
    new_session_id: String,
    boundary_message_id: String,
}

/// `POST /lingxi/v1/sessions/{id}/fork` — 从指定消息分叉会话（R06-A05）。
async fn session_operations_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(sid): Path<String>,
) -> Response {
    let deleted = match state.sessions.get_for(&principal, &sid).await {
        Ok(sessions::SessionAccess::Ok(_)) => false,
        Ok(sessions::SessionAccess::NotFound) => true,
        Ok(sessions::SessionAccess::Forbidden) => {
            return EndpointError::forbidden("cross_principal_access").into_response()
        }
        Err(error) => return EndpointError::storage(&error).into_response(),
    };
    let kp = sessions::kernel_principal_of(&principal);
    match lingxi_adapters::storage::session_operations::list(
        state.storage(),
        &sid,
        kp.storage_kind(),
        &kp.storage_subject(),
    )
    .await
    {
        Ok(operations) if deleted && operations.is_empty() => {
            EndpointError::not_found().into_response()
        }
        Ok(operations) => (
            StatusCode::OK,
            Json(serde_json::json!({"operations":operations})),
        )
            .into_response(),
        Err(e) => EndpointError::storage(&e).into_response(),
    }
}

async fn fork_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<ForkRequestBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if let Err(resp) = gate_session_tree_write(&state, &principal, &session_id).await {
        return resp;
    }
    match state
        .session_tree
        .fork_session(
            &principal,
            &session_id,
            &req.new_session_id,
            &req.boundary_message_id,
            req.request_id.as_deref(),
            state.clock.now_unix_ms(),
        )
        .await
    {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(err) => session_tree_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetryRequestBody {
    #[serde(default)]
    request_id: Option<String>,
    /// 目标消息（user=该回合；assistant=其前方最近 user 回合）。
    /// 缺省 = 最近 user 回合（现役 latestUserOnly 兼容形态）。
    /// 重置点只由服务端从当前分支解析——客户端传 new_head 的旧口径已废止。
    #[serde(default)]
    target_message_id: Option<String>,
    /// 文件回退（现役 sessions.ts:1584-1607 契约）：缺省/none = 不回退；
    /// workspace = 随 retry 内容级回滚工作区文件（需偏好开启，未开 403
    /// file_rollback_disabled）；其他值响亮 400 invalid_file_rollback。
    #[serde(default)]
    file_rollback: Option<String>,
}

async fn workspace_rollback_get_route(State(state): State<ServiceState>) -> Response {
    Json(serde_json::json!({"enabled": state.rollback_file_changes_enabled()})).into_response()
}

#[derive(serde::Deserialize)]
struct WorkspaceRollbackPreferenceBody {
    enabled: bool,
}

async fn workspace_rollback_put_route(
    State(state): State<ServiceState>,
    body: Result<Json<WorkspaceRollbackPreferenceBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    match state.save_rollback_file_changes(request.enabled).await {
        Ok(()) => Json(serde_json::json!({"ok": true, "enabled": request.enabled})).into_response(),
        Err(err) => EndpointError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            format!("cannot save workspace rollback preference: {err}"),
        )
        .into_response(),
    }
}

/// `POST /lingxi/v1/sessions/{id}/turns/retry` — 服务端解析重置点 →
/// 分支头回移 + reset 标记；回合输入随响应返回，客户端经 execute 提交为
/// 全新 run（D6 两段式；旧结果永不覆盖）。fileRollback=workspace 时在分支
/// 重置前对工作区做内容级回滚（与 rewind 同一机制），报告随响应返回。
async fn retry_turn_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<RetryRequestBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    // 现役契约（sessions.ts:1584-1597）：fileRollback 只接受 none/workspace；
    // 其他值 → 400 invalid_file_rollback，绝不静默降级成 none。
    let file_rollback = match req.file_rollback.as_deref() {
        None | Some("none") => session_tree::FileRollbackMode::None,
        Some("workspace") => session_tree::FileRollbackMode::Workspace,
        Some(_) => {
            return EndpointError::invalid_message("invalid fileRollback")
                .with_reason("invalid_file_rollback")
                .into_response();
        }
    };
    if let Err(resp) = gate_session_tree_write(&state, &principal, &session_id).await {
        return resp;
    }
    let rollback_enabled = state.rollback_file_changes_enabled();
    match state
        .session_tree
        .retry_turn(
            &principal,
            &session_id,
            req.target_message_id.as_deref(),
            file_rollback,
            rollback_enabled,
            req.request_id.as_deref(),
            state.clock.now_unix_ms(),
        )
        .await
    {
        Ok(outcome) => (StatusCode::OK, Json(outcome)).into_response(),
        Err(err) => session_tree_error_response(err),
    }
}

/// `POST /lingxi/v1/sessions/{id}/turns/rollback-preview` — 只读预览重试轮次
/// 的分支重置点与文件影响（现役 /api/sessions/turns/rollback-preview 对应面，
/// R08 收尾补齐；不写状态、不动文件、不发布事件）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TurnsRollbackPreviewBody {
    #[serde(default)]
    target_message_id: Option<String>,
}

async fn turns_rollback_preview_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<TurnsRollbackPreviewBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if let Err(resp) = gate_session_tree_write(&state, &principal, &session_id).await {
        return resp;
    }
    let rollback_enabled = state.rollback_file_changes_enabled();
    match state
        .session_tree
        .preview_turn_rollback(
            &session_id,
            req.target_message_id.as_deref(),
            rollback_enabled,
        )
        .await
    {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => session_tree_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RewindRequestBody {
    #[serde(default)]
    request_id: Option<String>,
    checkpoint: String,
    #[serde(default)]
    restore_files: bool,
}

/// `POST /lingxi/v1/sessions/{id}/rewind` — 回退到具名检查点（R06-A06）。
/// 重置点由服务端从检查点目标消息解析；restoreFiles 时内容级回滚：
/// 逐文件三档判定（restored/conflicted/skipped/failed），冲突文件保留
/// 用户修改、分支照常回移、收据如实标注（REPAIR-R1 FINDING-05 裁决语义）。
async fn rewind_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<RewindRequestBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if let Err(resp) = gate_session_tree_write(&state, &principal, &session_id).await {
        return resp;
    }
    // 回退偏好闸（现役 getRollbackFileChanges）：restoreFiles 且偏好未开 → 403。
    let rollback_enabled = state.rollback_file_changes_enabled();
    match state
        .session_tree
        .rewind_to_checkpoint(
            &session_id,
            session_tree::RewindRequest {
                checkpoint_name: &req.checkpoint,
                restore_files: req.restore_files,
                file_rollback_enabled: rollback_enabled,
                principal: &principal,
                request_id: req.request_id.as_deref(),
                now_unix_ms: state.clock.now_unix_ms(),
            },
        )
        .await
    {
        Ok(receipt) => (StatusCode::OK, Json(receipt)).into_response(),
        Err(err) => session_tree_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RewindPreviewRequestBody {
    checkpoint: String,
    #[serde(default)]
    restore_files: bool,
}

/// `POST /lingxi/v1/sessions/{id}/rewind/preview` — 只读预览：分支重置点 +
/// 逐文件判定；不写状态、不发布事件（ROLLBACK-PREVIE 叶）。
async fn rewind_preview_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<RewindPreviewRequestBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    // 预览是读操作：归属闸，但不闸生命周期（归档会话也允许预览其检查点）。
    match state.sessions.get_for(&principal, &session_id).await {
        Ok(sessions::SessionAccess::Ok(_)) => {}
        Ok(sessions::SessionAccess::NotFound) => {
            return EndpointError::not_found().into_response();
        }
        Ok(sessions::SessionAccess::Forbidden) => {
            return EndpointError::forbidden("cross_principal_access").into_response();
        }
        Err(err) => return EndpointError::storage(&err).into_response(),
    }
    let rollback_enabled = state.rollback_file_changes_enabled();
    match state
        .session_tree
        .preview_rewind(
            &principal,
            &session_id,
            &req.checkpoint,
            req.restore_files,
            rollback_enabled,
        )
        .await
    {
        Ok(preview) => (StatusCode::OK, Json(preview)).into_response(),
        Err(err) => session_tree_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateCheckpointBody {
    name: String,
    target_message_id: String,
    /// 需要记录版本的文件（rewind 冲突检测基线）；逐条须在会话授权目录下。
    #[serde(default)]
    file_paths: Vec<String>,
}

/// `POST /lingxi/v1/sessions/{id}/checkpoints` — 创建具名检查点
/// （latest 覆盖/其余重名 409/上限 200；文件版本经授权目录闸）。
async fn create_checkpoint_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<CreateCheckpointBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if req.name.trim().is_empty() {
        return EndpointError::invalid_message("checkpoint name must be non-empty").into_response();
    }
    if let Err(resp) = gate_session_tree_write(&state, &principal, &session_id).await {
        return resp;
    }
    match state
        .session_tree
        .create_checkpoint(
            &principal,
            &session_id,
            req.name.trim(),
            &req.target_message_id,
            &req.file_paths,
            state.clock.now_unix_ms(),
        )
        .await
    {
        Ok(row) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "checkpointId": row.checkpoint_id,
                "sessionId": row.session_id,
                "name": row.name,
                "targetMessageId": row.target_message_id,
                "messageCount": row.message_count,
                "createdAtUnixMs": row.created_at_unix_ms,
            })),
        )
            .into_response(),
        Err(err) => session_tree_error_response(err),
    }
}

/// `GET /lingxi/v1/sessions/{id}/checkpoints` — 列出具名检查点。
async fn list_checkpoints_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    match state.sessions.get_for(&principal, &session_id).await {
        Ok(sessions::SessionAccess::Ok(_)) => {}
        Ok(sessions::SessionAccess::NotFound) => {
            return EndpointError::not_found().into_response();
        }
        Ok(sessions::SessionAccess::Forbidden) => {
            return EndpointError::forbidden("cross_principal_access").into_response();
        }
        Err(err) => return EndpointError::storage(&err).into_response(),
    }
    match state.session_tree.list_checkpoints(&session_id).await {
        Ok(rows) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "checkpoints": rows.iter().map(|r| serde_json::json!({
                    "checkpointId": r.checkpoint_id,
                    "name": r.name,
                    "targetMessageId": r.target_message_id,
                    "messageCount": r.message_count,
                    "createdAtUnixMs": r.created_at_unix_ms,
                })).collect::<Vec<_>>(),
            })),
        )
            .into_response(),
        Err(err) => session_tree_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteCheckpointBody {
    name: String,
}

/// `POST /lingxi/v1/sessions/{id}/checkpoints/delete` — 删除具名检查点
/// （不存在 → 404；现役检查点删除走 tool 面，本路由是其 REST 面）。
async fn delete_checkpoint_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<DeleteCheckpointBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if let Err(resp) = gate_session_tree_write(&state, &principal, &session_id).await {
        return resp;
    }
    match state
        .session_tree
        .delete_checkpoint(&session_id, &req.name)
        .await
    {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Ok(false) => EndpointError::not_found().into_response(),
        Err(err) => session_tree_error_response(err),
    }
}

// ------------------------------------------------------- R06-T03 会话管理面
// 现役锚点（sessions.ts）：rename:2493 / cleanup:2517 / archived:2555 /
// archive:2569 / restore:2813 / archived-delete:2882 / pin:1014 /
// pin-order:1052 / search:857 / find:922 / memory GET:1103 PATCH:1135。
// 归属闸：会话级操作经 get_for；列表级操作按主体 owner 过滤
// （本地主人视野 = 全部，设备主体 = 本人）。

fn session_admin_error_response(err: session_admin::SessionAdminError) -> Response {
    use session_admin::SessionAdminError as E;
    match err {
        E::NotFound => EndpointError::not_found().into_response(),
        E::InactiveMemorySession => {
            EndpointError::forbidden("session_memory_inactive").into_response()
        }
        E::LegacyFileConflict { detail } => {
            EndpointError::resource_failure(StatusCode::CONFLICT, "session_file_conflict", detail)
                .into_response()
        }
        E::WrongLifecycle { detail } => {
            EndpointError::new(StatusCode::CONFLICT, ErrorCode::Conflict, detail)
                .with_reason("session_lifecycle_conflict")
                .into_response()
        }
        E::IdentityRetired { session_id } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!("session {session_id} identity is retired"),
        )
        .with_reason("session_identity_retired")
        .into_response(),
        E::SessionExists { session_id } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            format!("session {session_id} already exists"),
        )
        .with_reason("session_exists")
        .with_cause("session.exists")
        .into_response(),
        E::ChildrenPresent { child_count } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            "child sessions present",
        )
        .with_reason("child_sessions_present")
        .with_detail_value("childCount", serde_json::json!(child_count))
        .into_response(),
        E::InvalidRequest { detail } => EndpointError::invalid_message(detail).into_response(),
        E::Busy => EndpointError::session_busy().into_response(),
        E::Storage(err) => EndpointError::storage(&err).into_response(),
        E::ArchiveFailed {
            source,
            view,
            rolled_back,
        } => EndpointError::storage(&source)
            .with_detail_value("archive", serde_json::json!(view))
            .with_detail_value("rolledBack", serde_json::json!(rolled_back))
            .into_response(),
    }
}

/// 列表级操作的主体过滤：本地主人 = None（全部）；设备主体 = 本人 user id。
fn owner_filter(principal: &Principal) -> Option<&str> {
    if principal.is_local_owner() {
        None
    } else {
        principal.user_id.as_deref()
    }
}

/// 会话级管理操作的归属闸（不做生命周期闸——各操作生命周期语义不同：
/// archive 要 active、restore/delete 要 archived，由服务层结局区分）。
/// Err(Response) 同 gate_session_tree_write：handler 原样 return。
#[allow(clippy::result_large_err)]
async fn gate_session_owned(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Result<(), Response> {
    get_session_owned(state, principal, session_id)
        .await
        .map(|_| ())
}

#[allow(clippy::result_large_err)]
async fn get_session_owned(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Result<sessions::SessionFacts, Response> {
    match state.sessions.get_for(principal, session_id).await {
        Ok(sessions::SessionAccess::Ok(session)) => Ok(session),
        Ok(sessions::SessionAccess::NotFound) => Err(EndpointError::not_found().into_response()),
        Ok(sessions::SessionAccess::Forbidden) => {
            Err(EndpointError::forbidden("cross_principal_access").into_response())
        }
        Err(err) => Err(EndpointError::storage(&err).into_response()),
    }
}

#[allow(clippy::result_large_err)]
fn gate_agent_not_deleted(state: &ServiceState, agent_id: &str) -> Result<(), Response> {
    match state.memory().is_agent_deleted_checked(agent_id) {
        Ok(true) => Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error":"agent_deleted","reason":"agent_deleted"})),
        )
            .into_response()),
        Ok(false) => Ok(()),
        Err(error) => Err(memory::error_response(&error)),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateSessionBody {
    session_id: String,
    agent_id: String,
    title: String,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    authorized_folders: Vec<String>,
    #[serde(default = "default_true")]
    memory_enabled: bool,
}

fn default_true() -> bool {
    true
}

/// `POST /lingxi/v1/sessions` — 创建顶层会话（谱系深度 0；授权目录须为
/// 现存目录，否则响亮 400；fork 不走这里）。
async fn create_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<CreateSessionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if req.session_id.trim().is_empty() || req.title.trim().is_empty() {
        return EndpointError::invalid_message("sessionId and title must be non-empty")
            .into_response();
    }
    let owner = principal
        .user_id
        .clone()
        .unwrap_or_else(|| crate::auth::LOCAL_OWNER_USER_ID.to_string());
    let mut normalized_folders = Vec::with_capacity(req.authorized_folders.len());
    for folder in &req.authorized_folders {
        match validate_authorized_folder(Some(folder)) {
            Ok(folder) => normalized_folders.push(folder),
            Err(response) => return response,
        }
    }
    match state
        .session_admin
        .create_session(
            req.session_id.trim(),
            req.agent_id.trim(),
            &owner,
            req.title.trim(),
            req.permission_mode.as_deref(),
            &normalized_folders,
            req.memory_enabled,
            state.clock.now_unix_ms(),
        )
        .await
    {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "sessionId": req.session_id.trim(),
                "ownerUserId": owner,
            })),
        )
            .into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RenameSessionBody {
    session_id: String,
    title: String,
}

/// `POST /lingxi/v1/sessions/rename` — 改名（现役 sessions.ts:2493）。
async fn rename_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<RenameSessionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if let Err(resp) = gate_session_owned(&state, &principal, &req.session_id).await {
        return resp;
    }
    match state
        .session_admin
        .rename_session(&req.session_id, &req.title)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PinSessionBody {
    session_id: String,
    pinned: bool,
}

/// `POST /lingxi/v1/sessions/pin` — 置顶/取消置顶（现役 :1014）。
async fn pin_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<PinSessionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let session = match get_session_owned(&state, &principal, &req.session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if req.pinned {
        if let Err(response) = gate_agent_not_deleted(&state, &session.agent_id) {
            return response;
        }
    }
    match state
        .session_admin
        .set_pinned(&req.session_id, req.pinned, state.clock.now_unix_ms())
        .await
    {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PinOrderBody {
    session_ids: Vec<String>,
}

/// `POST /lingxi/v1/sessions/pin-order` — 置顶区整体重编号
/// （现役 :1052；重复 id 响亮 400；每个 id 独立归属校验——一次请求跨多个
/// 会话，授权不只看第一个）。
async fn pin_order_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<PinOrderBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if req.session_ids.is_empty() {
        return EndpointError::invalid_message("sessionIds must be non-empty").into_response();
    }
    for id in &req.session_ids {
        if let Err(resp) = gate_session_owned(&state, &principal, id).await {
            return resp;
        }
    }
    match state
        .session_admin
        .set_pin_order(&req.session_ids, state.clock.now_unix_ms())
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveSessionBody {
    session_id: String,
    /// 子对话处置：archive_children / detach_children；缺省且有子对话 → 409。
    #[serde(default)]
    child_mode: Option<String>,
}

/// `POST /lingxi/v1/sessions/archive` — 归档（现役 :2569；子对话策略
/// 与流式跳过语义同现役）。
async fn archive_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<ArchiveSessionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if let Err(resp) = gate_session_owned(&state, &principal, &req.session_id).await {
        return resp;
    }
    let child_mode = match req.child_mode.as_deref() {
        None => None,
        Some("archive_children") => {
            Some(lingxi_adapters::storage::session_admin::ChildMode::ArchiveChildren)
        }
        Some("detach_children") => {
            Some(lingxi_adapters::storage::session_admin::ChildMode::DetachChildren)
        }
        Some(other) => {
            return EndpointError::invalid_message(format!(
                "unknown childMode {other} (archive_children|detach_children)"
            ))
            .into_response();
        }
    };
    match state
        .session_admin
        .archive_session(&req.session_id, child_mode, state.clock.now_unix_ms())
        .await
    {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionIdBody {
    session_id: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestoreSessionBody {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default, alias = "path")]
    session_path: Option<std::path::PathBuf>,
}
/// `POST /lingxi/v1/sessions/restore` — 恢复归档会话（现役 :2813；仅归档态）。
async fn restore_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<RestoreSessionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let requested_path = match req.session_path {
        Some(path) => {
            if !path.is_absolute() {
                return EndpointError::forbidden("invalid_session_path").into_response();
            }
            let root = state.storage.legacy_session_root().unwrap();
            let path = if let Ok(rel) = path.strip_prefix(&state.config.data_home) {
                root.join(rel)
            } else {
                path
            };
            Some(path)
        }
        None => None,
    };
    let path_id = if let Some(path) = &requested_path {
        match lingxi_adapters::storage::legacy_session_files::session_id_for_path(
            &state.storage,
            path,
        )
        .await
        {
            Ok(id) => Some(id),
            Err(StorageError::InvalidRequest { detail })
                if detail == "legacy_session_file_not_found" =>
            {
                return EndpointError::not_found().into_response()
            }
            Err(error) => {
                return EndpointError::forbidden(&format!("invalid_session_path: {error}"))
                    .into_response()
            }
        }
    } else {
        None
    };
    let session_id = match (req.session_id, path_id) {
        (Some(id), Some(path_id)) if id != path_id => {
            return EndpointError::resource_failure(
                StatusCode::CONFLICT,
                "session_locator_mismatch",
                "sessionId and sessionPath disagree",
            )
            .into_response()
        }
        (Some(id), _) | (_, Some(id)) if !id.is_empty() => id,
        _ => {
            return EndpointError::invalid_message("sessionId or sessionPath is required")
                .into_response()
        }
    };
    let facts = match get_session_owned(&state, &principal, &session_id).await {
        Ok(facts) => facts,
        Err(response) => return response,
    };
    let expected = lingxi_adapters::storage::legacy_session_files::ExpectedSource {
        agent: facts.agent_id,
        owner: facts.owner_user_id,
        path: requested_path,
    };
    match state
        .session_admin
        .restore_session_checked(&session_id, state.clock.now_unix_ms(), expected)
        .await
    {
        Ok(Some(path)) => (
            StatusCode::OK,
            Json(serde_json::json!({"ok":true,"sessionId":session_id,"restoredPath":path})),
        )
            .into_response(),
        Ok(None) => (StatusCode::OK, Json(serde_json::json!({"ok":true}))).into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

/// `POST /lingxi/v1/sessions/archived/delete` — 永久删除已归档会话
/// （现役 :2882；仅归档态；FK 依赖序清全部派生数据）。
async fn delete_archived_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<SessionIdBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    if let Err(resp) = gate_session_owned(&state, &principal, &req.session_id).await {
        return resp;
    }
    match state
        .session_admin
        .delete_archived_session(&req.session_id)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupBody {
    #[serde(default = "default_max_age_days")]
    max_age_days: u64,
}

/// 现役 cleanup 默认窗口（sessions.ts:2519 `maxAgeDays = 90`）。
fn default_max_age_days() -> u64 {
    90
}

/// `POST /lingxi/v1/sessions/cleanup` — 清理过期归档（现役 :2517）。
async fn cleanup_archived_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<CleanupBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    match state
        .session_admin
        .cleanup_archived(
            owner_filter(&principal),
            req.max_age_days,
            state.clock.now_unix_ms(),
        )
        .await
    {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

/// `GET /lingxi/v1/sessions/archived` — 列出已归档会话（现役 :2555）。
async fn list_archived_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
) -> Response {
    match state
        .session_admin
        .list_archived(owner_filter(&principal))
        .await
    {
        Ok(rows) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "sessions": rows.iter().map(|r| serde_json::json!({
                    "sessionId": r.session_id,
                    "agentId": r.agent_id,
                    "title": r.title,
                    "lifecycle": r.lifecycle,
                    "createdAtUnixMs": r.created_at_unix_ms,
                    "archivedAtUnixMs": r.archived_at_unix_ms,
                    "firstMessage": r.first_message,
                })).collect::<Vec<_>>(),
            })),
        )
            .into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

/// `GET /lingxi/v1/sessions/search?q=&phase=&limit=` — 会话搜索
/// （现役 :857；query 上限 512；phase=title|content）。
async fn search_sessions_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    let pairs = ws::parse_query_pairs(query.as_deref().unwrap_or(""));
    let mut q = String::new();
    let mut phase = String::from("title");
    let mut limit = 50usize;
    for (key, value) in pairs {
        match key.as_str() {
            "q" => q = value,
            "phase" => phase = value,
            "limit" => match value.parse::<usize>() {
                Ok(n) => limit = n,
                Err(_) => {
                    return EndpointError::invalid_message("limit must be a number")
                        .into_response();
                }
            },
            _ => {
                return EndpointError::invalid_message(format!("unknown query key {key}"))
                    .into_response();
            }
        }
    }
    match state
        .session_admin
        .search(owner_filter(&principal), &q, &phase, limit)
        .await
    {
        Ok(hits) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "query": q,
                "phase": phase,
                "results": hits.iter().map(|h| serde_json::json!({
                    "sessionId": h.session_id,
                    "title": h.title,
                    "snippet": h.snippet,
                    "matchKind": h.match_kind,
                    "createdAtUnixMs": h.created_at_unix_ms,
                    "lastActivityUnixMs": h.last_activity_unix_ms,
                })).collect::<Vec<_>>(),
            })),
        )
            .into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

/// `GET /lingxi/v1/sessions/find?sessionId=` — 按 id 定位会话
/// （现役 :922 manifest 定位；本实现返回会话行全列投影）。
async fn find_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    let pairs = ws::parse_query_pairs(query.as_deref().unwrap_or(""));
    let mut session_id: Option<String> = None;
    for (key, value) in pairs {
        match key.as_str() {
            "sessionId" => session_id = Some(value),
            _ => {
                return EndpointError::invalid_message(format!("unknown query key {key}"))
                    .into_response();
            }
        }
    }
    let Some(session_id) = session_id else {
        return EndpointError::invalid_message("missing query key sessionId").into_response();
    };
    if let Err(resp) = gate_session_owned(&state, &principal, &session_id).await {
        return resp;
    }
    match state.session_admin.get_session_row(&session_id).await {
        Ok(Some(row)) => (StatusCode::OK, Json(session_row_json(&row))).into_response(),
        Ok(None) => EndpointError::not_found().into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

/// 会话行全列投影的 JSON 形状（find/get 扩展共用）。
fn session_row_json(
    row: &lingxi_adapters::storage::session_tree::SessionTreeRow,
) -> serde_json::Value {
    serde_json::json!({
        "sessionId": row.session_id,
        "agentId": row.agent_id,
        "ownerUserId": row.owner_user_id,
        "title": row.title,
        "createdAtUnixMs": row.created_at_unix_ms,
        "parentSessionId": row.parent_session_id,
        "forkPointMessageId": row.fork_point_message_id,
        "lineageDepth": row.lineage_depth,
        "lifecycle": row.lifecycle,
        "pinnedAtUnixMs": row.pinned_at_unix_ms,
        "pinOrder": row.pin_order,
        "memoryEnabled": row.memory_enabled,
        "authorizedFolders": row.authorized_folders,
        // D-7（2026-10-24）：旧栈 switch 投影把无显式模式归一化为默认 ask；
            // Rust 透传 DB 原值会让迁移会话拿到 null。对齐旧栈：null/空 → "ask"。
            "permissionMode": row.permission_mode.as_deref().filter(|m| !m.is_empty()).unwrap_or("ask"),
        "archivedAtUnixMs": row.archived_at_unix_ms,
        "lastActivityUnixMs": row.last_activity_unix_ms,
    })
}

/// `GET /lingxi/v1/sessions/{id}/memory` — 记忆开关读取（现役 :1103）。
async fn get_memory_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    let session = match get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = gate_agent_not_deleted(&state, &session.agent_id) {
        return response;
    }
    match state.session_admin.get_memory_enabled(&session_id).await {
        Ok(enabled) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "memoryEnabled": enabled })),
        )
            .into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryPatchBody {
    memory_enabled: bool,
}

/// `PATCH /lingxi/v1/sessions/{id}/memory` — 记忆开关写入（现役 :1135）。
async fn patch_memory_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<MemoryPatchBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let session = match get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = gate_agent_not_deleted(&state, &session.agent_id) {
        return response;
    }
    match state
        .session_admin
        .set_memory_enabled(&session_id, req.memory_enabled)
        .await
    {
        Ok(enabled) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "memoryEnabled": enabled })),
        )
            .into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

/// `GET /lingxi/v1/sessions/{id}/authorized-folders` — 授权目录读取
/// （现役 :1178；cwd/workspaceFolders/sandboxFolders 本端口不由服务端
/// 跟踪，固定返回 null/空数组，不猜造）。
async fn get_authorized_folders_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    let session = match get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = gate_agent_not_deleted(&state, &session.agent_id) {
        return response;
    }
    match state.session_admin.authorized_folders(&session_id).await {
        Ok(folders) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "sessionId": session_id,
                "cwd": serde_json::Value::Null,
                "workspaceFolders": [],
                "authorizedFolders": folders,
                "sandboxFolders": [],
            })),
        )
            .into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizedFoldersPatchBody {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    folders: Option<Vec<String>>,
}

/// 现役 validateAuthorizedFolder：必填 → 绝对化 → 必须存在且为目录；
/// 全部按 400 家族显式拒绝。
#[allow(clippy::result_large_err)]
fn validate_authorized_folder(raw: Option<&str>) -> Result<String, Response> {
    let Some(folder) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(EndpointError::invalid_message("folder is required").into_response());
    };
    let absolute = std::path::absolute(folder)
        .map_err(|err| EndpointError::invalid_message(format!("folder path invalid: {err}")))
        .unwrap_or_else(|_| std::path::PathBuf::from(folder));
    let metadata = match std::fs::metadata(&absolute) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Err(EndpointError::invalid_message("folder does not exist").into_response());
        }
    };
    if !metadata.is_dir() {
        return Err(EndpointError::invalid_message("folder must be a directory").into_response());
    }
    Ok(absolute
        .canonicalize()
        .unwrap_or(absolute)
        .to_string_lossy()
        .into_owned())
}

/// 现役 normalizeAuthorizedFolderPath：remove 分支只规范化，不要求存在。
#[allow(clippy::result_large_err)]
fn normalize_authorized_folder(raw: Option<&str>) -> Result<String, Response> {
    let Some(folder) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(EndpointError::invalid_message("folder is required").into_response());
    };
    let absolute = std::path::absolute(folder).unwrap_or_else(|_| std::path::PathBuf::from(folder));
    Ok(absolute
        .canonicalize()
        .unwrap_or(absolute)
        .to_string_lossy()
        .into_owned())
}

/// `PATCH /lingxi/v1/sessions/{id}/authorized-folders` — add/remove/set
/// （现役 :1206）。
async fn patch_authorized_folders_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<AuthorizedFoldersPatchBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let session = match get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = gate_agent_not_deleted(&state, &session.agent_id) {
        return response;
    }
    let action = req.action.as_deref().map(str::trim).unwrap_or("set");
    let mutation = match action {
        "add" => match validate_authorized_folder(req.folder.as_deref()) {
            Ok(folder) => session_admin::FolderAction::Add(folder),
            Err(response) => return response,
        },
        "remove" => match normalize_authorized_folder(req.folder.as_deref()) {
            Ok(folder) => session_admin::FolderAction::Remove(folder),
            Err(response) => return response,
        },
        "set" => {
            let Some(folders) = req.folders.as_ref() else {
                return EndpointError::invalid_message("folders is required").into_response();
            };
            let mut normalized = Vec::with_capacity(folders.len());
            for folder in folders {
                match validate_authorized_folder(Some(folder)) {
                    Ok(folder) => normalized.push(folder),
                    Err(response) => return response,
                }
            }
            session_admin::FolderAction::Set(normalized)
        }
        other => {
            return EndpointError::invalid_message(format!(
                "Invalid action {other} (add|remove|set)"
            ))
            .into_response();
        }
    };
    match state
        .session_admin
        .mutate_authorized_folders(&session_id, mutation)
        .await
    {
        Ok(folders) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "sessionId": session_id,
                "cwd": serde_json::Value::Null,
                "workspaceFolders": [],
                "authorizedFolders": folders,
                "sandboxFolders": [],
            })),
        )
            .into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

/// `POST /lingxi/v1/sessions/switch` — 焦点会话切换（现役 :2323）。
/// 端口边界：浏览器恢复在无 browser host 的服务端按现役 server/PWA 分支
/// 记录 typed skip，不猜造浏览器状态。
async fn switch_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<SwitchSessionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let session_id = {
        let trimmed = req.session_id.trim();
        if trimmed.is_empty() {
            return EndpointError::invalid_message("sessionId is required").into_response();
        }
        trimmed.to_string()
    };
    let session = match get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = gate_agent_not_deleted(&state, &session.agent_id) {
        return response;
    }
    let row =
        match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), &session_id)
            .await
        {
            Ok(Some(row)) => row,
            Ok(None) => return EndpointError::not_found().into_response(),
            Err(err) => return EndpointError::storage(&err).into_response(),
        };
    if row.archived_at_unix_ms.is_some() {
        return EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            "archived session must be restored before switch",
        )
        .with_reason("session_lifecycle_conflict")
        .into_response();
    }
    let metadata = match session_ui::read(state.storage(), &session_id) {
        Ok(value) => value,
        Err(error) => return EndpointError::storage(&error).into_response(),
    };
    // 没配置模型的服务仍可浏览既有会话，但须如实声明模型当前不可用。
    let choice_result = if state.model_gateway().is_some() {
        chat_settings::read_session_choice(&state, &session)
    } else {
        chat_settings::choice(state.memory().home(), &session.owner_user_id, &session_id)
    };
    let choice = match choice_result {
        Ok(value) => value,
        Err(error) => {
            return EndpointError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                error,
            )
            .into_response()
        }
    };
    let streaming = state.sessions().session_supervisor().is_busy(&session_id);
    let _focus = state.focus_mutation.lock().await;
    let _application = character_cards::CARD_APPLICATION_GATE.lock().await;
    if let Err(response) = gate_agent_not_deleted(&state, &session.agent_id) {
        return response;
    }
    // 只有数据库历史、尚无本地助手目录的旧会话仍可浏览；不能据此制造可创建会话的助手偏好。
    let focus = match state.memory().agent_dir_checked(&session.agent_id) {
        Ok(_) => state
            .memory()
            .set_current_focus(&session.agent_id, &session_id),
        Err(error) if error.is_missing() => state.memory().set_current_session(&session_id),
        Err(error) => Err(error),
    };
    if let Err(err) = focus {
        return EndpointError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            err.message,
        )
        .into_response();
    }
    // 焦点提交后等待当前桌面实际恢复；下游失败须如实呈现，不能假称恢复成功。
    let browser_resume = match state
        .browsers()
        .resume_for(
            &principal,
            &session_id,
            format!("restore-browser-{}", state.request_ids().next_request_id()),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => {
            return EndpointError::new(
                StatusCode::BAD_GATEWAY,
                ErrorCode::Internal,
                error.to_string(),
            )
            .with_reason("browser_resume_failed")
            .into_response()
        }
    };
    let history_revision = match (
        lingxi_adapters::storage::canonical_message_store::read_branch_head_revision(
            state.storage(),
            &session_id,
        )
        .await,
        lingxi_adapters::storage::canonical_message_store::read_history_event_revision(
            state.storage(),
            &session_id,
        )
        .await,
    ) {
        (Ok(head), Ok(event)) => format!("\"r{head}-e{event}\""),
        (Err(error), _) | (_, Err(error)) => return EndpointError::storage(&error).into_response(),
    };
    let agent_name = state
        .memory()
        .agent_display_name(&session.agent_id)
        .unwrap_or_else(|| session.agent_id.clone());
    state.app_events().publish(
        "session-focus-changed",
        serde_json::json!({ "sessionId": session_id, "agentId": session.agent_id }),
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "sessionId": session_id,
            "agentId": session.agent_id,
            "agentName": agent_name,
            "memoryEnabled": row.memory_enabled,
            "authorizedFolders": row.authorized_folders,
            "path": session_locator::native(&session_id),
            "workspaceFolders": metadata.as_ref().map(|value| value.workspace_folders.clone()).unwrap_or_default(),
            "cwd": metadata.as_ref().and_then(|value| value.cwd.clone()),
            "projectId": row.project_id,
            // D-7（2026-10-24）：旧栈 switch 投影把无显式模式归一化为默认 ask；
            // Rust 透传 DB 原值会让迁移会话拿到 null。对齐旧栈：null/空 → "ask"。
            "permissionMode": row.permission_mode.as_deref().filter(|m| !m.is_empty()).unwrap_or("ask"),
            "thinkingLevel": choice.thinking_level,
            "currentModelProvider": choice.model.as_ref().map(|pair| pair.0.clone()),
            "currentModelId": choice.model.as_ref().map(|pair| pair.1.clone()),
            "currentModelAvailable": state.model_gateway().is_some(),
            "currentModelUnavailableReason": if state.model_gateway().is_none() { Some("model_plane_unavailable") } else { None },
            "isStreaming": streaming,
            "historyRevision": history_revision,
            "browserResume": browser_resume,
            "browserRunning": browser_resume["found"] == true,
            "browserUrl": browser_resume.get("url"),
        })),
    )
        .into_response()
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SwitchSessionBody {
    session_id: String,
}

/// `GET/PUT /lingxi/v1/sessions/{id}/project` — 项目归属读/写
/// （现役 setSessionProjectAssignment 的最小端口：分配或 null 清除，
/// 返回新分配；项目目录树属 E 块）。
async fn get_session_project_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    let session = match get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let _ = session;
    match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), &session_id)
        .await
    {
        Ok(Some(row)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "sessionId": session_id,
                "projectId": row.project_id,
            })),
        )
            .into_response(),
        Ok(None) => EndpointError::not_found().into_response(),
        Err(err) => EndpointError::storage(&err).into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionProjectBody {
    project_id: Option<String>,
}

async fn put_session_project_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<SessionProjectBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let session = match get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = gate_agent_not_deleted(&state, &session.agent_id) {
        return response;
    }
    match state
        .session_admin
        .set_session_project(&session_id, req.project_id.as_deref())
        .await
    {
        Ok(Some(project)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "sessionId": session_id,
                "projectId": project,
            })),
        )
            .into_response(),
        Ok(None) => EndpointError::not_found().into_response(),
        Err(err) => session_admin_error_response(err),
    }
}

/// R06 会话 id 生成：毫秒 + 进程级原子序号，同毫秒多次创建不碰撞
/// （裸 `pid-ms` 在一毫秒内两次轮换会撞 session_exists）。
pub(crate) fn fresh_session_id(now_unix_ms: u64) -> String {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("sess-{}-{:x}-{seq}", std::process::id(), now_unix_ms)
}

/// `POST /lingxi/v1/sessions/rotate` — 轮换新会话（现役 POST /sessions/new
/// 的引擎轮换语义端口）：归档该助手最近一个活动会话后新建并接管焦点；
/// 无活动会话时仍创建。接管态（多客户端焦点抢占）本端口无此概念，
/// 不以猜造的拒绝冒充。
async fn rotate_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<RotateSessionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let agent_id = match req
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| state.memory().current_agent_preference().ok().flatten())
    {
        Some(agent) => agent,
        None => {
            return EndpointError::invalid_message(
                "agentId is required when no current agent is set",
            )
            .into_response();
        }
    };
    if let Err(response) = gate_agent_not_deleted(&state, &agent_id) {
        return response;
    }
    let previous = match lingxi_adapters::storage::session_admin::latest_active_session_for_agent(
        state.storage(),
        &agent_id,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let mut archived_session_id = None;
    if let Some(previous_id) = previous.as_deref() {
        match get_session_owned(&state, &principal, previous_id).await {
            Ok(_) => match state
                .session_admin
                .archive_session(previous_id, None, state.clock.now_unix_ms())
                .await
            {
                Ok(_) => archived_session_id = Some(previous_id.to_string()),
                Err(session_admin::SessionAdminError::Busy) => {
                    return EndpointError::new(
                        StatusCode::CONFLICT,
                        ErrorCode::Conflict,
                        "previous session is busy; retry after it settles",
                    )
                    .with_reason("session_busy")
                    .into_response();
                }
                Err(err) => return session_admin_error_response(err),
            },
            Err(response) => return response,
        }
    }
    let session_id = fresh_session_id(state.clock.now_unix_ms());
    let owner = principal
        .user_id
        .clone()
        .unwrap_or_else(|| crate::auth::LOCAL_OWNER_USER_ID.to_string());
    let title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "新会话".to_string());
    if let Err(err) = state
        .session_admin
        .create_session(
            &session_id,
            &agent_id,
            &owner,
            &title,
            None,
            &[],
            true,
            state.clock.now_unix_ms(),
        )
        .await
    {
        return session_admin_error_response(err);
    }
    let mut project_assigned: Option<Option<String>> = None;
    if req.project_id.is_some() {
        match state
            .session_admin
            .set_session_project(&session_id, req.project_id.as_deref())
            .await
        {
            Ok(Some(value)) => project_assigned = Some(value),
            Ok(None) => return EndpointError::not_found().into_response(),
            Err(err) => return session_admin_error_response(err),
        }
    }
    // 轮换接管焦点（区别于 new-detached）。
    if let Err(err) = state.memory().set_current_session(&session_id) {
        return EndpointError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            err.message,
        )
        .into_response();
    }
    state.app_events().publish(
        "session-created",
        serde_json::json!({
            "sessionId": session_id,
            "agentId": agent_id,
            "rotatedFrom": archived_session_id,
        }),
    );
    if let Some(archived) = archived_session_id.as_deref() {
        state.app_events().publish(
            "session-archived",
            serde_json::json!({ "sessionId": archived }),
        );
    }
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "ok": true,
            "sessionId": session_id,
            "agentId": agent_id,
            "archivedSessionId": archived_session_id,
            "projectId": project_assigned.flatten(),
        })),
    )
        .into_response()
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RotateSessionBody {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
}

/// `POST /lingxi/v1/sessions/continue-deleted-agent` — 续接已删 Agent 的
/// 会话（现役 :2261 端口）：新助手 + 新会话身份，继承标题与授权目录；
/// 助手未删除 → 400 agent_not_deleted；会话须存在且活动。
async fn continue_deleted_agent_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<ContinueDeletedBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let session_id = {
        let trimmed = req.session_id.trim();
        if trimmed.is_empty() {
            return EndpointError::invalid_message("sessionId is required").into_response();
        }
        trimmed.to_string()
    };
    let session = match get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state.memory().is_agent_deleted_checked(&session.agent_id) {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "agent_not_deleted",
                    "reason": "agent_not_deleted",
                })),
            )
                .into_response();
        }
        Err(err) => return memory::error_response(&err),
    }
    let row =
        match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), &session_id)
            .await
        {
            Ok(Some(row)) => row,
            Ok(None) => return EndpointError::not_found().into_response(),
            Err(err) => return EndpointError::storage(&err).into_response(),
        };
    if row.archived_at_unix_ms.is_some() {
        return EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            "archived session must be restored before continue",
        )
        .with_reason("session_lifecycle_conflict")
        .into_response();
    }
    // 新助手身份：优先沿用原助手显示名。
    let previous_name = state.memory().agent_display_name(&session.agent_id);
    let new_name = previous_name.unwrap_or_else(|| "续接助手".to_string());
    let (new_agent_id, new_agent_name) = match state.memory().create_agent(&new_name, None, None) {
        Ok(pair) => pair,
        Err(err) => return err.response(),
    };
    let new_session_id = fresh_session_id(state.clock.now_unix_ms());
    let title = format!("{} (续接)", row.title);
    let owner = principal
        .user_id
        .clone()
        .unwrap_or_else(|| crate::auth::LOCAL_OWNER_USER_ID.to_string());
    if let Err(err) = state
        .session_admin
        .create_session(
            &new_session_id,
            &new_agent_id,
            &owner,
            &title,
            None,
            &row.authorized_folders,
            true,
            state.clock.now_unix_ms(),
        )
        .await
    {
        return session_admin_error_response(err);
    }
    state.app_events().publish(
        "session-created",
        serde_json::json!({
            "sessionId": new_session_id,
            "agentId": new_agent_id,
            "continuedFrom": session_id,
        }),
    );
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "ok": true,
            "sessionId": new_session_id,
            "agentId": new_agent_id,
            "agentName": new_agent_name,
            "continuedFromSessionId": session_id,
            "authorizedFolders": row.authorized_folders,
            "workspaceFolders": [],
            "cwd": serde_json::Value::Null,
            "projectId": serde_json::Value::Null,
            "compacted": false,
            "compactionError": serde_json::Value::Null,
        })),
    )
        .into_response()
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContinueDeletedBody {
    session_id: String,
}

/// `POST /lingxi/v1/sessions/new-detached` — 创建独立会话（现役 :2146）：
/// 不改变当前焦点；可选谱系（forkedFromSessionId，深度过限 409）与项目归属。
async fn new_detached_session_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<NewDetachedBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return EndpointError::from_json_rejection(&rej).into_response(),
    };
    let agent_id = match req
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| state.memory().current_agent_preference().ok().flatten())
    {
        Some(agent) => agent,
        None => {
            return EndpointError::invalid_message(
                "agentId is required when no current agent is set",
            )
            .into_response();
        }
    };
    if let Err(response) = gate_agent_not_deleted(&state, &agent_id) {
        return response;
    }
    // R08 收尾 D02（9D9E72CB6051 A1）：显式 agentId 必须真实存在——此前只查
    // 已删标记，不存在的助手目录也能为幽灵助手建独立会话（t07 ab 实证 201）。
    // 与焦点新建（session_focus::create_session）同一存在性闸。
    if let Err(error) = state.memory().agent_dir_checked(&agent_id) {
        return error.response();
    }
    let mut lineage: Option<(String, i64)> = None;
    if let Some(parent_id) = req
        .forked_from_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let parent = match get_session_owned(&state, &principal, parent_id).await {
            Ok(parent) => parent,
            Err(response) => return response,
        };
        let _ = parent;
        let row = match lingxi_adapters::storage::session_tree::get_session_row(
            state.storage(),
            parent_id,
        )
        .await
        {
            Ok(Some(row)) => row,
            Ok(None) => return EndpointError::not_found().into_response(),
            Err(err) => return EndpointError::storage(&err).into_response(),
        };
        let child_depth =
            match lingxi_kernel::session_tree::check_fork_depth(row.lineage_depth as u32) {
                Some(depth) => depth,
                None => {
                    return EndpointError::new(
                        StatusCode::CONFLICT,
                        ErrorCode::Conflict,
                        format!(
                            "fork lineage exceeds the max derivation depth of {}",
                            lingxi_kernel::session_tree::MAX_FORK_LINEAGE_DEPTH
                        ),
                    )
                    .with_reason("session_fork_depth_limit")
                    .into_response();
                }
            };
        lineage = Some((parent_id.to_string(), i64::from(child_depth)));
    }
    let session_id = req
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fresh_session_id(state.clock.now_unix_ms()));
    let title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let title = title
        .map(str::to_string)
        .unwrap_or_else(|| "独立会话".to_string());
    let owner = principal
        .user_id
        .clone()
        .unwrap_or_else(|| crate::auth::LOCAL_OWNER_USER_ID.to_string());
    if let Err(err) = state
        .session_admin
        .create_session(
            &session_id,
            &agent_id,
            &owner,
            &title,
            None,
            &[],
            true,
            state.clock.now_unix_ms(),
        )
        .await
    {
        return session_admin_error_response(err);
    }
    if let Some((parent_id, depth)) = lineage.as_ref() {
        if let Err(err) = state
            .session_admin
            .set_session_lineage(&session_id, parent_id, *depth as u32)
            .await
        {
            return session_admin_error_response(err);
        }
    }
    let mut project_assigned: Option<Option<String>> = None;
    if req.project_id.is_some() {
        match state
            .session_admin
            .set_session_project(&session_id, req.project_id.as_deref())
            .await
        {
            Ok(Some(value)) => project_assigned = Some(value),
            Ok(None) => return EndpointError::not_found().into_response(),
            Err(err) => return session_admin_error_response(err),
        }
    }
    // 独立会话不切走当前焦点：不写 current_session_id。
    state.app_events().publish(
        "session-created",
        serde_json::json!({
            "sessionId": session_id,
            "agentId": agent_id,
            "detached": true,
        }),
    );
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "ok": true,
            "sessionId": session_id,
            "agentId": agent_id,
            "forkedFromSessionId": lineage.as_ref().map(|(parent, _)| parent.clone()),
            "lineageDepth": lineage.as_ref().map(|(_, depth)| *depth).unwrap_or(0),
            "projectId": project_assigned.flatten(),
        })),
    )
        .into_response()
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewDetachedBody {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    forked_from_session_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
}

/// `GET /lingxi/v1/sessions/{id}/branch` — 当前分支历史投影（含 reset 标记）。
async fn branch_history_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    // 归属闸（现役 can_access）：跨主体 → 403，不存在 → 404。
    match state.sessions.get_for(&principal, &session_id).await {
        Ok(sessions::SessionAccess::Ok(_)) => {}
        Ok(sessions::SessionAccess::NotFound) => {
            return EndpointError::not_found().into_response();
        }
        Ok(sessions::SessionAccess::Forbidden) => {
            return EndpointError::forbidden("cross_principal_access").into_response();
        }
        Err(err) => return EndpointError::storage(&err).into_response(),
    }
    match state.session_tree.branch_history(&session_id).await {
        Ok(msgs) => (
            StatusCode::OK,
            Json(serde_json::json!({ "messages": msgs })),
        )
            .into_response(),
        Err(err) => session_tree_error_response(err),
    }
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

/// `GET /lingxi/v1/sessions/{id}/context-observation` — the R06-T01
/// observation read face. The served view is built from the SAME frozen
/// artifact whose render the run supervisor sends as every turn's system
/// text (R06-A01: one build result, no second prompt reconstruction).
/// Digest-only segments carry per-segment sha256 over the SENT bytes and
/// never their content. A session whose run never reached a model call has
/// no compiled artifact — 404, observation never builds one on demand.
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextObservationQuery {
    run_id: Option<String>,
    model_call_id: Option<String>,
    snapshot_id: Option<String>,
    #[serde(default)]
    include_cache_stats: bool,
}

async fn session_context_observation(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ContextObservationQuery>,
) -> Response {
    match state.sessions.get_for(&principal, &session_id).await {
        Ok(sessions::SessionAccess::Ok(_facts)) => {
            match state
                .storage()
                .model_context_observation(
                    &session_id,
                    query.run_id.as_deref(),
                    query.model_call_id.as_deref(),
                    query.snapshot_id.as_deref(),
                )
                .await
            {
                Ok(Some(view)) if query.include_cache_stats => {
                    let stats = state
                        .context_compiler()
                        .map(|compiler| compiler.cache_stats());
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({"snapshot":view,"cacheStats":stats})),
                    )
                        .into_response()
                }
                Ok(Some(view)) => (StatusCode::OK, Json(view)).into_response(),
                Ok(None) => EndpointError::not_found().into_response(),
                Err(err) => EndpointError::storage(&err).into_response(),
            }
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
    session_events_page_inner(principal, state, session_id, query, false).await
}
async fn session_mood_events_page(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    session_events_page_inner(principal, state, session_id, query, true).await
}
async fn session_events_page_inner(
    principal: Principal,
    state: ServiceState,
    session_id: String,
    query: Option<String>,
    mood: bool,
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
    let result = if mood {
        state
            .events
            .mood_events_page(&principal, &session_id, cursor, limit)
            .await
    } else {
        state
            .events
            .events_page(&principal, &session_id, cursor, limit)
            .await
    };
    match result {
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
        knowledge: request.knowledge.as_ref(),
        input: &request.input,
        request_id: validated_request_id.as_deref(),
    };
    if let Err(resp) = gate_session_owned(&state, &principal, &session_id).await {
        return resp;
    }
    // R06-T03 生命周期闸（现役 assertManifestLifecycle "active"）：归档会话
    // 拒绝执行。归档语义 = 不再接受新轮次；不闸归属（归属在 admit 链上
    // 已有 404/403 区分），此处只查 lifecycle，存在性由 admit 链负责。
    match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), &session_id)
        .await
    {
        Ok(Some(row)) if row.lifecycle != "active" => {
            return EndpointError::new(
                StatusCode::CONFLICT,
                ErrorCode::Conflict,
                format!(
                    "session lifecycle is {}, execute requires active",
                    row.lifecycle
                ),
            )
            .with_reason("session_not_active")
            .with_cause("session.not_active")
            .into_response();
        }
        Ok(_) => {}
        Err(err) => return EndpointError::storage(&err).into_response(),
    }
    match state
        .sessions
        .execute_submission_owned_with_knowledge(
            storage,
            Arc::clone(state.events()),
            Arc::clone(state.runs()),
            principal,
            session_id,
            submission.input.to_string(),
            submission.request_id.map(str::to_string),
            request.knowledge,
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
        Err(sessions::SessionExecuteError::AdmissionInFlight { request_id }) => {
            EndpointError::admission_in_flight(&request_id).into_response()
        }
        Err(sessions::SessionExecuteError::RequestIdBoundToEarlierRun { request_id, run_id }) => {
            EndpointError::request_id_bound_to_earlier_run(&request_id, &run_id).into_response()
        }
        Err(sessions::SessionExecuteError::RequestIdBoundAmbiguous {
            request_id,
            run_ids,
        }) => EndpointError::request_id_bound_ambiguously(&request_id, &run_ids).into_response(),
        Err(sessions::SessionExecuteError::IdempotencyRegistryFull { .. }) => {
            EndpointError::idempotency_registry_full().into_response()
        }
        Err(sessions::SessionExecuteError::InvalidRequestId { detail }) => {
            EndpointError::invalid_message(format!("requestId invalid: {detail}")).into_response()
        }
        Err(sessions::SessionExecuteError::InputTooLarge { bytes, limit_bytes }) => {
            // R03 repair G05/F06: the official input budget refused the
            // submission at admission, before any side effect. 413 (the
            // same family as the transport body limit) with both byte
            // counts named — never a silent truncation. Unreachable via
            // this route today (the 1 MiB framework body limit rejects
            // larger bodies first); mapped so the surface stays closed
            // and loud for any future wiring.
            EndpointError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                ErrorCode::InvalidMessage,
                format!(
                    "submission input is {bytes} bytes, over the supported budget of \
                     {limit_bytes} bytes; resend within the budget — input is never \
                     silently truncated"
                ),
            )
            .with_reason("input_too_large")
            .with_cause("session.input_too_large")
            .into_response()
        }
        Err(sessions::SessionExecuteError::ShuttingDown) => {
            // R03-A14: the exit window refuses fresh submissions.
            EndpointError::shutting_down().into_response()
        }
        Err(sessions::SessionExecuteError::BackgroundRegistryFull { .. }) => {
            // Not reachable from the execute route (background submission
            // is a service-layer surface in R03); mapped anyway so the
            // surface stays closed and loud if a future route wires it.
            EndpointError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::BudgetExceeded,
                "background-drive registry is full",
            )
            .with_reason("background_registry_full")
            .with_cause("session.background_registry_full")
            .into_response()
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
            session_operation_storage_response(&err)
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
    let request_id = DiagnosticRequestId(&request_id);
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

/// Dispatch for the D13 terminal frame family. `None` means "not a terminal
/// frame" (the caller falls through to the regular request match); `Some`
/// is the exact response frame to send on this connection.
///
/// Identity/authorization failures still answer with an explicit `error`
/// frame (the incumbent generic WS error shape) instead of a content-bearing
/// frame — a mismatched or foreign session never reaches terminal records.
async fn handle_terminal_frame(
    state: &ServiceState,
    principal: &Principal,
    request: &ws::WsClientRequest,
) -> Option<serde_json::Value> {
    let (session_id, session_path) = match request {
        ws::WsClientRequest::TerminalSnapshotRequest {
            session_id,
            session_path,
        }
        | ws::WsClientRequest::TerminalTailRequest {
            session_id,
            session_path,
            ..
        }
        | ws::WsClientRequest::TerminalCloseRequest {
            session_id,
            session_path,
            ..
        } => (session_id.as_deref(), session_path.as_deref()),
        _ => return None,
    };
    let target = match terminals::resolve_target(state, principal, session_id, session_path).await {
        Ok(target) => target,
        Err(frame) => return Some(frame),
    };
    let requester = terminals::TerminalRequester::from_principal(principal);
    let hub = terminals::TerminalHub::new(state.native_processes.clone());
    Some(match request {
        ws::WsClientRequest::TerminalSnapshotRequest { .. } => hub.snapshot(&requester, &target),
        ws::WsClientRequest::TerminalTailRequest {
            terminal_id,
            since_seq,
            ..
        } => hub.tail(
            &requester,
            &target,
            terminal_id.as_deref(),
            since_seq.as_ref(),
        ),
        ws::WsClientRequest::TerminalCloseRequest {
            terminal_id,
            request_id,
            ..
        } => {
            hub.close(
                &requester,
                &target,
                terminal_id.as_deref(),
                request_id.as_deref(),
            )
            .await
        }
        _ => unreachable!("only terminal frames reach this match"),
    })
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
                        data_epoch: ContractVersions::R08_CUTOVER.data_epoch,
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
    let (slash_tx, mut slash_rx) = tokio::sync::mpsc::channel::<slash::SlashResult>(8);
    let mut slash_tasks = tokio::task::JoinSet::new();
    let (chat_tx, mut chat_rx)=tokio::sync::mpsc::channel::<serde_json::Value>(64);
    let mut chat_jobs=tokio::task::JoinSet::new();
    let mut chat_events=state.chat_wire.subscribe();
    loop {
        let sub_mailbox = subscription.as_ref().map(|s| s.mailbox().clone());
        let sub_frame = async {
            match sub_mailbox {
                Some(mailbox) => mailbox.recv().await,
                None => std::future::pending::<Option<events::SubscriptionFrame>>().await,
            }
        };
        tokio::select! {
            Some(result)=chat_rx.recv()=> {
                if !ensure_ws_principal_current(&mut writer,&state,&principal,secure_transport).await{return;}
                if ws::write_ws_text(&mut writer,&canon::canonical_bytes(&result)).await.is_err(){return;}
            }
            frame=chat_events.recv()=>{
                if !ensure_ws_principal_current(&mut writer,&state,&principal,secure_transport).await{return;}
                match frame {
                    Ok(frame)=>{
                        if let Some(id)=frame["sessionId"].as_str(){
                            if get_session_owned(&state,&principal,id).await.is_ok() && ws::write_ws_text(&mut writer,&canon::canonical_bytes(&frame)).await.is_err(){return;}
                        }
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>{
                        let error=ProtocolError::new(ErrorCode::Conflict,"chat event gap: reconnect and resume current stream",true);
                        let _=ws::write_ws_text(&mut writer,&canon::canonical_bytes(&error)).await;
                        let _=ws::write_ws_close(&mut writer,1013,"chat_resume_required").await;return;
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Closed)=>return,
                }
            }
            Some(result)=chat_jobs.join_next(), if !chat_jobs.is_empty()=>{
                if let Err(error)=result {tracing::error!(%error,"chat request task failed");let error=ProtocolError::new(ErrorCode::Internal,"chat request task failed",false);let _=ws::write_ws_text(&mut writer,&canon::canonical_bytes(&error)).await;}
            }
            Some(result) = slash_rx.recv() => {
                if !ensure_ws_principal_current(&mut writer, &state, &principal, secure_transport).await { return; }
                if ws::write_ws_text(&mut writer, &canon::canonical_bytes(&result)).await.is_err() { return; }
            }
            Some(result) = slash_tasks.join_next(), if !slash_tasks.is_empty() => {
                if let Err(error) = result {
                    tracing::error!(%error, "slash command task failed");
                    let error = ProtocolError::new(ErrorCode::Internal, "slash command task failed", false);
                    let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                }
            }
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
                        let mood_request = matches!(&request, ws::WsClientRequest::SubscribeMood { .. });
                        let chat_interject=matches!(&request,ws::WsClientRequest::Interject(_));
                        // ── D13 终端帧族（snapshot / tail / close）──
                        // 真实实现：session 身份经 session_locator::resolve + 会话归属
                        // 授权，再落到进程监督器的持久终端记录（见 terminals.rs）；
                        // 不再回显式 capability error。
                        if let Some(frame) = handle_terminal_frame(&state, &principal, &request).await {
                            if ws::write_ws_text(&mut writer, &serde_json::to_vec(&frame).unwrap_or_default()).await.is_err() {
                                return;
                            }
                            continue;
                        }
                        match request {
                            ws::WsClientRequest::SubagentStopRequest {session_id, session_path, task_id, request_id} => {
                                let result = background_tasks::ws_stop(&state, &principal, background_tasks::StopBody {session_id,session_path}, &task_id, request_id).await;
                                if ws::write_ws_text(&mut writer, &canon::canonical_json_bytes(&result)).await.is_err() {return;}
                            }
                            ws::WsClientRequest::Prompt(req)|ws::WsClientRequest::Interject(req)=>{
                                if chat_jobs.len()>=32 {let error=ProtocolError::new(ErrorCode::Conflict,"chat request capacity exceeded",true);let _=ws::write_ws_text(&mut writer,&canon::canonical_bytes(&error)).await;continue;}
                                let (state,principal,tx)=(state.clone(),principal.clone(),chat_tx.clone());
                                chat_jobs.spawn(async move {chat_wire::prompt(state,principal,req,chat_interject,tx).await;});
                            }
                            ws::WsClientRequest::Abort(req)=>{chat_wire::abort(&state,&principal,req,&chat_tx).await;}
                            ws::WsClientRequest::ResumeStream(req)=>{chat_wire::resume(&state,&principal,req,&chat_tx).await;}
                            ws::WsClientRequest::Steer(req)=>{
                                if chat_jobs.len()>=32 {let error=ProtocolError::new(ErrorCode::Conflict,"chat request capacity exceeded",true);let _=ws::write_ws_text(&mut writer,&canon::canonical_bytes(&error)).await;continue;}
                                let (state,principal,tx)=(state.clone(),principal.clone(),chat_tx.clone());chat_jobs.spawn(async move {chat_wire::steer(state,principal,req,tx).await;});
                            }
                            ws::WsClientRequest::Compact(req)=>{
                                if chat_jobs.len()>=32 {let error=ProtocolError::new(ErrorCode::Conflict,"chat request capacity exceeded",true);let _=ws::write_ws_text(&mut writer,&canon::canonical_bytes(&error)).await;continue;}
                                let (state,principal,tx)=(state.clone(),principal.clone(),chat_tx.clone());chat_jobs.spawn(async move {chat_wire::compact(&state,&principal,req,&tx).await;});
                            }
                            ws::WsClientRequest::Slash { session_id, text, agent_id } => {
                                if !slash_tasks.is_empty() {
                                    let error = ProtocolError::new(ErrorCode::Conflict, "slash command already active on this connection", true);
                                    let _ = ws::write_ws_text(&mut writer, &canon::canonical_bytes(&error)).await;
                                    continue;
                                }
                                let (state, principal, tx) = (state.clone(), principal.clone(), slash_tx.clone());
                                slash_tasks.spawn(async move {
                                    slash::dispatch(&state, &principal, &session_id, slash::SlashRequest { text, agent_id }, tx).await;
                                });
                            }
                            // D13 终端帧：上方 handle_terminal_frame 已处理并 continue，
                            // 此臂仅为 match 穷尽性（不可达）。
                            ws::WsClientRequest::TerminalSnapshotRequest { .. }
                            | ws::WsClientRequest::TerminalTailRequest { .. }
                            | ws::WsClientRequest::TerminalCloseRequest { .. } => {}
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
                            ws::WsClientRequest::SubscribeEvents { stream_id, cursor }
                            | ws::WsClientRequest::SubscribeMood { session_id: stream_id, cursor } => {
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
                                let result = if mood_request { state.events.subscribe_mood(&principal, &stream_id, cursor).await }
                                    else { state.events.subscribe(&principal, &stream_id, cursor).await };
                                match result {
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
        .merge(conversation_routes::router())
        .merge(browser_routes::router())
        .merge(office_routes::router())
        .merge(media_routes::router())
        .merge(media_admin::router())
        .merge(media_generate::router())
        .route("/lingxi/v1/search/config", get(search_routes::settings))
        .route("/lingxi/v1/search/verify", post(search_routes::verify))
        .route(
            "/lingxi/v1/session-collab/apply",
            post(session_collab::apply_route),
        )
        .route(
            "/lingxi/v1/session-collab/reject",
            post(session_collab::reject_route),
        )
        .route(
            "/lingxi/v1/session-collab/drafts",
            get(session_collab::drafts_route),
        )
        .route("/lingxi/v1/bridge/status", get(bridge_routes::status_route))
        .route(
            "/lingxi/v1/bridge/sessions/{sessionKey}/messages",
            get(bridge_session_routes::messages_route),
        )
        .route(
            "/lingxi/v1/bridge/sessions/{sessionKey}/reset",
            post(bridge_session_routes::reset_route),
        )
        .route(
            "/lingxi/v1/bridge/send-media",
            post(bridge_session_routes::send_media_route),
        )
        .route(
            "/lingxi/v1/bridge/deliveries/{deliveryId}",
            get(bridge_session_routes::delivery_route),
        )
        .route("/lingxi/v1/bridge/test", post(bridge_routes::test_route))
        .route(
            "/lingxi/v1/bridge/wechat/qrcode",
            post(bridge_routes::qrcode_route),
        )
        .route(
            "/lingxi/v1/bridge/wechat/qrcode-status",
            post(bridge_routes::qrcode_status_route),
        )
        .route(
            "/lingxi/v1/bridge/config",
            post(bridge_routes::config_route),
        )
        .route("/lingxi/v1/bridge/owner", post(bridge_routes::owner_route))
        .route("/lingxi/v1/bridge/stop", post(bridge_routes::stop_route))
        .route(
            "/lingxi/v1/bridge/settings",
            post(bridge_routes::settings_route),
        )
        .route(
            "/lingxi/v1/bridge/messages",
            get(bridge_routes::messages_route),
        )
        .route(
            "/lingxi/v1/bridge/sessions",
            get(bridge_routes::sessions_route),
        )
        .route("/lingxi/v1/me", get(me))
        .route(
            "/lingxi/v1/input-drafts",
            get(input_drafts::get_route).put(input_drafts::put_route),
        )
        .route(
            "/lingxi/v1/desk/skills",
            get(skills::workspace_skills_route),
        )
        // R06 F 块：工作区技能目录删除（现役 desk 删除面，只允许已知工作区根）。
        .route(
            "/lingxi/v1/desk/delete-skill",
            post(skills::workspace_skill_delete_route),
        )
        .route(
            "/lingxi/v1/sessions",
            get(list_sessions).post(create_session_route),
        )
        // R06-T03 管理面静态路由（axum 静态段优先于 {session_id} 参数段）。
        .route("/lingxi/v1/sessions/rename", post(rename_session_route))
        // R06 焦点切换与独立会话创建（现役 /sessions/switch、/sessions/new-detached）
        .route("/lingxi/v1/sessions/new", post(session_focus::create_route))
        .route(
            "/lingxi/v1/sessions/new-detached-context",
            post(session_focus::create_detached_route),
        )
        .route(
            "/lingxi/v1/sessions/{id}/workspace-files",
            get(session_focus::workspace_files_route),
        )
        .route("/lingxi/v1/sessions/switch", post(switch_session_route))
        .route("/lingxi/v1/sessions/rotate", post(rotate_session_route))
        .route(
            "/lingxi/v1/sessions/continue-deleted-agent",
            post(continue_deleted_agent_route),
        )
        .route(
            "/lingxi/v1/sessions/new-detached",
            post(new_detached_session_route),
        )
        .route("/lingxi/v1/sessions/pin", post(pin_session_route))
        .route("/lingxi/v1/sessions/pin-order", post(pin_order_route))
        .route(
            "/lingxi/v1/desk/activities",
            get(automation_activity::list_route),
        )
        .route(
            "/lingxi/v1/desk/activities/{id}/session",
            get(automation_activity::session_route),
        )
        .route(
            "/lingxi/v1/desk/activities/{id}/promote",
            post(automation_activity::promote_route),
        )
        .route(
            "/lingxi/v1/desk/cron",
            get(automation::cron_get_route).post(automation::cron_post_route),
        )
        .route(
            "/lingxi/v1/desk/heartbeat",
            axum::routing::post(automation::heartbeat_route),
        )
        .route(
            "/lingxi/v1/desk/cron/history",
            get(automation::history_route),
        )
        .route(
            "/lingxi/v1/preferences/notifications",
            get(automation::notification_preferences_get_route)
                .put(automation::notification_preferences_put_route),
        )
        .route(
            "/lingxi/v1/preferences/setup-complete",
            post(userprefs::setup_complete_route),
        )
        .route(
            "/lingxi/v1/automation/status",
            get(automation::status_route),
        )
        .route(
            "/lingxi/v1/automation/notifications",
            get(automation::notifications_get_route).post(automation::notifications_post_route),
        )
        .route(
            "/lingxi/v1/sessions/workspace-rollback",
            get(workspace_rollback_get_route).put(workspace_rollback_put_route),
        )
        // R06 G 块：计划/待办面板（todo-state-v1 快照 + 三动作 + 读面）。
        .route(
            "/lingxi/v1/sessions/todos/complete",
            post(todos::complete_route),
        )
        .route(
            "/lingxi/v1/sessions/todos/cancel",
            post(todos::cancel_route),
        )
        .route(
            "/lingxi/v1/sessions/todos/dismiss",
            post(todos::dismiss_route),
        )
        .route(
            "/lingxi/v1/sessions/interactions",
            get(interactive_tools::state_route),
        )
        .route(
            "/lingxi/v1/sessions/interactions/answer",
            post(interactive_tools::answer_route),
        )
        .route("/lingxi/v1/sessions/todos", get(todos::panel_route))
        // R06 R2：会话列表投影（sessions.read + Studio 身份闸，现役 studio-list 面）。
        .route(
            "/lingxi/v1/sessions/studio-list",
            get(sessions::sessions_studio_list_route),
        )
        // R07-T07 β：工作台/Studio 挂载面（现役 studio-workspaces.ts +
        // mobile-workbench.ts 全动作端口；R06 R2 的 writeText 单面并入
        // workbench::router() 的 actions 处理器，响应形状保持兼容）。
        .route("/lingxi/v1/desk/logs", get(desk_logs::list_route))
        .route(
            "/lingxi/v1/desk/activities/summarize",
            post(automation_activity::summarize_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/agent-activities",
            get(workflow_routes::activities_route),
        )
        .merge(workbench::router())
        // R07-T07 β3：desk 工作台文件面（现役 desk.ts 的 path/files/
        // search-files/jian 六路由，批准目录门禁 + local-owner upload 闸）。
        .merge(desk_files::router())
        // R07-T07 β4：日记面（现役 diary.ts 两路由 + diary-writer.ts
        // 材料/模型链全语义；memory 槽 AuxiliaryExecutor，登记偏差见
        // T07_STATUS.md β4）。
        .merge(diary::router())
        // R07-T07 γ：git 环境信息卡面（现役 git-environment.ts 22 路由 +
        // git-command.ts 执行/解析全语义；批准目录门禁 + studio 挂载根 +
        // summarize 槽 AI 提交信息）。
        .merge(git_panel::router())
        // R06 C1 收尾：工作台处置与孤儿清扫（现役 workspace-disposal /
        // sweep-orphaned-workspaces，sessions.write 口径）。
        .route(
            "/lingxi/v1/sessions/workspace-disposal",
            post(session_admin::workspace_disposal_route),
        )
        .route(
            "/lingxi/v1/sessions/sweep-orphaned-workspaces",
            post(session_admin::sweep_orphaned_route),
        )
        .route(
            "/lingxi/v1/sessions/{id}/operations",
            get(session_operations_route),
        )
        .route("/lingxi/v1/sessions/archive", post(archive_session_route))
        .route("/lingxi/v1/sessions/restore", post(restore_session_route))
        .route("/lingxi/v1/sessions/archived", get(list_archived_route))
        .route(
            "/lingxi/v1/sessions/archived/delete",
            post(delete_archived_route),
        )
        .route("/lingxi/v1/sessions/cleanup", post(cleanup_archived_route))
        .route("/lingxi/v1/sessions/search", get(search_sessions_route))
        .route("/lingxi/v1/sessions/find", get(find_session_route))
        .route("/lingxi/v1/sessions/{session_id}", get(get_session))
        .route(
            "/lingxi/v1/sessions/{session_id}/execute",
            post(execute_session),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/events",
            get(session_events_page),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/context-observation",
            get(session_context_observation),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/fork",
            post(fork_session_route),
        )
        // R06 C1 收尾：会话工作台身份（workspaceMountId/cwd 落库，处置/清扫匹配面）。
        .route(
            "/lingxi/v1/sessions/{session_id}/workspace",
            put(session_admin::put_session_workspace_route),
        )
        // R06 C2：会话历史与内容读取（现役 find / latest-user-message/replay /
        // history-overview / content / summary / conversation-map / fresh-compact）。
        .route(
            "/lingxi/v1/sessions/{session_id}/find",
            get(history::history_content::session_find_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/latest-user-message/replay",
            post(history::history_content::latest_user_message_replay_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/history-overview",
            get(history::history_content::session_history_overview_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/content/{content_id}",
            get(history::history_content::session_content_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/summary",
            get(history::history_content::session_summary_route),
        )
        .route(
            "/lingxi/v1/conversation-map/layout",
            get(conversation_map::owner_layout_get_route)
                .put(conversation_map::owner_layout_put_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/conversation-map/turns",
            get(history::history_content::conversation_map_turns_route),
        )
        // R06 收口：会话地图画布布局（现役 conversation-map.ts 的 layout
        // 叶——GET 持久化/默认空布局，PUT positions 合并/整体替换与
        // collapsed 整体替换；归属闸在 handler 内逐请求复检）。
        .route(
            "/lingxi/v1/sessions/{session_id}/conversation-map/layout",
            get(conversation_map::layout_get_route).put(conversation_map::layout_put_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/slash",
            post(slash::slash_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/bridge",
            post(slash::register_bridge_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/bridge/attachment",
            post(slash::attach_route).delete(slash::detach_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/compact",
            post(history::history_content::compact_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/compact/cancel",
            post(history::history_content::cancel_compaction_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/fresh-compact",
            post(history::history_content::fresh_compact_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/turns/retry",
            post(retry_turn_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/turns/rollback-preview",
            post(turns_rollback_preview_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/rewind",
            post(rewind_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/rewind/preview",
            post(rewind_preview_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/approvals",
            get(sessiontools::approvals_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/approvals/{approval_id}",
            post(sessiontools::answer_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/deferred-rewind",
            get(sessiontools::deferred_route),
        )
        .route(
            "/lingxi/v1/task/{task_id}/abort",
            post(background_tasks::stop_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/prompt-snapshot",
            get(session_prompt::get_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/checkpoints",
            get(list_checkpoints_route).post(create_checkpoint_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/checkpoints/delete",
            post(delete_checkpoint_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/memory",
            get(get_memory_route).patch(patch_memory_route),
        )
        // R06 会话授权目录（现役 GET/PATCH /sessions/authorized-folders）
        .route(
            "/lingxi/v1/sessions/{session_id}/authorized-folders",
            get(get_authorized_folders_route).patch(patch_authorized_folders_route),
        )
        // R06 v12 会话项目归属（现役 setSessionProjectAssignment 最小端口）
        .route(
            "/lingxi/v1/sessions/{session_id}/project",
            get(get_session_project_route).put(put_session_project_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/branch",
            get(branch_history_route),
        )
        // R06-T04 统一历史读面：分页投影与全量导出（同一投影函数）。
        .route(
            "/lingxi/v1/sessions/{session_id}/history",
            get(history::session_history_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/mood-events",
            get(session_mood_events_page),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/mood-history",
            get(history::session_mood_history_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/export",
            get(history::session_export_route),
        )
        // R06-T05 会话文件面：sf_ 注册/列表/单件（alias 感知）/旧 sidecar
        // 导入/本地与内存附件。归属闸在 handler 内先于一切实体读取。
        .route(
            "/lingxi/v1/sessions/{session_id}/files",
            get(sessionfiles::list_session_files_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/files/import-legacy",
            post(sessionfiles::import_legacy_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/files/{file_id}",
            get(sessionfiles::get_session_file_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/attachments/local",
            post(sessionfiles::attach_local_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/attachments/blob",
            post(sessionfiles::attach_blob_route),
        )
        // R06-T05 资源面：res_ 信封读取、内容票据签发、内容读取
        // （ETag/Range/Content-Disposition 与现役同口径；ticket 在
        // handler 内作内容级校验——候选人中间件对一切请求鉴权，现役
        // server/index.ts:642 的 ticket 鉴权豁免差异见 R06-T05 台账）。
        .route(
            "/lingxi/v1/resources/{resource_id}",
            get(resources::get_resource_route),
        )
        .route(
            "/lingxi/v1/resources/{resource_id}/ticket",
            post(resources::issue_resource_ticket_route),
        )
        .route(
            "/lingxi/v1/resources/{resource_id}/content",
            get(resources::resource_content_route),
        )
        // R06-T05 Round 4 ResourceIO 面（server/routes/resource-io.ts 的
        // 15 叶面）：stat/read/list/search/write/write-expected-version/
        // rename/move/trash/subscribe/watch + subscriptions/{id} 与
        // watch/{id} 释放 + watch-diagnostics + events。
        .route("/lingxi/v1/resource-io/stat", post(resourceio::stat_route))
        .route("/lingxi/v1/resource-io/read", post(resourceio::read_route))
        .route("/lingxi/v1/resource-io/list", post(resourceio::list_route))
        .route(
            "/lingxi/v1/resource-io/search",
            post(resourceio::search_route),
        )
        .route(
            "/lingxi/v1/resource-io/write",
            post(resourceio::write_route),
        )
        .route(
            "/lingxi/v1/resource-io/write-expected-version",
            post(resourceio::write_expected_version_route),
        )
        .route(
            "/lingxi/v1/resource-io/rename",
            post(resourceio::rename_route),
        )
        .route("/lingxi/v1/resource-io/move", post(resourceio::move_route))
        .route(
            "/lingxi/v1/resource-io/trash",
            post(resourceio::trash_route),
        )
        .route(
            "/lingxi/v1/resource-io/subscribe",
            post(resourceio::subscribe_route),
        )
        .route(
            "/lingxi/v1/resource-io/subscriptions/{subscription_id}",
            delete(resourceio::unsubscribe_route),
        )
        .route(
            "/lingxi/v1/resource-io/watch",
            post(resourceio::watch_route),
        )
        .route(
            "/lingxi/v1/resource-io/watch/{watch_id}",
            delete(resourceio::unwatch_route),
        )
        .route(
            "/lingxi/v1/resource-io/watch-diagnostics",
            get(resourceio::watch_diagnostics_route),
        )
        .route(
            "/lingxi/v1/resource-io/events",
            get(resourceio::events_route),
        )
        // R06-T05 Round 5 HTML 预览面（server/routes/html-preview.ts，
        // D5 前缀）：POST 建立（chat scope），GET/HEAD 读取与素材为
        // Public + token 即凭证（无效/过期一律 404，见 auth.rs 分类）。
        .route(
            "/lingxi/v1/preview/html",
            post(preview::create_preview_route),
        )
        .route(
            "/lingxi/v1/preview/html/{id}",
            get(preview::serve_preview_route),
        )
        .route(
            "/lingxi/v1/preview/html/{id}/assets/{token}/{*asset_path}",
            get(preview::serve_preview_asset_route),
        )
        // R06-T05 Round 5 桥媒体面（server/routes/bridge.ts:676-701，
        // D5 前缀）：Public + token 即凭证，50MB → 413。
        .route(
            "/lingxi/v1/bridge/media/{token}",
            get(bridgemedia::bridge_media_route),
        )
        // R06-T05 Round 5 文件历史面（server/routes/file-history.ts，
        // D5 前缀）：四叶面 LocalOnly（auth.rs 分类），agentId 必填。
        .route(
            "/lingxi/v1/file-history/files",
            get(filehistory::list_files_route),
        )
        .route(
            "/lingxi/v1/file-history/versions",
            get(filehistory::list_versions_route),
        )
        .route(
            "/lingxi/v1/file-history/snapshot",
            get(filehistory::snapshot_route),
        )
        .route(
            "/lingxi/v1/file-history/restore",
            post(filehistory::restore_route),
        )
        // R06 收口：独立改前存档 restore（现役 /api/checkpoints/:id/restore，
        // lib/checkpoint-store.ts 的落盘契约；区别于会话 rewind——不移动
        // 分支头，只把存档字节写回存档记录的绝对路径并返回写回位置）。
        // LocalOnly（auth.rs 分类：现役 /api/checkpoints/* 落 STUDIO_OWNER
        // 兜底，Rust 侧按既有口径映射最严格档）。
        .route("/lingxi/v1/checkpoints", get(checkpoint_store::list_route))
        .route(
            "/lingxi/v1/checkpoints/user-edit",
            post(checkpoint_store::user_edit_route),
        )
        .route(
            "/lingxi/v1/checkpoints/{checkpoint_id}/restore",
            post(checkpoint_store::restore_route),
        )
        // R08-T01：现役 DELETE /api/checkpoints/:id（removeCheckpoint）的
        // 端口；LocalOnly 沿同族既有分类，缺失幂等、IO 失败响亮。
        .route(
            "/lingxi/v1/checkpoints/{checkpoint_id}",
            delete(checkpoint_store::delete_route),
        )
        // R06-T05 Round 5 fs 直读面（server/routes/fs.ts :89-115，D5
        // 前缀）：LocalOnly，授权根 = data_home + workspace。
        .route("/lingxi/v1/fs/read", get(fsread::fs_read_route))
        .route("/lingxi/v1/fs/docx-html", get(fsread::fs_docx_html_route))
        .route("/lingxi/v1/fs/xlsx-html", get(fsread::fs_xlsx_html_route))
        .route(
            "/lingxi/v1/fs/read-base64",
            get(fsread::fs_read_base64_route),
        )
        // ── R06-T07 现役知识库面（server/routes/knowledge.ts 镜像；LocalOnly，
        // 单 studio 归属闸在 auth 分类 + handler studio 复核）──
        .merge(knowledge::knowledge_routes())
        // ── R06-T06 记忆面（显式 agentId 是唯一归属边界）──
        .route(
            "/lingxi/v1/memories",
            get(memory::memories_list_route).delete(memory::memories_delete_route),
        )
        .route(
            "/lingxi/v1/memories/health",
            get(memory::memories_health_route),
        )
        .route(
            "/lingxi/v1/memories/compiled",
            get(memory::memories_compiled_get_route).delete(memory::memories_compiled_delete_route),
        )
        .route(
            "/lingxi/v1/memories/compiled/facts",
            put(memory::memories_compiled_facts_route),
        )
        .route(
            "/lingxi/v1/memories/compiled/today",
            put(memory::memories_compiled_today_route),
        )
        .route(
            "/lingxi/v1/memories/compiled/longterm",
            put(memory::memories_compiled_longterm_route),
        )
        .route(
            "/lingxi/v1/memories/compiled/week/days",
            get(memory::memories_week_days_route),
        )
        .route(
            "/lingxi/v1/memories/compiled/week/days/{date}",
            put(memory::memories_week_day_put_route),
        )
        .route(
            "/lingxi/v1/memories/export",
            get(memory::memories_export_route),
        )
        .route(
            "/lingxi/v1/memories/import",
            post(memory::memories_import_route),
        )
        .route(
            "/lingxi/v1/memories/dream/status",
            get(memory::dream_status_route),
        )
        .route(
            "/lingxi/v1/memories/dream/runs",
            post(memory::dream_run_route).delete(memory::dream_cancel_route),
        )
        .route(
            "/lingxi/v1/memories/dream/revisions",
            get(memory::dream_revisions_route),
        )
        .route(
            "/lingxi/v1/memories/dream/revisions/{revision_id}",
            get(memory::dream_revision_detail_route),
        )
        .route(
            "/lingxi/v1/memories/dream/revisions/{revision_id}/restore",
            post(memory::dream_revision_restore_route),
        )
        // ── R06 助手生命周期集合面（列表/创建/切换/删除/主助手/排序）──
        .route(
            "/lingxi/v1/agent-deletions",
            get(agents_admin::agent_deletions::deletion_receipts_route),
        )
        .route(
            "/lingxi/v1/agent-deletions/{deletion_id}/cleanup",
            post(agents_admin::agent_deletions::deletion_cleanup_route),
        )
        .route(
            "/lingxi/v1/agents",
            get(agents_admin::agents_list_route).post(agents_admin::agents_create_route),
        )
        .route(
            "/lingxi/v1/agents/switch",
            post(agents_admin::agents_switch_route),
        )
        .route(
            "/lingxi/v1/agents/primary",
            put(agents_admin::agents_primary_route),
        )
        .route(
            "/lingxi/v1/agents/order",
            put(agents_admin::agents_order_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}",
            delete(agents_admin::agents_delete_route),
        )
        // R06 app 级事件广播检索面（LocalOnly：助手名册不向远端设备披露）
        .route("/lingxi/v1/app-events", get(appevents::events_route))
        .route(
            "/lingxi/v1/preferences/goal",
            get(userprefs::goal_get_route).put(userprefs::goal_put_route),
        )
        .route(
            "/lingxi/v1/preferences/keybindings",
            get(userprefs::keybindings_get_route).put(userprefs::keybindings_put_route),
        )
        .route(
            "/lingxi/v1/preferences/sidebar-ui",
            get(userprefs::sidebar_ui_get_route).put(userprefs::sidebar_ui_put_route),
        )
        // R08-T01：现役 /api/preferences/appearance 端口（规范化合并写 +
        // 变化事件发布；读 settings.read / 写 settings.write 沿现役分类）。
        .route(
            "/lingxi/v1/preferences/appearance",
            get(userprefs::appearance_get_route).put(userprefs::appearance_put_route),
        )
        // R08-T01：现役 /api/system/env-deps 端口（环境依赖探测报告 +
        // 强制刷新；现役无显式安全条目 → 走 /api/* STUDIO_OWNER 兜底，候选
        // 取最严 LocalOnly；refresh 路径现役仅注册 POST，GET 保持 404）。
        .route(
            "/lingxi/v1/system/env-deps",
            get(env_deps::env_deps_get_route),
        )
        .route(
            "/lingxi/v1/system/env-deps/refresh",
            post(env_deps::env_deps_refresh_route),
        )
        // R08-T01：现役 /api/preferences/workspace-ui-state 端口（workspace+surface
        // 键控读写、全量规范化与缺失目录 GC 裁剪；读 files.read / 写 files.write
        // 沿现役 route-security 显式分类）。
        .route(
            "/lingxi/v1/preferences/workspace-ui-state",
            get(workspace_ui_state::workspace_ui_state_get_route)
                .put(workspace_ui_state::workspace_ui_state_put_route),
        )
        // R06 用户偏好对（现役 /preferences/autolearn、/preferences/quick-chat）
        .route(
            "/lingxi/v1/preferences/autolearn",
            get(userprefs::autolearn_get_route).put(userprefs::autolearn_put_route),
        )
        .route(
            "/lingxi/v1/preferences/session-permission-default",
            get(userprefs::session_permission_default_get_route)
                .put(userprefs::session_permission_default_put_route),
        )
        .route(
            "/lingxi/v1/preferences/quick-chat",
            get(userprefs::quick_chat_get_route).put(userprefs::quick_chat_put_route),
        )
        // R06 会话项目/文件夹目录（现役 /session-projects* 全量面）
        .route("/lingxi/v1/session-projects", get(projects::catalog_route))
        .route(
            "/lingxi/v1/session-projects/session-assignment",
            post(projects::set_session_assignment_route),
        )
        .route(
            "/lingxi/v1/session-projects/projects",
            post(projects::create_project_route),
        )
        .route(
            "/lingxi/v1/session-projects/projects/reorder",
            post(projects::reorder_projects_route),
        )
        .route(
            "/lingxi/v1/session-projects/projects/{project_id}",
            axum::routing::patch(projects::update_project_route)
                .delete(projects::delete_project_route),
        )
        .route(
            "/lingxi/v1/session-projects/folders",
            post(projects::create_folder_route),
        )
        .route(
            "/lingxi/v1/session-projects/folders/reorder",
            post(projects::reorder_folders_route),
        )
        .route(
            "/lingxi/v1/session-projects/folders/{folder_id}",
            axum::routing::patch(projects::update_folder_route)
                .delete(projects::delete_folder_route),
        )
        // 角色卡文件与上传面复用同一原生服务及权限边界。
        .route(
            "/lingxi/v1/character-cards/plan",
            post(character_cards::plan_route)
                .layer(axum::extract::DefaultBodyLimit::max(80 * 1024 * 1024)),
        )
        .route(
            "/lingxi/v1/character-cards/import",
            post(character_cards::import_route),
        )
        .route(
            "/lingxi/v1/character-cards/export/preview",
            post(character_cards::export_preview_route),
        )
        .route(
            "/lingxi/v1/character-cards/export/plan",
            post(character_cards::export_preview_route),
        )
        .route(
            "/lingxi/v1/character-cards/export",
            post(character_cards::export_route),
        )
        .route(
            "/lingxi/v1/character-cards/plans/{token}/assets/{key}",
            get(character_cards::plan_asset_route),
        )
        .route(
            "/lingxi/v1/character-cards/export/{agent_id}/assets/{key}",
            get(character_cards::export_asset_route),
        )
        // 当前桌面助手设置聚合读面，错误沿原处理器返回，其他设置页不借此冒充可用。
        .route(
            "/lingxi/v1/settings/snapshot",
            get(settings_ui::snapshot_route),
        )
        .route(
            "/lingxi/v1/settings/config",
            get(settings_ui::config_route).put(settings_configuration::config_put_route),
        )
        .route(
            "/lingxi/v1/preferences/computer-use",
            get(computer_use_settings::get_route).put(computer_use_settings::put_route),
        )
        .route(
            "/lingxi/v1/preferences/computer-use/approvals",
            post(computer_use_settings::approve_route).delete(computer_use_settings::revoke_route),
        )
        .route(
            "/lingxi/v1/preferences/computer-use/request-permissions",
            post(computer_use_settings::request_route),
        )
        .route(
            "/lingxi/v1/settings/avatar-health",
            get(settings_ui::avatars_health_route),
        )
        .route("/lingxi/v1/models", get(chat_wire::models_route))
        .route("/lingxi/v1/models/set", post(chat_wire::model_set_route))
        // R07-T09：CLI/mobile 共享的只读聊天上下文面（chat scope）。
        .route(
            "/lingxi/v1/chat/models",
            get(client_context::chat_models_route),
        )
        .route(
            "/lingxi/v1/mobile/bootstrap",
            get(client_context::mobile_bootstrap_route),
        )
        .route(
            "/lingxi/v1/models/switch",
            post(chat_wire::model_switch_route),
        )
        .route(
            "/lingxi/v1/preferences/models",
            get(settings_ui::preferences_models_route)
                .put(settings_configuration::models_put_route),
        )
        .route(
            "/lingxi/v1/providers/summary",
            get(provider_routes::summary_route),
        )
        .route(
            "/lingxi/v1/user-profile",
            get(settings_ui::user_profile_route)
                .put(settings_configuration::user_profile_put_route),
        )
        // ── R06-T06 tenets/pinned/experience/config 人格面 ──
        .route(
            "/lingxi/v1/agents/{agent_id}/tenets",
            get(memory::tenets_list_route).post(memory::tenets_post_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/tenets/{tenet_id}/decide",
            post(memory::tenets_decide_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/tenets/{tenet_id}",
            delete(memory::tenets_delete_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/pinned",
            get(memory::pinned_get_route).put(memory::pinned_put_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/experience",
            get(memory::experience_get_route).put(memory::experience_put_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/experience/search",
            get(memory::experience_search_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/experience/feedback",
            post(memory::experience_feedback_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/config",
            get(agents_admin::agent_config_readback::agent_config_get_route)
                .patch(memory::agent_config_patch_route)
                .put(agents_admin::agent_config_readback::agent_config_put_route),
        )
        // R06 A 块读回叶：运行时状态键 / 实验读写 / 观察记录读面。
        .route(
            "/lingxi/v1/current-status",
            get(agents_admin::agent_config_readback::current_status_route),
        )
        .route(
            "/lingxi/v1/experiments",
            get(agents_admin::agent_config_readback::experiments_list_route),
        )
        .route(
            "/lingxi/v1/experiments/{experiment_id}",
            patch(agents_admin::agent_config_readback::experiment_patch_route),
        )
        .route(
            "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation",
            get(agents_admin::agent_config_readback::cache_snapshot_observation_get_route).delete(
                agents_admin::agent_config_readback::cache_snapshot_observation_delete_route,
            ),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/config-flags",
            get(memory::agent_config_flags_route),
        )
        // ── R06-T06 人格文件面（ishiki/public-ishiki 为现役兼容别名）──
        .route(
            "/lingxi/v1/agents/{agent_id}/agents-md",
            get(persona::agents_md_get_route).put(persona::agents_md_put_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/ishiki",
            get(persona::agents_md_get_route).put(persona::agents_md_put_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/public-agents-md",
            get(persona::public_agents_md_get_route).put(persona::public_agents_md_put_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/public-ishiki",
            get(persona::public_agents_md_get_route).put(persona::public_agents_md_put_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/identity",
            get(persona::identity_get_route).put(persona::identity_put_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/avatar",
            get(persona::avatar_get_route)
                .post(persona::avatar_post_route)
                .delete(persona::avatar_delete_route)
                .layer(axum::extract::DefaultBodyLimit::max(
                    persona::MAX_AVATAR_BYTES,
                )),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/avatar-path",
            get(persona::avatar_path_route),
        )
        .route(
            "/lingxi/v1/avatar/{role}",
            get(persona::role_avatar_get_route)
                .post(persona::role_avatar_post_route)
                .delete(persona::role_avatar_delete_route)
                .layer(axum::extract::DefaultBodyLimit::max(
                    persona::MAX_AVATAR_BYTES,
                )),
        )
        // ── R06-T06 skills 面 ──
        .route("/lingxi/v1/skills", get(skills::skills_list_route))
        .route(
            "/lingxi/v1/skills/translate",
            post(skills::completion::translate_route),
        )
        .route(
            "/lingxi/v1/desk/install-skill",
            post(skills::completion::workspace_install_route),
        )
        .route(
            "/lingxi/v1/skills/install",
            post(skills::skills_install_route),
        )
        .route(
            "/lingxi/v1/skills/reload",
            post(skills::skills_reload_route),
        )
        .route(
            "/lingxi/v1/skills/external-paths",
            get(skills::skills_external_paths_get_route)
                .put(skills::skills_external_paths_put_route),
        )
        .route(
            "/lingxi/v1/skills/bundles",
            get(skills::skills_bundles_list_route).post(skills::skills_bundles_create_route),
        )
        .route(
            "/lingxi/v1/skills/bundles/order",
            put(skills::skills_bundles_order_route),
        )
        .route(
            "/lingxi/v1/skills/bundles/{bundle_id}",
            put(skills::skills_bundle_update_route).delete(skills::skills_bundle_delete_route),
        )
        // R06 F 块：组合包导出（store-mode zip，现役 POST /skills/bundles/:id/export）。
        .route(
            "/lingxi/v1/skills/bundles/{bundle_id}/export",
            post(skills::skill_export::skills_bundle_export_route),
        )
        .route(
            "/lingxi/v1/skills/{name}",
            delete(skills::skills_delete_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/skills",
            put(skills::agent_skills_put_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/skills/{name}",
            patch(skills::agent_skill_patch_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/skill-bundles/{bundle_id}",
            patch(skills::agent_skill_bundle_patch_route),
        )
        .route(
            "/lingxi/v1/agents/{agent_id}/skills/cleanup-preview",
            get(skills::agent_skills_cleanup_preview_route),
        )
        // ── R06-T06 会话上下文笔记（现役 {sessionPath}.context-notes.md）──
        .route(
            "/lingxi/v1/sessions/{session_id}/context-notes",
            get(memory::context_notes_get_route).put(memory::context_notes_put_route),
        )
        .route("/lingxi/v1/ws-ticket", post(ws_ticket))
        .route(
            "/lingxi/v1/devices/credentials",
            post(issue_device_credential),
        )
        .route("/lingxi/v1/ws", get(ws_handler))
        .route("/lingxi/v1/browser-host", get(browser_host::route))
        .route(
            "/lingxi/v1/settings/config/workspaces/recent",
            post(settings_configuration::recent_add_route)
                .delete(settings_configuration::recent_remove_route),
        )
        .route(
            "/lingxi/v1/settings/config/workspaces/recent/all",
            axum::routing::delete(settings_configuration::recent_clear_route),
        )
        .route(
            "/lingxi/v1/settings/config/default-workspace",
            get(settings_configuration::default_workspace_route)
                .post(settings_configuration::ensure_default_workspace_route),
        )
        .route(
            "/lingxi/v1/settings/export",
            get(settings_configuration::export_route),
        )
        .route(
            "/lingxi/v1/settings/import",
            post(settings_configuration::import_route),
        )
        .route(
            "/lingxi/v1/cards/{card_id}/frame/{ticket}",
            get(cards::frame_route),
        )
        .route(
            "/lingxi/v1/cards/{card_id}",
            get(cards::get_route).put(cards::put_route),
        )
        .merge(provider_routes::router())
        .merge(mcp_session_permissions::router())
        .merge(mcp_settings::router())
        .merge(management::routes())
        .merge(static_web::routes())
        // R07-T10：观测查询/导出/旧库导入（LocalOnly，auth_guard 统一把关）。
        .merge(observability::routes())
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

// ── R08-T05 cutover self-check surface ───────────────────────────────────────

/// Restricted router served by `lingxi-service --self-check` (design §9.1).
/// The full startup chain still ran (layout → epoch gate → storage open →
/// instance lock), but the business surface is STRUCTURALLY absent: only
/// `/lingxi/v1/health` (public, same as the normal router) and
/// `/lingxi/v1/self-check/probe` (local-owner token, read-only counts) are
/// routed; everything else — reads included — hits the loud 503 fallback,
/// never a silent 404. The SAME middleware stack as [`build_router`] applies
/// (error envelope, admission, transport guard, token auth), so auth parity
/// with the normal path is preserved.
pub fn build_self_check_router(state: ServiceState) -> Router {
    Router::new()
        .route("/lingxi/v1/health", get(health))
        .route("/lingxi/v1/self-check/probe", get(self_check_probe))
        .fallback(self_check_fallback)
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

/// One read-only count through the storage worker's inspection seam
/// (`query_one_text` — never the write path). A missing row or a
/// non-numeric value is a loud probe failure, never a guessed zero.
async fn self_check_count(state: &ServiceState, sql: &str) -> Result<u64, String> {
    let raw = state
        .storage()
        .query_one_text(sql, Vec::new())
        .await
        .map_err(|error| format!("probe query failed: {sql}: {error}"))?;
    let raw = raw.ok_or_else(|| format!("probe query returned no row: {sql}"))?;
    raw.parse::<u64>()
        .map_err(|error| format!("probe query non-numeric: {sql}: {raw}: {error}"))
}

/// GET `/lingxi/v1/self-check/probe` — read-only adoption-verification
/// counts (design §9.1: session count, model_call_usage count, cron task
/// count, plus the schema user_version). `classify_route` leaves this path
/// at the default `LocalOnly` policy, so `auth_guard` already required the
/// loopback owner token; the explicit `is_local_owner` check mirrors the
/// cron surface's belt-and-braces pattern.
async fn self_check_probe(
    State(state): State<ServiceState>,
    axum::Extension(principal): axum::Extension<Principal>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    let probe = async {
        Ok::<_, String>(serde_json::json!({
            "schemaVersion": 1,
            "mode": "self-check",
            "counts": {
                "sessions": self_check_count(&state, "SELECT COUNT(*) FROM sessions").await?,
                "modelCallUsage": self_check_count(&state, "SELECT COUNT(*) FROM model_call_usage").await?,
                // desk/cron visible-jobs semantics: soft-deleted rows are
                // not tasks the coordinator expects to see.
                "cronJobs": self_check_count(&state, "SELECT COUNT(*) FROM automation_jobs_v1 WHERE deleted_at IS NULL").await?,
            },
            "userVersion": self_check_count(&state, "PRAGMA user_version").await?,
        }))
    };
    match probe.await {
        Ok(body) => Json(body).into_response(),
        Err(detail) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":"self_check_probe_failed","detail":detail})),
        )
            .into_response(),
    }
}

/// Loud closure for everything outside the self-check surface — a 503 with
/// an explicit code, so a misdirected write can never look like success or
/// a silent no-route 404 (设计 §9.1「响亮 405/503，不静默吞」).
async fn self_check_fallback() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": "self_check_surface_closed",
            "mode": "self-check",
            "detail": "cutover self-check serves only /lingxi/v1/health and /lingxi/v1/self-check/probe; the business surface stays closed until the cutover journal reaches committed",
        })),
    )
        .into_response()
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

/// 监听失败和正常退出共用停止信号，外部仍持有服务引用也不能继续唤醒任务。
fn begin_background_shutdown(state: &ServiceState) {
    state.loop_services().stop_idle_listener();
    if let Some(gateway) = state.tool_gateway() {
        gateway.close_mcp_session_permissions();
    }
    state.memory().dream().stop_all();
    state.automation().stop();
    state.bridge().stop();
    state.conversations().stop();
    state.browsers().stop();
    state.mcp_settings().begin_stop();
    state.resource_io().shutdown();
    state.loop_services().alarm.dispose();
    state.loop_services().controller.dispose();
    state.workflows().begin_stop();
    state.media().begin_stop();
    state.beautify().begin_stop();
    state.knowledge().shutdown();
}

/// 尚未开始监听就失败时，也先通知全部已恢复的后台组件，再核对实际退出。
async fn cleanup_before_listen_failure(
    state: &ServiceState,
    original: &std::io::Error,
) -> Result<(), ServiceError> {
    begin_background_shutdown(state);
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_millis(state.config().shutdown_timeout_ms);
    let mcp = tokio::time::timeout_at(deadline, state.mcp_settings().shutdown()).await;
    let knowledge =
        tokio::time::timeout_at(deadline, state.knowledge().wait_worker_stopped()).await;
    let mut failures = Vec::new();
    match mcp {
        Ok(Ok(())) => {}
        Ok(Err(error)) => failures.push(format!("MCP: {error}")),
        Err(_) => failures.push("MCP: cleanup timed out".into()),
    }
    match knowledge {
        Ok(Ok(())) => {}
        Ok(Err(error)) => failures.push(format!("knowledge: {error}")),
        Err(_) => failures.push("knowledge: cleanup timed out".into()),
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ServiceError::Serve {
            source: std::io::Error::other(format!(
                "listen failed: {original}; cleanup unconfirmed: {}",
                failures.join("; ")
            )),
        })
    }
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
    let knowledge_worker = Arc::clone(state.knowledge());
    let bind_addr = state.config().bind_addr;
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => listener,
        Err(source) => {
            cleanup_before_listen_failure(&state, &source).await?;
            return Err(ServiceError::Bind {
                addr: bind_addr,
                source,
            });
        }
    };
    let local = match listener.local_addr() {
        Ok(addr) => addr,
        Err(source) => {
            cleanup_before_listen_failure(&state, &source).await?;
            return Err(ServiceError::Bind {
                addr: bind_addr,
                source,
            });
        }
    };
    *state
        .bound_addr
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(local);
    let self_check = state.self_check();
    // R08-T05: in self-check mode none of the background writers ever start
    // — the dream ticker, the automation scheduler, bridge/conversation
    // recovery (inbox/outbox redispatch is a write path), and the browser
    // host stay down so the imported tree is byte-frozen while the probe
    // counts are read. Their stop paths below are safe on never-started
    // hosts (idempotent no-ops).
    if !self_check {
        state.memory().dream().ticker().start(state.clone());
        state.automation().start(state.clone());
        state.bridge().start(state.clone());
        // 桥启动恢复（inbox/outbox/指令重投）必须抢在就绪前完成；恢复失败
        // 响亮拒绝服务，不在半恢复状态接流量（就绪前的外部位锁故障不再
        // 在半途中杀死桥宿主）。
        state
            .bridge()
            .wait_recovery()
            .await
            .map_err(|error| ServiceError::Serve {
                source: std::io::Error::other(format!("bridge recovery: {error}")),
            })?;
        state.conversations().start(state.clone());
        // 对话宿主的启动恢复同理：就绪前拿到确定结果，失败响亮拒绝。
        state
            .conversations()
            .wait_recovery()
            .await
            .map_err(|error| ServiceError::Serve {
                source: std::io::Error::other(format!("conversation recovery: {error}")),
            })?;
        state.browsers().start();
    }
    on_ready(local);

    // R2-F04: the serving loop enforces CONNECTION-level admission at the
    // accept edge (count cap) and a header-read budget per connection
    // (time cap) — axum's `serve` exposes neither, so the loop lives in
    // `crate::serve` with the axum graceful-drain semantics preserved.
    // Both budgets come from the SAME knobs as the request-level gate
    // (`--http-max-in-flight` / `--http-request-budget-ms`).
    let conn_admission = Arc::clone(state.connection_admission());
    let header_budget = state.admission().request_budget();
    let dream_service = Arc::clone(state.memory());
    let media_service = Arc::clone(state.media());
    let beautify_service = Arc::clone(state.beautify());
    let browser_service = Arc::clone(state.browsers());
    let mcp_service = Arc::clone(state.mcp_settings());
    let conversation_host = Arc::clone(state.conversations());
    let bridge_host = Arc::clone(state.bridge());
    let automation_host = Arc::clone(state.automation());
    let dream_budget = drain_budget.unwrap_or(std::time::Duration::from_millis(
        state.config().shutdown_timeout_ms,
    ));
    let dream_stop_at = Arc::new(std::sync::Mutex::new(None::<std::time::Instant>));
    let dream_signal_at = Arc::clone(&dream_stop_at);
    let workflows = Arc::clone(state.workflows());
    let native_processes = state.native_processes.clone();
    let state_on_signal = state.clone();
    // R08-T05: self-check mode serves the restricted read-only surface —
    // every business route (read or write) is structurally absent and the
    // fallback answers a loud 503 instead of a silent 404.
    let router = if self_check {
        build_self_check_router(state)
    } else {
        build_router(state)
    };
    // The graceful-shutdown future consumes the stop signal and records the
    // signal instant; the drain budget watchdog is armed at THAT instant (the
    // unified from-signal budget — F03) and races the drain. Serving therefore
    // polls the accept loop from the start; dropping the serve future on a
    // watchdog win abandons whatever the drain could not finish.
    let (signal_tx, signal_rx) = tokio::sync::oneshot::channel::<std::time::Instant>();
    let graceful = async move {
        shutdown.await;
        *dream_signal_at.lock().unwrap() = Some(std::time::Instant::now());
        begin_background_shutdown(&state_on_signal);
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
    let outcome = match drain_budget {
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
    };
    if let Some(processes) = native_processes {
        let remaining = dream_stop_at
            .lock()
            .unwrap()
            .map(|at| dream_budget.saturating_sub(at.elapsed()))
            .unwrap_or(dream_budget);
        match tokio::time::timeout(remaining, processes.shutdown_all()).await {
            Ok(receipts)
                if receipts.iter().all(|receipt| {
                    !matches!(
                        receipt.outcome,
                        procsupervisor::TerminationOutcome::CleanupTimedOut
                    )
                }) => {}
            _ => {
                return outcome.map(|_| ServeOutcome {
                    drain_timed_out: true,
                })
            }
        }
    }
    knowledge_worker
        .wait_worker_stopped()
        .await
        .map_err(|error| ServiceError::Serve {
            source: std::io::Error::other(format!("knowledge-worker stop: {error}")),
        })?;
    dream_service.dream().stop_all();
    automation_host.stop();
    bridge_host.stop();
    conversation_host.stop();
    browser_service.stop();
    mcp_service.begin_stop();
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, mcp_service.shutdown()).await {
        Ok(Ok(())) => {}
        Err(_) => {
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            })
        }
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("mcp shutdown: {error}")),
            })
        }
    }
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, workflows.stop(remaining)).await {
        Ok(Ok(unfinished)) if unfinished.is_empty() => {}
        Ok(Ok(_)) | Err(_) => {
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            })
        }
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("workflows stop: {error}")),
            })
        }
    }
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, media_service.stop(remaining)).await {
        Ok(Ok(unfinished)) if unfinished.is_empty() => {}
        Ok(Ok(_)) | Err(_) => {
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            })
        }
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("media stop: {error}")),
            })
        }
    }
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, beautify_service.shutdown(remaining)).await {
        Ok(Ok(unfinished)) if unfinished.is_empty() => {}
        Ok(Ok(_)) | Err(_) => {
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            })
        }
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("beautify shutdown: {error}")),
            })
        }
    }
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, browser_service.wait_stopped()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("browser stop: {error}")),
            })
        }
        Err(_) => {
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            })
        }
    }
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, conversation_host.wait_stopped()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("conversation stop: {error}")),
            })
        }
        Err(_) => {
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            })
        }
    }
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, bridge_host.wait_stopped()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("bridge stop: {error}")),
            })
        }
        Err(_) => {
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            })
        }
    }
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, automation_host.wait_stopped()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("automation stop: {error}")),
            })
        }
        Err(_) => {
            tracing::error!("自动化任务清理超时，未确认的任务保留恢复记录");
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            });
        }
    }
    let remaining = dream_stop_at
        .lock()
        .unwrap()
        .map(|at| dream_budget.saturating_sub(at.elapsed()))
        .unwrap_or(dream_budget);
    match tokio::time::timeout(remaining, dream_service.dream().wait_stopped_checked()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return Err(ServiceError::Serve {
                source: std::io::Error::other(format!("dream stop: {error}")),
            })
        }
        Err(_) => {
            eprintln!(
                "{} phase=memory_dream residue=unsettled_model_call",
                shutdown::SHUTDOWN_TIMEOUT_MARKER
            );
            return outcome.map(|_| ServeOutcome {
                drain_timed_out: true,
            });
        }
    }
    outcome
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
                "dataEpoch": 2
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
