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
        let gate = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let number = Arc::new(AtomicUsize::new(0));
        let (w, f, g, e, r) = (
            wire.clone(),
            fail.clone(),
            gate.clone(),
            entered.clone(),
            release.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (w, f, g, e, r, n) = (
                    w.clone(),
                    f.clone(),
                    g.clone(),
                    e.clone(),
                    r.clone(),
                    number.clone(),
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
                    let text = if is_summary {
                        summary().to_string()
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

#[tokio::test(flavor="multi_thread", worker_threads=4)]
async fn independent_tool_and_settings_concurrency_rejects_stale_summary() {
    let home = unique_dir("review-concurrent"); fixture(&home);
    let remote = Remote::start().await; let h = boot(home, &remote, 1000000).await;
    h.fill("review-concurrent").await; h.settings(json!({"compaction":{"keepRecentTokens":1}})).await;
    let original = h.snapshot("review-concurrent").await;
    remote.gate.store(true, Ordering::SeqCst);
    for tools_change in [false, true] {
        let client = ApiClient{addr:h.server.addr, token:h.server.token.clone()};
        let pending = tokio::spawn(async move {client.request("POST", "/lingxi/v1/sessions/review-concurrent/compact", Some(json!({}))).await});
        remote.parked().await;
        if tools_change {
            let registry = h.state.tool_gateway().unwrap().registry();
            let declaration = lingxi_kernel::model_exchange::ToolDeclarationSnapshot::from_registry(registry).unwrap().declarations[0].target.clone();
            registry.set_availability(&lingxi_kernel::toolcatalog::ToolTargetId::parse(&declaration), lingxi_kernel::toolcatalog::Availability::Disabled{reason:"independent external user disabled tool".into()}).unwrap();
        } else { h.settings(json!({"compaction":{"reserveTokens":8192}})).await; }
        remote.release.add_permits(1);
        let (code, result) = pending.await.unwrap();
        assert_eq!(code,409,"{result}"); assert_eq!(result["details"]["reason"],"compaction_context_changed");
        assert_eq!(original,h.snapshot("review-concurrent").await);
    }
    h.stop().await; remote.stop();
}

#[tokio::test(flavor="multi_thread", worker_threads=4)]
async fn independent_cancel_while_actual_sqlite_commit_waits() {
    let home = unique_dir("review-commit-cancel"); fixture(&home);
    let remote = Remote::start().await; let h = boot(home, &remote, 1000000).await;
    h.fill("review-cancel").await; h.settings(json!({"compaction":{"keepRecentTokens":1}})).await;
    let original = h.snapshot("review-cancel").await;
    remote.gate.store(true, Ordering::SeqCst);
    let client = ApiClient{addr:h.server.addr,token:h.server.token.clone()};
    let pending = tokio::spawn(async move {client.request("POST","/lingxi/v1/sessions/review-cancel/compact",Some(json!({}))).await});
    remote.parked().await;
    let writer = rusqlite::Connection::open(h.home.join("lingxi-service/data/runs.db")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    remote.release.add_permits(1); tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(tokio::time::timeout(Duration::from_millis(100),h.state.storage().query_one_text("SELECT 'queue-probe'",vec![])).await.is_err(),"实际保存事务应正在等外部写锁");
    assert!(h.state.compaction().unwrap().cancel_manual("review-cancel").unwrap());
    writer.execute_batch("ROLLBACK").unwrap(); drop(writer);
    let (code,result)=pending.await.unwrap(); assert_eq!(code,409,"{result}");
    assert_eq!(original,h.snapshot("review-cancel").await);
    h.stop().await; remote.stop();
}

#[tokio::test(flavor="multi_thread", worker_threads=4)]
async fn independent_material_change_after_recheck_before_sqlite_commit_must_be_refused() {
    let home=unique_dir("review-late-material"); fixture(&home);
    let remote=Remote::start().await; let h=boot(home,&remote,1000000).await;
    h.fill("review-late").await; h.settings(json!({"compaction":{"keepRecentTokens":1}})).await;
    let original=h.snapshot("review-late").await;
    remote.gate.store(true,Ordering::SeqCst);
    let client=ApiClient{addr:h.server.addr,token:h.server.token.clone()};
    let pending=tokio::spawn(async move{client.request("POST","/lingxi/v1/sessions/review-late/fresh-compact",Some(json!({}))).await});
    remote.parked().await;
    let writer=rusqlite::Connection::open(h.home.join("lingxi-service/data/runs.db")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    remote.release.add_permits(1); tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(tokio::time::timeout(Duration::from_millis(100),h.state.storage().query_one_text("SELECT 'queue-probe'",vec![])).await.is_err(),"实际保存事务应正在等外部写锁");
    let config=h.home.join("agents/lingxi/config.yaml");
    let old=std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config,old.replace("EX01-OLD-NAME","INDEPENDENT-LATE-NEW-NAME")).unwrap();
    writer.execute_batch("ROLLBACK").unwrap(); drop(writer);
    let (code,result)=pending.await.unwrap();
    eprintln!("INDEPENDENT_LATE_MATERIAL status={code} result={result}");
    if code==200 {eprintln!("INDEPENDENT_STALE_ARCHIVE render={}",h.archive("review-late").await["refreshedContext"]["render"]);}
    assert_eq!(code,409,"材料在真实提交前已改变，不应提交旧刷新结果: {result}");
    assert_eq!(original,h.snapshot("review-late").await);
    h.stop().await;remote.stop();
}

#[tokio::test(flavor="multi_thread", worker_threads=4)]
async fn independent_post_usage_precommit_material_change_must_not_commit_stale_context() {
    let home=unique_dir("review-after-usage");fixture(&home);
    let remote=Remote::start().await;let h=boot(home,&remote,1000000).await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}})).await;
    remote.gate.store(true,Ordering::SeqCst);
    let mut caught=false;
    for index in 0..12 {
        let id=format!("late-usage-{index}");h.fill(&id).await;
        let original=h.snapshot(&id).await;
        let client=ApiClient{addr:h.server.addr,token:h.server.token.clone()};let request_id=id.clone();
        let pending=tokio::spawn(async move {client.request("POST",&format!("/lingxi/v1/sessions/{request_id}/fresh-compact"),Some(json!({}))).await});
        remote.parked().await;
        let db=h.home.join("lingxi-service/data/runs.db");let watcher_id=id.clone();
        let watcher=tokio::task::spawn_blocking(move || {
            let writer=rusqlite::Connection::open(db).unwrap();
            for _ in 0..100000 {
                let count:i64=writer.query_row("SELECT COUNT(*) FROM model_call_usage WHERE session_id=?1 AND purpose='auxiliary.summarize'",[&watcher_id],|r|r.get(0)).unwrap();
                if count>0 {
                    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
                    let committed:i64=writer.query_row("SELECT COUNT(*) FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%'",[&watcher_id],|r|r.get(0)).unwrap();
                    return (writer,committed==0);
                }
                std::thread::sleep(Duration::from_micros(50));
            }
            panic!("summary usage never settled")
        });
        remote.release.add_permits(1);
        let (writer, before_commit)=watcher.await.unwrap();
        if !before_commit {writer.execute_batch("ROLLBACK").unwrap();drop(writer);assert_eq!(pending.await.unwrap().0,200);continue;}
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(tokio::time::timeout(Duration::from_millis(100),h.state.storage().query_one_text("SELECT 'queue-probe'",vec![])).await.is_err());
        let config=h.home.join("agents/lingxi/config.yaml");let old=std::fs::read_to_string(&config).unwrap();
        std::fs::write(&config,old.replace("EX01-OLD-NAME","INDEPENDENT-AFTER-USAGE-NEW-NAME")).unwrap();
        writer.execute_batch("ROLLBACK").unwrap();drop(writer);
        let (code,result)=pending.await.unwrap();
        eprintln!("INDEPENDENT_AFTER_USAGE_PRECOMMIT session={id} status={code} result={result}");
        if code==200 {eprintln!("INDEPENDENT_AFTER_USAGE_STALE_RENDER={}",h.archive(&id).await["refreshedContext"]["render"]);}
        assert_eq!(code,409,"摘要结算完成且保存未完成时材料已修改，必须拒绝旧材料提交: {result}");
        assert_eq!(original,h.snapshot(&id).await); independent_assert_no_commit(&h,&id).await; caught=true;break;
    }
    assert!(caught,"未抓到真实摘要结算后/提交前窗口，不能声称覆盖");
    h.stop().await;remote.stop();
}

