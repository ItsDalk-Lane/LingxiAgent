//! F03：真实生产会话准入、收据、恢复与HTTP业务。仅外部模型使用loopback替身。
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
struct StubResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl StubResponse {
    fn sse(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/event-stream",
            body,
        }
    }

    fn error(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json",
            body,
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct RecordedRequest {
    path: String,
    headers: Vec<(String, String)>,
    body: serde_json::Value,
}

struct StubServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    slow_release: Arc<tokio::sync::Semaphore>,
    closed_slow: Arc<AtomicUsize>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start(responses: Vec<StubResponse>) -> Self {
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
        let slow_release = Arc::new(tokio::sync::Semaphore::new(0));
        let slow = Arc::clone(&slow_release);
        let closed_slow = Arc::new(AtomicUsize::new(0));
        let task_closed = Arc::clone(&closed_slow);
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                task_hits.fetch_add(1, Ordering::SeqCst);
                let task_requests = Arc::clone(&task_requests);
                let task_responses = Arc::clone(&task_responses);
                let slow = Arc::clone(&slow);
                let closed = Arc::clone(&task_closed);
                children.spawn(async move {
                let recorded = read_request(&mut socket).await;
                let is_slow=recorded.body.to_string().contains("W03_SLOW");
                task_requests.lock().expect("requests").push(recorded);
                let next = task_responses.lock().expect("responses").pop_front();
                let response = match next {
                    Some(scripted) => scripted,
                    None => StubResponse::error(
                        500,
                        r#"{"error":{"message":"stub script exhausted"}}"#.to_string(),
                    ),
                };
                if is_slow {
                    let mut byte = [0_u8; 1];
                    tokio::select! {
                        permit = slow.acquire() => permit.unwrap().forget(),
                        read = socket.read(&mut byte) => {
                            assert!(matches!(read, Ok(0) | Err(_)), "模型等待期收到意外额外字节");
                            closed.fetch_add(1, Ordering::SeqCst);
                            return;
                        }
                    }
                }
                let reason = match response.status {
                    200 => "OK",
                    400 => "Bad Request",
                    401 => "Unauthorized",
                    429 => "Too Many Requests",
                    500 => "Internal Server Error",
                    other => panic!("stub: unscripted status {other}"),
                };
                let payload = response.body;
                let raw = format!(
                    "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    response.status,
                    response.content_type,
                    payload.len()
                );
                let _ = socket.write_all(raw.as_bytes()).await;
                let _ = socket.shutdown().await;
                });
            }
            children.abort_all();
            while children.join_next().await.is_some() {}
        });
        Self {
            addr,
            requests,
            slow_release,
            closed_slow,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn v1(&self) -> String {
        format!("{}/v1", self.origin())
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .map(|r| RecordedRequest {
                path: r.path.clone(),
                headers: r.headers.clone(),
                body: r.body.clone(),
            })
            .collect()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task).await;
        save(
            &format!("wire-{}", self.addr.port()),
            serde_json::json!({"loopback":self.addr.to_string(),"requests":self.requests(),"request_count":self.requests().len()}),
        );
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> RecordedRequest {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
                .await
                .expect("stub read stalled")
                .expect("stub read");
        if read == 0 {
            panic!("stub: connection closed before headers completed");
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = find_subslice(&raw, b"\r\n\r\n") {
            break pos;
        }
        assert!(raw.len() < 256 * 1024, "stub: header block too large");
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("stub: utf8 headers");
    let mut lines = head.split("\r\n");
    let request_line = lines.next().expect("request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("request path")
        .to_string();
    let mut headers = Vec::new();
    let mut content_length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = Some(value.parse::<usize>().expect("numeric content-length"));
            }
            headers.push((name, value));
        }
    }
    let content_length = content_length.expect("json posts carry a content-length");
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 4096];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
                .await
                .expect("stub body read stalled")
                .expect("stub body read");
        if read == 0 {
            panic!("stub: connection closed mid-body");
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&raw[body_start..body_start + content_length])
            .expect("stub: request body must be JSON");
    RecordedRequest {
        path,
        headers,
        body,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn unique_dir(tag: &str) -> PathBuf {
    let root = std::env::var_os("LINGXI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!(
        "lingxi-w03-{tag}-{}-{}",
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
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

struct PlaneBoot {
    state: ServiceState,
    home: PathBuf,
    workspace: PathBuf,
    config_path: PathBuf,
}

async fn boot_with_config(tag: &str, plane_json: &str) -> PlaneBoot {
    boot_with_config_hook(tag, plane_json, None).await
}
async fn boot_with_config_hook(
    tag: &str,
    plane_json: &str,
    hook: Option<lingxi_service::filetools::MutationTestHook>,
) -> PlaneBoot {
    let home = unique_dir(&format!("{tag}-home"));
    let workspace = unique_dir(&format!("{tag}-ws"));
    let config_root = unique_dir(&format!("{tag}-cfg"));
    let config_path = config_root.join("service.json");
    let config_json = format!(
        r#"{{"home": {}, "workspace": {}, {plane_json}}}"#,
        serde_json::to_string(&home.to_string_lossy()).expect("home json"),
        serde_json::to_string(&workspace.to_string_lossy()).expect("ws json"),
    );
    std::fs::write(&config_path, &config_json).expect("write config");
    let layout = prepare_layout(&home).expect("layout");
    let file = lingxi_service::config::read_service_config(&config_path).expect("service config");
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .expect("plane resolves")
            .expect("plane present");
    let credential_service = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credential service"),
    );
    let deps = ServiceDeps {
        native_file_mutation_test_hook: hook,
        model_gateway: Some(Arc::new(
            lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
        )),
        model_plane_source: Some(source),
        workspace_root: file.workspace,
        credential_service: Some(credential_service),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    register_evidence_home(&home);
    PlaneBoot {
        state,
        home,
        workspace,
        config_path,
    }
}

fn openai_sse(frames: &[serde_json::Value]) -> StubResponse {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse::sse(body)
}

fn openai_final(text: &str) -> StubResponse {
    openai_sse(&[
        serde_json::json!({"id":"chatcmpl-stub","model":"the-stub-lies","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-stub","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        serde_json::json!({"id":"chatcmpl-stub","choices":[],"usage":{"prompt_tokens":21,"completion_tokens":9}}),
    ])
}

async fn boot(tag: &str, stub: &StubServer) -> PlaneBoot {
    boot_with_config(tag, &format!(r#""providers": {{"fixture": {{"endpoint": {}, "protocol": "openai-completions", "auth": {{"kind": "none"}}}}}}, "models": {{"chat": {{"provider":"fixture","model":"fixture-model","capabilities": {{"tools":true}}, "compat":{{"contextWindow":1000000}}}}}}"#, serde_json::to_string(&stub.v1()).unwrap())).await
}
/// Plain HTTP/1.1 over a one-shot TCP connection（与 auth_matrix 同款）。
async fn http(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await.expect("write body");
    }
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                break;
            }
            Err(err) => panic!("read response: {err}"),
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status in {head:?}"));
    (status, body.to_string())
}

/// 创建顶层会话（管理面创建路由）。
struct HttpServer {
    boot: PlaneBoot,
    addr: SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    handle:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl HttpServer {
    async fn start(tag: &str, stub: &StubServer) -> Self {
        let boot = boot(tag, stub).await;
        assert!(boot
            .home
            .starts_with(std::env::var("LINGXI_HOME").expect("独立home")));
        Self::from_boot(boot).await
    }
    async fn from_boot(boot: PlaneBoot) -> Self {
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (tx, ready) = tokio::sync::oneshot::channel();
        let state = boot.state.clone();
        let handle = tokio::spawn(async move {
            lingxi_service::run(
                state,
                async {
                    let _ = rx.await;
                },
                |addr| {
                    let _ = tx.send(addr);
                },
                None,
            )
            .await
        });
        let addr = ready.await.unwrap();
        register_evidence_home(&boot.home);
        save(
            &format!("ready-{}", addr.port()),
            serde_json::json!({"actual_home":boot.home,"home_source":"Cli","ready_address":addr.to_string(),"workspace":boot.workspace,"config_path":boot.config_path}),
        );
        Self {
            boot,
            addr,
            stop,
            handle,
        }
    }
    async fn post(&self, path: &str, value: serde_json::Value) -> (u16, String) {
        http(
            &self.addr,
            "POST",
            path,
            &[(
                "Authorization",
                &format!("Bearer {}", self.boot.state.auth().local_token()),
            )],
            Some(&value.to_string()),
        )
        .await
    }
    async fn finish(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), self.handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
fn evidence_homes() -> &'static Mutex<Vec<PathBuf>> {
    static HOMES: std::sync::OnceLock<Mutex<Vec<PathBuf>>> = std::sync::OnceLock::new();
    HOMES.get_or_init(|| Mutex::new(Vec::new()))
}
fn register_evidence_home(home: &Path) {
    let mut homes = evidence_homes().lock().unwrap();
    if !homes.iter().any(|p| p == home) {
        homes.push(home.to_path_buf());
    }
}
// 按实际数据库读取原始 SQL 值；只留本地合成证据，不写回被测库。
fn save_sql_snapshot(root: &Path, tag: &str) {
    let homes = evidence_homes().lock().unwrap().clone();
    let mut snapshots = Vec::new();
    for home in homes {
        let db = home.join("lingxi-service").join("data").join("runs.db");
        if !db.is_file() {
            continue;
        }
        let conn =
            rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        conn.execute_batch("BEGIN").unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut data = Vec::new();
        for table in tables {
            let quoted = table.replace('"', "\"\"");
            let mut fields = conn
                .prepare(&format!("PRAGMA table_info(\"{quoted}\")"))
                .unwrap();
            let columns = fields
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let expressions = columns
                .iter()
                .map(|c| format!("quote(\"{}\")", c.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("SELECT {expressions} FROM \"{quoted}\"");
            let mut rows_stmt = conn.prepare(&sql).unwrap();
            let rows = rows_stmt
                .query_map([], |row| {
                    (0..columns.len())
                        .map(|i| row.get::<_, String>(i))
                        .collect::<Result<Vec<_>, _>>()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            data.push(serde_json::json!({"table":table,"columns":columns,"sql":sql,"rows_as_sql_values":rows}));
        }
        snapshots.push(serde_json::json!({"actual_home":home,"db":db,"tables":data}));
    }
    std::fs::write(
        root.join(format!("{tag}.sql.json")),
        serde_json::to_vec_pretty(&snapshots).unwrap(),
    )
    .unwrap();
}
fn save(tag: &str, v: serde_json::Value) {
    let root = PathBuf::from(std::env::var("LINGXI_W03_EVIDENCE").unwrap());
    save_sql_snapshot(&root, tag);
    std::fs::write(
        root.join(format!("{tag}.json")),
        serde_json::to_vec_pretty(&v).unwrap(),
    )
    .unwrap();
}
#[tokio::test]
async fn positive_and_fork_event_atomicity() {
    let stub = StubServer::start(vec![openai_final("首次正常回复")]).await;
    let svc = HttpServer::start("atomic", &stub).await;
    let (status, body) = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"W03正常正对照"}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let msgs = lingxi_adapters::storage::session_tree::list_branch_messages(
        svc.boot.state.storage(),
        "sess_local_alpha",
    )
    .await
    .unwrap();
    let boundary = msgs[0].message_id.clone();
    let (positive, positive_body) = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/fork",
            serde_json::json!({"newSessionId":"w03-positive","boundaryMessageId":boundary}),
        )
        .await;
    assert_eq!(positive, 200, "{positive_body}");
    // 真实SQLite触发器在关键事件提交处失败，未替换任何被测组件。
    let conn = rusqlite::Connection::open(svc.boot.state.storage().db_path()).unwrap();
    conn.execute_batch("CREATE TRIGGER w03_event_failure BEFORE INSERT ON key_events WHEN NEW.event_type='session_created' BEGIN SELECT RAISE(ABORT,'W03 controlled event failure'); END;").unwrap();
    drop(conn);
    let (failed, error) = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/fork",
            serde_json::json!({"newSessionId":"w03-atomic","boundaryMessageId":boundary}),
        )
        .await;
    assert!(failed >= 400, "{error}");
    let row = lingxi_adapters::storage::session_tree::get_session_row(
        svc.boot.state.storage(),
        "w03-atomic",
    )
    .await
    .unwrap();
    save(
        "atomicity",
        serde_json::json!({"positive_status":positive,"positive_body":positive_body,"model_requests":stub.requests().iter().map(|r|r.body.clone()).collect::<Vec<_>>(),"failure_status":failed,"failure_body":error,"fork_exists_after_event_failure":row.is_some()}),
    );
    assert!(
        row.is_none(),
        "关键事件失败后fork状态仍落库，违反原子恢复契约"
    );
    let home = svc.boot.home.clone();
    let workspace = svc.boot.workspace.clone();
    let config = svc.boot.config_path.clone();
    svc.finish().await;
    let svc = HttpServer::from_boot(reopen(home, workspace, config).await).await;
    let absent = http(
        &svc.addr,
        "GET",
        "/lingxi/v1/sessions/w03-atomic",
        &[(
            "Authorization",
            &format!("Bearer {}", svc.boot.state.auth().local_token()),
        )],
        None,
    )
    .await;
    assert_eq!(absent.0, 404, "restart must not reconstruct a failed fork");
    let event_count = svc.boot.state.storage().query_one_text(
        "SELECT CAST(COUNT(*) AS TEXT) FROM key_events WHERE session_id='w03-atomic' AND event_type='session_created'", vec![],
    ).await.unwrap().unwrap();
    assert_eq!(event_count, "0");
    let operation_query = http(
        &svc.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/operations",
        &[(
            "Authorization",
            &format!("Bearer {}", svc.boot.state.auth().local_token()),
        )],
        None,
    )
    .await;
    assert_eq!(operation_query.0, 200);
    assert!(
        operation_query.1.contains("prepared"),
        "failed operation remains queryable"
    );
    assert!(lingxi_adapters::storage::session_tree::get_session_row(
        svc.boot.state.storage(),
        "w03-positive"
    )
    .await
    .unwrap()
    .is_some());
    save(
        "atomicity-restart",
        serde_json::json!({"failed_fork_query":absent,"failed_fork_event_count":event_count,"source_operation_query":operation_query,"positive_fork_preserved":true}),
    );
    svc.finish().await;
    stub.stop().await;
}

struct BarrierProbe {
    kind: &'static str,
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}
impl BarrierProbe {
    fn new(kind: &'static str) -> Arc<Self> {
        Arc::new(Self {
            kind,
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
        })
    }
    async fn wait(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}
impl lingxi_service::session_tree::SessionOperationProbe for BarrierProbe {
    fn reached<'a>(
        &'a self,
        kind: &'a str,
        _sid: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if kind == self.kind {
                self.entered.add_permits(1);
                self.release.acquire().await.unwrap().forget();
            }
        })
    }
}
async fn messages(
    svc: &HttpServer,
    sid: &str,
) -> Vec<lingxi_adapters::storage::session_tree::MessageRow> {
    lingxi_adapters::storage::session_tree::list_branch_messages(svc.boot.state.storage(), sid)
        .await
        .unwrap()
}
async fn post_at(
    addr: SocketAddr,
    token: String,
    path: String,
    v: serde_json::Value,
) -> (u16, String) {
    http(
        &addr,
        "POST",
        &path,
        &[("Authorization", &format!("Bearer {token}"))],
        Some(&v.to_string()),
    )
    .await
}
#[tokio::test]
async fn authenticated_barrier_rewind_execute_and_cancelled_waiter() {
    let stub = StubServer::start(vec![openai_final("one"), openai_final("two")]).await;
    let svc = HttpServer::start("barrier", &stub).await;
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"first"})
        )
        .await
        .0,
        200
    );
    let boundary = messages(&svc, "sess_local_alpha").await[0]
        .message_id
        .clone();
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/sess_local_alpha/checkpoints",
            serde_json::json!({"name":"before","targetMessageId":boundary})
        )
        .await
        .0,
        200
    );
    let probe = BarrierProbe::new("rewind");
    svc.boot
        .state
        .session_tree()
        .set_operation_probe(Some(probe.clone()));
    let token = svc.boot.state.auth().local_token().to_string();
    let waiter = tokio::spawn(post_at(
        svc.addr,
        token,
        "/lingxi/v1/sessions/sess_local_alpha/rewind".into(),
        serde_json::json!({"checkpoint":"before","requestId":"cancel-wait"}),
    ));
    probe.wait().await;
    let denied = http(
        &svc.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &[],
        Some("{\"input\":\"secret\"}"),
    )
    .await;
    assert_eq!(denied.0, 401);
    let busy = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"must not run"}),
        )
        .await;
    assert_eq!(busy.0, 409, "{}", busy.1);
    for (path, value) in [
        (
            "/lingxi/v1/sessions/sess_local_alpha/turns/retry",
            serde_json::json!({}),
        ),
        (
            "/lingxi/v1/sessions/sess_local_alpha/rewind",
            serde_json::json!({"checkpoint":"before"}),
        ),
    ] {
        let rejected = svc.post(path, value).await;
        assert_eq!(rejected.0, 409, "{path}: {}", rejected.1);
    }
    let issued = svc
        .post(
            "/lingxi/v1/devices/credentials",
            serde_json::json!({"userId":"unrelated-busy","scopes":["chat"]}),
        )
        .await;
    assert_eq!(issued.0, 201);
    let issue: serde_json::Value = serde_json::from_str(&issued.1).unwrap();
    let secret = issue["secret"].as_str().unwrap();
    let outsider = http(
        &svc.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &[("Authorization", &format!("Bearer {secret}"))],
        Some("{\"input\":\"other user\"}"),
    )
    .await;
    assert_eq!(outsider.0, 403, "{}", outsider.1);
    waiter.abort();
    let _ = waiter.await;
    let still_busy = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/fork",
            serde_json::json!({"newSessionId":"during-cancel","boundaryMessageId":boundary}),
        )
        .await;
    assert_eq!(still_busy.0, 409);
    probe.release.add_permits(1);
    for _ in 0..100 {
        if !svc
            .boot
            .state
            .sessions()
            .session_supervisor()
            .is_busy("sess_local_alpha")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha"));
    svc.boot.state.session_tree().set_operation_probe(None);
    let replay = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/rewind",
            serde_json::json!({"checkpoint":"before","requestId":"cancel-wait"}),
        )
        .await;
    assert_eq!(replay.0, 200, "{}", replay.1);
    let next = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"after rewind"}),
        )
        .await;
    assert_eq!(next.0, 200, "{}", next.1);
    assert_eq!(stub.requests().len(), 2);
    save(
        "barrier",
        serde_json::json!({"unauthorized":denied,"busy":busy,"cancel_busy":still_busy,"replay":replay,"next":next,"wire":stub.requests().iter().map(|r|r.body.clone()).collect::<Vec<_>>()}),
    );
    svc.finish().await;
    stub.stop().await;
}
#[tokio::test]
async fn slow_model_does_not_block_independent_session_or_metadata() {
    let stub = StubServer::start(vec![openai_final("slow"), openai_final("parallel")]).await;
    let svc = HttpServer::start("parallel", &stub).await;
    let token = svc.boot.state.auth().local_token().to_string();
    let slow = tokio::spawn(post_at(
        svc.addr,
        token,
        "/lingxi/v1/sessions/sess_local_alpha/execute".into(),
        serde_json::json!({"input":"W03_SLOW"}),
    ));
    for _ in 0..100 {
        if !stub.requests().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(stub.requests().len(), 1);
    let mut refusals = Vec::new();
    for (p, b) in [
        (
            "/lingxi/v1/sessions/sess_local_alpha/fork",
            serde_json::json!({"newSessionId":"slow-fork","boundaryMessageId":"unknown"}),
        ),
        (
            "/lingxi/v1/sessions/sess_local_alpha/turns/retry",
            serde_json::json!({}),
        ),
        (
            "/lingxi/v1/sessions/sess_local_alpha/rewind",
            serde_json::json!({"checkpoint":"unknown"}),
        ),
        (
            "/lingxi/v1/sessions/archive",
            serde_json::json!({"sessionId":"sess_local_alpha"}),
        ),
    ] {
        let result = svc.post(p, b).await;
        assert_eq!(result.0, 409, "{p}:{}", result.1);
        refusals.push(result);
    }
    let rename = svc
        .post(
            "/lingxi/v1/sessions/rename",
            serde_json::json!({"sessionId":"sess_local_alpha","title":"still usable"}),
        )
        .await;
    assert_eq!(rename.0, 200, "{}", rename.1);
    let pin = svc
        .post(
            "/lingxi/v1/sessions/pin",
            serde_json::json!({"sessionId":"sess_local_alpha","pinned":true}),
        )
        .await;
    assert_eq!(pin.0, 200, "{}", pin.1);
    let pin_order = svc
        .post(
            "/lingxi/v1/sessions/pin-order",
            serde_json::json!({"sessionIds":["sess_local_alpha"]}),
        )
        .await;
    assert_eq!(pin_order.0, 200, "{}", pin_order.1);
    let mem = http(
        &svc.addr,
        "PATCH",
        "/lingxi/v1/sessions/sess_local_alpha/memory",
        &[(
            "Authorization",
            &format!("Bearer {}", svc.boot.state.auth().local_token()),
        )],
        Some("{\"memoryEnabled\":false}"),
    )
    .await;
    assert_eq!(mem.0, 200, "{}", mem.1);
    let parallel = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        svc.post(
            "/lingxi/v1/sessions/sess_local_beta/execute",
            serde_json::json!({"input":"independent"}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(parallel.0, 200, "{}", parallel.1);
    assert!(!slow.is_finished());
    stub.slow_release.add_permits(1);
    let finished = slow.await.unwrap();
    assert_eq!(finished.0, 200, "{}", finished.1);
    save(
        "parallel",
        serde_json::json!({"busy_refusals":refusals,"rename":rename,"pin":pin,"pin_order":pin_order,"memory":mem,"independent":parallel,"slow":finished}),
    );
    svc.finish().await;
    stub.stop().await;
}
#[tokio::test]
async fn same_database_constructor_and_instance_contract() {
    let stub = StubServer::start(vec![]).await;
    let svc = HttpServer::start("constructors", &stub).await;
    let second = lingxi_adapters::storage::RunDatabase::open(
        svc.boot.state.storage().db_path(),
        Default::default(),
    )
    .await
    .unwrap();
    assert!(Arc::ptr_eq(
        second.session_admission(),
        svc.boot.state.storage().session_admission()
    ));
    let a = lingxi_service::sessions::SessionStore::new(second);
    let lease = a
        .session_supervisor()
        .try_begin_run("sess_local_alpha")
        .unwrap();
    assert!(svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha"));
    drop(lease);
    let layout = prepare_layout(&svc.boot.home).unwrap();
    let (guard, _) = lingxi_service::instance::acquire(&layout).unwrap();
    assert!(lingxi_service::instance::acquire(&layout).is_err());
    let other = prepare_layout(&unique_dir("independent-instance")).unwrap();
    let (other_guard, _) = lingxi_service::instance::acquire(&other).unwrap();
    drop(other_guard);
    drop(guard);
    let (reopen, _) = lingxi_service::instance::acquire(&layout).unwrap();
    drop(reopen);
    save(
        "constructors",
        serde_json::json!({"same_database_shared":true,"same_home_second_refused":true,"independent_home_allowed":true,"reopen_after_drop":true}),
    );
    svc.finish().await;
    stub.stop().await;
}

async fn reopen(home: PathBuf, workspace: PathBuf, config_path: PathBuf) -> PlaneBoot {
    let layout = prepare_layout(&home).unwrap();
    let (_, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .unwrap()
            .unwrap();
    let credential_service = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let deps = ServiceDeps {
        model_gateway: Some(Arc::new(
            lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
        )),
        workspace_root: Some(workspace.clone()),
        credential_service: Some(credential_service),
        ..Default::default()
    };
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .unwrap();
    PlaneBoot {
        state,
        home,
        workspace,
        config_path,
    }
}
#[tokio::test]
async fn unknown_file_effect_survives_storage_failure_restart_and_replay() {
    let stub = StubServer::start(vec![
        openai_final("original"),
        openai_final("fresh identity"),
    ])
    .await;
    let svc = HttpServer::start("unknown", &stub).await;
    let file = svc.boot.workspace.join("sentinel.txt");
    std::fs::write(&file, b"checkpoint-original").unwrap();
    let create=svc.post("/lingxi/v1/sessions",serde_json::json!({"sessionId":"w03-files","agentId":"lingxi","title":"files","authorizedFolders":[svc.boot.workspace]})).await;
    assert_eq!(create.0, 201, "{}", create.1);
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/w03-files/execute",
            serde_json::json!({"input":"first"})
        )
        .await
        .0,
        200
    );
    let boundary = messages(&svc, "w03-files").await[0].message_id.clone();
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/w03-files/checkpoints",
            serde_json::json!({"name":"old","targetMessageId":boundary,"filePaths":[file]})
        )
        .await
        .0,
        200
    );
    std::fs::write(&file, b"witnessed-later").unwrap();
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/w03-files/checkpoints",
            serde_json::json!({"name":"later","targetMessageId":boundary,"filePaths":[file]})
        )
        .await
        .0,
        200
    );
    let before = lingxi_adapters::storage::session_tree::list_branch_messages(
        svc.boot.state.storage(),
        "w03-files",
    )
    .await
    .unwrap();
    let conn = rusqlite::Connection::open(svc.boot.state.storage().db_path()).unwrap();
    conn.execute_batch("CREATE TRIGGER w03_reset_failure BEFORE INSERT ON key_events WHEN NEW.event_type='session_branch_reset' BEGIN SELECT RAISE(ABORT,'W03 reset event failure'); END;").unwrap();
    drop(conn);
    svc.boot.state.set_rollback_file_changes(true);
    let probe = BarrierProbe::new("rewind:after-files");
    svc.boot
        .state
        .session_tree()
        .set_operation_probe(Some(probe.clone()));
    let request =
        serde_json::json!({"checkpoint":"old","restoreFiles":true,"requestId":"uncertain-file"});
    let waiting = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/w03-files/rewind".into(),
        request.clone(),
    ));
    probe.wait().await;
    assert_eq!(std::fs::read(&file).unwrap(), b"checkpoint-original");
    let busy = svc
        .post(
            "/lingxi/v1/sessions/w03-files/execute",
            serde_json::json!({"input":"while file task continues"}),
        )
        .await;
    assert_eq!(busy.0, 409);
    // 真正取消客户端等待者，后台实际业务必须继续持权到持久化返回。
    waiting.abort();
    let _ = waiting.await;
    assert!(svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("w03-files"));
    probe.release.add_permits(1);
    for _ in 0..100 {
        if !svc
            .boot
            .state
            .sessions()
            .session_supervisor()
            .is_busy("w03-files")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("w03-files"));
    assert_eq!(
        format!("{before:?}"),
        format!(
            "{:?}",
            lingxi_adapters::storage::session_tree::list_branch_messages(
                svc.boot.state.storage(),
                "w03-files"
            )
            .await
            .unwrap()
        )
    );
    std::fs::write(&file, b"external-after-failure").unwrap();
    let home = svc.boot.home.clone();
    let ws = svc.boot.workspace.clone();
    let cfg = svc.boot.config_path.clone();
    svc.finish().await;
    let svc = HttpServer::from_boot(reopen(home, ws, cfg).await).await;
    svc.boot.state.set_rollback_file_changes(true);
    let replay = svc
        .post("/lingxi/v1/sessions/w03-files/rewind", request)
        .await;
    assert_eq!(replay.0, 409, "{}", replay.1);
    let execute = svc
        .post(
            "/lingxi/v1/sessions/w03-files/execute",
            serde_json::json!({"input":"after unknown operation"}),
        )
        .await;
    assert_eq!(execute.0, 409, "{}", execute.1);
    let query = http(
        &svc.addr,
        "GET",
        "/lingxi/v1/sessions/w03-files/operations",
        &[(
            "Authorization",
            &format!("Bearer {}", svc.boot.state.auth().local_token()),
        )],
        None,
    )
    .await;
    assert_eq!(query.0, 200, "{}", query.1);
    assert!(query.1.contains("prepared"));
    assert!(query.1.contains("\"fileEffects\":true"));
    assert_eq!(std::fs::read(&file).unwrap(), b"external-after-failure");
    assert_eq!(stub.requests().len(), 1);
    save(
        "unknown-restart",
        serde_json::json!({"busy_before_failure":busy,"same_request_replay":replay,"execute_after_restart":execute,"query":query,"file_sha":lingxi_kernel::session_tree::sha256_hex(&std::fs::read(&file).unwrap()),"head":format!("{before:?}"),"wire_count":stub.requests().len()}),
    );
    let archived = svc
        .post(
            "/lingxi/v1/sessions/archive",
            serde_json::json!({"sessionId":"w03-files"}),
        )
        .await;
    assert_eq!(archived.0, 200, "{}", archived.1);
    let deleted = svc
        .post(
            "/lingxi/v1/sessions/archived/delete",
            serde_json::json!({"sessionId":"w03-files"}),
        )
        .await;
    assert_eq!(deleted.0, 200, "{}", deleted.1);
    let home = svc.boot.home.clone();
    let ws = svc.boot.workspace.clone();
    let cfg = svc.boot.config_path.clone();
    svc.finish().await;
    let svc = HttpServer::from_boot(reopen(home, ws, cfg).await).await;
    let own_query = http(
        &svc.addr,
        "GET",
        "/lingxi/v1/sessions/w03-files/operations",
        &[(
            "Authorization",
            &format!("Bearer {}", svc.boot.state.auth().local_token()),
        )],
        None,
    )
    .await;
    let replay_deleted = svc.post("/lingxi/v1/sessions/w03-files/rewind", serde_json::json!({"checkpoint":"old","restoreFiles":true,"requestId":"uncertain-file"})).await;
    let credential = svc
        .post(
            "/lingxi/v1/devices/credentials",
            serde_json::json!({"userId":"tombstone-foreign","scopes":["chat"]}),
        )
        .await;
    assert_eq!(credential.0, 201);
    let credential: serde_json::Value = serde_json::from_str(&credential.1).unwrap();
    let foreign_query = http(
        &svc.addr,
        "GET",
        "/lingxi/v1/sessions/w03-files/operations",
        &[(
            "Authorization",
            &format!("Bearer {}", credential["secret"].as_str().unwrap()),
        )],
        None,
    )
    .await;
    let no_journal = http(
        &svc.addr,
        "GET",
        "/lingxi/v1/sessions/no-such-journal/operations",
        &[(
            "Authorization",
            &format!("Bearer {}", svc.boot.state.auth().local_token()),
        )],
        None,
    )
    .await;
    let recreate=svc.post("/lingxi/v1/sessions",serde_json::json!({"sessionId":"w03-files","agentId":"lingxi","title":"recycled identity"})).await;
    save(
        "deleted-identity-recreate",
        serde_json::json!({"recreate":recreate,"owner_query_before_recreate":own_query,"external_file_preserved":std::fs::read(&file).unwrap()==b"external-after-failure","wire_count":stub.requests().len()}),
    );
    save(
        "deleted-unknown-receipt",
        serde_json::json!({"archive":archived,"delete":deleted,"owner_query":own_query,"foreign_query":foreign_query,"no_journal":no_journal,"replay_deleted":replay_deleted,"wire_count":stub.requests().len(),"external_file_preserved":std::fs::read(&file).unwrap()==b"external-after-failure"}),
    );
    assert_eq!(
        recreate.0, 409,
        "真实旧操作墓碑的业务身份不能复用：{}",
        recreate.1
    );
    assert_eq!(own_query.0, 200, "{}", own_query.1);
    assert!(own_query.1.contains("prepared") && own_query.1.contains("\"fileEffects\":true"));
    assert_eq!(foreign_query.0, 404, "{}", foreign_query.1);
    assert_eq!(no_journal.0, 404, "{}", no_journal.1);
    assert_eq!(replay_deleted.0, 404, "{}", replay_deleted.1);
    assert_eq!(std::fs::read(&file).unwrap(), b"external-after-failure");
    assert_eq!(stub.requests().len(), 1);
    assert!(recreate.1.contains("session_identity_retired"));
    let fresh=svc.post("/lingxi/v1/sessions",serde_json::json!({"sessionId":"w03-files-fresh","agentId":"lingxi","title":"fresh identity"})).await;
    assert_eq!(fresh.0, 201);
    let execute_fresh = svc
        .post(
            "/lingxi/v1/sessions/w03-files-fresh/execute",
            serde_json::json!({"input":"fresh session still works"}),
        )
        .await;
    assert_eq!(execute_fresh.0, 200, "{}", execute_fresh.1);
    assert_eq!(stub.requests().len(), 2);
    assert_eq!(std::fs::read(&file).unwrap(), b"external-after-failure");
    save(
        "retired-and-fresh-identities",
        serde_json::json!({"retired":recreate,"fresh_create":fresh,"fresh_execute":execute_fresh,"wire_count":2,"external_file_preserved":true}),
    );
    svc.finish().await;
    stub.stop().await;
}
#[tokio::test]
async fn completed_fork_replay_conflict_and_recovered_event_are_consistent() {
    let stub = StubServer::start(vec![openai_final("first")]).await;
    let svc = HttpServer::start("receipt", &stub).await;
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"first"})
        )
        .await
        .0,
        200
    );
    let boundary = messages(&svc, "sess_local_alpha").await[0]
        .message_id
        .clone();
    let input = serde_json::json!({"newSessionId":"receipt-fork","boundaryMessageId":boundary,"requestId":"fork-receipt"});
    let positive = svc
        .post("/lingxi/v1/sessions/sess_local_alpha/fork", input.clone())
        .await;
    assert_eq!(positive.0, 200, "{}", positive.1);
    let home = svc.boot.home.clone();
    let ws = svc.boot.workspace.clone();
    let cfg = svc.boot.config_path.clone();
    svc.finish().await;
    let svc = HttpServer::from_boot(reopen(home, ws, cfg).await).await;
    let replay = svc
        .post("/lingxi/v1/sessions/sess_local_alpha/fork", input)
        .await;
    assert_eq!(replay, positive);
    let issued = svc
        .post(
            "/lingxi/v1/devices/credentials",
            serde_json::json!({"userId":"unrelated","scopes":["chat"]}),
        )
        .await;
    assert_eq!(issued.0, 201);
    let issue: serde_json::Value = serde_json::from_str(&issued.1).unwrap();
    let remote = issue["secret"].as_str().unwrap();
    let forged=http(&svc.addr,"POST","/lingxi/v1/sessions/sess_local_alpha/fork",&[("Authorization",&format!("Bearer {remote}"))],Some(&serde_json::json!({"newSessionId":"receipt-fork","boundaryMessageId":boundary,"requestId":"fork-receipt"}).to_string())).await;
    assert_eq!(forged.0, 403, "{}", forged.1);
    let op_conflict = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/turns/retry",
            serde_json::json!({"requestId":"fork-receipt"}),
        )
        .await;
    assert_eq!(op_conflict.0, 409);
    let execute_conflict = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"forged type","requestId":"fork-receipt"}),
        )
        .await;
    assert_eq!(execute_conflict.0, 409, "{}", execute_conflict.1);
    let conflict=svc.post("/lingxi/v1/sessions/sess_local_alpha/fork",serde_json::json!({"newSessionId":"forged-second","boundaryMessageId":boundary,"requestId":"fork-receipt"})).await;
    assert_eq!(conflict.0, 409, "{}", conflict.1);
    let events = http(
        &svc.addr,
        "GET",
        "/lingxi/v1/sessions/receipt-fork/events",
        &[(
            "Authorization",
            &format!("Bearer {}", svc.boot.state.auth().local_token()),
        )],
        None,
    )
    .await;
    assert_eq!(events.0, 200, "{}", events.1);
    let event_json: serde_json::Value = serde_json::from_str(&events.1).unwrap();
    assert_eq!(
        event_json["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["eventType"] == "session_created")
            .count(),
        1
    );
    assert!(lingxi_adapters::storage::session_tree::get_session_row(
        svc.boot.state.storage(),
        "forged-second"
    )
    .await
    .unwrap()
    .is_none());
    save(
        "receipt-restart",
        serde_json::json!({"positive":positive,"replay":replay,"conflict":conflict,"event_query":events,"new_branch_count":1}),
    );
    svc.finish().await;
    stub.stop().await;
}

