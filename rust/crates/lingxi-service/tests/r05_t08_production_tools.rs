//! R05 RR1 F24 (WP-T08): the FORMAL composition root must register the four
//! basic tools (read/write/edit + exec_command) and ONE config-controlled
//! single-operation worker — the CL-04/CL-05 counterexamples: the frozen
//! candidate's production wire carried only edit/read/write
//! (`process tools stay unregistered`), and `register_worker_tool` had no
//! production call site, so T08-C11 could only be "proven" by tests that
//! hand-injected ServiceDeps.
//!
//! Every leg here drives the REAL `lingxi-service` binary (real argv, real
//! `--config` file, authenticated HTTP). The far end of the model wire is a
//! loopback behavioral stub; the worker is a REAL child process (the
//! `r04_t07_fixture` binary, argv declared by the config under test — the
//! config-controlled registration path is the production code under test,
//! the fixture is just the controlled single-op worker executable that the
//! operator-declared argv points at). Nothing injects ServiceDeps or
//! replaces RunSupervisor/ToolGateway/Storage.
//!
//! Legs:
//! - f24 wire: the declared tool list on the first model request carries
//!   read/write/edit/exec_command (+write_stdin, the R04 terminal pair).
//! - f24 exec closed loop: model → exec_command(argv) → REAL subprocess →
//!   tool result (runtime nonce) → next model request → final.
//! - f24 exec permission boundary: workdir outside the authorized
//!   workspace is refused before any process runs.
//! - f24 worker chain (T08-C11): global model permit = 1; main model HTTP →
//!   worker tool → REAL child process → host callback through the REAL
//!   gateway (aux slot HTTP) → worker result → main model final; parent/
//!   child usage JOINs in the ledger; no deadlock under permit=1.
//! - f24 worker secret boundary: no credential material reaches the worker
//!   child environment.
//! - f24 worker reclamation: SIGTERM while the callback is parked reaps the
//!   worker child (no orphan survives the service).

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
    Sse(String),
    /// Answer the first bytes then HOLD until the gate flips (the parked
    /// callback of the reclamation leg).
    SseGated {
        first: String,
        release: Arc<AtomicBool>,
    },
}

type Router = Box<dyn Fn(&serde_json::Value) -> Step + Send + Sync>;

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    #[allow(dead_code)]
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
                    Step::SseGated { first, release } => {
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
                        let deadline = Instant::now() + Duration::from_secs(180);
                        while !release.load(Ordering::SeqCst) {
                            if Instant::now() > deadline {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        let _ = socket
                            .write_all(sse_final("LATE-CALLBACK-NEVER-COMPLETES").as_bytes())
                            .await;
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

// ── openai-completions SSE builders ─────────────────────────────────────────

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
                "model": "stub-model-f24",
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

fn sse_final(text: &str) -> String {
    sse_of(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                "model": "stub-model-f24",
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

/// The content of the LAST `role: "tool"` message of a provider request.
fn last_tool_result(body: &serde_json::Value) -> Option<String> {
    body["messages"]
        .as_array()?
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .and_then(|m| m["content"].as_str())
        .map(str::to_string)
}

// ── child process harness (the REAL binary, async spawn/read) ───────────────

struct ServiceChild {
    child: tokio::process::Child,
    addr: SocketAddr,
    #[allow(dead_code)]
    stderr_lines: Arc<Mutex<Vec<String>>>,
    _drain: tokio::task::JoinHandle<()>,
    _stderr: tokio::task::JoinHandle<()>,
}

impl ServiceChild {
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
                panic!(
                    "the real binary exited before readiness\nstderr so far:\n{stderr}\n(the config-declared worker registration path refused startup — see above)"
                );
            }
            let trimmed = line.trim_end().to_string();
            if trimmed.starts_with("LINGXI_SERVICE_READY ") {
                break trimmed;
            }
        };
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
            stderr_lines,
            _drain: drain,
            _stderr: stderr_task,
        }
    }

    fn pid(&self) -> i32 {
        self.child.id().expect("child pid") as i32
    }

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
}

impl Drop for ServiceChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.child.start_kill().expect("cleanup kill");
        }
    }
}

// ── authenticated HTTP client ────────────────────────────────────────────────

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
        "lingxi-r05f24-{tag}-{}-{}",
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

fn runtime_nonce(tag: &str) -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!(
        "NONCE-F24-{tag}-{}-{nanos}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

fn fixture_exe() -> &'static str {
    env!("CARGO_BIN_EXE_r04_t07_fixture")
}

