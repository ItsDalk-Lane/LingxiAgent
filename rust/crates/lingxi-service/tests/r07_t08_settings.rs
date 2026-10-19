//! 使用真实服务、偏好存储及模型配置发布链检查设置交换；不发真实供应商请求。
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

struct Harness {
    home: PathBuf,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-settings-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::create_dir_all(home.join("agents/b")).unwrap();
        for agent in ["a", "b"] {
            std::fs::write(
                home.join(format!("agents/{agent}/config.yaml")),
                format!(
                    "agent:\n  name: {agent}\nmemory:\n  enabled: false\nfuture:\n  preserve: 19\n"
                ),
            )
            .unwrap();
        }
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(home.join("user/preferences.json"),serde_json::to_vec(&json!({"current_agent_id":"b","future":{"preserve":17},"search_provider":"tavily","search_api_key":"synthetic-secret-legacy","search_api_keys":{"tavily":"synthetic-secret-tavily","brave":"synthetic-secret-brave"},"headers":{"X-Private":"synthetic-secret-header"}})).unwrap()).unwrap();
        Self::at(home.canonicalize().unwrap()).await
    }
    async fn at(home: PathBuf) -> Self {
        let layout = prepare_layout(&home).unwrap();
        let model_path = layout.runtime_dir.join("models.json");
        let (plane, source) = if model_path.exists() {
            (
                ModelPlaneConfig::parse_and_validate(
                    &std::fs::read_to_string(&model_path).unwrap(),
                )
                .unwrap(),
                Some(lingxi_service::config::ModelPlaneSource::ModelsFile { path: model_path }),
            )
        } else {
            (
                ModelPlaneConfig::from_value(json!({"providers":{},"models":{}})).unwrap(),
                None,
            )
        };
        let active = source.is_some();
        let clock = Arc::new(lingxi_service::inject::SystemClock);
        let credentials = Arc::new(
            lingxi_service::credentials::CredentialService::bootstrap(
                &plane,
                &layout.runtime_dir,
                clock.clone(),
            )
            .unwrap(),
        );
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                model_gateway: Some(Arc::new(ConfigModelGateway::from_validated(plane))),
                credential_service: Some(credentials),
                model_plane_source: source,
                model_plane_activation: Some(Arc::new(AtomicBool::new(active))),
                network_plane: Some(NetworkPlane::direct_isolated()),
                clock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, addr) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let task = tokio::spawn(async move {
            lingxi_service::run(
                serving,
                async {
                    let _ = rx.await;
                },
                |address| {
                    let _ = ready.send(address);
                },
                None,
            )
            .await
            .unwrap();
        });
        Self {
            home,
            state,
            base: format!("http://{}", addr.await.unwrap()),
            stop,
            task,
        }
    }
    async fn request_as(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let raw = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&raw).unwrap_or(json!({"raw":raw})),
        )
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        self.request_as(&self.state.auth().local_token(), method, path, body)
            .await
    }
    fn preferences(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.home.join("user/preferences.json")).unwrap())
            .unwrap()
    }
    async fn stop(self) -> PathBuf {
        let _ = self.stop.send(());
        self.task.await.unwrap();
        self.state.storage().close().await.unwrap();
        self.home
    }
    async fn close(self) {
        let home = self.stop().await;
        std::fs::remove_dir_all(home).unwrap();
    }
}
#[tokio::test]
async fn snapshots_and_default_exports_hide_all_saved_secret_shapes() {
    let h = Harness::boot().await;
    for path in [
        "/lingxi/v1/settings/snapshot?agentId=a",
        "/lingxi/v1/preferences/models",
        "/lingxi/v1/settings/export",
    ] {
        let (status, value) = h.request(reqwest::Method::GET, path, None).await;
        assert_eq!(status, 200, "{value}");
        let text = value.to_string();
        for secret in [
            "synthetic-secret-legacy",
            "synthetic-secret-tavily",
            "synthetic-secret-brave",
            "synthetic-secret-header",
        ] {
            assert!(!text.contains(secret), "secret exposed at {path}");
        }
    }
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/settings/export", None)
        .await;
    assert_eq!(status, 200, "{value}");
    let typed: lingxi_protocol::SettingsArchive = serde_json::from_value(value).unwrap();
    assert_eq!(typed.version, 1);
    assert_eq!(
        h.preferences()["search_api_keys"]["tavily"],
        "synthetic-secret-tavily"
    );
    h.close().await;
}
#[tokio::test]
async fn one_global_edit_and_masked_search_edit_preserve_other_values_and_restart() {
    let h = Harness::boot().await;
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/settings/config",
            Some(json!({"locale":"en","user":{"name":"Shared user"}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/models",
            Some(json!({"search":{"provider":"tavily","api_keys":{"tavily":"********"}}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(h.preferences()["future"]["preserve"], 17);
    assert_eq!(
        h.preferences()["search_api_keys"]["brave"],
        "synthetic-secret-brave"
    );
    assert_eq!(
        h.preferences()["search_api_keys"]["tavily"],
        "synthetic-secret-tavily"
    );
    let home = h.stop().await;
    let h = Harness::at(home).await;
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/settings/config", None)
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["locale"], "en");
    assert_eq!(value["user"]["name"], "Shared user");
    h.close().await;
}
#[tokio::test]
async fn invalid_import_and_unknown_global_fields_never_write_any_preferences() {
    let h = Harness::boot().await;
    let path = h.home.join("user/preferences.json");
    let before = std::fs::read(&path).unwrap();
    for patch in [
        json!({"locale":"en","agent":{"name":"wrong-target"}}),
        json!({"desk":{"heartbeat_master":"false"}}),
        json!({"automation":{"permissionMode":"invalid"}}),
    ] {
        let (status, value) = h
            .request(
                reqwest::Method::PUT,
                "/lingxi/v1/settings/config",
                Some(patch),
            )
            .await;
        assert_eq!(status, 400, "{value}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    for body in [
        json!({"format":"lingxi-settings","version":1,"config":{"locale":"en"},"modelPreferences":{"models":{"embedding":null}},"experiments":{}}),
        json!({"format":"lingxi-settings","version":1,"config":{"locale":"en"},"modelPreferences":{},"experiments":{"unknown.experiment":true}}),
        json!({"format":"lingxi-settings","version":99,"config":{},"modelPreferences":{},"experiments":{}}),
    ] {
        let (status, value) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/settings/import",
                Some(body),
            )
            .await;
        assert_eq!(status, 400, "{value}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    h.close().await;
}
#[tokio::test]
async fn export_import_roundtrip_preserves_masked_secrets_and_unmentioned_preferences() {
    let h = Harness::boot().await;
    let (status, mut archive) = h
        .request(reqwest::Method::GET, "/lingxi/v1/settings/export", None)
        .await;
    assert_eq!(status, 200, "{archive}");
    archive["config"]["locale"] = json!("ja");
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/settings/import",
            Some(archive),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let saved = h.preferences();
    assert_eq!(saved["locale"], "ja");
    assert_eq!(saved["future"]["preserve"], 17);
    assert_eq!(
        saved["search_api_keys"]["tavily"],
        "synthetic-secret-tavily"
    );
    assert_eq!(saved["search_api_keys"]["brave"], "synthetic-secret-brave");
    h.close().await;
}
#[tokio::test]
async fn corrupt_preferences_are_preserved_and_profile_writes_reject_symlinks() {
    let h = Harness::boot().await;
    let path = h.home.join("user/preferences.json");
    std::fs::write(&path, b"{broken-input").unwrap();
    let (status, _) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/settings/config",
            Some(json!({"locale":"ko"})),
        )
        .await;
    assert_eq!(status, 500);
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken-input");
    #[cfg(unix)]
    {
        let target = h.home.join("unrelated.md");
        std::fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, h.home.join("user/user.md")).unwrap();
        let (status, _) = h
            .request(
                reqwest::Method::PUT,
                "/lingxi/v1/user-profile",
                Some(json!({"content":"must not land"})),
            )
            .await;
        assert_eq!(status, 500);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
    }
    h.close().await;
}
#[tokio::test]
async fn workspace_history_uses_explicit_agent_and_profile_survives_restart() {
    let h = Harness::boot().await;
    let folder = h.home.join("workspace");
    std::fs::create_dir(&folder).unwrap();
    let (status, value) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/settings/config/workspaces/recent?agentId=a",
            Some(json!({"path":folder})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["cwd_history"], json!([folder]));
    let a = std::fs::read_to_string(h.home.join("agents/a/config.yaml")).unwrap();
    let b = std::fs::read_to_string(h.home.join("agents/b/config.yaml")).unwrap();
    assert!(a.contains("cwd_history"));
    assert!(!b.contains("cwd_history"));
    assert!(a.contains("preserve: 19"));
    let (status, _) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/settings/config/workspaces/recent",
            Some(json!({"path":folder})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/user-profile",
            Some(json!({"content":"本机测试使用者的真实资料"})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let home = h.stop().await;
    let h = Harness::at(home).await;
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/user-profile", None)
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["content"], "本机测试使用者的真实资料");
    let (status, value) = h
        .request(
            reqwest::Method::DELETE,
            "/lingxi/v1/settings/config/workspaces/recent/all?agentId=a",
            None,
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["cwd_history"], json!([]));
    h.close().await;
}
#[tokio::test]
async fn remote_settings_scope_cannot_use_local_only_settings_archive() {
    let h = Harness::boot().await;
    let issued = h
        .state
        .auth()
        .issue_device_credential("user_local", &["settings.read", "settings.write"], None)
        .unwrap();
    for (method, path, body) in [
        (reqwest::Method::GET, "/lingxi/v1/settings/export", None),
        (
            reqwest::Method::POST,
            "/lingxi/v1/settings/import",
            Some(
                json!({"format":"lingxi-settings","version":1,"config":{},"modelPreferences":{},"experiments":{}}),
            ),
        ),
    ] {
        let (status, _) = h.request_as(&issued.secret, method, path, body).await;
        assert_eq!(status, 403);
    }
    h.close().await;
}

#[tokio::test]
async fn rejected_agent_provider_and_yuan_patches_do_not_save_global_values_first() {
    let h = Harness::boot().await;
    let path = h.home.join("user/preferences.json");
    let before = std::fs::read(&path).unwrap();
    for patch in [
        json!({"locale":"ja","providers":[]}),
        json!({"locale":"ja","agent":{"yuan":"unregistered-unknown-yuan"}}),
    ] {
        let (status, value) = h
            .request(
                reqwest::Method::PUT,
                "/lingxi/v1/agents/a/config",
                Some(patch),
            )
            .await;
        assert_eq!(status, 400, "{value}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    h.close().await;
}

#[tokio::test]
async fn saved_proxy_credentials_never_leave_read_or_export_and_new_unsupported_credentials_are_refused(
) {
    let h = Harness::boot().await;
    let path = h.home.join("user/preferences.json");
    let mut prefs = h.preferences();
    prefs["network_proxy"] = json!({"mode":"manual","httpProxy":"http://fixture-user:synthetic-proxy-secret@127.0.0.1:8888","noProxy":"localhost"});
    std::fs::write(&path, serde_json::to_vec(&prefs).unwrap()).unwrap();
    for endpoint in [
        "/lingxi/v1/settings/config",
        "/lingxi/v1/settings/export",
        "/lingxi/v1/settings/snapshot?agentId=a",
    ] {
        let (status, value) = h.request(reqwest::Method::GET, endpoint, None).await;
        assert_eq!(status, 200, "{value}");
        let text = value.to_string();
        assert!(!text.contains("synthetic-proxy-secret"));
        assert!(!text.contains("fixture-user"));
    }
    let before = std::fs::read(&path).unwrap();
    let(status,_)=h.request(reqwest::Method::PUT,"/lingxi/v1/settings/config",Some(json!({"network_proxy":{"mode":"manual","httpProxy":"http://new-user:new-secret@127.0.0.1:8889"}}))).await;
    assert!(status >= 400);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    h.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn user_profile_parent_directory_link_cannot_redirect_reads_or_writes() {
    let h = Harness::boot().await;
    let outside = h.home.join("outside-user");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("user.md"), "outside-must-stay").unwrap();
    std::fs::rename(h.home.join("user"), h.home.join("user-original")).unwrap();
    std::os::unix::fs::symlink(&outside, h.home.join("user")).unwrap();
    for method in [reqwest::Method::GET, reqwest::Method::PUT] {
        let body = (method == reqwest::Method::PUT).then(|| json!({"content":"must not land"}));
        let (status, value) = h.request(method, "/lingxi/v1/user-profile", body).await;
        assert_eq!(status, 500, "{value}");
    }
    assert_eq!(
        std::fs::read_to_string(outside.join("user.md")).unwrap(),
        "outside-must-stay"
    );
    h.close().await;
}

#[tokio::test]
async fn partial_global_save_still_emits_only_the_changes_that_really_happened() {
    let h = Harness::boot().await;
    for (path, locale) in [
        ("/lingxi/v1/settings/config", "ko"),
        ("/lingxi/v1/agents/a/config", "ja"),
    ] {
        let cursor = h.state.app_events().since(0)["latestSequence"]
            .as_u64()
            .unwrap();
        let (status, result) = h.request(reqwest::Method::PUT, path, Some(json!({"locale":locale,"providers":{"invalid":{"api":"not-supported","base_url":"http://127.0.0.1:1","models":["x"]}}}))).await;
        assert!(status >= 400, "{result}");
        assert_eq!(result["preferencesSaved"], true, "{result}");
        assert_eq!(h.preferences()["locale"], locale);
        let events = h.state.app_events().since(cursor)["events"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(
            events
                .iter()
                .filter(|e| e["type"] == "locale-changed" && e["payload"]["locale"] == locale)
                .count(),
            1,
            "{events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|e| e["type"] == "models-changed" || e["type"] == "agent-updated"),
            "未发布模型或助手不能虚报变化：{events:?}"
        );
    }
    h.close().await;
}

#[tokio::test]
async fn applied_model_projection_error_still_notifies_current_model_consumers() {
    let h = Harness::boot().await;
    let (status, result) = h.request(reqwest::Method::PUT, "/lingxi/v1/settings/config", Some(json!({"providers":{"fixture":{"api":"openai-completions","base_url":"http://127.0.0.1:1/v1","api_key":"synthetic-key","models":[{"id":"chat","inputs":["text"]}]}}}))).await;
    assert_eq!(status, 200, "{result}");
    std::fs::write(
        h.home.join("provider-catalog.json"),
        b"{broken compatibility projection",
    )
    .unwrap();
    let cursor = h.state.app_events().since(0)["latestSequence"]
        .as_u64()
        .unwrap();
    let (status, result) = h.request(reqwest::Method::PUT, "/lingxi/v1/settings/config", Some(json!({"locale":"ko","providers":{"fixture":{"models":[{"id":"chat","inputs":["text"],"contextWindow":4096}]}}}))).await;
    assert!(status >= 400, "{result}");
    assert_eq!(result["code"], "model_projection_unconfirmed", "{result}");
    assert_eq!(result["result"]["applied"], true);
    assert_eq!(result["preferencesSaved"], true);
    let events = h.state.app_events().since(cursor)["events"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "locale-changed")
            .count(),
        1,
        "{events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "models-changed")
            .count(),
        1,
        "{events:?}"
    );
    let snapshot = h
        .state
        .model_configuration()
        .snapshot(
            &h.state
                .auth()
                .authenticate(
                    Some(&format!("Bearer {}", h.state.auth().local_token())),
                    None,
                    false,
                    lingxi_service::ConnectionKind::Local,
                )
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        snapshot["providers"]["fixture"]["models"][0]["compat"]["contextWindow"],
        4096
    );
    h.close().await;
}

#[tokio::test]
async fn partial_proxy_and_editor_updates_publish_complete_saved_values_for_desktop_consumers() {
    let h = Harness::boot().await;
    let initial = json!({
        "network_proxy": {
            "mode":"manual", "httpProxy":"http://127.0.0.1:19001",
            "httpsProxy":"http://127.0.0.1:19002", "wsProxy":"http://127.0.0.1:19003",
            "wssProxy":"http://127.0.0.1:19004", "noProxy":"localhost"
        },
        "editor":{"markdown":{"fontPreset":"serif","bodyFontSize":18,
            "heading1FontSize":32,"lineHeight":1.8,"contentPadding":40,"contentWidth":800}}
    });
    let (status, result) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/settings/config",
            Some(initial),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    for (index, (method, path)) in [
        (reqwest::Method::PUT, "/lingxi/v1/settings/config"),
        (reqwest::Method::PUT, "/lingxi/v1/agents/a/config"),
        (reqwest::Method::POST, "/lingxi/v1/settings/import"),
    ]
    .into_iter()
    .enumerate()
    {
        let previous = h.preferences();
        let cursor = h.state.app_events().since(0)["latestSequence"]
            .as_u64()
            .unwrap();
        let http_proxy = format!("http://127.0.0.1:{}", 19100 + index);
        let body_size = 19 + index;
        let patch = json!({"network_proxy":{"httpProxy":http_proxy},
            "editor":{"markdown":{"bodyFontSize":body_size}}});
        let body = if path.ends_with("/import") {
            json!({"format":"lingxi-settings","version":1,"config":patch,"modelPreferences":{},"experiments":{}})
        } else {
            patch
        };
        let (status, result) = h.request(method, path, Some(body)).await;
        assert_eq!(status, 200, "{path}: {result}");
        let saved = h.preferences();
        assert_eq!(saved["network_proxy"]["httpProxy"], http_proxy);
        for field in ["mode", "httpsProxy", "wsProxy", "wssProxy", "noProxy"] {
            assert_eq!(
                saved["network_proxy"][field], previous["network_proxy"][field],
                "{path}: {field}"
            );
        }
        assert_eq!(
            saved["editor"]["markdown"]["bodyFontSize"].as_f64(),
            Some(body_size as f64)
        );
        for field in [
            "fontPreset",
            "heading1FontSize",
            "heading2FontSize",
            "heading3FontSize",
            "heading4FontSize",
            "heading5FontSize",
            "heading6FontSize",
            "lineHeight",
            "contentPadding",
            "contentWidth",
        ] {
            assert_eq!(
                saved["editor"]["markdown"][field], previous["editor"]["markdown"][field],
                "{path}: {field}"
            );
        }
        let events = h.state.app_events().since(cursor)["events"]
            .as_array()
            .unwrap()
            .clone();
        for (event_type, field) in [
            ("network-proxy-changed", "network_proxy"),
            ("editor-typography-changed", "editor"),
        ] {
            let matches: Vec<_> = events
                .iter()
                .filter(|event| event["type"] == event_type)
                .collect();
            assert_eq!(matches.len(), 1, "{path}: {events:?}");
            assert_eq!(
                matches[0]["payload"][field], saved[field],
                "桌面事件必须携带刚保存的完整设置，不能携带单项补丁：{path}"
            );
        }
    }
    h.close().await;
}

#[tokio::test]
async fn deleting_one_recent_workspace_keeps_the_other_workspace_and_other_agent() {
    let h = Harness::boot().await;
    let folders = [h.home.join("one"), h.home.join("two")];
    for folder in &folders {
        std::fs::create_dir(folder).unwrap();
        let (status, result) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/settings/config/workspaces/recent?agentId=a",
                Some(json!({"path":folder})),
            )
            .await;
        assert_eq!(status, 200, "{result}");
    }
    let other = std::fs::read(h.home.join("agents/b/config.yaml")).unwrap();
    for _ in 0..2 {
        let (status, result) = h
            .request(
                reqwest::Method::DELETE,
                "/lingxi/v1/settings/config/workspaces/recent?agentId=a",
                Some(json!({"path":folders[0]})),
            )
            .await;
        assert_eq!(status, 200, "{result}");
        assert_eq!(result["cwd_history"], json!([folders[1]]));
        assert_eq!(
            std::fs::read(h.home.join("agents/b/config.yaml")).unwrap(),
            other
        );
    }
    assert!(
        folders.iter().all(|path| path.is_dir()),
        "删除历史不能删除实际工作区"
    );
    let bytes = std::fs::read(h.home.join("agents/a/config.yaml")).unwrap();
    let (status, _) = h
        .request(
            reqwest::Method::DELETE,
            "/lingxi/v1/settings/config/workspaces/recent?agentId=a",
            Some(json!({"path":42})),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(
        std::fs::read(h.home.join("agents/a/config.yaml")).unwrap(),
        bytes
    );
    h.close().await;
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn existing_machine_default_workspace_get_and_ensure_do_not_depend_on_service_home_or_modify_user_files(
) {
    let h = Harness::boot().await;
    let expected = PathBuf::from(std::env::var_os("HOME").expect("读取真实系统用户根，不改写环境"))
        .join("Desktop/OH-WorkSpace");
    assert!(
        expected.is_dir(),
        "本机幂等检查要求已有默认目录；不创建或清理用户文件"
    );
    let before = std::fs::metadata(&expected).unwrap();
    for (method, body) in [
        (reqwest::Method::GET, None),
        (reqwest::Method::POST, Some(json!({}))),
        (reqwest::Method::POST, Some(json!({}))),
    ] {
        let is_post = method == reqwest::Method::POST;
        let (status, result) = h
            .request(method, "/lingxi/v1/settings/config/default-workspace", body)
            .await;
        assert_eq!(status, 200, "{result}");
        assert_eq!(result["path"], json!(expected));
        if is_post {
            assert_eq!(result["ok"], true);
        }
    }
    let after = std::fs::metadata(&expected).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!(
        (
            before.dev(),
            before.ino(),
            before.mtime(),
            before.mtime_nsec()
        ),
        (after.dev(), after.ino(), after.mtime(), after.mtime_nsec())
    );
    assert_ne!(expected, h.home.join("Desktop/OH-WorkSpace"));
    h.close().await;
}
