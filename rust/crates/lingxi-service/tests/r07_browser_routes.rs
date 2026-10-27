//! 仅替换受控浏览器外部响应；HTTP、身份、数据库、会话管理与正式命令收据均使用生产链。
use lingxi_adapters::storage::{browser as store, session_tree};
use lingxi_kernel::browser::{
    BrowserDownload, BrowserError, BrowserFuture, BrowserOutput, BrowserPort, BrowserScope,
};
use lingxi_protocol::{
    BrowserAction, BrowserCommand, BrowserOpenBehavior, BrowserReply, BrowserSettings, BrowserTab,
    BrowserWorkspace,
};
use lingxi_service::{
    inject::ManualClock, prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const NOW: u64 = 1_800_000_000_000;

fn screenshot_png() -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jD1EAAAAASUVORK5CYII=")
        .unwrap()
}

#[derive(Default)]
struct ExternalBrowser {
    states: Mutex<BTreeMap<String, BrowserWorkspace>>,
    commands: Mutex<Vec<(BrowserScope, BrowserCommand)>>,
    revokes: Mutex<Vec<BrowserScope>>,
    forgotten: Mutex<Vec<BrowserScope>>,
    restored: Mutex<Vec<(BrowserScope, BrowserWorkspace)>>,
    settings: Mutex<Vec<BrowserSettings>>,
    configure_failures: AtomicUsize,
    clear_calls: AtomicUsize,
    clear_failed: AtomicBool,
    revoke_unknown: AtomicBool,
    resume_stays_cold: AtomicBool,
    download_pending: AtomicBool,
    download_ack_failures: AtomicUsize,
    download_ack_calls: AtomicUsize,
    shutdown_failure: AtomicBool,
    shutdown_panic: AtomicBool,
    shutdown_calls: AtomicUsize,
}
fn browser_error(code: &str, unknown: bool) -> BrowserError {
    BrowserError {
        code: code.into(),
        message: "外部浏览器替身返回明确测试结果".into(),
        effect_unknown: unknown,
    }
}
fn workspace(scope: &BrowserScope) -> BrowserWorkspace {
    BrowserWorkspace {
        session_id: scope.session_id.clone(),
        session_path: format!("native:{}", scope.session_id),
        execution_node: scope.execution_node.clone(),
        running: false,
        resumable: false,
        unavailable_reason: None,
        url: None,
        title: None,
        active_tab_id: None,
        tabs: vec![],
        suspended: false,
        user_takeover: false,
        revoked: false,
        visible_capable: true,
    }
}
impl BrowserPort for ExternalBrowser {
    fn execute<'a>(
        &'a self,
        scope: BrowserScope,
        command: BrowserCommand,
        cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, BrowserOutput> {
        Box::pin(async move {
            if cancel.load(Ordering::Acquire) {
                return Err(browser_error("BROWSER_CANCELLED", false));
            }
            self.commands
                .lock()
                .unwrap()
                .push((scope.clone(), command.clone()));
            let mut states = self.states.lock().unwrap();
            let state = states
                .entry(scope.session_id.clone())
                .or_insert_with(|| workspace(&scope));
            if command.action == BrowserAction::Resume
                && self.resume_stays_cold.load(Ordering::Acquire)
            {
                return Ok(BrowserOutput {
                    reply: BrowserReply {
                        state: state.clone(),
                        result: json!({"externalConfirmed":true}),
                        files: vec![],
                        downloads: vec![],
                    },
                    screenshot: None,
                    downloads: vec![],
                });
            }
            match command.action {
                BrowserAction::Start
                | BrowserAction::Resume
                | BrowserAction::Navigate
                | BrowserAction::NewTab => {
                    state.running = true;
                    state.revoked = false;
                    if state.tabs.is_empty() || command.action == BrowserAction::NewTab {
                        let id = format!("external-tab-{}", state.tabs.len() + 1);
                        let url = command.url.clone().unwrap_or_else(|| "about:blank".into());
                        state.tabs.push(BrowserTab {
                            tab_id: id.clone(),
                            title: "正式外部响应".into(),
                            url: Some(url),
                            can_go_back: false,
                            can_go_forward: false,
                            created_at: "2026-10-16T00:00:00Z".into(),
                            updated_at: "2026-10-16T00:00:00Z".into(),
                        });
                        state.active_tab_id = Some(id);
                    }
                    if let Some(url) = command.url {
                        state.url = Some(url.clone());
                        if let Some(tab) = state.tabs.last_mut() {
                            tab.url = Some(url);
                        }
                    } else {
                        state.url = state.tabs.first().and_then(|tab| tab.url.clone());
                    }
                    state.title = Some("正式外部响应".into());
                    state.resumable = true;
                }
                BrowserAction::Stop | BrowserAction::Destroy => {
                    state.running = false;
                    state.user_takeover = false;
                }
                BrowserAction::Takeover => state.user_takeover = true,
                BrowserAction::Release => state.user_takeover = false,
                _ => {}
            }
            Ok(BrowserOutput {
                reply: BrowserReply {
                    state: state.clone(),
                    result: json!({"externalConfirmed":true}),
                    files: vec![],
                    downloads: vec![],
                },
                screenshot: (command.action == BrowserAction::Screenshot).then(screenshot_png),
                downloads: if self.download_pending.load(Ordering::Acquire) {
                    vec![BrowserDownload {
                        id: "owned-download-guid".into(),
                        suggested_filename: "report.txt".into(),
                        bytes: Some(b"actual browser download".to_vec()),
                        completed: true,
                    }]
                } else {
                    vec![]
                },
            })
        })
    }
    fn acknowledge_downloads<'a>(
        &'a self,
        _scope: BrowserScope,
        ids: Vec<String>,
    ) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            if ids.is_empty() {
                return Ok(());
            }
            assert_eq!(ids, ["owned-download-guid"]);
            self.download_ack_calls.fetch_add(1, Ordering::AcqRel);
            if self
                .download_ack_failures
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
                .is_ok()
            {
                return Err(browser_error("BROWSER_ACK_UNKNOWN", true));
            }
            self.download_pending.store(false, Ordering::Release);
            Ok(())
        })
    }
    fn workspaces(&self) -> Result<Vec<BrowserWorkspace>, BrowserError> {
        Ok(self.states.lock().unwrap().values().cloned().collect())
    }
    fn restore(&self, scope: BrowserScope, state: BrowserWorkspace) -> Result<(), BrowserError> {
        self.restored
            .lock()
            .unwrap()
            .push((scope.clone(), state.clone()));
        self.states.lock().unwrap().insert(scope.session_id, state);
        Ok(())
    }
    fn revoke<'a>(&'a self, scope: BrowserScope) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            self.revokes.lock().unwrap().push(scope.clone());
            if let Some(state) = self.states.lock().unwrap().get_mut(&scope.session_id) {
                state.running = false;
                state.user_takeover = false;
                state.revoked = true;
            }
            if self.revoke_unknown.load(Ordering::Acquire) {
                return Err(browser_error("BROWSER_REVOKE_UNKNOWN", true));
            }
            Ok(())
        })
    }
    fn configure<'a>(&'a self, settings: BrowserSettings) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            // 模拟外部状态已经部分改变、但本次确认失败。
            self.settings.lock().unwrap().push(settings);
            let failed = self
                .configure_failures
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                    Some(left.saturating_sub(1))
                })
                .unwrap();
            if failed > 0 {
                return Err(browser_error("BROWSER_CONFIGURE_FAILED", false));
            }
            Ok(())
        })
    }
    fn forget<'a>(&'a self, scope: BrowserScope) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            self.forgotten.lock().unwrap().push(scope.clone());
            self.states.lock().unwrap().remove(&scope.session_id);
            Ok(())
        })
    }
    fn clear_site_data<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            self.clear_calls.fetch_add(1, Ordering::AcqRel);
            if self.clear_failed.load(Ordering::Acquire) {
                return Err(browser_error("BROWSER_CLEAR_FAILED", false));
            }
            Ok(())
        })
    }
    fn read_auxiliary_page<'a>(
        &'a self,
        _: String,
        _: String,
        _: Option<String>,
        _: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, Value> {
        Box::pin(async { Err(browser_error("BROWSER_TEST_UNUSED", false)) })
    }
    fn reap_idle<'a>(&'a self, _: u64) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn shutdown<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            self.shutdown_calls.fetch_add(1, Ordering::AcqRel);
            assert!(
                !self.shutdown_panic.load(Ordering::Acquire),
                "外部宿主退出异常测试"
            );
            if self.shutdown_failure.load(Ordering::Acquire) {
                return Err(browser_error("BROWSER_CLEANUP_UNKNOWN", true));
            }
            for state in self.states.lock().unwrap().values_mut() {
                state.running = false;
            }
            Ok(())
        })
    }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    port: Arc<ExternalBrowser>,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot(existing: Option<PathBuf>) -> Self {
        let home = existing.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-r07-browser-routes-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::write(
            home.join("agents/lingxi/config.yaml"),
            "agent:\n  name: 合成助手\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        if !home.join("user/preferences.json").exists() {
            std::fs::write(
                home.join("user/preferences.json"),
                json!({"primary_agent_id":"lingxi","current_agent_id":"lingxi","unrelated":{"keep":7},"browser":{"acceptCookies":false,"agentOpenBehavior":"new_tab","futureSetting":{"keep":8}}}).to_string(),
            ).unwrap();
        }
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let port = Arc::new(ExternalBrowser::default());
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 2000,
            },
            &layout,
            ServiceDeps {
                clock: Arc::new(ManualClock::new(NOW)),
                browser_port: Some(port.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let router = lingxi_service::build_router(state.clone());
        let server = tokio::spawn(async move {
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
        Self {
            home,
            state,
            port,
            url,
            token,
            stop,
            server,
        }
    }
    async fn request_as(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(token)
            .timeout(Duration::from_secs(8));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw":text})),
        )
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        self.request_as(&self.token, method, path, body).await
    }
    async fn post(&self, path: &str, body: Value) -> Value {
        let (status, value) = self.request(reqwest::Method::POST, path, Some(body)).await;
        assert!((200..300).contains(&status), "HTTP {status}: {value}");
        value
    }
    async fn get(&self, path: &str) -> Value {
        let (status, value) = self.request(reqwest::Method::GET, path, None).await;
        assert_eq!(status, 200, "{value}");
        value
    }
    async fn session(&self, sid: &str) {
        self.post("/lingxi/v1/sessions", json!({"sessionId":sid,"agentId":"lingxi","title":"浏览器真实会话","permissionMode":"operate"})).await;
    }
    async fn command(&self, sid: &str, id: &str, action: &str) -> Value {
        self.post(
            "/lingxi/v1/browser/command",
            json!({"sessionPath":format!("native:{sid}"),"requestId":id,"action":action}),
        )
        .await
    }
    async fn device(&self, scopes: &[&str]) -> String {
        self.post(
            "/lingxi/v1/devices/credentials",
            json!({"userId":"user_local","scopes":scopes}),
        )
        .await["secret"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn close(self, delete: bool) -> PathBuf {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), self.server)
            .await
            .unwrap()
            .unwrap();
        self.state.browsers().shutdown().await.unwrap();
        self.state.storage().close().await.unwrap();
        let home = self.home.clone();
        drop(self.state);
        if delete {
            std::fs::remove_dir_all(&home).unwrap();
        }
        home
    }
}

#[tokio::test]
async fn actual_http_command_replay_changed_input_and_emergency_receipt_never_repeat_external_actions(
) {
    let h = Harness::boot(None).await;
    h.session("s").await;
    let original = h.command("s", "stable-tab", "new_tab").await;
    let replay = h.command("s", "stable-tab", "new_tab").await;
    assert_eq!(original, replay);
    assert_eq!(h.port.commands.lock().unwrap().len(), 1);
    let (changed, _) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/browser/command",
            Some(json!({"sessionPath":"native:s","requestId":"stable-tab","action":"reload"})),
        )
        .await;
    assert!(!(200..300).contains(&changed));
    assert_eq!(h.port.commands.lock().unwrap().len(), 1);
    let stop = json!({"sessionPath":"native:s","requestId":"stable-stop","revoke":true});
    h.post("/lingxi/v1/browser/close-session", stop.clone())
        .await;
    h.post("/lingxi/v1/browser/close-session", stop).await;
    assert_eq!(h.port.revokes.lock().unwrap().len(), 1);
    let authority = store::get(h.state.storage(), "s").await.unwrap().unwrap();
    assert!(authority.revoked);
    assert!(authority.epoch > 1);
    let (denied, body) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/browser/command",
            Some(json!({"sessionPath":"native:s","requestId":"after-revoke","action":"new_tab"})),
        )
        .await;
    assert_eq!(denied, 403, "{body}");
    assert_eq!(h.port.commands.lock().unwrap().len(), 1);
    let rows = h.get("/lingxi/v1/browser/workspaces").await;
    assert_eq!(rows["workspaces"][0]["revoked"], true);
    assert_eq!(rows["workspaces"][0]["running"], false);
    h.close(true).await;
}

