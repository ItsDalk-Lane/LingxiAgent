//! 真HTTP→唯一配置发布者→真实网关/凭证/存储；仅供应商响应为回环替身。
use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_kernel::{ports::StoragePort as _, usage::ModelUsageQuery};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

#[derive(Clone)]
struct ObservedRequest {
    path: &'static str,
    headers: HeaderMap,
    body: Value,
}

#[derive(Default)]
struct External {
    requests: Mutex<Vec<ObservedRequest>>,
    next_tool: Mutex<Option<Value>>,
}

async fn models(State(external): State<Arc<External>>, headers: HeaderMap) -> Response {
    external.requests.lock().unwrap().push(ObservedRequest {
        path: "models",
        headers,
        body: Value::Null,
    });
    Json(json!({"data":[{"id":"discovered-only","api":"do-not-import","base_url":"https://must-not-use.invalid","api_key":"remote-untrusted-secret","context_length":8192,"inputs":["text"]}]})).into_response()
}

async fn chat(
    State(external): State<Arc<External>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    external.requests.lock().unwrap().push(ObservedRequest {
        path: "chat",
        headers,
        body: body.clone(),
    });
    if let Some(arguments) = external.next_tool.lock().unwrap().take() {
        let tool = json!({"index":0,"id":"fixture-settings-call","type":"function","function":{"name":"update_settings","arguments":arguments.to_string()}});
        if body["stream"] == true {
            let first=json!({"id":"fixture-tool-response","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[tool]},"finish_reason":null}]}).to_string();
            let last=json!({"id":"fixture-tool-response","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":3,"completion_tokens":1,"total_tokens":4}}).to_string();
            return (
                [("content-type", "text/event-stream")],
                format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
            )
                .into_response();
        }
        return Json(json!({"id":"fixture-tool-response","object":"chat.completion","model":body["model"],"choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[tool]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":3,"completion_tokens":1,"total_tokens":4}})).into_response();
    }
    if body["stream"] == true {
        let first=json!({"id":"fixture-response","object":"chat.completion.chunk","model":body["model"],"choices":[{"index":0,"delta":{"role":"assistant","content":"OK"},"finish_reason":null}]}).to_string();
        let last=json!({"id":"fixture-response","object":"chat.completion.chunk","model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":1,"total_tokens":4}}).to_string();
        (
            [("content-type", "text/event-stream")],
            format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
        )
            .into_response()
    } else {
        Json(json!({"id":"fixture-response","object":"chat.completion","model":body["model"],"choices":[{"index":0,"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":1,"total_tokens":4}})).into_response()
    }
}

async fn anthropic(
    State(external): State<Arc<External>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    external.requests.lock().unwrap().push(ObservedRequest {
        path: "anthropic",
        headers,
        body: body.clone(),
    });
    if body["stream"] == true {
        let events = [
            (
                "message_start",
                json!({"type":"message_start","message":{"id":"fixture-anthropic","type":"message","role":"assistant","model":body["model"],"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":3,"output_tokens":0}}}),
            ),
            (
                "content_block_start",
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            ),
            (
                "content_block_delta",
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"OK"}}),
            ),
            (
                "content_block_stop",
                json!({"type":"content_block_stop","index":0}),
            ),
            (
                "message_delta",
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}),
            ),
            ("message_stop", json!({"type":"message_stop"})),
        ];
        let text = events
            .iter()
            .map(|(event, value)| format!("event: {event}\ndata: {value}\n\n"))
            .collect::<String>();
        ([("content-type", "text/event-stream")], text).into_response()
    } else {
        Json(json!({"id":"fixture-anthropic","type":"message","role":"assistant","model":body["model"],"content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":3,"output_tokens":1}})).into_response()
    }
}

