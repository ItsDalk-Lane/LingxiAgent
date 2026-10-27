//! 只替代外部模型响应；Run/存储/授权网关/解释器/进程组均用真实实现。
#![cfg(unix)]
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
    RunContext,
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, RunId, RunStatus};
use lingxi_service::{
    approval_service::ApprovalService, coding_security, inject::ManualClock, prepare_layout,
    toolgateway::ToolInvocationGateway, ExecuteSubmission, HomeSource, NetworkMode,
    ProcessSupervisor, ResourceAccess, ServiceConfig, ServiceDeps, ServiceState, SupervisorLimits,
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
    contexts: Mutex<BTreeMap<String, RunContext>>,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "fixture.external".into(),
            model: "run-code".into(),
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
            self.contexts
                .lock()
                .unwrap()
                .insert(ctx.run_id.to_string(), ctx.clone());
            if input.turn == 1 {
                let body: Value = serde_json::from_str(
                    input
                        .submission
                        .rsplit("\n\n")
                        .next()
                        .unwrap_or(&input.submission),
                )
                .unwrap();
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            body["tool"].as_str().unwrap(),
                            body["args"].clone(),
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
                            text: "外部模型替身已收到真实工具结果".into(),
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
    supervisor: Arc<ProcessSupervisor>,
    provider: Arc<Provider>,
}
impl Harness {
    async fn boot() -> Self {
        Self::boot_mode(false).await
    }
    async fn boot_mode(read_only: bool) -> Self {
        Self::boot_subroot(read_only, false).await
    }
    async fn boot_subroot(read_only: bool, subroot: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r08-security-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        let home = root.join("data");
        let repository = root.join("workspace");
        let workspace = if subroot {
            repository.join("sub")
        } else {
            repository.clone()
        };
        std::fs::create_dir_all(&workspace).unwrap();
        if subroot {
            let output = std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&repository)
                .output()
                .unwrap();
            assert!(output.status.success());
            std::fs::write(repository.join("outside_scope.ts"), "parent-only-sentinel").unwrap();
            std::fs::write(workspace.join("inside.ts"), "const value=1;").unwrap();
        }
        for agent in ["a", "b"] {
            std::fs::create_dir_all(home.join(format!("agents/{agent}"))).unwrap();
            std::fs::write(
                home.join(format!("agents/{agent}/config.yaml")),
                format!("agent:\n  name: {agent}\nmemory:\n  enabled: false\n"),
            )
            .unwrap();
        }
        std::fs::create_dir_all(home.join("user")).unwrap();
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
        let provider = Arc::new(Provider::default());
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let registry = Arc::new(ToolRegistry::new());
        let gateway = Arc::new(ToolInvocationGateway::new(
            registry.clone(),
            approvals.clone(),
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
        let supervisor = Arc::new(
            ProcessSupervisor::new(
                clock.clone(),
                SupervisorLimits::with_spill_dir(root.join("spill")),
            )
            .unwrap(),
        );
        let access = Arc::new(
            ResourceAccess::new(if read_only {
                &[]
            } else {
                std::slice::from_ref(&workspace)
            })
            .unwrap(),
        );
        if read_only {
            access
                .grant_root(
                    "local_user",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "kernel-a",
                    &workspace,
                    lingxi_service::resourceaccess::ResourceOp::Read,
                )
                .unwrap();
        }
        // 注入provider时正式组合根保留测试自有的真实工具平面；调用正式registrar。
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
                clock,
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway.clone()),
                approval_gate: Some(approvals),
                workspace_root: Some(workspace.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        coding_security::register(
            &registry,
            &gateway,
            access,
            workspace.clone(),
            state.config().data_home.clone(),
            state.resource_io().clone(),
            None,
            state.clock().clone(),
            &SchemaBudget::default(),
        )
        .unwrap();
        for (session, agent) in [
            ("kernel-a", "a"),
            ("kernel-a-other", "a"),
            ("kernel-b", "b"),
        ] {
            state
                .session_admin()
                .create_session(
                    session,
                    agent,
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "真实代码内核",
                    Some("operate"),
                    &[],
                    true,
                    NOW,
                )
                .await
                .unwrap();
        }
        Self {
            root,
            workspace,
            state,
            supervisor,
            provider,
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
    async fn submit(&self, session: &str, args: Value) -> String {
        self.state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &self.principal(),
                session,
                &ExecuteSubmission::plain(&args.to_string()),
                NOW,
            )
            .await
            .unwrap()
            .run_id
    }
    async fn wait(&self, run: &str, expected: RunStatus) {
        tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                let row = self
                    .state
                    .storage()
                    .load_run(&RunId::new(run))
                    .await
                    .unwrap()
                    .unwrap();
                if row.status.is_terminal() {
                    assert_eq!(row.status, expected, "真实Run状态不符：{row:?}");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("真实Run未在预算内结束");
    }
    async fn invoke(&self, tool: &str, args: Value) -> ToolOutcome {
        let run = self
            .submit(
                "kernel-a",
                json!({"tool":format!("tool:first-party:{tool}"),"args":args}),
            )
            .await;
        self.wait(&run, RunStatus::Completed).await;
        self.provider
            .results
            .lock()
            .unwrap()
            .get(&run)
            .cloned()
            .expect("真实工具结果未回到模型")
            .clone()
    }
    async fn close(self) {
        let receipts = self.supervisor.shutdown_all().await;
        assert!(
            self.supervisor.live_handles().is_empty(),
            "真实内核未清理：{receipts:?}"
        );
        self.state.conversations().stop();
        self.state.conversations().wait_stopped().await.unwrap();
        self.state.storage().close().await.unwrap();
        std::fs::remove_dir_all(self.root).unwrap();
    }
}
fn text(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|block| {
                if let ContentBlock::Text { text } = block {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        ToolOutcome::Failed { error } => error.message.clone(),
        other => format!("{other:?}"),
    }
}
fn successful(outcome: &ToolOutcome) -> &lingxi_kernel::ports::ToolSuccess {
    if let ToolOutcome::Success { result } = outcome {
        result
    } else {
        panic!("应为真实代码执行成功，实际：{outcome:?}")
    }
}
fn details(outcome: &ToolOutcome) -> Value {
    successful(outcome)
        .details
        .clone()
        .map(Value::Object)
        .unwrap_or(Value::Null)
}

include!("r08_admission_security_cases.inc");