#[tokio::test(flavor="multi_thread", worker_threads=4)]
async fn independent_post_usage_precommit_tool_change_must_not_commit_stale_context() {
    let home=unique_dir("review-after-usage-tool");fixture(&home);
    let remote=Remote::start().await;let h=boot(home,&remote,1000000).await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}})).await;
    remote.gate.store(true,Ordering::SeqCst);
    let mut caught=false;
    for index in 0..12 {
        let id=format!("late-tool-{index}");h.fill(&id).await;
        let original=h.snapshot(&id).await;
        let client=ApiClient{addr:h.server.addr,token:h.server.token.clone()};let request_id=id.clone();
        let pending=tokio::spawn(async move {client.request("POST",&format!("/lingxi/v1/sessions/{request_id}/fresh-compact"),Some(json!({}))).await});
        remote.parked().await;
        let db=h.home.join("lingxi-service/data/runs.db");let watcher_id=id.clone();
        let watcher=tokio::task::spawn_blocking(move || {
            let writer=rusqlite::Connection::open(db).unwrap();
            for _ in 0..100000 {
                let count:i64=writer.query_row("SELECT COUNT(*) FROM model_call_usage WHERE session_id=?1 AND purpose='auxiliary.summarize'",[&watcher_id],|r|r.get(0)).unwrap();
                if count>0 {
                    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
                    let committed:i64=writer.query_row("SELECT COUNT(*) FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%'",[&watcher_id],|r|r.get(0)).unwrap();
                    return (writer,committed==0);
                }
                std::thread::sleep(Duration::from_micros(50));
            }
            panic!("summary usage never settled")
        });
        remote.release.add_permits(1);
        let (writer, before_commit)=watcher.await.unwrap();
        if !before_commit {writer.execute_batch("ROLLBACK").unwrap();drop(writer);assert_eq!(pending.await.unwrap().0,200);continue;}
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(tokio::time::timeout(Duration::from_millis(100),h.state.storage().query_one_text("SELECT 'queue-probe'",vec![])).await.is_err());
        let registry=h.state.tool_gateway().unwrap().registry();
        let declaration=lingxi_kernel::model_exchange::ToolDeclarationSnapshot::from_registry(registry).unwrap().declarations[0].target.clone();
        registry.set_availability(&lingxi_kernel::toolcatalog::ToolTargetId::parse(&declaration),lingxi_kernel::toolcatalog::Availability::Disabled{reason:"independent late user tool disable".into()}).unwrap();
        writer.execute_batch("ROLLBACK").unwrap();drop(writer);
        let (code,result)=pending.await.unwrap();
        eprintln!("INDEPENDENT_AFTER_USAGE_TOOL_PRECOMMIT session={id} status={code} result={result}");
        if code==200 {eprintln!("INDEPENDENT_AFTER_USAGE_STALE_TOOLS={}",h.archive(&id).await["refreshedContext"]["tools"]);}
        assert_eq!(code,409,"摘要结算完成且保存未完成时工具已禁用，必须拒绝旧工具快照提交: {result}");
        assert_eq!(original,h.snapshot(&id).await); independent_assert_no_commit(&h,&id).await; caught=true;break;
    }
    assert!(caught,"未抓到真实摘要结算后/提交前窗口，不能声称覆盖");
    h.stop().await;remote.stop();
}

