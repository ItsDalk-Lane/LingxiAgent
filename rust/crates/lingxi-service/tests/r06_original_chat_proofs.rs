//! 原聊天/子代理义务的专属补证：真实服务、数据库、连接与工具循环。
//! 外部模型替身仅提供响应；权限、登记、执行和持久化均由生产入口决定。

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_kernel::model_exchange::{ExchangeItem, ModelTurnInput};
use lingxi_kernel::ports::{
    DelegationRequest, EventStorePort, ProviderDescriptor, ProviderTurn, ProviderTurnResult,
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, TurnDeltaSink,
    TurnProviderPort,
};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, EventEnvelope, EventPayload, KnownEventPayload, ModelCallId,
    NormalizedMessage, Seq, ToolCallId, ToolResultStatus, ToolResultWire, ToolSchemaDocument,
};
use lingxi_service::toolgateway::ToolInvocationGateway;
use lingxi_service::ws::WsFrame;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Fixture {
    home: PathBuf,
    state: ServiceState,
    addr: SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}

impl Fixture {
    async fn start(tag: &str, deps: ServiceDeps) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-original-chat-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
            },
            &layout,
            deps,
        )
        .await
        .unwrap();
        let (stop, stop_rx) = tokio::sync::oneshot::channel();
        let (ready, ready_rx) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let shutdown = state.ws_shutdown();
        let task = tokio::spawn(async move {
            run(
                serving,
                async move {
                    let _ = stop_rx.await;
                    shutdown.request_close();
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        let addr = ready_rx.await.unwrap();
        Self {
            home,
            state,
            addr,
            stop,
            task,
        }
    }

    async fn http(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        token: &str,
    ) -> (u16, Value) {
        http_actual(self.addr, method, path, body, token).await
    }

    async fn events(&self, session: &str) -> Vec<EventEnvelope> {
        self.state
            .storage()
            .stream_events_after(session, Seq::new(0), 10_000)
            .await
            .unwrap()
    }

    async fn session_view(&self, session: &str) -> Value {
        let response = self
            .http(
                "GET",
                &format!("/lingxi/v1/sessions/{session}"),
                None,
                &self.state.auth().local_token(),
            )
            .await;
        assert_eq!(response.0, 200);
        response.1
    }

    async fn child_count(&self, session: &str) -> String {
        self.state
            .storage()
            .query_one_text(
                "SELECT COUNT(*) FROM runs r JOIN run_lineage l ON l.run_id = r.run_id \
             WHERE r.session_id = ?1 AND l.parent_run_id IS NOT NULL",
                vec![session.to_owned()],
            )
            .await
            .unwrap()
            .unwrap()
    }

    async fn finish(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        self.state.storage().close().await.unwrap();
        std::fs::remove_dir_all(self.home).unwrap();
    }
}

async fn http_actual(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
    token: &str,
) -> (u16, Value) {
    let body = body.map(|v| v.to_string()).unwrap_or_default();
    let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            addr, body.len()
        );
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    let split = response.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
    let headers = std::str::from_utf8(&response[..split]).unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (
        status,
        serde_json::from_slice(&response[split + 4..]).unwrap(),
    )
}

async fn wait_until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn ws_connect(fx: &Fixture, token: &str) -> TcpStream {
    let mut stream = TcpStream::connect(fx.addr).await.unwrap();
    let request = format!(
        "GET /lingxi/v1/ws HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {token}\r\n\r\n",
        fx.addr
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut header = Vec::new();
    let mut byte = [0];
    while !header.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        header.push(byte[0]);
    }
    assert!(header.starts_with(b"HTTP/1.1 101"));
    ws_send(
        &mut stream,
        json!({"protocol":"lingxi.wire","clientKind":"test",
        "clientVersion":"0","protocolMin":1,"protocolMax":1}),
    )
    .await;
    let hello = ws_json(&mut stream).await;
    assert_eq!(hello["protocol"], "lingxi.wire");
    stream
}

async fn ws_send(stream: &mut TcpStream, value: Value) {
    let payload = value.to_string().into_bytes();
    let mask = [0x12, 0x34, 0x56, 0x78];
    let mut frame = vec![0x81];
    if payload.len() < 126 {
        frame.push(0x80 | u8::try_from(payload.len()).unwrap());
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, v)| v ^ mask[i % 4]));
    stream.write_all(&frame).await.unwrap();
}

