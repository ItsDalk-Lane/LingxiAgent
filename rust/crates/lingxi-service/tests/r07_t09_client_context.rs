//! R07-T09：CLI/mobile 共享只读上下文面（/lingxi/v1/chat/models、
//! /lingxi/v1/mobile/bootstrap）的真实服务集成测试。
//!
//! 真实服务、真实 SQLite、真实权限闸；仅外部模型供应商替换为 loopback
//! 替身（本组不断言真实模型调用，模型面注入仅为目录/选择投影）。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

struct Fixture {
    addr: SocketAddr,
    home: PathBuf,
    owner: String,
    chat: String,
    denied: String,
    foreign_chat: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await.expect("write body");
    }
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let split = text.find("\r\n\r\n").expect("HTTP head/body split");
    let status = text[..split]
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .expect("HTTP status");
    (status, text[split + 4..].to_string())
}

impl Fixture {
    /// with_model_plane=false 时无模型面：目录必须真实为空，状态
    /// unconfigured，代次与未激活口径一致（"0"），不得编造目录。
    async fn start(tag: &str, with_model_plane: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "r07t09-client-context-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        for (id, name) in [("agentA", "助手甲"), ("agentB", "助手乙")] {
            let dir = home.join("agents").join(id);
            std::fs::create_dir_all(dir.join("memory")).unwrap();
            std::fs::write(
                dir.join("config.yaml"),
                format!("locale: zh-CN\nagent:\n  name: {name}\n  yuan: lingxi\nmemory:\n  enabled: true\n"),
            )
            .unwrap();
        }
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user").join("preferences.json"),
            json!({"locale":"zh-CN","userName":"测试用户","current_agent_id":"agentA","appearance":{"theme":"dark"},"editor":{"fontSize":14},"chat":{"layout":"bubbles"}}).to_string(),
        )
        .unwrap();
        let config = ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        };
        let layout = prepare_layout(&home).unwrap();
        let deps = if with_model_plane {
            let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(
                &json!({"providers":{"stub":{"protocol":"openai-completions","endpoint":"http://127.0.0.1:9/v1","auth":{"kind":"apiKey","apiKey":"synthetic-t09-key"}}},"models":{"chat":{"provider":"stub","model":"chat-model","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"memory":{"provider":"stub","model":"memory-model","compat":{"contextWindow":200000}}}}).to_string(),
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
            ServiceDeps {
                model_gateway: Some(Arc::new(
                    lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
                )),
                credential_service: Some(credential),
                network_plane: Some(
                    lingxi_adapters::models::network::NetworkPlane::direct_isolated(),
                ),
                ..ServiceDeps::default()
            }
        } else {
            ServiceDeps::default()
        };
        let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
            .await
            .unwrap();
        let owner = state.auth().local_token();
        let chat = state
            .auth()
            .issue_device_credential("user_local", &["chat"], None)
            .unwrap()
            .secret;
        let denied = state
            .auth()
            .issue_device_credential("user_local", &[], None)
            .unwrap()
            .secret;
        let foreign_chat = state
            .auth()
            .issue_device_credential("user_other", &["chat"], None)
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
        Fixture {
            addr,
            home,
            owner,
            chat,
            denied,
            foreign_chat,
            stop,
            task,
        }
    }

    async fn stop(self) {
        self.stop.send(()).expect("stop signal");
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .expect("shutdown timeout")
            .expect("join");
        let _ = std::fs::remove_dir_all(&self.home);
    }

    fn bearer(token: &str) -> String {
        format!("Bearer {token}")
    }
}

/// 递归断言投影里不存在供应商端点/密钥/自由元数据键。
fn assert_no_secret_keys(value: &Value, path: &str) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                assert!(
                    !matches!(
                        key.as_str(),
                        "apiKey" | "api_key" | "endpoint" | "base_url" | "baseUrl" | "headers"
                    ),
                    "secret/provider key {key} leaked at {path}"
                );
                assert_no_secret_keys(child, &format!("{path}.{key}"));
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                assert_no_secret_keys(child, &format!("{path}[{index}]"));
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn bootstrap_and_models_project_real_context_without_secrets() {
    let fixture = Fixture::start("owner", true).await;
    let (status, body) = http(
        fixture.addr,
        "GET",
        "/lingxi/v1/mobile/bootstrap",
        &[("Authorization", &Fixture::bearer(&fixture.owner))],
        None,
    )
    .await;
    assert_eq!(status, 200, "bootstrap: {body}");
    let bootstrap: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(bootstrap["locale"], "zh-CN");
    assert_eq!(bootstrap["userName"], "测试用户");
    assert_eq!(bootstrap["currentAgentId"], "agentA");
    assert_eq!(bootstrap["agentName"], "助手甲");
    assert_eq!(bootstrap["permissionMode"], "auto");
    assert_eq!(bootstrap["modelState"], "ready");
    assert_eq!(bootstrap["activeModel"], json!({"id":"chat-model","provider":"stub"}));
    assert_eq!(bootstrap["currentModel"], json!({"id":"chat-model","provider":"stub"}));
    let generation = bootstrap["modelGeneration"].as_str().expect("generation string");
    assert!(generation.parse::<u64>().is_ok(), "decimal generation: {generation}");
    let agents = bootstrap["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 2);
    assert!(agents.iter().any(|a| a["id"] == "agentA" && a["isCurrent"] == true));
    let models = bootstrap["models"].as_array().unwrap();
    assert!(models.iter().any(|m| m["id"] == "chat-model" && m["isCurrent"] == true));
    assert_no_secret_keys(&bootstrap, "$");

    let (status, body) = http(
        fixture.addr,
        "GET",
        "/lingxi/v1/chat/models",
        &[("Authorization", &Fixture::bearer(&fixture.chat))],
        None,
    )
    .await;
    assert_eq!(status, 200, "chat/models: {body}");
    let context: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(context["modelState"], "ready");
    assert_eq!(context["activeModel"], json!({"id":"chat-model","provider":"stub"}));
    let models = context["models"].as_array().unwrap();
    let chat_model = models.iter().find(|m| m["id"] == "chat-model").unwrap();
    assert_eq!(chat_model["isCurrent"], true);
    assert!(chat_model["thinkingLevels"].as_array().unwrap().iter().any(|l| l == "medium"));
    assert!(chat_model["input"].as_array().unwrap().iter().any(|i| i == "text"));
    assert_no_secret_keys(&context, "$");
    fixture.stop().await;
}

#[tokio::test]
async fn unconfigured_plane_reports_empty_catalog_honestly() {
    let fixture = Fixture::start("unconfigured", false).await;
    let (status, body) = http(
        fixture.addr,
        "GET",
        "/lingxi/v1/chat/models",
        &[("Authorization", &Fixture::bearer(&fixture.chat))],
        None,
    )
    .await;
    assert_eq!(status, 200, "chat/models: {body}");
    let context: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(context["models"], json!([]));
    assert_eq!(context["modelState"], "unconfigured");
    assert_eq!(context["activeModel"], Value::Null);
    assert_eq!(context["modelGeneration"], "0");

    let (status, body) = http(
        fixture.addr,
        "GET",
        "/lingxi/v1/mobile/bootstrap",
        &[("Authorization", &Fixture::bearer(&fixture.chat))],
        None,
    )
    .await;
    assert_eq!(status, 200, "bootstrap: {body}");
    let bootstrap: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(bootstrap["modelState"], "unconfigured");
    assert_eq!(bootstrap["models"], json!([]));
    fixture.stop().await;
}

#[tokio::test]
async fn credentials_and_scope_are_enforced() {
    let fixture = Fixture::start("scope", true).await;
    for path in ["/lingxi/v1/chat/models", "/lingxi/v1/mobile/bootstrap"] {
        let (status, _) = http(fixture.addr, "GET", path, &[], None).await;
        assert_eq!(status, 401, "no credential must be 401 for {path}");
        let (status, _) = http(
            fixture.addr,
            "GET",
            path,
            &[("Authorization", &Fixture::bearer(&fixture.denied))],
            None,
        )
        .await;
        assert_eq!(status, 403, "scope-less credential must be 403 for {path}");
        let (status, _) = http(
            fixture.addr,
            "POST",
            path,
            &[("Authorization", &Fixture::bearer(&fixture.owner))],
            Some("{}"),
        )
        .await;
        assert!(matches!(status, 401 | 403 | 405), "POST must not be served for {path}: {status}");
    }
    fixture.stop().await;
}

#[tokio::test]
async fn explicit_agent_and_session_targets_are_validated() {
    let fixture = Fixture::start("targets", true).await;
    // 显式存在的助手。
    let (status, body) = http(
        fixture.addr,
        "GET",
        "/lingxi/v1/mobile/bootstrap?agentId=agentB",
        &[("Authorization", &Fixture::bearer(&fixture.chat))],
        None,
    )
    .await;
    assert_eq!(status, 200, "explicit agent bootstrap: {body}");
    let bootstrap: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(bootstrap["currentAgentId"], "agentB");
    assert_eq!(bootstrap["agentName"], "助手乙");
    // 不存在的助手：拒绝，不回退到焦点助手。
    let (status, _) = http(
        fixture.addr,
        "GET",
        "/lingxi/v1/mobile/bootstrap?agentId=missing",
        &[("Authorization", &Fixture::bearer(&fixture.chat))],
        None,
    )
    .await;
    assert_eq!(status, 404);

    // owner 创建真实会话（agentA），随后 chat 凭证按会话取上下文。
    let (status, body) = http(
        fixture.addr,
        "POST",
        "/lingxi/v1/sessions/new",
        &[("Authorization", &Fixture::bearer(&fixture.owner))],
        Some(&json!({"agentId":"agentA","title":"t09 目标会话"}).to_string()),
    )
    .await;
    assert_eq!(status, 200, "session create: {body}");
    let created: Value = serde_json::from_str(&body).unwrap();
    let session_id = created["sessionId"].as_str().expect("sessionId");
    let (status, body) = http(
        fixture.addr,
        "GET",
        &format!("/lingxi/v1/chat/models?sessionId={session_id}"),
        &[("Authorization", &Fixture::bearer(&fixture.chat))],
        None,
    )
    .await;
    assert_eq!(status, 200, "session-targeted models: {body}");
    // 会话归属与会话外显式 agentId 矛盾：拒绝，不猜目标。
    let (status, _) = http(
        fixture.addr,
        "GET",
        &format!("/lingxi/v1/chat/models?sessionId={session_id}&agentId=agentB"),
        &[("Authorization", &Fixture::bearer(&fixture.chat))],
        None,
    )
    .await;
    assert_eq!(status, 409);
    // 他人的 chat 凭证读 owner 的会话：拒绝（不区分 403/404 的隐匿口径，两者都合法拒绝）。
    let (status, _) = http(
        fixture.addr,
        "GET",
        &format!("/lingxi/v1/mobile/bootstrap?sessionId={session_id}"),
        &[("Authorization", &Fixture::bearer(&fixture.foreign_chat))],
        None,
    )
    .await;
    assert!(matches!(status, 403 | 404), "cross-owner must be refused: {status}");
    // 不存在的会话：404。
    let (status, _) = http(
        fixture.addr,
        "GET",
        "/lingxi/v1/chat/models?sessionId=sess-missing",
        &[("Authorization", &Fixture::bearer(&fixture.chat))],
        None,
    )
    .await;
    assert_eq!(status, 404);
    fixture.stop().await;
}
