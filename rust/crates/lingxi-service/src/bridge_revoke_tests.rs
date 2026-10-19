//! 撤权发生在真实 Run 已接收、收件尚未绑定之间时，仍必须按因果记录取消。
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
    subagent::SessionPermissionMode,
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry, ToolTargetId,
    },
};
use lingxi_protocol::{BridgeRole, ModelCallId, ToolCallId, ToolSchemaDocument};
use std::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
};

struct ParkedProvider {
    entered: Arc<tokio::sync::Semaphore>,
    release: Option<Arc<tokio::sync::Semaphore>>,
    calls: Arc<AtomicUsize>,
    write_execute: bool,
}
impl TurnProviderPort for ParkedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.revoke.fixture".into(),
            model: "synthetic".into(),
            operation: "chat".into(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _sink: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.add_permits(1);
            // 仅供应商响应暂停；任务、身份、收件与取消全部使用正式服务。
            match &self.release {
                None => std::future::pending().await,
                Some(release) => {
                    release.acquire().await.unwrap().forget();
                    ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::ToolRequests {
                            requests: if self.write_execute {
                                [
                                    "tool:first-party:bridge_write_probe",
                                    "tool:first-party:bridge_exec_probe",
                                ]
                                .into_iter()
                                .map(|target| {
                                    ToolRequest::from_effective_arguments(
                                        target,
                                        json!({}),
                                        &SchemaBudget::default(),
                                    )
                                    .unwrap()
                                })
                                .collect()
                            } else {
                                vec![ToolRequest::from_effective_arguments(
                                    "tool:first-party:bridge_probe",
                                    json!({}),
                                    &SchemaBudget::default(),
                                )
                                .unwrap()]
                            },
                            content: vec![],
                        },
                    )
                }
            }
        })
    }
}
struct ProbeExecutor(Arc<AtomicUsize>);
impl ToolExecutorPort for ProbeExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            ToolExecutionResult::of_ctx(ctx, ToolOutcome::success_text("实际工具已调用"))
        })
    }
}

#[tokio::test]
async fn revoke_cancels_accepted_unbound_run_after_inbox_becomes_unknown() {
    revoke_case(false, false).await;
}

#[tokio::test]
async fn revoked_while_model_parked_cancels_before_any_tool_dispatch() {
    revoke_case(true, false).await;
}

#[tokio::test]
async fn permission_narrowed_while_model_parked_cancels_before_write_or_exec_dispatch() {
    revoke_case(true, true).await;
}

