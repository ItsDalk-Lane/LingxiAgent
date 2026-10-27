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
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

#[derive(Default)]
struct Provider {
    calls: AtomicUsize,
    results: Mutex<Vec<Value>>,
}
#[derive(Default)]
struct PendingReview {
    started: tokio::sync::Notify,
    run: Mutex<Option<String>>,
    dropped: AtomicBool,
}
struct ReviewDrop<'a>(&'a AtomicBool);
impl Drop for ReviewDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl lingxi_service::automation_review::AutoReviewPort for PendingReview {
    fn review<'a>(
        &'a self,
        request: &'a lingxi_service::automation_review::AutoReviewRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = lingxi_service::automation_review::AutoReviewDecision>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let _drop = ReviewDrop(&self.dropped);
            *self.run.lock().unwrap() = Some(request.ctx.run_id.to_string());
            self.started.notify_one();
            std::future::pending().await
        })
    }
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
        self.add_prompt(kind, schedule, "核对合成资料").await
    }
    async fn add_prompt(&self, kind: &str, schedule: Value, prompt: &str) -> Value {
        let (status,body)=self.request("POST","/lingxi/v1/desk/cron",json!({"action":"add","actorAgentId":"lingxi","type":kind,"schedule":schedule,"prompt":prompt,"label":"合成计划","executionContext":{"sourceSessionId":"scheduler-source"},"customUnknown":{"keep":true}})).await;
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
        tokio::time::timeout(std::time::Duration::from_secs(10), self.task)
            .await
            .expect("测试HTTP服务未在期限内关闭")
            .unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.state.storage().close(),
        )
        .await
        .expect("测试存储未在期限内关闭")
        .unwrap();
        self.home
    }
}

