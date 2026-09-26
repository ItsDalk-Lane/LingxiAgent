//! lingxi-service — Rust 独立服务与组合根（R02-T01 建立，R02-T02 扩展配置/路径/实例）。
//!
//! 契约锚点（任务书 `02_目标架构与强制契约.md` §1/§2，R02-T01/T02）：
//! - 本 crate 是进程组合根：配置、传输（HTTP）与 port 注入都在这里发生；
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
//! - 版本事实单一来源：wire 协议版本与 data epoch 取自 lingxi-protocol
//!   常量，本 crate 不复制第二份（实例记录快照同一来源）。

pub mod config;
pub mod instance;
pub mod paths;

pub use config::{
    parse_cli, read_config_home, resolve_effective_home, CliOptions, ConfigError, HomeSource,
    IgnoredHomeSource, ResolvedHome, HOME_ENV_VAR,
};
pub use instance::{
    acquire, probe_peer, InstanceGuard, InstanceIdentity, InstanceLockError, InstanceRecord,
    PeerProbe, SINGLE_WRITER_BLOCKED_MARKER, STALE_RECORD_MARKER,
};
pub use paths::{prepare_layout, DataRootLayout};

use std::fmt;
use std::future::Future;
use std::net::SocketAddr;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use lingxi_protocol::handshake::{WIRE_PROTOCOL_MAX_SUPPORTED, WIRE_PROTOCOL_MIN_SUPPORTED};
use lingxi_protocol::ContractVersions;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    /// Socket address to listen on. Defaults to loopback with an ephemeral
    /// port; LAN binding requires explicit configuration (R02-T03 owns the
    /// auth surface that would make that safe).
    pub bind_addr: SocketAddr,
    /// Absolute path of the isolated service data root (as given by the
    /// winning source; canonicalization happens in [`paths::prepare_layout`]).
    pub data_home: std::path::PathBuf,
    /// Which source supplied `data_home` (diagnostics; see [`HomeSource`]).
    pub home_source: HomeSource,
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
        Ok(Self {
            bind_addr,
            data_home,
            home_source: resolved.source,
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
/// `lingxi.wire` v1 vocabulary — R02-T03 owns the endpoint permission table;
/// values are sourced from the single authorities, never duplicated here).
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

/// Live service state shared by handlers. Port implementations
/// (storage/events/auth) are injected here as their R02 tasks land them.
#[derive(Debug, Clone)]
pub struct ServiceState {
    config: std::sync::Arc<ServiceConfig>,
}

impl ServiceState {
    pub fn new(config: ServiceConfig) -> Self {
        Self {
            config: std::sync::Arc::new(config),
        }
    }

    pub fn config(&self) -> &ServiceConfig {
        &self.config
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

async fn health(State(_state): State<ServiceState>) -> Json<HealthResponse> {
    Json(health_payload())
}

/// Builds the full HTTP router with all injected state.
pub fn build_router(state: ServiceState) -> Router {
    Router::new()
        .route("/lingxi/v1/health", get(health))
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
/// `on_ready`); this function stays transport-only so the in-process test
/// harness keeps working without a data root.
pub async fn run<F>(
    config: ServiceConfig,
    shutdown: F,
    on_ready: impl FnOnce(SocketAddr),
) -> Result<(), ServiceError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .map_err(|source| ServiceError::Bind {
            addr: config.bind_addr,
            source,
        })?;
    let local = listener.local_addr().map_err(|source| ServiceError::Bind {
        addr: config.bind_addr,
        source,
    })?;
    on_ready(local);

    let router = build_router(ServiceState::new(config));
    let server = axum::serve(listener, router).with_graceful_shutdown(shutdown);
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
    }

    #[test]
    fn config_defaults_to_loopback_ephemeral() {
        let cfg = ServiceConfig::from_cli_args(args(&["--home", "/tmp/h"])).unwrap();
        assert!(cfg.bind_addr.ip().is_loopback());
        assert_eq!(cfg.bind_addr.port(), 0);
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
        // configuration echo: minimal public surface (R02-T03 hardens this).
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
