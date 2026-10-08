//! R05-T08 binary closed loop (Appendix B T08 C01–C09/C12 at the layer the
//! acceptance demands): the REAL `lingxi-service` binary — started as a
//! subprocess with real argv and a real `--config` file — driven through
//! authenticated submissions, REAL file tools, REAL protocol turns against a
//! loopback provider stub, honest crash/restart legs and bounded-resource
//! cycles. Nothing here calls `bootstrap_with_deps` or any test constructor.
//!
//! The provider stub is a BEHAVIORAL stand-in for the external network only:
//! it never reads the workspace, never knows the runtime nonces (each is
//! generated AFTER the stub router is fixed), and never decides Run /
//! approval / storage state. It routes on what a real model legitimately
//! sees — the user text and the tool results that rode the previous request
//! — and every anti-cheat assertion checks the RUNTIME nonce reached the
//! next request verbatim through the real file tool.
//!
//! Per-case scope (the remaining T08 C-IDs are mapped in
//! docs/rust-tauri/R05/R05_TEST_MAP.json to their owning suites/commands):
//! - C01 five-way chain: HTTP/journal/file/network/final-message agreement
//!   across read→edit→read→final→SIGTERM→restart→history-read.
//! - C02 two full chain runs, different runtime nonces per run.
//! - C03 allowed vs denied file sentinels (ResourceAccess boundary).
//! - C04 requestId idempotent replay vs content conflict.
//! - C05 restart reads history with ZERO re-execution.
//! - C06 SIGKILL after the tool effect landed (model turn parked) → no
//!   blind re-dispatch, honest interrupted terminal, effect preserved.
//! - C07 SIGKILL mid-stream (delta sent, terminal frame withheld) → no
//!   fabricated final, partial preserved, parked connection dropped.
//! - C08 two concurrent sessions, shared external callId, isolated files.
//! - C09 live event page vs persisted projection of the same stream.
//! - C12 repeated mixed cycles stay inside pre-registered resource bounds.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

// ── routing stub provider (the far end of the wire) ─────────────────────────

struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    body: serde_json::Value,
}

/// What the stub does once it has read one provider request.
enum Step {
    /// Answer with a complete SSE body, then close the connection.
    Sse(String),
    /// Answer with the first bytes, then HOLD the connection open until the
    /// gate flips, then send the rest (C09's live-first barrier).
    SseGated {
        first: String,
        rest: String,
        release: Arc<AtomicBool>,
    },
    /// Read the request and never answer (C06's parked model turn), or send
    /// only the first bytes and stall (C07's mid-stream cut).
    Park { first_bytes: Option<String> },
    /// Plain HTTP error (C12's provider-failure cycles).
    Error(u16, String),
}

type Router = Box<dyn Fn(&serde_json::Value) -> Step + Send + Sync>;

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    /// Parked connections the far side (the service) has CLOSED — observed
    /// by the stub's read loop on the held socket.
    dropped: Arc<AtomicUsize>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start(router: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let dropped = Arc::new(AtomicUsize::new(0));
        let router = Arc::new(router);
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_requests, task_dropped) = (
            Arc::clone(&hits),
            Arc::clone(&requests),
            Arc::clone(&dropped),
        );
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                task_hits.fetch_add(1, Ordering::SeqCst);
                let recorded = read_one_request(&mut socket).await;
                let step = router(&recorded.body);
                task_requests.lock().expect("requests").push(recorded);
                match step {
                    Step::Sse(body) => {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    }
                    Step::SseGated {
                        first,
                        rest,
                        release,
                    } => {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                             Cache-Control: no-cache\r\nConnection: close\r\n\r\n";
                        if socket.write_all(head.as_bytes()).await.is_err() {
                            task_dropped.fetch_add(1, Ordering::SeqCst);
                            continue;
                        }
                        if socket.write_all(first.as_bytes()).await.is_err() {
                            task_dropped.fetch_add(1, Ordering::SeqCst);
                            continue;
                        }
                        socket.flush().await.ok();
                        let deadline = Instant::now() + Duration::from_secs(120);
                        while !release.load(Ordering::SeqCst) {
                            if Instant::now() > deadline {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        let _ = socket.write_all(rest.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    }
                    Step::Park { first_bytes } => {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                             Cache-Control: no-cache\r\nConnection: close\r\n\r\n";
                        let _ = socket.write_all(head.as_bytes()).await;
                        if let Some(bytes) = first_bytes {
                            let _ = socket.write_all(bytes.as_bytes()).await;
                        }
                        socket.flush().await.ok();
                        // Hold until the far side goes away; a 0-byte read or
                        // an error means the service dropped the connection.
                        let mut probe = [0_u8; 512];
                        loop {
                            match tokio::time::timeout(
                                Duration::from_secs(3600),
                                socket.read(&mut probe),
                            )
                            .await
                            {
                                Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
                                Ok(Ok(_)) => continue, // tolerate stray bytes
                            }
                        }
                        task_dropped.fetch_add(1, Ordering::SeqCst);
                    }
                    Step::Error(status, body) => {
                        let response = format!(
                            "HTTP/1.1 {status} StubError\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    }
                }
            }
        });
        Self {
            addr,
            hits,
            requests,
            dropped,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn dropped(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }

    fn requests(&self) -> Vec<(String, Option<String>, serde_json::Value)> {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .map(|r| (r.path.clone(), r.authorization.clone(), r.body.clone()))
            .collect()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

async fn read_one_request(socket: &mut tokio::net::TcpStream) -> RecordedRequest {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(30), socket.read(&mut chunk))
            .await
            .expect("stub read stalled")
            .expect("stub read");
        if read == 0 {
            panic!("stub: connection closed before headers completed");
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
        assert!(
            raw.len() < 256 * 1024,
            "stub: header block unreasonably large"
        );
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("stub utf8 headers");
    let mut lines = head.split("\r\n");
    let path = lines
        .next()
        .expect("request line")
        .split_whitespace()
        .nth(1)
        .expect("request path")
        .to_string();
    let mut authorization = None;
    let mut content_length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "authorization" {
                authorization = Some(value.to_string());
            }
            if name == "content-length" {
                content_length = Some(value.parse::<usize>().expect("numeric content-length"));
            }
        }
    }
    let content_length = content_length.expect("json posts carry a content-length");
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(30), socket.read(&mut chunk))
            .await
            .expect("stub body read stalled")
            .expect("stub body read");
        if read == 0 {
            panic!("stub: connection closed mid-body");
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    RecordedRequest {
        path,
        authorization,
        body: serde_json::from_slice(&raw[body_start..body_start + content_length])
            .expect("stub: request body must be JSON"),
    }
}

// ── openai-completions SSE builders (production streams) ────────────────────

fn sse_of(frames: &[serde_json::Value], usage: serde_json::Value) -> String {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"id": "chatcmpl-stub", "choices": [], "usage": usage})
    ));
    body.push_str("data: [DONE]\n\n");
    body
}

fn tool_call_turn(call_id: &str, name: &str, arguments: &str) -> String {
    sse_of(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                "model": "stub-model-t08",
                "choices": [{
                    "index": 0,
                    "finish_reason": null,
                    "delta": {
                        "role": "assistant",
                        "tool_calls": [{
                            "index": 0,
                            "id": call_id,
                            "type": "function",
                            "function": {"name": name, "arguments": arguments}
                        }]
                    }
                }]
            }),
            serde_json::json!({
                "id": "chatcmpl-stub",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
            }),
        ],
        serde_json::json!({"prompt_tokens": 30, "completion_tokens": 11}),
    )
}

fn final_turn(text: &str) -> String {
    sse_of(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                "model": "stub-model-t08",
                "choices": [{
                    "index": 0,
                    "finish_reason": null,
                    "delta": {"role": "assistant", "content": text}
                }]
            }),
            serde_json::json!({
                "id": "chatcmpl-stub",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
            }),
        ],
        serde_json::json!({"prompt_tokens": 40, "completion_tokens": 17}),
    )
}

// ── child process harness (the REAL binary, async spawn/read) ───────────────

struct ServiceChild {
    child: tokio::process::Child,
    addr: SocketAddr,
    ready_line: String,
    /// The exact argv tail the process under test was launched with.
    argv_tail: Vec<String>,
    /// Kept for post-mortem diagnostics of later legs; the startup panic
    /// paths read their own clones before the struct exists.
    #[allow(dead_code)]
    stderr_lines: Arc<Mutex<Vec<String>>>,
    /// Detached pipe-drain tasks: they exit with the child's pipes; the
    /// handles are held only so the tasks are not dropped before the child.
    _drain: tokio::task::JoinHandle<()>,
    _stderr: tokio::task::JoinHandle<()>,
}

