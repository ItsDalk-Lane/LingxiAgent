//! 3516：当前React桌面Rust设置入口、真实服务与删除副作用联合证明。
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_delete_orphan_skills_current_desktop_rust_api_and_ui() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    for scenario in ["complete", "changed", "failed", "last"] {
        let server = boot().await;
        for (id, name) in [("victim", "待删除助手"), ("survivor", "保留助手")] {
            assert_eq!(
                ok(
                    &server,
                    "POST",
                    "/lingxi/v1/agents",
                    Some(&json!({"id":id,"name":name}))
                )
                .await
                .0,
                200
            );
        }
        assert_eq!(
            ok(
                &server,
                "PUT",
                "/lingxi/v1/agents/primary",
                Some(&json!({"id":"survivor"}))
            )
            .await
            .0,
            200
        );
        for name in ["demo-owned", "demo-keep"] {
            let dir = server.home.join("skills").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: 合成独占技能\n---\n真实保留正文{name}\n"),
            )
            .unwrap();
        }
        let victim_config = server.home.join("agents/victim/config.yaml");
        std::fs::write(
            &victim_config,
            "agent:\n  name: 待删除助手\nskills:\n  enabled: [demo-owned, demo-keep]\n",
        )
        .unwrap();
        let tombstone = server.home.join("agents/tombstone");
        std::fs::create_dir_all(&tombstone).unwrap();
        std::fs::write(
            tombstone.join("config.yaml"),
            "agent:\n  name: 墓碑资料\nskills:\n  enabled: [demo-owned, demo-keep]\n",
        )
        .unwrap();
        std::fs::write(tombstone.join(".deleted-agent.json"), "{}").unwrap();
        let bundles = lingxi_adapters::storage::skill_library::BundlesStore::new(&server.home);
        bundles
            .create(
                "保留组合",
                &["demo-owned".into(), "demo-keep".into()],
                "user",
                None,
            )
            .unwrap();
        let config_before = std::fs::read(&victim_config).unwrap();
        let skill_before = std::fs::read(server.home.join("skills/demo-owned/SKILL.md")).unwrap();
        let bundle_before = bundles.load_checked().unwrap();
        let tombstone_before = std::fs::read(tombstone.join("config.yaml")).unwrap();
        // 未鉴权的删除必须先被真实鉴权链拒绝，任何资料均保全。
        assert_eq!(
            http(
                &server,
                "DELETE",
                "/lingxi/v1/agents/victim",
                Some(&json!({"deleteSkills":["demo-owned"]})),
                None
            )
            .await
            .0,
            401
        );
        assert_eq!(std::fs::read(&victim_config).unwrap(), config_before);
        assert_eq!(
            std::fs::read(server.home.join("skills/demo-owned/SKILL.md")).unwrap(),
            skill_before
        );
        let output_file = server.home.join("ui-result.json");
        let node = std::env::var("R06_NODE_BIN").unwrap_or_else(|_| "node".into());
        let output = tokio::process::Command::new(node)
            .arg(repo.join("node_modules/vitest/vitest.mjs"))
            .args([
                "run",
                "desktop/src/react/settings/__tests__/R06AgentDeleteRustLive.test.tsx",
                "-t",
                "^rust_agent_delete_current_settings_real_service$",
                "--reporter=json",
                "--maxWorkers=1",
            ])
            .arg(format!("--outputFile={}", output_file.display()))
            .current_dir(&repo)
            .env("R06_AGENT_DELETE_UI_URL", format!("http://{}", server.addr))
            .env("R06_AGENT_DELETE_UI_TOKEN", &server.token)
            .env("R06_AGENT_DELETE_UI_HOME", &server.home)
            .env("R06_AGENT_DELETE_UI_CASE", scenario)
            .output()
            .await
            .unwrap();
        // 包装输出逐行加前缀，不能干扰正式Rust精确用例结果计数。
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            eprintln!("UI-CHILD[{scenario}]: {line}");
        }
        for line in String::from_utf8_lossy(&output.stderr).lines() {
            eprintln!("UI-CHILD-ERR[{scenario}]: {line}");
        }
        assert!(
            output.status.success(),
            "当前桌面入口真实HTTP/UI子用例失败：{scenario}"
        );
        let proof: Value = serde_json::from_slice(&std::fs::read(output_file).unwrap()).unwrap();
        assert_eq!(proof["success"], true);
        assert_eq!(proof["numTotalTests"], 1);
        assert_eq!(proof["numPassedTests"], 1);
        assert_eq!(proof["numFailedTests"], 0);
        if let Ok(dir) = std::env::var("R06_AGENT_DELETE_EVIDENCE_DIR") {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{scenario}-ui.json")),
                serde_json::to_vec_pretty(&proof).unwrap(),
            )
            .unwrap();
        }
        let results = proof["testResults"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        let assertions = results[0]["assertionResults"].as_array().unwrap();
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0]["status"], "passed");
        assert_eq!(
            assertions[0]["fullName"],
            "rust_agent_delete_current_settings_real_service"
        );
        assert!(
            server.home.join("skills/demo-keep/SKILL.md").is_file(),
            "未勾选技能必须保留"
        );
        if scenario == "complete" {
            assert!(!server.home.join("agents/victim").exists());
            assert!(!server.home.join("skills/demo-owned").exists());
            let (enabled, _) =
                lingxi_adapters::storage::agent_memory::AgentConfigPatch::read_skill_lists(
                    &std::fs::read_to_string(tombstone.join("config.yaml")).unwrap(),
                )
                .unwrap();
            assert_eq!(enabled, vec!["demo-keep"]);
            assert_eq!(
                bundles.load_checked().unwrap()[0]["skillNames"],
                json!(["demo-keep"])
            );
            for id in ["survivor", "tombstone"] {
                let (enabled, _) =
                    lingxi_adapters::storage::agent_memory::AgentConfigPatch::read_skill_lists(
                        &std::fs::read_to_string(
                            server.home.join("agents").join(id).join("config.yaml"),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                assert!(!enabled.contains(&"demo-owned".into()));
            }
        } else {
            assert_eq!(
                std::fs::read(server.home.join("skills/demo-owned/SKILL.md")).unwrap(),
                skill_before
            );
            assert_eq!(bundles.load_checked().unwrap(), bundle_before);
            assert_eq!(
                std::fs::read(tombstone.join("config.yaml")).unwrap(),
                tombstone_before
            );
            if scenario == "last" {
                assert_eq!(std::fs::read(&victim_config).unwrap(), config_before);
            } else {
                assert!(!server.home.join("agents/victim").exists());
            }
            if scenario == "changed" {
                let (_, config) =
                    ok(&server, "GET", "/lingxi/v1/agents/survivor/config", None).await;
                assert_eq!(config["skills"]["enabled"], json!(["demo-owned"]));
            }
        }
        let (_, feed) = ok(&server, "GET", "/lingxi/v1/app-events?since=0", None).await;
        let events = feed["events"].as_array().unwrap();
        let deleted = events
            .iter()
            .any(|e| e["type"] == "agent-deleted" && e["payload"]["agentId"] == "victim");
        assert_eq!(deleted, scenario != "last");
        if scenario == "complete" {
            let agent_position = events
                .iter()
                .position(|e| e["type"] == "agent-deleted" && e["payload"]["agentId"] == "victim")
                .unwrap();
            let skill_position = events
                .iter()
                .position(|e| e["type"] == "skills-changed")
                .unwrap();
            assert!(
                agent_position < skill_position,
                "先删除助手并发布事件，再完成连带技能删除"
            );
        }
        server.stop.send(()).unwrap();
        server.handle.await.unwrap().unwrap();
        std::fs::remove_dir_all(server.home).unwrap();
    }
}

