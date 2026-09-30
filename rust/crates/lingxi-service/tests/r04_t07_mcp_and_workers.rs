//! R04-T07 acceptance: MCP and plugin worker integration
//! (R04-A13 / R04-A14 + the adversarial additions).
//!
//! Everything here runs against the REAL composition surfaces: the REAL
//! `ToolRegistry`/`ToolInvocationGateway` (T01/T02), the REAL
//! ApprovalService policy face (T03), the REAL ResourceAccess (T04) and
//! — for the sandboxed worker leg — the REAL T06 seatbelt sandbox. The
//! MCP side speaks the OFFICIAL rmcp 3.4.1 SDK end to end: a synthetic
//! in-process server (an rmcp `ServerHandler` served over a breakable
//! duplex transport — REAL initialize handshake, REAL version
//! negotiation, REAL tools/list and tools/call) plus a REAL
//! child-process stdio server (the `r04_t07_fixture` bin) driven through
//! the SAME bridge spawn path production uses. Never a user-installed
//! MCP server, never an external network target.
//!
//! Test-double boundary: the synthetic MCP servers and the worker
//! fixtures are EXTERNAL systems under test (they produce the untrusted
//! inputs, including deliberately hostile ones); no double replaces the
//! bridge, the gateway, the policy, the registry, the journal or the
//! driver.
//!
//! Runtime note: the in-process rmcp handshake requires a multi-thread
//! tokio runtime (the production service runtime; single-thread test
//! runtimes starve rmcp's internal concurrent tasks into a premature
//! transport close) — hence the explicit `multi_thread` flavor below.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolExecutionResult,
    ToolOutcome, ToolRequest, TurnProviderPort,
};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetId};
use lingxi_kernel::Principal;
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, ToolCallId};
use lingxi_service::approval_service::ApprovalService;
use lingxi_service::mcpbridge::{
    breakable_duplex, refresh_mcp_server, register_mcp_server, McpEndpoint, McpServer,
};
use lingxi_service::toolgateway::ToolInvocationGateway;
use lingxi_service::workerrpc::{
    register_worker_tool, UnconfiguredWorkerModel, WorkerLimits, WorkerModelPort, WorkerRuntime,
    WorkerToolSpec,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult, ServerCapabilities,
    ServerConfig, Tool as McpTool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

fn fixture_exe() -> &'static str {
    env!("CARGO_BIN_EXE_r04_t07_fixture")
}

fn unique_dir(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "r04t07-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).expect("test dir");
    base
}

fn unique_suffix() -> String {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    )
}

// ── the synthetic in-process MCP server (an rmcp ServerHandler) ────────────

fn rmcp_tool_of(spec: &SyntheticTool) -> McpTool {
    let mut annotations = rmcp::model::ToolAnnotations::new();
    annotations.read_only_hint = Some(spec.read_only_hint);
    let mut tool = McpTool::new(spec.name.clone(), spec.description, serde_json::Map::new());
    let schema_map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_value(spec.schema.clone()).expect("schema is an object");
    tool.input_schema = Arc::new(schema_map);
    if let Some(output) = spec.output_schema.clone() {
        let output_map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(output).expect("output schema is an object");
        tool.output_schema = Some(Arc::new(output_map));
    }
    tool.annotations = Some(annotations);
    tool
}

struct SyntheticTool {
    name: String,
    description: &'static str,
    schema: serde_json::Value,
    output_schema: Option<serde_json::Value>,
    read_only_hint: bool,
}

#[derive(Clone)]
enum CallMode {
    /// Reply normally.
    Normal,
    /// Execute the side effect, SIGNAL the harness, then WAIT for the
    /// harness gate — the harness trips the transport break while the
    /// reply is parked, so the receipt never arrives intact.
    BreakBeforeReceipt,
}

struct SyntheticMcpServer {
    identity: String,
    /// SHARED listing — the harness mutates it to simulate a server-side
    /// tools/list change; the server reads it at list_tools time.
    tools: Arc<Mutex<Vec<SyntheticTool>>>,
    /// The side-effect counter that SURVIVES reconnects (the "remote
    /// server" state shared across server instances).
    counter: Arc<AtomicUsize>,
    /// How many tools/call requests this server received in total.
    received_calls: Arc<AtomicUsize>,
    /// SHARED paging knob (read at list_tools time).
    page_size: Arc<Mutex<Option<usize>>>,
    /// SHARED endless-paging knob: when set, every list page returns a
    /// next cursor — an untrusted server trying to pin the host in an
    /// endless paging walk.
    endless_paging: Arc<AtomicBool>,
    /// SHARED call-mode knob (read at call time).
    mode: Arc<Mutex<CallMode>>,
    executed_signal: mpsc::Sender<()>,
    /// The gate the BreakBeforeReceipt mode parks on (per connection).
    release_gate: Mutex<Option<oneshot::Receiver<()>>>,
}

impl ServerHandler for SyntheticMcpServer {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        let mut imp = Implementation::from_build_env();
        imp.name = self.identity.clone();
        imp.version = "1.0.0".into();
        info.server_info = imp;
        info
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, rmcp::ErrorData>> + Send + '_ {
        let name = request.name.to_string();
        let arguments = request
            .arguments
            .as_ref()
            .map(|a| serde_json::to_value(a).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null);
        let counter = Arc::clone(&self.counter);
        let received = Arc::clone(&self.received_calls);
        let mode = self.mode.lock().unwrap().clone();
        let executed = self.executed_signal.clone();
        let gate = self.release_gate.lock().unwrap().take();
        async move {
            received.fetch_add(1, Ordering::SeqCst);
            let ok_text = |text: String| {
                Ok(CallToolResponse::Complete(
                    rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::text(
                        text,
                    )]),
                ))
            };
            match name.as_str() {
                "count_up" => {
                    // The side effect happens exactly once per RECEIVED
                    // request; the receipt is what can get lost.
                    let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    if let CallMode::BreakBeforeReceipt = mode {
                        let _ = executed.send(()).await;
                        if let Some(gate) = gate {
                            let _ = gate.await;
                        }
                    }
                    ok_text(format!("count={n}"))
                }
                "get_count" => ok_text(format!("count={}", counter.load(Ordering::SeqCst))),
                "echo" => ok_text(format!(
                    "echo:{}",
                    arguments.get("text").and_then(|t| t.as_str()).unwrap_or("")
                )),
                "boom" => {
                    let mut result = rmcp::model::CallToolResult::success(vec![
                        rmcp::model::ContentBlock::text("the upstream exploded"),
                    ]);
                    result.is_error = Some(true);
                    Ok(CallToolResponse::Complete(result))
                }
                "huge" => ok_text("x".repeat(300 * 1024)),
                "bad_output" => {
                    let mut result = rmcp::model::CallToolResult::success(vec![
                        rmcp::model::ContentBlock::text("structured payload violates the schema"),
                    ]);
                    result.structured_content = Some(json!({"count": -5}));
                    Ok(CallToolResponse::Complete(result))
                }
                "forge_file" => Ok(CallToolResponse::Complete(
                    rmcp::model::CallToolResult::success(vec![
                        rmcp::model::ContentBlock::ResourceLink(rmcp::model::Resource::new(
                            "file:///etc/passwd",
                            "passwd",
                        )),
                    ]),
                )),
                _ => Err(rmcp::ErrorData::method_not_found::<
                    rmcp::model::CallToolRequestMethod,
                >()),
            }
        }
    }

    fn list_tools(
        &self,
        request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_ {
        let cursor = request
            .and_then(|p| p.cursor)
            .unwrap_or_else(|| "start".to_string());
        let page_size = *self.page_size.lock().unwrap();
        let endless = self.endless_paging.load(Ordering::SeqCst);
        let tools: Vec<McpTool> = self
            .tools
            .lock()
            .unwrap()
            .iter()
            .map(rmcp_tool_of)
            .collect();
        async move {
            // The cursor is the numeric offset of the next page ("start"
            // for the first request) — a deterministic walk where every
            // page's cursor names exactly where the next page begins.
            let (slice, next_cursor) = match page_size {
                None => (tools, None),
                Some(size) => {
                    let offset: Option<usize> = match cursor.as_str() {
                        "start" => Some(0),
                        raw => raw.parse().ok(),
                    };
                    match offset {
                        Some(offset) if offset < tools.len() => {
                            let end = (offset + size).min(tools.len());
                            let next = if end < tools.len() {
                                Some(end.to_string())
                            } else {
                                None
                            };
                            (tools[offset..end].to_vec(), next)
                        }
                        _ => (Vec::new(), None),
                    }
                }
            };
            let mut result = ListToolsResult::with_all_items(slice);
            if endless {
                result.next_cursor = Some("0".to_string());
            } else {
                result.next_cursor = next_cursor;
            }
            Ok(result)
        }
    }
}

