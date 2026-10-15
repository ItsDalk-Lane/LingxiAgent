//! 原七叶补齐证明：真实 HTTP、技能包/配置/经验回读、真实模型协议及账本。
use lingxi_kernel::ports::StoragePort;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Harness {
    home: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    url: String,
    token: String,
    readonly: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn boot() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-skills-completion-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        for id in ["agentA", "agentB"] {
            std::fs::create_dir_all(home.join("agents").join(id)).unwrap();
            std::fs::write(home.join("agents").join(id).join("config.yaml"),"agent:\n  name: 测试助手\nskills:\n  enabled: []\nmemory:\n  enabled: true\n  experience_enabled: true\n").unwrap();
        }
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            r#"{"learn_skills":{"enabled":true,"safety_review":true}}"#,
        )
        .unwrap();
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 5000,
            },
            &layout,
            ServiceDeps {
                workspace_root: Some(workspace.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let readonly = state
            .auth()
            .issue_device_credential(
                lingxi_service::LOCAL_OWNER_USER_ID,
                &["settings.read"],
                None,
            )
            .unwrap()
            .secret;
        let run_state = state.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (ready, received) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            run(
                run_state,
                async {
                    let _ = rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        let addr = received.await.unwrap();
        Self {
            home,
            workspace,
            state,
            url: format!("http://{addr}"),
            token,
            readonly,
            stop: Some(tx),
            task,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value, readonly: bool) -> (u16, Value) {
        let payload = body.to_string();
        let token = if readonly {
            &self.readonly
        } else {
            &self.token
        };
        let wire=format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
        let mut stream = tokio::net::TcpStream::connect(self.url.strip_prefix("http://").unwrap())
            .await
            .unwrap();
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let split = bytes.windows(4).position(|x| x == b"\r\n\r\n").unwrap();
        let header = String::from_utf8_lossy(&bytes[..split]);
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let value = serde_json::from_slice(&bytes[split + 4..]).unwrap();
        (status, value)
    }
    async fn close(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(self.home.parent().unwrap()).unwrap();
    }
    fn package(&self, name: &str) -> PathBuf {
        let path = self.home.parent().unwrap().join(format!("source-{name}"));
        std::fs::create_dir_all(path.join("examples")).unwrap();
        std::fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: 完整技能\n---\n真实技能正文\n"),
        )
        .unwrap();
        std::fs::write(path.join("examples/sample.txt"), "配套文件原文").unwrap();
        path
    }
}
fn zip_package(name: &str) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(format!("{name}/SKILL.md"), options).unwrap();
    write!(
        zip,
        "---\nname: {name}\ndescription: 压缩包\n---\n压缩技能真实正文"
    )
    .unwrap();
    zip.start_file(format!("{name}/examples/中文.txt"), options)
        .unwrap();
    zip.write_all("压缩配套正文".as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_skill_packages_workspace_source_global_and_agent_install_rejections() {
    use base64::Engine as _;
    let h = Harness::boot().await;
    let source = h.package("panel-skill");
    let (status, data) = h
        .request(
            "POST",
            "/lingxi/v1/skills/install?agentId=agentA",
            json!({"path":source}),
            false,
        )
        .await;
    assert_eq!(status, 200, "{data}");
    let (_, a) = h
        .request(
            "GET",
            "/lingxi/v1/skills?agentId=agentA&runtime=1",
            Value::Null,
            false,
        )
        .await;
    assert_eq!(a["skills"][0]["enabled"], true);
    let (_, b) = h
        .request(
            "GET",
            "/lingxi/v1/skills?agentId=agentB&runtime=1",
            Value::Null,
            false,
        )
        .await;
    assert_eq!(b["skills"][0]["enabled"], false);
    let bytes = zip_package("global-zip");
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let (status, data) = h
        .request(
            "POST",
            "/lingxi/v1/skills/install",
            json!({"file":{"filename":"global.skill","contentBase64":encoded}}),
            false,
        )
        .await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(
        std::fs::read_to_string(h.home.join("skills/global-zip/examples/中文.txt")).unwrap(),
        "压缩配套正文"
    );
    let (_, a) = h
        .request(
            "GET",
            "/lingxi/v1/skills?agentId=agentA&runtime=1",
            Value::Null,
            false,
        )
        .await;
    assert_eq!(
        a["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "global-zip")
            .unwrap()["enabled"],
        false
    );
    let archive = h.home.parent().unwrap().join("workspace.skill");
    std::fs::write(&archive, zip_package("workspace-zip")).unwrap();
    let (status, data) = h
        .request(
            "POST",
            "/lingxi/v1/desk/install-skill",
            json!({"filePath":archive,"dir":h.workspace,"agentId":"agentA"}),
            false,
        )
        .await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(data["installedSkillSource"]["owner"], "workspace");
    assert_eq!(data["name"], "workspace-zip");
    assert!(h
        .workspace
        .join(".agents/skills/workspace-zip/SKILL.md")
        .is_file());
    let (_, catalog) = h
        .request(
            "GET",
            "/lingxi/v1/desk/skills?agentId=agentA",
            Value::Null,
            false,
        )
        .await;
    assert!(catalog.to_string().contains("workspace-zip"), "{catalog}");
    let before = std::fs::read(h.home.join("agents/agentA/config.yaml")).unwrap();
    for (route, payload, ro) in [
        (
            "/lingxi/v1/desk/install-skill",
            json!({"filePath":archive,"dir":h.home}),
            false,
        ),
        (
            "/lingxi/v1/desk/install-skill",
            json!({"filePath":archive}),
            true,
        ),
        (
            "/lingxi/v1/skills/install",
            json!({"file":{"filename":"bad.skill","contentBase64":"@invalid"}}),
            false,
        ),
        (
            "/lingxi/v1/skills/install?agentId=missing",
            json!({"path":source}),
            false,
        ),
    ] {
        let (status, value) = h.request("POST", route, payload, ro).await;
        assert!(status >= 400, "{route}: {value}");
        assert_eq!(
            std::fs::read(h.home.join("agents/agentA/config.yaml")).unwrap(),
            before
        );
    }
    let (status, reloaded) = h
        .request("POST", "/lingxi/v1/skills/reload", json!({}), false)
        .await;
    assert_eq!(status, 200);
    assert!(reloaded["skills"].is_null());
    assert!(reloaded["scanned"].as_u64().unwrap() >= 2);
    let (_, events) = h
        .request("GET", "/lingxi/v1/app-events?since=0", Value::Null, false)
        .await;
    assert!(events["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["type"] == "skills-changed" && event["payload"]["agentId"].is_null()));
    h.close().await;
}
struct Guard;
impl lingxi_service::skills::SkillGuardPort for Guard {
    fn review<'a>(
        &'a self,
        _: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = lingxi_service::skills::SkillReviewOutcome>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            lingxi_service::skills::SkillReviewOutcome::Suspicious("需要明确接受".into())
        })
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_lesson_review_one_shot_content_binding_conflict_and_real_retrieval() {
    let h = Harness::boot().await;
    h.state.skills().set_guard(Arc::new(Guard));
    let service = h.state.skills();
    let args = json!({"name":"learned-pitfall","description":"重试前检查原始输入","lesson":"# 教训\n先确认文件身份再执行。"});
    let before = std::fs::read(h.home.join("agents/agentA/config.yaml")).unwrap();
    let warning = service.learn_lesson("agentA", &args).await.unwrap();
    assert_eq!(warning["requiresRiskConfirmation"], true);
    assert!(!h.home.join("skills/learned-pitfall").exists());
    assert_eq!(
        std::fs::read(h.home.join("agents/agentA/config.yaml")).unwrap(),
        before
    );
    let mut accepted = args.clone();
    accepted["risk_accepted"] = json!(true);
    accepted["risk_confirmation_token"] = warning["riskConfirmationToken"].clone();
    let mut wrong = accepted.clone();
    wrong["lesson"] = json!("已改变的正文");
    let refusal = service.learn_lesson("agentA", &wrong).await.unwrap();
    assert_eq!(refusal["ok"], false);
    assert!(!h.home.join("skills/learned-pitfall").exists());
    // 内容改变会作废旧 token；重新展示当前内容的风险后，必须重新明确接受。
    let renewed = service.learn_lesson("agentA", &args).await.unwrap();
    accepted["risk_confirmation_token"] = renewed["riskConfirmationToken"].clone();
    let success = service.learn_lesson("agentA", &accepted).await.unwrap();
    assert_eq!(success["ok"], true, "{success}");
    assert_eq!(success["riskOverride"], true);
    assert_eq!(success["experienceIndexed"], true);
    let file = h.home.join("skills/learned-pitfall/SKILL.md");
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("先确认文件身份再执行。"));
    let text = std::fs::read_to_string(h.home.join("agents/agentA/config.yaml")).unwrap();
    assert!(text.contains("learned-pitfall"));
    assert!(
        !std::fs::read_to_string(h.home.join("agents/agentB/config.yaml"))
            .unwrap()
            .contains("learned-pitfall")
    );
    let experience =
        lingxi_adapters::storage::agent_memory::ExperienceStore::new(&h.home.join("agents/agentA"));
    assert!(experience
        .index_text_checked()
        .unwrap()
        .contains("learned lessons"));
    assert!(experience
        .list_documents_checked()
        .unwrap()
        .iter()
        .any(|doc| doc.body.contains("learned-pitfall")));
    let (_, catalog) = h
        .request(
            "GET",
            "/lingxi/v1/skills?agentId=agentA&runtime=1",
            Value::Null,
            false,
        )
        .await;
    assert!(catalog["skills"]
        .as_array()
        .unwrap()
        .iter()
        .any(|skill| skill["name"] == "learned-pitfall" && skill["enabled"] == true));
    let written = std::fs::read(&file).unwrap();
    assert!(service.learn_lesson("agentA", &accepted).await.is_err());
    assert_eq!(std::fs::read(&file).unwrap(), written);
    assert!(service.learn_lesson("missing", &args).await.is_err());
    assert!(service
        .learn_lesson(
            "agentA",
            &json!({"name":"../escape","description":"x","lesson":"y"})
        )
        .await
        .is_err());
    let bad_pool = h.home.join("skills");
    let held = h.home.join("held-skills");
    std::fs::rename(&bad_pool, &held).unwrap();
    std::fs::write(&bad_pool, "blocked").unwrap();
    let failed = service
        .learn_lesson(
            "agentA",
            &json!({"name":"cannot-write","description":"x","lesson":"y","risk_accepted":true}),
        )
        .await;
    assert!(failed.is_err());
    assert!(!held.join("cannot-write").exists());
    std::fs::remove_file(&bad_pool).unwrap();
    std::fs::rename(held, bad_pool).unwrap();
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_translation_uses_real_title_model_protocol_accounts_and_refuses_cross_catalog() {
    let h = Harness::boot().await;
    let source = h.package("review-plan");
    h.state.skills().install_manual(&source).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&requests);
    let upstream = tokio::spawn(async move {
        for answer in [
            r#"{"review-plan":"审阅计划"}"#,
            r#"{"unexpected":"不应接受"}"#,
        ] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut block = [0; 4096];
            loop {
                let n = socket.read(&mut block).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&block[..n]);
                if let Some(pos) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..pos]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= pos + 4 + length {
                        let request: Value = serde_json::from_slice(&bytes[pos + 4..]).unwrap();
                        received.lock().unwrap().push(request);
                        break;
                    }
                }
            }
            let frame = json!({"id":"translation","choices":[{"index":0,"delta":{"role":"assistant","content":answer},"finish_reason":"stop"}]});
            let sse = format!(
                "data: {frame}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"id":"translation","choices":[],"usage":{"prompt_tokens":19,"completion_tokens":7}})
            );
            let wire=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",sse.len());
            socket.write_all(wire.as_bytes()).await.unwrap();
        }
    });
    let plane=lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&json!({"providers":{"skills-fixture":{"protocol":"openai-completions","endpoint":format!("http://{address}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-only"}}},"models":{"title":{"provider":"skills-fixture","model":"actual-title-model"}}}).to_string()).unwrap();
    let credential = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &h.home.join("fixture-credentials"),
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let gateway =
        Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane));
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new_with_network(
        gateway,
        credential,
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
        lingxi_adapters::models::network::NetworkPlane::direct_isolated(),
    )
    .unwrap();
    h.state.skills().set_auxiliary(Arc::new(executor));
    for payload in [
        json!({"names":"invalid","lang":"zh"}),
        json!({"names":["review-plan"],"lang":""}),
        json!({"names":["review-plan"],"lang":"en"}),
    ] {
        let (status, value) = h
            .request("POST", "/lingxi/v1/skills/translate", payload, false)
            .await;
        assert_eq!(status, 200);
        assert_eq!(value, json!({}));
    }
    assert!(requests.lock().unwrap().is_empty());
    let (status, value) = h
        .request(
            "POST",
            "/lingxi/v1/skills/translate",
            json!({"names":["review-plan"],"lang":"zh"}),
            false,
        )
        .await;
    assert_eq!(status, 400, "{value}");
    let (status, value) = h
        .request(
            "POST",
            "/lingxi/v1/skills/translate",
            json!({"agentId":"agentA","names":["not-in-catalog"],"lang":"zh"}),
            false,
        )
        .await;
    assert_eq!(status, 400, "{value}");
    let payload = json!({"agentId":"agentA","names":["review-plan"],"lang":"zh"});
    let (status, value) = h
        .request(
            "POST",
            "/lingxi/v1/skills/translate",
            payload.clone(),
            false,
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value, json!({"review-plan":"审阅计划"}));
    let (status, value) = h
        .request("POST", "/lingxi/v1/skills/translate", payload, false)
        .await;
    assert!(status >= 400, "{value}");
    upstream.await.unwrap();
    {
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert!(captured
            .iter()
            .all(|body| body["model"] == "actual-title-model"));
    }
    let records = h
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery::default())
        .await
        .unwrap();
    let records = records
        .iter()
        .filter(|record| record.origin == "skills.translate")
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record.usage.is_some() && record.transport_attempts == Some(1)));
    h.close().await;
}