#[tokio::test(flavor="multi_thread", worker_threads=4)]
async fn independent_post_usage_precommit_cancel_preserves_history() {
    let home=unique_dir("review-after-usage-cancel");fixture(&home);
    let remote=Remote::start().await;let h=boot(home,&remote,1000000).await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}})).await;
    remote.gate.store(true,Ordering::SeqCst);
    let mut caught=false;
    for index in 0..12 {
        let id=format!("late-cancel-{index}");h.fill(&id).await;
        let original=h.snapshot(&id).await;
        let client=ApiClient{addr:h.server.addr,token:h.server.token.clone()};let request_id=id.clone();
        let pending=tokio::spawn(async move {client.request("POST",&format!("/lingxi/v1/sessions/{request_id}/fresh-compact"),Some(json!({}))).await});
        remote.parked().await;
        let db=h.home.join("lingxi-service/data/runs.db");let watcher_id=id.clone();
        let watcher=tokio::task::spawn_blocking(move || {
            let writer=rusqlite::Connection::open(db).unwrap();
            for _ in 0..100000 {
                let count:i64=writer.query_row("SELECT COUNT(*) FROM model_call_usage WHERE session_id=?1 AND purpose='auxiliary.summarize'",[&watcher_id],|r|r.get(0)).unwrap();
                if count>0 {
                    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
                    let committed:i64=writer.query_row("SELECT COUNT(*) FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%'",[&watcher_id],|r|r.get(0)).unwrap();
                    return (writer,committed==0);
                }
                std::thread::sleep(Duration::from_micros(50));
            }
            panic!("summary usage never settled")
        });
        remote.release.add_permits(1);
        let (writer, before_commit)=watcher.await.unwrap();
        if !before_commit {writer.execute_batch("ROLLBACK").unwrap();drop(writer);assert_eq!(pending.await.unwrap().0,200);continue;}
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(tokio::time::timeout(Duration::from_millis(100),h.state.storage().query_one_text("SELECT 'queue-probe'",vec![])).await.is_err());
        assert!(h.state.compaction().unwrap().cancel_manual(&id).unwrap(),"实际取消域必须赢得尚未提交的裁决");
        writer.execute_batch("ROLLBACK").unwrap();drop(writer);
        let (code,result)=pending.await.unwrap();
        eprintln!("INDEPENDENT_AFTER_USAGE_CANCEL_PRECOMMIT session={id} status={code} result={result}");
        if code==200 {eprintln!("INDEPENDENT_AFTER_USAGE_STALE_RENDER={}",h.archive(&id).await["refreshedContext"]["render"]);}
        assert_eq!(code,409,"摘要结算完成且保存未完成时取消已获准，必须拒绝提交: {result}");
        assert_eq!(original,h.snapshot(&id).await); independent_assert_no_commit(&h,&id).await; caught=true;break;
    }
    assert!(caught,"未抓到真实摘要结算后/提交前窗口，不能声称覆盖");
    h.stop().await;remote.stop();
}


