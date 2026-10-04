//! R05-T01 binary-level evidence (C02/C03 at the level the acceptance
//! demands): the REAL `lingxi-service` binary started as a subprocess with
//! real argv, a real `--config` file, a real loopback stub implementing the
//! openai-completions family, submissions through the AUTHENTICATED HTTP
//! endpoint, and journal correlation straight from the run database.
//! Nothing here calls `bootstrap_with_deps` — the process under test is
//! the shipping entrypoint.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── loopback stub provider (the far end of the wire) ────────────────────────

struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    body: serde_json::Value,
}

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start(responses: Vec<String>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_requests, task_responses) = (
            Arc::clone(&hits),
            Arc::clone(&requests),
            Arc::clone(&responses),
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
                task_requests.lock().expect("requests").push(recorded);
                let (status, payload) = match task_responses.lock().expect("responses").pop_front()
                {
                    Some(body) => ("200 OK", body),
                    None => (
                        "500 Internal Server Error",
                        r#"{"error":{"message":"stub script exhausted"}}"#.to_string(),
                    ),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            addr,
            hits,
            requests,
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
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
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
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("stub: utf8 headers");
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
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
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

// ── child process harness (the REAL binary) ─────────────────────────────────

struct ServiceChild {
    child: Child,
    addr: SocketAddr,
    ready_line: String,
    /// The exact argv the process under test was launched with (C02's
    /// subprocess evidence; asserted by the binary-chain test).
    argv: Vec<String>,
    /// Captured output for post-mortem diagnostics (read on the panic
    /// paths; kept on the struct so a failing assertion can dump them).
    #[allow(dead_code)]
    stdout_lines: Arc<Mutex<Vec<String>>>,
    #[allow(dead_code)]
    stderr_lines: Arc<Mutex<Vec<String>>>,
}

impl ServiceChild {
    /// Starts the REAL binary with the given args; waits (bounded) for the
    /// machine-readable readiness line on stdout.
    fn start(args: &[String]) -> Self {
        let binary = env!("CARGO_BIN_EXE_lingxi-service");
        let mut argv = vec![binary.to_string()];
        argv.extend(args.iter().cloned());
        let mut child = Command::new(binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the real lingxi-service binary");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let stdout_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<String>();
        {
            let stdout_lines = Arc::clone(&stdout_lines);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let trimmed = line.trim_end().to_string();
                            if trimmed.starts_with("LINGXI_SERVICE_READY ") {
                                let _ = ready_tx.send(trimmed.clone());
                            }
                            stdout_lines.lock().expect("stdout log").push(trimmed);
                        }
                    }
                }
            });
        }
        {
            let stderr_lines = Arc::clone(&stderr_lines);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => stderr_lines
                            .lock()
                            .expect("stderr log")
                            .push(line.trim_end().to_string()),
                    }
                }
            });
        }
        let ready_line = match ready_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(line) => line,
            Err(_) => {
                let stderr = stderr_lines.lock().expect("stderr log").join("\n");
                panic!("the real binary never printed LINGXI_SERVICE_READY within 30s\nstderr so far:\n{stderr}");
            }
        };
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
            argv,
            stdout_lines,
            stderr_lines,
        }
    }

    /// Graceful SIGTERM (the production signal path), then a bounded wait;
    /// SIGKILL only as the honest last resort of test cleanup.
    fn stop(mut self) -> std::process::ExitStatus {
        let pid = self.child.id() as i32;
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("the real binary did not exit within 30s of SIGTERM");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for ServiceChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
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
        "lingxi-r05t01-bin-{tag}-{}-{}",
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
        // COUNT(*) comes back INTEGER, the text columns TEXT — read both.
        row.get::<_, rusqlite::types::Value>(0)
            .map(|value| match value {
                rusqlite::types::Value::Integer(n) => n.to_string(),
                rusqlite::types::Value::Text(s) => s,
                other => format!("{other:?}"),
            })
    })
    .ok()
}