struct Harness {
    home: PathBuf,
    workspace: PathBuf,
    #[allow(dead_code)]
    config_path: PathBuf,
    argv_tail: Vec<String>,
}

impl Harness {
    /// home/workspace/config around a stub endpoint with BOTH the chat route
    /// and the `summarize` auxiliary slot (the worker callback's route) bound
    /// to the same stub; `workers_json` (when given) is embedded verbatim as
    /// the config's `workers` section.
    fn new(
        tag: &str,
        stub_endpoint: &str,
        workers_json: Option<&str>,
        extra_argv: &[&str],
    ) -> Self {
        let home = unique_dir(&format!("{tag}-home"));
        let workspace = unique_dir(&format!("{tag}-ws"));
        let config_root = unique_dir(&format!("{tag}-cfg"));
        let config_path = config_root.join("service.json");
        let workers = workers_json
            .map(|w| format!(", \"workers\": {w}"))
            .unwrap_or_default();
        std::fs::write(
            &config_path,
            format!(
                r#"{{"home": {}, "workspace": {}, "providers": {{
                    "main": {{
                        "protocol": "openai-completions",
                        "endpoint": "{}",
                        "auth": {{"kind": "apiKey", "apiKey": "sk-test-f24"}}
                    }},
                    "aux": {{
                        "protocol": "openai-completions",
                        "endpoint": "{}",
                        "auth": {{"kind": "apiKey", "apiKey": "sk-aux-f24"}}
                    }}
                }},
                "models": {{
                    "chat": {{"provider": "main", "model": "stub-model-f24", "capabilities": {{"tools": true}}}},
                    "summarize": {{"provider": "aux", "model": "summarize-model-f24"}}
                }}{workers}}}"#,
                serde_json::to_string(&home.to_string_lossy()).expect("home json"),
                serde_json::to_string(&workspace.to_string_lossy()).expect("ws json"),
                stub_endpoint,
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

async fn wait_run_status(
    home: &Path,
    run_id: &str,
    mut wanted: impl FnMut(&str) -> bool,
    what: &str,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let db = open_runs_db(home);
        let status = query_one(&db, "SELECT status FROM runs WHERE run_id = ?1", &[run_id])
            .unwrap_or_default();
        if wanted(&status) {
            return status;
        }
        if Instant::now() > deadline {
            let db = open_runs_db(home);
            let reason = query_one(
                &db,
                "SELECT terminal_reason FROM runs WHERE run_id = ?1",
                &[run_id],
            )
            .unwrap_or_default();
            let last_events = query_one(
                &db,
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
                &[run_id],
            )
            .unwrap_or_default();
            let mut stmt = db
                .prepare(
                    "SELECT event_type, payload_json FROM key_events WHERE run_id = ?1 \
                     ORDER BY event_id DESC LIMIT 3",
                )
                .expect("diag statement");
            let tails: Vec<String> = stmt
                .query_map(rusqlite::params![run_id], |row| {
                    Ok(format!(
                        "{}: {}",
                        row.get::<_, String>(0).unwrap_or_default(),
                        row.get::<_, String>(1).unwrap_or_default()
                    ))
                })
                .expect("diag query")
                .filter_map(Result::ok)
                .collect();
            drop(stmt);
            panic!(
                "run {run_id} never reached {what}; last status: {status}, terminal_reason: \
                 {reason}, key events: {last_events}, latest: {tails:#?}"
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Fires a submission WITHOUT awaiting its response (a parked provider
/// stream would otherwise hold this task for the whole request budget).
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
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                        ) =>
                    {
                        break;
                    }
                    Err(err) => panic!("bg read response: {err}"),
                }
            }
            raw
        })
        .await;
        let Ok(raw) = exchanged else {
            return None;
        };
        let text = String::from_utf8_lossy(&raw);
        let (head, body) = text.split_once("\r\n\r\n")?;
        let status = head
            .split_whitespace()
            .nth(1)
            .expect("status")
            .parse::<u16>()
            .expect("numeric status");
        Some((
            status,
            serde_json::from_str(if body.is_empty() { "null" } else { body })
                .expect("JSON response body"),
        ))
    })
}

