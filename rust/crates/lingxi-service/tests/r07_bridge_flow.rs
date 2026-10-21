//! 平台协议响应使用替身；服务、子进程、数据库、身份和 Run 均走正式实现。
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
    approval_service::ApprovalService, bridge_worker::BridgeWorkerLaunch, inject::ManualClock,
    prepare_layout, toolgateway::ToolInvocationGateway, HomeSource, NetworkMode, ServiceConfig,
    ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
#[derive(Default)]
struct Provider {
    calls: AtomicUsize,
    priors: Mutex<Vec<(String, String)>>,
    /// 测试闸门：armed 时含「阻塞」的提交挂起，用于构造在跑的 run。
    gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.fixture".into(),
            model: "synthetic".into(),
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
        self.priors
            .lock()
            .unwrap()
            .push((input.submission.clone(), format!("{:?}", input.prior)));
        let gate = self.gate.lock().unwrap().clone();
        Box::pin(async move {
            if let Some(gate) = &gate {
                if input.submission.contains("阻塞") {
                    let _ = tokio::time::timeout(Duration::from_secs(15), gate.notified()).await;
                }
            }
            if input.turn == 1 {
                if let Ok(command) = serde_json::from_str::<Value>(&input.submission) {
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
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: if input.submission.contains("撤权后问答") {
                                "访客公开回复".into()
                            } else {
                                "<t> 10-16 09:30 </t>实际回复 PRIVATE_OWNER_ONLY".into()
                            },
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
    provider: Arc<Provider>,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot(home: Option<PathBuf>, unknown: bool) -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("lingxi_service=debug")
            .with_test_writer()
            .try_init();
        let home = home.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "r07-bridge-flow-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::write(
            home.join("agents/lingxi/config.yaml"),
            "agent:\n  name: 合成助手\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let script = home.join("platform-fixture.cjs");
        // 同一真实消息编号重投两次；回执由外部协议替身响应，不由宿主补造。
        std::fs::write(&script,format!(r#"const fs=require('node:fs');const path=require('node:path');const rl=require('node:readline').createInterface({{input:process.stdin}});let init;let sent=0;const media=new Map();
rl.on('line',line=>{{const command=JSON.parse(line);const emit=(body)=>process.stdout.write(JSON.stringify({{connectorId:init.connectorId,generation:init.generation,...body}})+'\n');if(command.kind==='init'){{init=command;emit({{kind:'ready'}});const message={{kind:'message',messageId:'platform-message-1',conversationId:'conversation-1',conversationKind:'dm',senderId:'sender-1',text:'测试消息',attachments:[],replyContext:{{isGroup:false,targetChatId:'sender-1',contextToken:'synthetic-context-token'}}}};emit(message);emit(message);}}else if(command.kind==='media_begin'){{media.set(command.attachmentId,[]);}}else if(command.kind==='media_chunk'){{media.get(command.attachmentId).push(Buffer.from(command.data,'base64'));}}else if(command.kind==='send'){{fs.appendFileSync(path.join(__dirname,'platform-sends.jsonl'),JSON.stringify({{deliveryId:command.deliveryId,text:command.text,attachments:command.attachments.map(id=>Buffer.concat(media.get(id)).toString('base64'))}})+'\n');if({unknown}||fs.existsSync(path.join(__dirname,'suppress-receipts')))return;emit({{kind:'receipt',deliveryId:command.deliveryId,status:'confirmed',platformMessageIds:['platform-reply-'+(++sent)],error:null}});}}else if(command.kind==='stop')process.exit(0);}});setInterval(()=>{{const file=path.join(__dirname,'next-platform-message.json');if(!init||!fs.existsSync(file))return;const message=JSON.parse(fs.readFileSync(file,'utf8'));fs.unlinkSync(file);process.stdout.write(JSON.stringify({{connectorId:init.connectorId,generation:init.generation,...message}})+'\n');}},20);"#,unknown=unknown)).unwrap();
        let node = std::process::Command::new("which")
            .arg("node")
            .output()
            .unwrap();
        assert!(node.status.success());
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let provider = Arc::new(Provider::default());
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
                shutdown_timeout_ms: 5000,
            },
            &layout,
            ServiceDeps {
                bridge_worker_launch: Some(BridgeWorkerLaunch {
                    program: PathBuf::from(String::from_utf8(node.stdout).unwrap().trim()),
                    args: vec![script.to_string_lossy().into_owned()],
                    cwd: home.clone(),
                }),
                clock,
                tool_gateway: Some(gateway),
                turn_provider: Some(provider.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let router = lingxi_service::build_router(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
        });
        state.bridge().start(state.clone());
        Self {
            home,
            state,
            provider,
            url,
            token,
            stop,
            server,
        }
    }
    async fn post(&self, path: &str, body: Value) -> Value {
        let response = reqwest::Client::new()
            .post(format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let value: Value = response.json().await.unwrap();
        assert!(status.is_success(), "{status}: {value}");
        value
    }
    async fn enable(&self, platform: &str) {
        self.post("/lingxi/v1/bridge/config?agentId=lingxi",json!({"platform":platform,"enabled":false,"credentials":match platform{"telegram"=>json!({"token":"synthetic-token"}),"wechat"=>json!({"botToken":"synthetic-token"}),"feishu"=>json!({"appId":"fixture","appSecret":"synthetic-secret"}),"dingtalk"=>json!({"clientId":"fixture","clientSecret":"synthetic-secret"}),_=>json!({"appID":"fixture","appSecret":"synthetic-secret"})}})).await;
        self.post(
            "/lingxi/v1/bridge/owner?agentId=lingxi",
            json!({"platform":platform,"userId":"sender-1"}),
        )
        .await;
        self.post(
            "/lingxi/v1/bridge/config?agentId=lingxi",
            json!({"platform":platform,"enabled":true}),
        )
        .await;
    }
    async fn sql_opt(&self, sql: &str) -> Option<String> {
        self.state
            .storage()
            .query_one_text(sql, vec![])
            .await
            .unwrap()
    }
    async fn sql(&self, sql: &str) -> String {
        self.sql_opt(sql).await.unwrap()
    }
    async fn wait(&self, sql: &str, expected: &str) {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                // 行尚未出现（None）继续轮询到超时，不当场 panic。
                if self.sql_opt(sql).await.as_deref() == Some(expected) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("Bridge expected {expected} for {sql}"));
    }
    async fn inbound(&self, id: &str, sender: &str, text: &str) {
        let message = json!({"kind":"message","messageId":id,"conversationId":"conversation-1","conversationKind":"dm","senderId":sender,"text":text,"attachments":[],"replyContext":{"isGroup":false,"targetChatId":sender,"contextToken":"synthetic-context-token"}});
        let path = self.home.join("next-platform-message.json");
        std::fs::write(&path, message.to_string()).unwrap();
        // 替身 20ms 轮询该文件；等它消费删除后再返回，连续投递不互相覆盖。
        tokio::time::timeout(Duration::from_secs(10), async {
            while path.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    /// 真实外发记录（协议替身在 send 命令时落 jsonl）。
    fn sends(&self) -> Vec<Value> {
        let path = self.home.join("platform-sends.jsonl");
        if !path.exists() {
            return Vec::new();
        }
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    async fn notify(&self, request: &str) -> String {
        if self
            .state
            .storage()
            .get_session("local-notify-source")
            .await
            .unwrap()
            .is_none()
        {
            self.state
                .session_admin()
                .create_session(
                    "local-notify-source",
                    "lingxi",
                    "user_local",
                    "本地合成来源",
                    Some("operate"),
                    &[],
                    true,
                    1_800_000_000_000,
                )
                .await
                .unwrap();
        }
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
        let accepted=self.state.sessions().execute_background_for(self.state.storage(),self.state.events(),self.state.runs(),self.state.background(),&principal,"local-notify-source",&lingxi_service::ExecuteSubmission::plain(&json!({"tool":"notify","args":{"title":"本地合成提醒","body":request,"channels":["bridge_owner"],"bridgePlatforms":["wechat"]}}).to_string()),1_800_000_000_000).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let run = self
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
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        accepted.run_id
    }
    async fn close(self) -> PathBuf {
        self.state.loop_services().stop_idle_listener();
        self.state.loop_services().alarm.dispose();
        self.state.loop_services().controller.dispose();
        self.state.bridge().stop();
        tokio::time::timeout(Duration::from_secs(12), self.state.bridge().wait_stopped())
            .await
            .expect("Bridge 退出后未在期限内回收")
            .unwrap();
        let _ = self.stop.send(());
        self.server.await.unwrap();
        self.state.storage().close().await.unwrap();
        self.home
    }
}
#[tokio::test]
async fn stopping_immediately_after_start_does_not_lose_shutdown() {
    let h = Harness::boot(None, false).await;
    h.close().await;
}
#[tokio::test]
async fn all_five_platforms_use_real_runs_and_stable_reply_receipts() {
    let h = Harness::boot(None, false).await;
    for platform in ["telegram", "feishu", "dingtalk", "qq", "wechat"] {
        h.enable(platform).await;
    }
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "5",
    )
    .await;
    assert_eq!(h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs").await, "5");
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 5);
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE owner_kind='bridge'")
            .await,
        "5"
    );
    assert_eq!(h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_inbox_v1 WHERE status='succeeded' AND role='owner'").await,"5");
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/bridge/status?agentId=lingxi", h.url))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();
    let text = response.text().await.unwrap();
    let status: Value = serde_json::from_str(&text).unwrap();
    for platform in ["telegram", "feishu", "dingtalk", "qq", "wechat"] {
        assert_eq!(status[platform]["configured"], true, "{platform}: {status}");
    }
    assert_eq!(status["wechat"]["hasToken"], true);
    assert_eq!(status["wechat"]["token"], "********");
    assert!(!text.contains("synthetic-secret"));
    assert!(!text.contains("synthetic-token"));
    assert!(!text.contains("synthetic-context-token"));
    let raw = h
        .sql("SELECT payload_json FROM bridge_outbox_v1 LIMIT 1")
        .await;
    assert!(raw.contains("实际回复"));
    assert!(!raw.contains("10-16"));
    let home = h.close().await;
    let restarted = Harness::boot(Some(home), false).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        restarted
            .sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs")
            .await,
        "5"
    );
    assert_eq!(restarted.provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        restarted
            .sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'")
            .await,
        "5"
    );
    restarted.close().await;
}
#[tokio::test]
async fn sent_without_receipt_becomes_unknown_on_restart_and_is_not_sent_again() {
    let h = Harness::boot(None, true).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='started'",
        "1",
    )
    .await;
    let home = h.close().await;
    let restarted = Harness::boot(Some(home), false).await;
    restarted
        .wait(
            "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='unknown'",
            "1",
        )
        .await;
    assert_eq!(
        restarted
            .sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs")
            .await,
        "1"
    );
    assert_eq!(restarted.provider.calls.load(Ordering::SeqCst), 0);
    restarted.close().await;
}

#[tokio::test]
async fn delivered_owner_notice_is_context_only_after_real_receipt_and_survives_restart() {
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    let run = h.notify("真实回执后保留一次").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_notice_receipts_v1",
        "1",
    )
    .await;
    assert_eq!(h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE role='host_context' AND run_id IS NULL").await,"1");
    assert_eq!(h.sql(&format!("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE run_id='{run}' AND status='confirmed'")).await,"1");
    let home = h.close().await;
    let restarted = Harness::boot(Some(home), false).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        restarted
            .sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_notice_receipts_v1")
            .await,
        "1"
    );
    assert_eq!(restarted.provider.calls.load(Ordering::SeqCst), 0);
    restarted.close().await;
}
#[tokio::test]
async fn unknown_owner_notice_never_claims_delivery_or_enters_context() {
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    std::fs::write(h.home.join("suppress-receipts"), "").unwrap();
    h.notify("平台未确认不能记作通知成功").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='started'",
        "1",
    )
    .await;
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_notice_receipts_v1")
            .await,
        "0"
    );
    let home = h.close().await;
    std::fs::remove_file(home.join("suppress-receipts")).unwrap();
    let restarted = Harness::boot(Some(home), false).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        restarted
            .sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='unknown'")
            .await,
        "1"
    );
    assert_eq!(
        restarted
            .sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_notice_receipts_v1")
            .await,
        "0"
    );
    assert_eq!(restarted.provider.calls.load(Ordering::SeqCst), 0);
    restarted.close().await;
}
#[tokio::test]
async fn clearing_owner_changes_new_messages_to_guest_without_replaying_old_owner_task() {
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    h.post(
        "/lingxi/v1/bridge/owner?agentId=lingxi",
        json!({"platform":"wechat","userId":null}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    h.inbound("message-after-revocation", "sender-1", "撤权后问答")
        .await;
    h.wait("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_inbox_v1 WHERE role='guest' AND status='succeeded'","1").await;
    assert_eq!(h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs").await, "2");
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_inbox_v1 WHERE role='owner'")
            .await,
        "1"
    );
    assert_eq!(
        h.sql("SELECT CAST(COUNT(DISTINCT session_id) AS TEXT) FROM bridge_inbox_v1")
            .await,
        "2"
    );
    let priors = h.provider.priors.lock().unwrap().clone();
    let guest = priors
        .iter()
        .find(|(input, _)| input.contains("撤权后问答"))
        .unwrap();
    assert!(
        !guest.1.contains("PRIVATE_OWNER_ONLY"),
        "撤权后不能继承旧主人私有上下文: {}",
        guest.1
    );
    h.close().await;
}

#[tokio::test]
async fn manual_file_delivery_uses_actual_managed_bytes_and_retry_keeps_one_send() {
    use lingxi_adapters::storage::bridge as store;
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    h.state
        .session_admin()
        .create_session(
            "local-file-source",
            "lingxi",
            "user_local",
            "文件来源",
            Some("operate"),
            &[],
            true,
            1_800_000_000_000,
        )
        .await
        .unwrap();
    let content = b"R07 actual managed file bytes";
    let file = h
        .state
        .session_files()
        .register_bridge_blob(
            "local-file-source",
            Some("actual.txt"),
            "text/plain",
            content,
            "attachment",
        )
        .await
        .unwrap();
    let connector = store::list_connectors(h.state.storage())
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.input.platform == "wechat")
        .unwrap();
    let conversation = store::list_conversations(h.state.storage(), &connector.input.id, 5)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let request = json!({"platform":"wechat","sessionKey":store::conversation_key(&conversation),"sessionId":"local-file-source","fileId":file.file_id,"requestId":"manual-file-actual-1"});
    let first = h
        .post(
            "/lingxi/v1/bridge/send-media?agentId=lingxi",
            request.clone(),
        )
        .await;
    let delivery = first["deliveryId"].as_str().unwrap();
    h.wait(
        &format!("SELECT status FROM bridge_outbox_v1 WHERE delivery_id='{delivery}'"),
        "confirmed",
    )
    .await;
    let replay = h
        .post("/lingxi/v1/bridge/send-media?agentId=lingxi", request)
        .await;
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["deliveryId"], first["deliveryId"]);
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs").await,
        "1",
        "手动发文件不能虚构模型任务"
    );
    let sends = std::fs::read_to_string(h.home.join("platform-sends.jsonl")).unwrap();
    let sends: Vec<Value> = sends
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let manual: Vec<_> = sends
        .iter()
        .filter(|r| r["deliveryId"] == delivery)
        .collect();
    assert_eq!(manual.len(), 1);
    use base64::Engine;
    assert_eq!(
        manual[0]["attachments"][0],
        base64::engine::general_purpose::STANDARD.encode(content)
    );
    h.close().await;
}

#[tokio::test]
async fn actual_owner_can_take_over_only_selected_desktop_session_and_exit_restores_bridge() {
    let h = Harness::boot(None, false).await;
    for (id, title) in [
        ("a-selected-desktop", "选中的桌面"),
        ("z-other-desktop", "另一桌面"),
    ] {
        h.state
            .session_admin()
            .create_session(
                id,
                "lingxi",
                "user_local",
                title,
                Some("operate"),
                &[],
                true,
                1_800_000_000_000,
            )
            .await
            .unwrap();
    }
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    h.inbound("rc-list", "sender-1", "/rc").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "2",
    )
    .await;
    h.inbound("rc-select", "sender-1", "1").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "2",
    )
    .await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "3",
    )
    .await;
    h.inbound("rc-question", "sender-1", "在选中的桌面继续")
        .await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_inbox_v1 WHERE status='succeeded'",
        "2",
    )
    .await;
    assert_eq!(h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE owner_kind='bridge_takeover' AND session_id='a-selected-desktop'").await,"1");
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE session_id='z-other-desktop'")
            .await,
        "0"
    );
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs").await,
        "2",
        "选择命令没有伪造模型运行"
    );
    let events = h.state.app_events().since(0).to_string();
    assert!(events.contains("native:a-selected-desktop"));
    assert!(events.contains("bridge_rc_attached"));
    h.inbound("rc-exit", "sender-1", "/exitrc").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "3",
    )
    .await;
    h.inbound("after-rc-exit", "sender-1", "回到独立桥接会话")
        .await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_inbox_v1 WHERE status='succeeded'",
        "3",
    )
    .await;
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE owner_kind='bridge'")
            .await,
        "2"
    );
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE owner_kind='bridge_takeover'")
            .await,
        "1"
    );
    assert!(h
        .state
        .app_events()
        .since(0)
        .to_string()
        .contains("bridge_rc_detached"));
    h.close().await;
}