fn tool_call_then_final_responses(file_arg: &str) -> Vec<String> {
    // R05-T04: production streams — the stub answers SSE frames (delta →
    // finish_reason → usage → [DONE]); closed-batch semantics unchanged.
    let arguments = serde_json::json!({"path": file_arg}).to_string();
    let sse = |frames: &[serde_json::Value], usage: serde_json::Value| {
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
    };
    vec![
        sse(
            &[
                serde_json::json!({
                    "id": "chatcmpl-stub",
                    "model": "the-stub-lies",
                    "choices": [{
                        "index": 0,
                        "finish_reason": null,
                        "delta": {
                            "role": "assistant",
                            "tool_calls": [{
                                "index": 0,
                                "id": "call_bin_1",
                                "type": "function",
                                "function": {"name": "read", "arguments": arguments}
                            }]
                        }
                    }]
                }),
                serde_json::json!({
                    "id": "chatcmpl-stub",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
                }),
            ],
            serde_json::json!({"prompt_tokens": 31, "completion_tokens": 13}),
        ),
        sse(
            &[
                serde_json::json!({
                    "id": "chatcmpl-stub",
                    "model": "the-stub-lies",
                    "choices": [{
                        "index": 0,
                        "finish_reason": null,
                        "delta": {"role": "assistant", "content": "binary chain final answer"}
                    }]
                }),
                serde_json::json!({
                    "id": "chatcmpl-stub",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
                }),
            ],
            serde_json::json!({"prompt_tokens": 41, "completion_tokens": 17}),
        ),
    ]
}

// ── C02 (binary level): real subprocess, real argv, authenticated endpoint ──

#[tokio::test]
async fn c02_real_binary_full_chain_through_authenticated_endpoint() {
    const FILE_BODY: &str = "R05-T01 binary-level real file body: 真实内容-bin\n";
    let stub = StubServer::start(tool_call_then_final_responses("note.txt")).await;
    let home = unique_dir("c02-home");
    let workspace = unique_dir("c02-ws");
    std::fs::write(workspace.join("note.txt"), FILE_BODY).expect("seed workspace file");
    let config_root = unique_dir("c02-cfg");
    let config_path = config_root.join("service.json");
    std::fs::write(
        &config_path,
        format!(
            r#"{{"home": {}, "workspace": {}, "providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-test-binary"}}
                }}
            }},
            "models": {{"chat": {{"provider": "main", "model": "stub-model-bin"}}}}}}"#,
            serde_json::to_string(&home.to_string_lossy()).expect("home json"),
            serde_json::to_string(&workspace.to_string_lossy()).expect("ws json"),
            stub.endpoint()
        ),
    )
    .expect("write config");

    // The REAL binary, real argv — no in-process shortcut anywhere.
    let argv: Vec<String> = vec![
        "--home".to_string(),
        home.to_string_lossy().to_string(),
        "--config".to_string(),
        config_path.to_string_lossy().to_string(),
        "--bind".to_string(),
        "127.0.0.1:0".to_string(),
    ];
    let child = ServiceChild::start(&argv);
    // The evidence anchor: the process under test IS the shipping binary,
    // launched with these exact argv (no in-process shortcut).
    assert_eq!(child.argv[1..], argv[..]);
    assert!(
        child.ready_line.contains("source=cli"),
        "the home came from --home: {}",
        child.ready_line
    );
    let addr = child.addr;
    let token = read_loopback_token(&home);

    // Submit through the AUTHENTICATED endpoint (no token => 401 first).
    let (unauthorized, _) = http_json(
        addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        "wrong-token",
        Some(&serde_json::json!({"input": "read note.txt please"})),
    )
    .await;
    assert_eq!(unauthorized, 401, "the endpoint is authenticated");

    let (status, accepted) = http_json(
        addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &token,
        Some(&serde_json::json!({"input": "read note.txt please"})),
    )
    .await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"]
        .as_str()
        .expect("acceptance carries runId")
        .to_string();

    // Network capture (the stub IS the capture): exactly the two protocol
    // turns, real credential flow, real tool result on the second turn.
    let requests = stub.requests();
    assert_eq!(stub.hits(), 2, "exactly two provider turns hit the wire");
    assert_eq!(requests.len(), 2);
    for (path, authorization, body) in &requests {
        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(authorization.as_deref(), Some("Bearer sk-test-binary"));
        assert_eq!(body["model"], "stub-model-bin");
    }
    let messages = requests[1].2["messages"].as_array().expect("messages");
    assert_eq!(
        messages.len(),
        3,
        "user + assistant(tool_calls) + tool result"
    );
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_bin_1");
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["tool_call_id"], "call_bin_1");
    assert_eq!(
        messages[2]["content"], FILE_BODY,
        "REAL file content rode the wire"
    );

    // Journal correlation straight from the on-disk database.
    let db = open_runs_db(&home);
    assert_eq!(
        query_one(&db, "SELECT status FROM runs WHERE run_id = ?1", &[&run_id]).as_deref(),
        Some("completed")
    );
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
        Some("2")
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
    // The persisted per-call usage is the provider-REPORTED usage.
    let usage_payload = query_one(
        &db,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_completed' ORDER BY event_id LIMIT 1",
        &[&run_id],
    )
    .expect("usage payload row");
    assert!(
        usage_payload.contains("\"inputTokens\":\"31\""),
        "first call usage persisted verbatim: {usage_payload}"
    );
    // The persisted serving identity is the resolved route, not the stub's
    // claimed model.
    let started_payload = query_one(
        &db,
        "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_started' ORDER BY event_id LIMIT 1",
        &[&run_id],
    )
    .expect("started payload row");
    assert!(
        started_payload.contains("\"provider\":\"main\""),
        "{started_payload}"
    );
    assert!(
        started_payload.contains("\"model\":\"stub-model-bin\""),
        "{started_payload}"
    );
    assert!(
        !started_payload.contains("the-stub-lies"),
        "{started_payload}"
    );

    // Graceful shutdown of the REAL process (exit 0 through the signal path).
    let exit = child.stop();
    assert!(
        exit.success(),
        "the real binary shut down cleanly, got {exit}"
    );
    stub.stop().await;
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&config_root);
}

