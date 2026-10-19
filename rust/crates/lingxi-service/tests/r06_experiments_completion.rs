//! 实验页与主动委派：原设置容器、真实权限/HTTP、创建策略及模型请求。

use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolOutcome, ToolRequest,
        TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Mutex};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[derive(Default)]
struct Provider {
    results: Mutex<HashMap<String, Value>>,
    calls: std::sync::atomic::AtomicUsize,
    prompts: std::sync::Mutex<Vec<Option<String>>>,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "experiments.fixture".into(),
            model: "deterministic".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _sink: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.prompts
            .lock()
            .unwrap()
            .push(input.system_prompt.clone());
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if input.turn == 1 {
                let command: Value = serde_json::from_str(&input.submission).unwrap();
                let request = ToolRequest::from_effective_arguments(
                    format!("tool:first-party:{}", command["tool"].as_str().unwrap()),
                    command["args"].clone(),
                    &SchemaBudget::default(),
                )
                .unwrap();
                ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![request],
                        content: vec![],
                    },
                )
            } else {
                let value = input
                    .prior
                    .iter()
                    .rev()
                    .find_map(|item| {
                        if let ExchangeItem::ToolResult { outcome, .. } = item {
                            Some(match outcome {
                                ToolOutcome::Success { result } => {
                                    json!({"ok":true,"details":result.details})
                                }
                                ToolOutcome::Failed { error } => {
                                    json!({"ok":false,"error":error.message})
                                }
                                other => json!({"ok":false,"outcome":format!("{other:?}")}),
                            })
                        } else {
                            None
                        }
                    })
                    .expect("必须收到实际工具结果");
                self.results
                    .lock()
                    .unwrap()
                    .insert(format!("{}|{}", ctx.session_id, input.submission), value);
                ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::Final {
                        message: NormalizedMessage {
                            role: "assistant".into(),
                            content: vec![ContentBlock::Text {
                                text: "处理结束".into(),
                            }],
                            model_call_id: Some(call.clone()),
                        },
                    },
                )
            }
        })
    }
}
struct Harness {
    provider: Arc<Provider>,
    home: PathBuf,
    state: ServiceState,
    url: String,
    token: String,
    readonly: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn boot() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-experiments-completion-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        for id in ["agentA", "agentB"] {
            std::fs::create_dir_all(home.join("agents").join(id)).unwrap();
            std::fs::write(home.join("agents").join(id).join("config.yaml"),"agent:\n  name: 测试助手\nskills:\n  enabled: []\nmemory:\n  enabled: true\n  experience_enabled: true\n").unwrap();
        }
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            r#"{"learn_skills":{"enabled":true,"safety_review":true}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(workspace.join("yuan/en")).unwrap();
        std::fs::write(
            workspace.join("yuan/en/lingxi.md"),
            "You are a test assistant.",
        )
        .unwrap();
        std::fs::write(workspace.join("yuan/lingxi.md"), "你是测试助手。").unwrap();
        let layout = prepare_layout(&home).unwrap();
        let compiler = Arc::new(
            lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
                lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                    &home,
                    workspace.clone(),
                    lingxi_service::context_compiler::HostFacts {
                        platform: "darwin".into(),
                        os_type: "Darwin".into(),
                        os_release: "test".into(),
                        shell_label: "zsh".into(),
                        workspace: workspace.to_string_lossy().into(),
                    },
                ),
            )),
        );
        let clock = Arc::new(lingxi_service::inject::ManualClock::new(1_800_000_000_000));
        let approval = Arc::new(lingxi_service::approval_service::ApprovalService::new(
            clock.clone(),
        ));
        let gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            approval.clone(),
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
        lingxi_service::subagents::register_tools(
            gateway.registry(),
            gateway.as_ref(),
            &SchemaBudget::default(),
        );
        let provider = Arc::new(Provider::default());
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 5000,
            },
            &layout,
            ServiceDeps {
                workspace_root: Some(workspace.clone()),
                context_compiler: Some(compiler),
                clock,
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway),
                approval_gate: Some(approval),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let readonly = state
            .auth()
            .issue_device_credential(
                lingxi_service::LOCAL_OWNER_USER_ID,
                &["settings.read"],
                None,
            )
            .unwrap()
            .secret;
        let run_state = state.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (ready, received) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            run(
                run_state,
                async {
                    let _ = rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        let addr = received.await.unwrap();
        Self {
            provider,
            home,
            state,
            url: format!("http://{addr}"),
            token,
            readonly,
            stop: Some(tx),
            task,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value, readonly: bool) -> (u16, Value) {
        let payload = body.to_string();
        let token = if readonly {
            &self.readonly
        } else {
            &self.token
        };
        let wire=format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
        let mut stream = tokio::net::TcpStream::connect(self.url.strip_prefix("http://").unwrap())
            .await
            .unwrap();
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let split = bytes.windows(4).position(|x| x == b"\r\n\r\n").unwrap();
        let header = String::from_utf8_lossy(&bytes[..split]);
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let value = serde_json::from_slice(&bytes[split + 4..]).unwrap();
        (status, value)
    }
    async fn close(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        println!("EXPERIMENT_PROOF {}", self.home.parent().unwrap().display());
    }
}
const EXP: &str = "/lingxi/v1/experiments/subagent.proactive_delegation";
async fn create(h: &Harness, id: &str) {
    let(s,v)=h.request("POST","/lingxi/v1/sessions",json!({"sessionId":id,"agentId":"agentA","title":"真实新会话","permissionMode":"read_only"}),false).await;
    assert_eq!(s, 201, "{v}");
}
fn prompt(h: &Harness, id: &str, child: bool) -> String {
    h.state
        .context_compiler()
        .unwrap()
        .compile_session_snapshot(Some(h.state.storage()), id, "agentA", child, 1, true)
        .unwrap()
        .render()
        .to_string()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_main_session_policy_is_frozen_durable_and_never_grants_tools() {
    let h = Harness::boot().await;
    create(&h, "old").await;
    let hint = "delegate independent research with access=\"read\"";
    assert!(!prompt(&h, "old", false).contains(hint));
    let (status, v) = h.request("PATCH", EXP, json!({"value":true}), false).await;
    assert_eq!(status, 200, "{v}");
    create(&h, "new").await;
    assert!(prompt(&h, "new", false).contains(hint));
    assert!(!prompt(&h, "old", false).contains(hint));
    assert!(!prompt(&h, "new", true).contains(hint));
    assert_eq!(
        h.request("PATCH", EXP, json!({"value":false}), false)
            .await
            .0,
        200
    );
    assert!(prompt(&h, "new", false).contains(hint));
    create(&h, "after-off").await;
    assert!(!prompt(&h, "after-off", false).contains(hint));
    // 提示真正进入本轮模型，写档子代理仍在正式网关被拒绝，零子任务。
    let command =
        json!({"tool":"subagent","args":{"task":"独立调研","access":"write"}}).to_string();
    let (status, result) = h
        .request(
            "POST",
            "/lingxi/v1/sessions/new/execute",
            json!({"input":command}),
            false,
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert!(h
        .provider
        .prompts
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(|p| p.contains(hint)));
    let key = format!("new|{command}");
    let outcome = h
        .provider
        .results
        .lock()
        .unwrap()
        .get(&key)
        .cloned()
        .unwrap();
    assert_eq!(outcome["ok"], false, "{outcome}");
    assert!(
        outcome.to_string().contains("write") || outcome.to_string().contains("read"),
        "不是实际权限拒绝：{outcome}"
    );
    let conn = rusqlite::Connection::open(h.state.storage().db_path()).unwrap();
    let children: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM run_lineage WHERE parent_run_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(children, 0);
    // 重开数据库，创建时的值仍在；权限仍由原权限列负责。
    let conn = rusqlite::Connection::open(h.state.storage().db_path()).unwrap();
    let rows:Vec<(String,i64,String)>=conn.prepare("SELECT session_id,proactive_delegation,permission_mode FROM sessions WHERE session_id IN ('old','new','after-off') ORDER BY session_id").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        rows,
        vec![
            ("after-off".into(), 0, "read_only".into()),
            ("new".into(), 1, "read_only".into()),
            ("old".into(), 0, "read_only".into())
        ]
    );
    // 传统种子/已有会话也维持关闭，不根据全局设置追溯开启。
    assert!(h
        .state
        .context_compiler()
        .unwrap()
        .compile_session_snapshot(Some(h.state.storage()), "missing", "agentA", false, 1, true)
        .is_err());
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn current_experiment_persistence_events_observation_and_real_system_permissions() {
    let h = Harness::boot().await;
    let (status, initial) = h
        .request("GET", "/lingxi/v1/experiments", Value::Null, false)
        .await;
    assert_eq!(status, 200);
    let list = initial["experiments"].as_array().unwrap();
    assert_eq!(list.len(), 4);
    assert!(!list
        .iter()
        .any(|v| v["id"] == "memory.cache_snapshot_reflection"));
    assert_eq!(
        list.iter()
            .find(|v| v["id"] == "subagent.proactive_delegation")
            .unwrap()["value"],
        false
    );
    let before = h.state.app_events().since(0)["latestSequence"]
        .as_u64()
        .unwrap();
    for (value, readonly, status) in [(json!(true), true, 403)] {
        assert_eq!(
            h.request("PATCH", EXP, json!({"value":value}), readonly)
                .await
                .0,
            status
        );
    }
    assert_eq!(
        h.request(
            "PATCH",
            "/lingxi/v1/experiments/unknown",
            json!({"value":true}),
            false
        )
        .await
        .0,
        400
    );
    assert_eq!(h.state.app_events().since(before)["events"], json!([]));
    assert_eq!(
        h.request("PATCH", EXP, json!({"value":true}), false)
            .await
            .0,
        200
    );
    let events = h.state.app_events().since(before);
    assert_eq!(events["events"][0]["type"], "experiment-changed");
    assert_eq!(events["events"][0]["payload"]["value"], true);
    let observation =
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agentA";
    let (s, v) = h.request("GET", observation, Value::Null, false).await;
    assert_eq!(s, 200, "{v}");
    assert!(v["observation"].is_null());
    assert_eq!(
        h.request("DELETE", observation, Value::Null, false).await.0,
        200
    );
    let observation_file = h
        .home
        .join("agents/agentA/experiments/cache-snapshot-reflection/latest.json");
    std::fs::create_dir_all(observation_file.parent().unwrap()).unwrap();
    std::fs::write(&observation_file,json!({"status":"success","mode":"shadow","createdAt":"2026-10-15T00:00:00Z","summaryPreview":"真实保存的观察记录"}).to_string()).unwrap();
    let (status, observed) = h.request("GET", observation, Value::Null, false).await;
    assert_eq!(status, 200);
    assert_eq!(
        observed["observation"]["summaryPreview"],
        "真实保存的观察记录"
    );
    assert_eq!(
        h.request("DELETE", observation, Value::Null, true).await.0,
        403
    );
    assert!(observation_file.is_file());
    let (status, cleared) = h.request("DELETE", observation, Value::Null, false).await;
    assert_eq!(status, 200);
    assert_eq!(cleared["deleted"], true);
    assert!(!observation_file.exists());
    std::fs::create_dir(&observation_file).unwrap();
    assert_eq!(
        h.request("GET", observation, Value::Null, false).await.0,
        400
    );
    assert_eq!(
        h.request("DELETE", observation, Value::Null, false).await.0,
        500
    );
    assert!(observation_file.is_dir());
    std::fs::remove_dir(&observation_file).unwrap();
    let (s, c) = h
        .request(
            "GET",
            "/lingxi/v1/preferences/computer-use",
            Value::Null,
            false,
        )
        .await;
    assert_eq!(s, 200, "{c}");
    assert_eq!(c["settings"]["enabled"], false);
    assert!(c["status"]["activeLease"].is_null());
    #[cfg(target_os = "macos")]
    {
        assert_eq!(c["selectedProviderId"], "macos:cua");
        let status = &c["status"]["providers"][0]["status"];
        assert_eq!(status["available"], false);
        assert_eq!(status["reason"], "computer_control_engine_unavailable");
        assert_eq!(
            status["permissions"],
            json!(lingxi_service::computer_use_settings::system_permissions())
        );
        assert_eq!(status["permissionSubject"], "service_process");
    }
    assert_eq!(
        h.request(
            "PUT",
            "/lingxi/v1/preferences/computer-use",
            json!({"settings":{"enabled":true}}),
            true
        )
        .await
        .0,
        403
    );
    assert_eq!(
        h.request(
            "POST",
            "/lingxi/v1/preferences/computer-use/request-permissions",
            json!({}),
            true
        )
        .await
        .0,
        403
    );
    assert_eq!(
        h.request(
            "POST",
            "/lingxi/v1/preferences/computer-use/request-permissions",
            json!({"providerId":"neighbor"}),
            false
        )
        .await
        .0,
        400
    );
    let persisted_before = std::fs::read(h.home.join("user/preferences.json")).unwrap();
    assert_eq!(
        h.request(
            "PUT",
            "/lingxi/v1/preferences/computer-use",
            json!({"settings":{"enabled":true,"app_approvals":[{"appId":"bad"}]}}),
            false
        )
        .await
        .0,
        400
    );
    assert_eq!(
        std::fs::read(h.home.join("user/preferences.json")).unwrap(),
        persisted_before
    );
    #[cfg(any(target_os = "macos", windows))]
    {
        assert_eq!(
            h.request(
                "PUT",
                "/lingxi/v1/preferences/computer-use",
                json!({"settings":{"enabled":true}}),
                false
            )
            .await
            .0,
            200
        );
        let (_, enabled) = h
            .request(
                "GET",
                "/lingxi/v1/preferences/computer-use",
                Value::Null,
                false,
            )
            .await;
        assert_eq!(enabled["settings"]["enabled"], true);
        assert_eq!(
            enabled["status"]["providers"][0]["status"]["available"],
            false
        );
    }
    let (s, snapshot) = h
        .request(
            "GET",
            "/lingxi/v1/settings/snapshot?agentId=agentA",
            Value::Null,
            false,
        )
        .await;
    assert_eq!(s, 200, "{snapshot}");
    assert!(snapshot["preferences"]["experiments"].is_array());
    assert!(snapshot["preferences"]["computerUse"].is_object());
    #[cfg(any(target_os = "macos", windows))]
    {
        let approval =
            json!({"providerId":"macos:cua","appId":"fixture.app","appName":"已有批准应用"});
        let route = "/lingxi/v1/preferences/computer-use/approvals";
        assert_eq!(
            h.request("POST", route, approval.clone(), true).await.0,
            403
        );
        let (status, added) = h.request("POST", route, approval.clone(), false).await;
        assert_eq!(status, 200);
        assert_eq!(
            added["settings"]["app_approvals"][0]["appName"],
            "已有批准应用"
        );
        let (status, removed) = h.request("DELETE", route, approval, false).await;
        assert_eq!(status, 200);
        assert_eq!(removed["settings"]["app_approvals"], json!([]));
    }
    // 损坏偏好必须失败，禁止假称空列表加载成功或发布成功通知。
    let cursor = h.state.app_events().since(0)["latestSequence"]
        .as_u64()
        .unwrap();
    std::fs::write(h.home.join("user/preferences.json"), "{").unwrap();
    assert_eq!(
        h.request(
            "PUT",
            "/lingxi/v1/preferences/computer-use",
            json!({"settings":{"enabled":false}}),
            false
        )
        .await
        .0,
        500
    );
    assert_eq!(
        h.request("PATCH", EXP, json!({"value":false}), false)
            .await
            .0,
        500
    );
    assert_eq!(h.state.app_events().since(cursor)["events"], json!([]));
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_settings_container_experiments_computer_components_and_current_live_http() {
    let h = Harness::boot().await;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    assert_eq!(
        h.request(
            "POST",
            "/lingxi/v1/preferences/computer-use/approvals",
            json!({"providerId":"macos:cua","appId":"fixture.app","appName":"已有批准应用"}),
            false
        )
        .await
        .0,
        200
    );
    let proof = h.home.join("settings-ui-evidence");
    std::fs::create_dir_all(&proof).unwrap();
    let output = tokio::process::Command::new("node")
        .arg(repo.join("node_modules/vitest/vitest.mjs"))
        .args([
            "run",
            "desktop/src/react/settings/__tests__/R02SharingLeaf.test.tsx",
            "desktop/src/react/settings/tabs/__tests__/ExperimentsTab.test.tsx",
            "desktop/src/react/settings/tabs/__tests__/ComputerUseSection.test.tsx",
            "desktop/src/react/settings/__tests__/R06ExperimentsRustLive.test.tsx",
        ])
        .current_dir(&repo)
        .env("R06_EXPERIMENT_URL", &h.url)
        .env("R06_EXPERIMENT_TOKEN", &h.token)
        .env("R06_EXPERIMENT_READONLY", &h.readonly)
        .env("R06_EXPERIMENT_RECEIPT", proof.join("receipt.json"))
        .output()
        .await
        .unwrap();
    std::fs::write(proof.join("stdout.log"), &output.stdout).unwrap();
    std::fs::write(proof.join("stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "原页面/当前服务失败 {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(proof.join("receipt.json")).unwrap()).unwrap();
    assert!(receipt["checks"].as_array().unwrap().len() >= 5);
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_experiments_current_tauri_window_real_service() {
    let h = Harness::boot().await;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    assert_eq!(
        h.request(
            "POST",
            "/lingxi/v1/preferences/computer-use/approvals",
            json!({"providerId":"macos:cua","appId":"fixture.app","appName":"已有批准应用"}),
            false
        )
        .await
        .0,
        200
    );
    let proof = h.home.join("native-ui-evidence");
    // 原生构建缓存位置与 directory_picker/skills_completion 同一环境合同
    // （LINGXI_SKILLS_NATIVE_TARGET 优先），硬编码默认值仅作历史回退。
    let native_target = std::env::var_os("LINGXI_SKILLS_NATIVE_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("/Users/study_superior/.cache/lingxi-r06-builds/tauri-resources")
        });
    assert!(native_target.is_dir(), "需要已保留的原生构建缓存；此证明不创建新target");
    let output = tokio::process::Command::new("node")
        .arg(repo.join("desktop/src/react/settings/__tests__/run-experiments-live-ui.mjs"))
        .current_dir(&repo)
        .env("R06_EXPERIMENT_URL", &h.url)
        .env("R06_EXPERIMENT_TOKEN", &h.token)
        .env("R06_EXPERIMENT_READONLY", &h.readonly)
        .env("R06_EXPERIMENT_PROOF", &proof)
        .env("R06_EXPERIMENT_NATIVE_TARGET", &native_target)
        .output()
        .await
        .unwrap();
    std::fs::create_dir_all(&proof).unwrap();
    std::fs::write(proof.join("parent-stdout.log"), &output.stdout).unwrap();
    std::fs::write(proof.join("parent-stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "真实原生页失败 {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(proof.join("ui-receipt.json")).unwrap()).unwrap();
    assert!(receipt["checks"].as_array().unwrap().len() >= 5);
    h.close().await;
}
#[test]
fn actual_cli_focused_creation_and_frozen_hint_reach_wire_model_after_toggle_off() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let proof = std::env::temp_dir().join(format!(
        "lingxi-experiment-focused-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    let output = std::process::Command::new("python3")
        .arg(repo.join("tests/rust-native/experiments-focus-live.py"))
        .current_dir(&repo)
        .env("R06_SESSION_EVIDENCE", &proof)
        .env(
            "R06_SESSION_SERVICE_BIN",
            env!("CARGO_BIN_EXE_lingxi-service"),
        )
        .env("R06_SESSION_UI", "0")
        .output()
        .unwrap();
    std::fs::create_dir_all(&proof).unwrap();
    std::fs::write(proof.join("parent-stdout.log"), &output.stdout).unwrap();
    std::fs::write(proof.join("parent-stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "真实CLI/模型请求失败 {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let v: Value =
        serde_json::from_slice(&std::fs::read(proof.join("result.json")).unwrap()).unwrap();
    assert_eq!(v["status"], "PASS");
    assert_eq!(v["persistedModelCalls"], 9);
    println!("EXPERIMENT_FOCUSED_PROOF {}", proof.display());
}
