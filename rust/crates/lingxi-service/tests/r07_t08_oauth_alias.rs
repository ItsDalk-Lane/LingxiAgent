//! OAuth显示别名沿真实服务、凭证账本和模型链定位；只连接本地合成授权响应。
use axum::{
    extract::State,
    http::{HeaderMap, Uri},
    routing::post,
    Json, Router,
};
use base64::Engine as _;
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_kernel::ports::StoragePort as _;
use lingxi_service::{
    config::ModelPlaneSource, prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Map, Value};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

#[derive(Default)]
struct External {
    tokens: Mutex<Vec<String>>,
    devices: Mutex<Vec<String>>,
    models: Mutex<Vec<(HeaderMap, Value)>>,
    paths: Mutex<Vec<String>>,
}
fn synthetic_access_token() -> String {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({"https://api.openai.com/auth":{"chatgpt_account_id":"owned-local-test-account"}})).unwrap(),
    );
    format!("owned-local-header.{payload}.owned-local-signature")
}
async fn token(State(external): State<Arc<External>>, body: String) -> Json<Value> {
    external.tokens.lock().unwrap().push(body);
    Json(
        json!({"access_token":synthetic_access_token(),"refresh_token":"synthetic-local-refresh","expires_in":3600,"token_type":"Bearer"}),
    )
}
async fn device(State(external): State<Arc<External>>, body: String) -> Json<Value> {
    external.devices.lock().unwrap().push(body);
    Json(
        json!({"device_code":"local-device-code","user_code":"LOCAL-CODE","verification_uri":"http://127.0.0.1:9/local-verification","expires_in":900,"interval":1}),
    )
}
async fn model(
    State(external): State<Arc<External>>,
    headers: HeaderMap,
    uri: Uri,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    external.models.lock().unwrap().push((headers, body));
    external.paths.lock().unwrap().push(uri.path().to_owned());
    if uri.path().ends_with("/responses") {
        let created = json!({"type":"response.created","response":{"id":"local-oauth","status":"in_progress"}});
        let completed = json!({"type":"response.completed","response":{"id":"local-oauth","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"OK"}]}],"usage":{"input_tokens":2,"output_tokens":1}}});
        return (
            [("content-type", "text/event-stream")],
            format!("data: {created}\n\ndata: {completed}\n\n"),
        );
    }
    let frame = json!({"id":"local-oauth","choices":[{"index":0,"delta":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {frame}\n\ndata: [DONE]\n\n"),
    )
}
struct Harness {
    home: PathBuf,
    source: PathBuf,
    runtime: PathBuf,
    state: ServiceState,
    base: String,
    external: Arc<External>,
    stop: tokio::sync::oneshot::Sender<()>,
    server:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
    external_stop: tokio::sync::oneshot::Sender<()>,
    external_server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot(provider: &str, pkce: bool) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-oauth-alias-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(home.join("agents/a/config.yaml"), format!("agent:\n  name: a\nmemory:\n  enabled: false\napi:\n  provider: {provider}\n  model: alias-model\n")).unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            "{\"current_agent_id\":\"a\"}",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let external_base = format!("http://{}", listener.local_addr().unwrap());
        let external = Arc::new(External::default());
        let app = Router::new()
            .route("/token", post(token))
            .route("/device", post(device))
            .route("/v1/chat/completions", post(model))
            .route("/v1/responses", post(model))
            .route("/v1/codex/responses", post(model))
            .with_state(external.clone());
        let (external_stop, stopped) = tokio::sync::oneshot::channel();
        let external_server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let mut auth = json!({"kind":"oauth","flow":if pkce {"authorizationCodePkce"} else {"deviceCode"},"clientId":"local-alias-client","tokenEndpoint":format!("{external_base}/token")});
        if pkce {
            auth["authorizeEndpoint"] = json!(format!("{external_base}/authorize"));
        } else {
            auth["deviceAuthorizationEndpoint"] = json!(format!("{external_base}/device"));
        }
        let mut providers = Map::new();
        let protocol = if provider == "xai-oauth" {
            "openai-responses"
        } else {
            "openai-codex-responses"
        };
        providers.insert(provider.into(),json!({"protocol":protocol,"endpoint":format!("{external_base}/v1"),"auth":auth,"metadata":{"future":{"preserve":19}}}));
        providers.insert("unrelated".into(),json!({"protocol":"openai-completions","endpoint":"http://127.0.0.1:9/v1","auth":{"kind":"apiKey","apiKey":"synthetic-unrelated-key"},"models":[]}));
        let document = json!({"providers":providers,"models":{"chat":{"provider":provider,"model":"alias-model","capabilities":{"tools":true}}}});
        let plane = ModelPlaneConfig::from_value(document.clone()).unwrap();
        let source = layout.runtime_dir.join("models.json");
        std::fs::write(&source, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
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
                model_gateway: Some(Arc::new(ConfigModelGateway::from_validated(plane))),
                credential_service: Some(credentials),
                model_plane_source: Some(ModelPlaneSource::ModelsFile {
                    path: source.clone(),
                }),
                model_plane_activation: Some(Arc::new(AtomicBool::new(true))),
                network_plane: Some(NetworkPlane::direct_isolated()),
                clock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (ready, address) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let server = tokio::spawn(async move {
            lingxi_service::run(
                serving,
                async {
                    let _ = stopped.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        Self {
            home,
            source,
            runtime: layout.runtime_dir,
            state,
            base: format!("http://{}", address.await.unwrap()),
            external,
            stop,
            server,
            external_stop,
            external_server,
        }
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        self.request_token(method, path, body, Some(&self.state.auth().local_token()))
            .await
    }
    async fn request_token(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new().request(method, format!("{}{path}", self.base));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
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
    async fn login(&self, requested: &str, canonical: &str, pkce: bool) {
        let path = format!("/lingxi/v1/models/credentials/{requested}/login");
        let (status, start) = self.request(reqwest::Method::POST, &path, None).await;
        assert_eq!(status, 200, "{start}");
        assert_eq!(start["provider"], canonical);
        assert!(!start.to_string().contains(&synthetic_access_token()));
        let (path, body) = if pkce {
            let url =
                reqwest::Url::parse(start["start"]["authorizeUrl"].as_str().unwrap()).unwrap();
            let state = url
                .query_pairs()
                .find(|(key, _)| key == "state")
                .unwrap()
                .1
                .into_owned();
            let callback = format!("{path}/callback");
            let count = self.external.tokens.lock().unwrap().len();
            let (status, wrong) = self
                .request(
                    reqwest::Method::POST,
                    &callback,
                    Some(json!({"state":"wrong-local-state","code":"local-code"})),
                )
                .await;
            assert_eq!(status, 409, "{wrong}");
            assert_eq!(
                self.external.tokens.lock().unwrap().len(),
                count,
                "错误state不能交换凭证"
            );
            (callback, Some(json!({"state":state,"code":"local-code"})))
        } else {
            assert_eq!(start["start"]["userCode"], "LOCAL-CODE");
            (format!("{path}/poll"), None)
        };
        let (status, result) = self.request(reqwest::Method::POST, &path, body).await;
        assert_eq!(status, 200, "{result}");
        assert_eq!(result["provider"], canonical);
        assert_eq!(result["outcome"]["status"], "loggedIn");
        assert_eq!(result["outcome"]["persisted"], true);
        assert!(!result.to_string().contains(&synthetic_access_token()));
    }
    async fn close(self) {
        let _ = self.stop.send(());
        let outcome = self.server.await.unwrap().unwrap();
        assert!(!outcome.drain_timed_out);
        let _ = self.external_stop.send(());
        self.external_server.await.unwrap();
        self.state.storage().close().await.unwrap();
        drop(self.state);
        std::fs::remove_dir_all(self.home).unwrap();
    }
}

#[tokio::test]
async fn oauth_display_alias_reaches_pkce_device_registry_logout_revoke_and_actual_model_without_renaming_source(
) {
    for (canonical, requested, pkce) in [
        ("openai-codex", "openai-codex-oauth", true),
        ("openai-codex-oauth", "openai-codex", false),
        ("xai-oauth", "xai-oauth", false),
    ] {
        let h = Harness::boot(canonical, pkce).await;
        let source_before = std::fs::read(&h.source).unwrap();
        let credential_path = format!("/lingxi/v1/models/credentials/{requested}");
        let models_path = format!("/lingxi/v1/models/oauth/{requested}/models");
        let (status, _) = h
            .request_token(
                reqwest::Method::POST,
                &format!("{credential_path}/login"),
                None,
                None,
            )
            .await;
        assert_eq!(status, 401);
        let device = h
            .state
            .auth()
            .issue_device_credential(
                lingxi_service::LOCAL_OWNER_USER_ID,
                &["settings.read"],
                None,
            )
            .unwrap();
        let (status, _) = h
            .request_token(
                reqwest::Method::POST,
                &format!("{credential_path}/login"),
                None,
                Some(&device.secret),
            )
            .await;
        assert_eq!(status, 403);
        for unknown in ["openai-codex-oauth-extra", "other-oauth", "unrelated"] {
            let (status, result) = h
                .request(
                    reqwest::Method::GET,
                    &format!("/lingxi/v1/models/oauth/{unknown}/models"),
                    None,
                )
                .await;
            assert_eq!(status, 404, "{result}");
            assert!(!result.to_string().contains("synthetic-unrelated-key"));
            let (status, result) = h
                .request(
                    reqwest::Method::POST,
                    &format!("/lingxi/v1/models/credentials/{unknown}/login"),
                    None,
                )
                .await;
            assert_eq!(status, 409, "登录写面沿原明确拒绝语义：{result}");
        }
        assert!(h.external.tokens.lock().unwrap().is_empty());
        assert!(h.external.devices.lock().unwrap().is_empty());
        let (status, before) = h.request(reqwest::Method::GET, &models_path, None).await;
        assert_eq!(status, 200, "{before}");
        assert_eq!(before["provider"], canonical);
        assert_eq!(before["loggedIn"], false);
        assert_eq!(before["models"], json!([]));
        h.login(requested, canonical, pkce).await;
        let (status, current) = h.request(reqwest::Method::GET, &models_path, None).await;
        assert_eq!(status, 200, "{current}");
        assert_eq!(current["provider"], canonical);
        assert_eq!(current["loggedIn"], true);
        assert_eq!(current["models"], json!(["alias-model"]));
        let credentials: Value =
            serde_json::from_slice(&std::fs::read(h.runtime.join("credentials.json")).unwrap())
                .unwrap();
        assert!(credentials["providers"].get(canonical).is_some());
        if canonical != requested {
            assert!(
                credentials["providers"].get(requested).is_none(),
                "别名不能另建一套凭证"
            );
        }
        let (status, health) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/models/health",
                Some(json!({"provider":requested,"model":"alias-model"})),
            )
            .await;
        assert_eq!(status, 200, "{health}");
        assert_eq!(health["ok"], true, "{health}");
        let codex = canonical != "xai-oauth";
        if codex {
            assert_eq!(
                health["skipped"], true,
                "Codex沿用明确跳过独立健康探测的语义"
            );
            assert!(h.external.models.lock().unwrap().is_empty());
            assert!(h
                .state
                .storage()
                .query_model_call_usage(Default::default())
                .await
                .unwrap()
                .is_empty());
        }
        // 不把Codex健康检查的skipped当生成成功；以真实会话提交证明登录后的消费。
        h.state
            .session_admin()
            .create_session(
                "oauth-brand-chat",
                "a",
                lingxi_kernel::LOCAL_OWNER_SUBJECT,
                "OAuth实际聊天",
                Some("read_only"),
                &[],
                false,
                h.state.clock().now_unix_ms(),
            )
            .await
            .unwrap();
        let (status, chat) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/sessions/oauth-brand-chat/execute",
                Some(json!({"input":"请回复OK","requestId":"oauth-brand-chat-request"})),
            )
            .await;
        assert_eq!(status, 200, "{chat}");
        let run = chat["runId"].as_str().expect("必须返回同一真实Run身份");
        assert_eq!(
            h.state
                .storage()
                .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run.into()])
                .await
                .unwrap()
                .as_deref(),
            Some("completed"),
            "{chat}"
        );
        let expected_calls = if codex { 1 } else { 2 };
        {
            let calls = h.external.models.lock().unwrap();
            assert_eq!(calls.len(), expected_calls);
            assert_eq!(
                calls[0].0.get("authorization").unwrap(),
                format!("Bearer {}", synthetic_access_token()).as_str()
            );
            assert_eq!(calls[0].1["model"], "alias-model");
            assert!(
                calls[0].1["input"].is_array(),
                "应走该OAuth供应商的真实responses协议"
            );
            if codex {
                assert_eq!(
                    calls[0].0.get("chatgpt-account-id").unwrap(),
                    "owned-local-test-account"
                );
            }
        }
        assert_eq!(
            *h.external.paths.lock().unwrap(),
            vec![
                if codex {
                    "/v1/codex/responses"
                } else {
                    "/v1/responses"
                };
                expected_calls
            ],
            "不能用completions替代现役OAuth协议"
        );
        let usage = h
            .state
            .storage()
            .query_model_call_usage(Default::default())
            .await
            .unwrap();
        assert_eq!(usage.len(), expected_calls);
        assert_eq!(usage[0].provider, canonical);
        assert_eq!(usage[0].transport_attempts, Some(1));
        assert!(
            usage.iter().any(|row| row.run_id.as_deref() == Some(run)),
            "用量必须关联此次真实聊天Run"
        );
        for row in &usage {
            assert_eq!(row.provider, canonical);
            assert_eq!(
                row.protocol,
                if codex {
                    "openai-codex-responses"
                } else {
                    "openai-responses"
                }
            );
        }
        let (status, added) = h
            .request(
                reqwest::Method::POST,
                &models_path,
                Some(json!({"modelId":"custom-alias-model"})),
            )
            .await;
        assert_eq!(status, 200, "{added}");
        assert_eq!(added["provider"], canonical);
        assert_eq!(
            added["models"],
            json!(["alias-model", "custom-alias-model"])
        );
        let (status, removed) = h
            .request(
                reqwest::Method::DELETE,
                &format!("{models_path}/custom-alias-model"),
                None,
            )
            .await;
        assert_eq!(status, 200, "{removed}");
        assert_eq!(removed["provider"], canonical);
        assert_eq!(removed["models"], json!(["alias-model"]));
        let (status, logout) = h
            .request(
                reqwest::Method::POST,
                &format!("{credential_path}/logout"),
                None,
            )
            .await;
        assert_eq!(status, 200, "{logout}");
        assert_eq!(logout["provider"], canonical);
        assert_eq!(logout["models"], json!([]));
        let (_, logged_out) = h.request(reqwest::Method::GET, &models_path, None).await;
        assert_eq!(logged_out["loggedIn"], false);
        assert_eq!(logged_out["models"], json!([]));
        h.login(requested, canonical, pkce).await;
        let (status, revoked) = h
            .request(
                reqwest::Method::POST,
                &format!("{credential_path}/revoke"),
                None,
            )
            .await;
        assert_eq!(status, 200, "{revoked}");
        assert_eq!(revoked["provider"], canonical);
        let (_, after_revoke) = h.request(reqwest::Method::GET, &models_path, None).await;
        assert_eq!(after_revoke["loggedIn"], false);
        assert_eq!(after_revoke["models"], json!([]));
        assert_eq!(
            std::fs::read(&h.source).unwrap(),
            source_before,
            "登录管理不能改Source键或其它设置"
        );
        let audits: Value =
            serde_json::from_slice(&std::fs::read(h.runtime.join("management.json")).unwrap())
                .unwrap();
        for action in [
            "models.credentials.login.start",
            "models.oauth.model.add",
            "models.oauth.model.remove",
            "models.credentials.logout",
            "models.credentials.revoke",
        ] {
            let rows: Vec<_> = audits["audit"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["action"] == action)
                .collect();
            assert!(!rows.is_empty(), "缺少原审计动作：{action}");
            assert!(rows
                .iter()
                .all(|row| row["metadata"]["provider"] == canonical));
        }
        assert!(!audits.to_string().contains(&synthetic_access_token()));
        let tokens = h.external.tokens.lock().unwrap().clone();
        assert_eq!(tokens.len(), 2);
        for body in tokens {
            assert!(
                body.contains(if pkce {
                    "grant_type=authorization_code"
                } else {
                    "device_code=local-device-code"
                }),
                "{body}"
            );
        }
        assert_eq!(h.external.models.lock().unwrap().len(), expected_calls);
        h.close().await;
    }
}
