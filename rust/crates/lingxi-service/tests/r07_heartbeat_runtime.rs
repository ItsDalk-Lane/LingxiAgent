//! 心跳使用真实HTTP、工具网关、ResourceIO和SQLite；仅模型供应商为loopback替身。
use lingxi_service::resourceio::OpContext;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static HARNESS_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct Remote {
    endpoint: String,
    wire: Arc<Mutex<Vec<Value>>>,
    initial: Arc<AtomicUsize>,
    park: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    task: tokio::task::JoinHandle<()>,
}
impl Remote {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let wire = Arc::new(Mutex::new(Vec::new()));
        let initial = Arc::new(AtomicUsize::new(0));
        let park = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let (w, c, p, e, r) = (
            wire.clone(),
            initial.clone(),
            park.clone(),
            entered.clone(),
            release.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (w, c, p, e, r) = (w.clone(), c.clone(), p.clone(), e.clone(), r.clone());
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0; 8192];
                    let head = loop {
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if let Some(i) = raw.windows(4).position(|v| v == b"\r\n\r\n") {
                            break i + 4;
                        }
                    };
                    let len = String::from_utf8_lossy(&raw[..head])
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|s| s.trim().parse::<usize>().ok())
                        })
                        .unwrap();
                    while raw.len() < head + len {
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                    }
                    let request: Value = serde_json::from_slice(&raw[head..head + len]).unwrap();
                    w.lock().unwrap().push(request.clone());
                    let messages = request["messages"].as_array().unwrap();
                    let tool_result = messages.iter().any(|m| m["role"] == "tool");
                    let frames = if !tool_result {
                        c.fetch_add(1, Ordering::SeqCst);
                        if p.load(Ordering::SeqCst) {
                            e.add_permits(1);
                            r.acquire().await.unwrap().forget();
                        }
                        let input = messages.iter().rfind(|m| m["role"] == "user").unwrap()
                            ["content"]
                            .to_string();
                        let jian =
                            input.contains("[目录巡检]") || input.contains("[Directory Patrol]");
                        let name = if jian {
                            "jian_update_status"
                        } else {
                            "patrol_update_log"
                        };
                        let declared = request["tools"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|t| t["function"]["name"] == name)
                            .unwrap_or_else(|| panic!("真实模型请求未声明{name}: {request}"));
                        assert_eq!(declared["function"]["name"], name);
                        let args = if jian {
                            json!({"status":"in_progress","progress":"1/5","note":"完成本轮笺任务"})
                        } else {
                            json!({"status":"completed","note":"完成真实工作台巡检"})
                        };
                        vec![
                            json!({"id":"patrol-synthetic","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"patrol-fixed-log","type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]}),
                            json!({"id":"patrol-synthetic","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
                            json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":10}}),
                        ]
                    } else {
                        vec![
                            json!({"id":"patrol-synthetic","choices":[{"index":0,"delta":{"role":"assistant","content":"本轮已完成。"},"finish_reason":null}]}),
                            json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                            json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":10}}),
                        ]
                    };
                    let payload = frames
                        .into_iter()
                        .map(|f| format!("data: {f}\n\n"))
                        .collect::<String>()
                        + "data: [DONE]\n\n";
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            endpoint,
            wire,
            initial,
            park,
            entered,
            release,
            task,
        }
    }
    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}
impl Drop for Remote {
    fn drop(&mut self) {
        self.park.store(false, Ordering::SeqCst);
        self.release.add_permits(100);
        self.task.abort();
    }
}

