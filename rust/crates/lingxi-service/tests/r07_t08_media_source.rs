//! 媒体模型管理真实 HTTP → 唯一 Source → 同一 Operation/生成作业；仅外端响应替身。
//! 四类默认选择仍为媒体偏好，不声称页面可重绑独立操作槽。
use axum::{body::Bytes, extract::State, http::HeaderMap, routing::post, Json, Router};
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_kernel::{
    ports::StoragePort as _, usage::ModelUsageQuery, Principal as KernelPrincipal,
};
use lingxi_service::{
    approval_service::{Answer, ApprovalService},
    config::ModelPlaneSource,
    model_configuration::ModelSettingsChange,
    prepare_layout,
    toolgateway::ToolInvocationGateway,
    HomeSource, NetworkMode, Principal, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};
const MP3: &[u8] = b"ID3\x04\0\0\0\0\0\0owned-media-source";
const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
async fn speech(
    State(wire): State<Arc<Mutex<Vec<Value>>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ([(&'static str, &'static str); 1], Bytes) {
    assert_eq!(headers["authorization"], "Bearer synthetic-media-key");
    wire.lock().unwrap().push(body);
    ([("content-type", "audio/mpeg")], Bytes::from_static(MP3))
}
async fn image(
    State(wire): State<Arc<Mutex<Vec<Value>>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    assert_eq!(headers["authorization"], "Bearer synthetic-media-key");
    wire.lock().unwrap().push(body);
    Json(json!({"data":[{"b64_json":PNG}]}))
}
struct Platform {
    endpoint: String,
    wire: Arc<Mutex<Vec<Value>>>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}
impl Platform {
    async fn boot() -> Self {
        let wire = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/audio/speech", post(speech))
            .route("/images/generations", post(image))
            .with_state(wire.clone());
        let (stop, rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .unwrap();
        });
        Self {
            endpoint,
            wire,
            stop,
            task,
        }
    }
    async fn close(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}
struct Harness {
    home: PathBuf,
    source: PathBuf,
    state: ServiceState,
    base: String,
    approvals: Arc<ApprovalService>,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn boot(endpoint: &str, existing: Option<PathBuf>) -> Self {
        let home = existing.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-r07-t08-media-source-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        if !home.join("agents/a/config.yaml").exists() {
            std::fs::write(
                home.join("agents/a/config.yaml"),
                "agent:\n  name: a\nmemory:\n  enabled: false\n",
            )
            .unwrap();
            std::fs::create_dir_all(home.join("user")).unwrap();
            std::fs::write(
                home.join("user/preferences.json"),
                r#"{"current_agent_id":"a"}"#,
            )
            .unwrap();
        }
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let source = home.join("model-plane.json");
        if !source.exists() {
            std::fs::write(&source,serde_json::to_vec_pretty(&json!({"providers":{"fixture":{"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-media-key"},"models":[{"id":"chat-only","input":["text"],"context":8192},{"id":"seed-speech","protocol":"openai-audio-speech","outputs":["audio"],"unknownKeep":{"nested":true}}]}},"models":{"chat":{"provider":"fixture","model":"chat-only"},"speech":{"provider":"fixture","model":"seed-speech"}},"network":{"proxy":{"mode":"direct"}}})).unwrap()).unwrap();
        }
        let plane = ModelPlaneConfig::from_value(
            serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap(),
        )
        .unwrap();
        let clock = Arc::new(lingxi_service::inject::SystemClock);
        let credentials = Arc::new(
            lingxi_service::credentials::CredentialService::bootstrap(
                &plane,
                &layout.runtime_dir,
                clock.clone(),
            )
            .unwrap(),
        );
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new()),
            approvals.clone(),
            clock.clone(),
            Default::default(),
            300000,
            1000,
        ));
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 3000,
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
                tool_gateway: Some(gateway),
                approval_gate: Some(approvals.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        if state
            .storage()
            .load_session_admission("media-source")
            .await
            .unwrap()
            .is_none()
        {
            state
                .session_admin()
                .create_session(
                    "media-source",
                    "a",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "媒体Source实测",
                    Some("operate"),
                    &[],
                    false,
                    1,
                )
                .await
                .unwrap();
        }
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, addr) = tokio::sync::oneshot::channel();
        let service = state.clone();
        let task = tokio::spawn(async move {
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
        eprintln!("媒体唯一Source现场：{}", home.display());
        Self {
            home,
            source,
            state,
            base,
            approvals,
            stop,
            task,
        }
    }
    fn owner(&self) -> Principal {
        self.state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", self.state.auth().local_token())),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap()
    }
    fn source(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.source).unwrap()).unwrap()
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        request(
            &self.base,
            &self.state.auth().local_token(),
            method,
            path,
            body,
        )
        .await
    }
    async fn assert_added(&self, kind: &str, id: &str) {
        let path = if kind == "asr" {
            "/lingxi/v1/speech-recognition/providers".into()
        } else {
            format!("/lingxi/v1/media/{kind}/providers")
        };
        let (status, body) = self.request(reqwest::Method::GET, &path, None).await;
        assert_eq!(status, 200, "{body}");
        assert!(
            body["providers"]["fixture"]["models"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["id"] == id),
            "Source中的真实模型必须可见：{body}"
        );
    }
    async fn generate(&self, kind: &str, id: &str) -> Value {
        let path = format!("/lingxi/v1/media/{kind}/generate");
        let body = json!({"sessionId":"media-source","prompt":"本机媒体测试","provider":"fixture","model":id,"delivery":{"mode":"response"}});
        let base = self.base.clone();
        let token = self.state.auth().local_token().to_owned();
        let pending = tokio::spawn(async move {
            request(&base, &token, reqwest::Method::POST, &path, Some(body)).await
        });
        let approval = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(item) = self
                    .approvals
                    .pending_for_principal(&KernelPrincipal::LocalUser, "media-source")
                    .into_iter()
                    .next()
                {
                    break item.approval_id;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("真实生成应进入批准");
        assert!(self
            .approvals
            .answer_for_principal(
                &KernelPrincipal::LocalUser,
                "media-source",
                &approval,
                Answer::Approve
            )
            .settled());
        let (status, body) = pending.await.unwrap();
        assert_eq!(status, 200, "{body}");
        if kind == "speech" {
            assert!(
                !body["tasks"][0]["files"].as_array().unwrap().is_empty(),
                "完成回执须有真实文件：{body}"
            );
        } else {
            assert!(
                body["tasks"][0]["files"].is_null(),
                "受理不能冒充生成完成：{body}"
            );
            let task = body["batchId"].as_str().unwrap();
            tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    let (status, result) = self
                        .request(
                            reqwest::Method::GET,
                            &format!("/lingxi/v1/media/tasks/{task}"),
                            None,
                        )
                        .await;
                    assert_eq!(status, 200, "{result}");
                    match result["task"]["status"].as_str() {
                        Some("done") => {
                            assert!(
                                !result["task"]["files"].as_array().unwrap().is_empty(),
                                "{result}"
                            );
                            break;
                        }
                        Some("pending") => tokio::time::sleep(Duration::from_millis(10)).await,
                        _ => panic!("实际生成未成功：{result}"),
                    }
                }
            })
            .await
            .expect("真实媒体生成须在预算内完成");
        }
        body
    }
    async fn close(self) -> PathBuf {
        self.state
            .model_configuration()
            .set_authorization_observer(None);
        self.stop.send(()).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!outcome.drain_timed_out);
        self.state.storage().close().await.unwrap();
        self.home
    }
}
async fn request(
    base: &str,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .request(method, format!("{base}{path}"))
        .bearer_auth(token)
        .timeout(Duration::from_secs(30));
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
fn collection(kind: &str) -> String {
    if kind == "asr" {
        "/lingxi/v1/speech-recognition/providers/fixture/models".into()
    } else {
        format!("/lingxi/v1/media/{kind}/providers/fixture/models")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn media_http_source_crud_same_operations_actual_generation_restart_and_projection_corruption(
) {
    let platform = Platform::boot().await;
    let h = Harness::boot(&platform.endpoint, None).await;
    let operations = h.state.operations().unwrap().clone();
    let original = h.source();
    for (kind, id, protocol) in [
        ("speech", "new-speech", "openai-audio-speech"),
        ("image", "new-image", "openai-images"),
        ("video", "new-video", "agnes-videos"),
        ("asr", "new-asr", "openai-audio-transcriptions"),
    ] {
        let(status,body)=h.request(reqwest::Method::POST,&collection(kind),Some(json!({"id":id,"protocolId":protocol,"displayName":format!("{kind}新模型"),"customField":{"keep":true}}))).await;
        assert_eq!(status, 200, "{kind}: {body}");
        assert_eq!(body["result"]["applied"], true, "{body}");
        h.assert_added(kind, id).await;
        let before = std::fs::read(&h.source).unwrap();
        let (status, body) = h
            .request(
                reqwest::Method::POST,
                &collection(kind),
                Some(json!({"id":id,"protocolId":protocol})),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            std::fs::read(&h.source).unwrap(),
            before,
            "重复添加不得改Source"
        );
        let (status, body) = h
            .request(
                reqwest::Method::PUT,
                &format!("{}/{id}", collection(kind)),
                Some(json!({"displayName":format!("{kind}改名")})),
            )
            .await;
        assert_eq!(status, 200, "{body}");
    }
    assert!(
        Arc::ptr_eq(&operations, h.state.operations().unwrap()),
        "即时更新必须消费同一Operation实例"
    );
    let published = h.source();
    assert_eq!(
        published["models"], original["models"],
        "模型目录变更不能暗改独立槽位"
    );
    assert_eq!(
        published["providers"]["fixture"]["models"][1]["unknownKeep"],
        original["providers"]["fixture"]["models"][1]["unknownKeep"]
    );
    assert_eq!(
        published["providers"]["fixture"]["models"][0],
        original["providers"]["fixture"]["models"][0],
        "聊天模型必须原样保留"
    );
    let before = std::fs::read(&h.source).unwrap();
    for (method, path, body) in [
        (
            reqwest::Method::POST,
            collection("speech"),
            Some(json!({"id":"chat-only","protocolId":"openai-audio-speech"})),
        ),
        (
            reqwest::Method::PUT,
            format!("{}/new-image", collection("speech")),
            Some(json!({"displayName":"越类写"})),
        ),
        (
            reqwest::Method::DELETE,
            format!("{}/new-speech", collection("asr")),
            None,
        ),
    ] {
        let (status, result) = h.request(method, &path, body).await;
        assert_eq!(status, 409, "跨类别同名操作必须零改动：{result}");
        assert_eq!(std::fs::read(&h.source).unwrap(), before);
    }
    // 损坏兼容投影不能夺取权威；发布者报告真实已应用但投影失败。
    std::fs::write(h.home.join("provider-catalog.json"), b"{broken").unwrap();
    let (status, body) = h
        .request(
            reqwest::Method::PUT,
            &format!("{}/new-speech", collection("speech")),
            Some(json!({"displayName":"投影坏后仍真实应用"})),
        )
        .await;
    assert!(status >= 400, "兼容投影失败必须诚实返回：{body}");
    assert_eq!(body["result"]["saved"], true, "{body}");
    assert_eq!(body["result"]["applied"], true, "{body}");
    h.assert_added("speech", "new-speech").await;
    let receipt = h.generate("speech", "new-speech").await;
    let filename = receipt["tasks"][0]["files"][0].as_str().unwrap();
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/media/generated/{filename}", h.base))
        .bearer_auth(h.state.auth().local_token())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.bytes().await.unwrap().as_ref(), MP3);
    h.generate("image", "new-image").await;
    let wire = platform.wire.lock().unwrap().clone();
    assert!(wire.iter().any(|v| v["model"] == "new-speech"));
    assert!(wire.iter().any(|v| v["model"] == "new-image"));
    let usage = h
        .state
        .storage()
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .unwrap();
    assert!(usage.iter().any(|v| v.provider == "fixture"
        && v.model == "new-speech"
        && v.transport_attempts == Some(1)));
    assert!(usage.iter().any(|v| v.provider == "fixture"
        && v.model == "new-image"
        && v.transport_attempts == Some(1)));
    let home = h.close().await;
    let h = Harness::boot(&platform.endpoint, Some(home)).await;
    h.assert_added("speech", "new-speech").await;
    assert_eq!(
        h.source()["providers"]["fixture"]["models"][1]["unknownKeep"],
        original["providers"]["fixture"]["models"][1]["unknownKeep"]
    );
    // 修复本测试自己破坏的兼容文件，只为后续回执可完整验证；Source不变。
    std::fs::write(h.home.join("provider-catalog.json"), serde_json::to_vec(&json!({"catalogVersion":2,"providers":{},"capabilities":{"search":{"providers":[]}},"meta":{}})).unwrap()).unwrap();
    for (kind, id) in [
        ("speech", "new-speech"),
        ("image", "new-image"),
        ("video", "new-video"),
        ("asr", "new-asr"),
    ] {
        let (status, body) = h
            .request(
                reqwest::Method::DELETE,
                &format!("{}/{id}", collection(kind)),
                None,
            )
            .await;
        assert_eq!(status, 200, "{body}");
    }
    assert!(
        h.state
            .operations()
            .unwrap()
            .media_route("speech", Some("fixture".into()), Some("new-speech".into()))
            .is_err(),
        "删除后旧选择必须立即失效"
    );
    assert!(h
        .state
        .operations()
        .unwrap()
        .media_route("image", Some("fixture".into()), Some("new-image".into()))
        .is_err());
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
    platform.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn media_source_queued_revocation_and_stale_edit_cannot_resurrect_deleted_model() {
    let platform = Platform::boot().await;
    let h = Harness::boot(&platform.endpoint, None).await;
    let device = h
        .state
        .auth()
        .issue_device_credential("user_local", &["settings.read", "providers.manage"], None)
        .unwrap();
    let before = std::fs::read(&h.source).unwrap();
    // 原媒体管理面为 LocalOnly；普通设备在HTTP守卫处拒绝，不能假称到达写锁。
    let (status, body) = request(
        &h.base,
        &device.secret,
        reqwest::Method::POST,
        &collection("speech"),
        Some(json!({"id":"device-model","protocolId":"openai-audio-speech"})),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(std::fs::read(&h.source).unwrap(), before);
    let signal = Arc::new(tokio::sync::Notify::new());
    let observed = signal.clone();
    h.state
        .model_configuration()
        .set_authorization_observer(Some(Arc::new(move |phase| {
            if phase == "snapshot.queued" {
                observed.notify_one();
            }
        })));
    let lease = h
        .state
        .model_configuration()
        .mutation_lease_for_observation()
        .await;
    let base = h.base.clone();
    let token = h.state.auth().local_token();
    let pending = tokio::spawn(async move {
        request(
            &base,
            &token,
            reqwest::Method::POST,
            &collection("speech"),
            Some(json!({"id":"revoked-model","protocolId":"openai-audio-speech"})),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), signal.notified())
        .await
        .expect("原本有效的本机主人必须已到真实写锁队列");
    let layout = prepare_layout(&h.home).unwrap();
    let _replacement =
        lingxi_service::auth::AuthService::bootstrap(&layout, "replaced-queued-media-owner")
            .unwrap();
    drop(lease);
    let (status, body) = pending.await.unwrap();
    assert!(
        status == 401 || status == 403,
        "排队后原本机主人已撤销必须拒绝：{body}"
    );
    assert_eq!(std::fs::read(&h.source).unwrap(), before);
    assert!(platform.wire.lock().unwrap().is_empty());
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
    // 新现场独立验证过时代次，避免把被撤销的身份或令牌恢复来混淆两个检查。
    let h = Harness::boot(&platform.endpoint, None).await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let notify = entered.clone();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let gate = release.clone();
    let once = Arc::new(AtomicBool::new(false));
    h.state
        .model_configuration()
        .set_authorization_observer(Some(Arc::new(move |phase| {
            if phase == "save.queued" && !once.swap(true, Ordering::SeqCst) {
                notify.notify_one();
                let (lock, wake) = &*gate;
                let blocked = lock.lock().unwrap();
                let (done, wait) = wake
                    .wait_timeout_while(blocked, Duration::from_secs(10), |done| !*done)
                    .unwrap();
                assert!(*done && !wait.timed_out(), "测试写入闸门须真实释放");
            }
        })));
    let base = h.base.clone();
    let token = h.state.auth().local_token().to_owned();
    let edit = tokio::spawn(async move {
        request(
            &base,
            &token,
            reqwest::Method::PUT,
            &format!("{}/seed-speech", collection("speech")),
            Some(json!({"displayName":"过时代次编辑"})),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    h.state
        .model_configuration()
        .save(
            &h.owner(),
            &[ModelSettingsChange::ModelRemove {
                provider: "fixture".into(),
                model: "seed-speech".into(),
            }],
        )
        .await
        .unwrap();
    {
        let (lock, wake) = &*release;
        *lock.lock().unwrap() = true;
        wake.notify_one();
    }
    let (status, body) = edit.await.unwrap();
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "model_generation_changed");
    assert!(!h.source()["providers"]["fixture"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == "seed-speech"));
    assert!(platform.wire.lock().unwrap().is_empty());
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
    platform.close().await;
}