#[tokio::test]
async fn chat_device_reads_state_but_only_explicit_remote_admin_controls_the_actual_service_node() {
    let h = Harness::boot(None).await;
    h.session("s").await;
    h.command("s", "start", "new_tab").await;
    let node = h.get("/lingxi/v1/me").await["serverId"]
        .as_str()
        .unwrap()
        .to_string();
    let chat = h.device(&["chat"]).await;
    let (read, states) = h
        .request_as(
            &chat,
            reqwest::Method::GET,
            "/lingxi/v1/browser/session-states",
            None,
        )
        .await;
    assert_eq!(read, 200, "{states}");
    assert_eq!(states["native:s"]["executionNode"], node);
    for (path, body) in [
        (
            "/lingxi/v1/browser/command",
            json!({"sessionPath":"native:s","requestId":"chat-command","action":"takeover"}),
        ),
        (
            "/lingxi/v1/browser/open-session",
            json!({"sessionPath":"native:s","requestId":"chat-open"}),
        ),
        (
            "/lingxi/v1/browser/close-session",
            json!({"sessionPath":"native:s","requestId":"chat-close","revoke":true}),
        ),
    ] {
        let (status, result) = h
            .request_as(&chat, reqwest::Method::POST, path, Some(body))
            .await;
        assert_eq!(status, 403, "{path}: {result}");
    }
    assert_eq!(h.port.commands.lock().unwrap().len(), 1);
    assert!(h.port.revokes.lock().unwrap().is_empty());
    let admin = h.device(&["chat", "admin"]).await;
    let (status, result) = h
        .request_as(
            &admin,
            reqwest::Method::POST,
            "/lingxi/v1/browser/command",
            Some(json!({"sessionPath":"native:s","requestId":"admin-show","action":"show"})),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["state"]["executionNode"], node);
    {
        let calls = h.port.commands.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].0.execution_node, node);
        assert!(calls[1].0.human);
    }
    let (prefs, _) = h
        .request_as(
            &admin,
            reqwest::Method::GET,
            "/lingxi/v1/preferences/browser",
            None,
        )
        .await;
    assert_eq!(prefs, 403, "全局设置仍只接受本机所有者");
    let (forged, _) = h.request_as(&admin,reqwest::Method::POST,"/lingxi/v1/browser/command",Some(json!({"sessionPath":"native:s","requestId":"forged-node","action":"show","executionNode":"attacker-device"}))).await;
    assert_eq!(forged, 400);
    assert_eq!(h.port.commands.lock().unwrap().len(), 2);
    h.close(true).await;
}

