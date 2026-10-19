//! 只替外部浏览器和模型响应；正式任务、审批、调用记录、浏览器回执及会话文件均用真实服务。
use lingxi_kernel::{
    browser::{BrowserError, BrowserFuture, BrowserOutput, BrowserPort, BrowserScope},
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
    RunContext,
};
use lingxi_protocol::{
    BrowserAction, BrowserCommand, BrowserReply, BrowserSettings, BrowserWorkspace, ContentBlock,
    ModelCallId, NormalizedMessage, RunId,
};
use lingxi_service::{
    approval_service::{Answer, ApprovalService},
    inject::ManualClock,
    prepare_layout,
    toolgateway::ToolInvocationGateway,
    ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const PIXELS: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
fn pixels() -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(PIXELS)
        .unwrap()
}
const NOW: u64 = 1_800_000_000_000;
#[derive(Default)]
struct Browser {
    pages: Mutex<BTreeMap<String, BrowserWorkspace>>,
    calls: Mutex<Vec<(BrowserScope, BrowserCommand)>>,
    unknown: AtomicBool,
    revoke_unknown: AtomicBool,
    wait_for_cancel: AtomicBool,
    cleanup_confirmed: AtomicBool,
    echo: Mutex<Option<String>>,
    revoke_gate: Mutex<Option<Arc<RevokeGate>>>,
    revoke_count: AtomicUsize,
}
struct RevokeGate {
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}
impl Browser {
    fn seed(&self, sid: &str, node: &str) {
        let state:BrowserWorkspace=serde_json::from_value(json!({"sessionId":sid,"sessionPath":format!("native:{sid}"),"executionNode":node,"running":true,"resumable":false,"unavailableReason":null,"url":"https://example.org/old","title":"测试页","activeTabId":format!("tab-{sid}"),"tabs":[{"tabId":format!("tab-{sid}"),"title":"测试页","url":"https://example.org/old","canGoBack":false,"canGoForward":false,"createdAt":"2027-01-15T00:00:00Z","updatedAt":"2027-01-15T00:00:00Z"}],"suspended":false,"userTakeover":false,"revoked":false,"visibleCapable":true})).unwrap();
        self.pages.lock().unwrap().insert(sid.into(), state);
    }
    fn move_page(&self, sid: &str) {
        let mut pages = self.pages.lock().unwrap();
        let state = pages.get_mut(sid).unwrap();
        state.url = Some("https://example.org/changed".into());
        state.tabs[0].url = state.url.clone();
    }
}
impl BrowserPort for Browser {
    fn execute<'a>(
        &'a self,
        scope: BrowserScope,
        command: BrowserCommand,
        cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, BrowserOutput> {
        Box::pin(async move {
            assert!(!cancel.load(Ordering::Acquire));
            assert!(!scope.human, "模型工具不能冒充用户接管");
            self.calls
                .lock()
                .unwrap()
                .push((scope.clone(), command.clone()));
            if self.wait_for_cancel.load(Ordering::Acquire) {
                while !cancel.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                self.cleanup_confirmed.store(true, Ordering::Release);
                return Err(BrowserError {
                    code: "BROWSER_CANCELLED".into(),
                    message: "外部操作观察到取消并确认清理".into(),
                    effect_unknown: false,
                });
            }
            if self.unknown.load(Ordering::Acquire) {
                return Err(BrowserError {
                    code: "BROWSER_EFFECT_UNKNOWN".into(),
                    message: "外部浏览器断线，无法确认点击结果".into(),
                    effect_unknown: true,
                });
            }
            let mut pages = self.pages.lock().unwrap();
            let state = pages.get_mut(&scope.session_id).unwrap();
            assert_eq!(state.execution_node, scope.execution_node);
            if command.action == BrowserAction::Start {
                state.running = true;
                state.revoked = false;
            }
            if command.action == BrowserAction::Navigate {
                state.running = true;
                state.revoked = false;
                state.url = command.url.clone();
                if state.tabs.is_empty() {
                    let tab = serde_json::from_value(json!({"tabId":format!("tab-{}",scope.session_id),"title":"新测试页","url":command.url,"canGoBack":false,"canGoForward":false,"createdAt":"2027-01-15T00:00:00Z","updatedAt":"2027-01-15T00:00:00Z"})).unwrap();
                    state.tabs.push(tab);
                    state.active_tab_id = Some(format!("tab-{}", scope.session_id));
                } else {
                    state.tabs[0].url = command.url.clone();
                }
            }
            Ok(BrowserOutput {
                reply: BrowserReply {
                    state: state.clone(),
                    result: json!({"text":self.echo.lock().unwrap().clone().unwrap_or_else(|| "真实外部页结果".into()),"refs":[{"ref":1,"text":"按钮"}]}),
                    files: vec![],
                    downloads: vec![],
                },
                screenshot: if command.action == BrowserAction::Screenshot {
                    Some(pixels())
                } else {
                    None
                },
                downloads: vec![],
            })
        })
    }
    fn workspaces(&self) -> Result<Vec<BrowserWorkspace>, BrowserError> {
        Ok(self.pages.lock().unwrap().values().cloned().collect())
    }
    fn restore(&self, _scope: BrowserScope, state: BrowserWorkspace) -> Result<(), BrowserError> {
        self.pages
            .lock()
            .unwrap()
            .insert(state.session_id.clone(), state);
        Ok(())
    }
    fn revoke<'a>(&'a self, scope: BrowserScope) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            self.revoke_count.fetch_add(1, Ordering::AcqRel);
            let gate = self.revoke_gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.entered.add_permits(1);
                gate.release.acquire().await.unwrap().forget();
            }
            if self.revoke_unknown.load(Ordering::Acquire) {
                return Err(BrowserError {
                    code: "BROWSER_CLEANUP_UNKNOWN".into(),
                    message: "外部进程未确认退出".into(),
                    effect_unknown: true,
                });
            }
            if let Some(state) = self.pages.lock().unwrap().get_mut(&scope.session_id) {
                state.revoked = true;
                state.running = false;
                state.resumable = false;
                state.tabs.clear();
                state.active_tab_id = None;
                state.url = None;
                state.title = None;
            }
            Ok(())
        })
    }
    fn configure<'a>(&'a self, _settings: BrowserSettings) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn clear_site_data<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn read_auxiliary_page<'a>(
        &'a self,
        _url: String,
        _expression: String,
        _locale: Option<String>,
        _cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, Value> {
        Box::pin(async { unreachable!("聊天工具不能调用搜索临时页") })
    }
    fn reap_idle<'a>(&'a self, _now: u64) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn shutdown<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
