//! R07-T06 工作台媒体动作面（media_generate + media_admin + media_gallery）的
//! 真实回环验证：HTTP 路由 → 合成来源 Run → 同一工具网关 → 真实媒体作业 →
//! 图库投影 → 产物文件服务/系统打开/ASR 转写。只有供应商平台响应是本地回环
//! fixture；绝不经聊天模型通道，fixture 的 /chat/completions 被调用即失败。
use axum::{
    body::Bytes,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use lingxi_kernel::{ports::StoragePort, Principal as KernelPrincipal};
use lingxi_service::{
    approval_service::{Answer, ApprovalService},
    inject::SystemClock,
    prepare_layout,
    toolgateway::ToolInvocationGateway,
    HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
const MP3: &[u8] = b"ID3\x04\0\0\0\0\0\0synthetic-mp3";
const MP4: &[u8] = b"\0\0\0\x18ftypmp42synthetic-video";
const ASR_TEXT: &str = "装配线实录";

/// 供应商回环替身：只充当外部平台，任何宿主授权都不来自这里。
struct Fixture {
    wire: Mutex<Vec<(String, HeaderMap)>>,
    fail_image: AtomicBool,
    fail_asr: AtomicBool,
    video_url: Mutex<String>,
}
impl Fixture {
    fn hits(&self, name: &str) -> usize {
        self.wire
            .lock()
            .unwrap()
            .iter()
            .filter(|(route, _)| route == name)
            .count()
    }
    fn auth_header(&self, name: &str) -> Option<String> {
        self.wire
            .lock()
            .unwrap()
            .iter()
            .find(|(route, _)| route == name)
            .and_then(|(_, headers)| {
                headers
                    .get("authorization")
                    .map(|value| value.to_str().unwrap().to_string())
            })
    }
}
async fn image(State(data): State<Arc<Fixture>>, headers: HeaderMap) -> Response {
    if data.fail_image.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"message":"synthetic image backend rejected"}})),
        )
            .into_response();
    }
    data.wire.lock().unwrap().push(("image".into(), headers));
    Json(json!({"data":[{"b64_json":PNG}]})).into_response()
}
async fn speech(State(data): State<Arc<Fixture>>, headers: HeaderMap) -> Response {
    data.wire.lock().unwrap().push(("speech".into(), headers));
    ([("content-type", "audio/mpeg")], Bytes::from_static(MP3)).into_response()
}
// 视频供应商是异步提交/轮询/下载三段式（agnes-videos 协议），与图片同步
// 响应不同；三段都必须真实命中本替身。
async fn video_submit(State(data): State<Arc<Fixture>>, headers: HeaderMap) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("video-submit".into(), headers));
    Json(json!({"id":"synthetic-video-remote","video_id":"synthetic-video-remote"})).into_response()
}
async fn video_query(State(data): State<Arc<Fixture>>, headers: HeaderMap) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("video-query".into(), headers));
    let url = data.video_url.lock().unwrap().clone();
    Json(json!({"status":"completed","video_url":url})).into_response()
}
async fn video_bytes(State(data): State<Arc<Fixture>>, headers: HeaderMap) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("video-bytes".into(), headers));
    ([("content-type", "video/mp4")], Bytes::from_static(MP4)).into_response()
}
async fn transcribe(State(data): State<Arc<Fixture>>, headers: HeaderMap, body: Bytes) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("transcribe".into(), headers));
    assert!(
        !body.is_empty(),
        "转写请求必须携带真实音频字节（multipart 正文非空）"
    );
    if data.fail_asr.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"message":"synthetic asr backend rejected"}})),
        )
            .into_response();
    }
    Json(json!({"text": ASR_TEXT})).into_response()
}
/// 本面绝不经过聊天模型；被调用即响亮失败（区别于"恰好没走到"）。
async fn chat_catcher() -> Response {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":{"message":"media admin surface must not reach the chat model"}})),
    )
        .into_response()
}

