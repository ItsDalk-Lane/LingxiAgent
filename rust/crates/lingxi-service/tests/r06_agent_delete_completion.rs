//! R3 助手删除真实原子暂存、待清理收据与重启恢复。
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
    home: PathBuf,
    token: String,
    denied: String,
    chat: String,
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
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            config,
            &layout,
            ServiceDeps {
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
        let denied = state
            .auth()
            .issue_device_credential("user_local", &[], None)
            .unwrap()
            .secret;
        let chat = state
            .auth()
            .issue_device_credential("user_local", &["chat"], None)
            .unwrap()
            .secret;
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
            home,
            token,
            denied,
            chat,
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
async fn wait_result(fx: &Fixture, agent: &str) -> Value {
    for _ in 0..400 {
        let (code, status) = fx
            .request(
                "GET",
                &format!("/lingxi/v1/memories/dream/status?agentId={agent}"),
                Value::Null,
            )
            .await;
        assert_eq!(code, 200, "{status}");
        if status["status"] != "running" {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("dream未完成")
}
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_top_uchg_preserves_all_materials_and_live_core_after_failed_move() {
    let fx = Fixture::start("r3-top-immutable").await;
    seed(&fx, "agentA", "- 甲用户资料");
    seed(&fx, "agentB", "- 乙独立资料");
    let dir = fx.home.join("agents/agentA");
    let paths = [
        "config.yaml",
        "memory/facts.md",
        "memory/longterm.md",
        "memory/memory.md",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|name| std::fs::read(dir.join(name)).unwrap())
        .collect();
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
    assert!(std::process::Command::new("chflags")
        .arg("uchg")
        .arg(&dir)
        .status()
        .unwrap()
        .success());
    let deleted = fx
        .request("DELETE", "/lingxi/v1/agents/agentA", json!({}))
        .await;
    assert!(std::process::Command::new("chflags")
        .arg("nouchg")
        .arg(&dir)
        .status()
        .unwrap()
        .success());
    assert_eq!(deleted.0, 500, "{deleted:?}");
    for (name, bytes) in paths.iter().zip(&before) {
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), *bytes, "{name}");
    }
    assert_eq!(
        fx.request(
            "GET",
            "/lingxi/v1/memories/dream/status?agentId=agentA",
            Value::Null
        )
        .await
        .0,
        200
    );
    release.send(()).unwrap();
    assert_eq!(
        wait_result(&fx, "agentA").await["lastRun"]["errorCode"],
        "dream_cancelled"
    );
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
    assert_eq!(wait_result(&fx, "agentA").await["status"], "succeeded");
    let paid = fx
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
            purpose: Some("auxiliary.memory".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(paid.len(), 6);
    assert_eq!(fx.requests.lock().unwrap().len(), 6);
    assert!(paid.iter().all(|row| row.usage.is_some()));
    fx.finish().await;
}
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_cleanup_failure_is_logical_delete_with_receipt_and_restart_retry() {
    let mut fx = Fixture::start("r3-cleanup-pending").await;
    seed(&fx, "agentA", "- 甲独有资料");
    seed(&fx, "agentB", "- 乙仍存活资料");
    assert_eq!(
        fx.request("PUT", "/lingxi/v1/agents/primary", json!({"id":"agentA"}))
            .await
            .0,
        200
    );
    assert_eq!(
        fx.request("POST", "/lingxi/v1/agents/switch", json!({"id":"agentA"}))
            .await
            .0,
        200
    );
    let longterm = fx.home.join("agents/agentA/memory/longterm.md");
    assert!(std::process::Command::new("chflags")
        .arg("uchg")
        .arg(&longterm)
        .status()
        .unwrap()
        .success());
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
    let deletion = fx
        .request("DELETE", "/lingxi/v1/agents/agentA", json!({}))
        .await;
    assert_eq!(deletion.0, 200, "{deletion:?}");
    assert_eq!(deletion.1["logicalDeleted"], true);
    assert_eq!(deletion.1["cleanupPending"], true);
    assert_eq!(deletion.1["referencesPending"], false);
    assert_eq!(deletion.1["replacementAgentId"], "agentB");
    let id = deletion.1["deletionId"].as_str().unwrap().to_owned();
    let staged = fx
        .home
        .join("lingxi-service/agent-deletions")
        .join(&id)
        .join("data/memory/longterm.md");
    assert!(staged.is_file());
    assert!(!fx.home.join("agents/agentA").exists());
    let refs: Value =
        serde_json::from_slice(&std::fs::read(fx.home.join("user/preferences.json")).unwrap())
            .unwrap();
    assert_eq!(refs["primary_agent_id"], "agentB");
    assert_eq!(refs["current_agent_id"], "agentB");
    release.send(()).unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        fx.state.memory().dream().wait_stopped(),
    )
    .await
    .unwrap();
    assert!(!fx.home.join("agents/agentA").exists());
    assert_eq!(
        fx.state
            .storage()
            .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
                purpose: Some("auxiliary.memory".into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
    fx.restart().await;
    let receipts = fx
        .request("GET", "/lingxi/v1/agent-deletions", Value::Null)
        .await;
    assert_eq!(receipts.0, 200);
    assert_eq!(receipts.1["deletions"][0]["cleanupPending"], true);
    assert!(!fx.home.join("agents/agentA").exists());
    assert!(std::process::Command::new("chflags")
        .arg("nouchg")
        .arg(&staged)
        .status()
        .unwrap()
        .success());
    let cleaned = fx
        .request(
            "POST",
            &format!("/lingxi/v1/agent-deletions/{id}/cleanup"),
            json!({}),
        )
        .await;
    assert_eq!(cleaned.0, 200, "{cleaned:?}");
    assert_eq!(cleaned.1["status"], "completed");
    assert_eq!(cleaned.1["cleanupPending"], false);
    assert!(!staged.parent().unwrap().parent().unwrap().exists());
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
    assert_eq!(wait_result(&fx, "agentB").await["status"], "succeeded");
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_concurrent_deletions_refuse_last_agent_before_cancelling_it() {
    let fx = Fixture::start("r3-concurrent").await;
    seed(&fx, "agentA", "- 甲资料");
    seed(&fx, "agentB", "- 乙资料");
    let (a, b) = tokio::join!(
        fx.request("DELETE", "/lingxi/v1/agents/agentA", json!({})),
        fx.request("DELETE", "/lingxi/v1/agents/agentB", json!({}))
    );
    let mut codes = vec![a.0, b.0];
    codes.sort();
    assert_eq!(codes, vec![200, 400], "{a:?} {b:?}");
    let agents = fx.request("GET", "/lingxi/v1/agents", Value::Null).await;
    assert_eq!(agents.1["agents"].as_array().unwrap().len(), 1);
    let survivor = agents.1["agents"][0]["id"].as_str().unwrap();
    assert_eq!(
        fx.request(
            "POST",
            &format!("/lingxi/v1/memories/dream/runs?agentId={survivor}"),
            json!({})
        )
        .await
        .0,
        202
    );
    assert_eq!(wait_result(&fx, survivor).await["status"], "succeeded");
    fx.finish().await;
}
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_real_move_with_commit_receipt_failure_recovers_from_prepared_on_restart() {
    use lingxi_service::agents_admin::agent_deletions::{deletion_test_hook, DeletionPhase};
    let mut fx = Fixture::start("r3-restart-window").await;
    seed(&fx, "agentA", "- 真实暂存资料");
    let seen = Arc::new(Mutex::new(None::<PathBuf>));
    let hook_seen = seen.clone();
    let hook = deletion_test_hook(
        &fx.home,
        Arc::new(move |phase, dir| {
            if phase == DeletionPhase::AfterMoved {
                let path = dir.join("receipt.json");
                assert!(std::process::Command::new("chflags")
                    .arg("uchg")
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success());
                *hook_seen.lock().unwrap() = Some(path);
            }
        }),
    );
    let result = fx
        .request("DELETE", "/lingxi/v1/agents/agentA", json!({}))
        .await;
    drop(hook);
    assert_eq!(result.0, 200, "{result:?}");
    assert_eq!(result.1["logicalDeleted"], true);
    assert_eq!(result.1["cleanupPending"], true);
    assert!(result.1["journalError"].as_str().is_some());
    let receipt = seen.lock().unwrap().clone().unwrap();
    let raw: Value = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(raw["status"], "prepared");
    assert!(receipt
        .parent()
        .unwrap()
        .join("data/memory/facts.md")
        .is_file());
    assert!(!fx.home.join("agents/agentA").exists());
    let queried = fx
        .request("GET", "/lingxi/v1/agent-deletions", Value::Null)
        .await;
    assert_eq!(queried.0, 200);
    assert_eq!(queried.1["deletions"][0]["logicalDeleted"], true);
    assert_eq!(queried.1["deletions"][0]["journalCheckpointPending"], true);
    assert!(std::process::Command::new("chflags")
        .arg("nouchg")
        .arg(&receipt)
        .status()
        .unwrap()
        .success());
    fx.restart().await;
    let current = fx
        .request("GET", "/lingxi/v1/agent-deletions", Value::Null)
        .await;
    assert_eq!(current.0, 200);
    assert_eq!(current.1["deletions"][0]["logicalDeleted"], true);
    assert_eq!(current.1["deletions"][0]["status"], "completed");
    assert!(!receipt.parent().unwrap().join("data").exists());
    assert!(!fx.home.join("agents/agentA").exists());
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_receipts_and_cleanup_require_settings_scopes_and_ids_cannot_escape() {
    let fx = Fixture::start("r3-scopes").await;
    let (code, _, _) = fx
        .raw("GET", "/lingxi/v1/agent-deletions", &fx.chat, b"", None)
        .await;
    assert_eq!(code, 403);
    let (code, _, _) = fx
        .raw(
            "POST",
            "/lingxi/v1/agent-deletions/fake/cleanup",
            &fx.denied,
            b"{}",
            None,
        )
        .await;
    assert_eq!(code, 403);
    let code = fx
        .request(
            "POST",
            "/lingxi/v1/agent-deletions/bad%2F..%2F..%2Fescape/cleanup",
            json!({}),
        )
        .await
        .0;
    assert_eq!(code, 400);
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_reference_failure_is_explicit_pending_and_retry_uses_actual_preferences() {
    let fx = Fixture::start("r3-reference-pending").await;
    seed(&fx, "agentA", "- 待删除资料");
    assert_eq!(
        fx.request("PUT", "/lingxi/v1/agents/primary", json!({"id":"agentA"}))
            .await
            .0,
        200
    );
    assert_eq!(
        fx.request("POST", "/lingxi/v1/agents/switch", json!({"id":"agentA"}))
            .await
            .0,
        200
    );
    let prefs = fx.home.join("user/preferences.json");
    assert!(std::process::Command::new("chflags")
        .arg("uchg")
        .arg(&prefs)
        .status()
        .unwrap()
        .success());
    let deleted = fx
        .request("DELETE", "/lingxi/v1/agents/agentA", json!({}))
        .await;
    assert!(std::process::Command::new("chflags")
        .arg("nouchg")
        .arg(&prefs)
        .status()
        .unwrap()
        .success());
    assert_eq!(deleted.0, 200, "{deleted:?}");
    assert_eq!(deleted.1["logicalDeleted"], true);
    assert_eq!(deleted.1["referencesPending"], true);
    assert_eq!(deleted.1["cleanupPending"], true);
    assert_eq!(deleted.1["replacementAgentId"], Value::Null);
    assert!(deleted.1["referenceError"].as_str().is_some());
    let before: Value = serde_json::from_slice(&std::fs::read(&prefs).unwrap()).unwrap();
    assert_eq!(before["current_agent_id"], "agentA");
    assert!(!fx.home.join("agents/agentA").exists());
    let id = deleted.1["deletionId"].as_str().unwrap();
    let retried = fx
        .request(
            "POST",
            &format!("/lingxi/v1/agent-deletions/{id}/cleanup"),
            json!({}),
        )
        .await;
    assert_eq!(retried.0, 200, "{retried:?}");
    assert_eq!(retried.1["referencesPending"], false);
    assert_eq!(retried.1["cleanupPending"], false);
    let after: Value = serde_json::from_slice(&std::fs::read(&prefs).unwrap()).unwrap();
    assert_eq!(after["current_agent_id"], "agentB");
    assert_eq!(after["primary_agent_id"], "agentB");
    let current = fx
        .request(
            "GET",
            "/lingxi/v1/current-status?action=get&key=agent",
            Value::Null,
        )
        .await;
    assert_eq!(current.0, 200, "{current:?}");
    assert_eq!(current.1["agent"]["id"], "agentB");
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_restart_prunes_only_unpublished_metadata_before_any_move() {
    let mut fx = Fixture::start("r3-empty-intent").await;
    seed(&fx, "agentA", "- 公共资料必须保留");
    let facts = fx.home.join("agents/agentA/memory/facts.md");
    let before = std::fs::read(&facts).unwrap();
    // 这是首次持久收据发布前的真实磁盘残留，没有data且没有已提交删除。
    let orphan = fx
        .home
        .join("lingxi-service/agent-deletions/delete-123-456-0");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join(".receipt-123.tmp"), b"{unfinished").unwrap();
    fx.restart().await;
    assert!(!orphan.exists());
    assert_eq!(std::fs::read(&facts).unwrap(), before);
    assert_eq!(
        fx.request(
            "GET",
            "/lingxi/v1/memories/dream/status?agentId=agentA",
            Value::Null
        )
        .await
        .0,
        200
    );
    assert_eq!(
        fx.request("GET", "/lingxi/v1/agent-deletions", Value::Null)
            .await
            .1["deletions"],
        json!([])
    );
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r4_corrupt_aborted_record_is_rejected_by_get_cleanup_and_actual_bootstrap() {
    let fx = Fixture::start("r4-corrupt-aborted").await;
    seed(&fx, "agentA", "- 仍然现役的完整资料");
    assert_eq!(
        fx.request("PUT", "/lingxi/v1/agents/primary", json!({"id":"agentA"}))
            .await
            .0,
        200
    );
    assert_eq!(
        fx.request("POST", "/lingxi/v1/agents/switch", json!({"id":"agentA"}))
            .await
            .0,
        200
    );
    let dir = fx.home.join("agents/agentA");
    let paths = [
        "config.yaml",
        "memory/facts.md",
        "memory/longterm.md",
        "memory/memory.md",
        "memory/today.md",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|p| std::fs::read(dir.join(p)).unwrap())
        .collect();
    let prefs = fx.home.join("user/preferences.json");
    let prefs_before = std::fs::read(&prefs).unwrap();
    assert!(std::process::Command::new("chflags")
        .arg("uchg")
        .arg(&dir)
        .status()
        .unwrap()
        .success());
    let failed = fx
        .request("DELETE", "/lingxi/v1/agents/agentA", json!({}))
        .await;
    assert!(std::process::Command::new("chflags")
        .arg("nouchg")
        .arg(&dir)
        .status()
        .unwrap()
        .success());
    assert_eq!(failed.0, 500);
    let transaction = std::fs::read_dir(fx.home.join("lingxi-service/agent-deletions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let path = transaction.join("receipt.json");
    let original = std::fs::read(&path).unwrap();
    let mut corrupted: Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(corrupted["status"], "aborted");
    assert!(!transaction.join("data").exists());
    // 明确的人为收据损坏前置，没有发生正常rename，不把它包装成成功删除。
    for status in ["committed", "completed"] {
        corrupted["status"] = json!(status);
        corrupted["logicalDeleted"] = json!(true);
        corrupted["cleanupPending"] = json!(status == "committed");
        corrupted["referencesPending"] = json!(status == "committed");
        let bytes = serde_json::to_vec(&corrupted).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let id = corrupted["deletionId"].as_str().unwrap();
        let list = fx
            .request("GET", "/lingxi/v1/agent-deletions", Value::Null)
            .await;
        assert_eq!(list.0, 500, "{list:?}");
        let retried = fx
            .request(
                "POST",
                &format!("/lingxi/v1/agent-deletions/{id}/cleanup"),
                json!({}),
            )
            .await;
        assert_eq!(retried.0, 500, "{retried:?}");
        assert!(fx.state.memory().recover_agent_deletions().is_err());
        let boot = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: fx.home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
            },
            &prepare_layout(&fx.home).unwrap(),
            ServiceDeps {
                product_material_dir: Some(fx.home.parent().unwrap().join("product")),
                ..ServiceDeps::default()
            },
        )
        .await;
        let error = match boot {
            Err(error) => error.to_string(),
            Ok(_) => panic!("实际启动不能接受矛盾收据"),
        };
        assert!(error.contains("contradicts actual"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read(&prefs).unwrap(), prefs_before);
        for (name, bytes) in paths.iter().zip(&before) {
            assert_eq!(std::fs::read(dir.join(name)).unwrap(), *bytes);
        }
        assert_eq!(
            fx.request(
                "GET",
                "/lingxi/v1/memories/dream/status?agentId=agentA",
                Value::Null
            )
            .await
            .0,
            200
        );
    }
    std::fs::write(&path, original).unwrap();
    assert_eq!(
        fx.request("GET", "/lingxi/v1/agent-deletions", Value::Null)
            .await
            .0,
        200
    );
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
    assert_eq!(wait_result(&fx, "agentA").await["status"], "succeeded");
    assert_eq!(fx.requests.lock().unwrap().len(), 5);
    fx.finish().await;
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r4_completed_record_with_public_symlink_or_file_cannot_touch_target() {
    let fx = Fixture::start("r4-public-link").await;
    seed(&fx, "agentA", "- 真正待删除资料");
    let deleted = fx
        .request("DELETE", "/lingxi/v1/agents/agentA", json!({}))
        .await;
    assert_eq!(deleted.0, 200, "{deleted:?}");
    assert_eq!(deleted.1["status"], "completed");
    let id = deleted.1["deletionId"].as_str().unwrap();
    let public = fx.home.join("agents/agentA");
    let outside = fx.home.parent().unwrap().join("outside-user-dir");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("user.txt"), "绝不能清理的另一个用户文件").unwrap();
    let original = std::fs::read(outside.join("user.txt")).unwrap();
    // 对真实已完成收据注入矛盾的公共符号链接/普通文件，均须先拒绝。
    std::os::unix::fs::symlink(&outside, &public).unwrap();
    for link in [true, false] {
        if !link {
            std::fs::remove_file(&public).unwrap();
            std::fs::write(&public, "另一个普通文件").unwrap();
        }
        assert_eq!(
            fx.request("GET", "/lingxi/v1/agent-deletions", Value::Null)
                .await
                .0,
            500
        );
        assert_eq!(
            fx.request(
                "POST",
                &format!("/lingxi/v1/agent-deletions/{id}/cleanup"),
                json!({})
            )
            .await
            .0,
            500
        );
        assert_eq!(std::fs::read(outside.join("user.txt")).unwrap(), original);
        assert_eq!(
            fx.request(
                "GET",
                "/lingxi/v1/memories/dream/status?agentId=agentB",
                Value::Null
            )
            .await
            .0,
            200
        );
    }
    std::fs::remove_file(&public).unwrap();
    let valid = fx
        .request(
            "POST",
            &format!("/lingxi/v1/agent-deletions/{id}/cleanup"),
            json!({}),
        )
        .await;
    assert_eq!(valid.0, 200, "{valid:?}");
    assert_eq!(valid.1["status"], "completed");
    assert_eq!(valid.1["logicalDeleted"], true);
    assert_eq!(valid.1["cleanupPending"], false);
    fx.finish().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r4_completed_with_staged_data_is_rejected_but_committed_after_cleanup_is_valid() {
    let mut fx = Fixture::start("r4-stage-conflict").await;
    seed(&fx, "agentA", "- 原待删除资料");
    seed(&fx, "agentB", "- B真正用于正常整理的独立资料");
    let deleted = fx
        .request("DELETE", "/lingxi/v1/agents/agentA", json!({}))
        .await;
    assert_eq!(deleted.0, 200);
    let id = deleted.1["deletionId"].as_str().unwrap();
    let transaction = fx.home.join("lingxi-service/agent-deletions").join(id);
    let staged = transaction.join("data");
    std::fs::create_dir(&staged).unwrap();
    std::fs::write(staged.join("injected.txt"), "人为的矛盾暂存内容").unwrap();
    assert_eq!(
        fx.request("GET", "/lingxi/v1/agent-deletions", Value::Null)
            .await
            .0,
        500
    );
    assert_eq!(
        fx.request(
            "POST",
            &format!("/lingxi/v1/agent-deletions/{id}/cleanup"),
            json!({})
        )
        .await
        .0,
        500
    );
    assert_eq!(
        std::fs::read_to_string(staged.join("injected.txt")).unwrap(),
        "人为的矛盾暂存内容"
    );
    std::fs::remove_dir_all(&staged).unwrap();
    // 实际data已清完但最后completed收据尚未发布，是正常可恢复窗口。
    let receipt_path = transaction.join("receipt.json");
    let mut receipt: Value =
        serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
    receipt["status"] = json!("committed");
    receipt["cleanupPending"] = json!(true);
    std::fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    fx.restart().await;
    let listed = fx
        .request("GET", "/lingxi/v1/agent-deletions", Value::Null)
        .await;
    assert_eq!(listed.0, 200);
    assert_eq!(listed.1["deletions"][0]["status"], "completed");
    assert!(!fx.home.join("agents/agentA").exists());
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
    assert_eq!(wait_result(&fx, "agentB").await["status"], "succeeded");
    fx.finish().await;
}
