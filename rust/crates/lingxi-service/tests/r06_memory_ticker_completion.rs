//! R06 0B：真实周期宿主、开关、日期、取消、费用与混合配置失败实效。
//! 只门控外部供应商响应，不替换被验收业务核心。
use lingxi_kernel::ports::StoragePort;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

type ProviderGate = Option<(
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<()>,
)>;

struct Fixture {
    addr: SocketAddr,
    state: ServiceState,
    clock: Arc<lingxi_service::inject::ManualClock>,
    home: PathBuf,
    token: String,
    requests: Arc<Mutex<Vec<Value>>>,
    provider_gate: Arc<Mutex<ProviderGate>>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
    stub_stop: tokio::sync::oneshot::Sender<()>,
    stub_task: tokio::task::JoinHandle<()>,
}
async fn read_request(socket: &mut TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut block = [0u8; 4096];
    loop {
        let n = socket.read(&mut block).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&block[..n]);
        if let Some(index) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let headers = String::from_utf8(bytes[..index].to_vec()).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= index + 4 + length {
                return (headers, bytes[index + 4..index + 4 + length].to_vec());
            }
        }
    }
}
impl Fixture {
    async fn start(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "dream-completion-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let product = root.join("product");
        for (dir, name, text) in [
            ("identity-templates", "completion-persona", "默认身份"),
            ("agents-templates", "completion-persona", "默认说明"),
            ("yuan", "completion-persona", "你是 {{agentName}}，协作助手"),
        ] {
            std::fs::create_dir_all(product.join(dir)).unwrap();
            std::fs::write(product.join(dir).join(format!("{name}.md")), text).unwrap();
        }
        for id in ["agentA", "agentB"] {
            let dir = home.join("agents").join(id);
            std::fs::create_dir_all(dir.join("memory")).unwrap();
            std::fs::write(dir.join("config.yaml"),format!("locale: zh\nagent:\n  name: 助手{id}\n  yuan: completion-persona\nmemory:\n  enabled: true\n")).unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stub_addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let saved = requests.clone();
        let provider_gate = Arc::new(Mutex::new(
            None::<(
                tokio::sync::oneshot::Sender<()>,
                tokio::sync::oneshot::Receiver<()>,
            )>,
        ));
        let served_gate = provider_gate.clone();
        let (stub_stop, mut stop_rx) = tokio::sync::oneshot::channel();
        let stub_task = tokio::spawn(async move {
            loop {
                let accepted =
                    tokio::select! {_=&mut stop_rx=>break,value=listener.accept()=>value};
                let (mut socket, _) = accepted.unwrap();
                let saved = saved.clone();
                let served_gate = served_gate.clone();
                tokio::spawn(async move {
                    let (_, body) = read_request(&mut socket).await;
                    let request: Value = serde_json::from_slice(&body).unwrap();
                    let instruction = request["messages"][0]["content"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let payload: Value = serde_json::from_str(
                        request["messages"].as_array().unwrap().last().unwrap()["content"]
                            .as_str()
                            .unwrap_or("null"),
                    )
                    .unwrap_or(Value::Null);
                    saved.lock().unwrap().push(request.clone());
                    let gate = { served_gate.lock().unwrap().take() };
                    if let Some((entered, release)) = gate {
                        entered.send(()).unwrap();
                        release.await.unwrap();
                    }
                    let content = if request["model"] == "chat-model" {
                        "实际消费后的模型回复".to_string()
                    } else {
                        provider_answer(&instruction, &payload).to_string()
                    };
                    let frame = json!({"id":"dream","choices":[{"index":0,"delta":{"role":"assistant","content":content},"finish_reason":"stop"}]});
                    let sse = format!(
                        "data: {frame}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        json!({"id":"persona","choices":[],"usage":{"prompt_tokens":23,"completion_tokens":8}})
                    );
                    let wire=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",sse.len());
                    socket.write_all(wire.as_bytes()).await.unwrap();
                });
            }
        });
        let mut plane_json = json!({"providers":{"persona-stub":{"protocol":"openai-completions","endpoint":format!("http://{stub_addr}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-persona-key"}}},"models":{"chat":{"provider":"persona-stub","model":"chat-model","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"memory":{"provider":"persona-stub","model":"memory-model","compat":{"contextWindow":200000}}}});
        if tag == "unconfigured" {
            plane_json["models"]
                .as_object_mut()
                .unwrap()
                .remove("memory");
        }
        let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(
            &plane_json.to_string(),
        )
        .unwrap();
        let credential = Arc::new(
            lingxi_service::credentials::CredentialService::bootstrap(
                &plane,
                &root.join("credentials"),
                Arc::new(lingxi_service::inject::SystemClock),
            )
            .unwrap(),
        );
        let gateway =
            Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane));
        let compiler = Arc::new(
            lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
                lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                    &home,
                    product.clone(),
                    lingxi_service::context_compiler::HostFacts {
                        platform: "darwin".into(),
                        os_type: "Darwin".into(),
                        os_release: "test".into(),
                        shell_label: "zsh".into(),
                        workspace: workspace.to_string_lossy().into_owned(),
                    },
                ),
            )),
        );
        let config = ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        };
        use chrono::TimeZone;
        let ms = chrono::Local
            .with_ymd_and_hms(2026, 10, 15, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64;
        let clock = Arc::new(lingxi_service::inject::ManualClock::new(ms));
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            config,
            &layout,
            ServiceDeps {
                clock: clock.clone(),
                memory_ticker_interval: Some(Duration::from_millis(150)),
                model_gateway: Some(gateway),
                credential_service: Some(credential),
                network_plane: if tag == "missing-network" {
                    None
                } else {
                    Some(lingxi_adapters::models::network::NetworkPlane::direct_isolated())
                },
                workspace_root: Some(workspace),
                product_material_dir: Some(product),
                context_compiler: Some(compiler),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let (stop, stop_rx) = tokio::sync::oneshot::channel();
        let (ready, ready_rx) = tokio::sync::oneshot::channel();
        let served = state.clone();
        let task = tokio::spawn(async move {
            run(
                served,
                async {
                    let _ = stop_rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
            .unwrap();
        });
        let addr = ready_rx.await.unwrap();
        Self {
            addr,
            state,
            clock,
            home,
            token,
            requests,
            provider_gate,
            stop,
            task,
            stub_stop,
            stub_task,
        }
    }
    fn pause_next_provider(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, received) = tokio::sync::oneshot::channel();
        let (release, resumed) = tokio::sync::oneshot::channel();
        assert!(self
            .provider_gate
            .lock()
            .unwrap()
            .replace((entered, resumed))
            .is_none());
        (received, release)
    }
    async fn raw(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: &[u8],
        etag: Option<&str>,
    ) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        let conditional = etag
            .map(|e| format!("If-None-Match: {e}\r\n"))
            .unwrap_or_default();
        let headers=format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{conditional}Connection: close\r\n\r\n",body.len());
        stream.write_all(headers.as_bytes()).await.unwrap();
        let _ = stream.write_all(body).await;
        let mut received = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(40), stream.read_to_end(&mut received))
            .await
            .unwrap();
        if let Err(error) = read {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset, "{error}");
        }
        let index = received.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
        let headers = String::from_utf8(received[..index].to_vec()).unwrap();
        let code = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (code, headers, received[index + 4..].to_vec())
    }
    async fn request(&self, method: &str, path: &str, payload: Value) -> (u16, Value) {
        let (code, _, body) = self
            .raw(
                method,
                path,
                &self.token,
                payload.to_string().as_bytes(),
                None,
            )
            .await;
        (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }
    async fn restart(&mut self) {
        let gateway = self.state.model_gateway().cloned();
        let credentials = self.state.credential_service().cloned();
        let (placeholder, _) = tokio::sync::oneshot::channel();
        std::mem::replace(&mut self.stop, placeholder)
            .send(())
            .unwrap();
        let old = std::mem::replace(&mut self.task, tokio::spawn(async {}));
        old.await.unwrap();
        let config = ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: self.home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        };
        self.state = ServiceState::bootstrap_with_deps(
            config,
            &prepare_layout(&self.home).unwrap(),
            ServiceDeps {
                clock: self.clock.clone(),
                memory_ticker_interval: Some(Duration::from_millis(150)),
                model_gateway: gateway,
                credential_service: credentials,
                network_plane: Some(
                    lingxi_adapters::models::network::NetworkPlane::direct_isolated(),
                ),
                workspace_root: Some(self.home.parent().unwrap().join("workspace")),
                product_material_dir: Some(self.home.parent().unwrap().join("product")),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        self.token = self.state.auth().local_token();
        let (stop, stop_rx) = tokio::sync::oneshot::channel();
        let (ready, ready_rx) = tokio::sync::oneshot::channel();
        let state = self.state.clone();
        self.stop = stop;
        self.task = tokio::spawn(async move {
            run(
                state,
                async {
                    let _ = stop_rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
            .unwrap();
        });
        self.addr = ready_rx.await.unwrap();
    }
    async fn finish(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap();
        let _ = self.stub_stop.send(());
        self.stub_task.await.unwrap();
        std::fs::remove_dir_all(self.home.parent().unwrap()).unwrap();
    }
}
fn provider_answer(instruction: &str, input: &Value) -> Value {
    if input.to_string().contains("破坏来源") {
        return json!({"units":[{"sourceBlockId":"unknown","section":"facts","text":"非法新信息"}]});
    }
    if instruction.contains("memory.dream.atomize") {
        json!({"units":input["sourceBlocks"].as_array().unwrap().iter().map(|b|json!({"sourceBlockId":b["id"],"section":b["section"],"text":b["text"]})).collect::<Vec<_>>()})
    } else if instruction.contains("memory.dream.dedupe") {
        json!({"groups":input["units"].as_array().unwrap().iter().map(|u|json!({"sourceUnitIds":[u["id"]],"relation":"distinct"})).collect::<Vec<_>>()})
    } else if instruction.contains("memory.dream.optimize") {
        json!({"units":input["groups"].as_array().unwrap().iter().map(|g|json!({"groupId":g["id"],"section":g["section"],"text":g["sources"][0]["text"]})).collect::<Vec<_>>(),"removedGroups":[]})
    } else if instruction.contains("memory.dream.compose") {
        json!({"paragraphs":input["units"].as_array().unwrap().iter().map(|u|json!({"sourceUnitIds":[u["id"]],"section":u["section"],"topic":"用户偏好","text":u["text"]})).collect::<Vec<_>>()})
    } else {
        json!({"ok":true,"missingClaims":[],"compoundUnits":[],"incorrectMerges":[],"unsupportedClaims":[],"subjectLeaks":[],"unsafeRemovals":[],"duplicateClaims":[],"fragmentedTopics":[],"incoherentParagraphs":[],"insufficientCompression":[]})
    }
}
fn seed(fx: &Fixture, agent: &str, text: &str) {
    let store = fx.state.memory().compiled_store(agent).unwrap();
    store.write_facts(text).unwrap();
    store.write_longterm("- 用户长期学习音乐").unwrap();
    store.write_today("今日不能改变").unwrap();
}
async fn ticker(fx: &Fixture, agent: &str) -> Value {
    let (code, value) = fx
        .request(
            "GET",
            &format!("/lingxi/v1/memories/health?agentId={agent}"),
            Value::Null,
        )
        .await;
    assert_eq!(code, 200, "{value}");
    value["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "memory_ticker")
        .unwrap()["ticker"]
        .clone()
}
async fn wait_checks(fx: &Fixture, agent: &str, target: u64) -> Value {
    for _ in 0..200 {
        let value = ticker(fx, agent).await;
        if value["checkCount"].as_u64().unwrap_or(0) >= target {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    panic!("实际周期没有继续执行")
}
async fn auto(fx: &Fixture, agent: &str, enabled: bool) {
    let result = fx
        .request(
            "PUT",
            &format!("/lingxi/v1/agents/{agent}/config"),
            json!({"memory":{"dream":{"auto_enabled":enabled}}}),
        )
        .await;
    assert_eq!(result.0, 200, "{result:?}");
}
async fn wait_report(fx: &Fixture, agent: &str, trigger: &str, date: &str, status: &str) -> Value {
    for _ in 0..300 {
        let (code, value) = fx
            .request(
                "GET",
                &format!("/lingxi/v1/memories/dream/status?agentId={agent}"),
                Value::Null,
            )
            .await;
        assert_eq!(code, 200, "{value}");
        if value["status"] == status
            && value["lastRun"]["trigger"] == trigger
            && value["lastRun"]["logicalDate"] == date
        {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    panic!("期待报告未出现: {agent} {trigger} {date} {status}")
}
fn at(fx: &Fixture, day: u32, hour: u32, minute: u32) {
    use chrono::TimeZone;
    fx.clock.set(
        chrono::Local
            .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64,
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn periodic_timer_default_auto_off_does_not_create_dream_or_call_models() {
    let fx = Fixture::start("ticker-off").await;
    seed(&fx, "agentA", "- 默认休眠资料");
    let config = std::fs::read(fx.home.join("agents/agentA/config.yaml")).unwrap();
    let snapshot = wait_checks(&fx, "agentA", 3).await;
    assert_eq!(snapshot["hostRunning"], true);
    assert_eq!(snapshot["checkIntervalMs"], 150);
    assert_eq!(snapshot["state"], "automatic_disabled");
    assert!(!fx
        .home
        .join("agents/agentA/memory/dream/state.json")
        .exists());
    assert!(fx.requests.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentA/config.yaml")).unwrap(),
        config
    );
    let usage = fx
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    assert!(usage.is_empty());
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn toggling_memory_really_pauses_then_resumes_timer_and_preserves_paid_cancel() {
    let fx = Fixture::start("ticker-toggle").await;
    seed(&fx, "agentA", "- A资料");
    seed(&fx, "agentB", "- B独立资料");
    let (entered, release) = fx.pause_next_provider();
    auto(&fx, "agentA", true).await;
    entered.await.unwrap();
    auto(&fx, "agentB", true).await;
    assert_eq!(
        wait_report(&fx, "agentB", "automatic", "2026-10-15", "succeeded").await["lastRun"]
            ["trigger"],
        "automatic"
    );
    let b = std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap();
    let off = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            json!({"memory":{"enabled":false}}),
        )
        .await;
    assert_eq!(off.0, 200, "{off:?}");
    let paused = wait_checks(
        &fx,
        "agentA",
        ticker(&fx, "agentA").await["checkCount"].as_u64().unwrap() + 2,
    )
    .await;
    assert_eq!(paused["state"], "paused");
    assert_eq!(paused["paused"], true);
    assert_eq!(paused["automaticEnabled"], true);
    let cfg = fx
        .request("GET", "/lingxi/v1/agents/agentA/config", Value::Null)
        .await;
    assert!(cfg.1["memory"]["disabledSince"].as_str().is_some());
    release.send(()).unwrap();
    let cancelled = wait_report(&fx, "agentA", "automatic", "2026-10-15", "failed").await;
    assert_eq!(cancelled["lastRun"]["errorCode"], "dream_cancelled");
    assert_eq!(fx.requests.lock().unwrap().len(), 6);
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        b
    );
    let on = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            json!({"memory":{"enabled":true}}),
        )
        .await;
    assert_eq!(on.0, 200);
    assert!(fx
        .request("GET", "/lingxi/v1/agents/agentA/config", Value::Null)
        .await
        .1["memory"]["reenableAt"]
        .as_str()
        .is_some());
    assert_eq!(ticker(&fx, "agentA").await["paused"], false);
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        b
    );
    auto(&fx, "agentB", false).await;
    let expected_b = std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap();
    at(&fx, 16, 12, 0);
    wait_report(&fx, "agentA", "automatic", "2026-10-16", "succeeded").await;
    assert_eq!(fx.requests.lock().unwrap().len(), 11);
    let usage = fx
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    assert_eq!(usage.len(), 11);
    assert!(usage.iter().all(|u| u.usage.is_some()));
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        expected_b
    );
    // B仅修改了自己明确收到的auto开关，不被A总开关串写。
    let b_after = std::fs::read_to_string(fx.home.join("agents/agentB/config.yaml")).unwrap();
    assert!(!b_after.contains("disabledSince"));
    assert!(!b_after.contains("reenableAt"));
    assert!(!b.is_empty());
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_attempt_date_boundary_restart_and_manual_success_are_real_eligibility() {
    let mut fx = Fixture::start("ticker-empty-date").await;
    auto(&fx, "agentA", true).await;
    let empty = wait_report(&fx, "agentA", "automatic", "2026-10-15", "failed").await;
    assert_eq!(empty["lastRun"]["errorCode"], "dream_no_memory");
    assert!(empty["lastRun"]["runId"].as_str().is_some());
    assert!(empty["lastRun"]["beforeChars"].is_number());
    assert_eq!(empty["lastRun"]["appliedOperationCount"], 0);
    assert!(fx.requests.lock().unwrap().is_empty());
    let state_path = fx.home.join("agents/agentA/memory/dream/state.json");
    let saved: Value = serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(saved["lastAutomaticAttemptDate"], "2026-10-15");
    seed(&fx, "agentA", "- 空尝试之后新写入资料");
    at(&fx, 16, 3, 59);
    wait_checks(
        &fx,
        "agentA",
        ticker(&fx, "agentA").await["checkCount"].as_u64().unwrap() + 2,
    )
    .await;
    assert!(fx.requests.lock().unwrap().is_empty());
    at(&fx, 16, 4, 0);
    wait_report(&fx, "agentA", "automatic", "2026-10-16", "succeeded").await;
    assert_eq!(fx.requests.lock().unwrap().len(), 5);
    fx.restart().await;
    wait_checks(&fx, "agentA", 2).await;
    assert_eq!(fx.requests.lock().unwrap().len(), 5);
    seed(&fx, "agentB", "- 手动成功日不再自动整理");
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentB",
            json!({})
        )
        .await
        .0,
        202
    );
    wait_report(&fx, "agentB", "manual", "2026-10-16", "succeeded").await;
    auto(&fx, "agentB", true).await;
    wait_checks(
        &fx,
        "agentB",
        ticker(&fx, "agentB").await["checkCount"].as_u64().unwrap() + 2,
    )
    .await;
    assert_eq!(
        ticker(&fx, "agentB").await["state"],
        "manual_succeeded_today"
    );
    assert_eq!(fx.requests.lock().unwrap().len(), 10);
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_automatic_work_does_not_disable_manual_dream_or_host() {
    let fx = Fixture::start("ticker-cancel").await;
    seed(&fx, "agentA", "- 取消后仍可手动整理");
    let (entered, release) = fx.pause_next_provider();
    auto(&fx, "agentA", true).await;
    entered.await.unwrap();
    let cancelled = fx
        .request(
            "DELETE",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            json!({}),
        )
        .await;
    assert_eq!(cancelled.0, 202, "{cancelled:?}");
    release.send(()).unwrap();
    wait_report(&fx, "agentA", "automatic", "2026-10-15", "failed").await;
    auto(&fx, "agentA", false).await;
    assert_eq!(ticker(&fx, "agentA").await["hostRunning"], true);
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            json!({})
        )
        .await
        .0,
        202
    );
    wait_report(&fx, "agentA", "manual", "2026-10-15", "succeeded").await;
    assert_eq!(fx.requests.lock().unwrap().len(), 6);
    let usage = fx
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    assert_eq!(usage.len(), 6);
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_joins_periodic_host_and_paid_cancel_before_real_restart() {
    let mut fx = Fixture::start("ticker-shutdown").await;
    seed(&fx, "agentA", "- 关机保留已付费整理");
    let (entered, release) = fx.pause_next_provider();
    auto(&fx, "agentA", true).await;
    entered.await.unwrap();
    let old = fx.state.clone();
    let requests = fx.requests.clone();
    {
        let restart = fx.restart();
        tokio::pin!(restart);
        assert!(
            tokio::time::timeout(Duration::from_millis(300), restart.as_mut())
                .await
                .is_err()
        );
        assert_eq!(
            old.memory().dream().ticker().snapshot("agentA")["hostRunning"],
            false
        );
        assert_eq!(requests.lock().unwrap().len(), 1);
        release.send(()).unwrap();
        restart.await;
    }
    // 新宿主读取同日实际尝试记录，不重放付费自动任务。
    wait_checks(&fx, "agentA", 2).await;
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            json!({})
        )
        .await
        .0,
        202
    );
    wait_report(&fx, "agentA", "manual", "2026-10-15", "succeeded").await;
    let usage = fx
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    assert_eq!(usage.len(), 6);
    assert!(usage.iter().all(|u| u.usage.is_some()));
    fx.finish().await;
}
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mixed_patch_real_late_io_failure_preserves_committed_global_without_fake_rollback() {
    let fx = Fixture::start("ticker-mixed-io").await;
    seed(&fx, "agentA", "- 混合写失败后仍有完整资料");
    let path = fx.home.join("agents/agentA/config.yaml");
    let before = std::fs::read(&path).unwrap();
    let b_before = std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap();
    assert!(std::process::Command::new("chflags")
        .arg("uchg")
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let result = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            json!({"locale":"en","memory":{"enabled":false,"dream":{"auto_enabled":true}}}),
        )
        .await;
    assert!(std::process::Command::new("chflags")
        .arg("nouchg")
        .arg(&path)
        .status()
        .unwrap()
        .success());
    assert_eq!(result.0, 500, "{result:?}");
    let prefs: Value =
        serde_json::from_slice(&std::fs::read(fx.home.join("user/preferences.json")).unwrap())
            .unwrap();
    assert_eq!(prefs["locale"], "en");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        b_before
    );
    let config = fx
        .request("GET", "/lingxi/v1/agents/agentA/config", Value::Null)
        .await;
    assert_eq!(config.0, 200);
    assert_eq!(config.1["locale"], "en");
    assert_eq!(config.1["memory"]["enabled"], true);
    assert!(config.1["memory"]["disabledSince"].is_null());
    assert_ne!(config.1["memory"]["dream"]["auto_enabled"], true);
    let snapshot = wait_checks(
        &fx,
        "agentA",
        ticker(&fx, "agentA").await["checkCount"].as_u64().unwrap() + 2,
    )
    .await;
    assert_eq!(snapshot["paused"], false);
    assert_eq!(snapshot["state"], "automatic_disabled");
    assert!(fx.requests.lock().unwrap().is_empty());
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unconfigured_automatic_attempt_is_loud_and_has_no_invented_model_or_fee() {
    let fx = Fixture::start("unconfigured").await;
    seed(&fx, "agentA", "- 没配置模型时不能编造结果");
    auto(&fx, "agentA", true).await;
    for _ in 0..200 {
        if ticker(&fx, "agentA").await["lastError"].as_str().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let snapshot = ticker(&fx, "agentA").await;
    assert_eq!(snapshot["automaticEnabled"], true);
    assert!(snapshot["lastError"]
        .as_str()
        .unwrap()
        .contains("dream_model_unconfigured"));
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            json!({})
        )
        .await
        .0,
        503
    );
    let saved: Value = serde_json::from_slice(
        &std::fs::read(fx.home.join("agents/agentA/memory/dream/state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["lastAutomaticAttemptDate"], "2026-10-15");
    assert_eq!(saved["lastRun"]["status"], "failed");
    assert!(saved["lastRun"]["model"].is_null());
    assert!(fx.requests.lock().unwrap().is_empty());
    assert!(fx
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap()
        .is_empty());
    fx.finish().await;
}

async fn chat_run(fx: &Fixture, input: &str) {
    let started = fx
        .request(
            "POST",
            "/lingxi/v1/sessions/dream-consumer/execute",
            json!({"input":input}),
        )
        .await;
    assert_eq!(started.0, 200, "{started:?}");
    let id = started.1["runId"].as_str().unwrap();
    for _ in 0..400 {
        let status = fx
            .state
            .storage()
            .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![id.into()])
            .await
            .unwrap();
        if status.as_deref() == Some("completed") {
            return;
        }
        assert_ne!(status.as_deref(), Some("failed"), "Run失败");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("实际Run没有完成");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busy_manual_does_not_spend_automatic_day_and_cancel_releases_actual_lease() {
    let fx = Fixture::start("ticker-busy").await;
    seed(&fx, "agentA", "- 同助手忙期间不占自动机会");
    let (entered, release) = fx.pause_next_provider();
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            json!({})
        )
        .await
        .0,
        202
    );
    entered.await.unwrap();
    auto(&fx, "agentA", true).await;
    let current = ticker(&fx, "agentA").await["checkCount"].as_u64().unwrap();
    let busy = wait_checks(&fx, "agentA", current + 2).await;
    assert_eq!(busy["state"], "dream_already_running");
    let saved: Value = serde_json::from_slice(
        &std::fs::read(fx.home.join("agents/agentA/memory/dream/state.json")).unwrap(),
    )
    .unwrap();
    assert!(saved["lastAutomaticAttemptDate"].is_null());
    assert_eq!(saved["runtime"]["trigger"], "manual");
    assert_eq!(
        fx.request(
            "DELETE",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            json!({})
        )
        .await
        .0,
        202
    );
    release.send(()).unwrap();
    wait_report(&fx, "agentA", "automatic", "2026-10-15", "succeeded").await;
    assert_eq!(fx.requests.lock().unwrap().len(), 6);
    let usage = fx
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    assert_eq!(usage.len(), 6);
    assert!(usage.iter().all(|u| u.usage.is_some()));
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn automatic_revision_is_really_applied_and_next_same_session_run_consumes_it() {
    let fx = Fixture::start("ticker-consumer").await;
    seed(&fx, "agentA", "- 用户喜欢音乐\n- 用户喜欢音乐");
    seed(&fx, "agentB", "- B不能串入");
    let before_b = std::fs::read(fx.home.join("agents/agentB/memory/memory.md")).unwrap();
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/sessions",
            json!({"sessionId":"dream-consumer","agentId":"agentA","title":"自动整理下次实际消费"})
        )
        .await
        .0,
        201
    );
    chat_run(&fx, "自动整理前读取").await;
    let initial = fx
        .requests
        .lock()
        .unwrap()
        .iter()
        .rfind(|r| r["model"] == "chat-model")
        .unwrap()
        .to_string();
    assert!(initial.contains("- 用户喜欢音乐\\n- 用户喜欢音乐"));
    auto(&fx, "agentA", true).await;
    let result = wait_report(&fx, "agentA", "automatic", "2026-10-15", "succeeded").await;
    assert_eq!(result["lastRun"]["changed"], true);
    let id = result["lastRun"]["revisionId"].as_str().unwrap();
    let revision = fx
        .request(
            "GET",
            &format!("/lingxi/v1/memories/dream/revisions/{id}?agentId=agentA"),
            Value::Null,
        )
        .await;
    assert_eq!(revision.0, 200);
    assert_eq!(revision.1["current"]["facts"], "用户喜欢音乐");
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/memory/memory.md")).unwrap(),
        before_b
    );
    chat_run(&fx, "自动整理后读取").await;
    let after = fx
        .requests
        .lock()
        .unwrap()
        .iter()
        .rfind(|r| r["model"] == "chat-model")
        .unwrap()
        .to_string();
    assert!(after.contains("用户喜欢音乐"));
    assert!(!after.contains("- 用户喜欢音乐\\n- 用户喜欢音乐"));
    let usage = fx
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
            purpose: Some("auxiliary.memory".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(usage.len(), 5);
    fx.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_real_config_put_cannot_be_overwritten_by_old_tick_publication() {
    use lingxi_service::memory_ticker::TickerPublicationPhase;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Condvar,
    };
    let fx = Fixture::start("ticker-publish-race").await;
    seed(&fx, "agentA", "- 真实关闭必须优先于旧周期显示");
    let before_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let after_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let before_wait = before_gate.clone();
    let after_wait = after_gate.clone();
    let (before_tx, before_rx) = tokio::sync::oneshot::channel();
    let before_tx = Mutex::new(Some(before_tx));
    let (after_tx, after_rx) = tokio::sync::oneshot::channel();
    let after_tx = Mutex::new(Some(after_tx));
    let claimed = AtomicBool::new(false);
    let hook = fx
        .state
        .memory()
        .dream()
        .ticker()
        .publication_test_hook(Arc::new(move |phase, agent, outcome| {
            if agent != "agentA" || outcome["state"] != "automatic_started" {
                return;
            }
            if phase == TickerPublicationPhase::BeforePublish {
                if claimed.swap(true, Ordering::SeqCst) {
                    return;
                }
                before_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                let (lock, changed) = &*before_wait;
                let released = lock.lock().unwrap();
                let (_released_guard, timeout) = changed
                    .wait_timeout_while(released, Duration::from_secs(10), |v| !*v)
                    .unwrap();
                assert!(!timeout.timed_out());
            } else {
                after_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                let (lock, changed) = &*after_wait;
                let released = lock.lock().unwrap();
                let (_released_guard, timeout) = changed
                    .wait_timeout_while(released, Duration::from_secs(10), |v| !*v)
                    .unwrap();
                assert!(!timeout.timed_out());
            }
        }));
    auto(&fx, "agentA", true).await;
    before_rx.await.unwrap();
    let disabled = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            json!({"memory":{"enabled":false}}),
        )
        .await;
    assert_eq!(disabled.0, 200, "{disabled:?}");
    *before_gate.0.lock().unwrap() = true;
    before_gate.1.notify_one();
    after_rx.await.unwrap();
    // 旧周期已经真实到达发布后边界，新周期仍被挡住；读回不能靠下一周期纠正。
    let actual = ticker(&fx, "agentA").await;
    assert_eq!(actual["paused"], true);
    assert_eq!(actual["state"], "paused");
    let config = fx
        .request("GET", "/lingxi/v1/agents/agentA/config", Value::Null)
        .await;
    assert_eq!(config.1["memory"]["enabled"], false);
    assert!(config.1["memory"]["disabledSince"].as_str().is_some());
    *after_gate.0.lock().unwrap() = true;
    after_gate.1.notify_one();
    drop(hook);
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            json!({})
        )
        .await
        .0,
        409
    );
    fx.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn review_paid_automatic_failure_is_visible_to_health_and_next_day_recovery_is_real() {
    let fx = Fixture::start("review-paid-auto-failure").await;
    seed(&fx, "agentA", "- 破坏来源");
    let before = std::fs::read(fx.home.join("agents/agentA/memory/facts.md")).unwrap();
    auto(&fx, "agentA", true).await;
    let report = wait_report(&fx, "agentA", "automatic", "2026-10-15", "failed").await;
    assert_eq!(report["lastRun"]["status"], "failed");
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentA/memory/facts.md")).unwrap(),
        before
    );
    let first = ticker(&fx, "agentA").await;
    wait_checks(&fx, "agentA", first["checkCount"].as_u64().unwrap() + 2).await;
    let health = fx
        .request(
            "GET",
            "/lingxi/v1/memories/health?agentId=agentA",
            Value::Null,
        )
        .await;
    assert_eq!(health.0, 200);
    let step = health.1["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "memory_ticker")
        .unwrap()
        .clone();
    eprintln!(
        "PAID_AUTO_FAILED actual_report={} actual_health_step={step}",
        report["lastRun"]
    );
    let usage = fx
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
            purpose: Some("auxiliary.memory".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(usage.len(), 2);
    assert!(usage.iter().all(|r| r.usage.is_some()));
    assert_eq!(fx.requests.lock().unwrap().len(), 2);
    // 移除远端坏来源前置，到真实次日后由同一正在运行的 timer 自动接力。
    seed(&fx, "agentA", "- 用户新一天的真实资料");
    at(&fx, 16, 12, 0);
    let recovered = wait_report(&fx, "agentA", "automatic", "2026-10-16", "succeeded").await;
    assert_eq!(recovered["lastRun"]["status"], "succeeded");
    assert_eq!(fx.requests.lock().unwrap().len(), 7);
    let after = ticker(&fx, "agentA").await;
    wait_checks(&fx, "agentA", after["checkCount"].as_u64().unwrap() + 1).await;
    let restored = fx
        .request(
            "GET",
            "/lingxi/v1/memories/health?agentId=agentA",
            Value::Null,
        )
        .await;
    let good = restored.1["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "memory_ticker")
        .unwrap()
        .clone();
    fx.finish().await;
    assert_eq!(
        step["ok"], false,
        "有真实已付费失败且没有后续成功，automatic_dream健康不得报成功: {step}"
    );
    assert!(
        step["error"].as_str().is_some(),
        "实际失败原因应进入健康读回: {step}"
    );
    assert_eq!(good["ok"], true, "真实次日恢复后才清理原错误: {good}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paid_terminal_feedback_survives_restart_and_old_epoch_cannot_unpause_host() {
    let mut fx = Fixture::start("ticker-terminal-epoch").await;
    seed(&fx, "agentA", "- 破坏来源");
    auto(&fx, "agentA", true).await;
    let failed = wait_report(&fx, "agentA", "automatic", "2026-10-15", "failed").await;
    let failed_id = failed["lastRun"]["runId"].clone();
    let initial = ticker(&fx, "agentA").await;
    let failed_tick = wait_checks(&fx, "agentA", initial["checkCount"].as_u64().unwrap() + 2).await;
    assert_eq!(failed_tick["lastRunId"], failed_id);
    assert_eq!(failed_tick["executionState"], "failed");
    assert_eq!(failed_tick["state"], "automatic_failed");
    assert_eq!(failed_tick["lastError"], failed["lastRun"]["error"]);
    fx.restart().await;
    let restarted = wait_checks(&fx, "agentA", 2).await;
    assert_eq!(restarted["lastRunId"], failed_id);
    assert_eq!(restarted["executionState"], "failed");
    assert_eq!(restarted["paused"], false);
    assert_eq!(restarted["lastError"], failed["lastRun"]["error"]);
    assert_eq!(fx.requests.lock().unwrap().len(), 2);
    assert_eq!(
        fx.state
            .storage()
            .query_model_call_usage(Default::default())
            .await
            .unwrap()
            .len(),
        2
    );
    seed(&fx, "agentA", "- 下一日新的真实资料");
    let (entered, release) = fx.pause_next_provider();
    at(&fx, 16, 12, 0);
    entered.await.unwrap();
    let retry = ticker(&fx, "agentA").await;
    assert_eq!(retry["executionState"], "running");
    assert_ne!(retry["lastRunId"], failed_id);
    assert_eq!(retry["lastFailedRunId"], failed_id);
    assert_eq!(retry["lastError"], failed["lastRun"]["error"]);
    let health = fx
        .request(
            "GET",
            "/lingxi/v1/memories/health?agentId=agentA",
            Value::Null,
        )
        .await;
    let step = health.1["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "memory_ticker")
        .unwrap();
    assert_eq!(step["ok"], false, "新执行尚未成功，不得清除已有失败");
    assert!(step["error"].as_str().is_some());
    let disabled = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            json!({"memory":{"enabled":false}}),
        )
        .await;
    assert_eq!(disabled.0, 200);
    assert_eq!(ticker(&fx, "agentA").await["state"], "paused");
    release.send(()).unwrap();
    let cancelled = wait_report(&fx, "agentA", "automatic", "2026-10-16", "failed").await;
    assert_eq!(cancelled["lastRun"]["errorCode"], "dream_cancelled");
    let after = ticker(&fx, "agentA").await;
    let paused = wait_checks(&fx, "agentA", after["checkCount"].as_u64().unwrap() + 2).await;
    assert_eq!(
        paused["state"], "paused",
        "旧配置任务结算不能取消新配置的暂停"
    );
    assert_eq!(paused["paused"], true);
    assert_eq!(paused["executionState"], "failed");
    assert_eq!(paused["lastRunId"], cancelled["lastRun"]["runId"]);
    assert_eq!(paused["lastExecutionErrorCode"], "dream_cancelled");
    assert_eq!(fx.requests.lock().unwrap().len(), 3);
    let enabled = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            json!({"memory":{"enabled":true}}),
        )
        .await;
    assert_eq!(enabled.0, 200);
    let reenabled = wait_checks(&fx, "agentA", paused["checkCount"].as_u64().unwrap() + 2).await;
    assert_eq!(reenabled["paused"], false);
    assert_eq!(reenabled["state"], "automatic_failed");
    assert_eq!(fx.requests.lock().unwrap().len(), 3);
    at(&fx, 17, 12, 0);
    let success = wait_report(&fx, "agentA", "automatic", "2026-10-17", "succeeded").await;
    let current = ticker(&fx, "agentA").await;
    let good = wait_checks(&fx, "agentA", current["checkCount"].as_u64().unwrap() + 1).await;
    assert_eq!(good["lastRunId"], success["lastRun"]["runId"]);
    assert_eq!(good["executionState"], "succeeded");
    assert_eq!(good["state"], "automatic_succeeded");
    assert!(good["lastError"].is_null());
    assert!(good["lastExecutionErrorCode"].is_null());
    assert!(good["lastFailedRunId"].is_null());
    assert_eq!(fx.requests.lock().unwrap().len(), 8);
    let ledger = fx
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    assert_eq!(ledger.len(), 8);
    assert!(ledger.iter().all(|u| u.usage.is_some()));
    fx.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn genuinely_stopped_auto_host_still_preserves_live_manual_core() {
    let fx = Fixture::start("ticker-stopped-manual").await;
    seed(&fx, "agentA", "- 停止定时检查后仍可手动整理");
    assert!(fx.state.memory().dream().ticker().is_running());
    fx.state.memory().dream().ticker().stop();
    fx.state
        .memory()
        .dream()
        .wait_stopped_checked()
        .await
        .unwrap();
    assert!(!fx.state.memory().dream().ticker().is_running());
    let health = fx
        .request(
            "GET",
            "/lingxi/v1/memories/health?agentId=agentA",
            Value::Null,
        )
        .await;
    assert_eq!(health.0, 200);
    let step = health.1["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "memory_ticker")
        .unwrap();
    assert_eq!(step["ok"], false);
    assert_eq!(step["reason"], "ticker_absent_in_candidate");
    assert_eq!(step["ticker"]["hostRunning"], false);
    assert_eq!(step["ticker"]["state"], "stopped");
    let status = fx
        .request(
            "GET",
            "/lingxi/v1/memories/dream/status?agentId=agentA",
            Value::Null,
        )
        .await;
    assert_eq!(status.0, 200);
    assert_eq!(status.1["coreAvailable"], true);
    assert_eq!(status.1["tickerAvailable"], false);
    let manual = fx
        .request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            json!({}),
        )
        .await;
    assert_eq!(manual.0, 202, "{manual:?}");
    let report = wait_report(&fx, "agentA", "manual", "2026-10-15", "succeeded").await;
    assert_eq!(report["coreAvailable"], true);
    assert_eq!(report["tickerAvailable"], false);
    assert_eq!(fx.requests.lock().unwrap().len(), 5);
    let usage = fx
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    assert_eq!(usage.len(), 5);
    assert!(usage.iter().all(|row| row.usage.is_some()));
    assert_eq!(ticker(&fx, "agentA").await["hostRunning"], false);
    fx.finish().await;
}