// ── R07 收口 B 组：/loop 与 /stop 经 SDK/Bridge 真实输入链 ─────────────
// 入口全部是协议替身送来的平台消息（非内部 API 直调）；回执经命令收据 →
// outbox → 协议替身 send 落 jsonl。循环轮经真实调度链（origin=loop 循环
// 根 run），回执由循环投递台账推进为真实外发。

fn loop_state_file(home: &std::path::Path) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(home.join(".ephemeral").join("loop-state.json")).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn bridge_loop_commands_run_the_full_lifecycle_over_the_real_inbound_chain() {
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    // 启动：命令收据文本逐字，kickoff 轮是循环根 run，回执走台账链。
    h.inbound("loop-start", "sender-1", "/loop 每隔一会儿提醒我喝水")
        .await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE command='loop' AND status='confirmed'",
        "1",
    )
    .await;
    assert_eq!(
        h.sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 WHERE command='loop' ORDER BY rowid LIMIT 1 OFFSET 0").await,
        "（循环已启动，/loop status 查看进度，/loop stop 终止）"
    );
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM run_lineage WHERE origin='loop'",
        "1",
    )
    .await;
    h.wait(
        "SELECT status FROM bridge_outbox_v1 WHERE delivery_id LIKE 'loop-reply:%'",
        "confirmed",
    )
    .await;
    let sid = h
        .sql("SELECT current_session_id FROM bridge_conversations_v1")
        .await;
    assert_eq!(
        h.sql("SELECT r.session_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE l.origin='loop'")
            .await,
        sid
    );
    assert_eq!(
        h.sql("SELECT r.owner_kind FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE l.origin='loop'")
            .await,
        "local_user"
    );
    assert_eq!(
        h.sql("SELECT l.parent_run_id IS NULL AND l.cause_id=r.session_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE l.origin='loop'")
            .await,
        "1"
    );
    let sends = h.sends();
    assert!(
        sends.iter().any(|row| row["deliveryId"]
            .as_str()
            .is_some_and(|id| id.starts_with("loop-reply:"))
            && row["text"]
                .as_str()
                .is_some_and(|text| text.contains("实际回复"))),
        "循环轮回执须真实外发：{sends:?}"
    );
    // 会话状态持久化：桥目标、绑定会话、运行中。
    let stored = loop_state_file(&h.home);
    assert_eq!(stored["loops"][0]["key"].as_str().unwrap(), sid);
    assert_eq!(
        stored["loops"][0]["target"]["kind"].as_str().unwrap(),
        "bridge"
    );
    assert_eq!(stored["loops"][0]["status"].as_str().unwrap(), "running");
    // 状态查询。
    h.inbound("loop-status", "sender-1", "/loop status").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "2",
    )
    .await;
    let status_text = h
        .sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 1")
        .await;
    assert!(status_text.contains("循环状态：running"), "{status_text}");
    assert!(
        status_text.contains("每隔一会儿提醒我喝水"),
        "{status_text}"
    );
    // 重复投递同一平台消息编号：不产生第二张收据、不产生第二条外发。
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "4",
    )
    .await;
    h.inbound("loop-status", "sender-1", "/loop status").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1")
            .await,
        "2"
    );
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'")
            .await,
        "4"
    );
    // 暂停 → 恢复（恢复注入新一轮）→ 终止。
    h.inbound("loop-pause", "sender-1", "/loop pause").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "3",
    )
    .await;
    assert_eq!(
        h.sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 2").await,
        "（循环已暂停，/loop resume 恢复）"
    );
    assert_eq!(
        loop_state_file(&h.home)["loops"][0]["status"]
            .as_str()
            .unwrap(),
        "paused"
    );
    h.inbound("loop-resume", "sender-1", "/loop resume").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "4",
    )
    .await;
    assert_eq!(
        h.sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 3").await,
        "（循环已恢复）"
    );
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM run_lineage WHERE origin='loop'",
        "2",
    )
    .await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE delivery_id LIKE 'loop-reply:%' AND status='confirmed'",
        "2",
    )
    .await;
    h.inbound("loop-stop", "sender-1", "/loop stop").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "5",
    )
    .await;
    assert_eq!(
        h.sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 4").await,
        "（循环已终止）"
    );
    assert_eq!(
        loop_state_file(&h.home)["loops"][0]["status"]
            .as_str()
            .unwrap(),
        "stopped"
    );
    h.close().await;
}

