//! R07-T07-δ goal + /loop + loop_control 集成验收（对照 T07_STATUS.md 的
//! 「δ 验收测试形态」登记）：
//! - goal 工具六动作（create/status/pause/resume/complete/drop）经真 Run 的
//!   工具调用闭环；替身逐轮上报 token usage → 预算累计 → 超支事件恰好一次
//!   （app_events 的 goal_over_budget 有且仅有一条；模型可见面 steering
//!   `<hana-goal-over-budget>` 随下一轮提交送达替身）。
//! - /loop 五子命令（启动/status/pause/resume/stop）作用于真会话：kickoff
//!   驱动一个替身模型轮；轮内 loop_control schedule 设闹钟 → 闹钟触发唤醒
//!   轮 → 轮内 loop_control complete → /loop status 可见 completed 与摘要。
//!
//! 模型替身的行为纪律与 r07_automation_flow 一致：路由只看提交内容（命令
//! JSON / 循环消息标记），不读工作区、不知道任何运行内幕；工具结果经
//! input.prior 的 ToolResult 观察。与自动化用例的唯一差异：本文件的替身
//! 每轮都上报 usage（goal 预算进账的观察缝）。

use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, UsageRecord};
use lingxi_service::{
    approval_service::ApprovalService,
    inject::{ManualClock, ServiceClock},
    loopx::LoopStatus,
    prepare_layout,
    toolgateway::ToolInvocationGateway,
    HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

/// 确定性替身：命令 JSON → 工具调用；循环唤醒消息 → loop_control；其余 →
/// 文本终稿。每一轮都上报 30+30 token（goal 预算进账的观察样本）。
#[derive(Default)]
struct Provider {
    calls: AtomicUsize,
    submissions: Mutex<Vec<String>>,
    results: Mutex<Vec<Value>>,
    held_loop_requests: Arc<AtomicUsize>,
    held_loop_run: Mutex<Option<String>>,
}

struct HeldLoopRequest(Arc<AtomicUsize>);
impl Drop for HeldLoopRequest {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Provider {
    fn submissions(&self) -> Vec<String> {
        self.submissions.lock().unwrap().clone()
    }
    fn last_result(&self) -> Value {
        self.results.lock().unwrap().last().unwrap().clone()
    }
}

impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.fixture".into(),
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        let submission = input.submission.clone();
        self.submissions.lock().unwrap().push(submission.clone());
        Box::pin(async move {
            if submission.contains("<hana-loop kind=\"kickoff\">")
                && submission.contains("cancel-held-loop")
            {
                *self.held_loop_run.lock().unwrap() = Some(ctx.run_id.to_string());
                self.held_loop_requests.fetch_add(1, Ordering::SeqCst);
                let _guard = HeldLoopRequest(self.held_loop_requests.clone());
                // 外部模型请求没有响应；正式取消应释放此future并完成真实Run清理。
                std::future::pending::<()>().await;
            }
            if submission.contains("late-loop-new-held") {
                *self.held_loop_run.lock().unwrap() = Some(ctx.run_id.to_string());
                self.held_loop_requests.fetch_add(1, Ordering::SeqCst);
                let _guard = HeldLoopRequest(self.held_loop_requests.clone());
                // 新一代外部请求独立挂住，避免其合法完成掩盖旧运行误记账。
                std::future::pending::<()>().await;
            }
            if input.turn == 1 && submission.contains("late-loop-old-") {
                let args = if submission.contains("late-loop-old-schedule") {
                    json!({"action":"schedule","delay_seconds":60,"reason":"旧运行不可覆盖新闹钟"})
                } else {
                    json!({"action":"complete","reason":"旧运行不可收束新循环"})
                };
                return ProviderTurnResult::of_ctx_with_usage(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            "tool:first-party:loop_control".to_owned(),
                            args,
                            &SchemaBudget::default(),
                        )
                        .unwrap()],
                        content: vec![],
                    },
                    UsageRecord {
                        input_tokens: 30,
                        output_tokens: 30,
                    },
                );
            }
            if input.turn == 1 {
                // 命令通道：用户文本整体是 {"tool":..., "args":...}；超支
                // steering 会以 "\n\n[steering]\n" 后缀拼进提交，先剥掉再解析。
                let head = submission
                    .split("\n\n[steering]\n")
                    .next()
                    .unwrap_or(submission.as_str());
                let command = serde_json::from_str::<Value>(head).ok().or_else(|| {
                    head.rsplit("\n\n")
                        .find_map(|part| serde_json::from_str::<Value>(part).ok())
                });
                if let Some(command) = command {
                    if let Some(tool) = command["tool"].as_str() {
                        return ProviderTurnResult::of_ctx_with_usage(
                            ctx,
                            ProviderTurn::ToolRequests {
                                requests: vec![ToolRequest::from_effective_arguments(
                                    format!("tool:first-party:{tool}"),
                                    command["args"].clone(),
                                    &SchemaBudget::default(),
                                )
                                .unwrap()],
                                content: vec![],
                            },
                            UsageRecord {
                                input_tokens: 30,
                                output_tokens: 30,
                            },
                        );
                    }
                }
                // 循环轮路由：resume 轮设闹钟（60s = 现役 min_delay 钳制值），
                // 闹钟唤醒轮收束循环。理由词是两轮之间的确定性区分符。
                if submission.contains("<hana-loop kind=\"wakeup\">") {
                    let args = if submission.contains("Reason: loop resumed") {
                        json!({"action":"schedule","delay_seconds":60,"reason":"poke-check"})
                    } else if submission.contains("Reason: poke-check") {
                        json!({"action":"complete","reason":"目标达成"})
                    } else {
                        json!({"action":"complete","reason":"未知唤醒直接收束"})
                    };
                    return ProviderTurnResult::of_ctx_with_usage(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: vec![ToolRequest::from_effective_arguments(
                                "tool:first-party:loop_control".to_string(),
                                args,
                                &SchemaBudget::default(),
                            )
                            .unwrap()],
                            content: vec![],
                        },
                        UsageRecord {
                            input_tokens: 30,
                            output_tokens: 30,
                        },
                    );
                }
            }
            if let Some(result) = input.prior.iter().rev().find_map(|entry| {
                if let ExchangeItem::ToolResult { outcome, .. } = entry {
                    Some(match outcome {
                        ToolOutcome::Success { result } => {
                            let text = result
                                .content
                                .iter()
                                .find_map(|block| match block {
                                    ContentBlock::Text { text } => Some(text.clone()),
                                    _ => None,
                                })
                                .unwrap_or_default();
                            json!({"ok":true,"text":text,"details":result.details})
                        }
                        ToolOutcome::Failed { error } => {
                            json!({"ok":false,"error":error.message})
                        }
                        _ => json!({"ok":false}),
                    })
                } else {
                    None
                }
            }) {
                self.results.lock().unwrap().push(result);
            }
            ProviderTurnResult::of_ctx_with_usage(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: "合成应答".into(),
                        }],
                        model_call_id: Some(call.clone()),
                    },
                },
                UsageRecord {
                    input_tokens: 30,
                    output_tokens: 30,
                },
            )
        })
    }
}

