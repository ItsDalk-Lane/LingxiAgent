//! 原三叶：真实工具网关、会话日志/事件、超时与当前原生界面。
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolOutcome, ToolRequest,
        TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
use lingxi_service::{
    approval_service::{Answer, ApprovalService},
    inject::ManualClock,
    prepare_layout, run,
    toolgateway::ToolInvocationGateway,
    HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[derive(Default)]
struct Provider {
    results: Mutex<HashMap<String, Value>>,
    calls: std::sync::atomic::AtomicUsize,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "interaction.fixture".into(),
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
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if input.turn == 1 {
                let command: Value = serde_json::from_str(&input.submission).unwrap();
                let request = ToolRequest::from_effective_arguments(
                    format!("tool:first-party:{}", command["tool"].as_str().unwrap()),
                    command["args"].clone(),
                    &SchemaBudget::default(),
                )
                .unwrap();
                ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![request],
                        content: vec![],
                    },
                )
            } else {
                let value = input
                    .prior
                    .iter()
                    .rev()
                    .find_map(|item| {
                        if let ExchangeItem::ToolResult { outcome, .. } = item {
                            Some(match outcome {
                                ToolOutcome::Success { result } => {
                                    json!({"ok":true,"details":result.details})
                                }
                                ToolOutcome::Failed { error } => {
                                    json!({"ok":false,"error":error.message})
                                }
                                other => json!({"ok":false,"outcome":format!("{other:?}")}),
                            })
                        } else {
                            None
                        }
                    })
                    .expect("必须收到实际工具结果");
                self.results
                    .lock()
                    .unwrap()
                    .insert(format!("{}|{}", ctx.session_id, input.submission), value);
                ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::Final {
                        message: NormalizedMessage {
                            role: "assistant".into(),
                            content: vec![ContentBlock::Text {
                                text: "处理结束".into(),
                            }],
                            model_call_id: Some(call.clone()),
                        },
                    },
                )
            }
        })
    }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    url: String,
    token: String,
    clock: Arc<ManualClock>,
    provider: Arc<Provider>,
    approvals: Arc<ApprovalService>,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