impl ServiceChild {
    /// Starts the REAL shipping binary with the given args; waits (bounded)
    /// for the machine-readable readiness line on stdout.
    async fn start(args: &[String]) -> Self {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_lingxi-service"))
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn the real lingxi-service binary");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_task = {
            let sink = Arc::clone(&stderr_lines);
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => sink
                            .lock()
                            .expect("stderr log")
                            .push(line.trim_end().to_string()),
                    }
                }
            })
        };
        // Read stdout lines until the readiness marker (bounded). Every
        // non-marker exit path panics, so the loop's value is always the
        // readiness line.
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        let ready_line = loop {
            line.clear();
            let read = tokio::time::timeout(
                deadline.saturating_duration_since(Instant::now()),
                reader.read_line(&mut line),
            )
            .await
            .unwrap_or_else(|_| {
                let stderr = stderr_lines.lock().expect("stderr log").join("\n");
                panic!(
                    "the real binary never printed LINGXI_SERVICE_READY within 30s\nstderr so far:\n{stderr}"
                )
            })
            .expect("stdout read");
            if read == 0 {
                let stderr = stderr_lines.lock().expect("stderr log").join("\n");
                panic!("the real binary exited before readiness\nstderr so far:\n{stderr}");
            }
            let trimmed = line.trim_end().to_string();
            if trimmed.starts_with("LINGXI_SERVICE_READY ") {
                break trimmed;
            }
        };
        // Keep draining stdout in the background so the pipe never fills.
        let drain = tokio::spawn(async move {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => continue,
                }
            }
        });
        let addr: SocketAddr = ready_line
            .split_whitespace()
            .find_map(|part| part.strip_prefix("addr="))
            .expect("readiness line carries addr=")
            .parse()
            .expect("ready addr parses");
        Self {
            child,
            addr,
            ready_line,
            argv_tail: args.to_vec(),
            stderr_lines,
            _drain: drain,
            _stderr: stderr_task,
        }
    }

    fn pid(&self) -> i32 {
        self.child.id().expect("child pid") as i32
    }
}

impl ServiceChild {
    /// Graceful SIGTERM (the production signal path), bounded wait.
    async fn stop(mut self) -> std::process::ExitStatus {
        let pid = self.pid();
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.try_wait().expect("poll child") {
                return status;
            }
            if Instant::now() > deadline {
                self.child.kill().await.expect("last-resort kill");
                panic!("the real binary did not exit within 30s of SIGTERM");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Hard SIGKILL (the crash leg — the process is NOT allowed to drain).
    async fn kill(mut self) -> std::process::ExitStatus {
        self.child.kill().await.expect("kill the child");
        self.child.wait().await.expect("reap the killed child")
    }
}

impl Drop for ServiceChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.child.start_kill().expect("cleanup kill");
        }
    }
}

// ── authenticated HTTP client (loopback, one request per connection) ────────

async fn http_json(
    addr: SocketAddr,
    method: &str,
    path: &str,
    bearer: &str,
    body: Option<&serde_json::Value>,
) -> (u16, serde_json::Value) {
    let exchanged = tokio::time::timeout(Duration::from_secs(120), async {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to the real service");
        let payload = body.map(|b| b.to_string());
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {bearer}\r\nConnection: close\r\n"
        );
        if let Some(payload) = &payload {
            head.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                payload.len()
            ));
        } else {
            head.push_str("Content-Length: 0\r\n");
        }
        head.push_str("\r\n");
        socket.write_all(head.as_bytes()).await.expect("write head");
        if let Some(payload) = &payload {
            socket
                .write_all(payload.as_bytes())
                .await
                .expect("write body");
        }
        let mut raw = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            match socket.read(&mut chunk).await {
                Ok(0) => break,
                Ok(read) => raw.extend_from_slice(&chunk[..read]),
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    break;
                }
                Err(err) => panic!("read response: {err}"),
            }
        }
        raw
    })
    .await
    .expect("request to the real service stalled — surfaced honestly, not skipped");
    let text = String::from_utf8_lossy(&exchanged);
    let (head, body) = text.split_once("\r\n\r\n").expect("header/body split");
    let status = head
        .split_whitespace()
        .nth(1)
        .expect("status")
        .parse::<u16>()
        .expect("numeric status");
    (
        status,
        serde_json::from_str(if body.is_empty() { "null" } else { body })
            .expect("JSON response body"),
    )
}

// ── shared helpers ──────────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t08-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

/// A runtime nonce the stub router cannot know: generated per invocation,
/// AFTER the stub's routing rules are fixed (each test writes the nonce into
/// the workspace file only after `StubServer::start` returned).
fn runtime_nonce(tag: &str) -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!(
        "NONCE-T08-{tag}-{}-{nanos}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

struct Harness {
    home: PathBuf,
    workspace: PathBuf,
    #[allow(dead_code)]
    config_path: PathBuf,
    argv_tail: Vec<String>,
}

impl Harness {
    /// Creates home/workspace/config around a stub endpoint; the caller
    /// seeds workspace files AFTER the stub router is fixed.
    fn new(tag: &str, stub_endpoint: &str, extra_argv: &[&str]) -> Self {
        let home = unique_dir(&format!("{tag}-home"));
        let workspace = unique_dir(&format!("{tag}-ws"));
        let config_root = unique_dir(&format!("{tag}-cfg"));
        let config_path = config_root.join("service.json");
        std::fs::write(
            &config_path,
            format!(
                r#"{{"home": {}, "workspace": {}, "providers": {{
                    "main": {{
                        "protocol": "openai-completions",
                        "endpoint": "{}",
                        "auth": {{"kind": "apiKey", "apiKey": "sk-test-t08"}}
                    }}
                }},
                "models": {{"chat": {{"provider": "main", "model": "stub-model-t08", "capabilities": {{"tools": true}}}}}}}}"#,
                serde_json::to_string(&home.to_string_lossy()).expect("home json"),
                serde_json::to_string(&workspace.to_string_lossy()).expect("ws json"),
                stub_endpoint
            ),
        )
        .expect("write config");
        let mut argv_tail = vec![
            "--home".to_string(),
            home.to_string_lossy().to_string(),
            "--config".to_string(),
            config_path.to_string_lossy().to_string(),
            "--bind".to_string(),
            "127.0.0.1:0".to_string(),
        ];
        for extra in extra_argv {
            argv_tail.push((*extra).to_string());
        }
        Self {
            home,
            workspace,
            config_path,
            argv_tail,
        }
    }

    async fn start_service(&self) -> ServiceChild {
        ServiceChild::start(&self.argv_tail).await
    }

    fn token(&self) -> String {
        read_loopback_token(&self.home)
    }
}

fn read_loopback_token(home: &Path) -> String {
    let canonical = std::fs::canonicalize(home).expect("canonical home");
    let token_path = canonical.join("lingxi-service").join("local-token.json");
    let file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&token_path).expect("loopback token file readable"))
            .expect("token file JSON");
    file["token"].as_str().expect("token field").to_string()
}

/// Opens the run database READ-ONLY (the service still owns the writer;
/// WAL permits concurrent readers) straight from the on-disk bytes.
fn open_runs_db(home: &Path) -> rusqlite::Connection {
    let canonical = std::fs::canonicalize(home).expect("canonical home");
    let db_path = canonical
        .join("lingxi-service")
        .join("data")
        .join("runs.db");
    rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("open the run database read-only")
}

fn query_one(db: &rusqlite::Connection, sql: &str, params: &[&str]) -> Option<String> {
    db.query_row(sql, rusqlite::params_from_iter(params.iter()), |row| {
        row.get::<_, rusqlite::types::Value>(0)
            .map(|value| match value {
                rusqlite::types::Value::Integer(n) => n.to_string(),
                rusqlite::types::Value::Text(s) => s,
                other => format!("{other:?}"),
            })
    })
    .ok()
}

async fn submit(
    addr: SocketAddr,
    token: &str,
    session: &str,
    input: &str,
) -> (u16, serde_json::Value) {
    http_json(
        addr,
        "POST",
        &format!("/lingxi/v1/sessions/{session}/execute"),
        token,
        Some(&serde_json::json!({"input": input})),
    )
    .await
}