#[tokio::test(flavor="multi_thread", worker_threads=4)]
async fn independent_post_usage_precommit_actual_put_settings_change_preserves_history() {
    let home=unique_dir("review-after-usage-settings");fixture(&home);
    let remote=Remote::start().await;let h=boot(home,&remote,1000000).await;
    h.settings(json!({"compaction":{"keepRecentTokens":1}})).await;
    remote.gate.store(true,Ordering::SeqCst);
    let mut caught=false;
    for index in 0..12 {
        let id=format!("late-settings-{index}");h.fill(&id).await;
        let original=h.snapshot(&id).await;
        let client=ApiClient{addr:h.server.addr,token:h.server.token.clone()};let request_id=id.clone();
        let pending=tokio::spawn(async move {client.request("POST",&format!("/lingxi/v1/sessions/{request_id}/fresh-compact"),Some(json!({}))).await});
        remote.parked().await;
        let db=h.home.join("lingxi-service/data/runs.db");let watcher_id=id.clone();
        let watcher=tokio::task::spawn_blocking(move || {
            let writer=rusqlite::Connection::open(db).unwrap();
            for _ in 0..100000 {
                let count:i64=writer.query_row("SELECT COUNT(*) FROM model_call_usage WHERE session_id=?1 AND purpose='auxiliary.summarize'",[&watcher_id],|r|r.get(0)).unwrap();
                if count>0 {
                    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
                    let committed:i64=writer.query_row("SELECT COUNT(*) FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%'",[&watcher_id],|r|r.get(0)).unwrap();
                    return (writer,committed==0);
                }
                std::thread::sleep(Duration::from_micros(50));
            }
            panic!("summary usage never settled")
        });
        remote.release.add_permits(1);
        let (writer, before_commit)=watcher.await.unwrap();
        if !before_commit {writer.execute_batch("ROLLBACK").unwrap();drop(writer);assert_eq!(pending.await.unwrap().0,200);continue;}
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(tokio::time::timeout(Duration::from_millis(100),h.state.storage().query_one_text("SELECT 'queue-probe'",vec![])).await.is_err());
        h.settings(json!({"compaction":{"reserveTokens":17001}})).await;
        assert_eq!(h.state.compaction().unwrap().settings_for("lingxi").unwrap().reserve_tokens,Some(17001));
        writer.execute_batch("ROLLBACK").unwrap();drop(writer);
        let (code,result)=pending.await.unwrap();
        eprintln!("INDEPENDENT_AFTER_USAGE_SETTINGS_PRECOMMIT session={id} status={code} result={result}");
        if code==200 {eprintln!("INDEPENDENT_AFTER_USAGE_STALE_RENDER={}",h.archive(&id).await["refreshedContext"]["render"]);}
        assert_eq!(code,409,"摘要结算完成且保存未完成时真实PUT设置已修改，必须拒绝提交: {result}");
        assert_eq!(original,h.snapshot(&id).await); independent_assert_no_commit(&h,&id).await; caught=true;break;
    }
    assert!(caught,"未抓到真实摘要结算后/提交前窗口，不能声称覆盖");
    h.stop().await;remote.stop();
}


