//! 真实HTTP设置写入绑定明确助手；链接或目标失效不能改焦点和根外文件。
#![cfg(unix)]
use lingxi_service::{prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceState};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
};
struct Harness {
    scratch: PathBuf,
    home: PathBuf,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn boot() -> Self {
        Self::boot_model_plane(false).await
    }
    async fn boot_model_plane(wired: bool) -> Self {
        let scratch = std::env::temp_dir().join(format!(
            "lingxi-t08-agent-target-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        let home = scratch.join("home");
        for agent in ["a", "b"] {
            std::fs::create_dir_all(home.join(format!("agents/{agent}"))).unwrap();
            std::fs::write(
                home.join(format!("agents/{agent}/config.yaml")),
                format!(
                    "agent:\n  name: {agent}\nmemory:\n  enabled: false\nfuture:\n  keep: 19\nbridge:\n  telegram:\n    enabled: false\n  feishu:\n    enabled: false\n  dingtalk:\n    enabled: false\n  qq:\n    enabled: false\n  wechat:\n    enabled: false\n"
                ),
            )
            .unwrap();
        }
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            br#"{"current_agent_id":"a"}"#,
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let scratch = scratch.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let mut deps = lingxi_service::ServiceDeps::default();
        if wired {
            let plane = lingxi_adapters::models::config::ModelPlaneConfig::from_value(
                json!({"providers":{},"models":{}}),
            )
            .unwrap();
            deps.model_gateway = Some(std::sync::Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane.clone()),
            ));
            deps.credential_service = Some(std::sync::Arc::new(
                lingxi_service::credentials::CredentialService::bootstrap(
                    &plane,
                    &layout.runtime_dir,
                    deps.clock.clone(),
                )
                .unwrap(),
            ));
            deps.network_plane =
                Some(lingxi_adapters::models::network::NetworkPlane::direct_isolated());
            deps.model_plane_activation = Some(std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ));
        }
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            deps,
        )
        .await
        .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, address) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let server = tokio::spawn(async move {
            lingxi_service::run(
                serving,
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
        let address = address.await.unwrap();
        // 监听就绪并不表示异步旧配置导入已结束。等两助手的五个平台均有真实
        // 禁用记录后再破坏路径，避免把启动导入失败混入本组HTTP写入边界检查。
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let connectors = lingxi_adapters::storage::bridge::list_connectors(state.storage())
                    .await
                    .unwrap();
                if connectors.len() == 10 {
                    for agent in ["a", "b"] {
                        for platform in lingxi_service::bridge_routes::PLATFORMS {
                            assert!(connectors.iter().any(|row| {
                                row.input.actor_id == agent
                                    && row.input.platform == platform
                                    && !row.input.enabled
                            }));
                        }
                    }
                    break;
                }
                assert!(!server.is_finished(), "服务在完成初次导入前已退出");
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("真实桥接初次导入未完成，不开始路径故障注入");
        Self {
            scratch,
            home,
            state,
            base: format!("http://{address}"),
            stop,
            server,
        }
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.state.auth().local_token());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let bytes = response.bytes().await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)})),
        )
    }
    async fn close(self) {
        let _ = self.stop.send(());
        let result = self.server.await.expect("服务任务异常退出");
        let outcome = result.unwrap_or_else(|error| {
            panic!(
                "service stop failed: {error:?}; evidence root={}",
                self.scratch.display()
            )
        });
        assert!(!outcome.drain_timed_out, "服务清理未完成");
        self.state.storage().close().await.unwrap();
        drop(self.state);
        std::fs::remove_dir_all(self.scratch).unwrap();
    }
}
#[tokio::test]
async fn missing_explicit_agent_is_not_recreated_or_redirected_to_focus_and_valid_target_is_private(
) {
    let h = Harness::boot().await;
    let a = h.home.join("agents/a/config.yaml");
    let b = h.home.join("agents/b/config.yaml");
    let a_before = std::fs::read(&a).unwrap();
    let b_before = std::fs::read(&b).unwrap();
    std::fs::remove_file(&b).unwrap();
    for (method, path, body) in [
        (
            reqwest::Method::PUT,
            "/lingxi/v1/agents/b/config",
            json!({"agent":{"name":"must-not-reach-A"}}),
        ),
        (
            reqwest::Method::POST,
            "/lingxi/v1/settings/config/workspaces/recent?agentId=b",
            json!({"path":h.home}),
        ),
    ] {
        let (status, value) = h.request(method, path, Some(body)).await;
        assert!(status >= 400, "{value}");
        assert_eq!(std::fs::read(&a).unwrap(), a_before);
        assert!(!b.exists());
    }
    std::fs::write(&b, &b_before).unwrap();
    std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/agents/b/config",
            Some(json!({"agent":{"name":"B actually saved"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let b_doc = lingxi_adapters::storage::agent_memory::config::read_yaml_mapping_as_json(&b)
        .unwrap()
        .unwrap();
    assert_eq!(b_doc["agent"]["name"], "B actually saved");
    assert_eq!(b_doc["future"]["keep"], 19);
    assert_eq!(std::fs::read(&a).unwrap(), a_before);
    assert_eq!(
        std::fs::metadata(&b).unwrap().permissions().mode() & 0o777,
        0o600
    );
    h.close().await;
}
#[tokio::test]
async fn linked_agent_directory_agents_root_or_config_never_writes_outside_for_present_or_missing_targets(
) {
    for boundary in ["agent", "agents", "config"] {
        for target_exists in [true, false] {
            let h = Harness::boot().await;
            let a = h.home.join("agents/a/config.yaml");
            let a_before = std::fs::read(&a).unwrap();
            let outside = h.scratch.join("outside");
            std::fs::create_dir(&outside).unwrap();
            let original = match boundary {
                "agent" => h.home.join("agents/b"),
                "agents" => h.home.join("agents"),
                _ => h.home.join("agents/b/config.yaml"),
            };
            let retained = h.home.join(format!("retained-{boundary}"));
            std::fs::rename(&original, &retained).unwrap();
            let target = match boundary {
                "agents" => {
                    std::fs::create_dir(outside.join("b")).unwrap();
                    outside.clone()
                }
                "agent" => outside.clone(),
                _ => outside.join("config.yaml"),
            };
            let outside_config = if boundary == "agents" {
                outside.join("b/config.yaml")
            } else {
                outside.join("config.yaml")
            };
            let original_bytes = b"agent:\n  name: outside-must-stay\nmemory:\n  enabled: false\n";
            if target_exists {
                std::fs::write(&outside_config, original_bytes).unwrap();
            }
            symlink(&target, &original).unwrap();
            for method in [reqwest::Method::GET, reqwest::Method::PUT] {
                let body = (method == reqwest::Method::PUT)
                    .then(|| json!({"agent":{"name":"must-not-write-outside"}}));
                let (status, value) = h.request(method, "/lingxi/v1/agents/b/config", body).await;
                assert!(status >= 400, "{boundary}/{target_exists}: {value}");
            }
            let (status, value) = h
                .request(
                    reqwest::Method::POST,
                    "/lingxi/v1/settings/config/workspaces/recent?agentId=b",
                    Some(json!({"path":h.home})),
                )
                .await;
            assert!(status >= 400, "{boundary}/{target_exists}: {value}");
            if target_exists {
                assert_eq!(std::fs::read(&outside_config).unwrap(), original_bytes);
            } else {
                assert!(!outside_config.exists());
            }
            std::fs::remove_file(&original).unwrap();
            std::fs::rename(&retained, &original).unwrap();
            assert_eq!(std::fs::read(&a).unwrap(), a_before);
            h.close().await;
        }
    }
}

#[tokio::test]
async fn agent_get_uses_published_models_after_projection_failure_instead_of_legacy_catalog() {
    let h = Harness::boot_model_plane(true).await;
    let (status,result)=h.request(reqwest::Method::PUT,"/lingxi/v1/settings/config",Some(json!({"providers":{"fixture":{"api":"openai-completions","base_url":"http://127.0.0.1:9/v1","api_key":"synthetic-agent-readback-secret","models":[{"id":"unchanged","inputs":["text"]},{"id":"updated","name":"old metadata","inputs":["text"],"contextWindow":4096}]}}}))).await;
    assert_eq!(status, 200, "{result}");
    let catalog = h.home.join("provider-catalog.json");
    let damaged = b"{damaged compatibility projection";
    std::fs::write(&catalog, damaged).unwrap();
    let (status, result) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/providers/fixture/models/updated",
            Some(
                json!({"name":"actually published metadata","contextWindow":16384,"maxTokens":512}),
            ),
        )
        .await;
    assert_eq!(status, 500, "{result}");
    assert_eq!(result["code"], "model_projection_unconfirmed");
    assert_eq!(result["result"]["saved"], true);
    assert_eq!(result["result"]["applied"], true);
    let (status, config) = h
        .request(reqwest::Method::GET, "/lingxi/v1/agents/b/config", None)
        .await;
    assert_eq!(status, 200, "{config}");
    let provider = &config["providers"]["fixture"];
    assert_eq!(provider["model_count"], 2);
    let model = provider["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "updated")
        .unwrap();
    assert_eq!(model["name"], "actually published metadata");
    assert_eq!(model["compat"]["contextWindow"], 16384);
    assert_eq!(model["compat"]["maxTokens"], 512);
    assert!(!config
        .to_string()
        .contains("synthetic-agent-readback-secret"));
    let (status, snapshot) = h
        .request(
            reqwest::Method::GET,
            "/lingxi/v1/settings/snapshot?agentId=b",
            None,
        )
        .await;
    assert_eq!(status, 200, "{snapshot}");
    assert_eq!(snapshot["config"]["providers"]["fixture"]["model_count"], 2);
    assert_eq!(
        std::fs::read(&catalog).unwrap(),
        damaged,
        "读面不能重写或掩盖坏掉的兼容文件"
    );
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/agents/b/config", h.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    h.close().await;
}

#[tokio::test]
async fn unwired_agent_get_marks_inactive_and_does_not_promote_legacy_only_catalog() {
    let h = Harness::boot().await;
    let catalog = h.home.join("provider-catalog.json");
    let legacy = json!({"catalogVersion":2,"providers":{"legacy-only":{"api":"openai-completions","base_url":"http://127.0.0.1:9/v1","api_key":"synthetic-legacy-only-secret","models":[{"id":"must-not-become-runtime"}]}}});
    let before = serde_json::to_vec_pretty(&legacy).unwrap();
    std::fs::write(&catalog, &before).unwrap();
    let (status, result) = h
        .request(reqwest::Method::GET, "/lingxi/v1/agents/b/config", None)
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["modelConfiguration"]["activated"], false);
    assert!(result["providers"].as_object().unwrap().is_empty());
    assert!(!result.to_string().contains("must-not-become-runtime"));
    assert_eq!(std::fs::read(catalog).unwrap(), before);
    h.close().await;
}
