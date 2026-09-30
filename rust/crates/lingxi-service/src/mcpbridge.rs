//! R04-T07: MCP integration through the OFFICIAL `rmcp` 3.4.1 SDK
//! (R01 D-06 — no self-written MCP protocol stack).
//!
//! Scope (frozen by the incumbent surface + the taskbook):
//!
//! - **Transport**: the SDK's async duplex transport over any
//!   `AsyncRead + AsyncWrite`, including REAL child-process stdio servers
//!   spawned by this bridge with a whitelisted environment. HTTP/SSE
//!   transports need rmcp features that are NOT in the lock (D-06:
//!   R04/R07 按需开启并重测) — an `McpEndpoint::Unsupported` endpoint is
//!   an honest loud refusal, never a fallback.
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
//! Composition note (production default unchanged): like T04/T05/T06 the
//! production bootstrap does NOT call any `register_*` in this module;
//! the registration functions are composition entries for the R04 tool
//! surface and tests.

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
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ClientCapabilities, ClientConfig, Implementation,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, Tool as McpTool,
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
    /// A transport this build does not carry (HTTP/SSE need rmcp features
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
    #[allow(dead_code)]
    negotiated_protocol: String,
    #[allow(dead_code)]
    server_identity: String,
}

/// One configured MCP server: endpoint + live connection + the tool
/// targets currently registered from its listing.
pub struct McpServer {
    id: String,
    endpoint: McpEndpoint,
    connection: AsyncMutex<Option<McpConnection>>,
    /// target id -> server tool name for everything registered from the
    /// current listing (the authoritative target↔tool binding).
    registered: std::sync::Mutex<BTreeMap<String, String>>,
    /// target id -> the EXACT manifest the last sync registered/updated
    /// (change detection + the executor's output-schema snapshot).
    manifests: std::sync::Mutex<BTreeMap<String, ToolManifest>>,
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
}

impl McpBridgeError {
    pub fn code(&self) -> &'static str {
        match self {
            McpBridgeError::EndpointUnsupported { .. } => "mcp_endpoint_unsupported",
            McpBridgeError::ConnectFailed { .. } => "mcp_connect_failed",
            McpBridgeError::NegotiationMissing => "mcp_negotiation_missing",
            McpBridgeError::ServerGone => "mcp_server_gone",
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
        }
    }
}

impl McpServer {
    pub fn new(id: impl Into<String>, endpoint: McpEndpoint) -> Arc<Self> {
        Arc::new(Self {
            id: id.into(),
            endpoint,
            connection: AsyncMutex::new(None),
            registered: std::sync::Mutex::new(BTreeMap::new()),
            manifests: std::sync::Mutex::new(BTreeMap::new()),
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
            .map(|c| c.is_some())
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
            .stderr(Stdio::null());
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
        let mut guard = self.connection.lock().await;
        if let Some(old) = guard.take() {
            let _ = old.service.cancel().await;
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
                let mut child = self.spawn_stdio(command, args, env, cwd)?;
                let stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| McpBridgeError::ConnectFailed {
                        detail: "stdio child had no stdin".to_string(),
                    })?;
                let stdout = child
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
                // Reap the child detached: when the service cancels, the
                // ChildStdin half drops and stdin closes, terminating a
                // well-behaved stdio server; the wait() prevents zombies.
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                Self::finalize(service, guard, self.connect_count.as_ref())
            }
            McpEndpoint::Duplex { factory } => {
                let io = factory();
                let service = handler.serve((io.read, io.write)).await.map_err(|e| {
                    McpBridgeError::ConnectFailed {
                        detail: format!("{e}"),
                    }
                })?;
                Self::finalize(service, guard, self.connect_count.as_ref())
            }
            McpEndpoint::Unsupported { transport } => Err(McpBridgeError::EndpointUnsupported {
                transport: transport.clone(),
            }),
        }
    }

    fn finalize(
        service: BridgeClient,
        mut guard: AsyncMutexGuard<'_, Option<McpConnection>>,
        connect_count: &AtomicU64,
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
        *guard = Some(McpConnection {
            service,
            negotiated_protocol: negotiated.clone(),
            server_identity: identity.clone(),
        });
        Ok((negotiated, identity))
    }

    pub async fn disconnect(&self) {
        let mut guard = self.connection.lock().await;
        if let Some(old) = guard.take() {
            let _ = old.service.cancel().await;
        }
    }

    async fn list_tools(&self) -> Result<Vec<McpTool>, McpBridgeError> {
        let guard = self.connection.lock().await;
        let Some(conn) = guard.as_ref() else {
            return Err(McpBridgeError::ServerGone);
        };
        // Full paginated listing (tools/list until no cursor remains) —
        // BOUNDED: an untrusted server that always returns another cursor
        // or floods tools is refused loudly, never indulged.
        let mut tools = Vec::new();
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
            let page: ListToolsResult = conn
                .service
                .peer()
                .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                .await
                .map_err(|e| McpBridgeError::ConnectFailed {
                    detail: format!("list_tools: {e}"),
                })?;
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
    async fn call_raw(
        &self,
        tool: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
        deadline_ms: u64,
    ) -> Result<rmcp::model::CallToolResult, McpCallFailure> {
        let guard = self.connection.lock().await;
        let Some(conn) = guard.as_ref() else {
            return Err(McpCallFailure::NotConnected);
        };
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        let call = conn.service.peer().call_tool_once(params);
        match tokio::time::timeout(std::time::Duration::from_millis(deadline_ms), call).await {
            // Deadline expired: the request may have reached the server
            // and executed — the honest UNKNOWN classification (never a
            // blind retry, never a fabricated clean failure).
            Err(_elapsed) => Err(McpCallFailure::DeadlineExceeded),
            Ok(Err(transport_error)) => {
                Err(McpCallFailure::Transport(format!("{transport_error}")))
            }
            Ok(Ok(response)) => match response {
                CallToolResponse::Complete(result) => Ok(result),
                CallToolResponse::InputRequired(_) => {
                    Err(McpCallFailure::InputRequiredRoundRefused)
                }
                CallToolResponse::Task(_) => Err(McpCallFailure::TaskRoundRefused),
                _ => Err(McpCallFailure::TaskRoundRefused),
            },
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
    InputRequiredRoundRefused,
    TaskRoundRefused,
}

impl McpCallFailure {
    fn reason(&self) -> String {
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
    let bindings = executor_bindings(server, registry);
    let targets: Vec<ToolTargetId> = bindings
        .keys()
        .map(|key| ToolTargetId::parse(key))
        .collect();
    executor.bind_targets(bindings);
    for target in &targets {
        gateway.bind_executor(
            target.clone(),
            Arc::clone(executor) as Arc<dyn ToolExecutorPort>,
            "R04-T07 MCP bridge executor",
        );
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
    let (negotiated, identity) = server.connect().await?;
    let mut sync = sync_server_tools(&server, registry, budget).await?;
    sync.negotiated_protocol = negotiated;
    sync.server_identity = identity;
    let executor = McpToolExecutor::new(Arc::clone(&server), *budget);
    let targets = bind_all(&server, &executor, registry, gateway);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
