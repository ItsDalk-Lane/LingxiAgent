//! R05 RR1 (WP-T06): the permanent counterexamples for F17 / F18 / F19.
//!
//! Migrated from the adversarial evidence package
//! `artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/audit/worker_usage`
//! (P1 = F17, P2 = F18, P3 = F19) with the ORIGINAL behavioral assertions
//! kept intact; only the harness (loopback stub, fixture dirs) is adapted
//! to the in-repo test shape. Every test drives the REAL production chain
//! (ConfigModelGateway → CredentialService → OperationService → dialect →
//! dispatcher → egress-guarded product settlement) against loopback stubs.
//! Offline; NOT_REAL_API.
//!
//! The wrong-behavior demonstrability was proven on the unfixed candidate
//! (see `artifacts/rust-tauri/R05/RR1/WP-T06-E01/`): these assertions were
//! RED on the pre-fix tree and GREEN after the repair — they are contract
//! assertions, not restatements of the implementation.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_adapters::models::config::ModelPlaneConfig;
use lingxi_adapters::models::gateway::ConfigModelGateway;
use lingxi_adapters::models::operations::image::ImageRequest;
use lingxi_adapters::models::operations::video::VideoRequest;
use lingxi_kernel::{Principal, RunContext};
use lingxi_service::operations::{
    ImageGenerationOutcome, MediaTaskStatus, OperationService, RegisteredProduct,
};
use lingxi_service::quotas::{LayeredQuotaLimits, QuotaLimits, QuotaManager};
use sha2::Digest as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── the multi-path loopback stub (same equipment as r05_t06_operations) ─────

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
    #[allow(dead_code)]
    headers: Vec<(String, String)>,
}

impl Recorded {
    #[allow(dead_code)]
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

impl Stub {
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
                                "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
                                 Connection: close\r\n",
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

    async fn await_path(&self, prefix: &str) {
        for _ in 0..600 {
            if self.requests().iter().any(|r| r.path.starts_with(prefix)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("stub never saw a request under {prefix}");
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
    // The body is fully DRAINED (the stub answers scripted entries only
    // after the request body arrived); its bytes are not asserted on.
    Recorded { path, headers }
}

// ── the shared production assembly ─────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t06rr1-{tag}-{}-{}",
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
            per_agent: 8,
            per_session: 8,
        },
        ..QuotaLimits::default()
    }
}

/// The production operation chain over separate loopback providers with
/// DISTINCT synthetic keys (text / image / speech / video), mirroring the
/// adversarial package's `chain` equipment. Exposes the gateway so the
/// reload legs can swap the live plane.
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
    chain_with_quota(text_script, media_script, 4).await
}

async fn chain_with_quota(
    text_script: impl FnOnce(&str) -> Vec<ScriptEntry>,
    media_script: impl FnOnce(&str) -> Vec<ScriptEntry>,
    quota_global: usize,
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
    let (operations, _gateway) = build_operations(
        &plane_json,
        &runtime_dir,
        &products_root,
        quota_global,
        vec![
            format!("{}/v1", text_stub.endpoint),
            media_stub.endpoint.clone(),
            format!("{}/v1", media_stub.endpoint),
        ],
    );
    Chain {
        text_stub,
        media_stub,
        operations,
        products_root,
    }
}

fn build_operations(
    plane_json: &str,
    runtime_dir: &std::path::Path,
    products_root: &std::path::Path,
    quota_global: usize,
    endpoints: Vec<String>,
) -> (Arc<OperationService>, Arc<ConfigModelGateway>) {
    let plane = ModelPlaneConfig::parse_and_validate(plane_json).expect("plane parses");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credential service"),
    );
    let gateway = Arc::new(ConfigModelGateway::from_validated(plane));
    let operations = Arc::new(
        OperationService::new(
            Arc::clone(&gateway),
            credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
            Arc::new(QuotaManager::new(quota_limits(quota_global))),
            endpoints,
            products_root.to_path_buf(),
        )
        .expect("operation service"),
    );
    (operations, gateway)
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

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
        session_id: lingxi_protocol::SessionId::new("sess-t06rr1"),
        run_id: lingxi_protocol::RunId::new("run-t06rr1"),
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

// ═══ F17: the authorized attachment read must read the authorized scope ════

