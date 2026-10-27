//! 39 个内置品牌的真实服务矩阵；只替换供应商的 HTTP 响应，不替换发布、凭证、网关和记账。
use axum::{
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json, Router,
};
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_kernel::{
    ports::StoragePort as _,
    usage::{CallOutcome, ModelCallUsageRecord, ModelUsageQuery},
};
use lingxi_service::{
    prepare_layout, provider_probe::builtin_provider_definitions, HomeSource, NetworkMode,
    ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

#[derive(Clone)]
struct Brand {
    id: String,
    protocol: String,
    model: String,
    key: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
}
impl Brand {
    fn endpoint(&self, origin: &str) -> String {
        let version = if self.protocol == "google-generative-ai" {
            "v1beta"
        } else {
            "v1"
        };
        format!("{origin}/{}/{version}", self.id)
    }
    fn path(&self) -> String {
        match self.protocol.as_str() {
            "openai-completions" => format!("/{}/v1/chat/completions", self.id),
            "openai-responses" => format!("/{}/v1/responses", self.id),
            "anthropic-messages" => format!("/{}/v1/messages", self.id),
            "google-generative-ai" => format!(
                "/{}/v1beta/models/{}:streamGenerateContent?alt=sse",
                self.id, self.model
            ),
            other => panic!("内置品牌 {} 声明了未接通的聊天协议 {other}", self.id),
        }
    }
    fn response(&self, stream: bool) -> Response {
        let text = format!("matrix-reply-{}", self.id);
        let input = self.input_tokens;
        let output = self.output_tokens;
        match self.protocol.as_str() {
            "openai-completions" if !stream => Json(json!({"id":format!("matrix-{}",self.id),"model":self.model,"choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output}})).into_response(),
            "anthropic-messages" if !stream => Json(json!({"id":format!("matrix-{}",self.id),"type":"message","role":"assistant","model":self.model,"content":[{"type":"text","text":text}],"stop_reason":"end_turn","usage":{"input_tokens":input,"output_tokens":output}})).into_response(),
            "openai-completions" => {
                let first=json!({"id":format!("matrix-{}",self.id),"model":self.model,"choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]});
                let last=json!({"id":format!("matrix-{}",self.id),"model":self.model,"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output}});
                ([("content-type","text/event-stream")],format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n")).into_response()
            }
            "openai-responses" => {
                let first=json!({"type":"response.created","response":{"id":format!("matrix-{}",self.id),"status":"in_progress"}});
                let delta=json!({"type":"response.output_text.delta","delta":text});
                let last=json!({"type":"response.completed","response":{"id":format!("matrix-{}",self.id),"status":"completed","model":self.model,"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":input,"output_tokens":output}}});
                ([("content-type","text/event-stream")],format!("data: {first}\n\ndata: {delta}\n\ndata: {last}\n\n")).into_response()
            }
            "anthropic-messages" => {
                let events = [
                    ("message_start",json!({"type":"message_start","message":{"id":format!("matrix-{}",self.id),"type":"message","role":"assistant","model":self.model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":input,"output_tokens":0}}})),
                    ("content_block_start",json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
                    ("content_block_delta",json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}})),
                    ("content_block_stop",json!({"type":"content_block_stop","index":0})),
                    ("message_delta",json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":output}})),
                    ("message_stop",json!({"type":"message_stop"})),
                ];
                let events=events.iter().map(|(event,value)|format!("event: {event}\ndata: {value}\n\n")).collect::<String>();
                ([("content-type","text/event-stream")],events).into_response()
            }
            "google-generative-ai" => {
                let event=json!({"candidates":[{"content":{"role":"model","parts":[{"text":text}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":input,"candidatesTokenCount":output}});
                ([("content-type","text/event-stream")],format!("data: {event}\n\n")).into_response()
            }
            _ => (StatusCode::BAD_REQUEST, "无此协议的替身").into_response(),
        }
    }
}

#[derive(Clone)]
struct Arrival {
    method: Method,
    uri: String,
    headers: HeaderMap,
    body: Value,
}
struct Platform {
    brands: BTreeMap<String, Brand>,
    arrivals: Mutex<Vec<Arrival>>,
}
async fn platform_response(
    State(platform): State<Arc<Platform>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let path = uri.path_and_query().unwrap().as_str().to_owned();
    platform.arrivals.lock().unwrap().push(Arrival {
        method: method.clone(),
        uri: path.clone(),
        headers: headers.clone(),
        body: body.clone(),
    });
    let id = uri.path().split('/').nth(1).unwrap_or_default();
    let Some(brand) = platform.brands.get(id) else {
        return (StatusCode::NOT_FOUND, "未知品牌，不返回假成功").into_response();
    };
    if method != Method::POST || path != brand.path() {
        return (
            StatusCode::BAD_REQUEST,
            "品牌实际请求路径与其声明协议不一致",
        )
            .into_response();
    }
    // 只在自有供应商外端核对凭据；产品内部保存/凭证/网关/记账均保持真实。
    let auth_header = match brand.protocol.as_str() {
        "anthropic-messages" => "x-api-key",
        "google-generative-ai" => "x-goog-api-key",
        _ => "authorization",
    };
    if let Some(key) = &brand.key {
        let expected = if auth_header == "authorization" {
            format!("Bearer {key}")
        } else {
            key.clone()
        };
        if !headers
            .get(auth_header)
            .is_some_and(|value| value == expected.as_str())
        {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error":{"code":"invalid_api_key","type":"authentication_error","message":"自有供应商外端拒绝缺失或错误凭据"}}))).into_response();
        }
    }
    brand.response(body["stream"].as_bool().unwrap_or(true))
}

struct Harness {
    home: PathBuf,
    state: ServiceState,
    base: String,
    origin: String,
    platform: Arc<Platform>,
    stop: tokio::sync::oneshot::Sender<()>,
    server:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
    platform_task: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot(brands: &[Brand]) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-provider-matrix-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
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
        let home = home.canonicalize().unwrap();
        // 保留失败现场路径；测试通过后才清理这个完全私有的临时目录。
        eprintln!("provider_matrix 现场：{}", home.display());
        let layout = prepare_layout(&home).unwrap();
        let plane = ModelPlaneConfig::from_value(json!({"providers":{},"models":{}})).unwrap();
        let clock = Arc::new(lingxi_service::inject::SystemClock);
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
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                model_gateway: Some(Arc::new(ConfigModelGateway::from_validated(plane))),
                credential_service: Some(credentials),
                network_plane: Some(NetworkPlane::direct_isolated()),
                model_plane_activation: Some(Arc::new(AtomicBool::new(false))),
                clock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let platform = Arc::new(Platform {
            brands: brands
                .iter()
                .cloned()
                .map(|brand| (brand.id.clone(), brand))
                .collect(),
            arrivals: Mutex::new(vec![]),
        });
        let app = Router::new()
            .fallback(platform_response)
            .with_state(platform.clone());
        let platform_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, address) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let server = tokio::spawn(async move {
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
        Self {
            home,
            state,
            base: format!("http://{}", address.await.unwrap()),
            origin,
            platform,
            stop,
            server,
            platform_task,
        }
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
            request = request.json(&body)
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw":text})),
        )
    }
    fn arrivals(&self) -> Vec<Arrival> {
        self.platform.arrivals.lock().unwrap().clone()
    }
    async fn usage(&self) -> Vec<ModelCallUsageRecord> {
        self.state
            .storage()
            .query_model_call_usage(ModelUsageQuery::default())
            .await
            .unwrap()
    }
    fn source(&self) -> Value {
        serde_json::from_slice(
            &std::fs::read(self.home.join("lingxi-service/models.json")).unwrap(),
        )
        .unwrap()
    }
    async fn close(self) {
        self.stop.send(()).expect("测试结束前服务不应提前退出");
        let outcome = self
            .server
            .await
            .expect("真实服务任务不应崩溃")
            .expect("真实服务必须正常退出");
        assert!(!outcome.drain_timed_out, "真实服务退出时不应放弃未完成连接");
        self.platform_task.abort();
        let _ = self.platform_task.await;
        self.state.storage().close().await.unwrap();
        std::fs::remove_dir_all(self.home).unwrap();
    }
}

fn assert_arrival(brand: &Brand, arrival: &Arrival) {
    assert_eq!(arrival.method, Method::POST, "{}", brand.id);
    assert_eq!(arrival.uri, brand.path(), "{}", brand.id);
    assert_eq!(
        arrival.headers["x-matrix-provider"].to_str().unwrap(),
        brand.id.as_str(),
        "{}",
        brand.id
    );
    let expected_header = match brand.protocol.as_str() {
        "anthropic-messages" => "x-api-key",
        "google-generative-ai" => "x-goog-api-key",
        _ => "authorization",
    };
    for header in ["authorization", "x-api-key", "x-goog-api-key"] {
        if header == expected_header {
            if let Some(key) = &brand.key {
                let expected = if header == "authorization" {
                    format!("Bearer {key}")
                } else {
                    key.clone()
                };
                // 不把测试密钥写进失败信息；只报品牌和错误的头名称。
                assert!(
                    arrival
                        .headers
                        .get(header)
                        .is_some_and(|v| v == expected.as_str()),
                    "{} 的 {} 未使用自身凭证",
                    brand.id,
                    header
                );
            } else {
                assert!(
                    arrival.headers.get(header).is_none(),
                    "{} 应保持 AuthNone",
                    brand.id
                )
            }
        } else {
            assert!(
                arrival.headers.get(header).is_none(),
                "{} 不应带 {}",
                brand.id,
                header
            )
        }
    }
    if brand.protocol == "google-generative-ai" {
        assert!(
            arrival.body["contents"].is_array(),
            "{} 必须发 Google 请求正文",
            brand.id
        );
    } else {
        assert_eq!(arrival.body["model"], brand.model, "{}", brand.id)
    }
    assert!(
        arrival.body.to_string().contains("Reply exactly OK."),
        "{} 未发送真实健康提示",
        brand.id
    );
}

#[tokio::test]
async fn every_builtin_chat_identity_dispatches_its_own_protocol_credentials_model_and_usage() {
    let definitions = builtin_provider_definitions();
    assert_eq!(definitions.len(), 39);
    let unique = definitions
        .iter()
        .map(|d| d["id"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), 39);
    assert_eq!(
        definitions
            .iter()
            .filter(|d| d["authType"] == "api-key")
            .count(),
        35
    );
    assert_eq!(
        definitions
            .iter()
            .filter(|d| d["authType"] == "oauth")
            .count(),
        2
    );
    assert_eq!(
        definitions
            .iter()
            .filter(|d| d["authType"] == "none")
            .count(),
        2
    );
    // 35 个聊天身份 = 34 个密钥身份 + 原本免密钥的 Ollama；另有两个 OAuth、两个语音。
    let brands = definitions
        .iter()
        .filter(|d| {
            d["authType"] != "oauth"
                && !matches!(
                    d["id"].as_str().unwrap(),
                    "system-speech" | "volcengine-speech"
                )
        })
        .enumerate()
        .map(|(index, d)| {
            let id = d["id"].as_str().unwrap().to_owned();
            Brand {
                model: format!("matrix-model-{id}"),
                key: (d["authType"] == "api-key").then(|| format!("matrix-private-key-{id}")),
                protocol: d["api"].as_str().unwrap().to_owned(),
                id,
                input_tokens: 100 + index as u64,
                output_tokens: 10 + index as u64,
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(brands.len(), 35);
    let h = Harness::boot(&brands).await;
    let mut providers = Map::new();
    for brand in &brands {
        providers.insert(brand.id.clone(),json!({"api":brand.protocol,"base_url":brand.endpoint(&h.origin),"api_key":brand.key.clone().unwrap_or_default(),"headers":{"X-Matrix-Provider":brand.id},"models":[{"id":brand.model,"inputs":["text"]}]}));
    }
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/settings/config",
            Some(json!({"providers":providers})),
        )
        .await;
    assert_eq!(status, 200, "35 品牌必须通过真实全局 HTTP 保存：{value}");
    assert!(h.arrivals().is_empty());
    assert!(h.usage().await.is_empty());
    for (index, brand) in brands.iter().enumerate() {
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/models/health",
                Some(json!({"provider":brand.id,"modelId":brand.model})),
            )
            .await;
        assert_eq!(status, 200, "{} 真实健康调用失败：{value}", brand.id);
        assert_eq!(
            value["ok"], true,
            "{} 未消费平台的协议响应：{value}",
            brand.id
        );
        assert_eq!(value["status"], 200, "{}", brand.id);
        let arrivals = h.arrivals();
        assert_eq!(
            arrivals.len(),
            index + 1,
            "{} 必须恰好发出一次自身请求",
            brand.id
        );
        assert_arrival(brand, &arrivals[index]);
        let rows = h.usage().await;
        assert_eq!(
            rows.len(),
            index + 1,
            "{} 必须产生自己的真实记账行",
            brand.id
        );
        let matching = rows
            .iter()
            .filter(|row| row.provider == brand.id && row.model == brand.model)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "{} 不可复用其他品牌的结果", brand.id);
        let row = matching[0];
        assert_eq!(row.protocol, brand.protocol, "{}", brand.id);
        assert_eq!(row.purpose, "model_health");
        assert_eq!(row.origin, "health_check");
        assert_eq!(row.outcome, CallOutcome::Succeeded, "{}", brand.id);
        assert_eq!(row.transport_attempts, Some(1), "{}", brand.id);
        assert!(row.session_id.is_none() && row.run_id.is_none());
        let usage = row
            .usage
            .as_ref()
            .expect("平台响应中明确报告的用量必须落库");
        assert_eq!(
            usage.input_tokens,
            Some(brand.input_tokens),
            "{} 的独立响应输入用量",
            brand.id
        );
        assert_eq!(
            usage.output_tokens,
            Some(brand.output_tokens),
            "{} 的独立响应输出用量",
            brand.id
        );
        eprintln!(
            "provider_matrix 已核实 provider={} protocol={} model={} call_id={} usage={}/{}",
            brand.id,
            brand.protocol,
            brand.model,
            row.model_call_id,
            brand.input_tokens,
            brand.output_tokens
        );
    }
    let (status, summary) = h
        .request(reqwest::Method::GET, "/lingxi/v1/providers/summary", None)
        .await;
    assert_eq!(status, 200, "{summary}");
    for brand in &brands {
        if let Some(key) = &brand.key {
            assert!(
                !summary.to_string().contains(key),
                "{} 的密钥不得出现在普通读取响应",
                brand.id
            )
        }
    }
    // 编辑 A 的目录不能改变 B 的配置或凭证，并再次实调 B 核对到站事实。
    let a = &brands[0];
    let b = &brands[1];
    let before_b = h.source()["providers"][&b.id].clone();
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            &format!("/lingxi/v1/providers/{}/models/{}", a.id, a.model),
            Some(json!({"name":"仅修改A","contextWindow":8192,"inputs":["text"]})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert!(
        h.source()["providers"][&b.id] == before_b,
        "编辑 A 不得改变 B 的实际来源或凭证"
    );
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/models/health",
            Some(json!({"provider":b.id,"modelId":b.model})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(h.arrivals().len(), 36);
    assert_arrival(b, h.arrivals().last().unwrap());
    let rows = h.usage().await;
    assert_eq!(rows.len(), 36);
    assert_eq!(
        rows.iter()
            .map(|row| &row.model_call_id)
            .collect::<BTreeSet<_>>()
            .len(),
        36,
        "每次真实调用都必须有独立记账身份"
    );
    let b_rows = rows
        .iter()
        .filter(|row| row.provider == b.id && row.model == b.model)
        .collect::<Vec<_>>();
    assert_eq!(b_rows.len(), 2, "编辑A后再次调用B也必须产生独立记账");
    assert!(b_rows
        .iter()
        .all(|row| row.outcome == CallOutcome::Succeeded
            && row.transport_attempts == Some(1)
            && row
                .usage
                .as_ref()
                .is_some_and(|usage| usage.input_tokens == Some(b.input_tokens)
                    && usage.output_tokens == Some(b.output_tokens))));
    // 未登记品牌、未登记模型、已删除模型均不得向外部平台发请求或虚增用量。
    let (status, value) = h
        .request(
            reqwest::Method::DELETE,
            &format!("/lingxi/v1/providers/{}/models/{}", a.id, a.model),
            None,
        )
        .await;
    assert_eq!(status, 200, "{value}");
    for (provider, model) in [
        ("not-registered-brand", "not-registered-model"),
        (b.id.as_str(), "not-registered-model"),
        (a.id.as_str(), a.model.as_str()),
    ] {
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/models/health",
                Some(json!({"provider":provider,"modelId":model})),
            )
            .await;
        assert_eq!(status, 400, "{provider}/{model} 必须明确拒绝：{value}");
        assert_eq!(h.arrivals().len(), 36);
        assert_eq!(h.usage().await.len(), 36);
    }
    // 未登录 OAuth 的声明目录不代表登录或真实生成；不执行任何远端登录流程。
    for id in ["openai-codex-oauth", "xai-oauth"] {
        assert_eq!(
            summary["providers"][id]["logged_in"], false,
            "{id} 不可伪造已登录"
        );
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/providers/fetch-models",
                Some(json!({"name":id})),
            )
            .await;
        assert_eq!(status, 200, "{id}：{value}");
        assert_eq!(value["source"], "hana-catalog");
        assert!(value["models"].is_array());
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/models/health",
                Some(json!({"provider":id,"modelId":"not-logged-in-model"})),
            )
            .await;
        assert_eq!(status, 400, "未登记未登录身份不可假生成：{value}");
    }
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/providers/test",
            Some(json!({"name":"openai-codex-oauth"})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["skipped"], true);
    assert_eq!(value["status"], 0);
    // 专用语音由 T06 的真实媒体链验证；本检查证明通用 HTTP 明确报告不适用。
    for id in ["system-speech", "volcengine-speech"] {
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/providers/test",
                Some(json!({"name":id})),
            )
            .await;
        assert_eq!(status, 200, "{id}：{value}");
        assert_eq!(value["ok"], false);
        assert_eq!(value["unavailable"], true);
        assert_eq!(value["status"], 0);
    }
    assert_eq!(
        h.arrivals().len(),
        36,
        "OAuth 声明/跳过与语音边界均不能假请求"
    );
    assert_eq!(
        h.usage().await.len(),
        36,
        "OAuth 声明/跳过与语音边界均不能假记账"
    );
    h.close().await;
}

// 每个测试名称绑定精确品牌；免密钥、运行拒绝与保存校验分别记录，不共享假结果。
async fn builtin_identity_rejects_invalid_requests(id: &str) {
    use lingxi_kernel::model_exchange::{ModelGatewayError, ModelOperation, ModelRouteRequest};
    let definition = builtin_provider_definitions()
        .into_iter()
        .find(|row| row["id"] == id)
        .expect("必须选择实际内置品牌");
    assert!(definition["authType"] == "api-key" || id == "ollama");
    let brand = Brand {
        id: id.into(),
        protocol: definition["api"].as_str().unwrap().into(),
        model: format!("matrix-boundary-{id}"),
        key: (definition["authType"] == "api-key").then(|| format!("matrix-required-key-{id}")),
        input_tokens: 19,
        output_tokens: 7,
    };
    let h = Harness::boot(std::slice::from_ref(&brand)).await;
    // 空key按原保存合同成为AuthNone，不伪称这是本地缺凭据配置错误。
    let (status, saved) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/settings/config",
            Some(json!({"providers":{(id):{
                "api":brand.protocol,"base_url":brand.endpoint(&h.origin),"api_key":"",
                "headers":{"X-Matrix-Provider":id},"models":[{"id":brand.model,"inputs":["text"]}]
            }}})),
        )
        .await;
    assert_eq!(status, 200, "{id}真实匿名配置应保存并发布：{saved}");
    assert_eq!(h.source()["providers"][id]["auth"]["kind"], "none", "{id}");
    assert!(h.arrivals().is_empty());
    assert!(h.usage().await.is_empty());
    let (status, health) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/models/health",
            Some(json!({"provider":id,"modelId":brand.model})),
        )
        .await;
    assert_eq!(status, 200, "{id}真实调用报告应保持HTTP合同：{health}");
    let rows = h.usage().await;
    assert_eq!(rows.len(), 1, "{id}必须由真实探测链结算一行");
    let row = &rows[0];
    assert_eq!(row.provider, id);
    assert_eq!(row.model, brand.model);
    assert_eq!(row.protocol, brand.protocol);
    assert_eq!(row.purpose, "model_health");
    assert_eq!(row.origin, "health_check");
    assert_eq!(row.transport_attempts, Some(1), "{id}缺凭据不能重投");
    assert!(row.session_id.is_none() && row.run_id.is_none());
    assert_eq!(h.arrivals().len(), 1, "{id}必须仅请求自己的外端一次");
    let mut keyless = brand.clone();
    keyless.key = None;
    assert_arrival(&keyless, &h.arrivals()[0]);
    if brand.key.is_some() {
        // 外端真实401由协议执行器转换为失败报告；不是保存层拒绝或假HTTP状态。
        assert_eq!(
            health["ok"], false,
            "{id}不能把外端401报成生成成功：{health}"
        );
        assert_eq!(health["status"], 0, "{id}保持健康探测失败的原返回合同");
        assert_eq!(
            health["code"], "unauthorized",
            "{id}必须保留实际401分类：{health}"
        );
        assert_eq!(row.outcome, CallOutcome::Failed);
        assert!(
            row.usage.is_none(),
            "{id}拒绝没有供应商用量时不能伪造token数"
        );
    } else {
        assert_eq!(id, "ollama", "仅原免密钥身份进入该分支");
        assert_eq!(health["ok"], true, "Ollama无key不能误报凭据失败：{health}");
        assert_eq!(health["status"], 200);
        assert_eq!(row.outcome, CallOutcome::Succeeded);
        let usage = row.usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, Some(brand.input_tokens));
        assert_eq!(usage.output_tokens, Some(brand.output_tokens));
    }
    eprintln!("provider_matrix_boundary provider={id} default_protocol={} keyless_external={} call_id={} physical=1 usage_rows=1", brand.protocol, if brand.key.is_some() {"401_rejected"} else {"200_allowed"}, row.model_call_id);
    let source_before = h.source();
    let generation_before = h.state.model_gateway().unwrap().upcoming_generation();
    for (label, model) in [
        ("empty_model", "".to_owned()),
        ("unregistered_model", format!("matrix-unregistered-{id}")),
    ] {
        let (status, rejected) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/models/health",
                Some(json!({"provider":id,"modelId":model})),
            )
            .await;
        assert_eq!(
            status, 400,
            "{id}/{label}必须在正式调用入口明确拒绝：{rejected}"
        );
        assert_eq!(h.arrivals().len(), 1, "{id}/{label}不得尝试默认或其它品牌");
        assert_eq!(
            h.usage().await.len(),
            1,
            "{id}/{label}尚未进入生成，不得虚增用量"
        );
        assert!(
            h.source() == source_before,
            "{id}/{label}读取拒绝不能改变来源"
        );
        assert_eq!(
            h.state.model_gateway().unwrap().upcoming_generation(),
            generation_before,
            "{id}/{label}不能改变发布代次"
        );
        eprintln!("provider_matrix_boundary provider={id} case={label} http=400 extra_physical=0 extra_usage=0");
    }
    // 非法协议只有保存层400证据，不能登记成实际生成拒绝。
    let (status, rejected) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/settings/config",
            Some(json!({"providers":{(id):{"api":"matrix-unknown-protocol"}}})),
        )
        .await;
    assert_eq!(status, 400, "{id}非法协议保存必须拒绝：{rejected}");
    assert!(h.source() == source_before, "{id}非法协议不得写Source");
    assert_eq!(
        h.state.model_gateway().unwrap().upcoming_generation(),
        generation_before
    );
    assert_eq!(h.arrivals().len(), 1);
    assert_eq!(h.usage().await.len(), 1);
    eprintln!("provider_matrix_boundary provider={id} case=invalid_protocol_save http=400 source_unchanged=true extra_physical=0 extra_usage=0");
    // 明确把该模型改为合法的embedding专用目录，再证明Chat运行入口不偷用默认协议。
    let (status, saved) = h.request(reqwest::Method::PUT, &format!("/lingxi/v1/providers/{id}/models/{}",brand.model), Some(json!({"api":"openai-embeddings","operations":["embedding"],"operationProtocol":"openai-embeddings","inputs":["text"]}))).await;
    assert_eq!(
        status, 200,
        "{id}合法embedding专用模型必须真实保存：{saved}"
    );
    let source = h.source();
    assert_eq!(
        source["providers"][id]["protocol"], brand.protocol,
        "{id}不能改整个供应商默认协议"
    );
    assert_eq!(
        source["providers"][id]["models"][0]["protocol"],
        "openai-embeddings"
    );
    assert_eq!(
        source["providers"][id]["models"][0]["operations"],
        json!(["embedding"])
    );
    let dispatch = h
        .state
        .model_gateway()
        .unwrap()
        .resolve_dispatch(&ModelRouteRequest {
            operation: ModelOperation::Chat,
            provider: Some(id.into()),
            model: Some(brand.model.clone()),
        });
    assert!(
        matches!(
            dispatch,
            Err(ModelGatewayError::OperationUnsupportedByProvider { .. })
        ),
        "{id}共享网关必须明确拒绝Chat用于embedding专用模型"
    );
    let (status, rejected) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/models/health",
            Some(json!({"provider":id,"modelId":brand.model})),
        )
        .await;
    assert_eq!(
        status, 400,
        "{id}已保存模型不支持Chat时运行入口必须拒绝：{rejected}"
    );
    assert_eq!(
        h.arrivals().len(),
        1,
        "{id}不得丢掉模型能力限制或换协议发送"
    );
    assert_eq!(h.usage().await.len(), 1, "{id}运行前拒绝不得虚增用量");
    assert!(h.source() == source, "{id}运行拒绝不得自动重绑模型");
    eprintln!("provider_matrix_boundary provider={id} case=chat_on_saved_embedding_only_model http=400 extra_physical=0 extra_usage=0 default_protocol_preserved=true");
    h.close().await;
}

macro_rules! builtin_identity_negative_case {
    ($name:ident, $id:literal) => {
        #[tokio::test]
        async fn $name() {
            builtin_identity_rejects_invalid_requests($id).await;
        }
    };
}
builtin_identity_negative_case!(builtin_agnes_rejects_invalid_requests, "agnes");
builtin_identity_negative_case!(builtin_dashscope_rejects_invalid_requests, "dashscope");
builtin_identity_negative_case!(builtin_openai_rejects_invalid_requests, "openai");
builtin_identity_negative_case!(builtin_anthropic_rejects_invalid_requests, "anthropic");
builtin_identity_negative_case!(builtin_deepseek_rejects_invalid_requests, "deepseek");
builtin_identity_negative_case!(
    builtin_deepseek_responses_rejects_invalid_requests,
    "deepseek-responses"
);
builtin_identity_negative_case!(builtin_gemini_rejects_invalid_requests, "gemini");
builtin_identity_negative_case!(builtin_openrouter_rejects_invalid_requests, "openrouter");
builtin_identity_negative_case!(builtin_opencode_rejects_invalid_requests, "opencode");
builtin_identity_negative_case!(builtin_opencode_go_rejects_invalid_requests, "opencode-go");
builtin_identity_negative_case!(builtin_ollama_rejects_invalid_requests, "ollama");
builtin_identity_negative_case!(builtin_minimax_rejects_invalid_requests, "minimax");
builtin_identity_negative_case!(
    builtin_minimax_token_plan_rejects_invalid_requests,
    "minimax-token-plan"
);
builtin_identity_negative_case!(builtin_siliconflow_rejects_invalid_requests, "siliconflow");
builtin_identity_negative_case!(builtin_zhipu_rejects_invalid_requests, "zhipu");
builtin_identity_negative_case!(builtin_moonshot_rejects_invalid_requests, "moonshot");
builtin_identity_negative_case!(builtin_baichuan_rejects_invalid_requests, "baichuan");
builtin_identity_negative_case!(builtin_stepfun_rejects_invalid_requests, "stepfun");
builtin_identity_negative_case!(builtin_volcengine_rejects_invalid_requests, "volcengine");
builtin_identity_negative_case!(builtin_hunyuan_rejects_invalid_requests, "hunyuan");
builtin_identity_negative_case!(builtin_baidu_cloud_rejects_invalid_requests, "baidu-cloud");
builtin_identity_negative_case!(builtin_modelscope_rejects_invalid_requests, "modelscope");
builtin_identity_negative_case!(builtin_infini_rejects_invalid_requests, "infini");
builtin_identity_negative_case!(builtin_mimo_rejects_invalid_requests, "mimo");
builtin_identity_negative_case!(
    builtin_mimo_token_plan_rejects_invalid_requests,
    "mimo-token-plan"
);
builtin_identity_negative_case!(builtin_groq_rejects_invalid_requests, "groq");
builtin_identity_negative_case!(builtin_together_rejects_invalid_requests, "together");
builtin_identity_negative_case!(builtin_fireworks_rejects_invalid_requests, "fireworks");
builtin_identity_negative_case!(builtin_mistral_rejects_invalid_requests, "mistral");
builtin_identity_negative_case!(builtin_perplexity_rejects_invalid_requests, "perplexity");
builtin_identity_negative_case!(builtin_xai_rejects_invalid_requests, "xai");
builtin_identity_negative_case!(
    builtin_dashscope_coding_rejects_invalid_requests,
    "dashscope-coding"
);
builtin_identity_negative_case!(builtin_kimi_coding_rejects_invalid_requests, "kimi-coding");
builtin_identity_negative_case!(
    builtin_volcengine_coding_rejects_invalid_requests,
    "volcengine-coding"
);
builtin_identity_negative_case!(
    builtin_zhipu_coding_rejects_invalid_requests,
    "zhipu-coding"
);
