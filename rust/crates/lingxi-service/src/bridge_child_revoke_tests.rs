//! 真正由 Bridge 父 Run 委派的子 Run，也必须在实际工具派发前观察撤权。
use super::*;
use crate::{
    approval_service::ApprovalService, inject::ManualClock, prepare_layout,
    toolgateway::ToolInvocationGateway, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
};
use lingxi_kernel::{
    model_exchange::ModelTurnInput,
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult,
        ToolExecutorPort, ToolOutcome, ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    subagent::{RunOrigin, SessionPermissionMode},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
};
use lingxi_protocol::{BridgeRole, ModelCallId, ToolCallId, ToolSchemaDocument};
use std::{future::Future, pin::Pin, sync::atomic::AtomicUsize};

const CHILD_TASK: &str = "真实桥接子任务：等待后尝试写入和执行";
const PROBES: [&str; 2] = ["bridge_child_write_probe", "bridge_child_exec_probe"];

struct DelegatingProvider {
    parent_finishes: bool,
    parent_parked: Arc<tokio::sync::Semaphore>,
    child_parked: Arc<tokio::sync::Semaphore>,
    child_release: Arc<tokio::sync::Semaphore>,
    child_id: Arc<Mutex<Option<String>>>,
}
impl TurnProviderPort for DelegatingProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.bridge-child.fixture".into(),
            model: "synthetic".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _sink: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(async move {
            if input.submission == CHILD_TASK {
                if input.turn == 1 {
                    *self.child_id.lock().unwrap() = Some(ctx.run_id.to_string());
                    self.child_parked.add_permits(1);
                    self.child_release.acquire().await.unwrap().forget();
                    ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: PROBES
                                .into_iter()
                                .map(|name| {
                                    let target = format!("tool:first-party:{name}");
                                    assert!(
                                        input.tools.bind_target(&target).is_some(),
                                        "子任务须收到真实工具声明"
                                    );
                                    ToolRequest::from_effective_arguments(
                                        target,
                                        json!({}),
                                        &SchemaBudget::default(),
                                    )
                                    .unwrap()
                                })
                                .collect(),
                            content: vec![],
                        },
                    )
                } else {
                    ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::Final {
                            message: NormalizedMessage {
                                role: "assistant".into(),
                                content: vec![ContentBlock::Text {
                                    text: "子任务已完成".into(),
                                }],
                                model_call_id: Some(call.clone()),
                            },
                        },
                    )
                }
            } else if input.turn == 1 {
                assert!(
                    input
                        .tools
                        .bind_target("tool:first-party:subagent")
                        .is_some(),
                    "父任务须从正式目录取得委派工具"
                );
                ProviderTurnResult::of_ctx(ctx, ProviderTurn::ToolRequests {
                    requests: vec![ToolRequest::from_effective_arguments(
                        "tool:first-party:subagent",
                        json!({"task":CHILD_TASK,"agent":"helper","access":"write","label":"桥接权限继承检查"}),
                        &SchemaBudget::default(),
                    ).unwrap()],
                    content: vec![],
                })
            } else {
                // 父亲仍停在外部模型请求，不能靠父亲下一次派发检查替孩子补做撤权。
                self.parent_parked.add_permits(1);
                if self.parent_finishes {
                    ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::Final {
                            message: NormalizedMessage {
                                role: "assistant".into(),
                                content: vec![ContentBlock::Text {
                                    text: "父任务已完成，子任务仍在执行".into(),
                                }],
                                model_call_id: Some(call.clone()),
                            },
                        },
                    )
                } else {
                    std::future::pending().await
                }
            }
        })
    }
}
struct CountingExecutor(Arc<AtomicUsize>);
impl ToolExecutorPort for CountingExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            ToolExecutionResult::of_ctx(
                ctx,
                ToolOutcome::success_text("真实网关已经派发子任务工具"),
            )
        })
    }
}

#[derive(Clone, Copy)]
enum Change {
    None,
    ReadOnly,
    Disable,
    ParentCompletedDisable,
}

#[tokio::test]
async fn bridge_child_operate_positive_control_reaches_both_real_executors() {
    delegated_case(Change::None).await;
}
#[tokio::test]
async fn bridge_child_observes_actual_permission_downgrade_while_parent_model_is_parked() {
    delegated_case(Change::ReadOnly).await;
}
#[tokio::test]
async fn bridge_child_observes_actual_connector_disable_while_parent_model_is_parked() {
    delegated_case(Change::Disable).await;
}

#[tokio::test]
async fn connector_cleanup_cancels_parked_real_child_after_parent_completed_without_rewriting_parent(
) {
    delegated_case(Change::ParentCompletedDisable).await;
}

