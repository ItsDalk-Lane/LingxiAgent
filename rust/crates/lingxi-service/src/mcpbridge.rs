//! R04-T07: MCP integration through the OFFICIAL `rmcp` 3.4.1 SDK
//! (R01 D-06 — no self-written MCP protocol stack).
//!
//! Scope (frozen by the incumbent surface + the taskbook):
//!
//! - **Transport**: the SDK's async duplex transport over any
//!   `AsyncRead + AsyncWrite`, including REAL child-process stdio servers
//!   spawned by this bridge with a whitelisted environment. R07另接官方
//!   Streamable HTTP，复用产品网络与独立OAuth；旧独立SSE仍明确拒绝，
//!   不将HTTP中的SSE事件解析冒充旧SSE传输。
//! - **Identity**: one server id namespaces every tool
//!   (`ToolOrigin::Mcp { server_id }` → target id
//!   `tool:mcp:{server}:{tool}`, byte-compatible with the incumbent TS
//!   identity builder). Two servers listing the same tool name are two
//!   DIFFERENT targets (T01 semantics — no overwrite; a name-only
//!   resolution across servers is explicitly ambiguous).
//! - **Trust posture**: EVERYTHING a server sends (descriptions, schemas,
//!   annotations, results, resource URIs) is untrusted input. Server
//!   annotations land in `declared_permission` (audit only) — every MCP
//!   tool registers as `PermissionKind::Execute` and keeps the
//!   CONSERVATIVE recovery classification regardless of any
//!   `readOnlyHint` claim. A remote resource URI is NEVER mapped to a
//!   local `ResourceRef`: a server claiming `file:///home/...` mints
//!   nothing locally (remote identity is not a local authorization
//!   fact).
//! - **Unknown-not-retry**: a transport break or deadline expiry mid-call
//!   is `ToolOutcome::Unknown` — the request MAY have reached the server
//!   and executed. The bridge NEVER auto-replays a call; reconnecting
//!   re-initializes and re-lists, but re-issuing a write is a NEW
//!   invocation decision that belongs to the run driver / the user
//!   (R04-A13).
//! - **Host credentials never transit**: the bridge passes NO host
//!   token, ticket or credential to any server; stdio children get a
//!   whitelist environment only ([`SAFE_MCP_ENV_PASSTHROUGH`]), never
//!   the service process environment.
//!
//! R07 由唯一的连接器设置管理器在生产启动和设置变更时接入本模块。
//! 首次注册就绑定按助手检查权限的执行器；直接注册入口仍供既有调用方使用。

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use lingxi_kernel::invocation::ToolRecoveryCapability;
use lingxi_kernel::ports::{
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess,
};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolCatalogError, ToolManifest, ToolOrigin, ToolRegistry, ToolTargetId,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
#[path = "mcp_http.rs"]
pub mod http;

use rmcp::model::{
    CallToolRequest, CallToolRequestParams, CancelledNotificationParam, ClientCapabilities,
    ClientConfig, ClientRequest, Implementation, PaginatedRequestParams, ProtocolVersion,
    RequestId, ServerResult, Tool as McpTool,
};
use rmcp::service::{NotificationContext, RoleClient, RunningService};
use rmcp::{ClientHandler, ServiceExt};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Mutex as AsyncMutex;

use crate::toolgateway::ToolInvocationGateway;

// ── environment whitelist (stdio children) ─────────────────────────────────

/// The ONLY variables an untrusted stdio MCP server inherits. This is a
/// SUBSET of the exec-command whitelist (exectools) — deliberately
/// without `HOME` (an untrusted external server has no legitimate need
/// to walk the user's home) and without any service credential, token or
/// secret. The service process environment is NEVER wholesale inherited.
pub const SAFE_MCP_ENV_PASSTHROUGH: &[&str] =
    &["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TMPDIR"];

/// Max bytes of one tool result CONTENT before truncation (honest
/// `truncated: true`, never a silent cut).
pub const MCP_MAX_RESULT_CONTENT_BYTES: usize = 256 * 1024;

/// Default per-call deadline when the manifest carries none.
pub const MCP_DEFAULT_CALL_DEADLINE_MS: u64 = 120_000;

/// Hard bound on how many `tools/list` pages ONE listing walk accepts. The
/// server is UNTRUSTED: a server that always returns another cursor could
/// otherwise pin the host in an endless paging loop — the walk refuses
/// loudly instead.
pub const MCP_MAX_LIST_PAGES: usize = 64;

/// Hard bound on how many tools ONE listing walk accepts (across all
/// pages). A server flooding the catalog is refused, not indulged.
pub const MCP_MAX_LISTED_TOOLS: usize = 1024;

// ── the boxed transport surface ─────────────────────────────────────────────

/// One transport endpoint expressed as boxed read/write halves. rmcp's
/// async-rw `IntoTransport` accepts exactly this `(R, W)` tuple shape, so
/// every transport this bridge speaks (in-process duplex with an injected
/// fault point, or a real stdio child) reduces to these two boxes.
pub struct BridgeIo {
    pub read: Box<dyn AsyncRead + Send + Unpin>,
    pub write: Box<dyn AsyncWrite + Send + Unpin>,
}

/// A read-only half that dies with the shared break switch: when
/// `broken` is set every read returns `ECONNRESET` — a real transport
/// death, not a graceful EOF (the deterministic fault point for
/// disconnect/reconnect and the R04-A13 receipt-loss scenario).
pub struct BreakableRead<R: AsyncRead + Unpin> {
    inner: R,
    broken: Arc<AtomicBool>,
}

impl<R: AsyncRead + Unpin> AsyncRead for BreakableRead<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.broken.load(Ordering::SeqCst) {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "transport broken (injected fault)",
            )));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

/// The write-only counterpart of [`BreakableRead`].
pub struct BreakableWrite<W: AsyncWrite + Unpin> {
    inner: W,
    broken: Arc<AtomicBool>,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for BreakableWrite<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.broken.load(Ordering::SeqCst) {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "transport broken (injected fault)",
            )));
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Creates ONE breakable transport pair modelled on a REAL socket: two
/// INDEPENDENT unidirectional pipes (one per direction), each half
/// wrapped with the shared break switch. Tripping the switch fails BOTH
/// directions of BOTH ends with `ECONNRESET` — exactly what a connection
/// reset does; dropping one end entirely gives the other end a graceful
/// EOF on its read (socket semantics, not a shared-lock duplex).
///
/// (Two independent pipes rather than one split duplex: rmcp's transport
/// drives its read loop and its spawned send tasks CONCURRENTLY —
/// independent pipes give each direction exclusive ownership instead of
/// interleaving through a shared split lock.)
pub fn breakable_duplex(max_buf: usize) -> (BridgeIo, BridgeIo, Arc<AtomicBool>) {
    let broken = Arc::new(AtomicBool::new(false));
    // client → peer direction
    let (client_tx, peer_rx) = tokio::io::duplex(max_buf);
    // peer → client direction
    let (peer_tx, client_rx) = tokio::io::duplex(max_buf);
    let client_io = BridgeIo {
        read: Box::new(BreakableRead {
            inner: client_rx,
            broken: Arc::clone(&broken),
        }),
        write: Box::new(BreakableWrite {
            inner: client_tx,
            broken: Arc::clone(&broken),
        }),
    };
    let peer_io = BridgeIo {
        read: Box::new(BreakableRead {
            inner: peer_rx,
            broken: Arc::clone(&broken),
        }),
        write: Box::new(BreakableWrite {
            inner: peer_tx,
            broken: Arc::clone(&broken),
        }),
    };
    (client_io, peer_io, broken)
}

// ── endpoint model ──────────────────────────────────────────────────────────

/// How to reach one MCP server. The endpoint is HOST-controlled
/// configuration — never something a model argument can mint.
#[derive(Clone)]
pub enum McpEndpoint {
    /// A stdio child process (the incumbent's primary transport). Spawned
    /// by THIS bridge: whitelisted env, piped stdio, no shell. The child
    /// is reaped detached; closing the service closes its stdin, which
    /// terminates a well-behaved stdio server.
    Stdio {
        command: String,
        args: Vec<String>,
        /// Explicit extra environment (host-approved; validated at spawn:
        /// names without '='/NUL, values without NUL and <=8KiB).
        env: BTreeMap<String, String>,
        cwd: Option<PathBuf>,
    },
    /// An in-process duplex transport factory (tests and future
    /// composition). The factory OWNS the peer side: it spawns whatever
    /// serves it (an rmcp server task in tests) and returns only the
    /// client-facing IO.
    Duplex {
        factory: Arc<dyn Fn() -> BridgeIo + Send + Sync>,
    },
    /// 官方 Streamable HTTP，网络与授权均由真实组合根提供。
    StreamableHttp {
        url: String,
        client: http::NetworkMcpClient,
        headers:
            std::collections::HashMap<reqwest::header::HeaderName, reqwest::header::HeaderValue>,
        bearer: Option<String>,
        oauth: Option<rmcp::transport::auth::AuthClient<http::NetworkMcpClient>>,
    },
    /// A transport this build does not carry (legacy SSE needs separate official transport
    /// outside the lock — D-06). Connecting is a LOUD refusal, never a
    /// fallback to anything.
    Unsupported { transport: String },
}