async fn independent_assert_no_commit(h:&Harness,id:&str) {
    let count=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id=?1 AND message_id LIKE 'model-history:manual:%'",vec![id.into()]).await.unwrap().unwrap();
    assert_eq!(count,"0");
    let events=h.state.app_events().since(0);let all=events["events"].as_array().unwrap();
    let accepted:Vec<_>=all.iter().filter(|e|e["payload"]["sessionId"]==id && e["type"]=="compaction_accepted").collect();
    assert_eq!(accepted.len(),1);
    assert!(!all.iter().any(|e|e["payload"]["sessionId"]==id && e["type"]=="compaction_succeeded"));
    let ledger=h.state.storage().query_one_text("SELECT json_object('count',COUNT(*),'run',run_id,'attempt',attempt,'cause',cause_ref) FROM model_call_usage WHERE session_id=?1 AND purpose='auxiliary.summarize'",vec![id.into()]).await.unwrap().unwrap();
    let ledger:Value=serde_json::from_str(&ledger).unwrap();assert_eq!(ledger["count"],1);assert!(ledger["run"].is_null() && ledger["attempt"].is_null());assert_eq!(ledger["cause"],accepted[0]["payload"]["actionId"]);
}

#[tokio::test(flavor="multi_thread",worker_threads=4)]
async fn independent_actual_commit_wins_then_cancel_is_refused() {
    let home=unique_dir("review-commit-wins");fixture(&home);let remote=Remote::start().await;
    let h=boot(home,&remote,1000000).await;h.fill("commit-wins").await;h.settings(json!({"compaction":{"keepRecentTokens":1}})).await;
    remote.gate.store(true,Ordering::SeqCst);
    let client=ApiClient{addr:h.server.addr,token:h.server.token.clone()};
    let pending=tokio::spawn(async move{client.request("POST","/lingxi/v1/sessions/commit-wins/compact",Some(json!({}))).await});
    remote.parked().await;let db=h.home.join("lingxi-service/data/runs.db");
    let watcher=tokio::task::spawn_blocking(move|| {
        let conn=rusqlite::Connection::open(db).unwrap();let deadline=std::time::Instant::now()+Duration::from_secs(10);
        while std::time::Instant::now()<deadline {
            let count:i64=conn.query_row("SELECT COUNT(*) FROM messages WHERE session_id='commit-wins' AND message_id LIKE 'model-history:manual:%'",[],|r|r.get(0)).unwrap();
            if count==1{return;}
            std::thread::sleep(Duration::from_micros(50));
        }
        panic!("未观察到真实提交")
    });
    remote.release.add_permits(1);watcher.await.unwrap();
    assert!(!h.state.compaction().unwrap().cancel_manual("commit-wins").unwrap(),"提交已获胜不得再接受取消");
    let (code,result)=pending.await.unwrap();assert_eq!(code,200,"{result}");
    let saved=h.snapshot("commit-wins").await;
    let (code,result)=h.server.request("POST","/lingxi/v1/sessions/commit-wins/compact/cancel",None).await;assert_eq!(code,409,"{result}");
    assert_eq!(saved,h.snapshot("commit-wins").await);
    h.execute("commit-wins","INDEPENDENT-AFTER-COMMIT-WINS").await;
    assert!(remote.captured().last().unwrap()["messages"].to_string().contains("EX01-SUMMARY"));
    h.stop().await;remote.stop();
}