struct Harness {
    base: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn start(remote: &Remote) -> Self {
        let base = PathBuf::from(std::env::var("LINGXI_HOME").expect("隔离home")).join(format!(
            "r07-patrol-{}-{}-{}",
            std::process::id(),
            HARNESS_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = base.join("home");
        let workspace = base.join("workspace");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        let home = home.canonicalize().unwrap();
        let workspace = workspace.canonicalize().unwrap();
        for agent in ["lingxi", "other"] {
            let dir = home.join("agents").join(agent);
            std::fs::create_dir_all(dir.join("desk")).unwrap();
            let root = if agent == "lingxi" {
                workspace.clone()
            } else {
                workspace.join("other-workspace")
            };
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(dir.join("config.yaml"),format!("locale: zh-CN\nagent:\n  name: {agent}\n  yuan: lingxi\nmemory:\n  enabled: false\ndesk:\n  heartbeat_enabled: true\n  heartbeat_interval: 31\n  home_folder: {}\n",serde_json::to_string(&root.to_string_lossy()).unwrap())).unwrap();
            std::fs::write(
                dir.join("desk/overwatch.md"),
                "OVERWATCH-REAL-INPUT：留意用户待办。",
            )
            .unwrap();
        }
        std::fs::create_dir_all(workspace.join("project/deeper")).unwrap();
        std::fs::create_dir_all(workspace.join(".hidden")).unwrap();
        std::fs::write(workspace.join("jian.md"), "根目录任务：执行五次。\n").unwrap();
        std::fs::write(workspace.join("project/jian.md"), "一级任务：执行五次。\n").unwrap();
        std::fs::write(workspace.join("project/deeper/jian.md"), "不应读取的二级笺").unwrap();
        std::fs::write(workspace.join(".hidden/jian.md"), "不应读取的隐藏笺").unwrap();
        let product = home.join("product");
        std::fs::create_dir_all(product.join("yuan")).unwrap();
        std::fs::write(
            product.join("yuan/lingxi.md"),
            "你叫{{agentName}}，独立完成用户授权的工作。",
        )
        .unwrap();
        let layout = prepare_layout(&home).unwrap();
        let plane=lingxi_adapters::models::config::ModelPlaneConfig::from_value(json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":remote.endpoint,"auth":{"kind":"apiKey","apiKey":"R07_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}}}})).unwrap();
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
                        os_release: "synthetic".into(),
                        shell_label: "test-shell".into(),
                        workspace: workspace.to_string_lossy().into_owned(),
                    },
                ),
            )),
        );
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home,
                home_source: HomeSource::Env,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 10_000,
            },
            &layout,
            ServiceDeps {
                model_gateway: Some(Arc::new(
                    lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
                )),
                credential_service: Some(credentials),
                context_compiler: Some(compiler),
                workspace_root: Some(workspace.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (ready, rx) = tokio::sync::oneshot::channel();
        let service = state.clone();
        let task = tokio::spawn(async move {
            run(
                service,
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
        let addr = rx.await.unwrap();
        Self {
            base,
            workspace,
            state,
            url: format!("http://{addr}"),
            token,
            stop,
            task,
        }
    }
    async fn request(&self, path: &str, body: Value) -> (u16, Value) {
        let response = reqwest::Client::new()
            .post(format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let value = response.json().await.unwrap();
        (status, value)
    }
    async fn trigger(&self, agent: &str) -> Value {
        let (code, body) = self
            .request(
                &format!("/lingxi/v1/desk/heartbeat?agentId={agent}"),
                json!({}),
            )
            .await;
        assert_eq!(code, 200, "{body}");
        body
    }
    async fn finished(&self, agent: &str) {
        let result = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if self.state.automation().patrol().status(agent)["running"] == false {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(result.is_ok(),"心跳未按时退出: {}; 真实运行: {:?}",self.state.automation().patrol().status(agent),self.state.storage().query_one_text("SELECT group_concat(r.run_id || ':' || r.status || ':' || coalesce(r.terminal_reason,''), '; ') FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE l.origin='heartbeat'",vec![]).await);
    }
    async fn stop(self) {
        let _ = self.stop.send(());
        let outcome = tokio::time::timeout(Duration::from_secs(12), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!outcome.drain_timed_out);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_agent_cannot_receive_a_workspace_scope_or_background_run() {
    let remote = Remote::start().await;
    let h = Harness::start(&remote).await;
    let (status, body) = h
        .request("/lingxi/v1/desk/heartbeat?agentId=missing-agent", json!({}))
        .await;
    assert!(status >= 400, "{body}");
    let count = h
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM sessions WHERE agent_id='missing-agent'",
            vec![],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count, "0");
    assert_eq!(remote.initial.load(Ordering::SeqCst), 0);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_patrol_and_root_first_level_jian_use_gateway_fixed_logs() {
    let remote = Remote::start().await;
    let h = Harness::start(&remote).await;
    assert_eq!(h.trigger("lingxi").await["started"], true);
    h.finished("lingxi").await;
    let status = h.state.automation().patrol().status("lingxi");
    assert!(status["lastError"].is_null(), "{status}");
    assert_eq!(remote.initial.load(Ordering::SeqCst), 3);
    let log =
        std::fs::read_to_string(h.workspace.join("OH-Works/lingxi的巡检/patrol-log.md")).unwrap();
    assert!(log.contains("完成真实工作台巡检"));
    for (file, body) in [
        ("jian.md", "根目录任务：执行五次。"),
        ("project/jian.md", "一级任务：执行五次。"),
    ] {
        let content = std::fs::read_to_string(h.workspace.join(file)).unwrap();
        assert!(content.starts_with(body));
        assert!(content.contains("<!-- exec-log -->"));
        assert!(content.contains("- 进度：1/5"));
        assert!(content.contains("上次任务快照"));
    }
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("project/deeper/jian.md")).unwrap(),
        "不应读取的二级笺"
    );
    assert_eq!(
        std::fs::read_to_string(h.workspace.join(".hidden/jian.md")).unwrap(),
        "不应读取的隐藏笺"
    );
    let cause = h
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM run_lineage WHERE origin='heartbeat'",
            vec![],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cause, "3");
    let wire = remote.wire.lock().unwrap().clone();
    assert!(wire
        .iter()
        .any(|r| r.to_string().contains("OVERWATCH-REAL-INPUT")));
    assert!(!wire
        .iter()
        .any(|r| r.to_string().contains("不应读取的二级笺")));
    assert!(
        !wire.iter().any(|r| r.to_string().contains("-overwatch-")),
        "模型不能拿collector scope作为来源"
    );
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn separate_agents_run_together_but_same_agent_overlap_is_discarded() {
    let remote = Remote::start().await;
    remote.park.store(true, Ordering::SeqCst);
    let h = Harness::start(&remote).await;
    assert_eq!(h.trigger("lingxi").await["started"], true);
    remote.entered().await;
    assert_eq!(h.trigger("lingxi").await["started"], false);
    assert_eq!(h.trigger("other").await["started"], true);
    remote.entered().await;
    assert_eq!(
        remote.initial.load(Ordering::SeqCst),
        2,
        "两个助手真实HTTP都已进入，未共用全局运行锁"
    );
    remote.park.store(false, Ordering::SeqCst);
    remote.release.add_permits(2);
    h.finished("lingxi").await;
    h.finished("other").await;
    assert!(h.state.automation().patrol().status("lingxi")["lastError"].is_null());
    assert!(h.state.automation().patrol().status("other")["lastError"].is_null());
    let (code, body) = h
        .request("/lingxi/v1/desk/heartbeat?agentId=lingxi", json!({}))
        .await;
    assert!(code >= 400, "手动2分钟冷却必须实际拒绝: {body}");
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_switch_off_cancels_real_inflight_patrol_and_does_not_write_log() {
    let remote = Remote::start().await;
    remote.park.store(true, Ordering::SeqCst);
    let h = Harness::start(&remote).await;
    h.trigger("lingxi").await;
    remote.entered().await;
    let response = reqwest::Client::new()
        .put(format!("{}/lingxi/v1/agents/lingxi/config", h.url))
        .bearer_auth(&h.token)
        .json(&json!({"desk":{"heartbeat_enabled":false}}))
        .send()
        .await
        .unwrap();
    let code = response.status();
    let body: Value = response.json().await.unwrap();
    assert!(code.is_success(), "{body}");
    h.finished("lingxi").await;
    assert!(h.state.automation().patrol().status("lingxi")["lastError"].is_string());
    assert!(!h
        .workspace
        .join("OH-Works/lingxi的巡检/patrol-log.md")
        .exists());
    let run=h.state.storage().query_one_text("SELECT r.status FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE l.origin='heartbeat'",vec![]).await.unwrap().unwrap();
    assert_eq!(run, "cancelled");
    remote.park.store(false, Ordering::SeqCst);
    remote.release.add_permits(1);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn create_if_absent_never_overwrites_a_competing_resource() {
    let remote = Remote::start().await;
    let h = Harness::start(&remote).await;
    let io = h.state.resource_io();
    let ctx = OpContext::local_owner("");
    let path = h.workspace.join("new-logs/patrol.md");
    let resource = json!({"kind":"local-file","path":path});
    let first = io
        .write_expected_version(
            &resource,
            b"first-owner".to_vec(),
            &json!({"exists":false}),
            &ctx,
        )
        .await
        .unwrap();
    assert_ne!(first["ok"], false);
    let second = io
        .write_expected_version(
            &resource,
            b"must-not-overwrite".to_vec(),
            &json!({"exists":false}),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(second["conflict"], true);
    assert_eq!(std::fs::read(&path).unwrap(), b"first-owner");
    let path = h.workspace.join("new-logs/race.md");
    let resource = json!({"kind":"local-file","path":path});
    let expected = json!({"exists":false});
    let (a, b) = tokio::join!(
        io.write_expected_version(&resource, b"A".to_vec(), &expected, &ctx),
        io.write_expected_version(&resource, b"B".to_vec(), &expected, &ctx)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a["conflict"] == true, b["conflict"] == true);
    let content = std::fs::read(path).unwrap();
    assert!(content == b"A" || content == b"B");
    let outside = h.base.join("not-authorized/log.md");
    assert!(io
        .write_expected_version(
            &json!({"kind":"local-file","path":outside}),
            b"forbidden".to_vec(),
            &expected,
            &ctx
        )
        .await
        .is_err());
    assert!(!outside.exists());
    #[cfg(unix)]
    {
        let outside_dir = h.base.join("outside-link-target");
        std::fs::create_dir_all(&outside_dir).unwrap();
        std::os::unix::fs::symlink(&outside_dir, h.workspace.join("redirected-logs")).unwrap();
        assert!(io
            .write_expected_version(
                &json!({"kind":"local-file","path":h.workspace.join("redirected-logs/patrol.md")}),
                b"forbidden".to_vec(),
                &expected,
                &ctx
            )
            .await
            .is_err());
        assert!(!outside_dir.join("patrol.md").exists());
    }
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordinary_chat_cannot_borrow_heartbeat_program_log_target() {
    let remote = Remote::start().await;
    let h = Harness::start(&remote).await;
    let (code, body) = h
        .request(
            "/lingxi/v1/sessions",
            json!({"sessionId":"ordinary","agentId":"lingxi","title":"普通对话"}),
        )
        .await;
    assert_eq!(code, 201, "{body}");
    // 合成供应商故意请求全局声明中的巡检日志工具；准备阶段必须拒绝缺少宿主绑定的调用。
    let (code, body) = h
        .request(
            "/lingxi/v1/sessions/ordinary/execute",
            json!({"input":"普通对话不应取得巡检日志写权限"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(!h
        .workspace
        .join("OH-Works/lingxi的巡检/patrol-log.md")
        .exists());
    let stored = h
        .state
        .storage()
        .query_one_text(
            "SELECT group_concat(content_json,'|') FROM messages WHERE session_id='ordinary'",
            vec![],
        )
        .await
        .unwrap()
        .unwrap();
    assert!(
        stored.contains("no fixed heartbeat log target") || stored.contains("resource_scope"),
        "真实网关拒绝必须保留在历史中: {stored}"
    );
    assert_eq!(
        h.state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM run_lineage WHERE origin='heartbeat'",
                vec![]
            )
            .await
            .unwrap()
            .unwrap(),
        "0"
    );
    h.stop().await;
}