// ── client handler (notifications from the server) ─────────────────────────

/// The bridge's rmcp client handler. Server-initiated REQUESTS keep their
/// default refusals (no sampling, no roots, no elicitation — the host
/// never lets an untrusted server drive it); the `tools/list_changed`
/// notification bumps a counter the host reads at its next refresh.
pub struct BridgeClientHandler {
    tool_list_changed: Arc<AtomicU64>,
}

impl BridgeClientHandler {
    /// Test/diagnostic constructor.
    pub fn for_tests() -> Self {
        Self {
            tool_list_changed: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl ClientHandler for BridgeClientHandler {
    fn get_info(&self) -> ClientConfig {
        let mut imp = Implementation::from_build_env();
        imp.name = "lingxi".into();
        imp.version = env!("CARGO_PKG_VERSION").into();
        ClientConfig::new(ClientCapabilities::default(), imp)
            .with_protocol_version(ProtocolVersion::LATEST)
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.tool_list_changed.fetch_add(1, Ordering::SeqCst);
        std::future::ready(())
    }
}

/// Test/diagnostic constructor for [`BridgeClientHandler`].
pub fn bridge_client_for_tests() -> BridgeClientHandler {
    BridgeClientHandler {
        tool_list_changed: Arc::new(AtomicU64::new(0)),
    }
}

// ── connection state ────────────────────────────────────────────────────────

type BridgeClient = RunningService<RoleClient, BridgeClientHandler>;

struct McpConnection {
    service: BridgeClient,
    requests: Arc<McpRequestLifetime>,
    child: Option<McpChild>,
    #[allow(dead_code)]
    negotiated_protocol: String,
    #[allow(dead_code)]
    server_identity: String,
}

/// 每次连接独立封口；先让已投请求用官方SDK取消，再关闭同一连接。
#[derive(Default)]
struct McpRequestLifetime {
    stopping: AtomicBool,
    active: AtomicUsize,
    // 仅记录单次调用的远端结果未知；不把历史未知混成本地进程未回收。
    cancellation_unconfirmed: AtomicBool,
    stopped: tokio::sync::Notify,
    drained: tokio::sync::Notify,
}
impl McpRequestLifetime {
    // 调用方持有实际连接锁，因此领取和该连接被取走不会交错。
    fn enter(self: &Arc<Self>) -> Option<McpActiveRequest> {
        if self.stopping.load(Ordering::Acquire) {
            return None;
        }
        self.active.fetch_add(1, Ordering::AcqRel);
        let guard = McpActiveRequest(self.clone());
        if self.stopping.load(Ordering::Acquire) {
            drop(guard);
            None
        } else {
            Some(guard)
        }
    }
    fn begin_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.stopped.notify_waiters();
    }
    async fn cancelled(&self) {
        loop {
            let notified = self.stopped.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.stopping.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
    async fn drain(&self) -> Result<(), McpBridgeError> {
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let notified = self.drained.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.active.load(Ordering::Acquire) == 0 {
                    return;
                }
                notified.await;
            }
        })
        .await;
        if result.is_err() {
            Err(McpBridgeError::CleanupFailed {
                detail: "已投MCP请求的本地取消收尾未结束；仍继续关闭连接".into(),
            })
        } else {
            if self.cancellation_unconfirmed.load(Ordering::Acquire) {
                tracing::warn!("历史MCP调用的取消通知未确认，远端结果仍未知；本次仅继续核对本地连接及子进程退出");
            }
            Ok(())
        }
    }
}
struct McpActiveRequest(Arc<McpRequestLifetime>);
impl Drop for McpActiveRequest {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
        self.0.drained.notify_waiters();
    }
}

/// 子进程归连接所有；初始化取消、断连和所有者退出均回收同一进程。
struct McpChild {
    process: tokio::process::Child,
    killed: bool,
}

#[cfg(target_os = "macos")]
fn macos_process_group_absent(pgid: u32) -> std::io::Result<bool> {
    let mut query = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PGRP,
        pgid as i32,
    ];
    // 只核对是否为空；缓冲不足或权限错误都不能当成资源已经回收。
    let mut buffer = [0u8; 4096];
    let mut bytes = buffer.len();
    let result = unsafe {
        libc::sysctl(
            query.as_mut_ptr(),
            query.len() as u32,
            buffer.as_mut_ptr().cast(),
            &mut bytes,
            std::ptr::null_mut(),
            0,
        )
    };
    if result == 0 {
        Ok(bytes == 0)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

impl McpChild {
    fn kill(&mut self) -> std::io::Result<()> {
        if self.killed {
            return Ok(());
        }
        if let Some(_pid) = self.process.id() {
            #[cfg(unix)]
            {
                // 自有且尚未回收的子进程编号不会被复用；组内后代一起结束。
                if unsafe { libc::killpg(_pid as i32, libc::SIGKILL) } != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        #[cfg(target_os = "macos")]
                        if error.raw_os_error() == Some(libc::EPERM)
                            && self.process.try_wait()?.is_some()
                            && macos_process_group_absent(_pid)?
                        {
                            // macOS对仅剩未回收退出记录的组返回EPERM；先实收子进程，再核对组空。
                            self.killed = true;
                            return Ok(());
                        }
                        return Err(error);
                    }
                }
            }
            #[cfg(not(unix))]
            self.process.start_kill()?;
            self.killed = true;
        }
        Ok(())
    }
    async fn stop(&mut self) -> Result<(), McpBridgeError> {
        let owned_pid = self.process.id();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let kill = self.kill();
        #[cfg(target_os = "macos")]
        if kill
            .as_ref()
            .is_err_and(|error| error.raw_os_error() == Some(libc::EPERM))
        {
            if let Some(pgid) = owned_pid {
                // 真实Mac退出空窗可能同时getpgid=ESRCH、try_wait=None；
                // 仍须在原预算内实收自有child，并核对整组为空，不把EPERM吞成成功。
                tokio::time::timeout_at(deadline, self.process.wait())
                    .await
                    .map_err(|_| McpBridgeError::CleanupFailed {
                        detail: format!("等待自有退出记录超时；owned_pid={pgid}"),
                    })?
                    .map_err(|error| McpBridgeError::CleanupFailed {
                        detail: format!("等待自有退出记录失败；owned_pid={pgid}：{error}"),
                    })?;
                if !macos_process_group_absent(pgid).map_err(|error| {
                    McpBridgeError::CleanupFailed {
                        detail: format!("自有进程组核对失败；owned_pid={pgid}：{error}"),
                    }
                })? {
                    return Err(McpBridgeError::CleanupFailed {
                        detail: format!("自有child已回收但组仍非空；owned_pid={pgid}"),
                    });
                }
                self.killed = true;
                return Ok(());
            }
        }
        kill.map_err(|error| McpBridgeError::CleanupFailed {
            detail: format!("停止自有进程组失败：{error}; owned_pid={owned_pid:?}"),
        })?;
        tokio::time::timeout_at(deadline, self.process.wait())
            .await
            .map_err(|_| McpBridgeError::CleanupFailed {
                detail: "等待自有子进程退出超时".into(),
            })?
            .map_err(|error| McpBridgeError::CleanupFailed {
                detail: format!("等待自有子进程失败：{error}"),
            })?;
        Ok(())
    }
}
impl Drop for McpChild {
    fn drop(&mut self) {
        if self.kill().is_err() {
            tracing::error!("MCP 子进程组回收未确认");
        }
    }
}
impl McpConnection {
    async fn stop(self) -> Result<(), McpBridgeError> {
        let Self {
            service,
            requests,
            mut child,
            ..
        } = self;
        requests.begin_stop();
        let request_result = requests.drain().await;
        let child_result = if let Some(process) = child.as_mut() {
            process.stop().await
        } else {
            Ok(())
        };
        let service_result =
            tokio::time::timeout(std::time::Duration::from_secs(2), service.cancel())
                .await
                .map_err(|_| McpBridgeError::CleanupFailed {
                    detail: "等待SDK连接退出超时".into(),
                })?
                .map_err(|error| McpBridgeError::CleanupFailed {
                    detail: format!("SDK连接退出任务失败：{error}"),
                });
        request_result
            .and(child_result)
            .and(service_result.map(|_| ()))
    }
}