async fn ws_read(stream: &mut TcpStream) -> WsFrame {
    tokio::time::timeout(
        Duration::from_secs(5),
        lingxi_service::ws::read_ws_frame(stream),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap()
}

async fn ws_json(stream: &mut TcpStream) -> Value {
    match ws_read(stream).await {
        WsFrame::Text(bytes) => serde_json::from_slice(&bytes).unwrap(),
        frame => panic!("expected actual text frame: {frame:?}"),
    }
}

async fn subscribe_actual(stream: &mut TcpStream, session: &str, expected: &[EventEnvelope]) {
    ws_send(
        stream,
        json!({"type":"subscribe_events","streamId":session}),
    )
    .await;
    let control = ws_json(stream).await;
    assert_eq!(control["type"], "subscribed");
    assert_eq!(control["streamId"], session);
    for event in expected {
        assert_eq!(ws_json(stream).await, serde_json::to_value(event).unwrap());
    }
}

#[tokio::test]
async fn websocket_registration_and_foreign_messages_preserve_authorized_session_streams() {
    let fx = Fixture::start("ws-registration", ServiceDeps::default()).await;
    let token = fx.state.auth().local_token();
    assert_eq!(fx.state.ws_connection_count(), 0);
    assert_eq!(fx.state.ws_shutdown().open_count(), 0);
    for session in ["sess_local_alpha", "sess_local_beta"] {
        assert_eq!(
            fx.http(
                "POST",
                &format!("/lingxi/v1/sessions/{session}/execute"),
                Some(json!({"input":format!("seed {session}")})),
                &token
            )
            .await
            .0,
            200
        );
    }
    let mut alpha = ws_connect(&fx, &token).await;
    assert_eq!(fx.state.ws_connection_count(), 1);
    assert_eq!(fx.state.ws_shutdown().open_count(), 1);
    let mut beta = ws_connect(&fx, &token).await;
    assert_eq!(fx.state.ws_connection_count(), 2);
    assert_eq!(fx.state.ws_shutdown().open_count(), 2);
    let alpha_events = fx.events("sess_local_alpha").await;
    let beta_events = fx.events("sess_local_beta").await;
    assert!(!alpha_events.is_empty() && !beta_events.is_empty());
    subscribe_actual(&mut alpha, "sess_local_alpha", &alpha_events).await;
    subscribe_actual(&mut beta, "sess_local_beta", &beta_events).await;
    let alpha_view = fx.session_view("sess_local_alpha").await;
    let beta_view = fx.session_view("sess_local_beta").await;
    let registered = fx.state.events().hub().hub_stats().live_subscribers;
    assert_eq!(registered, 2);

    // 凭证由实际管理入口签发；同一外来身份分别走读取和订阅拒绝。
    let issued = fx
        .http(
            "POST",
            "/lingxi/v1/devices/credentials",
            Some(json!({"userId":"user_proof_foreign","scopes":["chat"]})),
            &token,
        )
        .await;
    assert_eq!(issued.0, 201);
    let foreign_token = issued.1["secret"].as_str().unwrap();
    let me = fx.http("GET", "/lingxi/v1/me", None, foreign_token).await;
    assert_eq!(me.0, 200);
    assert_eq!(me.1["userId"], "user_proof_foreign");
    for request in [
        json!({"type":"session_read","sessionId":"sess_local_alpha"}),
        json!({"type":"subscribe_events","streamId":"sess_local_beta"}),
    ] {
        let mut foreign = ws_connect(&fx, foreign_token).await;
        assert_eq!(fx.state.ws_connection_count(), 3);
        assert_eq!(fx.state.ws_shutdown().open_count(), 3);
        ws_send(&mut foreign, request).await;
        let denied = ws_json(&mut foreign).await;
        assert_eq!(denied["code"], "forbidden", "{denied}");
        assert!(matches!(
            ws_read(&mut foreign).await,
            WsFrame::Close(4403, _)
        ));
        drop(foreign);
        wait_until(|| {
            fx.state.ws_connection_count() == 2 && fx.state.ws_shutdown().open_count() == 2
        })
        .await;
        assert_eq!(
            fx.state.events().hub().hub_stats().live_subscribers,
            registered
        );
        assert_eq!(fx.session_view("sess_local_alpha").await, alpha_view);
        assert_eq!(fx.session_view("sess_local_beta").await, beta_view);
        assert_eq!(fx.events("sess_local_alpha").await, alpha_events);
        assert_eq!(fx.events("sess_local_beta").await, beta_events);
        assert!(tokio::time::timeout(
            Duration::from_millis(75),
            lingxi_service::ws::read_ws_frame(&mut alpha)
        )
        .await
        .is_err());
        assert!(tokio::time::timeout(
            Duration::from_millis(75),
            lingxi_service::ws::read_ws_frame(&mut beta)
        )
        .await
        .is_err());
    }

    // 拒绝之后仍能按所属会话转发实际新事件，且不能广播给另一会话。
    assert_eq!(
        fx.http(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"authorized live alpha"})),
            &token
        )
        .await
        .0,
        200
    );
    let after = fx.events("sess_local_alpha").await;
    let live = &after[alpha_events.len()..];
    assert!(!live.is_empty());
    for event in live {
        assert_eq!(
            ws_json(&mut alpha).await,
            serde_json::to_value(event).unwrap()
        );
    }
    assert!(tokio::time::timeout(
        Duration::from_millis(75),
        lingxi_service::ws::read_ws_frame(&mut beta)
    )
    .await
    .is_err());
    assert_eq!(fx.events("sess_local_beta").await, beta_events);
    drop(alpha);
    drop(beta);
    wait_until(|| fx.state.ws_connection_count() == 0 && fx.state.ws_shutdown().open_count() == 0)
        .await;
    assert_eq!(fx.state.events().hub().hub_stats().live_subscribers, 0);
    fx.finish().await;
}

enum Step {
    Final(&'static str),
    Call { target: &'static str, arguments: Value },
    Panic(&'static str),
    Failed(&'static str),
    Delegate {
        target: &'static str,
        task: &'static str,
        thread_key: Option<&'static str>,
    },
}

struct ScriptedProvider {
    scripts: Mutex<HashMap<String, VecDeque<Step>>>,
    threads: Mutex<HashMap<String, String>>,
    calls: Mutex<Vec<(String, ModelTurnInput)>>,
    gates: HashMap<(String, u32), Arc<tokio::sync::Semaphore>>,
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "proof.scripted".into(),
            model: "proof.model".into(),
            operation: "chat".into(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _sink: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let marker = input.submission.split(':').next().unwrap().to_owned();
        let step = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&marker)
            .unwrap()
            .pop_front()
            .unwrap();
        let gate = self.gates.get(&(marker.clone(), input.turn)).cloned();
        self.calls.lock().unwrap().push((marker, input.clone()));
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.acquire().await.unwrap().forget();
            }
            let turn = match step {
                Step::Final(text) => ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text { text: text.into() }],
                        model_call_id: None,
                    },
                },
                Step::Panic(reason)=>panic!("{reason}"),
                Step::Failed(reason)=>ProviderTurn::Failed {error:lingxi_protocol::ProtocolError::new(ErrorCode::UpstreamUnavailable,reason,false),retryable:false},
                Step::Call {target,arguments}=>{
                    let declaration=input.tools.declarations.iter().find(|d|d.wire_name==target).expect("正式工具须在当轮实际发送声明中");
                    assert!(input.tools.catalog_generation>0);
                    ProviderTurn::ToolRequests {content:Vec::new(),requests:vec![ToolRequest::from_effective_arguments(&declaration.target,arguments,&SchemaBudget::default()).unwrap()]}
                }
                Step::Delegate {
                    target,
                    task,
                    thread_key,
                } => {
                    let thread_id = thread_key.map(|key| self.threads.lock().unwrap()[key].clone());
                    // 使用当轮实际发送目录，不自造声明代次或目标身份。
                    let declaration = input
                        .tools
                        .declarations
                        .iter()
                        .find(|declaration| declaration.wire_name == target)
                        .expect("实际发送目录须包含原子代理入口");
                    assert!(input.tools.catalog_generation > 0);
                    let mut arguments = match target {
                        "subagent_close"=>json!({"reason":task}),
                        "subagent_reply"=>json!({"task":task}),
                        _=>json!({"task":task,"label":task}),
                    };
                    if let Some(thread_id) = &thread_id {
                        arguments["threadId"] = json!(thread_id);
                    }
                    ProviderTurn::ToolRequests {
                        content: Vec::new(),
                        requests: vec![ToolRequest::from_effective_arguments(
                            &declaration.target,
                            arguments,
                            &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                        )
                        .unwrap()
                        .with_summary(format!("{target}: {task}"))],
                    }
                }
            };
            ProviderTurnResult::of_ctx(ctx, turn)
        })
    }
}