/// Counts live processes whose FULL command line contains `needle`
/// (`pgrep -f`), excluding this test process itself.
fn count_processes(needle: &str) -> usize {
    let out = std::process::Command::new("pgrep")
        .arg("-fl")
        .arg(needle)
        .output()
        .expect("pgrep runs");
    assert!(
        out.status.success() || out.status.code() == Some(1),
        "pgrep itself failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| {
            // pgrep -fl prints "PID command…"; the harness binary (this test
            // process, whose argv table includes the fixture path via env!
            // expansion is NOT in the command line — but the needle is a
            // runtime nonce, only the real child carries it).
            line.contains(needle)
        })
        .count()
}

// ── the wire leg: the declared tool list of the production plane ────────────

#[tokio::test]
async fn f24_production_wire_declares_the_four_tools() {
    let stub = StubServer::start(Box::new(|_body| Step::Sse(sse_final("wire probe done")))).await;
    let h = Harness::new("wire", &stub.endpoint(), None, &[]);
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (status, accepted) = submit(addr, &token, "sess_local_alpha", "any").await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    let requests = stub.requests();
    assert_eq!(stub.hits(), 1, "exactly one provider turn");
    let tools: Vec<String> = requests[0].2["tools"]
        .as_array()
        .expect("the request declares tools")
        .iter()
        .map(|t| {
            t["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    for required in ["read", "write", "edit", "exec_command", "write_stdin"] {
        assert!(
            tools.contains(&required.to_string()),
            "the production wire must declare {required}; got {tools:?} \
             (F24/CL-05: process tools stay unregistered in the frozen candidate)"
        );
    }

    child.stop().await;
    stub.stop().await;
}

// ── the exec closed loop ─────────────────────────────────────────────────────

#[tokio::test]
async fn f24_exec_command_runs_a_real_subprocess_end_to_end() {
    let nonce = runtime_nonce("exec");
    let exec_args = serde_json::json!({
        "argv": ["/bin/echo", "-n", &nonce]
    })
    .to_string();
    let stub = StubServer::start(Box::new(move |body| {
        if last_tool_result(body).is_some() {
            Step::Sse(sse_final("exec chain final answer"))
        } else {
            Step::Sse(tool_call_turn("call_f24_exec", "exec_command", &exec_args))
        }
    }))
    .await;
    let h = Harness::new("exec", &stub.endpoint(), None, &[]);
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (status, accepted) = submit(addr, &token, "sess_local_alpha", "run the echo").await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    let requests = stub.requests();
    assert_eq!(stub.hits(), 2, "tool round + final round");
    let tool_result =
        last_tool_result(&requests[1].2).expect("the second request carries the exec tool result");
    assert!(
        tool_result.contains(&nonce),
        "the REAL subprocess output rode the wire: {tool_result}"
    );

    // The single terminal is honest and the journal saw the tool round.
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
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'tool_call_completed'",
            &[&run_id]
        )
        .as_deref(),
        Some("1")
    );

    child.stop().await;
    stub.stop().await;
}

// ── the exec permission boundary ─────────────────────────────────────────────

#[tokio::test]
async fn f24_exec_command_workdir_outside_the_workspace_is_refused() {
    let outside = unique_dir("exec-outside");
    let exec_args = serde_json::json!({
        "argv": ["/bin/echo", "SHOULD-NEVER-RUN"],
        "workdir": outside.to_string_lossy()
    })
    .to_string();
    let stub = StubServer::start(Box::new(move |body| {
        if last_tool_result(body).is_some() {
            Step::Sse(sse_final("refusal chain final"))
        } else {
            Step::Sse(tool_call_turn(
                "call_f24_refuse",
                "exec_command",
                &exec_args,
            ))
        }
    }))
    .await;
    let h = Harness::new("execref", &stub.endpoint(), None, &[]);
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (status, accepted) = submit(addr, &token, "sess_local_alpha", "try outside").await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    let requests = stub.requests();
    assert_eq!(stub.hits(), 2);
    let tool_result = last_tool_result(&requests[1].2).expect("tool result on the wire");
    assert!(
        !tool_result.contains("SHOULD-NEVER-RUN"),
        "no refused process output may reach the model: {tool_result}"
    );
    let lowered = tool_result.to_lowercase();
    assert!(
        lowered.contains("forbid")
            || lowered.contains("refus")
            || lowered.contains("denied")
            || lowered.contains("outside")
            || lowered.contains("authoriz"),
        "the refusal names the authorization boundary: {tool_result}"
    );

    child.stop().await;
    stub.stop().await;
}

