//! R07-T07-δ8 workflow 引擎集成验收（对照 T07_STATUS.md 的「δ 验收测试形态」
//! 登记）：替身模型下 script 起 2 节点 fan-out → runId 即回、workflow_progress
//! 事件流、终稿回灌、journal 文件生成；resumeFromRunId 复用未变节点（替身
//! 调用计数断言）+ 首处变更起全重跑；writeFolders 校验矩阵 fail-closed；
//! meta 非法同步拒绝；budgetTokens 硬上限在第二节点前落闸。
//!
//! 替身纪律：父会话提交整体是 {"tool":"workflow","args":...} 命令 JSON
//! （前序 workflow 完成的 steering 回灌以 "\n\n[steering]\n" 后缀拼接，先剥
//! 再解析）；子 run 提交 = 节点 prompt 原文，替身回显 "R:{prompt}" 作为节点
//! 结果。节点调用计数按「提交 == 节点 prompt 原文」精确匹配——父派单提交里
//! 携带的脚本源码不会误命中。

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

#[derive(Default)]
struct Provider {
    submissions: Mutex<Vec<String>>,
    results: Mutex<Vec<Value>>,
    all_results: Mutex<std::collections::HashMap<String, Value>>,
    blocked: Arc<AtomicUsize>,
    release: tokio::sync::Notify,
    workflow_deliveries: Mutex<Vec<(String, String)>>,
    delivery_cursor: Mutex<std::collections::HashMap<String, usize>>,
    loop_parent_blocked: Arc<AtomicUsize>,
    loop_parent_release: tokio::sync::Notify,
    loop_parent_run: Mutex<Option<String>>,
}

