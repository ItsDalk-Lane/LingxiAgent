//! 外部模型是确定性替身；真正的 Run、审批、网关、双份消息、文件回执和因果存储均走正式实现。
use lingxi_adapters::{
    conversation_files::{ConversationFiles, ConversationMutation},
    storage::conversations as store,
};
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    subagent::{SessionPermissionMode, ToolAccessTier},
    toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetRef},
    RunContext,
};
use lingxi_protocol::{
    AttemptId, ContentBlock, ModelCallId, NormalizedMessage, RunId, RunStatus, SessionId,
    ToolCallId,
};
use lingxi_service::{
    approval_service::{Answer, ApprovalService, ACTION_BLOCKED_BY_READ_ONLY},
    inject::ManualClock,
    prepare_layout,
    toolgateway::{
        CallerSurface, GatewayRefusal, InvocationPermissionContext, InvocationRequest,
        PolicyVerdict, ToolInvocationGateway,
    },
    ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

const NOW: u64 = 1_800_000_000_000;
#[derive(Default)]
struct Provider {
    results: Mutex<BTreeMap<String, ToolOutcome>>,
    held: Mutex<Option<RunContext>>,
    release: tokio::sync::Notify,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.fixture".into(),
            model: "conversation-tools".into(),
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
            if input.turn == 1 {
                let body = serde_json::from_str::<Value>(
                    input
                        .submission
                        .rsplit("\n\n")
                        .next()
                        .unwrap_or(&input.submission),
                )
                .unwrap_or(Value::Null);
                if body["hold"] == true {
                    *self.held.lock().unwrap() = Some(ctx.clone());
                    self.release.notified().await;
                }
                if let Some(name) = body["tool"].as_str() {
                    return ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: vec![ToolRequest::from_effective_arguments(
                                format!("tool:first-party:{name}"),
                                body["args"].clone(),
                                &SchemaBudget::default(),
                            )
                            .unwrap()],
                            content: vec![],
                        },
                    );
                }
            }
            if let Some(outcome) = input.prior.iter().rev().find_map(|item| match item {
                ExchangeItem::ToolResult { outcome, .. } => Some(outcome.clone()),
                _ => None,
            }) {
                self.results
                    .lock()
                    .unwrap()
                    .insert(ctx.run_id.to_string(), outcome);
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: "本地替身完成".into(),
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
    gateway: Arc<ToolInvocationGateway>,
    approvals: Arc<ApprovalService>,
    provider: Arc<Provider>,
    files: ConversationFiles,
}
impl Harness {
    async fn boot(mode: &str) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-conversation-tools-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        for actor in ["a", "b", "c"] {
            std::fs::create_dir_all(home.join(format!("agents/{actor}"))).unwrap();
            std::fs::write(
                home.join(format!("agents/{actor}/config.yaml")),
                format!("agent:\n  name: {actor}\nmemory:\n  enabled: false\n"),
            )
            .unwrap();
        }
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        // 焦点与主助手均为 b，实际工具来源必须仍取 source-a 的 a。
        std::fs::write(
            home.join("user/preferences.json"),
            json!({"channels_enabled":true,"primary_agent_id":"b","current_agent_id":"b"})
                .to_string(),
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let provider = Arc::new(Provider::default());
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            approvals.clone(),
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
                clock,
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway.clone()),
                approval_gate: Some(approvals.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        state
            .session_admin()
            .create_session(
                "source-a",
                "a",
                lingxi_kernel::LOCAL_OWNER_SUBJECT,
                "工具来源",
                Some(mode),
                &[],
                true,
                NOW,
            )
            .await
            .unwrap();
        assert!(
            gateway
                .registry()
                .describe(
                    &lingxi_kernel::toolcatalog::tool_target_id(
                        &lingxi_kernel::toolcatalog::ToolOrigin::FirstParty,
                        "channel"
                    )
                    .unwrap()
                )
                .is_ok(),
            "生产组合根必须登记正式工具"
        );
        let files = ConversationFiles::new(home.clone()).unwrap();
        Self {
            home,
            state,
            gateway,
            approvals,
            provider,
            files,
        }
    }
    fn principal(&self) -> lingxi_service::Principal {
        self.state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", self.state.auth().local_token())),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap()
    }
    async fn channel(&self, id: &str, name: &str, roster: &[&str]) {
        self.files
            .apply(
                self.state.storage(),
                &format!("fixture-create-{id}"),
                &ConversationMutation::CreateChannel {
                    id: id.into(),
                    meta: json!({"id":id,"name":name,"members":roster}),
                    intro: None,
                    timestamp: "2027-01-15 12:00:00".into(),
                },
                NOW,
            )
            .await
            .unwrap();
    }
    async fn submit(&self, body: Value) -> String {
        self.state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &self.principal(),
                "source-a",
                &ExecuteSubmission::plain(&body.to_string()),
                NOW,
            )
            .await
            .unwrap()
            .run_id
    }
    async fn wait(&self, run: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let row = self
                    .state
                    .storage()
                    .load_run(&RunId::new(run))
                    .await
                    .unwrap()
                    .unwrap();
                if row.status.is_terminal() {
                    assert_eq!(
                        row.status,
                        RunStatus::Completed,
                        "真实 Run 意外失败：{row:?}"
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("真实工具 Run 未在10秒内完成");
    }
    async fn pending(&self, run: &str) -> String {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(p) = self
                    .approvals
                    .pending_for_principal(&lingxi_kernel::Principal::LocalUser, "source-a")
                    .into_iter()
                    .find(|p| p.run_id == run)
                {
                    return p.approval_id;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("写工具没有进入真实逐次确认")
    }
    fn approve(&self, id: &str) {
        assert!(self
            .approvals
            .answer_for_principal(
                &lingxi_kernel::Principal::LocalUser,
                "source-a",
                id,
                Answer::Approve
            )
            .settled());
    }
    fn outcome(&self, run: &str) -> ToolOutcome {
        self.provider
            .results
            .lock()
            .unwrap()
            .get(run)
            .cloned()
            .expect("模型必须收到真实工具结果")
    }
    fn success(&self, run: &str) -> Value {
        match self.outcome(run) {
            ToolOutcome::Success { result } => Value::Object(result.details.unwrap()),
            other => panic!("应成功，实际为 {other:?}"),
        }
    }
    async fn close(self) {
        self.state.conversations().stop();
        self.state.conversations().wait_stopped().await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.state.storage().close())
            .await
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(self.home).unwrap();
    }
    fn request(
        &self,
        ctx: &RunContext,
        name: &str,
        args: Value,
        permission: InvocationPermissionContext,
        call: &str,
    ) -> InvocationRequest {
        InvocationRequest::from_trusted_entry(
            ctx,
            CallerSurface::UserRun,
            "a",
            permission,
            ToolTargetRef::ByName { name: name.into() },
            None,
            args,
            ToolCallId::new(call),
        )
    }
}

#[tokio::test]
async fn dynamic_channel_contract_preserves_read_review_and_inherited_read_only() {
    let h = Harness::boot("operate").await;
    h.channel("ch_shared", "same", &["a", "b"]).await;
    let ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("source-a"),
        run_id: RunId::new("matrix"),
        attempt: AttemptId::new("matrix#a1"),
        generation: 1,
    };
    for action in [
        json!({"action":"list"}),
        json!({"action":"read","channel":"ch_shared"}),
    ] {
        let prepared = h
            .gateway
            .prepare(h.request(
                &ctx,
                "channel",
                action,
                InvocationPermissionContext::UserSession {
                    mode: SessionPermissionMode::ReadOnly,
                },
                "read",
            ))
            .unwrap();
        assert_eq!(prepared.policy, PolicyVerdict::Allowed);
    }
    for (name, args) in [
        (
            "channel",
            json!({"action":"post","channel":"ch_shared","content":"review"}),
        ),
        (
            "channel",
            json!({"action":"create","name":"new","members":["b"]}),
        ),
        ("dm", json!({"to":"b","message":"review"})),
    ] {
        let prepared = h
            .gateway
            .prepare(h.request(
                &ctx,
                name,
                args.clone(),
                InvocationPermissionContext::UserSession {
                    mode: SessionPermissionMode::Operate,
                },
                "write",
            ))
            .unwrap();
        assert!(
            matches!(prepared.policy, PolicyVerdict::NeedsApproval { .. }),
            "operate不能免除Review"
        );
        assert!(
            matches!(h.gateway.prepare(h.request(&ctx,name,args,InvocationPermissionContext::UserSession{mode:SessionPermissionMode::ReadOnly},"blocked")),Err(GatewayRefusal::PolicyDenied{code,..})if code==ACTION_BLOCKED_BY_READ_ONLY)
        );
    }
    for mode in [SessionPermissionMode::Ask, SessionPermissionMode::Operate] {
        let prepared = h
            .gateway
            .prepare(h.request(
                &ctx,
                "session",
                json!({"action":"create","agent":"b","message":"proposal only"}),
                InvocationPermissionContext::UserSession { mode },
                "allowed-proposal",
            ))
            .unwrap();
        assert_eq!(prepared.policy, PolicyVerdict::Allowed);
    }
    // session 工具虽然只产卡，也不能把只读子授权提升为可写建议。
    for permission in [
        InvocationPermissionContext::UserSession {
            mode: SessionPermissionMode::ReadOnly,
        },
        InvocationPermissionContext::Automation {
            mode: SessionPermissionMode::ReadOnly,
        },
        InvocationPermissionContext::Subagent {
            tier: ToolAccessTier::ReadOnly,
            parent_mode: SessionPermissionMode::Operate,
        },
        InvocationPermissionContext::Subagent {
            tier: ToolAccessTier::Operate,
            parent_mode: SessionPermissionMode::ReadOnly,
        },
    ] {
        for args in [
            json!({"action":"send","sessionId":"target","message":"no"}),
            json!({"action":"create","agent":"b","message":"no"}),
        ] {
            assert!(
                matches!(h.gateway.prepare(h.request(&ctx,"session",args,permission,"proposal")),Err(GatewayRefusal::PolicyDenied{code,..})if code==ACTION_BLOCKED_BY_READ_ONLY)
            );
        }
    }
    h.close().await;
}
#[tokio::test]
async fn real_read_run_is_member_scoped_and_ambiguous_names_list_ids() {
    let h = Harness::boot("read_only").await;
    h.channel("ch_shared", "shared", &["a", "b"]).await;
    h.channel("ch_private", "private", &["b", "c"]).await;
    let run = h
        .submit(json!({"tool":"channel","args":{"action":"list"}}))
        .await;
    h.wait(&run).await;
    let data = h.success(&run);
    assert_eq!(data["channels"].as_array().unwrap().len(), 1);
    assert_eq!(data["channels"][0]["id"], "ch_shared");
    assert!(!data.to_string().contains("ch_private"));
    let run = h
        .submit(json!({"tool":"channel","args":{"action":"read","channel":"ch_private"}}))
        .await;
    h.wait(&run).await;
    assert!(matches!(h.outcome(&run), ToolOutcome::Failed { .. }));
    h.channel("ch_duplicate", "shared", &["a", "c"]).await;
    let run = h
        .submit(json!({"tool":"channel","args":{"action":"read","channel":"shared"}}))
        .await;
    h.wait(&run).await;
    match h.outcome(&run) {
        ToolOutcome::Failed { error } => {
            assert!(error.message.contains("ch_shared"));
            assert!(error.message.contains("ch_duplicate"));
        }
        other => panic!("歧义应拒绝：{other:?}"),
    }
    h.close().await;
}
#[tokio::test]
async fn real_channel_post_waits_for_approval_and_records_actual_actor_and_source() {
    let h = Harness::boot("operate").await;
    h.channel("ch_shared", "shared", &["a", "b"]).await;
    let run=h.submit(json!({"tool":"channel","args":{"action":"post","channel":"ch_shared","content":"actual a post"}})).await;
    let pending = h.pending(&run).await;
    assert!(h
        .files
        .read_channel("ch_shared")
        .unwrap()
        .messages
        .is_empty());
    h.approve(&pending);
    h.wait(&run).await;
    let data = h.success(&run);
    let op = data["operationId"].as_str().unwrap();
    assert!(op.starts_with("conversation-tool-"));
    let doc = h.files.read_channel("ch_shared").unwrap();
    assert_eq!(doc.messages.len(), 1);
    assert_eq!(doc.messages[0].sender, "a");
    assert_eq!(
        store::message_operation_source(h.state.storage(), op)
            .await
            .unwrap()
            .as_deref(),
        Some(run.as_str())
    );
    let deliveries = store::list_pending_deliveries(h.state.storage())
        .await
        .unwrap();
    assert!(
        deliveries
            .iter()
            .any(|d| d.input.source_run_id.as_deref() == Some(run.as_str())
                && d.input.actor_id == "b")
    );
    h.close().await;
}
#[tokio::test]
async fn real_dm_confirmation_writes_both_histories_once_as_session_actor() {
    let h = Harness::boot("operate").await;
    let run = h
        .submit(json!({"tool":"dm","args":{"to":"b","message":"first actual DM"}}))
        .await;
    let pending = h.pending(&run).await;
    assert!(!h.home.join("agents/a/dm/b.md").exists());
    assert!(!h.home.join("agents/b/dm/a.md").exists());
    h.approve(&pending);
    h.wait(&run).await;
    let data = h.success(&run);
    assert_eq!(data["from"], "a");
    assert_eq!(data["to"], "b");
    let mine = h.files.read_dm("a", "b").unwrap();
    let theirs = h.files.read_dm("b", "a").unwrap();
    assert_eq!(mine.messages, theirs.messages);
    assert_eq!(mine.messages.len(), 1);
    assert_eq!(mine.messages[0].sender, "a");
    let op = data["operationId"].as_str().unwrap();
    assert_eq!(
        store::message_operation_source(h.state.storage(), op)
            .await
            .unwrap()
            .as_deref(),
        Some(run.as_str())
    );
    h.close().await;
}
#[tokio::test]
async fn real_create_adds_actual_actor_and_keeps_a_durable_source_receipt() {
    let h = Harness::boot("operate").await;
    let run=h.submit(json!({"tool":"channel","args":{"action":"create","name":"created by a","members":["b"],"intro":"actual introduction"}})).await;
    let pending = h.pending(&run).await;
    assert!(h.files.list_channels().unwrap().is_empty());
    h.approve(&pending);
    h.wait(&run).await;
    let data = h.success(&run);
    let doc = h
        .files
        .read_channel(data["channel"].as_str().unwrap())
        .unwrap();
    assert_eq!(doc.meta["members"], json!(["a", "b"]));
    assert_eq!(
        store::message_operation_source(h.state.storage(), data["operationId"].as_str().unwrap())
            .await
            .unwrap()
            .as_deref(),
        Some(run.as_str())
    );
    h.close().await;
}
#[tokio::test]
async fn membership_revoked_during_real_approval_cannot_publish() {
    let h = Harness::boot("operate").await;
    h.channel("ch_shared", "shared", &["a", "b", "c"]).await;
    let run=h.submit(json!({"tool":"channel","args":{"action":"post","channel":"ch_shared","content":"must not appear"}})).await;
    let pending = h.pending(&run).await;
    h.files
        .apply(
            h.state.storage(),
            "fixture-revoke",
            &ConversationMutation::PatchChannel {
                id: "ch_shared".into(),
                patch: json!({"members":["b","c"]}),
            },
            NOW,
        )
        .await
        .unwrap();
    h.approve(&pending);
    h.wait(&run).await;
    assert!(matches!(h.outcome(&run), ToolOutcome::Failed { .. }));
    assert!(h
        .files
        .read_channel("ch_shared")
        .unwrap()
        .messages
        .is_empty());
    h.close().await;
}
#[tokio::test]
async fn executor_rechecks_real_run_generation_attempt_and_owner() {
    let h = Harness::boot("operate").await;
    h.channel("ch_shared", "shared", &["a", "b"]).await;
    let run = h.submit(json!({"hold":true})).await;
    let ctx = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(ctx) = h.provider.held.lock().unwrap().clone() {
                break ctx;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    for (index, mut changed) in [ctx.clone(), ctx.clone(), ctx.clone()]
        .into_iter()
        .enumerate()
    {
        match index {
            0 => changed.generation += 1,
            1 => changed.attempt = lingxi_kernel::attempt_id(&ctx.run_id, 2),
            _ => {
                changed.principal = lingxi_kernel::Principal::Device {
                    device_id: "other".into(),
                    user_id: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
                }
            }
        }
        let call = ToolCallId::new(format!("identity-probe-{index}"));
        let request = h.request(
            &changed,
            "channel",
            json!({"action":"list"}),
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            call.as_str(),
        );
        let prepared = h.gateway.prepare(request).unwrap();
        let result = h
            .gateway
            .execute_prepared(&changed, &call, &prepared.handle)
            .await
            .unwrap();
        assert!(matches!(result.outcome, ToolOutcome::Failed { .. }));
    }
    h.provider.release.notify_one();
    h.wait(&run).await;
    assert!(h
        .files
        .read_channel("ch_shared")
        .unwrap()
        .messages
        .is_empty());
    h.close().await;
}
