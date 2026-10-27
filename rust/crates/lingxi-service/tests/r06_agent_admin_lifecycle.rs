//! R06 助手生命周期集合面：列表/创建/切换/删除/主助手/排序的正反证明。
//! 全部断言经真实 HTTP + 真实磁盘（agents 目录、config.yaml、preferences.json）。
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

impl Server {
    fn preferences(&self) -> Value {
        let path = self.home.join("user/preferences.json");
        serde_json::from_slice(&std::fs::read(path).expect("preferences.json 可读"))
            .expect("preferences.json 合法")
    }
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-agent-admin-{uniq}"));
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
    // 焦点切换创建真实会话；夹具声明实际目录和模型身份，不调用外部供应商。
    let workspace = home.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(
        &json!({"providers":{"lifecycle":{"protocol":"openai-completions","endpoint":"http://127.0.0.1:1/v1","auth":{"kind":"none"}}},"models":{"chat":{"provider":"lifecycle","model":"synthetic","capabilities":{"tools":true}}}}).to_string()
    ).unwrap();
    let credentials = std::sync::Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            std::sync::Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let deps = ServiceDeps {
        workspace_root: Some(workspace),
        model_gateway: Some(std::sync::Arc::new(
            lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
        )),
        credential_service: Some(credentials),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
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

/// 原始 HTTP：返回 (状态码, JSON 体)。空体返回 Value::Null。
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

fn agent_ids(listed: &Value) -> Vec<String> {
    listed["agents"]
        .as_array()
        .expect("agents 数组")
        .iter()
        .map(|agent| agent["id"].as_str().expect("id 字符串").to_string())
        .collect()
}

fn agent_of<'a>(listed: &'a Value, id: &str) -> &'a Value {
    listed["agents"]
        .as_array()
        .expect("agents 数组")
        .iter()
        .find(|agent| agent["id"] == json!(id))
        .unwrap_or_else(|| panic!("列表包含 {id}"))
}

fn config_yaml(server: &Server, id: &str) -> String {
    std::fs::read_to_string(server.home.join("agents").join(id).join("config.yaml"))
        .expect("config.yaml 可读")
}