fn registered_delegation_gateway() -> (Arc<ToolInvocationGateway>,Arc<ToolRegistry>) {
    let registry=Arc::new(ToolRegistry::new());
    let clock=Arc::new(lingxi_service::inject::SystemClock);
    let approvals=Arc::new(lingxi_service::approval_service::ApprovalService::new(clock.clone()));
    let gateway=Arc::new(ToolInvocationGateway::new(registry.clone(),approvals,clock,SchemaBudget::default(),lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP));
    lingxi_service::subagents::register_tools(&registry,&gateway,&SchemaBudget::default());
    (gateway,registry)
}

fn delegation_gateway() -> Arc<ToolInvocationGateway> {registered_delegation_gateway().0}

fn scripted(scripts:Vec<(&str,Vec<Step>)>,gates:Vec<((&str,u32),Arc<tokio::sync::Semaphore>)>) -> Arc<ScriptedProvider> {
    Arc::new(ScriptedProvider {
        scripts:Mutex::new(scripts.into_iter().map(|(key,steps)|(key.into(),steps.into())).collect()),
        threads:Mutex::new(HashMap::new()),calls:Mutex::new(Vec::new()),
        gates:gates.into_iter().map(|((marker,turn),gate)|((marker.into(),turn),gate)).collect(),
    })
}

fn outcome_json(outcome:&ToolOutcome)->Value {
    match outcome {
        ToolOutcome::Success {result}=>{
            let text=result.content.iter().filter_map(|c|match c{ContentBlock::Text{text}=>Some(text.as_str()),_=>None}).collect::<String>();
            serde_json::from_str(&text).unwrap()
        }
        other=>panic!("须返回真实成功正文：{other:?}"),
    }
}

async fn execute_actual(addr: SocketAddr, token: &str, session: &str, text: &str) -> String {
    let (status, response) = http_actual(
        addr,
        "POST",
        &format!("/lingxi/v1/sessions/{session}/execute"),
        Some(json!({"input":text})),
        token,
    )
    .await;
    assert_eq!(status, 200, "真实HTTP执行返回：{response}");
    response["runId"].as_str().unwrap().to_owned()
}

fn tool_results(events: &[EventEnvelope], run: &str) -> Vec<ToolResultWire> {
    events
        .iter()
        .filter(|event| event.run_id.as_ref().map(|id| id.as_str()) == Some(run))
        .filter_map(|event| match &event.payload {
            EventPayload::Known(KnownEventPayload::ToolCallCompleted(payload)) => {
                Some(payload.result.clone())
            }
            _ => None,
        })
        .collect()
}

