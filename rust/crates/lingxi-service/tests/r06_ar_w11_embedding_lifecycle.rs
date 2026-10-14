//! W11：真实服务、Gateway、driver、存储；仅网络远端为合成loopback。
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r06-ar-w11-{tag}-{}-{}",
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

async fn wait_until_job(
    server: &ApiClient,
    notebook_id: &str,
    source_id: &str,
    expect_status: &str,
) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (status, jobs) = server
            .request(
                "GET",
                &format!(
                    "/lingxi/v1/knowledge/ingestion?notebookId={notebook_id}&sourceId={source_id}"
                ),
                None,
            )
            .await;
        assert_eq!(status, 200, "{jobs}");
        let hit = jobs["jobs"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .find(|j| j["status"] == expect_status)
            .cloned();
        if let Some(job) = hit {
            return job;
        }
        if std::time::Instant::now() > deadline {
            panic!("job never reached {expect_status}: {jobs}");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

use std::sync::atomic::{AtomicBool, Ordering};

struct FarEnd {
    addr: SocketAddr,
    wire: Arc<std::sync::Mutex<Vec<Value>>>,
    gate_query: Arc<AtomicBool>,
    gate_token: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    task: tokio::task::JoinHandle<()>,
}
impl FarEnd {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let wire = Arc::new(std::sync::Mutex::new(Vec::new()));
        let gate_query = Arc::new(AtomicBool::new(false));
        let gate_token = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let (w, g, t, e, r) = (
            wire.clone(),
            gate_query.clone(),
            gate_token.clone(),
            entered.clone(),
            release.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (w, g, t, e, r) = (w.clone(), g.clone(), t.clone(), e.clone(), r.clone());
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let (start, length, path) = loop {
                        let mut buf = [0_u8; 65536];
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if let Some(head) = raw.windows(4).position(|x| x == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&raw[..head]);
                            let len = headers
                                .lines()
                                .find_map(|l| {
                                    l.split_once(':')
                                        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                        .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            let path = headers.split_whitespace().nth(1).unwrap().to_string();
                            break (head + 4, len, path);
                        }
                    };
                    while raw.len() < start + length {
                        let mut buf = [0_u8; 65536];
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                    }
                    if path == "/token" {
                        // 原始凭证表单仅存本任务本地目录，安全证据只记录摘要。
                        let body_bytes = &raw[start..start + length];
                        let digest = lingxi_kernel::knowledge::sha256_hex(body_bytes);
                        std::fs::write(
                            PathBuf::from(std::env::var("TMPDIR").unwrap())
                                .join(format!("credential-wire-{digest}.local-only")),
                            body_bytes,
                        )
                        .unwrap();
                        w.lock()
                            .unwrap()
                            .push(json!({"path":path,"credentialFormSHA256":digest}));
                        if t.load(Ordering::SeqCst) {
                            e.add_permits(1);
                            r.acquire().await.unwrap().forget();
                        }
                        let payload=json!({"access_token":"W11_SYNTHETIC_REFRESHED","refresh_token":"W11_SYNTHETIC_REFRESH","expires_in":3600}).to_string();
                        let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.shutdown().await;
                        return;
                    }
                    let body: Value = serde_json::from_slice(&raw[start..start + length]).unwrap();
                    w.lock().unwrap().push(json!({"path":path,"body":body,"rawBody":String::from_utf8_lossy(&raw[start..start+length])}));
                    let (content, payload) = if path.ends_with("embeddings") {
                        if body["input"][0] == "期限取消证据" && g.load(Ordering::SeqCst) {
                            e.add_permits(1);
                            r.acquire().await.unwrap().forget();
                        }
                        let inputs = body["input"].as_array().unwrap();
                        ("application/json",json!({"data":inputs.iter().enumerate().map(|(i,_)|json!({"index":i,"embedding":[1.0,0.0]})).collect::<Vec<_>>(),"usage":{"prompt_tokens":inputs.len()*5,"total_tokens":inputs.len()*5}}).to_string())
                    } else {
                        let tool_count = body["messages"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|m| m["role"] == "tool")
                            .count();
                        let frames = if tool_count >= 2 {
                            vec![
                                json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"已读取合成证据"},"finish_reason":null}]}),
                                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                            ]
                        } else {
                            // 本轮句柄只能来自真实提交注入的提示，不再手工创建旧UI范围。
                            let system = body["messages"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter(|m| m["role"] == "system")
                                .filter_map(|m| m["content"].as_str())
                                .collect::<Vec<_>>()
                                .join("\n");
                            let tail = system
                                .split_once("本轮知识资料范围：")
                                .expect("真实提交必须提供本轮知识选择")
                                .1;
                            let selection = serde_json::Deserializer::from_str(tail)
                                .into_iter::<Value>()
                                .next()
                                .unwrap()
                                .unwrap();
                            let (name, args) = if tool_count == 0 {
                                ("mcp_describe_tool", json!({"name":"knowledge_search"}))
                            } else {
                                (
                                    "mcp_call",
                                    json!({"tool":"knowledge_search","arguments":{"scopeId":selection["scopeId"],"query":"期限取消证据","channel":"hybrid","limit":8}}),
                                )
                            };
                            assert!(
                                body["tools"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .any(|tool| tool["function"]["name"] == name),
                                "实际模型声明必须含当前目录入口"
                            );
                            vec![
                                json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":format!("external-call-{tool_count}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]}),
                                json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
                            ]
                        };
                        let mut text = String::new();
                        for f in frames {
                            text.push_str(&format!("data: {f}\n\n"));
                        }
                        text.push_str(&format!(
                            "data: {}\n\ndata: [DONE]\n\n",
                            json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2}})
                        ));
                        ("text/event-stream", text)
                    };
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: {content}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self {
            addr,
            wire,
            gate_query,
            gate_token,
            entered,
            release,
            task,
        }
    }
    async fn parked(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    fn capture(&self) -> Vec<Value> {
        self.wire.lock().unwrap().clone()
    }
    fn stop(&self) {
        self.release.add_permits(16);
        self.task.abort();
    }
}