#[tokio::test]
async fn agent_lifecycle_roundtrip_and_rejections() {
    let server = boot().await;

    // ── 创建：成功 / 重名 / 空名 / 非法 id ──
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Alpha", "id": "alpha"})),
    )
    .await;
    assert_eq!(status, 200, "创建 alpha：{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["id"], json!("alpha"));
    assert_eq!(body["name"], json!("Alpha"));
    assert!(server.home.join("agents/alpha/config.yaml").is_file());

    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Alpha 2", "id": "alpha"})),
    )
    .await;
    assert_eq!(status, 409, "重名创建必须 409：{body}");

    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "   "})),
    )
    .await;
    assert_eq!(status, 400, "空名必须 400：{body}");

    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Bad", "id": "../escape"})),
    )
    .await;
    assert_eq!(status, 400, "路径样式 id 必须 400：{body}");

    let (_, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Beta", "id": "beta"})),
    )
    .await;
    assert_eq!(body["id"], json!("beta"));
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Gamma 婉", "id": "gamma", "yuan": "测试人格"})),
    )
    .await;
    // gamma 配置主目录：清单必须回传有效主目录（未配置者为 null，如实缺席）。
    let gamma_home = server.home.join("gamma-home");
    std::fs::create_dir_all(&gamma_home).unwrap();
    std::fs::write(
        server.home.join("agents/gamma/config.yaml"),
        format!(
            "agent:\n  name: Gamma 婉\n  yuan: 测试人格\ndesk:\n  home_folder: {}\n",
            gamma_home.display()
        ),
    )
    .unwrap();

    // ── 列表：字段/排序/新鲜参数 ──
    let (status, listed) = ok(&server, "GET", "/lingxi/v1/agents", None).await;
    assert_eq!(status, 200);
    assert_eq!(agent_ids(&listed), vec!["alpha", "beta", "gamma"]);
    assert_eq!(agent_of(&listed, "alpha")["name"], json!("Alpha"));
    assert_eq!(agent_of(&listed, "gamma")["yuan"], json!("测试人格"));
    assert_eq!(agent_of(&listed, "beta")["memoryEnabled"], json!(true));
    assert_eq!(agent_of(&listed, "alpha")["isPrimary"], json!(false));
    for agent in listed["agents"].as_array().expect("agents 数组") {
        assert!(
            agent.get("homeFolder").is_some() && agent.get("effectiveHomeFolder").is_some(),
            "清单逐项含主目录字段：{agent}"
        );
    }
    assert_eq!(
        agent_of(&listed, "gamma")["homeFolder"],
        json!(gamma_home.display().to_string()),
        "配置过的主目录如实回传"
    );
    assert_eq!(
        agent_of(&listed, "alpha")["homeFolder"],
        Value::Null,
        "未配置主目录显式为 null"
    );

    let (status, _) = ok(&server, "GET", "/lingxi/v1/agents?fresh=1", None).await;
    assert_eq!(status, 200, "fresh=1 被接受");

    // ── 排序：成功 / 重复 / 未知 / 非数组 ──
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/order",
        Some(&json!({"order": ["gamma", "alpha", "beta"]})),
    )
    .await;
    assert_eq!(status, 200, "排序保存：{body}");
    let (_, listed) = ok(&server, "GET", "/lingxi/v1/agents", None).await;
    assert_eq!(agent_ids(&listed), vec!["gamma", "alpha", "beta"]);

    for (payload, label) in [
        (json!({"order": ["alpha", "alpha"]}), "重复 id"),
        (json!({"order": ["ghost"]}), "未知 id"),
        (json!({"order": "alpha"}), "非数组"),
        (json!({}), "缺少 order"),
    ] {
        let (status, body) = ok(&server, "PUT", "/lingxi/v1/agents/order", Some(&payload)).await;
        assert_eq!(status, 400, "{label} 必须 400：{body}");
    }

    // ── 主助手：成功 / 校验 / 持久化 ──
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/primary",
        Some(&json!({"id": "alpha"})),
    )
    .await;
    assert_eq!(status, 200, "设置主助手：{body}");
    assert_eq!(server.preferences()["primary_agent_id"], json!("alpha"));
    let (_, listed) = ok(&server, "GET", "/lingxi/v1/agents", None).await;
    assert_eq!(agent_of(&listed, "alpha")["isPrimary"], json!(true));
    assert_eq!(agent_of(&listed, "beta")["isPrimary"], json!(false));

    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/primary",
        Some(&json!({"id": "ghost"})),
    )
    .await;
    assert_eq!(status, 404, "未知主助手 404");
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/primary",
        Some(&json!({"id": "../x"})),
    )
    .await;
    assert_eq!(status, 400, "非法主助手 id 400");

    // ── 切换：上下文 / cwd 持久化 / 去重历史 ──
    let first = server.home.join("first");
    let second = server.home.join("second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    let (status, context) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents/switch",
        Some(&json!({"id": "beta", "cwd": first})),
    )
    .await;
    assert_eq!(status, 200, "切换 beta：{context}");
    assert_eq!(context["agent"]["id"], json!("beta"));
    assert_eq!(context["cwd"], json!(first));
    assert_eq!(context["cwdHistory"], json!([first]), "首条历史：{context}");
    assert_eq!(server.preferences()["current_agent_id"], json!("beta"));
    let beta_yaml = config_yaml(&server, "beta");
    let saved_config =
        lingxi_adapters::storage::agent_memory::config::AgentConfigDocument::parse(&beta_yaml)
            .unwrap();
    assert_eq!(
        saved_config.string("desk", "last_cwd").unwrap(),
        Some(first.to_string_lossy().into_owned()),
        "{beta_yaml}"
    );
    assert!(beta_yaml.contains("cwd_history:"), "{beta_yaml}");

    let (_, context) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents/switch",
        Some(&json!({"id": "beta", "cwd": second})),
    )
    .await;
    assert_eq!(
        context["cwdHistory"],
        json!([first, second]),
        "历史按序追加"
    );
    let (_, context) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents/switch",
        Some(&json!({"id": "beta", "cwd": first})),
    )
    .await;
    assert_eq!(
        context["cwdHistory"],
        json!([second, first]),
        "重复 cwd 去重后移到末尾"
    );

    for (payload, expected, label) in [
        (json!({"id": "ghost"}), 404, "切换未知助手"),
        (json!({"id": "../x"}), 400, "切换非法 id"),
        (json!({}), 400, "切换缺 id"),
    ] {
        let (status, body) = ok(&server, "POST", "/lingxi/v1/agents/switch", Some(&payload)).await;
        assert_eq!(status, expected, "{label}：{body}");
    }

    // ── 删除：deleteSkills 显式降级 / 当前助手切换 / 未知 / 最后一个 ──
    let (status, body) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/agents/gamma",
        Some(&json!({"deleteSkills": ["some-skill"]})),
    )
    .await;
    assert_eq!(status, 200, "删除 gamma：{body}");
    assert_eq!(body["skillsDeleted"], json!([]));
    assert_eq!(
        body["skillsSkipped"],
        json!([{ "name": "some-skill", "reason": "removal_failed" }]),
        "不存在的技能按删除失败如实回报"
    );
    assert!(!server.home.join("agents/gamma").exists(), "目录已移除");

    let (status, _) = ok(&server, "DELETE", "/lingxi/v1/agents/ghost", None).await;
    assert_eq!(status, 404, "删除未知 404");

    // 删除当前助手 beta：必须切到替代助手并落盘。
    let (status, body) = ok(&server, "DELETE", "/lingxi/v1/agents/beta", None).await;
    assert_eq!(status, 200, "删除当前助手 beta：{body}");
    assert_eq!(
        body["replacementAgentId"],
        json!("alpha"),
        "按序替代：{body}"
    );
    assert_eq!(
        body["skillsDeleted"],
        json!([]),
        "缺省 deleteSkills 不删除任何技能：{body}"
    );
    assert_eq!(
        body["skillsSkipped"],
        json!([]),
        "缺省 deleteSkills 不产生跳过项：{body}"
    );
    assert_eq!(
        server.preferences()["current_agent_id"],
        json!("alpha"),
        "当前助手指向替代者"
    );

    // 删除主助手 alpha（同时是最后一个）必须拒绝。
    let (status, body) = ok(&server, "DELETE", "/lingxi/v1/agents/alpha", None).await;
    assert_eq!(status, 400, "最后一个助手不可删：{body}");
    assert!(server.home.join("agents/alpha/config.yaml").is_file());

    // ── 已删除标记的助手不出现在列表且不可切换 ──
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "Marked", "id": "marked"})),
    )
    .await;
    std::fs::write(server.home.join("agents/marked/.deleted-agent.json"), "{}").unwrap();
    let (_, listed) = ok(&server, "GET", "/lingxi/v1/agents", None).await;
    assert!(
        !agent_ids(&listed).contains(&"marked".to_string()),
        "标记助手排除"
    );
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents/switch",
        Some(&json!({"id": "marked"})),
    )
    .await;
    assert_eq!(status, 404, "标记助手不可切换");
    let (status, _) = ok(&server, "DELETE", "/lingxi/v1/agents/marked", None).await;
    assert_eq!(status, 404, "标记助手按不存在处理");

    // ── app 事件广播：序列 / 载荷 / 顺序 / 游标 ──
    let (status, feed) = ok(&server, "GET", "/lingxi/v1/app-events?since=0", None).await;
    assert_eq!(status, 200, "事件检索：{feed}");
    assert_eq!(feed["stale"], json!(false));
    let events = feed["events"].as_array().expect("events 数组");
    let observed: Vec<(String, String)> = events
        .iter()
        .map(|event| {
            (
                event["type"].as_str().expect("type").to_string(),
                event["payload"]["agentId"]
                    .as_str()
                    .expect("agentId")
                    .to_string(),
            )
        })
        .collect();
    let expected: Vec<(String, String)> = [
        ("agent-created", "alpha"),
        ("agent-created", "beta"),
        ("agent-created", "gamma"),
        ("session-created", "beta"),
        ("session-focus-changed", "beta"),
        ("agent-switched", "beta"),
        ("session-created", "beta"),
        ("session-focus-changed", "beta"),
        ("agent-switched", "beta"),
        ("session-created", "beta"),
        ("session-focus-changed", "beta"),
        ("agent-switched", "beta"),
        ("agent-deleted", "gamma"),
        // 删除当前助手 beta：替代切换事件先于删除事件（原实现顺序）。
        ("session-created", "alpha"),
        ("session-focus-changed", "alpha"),
        ("agent-switched", "alpha"),
        ("agent-deleted", "beta"),
        ("agent-created", "marked"),
    ]
    .into_iter()
    .map(|(kind, id)| (kind.to_string(), id.to_string()))
    .collect();
    assert_eq!(observed, expected, "广播序列必须与操作历史一致");
    for event in events {
        assert_eq!(event["source"], json!("server"));
        assert!(event["seq"].as_u64().expect("seq") > 0);
        assert!(event["atUnixMs"].as_u64().expect("atUnixMs") > 0);
    }
    assert_eq!(events[0]["payload"]["name"], json!("Alpha"));
    assert_eq!(events[5]["payload"]["agentName"], json!("Beta"));
    assert_eq!(events[5]["payload"]["cwd"], json!(first));
    assert_eq!(events[15]["payload"]["agentName"], json!("Alpha"));

    let latest = feed["latestSequence"].as_u64().expect("latestSequence");
    let (_, tail) = ok(
        &server,
        "GET",
        &format!("/lingxi/v1/app-events?since={latest}"),
        None,
    )
    .await;
    assert_eq!(tail["stale"], json!(false));
    assert_eq!(tail["events"], json!([]), "追平后无新事件");
    let (_, mid) = ok(
        &server,
        "GET",
        &format!("/lingxi/v1/app-events?since={}", latest - 1),
        None,
    )
    .await;
    assert_eq!(mid["events"].as_array().expect("尾部数组").len(), 1);

    let (status, _) = ok(&server, "GET", "/lingxi/v1/app-events?since=-1", None).await;
    assert_eq!(status, 400, "负游标 400");
    let (status, _) = ok(&server, "GET", "/lingxi/v1/app-events?since=abc", None).await;
    assert_eq!(status, 400, "非数字游标 400");

    // ── 鉴权：无凭证拒绝 ──
    let (status, _) = http(&server, "GET", "/lingxi/v1/agents", None, None).await;
    assert_eq!(status, 401, "无凭证列表 401");
    let (status, _) = http(&server, "GET", "/lingxi/v1/app-events", None, None).await;
    assert_eq!(status, 401, "无凭证事件检索 401");
    let (status, _) = http(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "X"})),
        None,
    )
    .await;
    assert_eq!(status, 401, "无凭证创建 401");

    server.stop.send(()).expect("仍在监听");
    let outcome = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机")
        .expect("join")
        .expect("serve 结果");
    let _ = outcome;
    let _ = std::fs::remove_dir_all(&server.home);
}