#[tokio::test]
async fn actual_child_keeps_parent_admission_until_exit() {
    let stub = StubServer::start(vec![openai_final("parent"), openai_final("child")]).await;
    let svc = HttpServer::start("child-permit", &stub).await;
    let parent = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/sess_local_alpha/execute".into(),
        serde_json::json!({"input":"W03_SLOW_PARENT"}),
    ));
    for _ in 0..100 {
        if stub.requests().len() == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let run = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 1)
        .await
        .unwrap()[0]
        .run_id
        .clone();
    let scope = svc.boot.state.runs().run_scope(&run).unwrap();
    let child = svc
        .boot
        .state
        .subagents()
        .dispatch_child(
            lingxi_service::subagents::ParentRunFacts {
                principal: lingxi_kernel::Principal::LocalUser,
                session_id: "sess_local_alpha".into(),
                agent_id: "lingxi".into(),
                parent_run_id: lingxi_protocol::RunId::new(run.clone()),
                parent_scope: scope,
                source_model_call: lingxi_protocol::ModelCallId::new("w03-source"),
                cause_tool_call: lingxi_protocol::ToolCallId::new("w03-cause"),
                now_ms: 1791700000000,
            },
            lingxi_kernel::ports::DelegationRequest {
                task: "W03_SLOW_CHILD".into(),
                access: Some(lingxi_kernel::subagent::AccessRequest::Read),
                label: None,
                agent_id: None,
                model: None,
                thread_id: None,
            },
        )
        .await
        .unwrap();
    for _ in 0..100 {
        if stub.requests().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(stub.requests().len(), 2);
    stub.slow_release.add_permits(1);
    let parent_done = parent.await.unwrap();
    assert_eq!(parent_done.0, 200, "{}", parent_done.1);
    assert!(svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha"));
    let denied = svc
        .post(
            "/lingxi/v1/sessions/archive",
            serde_json::json!({"sessionId":"sess_local_alpha"}),
        )
        .await;
    assert_eq!(denied.0, 409, "{}", denied.1);
    stub.slow_release.add_permits(1);
    for _ in 0..100 {
        if !svc
            .boot
            .state
            .sessions()
            .session_supervisor()
            .is_busy("sess_local_alpha")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha"));
    save(
        "actual-child",
        serde_json::json!({"parent_run":run,"child_run":child.child_run_id,"parent_completed":parent_done,"archive_while_child_working":denied,"busy_after_child_exit":false,"wire_count":stub.requests().len()}),
    );
    svc.finish().await;
    stub.stop().await;
}
#[tokio::test]
async fn latest_permission_and_lifecycle_are_rechecked_before_commit() {
    // 原联合反例保留，再分别证明每个关键事实独立参与最终再检。
    for (label, sql) in [
        ("combined", "UPDATE sessions SET permission_mode='read_only',lifecycle='archived' WHERE session_id='sess_local_alpha'"),
        ("permission", "UPDATE sessions SET permission_mode='read_only' WHERE session_id='sess_local_alpha'"),
        ("lifecycle", "UPDATE sessions SET lifecycle='archived' WHERE session_id='sess_local_alpha'"),
        ("owner", "UPDATE sessions SET owner_user_id='changed-owner' WHERE session_id='sess_local_alpha'"),
        ("folders", "UPDATE sessions SET authorized_folders_json='[\"changed-folder\"]' WHERE session_id='sess_local_alpha'"),
        ("revision", "UPDATE session_branch_heads SET revision=revision+1 WHERE session_id='sess_local_alpha'"),
    ] {
    let stub = StubServer::start(vec![openai_final("first")]).await;
    let svc = HttpServer::start(&format!("snapshot-{label}"), &stub).await;
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"first"})
        )
        .await
        .0,
        200
    );
    let boundary = messages(&svc, "sess_local_alpha").await[0]
        .message_id
        .clone();
    let probe = BarrierProbe::new("fork");
    svc.boot
        .state
        .session_tree()
        .set_operation_probe(Some(probe.clone()));
    let fork = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/sess_local_alpha/fork".into(),
        serde_json::json!({"newSessionId":"snapshot-rejected","boundaryMessageId":boundary,"requestId":"snapshot-check"}),
    ));
    probe.wait().await;
    // 真SQLite故障窗口改变关键权限事实，旧快照不能继续持久化成功。
    let conn = rusqlite::Connection::open(svc.boot.state.storage().db_path()).unwrap();
    conn.execute(sql, []).unwrap();
    drop(conn);
    probe.release.add_permits(1);
    let result = fork.await.unwrap();
    assert_eq!(result.0, 409, "{}", result.1);
    assert!(lingxi_adapters::storage::session_tree::get_session_row(
        svc.boot.state.storage(),
        "snapshot-rejected"
    )
    .await
    .unwrap()
    .is_none());
    save(
        &format!("snapshot-{label}"),
        serde_json::json!({"changed_fact":label,"fork_response":result,"fork_exists":false}),
    );
    svc.finish().await;
    stub.stop().await;
    }
}

