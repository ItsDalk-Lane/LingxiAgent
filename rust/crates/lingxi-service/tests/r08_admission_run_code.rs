//! 只替代外部模型响应；Run/存储/授权网关/解释器/进程组均用真实实现。
#![cfg(unix)]
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    subagent::SessionPermissionMode,
    toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetRef},
    RunContext,
};
use lingxi_protocol::{
    AttemptId, ContentBlock, ModelCallId, NormalizedMessage, RunId, RunStatus, SessionId,
    ToolCallId,
};
use lingxi_service::{
    approval_service::ApprovalService,
    coding_run_code,
    inject::ManualClock,
    prepare_layout,
    toolgateway::{
        CallerSurface, InvocationPermissionContext, InvocationRequest, PolicyVerdict,
        ToolInvocationGateway,
    },
    ExecuteSubmission, HomeSource, NetworkMode, ProcessSupervisor, ResourceAccess, ServiceConfig,
    ServiceDeps, ServiceState, SupervisorLimits,
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
            if input.turn == 1 && !input.submission.contains("[workflow-result task=") {
                let body: Value = serde_json::from_str(
                    input
                        .submission
                        .rsplit("\n\n")
                        .next()
                        .unwrap_or(&input.submission),
                )
                .unwrap();
                let (target, body) = if body["tool"] == "workflow" {
                    ("tool:first-party:workflow", body["args"].clone())
                } else {
                    ("tool:first-party:run_code", body)
                };
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            target,
                            body,
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
    gateway: Arc<ToolInvocationGateway>,
    supervisor: Arc<ProcessSupervisor>,
    provider: Arc<Provider>,
}
impl Harness {
    async fn boot() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r08-run-code-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        let home = root.join("data");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
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
        let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&workspace)).unwrap());
        // 注入provider时正式组合根保留测试自有的真实工具平面；调用正式registrar。
        coding_run_code::register(
            &registry,
            &gateway,
            access,
            workspace.clone(),
            supervisor.clone(),
            clock.clone(),
            &SchemaBudget::default(),
        )
        .unwrap();
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
                    &[workspace.to_string_lossy().into_owned()],
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
            gateway,
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
    async fn invoke(&self, session: &str, language: &str, action: &str, code: &str) -> ToolOutcome {
        let run = self
            .submit(
                session,
                json!({"language":language,"action":action,"code":code}),
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
        self.state
            .workflows()
            .stop(Duration::from_secs(5))
            .await
            .unwrap();
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
    Value::Object(
        successful(outcome)
            .details
            .clone()
            .expect("内核必须报告结构化状态"),
    )
}
async fn eventually(check: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("实际副作用未在预算内发生");
}
fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[tokio::test]
async fn absent_real_interpreters_report_install_guidance_without_starting_kernel() {
    let empty = std::env::temp_dir().join(format!(
        "lingxi-r08-empty-path-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&empty).unwrap();
    // 在独立测试进程收窄 PATH，不改并行测试或当前服务的环境。
    let output = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "missing_language_child", "--nocapture"])
        .env("PATH", &empty)
        .env("LINGXI_R08_MISSING_LANGUAGE_CHILD", "1")
        .output()
        .await
        .unwrap();
    std::fs::remove_dir_all(empty).unwrap();
    assert!(
        output.status.success(),
        "缺语言真实子进程失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("MISSING_LANGUAGE_ASSERTED"));
}

#[tokio::test]
async fn missing_language_child() {
    if std::env::var("LINGXI_R08_MISSING_LANGUAGE_CHILD").as_deref() != Ok("1") {
        return;
    }
    let h = Harness::boot().await;
    for language in ["python3", "node"] {
        let outcome = h
            .invoke("kernel-a", language, "run", "print('must not execute')")
            .await;
        assert!(matches!(outcome, ToolOutcome::Failed { .. }));
        assert!(text(&outcome).contains("RUN_CODE_LANGUAGE_MISSING"));
        assert!(text(&outcome).contains("环境依赖设置"));
        assert!(h.supervisor.live_handles().is_empty());
    }
    h.close().await;
    println!("MISSING_LANGUAGE_ASSERTED");
}

#[tokio::test]
async fn actual_code_output_and_side_effect_reload_from_real_history_without_reexecution() {
    let h = Harness::boot().await;
    let outcome=h.invoke("kernel-a","python3","run","from pathlib import Path\np=Path('one-effect.txt')\np.write_text(str(int(p.read_text())+1) if p.exists() else '1')\nprint('DURABLE_OUTPUT_实际')").await;
    successful(&outcome);
    let history = lingxi_service::model_history::ModelHistory::new(h.state.storage().clone());
    for _ in 0..2 {
        let restored = history
            .load(&lingxi_kernel::Principal::LocalUser, "a", "kernel-a", "")
            .await
            .unwrap();
        assert!(restored.iter().any(|item|matches!(item,ExchangeItem::ToolResult{outcome:stored,..} if text(stored).contains("DURABLE_OUTPUT_实际"))));
        assert_eq!(
            std::fs::read_to_string(h.workspace.join("one-effect.txt")).unwrap(),
            "1",
            "读历史不能重复执行代码"
        );
    }
    h.close().await;
}

#[tokio::test]
async fn python_and_node_real_kernels_preserve_variables_functions_imports_and_isolate_agent_sessions(
) {
    let h = Harness::boot().await;
    for (language, setup, query, missing) in [
        (
            "python3",
            "import math\nx=40\ndef plus(n): return n+2",
            "print(plus(x),math.isqrt(81))",
            "print(x)",
        ),
        (
            "node",
            "const path=require('node:path'); const x=40; function plus(n){return n+2}",
            "console.log(plus(x),path.basename('/a/b'))",
            "console.log(x)",
        ),
    ] {
        let unstarted = h.invoke("kernel-a", language, "status", "").await;
        assert_eq!(details(&unstarted)["started"], false);
        let first = h.invoke("kernel-a", language, "run", setup).await;
        let first_id = details(&first)["kernelId"].clone();
        let second = h.invoke("kernel-a", language, "run", query).await;
        assert_eq!(details(&second)["kernelId"], first_id);
        assert!(text(&second).contains("42"));
        if language == "node" {
            let imported=h.invoke("kernel-a",language,"run","const fs = await import('node:fs'); console.log(typeof fs.readFile,await Promise.resolve(19))").await;
            assert!(
                text(&imported).contains("function") && text(&imported).contains("19"),
                "顶层 await/import 必须真实完成：{imported:?}"
            );
            let retained = h
                .invoke(
                    "kernel-a",
                    language,
                    "run",
                    "console.log(typeof fs.readFile)",
                )
                .await;
            assert!(
                text(&retained).contains("function"),
                "动态 import 必须跨调用保留：{retained:?}"
            );
        }
        for foreign in ["kernel-a-other", "kernel-b"] {
            let outcome = h.invoke(foreign, language, "run", missing).await;
            assert!(
                matches!(outcome, ToolOutcome::Failed { .. }),
                "不同agent/session不应共享变量：{outcome:?}"
            );
            assert!(text(&outcome).contains("RUN_CODE_EXECUTION_FAILED"));
        }
        let fresh = h.invoke("kernel-a", language, "restart", "").await;
        assert_ne!(details(&fresh)["kernelId"], first_id);
        assert_eq!(details(&fresh)["restarted"], true);
        let gone = h.invoke("kernel-a", language, "run", missing).await;
        assert!(matches!(gone, ToolOutcome::Failed { .. }));
        assert!(text(&gone).contains("not defined"));
    }
    h.close().await;
}

#[tokio::test]
async fn real_gateway_status_remains_read_only_and_code_never_inherits_service_secrets() {
    let h = Harness::boot().await;
    let ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("kernel-a"),
        run_id: RunId::new("read-only-status"),
        attempt: AttemptId::new("read-only-status#a1"),
        generation: 1,
    };
    for action in ["run", "status", "restart"] {
        let request = InvocationRequest::from_trusted_entry(
            &ctx,
            CallerSurface::UserRun,
            "a",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::ReadOnly,
            },
            ToolTargetRef::ByName {
                name: "run_code".into(),
            },
            None,
            json!({"action":action,"language":"python3","code":"print('must not execute')"}),
            ToolCallId::new(format!("read-only-{action}")),
        );
        let prepared = h.gateway.prepare(request);
        if action == "status" {
            assert_eq!(prepared.unwrap().policy, PolicyVerdict::Allowed);
        } else {
            assert!(prepared.is_err(), "只读不允许run/restart");
        }
    }
    let python = h
        .invoke(
            "kernel-a",
            "python3",
            "run",
            "import os; print(sorted(os.environ.keys()))",
        )
        .await;
    let node = h
        .invoke(
            "kernel-a",
            "node",
            "run",
            "console.log(Object.keys(process.env).sort())",
        )
        .await;
    for result in [&python, &node] {
        let body = text(result);
        successful(result);
        for forbidden in [
            "LINGXI_HOME",
            "LINGXI_SERVICE_TOKEN",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "NODE_OPTIONS",
            "PYTHONPATH",
        ] {
            assert!(!body.contains(forbidden), "解释器继承服务环境：{body}");
        }
    }
    let invalid = h.invoke("kernel-a", "ruby", "run", "puts 1").await;
    assert!(matches!(invalid, ToolOutcome::Failed { .. }));
    assert!(text(&invalid).contains("RUN_CODE_INVALID_LANGUAGE"));
    assert!(h
        .state
        .sessions()
        .execute_background_for(
            h.state.storage(),
            h.state.events(),
            h.state.runs(),
            h.state.background(),
            &h.principal(),
            "no-such-session",
            &ExecuteSubmission::plain("{}"),
            NOW
        )
        .await
        .is_err());
    h.close().await;
}

