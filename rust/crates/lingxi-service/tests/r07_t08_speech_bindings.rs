//! 两个专用语音身份：预置独立操作绑定，关键供应商变更必须经真实 T08 HTTP。
//!
//! 本检查不声称页面能重绑 speech/speechRecognition 独立槽，也不伪造系统识别。
//! 同一 ServiceState 的既有 OperationService 必须立即消费发布后的协议与凭证。
//! 仅 BigASR 的外部平台响应使用本机 HTTP 替身；网关、存储、执行与系统 say 均真实。

use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use base64::Engine as _;
#[cfg(target_os = "macos")]
use lingxi_adapters::models::operations::speech::SpeechRequest;
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
#[cfg(target_os = "macos")]
use lingxi_kernel::model_exchange::{
    MediaGenerationKind, ModelGatewayPort, ModelOperation, ModelRouteRequest,
};
use lingxi_kernel::{
    ports::StoragePort as _,
    usage::{CallOutcome, ModelCallUsageRecord, ModelUsageQuery},
};
use lingxi_service::{
    config::ModelPlaneSource, operations::HostAudio, prepare_layout, HomeSource, NetworkMode,
    ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
#[cfg(target_os = "macos")]
use sha2::Digest as _;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Harness {
    home: PathBuf,
    source: PathBuf,
    #[cfg(target_os = "macos")]
    products: PathBuf,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn boot(document: Value) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-speech-bindings-{}",
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
            "{\"current_agent_id\":\"a\"}",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        #[cfg(target_os = "macos")]
        let products = layout.runtime_dir.join("media-products");
        let source = home.join("model-plane.json");
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
                shutdown_timeout_ms: 3000,
            },
            &layout,
            ServiceDeps {
                model_gateway: Some(Arc::new(ConfigModelGateway::from_validated(plane))),
                credential_service: Some(credentials),
                network_plane: Some(NetworkPlane::direct_isolated()),
                model_plane_source: Some(ModelPlaneSource::ModelsFile {
                    path: source.clone(),
                }),
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
                    ready.send(addr).unwrap();
                },
                None,
            )
            .await
            .unwrap();
        });
        let base = format!("http://{}", address.await.unwrap());
        eprintln!("T08 专用语音真实现场：{}", home.display());
        Self {
            home,
            source,
            #[cfg(target_os = "macos")]
            products,
            state,
            base,
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
        let mut request = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.state.auth().local_token());
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

    async fn publish(&self, provider: &str, patch: Value) {
        let mut providers = serde_json::Map::new();
        providers.insert(provider.to_owned(), patch);
        let (status, report) = self
            .request(
                reqwest::Method::PUT,
                "/lingxi/v1/settings/config",
                Some(json!({"providers":providers})),
            )
            .await;
        assert_eq!(status, 200, "真实 HTTP 保存应完成：{report}");
        assert_eq!(report["modelConfiguration"]["saved"], true, "{report}");
        assert_eq!(report["modelConfiguration"]["applied"], true, "{report}");
        assert_eq!(report["modelConfiguration"]["activated"], true, "{report}");
    }

    async fn usage(&self) -> Vec<ModelCallUsageRecord> {
        self.state
            .storage()
            .query_model_call_usage(ModelUsageQuery::default())
            .await
            .unwrap()
    }

    fn source(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.source).unwrap()).unwrap()
    }

    async fn assert_summary(&self, provider: &str, capability: &str) {
        let (status, summary) = self
            .request(reqwest::Method::GET, "/lingxi/v1/providers/summary", None)
            .await;
        assert_eq!(status, 200, "{summary}");
        assert!(
            summary["providers"][provider]["media_capability_bindings"]
                .as_array()
                .unwrap()
                .iter()
                .any(
                    |row| row["runtime_provider_id"] == provider && row["capability"] == capability
                ),
            "summary 必须展示同一个专用语音身份，而不是别的媒体供应商：{summary}"
        );
        assert!(
            !self
                .state
                .model_gateway()
                .unwrap()
                .chat_model_keys()
                .iter()
                .any(|key| key.starts_with(&format!("{provider}/"))),
            "专用语音模型不得冒充聊天模型"
        );
        let (status, probe) = self
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/providers/test",
                Some(json!({"name":provider})),
            )
            .await;
        assert_eq!(status, 200, "{probe}");
        assert_eq!(
            probe["unavailable"], true,
            "普通探测不适用于专用语音：{probe}"
        );
        assert_eq!(probe["ok"], false);
        assert_eq!(probe["status"], 0);
    }

    async fn close(self) {
        self.stop.send(()).expect("真实服务应仍接收退出信号");
        tokio::time::timeout(Duration::from_secs(10), self.server)
            .await
            .unwrap()
            .unwrap();
        self.state.storage().close().await.unwrap();
        std::fs::remove_dir_all(self.home).unwrap();
    }
}