#[tokio::test]
async fn real_gateway_cooperative_cancel_releases_after_actual_task_exit() {
    let stub = StubServer::start(vec![
        openai_final("late"),
        openai_final("independent"),
        openai_final("next"),
    ])
    .await;
    let svc = HttpServer::start("real-cancel", &stub).await;
    let caller = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/sess_local_alpha/execute".into(),
        serde_json::json!({"input":"W03_SLOW_CANCEL","requestId":"cooperative-cancel"}),
    ));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while stub.requests().len() != 1 {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let run = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 1)
        .await
        .unwrap()[0]
        .run_id
        .clone();
    let independent = svc
        .post(
            "/lingxi/v1/sessions/sess_local_beta/execute",
            serde_json::json!({"input":"independent during cancel"}),
        )
        .await;
    assert_eq!(independent.0, 200, "{}", independent.1);
    let principal = svc
        .boot
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", svc.boot.state.auth().local_token())),
            None,
            false,
            lingxi_service::transport::ConnectionKind::Local,
        )
        .unwrap();
    let cancellation = svc
        .boot
        .state
        .sessions()
        .cancel_run_for(
            svc.boot.state.storage().as_ref(),
            svc.boot.state.runs(),
            &principal,
            &run,
        )
        .await
        .unwrap();
    assert!(matches!(
        cancellation,
        lingxi_service::CancelRunOutcome::Accepted { .. }
    ));
    let finished = caller.await.unwrap();
    assert_eq!(finished.0, 200, "{}", finished.1);
    while svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha")
    {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(svc
        .boot
        .state
        .runs()
        .task_supervisor()
        .live_children_of(&run)
        .is_empty());
    let next = svc.post("/lingxi/v1/sessions/sess_local_alpha/execute", serde_json::json!({"input":"next after actual cancellation","requestId":"after-cooperative-cancel"})).await;
    assert_eq!(next.0, 200, "{}", next.1);
    assert_eq!(stub.requests().len(), 3);
    save(
        "real-cancel",
        serde_json::json!({"run":run,"independent":independent,"cancelled_submission":finished,"next":next,"actual_live_children_after_cancel":0,"wire_count":3}),
    );
    svc.finish().await;
    stub.stop().await;
}