fn parent_tool_outcomes(provider: &ScriptedProvider, marker: &str, run: &str) -> Vec<ToolOutcome> {
    provider
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(key, _)| key == marker)
        .last()
        .unwrap()
        .1
        .prior
        .iter()
        .filter_map(|item| match item {
            ExchangeItem::ToolResult {
                tool_call_id,
                outcome,
                ..
            } if tool_call_id.as_str().starts_with(&format!("{run}-tc")) => Some(outcome.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn subagent_close_and_reply_refusals_preserve_other_threads_and_sessions() {
    use Step::{Delegate, Final};
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = Arc::new(ScriptedProvider {
        scripts: Mutex::new(
            [
                (
                    "CREATE",
                    vec![
                        Delegate {
                            target: "subagent",
                            task: "CHILD-A: first",
                            thread_key: None,
                        },
                        Delegate {
                            target: "subagent",
                            task: "CHILD-B: second",
                            thread_key: None,
                        },
                        Final("created two"),
                    ],
                ),
                ("CHILD-A", vec![Final("actual result A")]),
                ("CHILD-B", vec![Final("actual result B")]),
                (
                    "FOREIGN",
                    vec![
                        Delegate {
                            target: "subagent_close",
                            task: "foreign close",
                            thread_key: Some("B"),
                        },
                        Delegate {
                            target: "subagent_reply",
                            task: "foreign reply",
                            thread_key: Some("B"),
                        },
                        Final("foreign refused"),
                    ],
                ),
                (
                    "CLOSE",
                    vec![
                        Delegate {
                            target: "subagent_close",
                            task: "close A only",
                            thread_key: Some("A"),
                        },
                        Delegate {
                            target: "subagent_close",
                            task: "unknown",
                            thread_key: Some("missing"),
                        },
                        Delegate {
                            target: "subagent_close",
                            task: "repeat close",
                            thread_key: Some("A"),
                        },
                        Delegate {
                            target: "subagent_reply",
                            task: "closed reply",
                            thread_key: Some("A"),
                        },
                        Delegate {
                            target: "subagent_reply",
                            task: "unknown reply",
                            thread_key: Some("missing"),
                        },
                        Final("closed and refused"),
                    ],
                ),
            ]
            .into_iter()
            .map(|(key, steps)| (key.into(), steps.into()))
            .collect(),
        ),
        threads: Mutex::new(HashMap::from([(
            "missing".into(),
            "thread-does-not-exist".into(),
        )])),
        calls: Mutex::new(Vec::new()),
        gates: HashMap::from([(("CREATE".into(), 3), gate.clone())]),
    });
    let fx = Fixture::start(
        "subagent-boundaries",
        ServiceDeps {
            turn_provider: Some(provider.clone()),
            tool_gateway: Some(delegation_gateway()),
            ..ServiceDeps::default()
        },
    )
    .await;
    let addr = fx.addr;
    let token = fx.state.auth().local_token();
    let task_token = token.clone();
    let create = tokio::spawn(async move {
        execute_actual(
            addr,
            &task_token,
            "sess_local_alpha",
            "CREATE: two children",
        )
        .await
    });
    wait_until(|| {
        let threads = fx.state.subagents().threads_of("sess_local_alpha");
        threads.len() == 2
            && threads
                .iter()
                .all(|t| !t.busy && t.last_run_status.is_some())
    })
    .await;
    let before = fx.state.subagents().threads_of("sess_local_alpha");
    let a = before
        .iter()
        .find(|t| t.label.as_deref() == Some("CHILD-A: first"))
        .unwrap()
        .clone();
    let b = before
        .iter()
        .find(|t| t.label.as_deref() == Some("CHILD-B: second"))
        .unwrap()
        .clone();
    provider.threads.lock().unwrap().extend([
        ("A".into(), a.thread_id.clone()),
        ("B".into(), b.thread_id.clone()),
    ]);
    gate.add_permits(1);
    let create_run = create.await.unwrap();
    assert_eq!(fx.child_count("sess_local_alpha").await, "2");
    assert_eq!(
        tool_results(&fx.events("sess_local_alpha").await, &create_run).len(),
        2
    );

    let alpha_events = fx.events("sess_local_alpha").await;
    let alpha_view = fx.session_view("sess_local_alpha").await;
    let foreign_run = execute_actual(
        addr,
        &token,
        "sess_local_beta",
        "FOREIGN: try another parent",
    )
    .await;
    assert_eq!(fx.state.subagents().threads_of("sess_local_alpha"), before);
    assert_eq!(fx.child_count("sess_local_alpha").await, "2");
    assert_eq!(fx.child_count("sess_local_beta").await, "0");
    assert!(fx
        .state
        .subagents()
        .threads_of("sess_local_beta")
        .is_empty());
    assert_eq!(fx.events("sess_local_alpha").await, alpha_events);
    assert_eq!(fx.session_view("sess_local_alpha").await, alpha_view);
    let refused = tool_results(&fx.events("sess_local_beta").await, &foreign_run);
    assert_eq!(refused.len(), 2);
    for result in refused {
        assert_eq!(result.status, ToolResultStatus::Failed);
        let error = result.error.unwrap();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert!(error.message.contains(&b.thread_id) && error.message.contains("sess_local_beta"));
    }
    let parent_refusals = parent_tool_outcomes(&provider, "FOREIGN", &foreign_run);
    assert_eq!(parent_refusals.len(), 2);
    assert!(parent_refusals.iter().all(|result| matches!(result,
        ToolOutcome::Failed { error } if error.code == ErrorCode::Forbidden)));

    let beta_events = fx.events("sess_local_beta").await;
    let beta_view = fx.session_view("sess_local_beta").await;
    let closed_run = execute_actual(addr, &token, "sess_local_alpha", "CLOSE: only A").await;
    let after = fx.state.subagents().threads_of("sess_local_alpha");
    assert_eq!(after.len(), 2);
    assert_eq!(
        after.iter().find(|t| t.thread_id == b.thread_id).unwrap(),
        &b
    );
    let closed = after.iter().find(|t| t.thread_id == a.thread_id).unwrap();
    assert_eq!(closed.status.wire_name(), "closed");
    assert_eq!(closed.child_run_id, a.child_run_id);
    assert_eq!(
        closed.last_run_status.as_deref(),
        Some("closed: close A only")
    );
    assert_eq!(fx.child_count("sess_local_alpha").await, "2");
    assert_eq!(fx.child_count("sess_local_beta").await, "0");
    assert_eq!(fx.events("sess_local_beta").await, beta_events);
    assert_eq!(fx.session_view("sess_local_beta").await, beta_view);
    let results = tool_results(&fx.events("sess_local_alpha").await, &closed_run);
    assert_eq!(results.len(), 5);
    assert_eq!(results[0].status, ToolResultStatus::Success);
    let closed_text=match &results[0].content[0] {ContentBlock::Text{text}=>text,other=>panic!("closed正文：{other:?}")};
    let closed_ack:Value=serde_json::from_str(closed_text).unwrap();
    assert_eq!(closed_ack["threadId"],a.thread_id);
    assert_eq!(closed_ack["streamStatus"],"closed");
    assert_eq!(closed_ack["status"],"closed");
    let persisted=fx.state.storage().query_one_text("SELECT state_json FROM subagent_threads_v1 WHERE thread_id=?1",vec![a.thread_id.clone()]).await.unwrap().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&persisted).unwrap()["streamStatus"],"closed");
    for (index, code, id) in [
        (1, ErrorCode::NotFound, "thread-does-not-exist"),
        (2, ErrorCode::Conflict, a.thread_id.as_str()),
        (3, ErrorCode::Conflict, a.thread_id.as_str()),
        (4, ErrorCode::NotFound, "thread-does-not-exist"),
    ] {
        assert_eq!(results[index].status, ToolResultStatus::Failed);
        let error = results[index].error.as_ref().unwrap();
        assert_eq!(error.code, code);
        assert!(error.message.contains(id));
    }
    // 父模型收到与正式事件相同的最终closed状态，不只收到线程身份。
    let parent_results=parent_tool_outcomes(&provider,"CLOSE",&closed_run);
    assert_eq!(parent_results.len(),5);
    assert_eq!(outcome_json(&parent_results[0]),closed_ack);
    assert!(
        matches!(&parent_results[1], ToolOutcome::Failed { error } if error.code == ErrorCode::NotFound)
    );
    assert!(parent_results[2..4].iter().all(|result| matches!(result,
        ToolOutcome::Failed { error } if error.code == ErrorCode::Conflict)));
    assert!(
        matches!(&parent_results[4], ToolOutcome::Failed { error } if error.code == ErrorCode::NotFound)
    );
    // 子结果正文、线程和运行归属均来自真实完成记录，并只进入所属父会话。
    let calls = provider.calls.lock().unwrap().clone();
    for (thread, body) in [(&a, "actual result A"), (&b, "actual result B")] {
        let delivered = format!(
            "[subagent-result thread={} run={}] {body}",
            thread.thread_id,
            thread.child_run_id.as_ref().unwrap()
        );
        assert!(calls.iter().any(|(marker, input)| {
            matches!(marker.as_str(), "CREATE" | "CLOSE") && input.submission.contains(&delivered)
        }));
        assert!(calls
            .iter()
            .filter(|(marker, _)| marker == "FOREIGN")
            .all(|(_, input)| !input.submission.contains(&delivered)));
    }
    fx.finish().await;
}

struct CountWrite(Arc<std::sync::atomic::AtomicUsize>);
impl ToolExecutorPort for CountWrite {
    fn execute<'a>(&'a self,ctx:&'a RunContext,_call:&'a ToolCallId,_request:&'a ToolRequest)->Pin<Box<dyn std::future::Future<Output=ToolExecutionResult>+Send+'a>> {
        self.0.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {ToolExecutionResult::of_ctx(ctx,ToolOutcome::success_text("真实外部执行替身"))})
    }
}
fn gateway_with_write(count:Arc<std::sync::atomic::AtomicUsize>)->Arc<ToolInvocationGateway> {
    let (gateway,registry)=registered_delegation_gateway();
    let receipt=registry.register(ToolManifest {
        origin:ToolOrigin::FirstParty,local_name:"write".into(),display_name:"write".into(),aliases:Vec::new(),version:"1.0.0".into(),description:"原权限交集检查的写工具".into(),
        input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema:json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false})},output_schema:None,
        permission:PermissionContract{kind:PermissionKind::Execute,capability_base:"write.capability".into()},availability:Availability::Available,timeout_ms:Some(30_000),max_concurrency:Some(4),declared_permission:DeclaredPermission::None,recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    },&SchemaBudget::default()).unwrap();
    gateway.bind_executor(receipt.target_id,Arc::new(CountWrite(count)),"真实写入边界计数器");
    gateway
}