struct Harness {
    home: PathBuf,
    state: ServiceState,
    clock: Arc<ManualClock>,
    provider: Arc<Provider>,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl Harness {
    /// sessions: (session_id, authorized_folders)——授权目录须已存在。
    async fn boot(sessions: &[(&str, &[String])]) -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("lingxi_service=debug")
            .with_test_writer()
            .try_init();
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-goal-loop-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::write(
            home.join("agents/lingxi/config.yaml"),
            "agent:\n  name: 合成助手\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        Self::boot_home(home, sessions, 1_800_000_000_000).await
    }

    /// 重启沿用真实目录和数据库，不重建会话，也不替换内部恢复钩子。
    async fn boot_home(home: PathBuf, sessions: &[(&str, &[String])], now: u64) -> Self {
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(ManualClock::new(now));
        let provider = Arc::new(Provider::default());
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            approvals,
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
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
                clock: clock.clone(),
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, done) = tokio::sync::oneshot::channel();
        let router = lingxi_service::build_router(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = done.await;
            })
            .await
            .unwrap();
        });
        for (session, folders) in sessions {
            state
                .session_admin()
                .create_session(
                    session,
                    "lingxi",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "合成会话",
                    Some("operate"),
                    folders,
                    true,
                    clock.now_unix_ms(),
                )
                .await
                .unwrap();
        }
        Self {
            home,
            state,
            clock,
            provider,
            url: format!("http://{addr}"),
            token,
            stop,
            task,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value) -> (u16, Value) {
        let client = reqwest::Client::new();
        let result = client
            .request(method.parse().unwrap(), format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = result.status().as_u16();
        let body = result.json().await.unwrap();
        (status, body)
    }
    async fn slash(&self, session: &str, text: &str) -> Value {
        let (status, body) = self
            .request(
                "POST",
                &format!("/lingxi/v1/sessions/{session}/slash"),
                json!({"text": text}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        body
    }
    async fn close(self) -> PathBuf {
        self.state.loop_services().stop_idle_listener();
        self.state.loop_services().alarm.dispose();
        self.state.loop_services().controller.dispose();
        assert!(self
            .state
            .workflows()
            .stop(std::time::Duration::from_secs(5))
            .await
            .unwrap()
            .is_empty());
        assert!(self
            .state
            .background()
            .drain_within(std::time::Duration::from_secs(5))
            .await
            .unconfirmed
            .is_empty());
        let _ = self.stop.send(());
        self.task.await.unwrap();
        self.state.storage().close().await.unwrap();
        self.home
    }
}

/// 真 Run 驱动一次工具调用并等到终态。
async fn chat(h: &Harness, session: &str, input: Value) -> String {
    let principal = h
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", h.token)),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    let accepted = h
        .state
        .sessions()
        .execute_background_for(
            h.state.storage(),
            h.state.events(),
            h.state.runs(),
            h.state.background(),
            &principal,
            session,
            &lingxi_service::ExecuteSubmission::plain(&input.to_string()),
            h.clock.now_unix_ms(),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let run = h
                .state
                .storage()
                .load_run(&lingxi_protocol::RunId::new(&accepted.run_id))
                .await
                .unwrap()
                .unwrap();
            if run.status.is_terminal() {
                assert_eq!(run.status, lingxi_protocol::RunStatus::Completed);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("真实Run必须结束");
    accepted.run_id
}

async fn wait_until(what: &str, timeout_secs: u64, mut pred: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async move {
        loop {
            if pred() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{what} 未在 {timeout_secs}s 内满足"));
}

fn goal_over_budget_events(h: &Harness) -> Vec<Value> {
    h.state.app_events().since(0)["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["type"] == "goal_over_budget")
        .cloned()
        .collect()
}

#[tokio::test]
async fn goal_six_actions_close_loop_and_over_budget_fires_exactly_once() {
    let h = Harness::boot(&[("goal-session", &[])]).await;

    // create：token 预算 50。进账时序（账本落盘钩在模型调用收束时触发，
    // 工具执行在其后）：本 run 第一轮的 60 token 落账时 goal 尚不存在
    // （不记），create 执行后第二轮的 60 进账 → 60 ≥ 50 越限，事件在
    // 本 run 内发出。
    chat(
        &h,
        "goal-session",
        json!({"tool":"goal","args":{"action":"create","name":"整备资料","token_budget":50}}),
    )
    .await;
    let created = h.provider.last_result();
    assert_eq!(created["ok"], true, "{created}");
    assert_eq!(created["details"]["goal"]["name"], "整备资料");
    assert_eq!(created["details"]["goal"]["tokenBudget"], 50);
    assert_eq!(created["details"]["goal"]["status"], "active");
    assert_eq!(created["details"]["goal"]["tokensUsed"], 0);

    // 超支事件恰好一次（create run 的第二轮进账 60 越限时触发）。
    let events = goal_over_budget_events(&h);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["payload"]["name"], "整备资料");
    assert_eq!(events[0]["payload"]["kind"], "tokens");
    assert_eq!(events[0]["payload"]["tokenBudget"], 50);
    assert_eq!(events[0]["payload"]["tokensUsed"], 60);
    assert_eq!(events[0]["payload"]["sessionId"], "goal-session");

    // status：本 run 第一轮再进账 60（累计恰为 120 时读取），overBudget
    // 保持；且模型可见面的 <hana-goal-over-budget> steering 随本论提交送达
    // 替身。
    chat(
        &h,
        "goal-session",
        json!({"tool":"goal","args":{"action":"status"}}),
    )
    .await;
    let status = h.provider.last_result();
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(status["details"]["hasGoal"], true);
    assert_eq!(status["details"]["goal"]["overBudget"], true);
    assert_eq!(status["details"]["goal"]["tokensUsed"], 120);
    assert!(
        h.provider.submissions().iter().any(|s| s.contains(
            "<hana-goal-over-budget goal=\"整备资料\">token budget 50 exceeded (used 60)."
        )),
        "超支 steering 必须随下一轮送达模型"
    );
    // 又两轮进账（240 累计）后仍然只有一条超支事件——「恰好一次」。
    assert_eq!(goal_over_budget_events(&h).len(), 1);

    // pause / resume。
    chat(
        &h,
        "goal-session",
        json!({"tool":"goal","args":{"action":"pause"}}),
    )
    .await;
    let paused = h.provider.last_result();
    assert_eq!(paused["ok"], true, "{paused}");
    assert_eq!(paused["details"]["ok"], true);
    chat(
        &h,
        "goal-session",
        json!({"tool":"goal","args":{"action":"resume"}}),
    )
    .await;
    let resumed = h.provider.last_result();
    assert_eq!(resumed["ok"], true, "{resumed}");
    assert_eq!(resumed["details"]["ok"], true);

    // complete：收官文本含最终读数；此后 goal 不再活跃。
    chat(
        &h,
        "goal-session",
        json!({"tool":"goal","args":{"action":"complete"}}),
    )
    .await;
    let completed = h.provider.last_result();
    assert_eq!(completed["ok"], true, "{completed}");
    assert_eq!(completed["details"]["ok"], true);
    assert!(completed["text"]
        .as_str()
        .unwrap()
        .contains("goal completed"));

    // create 可替换已收官 goal；drop 收尾；status 对收官 goal 保持可读
    // （现役 readout 语义：status 回报最后一任 goal 的终态与读数）。
    chat(
        &h,
        "goal-session",
        json!({"tool":"goal","args":{"action":"create","name":"临时","token_budget":1000}}),
    )
    .await;
    let recreated = h.provider.last_result();
    assert_eq!(recreated["ok"], true, "{recreated}");
    assert_eq!(recreated["details"]["goal"]["name"], "临时");
    chat(
        &h,
        "goal-session",
        json!({"tool":"goal","args":{"action":"drop"}}),
    )
    .await;
    let dropped = h.provider.last_result();
    assert_eq!(dropped["ok"], true, "{dropped}");
    assert_eq!(dropped["details"]["ok"], true);
    assert_eq!(dropped["text"].as_str().unwrap(), "goal dropped");
    chat(
        &h,
        "goal-session",
        json!({"tool":"goal","args":{"action":"status"}}),
    )
    .await;
    let readout = h.provider.last_result();
    assert_eq!(readout["details"]["hasGoal"], true, "{readout}");
    assert_eq!(readout["details"]["goal"]["status"], "dropped");
    assert_eq!(readout["details"]["goal"]["name"], "临时");
    assert_eq!(goal_over_budget_events(&h).len(), 1);

    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn loop_slash_lifecycle_alarm_wakeup_and_loop_control() {
    let h = Harness::boot(&[("loop-session", &[])]).await;
    let controller = &h.state.loop_services().controller;

    // 启动：kickoff 驱动一个替身模型轮（替身直接终稿，不调工具）。
    let started = h.slash("loop-session", "/loop 盯一下合成队列").await;
    assert_eq!(started["results"][0]["level"], "success", "{started}");
    assert!(
        started["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("循环已启动"),
        "{started}"
    );
    wait_until("kickoff 轮收束", 10, || {
        controller
            .tool_status("loop-session")
            .is_some_and(|record| record.turn_count == 1)
    })
    .await;
    let record = controller.tool_status("loop-session").unwrap();
    assert_eq!(record.status, LoopStatus::Running);
    assert_eq!(record.turn_count, 1);

    // status：running 可见。
    let status = h.slash("loop-session", "/loop status").await;
    let text = status["results"][0]["text"].as_str().unwrap();
    assert!(text.contains("循环状态：running"), "{text}");
    assert!(text.contains("轮数：1/50"), "{text}");
    assert!(text.contains("任务：盯一下合成队列"), "{text}");

    // pause → resume：暂停期间闹钟槽清空；resume 轮替身调 loop_control
    // schedule 设 60s 闹钟（现役 min_delay 钳制值，真实等待触发）。
    let paused = h.slash("loop-session", "/loop pause").await;
    assert_eq!(paused["results"][0]["level"], "success", "{paused}");
    assert_eq!(
        controller.tool_status("loop-session").unwrap().status,
        LoopStatus::Paused
    );
    let status = h.slash("loop-session", "/loop status").await;
    assert!(
        status["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("循环状态：paused（user）"),
        "{status}"
    );
    let resumed = h.slash("loop-session", "/loop resume").await;
    assert_eq!(resumed["results"][0]["level"], "success", "{resumed}");
    wait_until("resume 轮设好闹钟", 10, || {
        controller
            .tool_status("loop-session")
            .is_some_and(|record| record.status == LoopStatus::Running && record.alarm.is_some())
            && h.provider
                .results
                .lock()
                .unwrap()
                .last()
                .is_some_and(|result| {
                    result["text"]
                        .as_str()
                        .is_some_and(|text| text.contains("Wakeup scheduled in 60s"))
                })
            && !h
                .state
                .sessions()
                .session_supervisor()
                .is_busy("loop-session")
    })
    .await;
    let schedule_result = h.provider.last_result();
    assert_eq!(schedule_result["ok"], true, "{schedule_result}");
    assert!(
        schedule_result["text"]
            .as_str()
            .unwrap()
            .contains("Wakeup scheduled in 60s"),
        "{schedule_result}"
    );

    // 闹钟真实触发 → 唤醒轮替身调 loop_control complete → 循环收束。
    wait_until("闹钟唤醒轮收束循环", 100, || {
        controller
            .tool_status("loop-session")
            .is_some_and(|record| record.status == LoopStatus::Completed)
            && h.provider
                .results
                .lock()
                .unwrap()
                .last()
                .is_some_and(|result| result["text"] == "Loop completed and closed.")
            && !h
                .state
                .sessions()
                .session_supervisor()
                .is_busy("loop-session")
    })
    .await;
    let complete_result = h.provider.last_result();
    assert_eq!(complete_result["ok"], true, "{complete_result}");
    assert_eq!(
        complete_result["text"].as_str().unwrap(),
        "Loop completed and closed."
    );

    // complete 后状态可见：completed + 摘要。
    let status = h.slash("loop-session", "/loop status").await;
    let text = status["results"][0]["text"].as_str().unwrap();
    assert!(text.contains("循环状态：completed"), "{text}");
    assert!(text.contains("完成摘要：目标达成"), "{text}");

    // stop 子命令对已收束循环依然响亮可用（现役命令面语义）。
    let stopped = h.slash("loop-session", "/loop stop").await;
    assert_eq!(stopped["results"][0]["level"], "success", "{stopped}");
    assert_eq!(
        controller.tool_status("loop-session").unwrap().status,
        LoopStatus::Stopped
    );

    // 替身轮次与路由证据：kickoff 一轮、resume 一轮、闹钟唤醒一轮，共 5 次
    // 模型调用（1+2+2），三类循环消息逐一出现。
    let submissions = h.provider.submissions();
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 5);
    assert!(submissions
        .iter()
        .any(|s| s.contains("<hana-loop kind=\"kickoff\">") && s.contains("Task: 盯一下合成队列")));
    assert!(submissions
        .iter()
        .any(|s| s.contains("kind=\"wakeup\"") && s.contains("Reason: loop resumed")));
    assert!(submissions
        .iter()
        .any(|s| s.contains("kind=\"wakeup\"") && s.contains("Reason: poke-check")));

    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

/// 真实数据库写锁制造启动等待；只替换外部模型，不替换存储或循环钩子。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_database_lock_stop_supersedes_http_loop_start_without_new_run() {
    use std::time::Duration;

    let session = "loop-start-db-lock";
    let h = Harness::boot(&[(session, &[])]).await;
    assert_eq!(h.state.storage().count_runs(session).await.unwrap(), 0);
    let connection = rusqlite::Connection::open(h.state.storage().db_path()).unwrap();
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    // WAL允许真实HTTP的前置归属读取，但挡住后续展示消息的BEGIN IMMEDIATE。
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let url = format!("{}/lingxi/v1/sessions/{session}/slash", h.url);
    let token = h.token.clone();
    let start = tokio::spawn(async move {
        let response = reqwest::Client::new()
            .post(url)
            .bearer_auth(token)
            .json(&json!({"text":"/loop 等待真实存储后再启动"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        response.json::<Value>().await.unwrap()
    });
    let loop_file = h.home.join(".ephemeral/loop-state.json");
    wait_until("真实loop-state先保存running", 2, || {
        std::fs::read(&loop_file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|value| {
                value["loops"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|record| record["key"] == session && record["status"] == "running")
            })
    })
    .await;
    // 只读探针也经正式队列：超时证明队列确实被前面的真实写事务挡住，
    // 不以固定sleep或替换record_user_prompt来猜测已进入等待窗口。
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if tokio::time::timeout(
                Duration::from_millis(100),
                h.state.storage().count_runs(session),
            )
            .await
            .is_err()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("展示消息实际写入必须被外部SQLite事务挡住");
    assert!(!start.is_finished());
    let messages_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id=?1 AND entry_type='loop-user-prompt'",
            [session],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(messages_before, 0, "持有写锁期间展示消息尚未提交");
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 0);

    // 第二个HTTP也须经同一队列读取owner，因此这里直接调用同一个生产控制器。
    let stopped = tokio::time::timeout(
        Duration::from_secs(1),
        h.state
            .loop_services()
            .controller
            .stop(&lingxi_service::loopx::LoopTarget::desktop(session)),
    )
    .await
    .expect("停止不能等待被锁住的消息写入")
    .unwrap();
    assert_eq!(stopped.status, LoopStatus::Stopped);
    assert!(stopped.alarm.is_none());
    let response = tokio::time::timeout(Duration::from_secs(1), start)
        .await
        .expect("旧启动必须在数据库仍被锁住时返回已被取代")
        .unwrap();
    assert_eq!(response["results"][0]["level"], "error", "{response}");
    assert!(
        response["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("取代"),
        "{response}"
    );

    connection.execute_batch("ROLLBACK").unwrap();
    // 已送入正式写队列的展示消息可以在解锁后落盘；不能假称该写入被回滚。
    // 队列读回同时等待原写事务收尾，随后核验真实执行事实没有新增。
    assert_eq!(h.state.storage().count_runs(session).await.unwrap(), 0);
    let messages_after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id=?1 AND entry_type='loop-user-prompt'",
            [session],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(messages_after, 1, "实际展示消息已保存，未触发模型任务");
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 0);
    assert!(!h.state.sessions().session_supervisor().is_busy(session));
    let persisted: Value = serde_json::from_slice(&std::fs::read(loop_file).unwrap()).unwrap();
    let record = persisted["loops"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["key"] == session)
        .unwrap();
    assert_eq!(record["status"], "stopped");
    assert!(record["alarm"].is_null());
    // 本场景没有触发任何执行轮，也没有已登记闹钟；内部计时器登记表的
    // 交错回收另由loopx单元检查直接观测，不能把磁盘空槽冒称为其内存观测。
    drop(connection);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

/// 只从已提交的展示消息抢真实WAL写锁；命中后必须同时看见取消登记和缺失的Run行。
/// 错过窗口会明确返回None，由独立的新目录重建场景，不能把未命中算通过。
fn observe_registered_before_start(
    h: &Harness,
    session: &'static str,
) -> (
    tokio::sync::oneshot::Receiver<Option<String>>,
    std::sync::mpsc::Sender<()>,
    std::thread::JoinHandle<()>,
) {
    let path = h.state.storage().db_path().to_path_buf();
    let runs = h.state.runs().clone();
    let (observed, observation) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let db = rusqlite::Connection::open(path).unwrap();
        db.busy_timeout(std::time::Duration::ZERO).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut locked = false;
        while std::time::Instant::now() < deadline {
            let prompts: i64 = db.query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id=?1 AND entry_type='loop-user-prompt'",
                [session], |row| row.get(0),
            ).unwrap();
            if prompts > 0 && db.execute_batch("BEGIN IMMEDIATE").is_ok() {
                locked = true;
                break;
            }
            std::thread::yield_now();
        }
        let mut hit = None;
        if locked {
            while std::time::Instant::now() < deadline {
                let ids = runs.cancel_registry().live_run_ids();
                if let Some(id) = ids.first() {
                    let durable: i64 = db
                        .query_row("SELECT COUNT(*) FROM runs WHERE run_id=?1", [id], |row| {
                            row.get(0)
                        })
                        .unwrap();
                    if durable == 0 {
                        hit = Some(id.clone());
                    }
                    break;
                }
                std::thread::yield_now();
            }
        }
        let matched = hit.is_some();
        let _ = observed.send(hit);
        if matched {
            // 此等待只保持已经证实的外部锁；不以等待时间推断产品执行位置。
            let _ = released.recv_timeout(std::time::Duration::from_secs(3));
        }
        if locked {
            db.execute_batch("ROLLBACK").unwrap();
        }
    });
    (observation, release, thread)
}

fn assert_new_loop_unaffected(h: &Harness, session: &str) {
    let record = h
        .state
        .loop_services()
        .controller
        .tool_status(session)
        .unwrap();
    assert_eq!(record.prompt, "late-loop-new-held");
    assert_eq!(record.status, LoopStatus::Running, "旧工具不得收束新一代");
    assert_eq!(record.turn_count, 0, "旧运行不得占用新一代预算");
    assert_eq!(record.consecutive_failures, 0, "旧运行失败不得记入新一代");
    assert!(record
        .alarm
        .as_ref()
        .is_none_or(|alarm| alarm.reason != "旧运行不可覆盖新闹钟"));
}

async fn wait_new_loop_file(h: &Harness, session: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if h.state
                .loop_services()
                .controller
                .tool_status(session)
                .is_some_and(|r| {
                    r.status == LoopStatus::Running && r.prompt == "late-loop-new-held"
                })
            {
                let file: Value = serde_json::from_slice(
                    &std::fs::read(h.home.join(".ephemeral/loop-state.json")).unwrap(),
                )
                .unwrap();
                assert!(file["loops"].as_array().unwrap().iter().any(|r| {
                    r["key"] == session
                        && r["status"] == "running"
                        && r["prompt"] == "late-loop-new-held"
                }));
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("真实新Running与新任务文本必须先保存");
}

/// 旧driver已登记取消范围但实际Run事务尚未提交，换代后必须统一收尾，不能借新身份。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_wal_registered_before_started_cannot_borrow_new_loop_identity() {
    use std::time::Duration;
    let session = "loop-late-real-start";
    for action in ["complete", "schedule"] {
        let mut matched = false;
        for attempt in 0..8 {
            let h = Harness::boot(&[(session, &[])]).await;
            let (observation, release, observer) = observe_registered_before_start(&h, session);
            let url = format!("{}/lingxi/v1/sessions/{session}/slash", h.url);
            let token = h.token.clone();
            let text = format!("/loop late-loop-old-{action}");
            let old_start = tokio::spawn(async move {
                reqwest::Client::new()
                    .post(url)
                    .bearer_auth(token)
                    .json(&json!({"text":text}))
                    .send()
                    .await
                    .unwrap()
                    .json::<Value>()
                    .await
                    .unwrap()
            });
            let old_id = observation.await.unwrap();
            let Some(old_id) = old_id else {
                observer.join().unwrap();
                let _ = old_start.await.unwrap();
                h.state
                    .loop_services()
                    .controller
                    .stop(&lingxi_service::loopx::LoopTarget::desktop(session))
                    .await
                    .unwrap();
                eprintln!("真实初始登记窗口未命中：{action} attempt={attempt}；本次不计通过");
                let home = h.close().await;
                std::fs::remove_dir_all(home).unwrap();
                continue;
            };
            assert!(h
                .state
                .runs()
                .cancel_registry()
                .live_run_ids()
                .contains(&old_id));
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(100),
                    h.state.storage().count_runs(session)
                )
                .await
                .is_err(),
                "仅在正式队列也被真实开始写事务阻塞时才算命中"
            );
            h.state
                .loop_services()
                .controller
                .stop(&lingxi_service::loopx::LoopTarget::desktop(session))
                .await
                .unwrap();
            let controller = h.state.loop_services().controller.clone();
            let new_start = tokio::spawn(async move {
                controller
                    .start(
                        &lingxi_service::loopx::LoopTarget::desktop(session),
                        "late-loop-new-held",
                    )
                    .await
            });
            wait_new_loop_file(&h, session).await;
            assert_new_loop_unaffected(&h, session);
            release.send(()).unwrap();
            observer.join().unwrap();
            let old_response = tokio::time::timeout(Duration::from_secs(5), old_start)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                old_response["results"][0]["level"], "error",
                "{old_response}"
            );
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let old = h
                        .state
                        .storage()
                        .load_run(&lingxi_protocol::RunId::new(&old_id))
                        .await
                        .unwrap()
                        .unwrap();
                    if old.status.is_terminal() {
                        assert!(
                            matches!(
                                old.status,
                                lingxi_protocol::RunStatus::Cancelled
                                    | lingxi_protocol::RunStatus::Failed
                            ),
                            "旧Run不可继续成功调用工具：{:?}",
                            old.status
                        );
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("已登记且实际开始提交的旧Run必须进入真实终态");
            let _ = tokio::time::timeout(Duration::from_secs(5), new_start)
                .await
                .unwrap()
                .unwrap();
            assert_new_loop_unaffected(&h, session);
            assert!(
                h.provider
                    .submissions()
                    .iter()
                    .all(|s| !s.contains(&format!("late-loop-old-{action}"))),
                "旧身份在真实开始写入后已失效，不可到达外部模型或loop_control"
            );
            // 新Run若已实际发出，仅取消其外部挂起请求；不能让合法完成污染上述计数。
            h.slash(session, "/stop").await;
            h.state
                .loop_services()
                .controller
                .stop(&lingxi_service::loopx::LoopTarget::desktop(session))
                .await
                .unwrap();
            let home = h.close().await;
            std::fs::remove_dir_all(home).unwrap();
            matched = true;
            break;
        }
        assert!(
            matched,
            "8个独立真实目录都未命中{action}登记窗口；未验证，不得当通过"
        );
    }
}

/// 旧投递已冻结身份、尚在真实受理读取等待，换代后不能晚启动并认领新循环。
#[tokio::test(flavor = "current_thread")]
async fn real_db_admission_waiting_injection_cannot_claim_replacement_loop() {
    use lingxi_adapters::storage::session_tree::{append_message, AppendMessage};
    use std::time::Duration;
    let session = "loop-late-real-admission";
    let h = Harness::boot(&[(session, &[]), ("loop-db-blocker", &[])]).await;
    h.slash(session, "/loop 正常旧首轮").await;
    wait_until("旧首轮完成并真正释放会话", 5, || {
        h.state
            .loop_services()
            .controller
            .tool_status(session)
            .is_some_and(|r| r.turn_count == 1)
            && !h.state.sessions().session_supervisor().is_busy(session)
    })
    .await;
    let db = rusqlite::Connection::open(h.state.storage().db_path()).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let storage = h.state.storage().clone();
    let (writing, writer_entered) = tokio::sync::oneshot::channel();
    let write = tokio::spawn(async move {
        let _ = writing.send(());
        append_message(
            &storage,
            AppendMessage {
                session_id: "loop-db-blocker",
                message_id: "r07-lock-marker",
                parent_message_id: None,
                run_id: None,
                role: "user",
                content_json: "[]",
                entry_type: "message",
                model_call_id: None,
                now_unix_ms: 1_800_000_000_000,
            },
        )
        .await
        .unwrap();
    });
    writer_entered.await.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            h.state.storage().count_runs(session)
        )
        .await
        .is_err(),
        "真实写任务必须占住统一队列"
    );
    let controller = h.state.loop_services().controller.clone();
    let (entered, entry) = tokio::sync::oneshot::channel();
    let old_delivery = tokio::spawn(async move {
        let _ = entered.send(());
        // 同一个current_thread在首个真实await挂起前不会调度接收方；无内部替身钩子。
        controller
            .deliver_wakeup_turn(session, "late-loop-old-complete")
            .await
    });
    entry.await.unwrap();
    assert!(!old_delivery.is_finished(), "旧投递应停在真实受理读取");
    assert!(
        h.state.runs().cancel_registry().live_run_ids().is_empty(),
        "尚未启动driver"
    );
    h.state
        .loop_services()
        .controller
        .stop(&lingxi_service::loopx::LoopTarget::desktop(session))
        .await
        .unwrap();
    let controller = h.state.loop_services().controller.clone();
    let new_start = tokio::spawn(async move {
        controller
            .start(
                &lingxi_service::loopx::LoopTarget::desktop(session),
                "late-loop-new-held",
            )
            .await
    });
    wait_new_loop_file(&h, session).await;
    let delivery = tokio::time::timeout(Duration::from_secs(1), old_delivery)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.mode, lingxi_service::loopx::DeliveryMode::Stopped);
    db.execute_batch("ROLLBACK").unwrap();
    write.await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), new_start)
        .await
        .unwrap()
        .unwrap();
    assert_new_loop_unaffected(&h, session);
    assert_eq!(
        h.state.storage().count_runs(session).await.unwrap(),
        2,
        "真实Run只有旧首轮与新一代；被取代的受理不得晚建第三条"
    );
    assert!(h
        .provider
        .submissions()
        .iter()
        .all(|s| !s.contains("late-loop-old-complete")));
    h.slash(session, "/stop").await;
    h.state
        .loop_services()
        .controller
        .stop(&lingxi_service::loopx::LoopTarget::desktop(session))
        .await
        .unwrap();
    drop(db);
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn same_home_restart_recovers_overdue_loop_without_replaying_kickoff() {
    let session = "loop-real-restart";
    let h = Harness::boot(&[(session, &[])]).await;
    let started = h.slash(session, "/loop 重启后继续原任务").await;
    assert_eq!(started["results"][0]["level"], "success", "{started}");
    wait_until(
        "原循环真实首轮结束并保存兜底闹钟",
        10,
        || {
            h.state
                .loop_services()
                .controller
                .tool_status(session)
                .is_some_and(|record| record.turn_count == 1 && record.alarm.is_some())
                && !h.state.sessions().session_supervisor().is_busy(session)
        },
    )
    .await;
    assert_eq!(h.state.storage().count_runs(session).await.unwrap(), 1);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    let wake_at = h
        .state
        .loop_services()
        .controller
        .tool_status(session)
        .unwrap()
        .alarm
        .unwrap()
        .wake_at;
    let home = h.close().await;
    // 正常停服务只回收本机计时器，不改写持久running记录或闹钟槽。
    let stored: Value =
        serde_json::from_slice(&std::fs::read(home.join(".ephemeral/loop-state.json")).unwrap())
            .unwrap();
    assert_eq!(stored["loops"][0]["status"], "running");
    assert_eq!(stored["loops"][0]["alarm"]["wakeAt"], wake_at);

    // 同一真实home/DB重新bootstrap，时钟移到已过期之后，正式恢复调度加jitter唤醒。
    let restarted = Harness::boot_home(home, &[], wake_at + 1).await;
    wait_until("真实重启后的过期闹钟完成原循环", 15, || {
        restarted
            .state
            .loop_services()
            .controller
            .tool_status(session)
            .is_some_and(|record| record.status == LoopStatus::Completed)
            && !restarted
                .state
                .sessions()
                .session_supervisor()
                .is_busy(session)
    })
    .await;
    assert_eq!(
        restarted.state.storage().count_runs(session).await.unwrap(),
        2
    );
    let submissions = restarted.provider.submissions();
    assert_eq!(restarted.provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        submissions
            .iter()
            .filter(|text| text.contains("<hana-loop kind=\"wakeup\">"))
            .count(),
        2,
        "同一唤醒run的模型调用和工具返回轮均携带原提交：{submissions:?}"
    );
    assert!(!submissions
        .iter()
        .any(|text| text.contains("<hana-loop kind=\"kickoff\">")));
    let record = restarted
        .state
        .loop_services()
        .controller
        .tool_status(session)
        .unwrap();
    assert_eq!(record.prompt, "重启后继续原任务");
    assert!(record.alarm.is_none());
    let status = restarted.slash(session, "/loop status").await;
    assert!(
        status["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("completed"),
        "{status}"
    );
    let root = restarted.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn real_composition_releases_loop_workflow_and_gateway_after_stop() {
    let session = "loop-real-release";
    let h = Harness::boot(&[(session, &[])]).await;
    let controller = Arc::downgrade(&h.state.loop_services().controller);
    let alarm = Arc::downgrade(&h.state.loop_services().alarm);
    let workflows = Arc::downgrade(h.state.workflows());
    let runs = Arc::downgrade(h.state.runs());
    let gateway = Arc::downgrade(h.state.tool_gateway().unwrap());
    let sessions = Arc::downgrade(h.state.sessions().session_supervisor());
    let started = h.slash(session, "/loop 核对真实组合根释放").await;
    assert_eq!(started["results"][0]["level"], "success", "{started}");
    wait_until(
        "真实循环曾武装计时器且执行轮已结束",
        10,
        || {
            h.state
                .loop_services()
                .controller
                .tool_status(session)
                .is_some_and(|record| record.turn_count == 1 && record.alarm.is_some())
                && !h.state.sessions().session_supervisor().is_busy(session)
        },
    )
    .await;
    let stopped = h.slash(session, "/loop stop").await;
    assert_eq!(stopped["results"][0]["level"], "success", "{stopped}");
    assert_eq!(h.state.storage().count_runs(session).await.unwrap(), 1);
    let root = h.close().await;
    // 不手工解绑工具，不清私有registry；真实停机后所有组合根弱引用均应失效。
    wait_until(
        "真实组合根及计时任务释放全部所属引用",
        5,
        || {
            controller.upgrade().is_none()
                && alarm.upgrade().is_none()
                && workflows.upgrade().is_none()
                && runs.upgrade().is_none()
                && gateway.upgrade().is_none()
                && sessions.upgrade().is_none()
        },
    )
    .await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn actual_loop_cancel_settles_once_and_next_user_run_cannot_borrow_old_active_mark() {
    let session = "loop-real-cancel";
    let h = Harness::boot(&[(session, &[])]).await;
    let started = h.slash(session, "/loop cancel-held-loop").await;
    assert_eq!(started["results"][0]["level"], "success", "{started}");
    wait_until("真实Loop模型请求已挂住", 5, || {
        h.provider.held_loop_requests.load(Ordering::SeqCst) == 1
    })
    .await;
    let run_id = h.provider.held_loop_run.lock().unwrap().clone().unwrap();
    assert_eq!(
        h.state
            .loop_services()
            .controller
            .tool_status(session)
            .unwrap()
            .turn_count,
        0
    );
    let stopped = h.slash(session, "/stop").await;
    assert_eq!(stopped["results"][0]["level"], "success", "{stopped}");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let run = h
                .state
                .storage()
                .load_run(&lingxi_protocol::RunId::new(&run_id))
                .await
                .unwrap()
                .unwrap();
            let record = h
                .state
                .loop_services()
                .controller
                .tool_status(session)
                .unwrap();
            if run.status == lingxi_protocol::RunStatus::Cancelled
                && record.turn_count == 1
                && record.consecutive_failures == 1
                && h.provider.held_loop_requests.load(Ordering::SeqCst) == 0
                && !h.state.sessions().session_supervisor().is_busy(session)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("正式取消须真实Cancelled、释放模型和许可，并且仅结算一轮");
    let permit = h
        .state
        .storage()
        .session_admission()
        .try_acquire(&[session], usize::MAX)
        .expect("真实取消后原会话许可已释放");
    drop(permit);
    let terminal_count = h.state.storage().query_one_text(
        "SELECT COUNT(*) FROM key_events WHERE run_id=?1 AND event_type='run_state_changed' AND (payload_json LIKE '%\"to\":\"completed\"%' OR payload_json LIKE '%\"to\":\"failed\"%' OR payload_json LIKE '%\"to\":\"cancelled\"%')",
        vec![run_id.clone()],
    ).await.unwrap();
    assert_eq!(terminal_count.as_deref(), Some("1"));

    // 再次停止是明确的空操作；后续普通用户轮不能继承已经结算的Loop标记。
    let repeated = h.slash(session, "/stop").await;
    assert_eq!(repeated["results"][0]["level"], "success", "{repeated}");
    chat(&h, session, json!("取消后正常的新用户轮")).await;
    wait_until("后续真实用户轮释放会话", 5, || {
        !h.state.sessions().session_supervisor().is_busy(session)
    })
    .await;
    let record = h
        .state
        .loop_services()
        .controller
        .tool_status(session)
        .unwrap();
    assert_eq!(record.turn_count, 1, "普通用户轮不得借用旧Loop active标记");
    assert_eq!(record.consecutive_failures, 1);
    let old = h
        .state
        .storage()
        .load_run(&lingxi_protocol::RunId::new(&run_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.status, lingxi_protocol::RunStatus::Cancelled);
    assert_eq!(h.state.storage().count_runs(session).await.unwrap(), 2);
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 2);
    h.slash(session, "/loop stop").await;
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn goal_corrupt_sidecar_is_tool_failure_through_real_run_and_never_replaced() {
    let h = Harness::boot(&[("goal-corrupt", &[])]).await;
    let dir = h.home.join("goal-sidecars");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("goal-corrupt.goal.json");
    std::fs::write(&path, b"{damaged goal original").unwrap();
    for action in ["status", "create", "pause", "resume", "complete", "drop"] {
        chat(
            &h,
            "goal-corrupt",
            json!({"tool":"goal","args":{"action":action,"token_budget":10}}),
        )
        .await;
        let result = h.provider.last_result();
        assert_eq!(result["ok"], false, "{action}: {result}");
        assert!(
            result["error"]
                .as_str()
                .unwrap()
                .contains("goal storage is corrupt"),
            "{result}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"{damaged goal original");
    }
    assert!(goal_over_budget_events(&h).is_empty());
    h.close().await;
}

#[tokio::test]
async fn goal_write_fault_is_tool_failure_and_no_budget_notification_through_real_run() {
    let h = Harness::boot(&[("goal-write-fault", &[])]).await;
    chat(
        &h,
        "goal-write-fault",
        json!({"tool":"goal","args":{"action":"create","token_budget":10000}}),
    )
    .await;
    let path = h.home.join("goal-sidecars/goal-write-fault.goal.json");
    let before = std::fs::read(&path).unwrap();
    let recovery = h.home.join("file-recovery-records");
    if recovery.exists() {
        std::fs::rename(&recovery, h.home.join("fixture-retained-recovery-records")).unwrap();
    }
    std::fs::write(&recovery, b"blocker").unwrap();
    chat(
        &h,
        "goal-write-fault",
        json!({"tool":"goal","args":{"action":"pause"}}),
    )
    .await;
    let result = h.provider.last_result();
    assert_eq!(result["ok"], false, "{result}");
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("goal storage write failed"),
        "{result}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(goal_over_budget_events(&h).is_empty());
    h.close().await;
}

#[tokio::test]
async fn goal_preferences_read_is_pure_and_single_edit_preserves_budget_and_unknown_settings() {
    let h = Harness::boot(&[("goal-prefs", &[])]).await;
    let preferences = h.home.join("user/preferences.json");
    std::fs::create_dir_all(preferences.parent().unwrap()).unwrap();
    std::fs::write(
        &preferences,
        serde_json::to_vec(&json!({"unrelated":{"keep":true}})).unwrap(),
    )
    .unwrap();
    let before = std::fs::read(&preferences).unwrap();
    let (status, defaults) = h
        .request("GET", "/lingxi/v1/preferences/goal", Value::Null)
        .await;
    assert_eq!(status, 200, "{defaults}");
    assert_eq!(defaults["goal"]["enabled"], true);
    assert_eq!(std::fs::read(&preferences).unwrap(), before);
    let (status, saved) = h
        .request(
            "PUT",
            "/lingxi/v1/preferences/goal",
            json!({"enabled":false,"default_token_budget":500,"custom_compat":"keep"}),
        )
        .await;
    assert_eq!(status, 200, "{saved}");
    let (status, saved) = h
        .request(
            "PUT",
            "/lingxi/v1/preferences/goal",
            json!({"enabled":true}),
        )
        .await;
    assert_eq!(status, 200, "{saved}");
    let (status, read) = h
        .request("GET", "/lingxi/v1/preferences/goal", Value::Null)
        .await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["goal"]["enabled"], true);
    assert_eq!(read["goal"]["default_token_budget"], 500);
    assert_eq!(read["goal"]["custom_compat"], "keep");
    let stored: Value = serde_json::from_slice(&std::fs::read(&preferences).unwrap()).unwrap();
    assert_eq!(stored["unrelated"]["keep"], true);
    chat(
        &h,
        "goal-prefs",
        json!({"tool":"goal","args":{"action":"create","name":"使用保存预算"}}),
    )
    .await;
    assert_eq!(
        h.provider.last_result()["details"]["goal"]["tokenBudget"],
        500
    );
    let before_invalid = std::fs::read(&preferences).unwrap();
    let (status, _) = h
        .request(
            "PUT",
            "/lingxi/v1/preferences/goal",
            json!({"enabled":"wrong"}),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(std::fs::read(&preferences).unwrap(), before_invalid);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