#[tokio::test]
async fn archive_and_permanent_delete_reclaim_only_the_actual_session_browser_before_success() {
    let h = Harness::boot(None).await;
    for sid in ["archived", "independent"] {
        h.session(sid).await;
        h.command(sid, sid, "new_tab").await;
    }
    h.post(
        "/lingxi/v1/sessions/archive",
        json!({"sessionId":"archived"}),
    )
    .await;
    assert_eq!(
        h.port
            .revokes
            .lock()
            .unwrap()
            .iter()
            .map(|scope| scope.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["archived"]
    );
    assert!(!h.port.states.lock().unwrap()["archived"].running);
    assert!(h.port.states.lock().unwrap()["independent"].running);
    assert!(h.port.forgotten.lock().unwrap().is_empty());
    let rows = h.get("/lingxi/v1/browser/workspaces").await;
    assert_eq!(rows["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(rows["workspaces"][0]["sessionId"], "independent");
    h.post(
        "/lingxi/v1/sessions/archived/delete",
        json!({"sessionId":"archived"}),
    )
    .await;
    assert!(session_tree::get_session_row(h.state.storage(), "archived")
        .await
        .unwrap()
        .is_none());
    assert!(store::get(h.state.storage(), "archived")
        .await
        .unwrap()
        .is_none());
    assert_eq!(h.port.revokes.lock().unwrap().len(), 1);
    {
        let purged = h.port.forgotten.lock().unwrap();
        assert_eq!(purged.len(), 1);
        assert_eq!(purged[0].session_id, "archived");
        let archived = h.port.revokes.lock().unwrap();
        assert_eq!(purged[0].owner_subject, archived[0].owner_subject);
        assert_eq!(purged[0].execution_node, archived[0].execution_node);
        assert!(purged[0].epoch >= archived[0].epoch);
    }
    assert!(!h.port.states.lock().unwrap().contains_key("archived"));
    assert!(h.port.states.lock().unwrap()["independent"].running);
    let (denied, _) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/browser/open-session",
            Some(json!({"sessionPath":"native:archived"})),
        )
        .await;
    assert!(!(200..300).contains(&denied));
    assert_eq!(h.port.commands.lock().unwrap().len(), 2);
    h.close(true).await;
}

#[tokio::test]
async fn restart_restores_cold_metadata_and_confirmed_receipt_without_automatically_opening_a_page()
{
    let h = Harness::boot(None).await;
    h.session("s").await;
    let body = json!({"sessionPath":"native:s","requestId":"persistent-navigation","action":"navigate","url":"https://fixture.invalid/saved"});
    let first = h.post("/lingxi/v1/browser/command", body.clone()).await;
    assert!(first["state"]["running"].as_bool().unwrap());
    let home = h.close(false).await;
    let next = Harness::boot(Some(home)).await;
    assert!(next.port.commands.lock().unwrap().is_empty());
    {
        let restored = next.port.restored.lock().unwrap();
        assert_eq!(restored.len(), 1);
        assert!(!restored[0].1.running);
        assert!(!restored[0].1.user_takeover);
        assert!(restored[0].1.resumable);
    }
    let states = next.get("/lingxi/v1/browser/workspaces").await;
    assert_eq!(states["workspaces"][0]["running"], false);
    assert_eq!(states["workspaces"][0]["resumable"], true);
    assert_eq!(
        next.get("/lingxi/v1/browser/sessions").await["native:s"],
        "https://fixture.invalid/saved",
        "冷恢复的旧网址仍在浏览器目录中，但读取目录不能启动页面"
    );
    assert_eq!(next.post("/lingxi/v1/browser/command", body).await, first);
    assert!(
        next.port.commands.lock().unwrap().is_empty(),
        "重放确认收据不能再派发外部动作"
    );
    next.port.resume_stays_cold.store(true, Ordering::Release);
    let attempted = next
        .post(
            "/lingxi/v1/browser/open-session",
            json!({"sessionPath":"native:s","requestId":"explicit-cold-attempt"}),
        )
        .await;
    assert_eq!(attempted["resume"]["status"], "attempted");
    assert_eq!(attempted["resume"]["found"], false);
    assert_eq!(attempted["resume"]["running"], false);
    next.port.resume_stays_cold.store(false, Ordering::Release);
    let resumed = next
        .post(
            "/lingxi/v1/browser/open-session",
            json!({"sessionPath":"native:s","requestId":"explicit-resume"}),
        )
        .await;
    assert_eq!(resumed["resume"]["running"], true);
    assert_eq!(next.port.commands.lock().unwrap().len(), 2);
    next.close(true).await;
}

#[tokio::test]
async fn screenshot_registers_a_real_png_session_file_and_http_content_preserves_its_type_and_bytes(
) {
    let h = Harness::boot(None).await;
    h.session("screenshot").await;
    h.command("screenshot", "open-for-shot", "new_tab").await;
    let reply = h.command("screenshot", "take-shot", "screenshot").await;
    let decoded: BrowserReply =
        serde_json::from_value(reply.clone()).expect("真实接口符合统一回复定义");
    assert_eq!(decoded.files.len(), 1);
    let file = &reply["files"][0];
    let filename = file["filename"].as_str().unwrap();
    assert!(filename.starts_with("browser-screenshot"));
    assert!(filename.ends_with(".png"));
    assert_eq!(file["mime"], "image/png");
    assert_eq!(file["sizeBytes"], screenshot_png().len());
    let id = file["fileId"].as_str().unwrap();
    let registered = h
        .get(&format!("/lingxi/v1/sessions/screenshot/files/{id}"))
        .await;
    assert_eq!(registered["file"], *file);
    let content = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/resources/res_{id}/content", h.url))
        .bearer_auth(&h.token)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .unwrap();
    assert_eq!(content.status(), reqwest::StatusCode::OK);
    assert_eq!(
        content.headers()[reqwest::header::CONTENT_TYPE],
        "image/png"
    );
    assert!(content.headers()[reqwest::header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains(filename));
    let bytes = content.bytes().await.unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(bytes.as_ref(), screenshot_png());
    h.close(true).await;
}

#[tokio::test]
async fn concurrent_and_repeated_shutdown_waiters_preserve_failure_and_panic_results() {
    for panic in [false, true] {
        let h = Harness::boot(None).await;
        h.port.shutdown_failure.store(!panic, Ordering::Release);
        h.port.shutdown_panic.store(panic, Ordering::Release);
        let host = h.state.browsers();
        host.start();
        host.stop();
        let (first, second) = tokio::join!(host.wait_stopped(), host.wait_stopped());
        assert!(first.is_err());
        assert_eq!(first, second);
        assert_eq!(host.wait_stopped().await, first);
        host.start();
        assert_eq!(
            host.wait_stopped().await,
            first,
            "结束的宿主不能重新启动来抹掉退出事实"
        );
        assert_eq!(h.port.shutdown_calls.load(Ordering::Acquire), 1);
        h.port.shutdown_failure.store(false, Ordering::Release);
        h.port.shutdown_panic.store(false, Ordering::Release);
        h.close(true).await;
    }
}

#[tokio::test]
async fn repeated_download_after_unknown_ack_reuses_actual_delivered_file_and_receipt() {
    let h = Harness::boot(None).await;
    h.session("download").await;
    h.command("download", "open-download", "new_tab").await;
    h.port.download_pending.store(true, Ordering::Release);
    h.port.download_ack_failures.store(1, Ordering::Release);
    let first = json!({"sessionPath":"native:download","requestId":"download-ack-unknown","action":"snapshot"});
    let (status, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/browser/command",
            Some(first.clone()),
        )
        .await;
    assert_eq!(status, 409, "{result}");
    assert_eq!(result["effectUnknown"], true);
    let initial = h.get("/lingxi/v1/sessions/download/files").await;
    let initial_files = initial["files"].as_array().unwrap();
    assert_eq!(initial_files.len(), 1);
    let file_id = initial_files[0]["fileId"].as_str().unwrap();
    let before = h.port.commands.lock().unwrap().len();
    let (replayed, _) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/browser/command",
            Some(first),
        )
        .await;
    assert_eq!(replayed, 409);
    assert_eq!(h.port.commands.lock().unwrap().len(), before);
    let second = h
        .command("download", "confirm-pending-download", "snapshot")
        .await;
    assert_eq!(second["downloads"][0]["fileId"], file_id);
    assert_eq!(second["downloads"][0]["status"], "completed");
    assert_eq!(second["files"][0]["fileId"], file_id);
    assert_eq!(
        h.get("/lingxi/v1/sessions/download/files").await["files"],
        initial["files"]
    );
    assert_eq!(h.port.download_ack_calls.load(Ordering::Acquire), 2);
    let receipt = h.state.storage().query_one_text(
        "SELECT file_id FROM browser_downloads_v1 WHERE session_id=?1 AND download_id=?2 AND status='completed'",
        vec!["download".into(), "owned-download-guid".into()]
    ).await.unwrap();
    assert_eq!(receipt.as_deref(), Some(file_id));
    let content = reqwest::Client::new()
        .get(format!(
            "{}/lingxi/v1/resources/res_{file_id}/content",
            h.url
        ))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();
    assert_eq!(content.status(), reqwest::StatusCode::OK);
    assert_eq!(
        content.bytes().await.unwrap().as_ref(),
        b"actual browser download"
    );
    h.close(true).await;
}

#[tokio::test]
async fn partial_preferences_preserve_unknown_fields_and_external_cookie_failure_is_not_success() {
    let h = Harness::boot(None).await;
    let (status, saved) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/browser",
            Some(json!({"browser":{"acceptCookies":true}})),
        )
        .await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(
        saved["browser"],
        json!({"acceptCookies":true,"agentOpenBehavior":"new_tab"})
    );
    let disk = serde_json::from_slice::<Value>(
        &std::fs::read(h.home.join("user/preferences.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(disk["browser"]["futureSetting"], json!({"keep":8}));
    assert_eq!(disk["unrelated"], json!({"keep":7}));
    assert_eq!(
        h.port
            .settings
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .agent_open_behavior,
        BrowserOpenBehavior::NewTab
    );
    let configured = h.port.settings.lock().unwrap().len();
    let (bad, _) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/browser",
            Some(json!({"browser":{"acceptCookies":false,"newUnapprovedField":true}})),
        )
        .await;
    assert_eq!(bad, 400);
    assert_eq!(h.port.settings.lock().unwrap().len(), configured);
    assert_eq!(
        serde_json::from_slice::<Value>(
            &std::fs::read(h.home.join("user/preferences.json")).unwrap()
        )
        .unwrap(),
        disk
    );
    h.port.clear_failed.store(true, Ordering::Release);
    let (failed, result) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/preferences/browser/clear-cookies",
            Some(json!({})),
        )
        .await;
    assert!(!(200..300).contains(&failed));
    assert_eq!(result["ok"], false);
    assert_eq!(result["code"], "BROWSER_CLEAR_FAILED");
    assert_eq!(h.port.clear_calls.load(Ordering::Acquire), 1);
    h.port.clear_failed.store(false, Ordering::Release);
    assert_eq!(
        h.post("/lingxi/v1/preferences/browser/clear-cookies", json!({}))
            .await["ok"],
        true
    );
    assert_eq!(h.port.clear_calls.load(Ordering::Acquire), 2);
    assert!(h.port.commands.lock().unwrap().is_empty());
    h.close(true).await;
}

#[tokio::test]
async fn unknown_external_revoke_receipt_never_repeats_the_stop_or_reopens_content() {
    let h = Harness::boot(None).await;
    h.session("s").await;
    h.command("s", "start", "new_tab").await;
    h.port.revoke_unknown.store(true, Ordering::Release);
    let stop = json!({"sessionPath":"native:s","requestId":"unknown-stop","revoke":true});
    for _ in 0..2 {
        let (status, body) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/browser/close-session",
                Some(stop.clone()),
            )
            .await;
        assert_eq!(status, 409, "{body}");
        assert_eq!(body["effectUnknown"], true);
    }
    assert_eq!(h.port.revokes.lock().unwrap().len(), 1);
    let (blocked, _) = h
        .request(
            reqwest::Method::POST,
            "/lingxi/v1/browser/command",
            Some(json!({"sessionPath":"native:s","requestId":"cannot-reopen","action":"new_tab"})),
        )
        .await;
    assert_eq!(blocked, 403);
    assert_eq!(h.port.commands.lock().unwrap().len(), 1);
    let receipt = h
        .state
        .storage()
        .query_one_text(
            "SELECT status FROM browser_commands_v1 WHERE input_json=?1",
            vec![json!({"action":"emergency_stop"}).to_string()],
        )
        .await
        .unwrap();
    assert_eq!(receipt.as_deref(), Some("unknown"));
    h.close(true).await;
}

#[tokio::test]
async fn partial_external_configuration_is_rolled_back_and_unconfirmed_rollback_is_explicit_unknown(
) {
    let h = Harness::boot(None).await;
    let disk = std::fs::read(h.home.join("user/preferences.json")).unwrap();
    let old = h.port.settings.lock().unwrap().last().unwrap().clone();
    assert!(!old.accept_cookies);
    let count = h.port.settings.lock().unwrap().len();
    h.port.configure_failures.store(1, Ordering::Release);
    let (failed, result) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/browser",
            Some(json!({"browser":{"acceptCookies":true}})),
        )
        .await;
    assert!(!(200..300).contains(&failed), "{result}");
    assert_eq!(result["ok"], false);
    assert_ne!(result["effectUnknown"], true);
    assert_eq!(
        std::fs::read(h.home.join("user/preferences.json")).unwrap(),
        disk
    );
    {
        let calls = h.port.settings.lock().unwrap();
        assert_eq!(calls.len(), count + 2);
        assert!(calls[count].accept_cookies);
        assert_eq!(calls[count + 1], old, "外部配置失败后必须恢复原有实际策略");
    }
    h.port.configure_failures.store(2, Ordering::Release);
    let (unknown, result) = h
        .request(
            reqwest::Method::PUT,
            "/lingxi/v1/preferences/browser",
            Some(json!({"browser":{"acceptCookies":true}})),
        )
        .await;
    assert_eq!(unknown, 409, "{result}");
    assert_eq!(result["ok"], false);
    assert_eq!(
        result["effectUnknown"], true,
        "回滚确认也失败，不能声称原策略已经恢复"
    );
    assert_eq!(
        std::fs::read(h.home.join("user/preferences.json")).unwrap(),
        disk
    );
    assert_eq!(h.port.settings.lock().unwrap().len(), count + 4);
    h.close(true).await;
}