// ── the worker nested chain (T08-C11) ────────────────────────────────────────

/// The workers-section JSON of the config-declared single-op worker.
/// `mode`/`extra` ride the fixture argv (argv = [fixture, mode, extra?]);
/// the callback purpose = `purpose` (the host-granted purpose whitelist
/// must name it — C09).
fn workers_section(mode: &str, extra: &str, purpose: &str) -> String {
    let mut argv = vec![serde_json::to_string(fixture_exe()).expect("argv json")];
    if !mode.is_empty() {
        argv.push(serde_json::to_string(mode).expect("mode json"));
    }
    if !extra.is_empty() {
        argv.push(serde_json::to_string(extra).expect("extra json"));
    }
    format!(
        r#"{{
            "localName": "probe",
            "op": "probe",
            "description": "F24 config-declared controlled single-op worker",
            "argv": [{}],
            "inputSchema": {{
                "type": "object",
                "properties": {{"input": {{"type": "string"}}}},
                "required": ["input"],
                "additionalProperties": false
            }},
            "pathArgs": ["input"],
            "allowedModelPurposes": [{}]
        }}"#,
        argv.join(", "),
        serde_json::to_string(purpose).expect("purpose json"),
    )
}

#[tokio::test]
async fn f24_worker_nested_chain_under_global_permit_one() {
    let stub = StubServer::start(Box::new(|body| match body["model"].as_str() {
        Some("summarize-model-f24") => Step::Sse(sse_final("callback answer from the aux slot")),
        _ => {
            if last_tool_result(body).is_some() {
                Step::Sse(sse_final("worker chain final answer"))
            } else {
                let arguments = serde_json::json!({"input": "input.txt"}).to_string();
                Step::Sse(tool_call_turn("call_f24_worker", "probe", &arguments))
            }
        }
    }))
    .await;
    let h = Harness::new(
        "worker",
        &stub.endpoint(),
        Some(&workers_section("ask_model", "", "summarize")),
        &["--model-global-permits", "1"],
    );
    std::fs::write(h.workspace.join("input.txt"), "granted worker input\n")
        .expect("seed worker input");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (status, accepted) = submit(
        addr,
        &token,
        "sess_local_alpha",
        "call the worker and summarize",
    )
    .await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    // permit=1 must NOT deadlock the nested chain: the main model's permit
    // is released before the tool round, so the callback can acquire it.
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    // The wire: 2 main turns + exactly 1 callback turn through the REAL
    // gateway (aux-slot route + aux credential).
    let requests = stub.requests();
    assert_eq!(stub.hits(), 3, "main + callback + main-final");
    let callback = &requests[1].2;
    assert_eq!(callback["model"], "summarize-model-f24");
    assert_eq!(requests[1].1.as_deref(), Some("Bearer sk-aux-f24"));
    // The worker's result (with the callback reply) rode the LAST main turn.
    let tool_result = last_tool_result(&requests[2].2).expect("worker tool result on the wire");
    assert!(
        tool_result.contains("callback answer from the aux slot"),
        "the callback reply reached the main model through the worker result: {tool_result}"
    );

    // The ledger JOINs the child callback to the parent tool call (T08-C11).
    let db = open_runs_db(&h.home);
    assert_eq!(
        query_one(
            &db,
            "SELECT COUNT(*) FROM model_call_usage WHERE model = 'summarize-model-f24' \
             AND parent_tool_call_id IS NOT NULL",
            &[]
        )
        .as_deref(),
        Some("1"),
        "the callback's usage row carries the parent tool call JOIN key"
    );
    assert_eq!(
        query_one(
            &db,
            "SELECT COUNT(*) FROM model_call_usage WHERE model = 'stub-model-f24'",
            &[]
        )
        .as_deref(),
        Some("2"),
        "the two main turns are both in the ledger"
    );
    // The final answer is committed once.
    assert_eq!(
        query_one(
            &db,
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            &[&run_id]
        )
        .as_deref(),
        Some("completed.with_final")
    );

    child.stop().await;
    stub.stop().await;
}

// ── the worker secret boundary ───────────────────────────────────────────────