fn call(target:&'static str,arguments:Value)->Step {Step::Call {target,arguments}}
fn submission_has(provider:&ScriptedProvider,marker:&str,part:&str)->bool {
    provider.calls.lock().unwrap().iter().filter(|(key,_)|key==marker).any(|(_,input)|input.submission.contains(part))
}
async fn create_owned_session(fx:&Fixture,id:&str,mode:&str) {
    let row=fx.state.storage().get_session("sess_local_alpha").await.unwrap().unwrap();
    let (status,body)=fx.http("POST","/lingxi/v1/sessions",Some(json!({"sessionId":id,"agentId":row.agent_id,"title":id,"permissionMode":mode})),&fx.state.auth().local_token()).await;
    assert_eq!(status,201,"{body}");
}

#[tokio::test]
async fn subagent_list_launch_progress_and_permission_intersection_use_real_facts() {
    let parent_gate=Arc::new(tokio::sync::Semaphore::new(0));
    let child_gate=Arc::new(tokio::sync::Semaphore::new(0));
    let writes=Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let provider=scripted(vec![
        ("EMPTY",vec![call("subagent",json!({"agent":"?","task":""})),Step::Final("候选为空已如实说明")]),
        ("LIST",vec![call("subagent",json!({"agent":"list","task":""})),Step::Final("读到真实候选")]),
        ("LAUNCH",vec![call("subagent",json!({"agent":"worker","task":"CHILD-SCOPED: research","access":"read","label":"research"})),call("subagent",json!({"agent":"?","task":""})),Step::Final("父继续")]),
        ("CHILD-SCOPED",vec![call("write",json!({"path":"/tmp/original-subagent-write","content":"must be refused"})),Step::Final("REAL_CHILD_SCOPED_RESULT")]),
        ("RESULT",vec![Step::Final("父已接收真实结果")]),
        ("FOREIGN",vec![Step::Final("另一父会话")]),
        ("DENY",vec![call("subagent",json!({"agent":"worker","task":"CHILD-DENIED: must not start","access":"write"})),Step::Final("父看到权限拒绝")]),
        ("MISSING",vec![call("subagent",json!({"agent":"not-a-real-agent","task":"CHILD-MISSING: must not start"})),Step::Final("父看到无候选拒绝")]),
    ],vec![(("LAUNCH",2),parent_gate.clone()),(("CHILD-SCOPED",1),child_gate.clone())]);
    let fx=Fixture::start("subagent-list-launch",ServiceDeps{turn_provider:Some(provider.clone()),tool_gateway:Some(gateway_with_write(writes.clone())),..ServiceDeps::default()}).await;
    let token=fx.state.auth().local_token();
    let empty=execute_actual(fx.addr,&token,"sess_local_alpha","EMPTY: truthful empty roster").await;
    let no_candidates=outcome_json(&parent_tool_outcomes(&provider,"EMPTY",&empty)[0]);
    assert_eq!(no_candidates["agents"],json!([]));assert_eq!(no_candidates["noCandidates"],true);
    assert!(no_candidates["reason"].as_str().unwrap().contains("No other available agents"));
    assert_eq!(fx.child_count("sess_local_alpha").await,"0");
    let worker=fx.home.join("agents/worker");std::fs::create_dir_all(&worker).unwrap();
    std::fs::write(worker.join("config.yaml"),"agent:\n  name: 真实研究助手\n").unwrap();
    let list=execute_actual(fx.addr,&token,"sess_local_alpha","LIST: actual roster").await;
    let roster=outcome_json(&parent_tool_outcomes(&provider,"LIST",&list)[0]);
    assert_eq!(roster["agents"],json!([{"id":"worker","name":"真实研究助手"}]));
    assert_eq!(roster["noCandidates"],false);assert_eq!(fx.child_count("sess_local_alpha").await,"0");
    let addr=fx.addr;let auth=token.clone();
    let launched=tokio::spawn(async move {execute_actual(addr,&auth,"sess_local_alpha","LAUNCH: delegate read-only work").await});
    wait_until(||provider.calls.lock().unwrap().iter().any(|(key,_)|key=="CHILD-SCOPED")).await;
    parent_gate.add_permits(1);
    let parent=launched.await.unwrap();
    let outcomes=parent_tool_outcomes(&provider,"LAUNCH",&parent);
    assert_eq!(outcomes.len(),2);
    let launch=outcome_json(&outcomes[0]);let thread=fx.state.subagents().threads_of("sess_local_alpha").pop().unwrap();
    assert_eq!(launch["taskId"],thread.child_run_id.as_deref().unwrap());
    assert_eq!(launch["threadId"],thread.thread_id);assert_eq!(launch["childRunId"],launch["taskId"]);
    assert!(matches!(launch["status"].as_str(),Some("queued"|"running")));assert!(launch["progressError"].is_null());assert_eq!(thread.tier,lingxi_kernel::subagent::ToolAccessTier::ReadOnly);
    let progress=outcome_json(&outcomes[1]);let task=&progress["tasks"][0];
    assert_eq!(task["taskId"],launch["taskId"]);assert_eq!(task["threadId"],launch["threadId"]);
    assert_eq!(task["status"],"running");assert_eq!(task["modelCallsCompleted"],0);assert_eq!(task["toolCallsCompleted"],0);
    let child=thread.child_run_id.clone().unwrap();
    let lineage=fx.state.storage().query_one_text("SELECT parent_run_id FROM run_lineage WHERE run_id=?1",vec![child.clone()]).await.unwrap().unwrap();
    assert_eq!(lineage,parent);
    child_gate.add_permits(1);
    wait_until(||fx.state.subagents().active_counts("sess_local_alpha")== (0,0) && fx.state.sessions().session_supervisor().steering_pending("sess_local_alpha")>0).await;
    assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst),0,"只读交集必须在正式边界阻止真实外部执行");
    let child_tools=tool_results(&fx.events("sess_local_alpha").await,&child);assert_eq!(child_tools.len(),1);
    assert!(child_tools[0].error.as_ref().unwrap().message.contains("ACTION_BLOCKED_BY_READ_ONLY"));
    let final_parent=execute_actual(fx.addr,&token,"sess_local_alpha","RESULT: consume retained child").await;
    assert!(!final_parent.is_empty());assert!(submission_has(&provider,"RESULT","REAL_CHILD_SCOPED_RESULT"));
    assert!(submission_has(&provider,"RESULT",&thread.thread_id));
    execute_actual(fx.addr,&token,"sess_local_beta","FOREIGN: separate parent").await;
    assert!(!submission_has(&provider,"FOREIGN","REAL_CHILD_SCOPED_RESULT"));
    create_owned_session(&fx,"readonly-launch","read_only").await;
    let denial=execute_actual(fx.addr,&token,"readonly-launch","DENY: refuse escalation").await;
    let denied=parent_tool_outcomes(&provider,"DENY",&denial);
    assert!(matches!(&denied[0],ToolOutcome::Failed{error} if error.message.contains("SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY")));
    assert_eq!(fx.child_count("readonly-launch").await,"0");
    let missing=execute_actual(fx.addr,&token,"sess_local_alpha","MISSING: refuse unavailable candidate").await;
    assert!(matches!(&parent_tool_outcomes(&provider,"MISSING",&missing)[0],ToolOutcome::Failed{error} if error.code==ErrorCode::NotFound && error.message.contains("not-a-real-agent")));
    assert_eq!(fx.child_count("sess_local_alpha").await,"1");assert_eq!(fx.state.subagents().active_counts("sess_local_alpha"),(0,0));
    assert!(!provider.calls.lock().unwrap().iter().any(|(key,_)|matches!(key.as_str(),"CHILD-DENIED"|"CHILD-MISSING")));
    fx.finish().await;
}

