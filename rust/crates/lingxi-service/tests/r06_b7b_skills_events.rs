//! R06 收口 B7b：技能簇 skills-changed 广播语义（原 server/routes/skills.ts
//! 的 emitAppEvent 面）与组合/外部路径/名单/重载各入口的成功广播、逐臂拒绝
//! 不变性。仅绑定原断言，不新增产品语义；所有断言走真实 HTTP 面。
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-b7b-skills-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06-b7b-skills-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    for agent_id in ["agentA", "agentB"] {
        let agent_dir = home.join("agents").join(agent_id);
        std::fs::create_dir_all(agent_dir.join("memory")).expect("mkdir agent");
        std::fs::write(
            agent_dir.join("config.yaml"),
            "locale: zh\nagent:\n  name: 助手\nmemory:\n  enabled: true\n",
        )
        .expect("write config");
    }
    for skill in ["alpha", "beta"] {
        let dir = home.join("skills").join(skill);
        std::fs::create_dir_all(&dir).expect("mkdir skill");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {skill}\ndescription: B7b 技能\n---\n正文"),
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

    /// app 事件面：返回 (最新序号, 自 since 之后类型匹配且 agentId 等于期望的事件数)。
    async fn skills_changed_events(&self, since: u64, expect_agent: Option<&str>) -> (u64, usize) {
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
                        event["type"] == json!("skills-changed")
                            && match expect_agent {
                                None => event["payload"]["agentId"].is_null(),
                                Some(agent) => event["payload"]["agentId"].as_str() == Some(agent),
                            }
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

fn bundles_file(home: &Path) -> PathBuf {
    home.join("skill-bundles.json")
}

fn enabled_flags(view: &Value, name: &str) -> Option<bool> {
    view["skills"]
        .as_array()?
        .iter()
        .find(|skill| skill["name"].as_str() == Some(name))
        .and_then(|skill| skill["enabled"].as_bool())
}

#[tokio::test]
async fn skills_faces_broadcast_and_bundle_boundaries() {
    let server = start_server("broadcast").await;
    let (latest, _) = server.skills_changed_events(0, None).await;

    // ── PUT /agents/{id}/skills：名单写盘后广播 skills-changed（skills.ts:303）。
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/skills",
            Some(json!({"enabled": ["alpha", "beta"]})),
        )
        .await;
    assert_eq!(status, 200, "名单 PUT：{body}");
    assert_eq!(body["enabled"], json!(["alpha", "beta"]));
    let (latest, hits) = server.skills_changed_events(latest, Some("agentA")).await;
    assert_eq!(hits, 1, "名单 PUT 广播一次 skills-changed(agentId=agentA)");

    // 空数组：同一 PUT 接口停用所有可见普通技能（B097 G1）。
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/skills",
            Some(json!({"enabled": []})),
        )
        .await;
    assert_eq!(status, 200, "空数组 PUT：{body}");
    assert_eq!(body["enabled"], json!([]));
    let (status, view) = server
        .request("GET", "/lingxi/v1/skills?agentId=agentA&runtime=1", None)
        .await;
    assert_eq!(status, 200, "运行时列表：{view}");
    assert_eq!(
        (enabled_flags(&view, "alpha"), enabled_flags(&view, "beta")),
        (Some(false), Some(false)),
        "空数组停用所有可见普通技能：{view}"
    );
    let config_text =
        std::fs::read_to_string(server.home.join("agents/agentA/config.yaml")).unwrap();
    assert!(!config_text.contains("- alpha"), "名单清空: {config_text}");
    let (latest, hits) = server.skills_changed_events(latest, Some("agentA")).await;
    assert_eq!(hits, 1, "空数组停用同样广播 skills-changed");
    // 同一接口恢复子集。
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/skills",
            Some(json!({"enabled": ["alpha"]})),
        )
        .await;
    assert_eq!(status, 200);
    let (_, view) = server
        .request("GET", "/lingxi/v1/skills?agentId=agentA&runtime=1", None)
        .await;
    assert_eq!(
        (enabled_flags(&view, "alpha"), enabled_flags(&view, "beta")),
        (Some(true), Some(false)),
        "同一 PUT 接口恢复子集"
    );
    let (latest, _) = server.skills_changed_events(latest, Some("agentA")).await;

    // ── POST /skills/reload：重扫后广播（skills.ts:548-559）。
    let (status, body) = server
        .request("POST", "/lingxi/v1/skills/reload", None)
        .await;
    assert_eq!(status, 200, "reload：{body}");
    assert!(body["scanned"].as_u64().unwrap_or(0) >= 2, "{body}");
    assert!(
        body.get("skills").is_none(),
        "reload 不回传无 agent 视角的列表"
    );
    let (latest, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 1, "reload 广播 skills-changed(agentId=null)");

    // ── PUT /skills/external-paths：保存并重载来源后广播（skills.ts:466-495）。
    let external_root = std::env::temp_dir().join(format!(
        "lingxi-r06-b7b-ext-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let ext_skill = external_root.join("gamma-ext");
    std::fs::create_dir_all(&ext_skill).expect("mkdir external skill");
    std::fs::write(
        ext_skill.join("SKILL.md"),
        "---\nname: gamma-ext\ndescription: 外部技能\n---\n正文",
    )
    .expect("write external SKILL.md");
    let (status, body) = server
        .request(
            "PUT",
            "/lingxi/v1/skills/external-paths",
            Some(json!({"paths": [external_root]})),
        )
        .await;
    assert_eq!(status, 200, "外部路径 PUT：{body}");
    let (latest, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 1, "外部路径保存广播 skills-changed");
    let (status, actual) = server
        .request("GET", "/lingxi/v1/skills/external-paths", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        actual["configured"],
        json!([external_root.to_string_lossy().to_string()]),
        "清单回读一致"
    );
    let (_, view) = server
        .request("GET", "/lingxi/v1/skills?agentId=agentA", None)
        .await;
    let ext_entry = view["skills"]
        .as_array()
        .expect("技能数组")
        .iter()
        .find(|skill| skill["name"].as_str() == Some("gamma-ext"))
        .expect("外部技能可见")
        .clone();
    assert_eq!(
        ext_entry["readonly"],
        json!(true),
        "外部技能只读：{ext_entry}"
    );
    // 空数组移除全部外部路径，同样广播。
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/skills/external-paths",
            Some(json!({"paths": []})),
        )
        .await;
    assert_eq!(status, 200);
    let (latest, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 1, "空数组移除广播 skills-changed");
    let (_, view) = server
        .request("GET", "/lingxi/v1/skills?agentId=agentA", None)
        .await;
    assert!(
        enabled_flags(&view, "gamma-ext").is_none(),
        "移除后外部技能不再可见"
    );
    let _ = std::fs::remove_dir_all(&external_root);

    // ── POST /skills/bundles：创建持久 + 广播（skills.ts:178-206）。
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/skills/bundles?agentId=agentA",
            Some(json!({"name": "Combo", "skillNames": ["alpha", "beta"]})),
        )
        .await;
    assert_eq!(status, 200, "创建组合：{body}");
    let bundle_id = body["bundle"]["id"].as_str().expect("组合 id").to_string();
    assert_eq!(body["bundle"]["skillNames"], json!(["alpha", "beta"]));
    let (latest, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 1, "创建组合广播 skills-changed(agentId=null)");

    // 逐臂拒绝（EFEEEF G1）：缺 query.agentId → 400，存储不变、零事件。
    let store_before = std::fs::read(bundles_file(&server.home)).unwrap_or_default();
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/skills/bundles",
            Some(json!({"name": "NoAgent", "skillNames": ["alpha"]})),
        )
        .await;
    assert_eq!(status, 400, "缺 agentId 400：{body}");
    assert_eq!(
        std::fs::read(bundles_file(&server.home)).unwrap_or_default(),
        store_before,
        "拒绝后组合存储不变"
    );
    let (latest, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 0, "拒绝不广播");
    // 未知成员 → 400，存储不变、零事件。
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/skills/bundles?agentId=agentA",
            Some(json!({"name": "Broken", "skillNames": ["alpha", "ghost-x"]})),
        )
        .await;
    assert_eq!(status, 400, "未知成员 400：{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("ghost-x"),
        "{body}"
    );
    assert_eq!(
        std::fs::read(bundles_file(&server.home)).unwrap_or_default(),
        store_before,
        "拒绝后组合存储不变"
    );
    let (latest, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 0, "拒绝不广播");
    // body.name/skillNames 缺省 → 原实现 normalize 默认（"Skill Bundle"/[]），不是拒绝。
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/skills/bundles?agentId=agentA",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "缺省字段按原实现归一：{body}");
    assert_eq!(body["bundle"]["name"], json!("Skill Bundle"));
    assert_eq!(body["bundle"]["skillNames"], json!([]));
    let default_bundle_id = body["bundle"]["id"].as_str().unwrap().to_string();
    let (latest, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 1, "归一创建同样广播");

    // ── DELETE /skills/bundles/{id}：删除定义 + 广播，不删成员技能（skills.ts:244-253）。
    let (status, body) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/skills/bundles/{bundle_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "删除组合：{body}");
    let (latest, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 1, "删除组合广播 skills-changed");
    let store: Value =
        serde_json::from_slice(&std::fs::read(bundles_file(&server.home)).expect("组合存储存在"))
            .unwrap();
    let remaining: Vec<&str> = store["bundles"]
        .as_array()
        .map(|bundles| {
            bundles
                .iter()
                .filter_map(|bundle| bundle["id"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(!remaining.contains(&bundle_id.as_str()), "定义已删除");
    assert!(
        server.home.join("skills/alpha/SKILL.md").is_file(),
        "成员技能 alpha 保留"
    );
    assert!(
        server.home.join("skills/beta/SKILL.md").is_file(),
        "成员技能 beta 保留"
    );
    // 未知 id → 404 且不广播。
    let (status, _) = server
        .request("DELETE", "/lingxi/v1/skills/bundles/no-such", None)
        .await;
    assert_eq!(status, 404);
    let (_, hits) = server.skills_changed_events(latest, None).await;
    assert_eq!(hits, 0, "404 不广播");
    // 清掉归一默认组合，保持存储干净。
    let (status, _) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/skills/bundles/{default_bundle_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200);

    server.stop_and_clean().await;
}