#[tokio::test(flavor="multi_thread",worker_threads=4)]
async fn independent_actual_config_put_cannot_cross_active_material_commit_fence() {
    let home=unique_dir("review-actual-put-fence");fixture(&home);let remote=Remote::start().await;
    let h=boot(home,&remote,1000000).await;
    let config_path=h.home.join("agents/lingxi/config.yaml");
    let original_config=std::fs::read(&config_path).unwrap();
    let epoch=lingxi_adapters::storage::agent_memory::context_material_epoch(&h.home);
    let (entered_tx,entered_rx)=std::sync::mpsc::channel();let (release_tx,release_rx)=std::sync::mpsc::channel();
    let held_epoch=epoch.clone();
    let before_revision=held_epoch.stable_revision().unwrap();
    let guard=std::thread::spawn(move||epoch.with_stable_commit(||{entered_tx.send(()).unwrap();release_rx.recv_timeout(Duration::from_secs(5)).unwrap();}).unwrap());
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let client=ApiClient{addr:h.server.addr,token:h.server.token.clone()};
    let mut put=tokio::spawn(async move{client.request("PUT","/lingxi/v1/agents/lingxi/config",Some(json!({"compaction":{"reserveTokens":18001}}))).await});
    let observed=tokio::time::timeout(Duration::from_millis(250),&mut put).await;
    let during_revision=held_epoch.stable_revision();
    let during_reserve=h.state.compaction().unwrap().settings_for("lingxi").unwrap().reserve_tokens;
    let during_config=std::fs::read(&config_path).unwrap();
    eprintln!("INDEPENDENT_PUT_FENCE during_reserve={during_reserve:?} before_revision={before_revision} during_revision={during_revision:?}");
    // 先释放真栅栏，无论反例成功与否都不留下被阻塞线程。
    release_tx.send(()).unwrap();guard.join().unwrap();
    match observed {
        Ok(result)=>{let (code,value)=result.unwrap();eprintln!("INDEPENDENT_PUT_CROSSED_COMMIT_FENCE status={code} result={value}");assert_ne!(code,200,"真实PUT不应在最终保存材料栅栏活动期间成功发布");},
        Err(_)=>{assert_eq!(during_config,original_config,"保存栅栏中磁盘配置不能改变");assert_eq!(during_revision,Some(before_revision));assert_eq!(put.await.unwrap().0,200);assert_eq!(h.state.compaction().unwrap().settings_for("lingxi").unwrap().reserve_tokens,Some(18001));assert_ne!(std::fs::read(&config_path).unwrap(),original_config);assert_eq!(held_epoch.stable_revision(),Some(before_revision+4));eprintln!("INDEPENDENT_PUT_FENCE_RELEASED reserve=18001 revision={:?}",held_epoch.stable_revision());}
    }
    h.stop().await;remote.stop();
}
