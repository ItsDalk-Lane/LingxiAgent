//! 模型合法扩展字段的真实 Source/HTTP 遮盖与按身份回填；不替换授权或存储。
//! 仅使用隔离目录与合成秘密，不发任何外部平台请求。
use lingxi_adapters::models::{
    config::ModelPlaneConfig, credentials::ProviderCredentialPort, gateway::ConfigModelGateway,
    network::NetworkPlane,
};
use lingxi_kernel::model_exchange::{ModelGatewayPort, ModelOperation, ModelRouteRequest};
use lingxi_service::{
    config::ModelPlaneSource, model_configuration::ModelSettingsChange, prepare_layout, HomeSource,
    NetworkMode, Principal, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

const FIELDS: [&str; 3] = ["api_key", "access_token", "refresh_token"];
fn extension(id: &str) -> Value {
    json!({"api_key":format!("synthetic-{id}-api"),"access_token":format!("synthetic-{id}-access"),"refresh_token":format!("synthetic-{id}-refresh"),"headers":{"X-Synthetic":format!("synthetic-{id}-header")},"api_keys":{"nested":format!("synthetic-{id}-bucket")},"ordinary":{"keep":true,"label":id},"access_tokenLabel":"普通显示字段","threshold":19})
}
struct Harness {
    home: PathBuf,
    source: PathBuf,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    serving:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn boot() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-metadata-redaction-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            r#"{"current_agent_id":"a","unrelated":{"keep":7}}"#,
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let source = home.join("model-plane.json");
        let document = json!({"providers":{"fixture":{"protocol":"openai-completions","endpoint":"http://127.0.0.1:9/v1","auth":{"kind":"none"},"models":[{"id":"alpha","inputs":["text"],"extension":extension("alpha")},{"id":"beta","inputs":["text"],"extension":extension("beta")}],"metadata":{"ordinaryProvider":{"keep":true}}}},"models":{},"network":{"proxy":{"mode":"direct"}}});
        // 预置真实旧 Source 中合法的未知扩展，不能靠新写入校验禁止它来掩盖读面泄漏。
        std::fs::write(&source, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        let plane = ModelPlaneConfig::from_value(document).unwrap();
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
                shutdown_timeout_ms: 2000,
            },
            &layout,
            ServiceDeps {
                clock,
                model_gateway: Some(Arc::new(ConfigModelGateway::from_validated(plane))),
                credential_service: Some(credentials),
                network_plane: Some(NetworkPlane::direct_isolated()),
                model_plane_source: Some(ModelPlaneSource::ModelsFile {
                    path: source.clone(),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, addr) = tokio::sync::oneshot::channel();
        let service = state.clone();
        let serving = tokio::spawn(async move {
            lingxi_service::run(
                service,
                async {
                    let _ = rx.await;
                },
                |bound| {
                    let _ = ready.send(bound);
                },
                None,
            )
            .await
        });
        let base = format!("http://{}", addr.await.unwrap());
        eprintln!("模型扩展遮盖真实现场：{}", home.display());
        Self {
            home,
            source,
            state,
            base,
            stop,
            serving,
        }
    }
    fn principal(&self, token: &str) -> Principal {
        self.state
            .auth()
            .authenticate(
                Some(&format!("Bearer {token}")),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap()
    }
    fn source(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.source).unwrap()).unwrap()
    }
    fn model(&self, id: &str) -> Value {
        self.source()["providers"]["fixture"]["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == id)
            .unwrap()
            .clone()
    }
    fn generation(&self) -> u64 {
        self.state.model_gateway().unwrap().config_generation()
    }
    async fn request(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token)
            .timeout(Duration::from_secs(10));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw":text})),
        )
    }
    async fn save_models(&self, models: Vec<Value>) -> Value {
        let (status, body) = self
            .request(
                &self.state.auth().local_token(),
                reqwest::Method::PUT,
                "/lingxi/v1/agents/a/config",
                Some(json!({"providers":{"fixture":{"models":models}}})),
            )
            .await;
        assert_eq!(status, 200, "遮盖回写或合法秘密变更须真实成功：{body}");
        assert_eq!(body["modelConfiguration"]["saved"], true, "{body}");
        assert_eq!(body["modelConfiguration"]["applied"], true, "{body}");
        body
    }
    async fn masked_models(&self) -> Vec<Value> {
        let (status, body) = self
            .request(
                &self.state.auth().local_token(),
                reqwest::Method::GET,
                "/lingxi/v1/settings/config",
                None,
            )
            .await;
        assert_eq!(status, 200, "{body}");
        body["providers"]["fixture"]["models"]
            .as_array()
            .unwrap()
            .clone()
    }
    async fn close(self) {
        self.stop.send(()).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(8), self.serving)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!outcome.drain_timed_out);
        self.state.storage().close().await.unwrap();
        std::fs::remove_dir_all(self.home).unwrap();
    }
}
fn assert_masked_view(body: &Value, export: bool) {
    let providers = if export {
        &body["config"]["providers"]
    } else {
        &body["providers"]
    };
    let models = providers["fixture"]["models"]
        .as_array()
        .expect("必须保留合法模型目录和普通扩展");
    let serialized = serde_json::to_string(body).unwrap();
    for id in ["alpha", "beta"] {
        let model = models.iter().find(|m| m["id"] == id).unwrap();
        let ext = &model["extension"];
        assert_eq!(ext["ordinary"], json!({"keep":true,"label":id}));
        assert_eq!(ext["access_tokenLabel"], "普通显示字段");
        assert_eq!(ext["threshold"], 19);
        for field in FIELDS {
            assert_eq!(ext[field], "********", "已知扩展秘密必须精确遮盖：{field}");
        }
        assert_eq!(ext["headers"]["X-Synthetic"], "********");
        assert_eq!(ext["api_keys"]["nested"], "********");
        for secret in [
            format!("synthetic-{id}-api"),
            format!("synthetic-{id}-access"),
            format!("synthetic-{id}-refresh"),
            format!("synthetic-{id}-header"),
            format!("synthetic-{id}-bucket"),
        ] {
            assert!(!serialized.contains(&secret), "整个读面不能出现任何原秘密");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metadata_reads_mask_known_nested_secrets_and_masked_model_lists_refill_only_same_id() {
    let h = Harness::boot().await;
    let original = h.source();
    let owner = h.state.auth().local_token();
    let read_only = h
        .state
        .auth()
        .issue_device_credential("user_local", &["settings.read"], None)
        .unwrap();
    for (path, export) in [
        ("/lingxi/v1/providers/summary", false),
        ("/lingxi/v1/settings/config", false),
        ("/lingxi/v1/settings/export", true),
    ] {
        let (status, body) = h.request(&owner, reqwest::Method::GET, path, None).await;
        assert_eq!(status, 200, "{path}: {body}");
        assert_masked_view(&body, export);
        let (status, body) = h
            .request(&read_only.secret, reqwest::Method::GET, path, None)
            .await;
        if export {
            assert_eq!(
                status, 403,
                "归档仍是本机主人权限，不扩大到只读设备：{body}"
            );
        } else {
            assert_eq!(status, 200, "{path}: {body}");
            assert_masked_view(&body, false);
        }
    }
    assert_eq!(h.source(), original, "读面脱敏不得写回清空真实Source");
    let masked = h.masked_models().await;
    h.save_models(masked).await;
    for id in ["alpha", "beta"] {
        assert_eq!(
            h.model(id)["extension"],
            extension(id),
            "遮盖回写保留原id全部秘密与未知字段"
        );
    }
    let mut reordered = h.masked_models().await;
    reordered.reverse();
    h.save_models(reordered).await;
    assert_eq!(
        h.source()["providers"]["fixture"]["models"][0]["id"],
        "beta",
        "必须实际持久化重排，不能忽略请求制造假保留"
    );
    for id in ["alpha", "beta"] {
        assert_eq!(
            h.model(id)["extension"],
            extension(id),
            "模型重排不能按旧位置回填秘密"
        );
    }
    let mut models = h.masked_models().await;
    let mut gamma = models.iter().find(|m| m["id"] == "alpha").unwrap().clone();
    gamma["id"] = json!("gamma");
    gamma["extension"]["ordinary"]["label"] = json!("gamma");
    models.insert(0, gamma);
    h.save_models(models).await;
    let gamma = h.model("gamma");
    for field in FIELDS {
        assert_eq!(
            gamma["extension"][field], "",
            "新id没有原秘密，不能借别的id值或把星号保存为材料"
        );
    }
    assert_eq!(gamma["extension"]["headers"]["X-Synthetic"], "");
    assert_eq!(gamma["extension"]["api_keys"]["nested"], "");
    assert_eq!(
        gamma["extension"]["ordinary"],
        json!({"keep":true,"label":"gamma"})
    );
    for id in ["alpha", "beta"] {
        assert_eq!(h.model(id)["extension"], extension(id));
    }
    let mut models = h.masked_models().await;
    let alpha = models.iter_mut().find(|m| m["id"] == "alpha").unwrap();
    alpha["extension"]["api_key"] = json!("");
    alpha["extension"]["access_token"] = Value::Null;
    alpha["extension"]["refresh_token"] = json!("");
    alpha["extension"]["headers"]["X-Synthetic"] = json!("");
    alpha["extension"]["api_keys"]["nested"] = Value::Null;
    h.save_models(models).await;
    let cleared = h.model("alpha");
    for field in FIELDS {
        assert!(
            cleared["extension"][field].is_null() || cleared["extension"][field] == "",
            "主人显式清空必须实际写入Source"
        );
    }
    assert_eq!(cleared["extension"]["headers"]["X-Synthetic"], "");
    assert!(
        cleared["extension"]["api_keys"]["nested"].is_null()
            || cleared["extension"]["api_keys"]["nested"] == ""
    );
    assert_eq!(h.model("beta")["extension"], extension("beta"));
    assert_eq!(
        cleared["extension"]["ordinary"],
        extension("alpha")["ordinary"]
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn providers_manage_without_secrets_write_cannot_replace_or_clear_model_extension_credentials(
) {
    let h = Harness::boot().await;
    let device = h
        .state
        .auth()
        .issue_device_credential(
            "user_local",
            &["settings.read", "settings.write", "providers.manage"],
            None,
        )
        .unwrap();
    let principal = h.principal(&device.secret);
    assert!(principal.has_scope("providers.manage") && !principal.has_scope("secrets.write"));
    let (status, body) = h
        .request(
            &device.secret,
            reqwest::Method::GET,
            "/lingxi/v1/settings/config",
            None,
        )
        .await;
    assert_eq!(status, 200, "有效device必须能真实读取设置：{body}");
    assert_masked_view(&body, false);
    let before = std::fs::read(&h.source).unwrap();
    let generation = h.generation();
    for (field, value) in [
        ("api_key", json!("unauthorized-replacement")),
        ("access_token", json!("unauthorized-replacement")),
        ("refresh_token", json!("unauthorized-replacement")),
        ("api_key", json!("")),
        ("access_token", Value::Null),
        ("refresh_token", json!("")),
        ("headers", json!({"X-Synthetic":""})),
        ("api_keys", json!({"nested":null})),
    ] {
        let mut ext = serde_json::Map::new();
        ext.insert(field.into(), value.clone());
        let patch = json!({"extension":ext});
        // 保留真实HTTP边界；若该路径更严格地LocalOnly拒绝，不能把它充作publisher权限检查。
        let (status, _body) = h
            .request(
                &device.secret,
                reqwest::Method::PUT,
                "/lingxi/v1/agents/a/config",
                Some(json!({"providers":{"fixture":{"models":[{"id":"alpha","extension":ext}]}}})),
            )
            .await;
        assert_eq!(status, 403, "无秘密权限的HTTP写必须明确拒绝");
        assert_eq!(std::fs::read(&h.source).unwrap(), before);
        assert_eq!(h.generation(), generation);
        let error = h
            .state
            .model_configuration()
            .save(
                &principal,
                &[ModelSettingsChange::ModelPatch {
                    provider: "fixture".into(),
                    model: "alpha".into(),
                    patch,
                }],
            )
            .await
            .expect_err("真实publisher本身必须再拒绝嵌套秘密写/清空");
        assert_eq!(error.status, 403);
        assert!(!error.report.saved && !error.report.applied);
        assert_eq!(
            std::fs::read(&h.source).unwrap(),
            before,
            "拒绝不得先写Source"
        );
        assert_eq!(h.generation(), generation, "拒绝不得发布新代次");
    }
    assert_eq!(h.model("alpha")["extension"], extension("alpha"));
    assert_eq!(h.model("beta")["extension"], extension("beta"));
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_header_mask_refills_only_same_header_and_plain_metadata_value_stays_literal() {
    let h = Harness::boot().await;
    let owner = h.principal(&h.state.auth().local_token());
    h.state.model_configuration().save(&owner,&[ModelSettingsChange::ProviderPatch{provider:"fixture".into(),patch:json!({"auth":{"kind":"authHeader","header":"X-Synthetic-Auth","value":"synthetic-real-auth-header"}})}]).await.unwrap();
    h.state.model_configuration().save(&owner,&[ModelSettingsChange::ProviderPatch{provider:"fixture".into(),patch:json!({"auth":{"kind":"authHeader","header":"x-synthetic-auth","value":"********"}})}]).await.unwrap();
    assert_eq!(
        h.source()["providers"]["fixture"]["auth"]["value"],
        "synthetic-real-auth-header",
        "同头大小写改变仍须保留真实凭据，不能保存星号"
    );
    let route = h
        .state
        .model_gateway()
        .unwrap()
        .resolve_route(&ModelRouteRequest {
            operation: ModelOperation::Chat,
            provider: Some("fixture".into()),
            model: Some("alpha".into()),
        })
        .unwrap();
    let auth = h
        .state
        .credential_service()
        .unwrap()
        .resolve(&route)
        .await
        .unwrap();
    assert!(
        auth.header_values("authorization", "Bearer ")
            .iter()
            .any(
                |(name, value)| name.eq_ignore_ascii_case("X-Synthetic-Auth")
                    && value == "synthetic-real-auth-header"
            ),
        "真实统一凭据出口必须提供原值，不能只是磁盘看似保留"
    );
    let before = std::fs::read(&h.source).unwrap();
    let generation = h.generation();
    let error=h.state.model_configuration().save(&owner,&[ModelSettingsChange::ProviderPatch{provider:"fixture".into(),patch:json!({"auth":{"kind":"authHeader","header":"X-Unrelated-Auth","value":"********"}})}]).await.expect_err("不同头不能借用旧认证材料");
    assert!((400..500).contains(&error.status));
    assert!(!error.report.saved && !error.report.applied);
    assert_eq!(std::fs::read(&h.source).unwrap(), before);
    assert_eq!(h.generation(), generation);
    h.state
        .model_configuration()
        .save(
            &owner,
            &[ModelSettingsChange::ModelPatch {
                provider: "fixture".into(),
                model: "alpha".into(),
                patch: json!({"extension":{"ordinaryValue":{"value":"普通元数据值"}}}),
            }],
        )
        .await
        .unwrap();
    h.state
        .model_configuration()
        .save(
            &owner,
            &[ModelSettingsChange::ModelPatch {
                provider: "fixture".into(),
                model: "alpha".into(),
                patch: json!({"extension":{"ordinaryValue":{"value":"********"}}}),
            }],
        )
        .await
        .unwrap();
    assert_eq!(
        h.model("alpha")["extension"]["ordinaryValue"]["value"],
        "********",
        "普通metadata.value不能错误回填旧值"
    );
    h.close().await;
}