#[tokio::test]
async fn bridge_guest_slash_text_never_becomes_a_command() {
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    // owner 先起循环，坐实「有状态可偷」。
    h.inbound("owner-loop", "sender-1", "/loop 主人循环").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE command='loop' AND status='confirmed'",
        "1",
    )
    .await;
    // 访客的 /loop 与 /stop 不是命令：无收据、无循环副作用，只产生
    // 访客自己会话里的普通模型回复（现行桥边界：命令分类只认 owner 私聊）。
    h.inbound("guest-loop", "sender-2", "/loop status").await;
    h.inbound("guest-stop", "sender-2", "/stop").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_inbox_v1 WHERE role='guest' AND status='succeeded'",
        "2",
    )
    .await;
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1")
            .await,
        "1"
    );
    assert_eq!(
        h.sql("SELECT DISTINCT role FROM bridge_inbox_v1 WHERE sender_id='sender-2'")
            .await,
        "guest"
    );
    let stored = loop_state_file(&h.home);
    let loops = stored["loops"].as_array().unwrap();
    assert_eq!(loops.len(), 1, "访客命令文本不得触碰循环状态：{stored}");
    assert_eq!(loops[0]["status"].as_str().unwrap(), "running");
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE owner_kind='bridge' AND status='cancelled'")
            .await,
        "0"
    );
    h.close().await;
}

