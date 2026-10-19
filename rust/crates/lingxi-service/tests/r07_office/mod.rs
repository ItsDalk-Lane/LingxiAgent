//! 只替模型与外部 PDF 响应；文档 worker、Run、权限、资源、文件和 HTTP 均使用正式入口。
use lingxi_kernel::{
    browser::{
        BrowserError, BrowserFuture, BrowserOutput, BrowserPort, BrowserScope, PdfRenderJob,
        PdfRendered, PdfScope,
    },
    model_exchange::{ExchangeItem, ModelTurnInput},
    ports::{
        ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome,
        ToolRequest, TurnDeltaSink, TurnProviderPort,
    },
    toolcatalog::{SchemaBudget, ToolRegistry},
    RunContext,
};
use lingxi_protocol::{
    BrowserCommand, BrowserSettings, BrowserWorkspace, ContentBlock, ModelCallId,
    NormalizedMessage, RunId,
};
use lingxi_service::{
    approval_service::{Answer, ApprovalService},
    inject::ManualClock,
    office_tools::{OfficeAssets, OfficeWorkerLaunch},
    prepare_layout,
    toolgateway::ToolInvocationGateway,
    ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const NOW: u64 = 1_800_000_000_000;
const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jD1EAAAAASUVORK5CYII=";
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}
fn png() -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(PNG)
        .unwrap()
}
#[derive(Default)]
struct Pdf {
    jobs: Mutex<Vec<(PdfScope, PdfRenderJob)>>,
    wait_for_cancel: AtomicBool,
    cleanup: AtomicBool,
}
impl BrowserPort for Pdf {
    fn execute<'a>(
        &'a self,
        _: BrowserScope,
        _: BrowserCommand,
        _: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, BrowserOutput> {
        Box::pin(async { unreachable!("办公工具不能借用聊天浏览器命令") })
    }
    fn workspaces(&self) -> Result<Vec<BrowserWorkspace>, BrowserError> {
        Ok(vec![])
    }
    fn restore(&self, _: BrowserScope, _: BrowserWorkspace) -> Result<(), BrowserError> {
        Ok(())
    }
    fn revoke<'a>(&'a self, _: BrowserScope) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn configure<'a>(&'a self, _: BrowserSettings) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn clear_site_data<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn read_auxiliary_page<'a>(
        &'a self,
        _: String,
        _: String,
        _: Option<String>,
        _: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, Value> {
        Box::pin(async { unreachable!("文档渲染不能借用搜索权限") })
    }
    fn reap_idle<'a>(&'a self, _: u64) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn shutdown<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn render_pdf<'a>(
        &'a self,
        scope: PdfScope,
        job: PdfRenderJob,
        cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, PdfRendered> {
        Box::pin(async move {
            assert_eq!(scope.owner_subject, lingxi_kernel::LOCAL_OWNER_SUBJECT);
            assert_eq!(scope.session_id, "office-a");
            self.jobs.lock().unwrap().push((scope, job));
            if self.wait_for_cancel.load(Ordering::Acquire) {
                while !cancel.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                self.cleanup.store(true, Ordering::Release);
                return Err(BrowserError {
                    code: "PDF_CANCELLED".into(),
                    message: "已确认关闭隔离页".into(),
                    effect_unknown: false,
                });
            }
            let bytes =
                std::fs::read(repo().join("tests/fixtures/document-extract/sample-text.pdf"))
                    .unwrap();
            Ok(PdfRendered {
                sha256: lingxi_protocol::canon::sha256_hex(&bytes),
                bytes,
                renderer: "external-fixture-chromium".into(),
            })
        })
    }
}
#[derive(Default)]
struct Model {
    outcomes: Mutex<BTreeMap<String, ToolOutcome>>,
}
impl TurnProviderPort for Model {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "fixture".into(),
            model: "office".into(),
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
            // 正式工具回合带明确目标；同一生产 provider 的标题等辅助调用不造工具。
            let body = input.submission.lines().find_map(|line| {
                serde_json::from_str::<Value>(line)
                    .ok()
                    .filter(|v| v["target"].as_str().is_some() && v["args"].is_object())
            });
            if let Some(body) = body.filter(|_| input.turn == 1) {
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            body["target"].as_str().unwrap(),
                            body["args"].clone(),
                            &SchemaBudget::default(),
                        )
                        .unwrap()],
                        content: vec![],
                    },
                );
            }
            if let Some(outcome) = input.prior.iter().rev().find_map(|item| match item {
                ExchangeItem::ToolResult { outcome, .. } => Some(outcome.clone()),
                _ => None,
            }) {
                self.outcomes
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
                            text: "办公本地验证完成".into(),
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
    model: Arc<Model>,
    pdf: Arc<Pdf>,
    approvals: Arc<ApprovalService>,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
/// 媒体/文本供应商的确定性回环替身：按路径路由脚本化响应，
/// 记录命中路径，可在首次图片命中时执行真实副作用（如改写目标文档）
/// 以验证冻结写。
struct MediaStub {
    endpoint: String,
    hits: Arc<Mutex<Vec<String>>>,
    hold_images: Arc<AtomicBool>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}
impl MediaStub {
    async fn start(image_b64: String, on_first_image: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("媒体替身绑定");
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(Mutex::new(Vec::new()));
        let serve_hits = hits.clone();
        let hold_images = Arc::new(AtomicBool::new(false));
        let serve_hold = hold_images.clone();
        let (stop, mut rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let accept = tokio::select! {
                    _ = &mut rx => break,
                    _ = connections.join_next(), if !connections.is_empty() => continue,
                    accept = listener.accept() => accept,
                };
                let Ok((mut socket, _)) = accept else { break };
                let image_b64 = image_b64.clone();
                let on_first_image = on_first_image.clone();
                let serve_hits = serve_hits.clone();
                let serve_hold = serve_hold.clone();
                connections.spawn(async move {
                    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    let header_end = loop {
                        let read = socket.read(&mut chunk).await.expect("媒体替身读取");
                        assert!(read > 0, "媒体替身连接提前关闭");
                        buf.extend_from_slice(&chunk[..read]);
                        if let Some(at) = find_headers_end(&buf) {
                            break at;
                        }
                        assert!(buf.len() < 256 * 1024, "媒体替身头块过大");
                    };
                    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let path = head
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_string();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    while buf.len() < header_end + 4 + length {
                        let read = socket.read(&mut chunk).await.expect("媒体替身读取正文");
                        assert!(read > 0, "媒体替身正文提前关闭");
                        buf.extend_from_slice(&chunk[..read]);
                    }
                    let is_first_image = path.contains("/images/generations") && {
                        let mut recorded = serve_hits.lock().unwrap();
                        recorded.push(path.clone());
                        recorded
                            .iter()
                            .filter(|p| p.contains("/images/generations"))
                            .count()
                            == 1
                    };
                    if is_first_image {
                        if let Some(action) = on_first_image {
                            action();
                        }
                    }
                    if path.contains("/images/generations") {
                        while serve_hold.load(Ordering::Acquire) {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                    if !path.contains("/images/generations") {
                        serve_hits.lock().unwrap().push(path.clone());
                    }
                    let (content_type, body) = if path.contains("/images/generations") {
                        (
                            "application/json",
                            format!("{{\"data\":[{{\"b64_json\":\"{image_b64}\"}}]}}"),
                        )
                    } else if path.contains("/chat/completions") {
                        let mut sse = String::new();
                        for frame in [
                            json!({"id":"chatcmpl-cover","model":"dall-e-3","choices":[{"index":0,"delta":{"role":"assistant","content":"a calm editorial cover, soft light"},"finish_reason":null}]}),
                            json!({"id":"chatcmpl-cover","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                            json!({"id":"chatcmpl-cover","choices":[],"usage":{"prompt_tokens":17,"completion_tokens":5}}),
                        ] {
                            sse.push_str(&format!("data: {frame}\n\n"));
                        }
                        sse.push_str("data: [DONE]\n\n");
                        ("text/event-stream", sse)
                    } else {
                        ("application/json", "{}".into())
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    // 取消会主动关闭上游连接；请求计数已经记录，关闭不是替身失败。
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        Self {
            endpoint,
            hits,
            hold_images,
            stop: Some(stop),
            task,
        }
    }
    fn image_hits(&self) -> usize {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.contains("/images/generations"))
            .count()
    }
    async fn close(mut self) {
        self.hold_images.store(false, Ordering::Release);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}
fn find_headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}
impl Harness {
    async fn boot(mode: &str) -> Self {
        Self::boot_full(mode, None).await
    }
    /// 带真实媒体面配置的组合根：图片供应商指向回环替身，
    /// 其余依赖与 boot 完全一致。
    async fn boot_media(mode: &str, media_endpoint: &str) -> Self {
        Self::boot_full(mode, Some(media_endpoint)).await
    }
    async fn boot_full(mode: &str, media_endpoint: Option<&str>) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r07-office-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        for agent in ["a", "b"] {
            std::fs::create_dir_all(home.join(format!("agents/{agent}"))).unwrap();
            std::fs::write(
                home.join(format!("agents/{agent}/config.yaml")),
                format!(
                    "agent:\n  name: {agent}\nmemory:\n  enabled: false\n{}",
                    if agent == "b" {
                        "tools:\n  disabled: [office, beautify]\n"
                    } else {
                        ""
                    }
                ),
            )
            .unwrap();
        }
        std::fs::write(
            home.join("user/preferences.json"),
            json!({"locale":"en","primary_agent_id":"b","current_agent_id":"b"}).to_string(),
        )
        .unwrap();
        std::fs::write(
            workspace.join("article.md"),
            "---\ntitle: Kept\nfutureField:\n  keep: 7\n---\n# Body stays\n",
        )
        .unwrap();
        std::fs::write(workspace.join("image.png"), png()).unwrap();
        let root = root.canonicalize().unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        // 媒体模式必须让组合根接真实模型链（turn_provider 缺席才建 OperationService），
        // 聊天与图片供应商都指向同一个回环替身。
        let media_plane = media_endpoint.map(|endpoint| {
            let plane_json = format!(
                r#"{{"providers":{{"text_a":{{"protocol":"openai-completions","endpoint":"{endpoint}/v1","auth":{{"kind":"apiKey","apiKey":"sk-TEXT-A-SECRET"}}}},"image_b":{{"protocol":"volcengine-images","endpoint":"{endpoint}","auth":{{"kind":"apiKey","apiKey":"sk-IMAGE-B-SECRET"}}}}}},"models":{{"chat":{{"provider":"text_a","model":"text-model","capabilities":{{"tools":true}}}},"image":{{"provider":"image_b","model":"fixture-image"}}}}}}"#
            );
            let plane =
                lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&plane_json)
                    .expect("媒体面配置必须解析");
            let credentials = Arc::new(
                lingxi_service::credentials::CredentialService::bootstrap(
                    &plane,
                    &layout.runtime_dir,
                    Arc::new(lingxi_service::inject::SystemClock),
                )
                .expect("媒体凭证服务"),
            );
            let gateway = Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            );
            (gateway, credentials)
        });
        let model = Arc::new(Model::default());
        let pdf = Arc::new(Pdf::default());
        let clock = Arc::new(ManualClock::new(NOW));
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            approvals.clone(),
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
        let node = std::process::Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .expect("本地办公测试需要项目规定的 Node 24");
        assert!(node.status.success());
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
                workspace_root: Some(workspace.clone()),
                turn_provider: if media_plane.is_some() {
                    None
                } else {
                    Some(model.clone())
                },
                tool_gateway: Some(gateway),
                approval_gate: Some(approvals.clone()),
                browser_port: Some(pdf.clone()),
                office_assets: Some(OfficeAssets {
                    themes_dir: repo().join("desktop/src/themes"),
                    cover_gallery_dir: repo().join("desktop/src/assets/cover-gallery"),
                }),
                office_document_worker: Some(OfficeWorkerLaunch {
                    argv: vec![
                        String::from_utf8(node.stdout).unwrap().trim().into(),
                        "--max-old-space-size=384".into(),
                        repo()
                            .join("scripts/office-document-worker.ts")
                            .to_string_lossy()
                            .into(),
                    ],
                    cwd: repo(),
                }),
                model_gateway: media_plane.as_ref().map(|(gateway, _)| gateway.clone()),
                credential_service: media_plane.map(|(_, credentials)| credentials),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        state
            .session_admin()
            .create_session(
                "office-a",
                "a",
                lingxi_kernel::LOCAL_OWNER_SUBJECT,
                "办公来源",
                Some(mode),
                &[],
                true,
                NOW,
            )
            .await
            .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, address) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let task = tokio::spawn(async move {
            lingxi_service::run(
                serving,
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
        let base = format!("http://{}", address.await.unwrap());
        Self {
            root,
            workspace,
            state,
            model,
            pdf,
            approvals,
            base,
            stop,
            task,
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
    async fn submit(&self, target: &str, args: Value) -> String {
        self.state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &self.principal(),
                "office-a",
                &ExecuteSubmission::plain(&json!({"target":target,"args":args}).to_string()),
                NOW,
            )
            .await
            .unwrap()
            .run_id
    }
    async fn wait(&self, run: &str) -> ToolOutcome {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let row = self
                    .state
                    .storage()
                    .load_run(&RunId::new(run))
                    .await
                    .unwrap()
                    .unwrap();
                if row.status.is_terminal() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("真实办公 Run 在15秒内未完成");
        self.model
            .outcomes
            .lock()
            .unwrap()
            .remove(run)
            .expect("模型必须收到真实工具结果")
    }
    async fn pending(&self, run: &str) -> String {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(pending) = self
                    .approvals
                    .pending_for_principal(&lingxi_kernel::Principal::LocalUser, "office-a")
                    .into_iter()
                    .find(|p| p.run_id == run)
                {
                    return pending.approval_id;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("写文档必须进入正式确认")
    }
    fn approve(&self, id: &str) {
        assert!(self
            .approvals
            .answer_for_principal(
                &lingxi_kernel::Principal::LocalUser,
                "office-a",
                id,
                Answer::Approve
            )
            .settled());
    }
    async fn http(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.state.auth().local_token());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw":text})),
        )
    }
    async fn close(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(self.root).unwrap();
    }
}
fn success(outcome: ToolOutcome) -> Value {
    match outcome {
        ToolOutcome::Success { result } => {
            Value::Object(result.details.expect("办公工具必须返回真实结构"))
        }
        other => panic!("预期成功，实际{other:?}"),
    }
}

#[tokio::test]
async fn real_document_worker_reads_docx_xlsx_and_paged_pdf_without_changing_inputs() {
    let h = Harness::boot("read_only").await;
    for (name, format, kind, expected) in [
        ("sample.docx", "html", "docx", "Quarterly Notes"),
        ("sample.xlsx", "json", "xlsx", "North"),
        ("sample-text.pdf", "markdown", "pdf", "Hello from PDF"),
    ] {
        let bytes =
            std::fs::read(repo().join("tests/fixtures/document-extract").join(name)).unwrap();
        let path = h.workspace.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let run = h
            .submit(
                "tool:plugin:office:read-document",
                json!({"filePath":path,"outputFormat":format,"maxPages":1}),
            )
            .await;
        let result = success(h.wait(&run).await);
        assert_eq!(result["office"]["kind"], kind);
        assert!(result.to_string().contains(expected));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(h.state.storage().query_one_text("SELECT journal_id FROM invocation_journal WHERE run_id=?1 AND phase='succeeded' AND dispatched=1",vec![run]).await.unwrap().is_some());
    }
    let scan = h.workspace.join("scan.pdf");
    std::fs::copy(
        repo().join("tests/fixtures/document-extract/sample-scanned.pdf"),
        &scan,
    )
    .unwrap();
    let run = h
        .submit("tool:plugin:office:read-document", json!({"filePath":scan}))
        .await;
    match h.wait(&run).await {
        ToolOutcome::Failed { error } => assert!(error.message.contains("OCR")),
        other => panic!("扫描件不得冒称读取成功：{other:?}"),
    }
    h.close().await;
}

#[tokio::test]
async fn cover_requires_confirmation_and_preserves_markdown_and_actual_image() {
    let h = Harness::boot("operate").await;
    let path = h.workspace.join("article.md");
    let before = std::fs::read(&path).unwrap();
    let run = h
        .submit(
            "tool:plugin:beautify:create-cover",
            json!({"targetFilePath":path,"generatedFilePath":h.workspace.join("image.png")}),
        )
        .await;
    let approval = h.pending(&run).await;
    assert_eq!(std::fs::read(&path).unwrap(), before);
    h.approve(&approval);
    let result = success(h.wait(&run).await);
    let cover = &result["beautifyCover"];
    let copied = Path::new(cover["attachmentPath"].as_str().unwrap());
    assert_eq!(std::fs::read(copied).unwrap(), png());
    let markdown = std::fs::read_to_string(path).unwrap();
    assert!(markdown.contains("title: Kept"));
    assert!(markdown.contains("keep: 7"));
    assert!(markdown.ends_with("# Body stays\n"));
    assert!(markdown.contains(cover["cover"]["image"].as_str().unwrap()));
    assert_eq!(cover["cover"]["actualRatio"], "1:1");
    h.close().await;
}

/// apply-cover-candidate 与 create-cover 是独立的已注册工具目标：
/// 候选图应用同样经审批、保留旧 frontmatter 并交付真实字节。
#[tokio::test]
async fn apply_cover_candidate_target_applies_existing_image_through_real_approval() {
    let h = Harness::boot("operate").await;
    let path = h.workspace.join("article.md");
    let before = std::fs::read(&path).unwrap();
    let run = h
        .submit(
            "tool:plugin:beautify:apply-cover-candidate",
            json!({"targetFilePath":path,"imageFilePath":h.workspace.join("image.png")}),
        )
        .await;
    let approval = h.pending(&run).await;
    assert_eq!(std::fs::read(&path).unwrap(), before);
    h.approve(&approval);
    let result = success(h.wait(&run).await);
    let cover = &result["beautifyCover"];
    let copied = Path::new(cover["attachmentPath"].as_str().unwrap());
    assert_eq!(std::fs::read(copied).unwrap(), png());
    let markdown = std::fs::read_to_string(path).unwrap();
    assert!(markdown.contains("title: Kept"));
    assert!(markdown.contains("keep: 7"));
    assert!(markdown.contains(cover["cover"]["image"].as_str().unwrap()));
    h.close().await;
}

#[tokio::test]
async fn read_only_and_live_toggle_revocation_prevent_cover_writes() {
    let h = Harness::boot("read_only").await;
    let path = h.workspace.join("article.md");
    let before = std::fs::read(&path).unwrap();
    let run = h
        .submit(
            "tool:plugin:beautify:create-cover",
            json!({"targetFilePath":path,"generatedFilePath":h.workspace.join("image.png")}),
        )
        .await;
    assert!(matches!(h.wait(&run).await, ToolOutcome::Failed { .. }));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!h.workspace.join("Attachments").exists());
    h.close().await;
    let h = Harness::boot("ask").await;
    let path = h.workspace.join("article.md");
    let before = std::fs::read(&path).unwrap();
    let run = h
        .submit(
            "tool:plugin:beautify:create-cover",
            json!({"targetFilePath":path,"generatedFilePath":h.workspace.join("image.png")}),
        )
        .await;
    let approval = h.pending(&run).await;
    std::fs::write(
        h.state.memory().config_path("a").unwrap(),
        "agent:\n  name: a\nmemory:\n  enabled: false\ntools:\n  disabled: [beautify]\n",
    )
    .unwrap();
    h.approve(&approval);
    assert!(matches!(h.wait(&run).await, ToolOutcome::Failed { .. }));
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert!(h.pdf.jobs.lock().unwrap().is_empty());
    h.close().await;
}

#[tokio::test]
async fn pdf_collects_authorized_assets_and_three_bundled_fonts_then_registers_real_file() {
    let h = Harness::boot("operate").await;
    let html = h.workspace.join("source.html");
    std::fs::write(&html,"<!doctype html><html><head><title>Local</title></head><body><img src='image.png'><a href='https://example.invalid/'>link</a></body></html>").unwrap();
    let run = h
        .submit(
            "tool:plugin:office:html-to-pdf",
            json!({"htmlPath":html,"filename":"actual.pdf","pageSize":"A5","margins":{"top":"0.25"}}),
        )
        .await;
    let approval = h.pending(&run).await;
    assert!(h.pdf.jobs.lock().unwrap().is_empty());
    h.approve(&approval);
    let result = success(h.wait(&run).await);
    let file = &result["office"]["sessionFile"];
    assert_eq!(file["sessionId"], "office-a");
    assert_eq!(file["mime"], "application/pdf");
    assert_eq!(
        std::fs::read(file["filePath"].as_str().unwrap()).unwrap(),
        std::fs::read(repo().join("tests/fixtures/document-extract/sample-text.pdf")).unwrap()
    );
    {
        let jobs = h.pdf.jobs.lock().unwrap();
        assert_eq!(jobs.len(), 1);
        let (scope, job) = &jobs[0];
        assert!(!scope.execution_node.is_empty());
        assert_eq!(job.settings.paper_width, 5.83);
        assert_eq!(job.settings.margin_top, 0.25);
        assert!(!job.allow_javascript);
        assert!(!job.html.contains("src='image.png'"));
        assert!(job.assets.iter().any(|asset| asset.bytes == png()));
        for family in ["EB Garamond", "Noto Serif SC", "JetBrains Mono"] {
            assert!(job.html.contains(family));
        }
        assert!(job
            .assets
            .iter()
            .all(|asset| asset.url.starts_with("https://lingxi-pdf.invalid/")));
    }
    let response = reqwest::Client::new()
        .get(format!(
            "{}/lingxi/v1/resources/res_{}/content",
            h.base,
            file["fileId"].as_str().unwrap()
        ))
        .bearer_auth(h.state.auth().local_token())
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        response.headers()[reqwest::header::CONTENT_TYPE],
        "application/pdf"
    );
    assert!(response.bytes().await.unwrap().starts_with(b"%PDF-"));
    h.close().await;
}

#[tokio::test]
async fn pdf_private_asset_is_refused_before_any_external_renderer_call() {
    let h = Harness::boot("operate").await;
    let private = h.root.join("private.png");
    std::fs::write(&private, png()).unwrap();
    let html = h.workspace.join("private.html");
    std::fs::write(&html, format!("<img src='{}'>", private.display())).unwrap();
    let run = h
        .submit(
            "tool:plugin:office:html-to-pdf",
            json!({"htmlPath":html,"embedLingxiFonts":false}),
        )
        .await;
    let approval = h.pending(&run).await;
    h.approve(&approval);
    assert!(matches!(h.wait(&run).await, ToolOutcome::Failed { .. }));
    assert!(h.pdf.jobs.lock().unwrap().is_empty());
    h.close().await;
}

#[tokio::test]
async fn actual_http_apply_preserves_old_shape_and_refuses_self_reported_identity() {
    let h = Harness::boot("read_only").await;
    let path = h.workspace.join("article.md");
    let body = json!({"filePath":path,"image":{"filename":"upload.png","contentBase64":PNG},"requestId":"human-cover-one"});
    let (status, value) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/beautify/cover/apply",
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["ok"], true);
    let copied = value["beautifyCover"]["attachmentPath"].as_str().unwrap();
    assert_eq!(std::fs::read(copied).unwrap(), png());
    let (status, replayed) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/beautify/cover/apply",
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, 200, "{replayed}");
    assert_eq!(
        replayed["beautifyCover"]["attachmentPath"],
        value["beautifyCover"]["attachmentPath"]
    );
    let mut forged = body;
    forged["principal"] = json!({"kind":"localOwner"});
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/beautify/cover/apply",
            Some(forged),
        )
        .await;
    assert!((400..500).contains(&status));
    let (status, value) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/desk/beautify/status?agentId=b",
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        value["agentGenerate"]["disabledReason"],
        "beautify-disabled"
    );
    h.close().await;
}

/// 内置预设封面：图库字节必须来自真实捆绑资产目录（canonicalize 后仍在其内），
/// 应用保留旧 frontmatter；未知 presetId 响亮 400 且不触碰目标文档。
#[tokio::test]
async fn bundled_preset_apply_uses_real_gallery_bytes_and_rejects_unknown_presets() {
    let h = Harness::boot("read_only").await;
    let path = h.workspace.join("article.md");
    let body =
        json!({"filePath":path,"presetId":"blue-island-watercolor","requestId":"human-preset-one"});
    let (status, value) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/beautify/cover/preset/apply",
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["ok"], true);
    let gallery = repo().join("desktop/src/assets/cover-gallery/blue-island-watercolor.jpg");
    let copied = value["beautifyCover"]["attachmentPath"].as_str().unwrap();
    assert_eq!(
        std::fs::read(copied).unwrap(),
        std::fs::read(&gallery).unwrap()
    );
    let markdown = std::fs::read_to_string(&path).unwrap();
    assert!(markdown.contains("title: Kept"));
    assert!(markdown.contains("keep: 7"));
    assert!(markdown.ends_with("# Body stays\n"));
    let (status, replayed) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/beautify/cover/preset/apply",
            Some(body),
        )
        .await;
    assert_eq!(status, 200, "{replayed}");
    assert_eq!(
        replayed["beautifyCover"]["attachmentPath"], value["beautifyCover"]["attachmentPath"],
        "同一 requestId 只读原收据"
    );
    let before = std::fs::read(&path).unwrap();
    let (status, refused) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/beautify/cover/preset/apply",
            Some(json!({"filePath":path,"presetId":"../../escape","requestId":"human-preset-two"})),
        )
        .await;
    assert_eq!(status, 400, "{refused}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "未知预设不得触碰文档"
    );
    h.close().await;
}