// ── C03 (binary level): no plane => the honest unconfigured outcome ─────────

#[tokio::test]
async fn c03_real_binary_without_plane_settles_unconfigured() {
    let home = unique_dir("c03-home");
    let argv: Vec<String> = vec![
        "--home".to_string(),
        home.to_string_lossy().to_string(),
        "--bind".to_string(),
        "127.0.0.1:0".to_string(),
    ];
    let child = ServiceChild::start(&argv);
    let addr = child.addr;
    let token = read_loopback_token(&home);

    let (status, accepted) = http_json(
        addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &token,
        Some(&serde_json::json!({"input": "this needs a model"})),
    )
    .await;
    assert_eq!(status, 200, "the submission is accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();

    // The run settles with the EXPLICIT unconfigured outcome — no fabricated
    // reply, no hidden fallback, no tool side effect.
    let db = open_runs_db(&home);
    assert_eq!(
        query_one(&db, "SELECT status FROM runs WHERE run_id = ?1", &[&run_id]).as_deref(),
        Some("completed")
    );
    assert_eq!(
        query_one(
            &db,
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            &[&run_id]
        )
        .as_deref(),
        Some("completed.no_final.no_provider_configured")
    );
    assert_eq!(
        query_one(
            &db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'model_call_started'",
            &[&run_id]
        )
        .as_deref(),
        Some("0"),
        "no model call was even started"
    );
    assert_eq!(
        query_one(
            &db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'tool_call_started'",
            &[&run_id]
        )
        .as_deref(),
        Some("0"),
        "no tool side effect happened"
    );
    // No message row was fabricated for the run.
    assert_eq!(
        query_one(
            &db,
            "SELECT COUNT(*) FROM messages WHERE run_id = ?1",
            &[&run_id]
        )
        .as_deref(),
        Some("0")
    );

    let exit = child.stop();
    assert!(exit.success(), "clean shutdown, got {exit}");
    let _ = std::fs::remove_dir_all(&home);
}
