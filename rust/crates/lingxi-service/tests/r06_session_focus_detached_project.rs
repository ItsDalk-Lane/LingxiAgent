//! R06 C1 第二批：焦点切换 / 独立会话创建（谱系深度）/ 项目归属。
//! 全部经真实 HTTP + 真实 SQLite + 真实 preferences.json。
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    addr: String,
    token: String,
    home: PathBuf,
    state: ServiceState,
    stop: tokio::sync::oneshot::Sender<()>,
    handle:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}

async fn boot() -> Server {
    // 同进程内并行 boot 的 SystemTime 纳秒可能落在同一时钟粒度内（pid 相同），
    // 纯 pid+nanos 曾让两个测试引导同一 runs.db 并在迁移窗口互撞
    // （"table sessions already exists"）；原子序号保证 home 唯一。
    static BOOT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let uniq = format!(
        "{}-{}-{}",
        std::process::id(),
        BOOT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06-c1b-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("mkdir home");
    let home = home.canonicalize().expect("canonical home");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("loopback"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, ServiceDeps::default())
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<std::net::SocketAddr>();
    let serve_state = state.clone();
    let handle = tokio::spawn(async move {
        run(
            serve_state,
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
    let addr = ready_rx.await.expect("readiness");
    Server {
        addr: addr.to_string(),
        token,
        home,
        state,
        stop,
        handle,
    }
}

async fn http(
    server: &Server,
    method: &str,
    path: &str,
    body: Option<&Value>,
    token: Option<&str>,
) -> (u16, Value) {
    let task = async {
        let mut socket = tokio::net::TcpStream::connect(&server.addr).await.unwrap();
        let payload = body.map(|value| value.to_string()).unwrap_or_default();
        let auth = token
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{payload}",
            addr = server.addr,
            len = payload.len(),
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        socket.read_to_end(&mut response).await.unwrap();
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .expect("HTTP 头存在");
        let headers = String::from_utf8_lossy(&response[..boundary]);
        let status: u16 = headers
            .split_whitespace()
            .nth(1)
            .expect("状态码")
            .parse()
            .expect("合法状态码");
        let body = &response[boundary + 4..];
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(body).expect("JSON 体")
        };
        (status, value)
    };
    tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .expect("HTTP 往返在限时内")
}

async fn ok(server: &Server, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    http(server, method, path, body, Some(&server.token)).await
}

fn preferences(server: &Server) -> Value {
    serde_json::from_slice(
        &std::fs::read(server.home.join("user/preferences.json")).expect("preferences 可读"),
    )
    .expect("preferences 合法")
}

async fn create_session(server: &Server, id: &str, agent: &str) {
    let (status, body) = ok(
        server,
        "POST",
        "/lingxi/v1/sessions",
        Some(&json!({"sessionId": id, "agentId": agent, "title": "C1 批二"})),
    )
    .await;
    assert_eq!(status, 201, "会话 {id} 创建：{body}");
}

#[tokio::test]
async fn focus_switch_detached_create_and_project_assignment() {
    let server = boot().await;
    create_session(&server, "sA", "agentA").await;
    create_session(&server, "sB", "agentA").await;

    // ── 焦点切换：成功 / 持久化 / 未知 / 空 id / 归档拒绝 ──
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/switch",
        Some(&json!({"sessionId": "sA"})),
    )
    .await;
    assert_eq!(status, 200, "切换 sA：{body}");
    assert_eq!(body["sessionId"], json!("sA"));
    assert_eq!(body["agentId"], json!("agentA"));
    assert_eq!(body["memoryEnabled"], json!(true));
    assert_eq!(body["authorizedFolders"], json!([]));
    assert_eq!(
        body["browserResume"]["status"],
        json!("skipped"),
        "typed skip"
    );
    assert_eq!(preferences(&server)["current_session_id"], json!("sA"));

    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/switch",
        Some(&json!({"sessionId": "ghost"})),
    )
    .await;
    assert_eq!(status, 404, "未知会话 404");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/switch",
        Some(&json!({"sessionId": "  "})),
    )
    .await;
    assert_eq!(status, 400, "空 id 400");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/archive",
        Some(&json!({"sessionId": "sB"})),
    )
    .await;
    assert_eq!(status, 200, "归档 sB");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/switch",
        Some(&json!({"sessionId": "sB"})),
    )
    .await;
    assert_eq!(status, 409, "归档会话切换 409");
    assert_eq!(
        preferences(&server)["current_session_id"],
        json!("sA"),
        "失败不挪焦点"
    );

    // ── 项目归属：分配 / 清除 / 读回 / 未知会话 / 归档拒绝 ──
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/sA/project",
        Some(&json!({"projectId": " proj-1 "})),
    )
    .await;
    assert_eq!(status, 200, "分配项目：{body}");
    assert_eq!(body["projectId"], json!("proj-1"), "trim 后生效");
    let (_, body) = ok(&server, "GET", "/lingxi/v1/sessions/sA/project", None).await;
    assert_eq!(body["projectId"], json!("proj-1"), "读回一致");
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/sA/project",
        Some(&json!({"projectId": null})),
    )
    .await;
    assert_eq!(status, 200, "清除归属：{body}");
    assert_eq!(body["projectId"], Value::Null);
    let (_, body) = ok(&server, "GET", "/lingxi/v1/sessions/sA/project", None).await;
    assert_eq!(body["projectId"], Value::Null, "清除后读回 null");
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/sA/project",
        Some(&json!({"projectId": ""})),
    )
    .await;
    assert_eq!(status, 200, "空串等价清除");
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/ghost/project",
        Some(&json!({"projectId": "p"})),
    )
    .await;
    assert_eq!(status, 404, "未知会话 404");
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/sB/project",
        Some(&json!({"projectId": "p"})),
    )
    .await;
    assert_eq!(status, 409, "归档会话改归属 409");

    // ── 独立会话创建：不挪焦点 / 谱系 / 深度过限 / 项目联动 ──
    let focus_before = preferences(&server)["current_session_id"].clone();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/new-detached",
        Some(&json!({"sessionId": "d1", "agentId": "agentA", "forkedFromSessionId": "sA", "projectId": "proj-9"})),
    )
    .await;
    assert_eq!(status, 201, "独立创建：{body}");
    assert_eq!(body["sessionId"], json!("d1"));
    assert_eq!(body["forkedFromSessionId"], json!("sA"));
    assert_eq!(body["lineageDepth"], json!(1), "源深度 0 → 子深度 1");
    assert_eq!(body["projectId"], json!("proj-9"));
    assert_eq!(
        preferences(&server)["current_session_id"],
        focus_before,
        "独立创建不切走焦点"
    );
    let (_, body) = ok(&server, "GET", "/lingxi/v1/sessions/d1/project", None).await;
    assert_eq!(body["projectId"], json!("proj-9"), "创建时项目联动落库");

    // 深度链：d1(1) → d2(2) → d3 过限 409。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/new-detached",
        Some(&json!({"sessionId": "d2", "agentId": "agentA", "forkedFromSessionId": "d1"})),
    )
    .await;
    assert_eq!(status, 201, "二级独立创建：{body}");
    assert_eq!(body["lineageDepth"], json!(2));
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/new-detached",
        Some(&json!({"sessionId": "d3", "agentId": "agentA", "forkedFromSessionId": "d2"})),
    )
    .await;
    assert_eq!(status, 409, "深度过限 409：{body}");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/new-detached",
        Some(&json!({"sessionId": "d4", "agentId": "agentA", "forkedFromSessionId": "ghost"})),
    )
    .await;
    assert_eq!(status, 404, "未知谱系源 404");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/new-detached",
        Some(&json!({"title": "无助手"})),
    )
    .await;
    assert_eq!(status, 400, "无当前助手且缺 agentId 400");
    let (status, _) = http(
        &server,
        "POST",
        "/lingxi/v1/sessions/new-detached",
        Some(&json!({"agentId": "agentA"})),
        None,
    )
    .await;
    assert_eq!(status, 401, "无凭证 401");

    // app 事件：切换与创建均有广播。
    let (_, feed) = ok(&server, "GET", "/lingxi/v1/app-events?since=0", None).await;
    let types: Vec<&str> = feed["events"]
        .as_array()
        .expect("事件数组")
        .iter()
        .map(|event| event["type"].as_str().expect("type"))
        .collect();
    assert!(
        types.contains(&"session-focus-changed"),
        "焦点广播存在：{types:?}"
    );
    assert!(
        types.contains(&"session-created"),
        "创建广播存在：{types:?}"
    );

    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}

