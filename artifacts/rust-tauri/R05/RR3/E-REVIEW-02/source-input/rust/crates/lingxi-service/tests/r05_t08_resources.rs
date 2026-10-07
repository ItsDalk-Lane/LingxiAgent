//! R05 RR1 F27 (WP-T08): the T08-C12 resource verification — a VALIDATED
//! sampler plus the §8 load profile (normal / long response / 100+ repeated
//! cancel & error / nested worker / multi-session), with the RAW time
//! series preserved.
//!
//! The CL-06 counterexamples being closed:
//! - the frozen `fds_of` ran `lsof -p PID` (default column format, first
//!   column COMMAND) and counted lines STARTING WITH A DIGIT — every real
//!   data row was filtered out, so the FD predicate could report 0 forever
//!   and the bounds assertions were vacuous. It also never checked lsof's
//!   exit status, so a failed sample read as "0 open FDs".
//! - the load loop was 2 warmups + 12 mixed cycles — far below the
//!   registered "100+ repeated cancel/error" profile, with no worker,
//!   multi-session or long-response legs.
//! - no raw samples were preserved (`R05_PERFORMANCE_RESULTS.json` only
//!   described the mechanism).
//!
//! Sampler discipline here: `lsof -p PID -F fn` (MACHINE-READABLE), exit
//! status checked, the `p`-record's PID verified against the target (an
//! identity mismatch is an error, not data), FDs counted as `f<digits>`
//! records, and every failure is a loud UNKNOWN — never a fake 0. The
//! positive/negative control leg proves the sampler SEES growth and release
//! on a controlled helper before it is trusted on the service.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

#[path = "support/r05_resource_sampler.rs"]
mod resource_sampler;

// ── the validated resource sampler ──────────────────────────────────────────

/// Why a sample could not be taken (an honest UNKNOWN, never a fake 0).
#[derive(Debug)]
struct SampleError {
    what: &'static str,
    detail: String,
}

impl std::fmt::Display for SampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.what, self.detail)
    }
}