#[test]
fn original_native_skill_viewer_current_tauri_window_and_read_boundaries() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let target = std::env::var_os("LINGXI_SKILLS_NATIVE_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("需要当前用户的构建缓存位置"))
                .join(".cache/lingxi-r06-builds/tauri-resources")
        });
    assert!(
        target.is_dir(),
        "需要已保留的原生构建缓存；此证明不创建新target"
    );
    let proof = std::env::var_os("LINGXI_SKILLS_NATIVE_PROOF")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-skills-native-proof-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
    let frontend = std::process::Command::new("node")
        .arg(repo.join("node_modules/vite/bin/vite.js"))
        .args([
            "build",
            "--config",
            "desktop/vite.tauri-resources.config.ts",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        frontend.status.success(),
        "{}",
        String::from_utf8_lossy(&frontend.stderr)
    );
    std::fs::create_dir_all(&proof).unwrap();
    let unit = std::process::Command::new(std::env::var_os("CARGO").expect("需要当前Cargo工具链"))
        .args([
            "test",
            "--locked",
            "--manifest-path",
            "desktop/src-tauri/Cargo.toml",
            "--features",
            "e2e-test",
            "--lib",
            "skills::tests::native_skill_reader_rejects_replacement_link_and_binary_content",
            "--",
            "--exact",
            "--show-output",
        ])
        .current_dir(&repo)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0")
        .output()
        .unwrap();
    std::fs::write(proof.join("reader-boundary-stdout.log"), &unit.stdout).unwrap();
    std::fs::write(proof.join("reader-boundary-stderr.log"), &unit.stderr).unwrap();
    assert!(
        unit.status.success(),
        "实际读取替换/二进制边界未通过：{} {}",
        String::from_utf8_lossy(&unit.stdout),
        String::from_utf8_lossy(&unit.stderr)
    );
    assert!(String::from_utf8_lossy(&unit.stdout)
        .contains("test result: ok. 1 passed; 0 failed; 0 ignored;"));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let output =
        std::process::Command::new(std::env::var_os("CARGO").expect("需要当前Cargo工具链"))
            .args([
                "test",
                "--locked",
                "--manifest-path",
                "desktop/src-tauri/Cargo.toml",
                "--features",
                "e2e-test",
                "--test",
                "skill_viewer_host",
                "--",
                "--nocapture",
            ])
            .current_dir(&repo)
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_INCREMENTAL", "0")
            .env("TAURI_WEBDRIVER_PORT", port.to_string())
            .env("LINGXI_SKILLS_NATIVE_PROOF", &proof)
            .output()
            .unwrap();
    std::fs::create_dir_all(&proof).unwrap();
    std::fs::write(proof.join("runner-stdout.log"), &output.stdout).unwrap();
    std::fs::write(proof.join("runner-stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "当前原生技能窗口失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(proof.join("receipt.json").is_file());
    println!("NATIVE_SKILL_CURRENT_PROOF {}", proof.display());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_skills_settings_and_panel_current_real_service_projection() {
    let h = Harness::boot().await;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let global = h.home.parent().unwrap().join("global-ui.skill");
    std::fs::write(&global, zip_package("global-ui")).unwrap();
    let panel = h.home.parent().unwrap().join("panel-ui.skill");
    std::fs::write(&panel, zip_package("panel-ui")).unwrap();
    let external = h.home.parent().unwrap().join("external");
    std::fs::create_dir(&external).unwrap();
    let receipt = h.home.join("ui-receipt.json");
    let output = tokio::process::Command::new("node")
        .arg(repo.join("node_modules/vitest/vitest.mjs"))
        .args([
            "run",
            "desktop/src/react/settings/__tests__/R06SkillsRustLive.test.tsx",
            "desktop/src/react/__tests__/components/SkillsPanel.test.tsx",
            "desktop/src/react/settings/__tests__/R02SharingLeaf.test.tsx",
            "desktop/src/react/settings/tabs/skills/__tests__/SkillBundleTree.test.tsx",
            "desktop/src/react/settings/tabs/__tests__/SkillsTab.test.tsx",
        ])
        .current_dir(&repo)
        .env("R06_SKILLS_URL", &h.url)
        .env("R06_SKILLS_TOKEN", &h.token)
        .env("R06_SKILLS_READONLY", &h.readonly)
        .env("R06_SKILLS_GLOBAL_SOURCE", global)
        .env("R06_SKILLS_PANEL_SOURCE", panel)
        .env("R06_SKILLS_EXTERNAL", external)
        .env(
            "R06_SKILLS_PANEL_INSTALLED",
            h.home.join("skills/panel-ui/SKILL.md"),
        )
        .env("R06_SKILLS_UI_RECEIPT", &receipt)
        .output()
        .await
        .unwrap();
    let evidence = h.home.join("skills-ui-evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(evidence.join("stdout.log"), &output.stdout).unwrap();
    std::fs::write(evidence.join("stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "当前页面失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    assert!(value["checks"].as_array().unwrap().len() >= 12);
    std::fs::write(
        evidence.join("receipt.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    assert!(
        !h.home.join("skills/panel-ui").exists(),
        "页面真实删除后不保留伪安装目录"
    );
    assert!(h.home.join("skills/global-ui/examples/中文.txt").is_file());
    let prefs: Value =
        serde_json::from_slice(&std::fs::read(h.home.join("user/preferences.json")).unwrap())
            .unwrap();
    assert_eq!(prefs["learn_skills"]["enabled"], false);
    h.close().await;
}