async fn boot_owned(home: &Path, stub: &FarEnd, budget: Duration) -> ServiceState {
    boot_owned_auth(home, stub, budget, false).await
}
async fn boot_owned_auth(
    home: &Path,
    stub: &FarEnd,
    budget: Duration,
    oauth: bool,
) -> ServiceState {
    let layout = prepare_layout(home).unwrap();
    let endpoint = format!("http://{}/v1", stub.addr);
    let mut config = json!({"home":home,"workspace":home,"providers":{"main":{"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"W11_SYNTHETIC_ONLY"}},"embed":{"protocol":"openai-embeddings","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"W11_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"main","model":"chat-owned","capabilities":{"tools":true}},"embedding":{"provider":"embed","model":"embed-owned"}}});
    if oauth {
        config["providers"]["embed"]["auth"] = json!({"kind":"oauth","flow":"deviceCode","clientId":"W11-synthetic-client","tokenEndpoint":format!("http://{}/token",stub.addr),"deviceAuthorizationEndpoint":format!("http://{}/token/device",stub.addr)});
        std::fs::write(layout.runtime_dir.join("credentials.json"),json!({"version":1,"providers":{"embed":{"tokens":{"accessToken":"W11_SYNTHETIC_EXPIRED","refreshToken":"W11_SYNTHETIC_REFRESH","expiresAtUnixMs":1}}}}).to_string()).unwrap();
    }
    let config_path = home.join("service.json");
    std::fs::write(&config_path, config.to_string()).unwrap();
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .unwrap()
            .unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let mut limits = lingxi_service::QuotaLimits::default();
    limits.model.global = 1;
    let deps = ServiceDeps {
        model_gateway: Some(Arc::new(
            lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
        )),
        model_plane_source: Some(source),
        credential_service: Some(credentials),
        workspace_root: Some(home.to_path_buf()),
        quota_limits: limits,
        model_call_tuning: lingxi_service::runs::ModelCallTuning {
            total_budget: budget,
            ..Default::default()
        },
        ..ServiceDeps::default()
    };
    ServiceState::bootstrap_with_deps(config_for(home), &layout, deps)
        .await
        .unwrap()
}

fn rows(home: &Path, sql: &str) -> Value {
    let db = rusqlite::Connection::open_with_flags(
        home.join("lingxi-service/data/runs.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut statement = db.prepare(sql).unwrap();
    let keys = statement
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    json!(statement
        .query_map([], |row| {
            let mut v = serde_json::Map::new();
            for (i, k) in keys.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(n) => json!(n),
                    rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                    rusqlite::types::ValueRef::Blob(_) => json!("LOCAL_ONLY_BYTES"),
                };
                v.insert(k.clone(), value);
            }
            Ok(Value::Object(v))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>())
}
fn receipt(tag: &str, value: Value) {
    let root = PathBuf::from(std::env::var("W11_EVIDENCE").unwrap());
    std::fs::write(
        root.join(format!("{tag}.json")),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}
async fn scoped_source(server: &ApiClient) -> Value {
    let (c, n) = server
        .request(
            "POST",
            "/lingxi/v1/knowledge/notebooks",
            Some(json!({"name":"W11合成资料"})),
        )
        .await;
    assert_eq!(c, 201, "{n}");
    let n = n["notebook"]["id"].as_str().unwrap();
    let(c,s)=server.request("POST",&format!("/lingxi/v1/knowledge/notebooks/{n}/sources"),Some(json!({"kind":"pasted_text","text":"# 资料\n\n期限取消证据：作用域内只有合成句子。","displayName":"证据"}))).await;
    assert_eq!(c, 201, "{s}");
    let sid = s["source"]["id"].as_str().unwrap();
    wait_until_job(server, n, sid, "done").await;
    json!({"notebookIds":[n],"sourceIds":[sid]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c01_c04_actual_driver_gateway_embedding_one_permit_and_replay() {
    let home = unique_dir("driver");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_secs(10)).await;
    let recovery = state.operations().unwrap().clone();
    let server = serve(state).await;
    let knowledge = scoped_source(&server).await;
    let (c, response) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"请用知识工具查期限取消证据","requestId":"W11-exact-one","knowledge":knowledge.clone()})),
        )
        .await;
    let before = rows(
        &home,
        "SELECT * FROM model_call_usage ORDER BY started_at_unix_ms",
    );
    let journal=rows(&home,"SELECT journal_id,session_id,run_id,attempt,owner_kind,owner_subject,target,phase,receipt_outcome FROM invocation_journal");
    receipt(
        "c01-c04-before-assert",
        json!({"home":home,"status":c,"response":response,"wire":stub.capture(),"usage":before,"journal":journal}),
    );
    assert_eq!(c, 200, "{response}");
    let tool = journal
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["target"].as_str().unwrap().ends_with("knowledge_search"))
        .expect("真实工具journal");
    let embed = before
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["purpose"] == "embedding" && r["run_id"].is_string())
        .collect::<Vec<_>>();
    assert_eq!(embed.len(), 1, "{before}");
    let e = embed[0];
    assert_eq!(e["run_id"], tool["run_id"]);
    assert_eq!(e["session_id"], tool["session_id"]);
    assert_eq!(e["attempt"], tool["attempt"]);
    assert_eq!(e["parent_tool_call_id"], tool["journal_id"]);
    assert_eq!(
        e["cause_ref"],
        format!("tool:{}", tool["journal_id"].as_str().unwrap())
    );
    assert_eq!(e["outcome"], "succeeded");
    assert_eq!(e["transport_attempts"], 1);
    recovery.recover_embedding_usage().await.unwrap();
    recovery.recover_embedding_usage().await.unwrap();
    assert_eq!(
        before,
        rows(
            &home,
            "SELECT * FROM model_call_usage ORDER BY started_at_unix_ms"
        )
    );
    let count = stub.capture().len();
    let (c, _) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"请用知识工具查期限取消证据","requestId":"W11-exact-one","knowledge":knowledge.clone()})),
        )
        .await;
    assert_eq!(c, 200);
    assert_eq!(stub.capture().len(), count);
    let (c, r) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"同族下一合法任务","requestId":"W11-next"})),
        )
        .await;
    assert_eq!(c, 200, "{r}");
    receipt(
        "c01-c04-final",
        json!({"home":home,"wire":stub.capture(),"usage":rows(&home,"SELECT * FROM model_call_usage ORDER BY started_at_unix_ms"),"journal":rows(&home,"SELECT journal_id,session_id,run_id,attempt,target,phase,receipt_outcome FROM invocation_journal")}),
    );
    server.stop().await;
    stub.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c03_actual_parent_cancel_during_embedding_network_settles_once() {
    let home = unique_dir("cancel");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_secs(10)).await;
    let runs = state.runs().clone();
    let server = serve(state).await;
    let knowledge = scoped_source(&server).await;
    stub.gate_query.store(true, Ordering::SeqCst);
    let client = ApiClient {
        addr: server.addr,
        token: server.token.clone(),
    };
    let pending = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/sess_local_alpha/execute",
                Some(json!({"input":"取消网络中的知识工具","requestId":"W11-cancel","knowledge":knowledge.clone()})),
            )
            .await
    });
    stub.parked().await;
    let live = rows(&home, "SELECT run_id FROM runs ORDER BY rowid DESC LIMIT 1");
    let run = live[0]["run_id"].as_str().unwrap();
    let start = std::time::Instant::now();
    runs.cancel_run(run, "W11真实父取消");
    let response = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .unwrap()
        .unwrap();
    let usage = rows(
        &home,
        "SELECT * FROM model_call_usage WHERE purpose='embedding' ORDER BY started_at_unix_ms",
    );
    receipt(
        "c03-network-cancel",
        json!({"home":home,"elapsedMs":start.elapsed().as_millis(),"response":response,"wire":stub.capture(),"usage":usage,"runs":rows(&home,"SELECT run_id,attempt_count,status FROM runs")}),
    );
    let entries = usage
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["run_id"] == run)
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "{usage}");
    assert_eq!(entries[0]["outcome"], "cancelled");
    assert_eq!(entries[0]["usage_state"], "unknown");
    assert_eq!(entries[0]["transport_attempts"], 1);
    let count = stub.capture().len();
    stub.gate_query.store(false, Ordering::SeqCst);
    stub.release.add_permits(1);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(stub.capture().len(), count);
    let (c, next) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"取消后下一任务","requestId":"W11-after-cancel"})),
        )
        .await;
    assert_eq!(c, 200, "{next}");
    server.stop().await;
    stub.stop();
}

