//! T08真实服务、存储和执行链；只替换外部模型回复，不替换设置、权限和文件写入。
use axum::http::StatusCode;
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
    approval_service::ApprovalService, inject::ManualClock, prepare_layout,
    toolgateway::ToolInvocationGateway, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

const NOW: u64 = 1_800_000_000_000;
#[derive(Default)]
struct Provider {
    result: Mutex<Option<Value>>,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.fixture".into(),
            model: "cards".into(),
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
        Box::pin(async move {
            if input.turn == 1 {
                let body: Value = serde_json::from_str(&input.submission).unwrap();
                return ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::ToolRequests {
                        requests: vec![ToolRequest::from_effective_arguments(
                            "tool:first-party:show_card".to_owned(),
                            body,
                            &SchemaBudget::default(),
                        )
                        .unwrap()],
                        content: vec![],
                    },
                );
            }
            for prior in &input.prior {
                if let ExchangeItem::ToolResult { outcome, .. } = prior {
                    *self.result.lock().unwrap() = Some(match outcome {
                        ToolOutcome::Success { result } => {
                            json!({"ok":true,"details":result.details})
                        }
                        ToolOutcome::Failed { error } => json!({"ok":false,"error":error.message}),
                        _ => json!({"ok":false}),
                    });
                }
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: "卡片真实工具调用已完成".into(),
                        }],
                        model_call_id: Some(call.clone()),
                    },
                },
            )
        })
    }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    clock: Arc<ManualClock>,
    _provider: Arc<Provider>,
    _approvals: Arc<ApprovalService>,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-skills-cards-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        for id in ["a", "b"] {
            std::fs::create_dir_all(home.join("agents").join(id)).unwrap();
            std::fs::write(
                home.join("agents").join(id).join("config.yaml"),
                format!("agent:\n  name: 助手{id}\nmemory:\n  enabled: false\n"),
            )
            .unwrap();
        }
        std::fs::create_dir_all(home.join("workspace")).unwrap();
        Self::boot_home(home.canonicalize().unwrap()).await
    }
    async fn boot_home(home: PathBuf) -> Self {
        let layout = prepare_layout(&home).unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let provider = Arc::new(Provider::default());
        let approvals = Arc::new(ApprovalService::new(clock.clone()));
        let gateway = Arc::new(ToolInvocationGateway::new(
            Arc::new(ToolRegistry::new()),
            approvals.clone(),
            clock.clone(),
            SchemaBudget::default(),
            300_000,
            1000,
        ));
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
                workspace_root: Some(home.join("workspace")),
                clock: clock.clone(),
                turn_provider: Some(provider.clone()),
                tool_gateway: Some(gateway),
                approval_gate: Some(approvals.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        if state
            .storage()
            .get_session("card-session")
            .await
            .unwrap()
            .is_none()
        {
            state
                .session_admin()
                .create_session(
                    "card-session",
                    "a",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "卡片真实会话",
                    Some("operate"),
                    &[],
                    true,
                    NOW,
                )
                .await
                .unwrap();
        }
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = lingxi_service::build_router(state.clone());
        let (stop, done) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = done.await;
            })
            .await
            .unwrap();
        });
        Self {
            home,
            state,
            clock,
            _provider: provider,
            _approvals: approvals,
            url: format!("http://{addr}"),
            token,
            stop,
            server,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value) -> reqwest::Response {
        reqwest::Client::new()
            .request(method.parse().unwrap(), format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap()
    }
    async fn json(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let r = self.request(method, path, body).await;
        let status = r.status();
        (status, r.json().await.unwrap())
    }
    #[cfg(target_os = "macos")]
    async fn card_run(&self, code: &str) -> Value {
        use lingxi_kernel::ports::StoragePort;
        let principal = self
            .state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", self.token)),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        let run = self
            .state
            .sessions()
            .execute_background_for(
                self.state.storage(),
                self.state.events(),
                self.state.runs(),
                self.state.background(),
                &principal,
                "card-session",
                &lingxi_service::ExecuteSubmission::plain(
                    &json!({"title":"真实互动卡","code":code}).to_string(),
                ),
                NOW,
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                for p in self
                    ._approvals
                    .pending_for_principal(&lingxi_kernel::Principal::LocalUser, "card-session")
                {
                    assert!(self
                        ._approvals
                        .answer_for_principal(
                            &lingxi_kernel::Principal::LocalUser,
                            "card-session",
                            &p.approval_id,
                            lingxi_service::approval_service::Answer::Approve
                        )
                        .settled());
                }
                if self
                    .state
                    .storage()
                    .load_run(&lingxi_protocol::RunId::new(&run.run_id))
                    .await
                    .unwrap()
                    .unwrap()
                    .status
                    .is_terminal()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        self._provider.result.lock().unwrap().clone().unwrap()
    }
    async fn close(self) -> PathBuf {
        self.state.loop_services().stop_idle_listener();
        self.state.loop_services().alarm.dispose();
        self.state.loop_services().controller.dispose();
        assert!(self
            .state
            .workflows()
            .stop(Duration::from_secs(5))
            .await
            .unwrap()
            .is_empty());
        assert!(self
            .state
            .background()
            .drain_within(Duration::from_secs(5))
            .await
            .unconfirmed
            .is_empty());
        let _ = self.stop.send(());
        self.server.await.unwrap();
        self.state.storage().close().await.unwrap();
        self.home
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_skill_paths_reject_bad_input_and_corrupt_preferences_without_mutation() {
    let h = Harness::boot().await;
    let prefs = h.home.join("user/preferences.json");
    let path = h.home.join("external");
    std::fs::create_dir_all(&path).unwrap();
    let original =
        json!({"external_skill_paths":[path],"goal":{"enabled":true},"unrelated":{"future":17}});
    std::fs::write(&prefs, original.to_string()).unwrap();
    for value in [
        json!([123]),
        json!([path, false]),
        Value::Null,
        json!("bad"),
    ] {
        let before = std::fs::read(&prefs).unwrap();
        let event = h.state.app_events().since(0);
        let memory = h.state.skills().get_external_paths();
        assert_eq!(
            h.json(
                "PUT",
                "/lingxi/v1/skills/external-paths",
                json!({"paths":value})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(std::fs::read(&prefs).unwrap(), before);
        assert_eq!(h.state.skills().get_external_paths(), memory);
        assert_eq!(h.state.app_events().since(0), event);
    }
    for bytes in [b"{broken".as_slice(), b"[]".as_slice()] {
        std::fs::write(&prefs, bytes).unwrap();
        let memory = h.state.skills().get_external_paths();
        let events = h.state.app_events().since(0);
        assert_eq!(
            h.json(
                "PUT",
                "/lingxi/v1/skills/external-paths",
                json!({"paths":[path]})
            )
            .await
            .0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(std::fs::read(&prefs).unwrap(), bytes);
        assert_eq!(h.state.skills().get_external_paths(), memory);
        assert_eq!(h.state.app_events().since(0), events);
    }
    std::fs::remove_file(&prefs).unwrap();
    std::fs::create_dir(&prefs).unwrap();
    assert_eq!(
        h.json(
            "PUT",
            "/lingxi/v1/skills/external-paths",
            json!({"paths":[path]})
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(prefs.is_dir());
    std::fs::remove_dir(&prefs).unwrap();
    std::fs::write(&prefs, original.to_string()).unwrap();
    assert_eq!(
        h.json(
            "PUT",
            "/lingxi/v1/skills/external-paths",
            json!({"paths":[]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(&prefs).unwrap()).unwrap();
    assert_eq!(saved["external_skill_paths"], json!([]));
    assert_eq!(saved["unrelated"], original["unrelated"]);
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn external_paths_and_other_preferences_share_real_read_merge_write_and_restart() {
    let h = Harness::boot().await;
    let prefs = h.home.join("user/preferences.json");
    std::fs::write(&prefs, json!({"unrelated":{"future":"保留"}}).to_string()).unwrap();
    let path = h.home.join("external");
    std::fs::create_dir(&path).unwrap();
    for n in 0..16 {
        let (a, b) = tokio::join!(
            h.json(
                "PUT",
                "/lingxi/v1/skills/external-paths",
                json!({"paths":[path]})
            ),
            h.json(
                "PUT",
                "/lingxi/v1/preferences/goal",
                json!({"enabled":false,"default_token_budget":1000+n})
            )
        );
        assert_eq!(a.0, StatusCode::OK, "{}", a.1);
        assert_eq!(b.0, StatusCode::OK, "{}", b.1);
        let saved: Value = serde_json::from_slice(&std::fs::read(&prefs).unwrap()).unwrap();
        assert_eq!(saved["external_skill_paths"], json!([path]));
        assert_eq!(saved["goal"]["default_token_budget"], 1000 + n);
        assert_eq!(saved["unrelated"]["future"], "保留");
    }
    let home = h.close().await;
    let h = Harness::boot_home(home).await;
    assert_eq!(
        h.json("GET", "/lingxi/v1/skills/external-paths", Value::Null)
            .await
            .1["configured"],
        json!([path])
    );
    let (status, goal) = h
        .json("GET", "/lingxi/v1/preferences/goal", Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK, "{goal}");
    assert_eq!(goal["goal"]["default_token_budget"], 1015);
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn card_http_is_bounded_local_only_sandboxed_and_rebuilt_after_real_restart() {
    let h = Harness::boot().await;
    let path = "/lingxi/v1/cards/c_real";
    let code = "<button onclick=\"this.textContent='已点击'\">实际卡片</button>";
    let (status, registered) = h
        .json(
            "PUT",
            path,
            json!({"code":code,"varsCss":"--accent:red;</style><script>escape</script>"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let frame = registered["frameUrl"].as_str().unwrap();
    assert!(!frame.contains(&h.token));
    assert!(!frame.contains('?'));
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{frame}", h.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        reqwest::Client::new()
            .head(format!("{}{frame}", h.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}/lingxi/v1/sessions", h.url))
            .bearer_auth(frame.rsplit('/').next().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED,
        "卡片能力不能作为整机凭证访问其他接口"
    );
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{}", h.url, frame.replace("c_real/", "other/")))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = h.request("GET", path, Value::Null).await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let html = response.text().await.unwrap();
    assert!(html.contains(code));
    assert!(!html.contains("</style><script>escape"));
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    let csp = headers["content-security-policy"].to_str().unwrap();
    for part in [
        "sandbox allow-scripts",
        "connect-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
        "frame-src 'none'",
    ] {
        assert!(csp.contains(part), "{csp}");
    }
    // 原通用接口仍受原权限限制；框架只使用短期卡片能力，不附整机凭证。
    h.clock.advance(120_000);
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{frame}", h.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let (_, renewed) = h
        .json(
            "PUT",
            path,
            json!({"code":code,"varsCss":"--accent:red;</style><script>escape</script>"}),
        )
        .await;
    let replaced = renewed["frameUrl"].as_str().unwrap();
    assert_eq!(
        h.json("PUT", path, json!({"code":"<p>新版本</p>"})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{replaced}", h.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.json("PUT", path, json!({"code":code})).await.0,
        StatusCode::OK
    );
    // 改回旧代码也不得复活旧票据。
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{replaced}", h.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let (_, first_grant) = h.json("PUT", path, json!({"code":code})).await;
    let first_grant = first_grant["frameUrl"].as_str().unwrap().to_owned();
    for _ in 0..8 {
        assert_eq!(
            h.json("PUT", path, json!({"code":code})).await.0,
            StatusCode::OK
        );
    }
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{first_grant}", h.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "单卡旧票据必须有界回收"
    );
    let device = h
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["chat", "settings.read"],
            None,
        )
        .unwrap();
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{path}", h.url))
            .bearer_auth(&device.secret)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "不能从本机owner扩权给聊天设备"
    );
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{path}", h.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for (body, status) in [
        (json!({"code":"ok","varsCss":null}), StatusCode::BAD_REQUEST),
        (json!({"code":"ok","varsCss":12}), StatusCode::BAD_REQUEST),
        (
            json!({"code":"x".repeat(512*1024+1)}),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        assert_eq!(h.json("PUT", path, body).await.0, status);
    }
    assert!(h
        .request("GET", path, Value::Null)
        .await
        .text()
        .await
        .unwrap()
        .contains(code));
    for n in 0..256 {
        if n % 64 == 0 {
            h.clock.advance(61_000);
        }
        assert_eq!(
            h.json(
                "PUT",
                &format!("/lingxi/v1/cards/lru_{n}"),
                json!({"code":"<p>LRU</p>"})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        h.request("GET", path, Value::Null).await.status(),
        StatusCode::NOT_FOUND
    );
    let (_, last) = h
        .json("PUT", "/lingxi/v1/cards/lru_255", json!({"code":"restart"}))
        .await;
    let last = last["frameUrl"].as_str().unwrap().to_owned();
    let home = h.close().await;
    let h = Harness::boot_home(home).await;
    assert_eq!(
        reqwest::Client::new()
            .get(format!("{}{last}", h.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.request("GET", "/lingxi/v1/cards/lru_255", Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.json("PUT", path, json!({"code":code})).await.0,
        StatusCode::OK
    );
    assert!(h
        .request("GET", path, Value::Null)
        .await
        .text()
        .await
        .unwrap()
        .contains(code));
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_run_show_card_reaches_real_component_registration_and_http_document() {
    let h = Harness::boot().await;
    let outcome = h
        .card_run(
            "<button id=\"actual-card\" onclick=\"this.textContent='done'\">真实Run卡片</button>",
        )
        .await;
    assert_eq!(outcome["ok"], true, "{outcome}");
    let card = outcome["details"].clone();
    assert!(card["cardId"].as_str().unwrap().starts_with("c_"));
    // 真组件在独立Node进程连接此真实Rust服务，不能用mock fetch伪造登记。
    let url = h.url.clone();
    let token = h.token.clone();
    let serialized = card.to_string();
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let home = h.home.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .arg("node_modules/vitest/vitest.mjs")
            .args([
                "run",
                "desktop/src/react/__tests__/components/r07-t08-card-real-service.test.tsx",
                "--maxWorkers=1",
            ])
            .current_dir(cwd)
            .env("LINGXI_R07_CARD_HOME", home)
            .env("LINGXI_R07_CARD_URL", url)
            .env("LINGXI_R07_CARD_TOKEN", token)
            .env("LINGXI_R07_CARD_BLOCK", serialized)
            .env("LINGXI_R07_CARD_GUARD", "1")
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "实际卡片组件未通过：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let id = card["cardId"].as_str().unwrap();
    let html = h
        .request("GET", &format!("/lingxi/v1/cards/{id}"), Value::Null)
        .await;
    assert_eq!(html.status(), StatusCode::OK);
    assert!(html.text().await.unwrap().contains("真实Run卡片"));
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn character_card_default_zip_contains_no_configuration_secrets_and_bad_zip_creates_no_agent()
{
    use base64::Engine;
    use std::io::{Read, Write};
    let h = Harness::boot().await;
    let sentinels = [
        "r07-synthetic-search-secret-never-export",
        "r07-synthetic-provider-secret-never-export",
    ];
    std::fs::write(
        h.home.join("user/preferences.json"),
        json!({"search":{"api_keys":{"brave":sentinels[0]}},"future":{"keep":true}}).to_string(),
    )
    .unwrap();
    std::fs::write(h.home.join("models.json"),json!({"providers":{"fixture":{"baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","apiKey":sentinels[1],"models":[]}}}).to_string()).unwrap();
    std::fs::write(h.home.join("agents/a/avatar.png"),base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aV5kAAAAASUVORK5CYII=").unwrap()).unwrap();
    let (status, result) = h
        .json(
            "POST",
            "/lingxi/v1/character-cards/export",
            json!({"agentId":"a"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let package = PathBuf::from(result["filePath"].as_str().unwrap());
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&package).unwrap()).unwrap();
    let mut descriptor = None;
    for n in 0..zip.len() {
        let mut entry = zip.by_index(n).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        for secret in sentinels {
            assert!(
                !bytes.windows(secret.len()).any(|b| b == secret.as_bytes()),
                "默认导出带出合成秘密"
            );
        }
        if entry.name().unwrap().as_ref() == "card.json" {
            descriptor = Some(serde_json::from_slice::<Value>(&bytes).unwrap());
        }
    }
    let descriptor = descriptor.unwrap();
    assert!(descriptor.get("memory").is_none());
    assert!(descriptor.get("providers").is_none());
    let mut before: Vec<_> = std::fs::read_dir(h.home.join("agents"))
        .unwrap()
        .map(|r| r.unwrap().file_name())
        .collect();
    let (status, planned) = h
        .json(
            "POST",
            "/lingxi/v1/character-cards/plan",
            json!({"path":package}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let token = planned["plan"]["token"].as_str().unwrap();
    let staged = h
        .home
        .join(".ephemeral/character-card-imports")
        .join(token)
        .join("package")
        .join(descriptor["assets"]["avatar"].as_str().unwrap());
    assert!(staged.is_file());
    std::fs::write(&staged, b"corrupt-image-material").unwrap();
    assert!(
        h.json(
            "POST",
            "/lingxi/v1/character-cards/import",
            json!({"token":token})
        )
        .await
        .0
        .is_client_error(),
        "真实预览后损坏的素材须在创建助手前拒绝"
    );
    let bad = h.home.join("workspace/escape.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&bad).unwrap());
    writer
        .start_file(
            "../must-not-create",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(b"bad").unwrap();
    writer.finish().unwrap();
    assert!(h
        .json(
            "POST",
            "/lingxi/v1/character-cards/plan",
            json!({"path":bad})
        )
        .await
        .0
        .is_client_error());
    assert!(h
        .json(
            "POST",
            "/lingxi/v1/character-cards/import",
            json!({"token":"broken-token"})
        )
        .await
        .0
        .is_client_error());
    let mut after: Vec<_> = std::fs::read_dir(h.home.join("agents"))
        .unwrap()
        .map(|r| r.unwrap().file_name())
        .collect();
    before.sort();
    after.sort();
    assert_eq!(before, after);
    assert!(!h.home.join("must-not-create").exists());
    let home = h.close().await;
    std::fs::remove_dir_all(home).unwrap();
}
