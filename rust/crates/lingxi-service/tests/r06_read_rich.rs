//! read 的真实文档/图片消费与拒绝、取消、重启证明。
//! 仅外部供应商使用真实 loopback 替身。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r06-ex01-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));

    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn config_for(home: &Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Env,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

struct ApiClient {
    addr: SocketAddr,
    token: String,
}
struct TestServer {
    api: ApiClient,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn serve(state: ServiceState) -> TestServer {
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        api: ApiClient { addr, token },
        stop: stop_tx,
        handle,
    }
}

impl std::ops::Deref for TestServer {
    type Target = ApiClient;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl ApiClient {
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let (status, _, body) = self.request_raw(method, path, body).await;
        if std::env::var("R06T07_DEBUG").is_ok() {
            eprintln!(
                "DEBUG {method} {path} -> {status}: {}",
                String::from_utf8_lossy(&body)
            );
        }
        let parsed = if body.is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(_) => {
                    eprintln!(
                        "DEBUG non-JSON body for {method} {path}: {}",
                        String::from_utf8_lossy(&body)
                    );
                    Value::Null
                }
            }
        };
        (status, parsed)
    }

    async fn request_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            self.token,
            payload.len()
        );
        let mut stream = TcpStream::connect(self.addr).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut buf)).await;
        let header_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("http header end");
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let headers: Vec<(String, String)> = head
            .split("\r\n")
            .skip(1)
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            })
            .collect();
        (status, headers, buf[header_end + 4..].to_vec())
    }
}

impl TestServer {
    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
    }
}