#[tokio::test]
async fn real_http_disconnect_cancels_only_its_drive_and_reclaims_provider_socket() {
    let stub = StubServer::start(vec![
        openai_final("discarded"),
        openai_final("beta"),
        openai_final("next"),
    ])
    .await;
    let svc = HttpServer::start("disconnect", &stub).await;
    let caller = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/sess_local_alpha/execute".into(),
        serde_json::json!({"input":"W03_SLOW_DISCONNECT","requestId":"disconnect-one"}),
    ));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while stub.requests().len() != 1 {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let run = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 1)
        .await
        .unwrap()[0]
        .run_id
        .clone();
    let independent = svc
        .post(
            "/lingxi/v1/sessions/sess_local_beta/execute",
            serde_json::json!({"input":"beta survives other disconnect"}),
        )
        .await;
    assert_eq!(independent.0, 200, "{}", independent.1);
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    while svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha")
        || stub.closed_slow.load(Ordering::SeqCst) != 1
    {
        assert!(
            std::time::Instant::now() < deadline,
            "断连后实际工作或模型连接未回收"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(svc
        .boot
        .state
        .runs()
        .task_supervisor()
        .live_children_of(&run)
        .is_empty());
    let next = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"next after disconnect","requestId":"disconnect-next"}),
        )
        .await;
    assert_eq!(next.0, 200, "{}", next.1);
    assert_eq!(stub.requests().len(), 3);
    save(
        "real-disconnect",
        serde_json::json!({"run":run,"independent":independent,"next":next,"closed_provider_connections":1,"wire_count":3,"actual_live_children_after_disconnect":0}),
    );
    svc.finish().await;
    stub.stop().await;
}