async fn post(state: &ServiceState, url: &str, path: &str, body: Value) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{url}{path}"))
        .bearer_auth(state.auth().local_token())
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: Value = response.json().await.unwrap();
    assert!(status.is_success(), "{status}: {value}");
    value
}
async fn arrived(semaphore: &tokio::sync::Semaphore, label: &str) {
    tokio::time::timeout(Duration::from_secs(10), semaphore.acquire())
        .await
        .unwrap_or_else(|_| panic!("{label}未进入真实模型请求"))
        .unwrap()
        .forget();
}
async fn delegated_case(change: Change) {
    let home = std::env::temp_dir().join(format!(
        "r07-bridge-child-revoke-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    for agent in ["lingxi", "helper"] {
        std::fs::create_dir_all(home.join(format!("agents/{agent}"))).unwrap();
        std::fs::write(
            home.join(format!("agents/{agent}/config.yaml")),
            format!("agent:\n  name: {agent}\nmemory:\n  enabled: false\n"),
        )
        .unwrap();
    }
    let home = home.canonicalize().unwrap();
    let layout = prepare_layout(&home).unwrap();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let parent_parked = Arc::new(tokio::sync::Semaphore::new(0));
    let child_parked = Arc::new(tokio::sync::Semaphore::new(0));
    let child_release = Arc::new(tokio::sync::Semaphore::new(0));
    let child_id = Arc::new(Mutex::new(None));
    let executions = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(ToolRegistry::new());
    let gateway = Arc::new(ToolInvocationGateway::new(
        registry.clone(),
        Arc::new(ApprovalService::new(clock.clone())),
        clock.clone(),
        SchemaBudget::default(),
        300_000,
        1000,
    ));
    crate::subagents::register_tools(&registry, &gateway, &SchemaBudget::default());
    registry
        .add_model_resident(&lingxi_kernel::toolcatalog::ToolTargetId::parse(
            "tool:first-party:subagent",
        ))
        .unwrap();
    for name in PROBES {
        let receipt = registry.register(ToolManifest {
            origin: ToolOrigin::FirstParty, local_name: name.into(), display_name: "子任务派发检查".into(), aliases: vec![], version: "1.0.0".into(),
            description: "只记录正式网关是否已实际调用".into(),
            input_schema: ToolSchemaDocument { dialect: "json-schema/2020-12".into(), schema: json!({"type":"object","properties":{},"additionalProperties":false}) },
            output_schema: None,
            permission: PermissionContract { kind: PermissionKind::Execute, capability_base: format!("{name}.execute") },
            availability: Availability::Available, timeout_ms: Some(30_000), max_concurrency: Some(1), declared_permission: DeclaredPermission::Execute,
            recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
        }, &SchemaBudget::default()).unwrap();
        registry.add_model_resident(&receipt.target_id).unwrap();
        gateway.bind_executor(
            receipt.target_id,
            Arc::new(CountingExecutor(executions.clone())),
            "仅替换外部执行器并计数",
        );
    }
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
            tool_gateway: Some(gateway),
            turn_provider: Some(Arc::new(DelegatingProvider {
                parent_finishes: matches!(change, Change::ParentCompletedDisable),
                parent_parked: parent_parked.clone(),
                child_parked: child_parked.clone(),
                child_release: child_release.clone(),
                child_id: child_id.clone(),
            })),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = crate::build_router(state.clone());
    let http = tokio::spawn(async move {
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
    post(
        &state,
        &url,
        "/lingxi/v1/bridge/settings",
        json!({"permissionMode":"operate"}),
    )
    .await;
    post(&state, &url, "/lingxi/v1/bridge/config?agentId=lingxi", json!({"platform":"telegram","enabled":false,"credentials":{"token":"synthetic-no-network-token"}})).await;
    post(
        &state,
        &url,
        "/lingxi/v1/bridge/owner?agentId=lingxi",
        json!({"platform":"telegram","userId":"native-owner"}),
    )
    .await;
    post(
        &state,
        &url,
        "/lingxi/v1/bridge/config?agentId=lingxi",
        json!({"platform":"telegram","enabled":true}),
    )
    .await;
    let cfg = store::list_connectors(state.storage())
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.input.platform == "telegram")
        .unwrap();
    let inbox = store::claim_inbox(
        state.storage(),
        &store::BridgeInboxInput {
            platform: "telegram".into(),
            connector_id: cfg.input.id.clone(),
            conversation_id: "native-dm".into(),
            conversation_kind: store::ConversationKind::Dm,
            message_id: "delegating-owner-message".into(),
            sender_id: "native-owner".into(),
            payload_json: json!({"text":"真实桥接父任务：请委派子任务"}).to_string(),
            attachments: vec![],
        },
        cfg.generation,
        1_800_000_000_001,
    )
    .await
    .unwrap()
    .inbox;
    let sid = inbox.session_id.as_deref().unwrap();
    let principal =
        crate::auth::Principal::bridge_actor("telegram", &cfg.input.id, "native-owner").unwrap();
    state
        .session_admin()
        .create_session(
            sid,
            "lingxi",
            principal.user_id.as_deref().unwrap(),
            "Bridge",
            Some("operate"),
            &[],
            false,
            1_800_000_000_002,
        )
        .await
        .unwrap();
    store::prepare_inbox_session(
        state.storage(),
        &inbox.id,
        cfg.generation,
        sid,
        &[],
        1_800_000_000_003,
    )
    .await
    .unwrap();
    let accepted = state
        .sessions()
        .execute_bridge_for(
            state.storage(),
            state.events(),
            state.runs(),
            state.background(),
            &principal,
            sid,
            "真实桥接父任务：请委派子任务",
            None,
            &inbox.id,
            &inbox.input.message_id,
            BridgeRole::Owner,
            SessionPermissionMode::Operate,
            None,
            1_800_000_000_004,
        )
        .await
        .unwrap();
    store::bind_inbox(
        state.storage(),
        &inbox.id,
        cfg.generation,
        sid,
        &accepted.run_id,
        1_800_000_000_005,
    )
    .await
    .unwrap();
    arrived(&parent_parked, "父任务下一轮").await;
    arrived(&child_parked, "子任务").await;
    let child = RunId::new(child_id.lock().unwrap().clone().unwrap());
    let lineage = state
        .storage()
        .load_run_lineage(&child)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lineage.origin, RunOrigin::Subagent);
    assert_eq!(
        lineage.parent_run_id.as_ref().map(|id| id.as_str()),
        Some(accepted.run_id.as_str())
    );
    assert!(lineage.cause_id.as_deref().is_some_and(|id| !id.is_empty()));
    assert_eq!(
        state
            .storage()
            .load_run(&child)
            .await
            .unwrap()
            .unwrap()
            .session_id
            .as_str(),
        sid
    );
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    if matches!(change, Change::ParentCompletedDisable) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let parent = state
                    .storage()
                    .load_run(&RunId::new(&accepted.run_id))
                    .await
                    .unwrap()
                    .unwrap();
                if parent.status.is_terminal() {
                    assert_eq!(parent.status, RunStatus::Completed);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("真实父任务应独立完成，不能靠假造父终态制造场景");
        store::settle_inbox(
            state.storage(),
            &inbox.id,
            store::BridgeInboxStatus::Succeeded,
            Some(json!({"runId":accepted.run_id})),
            None,
            1_800_000_000_006,
        )
        .await
        .unwrap();
    }
    match change {
        Change::None => {}
        Change::ReadOnly => {
            post(
                &state,
                &url,
                "/lingxi/v1/bridge/settings",
                json!({"permissionMode":"read_only"}),
            )
            .await;
            assert_eq!(
                store::get_connector(state.storage(), &cfg.input.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .generation,
                cfg.generation,
                "此场景不能借连接换代取消代替权限复核"
            );
        }
        Change::Disable | Change::ParentCompletedDisable => {
            post(
                &state,
                &url,
                "/lingxi/v1/bridge/stop?agentId=lingxi",
                json!({"platform":"telegram"}),
            )
            .await;
            assert!(
                !store::get_connector(state.storage(), &cfg.input.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .input
                    .enabled
            );
        }
    }
    if matches!(change, Change::ParentCompletedDisable) {
        // 使用真实宿主的停用回收入口；子供应商一直不返回，必须主动取消活后代。
        assert_eq!(child_release.available_permits(), 0);
        cancel_connector_runs(&state, &cfg).await.unwrap();
    } else {
        // 只恢复子模型，不恢复父模型，也不调用 BridgeHost 的取消扫描。
        child_release.add_permits(1);
    }
    let child_status = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let run = state.storage().load_run(&child).await.unwrap().unwrap();
            if run.status.is_terminal() {
                break run.status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("子运行必须自行检查来源并经正式终态收口");
    let expected = if matches!(change, Change::None) {
        RunStatus::Completed
    } else {
        RunStatus::Cancelled
    };
    assert_eq!(child_status, expected);
    assert_eq!(
        executions.load(Ordering::SeqCst),
        if matches!(change, Change::None) { 2 } else { 0 }
    );
    let parent = state
        .storage()
        .load_run(&RunId::new(&accepted.run_id))
        .await
        .unwrap()
        .unwrap();
    if matches!(change, Change::ParentCompletedDisable) {
        assert_eq!(parent.status, RunStatus::Completed);
        assert_eq!(
            child_release.available_permits(),
            0,
            "不能放开子供应商来触发下一轮权限检查"
        );
    } else {
        assert!(
            !parent.status.is_terminal(),
            "父模型仍在暂停，子任务不能借父亲派发检查补做撤权"
        );
        state
            .sessions()
            .cancel_run_for(
                state.storage().as_ref(),
                state.runs(),
                &principal,
                &accepted.run_id,
            )
            .await
            .unwrap();
    }
    let report = state
        .background()
        .drain_within(Duration::from_secs(5))
        .await;
    assert!(
        report.unconfirmed.is_empty(),
        "统一取消回收不能留下未知后台任务"
    );
    assert_eq!(
        state
            .storage()
            .load_run(&RunId::new(&accepted.run_id))
            .await
            .unwrap()
            .unwrap()
            .status,
        if matches!(change, Change::ParentCompletedDisable) {
            RunStatus::Completed
        } else {
            RunStatus::Cancelled
        }
    );
    state.bridge().stop();
    state.bridge().wait_stopped().await.unwrap();
    let _ = stop.send(());
    http.await.unwrap();
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
}
