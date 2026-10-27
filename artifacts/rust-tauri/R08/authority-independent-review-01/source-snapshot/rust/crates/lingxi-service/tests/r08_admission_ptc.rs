//! 只替代外部模型，程序、授权、子调用、文件和持久存储均使用正式实现。
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
    approval_service::{Answer, AnswerOutcome, ApprovalService},
    coding_ptc,
    filehistory::FileProductChangeLog,
    inject::ManualClock,
    prepare_layout,
    toolgateway::ToolInvocationGateway,
    ExecuteSubmission, HomeSource, NetworkMode, ResourceAccess, ServiceConfig, ServiceDeps,
    ServiceState,
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
}
impl Harness {
    async fn boot() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r08-ptc-{}",
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
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        // 正式文件 registrar、正式文件历史和正式 PTC registrar，不绑定假执行器。
        lingxi_service::register_core_file_tools(
            &registry,
            &gateway,
            Arc::new(ResourceAccess::new(std::slice::from_ref(&workspace)).unwrap()),
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
        self.state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &self.principal(),
                session,
                &ExecuteSubmission::plain(
                    &json!({"code":code,"description":"真实程序编排验收"}).to_string(),
                ),
                NOW,
            )
            .await
            .unwrap()
            .run_id
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
    async fn invoke(&self, session: &str, code: &str) -> ToolOutcome {
        let run = self.submit(session, code).await;
        self.wait(&run, RunStatus::Completed).await;
        self.provider
            .results
            .lock()
            .unwrap()
            .get(&run)
            .cloned()
            .unwrap()
    }
    fn raw_ui_details(&self) -> Value {
        // 原始提交后事件才是UI展开材料；不能把32KB模型投影当完整UI收据。
        let connection = rusqlite::Connection::open_with_flags(
            self.state.storage().db_path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let body:String=connection.query_row("SELECT payload_json FROM key_events WHERE event_type='tool_call_completed' ORDER BY rowid DESC LIMIT 1",[],|row|row.get(0)).unwrap();
        serde_json::from_str::<Value>(&body).unwrap()["result"]["details"].clone()
    }
    async fn close(self) {
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
            .filter_map(|b| {
                if let ContentBlock::Text { text } = b {
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
fn details(outcome: &ToolOutcome) -> Value {
    match outcome {
        ToolOutcome::Success { result } => Value::Object(result.details.clone().unwrap()),
        ToolOutcome::Failed { error } => Value::Object(error.details.clone().unwrap()),
        other => panic!("结果未确认：{other:?}"),
    }
}
async fn eventually(check: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn real_parallel_children_only_return_selected_summary_and_keep_bounded_ui_receipts() {
    let h = Harness::boot().await;
    std::fs::write(h.workspace.join("a.txt"), "SECRET_CHILD_A").unwrap();
    std::fs::write(h.workspace.join("b.txt"), "SECRET_CHILD_B").unwrap();
    let result=h.invoke("ptc-operate","const values: string[] = await Promise.all(['a.txt','b.txt'].map(path=>tools.read({path}))); console.log('selected summary'); return values.length;").await;
    assert!(matches!(result, ToolOutcome::Success { .. }), "{result:?}");
    assert!(text(&result).contains("selected summary") && text(&result).contains('2'));
    assert!(!text(&result).contains("SECRET_CHILD"));
    let calls = details(&result)["subcalls"].as_array().unwrap().clone();
    assert_eq!(calls.len(), 2);
    for call in calls {
        assert_eq!(call["ok"], true, "{call:?}");
        assert!(call["output"].as_str().unwrap().contains("SECRET_CHILD"));
        assert!(call["args"].is_object());
    }
    let history = lingxi_service::model_history::ModelHistory::new(h.state.storage().clone())
        .load(&lingxi_kernel::Principal::LocalUser, "a", "ptc-operate", "")
        .await
        .unwrap();
    let results: Vec<_> = history
        .iter()
        .filter_map(|item| {
            if let ExchangeItem::ToolResult { outcome, .. } = item {
                Some(outcome)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(results.len(), 1, "子调用结果不能逐条灌入模型历史");
    assert!(!text(results[0]).contains("SECRET_CHILD"));
    h.close().await;
}
#[tokio::test]
async fn real_child_approval_is_separate_from_outer_approval_and_denial_writes_nothing() {
    let h = Harness::boot().await;
    let run=h.submit("ptc-ask","try {await tools.write({path:'denied.txt',content:'MUST_NOT_WRITE'});} catch(e) {console.log(e.name,e.toolName);return 'denied safely';}").await;
    let principal = lingxi_kernel::Principal::LocalUser;
    eventually(|| {
        !h.approvals
            .pending_for_principal(&principal, "ptc-ask")
            .is_empty()
    })
    .await;
    let outer = h.approvals.pending_for_principal(&principal, "ptc-ask")[0].clone();
    assert!(outer.target.contains("run_tools"));
    assert!(matches!(
        h.approvals.answer_for_principal(
            &principal,
            "ptc-ask",
            &outer.approval_id,
            Answer::Approve
        ),
        AnswerOutcome::Settled(_)
    ));
    eventually(|| {
        !h.approvals
            .pending_for_principal(&principal, "ptc-ask")
            .is_empty()
    })
    .await;
    let child = h.approvals.pending_for_principal(&principal, "ptc-ask")[0].clone();
    assert_ne!(child.approval_id, outer.approval_id);
    assert!(child.target.contains("write"));
    assert!(!h.workspace.join("denied.txt").exists());
    assert!(matches!(
        h.approvals.answer_for_principal(
            &principal,
            "ptc-ask",
            &child.approval_id,
            Answer::Reject {
                reason: "合成验收拒绝".into()
            }
        ),
        AnswerOutcome::Settled(_)
    ));
    h.wait(&run, RunStatus::Completed).await;
    let result = h.provider.results.lock().unwrap()[&run].clone();
    assert!(text(&result).contains("ToolCallError") && text(&result).contains("write"));
    assert_eq!(details(&result)["subcalls"][0]["ok"], false);
    assert!(!h.workspace.join("denied.txt").exists());
    h.close().await;
}
#[tokio::test]
async fn later_program_failure_keeps_real_completed_effect_and_exposes_it_in_receipts() {
    let h = Harness::boot().await;
    let result=h.invoke("ptc-operate","await tools.write({path:'retained.txt',content:'ONE_REAL_EFFECT'});throw new Error('outer failure');").await;
    assert!(matches!(result, ToolOutcome::Failed { .. }), "{result:?}");
    assert!(text(&result).contains("outer failure"));
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("retained.txt")).unwrap(),
        "ONE_REAL_EFFECT"
    );
    assert_eq!(details(&result)["subcalls"][0]["ok"], true);
    h.close().await;
}
#[tokio::test]
async fn unknown_recursive_and_sandbox_failures_are_explicit_and_catchable() {
    let h = Harness::boot().await;
    let caught=h.invoke("ptc-operate","const output=[];for(const name of ['not_a_real_tool','run_tools']){try{await tools[name]({});}catch(e){output.push([e.name,e.toolName]);}}return output;").await;
    assert!(matches!(caught, ToolOutcome::Success { .. }), "{caught:?}");
    assert!(text(&caught).contains("ToolCallError"));
    assert_eq!(details(&caught)["subcalls"].as_array().unwrap().len(), 2);
    for call in details(&caught)["subcalls"].as_array().unwrap() {
        assert_eq!(call["ok"], false);
    }
    let globals = h
        .invoke(
            "ptc-operate",
            "return [typeof require,typeof process,typeof fetch];",
        )
        .await;
    assert_eq!(
        text(&globals),
        "[\n  \"undefined\",\n  \"undefined\",\n  \"undefined\"\n]"
    );
    let bad = h.invoke("ptc-operate", "return process.env;").await;
    assert!(matches!(bad, ToolOutcome::Failed { .. }));
    assert_eq!(details(&bad)["errorCode"], "RUN_TOOLS_EXCEPTION");
    h.close().await;
}
#[tokio::test]
async fn cancelling_actual_program_reaps_its_real_node_process() {
    let h = Harness::boot().await;
    let run = h
        .submit(
            "ptc-operate",
            "await tools.write({path:'started.txt',content:'ready'});while(true){}",
        )
        .await;
    eventually(|| h.workspace.join("started.txt").is_file()).await;
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,args="])
        .output()
        .unwrap();
    let owner = std::process::id().to_string();
    let pids: Vec<i32> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let pid = cols.next()?;
            let ppid = cols.next()?;
            if ppid == owner && line.contains("r08-ptc-worker.mjs") {
                pid.parse().ok()
            } else {
                None
            }
        })
        .collect();
    assert_eq!(pids.len(), 1, "必须捕获实际外围进程");
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
    eventually(|| pids.iter().all(|pid| unsafe { libc::kill(*pid, 0) != 0 })).await;
    h.close().await;
}

#[tokio::test]
async fn real_file_child_receipts_bound_arguments_and_outputs_without_losing_head_tail() {
    let h = Harness::boot().await;
    for i in 0..10 {
        std::fs::write(
            h.workspace.join(format!("large-{i}.txt")),
            format!("REAL_HEAD{}REAL_TAIL", "汉".repeat(8000)),
        )
        .unwrap();
    }
    let result=h.invoke("ptc-operate","await tools.write({path:'large-args.txt',content:'x'.repeat(5000)}); const values=await Promise.all(Array.from({length:10},(_,i)=>tools.read({path:'large-'+i+'.txt'})));return values.length;").await;
    assert!(matches!(result, ToolOutcome::Success { .. }), "{result:?}");
    assert_eq!(text(&result), "10");
    let calls = h.raw_ui_details()["subcalls"].as_array().unwrap().clone();
    assert_eq!(calls.len(), 11);
    assert_eq!(calls[0]["argsOmitted"], true);
    assert!(calls[0].get("args").is_none());
    assert!(calls[0]["argsSummary"].as_str().unwrap().chars().count() <= 121);
    let shown: Vec<_> = calls
        .iter()
        .skip(1)
        .filter_map(|call| call["output"].as_str())
        .collect();
    assert!(!shown.is_empty());
    for output in &shown {
        assert!(output.contains("REAL_HEAD") && output.contains("REAL_TAIL"));
        assert!(!output.contains('\u{fffd}'));
        assert!(output.len() < 8200);
    }
    assert!(calls.iter().any(|call| call["outputOmitted"] == true));
    h.close().await;
}

#[tokio::test]
async fn actual_vm_escape_cannot_read_write_connect_or_delegate_a_host_write() {
    let h = Harness::boot().await;
    let secret = h.root.join("private-synthetic.txt");
    let target = h.root.join("MUST_NOT_WRITE.txt");
    std::fs::write(&secret, "SYNTHETIC_SECRET_MUST_STAY_PRIVATE").unwrap();
    let code=format!("const p=structuredClone.constructor('return process')();const fs=p.getBuiltinModule('fs');const errors=[];try{{fs.writeFileSync({},'MUST_NOT_WRITE');errors.push('WROTE');}}catch(e){{errors.push(e.code);}}try{{fs.readFileSync({});errors.push('READ');}}catch(e){{errors.push(e.code);}}const child=p.getBuiltinModule('child_process').spawnSync('/bin/sh',['-c',{}],{{stdio:[0,2,2]}});errors.push(child.status);return errors;",serde_json::to_string(&target).unwrap(),serde_json::to_string(&secret).unwrap(),serde_json::to_string(&format!("printf child > {}",serde_json::to_string(&target).unwrap())).unwrap());
    let result = h.invoke("ptc-operate", &code).await;
    assert!(matches!(result, ToolOutcome::Success { .. }), "{result:?}");
    let actual: Value = serde_json::from_str(&text(&result)).unwrap();
    for item in [&actual[0], &actual[1]] {
        if cfg!(target_os = "macos") {
            assert_eq!(*item, "EPERM");
        } else {
            assert!(
                matches!(item.as_str(), Some("ENOENT" | "EACCES" | "EPERM")),
                "{actual}"
            );
        }
    }
    assert!(
        actual[2].as_i64().is_some_and(|status| status > 0),
        "必须真实启动子进程并观察拒写退出码：{actual}"
    );
    assert!(!target.exists());
    assert_eq!(
        std::fs::read_to_string(&secret).unwrap(),
        "SYNTHETIC_SECRET_MUST_STAY_PRIVATE"
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let code=format!("const p=structuredClone.constructor('return process')();return await new Promise(resolve=>{{const s=p.getBuiltinModule('net').connect({{host:'127.0.0.1',port:{port}}});s.once('connect',()=>{{s.destroy();resolve('CONNECTED');}});s.once('error',e=>resolve('BLOCKED:'+e.code));}});");
    let result = h.invoke("ptc-operate", &code).await;
    assert!(matches!(result, ToolOutcome::Success { .. }), "{result:?}");
    assert!(text(&result).contains("BLOCKED"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    assert!(
        details(&result)["subcalls"].as_array().unwrap().is_empty(),
        "反向样本不借合法工具产生副作用"
    );
    h.close().await;
}

#[tokio::test]
async fn real_default_ten_minute_timeout_keeps_completed_action_and_reaps_worker() {
    let h = Harness::boot().await;
    let started = std::time::Instant::now();
    let run=h.submit("ptc-operate","await tools.write({path:'before-timeout.txt',content:'ONE_CONFIRMED_ACTION'});console.log('REAL_PREFIX_BEFORE_DEFAULT_TIMEOUT');await new Promise(()=>{});").await;
    eventually(|| h.workspace.join("before-timeout.txt").is_file()).await;
    tokio::time::timeout(Duration::from_secs(625), async {
        loop {
            let row = h
                .state
                .storage()
                .load_run(&RunId::new(&run))
                .await
                .unwrap()
                .unwrap();
            if row.status.is_terminal() {
                assert_eq!(row.status, RunStatus::Completed, "{row:?}");
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("默认10分钟预算后真实Run必须结束");
    assert!(
        started.elapsed() >= Duration::from_secs(600),
        "不得把短预算或假钟当默认超时证据"
    );
    let result = h.provider.results.lock().unwrap()[&run].clone();
    assert!(matches!(result, ToolOutcome::Failed { .. }), "{result:?}");
    assert_eq!(details(&result)["errorCode"], "RUN_TOOLS_TIMEOUT");
    assert!(text(&result).contains("REAL_PREFIX_BEFORE_DEFAULT_TIMEOUT"));
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("before-timeout.txt")).unwrap(),
        "ONE_CONFIRMED_ACTION"
    );
    assert_eq!(h.raw_ui_details()["subcalls"][0]["ok"], true);
    let processes = std::process::Command::new("ps")
        .args(["-axo", "ppid=,args="])
        .output()
        .unwrap();
    let owner = std::process::id().to_string();
    assert!(
        !String::from_utf8(processes.stdout)
            .unwrap()
            .lines()
            .any(
                |line| line.split_whitespace().next() == Some(owner.as_str())
                    && line.contains("r08-ptc-worker.mjs")
            ),
        "预算终态前必须实际回收本测试的外围领头"
    );
    h.close().await;
}
