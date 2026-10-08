//! R06-T01 二进制闭环腿（R06-A01 的进程级证明）：真实 `lingxi-service`
//! 二进制（真实 argv、真实 --config、loopback provider 替身）跑一次真实
//! 提交，然后断言——
//! - 替身捕获的线上请求里 messages[0]（system 槽）的字节，与
//!   `GET /lingxi/v1/sessions/{id}/context-observation` 观测视图的整文
//!   digest、逐段 digest 完全一致（一份构建结果，没有第二套 prompt 重建）；
//! - full 披露段的观测正文逐字节等于发送切片，digest-only 段在观测里没有
//!   正文、只有对发送字节算出的 sha256；
//! - 未发送过请求的会话观测 = 404；无凭证 = 401/403。
//!
//! 替身边界：stub 只扮演外部模型端点（读请求、回固定 SSE）；它从不决定
//! 运行/审批/存储状态。人格模板走真实回落链（exe 上溯命中仓库 lib/）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t01-bin-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create unique dir");
    dir
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, content).expect("write fixture");
}

// ── loopback provider 替身 ──

struct RecordedRequest {
    body: serde_json::Value,
}

struct StubServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start(sse_body: String) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                let recorded = read_one_request(&mut socket).await;
                task_requests.lock().expect("requests").push(recorded);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
                    sse_body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            addr,
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("requests").drain(..).collect()
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
        assert!(raw.len() < 1024 * 1024, "stub: header block too large");
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("stub utf8 headers");
    let mut content_length = None;
    for line in head.split("\r\n").skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().parse::<usize>().expect("content-length"));
            }
        }
    }
    let content_length = content_length.expect("json posts carry a content-length");
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 8192];
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
        body: serde_json::from_slice(&raw[body_start..body_start + content_length])
            .expect("stub: request body must be JSON"),
    }
}

