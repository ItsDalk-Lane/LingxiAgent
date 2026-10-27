//! 只替外部HTTP/搜索页面响应；请求、正式网关、Run、偏好和会话文件都使用真实实现。
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json, Router,
};
use lingxi_adapters::{models::network::NetworkPlane, storage::agent_memory::random_token_suffix};
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
    RunContext,
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, RunId};
use lingxi_service::{
    approval_service::ApprovalService, inject::ManualClock, prepare_layout,
    toolgateway::ToolInvocationGateway, ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig,
    ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

/// 只替换外部搜索页面返回值；正式供应商选择、抽取脚本、取消标志与诊断仍由WebTools执行。
#[derive(Default)]
struct BrowserPages {
    seen: Mutex<Vec<String>>,
    all_failed: bool,
    wait_for_cancel: std::sync::atomic::AtomicBool,
    cleanup_confirmed: std::sync::atomic::AtomicBool,
}
impl lingxi_kernel::browser::BrowserPort for BrowserPages {
    fn execute<'a>(
        &'a self,
        _scope: lingxi_kernel::browser::BrowserScope,
        _command: lingxi_protocol::BrowserCommand,
        _cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> lingxi_kernel::browser::BrowserFuture<'a, lingxi_kernel::browser::BrowserOutput> {
        Box::pin(async { unreachable!("搜索不能触碰聊天浏览器") })
    }
    fn workspaces(
        &self,
    ) -> Result<Vec<lingxi_protocol::BrowserWorkspace>, lingxi_kernel::browser::BrowserError> {
        Ok(vec![])
    }
    fn restore(
        &self,
        _scope: lingxi_kernel::browser::BrowserScope,
        _state: lingxi_protocol::BrowserWorkspace,
    ) -> Result<(), lingxi_kernel::browser::BrowserError> {
        unreachable!("搜索不能恢复聊天页面")
    }
    fn revoke<'a>(
        &'a self,
        _scope: lingxi_kernel::browser::BrowserScope,
    ) -> lingxi_kernel::browser::BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn configure<'a>(
        &'a self,
        _settings: lingxi_protocol::BrowserSettings,
    ) -> lingxi_kernel::browser::BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn clear_site_data<'a>(&'a self) -> lingxi_kernel::browser::BrowserFuture<'a, ()> {
        Box::pin(async { unreachable!("搜索不能清理用户登录") })
    }
    fn read_auxiliary_page<'a>(
        &'a self,
        url: String,
        expression: String,
        locale: Option<String>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> lingxi_kernel::browser::BrowserFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cancel.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(locale.as_deref(), Some("zh"));
            assert!(expression.contains("buildBrowserSearchExtractionScript"));
            assert!(!expression.contains("existing-key") && !expression.contains("rejected-key"));
            self.seen.lock().unwrap().push(url.clone());
            if self
                .wait_for_cancel
                .load(std::sync::atomic::Ordering::Acquire)
            {
                while !cancel.load(std::sync::atomic::Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                self.cleanup_confirmed
                    .store(true, std::sync::atomic::Ordering::Release);
                return Err(lingxi_kernel::browser::BrowserError {
                    code: "cancelled".into(),
                    message: "外部临时页已确认回收".into(),
                    effect_unknown: false,
                });
            }
            if url.contains("www.bing.com") {
                Ok(json!({"status":"blocked","blocked":true,"captcha":true,"results":[]}))
            } else if url.contains("www.google.com") || self.all_failed {
                Ok(json!({"status":"extraction_failed","results":[]}))
            } else {
                Ok(
                    json!({"status":"ok","final_url":url,"results":[{"title":"浏览器结果","url":"https://example.org/browser","content":"隔离页面正文","rank":1}]}),
                )
            }
        })
    }
    fn reap_idle<'a>(&'a self, _now: u64) -> lingxi_kernel::browser::BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn shutdown<'a>(&'a self) -> lingxi_kernel::browser::BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct Model {
    results: Mutex<BTreeMap<String, ToolOutcome>>,
    materialize: Mutex<BTreeMap<String, Value>>,
}
impl TurnProviderPort for Model {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "fixture".into(),
            model: "web-tools".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _sink: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(async move {
            if input.turn == 1 {
                let request: Value = serde_json::from_str(&input.submission).unwrap();
                if request["tool"] == "materialize" {
                    self.materialize
                        .lock()
                        .unwrap()
                        .insert(ctx.run_id.to_string(), request["args"].clone());
                    return ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: vec![ToolRequest::from_effective_arguments(
                                "tool:first-party:mcp_describe_tool",
                                json!({"name":"materialize"}),
                                &SchemaBudget::default(),
                            )
                            .unwrap()],
                            content: vec![],
                        },
                    );
                }
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            format!("tool:first-party:{}", request["tool"].as_str().unwrap()),
                            request["args"].clone(),
                            &SchemaBudget::default(),
                        )
                        .unwrap()],
                        content: vec![],
                    },
                );
            }
            if let Some(outcome) = input.prior.iter().rev().find_map(|e| {
                if let ExchangeItem::ToolResult { outcome, .. } = e {
                    Some(outcome.clone())
                } else {
                    None
                }
            }) {
                let materialize = self.materialize.lock().unwrap().remove(ctx.run_id.as_str());
                if let Some(args) = materialize {
                    assert!(
                        matches!(outcome, ToolOutcome::Success { .. }),
                        "必须先取得真实materialize说明：{outcome:?}"
                    );
                    return ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: vec![ToolRequest::from_effective_arguments(
                                "tool:first-party:mcp_call",
                                json!({"tool":"materialize","arguments":args}),
                                &SchemaBudget::default(),
                            )
                            .unwrap()],
                            content: vec![],
                        },
                    );
                }
                self.results
                    .lock()
                    .unwrap()
                    .insert(ctx.run_id.to_string(), outcome);
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: "本地验证完成".into(),
                        }],
                        model_call_id: Some(call.clone()),
                    },
                },
            )
        })
    }
}
#[derive(Default)]
struct External {
    seen: Mutex<Vec<(String, HeaderMap, Value)>>,
    free_empty: std::sync::atomic::AtomicBool,
}
async fn external(
    State(data): State<Arc<External>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path().to_owned();
    let payload = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    data.seen
        .lock()
        .unwrap()
        .push((path.clone(), headers.clone(), payload));
    if headers
        .get("authorization")
        .is_some_and(|v| v == "Bearer rejected-key")
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"echo rejected-key must never escape"})),
        )
            .into_response();
    }
    match path.as_str(){
  "/anysearch"=>Json(json!({"code":0,"data":{"results":[{"title":"AnySearch","url":"https://example.org/a","content":"正文","rank":1,"quality_score":0.8}],"metadata":{"total_results":1}}})).into_response(),
  "/free"=>Json(json!({"results":if data.free_empty.load(std::sync::atomic::Ordering::SeqCst){json!([])}else{json!([{"title":"Free","url":"https://example.org/free","content":"免费结果"}])}})).into_response(),
  "/tavily"=>Json(json!({"results":[{"title":"Tavily","url":"https://example.org/t","content":"摘要"}]})).into_response(),
  "/brave"=>Json(json!({"web":{"results":[{"title":"Brave","url":"https://example.org/b","description":"摘要"}]}})).into_response(),
  "/serper"=>Json(json!({"organic":[{"title":"Serper","link":"https://example.org/s","snippet":"摘要"}]})).into_response(),
  "/html"=>([("content-type","text/html")],format!("<html><head><title>fixture</title><script>隐藏材料</script></head><article><h1>文章</h1><p>{}</p><a href='/more'>继续阅读</a><p>TAIL-CONTINUATION</p></article></html>","长正文".repeat(300))).into_response(),
  "/json"=>Json(json!({"answer":42})).into_response(),
  "/private"=>(StatusCode::FOUND,[("location","http://127.0.0.1/private-target")]).into_response(),
  "/see-other"=>(StatusCode::SEE_OTHER,[("location","/should-not-follow-303")]).into_response(),
  "/rate"=>(StatusCode::TOO_MANY_REQUESTS,[("retry-after","2")],Json(json!({"error":"limit"}))).into_response(),
  "/auth-redirect"=>(StatusCode::FOUND,[("location","/should-not-receive-key")]).into_response(),
  "/oversized"=>([("content-type","text/plain"),("content-length","8388609")],"x".repeat(8*1024*1024+1)).into_response(),
  "/blocked-page"=>(StatusCode::FORBIDDEN,([("content-type","text/html")],"<html><title>Blocked</title><body>CAPTCHA-BLOCK-MARKER</body></html>")).into_response(),
  _ if path.starts_with("/redirect/")=>{let remaining=path.trim_start_matches("/redirect/").parse::<usize>().unwrap();if remaining==0{Json(json!({"done":true})).into_response()}else{(StatusCode::FOUND,[("location",format!("/redirect/{}",remaining-1))]).into_response()}},
  _=>(StatusCode::NOT_FOUND,"unknown fixture").into_response(),
 }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    model: Arc<Model>,
    external: Arc<External>,
    address: std::net::SocketAddr,
    url: String,
    app: tokio::task::JoinHandle<()>,
    fixture: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn new() -> Self {
        let home = std::env::temp_dir().join(format!("lingxi-r07-web-{}", random_token_suffix()));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            json!({"primary_agent_id":"a","search_provider":"auto","custom":{"keep":true}})
                .to_string(),
        )
        .unwrap();
        let external_data = Arc::new(External::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .fallback(external)
            .with_state(external_data.clone());
        let fixture = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let clock = Arc::new(ManualClock::new(1_800_000_000_000));
        let model = Arc::new(Model::default());
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ApprovalService::new(clock.clone())),
            clock.clone(),
            SchemaBudget::default(),
            300000,
            1000,
        ));
        // 显式使用正式目录，保留先读说明再调度的真实模型调用链。
        lingxi_service::toolcatalog_bridge::register(
            gateway.registry(),
            &gateway,
            &SchemaBudget::default(),
        );
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                assemble_session_file_tools: true,
                clock,
                turn_provider: Some(model.clone()),
                tool_gateway: Some(gateway.clone()),
                network_plane: Some(NetworkPlane::direct_isolated()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        // 不指定workspace、不手工注册文件工具；明确装配后只提供封闭根的read/materialize。
        for name in ["read", "materialize"] {
            assert!(gateway
                .registry()
                .describe(
                    &lingxi_kernel::toolcatalog::tool_target_id(
                        &lingxi_kernel::toolcatalog::ToolOrigin::FirstParty,
                        name
                    )
                    .unwrap()
                )
                .is_ok());
        }
        for name in ["write", "edit", "exec_command"] {
            assert!(gateway
                .registry()
                .describe(
                    &lingxi_kernel::toolcatalog::tool_target_id(
                        &lingxi_kernel::toolcatalog::ToolOrigin::FirstParty,
                        name
                    )
                    .unwrap()
                )
                .is_err());
        }
        for sid in ["web-session", "web-other"] {
            state
                .session_admin()
                .create_session(
                    sid,
                    "a",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "网页验证",
                    Some("read_only"),
                    &[],
                    false,
                    1_800_000_000_000,
                )
                .await
                .unwrap();
        }
        let endpoints = [
            ("anysearch", "anysearch"),
            ("anysearch_free", "free"),
            ("tavily", "tavily"),
            ("brave", "brave"),
            ("serper", "serper"),
        ]
        .into_iter()
        .map(|(p, path)| (p.into(), format!("http://{address}/{path}")))
        .collect();
        state
            .web_tools()
            .set_fixture_http(endpoints, Some(address))
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = lingxi_service::build_router(state.clone());
        let app = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            home,
            state,
            model,
            external: external_data,
            address,
            url,
            app,
            fixture,
        }
    }
    fn preferences(&self, value: Value) {
        std::fs::write(self.home.join("user/preferences.json"), value.to_string()).unwrap();
    }
    fn saved(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.home.join("user/preferences.json")).unwrap())
            .unwrap()
    }
    fn fetch_url(&self, path: &str) -> String {
        format!(
            "http://r07-web-fixture.invalid:{}{path}",
            self.address.port()
        )
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut r = reqwest::Client::new()
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(self.state.auth().local_token());
        if let Some(body) = body {
            r = r.json(&body);
        }
        let r = r.send().await.unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap())
    }
    async fn submit(&self, name: &str, args: Value) -> String {
        self.submit_for("web-session", name, args).await
    }
    async fn submit_for(&self, session: &str, name: &str, args: Value) -> String {
        let principal = self
            .state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", self.state.auth().local_token())),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        self.state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &principal,
                session,
                &ExecuteSubmission::plain(&json!({"tool":name,"args":args}).to_string()),
                1_800_000_000_000,
            )
            .await
            .unwrap()
            .run_id
    }
    async fn run(&self, name: &str, args: Value) -> ToolOutcome {
        self.run_for("web-session", name, args).await
    }
    async fn run_for(&self, session: &str, name: &str, args: Value) -> ToolOutcome {
        let run_id = self.submit_for(session, name, args).await;
        tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let r = self
                    .state
                    .storage()
                    .load_run(&RunId::new(&run_id))
                    .await
                    .unwrap()
                    .unwrap();
                if r.status.is_terminal() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("真实网页工具Run必须终结");
        self.model
            .results
            .lock()
            .unwrap()
            .remove(&run_id)
            .expect("模型必须收到正式工具结果")
    }
    async fn close(self) {
        let report = self
            .state
            .background()
            .drain_within(Duration::from_secs(5))
            .await;
        assert!(report.unconfirmed.is_empty(), "真实网页任务必须确认回收");
        self.app.abort();
        self.fixture.abort();
        let _ = self.app.await;
        let _ = self.fixture.await;
        drop(self.state);
        let _ = std::fs::remove_dir_all(self.home);
    }
}
fn success(outcome: ToolOutcome) -> lingxi_kernel::ports::ToolSuccess {
    match outcome {
        ToolOutcome::Success { result } => result,
        other => panic!("正式工具意外失败：{other:?}"),
    }
}
fn failure(outcome: ToolOutcome, code: &str) {
    match outcome {
        ToolOutcome::Failed { error } => assert_eq!(error.details.unwrap()["errorCode"], code),
        other => panic!("应明确失败：{other:?}"),
    }
}