/// R09-T05 逐断言：1809C8/367052/593534/178C8C——读取偏好与会话投影只读且真实；
/// 无效输入/未授权显式错误；普通关闭（revoke=false）走真实 Stop 并广播状态、不撤权。
#[tokio::test]
async fn preferences_and_session_projection_are_read_only_and_close_without_revoke_is_real() {
    let h = Harness::boot(None).await;
    h.session("s").await;
    h.command("s", "open-for-projection", "new_tab").await;
    let disk = std::fs::read(h.home.join("user/preferences.json")).unwrap();
    let settings_before = h.port.settings.lock().unwrap().len();
    let (status, prefs) = h
        .request(reqwest::Method::GET, "/lingxi/v1/preferences/browser", None)
        .await;
    assert_eq!(status, 200, "{prefs}");
    assert_eq!(prefs["browser"]["acceptCookies"], false);
    assert_eq!(prefs["browser"]["agentOpenBehavior"], "new_tab");
    assert_eq!(
        h.port.settings.lock().unwrap().len(),
        settings_before,
        "读取偏好不得写外部 BrowserManager 设置"
    );
    assert_eq!(
        std::fs::read(h.home.join("user/preferences.json")).unwrap(),
        disk,
        "读取偏好不得改磁盘偏好"
    );
    let node = h.get("/lingxi/v1/me").await["serverId"]
        .as_str()
        .unwrap()
        .to_string();
    let states = h.get("/lingxi/v1/browser/session-states").await;
    assert_eq!(states["native:s"]["running"], true);
    assert_eq!(states["native:s"]["executionNode"], node);
    assert!(states["native:s"]["url"].is_string());
    let sessions = h.get("/lingxi/v1/browser/sessions").await;
    assert!(sessions["native:s"].is_string(), "会话目录保留可恢复网址");
    // 无效/未授权输入必须显式错误，且不派发外部动作。
    let calls_before = h.port.commands.lock().unwrap().len();
    for body in [
        json!({"sessionPath":"native:missing","requestId":"missing-session","action":"new_tab"}),
        json!({"sessionPath":"not-a-resolvable-path","requestId":"bad-path","action":"new_tab"}),
        json!({"requestId":"no-session-path","action":"new_tab"}),
    ] {
        let (status, _) = h
            .request(
                reqwest::Method::POST,
                "/lingxi/v1/browser/command",
                Some(body),
            )
            .await;
        assert!(
            !(200..300).contains(&status),
            "无效输入必须显式失败（不呈现成功结果）"
        );
    }
    for path in [
        "/lingxi/v1/browser/open-session",
        "/lingxi/v1/browser/close-session",
    ] {
        for body in [
            json!({"sessionPath":"native:missing"}),
            json!({"sessionPath":"not-a-resolvable-path"}),
        ] {
            let (status, _) = h.request(reqwest::Method::POST, path, Some(body)).await;
            assert!(!(200..300).contains(&status), "{path} 无效输入必须显式失败");
        }
    }
    assert_eq!(
        h.port.commands.lock().unwrap().len(),
        calls_before,
        "无效输入不得派发外部动作"
    );
    // 普通关闭（revoke=false）：真实 Stop、状态投影 running=false、不撤权、重放不重复外部动作。
    let revokes_before = h.port.revokes.lock().unwrap().len();
    let close_body = json!({"sessionPath":"native:s","requestId":"close-no-revoke"});
    let closed = h
        .post("/lingxi/v1/browser/close-session", close_body.clone())
        .await;
    assert_eq!(closed["ok"], true, "{closed}");
    assert_eq!(closed["sessions"]["native:s"]["running"], false);
    assert_eq!(
        h.port.revokes.lock().unwrap().len(),
        revokes_before,
        "普通关闭不得误记为急停撤权"
    );
    {
        let calls = h.port.commands.lock().unwrap();
        assert_eq!(calls.last().unwrap().1.action, BrowserAction::Stop);
    }
    let external_before_replay = h.port.commands.lock().unwrap().len();
    let replay = h.post("/lingxi/v1/browser/close-session", close_body).await;
    assert_eq!(replay["ok"], true);
    assert_eq!(
        h.port.commands.lock().unwrap().len(),
        external_before_replay,
        "关闭重放只读原收据，不重复派发外部 Stop"
    );
    h.close(true).await;
}