/// One configured MCP server: endpoint + live connection + the tool
/// targets currently registered from its listing.
pub struct McpServer {
    id: String,
    endpoint: McpEndpoint,
    connection: AsyncMutex<Option<McpConnection>>,
    cancellation: std::sync::Mutex<Option<Arc<McpRequestLifetime>>>,
    /// target id -> server tool name for everything registered from the
    /// current listing (the authoritative target↔tool binding).
    registered: std::sync::Mutex<BTreeMap<String, String>>,
    /// target id -> the EXACT manifest the last sync registered/updated
    /// (change detection + the executor's output-schema snapshot).
    manifests: std::sync::Mutex<BTreeMap<String, ToolManifest>>,
    wire_tools: std::sync::Mutex<BTreeMap<String, McpTool>>,
    metadata_epoch: AtomicU64,
    /// Monotonic count of `tools/list_changed` notifications observed.
    pub tool_list_changed_notifications: Arc<AtomicU64>,
    connect_count: Arc<AtomicU64>,
}

/// The loud refusal vocabulary of the bridge (stable codes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpBridgeError {
    EndpointUnsupported { transport: String },
    ConnectFailed { detail: String },
    NegotiationMissing,
    ServerGone,
    CleanupFailed { detail: String },
}

impl McpBridgeError {
    pub fn code(&self) -> &'static str {
        match self {
            McpBridgeError::EndpointUnsupported { .. } => "mcp_endpoint_unsupported",
            McpBridgeError::ConnectFailed { .. } => "mcp_connect_failed",
            McpBridgeError::NegotiationMissing => "mcp_negotiation_missing",
            McpBridgeError::ServerGone => "mcp_server_gone",
            McpBridgeError::CleanupFailed { .. } => "mcp_cleanup_failed",
        }
    }

    pub fn message(&self) -> String {
        match self {
            McpBridgeError::EndpointUnsupported { transport } => format!(
                "MCP transport {transport:?} is not carried by this build (R01 D-06 lock); \
                 refusing instead of falling back"
            ),
            McpBridgeError::ConnectFailed { detail } => {
                format!("MCP connect/initialize failed: {detail}")
            }
            McpBridgeError::NegotiationMissing => {
                "MCP initialize completed without a negotiated protocol version".to_string()
            }
            McpBridgeError::ServerGone => {
                "MCP server connection is not available for this call".to_string()
            }
            McpBridgeError::CleanupFailed { detail } => {
                format!("MCP 连接或子进程回收未确认：{detail}")
            }
        }
    }
}