async fn connection_fixture(server: &Server) {
    for (id, name) in [("victim", "待删除助手"), ("survivor", "保留助手")] {
        assert_eq!(
            ok(
                server,
                "POST",
                "/lingxi/v1/agents",
                Some(&json!({"id":id,"name":name}))
            )
            .await
            .0,
            200
        );
    }
    assert_eq!(
        ok(
            server,
            "PUT",
            "/lingxi/v1/agents/primary",
            Some(&json!({"id":"survivor"}))
        )
        .await
        .0,
        200
    );
    std::fs::write(
        server.home.join("agents/victim/config.yaml"),
        "agent:\n  name: 待删除助手\nskills:\n  enabled: [demo-owned, demo-keep]\n",
    )
    .unwrap();
    for name in ["demo-owned", "demo-keep"] {
        let dir = server.home.join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: 合成独占技能\n---\n正文{name}\n"),
        )
        .unwrap();
    }
}
fn connection_material_snapshot(server: &Server) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(dir: &std::path::Path, home: &std::path::Path, items: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, home, items);
            } else {
                items.push((
                    path.strip_prefix(home).unwrap().into(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut items = Vec::new();
    for name in ["agents", "skills", "user"] {
        collect(&server.home.join(name), &server.home, &mut items);
    }
    items.sort();
    items
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_delete_connection_changes_preserve_both_real_service_roots() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    for scenario in [
        "guard-original",
        "guard-node-new-id",
        "guard-node-same-id",
        "guard-rust-endpoint",
        "guard-transport",
        "guard-instance",
        "guard-kind",
        "guard-token",
        "guard-roundtrip",
        "guard-stable",
        "guard-registry-original",
        "guard-registry-endpoint",
        "guard-registry-roundtrip",
        "guard-registry-stable",
        "guard-registry-source",
        "guard-registry-fallback",
        "guard-registry-unrelated",
    ] {
        let original = boot().await;
        let other = boot().await;
        connection_fixture(&original).await;
        connection_fixture(&other).await;
        let original_before = connection_material_snapshot(&original);
        let other_before = connection_material_snapshot(&other);
        let independent = matches!(scenario, "guard-original" | "guard-registry-original");
        let (file, title) = if scenario == "guard-registry-original" {
            (
                "desktop/src/react/settings/__tests__/R06AgentDeleteRegistryReview.test.tsx",
                "rust_agent_delete_registry_only_counterexample",
            )
        } else if independent {
            (
                "desktop/src/react/settings/__tests__/R06AgentDeleteConnectionReview.test.tsx",
                "rust_agent_delete_connection_change_counterexample",
            )
        } else {
            (
                "desktop/src/react/settings/__tests__/R06AgentDeleteRustLive.test.tsx",
                "rust_agent_delete_current_settings_real_service",
            )
        };
        let output_file = original.home.join("ui-result.json");
        let output = tokio::process::Command::new(
            std::env::var("R06_NODE_BIN").unwrap_or_else(|_| "node".into()),
        )
        .arg(repo.join("node_modules/vitest/vitest.mjs"))
        .args([
            "run",
            file,
            "-t",
            &format!("^{title}$"),
            "--reporter=json",
            "--maxWorkers=1",
        ])
        .arg(format!("--outputFile={}", output_file.display()))
        .current_dir(&repo)
        .env(
            "R06_AGENT_DELETE_UI_URL",
            format!("http://{}", original.addr),
        )
        .env("R06_AGENT_DELETE_UI_TOKEN", &original.token)
        .env("R06_AGENT_DELETE_UI_HOME", &original.home)
        .env(
            "R06_AGENT_DELETE_UI_CASE",
            if independent { "complete" } else { scenario },
        )
        .env(
            "R06_AGENT_DELETE_UI_OTHER_URL",
            format!("http://{}", other.addr),
        )
        .env("R06_AGENT_DELETE_UI_OTHER_TOKEN", &other.token)
        .output()
        .await
        .unwrap();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            eprintln!("UI-CONNECTION[{scenario}]: {line}");
        }
        for line in String::from_utf8_lossy(&output.stderr).lines() {
            eprintln!("UI-CONNECTION-ERR[{scenario}]: {line}");
        }
        assert!(
            output.status.success(),
            "真实连接切换保护子用例失败：{scenario}"
        );
        let proof: Value = serde_json::from_slice(&std::fs::read(output_file).unwrap()).unwrap();
        assert_eq!(proof["success"], true);
        assert_eq!(proof["numTotalTests"], 1);
        assert_eq!(proof["numPassedTests"], 1);
        assert_eq!(proof["numFailedTests"], 0);
        let results = proof["testResults"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        let assertions = results[0]["assertionResults"].as_array().unwrap();
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0]["fullName"], title);
        assert_eq!(assertions[0]["status"], "passed");
        if let Ok(dir) = std::env::var("R06_AGENT_DELETE_EVIDENCE_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                PathBuf::from(dir).join(format!("{scenario}-ui.json")),
                serde_json::to_vec_pretty(&proof).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(
            connection_material_snapshot(&other),
            other_before,
            "另一真实服务同ID助手及技能必须全部不变"
        );
        if matches!(
            scenario,
            "guard-stable" | "guard-registry-stable" | "guard-registry-unrelated"
        ) {
            assert!(!original.home.join("agents/victim").exists());
            assert!(!original.home.join("skills/demo-owned").exists());
            assert!(original.home.join("skills/demo-keep/SKILL.md").is_file());
        } else {
            assert_eq!(
                connection_material_snapshot(&original),
                original_before,
                "拒绝旧确认后原助手及技能全部不变"
            );
        }
        for server in [original, other] {
            server.stop.send(()).unwrap();
            server.handle.await.unwrap().unwrap();
            std::fs::remove_dir_all(server.home).unwrap();
        }
    }
}
