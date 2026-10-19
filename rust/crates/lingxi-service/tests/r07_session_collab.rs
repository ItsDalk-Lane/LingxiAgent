//! 只替换模型服务；确认卡、HTTP、会话创建、排队与因果Run使用正式实现。
use lingxi_adapters::storage::{conversations_collab as store, session_tree};
use lingxi_kernel::{
    model_exchange::ModelTurnInput,
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolRequest,
        TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
use lingxi_service::{
    approval_service::ApprovalService, inject::ManualClock, prepare_layout, session_collab,
    toolgateway::ToolInvocationGateway, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

struct Model {
    releases: tokio::sync::Semaphore,
    inputs: Mutex<Vec<String>>,
    priors: Mutex<Vec<(String, String)>>,
    db: OnceLock<Arc<lingxi_adapters::storage::RunDatabase>>,
    phone_target: Mutex<Option<String>>,
    hold_phone: AtomicBool,
}
impl TurnProviderPort for Model {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "fixture".into(),
            model: "collab".into(),
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
        self.inputs.lock().unwrap().push(input.submission.clone());
        self.priors
            .lock()
            .unwrap()
            .push((input.submission.clone(), format!("{:?}", input.prior)));
        Box::pin(async move {
            if input.submission == "park-target" {
                self.releases.acquire().await.unwrap().forget();
            }
            if input.turn == 1 {
                let phone_target = self.phone_target.lock().unwrap().clone();
                if let Some(target) = phone_target {
                    let db = self.db.get().expect("正式数据库已连接");
                    let lineage = db.load_run_lineage(&ctx.run_id).await.unwrap().unwrap();
                    let actor = session_tree::get_session_row(db, ctx.session_id.as_str())
                        .await
                        .unwrap()
                        .unwrap()
                        .agent_id;
                    if actor == "agent"
                        && matches!(
                            lineage.origin,
                            lingxi_kernel::subagent::RunOrigin::Channel
                                | lingxi_kernel::subagent::RunOrigin::Dm
                        )
                    {
                        if self.hold_phone.load(Ordering::SeqCst) {
                            self.releases.acquire().await.unwrap().forget();
                        }
                        return ProviderTurnResult::of_ctx(ctx,ProviderTurn::ToolRequests{requests:vec![ToolRequest::from_effective_arguments("tool:first-party:session",json!({"action":"send","sessionId":target,"message":"手机来源的待确认协作正文"}),&SchemaBudget::default()).unwrap()],content:vec![]});
                    }
                }
                if let Ok(command) = serde_json::from_str::<Value>(&input.submission) {
                    if command.get("collab").is_some() {
                        return ProviderTurnResult::of_ctx(
                            ctx,
                            ProviderTurn::ToolRequests {
                                requests: vec![ToolRequest::from_effective_arguments(
                                    "tool:first-party:session",
                                    command["collab"].clone(),
                                    &SchemaBudget::default(),
                                )
                                .unwrap()],
                                content: vec![],
                            },
                        );
                    }
                }
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: "真实受理后的模型答复".into(),
                        }],
                        model_call_id: Some(call.clone()),
                    },
                },
            )
        })
    }
}
struct ReleaseModel(Arc<Model>);
impl Drop for ReleaseModel {
    fn drop(&mut self) {
        self.0.releases.add_permits(32);
    }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    model: Arc<Model>,
    clock: Arc<ManualClock>,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot(home: Option<PathBuf>) -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("lingxi_service=debug")
            .with_test_writer()
            .try_init();
        let home = home.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "r07-collab-service-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        for agent in ["agent", "other"] {
            std::fs::create_dir_all(home.join(format!("agents/{agent}"))).unwrap();
            std::fs::write(
                home.join(format!("agents/{agent}/config.yaml")),
                format!("agent:\n  name: {agent}\nmemory:\n  enabled: false\n"),
            )
            .unwrap();
        }
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let model = Arc::new(Model {
            releases: tokio::sync::Semaphore::new(0),
            inputs: Mutex::new(vec![]),
            priors: Mutex::new(vec![]),
            db: OnceLock::new(),
            phone_target: Mutex::new(None),
            hold_phone: AtomicBool::new(false),
        });
        let clock = Arc::new(ManualClock::new(1_800_000_000_000));
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
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                clock: clock.clone(),
                turn_provider: Some(model.clone()),
                tool_gateway: Some(gateway),
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
            model,
            clock,
            url,
            token,
            stop,
            server,
        }
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(&self.token);
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
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.request(reqwest::Method::POST, path, Some(body)).await
    }
    async fn create(&self, id: &str, agent: &str, mode: Option<&str>) {
        let (status, body) = self
            .post(
                "/lingxi/v1/sessions",
                json!({"sessionId":id,"agentId":agent,"title":id,"permissionMode":mode}),
            )
            .await;
        assert_eq!(status, 201, "{body}");
    }
    async fn execute(&self, sid: &str, input: &str) -> String {
        let (status, body) = self
            .post(
                &format!("/lingxi/v1/sessions/{sid}/execute"),
                json!({"input":input}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        body["runId"].as_str().unwrap().into()
    }
    async fn park(&self, sid: &str) -> (String, tokio::task::JoinHandle<()>) {
        let (url, token) = (
            format!("{}/lingxi/v1/sessions/{sid}/execute", self.url),
            self.token.clone(),
        );
        let request = tokio::spawn(async move {
            let response = reqwest::Client::new()
                .post(url)
                .bearer_auth(token)
                .json(&json!({"input":"park-target"}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
        });
        let id=tokio::time::timeout(Duration::from_secs(5),async {
            loop {
                if let Some(id)=self.state.storage().query_one_text("SELECT run_id FROM runs WHERE session_id=?1 AND status='running' ORDER BY created_at_unix_ms DESC LIMIT 1",vec![sid.into()]).await.unwrap(){return id;}
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("模型真实停在运行中");
        (id, request)
    }
    async fn wait(&self, id: &str) {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let status = self
                    .state
                    .storage()
                    .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![id.into()])
                    .await
                    .unwrap();
                if let Some(s) = status {
                    if !["running", "queued", "waiting_approval", "cancelling"]
                        .contains(&s.as_str())
                    {
                        assert_eq!(s, "completed");
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "真实Run终结: {error:?}; live={:?}; exits={:?}; inputs={:?}",
                self.state.background().live_ids(),
                self.state.background().recent_exits(),
                self.model.inputs.lock().unwrap()
            )
        });
    }
    async fn draft(&self, args: Value) -> store::CollabDraft {
        let run = self
            .execute("source", &json!({"collab":args}).to_string())
            .await;
        self.wait(&run).await;
        let rows = store::list_collab_drafts_for_session(self.state.storage(), "source")
            .await
            .unwrap();
        rows.into_iter()
            .find(|d| d.input.source_run_id == run)
            .expect("正式工具生成持久草稿")
    }
    async fn apply(&self, id: &str, edited: Option<Value>) -> (u16, Value) {
        self.post(
            "/lingxi/v1/session-collab/apply",
            json!({"suggestionId":id,"draft":edited}),
        )
        .await
    }
    async fn close(self, delete: bool) -> PathBuf {
        self.model.releases.add_permits(32);
        self.state
            .background()
            .drain_within(Duration::from_secs(5))
            .await;
        self.stop.send(()).unwrap();
        self.server.await.unwrap();
        self.state.storage().close().await.unwrap();
        let home = self.home.clone();
        drop(self.state);
        if delete {
            std::fs::remove_dir_all(&home).unwrap();
        }
        home
    }
}
async fn count(h: &Harness, table: &str) -> u64 {
    h.state
        .storage()
        .query_one_text(
            &format!("SELECT CAST(COUNT(*) AS TEXT) FROM {table}"),
            vec![],
        )
        .await
        .unwrap()
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registered_tool_only_cards_then_busy_queue_accepts_one_causal_run_and_records_edits() {
    let h = Harness::boot(None).await;
    let initial_sessions = count(&h, "sessions").await;
    h.create("source", "agent", None).await;
    h.create("target", "other", None).await;
    let _release = ReleaseModel(h.model.clone());
    let (parked, parked_request) = h.park("target").await;
    let d = h
        .draft(json!({"action":"send","sessionId":"target","message":"原始正文"}))
        .await;
    assert_eq!(count(&h, "runs").await, 2);
    assert_eq!(count(&h, "sessions").await, initial_sessions + 2);
    assert_eq!(d.delivery_status, store::CollabDeliveryStatus::None);
    let edited = json!({"targetSessionId":"target","message":"用户确认的编辑正文"});
    let (a, b) = tokio::join!(
        h.apply(&d.input.id, Some(edited.clone())),
        h.apply(&d.input.id, Some(edited.clone()))
    );
    assert_eq!(a.0, 200, "{:?}", a);
    assert_eq!(b.0, 200, "{:?}", b);
    assert_eq!(a.1["result"]["deliveryStatus"], "queued");
    assert_eq!(a.1["result"]["firstMessageAccepted"], false);
    session_collab::tick(&h.state).await.unwrap();
    let pending = store::get_collab_draft(h.state.storage(), &d.input.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.delivery_status, store::CollabDeliveryStatus::Queued);
    assert_eq!(count(&h, "runs").await, 2);
    h.model.releases.add_permits(1);
    h.wait(&parked).await;
    parked_request.await.unwrap();
    session_collab::tick(&h.state).await.unwrap();
    let done = store::get_collab_draft(h.state.storage(), &d.input.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(done.delivery_status, store::CollabDeliveryStatus::Accepted);
    let accepted = done.accepted_run_id.as_deref().unwrap();
    h.wait(accepted).await;
    let actual = store::get_collab_run(h.state.storage(), &d.input.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual.0, accepted);
    assert_eq!(actual.1, "target");
    assert!(h
        .model
        .inputs
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.contains("来自助手 agent") && s.contains("用户确认的编辑正文")));
    assert_eq!(h.apply(&d.input.id, Some(edited.clone())).await.0, 200);
    assert_eq!(
        h.apply(
            &d.input.id,
            Some(json!({"targetSessionId":"target","message":"偷换正文"}))
        )
        .await
        .0,
        409
    );
    session_collab::tick(&h.state).await.unwrap();
    assert_eq!(count(&h, "runs").await, 3);
    let followup = h.execute("target", "后续问题读取协作上下文").await;
    h.wait(&followup).await;
    let priors = h.model.priors.lock().unwrap().clone();
    let prior = priors
        .iter()
        .find(|(input, _)| input == "后续问题读取协作上下文")
        .unwrap();
    assert!(
        prior.1.contains("用户确认的编辑正文")
            && prior.1.contains("来自助手 agent")
            && prior.1.contains("真实受理后的模型答复"),
        "后台真实输入及答复必须进入后续上下文: {prior:?}"
    );
    assert_eq!(count(&h, "runs").await, 4);
    let facts = store::collab_decisions_for_session(h.state.storage(), "source")
        .await
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0]["frozenApply"], edited);
    assert_eq!(facts[0]["result"]["deliveryStatus"], "accepted");
    let (status, history) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/sessions/source/history",
            None,
        )
        .await;
    assert_eq!(status, 200, "{history}");
    let text = history.to_string();
    assert!(
        text.contains("accepted") && text.contains("用户确认的编辑正文"),
        "历史投影保留宿主真实确认事实: {history}"
    );
    h.close(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_apply_is_single_target_and_survives_restart_without_replay() {
    let h = Harness::boot(None).await;
    let initial_sessions = count(&h, "sessions").await;
    h.create("source", "agent", None).await;
    let d = h
        .draft(json!({"action":"create","agent":"other","title":"原始标题","message":"首条正文"}))
        .await;
    assert_eq!(count(&h, "sessions").await, initial_sessions + 1);
    let edited =
        json!({"agentId":"other","model":null,"title":"确认标题","firstMessage":"确认首条"});
    let (a, b) = tokio::join!(
        h.apply(&d.input.id, Some(edited.clone())),
        h.apply(&d.input.id, Some(edited.clone()))
    );
    assert_eq!(a.0, 200, "{a:?}");
    assert_eq!(b.0, 200, "{b:?}");
    assert_eq!(count(&h, "sessions").await, initial_sessions + 1);
    session_collab::tick(&h.state).await.unwrap();
    let done = store::get_collab_draft(h.state.storage(), &d.input.id)
        .await
        .unwrap()
        .unwrap();
    let sid = done.target_session_id.clone().unwrap();
    assert!(done.target_created);
    h.wait(done.accepted_run_id.as_deref().unwrap()).await;
    let row = session_tree::get_session_row(h.state.storage(), &sid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.agent_id, "other");
    assert_eq!(row.title, "确认标题");
    assert_eq!(count(&h, "sessions").await, initial_sessions + 2);
    let home = h.close(false).await;
    let h = Harness::boot(Some(home)).await;
    session_collab::recover(&h.state).await.unwrap();
    assert_eq!(h.apply(&d.input.id, Some(edited)).await.0, 200);
    session_collab::tick(&h.state).await.unwrap();
    assert_eq!(count(&h, "sessions").await, initial_sessions + 2);
    assert_eq!(count(&h, "runs").await, 2);
    assert_eq!(
        store::get_collab_draft(h.state.storage(), &d.input.id)
            .await
            .unwrap()
            .unwrap()
            .target_session_id
            .as_deref(),
        Some(sid.as_str())
    );
    h.close(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_expiry_readonly_and_spoofed_source_are_checked_by_real_records() {
    let h = Harness::boot(None).await;
    h.create("source", "agent", None).await;
    h.create("target", "other", None).await;
    let d = h
        .draft(json!({"action":"send","sessionId":"target","message":"待拒绝"}))
        .await;
    let (status, _) = h
        .post(
            "/lingxi/v1/session-collab/reject",
            json!({"suggestionId":d.input.id,"sourceSessionId":"target"}),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(
        h.post(
            "/lingxi/v1/session-collab/apply",
            json!({"suggestionId":d.input.id,"sourceSessionId":"target"})
        )
        .await
        .0,
        400
    );
    assert_eq!(
        h.post(
            "/lingxi/v1/session-collab/reject",
            json!({"suggestionId":d.input.id,"sourceSessionId":"source"})
        )
        .await
        .0,
        200
    );
    assert_eq!(h.apply(&d.input.id, None).await.0, 404);
    let expired = h
        .draft(json!({"action":"send","sessionId":"target","message":"将过期"}))
        .await;
    h.clock.advance(8 * 24 * 60 * 60 * 1000);
    assert_eq!(h.apply(&expired.input.id, None).await.0, 404);
    h.create("readonly", "agent", Some("read_only")).await;
    let run = h
        .execute(
            "readonly",
            &json!({"collab":{"action":"send","sessionId":"target","message":"只读不能产卡"}})
                .to_string(),
        )
        .await;
    h.wait(&run).await;
    assert!(
        store::list_collab_drafts_for_session(h.state.storage(), "readonly")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(count(&h, "runs").await, 3);
    h.close(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unconfirmed_started_create_recovers_existing_target_but_never_resends() {
    let h = Harness::boot(None).await;
    let initial_sessions = count(&h, "sessions").await;
    h.create("source", "agent", None).await;
    let d = h
        .draft(json!({"action":"create","agent":"other","message":"不可盲重投"}))
        .await;
    h.apply(&d.input.id, None).await;
    let lease = store::claim_collab_delivery(h.state.storage(), &d.input.id, 1_800_000_000_000)
        .await
        .unwrap()
        .draft;
    let sid = lease.reserved_target_id.unwrap();
    h.state
        .session_admin()
        .create_session(
            &sid,
            "other",
            "user_local",
            "已创建尚未绑定",
            None,
            &[],
            true,
            1_800_000_000_000,
        )
        .await
        .unwrap();
    let home = h.close(false).await;
    let h = Harness::boot(Some(home)).await;
    session_collab::recover(&h.state).await.unwrap();
    let recovered = store::get_collab_draft(h.state.storage(), &d.input.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.delivery_status,
        store::CollabDeliveryStatus::Unknown
    );
    assert_eq!(recovered.target_session_id.as_deref(), Some(sid.as_str()));
    assert!(recovered.target_created);
    assert_eq!(h.apply(&d.input.id, None).await.0, 409);
    session_collab::tick(&h.state).await.unwrap();
    assert_eq!(count(&h, "sessions").await, initial_sessions + 2);
    assert_eq!(count(&h, "runs").await, 1);
    h.close(true).await;
}

async fn phone_queued_draft_cannot_outlive_authority(revocation: Option<&str>) {
    let h = Harness::boot(None).await;
    let _release = ReleaseModel(h.model.clone());
    for id in ["third", "targetagent"] {
        let (status, result) = h
            .post("/lingxi/v1/agents", json!({"id":id,"name":id}))
            .await;
        assert_eq!(status, 200, "{result}");
    }
    h.create("target", "targetagent", None).await;
    let (parked, parked_request) = h.park("target").await;
    *h.model.phone_target.lock().unwrap() = Some("target".into());
    let (status, result) = h
        .post(
            "/lingxi/v1/channels/toggle?agentId=agent",
            json!({"enabled":true}),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let(status,result)=h.post("/lingxi/v1/channels?agentId=agent",json!({"name":"手机协作撤权实测","members":["agent","other","third"],"requestId":"collab-phone-create"})).await;
    assert_eq!(status, 200, "{result}");
    let channel = result["id"].as_str().unwrap();
    let (status, result) = h
        .post(
            &format!("/lingxi/v1/conversations/{channel}/agent-phone-settings?agentId=agent"),
            json!({"mode":"write","proactiveEnabled":false,"guardLimit":4}),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let (status, result) = h
        .post(
            &format!("/lingxi/v1/channels/{channel}/messages?agentId=agent"),
            json!({"body":"请助手提出会话协作建议","requestId":"collab-phone-inbound"}),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let draft = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            h.state.conversations().tick(&h.state).await.unwrap();
            if let Some(id) = h
                .state
                .storage()
                .query_one_text(
                    "SELECT id FROM session_collab_drafts_v1 ORDER BY created_at LIMIT 1",
                    vec![],
                )
                .await
                .unwrap()
            {
                break store::get_collab_draft(h.state.storage(), &id)
                    .await
                    .unwrap()
                    .unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("真实Phone Run必须经正式session工具产生确认卡");
    let lineage = h
        .state
        .storage()
        .load_run_lineage(&lingxi_protocol::RunId::new(&draft.input.source_run_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lineage.origin, lingxi_kernel::subagent::RunOrigin::Channel);
    let root = lingxi_adapters::storage::conversations::get_delivery_run(
        h.state.storage(),
        lineage.cause_id.as_deref().unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(root.0, draft.input.source_run_id);
    h.wait(&root.0).await;
    assert_eq!(h.apply(&draft.input.id, None).await.0, 200);
    session_collab::tick(&h.state).await.unwrap();
    assert_eq!(
        store::get_collab_draft(h.state.storage(), &draft.input.id)
            .await
            .unwrap()
            .unwrap()
            .delivery_status,
        store::CollabDeliveryStatus::Queued,
        "目标忙时只能持久排队"
    );
    // 先等全部真实手机受理完成，之后只统计撤权带来的新运行。
    tokio::time::timeout(Duration::from_secs(8),async{
        loop {
            let active=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE l.origin IN ('channel','dm') AND r.status IN ('running','queued','waiting_approval','cancelling')",vec![]).await.unwrap().unwrap();
            if active=="0"{break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("全部手机来源真实终结");
    let before = count(&h, "runs").await;
    if let Some(revocation) = revocation {
        let (status, result) = if revocation == "member" {
            h.request(
                reqwest::Method::DELETE,
                &format!("/lingxi/v1/channels/{channel}/members/agent?agentId=other"),
                Some(json!({})),
            )
            .await
        } else if revocation == "read_only" {
            h.post(
                &format!("/lingxi/v1/conversations/{channel}/agent-phone-tool-mode?agentId=other"),
                json!({"mode":"read_only"}),
            )
            .await
        } else {
            h.post(
                "/lingxi/v1/channels/toggle?agentId=other",
                json!({"enabled":false}),
            )
            .await
        };
        assert_eq!(status, 200, "{result}");
    }
    h.model.releases.add_permits(1);
    h.wait(&parked).await;
    parked_request.await.unwrap();
    session_collab::tick(&h.state).await.unwrap();
    let failed = store::get_collab_draft(h.state.storage(), &draft.input.id)
        .await
        .unwrap()
        .unwrap();
    if revocation.is_none() {
        assert_eq!(
            failed.delivery_status,
            store::CollabDeliveryStatus::Accepted,
            "有效手机权限不能被整体封禁"
        );
        let accepted = failed.accepted_run_id.as_deref().unwrap();
        h.wait(accepted).await;
        assert_eq!(count(&h, "runs").await, before + 1);
        let inherited = h
            .state
            .storage()
            .load_run_lineage(&lingxi_protocol::RunId::new(accepted))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            inherited.origin,
            lingxi_kernel::subagent::RunOrigin::SessionCollab
        );
        assert_eq!(
            inherited.parent_run_id.as_ref().map(|id| id.as_str()),
            Some(draft.input.source_run_id.as_str())
        );
        h.close(true).await;
        return;
    }
    assert_eq!(failed.delivery_status, store::CollabDeliveryStatus::Failed);
    assert!(failed.accepted_run_id.is_none());
    assert!(store::get_collab_run(h.state.storage(), &draft.input.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        count(&h, "runs").await,
        before,
        "来源撤权后的queued协作不得创建任何Run"
    );
    assert_eq!(
        h.apply(&draft.input.id, None).await.0,
        403,
        "重复确认不能绕过已经撤销的手机权限"
    );
    assert_eq!(count(&h, "runs").await, before);
    h.close(true).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_phone_member_blocks_a_previously_confirmed_busy_collaboration_queue() {
    phone_queued_draft_cannot_outlive_authority(Some("member")).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabling_channels_blocks_a_previously_confirmed_busy_collaboration_queue() {
    phone_queued_draft_cannot_outlive_authority(Some("disabled")).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn current_phone_authority_still_allows_queued_collaboration_after_root_completed() {
    phone_queued_draft_cannot_outlive_authority(None).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phone_ticker_distinguishes_claims_actual_runs_and_the_persisted_reminder_slot() {
    use lingxi_adapters::storage::conversation_discovery;
    let h = Harness::boot(None).await;
    let _release = ReleaseModel(h.model.clone());
    h.create("target", "other", None).await;
    *h.model.phone_target.lock().unwrap() = Some("target".into());
    h.model.hold_phone.store(true, Ordering::SeqCst);
    assert_eq!(
        h.post(
            "/lingxi/v1/channels/toggle?agentId=agent",
            json!({"enabled":true})
        )
        .await
        .0,
        200
    );
    let (status, result) = h
        .post(
            "/lingxi/v1/channels?agentId=agent",
            json!({"name":"真实手机状态","members":["agent","other"],"requestId":"status-channel"}),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let channel = result["id"].as_str().unwrap();
    let settings = format!("/lingxi/v1/conversations/{channel}/agent-phone-settings?agentId=agent");
    assert_eq!(
        h.post(
            &settings,
            json!({"mode":"write","proactiveEnabled":false,"guardLimit":4})
        )
        .await
        .0,
        200
    );
    assert_eq!(
        h.post(
            &format!("/lingxi/v1/channels/{channel}/messages?agentId=agent"),
            json!({"body":"状态来源消息","requestId":"status-message"})
        )
        .await
        .0,
        200
    );
    let activities = format!("/lingxi/v1/conversations/{channel}/agent-activities?agentId=agent");
    h.state.conversations().tick(&h.state).await.unwrap();
    let (status, result) = h.request(reqwest::Method::GET, &activities, None).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["ticker"]["queued"], true);
    assert_eq!(result["ticker"]["active"], Value::Null);
    assert_eq!(result["ticker"]["running"], false);
    assert_eq!(count(&h, "runs").await, 0, "领取不得冒充真实运行");
    h.state.conversations().tick(&h.state).await.unwrap();
    tokio::time::timeout(Duration::from_secs(8),async{loop{
        let other=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM runs r JOIN sessions s ON s.session_id=r.session_id WHERE s.agent_id!='agent' AND r.status IN ('running','queued','waiting_approval','cancelling')",vec![]).await.unwrap().unwrap();
        if other=="0"{break;}
        tokio::time::sleep(Duration::from_millis(10)).await;
    }}).await.expect("另一位接收者真实完成后再读取当前执行者");
    let (status, result) = h.request(reqwest::Method::GET, &activities, None).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["ticker"]["running"], true);
    assert_eq!(result["ticker"]["active"]["agentId"], "agent");
    assert_eq!(result["ticker"]["active"]["agentCount"], 2);
    assert_eq!(result["ticker"]["active"]["delivered"], 2);
    assert_eq!(result["ticker"]["active"]["checks"], 2);
    assert_eq!(result["ticker"]["active"]["maxChecks"], 4);
    assert_eq!(result["ticker"]["active"]["mode"], "delivery");
    let completed_delay = 5 * 60_000u64;
    h.clock.advance(completed_delay);
    h.model.hold_phone.store(false, Ordering::SeqCst);
    h.model.releases.add_permits(1);
    tokio::time::timeout(Duration::from_secs(8),async{loop{let count=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE status IN ('running','queued','waiting_approval','cancelling')",vec![]).await.unwrap().unwrap();if count=="0"{break;}tokio::time::sleep(Duration::from_millis(10)).await;}}).await.unwrap();
    h.state.conversations().tick(&h.state).await.unwrap();
    assert_eq!(
        h.post(&settings, json!({"proactiveEnabled":true})).await.0,
        200
    );
    let (status, result) = h.request(reqwest::Method::GET, &activities, None).await;
    assert_eq!(status, 200, "{result}");
    let interval = 31 * 60_000u64;
    let initial = 1_800_000_000_000u64;
    assert_eq!(
        result["ticker"]["nextReminder"]["dueAtMs"],
        initial + completed_delay + interval
    );
    assert_eq!(result["ticker"]["nextReminder"]["intervalMs"], interval);
    assert_eq!(result["ticker"]["active"], Value::Null);
    assert_eq!(result["ticker"]["running"], false);
    h.clock.advance(interval + 60_000);
    h.state.conversations().tick(&h.state).await.unwrap();
    let cursor = conversation_discovery::cursor(h.state.storage(), channel)
        .await
        .unwrap()
        .unwrap();
    let now = initial + completed_delay + interval + 60_000;
    assert_eq!(cursor.proactive_slot, now / interval);
    let (status, result) = h.request(reqwest::Method::GET, &activities, None).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(
        result["ticker"]["nextReminder"],
        Value::Null,
        "未完成轮次不能预报尚未确定的下次提醒"
    );
    assert_eq!(result["ticker"]["queued"], true);
    assert_eq!(result["ticker"]["active"], Value::Null);
    h.state.conversations().tick(&h.state).await.unwrap();
    tokio::time::timeout(Duration::from_secs(8),async{loop{
        let live=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE status IN ('running','queued','waiting_approval','cancelling')",vec![]).await.unwrap().unwrap();
        if live=="0"{break;}
        tokio::time::sleep(Duration::from_millis(10)).await;
    }}).await.unwrap();
    h.state.conversations().tick(&h.state).await.unwrap();
    tokio::time::timeout(Duration::from_secs(8),async{loop{
        let other=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM runs r JOIN sessions s ON s.session_id=r.session_id WHERE s.agent_id!='agent' AND r.status IN ('running','queued','waiting_approval','cancelling')",vec![]).await.unwrap().unwrap();
        if other=="0"{break;}
        tokio::time::sleep(Duration::from_millis(10)).await;
    }}).await.expect("另一位接收者真实完成后再读取当前执行者");
    let (status, result) = h.request(reqwest::Method::GET, &activities, None).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(
        result["ticker"]["nextReminder"]["dueAtMs"],
        now + interval,
        "主动轮次完成后必须重新等待完整31分钟，不能下一全局slot提前再触发"
    );
    assert_eq!(
        conversation_discovery::cursor(h.state.storage(), channel)
            .await
            .unwrap()
            .unwrap()
            .proactive_slot,
        cursor.proactive_slot,
        "读取状态不得消费新的发现slot"
    );
    let (status, result) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/conversations/dm:other/agent-activities?agentId=agent",
            None,
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(
        result["ticker"]["nextReminder"],
        Value::Null,
        "DM没有主动提醒计时器"
    );
    assert_eq!(
        h.post(&settings, json!({"proactiveEnabled":false})).await.0,
        200
    );
    let (status, result) = h.request(reqwest::Method::GET, &activities, None).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["ticker"]["nextReminder"], Value::Null);
    h.close(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn narrowing_phone_mode_blocks_queued_collaboration_before_any_target_run_is_created() {
    phone_queued_draft_cannot_outlive_authority(Some("read_only")).await;
}