#[tokio::test]
async fn output_is_real_bounded_head_tail_utf8_and_dead_kernel_requires_explicit_restart() {
    let h = Harness::boot().await;
    let output = h
        .invoke(
            "kernel-a",
            "python3",
            "run",
            "print('REAL_HEAD'+('汉'*30000)+'REAL_TAIL')",
        )
        .await;
    assert!(successful(&output).truncated);
    let body = text(&output);
    assert!(body.contains("REAL_HEAD") && body.contains("REAL_TAIL"));
    assert!(!body.contains('\u{fffd}'));
    assert!(body.len() < 52_000);
    let death = h
        .invoke("kernel-a", "python3", "run", "import os; os._exit(7)")
        .await;
    assert!(matches!(death, ToolOutcome::Failed { .. }));
    assert!(text(&death).contains("RUN_CODE_KERNEL_EXITED"));
    let no_respawn = h
        .invoke(
            "kernel-a",
            "python3",
            "run",
            "print('must not silently respawn')",
        )
        .await;
    assert!(text(&no_respawn).contains("RUN_CODE_KERNEL_DEAD"));
    let fresh = h.invoke("kernel-a", "python3", "restart", "").await;
    assert_eq!(details(&fresh)["restarted"], true);
    let works = h
        .invoke("kernel-a", "python3", "run", "print('RESTARTED_REAL')")
        .await;
    assert!(text(&works).contains("RESTARTED_REAL"));
    h.close().await;
}