#[tokio::test]
async fn delete_agent_cascades_orphan_skills() {
    let server = boot().await;
    let home = server.home.clone();
    // 真实用户技能目录 + 两个启用它的助手 + 两个未启用的助手（其一作幸存者）。
    let skill_dir = home.join("skills").join("demo-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo-skill\n---\n演示技能\n",
    )
    .unwrap();
    for (id, name, enabled) in [
        ("skA", "持有者", true),
        ("skB", "共享者", true),
        ("skC", "旁观者", false),
        ("skD", "幸存者", false),
    ] {
        let (status, _) = ok(
            &server,
            "POST",
            "/lingxi/v1/agents",
            Some(&json!({"name": name, "id": id})),
        )
        .await;
        assert_eq!(status, 200);
        if enabled {
            let config = format!(
                "agent:\n  name: {}\nskills:\n  enabled:\n    - demo-skill\n",
                name
            );
            std::fs::write(home.join("agents").join(id).join("config.yaml"), config).unwrap();
        }
    }

    // 删除未启用者并带名单：非孤儿判定按其他启用者跳过（skA/skB 仍启用）。
    let (_, body) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/agents/skC",
        Some(&json!({"deleteSkills": ["demo-skill"]})),
    )
    .await;
    assert_eq!(
        body["skillsSkipped"],
        json!([{ "name": "demo-skill", "reason": "not_orphan" }]),
        "仍有其他助手启用 → 非孤儿"
    );
    assert!(home.join("skills/demo-skill").exists(), "技能保留");

    // 删除一个共享者：仍非孤儿。
    let (_, body) = ok(&server, "DELETE", "/lingxi/v1/agents/skB", None).await;
    let _ = body;
    // 删除最后持有者并连带删除：技能目录消失 + skills-changed 广播。
    let (_, body) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/agents/skA",
        Some(&json!({"deleteSkills": ["demo-skill", "ghost-skill"]})),
    )
    .await;
    assert_eq!(
        body["skillsDeleted"],
        json!(["demo-skill"]),
        "孤儿被连带删除：{body}"
    );
    assert_eq!(
        body["skillsSkipped"],
        json!([{ "name": "ghost-skill", "reason": "removal_failed" }]),
        "不存在的技能如实回报"
    );
    assert!(!home.join("skills/demo-skill").exists(), "技能目录已移除");
    let (_, feed) = ok(&server, "GET", "/lingxi/v1/app-events?since=0", None).await;
    let hit = feed["events"].as_array().unwrap().iter().any(|event| {
        event["type"] == json!("skills-changed") && event["payload"]["agentId"].is_null()
    });
    assert!(hit, "skills-changed 广播可见");

    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
}