#[tokio::test]
async fn f24_worker_child_environment_carries_no_credential_material() {
    let stub = StubServer::start(Box::new(|body| {
        if last_tool_result(body).is_some() {
            Step::Sse(sse_final("env chain final"))
        } else {
            let arguments = serde_json::json!({"input": "input.txt"}).to_string();
            Step::Sse(tool_call_turn("call_f24_env", "probe", &arguments))
        }
    }))
    .await;
    // The worker executable for this leg is a tiny REAL subprocess that
    // reports which environment variables reached it: the fixture's
    // env_probe mode (reports LINGXI_T07_SECRET_A/B/HOME/PATH).
    let h = Harness::new(
        "wenv",
        &stub.endpoint(),
        Some(&workers_section("env_probe", "", "summarize")),
        &[],
    );
    std::fs::write(h.workspace.join("input.txt"), "env probe input\n").expect("seed input");
    // Plant a synthetic secret in the SERVICE process's environment (the
    // service inherits it; the worker child must not). The guard removes it
    // on every path, panic included.
    struct EnvGuard(&'static str);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }
    let secret_name = "LINGXI_T07_SECRET_A";
    let secret_value = "SYNTHETIC-F24-SECRET-VALUE";
    std::env::set_var(secret_name, secret_value);
    let _guard = EnvGuard("LINGXI_T07_SECRET_A");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;
    let (status, accepted) = submit(addr, &token, "sess_local_alpha", "probe the env").await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    let requests = stub.requests();
    let tool_result = last_tool_result(&requests[1].2).expect("env probe result");
    // The worker saw NO provider credentials…
    assert!(
        !tool_result.contains("sk-test-f24") && !tool_result.contains("sk-aux-f24"),
        "no API key reached the worker child: {tool_result}"
    );
    // …and NOT the planted host secret: the probe reports the variable's
    // PRESENCE, and it must be `false` (the whitelist is PATH/LANG/… only).
    assert!(
        tool_result.contains("\"LINGXI_T07_SECRET_A\":false"),
        "the planted host secret did not pass through to the worker (probe says \
         LINGXI_T07_SECRET_A must be false): {tool_result}"
    );
    // …while the whitelisted PATH DID reach the child (the probe is real).
    assert!(
        tool_result.contains("\"PATH\":true"),
        "the whitelisted PATH reached the worker child: {tool_result}"
    );
    let _ = (secret_name, secret_value);

    child.stop().await;
    stub.stop().await;
}

// ── the worker reclamation under SIGTERM ─────────────────────────────────────

#[tokio::test]
async fn f24_sigterm_with_parked_callback_reaps_the_worker_child() {
    // The callback purpose is a unique runtime tag: the fixture child's
    // argv carries it (ask_model takes the purpose as its extra argv), so
    // `pgrep -f <tag>` identifies EXACTLY the worker under test.
    let tag = runtime_nonce("reap");
    let release = Arc::new(AtomicBool::new(false));
    let release_for_stub = Arc::clone(&release);
    let stub = StubServer::start(Box::new(move |body| match body["model"].as_str() {
        // The callback answer is PARKED: the worker sits in its callback
        // wait when the SIGTERM arrives.
        Some("summarize-model-f24") => Step::SseGated {
            first: String::new(),
            release: Arc::clone(&release_for_stub),
        },
        _ => {
            let arguments = serde_json::json!({"input": "input.txt"}).to_string();
            Step::Sse(tool_call_turn("call_f24_reap", "probe", &arguments))
        }
    }))
    .await;
    let h = Harness::new(
        "wreap",
        &stub.endpoint(),
        Some(&workers_section("ask_model", &tag, &tag)),
        &["--shutdown-timeout-ms", "8000"],
    );
    std::fs::write(h.workspace.join("input.txt"), "reap probe input\n").expect("seed input");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let bg = submit_bg(
        addr,
        token,
        "sess_local_alpha".to_string(),
        "call the worker".to_string(),
    )
    .await;
    // Wait until the worker child is actually alive in its callback wait.
    let deadline = Instant::now() + Duration::from_secs(30);
    while count_processes(&tag) == 0 {
        assert!(
            Instant::now() <= deadline,
            "the config-declared worker child never spawned (tag {tag})"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    // SIGTERM: the service must shut down within its budget AND reap the
    // worker child (process group + kill_on_drop discipline).
    let exit = child.stop().await;
    assert!(exit.success(), "graceful shutdown, got {exit}");
    let reap_deadline = Instant::now() + Duration::from_secs(15);
    while count_processes(&tag) > 0 {
        assert!(
            Instant::now() <= reap_deadline,
            "the worker child survived the service shutdown (tag {tag})"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    bg.abort();
    release.store(true, Ordering::SeqCst);
    stub.stop().await;
}