fn audio() -> HostAudio {
    // 合法的 16kHz 单声道 PCM WAV；平台替身只替换响应，不替换操作输入或输出。
    let samples = 160_u32;
    let data_len = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend_from_slice(&32_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.resize(bytes.len() + data_len as usize, 0);
    HostAudio {
        bytes,
        mime: "audio/wav".into(),
        filename: "owned-speech.wav".into(),
    }
}

fn assert_usage(
    row: &ModelCallUsageRecord,
    provider: &str,
    model: &str,
    protocol: &str,
    attempts: u32,
) {
    assert_eq!(row.provider, provider);
    assert_eq!(row.model, model);
    assert_eq!(row.protocol, protocol);
    assert_eq!(row.origin, "operation");
    assert_eq!(row.outcome, CallOutcome::Succeeded);
    assert_eq!(row.transport_attempts, Some(attempts));
    assert!(
        row.usage.is_none(),
        "未报告 token 用量时必须未知，不能捏造为0"
    );
    assert!(
        row.session_id.is_none() && row.run_id.is_none(),
        "本项是实际内部 Operation 调用，不借用其他会话"
    );
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_publish_system_speech_reaches_same_operation_real_say_product_zero_transport_and_recognition_refusal(
) {
    let model = "system-say-settings-model";
    let recognition = "system-recognition-settings-model";
    let h = Harness::boot(json!({
        "providers":{"system-speech":{"protocol":"openai-audio-speech","endpoint":"http://127.0.0.1:9","auth":{"kind":"apiKey","apiKey":"synthetic-old-system-key"},"models":[{"id":model},{"id":recognition,"protocol":"system-speech-recognition"}]}},
        "models":{"speech":{"provider":"system-speech","model":model},"speechRecognition":{"provider":"system-speech","model":recognition}}
    })).await;
    let operations = h.state.operations().unwrap().clone();
    let before_generation = h.state.model_gateway().unwrap().config_generation();
    let before = h
        .state
        .model_gateway()
        .unwrap()
        .resolve_route(&ModelRouteRequest::for_operation(
            ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Speech,
            },
        ))
        .unwrap();
    assert_eq!(before.protocol.config_name(), "openai-audio-speech");
    h.publish("system-speech", json!({"api":"system-speech","base_url":"","api_key":"","headers":null,
        "models":[{"id":model,"inputs":["text"],"outputs":["audio"]},{"id":recognition,"api":"system-speech-recognition","inputs":["audio"],"outputs":["text"]}]})).await;
    assert!(
        Arc::ptr_eq(&operations, h.state.operations().unwrap()),
        "HTTP保存不得另造Operation执行器"
    );
    assert!(h.state.model_gateway().unwrap().config_generation() > before_generation);
    let published = h.source();
    assert_eq!(published["providers"]["system-speech"]["endpoint"], "");
    assert_eq!(
        published["providers"]["system-speech"]["auth"]["kind"],
        "none"
    );
    assert_eq!(
        published["models"]["speech"]["model"], model,
        "本项保持预置绑定，不声称页面重绑槽位"
    );
    h.assert_summary("system-speech", "speechGeneration").await;
    h.assert_summary("system-speech", "speechRecognition").await;
    assert!(
        h.usage().await.is_empty(),
        "声明与不适用探测不得假记真实调用"
    );

    let product = operations
        .synthesize_speech(
            SpeechRequest {
                text: "Lingxi speech settings check".into(),
                voice: None,
                speed: None,
                format: None,
            },
            Some(lingxi_adapters::models::dispatch::unix_ms_now() + 30_000),
        )
        .await
        .unwrap();
    assert!(product.path.starts_with(&h.products));
    let bytes = std::fs::read(&product.path).unwrap();
    assert!(!bytes.is_empty());
    assert_eq!(product.size_bytes, bytes.len() as u64);
    assert_eq!(product.mime, "audio/mp4");
    let digest = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(product.sha256, digest);
    assert!(
        bytes.windows(4).any(|chunk| chunk == b"ftyp"),
        "必须是真实M4A容器"
    );
    let decoded = std::process::Command::new("/usr/bin/afinfo")
        .arg(&product.path)
        .output()
        .unwrap();
    assert!(decoded.status.success(), "系统音频解析器必须认可生成文件");
    let files = std::fs::read_dir(&h.products)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(files, vec![product.path]);
    let running = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(format!("{}/speech-say-", h.products.display()))
        .output()
        .unwrap();
    assert_eq!(
        running.status.code(),
        Some(1),
        "本次实际say进程必须已退出；检查命令异常也不能当已退出"
    );
    assert!(running.stdout.is_empty());
    let usage = h.usage().await;
    assert_eq!(usage.len(), 1);
    assert_usage(&usage[0], "system-speech", model, "system-speech", 0);
    assert_eq!(usage[0].purpose, "speech");

    let error = operations
        .transcribe(audio(), Some("zh".into()), None)
        .await
        .err()
        .expect("系统识别必须明确拒绝");
    assert_eq!(error.code, lingxi_protocol::ErrorCode::InvalidMessage);
    assert!(
        error.message.contains("system-speech-recognition")
            && error.message.contains("not implemented")
    );
    assert_eq!(
        h.usage().await.len(),
        1,
        "不支持的系统识别必须派发前拒绝，不能伪造识别或用量"
    );
    drop(operations);
    h.close().await;
}

#[derive(Clone)]
struct Arrival {
    headers: HeaderMap,
    body: Value,
}

async fn bigasr(
    State(arrivals): State<Arc<Mutex<Vec<Arrival>>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (HeaderMap, Json<Value>) {
    arrivals.lock().unwrap().push(Arrival { headers, body });
    let mut headers = HeaderMap::new();
    headers.insert("x-api-status-code", "20000000".parse().unwrap());
    (
        headers,
        Json(json!({"result":{"text":"设置后的火山语音转写"},"audio_info":{"duration":10}})),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_publish_volcengine_speech_reaches_same_operation_actual_bigasr_credentials_and_usage()
{
    let arrivals = Arc::new(Mutex::new(Vec::<Arrival>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/api/v3/auc/bigmodel/recognize/flash", post(bigasr))
        .with_state(arrivals.clone());
    let (stop_external, stopped) = tokio::sync::oneshot::channel();
    let external = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let model = "bigasr-settings-model";
    let h = Harness::boot(json!({
        "providers":{"volcengine-speech":{"protocol":"openai-audio-transcriptions","endpoint":format!("{endpoint}/not-published"),"auth":{"kind":"apiKey","apiKey":"synthetic-old-bigasr-key"},"models":[{"id":model}]}},
        "models":{"speechRecognition":{"provider":"volcengine-speech","model":model}}
    })).await;
    let operations = h.state.operations().unwrap().clone();
    let before = operations.speech_recognition_route().await.unwrap();
    assert_eq!(before.2, "openai-audio-transcriptions");
    h.publish("volcengine-speech", json!({"api":"volcengine-bigasr","base_url":endpoint,"api_key":"synthetic-current-bigasr-key","headers":{"X-Owned-Speech":"current-config"},
        "models":[{"id":model,"inputs":["audio"],"outputs":["text"]}]})).await;
    assert!(Arc::ptr_eq(&operations, h.state.operations().unwrap()));
    assert_eq!(
        operations.speech_recognition_route().await.unwrap(),
        (
            "volcengine-speech".to_string(),
            model.to_string(),
            "volcengine-bigasr".to_string()
        )
    );
    h.assert_summary("volcengine-speech", "speechRecognition")
        .await;
    assert!(arrivals.lock().unwrap().is_empty());
    assert!(h.usage().await.is_empty());
    let input = audio();
    let expected_audio = input.bytes.clone();
    let receipt = operations
        .transcribe(
            input,
            Some("zh".into()),
            Some(lingxi_adapters::models::dispatch::unix_ms_now() + 10_000),
        )
        .await
        .unwrap();
    assert_eq!(receipt.provider, "volcengine-speech");
    assert_eq!(receipt.model, model);
    assert_eq!(receipt.protocol, "volcengine-bigasr");
    assert_eq!(receipt.outcome.text, "设置后的火山语音转写");
    assert_eq!(receipt.outcome.duration_ms, Some(10.0));
    let wire = arrivals.lock().unwrap().clone();
    assert_eq!(wire.len(), 1, "只能是该专用协议真实一次出站");
    assert_eq!(wire[0].headers["x-api-key"], "synthetic-current-bigasr-key");
    assert_eq!(wire[0].headers["x-owned-speech"], "current-config");
    assert_eq!(
        wire[0].headers["x-api-resource-id"],
        "volc.bigasr.auc_turbo"
    );
    assert_eq!(wire[0].headers["x-api-sequence"], "-1");
    assert!(
        !wire[0].headers.contains_key("authorization"),
        "BigASR不能错走Bearer聊天出口"
    );
    let id = wire[0].headers["x-api-request-id"].to_str().unwrap();
    assert_eq!(id.len(), 36);
    assert_eq!(id.as_bytes()[14], b'4');
    assert_eq!(wire[0].body["user"]["uid"], "synthetic-current-bigasr-key");
    assert_eq!(wire[0].body["request"]["model_name"], "bigmodel");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(wire[0].body["audio"]["data"].as_str().unwrap())
            .unwrap(),
        expected_audio
    );
    assert!(!wire[0]
        .body
        .to_string()
        .contains("synthetic-old-bigasr-key"));
    let usage = h.usage().await;
    assert_eq!(usage.len(), 1);
    assert_usage(
        &usage[0],
        "volcengine-speech",
        model,
        "volcengine-bigasr",
        1,
    );
    assert_eq!(usage[0].purpose, "transcribe");
    let published = h.source();
    assert_eq!(
        published["providers"]["volcengine-speech"]["protocol"],
        "volcengine-bigasr"
    );
    assert_eq!(
        published["providers"]["volcengine-speech"]["endpoint"],
        endpoint
    );
    assert_eq!(published["models"]["speechRecognition"]["model"], model);
    drop(operations);
    h.close().await;
    stop_external.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), external)
        .await
        .unwrap()
        .unwrap();
}