struct BlockedModel(Arc<AtomicUsize>);
impl Drop for BlockedModel {
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
    fn delivered_task(&self, session: &str, task: &str) -> bool {
        let marker = format!("[workflow-result task={task} status=");
        self.workflow_deliveries
            .lock()
            .unwrap()
            .iter()
            .any(|(owner, text)| owner == session && text.contains(&marker))
    }
    fn take_deliveries(&self, session: &str) -> Vec<String> {
        let delivered = self.workflow_deliveries.lock().unwrap();
        let mut cursors = self.delivery_cursor.lock().unwrap();
        let cursor = cursors.entry(session.into()).or_default();
        let result = delivered
            .iter()
            .skip(*cursor)
            .filter(|(owner, _)| owner == session)
            .map(|(_, text)| text.clone())
            .collect();
        *cursor = delivered.len();
        result
    }
    /// 节点模型调用计数：子 run 提交 == 节点 prompt 原文（父派单提交是命令
    /// JSON，脚本源码只是 args 的字符串值，不会与此相等）。
    fn node_calls(&self, prompt: &str) -> usize {
        self.submissions()
            .iter()
            .filter(|submission| submission.as_str() == prompt)
            .count()
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
        let submission = input.submission.clone();
        self.submissions.lock().unwrap().push(submission.clone());
        if submission.contains("[workflow-result task=") {
            self.workflow_deliveries
                .lock()
                .unwrap()
                .push((ctx.session_id.to_string(), submission.clone()));
        }
        Box::pin(async move {
            if submission.contains("<hana-loop kind=\"kickoff\">")
                && submission.contains("loop-parent-child-accounting")
            {
                if input.turn == 1 {
                    let script = "export const meta={name:'父子轮记账',description:'d'}; return await agent('wf-release',{access:'read'});";
                    return ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: vec![ToolRequest::from_effective_arguments(
                                "tool:first-party:workflow".to_string(),
                                json!({"script":script}),
                                &SchemaBudget::default(),
                            )
                            .unwrap()],
                            content: vec![],
                        },
                    );
                }
                *self.loop_parent_run.lock().unwrap() = Some(ctx.run_id.to_string());
                self.loop_parent_blocked.fetch_add(1, Ordering::SeqCst);
                let _guard = BlockedModel(self.loop_parent_blocked.clone());
                self.loop_parent_release.notified().await;
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::Failed {
                        error: lingxi_protocol::ProtocolError::new(
                            lingxi_protocol::ErrorCode::InvalidMessage,
                            "确定性父循环模型失败",
                            false,
                        ),
                        retryable: false,
                    },
                );
            }
            if submission == "wf-block" {
                self.blocked.fetch_add(1, Ordering::SeqCst);
                let _guard = BlockedModel(self.blocked.clone());
                std::future::pending::<()>().await;
            }
            if submission == "wf-release" {
                self.blocked.fetch_add(1, Ordering::SeqCst);
                let _guard = BlockedModel(self.blocked.clone());
                self.release.notified().await;
            }
            if submission == "wf-retryable" {
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::Failed {
                        error: lingxi_protocol::ProtocolError::new(
                            lingxi_protocol::ErrorCode::UpstreamUnavailable,
                            "temporary fixture upstream unavailable",
                            true,
                        ),
                        retryable: true,
                    },
                );
            }
            if submission == "wf-fail-after-block" {
                while self.blocked.load(Ordering::SeqCst) == 0 {
                    tokio::task::yield_now().await;
                }
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::Failed {
                        error: lingxi_protocol::ProtocolError::new(
                            lingxi_protocol::ErrorCode::InvalidMessage,
                            "fixture node failed",
                            false,
                        ),
                        retryable: false,
                    },
                );
            }
            if input.turn == 1 {
                let head = submission
                    .split("\n\n[steering]\n")
                    .next()
                    .unwrap_or(submission.as_str());
                if let Ok(command) = serde_json::from_str::<Value>(head) {
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
                self.all_results
                    .lock()
                    .unwrap()
                    .insert(ctx.run_id.to_string(), result.clone());
                // 父工具回执和并行节点回执属于不同 Run，不能用子节点结果冒充派单回执。
                let command = submission
                    .split("\n\n[steering]\n")
                    .next()
                    .unwrap_or(&submission);
                if serde_json::from_str::<Value>(command)
                    .ok()
                    .is_some_and(|value| value["tool"] == "workflow")
                {
                    self.results.lock().unwrap().push(result);
                }
                return ProviderTurnResult::of_ctx_with_usage(
                    ctx,
                    ProviderTurn::Final {
                        message: NormalizedMessage {
                            role: "assistant".into(),
                            content: vec![ContentBlock::Text {
                                text: "工具已回".into(),
                            }],
                            model_call_id: Some(call.clone()),
                        },
                    },
                    UsageRecord {
                        input_tokens: 30,
                        output_tokens: 30,
                    },
                );
            }
            ProviderTurnResult::of_ctx_with_usage(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: format!("R:{submission}"),
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
    token: String,
}

impl Harness {
    /// 固定双会话：wf-main 无授权目录（父写根为空）；wf-scope 授权 home/wf-root。
    async fn boot() -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("lingxi_service=debug")
            .with_test_writer()
            .try_init();
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-workflow-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::write(
            home.join("agents/lingxi/config.yaml"),
            "agent:\n  name: 合成助手\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("wf-root")).unwrap();
        let home = home.canonicalize().unwrap();
        Self::boot_home(home, true).await
    }

    /// 真实重启沿用home和现有会话，重新装配网关与外部模型替身。
    async fn boot_home(home: PathBuf, seed_sessions: bool) -> Self {
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(ManualClock::new(1_800_000_000_000));
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
        let access = Arc::new(
            lingxi_service::resourceaccess::ResourceAccess::new(&[home.join("wf-root")]).unwrap(),
        );
        lingxi_service::filetools::register_core_file_tools(
            gateway.registry(),
            gateway.as_ref(),
            access,
            home.join("wf-root"),
            clock.clone(),
            Arc::new(lingxi_service::filetools::NoopChangeLog),
            &SchemaBudget::default(),
        );
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
        let scope_root = home.join("wf-root").to_string_lossy().into_owned();
        for (session, folders) in [("wf-main", Vec::new()), ("wf-scope", vec![scope_root])]
            .into_iter()
            .filter(|_| seed_sessions)
        {
            state
                .session_admin()
                .create_session(
                    session,
                    "lingxi",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "合成会话",
                    Some("operate"),
                    &folders,
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
            token,
        }
    }
    fn wf_root(&self) -> PathBuf {
        self.home.join("wf-root")
    }
    fn journal_path(&self, task_id: &str) -> PathBuf {
        self.home
            .join("agents/lingxi/workflow-journals")
            .join(format!("{task_id}.jsonl"))
    }
    fn steering_pending(&self, session: &str) -> usize {
        self.state
            .sessions()
            .session_supervisor()
            .steering_pending(session)
    }
    fn progress_events(&self, task_id: &str) -> Vec<Value> {
        self.state.app_events().since(0)["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| {
                event["type"] == "workflow_progress" && event["payload"]["taskId"] == task_id
            })
            .cloned()
            .collect()
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
        self.state.storage().close().await.unwrap();
        self.home
    }
}

async fn serve_snapshot_http(
    state: &ServiceState,
) -> (
    String,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = lingxi_service::build_router(state.clone());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    (format!("http://{address}"), stop, task)
}

/// 真 Run 驱动一次 workflow 工具调用（派单）或普通对话（steering 回灌观察），
/// 等到终态。
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
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
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

/// 派单 workflow 并等到终稿回灌（steering 收件箱非空），返回 taskId。
async fn dispatch_and_wait(h: &Harness, session: &str, args: Value) -> String {
    chat(h, session, json!({"tool":"workflow","args":args})).await;
    let dispatched = h.provider.last_result();
    assert_eq!(dispatched["ok"], true, "{dispatched}");
    let task_id = dispatched["details"]["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if h.steering_pending(session) > 0 || h.provider.delivered_task(session, &task_id) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("workflow 终稿必须在期限内回灌");
    task_id
}

/// 普通对话排空 steering 收件箱，返回替身看到的提交原文（含回灌文本）。
async fn drain_steering(h: &Harness, session: &str) -> String {
    let before = h.provider.submissions().len();
    chat(h, session, json!("ping")).await;
    let delivered = h.provider.take_deliveries(session);
    if !delivered.is_empty() {
        return delivered.join("\n\n");
    }
    h.provider
        .submissions()
        .get(before)
        .expect("follow-up 提交必须存在")
        .clone()
}

const FANOUT_SCRIPT: &str = r#"export const meta = { name: "合成扇出", description: "两个只读节点扇出后汇合" };
export default async function () {
  phase("扇出");
  const [a, b] = await parallel([
    () => agent("fold alpha", { access: "read", label: "A" }),
    () => agent("fold beta", { access: "read", label: "B" }),
  ]);
  log("两侧都回来了");
  return "joined:" + a + "|" + b;
}
"#;

#[tokio::test]
async fn fanout_dispatch_events_journal_and_completion_delivery() {
    let h = Harness::boot().await;
    let task_id = dispatch_and_wait(&h, "wf-main", json!({"script": FANOUT_SCRIPT})).await;

    // runId 即回：派单结果同步可见 taskId/runId/workflow/streamStatus。
    let dispatched = h.provider.last_result();
    assert!(task_id.starts_with("task_workflow_"), "{task_id}");
    assert_eq!(dispatched["details"]["runId"], task_id);
    assert_eq!(dispatched["details"]["workflow"], "合成扇出");
    assert_eq!(dispatched["details"]["streamStatus"], "running");
    assert_eq!(dispatched["details"]["startedAt"], 1_800_000_000_000_u64);
    assert!(
        dispatched["text"].as_str().unwrap().contains(&format!(
            "Workflow \"合成扇出\" has been dispatched for background execution (task {task_id})."
        )),
        "{dispatched}"
    );

    // journal 落盘：两个 ok 节点条目，结果是替身回显，key 为 16 位 hex。
    // nodeSeq 与 step/log 共享同一计数器（现役对齐前提）：parallel 消耗 1，
    // 两个 agent 节点取 2/3，log 取 4（step/log 不落 journal）。
    let journal = std::fs::read_to_string(h.journal_path(&task_id)).expect("journal 必须生成");
    let mut entries: Vec<Value> = journal
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(entries.len(), 2, "{journal}");
    // 并行节点按真实完成顺序追加；身份和结果必须匹配，完成先后不作假设。
    entries.sort_by_key(|entry| entry["nodeSeq"].as_u64().unwrap());
    for (index, expect) in ["R:fold alpha", "R:fold beta"].iter().enumerate() {
        let entry = &entries[index];
        assert_eq!(entry["nodeSeq"], (index + 2) as u64);
        assert_eq!(entry["status"], "ok");
        assert_eq!(entry["result"], *expect);
        assert_eq!(
            entry["attempts"],
            1,
            "节点实际记录={entry}; 完整进展={:?}",
            h.progress_events(&task_id)
        );
        let key = entry["key"].as_str().unwrap();
        assert!(key.len() == 16 && key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // workflow_progress 事件流：phase 的 title 与 log 的 message 各自到达。
    let events = h.progress_events(&task_id);
    assert!(
        events
            .iter()
            .any(|event| event["payload"]["title"] == "扇出"),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event["payload"]["message"] == "两侧都回来了"),
        "{events:?}"
    );

    // 终稿回灌：下一轮提交携带 [workflow-result ... status=done] 与汇合文本。
    let drained = drain_steering(&h, "wf-main").await;
    assert!(
        drained.contains(&format!("[workflow-result task={task_id} status=done]")),
        "{drained}"
    );
    assert!(
        drained.contains("joined:R:fold alpha|R:fold beta"),
        "{drained}"
    );

    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

const RESUME_V1: &str = r#"export const meta = { name: "合成续跑", description: "顺序两节点验证断点续跑" };
export default async function () {
  const a = await agent("step one v1", { access: "read" });
  const b = await agent("step two " + a, { access: "read" });
  return b;
}
"#;

#[tokio::test]
async fn real_atomic_prompt_snapshot_writes_do_not_collide_under_os_thread_barrier() {
    const WRITERS: usize = 16;
    const ROUNDS: usize = 128;
    let h = Harness::boot().await;
    // 与实际 session_prompt::record 一致的存储目录和目标，直接调用现役真实 IO 函数。
    let parent = h.state.storage().db_path().parent().unwrap();
    let target = parent.join("session-prompt-snapshots").join(format!(
        "{}.json",
        lingxi_kernel::session_tree::sha256_hex(b"wf-main")
    ));
    let barrier = Arc::new(std::sync::Barrier::new(WRITERS));
    let mut threads = Vec::new();
    for writer in 0..WRITERS {
        let target = target.clone();
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            let mut errors = Vec::new();
            for round in 0..ROUNDS {
                let value = json!({
                    "version":1,
                    "stage":"prepared_model_call",
                    "sessionId":"wf-main",
                    "runId":format!("parallel-writer-{writer}-{round}"),
                    "turn":1,
                    "writer":writer,
                    "round":round,
                    "promptSnapshot":{
                        "version":1,
                        "systemPrompt":format!("writer-{writer}:").repeat(4096),
                        "appendSystemPrompt":[]
                    },
                    "toolNames":["read_file","workflow"]
                });
                let bytes = serde_json::to_string_pretty(&value).unwrap();
                // 每轮都同步真实线程，不替换时钟、临时名、文件系统或任何存储响应。
                barrier.wait();
                if let Err(error) =
                    lingxi_adapters::storage::agent_memory::atomic_write(&target, &bytes)
                {
                    errors.push(format!("writer={writer}, round={round}: {error:?}"));
                }
            }
            errors
        }));
    }
    let mut errors = Vec::new();
    for thread in threads {
        errors.extend(thread.join().expect("真实写线程不得 panic"));
    }
    let final_value = std::fs::read(&target)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
        });
    let temp_files: Vec<_> = std::fs::read_dir(target.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path != &target)
        .collect();
    assert!(
        errors.is_empty(),
        "现役函数 {} 次并发写发生 {} 次真实 IO 错误；样例={:?}；最终JSON={:?}；临时文件={temp_files:?}；保留home={}",
        WRITERS * ROUNDS,
        errors.len(),
        errors.iter().take(12).collect::<Vec<_>>(),
        final_value.as_ref().map(|value| (&value["writer"], &value["round"])),
        h.home.display()
    );
    let value = final_value.expect("最终结果必须是完整JSON");
    let writer = value["writer"].as_u64().unwrap() as usize;
    let round = value["round"].as_u64().unwrap() as usize;
    assert!(writer < WRITERS && round < ROUNDS);
    assert_eq!(value["runId"], format!("parallel-writer-{writer}-{round}"));
    assert_eq!(
        value["promptSnapshot"]["systemPrompt"],
        format!("writer-{writer}:").repeat(4096),
        "最终内容必须完整属于一个实际写者，不能混入其他线程的正文"
    );
    assert_eq!(value["toolNames"], json!(["read_file", "workflow"]));
    assert!(
        temp_files.is_empty(),
        "成功路径不能遗留临时文件：{temp_files:?}"
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

const RESUME_V2: &str = r#"export const meta = { name: "合成续跑", description: "顺序两节点验证断点续跑" };
export default async function () {
  const a = await agent("step one v2", { access: "read" });
  const b = await agent("step two " + a, { access: "read" });
  return b;
}
"#;

#[tokio::test]
async fn resume_reuses_unchanged_nodes_and_reruns_from_first_change() {
    let h = Harness::boot().await;

    // 首跑：两个顺序节点各一次模型调用。
    let task1 = dispatch_and_wait(&h, "wf-main", json!({"script": RESUME_V1})).await;
    assert_eq!(h.provider.node_calls("step one v1"), 1);
    assert_eq!(h.provider.node_calls("step two R:step one v1"), 1);
    let drained = drain_steering(&h, "wf-main").await;
    assert!(drained.contains(&format!("[workflow-result task={task1} status=done]")));
    assert!(drained.contains("R:step two R:step one v1"));

    // 原样重发 + resumeFromRunId：两节点全部命中缓存，替身零新调用。
    let task2 = dispatch_and_wait(
        &h,
        "wf-main",
        json!({"script": RESUME_V1, "resumeFromRunId": task1}),
    )
    .await;
    assert_ne!(task1, task2);
    assert_eq!(h.provider.node_calls("step one v1"), 1, "未变节点必须复用");
    assert_eq!(
        h.provider.node_calls("step two R:step one v1"),
        1,
        "下游节点必须随未变上游一并复用"
    );
    assert!(h
        .progress_events(&task2)
        .iter()
        .any(|event| event["payload"]["message"] == "Resuming from 2 cached entries"));
    // 命中也复制进本次 journal（现役语义：缓存命中同样 record）。
    let journal = std::fs::read_to_string(h.journal_path(&task2)).unwrap();
    assert_eq!(journal.lines().count(), 2);
    let drained = drain_steering(&h, "wf-main").await;
    assert!(drained.contains(&format!("[workflow-result task={task2} status=done]")));
    assert!(drained.contains("R:step two R:step one v1"));

    // 改第一节点重发：首处变更起全重跑（v1 不再增调，v2 链各一次）。
    let task3 = dispatch_and_wait(
        &h,
        "wf-main",
        json!({"script": RESUME_V2, "resumeFromRunId": task1}),
    )
    .await;
    assert_eq!(h.provider.node_calls("step one v2"), 1, "变更节点必须重跑");
    assert_eq!(
        h.provider.node_calls("step two R:step one v2"),
        1,
        "首处变更后下游必须全重跑"
    );
    assert_eq!(h.provider.node_calls("step one v1"), 1);
    assert_eq!(h.provider.node_calls("step two R:step one v1"), 1);
    let drained = drain_steering(&h, "wf-main").await;
    assert!(drained.contains(&format!("[workflow-result task={task3} status=done]")));
    assert!(drained.contains("R:step two R:step one v2"));

    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

fn scope_script(opts: &str) -> String {
    format!(
        r#"export const meta = {{ name: "写作域探针", description: "writeFolders 校验矩阵" }};
export default async function () {{
  const r = await agent("scope probe", {opts});
  return r;
}}
"#
    )
}

async fn expect_workflow_failure(h: &Harness, session: &str, opts: &str, expect: &str) {
    let calls_before = h.provider.node_calls("scope probe");
    let task_id = dispatch_and_wait(h, session, json!({"script": scope_script(opts)})).await;
    assert_eq!(
        h.provider.node_calls("scope probe"),
        calls_before,
        "校验失败不得派发节点"
    );
    let drained = drain_steering(h, session).await;
    assert!(
        drained.contains(&format!("[workflow-result task={task_id} status=failed]")),
        "{drained}"
    );
    assert!(drained.contains(expect), "{drained}\n应含：{expect}");
    // 失败理由须附 resumeFromRunId 指引（现役文案）。
    assert!(
        drained.contains(&format!("resumeFromRunId: \"{task_id}\"")),
        "{drained}"
    );
}

#[tokio::test]
async fn writefolders_matrix_is_fail_closed() {
    let h = Harness::boot().await;
    let root = h.wf_root();
    let root_js = format!("{:?}", root.to_string_lossy());
    let home_js = format!("{:?}", h.home.to_string_lossy());
    let missing_js = format!("{:?}", root.join("missing").to_string_lossy());
    let file_js = format!("{:?}", root.join("plain.txt").to_string_lossy());
    std::fs::write(root.join("plain.txt"), "哨兵").unwrap();

    // 正向控制：授权根内的 writeFolders 放行，节点真实执行。
    let ok_task = dispatch_and_wait(
        &h,
        "wf-scope",
        json!({"script": scope_script(&format!("{{ access: \"write\", writeFolders: [{root_js}] }}"))}),
    )
    .await;
    assert_eq!(h.provider.node_calls("scope probe"), 1);
    let drained = drain_steering(&h, "wf-scope").await;
    assert!(drained.contains(&format!("[workflow-result task={ok_task} status=done]")));
    assert!(drained.contains("R:scope probe"));

    // bootstrap 形状闸（脚本侧同步拒绝，模型零调用）。
    expect_workflow_failure(
        &h,
        "wf-scope",
        "{ access: \"write\", writeFolders: [] }",
        "workflow agent() writeFolders must be a non-empty array of absolute folder paths.",
    )
    .await;
    expect_workflow_failure(
        &h,
        "wf-scope",
        &format!("{{ access: \"read\", writeFolders: [{root_js}] }}"),
        "workflow agent() writeFolders declares a write scope and cannot be combined with access:\"read\".",
    )
    .await;
    // 未声明写域且非只读：默认拒绝（WRITE_FOLDERS_REQUIRED，含修复指引）。
    expect_workflow_failure(
        &h,
        "wf-scope",
        "{}",
        "workflow node \"node-1\" is write-capable but declares no writeFolders.",
    )
    .await;

    // 原生复验闸（存在性/类型/包含，逐字文案）。
    expect_workflow_failure(
        &h,
        "wf-scope",
        &format!("{{ access: \"write\", writeFolders: [{home_js}] }}"),
        "workflow agent() writeFolders entry escapes the parent session folder scope:",
    )
    .await;
    expect_workflow_failure(
        &h,
        "wf-scope",
        &format!("{{ access: \"write\", writeFolders: [{missing_js}] }}"),
        "workflow agent() writeFolders entry does not exist:",
    )
    .await;
    expect_workflow_failure(
        &h,
        "wf-scope",
        &format!("{{ access: \"write\", writeFolders: [{file_js}] }}"),
        "workflow agent() writeFolders entry is not a directory:",
    )
    .await;
    // 父会话无任何写根（wf-main 无 cwd/workspace/授权目录）→ 包含判定不可用。
    expect_workflow_failure(
        &h,
        "wf-main",
        &format!("{{ access: \"write\", writeFolders: [{root_js}] }}"),
        "workflow agent() writeFolders requires the parent session folder scope, which is unavailable here.",
    )
    .await;

    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn invalid_meta_is_rejected_synchronously_at_dispatch() {
    let h = Harness::boot().await;

    // 缺 meta marker：工具调用同步失败，不派单、无后台任务。
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":"export default async function () { return 1; }"}}),
    )
    .await;
    let rejected = h.provider.last_result();
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(
        rejected["error"].as_str().unwrap(),
        "Workflow script invalid: workflow script 必须以 export const meta = {...} 开头"
    );

    // meta.nodes 声明式数组：同步拒绝（现役 declarativeNodesUnsupported 文案）。
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":"export const meta = { name: \"x\", description: \"y\", nodes: [] };\nexport default async function () { return 1; }"}}),
    )
    .await;
    let rejected = h.provider.last_result();
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(
        rejected["error"].as_str().unwrap(),
        "workflow meta.nodes is declarative metadata and is not executable yet; use agent()/parallel()/phase()/log() in the script body."
    );
    // 同步拒绝 = 从未派单：无 workflow_progress 事件、无 journal 文件。
    assert!(h.state.app_events().since(0)["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| event["type"] != "workflow_progress"));
    let journals = h.home.join("agents/lingxi/workflow-journals");
    assert!(
        !journals.exists() || std::fs::read_dir(&journals).unwrap().next().is_none(),
        "同步拒绝不得生成 journal"
    );

    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn budget_tokens_stops_the_second_node() {
    let h = Harness::boot().await;
    const BUDGET_SCRIPT: &str = r#"export const meta = { name: "预算闸门", description: "第二节点前预算落闸" };
export default async function () {
  const a = await agent("budget node one", { access: "read" });
  const b = await agent("budget node two", { access: "read" });
  return a + "|" + b;
}
"#;
    // 替身每轮报 30+30=60 token；预算 1 → 第一节点放行（0<1），落账 60；
    // 第二节点派发前查询命中硬闸。
    let task_id = dispatch_and_wait(
        &h,
        "wf-main",
        json!({"script": BUDGET_SCRIPT, "args": {"budgetTokens": 1}}),
    )
    .await;
    assert_eq!(h.provider.node_calls("budget node one"), 1);
    assert_eq!(
        h.provider.node_calls("budget node two"),
        0,
        "预算闸门后不得派发"
    );
    let drained = drain_steering(&h, "wf-main").await;
    assert!(drained.contains(&format!("[workflow-result task={task_id} status=failed]")));
    assert!(
        drained.contains("workflow token 预算耗尽（已用 60）"),
        "{drained}"
    );

    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

/// 真正写文件，验证节点目录会改变默认落点，父授权内的兄弟目录仍被拒绝。
#[tokio::test]
async fn node_write_folders_bind_actual_file_mutations_and_tool_filter() {
    let h = Harness::boot().await;
    let child = h.wf_root().join("narrow");
    std::fs::create_dir(&child).unwrap();
    let child_js = format!("{:?}", child.to_string_lossy());
    for (path, filter, expected) in [
        ("landed.txt".to_string(), "", true),
        (
            h.wf_root()
                .join("forbidden.txt")
                .to_string_lossy()
                .into_owned(),
            "",
            false,
        ),
        ("filtered.txt".to_string(), ", toolFilter: []", false),
    ] {
        let command =
            json!({"tool":"write","args":{"path":path,"content":"真实节点产物"}}).to_string();
        let script = format!("export const meta = {{name:'写域检查',description:'d'}}; return await agent({:?}, {{access:'write',writeFolders:[{child_js}]{filter}}});", command);
        dispatch_and_wait(&h, "wf-scope", json!({"script":script})).await;
        let actual = if std::path::Path::new(&path).is_absolute() {
            PathBuf::from(&path)
        } else {
            child.join(&path)
        };
        assert_eq!(actual.exists(), expected, "{path}");
        if expected {
            assert_eq!(std::fs::read_to_string(actual).unwrap(), "真实节点产物");
        }
        drain_steering(&h, "wf-scope").await;
    }
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn actual_task_stop_route_joins_workflow_and_running_model() {
    let h = Harness::boot().await;
    let script = "export const meta = {name:'可取消工作流',description:'d'}; return await agent('wf-block',{access:'read'});";
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":script}}),
    )
    .await;
    let task = h.provider.last_result()["details"]["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.blocked.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = lingxi_service::build_router(h.state.clone());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    let result = reqwest::Client::new()
        .post(format!("http://{addr}/lingxi/v1/task/{task}/abort"))
        .bearer_auth(&h.token)
        .json(&json!({"sessionId":"wf-main"}))
        .send()
        .await
        .unwrap();
    let status = result.status();
    let body = result.text().await.unwrap();
    assert_eq!(status.as_u16(), 200, "{body}");
    let result: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(result["status"], "aborted", "{result}");
    assert_eq!(
        h.provider.blocked.load(Ordering::SeqCst),
        0,
        "模型请求必须真实退出"
    );
    assert_eq!(h.state.workflows().active_count(), 0);
    assert_eq!(
        h.state
            .workflows()
            .activities_for_session("wf-main")
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == task)
            .unwrap()["status"],
        "aborted"
    );
    stop.send(()).unwrap();
    server.await.unwrap();
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn first_node_failure_cancels_parallel_blocked_sibling() {
    let h = Harness::boot().await;
    // 第一个节点确已挂起后，第二节点才返回失败；不能留下仍在请求模型的兄弟节点。
    let script = "export const meta={name:'失败回收',description:'d'}; return await Promise.all([agent('wf-block',{access:'read'}),agent('wf-fail-after-block',{access:'read',retries:0})]);";
    dispatch_and_wait(&h, "wf-main", json!({"script":script})).await;
    assert_eq!(h.provider.blocked.load(Ordering::SeqCst), 0);
    assert_eq!(h.state.workflows().active_count(), 0);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn resume_requires_same_session_and_agent_before_reading_cached_results() {
    let h = Harness::boot().await;
    let script = "export const meta={name:'归属检查',description:'d'}; return await agent('session-private-result',{access:'read'});";
    let old = dispatch_and_wait(&h, "wf-main", json!({"script":script})).await;
    drain_steering(&h, "wf-main").await;
    assert_eq!(h.provider.node_calls("session-private-result"), 1);
    // 同一会话可续跑并命中缓存。
    dispatch_and_wait(
        &h,
        "wf-main",
        json!({"script":script,"resumeFromRunId":old}),
    )
    .await;
    drain_steering(&h, "wf-main").await;
    assert_eq!(h.provider.node_calls("session-private-result"), 1);
    // 同一助手的另一个会话也不能读取此缓存，调用即失败，不另发模型节点。
    chat(
        &h,
        "wf-scope",
        json!({"tool":"workflow","args":{"script":script,"resumeFromRunId":old}}),
    )
    .await;
    let result = h.provider.last_result();
    assert_eq!(result["ok"], false, "{result}");
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("does not belong"));
    assert_eq!(h.provider.node_calls("session-private-result"), 1);
    assert_eq!(h.steering_pending("wf-scope"), 0);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn damaged_resume_journal_is_rejected_without_rerunning_nodes() {
    let h = Harness::boot().await;
    let script = "export const meta={name:'损坏恢复检查',description:'d'}; return await agent('landed-before-corruption',{access:'read'});";
    let old = dispatch_and_wait(&h, "wf-main", json!({"script":script})).await;
    drain_steering(&h, "wf-main").await;
    let journal = h.journal_path(&old);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&journal)
        .unwrap()
        .write_all(b"partial-invalid-record\n")
        .unwrap();
    let raw = std::fs::read(&journal).unwrap();
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":script,"resumeFromRunId":old}}),
    )
    .await;
    let result = h.provider.last_result();
    assert_eq!(result["ok"], false, "{result}");
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("automatic re-execution is refused"));
    assert_eq!(h.provider.node_calls("landed-before-corruption"), 1);
    assert_eq!(std::fs::read(journal).unwrap(), raw);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn journal_write_failure_reports_unsafe_resume_and_unrelated_run_cannot_cancel() {
    let h = Harness::boot().await;
    let old_run = chat(&h, "wf-main", json!("unrelated historical run")).await;
    let script = "export const meta={name:'记录失败检查',description:'d'}; return await agent('wf-block',{access:'read'});";
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":script}}),
    )
    .await;
    let task = h.provider.last_result()["details"]["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.blocked.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(h.state.workflows().cancel_for_run(&old_run).is_empty());
    assert_eq!(h.provider.blocked.load(Ordering::SeqCst), 1);
    let journal = h.journal_path(&task);
    std::fs::remove_file(&journal).unwrap();
    std::fs::create_dir(&journal).unwrap();
    h.state
        .workflows()
        .cancel_task("wf-main", &task, std::time::Duration::from_secs(5))
        .await
        .unwrap()
        .unwrap();
    let result = drain_steering(&h, "wf-main").await;
    assert!(result.contains("断点未确认保存，不能自动恢复"), "{result}");
    assert!(!result.contains("可用 resumeFromRunId:"), "{result}");
    let entry = h
        .state
        .workflows()
        .activities_for_session("wf-main")
        .unwrap()
        .into_iter()
        .find(|entry| entry["id"] == task)
        .unwrap();
    assert_eq!(entry["resumeSafe"], false);
    assert_eq!(entry["status"], "failed");
    assert_eq!(h.provider.blocked.load(Ordering::SeqCst), 0);
    assert_eq!(h.state.workflows().active_count(), 0);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

/// 真实节点已完成后破坏断点文件，后续节点结果不能覆盖/重建旧证据，恢复也不能重跑副作用。
#[tokio::test]
async fn deleted_truncated_or_replaced_live_journal_refuses_resume_and_preserves_fault() {
    for fault in ["delete", "truncate", "replace"] {
        let h = Harness::boot().await;
        let script = "export const meta={name:'断点连续性检查',description:'d'}; await agent('landed-before-gap',{access:'read'}); return await agent('wf-release',{access:'read'});";
        chat(
            &h,
            "wf-main",
            json!({"tool":"workflow","args":{"script":script}}),
        )
        .await;
        let task = h.provider.last_result()["details"]["taskId"]
            .as_str()
            .unwrap()
            .to_string();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while h.provider.blocked.load(Ordering::SeqCst) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // 普通会话准入在真实节点运行时保持互斥，运行中回执必须明确标为不可恢复。
        let live_entries = h
            .state
            .workflows()
            .activities_for_session("wf-main")
            .unwrap();
        let running = live_entries
            .iter()
            .find(|entry| entry["id"] == task)
            .unwrap();
        assert_eq!(running["status"], "running");
        assert_eq!(running["resumeSafe"], false);
        assert!(h.state.storage().session_admission().is_busy("wf-main"));
        assert_eq!(h.provider.node_calls("landed-before-gap"), 1);
        let path = h.journal_path(&task);
        let original = std::fs::read(&path).unwrap();
        assert_eq!(
            String::from_utf8(original.clone()).unwrap().lines().count(),
            1
        );
        match fault {
            "delete" => std::fs::remove_file(&path).unwrap(),
            "truncate" => std::fs::write(&path, b"").unwrap(),
            "replace" => {
                std::fs::rename(&path, path.with_extension("original")).unwrap();
                std::fs::write(&path, &original).unwrap();
            }
            _ => unreachable!(),
        }
        h.provider.release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while h.state.workflows().has_active_for_session("wf-main") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!h.state.storage().session_admission().is_busy("wf-main"));
        let delivered = drain_steering(&h, "wf-main").await;
        assert!(
            delivered.contains("断点未确认保存，不能自动恢复"),
            "{fault}: {delivered}"
        );
        let entry = h
            .state
            .workflows()
            .activities_for_session("wf-main")
            .unwrap()
            .into_iter()
            .find(|entry| entry["id"] == task)
            .unwrap();
        assert_eq!(entry["resumeSafe"], false, "{fault}: {entry}");
        assert_eq!(entry["status"], "failed", "{fault}: {entry}");
        chat(
            &h,
            "wf-main",
            json!({"tool":"workflow","args":{"script":script,"resumeFromRunId":task}}),
        )
        .await;
        assert_eq!(h.provider.last_result()["ok"], false, "{fault}");
        assert_eq!(h.provider.node_calls("landed-before-gap"), 1, "{fault}");
        assert_eq!(h.provider.node_calls("wf-release"), 1, "{fault}");
        assert_eq!(h.provider.blocked.load(Ordering::SeqCst), 0);
        assert_eq!(h.state.workflows().active_count(), 0);
        match fault {
            "delete" => assert!(!path.exists()),
            "truncate" => assert_eq!(std::fs::read(&path).unwrap(), b""),
            "replace" => assert_eq!(std::fs::read(&path).unwrap(), original),
            _ => unreachable!(),
        }
        let root = h.close().await;
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn completed_journal_receipt_rejects_valid_prefix_truncation_and_same_bytes_replacement() {
    for fault in ["truncate", "replace"] {
        let h = Harness::boot().await;
        let script = "export const meta={name:'终态断点检查',description:'d'}; await agent('completed-first',{access:'read'}); return await agent('completed-second',{access:'read'});";
        let task = dispatch_and_wait(&h, "wf-main", json!({"script":script})).await;
        drain_steering(&h, "wf-main").await;
        let path = h.journal_path(&task);
        let original = std::fs::read(&path).unwrap();
        let damaged = if fault == "truncate" {
            original[..=original.iter().position(|byte| *byte == b'\n').unwrap()].to_vec()
        } else {
            std::fs::rename(&path, path.with_extension("original")).unwrap();
            original
        };
        std::fs::write(&path, &damaged).unwrap();
        chat(
            &h,
            "wf-main",
            json!({"tool":"workflow","args":{"script":script,"resumeFromRunId":task}}),
        )
        .await;
        let result = h.provider.last_result();
        assert_eq!(result["ok"], false, "{fault}: {result}");
        assert!(
            result["error"]
                .as_str()
                .unwrap()
                .contains("automatic re-execution is refused"),
            "{result}"
        );
        assert_eq!(h.provider.node_calls("completed-first"), 1);
        assert_eq!(h.provider.node_calls("completed-second"), 1);
        assert_eq!(std::fs::read(&path).unwrap(), damaged);
        let root = h.close().await;
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// 父层和子层的相同调用必须各执行一次，并在子层执行前明确停用整个任务的自动恢复。
#[tokio::test]
async fn nested_and_outer_nodes_share_unique_sequences_and_refuse_unsafe_resume() {
    let h = Harness::boot().await;
    let child = "export const meta={name:'子流程',description:'d'}; await agent('same-outer-and-nested',{access:'read'}); return await agent('wf-release',{access:'read'});";
    let script = "export const meta={name:'嵌套编号检查',description:'d'}; const outer=await agent('same-outer-and-nested',{access:'read'}); const child=await workflow(__CHILD_SCRIPT__,{}); return outer+'|'+child;"
        .replace("__CHILD_SCRIPT__", &serde_json::to_string(child).unwrap());
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":script}}),
    )
    .await;
    let task = h.provider.last_result()["details"]["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.blocked.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(h.provider.node_calls("same-outer-and-nested"), 2);
    assert!(h.state.storage().session_admission().is_busy("wf-main"));
    let persisted: Value =
        serde_json::from_slice(&std::fs::read(h.home.join("workflow-activity.json")).unwrap())
            .unwrap();
    assert_eq!(persisted["entries"][&task]["resumeSafe"], false);
    assert_eq!(
        persisted["entries"][&task]["resumeBlockedReason"],
        "nested_workflow"
    );
    let entries = h
        .state
        .workflows()
        .activities_for_session("wf-main")
        .unwrap();
    let nodes: Vec<_> = entries
        .iter()
        .filter(|entry| entry["kind"] == "workflow_agent" && entry["parentTaskId"] == task)
        .collect();
    assert_eq!(nodes.len(), 3, "{entries:?}");
    let mut node_ids: Vec<_> = nodes
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    node_ids.sort();
    node_ids.dedup();
    assert_eq!(node_ids.len(), 3);
    h.provider.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.state.workflows().has_active_for_session("wf-main") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!h.state.storage().session_admission().is_busy("wf-main"));
    let delivered = drain_steering(&h, "wf-main").await;
    assert!(
        delivered.contains("此任务包含嵌套工作流，不能自动断点恢复"),
        "{delivered}"
    );
    let journal_path = h.journal_path(&task);
    let raw = std::fs::read_to_string(&journal_path).unwrap();
    let mut records: Vec<Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    records.sort_by_key(|entry| entry["nodeSeq"].as_u64().unwrap());
    assert_eq!(records.len(), 3);
    assert_eq!(
        records
            .iter()
            .map(|row| row["nodeSeq"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(records[0]["key"], records[1]["key"]);
    assert!(records.iter().all(|row| row["status"] == "ok"));
    let entries = h
        .state
        .workflows()
        .activities_for_session("wf-main")
        .unwrap();
    assert!(entries
        .iter()
        .filter(|entry| entry["kind"] == "workflow_agent" && entry["parentTaskId"] == task)
        .all(|entry| entry["status"] == "done"));
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":script,"resumeFromRunId":task}}),
    )
    .await;
    let result = h.provider.last_result();
    assert_eq!(result["ok"], false, "{result}");
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("contains nested workflows"),
        "{result}"
    );
    assert_eq!(h.provider.node_calls("same-outer-and-nested"), 2);
    assert_eq!(h.provider.node_calls("wf-release"), 1);
    assert_eq!(std::fs::read_to_string(&journal_path).unwrap(), raw);
    assert_eq!(h.provider.blocked.load(Ordering::SeqCst), 0);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn workflow_without_live_node_does_not_hold_session_admission() {
    let h = Harness::boot().await;
    let script = "export const meta={name:'后台空闲许可检查',description:'d'}; return await agent('wf-retryable',{access:'read',retries:3});";
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":script}}),
    )
    .await;
    let task = h.provider.last_result()["details"]["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.node_calls("wf-retryable") == 0
            || h.state.storage().session_admission().is_busy("wf-main")
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(h.state.workflows().has_active_for_session("wf-main"));
    assert_eq!(h.provider.blocked.load(Ordering::SeqCst), 0);
    // 临时失败后的真实退避期间没有节点运行，普通用户对话必须能实际完成。
    chat(&h, "wf-main", json!("foreground-while-workflow-idle")).await;
    assert!(h.state.workflows().has_active_for_session("wf-main"));
    assert!(h
        .provider
        .submissions()
        .iter()
        .any(|text| text == "\"foreground-while-workflow-idle\""));
    h.state
        .workflows()
        .cancel_task("wf-main", &task, std::time::Duration::from_secs(5))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(h.state.workflows().active_count(), 0);
    assert!(!h.state.storage().session_admission().is_busy("wf-main"));
    drain_steering(&h, "wf-main").await;
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn completed_workflow_wakes_running_idle_loop_once_after_cleanup() {
    let h = Harness::boot().await;
    let controller = h.state.loop_services().controller.clone();
    let target = lingxi_service::loopx::LoopTarget {
        kind: "desktop".into(),
        session_id: "wf-main".into(),
    };
    controller.start(&target, "等待后台工作完成").await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while controller.tool_status("wf-main").unwrap().turn_count != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let script = "export const meta={name:'完成后唤醒',description:'d'}; return await agent('wf-release',{access:'read'});";
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":script}}),
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.blocked.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(h.state.workflows().has_active_for_session("wf-main"));
    h.provider.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if h.provider
                .submissions()
                .iter()
                .any(|text| text.contains("Reason: background workflow completed"))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!h.state.workflows().has_active_for_session("wf-main"));
    assert_eq!(h.provider.blocked.load(Ordering::SeqCst), 0);
    assert_eq!(
        h.provider
            .submissions()
            .iter()
            .filter(|text| text.contains("Reason: background workflow completed"))
            .count(),
        1
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn activity_receipt_failure_never_blocks_real_cancel_or_fabricates_task_status() {
    let h = Harness::boot().await;
    let script = "export const meta={name:'活动回执故障',description:'d'}; return await agent('wf-block',{access:'read'});";
    chat(
        &h,
        "wf-main",
        json!({"tool":"workflow","args":{"script":script}}),
    )
    .await;
    let task = h.provider.last_result()["details"]["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.blocked.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let activity = h.home.join("workflow-activity.json");
    std::fs::rename(
        &activity,
        h.home.join("workflow-activity.before-fault.json"),
    )
    .unwrap();
    std::fs::create_dir(&activity).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = lingxi_service::build_router(h.state.clone());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{address}/lingxi/v1/task/{task}/abort"))
        .bearer_auth(&h.token)
        .json(&json!({"sessionId":"wf-main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 500);
    let body = response.text().await.unwrap();
    assert!(body.contains("活动回执未确认"), "{body}");
    assert!(body.contains("资源已回收"), "{body}");
    assert_eq!(
        h.provider.blocked.load(Ordering::SeqCst),
        0,
        "回执损坏也必须停止实际模型请求"
    );
    assert_eq!(h.state.workflows().active_count(), 0);
    assert!(h
        .state
        .workflows()
        .activities_for_session("wf-main")
        .is_err());
    assert!(h.state.workflows().list_for_session("wf-main").is_err());
    let snapshot = client
        .get(format!(
            "http://{address}/lingxi/v1/sessions/wf-main/agent-activities"
        ))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();
    assert_eq!(snapshot.status().as_u16(), 500, "不得继续发送旧running快照");
    let query_run = chat(
        &h,
        "wf-main",
        json!({"tool":"check_pending_tasks","args":{}}),
    )
    .await;
    let query = h
        .provider
        .all_results
        .lock()
        .unwrap()
        .get(&query_run)
        .unwrap()
        .clone();
    assert_eq!(query["ok"], false, "{query}");
    assert!(
        query["error"].as_str().unwrap().contains("unconfirmed"),
        "{query}"
    );
    let delivered = h.provider.take_deliveries("wf-main").join("\n");
    assert!(
        delivered.contains("activity-receipt-unconfirmed"),
        "{delivered}"
    );
    assert!(
        h.state
            .workflows()
            .cancel_task("wf-main", &task, std::time::Duration::from_secs(1))
            .await
            .is_err(),
        "无在册任务且回执未知不能冒称aborted"
    );
    stop.send(()).unwrap();
    server.await.unwrap();
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn actual_http_activity_snapshot_checks_owner_and_survives_same_home_restart() {
    let h = Harness::boot().await;
    let task = dispatch_and_wait(&h, "wf-main", json!({"script":FANOUT_SCRIPT})).await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.state.workflows().active_count() != 0
            || h.state.sessions().session_supervisor().is_busy("wf-main")
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("真实工作流和所属执行轮必须收尾后读取冷快照");
    let count_before = h.state.storage().count_runs("wf-main").await.unwrap();
    let (address, stop, server) = serve_snapshot_http(&h.state).await;
    let endpoint = format!("{address}/lingxi/v1/sessions/wf-main/agent-activities");
    let client = reqwest::Client::new();
    let response = client
        .get(&endpoint)
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let before: Value = response.json().await.unwrap();
    let typed: lingxi_protocol::AgentActivitySnapshot =
        serde_json::from_value(before.clone()).unwrap();
    let main = typed.entries.iter().find(|entry| entry.id == task).unwrap();
    assert_eq!(main.session_id.as_deref(), Some("wf-main"));
    assert_eq!(main.session_path.as_deref(), Some("native:wf-main"));
    assert_eq!(main.status, lingxi_protocol::AgentActivityStatus::Done);
    assert!(main.finished_at.is_some());
    assert!(main.child_session_path.is_none());
    assert_eq!(main.extra.get("resumeSafe"), Some(&json!(true)));
    assert!(main.extra["journalReceipt"].is_object());
    assert_eq!(
        serde_json::to_value(typed).unwrap(),
        before,
        "正式生成类型须保留真实null和扩展回执字段"
    );

    let no_scope = h
        .state
        .auth()
        .issue_device_credential("user_local", &[], None)
        .unwrap();
    let denied = client
        .get(&endpoint)
        .bearer_auth(&no_scope.secret)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status().as_u16(), 403);
    let denied: Value = denied.json().await.unwrap();
    assert_eq!(denied["details"]["requiredScope"], "chat");
    let other_owner = h
        .state
        .auth()
        .issue_device_credential("user_other", &["chat"], None)
        .unwrap();
    let denied = client
        .get(&endpoint)
        .bearer_auth(&other_owner.secret)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status().as_u16(), 403);
    assert!(
        !denied.text().await.unwrap().contains(&task),
        "拒绝响应不得披露工作流条目"
    );
    let reader = h
        .state
        .auth()
        .issue_device_credential("user_local", &["chat"], None)
        .unwrap();
    let allowed = client
        .get(&endpoint)
        .bearer_auth(&reader.secret)
        .send()
        .await
        .unwrap();
    assert_eq!(allowed.status().as_u16(), 200);
    assert_eq!(allowed.json::<Value>().await.unwrap(), before);
    assert!(h
        .state
        .auth()
        .revoke_device_credential(&reader.credential_id)
        .unwrap());
    let revoked = client
        .get(&endpoint)
        .bearer_auth(&reader.secret)
        .send()
        .await
        .unwrap();
    assert_eq!(revoked.status().as_u16(), 401);
    let missing_auth = client.get(&endpoint).send().await.unwrap();
    assert_eq!(missing_auth.status().as_u16(), 401);
    stop.send(()).unwrap();
    server.await.unwrap();
    let home = h.close().await;

    // 同home真正重建服务、存储、执行器与HTTP路由，不直接调用load_activities。
    let restarted = Harness::boot_home(home, false).await;
    let (address, stop, server) = serve_snapshot_http(&restarted.state).await;
    let endpoint = format!("{address}/lingxi/v1/sessions/wf-main/agent-activities");
    let response = client
        .get(&endpoint)
        .bearer_auth(&restarted.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let after: Value = response.json().await.unwrap();
    assert_eq!(after, before, "已完成工作流的真实冷快照应在重启后完整恢复");
    assert_eq!(
        restarted
            .state
            .storage()
            .count_runs("wf-main")
            .await
            .unwrap(),
        count_before
    );
    assert_eq!(restarted.state.workflows().active_count(), 0);
    assert!(
        restarted.provider.submissions().is_empty(),
        "读取冷快照不得重放节点或调用模型"
    );
    let revoked = client
        .get(&endpoint)
        .bearer_auth(&reader.secret)
        .send()
        .await
        .unwrap();
    assert_eq!(
        revoked.status().as_u16(),
        401,
        "撤销回执必须随真实存储重启恢复"
    );
    stop.send(()).unwrap();
    server.await.unwrap();
    let root = restarted.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn loop_parent_is_counted_only_after_its_real_terminal_not_child_completion() {
    let h = Harness::boot().await;
    let (address, stop, server) = serve_snapshot_http(&h.state).await;
    let response = reqwest::Client::new()
        .post(format!("{address}/lingxi/v1/sessions/wf-main/slash"))
        .bearer_auth(&h.token)
        .json(&json!({"text":"/loop loop-parent-child-accounting"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let started: Value = response.json().await.unwrap();
    assert_eq!(started["results"][0]["level"], "success", "{started}");
    // 只控制外部模型的两个独立响应：子节点与父工具后的下一次模型请求。
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.blocked.load(Ordering::SeqCst) != 1
            || h.provider.loop_parent_blocked.load(Ordering::SeqCst) != 1
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("真实子节点和真实父Run的后续模型请求均已进入");
    let parent_run = h.provider.loop_parent_run.lock().unwrap().clone().unwrap();
    assert_eq!(
        h.state
            .loop_services()
            .controller
            .tool_status("wf-main")
            .unwrap()
            .turn_count,
        0
    );
    h.provider.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.blocked.load(Ordering::SeqCst) != 0
            || h.state.workflows().active_count() != 0
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("真实子节点必须完成且工作流清理结束");
    assert_eq!(h.provider.loop_parent_blocked.load(Ordering::SeqCst), 1);
    let parent = h
        .state
        .storage()
        .load_run(&lingxi_protocol::RunId::new(&parent_run))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parent.status, lingxi_protocol::RunStatus::Running);
    let record = h
        .state
        .loop_services()
        .controller
        .tool_status("wf-main")
        .unwrap();
    assert_eq!(record.turn_count, 0, "子Run终态不得结算仍挂住的父Loop轮");
    assert_eq!(record.consecutive_failures, 0);
    assert!(h
        .state
        .workflows()
        .activities_for_session("wf-main")
        .unwrap()
        .iter()
        .any(|entry| entry["kind"] == "workflow" && entry["status"] == "done"));

    h.provider.loop_parent_release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let actual = h
                .state
                .storage()
                .load_run(&lingxi_protocol::RunId::new(&parent_run))
                .await
                .unwrap()
                .unwrap();
            let record = h
                .state
                .loop_services()
                .controller
                .tool_status("wf-main")
                .unwrap();
            if actual.status == lingxi_protocol::RunStatus::Failed
                && record.turn_count == 1
                && record.consecutive_failures == 1
                && !h.state.sessions().session_supervisor().is_busy("wf-main")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("只有父Run实际Failed后才能准确结算一次失败轮");
    assert_eq!(h.provider.loop_parent_blocked.load(Ordering::SeqCst), 0);
    let terminal_count = h.state.storage().query_one_text(
        "SELECT COUNT(*) FROM key_events WHERE run_id=?1 AND event_type='run_state_changed' AND payload_json LIKE '%\"to\":\"failed\"%'",
        vec![parent_run],
    ).await.unwrap();
    assert_eq!(terminal_count.as_deref(), Some("1"));
    h.state
        .loop_services()
        .controller
        .stop(&lingxi_service::loopx::LoopTarget::desktop("wf-main"))
        .await
        .unwrap();
    stop.send(()).unwrap();
    server.await.unwrap();
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
