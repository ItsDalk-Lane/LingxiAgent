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
    approval_service::ApprovalService, coding_ast_lsp, inject::ManualClock, prepare_layout,
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
        let root = std::env::temp_dir().join(format!(
            "lingxi-r08-ast-lsp-{}",
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
                workspace_root: if read_only {
                    None
                } else {
                    Some(workspace.clone())
                },
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let io = if read_only {
            let io = Arc::new(lingxi_service::resourceio::ResourceIoService::new(
                state.session_files().clone(),
                state.resources().clone(),
                root.join("data"),
                Some((access.clone(), workspace.clone())),
                250,
                Arc::new(|| NOW),
            ));
            io.set_file_safety(state.resource_io().file_safety());
            if let Some(history) = state.resource_io().file_history() {
                io.set_file_history(history.clone());
            }
            io
        } else {
            state.resource_io().clone()
        };
        coding_ast_lsp::register(
            &registry,
            &gateway,
            access,
            workspace.clone(),
            io,
            None,
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

#[tokio::test]
async fn r08_ast_structure_pagination_no_match_and_errors_use_real_run() {
    let h = Harness::boot().await;
    let original =
        "function greet(x) { return x + 1; }\nconst fake = 'greet(99)';\ngreet(2);\ngreet(3);\n";
    std::fs::write(h.workspace.join("example.js"), original).unwrap();
    let hostile_config = "deliberately invalid workspace config: [\n";
    std::fs::write(h.workspace.join("sgconfig.yml"), hostile_config).unwrap();
    std::fs::write(h.workspace.join("ignored.js"), "greet(999);").unwrap();
    std::fs::write(h.workspace.join(".gitignore"), "ignored.js\n").unwrap();

    let result = h
        .invoke(
            "ast_grep",
            json!({"pattern":"greet($A)","language":"javascript","files":["example.js"],"limit":1}),
        )
        .await;
    assert_eq!(
        details(&result)["matchCount"],
        2,
        "真实 AST 不得命中字符串：{result:?}"
    );
    assert_eq!(details(&result)["pageReturned"], 1);
    let counts=h.invoke("ast_grep",json!({"pattern":"greet($A)","language":"javascript","globs":["**/*.js"],"output_mode":"count"})).await;
    assert_eq!(
        details(&counts)["matchCount"],
        2,
        "默认扫描不得绕过ignore：{counts:?}"
    );
    let files = h
        .invoke(
            "ast_grep",
            json!({"pattern":"greet($A)","language":"javascript","output_mode":"files"}),
        )
        .await;
    assert_eq!(text(&files), "example.js: 2");
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("sgconfig.yml")).unwrap(),
        hostile_config
    );

    assert!(text(&result).contains("offset=1"));
    let page=h.invoke("ast_grep",json!({"pattern":"greet($A)","language":"javascript","files":["example.js"],"limit":1,"offset":1})).await;
    assert!(text(&page).contains("greet(3)"));
    let empty = h
        .invoke(
            "ast_grep",
            json!({"pattern":"missing($A)","language":"javascript","files":["example.js"]}),
        )
        .await;
    assert_eq!(details(&empty)["matchCount"], 0);
    assert_eq!(text(&empty), "no matches");
    let bad = h
        .invoke(
            "ast_grep",
            json!({"pattern":"greet($A)","language":"invalid-language","files":["example.js"]}),
        )
        .await;
    assert!(matches!(bad, ToolOutcome::Failed { .. }), "{bad:?}");
    let bad_pattern = h
        .invoke(
            "ast_grep",
            json!({"pattern":"foo(); bar();","language":"javascript","files":["example.js"]}),
        )
        .await;
    assert!(
        matches!(bad_pattern, ToolOutcome::Failed { .. })
            && text(&bad_pattern).contains("Cannot parse query"),
        "解析失败不得冒充无匹配：{bad_pattern:?}"
    );

    let escaped = h
        .invoke(
            "ast_grep",
            json!({"pattern":"x","language":"javascript","files":["../outside.js"]}),
        )
        .await;
    assert!(matches!(escaped, ToolOutcome::Failed { .. }), "{escaped:?}");
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("example.js")).unwrap(),
        original
    );
    h.close().await;
}

