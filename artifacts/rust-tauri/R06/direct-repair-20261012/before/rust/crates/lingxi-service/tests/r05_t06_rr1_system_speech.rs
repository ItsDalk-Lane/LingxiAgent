//! R05 RR1 (WP-T06) F20: the macOS system-speech lane under the unified
//! permit / deadline / cancellation supervision — REAL `/usr/bin/say`
//! evidence on this machine (macOS arm64), plus the honest-refusal legs.
//!
//! The pre-fix behavior was demonstrated on the unfixed candidate with a
//! dedicated probe (see `artifacts/rust-tauri/R05/RR1/WP-T06-E01/
//! old-red-current-tree-f20-probe.log`): the lane dispatched BEFORE the
//! admission permit (a queued call with a 150 ms deadline returned a
//! registered product while the single model permit was held), ignored
//! the caller's deadline, and a dropped caller future left the say
//! process alive with its intermediate output on disk. These permanent
//! tests pin the repaired contract. Offline (local process only);
//! NOT_REAL_API.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_adapters::models::config::ModelPlaneConfig;
use lingxi_adapters::models::gateway::ConfigModelGateway;
use lingxi_adapters::models::operations::image::ImageRequest;
use lingxi_adapters::models::operations::speech::SpeechRequest;
use lingxi_service::operations::{
    MediaTaskStatus, OperationService, OperationUsageFact, OperationUsageSink, RegisteredProduct,
    SYSTEM_SPEECH_KILL_GRACE_MS,
};
use lingxi_service::procsupervisor::{ProcessSupervisor, SupervisorLimits};
use lingxi_service::quotas::{LayeredQuotaLimits, QuotaLimits, QuotaManager};
use sha2::Digest as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── a minimal loopback stub (the permit-holder leg) ────────────────────────

struct Stub {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Stub {
    async fn start(body: &'static str, delay_ms: u64) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let endpoint = format!("http://{addr}");
        let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                let seen = Arc::clone(&task_requests);
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    loop {
                        let mut chunk = [0u8; 4096];
                        let Ok(read) = socket.read(&mut chunk).await else {
                            break;
                        };
                        if read == 0 {
                            break;
                        }
                        raw.extend_from_slice(&chunk[..read]);
                        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let head = String::from_utf8_lossy(&raw).to_string();
                    let path = head
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_string();
                    seen.lock().unwrap().push(path.clone());
                    if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: \
                         {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self {
            endpoint,
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn saw(&self, prefix: &str) -> bool {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.starts_with(prefix))
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

// ── the shared assembly ────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-f20-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn speech_plane(endpoint: &str) -> String {
    format!(
        r#"{{
            "providers": {{
                "say_host": {{
                    "protocol": "system-speech",
                    "endpoint": "{endpoint}",
                    "auth": {{"kind": "none"}}
                }}
            }},
            "models": {{
                "speech": {{"provider": "say_host", "model": "system-say"}}
            }}
        }}"#
    )
}

struct Assembly {
    operations: Arc<OperationService>,
    products_root: PathBuf,
    usage: Arc<RecordingSink>,
    _spill: PathBuf,
}

/// Records every usage fact (the settlement assertions read it).
struct RecordingSink {
    facts: Mutex<Vec<OperationUsageFact>>,
}

impl OperationUsageSink for RecordingSink {
    fn record<'a>(
        &'a self,
        fact: OperationUsageFact,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::StorageError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.facts.lock().unwrap().push(fact);
            Ok(())
        })
    }
}

fn supervisor(spill: &std::path::Path) -> Arc<ProcessSupervisor> {
    let mut limits = SupervisorLimits::with_spill_dir(spill.to_path_buf());
    limits.cleanup_timeout = Duration::from_millis(SYSTEM_SPEECH_KILL_GRACE_MS);
    Arc::new(
        ProcessSupervisor::new(Arc::new(lingxi_service::inject::SystemClock), limits)
            .expect("test supervisor"),
    )
}