/// The P1 counterexample, assertion intact: the user path is a LEGAL
/// relative filename whose authorized cwd holds the authorized file; the
/// PROCESS cwd holds a same-named file with different (synthetic secret)
/// content OUTSIDE every authorized root. Both readers must return the
/// authorized workspace content — reading the process-cwd file exposes
/// content no authorization ever scoped.
#[test]
fn rr1_f17_attachment_read_uses_the_authorized_canonical_path() {
    let root = unique_dir("f17-canonical");
    let ws = root.join("authorized-workspace");
    std::fs::create_dir_all(&ws).expect("ws");
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let context = ctx();
    let mut violations = Vec::new();
    for extension in ["png", "wav"] {
        let name = format!("rr1-authorized-path-{}.{extension}", std::process::id());
        let outside = std::env::current_dir().unwrap().join(&name);
        let inside = ws.join(&name);
        std::fs::write(&inside, b"AUTHORIZED_CONTENT").expect("authorized file");
        std::fs::write(&outside, b"OUTSIDE_WORKSPACE_SYNTHETIC_SECRET").expect("outside file");
        let read = if extension == "png" {
            match lingxi_service::operations::read_image_reference(
                &access,
                &context,
                std::path::Path::new(&name),
                &ws,
            )
            .expect("the authorized read")
            {
                lingxi_adapters::models::operations::ImageReference::Bytes { bytes, .. } => bytes,
                other => panic!("a host-read reference is Bytes: {other:?}"),
            }
        } else {
            lingxi_service::operations::read_host_audio(
                &access,
                &context,
                std::path::Path::new(&name),
                &ws,
            )
            .expect("the authorized audio read")
            .bytes
        };
        std::fs::remove_file(&outside).expect("cleanup of the outside file");
        println!(
            "RR1-F17 authorized attachment: type={extension}, authorized={inside:?}, \
             actual_read_is_outside_workspace={}",
            read == b"OUTSIDE_WORKSPACE_SYNTHETIC_SECRET"
        );
        if read != b"AUTHORIZED_CONTENT" {
            violations.push(extension);
        }
    }
    assert!(
        violations.is_empty(),
        "authorization validates cwd+relative path but the read must use the authorized \
         canonical scope, never the process cwd's same-named file: {violations:?}"
    );
}

/// The positive control for F17: an absolute path inside the workspace and
/// a relative path under the authorized cwd both keep reading the real
/// authorized file (the repair must not break legal reads).
#[test]
fn rr1_f17_legal_attachment_reads_still_work() {
    let root = unique_dir("f17-legal");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let context = ctx();
    std::fs::write(ws.join("ref.png"), b"PNG-BYTES").expect("ref");
    std::fs::write(ws.join("note.wav"), b"WAV-BYTES").expect("note");
    match lingxi_service::operations::read_image_reference(
        &access,
        &context,
        &ws.join("ref.png"),
        &ws,
    )
    .expect("absolute authorized image read")
    {
        lingxi_adapters::models::operations::ImageReference::Bytes {
            bytes,
            mime,
            filename,
        } => {
            assert_eq!(bytes, b"PNG-BYTES");
            assert_eq!(mime, "image/png");
            assert_eq!(filename, "ref.png");
        }
        other => panic!("Bytes expected: {other:?}"),
    }
    let audio = lingxi_service::operations::read_host_audio(
        &access,
        &context,
        std::path::Path::new("note.wav"),
        &ws,
    )
    .expect("relative authorized audio read");
    assert_eq!(audio.bytes, b"WAV-BYTES");
    assert_eq!(audio.mime, "audio/wav");
    let _ = std::fs::remove_dir_all(&root);
}

