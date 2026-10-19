//! 真Run→设置网关审批→MCP请求/应答→唯一管理器→官方SDK/自有stdio；模型只用回环平台响应。
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
use lingxi_kernel::{
    toolcatalog::{SchemaBudget, ToolTargetId},
    RunContext,
};
use lingxi_protocol::{AttemptId, McpSettingsResult, RunId, SessionId, ToolCallId};
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
        let tool = json!({"index":0,"id":"fixture-settings-call","type":"function","function":{"name":arguments["name"],"arguments":arguments["arguments"].to_string()}});
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
    server:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
    external_task: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot() -> Self {
        Self::boot_with_defer(false, 17).await
    }
    async fn boot_with_defer(defer_enabled: bool, defer_threshold: u64) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-mcp-settings-{}",
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
        std::fs::create_dir_all(home.join("agents/b")).unwrap();
        std::fs::write(
            home.join("agents/b/config.yaml"),
            "agent:\n  name: b\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("plugin-data/mcp")).unwrap();
        std::fs::write(home.join("plugin-data/mcp/config.json"),json!({"schemaVersion":1,"unknownRoot":73,"global":{"keep":"global","mcp":{"enabled":false,"deferEnabled":defer_enabled,"deferThreshold":defer_threshold,"unknownMcp":79,"servers":[]}},"agents":{"legacy":{"keep":true}},"sessions":{}}).to_string()).unwrap();
        let home = home.canonicalize().unwrap();
        eprintln!("MCP设置现场：{}", home.display());
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

    async fn run_tool(
        &self,
        session: &str,
        agent: &str,
        mode: &str,
        tool: &str,
        arguments: Value,
    ) -> String {
        self.run_tool_as(session, agent, mode, tool, arguments, None)
            .await
    }

    async fn run_tool_as(
        &self,
        session: &str,
        agent: &str,
        mode: &str,
        tool: &str,
        arguments: Value,
        cookie: Option<&str>,
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
        *self.external.next_tool.lock().unwrap() = Some(json!({"name":tool,"arguments":arguments}));
        let path = format!("/lingxi/v1/sessions/{session}/execute");
        let response = async {
            if let Some(cookie) = cookie {
                let response = reqwest::Client::new()
                    .post(format!("{}{}", self.base, path))
                    .header("cookie", cookie)
                    .json(&json!({"input":"执行明确设置工具操作"}))
                    .send()
                    .await
                    .unwrap();
                (
                    response.status().as_u16(),
                    response.json::<Value>().await.unwrap(),
                )
            } else {
                self.request(
                    reqwest::Method::POST,
                    &path,
                    Some(json!({"input":"执行明确设置工具操作"})),
                )
                .await
            }
        };
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
            assert!(
                matches!(
                    pending.target.as_str(),
                    "tool:first-party:update_settings" | "tool:mcp:fixture:env_report"
                ),
                "只批准本次私有平台上的真实工具调用：{pending:?}"
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
            "SELECT json_object('journalId',j.journal_id,'phase',j.phase,'outcome',j.receipt_outcome,'detail',j.receipt_detail,'dedupId',j.dedup_id,'event',json(e.payload_json)) FROM invocation_journal j LEFT JOIN key_events e ON e.run_id=j.run_id AND e.attempt=j.attempt AND e.session_id=j.session_id AND e.event_type='tool_call_completed' AND json_extract(e.payload_json,'$.toolCallId')=j.journal_id WHERE j.run_id=?1 ORDER BY j.prepared_at_unix_ms DESC LIMIT 1",
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
        self.close_runtime(true).await;
    }
    async fn close_runtime(self, remove_home: bool) {
        self.stop.send(()).expect("服务应仍在运行");
        let outcome = self
            .server
            .await
            .expect("服务任务不可崩溃")
            .expect("服务应正常退出");
        assert!(!outcome.drain_timed_out);
        self.external_task.abort();
        let _ = self.external_task.await;
        self.state.storage().close().await.unwrap();
        if remove_home {
            std::fs::remove_dir_all(&self.home).unwrap();
        }
    }
}

fn fixture_connector(id: &str) -> Value {
    json!({"id":id,"name":id,"transport":"stdio","command":env!("CARGO_BIN_EXE_r04_t07_fixture"),"args":["--mcp-stdio-server"],"env":{"LINGXI_T07_SECRET_A":"fixture-private-env"},"authorizationToken":"fixture-private-token","unknownConnector":{"preserve":83}})
}

async fn settled_state(h: &Harness) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let (status, value) = h
                .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=a", None)
                .await;
            assert_eq!(status, 200, "{value}");
            if value["config"]["connectors"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["status"] != "connecting")
            {
                return value;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("后台连接必须产生真实终态，不能把排队回执当完成")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_page_state_keeps_real_tools_agent_defaults_defer_and_private_edit_material() {
    let h = Harness::boot().await;
    let post = |action: &str, payload: Value, agent: Option<&str>| json!({"action":action,"payload":payload,"agentId":agent});
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(post("mcp.global.enabled", json!({"enabled":true}), None)),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(post(
                "mcp.connector.add",
                json!({"connector":fixture_connector("fixture"),"startIfEnabled":true}),
                None,
            )),
        )
        .await;
    assert_eq!(status, 200, "页面添加先保存并排队，不等待SDK：{value}");
    assert_eq!(value["settingsUpdate"]["saved"], true);
    assert_eq!(value["settingsUpdate"]["runtimeApplied"], false);
    settled_state(&h).await;
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=b", None)
        .await;
    assert_eq!(status, 200, "{value}");
    let typed: lingxi_protocol::McpSettingsState = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(typed.agent_id.as_deref(), Some("b"));
    let row = &value["config"]["connectors"][0];
    assert_eq!(row["status"], "running");
    assert_eq!(row["command"], env!("CARGO_BIN_EXE_r04_t07_fixture"));
    assert_eq!(row["env"]["LINGXI_T07_SECRET_A"], "********");
    assert!(!value.to_string().contains("fixture-private"));
    assert!(row["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "env_report"));
    assert_eq!(
        value["agentConfig"]["connectors"]["fixture"]["enabled"], true,
        "执行器缺省允许须显示为允许"
    );
    assert_eq!(
        value["agentConfig"]["connectors"]["fixture"]["tools"]["env_report"],
        true
    );
    invoke_fixture(&h, "b", "page-real-call").await;
    let a_before = std::fs::read(h.home.join("agents/a/config.yaml")).unwrap();
    let (ctx, call, held) = prepared(&h, "b", "page-before-disable");
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(post(
                "mcp.agent.tool.enable",
                json!({"connectorId":"fixture","tools":{"env_report":false,"future_tool":false}}),
                Some("b"),
            )),
        )
        .await;
    assert_eq!(status, 200, "批量权限须一次真实写入：{value}");
    assert!(
        h.state
            .tool_gateway()
            .unwrap()
            .execute_prepared(&ctx, &call, &held.handle)
            .await
            .is_err(),
        "撤权后旧准备不能继续"
    );
    let (_, b) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=b", None)
        .await;
    let (_, a) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=a", None)
        .await;
    assert_eq!(
        b["agentConfig"]["connectors"]["fixture"]["tools"]["env_report"],
        false
    );
    assert_eq!(
        a["agentConfig"]["connectors"]["fixture"]["tools"]["env_report"],
        true
    );
    assert_eq!(
        std::fs::read(h.home.join("agents/a/config.yaml")).unwrap(),
        a_before
    );
    invoke_fixture(&h, "a", "page-a-isolated").await;
    let registry = h.state.tool_gateway().unwrap().registry();
    let before = registry
        .model_declaration_snapshot()
        .1
        .iter()
        .map(|row| row.listing.target_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let builtin = before
        .iter()
        .find(|target| {
            target.as_str().starts_with("tool:first-party:")
                && !["mcp_search_tools", "mcp_describe_tool", "mcp_call"]
                    .iter()
                    .any(|name| target.as_str() == format!("tool:first-party:{name}"))
        })
        .expect("真实组合应有可延迟的原常驻内置工具")
        .clone();
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(post(
                "mcp.global.defer",
                json!({"builtinDeferEnabled":true}),
                None,
            )),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let stored = persisted(&h);
    assert_eq!(
        stored["global"]["mcp"]["deferEnabled"], false,
        "单字段不丢其他设置"
    );
    assert_eq!(stored["global"]["mcp"]["deferThreshold"], 17);
    assert_eq!(stored["unknownRoot"], 73);
    let after = registry.model_declaration_snapshot().1;
    assert!(
        !after.iter().any(|row| row.listing.target_id == builtin),
        "开关必须影响真实模型声明"
    );
    assert!(
        after
            .iter()
            .any(|row| row.listing.local_name == "mcp_search_tools"),
        "发现入口不能被一起隐藏"
    );
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(post(
                "mcp.global.defer",
                json!({"builtinDeferEnabled":false}),
                None,
            )),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(
        registry
            .model_declaration_snapshot()
            .1
            .iter()
            .map(|row| row.listing.target_id.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        before
    );
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(post(
                "mcp.connector.update",
                json!({"connectorId":"fixture","patch":{"pinnedTools":{"env_report":true}}}),
                None,
            )),
        )
        .await;
    assert_eq!(status, 200, "置顶须由同一管理者处理：{value}");
    let (_, still_live) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=a", None)
        .await;
    assert_eq!(
        still_live["config"]["connectors"][0]["status"], "running",
        "纯授权/置顶编辑不能停止真实连接"
    );
    invoke_fixture(&h, "a", "policy-keeps-real-sdk").await;
    let (status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(post("mcp.connector.update",json!({"connectorId":"fixture","patch":{"env":{"LINGXI_T07_SECRET_A":"********"},"pinnedTools":{"env_report":true}}}),None))).await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(
        persisted(&h)["global"]["mcp"]["connectors"][0]["env"]["LINGXI_T07_SECRET_A"],
        "fixture-private-env"
    );
    let (status, value) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/mcp/state?agentId=missing",
            None,
        )
        .await;
    assert_eq!(status, 400, "缺失明确助手不能回焦点：{value}");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_bulk_preflights_all_rows_and_keeps_saved_partial_runtime_failures() {
    let h = Harness::boot().await;
    let source = h.home.join("plugin-data/mcp/config.json");
    let before = std::fs::read(&source).unwrap();
    let (status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add_bulk","payload":{"connectors":[fixture_connector("fixture"),{"name":"bad","transport":"stdio","command":""}]}}))).await;
    assert_eq!(status, 400, "坏行须整批零写：{value}");
    assert_eq!(value["results"].as_array().unwrap().len(), 2);
    assert!(value["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["saved"] == false));
    assert_eq!(std::fs::read(&source).unwrap(), before);
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.enabled","payload":{"enabled":true}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let (status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add_bulk","payload":{"connectors":[fixture_connector("fixture"),{"id":"cannot-start","name":"cannot-start","transport":"stdio","command":"/nonexistent/owned-mcp-fixture"}]}}))).await;
    assert_eq!(
        status, 200,
        "合法输入已保存后的启动故障须保留逐项事实：{value}"
    );
    let typed: lingxi_protocol::McpConnectorBulkResult =
        serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        typed.settings_update.status,
        lingxi_protocol::McpSettingsStatus::Applied
    );
    assert!(typed.settings_update.saved);
    assert!(!typed.settings_update.runtime_applied);
    assert!(typed
        .results
        .iter()
        .all(|row| row.ok && row.saved && !row.runtime_applied));
    let completed = settled_state(&h).await;
    let receipts = completed["settingsResults"].as_array().unwrap();
    assert!(receipts.iter().any(
        |receipt| receipt["settingsUpdate"]["connectorId"] == "fixture"
            && receipt["settingsUpdate"]["runtimeApplied"] == true
    ));
    assert!(receipts
        .iter()
        .any(
            |receipt| receipt["settingsUpdate"]["connectorId"] == "cannot-start"
                && receipt["settingsUpdate"]["saved"] == true
                && receipt["settingsUpdate"]["status"] == "failed"
        ));
    assert_eq!(
        persisted(&h)["global"]["mcp"]["connectors"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    invoke_fixture(&h, "a", "bulk-real-sdk").await;
    let (_, state) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=a", None)
        .await;
    assert_eq!(state["config"]["connectors"][0]["status"], "running");
    assert_eq!(state["config"]["connectors"][1]["status"], "failed");
    let saved = std::fs::read(&source).unwrap();
    let generation = h
        .state
        .tool_gateway()
        .unwrap()
        .registry()
        .catalog_generation();
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.defer","payload":{"deferThreshold":0}})),
        )
        .await;
    assert_eq!(status, 400, "损坏输入零写：{value}");
    assert_eq!(std::fs::read(&source).unwrap(), saved);
    assert_eq!(
        h.state
            .tool_gateway()
            .unwrap()
            .registry()
            .catalog_generation(),
        generation
    );
    h.close().await;
}
fn persisted(h: &Harness) -> Value {
    serde_json::from_slice(&std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap())
        .unwrap()
}
fn tool_value(raw: &str) -> Value {
    let receipt: Value = serde_json::from_str(raw).unwrap();
    assert_eq!(
        receipt["event"]["result"]["status"], "success",
        "必须有真实工具完成回执：{raw}"
    );
    let content = receipt["event"]["result"]["content"]
        .as_array()
        .expect("实际正文");
    let text = content
        .iter()
        .find_map(|block| block["text"].as_str())
        .expect("实际文本结果");
    serde_json::from_str(text).unwrap_or_else(|e| panic!("实际工具结果不能解析：{e}: {raw}"))
}
async fn setting(h: &Harness, index: usize, key: &str, value: Value) -> McpSettingsResult {
    let value = if let Some(text) = value.as_str() {
        text.to_owned()
    } else {
        value.to_string()
    };
    let run = h
        .run_tool(
            &format!("mcp-setting-{index}"),
            "b",
            "ask",
            "update_settings",
            json!({"action":"apply","key":key,"value":value}),
        )
        .await;
    let receipt = h.tool_receipt(&run).await;
    serde_json::from_value(tool_value(&receipt))
        .unwrap_or_else(|e| panic!("管理器实际回执必须符合共用协议：{e}: {receipt}"))
}
fn applied(result: &McpSettingsResult) {
    assert_eq!(
        result.settings_update.status,
        lingxi_protocol::McpSettingsStatus::Applied,
        "{result:?}"
    );
    for change in &result.settings_update.changes {
        assert!(!change.label.is_empty());
        assert_ne!(change.after, "applied");
    }
}
fn prepared(
    h: &Harness,
    agent: &str,
    nonce: &str,
) -> (
    RunContext,
    ToolCallId,
    lingxi_service::toolgateway::PreparedInvocation,
) {
    use lingxi_service::toolgateway::{CallerSurface, InvocationPermissionContext};
    let ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("mcp-settings-consumer"),
        run_id: RunId::new(format!("mcp-diagnostic-{nonce}")),
        attempt: AttemptId::new("1"),
        generation: 1,
    };
    let call = ToolCallId::new(format!("mcp-call-{nonce}"));
    let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        "tool:mcp:fixture:env_report",
        json!({}),
        &SchemaBudget::default(),
    )
    .unwrap();
    let prepared = h
        .state
        .tool_gateway()
        .unwrap()
        .prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            agent,
            InvocationPermissionContext::UserSession {
                mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
            },
            &call,
            &request,
        )
        .expect("实际MCP工具应可准备");
    (ctx, call, prepared)
}
async fn invoke_fixture(h: &Harness, agent: &str, nonce: &str) {
    let (ctx, call, prepared) = prepared(h, agent, nonce);
    let result = h
        .state
        .tool_gateway()
        .unwrap()
        .execute_prepared(&ctx, &call, &prepared.handle)
        .await
        .expect("通过真实网关执行MCP");
    match result.outcome {
        lingxi_kernel::ports::ToolOutcome::Success { result } => {
            let text = result
                .content
                .iter()
                .find_map(|block| match block {
                    lingxi_protocol::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .unwrap();
            let env: Value = serde_json::from_str(text).unwrap();
            assert_eq!(env["LINGXI_T07_SECRET_A"], true);
            assert_eq!(env["HOME"], false);
            assert_eq!(env["PATH"], true);
            assert!(!text.contains("fixture-private-env"));
        }
        other => panic!("自有stdio服务器必须产生实际回执：{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_run_nine_mcp_settings_actions_persist_and_gate_actual_sdk_consumers() {
    let h = Harness::boot().await;
    h.configure().await;
    h.setup_chat().await;
    let a_before = std::fs::read(h.home.join("agents/a/config.yaml")).unwrap();
    let(status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add","payload":{"connector":fixture_connector("fixture")}}))).await;
    assert_eq!(status, 200, "{value}");
    let search = h
        .run_tool(
            "mcp-search",
            "b",
            "ask",
            "update_settings",
            json!({"action":"search","query":"mcp"}),
        )
        .await;
    let search = tool_value(&h.tool_receipt(&search).await);
    for (action, _, _) in lingxi_service::mcp_settings::ACTIONS {
        assert!(
            search["settings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["key"] == *action),
            "缺真实MCP动作{action}：{search}"
        );
    }
    applied(&setting(&h, 1, "mcp.global.enabled", json!(true)).await);
    applied(
        &setting(
            &h,
            2,
            "mcp.connector.start",
            json!({"connectorId":"fixture"}),
        )
        .await,
    );
    invoke_fixture(&h, "b", "initial-b").await;
    let (ctx, call, old) = prepared(&h, "b", "before-refresh");
    applied(
        &setting(
            &h,
            3,
            "mcp.connector.refresh_tools",
            json!({"connectorId":"fixture"}),
        )
        .await,
    );
    let refusal = h
        .state
        .tool_gateway()
        .unwrap()
        .execute_prepared(&ctx, &call, &old.handle)
        .await;
    assert!(
        refusal.is_err(),
        "刷新重新注册后旧句柄不能复活：{refusal:?}"
    );
    invoke_fixture(&h, "b", "after-refresh").await;
    applied(
        &setting(
            &h,
            4,
            "mcp.agent.connector.enable",
            json!({"connectorId":"fixture","enabled":false}),
        )
        .await,
    );
    let b_config = lingxi_adapters::storage::agent_memory::config::read_yaml_mapping_as_json(
        &h.home.join("agents/b/config.yaml"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(b_config["mcp"]["connectors"]["fixture"]["enabled"], false);
    assert_eq!(
        std::fs::read(h.home.join("agents/a/config.yaml")).unwrap(),
        a_before
    );
    let gateway = h.state.tool_gateway().unwrap();
    let args = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        "tool:mcp:fixture:env_report",
        json!({}),
        &SchemaBudget::default(),
    )
    .unwrap();
    let denied = gateway.prepare_from_request(
        &ctx,
        lingxi_service::toolgateway::CallerSurface::UserRun,
        "b",
        lingxi_service::toolgateway::InvocationPermissionContext::UserSession {
            mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
        },
        &ToolCallId::new("disabled-b"),
        &args,
    );
    assert!(denied.is_err(), "冻结助手B的关闭设置必须由真实准备面消费");
    invoke_fixture(&h, "a", "a-isolated").await;
    applied(
        &setting(
            &h,
            5,
            "mcp.agent.connector.enable",
            json!({"connectorId":"fixture","enabled":true}),
        )
        .await,
    );
    applied(
        &setting(
            &h,
            6,
            "mcp.agent.tool.enable",
            json!({"connectorId":"fixture","toolName":"env_report","enabled":false}),
        )
        .await,
    );
    let denied = gateway.prepare_from_request(
        &ctx,
        lingxi_service::toolgateway::CallerSurface::UserRun,
        "b",
        lingxi_service::toolgateway::InvocationPermissionContext::UserSession {
            mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
        },
        &ToolCallId::new("disabled-b-tool"),
        &args,
    );
    assert!(denied.is_err(), "单工具关闭必须由真实准备面消费");
    invoke_fixture(&h, "a", "a-tool-isolated").await;
    applied(
        &setting(
            &h,
            7,
            "mcp.agent.tool.enable",
            json!({"connectorId":"fixture","toolName":"env_report","enabled":true}),
        )
        .await,
    );
    let old_config = persisted(&h);
    let updated=setting(&h,8,"mcp.connector.update",json!({"connectorId":"fixture","patch":{"name":"renamed","authorizationToken":"********","env":"********"}})).await;
    applied(&updated);
    let new_config = persisted(&h);
    let connector = &new_config["global"]["mcp"]["connectors"][0];
    assert_eq!(connector["name"], "renamed");
    assert!(
        connector["authorizationToken"]
            == old_config["global"]["mcp"]["connectors"][0]["authorizationToken"]
    );
    assert!(connector["env"] == old_config["global"]["mcp"]["connectors"][0]["env"]);
    assert!(
        gateway
            .registry()
            .describe(&ToolTargetId::parse("tool:mcp:fixture:env_report"))
            .is_err(),
        "更新后旧连接与目标必须撤销"
    );
    applied(
        &setting(
            &h,
            9,
            "mcp.connector.start",
            json!({"connectorId":"fixture"}),
        )
        .await,
    );
    invoke_fixture(&h, "b", "updated-b").await;
    let (before_stop_ctx, before_stop_call, before_stop_handle) =
        prepared(&h, "b", "before-stop-restart");
    applied(
        &setting(
            &h,
            10,
            "mcp.connector.stop",
            json!({"connectorId":"fixture"}),
        )
        .await,
    );
    let manager = h.state.mcp_settings();
    manager.restore().await.unwrap();
    let (status, state) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state", None)
        .await;
    assert_eq!(status, 200, "{state}");
    assert_eq!(state["config"]["connectors"][0]["enabled"], false);
    assert_eq!(state["config"]["connectors"][0]["running"], false);
    assert!(!state.to_string().contains("fixture-private-env"));
    assert!(!state.to_string().contains("fixture-private-token"));
    assert!(state["config"]["connectors"][0].get("command").is_none());
    assert!(state["config"]["connectors"][0].get("args").is_none());
    assert!(
        state["config"]["servers"][0].get("command").is_none(),
        "旧别名同样不能绕过最小公开投影"
    );
    assert!(state["config"]["servers"][0].get("args").is_none());
    let (status, restarted) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.start","payload":{"connectorId":"fixture"}})),
        )
        .await;
    assert_eq!(status, 200, "{restarted}");
    let rejected = gateway
        .execute_prepared(
            &before_stop_ctx,
            &before_stop_call,
            &before_stop_handle.handle,
        )
        .await;
    assert!(
        rejected.is_err(),
        "相同ID启停后旧批准句柄不能复活：{rejected:?}"
    );
    invoke_fixture(&h, "b", "after-same-id-stop-restart").await;
    // HTTP传输已接入官方SDK；这里刻意填入只提供模型接口的自有/v1地址。
    // 可以保存连接器意图，但真实MCP握手失败必须给出精确连接错误。
    applied(
        &setting(
            &h,
            11,
            "mcp.connector.add",
            json!({"id":"remote","name":"remote","url":h.endpoint}),
        )
        .await,
    );
    let failed = setting(
        &h,
        12,
        "mcp.connector.start",
        json!({"connectorId":"remote"}),
    )
    .await;
    assert_eq!(
        failed.settings_update.status,
        lingxi_protocol::McpSettingsStatus::Failed
    );
    assert!(failed.settings_update.saved);
    assert!(!failed.settings_update.runtime_applied);
    assert_eq!(
        failed.settings_update.error.unwrap().code,
        "mcp_connect_failed"
    );
    let bytes = std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap();
    let denied=setting(&h,13,"mcp.connector.update",json!({"connectorId":"fixture","patch":{"command":"must-not-run","env":{"LEAK":"must-not-write"}}})).await;
    assert_eq!(
        denied.settings_update.status,
        lingxi_protocol::McpSettingsStatus::Failed
    );
    assert_eq!(
        denied.settings_update.error.unwrap().code,
        "mcp_boundary_user_only"
    );
    assert!(!denied.settings_update.saved);
    assert_eq!(
        std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap(),
        bytes
    );
    applied(
        &setting(
            &h,
            14,
            "mcp.connector.remove",
            json!({"connectorId":"remote"}),
        )
        .await,
    );
    applied(
        &setting(
            &h,
            15,
            "mcp.connector.remove",
            json!({"connectorId":"fixture"}),
        )
        .await,
    );
    applied(&setting(&h, 16, "mcp.global.enabled", json!(false)).await);
    let document = persisted(&h);
    assert_eq!(document["unknownRoot"], 73);
    assert_eq!(document["global"]["keep"], "global");
    assert_eq!(document["global"]["mcp"]["unknownMcp"], 79);
    assert_eq!(document["global"]["mcp"]["deferEnabled"], false);
    assert_eq!(document["global"]["mcp"]["deferThreshold"], 17);
    assert_eq!(document["global"]["mcp"]["servers"], json!([]));
    assert_eq!(document["agents"]["legacy"]["keep"], true);
    for request in h.observed() {
        assert_eq!(request.path, "chat");
        assert_eq!(request.headers["authorization"], "Bearer saved-fixture-key");
        assert_eq!(request.body["model"], "fixture-chat");
    }
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn management_rejects_other_identities_corrupt_files_links_and_replaced_parent() {
    let h = Harness::boot().await;
    let token = h
        .state
        .auth()
        .issue_device_credential(
            "user_local",
            &[
                "settings.write",
                "settings.read",
                "secrets.write",
                "providers.manage",
            ],
            None,
        )
        .unwrap()
        .secret;
    for (method, path, body) in [
        (reqwest::Method::GET, "/lingxi/v1/mcp/state", None),
        (
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.enabled","payload":{"enabled":true}})),
        ),
    ] {
        let (status, value) = h.request_as(&token, method, path, body).await;
        assert_eq!(status, 403, "{value}");
    }
    let path = h.home.join("plugin-data/mcp/config.json");
    std::fs::write(&path, b"{broken").unwrap();
    assert!(
        h.state.mcp_settings().restore().await.is_err(),
        "损坏配置必须阻止恢复，不能当空表"
    );
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.enabled","payload":{"enabled":true}})),
        )
        .await;
    assert_eq!(status, 500, "{value}");
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    #[cfg(unix)]
    {
        let outside = h.home.join("outside-private-config");
        std::fs::write(&outside, b"outside-must-stay").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/mcp/settings-action",
                Some(json!({"action":"mcp.global.enabled","payload":{"enabled":true}})),
            )
            .await;
        assert_eq!(status, 500, "{value}");
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside-must-stay");
        std::fs::remove_file(&path).unwrap();
    }
    let parent = path.parent().unwrap();
    std::fs::rename(parent, h.home.join("old-mcp-parent")).unwrap();
    std::fs::create_dir(parent).unwrap();
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.enabled","payload":{"enabled":true}})),
        )
        .await;
    assert_eq!(status, 500, "{value}");
    assert!(!path.exists(), "不能在已替换的父目录写入");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_provided_connector_shapes_never_write_and_schema_corruption_blocks_restore() {
    let h = Harness::boot().await;
    let path = h.home.join("plugin-data/mcp/config.json");
    let original = std::fs::read(&path).unwrap();
    for (field, bad) in [
        ("transport", json!(42)),
        ("transport", json!("invented")),
        ("cwd", json!([])),
        ("enabled", json!("yes")),
        ("args", json!([1])),
        ("env", json!({"KEY":false})),
        ("authorizationToken", json!({"token":"must-not-echo"})),
        ("oauth", json!([])),
    ] {
        let mut connector = fixture_connector("invalid");
        connector[field] = bad;
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/mcp/settings-action",
                Some(json!({"action":"mcp.connector.add","payload":{"connector":connector}})),
            )
            .await;
        assert_eq!(status, 400, "{field} 显式坏值必须拒绝：{value}");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!value.to_string().contains("must-not-echo"));
    }
    for patch in [
        json!({"schemaVersion":2}),
        json!({"agents":[]}),
        json!({"sessions":false}),
        json!({"global":{"mcp":{"enabled":"yes","connectors":[]}}}),
        json!({"global":{"mcp":{"enabled":true,"connectors":[{"id":"bad","transport":"stdio","command":42}]}}}),
    ] {
        std::fs::write(&path, patch.to_string()).unwrap();
        let corrupt = std::fs::read(&path).unwrap();
        assert!(
            h.state.mcp_settings().restore().await.is_err(),
            "坏的持久文件不能作为空配置恢复：{patch}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    }
    std::fs::write(&path, original).unwrap();
    h.close().await;
}

#[tokio::test]
async fn an_unconfigured_mcp_read_and_restore_do_not_create_directories_or_block_the_service() {
    let home = std::env::temp_dir().join(format!(
        "lingxi-r07-mcp-empty-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let memory = Arc::new(lingxi_service::memory::MemoryService::new(
        home.clone(),
        home.join("runtime"),
    ));
    let manager = lingxi_service::mcp_settings::McpSettingsManager::new(
        memory,
        Arc::new(lingxi_service::appevents::AppEventLog::new()),
        None,
    );
    manager
        .restore()
        .await
        .expect("没有配置时无需MCP网关或写目录能力");
    let state = manager.state().await.unwrap();
    assert_eq!(state["config"]["enabled"], false);
    assert_eq!(state["config"]["connectors"], json!([]));
    assert!(
        !home.join("plugin-data").exists(),
        "只读空状态不能创建MCP目录"
    );
    manager.shutdown().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_registration_never_exposes_a_bare_executor_to_disabled_agent() {
    let h = Harness::boot().await;
    let(status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add","payload":{"connector":fixture_connector("fixture"),"enableGlobal":true}}))).await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/agents/b/config",
            Some(json!({"mcp":{"connectors":{"fixture":{"enabled":false}}}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered)));
    let (release, blocked) = std::sync::mpsc::sync_channel::<()>(1);
    let blocked = Arc::new(Mutex::new(blocked));
    h.state
        .mcp_settings()
        .set_registration_observer(Some(Arc::new(move |target| {
            assert_eq!(target.as_str(), "tool:mcp:fixture:env_report");
            if let Some(entered) = entered.lock().unwrap().take() {
                entered.send(()).unwrap();
                blocked
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .expect("测试应释放真实注册时点");
            }
        })));
    {
        let start = h.request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.start","payload":{"connectorId":"fixture"}})),
        );
        tokio::pin!(start);
        tokio::select! {result=&mut start=>panic!("注册观察点前不能已结束：{result:?}"),result=waiting=>result.expect("真实SDK必须完成initialize/tools-list并进入binder"),_=tokio::time::sleep(std::time::Duration::from_secs(3))=>panic!("真实注册未到观察点")};
        let ctx = RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: SessionId::new("registration-window"),
            run_id: RunId::new("registration-window"),
            attempt: AttemptId::new("1"),
            generation: 1,
        };
        let call = ToolCallId::new("registration-window-call");
        let args = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
            "tool:mcp:fixture:env_report",
            json!({}),
            &SchemaBudget::default(),
        )
        .unwrap();
        let gateway = h.state.tool_gateway().unwrap();
        let prepare = gateway.prepare_from_request(
            &ctx,
            lingxi_service::toolgateway::CallerSurface::UserRun,
            "b",
            lingxi_service::toolgateway::InvocationPermissionContext::UserSession {
                mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
            },
            &call,
            &args,
        );
        if let Ok(prepared) = prepare {
            assert!(
                gateway
                    .execute_prepared(&ctx, &call, &prepared.handle)
                    .await
                    .is_err(),
                "正在注册时也不得绕过B的开关调用裸执行器"
            );
        }
        release.send(()).unwrap();
        let (status, value) = start.await;
        assert_eq!(status, 200, "{value}");
    }
    h.state.mcp_settings().set_registration_observer(None);
    invoke_fixture(&h, "a", "after-registration").await;
    h.close().await;
}

#[cfg(unix)]
fn owned_platform(home: &std::path::Path, id: &str, mode: &str) -> (Value, PathBuf) {
    let root = home.join(format!("external-{id}"));
    std::fs::create_dir_all(&root).unwrap();
    let script = root.join("platform.mjs");
    std::fs::write(&script,r#"
import fs from 'node:fs';import readline from 'node:readline';import {spawn} from 'node:child_process';
const root=process.argv[2],mode=process.argv[3];
const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});
fs.writeFileSync(root+'/pids.json',JSON.stringify({parent:process.pid,child:child.pid}));
setInterval(()=>{},1000);
const send=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
readline.createInterface({input:process.stdin}).on('line',line=>{
 const m=JSON.parse(line);if(!m.method)return;fs.appendFileSync(root+'/arrivals.ndjson',JSON.stringify({method:m.method})+'\n');
 if(m.method==='initialize'){
  fs.writeFileSync(root+'/initialize.json','{}');
  const answer=()=>send(m.id,{protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'owned-platform',version:'1'}});
  if(mode==='hold-init'){const timer=setInterval(()=>{if(fs.existsSync(root+'/release')){clearInterval(timer);answer();}},10);}else answer();
 }
 if(m.method==='tools/list')send(m.id,{tools:[{name:'env_report',description:'owned MCP tool',inputSchema:{type:'object',properties:{}}}]});
 if(m.method==='tools/call'){fs.writeFileSync(root+'/call.json','{}');if(mode!=='hold-call')send(m.id,{content:[{type:'text',text:'owned-mcp-completed'}]});}
});
"#).unwrap();
    (
        json!({"id":id,"transport":"stdio","command":"node","args":[script,root,mode],"name":id}),
        root,
    )
}
#[cfg(unix)]
async fn marker(path: &std::path::Path) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("自有平台未到达实际请求：{}", path.display()));
}
#[cfg(unix)]
async fn processes_gone(root: &std::path::Path) {
    let pids: Value =
        serde_json::from_slice(&std::fs::read(root.join("pids.json")).unwrap()).unwrap();
    let pids = [
        pids["parent"].as_i64().unwrap() as i32,
        pids["child"].as_i64().unwrap() as i32,
    ];
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while pids.iter().any(|pid| unsafe { libc::kill(*pid, 0) == 0 }) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("自有MCP进程组必须真实退出，不能只关闭等待");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_an_actual_inflight_mcp_call_records_unknown_without_replay_and_reaps_group() {
    let h = Harness::boot().await;
    h.configure().await;
    h.setup_chat().await;
    let (connector, root) = owned_platform(&h.home, "fixture", "hold-call");
    let(status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add","payload":{"connector":connector,"enableGlobal":true}}))).await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.start","payload":{"connectorId":"fixture"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let run = {
        let run = h.run_tool("mcp-inflight-stop", "b", "operate", "env_report", json!({}));
        tokio::pin!(run);
        let arrived = root.join("call.json");
        tokio::select! {_ = marker(&arrived)=>{},result=&mut run=>panic!("外端未返回前Run不应完成：{}",h.tool_receipt(&result).await)};
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/mcp/settings-action",
                Some(json!({"action":"mcp.connector.stop","payload":{"connectorId":"fixture"}})),
            )
            .await;
        assert_eq!(status, 200, "{value}");
        assert_eq!(value["settingsUpdate"]["saved"], true);
        assert_eq!(value["settingsUpdate"]["runtimeApplied"], true);
        run.await
    };
    let receipt=h.state.storage().query_one_text("SELECT json_object('phase',phase,'outcome',receipt_outcome) FROM invocation_journal WHERE run_id=?1 AND target='tool:mcp:fixture:env_report'",vec![run]).await.unwrap().expect("实际MCP调用必须记入journal");
    let receipt: Value = serde_json::from_str(&receipt).unwrap();
    assert_eq!(receipt["phase"], "unknown");
    assert_eq!(receipt["outcome"], "unknown");
    let calls = std::fs::read_to_string(root.join("arrivals.ndjson"))
        .unwrap()
        .lines()
        .filter(|line| serde_json::from_str::<Value>(line).unwrap()["method"] == "tools/call")
        .count();
    assert_eq!(calls, 1, "丢回执的MCP写调用不得盲目重放");
    processes_gone(&root).await;
    h.close().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_real_settings_run_interrupts_initialization_and_preserves_saved_intent() {
    let h = Harness::boot().await;
    h.configure().await;
    h.setup_chat().await;
    let (connector, root) = owned_platform(&h.home, "slow", "hold-init");
    let(status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add","payload":{"connector":connector,"enableGlobal":true}}))).await;
    assert_eq!(status, 200, "{value}");
    let run = {
        let run=h.run_tool("mcp-init-cancel","b","ask","update_settings",json!({"action":"apply","key":"mcp.connector.start","value":"{\"connectorId\":\"slow\"}"}));
        tokio::pin!(run);
        let initialized = root.join("initialize.json");
        tokio::select! {_ = marker(&initialized)=>{},result=&mut run=>panic!("初始化未返回前Run不应完成：{}",h.tool_receipt(&result).await)};
        let id=h.state.storage().query_one_text("SELECT run_id FROM invocation_journal WHERE session_id=?1 AND target='tool:first-party:update_settings' AND phase='started' LIMIT 1",vec!["mcp-init-cancel".into()]).await.unwrap().expect("取消须针对实际进入工具的Run");
        h.state.runs().cancel_run(&id, "测试取消实际MCP初始化");
        run.await
    };
    assert_eq!(
        h.state
            .storage()
            .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run])
            .await
            .unwrap()
            .as_deref(),
        Some("cancelled")
    );
    let (status, state) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state", None)
        .await;
    assert_eq!(status, 200, "{state}");
    let receipt = state["settingsResults"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["settingsUpdate"]["action"] == "mcp.connector.start")
        .expect("被取消的实际设置请求必须留下回执");
    assert_eq!(receipt["settingsUpdate"]["status"], "cancelled");
    assert_eq!(receipt["settingsUpdate"]["saved"], true);
    assert_eq!(receipt["settingsUpdate"]["runtimeApplied"], false);
    assert_eq!(
        persisted(&h)["global"]["mcp"]["connectors"][0]["enabled"],
        true,
        "取消不能抹去已保存的启动意图"
    );
    processes_gone(&root).await;
    h.close().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn occupied_listen_port_cleans_restored_mcp_and_knowledge_while_external_state_is_retained() {
    let seed = Harness::boot().await;
    let home = seed.home.clone();
    let session = "restored-loop-before-listen-failure";
    seed.state
        .session_admin()
        .create_session(
            session,
            "a",
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
            "监听失败前的真实循环",
            Some("ask"),
            &[],
            false,
            1_790_000_000_000,
        )
        .await
        .unwrap();
    seed.state
        .loop_services()
        .store
        .create(
            &lingxi_service::loopx::LoopTarget::desktop(session),
            "保留原循环及闹钟",
            None,
        )
        .unwrap();
    seed.state
        .loop_services()
        .alarm
        .schedule(session, 3600, "等待原任务", false)
        .unwrap();
    seed.close_runtime(false).await;
    let stored_before: Value =
        serde_json::from_slice(&std::fs::read(home.join(".ephemeral/loop-state.json")).unwrap())
            .unwrap();
    assert_eq!(stored_before["loops"][0]["status"], "running");
    assert!(stored_before["loops"][0]["alarm"]["wakeAt"].is_u64());
    let (connector, root) = owned_platform(&home, "fixture", "normal");
    std::fs::create_dir_all(home.join("plugin-data/mcp")).unwrap();
    std::fs::write(
        home.join("plugin-data/mcp/config.json"),
        json!({"schemaVersion":1,"global":{"mcp":{"enabled":true,"connectors":[connector]}}})
            .to_string(),
    )
    .unwrap();
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let clock = Arc::new(lingxi_service::inject::SystemClock);
    let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
        clock.clone(),
    ));
    let gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
        Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new()),
        approvals.clone(),
        clock.clone(),
        SchemaBudget::default(),
        300000,
        1000,
    ));
    let layout = prepare_layout(&home).unwrap();
    let state = ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: occupied.local_addr().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: 1000,
        },
        &layout,
        ServiceDeps {
            tool_gateway: Some(gateway),
            approval_gate: Some(approvals),
            clock,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let retained = state.clone();
    let restored = retained
        .loop_services()
        .controller
        .tool_status(session)
        .expect("同home启动必须恢复原循环");
    assert_eq!(restored.status, lingxi_service::loopx::LoopStatus::Running);
    assert!(restored.alarm.is_some());
    assert!(!retained.loop_services().controller.is_disposed());
    assert!(
        retained
            .storage()
            .session_admission()
            .register_release_listener(Arc::new(|_| {}))
            .is_err(),
        "失败前真实idle监听应占用唯一槽"
    );
    assert_eq!(
        retained.mcp_settings().state().await.unwrap()["config"]["connectors"][0]["running"],
        true
    );
    let knowledge_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.join("knowledge/ingestion-worker.lock"))
        .unwrap();
    assert!(
        knowledge_lock.try_lock().is_err(),
        "失败前真实知识worker应持有锁"
    );
    let result = lingxi_service::run(
        state,
        std::future::pending(),
        |_| panic!("占用端口不能报告已监听"),
        None,
    )
    .await;
    assert!(
        matches!(result, Err(lingxi_service::ServiceError::Bind { .. })),
        "占端口应明确拒绝且清理已确认：{result:?}"
    );
    assert_eq!(
        retained.mcp_settings().state().await.unwrap()["config"]["connectors"][0]["running"],
        false
    );
    processes_gone(&root).await;
    assert!(
        retained.loop_services().controller.is_disposed(),
        "外部State存活也不能继续循环调度"
    );
    let alarm_error = retained
        .loop_services()
        .alarm
        .schedule(session, 1, "after failed listen", false)
        .unwrap_err();
    assert_eq!(alarm_error.code, "loop_alarm_stopped");
    let released = retained
        .storage()
        .session_admission()
        .register_release_listener(Arc::new(|_| {}))
        .expect("监听失败必须真正卸载idle释放监听");
    drop(released);
    let stored_after: Value =
        serde_json::from_slice(&std::fs::read(home.join(".ephemeral/loop-state.json")).unwrap())
            .unwrap();
    assert_eq!(
        stored_after["loops"][0]["status"], "running",
        "关闭运行时不能伪改持久循环状态"
    );
    assert_eq!(
        stored_after["loops"][0]["alarm"],
        stored_before["loops"][0]["alarm"]
    );
    knowledge_lock
        .try_lock()
        .expect("即使外部State仍存活，启动失败也必须真正结束worker并释放文件锁");
    retained.storage().close().await.unwrap();
    drop(retained);
    drop(knowledge_lock);
    drop(occupied);
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_web_login_cannot_gain_local_settings_authority_through_an_approved_model_tool() {
    let h = Harness::boot().await;
    h.configure().await;
    h.setup_chat().await;
    let (status, body) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/access/account/profile",
            Some(json!({"username":"settings-web-user","displayName":"设置网页用户"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/access/account/password",
            Some(json!({"password":"private-settings-web-password"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let response = reqwest::Client::new().post(format!("{}/lingxi/v1/web-auth/login", h.base)).json(&json!({"username":"settings-web-user","password":"private-settings-web-password","clientKind":"web"})).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body: Value = response.json().await.unwrap();
    assert!(!body.to_string().contains("private-settings-web-password"));
    let status = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/web-auth/session", h.base))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(status.status(), reqwest::StatusCode::OK);
    let status: Value = status.json().await.unwrap();
    assert_eq!(status["principal"]["credentialKind"], "web_session");
    assert!(status["principal"]["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|scope| scope != "settings.write"));
    let path = h.home.join("plugin-data/mcp/config.json");
    let before = std::fs::read(&path).unwrap();
    let run = h
        .run_tool_as(
            "web-settings-no-upgrade",
            "b",
            "ask",
            "update_settings",
            json!({"action":"apply","key":"mcp.global.enabled","value":"true"}),
            Some(&cookie),
        )
        .await;
    let receipt: Value = serde_json::from_str(&h.tool_receipt(&run).await).unwrap();
    assert_eq!(
        receipt["event"]["result"]["status"], "failed",
        "网页聊天不能提升成本机设置授权：{receipt}"
    );
    assert!(
        receipt.to_string().contains("原始设置权限"),
        "拒绝必须来自冻结的真实入口身份：{receipt}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        h.state.mcp_settings().state().await.unwrap()["config"]["enabled"],
        false
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_pending_settings_approval_does_not_survive_local_credential_replacement() {
    let h = Harness::boot().await;
    h.configure().await;
    h.setup_chat().await;
    let session = "settings-old-local-credential";
    h.state
        .session_admin()
        .create_session(
            session,
            "b",
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
            "原授权撤换",
            Some("ask"),
            &[],
            false,
            1_790_000_000_000,
        )
        .await
        .unwrap();
    *h.external.next_tool.lock().unwrap() = Some(
        json!({"name":"update_settings","arguments":{"action":"apply","key":"mcp.global.enabled","value":"true"}}),
    );
    let before = std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap();
    let (status, response) = {
        let path = format!("/lingxi/v1/sessions/{session}/execute");
        let response = h.request(
            reqwest::Method::POST,
            &path,
            Some(json!({"input":"执行设置，等待真实批准"})),
        );
        tokio::pin!(response);
        let approval = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Some(pending) = h
                    .approvals
                    .pending_for_principal(&lingxi_kernel::Principal::LocalUser, session)
                    .into_iter()
                    .next()
                {
                    assert_eq!(pending.target.as_str(), "tool:first-party:update_settings");
                    break pending;
                }
                tokio::select! {
                    result=&mut response => panic!("真实审批出现前不能终结：{result:?}"),
                    _=tokio::time::sleep(std::time::Duration::from_millis(10))=>{}
                }
            }
        })
        .await
        .expect("必须先进入真实设置审批等待");
        // 用现役认证启动入口轮换同home本机凭证；准备记录没有保存任何token正文。
        let layout = prepare_layout(&h.home).unwrap();
        let replacement = lingxi_service::auth::AuthService::bootstrap(
            &layout,
            "replacement-during-settings-review",
        )
        .unwrap();
        assert_ne!(replacement.local_token(), h.state.auth().local_token());
        assert!(h
            .approvals
            .answer_for_principal(
                &lingxi_kernel::Principal::LocalUser,
                session,
                &approval.approval_id,
                lingxi_service::approval_service::Answer::Approve
            )
            .settled());
        response.await
    };
    assert_eq!(status, 200, "{response}");
    let run = response["runId"].as_str().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let status = h
                .state
                .storage()
                .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run.into()])
                .await
                .unwrap()
                .unwrap();
            if matches!(
                status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted_needs_attention"
            ) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("原设置Run必须真实结算");
    let receipt: Value = serde_json::from_str(&h.tool_receipt(run).await).unwrap();
    assert_eq!(
        receipt["event"]["result"]["status"], "failed",
        "原批准不能恢复已撤换的本机凭证：{receipt}"
    );
    assert!(
        receipt.to_string().contains("原始设置凭证已失效"),
        "必须明确归因原凭证失效：{receipt}"
    );
    assert_eq!(
        std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap(),
        before
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restoring_lazy_lifecycle_keeps_saved_intent_without_starting_external_processes() {
    let h = Harness::boot().await;
    for (id, mode) in [
        ("lazy-fixture", "lazy"),
        ("lazy-keep-fixture", "lazy-keep-alive"),
    ] {
        let mut connector = fixture_connector(id);
        connector["lifecycle"] = json!(mode);
        let (status, result) = h.request(reqwest::Method::POST, "/lingxi/v1/mcp/settings-action", Some(json!({"action":"mcp.connector.add","payload":{"connector":connector,"enableGlobal":true}}))).await;
        assert_eq!(status, 200, "{result}");
    }
    h.state.mcp_settings().restore().await.unwrap();
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state", None)
        .await;
    assert_eq!(status, 200, "{value}");
    for connector in value["config"]["connectors"].as_array().unwrap() {
        assert_eq!(connector["enabled"], true);
        assert_eq!(
            connector["running"], false,
            "按需配置恢复不能擅自启动：{connector}"
        );
        assert!(connector["error"].is_null());
        let target = ToolTargetId::parse(&format!(
            "tool:mcp:{}:env_report",
            connector["id"].as_str().unwrap()
        ));
        assert!(h
            .state
            .tool_gateway()
            .unwrap()
            .registry()
            .describe(&target)
            .is_err());
    }
    let (status, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.start","payload":{"connectorId":"lazy-fixture"}})),
        )
        .await;
    assert_eq!(status, 200, "显式启动仍须连接真实stdio：{result}");
    assert_eq!(
        h.state.mcp_settings().state().await.unwrap()["config"]["connectors"][0]["running"],
        true
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_mcp_catalog_crosses_the_shared_defer_threshold_without_losing_other_sources() {
    let h = Harness::boot_with_defer(true, 2).await;
    let registry = h.state.tool_gateway().unwrap().registry();
    let (_, before) = registry.model_declaration_snapshot();
    let builtin = before
        .iter()
        .map(|row| row.listing.target_id.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut generations = Vec::new();
    for (index, id) in ["fixture", "threshold-b", "threshold-c"]
        .into_iter()
        .enumerate()
    {
        let (status, value) = h.request(reqwest::Method::POST, "/lingxi/v1/mcp/settings-action", Some(json!({"action":"mcp.connector.add","payload":{"connector":fixture_connector(id),"enableGlobal":true}}))).await;
        assert_eq!(status, 200, "{value}");
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/mcp/settings-action",
                Some(json!({"action":"mcp.connector.start","payload":{"connectorId":id}})),
            )
            .await;
        assert_eq!(status, 200, "{value}");
        let (generation, rows) = registry.model_declaration_snapshot();
        let description = registry
            .describe(&ToolTargetId::parse(&format!("tool:mcp:{id}:env_report")))
            .unwrap();
        assert_eq!(
            description.availability,
            if index < 2 {
                lingxi_kernel::toolcatalog::Availability::Available
            } else {
                lingxi_kernel::toolcatalog::Availability::Deferred
            },
            "真实目录形状须与声明策略一致"
        );
        generations.push(generation);
        let mcp = rows
            .iter()
            .filter(|row| row.listing.target_id.as_str().starts_with("tool:mcp:"))
            .count();
        assert_eq!(
            mcp,
            if index < 2 { index + 1 } else { 0 },
            "严格超过门槛才全源延迟"
        );
        let remaining = rows
            .iter()
            .filter(|row| !row.listing.target_id.as_str().starts_with("tool:mcp:"))
            .map(|row| row.listing.target_id.as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(remaining, builtin, "其他来源的声明不能被MCP配置重置");
        for running in ["fixture", "threshold-b", "threshold-c"]
            .into_iter()
            .take(index + 1)
        {
            assert!(
                registry
                    .describe(&ToolTargetId::parse(&format!(
                        "tool:mcp:{running}:env_report"
                    )))
                    .is_ok(),
                "延迟不等于移除真实工具目录"
            );
        }
    }
    assert!(generations.windows(2).all(|pair| pair[1] > pair[0]));
    let held = registry.model_declaration_snapshot().0;
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.stop","payload":{"connectorId":"threshold-c"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let (generation, rows) = registry.model_declaration_snapshot();
    assert!(generation > held);
    assert_eq!(
        rows.iter()
            .filter(|row| row.listing.target_id.as_str().starts_with("tool:mcp:"))
            .count(),
        2,
        "回落门槛后恢复真实常驻声明"
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_authenticated_http_request_waiting_for_the_real_media_lock_cannot_use_a_replaced_owner_credential(
) {
    let h = Harness::boot().await;
    let before = std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap();
    let generation = h
        .state
        .tool_gateway()
        .unwrap()
        .registry()
        .model_declaration_snapshot()
        .0;
    let arrived = Arc::new(tokio::sync::Notify::new());
    let observer = arrived.clone();
    h.state
        .mcp_settings()
        .set_management_request_observer(Some(Arc::new(move |action| {
            assert_eq!(action, "mcp.global.enabled");
            observer.notify_one();
        })));
    let lease = h.state.media().authority_mutation_lease().await;
    let (status, value) = {
        let request = h.request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.enabled","payload":{"enabled":true}})),
        );
        tokio::pin!(request);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::select! {
                _=arrived.notified()=>{},
                result=&mut request=>panic!("持真实media锁时HTTP不应提前完成：{result:?}"),
            }
        })
        .await
        .expect("真实HTTP必须先通过身份和形状校验并到达锁前观测点");
        let layout = prepare_layout(&h.home).unwrap();
        let _replacement = lingxi_service::auth::AuthService::bootstrap(
            &layout,
            "replacement-during-http-media-wait",
        )
        .unwrap();
        drop(lease);
        request.await
    };
    assert_eq!(status, 403, "原凭证已撤换的排队HTTP必须明确拒绝：{value}");
    assert_eq!(
        value["settingsUpdate"]["error"]["code"],
        "mcp_original_authority_revoked"
    );
    assert_eq!(value["settingsUpdate"]["saved"], false);
    assert_eq!(
        std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap(),
        before
    );
    assert_eq!(
        h.state
            .tool_gateway()
            .unwrap()
            .registry()
            .model_declaration_snapshot()
            .0,
        generation
    );
    h.state.mcp_settings().set_management_request_observer(None);
    h.close().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_saved_start_is_immediate_and_duplicate_start_uses_one_actual_sdk_connection() {
    let h = Harness::boot().await;
    let (connector, root) = owned_platform(&h.home, "fixture", "hold-init");
    let (status, value) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        h.request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.add",
        "payload":{"connector":connector,"enableGlobal":true,"startIfEnabled":true}})),
        ),
    )
    .await
    .expect("保存回执不能等待自有外端握手或10秒连接上限");
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["settingsUpdate"]["saved"], true);
    assert_eq!(value["settingsUpdate"]["runtimeApplied"], false);
    marker(&root.join("initialize.json")).await;
    assert!(!root.join("release").exists());
    let (_, state) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=a", None)
        .await;
    assert_eq!(state["config"]["connectors"][0]["status"], "connecting");
    assert_eq!(state["config"]["connectors"][0]["running"], false);
    let (arrived, waiting) = tokio::sync::oneshot::channel();
    let arrived = Arc::new(Mutex::new(Some(arrived)));
    h.state
        .mcp_settings()
        .set_management_request_observer(Some(Arc::new(move |action| {
            if action == "mcp.connector.start" {
                if let Some(arrived) = arrived.lock().unwrap().take() {
                    let _ = arrived.send(());
                }
            }
        })));
    let start = h.request(
        reqwest::Method::POST,
        "/lingxi/v1/mcp/settings-action",
        Some(json!({"action":"mcp.connector.start","payload":{"connectorId":"fixture"}})),
    );
    let release = async {
        waiting.await.expect("重复启动实际进入同管理器HTTP入口");
        std::fs::write(root.join("release"), b"release").unwrap();
    };
    let ((status, value), ()) = tokio::join!(start, release);
    assert_eq!(status, 200, "{value}");
    h.state.mcp_settings().set_management_request_observer(None);
    let state = settled_state(&h).await;
    assert_eq!(state["config"]["connectors"][0]["running"], true);
    let arrivals = std::fs::read_to_string(root.join("arrivals.ndjson")).unwrap();
    assert_eq!(
        arrivals
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).unwrap()["method"] == "initialize")
            .count(),
        1,
        "重复启动不能再建立一个真实SDK连接"
    );
    let (ctx, call, prepared) = prepared(&h, "a", "queued-connection");
    let actual = h
        .state
        .tool_gateway()
        .unwrap()
        .execute_prepared(&ctx, &call, &prepared.handle)
        .await
        .unwrap();
    assert!(matches!(
        actual.outcome,
        lingxi_kernel::ports::ToolOutcome::Success { .. }
    ));
    marker(&root.join("call.json")).await;
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.stop","payload":{"connectorId":"fixture"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    processes_gone(&root).await;
    h.close().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_bulk_pending_start_does_not_block_other_saves_and_stop_disable_reap_real_groups() {
    let h = Harness::boot().await;
    let (one, first) = owned_platform(&h.home, "one", "hold-init");
    let (two, second) = owned_platform(&h.home, "two", "hold-init");
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.enabled","payload":{"enabled":true}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        h.request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.add_bulk","payload":{"connectors":[one,two]}})),
        ),
    )
    .await
    .expect("整批保存不得串行等待每个10秒连接");
    assert_eq!(status, 200, "{value}");
    assert!(value["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["ok"] == true && row["saved"] == true && row["runtimeApplied"] == false));
    marker(&first.join("initialize.json")).await;
    marker(&second.join("initialize.json")).await;
    let (status, value) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        h.request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.defer","payload":{"deferThreshold":9}})),
        ),
    )
    .await
    .expect("慢连接不得阻挡其它设置保存");
    assert_eq!(status, 200, "{value}");
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.stop","payload":{"connectorId":"one"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    processes_gone(&first).await;
    let (_, state) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=a", None)
        .await;
    assert_eq!(state["config"]["connectors"][0]["enabled"], false);
    assert_eq!(state["config"]["connectors"][1]["status"], "connecting");
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.global.enabled","payload":{"enabled":false}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    processes_gone(&second).await;
    std::fs::write(first.join("release"), b"late").unwrap();
    std::fs::write(second.join("release"), b"late").unwrap();
    let state = settled_state(&h).await;
    assert!(state["config"]["connectors"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["running"] == false));
    assert_eq!(persisted(&h)["global"]["mcp"]["deferThreshold"], 9);
    h.close().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_original_authority_revocation_and_shutdown_cancel_and_reap_owned_sdk_start() {
    for revoke in [true, false] {
        let h = Harness::boot().await;
        let (connector, root) = owned_platform(&h.home, "fixture", "hold-init");
        let (status, value) = h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add","payload":{"connector":connector,"enableGlobal":true,"startIfEnabled":true}}))).await;
        assert_eq!(status, 200, "{value}");
        assert_eq!(value["settingsUpdate"]["saved"], true);
        assert_eq!(value["settingsUpdate"]["runtimeApplied"], false);
        marker(&root.join("initialize.json")).await;
        let saved = std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap();
        if revoke {
            let layout = prepare_layout(&h.home).unwrap();
            let replacement = lingxi_service::auth::AuthService::bootstrap(
                &layout,
                "replacement-during-queued-mcp-start",
            )
            .unwrap();
            assert_ne!(replacement.local_token(), h.state.auth().local_token());
        } else {
            h.state
                .mcp_settings()
                .shutdown()
                .await
                .expect("退出必须确认后台连接清理");
        }
        processes_gone(&root).await;
        let state = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let state = h
                    .state
                    .mcp_settings()
                    .state_for_agent(Some("a"))
                    .await
                    .unwrap();
                if state["config"]["connectors"][0]["status"] != "connecting" {
                    break state;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(state["config"]["connectors"][0]["running"], false);
        let receipts = state["settingsResults"].as_array().unwrap();
        assert!(
            receipts.iter().any(
                |receipt| receipt["settingsUpdate"]["connectorId"] == "fixture"
                    && receipt["settingsUpdate"]["saved"] == true
                    && receipt["settingsUpdate"]["runtimeApplied"] == false
                    && receipt["settingsUpdate"]["error"]["code"]
                        == if revoke {
                            "mcp_original_authority_revoked"
                        } else {
                            "mcp_cancelled"
                        }
            ),
            "原授权撤销/退出不能被记成启动成功：{state}"
        );
        assert_eq!(
            std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap(),
            saved,
            "取消启动不能假回滚已保存配置"
        );
        std::fs::write(root.join("release"), b"late").unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while h
                .state
                .runs()
                .task_supervisor()
                .tasks()
                .iter()
                .any(|task| task.label.starts_with("mcp-start:") && task.exit.is_none())
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("后台任务必须有实际退出回执");
        h.close().await;
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn editing_a_queued_connector_cleans_old_generation_before_new_sdk_start() {
    let h = Harness::boot().await;
    let (connector, root) = owned_platform(&h.home, "fixture", "hold-init");
    let (status, value) = h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add","payload":{"connector":connector,"enableGlobal":true,"startIfEnabled":true}}))).await;
    assert_eq!(status, 200, "{value}");
    marker(&root.join("initialize.json")).await;
    let replacement = fixture_connector("fixture");
    let (status, value) = h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.update","payload":{"connectorId":"fixture","patch":replacement}}))).await;
    assert_eq!(status, 200, "{value}");
    processes_gone(&root).await;
    std::fs::write(root.join("release"), b"obsolete-generation").unwrap();
    assert_eq!(
        persisted(&h)["global"]["mcp"]["connectors"][0]["command"],
        env!("CARGO_BIN_EXE_r04_t07_fixture")
    );
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.start","payload":{"connectorId":"fixture"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    invoke_fixture(&h, "a", "queued-generation-replaced").await;
    let state = settled_state(&h).await;
    assert_eq!(state["config"]["connectors"][0]["running"], true);
    assert_eq!(
        state["config"]["connectors"][0]["command"],
        env!("CARGO_BIN_EXE_r04_t07_fixture")
    );
    h.close().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_update_must_not_cancel_an_accepted_background_sdk_start() {
    let h = Harness::boot().await;
    let (connector, root) = owned_platform(&h.home, "fixture", "hold-init");
    let (status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add","payload":{"connector":connector,"enableGlobal":true,"startIfEnabled":true}}))).await;
    assert_eq!(status, 200, "{value}");
    marker(&root.join("initialize.json")).await;
    let before = std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap();
    let (status, rejected) = h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.update","payload":{"connectorId":"fixture","patch":{"command":""}}}))).await;
    assert_eq!(status, 400, "{rejected}");
    assert_eq!(rejected["settingsUpdate"]["saved"], false);
    assert_eq!(rejected["settingsUpdate"]["runtimeApplied"], false);
    assert_eq!(
        std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap(),
        before
    );
    let (_, state) = h
        .request(reqwest::Method::GET, "/lingxi/v1/mcp/state?agentId=a", None)
        .await;
    let was_pending = state["config"]["connectors"][0]["status"] == "connecting";
    std::fs::write(root.join("release"), b"valid original request").unwrap();
    let final_state = settled_state(&h).await;
    let original_completed = final_state["config"]["connectors"][0]["running"] == true;
    if original_completed {
        let (ctx, call, prepared) = prepared(&h, "a", "rejected-patch-unchanged");
        let result = h
            .state
            .tool_gateway()
            .unwrap()
            .execute_prepared(&ctx, &call, &prepared.handle)
            .await
            .unwrap();
        assert!(matches!(
            result.outcome,
            lingxi_kernel::ports::ToolOutcome::Success { .. }
        ));
    }
    h.state.mcp_settings().shutdown().await.unwrap();
    processes_gone(&root).await;
    let home = h.home.clone();
    h.close_runtime(false).await;
    assert!(
        was_pending && original_completed,
        "拒绝的坏补丁不得取消原合法启动；home={},state={state},final={final_state}",
        home.display()
    );
    std::fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn explicit_start_revalidates_original_owner_while_real_sdk_initialize_is_waiting() {
    let h = Harness::boot().await;
    let (connector, root) = owned_platform(&h.home, "fixture", "hold-init");
    let(status,value)=h.request(reqwest::Method::POST,"/lingxi/v1/mcp/settings-action",Some(json!({"action":"mcp.connector.add","payload":{"connector":connector,"enableGlobal":true}}))).await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = {
        let start = h.request(
            reqwest::Method::POST,
            "/lingxi/v1/mcp/settings-action",
            Some(json!({"action":"mcp.connector.start","payload":{"connectorId":"fixture"}})),
        );
        tokio::pin!(start);
        let initialized = root.join("initialize.json");
        tokio::select! { _=marker(&initialized)=>{},result=&mut start=>panic!("真实初始化尚未回复前不可返回：{result:?}") }
        let layout = prepare_layout(&h.home).unwrap();
        let replacement =
            lingxi_service::auth::AuthService::bootstrap(&layout, "explicit-start-owner-replaced")
                .unwrap();
        assert_ne!(replacement.local_token(), h.state.auth().local_token());
        // 释放实际外端后才检查终态，旧实现会沿过期owner把连接登记为成功。
        std::fs::write(root.join("release"), b"after owner revocation").unwrap();
        start.await
    };
    let state = h
        .state
        .mcp_settings()
        .state_for_agent(Some("a"))
        .await
        .unwrap();
    h.state.mcp_settings().shutdown().await.unwrap();
    processes_gone(&root).await;
    let home = h.home.clone();
    h.close_runtime(false).await;
    assert_eq!(
        status,
        403,
        "原owner已撤销不能启动成功；home={},response={value},state={state}",
        home.display()
    );
    assert_eq!(
        value["settingsUpdate"]["error"]["code"],
        "mcp_original_authority_revoked"
    );
    assert_eq!(value["settingsUpdate"]["saved"], true);
    assert_eq!(value["settingsUpdate"]["runtimeApplied"], false);
    assert_eq!(state["config"]["connectors"][0]["running"], false);
    std::fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_disable_rejects_new_queued_start_while_old_sdk_cleanup_is_draining() {
    let h = Harness::boot().await;
    // 独立真实认证实例只为构造正式manager调用；不替换认证或模拟权限结论。
    let layout = prepare_layout(&h.home).unwrap();
    let auth = Arc::new(
        lingxi_service::auth::AuthService::bootstrap(&layout, "global-disable-real-manager-owner")
            .unwrap(),
    );
    let owner = auth
        .authenticate(
            Some(&format!("Bearer {}", auth.local_token())),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    let manager = h.state.mcp_settings().clone();
    let (first, first_root) = owned_platform(&h.home, "one", "hold-init");
    let value = manager
        .request(
            lingxi_service::mcp_settings::SETTINGS_TOPIC,
            &auth,
            &owner,
            None,
            "mcp.connector.add",
            &json!({"connector":first,"enableGlobal":true,"startIfEnabled":true}),
        )
        .await
        .unwrap();
    assert_eq!(value["settingsUpdate"]["saved"], true);
    marker(&first_root.join("initialize.json")).await;
    let (bound, waiting_bound) = tokio::sync::oneshot::channel();
    let bound = Arc::new(Mutex::new(Some(bound)));
    let (release, blocked) = std::sync::mpsc::channel();
    let blocked = Arc::new(Mutex::new(blocked));
    h.state
        .mcp_settings()
        .set_registration_observer(Some(Arc::new(move |_| {
            if let Some(bound) = bound.lock().unwrap().take() {
                let _ = bound.send(());
                blocked
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .expect("实际binder观察必须释放");
            }
        })));
    std::fs::write(first_root.join("release"), b"reach actual SDK binding").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), waiting_bound)
        .await
        .unwrap()
        .unwrap();
    let (disabling, waiting_disabling) = tokio::sync::oneshot::channel();
    let disabling = Arc::new(Mutex::new(Some(disabling)));
    h.state
        .mcp_settings()
        .set_management_request_observer(Some(Arc::new(move |action| {
            if action == "mcp.global.disable_pending" {
                if let Some(disabling) = disabling.lock().unwrap().take() {
                    let _ = disabling.send(());
                }
            }
        })));
    let disable_owner = owner.clone();
    let disable_input = json!({"enabled":false});
    let disabling = manager.request(
        lingxi_service::mcp_settings::SETTINGS_TOPIC,
        &auth,
        &disable_owner,
        None,
        "mcp.global.enabled",
        &disable_input,
    );
    tokio::pin!(disabling);
    tokio::select! { result=&mut disabling=>panic!("旧binder仍阻塞，停用不能已完成：{result:?}"),result=waiting_disabling=>result.unwrap() }
    let (second, second_root) = owned_platform(&h.home, "two", "hold-init");
    // 工具及管理者入口不由HTTP的媒体权限租约互斥；这里沿同一个正式request和真实本机principal。
    let added = manager
        .request(
            lingxi_service::mcp_settings::SETTINGS_TOPIC,
            &auth,
            &owner,
            None,
            "mcp.connector.add",
            &json!({"connector":second,"startIfEnabled":true}),
        )
        .await;
    let second_was_queued = added
        .as_ref()
        .is_ok_and(|value| value["settingsUpdate"]["saved"] == true);
    if second_was_queued {
        marker(&second_root.join("initialize.json")).await;
    }
    release.send(()).unwrap();
    let disabled = tokio::time::timeout(std::time::Duration::from_secs(2), &mut disabling).await;
    // 失败时也先真实回收新旧进程，避免把反例本身变成遗留任务。
    manager.begin_stop();
    let eventually = match disabled {
        Ok(result) => Some(result),
        Err(_) => Some(disabling.await),
    };
    manager.set_registration_observer(None);
    manager.set_management_request_observer(None);
    manager.shutdown().await.unwrap();
    processes_gone(&first_root).await;
    if second_root.join("pids.json").exists() {
        processes_gone(&second_root).await;
    }
    let home = h.home.clone();
    h.close_runtime(false).await;
    assert!(
        !second_was_queued,
        "停用意图已确认后不能再接受新的后台启动；home={},add={added:?},disable={eventually:?}",
        home.display()
    );
    assert!(
        eventually.unwrap().is_ok(),
        "停用应完成清理而非等待新慢握手超时"
    );
    assert!(
        !second_root.join("initialize.json").exists(),
        "新外端不得收到initialize"
    );
    std::fs::remove_dir_all(home).unwrap();
}
