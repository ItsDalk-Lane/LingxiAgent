//! R06-T02 binary closed loop：真 `lingxi-service` 二进制（真 argv、真
//! `--config` 文件、真认证面）经 loopback stub 的 mid-run 压缩闭环。
//! 这是唯一走**生产 bootstrap 链**（wired_real_model_chain 分支：config
//! 文件 → ModelGateway/CredentialService/AuxiliaryExecutor/CompactionService
//! 全自动接线）的验证——`bootstrap_with_deps` 注入路径不覆盖该分支。
//!
//! A03 腿：长历史越过 FORCE 线 → loop 顶自动压缩 → 压缩后首个 chat 请求
//! 的历史以摘要 user 消息（现役包装文本）开头、工具配对完整、被压旧区
//! 离开线体、台账落 auxiliary.summarize succeeded 行。
//! A04 腿：摘要 provider 返回错误（content_filter）→ run 以原历史续跑
//! 完成、错误落台账 failed 行、任何空/假摘要不覆盖历史。
//!
//! stub 是外部网络的行为替身：只按「真实模型合法可见的内容」（请求体
//! 文本）路由，从不读写工作区、不决定 run/审批/存储状态。不使用真实
//! 付费凭证（apiKey 是字面量占位）；全部材料落独立临时目录。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

/// kernel compaction 模块的现役包装（只读锚定——闭环断言摘要消息以这个
/// 逐字前缀进入 chat 请求的历史，而不是任何 system 槽）。
const SUMMARY_PREFIX: &str =
    "The conversation history before this point was compacted into the following summary:";
/// mid-run 摘要消息末尾追加的现役 notice（逐字开头标记）。
const MIDRUN_NOTICE_MARKER: &str = "[System compaction notice";
/// 压缩指令的唯一标记（router 据此区分摘要调用与 chat 调用）。
const INSTRUCTION_MARKER: &str = "Internal compaction-only run.";

// ── routing stub provider（线体的远端） ───

struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    body: serde_json::Value,
}

enum Step {
    Sse(String),
}

type Router = Box<dyn Fn(&serde_json::Value) -> Step + Send + Sync>;

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
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
        let router = Arc::new(router);
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_requests) = (Arc::clone(&hits), Arc::clone(&requests));
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                task_hits.fetch_add(1, Ordering::SeqCst);
                let request = read_one_request(&mut socket).await;
                let step = (router)(&request.body);
                task_requests.lock().expect("requests").push(request);
                match step {
                    Step::Sse(body) => {
                        let raw = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        if socket.write_all(raw.as_bytes()).await.is_err() {
                            continue;
                        }
                        let _ = socket.shutdown().await;
                    }
                }
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
    let total = loop {
        let mut chunk = [0_u8; 8192];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub read")
            .expect("stub read io");
        if read == 0 {
            panic!("connection closed before the full request arrived");
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4) {
            let headers = String::from_utf8_lossy(&raw[..pos]).to_string();
            let len = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if raw.len() >= pos + len {
                break pos + len;
            }
        }
    };
    let text = String::from_utf8_lossy(&raw[..total]).to_string();
    let (head, body) = text.split_once("\r\n\r\n").expect("header/body split");
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    let authorization = head.lines().find_map(|line| {
        line.to_ascii_lowercase()
            .starts_with("authorization:")
            .then(|| {
                line.split_once(':')
                    .expect("auth value")
                    .1
                    .trim()
                    .to_string()
            })
    });
    RecordedRequest {
        path,
        authorization,
        body: serde_json::from_str(body).expect("request body JSON"),
    }
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