/// Polls for the most recent run id of one session (the DB is the authority;
/// used with detached submissions whose HTTP response is not available).
async fn latest_run_id(home: &Path, session: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let db = open_runs_db(home);
        let id = query_one(
            &db,
            "SELECT run_id FROM runs WHERE session_id = ?1 ORDER BY rowid DESC LIMIT 1",
            &[session],
        );
        if let Some(id) = id {
            return id;
        }
        assert!(
            Instant::now() <= deadline,
            "no run appeared for session {session}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Fires a submission WITHOUT awaiting its response: the execute endpoint
/// drives the run to completion synchronously, so a parked/stalled provider
/// stream would otherwise hold this task for the whole request budget. The
/// connection stays OPEN (the run is not a client-disconnect case); the
/// eventual response (or the transport error when the service is killed) is
/// only observed if the caller awaits the handle.
async fn submit_bg(
    addr: SocketAddr,
    token: String,
    session: String,
    input: String,
) -> tokio::task::JoinHandle<Option<(u16, serde_json::Value)>> {
    tokio::spawn(async move {
        let path = format!("/lingxi/v1/sessions/{session}/execute");
        let body = serde_json::json!({"input": input});
        let exchanged = tokio::time::timeout(Duration::from_secs(150), async {
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect to the real service");
            let payload = body.to_string();
            let head = format!(
                "POST {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\
                 Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            socket.write_all(head.as_bytes()).await.expect("write head");
            socket
                .write_all(payload.as_bytes())
                .await
                .expect("write body");
            let mut raw = Vec::new();
            let mut chunk = [0_u8; 8192];
            loop {
                match socket.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(read) => raw.extend_from_slice(&chunk[..read]),
                    Err(_) => break, // killed service / reset connection
                }
            }
            raw
        })
        .await;
        let raw = match exchanged {
            Ok(raw) if !raw.is_empty() => raw,
            _ => return None, // transport died (the crash legs kill the service)
        };
        let text = String::from_utf8_lossy(&raw);
        let (head, body) = text.split_once("\r\n\r\n")?;
        let status = head.split_whitespace().nth(1)?.parse::<u16>().ok()?;
        let value = serde_json::from_str(if body.is_empty() { "null" } else { body }).ok()?;
        Some((status, value))
    })
}

async fn submit_with_request_id(
    addr: SocketAddr,
    token: &str,
    session: &str,
    input: &str,
    request_id: &str,
) -> (u16, serde_json::Value) {
    http_json(
        addr,
        "POST",
        &format!("/lingxi/v1/sessions/{session}/execute"),
        token,
        Some(&serde_json::json!({"input": input, "requestId": request_id})),
    )
    .await
}

/// Polls the run database until the run reaches a wanted status (bounded;
/// panics with the observed status on timeout — never sleeps "long enough"
/// and hopes).
async fn wait_run_status(
    home: &Path,
    run_id: &str,
    mut wanted: impl FnMut(&str) -> bool,
    what: &str,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let db = open_runs_db(home);
        let status = query_one(&db, "SELECT status FROM runs WHERE run_id = ?1", &[run_id])
            .unwrap_or_default();
        if wanted(&status) {
            return status;
        }
        if Instant::now() > deadline {
            panic!("run {run_id} never reached {what}; last status: {status}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// The content of the LAST `role: "tool"` message of a provider request —
/// what the model legitimately observed from the previous tool round.
fn last_tool_result(body: &serde_json::Value) -> Option<String> {
    let messages = body["messages"].as_array()?;
    messages
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .and_then(|m| m["content"].as_str())
        .map(str::to_string)
}

/// The user text of the request (the first user message) — how the
/// behavioral router tells two sessions' intents apart without any identity
/// payload (there is none: identity never rides model requests).
fn user_text(body: &serde_json::Value) -> String {
    body["messages"]
        .as_array()
        .and_then(|messages| {
            messages
                .iter()
                .find(|m| m["role"] == "user")
                .and_then(|m| m["content"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

const EDIT_MARKER: &str = "EDITED-BY-REAL-TOOL";
/// The edit tool's success receipt text (the router keys on it, exactly
/// like a real model reads "Successfully replaced N block(s)").
const EDIT_RECEIPT: &str = "Successfully replaced";

/// The C01/C02 behavioral profile: read → edit(exact observed content) →
/// read → final(quoting the observed edited content). The file name comes
/// from the router closure; the CONTENT always comes from the wire.
fn chain_router(file: &str) -> Router {
    let file = file.to_string();
    let read_args = serde_json::json!({"path": file}).to_string();
    Box::new(move |body| {
        let Some(observed) = last_tool_result(body) else {
            // No tool result yet → ask for the read.
            return Step::Sse(tool_call_turn("call_t08_read", "read", &read_args));
        };
        if observed.contains(EDIT_MARKER) {
            // The verification read observed the edited file → final.
            Step::Sse(final_turn(&format!(
                "t08 chain final — file now: {observed}"
            )))
        } else if observed.contains(EDIT_RECEIPT) {
            // The edit landed (its receipt says so) → verify with a read.
            Step::Sse(tool_call_turn("call_t08_read2", "read", &read_args))
        } else {
            // The read result IS the file content → construct the edit from
            // the OBSERVED bytes (oldText exact, marker appended).
            let edits = serde_json::json!({
                "path": file,
                "edits": [{
                    "oldText": observed,
                    "newText": format!("{observed}{EDIT_MARKER}\n"),
                }]
            })
            .to_string();
            Step::Sse(tool_call_turn("call_t08_edit", "edit", &edits))
        }
    })
}

// ── C01: five-way agreement across the full chain and a restart ─────────────

#[tokio::test]
async fn c01_read_edit_read_final_restart_five_way_chain() {
    let stub = StubServer::start(chain_router("alpha.txt")).await;
    let nonce = runtime_nonce("c01");
    let h = Harness::new("c01", &stub.endpoint(), &[]);
    let v1 = format!("key={nonce}\n");
    std::fs::write(h.workspace.join("alpha.txt"), &v1).expect("seed alpha.txt");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;
    assert!(child.ready_line.contains("source=cli"));
    // The process under test was launched with exactly these argv.
    assert_eq!(child.argv_tail, h.argv_tail);

    // Unauthorized first (the authenticated surface is real).
    let (unauthorized, _) = http_json(
        addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        "wrong-token",
        Some(&serde_json::json!({"input": "process alpha.txt"})),
    )
    .await;
    assert_eq!(unauthorized, 401, "the endpoint is authenticated");

    let (status, accepted) = submit(addr, &token, "sess_local_alpha", "process alpha.txt").await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    // (1) NETWORK: four real provider turns — read, edit, read, final —
    // each authenticated with the configured key and the configured model.
    let requests = stub.requests();
    assert_eq!(stub.hits(), 4, "exactly four provider turns hit the wire");
    for (path, authorization, body) in &requests {
        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(authorization.as_deref(), Some("Bearer sk-test-t08"));
        assert_eq!(body["model"], "stub-model-t08");
    }
    // The edit turn's request carried the REAL nonce as the read's tool
    // result (the stub only learns the nonce from the wire).
    let messages_of = |i: usize| {
        requests[i].2["messages"]
            .as_array()
            .expect("messages")
            .clone()
    };
    // R06-T01: every turn now leads with the compiled system context
    // (messages[0] = role "system", the frozen artifact's render); the
    // positional indices below shifted by exactly one.
    assert_eq!(messages_of(0).len(), 2, "first turn: system + user");
    assert_eq!(messages_of(0)[0]["role"], "system");
    assert_eq!(
        messages_of(1).len(),
        4,
        "system + user + assistant(tool) + tool result"
    );
    assert_eq!(messages_of(1)[3]["role"], "tool");
    assert_eq!(messages_of(1)[3]["tool_call_id"], "call_t08_read");
    assert_eq!(
        messages_of(1)[3]["content"],
        v1,
        "the runtime nonce rode the wire"
    );
    // Turn 3 (the verification read) carries the edit's receipt in place;
    // the FINAL turn then sees the EDITED content — proof the edit really
    // happened before the second read.
    let v2 = format!("{v1}{EDIT_MARKER}\n");
    assert_eq!(
        messages_of(2).len(),
        6,
        "system, user, asst(read), tool(read), asst(edit), tool(receipt)"
    );
    assert_eq!(messages_of(2)[5]["role"], "tool");
    assert_eq!(messages_of(2)[5]["tool_call_id"], "call_t08_edit");
    assert!(
        messages_of(2)[5]["content"]
            .as_str()
            .is_some_and(|c| c.contains("Successfully replaced")),
        "the edit receipt rode the wire: {}",
        messages_of(2)[5]["content"]
    );
    assert_eq!(
        messages_of(3).len(),
        8,
        "system, …, asst(read2), tool(read2 result)"
    );
    assert_eq!(messages_of(3)[7]["tool_call_id"], "call_t08_read2");
    assert_eq!(messages_of(3)[7]["content"], v2);

    // (2) FILE: the real edit landed on disk.
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("alpha.txt")).expect("read alpha.txt"),
        v2,
        "the real edit tool changed the file"
    );

    // (3) JOURNAL: straight from the on-disk database.
    {
        let db = open_runs_db(&h.home);
        assert_eq!(
            query_one(
                &db,
                "SELECT terminal_reason FROM runs WHERE run_id = ?1",
                &[&run_id]
            )
            .as_deref(),
            Some("completed.with_final")
        );
        assert_eq!(
            query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_completed'",
                &[&run_id]
            )
            .as_deref(),
            Some("4")
        );
        assert_eq!(
            query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'tool_call_completed'",
                &[&run_id]
            )
            .as_deref(),
            Some("3")
        );
        let final_row = query_one(
            &db,
            "SELECT content_json FROM messages WHERE run_id = ?1",
            &[&run_id],
        )
        .expect("final message row");
        assert!(
            final_row.contains(&nonce) && final_row.contains(EDIT_MARKER),
            "the persisted final message quotes the observed content: {final_row}"
        );
    }

    // (4)+(5) EXTERNAL RESULT via the authenticated history surface, before
    // AND after a real restart: same events, same final message, zero new
    // model/tool executions (the stub keeps counting across the restart).
    let (before_status, page_before) = http_json(
        addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events",
        &token,
        None,
    )
    .await;
    assert_eq!(before_status, 200);
    let items_before = page_before["items"].as_array().expect("items").len();
    assert!(
        items_before >= 8,
        "at least the 4 model + 3 tool + final events"
    );
    let hits_before = stub.hits();
    let child_pid_before_restart = child.pid();

    child.stop().await; // graceful SIGTERM — the production signal path
    let child2 = h.start_service().await;
    assert_ne!(
        child2.pid(),
        child_pid_before_restart,
        "a NEW process, not a resume"
    );
    let addr2 = child2.addr;
    // A fresh process mints a fresh loopback token — read it again.
    let token2 = h.token();
    let (snap_status, snapshot) = http_json(
        addr2,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha",
        &token2,
        None,
    )
    .await;
    assert_eq!(snap_status, 200);
    assert_eq!(
        snapshot["runCount"], 1,
        "no run was re-created by the restart"
    );
    let (after_status, page_after) = http_json(
        addr2,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events",
        &token2,
        None,
    )
    .await;
    assert_eq!(after_status, 200);
    assert_eq!(
        page_after["items"].as_array().expect("items").len(),
        items_before,
        "history after restart is IDENTICAL — nothing re-executed, nothing lost"
    );
    assert_eq!(
        stub.hits(),
        hits_before,
        "zero new provider turns after restart"
    );

    // The five-way agreement closes on the restarted surface too.
    let final_events: Vec<serde_json::Value> = page_after["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|e| e["payload"]["type"] == "final_message_committed")
        .cloned()
        .collect();
    assert_eq!(final_events.len(), 1, "exactly one final message");
    let final_text = serde_json::to_string(&final_events[0]["payload"]["message"]).unwrap();
    assert!(
        final_text.contains(&nonce) && final_text.contains(EDIT_MARKER),
        "the history final message quotes the real edited content: {final_text}"
    );

    child2.stop().await;
    stub.stop().await;
}

// ── C02: two full chain runs, different runtime nonces ──────────────────────

#[tokio::test]
async fn c02_two_full_chain_runs_different_runtime_nonces() {
    let stub = StubServer::start(chain_router("beta.txt")).await;
    let h = Harness::new("c02", &stub.endpoint(), &[]);
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let mut final_rows: Vec<String> = Vec::new();
    for round in 1..=2u32 {
        // The nonce is generated AFTER the stub router is fixed and written
        // into the file only now — the stub cannot pre-know either value.
        let nonce = runtime_nonce(&format!("c02r{round}"));
        let v1 = format!("key={nonce}\n");
        std::fs::write(h.workspace.join("beta.txt"), &v1).expect("seed beta.txt");
        let (status, accepted) = submit(
            addr,
            &token,
            "sess_local_alpha",
            &format!("process beta.txt round {round}"),
        )
        .await;
        assert_eq!(status, 200, "round {round} accepted: {accepted}");
        let run_id = accepted["runId"].as_str().expect("runId").to_string();
        wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;

        let requests = stub.requests();
        let this_round: Vec<(String, Option<String>, serde_json::Value)> = requests
            .iter()
            .skip(4 * (round as usize - 1))
            .cloned()
            .collect();
        assert_eq!(this_round.len(), 4, "round {round}: four provider turns");
        let messages = this_round[1].2["messages"].as_array().expect("messages");
        // R06-T01: messages[0] is the compiled system context; the read's
        // tool result now sits at index 3.
        assert_eq!(
            messages[3]["content"], v1,
            "round {round}: the FRESH nonce rode the next request verbatim"
        );
        let db = open_runs_db(&h.home);
        let final_row = query_one(
            &db,
            "SELECT content_json FROM messages WHERE run_id = ?1",
            &[&run_id],
        )
        .expect("final message row");
        assert!(
            final_row.contains(&nonce),
            "round {round} final came from THIS round's tool result: {final_row}"
        );
        final_rows.push(final_row);
    }

    // The two rounds' finals must differ (no fixed template, no turn-number
    // answer): the persisted content differs because the nonces differ.
    assert_eq!(final_rows.len(), 2, "one final per round");
    assert_ne!(
        final_rows[0], final_rows[1],
        "different nonces must yield different finals"
    );

    child.stop().await;
    stub.stop().await;
}

// ── C03: allowed vs denied file sentinels ───────────────────────────────────

#[tokio::test]
async fn c03_allowed_and_denied_file_sentinels() {
    let outside = unique_dir("c03-outside");
    let denied_sentinel = outside.join("secret.txt");
    std::fs::write(&denied_sentinel, "DO-NOT-TOUCH\n").expect("seed denied sentinel");
    let denied_abs = denied_sentinel.to_string_lossy().to_string();
    let read_denied = serde_json::json!({"path": denied_abs}).to_string();
    let edit_denied = serde_json::json!({
        "path": denied_abs,
        "edits": [{"oldText": "DO-NOT-TOUCH", "newText": "PWNED"}]
    })
    .to_string();
    let read_for_stub = read_denied.clone();
    let edit_for_stub = edit_denied.clone();
    let stub = StubServer::start(Box::new(move |body| {
        let tool_count = body["messages"]
            .as_array()
            .map(|m| m.iter().filter(|x| x["role"] == "tool").count())
            .unwrap_or(0);
        match last_tool_result(body).as_deref() {
            None => Step::Sse(tool_call_turn("call_t08_rd", "read", &read_for_stub)),
            Some(content) if content.contains("DO-NOT-TOUCH") => {
                // The outside read LEAKED through some hole — say so loudly;
                // the assertions below fail on this content.
                Step::Sse(final_turn(&format!("LEAKED: {content}")))
            }
            Some(_) if tool_count <= 1 => {
                // First refusal observed (the read was denied) → try edit.
                Step::Sse(tool_call_turn("call_t08_ed", "edit", &edit_for_stub))
            }
            Some(_) => Step::Sse(final_turn("both attempts refused — honest")),
        }
    }))
    .await;
    let h = Harness::new("c03", &stub.endpoint(), &[]);
    let allowed = h.workspace.join("gamma.txt");
    std::fs::write(&allowed, "allowed sentinel body\n").expect("seed allowed file");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (status, accepted) = submit(addr, &token, "sess_local_alpha", "try the outside file").await;
    assert_eq!(status, 200);
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    // The denied file never changed and never leaked.
    assert_eq!(
        std::fs::read_to_string(&denied_sentinel).expect("denied sentinel readable"),
        "DO-NOT-TOUCH\n",
        "zero side effects on the denied file"
    );
    let requests = stub.requests();
    let all_tool_results: Vec<String> = requests
        .iter()
        .flat_map(|(_, _, body)| {
            body["messages"]
                .as_array()
                .map(|m| {
                    m.iter()
                        .filter(|x| x["role"] == "tool")
                        .filter_map(|x| x["content"].as_str())
                        .map(str::to_string)
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default()
        })
        .collect();
    assert!(all_tool_results.len() >= 2, "both refusals ride the wire");
    for content in &all_tool_results {
        assert!(
            !content.contains("DO-NOT-TOUCH"),
            "the denied file's content never reached the model: {content}"
        );
    }
    assert!(
        all_tool_results
            .iter()
            .all(|c| c.contains("refused") || c.contains("tool error")),
        "structured refusals, not fake successes: {all_tool_results:?}"
    );
    let db = open_runs_db(&h.home);
    let final_row = query_one(
        &db,
        "SELECT content_json FROM messages WHERE run_id = ?1",
        &[&run_id],
    )
    .expect("final row");
    assert!(
        final_row.contains("refused") && !final_row.contains("DO-NOT-TOUCH"),
        "no fake product: {final_row}"
    );

    // The allowed sentinel (inside the workspace root) stays reachable —
    // the C01/C02 chains prove the same tools complete on in-root files.
    assert!(
        allowed.exists(),
        "the allowed sentinel stays reachable inside the workspace"
    );

    child.stop().await;
    stub.stop().await;
    let _ = std::fs::remove_dir_all(&outside);
}

// ── C04: requestId idempotent replay vs content conflict ────────────────────

#[tokio::test]
async fn c04_request_id_replay_and_conflict() {
    let read_args = serde_json::json!({"path": "delta.txt"}).to_string();
    let stub = StubServer::start(Box::new(move |body| {
        if last_tool_result(body).is_some() {
            Step::Sse(final_turn("c04 done"))
        } else {
            Step::Sse(tool_call_turn("call_t08_c4", "read", &read_args))
        }
    }))
    .await;
    let h = Harness::new("c04", &stub.endpoint(), &[]);
    std::fs::write(h.workspace.join("delta.txt"), "c04 body\n").expect("seed delta");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (s1, a1) = submit_with_request_id(
        addr,
        &token,
        "sess_local_alpha",
        "request id one",
        "req-t08-c4",
    )
    .await;
    assert_eq!(s1, 200, "{a1}");
    let run_id = a1["runId"].as_str().expect("runId").to_string();
    wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    let hits_after_first = stub.hits();
    assert_eq!(hits_after_first, 2);

    // Same requestId + SAME content → idempotent replay, nothing re-executed.
    let (s2, a2) = submit_with_request_id(
        addr,
        &token,
        "sess_local_alpha",
        "request id one",
        "req-t08-c4",
    )
    .await;
    assert_eq!(s2, 200, "replay accepted: {a2}");
    assert_eq!(
        a2["replayed"], true,
        "the response marks an idempotent REPLAY"
    );
    assert_eq!(a2["runId"], run_id, "the SAME run — no new execution");
    assert_eq!(
        stub.hits(),
        hits_after_first,
        "zero new provider turns on replay"
    );
    {
        let db = open_runs_db(&h.home);
        assert_eq!(
            query_one(&db, "SELECT COUNT(*) FROM runs", &[]).as_deref(),
            Some("1"),
            "still exactly one run"
        );
    }

    // Same requestId + DIFFERENT content → explicit conflict, no new run.
    let (s3, e3) = submit_with_request_id(
        addr,
        &token,
        "sess_local_alpha",
        "CHANGED content",
        "req-t08-c4",
    )
    .await;
    assert!(
        s3 == 409 || s3 == 400,
        "the conflict is an explicit client error, got {s3}: {e3}"
    );
    let err_text = e3.to_string();
    assert!(
        err_text.contains("req-t08-c4"),
        "the conflict names the requestId: {err_text}"
    );
    assert_eq!(
        stub.hits(),
        hits_after_first,
        "the conflict triggered no execution"
    );
    {
        let db = open_runs_db(&h.home);
        assert_eq!(
            query_one(&db, "SELECT COUNT(*) FROM runs", &[]).as_deref(),
            Some("1"),
            "no second run was created by the conflicting submission"
        );
    }

    child.stop().await;
    stub.stop().await;
}

// ── C05: restart reads history with zero re-execution ───────────────────────

#[tokio::test]
async fn c05_restart_reads_history_zero_reexecution() {
    let read_args = serde_json::json!({"path": "persist.txt"}).to_string();
    let stub = StubServer::start(Box::new(move |body| {
        if last_tool_result(body).is_some() {
            Step::Sse(final_turn("c05 done"))
        } else {
            Step::Sse(tool_call_turn("call_t05", "read", &read_args))
        }
    }))
    .await;
    let h = Harness::new("c05", &stub.endpoint(), &[]);
    let nonce = runtime_nonce("c05");
    std::fs::write(h.workspace.join("persist.txt"), format!("P={nonce}\n")).expect("seed persist");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (s, a) = submit(addr, &token, "sess_local_alpha", "read persist.txt").await;
    assert_eq!(s, 200, "{a}");
    let run_id = a["runId"].as_str().expect("runId").to_string();
    wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    let (hits, model_calls, tool_calls, finals) = {
        let db = open_runs_db(&h.home);
        (
            stub.hits(),
            query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_completed'",
                &[&run_id],
            ),
            query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'tool_call_completed'",
                &[&run_id],
            ),
            query_one(
                &db,
                "SELECT content_json FROM messages WHERE run_id = ?1",
                &[&run_id],
            ),
        )
    };
    assert_eq!(hits, 2);
    assert_eq!(model_calls.as_deref(), Some("2"));
    assert_eq!(tool_calls.as_deref(), Some("1"));
    let finals = finals.expect("final exists");

    child.stop().await;
    let child2 = h.start_service().await;
    let addr2 = child2.addr;
    // A fresh process mints a fresh loopback token — read it again.
    let token2 = h.token();

    // Read-only restart: counts identical, zero new executions.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(stub.hits(), hits, "no new provider turn after restart");
    let counts_after = {
        let db = open_runs_db(&h.home);
        (
            query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_completed'",
                &[&run_id],
            ),
            query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'tool_call_completed'",
                &[&run_id],
            ),
            query_one(
                &db,
                "SELECT content_json FROM messages WHERE run_id = ?1",
                &[&run_id],
            ),
        )
    };
    assert_eq!(counts_after.0.as_deref(), Some("2"), "no new model calls");
    assert_eq!(counts_after.1.as_deref(), Some("1"), "no new tool calls");
    assert_eq!(
        counts_after.2.as_deref(),
        Some(finals.as_str()),
        "identical final"
    );

    // The original resource stays accessible through a NEW run (the service
    // is functional post-restart without re-doing the old one).
    let (s2, a2) = submit(addr2, &token2, "sess_local_alpha", "read persist.txt again").await;
    assert_eq!(s2, 200, "{a2}");
    let run2 = a2["runId"].as_str().expect("runId").to_string();
    assert_ne!(run2, run_id);
    wait_run_status(&h.home, &run2, |s| s == "completed", "completed").await;
    {
        let db = open_runs_db(&h.home);
        let run1_models = query_one(
            &db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_completed'",
            &[&run_id],
        );
        assert_eq!(
            run1_models.as_deref(),
            Some("2"),
            "the OLD run's counts never move"
        );
        let run2_final = query_one(
            &db,
            "SELECT content_json FROM messages WHERE run_id = ?1",
            &[&run2],
        )
        .expect("run2 final");
        assert!(run2_final.contains("c05 done"));
    }

    child2.stop().await;
    stub.stop().await;
}

// ── C06: crash after the tool effect, no blind re-dispatch ──────────────────

#[tokio::test]
async fn c06_crash_after_tool_effect_no_blind_redo() {
    let read_args = serde_json::json!({"path": "crash.txt"}).to_string();
    let stub = StubServer::start(Box::new(move |body| {
        match last_tool_result(body).as_deref() {
            None => Step::Sse(tool_call_turn("call_t06_read", "read", &read_args)),
            Some(content) if content.contains(EDIT_MARKER) || content.contains(EDIT_RECEIPT) => {
                // The edit has landed (either its receipt or a verification
                // read proves it); PARK the model's continuation turn forever
                // — the service is mid-run when we SIGKILL it.
                Step::Park { first_bytes: None }
            }
            Some(content) => {
                let edits = serde_json::json!({
                    "path": "crash.txt",
                    "edits": [{
                        "oldText": content,
                        "newText": format!("{content}{EDIT_MARKER}\n")
                    }]
                })
                .to_string();
                Step::Sse(tool_call_turn("call_t06_edit", "edit", &edits))
            }
        }
    }))
    .await;
    let nonce = runtime_nonce("c06");
    let h = Harness::new("c06", &stub.endpoint(), &[]);
    let v1 = format!("key={nonce}\n");
    std::fs::write(h.workspace.join("crash.txt"), &v1).expect("seed crash.txt");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    // Detached submission: the parked continuation turn would hold the
    // synchronous execute response past its request budget — the run itself
    // keeps driving, which is exactly the crash window this case needs.
    let submit_task = submit_bg(
        addr,
        token.clone(),
        "sess_local_alpha".to_string(),
        "process crash.txt".to_string(),
    )
    .await;
    let run_id = latest_run_id(&h.home, "sess_local_alpha").await;

    // Wait until the EXTERNAL effect is observable: the file on disk carries
    // the edit (the tool executed; its effect lives outside the DB).
    let v2 = format!("{v1}{EDIT_MARKER}\n");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if matches!(std::fs::read_to_string(h.workspace.join("crash.txt")), Ok(on_disk) if on_disk == v2)
        {
            break;
        }
        if Instant::now() > deadline {
            panic!("the edit tool never landed its external effect");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The model's continuation turn is parked (hit 3 read, unanswered).
    let deadline = Instant::now() + Duration::from_secs(10);
    while stub.hits() < 3 {
        assert!(
            Instant::now() <= deadline,
            "the parked model turn never arrived"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // SIGKILL: the crash leg — no drain, no cleanup.
    child.kill().await;
    let hits_at_crash = stub.hits();

    // Restart on the same home/config; recovery must settle the interrupted
    // run honestly and must NOT re-execute or re-dispatch anything. A fresh
    // process mints a fresh loopback token — read it again.
    let child2 = h.start_service().await;
    let addr2 = child2.addr;
    let token2 = h.token();
    let settled = wait_run_status(
        &h.home,
        &run_id,
        |s| s != "running" && s != "queued",
        "a settled (non-running) status",
    )
    .await;
    assert_eq!(
        settled, "interrupted_needs_attention",
        "the interrupted run is explained, never silently completed nor faked"
    );
    // Give any (forbidden) re-drive a bounded window to appear, then prove
    // it never did: no new provider turns, no new tool executions.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        stub.hits(),
        hits_at_crash,
        "recovery re-dispatched NOTHING (no blind resend of the parked model turn)"
    );
    // The external effect survives and is NOT masked by a fake failure.
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("crash.txt")).expect("file survives"),
        v2,
        "the committed external effect is preserved"
    );
    {
        let db = open_runs_db(&h.home);
        assert_eq!(
            query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'tool_call_completed'",
                &[&run_id]
            )
            .as_deref(),
            Some("2"),
            "the read+edit that really happened stay recorded"
        );
        assert_eq!(
            query_one(
                &db,
                "SELECT COUNT(*) FROM messages WHERE run_id = ?1",
                &[&run_id]
            )
            .as_deref(),
            Some("0"),
            "no fabricated final message for the crashed run"
        );
    }
    // History remains readable through the authenticated surface.
    let (hs, history) = http_json(
        addr2,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events",
        &token2,
        None,
    )
    .await;
    assert_eq!(hs, 200);
    assert!(
        history["items"].as_array().is_some_and(|i| !i.is_empty()),
        "the crashed run's history stays queryable"
    );
    // The killed service dropped the parked submit connection — observed.
    assert!(
        submit_task.await.expect("submission task").is_none(),
        "the detached submission's transport died with the killed service"
    );

    child2.stop().await;
    stub.stop().await;
}

// ── C07: crash mid-stream keeps an honest terminal ──────────────────────────

#[tokio::test]
async fn c07_crash_midstream_honest_terminal() {
    let delta_frame = serde_json::json!({
        "id": "chatcmpl-t07",
        "model": "stub-model-t08",
        "choices": [{
            "index": 0,
            "finish_reason": null,
            "delta": {"role": "assistant", "content": "partial thinking before the cut"}
        }]
    });
    let first_bytes = format!("data: {delta_frame}\n\n");
    let first_for_stub = first_bytes.clone();
    let stub = StubServer::start(Box::new(move |_body| Step::Park {
        first_bytes: Some(first_for_stub.clone()),
    }))
    .await;
    let h = Harness::new("c07", &stub.endpoint(), &[]);
    std::fs::write(h.workspace.join("note.txt"), "c07\n").expect("seed note");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    // Detached submission: the stalled stream (first delta, then nothing)
    // holds the synchronous execute response indefinitely.
    let submit_task = submit_bg(
        addr,
        token.clone(),
        "sess_local_alpha".to_string(),
        "stream me an answer".to_string(),
    )
    .await;
    let run_id = latest_run_id(&h.home, "sess_local_alpha").await;

    // Wait until the first delta is durably observable, then crash.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let db = open_runs_db(&h.home);
        let deltas = query_one(
            &db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_delta'",
            &[&run_id],
        )
        .unwrap_or_default();
        let segments = query_one(
            &db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'assistant_segment_delta'",
            &[&run_id],
        )
        .unwrap_or_default();
        if deltas != "0" || segments != "0" {
            break;
        }
        if Instant::now() > deadline {
            panic!("the partial stream never became observable");
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let hits_at_crash = stub.hits();
    child.kill().await;

    let child2 = h.start_service().await;
    let addr2 = child2.addr;
    let token2 = h.token();
    let settled = wait_run_status(
        &h.home,
        &run_id,
        |s| s != "running" && s != "queued",
        "a settled status",
    )
    .await;
    assert_eq!(
        settled, "interrupted_needs_attention",
        "a mid-stream crash is an explained interruption — never a fake success"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        stub.hits(),
        hits_at_crash,
        "no re-request of the dead stream"
    );
    // The service DROPPED the parked connection (the stub observed it).
    let deadline = Instant::now() + Duration::from_secs(10);
    while stub.dropped() == 0 {
        assert!(
            Instant::now() <= deadline,
            "the killed service never dropped the parked stream connection"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    {
        let db = open_runs_db(&h.home);
        assert_eq!(
            query_one(
                &db,
                "SELECT COUNT(*) FROM messages WHERE run_id = ?1",
                &[&run_id]
            )
            .as_deref(),
            Some("0"),
            "no fabricated final message"
        );
        assert_eq!(
            query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'final_message_committed'",
                &[&run_id]
            )
            .as_deref(),
            Some("0"),
            "no fabricated final event"
        );
        // The partial that DID arrive stays recorded — not masked.
        let deltas = query_one(
            &db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_delta'",
            &[&run_id],
        )
        .unwrap_or_default();
        let segments = query_one(
            &db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'assistant_segment_delta'",
            &[&run_id],
        )
        .unwrap_or_default();
        assert!(
            deltas != "0" || segments != "0",
            "the delivered partial remains in the journal"
        );
    }
    let (hs, history) = http_json(
        addr2,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events",
        &token2,
        None,
    )
    .await;
    assert_eq!(hs, 200, "history surface healthy after the crash");
    assert!(history["items"].as_array().is_some_and(|i| !i.is_empty()));
    assert!(
        submit_task.await.expect("submission task").is_none(),
        "the detached submission's transport died with the killed service"
    );

    child2.stop().await;
    stub.stop().await;
}

// ── C08: two sessions, shared external callId, isolated everything ──────────

#[tokio::test]
async fn c08_two_sessions_shared_callid_isolated() {
    // One router for both sessions: the USER TEXT selects the file (a real
    // model reads the instruction); every session gets the SAME external
    // call id "call_shared_8" (the collision this case exists to prove safe).
    let eta_args = Arc::new(serde_json::json!({"path": "eta.txt"}).to_string());
    let zeta_args = Arc::new(serde_json::json!({"path": "zeta.txt"}).to_string());
    let stub = StubServer::start(Box::new(move |body| {
        let args = if user_text(body).contains("file-eta") {
            Arc::clone(&eta_args)
        } else {
            Arc::clone(&zeta_args)
        };
        match last_tool_result(body).as_deref() {
            None => Step::Sse(tool_call_turn("call_shared_8", "read", &args)),
            Some(content) => Step::Sse(final_turn(&format!("session final saw: {content}"))),
        }
    }))
    .await;
    let h = Harness::new("c08", &stub.endpoint(), &[]);
    let nonce_a = runtime_nonce("c08a");
    let nonce_b = runtime_nonce("c08b");
    std::fs::write(h.workspace.join("eta.txt"), format!("ETA={nonce_a}\n")).expect("seed eta");
    std::fs::write(h.workspace.join("zeta.txt"), format!("ZETA={nonce_b}\n")).expect("seed zeta");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    // Session A submits first (detached — the endpoint is synchronous);
    // once its read tool completed, session B starts while A is still
    // mid-run: the runs genuinely interleave at the provider.
    let submit_a = submit_bg(
        addr,
        token.clone(),
        "sess_local_alpha".to_string(),
        "handle file-eta".to_string(),
    )
    .await;
    let run_a = latest_run_id(&h.home, "sess_local_alpha").await;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let db = open_runs_db(&h.home);
        let count = query_one(
            &db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'tool_call_completed'",
            &[&run_a],
        )
        .unwrap_or_default();
        if count == "1" {
            break;
        }
        assert!(
            Instant::now() <= deadline,
            "session A's read never completed"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let submit_b = submit_bg(
        addr,
        token.clone(),
        "sess_local_beta".to_string(),
        "handle file-zeta".to_string(),
    )
    .await;
    let run_b = latest_run_id(&h.home, "sess_local_beta").await;

    wait_run_status(&h.home, &run_a, |s| s == "completed", "completed").await;
    wait_run_status(&h.home, &run_b, |s| s == "completed", "completed").await;
    // Both detached submissions completed with real 200 acceptances.
    for (name, handle) in [("alpha", submit_a), ("beta", submit_b)] {
        let response = handle.await.expect("submission task");
        assert_eq!(
            response.expect("transport survived").0,
            200,
            "session {name} submission accepted"
        );
    }

    let requests = stub.requests();
    assert!(requests.len() >= 4, "both sessions ran real provider turns");
    let mut saw_eta_nonce_on_a = false;
    let mut saw_zeta_nonce_on_b = false;
    for (_, _, body) in &requests {
        let session_of = if body["messages"]
            .as_array()
            .map(|m| {
                m.iter().any(|x| {
                    x["role"] == "user"
                        && x["content"]
                            .as_str()
                            .is_some_and(|c| c.contains("file-eta"))
                })
            })
            .unwrap_or(false)
        {
            "a"
        } else {
            "b"
        };
        for message in body["messages"].as_array().unwrap_or(&Vec::new()) {
            if message["role"] != "tool" {
                continue;
            }
            let content = message["content"].as_str().unwrap_or_default();
            if content.contains(&nonce_a) {
                assert_eq!(session_of, "a", "eta's nonce crossed into session b");
                saw_eta_nonce_on_a = true;
            }
            if content.contains(&nonce_b) {
                assert_eq!(session_of, "b", "zeta's nonce crossed into session a");
                saw_zeta_nonce_on_b = true;
            }
        }
    }
    assert!(saw_eta_nonce_on_a, "session A observed its own file");
    assert!(saw_zeta_nonce_on_b, "session B observed its own file");

    // The same external call id never fused the two sessions' state.
    {
        let db = open_runs_db(&h.home);
        let session_of_run = |run: &str| {
            query_one(&db, "SELECT session_id FROM runs WHERE run_id = ?1", &[run])
                .expect("session of run")
        };
        assert_eq!(session_of_run(&run_a), "sess_local_alpha");
        assert_eq!(session_of_run(&run_b), "sess_local_beta");
        let final_a = query_one(
            &db,
            "SELECT content_json FROM messages WHERE run_id = ?1",
            &[&run_a],
        )
        .expect("final a");
        let final_b = query_one(
            &db,
            "SELECT content_json FROM messages WHERE run_id = ?1",
            &[&run_b],
        )
        .expect("final b");
        assert!(
            final_a.contains(&nonce_a) && !final_a.contains(&nonce_b),
            "{final_a}"
        );
        assert!(
            final_b.contains(&nonce_b) && !final_b.contains(&nonce_a),
            "{final_b}"
        );
    }
    // History surfaces are per-session and do not cross.
    for (session, expect, other) in [
        ("sess_local_alpha", &nonce_a, &nonce_b),
        ("sess_local_beta", &nonce_b, &nonce_a),
    ] {
        let (hs, page) = http_json(
            addr,
            "GET",
            &format!("/lingxi/v1/sessions/{session}/events"),
            &token,
            None,
        )
        .await;
        assert_eq!(hs, 200);
        let text = page.to_string();
        assert!(
            text.contains(expect),
            "{session} history lost its own final"
        );
        assert!(
            !text.contains(other),
            "{session} history leaked the other session"
        );
    }

    child.stop().await;
    stub.stop().await;
}

// ── C09: live event page vs persisted projection ────────────────────────────

#[tokio::test]
async fn c09_live_stream_and_history_projection_agree() {
    let release = Arc::new(AtomicBool::new(false));
    let release_for_stub = Arc::clone(&release);
    let delta_a = "LIVE-FIRST-DELTA t08 c09 ";
    let delta_b = "live-second-delta then the terminal.";
    let first = {
        let frame = serde_json::json!({
            "id": "chatcmpl-t09",
            "model": "stub-model-t08",
            "choices": [{"index": 0, "finish_reason": null,
                "delta": {"role": "assistant", "content": delta_a}}]
        });
        format!("data: {frame}\n\n")
    };
    let rest = {
        let mut body = String::new();
        let mid = serde_json::json!({
            "id": "chatcmpl-t09",
            "choices": [{"index": 0, "finish_reason": null,
                "delta": {"content": delta_b}}]
        });
        let stop = serde_json::json!({
            "id": "chatcmpl-t09",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        });
        let usage = serde_json::json!({
            "id": "chatcmpl-t09",
            "choices": [],
            "usage": {"prompt_tokens": 20, "completion_tokens": 9}
        });
        body.push_str(&format!("data: {mid}\n\n"));
        body.push_str(&format!("data: {stop}\n\n"));
        body.push_str(&format!("data: {usage}\n\n"));
        body.push_str("data: [DONE]\n\n");
        body
    };
    let stub = StubServer::start(Box::new(move |_body| Step::SseGated {
        first: first.clone(),
        rest: rest.clone(),
        release: Arc::clone(&release_for_stub),
    }))
    .await;
    let h = Harness::new("c09", &stub.endpoint(), &[]);
    std::fs::write(h.workspace.join("note.txt"), "c09\n").expect("seed note");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    // Detached submission: the gated stream holds the synchronous execute
    // response open until the barrier flips — the LIVE leg below must observe
    // the first delta BEFORE that happens.
    let submit_task = submit_bg(
        addr,
        token.clone(),
        "sess_local_alpha".to_string(),
        "stream an answer".to_string(),
    )
    .await;
    let run_id = latest_run_id(&h.home, "sess_local_alpha").await;

    // LIVE leg: the first delta must be readable from the event page while
    // the model's HTTP response is STILL held open (release not yet flipped).
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (hs, page) = http_json(
            addr,
            "GET",
            "/lingxi/v1/sessions/sess_local_alpha/events",
            &token,
            None,
        )
        .await;
        assert_eq!(hs, 200);
        let text = page.to_string();
        if text.contains(delta_a) {
            assert!(
                !text.contains(delta_b),
                "the terminal delta must not precede the barrier"
            );
            break;
        }
        assert!(
            Instant::now() <= deadline,
            "the first live delta never arrived"
        );
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert!(!release.load(Ordering::SeqCst), "the stream is still open");
    // Disconnect (stop paging), then let the terminal frame through.
    release.store(true, Ordering::SeqCst);
    wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    // The synchronous submission now completes within its request budget
    // (the hold was well under the 30 s budget) and returns a real 200.
    let submit_response = submit_task.await.expect("submission task");
    assert_eq!(
        submit_response.expect("transport survived").0,
        200,
        "the submission completed after the barrier released"
    );

    // HISTORY leg: re-fetch the full page and the durable rows; the same
    // source must project identically with no duplicated concatenation.
    let (hs, page_full) = http_json(
        addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events",
        &token,
        None,
    )
    .await;
    assert_eq!(hs, 200);
    let page_text = page_full.to_string();
    assert!(page_text.contains(delta_a) && page_text.contains(delta_b));
    let final_events: Vec<serde_json::Value> = page_full["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|e| e["payload"]["type"] == "final_message_committed")
        .cloned()
        .collect();
    assert_eq!(final_events.len(), 1, "exactly one final");
    let final_text = serde_json::to_string(&final_events[0]["payload"]["message"]).unwrap();
    // The final message is EXACTLY the normalized whole (delta_a + delta_b
    // with no other text): no loss, no second concatenation, no re-emitted
    // partial spliced in twice. `matches` on the joined text pins the count.
    let joined = format!("{}{}", delta_a, delta_b);
    assert_eq!(
        final_text.matches(delta_a.trim()).count(),
        1,
        "the first delta appears exactly once in the final: {final_text}"
    );
    assert_eq!(
        final_text.matches(delta_b.trim()).count(),
        1,
        "the terminal delta appears exactly once in the final: {final_text}"
    );
    assert!(
        final_text.contains(joined.trim()),
        "the final message is the normalized whole in order: {final_text} (expected {joined})"
    );
    // DB parity: every page item type exists as a durable row for this run.
    {
        let db = open_runs_db(&h.home);
        let page_types: Vec<&str> = page_full["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter_map(|e| e["payload"]["type"].as_str())
            .collect();
        let mut missing = Vec::new();
        for ty in &page_types {
            let count = query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = ?2",
                &[&run_id, ty],
            )
            .unwrap_or_default();
            if count == "0" {
                missing.push((*ty).to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "page types absent from the journal: {missing:?}"
        );
    }

    child.stop().await;
    stub.stop().await;
}

// ── C12: repeated mixed cycles stay bounded ─────────────────────────────────

#[tokio::test]
async fn c12_repeated_cycles_stay_bounded() {
    // Pre-registered bounds (docs/rust-tauri/R05/R05_PERFORMANCE_RESULTS.json
    // freezes them BEFORE this run): RSS < 400 MiB and growth < 150 MiB
    // across the cycles, open FDs < 400, rotated log files <= 3.
    let mode = Arc::new(Mutex::new("ok".to_string()));
    let mode_for_stub = Arc::clone(&mode);
    let read_args = serde_json::json!({"path": "loop.txt"}).to_string();
    let stub = StubServer::start(Box::new(move |body| {
        match mode_for_stub.lock().expect("mode").as_str() {
            "fail" => Step::Error(
                500,
                r#"{"error":{"message":"injected provider failure"}}"#.to_string(),
            ),
            _ => {
                if last_tool_result(body).is_some() {
                    Step::Sse(final_turn("cycle done"))
                } else {
                    Step::Sse(tool_call_turn("call_t12", "read", &read_args))
                }
            }
        }
    }))
    .await;
    let h = Harness::new(
        "c12",
        &stub.endpoint(),
        &["--log-max-bytes", "65536", "--log-max-files", "3"],
    );
    std::fs::write(h.workspace.join("loop.txt"), "cycle body\n").expect("seed loop");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;
    let pid = child.pid();

    async fn rss_of(pid: i32) -> u64 {
        // R05 RR1 F27 (CL-06): the sampler must VALIDATE its sample — the
        // exit status and the numeric shape — a failed read is a loud
        // panic (UNKNOWN), never a fake 0.
        let pid_text = pid.to_string();
        let out = tokio::process::Command::new("ps")
            .arg("-o")
            .arg("rss=")
            .arg("-p")
            .arg(&pid_text)
            .output()
            .await
            .expect("ps runs");
        assert!(
            out.status.success(),
            "rss sampling of pid {pid} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert!(
            !text.is_empty(),
            "rss sampling of pid {pid} produced no row"
        );
        text.parse::<u64>()
            .unwrap_or_else(|e| panic!("non-numeric rss {text:?}: {e}"))
    }
    async fn fds_of(pid: i32) -> usize {
        // R05 RR1 F27 (CL-06): the pre-fix sampler ran `lsof -p PID`
        // (default column format, first column COMMAND) and counted lines
        // STARTING WITH A DIGIT — every real row was filtered out and the
        // FD bound could hold vacuously at 0. The machine-readable form
        // (`-F fn`), the exit-status check and the `p<digits>` identity
        // check make the count real; a failed sample is a loud panic, and
        // the sampler's own positive/negative controls live in
        // r05_t08_resources.rs (f27_sampler_controls_*).
        let pid_text = pid.to_string();
        let out = tokio::process::Command::new("lsof")
            .arg("-p")
            .arg(&pid_text)
            .arg("-F")
            .arg("fn")
            .output()
            .await
            .expect("lsof runs");
        assert!(
            out.status.success(),
            "fd sampling of pid {pid} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        let text = String::from_utf8_lossy(&out.stdout);
        let mut saw_pid_record = false;
        let mut fds = 0usize;
        for line in text.lines() {
            if let Some(recorded) = line.strip_prefix('p') {
                assert_eq!(
                    recorded, pid_text,
                    "lsof reported pid {recorded}, requested {pid_text}"
                );
                saw_pid_record = true;
            } else if line.starts_with('f') && line[1..].chars().all(|c| c.is_ascii_digit()) {
                fds += 1;
            }
        }
        assert!(
            saw_pid_record,
            "lsof output carried no p<{pid_text}> identity record"
        );
        fds
    }

    // Warmup + baseline sample AFTER two completed cycles.
    for i in 0..2 {
        let (s, a) = submit(addr, &token, "sess_local_alpha", &format!("cycle {i}")).await;
        assert_eq!(s, 200, "{a}");
        let run_id = a["runId"].as_str().expect("runId").to_string();
        wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    }
    let rss_start = rss_of(pid).await;
    let fds_start = fds_of(pid).await;

    // The mixed cycles: successes, provider failures, replays — bounded
    // counts, honest outcomes, no unbounded growth.
    let mut failure_runs = 0usize;
    for cycle in 0..12u32 {
        let label = format!("ok {cycle}");
        match cycle % 3 {
            0 | 1 => {
                let (s, a) = submit(addr, &token, "sess_local_alpha", &label).await;
                assert_eq!(s, 200, "{a}");
                let run_id = a["runId"].as_str().expect("runId").to_string();
                let st = wait_run_status(&h.home, &run_id, |st| st != "running", "settled").await;
                assert_eq!(st, "completed", "cycle {cycle} settled honestly: {st}");
            }
            _ => {
                *mode.lock().expect("mode") = "fail".to_string();
                let fail_label = format!("fail {cycle}");
                let (s, a) = submit(addr, &token, "sess_local_alpha", &fail_label).await;
                assert_eq!(s, 200, "{a}");
                let run_id = a["runId"].as_str().expect("runId").to_string();
                let st = wait_run_status(&h.home, &run_id, |st| st != "running", "settled").await;
                assert_eq!(st, "failed", "the provider failure settles honestly: {st}");
                failure_runs += 1;
                *mode.lock().expect("mode") = "ok".to_string();
            }
        }
        // Idempotent replay every other cycle: zero re-execution pressure.
        if cycle % 2 == 0 {
            let (s, a) = submit_with_request_id(
                addr,
                &token,
                "sess_local_alpha",
                &label,
                &format!("req-t12-{cycle}"),
            )
            .await;
            assert_eq!(s, 200, "{a}");
            let run_id = a["runId"].as_str().expect("runId").to_string();
            wait_run_status(&h.home, &run_id, |st| st == "completed", "completed").await;
            let hits = stub.hits();
            let (s2, a2) = submit_with_request_id(
                addr,
                &token,
                "sess_local_alpha",
                &label,
                &format!("req-t12-{cycle}"),
            )
            .await;
            assert_eq!(s2, 200, "{a2}");
            assert_eq!(a2["replayed"], true);
            assert_eq!(stub.hits(), hits, "replays add zero provider load");
        }
    }
    assert!(failure_runs >= 4, "at least four real failure cycles ran");

    let rss_end = rss_of(pid).await;
    let fds_end = fds_of(pid).await;
    assert!(
        rss_end < 400 * 1024,
        "RSS {rss_end} KiB exceeds the pre-registered 400 MiB bound"
    );
    assert!(
        rss_end.saturating_sub(rss_start) < 150 * 1024,
        "RSS grew {} KiB across the cycles (bound 150 MiB)",
        rss_end.saturating_sub(rss_start)
    );
    assert!(fds_end < 400, "open FDs {fds_end} exceed the bound");
    assert!(
        fds_end <= fds_start + 64,
        "open FDs grew by {} across the cycles (bound +64)",
        fds_end.saturating_sub(fds_start)
    );
    let log_dir = std::fs::canonicalize(&h.home)
        .expect("canonical home")
        .join("lingxi-service")
        .join("logs");
    let log_files = std::fs::read_dir(&log_dir)
        .expect("logs dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .count();
    assert!(
        log_files <= 3,
        "rotated log files {log_files} exceed the configured --log-max-files 3"
    );

    child.stop().await;
    stub.stop().await;
}

// ── R05 RR1 F13 (WP-T04): the normalized final projection on the REAL
// binary — authenticated HTTP history before AND after a graceful
// SIGTERM/restart, plus the on-disk row, all carry the SAME normalized
// message (the CL-02 反例 A shape: fenced literal think tag + standalone
// think block + mood block + visible answer; the frozen candidate
// persisted one raw Text block with every tag intact). ─────────────────

#[tokio::test]
async fn rr1_f13_normalized_final_survives_restart_on_the_real_binary() {
    let raw = "literal:\n```\n<think>keep quoted</think>\n```\n\
               <think>PRIVATE_THINK_T08</think><mood>PRIVATE_MOOD_T08</mood>VISIBLE_FINAL_T08";
    let stub = StubServer::start(Box::new(move |_body| Step::Sse(final_turn(raw)))).await;
    let h = Harness::new("rr1f13", &stub.endpoint(), &[]);
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (status, accepted) = submit(
        addr,
        &token,
        "sess_local_alpha",
        "reply with the model response",
    )
    .await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    // The history surface (authenticated HTTP) carries the normalized final.
    let (before_status, page_before) = http_json(
        addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events",
        &token,
        None,
    )
    .await;
    assert_eq!(before_status, 200);
    let assert_normalized = |page: &serde_json::Value, label: &str| {
        let finals: Vec<&serde_json::Value> = page["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter(|e| e["payload"]["type"] == "final_message_committed")
            .collect();
        assert_eq!(finals.len(), 1, "exactly one final message ({label})");
        let body = serde_json::to_string(&finals[0]["payload"]["message"]).unwrap();
        assert!(
            body.contains("VISIBLE_FINAL_T08") && body.contains("<think>keep quoted</think>"),
            "the visible answer and the fenced literal persist ({label}): {body}"
        );
        assert!(
            !body.contains("PRIVATE_MOOD_T08") && !body.contains("<mood>"),
            "mood content never returns as body ({label}): {body}"
        );
        assert!(
            body.contains("PRIVATE_THINK_T08") && body.contains("\"reasoning\""),
            "the think block persists as a reasoning block ({label}): {body}"
        );
        assert!(
            !body.contains("<think>PRIVATE_THINK_T08"),
            "the raw think tag never persists as body text ({label}): {body}"
        );
    };
    assert_normalized(&page_before, "before restart");

    // The live deltas on the same surface: think → reasoning phase, mood
    // stripped, text unresolved (F12) — one source with the final message.
    let deltas: Vec<&serde_json::Value> = page_before["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|e| e["payload"]["type"] == "model_call_delta")
        .collect();
    assert!(!deltas.is_empty());
    let reasoning: String = deltas
        .iter()
        .filter(|d| d["payload"]["phase"] == "reasoning")
        .filter_map(|d| d["payload"]["delta"].as_str())
        .collect();
    assert!(
        reasoning.contains("PRIVATE_THINK_T08"),
        "the think block streamed as live reasoning: {reasoning}"
    );
    for d in &deltas {
        assert!(
            d["payload"]["phase"] != "final_answer",
            "a live delta is never pre-classified final_answer: {}",
            serde_json::to_string(d).unwrap()
        );
    }
    let all_delta_text: String = deltas
        .iter()
        .filter_map(|d| d["payload"]["delta"].as_str())
        .collect();
    assert!(!all_delta_text.contains("PRIVATE_MOOD_T08"));

    // The on-disk row is the SAME normalized message.
    let db = open_runs_db(&h.home);
    let final_row = query_one(
        &db,
        "SELECT content_json FROM messages WHERE run_id = ?1",
        &[&run_id],
    )
    .expect("final message row");
    assert!(
        final_row.contains("VISIBLE_FINAL_T08") && final_row.contains("\"reasoning\""),
        "the DB row is the normalized projection: {final_row}"
    );
    assert!(
        !final_row.contains("PRIVATE_MOOD_T08") && !final_row.contains("<think>PRIVATE"),
        "no raw tags in the DB row: {final_row}"
    );
    drop(db);

    // Graceful SIGTERM → a NEW process reads back the IDENTICAL normalized
    // history (nothing re-executed: the stub count stays 1).
    let hits_before = stub.hits();
    child.stop().await;
    let child2 = h.start_service().await;
    let token2 = h.token();
    let (after_status, page_after) = http_json(
        child2.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events",
        &token2,
        None,
    )
    .await;
    assert_eq!(after_status, 200);
    assert_eq!(
        page_after["items"].as_array().expect("items").len(),
        page_before["items"].as_array().expect("items").len(),
        "history after restart is IDENTICAL"
    );
    assert_normalized(&page_after, "after restart");
    assert_eq!(stub.hits(), hits_before, "zero new provider turns");

    child2.stop().await;
    stub.stop().await;
}
