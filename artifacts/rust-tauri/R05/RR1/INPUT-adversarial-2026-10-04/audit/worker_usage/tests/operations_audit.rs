//! R05-T06 service-level acceptance: the REAL operation entry
//! (`OperationService`) over loopback stubs — C02 cross-provider media,
//! C03 authorized resource reads & product authenticity, C12 honest
//! media lifecycles.
//!
//! The far end of the wire is a loopback raw-TCP stub (test equipment);
//! the chain under test is ALL production code: ConfigModelGateway →
//! CredentialService → OperationService (quota admission → dialect →
//! dispatcher → egress-guarded product settlement → disk registration).
//! Offline; NOT_REAL_API.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_adapters::models::operations::image::ImageRequest;
use lingxi_adapters::models::operations::speech::SpeechRequest;
use lingxi_adapters::models::operations::video::VideoRequest;
use lingxi_kernel::{Principal, RunContext};
use lingxi_service::operations::{
    ImageGenerationOutcome, MediaTaskStatus, OperationService, RegisteredProduct,
};
use lingxi_service::quotas::{LayeredQuotaLimits, QuotaLimits, QuotaManager};
use sha2::Digest as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── the multi-path loopback stub ────────────────────────────────────────────

/// One scripted answer: the first matching path PREFIX (in order) serves
/// `status` / `content_type` / `body` after `delay_ms`.
struct ScriptEntry {
    prefix: String,
    status: u16,
    content_type: String,
    body: Vec<u8>,
    delay_ms: u64,
}