#[tokio::test]
async fn public_direct_drive_obeys_actual_same_database_admission() {
    let stub = StubServer::start(vec![openai_final("unexpected direct dispatch")]).await;
    let svc = HttpServer::start("direct-busy", &stub).await;
    let gate = svc.boot.state.storage().session_admission();
    let held = gate.try_acquire(&["sess_local_alpha"], usize::MAX).unwrap();
    let actual_agent = svc
        .boot
        .state
        .storage()
        .get_session("sess_local_alpha")
        .await
        .unwrap()
        .unwrap()
        .agent_id;
    let result = svc
        .boot
        .state
        .runs()
        .drive_run(
            svc.boot.state.storage().as_ref(),
            svc.boot.state.events(),
            &lingxi_kernel::Principal::LocalUser,
            "sess_local_alpha",
            &actual_agent,
            "w03_direct_busy",
            "direct public input",
            1,
            1,
            None,
            None,
            lingxi_service::runs::DriveAuthorization::user_submission(
                None,
                lingxi_kernel::subagent::SessionPermissionMode::Operate,
            ),
            None,
            "sess_local_alpha",
        )
        .await;
    let count = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 10)
        .await
        .unwrap()
        .len();
    save(
        "public-direct-busy",
        serde_json::json!({"result":format!("{result:?}"),"wire_count":stub.requests().len(),"durable_run_count":count}),
    );
    drop(held);
    assert!(
        matches!(result,Err(lingxi_service::runs::DriveError::Storage(lingxi_kernel::ports::StorageError::Conflict{ref detail})) if detail=="session_busy")
    );
    assert_eq!(stub.requests().len(), 0);
    assert_eq!(count, 0);
    svc.finish().await;
    stub.stop().await;
}

#[tokio::test]
async fn public_borrowed_submission_rejects_mixed_real_database_before_effects() {
    let stub = StubServer::start(vec![openai_final("unexpected mixed dispatch")]).await;
    let a = HttpServer::start("mixed-a", &stub).await;
    let b = HttpServer::start("mixed-b", &stub).await;
    let principal = a
        .boot
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", a.boot.state.auth().local_token())),
            None,
            false,
            lingxi_service::transport::ConnectionKind::Local,
        )
        .unwrap();
    let result = a
        .boot
        .state
        .sessions()
        .execute_submission_for(
            b.boot.state.storage().as_ref(),
            a.boot.state.events(),
            a.boot.state.runs(),
            &principal,
            "sess_local_alpha",
            &lingxi_service::sessions::ExecuteSubmission::plain("mixed input"),
            1,
        )
        .await;
    let a_count = a
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 10)
        .await
        .unwrap()
        .len();
    let b_count = b
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 10)
        .await
        .unwrap()
        .len();
    save(
        "public-mixed-storage",
        serde_json::json!({"result":format!("{result:?}"),"wire_count":stub.requests().len(),"a_runs":a_count,"b_runs":b_count}),
    );
    assert!(
        matches!(result,Err(lingxi_service::sessions::SessionExecuteError::Storage(lingxi_kernel::ports::StorageError::Conflict{ref detail})) if detail=="session_storage_binding_mismatch")
    );
    assert_eq!(stub.requests().len(), 0);
    assert_eq!(a_count, 0);
    assert_eq!(b_count, 0);
    a.finish().await;
    b.finish().await;
    stub.stop().await;
}
#[tokio::test]
async fn actual_different_agent_child_keeps_exact_parent_permit() {
    let stub = StubServer::start(vec![openai_final("parent"), openai_final("child")]).await;
    let svc = HttpServer::start("other-agent-child-permit", &stub).await;
    let parent = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/sess_local_alpha/execute".into(),
        serde_json::json!({"input":"W03_SLOW_PARENT"}),
    ));
    for _ in 0..100 {
        if stub.requests().len() == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let run = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 1)
        .await
        .unwrap()[0]
        .run_id
        .clone();
    let scope = svc.boot.state.runs().run_scope(&run).unwrap();
    let child = svc
        .boot
        .state
        .subagents()
        .dispatch_child(
            lingxi_service::subagents::ParentRunFacts {
                principal: lingxi_kernel::Principal::LocalUser,
                session_id: "sess_local_alpha".into(),
                agent_id: "lingxi".into(),
                parent_run_id: lingxi_protocol::RunId::new(run.clone()),
                parent_scope: scope,
                source_model_call: lingxi_protocol::ModelCallId::new("w03-source"),
                cause_tool_call: lingxi_protocol::ToolCallId::new("w03-cause"),
                now_ms: 1791700000000,
            },
            lingxi_kernel::ports::DelegationRequest {
                task: "W03_SLOW_CHILD".into(),
                access: Some(lingxi_kernel::subagent::AccessRequest::Read),
                label: None,
                agent_id: Some("other".into()),
                model: None,
                thread_id: None,
            },
        )
        .await
        .unwrap();
    for _ in 0..100 {
        if stub.requests().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(stub.requests().len(), 2);
    stub.slow_release.add_permits(1);
    let parent_done = parent.await.unwrap();
    assert_eq!(parent_done.0, 200, "{}", parent_done.1);
    assert!(svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha"));
    let denied = svc
        .post(
            "/lingxi/v1/sessions/archive",
            serde_json::json!({"sessionId":"sess_local_alpha"}),
        )
        .await;
    assert_eq!(denied.0, 409, "{}", denied.1);
    stub.slow_release.add_permits(1);
    for _ in 0..100 {
        if !svc
            .boot
            .state
            .sessions()
            .session_supervisor()
            .is_busy("sess_local_alpha")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha"));
    save(
        "actual-other-agent-child",
        serde_json::json!({"parent_run":run,"child_run":child.child_run_id,"parent_completed":parent_done,"archive_while_child_working":denied,"busy_after_child_exit":false,"wire_count":stub.requests().len()}),
    );
    svc.finish().await;
    stub.stop().await;
}

async fn direct(
    state: &ServiceState,
    agent: &str,
    run: &str,
    input: &str,
    authorization: lingxi_service::runs::DriveAuthorization,
) -> Result<lingxi_kernel::RunFinish, lingxi_service::runs::DriveError> {
    state
        .runs()
        .drive_run(
            state.storage().as_ref(),
            state.events(),
            &lingxi_kernel::Principal::LocalUser,
            "sess_local_alpha",
            agent,
            run,
            input,
            1,
            1791700000000,
            None,
            None,
            authorization,
            None,
            "sess_local_alpha",
        )
        .await
}
fn plain_authorization() -> lingxi_service::runs::DriveAuthorization {
    lingxi_service::runs::DriveAuthorization::user_submission(
        None,
        lingxi_kernel::subagent::SessionPermissionMode::Operate,
    )
}
#[tokio::test]
async fn direct_driver_independent_database_reentry_and_release() {
    let stub = StubServer::start(vec![
        openai_final("slow a"),
        openai_final("b"),
        openai_final("a next"),
    ])
    .await;
    let a = HttpServer::start("direct-parallel-a", &stub).await;
    let b = HttpServer::start("direct-parallel-b", &stub).await;
    let agent = a
        .boot
        .state
        .storage()
        .get_session("sess_local_alpha")
        .await
        .unwrap()
        .unwrap()
        .agent_id;
    let state = a.boot.state.clone();
    let drive_agent = agent.clone();
    let running = tokio::spawn(async move {
        direct(
            &state,
            &drive_agent,
            "w03-direct-owned",
            "W03_SLOW_DIRECT",
            plain_authorization(),
        )
        .await
    });
    for _ in 0..100 {
        if stub.requests().len() == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(stub.requests().len(), 1);
    let reentry = direct(
        &a.boot.state,
        &agent,
        "w03-direct-owned",
        "second input",
        plain_authorization(),
    )
    .await;
    assert!(
        matches!(reentry,Err(lingxi_service::runs::DriveError::Storage(lingxi_kernel::ports::StorageError::Conflict{ref detail})) if detail=="run_driver_already_active")
    );
    let wrong_gate = a
        .boot
        .state
        .runs()
        .drive_run(
            b.boot.state.storage().as_ref(),
            a.boot.state.events(),
            &lingxi_kernel::Principal::LocalUser,
            "sess_local_alpha",
            &agent,
            "w03-direct-owned",
            "mixed gate",
            1,
            1791700000000,
            None,
            None,
            plain_authorization(),
            None,
            "sess_local_alpha",
        )
        .await;
    assert!(
        matches!(wrong_gate,Err(lingxi_service::runs::DriveError::Storage(lingxi_kernel::ports::StorageError::Conflict{ref detail})) if detail=="session_storage_binding_mismatch")
    );
    let archived = a
        .post(
            "/lingxi/v1/sessions/archive",
            serde_json::json!({"sessionId":"sess_local_alpha"}),
        )
        .await;
    assert_eq!(archived.0, 409);
    assert_eq!(stub.requests().len(), 1);
    assert_eq!(
        b.boot
            .state
            .storage()
            .count_runs("sess_local_alpha")
            .await
            .unwrap(),
        0
    );
    assert!(direct(
        &b.boot.state,
        &agent,
        "w03-direct-owned",
        "independent b",
        plain_authorization()
    )
    .await
    .is_ok());
    assert_eq!(stub.requests().len(), 2);
    stub.slow_release.add_permits(1);
    assert!(running.await.unwrap().is_ok());
    assert!(!a
        .boot
        .state
        .storage()
        .session_admission()
        .is_busy("sess_local_alpha"));
    assert!(direct(
        &a.boot.state,
        &agent,
        "w03-direct-next",
        "next a",
        plain_authorization()
    )
    .await
    .is_ok());
    assert_eq!(stub.requests().len(), 3);
    assert_eq!(
        a.boot
            .state
            .storage()
            .count_runs("sess_local_alpha")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        b.boot
            .state
            .storage()
            .count_runs("sess_local_alpha")
            .await
            .unwrap(),
        1
    );
    save(
        "direct-driver-parallel",
        serde_json::json!({"reentry":format!("{reentry:?}"),"wrong_gate":format!("{wrong_gate:?}"),"archive":archived,"wire_count":3,"a_runs":2,"b_runs":1,"busy_after_actual_exit":false}),
    );
    a.finish().await;
    b.finish().await;
    stub.stop().await;
}
#[tokio::test]
async fn direct_driver_fake_agent_and_child_have_zero_effects() {
    let stub = StubServer::start(vec![]).await;
    let svc = HttpServer::start("direct-forgeries", &stub).await;
    let agent = svc
        .boot
        .state
        .storage()
        .get_session("sess_local_alpha")
        .await
        .unwrap()
        .unwrap()
        .agent_id;
    let fake_agent = direct(
        &svc.boot.state,
        "fake-other-agent",
        "w03-fake-agent",
        "fake agent",
        plain_authorization(),
    )
    .await;
    let mut child = plain_authorization();
    child.grant = lingxi_service::runs::RunGrant::Subagent {
        tier: lingxi_kernel::subagent::ToolAccessTier::ReadOnly,
    };
    child.lineage.origin = lingxi_kernel::subagent::RunOrigin::Subagent;
    child.lineage.parent_run_id = Some(lingxi_protocol::RunId::new("fake-parent"));
    let fake_child = direct(
        &svc.boot.state,
        &agent,
        "w03-fake-child",
        "fake child",
        child,
    )
    .await;
    for result in [&fake_agent, &fake_child] {
        assert!(
            matches!(result,Err(lingxi_service::runs::DriveError::Storage(lingxi_kernel::ports::StorageError::Conflict{detail})) if detail=="run_authorization_not_admitted")
        );
    }
    assert_eq!(stub.requests().len(), 0);
    assert_eq!(
        svc.boot
            .state
            .storage()
            .count_runs("sess_local_alpha")
            .await
            .unwrap(),
        0
    );
    assert!(!svc
        .boot
        .state
        .storage()
        .session_admission()
        .is_busy("sess_local_alpha"));
    save(
        "direct-driver-forgeries",
        serde_json::json!({"fake_agent":format!("{fake_agent:?}"),"fake_child":format!("{fake_child:?}"),"wire_count":0,"run_count":0,"busy":false}),
    );
    svc.finish().await;
    stub.stop().await;
}

#[tokio::test]
async fn subagent_exact_parent_scope_and_settled_parent_are_enforced() {
    let stub = StubServer::start(vec![
        openai_final("old parent"),
        openai_final("slow parent"),
        openai_final("settled parent child"),
    ])
    .await;
    let svc = HttpServer::start("parent-authority", &stub).await;
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"old completed same-session parent"})
        )
        .await
        .0,
        200
    );
    let old_parent = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 1)
        .await
        .unwrap()[0]
        .run_id
        .clone();
    let pending = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/sess_local_alpha/execute".into(),
        serde_json::json!({"input":"W03_SLOW_PARENT_AUTHORITY"}),
    ));
    for _ in 0..100 {
        if stub.requests().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(stub.requests().len(), 2);
    let parent_run = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 1)
        .await
        .unwrap()[0]
        .run_id
        .clone();
    let request = || lingxi_kernel::ports::DelegationRequest {
        task: "child".into(),
        access: Some(lingxi_kernel::subagent::AccessRequest::Read),
        label: None,
        agent_id: Some("other".into()),
        model: None,
        thread_id: None,
    };
    let facts = |run: &str,
                 principal: lingxi_kernel::Principal,
                 scope: lingxi_service::cancel::CancelScope| {
        lingxi_service::subagents::ParentRunFacts {
            principal,
            session_id: "sess_local_alpha".into(),
            agent_id: "lingxi".into(),
            parent_run_id: lingxi_protocol::RunId::new(run),
            parent_scope: scope,
            source_model_call: lingxi_protocol::ModelCallId::new("w03-parent-source"),
            cause_tool_call: lingxi_protocol::ToolCallId::new("w03-parent-cause"),
            now_ms: 1791700000000,
        }
    };
    let fake_scope = svc
        .boot
        .state
        .subagents()
        .dispatch_child(
            facts(
                &parent_run,
                lingxi_kernel::Principal::LocalUser,
                lingxi_service::cancel::CancelScope::run_root(&parent_run),
            ),
            request(),
        )
        .await;
    let wrong_live_parent = svc
        .boot
        .state
        .subagents()
        .dispatch_child(
            facts(
                &old_parent,
                lingxi_kernel::Principal::LocalUser,
                lingxi_service::cancel::CancelScope::run_root(&old_parent),
            ),
            request(),
        )
        .await;
    assert!(format!("{wrong_live_parent:?}").contains("session_busy"));
    let missing_parent = svc
        .boot
        .state
        .subagents()
        .dispatch_child(
            facts(
                "non-ancestor",
                lingxi_kernel::Principal::LocalUser,
                lingxi_service::cancel::CancelScope::run_root("non-ancestor"),
            ),
            request(),
        )
        .await;
    let foreign = svc
        .boot
        .state
        .subagents()
        .dispatch_child(
            facts(
                &parent_run,
                lingxi_kernel::Principal::Device {
                    device_id: "foreign-device".into(),
                    user_id: "foreign-user".into(),
                },
                svc.boot.state.runs().run_scope(&parent_run).unwrap(),
            ),
            request(),
        )
        .await;
    assert!(format!("{fake_scope:?}").contains("subagent_parent_scope_mismatch"));
    assert!(format!("{missing_parent:?}").contains("subagent_parent_ownership_mismatch"));
    assert!(format!("{foreign:?}").contains("subagent_parent_ownership_mismatch"));
    assert_eq!(stub.requests().len(), 2);
    assert_eq!(
        svc.boot
            .state
            .storage()
            .count_runs("sess_local_alpha")
            .await
            .unwrap(),
        2
    );
    stub.slow_release.add_permits(1);
    assert_eq!(pending.await.unwrap().0, 200);
    let child = svc
        .boot
        .state
        .subagents()
        .dispatch_child(
            facts(
                &parent_run,
                lingxi_kernel::Principal::LocalUser,
                lingxi_service::cancel::CancelScope::run_root(&parent_run),
            ),
            request(),
        )
        .await
        .unwrap();
    for _ in 0..100 {
        if !svc
            .boot
            .state
            .storage()
            .session_admission()
            .is_busy("sess_local_alpha")
            && stub.requests().len() == 3
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(stub.requests().len(), 3);
    assert_eq!(
        svc.boot
            .state
            .storage()
            .count_runs("sess_local_alpha")
            .await
            .unwrap(),
        3
    );
    assert!(!svc
        .boot
        .state
        .storage()
        .session_admission()
        .is_busy("sess_local_alpha"));
    save(
        "subagent-parent-authority",
        serde_json::json!({"fake_scope":format!("{fake_scope:?}"),"wrong_live_parent":format!("{wrong_live_parent:?}"),"missing_parent":format!("{missing_parent:?}"),"foreign":format!("{foreign:?}"),"completed_parent":parent_run,"legal_other_agent_child":child.child_run_id,"wire_count":3,"run_count":3,"busy":false,"persona_contract":"F05/EX03 remains separately open; this case proves admission and actual wire only"}),
    );
    svc.finish().await;
    stub.stop().await;
}

