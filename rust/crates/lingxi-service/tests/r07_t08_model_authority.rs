//! 真实HTTP到唯一模型发布者：排队、原子保存及探测前撤权必须零写、零外部派发。
use axum::{extract::State, routing::get, Json, Router};
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_kernel::model_exchange::ModelOperation;
use lingxi_service::{
    model_configuration::{ModelSelection, ModelSettingsChange},
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

async fn platform_models(State(requests): State<Arc<AtomicUsize>>) -> Json<Value> {
    requests.fetch_add(1, Ordering::SeqCst);
    Json(json!({"data":[{"id":"m1"}]}))
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    gateway: Arc<ConfigModelGateway>,
    source: PathBuf,
    base: String,
    requests: Arc<AtomicUsize>,
    stop: tokio::sync::oneshot::Sender<()>,
    serving:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
    platform: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-model-authority-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let home = home.canonicalize().unwrap();
        eprintln!("模型授权撤权现场：{}", home.display());
        let layout = prepare_layout(&home).unwrap();
        let source = layout.runtime_dir.join("models.json");
        let plane = ModelPlaneConfig::from_value(json!({"providers":{},"models":{}})).unwrap();
        let gateway = Arc::new(ConfigModelGateway::from_validated(plane.clone()));
        let clock = Arc::new(lingxi_service::inject::SystemClock);
        let credentials = Arc::new(
            lingxi_service::credentials::CredentialService::bootstrap(
                &plane,
                &layout.runtime_dir,
                clock.clone(),
            )
            .unwrap(),
        );
        let requests = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/v1/models", get(platform_models))
            .with_state(requests.clone());
        let platform = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
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
                model_gateway: Some(gateway.clone()),
                credential_service: Some(credentials),
                network_plane: Some(NetworkPlane::direct_isolated()),
                model_plane_activation: Some(Arc::new(AtomicBool::new(false))),
                clock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let owner = state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", state.auth().local_token())),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        state.model_configuration().save(&owner, &[
            ModelSettingsChange::ProviderPatch { provider:"fixture".into(),patch:json!({"api":"openai-completions","base_url":endpoint,"api_key":"authority-fixture-key","models":[{"id":"m1","inputs":["text"],"contextWindow":8192}]}) },
            ModelSettingsChange::SetBinding {operation:ModelOperation::Chat, selection:Some(ModelSelection {provider:"fixture".into(),model:"m1".into()})},
        ]).await.unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, addr) = tokio::sync::oneshot::channel();
        let serving_state = state.clone();
        let serving = tokio::spawn(async move {
            lingxi_service::run(
                serving_state,
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
        Self {
            home,
            state,
            gateway,
            source,
            base,
            requests,
            stop,
            serving,
            platform,
        }
    }
    async fn request(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{}", self.base, path))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        (response.status().as_u16(), response.json().await.unwrap())
    }
    fn device(&self) -> lingxi_service::auth::IssuedDeviceCredential {
        self.state
            .auth()
            .issue_device_credential(
                "user_local",
                &[
                    "chat",
                    "settings.read",
                    "settings.write",
                    "providers.manage",
                    "secrets.write",
                ],
                None,
            )
            .unwrap()
    }
    fn fingerprint(&self) -> Value {
        let files = [
            self.source.clone(),
            self.home.join("models.json"),
            self.home.join("provider-catalog.json"),
            self.home.join("models-cache.json"),
            self.home.join("user/preferences.json"),
        ]
        .into_iter()
        .map(|path| {
            (
                path.strip_prefix(&self.home)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                json!(std::fs::read(path).ok()),
            )
        })
        .collect::<serde_json::Map<_, _>>();
        json!({"files":files,"generation":self.gateway.upcoming_generation(),"activated":self.state.model_configuration().is_active(),"providers":self.gateway.configured_provider_views(),"routes":self.gateway.configured_routes()})
    }
    async fn usage(&self) -> String {
        self.state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage",
                vec![],
            )
            .await
            .unwrap()
            .unwrap()
    }
    fn observe(&self, phase: &'static str) -> Arc<tokio::sync::Notify> {
        let signal = Arc::new(tokio::sync::Notify::new());
        let observed = signal.clone();
        self.state
            .model_configuration()
            .set_authorization_observer(Some(Arc::new(move |at| {
                if at == phase {
                    observed.notify_one();
                }
            })));
        signal
    }
    async fn close(self) {
        self.state
            .model_configuration()
            .set_authorization_observer(None);
        self.stop.send(()).unwrap();
        let outcome = self.serving.await.unwrap().unwrap();
        assert!(!outcome.drain_timed_out);
        self.platform.abort();
        let _ = self.platform.await;
        self.state.storage().close().await.unwrap();
        drop(self.state);
        std::fs::remove_dir_all(self.home).unwrap();
    }
}
fn denied(status: u16, body: &Value) {
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["code"], "model_authority_revoked", "{body}");
    assert_eq!(body["result"]["saved"], false, "{body}");
    assert_eq!(body["result"]["applied"], false, "{body}");
    assert!(!body.to_string().contains("authority-fixture-key"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_device_model_save_revalidates_revocation_after_actual_mutation_queue() {
    let h = Harness::boot().await;
    let device = h.device();
    let (active_status, active_body) = h
        .request(
            &device.secret,
            reqwest::Method::PUT,
            "/lingxi/v1/providers/fixture/models/m1",
            Some(json!({"contextWindow":12288})),
        )
        .await;
    assert_eq!(
        active_status, 200,
        "原活设备必须先由真实授权成功保存：{active_body}"
    );
    assert_eq!(active_body["result"]["saved"], true);
    assert_eq!(active_body["result"]["applied"], true);
    let before = h.fingerprint();
    let arrived = h.observe("save.queued");
    let response = {
        let lease = h
            .state
            .model_configuration()
            .mutation_lease_for_observation()
            .await;
        let request = h.request(
            &device.secret,
            reqwest::Method::PUT,
            "/lingxi/v1/providers/fixture/models/m1",
            Some(json!({"contextWindow":16384})),
        );
        tokio::pin!(request);
        tokio::time::timeout(std::time::Duration::from_secs(3),async {tokio::select!{_=arrived.notified()=>{},value=&mut request=>panic!("真实写锁持有时不能完成：{value:?}")}}).await.unwrap();
        assert!(h
            .state
            .auth()
            .revoke_device_credential(&device.credential_id)
            .unwrap());
        drop(lease);
        request.await
    };
    denied(response.0, &response.1);
    assert_eq!(h.fingerprint(), before);
    assert_eq!(h.requests.load(Ordering::SeqCst), 0);
    assert_eq!(h.usage().await, "0");
    h.state
        .model_configuration()
        .set_authorization_observer(None);
    let (status, body) = h
        .request(
            &h.state.auth().local_token(),
            reqwest::Method::PUT,
            "/lingxi/v1/providers/fixture/models/m1",
            Some(json!({"contextWindow":16384})),
        )
        .await;
    assert_eq!(status, 200, "有效拥有者仍必须沿真实保存链成功：{body}");
    assert_ne!(h.fingerprint(), before);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_device_model_save_revalidates_again_at_filesafety_atomic_authorization() {
    let h = Harness::boot().await;
    let device = h.device();
    let before = h.fingerprint();
    let seen = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let notify = entered.clone();
    let (release, resume) = std::sync::mpsc::channel();
    let resume = Arc::new(Mutex::new(resume));
    h.state
        .model_configuration()
        .set_authorization_observer(Some(Arc::new(move |phase| {
            if phase == "save.authorize" && seen.fetch_add(1, Ordering::SeqCst) == 1 {
                notify.notify_one();
                resume
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .expect("原子授权检查必须由真实撤权后释放");
            }
        })));
    let request = {
        let base = h.base.clone();
        let token = device.secret.clone();
        tokio::spawn(async move {
            let response = reqwest::Client::new()
                .put(format!("{base}/lingxi/v1/providers/fixture/models/m1"))
                .bearer_auth(token)
                .json(&json!({"contextWindow":24576}))
                .send()
                .await
                .unwrap();
            (
                response.status().as_u16(),
                response.json::<Value>().await.unwrap(),
            )
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
        .await
        .expect("必须到达FileSafety临交换前实际authorize");
    assert!(h
        .state
        .auth()
        .revoke_device_credential(&device.credential_id)
        .unwrap());
    release.send(()).unwrap();
    let (status, body) = request.await.unwrap();
    denied(status, &body);
    assert_eq!(h.fingerprint(), before);
    assert_eq!(h.requests.load(Ordering::SeqCst), 0);
    assert_eq!(h.usage().await, "0");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_probe_and_saved_key_revalidate_revoked_devices_after_actual_queue() {
    let h = Harness::boot().await;
    for (phase, method, path, body) in [
        (
            "probe.queued",
            reqwest::Method::POST,
            "/lingxi/v1/providers/fetch-models",
            Some(json!({"name":"fixture"})),
        ),
        (
            "saved_api_key.queued",
            reqwest::Method::GET,
            "/lingxi/v1/providers/fixture/api-key",
            None,
        ),
    ] {
        let device = h.device();
        let before = h.fingerprint();
        let arrived = h.observe(phase);
        let (status, value) = {
            let lease = h
                .state
                .model_configuration()
                .mutation_lease_for_observation()
                .await;
            let request = h.request(&device.secret, method, path, body);
            tokio::pin!(request);
            tokio::time::timeout(std::time::Duration::from_secs(3),async {tokio::select!{_=arrived.notified()=>{},value=&mut request=>panic!("真实排队未释放：{value:?}")}}).await.unwrap();
            assert!(h
                .state
                .auth()
                .revoke_device_credential(&device.credential_id)
                .unwrap());
            drop(lease);
            request.await
        };
        denied(status, &value);
        assert_eq!(h.fingerprint(), before);
        assert_eq!(h.requests.load(Ordering::SeqCst), 0);
        assert_eq!(h.usage().await, "0");
    }
    h.state
        .model_configuration()
        .set_authorization_observer(None);
    let (status, value) = h
        .request(
            &h.state.auth().local_token(),
            reqwest::Method::POST,
            "/lingxi/v1/providers/fetch-models",
            Some(json!({"name":"fixture"})),
        )
        .await;
    assert_eq!(status, 200, "有效身份仍须真实到达平台目录：{value}");
    assert_eq!(h.requests.load(Ordering::SeqCst), 1);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_reload_revalidates_replaced_local_token_after_actual_mutation_queue() {
    let h = Harness::boot().await;
    let before = h.fingerprint();
    let arrived = h.observe("reload.queued");
    let token = h.state.auth().local_token();
    let (status, value) = {
        let lease = h
            .state
            .model_configuration()
            .mutation_lease_for_observation()
            .await;
        let request = h.request(
            &token,
            reqwest::Method::POST,
            "/lingxi/v1/models/reload",
            Some(json!({})),
        );
        tokio::pin!(request);
        tokio::time::timeout(std::time::Duration::from_secs(3),async {tokio::select!{_=arrived.notified()=>{},value=&mut request=>panic!("真实重载必须先等待写锁：{value:?}")}}).await.unwrap();
        let layout = prepare_layout(&h.home).unwrap();
        let _replacement =
            lingxi_service::auth::AuthService::bootstrap(&layout, "replaced-queued-model-reload")
                .unwrap();
        drop(lease);
        request.await
    };
    denied(status, &value);
    assert_eq!(h.fingerprint(), before);
    assert_eq!(h.requests.load(Ordering::SeqCst), 0);
    assert_eq!(h.usage().await, "0");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_web_session_save_revalidates_logout_after_actual_mutation_queue() {
    let h = Harness::boot().await;
    let login = reqwest::Client::new()
        .post(format!("{}/lingxi/v1/web-auth/login", h.base))
        .json(&json!({"credential":h.state.auth().local_token(),"clientKind":"web"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), reqwest::StatusCode::OK);
    let cookie = login.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let logged_in: Value = login.json().await.unwrap();
    assert!(logged_in["principal"]["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|scope| scope == "providers.manage"));
    let before = h.fingerprint();
    let arrived = h.observe("save.queued");
    let (status, body) = {
        let lease = h
            .state
            .model_configuration()
            .mutation_lease_for_observation()
            .await;
        let request = async {
            let response = reqwest::Client::new()
                .put(format!("{}/lingxi/v1/providers/fixture/models/m1", h.base))
                .header("cookie", &cookie)
                .json(&json!({"contextWindow":32768}))
                .send()
                .await
                .unwrap();
            (
                response.status().as_u16(),
                response.json::<Value>().await.unwrap(),
            )
        };
        tokio::pin!(request);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {tokio::select!{_=arrived.notified()=>{},value=&mut request=>panic!("网页保存须先进入真实写锁队列：{value:?}")}}).await.unwrap();
        let logout = reqwest::Client::new()
            .post(format!("{}/lingxi/v1/web-auth/logout", h.base))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(logout.status(), reqwest::StatusCode::OK);
        drop(lease);
        request.await
    };
    denied(status, &body);
    assert_eq!(h.fingerprint(), before);
    assert_eq!(h.requests.load(Ordering::SeqCst), 0);
    assert_eq!(h.usage().await, "0");
    h.close().await;
}