fn host_runtime(cause: &str, budget_ms: u64) -> lingxi_service::operations::EmbeddingCallRuntime {
    lingxi_service::operations::EmbeddingCallRuntime {
        ownership: lingxi_service::operations::OperationOwnership {
            principal_kind: "local_user".into(),
            principal_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
            agent_id: None,
            session_id: None,
            run_id: None,
            attempt: Some("host-attempt-1".into()),
            parent_tool_call_id: None,
            cause_ref: cause.into(),
            deadline_unix_ms: lingxi_adapters::models::dispatch::unix_ms_now() + budget_ms,
            budget_ms,
        },
        cancel: lingxi_service::cancel::CancelScope::background_root(cause),
        request_key: "host-exact-request".into(),
        manual_retry_authorized: false,
    }
}
fn host_request(text: &str) -> lingxi_adapters::models::operations::embedding::EmbeddingRequest {
    lingxi_adapters::models::operations::embedding::EmbeddingRequest {
        inputs: vec![text.into()],
        dimensions: None,
        context_window: None,
        input_type: lingxi_adapters::models::operations::embedding::EmbeddingInputType::Query,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c03_credential_and_quota_wait_cancel_and_deadline_are_real_and_not_sent() {
    for phase in ["credential", "quota"] {
        for cancelled in [false, true] {
            let home = unique_dir(&format!("{phase}-{cancelled}"));
            let stub = FarEnd::start().await;
            let state =
                boot_owned_auth(&home, &stub, Duration::from_secs(10), phase == "credential").await;
            let ops = state.operations().unwrap().clone();
            let quotas = state.runs().quotas_shared();
            let permit = if phase == "quota" {
                Some(
                    quotas
                        .acquire(
                            lingxi_service::QuotaResource::Model,
                            "blocking-host",
                            "blocking-root",
                        )
                        .await
                        .unwrap(),
                )
            } else {
                None
            };
            stub.gate_token.store(true, Ordering::SeqCst);
            let runtime = host_runtime(
                &format!("host-{phase}-{cancelled}"),
                if cancelled { 3000 } else { 150 },
            );
            let child_runtime = runtime.clone();
            let request = host_request("真实等待阶段");
            let started = std::time::Instant::now();
            let pending = tokio::spawn(async move {
                ops.embed_owned_observed(request, &child_runtime, None)
                    .await
            });
            if phase == "credential" {
                stub.parked().await;
            } else {
                tokio::time::sleep(Duration::from_millis(35)).await;
            }
            if cancelled {
                runtime.cancel.cancel("可信宿主明确取消");
            }
            let error = tokio::time::timeout(Duration::from_secs(2), pending)
                .await
                .unwrap()
                .unwrap()
                .err()
                .expect("等待必须响亮退出");
            let elapsed = started.elapsed().as_millis();
            let usage = rows(
                &home,
                "SELECT * FROM model_call_usage WHERE purpose='embedding'",
            );
            receipt(
                &format!("c03-{phase}-{cancelled}"),
                json!({"home":home,"elapsedMs":elapsed,"errorCode":format!("{:?}",error.code),"wire":stub.capture(),"usage":usage,"ownership":runtime.ownership}),
            );
            assert_eq!(
                error.code,
                if cancelled {
                    lingxi_protocol::ErrorCode::Cancelled
                } else {
                    lingxi_protocol::ErrorCode::BudgetExceeded
                }
            );
            assert!(elapsed < 1000);
            if cancelled {
                // 取消发生在任何传输尝试之前（凭证/配额等待中）：没有可能已
                // 付费的字节，费用台账不写 unknown 行——等待真实、响亮退出、
                // 零线路请求即为本次结算（与 W09 配额等待取消同语义，跨套件
                // 统一）。期限耗尽（未取消）仍保留 failed/unknown 行归属。
                assert!(
                    usage.as_array().unwrap().is_empty(),
                    "pre-send cancel must not fabricate a usage row: {usage}"
                );
            } else {
                assert_eq!(usage.as_array().unwrap().len(), 1);
                assert_eq!(usage[0]["transport_attempts"], 0);
                assert_eq!(usage[0]["usage_state"], "unknown");
                assert!(usage[0]["input_tokens"].is_null());
                assert!(usage[0]["session_id"].is_null());
                assert_eq!(usage[0]["cause_ref"], runtime.ownership.cause_ref);
            }
            assert!(!stub
                .capture()
                .iter()
                .any(|w| w["path"].as_str().unwrap().ends_with("embeddings")));
            drop(permit);
            stub.gate_token.store(false, Ordering::SeqCst);
            stub.release.add_permits(1);
            let next = host_runtime(&format!("host-next-{phase}-{cancelled}"), 3000);
            let result = state
                .operations()
                .unwrap()
                .embed_owned_observed(host_request("下一合法任务"), &next, None)
                .await
                .unwrap();
            assert!(result.accounting_error.is_none());
            assert_eq!(
                stub.capture()
                    .iter()
                    .filter(|w| w["path"].as_str().unwrap().ends_with("embeddings"))
                    .count(),
                1
            );
            let server = serve(state).await;
            server.stop().await;
            stub.stop();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c02_paid_receipt_claim_binding_and_future_schema_refusal() {
    let home = unique_dir("receipt-binding");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_secs(10)).await;
    let ops = state.operations().unwrap();
    let runtime = host_runtime("host-paid-operation", 5000);
    let (a, b) = tokio::join!(
        ops.embed_owned_observed(host_request("精确正文"), &runtime, None),
        ops.embed_owned_observed(host_request("精确正文"), &runtime, None)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.model_call_id, b.model_call_id);
    assert_ne!(a.reused_paid_response, b.reused_paid_response);
    assert_eq!(stub.capture().len(), 1);
    let other = host_runtime("another-legitimate-host-operation", 5000);
    let c = ops
        .embed_owned_observed(host_request("精确正文"), &other, None)
        .await
        .unwrap();
    assert_ne!(a.model_call_id, c.model_call_id);
    let d = ops
        .embed_owned_observed(host_request("另一正文"), &runtime, None)
        .await
        .unwrap();
    assert_ne!(a.model_call_id, d.model_call_id);
    assert_eq!(stub.capture().len(), 3);
    let products = home.join("lingxi-service/media-products/embedding-calls-v1");
    let path = std::fs::read_dir(&products)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut future: Value = serde_json::from_slice(&bytes).unwrap();
    future["schema"] = json!(999);
    std::fs::write(&path, future.to_string()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = ops
        .recover_embedding_usage()
        .await
        .expect_err("未知未来凭据版本必须拒绝");
    assert!(error.message.contains("schema unsupported"));
    assert_eq!(before, std::fs::read(&path).unwrap());
    receipt(
        "c02-paid-binding-schema",
        json!({"home":home,"wire":stub.capture(),"usage":rows(&home,"SELECT * FROM model_call_usage WHERE purpose='embedding'"),"futureSchemaError":error.message,"futureBytesUnchanged":true,"sameClaimCallId":a.model_call_id,"otherCauseCallId":c.model_call_id,"otherTextCallId":d.model_call_id}),
    );
    std::fs::write(&path, bytes).unwrap();
    let server = serve(state).await;
    server.stop().await;
    stub.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c03_actual_driver_embedding_network_deadline_is_not_renewed() {
    let home = unique_dir("driver-deadline");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_millis(250)).await;
    let server = serve(state).await;
    let knowledge = scoped_source(&server).await;
    stub.gate_query.store(true, Ordering::SeqCst);
    let client = ApiClient {
        addr: server.addr,
        token: server.token.clone(),
    };
    let started = std::time::Instant::now();
    let pending = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/sess_local_alpha/execute",
                Some(json!({"input":"耗尽真实模型原期限","requestId":"W11-deadline","knowledge":knowledge.clone()})),
            )
            .await
    });
    stub.parked().await;
    let response = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap();
    let usage = rows(
        &home,
        "SELECT * FROM model_call_usage WHERE purpose='embedding' AND run_id IS NOT NULL",
    );
    receipt(
        "c03-driver-deadline",
        json!({"home":home,"elapsedMs":started.elapsed().as_millis(),"response":response,"usage":usage,"wire":stub.capture()}),
    );
    assert_eq!(usage.as_array().unwrap().len(), 1);
    assert_eq!(usage[0]["outcome"], "failed");
    assert_eq!(usage[0]["usage_state"], "unknown");
    assert_eq!(usage[0]["transport_attempts"], 1);
    assert!(started.elapsed() < Duration::from_secs(1));
    let count = stub.capture().len();
    stub.gate_query.store(false, Ordering::SeqCst);
    stub.release.add_permits(1);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(count, stub.capture().len());
    let (c, r) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"期限后下一合法任务","requestId":"W11-after-deadline"})),
        )
        .await;
    assert_eq!(c, 200, "{r}");
    server.stop().await;
    stub.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c02_journal_native_write_failure_is_loud_and_paid_bytes_remain_available() {
    let home = unique_dir("journal-write");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_secs(10)).await;
    let ops = state.operations().unwrap().clone();
    let root = home.join("lingxi-service/media-products/embedding-calls-v1");
    std::fs::write(&root, b"synthetic journal directory obstruction").unwrap();
    let error = ops
        .embed_owned_observed(
            host_request("发送前故障"),
            &host_runtime("native-presend", 3000),
            None,
        )
        .await
        .err()
        .unwrap();
    receipt(
        "c02-native-presend-error",
        json!({"message":error.message,"code":error.code,"wire":stub.capture()}),
    );
    assert!(
        error
            .message
            .contains("embedding journal existing receipt read"),
        "{}",
        error.message
    );
    assert!(stub.capture().is_empty());
    std::fs::remove_file(&root).unwrap();
    stub.gate_query.store(true, Ordering::SeqCst);
    let runtime = host_runtime("native-paid-write-failure", 3000);
    let child = runtime.clone();
    let operation = ops.clone();
    let pending = tokio::spawn(async move {
        operation
            .embed_owned_observed(host_request("期限取消证据"), &child, None)
            .await
    });
    stub.parked().await;
    let path = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .unwrap();
    let before: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let pending_path = path.with_extension(format!(
        "{}.pending",
        lingxi_kernel::knowledge::sha256_hex(before["modelCallId"].as_str().unwrap().as_bytes())
    ));
    std::fs::create_dir(&pending_path).unwrap();
    stub.release.add_permits(1);
    let result = pending.await.unwrap().unwrap();
    assert_eq!(result.served.outcome.vectors, vec![vec![1.0, 0.0]]);
    let accounting = result.accounting_error.expect("已付结果不能吞凭据失败");
    assert!(accounting.message.contains("journal open"));
    assert_eq!(stub.capture().len(), 1);
    assert_eq!(
        before,
        serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap()
    );
    std::fs::remove_dir(&pending_path).unwrap();
    ops.settle_embedding_usage_for_cause(&runtime.ownership.cause_ref)
        .await
        .unwrap();
    let retry = ops
        .embed_owned_observed(host_request("期限取消证据"), &runtime, None)
        .await
        .unwrap();
    assert!(
        retry.reused_paid_response,
        "独立 paid 凭据证明结果已知，必须恢复而不再付费"
    );
    assert_eq!(retry.served.outcome.vectors, vec![vec![1.0, 0.0]]);
    assert_eq!(stub.capture().len(), 1);
    receipt(
        "c02-native-write",
        json!({"home":home,"presendError":error.message,"paidWriteError":accounting.message,"paidVectorsReturned":result.served.outcome.vectors,"reusedPaidResponse":retry.reused_paid_response,"usage":rows(&home,"SELECT * FROM model_call_usage WHERE purpose='embedding'"),"wire":stub.capture(),"nativeIOPhysicallyReturnedBeforeTaskJoin":true}),
    );
    let server = serve(state).await;
    server.stop().await;
    stub.stop();
}

