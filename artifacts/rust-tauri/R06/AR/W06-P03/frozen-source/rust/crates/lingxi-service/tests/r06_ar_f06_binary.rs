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

// R06-T06: this binary-level test references no crate code; the explicit
// reference force-links the default test guard (memory/time abort) anyway.
use lingxi_test_guard as _;

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
            .env("LINGXI_HOME", &args[1])
            .env("TEST_GUARD_VERBOSE", "1")
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
        let expected_home = std::fs::canonicalize(&args[1]).expect("合成 home 存在");
        assert!(ready_line.contains(&format!("home={} ", expected_home.display())));
        assert!(ready_line.contains("source=cli"));
        println!("verified_ready={ready_line}");
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

#[tokio::test]
async fn c01_c02_c03_c04_real_binary_authenticated_requests() {
    let cases = vec![
        ("block-disabled", Some("locale: zh-CN\nagent:\n  name: 灵犀\nmemory:\n  enabled: false\n"), Some(false), Some("灵犀")),
        ("flow-disabled", Some("locale: zh-CN\nagent: {name: 灵犀}\nmemory: {enabled: false}\n"), Some(false), Some("灵犀")),
        ("quoted-bom-crlf", Some("\u{feff}locale: zh-CN\r\nagent: {name: \"助手 #1: 中文 \\\"双引号\\\"\"} # 外部注释\r\nmemory: {enabled: false}\r\n"), Some(false), Some("助手 #1: 中文 \"双引号\"")),
        ("missing", None, Some(true), None),
        ("empty", Some(""), Some(true), None),
        ("partial", Some("agent: {name: 旧助手}\nunrelated: {enabled: false, nested: {name: 错误}}\n"), Some(true), Some("旧助手")),
        ("malformed", Some("memory: {enabled: false\n"), None, None),
        ("wrong-type", Some("memory: {enabled: \"false\"}\n"), None, None),
        ("duplicate", Some("memory: {enabled: false, enabled: true}\n"), None, None),
        ("duplicate-section", Some("memory: {enabled: false}\nmemory: {enabled: true}\n"), None, None),
    ];
    for (name, yaml, enabled, agent_name) in cases {
        let stub = StubServer::start(final_turn_sse("离线应答")).await;
        let home = unique_dir(name);
        let workspace = unique_dir("ws");
        let cfg_root = unique_dir("cfg");
        let cfg_path = cfg_root.join("service.json");
        write(&cfg_path, &serde_json::json!({"home": home, "workspace": workspace, "providers": {"main": {"protocol": "openai-completions", "endpoint": format!("http://{}",stub.addr), "auth": {"kind":"apiKey", "apiKey":"sk-synthetic-f06"}}}, "models": {"chat": {"provider":"main", "model":"stub-model-r06t01", "capabilities":{"tools":true}}}}).to_string());
        let child = ServiceChild::start(&[
            "--home".into(),
            home.to_string_lossy().into(),
            "--config".into(),
            cfg_path.to_string_lossy().into(),
            "--bind".into(),
            "127.0.0.1:0".into(),
        ])
        .await;
        let effective = std::fs::canonicalize(&home).unwrap();
        println!(
            "case={name} effective_LINGXI_HOME={} binary_pid={}",
            effective.display(),
            child.child.id().unwrap()
        );
        let token = read_loopback_token(&home);
        let agent_config = effective.join("agents/lingxi/config.yaml");
        if let Some(yaml) = yaml {
            write(&agent_config, yaml);
        }
        write(
            &effective.join("agents/lingxi/memory/memory.md"),
            "MEMORY-F06-NONCE-private-synthetic",
        );
        write(
            &effective.join("user/preferences.json"),
            r#"{"locale":"zh-CN"}"#,
        );
        let before = std::fs::read(&agent_config).ok();
        let (status, response) = http_json(
            child.addr,
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(&token),
            Some(&serde_json::json!({"input":"请正常回答"})),
        )
        .await;
        assert_eq!(status, 200, "execute受理结果: {response}");
        let requests = stub.requests();
        if let Some(enabled) = enabled {
            assert_eq!(requests.len(), 1, "{name} 必须真实发模型请求");
            let sent = requests[0].body["messages"][0]["content"].as_str().unwrap();
            assert_eq!(
                sent.contains("MEMORY-F06-NONCE-private-synthetic"),
                enabled,
                "{name} 的记忆开关"
            );
            if let Some(agent_name) = agent_name {
                assert!(sent.contains(agent_name), "{name} 的名字必须保真");
            }
            let (view_status, view) = http_json(
                child.addr,
                "GET",
                "/lingxi/v1/sessions/sess_local_alpha/context-observation",
                Some(&token),
                None,
            )
            .await;
            assert_eq!(view_status, 200);
            assert_eq!(view["renderSha256"].as_str().unwrap(), sha256_hex(sent));
            println!("case={name} request={}", requests[0].body);
            let (flags_status, flags) = http_json(
                child.addr,
                "GET",
                "/lingxi/v1/agents/lingxi/config-flags",
                Some(&token),
                None,
            )
            .await;
            assert_eq!(flags_status, 200);
            assert_eq!(flags["memoryEnabled"].as_bool(), Some(enabled));
        } else {
            assert!(requests.is_empty(), "{name} 配置错误不得发送默认开启请求");
            let database = rusqlite::Connection::open_with_flags(
                effective.join("lingxi-service/data/runs.db"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let (run_status, reason): (String, String) = database
                .query_row(
                    "SELECT status, terminal_reason FROM runs WHERE run_id=?1",
                    [response["runId"].as_str().unwrap()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(run_status, "failed");
            // 运行终态使用现行固定分类，具体配置诊断由配置接口核对。
            assert_eq!(reason, "failed.provider_error");
            println!("case={name} failed_run={response} db_status={run_status} reason={reason}");
            let (flags_status, flags) = http_json(
                child.addr,
                "GET",
                "/lingxi/v1/agents/lingxi/config-flags",
                Some(&token),
                None,
            )
            .await;
            println!("case={name} flags_error={flags}");
            assert_eq!(flags_status, 400, "{name} 配置错误必须明确诊断 {flags}");
            assert!(flags["error"].as_str().unwrap().contains("config.yaml"));
            let (feedback_status, feedback) = http_json(
                child.addr,
                "POST",
                "/lingxi/v1/agents/lingxi/experience/feedback",
                Some(&token),
                Some(&serde_json::json!({"rating":"up","excerpt":"不得写入的反馈"})),
            )
            .await;
            assert_eq!(
                feedback_status, 400,
                "损坏配置不能被反馈入口吞掉: {feedback}"
            );
            assert!(
                !effective.join("agents/lingxi/experience").exists(),
                "拒绝反馈后无写入"
            );
            let (persona_status, persona) = http_json(
                child.addr,
                "GET",
                "/lingxi/v1/agents/lingxi/identity",
                Some(&token),
                None,
            )
            .await;
            assert_eq!(persona_status, 400, "人格读取不能吞配置错误 {persona}");
        }
        assert_eq!(
            before,
            std::fs::read(&agent_config).ok(),
            "读取不允许重写用户配置"
        );
        assert!(child.stop().await.success());
        stub.stop().await;
        std::fs::remove_dir_all(home).unwrap();
        std::fs::remove_dir_all(workspace).unwrap();
        std::fs::remove_dir_all(cfg_root).unwrap();
    }
}

#[tokio::test]
async fn p01_owned_complex_keys_fail_closed_across_wire_and_shared_preview() {
    let cases = [
        ("plain-control", "locale: zh-CN\nagent: {name: 'P01姓名 # 中文'}\nmemory: {enabled: false}\nskills: {enabled: [shared-p01]}\nworkspace_context: {discover_project_skills: false, discover_compatible_project_skills: false}\n", false),
        ("root-memory", "? [memory]\n: {enabled: false}\n", true),
        ("field-memory", "memory:\n  ? [enabled]\n  : false\n", true),
        ("root-agent", "? [agent]\n: {name: 'P01姓名 # 中文'}\nmemory: {enabled: false}\n", true),
        ("field-name", "agent:\n  ? [name]\n  : 'P01姓名 # 中文'\nmemory: {enabled: false}\n", true),
        ("field-yuan", "agent:\n  ? [yuan]\n  : custom\nmemory: {enabled: false}\n", true),
        ("root-locale", "? [locale]\n: en\nmemory: {enabled: false}\n", true),
        ("field-experience", "experience:\n  ? [enabled]\n  : false\nmemory: {enabled: false}\n", true),
        ("field-discover", "workspace_context:\n  ? [discover_project_skills]\n  : false\nmemory: {enabled: false}\n", true),
        ("field-compatible", "workspace_context:\n  ? [discover_compatible_project_skills]\n  : false\nmemory: {enabled: false}\n", true),
        ("field-skills", "skills:\n  ? [enabled]\n  : [shared-p01]\nmemory: {enabled: false}\n", true),
        ("field-workspace-disabled", "skills:\n  ? [workspace_disabled]\n  : [project-p01]\nmemory: {enabled: false}\n", true),
        ("root-normalized-duplicate", "memory: {enabled: false}\n? [memory]\n: {enabled: true}\n", true),
        ("field-normalized-duplicate", "memory: {enabled: false, ? [enabled]: true}\n", true),
        ("anchor-owned", "defaults: &d {? [enabled]: false}\nmemory: *d\n", true),
        ("merge-owned", "defaults: &d {? [enabled]: false}\nmemory: {<<: *d}\n", true),
        ("merge-root", "defaults: &d {? [memory]: {enabled: false}}\n<<: *d\n", true),
        ("merge-overridden", "defaults: &d {memory: {? [enabled]: false}}\n<<: *d\nmemory: {enabled: false}\n", true),
    ];
    let mut failures = Vec::new();
    for (name, yaml, reject) in cases {
        let stub = StubServer::start(final_turn_sse("离线 P01")).await;
        let home = unique_dir(name);
        let workspace = unique_dir("p01-ws");
        let cfg_root = unique_dir("p01-cfg");
        let cfg_path = cfg_root.join("service.json");
        write(&cfg_path, &serde_json::json!({"home":home,"workspace":workspace,"providers":{"main":{"protocol":"openai-completions","endpoint":format!("http://{}",stub.addr),"auth":{"kind":"apiKey","apiKey":"synthetic-p01"}}},"models":{"chat":{"provider":"main","model":"stub-p01","capabilities":{"tools":true}}}}).to_string());
        let child = ServiceChild::start(&[
            "--home".into(),
            home.to_string_lossy().into(),
            "--config".into(),
            cfg_path.to_string_lossy().into(),
            "--bind".into(),
            "127.0.0.1:0".into(),
        ])
        .await;
        let effective = std::fs::canonicalize(&home).unwrap();
        let token = read_loopback_token(&home);
        let config = effective.join("agents/lingxi/config.yaml");
        write(&config, yaml);
        write(
            &effective.join("agents/lingxi/memory/memory.md"),
            "PRIVATE-P01-MEMORY",
        );
        let shared = effective.join("skills/shared-p01/SKILL.md");
        write(
            &shared,
            "---\nname: shared-p01\ndescription: shared P01\n---\nSHARED-P01-BODY",
        );
        write(
            &effective.join("agents/peer-p01/config.yaml"),
            "skills: {enabled: [shared-p01]}\n",
        );
        write(
            &workspace.join(".agents/skills/project-p01/SKILL.md"),
            "---\nname: project-p01\ndescription: PRIVATE-P01-PROJECT\n---\nSynthetic material",
        );
        let flags = http_json(
            child.addr,
            "GET",
            "/lingxi/v1/agents/lingxi/config-flags",
            Some(&token),
            None,
        )
        .await;
        let preview = http_json(
            child.addr,
            "GET",
            "/lingxi/v1/agents/lingxi/skills/cleanup-preview",
            Some(&token),
            None,
        )
        .await;
        let execute = http_json(
            child.addr,
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(&token),
            Some(&serde_json::json!({"input":"回答"})),
        )
        .await;
        let requests = stub.requests();
        let database = rusqlite::Connection::open_with_flags(
            effective.join("lingxi-service/data/runs.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let terminal: String = database
            .query_row(
                "SELECT status FROM runs WHERE run_id=?1",
                [execute.1["runId"].as_str().unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        let ok = if reject {
            flags.0 == 400 && preview.0 == 400 && requests.is_empty() && terminal == "failed"
        } else {
            let sent = requests
                .first()
                .and_then(|r| r.body["messages"][0]["content"].as_str())
                .unwrap_or("");
            flags.0 == 200
                && flags.1["memoryEnabled"] == false
                && flags.1["workspaceContext"]["discoverProjectSkills"] == false
                && preview.0 == 200
                && preview.1["skills"] == serde_json::json!([])
                && requests.len() == 1
                && terminal == "completed"
                && !sent.contains("PRIVATE-P01-MEMORY")
                && !sent.contains("PRIVATE-P01-PROJECT")
                && sent.contains("P01姓名 # 中文")
        };
        println!("p01_case={name} effective_LINGXI_HOME={} expected_reject={reject} flags={flags:?} preview={preview:?} terminal={terminal} wire={:?} matched={ok}",effective.display(),requests.iter().map(|r| &r.body).collect::<Vec<_>>());
        if !ok {
            failures.push(name);
        }
        assert_eq!(std::fs::read_to_string(&config).unwrap(), yaml);
        assert!(shared.is_file(), "仅做预览，不能删除共享技能");
        if !reject {
            // 其它助手的复合键必须让预览明确失败，而不是误列仍在使用的技能。
            let peer = effective.join("agents/peer-p01/config.yaml");
            let peer_yaml = "skills:\n  ? [enabled]\n  : [shared-p01]\n";
            write(&peer, peer_yaml);
            let peer_preview = http_json(
                child.addr,
                "GET",
                "/lingxi/v1/agents/lingxi/skills/cleanup-preview",
                Some(&token),
                None,
            )
            .await;
            println!("p01_case=peer-complex preview={peer_preview:?}");
            if peer_preview.0 != 400 {
                failures.push("peer-complex");
            }
            assert_eq!(std::fs::read_to_string(&peer).unwrap(), peer_yaml);
            assert!(shared.is_file());
        }
        drop(database);
        assert!(child.stop().await.success());
        stub.stop().await;
        std::fs::remove_dir_all(home).unwrap();
        std::fs::remove_dir_all(workspace).unwrap();
        std::fs::remove_dir_all(cfg_root).unwrap();
    }
    assert!(failures.is_empty(), "全族真实入口未失败关闭: {failures:?}");
}

#[tokio::test]
async fn p02_literal_keys_tags_and_config_roundtrip_use_real_wire_and_http() {
    let cases = [
        (
            "literal-memory",
            "memory: {\"<<\": {enabled: false}}",
            false,
            true,
            false,
            true,
        ),
        (
            "literal-explicit",
            "memory: {!!str <<: {enabled: false}}",
            false,
            true,
            false,
            true,
        ),
        (
            "literal-root",
            "memory: {enabled: false}\n\"<<\": 7",
            false,
            false,
            false,
            true,
        ),
        (
            "literal-future",
            "memory: {enabled: false}\nfuture: {\"<<\": {marker: keep}, other: value}",
            false,
            false,
            false,
            true,
        ),
        (
            "literal-experience",
            "memory: {enabled: false}\nexperience: {\"<<\": {enabled: true}}",
            false,
            false,
            false,
            true,
        ),
        (
            "literal-project",
            "memory: {enabled: false}\nworkspace_context: {\"<<\": {discover_project_skills: false}}",
            false,
            false,
            false,
            true,
        ),
        (
            "literal-name",
            "memory: {enabled: false}\nagent: {\"<<\": {name: P02-NAME-MUST-NOT-LIFT}}",
            false,
            false,
            false,
            true,
        ),
        (
            "true-merge",
            "a: &a {enabled: true}\nb: &b {enabled: false}\nmemory: {<<: [*a,*b], enabled: false}\nexperience: {!!merge <<: *a}",
            false,
            false,
            true,
            true,
        ),
        (
            "standard-tags",
            "locale: !!str en\nagent: !!map {name: !!str P02-STANDARD-NAME, yuan: !!str lingxi}\nmemory: {enabled: !!bool FALSE}\nskills: {enabled: !!seq [!!str shared-p02]}",
            false,
            false,
            false,
            true,
        ),
        (
            "tag-root",
            "!custom {memory: {enabled: false}}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-memory",
            "memory: !custom {enabled: true}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-bool",
            "memory: {enabled: !custom true}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-experience",
            "experience: {enabled: !custom true}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-name",
            "agent: {name: !custom P02-NAME}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-yuan",
            "agent: {yuan: !custom lingxi}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-locale",
            "locale: !custom en",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-skills",
            "skills: !custom {enabled: [shared-p02]}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-list",
            "skills: {enabled: !custom [shared-p02]}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-list-entry",
            "skills: {enabled: [!custom shared-p02]}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-disabled-entry",
            "skills: {workspace_disabled: [!custom project-p02]}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-project",
            "workspace_context: {discover_project_skills: !custom false}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-compatible",
            "workspace_context: {discover_compatible_project_skills: !custom false}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-unknown",
            "future: {nested: !custom {value: keep}}",
            true,
            false,
            false,
            false,
        ),
        (
            "tag-overridden",
            "a: &a {enabled: !custom true}\nmemory: {<<: *a, enabled: false}",
            true,
            false,
            false,
            false,
        ),
    ];
    let resource_depth = format!("future: {}x{}", "[".repeat(65), "]".repeat(65));
    let mut resource_alias = String::from("a0: &a0 [ok]\n");
    for i in 1..20 {
        resource_alias.push_str(&format!(
            "a{i}: &a{i} [*a{}, *a{}, *a{}, *a{}]\n",
            i - 1,
            i - 1,
            i - 1,
            i - 1
        ));
    }
    let cases=cases.into_iter().chain([
        ("p03-depth-budget",resource_depth.as_str(),true,false,false,false),
        ("p03-alias-budget",resource_alias.as_str(),true,false,false,false),
        ("p03-local-bool","memory: {enabled: !bool true}",true,false,false,false),
        ("p03-local-name","agent: {name: !str 姓名}",true,false,false,false),
        ("p03-local-map","memory: !map {enabled: false}",true,false,false,false),
        ("p03-binary-name","agent: {name: !!binary SGVsbG8=}",true,false,false,false),
        ("p03-date-name","agent: {name: 2026-10-11}",true,false,false,false),
        ("p03-source-wrong-bool","d: &d {enabled: 'true'}\nmemory: {<<: *d, enabled: false}",true,false,false,false),
        ("p03-standard-unknown","memory: {enabled: false}\nfuture: {data: !!binary /wABAA==, date: 2026-10-11, set: !!set {one: null}, pairs: !!pairs [{one: 1}]}",false,false,false,true),
    ]);
    for (name, yaml, reject, memory, experience, project) in cases {
        let stub = StubServer::start(final_turn_sse("离线 P02")).await;
        let root = unique_dir(name);
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        let cfg = root.join("service.json");
        write(&cfg, &serde_json::json!({"home":home,"workspace":workspace,"providers":{"main":{"protocol":"openai-completions","endpoint":format!("http://{}",stub.addr),"auth":{"kind":"apiKey","apiKey":"synthetic-p02"}}},"models":{"chat":{"provider":"main","model":"stub-p02","capabilities":{"tools":true}}}}).to_string());
        let child = ServiceChild::start(&[
            "--home".into(),
            home.to_string_lossy().into(),
            "--config".into(),
            cfg.to_string_lossy().into(),
            "--bind".into(),
            "127.0.0.1:0".into(),
        ])
        .await;
        let effective = std::fs::canonicalize(&home).unwrap();
        let token = read_loopback_token(&home);
        let config = effective.join("agents/lingxi/config.yaml");
        write(&config, yaml);
        write(
            &effective.join("agents/lingxi/memory/memory.md"),
            "P02-MEMORY-NONCE",
        );
        write(
            &effective.join("agents/lingxi/experience/p02.md"),
            "P02-EXPERIENCE-NONCE",
        );
        let shared = effective.join("skills/shared-p02/SKILL.md");
        write(
            &shared,
            "---\nname: shared-p02\ndescription: P02 shared\n---\nSynthetic",
        );
        write(
            &effective.join("agents/peer-p02/config.yaml"),
            "skills: {\"<<\": {enabled: [shared-p02]}}",
        );
        write(
            &workspace.join(".agents/skills/project-p02/SKILL.md"),
            "---\nname: project-p02\ndescription: P02-PROJECT-NONCE\n---\nSynthetic",
        );
        let database = rusqlite::Connection::open_with_flags(
            effective.join("lingxi-service/data/runs.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let flags = http_json(
            child.addr,
            "GET",
            "/lingxi/v1/agents/lingxi/config-flags",
            Some(&token),
            None,
        )
        .await;
        let experience_read = http_json(
            child.addr,
            "GET",
            "/lingxi/v1/agents/lingxi/experience",
            Some(&token),
            None,
        )
        .await;
        let execute = http_json(
            child.addr,
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(&token),
            Some(&serde_json::json!({"input":"回答"})),
        )
        .await;
        let requests = stub.requests();
        let terminal: String = database
            .query_row(
                "SELECT status FROM runs WHERE run_id=?1",
                [execute.1["runId"].as_str().unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(std::fs::read_to_string(&config).unwrap(), yaml);
        println!(
            "p02_case={name} input_sha={} flags={flags:?} experience={experience_read:?} terminal={terminal} wire={:?}",
            sha256_hex(yaml),
            requests.iter().map(|r| &r.body).collect::<Vec<_>>()
        );
        if reject {
            assert_eq!(flags.0, 400, "{name}");
            assert_eq!(terminal, "failed", "{name}");
            assert!(requests.is_empty(), "{name}");
            let experience_before =
                std::fs::read(effective.join("agents/lingxi/experience/p02.md")).unwrap();
            for (method, url, body) in [
                ("GET", "/lingxi/v1/agents/lingxi/identity", None),
                (
                    "PUT",
                    "/lingxi/v1/agents/lingxi/experience",
                    Some(serde_json::json!({"content":"# 不得写入\n错误配置"})),
                ),
                (
                    "POST",
                    "/lingxi/v1/agents/lingxi/experience/feedback",
                    Some(serde_json::json!({"rating":"up","excerpt":"不得写入"})),
                ),
                (
                    "PUT",
                    "/lingxi/v1/agents/lingxi/skills",
                    Some(serde_json::json!({"enabled":["shared-p02"]})),
                ),
                (
                    "PATCH",
                    "/lingxi/v1/agents/lingxi/skills/shared-p02",
                    Some(serde_json::json!({"enabled":true})),
                ),
            ] {
                let result = http_json(child.addr, method, url, Some(&token), body.as_ref()).await;
                println!(
                    "p03_reject_consumer name={name} method={method} path={url} receipt={result:?}"
                );
                assert_eq!(result.0, 400, "{name} {url}");
                assert_eq!(std::fs::read_to_string(&config).unwrap(), yaml);
                assert_eq!(
                    std::fs::read(effective.join("agents/lingxi/experience/p02.md")).unwrap(),
                    experience_before
                );
            }
        } else {
            assert_eq!(flags.0, 200, "{name}");
            assert_eq!(flags.1["memoryEnabled"], memory, "{name}");
            assert_eq!(flags.1["experienceEnabled"], experience, "{name}");
            assert_eq!(
                experience_read.0,
                if experience { 200 } else { 403 },
                "{name}"
            );
            if experience {
                assert!(format!("{:?}", experience_read.1).contains("P02-EXPERIENCE-NONCE"));
            }
            assert_eq!(terminal, "completed", "{name}");
            assert_eq!(requests.len(), 1, "{name}");
            let sent = requests[0].body["messages"][0]["content"].as_str().unwrap();
            assert_eq!(sent.contains("P02-MEMORY-NONCE"), memory, "{name}");
            assert_eq!(sent.contains("P02-PROJECT-NONCE"), project, "{name}");
            assert!(!sent.contains("P02-NAME-MUST-NOT-LIFT"), "{name}");
            if name == "standard-tags" {
                assert!(sent.contains("P02-STANDARD-NAME"));
                let preview = http_json(
                    child.addr,
                    "GET",
                    "/lingxi/v1/agents/lingxi/skills/cleanup-preview",
                    Some(&token),
                    None,
                )
                .await;
                assert_eq!(preview.0, 200);
                assert_eq!(preview.1["skills"][0]["name"], "shared-p02");
                write(
                    &effective.join("agents/peer-p02/config.yaml"),
                    "skills: {enabled: [!custom shared-p02]}",
                );
                let preview = http_json(
                    child.addr,
                    "GET",
                    "/lingxi/v1/agents/lingxi/skills/cleanup-preview",
                    Some(&token),
                    None,
                )
                .await;
                assert_eq!(preview.0, 400);
                write(
                    &effective.join("agents/peer-p02/config.yaml"),
                    "skills: {\"<<\": {enabled: [shared-p02]}}",
                );
            }
        }
        let patch = http_json(
            child.addr,
            "PATCH",
            "/lingxi/v1/agents/lingxi/config",
            Some(&token),
            Some(&serde_json::json!({"memory":{"enabled":false}})),
        )
        .await;
        let saved = std::fs::read_to_string(&config).unwrap();
        println!("p02_case={name} patch={patch:?} original={yaml:?} saved={saved:?}");
        if reject {
            assert_eq!(patch.0, 400, "{name}");
            assert_eq!(saved, yaml, "{name}");
        } else {
            assert_eq!(patch.0, 200, "{name}");
            // 业务再次读取检查默认、列表和深层字面键；完整未知值相等检查在适配层及独立现役语义探针。
            if name.starts_with("literal-") {
                assert!(saved.contains("<<"), "{name}: 字面键必须保留");
            }
            if name == "literal-future" {
                assert!(saved.contains("marker: keep"));
                assert!(saved.contains("other: value"));
            }
            let reread = http_json(
                child.addr,
                "GET",
                "/lingxi/v1/agents/lingxi/config-flags",
                Some(&token),
                None,
            )
            .await;
            assert_eq!(reread.0, 200, "{name}");
            assert_eq!(reread.1["memoryEnabled"], false, "{name}");
            assert_eq!(reread.1["experienceEnabled"], experience, "{name}");
            // 保存后的新会话必须重新采集配置；同会话缓存失效属于仍待修复的 F05。
            let created = http_json(child.addr,"POST","/lingxi/v1/sessions",Some(&token),Some(&serde_json::json!({"sessionId":"p02-reread","agentId":"lingxi","title":"保存后读取"}))).await;
            assert_eq!(created.0, 201, "{name}");
            let execute = http_json(
                child.addr,
                "POST",
                "/lingxi/v1/sessions/p02-reread/execute",
                Some(&token),
                Some(&serde_json::json!({"input":"保存后再次读取"})),
            )
            .await;
            let requests = stub.requests();
            assert_eq!(requests.len(), 1, "{name}");
            assert!(
                !requests[0].body["messages"][0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("P02-MEMORY-NONCE"),
                "{name}"
            );
            let terminal: String = database
                .query_row(
                    "SELECT status FROM runs WHERE run_id=?1",
                    [execute.1["runId"].as_str().unwrap()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(terminal, "completed", "{name}");
        }
        assert!(shared.is_file());
        drop(database);
        assert!(child.stop().await.success());
        stub.stop().await;
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn p03_delete_receipts_match_real_files_and_preflight() {
    for mode in [
        "normal",
        "bad-peer",
        "peer-directory",
        "bad-bundles",
        "bundle-directory",
        "late-write",
        "unreadable-agents",
    ] {
        let root = unique_dir(&format!("p03-delete-{mode}"));
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        let cfg = root.join("service.json");
        write(
            &cfg,
            &serde_json::json!({"home":home,"workspace":workspace}).to_string(),
        );
        let child = ServiceChild::start(&[
            "--home".into(),
            home.to_string_lossy().into(),
            "--config".into(),
            cfg.to_string_lossy().into(),
            "--bind".into(),
            "127.0.0.1:0".into(),
        ])
        .await;
        let token = read_loopback_token(&home);
        let config = home.join("agents/lingxi/config.yaml");
        let original="skills: {enabled: [p03-owned]}\nfuture: {payload: !!binary /wABAA==, date: 2026-10-11}";
        write(&config, original);
        let skill = home.join("skills/p03-owned/SKILL.md");
        write(
            &skill,
            "---\nname: p03-owned\ndescription: synthetic\n---\nOnly owned synthetic data",
        );
        let stranger = home.join("skills/p03-stranger/SKILL.md");
        write(
            &stranger,
            "---\nname: p03-stranger\ndescription: shared synthetic\n---\nMust stay unchanged",
        );
        let stranger_before = std::fs::read(&stranger).unwrap();
        let peer = home.join("agents/peer/config.yaml");
        if mode == "peer-directory" {
            std::fs::create_dir_all(&peer).unwrap();
        } else {
            write(
                &peer,
                if mode == "bad-peer" {
                    "skills: {enabled: !seq [p03-owned]}"
                } else {
                    "skills: {enabled: [p03-owned]}"
                },
            );
        }
        let bundles = home.join("skill-bundles.json");
        if mode == "bad-bundles" {
            write(&bundles, "{ invalid");
        } else if mode == "bundle-directory" {
            std::fs::create_dir_all(&bundles).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if mode == "late-write" {
                std::fs::set_permissions(
                    peer.parent().unwrap(),
                    std::fs::Permissions::from_mode(0o555),
                )
                .unwrap();
            }
            if mode == "unreadable-agents" {
                std::fs::set_permissions(
                    home.join("agents"),
                    std::fs::Permissions::from_mode(0o000),
                )
                .unwrap();
            }
        }
        let receipt = http_json(
            child.addr,
            "DELETE",
            "/lingxi/v1/skills/p03-owned?agentId=lingxi",
            Some(&token),
            None,
        )
        .await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(home.join("agents"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
            std::fs::set_permissions(
                peer.parent().unwrap(),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let after = std::fs::read_to_string(&config).unwrap();
        let exists = skill.exists();
        println!("p03_delete_wire mode={mode} input_sha={} receipt={receipt:?} skill_exists={exists} saved={after:?}",sha256_hex(original));
        assert_eq!(
            std::fs::read(&stranger).unwrap(),
            stranger_before,
            "不能误删或改写陌生共享资料"
        );
        if mode == "normal" {
            assert_eq!(receipt.0, 200);
            assert!(!exists);
            assert!(!after.contains("p03-owned"));
        } else if mode == "late-write" {
            assert_eq!(receipt.0, 500);
            assert!(!exists);
            assert_eq!(receipt.1["code"], "SKILL_DELETED_REFERENCE_CLEANUP_FAILED");
            assert_eq!(
                std::fs::read_to_string(&peer).unwrap(),
                "skills: {enabled: [p03-owned]}"
            );
        } else {
            assert!(receipt.0 >= 400);
            assert!(exists);
            assert_eq!(after, original);
        }
        assert!(child.stop().await.success());
        std::fs::remove_dir_all(root).unwrap();
    }
}
