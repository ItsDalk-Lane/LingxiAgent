//! 206 助手设置真实 HTTP、隔离 Electron 原生产入口及磁盘副作用。
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
fn node_bin() -> std::ffi::OsString {
    std::env::var_os("R06_NODE_BIN").unwrap_or_else(|| "node".into())
}
fn evidence_dir() -> PathBuf {
    let directory = std::env::var_os("R06_SETTINGS_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("r06-settings-evidence-{}", std::process::id()))
        });
    std::fs::create_dir_all(&directory).unwrap();
    directory
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
            "settings-completion-{tag}-{}-{}",
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
        for yuan in ["lingxi", "butter", "ming", "kong"] {
            std::fs::write(
                product.join("yuan").join(format!("{yuan}.md")),
                format!("{yuan}真实元模板"),
            )
            .unwrap();
            std::fs::write(
                product
                    .join("identity-templates")
                    .join(format!("{yuan}.md")),
                format!("{yuan}真实默认身份"),
            )
            .unwrap();
            std::fs::write(
                product.join("agents-templates").join(format!("{yuan}.md")),
                format!("{yuan}真实默认行为"),
            )
            .unwrap();
        }
        let assets = root.join("desktop/src/assets");
        std::fs::create_dir_all(assets.join("character-cards")).unwrap();
        // 仅复制仓库随附头像到合成产品目录，浏览器使用真实图片解码与裁剪。
        let png = std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../desktop/src/assets/Lingxi.png"),
        )
        .unwrap();
        std::fs::write(assets.join("Lingxi.png"), &png).unwrap();
        std::fs::write(
            assets.join("character-cards/yuan-lingxi-card-back.png"),
            &png,
        )
        .unwrap();
        std::fs::write(assets.join("character-cards/yuan-lingxi-emblem.png"), &png).unwrap();
        for id in ["agentA", "agentB"] {
            let dir = home.join("agents").join(id);
            std::fs::create_dir_all(dir.join("memory")).unwrap();
            std::fs::write(dir.join("config.yaml"),format!("locale: zh\nagent:\n  name: 助手{id}\n  yuan: completion-persona\nmemory:\n  enabled: true\n")).unwrap();
        }
        for id in ["agentA", "agentB"] {
            let dir = home.join("agents").join(id);
            std::fs::write(dir.join("identity.md"), format!("{id}已保存身份")).unwrap();
            std::fs::write(dir.join("AGENTS.md"), format!("{id}已保存行为")).unwrap();
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
        let mut plane_json = json!({"providers":{"persona-stub":{"protocol":"openai-completions","endpoint":format!("http://{stub_addr}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-persona-key"}},"embedding-stub":{"protocol":"openai-embeddings","endpoint":format!("http://{stub_addr}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-embedding-key"}}},"models":{"chat":{"provider":"persona-stub","model":"chat-model","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"memory":{"provider":"persona-stub","model":"memory-model","compat":{"contextWindow":200000}},"embedding":{"provider":"embedding-stub","model":"embedding-model"}}});
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
        if tag == "workflow" {
            // 真注册插件工具与执行器；页面只编辑真实工具配置，不替换工具注册表。
            let gateway = state.tool_gateway().unwrap();
            let workspace = home.parent().unwrap().join("workspace");
            let runtime = lingxi_service::WorkerRuntime::new(
                lingxi_service::WorkerLimits::default(),
                None,
                Arc::new(lingxi_service::inject::SystemClock),
            );
            let access = Arc::new(
                lingxi_service::ResourceAccess::new(std::slice::from_ref(&workspace)).unwrap(),
            );
            let worker = workspace.join("settings-plugin-worker.cjs");
            std::fs::write(&worker, "process.stdin.resume();\n").unwrap();
            for name in ["beautify", "office"] {
                lingxi_service::register_worker_tool(
                    gateway.registry(),
                    gateway,
                    &runtime,
                    &access,
                    &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                    lingxi_service::WorkerToolSpec {
                        plugin_id: name.into(),
                        op: name.into(),
                        local_name: name.into(),
                        description: "设置页真实注册的合成插件".into(),
                        input_schema: json!({"type":"object","additionalProperties":false}),
                        path_args: Vec::new(),
                        argv: vec![
                            node_bin().to_string_lossy().into_owned(),
                            worker.to_string_lossy().into_owned(),
                        ],
                        env: Default::default(),
                        cwd: workspace.clone(),
                        model: state
                            .worker_model()
                            .cloned()
                            .unwrap_or_else(|| Arc::new(lingxi_service::UnconfiguredWorkerModel)),
                        allowed_model_purposes: Vec::new(),
                        claimed_file_contract: None,
                    },
                )
                .unwrap();
            }
        }
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
async fn electron_ui(fx: &Fixture, scenario: &str) -> Value {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let output_file = fx.home.join("ui-result.json");
    let output = tokio::process::Command::new(node_bin())
        .arg(repo.join("node_modules/vitest/vitest.mjs"))
        .args([
            "run",
            "desktop/src/react/settings/__tests__/R06AgentSettingsElectronLive.test.ts",
            "-t",
            "^rust_agent_settings_current_electron_real_service$",
            "--reporter=json",
            "--maxWorkers=1",
        ])
        .arg(format!("--outputFile={}", output_file.display()))
        .current_dir(&repo)
        .env("R06_SETTINGS_URL", format!("http://{}", fx.addr))
        .env(
            "R06_SETTINGS_TOKEN",
            if scenario == "read-denied" {
                &fx.denied
            } else {
                &fx.token
            },
        )
        .env("R06_SETTINGS_HOME", &fx.home)
        .env("R06_SETTINGS_CASE", scenario)
        .env("R06_NODE_BIN", node_bin())
        .output()
        .await
        .unwrap();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        eprintln!("SETTINGS-UI-CHILD[{scenario}]: {line}");
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        eprintln!("SETTINGS-UI-CHILD[{scenario}]: {line}");
    }
    assert!(output.status.success(), "actual Electron UI failed");
    let result: Value = serde_json::from_slice(&std::fs::read(output_file).unwrap()).unwrap();
    assert_eq!(result["numTotalTests"], 1);
    assert_eq!(result["numPassedTests"], 1);
    assert_eq!(result["numFailedTests"], 0);
    assert_eq!(result["numPendingTests"], 0);
    assert_eq!(
        result["testResults"][0]["assertionResults"][0]["fullName"],
        "rust_agent_settings_current_electron_real_service"
    );
    serde_json::from_slice(&std::fs::read(fx.home.join("electron-result.json")).unwrap()).unwrap()
}
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_agent_settings_current_electron_entry_smoke() {
    let fx = Fixture::start("smoke").await;
    seed(&fx, "agentA", "- 用户喜欢中文解释");
    let result = electron_ui(&fx, "smoke").await;
    assert_eq!(result["checks"].as_array().unwrap().len(), 6);
    fx.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_settings_reads_keep_real_failures_and_paid_dream_consumption() {
    let fx = Fixture::start("reads").await;
    let (status, snapshot) = fx
        .request(
            "GET",
            "/lingxi/v1/settings/snapshot?agentId=agentA",
            Value::Null,
        )
        .await;
    assert_eq!(status, 200, "{snapshot}");
    assert_eq!(snapshot["agentId"], "agentA");
    assert_eq!(snapshot["identity"], "agentA已保存身份");
    assert_eq!(snapshot["agents"], "agentA已保存行为");
    assert_eq!(
        snapshot["globalModels"]["models"]["memory"]["id"],
        "memory-model"
    );
    assert_eq!(snapshot["experience"], "");
    assert!(snapshot.to_string().find("synthetic-persona-key").is_none());
    let (status, _, _) = fx
        .raw(
            "GET",
            "/lingxi/v1/settings/snapshot?agentId=agentA",
            &fx.chat,
            b"",
            None,
        )
        .await;
    assert_eq!(status, 403);
    let (status, _, _) = fx
        .raw("GET", "/lingxi/v1/models", &fx.denied, b"", None)
        .await;
    assert_eq!(status, 403);
    let b_before = std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap();
    std::fs::write(fx.home.join("provider-catalog.json"), "{broken catalog").unwrap();
    for route in [
        "/lingxi/v1/providers/summary",
        "/lingxi/v1/models",
        "/lingxi/v1/settings/snapshot?agentId=agentA",
    ] {
        let (status, response) = fx.request("GET", route, Value::Null).await;
        assert_eq!(status, 500, "{route}: {response}");
        assert!(response["error"].is_string());
        assert!(response.get("providers").is_none());
        assert!(response.get("models").is_none());
        assert!(response.get("config").is_none());
    }
    std::fs::remove_file(fx.home.join("provider-catalog.json")).unwrap();
    let compiled = fx.home.join("agents/agentA/memory/facts.md");
    std::fs::create_dir(&compiled).unwrap();
    let (status, body) = fx
        .request(
            "GET",
            "/lingxi/v1/memories/compiled?agentId=agentA",
            Value::Null,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert!(body.get("sections").is_none());
    std::fs::remove_dir(&compiled).unwrap();
    assert_eq!(
        fx.request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            json!({"experience":{"enabled":true}})
        )
        .await
        .0,
        200
    );
    let experience = fx.home.join("agents/agentA/experience");
    std::fs::create_dir_all(&experience).unwrap();
    std::fs::write(experience.join("invalid.md"), [0xff, 0xfe]).unwrap();
    let (status, body) = fx
        .request("GET", "/lingxi/v1/agents/agentA/experience", Value::Null)
        .await;
    assert_eq!(status, 500, "{body}");
    assert!(body.get("content").is_none());
    std::fs::remove_file(experience.join("invalid.md")).unwrap();
    std::fs::create_dir_all(fx.home.join("user")).unwrap();
    std::fs::create_dir(fx.home.join("user/user.md")).unwrap();
    let (status, body) = fx
        .request("GET", "/lingxi/v1/user-profile", Value::Null)
        .await;
    assert_eq!(status, 500, "{body}");
    assert!(body.get("content").is_none());
    std::fs::remove_dir(fx.home.join("user/user.md")).unwrap();
    seed(&fx, "agentA", "- 用户希望使用中文讨论音乐");
    let (entered, release) = fx.pause_next_provider();
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            Value::Null
        )
        .await
        .0,
        202
    );
    entered.await.unwrap();
    assert_eq!(
        fx.request(
            "GET",
            "/lingxi/v1/memories/dream/status?agentId=agentA",
            Value::Null
        )
        .await
        .1["status"],
        "running"
    );
    assert_eq!(
        fx.request(
            "GET",
            "/lingxi/v1/settings/snapshot?agentId=agentA",
            Value::Null
        )
        .await
        .0,
        200
    );
    release.send(()).unwrap();
    assert_eq!(wait_result(&fx, "agentA").await["status"], "succeeded");
    let charges = fx
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
            purpose: Some("auxiliary.memory".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(charges.len(), 5);
    assert_eq!(fx.requests.lock().unwrap().len(), 5);
    assert!(charges.iter().all(|charge| charge.usage.is_some()));
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        b_before
    );
    fx.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_character_card_zip_roundtrip_real_materials_and_memory_choice() {
    let fx = Fixture::start("cards").await;
    seed(&fx, "agentA", "- 角色卡中真实已保存事实");
    let avatar = std::fs::read(
        fx.home
            .parent()
            .unwrap()
            .join("desktop/src/assets/Lingxi.png"),
    )
    .unwrap();
    std::fs::write(fx.home.join("agents/agentA/avatar.png"), &avatar).unwrap();
    let tags = vec!["角色卡".to_string()];
    fx.state
        .memory()
        .fact_store("agentA")
        .unwrap()
        .import_all(&[lingxi_adapters::storage::agent_memory::FactInsert {
            fact: "真实结构事实",
            tags: &tags,
            time: None,
            session_id: Some("synthetic-session"),
        }])
        .unwrap();
    let skill = fx.home.join("skills/card-owned");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: card-owned\n---\n\n真实角色卡技能\n",
    )
    .unwrap();
    assert_eq!(
        fx.request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            json!({"skills":{"enabled":["card-owned"]}})
        )
        .await
        .0,
        200
    );
    let b_before = std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap();
    let (status, body) = fx
        .request(
            "POST",
            "/lingxi/v1/character-cards/export/preview",
            json!({"agentId":"agentA"}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["plan"]["memory"]["available"], true);
    assert_eq!(body["plan"]["skills"]["count"], 1);
    assert_eq!(body["plan"]["assets"]["avatar"], true);
    let (status, _, bytes) = fx
        .raw(
            "GET",
            "/lingxi/v1/character-cards/export/agentA/assets/avatar",
            &fx.token,
            b"",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(bytes, avatar);
    for export_memory in [false, true] {
        let (status, exported) = fx
            .request(
                "POST",
                "/lingxi/v1/character-cards/export",
                json!({"agentId":"agentA","exportMemory":export_memory}),
            )
            .await;
        assert_eq!(status, 200, "{exported}");
        let path = PathBuf::from(exported["filePath"].as_str().unwrap());
        assert!(path.starts_with(fx.home.parent().unwrap().join("workspace")));
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut descriptor = Vec::new();
        std::io::Read::read_to_end(&mut zip.by_name("card.json").unwrap(), &mut descriptor)
            .unwrap();
        let card: Value = serde_json::from_slice(&descriptor).unwrap();
        assert_eq!(card.get("memory").is_some(), export_memory);
        assert_eq!(card["prompts"]["identity"], "agentA已保存身份");
        assert_eq!(
            card["skills"]["bundles"][0]["skills"][0]["name"],
            "card-owned"
        );
        let (status, planned) = fx
            .request(
                "POST",
                "/lingxi/v1/character-cards/plan",
                json!({"path":path}),
            )
            .await;
        assert_eq!(status, 200, "{planned}");
        let token = planned["plan"]["token"].as_str().unwrap();
        assert_eq!(planned["plan"]["memory"]["available"], export_memory);
        let (status, imported) = fx
            .request(
                "POST",
                "/lingxi/v1/character-cards/import",
                json!({"token":token,"importMemory":export_memory}),
            )
            .await;
        assert_eq!(status, 200, "{imported}");
        let id = imported["agent"]["id"].as_str().unwrap();
        assert_ne!(id, "agentA");
        assert_ne!(id, "agentB");
        assert_eq!(
            std::fs::read_to_string(fx.home.join("agents").join(id).join("identity.md")).unwrap(),
            "agentA已保存身份"
        );
        assert_eq!(
            std::fs::read(fx.home.join("agents").join(id).join("avatars/agent.png")).unwrap(),
            avatar
        );
        assert_eq!(
            fx.state.memory().fact_store(id).unwrap().size().unwrap() > 0,
            export_memory
        );
        let sections = fx
            .state
            .memory()
            .compiled_store(id)
            .unwrap()
            .read_sections()
            .unwrap();
        assert_eq!(!sections.facts.is_empty(), export_memory);
        assert_eq!(
            fx.request(
                "POST",
                "/lingxi/v1/character-cards/import",
                json!({"token":token,"importMemory":true})
            )
            .await
            .0,
            409
        );
        let installed = imported["installedSkills"][0]["name"].as_str().unwrap();
        assert_ne!(installed, "card-owned");
        assert!(fx
            .home
            .join("skills")
            .join(installed)
            .join("SKILL.md")
            .is_file());
    }
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        b_before
    );
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_settings_nineteen_actions_actual_electron_and_service() {
    let fx = Fixture::start("workflow").await;
    seed(&fx, "agentA", "- Electron实际事实");
    fx.state
        .memory()
        .experience_store("agentA")
        .unwrap()
        .sync_categories(&[("协作经验".into(), "1. 真实经验条目".into())])
        .unwrap();
    fx.state
        .memory()
        .tenets_store("agentA")
        .unwrap()
        .add_proposal("待批准真实原则", Some("high"), None, "pending-real".into())
        .unwrap();
    fx.state
        .memory()
        .tenets_store("agentA")
        .unwrap()
        .add_proposal("待拒绝真实原则", Some("low"), None, "pending-reject".into())
        .unwrap();
    let date = (chrono::Utc::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let daily = fx.home.join("agents/agentA/memory/daily");
    std::fs::create_dir_all(&daily).unwrap();
    std::fs::write(daily.join(format!("{date}.md")), "前一日真实记忆").unwrap();
    fx.state
        .memory()
        .compiled_store("agentA")
        .unwrap()
        .write_week_day(&date, "前一日真实记忆")
        .unwrap();
    let tags = vec!["实际".to_string()];
    fx.state
        .memory()
        .fact_store("agentA")
        .unwrap()
        .import_all(&[lingxi_adapters::storage::agent_memory::FactInsert {
            fact: "Electron实际结构事实",
            tags: &tags,
            time: None,
            session_id: None,
        }])
        .unwrap();
    let result = electron_ui(&fx, "workflow").await;
    assert!(result["checks"].as_array().unwrap().len() >= 25);
    let charges = fx
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery::default())
        .await
        .unwrap();
    // 开启自动开关已授权即时调度，另一个用户点击才是手动整理；按各run精确核对五阶段。
    let mut allowed = std::collections::HashSet::new();
    for trigger in ["automatic", "manual"] {
        let report = &result["dreamRuns"][trigger];
        assert_eq!(report["trigger"], trigger);
        assert_eq!(report["status"], "succeeded");
        let run_id = report["runId"].as_str().unwrap();
        assert!(
            allowed.insert(run_id.to_string()),
            "自动与手动必须是不同运行"
        );
        let rows = charges
            .iter()
            .filter(|charge| {
                charge.purpose == "auxiliary.memory"
                    && charge.parent_run_id.as_deref() == Some(run_id)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows.len(),
            5,
            "{trigger} 的五阶段不能少计、重复或混入另一运行"
        );
        let expected = ["atomize", "dedupe", "optimize", "compose", "verify"];
        for stage in expected {
            let origin = format!("memory.dream.{stage}");
            let matching = rows
                .iter()
                .filter(|charge| charge.origin == origin)
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1, "{trigger}/{stage}");
            let row = matching[0];
            let usage = row.usage.as_ref().unwrap();
            assert_eq!(usage.input_tokens, Some(23));
            assert_eq!(usage.output_tokens, Some(8));
            assert_eq!(row.outcome, lingxi_kernel::usage::CallOutcome::Succeeded);
            assert!(row.model_call_id.starts_with(&format!("{run_id}-{stage}-")));
            assert_eq!(row.cause_ref.as_deref(), Some("agent:agentA"));
        }
    }
    let memory_rows = charges
        .iter()
        .filter(|charge| charge.purpose == "auxiliary.memory")
        .collect::<Vec<_>>();
    assert_eq!(memory_rows.len(), allowed.len() * 5);
    assert!(memory_rows.iter().all(|charge| charge
        .parent_run_id
        .as_ref()
        .is_some_and(|id| allowed.contains(id))));
    let calls = fx.requests.lock().unwrap().clone();
    for stage in ["atomize", "dedupe", "optimize", "compose", "verify"] {
        assert_eq!(
            calls
                .iter()
                .filter(|request| request["messages"][0]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with(&format!("memory.dream.{stage}：")))
                .count(),
            2,
            "供应商真实收到自动/手动各一次{stage}"
        );
    }
    std::fs::write(
        fx.home.join("dream-charge-proof.json"),
        serde_json::to_vec_pretty(
            &json!({"runs":result["dreamRuns"],"charges":charges.iter().map(|row|json!({"id":row.model_call_id,"parentRunId":row.parent_run_id,"purpose":row.purpose,"origin":row.origin,"outcome":row.outcome.wire_name(),"inputTokens":row.usage.as_ref().and_then(|usage|usage.input_tokens),"outputTokens":row.usage.as_ref().and_then(|usage|usage.output_tokens)})).collect::<Vec<_>>(),"supplierCalls":calls}),
        )
        .unwrap(),
    )
    .unwrap();
    // 创建/导入后的真实简介读取还会调用模型；所有真实付费请求都必须有计量，不能固定只数 Dream。
    assert_eq!(
        charges
            .iter()
            .filter(|charge| charge.usage.is_some())
            .count(),
        fx.requests.lock().unwrap().len()
    );
    let evidence = evidence_dir();
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(
        evidence.join("workflow-electron.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    std::fs::copy(
        fx.home.join("dream-charge-proof.json"),
        evidence.join("dream-charge-proof.json"),
    )
    .unwrap();
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_pending_reference_deletion_current_electron_shows_and_retries_truthfully() {
    let mut fx = Fixture::start("references-pending").await;
    seed(&fx, "agentA", "- 删除引用待完成资料");
    assert_eq!(
        fx.request("PUT", "/lingxi/v1/agents/primary", json!({"id":"agentB"}))
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
    // 排序中保留A引用；即使B焦点/主助手已提交，删除后仍必须真实写回引用。
    assert_eq!(
        fx.request(
            "PUT",
            "/lingxi/v1/agents/order",
            json!({"order":["agentA","agentB"]})
        )
        .await
        .0,
        200
    );
    let prefs = fx.home.join("user/preferences.json");
    // 替代B的真实焦点先提交，再在既有AfterMoved时点令引用持久化失败。
    use lingxi_service::agents_admin::agent_deletions::{deletion_test_hook, DeletionPhase};
    let fault_preferences = prefs.clone();
    let _fault = deletion_test_hook(
        &fx.home,
        Arc::new(move |phase, _| {
            if phase == DeletionPhase::AfterMoved {
                let focus: Value =
                    serde_json::from_slice(&std::fs::read(&fault_preferences).unwrap()).unwrap();
                assert_eq!(focus["current_agent_id"], "agentB");
                assert_eq!(focus["agent_order"], json!(["agentA", "agentB"]));
                assert!(std::process::Command::new("/usr/bin/chflags")
                    .arg("uchg")
                    .arg(&fault_preferences)
                    .status()
                    .unwrap()
                    .success());
            }
        }),
    );
    let result = electron_ui(&fx, "references-pending").await;
    assert_eq!(result["checks"].as_array().unwrap().len(), 12);
    fx.restart().await;
    let (code, receipts) = fx
        .request("GET", "/lingxi/v1/agent-deletions", Value::Null)
        .await;
    assert_eq!(code, 200);
    assert_eq!(receipts["deletions"][0]["status"], "completed");
    let evidence = evidence_dir();
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(
        evidence.join("references-pending-electron.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_settings_current_electron_real_io_failures_and_partial_writes() {
    let fx = Fixture::start("failures").await;
    seed(&fx, "agentA", "- 设置失败仍保留真实事实");
    let b_before = std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap();
    let result = electron_ui(&fx, "failures").await;
    assert_eq!(result["checks"].as_array().unwrap().len(), 18);
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        b_before
    );
    let evidence = evidence_dir();
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(
        evidence.join("failures-electron.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_settings_current_electron_actual_read_permission_denied_is_error() {
    let fx = Fixture::start("read-denied").await;
    let before = std::fs::read(fx.home.join("agents/agentA/config.yaml")).unwrap();
    let result = electron_ui(&fx, "read-denied").await;
    assert_eq!(result["checks"].as_array().unwrap().len(), 4);
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentA/config.yaml")).unwrap(),
        before
    );
    let evidence = evidence_dir();
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(
        evidence.join("read-denied-electron.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    fx.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_character_card_real_zip_refuses_escape_links_and_expansion_without_agents() {
    let fx = Fixture::start("zip-refusal").await;
    let before_a = std::fs::read(fx.home.join("agents/agentA/config.yaml")).unwrap();
    let before_b = std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap();
    for (kind, expected) in [
        ("escape", "safe relative path"),
        ("link", "links are forbidden"),
        ("expanded", "expands beyond limit"),
    ] {
        let zip_path = fx
            .home
            .parent()
            .unwrap()
            .join("workspace")
            .join(format!("{kind}.zip"));
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if kind == "link" {
            writer
                .add_symlink("avatar.png", "../../../must-not-read", options)
                .unwrap();
        } else {
            writer
                .start_file(
                    if kind == "escape" {
                        "../must-not-create"
                    } else {
                        "too-large.txt"
                    },
                    options,
                )
                .unwrap();
            if kind == "expanded" {
                let block = vec![0u8; 1024 * 1024];
                for _ in 0..81 {
                    std::io::Write::write_all(&mut writer, &block).unwrap();
                }
            } else {
                std::io::Write::write_all(&mut writer, b"forbidden").unwrap();
            }
        }
        writer.finish().unwrap();
        let (code, body) = fx
            .request(
                "POST",
                "/lingxi/v1/character-cards/plan",
                json!({"path":zip_path}),
            )
            .await;
        assert_eq!(code, 400, "{kind}: {body}");
        assert!(body["error"].as_str().unwrap().contains(expected), "{body}");
        assert!(body.get("plan").is_none());
    }
    assert!(!fx
        .home
        .join(".ephemeral/character-card-imports/must-not-create")
        .exists());
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentA/config.yaml")).unwrap(),
        before_a
    );
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        before_b
    );
    assert_eq!(
        fx.request("GET", "/lingxi/v1/agents", Value::Null).await.1["agents"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    fx.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_character_card_application_and_http_delete_never_recreate_deleted_directory() {
    use lingxi_adapters::storage::agent_memory as am;
    let fx = Fixture::start("card-delete-concurrent").await;
    let b_before = std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap();
    let input = fx
        .home
        .parent()
        .unwrap()
        .join("workspace/concurrent-card.json");
    std::fs::write(&input, json!({"agent":{"id":"card-owned","name":"真实并发卡"},"prompts":{"identity":"卡片真实身份"},"memory":{"compiled":{"facts":"- 卡片真实事实"}}}).to_string()).unwrap();
    let (code, planned) = fx
        .request(
            "POST",
            "/lingxi/v1/character-cards/plan",
            json!({"path":input}),
        )
        .await;
    assert_eq!(code, 200, "{planned}");
    let token = planned["plan"]["token"].as_str().unwrap();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered_tx)));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release = Arc::new(Mutex::new(release_rx));
    let memory_dir = fx
        .home
        .canonicalize()
        .unwrap()
        .join("agents/card-owned/memory");
    let _hook = am::memory_operation_test_hook(
        &memory_dir,
        Arc::new(move |kind| {
            if kind == am::MemoryOperationKind::Compile {
                if let Some(sender) = entered.lock().unwrap().take() {
                    sender.send(()).unwrap();
                    release
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(10))
                        .unwrap();
                }
            }
        }),
    );
    let importer = fx.request(
        "POST",
        "/lingxi/v1/character-cards/import",
        json!({"token":token,"importMemory":true}),
    );
    // 真正未收完的 multipart 大上传保持打开；它不应占用应用/删除门。
    let mut upload = TcpStream::connect(fx.addr).await.unwrap();
    let header = format!("POST /lingxi/v1/character-cards/plan HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: multipart/form-data; boundary=fixture-card\r\nContent-Length: 52428800\r\n\r\n--fixture-card\r\nContent-Disposition: form-data; name=\"file\"; filename=\"unfinished.zip\"\r\nContent-Type: application/zip\r\n\r\n", fx.token);
    upload.write_all(header.as_bytes()).await.unwrap();
    upload.write_all(&vec![0u8; 512 * 1024]).await.unwrap();
    let deletion = async {
        tokio::time::timeout(Duration::from_secs(10), entered_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(fx.home.join("agents/card-owned/config.yaml").is_file());
        let invalid = tokio::time::timeout(
            Duration::from_millis(200),
            fx.request(
                "POST",
                "/lingxi/v1/character-cards/import",
                json!({"token":"invalid"}),
            ),
        )
        .await
        .expect("无效确认不能等待应用门");
        assert_eq!(invalid.0, 400);
        let deletion = fx.request("DELETE", "/lingxi/v1/agents/card-owned", json!({}));
        tokio::pin!(deletion);
        let early = tokio::time::timeout(Duration::from_millis(200), &mut deletion).await;
        // 先释放真实存储门，再作断言，修前失败也不会遗留阻塞服务任务。
        release_tx.send(()).unwrap();
        let completed_while_paused = early.is_ok();
        let deleted = match early {
            Ok(value) => value,
            Err(_) => deletion.await,
        };
        (completed_while_paused, deleted)
    };
    let (imported, (completed_while_paused, deleted)) = tokio::join!(importer, deletion);
    let evidence = evidence_dir();
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(evidence.join(if completed_while_paused { "card-delete-unguarded.json" } else { "card-delete-guarded.json" }), serde_json::to_vec_pretty(&json!({"deleteCompletedWhileWritePaused":completed_while_paused,"importStatus":imported.0,"deleteStatus":deleted.0,"publicDirectoryExistsAfterBoth":fx.home.join("agents/card-owned").exists(),"agentBUnchanged":std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap()==b_before})).unwrap()).unwrap();
    assert!(
        !completed_while_paused,
        "DELETE completed while an actual card write was paused: {deleted:?}"
    );
    assert_eq!(imported.0, 200, "{imported:?}");
    assert_eq!(deleted.0, 200, "{deleted:?}");
    assert_eq!(deleted.1["logicalDeleted"], true);
    assert_eq!(deleted.1["cleanupPending"], false);
    assert!(!fx.home.join("agents/card-owned").exists());
    let mut probe = [0u8; 1];
    assert!(
        tokio::time::timeout(Duration::from_millis(30), upload.peek(&mut probe))
            .await
            .is_err(),
        "未完成上传仍在等待，删除却已真实完成"
    );
    drop(upload);
    assert_eq!(
        fx.request("GET", "/lingxi/v1/agents/card-owned/config", Value::Null)
            .await
            .0,
        404
    );
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentB/config.yaml")).unwrap(),
        b_before
    );
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/character-cards/import",
            json!({"token":token,"importMemory":true})
        )
        .await
        .0,
        409
    );
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_pending_receipt_owner_switch_two_real_services_never_posts_to_other_service() {
    let a = Fixture::start("receipt-owner-a").await;
    let b = Fixture::start("receipt-owner-b").await;
    seed(&a, "agentA", "- A 独占待清理资料");
    assert_eq!(
        a.request("POST", "/lingxi/v1/agents/switch", json!({"id":"agentA"}))
            .await
            .0,
        200
    );
    // 排序中保留A引用；即使B焦点/主助手已提交，删除后仍必须真实写回引用。
    assert_eq!(
        a.request(
            "PUT",
            "/lingxi/v1/agents/order",
            json!({"order":["agentA","agentB"]})
        )
        .await
        .0,
        200
    );
    let prefs = a.home.join("user/preferences.json");
    // 替代B的真实焦点先提交，再在既有AfterMoved时点令引用持久化失败。
    use lingxi_service::agents_admin::agent_deletions::{deletion_test_hook, DeletionPhase};
    let fault_preferences = prefs.clone();
    let _fault = deletion_test_hook(
        &a.home,
        Arc::new(move |phase, _| {
            if phase == DeletionPhase::AfterMoved {
                let focus: Value =
                    serde_json::from_slice(&std::fs::read(&fault_preferences).unwrap()).unwrap();
                assert_eq!(focus["current_agent_id"], "agentB");
                assert_eq!(focus["agent_order"], json!(["agentA", "agentB"]));
                assert!(std::process::Command::new("/usr/bin/chflags")
                    .arg("uchg")
                    .arg(&fault_preferences)
                    .status()
                    .unwrap()
                    .success());
            }
        }),
    );
    let (code, receipt) = a
        .request("DELETE", "/lingxi/v1/agents/agentA", json!({}))
        .await;
    assert_eq!(code, 200, "{receipt}");
    assert_eq!(receipt["referencesPending"], true);
    let b_config = std::fs::read(b.home.join("agents/agentA/config.yaml")).unwrap();
    std::fs::write(
        a.home.join("other-connection.json"),
        serde_json::to_vec(&json!({"url":format!("http://{}",b.addr),"token":b.token})).unwrap(),
    )
    .unwrap();
    let result = electron_ui(&a, "receipt-owner").await;
    assert_eq!(result["checks"].as_array().unwrap().len(), 11);
    assert_eq!(
        std::fs::read(b.home.join("agents/agentA/config.yaml")).unwrap(),
        b_config
    );
    assert_eq!(
        b.request("GET", "/lingxi/v1/agent-deletions", Value::Null)
            .await
            .1["deletions"],
        json!([])
    );
    std::fs::write(
        evidence_dir().join("receipt-owner-electron.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    assert!(std::process::Command::new("/usr/bin/chflags")
        .arg("nouchg")
        .arg(&prefs)
        .status()
        .unwrap()
        .success());
    a.finish().await;
    b.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_character_card_duplicate_skill_source_and_sparse_path_refuse_without_public_mutation(
) {
    let fx = Fixture::start("card-budget").await;
    let original = fx.request("GET", "/lingxi/v1/agents", Value::Null).await.1;
    let skills_before = fx.home.join("skills");
    let skill_count = std::fs::read_dir(&skills_before)
        .map(|entries| entries.count())
        .unwrap_or(0);
    let input = fx.home.parent().unwrap().join("workspace/repeated.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&input).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let card = json!({"agent":{"name":"不得创建"},"skills":{"bundles":[{"name":"重复包","skills":vec![json!({"path":"skills/one"});16]}]}});
    writer.start_file("card.json", options).unwrap();
    std::io::Write::write_all(&mut writer, card.to_string().as_bytes()).unwrap();
    writer.start_file("skills/one/SKILL.md", options).unwrap();
    std::io::Write::write_all(
        &mut writer,
        b"---\nname: unique-owned\n---\nsmall real skill",
    )
    .unwrap();
    writer.finish().unwrap();
    let (code, result) = fx
        .request(
            "POST",
            "/lingxi/v1/character-cards/plan",
            json!({"path":input}),
        )
        .await;
    assert_eq!(code, 400, "{result}");
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("duplicate skill"));
    let huge = fx
        .home
        .parent()
        .unwrap()
        .join("workspace/metadata-only.json");
    let file = std::fs::File::create(&huge).unwrap();
    file.set_len(81 * 1024 * 1024).unwrap();
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(&huge).unwrap().blocks(),
            0,
            "只用稀疏元数据，不写大文件内容"
        );
    }
    let (code, result) = fx
        .request(
            "POST",
            "/lingxi/v1/character-cards/plan",
            json!({"path":huge}),
        )
        .await;
    assert_eq!(code, 400, "{result}");
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("size exceeds limit"));
    assert_eq!(
        fx.request("GET", "/lingxi/v1/agents", Value::Null).await.1,
        original
    );
    assert_eq!(
        std::fs::read_dir(&skills_before)
            .map(|entries| entries.count())
            .unwrap_or(0),
        skill_count
    );
    assert_eq!(
        std::fs::read_dir(fx.home.join(".ephemeral/character-card-imports"))
            .unwrap()
            .count(),
        0,
        "拒绝前清理私有临时资料"
    );
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_character_card_owner_two_real_services_and_delayed_import_response() {
    let a = Fixture::start("card-owner-a").await;
    let b = Fixture::start("card-owner-b").await;
    seed(&a, "agentA", "- A 独有卡片记忆");
    assert_eq!(
        b.request(
            "POST",
            "/lingxi/v1/agents",
            json!({"name":"B 同名卡片助手","id":"card-from-a"})
        )
        .await
        .0,
        200
    );
    let b_agents = b.request("GET", "/lingxi/v1/agents", Value::Null).await.1;
    let b_config = std::fs::read(b.home.join("agents/card-from-a/config.yaml")).unwrap();
    std::fs::write(a.home.parent().unwrap().join("workspace/owner-card.json"), json!({"agent":{"id":"card-from-a","name":"A 专属导入"},"prompts":{"identity":"A专属身份"},"memory":{"compiled":{"facts":"- A 独有卡片记忆"}}}).to_string()).unwrap();
    std::fs::write(
        a.home.join("other-connection.json"),
        serde_json::to_vec(&json!({"url":format!("http://{}",b.addr),"token":b.token})).unwrap(),
    )
    .unwrap();
    let result = electron_ui(&a, "card-owner").await;
    assert_eq!(result["checks"].as_array().unwrap().len(), 14);
    assert_eq!(
        b.request("GET", "/lingxi/v1/agents", Value::Null).await.1,
        b_agents
    );
    assert_eq!(
        std::fs::read(b.home.join("agents/card-from-a/config.yaml")).unwrap(),
        b_config
    );
    std::fs::write(
        evidence_dir().join("card-owner-electron.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    a.finish().await;
    b.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_initialization_cache_summary_and_real_fifteen_second_timeout() {
    let fx = Fixture::start("initialization").await;
    seed(&fx, "agentA", "- 初始化真实资料");
    let result = electron_ui(&fx, "initialization").await;
    assert_eq!(result["checks"].as_array().unwrap().len(), 12);
    std::fs::write(
        evidence_dir().join("initialization-electron.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    fx.finish().await;
}
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_remaining_tenet_health_and_order_real_failures_and_pointer_cancel() {
    let fx = Fixture::start("boundaries").await;
    seed(&fx, "agentA", "- 边界真实资料");
    fx.state
        .memory()
        .tenets_store("agentA")
        .unwrap()
        .add_direct("已有真实原则", Some("high"), "existing-native-tenet".into())
        .unwrap();
    let result = electron_ui(&fx, "boundaries").await;
    assert_eq!(result["checks"].as_array().unwrap().len(), 16);
    std::fs::write(
        evidence_dir().join("remaining-boundaries-electron.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    fx.finish().await;
}