#[derive(Default)]
struct Model {
    results: Mutex<BTreeMap<String, ToolOutcome>>,
    retries: Mutex<BTreeMap<String, Value>>,
    observed: Mutex<BTreeMap<String, (u32, String)>>,
    materialize: Mutex<BTreeMap<String, Value>>,
    echo: Mutex<Option<String>>,
}
impl TurnProviderPort for Model {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "fixture".into(),
            model: "browser-tools".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _sink: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(async move {
            self.observed.lock().unwrap().insert(
                ctx.run_id.to_string(),
                (input.turn, input.submission.clone()),
            );
            if input.turn == 1 {
                let body: Value = serde_json::from_str(
                    input
                        .submission
                        .split("\n\n[steering]\n")
                        .next()
                        .unwrap_or(&input.submission)
                        .rsplit("\n\n")
                        .next()
                        .unwrap_or(&input.submission),
                )
                .unwrap();
                let args = body.get("args").unwrap_or(&body).clone();
                if body["tool"] == "materialize" && body["raw"] != true {
                    self.materialize
                        .lock()
                        .unwrap()
                        .insert(ctx.run_id.to_string(), args);
                    return ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: vec![ToolRequest::from_effective_arguments(
                                "tool:first-party:mcp_describe_tool",
                                json!({"name":"materialize"}),
                                &SchemaBudget::default(),
                            )
                            .unwrap()],
                            content: vec![],
                        },
                    );
                }
                if body["retryOnce"] == true {
                    self.retries
                        .lock()
                        .unwrap()
                        .insert(ctx.run_id.to_string(), args.clone());
                }
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            format!(
                                "tool:first-party:{}",
                                body["tool"].as_str().unwrap_or("browser")
                            ),
                            args,
                            &SchemaBudget::default(),
                        )
                        .unwrap()],
                        content: vec![],
                    },
                );
            }
            if let Some(result) = input.prior.iter().rev().find_map(|item| match item {
                ExchangeItem::ToolResult { outcome, .. } => Some(outcome.clone()),
                _ => None,
            }) {
                let materialize = self.materialize.lock().unwrap().remove(ctx.run_id.as_str());
                if let Some(args) = materialize {
                    assert!(
                        matches!(result, ToolOutcome::Success { .. }),
                        "必须先取得真实materialize说明：{result:?}"
                    );
                    return ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: vec![ToolRequest::from_effective_arguments(
                                "tool:first-party:mcp_call",
                                json!({"tool":"materialize","arguments":args}),
                                &SchemaBudget::default(),
                            )
                            .unwrap()],
                            content: vec![],
                        },
                    );
                }
                self.results
                    .lock()
                    .unwrap()
                    .insert(ctx.run_id.to_string(), result);
            }
            if let Some(args) = self.retries.lock().unwrap().remove(ctx.run_id.as_str()) {
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            "tool:first-party:browser",
                            args,
                            &SchemaBudget::default(),
                        )
                        .unwrap()],
                        content: vec![],
                    },
                );
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: self
                                .echo
                                .lock()
                                .unwrap()
                                .clone()
                                .unwrap_or_else(|| "本地浏览器验证完成".into()),
                        }],
                        model_call_id: Some(call.clone()),
                    },
                },
            )
        })
    }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    browser: Arc<Browser>,
    model: Arc<Model>,
    approvals: Arc<ApprovalService>,
}
impl Harness {
    async fn boot(mode: &str) -> Self {
        Self::boot_with_vision(mode, None).await
    }
    async fn boot_with_vision(mode: &str, endpoint: Option<String>) -> Self {
        Self::boot_at(mode, endpoint, None).await
    }
    async fn boot_at(mode: &str, endpoint: Option<String>, existing: Option<PathBuf>) -> Self {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_test_writer()
            .try_init();
        let fresh = existing.is_none();
        let home = existing.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-r07-browser-tools-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let model = Arc::new(Model::default());
        let browser = Arc::new(Browser::default());
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            approvals.clone(),
            clock.clone(),
            SchemaBudget::default(),
            300000,
            1000,
        ));
        // 外部模型响应虽用替身，目录仍显式采用生产正式说明与调度入口。
        lingxi_service::toolcatalog_bridge::register(
            gateway.registry(),
            &gateway,
            &SchemaBudget::default(),
        );
        let (model_gateway, credential_service) = if let Some(endpoint) = endpoint {
            let plane=lingxi_adapters::models::config::ModelPlaneConfig::from_value(json!({"providers":{"vision-fixture":{"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-vision-only"}}},"models":{"vision":{"provider":"vision-fixture","model":"vision","capabilities":{"imageInput":true},"compat":{"contextWindow":200000}}}})).unwrap();
            let credentials = Arc::new(
                lingxi_service::credentials::CredentialService::bootstrap(
                    &plane,
                    &layout.runtime_dir,
                    clock.clone(),
                )
                .unwrap(),
            );
            (
                Some(Arc::new(
                    lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
                )),
                Some(credentials),
            )
        } else {
            (None, None)
        };
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                assemble_session_file_tools: true,
                clock,
                turn_provider: Some(model.clone()),
                tool_gateway: Some(gateway.clone()),
                approval_gate: Some(approvals.clone()),
                browser_port: Some(browser.clone()),
                model_gateway,
                credential_service,
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        // 使用正式无workspace分支，不给缓存目录授权，也不手工替换read执行器。
        for name in ["read", "materialize"] {
            assert!(gateway
                .registry()
                .describe(
                    &lingxi_kernel::toolcatalog::tool_target_id(
                        &lingxi_kernel::toolcatalog::ToolOrigin::FirstParty,
                        name
                    )
                    .unwrap()
                )
                .is_ok());
        }
        for name in ["write", "edit", "exec_command"] {
            assert!(gateway
                .registry()
                .describe(
                    &lingxi_kernel::toolcatalog::tool_target_id(
                        &lingxi_kernel::toolcatalog::ToolOrigin::FirstParty,
                        name
                    )
                    .unwrap()
                )
                .is_err());
        }
        for sid in ["browser-a", "browser-other"] {
            if lingxi_adapters::storage::session_tree::get_session_row(state.storage(), sid)
                .await
                .unwrap()
                .is_none()
            {
                state
                    .session_admin()
                    .create_session(
                        sid,
                        "a",
                        lingxi_kernel::LOCAL_OWNER_SUBJECT,
                        "浏览器验证",
                        Some(mode),
                        &[],
                        false,
                        NOW,
                    )
                    .await
                    .unwrap();
            }
        }
        let principal = state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", state.auth().local_token())),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        for sid in ["browser-a", "browser-other"] {
            let row = state.browsers().authorize(&principal, sid).await.unwrap();
            if fresh {
                browser.seed(sid, &row.execution_node);
            }
        }
        Self {
            home,
            state,
            browser,
            model,
            approvals,
        }
    }
    async fn submit(&self, args: Value) -> String {
        self.submit_for("browser-a", args).await
    }
    async fn submit_for(&self, session: &str, args: Value) -> String {
        let principal = self
            .state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", self.state.auth().local_token())),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        self.state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &principal,
                session,
                &ExecuteSubmission::plain(&args.to_string()),
                NOW,
            )
            .await
            .unwrap()
            .run_id
    }
    async fn tool(&self, session: &str, name: &str, args: Value) -> ToolOutcome {
        let run = self
            .submit_for(session, json!({"tool":name,"args":args}))
            .await;
        self.wait(&run).await
    }
    async fn wait(&self, run: &str) -> ToolOutcome {
        let waited = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if self
                    .state
                    .storage()
                    .load_run(&RunId::new(run))
                    .await
                    .unwrap()
                    .unwrap()
                    .status
                    .is_terminal()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        assert!(
            waited.is_ok(),
            "真实浏览器Run必须终结；外部模型实际输入：{:?}；实际子任务：{:?}；后台退出：{:?}",
            self.model.observed.lock().unwrap().get(run),
            self.state.runs().task_supervisor().tasks(),
            self.state.background().recent_exits()
        );
        self.model
            .results
            .lock()
            .unwrap()
            .remove(run)
            .expect("模型必须收到正式工具结果")
    }
    async fn pending(&self, run: &str) -> String {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(p) = self
                    .approvals
                    .pending_for_principal(&lingxi_kernel::Principal::LocalUser, "browser-a")
                    .into_iter()
                    .find(|p| p.run_id == run)
                {
                    return p.approval_id;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("浏览器写操作必须进入真实确认")
    }
    fn approve(&self, id: &str) {
        assert!(self
            .approvals
            .answer_for_principal(
                &lingxi_kernel::Principal::LocalUser,
                "browser-a",
                id,
                Answer::Approve
            )
            .settled());
    }
    async fn receipt(&self) -> Option<String> {
        self.state
            .storage()
            .query_one_text(
                "SELECT status FROM browser_commands_v1 ORDER BY rowid DESC LIMIT 1",
                vec![],
            )
            .await
            .unwrap()
    }
    async fn shutdown_preserve(self) -> PathBuf {
        let home = self.home.clone();
        let report = self
            .state
            .background()
            .drain_within(Duration::from_secs(5))
            .await;
        assert!(report.unconfirmed.is_empty());
        self.state.browsers().stop();
        self.state.browsers().wait_stopped().await.unwrap();
        self.state.browsers().shutdown().await.unwrap();
        self.state.storage().close().await.unwrap();
        drop(self.state);
        home
    }
    async fn close(self) {
        let home = self.shutdown_preserve().await;
        let _ = std::fs::remove_dir_all(home);
    }
}
fn success(outcome: ToolOutcome) -> Value {
    match outcome {
        ToolOutcome::Success { result } => {
            Value::Object(result.details.expect("正式浏览器工具须返回实际状态"))
        }
        other => panic!("预期成功，实际 {other:?}"),
    }
}
#[tokio::test]
async fn actual_readonly_snapshot_persists_receipt_but_write_never_dispatches() {
    let h = Harness::boot("read_only").await;
    let run = h.submit(json!({"action":"snapshot"})).await;
    let details = success(h.wait(&run).await);
    assert_eq!(details["activeTabId"], "tab-browser-a");
    assert_eq!(details["result"]["text"], "真实外部页结果");
    assert_eq!(h.receipt().await.as_deref(), Some("confirmed"));
    assert_eq!(h.browser.calls.lock().unwrap().len(), 1);
    let journal = h
        .state
        .storage()
        .query_one_text(
            "SELECT target FROM invocation_journal WHERE run_id=?1 AND phase='succeeded' AND dispatched=1",
            vec![run],
        )
        .await
        .unwrap();
    assert_eq!(journal.as_deref(), Some("tool:first-party:browser"));
    let run = h
        .submit(json!({"action":"evaluate","tabId":"tab-browser-a","expression":"document.title"}))
        .await;
    assert!(matches!(h.wait(&run).await, ToolOutcome::Failed { .. }));
    assert_eq!(h.browser.calls.lock().unwrap().len(), 1);
    h.close().await;
}
#[tokio::test]
async fn actual_navigation_waits_for_approval_and_dispatches_exact_reviewed_url() {
    let h = Harness::boot("operate").await;
    let run = h
        .submit(json!({"action":"navigate","url":"https://example.org/exact?q=1"}))
        .await;
    let approval = h.pending(&run).await;
    assert!(h.browser.calls.lock().unwrap().is_empty());
    assert!(h.receipt().await.is_none());
    h.approve(&approval);
    let details = success(h.wait(&run).await);
    assert_eq!(details["url"], "https://example.org/exact?q=1");
    {
        let calls = h.browser.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].1.url.as_deref(),
            Some("https://example.org/exact?q=1")
        );
        assert_eq!(calls[0].0.session_id, "browser-a");
    }
    assert_eq!(h.receipt().await.as_deref(), Some("confirmed"));
    h.close().await;
}
#[tokio::test]
async fn page_change_and_foreign_tab_invalidate_approval_before_external_dispatch() {
    let h = Harness::boot("operate").await;
    let run = h
        .submit(json!({"action":"click","tabId":"tab-browser-a","ref":1}))
        .await;
    let approval = h.pending(&run).await;
    h.browser.move_page("browser-a");
    h.approve(&approval);
    assert!(matches!(h.wait(&run).await, ToolOutcome::Failed { .. }));
    assert!(h.browser.calls.lock().unwrap().is_empty());
    assert!(h.receipt().await.is_none());
    let run = h
        .submit(json!({"action":"click","tabId":"tab-browser-other","ref":1}))
        .await;
    assert!(matches!(h.wait(&run).await, ToolOutcome::Failed { .. }));
    assert!(h.browser.calls.lock().unwrap().is_empty());
    h.close().await;
}
#[tokio::test]
async fn external_unknown_effect_is_persisted_and_never_reported_as_success() {
    let h = Harness::boot("operate").await;
    h.browser.unknown.store(true, Ordering::Release);
    let run = h
        .submit(json!({"action":"click","tabId":"tab-browser-a","ref":1}))
        .await;
    let approval = h.pending(&run).await;
    h.approve(&approval);
    match h.wait(&run).await {
        ToolOutcome::Failed { error } => assert_eq!(
            serde_json::to_value(error).unwrap()["details"]["effectUnknown"],
            true
        ),
        other => panic!("无法确认的外部点击不能变为成功: {other:?}"),
    };
    assert_eq!(h.receipt().await.as_deref(), Some("unknown"));
    assert_eq!(h.browser.calls.lock().unwrap().len(), 1);
    h.close().await;
}

