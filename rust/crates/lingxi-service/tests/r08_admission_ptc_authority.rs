//! 独立补验：真实批准等待、实时撤权、工具配额及同请求重连；不修改产品或既有测试。
//! 仅外部模型使用替身；固定元数据时钟，实际任务、文件、授权、SQLite与事件续读均为正式实现。
#![cfg(unix)]
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        InvocationJournalEntry, InvocationPhase, ProviderDescriptor, ProviderTurn,
        ProviderTurnResult, ReceiptOutcome, StoragePort, ToolOutcome, ToolRequest, TurnDeltaSink,
        TurnProviderPort,
    },
    toolcatalog::{Availability, SchemaBudget, ToolManifest, ToolRegistry, ToolTargetRef},
    RunContext,
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, RunId, RunStatus};
use lingxi_service::{
    approval_service::{Answer, AnswerOutcome, ApprovalService, PendingView},
    coding_ptc,
    events::{SubscribeCursor, SubscribeOutcome},
    filehistory::FileProductChangeLog,
    inject::ManualClock,
    prepare_layout,
    quotas::{LayeredQuotaLimits, QuotaLimits, QuotaResource},
    resourceaccess::ResourceOp,
    toolgateway::ToolInvocationGateway,
    ExecuteSubmission, HomeSource, NetworkMode, ResourceAccess, ServiceConfig, ServiceDeps,
    ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const NOW: u64 = 1_800_000_000_000;
#[derive(Default)]
struct Provider {
    results: Mutex<BTreeMap<String, ToolOutcome>>,
    turns: AtomicUsize,
}
async fn eventually(check: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("实际状态未在限时内出现");
}

#[tokio::test]
async fn actual_resource_grant_revoked_while_child_waits_approval_refuses_dispatch() {
    let h = Harness::boot(QuotaLimits::default()).await;
    let session = "ptc-ask";
    // 目标不在默认工作区；没有这个真实会话授权，正式文件入口本来就不能写。
    let external = h.root.join("external-grant");
    std::fs::create_dir(&external).unwrap();
    let external = external.canonicalize().unwrap();
    let target = external.join("revoked.txt");
    let principal = lingxi_kernel::Principal::LocalUser;
    assert!(h
        .access
        .authorize(
            principal.storage_kind(),
            &principal.storage_subject(),
            session,
            &target,
            &h.workspace,
            ResourceOp::Write
        )
        .is_err());
    h.access
        .grant_root(
            principal.storage_kind(),
            &principal.storage_subject(),
            session,
            &external,
            ResourceOp::Write,
        )
        .unwrap();
    let code=format!("try {{await tools.write({{path:{},content:'MUST_NOT_WRITE'}});return 'unexpected';}} catch(e) {{return e.message;}}",
        serde_json::to_string(&target.to_string_lossy()).unwrap());
    let run = h.submit(session, &code).await;
    h.approve_outer(session, &run).await;
    let child = h.pending(session, "write").await;
    assert_eq!(child.run_id, run);
    assert!(
        child
            .resources
            .iter()
            .any(|scope| scope.path == target && scope.op == ResourceOp::Write),
        "{child:?}"
    );
    assert!(!target.exists());
    h.access.revoke_grants(
        principal.storage_kind(),
        &principal.storage_subject(),
        session,
    );
    assert!(h
        .access
        .authorize(
            principal.storage_kind(),
            &principal.storage_subject(),
            session,
            &target,
            &h.workspace,
            ResourceOp::Write
        )
        .is_err());
    h.approve(session, &child);
    h.wait(&run, RunStatus::Completed).await;
    assert!(!target.exists(), "旧批准不能复活已撤销的真实文件授权");
    h.assert_not_dispatched(&run).await;
    h.close().await;
}

async fn registry_change_during_child_approval(schema_change: bool) {
    let h = Harness::boot(QuotaLimits::default()).await;
    let session = "ptc-ask";
    let run=h.submit(session,"try {await tools.write({path:'stale.txt',content:'MUST_NOT_WRITE'});return 'unexpected';}catch(e){return e.message;}").await;
    h.approve_outer(session, &run).await;
    let child = h.pending(session, "write").await;
    let target = h
        .registry
        .resolve(&ToolTargetRef::ByName {
            name: "write".into(),
        })
        .unwrap();
    let old = h.registry.describe_full(&target).unwrap();
    let mutation = if schema_change {
        let mut schema = old.input_schema.clone();
        schema.schema["properties"]["newConsent"] = json!({"type":"string"});
        schema.schema["required"]
            .as_array_mut()
            .unwrap()
            .push(json!("newConsent"));
        let d = old.listing.clone();
        h.registry
            .update(
                &target,
                ToolManifest {
                    origin: d.origin,
                    local_name: d.local_name,
                    display_name: d.display_name,
                    aliases: d.aliases,
                    version: "2.0.0".into(),
                    description: d.description,
                    input_schema: schema,
                    output_schema: None,
                    permission: d.permission,
                    availability: d.availability,
                    timeout_ms: d.timeout_ms,
                    max_concurrency: None,
                    declared_permission: d.declared_permission,
                    recovery: d.recovery,
                },
                &SchemaBudget::default(),
            )
            .unwrap()
    } else {
        h.registry
            .set_availability(
                &target,
                Availability::Disabled {
                    reason: "合成验收期间用户禁用".into(),
                },
            )
            .unwrap()
    };
    assert!(mutation.target_generation > old.listing.target_generation);
    assert!(!h.workspace.join("stale.txt").exists());
    h.approve(session, &child);
    h.wait(&run, RunStatus::Completed).await;
    assert!(
        !h.workspace.join("stale.txt").exists(),
        "旧批准不能派发已换代工具"
    );
    h.assert_not_dispatched(&run).await;
    h.close().await;
}

#[tokio::test]
async fn actual_registry_disable_while_child_waits_approval_refuses_old_approval() {
    registry_change_during_child_approval(false).await;
}

#[tokio::test]
async fn actual_registry_schema_generation_change_refuses_old_approval() {
    registry_change_during_child_approval(true).await;
}

#[tokio::test]
async fn real_tool_quota_one_releases_parent_and_serializes_each_child() {
    let h = Harness::boot(QuotaLimits {
        tool: LayeredQuotaLimits {
            global: 1,
            per_agent: 1,
            per_session: 1,
        },
        ..QuotaLimits::default()
    })
    .await;
    let session = "ptc-ask";
    let run=h.submit(session,"await Promise.all(['first.txt','second.txt'].map(path=>tools.write({path,content:'ONE_REAL_WRITE'})));return 'two completed';").await;
    h.approve_outer(session, &run).await;
    // 编排父项若仍占唯一配额，这里根本不会出现子批准等待。
    let first = h.pending(session, "write").await;
    eventually(|| h.state.runs().quotas().waiting(QuotaResource::Tool) == 1).await;
    assert_eq!(h.state.runs().quotas().in_use(QuotaResource::Tool), 1);
    assert_eq!(
        h.state
            .runs()
            .quotas()
            .session_in_use(QuotaResource::Tool, session),
        1
    );
    assert_eq!(
        h.approvals
            .pending_for_principal(&lingxi_kernel::Principal::LocalUser, session)
            .len(),
        1
    );
    assert_eq!(first.resources.len(), 1);
    let first_path = first.resources[0].path.clone();
    assert!(!h.workspace.join("first.txt").exists() && !h.workspace.join("second.txt").exists());
    h.approve(session, &first);
    let second = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let next = h.pending(session, "write").await;
            if next.approval_id != first.approval_id {
                break next;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("第二子调用未取得同一配额");
    assert_eq!(h.state.runs().quotas().in_use(QuotaResource::Tool), 1);
    assert_eq!(h.state.runs().quotas().waiting(QuotaResource::Tool), 0);
    assert_eq!(
        std::fs::read_to_string(&first_path).unwrap(),
        "ONE_REAL_WRITE"
    );
    assert_eq!(second.resources.len(), 1);
    assert_ne!(second.resources[0].path, first_path);
    assert!(!second.resources[0].path.exists(), "排队子动作不得提前写");
    h.approve(session, &second);
    h.wait(&run, RunStatus::Completed).await;
    eventually(|| h.state.runs().quotas().in_use(QuotaResource::Tool) == 0).await;
    assert_eq!(h.state.runs().quotas().waiting(QuotaResource::Tool), 0);
    for path in ["first.txt", "second.txt"] {
        assert_eq!(
            std::fs::read_to_string(h.workspace.join(path)).unwrap(),
            "ONE_REAL_WRITE"
        );
    }
    let journal = h.child_journal(&run).await;
    assert_eq!(journal.len(), 2, "{journal:?}");
    assert!(
        journal
            .iter()
            .all(|row| row.phase == InvocationPhase::Succeeded
                && row
                    .receipt
                    .as_ref()
                    .is_some_and(|r| r.dispatched && r.outcome == ReceiptOutcome::Succeeded)),
        "{journal:?}"
    );
    println!("配额=1，两个真实子调用逐个完成：{journal:?}");
    h.close().await;
}

#[tokio::test]
async fn same_explicit_request_replay_and_real_event_resume_never_repeat_children() {
    let h = Harness::boot(QuotaLimits::default()).await;
    let session = "ptc-ask";
    std::fs::write(h.workspace.join("counter.txt"), "0").unwrap();
    let code="await tools.edit({path:'counter.txt',edits:[{oldText:'0',newText:'1'}]});return 'one real edit';";
    let request_id = "r08-ptc-reconnect-same-request";
    let accepted = h.submit_id(session, code, Some(request_id)).await;
    assert!(!accepted.replayed);
    let outer = h.pending(session, "run_tools").await;
    let inflight = h.submit_id(session, code, Some(request_id)).await;
    assert!(inflight.replayed, "同requestId恢复必须走正式幂等入口");
    assert_eq!(inflight.run_id, accepted.run_id);
    assert_eq!(inflight.run_count, accepted.run_count);
    let pending = h
        .approvals
        .pending_for_principal(&lingxi_kernel::Principal::LocalUser, session);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].approval_id, outer.approval_id);
    assert_eq!(h.provider.turns.load(Ordering::SeqCst), 1);
    h.approve(session, &outer);
    let edit = h.pending(session, "edit").await;
    h.approve(session, &edit);
    h.wait(&accepted.run_id, RunStatus::Completed).await;
    let journal = h.child_journal(&accepted.run_id).await;
    assert_eq!(journal.len(), 1, "{journal:?}");
    assert_eq!(journal[0].phase, InvocationPhase::Succeeded, "{journal:?}");
    assert!(journal[0].receipt.as_ref().unwrap().dispatched);
    let turns = h.provider.turns.load(Ordering::SeqCst);
    assert_eq!(turns, 2);
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("counter.txt")).unwrap(),
        "1"
    );

    // 游标只取自正式提交后快照；掉线后用同游标反复续读，不构造新用户命令。
    let (cursor, head) = match h
        .state
        .events()
        .subscribe(&h.principal(), session, None)
        .await
        .unwrap()
    {
        SubscribeOutcome::Started { cut, subscription } => {
            assert_eq!(cut.mode, "snapshot");
            assert!(!cut.events.is_empty());
            assert!(cut.next_cursor.is_none(), "样本须在真实快照的一页内");
            let cursor = SubscribeCursor {
                stream_id: cut.stream_id.clone(),
                seq: cut.snapshot_seq,
            }
            .encode();
            drop(subscription);
            (cursor, cut.snapshot_seq)
        }
        other => panic!("首次订阅未取得真实快照：{other:?}"),
    };
    for _ in 0..3 {
        match h
            .state
            .events()
            .subscribe(&h.principal(), session, Some(cursor.clone()))
            .await
            .unwrap()
        {
            SubscribeOutcome::Started { cut, subscription } => {
                assert_eq!(cut.mode, "resume");
                assert_eq!(cut.from_seq, head);
                assert_eq!(cut.snapshot_seq, head);
                assert!(cut.events.is_empty());
                drop(subscription);
            }
            other => panic!("有效游标不能被当成重发命令：{other:?}"),
        }
    }
    let completed = h.submit_id(session, code, Some(request_id)).await;
    assert!(completed.replayed);
    assert_eq!(completed.run_id, accepted.run_id);
    assert_eq!(completed.run_count, accepted.run_count);
    assert_eq!(h.provider.turns.load(Ordering::SeqCst), turns);
    assert_eq!(
        h.child_journal(&accepted.run_id).await,
        journal,
        "重连不能新增或改写子调用收据"
    );
    assert_eq!(
        h.state.events().stream_head(session).await.unwrap(),
        Some(head)
    );
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("counter.txt")).unwrap(),
        "1"
    );
    println!("同请求运行中/完成后均返回原Run；三次事件续读未改变实际文件和持久收据");
    h.close().await;
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "fixture.external".into(),
            model: "ptc".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(async move {
            self.turns.fetch_add(1, Ordering::SeqCst);
            if input.turn == 1 {
                let args: Value =
                    serde_json::from_str(input.submission.rsplit("\n\n").next().unwrap()).unwrap();
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            "tool:first-party:run_tools",
                            args,
                            &SchemaBudget::default(),
                        )
                        .unwrap()],
                        content: vec![],
                    },
                );
            }
            if let Some(outcome) = input.prior.iter().rev().find_map(|item| {
                if let ExchangeItem::ToolResult { outcome, .. } = item {
                    Some(outcome.clone())
                } else {
                    None
                }
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
                            text: "外部模型替身已收到程序结果".into(),
                        }],
                        model_call_id: Some(call.clone()),
                    },
                },
            )
        })
    }
}
struct Harness {
    root: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    provider: Arc<Provider>,
    approvals: Arc<ApprovalService>,
    registry: Arc<ToolRegistry>,
    access: Arc<ResourceAccess>,
}
impl Harness {
    async fn boot(limits: QuotaLimits) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r08-ptc-authority-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        let home = root.join("data");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            json!({"primary_agent_id":"a","current_agent_id":"a"}).to_string(),
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let root = root.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let registry = Arc::new(ToolRegistry::new());
        let provider = Arc::new(Provider::default());
        let gateway = Arc::new(ToolInvocationGateway::new(
            registry.clone(),
            approvals.clone(),
            clock.clone(),
            SchemaBudget::default(),
            300000,
            1000,
        ));
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home,
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 5000,
            },
            &layout,
            ServiceDeps {
                clock: clock.clone(),
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway.clone()),
                approval_gate: Some(approvals.clone()),
                workspace_root: Some(workspace.clone()),
                quota_limits: limits,
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        // 保存正式文件授权对象，撤销的是同一个执行器实际读取的授权。
        let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&workspace)).unwrap());
        lingxi_service::register_core_file_tools(
            &registry,
            &gateway,
            access.clone(),
            workspace.clone(),
            clock.clone(),
            Arc::new(FileProductChangeLog::new(
                state.file_history().clone(),
                state.session_files().clone(),
            )),
            &SchemaBudget::default(),
        );
        coding_ptc::register(
            &registry,
            &gateway,
            Arc::downgrade(state.runs()),
            state.storage().clone(),
            state.events().clone(),
            clock,
            &SchemaBudget::default(),
        )
        .unwrap();
        for (session, mode) in [("ptc-operate", "operate"), ("ptc-ask", "ask")] {
            state
                .session_admin()
                .create_session(
                    session,
                    "a",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "真实程序工具",
                    Some(mode),
                    &[],
                    false,
                    NOW,
                )
                .await
                .unwrap();
        }
        Self {
            root,
            workspace,
            state,
            provider,
            approvals,
            registry,
            access,
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
    async fn submit(&self, session: &str, code: &str) -> String {
        self.submit_id(session, code, None).await.run_id
    }
    async fn submit_id(
        &self,
        session: &str,
        code: &str,
        id: Option<&str>,
    ) -> lingxi_service::sessions::ExecuteAccepted {
        let input = json!({"code":code,"description":"真实程序撤权与重连验收"}).to_string();
        self.state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &self.principal(),
                session,
                &ExecuteSubmission {
                    input: &input,
                    knowledge: None,
                    request_id: id,
                },
                NOW,
            )
            .await
            .unwrap()
    }
    async fn wait(&self, run: &str, status: RunStatus) {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let row = self
                    .state
                    .storage()
                    .load_run(&RunId::new(run))
                    .await
                    .unwrap()
                    .unwrap();
                if row.status.is_terminal() {
                    assert_eq!(row.status, status, "{row:?}");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("真实Run未结束");
    }
    async fn pending(&self, session: &str, name: &str) -> PendingView {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let rows = self
                    .approvals
                    .pending_for_principal(&lingxi_kernel::Principal::LocalUser, session);
                if let Some(row) = rows
                    .into_iter()
                    .find(|row| row.target.ends_with(&format!(":{name}")))
                {
                    return row;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("未出现真实 {name} 批准等待"))
    }
    fn approve(&self, session: &str, pending: &PendingView) {
        assert!(matches!(
            self.approvals.answer_for_principal(
                &lingxi_kernel::Principal::LocalUser,
                session,
                &pending.approval_id,
                Answer::Approve
            ),
            AnswerOutcome::Settled(_)
        ));
    }
    async fn approve_outer(&self, session: &str, run: &str) {
        let outer = self.pending(session, "run_tools").await;
        assert_eq!(outer.run_id, run);
        self.approve(session, &outer);
    }
    async fn child_journal(&self, run: &str) -> Vec<InvocationJournalEntry> {
        self.state
            .storage()
            .load_invocation_journal(&RunId::new(run))
            .await
            .unwrap()
            .into_iter()
            .filter(|row| row.journal_id.contains(":ptc:"))
            .collect()
    }
    async fn assert_not_dispatched(&self, run: &str) {
        let journal = self.child_journal(run).await;
        assert_eq!(journal.len(), 1, "{journal:?}");
        assert_eq!(journal[0].phase, InvocationPhase::Failed, "{journal:?}");
        let receipt = journal[0].receipt.as_ref().expect("拒绝必须有持久收据");
        assert_eq!(receipt.outcome, ReceiptOutcome::Failed, "{receipt:?}");
        assert!(!receipt.dispatched, "撤权后不得交给文件执行器：{receipt:?}");
        assert!(!receipt.detail.is_empty(), "拒绝收据须说明原因");
        println!("拒绝收据：{receipt:?}");
    }
    async fn close(self) {
        self.state.conversations().stop();
        self.state.conversations().wait_stopped().await.unwrap();
        self.state.storage().close().await.unwrap();
        std::fs::remove_dir_all(self.root).unwrap();
    }
}