struct Harness {
    home: PathBuf,
    state: ServiceState,
    approvals: Arc<ApprovalService>,
    fixture: Arc<Fixture>,
    opened: Arc<Mutex<Vec<PathBuf>>>,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn boot() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-media-admin-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let fixture = Arc::new(Fixture {
            wire: Mutex::new(Vec::new()),
            fail_image: AtomicBool::new(false),
            fail_asr: AtomicBool::new(false),
            video_url: Mutex::new(String::new()),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let endpoint = format!("http://{address}");
        *fixture.video_url.lock().unwrap() = format!("{endpoint}/product/video");
        let external = Router::new()
            .route("/images/generations", post(image))
            .route("/audio/speech", post(speech))
            .route("/audio/transcriptions", post(transcribe))
            .route("/v1/videos", post(video_submit))
            .route("/agnesapi", get(video_query))
            .route("/product/video", get(video_bytes))
            .route("/chat/completions", post(chat_catcher))
            .with_state(fixture.clone());
        tokio::spawn(async move { axum::serve(listener, external).await.unwrap() });
        // 管理面写模型后经焦点代理刷新 config；焦点指针是持久面事实，
        // 直接以真实 user/preferences.json 落盘（不碰私有 API）。
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            serde_json::to_string(&json!({"current_agent_id":"a"})).unwrap(),
        )
        .unwrap();
        let layout = prepare_layout(&home).unwrap();
        let document = json!({
            "providers": {
                "chat": {"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-chat-secret"}},
                "image": {"protocol":"openai-images","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-image-secret"}},
                "video": {"protocol":"agnes-videos","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-video-secret"}},
                "speech": {"protocol":"openai-audio-speech","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-speech-secret"}},
                "asr": {"protocol":"openai-audio-transcriptions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-asr-secret"}}
            },
            "network": {"proxy":{"mode":"direct"}},
            "models": {
                "chat": {"provider":"chat","model":"fixture-chat","capabilities":{"tools":true}},
                "image": {"provider":"image","model":"gpt-image-1"},
                "video": {"provider":"video","model":"fixture-video"},
                "speech": {"provider":"speech","model":"tts-1"},
                "speechRecognition": {"provider":"asr","model":"whisper-1"}
            }
        });
        let source = home.join("model-plane.json");
        std::fs::write(&source, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        let plane =
            lingxi_adapters::models::config::ModelPlaneConfig::from_value(document).unwrap();
        let clock = Arc::new(SystemClock);
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
        let opened: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = opened.clone();
        let deps = ServiceDeps {
            clock,
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            credential_service: Some(credentials),
            network_plane: Some(lingxi_adapters::models::network::NetworkPlane::direct_isolated()),
            model_plane_source: Some(lingxi_service::config::ModelPlaneSource::ModelsFile {
                path: source,
            }),
            tool_gateway: Some(gateway),
            approval_gate: Some(approvals.clone()),
            media_file_opener: Some(Arc::new(move |path: &std::path::Path| {
                recorder.lock().unwrap().push(path.to_path_buf());
                Ok(())
            })),
            ..Default::default()
        };
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        for (sid, mode) in [
            ("media-a", "operate"),
            ("media-ask", "ask"),
            ("media-read", "read_only"),
        ] {
            state
                .session_admin()
                .create_session(
                    sid,
                    "a",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "媒体工作台验证",
                    Some(mode),
                    &[],
                    false,
                    now,
                )
                .await
                .unwrap();
        }
        let (stop, rx) = tokio::sync::oneshot::channel::<()>();
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
            .unwrap();
        });
        let base = format!("http://{}", address.await.unwrap());
        Self {
            home,
            state,
            approvals,
            fixture,
            opened,
            base,
            stop,
            server,
        }
    }

    async fn http(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.state.auth().local_token());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .timeout(Duration::from_secs(45))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw": text})),
        )
    }

    /// 图库读面轮询到目标状态（真实后台执行链驱动，不设内部快进）。
    async fn gallery_until(&self, task_id: &str, want: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let (status, body) = self
                    .http(
                        reqwest::Method::GET,
                        &format!("/lingxi/v1/media/tasks/{task_id}"),
                        None,
                    )
                    .await;
                if status == 200 {
                    let current = body["task"]["status"].as_str().unwrap_or("");
                    if current == want {
                        return body["task"].clone();
                    }
                    assert!(
                        current == "pending" || want != "done",
                        "任务 {task_id} 走到非预期终态：{body}"
                    );
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("图库任务未在预算内到达目标状态")
    }

    async fn close(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(10), self.server)
            .await
            .unwrap()
            .unwrap();
        self.state.storage().close().await.unwrap();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// POST 生成（可能阻塞在批准门），在后台任务里驱动。
fn spawn_post(
    base: &str,
    token: &str,
    path: &str,
    body: Value,
) -> tokio::task::JoinHandle<(u16, Value)> {
    let url = format!("{base}{path}");
    let token = token.to_string();
    tokio::spawn(async move {
        let response = reqwest::Client::new()
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .timeout(Duration::from_secs(45))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw": text})),
        )
    })
}