#[tokio::test]
async fn retired_seed_identity_stays_deleted_and_no_journal_reuse_is_preserved() {
    let stub = StubServer::start(vec![
        openai_final("alpha"),
        openai_final("beta"),
        openai_final("reused plain identity"),
    ])
    .await;
    let svc = HttpServer::start("retired-seed", &stub).await;
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"retire source with actual message"})
        )
        .await
        .0,
        200
    );
    let source_branch = lingxi_adapters::storage::session_tree::list_branch_messages(
        svc.boot.state.storage(),
        "sess_local_alpha",
    )
    .await
    .unwrap();
    let checkpoint = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/checkpoints",
            serde_json::json!({"name":"retirement-witness","targetMessageId":source_branch[0].message_id,"filePaths":[]}),
        )
        .await;
    assert_eq!(checkpoint.0, 200, "{}", checkpoint.1);
    let durable_operation=svc.post("/lingxi/v1/sessions/sess_local_alpha/rewind",serde_json::json!({"checkpoint":"retirement-witness","restoreFiles":false,"requestId":"default-retirement-op"})).await;
    assert_eq!(durable_operation.0, 200, "{}", durable_operation.1);
    assert_eq!(
        svc.boot
            .state
            .storage()
            .query_one_text(
                "SELECT COUNT(*) FROM session_operations WHERE session_id=?1",
                vec!["sess_local_alpha".into()]
            )
            .await
            .unwrap()
            .as_deref(),
        Some("1")
    );
    for path in [
        "/lingxi/v1/sessions/archive",
        "/lingxi/v1/sessions/archived/delete",
    ] {
        assert_eq!(
            svc.post(path, serde_json::json!({"sessionId":"sess_local_alpha"}))
                .await
                .0,
            200
        );
    }
    let home = svc.boot.home.clone();
    let workspace = svc.boot.workspace.clone();
    let config_path = svc.boot.config_path.clone();
    svc.finish().await;
    let svc = HttpServer::from_boot(reopen(home, workspace, config_path).await).await;
    assert!(svc
        .boot
        .state
        .storage()
        .get_session("sess_local_alpha")
        .await
        .unwrap()
        .is_none());
    assert!(svc
        .boot
        .state
        .storage()
        .get_session("sess_local_beta")
        .await
        .unwrap()
        .is_some());
    let execute_retired = svc
        .post(
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            serde_json::json!({"input":"must stay deleted"}),
        )
        .await;
    assert_eq!(execute_retired.0, 404);
    assert_eq!(stub.requests().len(), 1);
    let receipt = http(
        &svc.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/operations",
        &[(
            "Authorization",
            &format!("Bearer {}", svc.boot.state.auth().local_token()),
        )],
        None,
    )
    .await;
    assert_eq!(receipt.0, 200);
    let create_retired=svc.post("/lingxi/v1/sessions",serde_json::json!({"sessionId":"sess_local_alpha","agentId":"lingxi","title":"forbidden recycle"})).await;
    assert_eq!(create_retired.0, 409);
    assert!(create_retired.1.contains("session_identity_retired"));
    assert_eq!(
        svc.post(
            "/lingxi/v1/sessions/sess_local_beta/execute",
            serde_json::json!({"input":"beta still operates"})
        )
        .await
        .0,
        200
    );
    let branch = lingxi_adapters::storage::session_tree::list_branch_messages(
        svc.boot.state.storage(),
        "sess_local_beta",
    )
    .await
    .unwrap();
    let retired_fork=svc.post("/lingxi/v1/sessions/sess_local_beta/fork",serde_json::json!({"newSessionId":"sess_local_alpha","boundaryMessageId":branch[0].message_id})).await;
    assert_eq!(retired_fork.0, 409);
    assert!(retired_fork.1.contains("session_identity_retired"));
    assert!(svc
        .boot
        .state
        .storage()
        .get_session("sess_local_alpha")
        .await
        .unwrap()
        .is_none());
    let body =
        serde_json::json!({"sessionId":"w03-plain-reuse","agentId":"lingxi","title":"plain"});
    assert_eq!(svc.post("/lingxi/v1/sessions", body.clone()).await.0, 201);
    for path in [
        "/lingxi/v1/sessions/archive",
        "/lingxi/v1/sessions/archived/delete",
    ] {
        assert_eq!(
            svc.post(path, serde_json::json!({"sessionId":"w03-plain-reuse"}))
                .await
                .0,
            200
        );
    }
    let recreated = svc.post("/lingxi/v1/sessions", body).await;
    assert_eq!(recreated.0, 201);
    let executes = svc
        .post(
            "/lingxi/v1/sessions/w03-plain-reuse/execute",
            serde_json::json!({"input":"legal no-journal reuse"}),
        )
        .await;
    assert_eq!(executes.0, 200);
    assert_eq!(stub.requests().len(), 3);
    save(
        "retired-default-seed",
        serde_json::json!({"default_seed_after_restart":null,"retired_execute":execute_retired,"own_receipt":receipt,"retired_create":create_retired,"retired_fork":retired_fork,"no_journal_recreate":recreated,"no_journal_execute":executes,"other_seed_preserved":true,"wire_count":3}),
    );
    svc.finish().await;
    stub.stop().await;
}

