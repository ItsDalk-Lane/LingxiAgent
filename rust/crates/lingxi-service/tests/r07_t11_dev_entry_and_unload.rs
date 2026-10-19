//! R07-T11：开发入口授权矩阵（A21）与热卸载旧句柄失效（A22）。
//! A21：旧面 /api/plugins/dev* 开发入口在原生服务上不存在——任何身份调用
//! 都被响亮拒绝，无任何工具执行副作用。
//! A22：MCP 连接器经真实 HTTP 停用/移除后，缓存的批准句柄再次调用被网关
//! 拒绝（代次/目标失效），不会继续执行旧对象。
use lingxi_kernel::RunContext;
use lingxi_protocol::{AttemptId, RunId, SessionId, ToolCallId};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

fn unique_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r07-t11-{tag}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn config_for(home: &std::path::Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Env,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

struct Server {
    addr: SocketAddr,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
    state: ServiceState,
    home: PathBuf,
}

impl Server {
    /// 裸 TCP HTTP 调用（与 r07_t10_observability 同一已证形状）：
    /// token 为 None 时不带凭证。
    async fn request(
        &self,
        token: Option<&str>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        let mut stream = TcpStream::connect(self.addr).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut buf)).await;
        let header_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("http header end");
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let text = String::from_utf8_lossy(&buf[header_end + 4..]).to_string();
        (status, serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw": text})))
    }

    async fn local(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.request(Some(&self.token.clone()), method, path, body).await
    }

    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
        self.state.storage().close().await.expect("close storage");
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

async fn boot(tag: &str, with_gateway: bool) -> Server {
    boot_at(unique_dir(tag), with_gateway).await
}

async fn boot_at(home: PathBuf, with_gateway: bool) -> Server {
    std::fs::create_dir_all(home.join("plugin-data/mcp")).expect("mcp dir");
    std::fs::write(
        home.join("plugin-data/mcp/config.json"),
        json!({"schemaVersion":1,"global":{"mcp":{"enabled":true,"servers":[]}},"agents":{},"sessions":{}}).to_string(),
    )
    .expect("mcp config");
    let layout = prepare_layout(&home).expect("layout");
    let deps = if with_gateway {
        // 与 r04_rr1_f03_stop_honesty 同一已证形状：注册表+批准服务+网关三件套。
        let registry = Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new());
        let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
            Arc::new(lingxi_service::inject::SystemClock),
        ));
        let gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
            registry,
            Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
            Arc::new(lingxi_service::inject::SystemClock),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
            lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
        ));
        ServiceDeps {
            tool_gateway: Some(gateway),
            approval_gate: Some(Arc::clone(&approvals) as Arc<dyn lingxi_service::approval::ApprovalGate>),
            ..ServiceDeps::default()
        }
    } else {
        ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let serving = state.clone();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            serving,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = ready_rx.await.expect("readiness");
    Server { addr, token, stop: stop_tx, handle, state, home }
}

/// A21：开发入口矩阵——无凭证/远端设备凭证/本地所有者三种身份 × 旧面
/// dev 路径，全部响亮拒绝且不产生任何工具执行。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r07_t11_dev_entry_matrix_rejects_every_caller() {
    let server = boot("dev-matrix", false).await;

    // 远端设备凭证（chat scope）：普通远端客户端身份。
    let (status, body) = server
        .local(
            "POST",
            "/lingxi/v1/devices/credentials",
            Some(json!({"userId":"user_remote","scopes":["chat"]})),
        )
        .await;
    assert_eq!(status, 201, "issue device credential: {body}");
    let remote_secret = body["secret"].as_str().expect("secret").to_string();

    let dev_paths = [
        "/api/plugins/dev",
        "/api/plugins/dev/exec",
        "/api/plugins/dev/tools/call",
    ];
    for path in dev_paths {
        // 无凭证：鉴权层响亮 401。
        let (status, _) = server
            .request(None, "POST", path, Some(json!({"tool":"any","args":{}})))
            .await;
        assert_eq!(status, 401, "no credential must be rejected at auth: {path}");
        // 远端设备凭证：鉴权层以 local_owner_required 响亮 403（/api/* 未注册
        // 路径按管理面默认仅所有者），鉴权先于路由，永远到不了 404。
        let (status, _) = server
            .request(Some(&remote_secret), "POST", path, Some(json!({"tool":"any","args":{}})))
            .await;
        assert_eq!(status, 403, "remote client rejected at authz layer: {path}");
        // 本地所有者同样 404：Rust 面没有开发入口，本地身份也没有旁路。
        let (status, _) = server
            .local("POST", path, Some(json!({"tool":"any","args":{}})))
            .await;
        assert_eq!(status, 404, "local owner has no dev bypass either: {path}");
    }

    server.stop().await;
}