fn assemble(endpoint: &str, quota_global: usize, tag: &str) -> Assembly {
    let runtime_dir = unique_dir(&format!("{tag}-rt"));
    let products_root = unique_dir(&format!("{tag}-products"));
    let spill = unique_dir(&format!("{tag}-spill"));
    let plane = ModelPlaneConfig::parse_and_validate(&speech_plane(endpoint)).expect("plane");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credentials"),
    );
    let gateway = Arc::new(ConfigModelGateway::from_validated(plane));
    let usage = Arc::new(RecordingSink {
        facts: Mutex::new(Vec::new()),
    });
    let operations = Arc::new(
        OperationService::new(
            gateway,
            credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
            Arc::new(QuotaManager::new(QuotaLimits {
                model: LayeredQuotaLimits {
                    global: quota_global,
                    per_agent: 4,
                    per_session: 4,
                },
                ..QuotaLimits::default()
            })),
            vec![endpoint.to_string()],
            products_root.clone(),
        )
        .expect("operations")
        .with_process_supervisor(supervisor(&spill))
        .with_usage_sink(Arc::clone(&usage) as Arc<dyn OperationUsageSink>),
    );
    Assembly {
        operations,
        products_root,
        usage,
        _spill: spill,
    }
}

fn product_files(root: &std::path::Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .map(|entries| entries.map(|e| e.expect("entry").path()).collect())
        .unwrap_or_default()
}

/// Counts say processes spawned for THIS assembly only: the argv carries
/// the assembly's unique `-o {root}/speech-say-{tag}.m4a` — scoping the
/// pattern to the root keeps the count precise under parallel test
/// execution (a sibling test's say process is a different root).
fn say_processes_alive(root: &std::path::Path) -> usize {
    let pattern = format!("{}/speech-say-", root.display());
    std::process::Command::new("pgrep")
        .arg("-f")
        .arg(&pattern)
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|line| !line.is_empty())
                .count()
        })
        .unwrap_or(0)
}

fn speech_product_is_real(product: &RegisteredProduct) {
    let on_disk = std::fs::read(&product.path).expect("the registered product file exists");
    assert_eq!(product.size_bytes, on_disk.len() as u64);
    let digest: String = sha2::Sha256::digest(&on_disk)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(product.sha256, digest);
    assert_eq!(product.mime, "audio/mp4");
}

// ═══ the battery (real macOS platform legs) ════════════════════════════════

/// A REAL say synthesis: the registered product is a real non-empty m4a
/// with the digest of exactly what was written, and the accounting fact
/// lands with ZERO transport attempts (a local process is not a provider
/// HTTP call — the pre-fix shape fabricated an HTTP attempt).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_real_say_registers_product_with_zero_transport_attempts() {
    let a = assemble("http://127.0.0.1:9", 2, "real");
    let product = a
        .operations
        .synthesize_speech(
            SpeechRequest {
                text: "灵犀系统语音监督验证".to_string(),
                voice: None,
                speed: None,
                format: None,
            },
            None,
        )
        .await
        .expect("the real synthesis delivers");
    speech_product_is_real(&product);
    assert!(product.size_bytes > 0);
    // The intermediate say output is cleaned; only the registered product
    // remains.
    let files = product_files(&a.products_root);
    assert_eq!(files.len(), 1, "only the registered product file remains");
    let facts = a.usage.facts.lock().unwrap();
    assert_eq!(
        facts.len(),
        1,
        "exactly one settlement fact (op={} attempts={})",
        facts[0].operation,
        facts[0].transport_attempts
    );
    assert_eq!(facts[0].operation, "speech");
    assert_eq!(
        facts[0].transport_attempts, 0,
        "a local process makes no provider HTTP request"
    );
    assert_eq!(facts[0].protocol, "system-speech");
    assert_eq!(
        say_processes_alive(&a.products_root),
        0,
        "no say process outlives the call"
    );
}

