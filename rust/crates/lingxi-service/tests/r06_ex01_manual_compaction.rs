//! EX-01：真实 HTTP、ModelGateway、driver 与 SQLite；仅远端供应商为 loopback。
use lingxi_kernel::ports::StoragePort;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    wire: Arc<Mutex<Vec<Value>>>,
    fail: Arc<AtomicBool>,
    bad_once: Arc<AtomicBool>,
    gate: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    task: tokio::task::JoinHandle<()>,
}
fn summary() -> &'static str {
    "## Goal\nEX01-SUMMARY\n## Constraints & Preferences\n保留结果\n## Progress\n### Done\n旧工作\n### In Progress\n继续\n### Blocked\n无\n## Key Decisions\n真实来源\n## Next Steps\n下一轮\n## Critical Context\nEX01-SUMMARY"
}
impl Remote {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let wire = Arc::new(Mutex::new(Vec::new()));
        let fail = Arc::new(AtomicBool::new(false));
        let bad_once = Arc::new(AtomicBool::new(false));
        let gate = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let number = Arc::new(AtomicUsize::new(0));
        let (w, f, g, e, r, b) = (
            wire.clone(),
            fail.clone(),
            gate.clone(),
            entered.clone(),
            release.clone(),
            bad_once.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (w, f, g, e, r, n, b) = (
                    w.clone(),
                    f.clone(),
                    g.clone(),
                    e.clone(),
                    r.clone(),
                    number.clone(),
                    b.clone(),
                );
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let (head, len) = loop {
                        let mut buf = [0u8; 65536];
                        let count = socket.read(&mut buf).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..count]);
                        if let Some(pos) = raw.windows(4).position(|v| v == b"\r\n\r\n") {
                            let text = String::from_utf8_lossy(&raw[..pos]);
                            let len = text
                                .lines()
                                .find_map(|line| {
                                    line.split_once(':')
                                        .filter(|(key, _)| {
                                            key.eq_ignore_ascii_case("content-length")
                                        })
                                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            break (pos + 4, len);
                        }
                    };
                    while raw.len() < head + len {
                        let mut buf = [0u8; 65536];
                        let count = socket.read(&mut buf).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..count]);
                    }
                    let body: Value = serde_json::from_slice(&raw[head..head + len]).unwrap();
                    w.lock().unwrap().push(body.clone());
                    let is_summary = body["model"] == "summary";
                    if is_summary && g.load(Ordering::SeqCst) {
                        e.add_permits(1);
                        r.acquire().await.unwrap().forget();
                    }
                    if is_summary && f.load(Ordering::SeqCst) {
                        let payload =
                            json!({"error":{"message":"EX01_SYNTHETIC_PROVIDER_FAILURE"}})
                                .to_string();
                        let response=format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                        let _ = socket.write_all(response.as_bytes()).await;
                        return;
                    }
                    let source_text = body["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .rfind(|m| m["role"] == "user")
                        .map(|m| m["content"].to_string())
                        .unwrap_or_default();
                    if !is_summary
                        && (source_text.contains("EX02-SKILL-PATH=")
                            || source_text.contains("EX02-WRITE-PATH="))
                        && !body["messages"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|m| m["role"] == "tool")
                    {
                        let source = body["messages"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .rfind(|m| m["role"] == "user")
                            .unwrap()["content"]
                            .as_str()
                            .unwrap();
                        let writing = source.contains("EX02-WRITE-PATH=");
                        let path = source
                            .split(if writing {
                                "EX02-WRITE-PATH="
                            } else {
                                "EX02-SKILL-PATH="
                            })
                            .nth(1)
                            .unwrap()
                            .lines()
                            .next()
                            .unwrap();
                        let tools = body["tools"].as_array().unwrap();
                        let wire = |name: &str| {
                            tools
                                .iter()
                                .find_map(|t| {
                                    (t["function"]["name"].as_str() == Some(name))
                                        .then(|| name.to_string())
                                })
                                .unwrap_or_else(|| panic!("真实工具声明缺失 {name}: {tools:?}"))
                        };
                        let calls = json!([{"index":0,"id":"ex02-read","type":"function","function":{"name":wire(if writing {"write"} else {"read"}),"arguments":if writing {json!({"path":path,"content":"MUST-NOT-WRITE"}).to_string()}else{json!({"path":path}).to_string()}}}]);
                        let payload = format!(
                            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                            json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":calls},"finish_reason":null}]}),
                            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":10}})
                        );
                        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                        socket.write_all(response.as_bytes()).await.unwrap();
                        return;
                    }
                    let text = if is_summary {
                        if b.swap(false, Ordering::SeqCst) {
                            "invalid draft without required headings".into()
                        } else {
                            summary().to_string()
                        }
                    } else {
                        let index = n.fetch_add(1, Ordering::SeqCst);
                        if body["messages"].to_string().contains("EX01-SUMMARY") {
                            "EX01-NEXT-ACTUAL-RUN".to_string()
                        } else {
                            format!("OLD-TURN-{index} ").repeat(3500)
                        }
                    };
                    let mut payload = String::new();
                    for frame in [
                        json!({"id":"remote-ex01","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
                        json!({"id":"remote-ex01","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                        json!({"id":"remote-ex01","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":10}}),
                    ] {
                        payload.push_str(&format!("data: {frame}\n\n"));
                    }
                    payload.push_str("data: [DONE]\n\n");
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self {
            addr,
            wire,
            fail,
            bad_once,
            gate,
            entered,
            release,
            task,
        }
    }
    fn captured(&self) -> Vec<Value> {
        self.wire.lock().unwrap().clone()
    }
    async fn parked(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    fn stop(self) {
        self.release.add_permits(20);
        self.task.abort();
    }
}
struct Harness {
    server: TestServer,
    state: ServiceState,
    home: PathBuf,
}
fn fixture(home: &Path) {
    let agent = home.join("agents/lingxi");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("config.yaml"),"locale: zh-CN\nagent:\n  name: EX01-OLD-NAME\n  yuan: lingxi\nmemory:\n  enabled: false\ncompaction:\n  enabled: false\nunknown_future:\n  keep: EX01-UNKNOWN-PRESERVED\n").unwrap();
}
async fn boot(home: PathBuf, remote: &Remote, summary_window: u64) -> Harness {
    let layout = prepare_layout(&home).unwrap();
    let config = json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":format!("http://{}/v1",remote.addr),"auth":{"kind":"apiKey","apiKey":"EX01_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"summarize":{"provider":"remote","model":"summary","capabilities":{"tools":true},"compat":{"contextWindow":summary_window}}}});
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::from_value(config).unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let product = home.join("product");
    std::fs::create_dir_all(product.join("yuan")).unwrap();
    let template = product.join("yuan/lingxi.md");
    if !template.exists() {
        std::fs::write(&template, "你叫{{agentName}}，与用户一起完成任务。").unwrap();
    }
    let compiler = Arc::new(
        lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
            lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                &home,
                product,
                lingxi_service::context_compiler::HostFacts {
                    platform: std::env::consts::OS.into(),
                    os_type: std::env::consts::OS.into(),
                    os_release: String::from_utf8(
                        std::process::Command::new("uname")
                            .arg("-r")
                            .output()
                            .unwrap()
                            .stdout,
                    )
                    .unwrap()
                    .trim()
                    .into(),
                    shell_label: "zsh".into(),
                    workspace: home.to_string_lossy().into_owned(),
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
            workspace_root: Some(home.clone()),
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
    }
}
impl Harness {
    async fn create(&self, id: &str) {
        let (code, value) = self
            .server
            .request(
                "POST",
                "/lingxi/v1/sessions",
                Some(json!({"sessionId":id,"agentId":"lingxi","title":id})),
            )
            .await;
        assert_eq!(code, 201, "{value}");
    }
    async fn execute(&self, id: &str, input: &str) -> String {
        let (code, value) = self
            .server
            .request(
                "POST",
                &format!("/lingxi/v1/sessions/{id}/execute"),
                Some(json!({"input":input})),
            )
            .await;
        assert_eq!(code, 200, "{value}");
        let run = value["runId"].as_str().unwrap().to_string();
        let status = self
            .state
            .storage()
            .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run.clone()])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, "completed", "{value}");
        run
    }
    async fn fill(&self, id: &str) {
        self.create(id).await;
        for index in 0..4 {
            self.execute(id, &format!("OLD-USER-{index}")).await;
        }
    }
    async fn settings(&self, value: Value) {
        let (code, reply) = self
            .server
            .request("PUT", "/lingxi/v1/agents/lingxi/config", Some(value))
            .await;
        assert_eq!(code, 200, "{reply}");
    }
    async fn snapshot(&self, id: &str) -> String {
        self.state.storage().query_one_text("SELECT json_object('messages',(SELECT group_concat(message_id||':'||content_json,'|') FROM (SELECT * FROM messages WHERE session_id=?1 ORDER BY seq)),'head',(SELECT head_message_id FROM session_branch_heads WHERE session_id=?1),'revision',(SELECT revision FROM session_branch_heads WHERE session_id=?1),'runs',(SELECT COUNT(*) FROM runs WHERE session_id=?1))",vec![id.into()]).await.unwrap().unwrap()
    }
    async fn archive(&self, id: &str) -> Value {
        let text=self.state.storage().query_one_text("SELECT content_json FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%' ORDER BY seq DESC LIMIT 1",vec![id.into()]).await.unwrap().unwrap();
        serde_json::from_str(&text).unwrap()
    }
    async fn load(&self, id: &str) -> Vec<lingxi_kernel::model_exchange::ExchangeItem> {
        lingxi_service::model_history::ModelHistory::new(self.state.storage().clone())
            .load(&lingxi_kernel::Principal::LocalUser, "lingxi", id, "")
            .await
            .unwrap()
    }
    async fn stop(self) -> PathBuf {
        self.server.stop().await;
        self.home
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manual_disabled_auto_still_commits_focus_and_survives_next_run_restart_fork() {
    let home = unique_dir("manual-persist");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("manual").await;
    h.settings(json!({"compaction":{"enabled":false,"keepRecentTokens":1,"reserveTokens":16384}}))
        .await;
    let before = h.snapshot("manual").await;
    let (code, result) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/manual/compact",
            Some(json!({"customInstructions":"EX01-FOCUS: 优先保留批准边界"})),
        )
        .await;
    assert_eq!(code, 200, "{result}");
    assert_eq!(result["degraded"], false);
    assert!(result["tokensAfter"].as_u64() < result["tokensBefore"].as_u64());
    assert_ne!(before, h.snapshot("manual").await);
    let archive = h.archive("manual").await;
    assert_eq!(archive["sourceKind"], "manual_compaction");
    assert_eq!(archive["sourceAction"], result["actionId"]);
    assert_eq!(archive["refreshedContext"]["request"]["method"], "compact");
    assert_eq!(
        archive["refreshedContext"]["request"]["customInstructions"],
        "EX01-FOCUS: 优先保留批准边界"
    );
    assert!(archive.get("sourceRun").is_none());
    assert!(!archive["refreshedContext"]["tools"]["declarations"]
        .as_array()
        .unwrap()
        .is_empty());
    let summary_request = remote
        .captured()
        .into_iter()
        .find(|value| value["model"] == "summary")
        .unwrap();
    assert!(summary_request["messages"]
        .to_string()
        .contains("EX01-FOCUS"));
    assert!(!summary_request["tools"].as_array().unwrap().is_empty());
    let usage = h
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
            purpose: Some("auxiliary.summarize".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(usage.len(), 1);
    assert!(usage[0].run_id.is_none());
    assert!(usage[0].attempt.is_none());
    assert_eq!(usage[0].cause_ref.as_deref(), result["actionId"].as_str());
    let projected = h.load("manual").await;
    assert!(
        matches!(&projected[0],lingxi_kernel::model_exchange::ExchangeItem::CompactionSummary{summary,..} if summary.contains("EX01-SUMMARY"))
    );
    let home = h.stop().await;
    let h = boot(home, &remote, 1000000).await;
    assert_eq!(
        format!("{:?}", projected),
        format!("{:?}", h.load("manual").await)
    );
    h.execute("manual", "EX01-RESTART-NEXT").await;
    let next = remote.captured().last().unwrap().clone();
    assert!(next["messages"].to_string().contains("EX01-SUMMARY"));
    assert!(!next["messages"].to_string().contains("OLD-TURN-0 "));
    let boundary = format!(
        "model-history:manual:{}",
        result["actionId"].as_str().unwrap()
    );
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/manual/fork",
            Some(json!({"newSessionId":"manual-fork","boundaryMessageId":boundary})),
        )
        .await;
    assert_eq!(code, 200, "{value}");
    h.state
        .session_admin()
        .archive_session(
            "manual",
            Some(lingxi_adapters::storage::session_admin::ChildMode::DetachChildren),
            h.state.clock().now_unix_ms(),
        )
        .await
        .unwrap();
    h.state
        .session_admin()
        .delete_archived_session("manual")
        .await
        .unwrap();
    h.execute("manual-fork", "EX01-FORK-NEXT").await;
    assert!(remote.captured().last().unwrap()["messages"]
        .to_string()
        .contains("EX01-SUMMARY"));
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_commits_current_material_and_tools_then_real_driver_consumes_them() {
    let home = unique_dir("fresh-persist");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("fresh").await;
    h.settings(json!({"agent":{"name":"EX01-NEW-NAME"},"compaction":{"enabled":false,"keepRecentTokens":1}})).await;
    let (code, result) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/fresh/fresh-compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{result}");
    assert_eq!(result["fresh"], true);
    let archive = h.archive("fresh").await;
    assert_eq!(archive["refreshedContext"]["fresh"], true);
    assert!(archive["refreshedContext"]["render"]
        .as_str()
        .unwrap()
        .contains("EX01-NEW-NAME"));
    let home = h.stop().await;
    let h = boot(home, &remote, 1000000).await;
    h.execute("fresh", "EX01-FRESH-NEXT").await;
    let sent = remote.captured().last().unwrap().clone();
    assert!(sent["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("EX01-NEW-NAME"));
    assert!(sent["messages"].to_string().contains("EX01-SUMMARY"));
    let persisted = archive["refreshedContext"]["tools"]["declarations"]
        .as_array()
        .unwrap();
    let current = sent["tools"].as_array().unwrap();
    for declaration in persisted {
        assert!(current
            .iter()
            .any(|tool| tool["function"]["name"] == declaration["wireName"]));
    }
    assert!(
        std::fs::read_to_string(h.home.join("agents/lingxi/config.yaml"))
            .unwrap()
            .contains("EX01-UNKNOWN-PRESERVED")
    );
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_settings_change_keep_recent_and_auto_reserve_and_disabled_still_refuses_overflow() {
    let home = unique_dir("settings-real");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("keep-default").await;
    h.fill("keep-short").await;
    h.fill("auto").await;
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/keep-default/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{value}");
    let retained_default = h.archive("keep-default").await["payload"]["items"]
        .as_array()
        .unwrap()
        .len();
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/keep-short/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{value}");
    let retained_short = h.archive("keep-short").await["payload"]["items"]
        .as_array()
        .unwrap()
        .len();
    assert!(retained_short < retained_default);
    let summary_count = remote
        .captured()
        .iter()
        .filter(|call| call["model"] == "summary")
        .count();
    h.settings(json!({"compaction":{"enabled":true,"reserveTokens":170000}}))
        .await;
    h.execute("auto", "EX01-AUTO-CONSUMER").await;
    assert_eq!(
        remote
            .captured()
            .iter()
            .filter(|call| call["model"] == "summary")
            .count(),
        summary_count + 1
    );
    assert!(remote.captured().last().unwrap()["messages"]
        .to_string()
        .contains("EX01-SUMMARY"));
    h.settings(json!({"compaction":{"enabled":false,"reserveTokens":200000}}))
        .await;
    h.create("overflow").await;
    let wire = remote.captured().len();
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/overflow/execute",
            Some(json!({"input":"EX01-OVERFLOW"})),
        )
        .await;
    assert_eq!(code, 200, "{value}");
    let status = h
        .state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id=?1",
            vec![value["runId"].as_str().unwrap().into()],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, "failed");
    assert_eq!(remote.captured().len(), wire, "禁自动压缩仍不发送超窗请求");
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_permission_unknown_busy_provider_cancel_cas_and_storage_failure_preserve_history()
{
    let home = unique_dir("refusals");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("refused").await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let original = h.snapshot("refused").await;
    let wire = remote.captured().len();
    for (path, body, status) in [
        ("/lingxi/v1/sessions/ghost/compact", json!({}), 404),
        (
            "/lingxi/v1/sessions/refused/compact",
            json!({"method":"unknown"}),
            400,
        ),
        (
            "/lingxi/v1/sessions/refused/fresh-compact",
            json!({"customInstructions":true}),
            400,
        ),
    ] {
        let (code, value) = h.server.request("POST", path, Some(body)).await;
        assert_eq!(code, status, "{value}");
    }
    let (code, token) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/devices/credentials",
            Some(json!({"userId":"foreign","scopes":["chat"]})),
        )
        .await;
    assert_eq!(code, 201);
    let foreign = ApiClient {
        addr: h.server.addr,
        token: token["secret"].as_str().unwrap().into(),
    };
    let (code, value) = foreign
        .request(
            "POST",
            "/lingxi/v1/sessions/refused/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 403, "{value}");
    let unauth = ApiClient {
        addr: h.server.addr,
        token: String::new(),
    };
    assert_eq!(
        unauth
            .request(
                "POST",
                "/lingxi/v1/sessions/refused/compact",
                Some(json!({}))
            )
            .await
            .0,
        401
    );
    let config = std::fs::read(h.home.join("agents/lingxi/config.yaml")).unwrap();
    let (code, value) = h
        .server
        .request(
            "PUT",
            "/lingxi/v1/agents/lingxi/config",
            Some(json!({"compaction":{"reserveTokens":"bad"},"unknown_new":true})),
        )
        .await;
    assert_eq!(code, 400, "{value}");
    assert_eq!(
        std::fs::read(h.home.join("agents/lingxi/config.yaml")).unwrap(),
        config
    );
    assert_eq!(original, h.snapshot("refused").await);
    assert_eq!(wire, remote.captured().len());
    remote.fail.store(true, Ordering::SeqCst);
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/refused/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 500, "{value}");
    assert_eq!(original, h.snapshot("refused").await);
    remote.fail.store(false, Ordering::SeqCst);
    remote.gate.store(true, Ordering::SeqCst);
    let client = ApiClient {
        addr: h.server.addr,
        token: h.server.token.clone(),
    };
    let first = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/refused/compact",
                Some(json!({})),
            )
            .await
    });
    remote.parked().await;
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/refused/fresh-compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 409, "{value}");
    assert_eq!(original, h.snapshot("refused").await);
    assert_eq!(
        h.server
            .request("POST", "/lingxi/v1/sessions/refused/compact/cancel", None)
            .await
            .0,
        200
    );
    assert_eq!(first.await.unwrap().0, 409);
    assert_eq!(original, h.snapshot("refused").await);
    remote.release.add_permits(1);
    let client = ApiClient {
        addr: h.server.addr,
        token: h.server.token.clone(),
    };
    let cas = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/refused/compact",
                Some(json!({})),
            )
            .await
    });
    remote.parked().await;
    // 真实外部 SQLite 写者只推进 revision、保持同一 head，证明不是仅比较头字符串。
    let writer = rusqlite::Connection::open(h.home.join("lingxi-service/data/runs.db")).unwrap();
    writer
        .execute(
            "UPDATE session_branch_heads SET revision=revision+1 WHERE session_id=?1",
            ["refused"],
        )
        .unwrap();
    drop(writer);
    let concurrent = h.snapshot("refused").await;
    remote.release.add_permits(1);
    let (code, value) = cas.await.unwrap();
    assert_eq!(code, 409, "{value}");
    assert_eq!(concurrent, h.snapshot("refused").await);
    let client = ApiClient {
        addr: h.server.addr,
        token: h.server.token.clone(),
    };
    let changed = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/refused/fresh-compact",
                Some(json!({})),
            )
            .await
    });
    remote.parked().await;
    h.settings(json!({"agent":{"name":"EX01-MATERIAL-CHANGED-DURING-SUMMARY"}}))
        .await;
    remote.release.add_permits(1);
    let (code, value) = changed.await.unwrap();
    assert_eq!(code, 409, "{value}");
    assert_eq!(value["details"]["reason"], "compaction_context_changed");
    assert_eq!(concurrent, h.snapshot("refused").await);
    remote.gate.store(false, Ordering::SeqCst);
    let conn = rusqlite::Connection::open(h.home.join("lingxi-service/data/runs.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER ex01_refuse_manual BEFORE INSERT ON messages WHEN NEW.message_id LIKE 'model-history:manual:%' BEGIN SELECT RAISE(ABORT,'EX01_REAL_STORAGE_FAILURE'); END;").unwrap();
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/refused/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 500, "{value}");
    assert!(
        value.to_string().contains("EX01_REAL_STORAGE_FAILURE"),
        "必须达到实际档案写失败，不接受更早的其它错误: {value}"
    );
    assert_eq!(concurrent, h.snapshot("refused").await);
    conn.execute_batch("DROP TRIGGER ex01_refuse_manual")
        .unwrap();
    drop(conn);
    let original_config = std::fs::read(h.home.join("agents/lingxi/config.yaml")).unwrap();
    std::fs::write(
        h.home.join("agents/lingxi/config.yaml"),
        "compaction:\n  enabled: wrong-type\n",
    )
    .unwrap();
    let hits = remote.captured().len();
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/refused/compact",
            Some(json!({})),
        )
        .await;
    assert_ne!(code, 200, "{value}");
    assert_eq!(concurrent, h.snapshot("refused").await);
    assert_eq!(remote.captured().len(), hits);
    std::fs::write(h.home.join("agents/lingxi/config.yaml"), original_config).unwrap();
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hard_truncation_is_explicit_and_committed_without_fake_summary_request() {
    let home = unique_dir("hard-real");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000).await;
    h.fill("hard").await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let hits = remote.captured().len();
    let (code, value) = h
        .server
        .request("POST", "/lingxi/v1/sessions/hard/compact", Some(json!({})))
        .await;
    assert_eq!(code, 200, "{value}");
    assert_eq!(value["degraded"], true);
    assert_eq!(remote.captured().len(), hits);
    assert!(h.archive("hard").await["payload"]["items"][0]["summary"]
        .as_str()
        .unwrap()
        .contains("硬截断"));
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manual_archive_refuses_unknown_source_and_forged_retained_history_without_read_side_effects(
) {
    let home = unique_dir("archive-contract");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("contract").await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let (code, value) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/contract/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{value}");
    let original = h.archive("contract").await;
    let id = format!(
        "model-history:manual:{}",
        value["actionId"].as_str().unwrap()
    );
    let conn = rusqlite::Connection::open(h.home.join("lingxi-service/data/runs.db")).unwrap();
    for unknown_source in [true, false] {
        let mut bad = original.clone();
        bad.as_object_mut().unwrap().remove("digest");
        if unknown_source {
            bad["sourceKind"] = json!("unknown-action");
        } else {
            bad["payload"]["items"][1]["content"] =
                json!([{"type":"text","text":"EX01-FORGED-HISTORICAL-RESULT"}]);
        }
        bad["digest"] = json!(lingxi_protocol::canon::sha256_hex(
            &lingxi_protocol::canon::canonical_json_bytes(&bad)
        ));
        conn.execute(
            "UPDATE messages SET content_json=?1 WHERE session_id=?2 AND message_id=?3",
            rusqlite::params![bad.to_string(), "contract", id],
        )
        .unwrap();
        let before = h.snapshot("contract").await;
        let wire = remote.captured().len();
        let error = lingxi_service::model_history::ModelHistory::new(h.state.storage().clone())
            .load(
                &lingxi_kernel::Principal::LocalUser,
                "lingxi",
                "contract",
                "",
            )
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains(if unknown_source {
                "manual action identity"
            } else {
                "retained original history"
            }),
            "{error}"
        );
        assert_eq!(before, h.snapshot("contract").await);
        assert_eq!(wire, remote.captured().len());
    }
    conn.execute(
        "UPDATE messages SET content_json=?1 WHERE session_id=?2 AND message_id=?3",
        rusqlite::params![original.to_string(), "contract", id],
    )
    .unwrap();
    drop(conn);
    h.load("contract").await;
    h.stop().await;
    remote.stop();
}