async fn revoke_case(resume_model: bool, narrow_mode: bool) {
    let home = std::env::temp_dir().join(format!(
        "r07-bridge-revoke-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
    std::fs::write(
        home.join("agents/lingxi/config.yaml"),
        "agent:\n  name: 撤权验证助手\nmemory:\n  enabled: false\n",
    )
    .unwrap();
    let home = home.canonicalize().unwrap();
    let layout = prepare_layout(&home).unwrap();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let executions = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(ToolRegistry::new());
    for name in if narrow_mode {
        vec!["bridge_write_probe", "bridge_exec_probe"]
    } else {
        vec!["bridge_probe"]
    } {
        registry.register(
            ToolManifest {
                origin: ToolOrigin::FirstParty,
                local_name: name.into(),
                display_name: "撤权验证工具".into(),
                aliases: vec![],
                version: "1.0.0".into(),
                description: "记录正式网关是否实际派发了该工具".into(),
                input_schema: ToolSchemaDocument {
                    dialect: "json-schema/2020-12".into(),
                    schema: json!({"type":"object","properties":{},"additionalProperties":false}),
                },
                output_schema: None,
                permission: PermissionContract {
                    kind: if narrow_mode { PermissionKind::Execute } else { PermissionKind::Read },
                    capability_base: "bridge_probe.read".into(),
                },
                availability: Availability::Available,
                timeout_ms: Some(30_000),
                max_concurrency: Some(1),
                declared_permission: DeclaredPermission::None,
                recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
            },
            &SchemaBudget::default(),
        )
        .unwrap();
    }
    let gateway = Arc::new(ToolInvocationGateway::new(
        registry,
        Arc::new(ApprovalService::new(clock.clone())),
        clock.clone(),
        SchemaBudget::default(),
        300_000,
        1000,
    ));
    for target in if narrow_mode {
        vec![
            "tool:first-party:bridge_write_probe",
            "tool:first-party:bridge_exec_probe",
        ]
    } else {
        vec!["tool:first-party:bridge_probe"]
    } {
        gateway.bind_executor(
            ToolTargetId::parse(target),
            Arc::new(ProbeExecutor(executions.clone())),
            "真实网关派发计数",
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
            turn_provider: Some(Arc::new(ParkedProvider {
                entered: entered.clone(),
                release: resume_model.then(|| release.clone()),
                calls: calls.clone(),
                write_execute: narrow_mode,
            })),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap();
    let http = if narrow_mode {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let router = crate::build_router(state.clone());
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
        let response = reqwest::Client::new()
            .post(format!("{url}/lingxi/v1/bridge/settings"))
            .bearer_auth(state.auth().local_token())
            .json(&json!({"permissionMode":"operate"}))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().await.unwrap()
        );
        Some((url, stop, task))
    } else {
        None
    };
    let cfg = store::put_connector(
        state.storage(),
        &store::BridgeConnectorInput {
            id: "revoke-connector".into(),
            platform: "telegram".into(),
            studio_id: "fixture-studio".into(),
            actor_id: "lingxi".into(),
            owner_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
            owner_sender_id: Some("native-owner".into()),
            enabled: true,
            config_json: "{}".into(),
        },
        None,
        1_800_000_000_001,
    )
    .await
    .unwrap();
    let inbox = store::claim_inbox(
        state.storage(),
        &store::BridgeInboxInput {
            platform: "telegram".into(),
            connector_id: cfg.input.id.clone(),
            conversation_id: "native-dm".into(),
            conversation_kind: store::ConversationKind::Dm,
            message_id: "native-message".into(),
            sender_id: "native-owner".into(),
            payload_json: json!({"text":"暂停供应商后撤权"}).to_string(),
            attachments: vec![],
        },
        cfg.generation,
        1_800_000_000_002,
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
            Some(if narrow_mode { "operate" } else { "ask" }),
            &[],
            false,
            1_800_000_000_003,
        )
        .await
        .unwrap();
    store::prepare_inbox_session(
        state.storage(),
        &inbox.id,
        cfg.generation,
        sid,
        &[],
        1_800_000_000_004,
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
            "暂停供应商后撤权",
            None,
            &inbox.id,
            &inbox.input.message_id,
            BridgeRole::Owner,
            if narrow_mode {
                SessionPermissionMode::Operate
            } else {
                SessionPermissionMode::Ask
            },
            None,
            1_800_000_000_005,
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), entered.acquire())
        .await
        .expect("真实供应商调用应已开始")
        .unwrap()
        .forget();
    let run_id = RunId::new(&accepted.run_id);
    assert!(!state
        .storage()
        .load_run(&run_id)
        .await
        .unwrap()
        .unwrap()
        .status
        .is_terminal());
    if narrow_mode {
        let response = reqwest::Client::new()
            .post(format!(
                "{}/lingxi/v1/bridge/settings",
                http.as_ref().unwrap().0
            ))
            .bearer_auth(state.auth().local_token())
            .json(&json!({"permissionMode":"read_only"}))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().await.unwrap()
        );
        assert_eq!(
            store::get_connector(state.storage(), &cfg.input.id)
                .await
                .unwrap()
                .unwrap()
                .generation,
            cfg.generation,
            "本回归只收窄全局模式，不依赖连接换代取消"
        );
    } else {
        let mut disabled = cfg.input.clone();
        disabled.enabled = false;
        store::put_connector(
            state.storage(),
            &disabled,
            Some(cfg.generation),
            1_800_000_000_006,
        )
        .await
        .unwrap();
        let revoked = store::get_inbox(state.storage(), &inbox.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(revoked.status, store::BridgeInboxStatus::Unknown);
        assert_eq!(revoked.run_id, None);
        assert_eq!(
            store::get_inbox_run(state.storage(), &inbox.id)
                .await
                .unwrap()
                .unwrap()
                .0,
            accepted.run_id
        );
        assert!(store::bind_inbox(
            state.storage(),
            &inbox.id,
            cfg.generation,
            sid,
            &accepted.run_id,
            1_800_000_000_007,
        )
        .await
        .is_err());
    }

    if resume_model {
        // 不调用宿主清理；真实模型返回工具请求后，运行自身必须先观察撤权再取消。
        release.add_permits(1);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if state
                    .storage()
                    .load_run(&run_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .status
                    .is_terminal()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("撤权后的运行必须自行完成取消");
    } else {
        // 与宿主实际撤权收尾调用同一个函数，不能依赖 inbox.run_id 或 pending 状态。
        cancel_connector_runs(&state, &cfg).await.unwrap();
    }
    let status = state
        .storage()
        .load_run(&run_id)
        .await
        .unwrap()
        .unwrap()
        .status;
    let report = state
        .background()
        .drain_within(Duration::from_secs(5))
        .await;
    state.bridge().stop();
    state.bridge().wait_stopped().await.unwrap();
    if let Some((_, stop, task)) = http {
        let _ = stop.send(());
        task.await.unwrap();
    }
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
    assert_eq!(status, RunStatus::Cancelled);
    assert!(report.unconfirmed.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}