#[tokio::test]
async fn rotate_continue_deleted_agent_and_draft_cleanup() {
    let server = boot().await;

    // ── 轮换：无当前助手且缺 agentId → 400 ──
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/rotate",
        Some(&json!({})),
    )
    .await;
    assert_eq!(status, 400, "无可解析助手 400：{body}");

    // 建助手并设为主助手，使缺省 agentId 可解析。
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Rot", "id": "rotA"})),
    )
    .await;
    ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/primary",
        Some(&json!({"id": "rotA"})),
    )
    .await;
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Bare", "id": "rotB"})),
    )
    .await;

    // ── 轮换：旧会话归档、新会话接管焦点 ──
    create_session(&server, "s1", "rotA").await;
    create_session(&server, "s2", "rotA").await;
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/rotate",
        Some(&json!({})),
    )
    .await;
    assert_eq!(status, 201, "轮换：{body}");
    assert_eq!(body["archivedSessionId"], json!("s2"), "最近活动会话被归档");
    let rotated1 = body["sessionId"].as_str().expect("新会话 id").to_string();
    assert_eq!(preferences(&server)["current_session_id"], json!(rotated1));
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/switch",
        Some(&json!({"sessionId": "s2"})),
    )
    .await;
    assert_eq!(status, 409, "被轮换的旧会话已归档");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/switch",
        Some(&json!({"sessionId": "s1"})),
    )
    .await;
    assert_eq!(status, 200, "更早会话保持活动");

    // 再轮换：上轮新会话成为最近活动会话而被归档。
    let (_, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/rotate",
        Some(&json!({})),
    )
    .await;
    assert_eq!(
        body["archivedSessionId"],
        json!(rotated1),
        "连续轮换逐个归档"
    );

    // ── 无历史时仍创建 ──
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/rotate",
        Some(&json!({"agentId": "rotB"})),
    )
    .await;
    assert_eq!(status, 201, "无历史轮换：{body}");
    assert_eq!(body["archivedSessionId"], Value::Null, "无可归档项");
    let (status, _) = http(
        &server,
        "POST",
        "/lingxi/v1/sessions/rotate",
        Some(&json!({})),
        None,
    )
    .await;
    assert_eq!(status, 401, "无凭证 401");

    // ── 续接已删 Agent：未删 400 / 删除后成功 / 授权目录继承 ──
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Cont", "id": "contA"})),
    )
    .await;
    create_session(&server, "sC", "contA").await;
    let d1 = {
        let dir = std::env::temp_dir().join(format!("lingxi-r06-c1c-{}-d1", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir.canonicalize().expect("canonical")
    };
    let (_, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/sC/authorized-folders",
        Some(&json!({"action": "add", "folder": d1})),
    )
    .await;
    assert_eq!(
        body["authorizedFolders"].as_array().map(Vec::len),
        Some(1),
        "目录已授权"
    );

    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/continue-deleted-agent",
        Some(&json!({"sessionId": "sC"})),
    )
    .await;
    assert_eq!(status, 400, "助手未删 400：{body}");
    assert_eq!(body["reason"], json!("agent_not_deleted"));

    std::fs::write(server.home.join("agents/contA/.deleted-agent.json"), "{}").unwrap();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/continue-deleted-agent",
        Some(&json!({"sessionId": "sC"})),
    )
    .await;
    assert_eq!(status, 201, "续接成功：{body}");
    assert_ne!(body["agentId"], json!("contA"), "新助手身份");
    assert_eq!(body["agentName"], json!("Cont"), "沿用原助手显示名");
    assert_eq!(body["continuedFromSessionId"], json!("sC"));
    assert_eq!(
        body["authorizedFolders"],
        json!([d1.to_str().unwrap()]),
        "授权目录继承"
    );
    assert_eq!(body["compacted"], json!(false), "无压缩冒充");

    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/continue-deleted-agent",
        Some(&json!({"sessionId": "ghost"})),
    )
    .await;
    assert_eq!(status, 404, "未知会话 404");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/continue-deleted-agent",
        Some(&json!({"sessionId": " "})),
    )
    .await;
    assert_eq!(status, 400, "空 id 400");

    // ── 永久删除清理草稿 ──
    create_session(&server, "sD", "rotB").await;
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/input-drafts",
        Some(&json!({"surface": "electron", "sessionId": "sD", "text": "待清理草稿"})),
    )
    .await;
    assert_eq!(status, 200, "草稿写入");
    let drafts_path = server.home.join("input-drafts.v1.json");
    let drafts: Value =
        serde_json::from_slice(&std::fs::read(&drafts_path).expect("草稿文件")).expect("草稿合法");
    assert!(
        drafts["surfaces"]["electron"]["sessions"]["sD"].is_object(),
        "草稿存在"
    );
    ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/archive",
        Some(&json!({"sessionId": "sD"})),
    )
    .await;
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/archived/delete",
        Some(&json!({"sessionId": "sD"})),
    )
    .await;
    assert_eq!(status, 200, "永久删除：{body}");
    let drafts: Value =
        serde_json::from_slice(&std::fs::read(&drafts_path).expect("草稿文件")).expect("草稿合法");
    assert!(
        drafts["surfaces"]["electron"]["sessions"]["sD"].is_null(),
        "永久删除后草稿条目清除"
    );

    // 永久删除的反例：未知会话 / 空会话 / 无凭证。
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/archived/delete",
        Some(&json!({"sessionId": "ghost"})),
    )
    .await;
    assert_eq!(status, 404, "未知会话 404");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/archived/delete",
        Some(&json!({"sessionId": " "})),
    )
    .await;
    assert_eq!(status, 404, "空会话按未找到明确拒绝");
    let (status, _) = http(
        &server,
        "POST",
        "/lingxi/v1/sessions/archived/delete",
        Some(&json!({"sessionId": "x"})),
        None,
    )
    .await;
    assert_eq!(status, 401, "无凭证 401");

    let _ = std::fs::remove_dir_all(&d1);
    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}