// ── the MCP harness (shared server state across reconnects) ────────────────

struct McpHarness {
    server: Arc<McpServer>,
    counter: Arc<AtomicUsize>,
    received_calls: Arc<AtomicUsize>,
    listing: Arc<Mutex<Vec<SyntheticTool>>>,
    mode: Arc<Mutex<CallMode>>,
    /// The break switch of the CURRENT transport pair.
    break_switch: Arc<Mutex<Arc<AtomicBool>>>,
    /// The release gate of the CURRENT server instance.
    release_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    executed_rx: tokio::sync::Mutex<mpsc::Receiver<()>>,
    page_size: Arc<Mutex<Option<usize>>>,
    endless_paging: Arc<AtomicBool>,
}

impl McpHarness {
    fn default_listing() -> Vec<SyntheticTool> {
        let obj_schema = || json!({"type":"object","properties":{},"additionalProperties":false});
        vec![
            SyntheticTool {
                name: "count_up".into(),
                description: "increment the server-side counter (the side effect)",
                schema: obj_schema(),
                output_schema: None,
                read_only_hint: false,
            },
            SyntheticTool {
                name: "get_count".into(),
                description: "read the server-side counter (verified status query)",
                schema: obj_schema(),
                output_schema: None,
                read_only_hint: true,
            },
            SyntheticTool {
                name: "echo".into(),
                description: "echo back a marker",
                schema: json!({
                    "type":"object",
                    "properties":{"text":{"type":"string"},"mode":{"type":"string"}},
                    "required":["text"],
                    "additionalProperties":false
                }),
                output_schema: None,
                read_only_hint: true,
            },
            SyntheticTool {
                name: "boom".into(),
                description: "return an is_error result",
                schema: obj_schema(),
                output_schema: None,
                read_only_hint: false,
            },
            SyntheticTool {
                name: "huge".into(),
                description: "return an oversized text result",
                schema: obj_schema(),
                output_schema: None,
                read_only_hint: true,
            },
            SyntheticTool {
                name: "bad_output".into(),
                description: "structured result violating the declared output schema",
                schema: obj_schema(),
                output_schema: Some(json!({
                    "type":"object",
                    "properties":{"count":{"type":"integer","minimum":0}},
                    "required":["count"],
                    "additionalProperties":false
                })),
                read_only_hint: true,
            },
            SyntheticTool {
                name: "forge_file".into(),
                description: "claim a local file resource link",
                schema: obj_schema(),
                output_schema: None,
                read_only_hint: true,
            },
        ]
    }

    fn new(server_id: &str) -> McpHarness {
        Self::with_listing(server_id, Self::default_listing())
    }

    fn with_listing(server_id: &str, listing: Vec<SyntheticTool>) -> McpHarness {
        let (executed_tx, executed_rx) = mpsc::channel(4);
        let counter = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(AtomicUsize::new(0));
        let listing = Arc::new(Mutex::new(listing));
        let mode = Arc::new(Mutex::new(CallMode::Normal));
        let page_size: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
        let endless_paging = Arc::new(AtomicBool::new(false));
        let break_switch: Arc<Mutex<Arc<AtomicBool>>> =
            Arc::new(Mutex::new(Arc::new(AtomicBool::new(false))));
        let release_tx: Arc<Mutex<Option<oneshot::Sender<()>>>> = Arc::new(Mutex::new(None));
        let identity = format!("lingxi-synthetic-mcp-{server_id}");

        let factory = {
            let counter = Arc::clone(&counter);
            let received = Arc::clone(&received);
            let listing = Arc::clone(&listing);
            let mode = Arc::clone(&mode);
            let page_size = Arc::clone(&page_size);
            let endless_paging = Arc::clone(&endless_paging);
            let break_switch = Arc::clone(&break_switch);
            let release_tx = Arc::clone(&release_tx);
            let executed_tx = executed_tx.clone();
            let identity = identity.clone();
            Arc::new(move || {
                let (client_io, peer_io, broken) = breakable_duplex(64 * 1024);
                *break_switch.lock().unwrap() = Arc::clone(&broken);
                let (gate_tx, gate_rx) = oneshot::channel();
                *release_tx.lock().unwrap() = Some(gate_tx);
                let server = SyntheticMcpServer {
                    identity: identity.clone(),
                    tools: Arc::clone(&listing),
                    counter: Arc::clone(&counter),
                    received_calls: Arc::clone(&received),
                    page_size: Arc::clone(&page_size),
                    endless_paging: Arc::clone(&endless_paging),
                    mode: Arc::clone(&mode),
                    executed_signal: executed_tx.clone(),
                    release_gate: Mutex::new(Some(gate_rx)),
                };
                tokio::spawn(async move {
                    // HOLD the service for the connection's lifetime: a
                    // dropped RunningService closes the session
                    // asynchronously (rmcp's drop guard) — the server
                    // would die right after initialize. `waiting` parks
                    // until the transport ends (client cancel / break).
                    if let Ok(service) = server.serve((peer_io.read, peer_io.write)).await {
                        let _ = service.waiting().await;
                    }
                });
                client_io
            })
        };
        McpHarness {
            server: McpServer::new(server_id, McpEndpoint::Duplex { factory }),
            counter,
            received_calls: received,
            listing,
            mode,
            break_switch,
            release_tx,
            executed_rx: tokio::sync::Mutex::new(executed_rx),
            page_size,
            endless_paging,
        }
    }

    /// Trips the CURRENT transport's break switch and releases the parked
    /// server reply (it writes into a dead transport).
    fn trip_break(&self) {
        self.break_switch
            .lock()
            .unwrap()
            .store(true, Ordering::SeqCst);
        if let Some(tx) = self.release_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}

// ── the service-side harness ────────────────────────────────────────────────

struct StepsProvider {
    steps: Mutex<VecDeque<ProviderTurn>>,
}

impl TurnProviderPort for StepsProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        _turn: u32,
        _input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let next = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| final_turn("done"));
        let ctx_at_issue = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, next) })
    }
}

fn final_turn(text: &str) -> ProviderTurn {
    ProviderTurn::Final {
        message: NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            model_call_id: None,
        },
    }
}

fn tool_turn(target: &str, args: serde_json::Value) -> ProviderTurn {
    ProviderTurn::ToolRequests {
        requests: vec![
            ToolRequest::from_effective_arguments(target, args, &budget())
                .expect("effective request"),
        ],
    }
}

struct Harness {
    state: ServiceState,
    gateway: Arc<ToolInvocationGateway>,
    registry: Arc<ToolRegistry>,
    access: Arc<lingxi_service::ResourceAccess>,
    approvals: Arc<ApprovalService>,
    ws: PathBuf,
    restricted: PathBuf,
    provider: Arc<StepsProvider>,
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
        web_session_id: None,
        credential_id: None,
    }
}

fn user_operate() -> lingxi_service::toolgateway::InvocationPermissionContext {
    lingxi_service::toolgateway::InvocationPermissionContext::UserSession {
        mode: SessionPermissionMode::Operate,
    }
}

