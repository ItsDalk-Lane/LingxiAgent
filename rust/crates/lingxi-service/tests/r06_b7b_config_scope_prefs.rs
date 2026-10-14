//! R06 收口 B7b：偏好簇 splitByScope 语义。原 agents.ts PUT /agents/:id/config
//! 的端口（agent_config_put_route）：全局 setter 写 preferences.json、纯全局
//! patch 不碰助手 config.yaml、跨助手读取同一全局偏好（config-flags 最小读面）、
//! editor（全局叶）与 chat 布局（助手叶）双请求各归其叶、真 mixed patch 同请求
//! 覆盖两叶、后段失败时前段全局写入的部分成功读回（不宣称原子回滚）、
//! memory.enabled 的 disabledSince/reenableAt 与 ticker 缺席诚实面。
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-b7b-prefs-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06-b7b-prefs-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    for agent_id in ["agentA", "agentB"] {
        let agent_dir = home.join("agents").join(agent_id);
        std::fs::create_dir_all(agent_dir.join("memory")).expect("mkdir agent");
        std::fs::write(
            agent_dir.join("config.yaml"),
            "locale: zh\nagent:\n  name: 助手\nmemory:\n  enabled: true\n",
        )
        .expect("write config");
    }
    // 工作区技能：standard（.agents/skills）与 compatible（.claude/skills）各一。
    for (sub, name) in [(".agents/skills", "wskill"), (".claude/skills", "cskill")] {
        let dir = workspace.join(sub).join(name);
        std::fs::create_dir_all(&dir).expect("mkdir workspace skill");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: 工作区技能\n---\n正文"),
        )
        .expect("write SKILL.md");
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
        workspace,
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
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    /// app 事件面增量：返回 (最新序号, since 之后 type==expect 且 payload 命中的事件)。
    async fn events_since(&self, since: u64, expect_type: &str, payload: Value) -> (u64, usize) {
        let (status, feed) = self
            .request("GET", &format!("/lingxi/v1/app-events?since={since}"), None)
            .await;
        assert_eq!(status, 200, "app-events 读: {feed}");
        let latest = feed["latestSequence"].as_u64().unwrap_or(since);
        let hits = feed["events"]
            .as_array()
            .map(|events| {
                events
                    .iter()
                    .filter(|event| {
                        event["type"] == json!(expect_type)
                            && payload
                                .as_object()
                                .map(|wanted| {
                                    wanted.iter().all(|(key, value)| {
                                        event["payload"].get(key) == Some(value)
                                    })
                                })
                                .unwrap_or(true)
                    })
                    .count()
            })
            .unwrap_or(0);
        (latest, hits)
    }

    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
        std::fs::remove_dir_all(&self.workspace).expect("clean isolated test workspace");
    }
}

fn preferences(home: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(home.join("user/preferences.json")).unwrap_or_default())
        .unwrap_or(Value::Null)
}

fn agent_config(home: &Path, agent_id: &str) -> String {
    std::fs::read_to_string(home.join("agents").join(agent_id).join("config.yaml"))
        .unwrap_or_default()
}

async fn runtime_skill_enabled(server: &TestServer, agent: &str, name: &str) -> Option<bool> {
    let (status, view) = server
        .request(
            "GET",
            &format!("/lingxi/v1/skills?agentId={agent}&runtime=1"),
            None,
        )
        .await;
    assert_eq!(status, 200, "运行时列表：{view}");
    view["skills"]
        .as_array()?
        .iter()
        .find(|skill| skill["name"].as_str() == Some(name))
        .and_then(|skill| skill["enabled"].as_bool())
}