#[tokio::test]
async fn actual_run_cancellation_reaches_external_cleanup_and_settles_both_records() {
    let h = Harness::boot("read_only").await;
    h.browser.wait_for_cancel.store(true, Ordering::Release);
    let run = h.submit(json!({"action":"snapshot"})).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while h.browser.calls.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("必须先发生真实外部派发");
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", h.state.auth().local_token())),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    h.state
        .sessions()
        .cancel_run_for(h.state.storage().as_ref(), h.state.runs(), &principal, &run)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let record = h
                .state
                .storage()
                .load_run(&RunId::new(&run))
                .await
                .unwrap()
                .unwrap();
            if record.status.is_terminal() {
                assert_eq!(record.status, lingxi_protocol::RunStatus::Cancelled);
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("统一取消必须使真实Run终结");
    assert!(
        h.browser.cleanup_confirmed.load(Ordering::Acquire),
        "不能只停止等待，必须收到外部清理确认"
    );
    assert_eq!(
        h.receipt().await.as_deref(),
        Some("failed"),
        "已确认取消必须持久结束命令，不能遗留started"
    );
    assert_eq!(h.browser.calls.lock().unwrap().len(), 1);
    h.close().await;
}

#[tokio::test]
async fn new_actual_user_submission_releases_emergency_stop_with_a_new_epoch() {
    let h = Harness::boot("operate").await;
    let first = h
        .submit(json!({"action":"navigate","url":"https://example.org/first"}))
        .await;
    h.approve(&h.pending(&first).await);
    success(h.wait(&first).await);
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", h.state.auth().local_token())),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    let old = h
        .state
        .browsers()
        .authorize(&principal, "browser-a")
        .await
        .unwrap();
    h.state
        .browsers()
        .close(old.clone(), true, "true-user-emergency".into())
        .await
        .unwrap();
    let revoked = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(revoked.revoked);
    assert!(revoked.epoch > old.epoch);
    let next = h
        .submit(json!({"action":"navigate","url":"https://example.org/new-user"}))
        .await;
    let approval = h.pending(&next).await;
    assert_eq!(
        h.browser.calls.lock().unwrap().len(),
        1,
        "新任务仍须等本次确认"
    );
    let released = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(!released.revoked, "只有真正受理的新用户Run才能解除急停");
    assert!(released.epoch > revoked.epoch);
    h.approve(&approval);
    let result = success(h.wait(&next).await);
    assert_eq!(result["url"], "https://example.org/new-user");
    {
        let calls = h.browser.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].0.epoch, released.epoch);
        assert!(calls[1].0.epoch > calls[0].0.epoch);
    }
    h.close().await;
}