impl McpServer {
    pub fn new(id: impl Into<String>, endpoint: McpEndpoint) -> Arc<Self> {
        Arc::new(Self {
            id: id.into(),
            endpoint,
            connection: AsyncMutex::new(None),
            cancellation: std::sync::Mutex::new(None),
            registered: std::sync::Mutex::new(BTreeMap::new()),
            manifests: std::sync::Mutex::new(BTreeMap::new()),
            wire_tools: std::sync::Mutex::new(BTreeMap::new()),
            metadata_epoch: AtomicU64::new(0),
            tool_list_changed_notifications: Arc::new(AtomicU64::new(0)),
            connect_count: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// How many (re)connects this server has performed (audit + A13
    /// evidence: the reconnect happened exactly when expected).
    pub fn connect_count(&self) -> u64 {
        self.connect_count.load(Ordering::SeqCst)
    }

    pub fn is_connected(&self) -> bool {
        self.connection
            .try_lock()
            .map(|c| {
                c.as_ref()
                    .is_some_and(|connection| !connection.service.is_closed())
            })
            .unwrap_or(false)
    }

    pub fn registered_targets(&self) -> BTreeMap<String, String> {
        self.registered
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The EXACT manifest the last sync registered for one target (the
    /// executor's schema snapshot source).
    pub fn synced_manifest(&self, target: &str) -> Option<ToolManifest> {
        self.manifests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(target)
            .cloned()
    }

    pub fn wire_tools(&self) -> BTreeMap<String, McpTool> {
        self.wire_tools
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn metadata_epoch(&self) -> u64 {
        self.metadata_epoch.load(Ordering::SeqCst)
    }

    fn spawn_stdio(
        &self,
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        cwd: &Option<PathBuf>,
    ) -> Result<tokio::process::Child, McpBridgeError> {
        for (name, value) in env {
            if name.is_empty() || name.contains('=') || name.contains('\0') {
                return Err(McpBridgeError::ConnectFailed {
                    detail: format!("invalid MCP child env name {name:?}"),
                });
            }
            if value.contains('\0') || value.len() > 8 * 1024 {
                return Err(McpBridgeError::ConnectFailed {
                    detail: format!("invalid MCP child env value for {name:?}"),
                });
            }
        }
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        // Whitelist ONLY — the service environment (which holds host
        // credentials) is never inherited by an untrusted server process.
        let mut child_env = BTreeMap::new();
        for name in SAFE_MCP_ENV_PASSTHROUGH {
            if let Ok(value) = std::env::var(name) {
                child_env.insert((*name).to_string(), value);
            }
        }
        child_env.extend(env.clone());
        cmd.env_clear().envs(child_env);
        cmd.spawn().map_err(|e| McpBridgeError::ConnectFailed {
            detail: format!("stdio spawn of {command:?} failed: {e}"),
        })
    }

    /// Connects (or reconnects) with a REAL initialize handshake. Returns
    /// the negotiated protocol version + server identity. A reconnect
    /// NEVER replays any previous call — it re-initializes and re-lists;
    /// re-issuing anything is an explicit new decision.
    pub async fn connect(&self) -> Result<(String, String), McpBridgeError> {
        self.begin_stop();
        let mut guard = self.connection.lock().await;
        if let Some(old) = guard.take() {
            old.stop().await?;
        }
        let handler = BridgeClientHandler {
            tool_list_changed: Arc::clone(&self.tool_list_changed_notifications),
        };
        match &self.endpoint {
            McpEndpoint::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                let mut child = McpChild {
                    process: self.spawn_stdio(command, args, env, cwd)?,
                    killed: false,
                };
                let stdin =
                    child
                        .process
                        .stdin
                        .take()
                        .ok_or_else(|| McpBridgeError::ConnectFailed {
                            detail: "stdio child had no stdin".to_string(),
                        })?;
                let stdout =
                    child
                        .process
                        .stdout
                        .take()
                        .ok_or_else(|| McpBridgeError::ConnectFailed {
                            detail: "stdio child had no stdout".to_string(),
                        })?;
                let service = handler.serve((stdout, stdin)).await.map_err(|e| {
                    McpBridgeError::ConnectFailed {
                        detail: format!("{e}"),
                    }
                })?;
                Self::finalize(
                    service,
                    Some(child),
                    guard,
                    self.connect_count.as_ref(),
                    &self.cancellation,
                )
            }
            McpEndpoint::Duplex { factory } => {
                let io = factory();
                let service = handler.serve((io.read, io.write)).await.map_err(|e| {
                    McpBridgeError::ConnectFailed {
                        detail: format!("{e}"),
                    }
                })?;
                Self::finalize(
                    service,
                    None,
                    guard,
                    self.connect_count.as_ref(),
                    &self.cancellation,
                )
            }
            McpEndpoint::StreamableHttp {
                url,
                client,
                headers,
                bearer,
                oauth,
            } => {
                use rmcp::transport::streamable_http_client::{
                    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
                };
                let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone());
                config.custom_headers = headers.clone();
                config.auth_header = bearer.clone();
                config.max_sse_event_size = 1024 * 1024;
                // 不重放旧POST；过期会话必须由新的明确启动重新初始化。
                config.reinit_on_expired_session = false;
                let service = if let Some(oauth) = oauth {
                    handler
                        .serve(StreamableHttpClientTransport::with_client(
                            oauth.clone(),
                            config,
                        ))
                        .await
                        .map_err(|_| McpBridgeError::ConnectFailed {
                            detail: "MCP HTTP 初始化或授权失败".into(),
                        })?
                } else {
                    handler
                        .serve(StreamableHttpClientTransport::with_client(
                            client.clone(),
                            config,
                        ))
                        .await
                        .map_err(|_| McpBridgeError::ConnectFailed {
                            detail: "MCP HTTP 初始化失败".into(),
                        })?
                };
                Self::finalize(
                    service,
                    None,
                    guard,
                    self.connect_count.as_ref(),
                    &self.cancellation,
                )
            }
            McpEndpoint::Unsupported { transport } => Err(McpBridgeError::EndpointUnsupported {
                transport: transport.clone(),
            }),
        }
    }

    fn finalize(
        service: BridgeClient,
        child: Option<McpChild>,
        mut guard: AsyncMutexGuard<'_, Option<McpConnection>>,
        connect_count: &AtomicU64,
        cancellation: &std::sync::Mutex<Option<Arc<McpRequestLifetime>>>,
    ) -> Result<(String, String), McpBridgeError> {
        let peer = service
            .peer_info()
            .ok_or(McpBridgeError::NegotiationMissing)?;
        let negotiated = peer.protocol_version.as_str().to_string();
        if negotiated.is_empty() {
            return Err(McpBridgeError::NegotiationMissing);
        }
        let identity = peer
            .server_info
            .as_ref()
            .map(|i| i.name.to_string())
            .unwrap_or_default();
        connect_count.fetch_add(1, Ordering::SeqCst);
        let requests = Arc::new(McpRequestLifetime::default());
        *cancellation.lock().unwrap_or_else(|p| p.into_inner()) = Some(requests.clone());
        *guard = Some(McpConnection {
            service,
            requests,
            child,
            negotiated_protocol: negotiated.clone(),
            server_identity: identity.clone(),
        });
        Ok((negotiated, identity))
    }

    pub async fn disconnect(&self) {
        if let Err(error) = self.disconnect_checked().await {
            tracing::error!(code = error.code(), "MCP 断连清理未确认");
        }
    }

    /// 同步封住旧连接并唤醒其请求；异步收尾先发送精确取消，再关闭SDK。
    pub fn begin_stop(&self) {
        if let Some(token) = self
            .cancellation
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            token.begin_stop();
        }
    }

    pub async fn disconnect_checked(&self) -> Result<(), McpBridgeError> {
        self.begin_stop();
        let mut guard = self.connection.lock().await;
        if let Some(old) = guard.take() {
            old.stop().await?;
        }
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<McpTool>, McpBridgeError> {
        // Full paginated listing (tools/list until no cursor remains) —
        // BOUNDED: an untrusted server that always returns another cursor
        // or floods tools is refused loudly, never indulged.
        let mut tools = Vec::new();
        let mut listing_bytes = 0usize;
        let mut cursor = None;
        let mut pages = 0usize;
        loop {
            if pages >= MCP_MAX_LIST_PAGES {
                return Err(McpBridgeError::ConnectFailed {
                    detail: format!(
                        "tools/list walk exceeded the {MCP_MAX_LIST_PAGES} page bound \
                         (untrusted server)"
                    ),
                });
            }
            pages += 1;
            let page = match self
                .request_raw(
                    ClientRequest::ListToolsRequest(rmcp::model::ListToolsRequest::with_param(
                        PaginatedRequestParams::default().with_cursor(cursor),
                    )),
                    MCP_DEFAULT_CALL_DEADLINE_MS,
                    None,
                )
                .await
                .map_err(|error| McpBridgeError::ConnectFailed {
                    detail: format!("list_tools: {error:?}"),
                })? {
                ServerResult::ListToolsResult(page) => page,
                _ => {
                    return Err(McpBridgeError::ConnectFailed {
                        detail: "list_tools返回了其它结果".into(),
                    })
                }
            };
            // 工具的扩展元数据同属不可信目录，避免缓存大量 UI 描述耗尽资源。
            let page_bytes = serde_json::to_vec(&page.tools)
                .map_err(|_| McpBridgeError::ConnectFailed {
                    detail: "工具目录编码失败".into(),
                })?
                .len();
            listing_bytes = listing_bytes.saturating_add(page_bytes);
            if listing_bytes > 4 * 1024 * 1024 {
                return Err(McpBridgeError::ConnectFailed {
                    detail: "工具目录超过4MiB".into(),
                });
            }
            tools.extend(page.tools);
            if tools.len() > MCP_MAX_LISTED_TOOLS {
                return Err(McpBridgeError::ConnectFailed {
                    detail: format!(
                        "tools/list walk exceeded the {MCP_MAX_LISTED_TOOLS} tool bound \
                         (untrusted server)"
                    ),
                });
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    /// One `tools/call` with the deadline applied. Only the transport
    /// and deadline failures are UNKNOWN candidates — a server-returned
    /// `is_error` result is a RECEIPT (a completed call that failed).
    async fn request_raw(
        &self,
        request: ClientRequest,
        deadline_ms: u64,
        cancel_scope: Option<crate::cancel::CancelScope>,
    ) -> Result<ServerResult, McpCallFailure> {
        if cancel_scope
            .as_ref()
            .is_some_and(|scope| scope.is_cancelled())
        {
            return Err(McpCallFailure::Cancelled);
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
        let mut cancellation: Option<McpRequestCancellation> = None;
        let mut admission: Option<McpActiveRequest> = None;
        let response = {
            // 预算覆盖取得连接和SDK出站排队；mpsc发送被取消时尚未投递的消息会被丢弃。
            let call = async {
                let (peer, lifetime) = {
                    let guard = self.connection.lock().await;
                    let Some(conn) = guard.as_ref() else {
                        return Err(McpCallFailure::NotConnected);
                    };
                    admission = Some(conn.requests.enter().ok_or(McpCallFailure::Cancelled)?);
                    (conn.service.peer().clone(), conn.requests.clone())
                };
                let exchange = async {
                    let handle = peer
                        .send_cancellable_request(
                            request,
                            rmcp::service::PeerRequestOptions::no_options(),
                        )
                        .await
                        .map_err(|error| McpCallFailure::Transport(error.to_string()))?;
                    // 发送一旦完成就在下个等待点之前安装guard，直接丢future也会通知同一个请求。
                    cancellation = Some(McpRequestCancellation {
                        peer,
                        request_id: Some(handle.id.clone()),
                        admission: admission.take(),
                    });
                    handle
                        .await_response()
                        .await
                        .map_err(|error| McpCallFailure::Transport(error.to_string()))
                };
                tokio::select! {
                    biased;
                    () = lifetime.cancelled() => Err(McpCallFailure::Cancelled),
                    result = exchange => result,
                }
            };
            tokio::select! {
                response = tokio::time::timeout_at(deadline, call) => Some(response),
                () = async {
                    match &cancel_scope {
                        Some(scope) => scope.cancelled().await,
                        None => std::future::pending().await,
                    }
                } => None,
            }
        };
        match response {
            None => {
                if let Some(cancellation) = cancellation.as_mut() {
                    cancellation.cancel().await;
                }
                Err(McpCallFailure::Cancelled)
            }
            Some(Err(_)) => {
                if let Some(cancellation) = cancellation.as_mut() {
                    cancellation.cancel().await;
                }
                Err(McpCallFailure::DeadlineExceeded)
            }
            Some(Ok(result)) => {
                if let Some(cancellation) = cancellation.as_mut() {
                    if matches!(result, Err(McpCallFailure::Cancelled)) {
                        cancellation.cancel().await;
                    } else {
                        cancellation.disarm();
                    }
                }
                result
            }
        }
    }
    async fn call_raw(
        &self,
        tool: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
        deadline_ms: u64,
    ) -> Result<rmcp::model::CallToolResult, McpCallFailure> {
        let cancel = crate::toolgateway::TOOL_LIFETIME
            .try_with(|lifetime| lifetime.cancel.clone())
            .ok();
        let params = CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments);
        match self
            .request_raw(
                ClientRequest::CallToolRequest(CallToolRequest::new(params)),
                deadline_ms,
                cancel,
            )
            .await?
        {
            ServerResult::CallToolResult(result) => Ok(result),
            ServerResult::InputRequiredResult(_) => Err(McpCallFailure::InputRequiredRoundRefused),
            ServerResult::CreateTaskResult(_) => Err(McpCallFailure::TaskRoundRefused),
            _ => Err(McpCallFailure::Transport("unexpected MCP response".into())),
        }
    }
    pub async fn read_resource(
        &self,
        uri: &str,
        deadline_ms: u64,
        cancel: Option<crate::cancel::CancelScope>,
    ) -> Result<rmcp::model::ReadResourceResult, McpCallFailure> {
        let params = rmcp::model::ReadResourceRequestParams::new(uri.to_owned());
        match self
            .request_raw(
                ClientRequest::ReadResourceRequest(rmcp::model::ReadResourceRequest::new(params)),
                deadline_ms,
                cancel,
            )
            .await?
        {
            ServerResult::ReadResourceResult(result) => Ok(result),
            _ => Err(McpCallFailure::Transport(
                "unexpected MCP resource response".into(),
            )),
        }
    }
}

/// 单次调用取消只通知该请求；远端是否停止仍由实际回执决定。
struct McpRequestCancellation {
    peer: rmcp::Peer<RoleClient>,
    request_id: Option<RequestId>,
    admission: Option<McpActiveRequest>,
}

impl McpRequestCancellation {
    fn disarm(&mut self) {
        self.request_id = None;
        self.admission.take();
    }

    async fn cancel(&mut self) {
        let Some(request_id) = self.request_id.clone() else {
            return;
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.peer.notify_cancelled(CancelledNotificationParam::new(
                Some(request_id),
                Some("调用已取消或超时；远端结果尚未确认".into()),
            )),
        )
        .await;
        if !matches!(result, Ok(Ok(()))) {
            if let Some(admission) = &self.admission {
                admission
                    .0
                    .cancellation_unconfirmed
                    .store(true, Ordering::Release);
            }
            tracing::warn!("MCP 请求取消通知未确认，远端结果保持未知");
        }
        self.request_id = None;
        self.admission.take();
    }
}

impl Drop for McpRequestCancellation {
    fn drop(&mut self) {
        let Some(request_id) = self.request_id.take() else {
            return;
        };
        let mut cleanup = Self {
            peer: self.peer.clone(),
            request_id: Some(request_id),
            admission: self.admission.take(),
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    cleanup.cancel().await;
                });
            }
            Err(_) => {
                if let Some(admission) = &cleanup.admission {
                    admission
                        .0
                        .cancellation_unconfirmed
                        .store(true, Ordering::Release);
                }
                cleanup.disarm();
                tracing::warn!("MCP 运行时已退出，无法确认单次请求取消通知");
            }
        }
    }
}

use tokio::sync::MutexGuard as AsyncMutexGuard;

/// Why one raw call did not produce a `CallToolResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpCallFailure {
    NotConnected,
    Transport(String),
    DeadlineExceeded,
    Cancelled,
    InputRequiredRoundRefused,
    TaskRoundRefused,
}

impl McpCallFailure {
    pub fn reason(&self) -> String {
        match self {
            McpCallFailure::NotConnected => {
                "MCP server connection is not available; the call was not dispatched".to_string()
            }
            McpCallFailure::Transport(detail) => format!(
                "MCP transport failed before a receipt arrived ({detail}); the request MAY \
                 have reached the server — outcome is unknown, never blindly retried"
            ),
            McpCallFailure::DeadlineExceeded => {
                "MCP call deadline expired before a receipt arrived; the request MAY have \
                 reached the server — outcome is unknown, never blindly retried"
                    .to_string()
            }
            McpCallFailure::Cancelled => {
                "MCP 调用已取消；已请求远端停止，结果仍未知，不会自动重放".to_string()
            }
            McpCallFailure::InputRequiredRoundRefused => {
                "MCP server asked for interactive input (input_required); the bridge does \
                 not answer elicitation for untrusted servers — the call is refused"
                    .to_string()
            }
            McpCallFailure::TaskRoundRefused => {
                "MCP server materialized an async task (SEP-2663); task polling is not \
                 carried by this bridge — the outcome is unknown"
                    .to_string()
            }
        }
    }
}

// ── tool manifest derivation from a listing ─────────────────────────────────

/// Derives the registrable manifest for one server tool. Annotations are
/// UNTRUSTED: they land in `declared_permission` (audit), never in the
/// permission kind (stays Execute) and never in the recovery capability
/// (stays CONSERVATIVE — a `readOnlyHint` claim grants nothing).
pub fn manifest_for_tool(server_id: &str, tool: &McpTool) -> ToolManifest {
    let declared = match tool.annotations.as_ref().and_then(|a| a.read_only_hint) {
        Some(true) => DeclaredPermission::ReadOnly,
        _ => DeclaredPermission::Other,
    };
    let description = tool
        .description
        .as_ref()
        .map(|d| d.to_string())
        .unwrap_or_default();
    ToolManifest {
        origin: ToolOrigin::Mcp {
            server_id: server_id.to_string(),
        },
        local_name: tool.name.to_string(),
        display_name: tool.title.clone().unwrap_or_else(|| tool.name.to_string()),
        aliases: Vec::new(),
        version: "1".to_string(),
        description: if description.is_empty() {
            format!("MCP tool {}/{}", server_id, tool.name)
        } else {
            description
        },
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema: serde_json::Value::Object(tool.input_schema.as_ref().clone()),
        },
        output_schema: tool.output_schema.as_ref().map(|s| ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema: serde_json::Value::Object(s.as_ref().clone()),
        }),
        permission: PermissionContract {
            kind: PermissionKind::Execute,
            capability_base: format!("mcp.{}.invoke", tool.name),
        },
        availability: Availability::Deferred,
        timeout_ms: None,
        max_concurrency: None,
        declared_permission: declared,
        recovery: ToolRecoveryCapability::CONSERVATIVE,
    }
}

// ── listing sync → registry generation ──────────────────────────────────────

/// The per-server sync outcome (audit facts for the generation bump).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSyncReport {
    pub server_id: String,
    pub negotiated_protocol: String,
    pub server_identity: String,
    pub registered: usize,
    pub updated: usize,
    pub removed: usize,
    pub refused: Vec<String>,
    /// Catalog generation BEFORE and AFTER the sync (any change bumps it —
    /// old prepared handles and stale pins are refused by the T01/T02
    /// gates, and stale approvals die with the target/uninstall).
    pub generation_before: u64,
    pub generation_after: u64,
}