#[tokio::test]
async fn subagent_reply_restores_actual_thread_history_and_returns_only_to_its_parent() {
    let provider=scripted(vec![
        ("START",vec![Step::Delegate{target:"subagent",task:"CHILD-OLD: first private task",thread_key:None},Step::Final("PARENT_PRIVATE_CONTEXT_MARKER")]),
        ("CHILD-OLD",vec![Step::Final("ACTUAL_CHILD_PRIOR_SECRET")]),
        ("REPLY",vec![Step::Delegate{target:"subagent_reply",task:"CHILD-NEXT: continue using prior result",thread_key:Some("T")},Step::Final("父继续处理")]),
        ("CHILD-NEXT",vec![Step::Final("ACTUAL_CHILD_CONTINUATION_RESULT")]),
        ("CONSUME",vec![Step::Final("父接到续接结果")]),
        ("FOREIGN-HISTORY",vec![Step::Delegate{target:"subagent_reply",task:"CHILD-FOREIGN: forbidden",thread_key:Some("T")},Step::Final("foreign rejected")]),
    ],vec![]);
    let fx=Fixture::start("subagent-real-history",ServiceDeps{turn_provider:Some(provider.clone()),tool_gateway:Some(delegation_gateway()),..ServiceDeps::default()}).await;
    let token=fx.state.auth().local_token();
    let start=execute_actual(fx.addr,&token,"sess_local_alpha","START: PARENT_PRIVATE_CONTEXT_MARKER").await;
    wait_until(||fx.state.subagents().active_counts("sess_local_alpha")== (0,0) && !fx.state.subagents().threads_of("sess_local_alpha").is_empty()).await;
    let first=fx.state.subagents().threads_of("sess_local_alpha").pop().unwrap();
    let first_child=first.child_run_id.clone().unwrap();
    let ack=outcome_json(&parent_tool_outcomes(&provider,"START",&start)[0]);assert_eq!(ack["threadId"],first.thread_id);assert_eq!(ack["taskId"],first_child);
    provider.threads.lock().unwrap().insert("T".into(),first.thread_id.clone());
    let foreign_before=fx.events("sess_local_beta").await;
    let reply=execute_actual(fx.addr,&token,"sess_local_alpha","REPLY: append a task").await;
    wait_until(||fx.state.subagents().active_counts("sess_local_alpha")== (0,0)).await;
    let continued=fx.state.subagents().threads_of("sess_local_alpha").pop().unwrap();
    assert_eq!(continued.thread_id,first.thread_id);assert_ne!(continued.child_run_id,first.child_run_id);
    let continued_ack=outcome_json(&parent_tool_outcomes(&provider,"REPLY",&reply)[0]);assert_eq!(continued_ack["threadId"],first.thread_id);assert_eq!(continued_ack["taskId"],continued.child_run_id.as_deref().unwrap());
    let calls=provider.calls.lock().unwrap().clone();
    let old=&calls.iter().find(|(m,_)|m=="CHILD-OLD").unwrap().1;
    assert!(old.prior.is_empty());assert!(!old.submission.contains("PARENT_PRIVATE_CONTEXT_MARKER"));
    let next=&calls.iter().find(|(m,_)|m=="CHILD-NEXT").unwrap().1;
    assert!(next.prior.iter().any(|i|matches!(i,ExchangeItem::UserTurn{text} if text=="CHILD-OLD: first private task")));
    assert!(next.prior.iter().any(|i|matches!(i,ExchangeItem::AssistantTurn{content,..} if content.iter().any(|b|matches!(b,ContentBlock::Text{text} if text=="ACTUAL_CHILD_PRIOR_SECRET")))));
    assert!(!format!("{:?}",next.prior).contains("PARENT_PRIVATE_CONTEXT_MARKER"));
    let durable=fx.state.storage().query_one_text("SELECT history_json FROM subagent_history_v1 WHERE run_id=?1",vec![first_child.clone()]).await.unwrap().unwrap();
    assert!(durable.contains("ACTUAL_CHILD_PRIOR_SECRET"));assert!(durable.contains("CHILD-OLD: first private task"));
    execute_actual(fx.addr,&token,"sess_local_alpha","CONSUME: real continuation delivered").await;
    assert!(["REPLY","CONSUME"].iter().any(|marker|submission_has(&provider,marker,"ACTUAL_CHILD_CONTINUATION_RESULT")));
    assert!(["REPLY","CONSUME"].iter().any(|marker|submission_has(&provider,marker,continued.child_run_id.as_deref().unwrap())));
    assert_eq!(fx.events("sess_local_beta").await,foreign_before);
    let before=fx.state.subagents().threads_of("sess_local_alpha");
    let foreign=execute_actual(fx.addr,&token,"sess_local_beta","FOREIGN-HISTORY: reject other parent").await;
    assert!(matches!(&parent_tool_outcomes(&provider,"FOREIGN-HISTORY",&foreign)[0],ToolOutcome::Failed{error} if error.code==ErrorCode::Forbidden));
    assert_eq!(fx.state.subagents().threads_of("sess_local_alpha"),before);assert_eq!(fx.child_count("sess_local_beta").await,"0");
    assert!(!submission_has(&provider,"FOREIGN-HISTORY","ACTUAL_CHILD_PRIOR_SECRET"));
    assert!(!submission_has(&provider,"FOREIGN-HISTORY","ACTUAL_CHILD_CONTINUATION_RESULT"));
    assert!(!provider.calls.lock().unwrap().iter().any(|(key,_)|key=="CHILD-FOREIGN"));
    fx.finish().await;
}