#[tokio::test]
async fn background_input_and_real_tool_results_reload_after_restart_without_user_messages() {
    let h = Harness::boot(None).await;
    let prompt = json!({"tool":"automation","args":{"action":"list"}}).to_string();
    let job = h.add_prompt("every", json!(60_000), &prompt).await;
    h.clock.advance(60_000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 2);
    let results = h.provider.results.lock().unwrap().clone();
    assert_eq!(results[0]["ok"], true, "{results:?}");
    let rows = store::list_trigger_history(h.state.storage(), job["id"].as_str(), 10)
        .await
        .unwrap();
    assert_eq!(rows[0].status, store::TriggerStatus::Succeeded);
    let run = rows[0].run_id.clone().unwrap();
    let session = rows[0].session_id.clone().unwrap();
    let messages =
        lingxi_adapters::storage::session_tree::list_branch_messages(h.state.storage(), &session)
            .await
            .unwrap();
    assert!(messages
        .iter()
        .all(|row| row.role != "user" && !row.message_id.starts_with("user:")));
    let anchor = messages
        .iter()
        .find(|row| row.message_id == format!("model-history:{run}:input"))
        .unwrap();
    let body: Value = serde_json::from_str(&anchor.content_json).unwrap();
    assert_eq!(body["inputOrigin"], "cron");
    assert!(body["payload"]["items"][0]["text"]
        .as_str()
        .unwrap()
        .contains(&prompt));
    let home = h.close().await;
    let h = Harness::boot(Some(home)).await;
    let history = lingxi_service::model_history::ModelHistory::new(h.state.storage().clone());
    let restored = history
        .load(&lingxi_kernel::Principal::LocalUser, "lingxi", &session, "")
        .await
        .unwrap();
    assert!(
        matches!(restored.first(),Some(ExchangeItem::UserTurn{text}) if text.contains(&prompt))
    );
    assert!(restored.iter().any(|item| matches!(
        item,
        ExchangeItem::ToolResult {
            outcome: ToolOutcome::Success { .. },
            ..
        }
    )));
    assert_eq!(
        h.provider.calls.load(Ordering::SeqCst),
        0,
        "恢复历史不能重发模型或工具"
    );
    // 不能用后台输入锚点绕过原用户来源；活跃源记录缺谱系也必须响亮失败。
    rusqlite::Connection::open(h.state.storage().db_path())
        .unwrap()
        .execute(
            "UPDATE run_lineage SET origin='user' WHERE run_id=?1",
            [&run],
        )
        .unwrap();
    assert!(format!(
        "{:?}",
        history
            .load(&lingxi_kernel::Principal::LocalUser, "lingxi", &session, "")
            .await
            .unwrap_err()
    )
    .contains("automation input source lineage mismatch"));
    rusqlite::Connection::open(h.state.storage().db_path())
        .unwrap()
        .execute("DELETE FROM run_lineage WHERE run_id=?1", [&run])
        .unwrap();
    assert!(format!(
        "{:?}",
        history
            .load(&lingxi_kernel::Principal::LocalUser, "lingxi", &session, "")
            .await
            .unwrap_err()
    )
    .contains("automation input source lineage missing"));
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancellation_drops_pending_automatic_review_before_any_tool_dispatch() {
    let h = Harness::boot(None).await;
    let reviewer = Arc::new(PendingReview::default());
    h.state
        .tool_gateway()
        .unwrap()
        .set_automatic_reviewer(reviewer.clone());
    let prompt=json!({"tool":"notify","args":{"title":"合成提醒","body":"仅测试，不发送","channels":["desktop"]}}).to_string();
    let job = h.add_prompt("every", json!(60_000), &prompt).await;
    h.clock.advance(60_000);
    let cancelling = async {
        reviewer.started.notified().await;
        let due = store::get_job(h.state.storage(), job["id"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap()
            .next_run_at;
        // 真实任务跨过多个到期时点后，重复检查只让路，不能领取第二次或丢掉原游标。
        h.clock.advance(10 * 60_000);
        assert_eq!(h.check_due().await.unwrap(), 0);
        assert_eq!(
            store::get_job(h.state.storage(), job["id"].as_str().unwrap())
                .await
                .unwrap()
                .unwrap()
                .next_run_at,
            due
        );
        assert_eq!(
            store::list_trigger_history(h.state.storage(), job["id"].as_str(), 10)
                .await
                .unwrap()
                .len(),
            1
        );
        let run = reviewer.run.lock().unwrap().clone().unwrap();
        assert_eq!(
            h.state.runs().cancel_run(&run, "测试取消自动审核"),
            lingxi_service::cancel::FireOutcome::Fired
        );
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(h.check_due(), cancelling)
    })
    .await
    .expect("审核不能阻塞已接受的取消");
    assert_eq!(result.unwrap(), 1);
    assert!(
        reviewer.dropped.load(Ordering::SeqCst),
        "取消必须丢弃审核future并运行其Drop收尾"
    );
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    assert!(
        store::list_pending_notifications(h.state.storage())
            .await
            .unwrap()
            .is_empty(),
        "尚在审核的通知不能派发"
    );
    let rows = store::list_trigger_history(h.state.storage(), job["id"].as_str(), 10)
        .await
        .unwrap();
    assert_eq!(rows[0].status, store::TriggerStatus::Cancelled);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn real_management_dispatch_and_restart_keep_one_trigger_one_run() {
    let h = Harness::boot(None).await;
    let job = h.add("every", json!(60_000)).await;
    let id = job["id"].as_str().unwrap().to_string();
    h.clock.advance(60_000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    let triggers = store::list_trigger_history(h.state.storage(), Some(&id), 20)
        .await
        .unwrap();
    assert_eq!(triggers.len(), 1);
    assert_eq!(triggers[0].status, store::TriggerStatus::Succeeded);
    let run = triggers[0].run_id.as_ref().unwrap();
    let lineage = h
        .state
        .storage()
        .load_run_lineage(&lingxi_protocol::RunId::new(run))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lineage.origin, lingxi_kernel::subagent::RunOrigin::Cron);
    assert_eq!(
        lineage.cause_id.as_deref(),
        Some(triggers[0].trigger_id.as_str())
    );
    let home = h.close().await;
    let h = Harness::boot(Some(home)).await;
    h.clock.advance(60_000);
    h.state
        .automation()
        .recover_pending(&h.state)
        .await
        .unwrap();
    assert_eq!(h.check_due().await.unwrap(), 0);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 0);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn edits_preserve_unknown_fields_and_stale_revisions_are_rejected() {
    let h = Harness::boot(None).await;
    let job = h.add("every", json!(60_000)).await;
    let (status, changed) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"update","id":job["id"],"expectedConfigRevision":1,"label":"改名"}),
        )
        .await;
    assert_eq!(status, 200, "{changed}");
    assert_eq!(changed["job"]["customUnknown"]["keep"], true);
    let (status, _) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"toggle","id":job["id"],"expectedConfigRevision":1}),
        )
        .await;
    assert_eq!(status, 409);
    let (status, removed) = h
        .request(
            "POST",
            "/lingxi/v1/desk/cron",
            json!({"action":"remove","id":job["id"],"expectedConfigRevision":2}),
        )
        .await;
    assert_eq!(status, 200, "{removed}");
    assert!(removed["jobs"].as_array().unwrap().is_empty());
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn one_shot_is_disabled_after_real_execution() {
    let h = Harness::boot(None).await;
    let due = h.clock.now_unix_ms() + 60_000;
    let job = h
        .add(
            "at",
            json!(chrono::DateTime::from_timestamp_millis(due as i64)
                .unwrap()
                .to_rfc3339()),
        )
        .await;
    h.clock.advance(60_000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    let (_, list) = h.request("GET", "/lingxi/v1/desk/cron", Value::Null).await;
    assert_eq!(list["jobs"][0]["enabled"], false, "{list}");
    assert!(list["jobs"][0]["nextRunAt"].is_null());
    h.clock.advance(86_400_000);
    assert_eq!(h.check_due().await.unwrap(), 0);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    assert!(!job["id"].is_null());
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn unknown_claim_after_restart_is_not_reexecuted() {
    let h = Harness::boot(None).await;
    let job = h.add("every", json!(60_000)).await;
    h.clock.advance(60_000);
    let db_job = store::get_job(h.state.storage(), job["id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    let id = lingxi_service::automation::trigger_id(
        &db_job.studio_id,
        &db_job.id,
        db_job.config_revision,
        db_job.next_run_at.unwrap(),
    );
    store::claim_trigger(
        h.state.storage(),
        &store::TriggerInput {
            trigger_id: id.clone(),
            job_id: db_job.id.clone(),
            due_at: db_job.next_run_at.unwrap(),
            config_revision: db_job.config_revision,
        },
        h.clock.now_unix_ms() as i64,
    )
    .await
    .unwrap();
    let home = h.close().await;
    let h = Harness::boot(Some(home)).await;
    h.state
        .automation()
        .recover_pending(&h.state)
        .await
        .unwrap();
    h.clock.advance(86_400_000);
    assert_eq!(h.check_due().await.unwrap(), 0);
    assert_eq!(
        store::get_trigger(h.state.storage(), &id)
            .await
            .unwrap()
            .unwrap()
            .status,
        store::TriggerStatus::Unknown
    );
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 0);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn revoked_source_is_rejected_without_a_run() {
    let h = Harness::boot(None).await;
    let job = h.add("every", json!(60_000)).await;
    h.state
        .session_admin()
        .archive_session("scheduler-source", None, h.clock.now_unix_ms())
        .await
        .unwrap();
    h.clock.advance(60_000);
    assert_eq!(h.check_due().await.unwrap(), 1);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 0);
    let rows = store::list_trigger_history(h.state.storage(), job["id"].as_str(), 10)
        .await
        .unwrap();
    assert_eq!(rows[0].status, store::TriggerStatus::Failed);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
