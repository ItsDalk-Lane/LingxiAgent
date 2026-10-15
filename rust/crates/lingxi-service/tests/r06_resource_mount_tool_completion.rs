//! 复用已交资源工具的真实服务夹具，独立核验挂载由实际Run和下一模型消费。

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
use std::{collections::HashMap, sync::Mutex};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[derive(Default)]
struct Provider {
    results: Mutex<HashMap<String, Value>>,
    calls: std::sync::atomic::AtomicUsize,
    last_run: Mutex<Option<String>>,
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
        *self.last_run.lock().unwrap() = Some(ctx.run_id.to_string());
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
    approval: Arc<lingxi_service::approval_service::ApprovalService>,
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
                resource_url_provider: Some(Arc::new(
                    lingxi_service::resource_url::UrlProvider::new(&home),
                )),
                context_compiler: Some(compiler),
                clock,
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway),
                approval_gate: Some(approval.clone()),
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
            approval,
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
        let wire = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
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
async fn actual_run_mount_materialize_extract_copy_and_revoked_read() {
    let h = Harness::boot().await;
    h.create("mount-tool", "operate").await;
    let mount = h.home.parent().unwrap().join("configured-mount");
    std::fs::create_dir(&mount).unwrap();
    std::fs::write(mount.join("actual.txt"), "真实模型挂载正文").unwrap();
    let (_, identity) = h
        .request("GET", "/lingxi/v1/server/identity", json!({}), false)
        .await;
    let registry = h.home.join("studio-mounts.json");
    let original = json!({"schemaVersion":1,"mounts":[{"schemaVersion":1,"mountId":"model-mount","hostStudioId":identity["studioId"],"sourceKind":"storage","provider":"local_fs","rootLocator":{"path":mount},"label":"模型挂载","presentation":"folder","status":"active","capabilities":["read","write","list","watch","materialize"]}]});
    std::fs::write(&registry, serde_json::to_vec(&original).unwrap()).unwrap();
    let reference = json!({"kind":"mount","mountId":"model-mount","path":"actual.txt"});
    let materialized = h
        .tool("mount-tool", "materialize", json!({"resource":reference}))
        .await;
    assert_eq!(materialized["ok"], true, "{materialized}");
    assert_eq!(materialized["details"]["resource"]["kind"], "mount");
    assert_eq!(
        std::fs::read_to_string(materialized["details"]["filePath"].as_str().unwrap()).unwrap(),
        "真实模型挂载正文"
    );
    let extracted = h
        .tool(
            "mount-tool",
            "file",
            json!({"action":"extract","resource":reference}),
        )
        .await;
    assert_eq!(extracted["ok"], true, "{extracted}");
    assert_eq!(extracted["details"]["text"], "真实模型挂载正文");
    assert_eq!(
        extracted["details"]["source"]["resource"]["mountId"],
        "model-mount"
    );
    let archived=h.state.storage().query_one_text("SELECT json_extract(content_json,'$.payload.items[0].outcome.details') FROM messages WHERE session_id='mount-tool' AND role='model_history' AND json_extract(content_json,'$.payload.items[0].outcome.details.source.resource.mountId')='model-mount' ORDER BY seq DESC LIMIT 1",vec![]).await.unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&archived).unwrap(),
        extracted["details"]
    );
    assert!(h
        .state
        .session_files()
        .list_files("mount-tool")
        .await
        .unwrap()
        .iter()
        .all(|row| row.real_path != mount.join("actual.txt").to_string_lossy()));
    let copied = h
        .tool(
            "mount-tool",
            "file",
            json!({"action":"copy","resource":reference,"targetPath":"mounted-copy.txt"}),
        )
        .await;
    assert_eq!(copied["ok"], true, "{copied}");
    assert_eq!(
        std::fs::read_to_string(
            h.state
                .resource_io()
                .workspace_root()
                .unwrap()
                .join("mounted-copy.txt")
        )
        .unwrap(),
        "真实模型挂载正文"
    );
    let denied = h
        .tool(
            "mount-tool",
            "file",
            json!({"action":"extract","resource":reference,"sessionId":"other-session"}),
        )
        .await;
    assert_eq!(denied["ok"], false);
    assert!(denied.to_string().contains("session identity mismatch"));
    let changed = registry.clone();
    let mut disabled = original.clone();
    disabled["mounts"][0]["status"] = json!("disabled");
    h.state
        .session_files()
        .set_payload_read_observer(Arc::new(move || {
            std::fs::write(&changed, serde_json::to_vec(&disabled).unwrap()).unwrap()
        }));
    let refused = h
        .tool(
            "mount-tool",
            "file",
            json!({"action":"extract","resource":reference}),
        )
        .await;
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"], "挂载不存在或已停用：model-mount");
    // 已获授权复制的新文件有独立身份，源挂载撤销后仍可正常读取副本。
    h.state
        .session_files()
        .set_payload_read_observer(Arc::new(|| {}));
    let copy_id = copied["details"]["file"]["fileId"].as_str().unwrap();
    let independent = h
        .tool(
            "mount-tool",
            "file",
            json!({"action":"extract","fileId":copy_id}),
        )
        .await;
    assert_eq!(independent["ok"], true, "{independent}");
    assert_eq!(independent["details"]["text"], "真实模型挂载正文");

    assert_eq!(
        std::fs::read_to_string(mount.join("actual.txt")).unwrap(),
        "真实模型挂载正文"
    );
    assert_eq!(
        h.provider.calls.load(std::sync::atomic::Ordering::SeqCst),
        12
    );
    std::fs::write(&registry, serde_json::to_vec(&original).unwrap()).unwrap();
    let observed = h.provider.clone();
    let supervisor = h.state.runs().clone();
    h.state
        .session_files()
        .set_payload_read_observer(Arc::new(move || {
            supervisor.cancel_run(
                observed
                    .last_run
                    .lock()
                    .unwrap()
                    .as_ref()
                    .expect("真实Run取消树"),
                "mounted read actual cancellation",
            );
        }));
    let command=json!({"tool":"file","argsBase64":base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&json!({"action":"extract","resource":reference,"sessionId":"mount-tool"})).unwrap())}).to_string();
    let (status, cancelled) = h
        .request(
            "POST",
            "/lingxi/v1/sessions/mount-tool/execute",
            json!({"input":command}),
            false,
        )
        .await;
    assert_eq!(status, 200, "{cancelled}");
    let saved = h
        .state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id=?1",
            vec![cancelled["runId"].as_str().unwrap().into()],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved, "cancelled");
    assert_eq!(
        h.provider.calls.load(std::sync::atomic::Ordering::SeqCst),
        13
    );
    assert!(!h
        .provider
        .results
        .lock()
        .unwrap()
        .contains_key(&format!("mount-tool|{command}")));
    assert_eq!(
        std::fs::read_to_string(mount.join("actual.txt")).unwrap(),
        "真实模型挂载正文"
    );
    h.close().await;
}

