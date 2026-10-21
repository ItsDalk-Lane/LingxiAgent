//! 仅外部模型和供应商响应替换为回环服务；业务任务、批准、存储、下载及会话文件均为正式实现。
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
use lingxi_protocol::RunId;
use lingxi_service::{
    approval_service::{Answer, ApprovalService},
    inject::SystemClock,
    prepare_layout,
    toolgateway::ToolInvocationGateway,
    ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
struct External {
    kind: Mutex<String>,
    args: Mutex<Value>,
    wire: Mutex<Vec<(String, HeaderMap, Value)>>,
    generated: AtomicUsize,
    fail_generation: AtomicBool,
    queries: AtomicUsize,
    downloaded: AtomicUsize,
    video_url: Mutex<String>,
    release: tokio::sync::Semaphore,
    gate: bool,
}
impl External {
    fn new(kind: &str, gate: bool) -> Self {
        Self {
            kind: Mutex::new(kind.into()),
            args: Mutex::new(json!({"prompt":"本地合成测试，不是付费请求"})),
            wire: Mutex::new(Vec::new()),
            generated: AtomicUsize::new(0),
            fail_generation: AtomicBool::new(false),
            queries: AtomicUsize::new(0),
            downloaded: AtomicUsize::new(0),
            video_url: Mutex::new(String::new()),
            release: tokio::sync::Semaphore::new(0),
            gate,
        }
    }
}
fn sse(frames: Vec<Value>) -> Response {
    let mut output = String::new();
    for frame in frames {
        output.push_str(&format!("data: {frame}\n\n"));
    }
    output.push_str("data: [DONE]\n\n");
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        output,
    )
        .into_response()
}
async fn chat(
    State(data): State<Arc<External>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("chat".into(), headers, body.clone()));
    let count = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    let kind = data.kind.lock().unwrap().clone();
    let jimeng = kind.strip_prefix("jimeng-");
    let names = match jimeng {
        Some(jimeng_kind) => vec![
            "get-guide".to_owned(),
            "describe-options".to_owned(),
            format!("generate-{jimeng_kind}"),
        ],
        None => vec!["get-guide".to_owned(), format!("generate-{kind}")],
    };
    if count >= names.len() * 2 {
        return sse(vec![
            json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"已受理，本地测试会继续核对真实文件。"},"finish_reason":null}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":3}}),
        ]);
    }
    let tool = &names[count / 2];
    let describing = count % 2 == 0;
    let suffix = if describing {
        "mcp_describe_tool"
    } else {
        "mcp_call"
    };
    let arguments = if describing {
        json!({"name":tool,"server":"media"})
    } else {
        let args = if tool == "get-guide" {
            json!({})
        } else if tool == "describe-options" {
            json!({"kind":jimeng.unwrap(),"provider":"jimeng-cli"})
        } else {
            let mut args = data.args.lock().unwrap().clone();
            if jimeng.is_some() {
                args["provider"] = json!("jimeng-cli");
            }
            args
        };
        json!({"tool":tool,"server":"media","arguments":args})
    };
    let name = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["function"]["name"].as_str())
        .find(|name| name.ends_with(suffix))
        .expect("正式模型请求必须携带媒体工具");
    sse(vec![
        json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":format!("external-media-call-{count}"),"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":3}}),
    ])
}
async fn image(
    State(data): State<Arc<External>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("image".into(), headers, body));
    data.generated.fetch_add(1, Ordering::SeqCst);
    if data.gate {
        data.release.acquire().await.unwrap().forget();
    }
    Json(json!({"data":[{"b64_json":PNG}]})).into_response()
}
async fn speech(
    State(data): State<Arc<External>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("speech".into(), headers, body));
    data.generated.fetch_add(1, Ordering::SeqCst);
    if data.gate {
        data.release.acquire().await.unwrap().forget();
    }
    (
        [("content-type", "audio/mpeg")],
        Bytes::from_static(b"ID3\x04\0\0\0\0\0\0synthetic-mp3"),
    )
        .into_response()
}
async fn video_submit(
    State(data): State<Arc<External>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("video-submit".into(), headers, body));
    data.generated.fetch_add(1, Ordering::SeqCst);
    Json(json!({"id":"synthetic-video-remote","video_id":"synthetic-video-remote"})).into_response()
}
async fn video_query(State(data): State<Arc<External>>, headers: HeaderMap) -> Response {
    data.wire
        .lock()
        .unwrap()
        .push(("video-query".into(), headers, Value::Null));
    data.queries.fetch_add(1, Ordering::SeqCst);
    if data.fail_generation.load(Ordering::SeqCst) {
        return Json(
            json!({"status":"failed","error":"synthetic-video-secret generation rejected"}),
        )
        .into_response();
    }
    // 此 fixture 的端点由测试启动后写入；只作为供应商响应，不是宿主授权。
    if data.gate && data.release.available_permits() == 0 {
        return Json(json!({"status":"processing"})).into_response();
    }
    let url = data.video_url.lock().unwrap().clone();
    Json(json!({"status":"completed","video_url":url})).into_response()
}
async fn video_bytes(State(data): State<Arc<External>>, headers: HeaderMap) -> Response {
    assert!(
        headers.get("authorization").is_none(),
        "下载产物绝不携带生成凭据"
    );
    data.downloaded.fetch_add(1, Ordering::SeqCst);
    (
        [("content-type", "video/mp4")],
        Bytes::from_static(b"\0\0\0\x18ftypmp42synthetic-video"),
    )
        .into_response()
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    approvals: Arc<ApprovalService>,
    external: Arc<External>,
    fixture: tokio::task::JoinHandle<()>,
    deps: ServiceDeps,
}
impl Harness {
    async fn new(kind: &str, gate: bool) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-media-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let external = Arc::new(External::new(kind, gate));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let endpoint = format!("http://{address}");
        *external.video_url.lock().unwrap() = format!("{endpoint}/product/video");
        let router = Router::new()
            .route("/chat/completions", post(chat))
            .route("/images/generations", post(image))
            .route("/audio/speech", post(speech))
            .route("/v1/videos", post(video_submit))
            .route("/agnesapi", get(video_query))
            .route("/product/video", get(video_bytes))
            .with_state(external.clone());
        let fixture = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let layout = prepare_layout(&home).unwrap();
        let plane=lingxi_adapters::models::config::ModelPlaneConfig::from_value(json!({"providers":{"chat":{"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-chat-secret"}},"image":{"protocol":"openai-images","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-image-secret"}},"video":{"protocol":"agnes-videos","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-video-secret"}},"speech":{"protocol":"openai-audio-speech","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-speech-secret"}}},"models":{"chat":{"provider":"chat","model":"fixture-chat","capabilities":{"tools":true}},"image":{"provider":"image","model":"gpt-image-1"},"video":{"provider":"video","model":"fixture-video"},"speech":{"provider":"speech","model":"tts-1"}}})).unwrap();
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
        let deps = ServiceDeps {
            clock,
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            credential_service: Some(credentials),
            tool_gateway: Some(gateway),
            approval_gate: Some(approvals.clone()),
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
            deps.clone(),
        )
        .await
        .unwrap();
        state
            .session_admin()
            .create_session(
                "media-a",
                "a",
                lingxi_kernel::LOCAL_OWNER_SUBJECT,
                "媒体验证",
                Some("ask"),
                &[],
                false,
                now(),
            )
            .await
            .unwrap();
        Self {
            home,
            state,
            approvals,
            external,
            fixture,
            deps,
        }
    }
    async fn restart(&mut self) {
        assert!(self
            .state
            .media()
            .stop(Duration::from_secs(5))
            .await
            .unwrap()
            .is_empty());
        self.state.storage().close().await.unwrap();
        let layout = prepare_layout(&self.home).unwrap();
        // 新宿主建立新的正式目录与调用日志入口，不沿用旧进程已注册的网关。
        self.deps.tool_gateway = Some(Arc::new(ToolInvocationGateway::new(
            Arc::new(lingxi_kernel::toolcatalog::ToolRegistry::new()),
            self.approvals.clone(),
            self.deps.clock.clone(),
            Default::default(),
            300000,
            1000,
        )));
        self.state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: self.home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            self.deps.clone(),
        )
        .await
        .unwrap();
    }
    async fn wait_parent(&self, run: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if self
                    .state
                    .storage()
                    .load_run(&RunId::new(run))
                    .await
                    .unwrap()
                    .unwrap()
                    .status
                    .is_terminal()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn submit(&self) -> String {
        self.submit_with_request(None).await
    }
    async fn submit_with_request(&self, request_id: Option<&str>) -> String {
        let principal = self
            .state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", self.state.auth().local_token())),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        self.state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &principal,
                "media-a",
                &ExecuteSubmission {
                    input: "执行本地媒体生成验证",
                    knowledge: None,
                    request_id,
                },
                now(),
            )
            .await
            .unwrap()
            .run_id
    }
    async fn approve(&self, run: &str) {
        let id = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(item) = self
                    .approvals
                    .pending_for_principal(&KernelPrincipal::LocalUser, "media-a")
                    .into_iter()
                    .find(|p| p.run_id == run)
                {
                    break item.approval_id;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("外部生成须先进入正式批准");
        assert!(self
            .approvals
            .answer_for_principal(&KernelPrincipal::LocalUser, "media-a", &id, Answer::Approve)
            .settled());
    }
    async fn wait_generated(&self) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while self.external.generated.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("批准后的正式外部请求必须到达测试供应商");
    }
    async fn job(&self) -> lingxi_adapters::storage::media::MediaJob {
        tokio::time::timeout(Duration::from_secs(10),async{loop{
            if let Some(id)=self.state.storage().query_one_text("SELECT operation_id FROM media_jobs_v1 WHERE session_id='media-a' ORDER BY created_at DESC LIMIT 1",vec![]).await.unwrap(){
                let job=lingxi_adapters::storage::media::get(self.state.storage(),&id).await.unwrap().unwrap();
                if job.execution_run_id.is_some()||job.phase.terminal(){break job;}
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }}).await.unwrap()
    }
    async fn terminal_job(&self) -> lingxi_adapters::storage::media::MediaJob {
        let id = self.job().await.operation_id;
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let job = lingxi_adapters::storage::media::get(self.state.storage(), &id)
                    .await
                    .unwrap()
                    .unwrap();
                if job.phase.terminal() {
                    break job;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("真实媒体后台任务必须结算")
    }
    async fn stop(self) {
        self.external.release.add_permits(32);
        self.state.loop_services().stop_idle_listener();
        self.state.loop_services().alarm.dispose();
        self.state.loop_services().controller.dispose();
        assert!(
            self.state
                .media()
                .stop(Duration::from_secs(5))
                .await
                .unwrap()
                .is_empty(),
            "关闭数据库前必须确认媒体驱动与清理结束"
        );
        self.fixture.abort();
        let _ = self.fixture.await;
        self.state.storage().close().await.unwrap();
        let _ = std::fs::remove_dir_all(self.home);
    }
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
#[tokio::test]
async fn image_approval_starts_a_distinct_real_run_and_delivers_owned_session_file() {
    let h = Harness::new("image", true).await;
    h.external
        .args
        .lock()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("externalVideoUrl");
    let parent = h.submit().await;
    assert_eq!(
        h.external.generated.load(Ordering::SeqCst),
        0,
        "批准前不提交"
    );
    h.approve(&parent).await;
    let job = h.job().await;
    let execution = job
        .execution_run_id
        .as_ref()
        .expect("媒体任务必须有独立的实际执行Run");
    assert_ne!(execution, &parent);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if h.state
                .storage()
                .load_run(&RunId::new(&parent))
                .await
                .unwrap()
                .unwrap()
                .status
                .is_terminal()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    h.wait_generated().await;
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    h.external.release.add_permits(1);
    let completed = h.terminal_job().await;
    assert_eq!(
        completed.phase,
        lingxi_adapters::storage::media::MediaPhase::Completed,
        "{:?}",
        completed.state
    );
    let file_id = completed.state["files"][0]["fileId"].as_str().unwrap();
    let file = h
        .state
        .session_files()
        .get_file("media-a", file_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(file.mime, "image/png");
    assert_eq!(
        std::fs::read(&file.file_path).unwrap(),
        base64::engine::general_purpose::STANDARD
            .decode(PNG)
            .unwrap()
    );
    assert!(h
        .state
        .session_files()
        .get_file("other-session", file_id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    let encoded = completed.state.to_string();
    assert!(!encoded.contains("synthetic-image-secret"));
    assert!(!encoded.contains("synthetic-chat-secret"));
    h.stop().await;
}
#[tokio::test]
async fn speech_delivers_real_audio_bytes_and_keeps_the_media_job_causal_run() {
    let h = Harness::new("speech", false).await;
    h.external
        .args
        .lock()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("externalVideoUrl");
    let parent = h.submit().await;
    h.approve(&parent).await;
    let job = h.terminal_job().await;
    assert_eq!(
        job.phase,
        lingxi_adapters::storage::media::MediaPhase::Completed,
        "{:?}",
        job.state
    );
    let id = job.state["files"][0]["fileId"].as_str().unwrap();
    let file = h
        .state
        .session_files()
        .get_file("media-a", id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(file.mime, "audio/mpeg");
    assert!(std::fs::read(&file.file_path).unwrap().starts_with(b"ID3"));
    let usage=h.state.storage().query_one_text("SELECT run_id FROM model_call_usage WHERE purpose='speech' ORDER BY rowid DESC LIMIT 1",vec![]).await.unwrap();
    assert_eq!(usage, job.execution_run_id);
    h.stop().await;
}
#[tokio::test]
async fn stopping_a_submitted_job_does_not_claim_remote_cancellation_or_deliver_late_bytes() {
    let h = Harness::new("image", true).await;
    h.external
        .args
        .lock()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("externalVideoUrl");
    let parent = h.submit().await;
    h.approve(&parent).await;
    h.wait_generated().await;
    let job = h.job().await;
    let cancelled = h
        .state
        .media()
        .cancel_owned(
            &job.operation_id,
            "media-a",
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
        )
        .await
        .unwrap();
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["remoteCancel"], "unsupported");
    assert_eq!(cancelled["remoteOutcome"], "unknown");
    h.external.release.add_permits(1);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let job = lingxi_adapters::storage::media::get(h.state.storage(), &job.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        job.phase,
        lingxi_adapters::storage::media::MediaPhase::Cancelled
    );
    assert!(job.state["files"].is_null());
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    h.stop().await;
}
#[tokio::test]
async fn video_uses_real_submit_query_download_and_session_delivery_stages() {
    let h = Harness::new("video", true).await;
    let parent = h.submit().await;
    h.approve(&parent).await;
    h.wait_generated().await;
    let accepted = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = h.job().await;
            if job.phase == lingxi_adapters::storage::media::MediaPhase::Generating {
                break job;
            }
            assert!(!job.phase.terminal(), "{:?}", job.state);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(accepted.state["remoteOutcome"], "accepted");
    assert!(!accepted.state["tracker"].as_str().unwrap().is_empty());
    assert_eq!(h.external.downloaded.load(Ordering::SeqCst), 0);
    h.external.release.add_permits(1);
    let completed = h.terminal_job().await;
    assert_eq!(
        completed.phase,
        lingxi_adapters::storage::media::MediaPhase::Completed,
        "{:?}",
        completed.state
    );
    assert!(completed.revision > accepted.revision);
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    assert!(h.external.queries.load(Ordering::SeqCst) >= 2);
    assert_eq!(h.external.downloaded.load(Ordering::SeqCst), 1);
    let file = h
        .state
        .session_files()
        .get_file(
            "media-a",
            completed.state["files"][0]["fileId"].as_str().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(file.mime, "video/mp4");
    assert_eq!(
        std::fs::read(&file.file_path).unwrap(),
        b"\0\0\0\x18ftypmp42synthetic-video"
    );
    h.stop().await;
}
#[tokio::test]
async fn confirmed_supplier_failure_remains_failed_and_finishes_the_actual_execution_run() {
    let h = Harness::new("video", false).await;
    h.external.fail_generation.store(true, Ordering::SeqCst);
    let parent = h.submit().await;
    h.approve(&parent).await;
    let job = h.terminal_job().await;
    assert_eq!(
        job.phase,
        lingxi_adapters::storage::media::MediaPhase::Failed
    );
    let receipt = h
        .state
        .media()
        .list_owned("media-a", lingxi_kernel::LOCAL_OWNER_SUBJECT)
        .await
        .unwrap()
        .into_iter()
        .find(|receipt| receipt["taskId"] == job.operation_id)
        .unwrap();
    assert_eq!(receipt["remoteOutcome"], "failed");
    assert!(!receipt.to_string().contains("synthetic-video-secret"));
    assert_eq!(receipt["files"], json!([]));
    let execution = job.execution_run_id.as_deref().unwrap();
    h.wait_parent(execution).await;
    let actual = h
        .state
        .storage()
        .load_run(&RunId::new(execution))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(format!("{:?}", actual.status), "Failed");
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    assert_eq!(h.external.downloaded.load(Ordering::SeqCst), 0);
    h.stop().await;
}
#[tokio::test]
async fn restart_reopens_real_storage_and_queries_known_remote_without_resubmitting() {
    let mut h = Harness::new("video", true).await;
    let parent = h.submit().await;
    h.approve(&parent).await;
    h.wait_parent(&parent).await;
    let before = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = h.job().await;
            if job.phase == lingxi_adapters::storage::media::MediaPhase::Generating {
                break job;
            }
            assert!(!job.phase.terminal(), "{:?}", job.state);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let prior_execution = before.execution_run_id.clone().unwrap();
    let tracker = before.state["tracker"].clone();
    h.restart().await;
    let resumed = h.job().await;
    assert_eq!(resumed.operation_id, before.operation_id);
    assert_eq!(resumed.source_run_id, parent);
    assert_eq!(resumed.state["tracker"], tracker);
    assert_ne!(
        resumed.execution_run_id.as_deref(),
        Some(prior_execution.as_str())
    );
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    h.external.release.add_permits(1);
    let completed = h.terminal_job().await;
    assert_eq!(
        completed.phase,
        lingxi_adapters::storage::media::MediaPhase::Completed,
        "{:?}",
        completed.state
    );
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    assert_eq!(h.external.downloaded.load(Ordering::SeqCst), 1);
    let file = h
        .state
        .session_files()
        .get_file(
            "media-a",
            completed.state["files"][0]["fileId"].as_str().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read(file.file_path).unwrap(),
        b"\0\0\0\x18ftypmp42synthetic-video"
    );
    h.stop().await;
}
#[tokio::test]
async fn describe_options_uses_actual_config_and_existing_provider_modes_without_submitting() {
    let h = Harness::new("image", false).await;
    let operations = h.state.operations().unwrap();
    let described = operations
        .describe_media(
            "image",
            Some("image".into()),
            Some("1".into()),
            Some("image2image".into()),
        )
        .unwrap();
    assert_eq!(described["providers"][0]["model"]["id"], "gpt-image-1");
    assert_eq!(described["providers"][0]["mode"]["id"], "image2image");
    assert!(
        described["providers"][0]["mode"]["parameterSchema"]["properties"]["ratio"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "3:2")
    );
    assert!(operations
        .describe_media("image", Some("absent".into()), None, None)
        .is_err());
    assert!(operations
        .describe_media(
            "image",
            Some("image".into()),
            Some("invented-model".into()),
            None
        )
        .is_err());
    assert!(operations
        .describe_media(
            "image",
            Some("image".into()),
            Some("dall-e-3".into()),
            Some("image2image".into())
        )
        .is_err());
    let resolved = operations
        .media_route("image", Some("image".into()), Some("1".into()))
        .unwrap();
    assert_eq!(resolved.to_value()["model"], "gpt-image-1");
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 0);
    h.stop().await;
}
#[tokio::test]
async fn disabling_the_plugin_after_parent_completion_cancels_real_background_and_blocks_delivery()
{
    let h = Harness::new("image", true).await;
    let parent = h.submit().await;
    h.approve(&parent).await;
    h.wait_generated().await;
    let active = h.job().await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if h.state
                .storage()
                .load_run(&RunId::new(&parent))
                .await
                .unwrap()
                .unwrap()
                .status
                .is_terminal()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    std::fs::write(
        h.home.join("agents/a/config.yaml"),
        "agent:\n  name: a\nmemory:\n  enabled: false\ntools:\n  disabled: [media]\n",
    )
    .unwrap();
    let stopped = h.terminal_job().await;
    assert_eq!(
        stopped.phase,
        lingxi_adapters::storage::media::MediaPhase::Unknown,
        "{:?}",
        stopped.state
    );
    assert!(stopped.state["files"].is_null());
    let execution = active.execution_run_id.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if h.state
                .storage()
                .load_run(&RunId::new(&execution))
                .await
                .unwrap()
                .unwrap()
                .status
                .is_terminal()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    h.external.release.add_permits(1);
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    h.stop().await;
}
#[tokio::test]
async fn duplicate_user_request_reuses_real_run_and_does_not_create_another_generation() {
    let h = Harness::new("image", true).await;
    let parent = h
        .submit_with_request(Some("same-local-media-request"))
        .await;
    h.approve(&parent).await;
    h.wait_generated().await;
    let same = h
        .submit_with_request(Some("same-local-media-request"))
        .await;
    assert_eq!(same, parent);
    let count = h
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM media_jobs_v1 WHERE session_id='media-a'",
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(count.as_deref(), Some("1"));
    h.external.release.add_permits(1);
    assert_eq!(
        h.terminal_job().await.phase,
        lingxi_adapters::storage::media::MediaPhase::Completed
    );
    assert_eq!(h.external.generated.load(Ordering::SeqCst), 1);
    h.stop().await;
}
#[cfg(unix)]
async fn configure_synthetic_cli(h: &Harness) {
    use std::os::unix::fs::PermissionsExt;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|entry| entry.join("node"))
        .find(|entry| entry.is_file())
        .expect("本地测试需要已安装的Node，不会安装工具")
        .canonicalize()
        .unwrap();
    let output = h.home.join("media-parser-bundle");
    let code = format!(
        "import{{buildMediaWorker}}from {};buildMediaWorker({{rootDir:{},bundleOutDir:{}}});",
        serde_json::to_string(
            &root
                .join("scripts/build-media-worker.mjs")
                .to_string_lossy()
        )
        .unwrap(),
        serde_json::to_string(&root.to_string_lossy()).unwrap(),
        serde_json::to_string(&output.to_string_lossy()).unwrap()
    );
    let built = std::process::Command::new(&node)
        .args(["--input-type=module", "-e", &code])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let source =
        std::fs::read_to_string(root.join("tests/jimeng-cli-capabilities.test.ts")).unwrap();
    let mut helps = serde_json::Map::new();
    for (mode, name) in [
        ("text2image", "TEXT_TO_IMAGE_HELP"),
        ("image2image", "IMAGE_TO_IMAGE_HELP"),
        ("text2video", "TEXT_TO_VIDEO_HELP"),
        ("image2video", "IMAGE_TO_VIDEO_HELP"),
    ] {
        let marker = format!("const {name} = `");
        let help = source
            .split(&marker)
            .nth(1)
            .unwrap()
            .split('`')
            .next()
            .unwrap();
        helps.insert(mode.into(), json!(help));
    }
    let executable = h.home.join("synthetic-dreamina");
    let script = format!(
        r##"#!/usr/bin/python3 -I
import sys,json,os,base64
from pathlib import Path
root=Path(__file__).resolve().parent
args=sys.argv[1:]
with (root/'cli-calls.jsonl').open('a') as out:
    out.write(json.dumps({{'args':args,'home':os.environ.get('HOME'),'tokenInherited':'SYNTHETIC_MEDIA_FORBIDDEN_SECRET' in os.environ}})+'\n')
helps=json.loads({helps:?})
if args==['--version']:
    print(json.dumps({{'version':'synthetic-1.0.0','commit':'local-fixture'}}))
elif len(args)==2 and args[1]=='--help':
    print(helps[args[0]])
elif args==['user_credit']:
    print(json.dumps({{'synthetic_credit':1000}}))
elif args and args[0] in ['text2image','image2image','text2video','image2video']:
    assert args[args.index('--poll')+1]=='0'
    print(json.dumps({{'submit_id':'synthetic-cli-video' if 'video' in args[0] else 'synthetic-cli-image','gen_status':'querying'}}))
elif args and args[0]=='query_result':
    target=Path(args[args.index('--download_dir')+1])
    remote=args[args.index('--submit_id')+1]
    if remote=='synthetic-cli-video':
        (target/'actual.mp4').write_bytes(bytes([0,0,0,24])+b'ftypmp42synthetic-cli-video')
    else:
        (target/'actual.png').write_bytes(base64.b64decode({png:?}))
    print(json.dumps({{'submit_id':remote,'gen_status':'success'}}))
else:
    sys.exit(9)
"##,
        helps = serde_json::to_string(&helps).unwrap(),
        png = PNG
    );
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let private = h.home.join("cli-private");
    let runtime = lingxi_service::media_jimeng::JimengRuntime::new(
        lingxi_service::media_jimeng::JimengLaunch {
            executable,
            helper_argv: vec![
                node.to_string_lossy().into_owned(),
                output
                    .join("media-jimeng-worker.mjs")
                    .to_string_lossy()
                    .into_owned(),
            ],
            private_root: private.clone(),
            external_login_home: private.join("authorized-login"),
            credential_grant_generation: 1,
        },
        Arc::new(SystemClock),
    )
    .unwrap();
    h.state.media().set_jimeng(runtime).unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn jimeng_image_and_video_use_real_supervised_cli_and_current_sdk_with_owned_delivery() {
    for kind in ["image", "video"] {
        let h = Harness::new(&format!("jimeng-{kind}"), false).await;
        configure_synthetic_cli(&h).await;
        let parent = h.submit().await;
        h.approve(&parent).await;
        let job = h.terminal_job().await;
        assert_eq!(
            job.phase,
            lingxi_adapters::storage::media::MediaPhase::Completed,
            "{:?}",
            job.state
        );
        assert_eq!(job.state["tracker"], format!("synthetic-cli-{kind}"));
        assert_eq!(job.state["opaqueRemoteDownload"], true);
        let file = h
            .state
            .session_files()
            .get_file("media-a", job.state["files"][0]["fileId"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            file.mime,
            if kind == "image" {
                "image/png"
            } else {
                "video/mp4"
            }
        );
        assert!(std::fs::metadata(&file.file_path).unwrap().len() > 0);
        let records = std::fs::read_to_string(h.home.join("cli-calls.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .filter(
                    |record| record["args"][0].as_str().is_some_and(|command| matches!(
                        command,
                        "text2image" | "image2image" | "text2video" | "image2video"
                    )) && record["args"][1] != "--help"
                )
                .count(),
            1
        );
        assert!(records
            .iter()
            .any(|record| record["args"][0] == "--version"));
        assert_eq!(
            records
                .iter()
                .filter(|record| record["args"][1] == "--help")
                .count(),
            4
        );
        for record in records {
            assert_eq!(record["tokenInherited"], false);
            assert!(record["home"]
                .as_str()
                .unwrap()
                .starts_with(h.home.to_string_lossy().as_ref()));
        }
        assert_eq!(
            h.external.generated.load(Ordering::SeqCst),
            0,
            "CLI不会借HTTP供应商通道或真实账号"
        );
        h.stop().await;
    }
}
#[test]
fn untrusted_media_arguments_cannot_choose_credentials_or_paths_or_drop_wrong_typed_overrides() {
    use lingxi_service::media::MediaSpec;
    for value in [
        json!({"prompt":"x","token":"secret"}),
        json!({"prompt":"x","endpoint":"http://evil.invalid"}),
        json!({"prompt":"x","image":{"kind":"local-file","path":"/tmp/private"}}),
        json!({"prompt":"x","options":{"watermark":"false"}}),
        json!({"prompt":"x","ratio":"1:1","options":{"ratio":false}}),
        json!({"prompt":"x","model":"guessed"}),
        json!({"prompt":"x","mode":"image2image"}),
    ] {
        assert!(MediaSpec::parse("image", &value).is_err(), "{value}");
    }
    assert!(MediaSpec::parse(
        "image",
        &json!({"prompt":"x","count":1,"options":{"watermark":false}})
    )
    .is_ok());
    let video = MediaSpec::parse(
        "video",
        &json!({"prompt":"x","options":{"video_resolution":"720p","duration":5}}),
    )
    .unwrap();
    assert_eq!(video.resolution.as_deref(), Some("720p"));
    assert_eq!(video.duration, Some(5.0));
    assert!(video.options.is_empty());
    let image = MediaSpec::parse(
        "image",
        &json!({"prompt":"x","ratio":"1:1","options":{"n":2,"ratio":"3:2"}}),
    )
    .unwrap();
    assert_eq!(image.count, Some(2));
    assert_eq!(image.ratio.as_deref(), Some("1:1"));
}

/// Loop 来源媒体任务：正式持久授权 = 落盘循环记录（活跃状态）。kickoff 经
/// 真实调度链投递（execute_scheduled_for origin=loop），授权闸读同一记录
/// 放行并完成真实生成/交付；执行 run 的 lineage 必须挂在循环根下。
#[tokio::test]
async fn loop_origin_media_job_is_authorized_by_the_persistent_loop_record() {
    let h = Harness::new("image", false).await;
    h.external
        .args
        .lock()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("externalVideoUrl");
    let target = lingxi_service::loopx::LoopTarget::desktop("media-a");
    let controller = h.state.loop_services().controller.clone();
    let record = controller
        .start(&target, "循环盯一下本地合成媒体")
        .await
        .expect("循环启动");
    assert_eq!(record.status, lingxi_service::loopx::LoopStatus::Running);
    // kickoff 轮驱动媒体工具链，批准该循环 run 的外部生成批准。
    let approval = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(p) = h
                .approvals
                .pending_for_principal(&KernelPrincipal::LocalUser, "media-a")
                .into_iter()
                .next()
            {
                break p;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("循环轮的媒体生成须先进入正式批准");
    let loop_run = approval.run_id.clone();
    assert!(h
        .approvals
        .answer_for_principal(
            &KernelPrincipal::LocalUser,
            "media-a",
            &approval.approval_id,
            Answer::Approve
        )
        .settled());
    // 授权通过的证据：真实外部请求到达替身供应商。
    h.wait_generated().await;
    let completed = h.terminal_job().await;
    assert_eq!(
        completed.phase,
        lingxi_adapters::storage::media::MediaPhase::Completed,
        "{:?}",
        completed.state
    );
    // 媒体来源 run 即循环 run，且其 lineage 是 origin=loop 的循环根。
    assert_eq!(completed.source_run_id, loop_run);
    let lineage = h
        .state
        .storage()
        .load_run_lineage(&RunId::new(&loop_run))
        .await
        .unwrap()
        .expect("循环 run 必须有 lineage");
    assert_eq!(lineage.origin, lingxi_kernel::subagent::RunOrigin::Loop);
    assert!(lineage.parent_run_id.is_none());
    assert_eq!(lineage.cause_id.as_deref(), Some("media-a"));
    // 交付真实发生：会话文件登记且字节与供应商一致。
    let file_id = completed.state["files"][0]["fileId"].as_str().unwrap();
    let file = h
        .state
        .session_files()
        .get_file("media-a", file_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(file.mime, "image/png");
    // 落盘循环记录仍存在且活跃（授权锚点本体）。
    let stored: Value = serde_json::from_str(
        &std::fs::read_to_string(h.home.join(".ephemeral").join("loop-state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored["loops"][0]["key"], "media-a");
    assert_eq!(stored["loops"][0]["status"], "running");
    controller.stop(&target).await.expect("停循环");
    h.stop().await;
}

/// 撤权即停：循环停止后落盘记录写终态，媒体后台持续重查的下一拍拒绝，
/// 已提交远端的结果不得交付为会话文件，也不谎称远端已撤销。
#[tokio::test]
async fn stopping_the_loop_revokes_media_authority_and_late_bytes_are_not_delivered() {
    let h = Harness::new("image", true).await;
    h.external
        .args
        .lock()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("externalVideoUrl");
    let target = lingxi_service::loopx::LoopTarget::desktop("media-a");
    let controller = h.state.loop_services().controller.clone();
    controller
        .start(&target, "循环盯一下本地合成媒体")
        .await
        .expect("循环启动");
    let approval = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(p) = h
                .approvals
                .pending_for_principal(&KernelPrincipal::LocalUser, "media-a")
                .into_iter()
                .next()
            {
                break p;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("循环轮的媒体生成须先进入正式批准");
    assert!(h
        .approvals
        .answer_for_principal(
            &KernelPrincipal::LocalUser,
            "media-a",
            &approval.approval_id,
            Answer::Approve
        )
        .settled());
    // 远端请求已提交（闸门扣住响应），随后停止循环 = 撤权。
    h.wait_generated().await;
    let job = h.job().await;
    controller.stop(&target).await.expect("停止循环即撤权");
    let stored: Value = serde_json::from_str(
        &std::fs::read_to_string(h.home.join(".ephemeral").join("loop-state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored["loops"][0]["status"], "stopped");
    // 放行远端迟到响应：授权重查必须拒绝，产物不得交付。
    h.external.release.add_permits(1);
    let settled = h.terminal_job().await;
    assert_ne!(
        settled.phase,
        lingxi_adapters::storage::media::MediaPhase::Completed,
        "撤权后迟到字节不得交付: {:?}",
        settled.state
    );
    assert!(
        settled.state["files"].is_null()
            || settled.state["files"]
                .as_array()
                .is_some_and(|f| f.is_empty()),
        "撤权后不得登记会话文件: {:?}",
        settled.state
    );
    assert_eq!(settled.operation_id, job.operation_id);
    assert_eq!(
        h.external.generated.load(Ordering::SeqCst),
        1,
        "不得重复提交"
    );
    h.stop().await;
}