struct Stub {
    endpoint: String,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<Recorded>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct Recorded {
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

impl Stub {
    /// Binds, then builds the script WITH the bound endpoint (script
    /// bodies may point back at this stub's own origin).
    async fn start(build: impl FnOnce(&str) -> Vec<ScriptEntry>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let endpoint = format!("http://{addr}");
        let script = build(&endpoint);
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<Recorded>>> = Arc::new(Mutex::new(Vec::new()));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let script = Arc::new(script);
        let task_hits = Arc::clone(&hits);
        let task_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                let script = Arc::clone(&script);
                let hits = Arc::clone(&task_hits);
                let seen = Arc::clone(&task_requests);
                tokio::spawn(async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let recorded = read_request(&mut socket).await;
                    seen.lock().expect("requests").push(recorded.clone());
                    for entry in script.iter() {
                        if recorded.path.starts_with(&entry.prefix) {
                            if entry.delay_ms > 0 {
                                tokio::time::sleep(Duration::from_millis(entry.delay_ms)).await;
                            }
                            let reason = if (200..300).contains(&entry.status) {
                                "OK"
                            } else {
                                "Error"
                            };
                            let mut response = format!(
                                "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                                entry.status,
                                reason,
                                entry.content_type,
                                entry.body.len()
                            );
                            response.push_str("\r\n");
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.write_all(&entry.body).await;
                            let _ = socket.shutdown().await;
                            return;
                        }
                    }
                    panic!("stub: unscripted path {}", recorded.path);
                });
            }
        });
        Self {
            endpoint,
            hits,
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    async fn await_hits(&self, target: usize) {
        for _ in 0..200 {
            if self.hits() >= target {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("stub never reached {target} hits (now {})", self.hits());
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("requests").clone()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Recorded {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub read stalled")
            .expect("stub read");
        if read == 0 {
            panic!("stub: connection closed before headers completed");
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        assert!(raw.len() < 256 * 1024, "stub: header block too large");
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("utf8 headers");
    let mut lines = head.split("\r\n");
    let path = lines
        .next()
        .expect("request line")
        .split_whitespace()
        .nth(1)
        .expect("path")
        .to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().expect("numeric content-length");
            }
            headers.push((name, value));
        }
    }
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 64 * 1024];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub body read stalled")
            .expect("stub body read");
        if read == 0 {
            panic!("stub: connection closed mid-body");
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    let body = raw[body_start..body_start + content_length].to_vec();
    Recorded {
        path,
        headers,
        body,
    }
}

// ── the shared production assembly ─────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t06ops-{tag}-{}-{}",
        std::process::id(),
        DIR_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn entry(prefix: &str, status: u16, content_type: &str, body: String) -> ScriptEntry {
    ScriptEntry {
        prefix: prefix.to_string(),
        status,
        content_type: content_type.to_string(),
        body: body.into_bytes(),
        delay_ms: 0,
    }
}

fn entry_bytes(prefix: &str, content_type: &str, body: Vec<u8>) -> ScriptEntry {
    ScriptEntry {
        prefix: prefix.to_string(),
        status: 200,
        content_type: content_type.to_string(),
        body,
        delay_ms: 0,
    }
}

fn entry_delayed(prefix: &str, content_type: &str, body: String, delay_ms: u64) -> ScriptEntry {
    ScriptEntry {
        prefix: prefix.to_string(),
        status: 200,
        content_type: content_type.to_string(),
        body: body.into_bytes(),
        delay_ms,
    }
}

fn quota_limits(global: usize) -> QuotaLimits {
    QuotaLimits {
        model: LayeredQuotaLimits {
            global,
            per_agent: 4,
            per_session: 4,
        },
        ..QuotaLimits::default()
    }
}

/// Builds the production operation chain over separate stub providers
/// (text / image+speech+video on the media stub) with DISTINCT synthetic
/// keys per provider.
struct Chain {
    text_stub: Stub,
    media_stub: Stub,
    operations: Arc<OperationService>,
    products_root: PathBuf,
}

async fn chain(
    text_script: impl FnOnce(&str) -> Vec<ScriptEntry>,
    media_script: impl FnOnce(&str) -> Vec<ScriptEntry>,
) -> Chain {
    let text_stub = Stub::start(text_script).await;
    let media_stub = Stub::start(media_script).await;
    let runtime_dir = unique_dir("chain-rt");
    let products_root = unique_dir("chain-products");
    let text_v1 = format!("{}/v1", text_stub.endpoint);
    let media_root = media_stub.endpoint.clone();
    let media_v1 = format!("{}/v1", media_stub.endpoint);
    let plane_json = format!(
        r#"{{
            "providers": {{
                "text_a": {{
                    "protocol": "openai-completions",
                    "endpoint": "{text_v1}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-TEXT-A-SECRET"}}
                }},
                "image_b": {{
                    "protocol": "openai-images",
                    "endpoint": "{media_root}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-IMAGE-B-SECRET"}}
                }},
                "speech_c": {{
                    "protocol": "openai-audio-speech",
                    "endpoint": "{media_root}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-SPEECH-C-SECRET"}}
                }},
                "video_d": {{
                    "protocol": "agnes-videos",
                    "endpoint": "{media_v1}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-VIDEO-D-SECRET"}}
                }}
            }},
            "models": {{
                "chat": {{"provider": "text_a", "model": "text-model"}},
                "summarize": {{"provider": "text_a", "model": "sum-model"}},
                "image": {{"provider": "image_b", "model": "dall-e-3"}},
                "speech": {{"provider": "speech_c", "model": "gpt-4o-mini-tts"}},
                "video": {{"provider": "video_d", "model": "agnes-video"}}
            }}
        }}"#
    );
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&plane_json)
        .expect("plane parses");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credential service"),
    );
    let gateway =
        Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane));
    let endpoints = vec![
        format!("{}/v1", text_stub.endpoint),
        media_stub.endpoint.clone(),
        format!("{}/v1", media_stub.endpoint),
    ];
    let operations = Arc::new(
        OperationService::new(
            gateway,
            credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
            Arc::new(QuotaManager::new(quota_limits(4))),
            endpoints,
            products_root.clone(),
        )
        .expect("operation service"),
    );
    Chain {
        text_stub,
        media_stub,
        operations,
        products_root,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A registered product is a REAL file holding exactly the product bytes,
/// with the digest of what was written (C03: no ghost registrations).
fn product_is_real(product: &RegisteredProduct, bytes: &[u8], mime: &str) {
    assert_eq!(product.mime, mime);
    assert_eq!(product.size_bytes, bytes.len() as u64);
    assert_eq!(product.sha256, sha256_hex(bytes));
    let on_disk = std::fs::read(&product.path).expect("the registered product file exists");
    assert_eq!(on_disk, bytes, "the file holds exactly the product bytes");
}

fn ctx() -> RunContext {
    RunContext {
        principal: Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess-t06ops"),
        run_id: lingxi_protocol::RunId::new("run-t06ops"),
        attempt: lingxi_protocol::AttemptId::new("a-1"),
        generation: 1,
    }
}

fn product_files(root: &std::path::Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .expect("products root")
        .map(|e| e.expect("entry").path())
        .collect()
}

// ── C02: image B + speech C in one task — no provider crossing ──────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c02_media_providers_in_one_task_never_cross() {
    use base64::Engine as _;
    let image_bytes = vec![11u8, 22, 33, 44];
    let b64 = base64::engine::general_purpose::STANDARD.encode(&image_bytes);
    let speech_bytes = b"fake-mp3-bytes".to_vec();
    let c = chain(
        |_| {
            vec![entry(
                "/v1/chat/completions",
                200,
                "text/event-stream",
                "data: [DONE]\n\n".to_string(),
            )]
        },
        |_| {
            vec![
                entry(
                    "/images/generations",
                    200,
                    "application/json",
                    format!("{{\"data\":[{{\"b64_json\":\"{b64}\"}}]}}"),
                ),
                entry_bytes("/audio/speech", "audio/mpeg", speech_bytes.clone()),
            ]
        },
    )
    .await;

    let image = c
        .operations
        .generate_image(
            ImageRequest {
                prompt: "an isolated cube".to_string(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("image settles");
    let speech = c
        .operations
        .synthesize_speech(
            SpeechRequest {
                text: "spoken line".to_string(),
                voice: None,
                speed: None,
                format: None,
            },
            None,
        )
        .await
        .expect("speech settles");

    match image {
        ImageGenerationOutcome::Done { products } => {
            assert_eq!(products.len(), 1);
            product_is_real(&products[0], &image_bytes, "image/png");
        }
        other => panic!("the openai image family settles synchronously: {other:?}"),
    }
    product_is_real(&speech, &speech_bytes, "audio/mpeg");

    // The wire ledger: each media request carried ONLY its own provider's
    // key — no cross-bleed in either direction, and the text provider's
    // material never appears on the media wire.
    let requests = c.media_stub.requests();
    let image_req = requests
        .iter()
        .find(|r| r.path.starts_with("/images/generations"))
        .expect("the image request");
    assert_eq!(
        image_req.header("authorization"),
        Some("Bearer sk-IMAGE-B-SECRET")
    );
    let speech_req = requests
        .iter()
        .find(|r| r.path.starts_with("/audio/speech"))
        .expect("the speech request");
    assert_eq!(
        speech_req.header("authorization"),
        Some("Bearer sk-SPEECH-C-SECRET")
    );
    for request in &requests {
        let raw = format!("{:?} {:?}", request.headers, request.body);
        assert!(
            !raw.contains("sk-TEXT-A-SECRET"),
            "the text provider's material leaked to the media wire"
        );
        if request.path.starts_with("/images/generations") {
            assert!(!raw.contains("sk-SPEECH-C-SECRET"));
        }
        if request.path.starts_with("/audio/speech") {
            assert!(!raw.contains("sk-IMAGE-B-SECRET"));
        }
    }
    // The media legs never touched provider A's endpoint.
    assert_eq!(c.text_stub.hits(), 0);
    c.text_stub.stop().await;
    c.media_stub.stop().await;
}

// ── C03: authorized resource reads & product authenticity ──────────────────

#[test]
fn c03_reference_and_audio_reads_are_authorized_and_bounded() {
    let root = unique_dir("c03");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let context = ctx();

    // Inside the workspace: bytes + the extension-derived MIME.
    let inside = ws.join("ref.png");
    std::fs::write(&inside, [1u8, 2, 3]).expect("ref");
    match lingxi_service::operations::read_image_reference(&access, &context, &inside, &ws)
        .expect("the authorized read")
    {
        lingxi_adapters::models::operations::ImageReference::Bytes {
            bytes,
            mime,
            filename,
        } => {
            assert_eq!(bytes, vec![1, 2, 3]);
            assert_eq!(mime, "image/png");
            assert_eq!(filename, "ref.png");
        }
        other => panic!("a host-read reference is Bytes: {other:?}"),
    }

    // Outside the workspace: refused.
    let outside = root.join("outside.wav");
    std::fs::write(&outside, [9u8]).expect("outside");
    let refusal =
        lingxi_service::operations::read_image_reference(&access, &context, &outside, &ws)
            .expect_err("the unauthorized read refuses");
    assert_eq!(
        refusal.code,
        lingxi_protocol::ErrorCode::Forbidden,
        "{refusal:?}"
    );

    // Oversized reference: refused loudly, never truncated.
    let big = ws.join("big.png");
    std::fs::write(
        &big,
        vec![0u8; lingxi_service::operations::IMAGE_INPUT_MAX_BYTES + 1],
    )
    .expect("big");
    let refusal = lingxi_service::operations::read_image_reference(&access, &context, &big, &ws)
        .expect_err("the oversized read refuses");
    assert!(
        refusal.message.contains("refused, never truncated"),
        "{}",
        refusal.message
    );

    // The audio reader: the MIME table + the audio cap.
    let audio = ws.join("note.m4a");
    std::fs::write(&audio, [4u8, 5]).expect("audio");
    let host_audio = lingxi_service::operations::read_host_audio(&access, &context, &audio, &ws)
        .expect("audio read");
    assert_eq!(host_audio.mime, "audio/mp4");
    assert_eq!(host_audio.filename, "note.m4a");
    let big_audio = ws.join("big.wav");
    std::fs::write(
        &big_audio,
        vec![0u8; lingxi_service::operations::AUDIO_INPUT_MAX_BYTES + 1],
    )
    .expect("big audio");
    let refusal = lingxi_service::operations::read_host_audio(&access, &context, &big_audio, &ws)
        .expect_err("the oversized audio refuses");
    assert!(refusal.message.contains("refused, never truncated"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c03_url_products_download_guarded_and_ghost_products_refuse() {
    // A second loopback origin NOT in the configured endpoint set: the
    // guarded policy must refuse it (the SSRF leg at service level) even
    // though it is reachable.
    let other = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("other bind");
    let other_addr: SocketAddr = other.local_addr().expect("other addr");
    let thief = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        if let Ok((mut socket, _)) = other.accept().await {
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let body = "STOLEN";
            let _ = socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: \
                         {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
        }
    });

    let product_bytes = vec![77u8, 88, 99];
    let c = chain(
        |_| vec![],
        |_| {
            vec![
                entry_bytes("/files/prod.jpeg", "image/jpeg", product_bytes.clone()),
                // An empty b64 answer: the parse yields EMPTY product
                // bytes — the registration refuses the ghost file.
                entry(
                    "/images/generations",
                    200,
                    "application/json",
                    "{\"data\":[{\"b64_json\":\"\"}]}".to_string(),
                ),
            ]
        },
    )
    .await;

    // Leg 1 — the same-origin URL product downloads WITHOUT any
    // credential (the guard API is the download path the operation plane
    // uses; the c12 test below proves the full service flow).
    let guard = lingxi_adapters::models::egress::EgressGuard::from_provider_endpoints(&[
        c.media_stub.endpoint.clone(),
        format!("{}/v1", c.media_stub.endpoint),
    ])
    .expect("guard");
    let (downloaded, mime) = guard
        .download(&format!("{}/files/prod.jpeg", c.media_stub.endpoint), None)
        .await
        .expect("the same-origin download");
    assert_eq!(downloaded, product_bytes);
    assert_eq!(mime, "image/jpeg");
    let files_requests: Vec<_> = c
        .media_stub
        .requests()
        .into_iter()
        .filter(|r| r.path.starts_with("/files/"))
        .collect();
    assert_eq!(files_requests.len(), 1);
    assert!(
        files_requests[0].header("authorization").is_none(),
        "an egress download never carries credentials"
    );

    // Leg 2 — the cross-origin loopback origin refuses.
    let refusal = guard
        .download(&format!("http://{other_addr}/evil.jpeg"), None)
        .await
        .expect_err("the cross-origin loopback refuses");
    assert!(
        refusal.0.message.contains("guarded") || refusal.0.message.contains("plain http"),
        "{}",
        refusal.0.message
    );
    assert!(!thief.is_finished(), "the thief origin was never contacted");

    // Leg 3 — a ghost product refuses registration (no file appears).
    let refusal = c
        .operations
        .generate_image(
            ImageRequest {
                prompt: "ghost".to_string(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect_err("the empty product refuses");
    assert!(refusal.message.contains("ghost"), "{}", refusal.message);
    assert!(product_files(&c.products_root).is_empty());
    c.text_stub.stop().await;
    c.media_stub.stop().await;
}

// ── C12: honest media lifecycles ────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c12_video_job_acceptance_is_not_completion_and_cancel_is_local_only() {
    // agnes submit → a job id; a PENDING query stays Generating; a local
    // cancel states remote_cancel: "unsupported" (never a remote revoke
    // claim); the query after the cancel reports the local state.
    let c = chain(
        |_| vec![],
        |_| {
            vec![
                entry(
                    "/v1/videos",
                    200,
                    "application/json",
                    "{\"task_id\":\"track-1\",\"video_id\":\"vid-1\"}".to_string(),
                ),
                entry(
                    "/agnesapi",
                    200,
                    "application/json",
                    "{\"status\":\"processing\"}".to_string(),
                ),
            ]
        },
    )
    .await;
    let accepted = c
        .operations
        .submit_video(
            VideoRequest {
                prompt: "a lantern".to_string(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    assert_eq!(accepted.task_id, "track-1");
    assert_eq!(accepted.provider_task_id, "vid-1");
    assert_eq!(accepted.remote_cancel, "unsupported");

    // The job id is NOT a product: the pending query reports Generating.
    let status = c
        .operations
        .query_video(&accepted.task_id, None)
        .await
        .expect("query");
    assert!(matches!(status, MediaTaskStatus::Generating), "{status:?}");

    // The local cancel never claims a remote revoke.
    let cancelled = c
        .operations
        .cancel_media_task(&accepted.task_id)
        .expect("cancel");
    assert!(
        matches!(
            cancelled,
            MediaTaskStatus::CancelledLocally {
                remote_cancel: "unsupported"
            }
        ),
        "{cancelled:?}"
    );
    let status = c
        .operations
        .query_video(&accepted.task_id, None)
        .await
        .expect("query after cancel");
    assert!(
        matches!(
            status,
            MediaTaskStatus::CancelledLocally {
                remote_cancel: "unsupported"
            }
        ),
        "{status:?}"
    );
    // Exactly the submit + the one poll hit the wire (the cancel is local).
    assert_eq!(c.media_stub.hits(), 2);
    // An unknown task id is refused, never fabricated.
    let refusal = c
        .operations
        .query_video("fabricated-task", None)
        .await
        .expect_err("a fabricated id refuses");
    assert!(
        refusal.message.contains("not tracked"),
        "{}",
        refusal.message
    );
    c.text_stub.stop().await;
    c.media_stub.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c12_dashscope_pending_then_completed_via_registered_product() {
    // The async image task: submit → Pending; the completed query (with a
    // same-origin URL) → Completed with the REGISTERED product (download
    // + digest + disk) and a credential-free download request. The stub's
    // completed answer names the stub's OWN bound origin (the builder
    // closure receives it).
    let product_bytes = vec![1u8, 5, 9];
    let stub = Stub::start(|endpoint| {
        vec![
            entry(
                "/api/v1/services/aigc/image-generation/generation",
                200,
                "application/json",
                "{\"output\":{\"task_id\":\"tsk-c12\",\"task_status\":\"PENDING\"}}".to_string(),
            ),
            entry(
                "/api/v1/tasks/tsk-c12",
                200,
                "application/json",
                format!(
                    "{{\"output\":{{\"task_status\":\"SUCCEEDED\",\"results\":[{{\"url\":\"{endpoint}/files/img.png\"}}]}}}}"
                ),
            ),
            entry_bytes("/files/img.png", "image/png", product_bytes.clone()),
        ]
    })
    .await;
    let runtime_dir = unique_dir("c12b-rt");
    let products_root = unique_dir("c12b-products");
    let plane_json = format!(
        r#"{{
            "providers": {{
                "dash": {{
                    "protocol": "dashscope-images",
                    "endpoint": "{}/compatible-mode/v1",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-DASH-SECRET"}}
                }}
            }},
            "models": {{
                "image": {{"provider": "dash", "model": "wan2.6-t2i"}}
            }}
        }}"#,
        stub.endpoint
    );
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&plane_json)
        .expect("plane");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credentials"),
    );
    let gateway =
        Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane));
    let operations = OperationService::new(
        gateway,
        credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
        Arc::new(QuotaManager::new(quota_limits(2))),
        vec![
            format!("{}/compatible-mode/v1", stub.endpoint),
            format!("{}/api/v1", stub.endpoint),
        ],
        products_root.clone(),
    )
    .expect("operations");

    let outcome = operations
        .generate_image(
            ImageRequest {
                prompt: "a pending lake".to_string(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    let task_id = match outcome {
        ImageGenerationOutcome::Pending { task_id } => task_id,
        other => panic!("wan is async: {other:?}"),
    };
    assert_eq!(task_id, "tsk-c12");

    // The completed query registers the product (download + digest +
    // disk); the URL download carried no credential.
    let status = operations
        .query_media_task(&task_id, None)
        .await
        .expect("query");
    match status {
        MediaTaskStatus::Completed { products } => {
            assert_eq!(products.len(), 1);
            product_is_real(&products[0], &product_bytes, "image/png");
        }
        other => panic!("the completed query registers the product: {other:?}"),
    }
    let files_requests: Vec<_> = stub
        .requests()
        .into_iter()
        .filter(|r| r.path.starts_with("/files/"))
        .collect();
    assert_eq!(files_requests.len(), 1);
    assert!(
        files_requests[0].header("authorization").is_none(),
        "the product download never carries credentials"
    );
    // The products root holds exactly the one registered file.
    assert_eq!(product_files(&products_root).len(), 1);
    stub.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c12_a_dropped_inflight_call_registers_no_product() {
    // An operation call DROPPED mid-flight (the caller cancelled): one
    // request hit the wire, no product registered, nothing continues.
    let c = chain(
        |_| vec![],
        |_| {
            vec![entry_delayed(
                "/audio/speech",
                "audio/mpeg",
                "late-audio-bytes".to_string(),
                300,
            )]
        },
    )
    .await;
    let operations = Arc::clone(&c.operations);
    let request = SpeechRequest {
        text: "will be dropped".to_string(),
        voice: None,
        speed: None,
        format: None,
    };
    let call = tokio::spawn(async move { operations.synthesize_speech(request, None).await });
    // Drive it until the request is on the wire, then CANCEL it (a real
    // abort of the in-flight call).
    c.media_stub.await_hits(1).await;
    call.abort();
    // The stub's delayed answer lands on a closed connection: no product.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(c.media_stub.hits(), 1);
    assert!(product_files(&c.products_root).is_empty());
    c.text_stub.stop().await;
    c.media_stub.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c12_empty_speech_text_refuses_before_any_request() {
    // The dialect's empty-text guard fires before any wire work (the
    // system-speech platform lane shares the same rule).
    let c = chain(|_| vec![], |_| vec![]).await;
    let failure = c
        .operations
        .synthesize_speech(
            SpeechRequest {
                text: "   ".to_string(),
                voice: None,
                speed: None,
                format: None,
            },
            None,
        )
        .await
        .expect_err("an empty synthesis refuses");
    assert_eq!(failure.message, "prompt is required");
    assert_eq!(c.media_stub.hits(), 0);
    c.text_stub.stop().await;
    c.media_stub.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_video_poll_must_use_provider_job_id_and_preserve_legacy_tracker() {
    let c = chain(|_|vec![], |_|vec![
        entry("/v1/videos", 200, "application/json", "{\"task_id\":\"local-track\",\"video_id\":\"provider-vid\"}".into()),
        entry("/agnesapi", 200, "application/json", "{\"status\":\"processing\"}".into()),
    ]).await;
    let accepted = c.operations.submit_video(VideoRequest { prompt:"audit".into(), ..Default::default() },None).await.unwrap();
    let result = c.operations.query_video(&accepted.task_id,None).await;
    let paths: Vec<_> = c.media_stub.requests().iter().map(|r| r.path.clone()).collect();
    println!("AUDIT video poll: receipt={accepted:?}, paths={paths:?}, result={result:?}");
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    assert!(paths.iter().any(|p|p.contains("video_id=provider-vid")), "must send provider-facing video_id; local tracker is only legacy fallback");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_cancelled_media_poll_must_not_download_or_publish_completed() {
    let c = chain(|_|vec![], |endpoint|vec![
        entry("/v1/videos",200,"application/json","{\"task_id\":\"cancel-track\",\"video_id\":\"cancel-track\"}".into()),
        entry_delayed("/agnesapi","application/json",format!("{{\"status\":\"completed\",\"url\":\"{endpoint}/audit-product.mp4\"}}"),200),
        entry_bytes("/audit-product.mp4","video/mp4",vec![1,2,3,4]),
    ]).await;
    let accepted = c.operations.submit_video(VideoRequest { prompt:"audit".into(), ..Default::default() },None).await.unwrap();
    let operations = c.operations.clone();
    let id=accepted.task_id.clone();
    let pending=tokio::spawn(async move {operations.query_video(&id,None).await});
    tokio::time::timeout(Duration::from_secs(3),async {while c.media_stub.requests().iter().all(|r|!r.path.starts_with("/agnesapi")) {tokio::task::yield_now().await;}}).await.unwrap();
    let cancel=c.operations.cancel_media_task(&accepted.task_id).unwrap();
    let result=pending.await.unwrap();
    let paths: Vec<_> = c.media_stub.requests().iter().map(|r|r.path.clone()).collect();
    let product_count=product_files(&c.products_root).len();
    println!("AUDIT cancellation race: cancel_ack={cancel:?}; late_result={result:?}; requests={paths:?}; registered_products={product_count}");
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    assert!(!matches!(result,Ok(MediaTaskStatus::Completed{..})),"acknowledged local cancellation must fence an in-flight poll, not let it overwrite cancelled with Completed");
    assert_eq!(product_count,0,"must not download/register after acknowledged cancellation");
}

#[test]
fn audit_resource_read_must_use_the_same_authorized_canonical_path() {
    let root=unique_dir("audit-authorized-path");
    let ws=root.join("authorized-workspace");
    std::fs::create_dir_all(&ws).unwrap();
    let access=lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).unwrap();
    let context=ctx();
    let mut violations=Vec::new();
    for extension in ["png","wav"] {
        let name=format!("audit-relative-path-{}.{extension}",std::process::id());
        let outside=std::env::current_dir().unwrap().join(&name);
        let inside=ws.join(&name);
        std::fs::write(&inside,b"AUTHORIZED_CONTENT").unwrap();
        std::fs::write(&outside,b"OUTSIDE_WORKSPACE_SYNTHETIC_SECRET").unwrap();
        let read=if extension=="png" {
            match lingxi_service::operations::read_image_reference(&access,&context,std::path::Path::new(&name),&ws).unwrap() {
                lingxi_adapters::models::operations::ImageReference::Bytes{bytes,..}=>bytes,
                _=>panic!("bytes expected"),
            }
        }else{
            lingxi_service::operations::read_host_audio(&access,&context,std::path::Path::new(&name),&ws).unwrap().bytes
        };
        std::fs::remove_file(&outside).unwrap();
        println!("AUDIT authorized resource: type={extension}, authorized={inside:?}, actual_read_is_outside_workspace={}",read==b"OUTSIDE_WORKSPACE_SYNTHETIC_SECRET");
        if read!=b"AUTHORIZED_CONTENT" {violations.push(extension);}
    }
    assert!(violations.is_empty(),"authorization validates cwd+relative path but read uses process cwd instead, exposing unauthorized same-named file: {violations:?}");
}