/// A short deadline against a long synthesis: the call fails with the
/// budget fact, the process is killed and reaped through the supervisor
/// chain, the intermediate output is removed, no product is registered,
/// and the settlement still lands (attempts 0).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_short_deadline_times_out_kills_and_cleans() {
    let a = assemble("http://127.0.0.1:9", 2, "timeout");
    let long_text = "hello world ".repeat(2000);
    // The deadline (1.2 s) is far under the §29.13 cap (120 s) and under
    // the real synthesis time of this text (~15 s on this machine).
    let deadline = lingxi_adapters::models::dispatch::unix_ms_now() + 1_200;
    let failure = a
        .operations
        .synthesize_speech(
            SpeechRequest {
                text: long_text,
                voice: None,
                speed: None,
                format: None,
            },
            Some(deadline),
        )
        .await
        .expect_err("the short deadline must cut the synthesis");
    assert_eq!(
        failure.code,
        lingxi_protocol::ErrorCode::BudgetExceeded,
        "{failure:?}"
    );
    assert!(
        failure.message.contains("timed out"),
        "the failure names the timeout: {failure:?}"
    );
    assert!(
        failure.message.contains("1"),
        "the effective cap came from the caller's remaining budget: {failure:?}"
    );
    // Reaping is bounded by the kill grace; give it a margin.
    tokio::time::sleep(Duration::from_millis(SYSTEM_SPEECH_KILL_GRACE_MS + 800)).await;
    assert_eq!(
        say_processes_alive(&a.products_root),
        0,
        "the say process was killed"
    );
    assert!(
        product_files(&a.products_root).is_empty(),
        "no intermediate output and no product remain"
    );
    let facts = a.usage.facts.lock().unwrap();
    assert_eq!(
        facts.len(),
        1,
        "the failed call still settles (op={} attempts={})",
        facts[0].operation,
        facts[0].transport_attempts
    );
    assert_eq!(facts[0].transport_attempts, 0);
}

/// A dropped caller future (cancel): the ownership guard terminates the
/// process through the supervisor chain and the intermediate output is
/// removed — nothing of the call survives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_dropped_future_kills_process_and_cleans_output() {
    let a = assemble("http://127.0.0.1:9", 2, "cancel");
    let long_text = "hello world ".repeat(2000);
    let operations = Arc::clone(&a.operations);
    let call = tokio::spawn(async move {
        operations
            .synthesize_speech(
                SpeechRequest {
                    text: long_text,
                    voice: None,
                    speed: None,
                    format: None,
                },
                None,
            )
            .await
    });
    // Wait until say has created its intermediate output file.
    let mut appeared = false;
    for _ in 0..600 {
        if !product_files(&a.products_root).is_empty() {
            appeared = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(appeared, "the probe never observed the say output start");
    call.abort();
    tokio::time::sleep(Duration::from_millis(SYSTEM_SPEECH_KILL_GRACE_MS + 1500)).await;
    assert_eq!(
        say_processes_alive(&a.products_root),
        0,
        "aborting the caller future kills the spawned say process"
    );
    assert!(
        product_files(&a.products_root).is_empty(),
        "the intermediate say output is cleaned after the drop"
    );
    let facts = a.usage.facts.lock().unwrap();
    assert!(
        facts.is_empty(),
        "a dropped call never reaches a fake settlement (count={})",
        facts.len()
    );
}

/// The unified permit: while the SINGLE global model permit is held by an
/// in-flight network operation, a queued system-speech call whose total
/// budget expires in the queue fails honestly (BudgetExceeded) WITHOUT
/// spawning say — the pre-fix lane bypassed admission entirely.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_queued_speech_waits_for_the_unified_permit() {
    let stub = Stub::start(r#"{"data":[{"b64_json":"QUJD"}}"#, 600).await;
    // A plane with BOTH the image family (network, holds the permit) and
    // the system-speech lane, sharing the ONE global model permit.
    let runtime_dir = unique_dir("permit-rt");
    let products_root = unique_dir("permit-products");
    let spill = unique_dir("permit-spill");
    let plane_json = format!(
        r#"{{
            "providers": {{
                "image_b": {{
                    "protocol": "openai-images",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-IMAGE-B-SECRET"}}
                }},
                "say_host": {{
                    "protocol": "system-speech",
                    "endpoint": "http://127.0.0.1:9",
                    "auth": {{"kind": "none"}}
                }}
            }},
            "models": {{
                "image": {{"provider": "image_b", "model": "dall-e-3"}},
                "speech": {{"provider": "say_host", "model": "system-say"}}
            }}
        }}"#,
        stub.endpoint
    );
    let plane = ModelPlaneConfig::parse_and_validate(&plane_json).expect("plane");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credentials"),
    );
    let gateway = Arc::new(ConfigModelGateway::from_validated(plane));
    let operations = Arc::new(
        OperationService::new(
            gateway,
            credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
            Arc::new(QuotaManager::new(QuotaLimits {
                model: LayeredQuotaLimits {
                    global: 1,
                    per_agent: 2,
                    per_session: 2,
                },
                ..QuotaLimits::default()
            })),
            vec![stub.endpoint.clone()],
            products_root.clone(),
        )
        .expect("operations")
        .with_process_supervisor(supervisor(&spill)),
    );
    let holder = Arc::clone(&operations);
    let image = tokio::spawn(async move {
        holder
            .generate_image(
                ImageRequest {
                    prompt: "holder".into(),
                    ..Default::default()
                },
                None,
            )
            .await
    });
    while !stub.saw("/images/generations") {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let deadline = lingxi_adapters::models::dispatch::unix_ms_now() + 150;
    let speech = operations
        .synthesize_speech(
            SpeechRequest {
                text: "queued speech".into(),
                voice: None,
                speed: None,
                format: None,
            },
            Some(deadline),
        )
        .await;
    let outcome = format!("{speech:?}");
    let files = product_files(&products_root);
    let alive = say_processes_alive(&products_root);
    println!(
        "RR1-F20 queued leg: speech={outcome}; product_files={}; say_alive={alive}",
        files.len()
    );
    let _ = image.await;
    stub.stop().await;
    let refusal = speech.expect_err("the queued call outlived its budget in the queue");
    assert_eq!(
        refusal.code,
        lingxi_protocol::ErrorCode::BudgetExceeded,
        "{refusal:?}"
    );
    assert!(
        files.is_empty(),
        "no product may be registered by the fenced call"
    );
    assert_eq!(alive, 0, "no say process may be spawned by the fenced call");
}

