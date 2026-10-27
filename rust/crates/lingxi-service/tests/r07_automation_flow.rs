//! 外部模型响应使用确定性替身；入口、权限、运行、数据库和重启恢复均为真实实现。
use lingxi_adapters::storage::automation as store;
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
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
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

#[derive(Default)]
struct Provider {
    calls: AtomicUsize,
    results: Mutex<Vec<Value>>,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.fixture".into(),
            model: "deterministic".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _sink: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if input.turn == 1 {
                if let Ok(command) = serde_json::from_str::<Value>(
                    input
                        .submission
                        .rsplit("\n\n")
                        .next()
                        .unwrap_or(&input.submission),
                ) {
                    if let Some(tool) = command["tool"].as_str() {
                        return ProviderTurnResult::of_ctx(
                            ctx,
                            ProviderTurn::ToolRequests {
                                requests: vec![ToolRequest::from_effective_arguments(
                                    format!("tool:first-party:{tool}"),
                                    command["args"].clone(),
                                    &SchemaBudget::default(),
                                )
                                .unwrap()],
                                content: vec![],
                            },
                        );
                    }
                }
            }
            if let Some(result) = input.prior.iter().rev().find_map(|entry| {
                if let ExchangeItem::ToolResult { outcome, .. } = entry {
                    Some(match outcome {
                        ToolOutcome::Success { result } => {
                            json!({"ok":true,"details":result.details})
                        }
                        ToolOutcome::Failed { error } => json!({"ok":false,"error":error.message}),
                        _ => json!({"ok":false}),
                    })
                } else {
                    None
                }
            }) {
                self.results.lock().unwrap().push(result);
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: "实际替身响应".into(),
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
    task: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot(existing: Option<PathBuf>) -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("lingxi_service=debug")
            .with_test_writer()
            .try_init();
        let home = existing.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-r07-scheduler-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::write(
            home.join("agents/lingxi/config.yaml"),
            "agent:\n  name: 合成助手\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(ManualClock::new(1_800_000_000_000));
        let provider = Arc::new(Provider::default());
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            approvals,
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 5000,
            },
            &layout,
            ServiceDeps {
                clock: clock.clone(),
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, done) = tokio::sync::oneshot::channel();
        let router = lingxi_service::build_router(state.clone());
        let task = tokio::spawn(async move {
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
        if state
            .storage()
            .get_session("scheduler-source")
            .await
            .unwrap()
            .is_none()
        {
            state
                .session_admin()
                .create_session(
                    "scheduler-source",
                    "lingxi",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "合成来源",
                    Some("operate"),
                    &[],
                    true,
                    clock.now_unix_ms(),
                )
                .await
                .unwrap();
        }
        Self {
            home,
            state,
            clock,
            provider,
            url: format!("http://{addr}"),
            token,
            stop,
            task,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value) -> (u16, Value) {
        let client = reqwest::Client::new();
        let result = client
            .request(method.parse().unwrap(), format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = result.status().as_u16();
        let body = result.json().await.unwrap();
        (status, body)
    }
    async fn add(&self, kind: &str, schedule: Value) -> Value {
        let (status,body)=self.request("POST","/lingxi/v1/desk/cron",json!({"action":"add","actorAgentId":"lingxi","type":kind,"schedule":schedule,"prompt":"核对合成资料","label":"合成计划","executionContext":{"sourceSessionId":"scheduler-source"},"customUnknown":{"keep":true}})).await;
        assert_eq!(status, 200, "{body}");
        body["job"].clone()
    }
    async fn check_due(&self) -> Result<usize, String> {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.state.automation().check_due(&self.state),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "定时任务未在测试期限内结束：providerCalls={}，tasks={:?}，backgroundExits={:?}",
                self.provider.calls.load(Ordering::SeqCst),
                self.state.runs().task_supervisor().tasks(),
                self.state.background().recent_exits()
            )
        })
    }
    async fn close(self) -> PathBuf {
        let _ = self.stop.send(());
        self.task.await.unwrap();
        self.state.storage().close().await.unwrap();
        self.home
    }
}

async fn chat(h: &Harness, input: Value) -> String {
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
    let input = input.to_string();
    let accepted = h
        .state
        .sessions()
        .execute_background_for(
            h.state.storage(),
            h.state.events(),
            h.state.runs(),
            h.state.background(),
            &principal,
            "scheduler-source",
            &lingxi_service::ExecuteSubmission::plain(&input),
            h.clock.now_unix_ms(),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let run = h
                .state
                .storage()
                .load_run(&lingxi_protocol::RunId::new(&accepted.run_id))
                .await
                .unwrap()
                .unwrap();
            if run.status.is_terminal() {
                assert_eq!(run.status, lingxi_protocol::RunStatus::Completed);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("真实Run必须结束");
    accepted.run_id
}

#[tokio::test]
async fn real_tool_suggestion_requires_apply_and_replays_after_expiry() {
    let h = Harness::boot(None).await;
    chat(&h,json!({"tool":"automation","args":{"action":"create","scheduleType":"every","schedule":"1","prompt":"核对资料","label":"草案"}})).await;
    assert!(
        store::list_jobs(h.state.storage(), None)
            .await
            .unwrap()
            .is_empty(),
        "模型建议不能自行启用"
    );
    let result = h.provider.results.lock().unwrap().last().unwrap().clone();
    assert_eq!(result["ok"], true, "{result}");
    let suggestion = result["details"]["suggestionId"].as_str().unwrap();
    assert_eq!(
        result["details"]["automationSuggestion"]["suggestionId"],
        suggestion
    );
    let body = json!({"action":"apply_suggestion","sessionId":"scheduler-source","suggestionId":suggestion,"jobData":{"label":"用户改名","targetAgentId":"lingxi"}});
    let (status, applied) = h
        .request("POST", "/lingxi/v1/desk/cron", body.clone())
        .await;
    assert_eq!(status, 200, "{applied}");
    assert_eq!(applied["job"]["label"], "用户改名");
    assert_eq!(applied["job"]["schedule"], 60_000);
    h.clock.advance(31 * 60_000);
    let (status, replayed) = h.request("POST", "/lingxi/v1/desk/cron", body).await;
    assert_eq!(status, 200, "{replayed}");
    assert_eq!(replayed["job"]["id"], applied["job"]["id"]);
    assert_eq!(
        store::list_jobs(h.state.storage(), None)
            .await
            .unwrap()
            .len(),
        1
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn slash_apply_uses_frozen_owned_suggestion() {
    let h = Harness::boot(None).await;
    chat(&h,json!({"tool":"automation","args":{"action":"create","scheduleType":"every","schedule":"2","prompt":"整理资料","label":"每日资料核对"}})).await;
    let result = h.provider.results.lock().unwrap().last().unwrap().clone();
    assert_eq!(result["ok"], true, "{result}");
    let suggestion = result["details"]["suggestionId"].as_str().unwrap();
    let (status, response) = h
        .request(
            "POST",
            "/lingxi/v1/sessions/scheduler-source/slash",
            json!({"text":format!("/apply {suggestion}")}),
        )
        .await;
    assert_eq!(status, 200, "{response}");
    assert_eq!(response["results"][0]["level"], "success", "{response}");
    assert!(
        response["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("每日资料核对"),
        "{response}"
    );
    assert_eq!(
        store::list_jobs(h.state.storage(), None)
            .await
            .unwrap()
            .len(),
        1
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn real_notify_tool_claim_and_receipt_survive_restart_without_resend() {
    let h = Harness::boot(None).await;
    let run=chat(&h,json!({"tool":"notify","args":{"title":"合成提醒","body":"已整理完成","channels":["desktop"]}})).await;
    let result = h.provider.results.lock().unwrap().last().unwrap().clone();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["details"]["sent"], false);
    let (status, pending) = h
        .request("GET", "/lingxi/v1/automation/notifications", Value::Null)
        .await;
    assert_eq!(status, 200, "{pending}");
    assert_eq!(pending["deliveries"].as_array().unwrap().len(), 1);
    let id = pending["deliveries"][0]["deliveryId"].as_str().unwrap();
    assert_eq!(pending["deliveries"][0]["runId"], run);
    let (_, claimed) = h
        .request(
            "POST",
            "/lingxi/v1/automation/notifications",
            json!({"action":"claim","deliveryId":id}),
        )
        .await;
    assert_eq!(claimed["claim"]["reserved"], true, "{claimed}");
    let (_, again) = h
        .request(
            "POST",
            "/lingxi/v1/automation/notifications",
            json!({"action":"claim","deliveryId":id}),
        )
        .await;
    assert_eq!(again["claim"]["reserved"], false);
    let (status,rejected)=h.request("POST","/lingxi/v1/automation/notifications",json!({"action":"ack","deliveryId":id,"platformReceipt":{"deliveryId":id,"channel":"desktop","displayed":false}})).await;
    assert_eq!(status, 400, "{rejected}");
    let (status,confirmed)=h.request("POST","/lingxi/v1/automation/notifications",json!({"action":"ack","deliveryId":id,"platformReceipt":{"deliveryId":id,"channel":"desktop","displayed":true}})).await;
    assert_eq!(status, 200, "{confirmed}");
    assert_eq!(confirmed["delivery"]["status"], "confirmed");
    let id = id.to_string();
    let home = h.close().await;
    let h = Harness::boot(Some(home)).await;
    h.state
        .automation()
        .recover_pending(&h.state)
        .await
        .unwrap();
    let (_, pending) = h
        .request("GET", "/lingxi/v1/automation/notifications", Value::Null)
        .await;
    assert!(pending["deliveries"].as_array().unwrap().is_empty());
    assert_eq!(
        store::get_notification(h.state.storage(), &id)
            .await
            .unwrap()
            .unwrap()
            .status,
        store::NotificationStatus::Confirmed
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn started_display_with_no_receipt_becomes_unknown_without_resend() {
    let h = Harness::boot(None).await;
    chat(
        &h,
        json!({"tool":"notify","args":{"title":"合成提醒","body":"结果不明检查"}}),
    )
    .await;
    let (_, pending) = h
        .request("GET", "/lingxi/v1/automation/notifications", Value::Null)
        .await;
    let id = pending["deliveries"][0]["deliveryId"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, claimed) = h
        .request(
            "POST",
            "/lingxi/v1/automation/notifications",
            json!({"action":"claim","deliveryId":id}),
        )
        .await;
    assert_eq!(claimed["claim"]["reserved"], true, "{claimed}");
    let home = h.close().await;
    let h = Harness::boot(Some(home)).await;
    h.state
        .automation()
        .recover_pending(&h.state)
        .await
        .unwrap();
    let (_, pending) = h
        .request("GET", "/lingxi/v1/automation/notifications", Value::Null)
        .await;
    assert!(pending["deliveries"].as_array().unwrap().is_empty());
    assert_eq!(
        store::get_notification(h.state.storage(), &id)
            .await
            .unwrap()
            .unwrap()
            .status,
        store::NotificationStatus::Unknown
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn scheduled_reminder_keeps_trigger_run_and_confirmed_delivery_after_restart() {
    let h = Harness::boot(None).await;
    let preferences = h.home.join("user/preferences.json");
    std::fs::create_dir_all(preferences.parent().unwrap()).unwrap();
    std::fs::write(
        &preferences,
        json!({"automation":{"permissionMode":"operate"},"custom":{"keep":true}}).to_string(),
    )
    .unwrap();
    let (status,added)=h.request("POST","/lingxi/v1/desk/cron",json!({"action":"add","type":"every","schedule":60000,"actorAgentId":"lingxi","executionContext":{"sourceSessionId":"scheduler-source"},"prompt":json!({"tool":"notify","args":{"title":"定时合成提醒","body":"检查完成"}}).to_string()})).await;
    assert_eq!(status, 200, "{added}");
    h.clock.advance(60000);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        h.state.automation().check_due(&h.state),
    )
    .await
    .unwrap()
    .unwrap();
    let rows = store::list_trigger_history(h.state.storage(), added["job"]["id"].as_str(), 100)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, store::TriggerStatus::Succeeded);
    let (_, pending) = h
        .request("GET", "/lingxi/v1/automation/notifications", Value::Null)
        .await;
    let delivery = &pending["deliveries"][0];
    assert_eq!(delivery["triggerId"], rows[0].trigger_id);
    assert_eq!(delivery["runId"].as_str(), rows[0].run_id.as_deref());
    let id = delivery["deliveryId"].as_str().unwrap().to_string();
    let (_, claimed) = h
        .request(
            "POST",
            "/lingxi/v1/automation/notifications",
            json!({"action":"claim","deliveryId":id}),
        )
        .await;
    assert_eq!(claimed["claim"]["reserved"], true, "{claimed}");
    let (status,ack)=h.request("POST","/lingxi/v1/automation/notifications",json!({"action":"ack","deliveryId":id,"platformReceipt":{"deliveryId":id,"channel":"desktop","displayed":true}})).await;
    assert_eq!(status, 200, "{ack}");
    let home = h.close().await;
    let h = Harness::boot(Some(home)).await;
    h.state
        .automation()
        .recover_pending(&h.state)
        .await
        .unwrap();
    h.clock.advance(60000);
    assert_eq!(h.state.automation().check_due(&h.state).await.unwrap(), 0);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 0);
    let (_, pending) = h
        .request("GET", "/lingxi/v1/automation/notifications", Value::Null)
        .await;
    assert!(pending["deliveries"].as_array().unwrap().is_empty());
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn existing_global_switch_and_agent_setting_roundtrip_preserve_other_fields() {
    let h = Harness::boot(None).await;
    let preferences = h.home.join("user/preferences.json");
    std::fs::create_dir_all(preferences.parent().unwrap()).unwrap();
    std::fs::write(
        &preferences,
        json!({"custom":{"keep":true},"automation":{"custom":"retained"}}).to_string(),
    )
    .unwrap();
    let (status, updated) = h
        .request(
            "PUT",
            "/lingxi/v1/settings/config",
            json!({"desk":{"heartbeat_master":false},"automation":{"permissionMode":"read_only"}}),
        )
        .await;
    assert_eq!(status, 200, "{updated}");
    let (status, config) = h
        .request("GET", "/lingxi/v1/settings/config", Value::Null)
        .await;
    assert_eq!(status, 200, "{config}");
    assert_eq!(config["desk"]["heartbeat_master"], false);
    assert_eq!(config["automation"]["permissionMode"], "read_only");
    let saved: Value = serde_json::from_slice(&std::fs::read(&preferences).unwrap()).unwrap();
    assert_eq!(saved["custom"]["keep"], true);
    assert_eq!(saved["automation"]["custom"], "retained");
    let(status,changed)=h.request("PUT","/lingxi/v1/agents/lingxi/config",json!({"desk":{"heartbeat_enabled":true,"heartbeat_interval":31},"automation":{"permissionMode":"operate"}})).await;
    assert_eq!(status, 200, "{changed}");
    let (status, config) = h
        .request("GET", "/lingxi/v1/agents/lingxi/config", Value::Null)
        .await;
    assert_eq!(status, 200, "{config}");
    assert_eq!(config["desk"]["heartbeat_enabled"], true);
    assert_eq!(config["desk"]["heartbeat_interval"], 31);
    assert_eq!(config["automation"]["permissionMode"], "operate");
    let (status, rejected) = h
        .request(
            "PUT",
            "/lingxi/v1/settings/config",
            json!({"desk":{"heartbeat_master":true},"automation":{"permissionMode":"forged"}}),
        )
        .await;
    assert_eq!(status, 400, "{rejected}");
    let saved: Value = serde_json::from_slice(&std::fs::read(&preferences).unwrap()).unwrap();
    assert_eq!(saved["heartbeat_master"], false);
    assert_eq!(saved["automation"]["permissionMode"], "operate");
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn unsupported_future_schema_is_preserved_and_does_not_block_later_jobs() {
    let h = Harness::boot(None).await;
    let job = h.add("every", json!(60000)).await;
    let row = store::get_job(h.state.storage(), job["id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    let mut config: Value = serde_json::from_str(&row.config_json).unwrap();
    config["schemaVersion"] = json!(99);
    config["customFuture"] = json!({"keep":true});
    store::create_job(
        h.state.storage(),
        &store::AutomationJobInput {
            id: "future-format".into(),
            studio_id: row.studio_id,
            actor_id: row.actor_id,
            owner_kind: row.owner_kind,
            owner_subject: row.owner_subject,
            schema_version: 99,
            config_json: config.to_string(),
            next_run_at: row.next_run_at,
        },
        h.clock.now_unix_ms() as i64,
    )
    .await
    .unwrap();
    h.clock.advance(60000);
    assert_eq!(h.check_due().await.unwrap(), 2);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    let future = store::get_job(h.state.storage(), "future-format")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(future.schema_version, 99);
    assert_eq!(
        serde_json::from_str::<Value>(&future.config_json).unwrap()["customFuture"]["keep"],
        true
    );
    let runs = store::list_trigger_history(h.state.storage(), Some("future-format"), 10)
        .await
        .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, store::TriggerStatus::Skipped);
    assert_eq!(
        runs[0].result.as_ref().unwrap()["reason"],
        "unsupported_automation_schema"
    );
    h.clock.advance(60000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 2);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn completion_preference_only_queues_real_terminal_results_and_keeps_focus_policy() {
    let h = Harness::boot(None).await;
    let job = h.add("every", json!(60000)).await;
    h.clock.advance(60000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    assert!(
        store::list_pending_notifications(h.state.storage())
            .await
            .unwrap()
            .is_empty(),
        "默认关闭不能擅自打开完成提醒"
    );
    let(status,settings)=h.request("PUT","/lingxi/v1/preferences/notifications",json!({"notifications":{"scheduledTaskCompletion":"when_unfocused","patrolCompletion":"never"}})).await;
    assert_eq!(status, 200, "{settings}");
    h.clock.advance(60000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    let pending = store::list_pending_notifications(h.state.storage())
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    let payload: Value = serde_json::from_str(&pending[0].payload_json).unwrap();
    assert_eq!(payload["desktopFocusPolicy"], "when_unfocused");
    assert_eq!(payload["agentId"], "lingxi");
    assert!(!payload["body"].as_str().unwrap().is_empty());
    let trigger = pending[0].trigger_id.as_deref().unwrap();
    let row = store::get_trigger(h.state.storage(), trigger)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.job_id, job["id"].as_str().unwrap());
    assert_eq!(row.status, store::TriggerStatus::Succeeded);
    assert!(h
        .state
        .storage()
        .load_run(&lingxi_protocol::RunId::new(
            pending[0].run_id.as_deref().unwrap()
        ))
        .await
        .unwrap()
        .unwrap()
        .status
        .is_terminal());
    let id = pending[0].delivery_id.clone();
    let (_, claim) = h
        .request(
            "POST",
            "/lingxi/v1/automation/notifications",
            json!({"action":"claim","deliveryId":id}),
        )
        .await;
    assert_eq!(claim["claim"]["reserved"], true);
    let(status,ack)=h.request("POST","/lingxi/v1/automation/notifications",json!({"action":"ack","deliveryId":id,"platformReceipt":{"deliveryId":id,"channel":"desktop","displayed":true}})).await;
    assert_eq!(status, 200, "{ack}");
    let home = h.close().await;
    let h = Harness::boot(Some(home)).await;
    h.state
        .automation()
        .recover_pending(&h.state)
        .await
        .unwrap();
    assert!(store::list_pending_notifications(h.state.storage())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store::get_notification(h.state.storage(), &id)
            .await
            .unwrap()
            .unwrap()
            .status,
        store::NotificationStatus::Confirmed
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn management_preserves_minute_strings_and_millisecond_numbers_from_real_ui() {
    let h = Harness::boot(None).await;
    let(status,added)=h.request("POST","/lingxi/v1/desk/cron",json!({"action":"add","scheduleType":"every","schedule":"15","actorAgentId":"lingxi","executionContext":{"sourceSessionId":"scheduler-source"},"prompt":"真实界面分钟输入","unknownUiField":{"keep":true}})).await;
    assert_eq!(status, 200, "{added}");
    assert_eq!(added["job"]["schedule"], 900000);
    let id = added["job"]["id"].as_str().unwrap();
    let (status, changed) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"update","id":id,"schedule":"30","label":"改为半小时"}),
        )
        .await;
    assert_eq!(status, 200, "{changed}");
    assert_eq!(changed["job"]["schedule"], 1800000);
    assert_eq!(changed["job"]["unknownUiField"]["keep"], true);
    let (status, off) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"toggle","id":id}),
        )
        .await;
    assert_eq!(status, 200, "{off}");
    assert_eq!(off["job"]["enabled"], false);
    assert!(off["job"]["nextRunAt"].is_null());
    assert_eq!(off["jobs"].as_array().unwrap().len(), 1);
    let (status, on) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"toggle","id":id}),
        )
        .await;
    assert_eq!(status, 200, "{on}");
    assert_eq!(on["job"]["enabled"], true);
    assert_eq!(on["job"]["schedule"], 1800000);
    let (status, changed) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"update","id":id,"schedule":60000}),
        )
        .await;
    assert_eq!(status, 200, "{changed}");
    assert_eq!(changed["job"]["schedule"], 60000);
    let (status, rejected) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"update","id":id,"schedule":30000}),
        )
        .await;
    assert_eq!(status, 400, "{rejected}");
    h.clock.advance(60000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn restart_between_real_run_completion_and_receipt_settlement_catches_up_once() {
    let h = Harness::boot(None).await;
    let job = h.add("every", json!(60000)).await;
    h.clock.advance(60000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    let rows = store::list_trigger_history(h.state.storage(), job["id"].as_str(), 10)
        .await
        .unwrap();
    let trigger = rows[0].trigger_id.clone();
    let due = rows[0].due_at;
    let original = rows[0].run_id.clone().unwrap();
    let run = h
        .state
        .storage()
        .load_run(&lingxi_protocol::RunId::new(&original))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, lingxi_protocol::RunStatus::Completed);
    let completed_at = h
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(updated_at_unix_ms AS TEXT) FROM runs WHERE run_id=?1",
            vec![original.clone()],
        )
        .await
        .unwrap()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    // 只在合成数据库中重建“真实Run已落终态、触发收口尚未提交”的崩溃窗口，不替换执行链。
    h.state.storage().query_one_text("UPDATE automation_triggers_v1 SET status='running',result_json=NULL,error=NULL WHERE trigger_id=?1 RETURNING trigger_id",vec![trigger.clone()]).await.unwrap();
    h.state
        .storage()
        .query_one_text(
            "UPDATE automation_jobs_v1 SET next_run_at=?1 WHERE id=?2 RETURNING id",
            vec![due.to_string(), job["id"].as_str().unwrap().into()],
        )
        .await
        .unwrap();
    let home = h.close().await;
    let h = Harness::boot(Some(home)).await;
    h.clock.advance(3660000);
    h.state
        .automation()
        .recover_pending(&h.state)
        .await
        .unwrap();
    let current = store::get_job(h.state.storage(), job["id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.next_run_at, Some(completed_at + 60000));
    assert!(current.next_run_at.unwrap() < h.clock.now_unix_ms() as i64);
    assert_eq!(h.check_due().await.unwrap(), 1);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.check_due().await.unwrap(), 0);
    let rows = store::list_trigger_history(h.state.storage(), job["id"].as_str(), 10)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .any(|row| row.run_id.as_deref() == Some(original.as_str())));
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn notification_preference_updates_preserve_other_settings_and_reject_damage() {
    let h = Harness::boot(None).await;
    let path = h.home.join("user/preferences.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path,json!({"customRoot":"retained","notifications":{"turnCompletion":"when_unfocused","futurePreference":{"keep":true},"patrolCompletion":"never"}}).to_string()).unwrap();
    let (status, settings) = h
        .request(
            "PUT",
            "/lingxi/v1/preferences/notifications",
            json!({"notifications":{"scheduledTaskCompletion":"always"}}),
        )
        .await;
    assert_eq!(status, 200, "{settings}");
    assert_eq!(
        settings["notifications"]["chatCompletion"],
        "when_unfocused"
    );
    assert_eq!(
        settings["notifications"]["scheduledTaskCompletion"],
        "always"
    );
    let saved = std::fs::read(&path).unwrap();
    let config: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(config["customRoot"], "retained");
    assert_eq!(config["notifications"]["futurePreference"]["keep"], true);
    let (status, rejected) = h
        .request(
            "PUT",
            "/lingxi/v1/preferences/notifications",
            json!({"notifications":{"scheduledTaskCompletion":true}}),
        )
        .await;
    assert_eq!(status, 400, "{rejected}");
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    let (status, rejected) = h
        .request(
            "PUT",
            "/lingxi/v1/preferences/notifications",
            json!({"notifications":{"scheduledTaskCompletion":"invented"}}),
        )
        .await;
    assert_eq!(status, 400, "{rejected}");
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    let (status, read) = h
        .request("GET", "/lingxi/v1/preferences/notifications", Value::Null)
        .await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["notifications"], settings["notifications"]);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

// F02 批次A：R00-T02-LA-095E0402EEEC 的“过期/冲突拒绝”与“拒绝时无越权或
// 错误持久化、错误对用户可见”此前无真实证据（F02 范围审查错端点组）。
#[tokio::test]
async fn apply_suggestion_rejections_are_explicit_and_persist_nothing() {
    let h = Harness::boot(None).await;
    chat(&h,json!({"tool":"automation","args":{"action":"create","scheduleType":"every","schedule":"5","prompt":"拒绝路径","label":"拒绝路径"}})).await;
    let result = h.provider.results.lock().unwrap().last().unwrap().clone();
    assert_eq!(result["ok"], true, "{result}");
    let suggestion = result["details"]["suggestionId"].as_str().unwrap();
    // 未知建议：错误可见。
    let (status, missing) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"apply_suggestion","sessionId":"scheduler-source","suggestionId":"no-such-suggestion"}),
        )
        .await;
    assert_eq!(status, 400, "{missing}");
    assert!(
        missing["error"]
            .to_string()
            .contains("automation_suggestion_unavailable"),
        "{missing}"
    );
    // 冲突：建议目标 Agent 不可更换。
    let (status, agent) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"apply_suggestion","sessionId":"scheduler-source","suggestionId":suggestion,"jobData":{"targetAgentId":"someone-else"}}),
        )
        .await;
    assert_eq!(status, 400, "{agent}");
    assert!(
        agent["error"]
            .to_string()
            .contains("suggestion target agent cannot change"),
        "{agent}"
    );
    // 冲突：不支持的建议编辑字段。
    let (status, edit) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"apply_suggestion","sessionId":"scheduler-source","suggestionId":suggestion,"jobData":{"unsupportedKey":true}}),
        )
        .await;
    assert_eq!(status, 400, "{edit}");
    assert!(
        edit["error"]
            .to_string()
            .contains("unsupported suggestion edit"),
        "{edit}"
    );
    // 会话/Studio 身份不符按不可用处理。
    let (status, other) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"apply_suggestion","sessionId":"some-other-session","suggestionId":suggestion}),
        )
        .await;
    assert_eq!(status, 400, "{other}");
    assert!(
        other["error"]
            .to_string()
            .contains("automation_suggestion_unavailable"),
        "{other}"
    );
    // 未应用建议过期后拒绝（TTL 30 分钟，automation_tools.rs:152）。
    h.clock.advance(31 * 60_000);
    let (status, expired) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"apply_suggestion","sessionId":"scheduler-source","suggestionId":suggestion}),
        )
        .await;
    assert_eq!(status, 400, "{expired}");
    assert!(
        expired["error"]
            .to_string()
            .contains("automation_suggestion_expired"),
        "{expired}"
    );
    assert!(
        store::list_jobs(h.state.storage(), None)
            .await
            .unwrap()
            .is_empty(),
        "全部拒绝路径不得创建任务"
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

// F02 批次A：R00-T02-LA-5F394AA4C9A3 的“找不到建议、Studio 身份缺失或已应用
// 时不创建重复任务”此前无真实证据。
#[tokio::test]
async fn slash_apply_missing_cross_session_and_repeat_never_duplicate_jobs() {
    let h = Harness::boot(None).await;
    chat(&h,json!({"tool":"automation","args":{"action":"create","scheduleType":"every","schedule":"2","prompt":"身份与重复","label":"身份与重复"}})).await;
    let result = h.provider.results.lock().unwrap().last().unwrap().clone();
    assert_eq!(result["ok"], true, "{result}");
    let suggestion = result["details"]["suggestionId"].as_str().unwrap();
    // 找不到建议：错误对用户可见。
    let (status, missing) = h
        .request(
            "POST",
            "/lingxi/v1/sessions/scheduler-source/slash",
            json!({"text":"/apply no-such-suggestion"}),
        )
        .await;
    assert_eq!(status, 200, "{missing}");
    assert_eq!(missing["results"][0]["level"], "error", "{missing}");
    assert!(
        missing["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("automation_suggestion_unavailable"),
        "{missing}"
    );
    // Studio/会话身份缺失：另一会话引用本会话建议按不可用拒绝，不产生任务。
    h.state
        .session_admin()
        .create_session(
            "other-source",
            "lingxi",
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
            "身份边界",
            Some("operate"),
            &[],
            true,
            h.clock.now_unix_ms(),
        )
        .await
        .unwrap();
    let (status, cross) = h
        .request(
            "POST",
            "/lingxi/v1/sessions/other-source/slash",
            json!({"text":format!("/apply {suggestion}")}),
        )
        .await;
    assert_eq!(status, 200, "{cross}");
    assert_eq!(cross["results"][0]["level"], "error", "{cross}");
    assert!(
        cross["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("automation_suggestion_unavailable"),
        "{cross}"
    );
    assert!(store::list_jobs(h.state.storage(), None)
        .await
        .unwrap()
        .is_empty());
    // 已应用后重复 /apply：幂等返回，不创建重复任务。
    for expected in ["success", "success"] {
        let (status, applied) = h
            .request(
                "POST",
                "/lingxi/v1/sessions/scheduler-source/slash",
                json!({"text":format!("/apply {suggestion}")}),
            )
            .await;
        assert_eq!(status, 200, "{applied}");
        assert_eq!(applied["results"][0]["level"], expected, "{applied}");
    }
    assert_eq!(
        store::list_jobs(h.state.storage(), None)
            .await
            .unwrap()
            .len(),
        1
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
