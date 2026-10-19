//! 仅替换外部模型：真实 HTTP、手机宿主、工具网关、文件动作、Run 和重启恢复共同验证。
use lingxi_adapters::storage::{conversations as store, session_tree, RunDatabase};
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    subagent::RunOrigin,
    toolcatalog::{SchemaBudget, ToolRegistry},
    RunContext,
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, RunId, RunStatus};
use lingxi_service::{
    approval_service::{Answer, ApprovalService},
    filetools::{register_core_file_tools, NoopChangeLog},
    inject::{ManualClock, ServiceClock},
    prepare_layout,
    resourceaccess::ResourceAccess,
    toolgateway::ToolInvocationGateway,
    ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

const NOW: u64 = 1_800_000_000_000;
#[derive(Clone, Copy)]
enum Strategy {
    Mixed,
    Pass,
    Hold,
    HoldPass,
    Write,
    Ping,
    Stop,
    StopMarker(&'static str),
}
#[derive(Clone, Debug)]
struct Observation {
    ctx: RunContext,
    actor: String,
    origin: RunOrigin,
    delivery: String,
    declarations: Vec<String>,
}
struct Model {
    db: OnceLock<Arc<RunDatabase>>,
    strategy: Strategy,
    workspace: PathBuf,
    observations: Mutex<Vec<Observation>>,
    outcomes: Mutex<Vec<ToolOutcome>>,
    release: tokio::sync::Semaphore,
}
impl TurnProviderPort for Model {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.fixture".into(),
            model: "phone-runtime".into(),
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
            let db = self.db.get().expect("正式数据库已接入模型观察端");
            let lineage = db.load_run_lineage(&ctx.run_id).await.unwrap().unwrap();
            let phone = matches!(lineage.origin, RunOrigin::Channel | RunOrigin::Dm);
            let actor = session_tree::get_session_row(db, ctx.session_id.as_str())
                .await
                .unwrap()
                .unwrap()
                .agent_id;
            if input.turn == 1 {
                if phone {
                    self.observations.lock().unwrap().push(Observation {
                        ctx: ctx.clone(),
                        actor: actor.clone(),
                        origin: lineage.origin,
                        delivery: lineage.cause_id.clone().expect("手机真实来源"),
                        declarations: input
                            .tools
                            .declarations
                            .iter()
                            .map(|d| d.target.clone())
                            .collect(),
                    });
                    if matches!(self.strategy, Strategy::Hold | Strategy::HoldPass) {
                        self.release.acquire().await.unwrap().forget();
                    }
                    let tool = match self.strategy {
                        Strategy::Mixed if actor == "a" => {
                            Some(("channel_reply", json!({"content":"公开回复<done>"})))
                        }
                        Strategy::Mixed => None,
                        Strategy::Pass | Strategy::HoldPass => {
                            Some(("channel_pass", json!({"reason":"本次明确跳过"})))
                        }
                        Strategy::Hold => {
                            Some(("channel_reply", json!({"content":"撤权后不应出现<done>"})))
                        }
                        Strategy::Write => Some((
                            "write",
                            json!({"path":self.workspace.join("forbidden.txt"),"content":"不应写入"}),
                        )),
                        Strategy::Ping => Some((
                            "channel_reply",
                            json!({"content":format!("来自{actor}的往返消息")}),
                        )),
                        Strategy::Stop => Some((
                            "channel_reply",
                            json!({"content":format!("来自{actor}的停止消息<done>")}),
                        )),
                        Strategy::StopMarker(marker) => Some((
                            "channel_reply",
                            json!({"content":format!("标记清理后的正文 {marker}")}),
                        )),
                    };
                    if let Some((name, args)) = tool {
                        return ProviderTurnResult::of_ctx(
                            ctx,
                            ProviderTurn::ToolRequests {
                                requests: vec![ToolRequest::from_effective_arguments(
                                    format!("tool:first-party:{name}"),
                                    args,
                                    &SchemaBudget::default(),
                                )
                                .unwrap()],
                                content: vec![],
                            },
                        );
                    }
                } else if let Ok(command) = serde_json::from_str::<Value>(
                    input
                        .submission
                        .rsplit("\n\n")
                        .next()
                        .unwrap_or(&input.submission),
                ) {
                    if command["tool"] == "dm" {
                        return ProviderTurnResult::of_ctx(
                            ctx,
                            ProviderTurn::ToolRequests {
                                requests: vec![ToolRequest::from_effective_arguments(
                                    "tool:first-party:dm",
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
            if let Some(outcome) = input.prior.iter().rev().find_map(|item| match item {
                ExchangeItem::ToolResult { outcome, .. } => Some(outcome.clone()),
                _ => None,
            }) {
                self.outcomes.lock().unwrap().push(outcome);
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: format!("{actor}的私人普通结尾"),
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
    model: Arc<Model>,
    approvals: Arc<ApprovalService>,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot(existing: Option<PathBuf>, strategy: Strategy) -> Self {
        let home = existing.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "r07-phone-runtime-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        for actor in ["a", "b", "c"] {
            std::fs::create_dir_all(home.join(format!("agents/{actor}"))).unwrap();
            std::fs::write(
                home.join(format!("agents/{actor}/config.yaml")),
                format!("agent:\n  name: {actor}\nmemory:\n  enabled: false\n"),
            )
            .unwrap();
        }
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            json!({"channels_enabled":true,"primary_agent_id":"a","current_agent_id":"c"})
                .to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(home.join("workspace")).unwrap();
        let home = home.canonicalize().unwrap();
        let workspace = home.join("workspace");
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let model = Arc::new(Model {
            db: OnceLock::new(),
            strategy,
            workspace: workspace.clone(),
            observations: Mutex::new(vec![]),
            outcomes: Mutex::new(vec![]),
            release: tokio::sync::Semaphore::new(0),
        });
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let registry = Arc::new(ToolRegistry::new());
        let budget = SchemaBudget::default();
        let gateway = Arc::new(ToolInvocationGateway::new(
            registry.clone(),
            approvals.clone(),
            clock.clone(),
            budget,
            300_000,
            1000,
        ));
        // 使用生产文件执行器，拒绝时必须真正保持磁盘不变。
        register_core_file_tools(
            &registry,
            &gateway,
            Arc::new(ResourceAccess::new(std::slice::from_ref(&workspace)).unwrap()),
            workspace.clone(),
            clock.clone(),
            Arc::new(NoopChangeLog),
            &budget,
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
                clock: clock.clone(),
                turn_provider: Some(model.clone()),
                tool_gateway: Some(gateway),
                approval_gate: Some(approvals.clone()),
                workspace_root: Some(workspace),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        assert!(model.db.set(state.storage().clone()).is_ok());
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = lingxi_service::build_router(state.clone());
        let (stop, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .unwrap();
        });
        Self {
            home,
            state,
            clock,
            model,
            approvals,
            url,
            token,
            stop,
            server,
        }
    }
    fn start(&self) {
        self.state.conversations().start(self.state.clone());
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(12));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw":text})),
        )
    }
    async fn post(&self, path: &str, body: Value) -> Value {
        let (status, result) = self.request(reqwest::Method::POST, path, Some(body)).await;
        assert!((200..300).contains(&status), "HTTP {status}: {result}");
        result
    }
    async fn get(&self, path: &str) -> Value {
        let (status, result) = self.request(reqwest::Method::GET, path, None).await;
        assert_eq!(status, 200, "{result}");
        result
    }
    async fn channel(&self, roster: &[&str]) -> String {
        let result = self
            .post(
                "/lingxi/v1/channels",
                json!({"name":"真实手机频道","members":roster,"requestId":"create-phone-channel"}),
            )
            .await;
        let id = result["id"].as_str().unwrap().to_string();
        self.post(
            &format!("/lingxi/v1/conversations/{id}/agent-phone-settings"),
            json!({"proactiveEnabled":false,"mode":"read_only","guardLimit":4}),
        )
        .await;
        id
    }
    async fn message(&self, id: &str, key: &str, body: &str) -> Value {
        self.post(
            &format!("/lingxi/v1/channels/{id}/messages"),
            json!({"body":body,"requestId":key}),
        )
        .await
    }
    async fn ids(&self) -> Vec<String> {
        let text=self.state.storage().query_one_text("SELECT json_group_array(delivery_id) FROM (SELECT delivery_id FROM phone_deliveries_v1 ORDER BY rowid)",vec![]).await.unwrap().unwrap();
        serde_json::from_str(&text).unwrap()
    }
    async fn deliveries(&self) -> Vec<store::PhoneDelivery> {
        let mut result = vec![];
        for id in self.ids().await {
            result.push(
                store::get_delivery(self.state.storage(), &id)
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        result
    }
    async fn settled(&self, count: usize) -> Vec<store::PhoneDelivery> {
        tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let rows = self.deliveries().await;
                if rows.len() >= count
                    && rows.iter().all(|d| {
                        !matches!(
                            d.status,
                            store::PhoneDeliveryStatus::Claimed
                                | store::PhoneDeliveryStatus::Running
                        )
                    })
                {
                    return rows;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "手机真实投递未完成；观察={:?}，宿主={}",
                self.model.observations.lock().unwrap(),
                self.state.conversations().status()
            )
        })
    }
    async fn observed(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(12), async {
            while self.model.observations.lock().unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("真正手机模型调用未出现");
    }
    async fn run_terminal(&self, id: &str) -> RunStatus {
        tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let row = self
                    .state
                    .storage()
                    .load_run(&RunId::new(id))
                    .await
                    .unwrap()
                    .unwrap();
                if row.status.is_terminal() {
                    return row.status;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("正式 Run 未终结")
    }
    async fn dm_settings(&self) {
        for (actor, peer) in [("a", "b"), ("b", "a")] {
            self.post(
                &format!("/lingxi/v1/conversations/dm:{peer}/agent-phone-settings?agentId={actor}"),
                json!({"proactiveEnabled":false,"mode":"read_only"}),
            )
            .await;
        }
        self.post("/lingxi/v1/sessions",json!({"sessionId":"dm-source","agentId":"a","title":"私信来源","permissionMode":"operate"})).await;
    }
    async fn send_dm(&self, body: &str) -> String {
        let principal = self
            .state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", self.token)),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        let input = json!({"tool":"dm","args":{"to":"b","message":body}}).to_string();
        let run = self
            .state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &principal,
                "dm-source",
                &ExecuteSubmission::plain(&input),
                self.clock.now_unix_ms(),
            )
            .await
            .unwrap()
            .run_id;
        let id = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(p) = self
                    .approvals
                    .pending_for_principal(&lingxi_kernel::Principal::LocalUser, "dm-source")
                    .into_iter()
                    .find(|p| p.run_id == run)
                {
                    break p.approval_id;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("正式私信未进入人工确认");
        assert!(self
            .approvals
            .answer_for_principal(
                &lingxi_kernel::Principal::LocalUser,
                "dm-source",
                &id,
                Answer::Approve
            )
            .settled());
        assert_eq!(self.run_terminal(&run).await, RunStatus::Completed);
        run
    }
    async fn tick(&self) {
        self.state.conversations().tick(&self.state).await.unwrap();
    }
    async fn close(self, delete: bool) -> PathBuf {
        self.model.release.add_permits(32);
        self.state.conversations().stop();
        tokio::time::timeout(
            Duration::from_secs(12),
            self.state.conversations().wait_stopped(),
        )
        .await
        .unwrap()
        .unwrap();
        self.state
            .background()
            .drain_within(Duration::from_secs(5))
            .await;
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), self.server)
            .await
            .unwrap()
            .unwrap();
        self.state.storage().close().await.unwrap();
        let home = self.home.clone();
        drop(self.state);
        if delete {
            std::fs::remove_dir_all(&home).unwrap();
        }
        home
    }
}

#[tokio::test]
async fn actual_http_phone_only_explicit_reply_is_public_request_retry_and_restart_do_not_repeat() {
    let h = Harness::boot(None, Strategy::Mixed).await;
    let id = h.channel(&["a", "b"]).await;
    h.start();
    let first = h.message(&id, "user-action-1", "请各自处理").await;
    let retry = h.message(&id, "user-action-1", "请各自处理").await;
    assert_eq!(first, retry);
    let rows = h.settled(2).await;
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|d| d.status == store::PhoneDeliveryStatus::Completed));
    let history = h.get(&format!("/lingxi/v1/channels/{id}")).await;
    let messages = history["messages"].as_array().unwrap();
    assert_eq!(
        messages.len(),
        2,
        "同一请求只能写入一次，私人结尾不能公开：{history}"
    );
    assert_eq!(messages[0]["sender"], "User");
    assert_eq!(messages[1]["sender"], "a");
    assert_eq!(messages[1]["body"], "公开回复");
    assert_eq!(
        rows.iter()
            .find(|d| d.input.actor_id == "a")
            .unwrap()
            .decision_body
            .as_deref(),
        Some("公开回复<done>"),
        "原始决策回执仍须保留控制标记证据"
    );
    let observations = h.model.observations.lock().unwrap().clone();
    for observation in observations {
        assert_eq!(observation.origin, RunOrigin::Channel);
        let delivery = store::get_delivery(h.state.storage(), &observation.delivery)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            delivery.run_id.as_deref(),
            Some(observation.ctx.run_id.as_str())
        );
        assert_eq!(delivery.input.actor_id, observation.actor);
        assert!(observation
            .declarations
            .iter()
            .any(|t| t.ends_with(":channel_reply")));
        assert!(observation
            .declarations
            .iter()
            .all(|t| !["channel", "dm", "browser", "chrome"]
                .contains(&t.rsplit(':').next().unwrap())));
    }
    let activities = h
        .get(&format!("/lingxi/v1/conversations/{id}/agent-activities"))
        .await;
    let activities = activities["activities"].as_array().unwrap();
    assert!(!activities.is_empty());
    for activity in activities {
        assert_eq!(activity["conversationId"], id);
        assert!(["a", "b"].contains(&activity["agentId"].as_str().unwrap()));
        assert!(
            chrono::DateTime::parse_from_rfc3339(activity["timestamp"].as_str().unwrap()).is_ok()
        );
        assert!(activity["state"].is_string() && activity["summary"].is_string());
        let sid = activity["details"]["sessionId"].as_str().unwrap();
        assert_eq!(activity["details"]["sessionPath"], format!("native:{sid}"));
    }
    for row in &rows {
        let sid = row.session_id.as_deref().unwrap();
        let native = h.get(&format!("/lingxi/v1/sessions/{sid}/history")).await;
        assert!(
            native.to_string().contains("私人普通结尾"),
            "手机活动必须能打开实际私人历史：{native}"
        );
    }
    let home = h.close(false).await;
    let h = Harness::boot(Some(home), Strategy::Mixed).await;
    h.start();
    h.tick().await;
    h.message(&id, "user-action-1", "请各自处理").await;
    h.tick().await;
    assert_eq!(h.ids().await.len(), 2, "重启或请求重试不能重建已确认投递");
    assert!(h.model.observations.lock().unwrap().is_empty());
    assert_eq!(
        h.get(&format!("/lingxi/v1/channels/{id}")).await["messages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    h.close(true).await;
}

#[tokio::test]
async fn actual_channel_pass_records_decision_without_publishing_ordinary_final() {
    let h = Harness::boot(None, Strategy::Pass).await;
    let id = h.channel(&["a", "b"]).await;
    h.start();
    h.message(&id, "skip-action", "可以跳过").await;
    let rows = h.settled(2).await;
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|d| d.decision == Some(store::PhoneDecision::Pass)
            && d.status == store::PhoneDeliveryStatus::Completed));
    assert_eq!(
        h.get(&format!("/lingxi/v1/channels/{id}")).await["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    h.close(true).await;
}

#[tokio::test]
async fn actual_member_removal_cancels_waiting_models_before_any_publication() {
    let h = Harness::boot(None, Strategy::Hold).await;
    let id = h.channel(&["a", "b", "c"]).await;
    h.start();
    h.message(&id, "held-action", "等待时撤销成员").await;
    h.observed(3).await;
    let (status, result) = h
        .request(
            reqwest::Method::DELETE,
            &format!("/lingxi/v1/channels/{id}/members/b"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{result}");
    h.model.release.add_permits(16);
    let rows = h.settled(3).await;
    assert!(rows.iter().all(|d| d.decision.is_none()));
    assert!(rows
        .iter()
        .all(|d| d.status != store::PhoneDeliveryStatus::Completed));
    assert_eq!(
        h.get(&format!("/lingxi/v1/channels/{id}")).await["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    h.close(true).await;
}

#[tokio::test]
async fn actual_read_only_phone_rejects_native_write_and_hides_recursive_tools() {
    let h = Harness::boot(None, Strategy::Write).await;
    let id = h.channel(&["a", "b"]).await;
    h.start();
    h.message(&id, "readonly-action", "尝试写文件").await;
    let rows = h.settled(2).await;
    assert!(rows.iter().all(|d| d.decision.is_none()));
    assert!(!h.home.join("workspace/forbidden.txt").exists());
    for row in &rows {
        let sid = row.session_id.as_deref().unwrap();
        assert_eq!(
            session_tree::get_session_row(h.state.storage(), sid)
                .await
                .unwrap()
                .unwrap()
                .permission_mode
                .as_deref(),
            Some("read_only")
        );
        let refusal=h.state.storage().query_one_text("SELECT receipt_detail FROM invocation_journal WHERE run_id=?1 AND target='tool:first-party:write' AND receipt_outcome='failed' AND dispatched=0",vec![row.run_id.clone().unwrap()]).await.unwrap().expect("实际文件执行前必须持久拒绝收据");
        assert!(
            refusal.contains("ACTION_BLOCKED_BY_READ_ONLY"),
            "必须证实是实际只读权限拒绝：{refusal}"
        );
    }
    for observation in h.model.observations.lock().unwrap().iter() {
        assert!(
            observation
                .declarations
                .iter()
                .any(|t| t.ends_with(":write")),
            "实际原生文件工具须已登记"
        );
        assert!(observation
            .declarations
            .iter()
            .all(|t| !["browser", "chrome", "channel", "dm"]
                .contains(&t.rsplit(':').next().unwrap())));
    }
    h.close(true).await;
}

#[tokio::test]
async fn actual_dm_chain_shares_three_run_budget_across_both_directions_and_cooldown() {
    let h = Harness::boot(None, Strategy::Ping).await;
    h.dm_settings().await;
    h.start();
    let source = h.send_dm("开始双向讨论").await;
    let first = h.settled(2).await;
    assert_eq!(first.len(), 2);
    assert_eq!(
        first
            .iter()
            .map(|d| d.input.actor_id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a"]
    );
    assert_eq!(h.ids().await.len(), 2, "同方向冷却不能被回发绕过");
    assert!(!store::pending_message_operations(h.state.storage())
        .await
        .unwrap()
        .is_empty());
    h.clock.advance(10_001);
    h.tick().await;
    let rows = h.settled(3).await;
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|d| d.input.actor_id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a", "b"]
    );
    assert!(rows
        .iter()
        .all(|d| d.status == store::PhoneDeliveryStatus::Completed
            && d.input.budget_limit == 3
            && d.input.root_chain_id == rows[0].input.root_chain_id));
    assert_eq!(
        rows[0].input.source_run_id.as_deref(),
        Some(source.as_str())
    );
    for row in rows.iter().skip(1) {
        let parent = row.input.source_run_id.as_deref().unwrap();
        assert!(rows.iter().any(|d| d.run_id.as_deref() == Some(parent)));
    }
    h.clock.advance(60_001);
    h.tick().await;
    assert_eq!(h.ids().await.len(), 3, "预算耗尽不能换方向新开根链");
    let a = h.get("/lingxi/v1/dm/b?agentId=a").await;
    let b = h.get("/lingxi/v1/dm/a?agentId=b").await;
    assert_eq!(a["messages"], b["messages"]);
    assert_eq!(a["messages"].as_array().unwrap().len(), 4);
    h.close(true).await;
}

#[tokio::test]
async fn actual_dm_stop_marker_stops_chain_and_new_user_action_waits_for_direction_cooldown() {
    let h = Harness::boot(None, Strategy::Stop).await;
    h.dm_settings().await;
    h.start();
    h.send_dm("第一条独立请求").await;
    let rows = h.settled(1).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].input.actor_id, "b");
    h.tick().await;
    assert_eq!(h.ids().await.len(), 1, "实际停止标记不触发反向手机轮次");
    h.send_dm("第二条独立请求").await;
    h.tick().await;
    assert_eq!(h.ids().await.len(), 1, "新动作写入不能绕过同方向冷却");
    h.clock.advance(10_001);
    h.tick().await;
    let rows = h.settled(2).await;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].input.root_chain_id, rows[1].input.root_chain_id);
    let a = h.get("/lingxi/v1/dm/b?agentId=a").await;
    assert_eq!(a["messages"].as_array().unwrap().len(), 4);
    assert!(rows
        .iter()
        .all(|d| d.decision_body.as_deref().unwrap().contains("<done>")));
    h.close(true).await;
}

fn local_stamp(now: u64) -> String {
    chrono::DateTime::from_timestamp_millis(i64::try_from(now).unwrap())
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[tokio::test]
async fn actual_legacy_markdown_first_discovery_external_append_and_restart_keep_one_cause() {
    let h = Harness::boot(None, Strategy::Pass).await;
    std::fs::create_dir_all(h.home.join("channels")).unwrap();
    let path = h.home.join("channels/ch_legacy.md");
    let original = format!(
        "---\nid: ch_legacy\nname: 旧频道\nmembers: [a, b]\nagentPhoneProactiveEnabled: false\ncustom: 保留旧字段\n---\n\n### user | {}\n\n旧文件中的第一条消息\n\n---\n",
        local_stamp(NOW)
    );
    // 直接写入真实旧格式文件，没有应用内写入回执。
    std::fs::write(&path, &original).unwrap();
    h.start();
    let first = h.settled(2).await;
    assert_eq!(first.len(), 2);
    assert!(first.iter().all(|d| d.input.cause_id.starts_with("legacy-")
        && d.input.input_json["prompt"]
            .as_str()
            .unwrap()
            .contains("旧文件中的第一条消息")));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    h.clock.advance(60_001);
    let now = h.clock.now_unix_ms();
    let appended = format!(
        "{original}\n### user | {}\n\n文件外部追加的新消息\n\n---\n",
        local_stamp(now)
    );
    std::fs::write(&path, &appended).unwrap();
    h.tick().await;
    let all = h.settled(4).await;
    assert_eq!(all.len(), 4);
    assert!(all[2..]
        .iter()
        .all(|d| d.input.cause_id != first[0].input.cause_id
            && d.input.input_json["prompt"]
                .as_str()
                .unwrap()
                .contains("文件外部追加的新消息")));
    h.tick().await;
    assert_eq!(h.ids().await.len(), 4);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), appended);
    let home = h.close(false).await;
    let h = Harness::boot(Some(home), Strategy::Pass).await;
    h.clock.set(now);
    h.start();
    h.tick().await;
    assert_eq!(h.ids().await.len(), 4, "持久发现游标不能在重启后重放旧文件");
    assert!(h.model.observations.lock().unwrap().is_empty());
    h.close(true).await;
}

#[tokio::test]
async fn actual_proactive_31_minutes_obeys_disable_enable_and_restart_slot_receipt() {
    let h = Harness::boot(None, Strategy::Pass).await;
    let id = h.channel(&["a", "b"]).await;
    h.post(
        &format!("/lingxi/v1/conversations/{id}/agent-phone-settings"),
        json!({"proactiveEnabled":true,"reminderIntervalMinutes":31}),
    )
    .await;
    h.start();
    h.message(&id, "proactive-seed", "最后一条需要关注的消息")
        .await;
    assert_eq!(h.settled(2).await.len(), 2);
    h.clock.advance(30 * 60_000);
    h.tick().await;
    assert_eq!(h.ids().await.len(), 2, "尚未满31分钟不能提前提醒");
    h.clock.advance(60_000);
    h.tick().await;
    let first_slot = h.settled(4).await;
    assert_eq!(first_slot.len(), 4);
    assert!(first_slot[2..]
        .iter()
        .all(|d| d.input.cause_id.starts_with("phone-proactive-")));
    h.post("/lingxi/v1/channels/toggle", json!({"enabled":false}))
        .await;
    h.clock.advance(31 * 60_000);
    h.tick().await;
    assert_eq!(h.ids().await.len(), 4, "全局关闭期间不产生主动运行");
    h.post("/lingxi/v1/channels/toggle", json!({"enabled":true}))
        .await;
    h.tick().await;
    let second_slot = h.settled(6).await;
    assert_eq!(second_slot.len(), 6);
    assert_ne!(second_slot[2].input.cause_id, second_slot[4].input.cause_id);
    h.tick().await;
    assert_eq!(h.ids().await.len(), 6, "同一主动时间槽只能处理一次");
    let now = h.clock.now_unix_ms();
    let home = h.close(false).await;
    let h = Harness::boot(Some(home), Strategy::Pass).await;
    h.clock.set(now);
    h.start();
    h.tick().await;
    assert_eq!(h.ids().await.len(), 6, "重启后仍尊重已处理的主动时间槽");
    assert!(h.model.observations.lock().unwrap().is_empty());
    h.close(true).await;
}

#[tokio::test]
async fn actual_phone_sessions_reuse_within_30_minutes_and_rotate_at_idle_expiry() {
    let h = Harness::boot(None, Strategy::Pass).await;
    let id = h.channel(&["a", "b"]).await;
    h.start();
    h.message(&id, "session-first", "第一次查看").await;
    let first = h.settled(2).await;
    let initial = first
        .iter()
        .map(|d| (d.input.actor_id.clone(), d.session_id.clone().unwrap()))
        .collect::<std::collections::BTreeMap<_, _>>();
    h.clock.advance(29 * 60_000 + 59_000);
    h.message(&id, "session-reuse", "仍在30分钟有效期内").await;
    let second = h.settled(4).await;
    assert_eq!(second.len(), 4);
    for d in &second[2..] {
        assert_eq!(d.session_id.as_ref(), initial.get(&d.input.actor_id));
    }
    let last_used = h.clock.now_unix_ms();
    for actor in ["a", "b"] {
        let binding = store::get_phone_binding(
            h.state.storage(),
            actor,
            &id,
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(binding.last_used, last_used);
        assert_eq!(binding.expires_at, last_used + 30 * 60_000);
    }
    h.clock.advance(30 * 60_000);
    h.message(&id, "session-expired", "上次使用已满30分钟")
        .await;
    let third = h.settled(6).await;
    assert_eq!(third.len(), 6);
    for d in &third[4..] {
        assert_ne!(
            d.session_id.as_ref(),
            initial.get(&d.input.actor_id),
            "到期应建立新的私人上下文"
        );
        assert_eq!(
            h.get(&format!(
                "/lingxi/v1/sessions/{}/history",
                d.session_id.as_deref().unwrap()
            ))
            .await["sessionId"],
            d.session_id.clone().unwrap()
        );
    }
    for sid in initial.values() {
        assert!(
            session_tree::get_session_row(h.state.storage(), sid)
                .await
                .unwrap()
                .is_some(),
            "过期不能删除原私人历史"
        );
    }
    h.close(true).await;
}

#[tokio::test]
async fn actual_same_second_dm_reset_hides_old_history_without_replaying_or_losing_new_action() {
    let h = Harness::boot(None, Strategy::Stop).await;
    h.dm_settings().await;
    h.start();
    h.send_dm("重置前已经处理的旧消息").await;
    let first = h.settled(1).await;
    let first_sid = first[0].session_id.clone().unwrap();
    h.post(
        "/lingxi/v1/dm/a/reset?agentId=b",
        json!({"requestId":"reset-once"}),
    )
    .await;
    assert!(h.get("/lingxi/v1/dm/a?agentId=b").await["messages"]
        .as_array()
        .unwrap()
        .is_empty());
    h.tick().await;
    assert_eq!(h.ids().await.len(), 1, "重置不能把已处理的投递重新执行");
    // 同一秒内的新动作必须以真实文件动作区分，不能只按秒级时间误删。
    h.send_dm("同秒重置后新动作仍须看见").await;
    h.tick().await;
    assert_eq!(h.ids().await.len(), 1, "仍需遵守方向冷却");
    h.clock.advance(10_001);
    h.tick().await;
    let rows = h.settled(2).await;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[1].session_id.as_deref(), Some(first_sid.as_str()));
    let prompt = rows[1].input.input_json["prompt"].as_str().unwrap();
    assert!(
        prompt.contains("同秒重置后新动作仍须看见"),
        "实际新投递必须保留同秒新动作：{prompt}"
    );
    assert!(
        !prompt.contains("重置前已经处理的旧消息"),
        "新手机上下文不能恢复被重置的旧边界"
    );
    let visible = h.get("/lingxi/v1/dm/a?agentId=b").await;
    let messages = visible["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "只保留真实新消息与其回复：{visible}");
    assert_eq!(messages[0]["body"], "同秒重置后新动作仍须看见");
    let binding = store::get_phone_binding(
        h.state.storage(),
        "b",
        "dm:a",
        lingxi_kernel::LOCAL_OWNER_SUBJECT,
    )
    .await
    .unwrap()
    .unwrap();
    h.post(
        "/lingxi/v1/dm/a/reset?agentId=b",
        json!({"requestId":"reset-once"}),
    )
    .await;
    assert_eq!(
        h.get("/lingxi/v1/dm/a?agentId=b").await["messages"],
        visible["messages"],
        "重投已完成的重置请求不能清空新消息"
    );
    let replayed = store::get_phone_binding(
        h.state.storage(),
        "b",
        "dm:a",
        lingxi_kernel::LOCAL_OWNER_SUBJECT,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_value(replayed).unwrap(),
        serde_json::to_value(binding).unwrap(),
        "重投不能清除或重建新会话关联"
    );
    h.tick().await;
    assert_eq!(h.ids().await.len(), 2);
    h.close(true).await;
}

#[tokio::test]
async fn actual_dm_stop_marker_variants_strip_public_text_and_never_start_reverse_run() {
    for marker in ["<done/>", "<DONE>", "<done />"] {
        let h = Harness::boot(None, Strategy::StopMarker(marker)).await;
        h.dm_settings().await;
        h.start();
        h.send_dm("检查真实结束标记").await;
        let rows = h.settled(1).await;
        assert_eq!(rows.len(), 1, "标记 {marker} 不能派生反向运行");
        assert_eq!(rows[0].status, store::PhoneDeliveryStatus::Completed);
        assert_eq!(rows[0].input.actor_id, "b");
        assert_eq!(
            rows[0].decision_body.as_deref(),
            Some(format!("标记清理后的正文 {marker}").as_str()),
            "原始决策保留模型实际内容"
        );
        let from = h.get("/lingxi/v1/dm/b?agentId=a").await;
        let to = h.get("/lingxi/v1/dm/a?agentId=b").await;
        assert_eq!(from["messages"], to["messages"], "双份实际文件必须一致");
        let messages = from["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2, "只应有原消息和一次公开回复：{marker}");
        assert_eq!(
            messages[1]["body"], "标记清理后的正文",
            "控制标记不能出现在实际发送正文"
        );
        h.clock.advance(60_001);
        h.tick().await;
        assert_eq!(
            h.ids().await.len(),
            1,
            "发现/恢复也不能重开已结束传播：{marker}"
        );
        h.close(true).await;
    }
}

#[tokio::test]
async fn actual_archived_phone_session_rotates_and_does_not_block_independent_channel() {
    let h = Harness::boot(None, Strategy::Pass).await;
    let id = h.channel(&["a", "b"]).await;
    let other = h
        .post(
            "/lingxi/v1/channels",
            json!({"name":"独立频道","members":["b","c"],"requestId":"create-independent-channel"}),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.post(
        &format!("/lingxi/v1/conversations/{other}/agent-phone-settings"),
        json!({"proactiveEnabled":false,"mode":"read_only"}),
    )
    .await;
    h.start();
    h.message(&id, "archive-before", "归档前先完成一次").await;
    let first = h.settled(2).await;
    let archived = first
        .iter()
        .find(|d| d.input.actor_id == "a")
        .unwrap()
        .session_id
        .clone()
        .unwrap();
    h.post("/lingxi/v1/sessions/archive", json!({"sessionId":archived}))
        .await;
    assert_eq!(
        session_tree::get_session_row(h.state.storage(), &archived)
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        "archived"
    );
    h.clock.advance(1000);
    h.message(&id, "archive-after", "归档后应使用新的有效上下文")
        .await;
    h.message(&other, "independent-after", "另一个频道也必须继续处理")
        .await;
    let all = h.settled(6).await;
    assert_eq!(all.len(), 6);
    assert!(
        all[2..]
            .iter()
            .all(|d| d.status == store::PhoneDeliveryStatus::Completed),
        "无效旧绑定不能阻塞后续投递：{all:?}"
    );
    let replacement = all[2..]
        .iter()
        .find(|d| d.input.conversation_id == id && d.input.actor_id == "a")
        .unwrap();
    assert_ne!(replacement.session_id.as_deref(), Some(archived.as_str()));
    assert_eq!(
        session_tree::get_session_row(
            h.state.storage(),
            replacement.session_id.as_deref().unwrap()
        )
        .await
        .unwrap()
        .unwrap()
        .lifecycle,
        "active"
    );
    let independent = all[2..]
        .iter()
        .filter(|d| d.input.conversation_id == other)
        .collect::<Vec<_>>();
    assert_eq!(independent.len(), 2);
    assert!(independent
        .iter()
        .all(|d| d.run_id.is_some() && d.decision == Some(store::PhoneDecision::Pass)));
    assert_eq!(
        session_tree::get_session_row(h.state.storage(), &archived)
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        "archived",
        "轮换不能偷偷恢复已归档会话"
    );
    h.close(true).await;
}

#[tokio::test]
async fn actual_completed_run_before_activity_restart_waits_full_interval_from_terminal_time() {
    let h = Harness::boot(None, Strategy::HoldPass).await;
    let id = h.channel(&["a", "b"]).await;
    h.post(
        &format!("/lingxi/v1/conversations/{id}/agent-phone-settings"),
        json!({"proactiveEnabled":true,"reminderIntervalMinutes":31}),
    )
    .await;
    h.message(&id, "completion-crash-window", "长任务结束后再重新计时")
        .await;
    for _ in 0..3 {
        h.tick().await;
    }
    h.observed(2).await;
    h.clock.advance(40 * 60_000);
    let completed_at = h.clock.now_unix_ms();
    h.model.release.add_permits(2);
    let deliveries = h.deliveries().await;
    for delivery in &deliveries {
        let actual = store::get_delivery_run(h.state.storage(), &delivery.input.delivery_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(h.run_terminal(&actual.0).await, RunStatus::Completed);
    }
    let activities = h
        .get(&format!("/lingxi/v1/conversations/{id}/agent-activities"))
        .await;
    assert!(
        activities["activities"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["state"] != "passed"),
        "模拟真实终态已落库、活动尚未补写的退出窗口"
    );
    let home = h.close(false).await;
    let h = Harness::boot(Some(home), Strategy::Pass).await;
    h.clock.set(completed_at);
    h.start();
    h.tick().await;
    assert_eq!(h.ids().await.len(), 2, "恢复不能把旧消息马上再执行");
    let status = h
        .get(&format!("/lingxi/v1/conversations/{id}/agent-activities"))
        .await;
    assert_eq!(
        status["ticker"]["nextReminder"]["dueAtMs"],
        json!(completed_at + 31 * 60_000)
    );
    h.clock.advance(30 * 60_000);
    h.tick().await;
    assert_eq!(h.ids().await.len(), 2, "必须从实际终态重新等满31分钟");
    h.clock.advance(60_000);
    h.tick().await;
    assert_eq!(h.settled(4).await.len(), 4);
    h.close(true).await;
}

#[tokio::test]
async fn actual_http_channel_management_and_dm_read_surfaces_preserve_real_state() {
    use lingxi_adapters::conversation_files::ConversationMutation;
    let h = Harness::boot(None, Strategy::Pass).await;
    let preferences = h.home.join("user/preferences.json");
    std::fs::write(&preferences,json!({"channels_enabled":true,"primary_agent_id":"a","current_agent_id":"c","userName":"真实用户名","custom":{"keep":true}}).to_string()).unwrap();
    let id = h.channel(&["a", "b"]).await;
    let channel_path = format!("/lingxi/v1/channels/{id}");
    let creation = h
        .post(
            "/lingxi/v1/channels",
            json!({"name":"真实手机频道","members":["a","b"],"requestId":"create-phone-channel"}),
        )
        .await;
    assert_eq!(
        creation,
        json!({"ok":true,"id":id,"name":"真实手机频道","members":["a","b"]})
    );
    h.state
        .conversation_files()
        .apply(
            h.state.storage(),
            "http-surface-legacy-meta",
            &ConversationMutation::PatchChannel {
                id: id.clone(),
                patch: json!({"legacyUnknown":"原样保留"}),
            },
            NOW,
        )
        .await
        .unwrap();
    let first = h
        .message(&id, "http-surface-message", "用户真实写入的消息")
        .await;
    let mut prefs: Value = serde_json::from_slice(&std::fs::read(&preferences).unwrap()).unwrap();
    prefs["userName"] = json!("后来修改的名字");
    std::fs::write(&preferences, prefs.to_string()).unwrap();
    assert_eq!(
        h.message(&id, "http-surface-message", "用户真实写入的消息")
            .await,
        first,
        "改名后重试仍沿用首次发送事实"
    );
    let detail = h.get(&channel_path).await;
    assert_eq!(detail["messages"].as_array().unwrap().len(), 1);
    assert_eq!(detail["messages"][0]["sender"], "真实用户名");
    let list = h.get("/lingxi/v1/channels").await;
    let row = &list["channels"].as_array().unwrap()[0];
    assert_eq!(row["id"], id);
    assert_eq!(row["messageCount"], 1);
    assert_eq!(row["newMessageCount"], 1);
    h.post(
        &format!("{channel_path}/read"),
        json!({"timestamp":first["timestamp"]}),
    )
    .await;
    let read = h.get("/lingxi/v1/channels").await;
    assert_eq!(read["channels"][0]["newMessageCount"], 0);
    assert_eq!(read["bookmarks"][&id], first["timestamp"]);
    let (bad_status, bad) = h
        .request(
            reqwest::Method::POST,
            &format!("{channel_path}/messages"),
            Some(json!({"body":"伪造","sender":"b"})),
        )
        .await;
    assert!(bad["error"].is_string());
    assert_eq!(bad_status, 400);
    h.post(&format!("{channel_path}/members"), json!({"memberId":"c"}))
        .await;
    assert_eq!(
        h.state
            .conversation_files()
            .read_bookmarks("c")
            .unwrap()
            .get(&id)
            .map(String::as_str),
        Some("never"),
        "加入成员须同步其真实书签"
    );
    assert_eq!(
        h.request(
            reqwest::Method::DELETE,
            &format!("{channel_path}/members/b"),
            None
        )
        .await
        .0,
        200
    );
    assert_eq!(h.get(&channel_path).await["members"], json!(["a", "c"]));
    assert!(
        !h.state
            .conversation_files()
            .read_bookmarks("b")
            .unwrap()
            .contains_key(&id),
        "移除成员须同步清理其书签"
    );
    assert_eq!(
        h.request(
            reqwest::Method::DELETE,
            &format!("{channel_path}/members/a"),
            None
        )
        .await
        .0,
        400,
        "不足两位时必须保留原成员"
    );
    assert_eq!(h.get(&channel_path).await["members"], json!(["a", "c"]));
    let same_name = h
        .post(
            "/lingxi/v1/channels",
            json!({"name":"真实手机频道","members":["a","b"],"requestId":"separate-friendly-name"}),
        )
        .await;
    assert_ne!(same_name["id"], id, "友好名称相同仍是不同频道");
    assert_eq!(h.request(reqwest::Method::POST,"/lingxi/v1/channels",Some(json!({"name":"变更同一动作的内容","members":["a","b"],"requestId":"separate-friendly-name"}))).await.0,409);
    assert_eq!(
        h.request(
            reqwest::Method::POST,
            "/lingxi/v1/channels",
            Some(json!({"name":"人数不足","members":["a"]}))
        )
        .await
        .0,
        400
    );
    let settings = format!("/lingxi/v1/conversations/{id}/agent-phone-settings");
    h.post(&settings, json!({"replyMaxChars":99})).await;
    h.post(
        &format!("/lingxi/v1/conversations/{id}/agent-phone-tool-mode"),
        json!({"mode":"write"}),
    )
    .await;
    assert_eq!(
        h.get(&format!(
            "/lingxi/v1/conversations/{id}/agent-phone-tool-mode"
        ))
        .await["mode"],
        "write"
    );
    let configured = h.get(&settings).await;
    assert_eq!(configured["mode"], "write");
    assert_eq!(configured["replyMaxChars"], 99);
    assert_eq!(configured["proactiveEnabled"], false);
    assert_eq!(
        h.state.conversation_files().read_channel(&id).unwrap().meta["legacyUnknown"],
        "原样保留"
    );
    assert_eq!(
        h.request(reqwest::Method::GET, "/lingxi/v1/dm/b?agentId=z", None)
            .await
            .0,
        404
    );
    assert_eq!(
        h.request(reqwest::Method::GET, "/lingxi/v1/dm/b", None)
            .await
            .0,
        404
    );
    // 列表读取真实双份私信文件，缺省使用主助手a，不能误用当前助手c。
    for (peer, stamp, body) in [
        ("b", local_stamp(NOW), "较旧的私信"),
        ("c", local_stamp(NOW + 60_000), "最新的私信"),
    ] {
        h.state
            .conversation_files()
            .apply(
                h.state.storage(),
                &format!("http-surface-dm-{peer}"),
                &ConversationMutation::AppendDm {
                    from: "a".into(),
                    to: peer.into(),
                    body: body.into(),
                    timestamp: stamp,
                },
                NOW,
            )
            .await
            .unwrap();
    }
    h.state
        .conversation_files()
        .apply(
            h.state.storage(),
            "http-surface-phone-unknown",
            &ConversationMutation::PhonePatch {
                actor: "a".into(),
                conversation: "dm:b".into(),
                conversation_type: "dm".into(),
                patch: json!({"legacyUnknown":"私信保留"}),
                timestamp: local_stamp(NOW),
            },
            NOW,
        )
        .await
        .unwrap();
    let dm_settings = "/lingxi/v1/conversations/dm:b/agent-phone-settings";
    h.post(
        dm_settings,
        json!({"replyMaxChars":88,"proactiveEnabled":false}),
    )
    .await;
    h.post(
        "/lingxi/v1/conversations/dm:b/agent-phone-tool-mode",
        json!({"mode":"write"}),
    )
    .await;
    assert_eq!(
        h.get("/lingxi/v1/conversations/dm:b/agent-phone-tool-mode")
            .await["mode"],
        "write"
    );
    let configured_dm = h.get(dm_settings).await;
    assert_eq!(configured_dm["mode"], "write");
    assert_eq!(configured_dm["replyMaxChars"], 88);
    assert_eq!(
        h.state
            .conversation_files()
            .read_phone("a", "dm:b")
            .unwrap()
            .meta["legacyUnknown"],
        "私信保留"
    );
    let dms = h.get("/lingxi/v1/dm").await;
    assert_eq!(dms["ownerAgentId"], "a");
    assert_eq!(dms["dms"].as_array().unwrap().len(), 2);
    assert_eq!(dms["dms"][0]["peerId"], "c");
    assert_eq!(dms["dms"][0]["lastMessage"], "最新的私信");
    assert_eq!(
        h.get("/lingxi/v1/dm/b").await["messages"][0]["body"],
        "较旧的私信"
    );
    assert_eq!(
        h.get("/lingxi/v1/dm/a?agentId=b").await["messages"][0]["body"],
        "较旧的私信"
    );
    h.post(
        "/lingxi/v1/dm/b/reset",
        json!({"requestId":"reset-http-export"}),
    )
    .await;
    assert!(h.get("/lingxi/v1/dm/b").await["messages"]
        .as_array()
        .unwrap()
        .is_empty());
    let export = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/conversations/dm:b/export", h.url))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();
    assert_eq!(export.status(), 200);
    assert_eq!(export.headers()["cache-control"], "private, no-store");
    assert!(export.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains("attachment"));
    assert!(
        export.text().await.unwrap().contains("较旧的私信"),
        "重置只改变可见范围，原始导出保留全部历史"
    );
    h.post("/lingxi/v1/channels/toggle", json!({"enabled":false}))
        .await;
    assert_eq!(
        h.request(reqwest::Method::GET, "/lingxi/v1/channels", None)
            .await
            .0,
        503
    );
    h.post("/lingxi/v1/channels/toggle", json!({"enabled":true}))
        .await;
    let channel_export = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/conversations/{id}/export", h.url))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();
    assert_eq!(channel_export.status(), 200);
    assert!(channel_export
        .text()
        .await
        .unwrap()
        .contains("用户真实写入的消息"));
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(preferences).unwrap()).unwrap()["custom"],
        json!({"keep":true})
    );
    assert_eq!(
        h.request(reqwest::Method::DELETE, &channel_path, None)
            .await
            .0,
        200
    );
    assert_eq!(
        h.request(reqwest::Method::GET, &channel_path, None).await.0,
        404
    );
    assert_eq!(
        h.request(reqwest::Method::DELETE, &channel_path, None)
            .await
            .0,
        404
    );
    assert!(!h
        .state
        .conversation_files()
        .list_channels()
        .unwrap()
        .iter()
        .any(|d| d.meta["id"] == id));
    h.close(true).await;
}

#[tokio::test]
async fn actual_channel_recursive_replies_share_one_guard_budget_and_restart_does_not_reopen_it() {
    let h = Harness::boot(None, Strategy::Ping).await;
    let id = h.channel(&["a", "b"]).await;
    h.start();
    h.message(&id, "channel-loop-budget", "开始有限频道讨论")
        .await;
    let rows = h.settled(4).await;
    assert_eq!(rows.len(), 4, "频道全部实际接收者共用配置的四轮预算");
    assert!(rows.iter().all(|d| d.input.budget_limit == 4
        && d.input.root_chain_id == rows[0].input.root_chain_id
        && d.run_id.is_some()));
    h.clock.advance(60_001);
    h.tick().await;
    assert_eq!(
        h.ids().await.len(),
        4,
        "耗尽的真实因果链不能从公开回复重开根"
    );
    let now = h.clock.now_unix_ms();
    let home = h.close(false).await;
    let h = Harness::boot(Some(home), Strategy::Ping).await;
    h.clock.set(now);
    h.start();
    h.tick().await;
    assert_eq!(h.ids().await.len(), 4, "重启后根预算及派发回执仍有效");
    assert!(h.model.observations.lock().unwrap().is_empty());
    h.close(true).await;
}