#[tokio::test]
async fn r08_lsp_real_clangd_positions_preview_rename_and_rejections() {
    let h = Harness::boot().await;
    let original =
        "int old_name = 1;\nint main() { const char* note = u8\"😀\"; return old_name; }\n";
    let reference_line = original.lines().nth(1).unwrap();
    let reference_column = reference_line[..reference_line.find("old_name").unwrap()]
        .encode_utf16()
        .count()
        + 1;
    std::fs::write(h.workspace.join("main.cpp"), original).unwrap();
    let status = h.invoke("lsp", json!({"action":"status"})).await;
    assert!(details(&status)["availability"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["language"] == "cpp" && a["available"] == true));
    let definition = h
        .invoke(
            "lsp",
            json!({"action":"definition","language":"cpp","file":"main.cpp","line":2,"column":reference_column}),
        )
        .await;
    let rendered = details(&definition)["result"].to_string();
    assert!(
        rendered.contains("main.cpp") && rendered.contains("\"line\":0"),
        "{definition:?}"
    );
    let hover = h
        .invoke(
            "lsp",
            json!({"action":"hover","language":"cpp","file":"main.cpp","line":2,"column":reference_column}),
        )
        .await;
    assert!(
        details(&hover)["result"].to_string().contains("old_name"),
        "{hover:?}"
    );
    let symbols = h
        .invoke(
            "lsp",
            json!({"action":"symbols","language":"cpp","file":"main.cpp"}),
        )
        .await;
    assert!(
        details(&symbols)["result"].to_string().contains("main"),
        "{symbols:?}"
    );
    let refs = h
        .invoke(
            "lsp",
            json!({"action":"references","language":"cpp","file":"main.cpp","line":1,"column":5}),
        )
        .await;
    assert!(
        details(&refs)["result"].as_array().unwrap().len() >= 2,
        "{refs:?}"
    );
    let preview=h.invoke("lsp",json!({"action":"rename","language":"cpp","file":"main.cpp","line":1,"column":5,"new_name":"new_name","preview":true})).await;
    assert_eq!(details(&preview)["applied"], false);
    assert!(details(&preview)["preview"]
        .to_string()
        .contains("new_name"));
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("main.cpp")).unwrap(),
        original
    );
    let rename=h.invoke("lsp",json!({"action":"rename","language":"cpp","file":"main.cpp","line":1,"column":5,"new_name":"new_name"})).await;
    assert_eq!(details(&rename)["applied"], true, "{rename:?}");
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("main.cpp")).unwrap(),
        original.replace("old_name", "new_name")
    );
    assert!(details(&rename)["receipts"].as_array().unwrap().len() == 1);
    let rejected=h.invoke("lsp",json!({"action":"rename","language":"cpp","file":"main.cpp","line":1,"column":1,"new_name":"bad"})).await;
    assert!(
        matches!(rejected, ToolOutcome::Failed { .. }),
        "{rejected:?}"
    );
    std::fs::write(
        h.workspace.join("broken.cpp"),
        "int main() { return missing; }\n",
    )
    .unwrap();
    let diagnostics = h
        .invoke(
            "lsp",
            json!({"action":"diagnostics","language":"cpp","file":"broken.cpp"}),
        )
        .await;
    assert!(
        details(&diagnostics)["result"]
            .to_string()
            .contains("missing"),
        "{diagnostics:?}"
    );
    h.close().await;
}

#[tokio::test]
async fn r08_lsp_read_grant_rename_returns_preview_without_writing() {
    let h = Harness::boot_mode(true).await;
    let original = "int old_name=1;\nint main(){return old_name;}\n";
    std::fs::write(h.workspace.join("read.cpp"), original).unwrap();
    let result=h.invoke("lsp",json!({"action":"rename","language":"cpp","file":"read.cpp","line":1,"column":5,"new_name":"new_name"})).await;
    assert_eq!(details(&result)["applied"], false, "{result:?}");
    assert_eq!(details(&result)["errorCode"], "LSP_WRITE_UNAVAILABLE");
    assert!(details(&result)["preview"].to_string().contains("new_name"));
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("read.cpp")).unwrap(),
        original
    );
    h.close().await;
}

