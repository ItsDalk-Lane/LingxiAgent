//! 真实Run驱动文件工具、来源、覆盖策略和远端只读暂存。

use base64::Engine as _;
use lingxi_kernel::{
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolOutcome, ToolRequest,
        TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::io::{Cursor, Write};
use std::{collections::HashMap, sync::Mutex};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[derive(Default)]
struct Provider {
    results: Mutex<HashMap<String, Value>>,
    calls: std::sync::atomic::AtomicUsize,
    prompts: std::sync::Mutex<Vec<Option<String>>>,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "experiments.fixture".into(),
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
        self.prompts
            .lock()
            .unwrap()
            .push(input.system_prompt.clone());
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if input.turn == 1 {
                let command: Value = serde_json::from_str(&input.submission).unwrap();
                let request = ToolRequest::from_effective_arguments(
                    format!("tool:first-party:{}", command["tool"].as_str().unwrap()),
                    serde_json::from_slice(
                        &base64::engine::general_purpose::STANDARD
                            .decode(command["argsBase64"].as_str().unwrap())
                            .unwrap(),
                    )
                    .unwrap(),
                    &SchemaBudget::default(),
                )
                .unwrap();
                ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![request],
                        content: vec![],
                    },
                )
            } else {
                let value = input
                    .prior
                    .iter()
                    .rev()
                    .find_map(|item| {
                        if let ExchangeItem::ToolResult { outcome, .. } = item {
                            Some(match outcome {
                                ToolOutcome::Success { result } => {
                                    json!({"ok":true,"details":result.details})
                                }
                                ToolOutcome::Failed { error } => {
                                    json!({"ok":false,"error":error.message})
                                }
                                other => json!({"ok":false,"outcome":format!("{other:?}")}),
                            })
                        } else {
                            None
                        }
                    })
                    .expect("必须收到实际工具结果");
                self.results
                    .lock()
                    .unwrap()
                    .insert(format!("{}|{}", ctx.session_id, input.submission), value);
                ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::Final {
                        message: NormalizedMessage {
                            role: "assistant".into(),
                            content: vec![ContentBlock::Text {
                                text: "处理结束".into(),
                            }],
                            model_call_id: Some(call.clone()),
                        },
                    },
                )
            }
        })
    }
}
struct Harness {
    provider: Arc<Provider>,
    home: PathBuf,
    state: ServiceState,
    url: String,
    token: String,
    readonly: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Harness {
    async fn boot() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-resource-tools-completion-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        for id in ["agentA", "agentB"] {
            std::fs::create_dir_all(home.join("agents").join(id)).unwrap();
            std::fs::write(home.join("agents").join(id).join("config.yaml"),"agent:\n  name: 测试助手\nskills:\n  enabled: []\nmemory:\n  enabled: true\n  experience_enabled: true\n").unwrap();
        }
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            r#"{"learn_skills":{"enabled":true,"safety_review":true}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(workspace.join("yuan/en")).unwrap();
        std::fs::write(
            workspace.join("yuan/en/lingxi.md"),
            "You are a test assistant.",
        )
        .unwrap();
        std::fs::write(workspace.join("yuan/lingxi.md"), "你是测试助手。").unwrap();
        let layout = prepare_layout(&home).unwrap();
        let compiler = Arc::new(
            lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
                lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                    &home,
                    workspace.clone(),
                    lingxi_service::context_compiler::HostFacts {
                        platform: "darwin".into(),
                        os_type: "Darwin".into(),
                        os_release: "test".into(),
                        shell_label: "zsh".into(),
                        workspace: workspace.to_string_lossy().into(),
                    },
                ),
            )),
        );
        let clock = Arc::new(lingxi_service::inject::ManualClock::new(1_800_000_000_000));
        let approval = Arc::new(lingxi_service::approval_service::ApprovalService::new(
            clock.clone(),
        ));
        let gateway = Arc::new(lingxi_service::toolgateway::ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            approval.clone(),
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
        lingxi_service::subagents::register_tools(
            gateway.registry(),
            gateway.as_ref(),
            &SchemaBudget::default(),
        );
        let provider = Arc::new(Provider::default());
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
                workspace_root: Some(workspace.clone()),
                resource_url_provider:Some(Arc::new(lingxi_service::resource_url::UrlProvider::new(&home))),
                context_compiler: Some(compiler),
                clock,
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway),
                approval_gate: Some(approval),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let readonly = state
            .auth()
            .issue_device_credential(
                lingxi_service::LOCAL_OWNER_USER_ID,
                &["settings.read"],
                None,
            )
            .unwrap()
            .secret;
        let run_state = state.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (ready, received) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            run(
                run_state,
                async {
                    let _ = rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        let addr = received.await.unwrap();
        Self {
            provider,
            home,
            state,
            url: format!("http://{addr}"),
            token,
            readonly,
            stop: Some(tx),
            task,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value, readonly: bool) -> (u16, Value) {
        let payload = body.to_string();
        let token = if readonly {
            &self.readonly
        } else {
            &self.token
        };
        let wire=format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
        let mut stream = tokio::net::TcpStream::connect(self.url.strip_prefix("http://").unwrap())
            .await
            .unwrap();
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let split = bytes.windows(4).position(|x| x == b"\r\n\r\n").unwrap();
        let header = String::from_utf8_lossy(&bytes[..split]);
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let value = serde_json::from_slice(&bytes[split + 4..]).unwrap();
        (status, value)
    }
    async fn close(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        println!(
            "RESOURCE_TOOL_PROOF {}",
            self.home.parent().unwrap().display()
        );
    }
}

impl Harness {
    async fn create(&self, id: &str, mode: &str) {
        let (status, value) = self
            .request(
                "POST",
                "/lingxi/v1/sessions",
                json!({"sessionId":id,"agentId":"agentA","title":"资源工具验证","permissionMode":mode}),
                false,
            )
            .await;
        assert_eq!(status, 201, "{value}");
    }
    async fn tool(&self, sid: &str, tool: &str, args: Value) -> Value {
        let command = json!({"tool":tool,"argsBase64":base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&args).unwrap())}).to_string();
        let (status, value) = self
            .request(
                "POST",
                &format!("/lingxi/v1/sessions/{sid}/execute"),
                json!({"input":command}),
                false,
            )
            .await;
        assert_eq!(status, 200, "{value}");
        self.provider
            .results
            .lock()
            .unwrap()
            .get(&format!("{sid}|{command}"))
            .cloned()
            .expect("下一轮实际模型必须看到工具结果")
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn current_file_copy_extract_stat_and_real_permission_failures_preserve_targets() {
    let h = Harness::boot().await;
    h.create("own", "operate").await;
    h.create("readonly", "read_only").await;
    h.create("other", "operate").await;
    let cwd = h.home.parent().unwrap().join("workspace");
    let source = cwd.join("source.txt");
    let dest = cwd.join("copy.txt");
    let bytes = "来源中文正文\n第二行".as_bytes();
    std::fs::write(&source, bytes).unwrap();
    let stat = h
        .tool("own", "file", json!({"action":"stat","path":"source.txt"}))
        .await;
    assert_eq!(stat["ok"], true, "{stat}");
    assert_eq!(stat["details"]["exists"], true);
    let copied = h
        .tool(
            "own",
            "file",
            json!({"action":"copy","path":"source.txt","targetPath":"copy.txt"}),
        )
        .await;
    assert_eq!(copied["ok"], true, "{copied}");
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert_eq!(copied["details"]["sessionFile"]["origin"], "file_copy");
    let file_id = copied["details"]["sessionFile"]["fileId"].as_str().unwrap();
    assert!(h
        .state
        .session_files()
        .get_file("own", file_id)
        .await
        .unwrap()
        .is_some());
    assert!(h
        .state
        .session_files()
        .get_file("other", file_id)
        .await
        .unwrap()
        .is_none());
    let extract = h
        .tool("own", "file", json!({"action":"extract","fileId":file_id}))
        .await;
    assert_eq!(extract["ok"], true, "{extract}");
    assert_eq!(
        extract["details"]["text"],
        String::from_utf8_lossy(bytes).as_ref()
    );
    assert_eq!(extract["details"]["source"]["filePath"], json!(dest));
    let local = h
        .tool("own", "materialize", json!({"path":"source.txt"}))
        .await;
    assert_eq!(local["ok"], true, "{local}");
    assert_eq!(local["details"]["filePath"], json!(source));
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    std::fs::write(&source, "新的来源").unwrap();
    for args in [
        json!({"action":"copy","path":"source.txt","targetPath":"copy.txt"}),
        json!({"action":"copy","path":"missing.txt","targetPath":"copy.txt"}),
        json!({"action":"copy","path":"source.txt","targetPath":"../outside.txt"}),
        json!({"action":"copy","path":"source.txt","targetPath":"copy.txt","conflictPolicy":"invalid"}),
    ] {
        let result = h.tool("own", "file", args).await;
        assert_eq!(result["ok"], false, "{result}");
        assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    }
    let renamed=h.tool("own","file",json!({"action":"copy","path":"source.txt","targetPath":"copy.txt","conflictPolicy":"rename"})).await;
    assert_eq!(renamed["ok"], true, "{renamed}");
    assert_eq!(
        std::fs::read(cwd.join("copy (1).txt")).unwrap(),
        "新的来源".as_bytes()
    );
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    let over=h.tool("own","file",json!({"action":"copy","path":"source.txt","targetPath":"copy.txt","conflictPolicy":"overwrite"})).await;
    assert_eq!(over["ok"], true, "{over}");
    assert_eq!(std::fs::read(&dest).unwrap(), "新的来源".as_bytes());
    for (sid, tool, args) in [
        (
            "readonly",
            "file",
            json!({"action":"copy","path":"source.txt","targetPath":"forbidden.txt"}),
        ),
        ("own", "file", json!({"action":"extract","path":"."})),
        ("own", "materialize", json!({})),
        ("own", "materialize", json!({"path":"../outside.txt"})),
        (
            "own",
            "materialize",
            json!({"fileId":file_id,"sessionId":"other"}),
        ),
    ] {
        let result = h.tool(sid, tool, args).await;
        assert_eq!(result["ok"], false, "{result}");
    }
    let foreign_command=json!({"tool":"file","argsBase64":base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&json!({"action":"extract","fileId":file_id})).unwrap())}).to_string();
    let (status, error) = h
        .request(
            "POST",
            "/lingxi/v1/sessions/other/execute",
            json!({"input":foreign_command}),
            false,
        )
        .await;
    assert_eq!(
        status, 500,
        "当前存储在模型工具记录阶段先拒绝外会话身份：{error}"
    );
    assert!(
        error
            .to_string()
            .contains("session_file_reference_not_owned"),
        "{error}"
    );
    assert!(!cwd.join("forbidden.txt").exists());
    assert!(!h.home.parent().unwrap().join("outside.txt").exists());
    let ro = h
        .tool(
            "readonly",
            "file",
            json!({"action":"extract","path":"source.txt"}),
        )
        .await;
    assert_eq!(ro["ok"], true, "{ro}");
    let private = h
        .tool(
            "own",
            "materialize",
            json!({"resource":{"kind":"url","url":"http://127.0.0.1:9/private"}}),
        )
        .await;
    assert_eq!(private["ok"], false, "{private}");
    assert!(
        private.to_string().contains("blocked_private_url"),
        "{private}"
    );
    h.close().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn current_public_url_materialization_uses_real_bytes_and_no_cross_session_grant() {
    let h = Harness::boot().await;
    h.create("remote", "read_only").await;
    h.create("other", "read_only").await;
    let result = h
        .tool(
            "remote",
            "materialize",
            json!({"resource":{"kind":"url","url":"https://1.1.1.1/cdn-cgi/trace"}}),
        )
        .await;
    assert_eq!(result["ok"], true, "{result}");
    let path = PathBuf::from(result["details"]["filePath"].as_str().unwrap());
    assert!(path.is_absolute() && path.exists());
    assert_eq!(
        lingxi_kernel::session_tree::sha256_hex(&std::fs::read(&path).unwrap()),
        result["details"]["version"]["sha256"]
    );
    assert_eq!(result["details"]["isDirectory"], false);
    let own = h
        .tool("remote", "file", json!({"action":"extract","path":path}))
        .await;
    assert_eq!(own["ok"], true, "{own}");
    assert!(own["details"]["text"]
        .as_str()
        .unwrap()
        .contains("h=1.1.1.1"));
    let foreign = h
        .tool("other", "file", json!({"action":"extract","path":path}))
        .await;
    assert_eq!(foreign["ok"], false, "{foreign}");
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, "外部替换").unwrap();
    let replaced = h
        .tool("remote", "file", json!({"action":"extract","path":path}))
        .await;
    assert_eq!(replaced["ok"], false, "{replaced}");
    assert!(!original.is_empty());
    h.close().await;
}