#[tokio::test]
async fn global_agent_leaf_split_cross_agent_readback_and_immediate_refilter() {
    let server = start_server("split").await;
    let config_a_before = agent_config(&server.home, "agentA");
    let config_b_before = agent_config(&server.home, "agentB");

    // 基线：两助手读到的全局偏好一致（未写盘键为 null）。
    let (status, flags_a) = server
        .request("GET", "/lingxi/v1/agents/agentA/config-flags", None)
        .await;
    assert_eq!(status, 200, "{flags_a}");
    let (status, flags_b) = server
        .request("GET", "/lingxi/v1/agents/agentB/config-flags", None)
        .await;
    assert_eq!(status, 200, "{flags_b}");
    assert_eq!(flags_a["preferences"], flags_b["preferences"]);
    assert_eq!(flags_a["preferences"]["keep_awake"], json!(Value::Null));
    let (latest, _) = server
        .events_since(0, "keep-awake-changed", json!({}))
        .await;

    // ── 纯全局 patch：keep_awake 只经 setter 写 preferences.json。
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"keep_awake": true})),
        )
        .await;
    assert_eq!(status, 200, "纯全局 patch：{body}");
    assert_eq!(preferences(&server.home)["keep_awake"], json!(true));
    assert_eq!(
        agent_config(&server.home, "agentA"),
        config_a_before,
        "agentPartial 为空时不写助手 config.yaml"
    );
    let (latest, hits) = server
        .events_since(latest, "keep-awake-changed", json!({"keep_awake": true}))
        .await;
    assert_eq!(hits, 1, "keep-awake-changed 广播");
    // 跨助手读取同一全局偏好：B 的读面与 A 完全一致。
    let (status, flags_a) = server
        .request("GET", "/lingxi/v1/agents/agentA/config-flags", None)
        .await;
    assert_eq!(status, 200);
    let (status, flags_b) = server
        .request("GET", "/lingxi/v1/agents/agentB/config-flags", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(flags_a["preferences"]["keep_awake"], json!(true));
    assert_eq!(
        flags_a["preferences"], flags_b["preferences"],
        "跨助手读到相同全局偏好"
    );
    assert_eq!(agent_config(&server.home, "agentB"), config_b_before);

    // ── editor 排版（全局叶）：请求一，只写 preferences.json（按排版契约归一）。
    let editor_patch = json!({"markdown": {"fontPreset": "serif", "lineHeight": 1.6}});
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"editor": editor_patch})),
        )
        .await;
    assert_eq!(status, 200, "editor 全局叶：{body}");
    assert_eq!(
        preferences(&server.home)["editor"]["markdown"]["fontPreset"],
        json!("serif")
    );
    assert_eq!(
        preferences(&server.home)["editor"]["markdown"]["lineHeight"],
        json!(1.6)
    );
    assert_eq!(
        agent_config(&server.home, "agentA"),
        config_a_before,
        "editor 全局叶不写助手 config.yaml"
    );
    let (latest, hits) = server
        .events_since(
            latest,
            "editor-typography-changed",
            json!({"editor": editor_patch}),
        )
        .await;
    assert_eq!(hits, 1, "editor-typography-changed 广播");

    // ── chat 布局（助手叶）：请求二，只写 agents/agentA/config.yaml。
    let prefs_before_chat = std::fs::read(server.home.join("user/preferences.json")).unwrap();
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"chat": {"layout": "compact", "showTimestamps": true}})),
        )
        .await;
    assert_eq!(status, 200, "chat 助手叶：{body}");
    let config_a_after_chat = agent_config(&server.home, "agentA");
    assert!(
        config_a_after_chat.contains("chat:"),
        "{config_a_after_chat}"
    );
    assert!(
        config_a_after_chat.contains("layout: compact"),
        "{config_a_after_chat}"
    );
    assert_eq!(
        std::fs::read(server.home.join("user/preferences.json")).unwrap(),
        prefs_before_chat,
        "chat 助手叶不写 preferences.json（两次请求各归其叶，非单次 mixed patch）"
    );
    assert_eq!(agent_config(&server.home, "agentB"), config_b_before);
    let (latest, hits) = server
        .events_since(latest, "keep-awake-changed", json!({}))
        .await;
    assert_eq!(hits, 0, "chat 助手叶请求不产生全局事件");
    let (latest, hits) = server
        .events_since(latest, "editor-typography-changed", json!({}))
        .await;
    assert_eq!(hits, 0);

    // ── 工作区技能策略：写配置后当前助手立即重筛（同一现役服务实例）。
    assert_eq!(
        (
            runtime_skill_enabled(&server, "agentA", "wskill").await,
            runtime_skill_enabled(&server, "agentA", "cskill").await
        ),
        (Some(true), Some(false)),
        "默认策略：standard 开、compatible 关"
    );
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"workspace_context": {"discover_project_skills": false}})),
        )
        .await;
    assert_eq!(status, 200, "策略 patch：{body}");
    assert_eq!(
        runtime_skill_enabled(&server, "agentA", "wskill").await,
        Some(false),
        "同一助手运行时立即重筛：standard 工作区技能停用"
    );
    let (latest, hits) = server
        .events_since(latest, "skills-changed", json!({"agentId": "agentA"}))
        .await;
    assert_eq!(hits, 1, "策略变化广播 skills-changed");
    let (status, flags) = server
        .request("GET", "/lingxi/v1/agents/agentA/config-flags", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        flags["workspaceContext"]["discoverProjectSkills"],
        json!(false)
    );
    assert!(
        agent_config(&server.home, "agentA").contains("discover_project_skills: false"),
        "策略写入 config.yaml"
    );
    // 重新启用恢复；compatible 开启后另一类工作区技能可用。
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"workspace_context": {
                "discover_project_skills": true,
                "discover_compatible_project_skills": true
            }})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        (
            runtime_skill_enabled(&server, "agentA", "wskill").await,
            runtime_skill_enabled(&server, "agentA", "cskill").await
        ),
        (Some(true), Some(true)),
        "恢复策略后两类工作区技能都可用"
    );
    let (_, hits) = server
        .events_since(latest, "skills-changed", json!({"agentId": "agentA"}))
        .await;
    assert_eq!(hits, 1);

    server.stop_and_clean().await;
}

