//! 八个人格原叶的真实认证 HTTP、磁盘、事件、花名册与下一模型请求证明。
//! 只替换外部供应商回答，不替换 PersonaService、配置、存储或编译器。
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
    product: PathBuf,
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
            "persona-completion-{tag}-{}-{}",
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
            std::fs::write(dir.join("config.yaml"),format!("locale: zh\nagent:\n  name: 助手{id}\n  yuan: completion-persona\nmemory:\n  enabled: false\n")).unwrap();
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
                let (_, body) = read_request(&mut socket).await;
                let request: Value = serde_json::from_slice(&body).unwrap();
                let failed = request.to_string().contains("拒绝描述");
                let summary = request["model"] == "summary-model"
                    || request.to_string().contains("第三方编辑");
                saved.lock().unwrap().push(request);
                let gate = { served_gate.lock().unwrap().take() };
                if let Some((entered, release)) = gate {
                    entered.send(()).unwrap();
                    release.await.unwrap();
                }
                let frame = if failed {
                    json!({"id":"persona","choices":[{"index":0,"delta":{},"finish_reason":"content_filter"}]})
                } else {
                    json!({"id":"persona","choices":[{"index":0,"delta":{"role":"assistant","content":if summary{"模型生成的专长简介"}else{"实际模型回复"}},"finish_reason":"stop"}]})
                };
                let sse = format!(
                    "data: {frame}\n\ndata: {}\n\ndata: [DONE]\n\n",
                    json!({"id":"persona","choices":[],"usage":{"prompt_tokens":23,"completion_tokens":8}})
                );
                let wire=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",sse.len());
                socket.write_all(wire.as_bytes()).await.unwrap();
            }
        });
        let mut plane_json = json!({"providers":{"persona-stub":{"protocol":"openai-completions","endpoint":format!("http://{stub_addr}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-persona-key"}}},"models":{"chat":{"provider":"persona-stub","model":"chat-model","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"summarize":{"provider":"persona-stub","model":"summary-model","compat":{"contextWindow":200000}}}});
        if tag == "fallback-chat" {
            plane_json["models"]
                .as_object_mut()
                .unwrap()
                .remove("summarize");
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
                product_material_dir: Some(product.clone()),
                model_gateway: Some(gateway),
                credential_service: Some(credential),
                network_plane: if tag == "missing-network" {
                    None
                } else {
                    Some(lingxi_adapters::models::network::NetworkPlane::direct_isolated())
                },
                workspace_root: Some(workspace),
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
            product,
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
    fn events(&self) -> Value {
        self.state.app_events().since(0)
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
    async fn next_run(&self) {
        let created = self
            .request(
                "POST",
                "/lingxi/v1/sessions",
                json!({"sessionId":"persona-consumer","agentId":"agentB","title":"消费描述"}),
            )
            .await;
        assert_eq!(created.0, 201, "{created:?}");
        let started = self
            .request(
                "POST",
                "/lingxi/v1/sessions/persona-consumer/execute",
                json!({"input":"证明下一花名册消费者"}),
            )
            .await;
        assert_eq!(started.0, 200, "{started:?}");
        let id = started.1["runId"].as_str().unwrap();
        let mut completed = false;
        for _ in 0..400 {
            let status = self
                .state
                .storage()
                .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![id.into()])
                .await
                .unwrap();
            if status.as_deref() == Some("completed") {
                completed = true;
                break;
            }
            if status.as_deref() == Some("failed") {
                let reason = self
                    .state
                    .storage()
                    .query_one_text(
                        "SELECT terminal_reason FROM runs WHERE run_id=?1",
                        vec![id.into()],
                    )
                    .await
                    .unwrap();
                panic!("next Run failed: {reason:?}");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(completed);
        let requests = self.requests.lock().unwrap();
        let wire = requests
            .iter()
            .find(|r| r["model"] == "chat-model")
            .unwrap()
            .to_string();
        assert!(
            wire.contains("模型生成的专长简介"),
            "下一实际模型请求没有更新后的花名册"
        );
        assert!(!wire.contains("sourceHash"));
    }
}

async fn persona_write_case(tag: &str, face: &str, file: &str, alias: &str, description: bool) {
    let fx = Fixture::start(tag).await;
    let dir = fx.home.join("agents/agentA");
    let other = fx.home.join("agents/agentB/identity.md");
    std::fs::write(&other, "乙的私有身份").unwrap();
    let config = std::fs::read(dir.join("config.yaml")).unwrap();
    let path = format!("/lingxi/v1/agents/agentA/{face}");
    let result = fx
        .request("PUT", &path, json!({"content":"甲的新人格"}))
        .await;
    assert_eq!(result.0, 200, "{result:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join(file)).unwrap(),
        "甲的新人格"
    );
    let result = fx
        .request(
            "GET",
            &format!("/lingxi/v1/agents/agentA/{alias}"),
            Value::Null,
        )
        .await;
    assert_eq!(result.0, 200);
    assert_eq!(result.1["content"], "甲的新人格");
    assert_eq!(fx.events()["events"][0]["type"], "agent-updated");
    assert_eq!(fx.events()["events"][0]["payload"]["agentId"], "agentA");
    let emitted = fx
        .request("GET", "/lingxi/v1/app-events?since=0", Value::Null)
        .await;
    assert_eq!(emitted.0, 200);
    assert_eq!(emitted.1, fx.events());
    if description {
        assert!(std::fs::read_to_string(dir.join("description.md"))
            .unwrap()
            .contains("模型生成的专长简介"));
        let listed = fx.request("GET", "/lingxi/v1/agents", Value::Null).await;
        assert_eq!(listed.0, 200);
        assert!(listed.1.to_string().contains("模型生成的专长简介"));
        fx.next_run().await;
    }
    let before = std::fs::read(dir.join(file)).unwrap();
    let events = fx.events();
    for body in [json!({}), json!({"content":7}), json!({"content":null})] {
        assert_eq!(fx.request("PUT", &path, body).await.0, 400);
        assert_eq!(std::fs::read(dir.join(file)).unwrap(), before);
        assert_eq!(fx.events(), events);
    }
    for token in [&fx.denied, &fx.chat, "invalid-credential"] {
        let result = fx
            .raw("PUT", &path, token, br#"{"content":"forbidden"}"#, None)
            .await;
        assert!(matches!(result.0, 401 | 403));
        assert_eq!(std::fs::read(dir.join(file)).unwrap(), before);
        assert_eq!(fx.events(), events);
    }
    assert_eq!(
        fx.request(
            "PUT",
            &format!("/lingxi/v1/agents/missing/{face}"),
            json!({"content":"wrong"})
        )
        .await
        .0,
        404
    );
    if description {
        let result = fx
            .request("PUT", &path, json!({"content":"拒绝描述"}))
            .await;
        assert_eq!(result.0, 500, "{result:?}");
        assert_eq!(std::fs::read(dir.join(file)).unwrap(), before);
        assert_eq!(fx.events(), events);
    }
    if description {
        let rows = fx
            .state
            .storage()
            .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
                purpose: Some("auxiliary.summarize".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(rows.iter().any(|r| r.origin == "persona.description"
            && r.outcome == lingxi_kernel::usage::CallOutcome::Succeeded));
        assert!(rows.iter().any(|r| r.origin == "persona.description"
            && r.outcome == lingxi_kernel::usage::CallOutcome::Failed));
        assert!(
            rows.iter().all(|r| r.session_id.is_none()
                && r.run_id.is_none()
                && r.cause_ref.as_deref() == Some("agent:agentA")),
            "辅助描述不能伪造会话或Run归属"
        );
    }
    let empty = fx
        .request(
            "PUT",
            &format!("/lingxi/v1/agents/agentA/{alias}"),
            json!({"content":""}),
        )
        .await;
    assert_eq!(empty.0, 200, "{empty:?}");
    assert_eq!(std::fs::read(dir.join(file)).unwrap(), b"");
    assert_eq!(std::fs::read(other).unwrap(), "乙的私有身份".as_bytes());
    assert_eq!(std::fs::read(dir.join("config.yaml")).unwrap(), config);
    fx.finish().await;
}
#[tokio::test]
async fn original_487080_agents_write_refreshes_actual_description_and_next_roster() {
    persona_write_case("agents", "agents-md", "AGENTS.md", "ishiki", true).await;
}
#[tokio::test]
async fn original_856b28_identity_write_refreshes_actual_description_and_next_roster() {
    persona_write_case("identity", "identity", "identity.md", "identity", true).await;
}
#[tokio::test]
async fn original_7c699f_public_write_and_legacy_alias_publish_only_after_persistence() {
    persona_write_case(
        "public",
        "public-agents-md",
        "AGENTS.public.md",
        "public-ishiki",
        false,
    )
    .await;
}
fn avatar_body(mime: &str, bytes: &[u8]) -> Value {
    use base64::Engine as _;
    json!({"data":format!("data:image/{mime};base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes))})
}
async fn avatars_refusals(fx: &Fixture, path: &str) {
    let events = fx.events();
    let before = fx.raw("GET", path, &fx.token, b"", None).await;
    for bad in [
        json!({}),
        json!({"data":7}),
        json!({"data":"data:image/gif;base64,YQ=="}),
        json!({"data":"data:image/png;base64,%"}),
        json!({"data":"data:image/png;base64,"}),
    ] {
        assert_eq!(fx.request("POST", path, bad).await.0, 400);
        assert_eq!(fx.raw("GET", path, &fx.token, b"", None).await.2, before.2);
        assert_eq!(fx.events(), events);
    }
    for token in [&fx.chat, &fx.denied, "wrong"] {
        assert!(matches!(
            fx.raw(
                "POST",
                path,
                token,
                avatar_body("png", b"forbidden").to_string().as_bytes(),
                None
            )
            .await
            .0,
            401 | 403
        ));
        assert_eq!(fx.raw("GET", path, &fx.token, b"", None).await.2, before.2);
        assert_eq!(fx.events(), events);
    }
}
#[tokio::test]
async fn original_7f0f3f_agent_avatar_upload_cleans_formats_refreshes_next_list_and_event() {
    let fx = Fixture::start("agent-upload").await;
    let dir = fx.home.join("agents/agentA/avatars");
    std::fs::create_dir_all(&dir).unwrap();
    for ext in ["png", "jpg", "jpeg", "webp"] {
        std::fs::write(dir.join(format!("agent.{ext}")), b"old").unwrap();
    }
    let path = "/lingxi/v1/agents/agentA/avatar";
    let r = fx
        .request("POST", path, avatar_body("jpeg", b"new-jpeg"))
        .await;
    assert_eq!(r.0, 200, "{r:?}");
    assert_eq!(r.1["ext"], "jpg");
    assert_eq!(std::fs::read(dir.join("agent.jpg")).unwrap(), b"new-jpeg");
    for ext in ["png", "jpeg", "webp"] {
        assert!(!dir.join(format!("agent.{ext}")).exists());
    }
    assert_eq!(
        fx.raw("GET", path, &fx.token, b"", None).await.2,
        b"new-jpeg"
    );
    let list = fx.request("GET", "/lingxi/v1/agents", Value::Null).await;
    assert!(list.1.to_string().contains("\"hasAvatar\":true"));
    assert_eq!(fx.events()["events"].as_array().unwrap().len(), 1);
    avatars_refusals(&fx, path).await;
    let too_big = vec![b' '; lingxi_service::persona::MAX_AVATAR_BYTES + 1];
    assert_eq!(fx.raw("POST", path, &fx.token, &too_big, None).await.0, 413);
    assert_eq!(std::fs::read(dir.join("agent.jpg")).unwrap(), b"new-jpeg");
    assert_eq!(
        fx.request(
            "POST",
            "/lingxi/v1/agents/missing/avatar",
            avatar_body("png", b"wrong")
        )
        .await
        .0,
        404
    );
    fx.finish().await;
}
#[tokio::test]
async fn original_bb4cd1_agent_avatar_delete_cleans_all_formats_and_is_idempotent() {
    let fx = Fixture::start("agent-delete").await;
    let dir = fx.home.join("agents/agentA/avatars");
    std::fs::create_dir_all(&dir).unwrap();
    for ext in ["png", "jpg", "jpeg", "webp"] {
        std::fs::write(dir.join(format!("agent.{ext}")), b"old").unwrap();
        std::fs::write(
            fx.home.join("agents/agentA").join(format!("avatar.{ext}")),
            b"legacy",
        )
        .unwrap();
    }
    let path = "/lingxi/v1/agents/agentA/avatar";
    assert_eq!(fx.request("DELETE", path, Value::Null).await.0, 200);
    assert_eq!(fx.raw("GET", path, &fx.token, b"", None).await.0, 404);
    assert_eq!(fx.request("DELETE", path, Value::Null).await.0, 200);
    assert_eq!(fx.events()["events"].as_array().unwrap().len(), 2);
    let before = fx.events();
    assert_eq!(fx.raw("DELETE", path, &fx.chat, b"", None).await.0, 403);
    assert_eq!(fx.events(), before);
    for ext in ["png", "jpg", "jpeg", "webp"] {
        assert!(!dir.join(format!("agent.{ext}")).exists());
        assert!(!fx
            .home
            .join("agents/agentA")
            .join(format!("avatar.{ext}"))
            .exists());
    }
    fx.finish().await;
}
#[tokio::test]
async fn original_7570be_role_avatar_upload_selects_explicit_agent_and_user_without_crosstalk() {
    let fx = Fixture::start("roles-upload").await;
    for (role, query) in [("user", ""), ("agent", "?agentId=agentA")] {
        let path = format!("/lingxi/v1/avatar/{role}{query}");
        let result = fx
            .request("POST", &path, avatar_body("webp", role.as_bytes()))
            .await;
        assert_eq!(result.0, 200, "{result:?}");
        assert_eq!(result.1["format"], "webp");
        assert_eq!(
            fx.raw("GET", &path, &fx.token, b"", None).await.2,
            role.as_bytes()
        );
        avatars_refusals(&fx, &path).await;
    }
    assert_eq!(
        fx.raw(
            "GET",
            "/lingxi/v1/avatar/agent?agentId=agentB",
            &fx.token,
            b"",
            None
        )
        .await
        .0,
        404
    );
    for path in [
        "/lingxi/v1/avatar/other",
        "/lingxi/v1/avatar/agent",
        "/lingxi/v1/avatar/agent?agentId=missing",
    ] {
        let result = fx.request("POST", path, avatar_body("png", b"wrong")).await;
        assert!(matches!(result.0, 400 | 404), "{result:?}");
    }
    fx.finish().await;
}
#[tokio::test]
async fn original_4a56f2_role_avatar_delete_restores_defaults_and_preserves_other_target() {
    let fx = Fixture::start("roles-delete").await;
    for path in [
        "/lingxi/v1/avatar/user",
        "/lingxi/v1/avatar/agent?agentId=agentA",
        "/lingxi/v1/avatar/agent?agentId=agentB",
    ] {
        assert_eq!(
            fx.request("POST", path, avatar_body("png", b"custom"))
                .await
                .0,
            200
        );
    }
    for path in [
        "/lingxi/v1/avatar/user",
        "/lingxi/v1/avatar/agent?agentId=agentA",
    ] {
        assert_eq!(fx.request("DELETE", path, Value::Null).await.0, 200);
        assert_eq!(fx.raw("GET", path, &fx.token, b"", None).await.0, 404);
        assert_eq!(fx.request("DELETE", path, Value::Null).await.0, 200);
    }
    assert_eq!(
        fx.raw(
            "GET",
            "/lingxi/v1/avatar/agent?agentId=agentB",
            &fx.token,
            b"",
            None
        )
        .await
        .2,
        b"custom"
    );
    assert_eq!(
        fx.raw(
            "DELETE",
            "/lingxi/v1/avatar/agent?agentId=agentB",
            &fx.chat,
            b"",
            None
        )
        .await
        .0,
        403
    );
    fx.finish().await;
}
#[tokio::test]
async fn original_e4bee8_role_avatar_returns_bytes_etag_304_and_404_under_actual_permissions() {
    let fx = Fixture::start("role-etag").await;
    for path in [
        "/lingxi/v1/avatar/user",
        "/lingxi/v1/avatar/agent?agentId=agentA",
    ] {
        assert_eq!(fx.raw("GET", path, &fx.token, b"", None).await.0, 404);
        assert_eq!(
            fx.request("POST", path, avatar_body("png", b"avatar-one"))
                .await
                .0,
            200
        );
        let (status, headers, body) = fx.raw("GET", path, &fx.chat, b"", None).await;
        assert_eq!(status, 200);
        assert_eq!(body, b"avatar-one");
        assert!(headers.to_lowercase().contains("content-type: image/png"));
        let etag = headers
            .lines()
            .find_map(|line| line.strip_prefix("etag: "))
            .unwrap();
        let next = fx.raw("GET", path, &fx.chat, b"", Some(etag)).await;
        assert_eq!(next.0, 304);
        assert!(next.2.is_empty());
        assert_eq!(
            fx.raw("GET", path, &fx.denied, b"", Some(etag)).await.0,
            403
        );
        assert_eq!(fx.raw("GET", path, "wrong", b"", Some(etag)).await.0, 401);
        assert_eq!(
            fx.request("POST", path, avatar_body("png", b"avatar-two"))
                .await
                .0,
            200
        );
        assert_eq!(fx.raw("GET", path, &fx.chat, b"", Some(etag)).await.0, 200);
    }
    fx.finish().await;
}

#[tokio::test]
async fn persona_writes_refuse_actual_filesystem_failure_and_symlink_without_publication() {
    let fx = Fixture::start("io-link-boundaries").await;
    let dir = fx.home.join("agents/agentA");
    let public = dir.join("AGENTS.public.md");
    std::fs::create_dir(&public).unwrap();
    let before = fx.events();
    assert_eq!(
        fx.request(
            "PUT",
            "/lingxi/v1/agents/agentA/public-agents-md",
            json!({"content":"rejected"})
        )
        .await
        .0,
        500
    );
    assert_eq!(fx.events(), before);
    assert!(public.is_dir());
    assert!(!std::fs::read_dir(&dir).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains(".tmp")));
    std::fs::remove_dir(&public).unwrap();
    #[cfg(unix)]
    {
        let sentinel = fx.home.parent().unwrap().join("unowned-secret");
        std::fs::write(&sentinel, b"other-owner").unwrap();
        std::os::unix::fs::symlink(&sentinel, &public).unwrap();
        assert_eq!(
            fx.request(
                "PUT",
                "/lingxi/v1/agents/agentA/public-ishiki",
                json!({"content":"no escaping"})
            )
            .await
            .0,
            400
        );
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"other-owner");
        assert_eq!(fx.events(), before);
        let outside = fx.home.parent().unwrap().join("unowned-avatars");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("agent.png"), b"private-image").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("avatars")).unwrap();
        assert_eq!(
            fx.raw(
                "GET",
                "/lingxi/v1/avatar/agent?agentId=agentA",
                &fx.token,
                b"",
                None
            )
            .await
            .0,
            400
        );
        assert_eq!(
            fx.request(
                "POST",
                "/lingxi/v1/avatar/agent?agentId=agentA",
                avatar_body("png", b"no write")
            )
            .await
            .0,
            400
        );
        assert_eq!(
            fx.request(
                "DELETE",
                "/lingxi/v1/avatar/agent?agentId=agentA",
                Value::Null
            )
            .await
            .0,
            400
        );
        assert_eq!(
            std::fs::read(outside.join("agent.png")).unwrap(),
            b"private-image"
        );
        assert_eq!(fx.events(), before);
    }
    let large = vec![b'i'; 900_000];
    let upload = fx
        .request("POST", "/lingxi/v1/avatar/user", avatar_body("png", &large))
        .await;
    assert_eq!(
        upload.0, 200,
        "15MB头像独立上限应允许超过普通1MB的请求：{upload:?}"
    );
    assert_eq!(
        fx.raw("GET", "/lingxi/v1/avatar/user", &fx.token, b"", None)
            .await
            .2,
        large
    );
    fx.finish().await;
}

#[tokio::test]
async fn persona_description_uses_explicit_chat_fallback_only_for_unconfigured_summary_slot() {
    let fx = Fixture::start("fallback-chat").await;
    let result = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/identity",
            json!({"content":"新的身份仍应生成简介"}),
        )
        .await;
    assert_eq!(result.0, 200, "{result:?}");
    assert_eq!(result.1["descriptionRefresh"], "updated");
    assert!(
        std::fs::read_to_string(fx.home.join("agents/agentA/description.md"))
            .unwrap()
            .contains("模型生成的专长简介")
    );
    let requests = fx.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"], "chat-model");
    let before = std::fs::read(fx.home.join("agents/agentA/identity.md")).unwrap();
    let events = fx.events();
    let failed = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/identity",
            json!({"content":"拒绝描述"}),
        )
        .await;
    assert_eq!(failed.0, 500, "{failed:?}");
    assert_eq!(
        fx.requests.lock().unwrap().len(),
        2,
        "已发送的供应商失败不能换路重试"
    );
    assert_eq!(
        std::fs::read(fx.home.join("agents/agentA/identity.md")).unwrap(),
        before
    );
    assert_eq!(fx.events(), events);
    fx.finish().await;

    // 未接网络配置时必须明确拒绝，不能暗中切换直连。
    let fx = Fixture::start("missing-network").await;
    let path = fx.home.join("agents/agentA/identity.md");
    std::fs::write(&path, "原始身份").unwrap();
    let events = fx.events();
    let result = fx
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/identity",
            json!({"content":"网络未接时不应提交"}),
        )
        .await;
    assert_eq!(result.0, 500, "{result:?}");
    assert!(result
        .1
        .to_string()
        .contains("network configuration unavailable"));
    assert!(
        fx.requests.lock().unwrap().is_empty(),
        "不能发出直连供应商请求"
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "原始身份");
    assert_eq!(fx.events(), events);
    fx.finish().await;
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn real_immutable_file_failure_rolls_back_persona_pair_and_partial_avatar_cleanup() {
    struct Immutable(PathBuf);
    impl Drop for Immutable {
        fn drop(&mut self) {
            let _ = std::process::Command::new("/usr/bin/chflags")
                .arg("nouchg")
                .arg(&self.0)
                .status();
        }
    }
    fn immutable(path: PathBuf) -> Immutable {
        assert!(std::process::Command::new("/usr/bin/chflags")
            .arg("uchg")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        Immutable(path)
    }
    let fx = Fixture::start("immutable-io").await;
    let path = "/lingxi/v1/agents/agentA/identity";
    assert_eq!(
        fx.request("PUT", path, json!({"content":"修改前身份"}))
            .await
            .0,
        200
    );
    let dir = fx.home.join("agents/agentA");
    let original = std::fs::read(dir.join("identity.md")).unwrap();
    let description = std::fs::read(dir.join("description.md")).unwrap();
    let events = fx.events();
    let guard = immutable(dir.join("description.md"));
    let result = fx
        .request("PUT", path, json!({"content":"提交简介时磁盘实际拒绝"}))
        .await;
    assert_eq!(result.0, 500, "{result:?}");
    assert_eq!(std::fs::read(dir.join("identity.md")).unwrap(), original);
    assert_eq!(
        std::fs::read(dir.join("description.md")).unwrap(),
        description
    );
    assert_eq!(fx.events(), events);
    assert!(!std::fs::read_dir(&dir).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains(".tmp")));
    drop(guard);
    let avatars = dir.join("avatars");
    std::fs::create_dir_all(&avatars).unwrap();
    for ext in ["png", "jpg", "jpeg", "webp"] {
        std::fs::write(avatars.join(format!("agent.{ext}")), ext.as_bytes()).unwrap();
    }
    let guard = immutable(avatars.join("agent.webp"));
    let result = fx
        .request("DELETE", "/lingxi/v1/agents/agentA/avatar", Value::Null)
        .await;
    assert_eq!(result.0, 500, "{result:?}");
    for ext in ["png", "jpg", "jpeg", "webp"] {
        assert_eq!(
            std::fs::read(avatars.join(format!("agent.{ext}"))).unwrap(),
            ext.as_bytes()
        );
    }
    assert_eq!(fx.events(), events);
    drop(guard);
    fx.finish().await;
}

#[tokio::test]
async fn description_all_material_races_refuse_stale_commit_and_preserve_paid_usage() {
    for case in [
        "sibling-agents",
        "sibling-identity",
        "own-identity",
        "config-name",
        "config-yuan",
        "config-locale",
        "preferences-name",
        "preferences-locale",
        "fallback-template",
        "fallback-example",
        "new-preferred-template",
        "new-own-persona",
        "empty-target-template",
    ] {
        let fx = Fixture::start(case).await;
        let product = fx.product.clone();
        let dir = fx.home.join("agents/agentA");
        std::fs::write(
            dir.join("identity.md"),
            "原始身份 {{agentName}} {{userName}}",
        )
        .unwrap();
        std::fs::write(dir.join("AGENTS.md"), "原始说明 {{agentName}} {{userName}}").unwrap();
        let prefs = fx.home.join("user/preferences.json");
        std::fs::create_dir_all(prefs.parent().unwrap()).unwrap();
        std::fs::write(&prefs, r#"{"userName":"原用户名","locale":"zh"}"#).unwrap();
        let config = dir.join("config.yaml");
        let en_template = product.join("agents-templates/en/completion-persona.md");
        let revised_template = product.join("agents-templates/revised-persona.md");
        let example = product.join("agents.example.md");
        let product_paths = [
            en_template.clone(),
            revised_template.clone(),
            example.clone(),
            product.join("agents-templates/completion-persona.md"),
            product.join("identity-templates/completion-persona.md"),
        ];
        let product_before: Vec<_> = product_paths
            .iter()
            .map(|path| (path.clone(), std::fs::read(path).ok()))
            .collect();
        let (face, target) = if case == "sibling-identity" {
            ("agents-md", "AGENTS.md")
        } else {
            ("identity", "identity.md")
        };
        let submission = if case == "empty-target-template" {
            ""
        } else {
            "此次拟提交人格 {{agentName}} {{userName}}"
        };
        let (changed, after): (PathBuf, Vec<u8>) = match case {
            "sibling-agents" => (dir.join("AGENTS.md"), "用户后改说明".as_bytes().to_vec()),
            "sibling-identity" => (dir.join("identity.md"), "用户后改身份".as_bytes().to_vec()),
            "own-identity" => (
                dir.join("identity.md"),
                "用户后改当前身份".as_bytes().to_vec(),
            ),
            "config-name" => (
                config.clone(),
                std::fs::read_to_string(&config)
                    .unwrap()
                    .replace("name: 助手agentA", "name: 用户后改名称")
                    .into_bytes(),
            ),
            "config-yuan" => {
                std::fs::write(&revised_template, "重新选择的人格模板").unwrap();
                std::fs::remove_file(dir.join("AGENTS.md")).unwrap();
                (
                    config.clone(),
                    std::fs::read_to_string(&config)
                        .unwrap()
                        .replace("yuan: completion-persona", "yuan: revised-persona")
                        .into_bytes(),
                )
            }
            "config-locale" => {
                std::fs::create_dir_all(en_template.parent().unwrap()).unwrap();
                std::fs::write(&en_template, "实际英文模板").unwrap();
                std::fs::remove_file(dir.join("AGENTS.md")).unwrap();
                (
                    config.clone(),
                    std::fs::read_to_string(&config)
                        .unwrap()
                        .replace("locale: zh", "locale: en")
                        .into_bytes(),
                )
            }
            "preferences-name" => (
                prefs.clone(),
                br#"{"userName":"post-user-name","locale":"zh"}"#.to_vec(),
            ),
            "preferences-locale" => {
                std::fs::create_dir_all(en_template.parent().unwrap()).unwrap();
                std::fs::write(&en_template, "实际英文模板").unwrap();
                std::fs::remove_file(dir.join("AGENTS.md")).unwrap();
                let original = std::fs::read_to_string(&config).unwrap();
                std::fs::write(&config, original.replace("locale: zh\n", "")).unwrap();
                (
                    prefs.clone(),
                    br#"{"userName":"original-name","locale":"en"}"#.to_vec(),
                )
            }
            "fallback-template" => {
                std::fs::remove_file(dir.join("AGENTS.md")).unwrap();
                (
                    product.join("agents-templates/completion-persona.md"),
                    "用户后改生效模板".as_bytes().to_vec(),
                )
            }
            "fallback-example" => {
                std::fs::remove_file(dir.join("AGENTS.md")).unwrap();
                std::fs::remove_file(product.join("agents-templates/completion-persona.md"))
                    .unwrap();
                std::fs::write(&example, "原始最终回落样例").unwrap();
                (example.clone(), "用户后改最终回落样例".as_bytes().to_vec())
            }
            "new-preferred-template" => {
                std::fs::remove_file(dir.join("AGENTS.md")).unwrap();
                std::fs::create_dir_all(en_template.parent().unwrap()).unwrap();
                match std::fs::remove_file(&en_template) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => panic!("{e}"),
                };
                let original = std::fs::read_to_string(&config).unwrap();
                std::fs::write(&config, original.replace("locale: zh", "locale: en")).unwrap();
                (
                    en_template.clone(),
                    "新出现的优先语言模板".as_bytes().to_vec(),
                )
            }
            "new-own-persona" => {
                std::fs::remove_file(dir.join("AGENTS.md")).unwrap();
                (
                    dir.join("AGENTS.md"),
                    "用户新建的优先人格正文".as_bytes().to_vec(),
                )
            }
            "empty-target-template" => (
                product.join("identity-templates/completion-persona.md"),
                "空正文实际采用的新模板".as_bytes().to_vec(),
            ),
            _ => unreachable!(),
        };
        let original_target = std::fs::read(dir.join(target)).unwrap();
        let description = dir.join("description.md");
        std::fs::write(
            &description,
            "<!-- sourceHash: original -->\n原始已提交简介",
        )
        .unwrap();
        let original_description = std::fs::read(&description).unwrap();
        let events = fx.events();
        let (received, release) = fx.pause_next_provider();
        let path = format!("/lingxi/v1/agents/agentA/{face}");
        let response = {
            let pending = fx.request("PUT", &path, json!({"content":submission}));
            tokio::pin!(pending);
            tokio::select! { response=&mut pending=>panic!("{case}: provider未收到请求即返回 {response:?}"), arrived=tokio::time::timeout(Duration::from_secs(10),received)=>{arrived.unwrap().unwrap();} }
            let sent = fx.requests.lock().unwrap().last().unwrap().to_string();
            let expected_old = match case {
                "sibling-agents" | "own-identity" => "原始说明",
                "sibling-identity" => "原始身份",
                "config-name" => "助手agentA",
                "preferences-name" => "原用户名",
                "fallback-example" => "原始最终回落样例",
                "empty-target-template" => "默认身份",
                _ => "默认说明",
            };
            assert!(
                sent.contains(expected_old),
                "{case}: 必须在线体内观察旧材料 {expected_old}"
            );
            std::fs::write(&changed, &after).unwrap();
            release.send(()).unwrap();
            pending.await
        };
        println!("description material race {case}: status {}", response.0);
        assert_eq!(
            response.0, 409,
            "{case}: 不能提交依据旧材料生成的简介，{response:?}"
        );
        if changed != dir.join(target) {
            assert_eq!(
                std::fs::read(dir.join(target)).unwrap(),
                original_target,
                "{case}"
            );
        }
        assert_eq!(
            std::fs::read(&changed).unwrap(),
            after,
            "{case}: 不覆盖外部修改"
        );
        assert_eq!(
            std::fs::read(&description).unwrap(),
            original_description,
            "{case}: 不提交旧来源简介"
        );
        assert_eq!(fx.events(), events, "{case}");
        let paid = fx
            .state
            .storage()
            .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
                purpose: Some("auxiliary.summarize".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(paid.len(), 1, "{case}: 已发生调用不得丢账");
        assert_eq!(
            paid[0].outcome,
            lingxi_kernel::usage::CallOutcome::Succeeded
        );
        assert_eq!(paid[0].transport_attempts, Some(1));
        assert!(paid[0].usage.is_some());
        let normal = fx
            .request("PUT", &path, json!({"content":submission}))
            .await;
        assert_eq!(normal.0, 200, "{case}: 稳定材料正常对照 {normal:?}");
        assert_eq!(normal.1["descriptionRefresh"], "updated");
        assert_eq!(
            std::fs::read(dir.join(target)).unwrap(),
            submission.as_bytes()
        );
        assert_ne!(std::fs::read(&description).unwrap(), original_description);
        assert_eq!(
            fx.events()["latestSequence"].as_u64(),
            events["latestSequence"].as_u64().map(|n| n + 1)
        );
        for (path, old) in product_before {
            match old {
                Some(bytes) => std::fs::write(path, bytes).unwrap(),
                None => match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => panic!("{e}"),
                },
            }
        }
        fx.finish().await;
    }
}