#[tokio::test]
async fn creation_generated_id_never_reuses_existing_case_alias_materials() {
    let server = boot().await;
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name":"助手agentA","id":"agentA","yuan":"ming"})),
    )
    .await;
    assert_eq!(status, 200);
    let original_dir = server.home.join("agents/agentA");
    let config_path = original_dir.join("config.yaml");
    let mut original = std::fs::read_to_string(&config_path).unwrap();
    original.push_str("\nretained_user_option: true\nmemory:\n  enabled: true\n");
    std::fs::write(&config_path, &original).unwrap();
    std::fs::create_dir_all(original_dir.join("memory")).unwrap();
    std::fs::write(original_dir.join("memory/facts.md"), "原助手专属资料").unwrap();
    let (status, created) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name":"助手agentA"})),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    assert_eq!(
        std::fs::read_to_string(&config_path).unwrap(),
        original,
        "自动生成ID不得因文件系统大小写别名覆盖原配置"
    );
    assert_eq!(
        std::fs::read_to_string(original_dir.join("memory/facts.md")).unwrap(),
        "原助手专属资料"
    );
    let new_id = created["id"].as_str().unwrap();
    assert!(server
        .home
        .join("agents")
        .join(new_id)
        .join("config.yaml")
        .is_file());
    assert!(
        !server
            .home
            .join("agents")
            .join(new_id)
            .join("memory/facts.md")
            .exists(),
        "新助手不得复用原记忆目录"
    );
    server.stop.send(()).unwrap();
    assert!(!server.handle.await.unwrap().unwrap().drain_timed_out);
}

#[tokio::test]
async fn creation_explicit_id_refuses_unindexed_directory_and_preserves_materials() {
    let server = boot().await;
    let existing = server.home.join("agents/unfinished");
    std::fs::create_dir_all(&existing).unwrap();
    std::fs::write(existing.join("retained.txt"), "未登记目录中的原文件").unwrap();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name":"新助手","id":"unfinished"})),
    )
    .await;
    assert_eq!(status, 409, "尚无配置的现存目录也不得覆盖：{body}");
    assert!(!existing.join("config.yaml").exists());
    assert_eq!(
        std::fs::read_to_string(existing.join("retained.txt")).unwrap(),
        "未登记目录中的原文件"
    );
    server.stop.send(()).unwrap();
    assert!(!server.handle.await.unwrap().unwrap().drain_timed_out);
}