async fn request(url: &str, token: &str, method: &str, path: &str, body: Value) -> (u16, Value) {
    let payload = body.to_string();
    let addr = url.strip_prefix("http://").unwrap();
    let wire=format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(45), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let split = bytes.windows(4).position(|x| x == b"\r\n\r\n").unwrap();
    let status = String::from_utf8_lossy(&bytes[..split])
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let value = serde_json::from_slice(&bytes[split + 4..])
        .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes[split+4..])}));
    (status, value)
}
impl Harness {
    async fn boot() -> Self {
        Self::boot_at(None).await
    }
    async fn boot_at(existing: Option<PathBuf>) -> Self {
        let home = existing.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-interaction-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/agent")).unwrap();
        std::fs::write(
            home.join("agents/agent/config.yaml"),
            "agent:\n  name: 合成助手\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let clock = Arc::new(ManualClock::new(1_800_000_000_000));
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let registry = Arc::new(ToolRegistry::new());
        let gateway = Arc::new(ToolInvocationGateway::new(
            registry,
            approvals.clone(),
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
        let provider = Arc::new(Provider::default());
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 5000,
            },
            &prepare_layout(&home).unwrap(),
            ServiceDeps {
                clock: clock.clone(),
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway),
                approval_gate: Some(approvals.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, wait) = tokio::sync::oneshot::channel();
        let server = state.clone();
        let task = tokio::spawn(async move {
            run(
                server,
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
        let url = format!("http://{}", wait.await.unwrap());
        Self {
            home,
            state,
            url,
            token,
            clock,
            provider,
            approvals,
            stop,
            task,
        }
    }
    async fn req(&self, method: &str, path: &str, body: Value) -> (u16, Value) {
        request(&self.url, &self.token, method, path, body).await
    }
    async fn session(&self, sid: &str) {
        self.session_mode(sid, "operate").await;
    }
    async fn session_mode(&self, sid: &str, mode: &str) {
        let (s, v) = self
            .req(
                "POST",
                "/lingxi/v1/sessions",
                json!({"sessionId":sid,"agentId":"agent","title":sid,"permissionMode":mode}),
            )
            .await;
        assert_eq!(s, 201, "{v}");
    }
    fn launch(
        &self,
        sid: &str,
        tool: &str,
        args: Value,
    ) -> (String, tokio::task::JoinHandle<(u16, Value)>) {
        let cmd = json!({"tool":tool,"args":args}).to_string();
        let url = self.url.clone();
        let token = self.token.clone();
        let path = format!("/lingxi/v1/sessions/{sid}/execute");
        let input = cmd.clone();
        (
            format!("{sid}|{cmd}"),
            tokio::spawn(async move {
                request(&url, &token, "POST", &path, json!({"input":input})).await
            }),
        )
    }
    async fn result(&self, cmd: &str) -> Value {
        for _ in 0..300 {
            if let Some(v) = self.provider.results.lock().unwrap().get(cmd).cloned() {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("没有实际工具结果：{cmd}");
    }
    async fn pending(&self, sid: &str) -> Value {
        for _ in 0..300 {
            let (s, v) = self
                .req(
                    "GET",
                    &format!("/lingxi/v1/sessions/interactions?sessionId={sid}"),
                    Value::Null,
                )
                .await;
            assert_eq!(s, 200, "{v}");
            if !v["questions"].as_array().unwrap().is_empty() {
                return v["questions"][0].clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("没有问题卡");
    }
    async fn close(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }
}
fn questions() -> Value {
    json!([{"key":"mode","question":"选择处理方式","options":[{"value":"fast","label":"快速"},{"value":"safe","label":"稳妥"}],"recommended":"safe"},{"key":"steps","question":"选择处理步骤","type":"multi","options":[{"label":"步骤一"},{"label":"步骤二"}]},{"key":"note","question":"补充说明","type":"text","required":false}])
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_batched_question_real_answers_timeout_dismiss_abort_and_refusals() {
    let h = Harness::boot().await;
    for sid in [
        "confirm",
        "dismiss",
        "timeout",
        "abort",
        "unavailable",
        "invalid",
    ] {
        h.session(sid).await;
    }
    let (cmd, run) = h.launch("confirm", "ask_user", json!({"questions":questions()}));
    let q = h.pending("confirm").await;
    assert_eq!(q["questions"][0]["options"][1]["label"], "稳妥");
    let(s,_)=h.req("POST","/lingxi/v1/sessions/interactions/answer",json!({"sessionId":"dismiss","id":q["id"],"action":"confirmed","values":{"mode":"fast"}})).await;
    assert_eq!(s, 409);
    let(s,_)=h.req("POST","/lingxi/v1/sessions/interactions/answer",json!({"sessionId":"confirm","id":q["id"],"action":"confirmed","values":{"mode":"safe","steps":["步骤二","unknown","步骤二"],"note":"  用户原文  "}})).await;
    assert_eq!(s, 200);
    {
        let response = run.await.unwrap();
        assert_eq!(response.0, 200, "{response:?}");
    }
    let result = h.result(&cmd).await;
    assert_eq!(result["details"]["answered"], true);
    assert_eq!(result["details"]["answers"][0]["key"], "mode");
    assert_eq!(result["details"]["answers"][0]["values"], json!(["safe"]));
    assert_eq!(result["details"]["answers"][0]["labels"], json!(["稳妥"]));
    assert_eq!(result["details"]["answers"][1]["values"], json!(["步骤二"]));
    assert_eq!(
        result["details"]["answers"][2]["values"],
        json!(["用户原文"])
    );
    for (sid, action) in [
        ("dismiss", "dismissed"),
        ("timeout", "timeout"),
        ("abort", "aborted"),
    ] {
        let (cmd, run) = h.launch(
            sid,
            "ask_user",
            json!({"questions":questions(),"timeout_seconds":30}),
        );
        let q = h.pending(sid).await;
        if action == "timeout" {
            h.clock.advance(30_001);
        } else if action == "aborted" {
            h.state.interactions().abort(q["id"].as_str().unwrap());
        } else {
            assert_eq!(h.req("POST","/lingxi/v1/sessions/interactions/answer",json!({"sessionId":sid,"id":q["id"],"action":"dismissed","values":{"mode":"fast"}})).await.0,200);
        }
        {
            let response = run.await.unwrap();
            assert_eq!(response.0, 200, "{response:?}");
        }
        let r = h.result(&cmd).await;
        if action == "timeout" {
            assert_eq!(r["details"]["timedOut"], true);
            assert_eq!(r["details"]["answers"][0]["values"], json!(["safe"]));
            assert_eq!(r["details"]["answers"][0]["autoSelected"], true);
            for i in [1, 2] {
                assert_eq!(r["details"]["answers"][i]["values"], json!([]));
                assert_eq!(r["details"]["answers"][i]["autoSelected"], false);
            }
        } else {
            assert_eq!(r["details"][action], true);
            assert_eq!(r["details"]["answers"], json!([]));
        }
        assert!(h.state.interactions().list_pending(sid).is_empty());
    }
    h.state.interactions().set_available(false);
    let (cmd, run) = h.launch("unavailable", "ask_user", json!({"questions":questions()}));
    {
        let response = run.await.unwrap();
        assert_eq!(response.0, 200, "{response:?}");
    }
    assert_eq!(h.result(&cmd).await["ok"], false);
    h.state.interactions().set_available(true);
    let (cmd, run) = h.launch(
        "invalid",
        "ask_user",
        json!({"questions":[{"key":"dup","question":"一"},{"key":"dup","question":"二"}]}),
    );
    {
        let response = run.await.unwrap();
        assert_eq!(response.0, 200, "{response:?}");
    }
    assert_eq!(h.result(&cmd).await["ok"], false);
    assert!(h.state.interactions().list_pending("invalid").is_empty());
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("missing"),
        run_id: lingxi_protocol::RunId::new("missing"),
        attempt: lingxi_protocol::AttemptId::new("missing#a1"),
        generation: 1,
    };
    assert!(h
        .state
        .interactions()
        .ask(&ctx, &json!({"questions":questions()}))
        .await
        .is_err());
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_full_todo_list_persists_all_states_emits_full_stream_and_invalid_preserves() {
    let h = Harness::boot().await;
    h.session("todos").await;
    let items = json!([{"content":"第一项","status":"pending"},{"content":"第二项","activeForm":"正在处理第二项","status":"in_progress"},{"content":"第三项","status":"blocked","blockedReason":"等待确认"},{"content":"第四项","status":"completed"}]);
    let (cmd, run) = h.launch("todos", "todo_write", json!({"todos":items}));
    {
        let response = run.await.unwrap();
        assert_eq!(response.0, 200, "{response:?}");
    }
    assert_eq!(h.result(&cmd).await["ok"], true);
    let (_, before) = h
        .req(
            "GET",
            "/lingxi/v1/sessions/interactions?sessionId=todos",
            Value::Null,
        )
        .await;
    assert_eq!(before["todoPanel"]["todos"].as_array().unwrap().len(), 4);
    assert_eq!(before["todoPanel"]["todos"][2]["blockedReason"], "等待确认");
    for bad in [
        json!([{"content":"x","status":"invalid"}]),
        json!([{"content":"x","status":"blocked"}]),
        json!([{"content":" ","status":"pending"}]),
        json!([{"content":"x","status":"pending"},{"content":"x","status":"completed"}]),
    ] {
        let (cmd, run) = h.launch("todos", "todo_write", json!({"todos":bad}));
        {
            let response = run.await.unwrap();
            assert_eq!(response.0, 200, "{response:?}");
        }
        assert_eq!(h.result(&cmd).await["ok"], false);
        let (_, after) = h
            .req(
                "GET",
                "/lingxi/v1/sessions/interactions?sessionId=todos",
                Value::Null,
            )
            .await;
        assert_eq!(before, after);
    }
    let event=h.state.storage().query_one_text("SELECT payload_json FROM key_events WHERE stream_id=?1 AND event_type='todo_update' ORDER BY seq DESC LIMIT 1",vec!["todos".into()]).await.unwrap().unwrap();
    let event: Value = serde_json::from_str(&event).unwrap();
    assert_eq!(event["todos"], before["todoPanel"]["todos"]);
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_card_persisted_session_result_and_real_review_refusal_without_source_write() {
    let h = Harness::boot().await;
    for sid in ["card", "denied"] {
        h.session_mode(sid, "ask").await;
    }
    let source = h.home.join("source.html");
    std::fs::write(&source, "原文件保持不变").unwrap();
    for sid in ["card", "denied"] {
        let (cmd, run) = h.launch(
            sid,
            "show_card",
            json!({"title":sid,"code":"<button>真实卡片</button><script>window.test=1</script>"}),
        );
        let mut pending = Vec::new();
        for _ in 0..150 {
            pending = h
                .approvals
                .pending_for_principal(&lingxi_kernel::Principal::LocalUser, sid);
            if !pending.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(pending.len(), 1);
        assert!(h.state.interactions().cards(sid).await.unwrap().is_empty());
        let decision = if sid == "card" {
            Answer::Approve
        } else {
            Answer::Reject {
                reason: "拒绝卡片".into(),
            }
        };
        assert!(h
            .approvals
            .answer_for_principal(
                &lingxi_kernel::Principal::LocalUser,
                sid,
                &pending[0].approval_id,
                decision
            )
            .settled());
        run.await.unwrap();
        let result = h.result(&cmd).await;
        let cards = h.state.interactions().cards(sid).await.unwrap();
        if sid == "card" {
            assert_eq!(result["ok"], true);
            assert!(result["details"]["cardId"]
                .as_str()
                .unwrap()
                .starts_with("c_"));
            assert_eq!(cards, vec![result["details"].clone()]);
        } else {
            assert_eq!(result["ok"], false);
            assert!(cards.is_empty());
        }
    }
    assert_eq!(std::fs::read_to_string(source).unwrap(), "原文件保持不变");
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_interactive_tools_current_tauri_components_real_http_and_iframe_sandbox() {
    let h = Harness::boot().await;
    for sid in ["confirm", "dismiss", "todos", "card", "denied"] {
        h.session_mode(sid, if sid == "denied" { "ask" } else { "operate" })
            .await;
    }
    let approvals = h.approvals.clone();
    let (stop, mut stopped) = tokio::sync::oneshot::channel();
    let adjudicator = tokio::spawn(async move {
        loop {
            tokio::select! {_=&mut stopped=>break,_=tokio::time::sleep(Duration::from_millis(20))=>{for sid in ["confirm","dismiss","todos","card","denied"]{for p in approvals.pending_for_principal(&lingxi_kernel::Principal::LocalUser,sid){let answer=if sid=="denied"{Answer::Reject{reason:"本次真实测试拒绝卡片".into()}}else{Answer::Approve};assert!(approvals.answer_for_principal(&lingxi_kernel::Principal::LocalUser,sid,&p.approval_id,answer).settled());}}}}
        }
    });
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let proof = h.home.join("interactive-ui-evidence");
    let target = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".cache/lingxi-r06-builds/tauri-resources");
    assert!(target.is_dir(), "只复用现有native target");
    let output = tokio::process::Command::new("node")
        .arg(repo.join("desktop/src/react/components/interactive/__tests__/run-live-ui.mjs"))
        .current_dir(&repo)
        .env("R06_INTERACTION_URL", &h.url)
        .env("R06_INTERACTION_TOKEN", &h.token)
        .env("R06_INTERACTION_PROOF", &proof)
        .env("R06_INTERACTION_NATIVE_TARGET", target)
        .output()
        .await
        .unwrap();
    std::fs::create_dir_all(&proof).unwrap();
    std::fs::write(proof.join("parent-stdout.log"), &output.stdout).unwrap();
    std::fs::write(proof.join("parent-stderr.log"), &output.stderr).unwrap();
    stop.send(()).unwrap();
    adjudicator.await.unwrap();
    assert!(
        output.status.success(),
        "真实界面失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let results = h.provider.results.lock().unwrap().clone();
    let confirmed = results
        .iter()
        .find(|(key, _)| key.starts_with("confirm|"))
        .unwrap()
        .1;
    assert_eq!(
        confirmed["details"]["answers"][0]["values"],
        json!(["safe"])
    );
    assert_eq!(
        confirmed["details"]["answers"][0]["labels"],
        json!(["稳妥"])
    );
    assert_eq!(
        confirmed["details"]["answers"][1]["values"],
        json!(["步骤二"])
    );
    assert_eq!(
        confirmed["details"]["answers"][2]["values"],
        json!(["界面真实输入"])
    );
    let dismissed = results
        .iter()
        .find(|(key, _)| key.starts_with("dismiss|"))
        .unwrap()
        .1;
    assert_eq!(dismissed["details"]["dismissed"], true);
    assert_eq!(dismissed["details"]["answers"], json!([]));
    let cards = h.state.interactions().cards("card").await.unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["title"], "隔离卡");
    assert!(cards[0]["code"]
        .as_str()
        .unwrap()
        .contains("parent.document"));
    assert!(h
        .state
        .interactions()
        .cards("denied")
        .await
        .unwrap()
        .is_empty());
    let old_home = h.home.clone();
    h.close().await;
    let restarted = Harness::boot_at(Some(old_home)).await;
    assert_eq!(
        restarted.state.interactions().cards("card").await.unwrap(),
        cards
    );
    let (_, snapshot) = restarted
        .req(
            "GET",
            "/lingxi/v1/sessions/interactions?sessionId=todos",
            Value::Null,
        )
        .await;
    assert_eq!(snapshot["todoPanel"]["todos"].as_array().unwrap().len(), 4);
    assert_eq!(
        snapshot["todoPanel"]["todos"][2]["blockedReason"],
        "等待确认"
    );
    assert!(restarted
        .state
        .interactions()
        .list_pending("confirm")
        .is_empty());
    println!("INTERACTION_CURRENT_PROOF {}", proof.display());
    restarted.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_fixed_ui_types_refuse_missing_run_foreign_owner_and_missing_user_anchor_before_model(
) {
    use lingxi_adapters::storage::session_tree as store;
    use lingxi_kernel::ports::StoragePort;
    let h = Harness::boot().await;
    for (i, entry_type) in [
        "todo-state-v1",
        "interactive-question-v1",
        "interactive-answer-v1",
        "interactive-card-v1",
    ]
    .iter()
    .enumerate()
    {
        for (j, case) in ["missing-run", "foreign-owner", "missing-anchor"]
            .iter()
            .enumerate()
        {
            let sid = format!("victim-{i}-{j}");
            h.session(&sid).await;
            let source = format!("source-{i}-{j}");
            let run_id = format!("source-run-{i}-{j}");
            let source_session = if *case == "foreign-owner" {
                store::seed_session(
                    h.state.storage(),
                    store::SeedSession {
                        session_id: &source,
                        agent_id: "agent",
                        owner_user_id: "foreign",
                        title: "邻会话",
                        now_unix_ms: 1,
                        permission_mode: Some("operate"),
                        authorized_folders: vec![],
                    },
                )
                .await
                .unwrap();
                source.clone()
            } else {
                sid.clone()
            };
            let ctx = lingxi_kernel::RunContext {
                principal: if *case == "foreign-owner" {
                    lingxi_kernel::Principal::Device {
                        user_id: "foreign".into(),
                        device_id: "foreign-device".into(),
                    }
                } else {
                    lingxi_kernel::Principal::LocalUser
                },
                session_id: lingxi_protocol::SessionId::new(source_session),
                run_id: lingxi_protocol::RunId::new(&run_id),
                attempt: lingxi_protocol::AttemptId::new(format!("{run_id}#a1")),
                generation: 1,
            };
            if *case != "missing-run" {
                h.state.storage().record_run_started(&ctx, 1).await.unwrap();
            }
            store::append_message(
                h.state.storage(),
                store::AppendMessage {
                    session_id: &sid,
                    message_id: &format!("tampered-{i}-{j}"),
                    parent_message_id: None,
                    run_id: if *case == "missing-run" {
                        None
                    } else {
                        Some(&run_id)
                    },
                    role: "custom",
                    content_json: "{}",
                    entry_type,
                    model_call_id: None,
                    now_unix_ms: 2,
                },
            )
            .await
            .unwrap();
            let history =
                lingxi_service::model_history::ModelHistory::new(h.state.storage().clone());
            let error = history
                .load(&lingxi_kernel::Principal::LocalUser, "agent", &sid, "")
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("interactive UI"),
                "{case}/{entry_type}: {error}"
            );
            let calls = h.provider.calls.load(std::sync::atomic::Ordering::SeqCst);
            let (_, run) = h.launch(&sid, "todo_write", json!({"todos":[]}));
            let response = run.await.unwrap();
            assert!(response.0 >= 400, "{case}/{entry_type}: {response:?}");
            assert_eq!(
                h.provider.calls.load(std::sync::atomic::Ordering::SeqCst),
                calls,
                "篡改条目不能到达模型"
            );
        }
    }
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_tool_ui_transaction_refuses_stale_attempt_generation_owner_and_terminal_before_write(
) {
    use lingxi_adapters::storage::session_tree as store;
    use lingxi_kernel::ports::{RunOutcome, StoragePort};
    let h = Harness::boot().await;
    h.session("fenced").await;
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("fenced"),
        run_id: lingxi_protocol::RunId::new("fenced-run"),
        attempt: lingxi_protocol::AttemptId::new("fenced-run#a1"),
        generation: 1,
    };
    h.state.storage().record_run_started(&ctx, 1).await.unwrap();
    for n in 0..5 {
        let mut wrong = ctx.clone();
        match n {
            0 => wrong.attempt = lingxi_protocol::AttemptId::new("old-attempt"),
            1 => wrong.generation = 2,
            2 => {
                wrong.principal = lingxi_kernel::Principal::Device {
                    user_id: "foreign".into(),
                    device_id: "other".into(),
                }
            }
            3 => wrong.session_id = lingxi_protocol::SessionId::new("different"),
            _ => {
                h.state
                    .storage()
                    .commit_run_outcome(
                        &ctx,
                        RunOutcome {
                            status: lingxi_protocol::RunStatus::Completed,
                            reason: None,
                            key_events: vec![],
                            final_message: None,
                        },
                        2,
                    )
                    .await
                    .unwrap();
            }
        }
        let event = store::unknown_payload(
            "interactive_card",
            json!({"cardId":"never"}).as_object().unwrap().clone(),
        );
        let result = store::append_tool_branch_entry(
            h.state.storage(),
            &wrong,
            store::AppendBranchEntry {
                session_id: "fenced",
                message_id: None,
                entry_type: "interactive-card-v1",
                role: "custom",
                content_json: "{}",
                now_unix_ms: 3,
            },
            event,
        )
        .await;
        assert!(result.is_err(), "伪身份 {n} 不应提交");
        assert!(store::list_branch_messages(h.state.storage(), "fenced")
            .await
            .unwrap()
            .is_empty());
        let count=h.state.storage().query_one_text("SELECT COUNT(*) FROM key_events WHERE stream_id=?1 AND event_type='interactive_card'",vec!["fenced".into()]).await.unwrap().unwrap();
        assert_eq!(count, "0");
    }
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_question_real_run_cancellation_clears_pending_without_fabricating_answers() {
    let h = Harness::boot().await;
    h.session("cancel-question").await;
    let (_, run) = h.launch(
        "cancel-question",
        "ask_user",
        json!({"questions":questions()}),
    );
    let pending = h.pending("cancel-question").await;
    let run_id = pending["runId"].as_str().unwrap();
    h.state.runs().cancel_run(run_id, "用户中止本轮");
    let _ = run.await.unwrap();
    for _ in 0..300 {
        if h.state
            .interactions()
            .list_pending("cancel-question")
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(h
        .state
        .interactions()
        .list_pending("cancel-question")
        .is_empty());
    let count=h.state.storage().query_one_text("SELECT COUNT(*) FROM messages WHERE session_id=?1 AND entry_type='interactive-answer-v1'",vec!["cancel-question".into()]).await.unwrap().unwrap();
    assert_eq!(count, "0");
    assert_eq!(h.req("POST","/lingxi/v1/sessions/interactions/answer",json!({"sessionId":"cancel-question","id":pending["id"],"action":"confirmed","values":{"mode":"fast"}})).await.0,409);
    h.close().await;
}