/// chat turn：大 content（把历史堆过 keep_recent 预算）+ 一次 read 工具
/// 调用。usage 固定 880+20——window=1000 时 ratio 90% 过 FORCE 线。
fn big_tool_call_turn(call_id: &str, arguments: &str) -> String {
    let content = "x".repeat(30_000);
    sse_of(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                "model": "stub-chat-t02",
                "choices": [{
                    "index": 0,
                    "finish_reason": null,
                    "delta": {
                        "role": "assistant",
                        "content": content,
                        "tool_calls": [{
                            "index": 0,
                            "id": call_id,
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
        serde_json::json!({"prompt_tokens": 880, "completion_tokens": 20}),
    )
}

fn final_turn(text: &str) -> String {
    sse_of(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                "model": "stub-chat-t02",
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
        serde_json::json!({"prompt_tokens": 880, "completion_tokens": 20}),
    )
}

/// 合法结构化摘要（kernel golden 形态，九标题齐全）。
fn summary_turn() -> String {
    let summary = "## Goal\n处理大文件\n\n## Constraints & Preferences\n- (none)\n\n\
         ## Progress\n### Done\n- [x] 读取四轮\n\n### In Progress\n- [ ] 汇总\n\n\
         ### Blocked\n- 无\n\n## Key Decisions\n- **用真实工具**: 避免猜测\n\n\
         ## Next Steps\n1. 写出结论\n\n## Critical Context\n- (none)\n";
    sse_of(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                "model": "stub-summarize-t02",
                "choices": [{
                    "index": 0,
                    "finish_reason": null,
                    "delta": {"role": "assistant", "content": summary}
                }]
            }),
            serde_json::json!({
                "id": "chatcmpl-stub",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
            }),
        ],
        serde_json::json!({"prompt_tokens": 5000, "completion_tokens": 120}),
    )
}

/// A04 腿的 provider 错误：content_filter 是非重试的协议级拒绝。
fn content_filter_refusal() -> String {
    sse_of(
        &[
            serde_json::json!({
                "id": "chatcmpl-stub",
                "model": "stub-summarize-t02",
                "choices": [{
                    "index": 0,
                    "finish_reason": null,
                    "delta": {"role": "assistant", "content": "..."}
                }]
            }),
            serde_json::json!({
                "id": "chatcmpl-stub",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "content_filter"}]
            }),
        ],
        serde_json::json!({"prompt_tokens": 5000, "completion_tokens": 1}),
    )
}

// ── child process harness（真二进制，异步 spawn/read） ───

struct ServiceChild {
    child: tokio::process::Child,
    addr: SocketAddr,
    ready_line: String,
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
                panic!("the real binary exited before readiness\nstderr so far:\n{stderr}");
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
            ready_line,
            _drain: drain,
            _stderr: stderr_task,
        }
    }

    async fn stop(mut self) -> std::process::ExitStatus {
        let pid = self.child.id().expect("child pid") as i32;
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

// ── authenticated HTTP client（loopback） ───

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
                Err(_) => break,
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

// ── harness ───

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t02-loop-{tag}-{}-{}",
        std::process::id(),
        UNIQUE_DIR_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    dir
}

static UNIQUE_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

struct Harness {
    home: PathBuf,
    workspace: PathBuf,
    argv_tail: Vec<String>,
}