#[cfg(unix)]
fn native_tasks(runs: &lingxi_service::RunSupervisor) -> Value {
    json!(runs.task_supervisor().tasks().iter().filter(|t|t.label.starts_with("embedding-native-io:")).map(|t|json!({"id":t.id,"runId":t.run_id,"label":t.label,"exit":t.exit.as_ref().map(|e|format!("{e:?}"))})).collect::<Vec<_>>())
}
#[cfg(unix)]
fn obstruct_canonical(root: &Path) -> (PathBuf, PathBuf, Value) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .find(|p| {
            let r: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
            r["state"] == "inflight"
        })
        .unwrap();
    let before: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let pending = path.with_extension(format!(
        "{}.pending",
        lingxi_kernel::knowledge::sha256_hex(before["modelCallId"].as_str().unwrap().as_bytes())
    ));
    let cpath = CString::new(pending.as_os_str().as_bytes()).unwrap();
    assert_eq!(
        unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) },
        0,
        "独占合成FIFO"
    );
    (path, pending, before)
}
#[cfg(unix)]
struct FifoCleanup(PathBuf);
#[cfg(unix)]
impl Drop for FifoCleanup {
    fn drop(&mut self) {
        use std::os::unix::fs::OpenOptionsExt;
        // 失败也释放本测试的实际 writer，避免 panic 把阻塞线程遗留给 runtime。
        if let Ok(reader) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&self.0)
        {
            std::thread::sleep(Duration::from_millis(100));
            drop(reader);
        }
    }
}
#[cfg(unix)]
fn release_native_fifo(path: &Path) -> std::fs::File {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .unwrap()
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c03_actual_driver_cancel_native_fifo_keeps_paid_ledger_and_physical_lease() {
    let home = unique_dir("native-driver-fifo");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_secs(20)).await;
    let runs = state.runs().clone();
    let ops = state.operations().unwrap().clone();
    let server = serve(state).await;
    let knowledge = scoped_source(&server).await;
    stub.gate_query.store(true, Ordering::SeqCst);
    let client = ApiClient {
        addr: server.addr,
        token: server.token.clone(),
    };
    let execution = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/sess_local_alpha/execute",
                Some(json!({"input":"真实文件等待中的知识工具","requestId":"W11-native-fifo","knowledge":knowledge.clone()})),
            )
            .await
    });
    stub.parked().await;
    let root = home.join("lingxi-service/media-products/embedding-calls-v1");
    let (path, pending, before) = obstruct_canonical(&root);
    let _fifo_cleanup = FifoCleanup(pending.clone());
    let run = before["ownership"]["run_id"].as_str().unwrap().to_string();
    stub.release.add_permits(1);
    let start = std::time::Instant::now();
    loop {
        let paid = rows(
            &home,
            "SELECT * FROM model_call_usage WHERE purpose='embedding' AND run_id IS NOT NULL",
        );
        if paid.as_array().is_some_and(|a| a.len() == 1)
            && runs.task_supervisor().tasks().iter().any(|t| {
                t.run_id.as_deref() == Some(&run)
                    && t.label.starts_with("embedding-native-io:")
                    && t.exit.is_none()
            })
        {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    let first = std::time::Instant::now();
    runs.cancel_run(&run, "W11真实native父取消");
    let response = tokio::time::timeout(Duration::from_secs(7), execution)
        .await
        .unwrap()
        .unwrap();
    let live = native_tasks(&runs);
    let usage = rows(
        &home,
        "SELECT * FROM model_call_usage WHERE purpose='embedding' AND run_id IS NOT NULL",
    );
    receipt(
        "c03-native-driver-before-release",
        json!({"home":home,"response":response,"elapsedMs":first.elapsed().as_millis(),"tasks":live,"usage":usage,"wire":stub.capture(),"cancelVerdict":format!("{:?}",runs.cancel_registry().recent_verdict(&run))}),
    );
    assert!(matches!(
        runs.cancel_registry().recent_verdict(&run),
        Some(lingxi_service::cancel::CancelPhase::StopUnconfirmed { .. })
    ));
    assert!(live
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["runId"] == run && t["exit"].is_null()));
    assert_eq!(usage[0]["input_tokens"], 5);
    assert_eq!(usage[0]["outcome"], "succeeded");
    assert_eq!(usage[0]["transport_attempts"], 1);
    let error = ops
        .recover_embedding_usage()
        .await
        .expect_err("物理写仍活着，不能冒中断恢复");
    assert!(error.message.contains("physically active"));
    stub.gate_query.store(false, Ordering::SeqCst);
    let (c, next) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"旧native仍活着时的另一合法调用","requestId":"W11-native-next"})),
        )
        .await;
    assert_eq!(c, 200, "{next}");
    let reader = release_native_fifo(&pending);
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while runs.task_supervisor().tasks().iter().any(|t| {
        t.run_id.as_deref() == Some(&run)
            && t.label.starts_with("embedding-native-io:")
            && t.exit.is_none()
    }) {
        assert!(std::time::Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(reader);
    assert_eq!(
        before,
        serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
        "过期物理open返回后不得晚写/rename"
    );
    std::fs::remove_file(&pending).unwrap();
    ops.recover_embedding_usage().await.unwrap();
    assert_eq!(
        usage,
        rows(
            &home,
            &format!("SELECT * FROM model_call_usage WHERE purpose='embedding' AND run_id='{run}'")
        )
    );
    receipt(
        "c03-native-driver-final",
        json!({"home":home,"tasks":native_tasks(&runs),"originalPaidUsage":usage,"allUsage":rows(&home,"SELECT * FROM model_call_usage WHERE purpose='embedding'"),"wire":stub.capture(),"canonicalBytesUnchanged":true,"physicalClosureExited":true}),
    );
    server.stop().await;
    stub.stop();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c03_absolute_deadline_native_fifo_retains_claim_and_known_paid_usage() {
    let home = unique_dir("native-deadline-fifo");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_secs(20)).await;
    let runs = state.runs().clone();
    let ops = state.operations().unwrap().clone();
    stub.gate_query.store(true, Ordering::SeqCst);
    let runtime = host_runtime("native-fifo-deadline", 500);
    let original_deadline = runtime.ownership.deadline_unix_ms;
    let child = runtime.clone();
    let operation = ops.clone();
    let pending_operation = tokio::spawn(async move {
        operation
            .embed_owned_observed(host_request("期限取消证据"), &child, None)
            .await
    });
    stub.parked().await;
    let root = home.join("lingxi-service/media-products/embedding-calls-v1");
    let (path, pending, before) = obstruct_canonical(&root);
    let _fifo_cleanup = FifoCleanup(pending.clone());
    stub.release.add_permits(1);
    let result = tokio::time::timeout(Duration::from_secs(7), pending_operation)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let error = result
        .accounting_error
        .expect("实际IO仍活，必须响亮报告未确认");
    assert!(error.message.contains("stop unconfirmed"));
    assert_eq!(result.served.outcome.vectors, vec![vec![1.0, 0.0]]);
    assert_eq!(runtime.ownership.deadline_unix_ms, original_deadline);
    let live = native_tasks(&runs);
    assert!(live.as_array().unwrap().iter().any(|t| t["exit"].is_null()));
    let usage = rows(
        &home,
        "SELECT * FROM model_call_usage WHERE purpose='embedding'",
    );
    assert_eq!(usage[0]["input_tokens"], 5);
    assert_eq!(usage[0]["transport_attempts"], 1);
    assert!(ops
        .recover_embedding_usage()
        .await
        .expect_err("不得冒认物理停止")
        .message
        .contains("physically active"));
    let same = runtime.clone();
    let refused = ops
        .embed_owned_observed(host_request("期限取消证据"), &same, None)
        .await
        .err()
        .unwrap();
    assert_eq!(refused.code, lingxi_protocol::ErrorCode::BudgetExceeded);
    assert_eq!(stub.capture().len(), 1);
    stub.gate_query.store(false, Ordering::SeqCst);
    let next = ops
        .embed_owned_observed(
            host_request("合法新cause"),
            &host_runtime("native-deadline-next", 1000),
            None,
        )
        .await
        .unwrap();
    assert!(next.accounting_error.is_none());
    assert_eq!(stub.capture().len(), 2);
    let reader = release_native_fifo(&pending);
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while native_tasks(&runs)
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["exit"].is_null())
    {
        assert!(std::time::Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(reader);
    assert_eq!(
        before,
        serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap()
    );
    std::fs::remove_file(&pending).unwrap();
    ops.recover_embedding_usage().await.unwrap();
    receipt(
        "c03-native-absolute-final",
        json!({"home":home,"originalDeadline":original_deadline,"nativeError":error.message,"sameRequestError":refused.message,"usage":rows(&home,"SELECT * FROM model_call_usage WHERE purpose='embedding'"),"wire":stub.capture(),"liveBeforeRelease":live,"tasksAfterRelease":native_tasks(&runs),"physicalClosureExited":true,"canonicalBytesUnchanged":true}),
    );
    let server = serve(state).await;
    server.stop().await;
    stub.stop();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c02_paid_native_waiter_drop_preserves_real_bytes_until_physical_exit_and_recovery() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let home = unique_dir("paid-waiter-drop");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_secs(20)).await;
    let runs = state.runs().clone();
    let ops = state.operations().unwrap().clone();
    stub.gate_query.store(true, Ordering::SeqCst);
    let runtime = host_runtime("paid-physical-waiter-drop", 20000);
    let child = runtime.clone();
    let operation = ops.clone();
    let waiter = tokio::spawn(async move {
        operation
            .embed_owned_observed(host_request("期限取消证据"), &child, None)
            .await
    });
    stub.parked().await;
    let root = home.join("lingxi-service/media-products/embedding-calls-v1");
    let canonical = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .unwrap();
    let before: Value = serde_json::from_slice(&std::fs::read(&canonical).unwrap()).unwrap();
    let call = before["modelCallId"].as_str().unwrap();
    let key = canonical.file_stem().unwrap().to_str().unwrap();
    let paid_path = root.join(format!("{key}-{call}.paid.json"));
    let pending = paid_path.with_extension(format!(
        "{}.pending",
        lingxi_kernel::knowledge::sha256_hex(call.as_bytes())
    ));
    let cpath = CString::new(pending.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    let _cleanup = FifoCleanup(pending.clone());
    stub.release.add_permits(1);
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if native_tasks(&runs)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["exit"].is_null())
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if native_tasks(&runs)
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["exit"].is_null())
            {
                break;
            }
        }
        assert!(std::time::Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    waiter.abort();
    match waiter.await {
        Err(error) => assert!(error.is_cancelled()),
        Ok(_) => panic!("实际等待者必须被取消"),
    }
    let still_live = native_tasks(&runs);
    assert!(still_live
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["exit"].is_null()));
    assert!(ops
        .recover_embedding_usage()
        .await
        .expect_err("物理IO仍活，paid缓存不能先释放claim")
        .message
        .contains("physically active"));
    let reader = release_native_fifo(&pending);
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while native_tasks(&runs)
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["exit"].is_null())
    {
        assert!(std::time::Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(reader);
    std::fs::remove_file(&pending).unwrap();
    assert_eq!(
        before,
        serde_json::from_slice::<Value>(&std::fs::read(&canonical).unwrap()).unwrap()
    );
    ops.recover_embedding_usage().await.unwrap();
    let paid: Value = serde_json::from_slice(&std::fs::read(&paid_path).unwrap()).unwrap();
    assert_eq!(paid["vectors"], json!([[1.0, 0.0]]));
    assert_eq!(paid["usage"]["prompt_tokens"], 5);
    let usage = rows(
        &home,
        "SELECT * FROM model_call_usage WHERE purpose='embedding'",
    );
    assert_eq!(usage.as_array().unwrap().len(), 1);
    assert_eq!(usage[0]["input_tokens"], 5);
    assert_eq!(usage[0]["outcome"], "succeeded");
    let retry = ops
        .embed_owned_observed(host_request("期限取消证据"), &runtime, None)
        .await
        .unwrap();
    assert!(retry.reused_paid_response);
    assert_eq!(stub.capture().len(), 1);
    receipt(
        "c02-paid-native-waiter-drop",
        json!({"home":home,"realWaiterJoinCancelled":true,"physicalTasksBeforeRelease":still_live,"physicalTasksAfterRelease":native_tasks(&runs),"canonicalBytesUnchanged":true,"paidReceipt":paid,"usage":usage,"wire":stub.capture(),"reusedPaidResponse":retry.reused_paid_response,"originalDeadline":runtime.ownership.deadline_unix_ms}),
    );
    let server = serve(state).await;
    server.stop().await;
    stub.stop();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c02_eight_retained_paid_results_refuse_ninth_before_send_and_recover_all() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let home = unique_dir("paid-full-capacity");
    let stub = FarEnd::start().await;
    let state = boot_owned(&home, &stub, Duration::from_secs(60)).await;
    let runs = state.runs().clone();
    let ops = state.operations().unwrap().clone();
    stub.gate_query.store(true, Ordering::SeqCst);
    let root = home.join("lingxi-service/media-products/embedding-calls-v1");
    let mut entries = Vec::new();
    let mut cleanups = Vec::new();
    for index in 0..8 {
        let cause = format!("paid-full-slot-{index}");
        let runtime = host_runtime(&cause, 60000);
        let child = runtime.clone();
        let operation = ops.clone();
        let waiter = tokio::spawn(async move {
            operation
                .embed_owned_observed(host_request("期限取消证据"), &child, None)
                .await
        });
        stub.parked().await;
        let (canonical, original) = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("json"))
            .filter_map(|path| {
                let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                (value["ownership"]["cause_ref"] == cause).then_some((path, value))
            })
            .next()
            .unwrap();
        let call = original["modelCallId"].as_str().unwrap();
        let paid_path = root.join(format!(
            "{}-{call}.paid.json",
            canonical.file_stem().unwrap().to_str().unwrap()
        ));
        let pending = paid_path.with_extension(format!(
            "{}.pending",
            lingxi_kernel::knowledge::sha256_hex(call.as_bytes())
        ));
        let cpath = CString::new(pending.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        cleanups.push(FifoCleanup(pending.clone()));
        stub.release.add_permits(1);
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while native_tasks(&runs)
            .as_array()
            .unwrap()
            .iter()
            .filter(|task| task["exit"].is_null())
            .count()
            < index + 1
        {
            assert!(std::time::Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // 等到实际paid open仍未返回，避免把短暂的其它文件读取误当paid保留屏障。
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            native_tasks(&runs)
                .as_array()
                .unwrap()
                .iter()
                .filter(|task| task["exit"].is_null())
                .count(),
            index + 1
        );
        waiter.abort();
        match waiter.await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(_) => panic!("paid waiter必须实际丢弃"),
        };
        entries.push((runtime, canonical, original, pending, paid_path));
    }
    assert_eq!(stub.capture().len(), 8);
    let rejected = ops
        .embed_owned_observed(
            host_request("期限取消证据"),
            &host_runtime("paid-full-ninth", 60000),
            None,
        )
        .await
        .err()
        .unwrap();
    assert!(rejected.message.contains("capacity full"));
    assert_eq!(
        stub.capture().len(),
        8,
        "第九次发送前拒绝，不能淘汰已付字节"
    );
    let before_release = native_tasks(&runs);
    assert_eq!(
        before_release
            .as_array()
            .unwrap()
            .iter()
            .filter(|task| task["exit"].is_null())
            .count(),
        8
    );
    let ninth_usage = rows(
        &home,
        "SELECT * FROM model_call_usage WHERE purpose='embedding' AND cause_ref='paid-full-ninth'",
    );
    assert_eq!(ninth_usage.as_array().unwrap().len(), 1);
    assert_eq!(ninth_usage[0]["transport_attempts"], 0);
    assert!(ninth_usage[0]["input_tokens"].is_null());
    let readers = entries
        .iter()
        .map(|entry| release_native_fifo(&entry.3))
        .collect::<Vec<_>>();
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while native_tasks(&runs)
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["exit"].is_null())
    {
        assert!(std::time::Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(readers);
    for (_, canonical, original, pending, paid_path) in &entries {
        assert_eq!(
            original,
            &serde_json::from_slice::<Value>(&std::fs::read(canonical).unwrap()).unwrap()
        );
        assert!(!paid_path.exists(), "FIFO不能冒称paid已持久化");
        std::fs::remove_file(pending).unwrap();
    }
    ops.recover_embedding_usage().await.unwrap();
    for (runtime, _, _, _, paid_path) in &entries {
        let paid: Value = serde_json::from_slice(&std::fs::read(paid_path).unwrap()).unwrap();
        assert_eq!(paid["vectors"], json!([[1.0, 0.0]]));
        let retry = ops
            .embed_owned_observed(host_request("期限取消证据"), runtime, None)
            .await
            .unwrap();
        assert!(retry.reused_paid_response);
    }
    assert_eq!(stub.capture().len(), 8, "八个原结果恢复均不重付");
    stub.gate_query.store(false, Ordering::SeqCst);
    let next = ops
        .embed_owned_observed(
            host_request("合法新cause"),
            &host_runtime("paid-capacity-after-recovery", 1000),
            None,
        )
        .await
        .unwrap();
    assert!(next.accounting_error.is_none());
    assert_eq!(stub.capture().len(), 9);
    let usage = rows(
        &home,
        "SELECT * FROM model_call_usage WHERE purpose='embedding'",
    );
    assert_eq!(usage.as_array().unwrap().len(), 10);
    assert_eq!(
        usage
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["outcome"] == "succeeded" && row["input_tokens"] == 5)
            .count(),
        9
    );
    receipt(
        "c02-paid-capacity-final",
        json!({"home":home,"ninthError":rejected.message,"ninthUsage":ninth_usage,"physicalBeforeRelease":before_release,"physicalAfterRelease":native_tasks(&runs),"usage":usage,"wire":stub.capture(),"allEightRecoveredWithoutNewHTTP":true}),
    );
    drop(cleanups);
    let server = serve(state).await;
    server.stop().await;
    stub.stop();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c03_retained_paid_recovery_native_wait_keeps_original_cancel_and_deadline() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    for deadline_case in [false, true] {
        let home = unique_dir(if deadline_case {
            "recovery-native-deadline"
        } else {
            "recovery-native-cancel"
        });
        let stub = FarEnd::start().await;
        let state = boot_owned(&home, &stub, Duration::from_secs(10)).await;
        let runs = state.runs().clone();
        let ops = state.operations().unwrap().clone();
        stub.gate_query.store(true, Ordering::SeqCst);
        let runtime = host_runtime(
            if deadline_case {
                "recovery-fifo-deadline"
            } else {
                "recovery-fifo-cancel"
            },
            if deadline_case { 500 } else { 10000 },
        );
        let original_deadline = runtime.ownership.deadline_unix_ms;
        let child = runtime.clone();
        let operation = ops.clone();
        let paid_call = tokio::spawn(async move {
            operation
                .embed_owned_observed(host_request("期限取消证据"), &child, None)
                .await
        });
        stub.parked().await;
        let root = home.join("lingxi-service/media-products/embedding-calls-v1");
        let canonical = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().and_then(|v| v.to_str()) == Some("json"))
            .unwrap();
        let original: Value = serde_json::from_slice(&std::fs::read(&canonical).unwrap()).unwrap();
        let call = original["modelCallId"].as_str().unwrap();
        let paid_path = root.join(format!(
            "{}-{call}.paid.json",
            canonical.file_stem().unwrap().to_str().unwrap()
        ));
        let pending = paid_path.with_extension(format!(
            "{}.pending",
            lingxi_kernel::knowledge::sha256_hex(call.as_bytes())
        ));
        std::fs::create_dir(&pending).unwrap();
        stub.release.add_permits(1);
        let paid = paid_call.await.unwrap().unwrap();
        assert!(paid.accounting_error.is_some());
        assert_eq!(paid.served.outcome.vectors, vec![vec![1.0, 0.0]]);
        let usage = rows(
            &home,
            "SELECT * FROM model_call_usage WHERE purpose='embedding'",
        );
        assert_eq!(usage[0]["input_tokens"], 5);
        std::fs::remove_dir(&pending).unwrap();
        let cpath = CString::new(pending.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let _cleanup = FifoCleanup(pending.clone());
        let child = runtime.clone();
        let operation = ops.clone();
        let recovery =
            tokio::spawn(async move { operation.settle_embedding_usage_for_runtime(&child).await });
        let until = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if native_tasks(&runs)
                .as_array()
                .unwrap()
                .iter()
                .any(|task| task["exit"].is_null())
            {
                tokio::time::sleep(Duration::from_millis(30)).await;
                if native_tasks(&runs)
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|task| task["exit"].is_null())
                {
                    break;
                }
            }
            assert!(std::time::Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if !deadline_case {
            runtime.cancel.cancel("W11真实补账父取消");
        }
        let error = tokio::time::timeout(Duration::from_secs(7), recovery)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(
            error.message.contains("stop unconfirmed"),
            "{}",
            error.message
        );
        assert_eq!(runtime.ownership.deadline_unix_ms, original_deadline);
        let live = native_tasks(&runs);
        assert!(live
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["exit"].is_null()));
        assert!(ops
            .recover_embedding_usage()
            .await
            .unwrap_err()
            .message
            .contains("physically active"));
        assert_eq!(stub.capture().len(), 1);
        let reader = release_native_fifo(&pending);
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while native_tasks(&runs)
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["exit"].is_null())
        {
            assert!(std::time::Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(reader);
        std::fs::remove_file(&pending).unwrap();
        ops.recover_embedding_usage().await.unwrap();
        assert_eq!(
            usage,
            rows(
                &home,
                "SELECT * FROM model_call_usage WHERE purpose='embedding'"
            )
        );
        let persisted: Value = serde_json::from_slice(&std::fs::read(&paid_path).unwrap()).unwrap();
        assert_eq!(persisted["vectors"], json!([[1.0, 0.0]]));
        assert_eq!(stub.capture().len(), 1);
        receipt(
            &format!("c03-native-recovery-{deadline_case}"),
            json!({"home":home,"originalDeadline":original_deadline,"error":error.message,"liveBeforeRelease":live,"tasksAfterRelease":native_tasks(&runs),"usage":usage,"wire":stub.capture(),"paidPersistedAfterPhysicalExit":true}),
        );
        let server = serve(state).await;
        server.stop().await;
        stub.stop();
    }
}
