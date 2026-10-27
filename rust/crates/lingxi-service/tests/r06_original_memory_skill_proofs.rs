//! 原 F-D07 逐断言补充：真实 HTTP、生产存储和所属 Agent 回读；不补未实现产品功能。
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
    }
}

fn memory_dir(server: &TestServer, agent: &str) -> PathBuf {
    server.home.join("agents").join(agent).join("memory")
}
fn facts(server: &TestServer, agent: &str) -> Vec<lingxi_adapters::storage::agent_memory::FactRow> {
    lingxi_service::memory::MemoryService::new(server.home.clone(), server.home.join("runtime"))
        .fact_store(agent)
        .unwrap()
        .get_all()
        .unwrap()
}
fn file_snapshot(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
        if !dir.exists() {
            return;
        }
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if !path.to_string_lossy().contains("facts.db") {
                out.push((
                    path.strip_prefix(root).unwrap().to_string_lossy().into(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}
async fn denied_write_keeps_files(server: &TestServer, method: &str, path: &str, body: Value) {
    let before_a = file_snapshot(&memory_dir(server, "agentA"));
    let before_b = file_snapshot(&memory_dir(server, "agentB"));
    let (status, response) = server
        .request_token(method, path, Some(body), &server.readonly_token)
        .await;
    assert_eq!(
        status, 403,
        "只有 settings.read 的真实凭证不能写：{response}"
    );
    assert_eq!(file_snapshot(&memory_dir(server, "agentA")), before_a);
    assert_eq!(file_snapshot(&memory_dir(server, "agentB")), before_b);
}

#[tokio::test]
async fn original_facts_v1_v2_import_export_and_rejections() {
    let server = start_server("facts-original").await;
    for (payload, expected) in [
        (
            json!({"version":1,"memories":[{"content":"旧版事实甲","date":"2026-01-01","tags":["旧版"]}]}),
            1,
        ),
        (
            json!({"version":2,"facts":[{"fact":"新版事实乙","time":"2026-01-02","tags":["新版"],"session_id":"原会话"}]}),
            1,
        ),
    ] {
        let (status, result) = server
            .request(
                "POST",
                "/lingxi/v1/memories/import?agentId=agentA",
                Some(payload),
            )
            .await;
        assert_eq!(status, 200, "{result}");
        assert_eq!(result["imported"], expected);
    }
    let stored = facts(&server, "agentA");
    assert_eq!(stored.len(), 2);
    assert!(stored.iter().any(|r| r.fact == "旧版事实甲"
        && r.time.as_deref() == Some("2026-01-01")
        && r.tags == ["旧版"]));
    assert!(stored
        .iter()
        .any(|r| r.fact == "新版事实乙" && r.session_id.as_deref() == Some("原会话")));
    assert!(
        facts(&server, "agentB").is_empty(),
        "其他 Agent 不被导入污染"
    );
    let (status, list) = server
        .request("GET", "/lingxi/v1/memories?agentId=agentA", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(list["memories"].as_array().unwrap().len(), 2);
    let (status, export) = server
        .request("GET", "/lingxi/v1/memories/export?agentId=agentA", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(export["version"], 2);
    assert!(export["exportedAt"].as_str().unwrap().contains('T'));
    assert_eq!(
        export["facts"], list["memories"],
        "导出为实际全部事实而不是缓存子集"
    );
    for payload in [json!({}), json!({"facts":[]}), json!({"facts":"bad"})] {
        let (status, error) = server
            .request(
                "POST",
                "/lingxi/v1/memories/import?agentId=agentA",
                Some(payload),
            )
            .await;
        assert_eq!(status, 400);
        assert!(error["error"].is_string());
        assert_eq!(facts(&server, "agentA").len(), 2);
    }
    let (status, error) = server
        .request_token(
            "POST",
            "/lingxi/v1/memories/import?agentId=agentA",
            Some(json!({"facts":[{"fact":"禁止写入"}]})),
            &server.readonly_token,
        )
        .await;
    assert_eq!(status, 403, "{error}");
    assert_eq!(facts(&server, "agentA").len(), 2);
    for path in [
        "/lingxi/v1/memories?agentId=ghost",
        "/lingxi/v1/memories/export?agentId=ghost",
        "/lingxi/v1/memories/export?agentId=..%2FagentA",
    ] {
        let (status, error) = server.request("GET", path, None).await;
        assert!(status == 400 || status == 404, "{error}");
        assert!(error["error"].is_string());
        assert_eq!(facts(&server, "agentA").len(), 2);
    }
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_compiled_three_sections_live_legacy_and_invalid_no_write() {
    let server = start_server("compiled-original").await;
    let dir = memory_dir(&server, "agentA");
    std::fs::write(dir.join("editable-facts.md"), "旧版迁移事实\n").unwrap();
    let (status, first) = server
        .request("GET", "/lingxi/v1/memories/compiled?agentId=agentA", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(first["sections"]["facts"], "旧版迁移事实");
    assert!(!dir.join("editable-facts.md").exists());
    assert!(first["content"].as_str().unwrap().contains("旧版迁移事实"));
    assert_eq!(
        std::fs::read_to_string(dir.join("facts.md")).unwrap(),
        "旧版迁移事实\n"
    );
    for (field, content) in [
        ("facts", "归一事实"),
        ("today", "当天记忆"),
        ("longterm", "长期记忆"),
    ] {
        let path = format!("/lingxi/v1/memories/compiled/{field}?agentId=agentA");
        let mut payload = json!({});
        payload[field] = json!(format!("  {content}  \n"));
        let (status, result) = server.request("PUT", &path, Some(payload)).await;
        assert_eq!(status, 200, "{result}");
        assert_eq!(result[field], content);
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{field}.md"))).unwrap(),
            format!("{content}\n")
        );
        let (status, live) = server
            .request("GET", "/lingxi/v1/memories/compiled?agentId=agentA", None)
            .await;
        assert_eq!(status, 200);
        assert_eq!(live["sections"][field], content);
        assert_eq!(
            std::fs::read_to_string(dir.join("memory.md")).unwrap(),
            live["content"].as_str().unwrap()
        );
        let baseline = file_snapshot(&dir);
        let mut bad = json!({});
        bad[field] = json!(123);
        let (status, error) = server.request("PUT", &path, Some(bad.clone())).await;
        assert_eq!(status, 400);
        assert!(error["error"].is_string());
        assert_eq!(file_snapshot(&dir), baseline);
        denied_write_keeps_files(&server, "PUT", &path, bad).await;
    }
    let (status, other) = server
        .request("GET", "/lingxi/v1/memories/compiled?agentId=agentB", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(other["sections"]["facts"], "");
    assert_eq!(other["sections"]["today"], "");
    assert_eq!(other["sections"]["longterm"], "");
    let before = file_snapshot(&dir);
    let (status, error) = server
        .request(
            "GET",
            "/lingxi/v1/memories/compiled?agentId=..%2FagentA",
            None,
        )
        .await;
    assert!(status == 400 || status == 404, "{error}");
    assert!(error["error"].is_string());
    assert_eq!(file_snapshot(&dir), before);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_week_days_list_edit_sync_and_rejected_dates() {
    let server = start_server("week-original").await;
    let dir = memory_dir(&server, "agentA");
    std::fs::create_dir_all(dir.join("daily")).unwrap();
    std::fs::write(dir.join("daily/2026-01-01.md"), "首日旧正文\n").unwrap();
    std::fs::write(dir.join("daily/2026-01-02.md"), "第二日保留\n").unwrap();
    let (status, days) = server
        .request(
            "GET",
            "/lingxi/v1/memories/compiled/week/days?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        days["days"],
        json!([{"date":"2026-01-01","body":"首日旧正文"},{"date":"2026-01-02","body":"第二日保留"}])
    );
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/memories/compiled/week/days/2026-01-01?agentId=agentA",
            Some(json!({"body":"  新日正文  "})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["body"], "新日正文");
    assert_eq!(
        std::fs::read_to_string(dir.join("daily/2026-01-01.md")).unwrap(),
        "新日正文\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("daily/2026-01-02.md")).unwrap(),
        "第二日保留\n"
    );
    assert!(std::fs::read_to_string(dir.join("week.md"))
        .unwrap()
        .contains("新日正文"));
    assert!(std::fs::read_to_string(dir.join("memory.md"))
        .unwrap()
        .contains("新日正文"));
    let before = file_snapshot(&dir);
    for (date, payload, want) in [
        ("2026-01-03", json!({"body":"不存在"}), 404),
        ("bad", json!({"body":"非法日期"}), 400),
        ("2026-01-01", json!({"body":true}), 400),
    ] {
        let (status, error) = server
            .request(
                "PUT",
                &format!("/lingxi/v1/memories/compiled/week/days/{date}?agentId=agentA"),
                Some(payload),
            )
            .await;
        assert_eq!(status, want, "{error}");
        assert!(error["error"].is_string());
        assert_eq!(file_snapshot(&dir), before);
    }
    denied_write_keeps_files(
        &server,
        "PUT",
        "/lingxi/v1/memories/compiled/week/days/2026-01-01?agentId=agentA",
        json!({"body":"禁止写"}),
    )
    .await;
    let (status, other) = server
        .request(
            "GET",
            "/lingxi/v1/memories/compiled/week/days?agentId=agentB",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(other["days"], json!([]));
    let (status, error) = server
        .request(
            "GET",
            "/lingxi/v1/memories/compiled/week/days?agentId=ghost",
            None,
        )
        .await;
    assert_eq!(status, 404);
    assert!(error["error"].is_string());
    assert_eq!(file_snapshot(&dir), before);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_clear_compiled_retains_raw_and_all_clear_removes_facts() {
    let server = start_server("clear-original").await;
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/memories/import?agentId=agentA",
            Some(json!({"facts":[{"fact":"原始事实保留"}]})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/memories/compiled/facts?agentId=agentA",
            Some(json!({"facts":"编译事实清除"})),
        )
        .await;
    assert_eq!(status, 200);
    denied_write_keeps_files(
        &server,
        "DELETE",
        "/lingxi/v1/memories/compiled?agentId=agentA",
        json!({}),
    )
    .await;
    let (status, result) = server
        .request(
            "DELETE",
            "/lingxi/v1/memories/compiled?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    assert_eq!(facts(&server, "agentA").len(), 1);
    let dir = memory_dir(&server, "agentA");
    for file in lingxi_adapters::storage::agent_memory::COMPILED_FILES {
        assert_eq!(std::fs::read_to_string(dir.join(file)).unwrap(), "");
    }
    let reset: Value =
        serde_json::from_slice(&std::fs::read(dir.join("reset.json")).unwrap()).unwrap();
    assert!(reset["compiledResetAt"].is_string());
    let (status, result) = server
        .request("DELETE", "/lingxi/v1/memories?agentId=agentA", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    assert!(facts(&server, "agentA").is_empty());
    let before = file_snapshot(&dir);
    let (status, error) = server
        .request("DELETE", "/lingxi/v1/memories?agentId=ghost", None)
        .await;
    assert_eq!(status, 404);
    assert!(error["error"].is_string());
    assert_eq!(file_snapshot(&dir), before);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_tenet_http_states_duplicate_limits_and_pinned_visibility() {
    let server = start_server("tenets-original").await;
    let service = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let store = service.tenets_store("agentA").unwrap();
    let (pending, _, _) = store
        .add_proposal("待审原则", None, None, "pending-original".into())
        .unwrap();
    let (rejected, _, _) = store
        .add_proposal("拒绝原则", None, None, "reject-original".into())
        .unwrap();
    store.decide(&rejected.id, false).unwrap();
    let (status, result) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/tenets",
            Some(json!({"content":"用户原则","priority":"critical"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["tenet"]["status"], "active");
    assert_eq!(result["duplicate"], false);
    let (status, duplicate) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/tenets",
            Some(json!({"content":"用户原则"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["tenet"]["id"], result["tenet"]["id"]);
    let (status, list) = server
        .request("GET", "/lingxi/v1/agents/agentA/tenets", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(list["tenets"].as_array().unwrap().len(), 3);
    assert!(list["tenets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == pending.id && t["status"] == "pending"));
    assert!(list["tenets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == rejected.id && t["status"] == "rejected"));
    let (status, pins) = server
        .request("GET", "/lingxi/v1/agents/agentA/pinned", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(pins["pins"], json!(["用户原则"]));
    let (status, pins_b) = server
        .request("GET", "/lingxi/v1/agents/agentB/pinned", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(pins_b["pins"], json!([]));
    let before = std::fs::read(store.file_path()).unwrap();
    for payload in [json!({"content":"  "}), json!({"content":123})] {
        let (status, error) = server
            .request("POST", "/lingxi/v1/agents/agentA/tenets", Some(payload))
            .await;
        assert_eq!(status, 400);
        assert!(error["error"].is_string());
        assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    }
    let (status, error) = server
        .request("GET", "/lingxi/v1/agents/ghost/tenets", None)
        .await;
    assert_eq!(status, 404);
    assert!(error["error"].is_string());
    denied_write_keeps_files(
        &server,
        "POST",
        "/lingxi/v1/agents/agentA/tenets",
        json!({"content":"禁止原则"}),
    )
    .await;
    for i in 1..lingxi_kernel::memory::MAX_ACTIVE_USER_TENETS {
        store
            .add_direct(&format!("容量原则{i}"), None, format!("capacity-{i}"))
            .unwrap();
    }
    let before = std::fs::read(store.file_path()).unwrap();
    let (status, error) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/tenets",
            Some(json!({"content":"超出容量"})),
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(error["code"], "TENET_LIMIT_REACHED");
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_tenet_decisions_and_pinned_replacement_preserve_nonactive() {
    let server = start_server("decide-original").await;
    let service = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let store = service.tenets_store("agentA").unwrap();
    let (yes, _, _) = store
        .add_proposal("批准原则", None, None, "yes-original".into())
        .unwrap();
    let (no, _, _) = store
        .add_proposal("拒绝原则", None, None, "no-original".into())
        .unwrap();
    let (pending, _, _) = store
        .add_proposal("保留待审", None, None, "pending-original".into())
        .unwrap();
    let (status, result) = server
        .request(
            "POST",
            &format!("/lingxi/v1/agents/agentA/tenets/{}/decide", yes.id),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["tenet"]["status"], "active");
    let (status, result) = server
        .request(
            "POST",
            &format!("/lingxi/v1/agents/agentA/tenets/{}/decide", no.id),
            Some(json!({"approve":false})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["tenet"]["status"], "rejected");
    assert_eq!(
        store
            .active()
            .iter()
            .map(|t| t.content.as_str())
            .collect::<Vec<_>>(),
        ["批准原则"]
    );
    let before = std::fs::read(store.file_path()).unwrap();
    let (status, error) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/tenets/no-such/decide",
            Some(json!({"approve":true})),
        )
        .await;
    assert_eq!(status, 404);
    assert!(error["error"].is_string());
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    denied_write_keeps_files(
        &server,
        "POST",
        &format!("/lingxi/v1/agents/agentA/tenets/{}/decide", pending.id),
        json!({"approve":true}),
    )
    .await;
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/pinned",
            Some(json!({"pins":["新原则","新原则"]})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["pins"], json!(["新原则"]));
    assert_eq!(store.pending().len(), 1);
    assert_eq!(store.pending()[0].id, pending.id);
    assert!(store
        .list()
        .tenets
        .iter()
        .any(|t| t.id == no.id && t.status.as_str() == "rejected"));
    assert!(!store.list().tenets.iter().any(|t| t.id == yes.id));
    assert_eq!(store.active()[0].source.as_str(), "user_direct");
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/pinned",
            Some(json!({"pins":[]})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["pins"], json!([]));
    assert_eq!(store.pending().len(), 1);
    let before = std::fs::read(store.file_path()).unwrap();
    let (status, error) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/pinned",
            Some(json!({"pins":"bad"})),
        )
        .await;
    assert_eq!(status, 400);
    assert!(error["error"].is_string());
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_experience_read_sync_paused_and_invalid_preserve_state() {
    let server = start_server("experience-original").await;
    let service = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let store = service.experience_store("agentA").unwrap();
    let (status, error) = server
        .request("GET", "/lingxi/v1/agents/agentA/experience", None)
        .await;
    assert_eq!(status, 403);
    assert!(error["error"].is_string());
    let (status, _) = server
        .request(
            "PATCH",
            "/lingxi/v1/agents/agentA/config",
            Some(json!({"experience":{"enabled":true}})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, empty) = server
        .request("GET", "/lingxi/v1/agents/agentA/experience", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(empty["content"], "");
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/experience",
            Some(json!({"content":"# 用户认可的做法\n1. 第一类\n# 用户不认可的做法\n1. 第二类"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    let (status, merged) = server
        .request("GET", "/lingxi/v1/agents/agentA/experience", None)
        .await;
    assert_eq!(status, 200);
    assert!(merged["content"].as_str().unwrap().contains("1. 第一类"));
    assert!(merged["content"].as_str().unwrap().contains("1. 第二类"));
    assert!(store.index_text().contains("用户认可的做法"));
    assert!(store.index_text().contains("用户不认可的做法"));
    let before = file_snapshot(&memory_dir(&server, "agentA"));
    for payload in [
        json!({"content":123}),
        json!({"content":"# ../invalid\n破坏内容"}),
    ] {
        let (status, error) = server
            .request("PUT", "/lingxi/v1/agents/agentA/experience", Some(payload))
            .await;
        assert_eq!(status, 400);
        assert!(error["error"].is_string());
        assert_eq!(file_snapshot(&memory_dir(&server, "agentA")), before);
    }
    denied_write_keeps_files(
        &server,
        "PUT",
        "/lingxi/v1/agents/agentA/experience",
        json!({"content":"# 用户认可的做法\n禁止"}),
    )
    .await;
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentA/experience",
            Some(json!({"content":""})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(store.read_merged().0, "");
    let (status, error) = server
        .request(
            "PUT",
            "/lingxi/v1/agents/agentB/experience",
            Some(json!({"content":"# 用户认可的做法\n不能写"})),
        )
        .await;
    assert_eq!(status, 403);
    assert!(error["error"].is_string());
    assert_eq!(
        service.experience_store("agentB").unwrap().read_merged().0,
        ""
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_feedback_paused_up_down_duplicate_excerpt_and_agent_isolation() {
    let server = start_server("feedback-original").await;
    let service = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    for (rating, category) in [("up", "用户认可的做法"), ("down", "用户不认可的做法")]
    {
        let payload = json!({"rating":rating,"excerpt":"反馈正文","sessionId":"session-12345"});
        let (status, result) = server
            .request(
                "POST",
                "/lingxi/v1/agents/agentA/experience/feedback",
                Some(payload.clone()),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(result["added"], true);
        assert_eq!(result["duplicate"], false);
        assert_eq!(result["category"], category);
        assert_eq!(result["experienceEnabled"], false);
        let (status, duplicate) = server
            .request(
                "POST",
                "/lingxi/v1/agents/agentA/experience/feedback",
                Some(payload),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(duplicate["added"], false);
        assert_eq!(duplicate["duplicate"], true);
    }
    let long = "甲".repeat(250);
    let (status, result) = server
        .request(
            "POST",
            "/lingxi/v1/agents/agentA/experience/feedback",
            Some(json!({"rating":"up","excerpt":long})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["added"], true);
    let store = service.experience_store("agentA").unwrap();
    let merged = store.read_merged().0;
    assert!(merged.contains(&"甲".repeat(200)));
    assert!(!merged.contains(&"甲".repeat(201)));
    assert!(merged.contains("反馈正文"));
    assert_eq!(
        service.experience_store("agentB").unwrap().read_merged().0,
        ""
    );
    let before = file_snapshot(&memory_dir(&server, "agentA"));
    for payload in [
        json!({"rating":"sideways","excerpt":"x"}),
        json!({"rating":"up","excerpt":"  "}),
        json!({"rating":"up"}),
    ] {
        let (status, error) = server
            .request(
                "POST",
                "/lingxi/v1/agents/agentA/experience/feedback",
                Some(payload),
            )
            .await;
        assert_eq!(status, 400);
        assert!(error["error"].is_string());
        assert_eq!(file_snapshot(&memory_dir(&server, "agentA")), before);
    }
    denied_write_keeps_files(
        &server,
        "POST",
        "/lingxi/v1/agents/agentA/experience/feedback",
        json!({"rating":"up","excerpt":"禁止"}),
    )
    .await;
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_revision_detail_reads_current_live_and_restore_keeps_agent_scope() {
    let server = start_server("revision-original").await;
    let service = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let compiled = service.compiled_store("agentA").unwrap();
    let revisions = service.dream_store("agentA").unwrap();
    compiled.write_facts("所选修订旧事实").unwrap();
    let revision = revisions
        .save("original-run", "manual", "dream", None)
        .unwrap();
    let id = revision["revisionId"].as_str().unwrap();
    compiled.write_facts("当前第一次内容").unwrap();
    let path = format!("/lingxi/v1/memories/dream/revisions/{id}?agentId=agentA");
    let (status, first) = server.request("GET", &path, None).await;
    assert_eq!(status, 200);
    assert_eq!(first["revision"]["before"]["facts"], "所选修订旧事实");
    assert_eq!(first["current"]["facts"], "当前第一次内容");
    compiled.write_facts("当前第二次内容").unwrap();
    let (status, second) = server.request("GET", &path, None).await;
    assert_eq!(status, 200);
    assert_eq!(second["current"]["facts"], "当前第二次内容");
    let before = file_snapshot(&memory_dir(&server, "agentA"));
    let (status, error) = server
        .request(
            "GET",
            "/lingxi/v1/memories/dream/revisions/no-such?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(error["code"], "dream_revision_not_found");
    assert_eq!(file_snapshot(&memory_dir(&server, "agentA")), before);
    let (status, error) = server
        .request(
            "GET",
            &format!("/lingxi/v1/memories/dream/revisions/{id}?agentId=agentB"),
            None,
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(error["code"], "dream_revision_not_found");
    denied_write_keeps_files(
        &server,
        "POST",
        &format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentA"),
        json!({}),
    )
    .await;
    let (status, result) = server
        .request(
            "POST",
            &format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentA"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["restored"], id);
    assert_eq!(compiled.read_sections().unwrap().facts, "所选修订旧事实");
    assert_eq!(
        service
            .compiled_store("agentB")
            .unwrap()
            .read_sections()
            .unwrap()
            .facts,
        ""
    );
    let before = file_snapshot(&memory_dir(&server, "agentA"));
    let (status, error) = server
        .request(
            "POST",
            "/lingxi/v1/memories/dream/revisions/no-such/restore?agentId=agentA",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(error["code"], "dream_revision_not_found");
    assert_eq!(file_snapshot(&memory_dir(&server, "agentA")), before);
    server.stop_and_clean().await;
}

fn original_context_compiler(
    home: &std::path::Path,
) -> lingxi_service::context_compiler::ContextCompilerService {
    use lingxi_service::context_compiler::{
        ContextCompilerService, FileContextMaterialSource, HostFacts,
    };
    let product = home.join("proof-product");
    std::fs::create_dir_all(product.join("yuan")).unwrap();
    std::fs::write(product.join("yuan/lingxi.md"), "你是{{agentName}}。").unwrap();
    std::fs::write(product.join("identity.example.md"), "独立夹具身份").unwrap();
    std::fs::write(product.join("agents.example.md"), "独立夹具规则").unwrap();
    ContextCompilerService::new(std::sync::Arc::new(FileContextMaterialSource::from_home(
        home,
        product,
        HostFacts {
            platform: "synthetic".into(),
            os_type: "Synthetic".into(),
            os_release: "1".into(),
            shell_label: "synthetic".into(),
            workspace: home.to_string_lossy().into(),
        },
    )))
}
struct OriginalMemoryTools {
    gateway: lingxi_service::toolgateway::ToolInvocationGateway,
    memory: std::sync::Arc<lingxi_service::memory::MemoryService>,
}
fn original_memory_tools(home: &std::path::Path) -> OriginalMemoryTools {
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
    use lingxi_service::toolgateway::ToolInvocationGateway;
    use lingxi_service::{
        approval_service, inject::SystemClock, memorytools, skills::SkillService,
    };
    let memory = std::sync::Arc::new(lingxi_service::memory::MemoryService::new(
        home.into(),
        home.join("runtime"),
    ));
    let registry = std::sync::Arc::new(ToolRegistry::new());
    let budget = SchemaBudget::default();
    let gateway = ToolInvocationGateway::new(
        std::sync::Arc::clone(&registry),
        std::sync::Arc::new(approval_service::ApprovalService::new(std::sync::Arc::new(
            SystemClock,
        ))),
        std::sync::Arc::new(SystemClock),
        budget,
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    );
    memorytools::register_memory_tools(
        &registry,
        &gateway,
        std::sync::Arc::clone(&memory),
        std::sync::Arc::new(SkillService::new(home.into(), home.join("proof-product"))),
        "agentA".into(),
        std::sync::Arc::new(SystemClock),
        &budget,
    );
    OriginalMemoryTools { gateway, memory }
}
impl OriginalMemoryTools {
    async fn call(
        &self,
        name: &str,
        args: Value,
        sequence: u32,
    ) -> Result<lingxi_kernel::ports::ToolExecutionResult, String> {
        use lingxi_service::toolgateway::{CallerSurface, InvocationPermissionContext};
        let ctx = lingxi_kernel::RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("sess-original-tools"),
            run_id: lingxi_protocol::RunId::new("run-original-tools"),
            attempt: lingxi_protocol::AttemptId::new("run-original-tools#a1"),
            generation: 1,
        };
        let call = lingxi_protocol::ToolCallId::new(format!("original-{sequence}"));
        let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
            format!("tool:first-party:{name}"),
            args,
            &lingxi_kernel::toolcatalog::SchemaBudget::default(),
        )
        .unwrap();
        let prepared = self
            .gateway
            .prepare_from_request(
                &ctx,
                CallerSurface::UserRun,
                "agentA",
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                &call,
                &request,
            )
            .map_err(|error| format!("{error:?}"))?;
        self.gateway
            .execute_prepared(&ctx, &call, &prepared.handle)
            .await
            .map_err(|error| format!("{error:?}"))
    }
}
fn original_tool_text(result: lingxi_kernel::ports::ToolExecutionResult) -> String {
    let lingxi_kernel::ports::ToolOutcome::Success { result } = result.outcome else {
        panic!("真实工具没有成功：{:?}", result.outcome)
    };
    result
        .content
        .iter()
        .filter_map(|block| match block {
            lingxi_protocol::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn original_pin_and_propose_tools_publish_context_and_preserve_rejections() {
    let server = start_server("tools-context-original").await;
    let compiler = original_context_compiler(&server.home);
    let tools = original_memory_tools(&server.home);
    let pin = original_tool_text(
        tools
            .call(
                "pin_memory",
                json!({"content":"PIN-ORIGINAL-秘密 sk-abcdef1234567890abcdef"}),
                1,
            )
            .await
            .unwrap(),
    );
    assert!(pin.contains("Pinned"));
    let prompt = compiler
        .compiled_for_run("sess-original-tools", "agentA", false, 1)
        .unwrap();
    assert!(prompt.render().contains("PIN-ORIGINAL-秘密"));
    assert!(prompt.render().contains("[REDACTED]"));
    assert!(!prompt.render().contains("sk-abcdef"));
    let other = compiler
        .compiled_for_run("sess-original-tools-b", "agentB", false, 1)
        .unwrap();
    assert!(!other.render().contains("PIN-ORIGINAL-秘密"));
    let store = tools.memory.tenets_store("agentA").unwrap();
    assert_eq!(
        store.active()[0].source,
        lingxi_kernel::memory::TenetSource::UserDirect
    );
    let before = std::fs::read(store.file_path()).unwrap();
    let duplicate = original_tool_text(
        tools
            .call(
                "pin_memory",
                json!({"content":"PIN-ORIGINAL-秘密 sk-abcdef1234567890abcdef"}),
                2,
            )
            .await
            .unwrap(),
    );
    assert!(duplicate.contains("already pinned"));
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    for (sequence, args) in [
        (3, json!({"content":""})),
        (4, json!({"content":"   "})),
        (5, json!({"content":"跨Agent不得写","agentId":"agentB"})),
    ] {
        let result = tools.call("pin_memory", args, sequence).await;
        assert!(result.as_ref().map_or(true, |result| matches!(
            result.outcome,
            lingxi_kernel::ports::ToolOutcome::Failed { .. }
        )));
        assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    }
    let other_before = file_snapshot(&memory_dir(&server, "agentB"));
    let proposed = original_tool_text(
        tools
            .call(
                "tenet_propose",
                json!({"content":"PROPOSE-ORIGINAL-待批","priority":"critical"}),
                6,
            )
            .await
            .unwrap(),
    );
    let pending = store.pending();
    assert_eq!(pending.len(), 1);
    assert!(proposed.contains("pending"));
    assert!(proposed.contains(&pending[0].id));
    let frozen_pending = compiler
        .compiled_for_run("sess-original-tools", "agentA", false, 1)
        .unwrap();
    assert!(!frozen_pending.render().contains("PROPOSE-ORIGINAL-待批"));
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), other_before);
    let before = std::fs::read(store.file_path()).unwrap();
    for (sequence, args) in [
        (7, json!({"content":"PROPOSE-ORIGINAL-待批"})),
        (8, json!({"content":"   "})),
        (9, json!({"content":"不得跨Agent提案","agentId":"agentB"})),
    ] {
        let _result = tools.call("tenet_propose", args, sequence).await;
        assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
        assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), other_before);
    }
    let (status, result) = server
        .request(
            "POST",
            &format!("/lingxi/v1/agents/agentA/tenets/{}/decide", pending[0].id),
            Some(json!({"approve":true})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["tenet"]["status"], "active");
    let approved = compiler
        .compiled_for_run("sess-original-tools", "agentA", false, 1)
        .unwrap();
    assert!(approved.render().contains("PROPOSE-ORIGINAL-待批"));
    assert!(!frozen_pending.render().contains("PROPOSE-ORIGINAL-待批"));
    let text = original_tool_text(
        tools
            .call(
                "tenet_propose",
                json!({"content":"REJECT-ORIGINAL-不得生效"}),
                10,
            )
            .await
            .unwrap(),
    );
    let pending = store.pending();
    assert_eq!(pending.len(), 1);
    assert!(text.contains(&pending[0].id));
    let (status, result) = server
        .request(
            "POST",
            &format!("/lingxi/v1/agents/agentA/tenets/{}/decide", pending[0].id),
            Some(json!({"approve":false})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["tenet"]["status"], "rejected");
    let rejected = compiler
        .compiled_for_run("sess-original-tools", "agentA", false, 1)
        .unwrap();
    assert!(!rejected.render().contains("REJECT-ORIGINAL-不得生效"));
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), other_before);
    for i in 1..lingxi_kernel::memory::MAX_ACTIVE_USER_TENETS {
        store
            .add_direct(&format!("容量填充{i}"), None, format!("tool-capacity-{i}"))
            .unwrap();
    }
    let before = std::fs::read(store.file_path()).unwrap();
    let result = tools
        .call("pin_memory", json!({"content":"超出工具容量不得写"}), 11)
        .await
        .unwrap();
    assert!(matches!(
        result.outcome,
        lingxi_kernel::ports::ToolOutcome::Failed { .. }
    ));
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), other_before);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_tenet_decision_nonpending_capacity_errors_keep_store() {
    let server = start_server("decide-boundary-original").await;
    let service = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let store = service.tenets_store("agentA").unwrap();
    for i in 0..lingxi_kernel::memory::MAX_ACTIVE_TENETS {
        let id = format!("active-model-{i}");
        store
            .add_proposal(&format!("现有模型原则{i}"), None, None, id.clone())
            .unwrap();
        store.decide(&id, true).unwrap();
    }
    store
        .add_proposal("容量受限待批", None, None, "waiting-original".into())
        .unwrap();
    let before = std::fs::read(store.file_path()).unwrap();
    for (id, approve, status, code) in [
        ("active-model-0", true, 409, "TENET_INVALID"),
        ("active-model-0", false, 409, "TENET_INVALID"),
        ("waiting-original", true, 409, "TENET_LIMIT_REACHED"),
        ("no-such", false, 404, "TENET_NOT_FOUND"),
    ] {
        let (actual, error) = server
            .request(
                "POST",
                &format!("/lingxi/v1/agents/agentA/tenets/{id}/decide"),
                Some(json!({"approve":approve})),
            )
            .await;
        assert_eq!(actual, status, "{error}");
        assert_eq!(error["code"], code);
        assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    }
    assert!(service
        .tenets_store("agentB")
        .unwrap()
        .list()
        .tenets
        .is_empty());
    server.stop_and_clean().await;
}

struct OriginalInstallRiskGuard;
impl lingxi_service::skills::SkillGuardPort for OriginalInstallRiskGuard {
    fn review<'a>(
        &'a self,
        _text: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = lingxi_service::skills::SkillReviewOutcome>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            lingxi_service::skills::SkillReviewOutcome::Suspicious(
                "合成外部审核结果：需用户确认".into(),
            )
        })
    }
}
struct OriginalInstallSafeGuard;
impl lingxi_service::skills::SkillGuardPort for OriginalInstallSafeGuard {
    fn review<'a>(
        &'a self,
        _text: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = lingxi_service::skills::SkillReviewOutcome>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { lingxi_service::skills::SkillReviewOutcome::Safe })
    }
}
fn original_install_gateway(
    home: &std::path::Path,
    skills: std::sync::Arc<lingxi_service::skills::SkillService>,
) -> OriginalMemoryTools {
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
    use lingxi_service::inject::SystemClock;
    let memory = std::sync::Arc::new(lingxi_service::memory::MemoryService::new(
        home.into(),
        home.join("runtime"),
    ));
    let registry = std::sync::Arc::new(ToolRegistry::new());
    let budget = SchemaBudget::default();
    let gateway = lingxi_service::toolgateway::ToolInvocationGateway::new(
        std::sync::Arc::clone(&registry),
        std::sync::Arc::new(lingxi_service::approval_service::ApprovalService::new(
            std::sync::Arc::new(SystemClock),
        )),
        std::sync::Arc::new(SystemClock),
        budget,
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    );
    lingxi_service::memorytools::register_memory_tools(
        &registry,
        &gateway,
        std::sync::Arc::clone(&memory),
        skills,
        "agentA".into(),
        std::sync::Arc::new(SystemClock),
        &budget,
    );
    OriginalMemoryTools { gateway, memory }
}
fn original_install_package(home: &std::path::Path, name: &str) -> PathBuf {
    let path = home.join("incoming").join(name);
    std::fs::create_dir_all(path.join("examples")).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 原安装工具完整包\n---\n完整技能正文"),
    )
    .unwrap();
    std::fs::write(
        path.join("examples/guide.txt"),
        "必须一同安装的原始配套文件",
    )
    .unwrap();
    path
}

#[tokio::test]
async fn original_manual_skill_install_enables_only_requested_agent_and_refreshes_runtime() {
    let server = start_server("manual-install-agent").await;
    let skills = lingxi_service::skills::SkillService::new(
        server.home.clone(),
        server.home.join("proof-product"),
    );
    let b_before = std::fs::read(server.home.join("agents/agentB/config.yaml")).unwrap();
    assert!(!skills
        .runtime_skill_catalog("agentA")
        .unwrap()
        .iter()
        .any(|skill| skill.name == "manual-original"));
    let package = original_install_package(&server.home, "manual-original");
    let installed = server
        .request(
            "POST",
            "/lingxi/v1/skills/install?agentId=agentA",
            Some(json!({"path": package})),
        )
        .await;
    assert_eq!(installed.0, 200, "{installed:?}");
    assert_eq!(installed.1["ok"], true);
    assert_eq!(installed.1["skill"]["name"], "manual-original");
    assert_eq!(
        std::fs::read(
            server
                .home
                .join("skills/manual-original/examples/guide.txt")
        )
        .unwrap(),
        std::fs::read(package.join("examples/guide.txt")).unwrap()
    );
    let a_config = std::fs::read_to_string(server.home.join("agents/agentA/config.yaml")).unwrap();
    let (enabled, _) =
        lingxi_adapters::storage::agent_memory::AgentConfigPatch::read_skill_lists(&a_config)
            .unwrap();
    assert!(enabled.contains(&"manual-original".to_string()));
    assert!(skills
        .runtime_skill_catalog("agentA")
        .unwrap()
        .iter()
        .any(|skill| skill.name == "manual-original"));
    assert!(!skills
        .runtime_skill_catalog("agentB")
        .unwrap()
        .iter()
        .any(|skill| skill.name == "manual-original"));
    let runtime = server
        .request("GET", "/lingxi/v1/skills?agentId=agentA&runtime=1", None)
        .await;
    assert_eq!(runtime.0, 200, "{runtime:?}");
    assert!(runtime.1["skills"]
        .as_array()
        .unwrap()
        .iter()
        .any(|skill| { skill["name"] == "manual-original" && skill["enabled"] == true }));
    assert_eq!(
        std::fs::read(server.home.join("agents/agentB/config.yaml")).unwrap(),
        b_before
    );
    let a_before = std::fs::read(server.home.join("agents/agentA/config.yaml")).unwrap();
    for (query, status) in [
        ("agentId=agentA&agentId=agentB", 400),
        ("agentId=missing", 404),
        ("agentId=bad%2Fid", 400),
        ("agentId=", 400),
    ] {
        let refused_package = original_install_package(&server.home, "manual-refused");
        let refused = server
            .request(
                "POST",
                &format!("/lingxi/v1/skills/install?{query}"),
                Some(json!({"path": refused_package})),
            )
            .await;
        assert_eq!(refused.0, status, "{query}: {refused:?}");
        assert!(refused.1.get("error").is_some());
        assert!(!server.home.join("skills/manual-refused").exists());
        assert_eq!(
            std::fs::read(server.home.join("agents/agentA/config.yaml")).unwrap(),
            a_before
        );
        assert_eq!(
            std::fs::read(server.home.join("agents/agentB/config.yaml")).unwrap(),
            b_before
        );
    }
    for path in [
        PathBuf::from("relative-skill-path"),
        server.home.join("missing-skill-package"),
    ] {
        let refused = server
            .request(
                "POST",
                "/lingxi/v1/skills/install?agentId=agentA",
                Some(json!({"path": path})),
            )
            .await;
        assert_eq!(refused.0, 400, "{refused:?}");
        assert!(refused.1.get("error").is_some());
        assert_eq!(
            std::fs::read(server.home.join("agents/agentA/config.yaml")).unwrap(),
            a_before
        );
        assert_eq!(
            std::fs::read(server.home.join("agents/agentB/config.yaml")).unwrap(),
            b_before
        );
    }
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_install_tool_risk_token_complete_package_agent_enable_and_failures() {
    let server = start_server("install-tool-original").await;
    std::fs::create_dir_all(server.home.join("user")).unwrap();
    std::fs::write(server.home.join("user/preferences.json"),json!({"capabilities":{"learn_skills":{"enabled":true,"safety_review":true,"allow_github_fetch":false}}}).to_string()).unwrap();
    let skills = std::sync::Arc::new(lingxi_service::skills::SkillService::new(
        server.home.clone(),
        server.home.join("proof-product"),
    ));
    skills.set_guard(std::sync::Arc::new(OriginalInstallRiskGuard));
    let tools = original_install_gateway(&server.home, skills.clone());
    let package = original_install_package(&server.home, "review-original");
    let installed = server.home.join("skills/review-original");
    let b_before = file_snapshot(&memory_dir(&server, "agentB"));
    let a_config = server.home.join("agents/agentA/config.yaml");
    let a_before = std::fs::read(&a_config).unwrap();
    let first = original_tool_text(
        tools
            .call("install_skill", json!({"local_path":package}), 100)
            .await
            .unwrap(),
    );
    assert!(first.contains("Safety review FAILED"));
    assert!(!installed.exists());
    assert_eq!(std::fs::read(&a_config).unwrap(), a_before);
    let token = first
        .split("risk_confirmation_token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .trim_end_matches('.')
        .to_string();
    assert!(token.starts_with("risk_"));
    let missing = original_tool_text(
        tools
            .call(
                "install_skill",
                json!({"local_path":package,"risk_accepted":true}),
                101,
            )
            .await
            .unwrap(),
    );
    assert!(missing.contains("Safety review FAILED"));
    assert!(!installed.exists());
    assert_eq!(std::fs::read(&a_config).unwrap(), a_before);
    let accepted = original_tool_text(
        tools
            .call(
                "install_skill",
                json!({"local_path":package,"risk_accepted":true,"risk_confirmation_token":token}),
                102,
            )
            .await
            .unwrap(),
    );
    assert!(accepted.contains("Skill installed: review-original"));
    assert!(accepted.contains("after explicit user confirmation"));
    assert!(std::fs::read_to_string(installed.join("SKILL.md"))
        .unwrap()
        .contains("完整技能正文"));
    assert_eq!(
        std::fs::read_to_string(installed.join("examples/guide.txt")).unwrap(),
        "必须一同安装的原始配套文件"
    );
    let enabled = lingxi_adapters::storage::agent_memory::AgentConfigPatch::read_skill_lists(
        &std::fs::read_to_string(&a_config).unwrap(),
    )
    .unwrap()
    .0;
    assert!(enabled.contains(&"review-original".into()));
    assert!(skills
        .runtime_skill_catalog("agentA")
        .unwrap()
        .iter()
        .any(|s| s.name == "review-original"));
    assert!(!skills
        .runtime_skill_catalog("agentB")
        .unwrap()
        .iter()
        .any(|s| s.name == "review-original"));
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), b_before);
    let pool_before = file_snapshot(&server.home.join("skills"));
    let config_before = std::fs::read(&a_config).unwrap();
    let replay = original_tool_text(
        tools
            .call(
                "install_skill",
                json!({"local_path":package,"risk_accepted":true,"risk_confirmation_token":token}),
                103,
            )
            .await
            .unwrap(),
    );
    assert!(replay.contains("Safety review FAILED"));
    assert_eq!(file_snapshot(&server.home.join("skills")), pool_before);
    assert_eq!(std::fs::read(&a_config).unwrap(), config_before);
    for (sequence, path) in [
        (104, PathBuf::from("relative-source")),
        (105, server.home.join("no-such-package")),
    ] {
        let result = tools
            .call("install_skill", json!({"local_path":path}), sequence)
            .await
            .unwrap();
        assert!(matches!(
            result.outcome,
            lingxi_kernel::ports::ToolOutcome::Failed { .. }
        ));
        assert_eq!(file_snapshot(&server.home.join("skills")), pool_before);
        assert_eq!(std::fs::read(&a_config).unwrap(), config_before);
    }
    #[cfg(unix)]
    {
        let escape = original_install_package(&server.home, "escape-original");
        std::os::unix::fs::symlink(
            server.home.join("agents/agentB"),
            escape.join("outside-link"),
        )
        .unwrap();
        let result = tools
            .call("install_skill", json!({"local_path":escape}), 106)
            .await
            .unwrap();
        assert!(matches!(
            result.outcome,
            lingxi_kernel::ports::ToolOutcome::Failed { .. }
        ));
        assert!(!server.home.join("skills/escape-original").exists());
        assert_eq!(file_snapshot(&server.home.join("skills")), pool_before);
        assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), b_before);
    }
    skills.set_guard(std::sync::Arc::new(OriginalInstallSafeGuard));
    let failed = original_install_package(&server.home, "failed-original");
    std::fs::write(
        server.home.join("skills/failed-original"),
        "既有不可覆盖文件",
    )
    .unwrap();
    let pool_before = file_snapshot(&server.home.join("skills"));
    let result = tools
        .call("install_skill", json!({"local_path":failed}), 107)
        .await
        .unwrap();
    assert!(matches!(
        result.outcome,
        lingxi_kernel::ports::ToolOutcome::Failed { .. }
    ));
    assert!(!server
        .home
        .join("skills/failed-original/SKILL.md")
        .is_file());
    assert_eq!(file_snapshot(&server.home.join("skills")), pool_before);
    assert_eq!(std::fs::read(&a_config).unwrap(), config_before);
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), b_before);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_unpin_exact_and_multi_match_keep_other_agent_and_nonactive() {
    let server = start_server("unpin-original").await;
    let tools = original_memory_tools(&server.home);
    let a = tools.memory.tenets_store("agentA").unwrap();
    let b = tools.memory.tenets_store("agentB").unwrap();
    for (id, text) in [
        ("exact-original", "精确原条目"),
        ("multi-1", "KEYWORD 原第一项"),
        ("multi-2", "keyword 原第二项"),
        ("keep-original", "不匹配保留"),
    ] {
        a.add_direct(text, None, id.into()).unwrap();
    }
    a.add_proposal(
        "keyword 待批不得删除",
        None,
        None,
        "pending-original".into(),
    )
    .unwrap();
    b.add_direct("keyword 其他助手不得删除", None, "b-original".into())
        .unwrap();
    let b_before = std::fs::read(b.file_path()).unwrap();
    let removed = original_tool_text(
        tools
            .call("unpin_memory", json!({"id":"exact-original"}), 200)
            .await
            .unwrap(),
    );
    assert!(removed.contains("Removed 1 pinned item(s): 精确原条目"));
    assert_eq!(a.active().len(), 3);
    assert!(!a.active().iter().any(|t| t.id == "exact-original"));
    let removed = original_tool_text(
        tools
            .call("unpin_memory", json!({"keyword":"KeYwOrD"}), 201)
            .await
            .unwrap(),
    );
    assert!(removed.contains("Removed 2 pinned item(s):"));
    assert!(removed.contains("KEYWORD 原第一项"));
    assert!(removed.contains("keyword 原第二项"));
    assert_eq!(a.active().len(), 1);
    assert_eq!(a.active()[0].id, "keep-original");
    assert_eq!(a.pending()[0].id, "pending-original");
    assert_eq!(std::fs::read(b.file_path()).unwrap(), b_before);
    let a_before = std::fs::read(a.file_path()).unwrap();
    for (sequence, args) in [
        (202, json!({"id":"no-match"})),
        (203, json!({"keyword":"   "})),
    ] {
        let text = original_tool_text(tools.call("unpin_memory", args, sequence).await.unwrap());
        assert!(text.contains("matched") || text.contains("Nothing matched"));
        assert_eq!(std::fs::read(a.file_path()).unwrap(), a_before);
        assert_eq!(std::fs::read(b.file_path()).unwrap(), b_before);
    }
    // 原removedCount字段和review确认链当前未实现，只保留实际文本计数的局部证明。
    server.stop_and_clean().await;
}

async fn original_notes_call(
    tools: &OriginalMemoryTools,
    session: &str,
    sequence: u32,
    args: Value,
) -> lingxi_kernel::ports::ToolExecutionResult {
    use lingxi_service::toolgateway::{CallerSurface, InvocationPermissionContext};
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(session),
        run_id: lingxi_protocol::RunId::new(format!("notes-original-{sequence}")),
        attempt: lingxi_protocol::AttemptId::new(format!("notes-original-{sequence}#a1")),
        generation: 1,
    };
    let call = lingxi_protocol::ToolCallId::new(format!("notes-original-{sequence}"));
    let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        "tool:first-party:context_notes",
        args,
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .unwrap();
    let prepared = tools
        .gateway
        .prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agentA",
            InvocationPermissionContext::UserSession {
                mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
            },
            &call,
            &request,
        )
        .unwrap();
    tools
        .gateway
        .execute_prepared(&ctx, &call, &prepared.handle)
        .await
        .unwrap()
}

#[tokio::test]
async fn original_context_notes_append_reopen_and_oversize_preserve_old_value() {
    let server = start_server("notes-tool-original").await;
    let tools = original_memory_tools(&server.home);
    let session = "sess-original-notes-reopen";
    let written = original_tool_text(
        original_notes_call(
            &tools,
            session,
            210,
            json!({"action":"write","text":"原第一块"}),
        )
        .await,
    );
    assert!(written.contains("context notes write"));
    let appended = original_tool_text(
        original_notes_call(
            &tools,
            session,
            211,
            json!({"action":"append","text":"原第二块"}),
        )
        .await,
    );
    assert!(appended.contains("context notes append"));
    let text = original_tool_text(
        original_notes_call(&tools, session, 212, json!({"action":"read"})).await,
    );
    assert_eq!(text, "context notes:\n原第一块\n原第二块");
    assert_eq!(
        tools.memory.context_notes().read(session).unwrap(),
        "原第一块\n原第二块"
    );
    drop(tools);
    // 重建工具及存储对象，用同会话标识的新Run读取真实已落盘笔记。
    let reopened = original_memory_tools(&server.home);
    let text = original_tool_text(
        original_notes_call(&reopened, session, 213, json!({"action":"read"})).await,
    );
    assert_eq!(text, "context notes:\n原第一块\n原第二块");
    let other = original_tool_text(
        original_notes_call(
            &reopened,
            "sess-other-original-notes",
            214,
            json!({"action":"read"}),
        )
        .await,
    );
    assert_eq!(other, "(no context notes yet)");
    let failed = original_notes_call(
        &reopened,
        session,
        215,
        json!({"action":"append","text":"x".repeat(17*1024)}),
    )
    .await;
    assert!(matches!(
        failed.outcome,
        lingxi_kernel::ports::ToolOutcome::Failed { .. }
    ));
    let text = original_tool_text(
        original_notes_call(&reopened, session, 216, json!({"action":"read"})).await,
    );
    assert_eq!(text, "context notes:\n原第一块\n原第二块");
    let empty = match original_notes_call(
        &reopened,
        session,
        217,
        json!({"action":"write","text":"   "}),
    )
    .await
    .outcome
    {
        lingxi_kernel::ports::ToolOutcome::Failed { error } => {
            assert_eq!(error.code, lingxi_protocol::ErrorCode::InvalidMessage);
            error.message
        }
        other => panic!("空写入必须明确失败：{other:?}"),
    };
    assert!(empty.contains("refusing to write empty notes"));
    assert_eq!(
        reopened.memory.context_notes().read(session).unwrap(),
        "原第一块\n原第二块"
    );
    for (sequence, args) in [
        (218, json!({"action":"append","text":"\n "})),
        (219, json!({"action":"write"})),
    ] {
        let rejected = original_notes_call(&reopened, session, sequence, args).await;
        assert!(matches!(
            rejected.outcome,
            lingxi_kernel::ports::ToolOutcome::Failed { .. }
        ));
        assert_eq!(
            reopened.memory.context_notes().read(session).unwrap(),
            "原第一块\n原第二块"
        );
    }
    let no_session = original_notes_call(
        &reopened,
        "",
        220,
        json!({"action":"write","text":"不得落盘"}),
    )
    .await;
    assert!(matches!(
        no_session.outcome,
        lingxi_kernel::ports::ToolOutcome::Failed { .. }
    ));
    assert_eq!(
        reopened.memory.context_notes().read(session).unwrap(),
        "原第一块\n原第二块"
    );
    let cap = lingxi_adapters::storage::agent_memory::CONTEXT_NOTES_MAX_BYTES;
    let exact = "x".repeat(cap);
    let exact_result = original_notes_call(
        &reopened,
        session,
        221,
        json!({"action":"write","text":exact}),
    )
    .await;
    assert!(matches!(
        exact_result.outcome,
        lingxi_kernel::ports::ToolOutcome::Success { .. }
    ));
    assert_eq!(
        reopened.memory.context_notes().read(session).unwrap().len(),
        cap
    );
    let overflow = original_notes_call(
        &reopened,
        session,
        222,
        json!({"action":"append","text":"y"}),
    )
    .await;
    assert!(matches!(
        overflow.outcome,
        lingxi_kernel::ports::ToolOutcome::Failed { .. }
    ));
    assert_eq!(
        reopened.memory.context_notes().read(session).unwrap(),
        "x".repeat(cap)
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_nonempty_revision_lists_are_agent_scoped_and_read_only() {
    let server = start_server("revision-list-original").await;
    let memory = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let a = memory.dream_store("agentA").unwrap();
    let b = memory.dream_store("agentB").unwrap();
    memory
        .compiled_store("agentA")
        .unwrap()
        .write_facts("A独有修订内容")
        .unwrap();
    memory
        .compiled_store("agentB")
        .unwrap()
        .write_facts("B独有修订内容")
        .unwrap();
    let first = a
        .save("run-original-list-a1", "manual", "dream", None)
        .unwrap();
    let second = a
        .save("run-original-list-a2", "manual", "dream", None)
        .unwrap();
    let other = b
        .save("run-original-list-b1", "manual", "dream", None)
        .unwrap();
    let before_a = file_snapshot(&memory_dir(&server, "agentA"));
    let before_b = file_snapshot(&memory_dir(&server, "agentB"));
    let (status, result) = server
        .request(
            "GET",
            "/lingxi/v1/memories/dream/revisions?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["agentId"], "agentA");
    let actual = result["revisions"].as_array().unwrap();
    assert_eq!(actual.len(), 2);
    assert!(actual
        .iter()
        .any(|r| r["revisionId"] == first["revisionId"]));
    assert!(actual
        .iter()
        .any(|r| r["revisionId"] == second["revisionId"]));
    assert!(!actual
        .iter()
        .any(|r| r["revisionId"] == other["revisionId"]));
    assert!(actual
        .iter()
        .all(|r| r.get("before").is_none() && r["sectionChars"]["facts"].as_u64().unwrap() > 0));
    let (status, result) = server
        .request(
            "GET",
            "/lingxi/v1/memories/dream/revisions?agentId=agentB",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["agentId"], "agentB");
    assert_eq!(result["revisions"].as_array().unwrap().len(), 1);
    assert_eq!(result["revisions"][0]["revisionId"], other["revisionId"]);
    assert_eq!(file_snapshot(&memory_dir(&server, "agentA")), before_a);
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), before_b);
    // 当前列表不依赖ticker而返回200，不能冒充原ticker缺席503边界已通过。
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_restore_real_io_failure_is_500_and_keeps_all_agent_files() {
    let server = start_server("restore-io-original").await;
    let memory = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let compiled = memory.compiled_store("agentA").unwrap();
    compiled.write_facts("可恢复的旧正文").unwrap();
    let revision = memory
        .dream_store("agentA")
        .unwrap()
        .save("restore-io-original", "manual", "dream", None)
        .unwrap();
    let id = revision["revisionId"].as_str().unwrap();
    compiled.write_facts("I/O失败必须保留的当前正文").unwrap();
    memory
        .compiled_store("agentB")
        .unwrap()
        .write_facts("B独有正文必须保留")
        .unwrap();
    // 把选中修订的文件位置变成目录，触发生产read_to_string的真实I/O错误。
    let path = memory_dir(&server, "agentA")
        .join("dream/revisions")
        .join(format!("{id}.json"));
    let original_revision = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let before_a = file_snapshot(&memory_dir(&server, "agentA"));
    let before_b = file_snapshot(&memory_dir(&server, "agentB"));
    let (status, error) = server
        .request(
            "POST",
            &format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentA"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 500, "{error}");
    assert_eq!(error["code"], "dream_restore_failed");
    assert!(error["error"].as_str().is_some_and(|text| !text.is_empty()));
    assert_eq!(
        compiled.read_sections().unwrap().facts,
        "I/O失败必须保留的当前正文"
    );
    assert_eq!(file_snapshot(&memory_dir(&server, "agentA")), before_a);
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), before_b);
    assert!(path.is_dir());
    // 只恢复本测试的合成资料；忙状态/内容冲突另有专属检查，不能用500代替。
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, original_revision).unwrap();
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_restore_detail_hash_refuses_concurrent_edits_and_preserves_agent_scope() {
    let server = start_server("restore-condition-original").await;
    let memory = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let compiled = memory.compiled_store("agentA").unwrap();
    compiled.write_facts("目标原事实").unwrap();
    compiled.write_today("目标原今日").unwrap();
    compiled.write_longterm("目标原长期").unwrap();
    let revision = memory
        .dream_store("agentA")
        .unwrap()
        .save("restore-condition-original", "manual", "dream", None)
        .unwrap();
    let id = revision["revisionId"].as_str().unwrap();
    compiled.write_facts("确认时当前事实").unwrap();
    compiled.write_today("确认时当前今日").unwrap();
    compiled.write_longterm("确认时当前长期").unwrap();
    memory
        .compiled_store("agentB")
        .unwrap()
        .write_facts("B必须保留的独有正文")
        .unwrap();
    let detail_path = format!("/lingxi/v1/memories/dream/revisions/{id}?agentId=agentA");
    let restore_path = format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentA");
    let (status, detail) = server.request("GET", &detail_path, None).await;
    assert_eq!(status, 200);
    let confirmed = detail["currentHash"].as_str().unwrap();
    assert_eq!(confirmed.len(), 64);
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/memories/compiled/facts?agentId=agentA",
            Some(json!({"facts":"用户确认后新写入，不能被覆盖"})),
        )
        .await;
    assert_eq!(status, 200);
    let before_a = file_snapshot(&memory_dir(&server, "agentA"));
    let before_b = file_snapshot(&memory_dir(&server, "agentB"));
    let (status, error) = server
        .request(
            "POST",
            &restore_path,
            Some(json!({"expectedCurrentHash":confirmed})),
        )
        .await;
    assert_eq!(status, 409, "{error}");
    assert_eq!(error["code"], "dream_memory_changed");
    assert_eq!(file_snapshot(&memory_dir(&server, "agentA")), before_a);
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), before_b);
    assert_eq!(
        compiled.read_sections().unwrap().facts,
        "用户确认后新写入，不能被覆盖"
    );
    for payload in [
        json!({"expectedCurrentHash":false}),
        json!({"expectedCurrentHash":"bad"}),
        json!([]),
    ] {
        assert_eq!(
            server.request("POST", &restore_path, Some(payload)).await.0,
            400
        );
        assert_eq!(file_snapshot(&memory_dir(&server, "agentA")), before_a);
    }
    let (status, fresh) = server.request("GET", &detail_path, None).await;
    assert_eq!(status, 200);
    assert_ne!(fresh["currentHash"], detail["currentHash"]);
    let (status, result) = server
        .request(
            "POST",
            &restore_path,
            Some(json!({"expectedCurrentHash":fresh["currentHash"]})),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["restored"], id);
    let actual = compiled.read_sections().unwrap();
    assert_eq!(actual.facts, "目标原事实");
    assert_eq!(actual.today, "目标原今日");
    assert_eq!(actual.longterm, "目标原长期");
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), before_b);
    // 原空body调用仍兼容，但当前UI恢复实际传详情中的条件。
    assert_eq!(
        server
            .request("POST", &restore_path, Some(json!({})))
            .await
            .0,
        200
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_restore_refuses_real_compilation_tenet_maintenance_dream_and_restore_busy() {
    use lingxi_adapters::storage::agent_memory::{
        memory_operation_test_hook, CompiledMemoryStore, DreamRevisionStore, MemoryOperationKind,
        TenetsStore,
    };
    let server = start_server("restore-busy-original").await;
    let memory = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    for (index, kind) in [
        MemoryOperationKind::Compile,
        MemoryOperationKind::Maintenance,
        MemoryOperationKind::Maintenance,
        MemoryOperationKind::Dream,
        MemoryOperationKind::Restore,
    ]
    .into_iter()
    .enumerate()
    {
        let dir = memory_dir(&server, "agentA");
        let compiled = memory.compiled_store("agentA").unwrap();
        let old = format!("忙检查目标原事实{index}");
        compiled.write_facts(&old).unwrap();
        let revision = memory
            .dream_store("agentA")
            .unwrap()
            .save(&format!("busy-a-{index}"), "manual", "dream", None)
            .unwrap();
        let id = revision["revisionId"].as_str().unwrap().to_string();
        compiled.write_facts("忙操作前的当前事实").unwrap();
        let b_compiled = memory.compiled_store("agentB").unwrap();
        b_compiled.write_facts("B合法恢复目标").unwrap();
        let b_revision = memory
            .dream_store("agentB")
            .unwrap()
            .save(&format!("busy-b-{index}"), "manual", "dream", None)
            .unwrap();
        b_compiled.write_facts("B当前事实").unwrap();
        let before = file_snapshot(&dir);
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let release =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let release_for_hook = release.clone();
        let hook = memory_operation_test_hook(
            &dir,
            std::sync::Arc::new(move |actual| {
                assert_eq!(actual, kind);
                entered_tx.send(actual).unwrap();
                let (lock, condition) = &*release_for_hook;
                let (released, _) = condition
                    .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(5), |released| {
                        !*released
                    })
                    .unwrap();
                assert!(*released, "真实操作等待释放超时");
            }),
        );
        let operation_dir = dir.clone();
        let operation_id = id.clone();
        let operation = tokio::task::spawn_blocking(
            move || -> Result<(), lingxi_adapters::storage::agent_memory::MemoryError> {
                match index {
                    0 => CompiledMemoryStore::new(&operation_dir, true)
                        .write_facts("真正编译写入的新事实")
                        .map(|_| ()),
                    1 => TenetsStore::new(&operation_dir)
                        .add_direct("真正原则维护", None, format!("busy-tenet-{index}"))
                        .map(|_| ()),
                    2 => CompiledMemoryStore::new(&operation_dir, true).reset_and_clear(),
                    3 => DreamRevisionStore::new(&operation_dir)
                        .save("actual-dream-save", "manual", "dream", None)
                        .map(|_| ()),
                    _ => DreamRevisionStore::new(&operation_dir)
                        .restore(&operation_id)
                        .map(|_| ()),
                }
            },
        );
        assert_eq!(
            tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(5)))
                .await
                .unwrap()
                .unwrap(),
            kind
        );
        let path = format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentA");
        let (status, error) = server.request("POST", &path, Some(json!({}))).await;
        assert_eq!(status, 409, "{index}: {error}");
        assert_eq!(
            error["code"],
            if kind == MemoryOperationKind::Dream {
                "dream_already_running"
            } else {
                "dream_memory_busy"
            }
        );
        assert_eq!(file_snapshot(&dir), before);
        assert_eq!(
            server
                .request_token("POST", &path, Some(json!({})), &server.readonly_token)
                .await
                .0,
            403
        );
        assert_eq!(file_snapshot(&dir), before);
        // A的真实操作锁不能扩大到B，B在同一时段仍可合法恢复。
        let b_path = format!(
            "/lingxi/v1/memories/dream/revisions/{}/restore?agentId=agentB",
            b_revision["revisionId"].as_str().unwrap()
        );
        assert_eq!(
            server.request("POST", &b_path, Some(json!({}))).await.0,
            200
        );
        assert_eq!(b_compiled.read_sections().unwrap().facts, "B合法恢复目标");
        assert_eq!(
            server
                .request(
                    "POST",
                    &format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentB"),
                    Some(json!({}))
                )
                .await
                .0,
            404
        );
        {
            let (lock, condition) = &*release;
            *lock.lock().unwrap() = true;
            condition.notify_all();
        }
        operation.await.unwrap().unwrap();
        drop(hook);
        let detail = server
            .request(
                "GET",
                &format!("/lingxi/v1/memories/dream/revisions/{id}?agentId=agentA"),
                None,
            )
            .await;
        assert_eq!(detail.0, 200);
        assert_eq!(
            server
                .request(
                    "POST",
                    &path,
                    Some(json!({"expectedCurrentHash":detail.1["currentHash"]}))
                )
                .await
                .0,
            200
        );
        assert_eq!(compiled.read_sections().unwrap().facts, old);
    }
    server.stop_and_clean().await;
}
