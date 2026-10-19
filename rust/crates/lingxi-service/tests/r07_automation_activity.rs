//! 活动接口走真实HTTP、运行与历史；模型回复仅用确定性替身。
use lingxi_adapters::storage::automation as store;
use lingxi_kernel::{
    model_exchange::ModelTurnInput,
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
use lingxi_service::{
    approval_service::ApprovalService,
    inject::{ManualClock, ServiceClock},
    prepare_layout,
    toolgateway::ToolInvocationGateway,
    HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[derive(Default)]
struct Provider {
    parked: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.fixture".into(),
            model: "activity".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _sink: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(async move {
            if self.parked.load(Ordering::Acquire) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: "真实活动的最终回复".into(),
                        }],
                        model_call_id: Some(call.clone()),
                    },
                },
            )
        })
    }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    clock: Arc<ManualClock>,
    provider: Arc<Provider>,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn start(existing: Option<PathBuf>) -> Self {
        let root = PathBuf::from(std::env::var("LINGXI_HOME").expect("隔离home"));
        let home = existing.unwrap_or_else(|| {
            root.join(format!(
                "r07-activity-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::create_dir_all(home.join("workspace")).unwrap();
        let home = home.canonicalize().unwrap();
        let workspace = home.join("workspace");
        std::fs::write(home.join("agents/lingxi/config.yaml"),format!("locale: zh-CN\nagent:\n  name: 活动助手\nmemory:\n  enabled: false\ndesk:\n  heartbeat_enabled: true\n  home_folder: {}\n",serde_json::to_string(&workspace).unwrap())).unwrap();
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(ManualClock::new(1_800_000_000_000));
        let provider = Arc::new(Provider::default());
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ApprovalService::new(clock.clone())),
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Env,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 5000,
            },
            &layout,
            ServiceDeps {
                clock: clock.clone(),
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway),
                workspace_root: Some(workspace.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        if lingxi_adapters::storage::session_tree::get_session_row(
            state.storage(),
            "activity-source",
        )
        .await
        .unwrap()
        .is_none()
        {
            state
                .session_admin()
                .create_session(
                    "activity-source",
                    "lingxi",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "活动来源",
                    Some("operate"),
                    &[workspace.to_string_lossy().into_owned()],
                    true,
                    clock.now_unix_ms(),
                )
                .await
                .unwrap();
        }
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = lingxi_service::build_router(state.clone());
        let (stop, done) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = done.await;
            })
            .await
            .unwrap();
        });
        Self {
            home,
            state,
            clock,
            provider,
            url: format!("http://{addr}"),
            token,
            stop,
            server,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method.parse().unwrap(), format!("{}{path}", self.url))
            .bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let bytes = response.bytes().await.unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!(
                "{method} {path} HTTP {status}: {error}; {}",
                String::from_utf8_lossy(&bytes)
            )
        });
        (status, value)
    }
    async fn add_due(&self) -> Value {
        let (status,body)=self.request("POST","/lingxi/v1/desk/cron",Some(json!({"action":"add","actorAgentId":"lingxi","type":"every","schedule":60000,"prompt":"这是后台任务材料，不是用户消息","label":"活动测试","executionContext":{"sourceSessionId":"activity-source"}}))).await;
        assert_eq!(status, 200, "{body}");
        let job = body["job"].clone();
        self.clock.advance(60_000);
        job
    }
    async fn execute(&self) -> String {
        let job = self.add_due().await;
        tokio::time::timeout(
            Duration::from_secs(10),
            self.state.automation().check_due(&self.state),
        )
        .await
        .unwrap()
        .unwrap();
        let history = store::list_trigger_history(self.state.storage(), job["id"].as_str(), 10)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        history[0].trigger_id.clone()
    }
    async fn counts(&self) -> Value {
        let value=self.state.storage().query_one_text("SELECT json_object('runs',(SELECT COUNT(*) FROM runs),'messages',(SELECT COUNT(*) FROM messages),'user',(SELECT COUNT(*) FROM messages WHERE role='user'),'lineage',(SELECT group_concat(run_id||':'||origin||':'||cause_id,';') FROM run_lineage))",vec![]).await.unwrap().unwrap();
        serde_json::from_str(&value).unwrap()
    }
    async fn list(&self) -> Vec<Value> {
        let (status, body) = self
            .request("GET", "/lingxi/v1/desk/activities", None)
            .await;
        assert_eq!(status, 200, "{body}");
        body["activities"].as_array().unwrap().clone()
    }
    async fn close(self) -> PathBuf {
        self.provider.parked.store(false, Ordering::Release);
        self.provider.release.notify_waiters();
        self.state.automation().stop();
        self.state.automation().wait_stopped().await.unwrap();
        let _ = self.stop.send(());
        self.server.await.unwrap();
        self.state.storage().close().await.unwrap();
        self.home
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_activity_promotes_the_same_real_session_without_new_messages_or_runs() {
    let h = Harness::start(None).await;
    let id = h.execute().await;
    let list = h.list().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], id);
    assert_eq!(list[0]["type"], "cron");
    assert_eq!(list[0]["status"], "done");
    assert_eq!(list[0]["summary"], "真实活动的最终回复");
    let session = list[0]["sessionId"].as_str().unwrap().to_owned();
    let (status, detail) = h
        .request(
            "GET",
            &format!("/lingxi/v1/desk/activities/{id}/session"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(
        detail["messages"],
        json!([{"role":"assistant","content":"真实活动的最终回复"}])
    );
    assert_eq!(detail["activity"]["agentName"], "活动助手");
    let (status, sessions) = h.request("GET", "/lingxi/v1/sessions", None).await;
    assert_eq!(status, 200, "{sessions}");
    assert!(
        !sessions["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["sessionId"] == session),
        "未升格的后台会话不能混入普通列表"
    );
    let before = h.counts().await;
    assert_eq!(before["user"], 0, "后台提示不能伪造用户消息");
    let (status, promoted) = h
        .request(
            "POST",
            &format!("/lingxi/v1/desk/activities/{id}/promote"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{promoted}");
    assert_eq!(promoted["sessionId"], session);
    assert_eq!(promoted["sessionPath"], format!("native:{session}"));
    assert_eq!(h.counts().await, before);
    assert!(h.list().await.is_empty());
    let (status, repeated) = h
        .request(
            "POST",
            &format!("/lingxi/v1/desk/activities/{id}/promote"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{repeated}");
    assert_eq!(repeated["sessionPath"], promoted["sessionPath"]);
    let (status, sessions) = h.request("GET", "/lingxi/v1/sessions", None).await;
    assert_eq!(status, 200, "{sessions}");
    assert!(sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["sessionId"] == session));
    let home = h.close().await;
    let h = Harness::start(Some(home)).await;
    assert!(h.list().await.is_empty());
    assert_eq!(h.counts().await, before);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn running_activity_uses_actual_state_and_cannot_be_promoted_while_busy() {
    let h = Harness::start(None).await;
    h.provider.parked.store(true, Ordering::Release);
    h.add_due().await;
    let state = h.state.clone();
    let check = tokio::spawn(async move { state.automation().check_due(&state).await });
    tokio::time::timeout(Duration::from_secs(10), h.provider.entered.notified())
        .await
        .unwrap();
    let list = h.list().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["status"], "running");
    assert!(list[0]["finishedAt"].is_null());
    let id = list[0]["id"].as_str().unwrap();
    let (status, body) = h
        .request(
            "POST",
            &format!("/lingxi/v1/desk/activities/{id}/promote"),
            None,
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "session_busy");
    h.provider.parked.store(false, Ordering::Release);
    h.provider.release.notify_one();
    tokio::time::timeout(Duration::from_secs(10), check)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(h.list().await[0]["status"], "done");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_cancelled_run_keeps_cancelled_activity_status() {
    let h = Harness::start(None).await;
    h.provider.parked.store(true, Ordering::Release);
    h.add_due().await;
    let state = h.state.clone();
    let check = tokio::spawn(async move { state.automation().check_due(&state).await });
    tokio::time::timeout(Duration::from_secs(10), h.provider.entered.notified())
        .await
        .unwrap();
    let list = h.list().await;
    let run = list[0]["runId"].as_str().unwrap();
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", h.token)),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    h.state
        .sessions()
        .cancel_run_for(h.state.storage().as_ref(), h.state.runs(), &principal, run)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), check)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let list = h.list().await;
    assert_eq!(list[0]["status"], "cancelled");
    assert!(list[0]["finishedAt"].is_number());
    assert_eq!(h.counts().await["user"], 0);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restored_actual_inflight_snapshot_reports_interrupted_as_unknown() {
    let h = Harness::start(None).await;
    h.provider.parked.store(true, Ordering::Release);
    h.add_due().await;
    let state = h.state.clone();
    let check = tokio::spawn(async move { state.automation().check_due(&state).await });
    tokio::time::timeout(Duration::from_secs(10), h.provider.entered.notified())
        .await
        .unwrap();
    let before = h.list().await;
    let id = before[0]["id"].as_str().unwrap().to_owned();
    let run = before[0]["runId"].as_str().unwrap().to_owned();
    let backup = h.home.join("inflight-backup");
    let data = h.state.storage().db_path().parent().unwrap().to_path_buf();
    // 使用正式在线备份保存真实运行中的数据库，重启复原该崩溃窗口，不手造Run或终态。
    h.state
        .storage()
        .backup_to(&backup, "runs", Default::default())
        .await
        .unwrap();
    h.state.automation().stop();
    tokio::time::timeout(Duration::from_secs(10), check)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let home = h.close().await;
    std::fs::rename(&data, home.join("cleanly-closed-data")).unwrap();
    lingxi_adapters::storage::backup::restore_backup(&backup, "runs", &data).unwrap();
    let h = Harness::start(Some(home)).await;
    let list = h.list().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], id);
    assert_eq!(list[0]["runId"], run);
    assert_eq!(list[0]["status"], "unknown");
    assert_eq!(list[0]["error"], "interrupted");
    let actual = h
        .state
        .storage()
        .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual, "interrupted_needs_attention");
    let (status, detail) = h
        .request(
            "GET",
            &format!("/lingxi/v1/desk/activities/{id}/session"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(detail["messages"], json!([]));
    assert_eq!(h.counts().await["user"], 0);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn withdrawn_source_folders_hide_details_and_block_promotion() {
    let h = Harness::start(None).await;
    let id = h.execute().await;
    let (status, body) = h
        .request(
            "PATCH",
            "/lingxi/v1/sessions/activity-source/authorized-folders",
            Some(json!({"action":"set","folders":[]})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let before = h.counts().await;
    let (status, body) = h
        .request(
            "GET",
            &format!("/lingxi/v1/desk/activities/{id}/session"),
            None,
        )
        .await;
    assert_eq!(status, 403, "{body}");
    let (status, body) = h
        .request(
            "POST",
            &format!("/lingxi/v1/desk/activities/{id}/promote"),
            None,
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert!(h.list().await.is_empty());
    assert_eq!(h.counts().await, before);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn heartbeat_and_its_internal_scope_project_without_fake_user_input() {
    let h = Harness::start(None).await;
    let (status, body) = h
        .request("POST", "/lingxi/v1/desk/heartbeat?agentId=lingxi", None)
        .await;
    assert_eq!(status, 200, "{body}");
    tokio::time::timeout(Duration::from_secs(10), async {
        while h.state.automation().patrol().status("lingxi")["running"] != false {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let list = h.list().await;
    assert_eq!(
        list.len(),
        1,
        "{}",
        h.state.automation().patrol().status("lingxi")
    );
    assert_eq!(list[0]["type"], "heartbeat");
    assert_eq!(list[0]["status"], "done");
    let id = list[0]["id"].as_str().unwrap();
    let (status, body) = h
        .request(
            "GET",
            &format!("/lingxi/v1/desk/activities/{id}/session"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["messages"],
        json!([{"role":"assistant","content":"真实活动的最终回复"}])
    );
    assert_eq!(h.counts().await["user"], 0);
    let (_, sessions) = h.request("GET", "/lingxi/v1/sessions", None).await;
    assert!(!sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["title"] == "心跳资源范围"));
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_trigger_without_real_run_is_never_displayed_as_finished_chat() {
    let h = Harness::start(None).await;
    let job = h.add_due().await;
    let job = store::get_job(h.state.storage(), job["id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    let id = lingxi_service::automation::trigger_id(
        &job.studio_id,
        &job.id,
        job.config_revision,
        h.clock.now_unix_ms() as i64,
    );
    store::claim_trigger(
        h.state.storage(),
        &store::TriggerInput {
            trigger_id: id.clone(),
            job_id: job.id,
            config_revision: job.config_revision,
            due_at: h.clock.now_unix_ms() as i64,
        },
        h.clock.now_unix_ms() as i64,
    )
    .await
    .unwrap();
    store::settle_trigger(
        h.state.storage(),
        &id,
        store::TriggerStatus::Unknown,
        None,
        Some("fixture uncertain receipt".into()),
        None,
        h.clock.now_unix_ms() as i64,
    )
    .await
    .unwrap();
    let before = h.counts().await;
    let list = h.list().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["status"], "unknown");
    assert!(list[0]["finishedAt"].is_null());
    assert!(list[0]["sessionFile"].is_null());
    for suffix in ["session", "promote"] {
        let method = if suffix == "session" { "GET" } else { "POST" };
        let (status, body) = h
            .request(
                method,
                &format!("/lingxi/v1/desk/activities/{id}/{suffix}"),
                None,
            )
            .await;
        assert_eq!(status, 404, "{body}");
    }
    assert_eq!(h.counts().await, before);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overdue_activity_list_cancels_real_run_before_publishing_timeout_and_keeps_receipt_after_restart(
) {
    let h = Harness::start(None).await;
    h.provider.parked.store(true, Ordering::Release);
    h.add_due().await;
    let state = h.state.clone();
    let check = tokio::spawn(async move { state.automation().check_due(&state).await });
    tokio::time::timeout(Duration::from_secs(10), h.provider.entered.notified())
        .await
        .unwrap();
    let before = h.list().await;
    let id = before[0]["id"].as_str().unwrap().to_owned();
    let run = before[0]["runId"].as_str().unwrap().to_owned();
    h.clock.advance(20 * 60_000);
    let list = h.list().await;
    assert_eq!(list[0]["status"], "error", "{list:?}");
    assert_eq!(list[0]["error"], "timeout");
    assert!(list[0]["summary"].as_str().unwrap().contains("已超时"));
    assert!(list[0]["finishedAt"].is_number());
    tokio::time::timeout(Duration::from_secs(10), check)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let actual = h
        .state
        .storage()
        .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual, "cancelled", "页面超时必须已经进入真实取消终态");
    assert_eq!(
        store::get_trigger(h.state.storage(), &id)
            .await
            .unwrap()
            .unwrap()
            .error
            .as_deref(),
        Some("timeout")
    );
    let events = h.state.app_events().since(0);
    assert!(events["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["type"] == "activity_update"
            && event["payload"]["activity"]["error"] == "timeout"));
    let (_, logs) = h.request("GET", "/lingxi/v1/desk/logs", None).await;
    assert!(logs["logs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["text"].as_str().unwrap().contains("timeout")));
    let count = logs["logs"].as_array().unwrap().len();
    assert_eq!(h.list().await[0]["error"], "timeout");
    let (_, logs) = h.request("GET", "/lingxi/v1/desk/logs", None).await;
    assert_eq!(logs["logs"].as_array().unwrap().len(), count);
    let home = h.close().await;
    let h = Harness::start(Some(home)).await;
    let list = h.list().await;
    assert_eq!(list[0]["error"], "timeout");
    let (_, logs) = h.request("GET", "/lingxi/v1/desk/logs", None).await;
    assert_eq!(logs["logs"], json!([]), "开发日志按现役语义不跨重启保留");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn desk_logs_return_real_bounded_records_and_reject_device_or_missing_credentials() {
    let h = Harness::start(None).await;
    for n in 0..205 {
        h.state.desk_logs().record(&format!("诊断{n}"), "info", n);
    }
    let (status, body) = h.request("GET", "/lingxi/v1/desk/logs", None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["logs"].as_array().unwrap().len(), 200);
    assert_eq!(body["logs"][0]["ts"], 5);
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/desk/logs", h.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 401);
    let credential = h
        .state
        .auth()
        .issue_device_credential("user_local", &["files.read"], None)
        .unwrap()
        .secret;
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/desk/logs", h.url))
        .bearer_auth(credential)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 403);
    h.close().await;
}
