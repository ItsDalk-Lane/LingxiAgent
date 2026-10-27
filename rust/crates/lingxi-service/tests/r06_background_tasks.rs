//! 外部模型响应可控；任务创建、正式工具、HTTP/WS、取消和存储均走生产链。
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        DelegationRequest, ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    RunContext,
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Semaphore,
};

enum Action {
    Tool(&'static str, Value),
    Final(&'static str),
}
struct Step {
    gate: Option<Arc<Semaphore>>,
    action: Action,
}
fn tool(name: &'static str, args: Value, gate: Option<&Arc<Semaphore>>) -> Step {
    Step {
        gate: gate.cloned(),
        action: Action::Tool(name, args),
    }
}
fn final_step(gate: Option<&Arc<Semaphore>>) -> Step {
    Step {
        gate: gate.cloned(),
        action: Action::Final("真实模型结果"),
    }
}
#[derive(Default)]
struct Provider {
    scripts: Mutex<HashMap<String, VecDeque<Step>>>,
    seen: Mutex<HashMap<String, Vec<Vec<ExchangeItem>>>>,
    tasks: Mutex<HashMap<String, String>>,
    contexts: Mutex<HashMap<String, RunContext>>,
}
impl Provider {
    fn script(&self, input: &str, steps: Vec<Step>) {
        self.scripts
            .lock()
            .unwrap()
            .insert(input.into(), steps.into());
    }
    fn count(&self, input: &str) -> usize {
        self.seen.lock().unwrap().get(input).map_or(0, Vec::len)
    }
    fn outcomes(&self, input: &str) -> Vec<ToolOutcome> {
        self.seen.lock().unwrap()[input]
            .last()
            .unwrap()
            .iter()
            .filter_map(|item| {
                if let ExchangeItem::ToolResult { outcome, .. } = item {
                    Some(outcome.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "local-response-fixture".into(),
            model: "controlled".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let text = input
            .submission
            .as_str()
            .lines()
            .next()
            .unwrap()
            .to_string();
        self.contexts
            .lock()
            .unwrap()
            .insert(text.clone(), ctx.clone());
        self.seen
            .lock()
            .unwrap()
            .entry(text.clone())
            .or_default()
            .push(input.prior.clone());
        let step = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&text)
            .unwrap_or_else(|| panic!("未知真实模型输入 {text}"))
            .pop_front()
            .expect("脚本未提前结束");
        Box::pin(async move {
            if let Some(gate) = step.gate {
                gate.acquire().await.unwrap().forget();
            }
            let turn = match step.action {
                Action::Final(text) => ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text { text: text.into() }],
                        model_call_id: None,
                    },
                },
                Action::Tool(name, mut args) => {
                    if args["task_id"] == "$own" {
                        args["task_id"] = json!(self.tasks.lock().unwrap()["a"]);
                    }
                    if args["task_id"] == "$foreign" {
                        args["task_id"] = json!(self.tasks.lock().unwrap()["b"]);
                    }
                    if args["threadId"] == "$thread" {
                        args["threadId"] = json!(self.tasks.lock().unwrap()["thread"]);
                    }
                    let declaration = input
                        .tools
                        .declarations
                        .iter()
                        .find(|d| d.wire_name == name)
                        .unwrap_or_else(|| panic!("正式目录缺少{name}"));
                    let mut request = ToolRequest::from_effective_arguments(
                        &declaration.target,
                        args.clone(),
                        &Default::default(),
                    )
                    .unwrap();
                    if name == "subagent" || name == "subagent_reply" {
                        request = request.with_delegation(DelegationRequest {
                            task: args["task"].as_str().unwrap().into(),
                            access: None,
                            label: None,
                            agent_id: None,
                            model: None,
                            thread_id: args["threadId"].as_str().map(str::to_string),
                        });
                    }
                    ProviderTurn::ToolRequests {
                        content: vec![],
                        requests: vec![request],
                    }
                }
            };
            ProviderTurnResult::of_ctx(ctx, turn)
        })
    }
}
struct Harness {
    state: ServiceState,
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn boot(provider: Arc<Provider>) -> Self {
        let home = std::env::temp_dir().join(format!(
            "r06-background-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let layout = prepare_layout(&home).unwrap();
        let home = home.canonicalize().unwrap();
        let registry = Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new());
        let clock = Arc::new(lingxi_service::inject::SystemClock);
        let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
            clock.clone(),
        ));
        let gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
            registry.clone(),
            approvals,
            clock,
            Default::default(),
            lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
            lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
        ));
        lingxi_service::subagents::register_tools(&registry, &gateway, &Default::default());
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 10000,
            },
            &layout,
            ServiceDeps {
                turn_provider: Some(provider),
                tool_gateway: Some(gateway),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, ready_rx) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let handle = tokio::spawn(async move {
            run(
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
        Self {
            state,
            addr: ready_rx.await.unwrap(),
            home,
            token,
            stop,
            handle,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value) -> (u16, Value) {
        http(self.addr, &self.token, method, path, body).await
    }
    fn start(&self, sid: &str, input: &str) -> tokio::task::JoinHandle<()> {
        let addr = self.addr;
        let token = self.token.clone();
        let path = format!("/lingxi/v1/sessions/{sid}/execute");
        let input = input.to_string();
        tokio::spawn(async move {
            let (code, body) = http(addr, &token, "POST", &path, json!({"input":input})).await;
            assert_eq!(code, 200, "{body}");
        })
    }
    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.handle.await.unwrap().unwrap();
        assert!(self.home.join("data").exists() || self.home.exists());
    }
}
async fn http(
    addr: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    body: Value,
) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let text = body.to_string();
    let request=format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",addr,token,text.len());
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut bytes = vec![];
    tokio::time::timeout(Duration::from_secs(15), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let boundary = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let header = String::from_utf8_lossy(&bytes[..boundary]);
    let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = serde_json::from_slice(&bytes[boundary + 4..])
        .unwrap_or_else(|e| panic!("HTTP正文解析失败 {e}: {}", String::from_utf8_lossy(&bytes)));
    (status, body)
}
async fn wait(mut condition: impl AsyncFnMut() -> bool) {
    let end = std::time::Instant::now() + Duration::from_secs(15);
    while !condition().await {
        assert!(std::time::Instant::now() < end, "真实任务未到指定状态");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
async fn tasks(h: &Harness, sid: &str) -> Vec<Value> {
    lingxi_adapters::storage::subagent_threads::background_tasks(h.state.storage(), Some(sid), None)
        .await
        .unwrap()
}
async fn child(h: &Harness, sid: &str) -> String {
    wait(async || !tasks(h, sid).await.is_empty()).await;
    tasks(h, sid).await[0]["taskId"].as_str().unwrap().into()
}
fn success(outcome: &ToolOutcome) -> Value {
    let ToolOutcome::Success { result } = outcome else {
        panic!("期望正式工具成功 {outcome:?}")
    };
    Value::Object(result.details.clone().unwrap())
}

async fn unavailable_runtime_tool(
    h: &Harness,
    ctx: &RunContext,
    name: &str,
    args: Value,
) -> ToolOutcome {
    let registry = Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new());
    let clock = Arc::new(lingxi_service::inject::SystemClock);
    let gateway = lingxi_service::toolgateway::ToolInvocationGateway::new(
        registry.clone(),
        Arc::new(lingxi_service::approval_service::ApprovalService::new(
            clock.clone(),
        )),
        clock,
        Default::default(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    );
    lingxi_service::background_tasks::register(
        &registry,
        &gateway,
        Arc::new(lingxi_service::background_tasks::TaskTools::new(
            h.state.storage().clone(),
            std::sync::Weak::new(),
            None,
        )),
        &Default::default(),
    );
    let request = ToolRequest::from_effective_arguments(
        format!("tool:first-party:{name}"),
        args,
        &Default::default(),
    )
    .unwrap();
    let call = lingxi_protocol::ToolCallId::new(format!("test-unavailable-{name}"));
    let prepared = gateway
        .prepare_from_request(
            ctx,
            lingxi_service::toolgateway::CallerSurface::UserRun,
            "agentA",
            lingxi_service::toolgateway::InvocationPermissionContext::UserSession {
                mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
            },
            &call,
            &request,
        )
        .unwrap();
    gateway
        .execute_prepared(ctx, &call, &prepared.handle)
        .await
        .unwrap()
        .outcome
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn model_pending_filters_stop_and_foreign_refusal_use_real_child_runs() {
    let provider = Arc::new(Provider::default());
    let child_a = Arc::new(Semaphore::new(0));
    let child_b = Arc::new(Semaphore::new(0));
    let parent_b = Arc::new(Semaphore::new(0));
    let gates = (0..8)
        .map(|_| Arc::new(Semaphore::new(0)))
        .collect::<Vec<_>>();
    provider.script(
        "parent-a",
        vec![
            tool("subagent", json!({"task":"child-a"}), None),
            tool("check_pending_tasks", json!({}), Some(&gates[0])),
            tool(
                "check_pending_tasks",
                json!({"status":"pending"}),
                Some(&gates[1]),
            ),
            tool("stop_task", json!({"task_id":"$own"}), Some(&gates[2])),
            tool(
                "check_pending_tasks",
                json!({"status":"failed"}),
                Some(&gates[3]),
            ),
            tool("stop_task", json!({"task_id":"$own"}), Some(&gates[4])),
            tool(
                "stop_task",
                json!({"task_id":"missing-background-task"}),
                Some(&gates[5]),
            ),
            tool("stop_task", json!({"task_id":"$foreign"}), Some(&gates[6])),
            final_step(Some(&gates[7])),
        ],
    );
    provider.script(
        "parent-b",
        vec![
            tool("subagent", json!({"task":"child-b"}), None),
            final_step(Some(&parent_b)),
        ],
    );
    provider.script("child-a", vec![final_step(Some(&child_a))]);
    provider.script("child-b", vec![final_step(Some(&child_b))]);
    let h = Harness::boot(provider.clone()).await;
    let parent_a = h.start("sess_local_alpha", "parent-a");
    let parent_b_task = h.start("sess_local_beta", "parent-b");
    let a = child(&h, "sess_local_alpha").await;
    let b = child(&h, "sess_local_beta").await;
    provider
        .tasks
        .lock()
        .unwrap()
        .extend([("a".into(), a.clone()), ("b".into(), b.clone())]);
    wait(async || provider.count("child-a") == 1 && provider.count("child-b") == 1).await;
    let ctx = provider.contexts.lock().unwrap()["parent-a"].clone();
    let unavailable = unavailable_runtime_tool(&h, &ctx, "stop_task", json!({"task_id":a})).await;
    assert_eq!(success(&unavailable)["reason"], "no_handler");
    assert_eq!(tasks(&h, "sess_local_alpha").await[0]["status"], "pending");
    let mut missing = ctx;
    missing.session_id = lingxi_protocol::SessionId::new("no-active-session");
    assert!(matches!(
        unavailable_runtime_tool(&h, &missing, "check_pending_tasks", json!({})).await,
        ToolOutcome::Failed { .. }
    ));
    for (index, gate) in gates.iter().enumerate().take(3) {
        gate.add_permits(1);
        wait(async || provider.count("parent-a") >= index + 3).await;
    }
    wait(async || tasks(&h, "sess_local_alpha").await[0]["status"] == "failed").await;
    let outcomes = provider.outcomes("parent-a");
    let all = success(&outcomes[1]);
    assert_eq!(all["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(all["tasks"][0]["taskId"], a);
    assert!(!all.to_string().contains(&b));
    let pending = success(&outcomes[2]);
    assert_eq!(pending["tasks"][0]["status"], "pending");
    assert_eq!(success(&outcomes[3])["status"], "aborted");
    for (index, gate) in gates.iter().enumerate().take(7).skip(3) {
        gate.add_permits(1);
        wait(async || provider.count("parent-a") >= index + 3).await;
    }
    let outcomes = provider.outcomes("parent-a");
    assert_eq!(success(&outcomes[4])["tasks"][0]["status"], "failed");
    assert_eq!(success(&outcomes[5])["status"], "already_stopped");
    assert_eq!(success(&outcomes[6])["reason"], "not_found");
    assert!(matches!(outcomes[7], ToolOutcome::Failed { .. }));
    assert_eq!(tasks(&h, "sess_local_beta").await[0]["status"], "pending");
    let before = h
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM key_events WHERE run_id=?1",
            vec![a.clone()],
        )
        .await
        .unwrap();
    child_a.add_permits(1);
    tokio::time::sleep(Duration::from_millis(75)).await;
    assert_eq!(
        h.state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM key_events WHERE run_id=?1",
                vec![a.clone()]
            )
            .await
            .unwrap(),
        before,
        "停止后旧模型不能产生新的结果或事件"
    );
    let cancelled = tasks(&h, "sess_local_alpha").await[0]["result"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        cancelled.contains("cancelled"),
        "保留真实取消原因：{cancelled}"
    );
    assert!(
        !cancelled.contains("真实模型结果"),
        "停止后不能写入迟到模型正文"
    );
    gates[7].add_permits(1);
    child_b.add_permits(1);
    wait(async || tasks(&h, "sess_local_beta").await[0]["status"] == "resolved").await;
    parent_b.add_permits(1);
    wait(async || {
        !h.state
            .sessions()
            .session_supervisor()
            .is_busy("sess_local_alpha")
            && !h
                .state
                .sessions()
                .session_supervisor()
                .is_busy("sess_local_beta")
    })
    .await;
    parent_a.await.unwrap();
    parent_b_task.await.unwrap();
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn continuation_preserves_old_task_result_and_old_stop_cannot_cancel_new_attempt() {
    let provider = Arc::new(Provider::default());
    let gates = (0..5)
        .map(|_| Arc::new(Semaphore::new(0)))
        .collect::<Vec<_>>();
    provider.script(
        "parent-a",
        vec![
            tool("subagent", json!({"task":"child-first"}), None),
            tool(
                "subagent_reply",
                json!({"threadId":"$thread","task":"child-second"}),
                Some(&gates[0]),
            ),
            tool(
                "check_pending_tasks",
                json!({"status":"resolved"}),
                Some(&gates[1]),
            ),
            tool("stop_task", json!({"task_id":"$own"}), Some(&gates[2])),
            final_step(Some(&gates[3])),
        ],
    );
    provider.script(
        "child-first",
        vec![Step {
            gate: None,
            action: Action::Final("第一轮真实独有结果"),
        }],
    );
    provider.script(
        "child-second",
        vec![Step {
            gate: Some(gates[4].clone()),
            action: Action::Final("第二轮真实独有结果"),
        }],
    );
    let h = Harness::boot(provider.clone()).await;
    let parent = h.start("sess_local_alpha", "parent-a");
    let first = child(&h, "sess_local_alpha").await;
    wait(async || {
        h.state
            .subagents()
            .threads_of("sess_local_alpha")
            .iter()
            .any(|t| !t.busy && t.last_run_status.is_some())
    })
    .await;
    let first_record = tasks(&h, "sess_local_alpha").await.remove(0);
    let thread = first_record["threadId"].as_str().unwrap().to_string();
    provider
        .tasks
        .lock()
        .unwrap()
        .extend([("a".into(), first.clone()), ("thread".into(), thread)]);
    gates[0].add_permits(1);
    wait(async || provider.count("child-second") == 1).await;
    gates[1].add_permits(1);
    wait(async || provider.count("parent-a") >= 4).await;
    let outcomes = provider.outcomes("parent-a");
    let resolved = success(&outcomes[2]);
    assert_eq!(resolved["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(resolved["tasks"][0]["taskId"], first);
    assert!(resolved["tasks"][0]["result"]
        .to_string()
        .contains("第一轮真实独有结果"));
    assert!(!resolved["tasks"][0]["result"]
        .to_string()
        .contains("第二轮真实独有结果"));
    gates[2].add_permits(1);
    wait(async || provider.count("parent-a") >= 5).await;
    assert_eq!(
        success(&provider.outcomes("parent-a")[3])["status"],
        "already_stopped"
    );
    assert!(tasks(&h, "sess_local_alpha")
        .await
        .iter()
        .any(|task| task["taskId"] != first && task["status"] == "pending"));
    gates[4].add_permits(1);
    wait(async || {
        tasks(&h, "sess_local_alpha")
            .await
            .iter()
            .all(|task| task["status"] == "resolved")
    })
    .await;
    gates[3].add_permits(1);
    parent.await.unwrap();
    h.stop().await;
}

async fn ws_send(stream: &mut TcpStream, value: Value) {
    let bytes = value.to_string().into_bytes();
    let mask = [2, 4, 6, 8];
    let mut frame = vec![0x81];
    if bytes.len() < 126 {
        frame.push(0x80 | u8::try_from(bytes.len()).unwrap());
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&u16::try_from(bytes.len()).unwrap().to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(bytes.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame).await.unwrap();
}
async fn ws_json(stream: &mut TcpStream) -> Value {
    match tokio::time::timeout(
        Duration::from_secs(5),
        lingxi_service::ws::read_ws_frame(stream),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap()
    {
        lingxi_service::ws::WsFrame::Text(bytes) => serde_json::from_slice(&bytes).unwrap(),
        frame => panic!("非结果帧{frame:?}"),
    }
}
async fn ws_open(h: &Harness, token: &str) -> TcpStream {
    let mut stream = TcpStream::connect(h.addr).await.unwrap();
    stream.write_all(format!("GET /lingxi/v1/ws HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {token}\r\n\r\n",h.addr).as_bytes()).await.unwrap();
    let mut header = vec![];
    let mut b = [0];
    while !header.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut b).await.unwrap();
        header.push(b[0]);
    }
    assert!(header.starts_with(b"HTTP/1.1 101"));
    ws_send(&mut stream,json!({"protocol":"lingxi.wire","clientKind":"test","clientVersion":"0","protocolMin":1,"protocolMax":1})).await;
    assert_eq!(ws_json(&mut stream).await["protocol"], "lingxi.wire");
    stream
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_and_websocket_stop_owned_only_and_reject_stale_locators() {
    let provider = Arc::new(Provider::default());
    let release = Arc::new(Semaphore::new(0));
    for name in ["a", "b"] {
        provider.script(
            &format!("parent-{name}"),
            vec![
                tool("subagent", json!({"task":format!("child-{name}")}), None),
                final_step(Some(&release)),
            ],
        );
        provider.script(&format!("child-{name}"), vec![final_step(Some(&release))]);
    }
    let h = Harness::boot(provider.clone()).await;
    let parent_a = h.start("sess_local_alpha", "parent-a");
    let parent_b_task = h.start("sess_local_beta", "parent-b");
    let a = child(&h, "sess_local_alpha").await;
    let b = child(&h, "sess_local_beta").await;
    wait(async || provider.count("child-a") == 1 && provider.count("child-b") == 1).await;
    let (code, value) = h
        .request(
            "POST",
            &format!("/lingxi/v1/task/{b}/abort"),
            json!({"sessionId":"sess_local_alpha"}),
        )
        .await;
    assert_eq!(code, 403, "{value}");
    assert_eq!(value["reason"], "session_mismatch");
    let (code, value) = h
        .request(
            "POST",
            "/lingxi/v1/task/missing/abort",
            json!({"sessionId":"sess_local_alpha"}),
        )
        .await;
    assert_eq!(code, 404, "{value}");
    assert_eq!(value["reason"], "not_found");
    let (code, _) = h
        .request(
            "POST",
            &format!("/lingxi/v1/task/{a}/abort"),
            json!({"sessionId":"sess_local_alpha","sessionPath":"native:sess_local_beta"}),
        )
        .await;
    assert_eq!(code, 400);
    let (code, credential) = h
        .request(
            "POST",
            "/lingxi/v1/devices/credentials",
            json!({"userId":"foreign-task-user","scopes":["chat"]}),
        )
        .await;
    assert_eq!(code, 201);
    let token = credential["secret"].as_str().unwrap();
    let (code, result) = http(
        h.addr,
        token,
        "POST",
        &format!("/lingxi/v1/task/{a}/abort"),
        json!({"sessionId":"sess_local_alpha"}),
    )
    .await;
    assert_eq!(code, 403, "{result}");
    let mut stranger = ws_open(&h, token).await;
    ws_send(&mut stranger,json!({"type":"subagent_stop_request","sessionId":"sess_local_alpha","taskId":a,"requestId":"foreign-principal"})).await;
    let result = ws_json(&mut stranger).await;
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["reason"], "session_forbidden");
    drop(stranger);
    let mut socket = ws_open(&h, &h.token).await;
    for body in [
        json!({"sessionId":"missing"}),
        json!({"sessionId":"sess_local_beta"}),
    ] {
        let mut value = body;
        value["type"] = json!("subagent_stop_request");
        value["taskId"] = json!(a);
        value["requestId"] = json!("negative");
        ws_send(&mut socket, value).await;
        let result = ws_json(&mut socket).await;
        assert_eq!(result["type"], "subagent_stop_result");
        assert_eq!(result["requestId"], "negative");
        assert_eq!(result["status"], "rejected");
    }
    assert_eq!(tasks(&h, "sess_local_alpha").await[0]["status"], "pending");
    assert_eq!(tasks(&h, "sess_local_beta").await[0]["status"], "pending");
    ws_send(&mut socket,json!({"type":"subagent_stop_request","sessionPath":"native:sess_local_alpha","taskId":a,"requestId":"real-stop"})).await;
    let value = ws_json(&mut socket).await;
    assert_eq!(value["status"], "aborted");
    assert_eq!(value["sessionId"], "sess_local_alpha");
    assert_eq!(value["requestId"], "real-stop");
    wait(async || tasks(&h, "sess_local_alpha").await[0]["status"] == "failed").await;
    assert_eq!(tasks(&h, "sess_local_beta").await[0]["status"], "pending");
    let (code, value) = h
        .request(
            "POST",
            &format!("/lingxi/v1/task/{b}/abort"),
            json!({"sessionPath":"native:sess_local_beta"}),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(value["status"], "aborted");
    wait(async || tasks(&h, "sess_local_beta").await[0]["status"] == "failed").await;
    let (code, value) = h
        .request(
            "POST",
            &format!("/lingxi/v1/task/{b}/abort"),
            json!({"sessionId":"sess_local_beta"}),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(value["status"], "already_stopped");
    release.add_permits(10);
    drop(socket);
    wait(async || {
        !h.state
            .sessions()
            .session_supervisor()
            .is_busy("sess_local_alpha")
            && !h
                .state
                .sessions()
                .session_supervisor()
                .is_busy("sess_local_beta")
    })
    .await;
    parent_a.await.unwrap();
    parent_b_task.await.unwrap();
    h.stop().await;
}