impl Harness {
    /// 批准该会话下一个真实挂起（媒体生成是 Review 类工具，任何用户会话模式
    /// 都要一次人工决定——现役裁决语义）；无挂起即超时失败，不伪造通过。
    async fn approve_next(&self, session: &str) {
        let approval_id = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(item) = self
                    .approvals
                    .pending_for_principal(&KernelPrincipal::LocalUser, session)
                    .into_iter()
                    .next()
                {
                    break item.approval_id;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("生成必须进入正式批准");
        assert!(self
            .approvals
            .answer_for_principal(
                &KernelPrincipal::LocalUser,
                session,
                &approval_id,
                Answer::Approve
            )
            .settled());
    }

    /// 一次完整的"HTTP 发起 → 真实人工批准 → 受理回执"生成。
    async fn generate_approved(&self, path: &str, session: &str, body: Value) -> (u16, Value) {
        let pending = spawn_post(&self.base, &self.state.auth().local_token(), path, body);
        self.approve_next(session).await;
        pending.await.unwrap()
    }
}

// ── 生成 → 图库 → 产物服务（A11：受理回执不冒充产物完成）──────────────────

#[tokio::test]
async fn http_image_generate_acceptance_then_gallery_done_and_real_bytes_served() {
    let h = Harness::boot().await;
    let (status, body) = h
        .generate_approved(
            "/lingxi/v1/media/image/generate",
            "media-a",
            json!({"sessionId":"media-a","prompt":"画一只本地测试猫","count":1}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["kind"], "image");
    let task_id = body["batchId"].as_str().expect("受理回执必须有任务编号");
    assert_eq!(body["tasks"][0]["taskId"], task_id);
    // A11：受理回执只带任务事实，绝不在产物交付前宣称文件。
    assert!(body["tasks"][0]["files"].is_null(), "{body}");
    assert_eq!(body["delivery"]["mode"], "session");
    // 来源 Run 以 process_only 收尾：提交完成 ≠ 产物完成。
    let reason = h
        .state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id=\
             (SELECT source_run_id FROM media_jobs_v1 WHERE operation_id=?1)",
            vec![task_id.to_string()],
        )
        .await
        .unwrap()
        .expect("来源 Run 必须真实持久");
    assert_eq!(reason, "completed.no_final.process_only");
    let source = h
        .state
        .storage()
        .query_one_text(
            "SELECT source_run_id FROM media_jobs_v1 WHERE operation_id=?1",
            vec![task_id.to_string()],
        )
        .await
        .unwrap()
        .unwrap();
    let journal = h
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(&source))
        .await
        .unwrap();
    assert_eq!(journal.len(), 1, "{journal:?}");
    let entry = &journal[0];
    assert_eq!(entry.journal_id, format!("{source}-media-generate"));
    assert_eq!(entry.target, "tool:plugin:media:generate-image");
    assert_eq!(entry.session_id, "media-a");
    assert_eq!(
        entry.phase,
        lingxi_kernel::ports::InvocationPhase::Succeeded
    );
    assert_eq!(
        entry.receipt.as_ref().unwrap().outcome,
        lingxi_kernel::ports::ReceiptOutcome::Succeeded
    );
    assert!(entry.receipt.as_ref().unwrap().dispatched);

    // 图库投影与运行时作业同事务同步：轮询到 done 后文件真实登记。
    let task = h.gallery_until(task_id, "done").await;
    assert_eq!(task["type"], "image");
    assert_eq!(task["kind"], "image");
    assert_eq!(task["prompt"], "画一只本地测试猫");
    assert_eq!(task["sessionId"], "media-a");
    let filename = task["files"][0]["filename"]
        .as_str()
        .expect("完成的任务必须有真实文件名");
    // 交付命名是 {kind}-{作业}-{序号}.{实际产物扩展名}（服役 MIME 由扩展名裁决，
    // 与现役平价），不假定 fixture 的编码暗示。
    assert!(filename.starts_with("image-"), "{filename}");
    let file_id = task["files"][0]["fileId"].as_str().unwrap();
    // 产物服务：真实字节 + 现役头（MIME/Accept-Ranges/Cache-Control）。
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/media/generated/{filename}", h.base))
        .bearer_auth(h.state.auth().local_token())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("image/"),
        "{:?}",
        response.headers()
    );
    assert_eq!(response.headers()["accept-ranges"], "bytes");
    let bytes = response.bytes().await.unwrap();
    let expected = base64::engine::general_purpose::STANDARD
        .decode(PNG)
        .unwrap();
    assert_eq!(bytes.as_ref(), expected.as_slice());
    let total = expected.len();
    // Range 窗口精确读取（206/416 与现役平价）。
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/media/generated/{filename}", h.base))
        .bearer_auth(h.state.auth().local_token())
        .header("range", "bytes=0-3")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(
        response.headers()["content-range"],
        &format!("bytes 0-3/{total}").as_str()
    );
    assert_eq!(response.bytes().await.unwrap().as_ref(), &expected[..4]);
    let (status, _) = {
        let response = reqwest::Client::new()
            .get(format!("{}/lingxi/v1/media/generated/{filename}", h.base))
            .bearer_auth(h.state.auth().local_token())
            .header("range", "bytes=999999-1000000")
            .send()
            .await
            .unwrap();
        (response.status().as_u16(), ())
    };
    assert_eq!(status, 416);
    // 不存在/非法文件名：404 / 400（文件名永不参与路径拼接）。
    let (status, _) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/generated/no-such-file.png",
            None,
        )
        .await;
    assert_eq!(status, 404);
    let (status, _) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/generated/%2E%2E",
            None,
        )
        .await;
    assert_eq!(status, 404, "穿越段在路由层即拒，永不进入文件名解析");
    let (status, _) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/generated/a..b.png",
            None,
        )
        .await;
    assert_eq!(status, 400, "含 .. 的文件名在校验层响亮拒绝");
    // 系统打开走注入 opener（录制替身）；录制路径必须是会话文件真实主机路径。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            &format!("/lingxi/v1/media/generated/open/{filename}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    let recorded = h.opened.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1);
    let view = h
        .state
        .session_files()
        .get_file("media-a", file_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recorded[0].to_string_lossy(), view.real_path);
    // 生成列表读面：filter 词表与批次读。
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/tasks?filter=images",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert!(body["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["taskId"] == task_id));
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/tasks?filter=videos",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert!(body["tasks"].as_array().unwrap().is_empty());
    let (status, _) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/tasks?filter=bogus",
            None,
        )
        .await;
    assert_eq!(status, 400);
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            &format!("/lingxi/v1/media/tasks/batch/{task_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["tasks"][0]["taskId"], task_id);
    // 凭据绝不出现在任何媒体面输出里。
    assert!(!body.to_string().contains("synthetic-image-secret"));
    h.close().await;
}