/// Synchronizes the server's CURRENT listing into the registry:
///
/// - new tools register under the `ToolOrigin::Mcp` namespace (a second
///   server with the same tool name is a DIFFERENT target — never an
///   overwrite);
/// - changed tools go through `registry.update`, which bumps the TARGET
///   generation — old prepared handles are refused at execute time with
///   `TargetChanged`;
/// - vanished tools are UNINSTALLED — the target stops resolving; old
///   handles fail with not-registered / target-changed (the A06
///   semantics applied to tools/list changes);
/// - a schema the catalog cannot faithfully execute (references,
///   unsupported keywords — T01's fail-closed subset) is REFUSED for
///   that one tool (recorded in `refused`), never silently loosened.
pub async fn sync_server_tools(
    server: &McpServer,
    registry: &ToolRegistry,
    budget: &SchemaBudget,
) -> Result<McpSyncReport, McpBridgeError> {
    let tools = server.list_tools().await?;
    let generation_before = registry.catalog_generation();
    let mut registered = 0usize;
    let mut updated = 0usize;
    let mut removed = 0usize;
    let mut refused = Vec::new();
    let mut seen_targets: BTreeMap<String, String> = BTreeMap::new();
    let mut next_manifests: BTreeMap<String, ToolManifest> = BTreeMap::new();

    // Duplicate names inside ONE listing fold to the first occurrence
    // (the incumbent's deterministic merge): within one server the name
    // alone selects the executor.
    let mut seen_names: BTreeSet<String> = BTreeSet::new();
    for tool in &tools {
        if !seen_names.insert(tool.name.to_string()) {
            continue;
        }
        let manifest = manifest_for_tool(server.id(), tool);
        let target = match manifest.validate_and_derive_id(budget) {
            Ok(t) => t,
            Err(e) => {
                refused.push(format!("{}: {}", tool.name, e));
                continue;
            }
        };
        let synced = server.synced_manifest(target.as_str());
        match synced {
            Some(previous) if previous == manifest => {
                seen_targets.insert(target.as_str().to_string(), tool.name.to_string());
                next_manifests.insert(target.as_str().to_string(), manifest);
            }
            Some(_) => {
                // Changed listing entry → identity-preserving update (the
                // target generation bumps; stale handles/approvals die).
                match registry.update(&target, manifest.clone(), budget) {
                    Ok(_) => {
                        updated += 1;
                        seen_targets.insert(target.as_str().to_string(), tool.name.to_string());
                        next_manifests.insert(target.as_str().to_string(), manifest);
                    }
                    Err(e) => refused.push(format!("{}: {}", tool.name, e)),
                }
            }
            None => match registry.register(manifest.clone(), budget) {
                Ok(_) => {
                    registered += 1;
                    seen_targets.insert(target.as_str().to_string(), tool.name.to_string());
                    next_manifests.insert(target.as_str().to_string(), manifest);
                }
                Err(e) => refused.push(format!("{}: {}", tool.name, e)),
            },
        }
    }

    // Uninstall targets that vanished from the listing.
    let stale_targets: Vec<ToolTargetId> = {
        let current = server.registered.lock().unwrap_or_else(|p| p.into_inner());
        current
            .keys()
            .filter(|t| !seen_targets.contains_key(*t))
            .map(|t| ToolTargetId::parse(t))
            .collect()
    };
    for target in stale_targets {
        if registry.uninstall(&target).is_ok() {
            removed += 1;
        }
    }
    server.metadata_epoch.fetch_add(1, Ordering::SeqCst);
    // 与实际注册相同，同名目录只保留首项，不能让后项元数据替换当前工具身份。
    let accepted_names = seen_targets.values().cloned().collect::<BTreeSet<_>>();
    let mut wire_tools = BTreeMap::new();
    for tool in tools {
        if accepted_names.contains(tool.name.as_ref()) {
            wire_tools.entry(tool.name.to_string()).or_insert(tool);
        }
    }
    *server.wire_tools.lock().unwrap_or_else(|p| p.into_inner()) = wire_tools;
    *server.registered.lock().unwrap_or_else(|p| p.into_inner()) = seen_targets;
    *server.manifests.lock().unwrap_or_else(|p| p.into_inner()) = next_manifests;

    Ok(McpSyncReport {
        server_id: server.id().to_string(),
        negotiated_protocol: String::new(),
        server_identity: String::new(),
        registered,
        updated,
        removed,
        refused,
        generation_before,
        generation_after: registry.catalog_generation(),
    })
}