#[tokio::test]
async fn mixed_patch_partial_success_and_memory_toggle_timestamps() {
    let server = start_server("mixed").await;

    // ── 真 mixed patch：keep_awake（全局叶）+ chat（助手叶）同请求覆盖两叶。
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"keep_awake": true, "chat": {"layout": "wide"}})),
        )
        .await;
    assert_eq!(status, 200, "mixed patch：{body}");
    assert_eq!(preferences(&server.home)["keep_awake"], json!(true));
    assert!(
        agent_config(&server.home, "agentA").contains("layout: wide"),
        "助手叶同请求落盘"
    );
    let (latest, keep_hits) = server
        .events_since(0, "keep-awake-changed", json!({"keep_awake": true}))
        .await;
    assert_eq!(keep_hits, 1, "mixed patch 的全局事件仍发布");

    // ── 后段失败：全局 setter 已写入 → 部分成功读回，不回滚。
    let config_before_failure = agent_config(&server.home, "agentA");
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"keep_awake": false, "api": {"api_key": "sk-b7b-inline"}})),
        )
        .await;
    assert_eq!(status, 400, "助手段（内联凭证缺 provider）后段失败：{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("api.provider is required"),
        "{body}"
    );
    assert_eq!(
        preferences(&server.home)["keep_awake"],
        json!(false),
        "前段全局 setter 已写入被如实读回（部分成功，不宣称原子回滚）"
    );
    assert_eq!(
        agent_config(&server.home, "agentA"),
        config_before_failure,
        "失败请求不改助手 config.yaml"
    );
    let (_, keep_hits) = server
        .events_since(latest, "keep-awake-changed", json!({}))
        .await;
    assert_eq!(keep_hits, 0, "失败请求不发布成功事件");

    // ── memory.enabled 时间戳：关→disabledSince，开→reenableAt（agents.ts:757）。
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"memory": {"enabled": false}})),
        )
        .await;
    assert_eq!(status, 200, "关闭记忆：{body}");
    let disabled_config = agent_config(&server.home, "agentA");
    assert!(
        disabled_config.contains("disabledSince:"),
        "disabledSince 写入：{disabled_config}"
    );
    assert!(
        disabled_config.contains("enabled: false"),
        "{disabled_config}"
    );
    let (_, hits) = server
        .events_since(
            latest,
            "memory-master-changed",
            json!({"agentId": "agentA", "enabled": false}),
        )
        .await;
    assert_eq!(hits, 1, "memory-master-changed(enabled=false) 广播");
    // 端口的诚实缺席面：记忆关闭时 ticker 步骤因 memory_disabled 不跑（无 R07 ticker 可暂停）。
    let (status, health) = server
        .request("GET", "/lingxi/v1/memories/health?agentId=agentA", None)
        .await;
    assert_eq!(status, 200, "{health}");
    assert_eq!(health["enabled"], json!(false));
    let ticker_step = health["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .find(|step| step["step"] == json!("memory_ticker"))
        .expect("memory_ticker 步骤")
        .clone();
    assert_eq!(ticker_step["ok"], json!(false));
    assert_eq!(ticker_step["reason"], json!("memory_disabled"));
    // 重新启用 → reenableAt；ticker 步骤回到诚实缺席（无 ticker 宿主，不猜造恢复）。
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"memory": {"enabled": true}})),
        )
        .await;
    assert_eq!(status, 200, "重新启用记忆：{body}");
    let reenabled_config = agent_config(&server.home, "agentA");
    assert!(
        reenabled_config.contains("reenableAt:"),
        "reenableAt 写入：{reenabled_config}"
    );
    assert!(
        reenabled_config.contains("enabled: true"),
        "{reenabled_config}"
    );
    let (status, health) = server
        .request("GET", "/lingxi/v1/memories/health?agentId=agentA", None)
        .await;
    assert_eq!(status, 200, "{health}");
    assert_eq!(health["enabled"], json!(true));
    let ticker_step = health["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .find(|step| step["step"] == json!("memory_ticker"))
        .expect("memory_ticker 步骤")
        .clone();
    assert_eq!(ticker_step["ok"], json!(false));
    assert_eq!(
        ticker_step["reason"],
        json!("ticker_absent_in_candidate"),
        "无 ticker 宿主时如实报告缺席，不冒充恢复"
    );

    server.stop_and_clean().await;
}
