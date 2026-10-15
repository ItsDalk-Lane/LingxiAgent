//! 三个原工具叶：真实注册、审批、运行、存储和供应商 HTTP 替身；核心路径不模拟。
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolOutcome, ToolRequest,
        ToolSuccess, TurnDeltaSink, TurnProviderPort,
    },
    subagent::SessionPermissionMode,
    toolcatalog::{PermissionKind, SchemaBudget, ToolRegistry},
    RunContext,
};
use lingxi_protocol::{
    ContentBlock, NormalizedMessage, ToolCallId, ToolResultStatus, ToolResultWire,
};
use lingxi_service::{
    approval_service::{Answer, AnswerOutcome, ApprovalService},
    inject::SystemClock,
    memory::MemoryService,
    prepare_layout,
    skills::SkillService,
    toolgateway::{CallerSurface, InvocationPermissionContext, ToolInvocationGateway},
    HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

fn dir(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lingxi-r06-tool-completion-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}
fn agents(home: &Path) {
    for id in ["lingxi", "other"] {
        std::fs::create_dir_all(home.join("agents").join(id).join("memory")).unwrap();
        std::fs::write(
            home.join("agents").join(id).join("config.yaml"),
            "agent:\n  name: 合成助手\nmemory:\n  enabled: true\nexperience:\n  enabled: true\n",
        )
        .unwrap();
    }
}
fn snapshot(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(path: &Path, root: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        if !path.exists() {
            return;
        }
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, root, out);
            } else {
                out.push((
                    path.strip_prefix(root).unwrap().into(),
                    std::fs::read(&path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    collect(path, path, &mut out);
    out.sort();
    out
}
fn ctx(session: &str) -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(session),
        run_id: lingxi_protocol::RunId::new("run-tool-completion"),
        attempt: lingxi_protocol::AttemptId::new("run-tool-completion#a1"),
        generation: 1,
    }
}
struct Tools {
    memory: Arc<MemoryService>,
    registry: Arc<ToolRegistry>,
    gateway: Arc<ToolInvocationGateway>,
    approvals: Arc<ApprovalService>,
    agent: String,
}
fn bind_tools(home: &Path, agent: &str, memory: Option<Arc<MemoryService>>) -> Tools {
    let memory =
        memory.unwrap_or_else(|| Arc::new(MemoryService::new(home.into(), home.join("runtime"))));
    let registry = Arc::new(ToolRegistry::new());
    let budget = SchemaBudget::default();
    let approvals = Arc::new(ApprovalService::with_bounds(
        Arc::new(SystemClock),
        8_000,
        64,
        64,
    ));
    let gateway = Arc::new(ToolInvocationGateway::new(
        registry.clone(),
        approvals.clone(),
        Arc::new(SystemClock),
        budget,
        30_000,
        64,
    ));
    lingxi_service::memorytools::register_memory_tools(
        &registry,
        &gateway,
        memory.clone(),
        Arc::new(SkillService::new(home.into(), home.join("product"))),
        agent.into(),
        Arc::new(SystemClock),
        &budget,
    );
    Tools {
        memory,
        registry,
        gateway,
        approvals,
        agent: agent.into(),
    }
}
impl Tools {
    async fn call(
        &self,
        name: &str,
        args: Value,
        sequence: usize,
        session: &str,
    ) -> Result<ToolOutcome, String> {
        let request = ToolRequest::from_effective_arguments(
            format!("tool:first-party:{name}"),
            args,
            &SchemaBudget::default(),
        )
        .unwrap();
        let call = ToolCallId::new(format!("completion-{sequence}"));
        let context = ctx(session);
        let prepared = self
            .gateway
            .prepare_from_request(
                &context,
                CallerSurface::UserRun,
                &self.agent,
                InvocationPermissionContext::UserSession {
                    mode: SessionPermissionMode::Operate,
                },
                &call,
                &request,
            )
            .map_err(|error| format!("{error:?}"))?;
        self.gateway
            .execute_prepared(&context, &call, &prepared.handle)
            .await
            .map(|value| value.outcome)
            .map_err(|error| format!("{error:?}"))
    }
}
async fn mismatched_agent_call(
    tools: &Tools,
    tool: &str,
    args: Value,
    sequence: usize,
) -> ToolOutcome {
    let context = ctx("sess_local_alpha");
    let call = ToolCallId::new(format!("mismatch-{sequence}"));
    let request = ToolRequest::from_effective_arguments(
        format!("tool:first-party:{tool}"),
        args,
        &SchemaBudget::default(),
    )
    .unwrap();
    let prepared = tools
        .gateway
        .prepare_from_request(
            &context,
            CallerSurface::UserRun,
            "other",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &call,
            &request,
        )
        .unwrap();
    tools
        .gateway
        .execute_prepared(&context, &call, &prepared.handle)
        .await
        .unwrap()
        .outcome
}
fn success(outcome: ToolOutcome) -> ToolSuccess {
    match outcome {
        ToolOutcome::Success { result } => result,
        other => panic!("expected success: {other:?}"),
    }
}
fn text(result: &ToolSuccess) -> String {
    result
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn details(result: &ToolSuccess) -> Value {
    Value::Object(result.details.clone().expect("原义务要求真正的结构化结果"))
}
fn verify_wire_details(result: &ToolSuccess) {
    let wire = ToolResultWire {
        details: result.details.clone(),
        status: ToolResultStatus::Success,
        content: result.content.clone(),
        resource_refs: result.resource_refs.clone(),
        truncated: result.truncated,
        error: None,
    };
    let encoded = serde_json::to_value(&wire).unwrap();
    assert!(encoded["details"].is_object());
    let restored: ToolResultWire = serde_json::from_value(encoded).unwrap();
    assert_eq!(restored.details, result.details);
}
async fn verify_archived_details(state: &ServiceState, expected: &Value) {
    let history = lingxi_service::model_history::ModelHistory::new(state.storage().clone())
        .load(
            &lingxi_kernel::Principal::LocalUser,
            "lingxi",
            "sess_local_alpha",
            "later-run",
        )
        .await
        .unwrap();
    assert!(history.iter().any(|item| matches!(item, ExchangeItem::ToolResult { outcome: ToolOutcome::Success { result }, .. } if result.details.as_ref().map(|fields| Value::Object(fields.clone())).as_ref() == Some(expected))));
}
fn owner() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".into(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.into()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".into()],
    }
}
#[derive(Default)]
struct Supplier {
    script: Mutex<VecDeque<ProviderTurn>>,
    outcomes: Mutex<Vec<ToolOutcome>>,
    priors: Mutex<Vec<Vec<ExchangeItem>>>,
}
impl Supplier {
    fn queue(&self, tool: &str, args: Value) {
        self.script
            .lock()
            .unwrap()
            .push_back(ProviderTurn::ToolRequests {
                content: Vec::new(),
                requests: vec![ToolRequest::from_effective_arguments(
                    format!("tool:first-party:{tool}"),
                    args,
                    &SchemaBudget::default(),
                )
                .unwrap()],
            });
        self.script.lock().unwrap().push_back(ProviderTurn::Final {
            message: NormalizedMessage {
                role: "assistant".into(),
                content: vec![ContentBlock::Text {
                    text: "完成".into(),
                }],
                model_call_id: None,
            },
        });
    }
}
impl TurnProviderPort for Supplier {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "synthetic-external".into(),
            model: "synthetic".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a lingxi_protocol::ModelCallId,
        input: &'a ModelTurnInput,
        _delta: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.priors.lock().unwrap().push(input.prior.clone());
        if let Some(ExchangeItem::ToolResult { outcome, .. }) = input.prior.last() {
            self.outcomes.lock().unwrap().push(outcome.clone());
        }
        let next = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .expect("合成供应商脚本齐全");
        let context = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&context, next) })
    }
}
fn config(home: &Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        data_home: home.into(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}
async fn boot_review(home: &Path, tools: &Tools, provider: Arc<Supplier>) -> ServiceState {
    let layout = prepare_layout(home).unwrap();
    let state = ServiceState::bootstrap_with_deps(
        config(home),
        &layout,
        ServiceDeps {
            turn_provider: Some(provider),
            tool_gateway: Some(tools.gateway.clone()),
            approval_gate: Some(tools.approvals.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    state
        .sessions()
        .set_permission_mode_for(&owner(), "sess_local_alpha", SessionPermissionMode::Operate)
        .await
        .unwrap();
    state
}
fn run_review(state: &ServiceState) -> tokio::task::JoinHandle<String> {
    let state = state.clone();
    tokio::spawn(async move {
        state
            .sessions()
            .execute_for(
                state.storage().as_ref(),
                state.events(),
                state.runs(),
                &owner(),
                "sess_local_alpha",
                "执行具体删除",
                1_790_409_600_000,
            )
            .await
            .unwrap()
            .run_id
    })
}
async fn pending(tools: &Tools) -> lingxi_service::approval_service::PendingView {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let pending = tools
                .approvals
                .pending_of(&ctx("sess_local_alpha"), "sess_local_alpha");
            if let Some(view) = pending.first() {
                return view.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("真实删除必须等待确认")
}
async fn finish_review(
    tools: &Tools,
    view: &lingxi_service::approval_service::PendingView,
    task: tokio::task::JoinHandle<String>,
    answer: Answer,
) -> String {
    assert!(matches!(
        tools.approvals.answer(
            &ctx("sess_local_alpha"),
            "sess_local_alpha",
            &view.approval_id,
            answer
        ),
        AnswerOutcome::Settled(_)
    ));
    tokio::time::timeout(Duration::from_secs(8), task)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn unpin_review_actual_scope_structured_counts_history_and_agent_isolation() {
    let home = dir("unpin");
    agents(&home);
    let tools = bind_tools(&home, "lingxi", None);
    let store = tools.memory.tenets_store("lingxi").unwrap();
    let other = tools.memory.tenets_store("other").unwrap();
    for (id, body) in [
        ("exact", "精确条目"),
        ("multi-one", "keyword 第一条"),
        ("multi-two", "KEYWORD 第二条"),
        ("keep", "保留原则"),
    ] {
        store.add_direct(body, None, id.into()).unwrap();
    }
    store
        .add_proposal("keyword 待审", None, None, "pending".into())
        .unwrap();
    let (rejected, _, _) = store
        .add_proposal("keyword 拒绝", None, None, "rejected".into())
        .unwrap();
    store.decide(&rejected.id, false).unwrap();
    other
        .add_direct("keyword 其他助手秘密", None, "other-secret".into())
        .unwrap();
    let other_before = std::fs::read(other.file_path()).unwrap();
    let listing = tools
        .registry
        .describe(&lingxi_kernel::toolcatalog::ToolTargetId::parse(
            "tool:first-party:unpin_memory",
        ))
        .unwrap();
    assert_eq!(listing.permission.kind, PermissionKind::Review);
    // 当前运行的助手不匹配时，连确认范围也不能读取另一助手的数据。
    let request = ToolRequest::from_effective_arguments(
        "tool:first-party:unpin_memory",
        json!({"keyword":"keyword"}),
        &SchemaBudget::default(),
    )
    .unwrap();
    let refusal = tools
        .gateway
        .prepare_from_request(
            &ctx("sess_local_alpha"),
            CallerSurface::UserRun,
            "other",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &ToolCallId::new("wrong-agent-review"),
            &request,
        )
        .unwrap_err();
    assert_eq!(refusal.code(), "gateway_resource_scope_denied");
    assert!(!refusal.to_string().contains("第一条"));
    assert!(!refusal.to_string().contains("其他助手秘密"));
    let provider = Arc::new(Supplier::default());
    let state = boot_review(&home, &tools, provider.clone()).await;
    // 即使有同一关键词参数的预授权，本次真实删除范围仍须逐次确认。
    let request = ToolRequest::from_effective_arguments(
        "tool:first-party:unpin_memory",
        json!({"keyword":"keyword"}),
        &SchemaBudget::default(),
    )
    .unwrap();
    tools
        .approvals
        .grant_preauthorization(
            &ctx("sess_local_alpha"),
            "sess_local_alpha",
            lingxi_service::approval_service::InvocationGrantKey {
                target_id: "tool:first-party:unpin_memory".into(),
                capability_base: "unpin_memory.unpin".into(),
                args_digest_hex: request.args_digest.hex,
            },
            1,
            60_000,
        )
        .unwrap();
    // operate 模式仍然进入生产审批；拒绝后真实运行不得删除。
    provider.queue("unpin_memory", json!({"keyword":"keyword"}));
    let before = std::fs::read(store.file_path()).unwrap();
    let task = run_review(&state);
    let view = pending(&tools).await;
    let summary = view.args_summary.as_deref().unwrap();
    assert!(summary.contains("删除 2 条"));
    assert!(summary.contains("multi-one"));
    assert!(summary.contains("multi-two"));
    assert!(!summary.contains("other-secret"));
    assert!(!summary.contains("pending"));
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    finish_review(
        &tools,
        &view,
        task,
        Answer::Reject {
            reason: "不批准这些删除".into(),
        },
    )
    .await;
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    assert!(matches!(
        provider.outcomes.lock().unwrap().last(),
        Some(ToolOutcome::Failed { .. })
    ));
    // 精确 ID 只删除一条，原 removedCount 字段和删除条目均由真实存储结果生成。
    provider.queue("unpin_memory", json!({"id":"exact"}));
    let task = run_review(&state);
    let view = pending(&tools).await;
    assert!(view.args_summary.as_deref().unwrap().contains("精确条目"));
    finish_review(&tools, &view, task, Answer::Approve).await;
    let one = success(provider.outcomes.lock().unwrap().last().unwrap().clone());
    assert_eq!(
        details(&one),
        json!({"removedCount":1,"removedItems":[{"id":"exact","content":"精确条目"}]})
    );
    assert!(!store.active().iter().any(|t| t.id == "exact"));
    provider.queue("unpin_memory", json!({"keyword":"KeYwOrD"}));
    let task = run_review(&state);
    let view = pending(&tools).await;
    finish_review(&tools, &view, task, Answer::Approve).await;
    let two = success(provider.outcomes.lock().unwrap().last().unwrap().clone());
    let fields = details(&two);
    assert_eq!(fields["removedCount"], 2);
    assert_eq!(fields["removedItems"].as_array().unwrap().len(), 2);
    let mut removed: Vec<_> = fields["removedItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    removed.sort();
    assert_eq!(removed, ["multi-one", "multi-two"]);
    assert_eq!(store.active().len(), 1);
    assert_eq!(store.active()[0].id, "keep");
    assert_eq!(store.pending()[0].id, "pending");
    assert!(store.list().tenets.iter().any(|t| t.id == "rejected"));
    assert_eq!(std::fs::read(other.file_path()).unwrap(), other_before);
    // 真 wire 序列化往返及真正已持久化的模型历史都保留业务字段。
    let wire = ToolResultWire {
        details: two.details.clone(),
        status: ToolResultStatus::Success,
        content: two.content.clone(),
        resource_refs: two.resource_refs.clone(),
        truncated: two.truncated,
        error: None,
    };
    let encoded = serde_json::to_value(&wire).unwrap();
    assert!(encoded["details"]["removedCount"].is_number());
    assert_eq!(
        serde_json::from_value::<ToolResultWire>(encoded).unwrap(),
        wire
    );
    let legacy: ToolResultWire =
        serde_json::from_value(json!({"status":"success","content":[],"truncated":false})).unwrap();
    assert!(legacy.details.is_none());
    let history = lingxi_service::model_history::ModelHistory::new(state.storage().clone())
        .load(
            &lingxi_kernel::Principal::LocalUser,
            "lingxi",
            "sess_local_alpha",
            "later-run",
        )
        .await
        .unwrap();
    assert!(history.iter().any(|item|matches!(item,ExchangeItem::ToolResult{outcome:ToolOutcome::Success{result},..} if result.details.as_ref().and_then(|d|d.get("removedCount"))==Some(&json!(2)))));
    // 无详情的旧式成功档案仍保持原字段形状，并由真实历史读取端恢复。
    provider.queue("context_notes", json!({"action":"read"}));
    run_review(&state).await.unwrap();
    let legacy_result = success(provider.outcomes.lock().unwrap().last().unwrap().clone());
    assert!(legacy_result.details.is_none());
    let rows = lingxi_adapters::storage::session_tree::list_branch_messages(
        state.storage(),
        "sess_local_alpha",
    )
    .await
    .unwrap();
    assert!(rows
        .iter()
        .filter(|row| row.entry_type == lingxi_service::model_history::ENTRY_TYPE)
        .any(|row| {
            let value: Value = serde_json::from_str(&row.content_json).unwrap();
            value["payload"]["items"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["kind"] == "tool"
                        && item["outcome"]["status"] == "success"
                        && item["outcome"].get("details").is_none()
                })
            })
        }));
    let legacy_history = lingxi_service::model_history::ModelHistory::new(state.storage().clone())
        .load(
            &lingxi_kernel::Principal::LocalUser,
            "lingxi",
            "sess_local_alpha",
            "later-run",
        )
        .await
        .unwrap();
    assert!(legacy_history.iter().any(|item| matches!(item, ExchangeItem::ToolResult { outcome: ToolOutcome::Success { result }, .. } if result.details.is_none() && result.content == legacy_result.content)));
    for (index, args) in [
        (100, json!({"keyword":"no-match"})),
        (101, json!({"keyword":"  "})),
    ] {
        let before = std::fs::read(store.file_path()).unwrap();
        let result = success(
            tools
                .call("unpin_memory", args, index, "sess_local_alpha")
                .await
                .unwrap(),
        );
        assert_eq!(
            details(&result),
            json!({"removedCount":0,"removedItems":[]})
        );
        assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    }
    store.remove("keep").unwrap();
    let before = std::fs::read(store.file_path()).unwrap();
    let result = success(
        tools
            .call(
                "unpin_memory",
                json!({"keyword":"anything"}),
                102,
                "sess_local_alpha",
            )
            .await
            .unwrap(),
    );
    assert_eq!(details(&result)["removedCount"], 0);
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    assert!(tools
        .call(
            "unpin_memory",
            json!({"agentId":"other","keyword":"keyword"}),
            103,
            "sess_local_alpha"
        )
        .await
        .unwrap_err()
        .contains("Arguments"));
    assert_eq!(std::fs::read(other.file_path()).unwrap(), other_before);
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn unpin_changed_matches_during_confirmation_refuse_all_deletions() {
    let home = dir("review-change");
    agents(&home);
    let tools = bind_tools(&home, "lingxi", None);
    let store = tools.memory.tenets_store("lingxi").unwrap();
    store
        .add_direct("keyword 第一条", None, "old".into())
        .unwrap();
    let provider = Arc::new(Supplier::default());
    let state = boot_review(&home, &tools, provider.clone()).await;
    provider.queue("unpin_memory", json!({"keyword":"keyword"}));
    let task = run_review(&state);
    let view = pending(&tools).await;
    assert!(view.args_summary.as_deref().unwrap().contains("删除 1 条"));
    store
        .add_direct("keyword 确认后新增", None, "new".into())
        .unwrap();
    let before = std::fs::read(store.file_path()).unwrap();
    finish_review(&tools, &view, task, Answer::Approve).await;
    assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
    assert_eq!(store.active().len(), 2);
    match provider.outcomes.lock().unwrap().last().unwrap() {
        ToolOutcome::Failed { error } => assert!(error.message.contains("scope changed")),
        other => panic!("范围漂移不得成功：{other:?}"),
    };
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn record_and_recall_registered_tools_preserve_failures_and_agent_boundaries() {
    let home = dir("experience");
    agents(&home);
    let a = bind_tools(&home, "lingxi", None);
    let b = bind_tools(&home, "other", None);
    let before_b = snapshot(&home.join("agents/other"));
    assert_eq!(
        a.registry
            .describe(&lingxi_kernel::toolcatalog::ToolTargetId::parse(
                "tool:first-party:record_experience"
            ))
            .unwrap()
            .permission
            .kind,
        PermissionKind::Execute
    );
    assert_eq!(
        a.registry
            .describe(&lingxi_kernel::toolcatalog::ToolTargetId::parse(
                "tool:first-party:recall_experience"
            ))
            .unwrap()
            .permission
            .kind,
        PermissionKind::Read
    );
    let recorded = success(
        a.call(
            "record_experience",
            json!({"category":"# 有效方法","content":"先阅读说明再执行"}),
            0,
            "sess_local_alpha",
        )
        .await
        .unwrap(),
    );
    assert_eq!(details(&recorded)["added"], true);
    let index = success(
        a.call("recall_experience", json!({}), 1, "sess_local_alpha")
            .await
            .unwrap(),
    );
    assert!(text(&index).contains("有效方法"));
    assert!(text(&index).contains("experience/"));
    let read = success(
        a.call(
            "recall_experience",
            json!({"category":"有效方法"}),
            2,
            "sess_local_alpha",
        )
        .await
        .unwrap(),
    );
    assert!(text(&read).contains("1. 先阅读说明再执行"));
    assert_eq!(details(&read)["category"], "有效方法");
    let before_a = snapshot(&home.join("agents/lingxi"));
    let duplicate = success(
        a.call(
            "record_experience",
            json!({"category":"有效方法","content":"先阅读说明再执行"}),
            3,
            "sess_local_alpha",
        )
        .await
        .unwrap(),
    );
    assert_eq!(details(&duplicate)["added"], false);
    assert_eq!(snapshot(&home.join("agents/lingxi")), before_a);
    assert!(matches!(
        a.call(
            "record_experience",
            json!({"category":"../escape","content":"禁止写"}),
            4,
            "sess_local_alpha"
        )
        .await
        .unwrap(),
        ToolOutcome::Failed { .. }
    ));
    assert_eq!(snapshot(&home.join("agents/lingxi")), before_a);
    for tool in ["record_experience", "recall_experience"] {
        let args = if tool == "record_experience" {
            json!({"agentId":"other","category":"有效方法","content":"跨助手写入"})
        } else {
            json!({"agentId":"other","category":"有效方法"})
        };
        assert!(a
            .call(tool, args, 5, "sess_local_alpha")
            .await
            .unwrap_err()
            .contains("Arguments"));
    }
    for (sequence, tool, args) in [
        (
            20,
            "record_experience",
            json!({"category":"有效方法", "content":"可信运行归属错用不得写入"}),
        ),
        (21, "recall_experience", json!({"category":"有效方法"})),
    ] {
        match mismatched_agent_call(&a, tool, args, sequence).await {
            ToolOutcome::Failed { error } => {
                assert_eq!(error.code, lingxi_protocol::ErrorCode::Unauthorized);
                assert!(!error.message.contains("先阅读说明再执行"));
            }
            other => panic!("不同运行助手不得读写：{other:?}"),
        }
    }
    assert_eq!(snapshot(&home.join("agents/lingxi")), before_a);
    let other_read = success(
        b.call(
            "recall_experience",
            json!({"category":"有效方法"}),
            6,
            "sess_local_alpha",
        )
        .await
        .unwrap(),
    );
    assert!(!text(&other_read).contains("先阅读说明再执行"));
    assert_eq!(details(&other_read)["found"], false);
    assert_eq!(snapshot(&home.join("agents/other")), before_b);
    // 真正的写入系统调用失败：目录不允许创建临时文件，旧正文和索引均保持。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let directory = home.join("agents/lingxi/experience");
        let permissions = std::fs::metadata(&directory).unwrap().permissions();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
        let failed = a
            .call(
                "record_experience",
                json!({"category":"有效方法","content":"权限失败时不能覆盖旧经验"}),
                9,
                "sess_local_alpha",
            )
            .await
            .unwrap();
        std::fs::set_permissions(&directory, permissions).unwrap();
        assert!(matches!(failed, ToolOutcome::Failed { .. }));
        assert_eq!(snapshot(&home.join("agents/lingxi")), before_a);
    }
    // 真正的存储写入故障：索引位置被目录占用，旧分类正文必须字节不变。
    let index_path = home.join("agents/lingxi/experience.md");
    let old_index = std::fs::read(&index_path).unwrap();
    std::fs::remove_file(&index_path).unwrap();
    std::fs::create_dir(&index_path).unwrap();
    let documents = snapshot(&home.join("agents/lingxi/experience"));
    assert!(matches!(
        a.call(
            "record_experience",
            json!({"category":"有效方法","content":"失败时不能覆盖旧经验"}),
            7,
            "sess_local_alpha"
        )
        .await
        .unwrap(),
        ToolOutcome::Failed { .. }
    ));
    assert_eq!(snapshot(&home.join("agents/lingxi/experience")), documents);
    std::fs::remove_dir(&index_path).unwrap();
    std::fs::write(&index_path, &old_index).unwrap();
    assert_eq!(snapshot(&home.join("agents/lingxi")), before_a);
    // 真实运行后的下一模型输入和持久化历史保留记录/召回的结构化业务字段。
    let provider = Arc::new(Supplier::default());
    let state = boot_review(&home, &a, provider.clone()).await;
    provider.queue(
        "record_experience",
        json!({"category":"运行记录", "content":"真正运行记录并可召回"}),
    );
    run_review(&state).await.unwrap();
    let recorded = success(provider.outcomes.lock().unwrap().last().unwrap().clone());
    assert_eq!(details(&recorded)["added"], true);
    assert_eq!(details(&recorded)["content"], "真正运行记录并可召回");
    verify_wire_details(&recorded);
    verify_archived_details(&state, &details(&recorded)).await;
    provider.queue("recall_experience", json!({"category":"运行记录"}));
    run_review(&state).await.unwrap();
    let recalled = success(provider.outcomes.lock().unwrap().last().unwrap().clone());
    assert!(text(&recalled).contains("真正运行记录并可召回"));
    assert_eq!(details(&recalled)["category"], "运行记录");
    verify_wire_details(&recalled);
    verify_archived_details(&state, &details(&recalled)).await;
    state.storage().close().await.unwrap();
    a.memory
        .patch_flags(
            "lingxi",
            lingxi_adapters::storage::agent_memory::AgentConfigFlags {
                experience_enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
    let before = snapshot(&home.join("agents/lingxi"));
    let paused = success(
        a.call(
            "record_experience",
            json!({"category":"有效方法","content":"暂停不得写入"}),
            8,
            "sess_local_alpha",
        )
        .await
        .unwrap(),
    );
    assert_eq!(details(&paused)["paused"], true);
    assert_eq!(snapshot(&home.join("agents/lingxi")), before);
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn memory_search_status_no_hits_not_searched_and_agent_channel_isolation() {
    let home = dir("search-status");
    agents(&home);
    let a = bind_tools(&home, "lingxi", None);
    let b = bind_tools(&home, "other", None);
    let store = a.memory.fact_store("lingxi").unwrap();
    for (body, tags, session) in [
        ("Vim 偏好", vec!["偏好".into()], None),
        ("channel secret", vec!["频道".into()], Some("channel-other")),
    ] {
        store
            .add(&lingxi_adapters::storage::agent_memory::FactInsert {
                fact: body,
                tags: &tags,
                time: Some("2026-10-14"),
                session_id: session,
            })
            .unwrap();
    }
    let hit = success(
        a.call(
            "search_memory",
            json!({"query":"Vim"}),
            0,
            "sess_local_alpha",
        )
        .await
        .unwrap(),
    );
    let fields = details(&hit);
    assert_eq!(fields["retrieval"], "matched");
    assert_eq!(fields["semantic"], "not_configured");
    assert_eq!(fields["facts"][0]["time"], "2026-10-14");
    assert_eq!(fields["facts"][0]["tags"], json!(["偏好"]));
    assert!(text(&hit).contains("not_configured"));
    let no = success(
        a.call(
            "search_memory",
            json!({"query":"absent"}),
            1,
            "sess_local_alpha",
        )
        .await
        .unwrap(),
    );
    assert_eq!(details(&no)["retrieval"], "no_matches");
    assert_eq!(details(&no)["semantic"], "not_configured");
    let not = success(
        a.call("search_memory", json!({}), 2, "sess_local_alpha")
            .await
            .unwrap(),
    );
    assert_eq!(details(&not)["retrieval"], "not_searched");
    assert_eq!(details(&not)["semantic"], "not_searched");
    assert!(text(&not).contains("not searched"));
    let other = success(
        b.call(
            "search_memory",
            json!({"query":"Vim"}),
            3,
            "sess_local_alpha",
        )
        .await
        .unwrap(),
    );
    assert_eq!(details(&other)["retrieval"], "empty_store");
    assert_eq!(details(&other)["semantic"], "not_searched");
    assert!(!text(&other).contains("Vim"));
    assert!(a
        .call(
            "search_memory",
            json!({"query":"Vim","agentId":"other"}),
            4,
            "sess_local_alpha"
        )
        .await
        .unwrap_err()
        .contains("Arguments"));
    match mismatched_agent_call(&a, "search_memory", json!({"query":"Vim"}), 22).await {
        ToolOutcome::Failed { error } => {
            assert_eq!(error.code, lingxi_protocol::ErrorCode::Unauthorized);
            assert!(!error.message.contains("Vim"));
        }
        other => panic!("不同运行助手不得检索：{other:?}"),
    }
    let scoped = success(
        a.call(
            "search_memory",
            json!({"query":"secret"}),
            5,
            "channel-current",
        )
        .await
        .unwrap(),
    );
    assert_eq!(details(&scoped)["retrieval"], "no_matches");
    assert!(!text(&scoped).contains("channel secret"));
    let explicit = success(
        a.call(
            "search_memory",
            json!({"query":"secret","cross_channel":true}),
            6,
            "channel-current",
        )
        .await
        .unwrap(),
    );
    assert_eq!(details(&explicit)["retrieval"], "matched");
    assert!(text(&explicit).contains("channel secret"));
    let provider = Arc::new(Supplier::default());
    let state = boot_review(&home, &a, provider.clone()).await;
    provider.queue("search_memory", json!({"query":"Vim"}));
    run_review(&state).await.unwrap();
    let consumed = success(provider.outcomes.lock().unwrap().last().unwrap().clone());
    assert_eq!(details(&consumed), fields);
    verify_wire_details(&consumed);
    verify_archived_details(&state, &details(&consumed)).await;
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

struct EmbedStub {
    requests: Arc<Mutex<Vec<Value>>>,
    addr: std::net::SocketAddr,
    mode: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    handle: tokio::task::JoinHandle<()>,
}
async fn embedding_stub() -> EmbedStub {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mode = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let stub_requests = requests.clone();
    let stub_mode = mode.clone();
    let stub_calls = calls.clone();
    let route = axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
        let mode = stub_mode.clone();
        let calls = stub_calls.clone();
        let requests = stub_requests.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            requests.lock().unwrap().push(body.clone());
            if mode.load(Ordering::SeqCst) == 1 {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            let vector = if mode.load(Ordering::SeqCst) == 2 {
                json!([])
            } else {
                json!([1.0, 0.0])
            };
            axum::Json(
                json!({"model":body["model"],"data":[{"index":0,"embedding":vector}],"usage":{"prompt_tokens":1,"total_tokens":1}}),
            )
        }
    });
    let router = axum::Router::new().route("/v1/embeddings", route);
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    EmbedStub {
        requests,
        addr,
        mode,
        calls,
        handle,
    }
}
async fn boot_embeddings(home: &Path, stub: &EmbedStub, model: bool) -> ServiceState {
    let layout = prepare_layout(home).unwrap();
    let file = home.join("provider-fixture.json");
    std::fs::write(&file,serde_json::to_vec(&json!({"home":home,"providers":{"embed":{"protocol":"openai-embeddings","endpoint":format!("http://{}/v1",stub.addr),"auth":{"kind":"apiKey","apiKey":"SYNTHETIC_MEMORY_STUB"}}},"models":if model{json!({"embedding":{"provider":"embed","model":"memory-embedding"}})}else{json!({})}})).unwrap()).unwrap();
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&file), &layout.runtime_dir)
            .unwrap()
            .unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    ServiceState::bootstrap_with_deps(
        config(home),
        &layout,
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            model_plane_source: Some(source),
            credential_service: Some(credentials),
            workspace_root: Some(home.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn memory_semantic_real_provider_no_model_no_coverage_timeout_and_actual_vector_hits() {
    let stub = embedding_stub().await;
    let home = dir("semantic");
    agents(&home);
    agent_embedding_ref(
        &home,
        "lingxi",
        json!({"provider":"embed","id":"memory-embedding"}),
    );
    let state = boot_embeddings(&home, &stub, true).await;
    let tools = bind_tools(&home, "lingxi", Some(state.memory().clone()));
    let store = tools.memory.fact_store("lingxi").unwrap();
    let fact = store
        .add(&lingxi_adapters::storage::agent_memory::FactInsert {
            fact: "语义独有内容",
            tags: &["合成向量".into()],
            time: Some("2026-10-14"),
            session_id: None,
        })
        .unwrap();
    let no_coverage = success(
        tools
            .call(
                "search_memory",
                json!({"query":"unrelated"}),
                0,
                "sess_local_alpha",
            )
            .await
            .unwrap(),
    );
    assert_eq!(details(&no_coverage)["semantic"], "skipped_no_coverage");
    assert_eq!(details(&no_coverage)["coverage"], 0);
    assert!(text(&no_coverage).contains("skipped_no_coverage"));
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    // 向量来自同一真实供应商链的实测身份；避免伪造 modelKey 声称覆盖。
    let lingxi_service::memory::SearchEmbeddingResolution::Ready(embedder) =
        state.memory().search_embedder_for_agent("lingxi").unwrap()
    else {
        panic!("已配置助手模型必须可解析")
    };
    let (identity, vectors) = embedder
        .embed(
            vec!["语义独有内容".into()],
            lingxi_service::knowledge::EmbeddingInputType::Document,
        )
        .await
        .unwrap();
    store
        .upsert_embedding(fact, &identity.key, &vectors[0])
        .unwrap();
    let hit = success(
        tools
            .call(
                "search_memory",
                json!({"query":"unrelated"}),
                1,
                "sess_local_alpha",
            )
            .await
            .unwrap(),
    );
    assert_eq!(details(&hit)["semantic"], "used");
    assert_eq!(details(&hit)["facts"][0]["fact"], "语义独有内容");
    assert_eq!(details(&hit)["facts"][0]["source"], "semantic");
    assert_eq!(details(&hit)["coverage"], 1);
    stub.mode.store(1, Ordering::SeqCst);
    let timeout = success(
        tools
            .call(
                "search_memory",
                json!({"query":"unrelated"}),
                2,
                "sess_local_alpha",
            )
            .await
            .unwrap(),
    );
    assert_eq!(details(&timeout)["semantic"], "skipped_timeout");
    assert_eq!(details(&timeout)["retrieval"], "no_matches");
    assert!(text(&timeout).contains("skipped_timeout"));
    stub.mode.store(2, Ordering::SeqCst);
    let invalid = success(
        tools
            .call(
                "search_memory",
                json!({"query":"unrelated"}),
                3,
                "sess_local_alpha",
            )
            .await
            .unwrap(),
    );
    assert_eq!(details(&invalid)["semantic"], "unavailable_unresolvable");
    assert!(text(&invalid).contains("unavailable_unresolvable"));
    let other = bind_tools(&home, "other", Some(state.memory().clone()));
    let result = success(
        other
            .call(
                "search_memory",
                json!({"query":"unrelated"}),
                4,
                "sess_local_alpha",
            )
            .await
            .unwrap(),
    );
    assert_eq!(details(&result)["retrieval"], "empty_store");
    assert!(!text(&result).contains("语义独有内容"));
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
    let home = dir("no-model");
    agents(&home);
    let state = boot_embeddings(&home, &stub, false).await;
    let tools = bind_tools(&home, "lingxi", Some(state.memory().clone()));
    tools
        .memory
        .fact_store("lingxi")
        .unwrap()
        .add(&lingxi_adapters::storage::agent_memory::FactInsert {
            fact: "existing fact",
            tags: &[],
            time: None,
            session_id: None,
        })
        .unwrap();
    let count = stub.calls.load(Ordering::SeqCst);
    let unavailable = success(
        tools
            .call(
                "search_memory",
                json!({"query":"absent"}),
                5,
                "sess_local_alpha",
            )
            .await
            .unwrap(),
    );
    assert_eq!(details(&unavailable)["semantic"], "unavailable_no_model");
    assert!(text(&unavailable).contains("unavailable_no_model"));
    assert_eq!(stub.calls.load(Ordering::SeqCst), count);
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
    stub.handle.abort();
}

#[tokio::test]
async fn large_native_details_share_projection_budget_keep_raw_and_restore_same_projection() {
    use lingxi_kernel::ports::EventStorePort as _;
    use lingxi_protocol::{EventPayload, KnownEventPayload};
    let home = dir("large-details");
    agents(&home);
    let tools = bind_tools(&home, "lingxi", None);
    let provider = Arc::new(Supplier::default());
    let state = boot_review(&home, &tools, provider.clone()).await;
    let content = format!("HEAD-经验{}TAIL-经验", "甲乙\u{0001}".repeat(16_000));
    assert!(content.len() > 100 * 1024);
    provider.queue(
        "record_experience",
        json!({"category":"真实大详情", "content":content}),
    );
    run_review(&state).await.unwrap();
    let projected = success(provider.outcomes.lock().unwrap().last().unwrap().clone());
    assert!(projected.truncated);
    let fields = details(&projected);
    assert_eq!(fields["added"], true);
    assert_eq!(fields["category"], "真实大详情");
    assert_eq!(fields["_modelProjection"]["detailsTruncated"], true);
    assert!(
        fields["_modelProjection"]["originalDetailsBytes"]
            .as_u64()
            .unwrap()
            > 100 * 1024
    );
    assert!(fields["content"].is_string());
    assert!(fields["content"].as_str().unwrap().contains("HEAD-经验"));
    assert!(fields["content"].as_str().unwrap().contains("TAIL-经验"));
    let cost = lingxi_service::hooks::tool_outcome_model_bytes(&ToolOutcome::Success {
        result: projected.clone(),
    });
    assert!(
        cost <= lingxi_service::hooks::MAX_TOOL_SUCCESS_PROJECTION_BYTES,
        "正文和native详情合计必须在旧F08预算内：{cost}"
    );
    let events = state
        .storage()
        .stream_events_after("sess_local_alpha", lingxi_protocol::Seq::new(0), 10_000)
        .await
        .unwrap();
    let raw = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::Known(KnownEventPayload::ToolCallCompleted(payload)) => {
                Some(&payload.result)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(raw.status, ToolResultStatus::Success);
    assert!(!raw.truncated);
    assert_eq!(raw.details.as_ref().unwrap()["content"], content);
    assert!(!raw
        .details
        .as_ref()
        .unwrap()
        .contains_key("_modelProjection"));
    let stored = tools.memory.experience_store("lingxi").unwrap();
    assert!(stored.read_merged().0.contains(&content));
    let projection = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::Unknown(payload)
                if payload.event_type == "tool_result_model_projection" =>
            {
                Some(&payload.raw)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(projection["result"]["details"], fields);
    assert_eq!(projection["result"]["truncated"], true);
    assert_eq!(
        projection["modelProjectionBytes"].as_u64(),
        Some(cost as u64)
    );
    verify_archived_details(&state, &fields).await;
    // 旧renderedBytes仍相同也不能把大native详情塞回投影；缺新统计字段同样拒绝。
    let mut oversized = Value::Object(projection.clone());
    oversized["result"]["details"]["content"] = json!(content);
    assert!(lingxi_service::model_history::projected_tool_item(&oversized).is_err());
    oversized
        .as_object_mut()
        .unwrap()
        .remove("modelProjectionBytes");
    assert!(lingxi_service::model_history::projected_tool_item(&oversized).is_err());
    let mut hidden = Value::Object(projection.clone());
    hidden["result"]["truncated"] = json!(false);
    assert!(lingxi_service::model_history::projected_tool_item(&hidden).is_err());
    // 下一实际运行只能恢复同一受限投影，不能偷偷以raw完整大详情重新塞入模型输入。
    provider.queue("context_notes", json!({"action":"read"}));
    run_review(&state).await.unwrap();
    {
        let requests = provider.priors.lock().unwrap();
        let next_request = &requests[2];
        assert!(next_request.iter().any(|item| matches!(item, ExchangeItem::ToolResult { outcome: ToolOutcome::Success { result }, .. } if result == &projected)));
        assert!(!next_request.iter().any(|item| matches!(item, ExchangeItem::ToolResult { outcome: ToolOutcome::Success { result }, .. } if result.details.as_ref().and_then(|fields|fields.get("content")) == Some(&json!(content)))));
    }
    let history = lingxi_service::model_history::ModelHistory::new(state.storage().clone())
        .load(
            &lingxi_kernel::Principal::LocalUser,
            "lingxi",
            "sess_local_alpha",
            "later-run",
        )
        .await
        .unwrap();
    let restored = history
        .iter()
        .find_map(|item| match item {
            ExchangeItem::ToolResult {
                outcome: ToolOutcome::Success { result },
                ..
            } if result
                .details
                .as_ref()
                .is_some_and(|fields| fields.contains_key("_modelProjection")) =>
            {
                Some(result)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(restored, &projected);
    assert!(
        lingxi_service::hooks::tool_outcome_model_bytes(&ToolOutcome::Success {
            result: restored.clone()
        }) <= 32768
    );
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn independent_concurrent_record_successes_are_all_recallable() {
    let home = dir("review-concurrent-record");
    agents(&home);
    let tools = bind_tools(&home, "lingxi", None);
    let store = tools.memory.experience_store("lingxi").unwrap();
    store.record_entry("共同类别", "已存在经验正控").unwrap();
    for n in 0..4 {
        store
            .record_entry("共同类别", &format!("顺序正控唯一项{n}结束"))
            .unwrap();
    }
    assert_eq!(
        store.list_documents_checked().unwrap()[0]
            .body
            .lines()
            .count(),
        5
    );
    let barrier = Arc::new(std::sync::Barrier::new(16));
    let jobs:Vec<_>=(0..16).map(|n|{
        let barrier=barrier.clone();let home=home.clone();
        std::thread::spawn(move||{
            let tools=bind_tools(&home,"lingxi",None);
            let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            barrier.wait();
            let outcome=runtime.block_on(tools.call("record_experience",json!({"category":"共同类别","content":format!("并发原生注册工具唯一项{n}结束")}),n,"sess_local_alpha")).unwrap();
            match outcome {ToolOutcome::Success {result}=>{assert_eq!(details(&result)["added"],true); Some(n)},other=>{eprintln!("REFUSED_{n}={other:?}");None}}
        })
    }).collect();
    let accepted: Vec<_> = jobs
        .into_iter()
        .filter_map(|job| job.join().unwrap())
        .collect();
    let docs = store.list_documents_checked().unwrap();
    let body = &docs[0].body;
    eprintln!("RECALLED_AFTER_16_SUCCESSES={body}");
    let missing: Vec<_> = accepted
        .iter()
        .filter(|n| !body.contains(&format!("并发原生注册工具唯一项{n}结束")))
        .collect();
    assert!(
        missing.is_empty(),
        "接受的真正注册工具均返回added=true，但以下经验无法召回: {missing:?}"
    );
    assert!(body.contains("已存在经验正控"));
    assert!(
        !accepted.is_empty(),
        "至少一条真实并发记录必须成功，不能通过拒绝全部请求规避验证"
    );
    let index = store.index_text_checked().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let recalled = success(
        runtime
            .block_on(tools.call(
                "recall_experience",
                json!({"category":"共同类别"}),
                100,
                "sess_local_alpha",
            ))
            .unwrap(),
    );
    for n in accepted {
        let item = format!("并发原生注册工具唯一项{n}结束");
        assert!(index.contains(&item), "成功项必须进入实际索引");
        assert!(
            text(&recalled).contains(&item),
            "成功项必须经正式注册召回工具读回"
        );
    }
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn experience_record_sync_rebuild_and_rollback_share_one_agent_operation_gate() {
    use lingxi_adapters::storage::agent_memory::{memory_operation_test_hook, MemoryOperationKind};
    let home = dir("experience-operation-gate");
    agents(&home);
    let tools = bind_tools(&home, "lingxi", None);
    let store = tools.memory.experience_store("lingxi").unwrap();
    store
        .record_entry("共享串行分类", "原成功经验唯一项")
        .unwrap();
    let agent_dir = home.join("agents/lingxi");
    for operation in ["record", "sync", "rebuild"] {
        let before = snapshot(&agent_dir);
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let release_rx = Mutex::new(release_rx);
        let hook = memory_operation_test_hook(
            &agent_dir.join("memory"),
            Arc::new(move |kind| {
                assert_eq!(kind, MemoryOperationKind::Maintenance);
                entered_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            }),
        );
        let operation_dir = agent_dir.clone();
        let worker = std::thread::spawn(move || {
            let store =
                lingxi_adapters::storage::agent_memory::ExperienceStore::new(&operation_dir);
            match operation {
                "record" => store
                    .record_entry("共享串行分类", "门内新成功记录")
                    .map(|_| ()),
                "sync" => {
                    store.sync_categories(&[("共享串行分类".into(), "1. 整份同步成功记录".into())])
                }
                "rebuild" => store.rebuild_index(),
                _ => unreachable!(),
            }
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        // 三种入口均拒绝并行写入，拒绝期间文档及索引完全不变。
        assert!(matches!(
            store.record_entry("共享串行分类", "不应写入的并发记录"),
            Err(lingxi_adapters::storage::agent_memory::MemoryError::Conflict { .. })
        ));
        assert!(matches!(
            store.sync_categories(&[("共享串行分类".into(), "1. 不应同步的并发记录".into())]),
            Err(lingxi_adapters::storage::agent_memory::MemoryError::Conflict { .. })
        ));
        assert!(matches!(
            store.rebuild_index(),
            Err(lingxi_adapters::storage::agent_memory::MemoryError::Conflict { .. })
        ));
        assert_eq!(snapshot(&agent_dir), before);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let refused = runtime
            .block_on(tools.call(
                "record_experience",
                json!({"category":"共享串行分类", "content":"正式工具忙时必须明确拒绝"}),
                100,
                "sess_local_alpha",
            ))
            .unwrap();
        assert!(
            matches!(refused, ToolOutcome::Failed { error } if error.code == lingxi_protocol::ErrorCode::Conflict && error.retryable)
        );
        let other = tools.memory.experience_store("other").unwrap();
        assert_eq!(
            other.record_entry("另一助手", &format!("独立成功记录{operation}")),
            Ok((true, None))
        );
        release_tx.send(()).unwrap();
        worker.join().unwrap().unwrap();
        drop(hook);
        assert!(store
            .list_documents_checked()
            .unwrap()
            .iter()
            .all(|doc| !doc.body.contains("不应")));
        let index = store.index_text_checked().unwrap();
        let expected = if operation == "record" {
            "门内新成功记录"
        } else {
            "整份同步成功记录"
        };
        assert!(store.list_documents_checked().unwrap()[0]
            .body
            .contains(expected));
        assert!(index.contains(expected));
    }
    std::fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn experience_failed_index_publish_keeps_operation_gate_through_document_rollback() {
    use lingxi_adapters::storage::agent_memory::{memory_operation_test_hook, MemoryOperationKind};
    use std::os::unix::fs::PermissionsExt;
    let home = dir("experience-index-rollback-gate");
    agents(&home);
    let tools = bind_tools(&home, "lingxi", None);
    let store = tools.memory.experience_store("lingxi").unwrap();
    store
        .record_entry("故障分类", "之前已成功的经验唯一项")
        .unwrap();
    let agent_dir = home.join("agents/lingxi");
    let before = snapshot(&agent_dir);
    let original_permissions = std::fs::metadata(&agent_dir).unwrap().permissions();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let release_rx = Mutex::new(release_rx);
    let hook = memory_operation_test_hook(
        &agent_dir.join("memory"),
        Arc::new(move |kind| {
            assert_eq!(kind, MemoryOperationKind::Maintenance);
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }),
    );
    let operation_dir = agent_dir.clone();
    let worker = std::thread::spawn(move || {
        lingxi_adapters::storage::agent_memory::ExperienceStore::new(&operation_dir)
            .record_entry("故障分类", "索引发布失败必须恢复的记录")
    });
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    // 分类目录仍可写，助手目录不能创建索引临时文件：实际先写文档，再失败并恢复正文。
    std::fs::set_permissions(&agent_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    assert!(matches!(
        store.record_entry("故障分类", "并行成功不应被虚报"),
        Err(lingxi_adapters::storage::agent_memory::MemoryError::Conflict { .. })
    ));
    assert!(matches!(
        store.sync_categories(&[]),
        Err(lingxi_adapters::storage::agent_memory::MemoryError::Conflict { .. })
    ));
    assert!(matches!(
        store.rebuild_index(),
        Err(lingxi_adapters::storage::agent_memory::MemoryError::Conflict { .. })
    ));
    assert_eq!(
        tools
            .memory
            .experience_store("other")
            .unwrap()
            .record_entry("另一助手", "失败窗内独立成功"),
        Ok((true, None))
    );
    release_tx.send(()).unwrap();
    let failed = worker.join().unwrap();
    std::fs::set_permissions(&agent_dir, original_permissions).unwrap();
    drop(hook);
    assert!(failed.is_err(), "索引发布的真实文件系统写入必须失败");
    assert_eq!(snapshot(&agent_dir), before, "失败恢复不得损伤旧经验或索引");
    assert_eq!(
        store.record_entry("故障分类", "失败后新增成功唯一项"),
        Ok((true, None))
    );
    let docs = store.list_documents_checked().unwrap();
    assert!(docs[0].body.contains("之前已成功的经验唯一项"));
    assert!(docs[0].body.contains("失败后新增成功唯一项"));
    assert!(!docs[0].body.contains("索引发布失败必须恢复的记录"));
    assert!(store
        .index_text_checked()
        .unwrap()
        .contains("失败后新增成功唯一项"));
    std::fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn experience_operation_gate_survives_memory_directory_creation_and_agent_path_alias() {
    use lingxi_adapters::storage::agent_memory::memory_operation_test_hook;
    let home = dir("experience-gate-path-alias");
    agents(&home);
    let agent_dir = home.join("agents/lingxi");
    let memory_dir = agent_dir.join("memory");
    std::fs::remove_dir(&memory_dir).unwrap();
    let alias = home.join("agent-alias");
    std::os::unix::fs::symlink(&agent_dir, &alias).unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let release_rx = Mutex::new(release_rx);
    let hook = memory_operation_test_hook(
        &alias.join("memory"),
        Arc::new(move |_| {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }),
    );
    let writer_dir = agent_dir.clone();
    let worker = std::thread::spawn(move || {
        lingxi_adapters::storage::agent_memory::ExperienceStore::new(&writer_dir)
            .record_entry("同一助手", "唯一成功项")
    });
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    std::fs::create_dir(&memory_dir).unwrap();
    let alias_store = lingxi_adapters::storage::agent_memory::ExperienceStore::new(&alias);
    assert!(matches!(
        alias_store.record_entry("同一助手", "路径别名不应另开锁"),
        Err(lingxi_adapters::storage::agent_memory::MemoryError::Conflict { .. })
    ));
    release_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap().unwrap(), (true, None));
    drop(hook);
    assert_eq!(
        alias_store.list_documents_checked().unwrap()[0].body,
        "1. 唯一成功项"
    );
    std::fs::remove_file(alias).unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn independent_agent_without_memory_embedding_must_not_spend_global_embedding() {
    let stub = embedding_stub().await;
    let home = dir("independent-no-agent-embedding");
    agents(&home);
    let state = boot_embeddings(&home, &stub, true).await;
    let tools = bind_tools(&home, "lingxi", Some(state.memory().clone()));
    tools
        .memory
        .fact_store("lingxi")
        .unwrap()
        .add(&lingxi_adapters::storage::agent_memory::FactInsert {
            fact: "agent fact",
            tags: &[],
            time: None,
            session_id: None,
        })
        .unwrap();
    let result = success(
        tools
            .call(
                "search_memory",
                json!({"query":"different phrase"}),
                101,
                "sess_local_alpha",
            )
            .await
            .unwrap(),
    );
    let receipt = json!({"semantic":details(&result),"providerCalls":stub.calls.load(Ordering::SeqCst),"agentConfig":std::fs::read_to_string(home.join("agents/lingxi/config.yaml")).unwrap()});
    std::fs::write(
        home.join("no-agent-embedding-facts.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!("EX06_AGENT_EMBEDDING {receipt}");
    state.storage().close().await.unwrap();
    stub.handle.abort();
    assert_eq!(
        stub.calls.load(Ordering::SeqCst),
        0,
        "当前助手没有memory.embedding_model，不能花费全局知识嵌入模型"
    );
    assert_eq!(details(&result)["semantic"], "unavailable_no_model");
}

fn agent_embedding_ref(home: &Path, agent: &str, model: Value) {
    let path = home.join("agents").join(agent).join("config.yaml");
    let mut doc = lingxi_adapters::storage::agent_memory::config::read_agent_config(&path).unwrap();
    doc.deep_merge(&json!({"memory":{"embedding_model":model}}))
        .unwrap();
    std::fs::write(path, doc.render().unwrap()).unwrap();
}
struct MemoryChatStub {
    addr: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<Value>>>,
    handle: tokio::task::JoinHandle<()>,
}
impl Drop for MemoryChatStub {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
async fn memory_chat_stub() -> MemoryChatStub {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let capture = requests.clone();
    let router=axum::Router::new().route("/v1/chat/completions",axum::routing::post(move |axum::Json(body):axum::Json<Value>| {
        let capture=capture.clone();async move {
            capture.lock().unwrap().push(body.clone());
            let tool_count=body["messages"].as_array().unwrap().iter().filter(|m|m["role"]=="tool").count();
            let frames=if tool_count>=2 {
                vec![json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"已检索当前助手记忆"},"finish_reason":null}]}),json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})]
            } else {
                let (name,args)=if tool_count==0 { ("mcp_describe_tool",json!({"name":"search_memory"})) } else { ("mcp_call",json!({"tool":"search_memory","arguments":{"query":"unrelated semantic phrase"}})) };
                assert!(body["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]==name));
                vec![json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":format!("actual-ex06-call-{tool_count}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]}),json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})]
            };
            let stream=frames.into_iter().map(|f|format!("data: {f}\n\n")).collect::<String>()+"data: [DONE]\n\n";
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],stream)
        }
    }));
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    MemoryChatStub {
        addr,
        requests,
        handle,
    }
}
async fn boot_agent_embedding_plane(
    home: &Path,
    a: &EmbedStub,
    b: &EmbedStub,
    global: &EmbedStub,
    global_enabled: bool,
) -> (ServiceState, MemoryChatStub) {
    let layout = prepare_layout(home).unwrap();
    let file = home.join("per-agent-provider-fixture.json");
    let chat = memory_chat_stub().await;
    let providers = json!({"a":{"protocol":"openai-embeddings","endpoint":format!("http://{}/v1",a.addr),"auth":{"kind":"apiKey","apiKey":"SYNTHETIC_A"}},"b":{"protocol":"openai-embeddings","endpoint":format!("http://{}/v1",b.addr),"auth":{"kind":"apiKey","apiKey":"SYNTHETIC_B"}},"global":{"protocol":"openai-embeddings","endpoint":format!("http://{}/v1",global.addr),"auth":{"kind":"apiKey","apiKey":"SYNTHETIC_GLOBAL"}},"chat":{"protocol":"openai-completions","endpoint":format!("http://{}/v1",chat.addr),"auth":{"kind":"apiKey","apiKey":"SYNTHETIC_CHAT"}}});
    let mut models = if global_enabled {
        json!({"embedding":{"provider":"global","model":"global-model"}})
    } else {
        json!({})
    };
    models["chat"] = json!({"provider":"chat","model":"chat-stub","capabilities":{"tools":true}});
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"home":home,"providers":providers,"models":models})).unwrap(),
    )
    .unwrap();
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&file), &layout.runtime_dir)
            .unwrap()
            .unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let state = ServiceState::bootstrap_with_deps(
        config(home),
        &layout,
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            model_plane_source: Some(source),
            credential_service: Some(credentials),
            workspace_root: Some(home.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    (state, chat)
}
fn embedding_usage_rows(home: &Path) -> Value {
    let c = rusqlite::Connection::open(home.join("lingxi-service/data/runs.db")).unwrap();
    let mut q=c.prepare("SELECT model_call_id,provider,model,session_id,run_id,attempt,parent_tool_call_id,transport_attempts,outcome,usage_state,input_tokens,cause_ref FROM model_call_usage WHERE purpose='embedding' ORDER BY recorded_at_unix_ms,model_call_id").unwrap();
    let rows=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"provider":r.get::<_,String>(1)?,"model":r.get::<_,String>(2)?,"session":r.get::<_,Option<String>>(3)?,"run":r.get::<_,Option<String>>(4)?,"attempt":r.get::<_,Option<String>>(5)?,"parentTool":r.get::<_,Option<String>>(6)?,"attempts":r.get::<_,Option<i64>>(7)?,"outcome":r.get::<_,String>(8)?,"usage":r.get::<_,String>(9)?,"inputTokens":r.get::<_,Option<i64>>(10)?,"cause":r.get::<_,Option<String>>(11)?}))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    json!(rows)
}
fn paid_memory_receipts(home: &Path) -> Vec<Value> {
    std::fs::read_dir(home.join("lingxi-service/media-products/embedding-calls-v1"))
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension().is_some_and(|s| s == "json"))
                .then(|| serde_json::from_slice::<Value>(&std::fs::read(p).unwrap()).unwrap())
        })
        .collect()
}
#[tokio::test]
async fn ex06_agent_selected_models_actual_wire_scope_usage_restart_and_invalid_configs() {
    let a = embedding_stub().await;
    let b = embedding_stub().await;
    let global = embedding_stub().await;
    let home = dir("ex06-agent-models");
    agents(&home);
    agent_embedding_ref(&home, "lingxi", json!({"provider":"a","id":"model-a"}));
    agent_embedding_ref(&home, "other", json!({"provider":"b","id":"model-b"}));
    // 现役目录声明两个真实可解析的嵌入模型；全局模型故意使用第三个服务商。
    std::fs::write(home.join("provider-catalog.json"),serde_json::to_vec(&json!({"catalogVersion":2,"providers":{"a":{"models":[{"id":"model-a","type":"embedding"}]},"b":{"models":[{"id":"model-b","operations":["embedding"]}]}},"meta":{}})).unwrap()).unwrap();
    let (state, provider) = boot_agent_embedding_plane(&home, &a, &b, &global, true).await;
    let ta = bind_tools(&home, "lingxi", Some(state.memory().clone()));
    let tb = bind_tools(&home, "other", Some(state.memory().clone()));
    let sa = ta.memory.fact_store("lingxi").unwrap();
    let sb = tb.memory.fact_store("other").unwrap();
    let fa = sa
        .add(&lingxi_adapters::storage::agent_memory::FactInsert {
            fact: "ALPHA_PRIVATE_BODY",
            tags: &[],
            time: None,
            session_id: None,
        })
        .unwrap();
    let fb = sb
        .add(&lingxi_adapters::storage::agent_memory::FactInsert {
            fact: "BETA_PRIVATE_BODY",
            tags: &[],
            time: None,
            session_id: None,
        })
        .unwrap();
    let query = json!({"query":"unrelated semantic phrase"});
    let none = success(
        ta.call("search_memory", query.clone(), 201, "ex06-a")
            .await
            .unwrap(),
    );
    assert_eq!(details(&none)["semantic"], "skipped_no_coverage");
    assert_eq!(a.calls.load(Ordering::SeqCst), 1);
    assert_eq!(b.calls.load(Ordering::SeqCst), 0);
    assert_eq!(global.calls.load(Ordering::SeqCst), 0);
    let lingxi_service::memory::SearchEmbeddingResolution::Ready(ea) =
        state.memory().search_embedder_for_agent("lingxi").unwrap()
    else {
        panic!("A必须可解析")
    };
    let lingxi_service::memory::SearchEmbeddingResolution::Ready(eb) =
        state.memory().search_embedder_for_agent("other").unwrap()
    else {
        panic!("B必须可解析")
    };
    let (ia, va) = ea
        .embed(
            vec!["ALPHA_PRIVATE_BODY".into()],
            lingxi_service::knowledge::EmbeddingInputType::Document,
        )
        .await
        .unwrap();
    let (ib, vb) = eb
        .embed(
            vec!["BETA_PRIVATE_BODY".into()],
            lingxi_service::knowledge::EmbeddingInputType::Document,
        )
        .await
        .unwrap();
    assert_ne!(ia.key, ib.key);
    sa.upsert_embedding(fa, &ib.key, &vb[0]).unwrap();
    let wrong_space = success(
        ta.call("search_memory", query.clone(), 202, "ex06-a")
            .await
            .unwrap(),
    );
    assert_eq!(details(&wrong_space)["semantic"], "skipped_no_coverage");
    sa.upsert_embedding(fa, &ia.key, &va[0]).unwrap();
    sb.upsert_embedding(fb, &ib.key, &vb[0]).unwrap();
    let hit_a = success(
        ta.call("search_memory", query.clone(), 203, "ex06-a")
            .await
            .unwrap(),
    );
    let hit_b = success(
        tb.call("search_memory", query.clone(), 204, "ex06-b")
            .await
            .unwrap(),
    );
    assert_eq!(details(&hit_a)["semantic"], "used");
    assert_eq!(details(&hit_b)["semantic"], "used");
    assert!(text(&hit_a).contains("ALPHA_PRIVATE_BODY"));
    assert!(!text(&hit_a).contains("BETA_PRIVATE_BODY"));
    assert!(text(&hit_b).contains("BETA_PRIVATE_BODY"));
    assert!(!text(&hit_b).contains("ALPHA_PRIVATE_BODY"));
    let before = (
        a.calls.load(Ordering::SeqCst),
        b.calls.load(Ordering::SeqCst),
    );
    assert!(matches!(
        mismatched_agent_call(&ta, "search_memory", query.clone(), 205).await,
        ToolOutcome::Failed { .. }
    ));
    assert_eq!(
        before,
        (
            a.calls.load(Ordering::SeqCst),
            b.calls.load(Ordering::SeqCst)
        )
    );
    state
        .sessions()
        .set_permission_mode_for(&owner(), "sess_local_alpha", SessionPermissionMode::Operate)
        .await
        .unwrap();
    let run = run_review(&state).await.unwrap();
    let chat_wire = provider.requests.lock().unwrap().clone();
    let actual = chat_wire.last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|m| m["role"] == "tool")
        .unwrap()["content"]
        .clone();
    assert!(actual.as_str().unwrap().contains("ALPHA_PRIVATE_BODY"));
    assert!(actual.as_str().unwrap().contains("used"));
    assert!(!actual.as_str().unwrap().contains("BETA_PRIVATE_BODY"));
    let usage = embedding_usage_rows(&home);
    let actual_rows = usage
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["run"] == run)
        .collect::<Vec<_>>();
    assert_eq!(actual_rows.len(), 1);
    let row = actual_rows[0];
    assert_eq!(row["provider"], "a");
    assert_eq!(row["model"], "model-a");
    assert_eq!(row["session"], "sess_local_alpha");
    assert!(row["parentTool"].is_string());
    assert_eq!(row["attempts"], 1);
    assert_eq!(row["outcome"], "succeeded");
    assert_eq!(row["inputTokens"], 1);
    let receipts = paid_memory_receipts(&home);
    assert!(receipts.iter().any(|r| r["ownership"]["run_id"] == run
        && r["ownership"]["agent_id"] == "lingxi"
        && r["ownerBinding"]["parentToolCallId"] == row["parentTool"]));
    let count_before_restart = (
        a.calls.load(Ordering::SeqCst),
        b.calls.load(Ordering::SeqCst),
    );
    drop(ea);
    drop(eb);
    drop(ta);
    drop(tb);
    drop(sa);
    drop(sb);
    state.storage().close().await.unwrap();
    drop(state);
    let (state, _) = boot_agent_embedding_plane(&home, &a, &b, &global, true).await;
    assert_eq!(
        count_before_restart,
        (
            a.calls.load(Ordering::SeqCst),
            b.calls.load(Ordering::SeqCst)
        )
    );
    let ta = bind_tools(&home, "lingxi", Some(state.memory().clone()));
    let restored = success(
        ta.call("search_memory", query.clone(), 206, "ex06-after-restart")
            .await
            .unwrap(),
    );
    assert_eq!(details(&restored)["semantic"], "used");
    assert!(text(&restored).contains("ALPHA_PRIVATE_BODY"));
    let before = (
        a.calls.load(Ordering::SeqCst),
        b.calls.load(Ordering::SeqCst),
    );
    let mut refused = Vec::new();
    for (i, reference) in [
        json!({"provider":"missing","id":"model-a"}),
        json!({"provider":"a","id":"missing-model"}),
        json!({"provider":"a"}),
        json!("not a reference"),
    ]
    .into_iter()
    .enumerate()
    {
        agent_embedding_ref(&home, "lingxi", reference.clone());
        let result = success(
            ta.call("search_memory", query.clone(), 210 + i, "ex06-invalid")
                .await
                .unwrap(),
        );
        assert_eq!(details(&result)["semantic"], "unavailable_unresolvable");
        refused.push(json!({"ref":reference,"result":details(&result)}));
    }
    agent_embedding_ref(&home, "lingxi", Value::Null);
    let no_model = success(
        ta.call("search_memory", query.clone(), 215, "ex06-no-model")
            .await
            .unwrap(),
    );
    assert_eq!(details(&no_model)["semantic"], "unavailable_no_model");
    assert_eq!(
        before,
        (
            a.calls.load(Ordering::SeqCst),
            b.calls.load(Ordering::SeqCst)
        )
    );
    assert_eq!(global.calls.load(Ordering::SeqCst), 0);
    agent_embedding_ref(&home, "lingxi", json!({"provider":"a","id":"model-a"}));
    drop(ta);
    state.storage().close().await.unwrap();
    drop(state);
    let (state, _) = boot_agent_embedding_plane(&home, &a, &b, &global, false).await;
    let ta = bind_tools(&home, "lingxi", Some(state.memory().clone()));
    let without_global = success(
        ta.call("search_memory", query, 216, "ex06-no-global")
            .await
            .unwrap(),
    );
    assert_eq!(details(&without_global)["semantic"], "used");
    assert_eq!(global.calls.load(Ordering::SeqCst), 0);
    assert!(a
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|r| r["model"] == "model-a"));
    assert!(b
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|r| r["model"] == "model-b"));
    let proof = json!({"home":home,"aWire":a.requests.lock().unwrap().clone(),"bWire":b.requests.lock().unwrap().clone(),"globalCalls":0,"differentSpaceKeys":[ia.key,ib.key],"aHit":details(&hit_a),"bHit":details(&hit_b),"wrongSpace":details(&wrong_space),"actualRun":run,"actualRunResult":actual,"actualChatWire":chat_wire,"usage":embedding_usage_rows(&home),"paidReceipts":paid_memory_receipts(&home),"invalid":refused,"noModel":details(&no_model),"afterRestart":details(&restored),"withoutGlobal":details(&without_global)});
    std::fs::write(
        home.join("ex06-per-agent-proof.json"),
        serde_json::to_vec_pretty(&proof).unwrap(),
    )
    .unwrap();
    println!(
        "EX06_PER_AGENT_PROOF {}",
        home.join("ex06-per-agent-proof.json").display()
    );
    state.storage().close().await.unwrap();
    a.handle.abort();
    b.handle.abort();
    global.handle.abort();
}
