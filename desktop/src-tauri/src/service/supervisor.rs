//! R09-T02 服务 supervisor：解析 → 校验 → spawn → READY 握手 → 身份/协议 → 发布
//! → 崩溃有限重启 → 退出清理（C01–C28 的 T01/T02 最小面）。
//!
//! 关键契约（与 Electron `desktop/main.cjs:1345-1508,2298-2349,6854-6940` 对齐）：
//! - **禁止固定 sleep**：READY 只认服务 stdout 的 `LINGXI_SERVICE_READY addr=…`，
//!   90s 预算，提前 exit/超时都失败并回收本轮 child；
//! - **协议门用 lingxi-protocol Rust 常量**（wire 1..=1、data_epoch 2），
//!   打包版查清单、运行时查 `instance.json`/health；
//! - 崩溃**最多重启 1 次**，再失败写失败记录并交给 fatal handler（弹窗/退出）；
//! - **reused/remote 结构性零接触**：这两种模式 `child` 永远为 None；
//! - release 拒绝远端（`REMOTE_REQUIRES_DEV_BUILD`）；
//! - 失败记录先落盘（`failure.json` + diagnostics + crash log），`fallbackAttempts`
//!   恒为 0——Tauri 宿主代码里不存在 Node 兜底路径。

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use super::client;
use super::diagnostics::{now_unix_ms, Diagnostics};
use super::env::EnvSnapshot;
use super::error::{codes, ServiceResult, StartError};
use super::instance;
use super::manifest::ProtocolIdentity;
use super::remote;
use super::resolve;
use super::shutdown::{self, OwnedChild, ShutdownReport};

pub const READY_PREFIX: &str = "LINGXI_SERVICE_READY addr=";
pub const READY_OUTPUT_LIMIT: usize = 64 * 1024;
pub const READY_LINE_LIMIT: usize = 8 * 1024;
pub const LOG_LINE_CAP: usize = 200;

pub type EventSink = Arc<dyn Fn(&str, Value) + Send + Sync>;
pub type FatalHandler = Arc<dyn Fn(&StartError) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct Timings {
    pub read_timeout: Duration,
    pub shutdown_grace: Duration,
    pub force_kill_wait: Duration,
    pub poll: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            read_timeout: Duration::from_secs(90),
            shutdown_grace: shutdown::SHUTDOWN_GRACE,
            force_kill_wait: shutdown::FORCE_KILL_WAIT,
            poll: shutdown::SHUTDOWN_POLL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMode {
    LocalOwned,
    LocalReused,
    Remote,
}

impl ServiceMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalOwned => "local-owned",
            Self::LocalReused => "local-reused",
            Self::Remote => "remote",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "local-owned" => Some(Self::LocalOwned),
            "local-reused" => Some(Self::LocalReused),
            "remote" => Some(Self::Remote),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Idle,
    Starting,
    Ready,
    Stopping,
    Stopped,
    Failed,
}

impl Lifecycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConnection {
    pub port: u16,
    pub token: String,
    pub server_node_kind: String,
    pub server_node_transport: String,
    /// 本地为 None（渲染层语义 = 127.0.0.1）；远端为真实 hostname。
    pub host: Option<String>,
    pub base_url: String,
    pub pid: Option<u32>,
    pub instance_id: Option<String>,
    pub mode: ServiceMode,
    pub protocol: Option<ProtocolIdentity>,
}

/// `get_server_connection_info` 的线形状：与 Electron IPC 逐字段一致。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfoView {
    pub port: Option<u16>,
    pub token: Option<String>,
    pub server_node_kind: Option<String>,
    pub server_node_transport: String,
    pub host: Option<String>,
}

impl ConnectionInfoView {
    pub fn empty() -> Self {
        Self {
            port: None,
            token: None,
            server_node_kind: None,
            server_node_transport: "http".to_string(),
            host: None,
        }
    }