fn package(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in parts {
        zip.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn word(text: &str) -> Vec<u8> {
    package(&[(
        "word/document.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>表格原文</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#
        ),
    )])
}
fn workbook() -> Vec<u8> {
    package(&[
        (
            "xl/workbook.xml",
            r#"<workbook xmlns:r="rel"><sheets><sheet name="First" sheetId="1" r:id="chosen"/><sheet name="Second" sheetId="2" r:id="other"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="other" Type="urn:/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="chosen" Type="urn:/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#,
        ),
        (
            "xl/sharedStrings.xml",
            r#"<sst><si><t>第一张 &lt;script&gt;alert(1)&lt;/script&gt; &amp; 原文</t></si><si><r><t>富文本</t></r><r><t>尾部</t></r></si></sst>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>不许返回第二张</t></is></c></row></sheetData></worksheet>"#,
        ),
        (
            "xl/worksheets/sheet2.xml",
            r#"<worksheet><sheetData><row r="2"><c r="A2" t="s"><v>0</v></c><c r="C2"><v>42</v></c></row><row r="4"><c r="B4" t="s"><v>1</v></c><c r="C4" t="b"><v>1</v></c></row></sheetData></worksheet>"#,
        ),
    ])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn current_file_extract_office_all_available_sheets_source_and_explicit_truncation() {
    let h = Harness::boot().await;
    h.create("office", "read_only").await;
    h.create("writer", "operate").await;
    let cwd = h.home.parent().unwrap().join("workspace");
    let docx = word("真正Word源内容");
    let xlsx = workbook();
    for (name, bytes, needles) in [
        ("source.docx", docx, vec!["真正Word源内容", "表格原文"]),
        ("source.xlsx", xlsx, vec!["第一张", "不许返回第二张"]),
    ] {
        let path = cwd.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let result = h
            .tool("office", "file", json!({"action":"extract","path":name}))
            .await;
        assert_eq!(result["ok"], true, "{result}");
        let text = result["details"]["text"].as_str().unwrap();
        for needle in needles {
            assert!(text.contains(needle), "{result}");
        }
        assert_eq!(result["details"]["source"]["filePath"], json!(path));
        assert_eq!(
            result["details"]["source"]["version"]["sha256"],
            lingxi_kernel::session_tree::sha256_hex(&bytes)
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    std::fs::write(cwd.join("large.txt"), "字".repeat(200_005)).unwrap();
    let large = h
        .tool(
            "office",
            "file",
            json!({"action":"extract","path":"large.txt"}),
        )
        .await;
    assert_eq!(large["ok"], true, "{large}");
    assert_eq!(large["details"]["truncated"], true);
    assert_eq!(large["details"]["totalChars"], 200005);
    assert!(large["details"]["text"]
        .as_str()
        .unwrap()
        .contains("5 characters omitted"));
    std::fs::write(cwd.join("bad.docx"), b"not an Office archive").unwrap();
    let bad = h
        .tool(
            "office",
            "file",
            json!({"action":"extract","path":"bad.docx"}),
        )
        .await;
    assert_eq!(bad["ok"], false, "{bad}");
    assert_eq!(
        std::fs::read(cwd.join("bad.docx")).unwrap(),
        b"not an Office archive"
    );
    let nested=h.tool("writer","file",json!({"action":"copy","path":"source.docx","targetDir":"outputs/nested","filename":"copy.docx"})).await;
    assert_eq!(nested["ok"], true, "{nested}");
    assert_eq!(
        std::fs::read(cwd.join("outputs/nested/copy.docx")).unwrap(),
        std::fs::read(cwd.join("source.docx")).unwrap()
    );
    let inspected=h.state.session_files().inspect_local_import(cwd.join("source.docx").to_str().unwrap()).unwrap();
    let owned=h.state.session_files().register_local_snapshot("office",&inspected,None,"user_attachment").await.unwrap();
    let resolved = h
        .tool("office", "materialize", json!({"fileId":owned.file_id}))
        .await;
    assert_eq!(resolved["ok"], true, "{resolved}");
    assert_eq!(resolved["details"]["filePath"], owned.real_path);
    let copied = h
        .tool(
            "writer",
            "file",
            json!({"action":"copy","path":"missing.txt","targetDir":"never-created/nested"}),
        )
        .await;
    assert_eq!(copied["ok"], false, "{copied}");
    assert!(!cwd.join("never-created").exists());
    h.close().await;
}