#[tokio::test]
async fn bridge_stop_family_reports_idle_and_cancels_an_active_run() {
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    // 空闲停止与别名收据（收据名 = 实际词元）。
    h.inbound("stop-idle", "sender-1", "/stop").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    assert_eq!(
        h.sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 0").await,
        "已停止（当前无活动回复）"
    );
    h.inbound("halt-idle", "sender-1", "/halt").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "2",
    )
    .await;
    assert_eq!(
        h.sql("SELECT command FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 1")
            .await,
        "halt"
    );
    assert_eq!(
        h.sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 1").await,
        "已停止（当前无活动回复）"
    );
    // 在跑的 run 被真实取消。
    let gate = Arc::new(tokio::sync::Notify::new());
    *h.provider.gate.lock().unwrap() = Some(gate.clone());
    h.inbound("blocking", "sender-1", "请阻塞一会儿").await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while h.provider.calls.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE status='running'",
        "1",
    )
    .await;
    h.inbound("stop-active", "sender-1", "/stop").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "3",
    )
    .await;
    assert_eq!(
        h.sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 2").await,
        "正在停止当前回复"
    );
    gate.notify_waiters();
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE status='cancelled'",
        "1",
    )
    .await;
    h.close().await;
}

#[tokio::test]
async fn bridge_loop_reply_ledger_survives_restart_and_reports_the_interrupted_turn() {
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    // kickoff 轮挂起在替身处：台账 queued 行已落账，回执文本尚未冻结。
    let gate = Arc::new(tokio::sync::Notify::new());
    *h.provider.gate.lock().unwrap() = Some(gate.clone());
    h.inbound("loop-gated", "sender-1", "/loop 阻塞循环").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE command='loop' AND status='confirmed'",
        "1",
    )
    .await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM run_lineage WHERE origin='loop'",
        "1",
    )
    .await;
    h.wait("SELECT status FROM bridge_loop_deliveries_v1", "queued")
        .await;
    let home = h.close().await;
    // 重启：中断的轮被恢复标记终态，台账由 tick 推进为真实外发回执；
    // 循环记录本身存活，可 status、可 stop。
    let r = Harness::boot(Some(home), false).await;
    r.wait(
        "SELECT status FROM bridge_outbox_v1 WHERE delivery_id LIKE 'loop-reply:%'",
        "confirmed",
    )
    .await;
    assert!(
        r.sends().iter().any(|row| row["deliveryId"]
            .as_str()
            .is_some_and(|id| id.starts_with("loop-reply:"))
            && row["text"]
                .as_str()
                .is_some_and(|text| text.contains("暂时无法确认"))),
        "中断轮的结果须如实外发：{:?}",
        r.sends()
    );
    r.inbound("loop-status-after-restart", "sender-1", "/loop status")
        .await;
    // 同库重启：重启前的启动收据仍在，status 收据是第 2 条、stop 第 3 条。
    r.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "2",
    )
    .await;
    let text = r
        .sql("SELECT json_extract(result_json,'$.text') FROM bridge_command_receipts_v1 ORDER BY rowid LIMIT 1 OFFSET 1")
        .await;
    assert!(text.contains("循环状态：running"), "{text}");
    r.inbound("loop-stop-after-restart", "sender-1", "/loop stop")
        .await;
    r.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_command_receipts_v1 WHERE status='confirmed'",
        "3",
    )
    .await;
    assert_eq!(
        loop_state_file(&r.home)["loops"][0]["status"]
            .as_str()
            .unwrap(),
        "stopped"
    );
    r.close().await;
}

