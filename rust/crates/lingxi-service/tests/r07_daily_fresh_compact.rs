//! 真实服务、SQLite、压缩执行与用量台账；仅模型供应商使用本机合成HTTP。
use lingxi_adapters::storage::{automation as store, session_operations};
use lingxi_service::{
    automation_daily::{check_daily_once, daily_logical_date},
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static HARNESS_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn unique_home() -> PathBuf {
    let root = PathBuf::from(std::env::var("LINGXI_HOME").expect("隔离测试目录"));
    let home = root.join(format!(
        "r07-daily-{}-{}-{}",
        std::process::id(),
        HARNESS_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&home).unwrap();
    home.canonicalize().unwrap()
}

struct Remote {
    endpoint: String,
    summaries: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    park: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    task: tokio::task::JoinHandle<()>,
}
impl Remote {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let summaries = Arc::new(AtomicUsize::new(0));
        let fail = Arc::new(AtomicBool::new(false));
        let park = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let (count, failing, parking, barrier, unpark) = (
            summaries.clone(),
            fail.clone(),
            park.clone(),
            entered.clone(),
            release.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (count, failing, parking, barrier, unpark) = (
                    count.clone(),
                    failing.clone(),
                    parking.clone(),
                    barrier.clone(),
                    unpark.clone(),
                );
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut chunk = [0; 4096];
                    let header = loop {
                        let n = socket.read(&mut chunk).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&chunk[..n]);
                        if let Some(at) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
                            break at + 4;
                        }
                    };
                    let length = String::from_utf8_lossy(&raw[..header])
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap();
                    while raw.len() < header + length {
                        let n = socket.read(&mut chunk).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&chunk[..n]);
                    }
                    let body: Value =
                        serde_json::from_slice(&raw[header..header + length]).unwrap();
                    let summary = body["model"] == "summary";
                    if summary {
                        count.fetch_add(1, Ordering::SeqCst);
                        if parking.load(Ordering::SeqCst) {
                            barrier.add_permits(1);
                            unpark.acquire().await.unwrap().forget();
                        }
                        if failing.load(Ordering::SeqCst) {
                            let body = json!({"error":{"message":"R07_DAILY_SYNTHETIC_FAILURE"}})
                                .to_string();
                            let response = format!(
                                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = socket.write_all(response.as_bytes()).await;
                            return;
                        }
                    }
                    let text = if summary {
                        "## Goal\nDAILY-REAL-SUMMARY\n## Constraints & Preferences\n保留旧资料\n## Progress\n### Done\n已确认\n### In Progress\n继续\n### Blocked\n无\n## Key Decisions\n保持真实来源\n## Next Steps\n下一轮\n## Critical Context\nDAILY-REAL-SUMMARY"
                    } else {
                        "真实历史回答：保留用户之前给出的资料和当前会话。"
                    };
                    let payload = format!(
                        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        json!({"choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
                        json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":100,"completion_tokens":50}})
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            endpoint,
            summaries,
            fail,
            park,
            entered,
            release,
            task,
        }
    }
    async fn wait_for_summary(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}
impl Drop for Remote {
    fn drop(&mut self) {
        self.release.add_permits(100);
        self.task.abort();
    }
}

struct Harness {
    home: PathBuf,
    state: ServiceState,
    endpoint: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}
impl Harness {
    async fn start(home: PathBuf, remote: &Remote) -> Self {
        let agent = home.join("agents/lingxi");
        std::fs::create_dir_all(&agent).unwrap();
        if !agent.join("config.yaml").exists() {
            std::fs::write(agent.join("config.yaml"),"locale: zh-CN\nagent:\n  name: Daily测试助手\n  yuan: lingxi\nmemory:\n  enabled: false\ncompaction:\n  enabled: false\n  keepRecentTokens: 1\n").unwrap();
        }
        let product = home.join("product");
        std::fs::create_dir_all(product.join("yuan")).unwrap();
        std::fs::write(
            product.join("yuan/lingxi.md"),
            "你叫{{agentName}}，继续帮助用户完成任务。",
        )
        .unwrap();
        let layout = prepare_layout(&home).unwrap();
        let plane=lingxi_adapters::models::config::ModelPlaneConfig::from_value(json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":remote.endpoint,"auth":{"kind":"apiKey","apiKey":"R07_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"summarize":{"provider":"remote","model":"summary","capabilities":{"tools":true},"compat":{"contextWindow":1000000}}}})).unwrap();
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
                        os_release: "synthetic-test-release".into(),
                        shell_label: "test-shell".into(),
                        workspace: home.to_string_lossy().into_owned(),
                    },
                ),
            )),
        );
        let config = ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Env,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: 10_000,
        };
        let state = ServiceState::bootstrap_with_deps(
            config,
            &layout,
            ServiceDeps {
                model_gateway: Some(Arc::new(
                    lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
                )),
                credential_service: Some(credentials),
                context_compiler: Some(compiler),
                workspace_root: Some(home.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (ready, ready_rx) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let task = tokio::spawn(async move {
            run(
                serving,
                async {
                    let _ = stopped.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        let addr = ready_rx.await.unwrap();
        Self {
            home,
            state,
            endpoint: format!("http://{addr}"),
            token,
            stop,
            task,
        }
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        payload: Option<Value>,
    ) -> (u16, Value) {
        let client = reqwest::Client::new();
        let mut request = client
            .request(method, format!("{}{path}", self.endpoint))
            .bearer_auth(&self.token);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let bytes = response.bytes().await.unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            panic!(
                "non-json HTTP {status}: {}",
                String::from_utf8_lossy(&bytes)
            )
        });
        (status, value)
    }
    async fn create(&self, id: &str, bridge: bool) {
        let (status, body) = self
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/sessions",
                Some(json!({"sessionId":id,"agentId":"lingxi","title":id})),
            )
            .await;
        assert_eq!(status, 201, "{body}");
        if bridge {
            let (status, body) = self
                .request(
                    reqwest::Method::POST,
                    &format!("/lingxi/v1/sessions/{id}/bridge"),
                    Some(json!({"channelKey":format!("synthetic:{id}")})),
                )
                .await;
            assert_eq!(status, 201, "{body}");
        }
    }
    async fn fill(&self, id: &str) {
        for n in 0..3 {
            let (status,body)=self.request(reqwest::Method::POST,&format!("/lingxi/v1/sessions/{id}/execute"),Some(json!({"input":format!("DAILY-PREVIOUS-USER-{n}：这是需要保留的历史材料。") }))).await;
            assert_eq!(status, 200, "{body}");
        }
    }
    async fn snapshot(&self, id: &str) -> String {
        self.state.storage().query_one_text("SELECT json_object('messages',(SELECT group_concat(message_id||':'||content_json,'|') FROM (SELECT * FROM messages WHERE session_id=?1 ORDER BY seq)),'head',(SELECT head_message_id FROM session_branch_heads WHERE session_id=?1),'revision',(SELECT revision FROM session_branch_heads WHERE session_id=?1),'runs',(SELECT COUNT(*) FROM runs WHERE session_id=?1))",vec![id.into()]).await.unwrap().unwrap()
    }
    async fn stop(self) -> PathBuf {
        let _ = self.stop.send(());
        let outcome = tokio::time::timeout(Duration::from_secs(12), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!outcome.drain_timed_out, "{outcome:?}");
        self.home
    }
}

fn midday() -> u64 {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(2026, 10, 16, 10, 0, 0)
        .unwrap()
        .timestamp_millis() as u64
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busy_bridge_is_remaining_work_without_claiming_or_interrupting_it() {
    let remote = Remote::start().await;
    let h = Harness::start(unique_home(), &remote).await;
    h.create("busy-bridge", true).await;
    h.fill("busy-bridge").await;
    let lease = h
        .state
        .sessions()
        .session_supervisor()
        .try_begin_run("busy-bridge")
        .unwrap();
    let before = h.snapshot("busy-bridge").await;
    let report = check_daily_once(&h.state, midday()).await.unwrap();
    assert_eq!(report["staleRemaining"], 1, "{report}");
    assert_eq!(report["bridgeCompacted"], 0, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), 0);
    assert_eq!(h.snapshot("busy-bridge").await, before);
    assert!(store::get_daily(
        h.state.storage(),
        "busy-bridge",
        &daily_logical_date(midday()).unwrap()
    )
    .await
    .unwrap()
    .is_none());
    drop(lease);
    let report = check_daily_once(&h.state, midday() + 3_600_000)
        .await
        .unwrap();
    assert_eq!(report["bridgeCompacted"], 1, "{report}");
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_bridge_only_compacts_once_then_replays_day_receipt_after_restart() {
    let remote = Remote::start().await;
    let h = Harness::start(unique_home(), &remote).await;
    h.create("bridge", true).await;
    h.create("desktop", false).await;
    h.create("attached-bridge", true).await;
    h.fill("bridge").await;
    h.fill("desktop").await;
    let (status, body) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/sessions/attached-bridge/bridge/attachment",
            Some(json!({"desktopSessionId":"desktop"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let desktop = h.snapshot("desktop").await;
    let before = h.snapshot("bridge").await;
    let runs_before = h
        .state
        .storage()
        .query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM runs", vec![])
        .await
        .unwrap();
    let report = check_daily_once(&h.state, midday()).await.unwrap();
    assert_eq!(report["bridgeCompacted"], 1, "{report}");
    assert_eq!(report["failed"], 0, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), 1);
    assert_ne!(h.snapshot("bridge").await, before);
    assert_eq!(h.snapshot("desktop").await, desktop);
    assert_eq!(
        h.state
            .storage()
            .query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM runs", vec![])
            .await
            .unwrap(),
        runs_before,
        "每日整理不制造假的Run"
    );
    let receipt = store::get_daily(
        h.state.storage(),
        "bridge",
        &daily_logical_date(midday()).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(receipt.status, store::DailyStatus::Succeeded);
    assert_eq!(receipt.attempt, 1);
    let usage=h.state.storage().query_one_text("SELECT json_object('count',COUNT(*),'run',run_id,'attempt',attempt,'origin',origin) FROM model_call_usage WHERE session_id='bridge' AND purpose='auxiliary.summarize'",vec![]).await.unwrap().unwrap();
    let usage: Value = serde_json::from_str(&usage).unwrap();
    assert_eq!(usage["count"], 1);
    assert!(usage["run"].is_null());
    assert!(usage["attempt"].is_null());
    assert_eq!(usage["origin"], "session.fresh_compact");
    let report = check_daily_once(&h.state, midday()).await.unwrap();
    assert_eq!(report["alreadySatisfied"], 1, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), 1);
    let home = h.stop().await;
    let h = Harness::start(home, &remote).await;
    // 重启清空接管关系，空会话可被真实检查并得到no-op收据；已成功的bridge不重压。
    let report = check_daily_once(&h.state, midday()).await.unwrap();
    assert_eq!(report["alreadySatisfied"], 1, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), 1);
    assert_eq!(h.snapshot("desktop").await, desktop);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn known_failure_waits_an_hour_and_retries_with_new_attempt() {
    let remote = Remote::start().await;
    let h = Harness::start(unique_home(), &remote).await;
    h.create("retry-bridge", true).await;
    h.fill("retry-bridge").await;
    remote.fail.store(true, Ordering::SeqCst);
    let before = h.snapshot("retry-bridge").await;
    let report = check_daily_once(&h.state, midday()).await.unwrap();
    assert_eq!(report["failed"], 1, "{report}");
    assert_eq!(report["staleRemaining"], 1, "{report}");
    assert_eq!(h.snapshot("retry-bridge").await, before);
    let count = remote.summaries.load(Ordering::SeqCst);
    assert!(count >= 1);
    let failed_receipt = store::get_daily(
        h.state.storage(),
        "retry-bridge",
        &daily_logical_date(midday()).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    let retry_at = failed_receipt.retry_at.unwrap() as u64;
    assert_eq!(
        retry_at - failed_receipt.completed_at.unwrap() as u64,
        3_600_000
    );
    let report = check_daily_once(&h.state, retry_at - 1_000).await.unwrap();
    assert_eq!(report["bridgeCompacted"], 0, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), count);
    remote.fail.store(false, Ordering::SeqCst);
    let report = check_daily_once(&h.state, retry_at).await.unwrap();
    assert_eq!(report["bridgeCompacted"], 1, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), count + 1);
    let receipt = store::get_daily(
        h.state.storage(),
        "retry-bridge",
        &daily_logical_date(midday()).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(receipt.attempt, 2);
    assert_eq!(receipt.status, store::DailyStatus::Succeeded);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn started_unknown_and_closed_intake_never_replay_or_claim_next_day() {
    let remote = Remote::start().await;
    let h = Harness::start(unique_home(), &remote).await;
    h.create("unknown-bridge", true).await;
    h.fill("unknown-bridge").await;
    let date = daily_logical_date(midday()).unwrap();
    let claim = store::claim_daily(h.state.storage(), "unknown-bridge", &date, midday() as i64)
        .await
        .unwrap();
    assert!(claim.claimed);
    let report = check_daily_once(&h.state, midday() + 3_600_000)
        .await
        .unwrap();
    assert_eq!(report["unknown"], 1, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), 0);
    store::settle_daily(
        h.state.storage(),
        "unknown-bridge",
        &date,
        1,
        store::DailyStatus::Unknown,
        None,
        Some("确认之前进程已退出，结果未知".into()),
        midday() as i64,
    )
    .await
    .unwrap();
    let report = check_daily_once(&h.state, midday() + 7_200_000)
        .await
        .unwrap();
    assert_eq!(report["unknown"], 1, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), 0);
    h.state.submission_intake().close();
    let report = check_daily_once(&h.state, midday() + 86_400_000)
        .await
        .unwrap();
    assert_eq!(report["cancelled"], true);
    assert!(store::get_daily(
        h.state.storage(),
        "unknown-bridge",
        &daily_logical_date(midday() + 86_400_000).unwrap()
    )
    .await
    .unwrap()
    .is_none());
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn permission_change_during_real_summary_cancels_and_keeps_unknown_receipt() {
    let remote = Remote::start().await;
    let h = Harness::start(unique_home(), &remote).await;
    h.create("revoked-bridge", true).await;
    h.fill("revoked-bridge").await;
    let before = h.snapshot("revoked-bridge").await;
    remote.park.store(true, Ordering::SeqCst);
    let state = h.state.clone();
    let check = tokio::spawn(async move { check_daily_once(&state, midday()).await });
    remote.wait_for_summary().await;
    session_operations::set_permission(
        h.state.storage(),
        "revoked-bridge",
        lingxi_kernel::subagent::SessionPermissionMode::ReadOnly,
    )
    .await
    .unwrap();
    let report = tokio::time::timeout(Duration::from_secs(10), check)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(report["unknown"], 1, "{report}");
    assert_eq!(report["bridgeCompacted"], 0, "{report}");
    assert_eq!(
        h.snapshot("revoked-bridge").await,
        before,
        "取消没有伪造压缩提交"
    );
    let receipt = store::get_daily(
        h.state.storage(),
        "revoked-bridge",
        &daily_logical_date(midday()).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(receipt.status, store::DailyStatus::Unknown);
    remote.park.store(false, Ordering::SeqCst);
    remote.release.add_permits(1);
    let report = check_daily_once(&h.state, midday() + 3_600_000)
        .await
        .unwrap();
    assert_eq!(report["unknown"], 1, "{report}");
    assert_eq!(remote.summaries.load(Ordering::SeqCst), 1);
    h.stop().await;
}

#[test]
fn logical_day_changes_at_local_four_am() {
    use chrono::TimeZone;
    let before = chrono::Local
        .with_ymd_and_hms(2026, 10, 16, 3, 59, 59)
        .unwrap()
        .timestamp_millis() as u64;
    let after = chrono::Local
        .with_ymd_and_hms(2026, 10, 16, 4, 0, 0)
        .unwrap()
        .timestamp_millis() as u64;
    assert_eq!(daily_logical_date(before).unwrap(), "2026-10-15");
    assert_eq!(daily_logical_date(after).unwrap(), "2026-10-16");
}