// ── the executor (one per server; serves every tool of that server) ────────

/// The binding entry for one MCP tool target on the executor: the
/// server-side tool name plus the CURRENT output schema (validation of
/// structured results happens against this snapshot).
#[derive(Debug, Clone)]
pub struct McpToolBinding {
    pub tool_name: String,
    pub output_schema: Option<serde_json::Value>,
}

/// The `ToolExecutorPort` face of one MCP server. Registered on the T02
/// gateway for every target of the server's listing (the gateway's
/// prepare → policy/approval → execute chain owns authorization; this
/// executor only performs the protocol call).
pub struct McpToolExecutor {
    server: Arc<McpServer>,
    bindings: std::sync::Mutex<BTreeMap<String, McpToolBinding>>,
    budget: SchemaBudget,
    max_result_bytes: AtomicUsize,
}

impl McpToolExecutor {
    pub fn new(server: Arc<McpServer>, budget: SchemaBudget) -> Arc<Self> {
        Arc::new(Self {
            server,
            bindings: std::sync::Mutex::new(BTreeMap::new()),
            budget,
            max_result_bytes: AtomicUsize::new(MCP_MAX_RESULT_CONTENT_BYTES),
        })
    }

    /// Test/diagnostic knob: shrink the result cap to prove truncation
    /// behavior without shipping a huge fixture.
    pub fn set_result_cap(&self, max: usize) {
        self.max_result_bytes.store(max.max(64), Ordering::SeqCst);
    }

    fn binding_of(&self, target: &str) -> Option<McpToolBinding> {
        self.bindings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(target)
            .cloned()
    }

    /// Re-binds this executor's target→tool map after a sync (the
    /// registration/refresh helpers call this).
    pub fn bind_targets(&self, targets: BTreeMap<String, McpToolBinding>) {
        *self.bindings.lock().unwrap_or_else(|p| p.into_inner()) = targets;
    }
}

/// Maps one rmcp content block onto the protocol content model. Remote
/// resources and resource links become TEXT (the server's claim about its
/// own URI) — NEVER a local `ResourceRef`.
pub fn map_content_block(block: rmcp::model::ContentBlock) -> ContentBlock {
    match block {
        rmcp::model::ContentBlock::Text(text) => ContentBlock::Text {
            text: text.text.to_string(),
        },
        rmcp::model::ContentBlock::Image(image) => ContentBlock::Text {
            text: format!(
                "[mcp image content: {} bytes of {}]",
                image.data.len(),
                image.mime_type
            ),
        },
        rmcp::model::ContentBlock::Audio(audio) => ContentBlock::Text {
            text: format!("[mcp audio content: {} bytes]", audio.data.len()),
        },
        rmcp::model::ContentBlock::Resource(resource) => {
            let summary = match &resource.resource {
                rmcp::model::ResourceContents::TextResourceContents { uri, text, .. } => {
                    format!("{uri} (text, {} bytes)", text.len())
                }
                rmcp::model::ResourceContents::BlobResourceContents { uri, blob, .. } => {
                    format!("{uri} (blob, {} bytes)", blob.len())
                }
                _ => "unknown resource shape".to_string(),
            };
            ContentBlock::Text {
                text: format!("[mcp resource {summary} — remote URI, not a local file]"),
            }
        }
        rmcp::model::ContentBlock::ResourceLink(link) => ContentBlock::Text {
            text: format!(
                "[mcp resource link {} — remote URI, not a local file]",
                link.uri
            ),
        },
        _ => ContentBlock::Text {
            text: "[mcp content block of an unknown shape]".to_string(),
        },
    }
}

/// Validation-only pass over a structured result against the tool's
/// output schema. Reuses the catalog's validation vocabulary through
/// normalize_arguments: any violation errors; if the pass would FILL A
/// DEFAULT the server did not send, that too is a violation (an output
/// the server produced must stand on its own).
pub fn validate_structured_output(
    schema: &serde_json::Value,
    value: &serde_json::Value,
    budget: &SchemaBudget,
) -> Result<(), Vec<String>> {
    match lingxi_kernel::toolcatalog::normalize_arguments(schema, value, budget) {
        Ok(normalized) if &normalized == value => Ok(()),
        Ok(_) => Err(vec![
            "structured result relies on schema defaults the server did not send".to_string(),
        ]),
        Err(ToolCatalogError::ArgumentsInvalid { violations }) => Err(violations),
        Err(e) => Err(vec![format!("{e}")]),
    }
}

impl ToolExecutorPort for McpToolExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let target = request.target.clone();
            let Some(binding) = self.binding_of(&target) else {
                return ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Failed {
                        error: ProtocolError::new(
                            ErrorCode::Internal,
                            format!("MCP target {target} is not bound on this executor"),
                            false,
                        ),
                    },
                );
            };
            let arguments = request.arguments.as_value().clone();
            let Some(args_map) = arguments.as_object().cloned() else {
                return ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Failed {
                        error: ProtocolError::new(
                            ErrorCode::InvalidMessage,
                            "MCP arguments must be an object",
                            false,
                        ),
                    },
                );
            };
            match self
                .server
                .call_raw(&binding.tool_name, args_map, MCP_DEFAULT_CALL_DEADLINE_MS)
                .await
            {
                Err(failure) => match failure {
                    // Not connected: the call was NOT dispatched — a
                    // clean, retryable-by-explicit-decision failure.
                    McpCallFailure::NotConnected => ToolExecutionResult::of_ctx(
                        ctx,
                        ToolOutcome::Failed {
                            error: ProtocolError::new(
                                ErrorCode::UpstreamUnavailable,
                                McpBridgeError::ServerGone.message(),
                                true,
                            ),
                        },
                    ),
                    // The receipt never arrived: UNKNOWN — never a blind
                    // retry, never a fabricated clean failure.
                    McpCallFailure::Transport(_)
                    | McpCallFailure::DeadlineExceeded
                    | McpCallFailure::Cancelled
                    | McpCallFailure::TaskRoundRefused => ToolExecutionResult::of_ctx(
                        ctx,
                        ToolOutcome::Unknown {
                            reason: failure.reason(),
                        },
                    ),
                    McpCallFailure::InputRequiredRoundRefused => ToolExecutionResult::of_ctx(
                        ctx,
                        ToolOutcome::Failed {
                            error: ProtocolError::new(
                                ErrorCode::InvalidMessage,
                                McpCallFailure::InputRequiredRoundRefused.reason(),
                                false,
                            ),
                        },
                    ),
                },
                Ok(result) => {
                    // A server-returned error result IS a receipt: a
                    // completed call that failed.
                    if result.is_error == Some(true) {
                        let text = result
                            .content
                            .iter()
                            .filter_map(|b| match b {
                                rmcp::model::ContentBlock::Text(t) => Some(t.text.to_string()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        return ToolExecutionResult::of_ctx(
                            ctx,
                            ToolOutcome::Failed {
                                error: ProtocolError::new(
                                    ErrorCode::UpstreamUnavailable,
                                    format!(
                                        "mcp_tool_error: {}",
                                        if text.is_empty() {
                                            "(no message)".to_string()
                                        } else {
                                            text
                                        }
                                    ),
                                    false,
                                ),
                            },
                        );
                    }
                    // Structured result vs the tool's registered OUTPUT
                    // schema (when it declared one): violations fail the
                    // call — the result is untrusted input, never
                    // force-fit.
                    if let (Some(structured), Some(schema)) = (
                        result.structured_content.as_ref(),
                        binding.output_schema.as_ref(),
                    ) {
                        if let Err(violations) =
                            validate_structured_output(schema, structured, &self.budget)
                        {
                            return ToolExecutionResult::of_ctx(
                                ctx,
                                ToolOutcome::Failed {
                                    error: ProtocolError::new(
                                        ErrorCode::InvalidMessage,
                                        format!(
                                            "mcp_structured_result_violates_schema: {}",
                                            violations.join("; ")
                                        ),
                                        false,
                                    ),
                                },
                            );
                        }
                    }
                    // Content mapping with the honest size cap.
                    let cap = self.max_result_bytes.load(Ordering::SeqCst);
                    let mut content = Vec::new();
                    let mut truncated = false;
                    let mut total = 0usize;
                    for block in result.content {
                        let mapped = map_content_block(block);
                        let size = match &mapped {
                            ContentBlock::Text { text } => text.len(),
                            _ => 64,
                        };
                        if total + size > cap {
                            truncated = true;
                            break;
                        }
                        total += size;
                        content.push(mapped);
                    }
                    if content.is_empty() && result.structured_content.is_none() && !truncated {
                        // No content at all: surfaced as-is with an honest
                        // note (the T08 product gate owns empty-artifact
                        // rejection).
                        content.push(ContentBlock::Text {
                            text: "[mcp tool returned no content]".to_string(),
                        });
                    }
                    if let Some(structured) = result.structured_content.as_ref() {
                        let text = serde_json::to_string(structured)
                            .unwrap_or_else(|_| "<unserializable>".to_string());
                        content.push(ContentBlock::Text {
                            text: format!("[mcp structured result: {text}]"),
                        });
                    }
                    if truncated {
                        content.push(ContentBlock::Text {
                            text: format!("[mcp result truncated: exceeded {cap} byte cap]"),
                        });
                    }
                    let mut success = ToolSuccess::from_content(content);
                    success.truncated = truncated;
                    ToolExecutionResult::of_ctx(ctx, ToolOutcome::Success { result: success })
                }
            }
        })
    }
}