/// An already-exhausted deadline refuses BEFORE spawning anything.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_exhausted_deadline_refuses_before_spawn() {
    let a = assemble("http://127.0.0.1:9", 2, "spent");
    let spent = lingxi_adapters::models::dispatch::unix_ms_now() - 1;
    let failure = a
        .operations
        .synthesize_speech(
            SpeechRequest {
                text: "too late".into(),
                voice: None,
                speed: None,
                format: None,
            },
            Some(spent),
        )
        .await
        .expect_err("a spent budget refuses before any local process");
    assert_eq!(failure.code, lingxi_protocol::ErrorCode::BudgetExceeded);
    assert!(product_files(&a.products_root).is_empty());
    assert_eq!(say_processes_alive(&a.products_root), 0);
    assert!(a.usage.facts.lock().unwrap().is_empty());
}

/// Argument-injection hardening (a same-class finding of the F20 scan,
/// verified on this platform): `say -o A "-oB"` makes say write B — user
/// text must NEVER ride as argv options. With the `--` separator the
/// leading-dash message is synthesized as TEXT and the output stays at
/// the host-chosen path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_leading_dash_text_is_message_not_option() {
    let a = assemble("http://127.0.0.1:9", 2, "dash");
    let product = a
        .operations
        .synthesize_speech(
            SpeechRequest {
                text: "-o/tmp/lingxi-f20-inject-target.m4a".to_string(),
                voice: None,
                speed: None,
                format: None,
            },
            None,
        )
        .await
        .expect("the leading-dash text synthesizes as a message");
    speech_product_is_real(&product);
    let files = product_files(&a.products_root);
    assert_eq!(
        files.len(),
        1,
        "only the registered product remains: {files:?}"
    );
    // The injected path never received the audio.
    assert!(
        !std::path::Path::new("/tmp/lingxi-f20-inject-target.m4a").exists(),
        "the leading-dash message must not redirect say's output"
    );
    assert_eq!(say_processes_alive(&a.products_root), 0);
    let facts = a.usage.facts.lock().unwrap();
    assert_eq!(facts.len(), 1);
}