    pub fn from_connection(connection: Option<&ServerConnection>) -> Self {
        match connection {
            Some(connection) => Self {
                port: Some(connection.port),
                token: Some(connection.token.clone()),
                server_node_kind: Some(connection.server_node_kind.clone()),
                server_node_transport: connection.server_node_transport.clone(),
                host: connection.host.clone(),
            },
            None => Self::empty(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// 已 canonicalize 的数据根；spawn env 与私有记录读取共用。
    pub home: PathBuf,
    pub env: EnvSnapshot,
    pub packaged: bool,
    /// `cfg!(debug_assertions)`：远端仅 debug/dev 预览允许。
    pub debug_build: bool,
    pub app_version: String,
    pub current_exe: PathBuf,
    /// dev 缺省二进制（`<repo>/rust/target/debug/lingxi-service`）。
    pub dev_default_binary: PathBuf,
    pub timings: Timings,
}

#[derive(Default)]
struct RuntimeState {
    mode: Option<ServiceMode>,
    connection: Option<ServerConnection>,
    child: Option<OwnedChild>,
    lifecycle: Lifecycle,
    restart_attempts: u8,
    stopping: bool,
    monitor_started: bool,
    last_error: Option<StartError>,
    resolved: Option<ResolvedService>,
    manifest: Option<Value>,
    spawn_record: Option<Value>,
    instance_record: Option<Value>,
    shutdown_report: Option<ShutdownReport>,
    ready_addr: Option<String>,
    ready_at_unix_ms: Option<u64>,
    published_at_unix_ms: Option<u64>,
    started_at_unix_ms: Option<u64>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone)]
struct ResolvedService {
    binary: PathBuf,
}

enum ReadyEvent {
    Line(String),
    Eof,
    TooLarge,
}

pub struct ServiceRuntime {
    config: SupervisorConfig,
    diagnostics: Diagnostics,
    state: Mutex<RuntimeState>,
    events: Mutex<Option<EventSink>>,
    fatal: Mutex<Option<FatalHandler>>,
    stopping: AtomicBool,
}

impl ServiceRuntime {
    pub fn new(config: SupervisorConfig) -> Arc<Self> {
        let diagnostics = Diagnostics::for_home(&config.home);
        Self::new_with_diagnostics(config, diagnostics)
    }

    /// 与宿主 setup 共享同一个 Diagnostics 实例（启动期缓冲/回填顺序唯一）。
    pub fn new_with_diagnostics(config: SupervisorConfig, diagnostics: Diagnostics) -> Arc<Self> {
        Arc::new(Self {
            config,
            diagnostics,
            state: Mutex::new(RuntimeState::default()),
            events: Mutex::new(None),
            fatal: Mutex::new(None),
            stopping: AtomicBool::new(false),
        })
    }

    pub fn home(&self) -> &std::path::Path {
        &self.config.home
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    pub fn set_event_sink(&self, sink: EventSink) {
        *self
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    }

    pub fn set_fatal_handler(&self, handler: FatalHandler) {
        *self
            .fatal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handler);
    }

    fn emit(&self, event: &str, payload: Value) {
        let sink = self
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(sink) = sink {
            sink(event, payload);
        }
    }

    pub fn connection(&self) -> Option<ServerConnection> {
        self.lock_state().connection.clone()
    }

    pub fn mode(&self) -> Option<ServiceMode> {
        self.lock_state().mode
    }

    pub fn view(&self) -> ConnectionInfoView {
        ConnectionInfoView::from_connection(self.lock_state().connection.as_ref())
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, RuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn status(&self) -> Value {
        let state = self.lock_state();
        let connection = state.connection.as_ref();
        json!({
            "schema": "lingxi.r09.service-status.v1",
            "mode": state.mode.map(|mode| mode.as_str()),
            "lifecycle": state.lifecycle.as_str(),
            "pid": connection.and_then(|connection| connection.pid),
            "port": connection.map(|connection| connection.port),
            "readyAddr": state.ready_addr,
            "host": connection.and_then(|connection| connection.host.clone()),
            "serverNodeKind": connection.map(|connection| connection.server_node_kind.clone()),
            "instanceId": connection.and_then(|connection| connection.instance_id.clone()),
            "restartAttempts": state.restart_attempts,
            "manifest": state.manifest,
            "spawn": state.spawn_record,
            "instance": state.instance_record,
            "lastError": state.last_error.as_ref().map(StartError::to_json),
            "shutdown": state.shutdown_report.as_ref().map(ShutdownReport::to_json),
            "readyAtUnixMs": state.ready_at_unix_ms,
            "publishedAtUnixMs": state.published_at_unix_ms,
            "startedAtUnixMs": state.started_at_unix_ms,
        })
    }

    /// 完整启动链（C02→C24）。成功时已发布连接并启动崩溃监控。
    pub async fn start_and_wait(self: &Arc<Self>) -> ServiceResult<ServerConnection> {
        self.stopping.store(false, Ordering::SeqCst);
        {
            let mut state = self.lock_state();
            state.lifecycle = Lifecycle::Starting;
            state.started_at_unix_ms = Some(now_unix_ms());
            state.last_error = None;
        }
        self.diagnostics.log(
            "resolve",
            json!({
                "home": self.config.home.display().to_string(),
                "packaged": self.config.packaged,
                "debugBuild": self.config.debug_build,
                "appVersion": self.config.app_version,
                "platform": resolve::platform_name(),
            }),
        );
        if let Some(error) = resolve::refuse_data_downgrade(&self.config.env) {
            return Err(self.fail(error));
        }
        if resolve::node_server_info_present(&self.config.home) {
            return Err(self.fail(
                StartError::new(
                    codes::NODE_SERVER_INFO_PRESENT,
                    "a legacy Node server record exists in this data root; resolve it before starting the Rust desktop",
                )
                .with_detail(json!({"path": self.config.home.join("server-info.json").display().to_string()})),
            ));
        }

        // 远端：仅在 debug/dev 预览允许，且结构性不读本地 instance/不 spawn。
        if remote::remote_present(&self.config.env) {
            if !self.config.debug_build {
                return Err(self.fail(StartError::new(
                    codes::REMOTE_REQUIRES_DEV_BUILD,
                    "Rust remote desktop connection is a dev preview; release builds refuse it",
                )));
            }
            let target = match remote::remote_from_env(&self.config.env) {
                Ok(Some(target)) => target,
                Ok(None) => unreachable!("remote_present checked above"),
                Err(error) => return Err(self.fail(error)),
            };
            self.diagnostics.register_secret(&target.token);
            self.diagnostics.log(
                "remote-connect",
                json!({"host": target.host, "port": target.port, "transport": target.transport}),
            );
            let identity =
                match client::verify_identity(&target.base_url, &target.token, &target.transport)
                    .await
                {
                    Ok(identity) => identity,
                    Err(error) => return Err(self.fail(error)),
                };
            self.diagnostics.activate();
            let connection = ServerConnection {
                port: target.port,
                token: target.token.clone(),
                server_node_kind: identity.server_node_kind.clone(),
                server_node_transport: identity.server_node_transport.clone(),
                host: Some(target.host.clone()),
                base_url: target.base_url.clone(),
                pid: None,
                instance_id: None,
                mode: ServiceMode::Remote,
                protocol: Some(ProtocolIdentity::host()),
            };
            self.publish(connection.clone());
            self.diagnostics.log(
                "service-ready",
                json!({
                    "mode": ServiceMode::Remote.as_str(),
                    "host": target.host,
                    "port": target.port,
                    "touchedRemote": false,
                    "identity": {"serverId": identity.server_id, "studioId": identity.studio_id},
                }),
            );
            return Ok(connection);
        }

        // 二进制解析/构建身份门（打包版：清单 + 协议 + 摘要/签名）。
        let resolved = if self.config.packaged {
            let verified = match resolve::resolve_packaged_service(
                &self.config.current_exe,
                &self.config.app_version,
            ) {
                Ok(verified) => verified,
                Err(error) => return Err(self.fail(error)),
            };
            self.diagnostics.log(
                "manifest",
                json!({
                    "binary": verified.binary.display().to_string(),
                    "appVersion": verified.manifest.app_version,
                    "target": verified.manifest.target,
                    "toolchain": verified.manifest.toolchain,
                    "sha256": verified.manifest.sha256,
                    "contentSha256": verified.manifest.content_sha256,
                    "protocol": verified.manifest.protocol.describe(),
                    "signedMutation": verified.signed_mutation,
                    "rendererSha256": verified.manifest.renderer_sha256,
                }),
            );
            {
                let mut state = self.lock_state();
                state.manifest = Some(json!({
                    "appVersion": verified.manifest.app_version,
                    "binary": verified.binary.display().to_string(),
                    "sha256": verified.manifest.sha256,
                    "contentSha256": verified.manifest.content_sha256,
                    "protocol": verified.manifest.protocol.describe(),
                    "signedMutation": verified.signed_mutation,
                    "rendererSha256": verified.manifest.renderer_sha256,
                }));
            }
            ResolvedService {
                binary: verified.binary.clone(),
            }
        } else {
            let binary = match resolve::resolve_dev_binary(
                &self.config.env,
                &self.config.dev_default_binary,
            ) {
                Ok(binary) => binary,
                Err(error) => return Err(self.fail(error)),
            };
            ResolvedService { binary }
        };
        self.diagnostics.log(
            "binary",
            json!({
                "binary": resolved.binary.display().to_string(),
                "packaged": self.config.packaged,
            }),
        );
        {
            let mut state = self.lock_state();
            state.resolved = Some(resolved.clone());
        }

        // C10：已存在实例 → 身份验证 → dev 允许复用（不拥有）/packaged 拒绝。
        let windows_reader = if cfg!(windows) {
            Some(resolved.binary.as_path())
        } else {
            None
        };
        match instance::read_existing(&self.config.home, windows_reader) {
            Ok(Some(existing)) => {
                match client::verify_identity(
                    &existing.base_url,
                    &existing.token,
                    &existing.transport,
                )
                .await
                {
                    Ok(identity) => {
                        if self.config.packaged {
                            return Err(self.fail(
                                StartError::new(
                                    codes::EXISTING_INSTANCE_UNVERIFIED,
                                    "an existing Rust service instance is running for this data root; quit it before starting this installed app",
                                )
                                .with_detail(json!({
                                    "pid": existing.pid,
                                    "port": existing.port,
                                    "instanceId": existing.instance_id,
                                    "fallbackAttempts": 0,
                                })),
                            ));
                        }
                        self.diagnostics.activate();
                        self.diagnostics.register_secret(&existing.token);
                        let connection = ServerConnection {
                            port: existing.port,
                            token: existing.token.clone(),
                            server_node_kind: identity.server_node_kind.clone(),
                            server_node_transport: identity.server_node_transport.clone(),
                            host: None,
                            base_url: existing.base_url.clone(),
                            pid: Some(existing.pid),
                            instance_id: Some(existing.instance_id.clone()),
                            mode: ServiceMode::LocalReused,
                            protocol: Some(ProtocolIdentity::host()),
                        };
                        self.publish(connection.clone());
                        self.diagnostics.log(
                            "service-ready",
                            json!({
                                "mode": ServiceMode::LocalReused.as_str(),
                                "pid": existing.pid,
                                "port": existing.port,
                                "instanceId": existing.instance_id,
                                "owned": false,
                            }),
                        );
                        return Ok(connection);
                    }
                    Err(identity_error) => {
                        return Err(self.fail(
                            StartError::new(
                                codes::RUNTIME_IDENTITY_MISMATCH,
                                "existing Rust instance record is alive but its identity could not be verified; refusing to kill it",
                            )
                            .with_detail(json!({
                                "pid": existing.pid,
                                "port": existing.port,
                                "instanceId": existing.instance_id,
                                "identityError": identity_error.to_json(),
                            })),
                        ));
                    }
                }
            }
            Ok(None) => {}
            Err(error) => return Err(self.fail(error)),
        }

        // spawn → READY → instance → identity。
        let connection = match self.spawn_and_verify(&resolved).await {
            Ok(connection) => connection,
            Err(error) => return Err(error),
        };
        self.publish(connection.clone());
        self.diagnostics.log(
            "service-ready",
            json!({
                "mode": ServiceMode::LocalOwned.as_str(),
                "pid": connection.pid,
                "port": connection.port,
                "instanceId": connection.instance_id,
                "protocol": connection.protocol.as_ref().map(ProtocolIdentity::describe),
            }),
        );
        self.spawn_monitor();
        Ok(connection)
    }

    fn publish(&self, connection: ServerConnection) {
        let mut state = self.lock_state();
        state.mode = Some(connection.mode);
        state.connection = Some(connection.clone());
        state.lifecycle = Lifecycle::Ready;
        state.ready_at_unix_ms = state.ready_at_unix_ms.or(Some(now_unix_ms()));
        state.published_at_unix_ms = Some(now_unix_ms());
    }

    async fn spawn_and_verify(
        self: &Arc<Self>,
        resolved: &ResolvedService,
    ) -> ServiceResult<ServerConnection> {
        let home = self.config.home.clone();
        let spawn_unix_ms = now_unix_ms();
        let spawn_instant = Instant::now();
        let binary = resolved.binary.clone();
        let mut command = std::process::Command::new(&binary);
        command
            .arg("--home")
            .arg(&home)
            .env(resolve::HOME_ENV_VAR, &home)
            .current_dir(binary.parent().unwrap_or(home.as_path()))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // 自有进程组：退出时整树收敛，且不误伤远端/复用进程。
            command.process_group(0);
        }
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let error = StartError::new(
                    codes::SERVICE_SPAWN_FAILED,
                    format!("Rust service spawn failed: {error}"),
                )
                .with_detail(json!({"binary": binary.display().to_string()}));
                return Err(self.fail(error));
            }
        };
        let mut child = child;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let owned = OwnedChild::new(child);
        let pid = owned.pid;
        let pgid = owned.pgid;

        let sink = self.diagnostics.clone();
        let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
        {
            let stderr_tail = stderr_tail.clone();
            let diagnostics = sink.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let line = diagnostics.redact(&line);
                    let mut tail = stderr_tail
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if tail.len() >= LOG_LINE_CAP {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            });
        }
        let (sender, receiver) = std::sync::mpsc::channel::<ReadyEvent>();
        {
            let diagnostics = sink.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                let mut total = 0usize;
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            total += line.len() + 1;
                            if total > READY_OUTPUT_LIMIT || line.len() > READY_LINE_LIMIT {
                                let _ = sender.send(ReadyEvent::TooLarge);
                                return;
                            }
                            let line = diagnostics.redact(&line);
                            if sender.send(ReadyEvent::Line(line)).is_err() {
                                return;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = sender.send(ReadyEvent::Eof);
            });
        }
        {
            let mut state = self.lock_state();
            state.child = Some(owned);
            state.spawn_record = Some(json!({
                "binary": binary.display().to_string(),
                "args": ["--home", home.display().to_string()],
                "pid": pid,
                "pgid": pgid,
                "spawnedAtUnixMs": spawn_unix_ms,
                "cwd": binary.parent().map(|parent| parent.display().to_string()),
            }));
        }
        self.diagnostics.log(
            "spawn",
            json!({
                "binary": binary.display().to_string(),
                "args": ["--home", home.display().to_string()],
                "pid": pid,
                "pgid": pgid,
                "spawnedAtUnixMs": spawn_unix_ms,
            }),
        );

        // READY 握手：只认 stdout 的 READY 行（禁止固定 sleep）。
        let deadline = Instant::now() + self.config.timings.read_timeout;
        let ready_addr = loop {
            if Instant::now() >= deadline {
                let error = StartError::new(
                    codes::SERVICE_READY_TIMEOUT,
                    format!(
                        "Rust service READY timed out after {}ms",
                        self.config.timings.read_timeout.as_millis()
                    ),
                )
                .with_detail(json!({
                    "pid": pid,
                    "stderrTail": self.stderr_tail(&stderr_tail),
                    "readyPrefix": READY_PREFIX,
                }));
                return Err(self.abort_start(error).await);
            }
            // 子进程提前退出。
            let exited = {
                let mut state = self.lock_state();
                state.child.as_mut().and_then(OwnedChild::try_wait)
            };
            if let Some(status) = exited {
                let tail = self.stderr_tail(&stderr_tail);
                let single_writer = tail
                    .iter()
                    .any(|line| line.contains("LINGXI_SERVICE_SINGLE_WRITER_BLOCKED"));
                let code = if single_writer {
                    codes::SERVICE_SINGLE_WRITER_BLOCKED
                } else {
                    codes::SERVICE_EXITED_BEFORE_READY
                };
                let mut error = StartError::new(
                    code,
                    format!(
                        "Rust service exited before READY ({})",
                        format_status(&status)
                    ),
                )
                .with_detail(
                    json!({"pid": pid, "exit": shutdown::status_json(status), "stderrTail": tail}),
                );
                if single_writer {
                    error = error.with_expected("exactly one service instance per data root");
                }
                return Err(self.abort_start(error).await);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait = remaining.min(self.config.timings.poll.max(Duration::from_millis(20)));
            match receiver.recv_timeout(wait) {
                Ok(ReadyEvent::Line(line)) => {
                    if let Some(addr) = parse_ready_line(&line) {
                        break addr;
                    }
                }
                Ok(ReadyEvent::TooLarge) => {
                    let error = StartError::new(
                        codes::SERVICE_READY_OUTPUT_EXCEEDED,
                        format!("Rust service READY output exceeded {READY_OUTPUT_LIMIT} bytes"),
                    )
                    .with_detail(json!({"pid": pid}));
                    return Err(self.abort_start(error).await);
                }
                Ok(ReadyEvent::Eof) => {
                    // stdout 关闭：下一轮用 exit/channel 断开兜底。
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        let ready_ms = spawn_instant.elapsed().as_millis() as u64;
        // 服务在监听前已完成 data-epoch stamp：此时 home 可安全写宿主诊断
        // （未 stamp 的新 home 若被宿主写入，下一次服务启动会被 epoch 门拒绝）。
        self.diagnostics.activate();
        {
            let mut state = self.lock_state();
            state.ready_addr = Some(ready_addr.clone());
        }
        self.diagnostics.log(
            "ready",
            json!({"pid": pid, "addr": ready_addr, "msSinceSpawn": ready_ms}),
        );

        // 私有记录 + 运行时协议门（A03 runtime-identity-mismatch）。
        let windows_reader = if cfg!(windows) {
            Some(resolved.binary.as_path())
        } else {
            None
        };
        let owned = match instance::read_owned(&home, pid, &ready_addr, windows_reader) {
            Ok(owned) => owned,
            Err(error) => return Err(self.abort_start(error).await),
        };
        self.diagnostics.register_secret(&owned.token);
        self.diagnostics.log(
            "instance",
            json!({
                "pid": pid,
                "instanceId": owned.instance_id,
                "addr": ready_addr,
                "protocol": format!(
                    "wire {}..={} epoch {}",
                    owned.record.wire_protocol_min,
                    owned.record.wire_protocol_max,
                    owned.record.data_epoch
                ),
                "msSinceSpawn": spawn_instant.elapsed().as_millis() as u64,
            }),
        );
        {
            let mut state = self.lock_state();
            state.instance_record = Some(json!({
                "pid": pid,
                "instanceId": owned.instance_id.clone(),
                "bindAddr": owned.record.bind_addr.clone(),
                "wireProtocolMin": owned.record.wire_protocol_min,
                "wireProtocolMax": owned.record.wire_protocol_max,
                "dataEpoch": owned.record.data_epoch,
            }));
        }
        // 健康面 + identity（Bearer 认证、transport 匹配）。
        if let Err(error) = client::verify_health(&owned.base_url).await {
            return Err(self.abort_start(error).await);
        }
        let identity =
            match client::verify_identity(&owned.base_url, &owned.token, &owned.transport).await {
                Ok(identity) => identity,
                Err(error) => return Err(self.abort_start(error).await),
            };
        self.diagnostics.log(
            "identity",
            json!({
                "pid": pid,
                "serverId": identity.server_id,
                "studioId": identity.studio_id,
                "transport": identity.server_node_transport,
                "msSinceSpawn": spawn_instant.elapsed().as_millis() as u64,
            }),
        );
        let connection = ServerConnection {
            port: owned.port,
            token: owned.token.clone(),
            server_node_kind: identity.server_node_kind.clone(),
            server_node_transport: identity.server_node_transport.clone(),
            host: None,
            base_url: owned.base_url.clone(),
            pid: Some(pid),
            instance_id: Some(owned.instance_id.clone()),
            mode: ServiceMode::LocalOwned,
            protocol: Some(ProtocolIdentity::host()),
        };
        Ok(connection)
    }

    fn stderr_tail(&self, tail: &Arc<Mutex<VecDeque<String>>>) -> Vec<String> {
        tail.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    /// 启动失败：先回收本轮已 spawn 的 child（SIGTERM→17s→SIGKILL→5s），再写记录。
    async fn abort_start(&self, error: StartError) -> StartError {
        let child = { self.lock_state().child.take() };
        if let Some(mut child) = child {
            let timings = self.config.timings.clone();
            let fallback = json!({"mode": "local-owned", "confirmed": false, "residual": []});
            let report = match tokio::task::spawn_blocking(move || {
                shutdown::stop_owned(&mut child, &timings)
            })
            .await
            {
                Ok(report) => report.to_json(),
                Err(join_error) => {
                    json!({"mode": "local-owned", "confirmed": false, "joinError": join_error.to_string()})
                }
            };
            let _ = fallback;
            self.diagnostics.log(
                "startup-abort",
                json!({"reason": error.code, "shutdown": report}),
            );
        }
        self.fail(error)
    }

    fn fail(&self, error: StartError) -> StartError {
        let record = {
            let mut state = self.lock_state();
            state.lifecycle = Lifecycle::Failed;
            state.connection = None;
            state.last_error = Some(error.clone());
            state.child = None;
            json!({
                "schema": "lingxi.r09.service-launch-failure.v1",
                "reason": error.code,
                "message": error.message,
                "expected": error.expected,
                "actual": error.actual,
                "detail": error.detail,
                "home": self.config.home.display().to_string(),
                "packaged": self.config.packaged,
                "debugBuild": self.config.debug_build,
                "appVersion": self.config.app_version,
                "fallbackAttempts": 0,
                "nodeFallback": "none",
                "atUnixMs": now_unix_ms(),
            })
        };
        self.diagnostics.write_failure(&record);
        self.diagnostics
            .log("service-launch-failed", record.clone());
        self.diagnostics
            .crash(&format!("service launch failed: {}", error.code));
        self.emit("service-launch-failed", record);
        let fatal = self
            .fatal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(fatal) = fatal {
            fatal(&error);
        }
        error
    }

    fn spawn_monitor(self: &Arc<Self>) {
        {
            let mut state = self.lock_state();
            if state.monitor_started {
                return;
            }
            state.monitor_started = true;
        }
        let runtime = Arc::clone(self);
        tokio::spawn(async move {
            runtime.monitor_loop().await;
        });
    }

    async fn monitor_loop(self: Arc<Self>) {
        loop {
            tokio::time::sleep(self.config.timings.poll).await;
            let exited = {
                let mut state = self.lock_state();
                if state.stopping || self.is_stopping() {
                    return;
                }
                match state.child.as_mut() {
                    Some(child) => (child.pid, child.try_wait()),
                    None => return,
                }
            };
            if let Some(status) = exited.1 {
                self.on_child_exit(exited.0, status).await;
                return;
            }
        }
    }

    async fn on_child_exit(self: &Arc<Self>, pid: u32, status: std::process::ExitStatus) {
        let exit = shutdown::status_json(status);
        let attempts = {
            let mut state = self.lock_state();
            state.child = None;
            state.connection = None;
            state.restart_attempts = state.restart_attempts.saturating_add(1);
            state.restart_attempts
        };
        let will_retry = attempts <= 1;
        self.diagnostics.log(
            "service-crashed",
            json!({"pid": pid, "exit": exit, "attempt": attempts, "willRetry": will_retry}),
        );
        self.emit(
            "service-crashed",
            json!({
                "reason": codes::SERVICE_EXITED_BEFORE_READY,
                "willRetry": will_retry,
                "attempt": attempts,
                "pid": pid,
                "exit": exit,
            }),
        );
        if !will_retry {
            self.fail(
                StartError::new(
                    codes::CRASH_RESTART_EXHAUSTED,
                    format!("Rust service crashed {attempts} times; not restarting again"),
                )
                .with_detail(json!({"pid": pid, "exit": exit, "fallbackAttempts": 0})),
            );
            return;
        }
        let Some(resolved) = ({ self.lock_state().resolved.clone() }) else {
            self.fail(StartError::new(
                codes::CRASH_RESTART_EXHAUSTED,
                "Rust service crashed before a resumable binary was resolved",
            ));
            return;
        };
        self.diagnostics.log(
            "restart",
            json!({"attempt": attempts, "binary": resolved.binary.display().to_string()}),
        );
        {
            let mut state = self.lock_state();
            state.monitor_started = false;
            state.lifecycle = Lifecycle::Starting;
        }
        match self.spawn_and_verify(&resolved).await {
            Ok(connection) => {
                self.publish(connection.clone());
                self.diagnostics.log(
                    "service-ready",
                    json!({
                        "mode": ServiceMode::LocalOwned.as_str(),
                        "pid": connection.pid,
                        "port": connection.port,
                        "restarted": true,
                        "attempt": attempts,
                    }),
                );
                self.emit(
                    "server-restarted",
                    json!({
                        "port": connection.port,
                        "token": connection.token,
                        "serverNodeKind": connection.server_node_kind,
                        "serverNodeTransport": connection.server_node_transport,
                        "host": connection.host,
                    }),
                );
                self.spawn_monitor();
            }
            Err(error) => {
                self.fail(
                    StartError::new(
                        codes::CRASH_RESTART_EXHAUSTED,
                        format!(
                            "Rust service restart failed after a crash: {}",
                            error.message
                        ),
                    )
                    .with_detail(json!({"restartAttempt": attempts, "lastError": error.to_json()})),
                );
            }
        }
    }

    /// owned 退出清理；reused/remote 结构性零接触（无 Child 句柄）。
    pub async fn stop(self: &Arc<Self>) -> ShutdownReport {
        self.stopping.store(true, Ordering::SeqCst);
        let (mode, child) = {
            let mut state = self.lock_state();
            state.stopping = true;
            state.lifecycle = Lifecycle::Stopping;
            (state.mode, state.child.take())
        };
        let report = match (mode, child) {
            (Some(ServiceMode::LocalOwned), Some(mut child)) => {
                let timings = self.config.timings.clone();
                match tokio::task::spawn_blocking(move || {
                    shutdown::stop_owned(&mut child, &timings)
                })
                .await
                {
                    Ok(report) => report,
                    Err(join_error) => ShutdownReport {
                        mode: "local-owned".to_string(),
                        owned_pids: Vec::new(),
                        signaled: Vec::new(),
                        residual: Vec::new(),
                        grace_ms: 0,
                        force_kill_wait_ms: 0,
                        confirmed: false,
                        touched_remote: false,
                        exit: Some(json!({"joinError": join_error.to_string()})),
                    },
                }
            }
            (Some(mode), _) => ShutdownReport::detached(mode.as_str()),
            (None, _) => ShutdownReport::detached("none"),
        };
        {
            let mut state = self.lock_state();
            state.lifecycle = Lifecycle::Stopped;
            state.connection = None;
            state.child = None;
            state.shutdown_report = Some(report.clone());
        }
        self.diagnostics.log("shutdown", report.to_json());
        report
    }

    /// 崩溃监控是否已经因退出清理而停止（证据/测试用）。
    pub fn is_ready(&self) -> bool {
        self.lock_state().lifecycle == Lifecycle::Ready
    }
}

fn format_status(status: &std::process::ExitStatus) -> String {
    if let Some(code) = status.code() {
        format!("exit code {code}")
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            format!("signal {:?}", status.signal())
        }
        #[cfg(not(unix))]
        {
            "unknown status".to_string()
        }
    }
}

/// 解析 READY 行：`LINGXI_SERVICE_READY addr=<addr> home=<path> source=<…>`。
pub fn parse_ready_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix(READY_PREFIX)?;
    let (addr, tail) = match rest.find(' ') {
        Some(index) => (&rest[..index], &rest[index + 1..]),
        None => (rest, ""),
    };
    if addr.is_empty() || !tail.starts_with("home=") {
        return None;
    }
    Some(addr.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_line_parses_only_the_contract_shape() {
        assert_eq!(
            parse_ready_line(
                "LINGXI_SERVICE_READY addr=127.0.0.1:43120 home=/private/tmp/h source=--home"
            ),
            Some("127.0.0.1:43120".to_string())
        );
        assert_eq!(
            parse_ready_line("LINGXI_SERVICE_READY addr=[::1]:43120 home=/tmp/h source=--home"),
            Some("[::1]:43120".to_string())
        );
        assert_eq!(
            parse_ready_line("LINGXI_SERVICE_READY addr=127.0.0.1:43120"),
            None
        );
        assert_eq!(
            parse_ready_line("ready addr=127.0.0.1:43120 home=/tmp"),
            None
        );
        assert_eq!(
            parse_ready_line("LINGXI_SERVICE_READY addr= home=/tmp"),
            None
        );
        assert_eq!(
            parse_ready_line("LINGXI_SERVICE_READY addr=127.0.0.1:1 xhome=/tmp"),
            None
        );
    }

    #[test]
    fn connection_view_matches_electron_fields() {
        let view = ConnectionInfoView::empty();
        assert_eq!(
            serde_json::to_value(&view).unwrap(),
            json!({
                "port": null,
                "token": null,
                "serverNodeKind": null,
                "serverNodeTransport": "http",
                "host": null,
            })
        );
    }

    #[test]
    fn service_mode_roundtrip() {
        for mode in [
            ServiceMode::LocalOwned,
            ServiceMode::LocalReused,
            ServiceMode::Remote,
        ] {
            assert_eq!(ServiceMode::from_str(mode.as_str()), Some(mode));
        }
        assert_eq!(ServiceMode::from_str("unknown"), None);
    }

    #[test]
    fn timings_match_electron_constants() {
        let timings = Timings::default();
        assert_eq!(timings.read_timeout, Duration::from_secs(90));
        assert_eq!(timings.shutdown_grace, Duration::from_secs(17));
        assert_eq!(timings.force_kill_wait, Duration::from_secs(5));
        assert_eq!(timings.poll, Duration::from_millis(200));
    }
}