#[tokio::test]
async fn cancelled_pdf_waits_for_actual_external_cleanup_and_never_registers_a_file() {
    let h = Harness::boot("operate").await;
    h.pdf.wait_for_cancel.store(true, Ordering::Release);
    let run = h
        .submit(
            "tool:plugin:office:html-to-pdf",
            json!({"html":"<p>cancel this render</p>","embedLingxiFonts":false}),
        )
        .await;
    let pending = h.pending(&run).await;
    h.approve(&pending);
    tokio::time::timeout(Duration::from_secs(10), async {
        while h.pdf.jobs.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
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
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if h.state
                .storage()
                .load_run(&RunId::new(&run))
                .await
                .unwrap()
                .unwrap()
                .status
                .is_terminal()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        h.pdf.cleanup.load(Ordering::Acquire),
        "任务结束前必须收到真实隔离页清理确认"
    );
    let files = h
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM session_files WHERE owner_session_id='office-a'",
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(files.as_deref(), Some("0"));
    h.close().await;
}

#[tokio::test]
async fn guide_state_is_owned_by_the_actual_session_and_plugin_toggle_is_effective() {
    let h = Harness::boot("read_only").await;
    let run = h
        .submit(
            "tool:plugin:beautify:get-html-style-guide",
            json!({"section":"layout"}),
        )
        .await;
    let blocked = success(h.wait(&run).await);
    assert_eq!(blocked["kind"], "must-read-first");
    assert_eq!(blocked["missingRequired"], json!(["color", "typography"]));
    for section in ["color", "typography", "layout"] {
        let run = h
            .submit(
                "tool:plugin:beautify:get-html-style-guide",
                json!({"section":section}),
            )
            .await;
        let details = success(h.wait(&run).await);
        assert_eq!(details["kind"], "section");
        assert_eq!(details["section"], section);
    }
    std::fs::write(
        h.state.memory().config_path("a").unwrap(),
        "agent:\n  name: a\nmemory:\n  enabled: false\ntools:\n  disabled: [office]\n",
    )
    .unwrap();
    let source = h.workspace.join("notes.txt");
    std::fs::write(&source, "private source remains").unwrap();
    let run = h
        .submit(
            "tool:plugin:office:read-document",
            json!({"filePath":source}),
        )
        .await;
    assert!(matches!(h.wait(&run).await, ToolOutcome::Failed { .. }));
    assert_eq!(
        std::fs::read_to_string(source).unwrap(),
        "private source remains"
    );
    h.close().await;
}

/// 能力目录与封面风格指南经真实网关交付实际内容：office/beautify 双目录、
/// 主题色调分支与用户指引透传，不是占位文本。
#[tokio::test]
async fn capabilities_and_cover_style_guide_deliver_real_catalog_content() {
    let h = Harness::boot("read_only").await;
    let run = h
        .submit("tool:plugin:office:list-capabilities", json!({}))
        .await;
    let office = success(h.wait(&run).await);
    assert_eq!(office["capabilities"][0]["id"], "read-document");
    assert_eq!(office["capabilities"][1]["id"], "html-to-pdf");
    assert!(office["capabilities"][0]["supportedInputs"]
        .as_array()
        .unwrap()
        .contains(&json!(".docx")));
    let run = h
        .submit("tool:plugin:beautify:list-capabilities", json!({}))
        .await;
    let caps = success(h.wait(&run).await);
    assert_eq!(caps["capabilities"][0]["id"], "markdown-cover");
    assert_eq!(caps["capabilities"][0]["imageRatio"], "3:2");
    assert_eq!(caps["capabilities"][1]["id"], "html-style-guide");
    let run = h
        .submit(
            "tool:plugin:beautify:get-cover-style-guide",
            json!({"themeTone":"dark","userGuidance":"calm"}),
        )
        .await;
    let outcome = h.wait(&run).await;
    let ToolOutcome::Success { result } = &outcome else {
        panic!("预期成功，实际{outcome:?}")
    };
    let text = result
        .content
        .iter()
        .filter_map(|block| match block {
            lingxi_protocol::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(text.contains("calm"), "用户指引必须透传进指南正文：{text}");
    let guide = Value::Object(result.details.clone().expect("风格指南必须返回真实结构"));
    assert_eq!(guide["themeTone"], "dark");
    assert_eq!(guide["version"], "2026-05-26");
    assert_eq!(guide["workflow"].as_array().unwrap().len(), 5);
    assert!(
        guide["text"].is_null(),
        "正文只走内容块，details 不重复携带 text"
    );
    let run = h
        .submit("tool:plugin:beautify:get-cover-style-guide", json!({}))
        .await;
    let light = success(h.wait(&run).await);
    assert_eq!(light["themeTone"], "light");
    h.close().await;
}

#[tokio::test]
async fn remote_cover_clients_cannot_turn_absolute_paths_into_local_owner_authority() {
    let h = Harness::boot("operate").await;
    let (status, credential) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/devices/credentials",
            Some(json!({"userId":"user_local","scopes":["resources.read","resources.write"]})),
        )
        .await;
    assert_eq!(status, 201, "{credential}");
    let secret = credential["secret"].as_str().unwrap();
    for body in [
        json!({"filePath":h.workspace.join("article.md"),"image":{"filename":"upload.png","contentBase64":PNG}}),
        json!({"target":{"kind":"workbench-file","name":"article.md"},"imageFilePath":h.workspace.join("image.png")}),
    ] {
        let response = reqwest::Client::new()
            .post(format!("{}/lingxi/v1/desk/beautify/cover/apply", h.base))
            .bearer_auth(secret)
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    }
    assert!(!h.workspace.join("Attachments").exists());
    h.close().await;
}

/// 后台封面任务全链：真实组合根 + 回环媒体替身。先返回 running 活动；
/// 同 requestId 重复提交只读原收据；完成后 Markdown 获得真实封面，
/// 附件字节与供应商交付逐字节一致，媒体供应商只被真实调用一次。
#[tokio::test]
async fn background_cover_job_completes_with_real_media_and_applies_frozen_write() {
    use base64::Engine as _;
    let image = png();
    let stub = MediaStub::start(
        base64::engine::general_purpose::STANDARD.encode(&image),
        None,
    )
    .await;
    let h = Harness::boot_media("operate", &stub.endpoint).await;
    let target = h.workspace.join("article.md");
    let request = json!({"filePath":target,"agentId":"a","themeTone":"light","userGuidance":"calm","requestId":"cover-happy"});
    let (status, first) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/beautify/cover",
            Some(request.clone()),
        )
        .await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["ok"], true);
    assert_eq!(first["activity"]["status"], "running", "{first}");
    assert_eq!(first["effectUnknown"], false);
    let job_id = first["jobId"].as_str().unwrap().to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let last = loop {
        let (status, again) = h
            .http(
                reqwest::Method::POST,
                "/lingxi/v1/desk/beautify/cover",
                Some(request.clone()),
            )
            .await;
        assert_eq!(status, 200, "{again}");
        assert_eq!(again["jobId"], job_id, "同requestId不得另建任务");
        if again["activity"]["status"] != "running" {
            break again;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "封面任务超时：{again}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    if last["activity"]["status"] != "completed" {
        let run_id = last["runId"].as_str().unwrap();
        let operation = format!(
            "media-{}",
            lingxi_protocol::canon::sha256_hex(
                json!([run_id, format!("{run_id}-cover-image"), "image"])
                    .to_string()
                    .as_bytes()
            )
        );
        let (mstatus, mjob) = h
            .http(
                reqwest::Method::GET,
                &format!(
                    "/lingxi/v1/sessions/{}/media/jobs/{operation}",
                    last["sessionId"].as_str().unwrap()
                ),
                None,
            )
            .await;
        panic!("封面任务未完成：{last}；媒体作业[{mstatus}]：{mjob}");
    }
    assert_eq!(stub.image_hits(), 1, "媒体供应商必须只被真实调用一次");
    let run_id = last["runId"].as_str().unwrap();
    let journal = h
        .state
        .storage()
        .load_invocation_journal(&lingxi_protocol::RunId::new(run_id))
        .await
        .unwrap();
    assert_eq!(journal.len(), 2, "{journal:?}");
    for (label, target) in [
        ("cover-image", "tool:plugin:media:generate-image"),
        ("cover-apply", "tool:plugin:beautify:create-cover"),
    ] {
        let entry = journal
            .iter()
            .find(|entry| entry.journal_id == format!("{run_id}-{label}"))
            .unwrap();
        assert_eq!(entry.target, target);
        assert_eq!(entry.session_id, last["sessionId"].as_str().unwrap());
        assert_eq!(
            entry.idempotency_key.as_deref(),
            Some(format!("{job_id}:{label}").as_str())
        );
        assert_eq!(
            entry.phase,
            lingxi_kernel::ports::InvocationPhase::Succeeded
        );
        let receipt = entry.receipt.as_ref().unwrap();
        assert!(receipt.dispatched);
        assert_eq!(
            receipt.outcome,
            lingxi_kernel::ports::ReceiptOutcome::Succeeded
        );
    }

    let applied = &last["activity"]["result"]["beautifyCover"];
    let attachment = PathBuf::from(applied["attachmentPath"].as_str().expect("附件路径"));
    assert_eq!(std::fs::read(&attachment).unwrap(), image);
    let markdown = std::fs::read_to_string(&target).unwrap();
    assert!(markdown.contains("title: Kept"));
    assert!(markdown.ends_with("# Body stays\n"));
    assert!(markdown.contains(applied["cover"]["image"].as_str().unwrap()));
    // 状态读取的正向面：启用美化且媒体路由可复核时必须如实报告可用，
    // 不得只在禁用路径下可证。
    let (status, probe) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/desk/beautify/status?agentId=a",
            None,
        )
        .await;
    assert_eq!(status, 200, "{probe}");
    assert_eq!(probe["agentGenerate"]["enabled"], true, "{probe}");
    assert_eq!(probe["agentGenerate"]["available"], true, "{probe}");
    stub.close().await;
    h.close().await;
}