async fn ws_send(stream: &mut tokio::net::TcpStream, value: Value) {
    use tokio::io::AsyncWriteExt;
    let bytes = value.to_string().into_bytes();
    let mask = [2, 4, 6, 8];
    let mut frame = vec![0x81];
    if bytes.len() < 126 {
        frame.push(0x80 | u8::try_from(bytes.len()).unwrap());
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&u16::try_from(bytes.len()).unwrap().to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(bytes.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame).await.unwrap();
}
async fn ws_json(stream: &mut tokio::net::TcpStream) -> Value {
    match tokio::time::timeout(
        Duration::from_secs(10),
        lingxi_service::ws::read_ws_frame(stream),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap()
    {
        lingxi_service::ws::WsFrame::Text(bytes) => serde_json::from_slice(&bytes).unwrap(),
        other => panic!("非结果帧: {other:?}"),
    }
}
async fn ws_open(addr: std::net::SocketAddr, token: &str) -> tokio::net::TcpStream {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(format!("GET /lingxi/v1/ws HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {token}\r\n\r\n").as_bytes()).await.unwrap();
    let mut header = vec![];
    let mut b = [0];
    while !header.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut b).await.unwrap();
        header.push(b[0]);
    }
    assert!(
        header.starts_with(b"HTTP/1.1 101"),
        "实际升级响应：{}",
        String::from_utf8_lossy(&header)
    );
    ws_send(&mut stream,json!({"protocol":"lingxi.wire","clientKind":"test","clientVersion":"0","protocolMin":1,"protocolMax":1})).await;
    assert_eq!(ws_json(&mut stream).await["protocol"], "lingxi.wire");
    stream
}
async fn steered(stream: &mut tokio::net::TcpStream, request: Value) -> Value {
    ws_send(stream, request).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = ws_json(stream).await;
            if frame["type"] == "steered" {
                return frame;
            }
            assert!(frame["type"] != "error", "插话失败: {frame}");
        }
    })
    .await
    .expect("必须收到实际插话受理回执")
}
async fn http_emergency(h: &Harness, addr: std::net::SocketAddr, id: &str) {
    let result = reqwest::Client::new()
        .post(format!("http://{addr}/lingxi/v1/browser/close-session"))
        .bearer_auth(h.state.auth().local_token())
        .json(&json!({"sessionPath":"native:browser-a","revoke":true,"requestId":id}))
        .send()
        .await
        .unwrap();
    let status = result.status();
    let body: Value = result.json().await.unwrap();
    assert!(status.is_success(), "{body}");
    assert_eq!(body["ok"], true);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accepted_real_websocket_steer_reauthorizes_only_current_run_and_old_request_never_lifts_later_stop(
) {
    let h = Harness::boot("operate").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = lingxi_service::build_router(h.state.clone());
    let app = tokio::spawn(async move {
        lingxi_service::serve::serve_with_connection_admission(
            listener,
            router,
            std::future::pending(),
            Arc::new(lingxi_service::limits::ConnectionAdmission::new(16)),
            Duration::from_secs(5),
            None,
        )
        .await
        .unwrap();
    });
    let mut ws = ws_open(addr, &h.state.auth().local_token()).await;
    let run=h.submit(json!({"retryOnce":true,"args":{"action":"navigate","url":"https://example.org/steered"}})).await;
    let old_approval = h.pending(&run).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if h.state
                .storage()
                .load_run(&RunId::new(&run))
                .await
                .unwrap()
                .unwrap()
                .status
                == lingxi_protocol::RunStatus::WaitingApproval
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("插话必须覆盖真实等待审批状态");
    http_emergency(&h, addr, "stop-before-steer").await;
    let revoked = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(revoked.revoked);
    let steer = json!({"type":"steer","sessionId":"browser-a","sessionPath":"native:browser-a","text":"请继续打开刚才的页面","requestId":"same-human-steer"});
    let frame = steered(&mut ws, steer.clone()).await;
    assert_ne!(frame["replayed"], true);
    let granted = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(!granted.revoked);
    assert!(granted.epoch > revoked.epoch);
    let receipt = h.state.storage().query_one_text("SELECT result_json FROM browser_commands_v1 WHERE session_id='browser-a' AND status='confirmed' AND json_extract(input_json,'$.action')='human_steer'", vec![]).await.unwrap().unwrap();
    let receipt: Value = serde_json::from_str(&receipt).unwrap();
    assert_eq!(receipt["runId"], run);
    assert_eq!(receipt["authorityGranted"], true);
    assert_eq!(receipt["epoch"], granted.epoch);
    h.approve(&old_approval);
    let fresh_approval = h.pending(&run).await;
    assert_ne!(fresh_approval, old_approval);
    assert!(
        h.browser.calls.lock().unwrap().is_empty(),
        "急停前的旧审批不能复活"
    );
    h.approve(&fresh_approval);
    let result = success(h.wait(&run).await);
    assert_eq!(result["url"], "https://example.org/steered");
    assert_eq!(
        h.browser.calls.lock().unwrap().len(),
        1,
        "旧真实Run只有新的确认才可继续"
    );
    let count=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM invocation_journal WHERE run_id=?1 AND dispatched=0 AND phase='failed'",vec![run.clone()]).await.unwrap();
    assert_eq!(count.as_deref(), Some("1"));
    http_emergency(&h, addr, "later-stop").await;
    let later = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(later.revoked);
    assert_eq!(steered(&mut ws, steer).await["replayed"], true);
    let unchanged = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(unchanged.revoked);
    assert_eq!(unchanged.epoch, later.epoch);
    assert_eq!(
        h.state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE session_id='browser-a'",
                vec![]
            )
            .await
            .unwrap()
            .as_deref(),
        Some("1"),
        "旧插话回放不能转成新用户任务"
    );
    let new_run = h
        .submit(json!({"action":"navigate","url":"https://example.org/new-after-steer"}))
        .await;
    h.approve(&h.pending(&new_run).await);
    success(h.wait(&new_run).await);
    assert_eq!(h.browser.calls.lock().unwrap().len(), 2);
    drop(ws);
    app.abort();
    let _ = app.await;
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn screenshot_owned_file_materializes_into_exact_read_pixels_without_granting_other_sessions()
{
    use axum::{extract::State, http::header, routing::post, Json, Router};
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    async fn vision(
        State(requests): State<Arc<Mutex<Vec<Value>>>>,
        Json(request): Json<Value>,
    ) -> ([(header::HeaderName, &'static str); 1], String) {
        requests.lock().unwrap().push(request);
        let result = json!({"id":"browser-vision","choices":[{"index":0,"delta":{"role":"assistant","content":"IMAGE-PIXELS-REACHED-REAL-VISION"},"finish_reason":"stop"}]});
        let usage = json!({"id":"browser-vision","choices":[],"usage":{"prompt_tokens":13,"completion_tokens":7}});
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            format!("data: {result}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/v1/chat/completions", post(vision))
        .with_state(requests.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let h = Harness::boot_with_vision("read_only", Some(format!("http://{addr}/v1"))).await;
    let run = h.submit(json!({"action":"screenshot"})).await;
    let result = success(h.wait(&run).await);
    let file = &result["files"][0];
    let file_id = file["fileId"].clone();
    let path = file["filePath"].clone();
    assert_eq!(std::fs::read(path.as_str().unwrap()).unwrap(), pixels());
    assert_eq!(
        file["mimeType"].as_str().or(file["mime"].as_str()),
        Some("image/png")
    );
    assert!(matches!(
        h.tool("browser-a", "read", json!({"path":path})).await,
        ToolOutcome::Failed { .. }
    ));
    assert!(matches!(
        h.tool("browser-other", "materialize", json!({"fileId":file_id}))
            .await,
        ToolOutcome::Failed { .. }
    ));
    let materialized = success(
        h.tool("browser-a", "materialize", json!({"fileId":file_id}))
            .await,
    );
    assert_eq!(materialized["filePath"], path);
    assert!(matches!(
        h.tool("browser-other", "read", json!({"path":path})).await,
        ToolOutcome::Failed { .. }
    ));
    let read = success(h.tool("browser-a", "read", json!({"path":path})).await);
    assert_eq!(read["visionAdapted"], true);
    {
        let received = requests.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert!(
            received[0]["messages"]
                .to_string()
                .contains(&format!("data:image/png;base64,{PIXELS}")),
            "供应商收到的像素必须等于浏览器实际交付文件"
        );
    }
    assert_eq!(h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE purpose='auxiliary.vision' AND outcome='succeeded' AND transport_attempts=1 AND parent_tool_call_id IS NOT NULL",vec![]).await.unwrap().as_deref(),Some("1"));
    server.abort();
    let _ = server.await;
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn confirmed_http_emergency_persists_empty_cold_state_across_restart_and_new_user_start() {
    let h = Harness::boot("operate").await;
    let run = h
        .submit(json!({"action":"navigate","url":"https://example.org/must-not-resurrect"}))
        .await;
    h.approve(&h.pending(&run).await);
    success(h.wait(&run).await);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = lingxi_service::build_router(h.state.clone());
    let app = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    http_emergency(&h, addr, "close-pages-for-real").await;
    let closed = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    let cold = closed.state.unwrap();
    assert!(closed.revoked);
    assert!(!cold.running && !cold.resumable && cold.tabs.is_empty());
    assert!(cold.url.is_none() && cold.active_tab_id.is_none());
    app.abort();
    let _ = app.await;
    let home = h.shutdown_preserve().await;
    let restarted = Harness::boot_at("operate", None, Some(home)).await;
    let before = restarted.browser.workspaces().unwrap();
    let cold = before
        .iter()
        .find(|state| state.session_id == "browser-a")
        .unwrap();
    assert!(
        cold.tabs.is_empty() && cold.url.is_none(),
        "重启不得把已确证关闭的旧URL重建为可恢复页面"
    );
    let run = restarted.submit(json!({"action":"start"})).await;
    let state = success(restarted.wait(&run).await);
    assert_eq!(state["running"], true);
    assert_eq!(state["url"], Value::Null);
    assert_eq!(state["tabs"], json!([]));
    assert_eq!(
        restarted.browser.calls.lock().unwrap().len(),
        1,
        "新用户只启动新空浏览器，没有隐式旧URL导航"
    );
    restarted.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unconfirmed_http_emergency_keeps_last_page_and_live_process_explicitly_unknown() {
    let h = Harness::boot("read_only").await;
    let run = h.submit(json!({"action":"snapshot"})).await;
    success(h.wait(&run).await);
    h.browser.revoke_unknown.store(true, Ordering::Release);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = lingxi_service::build_router(h.state.clone());
    let app = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    let response = client.post(format!("http://{addr}/lingxi/v1/browser/close-session"))
        .bearer_auth(h.state.auth().local_token())
        .json(&json!({"sessionPath":"native:browser-a","revoke":true,"requestId":"unconfirmed-close"}))
        .send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let response: Value = response.json().await.unwrap();
    assert_eq!(response["effectUnknown"], true);
    let row = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(row.revoked);
    let state = row.state.unwrap();
    assert!(state.running && state.revoked && !state.tabs.is_empty());
    assert_eq!(state.url.as_deref(), Some("https://example.org/old"));
    assert!(state
        .unavailable_reason
        .unwrap()
        .starts_with("BROWSER_CLEANUP_UNCONFIRMED"));
    assert_eq!(h.receipt().await.as_deref(), Some("unknown"));
    let response: Value = client
        .get(format!("http://{addr}/lingxi/v1/browser/workspaces"))
        .bearer_auth(h.state.auth().local_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let state = response["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|state| state["sessionId"] == "browser-a")
        .unwrap();
    assert_eq!(state["running"], true, "没有退出回执，不能宣称进程已经关闭");
    assert_eq!(state["revoked"], true);
    assert!(state["unavailableReason"]
        .as_str()
        .unwrap()
        .starts_with("BROWSER_CLEANUP_UNCONFIRMED"));
    app.abort();
    let _ = app.await;
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn undeclared_materialize_is_refused_without_dispatch_and_actual_run_finishes() {
    let h = Harness::boot("read_only").await;
    let run = h
        .submit(json!({"tool":"materialize","raw":true,"args":{"fileId":"sf_0000000000000000"}}))
        .await;
    let result = h.wait(&run).await;
    assert!(
        matches!(result, ToolOutcome::Failed { .. }),
        "模型未取得目录说明必须明确拒绝：{result:?}"
    );
    if let ToolOutcome::Failed { error } = &result {
        assert!(
            error.message.contains("声明"),
            "拒绝必须来自真实发送声明边界：{}",
            error.message
        );
    }
    let actual = h
        .state
        .storage()
        .load_run(&RunId::new(&run))
        .await
        .unwrap()
        .unwrap();
    assert!(actual.status.is_terminal());
    assert_eq!(h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM invocation_journal WHERE run_id=?1 AND dispatched=1", vec![run]).await.unwrap().as_deref(), Some("0"));
    assert!(h.browser.calls.lock().unwrap().is_empty());
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_model_and_tool_echo_foreign_file_marker_without_pinning_or_blocking_fork() {
    let h = Harness::boot("read_only").await;
    let run = h.submit(json!({"action":"screenshot"})).await;
    let file = success(h.wait(&run).await)["files"][0].clone();
    let file_id = file["fileId"].as_str().unwrap();
    assert_eq!(h.state.storage().query_one_text("SELECT CAST(COUNT(*)>0 AS TEXT) FROM session_file_refs WHERE session_id='browser-a' AND file_id=?1", vec![file_id.into()]).await.unwrap().as_deref(), Some("1"), "真实截图仍须登记实际归属引用");
    let marker = format!(
        "引用未取得的文件 [SessionFile] {}",
        json!({"fileId":file_id})
    );
    *h.browser.echo.lock().unwrap() = Some(marker.clone());
    *h.model.echo.lock().unwrap() = Some(marker.clone());
    let other = h
        .submit_for("browser-other", json!({"action":"snapshot"}))
        .await;
    let result = success(h.wait(&other).await);
    assert_eq!(result["result"]["text"], marker);
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", h.state.auth().local_token())),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    h.state
        .session_tree()
        .fork_session(
            &principal,
            "browser-other",
            "browser-marker-fork",
            &format!("{other}-final"),
            Some("foreign-marker-fork"),
            NOW,
        )
        .await
        .unwrap();
    assert_eq!(h.state.storage().query_one_text("SELECT COALESCE(run_id,'unbound') FROM messages WHERE session_id='browser-marker-fork' AND message_id=?1", vec![format!("{other}-final")]).await.unwrap().as_deref(),Some("unbound"));
    h.state
        .session_tree()
        .fork_session(
            &principal,
            "browser-marker-fork",
            "browser-marker-fork-2",
            &format!("{other}-final"),
            Some("foreign-marker-fork-2"),
            NOW,
        )
        .await
        .unwrap();
    for session in [
        "browser-other",
        "browser-marker-fork",
        "browser-marker-fork-2",
    ] {
        assert_eq!(
            h.state
                .storage()
                .query_one_text(
                    "SELECT CAST(COUNT(*) AS TEXT) FROM session_files WHERE owner_session_id=?1",
                    vec![session.into()]
                )
                .await
                .unwrap()
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            h.state
                .storage()
                .query_one_text(
                    "SELECT CAST(COUNT(*) AS TEXT) FROM session_file_refs WHERE session_id=?1",
                    vec![session.into()]
                )
                .await
                .unwrap()
                .as_deref(),
            Some("0")
        );
    }
    let kernel = lingxi_kernel::Principal::LocalUser;
    let history = lingxi_service::model_history::ModelHistory::new(h.state.storage().clone());
    let loaded = history
        .load(&kernel, "a", "browser-marker-fork-2", "")
        .await
        .unwrap();
    assert!(
        !loaded.is_empty(),
        "保留真实正文和工具结果，不能为了文件索引删掉历史"
    );
    let boundary = format!("{other}-final");
    let content = json!({"content":[{"type":"text","text":marker}]}).to_string();
    let ordinary = lingxi_adapters::storage::session_tree::append_message(
        h.state.storage(),
        lingxi_adapters::storage::session_tree::AppendMessage {
            session_id: "browser-other",
            message_id: "ordinary-foreign-marker",
            parent_message_id: Some(&boundary),
            run_id: None,
            role: "user",
            content_json: &content,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: NOW,
        },
    )
    .await;
    assert!(
        matches!(ordinary, Err(lingxi_kernel::ports::StorageError::InvalidRequest{ detail }) if detail.starts_with("session_file_reference_not_owned:")),
        "普通附件入口的归属校验必须保留"
    );
    assert!(std::fs::read(file["filePath"].as_str().unwrap()).is_ok());
    h.close().await;
}

/// 同生产方法的确定性时序检查；完整WS受理/回放另由上面的真实WS用例覆盖。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_service_interleaving_keeps_new_http_emergency_over_earlier_accepted_steer() {
    let h = Harness::boot("operate").await;
    let run = h
        .submit(json!({"action":"navigate","url":"https://example.org/older-intent"}))
        .await;
    h.pending(&run).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if h.state
                .storage()
                .load_run(&RunId::new(&run))
                .await
                .unwrap()
                .unwrap()
                .status
                == lingxi_protocol::RunStatus::WaitingApproval
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = lingxi_service::build_router(h.state.clone());
    let app = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    http_emergency(&h, addr, "initial-stop-for-race").await;
    let revoked = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(revoked.revoked);
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", h.state.auth().local_token())),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    let (captured, operation) = match h
        .state
        .browsers()
        .begin_human_steer(
            &principal,
            "browser-a",
            &run,
            "早先的人类插话",
            Some("older-steer-before-new-stop"),
        )
        .await
        .unwrap()
    {
        lingxi_service::browser::HumanSteerClaim::New { row, operation } => (*row, operation),
        _ => panic!("必须取得真实新插话收据"),
    };
    assert_eq!(captured.epoch, revoked.epoch);
    assert!(matches!(
        h.state
            .sessions()
            .steer_for(&principal, "browser-a", "早先的人类插话")
            .await
            .unwrap(),
        lingxi_service::session_supervisor::SteerOutcome::Accepted
    ));
    let refused = lingxi_adapters::storage::browser::revoke_human_intent(
        h.state.storage(),
        "browser-a",
        lingxi_kernel::LOCAL_OWNER_SUBJECT,
        &operation,
        NOW,
    )
    .await;
    assert!(
        matches!(
            refused,
            Err(lingxi_kernel::ports::StorageError::Conflict { .. })
        ),
        "插话收据不能冒充新人工急停授权"
    );
    let gate = Arc::new(RevokeGate {
        entered: tokio::sync::Semaphore::new(0),
        release: tokio::sync::Semaphore::new(0),
    });
    *h.browser.revoke_gate.lock().unwrap() = Some(gate.clone());
    let token = h.state.auth().local_token();
    let stop = tokio::spawn(async move {
        let response = reqwest::Client::new().post(format!("http://{addr}/lingxi/v1/browser/close-session"))
            .bearer_auth(token).json(&json!({"sessionPath":"native:browser-a","revoke":true,"requestId":"new-stop-wins"})).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert!(status.is_success(), "{body}");
        assert_eq!(body["ok"], true);
    });
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let newer = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(
        newer.revoked && newer.epoch > captured.epoch,
        "已撤权期间的全新人工急停也必须推进代次"
    );
    let host = h.state.browsers().clone();
    let finish_run = run.clone();
    let finish_operation = operation.clone();
    let mut finish = tokio::spawn(async move {
        host.finish_human_steer(captured, &finish_run, &finish_operation, true)
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut finish)
            .await
            .is_err(),
        "外部清理屏障未释放，旧插话不得越过同一授权锁"
    );
    gate.release.add_permits(1);
    stop.await.unwrap();
    finish.await.unwrap().unwrap();
    let actual = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(actual.revoked);
    assert_eq!(actual.epoch, newer.epoch);
    let receipt = h.state.storage().query_one_text("SELECT result_json FROM browser_commands_v1 WHERE operation_id=?1 AND status='confirmed'",vec![operation]).await.unwrap().unwrap();
    let receipt: Value = serde_json::from_str(&receipt).unwrap();
    assert_eq!(receipt["accepted"], true);
    assert_eq!(
        receipt["authorityGranted"], false,
        "如实确认文字被接收，但旧插话不能解除后来急停"
    );
    assert!(h.browser.calls.lock().unwrap().is_empty());
    h.state
        .sessions()
        .cancel_run_for(h.state.storage().as_ref(), h.state.runs(), &principal, &run)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let actual = h
                .state
                .storage()
                .load_run(&RunId::new(&run))
                .await
                .unwrap()
                .unwrap();
            if actual.status.is_terminal() {
                assert_eq!(actual.status, lingxi_protocol::RunStatus::Cancelled);
                // 终态落库早于后台租约释放；新用户任务还必须等实际会话占用归还。
                if !h.state.sessions().session_supervisor().is_busy("browser-a") {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let next = h.submit(json!({"action":"start"})).await;
    success(h.wait(&next).await);
    let released = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(!released.revoked && released.epoch > actual.epoch);
    http_emergency(&h, addr, "new-stop-wins").await;
    let replayed = lingxi_adapters::storage::browser::get(h.state.storage(), "browser-a")
        .await
        .unwrap()
        .unwrap();
    assert!(!replayed.revoked);
    assert_eq!(replayed.epoch, released.epoch);
    assert_eq!(
        h.browser.revoke_count.load(Ordering::Acquire),
        2,
        "同requestId回放不能再次关闭浏览器"
    );
    assert_eq!(
        h.browser.calls.lock().unwrap().len(),
        1,
        "只有新用户任务实际启动浏览器"
    );
    app.abort();
    let _ = app.await;
    h.close().await;
}
