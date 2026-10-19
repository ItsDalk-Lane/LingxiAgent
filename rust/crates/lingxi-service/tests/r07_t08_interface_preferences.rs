//! 当前界面偏好与全局改前存档：真实服务、原存储、真实凭证与同home重启。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Server {
    home: PathBuf,
    state: ServiceState,
    addr: String,
    token: String,
    clock: Arc<lingxi_service::inject::ManualClock>,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Server {
    async fn boot(home: Option<PathBuf>) -> Self {
        let home = home.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-interface-prefs-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(&home).unwrap();
        let home = home.canonicalize().unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(lingxi_service::inject::ManualClock::new(1_800_000_000_000));
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
                clock: clock.clone(),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, addr) = tokio::sync::oneshot::channel();
        let running = state.clone();
        let task = tokio::spawn(async move {
            run(
                running,
                async {
                    let _ = rx.await;
                },
                |a| {
                    let _ = ready.send(a);
                },
                None,
            )
            .await
        });
        Self {
            home,
            state,
            addr: addr.await.unwrap().to_string(),
            token,
            clock,
            stop,
            task,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value, token: &str) -> (u16, Value) {
        self.raw(method, path, &body.to_string(), token).await
    }
    async fn raw(&self, method: &str, path: &str, body: &str, token: &str) -> (u16, Value) {
        let mut stream = tokio::net::TcpStream::connect(&self.addr).await.unwrap();
        let wire=format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let end = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
        let status = String::from_utf8_lossy(&bytes[..end])
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (status, serde_json::from_slice(&bytes[end + 4..]).unwrap())
    }
    fn prefs(&self) -> PathBuf {
        self.home.join("user/preferences.json")
    }
    async fn close(self) -> PathBuf {
        self.stop.send(()).unwrap();
        let outcome = self.task.await.unwrap().unwrap();
        assert!(!outcome.drain_timed_out, "服务退出未完成真实连接排空");
        self.home
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_keybindings_full_replace_sidebar_partial_merge_scopes_and_same_home_restart() {
    let s = Server::boot(None).await;
    let initial = json!({"unrelated":{"keep":"sentinel"},"search":{"api_keys":{"fixture":"owned-secret-never-return"}}});
    std::fs::write(s.prefs(), serde_json::to_vec(&initial).unwrap()).unwrap();
    let read = s
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.read"],
            None,
        )
        .unwrap();
    let write = s
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.write"],
            None,
        )
        .unwrap();
    let chat = s
        .state
        .auth()
        .issue_device_credential(lingxi_service::LOCAL_OWNER_USER_ID, &["chat"], None)
        .unwrap();
    let key = "/lingxi/v1/preferences/keybindings";
    let side = "/lingxi/v1/preferences/sidebar-ui";
    let (status, value) = s.request("GET", key, Value::Null, &read.secret).await;
    assert_eq!(status, 200);
    assert_eq!(value, json!({"keybindings":{}}));
    for (token, method) in [
        (&chat.secret, "GET"),
        (&chat.secret, "PUT"),
        (&read.secret, "PUT"),
    ] {
        assert_eq!(s.request(method, key, json!({}), token).await.0, 403);
    }
    assert_eq!(
        s.request("GET", side, Value::Null, &write.secret).await.0,
        403
    );
    let (status,normalized)=s.request("PUT",key,json!({"keybindings":{"app.new-session":[" ctrl + n ","Control+N","Alt+ ","Esc","Meta+M"],"quick-chat.toggle":["Alt+Space"],"voice.record-toggle":[],"not-a-command":["Secret"]}}),&write.secret).await;
    assert_eq!(status, 200, "{normalized}");
    assert_eq!(
        normalized,
        json!({"ok":true,"keybindings":{"app.new-session":["Control+N","Alt+Space","Escape"],"voice.record-toggle":[]}})
    );
    let typed: lingxi_protocol::KeybindingsUpdate = serde_json::from_value(normalized).unwrap();
    assert!(typed.ok);
    let (status, value) = s.request("PUT", key, json!({}), &write.secret).await;
    assert_eq!(status, 200);
    assert_eq!(value, json!({"ok":true,"keybindings":{}}));
    assert_eq!(
        s.request(
            "PUT",
            key,
            json!({"app.open-settings":"cmdorctrl+,"}),
            &write.secret
        )
        .await
        .1["keybindings"],
        json!({})
    );
    assert_eq!(
        s.request(
            "PUT",
            key,
            json!({"app.new-session":["Meta+N"]}),
            &write.secret
        )
        .await
        .0,
        200
    );
    let (status, defaults) = s.request("GET", side, Value::Null, &chat.secret).await;
    assert_eq!(status, 200);
    assert_eq!(
        defaults,
        json!({"sidebarUi":{"projectView":{"collapsedProjectIds":[],"collapsedFolderIds":[],"showAllProjectIds":[]},"sessionList":{"rowMode":"two-line"}}})
    );
    let (status,value)=s.request("PUT",side,json!({"sidebarUi":{"projectView":{"collapsedProjectIds":[" p1 ","p1",8,"p2"],"collapsedFolderIds":["f1"]},"sessionList":{"rowMode":"single-line"}}}),&chat.secret).await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(
        value["sidebarUi"]["projectView"]["collapsedProjectIds"],
        json!(["p1", "p2"])
    );
    assert_eq!(
        s.request(
            "PUT",
            side,
            json!({"projectView":{"showAllProjectIds":["p2"]}}),
            &chat.secret
        )
        .await
        .0,
        200
    );
    let candidates: Vec<String> = (0..260).map(|n| format!("project-{n}")).collect();
    let (status, limited) = s
        .request(
            "PUT",
            side,
            json!({"projectView":{"showAllProjectIds":candidates}}),
            &chat.secret,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        limited["sidebarUi"]["projectView"]["showAllProjectIds"]
            .as_array()
            .unwrap()
            .len(),
        256
    );
    assert_eq!(
        limited["sidebarUi"]["projectView"]["collapsedFolderIds"],
        json!(["f1"])
    );
    assert_eq!(
        s.request(
            "PUT",
            side,
            json!({"projectView":{"showAllProjectIds":["p2"]}}),
            &chat.secret
        )
        .await
        .0,
        200
    );
    let expected = json!({"projectView":{"collapsedProjectIds":["p1","p2"],"collapsedFolderIds":["f1"],"showAllProjectIds":["p2"]},"sessionList":{"rowMode":"single-line"}});
    assert_eq!(
        s.request("GET", side, Value::Null, &chat.secret).await.1["sidebarUi"],
        expected
    );
    let before = std::fs::read(s.prefs()).unwrap();
    for (path, bad) in [
        (key, json!({"keybindings":false})),
        (key, json!([])),
        (side, json!({"sidebarUi":null})),
        (side, json!({"sessionList":false})),
    ] {
        assert_eq!(s.request("PUT", path, bad, &s.token).await.0, 400);
        assert_eq!(std::fs::read(s.prefs()).unwrap(), before);
    }
    assert_eq!(s.raw("PUT", key, "{bad-json", &s.token).await.0, 400);
    assert!(s
        .state
        .auth()
        .revoke_device_credential(&write.credential_id)
        .unwrap());
    assert_eq!(s.request("PUT", key, json!({}), &write.secret).await.0, 401);
    assert!(s
        .state
        .auth()
        .revoke_device_credential(&chat.credential_id)
        .unwrap());
    assert_eq!(s.request("PUT", side, json!({}), &chat.secret).await.0, 401);
    assert_eq!(std::fs::read(s.prefs()).unwrap(), before);
    let root: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(root["unrelated"], initial["unrelated"]);
    assert_eq!(root["search"], initial["search"]);
    let home = s.close().await;
    let s = Server::boot(Some(home)).await;
    let (status, value) = s.request("GET", key, Value::Null, &s.token).await;
    assert_eq!(status, 200);
    assert_eq!(value["keybindings"], json!({"app.new-session":["Meta+N"]}));
    assert!(!value.to_string().contains("owned-secret"));
    assert_eq!(
        s.request("GET", side, Value::Null, &s.token).await.1["sidebarUi"],
        expected
    );
    let home = s.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_global_archives_return_only_existing_metadata_and_reject_corrupt_or_unsafe_files(
) {
    let s = Server::boot(None).await;
    let path = "/lingxi/v1/checkpoints";
    let (status, empty) = s.request("GET", path, Value::Null, &s.token).await;
    assert_eq!(status, 200);
    assert_eq!(empty, json!({"checkpoints":[]}));
    assert!(!s.home.join("checkpoints").exists());
    let read = s
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.read", "settings.write", "chat", "files.read"],
            None,
        )
        .unwrap();
    assert_eq!(
        s.request("GET", path, Value::Null, &read.secret).await.0,
        403
    );
    let file = s.home.join("owned-edit.txt");
    std::fs::write(&file, "ARCHIVE_BODY_MUST_NOT_LEAK").unwrap();
    let (status, created) = s
        .request(
            "POST",
            "/lingxi/v1/checkpoints/user-edit",
            json!({"filePath":file,"reason":"edit-start"}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{created}");
    let first = created["checkpoint"]["id"].as_str().unwrap().to_owned();
    s.clock.advance(1000);
    std::fs::write(&file, [0u8, 255, 1]).unwrap();
    let (status, created) = s
        .request(
            "POST",
            "/lingxi/v1/checkpoints/user-edit",
            json!({"filePath":file,"reason":"autosave-interval"}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{created}");
    let second = created["checkpoint"]["id"].as_str().unwrap().to_owned();
    let archive_dir = s.home.join("checkpoints");
    std::fs::write(
        archive_dir.join("unfinished.json.tmp"),
        "invalid incomplete archive",
    )
    .unwrap();
    let (status, value) = s.request("GET", path, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{value}");
    let typed: lingxi_protocol::CheckpointArchiveList =
        serde_json::from_value(value.clone()).unwrap();
    assert_eq!(typed.checkpoints.len(), 2);
    assert_eq!(typed.checkpoints[0].id, second);
    assert_eq!(typed.checkpoints[1].id, first);
    for row in value["checkpoints"].as_array().unwrap() {
        assert_eq!(row.as_object().unwrap().len(), 8);
        assert!(row.get("content").is_none());
        assert!(row.get("content_b64").is_none());
        assert_eq!(row["tool"], "user-edit");
        assert_eq!(row["sessionPath"], Value::Null);
    }
    assert!(!value.to_string().contains("ARCHIVE_BODY_MUST_NOT_LEAK"));
    let disk = std::fs::read(archive_dir.join(format!("{first}.json"))).unwrap();
    assert!(String::from_utf8(disk.clone())
        .unwrap()
        .contains("ARCHIVE_BODY_MUST_NOT_LEAK"));
    std::fs::write(archive_dir.join("broken.json"), "{broken").unwrap();
    let (status, error) = s.request("GET", path, Value::Null, &s.token).await;
    assert_eq!(status, 500);
    assert!(error.get("checkpoints").is_none());
    std::fs::remove_file(archive_dir.join("broken.json")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&file, archive_dir.join("linked.json")).unwrap();
        assert_eq!(s.request("GET", path, Value::Null, &s.token).await.0, 500);
        std::fs::remove_file(archive_dir.join("linked.json")).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), vec![0, 255, 1]);
    }
    assert_eq!(
        std::fs::read(archive_dir.join(format!("{first}.json"))).unwrap(),
        disk
    );
    let home = s.close().await;
    let s = Server::boot(Some(home)).await;
    let (status, restarted) = s.request("GET", path, Value::Null, &s.token).await;
    assert_eq!(status, 200);
    assert_eq!(restarted, value);
    let home = s.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_corrupt_preferences_are_explicit_and_authorized_writer_refuses_revocation_at_commit(
) {
    let s = Server::boot(None).await;
    // 先确认本用例无关的 Bridge 首次导入和退出真实完成，再破坏偏好；
    // 保留原 HTTP、原子写拒绝及最终服务退出断言，不冒称损坏期间 Bridge 仍常驻。
    s.state.bridge().stop();
    tokio::time::timeout(Duration::from_secs(5), s.state.bridge().wait_stopped())
        .await
        .unwrap()
        .unwrap();
    let key = "/lingxi/v1/preferences/keybindings";
    let side = "/lingxi/v1/preferences/sidebar-ui";
    for raw in ["{broken", "[]", r#"{"keybindings":false,"sidebar_ui":7}"#] {
        std::fs::write(s.prefs(), raw).unwrap();
        for path in [key, side] {
            assert_eq!(s.request("GET", path, Value::Null, &s.token).await.0, 500);
            assert_eq!(s.request("PUT", path, json!({}), &s.token).await.0, 500);
            assert_eq!(std::fs::read_to_string(s.prefs()).unwrap(), raw);
        }
    }
    let original = r#"{"unrelated":{"keep":true}}"#;
    std::fs::write(s.prefs(), original).unwrap();
    let issued = s
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.write"],
            None,
        )
        .unwrap();
    let principal = s
        .state
        .auth()
        .authenticate(
            Some(&format!("Bearer {}", issued.secret)),
            None,
            false,
            lingxi_service::ConnectionKind::Local,
        )
        .unwrap();
    let checks = std::sync::atomic::AtomicUsize::new(0);
    // 使用同一实际原子writer和真实设备撤销，精确发生在临时文件落下后、rename前。
    let result = lingxi_adapters::storage::agent_memory::atomic_write_authorized(
        &s.prefs(),
        "must-not-commit",
        || {
            if checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                s.state
                    .auth()
                    .revoke_device_credential(&issued.credential_id)
                    .unwrap();
            }
            s.state.auth().validate_principal(&principal).is_ok()
                && principal.has_scope("settings.write")
        },
    );
    assert!(result.is_err());
    assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(std::fs::read_to_string(s.prefs()).unwrap(), original);
    assert!(!std::fs::read_dir(s.home.join("user")).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
    let home = s.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_preferences_match_shared_pure_normalizers_for_every_command_and_partial_sidebar(
) {
    let s = Server::boot(None).await;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    // 期望直接来自原共享纯规则；不读取Rust实现来构造expected，也不执行旧服务。
    let script = r#"
import kb from './shared/keybindings-preferences.cjs';
import { normalizeSidebarUiPrefsPatch, mergeSidebarUiPrefs } from './shared/sidebar-ui-state.ts';
const keyCases = [
  {},
  Object.fromEntries(kb.KEYBINDING_COMMAND_IDS.map(id => [id, [' ctrl + x ', 'Control+X', 'Alt+ ', 'Esc', 'Meta+M']])),
  Object.fromEntries(kb.KEYBINDING_COMMAND_IDS.map(id => [id, []])),
  Object.fromEntries(kb.KEYBINDING_COMMAND_IDS.map(id => [id, null])),
  Object.fromEntries(kb.KEYBINDING_COMMANDS.map(c => [c.id, c.defaultKeys])),
  {'app.new-session': ['\uFEFFcmdorctrl+q\uFEFF', 'option+Spacebar', 9, 'Shift+ß']},
  {'app.new-session': 9, 'voice.record-toggle': ' ctrl + m '}
].map(patch => ({patch, expected:kb.normalizeKeybindings(patch)}));
let current = {};
const sidebarCases = [
  {projectView:{collapsedProjectIds:[' x ','x',9,'y'],collapsedFolderIds:['f']}},
  {sessionList:{rowMode:'single-line'}},
  {projectView:{showAllProjectIds:['y']}},
  {sessionList:{rowMode:'unknown-future-mode'}},
  {projectView:{collapsedProjectIds:[]}}
].map(patch => {current = mergeSidebarUiPrefs(current,normalizeSidebarUiPrefsPatch(patch)); return {patch,expected:current};});
console.log(JSON.stringify({keyCases,sidebarCases}));
"#;
    let output = tokio::process::Command::new("node")
        .args(["--input-type=module", "-e", script])
        .current_dir(repo)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "共享纯规则输出失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cases: Value = serde_json::from_slice(&output.stdout).unwrap();
    for case in cases["keyCases"].as_array().unwrap() {
        let (status, actual) = s
            .request(
                "PUT",
                "/lingxi/v1/preferences/keybindings",
                json!({"keybindings":case["patch"]}),
                &s.token,
            )
            .await;
        assert_eq!(status, 200, "{actual}");
        assert_eq!(actual["keybindings"], case["expected"]);
    }
    for case in cases["sidebarCases"].as_array().unwrap() {
        let (status, actual) = s
            .request(
                "PUT",
                "/lingxi/v1/preferences/sidebar-ui",
                json!({"sidebarUi":case["patch"]}),
                &s.token,
            )
            .await;
        assert_eq!(status, 200, "{actual}");
        assert_eq!(actual["sidebarUi"], case["expected"]);
    }
    let home = s.close().await;
    std::fs::remove_dir_all(home).unwrap();
}