#[tokio::test]
async fn subagent_actual_panic_and_provider_failure_return_reasons_and_reclaim_capacity() {
    let provider=scripted(vec![
        ("PANIC-PARENT",vec![call("subagent",json!({"task":"CHILD-PANIC: actual external panic"})),Step::Final("parent launched")]),
        ("CHILD-PANIC",vec![Step::Panic("ACTUAL_CHILD_PANIC_REASON_941")]),
        ("FAIL-PARENT",vec![call("subagent",json!({"task":"CHILD-FAIL: actual provider failure"})),Step::Final("parent launched second")]),
        ("CHILD-FAIL",vec![Step::Failed("ACTUAL_CHILD_PROVIDER_FAILURE_REASON_942")]),
        ("SUCCESS-PARENT",vec![call("subagent",json!({"task":"CHILD-OK: capacity recovered"})),Step::Final("parent launched after failures")]),
        ("CHILD-OK",vec![Step::Final("AFTER_FAILURE_ACTUAL_SUCCESS")]),
        ("CONSUME-FAILURE",vec![Step::Final("parent sees concrete failures")]),
        ("FOREIGN-FAILURE",vec![Step::Final("other parent")]),
    ],vec![]);
    let fx=Fixture::start("subagent-failure-delivery",ServiceDeps{turn_provider:Some(provider.clone()),tool_gateway:Some(delegation_gateway()),subagent_policy:lingxi_kernel::subagent::SubagentPolicy{per_session_limit:1,global_limit:1,..Default::default()},..ServiceDeps::default()}).await;
    let token=fx.state.auth().local_token();
    for (parent,child,reason) in [("PANIC-PARENT: launch","CHILD-PANIC","ACTUAL_CHILD_PANIC_REASON_941"),("FAIL-PARENT: launch","CHILD-FAIL","ACTUAL_CHILD_PROVIDER_FAILURE_REASON_942")] {
        let run=execute_actual(fx.addr,&token,"sess_local_alpha",parent).await;
        let ack=outcome_json(&parent_tool_outcomes(&provider,parent.split(':').next().unwrap(),&run)[0]);
        wait_until(||fx.state.subagents().active_counts("sess_local_alpha")== (0,0) && (fx.state.sessions().session_supervisor().steering_pending("sess_local_alpha")>0 || ["PANIC-PARENT","FAIL-PARENT"].iter().any(|marker|submission_has(&provider,marker,reason)))).await;
        let record=fx.state.storage().query_one_text("SELECT state_json FROM subagent_threads_v1 WHERE thread_id=?1",vec![ack["threadId"].as_str().unwrap().into()]).await.unwrap().unwrap();
        let record:Value=serde_json::from_str(&record).unwrap();assert_eq!(record["lastRunStatus"],"failed");assert_eq!(record["busy"],false);
        assert!(record["error"].as_str().unwrap().contains(reason));assert!(record["result"].as_str().unwrap().contains(reason));
        assert!(provider.calls.lock().unwrap().iter().any(|(marker,_)|marker==child));
    }
    execute_actual(fx.addr,&token,"sess_local_alpha","CONSUME-FAILURE: read retained failures").await;
    for reason in ["ACTUAL_CHILD_PANIC_REASON_941","ACTUAL_CHILD_PROVIDER_FAILURE_REASON_942"] {
        assert!(["PANIC-PARENT","FAIL-PARENT","CONSUME-FAILURE"].iter().any(|marker|submission_has(&provider,marker,reason)),"真实失败正文必须进入所属父模型输入：{reason}");
    }
    execute_actual(fx.addr,&token,"sess_local_beta","FOREIGN-FAILURE: isolated").await;
    assert!(!submission_has(&provider,"FOREIGN-FAILURE","ACTUAL_CHILD_PANIC_REASON_941"));
    assert!(!submission_has(&provider,"FOREIGN-FAILURE","ACTUAL_CHILD_PROVIDER_FAILURE_REASON_942"));
    execute_actual(fx.addr,&token,"sess_local_alpha","SUCCESS-PARENT: reuse capacity").await;
    wait_until(||fx.state.subagents().active_counts("sess_local_alpha")== (0,0)).await;
    assert_eq!(fx.child_count("sess_local_alpha").await,"3");
    assert!(fx.state.subagents().threads_of("sess_local_alpha").iter().any(|t|t.last_run_status.as_deref()==Some("completed")));
    fx.finish().await;
}