// 只替代明确不可用/崩溃的外部语言服务，隔离子进程 PATH，不改测试主进程环境。
#[tokio::test]
async fn r08_lsp_missing_and_crashed_server_are_explicit_errors() {
    if let Ok(mode) = std::env::var("LINGXI_R08_LSP_NEGATIVE") {
        let h = Harness::boot().await;
        std::fs::write(h.workspace.join("main.cpp"), "int value=1;\n").unwrap();
        if mode == "empty" {
            let result=h.invoke("lsp",json!({"action":"rename","language":"cpp","file":"main.cpp","line":1,"column":5,"new_name":"new_name"})).await;
            assert_eq!(details(&result)["applied"], false);
            assert_eq!(details(&result)["changedFiles"], json!([]));
            assert_eq!(
                std::fs::read_to_string(h.workspace.join("main.cpp")).unwrap(),
                "int value=1;\n"
            );
            h.close().await;
            return;
        }
        if mode == "cancel" {
            let run=h.submit("kernel-a",json!({"tool":"tool:first-party:lsp","args":{"action":"hover","language":"cpp","file":"main.cpp","line":1,"column":5}})).await;
            let witness = PathBuf::from(std::env::var_os("LINGXI_R08_LSP_WITNESS").unwrap());
            tokio::time::timeout(Duration::from_secs(10), async {
                while !witness.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("语言服务进程树未启动");
            let pids: Vec<i32> = serde_json::from_slice(&std::fs::read(witness).unwrap()).unwrap();
            h.state.runs().cancel_run(&run, "真实进程树取消验证");
            h.wait(&run, RunStatus::Cancelled).await;
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if pids.iter().all(|pid| unsafe { libc::kill(*pid, 0) } != 0) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("语言服务或孙进程在取消后残留");
            h.close().await;
            return;
        }
        let result = h
            .invoke(
                "lsp",
                json!({"action":"hover","language":"cpp","file":"main.cpp","line":1,"column":5}),
            )
            .await;
        assert!(matches!(result, ToolOutcome::Failed { .. }), "{result:?}");
        assert!(
            text(&result).contains(if mode == "missing" {
                "LSP_SERVER_MISSING"
            } else {
                "exited"
            }),
            "{result:?}"
        );
        assert_eq!(
            std::fs::read_to_string(h.workspace.join("main.cpp")).unwrap(),
            "int value=1;\n"
        );
        h.close().await;
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "lingxi-r08-lsp-negative-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("node"))
        .find(|p| p.is_file())
        .unwrap();
    std::os::unix::fs::symlink(node, root.join("node")).unwrap();
    for mode in ["missing", "crash", "empty", "cancel"] {
        if mode != "missing" {
            std::fs::write(
                root.join("clangd"),
                if mode=="crash" {"#!/usr/bin/env node\nsetTimeout(()=>process.exit(77),100);\n".to_string()} else if mode=="empty" {r#"#!/usr/bin/env node
let b=Buffer.alloc(0);process.stdin.on('data',chunk=>{b=Buffer.concat([b,chunk]);for(;;){const h=b.indexOf('\r\n\r\n');if(h<0)return;const m=/Content-Length: (\d+)/i.exec(b.subarray(0,h).toString());if(!m)process.exit(2);const n=Number(m[1]);if(b.length<h+4+n)return;const q=JSON.parse(b.subarray(h+4,h+4+n));b=b.subarray(h+4+n);if(q.method==='exit')process.exit(0);if(q.id===undefined)continue;const result=q.method==='initialize'?{capabilities:{}}:q.method==='textDocument/prepareRename'?{start:{line:0,character:4},end:{line:0,character:9}}:q.method==='textDocument/rename'?{changes:{}}:null;const out=JSON.stringify({jsonrpc:'2.0',id:q.id,result});process.stdout.write('Content-Length: '+Buffer.byteLength(out)+'\r\n\r\n'+out);}});
"#.to_string()} else {format!("#!/usr/bin/env node\nconst cp=require('node:child_process'),fs=require('node:fs');const child=cp.spawn(process.execPath,['-e','setInterval(()=>{{}},1000)'],{{stdio:'ignore'}});fs.writeFileSync({},JSON.stringify([process.pid,child.pid]));setInterval(()=>{{}},1000);\n",serde_json::to_string(&root.join("witness.json").to_string_lossy()).unwrap())},
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.join("clangd"), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "r08_lsp_missing_and_crashed_server_are_explicit_errors",
                "--nocapture",
            ])
            .env("LINGXI_R08_LSP_NEGATIVE", mode)
            .env("LINGXI_R08_LSP_WITNESS", root.join("witness.json"))
            .env("PATH", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