#[tokio::test]
async fn bridge_loop_delivery_ledger_rejects_forged_or_guest_sources() {
    let h = Harness::boot(None, false).await;
    h.enable("wechat").await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    let conversation = h
        .sql("SELECT conversation_id FROM bridge_conversations_v1")
        .await;
    let actor = h.sql("SELECT actor_id FROM bridge_conversations_v1").await;
    let connector = h.sql("SELECT id FROM bridge_connectors_v1").await;
    let generation: u64 = h
        .sql("SELECT CAST(generation AS TEXT) FROM bridge_connectors_v1")
        .await
        .parse()
        .unwrap();
    let sid = h
        .sql("SELECT current_session_id FROM bridge_conversations_v1")
        .await;
    let now = || h.state.clock().now_unix_ms();
    use lingxi_adapters::storage::bridge as store;
    // 通知腿：合法 owner 私聊通知入队成功，由 tick 真实外发。
    store::enqueue_loop_delivery(
        h.state.storage(),
        &store::BridgeLoopDeliveryInput {
            delivery_id: "loop-notice:test-1".into(),
            connector_id: connector.clone(),
            connector_generation: generation,
            conversation_id: conversation.clone(),
            sender_id: "sender-1".into(),
            actor_id: actor.clone(),
            session_id: sid.clone(),
            run_id: None,
            kind: "notice".into(),
            text: Some("（合成通知：循环需要你回到灵犀查看）".into()),
        },
        now(),
    )
    .await
    .expect("合法通知须入队");
    h.wait(
        "SELECT status FROM bridge_outbox_v1 WHERE delivery_id='loop-notice:test-1'",
        "confirmed",
    )
    .await;
    assert!(h.sends().iter().any(
        |row| row["deliveryId"].as_str() == Some("loop-notice:test-1")
            && row["text"]
                .as_str()
                .is_some_and(|text| text.contains("合成通知"))
    ));
    // 伪造来源逐一响亮拒绝：非 owner 发件人、非循环血统的 run、编造的 run。
    let forged = |delivery_id: &str, sender: &str, run: Option<String>| {
        let notice = run.is_none();
        store::BridgeLoopDeliveryInput {
            delivery_id: delivery_id.into(),
            connector_id: connector.clone(),
            connector_generation: generation,
            conversation_id: conversation.clone(),
            sender_id: sender.into(),
            actor_id: actor.clone(),
            session_id: sid.clone(),
            run_id: run,
            kind: if notice {
                "notice".into()
            } else {
                "reply".into()
            },
            text: if notice {
                Some("伪造通知".into())
            } else {
                None
            },
        }
    };
    let err = store::enqueue_loop_delivery(
        h.state.storage(),
        &forged("loop-notice:test-guest", "sender-2", None),
        now(),
    )
    .await
    .expect_err("访客发件人的通知须拒绝");
    assert!(err.to_string().contains("conversation"), "{err}");
    let real_run = h.sql("SELECT run_id FROM runs LIMIT 1").await;
    let err = store::enqueue_loop_delivery(
        h.state.storage(),
        &forged("loop-reply:test-bridge-run", "sender-1", Some(real_run)),
        now(),
    )
    .await
    .expect_err("桥血统 run 冒充循环轮须拒绝");
    assert!(err.to_string().contains("loop"), "{err}");
    let err = store::enqueue_loop_delivery(
        h.state.storage(),
        &forged(
            "loop-reply:test-missing-run",
            "sender-1",
            Some("run-不存在".into()),
        ),
        now(),
    )
    .await
    .expect_err("编造 run 须拒绝");
    assert!(err.to_string().contains("missing"), "{err}");
    assert_eq!(
        h.sql("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_loop_deliveries_v1")
            .await,
        "1"
    );
    h.close().await;
}