/// An authorized path whose leaf was swapped to a symlink pointing OUTSIDE
/// every authorized root is judged by its REAL target — refused with zero
/// content exposure (the symlink-honesty discipline; the raw-path read
/// would have followed the link).
#[test]
fn rr1_f17_leaf_symlink_to_outside_is_refused_by_real_target() {
    let root = unique_dir("f17-symlink");
    let ws = root.join("ws");
    let outside = root.join("outside");
    std::fs::create_dir_all(&ws).expect("ws");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(outside.join("secret.png"), b"OUTSIDE_SECRET").expect("secret");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("secret.png"), ws.join("leak.png"))
            .expect("symlink");
        let access = Arc::new(
            lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"),
        );
        let refusal = lingxi_service::operations::read_image_reference(
            &access,
            &ctx(),
            std::path::Path::new("leak.png"),
            &ws,
        )
        .expect_err("a symlink whose real target is outside every root refuses");
        assert_eq!(
            refusal.code,
            lingxi_protocol::ErrorCode::Forbidden,
            "{refusal:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// A special file (FIFO) inside the workspace authorizes but must be
/// REFUSED as a bounded regular-file read, never read unbounded. The read
/// runs on a worker thread with a deadline: the unbounded old behavior
/// (an open that never returns) is a FAILURE here, not a hang.
#[test]
fn rr1_f17_special_file_is_refused_not_read_unbounded() {
    let root = unique_dir("f17-fifo");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let fifo = ws.join("pipe.wav");
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs on unix");
        let access = Arc::new(
            lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"),
        );
        let access_clone = Arc::clone(&access);
        let context = ctx();
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let outcome =
                lingxi_service::operations::read_host_audio(&access_clone, &context, &fifo, &ws);
            let _ = tx.send(outcome);
        });
        let outcome = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(outcome) => outcome,
            Err(_) => {
                panic!(
                    "the authorized read of a FIFO never returned — the reader must refuse \
                     non-regular sources bounded, never block or read them unbounded"
                )
            }
        };
        let refusal = outcome.expect_err("a FIFO is refused as an attachment");
        assert!(
            refusal.message.contains("not a regular file"),
            "the refusal names the non-regular fact: {refusal:?}"
        );
        assert!(
            !refusal.message.contains("OUTSIDE"),
            "no content ever rides the refusal"
        );
        // The thread may be blocked on the unbounded old behavior; the
        // assertion above already failed in that case. Give the detached
        // thread a bounded join window so the normal path stays clean.
        let _ = std::thread::spawn(move || {
            let _ = handle.join();
        });
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// The size caps stay loud refusals with the same vocabulary (never a
/// truncation) — the regression leg for the 20/25 MiB bounds.
#[test]
fn rr1_f17_attachment_size_caps_refuse_loudly() {
    let root = unique_dir("f17-caps");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("ws");
    let access =
        Arc::new(lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let context = ctx();
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
    let big_audio = ws.join("big.wav");
    std::fs::write(
        &big_audio,
        vec![0u8; lingxi_service::operations::AUDIO_INPUT_MAX_BYTES + 1],
    )
    .expect("big audio");
    let refusal = lingxi_service::operations::read_host_audio(&access, &context, &big_audio, &ws)
        .expect_err("the oversized audio refuses");
    assert!(refusal.message.contains("refused, never truncated"));
    let _ = std::fs::remove_dir_all(&root);
}

/// F37 leg (a) — the anti-WIDENING direction: the path is authorized while
/// its parent is a real directory; the parent component is THEN replaced by
/// a symlink pointing OUTSIDE every authorized root. The read is judged by
/// the REAL target of the path as it exists at read time (the authorize
/// step resolves the swapped chain, and the canonical no-follow open never
/// re-follows the raw name): both readers refuse with `Forbidden` and no
/// redirected content is ever exposed. A reader degraded to a raw-path
/// follow-open (the pre-F17 shape) would return the OUTSIDE bytes here.
#[test]
fn rr1_f37_parent_dir_swapped_to_outside_symlink_is_refused_by_real_target() {
    let root = unique_dir("f37-parent-outside");
    let ws = root.join("ws");
    let nested = ws.join("nested");
    std::fs::create_dir_all(&nested).expect("nested");
    let outside = root.join("outside-decoy");
    std::fs::create_dir_all(&outside).expect("outside decoy");
    std::fs::write(nested.join("leak.png"), b"AUTHORIZED_ORIGINAL_PNG").expect("png");
    std::fs::write(nested.join("leak.wav"), b"AUTHORIZED_ORIGINAL_WAV").expect("wav");
    std::fs::write(outside.join("leak.png"), b"OUTSIDE_REDIRECT_SECRET_PNG").expect("decoy png");
    std::fs::write(outside.join("leak.wav"), b"OUTSIDE_REDIRECT_SECRET_WAV").expect("decoy wav");
    #[cfg(unix)]
    {
        let access = Arc::new(
            lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"),
        );
        let context = ctx();
        // The path IS authorized while the parent is a real directory —
        // the swap below happens AFTER that authorization.
        access
            .authorize(
                context.principal.storage_kind(),
                &context.principal.storage_subject(),
                context.session_id.as_str(),
                std::path::Path::new("nested/leak.png"),
                &ws,
                lingxi_service::ResourceOp::Read,
            )
            .expect("authorized before the parent swap");
        std::fs::rename(&nested, root.join("attic-nested-outside"))
            .expect("move the real parent away");
        std::os::unix::fs::symlink(&outside, &nested)
            .expect("the parent component becomes an outside symlink");
        let refusal = lingxi_service::operations::read_image_reference(
            &access,
            &context,
            std::path::Path::new("nested/leak.png"),
            &ws,
        )
        .expect_err("judged by the REAL target outside every authorized root");
        assert_eq!(
            refusal.code,
            lingxi_protocol::ErrorCode::Forbidden,
            "{refusal:?}"
        );
        assert!(
            !refusal.message.contains("OUTSIDE_REDIRECT_SECRET"),
            "no redirected content ever rides the refusal: {refusal:?}"
        );
        let audio_refusal = lingxi_service::operations::read_host_audio(
            &access,
            &context,
            std::path::Path::new("nested/leak.wav"),
            &ws,
        )
        .expect_err("the same real-target judgment refuses the audio read");
        assert_eq!(
            audio_refusal.code,
            lingxi_protocol::ErrorCode::Forbidden,
            "{audio_refusal:?}"
        );
        assert!(
            !audio_refusal.message.contains("OUTSIDE_REDIRECT_SECRET"),
            "no redirected content ever rides the refusal: {audio_refusal:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// F37 leg (b) — the anti-TIGHTENING direction: the same parent swap, but
/// the symlink points at a directory INSIDE the authorized root. The real
/// target is authorized, so the read must deliver the REAL target's bytes
/// (not the stale original under the moved directory, and not a blanket
/// "symlink anywhere in the chain → refuse" rejection): symlink honesty
/// judges by the real target in BOTH directions.
#[test]
fn rr1_f37_parent_dir_swapped_to_inside_symlink_reads_the_real_target() {
    let root = unique_dir("f37-parent-inside");
    let ws = root.join("ws");
    let nested = ws.join("nested");
    let relocated = ws.join("relocated");
    std::fs::create_dir_all(&nested).expect("nested");
    std::fs::create_dir_all(&relocated).expect("relocated");
    std::fs::write(nested.join("leaf.png"), b"STALE_ORIGINAL_PNG").expect("stale png");
    std::fs::write(nested.join("leaf.wav"), b"STALE_ORIGINAL_WAV").expect("stale wav");
    std::fs::write(relocated.join("leaf.png"), b"REAL_TARGET_INSIDE_PNG").expect("real png");
    std::fs::write(relocated.join("leaf.wav"), b"REAL_TARGET_INSIDE_WAV").expect("real wav");
    #[cfg(unix)]
    {
        let access = Arc::new(
            lingxi_service::ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"),
        );
        let context = ctx();
        access
            .authorize(
                context.principal.storage_kind(),
                &context.principal.storage_subject(),
                context.session_id.as_str(),
                std::path::Path::new("nested/leaf.png"),
                &ws,
                lingxi_service::ResourceOp::Read,
            )
            .expect("authorized before the parent swap");
        std::fs::rename(&nested, root.join("attic-nested-inside"))
            .expect("move the real parent away");
        std::os::unix::fs::symlink(&relocated, &nested)
            .expect("the parent component becomes an inside symlink");
        match lingxi_service::operations::read_image_reference(
            &access,
            &context,
            std::path::Path::new("nested/leaf.png"),
            &ws,
        )
        .expect("the real target is inside the authorized root: the read delivers it")
        {
            lingxi_adapters::models::operations::ImageReference::Bytes {
                bytes,
                mime,
                filename,
            } => {
                assert_eq!(bytes, b"REAL_TARGET_INSIDE_PNG");
                assert_eq!(mime, "image/png");
                assert_eq!(filename, "leaf.png");
            }
            other => panic!("Bytes expected: {other:?}"),
        }
        let audio = lingxi_service::operations::read_host_audio(
            &access,
            &context,
            std::path::Path::new("nested/leaf.wav"),
            &ws,
        )
        .expect("the relocated audio reads through its real target");
        assert_eq!(audio.bytes, b"REAL_TARGET_INSIDE_WAV");
        assert_eq!(audio.mime, "audio/wav");
        assert_eq!(audio.filename, "leaf.wav");
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ═══ F18: the media task record keeps both identities ══════════════════════

/// The P2 counterexample, assertion intact: submit returns
/// task_id=local-track / provider_task_id=provider-vid; the poll MUST send
/// the provider-facing `video_id=provider-vid` — the local tracker is only
/// the legacy fallback identity.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f18_video_poll_sends_the_provider_job_id() {
    let c = chain(
        |_| vec![],
        |_| {
            vec![
                entry(
                    "/v1/videos",
                    200,
                    "application/json",
                    "{\"task_id\":\"local-track\",\"video_id\":\"provider-vid\"}".to_string(),
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
                prompt: "audit".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    assert_eq!(accepted.task_id, "local-track");
    assert_eq!(accepted.provider_task_id, "provider-vid");
    let status = c
        .operations
        .query_video(&accepted.task_id, None)
        .await
        .expect("query");
    assert!(matches!(status, MediaTaskStatus::Generating), "{status:?}");
    let paths: Vec<_> = c
        .media_stub
        .requests()
        .iter()
        .map(|r| r.path.clone())
        .collect();
    println!("RR1-F18 video poll paths: {paths:?}");
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    assert!(
        paths.iter().any(|p| p.contains("video_id=provider-vid")),
        "the poll must send the provider-facing video_id; the local tracker is only the \
         legacy fallback — actual paths: {paths:?}"
    );
}

/// The incumbent's legacy fallback: a non-2xx PRIMARY answer retries with
/// the DISTINCT host tracker id against `GET {v1}/videos/{legacy}` — the
/// service must have kept the host tracker identity to do so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f18_legacy_fallback_uses_the_distinct_host_tracker_id() {
    let c = chain(
        |_| vec![],
        |_| {
            vec![
                entry(
                    "/v1/videos",
                    200,
                    "application/json",
                    "{\"task_id\":\"local-track\",\"video_id\":\"provider-vid\"}".to_string(),
                ),
                // The PRIMARY query answers 404 → the legacy fallback leg.
                entry("/agnesapi", 404, "application/json", "{}".to_string()),
                entry(
                    "/v1/videos/local-track",
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
                prompt: "audit".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    let status = c
        .operations
        .query_video(&accepted.task_id, None)
        .await
        .expect("the legacy fallback settles the poll");
    assert!(matches!(status, MediaTaskStatus::Generating), "{status:?}");
    let paths: Vec<_> = c
        .media_stub
        .requests()
        .iter()
        .map(|r| r.path.clone())
        .collect();
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    assert!(
        paths
            .iter()
            .any(|p| p == "/v1/videos/local-track" || p.ends_with("/videos/local-track")),
        "the legacy fallback must poll the DISTINCT host tracker id — actual paths: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.contains("video_id=provider-vid")),
        "the primary query went out with the provider id first: {paths:?}"
    );
}

/// An old task NEVER retargets to a new provider: submit under video
/// provider A, reload the plane to provider B, poll the OLD task — the
/// poll must hit provider A with the stored provider job id, not be
/// re-resolved onto B.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f18_old_task_survives_reload_without_retargeting() {
    // Two SEPARATE media stubs: A took the submit; B is the reloaded
    // binding the old task must never reach.
    let stub_a = Stub::start(|endpoint| {
        vec![
            entry(
                "/v1/videos",
                200,
                "application/json",
                format!("{{\"task_id\":\"old-track\",\"video_id\":\"old-provider-vid\",\"confirm\":\"{endpoint}\"}}"),
            ),
            entry(
                "/agnesapi",
                200,
                "application/json",
                "{\"status\":\"processing\"}".to_string(),
            ),
        ]
    })
    .await;
    let stub_b = Stub::start(|_| {
        vec![entry(
            "/agnesapi",
            200,
            "application/json",
            "{\"status\":\"processing\"}".to_string(),
        )]
    })
    .await;
    let runtime_dir = unique_dir("reload-rt");
    let products_root = unique_dir("reload-products");
    let plane_a = format!(
        r#"{{
            "providers": {{
                "video_a": {{
                    "protocol": "agnes-videos",
                    "endpoint": "{}/v1",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-VIDEO-A-SECRET"}}
                }}
            }},
            "models": {{
                "video": {{"provider": "video_a", "model": "agnes-video"}}
            }}
        }}"#,
        stub_a.endpoint
    );
    let (operations, gateway) = build_operations(
        &plane_a,
        &runtime_dir,
        &products_root,
        4,
        vec![format!("{}/v1", stub_a.endpoint), stub_a.endpoint.clone()],
    );
    let accepted = operations
        .submit_video(
            VideoRequest {
                prompt: "before reload".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit under provider A");
    assert_eq!(accepted.provider_task_id, "old-provider-vid");

    // The hot reload: the SAME provider id now points at stub B.
    let plane_b = format!(
        r#"{{
            "providers": {{
                "video_a": {{
                    "protocol": "agnes-videos",
                    "endpoint": "{}/v1",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-VIDEO-A-SECRET"}}
                }}
            }},
            "models": {{
                "video": {{"provider": "video_a", "model": "agnes-video-NEW"}}
            }}
        }}"#,
        stub_b.endpoint
    );
    let plane_b = ModelPlaneConfig::parse_and_validate(&plane_b).expect("plane B parses");
    gateway.reload(plane_b);

    let status = operations
        .query_video(&accepted.task_id, None)
        .await
        .expect("the old task still polls");
    assert!(matches!(status, MediaTaskStatus::Generating), "{status:?}");
    let stub_b_hits = stub_b.hits();
    let a_paths: Vec<_> = stub_a.requests().iter().map(|r| r.path.clone()).collect();
    stub_a.stop().await;
    stub_b.stop().await;
    assert_eq!(
        stub_b_hits, 0,
        "the old task's poll must NEVER reach the reloaded provider binding"
    );
    assert!(
        a_paths
            .iter()
            .any(|p| p.contains("video_id=old-provider-vid")),
        "the old task polls provider A with the stored provider job id: {a_paths:?}"
    );
}

/// A duplicate provider task id while the first task is still live gets a
/// DISAMBIGUATED host tracker — cancelling one live task must never cancel
/// the other (the bare-id keyed map overwrote the first record before).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f18_duplicate_provider_task_id_is_disambiguated() {
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
    let first = c
        .operations
        .submit_video(
            VideoRequest {
                prompt: "first".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("first submit");
    let second = c
        .operations
        .submit_video(
            VideoRequest {
                prompt: "second".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("second submit");
    println!(
        "RR1-F18 duplicate tracker receipts: first={:?} second={:?}",
        first.task_id, second.task_id
    );
    assert_ne!(
        first.task_id, second.task_id,
        "two LIVE tasks sharing the provider's bare id must not share one host record"
    );
    // Cancelling the SECOND live task leaves the FIRST pollable.
    let cancelled = c
        .operations
        .cancel_media_task(&second.task_id)
        .expect("the second tracker cancels");
    assert!(matches!(
        cancelled,
        MediaTaskStatus::CancelledLocally { .. }
    ));
    let status = c
        .operations
        .query_video(&first.task_id, None)
        .await
        .expect("the first task is still independently pollable");
    assert!(
        matches!(status, MediaTaskStatus::Generating),
        "cancelling the duplicate-id sibling must not cancel this task: {status:?}"
    );
    c.text_stub.stop().await;
    c.media_stub.stop().await;
}

/// A video task id polled through the IMAGE entry is a kind mismatch —
/// refused as not tracked, never polled against the image binding.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f18_cross_kind_task_id_is_refused() {
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
                prompt: "video".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    let refusal = c
        .operations
        .query_media_task(&accepted.task_id, None)
        .await
        .expect_err("a video task id is not an image task");
    assert!(
        refusal.message.contains("not tracked"),
        "the refusal names the untracked-kind fact: {refusal:?}"
    );
    let dashscope_paths = c
        .media_stub
        .requests()
        .iter()
        .filter(|r| r.path.contains("/tasks/"))
        .count();
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    assert_eq!(
        dashscope_paths, 0,
        "the image poll entry must never poll a video task id"
    );
}

// ═══ F19: the cancellation fence spans poll / download / registration ═════

/// The P3 counterexample, assertion intact: the query request is in flight
/// when the local cancel is ACKNOWLEDGED; the late poll response must not
/// download media, register a file, or return Completed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f19_cancel_during_inflight_poll_fences_download_and_completion() {
    let c = chain(
        |_| vec![],
        |endpoint| {
            vec![
                entry(
                    "/v1/videos",
                    200,
                    "application/json",
                    "{\"task_id\":\"cancel-track\",\"video_id\":\"cancel-track\"}".to_string(),
                ),
                entry_delayed(
                    "/agnesapi",
                    "application/json",
                    format!(
                        "{{\"status\":\"completed\",\"url\":\"{endpoint}/audit-product.mp4\"}}"
                    ),
                    200,
                ),
                entry_bytes("/audit-product.mp4", "video/mp4", vec![1, 2, 3, 4]),
            ]
        },
    )
    .await;
    let accepted = c
        .operations
        .submit_video(
            VideoRequest {
                prompt: "audit".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    let operations = Arc::clone(&c.operations);
    let id = accepted.task_id.clone();
    let pending = tokio::spawn(async move { operations.query_video(&id, None).await });
    c.media_stub.await_path("/agnesapi").await;
    let cancel = c
        .operations
        .cancel_media_task(&accepted.task_id)
        .expect("the local cancel is acknowledged");
    assert!(
        matches!(
            cancel,
            MediaTaskStatus::CancelledLocally {
                remote_cancel: "unsupported"
            }
        ),
        "{cancel:?}"
    );
    let result = pending.await.expect("the poll settles");
    let paths: Vec<_> = c
        .media_stub
        .requests()
        .iter()
        .map(|r| r.path.clone())
        .collect();
    let product_count = product_files(&c.products_root).len();
    println!(
        "RR1-F19 cancellation race: late_result={result:?}; requests={paths:?}; \
         registered_products={product_count}"
    );
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    assert!(
        !matches!(result, Ok(MediaTaskStatus::Completed { .. })),
        "an acknowledged local cancellation must fence an in-flight poll, never let the \
         late answer overwrite the cancellation with Completed"
    );
    assert_eq!(
        product_count, 0,
        "no download/registration may happen after the acknowledged cancellation"
    );
}

/// The fence also spans the DOWNLOAD itself: the poll answered completed
/// and the product download is in flight when the cancel is acknowledged —
/// no file may remain registered (a late registration is cleaned up).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f19_cancel_during_download_leaves_no_registered_product() {
    let c = chain(
        |_| vec![],
        |endpoint| {
            vec![
                entry(
                    "/v1/videos",
                    200,
                    "application/json",
                    "{\"task_id\":\"dl-track\",\"video_id\":\"dl-track\"}".to_string(),
                ),
                entry(
                    "/agnesapi",
                    200,
                    "application/json",
                    format!(
                        "{{\"status\":\"completed\",\"url\":\"{endpoint}/audit-product.mp4\"}}"
                    ),
                ),
                entry_delayed(
                    "/audit-product.mp4",
                    "video/mp4",
                    "LATE-PRODUCT-BYTES".to_string(),
                    300,
                ),
            ]
        },
    )
    .await;
    let accepted = c
        .operations
        .submit_video(
            VideoRequest {
                prompt: "audit".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    let operations = Arc::clone(&c.operations);
    let id = accepted.task_id.clone();
    let pending = tokio::spawn(async move { operations.query_video(&id, None).await });
    // The poll response AND the download request are both on the wire.
    c.media_stub.await_path("/agnesapi").await;
    c.media_stub.await_path("/audit-product.mp4").await;
    let cancel = c
        .operations
        .cancel_media_task(&accepted.task_id)
        .expect("cancel during the in-flight download");
    assert!(matches!(cancel, MediaTaskStatus::CancelledLocally { .. }));
    let result = pending.await.expect("the poll settles");
    let product_count = product_files(&c.products_root).len();
    println!(
        "RR1-F19 cancel-during-download: late_result={result:?}; registered_products={product_count}"
    );
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    assert!(
        matches!(result, Ok(MediaTaskStatus::CancelledLocally { .. })),
        "the cancel was acknowledged before the completion committed: {result:?}"
    );
    assert_eq!(
        product_count, 0,
        "a registration raced by the acknowledged cancellation must be cleaned up"
    );
}

/// Concurrent polls of the SAME completed task deliver ONCE: one download,
/// one registered file, both callers get the SAME terminal receipt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f19_concurrent_polls_deliver_once() {
    let product_bytes = vec![9u8, 8, 7];
    let c = chain(
        |_| vec![],
        |endpoint| {
            vec![
                entry(
                    "/v1/videos",
                    200,
                    "application/json",
                    "{\"task_id\":\"dup-track\",\"video_id\":\"dup-track\"}".to_string(),
                ),
                entry(
                    "/agnesapi",
                    200,
                    "application/json",
                    format!("{{\"status\":\"completed\",\"url\":\"{endpoint}/prod.mp4\"}}"),
                ),
                entry_bytes("/prod.mp4", "video/mp4", product_bytes.clone()),
            ]
        },
    )
    .await;
    let accepted = c
        .operations
        .submit_video(
            VideoRequest {
                prompt: "audit".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    let left = Arc::clone(&c.operations);
    let right = Arc::clone(&c.operations);
    let id = accepted.task_id.clone();
    let left_id = id.clone();
    let left = tokio::spawn(async move { left.query_video(&left_id, None).await });
    let right = tokio::spawn(async move { right.query_video(&id, None).await });
    let (left_result, right_result) = (
        left.await.expect("left settles"),
        right.await.expect("right settles"),
    );
    let downloads = c
        .media_stub
        .requests()
        .iter()
        .filter(|r| r.path.starts_with("/prod.mp4"))
        .count();
    let files = product_files(&c.products_root);
    println!(
        "RR1-F19 concurrent polls: left={left_result:?} right={right_result:?} \
         downloads={downloads} files={}",
        files.len()
    );
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    let (
        Ok(MediaTaskStatus::Completed { products: pl }),
        Ok(MediaTaskStatus::Completed { products: pr }),
    ) = (&left_result, &right_result)
    else {
        panic!("both concurrent polls settle Completed: {left_result:?} / {right_result:?}")
    };
    assert_eq!(pl.len(), 1);
    assert_eq!(pr.len(), 1);
    assert_eq!(
        pl[0].resource_id, pr[0].resource_id,
        "both callers get the SAME terminal receipt"
    );
    product_is_real(&pl[0], &product_bytes, "video/mp4");
    assert_eq!(downloads, 1, "the product downloads exactly once");
    assert_eq!(files.len(), 1, "exactly one registered file exists");
}

/// Completion first: once a task has COMMITTED Completed, a later cancel
/// does not fabricate a cancellation or undo the delivered fact — and the
/// receipt stays queryable (idempotent).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f19_cancel_after_completion_keeps_the_delivered_fact() {
    let product_bytes = vec![4u8, 5, 6];
    let c = chain(
        |_| vec![],
        |endpoint| {
            vec![
                entry(
                    "/v1/videos",
                    200,
                    "application/json",
                    "{\"task_id\":\"done-track\",\"video_id\":\"done-track\"}".to_string(),
                ),
                entry(
                    "/agnesapi",
                    200,
                    "application/json",
                    format!("{{\"status\":\"completed\",\"url\":\"{endpoint}/done.mp4\"}}"),
                ),
                entry_bytes("/done.mp4", "video/mp4", product_bytes.clone()),
            ]
        },
    )
    .await;
    let accepted = c
        .operations
        .submit_video(
            VideoRequest {
                prompt: "audit".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("submit");
    let first = c
        .operations
        .query_video(&accepted.task_id, None)
        .await
        .expect("the first poll completes");
    let MediaTaskStatus::Completed { products } = first else {
        panic!("completed: {first:?}")
    };
    assert_eq!(products.len(), 1);
    let cancel = c
        .operations
        .cancel_media_task(&accepted.task_id)
        .expect("cancel after completion answers with the delivered fact");
    assert!(
        matches!(cancel, MediaTaskStatus::Completed { .. }),
        "a committed completion is never undone by a late cancel: {cancel:?}"
    );
    let again = c
        .operations
        .query_video(&accepted.task_id, None)
        .await
        .expect("the receipt stays queryable");
    let MediaTaskStatus::Completed { products: replay } = again else {
        panic!("receipt: {again:?}")
    };
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].resource_id, products[0].resource_id);
    assert_eq!(product_files(&c.products_root).len(), 1);
    let downloads = c
        .media_stub
        .requests()
        .iter()
        .filter(|r| r.path.starts_with("/done.mp4"))
        .count();
    c.text_stub.stop().await;
    c.media_stub.stop().await;
    assert_eq!(downloads, 1, "the receipt replay never re-downloads");
}

// ── an end-to-end positive control: the async image task still completes
// through the registered product (the F18/F19 repair keeps the honest
// media lifecycles of the c12 suite intact).

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f18_f19_dashscope_image_task_still_completes_via_registered_product() {
    let product_bytes = vec![1u8, 5, 9];
    let stub = Stub::start(|endpoint| {
        vec![
            entry(
                "/api/v1/services/aigc/image-generation/generation",
                200,
                "application/json",
                "{\"output\":{\"task_id\":\"tsk-rr1\",\"task_status\":\"PENDING\"}}".to_string(),
            ),
            entry(
                "/api/v1/tasks/tsk-rr1",
                200,
                "application/json",
                format!(
                    "{{\"output\":{{\"task_status\":\"SUCCEEDED\",\"results\":[{{\"url\":\
                     \"{endpoint}/files/img.png\"}}]}}}}"
                ),
            ),
            entry_bytes("/files/img.png", "image/png", product_bytes.clone()),
        ]
    })
    .await;
    let runtime_dir = unique_dir("dash-rt");
    let products_root = unique_dir("dash-products");
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
    let (operations, _gateway) = build_operations(
        &plane_json,
        &runtime_dir,
        &products_root,
        2,
        vec![
            format!("{}/compatible-mode/v1", stub.endpoint),
            format!("{}/api/v1", stub.endpoint),
        ],
    );
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
    assert_eq!(task_id, "tsk-rr1");
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
    assert_eq!(product_files(&products_root).len(), 1);
    stub.stop().await;
}

// ── keep the SocketAddr import used on the thief-origin shape below ────────

#[allow(dead_code)]
fn _thief_origin_shape(addr: SocketAddr) -> String {
    format!("http://{addr}/evil.jpeg")
}