/// Observed platform fact (this machine): an UNKNOWN voice name makes say
/// fall back to a default voice and exit 0 — the lane passes the voice
/// through, the synthesis still delivers a real product (registered and
/// accounted); this pins the honest behavior instead of guessing an
/// error the platform does not produce.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_unknown_voice_falls_back_and_delivers() {
    let a = assemble("http://127.0.0.1:9", 2, "fallback");
    let product = a
        .operations
        .synthesize_speech(
            SpeechRequest {
                text: "fallback voice".to_string(),
                voice: Some("definitely-not-a-voice-zzz".into()),
                speed: None,
                format: None,
            },
            None,
        )
        .await
        .expect("the platform synthesizes with its fallback voice");
    speech_product_is_real(&product);
    assert_eq!(product_files(&a.products_root).len(), 1);
    assert_eq!(say_processes_alive(&a.products_root), 0);
    let facts = a.usage.facts.lock().unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].transport_attempts, 0);
}

/// A write-denied product root: say cannot create its output — the
/// failure is honest, nothing is registered, nothing survives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_unwritable_output_fails_honestly() {
    let a = assemble("http://127.0.0.1:9", 2, "ro");
    // Drop write permission on the product root: say cannot create the
    // intermediate file and exits non-zero.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&a.products_root, std::fs::Permissions::from_mode(0o500))
            .expect("chmod");
    }
    let failure = a
        .operations
        .synthesize_speech(
            SpeechRequest {
                text: "no write".into(),
                voice: None,
                speed: None,
                format: None,
            },
            None,
        )
        .await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&a.products_root, std::fs::Permissions::from_mode(0o755))
            .expect("restore perms");
    }
    let failure = failure.expect_err("the unwritable root fails loudly");
    assert_eq!(
        failure.code,
        lingxi_protocol::ErrorCode::UpstreamUnavailable,
        "{failure:?}"
    );
    assert!(product_files(&a.products_root).is_empty());
    assert_eq!(say_processes_alive(&a.products_root), 0);
}

/// An operation service WITHOUT the supervised process plane refuses the
/// lane fail-closed — never spawns an unsupervised child (the pre-fix
/// shape would happily run).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_missing_supervisor_refuses_fail_closed() {
    let runtime_dir = unique_dir("nosup-rt");
    let products_root = unique_dir("nosup-products");
    let plane =
        ModelPlaneConfig::parse_and_validate(&speech_plane("http://127.0.0.1:9")).expect("plane");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credentials"),
    );
    let gateway = Arc::new(ConfigModelGateway::from_validated(plane));
    let operations = Arc::new(
        OperationService::new(
            gateway,
            credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
            Arc::new(QuotaManager::new(QuotaLimits::default())),
            vec![],
            products_root.clone(),
        )
        .expect("operations"),
    );
    let failure = operations
        .synthesize_speech(
            SpeechRequest {
                text: "no supervisor".into(),
                voice: None,
                speed: None,
                format: None,
            },
            None,
        )
        .await
        .expect_err("the lane refuses without the supervised process plane");
    assert!(
        failure.message.contains("supervised process plane"),
        "{failure:?}"
    );
    assert!(product_files(&products_root).is_empty());
    assert_eq!(say_processes_alive(&products_root), 0);
}

/// The empty-text guard fires before any spawn (the same rule the network
/// dialects enforce).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rr1_f20_empty_text_refuses_before_spawn() {
    let a = assemble("http://127.0.0.1:9", 2, "empty");
    let failure = a
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
        .expect_err("empty text refuses");
    assert_eq!(failure.message, "prompt is required");
    assert!(product_files(&a.products_root).is_empty());
    assert_eq!(say_processes_alive(&a.products_root), 0);
    // Keep the unused-import warning away for MediaTaskStatus in shapes
    // where only some legs compile assertions on it.
    let _ = std::any::type_name::<MediaTaskStatus>();
}