/// A22：热卸载使旧句柄失效——缓存的批准句柄在连接器移除后调用即拒绝，
/// 同名工具也不能再解析准备。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r07_t11_unload_invalidates_pinned_handles() {
    // R07 网关目录收紧：显式助手必须真实存在——先按已证夹具形状登记。
    let home = unique_dir("unload");
    std::fs::create_dir_all(home.join("agents/agent")).expect("agent dir");
    std::fs::write(
        home.join("agents/agent/config.yaml"),
        "agent:\n  name: agent\nmemory:\n  enabled: false\n",
    )
    .expect("agent config");
    let server = boot_at(home, true).await;
    let gateway = server
        .state
        .tool_gateway()
        .expect("gateway injected")
        .clone();

    // 真实 HTTP 添加并启动 stdio fixture 连接器（载荷形状对齐 r07_t08 已证模式）。
    let fixture = json!({
        "id":"fixture","name":"fixture","transport":"stdio",
        "command":env!("CARGO_BIN_EXE_r04_t07_fixture"),
        "args":["--mcp-stdio-server"],
        "env":{"LINGXI_T07_SECRET_A":"fixture-private-env"}
    });
    let (status, body) = server
        .local("POST", "/lingxi/v1/mcp/settings-action", Some(json!({"action":"mcp.connector.add","payload":{"connector":fixture,"startIfEnabled":true},"agentId":null})))
        .await;
    assert_eq!(status, 200, "add connector: {body}");
    let (status, body) = server
        .local("POST", "/lingxi/v1/mcp/settings-action", Some(json!({"action":"mcp.connector.start","payload":{"connectorId":"fixture"}})))
        .await;
    assert_eq!(status, 200, "start connector: {body}");

    // 等连接器真实就绪：状态面不再有 connecting，工具进入注册表。
    let target = lingxi_kernel::toolcatalog::ToolTargetId::parse("tool:mcp:fixture:env_report");
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let (status, state) = server
                .local("GET", "/lingxi/v1/mcp/state", None)
                .await;
            assert_eq!(status, 200, "{state}");
            let connecting = state["config"]["connectors"]
                .as_array()
                .map(|rows| rows.iter().any(|row| row["status"] == "connecting"))
                .unwrap_or(false);
            if !connecting && gateway.registry().describe(&target).is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("connector must settle within 15s");

    // 正向对照：工具经真实网关准备+执行成功（证明拒绝断言有基础）。
    let prepare = |nonce: &str| {
        let ctx = RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: SessionId::new("sess-t11"),
            run_id: RunId::new(format!("run-{nonce}")),
            attempt: AttemptId::new("a1"),
            generation: 1,
        };
        let call = ToolCallId::new(format!("call-{nonce}"));
        let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
            "tool:mcp:fixture:env_report",
            json!({}),
            &lingxi_kernel::toolcatalog::SchemaBudget::default(),
        )
        .expect("request");
        let prepared = gateway
            .prepare_from_request(
                &ctx,
                lingxi_service::toolgateway::CallerSurface::UserRun,
                "agent",
                lingxi_service::toolgateway::InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                &call,
                &request,
            )
            .expect("prepare before unload");
        (ctx, call, prepared)
    };

    let (ctx0, call0, prepared0) = prepare("before-remove");
    let result = gateway
        .execute_prepared(&ctx0, &call0, &prepared0.handle)
        .await
        .expect("execute before unload succeeds through the real chain");
    assert!(
        matches!(result.outcome, lingxi_kernel::ports::ToolOutcome::Success { .. }),
        "fixture executes before unload: {:?}",
        result.outcome
    );

    // 缓存第二个句柄（卸载前固定代次）。
    let (ctx1, call1, prepared1) = prepare("pinned");

    // 真实 HTTP 移除连接器（卸载）。
    let (status, body) = server
        .local("POST", "/lingxi/v1/mcp/settings-action", Some(json!({"action":"mcp.connector.remove","payload":{"connectorId":"fixture"}})))
        .await;
    assert_eq!(status, 200, "remove connector: {body}");

    // 等卸载真实生效：工具从注册表撤销（对齐 r07_t08 的撤销观测面）。
    tokio::time::timeout(Duration::from_secs(15), async {
        while gateway.registry().describe(&target).is_ok() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("unload must revoke the target within 15s");

    // 旧句柄再次调用：必须拒绝，不得继续执行旧对象。
    let rejected = gateway
        .execute_prepared(&ctx1, &call1, &prepared1.handle)
        .await;
    assert!(rejected.is_err(), "卸载后旧句柄不得执行: {rejected:?}");

    // 同名工具重新解析也被拒：目标已撤销，不是换个入口就能找到。
    let ctx2 = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("sess-t11"),
        run_id: RunId::new("run-after-remove"),
        attempt: AttemptId::new("a1"),
        generation: 1,
    };
    let call2 = ToolCallId::new("call-after-remove");
    let request2 = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        "tool:mcp:fixture:env_report",
        json!({}),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("request");
    let re_prepare = gateway.prepare_from_request(
        &ctx2,
        lingxi_service::toolgateway::CallerSurface::UserRun,
        "agent",
        lingxi_service::toolgateway::InvocationPermissionContext::UserSession {
            mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
        },
        &call2,
        &request2,
    );
    assert!(re_prepare.is_err(), "卸载后同名目标不得再解析: {re_prepare:?}");

    server.stop().await;
}