impl Harness {
    /// chat 槽声明 contextWindow=1000（reserve 线饱和为 0，任何非零
    /// usage 都过线）；summarize 槽声明 tools 能力（压缩请求携带 live
    /// 工具快照——能力门要求 summarize 路由声明 tools:true）。摘要输出
    /// 上限走现役 reserve 公式，与路由声明的 maxTokens 无关。
    fn new(tag: &str, stub_endpoint: &str) -> Self {
        let home = unique_dir(&format!("{tag}-home"));
        let workspace = unique_dir(&format!("{tag}-ws"));
        let config_root = unique_dir(&format!("{tag}-cfg"));
        let config_path = config_root.join("service.json");
        let config = serde_json::json!({
            "home": home.to_string_lossy(),
            "workspace": workspace.to_string_lossy(),
            "providers": {
                "main": {
                    "protocol": "openai-completions",
                    "endpoint": stub_endpoint,
                    "auth": {"kind": "apiKey", "apiKey": "sk-test-t02"}
                }
            },
            "models": {
                "chat": {
                    "provider": "main",
                    "model": "stub-chat-t02",
                    "capabilities": {"tools": true},
                    "compat": {"contextWindow": 1000}
                },
                "summarize": {
                    "provider": "main",
                    "model": "stub-summarize-t02",
                    "capabilities": {"tools": true},
                    // FIX-07（N-1）：现役 fit 检查以摘要模型的
                    // contextWindow 为准，未声明即硬截断。本闭环走摘要
                    // 模型路径，声明足够大的窗口让 fit 检查通过。
                    "compat": {"contextWindow": 1000000}
                }
            }
        });
        std::fs::write(&config_path, config.to_string()).expect("write config");
        let argv_tail = vec![
            "--home".to_string(),
            home.to_string_lossy().to_string(),
            "--config".to_string(),
            config_path.to_string_lossy().to_string(),
            "--bind".to_string(),
            "127.0.0.1:0".to_string(),
        ];
        Self {
            home,
            workspace,
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

/// 只读打开 run 数据库（service 仍持有写者；WAL 允许并发读者）。
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

// ── routers ───

/// A03：摘要请求回合法结构化摘要；chat 前 4 次回大 content + read 调用
/// （把历史堆过 keep_recent），第 5 次回 final。
fn compacting_router(file: &str, chat_hits: Arc<AtomicUsize>) -> Router {
    let read_args = serde_json::json!({"path": file}).to_string();
    Box::new(move |body| {
        let text = body.to_string();
        if text.contains(INSTRUCTION_MARKER) {
            return Step::Sse(summary_turn());
        }
        let n = chat_hits.fetch_add(1, Ordering::SeqCst);
        if n < 4 {
            Step::Sse(big_tool_call_turn(
                &format!("call_t02_read_{n}"),
                &read_args,
            ))
        } else {
            Step::Sse(final_turn("t02 closed-loop done"))
        }
    })
}

/// A04：摘要请求回 content_filter 拒绝；chat 形状与 A03 相同。
fn failing_summary_router(file: &str, chat_hits: Arc<AtomicUsize>) -> Router {
    let read_args = serde_json::json!({"path": file}).to_string();
    Box::new(move |body| {
        let text = body.to_string();
        if text.contains(INSTRUCTION_MARKER) {
            return Step::Sse(content_filter_refusal());
        }
        let n = chat_hits.fetch_add(1, Ordering::SeqCst);
        if n < 4 {
            Step::Sse(big_tool_call_turn(
                &format!("call_t02_read_{n}"),
                &read_args,
            ))
        } else {
            Step::Sse(final_turn("t02 closed-loop done"))
        }
    })
}

/// 一个 chat 请求体的配对完整性：每个 role=tool 消息都能在前面某个
/// assistant 消息的 tool_calls 里找到自己的 id（A03 的线体断言）。
fn assert_wire_pairs(body: &serde_json::Value, label: &str) {
    let messages = body["messages"].as_array().expect("messages");
    let mut known: Vec<String> = Vec::new();
    for message in messages {
        match message["role"].as_str() {
            Some("assistant") => {
                for call in message["tool_calls"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    known.push(call["id"].as_str().expect("call id").to_string());
                }
            }
            Some("tool") => {
                let id = message["tool_call_id"].as_str().expect("pairing id");
                assert!(
                    known.iter().any(|k| k == id),
                    "{label}: tool message {id} has no owning assistant call on the wire"
                );
            }
            _ => {}
        }
    }
}

// ── A03：真二进制上的自动压缩闭环 ───

#[tokio::test]
async fn a03_mid_run_compaction_lands_on_the_real_binary() {
    let chat_hits = Arc::new(AtomicUsize::new(0));
    let stub = StubServer::start(compacting_router("big.txt", Arc::clone(&chat_hits))).await;
    let h = Harness::new("a03", &stub.endpoint());
    // read 的目标文件真实存在（真实文件工具读它——内容小，历史体量由
    // assistant 的大 content 承担，不依赖 read 的输出预算）。
    std::fs::write(h.workspace.join("big.txt"), "seed\n").expect("seed big.txt");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;
    assert!(child.ready_line.contains("source=cli"));

    let (status, accepted) = submit(addr, &token, "sess_local_alpha", "process big.txt").await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    // 线体：5 次 chat + 恰好 1 次摘要调用。
    let requests = stub.requests();
    let summary_requests: Vec<_> = requests
        .iter()
        .filter(|(_, _, body)| body.to_string().contains(INSTRUCTION_MARKER))
        .collect();
    let chat_requests: Vec<_> = requests
        .iter()
        .filter(|(_, _, body)| !body.to_string().contains(INSTRUCTION_MARKER))
        .collect();
    assert_eq!(
        summary_requests.len(),
        1,
        "exactly one summary call hit the wire: {} total requests",
        stub.hits()
    );
    assert_eq!(chat_requests.len(), 5, "4 read turns + the final turn");

    for (path, authorization, body) in &requests {
        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(authorization.as_deref(), Some("Bearer sk-test-t02"));
        let _ = body;
    }

    // 摘要请求：生产链身份（summarize 槽路由的模型）、缓存保留形状
    // （messages[0]=system、messages[1]=user submission、末尾是指令）。
    // R2-F-01 修复后的现役线体语义（§十#5）：openai-completions 属
    // optional-cap 族——线上**不发**任何输出上限字段（0.8×reserve 公式
    // 只服务预算估算与 required 族兜底，非线体默认）。
    let summary_body = &summary_requests[0].2;
    assert_eq!(summary_body["model"], "stub-summarize-t02");
    assert!(
        summary_body.get("max_tokens").is_none()
            && summary_body.get("max_output_tokens").is_none()
            && summary_body.get("max_completion_tokens").is_none(),
        "the optional-cap family sends NO output cap field on the wire: {summary_body}"
    );
    let summary_messages = summary_body["messages"].as_array().expect("messages");
    assert_eq!(summary_messages[0]["role"], "system");
    assert_eq!(summary_messages[1]["role"], "user");
    assert_eq!(summary_messages[1]["content"], "process big.txt");
    let last = summary_messages.last().expect("instruction");
    assert_eq!(last["role"], "user");
    let instruction = last["content"].as_str().expect("instruction text");
    assert!(instruction.contains(INSTRUCTION_MARKER));
    assert!(instruction.contains("## Critical Context"));
    // 摘要请求自身的工具配对也完整（历史带工具调用上线）。
    assert_wire_pairs(summary_body, "summary request");

    // 压缩后的首个 chat 请求（第 5 次）：历史以摘要 user 消息开头——
    // 现役包装文本逐字 + mid-run notice；摘要不进任何 system 槽；
    // 被压旧区（call_t02_read_0 的调用与结果）已离开线体；保留区的
    // 工具配对完整。
    let final_chat = &chat_requests[4].2;
    assert_eq!(final_chat["model"], "stub-chat-t02");
    let messages = final_chat["messages"].as_array().expect("messages");
    let system_count = messages.iter().filter(|m| m["role"] == "system").count();
    assert_eq!(system_count, 1, "the summary never enters a system slot");
    // messages: [system, user(submission), user(摘要+notice), ...保留区]
    let summary_message = messages
        .iter()
        .find(|m| {
            m["role"] == "user"
                && m["content"]
                    .as_str()
                    .is_some_and(|c| c.starts_with(SUMMARY_PREFIX))
        })
        .expect("the summary user message rides the history");
    let summary_text = summary_message["content"].as_str().expect("summary text");
    assert!(
        summary_text.contains("## Goal"),
        "the structured summary survived sanitization"
    );
    // mid-run notice 是紧随摘要消息的一条独立 user 消息（渲染臂的形状）。
    assert!(
        messages.iter().any(|m| {
            m["role"] == "user"
                && m["content"]
                    .as_str()
                    .is_some_and(|c| c.starts_with(MIDRUN_NOTICE_MARKER))
        }),
        "the mid-run notice rides as its own user message, marked as not-a-user-message"
    );
    let wire = final_chat.to_string();
    assert!(
        !wire.contains("call_t02_read_0"),
        "the compacted old region left the wire"
    );
    assert!(
        wire.contains("call_t02_read_1"),
        "the retained suffix kept its calls"
    );
    assert_wire_pairs(final_chat, "post-compaction chat request");

    // 数据库对照：台账落一行 auxiliary.summarize（succeeded，身份是
    // RESOLVED 路由），chat 行 5 行全 succeeded。
    let db = open_runs_db(&h.home);
    let summary_rows = query_one(
        &db,
        "SELECT COUNT(*) FROM model_call_usage WHERE purpose = 'auxiliary.summarize' AND outcome = 'succeeded' AND model = 'stub-summarize-t02'",
        &[],
    );
    assert_eq!(
        summary_rows.as_deref(),
        Some("1"),
        "one succeeded summary ledger row"
    );
    let chat_rows = query_one(
        &db,
        "SELECT COUNT(*) FROM model_call_usage WHERE purpose = 'chat' AND outcome = 'succeeded'",
        &[],
    );
    assert_eq!(chat_rows.as_deref(), Some("5"), "every chat call accounted");

    let status = child.stop().await;
    assert!(status.success(), "graceful shutdown: {status}");
    stub.stop().await;
    let _ = std::fs::remove_dir_all(&h.home);
    let _ = std::fs::remove_dir_all(&h.workspace);
}

// ── A04：摘要 provider 返回错误 → 原历史续跑 + 失败落账 ───

#[tokio::test]
async fn a04_summary_provider_failure_keeps_the_original_history() {
    let chat_hits = Arc::new(AtomicUsize::new(0));
    let stub = StubServer::start(failing_summary_router("big.txt", Arc::clone(&chat_hits))).await;
    let h = Harness::new("a04", &stub.endpoint());
    std::fs::write(h.workspace.join("big.txt"), "seed\n").expect("seed big.txt");
    let child = h.start_service().await;
    let token = h.token();
    let addr = child.addr;

    let (status, accepted) = submit(addr, &token, "sess_local_alpha", "process big.txt").await;
    assert_eq!(status, 200, "submission accepted: {accepted}");
    let run_id = accepted["runId"].as_str().expect("runId").to_string();
    // A04：压缩失败永不中断 run——原始上下文续跑到 completed。
    let final_status = wait_run_status(&h.home, &run_id, |s| s == "completed", "completed").await;
    assert_eq!(final_status, "completed");

    let requests = stub.requests();
    let summary_requests: Vec<_> = requests
        .iter()
        .filter(|(_, _, body)| body.to_string().contains(INSTRUCTION_MARKER))
        .collect();
    let chat_requests: Vec<_> = requests
        .iter()
        .filter(|(_, _, body)| !body.to_string().contains(INSTRUCTION_MARKER))
        .collect();
    assert_eq!(summary_requests.len(), 1, "one summary attempt");
    assert_eq!(
        chat_requests.len(),
        5,
        "the run continued with the original history"
    );

    // 最后一次 chat 请求：无摘要包装（空/假摘要绝不覆盖历史），原始
    // 历史（含 call_t02_read_0）原样在线。
    let final_chat = &chat_requests[4].2;
    let messages = final_chat["messages"].as_array().expect("messages");
    assert!(
        !messages.iter().any(|m| {
            m["content"]
                .as_str()
                .is_some_and(|c| c.starts_with(SUMMARY_PREFIX))
        }),
        "no fabricated summary ever covers the history"
    );
    let wire = final_chat.to_string();
    assert!(
        wire.contains("call_t02_read_0"),
        "the original history rode the wire untouched"
    );
    assert_wire_pairs(final_chat, "failure-leg chat request");

    // 数据库对照：失败也落账（failed 行），chat 行完整。
    let db = open_runs_db(&h.home);
    let failed_rows = query_one(
        &db,
        "SELECT COUNT(*) FROM model_call_usage WHERE purpose = 'auxiliary.summarize' AND outcome = 'failed'",
        &[],
    );
    assert_eq!(
        failed_rows.as_deref(),
        Some("1"),
        "the failed summary call accounts honestly"
    );
    let chat_rows = query_one(
        &db,
        "SELECT COUNT(*) FROM model_call_usage WHERE purpose = 'chat' AND outcome = 'succeeded'",
        &[],
    );
    assert_eq!(chat_rows.as_deref(), Some("5"));

    let status = child.stop().await;
    assert!(status.success(), "graceful shutdown: {status}");
    stub.stop().await;
    let _ = std::fs::remove_dir_all(&h.home);
    let _ = std::fs::remove_dir_all(&h.workspace);
}