struct Remote {
    addr: SocketAddr,
    captured: Arc<Mutex<Vec<Value>>>,
    fail: Arc<AtomicBool>,
    hold: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    task: tokio::task::JoinHandle<()>,
}
impl Remote {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let fail = Arc::new(AtomicBool::new(false));
        let hold = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let (c, f, h, e, r) = (
            captured.clone(),
            fail.clone(),
            hold.clone(),
            entered.clone(),
            release.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (c, f, h, e, r) = (c.clone(), f.clone(), h.clone(), e.clone(), r.clone());
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let (start, size) = loop {
                        let mut buf = [0; 65536];
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if let Some(p) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&raw[..p]);
                            let size = head
                                .lines()
                                .find_map(|line| {
                                    line.split_once(':')
                                        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                        .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            break (p + 4, size);
                        }
                    };
                    while raw.len() < start + size {
                        let mut buf = [0; 65536];
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                    }
                    let body: Value = serde_json::from_slice(&raw[start..start + size]).unwrap();
                    c.lock().unwrap().push(body.clone());
                    let vision = body["model"] == "vision";
                    if vision && h.load(Ordering::SeqCst) {
                        e.add_permits(1);
                        r.acquire().await.unwrap().forget();
                    }
                    if vision && f.load(Ordering::SeqCst) {
                        let text = json!({"error":{"message":"READ-VISION-REAL-400"}}).to_string();
                        let _=socket.write_all(format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",text.len()).as_bytes()).await;
                        return;
                    }
                    let messages = body["messages"].as_array().unwrap();
                    let current = messages.iter().rposition(|m| m["role"] == "user").unwrap();
                    let tool_count = messages[current + 1..]
                        .iter()
                        .filter(|m| m["role"] == "tool")
                        .count();
                    let mut payload = String::new();
                    if !vision && tool_count < 2 {
                        let input = messages.iter().rev().find(|m| m["role"] == "user").unwrap()
                            ["content"]
                            .as_str()
                            .unwrap()
                            .to_string();
                        let raw_path = input.strip_prefix("READ-PATH:").unwrap();
                        let (name, args, id) = if tool_count == 0 {
                            ("mcp_describe_tool", json!({"name":"read"}), "read-describe")
                        } else {
                            (
                                "mcp_call",
                                json!({"tool":"read","arguments":{"path":raw_path}}),
                                "read-file",
                            )
                        };
                        payload.push_str(&format!("data: {}\n\ndata: {}\n\n",json!({"id":"read","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]}),json!({"id":"read","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})));
                    } else {
                        let answer = if vision {
                            "VISION-ACTUAL-BLACK-PIXELS"
                        } else {
                            "READ-NEXT-MODEL-CONSUMED"
                        };
                        payload.push_str(&format!("data: {}\n\ndata: {}\n\n",json!({"id":"read","choices":[{"index":0,"delta":{"role":"assistant","content":answer},"finish_reason":null}]}),json!({"id":"read","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})));
                    }
                    payload.push_str(&format!("data: {}\n\ndata: [DONE]\n\n",json!({"id":"read","choices":[],"usage":{"prompt_tokens":13,"completion_tokens":7}})));
                    let _=socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).as_bytes()).await;
                });
            }
        });
        Self {
            addr,
            captured,
            fail,
            hold,
            entered,
            release,
            task,
        }
    }
    fn captures(&self) -> Vec<Value> {
        self.captured.lock().unwrap().clone()
    }
    fn visions(&self) -> usize {
        self.captures()
            .iter()
            .filter(|v| v["model"] == "vision")
            .count()
    }
}
struct Harness {
    server: TestServer,
    state: ServiceState,
    home: PathBuf,
    workspace: PathBuf,
}
async fn boot(home: PathBuf, workspace: PathBuf, remote: &Remote, vision: bool) -> Harness {
    let layout = prepare_layout(&home).unwrap();
    let agent = home.join("agents/lingxi");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("config.yaml"),"locale: zh-CN\nagent:\n  name: Read\n  yuan: lingxi\nmemory:\n  enabled: false\ncompaction:\n  enabled: false\n").unwrap();
    let product = home.join("product");
    std::fs::create_dir_all(product.join("yuan")).unwrap();
    std::fs::write(product.join("yuan/lingxi.md"), "You are {{agentName}}.").unwrap();
    let mut config = json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":format!("http://{}/v1",remote.addr),"auth":{"kind":"apiKey","apiKey":"READ-SYNTHETIC"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}}}});
    if vision {
        config["models"]["vision"] = json!({"provider":"remote","model":"vision","capabilities":{"imageInput":true},"compat":{"contextWindow":200000}});
    }
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::from_value(config).unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let compiler = Arc::new(
        lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
            lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                &home,
                product,
                lingxi_service::context_compiler::HostFacts {
                    platform: std::env::consts::OS.into(),
                    os_type: std::env::consts::OS.into(),
                    os_release: "test-host".into(),
                    shell_label: "sh".into(),
                    workspace: workspace.display().to_string(),
                },
            ),
        )),
    );
    let state = ServiceState::bootstrap_with_deps(
        config_for(&home),
        &layout,
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            credential_service: Some(credentials),
            context_compiler: Some(compiler),
            workspace_root: Some(workspace.clone()),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap();
    let server = serve(state.clone()).await;
    Harness {
        server,
        state,
        home,
        workspace,
    }
}
impl Harness {
    async fn execute(&self, sid: &str, path: &Path) -> String {
        let (status, body) = self
            .server
            .request(
                "POST",
                "/lingxi/v1/sessions",
                Some(json!({"sessionId":sid,"agentId":"lingxi","title":sid})),
            )
            .await;
        assert_eq!(status, 201, "{body}");
        self.execute_existing(sid, path).await
    }
    async fn execute_existing(&self, sid: &str, path: &Path) -> String {
        let (status, body) = self
            .server
            .request(
                "POST",
                &format!("/lingxi/v1/sessions/{sid}/execute"),
                Some(json!({"input":format!("READ-PATH:{}",path.display())})),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        body["runId"].as_str().unwrap().into()
    }
    async fn count(&self, sql: &str, args: Vec<String>) -> String {
        self.state
            .storage()
            .query_one_text(sql, args)
            .await
            .unwrap()
            .unwrap()
    }
    async fn history(&self, sid: &str) -> String {
        self.count(
            "SELECT COALESCE(group_concat(content_json,'|'),'') FROM messages WHERE session_id=?1",
            vec![sid.into()],
        )
        .await
    }
    async fn stop(self) -> PathBuf {
        self.server.stop().await;
        self.home
    }
}
fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write as _;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn png() -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAFElEQVQImWNgYGD4z8DAwMDAxAADABkEAf+pJ6QAAAAASUVORK5CYII=").unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_read_office_and_vision_reach_next_model_with_source_and_restart_ledger() {
    let home = unique_dir("rich-read");
    let workspace = unique_dir("rich-read-source");
    let remote = Remote::start().await;
    let docx = archive(&[(
        "word/document.xml",
        r#"<w:document xmlns:w="urn:word"><w:body><w:p><w:r><w:t>READ-DOCX-ORIGINAL-正文</w:t></w:r></w:p></w:body></w:document>"#,
    )]);
    let xlsx = archive(&[
        (
            "xl/workbook.xml",
            r#"<workbook><sheets><sheet name="真实工作表" r:id="rId1" xmlns:r="urn:r"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>READ-XLSX-ORIGINAL-正文</t></is></c></row></sheetData></worksheet>"#,
        ),
    ]);
    for (name, bytes) in [
        ("doc.docx", docx.clone()),
        ("sheet.xlsx", xlsx.clone()),
        ("pixels.png", png()),
        ("text.txt", b"READ-TEXT-ORIGINAL".to_vec()),
    ] {
        std::fs::write(workspace.join(name), bytes).unwrap();
    }
    let h = boot(home, workspace.clone(), &remote, true).await;
    for (sid, file, expected) in [
        ("doc", "doc.docx", "READ-DOCX-ORIGINAL-正文"),
        ("sheet", "sheet.xlsx", "READ-XLSX-ORIGINAL-正文"),
        ("pixels", "pixels.png", "VISION-ACTUAL-BLACK-PIXELS"),
        ("text", "text.txt", "READ-TEXT-ORIGINAL"),
    ] {
        h.execute(sid, &workspace.join(file)).await;
        let actual = h.history(sid).await;
        assert!(actual.contains(expected), "{sid}: {actual}");
        {
            use sha2::Digest as _;
            let expected_sha = sha2::Sha256::digest(std::fs::read(workspace.join(file)).unwrap())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert!(
                actual.contains(&expected_sha),
                "source digest must equal the original file bytes: {actual}"
            );
        }
        assert!(
            remote.captures().iter().any(|v| v["model"] == "chat"
                && v["messages"].as_array().is_some_and(|rows| rows
                    .iter()
                    .any(|m| m["role"] == "tool" && m["content"].to_string().contains(expected)))),
            "next model did not consume {expected}"
        );
    }
    let image_call = remote
        .captures()
        .into_iter()
        .find(|v| v["model"] == "vision")
        .unwrap();
    assert!(image_call["messages"]
        .to_string()
        .contains("data:image/png;base64,"));
    assert!(!image_call
        .to_string()
        .contains(&workspace.display().to_string()));
    {
        use base64::Engine as _;
        let exact_pixels = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png())
        );
        assert!(
            image_call["messages"].to_string().contains(&exact_pixels),
            "provider pixels must exactly match the authorized source snapshot"
        );
    }
    assert_eq!(h.count("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE purpose='auxiliary.vision' AND outcome='succeeded' AND provider='remote' AND model='vision' AND input_tokens=13 AND output_tokens=7",vec![]).await,"1");
    assert_eq!(remote.visions(), 1);
    assert_eq!(h.count("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE purpose='auxiliary.vision' AND outcome='succeeded'",vec![]).await,"1");
    assert_eq!(h.count("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE purpose='auxiliary.vision' AND parent_tool_call_id IS NOT NULL AND transport_attempts=1",vec![]).await,"1");
    assert_eq!(
        h.count("SELECT CAST(COUNT(*) AS TEXT) FROM session_files", vec![])
            .await,
        "0"
    );
    assert_eq!(std::fs::read(workspace.join("doc.docx")).unwrap(), docx);
    assert_eq!(std::fs::read(workspace.join("sheet.xlsx")).unwrap(), xlsx);
    assert_eq!(std::fs::read(workspace.join("pixels.png")).unwrap(), png());
    let count = h
        .count(
            "SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage",
            vec![],
        )
        .await;
    let home = h.stop().await;
    let restarted = boot(home, workspace, &remote, true).await;
    assert_eq!(
        restarted
            .count(
                "SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage",
                vec![]
            )
            .await,
        count
    );
    assert!(restarted
        .history("pixels")
        .await
        .contains("VISION-ACTUAL-BLACK-PIXELS"));
    assert_eq!(remote.visions(), 1);
    restarted.stop().await;
    remote.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_read_refusals_never_send_foreign_pixels_or_register_products() {
    let home = unique_dir("rich-refusals");
    let workspace = unique_dir("rich-refusals-source");
    let outside = unique_dir("rich-private");
    let remote = Remote::start().await;
    std::fs::write(workspace.join("bad.docx"), b"not an archive").unwrap();
    std::fs::write(
        workspace.join("empty.xlsx"),
        archive(&[("xl/workbook.xml", "<workbook/>")]),
    )
    .unwrap();
    std::fs::write(workspace.join("bad.xlsx"), b"not a workbook").unwrap();
    std::fs::write(workspace.join("bad.png"), b"NOT-IMAGE-SECRET").unwrap();
    std::fs::write(workspace.join("pixels.png"), png()).unwrap();
    std::fs::write(
        outside.join("private.png"),
        [png(), b"FOREIGN-PIXELS-NOT-DISCLOSED".to_vec()].concat(),
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("private.png"), workspace.join("escape.png")).unwrap();
    std::fs::File::create(workspace.join("oversize.bin"))
        .unwrap()
        .set_len(21 * 1024 * 1024)
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let path = std::ffi::CString::new(workspace.join("fifo").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    }
    let h = boot(home, workspace.clone(), &remote, false).await;
    for (sid, path, expected) in [
        ("bad-doc", workspace.join("bad.docx"), "parse failed"),
        ("bad-sheet", workspace.join("bad.xlsx"), "parse failed"),
        ("empty-sheet", workspace.join("empty.xlsx"), "parse failed"),
        (
            "bad-image",
            workspace.join("bad.png"),
            "signature is invalid",
        ),
        ("missing", workspace.join("missing.txt"), "ENOENT"),
        ("no-vision", workspace.join("pixels.png"), "vision"),
        ("outside", outside.join("private.png"), "authorized root"),
        ("oversize", workspace.join("oversize.bin"), "20MiB"),
    ] {
        h.execute(sid, &path).await;
        let actual = h.history(sid).await;
        assert!(actual.contains(expected), "{sid}: {actual}");
        assert!(!actual.contains("FOREIGN-PIXELS-NOT-DISCLOSED"));
        assert!(!actual.contains("NOT-IMAGE-SECRET"));
    }
    #[cfg(unix)]
    {
        h.execute("link", &workspace.join("escape.png")).await;
        assert!(!h
            .history("link")
            .await
            .contains("FOREIGN-PIXELS-NOT-DISCLOSED"));
    }
    #[cfg(unix)]
    {
        tokio::time::timeout(
            Duration::from_secs(3),
            h.execute("fifo", &workspace.join("fifo")),
        )
        .await
        .expect("FIFO read must not block opening");
        assert!(h.history("fifo").await.contains("regular file"));
    }
    assert_eq!(remote.visions(), 0);
    assert!(!serde_json::to_string(&remote.captures())
        .unwrap()
        .contains("FOREIGN-PIXELS-NOT-DISCLOSED"));
    assert_eq!(
        h.count("SELECT CAST(COUNT(*) AS TEXT) FROM session_files", vec![])
            .await,
        "0"
    );
    assert_eq!(
        std::fs::read(workspace.join("bad.png")).unwrap(),
        b"NOT-IMAGE-SECRET"
    );
    assert!(!workspace.join("missing.txt").exists());
    assert_eq!(h.count("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE purpose='auxiliary.vision' AND transport_attempts>0",vec![]).await,"0");
    h.stop().await;
    remote.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_read_vision_failure_and_cancel_keep_honest_fees_and_release_session_lease() {
    let home = unique_dir("rich-cancel");
    let workspace = unique_dir("rich-cancel-source");
    let remote = Remote::start().await;
    std::fs::write(workspace.join("pixels.png"), png()).unwrap();
    let h = boot(home, workspace.clone(), &remote, true).await;
    remote.fail.store(true, Ordering::SeqCst);
    h.execute("fail", &workspace.join("pixels.png")).await;
    assert!(h.history("fail").await.contains("READ-VISION-REAL-400"));
    assert_eq!(h.count("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE purpose='auxiliary.vision' AND session_id='fail' AND outcome='failed' AND transport_attempts=1 AND input_tokens IS NULL",vec![]).await,"1");
    remote.fail.store(false, Ordering::SeqCst);
    remote.hold.store(true, Ordering::SeqCst);
    let (code, body) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions",
            Some(json!({"sessionId":"cancel","agentId":"lingxi","title":"cancel"})),
        )
        .await;
    assert_eq!(code, 201, "{body}");
    let addr = h.server.addr;
    let token = h.server.token.clone();
    let image = workspace.join("pixels.png");
    let pending = tokio::spawn(async move {
        ApiClient { addr, token }
            .request(
                "POST",
                "/lingxi/v1/sessions/cancel/execute",
                Some(json!({"input":format!("READ-PATH:{}",image.display())})),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(15), remote.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let run = h
        .count(
            "SELECT run_id FROM runs WHERE session_id='cancel' ORDER BY rowid DESC LIMIT 1",
            vec![],
        )
        .await;
    h.state
        .runs()
        .cancel_run(&run, "actual read image cancellation");
    tokio::time::timeout(Duration::from_secs(10), pending)
        .await
        .unwrap()
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if h.count("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE purpose='auxiliary.vision' AND session_id='cancel' AND outcome='cancelled' AND transport_attempts IS NULL",vec![]).await=="1" {break;}
        assert!(
            tokio::time::Instant::now() < deadline,
            "cancel accounting did not settle"
        );
        tokio::task::yield_now().await;
    }
    assert!(!h
        .history("cancel")
        .await
        .contains("VISION-ACTUAL-BLACK-PIXELS"));
    remote.hold.store(false, Ordering::SeqCst);
    remote.release.add_permits(1);
    h.execute_existing("cancel", &workspace.join("pixels.png"))
        .await;
    assert_eq!(h.count("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE purpose='auxiliary.vision' AND session_id='cancel' AND outcome='succeeded'",vec![]).await,"1");
    assert!(h
        .history("cancel")
        .await
        .contains("VISION-ACTUAL-BLACK-PIXELS"));
    assert_eq!(
        h.count("SELECT CAST(COUNT(*) AS TEXT) FROM session_files", vec![])
            .await,
        "0"
    );
    assert_eq!(
        std::fs::read(h.workspace.join("pixels.png")).unwrap(),
        png()
    );
    h.stop().await;
    remote.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_read_xlsx_keeps_eight_sheets_and_reports_the_ninth_without_silent_loss() {
    let home = unique_dir("rich-sheets");
    let workspace = unique_dir("rich-sheets-source");
    let remote = Remote::start().await;
    let mut owned=vec![("xl/workbook.xml".to_string(),format!("<workbook><sheets>{}</sheets></workbook>",(1..=9).map(|n|format!("<sheet name='sheet{n}' id='r{n}'/>")).collect::<String>())),("xl/_rels/workbook.xml.rels".into(),format!("<Relationships>{}</Relationships>",(1..=9).map(|n|format!("<Relationship Id='r{n}' Type='urn:worksheet/worksheet' Target='worksheets/sheet{n}.xml'/>")).collect::<String>()))];
    for n in 1..=9 {
        owned.push((format!("xl/worksheets/sheet{n}.xml"),format!("<worksheet><sheetData><row r='1'><c r='A1' t='inlineStr'><is><t>WORKSHEET-ORIGINAL-{n}</t></is></c></row></sheetData></worksheet>")));
    }
    let entries = owned
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    let bytes = archive(&entries);
    std::fs::write(workspace.join("nine.xlsx"), &bytes).unwrap();
    let h = boot(home, workspace.clone(), &remote, true).await;
    h.execute("nine", &workspace.join("nine.xlsx")).await;
    let actual = h.history("nine").await;
    for n in 1..=8 {
        assert!(actual.contains(&format!("WORKSHEET-ORIGINAL-{n}")));
    }
    assert!(!actual.contains("WORKSHEET-ORIGINAL-9"));
    assert!(actual.contains("Workbook truncated: showing 8/9 sheets"));
    assert!(actual.contains("\"truncated\":true"));
    assert_eq!(remote.visions(), 0);
    assert_eq!(std::fs::read(workspace.join("nine.xlsx")).unwrap(), bytes);
    h.stop().await;
    remote.task.abort();
}