async fn service_harness(root: &Path) -> Harness {
    let ws = root.join("ws");
    let restricted = root.join("restricted");
    std::fs::create_dir_all(&ws).expect("ws");
    std::fs::create_dir_all(&restricted).expect("restricted");
    let home_dir = unique_dir("home");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home_dir.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home_dir).expect("layout");
    let registry = Arc::new(ToolRegistry::new());
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let approvals = Arc::new(ApprovalService::new(Arc::new(
        lingxi_service::inject::SystemClock,
    )));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(lingxi_service::inject::SystemClock),
        budget(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    let provider = Arc::new(StepsProvider {
        steps: Mutex::new(VecDeque::new()),
    });
    let deps = ServiceDeps {
        turn_provider: Some(Arc::clone(&provider) as Arc<dyn TurnProviderPort>),
        tool_gateway: Some(Arc::clone(&gateway)),
        approval_gate: Some(
            Arc::clone(&approvals) as Arc<dyn lingxi_service::approval::ApprovalGate>
        ),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    Harness {
        state,
        gateway,
        registry,
        access,
        approvals,
        ws,
        restricted,
        provider,
    }
}

/// A direct two-phase gateway call (prepare → execute) for one target.
async fn gateway_call(
    h: &Harness,
    target: &str,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, lingxi_service::toolgateway::GatewayRefusal> {
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_t07"),
        run_id: lingxi_protocol::RunId::new(format!("run-{}", unique_suffix())),
        attempt: lingxi_protocol::AttemptId::new("a-1"),
        generation: 1,
    };
    let request =
        ToolRequest::from_effective_arguments(target, args, &budget()).expect("effective request");
    let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
    let prepared = h.gateway.prepare_from_request(
        &ctx,
        lingxi_service::toolgateway::CallerSurface::UserRun,
        "agent",
        user_operate(),
        &call_id,
        &request,
    )?;
    h.gateway
        .execute_prepared(&ctx, &call_id, &prepared.handle)
        .await
}

/// The stable machine code the boundary attaches to a failed outcome
/// (`details.code`) — the vocabulary tests pin.
fn code_of(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Failed { error } => error
            .details
            .as_ref()
            .and_then(|d| d.get("code"))
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn text_of(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        ToolOutcome::Failed { error } => format!("<failed: {}>", error.message),
        other => format!("<{other:?}>"),
    }
}

/// A stand-in EXTERNAL model gateway (the R05 provider face): answers
/// plainly and carries no credential material. The HOST budget logic in
/// front of it is the code under test.
struct OkModel;
impl WorkerModelPort for OkModel {
    fn complete(
        &self,
        _ctx: &RunContext,
        _worker: &str,
        _request: &lingxi_service::workerrpc::WorkerModelRequest,
    ) -> Result<
        lingxi_service::workerrpc::WorkerModelReply,
        lingxi_service::workerrpc::WorkerModelRefusal,
    > {
        Ok(lingxi_service::workerrpc::WorkerModelReply {
            text: "model-ok".to_string(),
        })
    }
}

// ── worker registration helper ──────────────────────────────────────────────

async fn register_worker(
    h: &Harness,
    runtime: &Arc<WorkerRuntime>,
    model: Arc<dyn WorkerModelPort>,
    mode: &str,
    extra: &str,
) -> ToolTargetId {
    let input = h.ws.join("input.txt");
    std::fs::write(&input, "granted-content\n").expect("input file");
    let local_name = format!("t07w_{mode}_{}", unique_suffix());
    // No empty argv ELEMENTS: the T06 sandbox refuses any argv containing
    // an empty string (a config-invalid refusal), so the optional `extra`
    // parameter is omitted entirely when empty.
    let mut argv = vec![fixture_exe().to_string(), mode.to_string()];
    if !extra.is_empty() {
        argv.push(extra.to_string());
    }
    let spec = WorkerToolSpec {
        plugin_id: "t07worker".to_string(),
        op: "probe".to_string(),
        local_name: local_name.clone(),
        description: format!("T07 synthetic worker ({mode})"),
        input_schema: json!({
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
            "additionalProperties": false
        }),
        path_args: vec!["input".to_string()],
        argv,
        env: BTreeMap::new(),
        cwd: h.ws.clone(),
        model,
    };
    register_worker_tool(
        &h.registry,
        h.gateway.as_ref(),
        runtime,
        &h.access,
        &budget(),
        spec,
    )
    .expect("worker tool registers")
    .target
}

// ── R04-A13 ─────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r04_a13_mcp_reconnect_never_replays_the_side_effect() {
    let root = unique_dir("a13");
    let h = service_harness(&root).await;
    let mcp = McpHarness::new("counter");

    let registered = register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("register");
    assert!(
        registered.sync.refused.is_empty(),
        "the default listing registers without refusals: {:?}",
        registered.sync.refused
    );
    // The handshake REALLY negotiated a protocol version with the real
    // server identity round-tripping.
    assert!(
        registered.sync.negotiated_protocol.starts_with("20"),
        "negotiated={:?}",
        registered.sync.negotiated_protocol
    );
    assert_eq!(
        registered.sync.server_identity,
        "lingxi-synthetic-mcp-counter"
    );

    // Arm the receipt-loss scenario: the server executes the counting
    // action, signals us, then parks its reply; we kill the transport
    // before the receipt can arrive.
    *mcp.mode.lock().unwrap() = CallMode::BreakBeforeReceipt;
    let call = {
        let gateway = Arc::clone(&h.gateway);
        let target = "tool:mcp:counter:count_up".to_string();
        tokio::spawn(async move {
            let ctx = RunContext {
                principal: Principal::LocalUser,
                session_id: lingxi_protocol::SessionId::new("sess_t07"),
                run_id: lingxi_protocol::RunId::new(format!("run-{}", unique_suffix())),
                attempt: lingxi_protocol::AttemptId::new("a-1"),
                generation: 1,
            };
            let request = ToolRequest::from_effective_arguments(&target, json!({}), &budget())
                .expect("effective request");
            let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
            let prepared = gateway
                .prepare_from_request(
                    &ctx,
                    lingxi_service::toolgateway::CallerSurface::UserRun,
                    "agent",
                    user_operate(),
                    &call_id,
                    &request,
                )
                .expect("prepare");
            gateway
                .execute_prepared(&ctx, &call_id, &prepared.handle)
                .await
                .expect("execute")
        })
    };
    mcp.executed_rx
        .lock()
        .await
        .recv()
        .await
        .expect("the server-side side effect signal");
    mcp.trip_break();
    let outcome = call.await.expect("call task joins");

    match &outcome.outcome {
        ToolOutcome::Unknown { reason } => {
            assert!(reason.contains("never blindly retried"), "{reason}");
        }
        other => panic!("expected Unknown after receipt loss, got {other:?}"),
    }
    // The side effect happened EXACTLY once and the server received
    // EXACTLY one call — no automatic replay happened.
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1);
    assert_eq!(mcp.received_calls.load(Ordering::SeqCst), 1);

    // Reconnect + re-list through the real refresh path: the listing on
    // the dead transport triggers exactly ONE fresh handshake. Never a
    // replay of the counting call.
    let refreshed = refresh_mcp_server(&registered, &h.registry, h.gateway.as_ref(), &budget())
        .await
        .expect("refresh reconnects");
    assert_eq!(mcp.server.connect_count(), 2, "exactly one reconnect");
    assert!(
        refreshed.negotiated_protocol.starts_with("20"),
        "the reconnect negotiated a real protocol: {:?}",
        refreshed.negotiated_protocol
    );
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1, "no blind re-send");
    assert_eq!(mcp.received_calls.load(Ordering::SeqCst), 1);

    // The VERIFIED STATUS QUERY (a read-only tool on the same server)
    // reconciles honestly: the count is one.
    let status = gateway_call(&h, "tool:mcp:counter:get_count", json!({}))
        .await
        .expect("status query");
    assert!(text_of(&status).contains("count=1"), "{}", text_of(&status));
    // Still exactly one counting action ever.
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1);
    assert_eq!(mcp.received_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r04_a13_full_chain_journals_unknown_and_a_new_decision_sees_the_real_count() {
    let root = unique_dir("a13-chain");
    let h = service_harness(&root).await;
    let mcp = McpHarness::new("chain");
    let registered = register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("register");
    assert_eq!(registered.sync.registered, 7);

    // First run: the counting call whose receipt is lost. A concurrent
    // task trips the transport as soon as the side effect lands.
    *mcp.mode.lock().unwrap() = CallMode::BreakBeforeReceipt;
    h.provider
        .steps
        .lock()
        .unwrap()
        .push_back(tool_turn("tool:mcp:chain:count_up", json!({})));
    let principal = owner_principal();
    h.state
        .sessions()
        .set_permission_mode_for(
            &principal,
            "sess_local_alpha",
            SessionPermissionMode::Operate,
        )
        .await
        .expect("operate mode");
    let trip = {
        let counter = Arc::clone(&mcp.counter);
        let switch = Arc::clone(&mcp.break_switch);
        let gate = Arc::clone(&mcp.release_tx);
        tokio::spawn(async move {
            loop {
                if counter.load(Ordering::SeqCst) == 1 {
                    switch.lock().unwrap().store(true, Ordering::SeqCst);
                    if let Some(tx) = gate.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    return;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
    };

    let run = h
        .state
        .sessions()
        .execute_for(
            h.state.storage().as_ref(),
            h.state.events(),
            h.state.runs(),
            &principal,
            "sess_local_alpha",
            "A13 chain: counting call with a lost receipt",
            1_000,
        )
        .await
        .expect("run settles");
    let _ = trip.await;

    // The journal entry for the counting call is UNKNOWN — never a
    // fabricated failure, never a success.
    let journal = h
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run.run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1, "{journal:?}");
    assert_eq!(
        journal[0].receipt.as_ref().expect("receipt").outcome,
        lingxi_kernel::ports::ReceiptOutcome::Unknown,
        "{journal:?}"
    );

    // RECONNECT through the real recovery path: the listing on the dead
    // transport triggers exactly one fresh handshake — no call replay.
    let refreshed = refresh_mcp_server(&registered, &h.registry, h.gateway.as_ref(), &budget())
        .await
        .expect("refresh reconnects");
    assert_eq!(mcp.server.connect_count(), 2, "exactly one reconnect");
    assert!(
        refreshed.negotiated_protocol.starts_with("20"),
        "{:?}",
        refreshed.negotiated_protocol
    );
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1, "no blind re-send");
    assert_eq!(mcp.received_calls.load(Ordering::SeqCst), 1);

    // A NEW explicit decision (a second run) reads the verified status
    // and sees exactly ONE counting action — no blind re-send happened.
    h.provider
        .steps
        .lock()
        .unwrap()
        .push_back(tool_turn("tool:mcp:chain:get_count", json!({})));
    let second = h
        .state
        .sessions()
        .execute_for(
            h.state.storage().as_ref(),
            h.state.events(),
            h.state.runs(),
            &principal,
            "sess_local_alpha",
            "A13 chain: verified status query",
            1_000,
        )
        .await
        .expect("second run settles");
    let journal2 = h
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(second.run_id.clone()))
        .await
        .expect("journal 2");
    assert_eq!(
        journal2[0].receipt.as_ref().expect("receipt").outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1);
    assert_eq!(mcp.received_calls.load(Ordering::SeqCst), 2);
}

// ── R04-A14 ─────────────────────────────────────────────────────────────────

/// Planting a host secret in the process environment is PROCESS-GLOBAL
/// state and cargo runs tests in parallel threads — so each env-probe
/// test plants its OWN uniquely-named secret (A14 → A, the stdio test →
/// B) and asserts only its own: no shared set/remove window to race,
/// and a failing assertion cannot leak its variable into the other
/// test's probe either.
static HOST_SECRET_A: &str = "LINGXI_T07_SECRET_A";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r04_a14_worker_cannot_bypass_grants_or_model_credentials() {
    let root = unique_dir("a14");
    let h = service_harness(&root).await;
    // Plant a HOST secret in the service process environment — a worker
    // that inherits (or is handed) credentials would see it.
    std::env::set_var(HOST_SECRET_A, "host-secret-value-t07");

    let model: Arc<dyn WorkerModelPort> = Arc::new(UnconfiguredWorkerModel);
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );

    // (1) Model credentials: the worker asks the host for the model; the
    // REAL host port refuses with capability-not-configured and hands
    // over NO credential material.
    let ask = register_worker(&h, &runtime, Arc::clone(&model), "ask_credentials", "").await;
    let result = gateway_call(&h, ask.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    let text = text_of(&result);
    assert!(text.contains("model_capability_not_configured"), "{text}");
    assert!(!text.contains("host-secret-value-t07"), "{text}");

    // (2) A file argument OUTSIDE the workspace: the REAL resource layer
    // refuses the grant — the worker process is never spawned and the
    // restricted tree is untouched.
    let outside = h.restricted.join("secret.txt");
    std::fs::write(&outside, "RESTRICTED-SENTINEL").expect("sentinel");
    let probe = register_worker(&h, &runtime, Arc::clone(&model), "ok", "").await;
    let refused = gateway_call(
        &h,
        probe.as_str(),
        json!({"input": "../restricted/secret.txt"}),
    )
    .await
    .expect("call");
    match &refused.outcome {
        ToolOutcome::Failed { error } => {
            assert!(
                error.message.contains("worker grant refused"),
                "{}",
                error.message
            );
        }
        other => panic!("expected grant refusal, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&outside).expect("sentinel"),
        "RESTRICTED-SENTINEL",
        "zero side effects on the restricted tree"
    );

    // (3) The worker CLAIMS a file outside its grant: the claim is
    // refused and no local resource reference is minted.
    let claim = register_worker(
        &h,
        &runtime,
        Arc::clone(&model),
        "claim_outside",
        outside.display().to_string().as_str(),
    )
    .await;
    let claimed = gateway_call(&h, claim.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    match &claimed.outcome {
        ToolOutcome::Failed { error } => {
            assert_eq!(
                code_of(&claimed),
                "worker_claimed_unauthorized_path",
                "{}",
                error.message
            );
        }
        other => panic!("expected claimed-path refusal, got {other:?}"),
    }

    // (4) Secrets never transit: the worker's environment has no host
    // secret and no HOME; only the whitelist reached it.
    let env_probe = register_worker(&h, &runtime, Arc::clone(&model), "env_probe", "").await;
    let probe_result = gateway_call(&h, env_probe.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    let text = text_of(&probe_result);
    assert!(text.contains("\"LINGXI_T07_SECRET_A\":false"), "{text}");
    assert!(text.contains("\"HOME\":false"), "{text}");
    assert!(text.contains("\"PATH\":true"), "{text}");

    // (5) The MAIN SERVICE remains fully usable: a legitimate worker
    // invocation succeeds through the same gateway afterwards.
    let ok_target = register_worker(&h, &runtime, model, "ok", "").await;
    let ok_result = gateway_call(&h, ok_target.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    assert!(
        text_of(&ok_result).contains("worked:probe"),
        "{}",
        text_of(&ok_result)
    );

    std::env::remove_var(HOST_SECRET_A);
}

// ── adversarial: MCP ────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_two_servers_same_tool_name_hit_their_own_targets() {
    let root = unique_dir("adv-same-name");
    let h = service_harness(&root).await;
    let a = McpHarness::new("alpha");
    let b = McpHarness::new("beta");
    let _ra = register_mcp_server(
        Arc::clone(&a.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("alpha registers");
    let _rb = register_mcp_server(
        Arc::clone(&b.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("beta registers");
    // Both servers list get_count; neither overwrote the other.
    assert!(h
        .registry
        .describe(&ToolTargetId::parse("tool:mcp:alpha:get_count"))
        .is_ok());
    assert!(h
        .registry
        .describe(&ToolTargetId::parse("tool:mcp:beta:get_count"))
        .is_ok());
    // Calling by exact target id hits EXACTLY that server.
    let ra = gateway_call(&h, "tool:mcp:alpha:get_count", json!({}))
        .await
        .expect("alpha call");
    assert!(text_of(&ra).contains("count=0"));
    assert_eq!(a.received_calls.load(Ordering::SeqCst), 1);
    assert_eq!(b.received_calls.load(Ordering::SeqCst), 0);
    let rb = gateway_call(&h, "tool:mcp:beta:get_count", json!({}))
        .await
        .expect("beta call");
    assert!(text_of(&rb).contains("count=0"));
    assert_eq!(b.received_calls.load(Ordering::SeqCst), 1);
    assert_eq!(a.received_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_tools_list_change_updates_generation_and_invalidates_old_shapes() {
    let root = unique_dir("adv-list-change");
    let h = service_harness(&root).await;
    let mcp = McpHarness::new("listing");
    let registered = register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("register");
    let echo = "tool:mcp:listing:echo";
    let generation_before = h.registry.catalog_generation();

    // The server's listing CHANGES: echo's schema gains a required field.
    {
        let mut listing = mcp.listing.lock().unwrap();
        for tool in listing.iter_mut() {
            if tool.name == "echo" {
                tool.schema = json!({
                    "type": "object",
                    "properties": {"text": {"type": "string"}, "mode": {"type": "string"}},
                    "required": ["text", "mode"],
                    "additionalProperties": false
                });
            }
        }
    }
    let refresh = refresh_mcp_server(&registered, &h.registry, h.gateway.as_ref(), &budget())
        .await
        .expect("refresh");
    assert_eq!(refresh.updated, 1, "{refresh:?}");
    assert!(
        refresh.generation_after > generation_before,
        "the catalog generation moved: {refresh:?}"
    );

    // An OLD-shape call (no `mode`) is refused by the CURRENT schema —
    // the old contract cannot keep executing against the new listing.
    let stale = gateway_call(&h, echo, json!({"text": "hi"})).await;
    match stale {
        Err(lingxi_service::toolgateway::GatewayRefusal::ArgumentsInvalid { .. }) => {}
        other => panic!("expected schema refusal for the stale shape, got {other:?}"),
    }
    // The NEW shape works.
    let fresh = gateway_call(&h, echo, json!({"text": "hi", "mode": "plain"}))
        .await
        .expect("new shape call");
    assert!(text_of(&fresh).contains("echo:hi"), "{}", text_of(&fresh));

    // A tool VANISHING from the listing: the target is uninstalled and
    // stops resolving.
    {
        let mut listing = mcp.listing.lock().unwrap();
        listing.retain(|t| t.name != "boom");
    }
    let removal = refresh_mcp_server(&registered, &h.registry, h.gateway.as_ref(), &budget())
        .await
        .expect("refresh after removal");
    assert_eq!(removal.removed, 1, "{removal:?}");
    assert!(h
        .registry
        .describe(&ToolTargetId::parse("tool:mcp:listing:boom"))
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_error_oversized_schema_violating_and_forged_results() {
    let root = unique_dir("adv-results");
    let h = service_harness(&root).await;
    let mcp = McpHarness::new("results");
    let registered = register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("register");
    // Shrink the result cap so the 300KiB fixture trips it deterministically.
    registered.executor.set_result_cap(8 * 1024);

    // A server error result IS a receipt: a completed-but-failed call.
    let boom = gateway_call(&h, "tool:mcp:results:boom", json!({}))
        .await
        .expect("call");
    match &boom.outcome {
        ToolOutcome::Failed { error } => {
            assert!(
                error.message.contains("mcp_tool_error"),
                "{}",
                error.message
            );
            assert!(
                error.message.contains("the upstream exploded"),
                "{}",
                error.message
            );
        }
        other => panic!("expected failed receipt, got {other:?}"),
    }

    // Oversized output: truncated=true with an explicit truncation note,
    // never a silent cut.
    let huge = gateway_call(&h, "tool:mcp:results:huge", json!({}))
        .await
        .expect("call");
    match &huge.outcome {
        ToolOutcome::Success { result } => {
            assert!(result.truncated, "the huge result is truncated");
            let text = text_of(&huge);
            assert!(text.contains("exceeded"), "{text}");
            assert!(text.len() < 32 * 1024, "content is bounded: {}", text.len());
        }
        other => panic!("expected truncated success, got {other:?}"),
    }

    // Structured result violating the tool's declared output schema: the
    // call FAILS — untrusted results are never force-fit.
    let bad = gateway_call(&h, "tool:mcp:results:bad_output", json!({}))
        .await
        .expect("call");
    match &bad.outcome {
        ToolOutcome::Failed { error } => {
            assert!(
                error
                    .message
                    .contains("mcp_structured_result_violates_schema"),
                "{}",
                error.message
            );
        }
        other => panic!("expected schema-violation failure, got {other:?}"),
    }

    // A forged local file link: the remote claim becomes TEXT, never a
    // local ResourceRef.
    let forge = gateway_call(&h, "tool:mcp:results:forge_file", json!({}))
        .await
        .expect("call");
    match &forge.outcome {
        ToolOutcome::Success { result } => {
            assert!(
                result.resource_refs.is_empty(),
                "{:?}",
                result.resource_refs
            );
            let text = text_of(&forge);
            assert!(text.contains("remote URI"), "{text}");
            assert!(text.contains("file:///etc/passwd"), "{text}");
        }
        other => panic!("expected success with text-only content, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_mcp_pagination_walks_all_pages() {
    let root = unique_dir("adv-paging");
    let h = service_harness(&root).await;
    let listing: Vec<SyntheticTool> = ["p1", "p2", "p3", "p4", "p5"]
        .iter()
        .map(|name| SyntheticTool {
            name: name.to_string(),
            description: "paged tool",
            schema: json!({"type":"object","properties":{},"additionalProperties":false}),
            output_schema: None,
            read_only_hint: true,
        })
        .collect();
    let mcp = McpHarness::with_listing("paged", listing);
    *mcp.page_size.lock().unwrap() = Some(2);
    let registered = register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("register");
    assert_eq!(registered.sync.registered, 5, "every page was walked");
    for name in ["p1", "p2", "p3", "p4", "p5"] {
        let target = format!("tool:mcp:paged:{name}");
        assert!(
            h.registry.describe(&ToolTargetId::parse(&target)).is_ok(),
            "{target} registered from a paged listing"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_unsupported_endpoint_refuses_loudly() {
    let root = unique_dir("adv-unsupported");
    let h = service_harness(&root).await;
    let server = McpServer::new(
        "remote",
        McpEndpoint::Unsupported {
            transport: "streamable-http".to_string(),
        },
    );
    let err = match register_mcp_server(
        Arc::clone(&server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    {
        Ok(_) => panic!("an unsupported endpoint must refuse to register"),
        Err(e) => e,
    };
    assert!(
        matches!(
            err,
            lingxi_service::mcpbridge::McpBridgeError::EndpointUnsupported { .. }
        ),
        "{err:?}"
    );
    assert!(
        err.message().contains("refusing instead of falling back"),
        "{}",
        err.message()
    );
    // Nothing registered from the refused endpoint (a fresh registry sits
    // at its initial generation).
    assert_eq!(h.registry.catalog_generation(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_stdio_child_transport_and_env_whitelist_hold() {
    let root = unique_dir("stdio");
    let h = service_harness(&root).await;
    std::env::set_var("LINGXI_T07_SECRET_B", "host-secret-value-t07");
    let server = McpServer::new(
        "stdiofx",
        McpEndpoint::Stdio {
            command: fixture_exe().to_string(),
            args: vec!["--mcp-stdio-server".to_string()],
            env: BTreeMap::new(),
            cwd: Some(h.ws.clone()),
        },
    );
    let registered = register_mcp_server(
        Arc::clone(&server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("stdio server registers");
    assert_eq!(registered.sync.server_identity, "lingxi-stdio-fixture");
    // A real tools/call over the REAL child stdio transport: the child
    // reports its environment — the host secret must be absent.
    let result = gateway_call(&h, "tool:mcp:stdiofx:env_report", json!({}))
        .await
        .expect("stdio call");
    let text = text_of(&result);
    assert!(text.contains("\"LINGXI_T07_SECRET_B\":false"), "{text}");
    assert!(text.contains("\"HOME\":false"), "{text}");
    assert!(text.contains("\"PATH\":true"), "{text}");
    assert_eq!(server.connect_count(), 1);
    std::env::remove_var("LINGXI_T07_SECRET_B");
}

// ── adversarial: worker ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_worker_protocol_violations_are_refused() {
    let root = unique_dir("adv-worker-violations");
    let h = service_harness(&root).await;
    let model: Arc<dyn WorkerModelPort> = Arc::new(UnconfiguredWorkerModel);
    let limits = WorkerLimits {
        max_line_bytes: 16 * 1024, // the huge_line fixture writes 64KiB
        ..WorkerLimits::default()
    };
    let runtime = WorkerRuntime::new(limits, None, Arc::new(lingxi_service::inject::SystemClock));

    for (mode, needle) in [
        ("bad_id", "does not match the invocation id"),
        ("huge_line", "exceeds the"),
        ("malformed", "malformed line"),
        ("unknown_kind", "unexpected line kind"),
    ] {
        let target = register_worker(&h, &runtime, Arc::clone(&model), mode, "").await;
        let result = gateway_call(&h, target.as_str(), json!({"input": "input.txt"}))
            .await
            .expect("call");
        match &result.outcome {
            ToolOutcome::Failed { error } => {
                assert_eq!(
                    code_of(&result),
                    "worker_protocol_violation",
                    "{mode}: {}",
                    error.message
                );
                assert!(error.message.contains(needle), "{mode}: {}", error.message);
            }
            other => panic!("{mode}: expected protocol violation, got {other:?}"),
        }
        // The main service is still usable after each violation.
        let ok_target = register_worker(&h, &runtime, Arc::clone(&model), "ok", "").await;
        let ok = gateway_call(&h, ok_target.as_str(), json!({"input": "input.txt"}))
            .await
            .expect("ok call");
        assert!(text_of(&ok).contains("worked:probe"), "{mode}");
    }

    // A worker sending TWO result lines gets exactly ONE credit: the
    // FIRST accepted result settles the invocation and the worker is
    // killed right after — the second line cannot credit anything
    // (single-credit semantics; cross-call ticket reuse is the bad_id
    // leg above — the host-minted CSPRNG id never matches a stale one).
    let double = register_worker(&h, &runtime, Arc::clone(&model), "double_result", "").await;
    let result = gateway_call(&h, double.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    match &result.outcome {
        ToolOutcome::Success { result } => {
            let text = result
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            assert_eq!(text, "first", "exactly the first result, once");
        }
        other => panic!("double_result: expected single-credit success, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_worker_deadline_and_silent_exit_are_unknown_never_clean_failures() {
    let root = unique_dir("adv-worker-unknown");
    let h = service_harness(&root).await;
    let model: Arc<dyn WorkerModelPort> = Arc::new(UnconfiguredWorkerModel);
    let limits = WorkerLimits {
        deadline_ms: 700, // the hang fixture sleeps 60s
        ..WorkerLimits::default()
    };
    let runtime = WorkerRuntime::new(limits, None, Arc::new(lingxi_service::inject::SystemClock));

    // (a) A hanging worker: deadline → bounded kill → UNKNOWN (side
    // effects unconfirmed), never a fabricated clean failure.
    let hang = register_worker(&h, &runtime, Arc::clone(&model), "hang", "").await;
    let started = std::time::Instant::now();
    let result = gateway_call(&h, hang.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    match &result.outcome {
        ToolOutcome::Unknown { reason } => {
            assert!(reason.contains("side effects unconfirmed"), "{reason}");
        }
        other => panic!("expected unknown, got {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the deadline path is bounded"
    );

    // (b) A worker exiting without a result: UNKNOWN, same honesty.
    let early = register_worker(&h, &runtime, Arc::clone(&model), "exit_early", "").await;
    let result = gateway_call(&h, early.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    match &result.outcome {
        ToolOutcome::Unknown { reason } => {
            assert!(reason.contains("unconfirmed"), "{reason}");
        }
        other => panic!("expected unknown, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_worker_error_result_and_callback_budget_are_host_owned() {
    let root = unique_dir("adv-worker-budget");
    let h = service_harness(&root).await;
    let model: Arc<dyn WorkerModelPort> = Arc::new(UnconfiguredWorkerModel);
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );

    // A worker error is a completed-but-failed call (not a protocol
    // violation, not unknown).
    let err = register_worker(&h, &runtime, Arc::clone(&model), "error_result", "").await;
    let result = gateway_call(&h, err.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    match &result.outcome {
        ToolOutcome::Failed { error } => {
            assert_eq!(code_of(&result), "worker_error", "{}", error.message);
            assert!(
                error.message.contains("could not parse"),
                "{}",
                error.message
            );
        }
        other => panic!("expected worker error, got {other:?}"),
    }

    // A callback storm: the host's model budget (BoundedWorkerModel,
    // cap 2, with an inner gateway that answers) REFUSES callbacks 3..6
    // host-side with budget codes — the worker sees refusal codes, never
    // credentials; the 2 budgeted calls complete; the invocation still
    // completes with its report.
    let ok_model = Arc::new(OkModel);
    let bounded = lingxi_service::workerrpc::BoundedWorkerModel::new(
        Some(Arc::clone(&ok_model) as Arc<dyn WorkerModelPort>),
        2,
        512,
    );
    let storm_limits = WorkerLimits {
        max_callbacks: 6,
        ..WorkerLimits::default()
    };
    let storm_runtime = WorkerRuntime::new(
        storm_limits,
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );
    let storm = register_worker(
        &h,
        &storm_runtime,
        bounded as Arc<dyn WorkerModelPort>,
        "callback_storm",
        "",
    )
    .await;
    let result = gateway_call(&h, storm.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    let text = text_of(&result);
    assert!(text.contains("refusals=4"), "{}", text);
    assert!(
        text.contains("oks=2"),
        "the two budgeted calls completed: {text}"
    );
    assert!(!text.contains("host-secret"), "{}", text);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_claim_in_grant_mints_a_verified_resource_ref() {
    let root = unique_dir("worker-claim-ok");
    let h = service_harness(&root).await;
    let model: Arc<dyn WorkerModelPort> = Arc::new(UnconfiguredWorkerModel);
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );
    let target = register_worker(&h, &runtime, model, "claim_ok", "").await;
    let result = gateway_call(&h, target.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    match &result.outcome {
        ToolOutcome::Success { result } => {
            assert_eq!(result.resource_refs.len(), 1, "{:?}", result.resource_refs);
            let uri = result.resource_refs[0].uri.as_deref().expect("uri");
            assert!(uri.starts_with("file://"), "{uri}");
            assert!(uri.contains("input.txt"), "{uri}");
        }
        other => panic!("expected success with a verified ref, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_concurrency_is_bounded_by_the_runtime_semaphore() {
    let root = unique_dir("worker-concurrency");
    let h = service_harness(&root).await;
    let model: Arc<dyn WorkerModelPort> = Arc::new(UnconfiguredWorkerModel);
    let limits = WorkerLimits {
        max_concurrent_workers: 1,
        ..WorkerLimits::default()
    };
    let runtime = WorkerRuntime::new(limits, None, Arc::new(lingxi_service::inject::SystemClock));
    let target = register_worker(&h, &runtime, model, "slow_ok", "").await;

    // Two invocations with a 400ms worker each: the semaphore (cap 1)
    // serializes them, so the pair takes at least ~800ms.
    let started = std::time::Instant::now();
    let (a, b) = tokio::join!(
        gateway_call(&h, target.as_str(), json!({"input": "input.txt"})),
        gateway_call(&h, target.as_str(), json!({"input": "input.txt"})),
    );
    let _ = a.expect("first");
    let _ = b.expect("second");
    assert!(
        started.elapsed() >= Duration::from_millis(780),
        "serialized: {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_sandbox_binding_denies_out_of_boundary_writes() {
    let root = unique_dir("worker-sandbox");
    let h = service_harness(&root).await;
    let home = unique_dir("sandbox-home");
    let agent_dir = home.join("agent");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    // The escape target: a file OUTSIDE the sandbox's writable roots. The
    // parent lives under cargo's CARGO_TARGET_TMPDIR — deliberately NOT
    // the OS $TMPDIR (the frozen sandbox contract legitimately allows
    // writes there as temporary resources, so rooting the escape target
    // under $TMPDIR would make the denial probe degenerate — the same
    // lesson T06's own harness recorded). It does NOT exist yet: if the
    // sandbox fails, the hostile write creates it and the test fails.
    let escape_parent = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "r04t07-escape-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&escape_parent).expect("escape parent");
    let escape = escape_parent.join("sandbox-escape.txt");

    // The REAL platform sandbox (macOS: seatbelt behind the pinned system
    // helper) with the workspace as the only writable root and network
    // denied — the same composition register_process_tools uses. The
    // frozen T06 policy confines WRITES to the writable roots (reads
    // outside the roots remain allowed by that same policy — a write is
    // the honest denial probe).
    let policy = lingxi_service::sandbox::SandboxPolicy::derive(
        &lingxi_service::sandbox::SandboxPolicyInput {
            lingxi_home: home.clone(),
            agent_dir: agent_dir.clone(),
            workspace_roots: vec![h.ws.clone()],
            runtime_writable_paths: vec![],
            network: lingxi_service::sandbox::SandboxNetworkPolicy::Denied,
        },
    )
    .expect("policy derives");
    let sandbox =
        lingxi_service::sandbox::platform_sandbox(policy, None).expect("platform sandbox");

    let model: Arc<dyn WorkerModelPort> = Arc::new(UnconfiguredWorkerModel);
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        Some(sandbox),
        Arc::new(lingxi_service::inject::SystemClock),
    );
    let target = register_worker(
        &h,
        &runtime,
        model,
        "write_outside",
        escape.display().to_string().as_str(),
    )
    .await;
    let result = gateway_call(&h, target.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("call");
    let text = text_of(&result);
    assert!(text.contains("WRITE_DENIED"), "{text}");
    assert!(
        !escape.exists(),
        "the out-of-boundary write never landed on disk"
    );

    // Allow contrast: the sandboxed worker still runs and answers INSIDE
    // the boundary (the denial is the boundary, not a total break).
    let ok_target =
        register_worker(&h, &runtime, Arc::new(UnconfiguredWorkerModel), "ok", "").await;
    let ok = gateway_call(&h, ok_target.as_str(), json!({"input": "input.txt"}))
        .await
        .expect("ok call");
    assert!(text_of(&ok).contains("worked:probe"), "{}", text_of(&ok));
}

// ── the full-chain happy path (structured ToolSuccess flows to the driver) ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_tool_success_flows_through_the_full_run_chain() {
    let root = unique_dir("mcp-happy");
    let h = service_harness(&root).await;
    let mcp = McpHarness::new("happy");
    let registered = register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("register");
    assert_eq!(registered.sync.registered, 7);
    h.provider
        .steps
        .lock()
        .unwrap()
        .push_back(tool_turn("tool:mcp:happy:get_count", json!({})));
    let principal = owner_principal();
    h.state
        .sessions()
        .set_permission_mode_for(
            &principal,
            "sess_local_alpha",
            SessionPermissionMode::Operate,
        )
        .await
        .expect("operate mode");
    let run = h
        .state
        .sessions()
        .execute_for(
            h.state.storage().as_ref(),
            h.state.events(),
            h.state.runs(),
            &principal,
            "sess_local_alpha",
            "T07: get_count round trip",
            1_000,
        )
        .await
        .expect("run completes");
    let journal = h
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run.run_id.clone()))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1);
    let receipt = journal[0].receipt.as_ref().expect("receipt");
    assert_eq!(
        receipt.outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );
    assert!(receipt.dispatched, "the call really dispatched");
    // The JOURNAL receipt carries the audit digest (the content lives in
    // the wire event — T01's receipt contract: dedup = derived content
    // digest; the raw text never lands in the journal detail).
    assert!(
        receipt.detail.contains("external content digest"),
        "{receipt:?}"
    );
    assert!(receipt.dedup_id.is_some(), "{receipt:?}");

    // The structured CONTENT is durable in the tool_call_completed wire
    // event (the payload R05 consumers read): the MCP text is there
    // verbatim.
    let payload = h
        .state
        .storage()
        .query_one_text(
            "SELECT payload_json FROM key_events WHERE run_id = ?1 \
             AND event_type = 'tool_call_completed'",
            vec![run.run_id.clone()],
        )
        .await
        .expect("event query")
        .expect("the tool_call_completed event exists");
    assert!(
        payload.contains("count="),
        "the structured content reached the durable wire event: {payload}"
    );
}

// ── adversarial (E02 completion): the untrusted-server paging bound ────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_endless_paging_is_refused_loudly() {
    let root = unique_dir("adv-endless-paging");
    let h = service_harness(&root).await;
    let mcp = McpHarness::new("endless");
    // An untrusted server that ALWAYS returns another cursor: the bridge's
    // listing walk must hit its page bound and refuse loudly — the
    // registration fails and NOTHING from that server is registered.
    mcp.endless_paging.store(true, Ordering::SeqCst);
    let err = match register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    {
        Ok(_) => panic!("an endless-paging server must be refused"),
        Err(e) => e,
    };
    assert!(
        matches!(
            err,
            lingxi_service::mcpbridge::McpBridgeError::ConnectFailed { .. }
        ),
        "{err:?}"
    );
    assert!(err.message().contains("page bound"), "{}", err.message());
    assert!(
        h.registry
            .describe(&ToolTargetId::parse("tool:mcp:endless:get_count"))
            .is_err(),
        "nothing registered from the refused listing"
    );
}

// ── adversarial (E02 completion): cancelled call + late response ────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_call_never_consumes_a_late_response() {
    let root = unique_dir("adv-cancel-late");
    let h = service_harness(&root).await;
    let mcp = McpHarness::new("late");
    let _registered = register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("register");

    // The server executes the side effect, signals, then PARKS its reply.
    *mcp.mode.lock().unwrap() = CallMode::BreakBeforeReceipt;
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess_t07"),
        run_id: lingxi_protocol::RunId::new(format!("run-{}", unique_suffix())),
        attempt: lingxi_protocol::AttemptId::new("a-1"),
        generation: 1,
    };
    let request =
        ToolRequest::from_effective_arguments("tool:mcp:late:count_up", json!({}), &budget())
            .expect("effective request");
    let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
    let prepared = h
        .gateway
        .prepare_from_request(
            &ctx,
            lingxi_service::toolgateway::CallerSurface::UserRun,
            "agent",
            user_operate(),
            &call_id,
            &request,
        )
        .expect("prepare");
    let handle = prepared.handle.clone();

    // The in-flight execution is CANCELLED (the CALL-level abort the run
    // driver performs) after the side effect landed but before any receipt.
    let execute = {
        let gateway = Arc::clone(&h.gateway);
        let ctx = ctx.clone();
        let call_id = call_id.clone();
        let handle = handle.clone();
        tokio::spawn(async move {
            gateway
                .execute_prepared(&ctx, &call_id, &handle)
                .await
                .expect("execute")
        })
    };
    mcp.executed_rx
        .lock()
        .await
        .recv()
        .await
        .expect("the side-effect signal");
    execute.abort();

    // The server's reply now arrives LATE — into a STILL-LIVE transport
    // (no break): the caller is already gone.
    if let Some(tx) = mcp.release_tx.lock().unwrap().take() {
        let _ = tx.send(());
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The cancelled execution never produced an outcome; the late
    // response credited nothing and resurrected nothing.
    let joined = execute.await.expect_err("the cancelled task joins");
    assert!(joined.is_cancelled());
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1);
    assert_eq!(mcp.received_calls.load(Ordering::SeqCst), 1);

    // The spent handle is single-use: replaying it gains nothing (the
    // late response cannot re-credit a consumed ticket).
    match h.gateway.execute_prepared(&ctx, &call_id, &handle).await {
        Err(
            lingxi_service::toolgateway::GatewayRefusal::PreparedHandleConsumed { .. }
            | lingxi_service::toolgateway::GatewayRefusal::PreparedHandleUnknown { .. },
        ) => {}
        other => panic!("replaying a spent handle must be refused, got {other:?}"),
    }
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1);
    assert_eq!(mcp.received_calls.load(Ordering::SeqCst), 1);

    // An honest NEW decision reconciles through the verified status query.
    let status = gateway_call(&h, "tool:mcp:late:get_count", json!({}))
        .await
        .expect("status query");
    assert!(text_of(&status).contains("count=1"), "{}", text_of(&status));
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1);
}

// ── adversarial (E02 completion): the T03 permission face owns T07 tools ────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_and_worker_tools_follow_the_t03_permission_face() {
    let root = unique_dir("adv-perm-face");
    let h = service_harness(&root).await;
    let mcp = McpHarness::new("perm");
    let _registered = register_mcp_server(
        Arc::clone(&mcp.server),
        &h.registry,
        h.gateway.as_ref(),
        &budget(),
    )
    .await
    .expect("register");
    let model: Arc<dyn WorkerModelPort> = Arc::new(UnconfiguredWorkerModel);
    let runtime = WorkerRuntime::new(
        WorkerLimits::default(),
        None,
        Arc::new(lingxi_service::inject::SystemClock),
    );
    let worker_target = register_worker(&h, &runtime, Arc::clone(&model), "ok", "").await;

    let mode_of = |mode: SessionPermissionMode| {
        lingxi_service::toolgateway::InvocationPermissionContext::UserSession { mode }
    };
    use lingxi_service::toolgateway::PolicyVerdict;

    #[derive(Debug, PartialEq)]
    enum Verdict {
        Allowed,
        NeedsApproval,
        Denied,
    }

    let verdict_of = |target: &str,
                      args: serde_json::Value,
                      permission|
     -> Result<Verdict, lingxi_service::toolgateway::GatewayRefusal> {
        let ctx = RunContext {
            principal: Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("sess_t07"),
            run_id: lingxi_protocol::RunId::new(format!("run-{}", unique_suffix())),
            attempt: lingxi_protocol::AttemptId::new("a-1"),
            generation: 1,
        };
        let request =
            ToolRequest::from_effective_arguments(target, args, &budget()).expect("request");
        let call_id = ToolCallId::new(format!("call-{}", unique_suffix()));
        h.gateway
            .prepare_from_request(
                &ctx,
                lingxi_service::toolgateway::CallerSurface::UserRun,
                "agent",
                permission,
                &call_id,
                &request,
            )
            .map(|prepared| match prepared.policy {
                PolicyVerdict::Allowed => Verdict::Allowed,
                PolicyVerdict::NeedsApproval { .. } => Verdict::NeedsApproval,
                PolicyVerdict::Denied { .. } => Verdict::Denied,
            })
    };

    // The MCP counting tool is an EXECUTE-class target: the T03 face says
    // operate→Allowed, ask→NeedsApproval, read_only→Denied (a policy
    // denial REFUSES the prepare outright — zero-dispatch) — the remote
    // server's own read-only HINT (count_up has none, get_count does)
    // grants nothing either way.
    for target in ["tool:mcp:perm:count_up", "tool:mcp:perm:get_count"] {
        assert_eq!(
            verdict_of(target, json!({}), mode_of(SessionPermissionMode::Operate)),
            Ok(Verdict::Allowed),
            "{target}: operate allows"
        );
        assert_eq!(
            verdict_of(target, json!({}), mode_of(SessionPermissionMode::Ask)),
            Ok(Verdict::NeedsApproval),
            "{target}: ask needs approval"
        );
        match verdict_of(target, json!({}), mode_of(SessionPermissionMode::ReadOnly)) {
            Err(lingxi_service::toolgateway::GatewayRefusal::PolicyDenied { code, .. }) => {
                assert!(
                    code.contains("ACTION_BLOCKED_BY_READ_ONLY"),
                    "{target}: {code}"
                );
            }
            other => panic!("{target}: read_only must deny the prepare, got {other:?}"),
        }
    }
    // The worker tool follows the same face.
    assert_eq!(
        verdict_of(
            worker_target.as_str(),
            json!({"input": "input.txt"}),
            mode_of(SessionPermissionMode::Operate)
        ),
        Ok(Verdict::Allowed)
    );
    assert_eq!(
        verdict_of(
            worker_target.as_str(),
            json!({"input": "input.txt"}),
            mode_of(SessionPermissionMode::Ask)
        ),
        Ok(Verdict::NeedsApproval)
    );

    // Full-chain leg: an ASK session's run on the MCP tool PARKS at the
    // real approval surface; the user approves; the call executes exactly
    // once — the T03 face owns the dispatch decision end to end.
    let principal = owner_principal();
    h.state
        .sessions()
        .set_permission_mode_for(&principal, "sess_local_alpha", SessionPermissionMode::Ask)
        .await
        .expect("ask mode");
    h.provider
        .steps
        .lock()
        .unwrap()
        .push_back(tool_turn("tool:mcp:perm:count_up", json!({})));
    let run_handle = {
        let state = h.state.clone();
        let principal = principal.clone();
        tokio::spawn(async move {
            state
                .sessions()
                .execute_for(
                    state.storage().as_ref(),
                    state.events(),
                    state.runs(),
                    &principal,
                    "sess_local_alpha",
                    "perm face: ask-session counting call",
                    1_000,
                )
                .await
                .expect("run settles")
                .run_id
        })
    };
    // Wait for the pending approval (the T03 record for the MCP target).
    let pending = {
        let ctx = RunContext {
            principal: Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("sess_local_alpha"),
            run_id: lingxi_protocol::RunId::new("pending-probe"),
            attempt: lingxi_protocol::AttemptId::new("a-1"),
            generation: 1,
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let matches: Vec<_> = h
                .approvals
                .pending_of(&ctx, "sess_local_alpha")
                .into_iter()
                .filter(|view| view.target == "tool:mcp:perm:count_up")
                .collect();
            if matches.len() == 1 {
                break matches[0].clone();
            }
            if matches.len() > 1 {
                panic!("one pending expected, saw {}", matches.len());
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the ask run never parked at the approval surface"
            );
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    };
    assert_eq!(
        pending.args_summary.as_deref(),
        Some("{}"),
        "count_up takes no arguments: the pending view is the SHAPE-only \
         summary (keys and type labels, never values)"
    );
    let answered = h.approvals.answer(
        &RunContext {
            principal: Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("sess_local_alpha"),
            run_id: lingxi_protocol::RunId::new("answerer"),
            attempt: lingxi_protocol::AttemptId::new("a-1"),
            generation: 1,
        },
        "sess_local_alpha",
        &pending.approval_id,
        lingxi_service::approval_service::Answer::Approve,
    );
    assert!(answered.settled(), "{answered:?}");
    let run_id = run_handle.await.expect("run completes");

    let journal = h
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id))
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1);
    let receipt = journal[0].receipt.as_ref().expect("receipt");
    assert_eq!(
        receipt.outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );
    assert!(receipt.dispatched);
    assert_eq!(mcp.counter.load(Ordering::SeqCst), 1);
    assert_eq!(mcp.received_calls.load(Ordering::SeqCst), 1);
}
