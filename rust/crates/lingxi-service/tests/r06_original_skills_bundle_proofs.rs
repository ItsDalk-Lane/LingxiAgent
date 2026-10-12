//! 原 F-D21 已实现接口逐断言补充：真实技能文件/配置/目录/组合回读；不补未实现功能。
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
    workspace: PathBuf,
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-original-skills-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06-original-skills-ws-{uniq}"));
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
        workspace,
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
fn write_package(root: &std::path::Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(dir.join("examples")).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 原断言专属技能\n---\n完整正文"),
    )
    .unwrap();
    std::fs::write(dir.join("examples/example.txt"), "配套文件原文").unwrap();
    dir
}
fn lists(server: &TestServer, agent: &str) -> (Vec<String>, Vec<String>) {
    let text = std::fs::read_to_string(server.home.join("agents").join(agent).join("config.yaml"))
        .unwrap();
    lingxi_adapters::storage::agent_memory::AgentConfigPatch::read_skill_lists(&text).unwrap()
}
async fn view(server: &TestServer, agent: &str, runtime: bool) -> Value {
    let (status, result) = server
        .request(
            "GET",
            &format!(
                "/lingxi/v1/skills?agentId={agent}&runtime={}",
                if runtime { 1 } else { 0 }
            ),
            None,
        )
        .await;
    assert_eq!(status, 200, "{result}");
    result
}