// ── 批准门与互斥准入 ────────────────────────────────────────────────────────

#[tokio::test]
async fn ask_mode_waits_for_real_decision_and_blocks_concurrent_admission() {
    let h = Harness::boot().await;
    let token = h.state.auth().local_token().to_string();
    let pending = spawn_post(
        &h.base,
        &token,
        "/lingxi/v1/media/image/generate",
        json!({"sessionId":"media-ask","prompt":"需要批准的生成"}),
    );
    // 批准门真实挂起（决定前绝不派发外部请求）。
    let approval_id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(item) = h
                .approvals
                .pending_for_principal(&KernelPrincipal::LocalUser, "media-ask")
                .into_iter()
                .next()
            {
                break item.approval_id;
            }
            assert!(!pending.is_finished(), "批准后才会返回的请求提前结束");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("ask 会话的生成必须进入正式批准");
    assert_eq!(h.fixture.hits("image"), 0, "批准前不得派发外部生成");
    // 会话准入互斥：同会话第二个 HTTP 生成响亮 409（登记的偏差：现役允许并发）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/image/generate",
            Some(json!({"sessionId":"media-ask","prompt":"并发第二次"})),
        )
        .await;
    assert_eq!(status, 409, "{body}");
    // 批准后完成受理。
    assert!(h
        .approvals
        .answer_for_principal(
            &KernelPrincipal::LocalUser,
            "media-ask",
            &approval_id,
            Answer::Approve
        )
        .settled());
    let (status, body) = pending.await.unwrap();
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    let first_task = body["batchId"].as_str().unwrap().to_string();
    h.gallery_until(&first_task, "done").await;
    assert_eq!(h.fixture.hits("image"), 1);
    // 拒绝路径：零派发 + 如实 403。
    let rejected = spawn_post(
        &h.base,
        &token,
        "/lingxi/v1/media/image/generate",
        json!({"sessionId":"media-ask","prompt":"会被拒绝的生成"}),
    );
    let approval_id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(item) = h
                .approvals
                .pending_for_principal(&KernelPrincipal::LocalUser, "media-ask")
                .into_iter()
                .next()
            {
                break item.approval_id;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(h
        .approvals
        .answer_for_principal(
            &KernelPrincipal::LocalUser,
            "media-ask",
            &approval_id,
            Answer::Reject {
                reason: "本地测试拒绝".into()
            }
        )
        .settled());
    let (status, body) = rejected.await.unwrap();
    assert_eq!(status, 403, "{body}");
    assert_eq!(h.fixture.hits("image"), 1, "拒绝绝不派发外部生成");
    let source = h
        .state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE session_id='media-ask' ORDER BY rowid DESC LIMIT 1",
            vec![],
        )
        .await
        .unwrap()
        .unwrap();
    let journal = h
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(source))
        .await
        .unwrap();
    assert_eq!(journal.len(), 1, "{journal:?}");
    let entry = &journal[0];
    assert_eq!(entry.target, "tool:plugin:media:generate-image");
    assert_eq!(entry.phase, lingxi_kernel::ports::InvocationPhase::Failed);
    assert_eq!(
        entry.receipt.as_ref().unwrap().outcome,
        lingxi_kernel::ports::ReceiptOutcome::Failed
    );
    assert!(!entry.receipt.as_ref().unwrap().dispatched);
    assert!(!h.state.storage().session_admission().is_busy("media-ask"));

    h.close().await;
}

