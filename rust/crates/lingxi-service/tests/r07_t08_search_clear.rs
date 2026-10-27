//! 真实HTTP清除网络搜索凭据：旧单密钥属于当前供应商，逐供应商清除不借用其它材料；不执行记忆搜索或付费搜索。
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
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn boot(legacy_only: bool) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-search-clear-{}",
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
        let mut prefs = json!({"current_agent_id":"b","future":{"preserve":17},"search_provider":"tavily","search_api_key":"search-clear-legacy","search_api_keys":{"brave":"search-clear-brave","future-provider":"search-clear-future"}});
        if !legacy_only {
            prefs["search_api_keys"]["tavily"] = json!("search-clear-tavily");
        }
        std::fs::write(
            home.join("user/preferences.json"),
            serde_json::to_vec(&prefs).unwrap(),
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        eprintln!("搜索凭据清除现场：{}", home.display());
        Self::at(home).await
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
        self.stop.send(()).expect("真实服务仍应运行");
        let outcome = self
            .task
            .await
            .expect("真实服务不可崩溃")
            .expect("真实服务退出必须成功");
        assert!(!outcome.drain_timed_out, "重启证明不能建立在未确认退出上");
        self.state.storage().close().await.unwrap();
        self.home
    }
    async fn close(self) {
        let home = self.stop().await;
        std::fs::remove_dir_all(home).unwrap();
    }
}

const SECRETS: [&str; 4] = [
    "search-clear-legacy",
    "search-clear-tavily",
    "search-clear-brave",
    "search-clear-future",
];
async fn read_search(h: &Harness) -> Value {
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/preferences/models", None)
        .await;
    assert_eq!(status, 200, "{value}");
    for path in [
        "/lingxi/v1/settings/snapshot?agentId=a",
        "/lingxi/v1/settings/config",
        "/lingxi/v1/settings/export",
    ] {
        let (status, projection) = h.request(reqwest::Method::GET, path, None).await;
        assert_eq!(status, 200, "{path}: {projection}");
        for secret in SECRETS {
            assert!(
                !projection.to_string().contains(secret),
                "读面泄露搜索凭据：{path}"
            );
        }
    }
    for secret in SECRETS {
        assert!(
            !value.to_string().contains(secret),
            "模型偏好读面泄露搜索凭据"
        );
    }
    value["search"].clone()
}
fn preserved(h: &Harness) {
    let prefs = h.preferences();
    assert_eq!(prefs["current_agent_id"], "b");
    assert_eq!(prefs["future"], json!({"preserve":17}));
    assert_eq!(
        prefs["search_api_keys"]["future-provider"],
        "search-clear-future"
    );
    for agent in ["a", "b"] {
        let config =
            std::fs::read_to_string(h.home.join(format!("agents/{agent}/config.yaml"))).unwrap();
        assert_eq!(
            config,
            format!(
                "agent:\n  name: {agent}\nmemory:\n  enabled: false\nfuture:\n  preserve: 19\n"
            )
        );
    }
}
async fn clear(h: &Harness, search: Value) {
    let (status, body) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/models",
            Some(json!({"search":search})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["preferencesSaved"], true);
    for secret in SECRETS {
        assert!(!body.to_string().contains(secret));
    }
    preserved(h);
}
async fn restarted(h: Harness, expected: &Value) -> Harness {
    let h = Harness::at(h.stop().await).await;
    assert_eq!(
        &h.preferences(),
        expected,
        "重启不得恢复已清除的旧密钥或覆盖其它供应商"
    );
    preserved(&h);
    h
}