#[tokio::test]
async fn original_global_manual_install_complete_package_without_agent_enable() {
    let server = start_server("manual-original").await;
    let source = write_package(&server.home.join("incoming"), "global-original");
    let before_a = lists(&server, "agentA");
    let before_b = lists(&server, "agentB");
    let (status, result) = server
        .request(
            "POST",
            "/lingxi/v1/skills/install",
            Some(json!({"path":source})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["skill"]["name"], "global-original");
    let installed = server.home.join("skills/global-original");
    assert!(
        std::fs::read_to_string(installed.join("SKILL.md"))
            .unwrap()
            .contains("完整正文")
    );
    assert_eq!(
        std::fs::read_to_string(installed.join("examples/example.txt")).unwrap(),
        "配套文件原文"
    );
    assert_eq!(lists(&server, "agentA"), before_a);
    assert_eq!(lists(&server, "agentB"), before_b);
    let actual = view(&server, "agentA", false).await;
    let skill = actual["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "global-original")
        .unwrap();
    assert_eq!(skill["enabled"], false);
    assert_eq!(skill["source"], "user");
    let before = file_snapshot(&server.home.join("skills"));
    let (status, error) = server
        .request(
            "POST",
            "/lingxi/v1/skills/install",
            Some(json!({"path":"relative/invalid"})),
        )
        .await;
    assert_eq!(status, 400);
    assert!(error["error"].is_string());
    assert_eq!(file_snapshot(&server.home.join("skills")), before);
    let empty = server.home.join("empty-source");
    std::fs::create_dir_all(&empty).unwrap();
    let (status, error) = server
        .request(
            "POST",
            "/lingxi/v1/skills/install",
            Some(json!({"path":empty})),
        )
        .await;
    assert!(status >= 400, "{error}");
    assert_eq!(file_snapshot(&server.home.join("skills")), before);
    let denied = write_package(&server.home.join("incoming"), "denied-original");
    let (status, error) = server
        .request_token(
            "POST",
            "/lingxi/v1/skills/install",
            Some(json!({"path":denied})),
            &server.readonly_token,
        )
        .await;
    assert_eq!(status, 403, "{error}");
    assert_eq!(file_snapshot(&server.home.join("skills")), before);
    assert_eq!(lists(&server, "agentA"), before_a);
    assert_eq!(lists(&server, "agentB"), before_b);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_skill_enable_disable_runtime_and_workspace_shadow_boundaries() {
    let server = start_server("toggle-original").await;
    write_package(&server.home.join("skills"), "ordinary-original");
    write_package(&server.home.join("skills"), "shadow-original");
    write_package(&server.workspace.join(".agents/skills"), "shadow-original");
    write_package(
        &server.workspace.join(".agents/skills"),
        "workspace-original",
    );
    for enable in [true, false] {
        let (status, result) = server
            .request(
                "PATCH",
                "/lingxi/v1/agents/agentA/skills/ordinary-original",
                Some(json!({"enabled":enable})),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(result["enabled"], enable);
        assert_eq!(
            lists(&server, "agentA")
                .0
                .contains(&"ordinary-original".into()),
            enable
        );
        let live = view(&server, "agentA", true).await;
        let entry = live["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "ordinary-original" && s["source"] == "user")
            .unwrap();
        assert_eq!(entry["enabled"], enable);
        assert!(
            !lists(&server, "agentB")
                .0
                .contains(&"ordinary-original".into())
        );
    }
    for enable in [false, true] {
        let (status, result) = server
            .request(
                "PATCH",
                "/lingxi/v1/agents/agentA/skills/workspace-original",
                Some(json!({"enabled":enable})),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(result["enabled"], enable);
        assert_eq!(
            lists(&server, "agentA")
                .1
                .contains(&"workspace-original".into()),
            !enable
        );
        let live = view(&server, "agentA", true).await;
        let entry = live["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "workspace-original")
            .unwrap();
        assert_eq!(entry["enabled"], enable);
        assert!(
            !lists(&server, "agentB")
                .1
                .contains(&"workspace-original".into())
        );
    }
    let before = file_snapshot(&server.home.join("agents"));
    let (status, error) = server
        .request(
            "PATCH",
            "/lingxi/v1/agents/agentA/skills/shadow-original",
            Some(json!({"enabled":true})),
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(error["code"], "WORKSPACE_SKILL_SHADOWED");
    assert_eq!(file_snapshot(&server.home.join("agents")), before);
    for (name, payload, want) in [
        ("ghost-original", json!({"enabled":true}), 404),
        ("ordinary-original", json!({"enabled":"true"}), 400),
    ] {
        let (status, error) = server
            .request(
                "PATCH",
                &format!("/lingxi/v1/agents/agentA/skills/{name}"),
                Some(payload),
            )
            .await;
        assert_eq!(status, want, "{error}");
        assert_eq!(file_snapshot(&server.home.join("agents")), before);
    }
    let (status, error) = server
        .request_token(
            "PATCH",
            "/lingxi/v1/agents/agentA/skills/ordinary-original",
            Some(json!({"enabled":true})),
            &server.readonly_token,
        )
        .await;
    assert_eq!(status, 403, "{error}");
    assert_eq!(file_snapshot(&server.home.join("agents")), before);
    let (status, error) = server
        .request("GET", "/lingxi/v1/skills?runtime=1", None)
        .await;
    assert_eq!(status, 400);
    assert!(error["error"].is_string());
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_external_paths_persist_readback_and_rejections_without_mutation() {
    let server = start_server("external-original").await;
    let external = server.home.join("external");
    write_package(&external, "external-original");
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/skills/external-paths",
            Some(json!({"paths":[external]})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    let (status, actual) = server
        .request("GET", "/lingxi/v1/skills/external-paths", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(actual["configured"], json!([external]));
    let prefs = server.home.join("user/preferences.json");
    let before = std::fs::read(&prefs).unwrap();
    let disk: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(disk["external_skill_paths"], actual["configured"]);
    let exposed = view(&server, "agentA", false).await;
    let entry = exposed["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "external-original")
        .unwrap();
    assert_eq!(entry["readonly"], true);
    for payload in [
        json!({"paths":["relative"]}),
        json!({"paths":[server.home.join("skills")]}),
        json!({"paths":"bad"}),
    ] {
        let (status, error) = server
            .request("PUT", "/lingxi/v1/skills/external-paths", Some(payload))
            .await;
        assert_eq!(status, 400, "{error}");
        assert_eq!(std::fs::read(&prefs).unwrap(), before);
    }
    let (status, error) = server
        .request_token(
            "PUT",
            "/lingxi/v1/skills/external-paths",
            Some(json!({"paths":[]})),
            &server.readonly_token,
        )
        .await;
    assert_eq!(status, 403, "{error}");
    assert_eq!(std::fs::read(&prefs).unwrap(), before);
    let (status, error) = server
        .request(
            "DELETE",
            "/lingxi/v1/skills/external-original?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(error["code"], "SKILL_EXTERNAL_READONLY");
    assert!(external.join("external-original/SKILL.md").is_file());
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/skills/external-paths",
            Some(json!({"paths":[]})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, result) = server
        .request("GET", "/lingxi/v1/skills/external-paths", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["configured"], json!([]));
    assert!(
        !view(&server, "agentA", false).await["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "external-original")
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_bundles_agent_projection_update_order_delete_and_rejections() {
    let server = start_server("bundle-original").await;
    write_package(&server.home.join("skills"), "one-original");
    write_package(&server.home.join("skills"), "two-original");
    let (status, _) = server
        .request(
            "PATCH",
            "/lingxi/v1/agents/agentA/skills/one-original",
            Some(json!({"enabled":true})),
        )
        .await;
    assert_eq!(status, 200);
    let mut ids = Vec::new();
    for name in ["组合一", "组合二"] {
        let (status, result) = server
            .request(
                "POST",
                "/lingxi/v1/skills/bundles?agentId=agentA",
                Some(json!({"name":name,"skillNames":["one-original","two-original"]})),
            )
            .await;
        assert_eq!(status, 200);
        ids.push(result["bundle"]["id"].as_str().unwrap().to_string());
    }
    let (status, view_a) = server
        .request("GET", "/lingxi/v1/skills/bundles?agentId=agentA", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        view_a["bundles"][0]["skills"],
        json!([{"name":"one-original","enabled":true,"source":"user","missing":false},{"name":"two-original","enabled":false,"source":"user","missing":false}])
    );
    let (status, view_b) = server
        .request("GET", "/lingxi/v1/skills/bundles?agentId=agentB", None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(view_b["bundles"][0]["skills"][0]["enabled"], false);
    let (status, result) = server
        .request(
            "PUT",
            &format!("/lingxi/v1/skills/bundles/{}?agentId=agentA", ids[0]),
            Some(json!({"name":"新名称"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["bundle"]["name"], "新名称");
    assert_eq!(
        result["bundle"]["skillNames"],
        json!(["one-original", "two-original"])
    );
    let (status, result) = server
        .request(
            "PUT",
            &format!("/lingxi/v1/skills/bundles/{}?agentId=agentA", ids[0]),
            Some(json!({"skillNames":["two-original"]})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["bundle"]["name"], "新名称");
    assert_eq!(result["bundle"]["skillNames"], json!(["two-original"]));
    let order = json!([ids[1], ids[0]]);
    let (status, result) = server
        .request(
            "PUT",
            "/lingxi/v1/skills/bundles/order?agentId=agentA",
            Some(json!({"bundleIds":order})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        result["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["id"].clone())
            .collect::<Vec<_>>(),
        order.as_array().unwrap().clone()
    );
    let store = lingxi_adapters::storage::skill_library::BundlesStore::new(&server.home);
    assert_eq!(
        store
            .load()
            .iter()
            .map(|b| b["id"].clone())
            .collect::<Vec<_>>(),
        order.as_array().unwrap().clone()
    );
    let before = store.load();
    for path in ["/lingxi/v1/skills/bundles", "/lingxi/v1/skills"] {
        let (status, error) = server.request("GET", path, None).await;
        assert_eq!(status, 400);
        assert!(error["error"].is_string());
        assert_eq!(store.load(), before);
    }
    for payload in [
        json!({"bundleIds":"bad"}),
        json!({"bundleIds":["unknown-a","unknown-b"]}),
    ] {
        let (status, error) = server
            .request(
                "PUT",
                "/lingxi/v1/skills/bundles/order?agentId=agentA",
                Some(payload),
            )
            .await;
        assert_eq!(status, 400, "{error}");
        assert_eq!(store.load(), before);
    }
    let (status, error) = server
        .request(
            "PUT",
            &format!("/lingxi/v1/skills/bundles/{}?agentId=agentA", ids[0]),
            Some(json!({"skillNames":["ghost"]})),
        )
        .await;
    assert_eq!(status, 400);
    assert!(error["error"].is_string());
    assert_eq!(store.load(), before);
    let (status, error) = server
        .request_token(
            "PUT",
            "/lingxi/v1/skills/bundles/order?agentId=agentA",
            Some(json!({"bundleIds":order})),
            &server.readonly_token,
        )
        .await;
    assert_eq!(status, 403, "{error}");
    assert_eq!(store.load(), before);
    let (status, result) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/skills/bundles/{}", ids[0]),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    assert_eq!(store.load().len(), 1);
    assert!(server.home.join("skills/one-original/SKILL.md").is_file());
    assert!(server.home.join("skills/two-original/SKILL.md").is_file());
    let before = store.load();
    let (status, error) = server
        .request("DELETE", "/lingxi/v1/skills/bundles/no-such", None)
        .await;
    assert_eq!(status, 404);
    assert!(error["error"].is_string());
    assert_eq!(store.load(), before);

    // 正常技能删除须回读全部助手与仍在使用该技能的组合引用。
    let (status, _) = server
        .request(
            "PATCH",
            "/lingxi/v1/agents/agentB/skills/one-original",
            Some(json!({"enabled":true})),
        )
        .await;
    assert_eq!(status, 200);
    assert!(lists(&server, "agentA").0.contains(&"one-original".into()));
    assert!(lists(&server, "agentB").0.contains(&"one-original".into()));
    let (status, result) = server
        .request(
            "DELETE",
            "/lingxi/v1/skills/one-original?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["ok"], true);
    assert!(!server.home.join("skills/one-original").exists());
    assert!(!lists(&server, "agentA").0.contains(&"one-original".into()));
    assert!(!lists(&server, "agentB").0.contains(&"one-original".into()));
    assert_eq!(store.load()[0]["skillNames"], json!(["two-original"]));
    assert!(server.home.join("skills/two-original/SKILL.md").is_file());
    let before = store.load();
    let (status, error) = server
        .request("DELETE", "/lingxi/v1/skills/no-such?agentId=agentA", None)
        .await;
    assert_eq!(status, 404);
    assert!(error["error"].is_string());
    assert_eq!(store.load(), before);

    // 已存组合引用缺失的技能，真实读取必须显示missing而不是伪造技能。
    let stale = store
        .create("缺文件旧组合", &["missing-original".into()], "user", None)
        .unwrap();
    let (status, projection) = server
        .request("GET", "/lingxi/v1/skills/bundles?agentId=agentA", None)
        .await;
    assert_eq!(status, 200);
    let stale_view = projection["bundles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|bundle| bundle["id"] == stale["id"])
        .unwrap();
    assert_eq!(
        stale_view["skills"],
        json!([{"name":"missing-original","enabled":false,"source":null,"missing":true}])
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_skill_patch_refreshes_same_compiler_and_frozen_previous_prompt() {
    use lingxi_service::context_compiler::{
        ContextCompilerService, FileContextMaterialSource, HostFacts,
    };
    let server = start_server("prompt-refresh-original").await;
    write_package(&server.home.join("skills"), "prompt-original");
    write_package(
        &server.workspace.join(".agents/skills"),
        "workspace-prompt-original",
    );
    let product = server.home.join("proof-product");
    std::fs::create_dir_all(product.join("yuan")).unwrap();
    std::fs::write(product.join("yuan/lingxi.md"), "你是{{agentName}}。").unwrap();
    std::fs::write(product.join("identity.example.md"), "独立身份").unwrap();
    std::fs::write(product.join("agents.example.md"), "独立规则").unwrap();
    let compiler =
        ContextCompilerService::new(std::sync::Arc::new(FileContextMaterialSource::from_home(
            &server.home,
            product.clone(),
            HostFacts {
                platform: "synthetic".into(),
                os_type: "Synthetic".into(),
                os_release: "1".into(),
                shell_label: "synthetic".into(),
                workspace: server.workspace.to_string_lossy().into(),
            },
        )));
    let skills = std::sync::Arc::new(lingxi_service::skills::SkillService::new(
        server.home.clone(),
        product,
    ));
    skills.set_workspace_root(Some(server.workspace.clone()));
    compiler.set_skill_materials(skills.clone());
    let before = compiler
        .compiled_for_run("sess-original-skill-refresh", "agentA", false, 1)
        .unwrap();
    assert!(!before.render().contains("<name>prompt-original</name>"));
    assert!(
        before
            .render()
            .contains("<name>workspace-prompt-original</name>")
    );
    let (status, _) = server
        .request(
            "PATCH",
            "/lingxi/v1/agents/agentA/skills/prompt-original",
            Some(json!({"enabled":true})),
        )
        .await;
    assert_eq!(status, 200);
    let enabled = compiler
        .compiled_for_run("sess-original-skill-refresh", "agentA", false, 1)
        .unwrap();
    assert!(enabled.render().contains("<name>prompt-original</name>"));
    assert!(!before.render().contains("<name>prompt-original</name>"));
    let (status, _) = server
        .request(
            "PATCH",
            "/lingxi/v1/agents/agentA/skills/prompt-original",
            Some(json!({"enabled":false})),
        )
        .await;
    assert_eq!(status, 200);
    let disabled = compiler
        .compiled_for_run("sess-original-skill-refresh", "agentA", false, 1)
        .unwrap();
    assert!(!disabled.render().contains("<name>prompt-original</name>"));
    assert!(enabled.render().contains("<name>prompt-original</name>"));
    let (status, _) = server
        .request(
            "PATCH",
            "/lingxi/v1/agents/agentA/skills/workspace-prompt-original",
            Some(json!({"enabled":false})),
        )
        .await;
    assert_eq!(status, 200);
    assert!(
        !skills
            .runtime_skill_catalog("agentA")
            .unwrap()
            .iter()
            .any(|s| s.name == "workspace-prompt-original")
    );
    let workspace_disabled = compiler
        .compiled_for_run("sess-original-skill-refresh", "agentA", false, 1)
        .unwrap();
    assert!(
        !workspace_disabled
            .render()
            .contains("<name>workspace-prompt-original</name>")
    );
    assert!(
        before
            .render()
            .contains("<name>workspace-prompt-original</name>")
    );
    let other = compiler
        .compiled_for_run("sess-original-skill-refresh-b", "agentB", false, 1)
        .unwrap();
    assert!(!other.render().contains("<name>prompt-original</name>"));
    assert!(
        other
            .render()
            .contains("<name>workspace-prompt-original</name>")
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_bundle_edit_requires_agent_query_without_mutating() {
    let server = start_server("edit-agent-original").await;
    write_package(&server.home.join("skills"), "edit-original");
    let store = lingxi_adapters::storage::skill_library::BundlesStore::new(&server.home);
    let created = store
        .create("原组合", &["edit-original".into()], "user", None)
        .unwrap();
    let id = created["id"].as_str().unwrap();
    let before = store.load();
    let (status, error) = server
        .request(
            "PUT",
            &format!("/lingxi/v1/skills/bundles/{id}"),
            Some(json!({"name":"未指定助手不得改"})),
        )
        .await;
    assert_eq!(status, 400);
    assert!(error["error"].is_string());
    assert_eq!(store.load(), before);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_nonempty_cleanup_preview_excludes_shared_and_readonly_sources() {
    let server = start_server("cleanup-preview-original").await;
    write_package(&server.home.join("skills"), "only-a-original");
    write_package(&server.home.join("skills"), "shared-original");
    write_package(&server.home.join("skills"), "disabled-original");
    let external = server.home.join("external-original/skills");
    write_package(&external, "external-original");
    let plugin = server.home.join("plugin:original/skills");
    write_package(&plugin, "plugin-original");
    write_package(
        &server.workspace.join(".agents/skills"),
        "workspace-preview-original",
    );
    let (status, _) = server
        .request(
            "PUT",
            "/lingxi/v1/skills/external-paths",
            Some(json!({"paths":[external,plugin]})),
        )
        .await;
    assert_eq!(status, 200);
    // 构造实际助手配置，预览本身只读，不经过启停接口来替代预览断言。
    for (agent, enabled) in [
        (
            "agentA",
            vec![
                "only-a-original",
                "shared-original",
                "external-original",
                "plugin-original",
                "builtin-original",
            ],
        ),
        ("agentB", vec!["shared-original"]),
    ] {
        let path = server.home.join("agents").join(agent).join("config.yaml");
        let text = std::fs::read_to_string(&path).unwrap();
        let enabled = enabled.into_iter().map(str::to_string).collect::<Vec<_>>();
        let next = lingxi_adapters::storage::agent_memory::AgentConfigPatch::write_skill_lists(
            &text,
            &enabled,
            &[],
        )
        .unwrap();
        std::fs::write(path, next).unwrap();
    }
    let before_pool = file_snapshot(&server.home.join("skills"));
    let before_a = file_snapshot(&server.home.join("agents/agentA"));
    let before_b = file_snapshot(&server.home.join("agents/agentB"));
    let before_external = file_snapshot(&external);
    let before_plugin = file_snapshot(&plugin);
    let (status, result) = server
        .request(
            "GET",
            "/lingxi/v1/agents/agentA/skills/cleanup-preview",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        result["skills"],
        json!([{"name":"only-a-original","description":"原断言专属技能"}])
    );
    let (status, other) = server
        .request(
            "GET",
            "/lingxi/v1/agents/agentB/skills/cleanup-preview",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(other["skills"], json!([]));
    // 同一个生产预览方法使用独立合成产品资料，直接证明内置来源也不入候选。
    let product = server.home.join("preview-product");
    write_package(&product.join("skills2set"), "builtin-original");
    let skills = lingxi_service::skills::SkillService::new(server.home.clone(), product.clone());
    skills.set_workspace_root(Some(server.workspace.clone()));
    let catalog = skills
        .list_for_agent("agentA", true, Some(&server.workspace))
        .unwrap();
    assert!(
        catalog.iter().any(|s| s["name"] == "builtin-original"
            && s["readonly"] == true
            && s["enabled"] == true)
    );
    assert!(catalog.iter().any(|s| s["name"] == "external-original"
        && s["readonly"] == true
        && s["enabled"] == true));
    assert!(
        catalog
            .iter()
            .any(|s| s["name"] == "workspace-preview-original" && s["source"] == "workspace")
    );
    let paths = [lingxi_adapters::storage::skill_library::ExternalSkillPath {
        dir_path: plugin.clone(),
        label: "plugin:original".into(),
        scope: "external".into(),
        category: "external".into(),
    }];
    assert!(
        lingxi_adapters::storage::skill_library::SkillCatalog::scan_external(&paths)
            .iter()
            .any(|s| s.name == "plugin-original" && s.plugin_skill && s.readonly)
    );
    let before_product = file_snapshot(&product);
    let production = skills
        .compute_orphan_skills("agentA", Some(&server.workspace))
        .unwrap();
    assert_eq!(production, result);
    assert_eq!(file_snapshot(&product), before_product);
    assert_eq!(file_snapshot(&server.home.join("skills")), before_pool);
    assert_eq!(file_snapshot(&server.home.join("agents/agentA")), before_a);
    assert_eq!(file_snapshot(&server.home.join("agents/agentB")), before_b);
    assert_eq!(file_snapshot(&external), before_external);
    assert_eq!(file_snapshot(&plugin), before_plugin);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_skill_delete_reads_every_agent_and_bundle_after_complete_or_partial_cleanup() {
    let modes = if cfg!(unix) {
        vec!["complete", "late-reference-write"]
    } else {
        vec!["complete"]
    };
    for mode in modes {
        let server = start_server(&format!("all-delete-refs-{mode}")).await;
        write_package(&server.home.join("skills"), "delete-all-original");
        let untouched = write_package(&server.home.join("skills"), "keep-all-original");
        let untouched_before = file_snapshot(&untouched);
        let extra = server.home.join("agents/agentC");
        std::fs::create_dir_all(extra.join("memory")).unwrap();
        std::fs::write(
            extra.join("config.yaml"),
            "agent:\n  name: 第三助手\nmemory:\n  enabled: true\n",
        )
        .unwrap();
        // 枚举当前合成HOME的全部实际助手，而不是只检查两个已知助手。
        let mut agents = std::fs::read_dir(server.home.join("agents"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|dir| dir.is_dir() && dir.join("config.yaml").is_file())
            .collect::<Vec<_>>();
        agents.sort();
        assert!(agents.len() >= 3);
        let mut agent_before = Vec::new();
        for dir in &agents {
            let id = dir.file_name().unwrap().to_str().unwrap().to_string();
            let path = dir.join("config.yaml");
            let text = std::fs::read_to_string(&path).unwrap();
            let (mut enabled, disabled) =
                lingxi_adapters::storage::agent_memory::AgentConfigPatch::read_skill_lists(&text)
                    .unwrap();
            for skill in ["delete-all-original", "keep-all-original"] {
                if !enabled.contains(&skill.to_string()) {
                    enabled.push(skill.to_string());
                }
            }
            let next = lingxi_adapters::storage::agent_memory::AgentConfigPatch::write_skill_lists(
                &text, &enabled, &disabled,
            )
            .unwrap();
            std::fs::write(&path, &next).unwrap();
            agent_before.push((id, next.into_bytes(), enabled, disabled));
        }
        let store = lingxi_adapters::storage::skill_library::BundlesStore::new(&server.home);
        for (name, members) in [
            (
                "两个成员组合",
                vec!["delete-all-original", "keep-all-original"],
            ),
            ("仅目标成员组合", vec!["delete-all-original"]),
            ("无关组合", vec!["keep-all-original"]),
        ] {
            let (status, receipt) = server
                .request(
                    "POST",
                    "/lingxi/v1/skills/bundles?agentId=agentA",
                    Some(json!({"name":name,"skillNames":members})),
                )
                .await;
            assert_eq!(status, 200, "{receipt}");
        }
        let bundles_before = store.load_checked().unwrap();
        assert_eq!(bundles_before.len(), 3);
        #[cfg(unix)]
        let mut permissions = Vec::new();
        #[cfg(unix)]
        if mode == "late-reference-write" {
            use std::os::unix::fs::PermissionsExt;
            // 预检可读；真实删除后首次atomic_write无权限，所有引用尚未改写。
            for dir in &agents {
                permissions.push((dir.clone(), std::fs::metadata(dir).unwrap().permissions()));
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
            }
        }
        let (status, receipt) = server
            .request(
                "DELETE",
                "/lingxi/v1/skills/delete-all-original?agentId=agentA",
                None,
            )
            .await;
        #[cfg(unix)]
        for (path, permission) in permissions {
            std::fs::set_permissions(path, permission).unwrap();
        }
        if mode == "complete" {
            assert_eq!(status, 200, "{receipt}");
            assert_eq!(receipt["ok"], true);
            let mut scrubbed = receipt["scrubbedAgents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().to_string())
                .collect::<Vec<_>>();
            scrubbed.sort();
            assert_eq!(
                scrubbed,
                agent_before
                    .iter()
                    .map(|(id, ..)| id.clone())
                    .collect::<Vec<_>>()
            );
        } else {
            assert_eq!(status, 500, "{receipt}");
            assert_eq!(receipt["code"], "SKILL_DELETED_REFERENCE_CLEANUP_FAILED");
            assert_ne!(receipt["ok"], true);
        }
        assert!(!server.home.join("skills/delete-all-original").exists());
        assert_eq!(file_snapshot(&untouched), untouched_before);
        let mut residual_agents = Vec::new();
        for (id, bytes, enabled, disabled) in &agent_before {
            let path = server.home.join("agents").join(id).join("config.yaml");
            let expected = if mode == "complete" {
                enabled
                    .iter()
                    .filter(|name| name.as_str() != "delete-all-original")
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                enabled.clone()
            };
            assert_eq!(lists(&server, id), (expected, disabled.clone()));
            if mode != "complete" {
                assert_eq!(std::fs::read(path).unwrap(), *bytes);
                residual_agents.push(id.clone());
            }
            let live = view(&server, id, true).await;
            assert!(
                !live["skills"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["name"] == "delete-all-original")
            );
            assert!(
                live["skills"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["name"] == "keep-all-original" && s["enabled"] == true)
            );
        }
        let bundles_after = store.load_checked().unwrap();
        assert_eq!(bundles_after.len(), if mode == "complete" { 2 } else { 3 });
        let mut residual_bundles = Vec::new();
        for original in &bundles_before {
            let actual = bundles_after
                .iter()
                .find(|bundle| bundle["id"] == original["id"]);
            let mut expected = original.clone();
            if mode == "complete" {
                expected["skillNames"] = json!(
                    original["skillNames"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|name| name.as_str() != Some("delete-all-original"))
                        .cloned()
                        .collect::<Vec<_>>()
                );
                if expected["skillNames"].as_array().unwrap().is_empty() {
                    assert!(actual.is_none());
                    continue;
                }
                // 清理会更新时间，但不能改变剩余成员及其它组合信息。
                assert!(actual.unwrap()["updatedAt"].is_string());
                expected["updatedAt"] = actual.unwrap()["updatedAt"].clone();
            }
            let actual = actual.unwrap();
            assert_eq!(actual, &expected);
            if actual["skillNames"]
                .as_array()
                .unwrap()
                .iter()
                .any(|name| name == "delete-all-original")
            {
                residual_bundles.push(actual["id"].as_str().unwrap().to_string());
            }
        }
        let (status, projected) = server
            .request("GET", "/lingxi/v1/skills/bundles?agentId=agentA", None)
            .await;
        assert_eq!(status, 200);
        assert_eq!(
            projected["bundles"].as_array().unwrap().len(),
            bundles_after.len()
        );
        for persisted in &bundles_after {
            let displayed = projected["bundles"]
                .as_array()
                .unwrap()
                .iter()
                .find(|bundle| bundle["id"] == persisted["id"])
                .unwrap();
            assert_eq!(displayed["skillNames"], persisted["skillNames"]);
            for member in displayed["skills"].as_array().unwrap() {
                if member["name"] == "delete-all-original" {
                    assert_eq!(member["missing"], true);
                    assert_eq!(member["enabled"], false);
                }
            }
        }
        if mode == "complete" {
            assert!(residual_agents.is_empty() && residual_bundles.is_empty());
        } else {
            assert_eq!(residual_agents.len(), agent_before.len());
            assert_eq!(residual_bundles.len(), 2);
        }
        println!(
            "原技能删除逐项回读 mode={mode} status={status} residual_agents={residual_agents:?} residual_bundles={residual_bundles:?}"
        );
        server.stop_and_clean().await;
    }
}