/// 扩展边界：旧用户插件目录只提示不加载（F-D16-E-USER-PLUGIN-LEGACY），
/// 插件动态斜杠命令没有注册入口（F-D16-E-DYNAMIC-SLASH-PROTOCOL）——
/// 旧插件残留的 commands 目录在真实调度面上是响亮未知，不是悄悄执行。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r07_t11_extension_boundaries_hold() {
    let home = unique_dir("ext-boundary");
    // 旧版残留：用户插件目录里有一个带 commands 的插件。
    let legacy = home.join("plugins/legacy-demo");
    std::fs::create_dir_all(legacy.join("commands")).expect("legacy plugin dir");
    std::fs::write(legacy.join("commands/legacy-cmd.js"), "module.exports = {}")
        .expect("legacy command handler");
    std::fs::write(legacy.join("manifest.json"), json!({"name":"legacy-demo"}).to_string())
        .expect("legacy manifest");

    // 提示的数据源如实报告残留；空目录与隐藏项不算。
    let names = lingxi_service::legacy_plugins::legacy_user_plugin_names(&home);
    assert_eq!(names, vec!["legacy-demo".to_string()]);

    // 会话创建需要真实存在的助手（R07 目录收紧），按已证夹具登记 lingxi。
    std::fs::create_dir_all(home.join("agents/lingxi")).expect("lingxi dir");
    std::fs::write(
        home.join("agents/lingxi/config.yaml"),
        "agent:\n  name: lingxi\n  yuan: lingxi\nmemory:\n  enabled: false\n",
    )
    .expect("lingxi config");

    // 真实启动不被旧目录阻塞。
    let server = boot_at(home, false).await;

    // 旧插件的斜杠命令从未进入注册表：真实调度面响亮未知。
    let (status, body) = server
        .local(
            "POST",
            "/lingxi/v1/sessions",
            Some(json!({"sessionId":"desktop","agentId":"lingxi","title":"desktop"})),
        )
        .await;
    assert_eq!(status, 201, "create session: {body}");
    let (status, reply) = server
        .local(
            "POST",
            "/lingxi/v1/sessions/desktop/slash",
            Some(json!({"text":"/legacy-cmd"})),
        )
        .await;
    assert_eq!(status, 200, "slash dispatcher exists: {reply}");
    assert_eq!(reply["results"][0]["type"], "slash_result");
    assert_eq!(reply["results"][0]["level"], "error");
    assert!(
        reply["results"][0]["text"].as_str().unwrap_or("").contains("未知命令"),
        "legacy plugin command must stay unknown: {reply}"
    );

    server.stop().await;
}