// ── R06-R2（485B2D0A8A02）：前会话运行中（Busy）时轮换的 409 session_busy ──

struct GatedProvider {
    release: Arc<tokio::sync::watch::Sender<bool>>,
}

impl lingxi_kernel::ports::TurnProviderPort for GatedProvider {
    fn descriptor(&self) -> lingxi_kernel::ports::ProviderDescriptor {
        lingxi_kernel::ports::ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a lingxi_protocol::ModelCallId,
        _input: &'a lingxi_kernel::model_exchange::ModelTurnInput,
        _deltas: &'a dyn lingxi_kernel::ports::TurnDeltaSink,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = lingxi_kernel::ports::ProviderTurnResult> + Send + 'a>,
    > {
        let mut gate = self.release.subscribe();
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            gate.wait_for(|open| *open)
                .await
                .expect("gate channel open");
            lingxi_kernel::ports::ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                lingxi_kernel::ports::ProviderTurn::Final {
                    message: lingxi_protocol::NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![lingxi_protocol::ContentBlock::Text {
                            text: "闸门释放后的答复".to_string(),
                        }],
                        model_call_id: Some(lingxi_protocol::ModelCallId::new(call_id)),
                    },
                },
            )
        })
    }
}

/// 闸门 provider 版 boot：execute 挂起直到 release=true（真实运行中态）。
async fn boot_gated(tag: &str) -> (Server, Arc<tokio::sync::watch::Sender<bool>>) {
    static BOOT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let uniq = format!(
        "{}-{}-{}-{tag}",
        std::process::id(),
        BOOT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06r2-busy-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("mkdir home");
    let home = home.canonicalize().expect("canonical home");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("loopback"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let (release, _gate_rx) = tokio::sync::watch::channel(false);
    let release = Arc::new(release);
    let provider = Arc::new(GatedProvider {
        release: Arc::clone(&release),
    });
    let state = ServiceState::bootstrap_with_deps(
        config,
        &layout,
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn lingxi_kernel::ports::TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<std::net::SocketAddr>();
    let serve_state = state.clone();
    let handle = tokio::spawn(async move {
        run(
            serve_state,
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
    let addr = ready_rx.await.expect("readiness");
    (
        Server {
            addr: addr.to_string(),
            token,
            home,
            state,
            stop,
            handle,
        },
        release,
    )
}

#[tokio::test]
async fn rotate_rejects_busy_previous_session_with_409() {
    let (server, release) = boot_gated("rotate-busy").await;
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Busy", "id": "busyA"})),
    )
    .await;
    create_session(&server, "s-busy", "busyA").await;

    // execute 挂起（provider 闸门）→ 会话 admission 被真实 run 持有。
    let exec_task = {
        let addr = server.addr.clone();
        let token = server.token.clone();
        tokio::spawn(async move {
            let payload = json!({ "input": "挂起中的任务" }).to_string();
            let request = format!(
                "POST /lingxi/v1/sessions/s-busy/execute HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len(),
            );
            let mut socket = tokio::net::TcpStream::connect(&addr).await.unwrap();
            use tokio::io::AsyncWriteExt;
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            use tokio::io::AsyncReadExt;
            socket.read_to_end(&mut response).await.unwrap();
            response
        })
    };
    // 等待 admission 真实被持有（run 已入列/运行）。
    let busy = {
        let mut seen = false;
        for _ in 0..400 {
            if is_session_busy(&server, "s-busy") {
                seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        seen
    };
    assert!(busy, "execute 必须先持有会话 admission");

    // 轮换 → 409 session_busy：旧会话运行中拒绝归档/轮换。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/rotate",
        Some(&json!({"agentId": "busyA"})),
    )
    .await;
    assert_eq!(status, 409, "运行中轮换必须 409：{body}");
    assert_eq!(
        body["details"]["reason"],
        json!("session_busy"),
        "reason: {body}"
    );

    // 释放闸门 → run 到终态 → 轮换成功（同一路径的正面）。
    release.send(true).expect("release gate");
    let _ = exec_task.await.expect("execute 完成");
    let settled = {
        let mut ok_seen = false;
        for _ in 0..400 {
            if !is_session_busy(&server, "s-busy") {
                ok_seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        ok_seen
    };
    assert!(settled, "run 终态后 admission 释放");
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/rotate",
        Some(&json!({"agentId": "busyA"})),
    )
    .await;
    assert_eq!(status, 201, "空闲后轮换成功（新会话创建态）：{body}");
    assert_eq!(body["archivedSessionId"], json!("s-busy"), "旧会话归档");

    server.stop.send(()).ok();
    let _ = server.handle.await;
    let _ = std::fs::remove_dir_all(&server.home);
}

/// 该会话当前是否持有 admission（生产 session_admission 注册表；
/// execute 的 run 持有 → archive_session 的 acquire 同表拒绝）。
fn is_session_busy(server: &Server, session_id: &str) -> bool {
    server
        .state
        .storage()
        .session_admission()
        .is_busy(session_id)
}