#[tokio::test]
async fn subagent_session_and_global_caps_refuse_and_recover_without_ghost_children() {
    let child_gate=Arc::new(tokio::sync::Semaphore::new(0));
    let provider=scripted(vec![
        ("CAP-A",vec![call("subagent",json!({"task":"HELD-A: first"})),call("subagent",json!({"task":"SHOULD-NOT-A: session cap"})),Step::Final("session cap refusal")]),
        ("HELD-A",vec![Step::Final("real result A")]),
        ("CAP-B",vec![call("subagent",json!({"task":"HELD-B: second"})),Step::Final("launched B")]),
        ("HELD-B",vec![Step::Final("real result B")]),
        ("CAP-GLOBAL-REFUSE",vec![call("subagent",json!({"task":"SHOULD-NOT-GLOBAL: global cap"})),Step::Final("global cap refusal")]),
        ("CAP-RECOVER",vec![call("subagent",json!({"task":"RECOVERED-CHILD: actual accepted"})),Step::Final("recovered")]),
        ("RECOVERED-CHILD",vec![Step::Final("true recovered result")]),
    ],vec![(("HELD-A",1),child_gate.clone()),(("HELD-B",1),child_gate.clone())]);
    let fx=Fixture::start("subagent-cap-matrix",ServiceDeps{turn_provider:Some(provider.clone()),tool_gateway:Some(delegation_gateway()),subagent_policy:lingxi_kernel::subagent::SubagentPolicy{per_session_limit:1,global_limit:2,..Default::default()},..ServiceDeps::default()}).await;
    let token=fx.state.auth().local_token();create_owned_session(&fx,"cap-third-parent","operate").await;
    let cap_parent=execute_actual(fx.addr,&token,"sess_local_alpha","CAP-A: launch then refuse second in same parent").await;
    wait_until(||provider.calls.lock().unwrap().iter().any(|(m,_)|m=="HELD-A")).await;
    let cap_outcomes=parent_tool_outcomes(&provider,"CAP-A",&cap_parent);
    assert_eq!(cap_outcomes.len(),2);
    assert!(matches!(&cap_outcomes[0],ToolOutcome::Success{..}));
    assert!(matches!(&cap_outcomes[1],ToolOutcome::Failed{error} if error.code==ErrorCode::BudgetExceeded && error.message.contains("per-session")));
    assert_eq!(fx.child_count("sess_local_alpha").await,"1");assert_eq!(fx.state.subagents().active_counts("sess_local_alpha"),(1,1));
    execute_actual(fx.addr,&token,"sess_local_beta","CAP-B: launch second").await;
    wait_until(||provider.calls.lock().unwrap().iter().any(|(m,_)|m=="HELD-B")).await;
    let global_refused=execute_actual(fx.addr,&token,"cap-third-parent","CAP-GLOBAL-REFUSE: no third child").await;
    assert!(matches!(&parent_tool_outcomes(&provider,"CAP-GLOBAL-REFUSE",&global_refused)[0],ToolOutcome::Failed{error} if error.code==ErrorCode::BudgetExceeded && error.message.contains("global")));
    assert_eq!(fx.child_count("cap-third-parent").await,"0");assert_eq!(fx.state.subagents().active_counts("cap-third-parent"),(0,2));
    assert!(!provider.calls.lock().unwrap().iter().any(|(m,_)|matches!(m.as_str(),"SHOULD-NOT-A"|"SHOULD-NOT-GLOBAL")));
    child_gate.add_permits(2);wait_until(||fx.state.subagents().active_counts("sess_local_alpha")== (0,0)).await;
    execute_actual(fx.addr,&token,"cap-third-parent","CAP-RECOVER: capacity reusable").await;
    wait_until(||fx.state.subagents().active_counts("cap-third-parent")== (0,0)).await;
    assert_eq!(fx.child_count("cap-third-parent").await,"1");
    assert_eq!(fx.child_count("sess_local_alpha").await,"1");assert_eq!(fx.child_count("sess_local_beta").await,"1");
    fx.finish().await;
}
