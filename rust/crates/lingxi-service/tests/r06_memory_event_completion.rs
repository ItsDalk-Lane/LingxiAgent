//! 原三叶事件补全：真实 HTTP、生产存储、下一实际运行与事件回放。
use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, NormalizedMessage};
use lingxi_service::context_compiler::{
    ContextCompilerService, FileContextMaterialSource, HostFacts,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    provider: Arc<RecordingProvider>,
    token: String,
    readonly_token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn start_server(tag: &str) -> TestServer {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06-original-mem-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06-original-mem-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    // 两个助手拥有独立的合成数据根。
    for agent_id in ["lingxi", "other"] {
        let agent_dir = home.join("agents").join(agent_id);
        std::fs::create_dir_all(agent_dir.join("memory")).expect("mkdir agent");
        std::fs::write(
            agent_dir.join("config.yaml"),
            "locale: zh\nagent:\n  name: 助手\nmemory:\n  enabled: true\n",
        )
        .expect("write config");
    }
    let home = home.canonicalize().expect("canonical home");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let product = home.join("product");
    for (name, content) in [
        ("yuan/lingxi.md", "你是{{agentName}}，用户为{{userName}}。"),
        ("identity-templates/lingxi.md", "身份"),
        ("agents-templates/lingxi.md", "行为"),
    ] {
        let file = product.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }
    let compiler = Arc::new(ContextCompilerService::new(Arc::new(
        FileContextMaterialSource::from_home(
            &home,
            product.clone(),
            HostFacts {
                platform: "darwin".into(),
                os_type: "Darwin".into(),
                os_release: "fixture".into(),
                shell_label: "zsh".into(),
                workspace: workspace.to_string_lossy().into(),
            },
        ),
    )));
    let provider = Arc::new(RecordingProvider::default());
    let deps = ServiceDeps {
        context_compiler: Some(compiler),
        turn_provider: Some(provider.clone()),
        workspace_root: Some(workspace.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let readonly_token = state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.read"],
            None,
        )
        .expect("只读凭证")
        .secret;
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let server_state = state.clone();
    let handle = tokio::spawn(async move {
        run(
            server_state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        addr,
        home,
        workspace,
        state,
        provider,
        token,
        readonly_token,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.request_token(method, path, body, &self.token).await
    }

    async fn request_token(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        token: &str,
    ) -> (u16, Value) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            token,
            payload.len()
        );
        let mut stream = TcpStream::connect(self.addr).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut buf)).await;
        let raw = String::from_utf8_lossy(&buf).to_string();
        let status = raw
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let body = raw
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_string())
            .unwrap_or_default();
        let value = serde_json::from_str(&body).unwrap_or(Value::Null);
        (status, value)
    }

    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

// 仅替代外部模型应答；权限、存储、提示词编译和运行执行均使用生产入口。
#[derive(Default)]
struct RecordingProvider {
    prompts: Mutex<Vec<String>>,
}
impl TurnProviderPort for RecordingProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "fixture".into(),
            model: "fixture".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a lingxi_protocol::ModelCallId,
        input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.prompts
            .lock()
            .unwrap()
            .push(input.system_prompt.clone().expect("真实运行必须发送提示词"));
        let ctx = ctx.clone();
        Box::pin(async move {
            ProviderTurnResult::of_ctx(
                &ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: "完成".into(),
                        }],
                        model_call_id: None,
                    },
                },
            )
        })
    }
}

