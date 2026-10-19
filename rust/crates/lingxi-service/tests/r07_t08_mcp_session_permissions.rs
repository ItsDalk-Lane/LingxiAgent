//! 原672564：真实身份、会话、Run、存储和官方SDK；仅模型平台与MCP端为自有外部响应。
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_kernel::ports::StoragePort;
use lingxi_protocol::RunId;
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct External {
    next: Mutex<Option<(String, Value)>>,
}
async fn model(State(external): State<Arc<External>>, Json(body): Json<Value>) -> Response {
    let pending = external.next.lock().unwrap().take();
    let (delta, finish) = if let Some((name, arguments)) = pending {
        let actual = body["tools"]
            .as_array()
            .expect("实际模型声明必须包含MCP工具")
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .find(|candidate| candidate.ends_with(&name))
            .expect("指定MCP工具必须来自真实声明");
        (
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"mcp-session-real-call","type":"function","function":{"name":actual,"arguments":arguments.to_string()}}]}),
            "tool_calls",
        )
    } else {
        (json!({"role":"assistant","content":"已处理"}), "stop")
    };
    let first = json!({"id":"local-fixture","object":"chat.completion.chunk","model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let last = json!({"id":"local-fixture","object":"chat.completion.chunk","model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":finish}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
        .into_response()
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    base: String,
    external: Arc<External>,
    stop: tokio::sync::oneshot::Sender<()>,
    server:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
    external_task: tokio::task::JoinHandle<()>,
}
impl Harness {
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
        let external = Arc::new(External::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/v1/models", get(|| async { Json(json!({"data":[]})) }))
            .route("/v1/chat/completions", post(model))
            .with_state(external.clone());
        let external_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
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
                shutdown_timeout_ms: 3000,
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
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (tx, ready) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let server = tokio::spawn(async move {
            lingxi_service::run(
                serving,
                async {
                    let _ = rx.await;
                },
                |address| {
                    let _ = tx.send(address);
                },
                None,
            )
            .await
        });
        let base = format!("http://{}", ready.await.unwrap());
        let h = Self {
            home,
            state,
            base,
            external,
            stop,
            server,
            external_task,
        };
        let (status,value)=h.owner("PUT","/lingxi/v1/settings/config",Some(json!({"providers":{"fixture":{"base_url":endpoint,"api":"openai-completions","api_key":"synthetic-only","models":[{"id":"chat","inputs":["text"],"toolUse":{"supportsTools":true,"dialect":"openai","toolResultFormat":"message"}}]}}}))).await;
        assert_eq!(status, 200, "{value}");
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
            .model_configuration()
            .save(
                &principal,
                &[
                    lingxi_service::model_configuration::ModelSettingsChange::SetBinding {
                        operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
                        selection: Some(lingxi_service::model_configuration::ModelSelection {
                            provider: "fixture".into(),
                            model: "chat".into(),
                        }),
                    },
                ],
            )
            .await
            .unwrap();
        h
    }
    async fn request(
        &self,
        token: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .bearer_auth(token)
            .timeout(Duration::from_secs(8));
        if let Some(body) = body {
            request = request.json(&body)
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(json!({"raw":text})),
        )
    }
    async fn owner(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.request(&self.state.auth().local_token(), method, path, body)
            .await
    }
    async fn action(&self, action: &str, payload: Value) {
        let (status, value) = self
            .owner(
                "POST",
                "/lingxi/v1/mcp/settings-action",
                Some(json!({"action":action,"agentId":"a","payload":payload})),
            )
            .await;
        assert_eq!(status, 200, "{value}");
        assert_eq!(value["settingsUpdate"]["status"], "applied", "{value}");
    }
    async fn configure_mcp(&self) {
        let path = self.home.join("mcp-external.cjs");
        std::fs::write(&path,r#"const fs=require('fs'),readline=require('readline');
fs.writeFileSync(process.argv[2]+'.pid',String(process.pid));
const send=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
readline.createInterface({input:process.stdin}).on('line',line=>{const m=JSON.parse(line);
if(m.method==='initialize')send(m.id,{protocolVersion:m.params.protocolVersion,serverInfo:{name:'session-capability',version:'1'},capabilities:{tools:{}}});
if(m.method==='tools/list')send(m.id,{tools:['alpha','beta'].map(name=>({name,description:'合成外端实际记录调用',inputSchema:{type:'object',properties:{n:{type:'integer'}},required:['n']}}))});
if(m.method==='tools/call'){fs.appendFileSync(process.argv[2],JSON.stringify(m.params)+'\n');send(m.id,{content:[{type:'text',text:JSON.stringify(m.params)}]});}
});"#).unwrap();
        self.action("mcp.global.enabled", json!({"enabled":true}))
            .await;
        self.action("mcp.connector.add",json!({"startIfEnabled":true,"connector":{"id":"fixture","name":"fixture","transport":"stdio","command":"node","args":[path,self.home.join("mcp-calls.jsonl")]}})).await;
        self.ready().await;
    }
    async fn ready(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (status, value) = self
                    .owner("GET", "/lingxi/v1/mcp/state?agentId=a", None)
                    .await;
                assert_eq!(
                    status,
                    200,
                    "真实MCP状态读取失败；home={}：{value}",
                    self.home.display()
                );
                let row = value["config"]["connectors"]
                    .as_array()
                    .expect("真实连接器列表必须存在")
                    .iter()
                    .find(|row| row["id"] == "fixture")
                    .expect("已保存连接器必须可读回");
                match row["status"].as_str() {
                    Some("running") => {
                        assert_eq!(
                            row["tools"].as_array().map(Vec::len),
                            Some(2),
                            "实际SDK目录不符合fixture；home={}：{value}",
                            self.home.display()
                        );
                        return;
                    }
                    Some("connecting") => {}
                    _ => panic!(
                        "MCP不是正在连接：不靠延长等待掩盖未启动或实际SDK错误；home={}：{value}",
                        self.home.display()
                    ),
                }
                // 与实际设置页面相同，只有连接中才有界轮询；不绕开真实限流。
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        })
        .await
        .expect("MCP须在原5秒预算内返回真实目录；不延长预算或提高限流");
    }
    async fn create(&self, sid: &str) {
        let(status,value)=self.owner("POST","/lingxi/v1/sessions",Some(json!({"sessionId":sid,"agentId":"a","title":"临时MCP授权实证","permissionMode":"ask","memoryEnabled":false}))).await;
        assert_eq!(status, 201, "{value}");
    }
    async fn paired(&self, scopes: Vec<&str>) -> (String, String) {
        let (status, pair) = self
            .owner(
                "POST",
                "/lingxi/v1/devices/pairing-sessions",
                Some(json!({"requestedDevice":{"deviceKind":"cli","displayName":"合成设备"}})),
            )
            .await;
        assert_eq!(status, 200, "{pair}");
        let (status, result) = self
            .owner(
                "POST",
                &format!(
                    "/lingxi/v1/devices/pairing-sessions/{}/approve",
                    pair["pairingSessionId"].as_str().unwrap()
                ),
                Some(json!({"userCode":pair["userCode"],"scopes":scopes})),
            )
            .await;
        assert_eq!(status, 200, "{result}");
        for scope in scopes {
            assert!(
                result["credential"]["scopes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == scope),
                "正式配对范围缺失：{result}"
            );
        }
        (
            result["secret"].as_str().unwrap().into(),
            result["credential"]["credentialId"]
                .as_str()
                .unwrap()
                .into(),
        )
    }
    fn execute(
        &self,
        token: &str,
        sid: &str,
        name: &str,
        n: i32,
    ) -> tokio::task::JoinHandle<Value> {
        *self.external.next.lock().unwrap() = Some((name.into(), json!({"n":n})));
        let url = format!("{}/lingxi/v1/sessions/{sid}/execute", self.base);
        let token = token.to_owned();
        tokio::spawn(async move {
            let response = reqwest::Client::new()
                .post(url)
                .bearer_auth(token)
                .timeout(Duration::from_secs(8))
                .json(&json!({"input":"执行自有MCP外端"}))
                .send()
                .await
                .unwrap();
            let status = response.status();
            let value: Value = response.json().await.unwrap();
            assert!(status.is_success(), "{status}: {value}");
            value
        })
    }
    async fn pending(&self, token: &str, sid: &str, stage: &str) -> Value {
        // 设备认证有真实密钥派生开销；先认证一次，五秒只计真实审批登记就绪。
        // 就绪后仍走一次正式设备 HTTP，不用只读观测代替鉴权、归属和响应契约。
        let principal = self
            .state
            .auth()
            .authenticate(
                Some(&format!("Bearer {token}")),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        self.pending_authenticated(token, &principal, sid, stage)
            .await
    }
    async fn pending_authenticated(
        &self,
        token: &str,
        principal: &lingxi_service::auth::Principal,
        sid: &str,
        stage: &str,
    ) -> Value {
        let started = std::time::Instant::now();
        let outcome = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let records = self
                    .state
                    .pending_approvals_for_authenticated(principal, sid, false)
                    .unwrap();
                if let Some(record) = records.first() {
                    return record.clone();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        let record = match outcome {
            Ok(record) => record,
            Err(error) => {
                let state=self.state.storage().query_one_text("SELECT json_group_array(json_object('runId',run_id,'status',status,'reason',terminal_reason,'createdAt',created_at_unix_ms,'updatedAt',updated_at_unix_ms)) FROM runs WHERE session_id=?1",vec![sid.into()]).await.unwrap();
                let journal=self.state.storage().query_one_text("SELECT group_concat(payload_json,'\n') FROM key_events WHERE session_id=?1 AND event_type IN ('tool_call_started','tool_call_completed','run_state_changed')",vec![sid.into()]).await.unwrap();
                panic!(
                    "真实批准登记超时，不延长预算：stage={stage} session={sid} home={} elapsedMs={} error={error:?} actualRuns={state:?} actualEvents={journal:?}",
                    self.home.display(),
                    started.elapsed().as_millis()
                );
            }
        };
        let ready_ms = started.elapsed().as_millis();
        let call_started = std::time::Instant::now();
        let (status, value) = self
            .request(
                token,
                "GET",
                &format!("/lingxi/v1/sessions/{sid}/approvals"),
                None,
            )
            .await;
        eprintln!(
            "MCP批准观测：stage={stage} session={sid} readyMs={ready_ms} status={status} requestMs={} approvalId={}",
            call_started.elapsed().as_millis(),
            record.approval_id
        );
        assert_eq!(
            status,
            200,
            "stage={stage} session={sid} home={}：{value}",
            self.home.display()
        );
        let row = value["approvals"].as_array().unwrap().iter().find(|row| row["approvalId"] == record.approval_id).unwrap_or_else(|| panic!("正式设备HTTP未返回真实已登记审批：stage={stage} expected={record:?} actual={value}"));
        assert_eq!(row["runId"], record.run_id);
        assert_eq!(row["target"], record.target);
        row.clone()
    }
    async fn grant(&self, token: &str, sid: &str, name: &str) -> (u16, Value) {
        self.request(token,"POST","/lingxi/v1/mcp/session-permissions",Some(json!({"sessionId":sid,"targetId":format!("tool:mcp:fixture:{name}"),"capability":format!("mcp.{name}.invoke")}))).await
    }
    async fn answer(&self, token: &str, sid: &str, pending: &Value, action: &str) {
        let (status, value) = self
            .request(
                token,
                "POST",
                &format!(
                    "/lingxi/v1/sessions/{sid}/approvals/{}",
                    pending["approvalId"].as_str().unwrap()
                ),
                Some(json!({"action":action})),
            )
            .await;
        assert_eq!(status, 200, "{value}");
        assert_eq!(value["ok"], true, "{value}");
    }
    fn calls(&self) -> Vec<Value> {
        std::fs::read_to_string(self.home.join("mcp-calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    async fn assert_dispatch(&self, response: &Value, dispatched: bool) {
        let journal = self
            .state
            .storage()
            .load_invocation_journal(&RunId::new(response["runId"].as_str().unwrap()))
            .await
            .unwrap();
        assert_eq!(journal.len(), 1, "{journal:?}");
        assert_eq!(
            journal[0].receipt.as_ref().unwrap().dispatched,
            dispatched,
            "{journal:?}"
        );
    }
    async fn close(self) {
        let Self {
            state,
            home,
            stop,
            server,
            external_task,
            ..
        } = self;
        let _ = stop.send(());
        let result = tokio::time::timeout(Duration::from_secs(8), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!result.drain_timed_out);
        #[cfg(unix)]
        {
            let pid: i32 = std::fs::read_to_string(home.join("mcp-calls.jsonl.pid"))
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "自有MCP子进程必须实际退出"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
        state.storage().close().await.unwrap();
        external_task.abort();
        let _ = external_task.await;
    }
}
fn home() -> PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-r07-mcp-session-permission-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_scope_grants_same_session_exact_capability_across_arguments_without_persisting() {
    let h = Harness::boot(home()).await;
    h.configure_mcp().await;
    h.create("mcp-session-a").await;
    h.create("mcp-session-b").await;
    let (token, _) = h.paired(vec!["chat", "sessions.write"]).await;
    let config = h.home.join("plugin-data/mcp/config.json");
    let before = std::fs::read(&config).unwrap();
    let first = h.execute(&token, "mcp-session-a", "alpha", 1);
    let pending = h.pending(&token, "mcp-session-a", "first-alpha").await;
    assert_eq!(pending["capability"], "mcp.alpha.invoke");
    let (status, grant) = h.grant(&token, "mcp-session-a", "alpha").await;
    assert_eq!(status, 200, "{grant}");
    assert_eq!(grant["persisted"], false);
    h.answer(&token, "mcp-session-a", &pending, "confirmed")
        .await;
    let first = first.await.unwrap();
    h.assert_dispatch(&first, true).await;
    assert_eq!(
        h.grant(&token, "mcp-session-a", "alpha").await.0,
        200,
        "重复请求保持同一会话授权"
    );
    let second = tokio::time::timeout(
        Duration::from_secs(5),
        h.execute(&token, "mcp-session-a", "alpha", 2),
    )
    .await
    .expect("同能力不同参数不应再等待批准")
    .unwrap();
    h.assert_dispatch(&second, true).await;
    assert_eq!(h.calls().len(), 2);
    assert_eq!(h.calls()[1]["arguments"]["n"], 2);
    let other = h.execute(&token, "mcp-session-b", "alpha", 3);
    let pending = h
        .pending(&token, "mcp-session-b", "other-session-alpha")
        .await;
    h.answer(&token, "mcp-session-b", &pending, "rejected")
        .await;
    h.assert_dispatch(&other.await.unwrap(), false).await;
    let other = h.execute(&token, "mcp-session-a", "beta", 4);
    let pending = h
        .pending(&token, "mcp-session-a", "other-capability-beta")
        .await;
    h.answer(&token, "mcp-session-a", &pending, "rejected")
        .await;
    h.assert_dispatch(&other.await.unwrap(), false).await;
    // 后台用户受理沿真实生产入口，但不能消费仅前台冻结的会话授权。
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {token}")),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    *h.external.next.lock().unwrap() = Some(("alpha".into(), json!({"n":9})));
    let background = h
        .state
        .sessions()
        .execute_background_for(
            h.state.storage(),
            h.state.events(),
            h.state.runs(),
            h.state.background(),
            &principal,
            "mcp-session-a",
            &lingxi_service::sessions::ExecuteSubmission::plain("后台不得继承临时能力"),
            h.state.clock().now_unix_ms(),
        )
        .await
        .unwrap();
    // 不强迫后台进入人工等待；确定拒绝的终态同样证明没有借用前台授权。
    let background_phase = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = h
                .state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id=?1",
                    vec![background.run_id.clone()],
                )
                .await
                .unwrap()
                .unwrap();
            if status == "waiting_approval"
                || matches!(status.as_str(), "completed" | "failed" | "cancelled")
            {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    eprintln!(
        "MCP显式后台未继承授权的实际处理：run={} status={background_phase}",
        background.run_id
    );
    if background_phase == "waiting_approval" {
        let pending = h
            .pending(&token, "mcp-session-a", "explicit-background-alpha")
            .await;
        h.answer(&token, "mcp-session-a", &pending, "rejected")
            .await;
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = h
                .state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id=?1",
                    vec![background.run_id.clone()],
                )
                .await
                .unwrap();
            if status
                .as_deref()
                .is_some_and(|value| matches!(value, "completed" | "failed" | "cancelled"))
            {
                break;
            }
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    h.assert_dispatch(&json!({"runId":background.run_id}), false)
        .await;
    assert_eq!(h.calls().len(), 2);
    assert_eq!(std::fs::read(config).unwrap(), before);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_scope_foreign_session_and_damaged_payload_never_grant_and_connector_restart_invalidates(
) {
    let h = Harness::boot(home()).await;
    h.configure_mcp().await;
    h.create("mcp-scope-check").await;
    let (chat, _) = h.paired(vec!["chat"]).await;
    let (token, _) = h.paired(vec!["chat", "sessions.write"]).await;
    assert_eq!(h.grant(&chat, "mcp-scope-check", "alpha").await.0, 403);
    for payload in [
        json!({"sessionId":"mcp-scope-check","targetId":"tool:mcp:fixture:alpha","capability":null}),
        json!({"sessionId":"mcp-scope-check","targetId":"tool:mcp:fixture:alpha","capability":"mcp.alpha.invoke","principal":{"kind":"local_user"}}),
        json!({"sessionId":" mcp-scope-check","targetId":"tool:mcp:fixture:alpha","capability":"mcp.alpha.invoke"}),
    ] {
        assert_eq!(
            h.request(
                &token,
                "POST",
                "/lingxi/v1/mcp/session-permissions",
                Some(payload)
            )
            .await
            .0,
            400
        );
    }
    assert_eq!(h.grant(&token, "missing-session", "alpha").await.0, 404);
    assert_eq!(h.grant(&token, "mcp-scope-check", "wrong").await.0, 403);
    // 另一真实用户的设备不能为本机会话授权。
    let foreign = h
        .state
        .auth()
        .issue_device_credential("another-synthetic-user", &["chat", "sessions.write"], None)
        .unwrap();
    assert_eq!(
        h.grant(&foreign.secret, "mcp-scope-check", "alpha").await.0,
        403
    );
    assert_eq!(h.grant(&token, "mcp-scope-check", "alpha").await.0, 200);
    let (status, value) = h
        .owner(
            "POST",
            "/lingxi/v1/sessions/archive",
            Some(json!({"sessionId":"mcp-scope-check"})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(h.grant(&token, "mcp-scope-check", "alpha").await.0, 409);
    let (status, value) = h
        .owner(
            "POST",
            "/lingxi/v1/sessions/restore",
            Some(json!({"sessionId":"mcp-scope-check"})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let restored = h.execute(&token, "mcp-scope-check", "alpha", 8);
    let pending = h
        .pending(&token, "mcp-scope-check", "restored-session-alpha")
        .await;
    h.answer(&token, "mcp-scope-check", &pending, "rejected")
        .await;
    h.assert_dispatch(&restored.await.unwrap(), false).await;
    assert_eq!(h.grant(&token, "mcp-scope-check", "alpha").await.0, 200);
    h.action("mcp.connector.stop", json!({"connectorId":"fixture"}))
        .await;
    h.action("mcp.connector.start", json!({"connectorId":"fixture"}))
        .await;
    h.ready().await;
    let after = h.execute(&token, "mcp-scope-check", "alpha", 5);
    let pending = h
        .pending(&token, "mcp-scope-check", "restarted-connector-alpha")
        .await;
    h.answer(&token, "mcp-scope-check", &pending, "rejected")
        .await;
    h.assert_dispatch(&after.await.unwrap(), false).await;
    assert!(h.calls().is_empty());
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn credential_revocation_and_same_home_real_restart_do_not_restore_temporary_grants() {
    let path = home();
    let h = Harness::boot(path.clone()).await;
    h.configure_mcp().await;
    h.create("mcp-restart-check").await;
    let (token, credential) = h.paired(vec!["chat", "sessions.write"]).await;
    assert_eq!(h.grant(&token, "mcp-restart-check", "alpha").await.0, 200);
    let (status, value) = h
        .owner(
            "POST",
            &format!("/lingxi/v1/devices/credentials/{credential}/revoke"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(h.grant(&token, "mcp-restart-check", "alpha").await.0, 401);
    // 新凭证同一用户不能继承旧凭证授予的会话能力。
    let (fresh, _) = h.paired(vec!["chat", "sessions.write"]).await;
    let run = h.execute(&fresh, "mcp-restart-check", "alpha", 6);
    let pending = h
        .pending(
            &fresh,
            "mcp-restart-check",
            "fresh-credential-before-restart",
        )
        .await;
    h.answer(&fresh, "mcp-restart-check", &pending, "rejected")
        .await;
    h.assert_dispatch(&run.await.unwrap(), false).await;
    assert_eq!(h.grant(&fresh, "mcp-restart-check", "alpha").await.0, 200);
    let before = std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap();
    h.close().await;
    let h = Harness::boot(path).await;
    h.ready().await;
    assert_eq!(
        std::fs::read(h.home.join("plugin-data/mcp/config.json")).unwrap(),
        before
    );
    let run = h.execute(&fresh, "mcp-restart-check", "alpha", 7);
    let pending = h
        .pending(&fresh, "mcp-restart-check", "same-home-after-restart")
        .await;
    h.answer(&fresh, "mcp-restart-check", &pending, "rejected")
        .await;
    h.assert_dispatch(&run.await.unwrap(), false).await;
    assert!(h.calls().is_empty());
    h.close().await;
}

// 与现役桌面/Web/手机一致，走真实鉴权的WS prompt，而不是HTTP execute替代入口。
async fn ws_send(stream: &mut tokio::net::TcpStream, value: Value) {
    use tokio::io::AsyncWriteExt;
    let bytes = serde_json::to_vec(&value).unwrap();
    let mask = [1u8, 2, 3, 4];
    let mut frame = vec![0x81];
    if bytes.len() < 126 {
        frame.push(0x80 | bytes.len() as u8);
    } else if bytes.len() <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        bytes
            .iter()
            .enumerate()
            .map(|(i, value)| value ^ mask[i % 4]),
    );
    stream.write_all(&frame).await.unwrap();
}
async fn ws_json(stream: &mut tokio::net::TcpStream) -> Value {
    match tokio::time::timeout(
        Duration::from_secs(8),
        lingxi_service::ws::read_ws_frame(stream),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap()
    {
        lingxi_service::ws::WsFrame::Text(bytes) => serde_json::from_slice(&bytes).unwrap(),
        other => panic!("未预期的真实WS帧：{other:?}"),
    }
}
async fn ws_connect(h: &Harness, token: &str) -> tokio::net::TcpStream {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let address = h.base.strip_prefix("http://").unwrap();
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET /lingxi/v1/ws HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {token}\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
        assert!(head.len() < 32_768);
    }
    assert!(
        head.starts_with(b"HTTP/1.1 101"),
        "必须通过真实设备身份升级：{}",
        String::from_utf8_lossy(&head)
    );
    ws_send(&mut stream,json!({"protocol":"lingxi.wire","clientKind":"test","clientVersion":"0","protocolMin":1,"protocolMax":1})).await;
    assert_eq!(ws_json(&mut stream).await["protocol"], "lingxi.wire");
    stream
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_ws_prompt_reuses_session_capability_for_second_different_arguments_without_approval(
) {
    let h = Harness::boot(home()).await;
    h.configure_mcp().await;
    h.create("mcp-ws-session").await;
    let (token, _) = h.paired(vec!["chat", "sessions.write"]).await;
    let mut ws = ws_connect(&h, &token).await;
    *h.external.next.lock().unwrap() = Some(("alpha".into(), json!({"n":11})));
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"mcp-ws-session","agentId":"a","text":"第一次WS实际调用","clientMessageId":"session-grant-ws-first"})).await;
    let pending = h.pending(&token, "mcp-ws-session", "ws-first-alpha").await;
    let (status, value) = h.grant(&token, "mcp-ws-session", "alpha").await;
    assert_eq!(status, 200, "{value}");
    h.answer(&token, "mcp-ws-session", &pending, "confirmed")
        .await;
    let first = tokio::time::timeout(Duration::from_secs(5), async {
        let mut run = None;
        loop {
            let frame = ws_json(&mut ws).await;
            if frame["type"] == "assistant_run_end" {
                run = Some(frame["runId"].as_str().unwrap().to_owned());
            }
            if frame["type"] == "status" && frame["isStreaming"] == false {
                if let Some(run) = run {
                    return run;
                }
            }
        }
    })
    .await
    .unwrap();
    h.assert_dispatch(&json!({"runId":first}), true).await;
    assert_eq!(h.calls().len(), 1);
    // select 每收到一帧会重建分支；同步设备认证必须只在发送前准备一次，不能阻塞读帧。
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {token}")),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    *h.external.next.lock().unwrap() = Some(("alpha".into(), json!({"n":12})));
    ws_send(&mut ws,json!({"type":"prompt","sessionId":"mcp-ws-session","agentId":"a","text":"第二次WS同能力不同参数","clientMessageId":"session-grant-ws-second"})).await;
    // 单独读真实socket，避免等待HTTP观测时丢弃半读的WebSocket帧。
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Value>(256);
    let reader = tokio::spawn(async move {
        loop {
            let frame = ws_json(&mut ws).await;
            if tx.send(frame).await.is_err() {
                return;
            }
        }
    });
    let mut repeated_pending = None;
    let second=tokio::time::timeout(Duration::from_secs(6),async {
        loop {
            tokio::select! {
                frame=rx.recv()=>{let frame=frame.expect("真实WS读取器意外退出");eprintln!("WS第二轮真实帧：type={} runId={}",frame["type"],frame["runId"]);assert_ne!(frame["type"],"input_rejected","{frame}");if frame["type"]=="assistant_run_end"{return frame["runId"].as_str().unwrap().to_owned()}},
                pending=h.pending_authenticated(&token,&principal,"mcp-ws-session","ws-second-alpha"), if repeated_pending.is_none()=>{
                    eprintln!("WS会话能力未复用真实现场：session=mcp-ws-session approval={} target={}",pending["approvalId"],pending["target"]);
                    h.answer(&token,"mcp-ws-session",&pending,"rejected").await;
                    repeated_pending=Some(pending);
                }
            }
        }
    }).await.unwrap();
    reader.abort();
    let _ = reader.await;
    let calls = h.calls();
    h.assert_dispatch(&json!({"runId":second}), repeated_pending.is_none())
        .await;
    h.close().await;
    assert!(
        repeated_pending.is_none(),
        "同一真实WS会话已授权MCP capability，但第二次不同参数仍进入批准等待；原始pending={repeated_pending:?}"
    );
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1]["arguments"]["n"], 12);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn in_flight_grant_cannot_land_after_real_archive_restore_completed() {
    let h = Harness::boot(home()).await;
    h.configure_mcp().await;
    h.create("mcp-grant-lifecycle-race").await;
    let (token, _) = h.paired(vec!["chat", "sessions.write"]).await;
    let (read, observed) = tokio::sync::oneshot::channel();
    let read = Arc::new(Mutex::new(Some(read)));
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let control = release.clone();
    h.state
        .tool_gateway()
        .unwrap()
        .set_mcp_session_grant_read_observer(Some(Arc::new(move |session| {
            if session != "mcp-grant-lifecycle-race" {
                return;
            }
            if let Some(read) = read.lock().unwrap().take() {
                let _ = read.send(());
            }
            let (lock, ready) = &*control;
            let (allowed, timeout) = ready
                .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(5), |allowed| {
                    !*allowed
                })
                .unwrap();
            assert!(
                !timeout.timed_out() && *allowed,
                "测试必须真实完成归档恢复并明确放行；观察器超时不能当交错命中"
            );
        })));
    let url = format!("{}/lingxi/v1/mcp/session-permissions", h.base);
    let grant_token = token.clone();
    let request = tokio::spawn(async move {
        let response=reqwest::Client::new().post(url).bearer_auth(grant_token).timeout(Duration::from_secs(8)).json(&json!({"sessionId":"mcp-grant-lifecycle-race","targetId":"tool:mcp:fixture:alpha","capability":"mcp.alpha.invoke"})).send().await.unwrap();
        let status = response.status().as_u16();
        let value = response.json::<Value>().await.unwrap();
        (status, value)
    });
    tokio::time::timeout(Duration::from_secs(5), observed)
        .await
        .expect("真实授权处理器必须读到active行")
        .unwrap();
    let (status, archived) = h
        .owner(
            "POST",
            "/lingxi/v1/sessions/archive",
            Some(json!({"sessionId":"mcp-grant-lifecycle-race"})),
        )
        .await;
    assert_eq!(status, 200, "{archived}");
    let (status, restored) = h
        .owner(
            "POST",
            "/lingxi/v1/sessions/restore",
            Some(json!({"sessionId":"mcp-grant-lifecycle-race"})),
        )
        .await;
    assert_eq!(status, 200, "{restored}");
    {
        let (lock, ready) = &*release;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }
    let (status, value) = request.await.unwrap();
    h.state
        .tool_gateway()
        .unwrap()
        .set_mcp_session_grant_read_observer(None);
    eprintln!(
        "实际grant读后归档恢复交错：HTTP={status} response={value} home={}",
        h.home.display()
    );
    let calls = h.calls();
    h.close().await;
    assert_eq!(
        status, 409,
        "归档恢复已完成，先前读到active的旧在途授权不能迟到插入：{value}"
    );
    assert!(calls.is_empty());
}