/// 媒体生成期间目标文档被改写：应用步骤必须执行受理时冻结的版本事实，
/// 响亮失败并保持文档现状，不能静默采纳新版本。
#[tokio::test]
async fn background_cover_job_refuses_to_apply_after_document_changed() {
    use base64::Engine as _;
    let slot = Arc::new(Mutex::new(None::<PathBuf>));
    let mutate = {
        let slot = slot.clone();
        Arc::new(move || {
            let path = slot.lock().unwrap().clone().expect("目标已记录");
            std::fs::write(
                path,
                "---\ntitle: Changed\n---\n# New body while media generates\n",
            )
            .unwrap();
        }) as Arc<dyn Fn() + Send + Sync>
    };
    let stub = MediaStub::start(
        base64::engine::general_purpose::STANDARD.encode(png()),
        Some(mutate),
    )
    .await;
    let h = Harness::boot_media("operate", &stub.endpoint).await;
    let target = h.workspace.join("article.md");
    *slot.lock().unwrap() = Some(target.clone());
    let request =
        json!({"filePath":target,"agentId":"a","themeTone":"light","requestId":"cover-drift"});
    let (status, first) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/beautify/cover",
            Some(request.clone()),
        )
        .await;
    assert_eq!(status, 200, "{first}");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let last = loop {
        let (status, again) = h
            .http(
                reqwest::Method::POST,
                "/lingxi/v1/desk/beautify/cover",
                Some(request.clone()),
            )
            .await;
        assert_eq!(status, 200, "{again}");
        if again["activity"]["status"] != "running" {
            break again;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "封面任务超时：{again}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(stub.image_hits(), 1);
    // 应用阶段被拒绝后，驱动不能断言副作用为零：诚实映射为 unknown 并在摘要保留拒绝原因。
    assert_eq!(last["activity"]["status"], "unknown", "{last}");
    assert_eq!(last["effectUnknown"], true);
    assert!(
        last["activity"]["summary"]
            .as_str()
            .is_some_and(|s| s.contains("文档在读取后、预检前已改变")),
        "{last}"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "---\ntitle: Changed\n---\n# New body while media generates\n",
        "应用必须保持文档现状"
    );
    assert!(!h.workspace.join("Attachments").exists());
    stub.close().await;
    h.close().await;
}

/// 图片请求已真实发出后撤销插件或取消任务，统一入口不得继续应用封面或重发付费请求。
#[tokio::test]
async fn background_cover_fixed_actions_honor_live_revocation_and_cancel_before_apply() {
    use base64::Engine as _;
    for revoke in [true, false] {
        let stub = MediaStub::start(
            base64::engine::general_purpose::STANDARD.encode(png()),
            None,
        )
        .await;
        stub.hold_images.store(true, Ordering::Release);
        let h = Harness::boot_media("operate", &stub.endpoint).await;
        let target = h.workspace.join("article.md");
        let original = std::fs::read(&target).unwrap();
        let request = json!({"filePath":target,"agentId":"a","themeTone":"light","requestId":if revoke {"cover-live-revoke"} else {"cover-live-cancel"}});
        let (status, first) = h
            .http(
                reqwest::Method::POST,
                "/lingxi/v1/desk/beautify/cover",
                Some(request),
            )
            .await;
        assert_eq!(status, 200, "{first}");
        let job_id = first["jobId"].as_str().unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while stub.image_hits() != 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let job = lingxi_adapters::storage::beautify::get(h.state.storage(), job_id)
            .await
            .unwrap()
            .unwrap();
        let run_id = job.run_id.as_deref().unwrap();
        if revoke {
            std::fs::write(
                h.state.memory().config_path("a").unwrap(),
                "agent:\n  name: a\nmemory:\n  enabled: false\ntools:\n  disabled: [beautify]\n",
            )
            .unwrap();
        } else {
            assert_eq!(
                h.state.runs().cancel_run(run_id, "用户取消真实封面动作"),
                lingxi_service::cancel::FireOutcome::Fired
            );
        }
        stub.hold_images.store(false, Ordering::Release);
        let terminal = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let actual = lingxi_adapters::storage::beautify::get(h.state.storage(), job_id)
                    .await
                    .unwrap()
                    .unwrap();
                let run = h
                    .state
                    .storage()
                    .load_run(&lingxi_protocol::RunId::new(run_id))
                    .await
                    .unwrap()
                    .unwrap();
                if actual.phase.terminal()
                    && run.status.is_terminal()
                    && !h
                        .state
                        .storage()
                        .session_admission()
                        .is_busy(&job.session_id)
                {
                    break actual;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_ne!(
            terminal.phase,
            lingxi_adapters::storage::beautify::CoverPhase::Completed,
            "{terminal:?}"
        );
        assert_eq!(stub.image_hits(), 1);
        assert_eq!(std::fs::read(&target).unwrap(), original);
        assert!(!h.workspace.join("Attachments").exists());
        let journal = h
            .state
            .storage()
            .load_invocation_journal(&lingxi_protocol::RunId::new(run_id))
            .await
            .unwrap();
        assert!(journal
            .iter()
            .any(|entry| entry.target == "tool:plugin:media:generate-image"
                && entry
                    .receipt
                    .as_ref()
                    .is_some_and(|receipt| receipt.dispatched)));
        assert!(
            !journal
                .iter()
                .any(|entry| entry.target == "tool:plugin:beautify:create-cover"
                    && entry
                        .receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.dispatched)),
            "{journal:?}"
        );
        stub.close().await;
        h.close().await;
    }
}