/// openai-completions 的固定 final SSE（一帧内容 + stop + usage + DONE）。
fn final_turn_sse(text: &str) -> String {
    let mut body = String::new();
    for frame in [
        serde_json::json!({
            "id": "chatcmpl-r06t01",
            "model": "stub-model-r06t01",
            "choices": [{
                "index": 0,
                "finish_reason": null,
                "delta": {"role": "assistant", "content": text}
            }]
        }),
        serde_json::json!({
            "id": "chatcmpl-r06t01",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str(&format!(
        "data: {}\n\n",
        serde_json::json!({"id": "chatcmpl-r06t01", "choices": [], "usage": {"prompt_tokens": 40, "completion_tokens": 17}})
    ));
    body.push_str("data: [DONE]\n\n");
    body
}

// ── 真实二进制 harness ──

struct ServiceChild {
    child: tokio::process::Child,
    addr: SocketAddr,
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
                panic!("binary never printed LINGXI_SERVICE_READY within 30s\nstderr:\n{stderr}")
            })
            .expect("stdout read");
            if read == 0 {
                let stderr = stderr_lines.lock().expect("stderr log").join("\n");
                panic!("binary exited before readiness\nstderr:\n{stderr}");
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
            assert!(
                Instant::now() <= deadline,
                "the real binary did not exit within 30s of SIGTERM\nstderr:\n{}",
                self.stderr_lines.lock().expect("stderr log").join("\n")
            );
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

async fn http_json(
    addr: SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&serde_json::Value>,
) -> (u16, serde_json::Value) {
    let exchanged = tokio::time::timeout(Duration::from_secs(120), async {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to the real service");
        let payload = body.map(|b| b.to_string());
        let auth = bearer
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        let mut head =
            format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Connection: close\r\n");
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
    .expect("request to the real service stalled");
    let text = String::from_utf8_lossy(&exchanged);
    let (head, body) = text.split_once("\r\n\r\n").expect("header/body split");
    let status = head
        .split_whitespace()
        .nth(1)
        .expect("status")
        .parse::<u16>()
        .expect("numeric status");
    let json = if body.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(body).expect("response body JSON")
    };
    (status, json)
}

fn read_loopback_token(home: &Path) -> String {
    let canonical = std::fs::canonicalize(home).expect("canonical home");
    let token_path = canonical.join("lingxi-service").join("local-token.json");
    let file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&token_path).expect("loopback token file readable"))
            .expect("token file JSON");
    file["token"].as_str().expect("token field").to_string()
}

/// R06-A01 二进制腿：线上发送字节 ↔ 观测 digest 的机器对照。
#[tokio::test]
async fn sent_wire_system_bytes_match_the_observation_digests() {
    let stub = StubServer::start(final_turn_sse("二进制闭环应答")).await;
    let home = unique_dir("home");
    let workspace = unique_dir("ws");
    let config_root = unique_dir("cfg");
    let config_path = config_root.join("service.json");
    write(
        &config_path,
        &format!(
            r#"{{"home": {}, "workspace": {}, "providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": "http://{}",
                    "auth": {{"kind": "apiKey", "apiKey": "sk-test-r06t01"}}
                }}
            }},
            "models": {{"chat": {{"provider": "main", "model": "stub-model-r06t01", "capabilities": {{"tools": true}}}}}}}}"#,
            serde_json::to_string(&home.to_string_lossy()).expect("home json"),
            serde_json::to_string(&workspace.to_string_lossy()).expect("ws json"),
            stub.addr
        ),
    );
    // 注意：材料必须先于「首次执行」落盘、但必须晚于「首次启动」——数据
    // 纪元闸在启动时拒绝「未盖章却已有数据」的 home（unstamped-home-with-
    // data，外来数据领养是 R08 的显式割接动作），所以启动前的 home 必须
    // 保持空白；材料读取发生在首个 run 的编译点，启动后落盘即可被读到。
    // 材料根 = canonical home 本身（现役 $LINGXI_HOME 布局：agents/ 与
    // user/ 直接在根下）。
    let child = ServiceChild::start(&[
        "--home".to_string(),
        home.to_string_lossy().to_string(),
        "--config".to_string(),
        config_path.to_string_lossy().to_string(),
        "--bind".to_string(),
        "127.0.0.1:0".to_string(),
    ])
    .await;
    let token = read_loopback_token(&home);
    let service_home = std::fs::canonicalize(&home).expect("canonical home");
    write(
        &service_home.join("user/preferences.json"),
        r#"{"userName":"黎","locale":"zh-CN"}"#,
    );
    write(
        &service_home.join("user/user.md"),
        "PROFILE-BINARY-TOP_SECRET 二进制腿用户档案\n",
    );
    write(
        &service_home.join("agents/lingxi/config.yaml"),
        "locale: zh-CN\nagent:\n  name: 灵犀\n",
    );
    write(
        &service_home.join("agents/lingxi/memory/memory.md"),
        "MEMORY-BINARY-TOP_SECRET 二进制腿记忆\n",
    );

    // 无凭证的观测请求必须被认证层拒绝。
    let (status, _) = http_json(
        child.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/context-observation",
        None,
        None,
    )
    .await;
    assert!(
        status == 401 || status == 403,
        "无凭证观测必须被拒（实际 {status}）"
    );
    // 从未发送过请求的会话：观测 404（观测绝不现构第二份 prompt）。
    let (status, _) = http_json(
        child.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_beta/context-observation",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, 404, "未编译会话的观测必须是 404");

    let (status, accepted) = http_json(
        child.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        Some(&token),
        Some(&serde_json::json!({"input": "二进制闭环第一句"})),
    )
    .await;
    assert_eq!(status, 200, "execute 必须受理: {accepted}");

    let requests = stub.requests();
    assert_eq!(requests.len(), 1, "一次 final 应答 = 一次线上调用");
    let messages = requests[0].body["messages"]
        .as_array()
        .expect("wire messages");
    assert_eq!(
        messages[0]["role"].as_str().expect("role"),
        "system",
        "openai-completions 的第一条消息必须是 system 槽"
    );
    let sent = messages[0]["content"]
        .as_str()
        .expect("system content")
        .to_string();
    // 发送文本携带真实材料（人格回落到仓库真实模板 + 用户档案 + 记忆）。
    assert!(
        sent.contains("PROFILE-BINARY-TOP_SECRET"),
        "用户档案须到达线上"
    );
    assert!(sent.contains("MEMORY-BINARY-TOP_SECRET"), "记忆须到达线上");
    assert!(
        sent.contains("read-all_write-scoped_network-on"),
        "平台 note 须到达线上"
    );
    assert!(
        sent.contains(&format!("<cwd>\n{}\n</cwd>", workspace.to_string_lossy())),
        "cwd 包装段须携带配置的 workspace"
    );

    let (status, view) = http_json(
        child.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/context-observation",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, 200, "观测端点必须可读: {view}");
    assert_eq!(
        view["schema"].as_str(),
        Some("lingxi.context-observation.v1")
    );
    assert_eq!(view["spanUnit"].as_str(), Some("utf8-bytes"));
    assert_eq!(view["locale"].as_str(), Some("zh-CN"));
    assert_eq!(view["forSubagent"].as_bool(), Some(false));
    // 整文同源：观测 digest == 线上发送字节的 digest。
    assert_eq!(
        view["renderSha256"].as_str().expect("renderSha256"),
        sha256_hex(&sent),
        "观测整文 digest 必须等于线上发送字节"
    );
    // 逐段同源 + 披露纪律。
    let segments = view["segments"].as_array().expect("segments");
    let ids: Vec<&str> = segments.iter().map(|s| s["id"].as_str().unwrap()).collect();
    // 该材料形态下必须出现的段锚点（门控未开的段——appearance/
    // computer-use/learn-skills/roster/tenets——正确地缺席）。
    for required in [
        "platform.intro",
        "platform.environment",
        "user.profile",
        "persona",
        "memory.rules",
        "memory.longterm",
        "session.time",
        "session.cwd",
    ] {
        assert!(ids.contains(&required), "观测段序列缺 {required}: {ids:?}");
    }
    assert_eq!(ids.first(), Some(&"platform.intro"));
    assert_eq!(ids.last(), Some(&"session.cwd"));
    for segment in segments {
        let start = segment["byteStart"].as_u64().expect("byteStart") as usize;
        let end = segment["byteEnd"].as_u64().expect("byteEnd") as usize;
        let slice = &sent[start..end];
        assert_eq!(
            segment["sha256"].as_str().expect("segment sha256"),
            sha256_hex(slice),
            "段 {} 的观测 digest 与线上切片不一致",
            segment["id"]
        );
        if segment["disclosure"] == "full" {
            assert_eq!(
                segment["text"].as_str().expect("full segment text"),
                slice,
                "full 段 {} 的观测正文必须逐字节等于线上切片",
                segment["id"]
            );
        } else {
            assert!(
                segment.get("text").is_none(),
                "digest-only 段 {} 不得携带正文",
                segment["id"]
            );
        }
        assert!(
            segment["tokens"].as_u64().expect("tokens")
                <= segment["tokenBudget"].as_u64().expect("budget")
        );
    }
    let serialized = serde_json::to_string(&view).expect("serialize observation");
    for marker in ["PROFILE-BINARY-TOP_SECRET", "MEMORY-BINARY-TOP_SECRET"] {
        assert!(!serialized.contains(marker), "观测视图不得泄漏 {marker}");
    }

    let status = child.stop().await;
    stub.stop().await;
    assert!(status.success(), "优雅停机须成功: {status}");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&config_root);
}