// ── registration (composition entry — production default does NOT call) ─────

/// The registered identities of one MCP server's tools.
pub struct RegisteredMcpServer {
    pub server: Arc<McpServer>,
    pub executor: Arc<McpToolExecutor>,
    pub targets: Vec<ToolTargetId>,
    pub sync: McpSyncReport,
}

fn executor_bindings(
    server: &McpServer,
    _registry: &ToolRegistry,
) -> BTreeMap<String, McpToolBinding> {
    server
        .registered_targets()
        .into_iter()
        .filter_map(|(target, tool_name)| {
            let manifest = server.synced_manifest(&target)?;
            Some((
                target,
                McpToolBinding {
                    tool_name,
                    output_schema: manifest.output_schema.map(|d| d.schema),
                },
            ))
        })
        .collect()
}

fn bind_all(
    server: &Arc<McpServer>,
    executor: &Arc<McpToolExecutor>,
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
) -> Vec<ToolTargetId> {
    bind_all_with(
        server,
        executor,
        registry,
        gateway,
        |gateway, target, executor| {
            gateway.bind_executor(
                target.clone(),
                Arc::clone(executor) as Arc<dyn ToolExecutorPort>,
                "R04-T07 MCP bridge executor",
            );
        },
    )
}

fn bind_all_with<F>(
    server: &Arc<McpServer>,
    executor: &Arc<McpToolExecutor>,
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    bind: F,
) -> Vec<ToolTargetId>
where
    F: Fn(&ToolInvocationGateway, &ToolTargetId, &Arc<McpToolExecutor>),
{
    let bindings = executor_bindings(server, registry);
    let targets: Vec<ToolTargetId> = bindings
        .keys()
        .map(|key| ToolTargetId::parse(key))
        .collect();
    executor.bind_targets(bindings);
    for target in &targets {
        bind(gateway, target, executor);
    }
    targets
}

/// Connects a server with a REAL initialize handshake, synchronizes its
/// tool listing into the registry (generation bump on change) and binds
/// the bridge executor for every listed tool on the T02 gateway.
pub async fn register_mcp_server(
    server: Arc<McpServer>,
    registry: &Arc<ToolRegistry>,
    gateway: &ToolInvocationGateway,
    budget: &SchemaBudget,
) -> Result<RegisteredMcpServer, McpBridgeError> {
    register_mcp_server_with_binding(
        server,
        registry,
        gateway,
        budget,
        |gateway, target, executor| {
            gateway.bind_executor(
                target.clone(),
                Arc::clone(executor) as Arc<dyn ToolExecutorPort>,
                "R04-T07 MCP bridge executor",
            );
        },
    )
    .await
}

/// 生产管理入口在首次公开执行器时就绑定助手权限，避免事后覆盖留下裸执行窗口。
pub async fn register_mcp_server_with_binding<F>(
    server: Arc<McpServer>,
    registry: &Arc<ToolRegistry>,
    gateway: &ToolInvocationGateway,
    budget: &SchemaBudget,
    bind: F,
) -> Result<RegisteredMcpServer, McpBridgeError>
where
    F: Fn(&ToolInvocationGateway, &ToolTargetId, &Arc<McpToolExecutor>) + Send + Sync,
{
    let (negotiated, identity) = server.connect().await?;
    let mut sync = sync_server_tools(&server, registry, budget).await?;
    sync.negotiated_protocol = negotiated;
    sync.server_identity = identity;
    let executor = McpToolExecutor::new(Arc::clone(&server), *budget);
    let targets = bind_all_with(&server, &executor, registry, gateway, bind);
    Ok(RegisteredMcpServer {
        server,
        executor,
        targets,
        sync,
    })
}

/// Re-synchronizes after a `tools/list_changed` notification (or any
/// explicit refresh): re-lists, registers/updates/uninstalls, rebinds the
/// executor's target map. A listing that fails on a dead transport
/// reconnects with a FRESH handshake exactly once (re-initialize +
/// re-list — never replaying calls). Old prepared handles are NOT
/// migrated — the T01/T02 generation gates refuse them.
pub async fn refresh_mcp_server(
    registered: &RegisteredMcpServer,
    registry: &Arc<ToolRegistry>,
    gateway: &ToolInvocationGateway,
    budget: &SchemaBudget,
) -> Result<McpSyncReport, McpBridgeError> {
    let mut sync = match sync_server_tools(&registered.server, registry, budget).await {
        Ok(sync) => sync,
        // A listing on a dead transport (e.g. the transport broke mid-call
        // in R04-A13) reconnects exactly once with a fresh handshake.
        Err(McpBridgeError::ConnectFailed { .. } | McpBridgeError::ServerGone) => {
            let (negotiated, identity) = registered.server.connect().await?;
            let mut retry = sync_server_tools(&registered.server, registry, budget).await?;
            retry.negotiated_protocol = negotiated;
            retry.server_identity = identity;
            retry
        }
        Err(e) => return Err(e),
    };
    if sync.negotiated_protocol.is_empty() {
        if let Some(conn) = registered.server.connection.lock().await.as_ref() {
            sync.negotiated_protocol = conn.negotiated_protocol.clone();
            sync.server_identity = conn.server_identity.clone();
        }
    }
    bind_all(&registered.server, &registered.executor, registry, gateway);
    Ok(sync)
}

// ── unit tests ───────────────────────────────────────────────────────────────