#[tokio::test]
async fn cancelling_real_run_stops_persistent_python_kernel_and_its_actual_grandchild() {
    let h = Harness::boot().await;
    let code="import os,sys,subprocess,json,time\np=subprocess.Popen([sys.executable,'-c','import time; time.sleep(120)'])\nwith open('cancel-pids.json','w') as f: json.dump([os.getpid(),p.pid],f)\nprint('kernel and child started',flush=True)\ntime.sleep(120)";
    let run = h
        .submit(
            "kernel-a",
            json!({"language":"python3","action":"run","code":code}),
        )
        .await;
    let file = h.workspace.join("cancel-pids.json");
    eventually(|| file.is_file()).await;
    let pids: Vec<i32> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(pids.len(), 2);
    assert!(pids.iter().all(|pid| alive(*pid)));
    assert!(
        !h.state
            .storage()
            .load_run(&RunId::new(&run))
            .await
            .unwrap()
            .unwrap()
            .status
            .is_terminal(),
        "代码仍执行时不得先将Run标完成"
    );
    let ctx = h
        .provider
        .contexts
        .lock()
        .unwrap()
        .get(&run)
        .cloned()
        .unwrap();
    let call = ToolCallId::new("live-kernel-status");
    let prepared = h
        .gateway
        .prepare(InvocationRequest::from_trusted_entry(
            &ctx,
            CallerSurface::UserRun,
            "a",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::ReadOnly,
            },
            ToolTargetRef::ByName {
                name: "run_code".into(),
            },
            None,
            json!({"language":"python3","action":"status","code":""}),
            call.clone(),
        ))
        .unwrap();
    let status = h
        .gateway
        .execute_prepared(&ctx, &call, &prepared.handle)
        .await
        .unwrap()
        .outcome;
    assert_eq!(details(&status)["stillBusy"], true);
    assert_eq!(details(&status)["codeCompleted"], false);
    h.state
        .sessions()
        .cancel_run_for(
            h.state.storage().as_ref(),
            h.state.runs(),
            &h.principal(),
            &run,
        )
        .await
        .unwrap();
    h.wait(&run, RunStatus::Cancelled).await;
    eventually(|| pids.iter().all(|pid| !alive(*pid))).await;
    assert!(h.supervisor.live_handles().is_empty());
    h.close().await;
}

