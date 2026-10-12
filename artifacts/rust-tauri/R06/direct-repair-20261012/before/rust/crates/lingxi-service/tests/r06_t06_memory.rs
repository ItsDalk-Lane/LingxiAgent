//! R06-T06 服务级测试（真 TCP + 真 HTTP + 真 bootstrap，T05 TestServer 模式）。
//!
//! R06-A11「Agent 记忆不串用」：两个 agent 各有事实与原则；分别读写；
//! 只读/改所属范围；跨 agent 与非法 id 被拒；子代理视角不扩大（工具面
//! 构造期绑定单一 agent，见 memorytools.rs + A12 套件）。

use std::net::SocketAddr;
use std::path::PathBuf;
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
    token: String,
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
    let deps = ServiceDeps {
        workspace_root: Some(workspace.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
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
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
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

// ── R06-A11：分别询问与写入只落所属范围 ─────────────────────────────────────

#[tokio::test]
async fn a11_memory_read_write_isolated_between_agents() {
    let server = start_server("a11-full").await;

    // 写入：import 到 A（v2 facts 形状）
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/memories/import?agentId=agentA",
            Some(json!({"facts": [
                {"fact": "AgentA 专属事实：写 Rust", "tags": ["技能"], "session_id": "s1"},
                {"fact": "AgentA 专属事实：喝美式", "tags": [], "session_id": "s1"}
            ]})),
        )
        .await;
    assert_eq!(status, 200, "import A: {body}");
    assert_eq!(body["imported"], 2);

    // 分别询问：A 有 2 条，B 是空库
    let (status, body) = server
        .request("GET", "/lingxi/v1/memories?agentId=agentA", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["memories"].as_array().unwrap().len(), 2);
    let (status, body) = server
        .request("GET", "/lingxi/v1/memories?agentId=agentB", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["memories"].as_array().unwrap().len(), 0);

    // B 写入自己的事实，A 不受影响
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/memories/import?agentId=agentB",
            Some(json!({"facts": [{"fact": "AgentB 专属事实：用 Vim", "tags": []}]})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (_, body) = server
        .request("GET", "/lingxi/v1/memories?agentId=agentA", None)
        .await;
    assert_eq!(
        body["memories"].as_array().unwrap().len(),
        2,
        "A 不被 B 的写入污染"
    );

    // 清除隔离：B 清空不动 A
    let (status, _) = server
        .request("DELETE", "/lingxi/v1/memories?agentId=agentB", None)
        .await;
    assert_eq!(status, 200);
    let (_, body) = server
        .request("GET", "/lingxi/v1/memories?agentId=agentA", None)
        .await;
    assert_eq!(
        body["memories"].as_array().unwrap().len(),
        2,
        "B 的清空绝不清 A"
    );

    server.stop_and_clean().await;
}

#[tokio::test]
async fn a11_tenets_and_compiled_memory_scoped_per_agent() {
    let server = start_server("a11-tenets").await;

    // tenets：A 直接钉一条 + B 提案一条
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/tenets",
            Some(json!({"content": "A 的原则：先测试后提交"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["tenet"]["status"], "active");

    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentB/tenets",
            Some(json!({"content": "B 的原则：文档优先"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");

    let (_, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/tenets", None)
        .await;
    let a_contents: Vec<&str> = body["tenets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["content"].as_str().unwrap())
        .collect();
    assert_eq!(a_contents, vec!["A 的原则：先测试后提交"]);
    let (_, body) = server
        .request("GET", "/lingxi/v1/agents/agentB/tenets", None)
        .await;
    let b_contents: Vec<&str> = body["tenets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["content"].as_str().unwrap())
        .collect();
    assert_eq!(b_contents, vec!["B 的原则：文档优先"]);

    // pinned shim 只回 active
    let (_, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/pinned", None)
        .await;
    assert_eq!(body["pins"], json!(["A 的原则：先测试后提交"]));

    // compiled memory 写入按 agent 分隔
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/memories/compiled/facts?agentId=agentA",
            Some(json!({"facts": "- A 的编译事实"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (_, body) = server
        .request("GET", "/lingxi/v1/memories/compiled?agentId=agentA", None)
        .await;
    assert!(body["content"].as_str().unwrap().contains("A 的编译事实"));
    assert!(body["content"].as_str().unwrap().contains("## 重要事实"));
    let (_, body) = server
        .request("GET", "/lingxi/v1/memories/compiled?agentId=agentB", None)
        .await;
    assert!(
        !body["content"].as_str().unwrap().contains("A 的编译事实"),
        "B 的编译视图绝不含 A 的内容"
    );

    // week days 编辑：只允许已存在日期（config.ts:569-573 语义）
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/memories/compiled/week/days/2026-01-01?agentId=agentA",
            Some(json!({"body": "x"})),
        )
        .await;
    assert_eq!(status, 404, "无该日条目 → 404: {body}");

    server.stop_and_clean().await;
}

// ── A11 负向：跨 agent 与非法 id 拒绝 ──────────────────────────────────────

#[tokio::test]
async fn a11_cross_agent_and_invalid_ids_are_rejected() {
    let server = start_server("a11-negative").await;

    // 缺 agentId → 400
    let (status, _) = server.request("GET", "/lingxi/v1/memories", None).await;
    assert_eq!(status, 400);

    // 未知 agentId → 404
    let (status, body) = server
        .request("GET", "/lingxi/v1/memories?agentId=ghost", None)
        .await;
    assert_eq!(status, 404, "{body}");

    // 路径形态 id → 400（config.ts:425 语义）
    for bad in ["..%2FagentA", "a.b"] {
        let (status, _) = server
            .request("GET", &format!("/lingxi/v1/memories?agentId={bad}"), None)
            .await;
        assert!(
            status == 400 || status == 404,
            "path-shaped id {bad} rejected, got {status}"
        );
    }

    // tenets 路由未知 agent → 404
    let (status, _) = server
        .request("GET", "/lingxi/v1/agents/ghost/tenets", None)
        .await;
    assert_eq!(status, 404);

    // 非法内容 → 400/409 语义（不是静默成功）
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/tenets",
            Some(json!({"content": "   "})),
        )
        .await;
    assert_eq!(status, 400, "{body}");

    // 低权 principal（无 settings.read scope）→ 403
    {
        let payload = format!(
            "GET /lingxi/v1/memories?agentId=agentA HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            server.token
        );
        let _ = payload; // local owner 全 scope；跨主体验证在 auth 套件（route_policy_table）
    }

    server.stop_and_clean().await;
}

// ── 记忆工具面 + dream 面错误语义 ───────────────────────────────────────────

#[tokio::test]
async fn dream_faces_report_absence_and_revisions_serve_from_store() {
    let server = start_server("dream").await;

    // ticker 缺席 → 503 dream_unavailable（memory-dream.ts:57-58 语义）
    let (status, body) = server
        .request(
            "GET",
            "/lingxi/v1/memories/dream/status?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["code"], "dream_unavailable");
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/memories/dream/runs?agentId=agentA",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 503);

    // revisions 面（文件存储直读，无 ticker 依赖）
    let (status, body) = server
        .request(
            "GET",
            "/lingxi/v1/memories/dream/revisions?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["revisions"].as_array().unwrap().len(), 0);

    // 未知修订 → 404 dream_revision_not_found
    let (status, body) = server
        .request(
            "GET",
            "/lingxi/v1/memories/dream/revisions/nope?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["code"], "dream_revision_not_found");

    server.stop_and_clean().await;
}

#[tokio::test]
async fn experience_gating_and_feedback_shape() {
    let server = start_server("experience").await;

    // experience 默认关闭 → 403（agents.ts:1000-1002）
    let (status, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/experience", None)
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "experience is paused");

    // 打开开关（config.yaml patch 面）
    let (status, _) = server
        .request(
            "PATCH",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"experience": {"enabled": true}})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/config-flags", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["experienceEnabled"], true);

    // 反馈（不受 experience.enabled 门控；rating/excerpt 校验）
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/experience/feedback",
            Some(json!({"rating": "up", "excerpt": "先读文档再动手", "sessionId": "abcdefgh1234"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["added"], true);
    assert_eq!(body["category"], "用户认可的做法");
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/experience/feedback",
            Some(json!({"rating": "sideways", "excerpt": "x"})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/experience/feedback",
            Some(json!({"rating": "up", "excerpt": "  "})),
        )
        .await;
    assert_eq!(status, 400);

    // 打开后 GET 返回合并内容
    let (status, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/experience", None)
        .await;
    assert_eq!(status, 200);
    assert!(body["content"].as_str().unwrap().contains("1. ["));
    server.stop_and_clean().await;
}

#[tokio::test]
async fn persona_files_follow_fallback_chain_and_broadcast_writes() {
    let server = start_server("persona").await;

    // GET（模板回落，fromTemplate=true；agent 无 identity.md）
    let (status, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/identity", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["fromTemplate"], true);

    // PUT（用户定制落盘）
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/identity",
            Some(json!({"content": "自定义身份文本"})),
        )
        .await;
    assert_eq!(status, 200);
    let on_disk = std::fs::read_to_string(
        server
            .home
            .join("agents")
            .join("agentA")
            .join("identity.md"),
    )
    .unwrap();
    assert_eq!(on_disk, "自定义身份文本");
    let (_, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/identity", None)
        .await;
    assert_eq!(body["content"], "自定义身份文本");
    assert_eq!(body["fromTemplate"], false);

    // AGENTS.md + ishiki 别名同 handler（agents.ts:7-8）
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/ishiki",
            Some(json!({"content": "人格"})),
        )
        .await;
    assert_eq!(status, 200);
    let on_disk =
        std::fs::read_to_string(server.home.join("agents").join("agentA").join("AGENTS.md"))
            .unwrap();
    assert_eq!(on_disk, "人格");
    let (status, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/agents-md", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["content"], "人格");

    // public persona + public-ishiki 别名
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/public-ishiki",
            Some(json!({"content": "公开说明"})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, body) = server
        .request("GET", "/lingxi/v1/agents/agentA/public-agents-md", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["content"], "公开说明");

    server.stop_and_clean().await;
}

#[tokio::test]
async fn context_notes_session_scoped_roundtrip() {
    let server = start_server("notes").await;
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/sessions/sess-test-1/context-notes",
            Some(json!({"notes": "关键坐标：/tmp/x"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = server
        .request("GET", "/lingxi/v1/sessions/sess-test-1/context-notes", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["notes"], "关键坐标：/tmp/x");
    // 溢出 → 409 CONTEXT_NOTES_OVERSIZE
    let big = "y".repeat(17 * 1024);
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/sessions/sess-test-1/context-notes",
            Some(json!({"notes": big})),
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "CONTEXT_NOTES_OVERSIZE");
    server.stop_and_clean().await;
}