#[tokio::test]
async fn all_five_http_providers_use_real_gateway_and_distinct_authentication() {
    let h = Harness::new().await;
    for provider in ["anysearch", "tavily", "brave", "serper", "anysearch_free"] {
        h.preferences(json!({"search_provider":provider,"search_api_keys":{"anysearch":"any-key","tavily":"tav-key","brave":"brave-key","serper":"serper-key"}}));
        let result = success(
            h.run("web_search", json!({"query":"本地响应契约","maxResults":1}))
                .await,
        );
        let details = result.details.unwrap();
        assert_eq!(details["provider"], provider);
        assert_eq!(details["source_type"], "api");
        assert_eq!(details["results"].as_array().unwrap().len(), 1);
    }
    {
        let seen = h.external.seen.lock().unwrap();
        assert_eq!(seen.len(), 5);
        assert_eq!(seen[0].1["authorization"], "Bearer any-key");
        assert_eq!(seen[0].2["query"], "本地响应契约");
        assert_eq!(seen[1].1["authorization"], "Bearer tav-key");
        assert_eq!(seen[2].1["x-subscription-token"], "brave-key");
        assert_eq!(seen[3].1["x-api-key"], "serper-key");
        assert!(seen[4].1.get("authorization").is_none());
    }
    h.close().await;
}
#[tokio::test]
async fn verification_real_http_masks_same_provider_and_only_persists_after_success() {
    let h = Harness::new().await;
    h.preferences(json!({"search_provider":"auto","search_api_keys":{"tavily":"existing-key","brave":"other-key"},"custom":{"keep":true}}));
    let before = h.saved();
    let (status, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/search/verify",
            Some(json!({"provider":"tavily","api_key":"rejected-key","search_provider":"auto"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], false);
    assert!(!result.to_string().contains("rejected-key"));
    assert_eq!(h.saved(), before);
    let (_, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/search/verify",
            Some(json!({"provider":"serper","api_key":"********","search_provider":"auto"})),
        )
        .await;
    assert_eq!(result["ok"], false);
    assert_eq!(
        h.external.seen.lock().unwrap().len(),
        1,
        "跨供应商掩码不能发出验证请求"
    );
    let (status, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/search/verify",
            Some(json!({"provider":"tavily","api_key":"********","search_provider":"auto"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    assert_eq!(h.saved()["search_provider"], "auto");
    assert_eq!(h.saved()["search_api_key"], "");
    assert_eq!(h.saved()["search_api_keys"]["brave"], "other-key");
    assert_eq!(h.saved()["custom"], json!({"keep":true}));
    {
        let seen = h.external.seen.lock().unwrap();
        assert_eq!(seen[1].1["authorization"], "Bearer existing-key");
        assert_eq!(seen[1].2["query"], "test");
        assert_eq!(seen[1].2["max_results"], 1);
    }
    let (_, masked) = h
        .request(reqwest::Method::GET, "/lingxi/v1/search/config", None)
        .await;
    assert_eq!(masked["api_keys"]["tavily"], "********");
    assert!(
        !masked.to_string().contains("existing-key") && !masked.to_string().contains("other-key")
    );
    h.close().await;
}
#[tokio::test]
async fn public_fetch_keeps_five_hop_guard_and_registers_readable_full_session_file() {
    let h = Harness::new().await;
    let output = success(
        h.run(
            "web_fetch",
            json!({"url":h.fetch_url("/html"),"maxLength":200}),
        )
        .await,
    );
    let details = output.details.unwrap();
    assert_eq!(details["truncated"], true);
    let file = &details["sessionFile"];
    assert!(file["fileId"].as_str().unwrap().starts_with("sf_"));
    assert_eq!(file["sessionId"], "web-session");
    let full = std::fs::read_to_string(file["filePath"].as_str().unwrap()).unwrap();
    assert!(full.contains("TAIL-CONTINUATION"));
    assert!(!full.contains("隐藏材料"));
    assert!(
        matches!(
            h.run("read", json!({"path":file["filePath"]})).await,
            ToolOutcome::Failed { .. }
        ),
        "直接缓存路径不能绕过文件闸"
    );
    assert!(
        matches!(
            h.run_for("web-other", "materialize", json!({"fileId":file["fileId"]}))
                .await,
            ToolOutcome::Failed { .. }
        ),
        "同一所有者的另一个聊天也不能冒用fileId"
    );
    let materialized = success(h.run("materialize", json!({"fileId":file["fileId"]})).await);
    let path = materialized.details.unwrap()["filePath"].clone();
    assert_eq!(path, file["filePath"]);
    let read = success(h.run("read", json!({"path":path})).await);
    assert!(
        format!("{:?}", read.content).contains("TAIL-CONTINUATION"),
        "真正materialize后的正式read必须看到全文尾部"
    );
    let target = PathBuf::from(path.as_str().unwrap());
    let sibling = target.parent().unwrap().join("not-granted.txt");
    std::fs::write(&sibling, "不能读同目录其他文件").unwrap();
    assert!(
        matches!(
            h.run("read", json!({"path":sibling})).await,
            ToolOutcome::Failed { .. }
        ),
        "materialize不能授予整个目录"
    );
    std::fs::write(&target, "NEW-FILE-VERSION").unwrap();
    assert!(
        matches!(
            h.run("read", json!({"path":path})).await,
            ToolOutcome::Failed { .. }
        ),
        "旧内容指纹不能授权改变后的文件"
    );
    success(h.run("materialize", json!({"fileId":file["fileId"]})).await);
    let changed = success(h.run("read", json!({"path":path})).await);
    assert!(format!("{:?}", changed.content).contains("NEW-FILE-VERSION"));
    let replacement = target.with_extension("replacement");
    std::fs::write(&replacement, "REPLACED-INODE").unwrap();
    std::fs::rename(&replacement, &target).unwrap();
    assert!(
        matches!(
            h.run("read", json!({"path":path})).await,
            ToolOutcome::Failed { .. }
        ),
        "旧授权不能读取被置换的文件"
    );
    success(h.run("materialize", json!({"fileId":file["fileId"]})).await);
    success(h.run("read", json!({"path":path})).await);
    let json = success(
        h.run("web_fetch", json!({"url":h.fetch_url("/redirect/5")}))
            .await,
    );
    assert_eq!(json.details.unwrap()["truncated"], false);
    failure(
        h.run("web_fetch", json!({"url":h.fetch_url("/redirect/6")}))
            .await,
        "WEB_FETCH_REDIRECT_LIMIT",
    );
    let before_303 = h.external.seen.lock().unwrap().len();
    failure(
        h.run("web_fetch", json!({"url":h.fetch_url("/see-other")}))
            .await,
        "WEB_FETCH_HTTP_303",
    );
    assert_eq!(
        h.external.seen.lock().unwrap().len(),
        before_303 + 1,
        "现役303不跟随规则须保留"
    );
    let count = h.external.seen.lock().unwrap().len();
    failure(
        h.run("web_fetch", json!({"url":"http://127.0.0.1/private"}))
            .await,
        "WEB_FETCH_SSRF_BLOCKED",
    );
    assert_eq!(h.external.seen.lock().unwrap().len(), count);
    failure(
        h.run("web_fetch", json!({"url":h.fetch_url("/private")}))
            .await,
        "WEB_FETCH_SSRF_BLOCKED",
    );
    assert_eq!(h.external.seen.lock().unwrap().len(), count + 1);
    failure(
        h.run("web_fetch", json!({"url":h.fetch_url("/oversized")}))
            .await,
        "WEB_FETCH_ERROR",
    );
    h.close().await;
}
#[tokio::test]
async fn provider_auth_redirect_and_rate_limit_keep_real_error_diagnostics() {
    let h = Harness::new().await;
    h.preferences(json!({"search_provider":"tavily","search_api_keys":{"tavily":"existing-key"}}));
    h.state
        .web_tools()
        .set_fixture_http(
            BTreeMap::from([(
                "tavily".into(),
                format!("http://{}/auth-redirect", h.address),
            )]),
            Some(h.address),
        )
        .unwrap();
    failure(
        h.run("web_search", json!({"query":"no redirect"})).await,
        "WEB_SEARCH_ERROR",
    );
    assert_eq!(h.external.seen.lock().unwrap().len(), 1);
    h.state
        .web_tools()
        .set_fixture_http(
            BTreeMap::from([("tavily".into(), format!("http://{}/rate", h.address))]),
            Some(h.address),
        )
        .unwrap();
    match h.run("web_search", json!({"query":"rate limit"})).await {
        ToolOutcome::Failed { error } => {
            let details = error.details.unwrap();
            assert_eq!(details["errorCode"], "WEB_SEARCH_RATE_LIMITED");
            assert_eq!(details["retryAfterMs"], 2000);
        }
        other => panic!("应保留限流错误 {other:?}"),
    };
    h.close().await;
}

#[tokio::test]
async fn auto_records_each_real_http_attempt_then_all_three_browser_page_diagnostics() {
    let h = Harness::new().await;
    h.preferences(json!({"search_provider":"auto","search_api_keys":{"anysearch":"rejected-key"}}));
    h.external
        .free_empty
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let pages = Arc::new(BrowserPages::default());
    h.state.web_tools().bind_browser_search(pages.clone());
    let result = success(h.run("web_search", json!({"query":"本地浏览器回退"})).await);
    let details = result.details.unwrap();
    assert_eq!(details["provider"], "duckduckgo_browser");
    assert_eq!(details["source_type"], "browser");
    let attempts = details["diagnostics"]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 5);
    assert_eq!(
        attempts
            .iter()
            .map(|a| a["provider"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "anysearch",
            "anysearch_free",
            "bing_browser",
            "google_browser",
            "duckduckgo_browser"
        ]
    );
    assert_eq!(attempts[0]["error_type"], "auth");
    assert_eq!(attempts[1]["status"], "empty");
    assert_eq!(attempts[2]["error_type"], "blocked");
    assert_eq!(attempts[3]["error_type"], "extraction_failed");
    assert_eq!(attempts[4]["status"], "ok");
    assert_eq!(pages.seen.lock().unwrap().len(), 3);
    assert_eq!(h.external.seen.lock().unwrap().len(), 2);
    assert_eq!(h.saved()["search_provider"], "auto");
    h.close().await;
}

#[tokio::test]
async fn auto_all_failed_is_explicit_and_fixed_provider_never_silently_falls_back() {
    let h = Harness::new().await;
    h.preferences(json!({"search_provider":"auto"}));
    h.external
        .free_empty
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let pages = Arc::new(BrowserPages {
        seen: Mutex::new(vec![]),
        all_failed: true,
        ..BrowserPages::default()
    });
    h.state.web_tools().bind_browser_search(pages.clone());
    let result = success(h.run("web_search", json!({"query":"本地全部失败"})).await);
    let details = result.details.unwrap();
    assert_eq!(details["diagnostics"]["status"], "all_failed");
    assert_eq!(
        details["diagnostics"]["attempts"].as_array().unwrap().len(),
        4
    );
    assert!(format!("{:?}", result.content).contains("All configured search providers failed"));
    h.preferences(json!({"search_provider":"tavily"}));
    failure(
        h.run("web_search", json!({"query":"固定供应商缺key"}))
            .await,
        "WEB_SEARCH_AUTH",
    );
    assert_eq!(h.external.seen.lock().unwrap().len(), 1);
    assert_eq!(pages.seen.lock().unwrap().len(), 3);
    h.close().await;
}

#[tokio::test]
async fn public_fetch_proxy_pin_limitation_is_explicit_without_sending_any_request() {
    use lingxi_adapters::models::network::{NetworkPolicy, ProxyPolicy};
    let h = Harness::new().await;
    h.state.network_plane().unwrap().apply(NetworkPolicy {
        proxy: ProxyPolicy::Manual {
            http: Some(format!("http://{}", h.address)),
            https: Some(format!("http://{}", h.address)),
            no_proxy: String::new(),
        },
        trusted_ca_pem: None,
    });
    // 字面公网IP不发DNS查询；当前实现必须在建立代理或源连接前明确拒绝。
    failure(
        h.run("web_fetch", json!({"url":"https://8.8.8.8/never-sent"}))
            .await,
        "WEB_FETCH_PROXY_PIN_UNAVAILABLE",
    );
    assert!(
        h.external.seen.lock().unwrap().is_empty(),
        "代理端也不得收到尝试，不能把拒绝悄悄降为直连"
    );
    h.close().await;
}

#[tokio::test]
async fn actual_search_cancel_waits_for_auxiliary_cleanup_and_stops_fallback_chain() {
    let h = Harness::new().await;
    h.preferences(json!({"search_provider":"auto"}));
    h.external
        .free_empty
        .store(true, std::sync::atomic::Ordering::Release);
    let pages = Arc::new(BrowserPages::default());
    pages
        .wait_for_cancel
        .store(true, std::sync::atomic::Ordering::Release);
    h.state.web_tools().bind_browser_search(pages.clone());
    let run = h
        .submit("web_search", json!({"query":"取消临时搜索页面"}))
        .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while pages.seen.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("必须先真正派发临时搜索页");
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
    h.state
        .sessions()
        .cancel_run_for(h.state.storage().as_ref(), h.state.runs(), &principal, &run)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let record = h
                .state
                .storage()
                .load_run(&RunId::new(&run))
                .await
                .unwrap()
                .unwrap();
            if record.status.is_terminal() {
                assert_eq!(record.status, lingxi_protocol::RunStatus::Cancelled);
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("真实搜索Run必须沿统一取消终结");
    assert!(
        pages
            .cleanup_confirmed
            .load(std::sync::atomic::Ordering::Acquire),
        "Run终结前必须确认临时页回收"
    );
    assert_eq!(
        pages.seen.lock().unwrap().len(),
        1,
        "取消后不得尝试Google/DuckDuckGo"
    );
    assert_eq!(
        h.external.seen.lock().unwrap().len(),
        1,
        "只有取消前的free尝试"
    );
    h.close().await;
}

/// R09-T05 逐断言：WEB-FETCH A2/A3——阻断页不当正常内容；URL 无效/重定向/网络失败显式错误且不写工作区。
#[tokio::test]
async fn web_fetch_blocked_page_invalid_urls_and_redirect_failures_are_explicit_without_writes() {
    let h = Harness::new().await;
    let files_before = h
        .state
        .storage()
        .query_one_text("SELECT COUNT(*) FROM session_files", vec![])
        .await
        .unwrap();
    let workspace_before = std::fs::read_dir(&h.home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    // A2：HTTP 阻断页（403 captcha）必须显式失败，不能当正文/摘要返回。
    failure(
        h.run("web_fetch", json!({"url": h.fetch_url("/blocked-page")}))
            .await,
        "WEB_FETCH_HTTP_403",
    );
    // A3：非法 URL（scheme/凭据/缺 host）与重定向上限显式失败。
    failure(
        h.run("web_fetch", json!({"url": "javascript:alert(1)"}))
            .await,
        "WEB_FETCH_INVALID_SCHEME",
    );
    failure(
        h.run("web_fetch", json!({"url": "file:///etc/passwd"}))
            .await,
        "WEB_FETCH_INVALID_SCHEME",
    );
    failure(
        h.run("web_fetch", json!({"url": "not a url"})).await,
        "WEB_FETCH_INVALID_URL",
    );
    failure(
        h.run("web_fetch", json!({"url": h.fetch_url("/redirect/6")}))
            .await,
        "WEB_FETCH_REDIRECT_LIMIT",
    );
    // A3：失败不自动写工作区/不登记会话文件。
    let files_after = h
        .state
        .storage()
        .query_one_text("SELECT COUNT(*) FROM session_files", vec![])
        .await
        .unwrap();
    assert_eq!(
        files_after, files_before,
        "失败的 web_fetch 不得登记会话文件"
    );
    let workspace_after = std::fs::read_dir(&h.home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(
        workspace_after, workspace_before,
        "失败的 web_fetch 不得写工作区"
    );
    h.close().await;
}