async fn seed_mixed_model_search(h: &Harness) -> Value {
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/settings/config",
            Some(json!({"providers":{"mixed-fixture":{"api":"openai-completions","base_url":"http://127.0.0.1:1/v1","models":[{"id":"first","inputs":["text"]},{"id":"second","inputs":["text"]}]}}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/models",
            Some(json!({"models":{"title":{"provider":"mixed-fixture","id":"first"}}})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    let (status, value) = h
        .request(reqwest::Method::GET, "/lingxi/v1/preferences/models", None)
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["models"]["title"]["id"], "first");
    assert_eq!(value["search"]["provider"], "tavily");
    assert_mixed_runtime(h, "first");
    value
}

fn assert_mixed_runtime(h: &Harness, model: &str) {
    let routes = h.state.model_gateway().unwrap().configured_routes();
    let title = routes.get("title").unwrap();
    assert_eq!(title.provider, "mixed-fixture");
    assert_eq!(title.model, model);
}

#[tokio::test]
async fn mixed_models_search_validation_failure_reads_both_sections_and_unchanged_runtime() {
    let h = Harness::boot(false).await;
    let before = seed_mixed_model_search(&h).await;
    let prefs_before = std::fs::read(h.home.join("user/preferences.json")).unwrap();
    let source = prepare_layout(&h.home)
        .unwrap()
        .runtime_dir
        .join("models.json");
    let source_before = std::fs::read(&source).unwrap();
    let cursor = h.state.app_events().since(0)["latestSequence"]
        .as_u64()
        .unwrap();
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/models",
            Some(json!({"models":{"title":{"provider":"mixed-fixture","id":"not-registered"}},"search":{"provider":"brave","api_keys":{"tavily":null}}})),
        )
        .await;
    assert_eq!(status, 400, "{value}");
    assert_eq!(value["preferencesSaved"], false, "{value}");
    let (status, after) = h
        .request(reqwest::Method::GET, "/lingxi/v1/preferences/models", None)
        .await;
    assert_eq!(status, 200, "{after}");
    assert_eq!(after["models"], before["models"]);
    assert_eq!(after["search"], before["search"]);
    assert_mixed_runtime(&h, "first");
    assert_eq!(std::fs::read(&source).unwrap(), source_before);
    assert_eq!(
        std::fs::read(h.home.join("user/preferences.json")).unwrap(),
        prefs_before
    );
    assert!(!h.state.app_events().since(cursor)["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["type"] == "models-changed"));
    for secret in SECRETS {
        assert!(!value.to_string().contains(secret));
        assert!(!after.to_string().contains(secret));
    }
    preserved(&h);
    h.close().await;
}

#[tokio::test]
async fn mixed_models_search_projection_failure_keeps_applied_model_and_unapplied_search_after_restart(
) {
    let h = Harness::boot(false).await;
    let before = seed_mixed_model_search(&h).await;
    let prefs_before = std::fs::read(h.home.join("user/preferences.json")).unwrap();
    let source = prepare_layout(&h.home)
        .unwrap()
        .runtime_dir
        .join("models.json");
    let source_before = std::fs::read(&source).unwrap();
    let catalog = h.home.join("provider-catalog.json");
    let catalog_before = std::fs::read(&catalog).unwrap();
    // 只破坏真实兼容投影：权威来源与运行时先发布，随后投影报错，搜索偏好尚未保存。
    std::fs::write(&catalog, b"{broken compatibility projection").unwrap();
    let cursor = h.state.app_events().since(0)["latestSequence"]
        .as_u64()
        .unwrap();
    let (status, value) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/models",
            Some(json!({"models":{"title":{"provider":"mixed-fixture","id":"second"}},"search":{"provider":"brave","api_keys":{"tavily":""}}})),
        )
        .await;
    assert_eq!(status, 500, "{value}");
    assert_eq!(value["code"], "model_projection_unconfirmed", "{value}");
    assert_eq!(value["result"]["applied"], true, "{value}");
    assert_eq!(value["preferencesSaved"], false, "{value}");
    let (status, after) = h
        .request(reqwest::Method::GET, "/lingxi/v1/preferences/models", None)
        .await;
    assert_eq!(status, 200, "{after}");
    assert_eq!(
        after["models"]["title"],
        json!({"provider":"mixed-fixture","id":"second"})
    );
    assert_eq!(after["search"], before["search"]);
    assert_mixed_runtime(&h, "second");
    assert_ne!(std::fs::read(&source).unwrap(), source_before);
    assert_eq!(
        std::fs::read(h.home.join("user/preferences.json")).unwrap(),
        prefs_before
    );
    assert_eq!(
        h.state.app_events().since(cursor)["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["type"] == "models-changed")
            .count(),
        1
    );
    for secret in SECRETS {
        assert!(!value.to_string().contains(secret));
        assert!(!after.to_string().contains(secret));
    }
    std::fs::write(&catalog, catalog_before).unwrap();
    let saved = h.preferences();
    let h = restarted(h, &saved).await;
    let (status, restarted) = h
        .request(reqwest::Method::GET, "/lingxi/v1/preferences/models", None)
        .await;
    assert_eq!(status, 200, "{restarted}");
    assert_eq!(restarted["models"]["title"], after["models"]["title"]);
    assert_eq!(restarted["search"], before["search"]);
    assert_mixed_runtime(&h, "second");
    h.close().await;
}

#[tokio::test]
async fn legacy_search_api_key_empty_and_null_clear_only_the_selected_provider_and_survive_restart()
{
    for clear_value in [json!(""), Value::Null] {
        let h = Harness::boot(true).await;
        assert!(
            h.preferences()["search_api_keys"].get("tavily").is_none(),
            "夹具必须只靠旧单密钥提供当前tavily凭据"
        );
        let initial = read_search(&h).await;
        assert_eq!(initial["provider"], "tavily");
        assert_eq!(initial["api_key"], "********");
        assert_eq!(initial["api_keys"]["tavily"], "********");
        clear(&h, json!({"api_key":clear_value})).await;
        let saved = h.preferences();
        assert_eq!(saved["search_provider"], "tavily");
        assert_eq!(saved["search_api_key"], "");
        assert!(
            saved["search_api_keys"].get("tavily").is_none(),
            "不能保留会在重启时复活的selected-provider备份"
        );
        assert_eq!(saved["search_api_keys"]["brave"], "search-clear-brave");
        let view = read_search(&h).await;
        assert_eq!(view["api_key"], "");
        assert_eq!(view["api_keys"]["tavily"], "");
        assert_eq!(view["api_keys"]["brave"], "********");
        let h = restarted(h, &saved).await;
        assert_eq!(read_search(&h).await, view);
        h.close().await;
    }
}

#[tokio::test]
async fn per_provider_search_clear_empty_and_null_remove_the_selected_key_without_legacy_resurrection(
) {
    for clear_value in [json!(""), Value::Null] {
        let h = Harness::boot(false).await;
        assert_eq!(h.preferences()["search_api_key"], "search-clear-legacy");
        assert_eq!(
            h.preferences()["search_api_keys"]["tavily"],
            "search-clear-tavily",
            "独立表材料优先，旧字段不能越过明确清除"
        );
        clear(&h, json!({"api_keys":{"tavily":clear_value}})).await;
        let saved = h.preferences();
        assert!(saved["search_api_keys"].get("tavily").is_none());
        assert_eq!(saved["search_api_key"], "");
        assert_eq!(saved["search_api_keys"]["brave"], "search-clear-brave");
        let cleared = read_search(&h).await;
        assert_eq!(cleared["api_key"], "");
        assert_eq!(cleared["api_keys"]["tavily"], "");
        assert_eq!(cleared["api_keys"]["brave"], "********");
        let h = restarted(h, &saved).await;
        assert_eq!(read_search(&h).await, cleared);
        clear(&h, json!({"provider":"brave"})).await;
        assert_eq!(h.preferences()["search_api_key"], "search-clear-brave");
        assert!(h.preferences()["search_api_keys"].get("tavily").is_none());
        let view = read_search(&h).await;
        assert_eq!(view["provider"], "brave");
        assert_eq!(view["api_key"], "********");
        let saved = h.preferences();
        let h = restarted(h, &saved).await;
        assert_eq!(read_search(&h).await, view);
        h.close().await;
    }
}

#[tokio::test]
async fn per_provider_search_clear_of_inactive_key_preserves_active_key_and_never_borrows_it_after_switch(
) {
    for clear_value in [json!(""), Value::Null] {
        let h = Harness::boot(false).await;
        clear(&h, json!({"api_keys":{"brave":clear_value}})).await;
        let saved = h.preferences();
        assert!(saved["search_api_keys"].get("brave").is_none());
        assert_eq!(saved["search_api_keys"]["tavily"], "search-clear-tavily");
        assert_eq!(saved["search_api_key"], "search-clear-tavily");
        let view = read_search(&h).await;
        assert_eq!(view["api_key"], "********");
        assert_eq!(view["api_keys"]["tavily"], "********");
        assert_eq!(view["api_keys"]["brave"], "");
        let h = restarted(h, &saved).await;
        assert_eq!(read_search(&h).await, view);
        clear(&h, json!({"provider":"brave"})).await;
        assert_eq!(h.preferences()["search_api_key"], "");
        assert_eq!(
            h.preferences()["search_api_keys"]["tavily"],
            "search-clear-tavily"
        );
        let view = read_search(&h).await;
        assert_eq!(view["provider"], "brave");
        assert_eq!(view["api_key"], "");
        assert_eq!(view["api_keys"]["tavily"], "********");
        let saved = h.preferences();
        let h = restarted(h, &saved).await;
        assert_eq!(read_search(&h).await, view);
        h.close().await;
    }
}
