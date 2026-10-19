//! 原六叶：真实管理HTTP、私密Source、官方MCP HTTP服务器与自有OAuth外端。
//! launch只核现役描述/输入绑定，不把它算成工具执行；独立SSE仍未实现。
use axum::{
    body::{to_bytes, Body},
    extract::{Form, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_kernel::model_exchange::ModelOperation;
use lingxi_protocol::{
    McpAppLaunchResponse, McpAppsResponse, McpOAuthPollResponse, McpOAuthStartResponse,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use rmcp::{
    model::*,
    service::{NotificationContext, RequestContext},
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    RoleServer, ServerHandler,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Default)]
struct RemoteState {
    base: Mutex<String>,
    request_methods: Mutex<Vec<String>>,
    authorizations: Mutex<Vec<Option<String>>>,
    codes: Mutex<HashMap<String, (String, String)>>,
    registrations: AtomicUsize,
    exchanges: AtomicUsize,
    refreshes: AtomicUsize,
    tool_calls: AtomicUsize,
    tool_post_attempts: AtomicUsize,
    reject_tool_post: AtomicBool,
    next_model_tool: AtomicBool,
    duplicate_metadata: AtomicBool,
    challenge_next_listing: AtomicBool,
    hold_refresh: AtomicBool,
    refresh_started: tokio::sync::Notify,
    release_refresh: tokio::sync::Notify,
    next_login_token: AtomicBool,
    read_started: tokio::sync::Notify,
    read_cancelled: AtomicUsize,
    cancel_notifications: AtomicUsize,
    wire_cancel_notifications: AtomicUsize,
    protocol_versions: Mutex<Vec<String>>,
    delete_requests: AtomicUsize,
    read_active: AtomicUsize,
    read_dropped: AtomicUsize,
    read_drop_ct_cancelled: AtomicUsize,
    read_ct_probe: Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    hold_token: AtomicBool,
    token_started: tokio::sync::Notify,
    release_token: tokio::sync::Notify,
}
struct HeldReadGuard {
    state: Arc<RemoteState>,
    ct_probe: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl Drop for HeldReadGuard {
    fn drop(&mut self) {
        if (self.ct_probe)() {
            self.state
                .read_drop_ct_cancelled
                .fetch_add(1, Ordering::SeqCst);
        }
        self.state.read_dropped.fetch_add(1, Ordering::SeqCst);
        self.state.read_active.fetch_sub(1, Ordering::SeqCst);
    }
}
#[derive(Clone)]
struct Remote(Arc<RemoteState>);
impl ServerHandler for Remote {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tool = |name: &str, meta: Value| {
            serde_json::from_value::<Tool>(json!({"name":name,"description":"自有外端工具","inputSchema":{"type":"object","properties":{}},"_meta":meta})).unwrap()
        };
        let mut tools = vec![
            tool(
                "app",
                json!({"ui":{"resourceUri":"ui://owned/app","visibility":["model","app"]},"fixtureExtra":{"keep":19}}),
            ),
            tool(
                "model_only",
                json!({"ui":{"resourceUri":"ui://owned/hidden","visibility":["model"]}}),
            ),
            tool(
                "legacy_app",
                json!({"openai/outputTemplate":"ui://owned/legacy"}),
            ),
            tool("plain", json!({})),
        ];
        if self.0.duplicate_metadata.load(Ordering::SeqCst) {
            tools.push(tool("app",json!({"ui":{"resourceUri":"ui://owned/last-must-not-replace","visibility":[]},"fixtureExtra":{"keep":99}})));
        }
        Ok(ListToolsResult::with_all_items(tools))
    }
    async fn on_cancelled(
        &self,
        _: CancelledNotificationParam,
        _: NotificationContext<RoleServer>,
    ) {
        self.0.cancel_notifications.fetch_add(1, Ordering::SeqCst);
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.uri == "ui://owned/hold" {
            self.0.read_active.fetch_add(1, Ordering::SeqCst);
            let ct = context.ct.clone();
            let probe: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || ct.is_cancelled());
            *self.0.read_ct_probe.lock().unwrap() = Some(probe.clone());
            let _held = HeldReadGuard {
                state: self.0.clone(),
                ct_probe: probe,
            };
            self.0.read_started.notify_one();
            context.ct.cancelled().await;
            self.0.read_cancelled.fetch_add(1, Ordering::SeqCst);
            return Err(ErrorData::internal_error("自有请求已取消", None));
        }
        let contents: Value = match request.uri.as_str() {
            "ui://owned/blob" => {
                json!([{"uri":request.uri,"mimeType":"text/plain","blob":base64::engine::general_purpose::STANDARD.encode("真实blob内容")}])
            }
            "ui://owned/empty" => json!([]),
            // 沿原选择顺序，HTML优先于精确URI匹配。
            _ => {
                json!([{"uri":request.uri,"text":"不能替代HTML正例","mimeType":"text/plain"},{"uri":"ui://owned/html","text":"<!doctype html><script>window.remoteFixture=true</script><p>真实应用内容</p>","mimeType":"text/html; charset=utf-8"}])
            }
        };
        Ok(
            serde_json::from_value::<ReadResourceResult>(json!({"contents":contents}))
                .unwrap()
                .into(),
        )
    }
    async fn call_tool(
        &self,
        _: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.0.tool_calls.fetch_add(1, Ordering::SeqCst);
        Ok(CallToolResult::success(vec![ContentBlock::text("真实工具结果")]).into())
    }
}
async fn observe_mcp(
    State(remote): State<Arc<RemoteState>>,
    request: Request,
    next: Next,
) -> Response {
    let (parts, body) = request.into_parts();
    if let Some(version) = parts
        .headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
    {
        let mut versions = remote.protocol_versions.lock().unwrap();
        if !versions.iter().any(|observed| observed == version) {
            versions.push(version.to_owned());
        }
    }
    if parts.method == axum::http::Method::DELETE {
        remote.delete_requests.fetch_add(1, Ordering::SeqCst);
    }
    remote.authorizations.lock().unwrap().push(
        parts
            .headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned),
    );
    let authorized = matches!(
        parts
            .headers
            .get("authorization")
            .and_then(|h| h.to_str().ok()),
        Some(
            "Bearer owned-bearer"
                | "Bearer owned-access"
                | "Bearer owned-refreshed"
                | "Bearer owned-next-login"
                | "Bearer owned-stale-late"
        )
    );
    if !authorized {
        let base = remote.base.lock().unwrap().clone();
        return (
            StatusCode::UNAUTHORIZED,
            [(
                "www-authenticate",
                format!("Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\""),
            )],
            "自有外端要求独立凭据",
        )
            .into_response();
    }
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
        if let Some(method) = value["method"].as_str() {
            remote.request_methods.lock().unwrap().push(method.into());
            if method == "notifications/cancelled" {
                remote
                    .wire_cancel_notifications
                    .fetch_add(1, Ordering::SeqCst);
            }
            if method == "tools/list" && remote.challenge_next_listing.swap(false, Ordering::SeqCst)
            {
                let base = remote.base.lock().unwrap().clone();
                return (StatusCode::UNAUTHORIZED, [("www-authenticate",format!("Bearer error=\"invalid_token\", resource_metadata=\"{base}/.well-known/oauth-protected-resource\""))], "自有端要求真实旧凭据刷新").into_response();
            }
            if method == "tools/call" {
                remote.tool_post_attempts.fetch_add(1, Ordering::SeqCst);
                if remote.reject_tool_post.load(Ordering::SeqCst) {
                    return (StatusCode::NOT_FOUND, "自有端拒绝旧请求；不能重投").into_response();
                }
            }
        }
    }
    // 仅观测和鉴权，实际MCP协议处理由官方服务完成。
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}
async fn resource_metadata(State(remote): State<Arc<RemoteState>>) -> Json<Value> {
    let base = remote.base.lock().unwrap().clone();
    Json(
        json!({"resource":format!("{base}/mcp"),"authorization_servers":[base],"scopes_supported":["mcp"]}),
    )
}
async fn authorization_metadata(State(remote): State<Arc<RemoteState>>) -> Json<Value> {
    let base = remote.base.lock().unwrap().clone();
    Json(
        json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"registration_endpoint":format!("{base}/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"],"scopes_supported":["mcp"]}),
    )
}
async fn register(State(remote): State<Arc<RemoteState>>, Json(body): Json<Value>) -> Json<Value> {
    remote.registrations.fetch_add(1, Ordering::SeqCst);
    Json(
        json!({"client_id":"owned-dynamic-client","redirect_uris":body["redirect_uris"],"token_endpoint_auth_method":"none","grant_types":["authorization_code","refresh_token"],"response_types":["code"]}),
    )
}
async fn authorize(
    State(remote): State<Arc<RemoteState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(
        query.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    let redirect = query.get("redirect_uri").unwrap();
    let mut url = reqwest::Url::parse(redirect).unwrap();
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert_eq!(url.path(), "/lingxi/v1/mcp/oauth/callback");
    let code = lingxi_adapters::storage::agent_memory::random_token_suffix();
    remote.codes.lock().unwrap().insert(
        code.clone(),
        (query["code_challenge"].clone(), redirect.clone()),
    );
    url.query_pairs_mut()
        .append_pair("state", &query["state"])
        .append_pair("code", &code);
    (StatusCode::FOUND, [("location", url.as_str())]).into_response()
}
async fn token(
    State(remote): State<Arc<RemoteState>>,
    headers: HeaderMap,
    Form(body): Form<HashMap<String, String>>,
) -> Json<Value> {
    assert!(
        !headers.contains_key("authorization"),
        "公开客户端不得借宿主Bearer或其它提供者凭据"
    );
    assert_eq!(body["client_id"], "owned-dynamic-client");
    if body["grant_type"] == "refresh_token" {
        assert_eq!(body["refresh_token"], "owned-refresh");
        remote.refreshes.fetch_add(1, Ordering::SeqCst);
        if remote.hold_refresh.load(Ordering::SeqCst) {
            remote.refresh_started.notify_one();
            remote.release_refresh.notified().await;
            return Json(
                json!({"access_token":"owned-stale-late","token_type":"Bearer","refresh_token":"owned-refresh","expires_in":3600,"scope":"mcp"}),
            );
        }
        return Json(
            json!({"access_token":"owned-refreshed","token_type":"Bearer","refresh_token":"owned-refresh","expires_in":3600,"scope":"mcp"}),
        );
    }
    assert_eq!(body["grant_type"], "authorization_code");
    let (challenge, redirect) = remote
        .codes
        .lock()
        .unwrap()
        .remove(&body["code"])
        .expect("每个真实授权码仅兑换一次");
    assert_eq!(body["redirect_uri"], redirect);
    let actual = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(body["code_verifier"].as_bytes()));
    assert_eq!(
        actual, challenge,
        "官方PKCE的真实verifier必须匹配浏览器challenge"
    );
    remote.exchanges.fetch_add(1, Ordering::SeqCst);
    remote.token_started.notify_one();
    if remote.hold_token.load(Ordering::SeqCst) {
        remote.release_token.notified().await;
    }
    if remote.next_login_token.load(Ordering::SeqCst) {
        return Json(
            json!({"access_token":"owned-next-login","token_type":"Bearer","refresh_token":"owned-next-refresh","expires_in":3600,"scope":"mcp"}),
        );
    }
    // 立即过期使下一次真实连接通过官方SDK刷新，不等待假时钟。
    Json(
        json!({"access_token":"owned-access","token_type":"Bearer","refresh_token":"owned-refresh","expires_in":0,"scope":"mcp"}),
    )
}
async fn chat(State(remote): State<Arc<RemoteState>>, Json(body): Json<Value>) -> Response {
    let next = remote.next_model_tool.swap(false, Ordering::SeqCst);
    let (delta, reason) = if next {
        let name = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .find(|n| n.ends_with("app") && !n.ends_with("legacy_app"))
            .expect("真实运行必须声明实际MCP工具");
        (
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"owned-http-call","type":"function","function":{"name":name,"arguments":"{}"}}]}),
            "tool_calls",
        )
    } else {
        (json!({"role":"assistant","content":"实际模型完成"}), "stop")
    };
    let a = json!({"id":"owned-model","object":"chat.completion.chunk","model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let b = json!({"id":"owned-model","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {a}\n\ndata: {b}\n\ndata: [DONE]\n\n"),
    )
        .into_response()
}
struct External {
    state: Arc<RemoteState>,
    base: String,
    stop: Box<dyn FnOnce() + Send>,
    task: tokio::task::JoinHandle<()>,
}
impl External {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(RemoteState::default());
        *state.base.lock().unwrap() = base.clone();
        let stop = StreamableHttpServerConfig::default().cancellation_token;
        let shutdown = stop.clone();
        let remote = Remote(state.clone());
        let sdk: StreamableHttpService<Remote, LocalSessionManager> = StreamableHttpService::new(
            move || Ok(remote.clone()),
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_cancellation_token(stop.child_token())
                .with_sse_keep_alive(Some(Duration::from_millis(100))),
        );
        let app = Router::new()
            .nest_service("/mcp", sdk)
            .layer(middleware::from_fn_with_state(state.clone(), observe_mcp));
        let app = Router::new()
            .merge(app)
            .route(
                "/.well-known/oauth-protected-resource",
                get(resource_metadata),
            )
            .route(
                "/.well-known/oauth-authorization-server",
                get(authorization_metadata),
            )
            .route("/register", post(register))
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .route("/v1/chat/completions", post(chat))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            state,
            base,
            stop: Box::new(move || stop.cancel()),
            task,
        }
    }
    async fn close(self) {
        (self.stop)();
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .expect("自有HTTP外端必须退出")
            .unwrap();
    }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-mcp-remote-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        Self::boot(home).await
    }
    async fn boot(home: PathBuf) -> Self {
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            "{\"current_agent_id\":\"a\"}",
        )
        .unwrap();
        if !home.join("plugin-data/mcp/config.json").exists() {
            std::fs::create_dir_all(home.join("plugin-data/mcp")).unwrap();
            std::fs::write(home.join("plugin-data/mcp/config.json"), json!({"schemaVersion":1,"rootExtra":{"keep":37},"global":{"mcp":{"enabled":false,"servers":[],"unknownGlobal":41}},"agents":{},"sessions":{}}).to_string()).unwrap();
        }
        let home = home.canonicalize().unwrap();
        eprintln!("MCP远端隔离现场：{}", home.display());
        let layout = prepare_layout(&home).unwrap();
        let plane = ModelPlaneConfig::from_value(json!({"providers":{},"models":{}})).unwrap();
        let clock = Arc::new(lingxi_service::inject::SystemClock);
        let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
            clock.clone(),
        ));
        let gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
            Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new()),
            approvals.clone(),
            clock.clone(),
            Default::default(),
            300_000,
            1000,
        ));
        let credentials = Arc::new(
            lingxi_service::credentials::CredentialService::bootstrap(
                &plane,
                &layout.runtime_dir,
                clock.clone(),
            )
            .unwrap(),
        );
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 10_000,
            },
            &layout,
            ServiceDeps {
                model_gateway: Some(Arc::new(ConfigModelGateway::from_validated(plane))),
                credential_service: Some(credentials),
                network_plane: Some(NetworkPlane::direct_isolated()),
                model_plane_activation: Some(Arc::new(AtomicBool::new(false))),
                tool_gateway: Some(gateway),
                approval_gate: Some(approvals),
                clock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, address) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let task = tokio::spawn(async move {
            lingxi_service::run(
                serving,
                async {
                    let _ = rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        let base = format!("http://{}", address.await.unwrap());
        Self {
            home,
            state,
            base,
            stop,
            task,
        }
    }
    fn source(&self) -> PathBuf {
        self.home.join("plugin-data/mcp/config.json")
    }
    fn document(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.source()).unwrap()).unwrap()
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.state.auth().local_token());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"text":text})),
        )
    }
    async fn action(&self, action: &str, payload: Value) -> (u16, Value) {
        self.request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":action,"payload":payload})),
        )
        .await
    }
    async fn add(&self, external: &External, oauth: bool) {
        let (status,value)=self.action("mcp.connector.add",json!({"connector":{"id":"owned","name":"Owned HTTP","transport":"streamable-http","url":format!("{}/mcp",external.base),"authType":if oauth{"oauth"}else{"bearer"},"authorizationToken":if oauth{""}else{"owned-bearer"},"unknownConnector":{"keep":43}},"enableGlobal":true,"startIfEnabled":false})).await;
        assert_eq!(status, 200, "{value}");
        assert_eq!(value["settingsUpdate"]["saved"], true);
    }
    async fn start(&self) {
        let (status, value) = self
            .action("mcp.connector.start", json!({"connectorId":"owned"}))
            .await;
        assert_eq!(status, 200, "真实官方SDK启动：{value}");
        assert_eq!(value["settingsUpdate"]["runtimeApplied"], true);
    }
    async fn oauth_start(&self) -> McpOAuthStartResponse {
        let (status, value) = self
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/mcp/connectors/owned/oauth/start",
                None,
            )
            .await;
        assert_eq!(status, 200, "{value}");
        serde_json::from_value(value).unwrap()
    }
    async fn poll(&self, session: &str) -> McpOAuthPollResponse {
        let (status, value) = self
            .request(
                reqwest::Method::GET,
                &format!("/lingxi/v1/mcp/oauth/poll/{session}"),
                None,
            )
            .await;
        assert_eq!(status, 200, "{value}");
        serde_json::from_value(value).unwrap()
    }
    async fn browser(&self, flow: &McpOAuthStartResponse) -> reqwest::Response {
        // 浏览器不附宿主Bearer，只有官方生成的state及PKCE授权码经过回调。
        reqwest::Client::new().get(&flow.url).send().await.unwrap()
    }
    async fn close(self, remove: bool) -> PathBuf {
        self.stop.send(()).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(15), self.task)
            .await
            .expect("服务退出有界")
            .unwrap()
            .unwrap();
        assert!(!outcome.drain_timed_out);
        self.state.storage().close().await.unwrap();
        if remove {
            std::fs::remove_dir_all(&self.home).unwrap();
        }
        self.home
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_apps_visibility_launch_input_resources_and_generation_cancellation() {
    let external = External::start().await;
    external
        .state
        .duplicate_metadata
        .store(true, Ordering::SeqCst);
    let h = Harness::new().await;
    h.add(&external, false).await;
    h.start().await;
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/apps?agentId=a", None)
        .await;
    assert_eq!(status, 200, "{value}");
    let apps: McpAppsResponse = serde_json::from_value(value).unwrap();
    assert_eq!(
        apps.apps
            .iter()
            .map(|a| a.tool_name.as_str())
            .collect::<Vec<_>>(),
        vec!["app", "legacy_app"]
    );
    assert_eq!(
        apps.apps[0].meta.as_ref().unwrap()["fixtureExtra"]["keep"],
        19
    );
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/connectors/owned/apps/app/launch",
            Some(json!({"agentId":"a","launchInput":{"text":"用户实际输入"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let launch: McpAppLaunchResponse = serde_json::from_value(value).unwrap();
    assert_eq!(launch.launch_input, json!({"text":"用户实际输入"}));
    assert_eq!(launch.kind, "mcp_app");
    assert_eq!(
        external.state.tool_calls.load(Ordering::SeqCst),
        0,
        "launch描述不能冒充工具执行"
    );
    let resource = reqwest::Client::new()
        .get(format!(
            "{}{}",
            h.base,
            launch.binding["resourceUrl"].as_str().unwrap()
        ))
        .bearer_auth(h.state.auth().local_token())
        .send()
        .await
        .unwrap();
    assert_eq!(resource.status(), StatusCode::OK);
    assert_eq!(resource.headers()["cache-control"], "no-store");
    assert_eq!(resource.headers()["x-content-type-options"], "nosniff");
    let csp = resource.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("sandbox allow-scripts"));
    assert!(!csp.contains("allow-same-origin"));
    assert!(resource.text().await.unwrap().contains("真实应用内容"));
    let (_, blob) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/mcp/connectors/owned/resources?uri=ui%3A%2F%2Fowned%2Fblob",
            None,
        )
        .await;
    assert_eq!(blob["text"], "真实blob内容");
    let (status, _) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/mcp/connectors/owned/resources?uri=file%3A%2F%2Fetc%2Fpasswd",
            None,
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/mcp/connectors/owned/resources?uri=ui%3A%2F%2Fowned%2Fempty",
            None,
        )
        .await;
    assert_eq!(status, 404);
    let request = reqwest::Client::new()
        .get(format!(
            "{}/lingxi/v1/mcp/connectors/owned/resources?uri=ui%3A%2F%2Fowned%2Fhold",
            h.base
        ))
        .bearer_auth(h.state.auth().local_token())
        .send();
    tokio::pin!(request);
    tokio::select! { _=&mut request=>panic!("真实资源等待不应提前结束"), _=external.state.read_started.notified()=>{} }
    let (status, value) = h
        .action("mcp.connector.stop", json!({"connectorId":"owned"}))
        .await;
    assert_eq!(status, 200, "{value}");
    let response = tokio::time::timeout(Duration::from_secs(3), request)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(response.status(), StatusCode::OK);
    let released = tokio::time::timeout(Duration::from_secs(3), async {
        while external.state.read_active.load(Ordering::SeqCst) != 0
            || external.state.read_dropped.load(Ordering::SeqCst) == 0
        {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let ct_cancelled = external
        .state
        .read_ct_probe
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|probe| probe());
    let facts = json!({"protocolVersions":external.state.protocol_versions.lock().unwrap().clone(),"wireCancelNotifications":external.state.wire_cancel_notifications.load(Ordering::SeqCst),
        "handlerCancelNotifications":external.state.cancel_notifications.load(Ordering::SeqCst),
        "handlerCtBranch":external.state.read_cancelled.load(Ordering::SeqCst),"handlerCtCancelled":ct_cancelled,
        "remoteHandlerDropped":external.state.read_dropped.load(Ordering::SeqCst),"remoteActive":external.state.read_active.load(Ordering::SeqCst),
        "handlerDropCtCancelled":external.state.read_drop_ct_cancelled.load(Ordering::SeqCst),"deleteRequests":external.state.delete_requests.load(Ordering::SeqCst)});
    eprintln!("自有官方SDK远端资源实际观测：{facts}");
    assert!(
        released.is_ok(),
        "远端handler未在原3秒内真实退出，不以本地drop冒充回收：{facts}"
    );
    let (_, apps) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/apps", None)
        .await;
    assert_eq!(apps["apps"], json!([]));
    assert!(external
        .state
        .authorizations
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .all(|h| h == "Bearer owned-bearer"));
    h.close(true).await;
    external.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_oauth_pkce_dcr_private_source_refresh_restart_resources_and_logout() {
    let external = External::start().await;
    let h = Harness::new().await;
    h.add(&external, true).await;
    let flow = h.oauth_start().await;
    assert_eq!(h.poll(&flow.session_id).await.status, "pending");
    let before = std::fs::read(h.source()).unwrap();
    let wrong = reqwest::Client::new()
        .get(format!(
            "{}/lingxi/v1/mcp/oauth/callback?state=wrong&code=wrong",
            h.base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read(h.source()).unwrap(), before);
    let browser = h.browser(&flow).await;
    assert_eq!(browser.status(), StatusCode::OK);
    assert!(browser.text().await.unwrap().contains("已登录"));
    assert_eq!(h.poll(&flow.session_id).await.status, "done");
    assert_eq!(external.state.registrations.load(Ordering::SeqCst), 1);
    assert_eq!(external.state.exchanges.load(Ordering::SeqCst), 1);
    let document = h.document();
    assert_eq!(document["rootExtra"]["keep"], 37);
    let connector = &document["global"]["mcp"]["servers"][0];
    assert_eq!(connector["unknownConnector"]["keep"], 43);
    assert_eq!(connector["oauthClientId"], "owned-dynamic-client");
    assert!(connector.pointer("/oauth/sdk/token_response").is_some());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(h.source()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let (_, view) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=a", None)
        .await;
    assert_eq!(view["config"]["connectors"][0]["authStatus"], "connected");
    assert!(!view.to_string().contains("owned-access"));
    assert!(!view.to_string().contains("owned-refresh"));
    h.start().await;
    assert!(
        external.state.refreshes.load(Ordering::SeqCst) >= 1,
        "下一真实连接必须消费同Source并由官方SDK刷新过期凭据"
    );
    let saved = std::fs::read(h.source()).unwrap();
    let replay = h.browser(&flow).await;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read(h.source()).unwrap(), saved);
    let home = h.close(false).await;
    let h = Harness::boot(home).await;
    assert_eq!(
        h.poll(&flow.session_id).await.status,
        "missing",
        "重启不恢复或重放旧浏览器事务"
    );
    let (status, apps) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/apps", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(apps["apps"].as_array().unwrap().len(), 2);
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/connectors/owned/oauth/logout",
            None,
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert!(h.document()["global"]["mcp"]["servers"][0]["oauth"]
        .as_object()
        .unwrap()
        .is_empty());
    let (status, value) = h
        .action("mcp.connector.start", json!({"connectorId":"owned"}))
        .await;
    assert_eq!(status, 409, "{value}");
    assert_eq!(
        value["settingsUpdate"]["error"]["code"],
        "mcp_authorization_required"
    );
    let (_, apps) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/apps", None)
        .await;
    assert_eq!(apps["apps"], json!([]));
    assert!(external
        .state
        .authorizations
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .all(|h| h == "Bearer owned-access" || h == "Bearer owned-refreshed"));
    h.close(true).await;
    external.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_cancel_changed_configuration_revoked_owner_and_shutdown_never_resurrect_credentials()
{
    let external = External::start().await;
    let h = Harness::new().await;
    h.add(&external, true).await;
    let flow = h.oauth_start().await;
    let before = std::fs::read(h.source()).unwrap();
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/connectors/owned/oauth/cancel",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(value["cancelled"], 1);
    assert_eq!(h.poll(&flow.session_id).await.status, "cancelled");
    let browser = h.browser(&flow).await;
    assert_eq!(browser.status(), StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read(h.source()).unwrap(), before);
    assert_eq!(external.state.exchanges.load(Ordering::SeqCst), 0);
    let flow = h.oauth_start().await;
    let (status, value) = h
        .action(
            "mcp.connector.update",
            json!({"connectorId":"owned","patch":{"headers":{"X-Owned-Config":"next"}}}),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let changed = std::fs::read(h.source()).unwrap();
    assert_eq!(h.browser(&flow).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read(h.source()).unwrap(), changed);
    assert_eq!(external.state.exchanges.load(Ordering::SeqCst), 0);
    let flow = h.oauth_start().await;
    let before = std::fs::read(h.source()).unwrap();
    let layout = prepare_layout(&h.home).unwrap();
    let replacement =
        lingxi_service::auth::AuthService::bootstrap(&layout, "owned-oauth-authority-replaced")
            .unwrap();
    assert_ne!(replacement.local_token(), h.state.auth().local_token());
    assert_eq!(h.browser(&flow).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read(h.source()).unwrap(), before);
    assert_eq!(external.state.exchanges.load(Ordering::SeqCst), 0);
    h.close(true).await;

    // token已经到达自有端后退出：事实可能已发生，但旧流程不能再写Source或重放。
    let h = Harness::new().await;
    h.add(&external, true).await;
    let flow = h.oauth_start().await;
    external.state.hold_token.store(true, Ordering::SeqCst);
    let before = std::fs::read(h.source()).unwrap();
    let status = {
        let browser = h.browser(&flow);
        tokio::pin!(browser);
        tokio::select! { _=&mut browser=>panic!("真实token外端尚未释放"), _=external.state.token_started.notified()=>{} }
        h.state.mcp_settings().begin_stop();
        external.state.release_token.notify_one();
        tokio::time::timeout(Duration::from_secs(3), browser)
            .await
            .unwrap()
            .status()
    };
    assert_eq!(status, StatusCode::BAD_REQUEST);
    h.state.mcp_settings().shutdown().await.unwrap();
    assert_eq!(std::fs::read(h.source()).unwrap(), before);
    assert_eq!(external.state.exchanges.load(Ordering::SeqCst), 1);
    h.close(true).await;
    external.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_session_run_http_404_never_replays_the_posted_mcp_tool_call() {
    use lingxi_service::model_configuration::{ModelSelection, ModelSettingsChange};
    let external = External::start().await;
    let h = Harness::new().await;
    h.add(&external, false).await;
    h.start().await;
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", h.state.auth().local_token())),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    let report=h.state.model_configuration().save(&principal,&[
        ModelSettingsChange::ProviderPatch{provider:"owned-model".into(),patch:json!({"api":"openai-completions","base_url":format!("{}/v1",external.base),"models":[{"id":"owned-chat","inputs":["text"],"toolUse":{"supportsTools":true,"dialect":"openai","toolResultFormat":"message"}}]})},
        ModelSettingsChange::SetBinding{operation:ModelOperation::Chat,selection:Some(ModelSelection{provider:"owned-model".into(),model:"owned-chat".into()})},
    ]).await.unwrap();
    assert!(report.saved && report.applied);
    h.state
        .session_admin()
        .create_session(
            "owned-http-unknown",
            "a",
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
            "实际HTTP工具未知结果",
            Some(lingxi_kernel::subagent::SessionPermissionMode::Operate.wire_name()),
            &[],
            false,
            1_792_000_000_000,
        )
        .await
        .unwrap();
    external
        .state
        .reject_tool_post
        .store(true, Ordering::SeqCst);
    external.state.next_model_tool.store(true, Ordering::SeqCst);
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/sessions/owned-http-unknown/execute",
            Some(json!({"input":"执行这次明确MCP调用"})),
        )
        .await;
    assert!((200..300).contains(&status), "{value}");
    let run = value["runId"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = h
                .state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id=?1",
                    vec![run.to_owned()],
                )
                .await
                .unwrap()
                .unwrap();
            if matches!(
                status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted_needs_attention"
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("真实Run必须收尾，不能重投未知调用");
    assert_eq!(
        external.state.tool_post_attempts.load(Ordering::SeqCst),
        1,
        "404之后绝不能自动重发已投工具请求"
    );
    assert_eq!(
        external.state.tool_calls.load(Ordering::SeqCst),
        0,
        "自有404发生在官方工具执行前，不能冒称执行成功"
    );
    h.close(true).await;
    external.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn old_official_sdk_refresh_cannot_overwrite_a_new_browser_login_committed_to_source() {
    let external = External::start().await;
    let h = Harness::new().await;
    h.add(&external, true).await;
    let initial = h.oauth_start().await;
    assert_eq!(h.browser(&initial).await.status(), StatusCode::OK);
    h.start().await;
    // 新浏览器流程先获得独立state，再让现役refresh-tools真实持有运行锁。
    let next = h.oauth_start().await;
    external.state.hold_refresh.store(true, Ordering::SeqCst);
    external
        .state
        .challenge_next_listing
        .store(true, Ordering::SeqCst);
    let preserved = {
        let refresh = h.action(
            "mcp.connector.refresh_tools",
            json!({"connectorId":"owned"}),
        );
        tokio::pin!(refresh);
        tokio::select! {
            result=&mut refresh=>panic!("旧SDK尚未到达受控refresh端就结束：HTTP {}",result.0),
            result=tokio::time::timeout(Duration::from_secs(5),external.state.refresh_started.notified())=>result.expect("旧SDK真实refresh必须先到达自有外端"),
        }
        external
            .state
            .next_login_token
            .store(true, Ordering::SeqCst);
        let browser = h.browser(&next);
        tokio::pin!(browser);
        let committed = async {
            loop {
                if h.document()["global"]["mcp"]["servers"][0]
                    .pointer("/oauth/sdk/token_response/access_token")
                    .and_then(Value::as_str)
                    == Some("owned-next-login")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        // 真实Source已经落入新登录凭据，callback随后等运行锁，旧refresh尚未释放。
        tokio::select! {
            response=&mut browser=>panic!("运行锁尚被旧refresh持有，新callback不应提前结束：{}",response.status()),
            result=tokio::time::timeout(Duration::from_secs(5),committed)=>result.expect("新登录必须先实际提交同一Source"),
        }
        external.state.release_refresh.notify_one();
        let (_, response) = tokio::time::timeout(Duration::from_secs(8), async {
            tokio::join!(refresh, browser)
        })
        .await
        .expect("旧refresh与新callback均须真实收尾");
        assert_eq!(response.status(), StatusCode::OK);
        let document = h.document();
        document["global"]["mcp"]["servers"][0]
            .pointer("/oauth/sdk/token_response/access_token")
            .and_then(Value::as_str)
            == Some("owned-next-login")
    };
    let home = h.home.clone();
    h.close(preserved).await;
    external.close().await;
    assert!(
        preserved,
        "旧SDK迟到refresh覆盖了已经确认保存的新登录；隔离现场={}",
        home.display()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_browser_flows_release_pending_capacity_without_erasing_saved_facts() {
    let external = External::start().await;
    let h = Harness::new().await;
    h.add(&external, true).await;
    let before = std::fs::read(h.source()).unwrap();
    for index in 0..32 {
        let flow = h.oauth_start().await;
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/mcp/connectors/owned/oauth/cancel",
                None,
            )
            .await;
        assert_eq!(status, 200, "round={index} {value}");
        assert_eq!(value["cancelled"], 1);
        assert_eq!(h.poll(&flow.session_id).await.status, "cancelled");
    }
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/connectors/owned/oauth/start",
            None,
        )
        .await;
    let unchanged = std::fs::read(h.source()).unwrap() == before;
    let exchanges = external.state.exchanges.load(Ordering::SeqCst);
    let home = h.home.clone();
    h.close(status == 200).await;
    external.close().await;
    assert!(unchanged);
    assert_eq!(exchanges, 0, "取消的等待不能兑换凭据");
    assert_eq!(
        status,
        200,
        "32个已取消历史不应仍算等待并封死下一次登录；home={} response={value}",
        home.display()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_content_mcp_oauth_page_consumes_the_same_owned_sdk_and_private_source() {
    let external = External::start().await;
    let h = Harness::new().await;
    h.add(&external, true).await;
    // 正常设置入口的模型目录读取需真实激活配置；复用已通过404实链的同一发布者。
    use lingxi_service::model_configuration::{ModelSelection, ModelSettingsChange};
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", h.state.auth().local_token())),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    let report=h.state.model_configuration().save(&principal,&[
        ModelSettingsChange::ProviderPatch{provider:"owned-model".into(),patch:json!({"api":"openai-completions","base_url":format!("{}/v1",external.base),"models":[{"id":"owned-chat","inputs":["text"],"toolUse":{"supportsTools":true,"dialect":"openai","toolResultFormat":"message"}}]})},
        ModelSettingsChange::SetBinding{operation:ModelOperation::Chat,selection:Some(ModelSelection{provider:"owned-model".into(),model:"owned-chat".into()})},
    ]).await.unwrap();
    assert!(report.saved && report.applied);
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let mut command = tokio::process::Command::new("npx");
    command
        .args([
            "vitest",
            "run",
            "desktop/src/react/__tests__/services/r07-mcp-oauth-real-service.test.tsx",
            "--maxWorkers=1",
        ])
        .current_dir(repo)
        .env("LINGXI_R07_MCP_UI_BASE", &h.base)
        .env("LINGXI_R07_MCP_UI_TOKEN", h.state.auth().local_token())
        .env("LINGXI_R07_MCP_UI_EXTERNAL", &external.base)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(60), command.output()).await;
    let completed = match output {
        Ok(Ok(output)) => {
            eprintln!(
                "真实MCP设置消费退出：code={:?}, success={}",
                output.status.code(),
                output.status.success()
            );
            eprintln!(
                "真实MCP设置消费stdout：\n{}",
                String::from_utf8_lossy(&output.stdout)
            );
            eprintln!(
                "真实MCP设置消费stderr：\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            Some(output.status)
        }
        Ok(Err(error)) => {
            eprintln!("真实MCP设置消费进程未启动：{error}");
            None
        }
        Err(_) => {
            eprintln!("真实MCP设置消费超过原60秒预算");
            None
        }
    };
    let document = h.document();
    let connector = &document["global"]["mcp"]["servers"][0];
    let cleared = connector["oauth"]
        .as_object()
        .is_some_and(|oauth| oauth.is_empty());
    let unknowns =
        document["rootExtra"]["keep"] == 37 && connector["unknownConnector"]["keep"] == 43;
    let exchanges = external.state.exchanges.load(Ordering::SeqCst);
    let refreshes = external.state.refreshes.load(Ordering::SeqCst);
    let calls = external.state.tool_calls.load(Ordering::SeqCst);
    let success = completed.is_some_and(|status| status.success())
        && cleared
        && unknowns
        && exchanges == 1
        && refreshes >= 1
        && calls == 0;
    let home = h.home.clone();
    h.close(success).await;
    external.close().await;
    assert!(
        completed.is_some_and(|status| status.success()),
        "真实设置页面失败，已清理服务/外端；现场={}",
        home.display()
    );
    assert!(cleared, "页面logout必须清空同Source SDK凭据");
    assert!(unknowns);
    assert_eq!(exchanges, 1, "取消第二次浏览器等待不能再兑换令牌");
    assert!(refreshes >= 1);
    assert_eq!(calls, 0, "登录/连接不冒充用户工具执行");
}