struct Harness {
    home: PathBuf,
    state: ServiceState,
    base: String,
    endpoint: String,
    external: Arc<External>,
    approvals: Arc<lingxi_service::approval_service::ApprovalService>,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
    external_task: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-provider-http-{}",
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
        let layout = prepare_layout(&home).unwrap();
        let plane = ModelPlaneConfig::from_value(json!({"providers":{},"models":{}})).unwrap();
        let clock = Arc::new(lingxi_service::inject::SystemClock);
        let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
            clock.clone(),
        ));
        let tool_gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
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
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                model_gateway: Some(Arc::new(ConfigModelGateway::from_validated(plane))),
                credential_service: Some(credentials),
                network_plane: Some(NetworkPlane::direct_isolated()),
                model_plane_activation: Some(Arc::new(AtomicBool::new(false))),
                tool_gateway: Some(tool_gateway),
                approval_gate: Some(approvals.clone()),
                clock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let external = Arc::new(External::default());
        let external_app = Router::new()
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(chat))
            .route("/v1/messages", post(anthropic))
            .with_state(external.clone());
        let external_task =
            tokio::spawn(async move { axum::serve(listener, external_app).await.unwrap() });
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
            .unwrap();
        });
        let base = format!("http://{}", address.await.unwrap());
        Self {
            home,
            state,
            base,
            endpoint,
            external,
            approvals,
            stop,
            server,
            external_task,
        }
    }
    async fn request_as(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        let value = serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw":text}));
        (status, value)
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        self.request_as(&self.state.auth().local_token(), method, path, body)
            .await
    }
    async fn configure(&self) {
        let(status,value)=self.request(reqwest::Method::PUT,"/lingxi/v1/settings/config",Some(json!({"providers":{
            "fixture":{"base_url":self.endpoint,"api":"openai-completions","api_key":"saved-fixture-key","headers":{"X-Fixture":"saved-header"},"custom":{"preserve":17},"models":[{"id":"fixture-chat","inputs":["text"],"toolUse":{"supportsTools":true,"dialect":"openai","toolResultFormat":"message"}},{"id":"health-model","inputs":["text"]},{"id":"fixture-chat2","inputs":["text"],"toolUse":{"supportsTools":true,"dialect":"openai","toolResultFormat":"message"}}]},
            "fixture-anthropic":{"base_url":self.endpoint,"api":"anthropic-messages","api_key":"saved-anthropic-key","models":[{"id":"claude-fixture","inputs":["text"]}]}
        }}))).await;
        assert_eq!(status, 200, "首次设置必须经过真实HTTP保存并生效：{value}");
    }
    fn observed(&self) -> Vec<ObservedRequest> {
        self.external.requests.lock().unwrap().clone()
    }

    async fn setup_chat(&self) {
        use lingxi_service::model_configuration::{ModelSelection, ModelSettingsChange};
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
            .model_configuration()
            .save(
                &principal,
                &[ModelSettingsChange::SetBinding {
                    operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
                    selection: Some(ModelSelection {
                        provider: "fixture".into(),
                        model: "fixture-chat".into(),
                    }),
                }],
            )
            .await
            .unwrap();
    }

    async fn run_settings(
        &self,
        session: &str,
        agent: &str,
        mode: &str,
        arguments: Value,
    ) -> String {
        self.state
            .session_admin()
            .create_session(
                session,
                agent,
                lingxi_kernel::LOCAL_OWNER_SUBJECT,
                "设置工具实链",
                Some(mode),
                &[],
                false,
                1_790_000_000_000,
            )
            .await
            .unwrap();
        *self.external.next_tool.lock().unwrap() = Some(arguments);
        let path = format!("/lingxi/v1/sessions/{session}/execute");
        let response = self.request(
            reqwest::Method::POST,
            &path,
            Some(json!({"input":"执行明确设置工具操作"})),
        );
        tokio::pin!(response);
        // execute是前台等待面；真实审批必须与HTTP请求同时回应，不能等响应后才批准。
        let (status, value) = loop {
            tokio::select! {
                value=&mut response => break value,
                _=tokio::time::sleep(std::time::Duration::from_millis(20)) => {self.answer_settings_approvals(session);}
            }
        };
        assert!(
            (200..300).contains(&status),
            "session={session},home={},HTTP {status}: {value}",
            self.home.display()
        );
        let run = value["runId"].as_str().expect("实际Run身份").to_owned();
        for _ in 0..500 {
            self.answer_settings_approvals(session);
            let status = self
                .state
                .storage()
                .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run.clone()])
                .await
                .unwrap()
                .unwrap();
            if matches!(
                status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted_needs_attention"
            ) {
                return run;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("真实设置Run未在预算内结算：{run}");
    }

    fn answer_settings_approvals(&self, session: &str) {
        for pending in self
            .approvals
            .pending_for_principal(&lingxi_kernel::Principal::LocalUser, session)
        {
            assert_eq!(
                pending.target, "tool:first-party:update_settings",
                "测试只回答本次真实设置工具审批：{pending:?}"
            );
            assert!(
                self.approvals
                    .answer_for_principal(
                        &lingxi_kernel::Principal::LocalUser,
                        session,
                        &pending.approval_id,
                        lingxi_service::approval_service::Answer::Approve
                    )
                    .settled(),
                "真实设置审批未结算：{pending:?}"
            );
        }
    }

    async fn tool_receipt(&self, run: &str) -> String {
        // journal只记审计摘要，实际内容在同run/attempt/call的原始完成事件；不能把摘要当正文。
        let raw = self.state.storage().query_one_text(
            "SELECT json_object('journalId',j.journal_id,'phase',j.phase,'outcome',j.receipt_outcome,'detail',j.receipt_detail,'dedupId',j.dedup_id,'event',json(e.payload_json)) FROM invocation_journal j LEFT JOIN key_events e ON e.run_id=j.run_id AND e.attempt=j.attempt AND e.session_id=j.session_id AND e.event_type='tool_call_completed' AND json_extract(e.payload_json,'$.toolCallId')=j.journal_id WHERE j.run_id=?1 AND j.target='tool:first-party:update_settings' ORDER BY j.prepared_at_unix_ms DESC LIMIT 1",
            vec![run.into()],
        ).await.unwrap_or_else(|e|panic!("读取真实工具回执失败，run={run}: {e:?}")).unwrap_or_else(||panic!("工具未实际进入journal，run={run},home={}",self.home.display()));
        let receipt: Value = serde_json::from_str(&raw).expect("数据库JSON回执必须可解析");
        assert_eq!(
            receipt["event"]["toolCallId"], receipt["journalId"],
            "实际完成事件必须绑定同一工具调用：{raw}"
        );
        if receipt["outcome"] == "succeeded" {
            assert_eq!(
                receipt["event"]["result"]["status"], "success",
                "journal成功必须有真实成功内容：{raw}"
            );
            let digest = lingxi_protocol::canon::canonical_sha256_hex(
                &receipt["event"]["result"]["content"],
            );
            assert_eq!(
                receipt["dedupId"].as_str(),
                Some(digest.as_str()),
                "原始完成内容必须匹配journal摘要：{raw}"
            );
        }
        raw
    }
    async fn close(self) {
        let _ = self.stop.send(());
        let _ = self.server.await;
        self.external_task.abort();
        let _ = self.external_task.await;
        self.state.storage().close().await.unwrap();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

#[tokio::test]
async fn catalog_draft_mask_and_cache_never_publish_remote_models_or_count_usage() {
    let h = Harness::boot().await;
    h.configure().await;
    let before = h
        .state
        .storage()
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .unwrap()
        .len();
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/providers/fetch-models",
            Some(json!({"name":"fixture","api_key":"draft-fixture-key","headers":null})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["models"][0]["id"], "discovered-only");
    assert!(value["models"][0].get("api_key").is_none());
    assert!(value["models"][0].get("api").is_none());
    let first = h.observed().last().unwrap().clone();
    assert_eq!(first.headers["authorization"], "Bearer draft-fixture-key");
    assert!(first.headers.get("x-fixture").is_none());
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/providers/test",
            Some(json!({"name":"fixture","api_key":"********"})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["ok"], true);
    assert_eq!(
        h.observed().last().unwrap().headers["authorization"],
        "Bearer saved-fixture-key"
    );
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/providers/test",
            Some(json!({"name":"fixture","api_key":"********","headers":{"X-Only":"draft"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["ok"], true);
    let header_draft = h.observed().last().unwrap().clone();
    assert_eq!(header_draft.headers["x-only"], "draft");
    assert!(
        header_draft.headers.get("authorization").is_none(),
        "明文header草稿不能暗取保存key"
    );
    assert!(
        header_draft.headers.get("x-fixture").is_none(),
        "草稿不能混保存header"
    );
    let (status, value) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/providers/fixture/discovered-models",
            None,
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["models"][0]["id"], "discovered-only");
    assert!(!value["fetchedAt"].is_null());
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/models/health",
            Some(json!({"provider":"fixture","modelId":"discovered-only"})),
        )
        .await;
    assert!(
        status >= 400 || value["ok"] == false,
        "发现不等于已添加：{value}"
    );
    assert_eq!(h.observed().len(), 3, "未添加模型不能拿凭证发新请求");
    assert_eq!(
        h.state
            .storage()
            .query_model_call_usage(ModelUsageQuery::default())
            .await
            .unwrap()
            .len(),
        before,
        "目录和GET连通性不得计作模型生成"
    );
    h.close().await;
}

#[tokio::test]
async fn health_pins_catalog_model_and_anthropic_connection_have_real_usage() {
    let h = Harness::boot().await;
    h.configure().await;
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/models/health",
            Some(json!({"provider":"fixture","modelId":"health-model"})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["ok"], true, "{value}");
    let request = h
        .observed()
        .into_iter()
        .find(|r| r.path == "chat")
        .expect("真实模型请求");
    assert_eq!(request.body["model"], "health-model");
    assert_eq!(request.headers["authorization"], "Bearer saved-fixture-key");
    assert!(request.body.to_string().contains("Reply exactly OK."));
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/providers/test",
            Some(json!({"name":"fixture-anthropic","model_id":"claude-fixture"})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["ok"], true, "{value}");
    let request = h
        .observed()
        .into_iter()
        .find(|r| r.path == "anthropic")
        .expect("Anthropic测试是真实生成");
    assert_eq!(request.body["model"], "claude-fixture");
    assert_eq!(request.body["max_tokens"], 1);
    assert_eq!(request.headers["x-api-key"], "saved-anthropic-key");
    let rows = h
        .state
        .storage()
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .unwrap();
    assert!(rows.iter().any(|r| r.provider == "fixture"
        && r.model == "health-model"
        && r.transport_attempts == Some(1)
        && r.run_id.is_none()
        && r.usage.is_some()));
    assert!(rows.iter().any(|r| r.provider == "fixture-anthropic"
        && r.model == "claude-fixture"
        && r.transport_attempts == Some(1)
        && r.usage.is_some()));
    let encoded = serde_json::to_string(
        &rows
            .iter()
            .map(|r| (&r.provider, &r.model, &r.purpose))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(!encoded.contains("saved-fixture-key"));
    h.close().await;
}

#[tokio::test]
async fn model_patch_and_removal_preserve_other_settings_and_stop_new_dispatch() {
    let h = Harness::boot().await;
    h.configure().await;
    let(status,value)=h.request(reqwest::Method::PUT,"/lingxi/v1/providers/fixture/models/health-model",Some(json!({"contextWindow":32000,"maxTokens":256,"name":"edited","inputs":["text","image"]}))).await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/providers/summary", None)
        .await;
    assert_eq!(status, 200, "{value}");
    let provider = &value["providers"]["fixture"];
    assert_eq!(provider["api_key"], "********");
    assert!(provider["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == "fixture-chat"));
    let model = provider["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "health-model")
        .unwrap();
    assert_eq!(model["name"], "edited");
    assert_eq!(model["context"], 32000);
    let (status, value) = h
        .request(
            reqwest::Method::DELETE,
            "/lingxi/v1/providers/fixture/models/health-model",
            None,
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let before = h.observed().len();
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/models/health",
            Some(json!({"provider":"fixture","modelId":"health-model"})),
        )
        .await;
    assert!(status >= 400 || value["ok"] == false, "{value}");
    assert_eq!(h.observed().len(), before);
    let (status, value) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/providers/fixture/api-key",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(value["api_key"], "saved-fixture-key");
    h.close().await;
}

#[tokio::test]
async fn api_key_reveal_requires_both_scopes_and_discovery_secret_drafts_cannot_bypass() {
    let h = Harness::boot().await;
    h.configure().await;
    for scopes in [&["providers.manage"][..], &["secrets.write"][..]] {
        let token = h
            .state
            .auth()
            .issue_device_credential("user_local", scopes, None)
            .unwrap()
            .secret;
        let (status, value) = h
            .request_as(
                &token,
                reqwest::Method::GET,
                "/lingxi/v1/providers/fixture/api-key",
                None,
            )
            .await;
        assert_eq!(status, 403, "{value}");
        assert!(!value.to_string().contains("saved-fixture-key"));
    }
    let token = h
        .state
        .auth()
        .issue_device_credential("user_local", &["providers.manage"], None)
        .unwrap()
        .secret;
    let (status, value) = h
        .request_as(
            &token,
            reqwest::Method::POST,
            "/lingxi/v1/providers/test",
            Some(json!({"name":"fixture","api_key":"new-secret"})),
        )
        .await;
    assert_eq!(status, 403, "{value}");
    assert!(h.observed().is_empty());
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/providers/summary", None)
        .await;
    assert_eq!(status, 200, "{value}");
    for definition in lingxi_service::provider_probe::builtin_provider_definitions() {
        assert!(
            !value["providers"][definition["id"].as_str().unwrap()].is_null(),
            "缺39定义中的项：{definition}"
        );
    }
    h.close().await;
}

#[tokio::test]
async fn actual_local_run_settings_search_apply_target_agent_and_security_refusals() {
    let h = Harness::boot().await;
    h.configure().await;
    h.setup_chat().await;
    std::fs::create_dir_all(h.home.join("agents/b")).unwrap();
    std::fs::write(
        h.home.join("agents/b/config.yaml"),
        "agent:\n  name: B\nmemory:\n  enabled: false\ncustom:\n  preserve: yes\n",
    )
    .unwrap();
    let search = h
        .run_settings(
            "settings-search",
            "b",
            "read_only",
            json!({"action":"search","query":"语言"}),
        )
        .await;
    let search_receipt = h.tool_receipt(&search).await;
    assert!(
        search_receipt.contains("locale"),
        "搜索必须真实返回语言设置，run={search},home={},receipt={search_receipt}",
        h.home.display()
    );
    let change = h
        .run_settings(
            "settings-name",
            "b",
            "operate",
            json!({"action":"apply","key":"agent.name","value":"B changed"}),
        )
        .await;
    let change_receipt = h.tool_receipt(&change).await;
    assert!(
        change_receipt.contains("applied"),
        "助手名必须真实应用，run={change},receipt={change_receipt}"
    );
    let a = std::fs::read_to_string(h.home.join("agents/a/config.yaml")).unwrap();
    let b = std::fs::read_to_string(h.home.join("agents/b/config.yaml")).unwrap();
    assert!(a.contains("name: a"));
    assert!(b.contains("B changed"));
    assert!(b.contains("preserve"));
    // 十四项旧字典逐项走真实Run；外部只替换模型响应，内部授权、存储和消费均为现役链。
    let workspace = h.home.join("tool-workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let updates = [
        ("locale", "en".to_owned()),
        ("timezone", "Asia/Shanghai".into()),
        ("file_backup", "3".into()),
        ("user.name", "Settings owner".into()),
        ("theme", "auto".into()),
        (
            "bridge_media_public_base_url",
            "https://media.example.test/share".into(),
        ),
        ("computer_use.enabled", "false".into()),
        ("thinking_level", "off".into()),
        ("memory.enabled", "false".into()),
        ("experience.enabled", "true".into()),
        ("memory.facts", "本条由真实设置工具保存".into()),
        ("home_folder", workspace.to_string_lossy().into_owned()),
        ("models.chat", "fixture/fixture-chat2".into()),
    ];
    for (index, (key, value)) in updates.iter().enumerate() {
        let run = h
            .run_settings(
                &format!("settings-key-{index}"),
                "b",
                "operate",
                json!({"action":"apply","key":key,"value":value}),
            )
            .await;
        let receipt = h.tool_receipt(&run).await;
        if *key == "computer_use.enabled" && cfg!(target_os = "linux") {
            assert!(
                !receipt.contains("applied"),
                "Linux不伪装电脑操作支持：{receipt}"
            );
        } else {
            assert!(receipt.contains("applied"), "{key}必须真实生效：{receipt}");
        }
    }
    let preferences: Value =
        serde_json::from_slice(&std::fs::read(h.home.join("user/preferences.json")).unwrap())
            .unwrap();
    assert_eq!(preferences["locale"], "en");
    assert_eq!(preferences["timezone"], "Asia/Shanghai");
    assert_eq!(
        preferences["file_backup"]["retention_days"].as_f64(),
        Some(3.0)
    );
    assert_eq!(preferences["userName"], "Settings owner");
    assert_eq!(preferences["appearance"]["theme"], "auto");
    assert_eq!(
        preferences["bridge"]["mediaPublicBaseUrl"],
        "https://media.example.test/share"
    );
    let b = std::fs::read_to_string(h.home.join("agents/b/config.yaml")).unwrap();
    assert!(b.contains("fixture-chat2"));
    assert!(b.contains("tool-workspace"));
    assert!(
        std::fs::read_to_string(h.home.join("agents/b/memory/facts.md"))
            .unwrap()
            .contains("本条由真实设置工具保存")
    );
    let next = h
        .run_settings(
            "settings-default-model-consumer",
            "b",
            "read_only",
            json!({"action":"search","query":"model"}),
        )
        .await;
    assert!(h.tool_receipt(&next).await.contains("models.chat"));
    assert_eq!(
        h.observed()
            .iter()
            .rev()
            .find(|r| r.path == "chat")
            .unwrap()
            .body["model"],
        "fixture-chat2"
    );
    // 仅调整既有会话的思考档位，不能把助手B的chat2偷偷改回全局chat1。
    let sid = "settings-default-model-consumer";
    let choices_path = h.home.join("chat-input-settings.json");
    let prior_choices: Value =
        serde_json::from_slice(&std::fs::read(&choices_path).unwrap()).unwrap();
    let (status, thinking) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/session-thinking-level",
            Some(json!({"sessionId":sid,"level":"off"})),
        )
        .await;
    assert_eq!(status, 200, "既有助手会话thinking更新必须成功：{thinking}");
    let choices: Value = serde_json::from_slice(&std::fs::read(&choices_path).unwrap()).unwrap();
    assert_eq!(
        choices["sessions"][sid]["model"],
        json!(["fixture", "fixture-chat2"]),
        "thinking更新必须持久保留该助手模型：{choices}"
    );
    assert_eq!(choices["sessions"][sid]["thinkingLevel"], "off");
    let committed = std::fs::read(&choices_path).unwrap();
    let (status, conflict) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/session-thinking-level",
            Some(json!({"sessionId":sid,"sessionPath":"another-session","level":"off"})),
        )
        .await;
    assert_eq!(status, 400, "冲突身份不能误写默认设置：{conflict}");
    assert_eq!(std::fs::read(&choices_path).unwrap(), committed);
    let (status, legacy) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/session-thinking-level",
            Some(json!({"sessionPath":sid,"level":"off"})),
        )
        .await;
    assert_eq!(status, 200, "兼容会话入口仍须指向同一模型：{legacy}");
    let legacy: Value = serde_json::from_slice(&std::fs::read(&choices_path).unwrap()).unwrap();
    assert_eq!(
        legacy["sessions"][sid]["model"],
        json!(["fixture", "fixture-chat2"])
    );
    assert_eq!(
        choices["defaults"], prior_choices["defaults"],
        "指定sessionId不能误写全局默认：{choices}"
    );
    let observed_before = h.observed().len();
    let (status, again) = h
        .request(
            reqwest::Method::POST,
            &format!("/lingxi/v1/sessions/{sid}/execute"),
            Some(json!({"input":"只检查同一会话下一次实际模型调用"})),
        )
        .await;
    assert_eq!(status, 200, "thinking更新后的同一会话必须真实执行：{again}");
    let again_id = again["runId"].as_str().expect("实际下一Run身份");
    assert_ne!(again_id, next, "必须是同一会话的新Run，不能重放旧请求");
    let observed = h.observed();
    let calls: Vec<_> = observed[observed_before..]
        .iter()
        .filter(|request| request.path == "chat")
        .collect();
    assert!(!calls.is_empty(), "必须观察到真实新增模型请求：{again}");
    assert!(
        calls
            .iter()
            .all(|request| request.body["model"] == "fixture-chat2"),
        "仅改thinking后实际请求仍必须选chat2：{}",
        json!(calls.iter().map(|r| r.body.clone()).collect::<Vec<_>>())
    );
    assert_eq!(
        h.state
            .storage()
            .query_one_text(
                "SELECT status FROM runs WHERE run_id=?1",
                vec![again_id.into()]
            )
            .await
            .unwrap()
            .as_deref(),
        Some("completed")
    );
    let before = std::fs::read(h.home.join("user/preferences.json")).unwrap();
    let readonly = h
        .run_settings(
            "settings-readonly",
            "b",
            "read_only",
            json!({"action":"apply","key":"locale","value":"en"}),
        )
        .await;
    assert!(!h
        .tool_receipt(&readonly)
        .await
        .contains("\"status\":\"applied\""));
    assert_eq!(
        std::fs::read(h.home.join("user/preferences.json")).unwrap(),
        before
    );
    let sandbox = h
        .run_settings(
            "settings-security",
            "b",
            "operate",
            json!({"action":"apply","key":"sandbox","value":"false"}),
        )
        .await;
    assert!(!h
        .tool_receipt(&sandbox)
        .await
        .contains("\"status\":\"applied\""));
    assert_eq!(
        std::fs::read(h.home.join("user/preferences.json")).unwrap(),
        before
    );
    h.close().await;
}

#[tokio::test]
async fn auxiliary_vision_http_tracks_same_gateway_catalog_binding_and_preferences_without_generation(
) {
    use lingxi_kernel::model_exchange::{AuxiliarySlot, ModelOperation, ModelRouteRequest};
    async fn read_vision(h: &Harness, token: &str, expected: Value) {
        let (status, body) = h
            .request_as(
                token,
                reqwest::Method::GET,
                "/lingxi/v1/models/auxiliary-vision",
                None,
            )
            .await;
        assert_eq!(status, 200, "辅助视觉真实状态读取失败：{body}");
        assert_eq!(
            body["auxiliaryVision"], expected,
            "状态必须与实际配置一致：{body}"
        );
        let encoded = body.to_string();
        for secret in ["saved-fixture-key", "saved-header", "saved-anthropic-key"] {
            assert!(!encoded.contains(secret), "辅助状态不能披露凭证：{body}");
        }
        assert!(body["auxiliaryVision"].get("api_key").is_none());
        assert!(body["auxiliaryVision"].get("headers").is_none());
    }
    async fn put_vision(h: &Harness, models: Value) {
        let (status, body) = h
            .request(
                reqwest::Method::PUT,
                "/lingxi/v1/preferences/models",
                Some(json!({"models":models})),
            )
            .await;
        assert_eq!(status, 200, "真实共享视觉设置必须保存并生效：{body}");
    }
    fn preferences(h: &Harness) -> Value {
        serde_json::from_slice(&std::fs::read(h.home.join("user/preferences.json")).unwrap())
            .unwrap()
    }
    fn route(model: &str) -> ModelRouteRequest {
        ModelRouteRequest {
            operation: ModelOperation::Auxiliary(AuxiliarySlot::Vision),
            provider: Some("fixture".into()),
            model: Some(model.into()),
        }
    }
    let h = Harness::boot().await;
    let reader = h
        .state
        .auth()
        .issue_device_credential("user_local", &["chat"], None)
        .unwrap()
        .secret;
    read_vision(&h,&reader,json!({"enabled":false,"configured":false,"available":false,"unavailableReason":"disabled","model":null})).await;
    put_vision(&h, json!({"vision_enabled":true})).await;
    assert_eq!(preferences(&h)["vision_auxiliary_enabled"], true);
    assert!(
        h.state.model_gateway().is_none(),
        "只打开辅助视觉开关不能冒充模型平面已经配置"
    );
    read_vision(&h,&reader,json!({"enabled":true,"configured":false,"available":false,"unavailableReason":"not_configured","model":null})).await;

    h.configure().await;
    let gateway = h.state.model_gateway().unwrap();
    let source_path = h.home.join("lingxi-service/models.json");
    let preferences_path = h.home.join("user/preferences.json");
    let source_before = std::fs::read(&source_path).unwrap();
    let preferences_before = std::fs::read(&preferences_path).unwrap();
    let gateway_before=serde_json::to_vec(&json!({"generation":gateway.upcoming_generation(),"routes":gateway.configured_routes(),"catalog":gateway.catalog_models()})).unwrap();
    let(status,rejected)=h.request(reqwest::Method::PUT,"/lingxi/v1/preferences/models",Some(json!({"models":{"vision_enabled":true,"vision":{"provider":"fixture","id":"health-model"}}}))).await;
    assert_eq!(
        status, 400,
        "不支持图片的模型不能被保存为视觉绑定：{rejected}"
    );
    assert_eq!(rejected["code"], "vision_capability_required");
    assert_eq!(
        rejected["result"]["saved"], false,
        "能力拒绝必须发生在保存前：{rejected}"
    );
    assert_eq!(
        std::fs::read(&source_path).unwrap(),
        source_before,
        "拒绝不能改Source字节"
    );
    assert_eq!(
        std::fs::read(&preferences_path).unwrap(),
        preferences_before,
        "拒绝不能改偏好字节"
    );
    assert_eq!(serde_json::to_vec(&json!({"generation":gateway.upcoming_generation(),"routes":gateway.configured_routes(),"catalog":gateway.catalog_models()})).unwrap(),gateway_before,"拒绝不能改变共享网关目录/绑定/代际");
    assert!(!gateway.configured_routes().contains_key("vision"));
    read_vision(&h,&reader,json!({"enabled":true,"configured":false,"available":false,"unavailableReason":"not_configured","model":null})).await;

    let (status, body) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/providers/fixture/models/vision-model",
            Some(json!({"name":"明确视觉目录","inputs":["text","image"]})),
        )
        .await;
    assert_eq!(status, 200, "真实目录添加视觉模型失败：{body}");
    put_vision(
        &h,
        json!({"vision_enabled":true,"vision":{"provider":"fixture","id":"vision-model"}}),
    )
    .await;
    assert_eq!(gateway.configured_routes()["vision"].model, "vision-model");
    assert_eq!(
        gateway
            .resolve_dispatch(&route("vision-model"))
            .unwrap()
            .capabilities
            .image_input,
        Some(true)
    );
    assert_eq!(preferences(&h)["vision_auxiliary_enabled"], true);
    read_vision(&h,&reader,json!({"enabled":true,"configured":true,"available":true,"unavailableReason":null,"model":{"provider":"fixture","id":"vision-model"}})).await;
    put_vision(&h, json!({"vision_enabled":false})).await;
    assert_eq!(preferences(&h)["vision_auxiliary_enabled"], false);
    assert_eq!(gateway.configured_routes()["vision"].model, "vision-model");
    read_vision(&h,&reader,json!({"enabled":false,"configured":true,"available":false,"unavailableReason":"disabled","model":{"provider":"fixture","id":"vision-model"}})).await;
    put_vision(&h, json!({"vision_enabled":true})).await;
    let legacy_path = h.home.join("user/preferences.json");
    let mut legacy_preferences = preferences(&h);
    legacy_preferences["vision_model"] = json!({"provider":"fixture","id":"vision-model"});
    std::fs::write(
        &legacy_path,
        serde_json::to_vec(&legacy_preferences).unwrap(),
    )
    .unwrap();
    put_vision(&h, json!({"vision":null})).await;
    assert!(!gateway.configured_routes().contains_key("vision"));
    assert!(
        gateway.resolve_dispatch(&route("vision-model")).is_ok(),
        "模型仍在目录中，明确清除槽也不得让旧偏好复活它"
    );
    read_vision(&h,&reader,json!({"enabled":true,"configured":false,"available":false,"unavailableReason":"not_configured","model":null})).await;
    put_vision(
        &h,
        json!({"vision":{"provider":"fixture","id":"vision-model"}}),
    )
    .await;
    let (status, body) = h
        .request(
            reqwest::Method::DELETE,
            "/lingxi/v1/providers/fixture/models/vision-model",
            None,
        )
        .await;
    assert_eq!(status, 200, "真实目录删除失败：{body}");
    assert!(
        !gateway.configured_routes().contains_key("vision"),
        "删除必须清除共享视觉绑定"
    );
    assert!(gateway.resolve_dispatch(&route("vision-model")).is_err());
    assert_eq!(preferences(&h)["vision_auxiliary_enabled"], true);
    read_vision(&h,&reader,json!({"enabled":true,"configured":false,"available":false,"unavailableReason":"not_configured","model":null})).await;

    let path = h.home.join("user/preferences.json");
    let saved = std::fs::read(&path).unwrap();
    let denied = h
        .state
        .auth()
        .issue_device_credential("user_local", &["settings.read"], None)
        .unwrap()
        .secret;
    let (status, body) = h
        .request_as(
            &denied,
            reqwest::Method::GET,
            "/lingxi/v1/models/auxiliary-vision",
            None,
        )
        .await;
    assert_eq!(status, 403, "缺chat权限不能读辅助视觉状态：{body}");
    assert!(body.get("auxiliaryVision").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), saved, "权限拒绝不能改偏好");
    // 仅损坏隔离夹具的真实偏好文件，验证handler不吞错误或覆盖原文件。
    std::fs::write(&path, b"{broken-vision-preferences").unwrap();
    let (status, body) = h
        .request_as(
            &reader,
            reqwest::Method::GET,
            "/lingxi/v1/models/auxiliary-vision",
            None,
        )
        .await;
    assert_eq!(status, 500, "偏好损坏不能伪装未配置成功：{body}");
    assert!(body.get("auxiliaryVision").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken-vision-preferences");
    std::fs::write(&path, &saved).unwrap();
    assert!(
        h.observed().is_empty(),
        "状态/设置/权限检查不得发出生成或目录请求"
    );
    assert!(
        h.state
            .storage()
            .query_model_call_usage(ModelUsageQuery::default())
            .await
            .unwrap()
            .is_empty(),
        "辅助视觉状态读取不能写虚假模型用量"
    );
    h.close().await;
}

#[tokio::test]
async fn provider_edits_deletes_and_reload_notify_real_applied_state_even_when_projection_fails() {
    let h = Harness::boot().await;
    h.configure().await;
    let catalog = h.home.join("provider-catalog.json");
    let catalog_before = std::fs::read(&catalog).unwrap();
    std::fs::write(&catalog, b"{broken compatibility projection").unwrap();
    for (method, path, body) in [
        (
            reqwest::Method::PUT,
            "/lingxi/v1/providers/fixture/models/fixture-chat2",
            Some(json!({"contextWindow":8192})),
        ),
        (
            reqwest::Method::DELETE,
            "/lingxi/v1/providers/fixture/models/fixture-chat2",
            None,
        ),
        (
            reqwest::Method::POST,
            "/lingxi/v1/models/reload",
            Some(json!({})),
        ),
    ] {
        let cursor = h.state.app_events().since(0)["latestSequence"]
            .as_u64()
            .unwrap();
        let (status, result) = h.request(method, path, body).await;
        assert!(status >= 400, "{path}: {result}");
        assert_eq!(
            result["code"], "model_projection_unconfirmed",
            "{path}: {result}"
        );
        assert_eq!(result["result"]["applied"], true, "{path}: {result}");
        let events = h.state.app_events().since(cursor)["events"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "models-changed")
                .count(),
            1,
            "已生效即须通知实际消费者：{path}: {events:?}"
        );
    }
    assert!(h
        .state
        .model_gateway()
        .unwrap()
        .resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
            operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
            provider: Some("fixture".into()),
            model: Some("fixture-chat2".into())
        })
        .is_err());
    std::fs::write(&catalog, &catalog_before).unwrap();
    let cursor = h.state.app_events().since(0)["latestSequence"]
        .as_u64()
        .unwrap();
    let (status, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/models/reload",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["result"]["projectionConfirmed"], true);
    let events = h.state.app_events().since(cursor)["events"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "models-changed")
            .count(),
        1
    );
    assert!(h.observed().is_empty());
    h.close().await;
}