#[cfg(all(test, unix))]
#[path = "mcpbridge_request_tests.rs"]
mod request_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn macos_natural_exit_stop_reaps_real_children_within_original_cleanup_budget() {
        let mut observed_exit = 0;
        for _ in 0..64 {
            let mut command = tokio::process::Command::new("/usr/bin/true");
            command.process_group(0).kill_on_drop(true);
            let mut child = McpChild {
                process: command.spawn().unwrap(),
                killed: false,
            };
            let pid = child.process.id().unwrap();
            // 等真实内核退出可见，不先try_wait收取状态，保留原失败的退出/回收窗口。
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    if unsafe { libc::getpgid(pid as i32) } == -1
                        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            observed_exit += 1;
            child.stop().await.unwrap_or_else(|error| {
                panic!(
                    "自有自然退出未收尾：owned_pid={pid},error={}",
                    error.message()
                )
            });
            assert!(macos_process_group_absent(pid).unwrap());
            assert!(child.process.try_wait().unwrap().is_some());
        }
        assert_eq!(observed_exit, 64);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_exited_unreaped_child_is_confirmed_empty_without_hiding_live_groups() {
        let mut live = tokio::process::Command::new("/bin/sleep");
        live.arg("30").process_group(0).kill_on_drop(true);
        let mut live = McpChild {
            process: live.spawn().unwrap(),
            killed: false,
        };
        assert!(!macos_process_group_absent(live.process.id().unwrap()).unwrap());
        live.stop().await.unwrap();

        let mut exited = tokio::process::Command::new("/usr/bin/true");
        exited.process_group(0).kill_on_drop(true);
        let mut exited = McpChild {
            process: exited.spawn().unwrap(),
            killed: false,
        };
        let pid = exited.process.id().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if unsafe { libc::getpgid(pid as i32) } == -1
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        // 保留真实Mac边界：此时组内只有尚未reap的退出记录，并非活进程的权限失败。
        assert_eq!(unsafe { libc::killpg(pid as i32, libc::SIGKILL) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM)
        );
        assert!(!macos_process_group_absent(pid).unwrap());
        exited.stop().await.unwrap();
        assert!(macos_process_group_absent(pid).unwrap());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_stdio_request_cancel_deadline_and_future_drop_keep_shared_connection_usable() {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r07-mcp-request-cancel-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("external.mjs");
        let arrivals = root.join("arrivals.ndjson");
        std::fs::write(&script, r#"
import fs from 'node:fs';
import readline from 'node:readline';
const reply=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
readline.createInterface({input:process.stdin}).on('line',line=>{
 const m=JSON.parse(line);fs.appendFileSync(process.argv[2],JSON.stringify(m)+'\n');
 if(m.method==='initialize')reply(m.id,{protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'owned-request-cancel',version:'1'}});
 if(m.method==='tools/call'&&m.params.name==='echo')reply(m.id,{content:[{type:'text',text:'连接仍可使用'}]});
});
"#).unwrap();
        let server = McpServer::new(
            "request-cancel",
            McpEndpoint::Stdio {
                command: "node".into(),
                args: vec![
                    script.to_string_lossy().into(),
                    arrivals.to_string_lossy().into(),
                ],
                env: BTreeMap::new(),
                cwd: Some(root.clone()),
            },
        );
        server.connect().await.unwrap();
        async fn observed(path: &std::path::Path, method: &str, count: usize) {
            tokio::time::timeout(std::time::Duration::from_secs(3), async {
                loop {
                    let rows = std::fs::read_to_string(path).unwrap_or_default();
                    let actual = rows
                        .lines()
                        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                        .filter(|row| row["method"] == method)
                        .count();
                    if actual >= count {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("外部真实SDK请求或取消通知必须到达");
        }
        assert_eq!(
            server
                .call_raw("hold", serde_json::Map::new(), 80)
                .await
                .unwrap_err(),
            McpCallFailure::DeadlineExceeded
        );
        observed(&arrivals, "notifications/cancelled", 1).await;

        let cancel = crate::cancel::CancelScope::run_root("mcp-request");
        let lifetime = crate::toolgateway::TrustedToolLifetime {
            workspace: None,
            cancel: cancel.clone(),
            deadline_unix_ms: u64::MAX,
            budget_ms: 3000,
        };
        let caller = server.clone();
        let active = tokio::spawn(
            crate::toolgateway::TOOL_LIFETIME.scope(lifetime, async move {
                caller.call_raw("hold", serde_json::Map::new(), 3000).await
            }),
        );
        observed(&arrivals, "tools/call", 2).await;
        cancel.cancel("取消真实单次MCP请求");
        assert_eq!(
            active.await.unwrap().unwrap_err(),
            McpCallFailure::Cancelled
        );
        observed(&arrivals, "notifications/cancelled", 2).await;

        let caller = server.clone();
        let dropped =
            tokio::spawn(
                async move { caller.call_raw("hold", serde_json::Map::new(), 3000).await },
            );
        observed(&arrivals, "tools/call", 3).await;
        dropped.abort();
        assert!(dropped.await.unwrap_err().is_cancelled());
        observed(&arrivals, "notifications/cancelled", 3).await;
        assert!(server.is_connected(), "取消单个请求不能关闭共享连接");
        let echo = server
            .call_raw("echo", serde_json::Map::new(), 1000)
            .await
            .unwrap();
        assert!(!echo.content.is_empty());
        let records = std::fs::read_to_string(&arrivals)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let calls = records
            .iter()
            .filter(|row| row["method"] == "tools/call")
            .collect::<Vec<_>>();
        let notices = records
            .iter()
            .filter(|row| row["method"] == "notifications/cancelled")
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 4, "取消后不得重放任何旧调用");
        assert_eq!(notices.len(), 3);
        for (call, notice) in calls.iter().zip(notices.iter()) {
            assert_eq!(
                call["id"], notice["params"]["requestId"],
                "只能取消原请求编号"
            );
        }
        // 真实连接停止也必须先走该请求的官方取消，不能先使SDK失效。
        let caller = server.clone();
        let stopped_request =
            tokio::spawn(
                async move { caller.call_raw("hold", serde_json::Map::new(), 3000).await },
            );
        observed(&arrivals, "tools/call", 5).await;
        let old_lifetime = {
            let connection = server.connection.lock().await;
            connection.as_ref().unwrap().requests.clone()
        };
        server.begin_stop();
        {
            let connection = server.connection.lock().await;
            assert!(
                !connection.as_ref().unwrap().service.is_closed(),
                "封口不能抢先关闭仍需发取消的SDK"
            );
        }
        assert_eq!(
            server
                .call_raw("must-not-enter", serde_json::Map::new(), 1000)
                .await
                .unwrap_err(),
            McpCallFailure::Cancelled
        );
        assert_eq!(
            stopped_request.await.unwrap().unwrap_err(),
            McpCallFailure::Cancelled
        );
        observed(&arrivals, "notifications/cancelled", 4).await;
        server.disconnect_checked().await.unwrap();
        assert_eq!(old_lifetime.active.load(Ordering::Acquire), 0);
        let rows = std::fs::read_to_string(&arrivals).unwrap();
        assert!(!rows.contains("must-not-enter"), "封口后请求不得投递到外端");
        server.connect().await.unwrap();
        old_lifetime.begin_stop();
        assert!(
            !server
                .call_raw("echo", serde_json::Map::new(), 1000)
                .await
                .unwrap()
                .content
                .is_empty(),
            "旧连接收尾不得取消新连接请求"
        );
        server.disconnect_checked().await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mcp_target_ids_are_origin_namespaced() {
        let a = lingxi_kernel::toolcatalog::tool_target_id(
            &ToolOrigin::Mcp {
                server_id: "server-a".into(),
            },
            "echo",
        )
        .expect("id");
        let b = lingxi_kernel::toolcatalog::tool_target_id(
            &ToolOrigin::Mcp {
                server_id: "server-b".into(),
            },
            "echo",
        )
        .expect("id");
        assert_eq!(a.as_str(), "tool:mcp:server-a:echo");
        assert_eq!(b.as_str(), "tool:mcp:server-b:echo");
        assert_ne!(a, b, "two servers' same-named tools are distinct targets");
    }

    #[test]
    fn server_annotations_never_soften_the_permission_or_recovery() {
        let mut annotations = rmcp::model::ToolAnnotations::new();
        annotations.read_only_hint = Some(true);
        let mut tool = McpTool::new(
            "writable",
            "a tool that claims to be read-only",
            serde_json::Map::new(),
        );
        tool.annotations = Some(annotations);
        let manifest = manifest_for_tool("srv", &tool);
        assert_eq!(manifest.permission.kind, PermissionKind::Execute);
        assert_eq!(manifest.declared_permission, DeclaredPermission::ReadOnly);
        assert_eq!(manifest.recovery, ToolRecoveryCapability::CONSERVATIVE);
    }

    #[test]
    fn remote_resource_uris_never_map_to_local_refs() {
        let link = rmcp::model::ContentBlock::ResourceLink(rmcp::model::Resource::new(
            "file:///etc/passwd",
            "passwd",
        ));
        match map_content_block(link) {
            ContentBlock::Text { text } => {
                assert!(text.contains("remote URI"), "{text}");
            }
            other => panic!("remote content must map to text, got {other:?}"),
        }
        let embedded = rmcp::model::ContentBlock::Resource(rmcp::model::EmbeddedResource::new(
            rmcp::model::ResourceContents::text("le secrets", "file:///Users/x/secret.txt"),
        ));
        match map_content_block(embedded) {
            ContentBlock::Text { text } => {
                assert!(text.contains("remote URI"), "{text}");
            }
            other => panic!("remote content must map to text, got {other:?}"),
        }
    }

    #[test]
    fn structured_output_validation_uses_the_catalog_rules() {
        let schema = json!({
            "type": "object",
            "properties": {"count": {"type": "integer", "minimum": 0}},
            "required": ["count"],
            "additionalProperties": false
        });
        let budget = SchemaBudget::default();
        assert!(
            validate_structured_output(&schema, &json!({"count": 3}), &budget).is_ok(),
            "a conforming structured result passes"
        );
        let violations = validate_structured_output(&schema, &json!({"count": -1}), &budget)
            .expect_err("minimum violated");
        assert!(!violations.is_empty());
        assert!(
            validate_structured_output(&schema, &json!({"count": 1, "extra": true}), &budget)
                .is_err(),
            "additionalProperties:false is enforced"
        );
        assert!(
            validate_structured_output(&schema, &json!({}), &budget).is_err(),
            "missing required field is a violation"
        );
    }
}
