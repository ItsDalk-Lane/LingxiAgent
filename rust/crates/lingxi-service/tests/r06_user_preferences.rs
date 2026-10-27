//! R06 B 块：自动学习/快捷聊天偏好读写对 + tenets 删除广播。
use std::path::PathBuf;
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-prefs-{uniq}"));
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
    let addr = ready_rx.await.expect("readiness");
    Server {
        addr: addr.to_string(),
        token,
        home,
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

#[tokio::test]
async fn user_preference_pairs_and_tenets_broadcast() {
    let server = boot().await;

    // 自动学习：默认开 / 读取不落盘 / 合并写 / 类型校验 / 保留其他键。
    let (status, body) = ok(&server, "GET", "/lingxi/v1/preferences/autolearn", None).await;
    assert_eq!(status, 200, "读取：{body}");
    assert_eq!(body["autolearn"], json!({ "enabled": true }), "默认开");
    assert!(
        !server.home.join("user/preferences.json").exists(),
        "读取不写盘"
    );

    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/preferences/autolearn",
        Some(&json!({"autolearn": {"enabled": false}})),
    )
    .await;
    assert_eq!(status, 200, "写入：{body}");
    assert_eq!(body["autolearn"], json!({ "enabled": false }));
    assert_eq!(
        preferences(&server)["autolearn"]["enabled"],
        json!(false),
        "落盘"
    );
    let (_, body) = ok(&server, "GET", "/lingxi/v1/preferences/autolearn", None).await;
    assert_eq!(body["autolearn"], json!({ "enabled": false }), "读回");

    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/preferences/autolearn",
        Some(&json!({"autolearn": {"enabled": "yes"}})),
    )
    .await;
    assert_eq!(status, 400, "非布尔 400");
    assert_eq!(
        preferences(&server)["autolearn"]["enabled"],
        json!(false),
        "失败不变"
    );

    // 快捷聊天：默认 / 规范化（snake 输入、上限钳制）/ 合并写。
    let (status, body) = ok(&server, "GET", "/lingxi/v1/preferences/quick-chat", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["quickChat"],
        json!({ "shortcut": "Alt+Space", "reuseTimeoutMinutes": 10 }),
        "默认值"
    );
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/preferences/quick-chat",
        Some(&json!({"quickChat": {"shortcut": "Ctrl+Shift+Q", "reuse_timeout_minutes": 999}})),
    )
    .await;
    assert_eq!(status, 200, "写入：{body}");
    assert_eq!(body["quickChat"]["shortcut"], json!("Ctrl+Shift+Q"));
    assert_eq!(
        body["quickChat"]["reuseTimeoutMinutes"],
        json!(120),
        "上限 120 钳制"
    );
    assert_eq!(
        preferences(&server)["quick_chat"]["shortcut"],
        json!("Ctrl+Shift+Q"),
        "落盘"
    );

    // 合并写保留先前键：autolearn 仍在。
    assert_eq!(
        preferences(&server)["autolearn"]["enabled"],
        json!(false),
        "其他键保留"
    );

    // 扁平 patch：不带 autolearn 包裹同样生效（原 engine 契约两形态）。
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/preferences/autolearn",
        Some(&json!({"enabled": true})),
    )
    .await;
    assert_eq!(status, 200, "扁平 patch：{body}");
    assert_eq!(body["autolearn"], json!({ "enabled": true }));
    let (_, body) = ok(&server, "GET", "/lingxi/v1/preferences/autolearn", None).await;
    assert_eq!(body["autolearn"], json!({ "enabled": true }), "扁平写读回");
    ok(
        &server,
        "PUT",
        "/lingxi/v1/preferences/autolearn",
        Some(&json!({"autolearn": {"enabled": false}})),
    )
    .await;
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/preferences/autolearn",
        Some(&json!({"enabled": "flat-bad"})),
    )
    .await;
    assert_eq!(status, 400, "扁平/包裹同受类型校验");

    // 快捷聊天：读取不改动设置（读取前后偏好字节一致）。
    let prefs_before = std::fs::read(server.home.join("user/preferences.json")).unwrap();
    let (_, body) = ok(&server, "GET", "/lingxi/v1/preferences/quick-chat", None).await;
    assert_eq!(
        body["quickChat"]["shortcut"],
        json!("Ctrl+Shift+Q"),
        "快捷聊天读回"
    );
    assert_eq!(
        std::fs::read(server.home.join("user/preferences.json")).unwrap(),
        prefs_before,
        "快捷聊天读取不写盘"
    );
    // 快捷聊天扁平 patch 同形态生效后复原。
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/preferences/quick-chat",
        Some(&json!({"shortcut": "Alt+Q", "reuse_timeout_minutes": 5})),
    )
    .await;
    assert_eq!(status, 200, "快捷聊天扁平 patch：{body}");
    assert_eq!(body["quickChat"]["shortcut"], json!("Alt+Q"));
    ok(
        &server,
        "PUT",
        "/lingxi/v1/preferences/quick-chat",
        Some(&json!({"quickChat": {"shortcut": "Ctrl+Shift+Q", "reuseTimeoutMinutes": 120}})),
    )
    .await;

    // 鉴权。
    let (status, _) = http(
        &server,
        "GET",
        "/lingxi/v1/preferences/autolearn",
        None,
        None,
    )
    .await;
    assert_eq!(status, 401);

    // tenets 删除广播：建助手 → 写入原则 → 删除 → app 事件含 tenets-changed。
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "T", "id": "agten"})),
    )
    .await;
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents/agten/tenets",
        Some(&json!({"content": "守时"})),
    )
    .await;
    assert!(status == 200 || status == 201, "原则写入：{body}");
    let tenet_id = body
        .pointer("/tenet/id")
        .or_else(|| body.get("id"))
        .and_then(Value::as_str)
        .expect("原则 id")
        .to_string();
    let (status, _) = ok(
        &server,
        "DELETE",
        &format!("/lingxi/v1/agents/agten/tenets/{tenet_id}"),
        None,
    )
    .await;
    assert_eq!(status, 200, "原则删除");
    let (status, _) = ok(
        &server,
        "DELETE",
        &format!("/lingxi/v1/agents/agten/tenets/{tenet_id}"),
        None,
    )
    .await;
    assert_eq!(status, 404, "删除不存在原则 404");
    let (_, feed) = ok(&server, "GET", "/lingxi/v1/app-events?since=0", None).await;
    let hit = feed["events"]
        .as_array()
        .expect("事件数组")
        .iter()
        .any(|event| {
            event["type"] == json!("tenets-changed")
                && event["payload"]["agentId"] == json!("agten")
                && event["payload"]["tenetId"] == json!(tenet_id)
        });
    assert!(hit, "tenets-changed 广播可见");

    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}