// 不以睡眠推测保存时点：必须先见到真实付费记录，再持有外部SQLite写锁，
// 验证档案尚未存在且生产队列确实等待，才能注入晚期变化并作保全断言。
async fn post_usage_precommit_refuses(kind: &str) {
    let home = unique_dir(&format!("late-{kind}"));
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    remote.gate.store(true, Ordering::SeqCst);
    let mut caught = false;
    for index in 0..12 {
        let id = format!("late-{kind}-{index}");
        h.fill(&id).await;
        let original = h.snapshot(&id).await;
        let receipts = h.state.storage().query_one_text(
            "SELECT json_object('operations',(SELECT COUNT(*) FROM session_operations WHERE session_id=?1),'events',(SELECT COUNT(*) FROM key_events WHERE session_id=?1))", vec![id.clone()]).await.unwrap();
        let client = ApiClient {
            addr: h.server.addr,
            token: h.server.token.clone(),
        };
        let request_id = id.clone();
        let pending = tokio::spawn(async move {
            client
                .request(
                    "POST",
                    &format!("/lingxi/v1/sessions/{request_id}/fresh-compact"),
                    Some(json!({})),
                )
                .await
        });
        remote.parked().await;
        let db = h.home.join("lingxi-service/data/runs.db");
        let watcher_id = id.clone();
        let watcher = tokio::task::spawn_blocking(move || {
            let writer = rusqlite::Connection::open(db).unwrap();
            for _ in 0..100000 {
                let count: i64 = writer.query_row(
                    "SELECT COUNT(*) FROM model_call_usage WHERE session_id=?1 AND purpose='auxiliary.summarize'", [&watcher_id], |r| r.get(0)).unwrap();
                if count > 0 {
                    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
                    let committed: i64 = writer.query_row(
                        "SELECT COUNT(*) FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%'", [&watcher_id], |r| r.get(0)).unwrap();
                    return (writer, committed == 0);
                }
                std::thread::sleep(Duration::from_micros(50));
            }
            panic!("没有观察到摘要付费记录，不能声称到达最终保存窗口")
        });
        remote.release.add_permits(1);
        let (writer, before_commit) = watcher.await.unwrap();
        if !before_commit {
            writer.execute_batch("ROLLBACK").unwrap();
            drop(writer);
            assert_eq!(pending.await.unwrap().0, 200);
            continue;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                h.state
                    .storage()
                    .query_one_text("SELECT 'queue-probe'", vec![])
            )
            .await
            .is_err(),
            "付费已结算、档案不存在、生产写者必须在等实际SQLite写锁"
        );
        match kind {
            "material" => {
                let config = h.home.join("agents/lingxi/config.yaml");
                let old = std::fs::read_to_string(&config).unwrap();
                assert!(old.contains("EX01-OLD-NAME"));
                std::fs::write(config, old.replace("EX01-OLD-NAME", "EX01-LATE-NEW-NAME")).unwrap();
            }
            "tools" => {
                let registry = h.state.tool_gateway().unwrap().registry();
                let declaration =
                    lingxi_kernel::model_exchange::ToolDeclarationSnapshot::from_registry(registry)
                        .unwrap()
                        .declarations[0]
                        .target
                        .clone();
                registry
                    .set_availability(
                        &lingxi_kernel::toolcatalog::ToolTargetId::parse(&declaration),
                        lingxi_kernel::toolcatalog::Availability::Disabled {
                            reason: "EX01实际晚期工具禁用".into(),
                        },
                    )
                    .unwrap();
            }
            "settings" => {
                // 真设置入口、原config.yaml落点、原发布栅栏；不是孤立测试配置。
                h.settings(json!({"compaction":{"reserveTokens":17000}}))
                    .await;
                assert_eq!(
                    h.state
                        .compaction()
                        .unwrap()
                        .settings_for("lingxi")
                        .unwrap()
                        .reserve_tokens,
                    Some(17000)
                );
            }
            "cancel" => assert!(
                h.state.compaction().unwrap().cancel_manual(&id).unwrap(),
                "取消必须赢得尚未完成的真实保存裁决"
            ),
            _ => panic!("unknown probe"),
        }
        writer.execute_batch("ROLLBACK").unwrap();
        drop(writer);
        let (code, result) = pending.await.unwrap();
        eprintln!(
            "EX01_AFTER_USAGE_PRECOMMIT kind={kind} session={id} status={code} result={result}"
        );
        assert_eq!(code, 409, "真实提交等待期间的变化必须拒绝保存: {result}");
        assert_eq!(
            original,
            h.snapshot(&id).await,
            "原消息、有效头、版本和Run数量须逐字保留"
        );
        let count = h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%'", vec![id.clone()]).await.unwrap().unwrap();
        assert_eq!(count, "0", "不能残留手动档案");
        assert_eq!(receipts, h.state.storage().query_one_text("SELECT json_object('operations',(SELECT COUNT(*) FROM session_operations WHERE session_id=?1),'events',(SELECT COUNT(*) FROM key_events WHERE session_id=?1))", vec![id.clone()]).await.unwrap(), "不能残留操作收据/持久成功事件");
        let events = h.state.app_events().since(0);
        let session_events: Vec<_> = events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["payload"]["sessionId"] == id)
            .collect();
        let accepted: Vec<_> = session_events
            .iter()
            .filter(|event| event["type"] == "compaction_accepted")
            .collect();
        assert_eq!(accepted.len(), 1);
        assert!(
            !session_events
                .iter()
                .any(|event| event["type"] == "compaction_succeeded"),
            "拒绝保存不能发布成功"
        );
        let usage = h.state.storage().query_one_text("SELECT json_object('count',COUNT(*),'run',run_id,'attempt',attempt,'cause',cause_ref) FROM model_call_usage WHERE session_id=?1 AND purpose='auxiliary.summarize'", vec![id]).await.unwrap().unwrap();
        let usage: Value = serde_json::from_str(&usage).unwrap();
        assert_eq!(usage["count"], 1, "付费记录必须保留");
        assert!(usage["run"].is_null() && usage["attempt"].is_null());
        assert_eq!(usage["cause"], accepted[0]["payload"]["actionId"]);
        caught = true;
        break;
    }
    assert!(caught, "未抓到实际付费后/提交前窗口，不能声称覆盖");
    h.stop().await;
    remote.stop();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_usage_precommit_material_change_preserves_history_and_usage() {
    post_usage_precommit_refuses("material").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_usage_precommit_tool_change_preserves_history_and_usage() {
    post_usage_precommit_refuses("tools").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_usage_precommit_setting_change_preserves_history_and_usage() {
    post_usage_precommit_refuses("settings").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_usage_precommit_cancel_preserves_history_and_usage() {
    post_usage_precommit_refuses("cancel").await;
}

/// EX02：真实read/context_notes经压缩、下一Run、重启及再次压缩仍恢复，摘要保持普通用户历史。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ex02_actual_skill_and_notes_survive_compaction_restart_next_run_and_repeat() {
    let home = unique_dir("ex02-material");
    fixture(&home);
    let skill = home.join("skills/ex02-skill");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"),"---\nname: ex02-skill\ndescription: 合成技能\n---\nEX02_SKILL_BODY_NONCE: 必须保留真实的详细步骤。\n").unwrap();
    let original_skill = std::fs::read_to_string(skill.join("SKILL.md")).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("{original_skill}{}", "详细步骤中🙂文 ".repeat(2000)),
    )
    .unwrap();
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.settings(json!({"skills":{"enabled":["ex02-skill"]},"compaction":{"enabled":false,"keepRecentTokens":1}})).await;
    h.create("ex02-material").await;
    let (notes_status, notes_result) = h
        .server
        .request(
            "PUT",
            "/lingxi/v1/sessions/ex02-material/context-notes",
            Some(json!({"notes":"EX02_CONTEXT_NOTES_NONCE: 保留当前目标与批准边界"})),
        )
        .await;
    assert_eq!(notes_status, 200, "{notes_result}");
    h.execute("ex02-material",&format!("[Use skill: ex02-skill]\nEX02-SKILL-PATH={}\n当前目标: 完成实际任务，批准未到不得执行写入。",skill.join("SKILL.md").display())).await;
    for i in 0..3 {
        h.execute("ex02-material", &format!("后续工作{i}")).await;
    }
    let current_skill = std::fs::read_to_string(skill.join("SKILL.md"))
        .unwrap()
        .replace(
            "EX02_SKILL_BODY_NONCE",
            "EX02_FRESH_SOURCE_NONCE: current selected skill bytes\nEX02_SKILL_BODY_NONCE",
        );
    std::fs::write(skill.join("SKILL.md"), current_skill).unwrap();
    let raw = remote.captured();
    assert!(raw
        .iter()
        .any(|v| v["messages"].to_string().contains("EX02_SKILL_BODY_NONCE")));
    assert!(h
        .state
        .memory()
        .context_notes()
        .read("ex02-material")
        .unwrap()
        .contains("EX02_CONTEXT_NOTES_NONCE"));
    let (status, result) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/ex02-material/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let after = h.load("ex02-material").await;
    let summary = match &after[0] {
        lingxi_kernel::model_exchange::ExchangeItem::CompactionSummary { summary, .. } => summary,
        _ => panic!("必须真实压缩"),
    };
    assert!(
        summary.contains("EX02_SKILL_BODY_NONCE"),
        "已读技能正文不可悄丢: {summary}"
    );
    assert!(
        summary.contains("EX02_CONTEXT_NOTES_NONCE"),
        "当前会话便签不可悄丢: {summary}"
    );
    assert!(summary.contains("EX02_FRESH_SOURCE_NONCE"));
    let recall = summary
        .split_once("<skill-recall>")
        .unwrap()
        .1
        .split_once("</skill-recall>")
        .unwrap()
        .0;
    let recall: Value = serde_json::from_str(recall).unwrap();
    assert!(
        recall["entries"][0]["content"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= 6000
    );
    assert!(recall["entries"][0]["content"]
        .as_str()
        .unwrap()
        .contains("truncated for compaction"));
    let home = h.stop().await;
    let h = boot(home, &remote, 1000000).await;
    h.execute("ex02-material", "EX02-RESTART-NEXT").await;
    let next = remote.captured().last().unwrap().clone();
    let messages = next["messages"].as_array().unwrap();
    assert!(
        messages
            .iter()
            .any(|v| v["role"] == "user"
                && v["content"].to_string().contains("EX02_SKILL_BODY_NONCE"))
    );
    assert!(!messages.iter().any(
        |v| v["role"] == "system" && v["content"].to_string().contains("EX02_SKILL_BODY_NONCE")
    ));
    std::fs::remove_file(skill.join("SKILL.md")).unwrap();
    remote.bad_once.store(true, Ordering::SeqCst);
    h.execute("ex02-material", &"继续当前任务 ".repeat(2000))
        .await;
    let (status, result) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/ex02-material/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert!(format!("{:?}", h.load("ex02-material").await).contains("EX02_SKILL_BODY_NONCE"));
    assert_eq!(
        remote
            .captured()
            .iter()
            .filter(|v| v["model"] == "summary")
            .count(),
        3,
        "第二次压缩必须真实经过一次格式修复"
    );
    assert!(format!("{:?}", h.load("ex02-material").await)
        .contains("source is not in the current agent catalog"));
    h.create("ex02-neighbor").await;
    h.execute("ex02-neighbor", "隔壁会话没有这些便签和已读材料")
        .await;
    assert!(!remote.captured().last().unwrap()["messages"]
        .to_string()
        .contains("EX02_CONTEXT_NOTES_NONCE"));
    std::fs::write(
        h.home.join("ex02-real-provider-wire.json"),
        serde_json::to_vec_pretty(&remote.captured()).unwrap(),
    )
    .unwrap();
    println!("EX02_RECOVERY_PROOF {}", h.home.display());
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ex02_unreadable_notes_fail_before_model_and_preserve_history() {
    let home = unique_dir("ex02-notes-failure");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    h.fill("notes-failure").await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let path = h.home.join("lingxi-service/context-notes/notes-failure.md");
    std::fs::create_dir_all(&path).unwrap();
    let before = h.snapshot("notes-failure").await;
    let calls = remote.captured().len();
    let (status, result) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/notes-failure/compact",
            Some(json!({})),
        )
        .await;
    assert!(status >= 400, "{status} {result}");
    assert!(result.to_string().contains("recovery"), "{result}");
    assert_eq!(before, h.snapshot("notes-failure").await);
    assert_eq!(remote.captured().len(), calls);
    std::fs::write(
        h.home.join("ex02-notes-failure-proof.json"),
        serde_json::to_vec_pretty(
            &json!({"status":status,"result":result,"historyUnchanged":true,"modelRequests":0}),
        )
        .unwrap(),
    )
    .unwrap();
    println!("EX02_NOTES_FAILURE_PROOF {}", h.home.display());
    h.stop().await;
    remote.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ex02_pending_tool_approval_and_current_goal_cannot_be_compacted_away() {
    let home = unique_dir("ex02-pending-approval");
    fixture(&home);
    let remote = Remote::start().await;
    let h = boot(home, &remote, 1000000).await;
    let(status,value)=h.server.request("POST","/lingxi/v1/sessions",Some(json!({"sessionId":"pending-approval","agentId":"lingxi","title":"等待批准","permissionMode":"ask"}))).await;
    assert_eq!(status, 201, "{value}");
    for n in 0..4 {
        h.execute("pending-approval", &format!("先前工作{n}")).await;
    }
    h.settings(json!({"compaction":{"keepRecentTokens":1}}))
        .await;
    let destination = h.home.join("must-not-write.txt");
    let input = format!(
        "EX02_CURRENT_GOAL_NONCE: 未批准不可写入\nEX02-WRITE-PATH={}",
        destination.display()
    );
    let client = ApiClient {
        addr: h.server.addr,
        token: h.server.token.clone(),
    };
    let task = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/pending-approval/execute",
                Some(json!({"input":input})),
            )
            .await
    });
    let pending = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (status, value) = h
                .server
                .request(
                    "GET",
                    "/lingxi/v1/sessions/pending-approval/approvals",
                    None,
                )
                .await;
            assert_eq!(status, 200, "{value}");
            if !value["approvals"].as_array().unwrap().is_empty() {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("实际写工具应等待批准");
    assert!(!destination.exists());
    let before = h.snapshot("pending-approval").await;
    let (status, result) = h
        .server
        .request(
            "POST",
            "/lingxi/v1/sessions/pending-approval/compact",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 409, "{result}");
    assert_eq!(before, h.snapshot("pending-approval").await);
    let (_, after) = h
        .server
        .request(
            "GET",
            "/lingxi/v1/sessions/pending-approval/approvals",
            None,
        )
        .await;
    assert_eq!(
        pending["approvals"][0]["approvalId"],
        after["approvals"][0]["approvalId"]
    );
    let id = pending["approvals"][0]["approvalId"].as_str().unwrap();
    let (status, value) = h
        .server
        .request(
            "POST",
            &format!("/lingxi/v1/sessions/pending-approval/approvals/{id}"),
            Some(json!({"action":"rejected"})),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .0,
        200
    );
    assert!(!destination.exists());
    assert!(remote.captured().last().unwrap()["messages"]
        .to_string()
        .contains("EX02_CURRENT_GOAL_NONCE"));
    std::fs::write(h.home.join("ex02-pending-proof.json"),serde_json::to_vec_pretty(&json!({"pending":pending,"afterCompactionRefusal":after,"compactionStatus":409,"compactionResult":result,"historyUnchanged":true,"destinationAbsent":true,"wire":remote.captured()})).unwrap()).unwrap();
    println!("EX02_PENDING_APPROVAL_PROOF {}", h.home.display());
    h.stop().await;
    remote.stop();
}