#[tokio::test]
async fn real_thirty_second_cap_reports_timeout_keeps_busy_state_and_restart_ends_it_without_replay(
) {
    let h = Harness::boot().await;
    let run=h.submit("kernel-a",json!({"language":"python3","action":"run","code":"import time\nx=77\nprint('REAL_STARTED',flush=True)\ntime.sleep(120)\nprint('MUST_NOT_FINISH')"})).await;
    h.wait(&run, RunStatus::Completed).await;
    let outcome = h
        .provider
        .results
        .lock()
        .unwrap()
        .get(&run)
        .cloned()
        .unwrap();
    assert!(matches!(outcome, ToolOutcome::Failed { .. }));
    assert!(text(&outcome).contains("RUN_CODE_TIMEOUT"));
    assert!(!text(&outcome).contains("MUST_NOT_FINISH"));
    let status = h.invoke("kernel-a", "python3", "status", "").await;
    assert_eq!(details(&status)["stillBusy"], true);
    assert_eq!(details(&status)["codeCompleted"], false);
    let old = details(&status)["kernelId"].as_str().unwrap().to_string();
    let restart = h.invoke("kernel-a", "python3", "restart", "").await;
    assert_ne!(details(&restart)["kernelId"], old);
    let lost = h.invoke("kernel-a", "python3", "run", "print(x)").await;
    assert!(text(&lost).contains("not defined"));
    h.close().await;
}

#[tokio::test]
async fn real_workflow_write_roots_cannot_be_bypassed_by_persistent_interpreter_run_or_restart() {
    let h = Harness::boot().await;
    let narrow = h.workspace.join("allowed-workflow");
    std::fs::create_dir(&narrow).unwrap();
    let forbidden = h.workspace.join("forbidden-workflow.txt");
    for (session, action) in [
        ("kernel-a", "run"),
        ("kernel-a-other", "restart"),
        ("kernel-b", "status"),
    ] {
        let payload=json!({"language":"python3","action":action,"code":format!("from pathlib import Path\nPath({:?}).write_text('MUST_NOT_WRITE')",forbidden.to_string_lossy())}).to_string();
        let script=format!("export const meta={{name:'受限工作流代码边界',description:'合成反向验收'}};return await agent({},{{access:'write',writeFolders:[{}]}});",serde_json::to_string(&payload).unwrap(),serde_json::to_string(&narrow.to_string_lossy()).unwrap());
        let parent = h
            .submit(session, json!({"tool":"workflow","args":{"script":script}}))
            .await;
        h.wait(&parent, RunStatus::Completed).await;
        let child = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let child = h
                    .provider
                    .contexts
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(run, _ctx)| {
                        *run != &parent && h.provider.results.lock().unwrap().contains_key(*run)
                    })
                    .map(|(run, ctx)| (run.clone(), ctx.clone()));
                if let Some(child) = child {
                    return child;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("真实工作流节点必须实际到达工具结果");
        let result = h.provider.results.lock().unwrap().remove(&child.0).unwrap();
        assert!(!forbidden.exists(), "窄write_roots之外的兄弟文件不得创建");
        assert!(
            h.supervisor.live_handles().is_empty(),
            "工作流拒绝不得启动持久内核"
        );
        if action == "status" {
            assert_eq!(details(&result)["started"], false);
        } else {
            assert!(matches!(result, ToolOutcome::Failed { .. }), "{result:?}");
            assert!(
                text(&result).contains("RUN_CODE_WORKFLOW_UNSUPPORTED"),
                "{result:?}"
            );
        }
        h.wait(&child.0, RunStatus::Completed).await;
        eventually(|| h.state.workflows().active_count() == 0).await;
        h.provider.contexts.lock().unwrap().clear();
        h.provider.results.lock().unwrap().clear();
    }
    h.close().await;
}