fn principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".into(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.into()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".into()],
    }
}
async fn next_run_prompt(server: &TestServer) -> String {
    server
        .state
        .sessions()
        .execute_for(
            server.state.storage().as_ref(),
            server.state.events(),
            server.state.runs(),
            &principal(),
            "sess_local_alpha",
            "读取最新材料",
            1_790_409_600_000,
        )
        .await
        .expect("真实运行执行");
    server
        .provider
        .prompts
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .clone()
}
fn snapshot(path: &std::path::Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(root: &std::path::Path, path: &std::path::Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        if !path.exists() {
            return;
        }
        for item in std::fs::read_dir(path).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                collect(root, &path, out);
            } else {
                out.push((
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(&path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    collect(path, path, &mut out);
    out.sort();
    out
}
async fn events(server: &TestServer) -> Value {
    let (status, result) = server
        .request("GET", "/lingxi/v1/app-events?since=0", None)
        .await;
    assert_eq!(status, 200);
    result
}
async fn assert_event(server: &TestServer, index: usize, kind: &str) {
    let result = events(server).await;
    assert_eq!(result["events"].as_array().unwrap().len(), index + 1);
    assert_eq!(result["events"][index]["type"], kind);
    assert_eq!(
        result["events"][index]["payload"],
        json!({"agentId":"lingxi"})
    );
}
async fn refused_preserves(
    server: &TestServer,
    method: &str,
    path: &str,
    body: Value,
    status: u16,
    readonly: bool,
) {
    let before = snapshot(&server.home.join("agents"));
    let before_events = events(server).await;
    let (actual, error) = server
        .request_token(
            method,
            path,
            Some(body),
            if readonly {
                &server.readonly_token
            } else {
                &server.token
            },
        )
        .await;
    assert_eq!(actual, status, "{error}");
    if readonly {
        assert!(error["code"].is_string(), "{error}");
        assert!(error["message"].is_string(), "{error}");
    } else {
        assert!(error["error"].is_string(), "{error}");
    }
    assert_eq!(snapshot(&server.home.join("agents")), before);
    assert_eq!(events(server).await, before_events);
}

#[tokio::test]
async fn direct_tenets_success_duplicate_events_and_refusals_are_agent_scoped() {
    let server = start_server("direct-events").await;
    let other = snapshot(&server.home.join("agents/other"));
    let store = server.state.memory().tenets_store("lingxi").unwrap();
    let (status, added) = server
        .request(
            "POST",
            "/lingxi/v1/agents/lingxi/tenets",
            Some(json!({"content":"原则事件甲","priority":"critical"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(added["duplicate"], false);
    assert_eq!(added["tenet"]["status"], "active");
    assert_eq!(added["tenet"]["priority"], "critical");
    assert_eq!(store.active()[0].content, "原则事件甲");
    assert_event(&server, 0, "tenets-changed").await;
    let before = std::fs::read(store.file_path()).unwrap();
    let (status, duplicate) = server
        .request(
            "POST",
            "/lingxi/v1/agents/lingxi/tenets",
            Some(json!({"content":"原则事件甲"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["tenet"]["id"], added["tenet"]["id"]);
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    assert_event(&server, 1, "tenets-changed").await;
    for bad in [json!({"content":" "}), json!({"content":123})] {
        refused_preserves(
            &server,
            "POST",
            "/lingxi/v1/agents/lingxi/tenets",
            bad,
            400,
            false,
        )
        .await;
    }
    refused_preserves(
        &server,
        "POST",
        "/lingxi/v1/agents/ghost/tenets",
        json!({"content":"秘密"}),
        404,
        false,
    )
    .await;
    refused_preserves(
        &server,
        "POST",
        "/lingxi/v1/agents/lingxi/tenets",
        json!({"content":"禁止写入"}),
        403,
        true,
    )
    .await;
    for i in 1..lingxi_kernel::memory::MAX_ACTIVE_USER_TENETS {
        store
            .add_direct(&format!("容量原则{i}"), None, format!("capacity-{i}"))
            .unwrap();
    }
    let before = snapshot(&server.home.join("agents"));
    let (status, error) = server
        .request(
            "POST",
            "/lingxi/v1/agents/lingxi/tenets",
            Some(json!({"content":"超出容量"})),
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(error["code"], "TENET_LIMIT_REACHED");
    assert_eq!(snapshot(&server.home.join("agents")), before);
    assert_eq!(events(&server).await["events"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot(&server.home.join("agents/other")), other);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn pinned_replacement_event_and_next_actual_run_use_latest_principles() {
    let server = start_server("pinned-events").await;
    let store = server.state.memory().tenets_store("lingxi").unwrap();
    let (old, _, _) = store
        .add_proposal("旧模型原则", None, None, "old-model".into())
        .unwrap();
    store.decide(&old.id, true).unwrap();
    let (pending, _, _) = store
        .add_proposal("保留待审", None, None, "pending".into())
        .unwrap();
    let (rejected, _, _) = store
        .add_proposal("保留拒绝", None, None, "rejected".into())
        .unwrap();
    store.decide(&rejected.id, false).unwrap();
    let other = snapshot(&server.home.join("agents/other"));
    assert!(next_run_prompt(&server).await.contains("旧模型原则"));
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/lingxi/pinned",
            Some(json!({"pins":["新置顶原则", "新置顶原则", "  "]})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["pins"], json!(["新置顶原则"]));
    assert_eq!(store.active().len(), 1);
    assert_eq!(store.active()[0].source.as_str(), "user_direct");
    assert!(!store.list().tenets.iter().any(|t| t.id == old.id));
    assert!(store
        .list()
        .tenets
        .iter()
        .any(|t| t.id == pending.id && t.status.as_str() == "pending"));
    assert!(store
        .list()
        .tenets
        .iter()
        .any(|t| t.id == rejected.id && t.status.as_str() == "rejected"));
    assert_event(&server, 0, "agent-updated").await;
    let prompt = next_run_prompt(&server).await;
    assert!(prompt.contains("新置顶原则"));
    assert!(!prompt.contains("旧模型原则"));
    assert!(!prompt.contains("保留待审"));
    assert!(!prompt.contains("保留拒绝"));
    refused_preserves(
        &server,
        "PUT",
        "/lingxi/v1/agents/lingxi/pinned",
        json!({"pins":"bad"}),
        400,
        false,
    )
    .await;
    refused_preserves(
        &server,
        "PUT",
        "/lingxi/v1/agents/lingxi/pinned",
        json!({"pins":["禁止写入"]}),
        403,
        true,
    )
    .await;
    refused_preserves(
        &server,
        "PUT",
        "/lingxi/v1/agents/ghost/pinned",
        json!({"pins":["秘密"]}),
        404,
        false,
    )
    .await;
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/lingxi/pinned",
            Some(json!({"pins":[]})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["pins"], json!([]));
    assert!(store.active().is_empty());
    assert_eq!(store.list().tenets.len(), 2);
    assert_event(&server, 1, "agent-updated").await;
    assert!(!next_run_prompt(&server).await.contains("新置顶原则"));
    assert_eq!(snapshot(&server.home.join("agents/other")), other);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn experience_sync_event_reads_latest_documents_and_next_run_reloads_materials() {
    let server = start_server("experience-events").await;
    server
        .state
        .memory()
        .patch_flags(
            "lingxi",
            lingxi_adapters::storage::agent_memory::AgentConfigFlags {
                experience_enabled: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
    let store = server.state.memory().experience_store("lingxi").unwrap();
    store.record_entry("旧分类", "旧经验").unwrap();
    let other = snapshot(&server.home.join("agents/other"));
    let old_prompt = next_run_prompt(&server).await;
    std::fs::write(
        server.home.join("agents/lingxi/config.yaml"),
        "locale: zh\nagent:\n  name: 经验同步后的新助手\nmemory:\n  enabled: true\nexperience:\n  enabled: true\n",
    ).unwrap();
    std::fs::write(
        server.home.join("agents/lingxi/identity.md"),
        "经验保存后的新身份",
    )
    .unwrap();
    let (status, result) = server.request("PUT", "/lingxi/v1/agents/lingxi/experience", Some(json!({
        "content":"#\t用户认可的做法\n1. 第一类\n# 用户不认可的做法\n1. 第二类\n# 用户认可的做法\n2. 合并重复分类"
    }))).await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    assert_event(&server, 0, "agent-updated").await;
    let (status, merged) = server
        .request("GET", "/lingxi/v1/agents/lingxi/experience", None)
        .await;
    assert_eq!(status, 200);
    let merged = merged["content"].as_str().unwrap();
    for expected in ["1. 第一类", "1. 第二类", "2. 合并重复分类"] {
        assert!(merged.contains(expected));
    }
    assert!(!merged.contains("旧经验"));
    assert!(store.index_text().contains("用户认可的做法"));
    assert!(store.index_text().contains("用户不认可的做法"));
    assert!(!store.index_text().contains("旧分类"));
    let (status, search) = server
        .request(
            "GET",
            "/lingxi/v1/agents/lingxi/experience/search?q=%E7%AC%AC%E4%B8%80%E7%B1%BB",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert!(search.to_string().contains("第一类"));
    let prompt = next_run_prompt(&server).await;
    assert_ne!(prompt, old_prompt);
    assert!(prompt.contains("经验保存后的新身份"));
    assert!(prompt.contains("经验同步后的新助手"));
    assert!(!old_prompt.contains("经验同步后的新助手"));
    // 经验正文由读取接口现读；现役提示词也不会直接拼入分类正文。
    assert!(!prompt.contains("1. 第一类"));
    for bad in [
        json!({"content":123}),
        json!({"content":"# ../invalid\n破坏内容"}),
    ] {
        refused_preserves(
            &server,
            "PUT",
            "/lingxi/v1/agents/lingxi/experience",
            bad,
            400,
            false,
        )
        .await;
    }
    refused_preserves(
        &server,
        "PUT",
        "/lingxi/v1/agents/lingxi/experience",
        json!({"content":""}),
        403,
        true,
    )
    .await;
    refused_preserves(
        &server,
        "PUT",
        "/lingxi/v1/agents/other/experience",
        json!({"content":"# 分类\n禁止"}),
        403,
        false,
    )
    .await;
    refused_preserves(
        &server,
        "PUT",
        "/lingxi/v1/agents/ghost/experience",
        json!({"content":""}),
        404,
        false,
    )
    .await;
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/lingxi/experience",
            Some(json!({"content":""})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    assert_event(&server, 1, "agent-updated").await;
    assert!(store.list_documents().is_empty());
    assert_eq!(store.read_merged().0, "");
    assert!(!store.index_text().contains("用户认可的做法"));
    let (status, merged) = server
        .request("GET", "/lingxi/v1/agents/lingxi/experience", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(merged["content"], "");
    assert_eq!(snapshot(&server.home.join("agents/other")), other);
    server.stop_and_clean().await;
}