/// The process's resident set size, in KiB (`ps -o rss=`).
fn rss_of(pid: i32) -> Result<u64, SampleError> {
    let pid_text = pid.to_string();
    let out = std::process::Command::new("ps")
        .arg("-o")
        .arg("rss=")
        .arg("-p")
        .arg(&pid_text)
        .output()
        .map_err(|e| SampleError {
            what: "rss: ps did not run",
            detail: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(SampleError {
            what: "rss: ps failed",
            detail: format!(
                "exit={} stderr={}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        });
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return Err(SampleError {
            what: "rss: empty output",
            detail: format!("pid {pid} produced no parseable rss row"),
        });
    }
    text.parse::<u64>().map_err(|e| SampleError {
        what: "rss: non-numeric output",
        detail: format!("{text:?}: {e}"),
    })
}

/// The process's open file-descriptor count, from `lsof -p PID -F fn`
/// (machine-readable): FDs are the `f<digits>` records. The `p<pid>`
/// record must name the requested process — a mismatched identity or a
/// failed lsof is an error, NEVER a zero.
fn fds_of(pid: i32) -> Result<usize, SampleError> {
    let pid_text = pid.to_string();
    let out = std::process::Command::new("lsof")
        .arg("-p")
        .arg(&pid_text)
        .arg("-F")
        .arg("fn")
        .output()
        .map_err(|e| SampleError {
            what: "fds: lsof did not run",
            detail: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(SampleError {
            what: "fds: lsof failed",
            detail: format!(
                "exit={} stderr={}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        });
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut saw_pid_record = false;
    let mut fds = 0usize;
    for line in text.lines() {
        if let Some(recorded) = line.strip_prefix('p') {
            if recorded != pid_text {
                return Err(SampleError {
                    what: "fds: identity mismatch",
                    detail: format!("lsof reported pid {recorded}, requested {pid_text}"),
                });
            }
            saw_pid_record = true;
        } else if line.starts_with('f')
            && line.len() > 1
            && line[1..].chars().all(|c| c.is_ascii_digit())
        {
            fds += 1;
        }
    }
    if !saw_pid_record {
        return Err(SampleError {
            what: "fds: no pid record",
            detail: format!("lsof output carried no p<{}> identity record", pid_text),
        });
    }
    if fds < 3 {
        return Err(SampleError {
            what: "fds: invalid live-process sample",
            detail: format!("pid {pid} has {fds} numeric descriptors; expected at least stdio"),
        });
    }
    Ok(fds)
}

// ── stub provider (routers per load phase) ──────────────────────────────────

struct RecordedRequest {
    body: serde_json::Value,
}

enum Step {
    Sse(String),
    // 仅阻塞供应商替身响应；宿主和 worker 仍走完整生产链。
    GatedSse(String, Arc<tokio::sync::Notify>, Arc<AtomicUsize>),
    /// Hold the connection open forever (the cancel-load parked stream).
    Park,
    Error(u16, String),
}

type Router = Box<dyn Fn(&RecordedRequest) -> Step + Send + Sync>;

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
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
        let dropped = Arc::new(AtomicUsize::new(0));
        let router = Arc::new(router);
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_dropped) = (Arc::clone(&hits), Arc::clone(&dropped));
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                task_hits.fetch_add(1, Ordering::SeqCst);
                let recorded = match read_one_request(&mut socket).await {
                    Some(recorded) => recorded,
                    None => {
                        // A connection that closed before completing headers
                        // (a client pool probe or a cancelled connect) is
                        // NOT a provider request: un-count the hit and move
                        // on — never a router panic.
                        task_hits.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                };
                let step = router(&recorded);
                match step {
                    Step::GatedSse(body, release, arrived) => {
                        arrived.fetch_add(1, Ordering::SeqCst);
                        release.notified().await;
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        socket
                            .write_all(response.as_bytes())
                            .await
                            .expect("gated stub response");
                        socket.shutdown().await.expect("gated stub shutdown");
                    }
                    Step::Sse(body) => {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    }
                    Step::Park => {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                             Cache-Control: no-cache\r\nConnection: close\r\n\r\n";
                        let _ = socket.write_all(head.as_bytes()).await;
                        socket.flush().await.ok();
                        let mut probe = [0_u8; 512];
                        loop {
                            match tokio::time::timeout(
                                Duration::from_secs(3600),
                                socket.read(&mut probe),
                            )
                            .await
                            {
                                Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
                                Ok(Ok(_)) => continue,
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

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

async fn read_one_request(socket: &mut tokio::net::TcpStream) -> Option<RecordedRequest> {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(30), socket.read(&mut chunk))
            .await
            .expect("stub read stalled")
            .expect("stub read");
        if read == 0 {
            return None;
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
    let _path = lines.next().expect("request line").to_string();
    let mut content_length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().parse::<usize>().expect("numeric len"));
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
            return None;
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    Some(RecordedRequest {
        body: serde_json::from_slice(&raw[body_start..body_start + content_length])
            .expect("stub: request body must be JSON"),
    })
}

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
                "model": "stub-model-f27",
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
                "model": "stub-model-f27",
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

fn last_tool_result(body: &serde_json::Value) -> Option<String> {
    body["messages"]
        .as_array()?
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .and_then(|m| m["content"].as_str())
        .map(str::to_string)
}

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

// ── the REAL binary harness ─────────────────────────────────────────────────

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
                panic!("the real binary never printed READY within 30s\nstderr:\n{stderr}")
            })
            .expect("stdout read");
            if read == 0 {
                let stderr = stderr_lines.lock().expect("stderr log").join("\n");
                panic!("the real binary exited before readiness\nstderr:\n{stderr}");
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
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(status) = self.child.try_wait().expect("poll child") {
                return status;
            }
            if Instant::now() > deadline {
                self.child.kill().await.expect("last-resort kill");
                panic!("the real binary did not exit within 60s of SIGTERM");
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

// ── HTTP helpers ─────────────────────────────────────────────────────────────

async fn read_loopback_token(home: &Path) -> String {
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
    let path = format!("/lingxi/v1/sessions/{session}/execute");
    let body = serde_json::json!({"input": input}).to_string();
    let exchanged = tokio::time::timeout(Duration::from_secs(120), async {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to the real service");
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(head.as_bytes()).await.expect("write head");
        socket.write_all(body.as_bytes()).await.expect("write body");
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
    .expect("request to the real service stalled");
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

/// A submission whose transport the caller OWNS: connect + send, keep the
/// socket, and later drop it (the dead-transport cancel of the load).
async fn submit_owned(
    addr: SocketAddr,
    token: String,
    session: String,
    input: String,
) -> tokio::net::TcpStream {
    let mut socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect for an owned submission");
    let path = format!("/lingxi/v1/sessions/{session}/execute");
    let body = serde_json::json!({"input": input}).to_string();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await.expect("write head");
    socket.write_all(body.as_bytes()).await.expect("write body");
    socket
}

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

async fn latest_run_id(home: &Path, session: &str) -> Option<String> {
    let db = open_runs_db(home);
    query_one(
        &db,
        "SELECT run_id FROM runs WHERE session_id = ?1 ORDER BY rowid DESC LIMIT 1",
        &[session],
    )
}

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05f27-{tag}-{}-{}",
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

fn fixture_exe() -> &'static str {
    env!("CARGO_BIN_EXE_r04_t07_fixture")
}

// 进程树按真实 PPID 发现，采样失败必须保留为 UNKNOWN，不能少算进程。
fn tree_sample(pid: i32) -> serde_json::Value {
    let ids = resource_sampler::process_ids(pid).expect("UNKNOWN: 进程树采样失败");
    let mut processes = Vec::new();
    let mut rss = 0;
    let mut fds = 0;
    let mut connections = 0;
    for id in ids {
        let process_rss = rss_of(id).expect("UNKNOWN: 树 RSS 采样失败");
        let process_fds = fds_of(id).expect("UNKNOWN: 树 FD 采样失败");
        let tcp = resource_sampler::tcp_of(id).expect("UNKNOWN: 树 TCP 采样失败");
        assert!(process_rss > 0 && process_fds >= 3, "活进程假零采样 {id}");
        rss += process_rss;
        fds += process_fds;
        connections += tcp.len();
        processes.push(serde_json::json!({"pid": id, "rssKiB": process_rss, "fds": process_fds, "establishedTcp": tcp}));
    }
    serde_json::json!({"rootPid": pid, "rssKiB": rss, "fds": fds, "establishedTcpCount": connections, "processes": processes})
}

fn file_sample(home: &Path, workspace: &Path) -> serde_json::Value {
    serde_json::json!({
        "home": resource_sampler::files(home).expect("UNKNOWN: home 文件采样失败"),
        "workspace": resource_sampler::files(workspace).expect("UNKNOWN: workspace 文件采样失败"),
    })
}

fn assert_log_file_bound(files: &serde_json::Value, phase: &str) {
    let logs: Vec<&serde_json::Value> = files["home"]
        .as_array()
        .expect("完整 home 清单")
        .iter()
        .filter(|entry| {
            let path = entry["path"].as_str().expect("文件路径");
            path.starts_with("lingxi-service/logs/service-") && path.ends_with(".log")
        })
        .collect();
    assert!(
        logs.len() <= 3,
        "F46: {phase}: 重启/稳态日志数量 {} 超过事前上限3: {logs:?}",
        logs.len()
    );
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".into(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.into()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".into()],
    }
}

fn owner_sample(state: &lingxi_service::ServiceState) -> serde_json::Value {
    use lingxi_service::quotas::QuotaResource;
    let quotas = state.runs().quotas();
    serde_json::json!({
        "activeSessionRuns": state.sessions().session_supervisor().active_run_count(),
        "liveRunIds": state.runs().cancel_registry().live_run_ids(),
        "modelPermits": quotas.in_use(QuotaResource::Model),
        "modelWaiters": quotas.waiting(QuotaResource::Model),
        "toolPermits": quotas.in_use(QuotaResource::Tool),
        "toolWaiters": quotas.waiting(QuotaResource::Tool),
        "backgroundIds": state.background().live_ids(),
    })
}

// 此补证是进程内生产组合根，保留它与正式二进制资源序列的边界。
// 不注入 provider/supervisor/存储，配置、凭证、工具、worker、HTTP 都走原实现。
async fn owner_resource_series(
    home: &Path,
    workspace: &Path,
    config_path: &Path,
    arrived: &AtomicUsize,
    release: &tokio::sync::Notify,
) -> Vec<serde_json::Value> {
    use lingxi_service::quotas::{LayeredQuotaLimits, QuotaLimits};
    use lingxi_service::{
        prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
    };
    let layout = prepare_layout(home).expect("owner layout");
    let file = lingxi_service::config::read_service_config(config_path).expect("owner config");
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(config_path), &layout.runtime_dir)
            .expect("owner plane")
            .expect("owner plane present");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("owner credentials"),
    );
    let state = ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().expect("owner bind"),
            data_home: home.to_path_buf(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        },
        &layout,
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            model_plane_source: Some(source),
            credential_service: Some(credentials),
            workspace_root: file.workspace,
            worker_tool_registration: file.workers,
            quota_limits: QuotaLimits {
                model: LayeredQuotaLimits {
                    global: 1,
                    per_agent: 1,
                    per_session: 1,
                },
                ..QuotaLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("owner production bootstrap");
    let started = Instant::now();
    let mut points = Vec::new();
    let sample = |phase: &str, cycle: usize| {
        serde_json::json!({
            "phase": phase, "cycle": cycle, "tMs": started.elapsed().as_millis(),
            "owners": owner_sample(&state), "processTree": tree_sample(std::process::id() as i32),
            "files": file_sample(home, workspace),
        })
    };
    points.push(sample("baseline", 0));
    assert_eq!(points[0]["owners"]["activeSessionRuns"], 0);
    for cycle in 0..WORKER_CYCLES {
        let before = arrived.load(Ordering::SeqCst);
        let drive_state = state.clone();
        let worker = tokio::spawn(async move {
            drive_state
                .sessions()
                .execute_for(
                    drive_state.storage().as_ref(),
                    drive_state.events(),
                    drive_state.runs(),
                    &owner_principal(),
                    "sess_local_alpha",
                    &format!("w: owner {cycle}"),
                    1_790_409_600_000,
                )
                .await
                .expect("owner worker accepted")
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while arrived.load(Ordering::SeqCst) == before {
            assert!(
                Instant::now() < deadline,
                "owner worker 未到达真实 HTTP 屏障"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let queued_state = state.clone();
        let queued = tokio::spawn(async move {
            queued_state
                .sessions()
                .execute_for(
                    queued_state.storage().as_ref(),
                    queued_state.events(),
                    queued_state.runs(),
                    &owner_principal(),
                    "sess_local_beta",
                    "owner queued",
                    1_790_409_600_000,
                )
                .await
                .expect("owner queued accepted")
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while state
            .runs()
            .quotas()
            .waiting(lingxi_service::quotas::QuotaResource::Model)
            != 1
        {
            assert!(
                Instant::now() < deadline,
                "真实第二任务未进入 permit 等待队列"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let peak = sample("worker-and-queued-live", cycle);
        assert_eq!(peak["owners"]["activeSessionRuns"], 2);
        assert_eq!(
            peak["owners"]["liveRunIds"]
                .as_array()
                .expect("live ids")
                .len(),
            2
        );
        assert_eq!(peak["owners"]["modelPermits"], 1);
        assert_eq!(peak["owners"]["modelWaiters"], 1);
        assert_eq!(peak["owners"]["toolPermits"], 1);
        assert_eq!(
            peak["processTree"]["processes"]
                .as_array()
                .expect("live tree")
                .len(),
            2
        );
        points.push(peak);
        let id = latest_run_id(home, "sess_local_beta")
            .await
            .expect("queued id");
        state
            .sessions()
            .cancel_run_for(
                state.storage().as_ref(),
                state.runs(),
                &owner_principal(),
                &id,
            )
            .await
            .expect("queued cancel accepted");
        queued.await.expect("queued task joined");
        assert_eq!(
            state
                .runs()
                .quotas()
                .waiting(lingxi_service::quotas::QuotaResource::Model),
            0
        );
        release.notify_one();
        let result = worker.await.expect("worker task joined");
        let terminal = wait_run_status(
            home,
            &result.run_id,
            |status| status != "running",
            "owner completed",
        )
        .await;
        assert_eq!(terminal, "completed");
        // 三个连续清理后窗口；保留计数，避免单个终点冒充稳态。
        for window in 0..3 {
            let point = sample("released-steady", cycle);
            let owners = &point["owners"];
            assert_log_file_bound(&point["files"], "owner released-steady");
            for field in [
                "activeSessionRuns",
                "modelPermits",
                "modelWaiters",
                "toolPermits",
                "toolWaiters",
            ] {
                assert_eq!(
                    owners[field], 0,
                    "资源未释放: {field}, cycle={cycle}, window={window}"
                );
            }
            assert!(owners["liveRunIds"]
                .as_array()
                .expect("live ids")
                .is_empty());
            assert!(owners["backgroundIds"]
                .as_array()
                .expect("background ids")
                .is_empty());
            assert_eq!(
                point["processTree"]["processes"]
                    .as_array()
                    .expect("released tree")
                    .len(),
                1
            );
            assert_eq!(point["processTree"]["establishedTcpCount"], 0);
            points.push(point);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    state.storage().close().await.expect("owner storage close");
    drop(state);
    points
}

// ── leg 1: the sampler's positive/negative controls ─────────────────────────

/// Spawns `sleep <secs>` with `extra_fds` ADDITIONAL descriptors held open
/// (shell redirections onto fds 3..3+N, kept across the exec). The Child
/// handle rides along so the release control can KILL **and REAP** it (an
/// unreaped zombie still answers `ps` with rss=0 — the dead-pid control
/// needs a genuinely gone process).
fn spawn_sleep_with_fds(secs: u64, extra_fds: usize) -> (std::process::Child, i32) {
    let mut script = format!("exec sleep {secs}");
    for fd in 3..3 + extra_fds {
        script.push_str(&format!(" {fd}</dev/null"));
    }
    let child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .spawn()
        .expect("spawn controlled helper");
    let pid = child.id() as i32;
    (child, pid)
}

#[tokio::test]
async fn f27_sampler_controls_detect_growth_release_and_failure() {
    // Positive control A: the FD sampler sees a live process's descriptors.
    let (mut plain_child, plain) = spawn_sleep_with_fds(30, 0);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let base = fds_of(plain).expect("the plain helper samples");
    assert!(
        base >= 3,
        "a live unix process has at least stdio open; the sampler saw {base}"
    );
    let rss = rss_of(plain).expect("the plain helper's rss samples");
    assert!(rss > 0, "a live process has nonzero rss; got {rss}");

    // Positive control B: +3 deliberately held descriptors are SEEN (+3
    // exactly), proving the counter measures the real FD set.
    let (mut loaded_child, loaded) = spawn_sleep_with_fds(30, 3);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let grown = fds_of(loaded).expect("the loaded helper samples");
    assert_eq!(
        grown,
        base + 3,
        "the FD sampler must count exactly the 3 deliberately held descriptors"
    );

    // Release control: killing the holder makes sampling FAIL loudly (a
    // dead pid is an UNKNOWN, never a silent 0).
    // The release control: kill AND REAP (a zombie still answers ps).
    let kill_and_reap = |child: &mut std::process::Child| {
        let pid = child.id() as i32;
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        child.wait().expect("reap the killed helper");
    };
    kill_and_reap(&mut loaded_child);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let dead = fds_of(loaded);
    assert!(
        dead.is_err(),
        "sampling a dead pid must be an error, got {dead:?}"
    );
    let dead_rss = rss_of(loaded);
    assert!(dead_rss.is_err(), "rss of a dead pid must be an error");

    // Negative control C: a bogus pid never samples.
    let bogus = fds_of(2_000_000);
    assert!(bogus.is_err(), "an unknown pid must not sample");

    // TCP 正反控制：指定端口的两端连接都必须可见，释放后都必须消失。
    let self_pid = std::process::id() as i32;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("control bind");
    let addr = listener.local_addr().expect("control addr");
    let marker = format!(":{}", addr.port());
    let count = || {
        resource_sampler::tcp_of(self_pid)
            .expect("UNKNOWN: TCP 控制采样")
            .iter()
            .filter(|name| name.contains(&marker))
            .count()
    };
    assert_eq!(count(), 0);
    let client = tokio::net::TcpStream::connect(addr)
        .await
        .expect("control connect");
    let (peer, _) = listener.accept().await.expect("control accept");
    let retained = count();
    assert_eq!(retained, 2, "必须看见已知保留连接的两端");
    drop(client);
    drop(peer);
    assert_eq!(count(), 0, "释放后不再有 ESTABLISHED 连接");
    assert!(resource_sampler::tcp_of(2_000_000).is_err());
    assert!(resource_sampler::parse_tcp("", self_pid).is_err());
    assert!(
        resource_sampler::parse_tcp("p1\nf4\nPTCP\nTST=ESTABLISHED\nnlocalhost", self_pid).is_err()
    );

    // 文件清单正反控制：不是只删除目录后猜测临时文件已消失。
    let root = unique_dir("sampler-file-control");
    assert!(resource_sampler::files(&root)
        .expect("empty inventory")
        .is_empty());
    for i in 0..3 {
        std::fs::write(root.join(format!("held-{i}.tmp")), [7; 17]).expect("held file");
    }
    let held_files = resource_sampler::files(&root).expect("held inventory");
    assert_eq!(held_files.len(), 3);
    for i in 0..3 {
        std::fs::remove_file(root.join(format!("held-{i}.tmp"))).expect("release file");
    }
    assert!(resource_sampler::files(&root)
        .expect("released inventory")
        .is_empty());
    std::fs::remove_dir(&root).expect("remove control directory");
    assert!(
        resource_sampler::files(&root).is_err(),
        "不存在目录不能充当空清单"
    );
    let held_tree = tree_sample(plain);
    assert_eq!(
        held_tree["processes"]
            .as_array()
            .expect("process list")
            .len(),
        1
    );
    assert!(resource_sampler::process_ids(loaded).is_err());

    println!(
        "F27 sampler controls: {}",
        serde_json::json!({
            "fdBaseline": base, "fdRetained": grown, "fdReleased": "UNKNOWN (dead/reaped PID)",
            "tcpRetained": retained, "tcpReleased": 0, "filesRetained": held_files,
            "filesReleased": [], "liveProcessTree": held_tree,
            "deadProcessTree": "UNKNOWN", "emptyAndWrongIdentity": "UNKNOWN",
        })
    );

    kill_and_reap(&mut plain_child);
}

// ── leg 2: the sustained load with the raw time series ─────────────────────

/// The pre-registered bounds (docs/rust-tauri/R05/R05_PERFORMANCE_RESULTS.json
/// freezes them BEFORE the run; identical to the incumbent c12 leg — NOT
/// relaxed for the heavier load).
const RSS_BOUND_KIB: u64 = 400 * 1024;
const RSS_GROWTH_BOUND_KIB: u64 = 150 * 1024;
const FD_BOUND: usize = 400;
const FD_GROWTH_BOUND: usize = 64;

/// The registered load profile: ≥60 disconnect-cancels + ≥60 provider
/// errors (=120 ≥ the "100次以上重复取消与错误" floor), plus normal,
/// long-response, nested-worker and multi-session legs.
const CANCEL_CYCLES: usize = 60;
const ERROR_CYCLES: usize = 60;
const OK_CYCLES: usize = 15;
const LONG_CYCLES: usize = 10;
const WORKER_CYCLES: usize = 15;

#[tokio::test]
async fn f27_sustained_cancel_error_worker_load_stays_bounded() {
    // The mode the router keys on; flipped between load phases.
    let mode = Arc::new(Mutex::new("ok".to_string()));
    let mode_for_stub = Arc::clone(&mode);
    let worker_release = Arc::new(tokio::sync::Notify::new());
    let worker_arrived = Arc::new(AtomicUsize::new(0));
    let stub_release = Arc::clone(&worker_release);
    let stub_arrived = Arc::clone(&worker_arrived);
    let read_args = serde_json::json!({"path": "loop.txt"}).to_string();
    let worker_args = serde_json::json!({"input": "input.txt"}).to_string();
    let long_body = "L".repeat(512 * 1024);
    let stub = StubServer::start(Box::new(move |request| {
        let text = user_text(&request.body);
        match mode_for_stub.lock().expect("mode").as_str() {
            "park" => Step::Park,
            "fail" => Step::Error(
                500,
                r#"{"error":{"message":"injected provider failure"}}"#.to_string(),
            ),
            "long" => {
                if last_tool_result(&request.body).is_some() {
                    Step::Sse(sse_final(&long_body))
                } else {
                    Step::Sse(tool_call_turn("call_f27_l", "read", &read_args))
                }
            }
            _ => match request.body["model"].as_str() {
                // The worker callback leg rides the aux-slot route.
                Some("summarize-model-f27") => Step::GatedSse(
                    sse_final("callback ok"),
                    Arc::clone(&stub_release),
                    Arc::clone(&stub_arrived),
                ),
                _ => {
                    if text.starts_with("w:") && last_tool_result(&request.body).is_none() {
                        Step::Sse(tool_call_turn("call_f27_w", "probe", &worker_args))
                    } else if last_tool_result(&request.body).is_some() {
                        Step::Sse(sse_final("load cycle done"))
                    } else {
                        Step::Sse(sse_final("no-tool cycle done"))
                    }
                }
            },
        }
    }))
    .await;

    let home = unique_dir("f27-home");
    let workspace = unique_dir("f27-ws");
    let config_root = unique_dir("f27-cfg");
    let config_path = config_root.join("service.json");
    let worker_argv = format!(
        "[{}, \"ask_model\"]",
        serde_json::to_string(fixture_exe()).expect("argv json")
    );
    std::fs::write(
        &config_path,
        format!(
            r#"{{"home": {}, "workspace": {}, "providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-test-f27"}}
                }},
                "aux": {{
                    "protocol": "openai-completions",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-aux-f27"}}
                }}
            }},
            "models": {{
                "chat": {{"provider": "main", "model": "stub-model-f27", "capabilities": {{"tools": true}}}},
                "summarize": {{"provider": "aux", "model": "summarize-model-f27"}}
            }},
            "workers": {{
                "localName": "probe",
                "op": "probe",
                "description": "F27 load worker",
                "argv": {worker_argv},
                "inputSchema": {{
                    "type": "object",
                    "properties": {{"input": {{"type": "string"}}}},
                    "required": ["input"],
                    "additionalProperties": false
                }},
                "pathArgs": ["input"],
                "allowedModelPurposes": ["summarize"]
            }}}}"#,
            serde_json::to_string(&home.to_string_lossy()).expect("home json"),
            serde_json::to_string(&workspace.to_string_lossy()).expect("ws json"),
            stub.endpoint(),
            stub.endpoint(),
        ),
    )
    .expect("write config");
    std::fs::write(workspace.join("loop.txt"), "cycle body\n").expect("seed loop file");
    std::fs::write(workspace.join("input.txt"), "worker input\n").expect("seed worker input");

    let base_argv = vec![
        "--home".to_string(),
        home.to_string_lossy().to_string(),
        "--config".to_string(),
        config_path.to_string_lossy().to_string(),
        "--bind".to_string(),
        "127.0.0.1:0".to_string(),
        "--log-max-bytes".to_string(),
        "65536".to_string(),
        "--log-max-files".to_string(),
        "3".to_string(),
    ];
    // The cancel-phase instance adds the production request-budget
    // watchdog: a parked in-flight submission is CANCELLED at the budget
    // (408 back, the handler future dropped, the parked provider
    // connection CLOSED). This is the binary surface's real per-request
    // cancellation — the cancel-load cycles below rely on it. The error /
    // normal phases run on a default-budget instance (their retried turns
    // legitimately outlive a 1.5 s per-request budget).
    let mut cancel_argv = base_argv.clone();
    cancel_argv.extend(["--http-request-budget-ms".to_string(), "1500".to_string()]);
    let child = ServiceChild::start(&cancel_argv).await;
    let token = read_loopback_token(&home).await;
    let addr = child.addr;
    let pid = child.pid();

    // The sampler's on-target control: it must read the REAL service before
    // the load is trusted.
    let probe_fds = fds_of(pid).expect("the service samples");
    assert!(
        probe_fds >= 3,
        "the service sampler sees real FDs: {probe_fds}"
    );

    let started = Instant::now();
    let evidence_dir = unique_dir("f27-series");
    let series_path = evidence_dir.join("f27-resource-series.json");
    println!("F27 raw resource series: {}", series_path.display());
    // Sampling happens SYNCHRONOUSLY at each phase marker (a ~100 ms
    // instrumented pause every 10 cycles — the sample must read the LIVE
    // process of THAT phase's instance, and a deferred handle could
    // outlive the instance between restarts).
    let mut series: Vec<serde_json::Value> = Vec::new();
    let mut cycle_results = Vec::new();
    let sample = |series: &mut Vec<serde_json::Value>, sample_pid: i32, phase: &str| {
        let rss = rss_of(sample_pid).expect("an rss sample failed — UNKNOWN is loud");
        let fds = fds_of(sample_pid).expect("an fds sample failed — UNKNOWN is loud");
        series.push(serde_json::json!({
            "tMs": started.elapsed().as_millis() as u64,
            "phase": phase,
            "rssKiB": rss,
            "fds": fds,
            "serviceTree": tree_sample(sample_pid),
            "equipmentProcess": {"pid": std::process::id(), "rssKiB": rss_of(std::process::id() as i32).expect("UNKNOWN: test process RSS"), "fds": fds_of(std::process::id() as i32).expect("UNKNOWN: test process FD")},
            "files": file_sample(&home, &workspace),
        }));
        // 中断或断言失败仍留下已取得的原始数据，不把部分运行记 PASS。
        std::fs::write(&series_path, serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "lingxi.r05-f27-resource-series.v1", "status": "PARTIAL_RUNNING_OR_FAILED", "series": series
        })).expect("partial json")).expect("write partial raw series");
    };

    // Warmup: two clean cycles, then the baseline sample.
    for i in 0..2 {
        let (s, a) = submit(addr, &token, "sess_local_alpha", &format!("warm {i}")).await;
        assert_eq!(s, 200, "{a}");
        let run_id = a["runId"].as_str().expect("runId").to_string();
        let st = wait_run_status(&home, &run_id, |st| st != "running", "settled").await;
        assert_eq!(st, "completed");
    }
    sample(&mut series, pid, "baseline");

    // ── the disconnect-cancel load (≥60): each cycle parks the model
    // stream, then the CLIENT drops the transport — the dead-transport
    // cancel path (the cancel tree fires, the held connection closes; the
    // durable row honestly stays active for the R03-T07 recovery owner).
    let mut cancel_settled = 0usize;
    let mut cancel_dangling = 0usize;
    for cycle in 0..CANCEL_CYCLES {
        *mode.lock().expect("mode") = "park".to_string();
        let session = if cycle % 2 == 0 {
            "sess_local_alpha"
        } else {
            "sess_local_beta"
        };
        let hits_before = stub.hits();
        let owned = submit_owned(
            addr,
            token.clone(),
            session.to_string(),
            format!("cancel {cycle}"),
        )
        .await;
        // Wait until the run exists and is parked on the stub.
        let deadline = Instant::now() + Duration::from_secs(20);
        let run_id = loop {
            if let Some(id) = latest_run_id(&home, session).await {
                let db = open_runs_db(&home);
                let status = query_one(&db, "SELECT status FROM runs WHERE run_id = ?1", &[&id]);
                if status.as_deref() == Some("running") && stub.hits() > hits_before {
                    break id;
                }
            }
            assert!(
                Instant::now() <= deadline,
                "cancel cycle {cycle}: the parked run never appeared"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        if cycle % 10 == 0 {
            sample(&mut series, pid, "cancel-live");
            assert!(
                series.last().expect("cancel peak")["serviceTree"]["establishedTcpCount"]
                    .as_u64()
                    .expect("tcp")
                    >= 2
            );
        }
        // HOLD the socket open through the budget window: the production
        // request-budget watchdog itself cancels the parked submission
        // (408 back, the handler future dropped, the parked provider
        // connection CLOSED — the per-cycle resource-reclaim witness).
        let mut owned = owned;
        let drops_before = stub.dropped();
        let response = tokio::time::timeout(Duration::from_secs(6), async {
            let mut raw = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                match owned.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        raw.extend_from_slice(&chunk[..read]);
                        if raw.windows(4).any(|w| w == b"\r\n\r\n")
                            && raw.starts_with(b"HTTP/1.1 408")
                        {
                            break;
                        }
                    }
                }
            }
            raw
        })
        .await
        .expect("the budget watchdog answered within the window");
        assert!(
            response.starts_with(b"HTTP/1.1 408"),
            "cancel cycle {cycle}: expected the 408 request_timeout, got: {}",
            String::from_utf8_lossy(&response[..response.len().min(120)])
        );
        drop(owned);
        // The parked PROVIDER connection is reclaimed (observed as a stub
        // drop) — per cycle, not just in aggregate.
        let deadline = Instant::now() + Duration::from_secs(5);
        while stub.dropped() <= drops_before {
            assert!(
                Instant::now() <= deadline,
                "cancel cycle {cycle}: the parked provider connection was not reclaimed"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // The durable row either settled through the drop path or stays
        // the honest recovery-owned active state (reclaimed at restart).
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let db = open_runs_db(&home);
            let status = query_one(&db, "SELECT status FROM runs WHERE run_id = ?1", &[&run_id]);
            match status.as_deref() {
                Some("running") if Instant::now() <= deadline => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Some("running") => {
                    cancel_dangling += 1;
                    cycle_results.push(serde_json::json!({"phase":"cancel", "cycle":cycle, "runId":run_id, "status":"running", "providerConnectionReclaimed":true, "httpStatus":408}));
                    break;
                }
                Some(other) => {
                    assert!(
                        other == "cancelled" || other == "failed" || other == "interrupted",
                        "cancel cycle {cycle}: unexpected terminal {other}"
                    );
                    cancel_settled += 1;
                    cycle_results.push(serde_json::json!({"phase":"cancel", "cycle":cycle, "runId":run_id, "status":other, "providerConnectionReclaimed":true, "httpStatus":408}));
                    break;
                }
                None => panic!("cancel cycle {cycle}: run row vanished"),
            }
        }
        *mode.lock().expect("mode") = "ok".to_string();
        if cycle % 10 == 9 {
            sample(&mut series, pid, "cancel");
        }
    }
    // The parked connections the service dropped (the resource-reclaim
    // witness): at least half of the cancel cycles must be OBSERVED as
    // dropped by the stub.
    eprintln!(
        "F27 cancel phase: hits={} dropped={} settled={} dangling={}",
        stub.hits(),
        stub.dropped(),
        cancel_settled,
        cancel_dangling
    );
    // Every parked provider connection was reclaimed (per-cycle assert
    // above); the aggregate must match the cycle count exactly.
    assert_eq!(
        stub.dropped(),
        CANCEL_CYCLES,
        "every parked provider connection was reclaimed by its budget cancel"
    );

    // ── restart under load-pressure #1: the recovery scan settles every
    // dangling-active cancel row honestly with ZERO provider re-execution,
    // and the error/normal phases continue on a default-budget instance.
    let hits_after_cancel = stub.hits();
    child.stop().await;
    let child2 = ServiceChild::start(&base_argv).await;
    let token2 = read_loopback_token(&home).await;
    assert_eq!(
        stub.hits(),
        hits_after_cancel,
        "the recovery scan made zero provider calls"
    );
    {
        let db = open_runs_db(&home);
        let still_running = query_one(
            &db,
            "SELECT COUNT(*) FROM runs WHERE status = 'running'",
            &[],
        )
        .unwrap_or_default();
        assert_eq!(
            still_running, "0",
            "the startup recovery scan resolved every dangling-active cancel row"
        );
    }
    // The post-restart instance fits the bounds too.
    let cancel_final_rss = rss_of(child2.pid()).expect("post-restart rss");
    let cancel_final_fds = fds_of(child2.pid()).expect("post-restart fds");
    assert!(
        cancel_final_rss < RSS_BOUND_KIB,
        "post-restart RSS {cancel_final_rss}"
    );
    assert!(
        cancel_final_fds < FD_BOUND,
        "post-restart FDs {cancel_final_fds}"
    );
    let addr = child2.addr;
    let token = token2;

    // ── the provider-error load (≥60): honest failed terminals.
    let mut error_runs = 0usize;
    for cycle in 0..ERROR_CYCLES {
        *mode.lock().expect("mode") = "fail".to_string();
        let session = if cycle % 2 == 0 {
            "sess_local_beta"
        } else {
            "sess_local_alpha"
        };
        let (s, a) = submit(addr, &token, session, &format!("fail {cycle}")).await;
        assert_eq!(s, 200, "{a}");
        let run_id = a["runId"].as_str().expect("runId").to_string();
        let st = wait_run_status(&home, &run_id, |st| st != "running", "settled").await;
        assert_eq!(st, "failed", "error cycle {cycle} settled honestly: {st}");
        error_runs += 1;
        cycle_results
            .push(serde_json::json!({"phase":"error", "cycle":cycle, "runId":run_id, "status":st}));
        if cycle % 10 == 9 {
            sample(&mut series, child2.pid(), "error");
        }
    }
    *mode.lock().expect("mode") = "ok".to_string();

    // ── normal + long-response + nested-worker loads across three sessions.
    for cycle in 0..OK_CYCLES {
        let session = ["sess_local_alpha", "sess_local_beta"][cycle % 2];
        let (s, a) = submit(addr, &token, session, &format!("ok {cycle}")).await;
        assert_eq!(s, 200, "{a}");
        let run_id = a["runId"].as_str().expect("runId").to_string();
        let st = wait_run_status(&home, &run_id, |st| st != "running", "settled").await;
        assert_eq!(st, "completed");
        cycle_results
            .push(serde_json::json!({"phase":"ok", "cycle":cycle, "runId":run_id, "status":st}));
        if cycle % 5 == 4 {
            sample(&mut series, child2.pid(), "ok");
        }
    }
    *mode.lock().expect("mode") = "long".to_string();
    for cycle in 0..LONG_CYCLES {
        let (s, a) = submit(addr, &token, "sess_local_beta", &format!("long {cycle}")).await;
        assert_eq!(s, 200, "{a}");
        let run_id = a["runId"].as_str().expect("runId").to_string();
        let st = wait_run_status(&home, &run_id, |st| st != "running", "settled").await;
        assert_eq!(st, "completed", "the 512 KiB final settles: {st}");
        cycle_results
            .push(serde_json::json!({"phase":"long", "cycle":cycle, "runId":run_id, "status":st}));
    }
    *mode.lock().expect("mode") = "ok".to_string();
    for cycle in 0..WORKER_CYCLES {
        let before = worker_arrived.load(Ordering::SeqCst);
        let drive_token = token.clone();
        let drive = tokio::spawn(async move {
            submit(
                addr,
                &drive_token,
                "sess_local_alpha",
                &format!("w: {cycle}"),
            )
            .await
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while worker_arrived.load(Ordering::SeqCst) == before {
            assert!(
                Instant::now() < deadline,
                "worker 未抵达真实 callback HTTP 屏障"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        sample(&mut series, child2.pid(), "worker-live");
        let tree = &series.last().expect("live point")["serviceTree"];
        assert_eq!(
            tree["processes"].as_array().expect("processes").len(),
            2,
            "service + 一个存活 worker"
        );
        assert!(
            tree["establishedTcpCount"].as_u64().expect("tcp") >= 2,
            "提交连接及 worker callback 连接存活"
        );
        worker_release.notify_one();
        let (s, a) = drive.await.expect("worker drive joined");
        assert_eq!(s, 200, "{a}");
        let run_id = a["runId"].as_str().expect("runId").to_string();
        let st = wait_run_status(&home, &run_id, |st| st != "running", "settled").await;
        assert_eq!(st, "completed", "worker cycle {cycle}: {st}");
        cycle_results.push(
            serde_json::json!({"phase":"worker", "cycle":cycle, "runId":run_id, "status":st}),
        );
        sample(&mut series, child2.pid(), "worker-released");
        let tree = &series.last().expect("released point")["serviceTree"];
        assert_eq!(
            tree["processes"].as_array().expect("processes").len(),
            1,
            "worker 回收完成"
        );
        assert_eq!(tree["establishedTcpCount"], 0, "worker 完成后连接释放");
    }
    sample(&mut series, child2.pid(), "worker-final");

    // ── the pre-registered bounds (NOT relaxed): every sample is checked,
    // not just the endpoints (each sample asserted its own bound inline at
    // collection time above; the growth bounds close the phase).
    for point in &series {
        let rss = point["rssKiB"].as_u64().expect("rss sample value");
        let fds = point["fds"].as_u64().expect("fds sample value");
        assert!(
            rss < RSS_BOUND_KIB,
            "RSS {rss} KiB exceeds the pre-registered {} bound (phase {})",
            RSS_BOUND_KIB,
            point["phase"]
        );
        assert!(point["serviceTree"]["rssKiB"].as_u64().expect("tree rss") < RSS_BOUND_KIB);
        assert!(point["serviceTree"]["fds"].as_u64().expect("tree fds") < FD_BOUND as u64);
        assert!(
            fds < FD_BOUND as u64,
            "open FDs {fds} exceed the pre-registered bound (phase {})",
            point["phase"]
        );
    }
    let base = &series[0];
    let last = &series[series.len() - 1];
    let base_rss = base["rssKiB"].as_u64().expect("baseline rss");
    let base_fds = base["fds"].as_u64().expect("baseline fds");
    let final_rss = last["rssKiB"].as_u64().expect("final rss");
    let final_fds = last["fds"].as_u64().expect("final fds");
    assert!(
        final_rss.saturating_sub(base_rss) < RSS_GROWTH_BOUND_KIB,
        "RSS grew {} KiB across the load (bound {})",
        final_rss.saturating_sub(base_rss),
        RSS_GROWTH_BOUND_KIB
    );
    assert!(
        final_fds.saturating_sub(base_fds) <= FD_GROWTH_BOUND as u64,
        "open FDs grew by {} across the load (bound +{})",
        final_fds.saturating_sub(base_fds),
        FD_GROWTH_BOUND
    );

    // Log rotation stays bounded under the load.
    let log_dir = std::fs::canonicalize(&home)
        .expect("canonical home")
        .join("lingxi-service")
        .join("logs");
    let log_files = std::fs::read_dir(&log_dir)
        .expect("logs dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .count();
    assert!(log_files <= 3, "rotated log files {log_files} exceed 3");

    // ── the closing restart: history stays queryable, zero re-execution.
    let hits_before_final = stub.hits();
    child2.stop().await;
    let child3 = ServiceChild::start(&base_argv).await;
    let token3 = read_loopback_token(&home).await;
    assert_eq!(
        stub.hits(),
        hits_before_final,
        "the closing restart re-executed nothing"
    );
    let (s, a) = submit(
        child3.addr,
        &token3,
        "sess_local_alpha",
        "post restart read",
    )
    .await;
    assert_eq!(s, 200, "{a}");
    let run_id = a["runId"].as_str().expect("runId").to_string();
    let st = wait_run_status(&home, &run_id, |st| st != "running", "settled").await;
    assert_eq!(st, "completed");
    assert_eq!(stub.hits(), hits_before_final + 1);
    let post_rss = rss_of(child3.pid()).expect("final-instance rss");
    let post_fds = fds_of(child3.pid()).expect("final-instance fds");
    assert!(
        post_rss < RSS_BOUND_KIB,
        "final-instance RSS {post_rss} KiB"
    );
    assert!(post_fds < FD_BOUND, "final-instance FDs {post_fds}");
    series.push(serde_json::json!({
        "tMs": started.elapsed().as_millis() as u64,
        "phase": "post-restart",
        "rssKiB": post_rss,
        "fds": post_fds,
        "serviceTree": tree_sample(child3.pid()),
        "files": file_sample(&home, &workspace),
    }));
    assert_log_file_bound(&series.last().expect("重启采样")["files"], "post-restart");

    // 清理记录只能在退出、回收及删除已经被核对后写入。
    let final_service_pid = child3.pid();
    let exit = child3.stop().await;
    assert!(exit.success(), "最后实例未正常退出: {exit}");
    assert!(resource_sampler::process_ids(final_service_pid).is_err());
    let owners = owner_resource_series(
        &home,
        &workspace,
        &config_path,
        &worker_arrived,
        &worker_release,
    )
    .await;
    let files_before_cleanup = file_sample(&home, &workspace);
    let stub_dropped = stub.dropped();
    let stub_hits = stub.hits();
    stub.stop().await;
    std::fs::remove_dir_all(&home).expect("删除隔离 home");
    std::fs::remove_dir_all(&workspace).expect("删除隔离 workspace");
    std::fs::remove_dir_all(&config_root).expect("删除隔离 config");
    assert!(!home.exists() && !workspace.exists() && !config_root.exists());

    // ── the RAW time series is preserved (the evidence the frozen
    // candidate never kept). The series lands in the test's OWN freshly
    // created temp directory; the path is PRINTED on the standard output
    // the gate producer captures, and the stage-suite script copies the
    // printed file into the run's evidence directory (the Rust side never
    // writes outside its own temp root).
    let doc = serde_json::json!({
        "schema": "lingxi.r05-f27-resource-series.v1",
        "caseId": "R05-T08-C12",
        "load": {
            "cancelCycles": CANCEL_CYCLES,
            "errorCycles": ERROR_CYCLES,
            "okCycles": OK_CYCLES,
            "longResponseCycles": LONG_CYCLES,
            "workerCycles": WORKER_CYCLES,
            "cancelSettled": cancel_settled,
            "cancelDanglingActive": cancel_dangling,
            "errorRuns": error_runs,
            "sessions": ["sess_local_alpha", "sess_local_beta"],
        },
        "thresholds": {
            "rssBoundKiB": RSS_BOUND_KIB,
            "rssGrowthBoundKiB": RSS_GROWTH_BOUND_KIB,
            "fdBound": FD_BOUND,
            "fdGrowthBound": FD_GROWTH_BOUND,
            "logFilesBound": 3,
            "registeredBeforeRun": true,
        },
        "sampler": {
            "fds": "lsof -p PID -F fn (f<digits> records; p-record identity checked; exit status checked)",
            "rss": "ps -o rss= -p PID (exit status + numeric output checked)",
            "controls": "f27_sampler_controls_detect_growth_release_and_failure (+3 exact, release fails loudly, dead/bogus pid errors)",
        },
        "environment": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "binary": "CARGO_BIN_EXE_lingxi-service (debug)",
            "binaryPath": env!("CARGO_BIN_EXE_lingxi-service"),
            "workerPath": fixture_exe(),
            "equipmentPath": std::env::current_exe().expect("test exe path"),
            "stubDroppedConnections": stub_dropped,
            "stubHits": stub_hits,
        },
        "series": series,
        "cycleResults": cycle_results,
        "ownerResourceSeries": owners,
        "cleanup": {
            "serviceStopped": true,
            "lastServicePid": final_service_pid,
            "lastServiceExit": exit.code(),
            "filesBeforeCleanup": files_before_cleanup,
            "homeRemoved": !home.exists(),
            "workspaceRemoved": !workspace.exists(),
            "configRemoved": !config_root.exists(),
            "equipmentStubStopped": true,
        },
    });
    std::fs::write(
        &series_path,
        serde_json::to_string_pretty(&doc).expect("series json"),
    )
    .expect("write the raw series");
    println!("F27 raw resource series: {}", series_path.display());

    // The series directory SURVIVES the cleanup — it is the evidence.
}