async fn raw_resource_request(
    h: &Harness,
    method: &str,
    path: &str,
    extra: &str,
) -> (u16, String, Vec<u8>) {
    let mut stream = tokio::net::TcpStream::connect(h.url.strip_prefix("http://").unwrap())
        .await
        .unwrap();
    let wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nConnection: close\r\n{extra}\r\n",
        h.token
    );
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(15), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let split = bytes.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    let header = String::from_utf8_lossy(&bytes[..split]).into_owned();
    (
        header.split_whitespace().nth(1).unwrap().parse().unwrap(),
        header,
        bytes[split + 4..].to_vec(),
    )
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resource_content_actual_swap_never_delivers_foreign_file() {
    let h = Harness::boot().await;
    h.create("content-owner", "operate").await;
    let victim = h.home.parent().unwrap().join("workspace/download.txt");
    let outside = h.home.parent().unwrap().join("private.txt");
    std::fs::write(&victim, "ALLOWED-1").unwrap();
    std::fs::write(&outside, "PRIVATE-1").unwrap();
    let view = h
        .state
        .session_files()
        .register_file("content-owner", &victim, None, None, "user_upload")
        .await
        .unwrap();
    let swapped = victim.clone();
    let secret = outside.clone();
    h.state
        .session_files()
        .set_payload_read_observer(Arc::new(move || {
            std::fs::remove_file(&swapped).unwrap();
            std::os::unix::fs::symlink(&secret, &swapped).unwrap();
        }));
    let (status, _, body) = raw_resource_request(
        &h,
        "GET",
        &format!("/lingxi/v1/resources/res_{}/content", view.file_id),
        "",
    )
    .await;
    assert_eq!(
        status,
        409,
        "换链接后必须拒绝，真实正文：{}",
        String::from_utf8_lossy(&body)
    );
    assert!(!String::from_utf8_lossy(&body).contains("PRIVATE-1"));
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "PRIVATE-1");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_resource_head_is_metadata_only_large_range_and_json_budget() {
    use std::io::{Seek, SeekFrom, Write};
    let h = Harness::boot().await;
    h.create("large-owner", "operate").await;
    let path = h.home.parent().unwrap().join("workspace/large.bin");
    let mut file = std::fs::File::create(&path).unwrap();
    file.set_len(64 * 1024 * 1024).unwrap();
    file.seek(SeekFrom::End(-9)).unwrap();
    file.write_all(b"REAL-TAIL").unwrap();
    drop(file);
    let view = h
        .state
        .session_files()
        .register_file("large-owner", &path, None, None, "user_upload")
        .await
        .unwrap();
    let url = format!("/lingxi/v1/resources/res_{}/content", view.file_id);
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = reads.clone();
    h.state
        .session_files()
        .set_payload_read_observer(Arc::new(move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
    let (status, header, body) = raw_resource_request(&h, "HEAD", &url, "").await;
    assert_eq!(status, 200);
    assert!(body.is_empty());
    assert!(header
        .to_ascii_lowercase()
        .contains("content-length: 67108864"));
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 0);
    let (status, header, body) = raw_resource_request(&h, "GET", &url, "Range: bytes=-9\r\n").await;
    assert_eq!(status, 206);
    assert_eq!(body, b"REAL-TAIL");
    assert!(header
        .to_ascii_lowercase()
        .contains("content-range: bytes 67108855-67108863/67108864"));
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    for reference in [
        json!({"kind":"session-file","fileId":view.file_id}),
        json!({"kind":"resource","resourceId":format!("res_{}",view.file_id)}),
    ] {
        let (status, value) = h
            .request(
                "POST",
                "/lingxi/v1/resource-io/read",
                json!({"resource":reference,"encoding":"base64"}),
                false,
            )
            .await;
        assert_eq!(status, 413, "{value}");
        assert!(value.to_string().contains("resource_read_too_large"));
    }
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 64 * 1024 * 1024);
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_mount_materialization_cannot_mint_a_revocation_bypass_file_id() {
    let h = Harness::boot().await;
    h.create("mount-lifetime", "operate").await;
    let mount = h.home.parent().unwrap().join("mount-lifetime");
    std::fs::create_dir(&mount).unwrap();
    let file = mount.join("source.txt");
    std::fs::write(&file, "MOUNT-ONLY-BYTES").unwrap();
    let (_, identity) = h
        .request("GET", "/lingxi/v1/server/identity", json!({}), false)
        .await;
    let registry = h.home.join("studio-mounts.json");
    let mut configured = json!({"schemaVersion":1,"mounts":[{"schemaVersion":1,"mountId":"lifetime","hostStudioId":identity["studioId"],"sourceKind":"storage","provider":"local_fs","rootLocator":{"path":mount},"label":"挂载生命周期","presentation":"folder","status":"active","capabilities":["read","list","materialize"]}]});
    std::fs::write(&registry, serde_json::to_vec(&configured).unwrap()).unwrap();
    let reference = json!({"kind":"mount","mountId":"lifetime","path":"source.txt"});
    let resolved = h
        .tool(
            "mount-lifetime",
            "materialize",
            json!({"resource":reference}),
        )
        .await;
    assert_eq!(resolved["ok"], true);
    let aliases = h
        .state
        .session_files()
        .list_files("mount-lifetime")
        .await
        .unwrap();
    configured["mounts"][0]["status"] = json!("disabled");
    std::fs::write(&registry, serde_json::to_vec(&configured).unwrap()).unwrap();
    let refused = h
        .tool(
            "mount-lifetime",
            "file",
            json!({"action":"extract","resource":reference}),
        )
        .await;
    assert_eq!(refused["ok"], false);
    let mut bypass = json!(null);
    if let Some(alias) = aliases
        .iter()
        .find(|row| row.real_path == file.to_string_lossy())
    {
        bypass = h
            .tool(
                "mount-lifetime",
                "file",
                json!({"action":"extract","fileId":alias.file_id}),
            )
            .await;
    }
    assert!(
        aliases
            .iter()
            .all(|row| row.real_path != file.to_string_lossy()),
        "物化不可铸造绕过挂载撤权的文件ID，真实后续工具结果：{bypass}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "MOUNT-ONLY-BYTES");
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_forged_mount_descriptor_cannot_bypass_session_file_ownership_or_target() {
    let h = Harness::boot().await;
    h.create("fake-a", "operate").await;
    h.create("fake-b", "operate").await;
    let long_mount_id = "long".repeat(100);
    let mount = h.home.parent().unwrap().join("fake-mount");
    std::fs::create_dir(&mount).unwrap();
    let allowed = mount.join("allowed.txt");
    let foreign = h.home.parent().unwrap().join("foreign.txt");
    std::fs::write(&allowed, "ALLOWED-MOUNT").unwrap();
    std::fs::write(&foreign, "FOREIGN-PRIVATE").unwrap();
    let other = h
        .state
        .session_files()
        .register_file("fake-b", &foreign, None, None, "user_upload")
        .await
        .unwrap();
    let (_, identity) = h
        .request("GET", "/lingxi/v1/server/identity", json!({}), false)
        .await;
    std::fs::write(h.home.join("studio-mounts.json"),serde_json::to_vec(&json!({"schemaVersion":1,"mounts":[{"schemaVersion":1,"mountId":long_mount_id,"hostStudioId":identity["studioId"],"sourceKind":"storage","provider":"local_fs","rootLocator":{"path":mount},"label":"伪造负控","presentation":"folder","status":"active","capabilities":["read","list","materialize"]}]})).unwrap()).unwrap();
    let forged = json!({"kind":"mount","provider":"mount","mountId":long_mount_id,"path":"allowed.txt","filePath":foreign});
    // 公开正文扫描不能靠一份看似完整的resource元数据跳过旧归属检查。
    for value in [
        forged.clone(),
        json!({"filePath":foreign,"resource":forged}),
        json!({"fileId":other.file_id,"resource":forged}),
        json!({"realPath":foreign,"resource":forged}),
        json!(format!("[SessionFile] {}", forged)),
    ] {
        let scan = h
            .state
            .session_files()
            .collect_reference_identities("fake-a", std::slice::from_ref(&value))
            .await
            .unwrap();
        assert!(
            !scan.broken.is_empty(),
            "伪造身份不得变成合法会话附件：{value}"
        );
        assert!(!lingxi_adapters::storage::session_files::reference_identities(&value).is_empty());
    }
    let result = h
        .tool(
            "fake-a",
            "file",
            json!({"action":"extract","resource":forged}),
        )
        .await;
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error"], "mount path claim mismatch");
    let actual = json!({"kind":"mount","provider":"mount","mountId":long_mount_id,"path":"allowed.txt","filePath":allowed});
    let result = h
        .tool(
            "fake-a",
            "file",
            json!({"action":"extract","resource":actual}),
        )
        .await;
    assert_eq!(result["ok"], true);
    assert_eq!(result["details"]["text"], "ALLOWED-MOUNT");
    assert!(!result.to_string().contains("FOREIGN-PRIVATE"));
    assert!(h
        .state
        .session_files()
        .list_files("fake-a")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        std::fs::read_to_string(&foreign).unwrap(),
        "FOREIGN-PRIVATE"
    );
    h.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_pending_copy_approval_binds_mount_root_and_rejects_new_root() {
    for drift in [false, true] {
        let h = Harness::boot().await;
        h.create("mount-approval", "ask").await;
        let a = h.home.parent().unwrap().join("approval-a");
        let b = h.home.parent().unwrap().join("approval-b");
        for path in [&a, &b] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(a.join("source.txt"), "FROM-A").unwrap();
        std::fs::write(b.join("source.txt"), "FROM-B").unwrap();
        let (_, identity) = h
            .request("GET", "/lingxi/v1/server/identity", json!({}), false)
            .await;
        let registry = h.home.join("studio-mounts.json");
        let mut value = json!({"schemaVersion":1,"mounts":[{"schemaVersion":1,"mountId":"approval","hostStudioId":identity["studioId"],"sourceKind":"storage","provider":"local_fs","rootLocator":{"path":a},"label":"批准来源","presentation":"folder","status":"active","capabilities":["read","list","materialize"]}]});
        std::fs::write(&registry, serde_json::to_vec(&value).unwrap()).unwrap();
        let call = h.tool("mount-approval", "file", json!({"action":"copy","resource":{"kind":"mount","mountId":"approval","path":"source.txt"},"targetPath":"approval-copy.txt"}));
        let answer = async {
            let pending = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Some(first) = h
                        .approval
                        .pending_for_principal(
                            &lingxi_kernel::Principal::LocalUser,
                            "mount-approval",
                        )
                        .first()
                    {
                        break first.clone();
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            if drift {
                value["mounts"][0]["rootLocator"]["path"] = json!(b);
                std::fs::write(&registry, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            // 同一真实网关使用的审批实例，批准真实挂起请求，不伪造工具结果。
            let decision = h.approval.answer_for_principal(
                &lingxi_kernel::Principal::LocalUser,
                "mount-approval",
                &pending.approval_id,
                lingxi_service::approval_service::Answer::Approve,
            );
            assert!(
                matches!(
                    decision,
                    lingxi_service::approval_service::AnswerOutcome::Settled(
                        lingxi_service::approval_service::Answer::Approve
                    )
                ),
                "{decision:?}"
            );
            json!({"approvalId":pending.approval_id,"resources":pending.resources.iter().map(|scope|json!({"path":scope.path,"operation":scope.op.wire_name()})).collect::<Vec<_>>()})
        };
        let (result, pending) = tokio::join!(call, answer);
        let target = h
            .state
            .resource_io()
            .workspace_root()
            .unwrap()
            .join("approval-copy.txt");
        let proof = json!({"drift":drift,"approval":pending,"result":result,"targetExists":target.exists()});
        std::fs::write(
            h.home.parent().unwrap().join("approval-result.json"),
            serde_json::to_vec_pretty(&proof).unwrap(),
        )
        .unwrap();
        assert_eq!(pending["resources"].as_array().unwrap().len(), 2);
        assert!(pending["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["path"] == json!(a.join("source.txt")) && r["operation"] == "read"));
        assert!(pending["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["path"] == json!(target) && r["operation"] == "write"));
        if drift {
            assert!(!target.exists(), "批准后换挂载根不得复制新来源：{proof}");
            assert_eq!(result["ok"], false);
            assert!(
                result["error"]
                    .as_str()
                    .unwrap()
                    .contains("real resource scope changed"),
                "{proof}"
            );
        } else {
            assert_eq!(result["ok"], true, "{proof}");
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "FROM-A");
        }
        assert_eq!(
            std::fs::read_to_string(a.join("source.txt")).unwrap(),
            "FROM-A"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("source.txt")).unwrap(),
            "FROM-B"
        );
        assert_eq!(
            h.provider.calls.load(std::sync::atomic::Ordering::SeqCst),
            2
        );
        h.close().await;
    }
}