#[tokio::test]
async fn local_owner_cross_session_owner_child_and_foreign_before_busy_are_preserved() {
    let stub = StubServer::start(vec![
        openai_final("local parent"),
        openai_final("local child"),
        openai_final("owner device"),
    ])
    .await;
    let svc = HttpServer::start("local-owner-contract", &stub).await;
    let issued = svc
        .post(
            "/lingxi/v1/devices/credentials",
            serde_json::json!({"userId":"owned-user-nine","scopes":["chat"]}),
        )
        .await;
    assert_eq!(issued.0, 201);
    let credential: serde_json::Value = serde_json::from_str(&issued.1).unwrap();
    let secret = credential["secret"].as_str().unwrap();
    let authenticated = svc
        .boot
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {secret}")),
            None,
            false,
            lingxi_service::transport::ConnectionKind::Local,
        )
        .unwrap();
    let owner = lingxi_kernel::Principal::Device {
        device_id: authenticated.device_id.clone().unwrap(),
        user_id: authenticated.user_id.clone().unwrap(),
    };
    let created=http(&svc.addr,"POST","/lingxi/v1/sessions",&[("Authorization",&format!("Bearer {secret}"))],Some(&serde_json::json!({"sessionId":"w03-owned-nine","agentId":"lingxi","title":"device owner"}).to_string())).await;
    assert_eq!(created.0, 201, "{}", created.1);
    let state = svc.boot.state.clone();
    let pending = tokio::spawn(async move {
        state
            .runs()
            .drive_run(
                state.storage().as_ref(),
                state.events(),
                &lingxi_kernel::Principal::LocalUser,
                "w03-owned-nine",
                "lingxi",
                "w03-local-cross-owner",
                "W03_SLOW_LOCAL_OTHER_OWNER",
                1,
                1791700000000,
                None,
                None,
                plain_authorization(),
                None,
                "w03-owned-nine",
            )
            .await
    });
    for _ in 0..100 {
        if stub.requests().len() == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(stub.requests().len(), 1);
    let child = svc
        .boot
        .state
        .subagents()
        .dispatch_child(
            lingxi_service::subagents::ParentRunFacts {
                principal: lingxi_kernel::Principal::LocalUser,
                session_id: "w03-owned-nine".into(),
                agent_id: "lingxi".into(),
                parent_run_id: lingxi_protocol::RunId::new("w03-local-cross-owner"),
                parent_scope: svc
                    .boot
                    .state
                    .runs()
                    .run_scope("w03-local-cross-owner")
                    .unwrap(),
                source_model_call: lingxi_protocol::ModelCallId::new("w03-owner-source"),
                cause_tool_call: lingxi_protocol::ToolCallId::new("w03-owner-cause"),
                now_ms: 1791700000000,
            },
            lingxi_kernel::ports::DelegationRequest {
                task: "W03_SLOW_LOCAL_CHILD".into(),
                access: Some(lingxi_kernel::subagent::AccessRequest::Read),
                label: None,
                agent_id: Some("other".into()),
                model: None,
                thread_id: None,
            },
        )
        .await
        .unwrap();
    for _ in 0..100 {
        if stub.requests().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(stub.requests().len(), 2);
    stub.slow_release.add_permits(1);
    assert!(pending.await.unwrap().is_ok());
    assert!(svc
        .boot
        .state
        .storage()
        .session_admission()
        .is_busy("w03-owned-nine"));
    let foreign = svc
        .boot
        .state
        .runs()
        .drive_run(
            svc.boot.state.storage().as_ref(),
            svc.boot.state.events(),
            &lingxi_kernel::Principal::Device {
                device_id: "wrong-owner-device".into(),
                user_id: "wrong-owner".into(),
            },
            "w03-owned-nine",
            "lingxi",
            "w03-foreign-owner",
            "forbidden",
            1,
            1791700000000,
            None,
            None,
            plain_authorization(),
            None,
            "w03-owned-nine",
        )
        .await;
    assert!(
        matches!(foreign,Err(lingxi_service::runs::DriveError::Storage(lingxi_kernel::ports::StorageError::Conflict{ref detail})) if detail=="cross_principal_access")
    );
    let owner_busy = svc
        .boot
        .state
        .runs()
        .drive_run(
            svc.boot.state.storage().as_ref(),
            svc.boot.state.events(),
            &owner,
            "w03-owned-nine",
            "lingxi",
            "w03-own-device",
            "busy",
            1,
            1791700000000,
            None,
            None,
            plain_authorization(),
            None,
            "w03-owned-nine",
        )
        .await;
    assert!(
        matches!(owner_busy,Err(lingxi_service::runs::DriveError::Storage(lingxi_kernel::ports::StorageError::Conflict{ref detail})) if detail=="session_busy")
    );
    assert_eq!(stub.requests().len(), 2);
    assert_eq!(
        svc.boot
            .state
            .storage()
            .count_runs("w03-owned-nine")
            .await
            .unwrap(),
        2
    );
    stub.slow_release.add_permits(1);
    for _ in 0..100 {
        if !svc
            .boot
            .state
            .storage()
            .session_admission()
            .is_busy("w03-owned-nine")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!svc
        .boot
        .state
        .storage()
        .session_admission()
        .is_busy("w03-owned-nine"));
    let owner_next = svc
        .boot
        .state
        .runs()
        .drive_run(
            svc.boot.state.storage().as_ref(),
            svc.boot.state.events(),
            &owner,
            "w03-owned-nine",
            "lingxi",
            "w03-owner-next",
            "owner after actual exit",
            1,
            1791700000000,
            None,
            None,
            plain_authorization(),
            None,
            "w03-owned-nine",
        )
        .await;
    assert!(owner_next.is_ok());
    assert_eq!(stub.requests().len(), 3);
    let source = svc
        .boot
        .state
        .storage()
        .get_session("w03-owned-nine")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(source.owner_user_id, "owned-user-nine");
    let parent_actor = svc
        .boot
        .state
        .storage()
        .query_one_text(
            "SELECT owner_kind||':'||owner_subject FROM runs WHERE run_id=?1",
            vec!["w03-local-cross-owner".into()],
        )
        .await
        .unwrap();
    assert_eq!(parent_actor.as_deref(), Some("local_user:user_local"));
    save(
        "local-owner-provenance",
        serde_json::json!({"session_owner":source.owner_user_id,"parent_actor":parent_actor,"legal_child":child.child_run_id,"foreign_before_busy":format!("{foreign:?}"),"owner_busy":format!("{owner_busy:?}"),"owner_next":format!("{owner_next:?}"),"wire_count":3,"run_count":3,"busy_after_actual_exit":false}),
    );
    svc.finish().await;
    stub.stop().await;
}

// 真实 blocking pool 写入不能随等待 future 的取消提前释放会话准入。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_native_write_cancel_retains_admission_until_blocking_exit() {
    native_blocking_cancel("write").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_native_edit_cancel_retains_admission_until_blocking_exit() {
    native_blocking_cancel("edit").await;
}
async fn native_blocking_cancel(kind: &str) {
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let parked = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    struct Release(Arc<(Mutex<bool>, std::sync::Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            *self.0 .0.lock().unwrap() = true;
            self.0 .1.notify_all();
        }
    }
    let release = Release(parked.clone());
    let hook_entered = entered.clone();
    let hook_parked = parked.clone();
    let hook: lingxi_service::filetools::MutationTestHook = Arc::new(move |_| {
        hook_entered.store(true, Ordering::SeqCst);
        let mut released = hook_parked.0.lock().unwrap();
        while !*released {
            released = hook_parked.1.wait(released).unwrap();
        }
    });
    let tool_response = |name: &str, args: serde_json::Value| {
        openai_sse(&[
            serde_json::json!({"id":"chatcmpl-file","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call-native-file","type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]}),
            serde_json::json!({"id":"chatcmpl-file","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ])
    };
    let mut responses = Vec::new();
    if kind == "edit" {
        responses.push(tool_response(
            "read",
            serde_json::json!({"path":"native-cancel.txt"}),
        ));
    }
    responses.push(tool_response(kind, if kind == "edit" {
        serde_json::json!({"path":"native-cancel.txt","edits":[{"oldText":"before edit","newText":"late write"}]})
    } else { serde_json::json!({"path":"native-cancel.txt","content":"late write"}) }));
    responses.extend([openai_final("independent"), openai_final("next")]);
    let stub = StubServer::start(responses).await;
    let plane = format!(
        r#""providers": {{"fixture": {{"endpoint": {}, "protocol": "openai-completions", "auth": {{"kind": "none"}}}}}}, "models": {{"chat": {{"provider":"fixture","model":"fixture-model","capabilities": {{"tools":true}}, "compat":{{"contextWindow":1000000}}}}}}"#,
        serde_json::to_string(&stub.v1()).unwrap()
    );
    let svc = HttpServer::from_boot(
        boot_with_config_hook(&format!("native-blocking-{kind}"), &plane, Some(hook)).await,
    )
    .await;
    let target = svc.boot.workspace.join("native-cancel.txt");
    if kind == "edit" {
        std::fs::write(&target, "before edit").unwrap();
    }
    let caller = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/sess_local_alpha/execute".into(),
        serde_json::json!({"input":"actual native write","requestId":"native-write"}),
    ));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !entered.load(Ordering::SeqCst) {
        assert!(
            std::time::Instant::now() < deadline,
            "真实 native write 未进入临时写后屏障"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(
        std::fs::read_to_string(&target).ok(),
        if kind == "edit" {
            Some("before edit".into())
        } else {
            None
        },
        "rename 前原状态保持"
    );
    let run = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 1)
        .await
        .unwrap()[0]
        .run_id
        .clone();
    let principal = svc
        .boot
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", svc.boot.state.auth().local_token())),
            None,
            false,
            lingxi_service::transport::ConnectionKind::Local,
        )
        .unwrap();
    svc.boot
        .state
        .sessions()
        .cancel_run_for(
            svc.boot.state.storage().as_ref(),
            svc.boot.state.runs(),
            &principal,
            &run,
        )
        .await
        .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), caller)
        .await
        .unwrap()
        .unwrap();
    let while_parked_busy = svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha");
    let archive = svc
        .post(
            "/lingxi/v1/sessions/archive",
            serde_json::json!({"sessionId":"sess_local_alpha"}),
        )
        .await;
    let beta = svc
        .post(
            "/lingxi/v1/sessions/sess_local_beta/execute",
            serde_json::json!({"input":"independent"}),
        )
        .await;
    let before = std::fs::read_to_string(&target).ok();
    // 失败断言前也必须释放真实线程，避免失败夹具遗留工作或阻塞 runtime 退出。
    drop(release);
    while std::fs::read_to_string(&target).ok().as_deref() != Some("late write") {
        assert!(
            std::time::Instant::now() < deadline,
            "blocking closure 未实际退出"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let after = std::fs::read_to_string(&target).unwrap();
    while svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha")
    {
        assert!(std::time::Instant::now() < deadline, "真实线程退出后应释放");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    save(
        &format!("native-blocking-{kind}"),
        serde_json::json!({"run":run,"cancel_response":completed,"busy_while_closure_alive":while_parked_busy,"archive_while_closure_alive":archive,"beta":beta,"file_exists_before_release":before,"file_after_release":after,"actual_blocking_exit_observed":true,"wire_count":stub.requests().len()}),
    );
    svc.finish().await;
    stub.stop().await;
    assert_eq!(completed.0, 200);
    assert!(
        while_parked_busy,
        "真实 write closure 尚活，会话准入不得提前释放"
    );
    assert_eq!(archive.0, 409, "{archive:?}");
    assert_eq!(beta.0, 200, "{beta:?}");
    assert_eq!(
        before,
        if kind == "edit" {
            Some("before edit".into())
        } else {
            None
        }
    );
    assert_eq!(after, "late write");
}

// 仅可信宿主新增转发工具；被测 ResourceIO/Gateway/driver/store 均为真实生产组件。
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_resourceio_owned_write_cancel_retains_until_fifo_exit() {
    use lingxi_kernel::ports::{ToolExecutionResult, ToolExecutorPort};
    struct IoForward {
        io: Arc<lingxi_service::resourceio::ResourceIoService>,
        same: Arc<lingxi_service::resourceio::ResourceIoService>,
        foreign: Arc<lingxi_service::resourceio::ResourceIoService>,
        path: PathBuf,
        foreign_path: PathBuf,
    }
    impl ToolExecutorPort for IoForward {
        fn execute<'a>(
            &'a self,
            ctx: &'a lingxi_kernel::RunContext,
            _call: &'a lingxi_protocol::ToolCallId,
            _request: &'a lingxi_kernel::ports::ToolRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>>
        {
            Box::pin(async move {
                let mut op =
                    lingxi_service::resourceio::OpContext::local_owner(ctx.session_id.as_str());
                op.emit = false;
                let sibling = self.path.with_file_name("same-backend.txt");
                self.same
                    .write(
                        &serde_json::json!({"kind":"local-file","path":sibling}),
                        b"same backend".to_vec(),
                        &op,
                    )
                    .await
                    .unwrap();
                let mut wrong_actor = op.clone();
                wrong_actor.principal_subject = "foreign".into();
                let actor_path = self.path.with_file_name("wrong-actor.txt");
                assert_eq!(
                    self.io
                        .write(
                            &serde_json::json!({"kind":"local-file","path":actor_path}),
                            b"forbidden".to_vec(),
                            &wrong_actor
                        )
                        .await
                        .unwrap_err()
                        .status,
                    409
                );
                assert!(!actor_path.exists());
                let mut wrong_session = op.clone();
                wrong_session.session_id = "sess_local_beta".into();
                let sid_path = self.path.with_file_name("wrong-session.txt");
                assert_eq!(
                    self.io
                        .write(
                            &serde_json::json!({"kind":"local-file","path":sid_path}),
                            b"forbidden".to_vec(),
                            &wrong_session
                        )
                        .await
                        .unwrap_err()
                        .status,
                    409
                );
                assert!(!sid_path.exists());
                assert_eq!(
                    self.foreign
                        .write(
                            &serde_json::json!({"kind":"local-file","path":self.foreign_path}),
                            b"forbidden".to_vec(),
                            &op
                        )
                        .await
                        .unwrap_err()
                        .status,
                    409
                );
                assert!(!self.foreign_path.exists());
                let result = self
                    .io
                    .write(
                        &serde_json::json!({"kind":"local-file","path":self.path}),
                        vec![b'x'; 1024 * 1024],
                        &op,
                    )
                    .await;
                let outcome = match result {
                    Ok(_) => {
                        lingxi_kernel::ports::ToolOutcome::success_text("true ResourceIO write")
                    }
                    Err(err) => lingxi_kernel::ports::ToolOutcome::Failed {
                        error: lingxi_protocol::ProtocolError::new(
                            lingxi_protocol::ErrorCode::Conflict,
                            err.to_string(),
                            false,
                        ),
                    },
                };
                ToolExecutionResult::of_ctx(ctx, outcome)
            })
        }
    }
    let stub = StubServer::start(vec![openai_sse(&[
        serde_json::json!({"id":"chatcmpl-io","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call-real-io","type":"function","function":{"name":"resource_fifo_write","arguments":"{}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-io","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ]), openai_final("independent"), openai_final("after actual io exit")]).await;
    let svc = HttpServer::start("resourceio-blocking", &stub).await;
    let foreign_boot = boot("resourceio-foreign", &stub).await;
    let same_db = Arc::new(
        lingxi_adapters::storage::RunDatabase::open(
            svc.boot.state.storage().db_path(),
            lingxi_adapters::storage::StoreOptions::default(),
        )
        .await
        .unwrap(),
    );
    assert!(Arc::ptr_eq(
        same_db.session_admission(),
        svc.boot.state.storage().session_admission()
    ));
    let same_files = Arc::new(lingxi_service::sessionfiles::SessionFileService::new(
        svc.boot.home.clone(),
        same_db.clone(),
        || 1791700000000,
    ));
    let same_io = Arc::new(lingxi_service::resourceio::ResourceIoService::new(
        same_files,
        svc.boot.state.resources().clone(),
        svc.boot.home.clone(),
        Some((
            Arc::new(
                lingxi_service::resourceaccess::ResourceAccess::new(std::slice::from_ref(
                    &svc.boot.workspace,
                ))
                .unwrap(),
            ),
            svc.boot.workspace.clone(),
        )),
        lingxi_service::resourceio::DEFAULT_WATCH_POLL_MS,
        Arc::new(|| 1791700000000),
    ));
    let fifo = svc.boot.workspace.join("actual-write.fifo");
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success());
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let released = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    struct FifoRelease {
        fifo: PathBuf,
        entered: Arc<std::sync::atomic::AtomicBool>,
        released: Arc<(Mutex<bool>, std::sync::Condvar)>,
    }
    impl Drop for FifoRelease {
        fn drop(&mut self) {
            if !self.entered.load(Ordering::SeqCst) {
                use std::io::Write;
                let mut dummy = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&self.fifo)
                    .unwrap();
                dummy.write_all(b"!").unwrap();
            }
            *self.released.0.lock().unwrap() = true;
            self.released.1.notify_all();
        }
    }
    let release = FifoRelease {
        fifo: fifo.clone(),
        entered: entered.clone(),
        released: released.clone(),
    };
    let reader_path = fifo.clone();
    let actual_entered = entered.clone();
    let actual_released = released.clone();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut file = std::fs::File::open(reader_path).unwrap();
        let mut first = [0u8; 1];
        file.read_exact(&mut first).unwrap();
        actual_entered.store(true, Ordering::SeqCst);
        let mut done = actual_released.0.lock().unwrap();
        while !*done {
            done = actual_released.1.wait(done).unwrap();
        }
        drop(done);
        let mut bytes = first.to_vec();
        file.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let gateway = svc.boot.state.tool_gateway().unwrap();
    let budget = lingxi_kernel::toolcatalog::SchemaBudget::default();
    let target = gateway.registry().register(lingxi_kernel::toolcatalog::ToolManifest {
        origin: lingxi_kernel::toolcatalog::ToolOrigin::FirstParty, local_name:"resource_fifo_write".into(), display_name:"ResourceIO forward".into(), aliases:vec![], version:"1.0.0".into(), description:"可信宿主真实 ResourceIO 写接口转发".into(),
        input_schema:lingxi_protocol::ToolSchemaDocument { dialect:"json-schema/2020-12".into(), schema:serde_json::json!({"type":"object","properties":{},"additionalProperties":false}) }, output_schema:None,
        permission:lingxi_kernel::toolcatalog::PermissionContract {kind:lingxi_kernel::toolcatalog::PermissionKind::Execute,capability_base:"resource_fifo_write.files".into()}, availability:lingxi_kernel::toolcatalog::Availability::Available, timeout_ms:Some(60000),max_concurrency:None,declared_permission:lingxi_kernel::toolcatalog::DeclaredPermission::Execute,recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    }, &budget).unwrap().target_id;
    gateway.bind_executor(
        target,
        Arc::new(IoForward {
            io: svc.boot.state.resource_io().clone(),
            same: same_io,
            foreign: foreign_boot.state.resource_io().clone(),
            foreign_path: foreign_boot.workspace.join("forbidden.txt"),
            path: fifo.clone(),
        }),
        "可信宿主新增 owned-run 接口消费者，不替换原生工具",
    );
    let caller = tokio::spawn(post_at(
        svc.addr,
        svc.boot.state.auth().local_token(),
        "/lingxi/v1/sessions/sess_local_alpha/execute".into(),
        serde_json::json!({"input":"real owned ResourceIO","requestId":"owned-io"}),
    ));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !entered.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let actual_write_entered = entered.load(Ordering::SeqCst);
    let run = svc
        .boot
        .state
        .storage()
        .recent_runs("sess_local_alpha", 1)
        .await
        .unwrap()[0]
        .run_id
        .clone();
    let forged_scope = lingxi_service::cancel::CancelScope::run_root(&run).child(
        "forged-resource-scope".into(),
        lingxi_service::cancel::ScopeKind::ToolCall,
    );
    let exact_io = svc.boot.state.resource_io().clone();
    let fake_path = svc.boot.workspace.join("wrong-scope.txt");
    let attempt_path = fake_path.clone();
    let scope_probe = svc
        .boot
        .state
        .runs()
        .task_supervisor()
        .spawn_linked(
            &run,
            &forged_scope,
            "actual wrong-scope ResourceIO consumer".into(),
            async move {
                exact_io
                    .write(
                        &serde_json::json!({"kind":"local-file","path":attempt_path}),
                        b"forbidden".to_vec(),
                        &lingxi_service::resourceio::OpContext::local_owner("sess_local_alpha"),
                    )
                    .await
            },
        )
        .unwrap()
        .wait()
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(scope_probe.status, 409);
    assert!(!fake_path.exists());
    let principal = svc
        .boot
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", svc.boot.state.auth().local_token())),
            None,
            false,
            lingxi_service::transport::ConnectionKind::Local,
        )
        .unwrap();
    svc.boot
        .state
        .sessions()
        .cancel_run_for(
            svc.boot.state.storage().as_ref(),
            svc.boot.state.runs(),
            &principal,
            &run,
        )
        .await
        .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), caller)
        .await
        .unwrap()
        .unwrap();
    let manual_path = svc.boot.workspace.join("manual-while-owned.txt");
    let manual=svc.post("/lingxi/v1/resource-io/write",serde_json::json!({"sessionId":"sess_local_alpha","resource":{"kind":"local-file","path":manual_path},"content":"manual allowed"})).await;
    assert_eq!(manual.0, 200, "手工 HTTP 在 run 忙时仍可编辑: {manual:?}");
    let empty_path = svc.boot.workspace.join("empty-sid-manual.txt");
    svc.boot
        .state
        .resource_io()
        .write(
            &serde_json::json!({"kind":"local-file","path":empty_path}),
            b"empty sid allowed".to_vec(),
            &lingxi_service::resourceio::OpContext::local_owner(""),
        )
        .await
        .unwrap();
    let busy = svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha");
    let archive = svc
        .post(
            "/lingxi/v1/sessions/archive",
            serde_json::json!({"sessionId":"sess_local_alpha"}),
        )
        .await;
    let beta = svc
        .post(
            "/lingxi/v1/sessions/sess_local_beta/execute",
            serde_json::json!({"input":"independent"}),
        )
        .await;
    drop(release);
    let bytes = reader.join().unwrap();
    while svc
        .boot
        .state
        .sessions()
        .session_supervisor()
        .is_busy("sess_local_alpha")
    {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let next = if archive.0 == 409 {
        Some(
            svc.post(
                "/lingxi/v1/sessions/sess_local_alpha/execute",
                serde_json::json!({"input":"after actual io exit"}),
            )
            .await,
        )
    } else {
        None
    };
    save(
        "resourceio-blocking",
        serde_json::json!({"run":run,"boundary":"trusted owned-run forwarding consumer; current native tools do not call ResourceIO","actual_fifo_first_byte_observed":actual_write_entered,"same_actual_db_two_storage_objects":actual_write_entered,"foreign_actual_backend_zero_files":actual_write_entered,"wrong_actor_and_session_zero_files":actual_write_entered,"wrong_scope_status":scope_probe.status,"manual_http":manual,"manual_file_content":std::fs::read_to_string(&manual_path).unwrap(),"empty_sid_content":std::fs::read_to_string(&empty_path).unwrap(),"cancel":completed,"busy_while_real_write_alive":busy,"archive":archive,"beta":beta,"bytes_after_actual_exit":bytes.len(),"next":next,"wire_count":stub.requests().len()}),
    );
    same_db.close().await.unwrap();
    foreign_boot.state.storage().close().await.unwrap();
    svc.finish().await;
    stub.stop().await;
    assert!(
        actual_write_entered,
        "实际FIFO首字节证明真实写已进入且尚未结束"
    );
    assert_eq!(bytes, vec![b'x'; 1024 * 1024]);
    assert_eq!(completed.0, 200);
    assert!(busy, "ResourceIO blocking write 尚活不能释放同根准入");
    assert_eq!(archive.0, 409);
    assert_eq!(beta.0, 200);
    assert_eq!(next.unwrap().0, 200);
}