/// HTTP媒体动作的批准等待期间撤权/取消，真实来源Run与调用回执必须一起收尾且无供应商请求。
#[tokio::test]
async fn direct_media_action_waiting_for_approval_honors_cancel_and_live_plugin_revocation() {
    for cancel in [true, false] {
        let h = Harness::boot().await;
        let pending = spawn_post(
            &h.base,
            &h.state.auth().local_token(),
            "/lingxi/v1/media/image/generate",
            json!({"sessionId":"media-ask","prompt":"批准等待期间取消或撤权"}),
        );
        let approval = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(item) = h
                    .approvals
                    .pending_for_principal(&KernelPrincipal::LocalUser, "media-ask")
                    .into_iter()
                    .next()
                {
                    break item;
                }
                assert!(!pending.is_finished());
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(h.fixture.hits("image"), 0);
        if cancel {
            assert_eq!(
                h.state
                    .runs()
                    .cancel_run(&approval.run_id, "用户在媒体批准前取消"),
                lingxi_service::cancel::FireOutcome::Fired
            );
        } else {
            std::fs::write(
                h.state.memory().config_path("a").unwrap(),
                "agent:\n  name: a\nmemory:\n  enabled: false\ntools:\n  disabled: [media]\n",
            )
            .unwrap();
            assert!(h
                .approvals
                .answer_for_principal(
                    &KernelPrincipal::LocalUser,
                    "media-ask",
                    &approval.approval_id,
                    Answer::Approve
                )
                .settled());
        }
        let (status, body) = tokio::time::timeout(Duration::from_secs(10), pending)
            .await
            .unwrap()
            .unwrap();
        assert!((400..500).contains(&status), "cancel={cancel}: {body}");
        assert_ne!(body["ok"], true);
        assert_eq!(h.fixture.hits("image"), 0);
        let run = h
            .state
            .storage()
            .load_run(&lingxi_protocol::RunId::new(&approval.run_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            run.status,
            if cancel {
                lingxi_protocol::RunStatus::Cancelled
            } else {
                lingxi_protocol::RunStatus::Failed
            }
        );
        let journal = h
            .state
            .storage()
            .load_invocation_journal(&lingxi_protocol::RunId::new(&approval.run_id))
            .await
            .unwrap();
        assert_eq!(journal.len(), 1, "{journal:?}");
        assert_eq!(
            journal[0].phase,
            lingxi_kernel::ports::InvocationPhase::Failed
        );
        let receipt = journal[0].receipt.as_ref().unwrap();
        assert_eq!(
            receipt.outcome,
            lingxi_kernel::ports::ReceiptOutcome::Failed
        );
        // 插件撤权可由真实工具执行器拒绝，取消批准则连工具都没有派发。
        if cancel {
            assert!(!receipt.dispatched);
        }
        assert!(!h.state.storage().session_admission().is_busy("media-ask"));
        assert!(h
            .approvals
            .pending_for_principal(&KernelPrincipal::LocalUser, "media-ask")
            .is_empty());
        h.close().await;
    }
}

#[tokio::test]
async fn read_only_session_generate_is_forbidden_without_dispatch() {
    let h = Harness::boot().await;
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/image/generate",
            Some(json!({"sessionId":"media-read","prompt":"只读会话不得生成"})),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(h.fixture.hits("image"), 0);
    // 缺 sessionId / 空提示词 / 未知会话：响亮 4xx，不猜默认。
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/image/generate",
            Some(json!({"prompt":"缺会话"})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/image/generate",
            Some(json!({"sessionId":"media-a","prompt":"  "})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/image/generate",
            Some(json!({"sessionId":"absent","prompt":"x"})),
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(h.fixture.hits("image"), 0);
    h.close().await;
}

// ── 语音 response 投递（朗读链：终态到达后才回文件）────────────────────────

#[tokio::test]
async fn speech_response_delivery_waits_for_terminal_and_returns_real_files() {
    let h = Harness::boot().await;
    let (status, body) = h
        .generate_approved(
            "/lingxi/v1/media/speech/generate",
            "media-a",
            json!({
                "sessionId":"media-a",
                "prompt":"朗读这句本地测试文本",
                "delivery":{"mode":"response"}
            }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["delivery"]["mode"], "response");
    let task_id = body["batchId"].as_str().unwrap();
    let filename = body["tasks"][0]["files"][0]
        .as_str()
        .expect("response 投递的语音在终态后必须回真实文件名");
    assert!(filename.ends_with(".mp3"), "{filename}");
    let task = h.gallery_until(task_id, "done").await;
    assert_eq!(task["type"], "speech");
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/media/generated/{filename}", h.base))
        .bearer_auth(h.state.auth().local_token())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "audio/mpeg");
    assert_eq!(response.bytes().await.unwrap().as_ref(), MP3);
    // 现役 normalizeMediaDelivery：delivery.mode 只有显式 response 才是 response。
    let (status, body) = h
        .generate_approved(
            "/lingxi/v1/media/speech/generate",
            "media-a",
            json!({
                "sessionId":"media-a",
                "prompt":"会话投递的语音",
                "delivery":{"mode":"session"}
            }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["delivery"]["mode"], "session");
    assert!(body["tasks"][0]["files"].is_null(), "{body}");
    h.gallery_until(body["batchId"].as_str().unwrap(), "done")
        .await;
    h.close().await;
}

// ── 视频 HTTP 生成（异步提交/轮询/下载三段式，受理不冒充完成）──────────────

#[tokio::test]
async fn http_video_generate_acceptance_then_gallery_done_and_real_bytes_served() {
    let h = Harness::boot().await;
    let (status, body) = h
        .generate_approved(
            "/lingxi/v1/media/video/generate",
            "media-a",
            json!({"sessionId":"media-a","prompt":"生成一段本地验证视频"}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["delivery"]["mode"], "session");
    assert!(
        body["tasks"][0]["files"].is_null(),
        "受理回执不得携带产物文件（A11）：{body}"
    );
    let task_id = body["batchId"].as_str().unwrap();
    let task = h.gallery_until(task_id, "done").await;
    assert_eq!(task["type"], "video");
    assert!(h.fixture.hits("video-submit") >= 1, "必须真实提交远端");
    assert!(h.fixture.hits("video-query") >= 1, "必须真实轮询远端");
    assert_eq!(h.fixture.hits("video-bytes"), 1, "产物必须真实下载一次");
    assert_eq!(
        h.fixture.auth_header("video-submit").as_deref(),
        Some("Bearer synthetic-video-secret"),
        "视频供应商必须收到自己的凭据"
    );
    // 画廊完成态登记的产物经文件服务回真实字节。
    let filename = task["files"][0]["filename"]
        .as_str()
        .expect("完成态任务必须登记真实产物文件名");
    assert!(filename.starts_with("video-"), "{filename}");
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/media/generated/{filename}", h.base))
        .bearer_auth(h.state.auth().local_token())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "video/mp4");
    assert_eq!(response.bytes().await.unwrap().as_ref(), MP4);
    // 任务读取面不泄漏供应商凭据。
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            &format!("/lingxi/v1/media/tasks/{task_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.to_string().contains("synthetic-video-secret"));
    h.close().await;
}

// ── 重试（现役 retryImageTask：仅图片、仅终态、换绑保连续性）───────────────
#[tokio::test]
async fn failed_image_task_retries_into_new_task_with_attempt_continuity() {
    let h = Harness::boot().await;
    h.fixture.fail_image.store(true, Ordering::SeqCst);
    let (status, body) = h
        .generate_approved(
            "/lingxi/v1/media/image/generate",
            "media-a",
            json!({"sessionId":"media-a","prompt":"注定失败的图"}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let old_task = body["batchId"].as_str().unwrap().to_string();
    let failed = h.gallery_until(&old_task, "failed").await;
    assert!(
        !failed["failReason"].as_str().unwrap_or("").is_empty(),
        "失败原因必须如实登记：{failed}"
    );
    assert_eq!(failed["attempt"], 1);
    h.fixture.fail_image.store(false, Ordering::SeqCst);
    // 重试铸新运行作业（同样经批准门：一次真实人工决定）；工作台任务换绑，
    // attempt/retryCount 连续。
    let (status, body) = h
        .generate_approved(
            &format!("/lingxi/v1/media/tasks/{old_task}/retry"),
            "media-a",
            json!({}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    let new_task = body["taskId"].as_str().unwrap().to_string();
    assert_ne!(new_task, old_task, "新运行身份绝不复用旧编号");
    assert_eq!(body["placeholder"]["status"], "pending");
    assert_eq!(body["placeholder"]["prompt"], "注定失败的图");
    let (status, _) = h
        .http(
            reqwest::Method::GET,
            &format!("/lingxi/v1/media/tasks/{old_task}"),
            None,
        )
        .await;
    assert_eq!(status, 404, "旧编号已换绑，不再指向任何工作台任务");
    let done = h.gallery_until(&new_task, "done").await;
    assert_eq!(done["attempt"], 2);
    assert_eq!(done["retryCount"], 1);
    assert!(done["files"][0]["filename"].as_str().is_some());
    // 现役门禁：非终态/已完成任务不得重试（409），非图片任务 400。
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            &format!("/lingxi/v1/media/tasks/{new_task}/retry"),
            None,
        )
        .await;
    assert_eq!(status, 409);
    let (status, body) = h
        .generate_approved(
            "/lingxi/v1/media/speech/generate",
            "media-a",
            json!({"sessionId":"media-a","prompt":"一条语音任务"}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let speech_task = body["batchId"].as_str().unwrap().to_string();
    h.gallery_until(&speech_task, "done").await;
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            &format!("/lingxi/v1/media/tasks/{speech_task}/retry"),
            None,
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/tasks/no-such-task/retry",
            None,
        )
        .await;
    assert_eq!(status, 404);
    h.close().await;
}

// ── 管理面（供应商/模型/偏好/ASR 配置）─────────────────────────────────────

#[tokio::test]
async fn admin_models_config_and_asr_preferences_roundtrip() {
    let h = Harness::boot().await;
    // 新增独立于预置槽位的模型，经真实 Source 发布；保留原增改删与默认偏好断言。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/image/providers/image/models",
            Some(json!({"id":"gpt-image-1-added","protocolId":"openai-images","displayName":"GPT 图像"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/image/providers",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let provider = &body["providers"]["image"];
    assert_eq!(provider["hasCredentials"], true);
    let model = provider["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["id"] == "gpt-image-1-added")
        .expect("新加模型必须出现在列表")
        .clone();
    assert_eq!(model["displayName"], "GPT 图像");
    assert_eq!(model["adapterAvailable"], true);
    // 重复添加幂等（现役 addMediaModel 语义）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/image/providers/image/models",
            Some(json!({"id":"gpt-image-1-added","protocolId":"openai-images"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    // 缺 protocolId 响亮拒绝（不猜调用方式）。
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/image/providers/image/models",
            Some(json!({"id":"no-protocol"})),
        )
        .await;
    assert_eq!(status, 400);
    // 编辑：合法补丁通过；不可编辑字段响亮 400。
    let (status, body) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/image/providers/image/models/gpt-image-1-added",
            Some(json!({"displayName":"改名后的图像模型","inputs":["text"],"outputs":["image"]})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, _) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/image/providers/image/models/gpt-image-1-added",
            Some(json!({"protocolId":"openai-completions"})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/image/providers/image/models/gpt-image-1-added",
            Some(json!({"outputs":["text"]})),
        )
        .await;
    assert_eq!(status, 400, "图片模型必须含 image 输出模态");
    // 配置写：默认模型必须可解析；随后读面如实回显。
    let (status, body) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/image/config",
            Some(json!({"defaultImageModel":{"provider":"image","id":"gpt-image-1-added"}})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/image/providers",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["config"]["defaultImageModel"],
        json!({"provider":"image","id":"gpt-image-1-added"})
    );
    let (status, _) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/image/config",
            Some(json!({"defaultImageModel":{"provider":"image","id":"invented-model"}})),
        )
        .await;
    assert_eq!(status, 400, "不可解析的默认模型不得落库");
    // 删除模型后默认引用失效前的列表事实。
    let (status, body) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/media/image/providers/image/models/gpt-image-1-added",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/image/providers",
            None,
        )
        .await;
    assert_eq!(status, 200);
    let still = body["providers"]["image"]["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .any(|model| model["id"] == "gpt-image-1-added")
        })
        .unwrap_or(false);
    assert!(!still, "删除后不得残留于有效模型");
    let (status, _) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/media/image/providers/image/models/gpt-image-1-added",
            None,
        )
        .await;
    assert_eq!(
        status, 400,
        "删除不存在条目如实报错（现役 invalid_message 语义）"
    );
    // ASR 面：模型 + enabled/defaultModel 偏好（默认模型须在列表内且适配可用）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/speech-recognition/providers/asr/models",
            Some(json!({"id":"whisper-1","protocolId":"openai-audio-transcriptions","displayName":"耳语"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/speech-recognition/config",
            Some(json!({"enabled":true,"defaultModel":{"provider":"asr","id":"whisper-1"}})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/speech-recognition/providers",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["config"]["enabled"], true);
    assert_eq!(
        body["config"]["defaultModel"],
        json!({"provider":"asr","id":"whisper-1"})
    );
    let asr_model = body["providers"]["asr"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["id"] == "whisper-1")
        .expect("ASR 模型必须出现在列表")
        .clone();
    assert_eq!(asr_model["adapterAvailable"], true);
    let (status, _) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/speech-recognition/config",
            Some(json!({"defaultModel":{"provider":"asr","id":"invented"}})),
        )
        .await;
    assert_eq!(status, 400, "不在列表内的 ASR 默认模型不得落库");
    // 语音配置读面（现役 getSpeechConfig 平价）。
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/media/speech/config", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["config"].is_object());
    // 语音模型写面 + 配置写（同一参数化处理器，speech 类别不变量：必须含 audio 输出）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/speech/providers/speech/models",
            Some(json!({"id":"tts-1-added","protocolId":"openai-audio-speech","displayName":"合成语音二"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/speech/providers",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["providers"]["speech"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["id"] == "tts-1-added"));
    let (status, _) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/speech/providers/speech/models/tts-1-added",
            Some(json!({"outputs":["text"]})),
        )
        .await;
    assert_eq!(status, 400, "语音模型必须含 audio 输出模态");
    let (status, body) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/speech/providers/speech/models/tts-1-added",
            Some(json!({"displayName":"合成语音二改","outputs":["audio"]})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/speech/config",
            Some(json!({"defaultSpeechModel":{"provider":"speech","id":"tts-1-added"}})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, _) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/media/speech/providers/speech/models/tts-1-added",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, _) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/media/speech/providers/speech/models/tts-1-added",
            None,
        )
        .await;
    assert_eq!(status, 400);
    // 视频模型写面 + 配置写（video 类别不变量：必须含 video 输出；默认模型取
    // 平面已绑定的 fixture-video——非语音类要求模型在声明目录或当前绑定内）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/video/providers/video/models",
            Some(json!({"id":"fixture-video-added","protocolId":"agnes-videos","displayName":"合成视频二"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/video/providers",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["providers"]["video"]["hasCredentials"], true);
    assert!(body["providers"]["video"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["id"] == "fixture-video-added"));
    let (status, _) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/video/providers/video/models/fixture-video-added",
            Some(json!({"outputs":["image"]})),
        )
        .await;
    assert_eq!(status, 400, "视频模型必须含 video 输出模态");
    let (status, body) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/video/providers/video/models/fixture-video-added",
            Some(json!({"displayName":"合成视频二改","outputs":["video"]})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/media/video/config",
            Some(json!({"defaultVideoModel":{"provider":"video","id":"fixture-video"}})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["config"]["defaultVideoModel"],
        json!({"provider":"video","id":"fixture-video"})
    );
    let (status, _) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/media/video/providers/video/models/fixture-video-added",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, _) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/media/video/providers/video/models/fixture-video-added",
            None,
        )
        .await;
    assert_eq!(status, 400);
    // 通用供应商查询路由（GET /lingxi/v1/media/providers?capability=）四类能力词表：
    // 与各类能力的专属列表路由共用同一 list_payload，响应必须逐字节一致。
    for (query, dedicated) in [
        ("image_generation", "/lingxi/v1/media/image/providers"),
        ("video_generation", "/lingxi/v1/media/video/providers"),
        ("speech_generation", "/lingxi/v1/media/speech/providers"),
        (
            "speech_recognition",
            "/lingxi/v1/speech-recognition/providers",
        ),
    ] {
        let (status, generic) = h
            .http(
                reqwest::Method::GET,
                &format!("/lingxi/v1/media/providers?capability={query}"),
                None,
            )
            .await;
        assert_eq!(status, 200, "{query}: {generic}");
        let (status, direct) = h.http(reqwest::Method::GET, dedicated, None).await;
        assert_eq!(status, 200, "{query}: {direct}");
        assert_eq!(generic, direct, "{query} 通用查询与专属路由负载必须一致");
        let providers = generic["providers"]
            .as_object()
            .expect("providers 必须是对象");
        assert!(!providers.is_empty(), "{query} 目录不得为空: {generic}");
        for (pid, entry) in providers {
            assert_eq!(entry["providerId"], json!(pid), "{query}/{pid}");
            assert!(entry["models"].is_array(), "{query}/{pid}: {entry}");
            assert!(
                entry["availableModels"].is_array(),
                "{query}/{pid}: {entry}"
            );
            assert!(
                entry["hasCredentials"].is_boolean(),
                "{query}/{pid}: {entry}"
            );
        }
    }
    // ASR 模型编辑与移除（增删改整链；类别不变量：inputs 含 audio、outputs 含 text）。
    let (status, body) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/speech-recognition/providers/asr/models/whisper-1",
            Some(json!({"displayName":"耳语改","inputs":["audio"],"outputs":["text"]})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, _) = h
        .http(
            reqwest::Method::PUT,
            "/lingxi/v1/speech-recognition/providers/asr/models/whisper-1",
            Some(json!({"inputs":["text"]})),
        )
        .await;
    assert_eq!(status, 400, "识别模型必须含 audio 输入模态");
    let (status, body) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/speech-recognition/providers/asr/models/whisper-1",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, _) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/speech-recognition/providers/asr/models/whisper-1",
            None,
        )
        .await;
    assert_eq!(status, 400);
    // 能力词表之外：404（不是静默落到图片面）。
    let (status, _) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/media/hologram/providers",
            None,
        )
        .await;
    assert_eq!(status, 404);
    h.close().await;
}

// ── ASR 转写（现役 _transcribeAudioWithinTrace：pending→终态事件，失败如实）─

#[tokio::test]
async fn asr_transcribe_roundtrip_events_and_failure_honesty() {
    let h = Harness::boot().await;
    let file = h
        .state
        .session_files()
        .register_blob(
            "media-a",
            Some("voice-note.mp3"),
            "audio/mpeg",
            b"synthetic voice note bytes",
            "user_upload",
        )
        .await
        .unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/asr/transcribe",
            Some(json!({"sessionId":"media-a","fileId":file.file_id,"language":"zh"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "ready");
    assert_eq!(body["text"], ASR_TEXT);
    assert_eq!(body["providerId"], "asr");
    assert_eq!(body["modelId"], "whisper-1");
    assert_eq!(body["protocolId"], "openai-audio-transcriptions");
    assert_eq!(body["language"], "zh");
    // 真实承运：回环供应商收到带凭据的请求；事件流 pending→ready 齐全。
    assert_eq!(
        h.fixture.auth_header("transcribe").as_deref(),
        Some("Bearer synthetic-asr-secret")
    );
    let events = h.state.app_events().since(0);
    let updates: Vec<&Value> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| {
            event["type"] == "voice_transcription_update"
                && event["payload"]["fileId"] == file.file_id
        })
        .collect();
    assert_eq!(updates.len(), 2, "pending 与终态各一条：{events}");
    assert_eq!(updates[0]["payload"]["transcription"]["status"], "pending");
    assert_eq!(updates[1]["payload"]["transcription"]["status"], "ready");
    // 失败如实：200 + status failed（CODE: message），绝不报 ready。
    h.fixture.fail_asr.store(true, Ordering::SeqCst);
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/asr/transcribe",
            Some(json!({"sessionId":"media-a","fileId":file.file_id})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "failed");
    assert!(
        body["error"].as_str().unwrap_or("").contains(':'),
        "失败透出 CODE: message：{body}"
    );
    let events = h.state.app_events().since(0);
    let updates: Vec<&Value> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| {
            event["type"] == "voice_transcription_update"
                && event["payload"]["fileId"] == file.file_id
        })
        .collect();
    assert_eq!(updates.len(), 4, "两次转写各 pending+终态：{events}");
    assert_eq!(updates[3]["payload"]["transcription"]["status"], "failed");
    h.fixture.fail_asr.store(false, Ordering::SeqCst);
    // 覆盖路由响亮 400（运营层只解析已配置默认路由，不静默改用默认）。
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/asr/transcribe",
            Some(json!({"sessionId":"media-a","fileId":file.file_id,"provider":"other"})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/asr/transcribe",
            Some(json!({"sessionId":"media-a","fileId":file.file_id,"model":"invented"})),
        )
        .await;
    assert_eq!(status, 400);
    // 缺参/未知文件/未知会话：响亮 4xx。
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/asr/transcribe",
            Some(json!({"sessionId":"media-a"})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/asr/transcribe",
            Some(json!({"sessionId":"media-a","fileId":"absent-file"})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/asr/transcribe",
            Some(json!({"sessionId":"absent","fileId":file.file_id})),
        )
        .await;
    assert_eq!(status, 404);
    h.close().await;
}

// ── 统一 generate 派发面（现役 generateMedia 的 kind 词表）─────────────────

#[tokio::test]
async fn unified_generate_dispatch_accepts_kind_vocabulary_and_rejects_unknown() {
    let h = Harness::boot().await;
    let (status, body) = h
        .generate_approved(
            "/lingxi/v1/media/generate",
            "media-a",
            json!({"kind":"image_generation","sessionId":"media-a","prompt":"统一派发入口"}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["kind"], "image");
    h.gallery_until(body["batchId"].as_str().unwrap(), "done")
        .await;
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/media/generate",
            Some(json!({"kind":"hologram","sessionId":"media-a","prompt":"x"})),
        )
        .await;
    assert_eq!(status, 400);
    h.close().await;
}
