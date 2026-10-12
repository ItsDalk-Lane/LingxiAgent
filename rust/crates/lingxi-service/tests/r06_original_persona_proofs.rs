//! 原F-D06读取面的真实认证HTTP证明；产品缺失的广播/助手管理不由本检查伪报完成。
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use lingxi_service::{
    HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps, ServiceError, ServiceState,
    prepare_layout, run,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    denied_token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn start_server(tag: &str) -> TestServer {
    // 主代理为本套件指定合成模板根；不写仓库或用户的产品材料。
    let product = PathBuf::from(
        std::env::var("LINGXI_PRODUCT_DIR").expect("本证明需要合成 LINGXI_PRODUCT_DIR"),
    );
    let temporary_root = std::env::temp_dir();
    let synthetic_runtime = temporary_root.parent().expect("合成TMPDIR须位于runtime内");
    let synthetic_home =
        PathBuf::from(std::env::var("LINGXI_HOME").expect("本证明需要合成LINGXI_HOME"));
    assert!(synthetic_home.starts_with(synthetic_runtime));
    assert_eq!(product, synthetic_runtime.join("persona-product"));
    assert!(product.is_absolute());
    assert!(
        !product
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    );
    std::fs::create_dir_all(&product).unwrap();
    assert_eq!(
        product.canonicalize().unwrap().parent().unwrap(),
        synthetic_runtime.canonicalize().unwrap()
    );
    let yuan = format!("original-persona-{tag}");
    for (dir, body) in [
        ("identity-templates", "身份模板原文"),
        ("agents-templates", "说明模板原文"),
    ] {
        std::fs::create_dir_all(product.join(dir)).unwrap();
        std::fs::write(product.join(dir).join(format!("{yuan}.md")), body).unwrap();
    }
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t06-mem-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06t06-mem-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    // 两个 agent（启动后落盘——R02 数据纪元闸只锁 runs.db 种子面；记忆材料
    // 是服务面文件，与 T05 的 canonical home 落盘纪律一致）。
    for agent_id in ["agentA", "agentB"] {
        let agent_dir = home.join("agents").join(agent_id);
        std::fs::create_dir_all(agent_dir.join("memory")).expect("mkdir agent");
        std::fs::write(
            agent_dir.join("config.yaml"),
            format!("locale: zh\nagent:\n  name: 助手\n  yuan: {yuan}\nmemory:\n  enabled: true\n"),
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
    let deps = ServiceDeps {
        workspace_root: Some(workspace.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let denied_token = state
        .auth()
        .issue_device_credential("user_local", &[], None)
        .unwrap()
        .secret;
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
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
        token,
        denied_token,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    async fn raw_get(&self, path: &str, token: &str) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        let payload = format!(
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(payload.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
            .await
            .unwrap()
            .unwrap();
        let boundary = raw.windows(4).position(|part| part == b"\r\n\r\n").unwrap();
        let headers = String::from_utf8(raw[..boundary].to_vec()).unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, headers, raw[boundary + 4..].to_vec())
    }
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            self.token,
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
    }
}

#[tokio::test]
async fn original_persona_reads_template_custom_alias_and_refusal() {
    let server = start_server("read-files").await;
    let a = server.home.join("agents/agentA");
    let b = server.home.join("agents/agentB");
    std::fs::write(b.join("identity.md"), "不得串用的乙身份").unwrap();
    std::fs::write(b.join("AGENTS.md"), "不得串用的乙说明").unwrap();
    std::fs::write(b.join("AGENTS.public.md"), "不得串用的乙公开说明").unwrap();
    let config_before = std::fs::read(a.join("config.yaml")).unwrap();

    for (face, expected) in [
        ("identity", "身份模板原文"),
        ("agents-md", "说明模板原文"),
        ("ishiki", "说明模板原文"),
    ] {
        let (status, body) = server
            .request("GET", &format!("/lingxi/v1/agents/agentA/{face}"), None)
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["content"], expected);
        assert_eq!(body["fromTemplate"], true);
        assert!(!body.to_string().contains("乙"));
    }
    assert!(
        !a.join("identity.md").exists(),
        "读取模板不能偷偷写定制文件"
    );
    assert!(!a.join("AGENTS.md").exists());

    for face in ["public-agents-md", "public-ishiki"] {
        let (status, body) = server
            .request("GET", &format!("/lingxi/v1/agents/agentA/{face}"), None)
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["content"], "");
    }
    for (file, content) in [
        ("identity.md", "甲定制身份"),
        ("AGENTS.md", "甲定制说明"),
        ("AGENTS.public.md", "甲定制公开说明"),
    ] {
        std::fs::write(a.join(file), content).unwrap();
    }
    for (face, expected, template_flag) in [
        ("identity", "甲定制身份", true),
        ("agents-md", "甲定制说明", true),
        ("ishiki", "甲定制说明", true),
        ("public-agents-md", "甲定制公开说明", false),
        ("public-ishiki", "甲定制公开说明", false),
    ] {
        let (status, body) = server
            .request("GET", &format!("/lingxi/v1/agents/agentA/{face}"), None)
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["content"], expected);
        if template_flag {
            assert_eq!(body["fromTemplate"], false);
        }
        assert!(!body.to_string().contains("乙"));
        let (status, _, bytes) = server
            .raw_get(
                &format!("/lingxi/v1/agents/agentA/{face}"),
                &server.denied_token,
            )
            .await;
        assert_eq!(status, 403, "已签发的无权设备不能读取人格材料");
        assert!(!String::from_utf8_lossy(&bytes).contains(expected));
        let (status, body) = server
            .request("GET", &format!("/lingxi/v1/agents/missing/{face}"), None)
            .await;
        assert_eq!(status, 404, "{body}");
        assert!(!body.to_string().contains("甲定制"));
        assert!(!body.to_string().contains("乙"));
    }
    // 无效ID当前为400，原AGENTS要求404仍保留语义缺口，不将此检查说成原条款通过。
    let (status, _) = server
        .request("GET", "/lingxi/v1/agents/bad%2Fid/agents-md", None)
        .await;
    assert_eq!(status, 400);
    assert_eq!(std::fs::read(a.join("config.yaml")).unwrap(), config_before);
    assert_eq!(
        std::fs::read_to_string(a.join("identity.md")).unwrap(),
        "甲定制身份"
    );
    assert_eq!(
        std::fs::read_to_string(a.join("AGENTS.md")).unwrap(),
        "甲定制说明"
    );
    assert_eq!(
        std::fs::read_to_string(a.join("AGENTS.public.md")).unwrap(),
        "甲定制公开说明"
    );
    assert_eq!(
        std::fs::read_to_string(b.join("identity.md")).unwrap(),
        "不得串用的乙身份"
    );
    assert_eq!(
        std::fs::read_to_string(b.join("AGENTS.md")).unwrap(),
        "不得串用的乙说明"
    );
    assert_eq!(
        std::fs::read_to_string(b.join("AGENTS.public.md")).unwrap(),
        "不得串用的乙公开说明"
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_avatar_read_prefers_all_formats_and_preserves_other_agent() {
    let server = start_server("read-avatar").await;
    let a = server.home.join("agents/agentA");
    let b = server.home.join("agents/agentB");
    std::fs::write(b.join("avatar.png"), b"SECRET_B_AVATAR").unwrap();
    for (ext, bytes) in [
        ("png", b"PNG_A"),
        ("jpg", b"JPG_A"),
        ("jpeg", b"JPE_A"),
        ("webp", b"WEB_A"),
    ] {
        std::fs::write(a.join(format!("avatar.{ext}")), bytes).unwrap();
    }
    for (ext, mime, expected) in [
        ("png", "image/png", b"PNG_A"),
        ("jpg", "image/jpeg", b"JPG_A"),
        ("jpeg", "image/jpeg", b"JPE_A"),
        ("webp", "image/webp", b"WEB_A"),
    ] {
        let (status, headers, bytes) = server
            .raw_get("/lingxi/v1/agents/agentA/avatar", &server.token)
            .await;
        assert_eq!(status, 200);
        let content_type = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                    .map(|(_, v)| v.trim())
            })
            .unwrap();
        assert_eq!(content_type, mime);
        assert_eq!(bytes, expected);
        assert!(!String::from_utf8_lossy(&bytes).contains("SECRET_B_AVATAR"));
        std::fs::remove_file(a.join(format!("avatar.{ext}"))).unwrap();
    }
    let (status, _, _) = server
        .raw_get("/lingxi/v1/agents/agentA/avatar", &server.token)
        .await;
    assert_eq!(status, 404);
    let (status, _, bytes) = server
        .raw_get("/lingxi/v1/agents/missing/avatar", &server.token)
        .await;
    assert_eq!(status, 404);
    assert!(!String::from_utf8_lossy(&bytes).contains("SECRET_B_AVATAR"));
    let (status, _, bytes) = server
        .raw_get("/lingxi/v1/agents/agentB/avatar", &server.denied_token)
        .await;
    assert_eq!(status, 403);
    assert!(!String::from_utf8_lossy(&bytes).contains("SECRET_B_AVATAR"));
    assert_eq!(
        std::fs::read(b.join("avatar.png")).unwrap(),
        b"SECRET_B_AVATAR"
    );
    server.stop_and_clean().await;
}