#[tokio::test]
async fn experience_search_and_global_config_setter() {
    let server = boot().await;
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "E", "id": "agexp"})),
    )
    .await;
    ok(
        &server,
        "PATCH",
        "/lingxi/v1/agents/agexp/config",
        Some(&json!({"experience": {"enabled": true}})),
    )
    .await;

    // 经验写入（按 # 标题同步）。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agexp/experience",
        Some(&json!({"content": "# 部署经验\n上线前必须跑迁移脚本\n# 备忘\n普通一行"})),
    )
    .await;
    assert_eq!(status, 200, "经验写入");

    // 检索：命中行 + 真实路径 + 不改文件；无命中 0。
    let experience_file = server.home.join("agents/agexp/experience.md");
    let experience_before = std::fs::read(&experience_file).expect("经验文档存在");
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/agents/agexp/experience/search?q=迁移",
        None,
    )
    .await;
    assert_eq!(status, 200, "检索：{body}");
    assert_eq!(body["count"], json!(1));
    let hit = &body["matches"][0];
    assert!(
        hit["path"].as_str().unwrap_or("").contains("agexp"),
        "命中来自当前助手"
    );
    assert!(hit["excerpt"].as_str().unwrap_or("").contains("迁移"));
    assert_eq!(
        std::fs::read(&experience_file).unwrap(),
        experience_before,
        "检索不改经验文件"
    );
    // 跨助手隔离：无经验的助手检索同名关键词必须为空，不泄露他人经验。
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "E2", "id": "agempty"})),
    )
    .await;
    ok(
        &server,
        "PATCH",
        "/lingxi/v1/agents/agempty/config",
        Some(&json!({"experience": {"enabled": true}})),
    )
    .await;
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/agents/agempty/experience/search?q=迁移",
        None,
    )
    .await;
    assert_eq!(status, 200, "空经验检索：{body}");
    assert_eq!(body["count"], json!(0), "不得泄露 agexp 的经验");
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/agents/agexp/experience/search?q=不存在词",
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["count"], json!(0));
    let (status, _) = ok(
        &server,
        "GET",
        "/lingxi/v1/agents/agexp/experience/search",
        None,
    )
    .await;
    assert_eq!(status, 400, "缺 q 400");

    // 全局 setter：纯全局 patch 只写 preferences，不碰助手配置。
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/agents/agexp/config",
        Some(&json!({"keep_awake": true, "locale": "zh-CN"})),
    )
    .await;
    assert_eq!(status, 200, "纯全局 patch：{body}");
    assert_eq!(body["globalApplied"], json!(["locale", "keep_awake"]));
    let prefs = preferences(&server);
    assert_eq!(prefs["keep_awake"], json!(true));
    assert_eq!(prefs["locale"], json!("zh-CN"));
    let config = std::fs::read_to_string(server.home.join("agents/agexp/config.yaml")).unwrap();
    assert!(!config.contains("keep_awake"), "助手配置不被全局键污染");

    // 混合 patch：助手键 + 全局键同时生效。
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/agents/agexp/config",
        Some(&json!({"memory": {"enabled": false}, "keep_awake": false})),
    )
    .await;
    assert_eq!(status, 200, "混合 patch：{body}");
    assert_eq!(preferences(&server)["keep_awake"], json!(false));
    let (_, flags) = ok(&server, "GET", "/lingxi/v1/agents/agexp/config-flags", None).await;
    assert_eq!(flags["memoryEnabled"], json!(false));

    // 类型错误 400 且偏好不变。
    let (status, _) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/agents/agexp/config",
        Some(&json!({"keep_awake": "yes"})),
    )
    .await;
    assert_eq!(status, 400, "全局键类型 400");
    assert_eq!(preferences(&server)["keep_awake"], json!(false), "失败不变");

    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}
