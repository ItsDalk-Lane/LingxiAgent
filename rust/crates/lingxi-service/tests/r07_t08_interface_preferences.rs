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
    std::fs::remove_dir_all(home).unwrap();
}

// R08-T01：现役 DELETE /api/checkpoints/:id 端口（removeCheckpoint）。缺失幂等
// {ok:true}（现役可观察契约），真实 IO 失败响亮 500（现役 remove catch{} 全吞
// 属静默降级不复刻）；LocalOnly：无凭证 401、非 loopback 凭证 403。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_global_archive_delete_is_idempotent_and_loud_on_io_failure() {
    let s = Server::boot(None).await;
    let file = s.home.join("delete-me.txt");
    std::fs::write(&file, "ARCHIVE_DELETE_BODY").unwrap();
    let (status, created) = s
        .request(
            "POST",
            "/lingxi/v1/checkpoints/user-edit",
            json!({"filePath":file,"reason":"edit-start"}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{created}");
    let id = created["checkpoint"]["id"].as_str().unwrap().to_owned();
    let archive_dir = s.home.join("checkpoints");
    assert!(archive_dir.join(format!("{id}.json")).exists());
    let path = format!("/lingxi/v1/checkpoints/{id}");

    // 非 loopback 凭证 → 403（LocalOnly：本族现役 STUDIO_OWNER 兜底的最严格档）。
    let scoped = s
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.read", "settings.write", "chat", "files.read"],
            None,
        )
        .unwrap();
    assert_eq!(
        s.request("DELETE", &path, Value::Null, &scoped.secret)
            .await
            .0,
        403
    );
    assert!(
        archive_dir.join(format!("{id}.json")).exists(),
        "拒绝不得删除"
    );

    // 本主凭证删除成功：200 {ok:true}，落盘文件消失，列表读回为空。
    let (status, body) = s.request("DELETE", &path, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"ok":true}));
    assert!(!archive_dir.join(format!("{id}.json")).exists());
    let (status, listed) = s
        .request("GET", "/lingxi/v1/checkpoints", Value::Null, &s.token)
        .await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed, json!({"checkpoints":[]}));

    // 幂等：重复删除与从未存在同为 {ok:true}（现役可观察契约）。
    assert_eq!(
        s.request("DELETE", &path, Value::Null, &s.token).await.1,
        json!({"ok":true})
    );
    assert_eq!(
        s.request(
            "DELETE",
            "/lingxi/v1/checkpoints/never-existed",
            Value::Null,
            &s.token
        )
        .await
        .1,
        json!({"ok":true})
    );

    // 真实 IO 失败响亮 500：{id}.json 为目录时 remove_file 失败，不得吞成成功。
    std::fs::create_dir(archive_dir.join("blocked.json")).unwrap();
    assert_eq!(
        s.request(
            "DELETE",
            "/lingxi/v1/checkpoints/blocked",
            Value::Null,
            &s.token
        )
        .await
        .0,
        500
    );
    std::fs::remove_dir(archive_dir.join("blocked.json")).unwrap();

    // 无凭证 → 401。
    assert_eq!(s.raw("DELETE", &path, "null", "").await.0, 401);

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

// R08-T01：现役 /api/preferences/appearance 端口。GET 规范化读取（仅
// theme 非空白串 + serif/paperTexture/leavesOverlay 布尔）；PUT 合并写入
// （wrapper 或裸 body 两形）并按旧 emitAppearanceEvents 口径仅对变化字段
// 发布 app 事件；读 settings.read / 写 settings.write；重启持久化。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_appearance_preferences_normalize_merge_publish_and_scope() {
    let s = Server::boot(None).await;
    let path = "/lingxi/v1/preferences/appearance";
    let events = "/lingxi/v1/app-events";

    // 初始为空对象；无凭证 401。
    let (status, body) = s.request("GET", path, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"appearance":{}}));
    assert_eq!(s.raw("GET", path, "null", "").await.0, 401);

    // scope 分类：settings.read 可读、settings.write 可写，缺 scope 各自 403。
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
    assert_eq!(
        s.request("GET", path, Value::Null, &read.secret).await.0,
        200
    );
    assert_eq!(s.request("PUT", path, json!({}), &read.secret).await.0, 403);
    assert_eq!(
        s.request("GET", path, Value::Null, &write.secret).await.0,
        403
    );
    assert_eq!(
        s.request("PUT", path, json!({"serif":true}), &write.secret)
            .await
            .0,
        200
    );

    // wrapper 形：未知键与非布尔字段规范化丢弃，theme trim。
    let (status, body) = s
        .request(
            "PUT",
            path,
            json!({"appearance":{"theme":" hana-ink ","serif":true,"serifx":"drop","unknown":1,"leavesOverlay":"no"}}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({"ok":true,"appearance":{"theme":"hana-ink","serif":true}})
    );
    let (status, body) = s.request("GET", path, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({"appearance":{"theme":"hana-ink","serif":true}})
    );

    // 事件：此前两次 PUT 的变化（serif:true、theme:hana-ink）已发布。
    let (status, log) = s.request("GET", events, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{log}");
    let published: Vec<&str> = log["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["type"].as_str())
        .collect();
    assert!(published.contains(&"theme-changed"), "{log}");
    assert!(published.contains(&"font-changed"), "{log}");
    assert!(!published.contains(&"paper-texture-changed"), "{log}");
    assert!(!published.contains(&"leaves-overlay-changed"), "{log}");
    let cursor = log["latestSequence"].as_u64().unwrap();

    // 裸 body 合并写：保留 theme，仅 serif 变化 → 只发 font-changed。
    let (status, body) = s
        .request("PUT", path, json!({"serif":false}), &s.token)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({"ok":true,"appearance":{"theme":"hana-ink","serif":false}})
    );
    let (status, log) = s
        .request(
            "GET",
            &format!("{events}?since={cursor}"),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{log}");
    assert_eq!(log["events"].as_array().unwrap().len(), 1, "{log}");
    assert_eq!(
        log["events"][0],
        json!({
            "seq": cursor + 1,
            "type": "font-changed",
            "payload": {"serif": false},
            "source": "server",
            "atUnixMs": log["events"][0]["atUnixMs"],
        })
    );
    let cursor = log["latestSequence"].as_u64().unwrap();

    // 幂等写（值未变）不发布事件；paper/leaves 变化发布 enabled 形。
    assert_eq!(
        s.request("PUT", path, json!({"theme":"hana-ink"}), &s.token)
            .await
            .0,
        200
    );
    let (status, log) = s
        .request(
            "GET",
            &format!("{events}?since={cursor}"),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{log}");
    assert!(log["events"].as_array().unwrap().is_empty(), "{log}");
    assert_eq!(
        s.request(
            "PUT",
            path,
            json!({"paperTexture":true,"leavesOverlay":false}),
            &s.token
        )
        .await
        .0,
        200
    );
    let (status, log) = s
        .request(
            "GET",
            &format!("{events}?since={cursor}"),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{log}");
    let types: Vec<&str> = log["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["type"].as_str())
        .collect();
    assert_eq!(
        types,
        ["paper-texture-changed", "leaves-overlay-changed"],
        "{log}"
    );
    assert_eq!(
        log["events"][0]["payload"],
        json!({"enabled":true}),
        "{log}"
    );
    assert_eq!(
        log["events"][1]["payload"],
        json!({"enabled":false}),
        "{log}"
    );

    // 体校验 400 且不落盘；重启持久化读回。
    let before = std::fs::read(s.prefs()).unwrap();
    assert_eq!(s.raw("PUT", path, "{bad-json", &s.token).await.0, 400);
    assert_eq!(s.request("PUT", path, json!([]), &s.token).await.0, 400);
    assert_eq!(std::fs::read(s.prefs()).unwrap(), before);
    let home = s.close().await;
    let s = Server::boot(Some(home)).await;
    let (status, body) = s.request("GET", path, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({"appearance":{"theme":"hana-ink","serif":false,"paperTexture":true,"leavesOverlay":false}})
    );
    let home = s.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

// R08-T01：现役 /api/preferences/workspace-ui-state 端口（workspace+surface
// 键控读写、全量规范化、files.read/files.write scope、重启持久化）。
// 注意：现役 GET 必经缺失目录 GC（core/engine.ts:3036 →
// core/preferences-manager.ts:671），因此需要读回的 workspace 必须是真实目录。
fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_workspace_ui_state_round_trip_normalize_surface_scope_and_same_home_restart() {
    let s = Server::boot(None).await;
    let path = "/lingxi/v1/preferences/workspace-ui-state";
    let repo = s.home.join("repo");
    let other = s.home.join("other");
    let legacy = s.home.join("legacy");
    for dir in [&repo, &other, &legacy] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let repo_key = repo.to_string_lossy().into_owned();
    let other_key = other.to_string_lossy().into_owned();
    let legacy_key = legacy.to_string_lossy().into_owned();
    let get_repo = format!(
        "{path}?workspace={}&surface=electron",
        url_encode(&repo_key)
    );

    // 初始无状态：GET 200 state:null；GC 未裁剪不落盘（preferences.json 不创建）。
    let (status, body) = s.request("GET", &get_repo, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"state":null}));
    assert!(!s.prefs().exists(), "GC 未变化不得落盘");
    // 无凭证 401。
    assert_eq!(s.raw("GET", &get_repo, "null", "").await.0, 401);
    assert_eq!(s.raw("PUT", path, "{}", "").await.0, 401);

    // 校验 400 同现役文案。
    let (status, body) = s
        .request(
            "GET",
            &format!("{path}?workspace=&surface=electron"),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"workspace must be a non-empty path"}));
    let (status, body) = s
        .request(
            "GET",
            &format!("{path}?workspace={}&surface=watch", url_encode(&repo_key)),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"workspace UI surface is invalid"}));
    let (status, body) = s
        .request(
            "PUT",
            path,
            json!({"workspace":repo_key,"surface":"watch","state":{}}),
            &s.token,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"workspace UI surface is invalid"}));
    let (status, body) = s
        .request(
            "PUT",
            path,
            json!({"surface":"electron","state":{}}),
            &s.token,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"workspace must be a non-empty path"}));

    // scope 分类（现役 route-security.ts:74-78）：GET files.read / PUT files.write。
    let read = s
        .state
        .auth()
        .issue_device_credential(lingxi_service::LOCAL_OWNER_USER_ID, &["files.read"], None)
        .unwrap();
    let write = s
        .state
        .auth()
        .issue_device_credential(lingxi_service::LOCAL_OWNER_USER_ID, &["files.write"], None)
        .unwrap();
    assert_eq!(
        s.request("GET", &get_repo, Value::Null, &read.secret)
            .await
            .0,
        200
    );
    assert_eq!(
        s.request(
            "PUT",
            path,
            json!({"workspace":repo_key,"state":{}}),
            &read.secret
        )
        .await
        .0,
        403
    );
    assert_eq!(
        s.request("GET", &get_repo, Value::Null, &write.secret)
            .await
            .0,
        403
    );
    // files.write 可写（surface 缺省回落 electron）。
    assert_eq!(
        s.request(
            "PUT",
            path,
            json!({"workspace":repo_key,"state":{"previewOpen":true}}),
            &write.secret
        )
        .await
        .0,
        200
    );

    // 全字段 PUT 规范化：未知字段丢弃、路径规范化去重去逃逸、openTabs 过滤
    // 重排、activeTabId 回落、readingPosition clamp/truthy 保留、显式 updatedAt。
    let tab_a = json!({
        "id":"tab-a",
        "filePath":"/abs/file.ts",
        "title":"A",
        "type":"file-info",
        "ext":"ts",
        "language":"ts",
        "readingPosition":{
            "preview":{"scrollTop":10000000,"scrollHeight":500,"ratio":0,"contentHash":"abc","updatedAt":5},
            "currentHeadingId":"h1",
            "contentHash":"hash",
            "updatedAt":7
        }
    });
    let tab_b = json!({
        "id":"tab-b",
        "relativePath":"docs/readme.md",
        "title":"",
        "type":"preview",
        "ext":"md",
        "language":null
    });
    let expected = json!({
        "updatedAt":111,
        "deskCurrentPath":"src/nested",
        "deskExpandedPaths":["src","x/y"],
        "deskSelectedPath":"",
        "rightWorkspaceTab":"plugin-widget:my-widget",
        "jianView":"kanban",
        "jianDrawerOpen":true,
        "previewOpen":true,
        "openTabs":["tab-b","tab-a"],
        "activeTabId":"tab-b",
        "previewTabs":[tab_a.clone(),tab_b.clone()]
    });
    let (status, body) = s
        .request(
            "PUT",
            path,
            json!({
                "workspace":repo_key,
                "surface":"electron",
                "state":{
                    "updatedAt":111,
                    "deskCurrentPath":"src\\nested/",
                    "deskExpandedPaths":["src","","../escape","src","a/./b","x//y"],
                    "deskSelectedPath":"../x",
                    "rightWorkspaceTab":"plugin-widget:my-widget",
                    "jianView":"kanban",
                    "jianDrawerOpen":true,
                    "previewOpen":true,
                    "openTabs":["tab-b","tab-missing","tab-a","tab-b"],
                    "activeTabId":"tab-missing",
                    "previewTabs":[
                        {"id":"tab-a","filePath":"/abs/file.ts","title":" A ","type":"","ext":"TS","language":"ts","content":"dropped-field",
                         "readingPosition":{"preview":{"scrollTop":99000000000i64,"scrollLeft":0,"scrollHeight":500,"clientHeight":0,"ratio":0,"anchorId":"","contentHash":"abc","updatedAt":5},
                                            "currentHeadingId":"h1","currentHeadingText":"","contentHash":"hash","updatedAt":7}},
                        {"id":"tab-a","relativePath":"dup"},
                        {"id":"","relativePath":"x"},
                        {"id":"tab-b","relativePath":"docs/readme.md","sourceRootPath":"relative/root","type":"preview","ext":"Md"},
                        {"id":"tab-c"}
                    ]
                }
            }),
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"ok":true,"state":expected.clone()}));
    let (status, body) = s.request("GET", &get_repo, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"state":expected.clone()}));

    // surface 分桶 + updatedAt 缺省回落 now（ManualClock 固定 1_800_000_000_000）
    // + record.updatedAt = max(各 surface updatedAt)。
    let pwa_expected = json!({
        "updatedAt":1800000000000i64,
        "deskCurrentPath":"",
        "deskExpandedPaths":[],
        "deskSelectedPath":"",
        "rightWorkspaceTab":"workspace",
        "jianView":"desk",
        "jianDrawerOpen":false,
        "previewOpen":true,
        "openTabs":[],
        "activeTabId":null,
        "previewTabs":[]
    });
    let (status, body) = s
        .request(
            "PUT",
            path,
            json!({"workspace":repo_key,"surface":"pwa","state":{"previewOpen":true}}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"ok":true,"state":pwa_expected.clone()}));
    let (status, body) = s
        .request(
            "GET",
            &format!("{path}?workspace={}&surface=pwa", url_encode(&repo_key)),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"state":pwa_expected}));
    // electron 桶不受影响。
    let (status, body) = s.request("GET", &get_repo, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"state":expected.clone()}));
    let stored: Value = serde_json::from_slice(&std::fs::read(s.prefs()).unwrap()).unwrap();
    let record = &stored["workspace_ui_state"]["workspaces"][repo_key.as_str()];
    assert_eq!(record["updatedAt"], json!(1800000000000i64), "{stored}");
    assert_eq!(
        record["surfaces"]["electron"]["updatedAt"],
        json!(111),
        "{stored}"
    );

    // openTabs 缺省回填 previewTabs[0].id；previewTabs 上限 32；缺省 surface 桶；
    // 严格布尔（"yes"/1 不为 true）；非法 rightWorkspaceTab/jianView 回落默认。
    let tabs: Vec<Value> = (0..35)
        .map(|i| json!({"id":format!("t{i}"),"relativePath":format!("f{i}.md")}))
        .collect();
    let (status, body) = s
        .request(
            "PUT",
            path,
            json!({
                "workspace":other_key,
                "state":{
                    "previewTabs":tabs,
                    "jianDrawerOpen":"yes",
                    "previewOpen":1,
                    "rightWorkspaceTab":"nonsense",
                    "jianView":""
                }
            }),
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let entry = &body["state"];
    assert_eq!(entry["previewTabs"].as_array().unwrap().len(), 32, "{body}");
    assert_eq!(entry["previewTabs"][0]["id"], json!("t0"), "{body}");
    assert_eq!(entry["previewTabs"][31]["id"], json!("t31"), "{body}");
    assert_eq!(entry["openTabs"], json!(["t0"]), "{body}");
    assert_eq!(entry["activeTabId"], json!("t0"), "{body}");
    assert_eq!(entry["jianDrawerOpen"], json!(false), "{body}");
    assert_eq!(entry["previewOpen"], json!(false), "{body}");
    assert_eq!(entry["rightWorkspaceTab"], json!("workspace"), "{body}");
    assert_eq!(entry["jianView"], json!("desk"), "{body}");
    // GET 缺省 surface → electron 桶（与写入一致）。
    let (status, body) = s
        .request(
            "GET",
            &format!("{path}?workspace={}", url_encode(&other_key)),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["state"]["openTabs"], json!(["t0"]), "{body}");

    // 旧版扁平记录迁移：无 surfaces 字段时物化到 electron 桶。
    let mut stored: Value = serde_json::from_slice(&std::fs::read(s.prefs()).unwrap()).unwrap();
    stored["workspace_ui_state"]["workspaces"][legacy_key.as_str()] =
        json!({"updatedAt":5,"deskCurrentPath":"docs","previewOpen":true});
    std::fs::write(s.prefs(), serde_json::to_vec(&stored).unwrap()).unwrap();
    let (status, body) = s
        .request(
            "GET",
            &format!(
                "{path}?workspace={}&surface=electron",
                url_encode(&legacy_key)
            ),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({"state":{
            "updatedAt":5,
            "deskCurrentPath":"docs",
            "deskExpandedPaths":[],
            "deskSelectedPath":"",
            "rightWorkspaceTab":"workspace",
            "jianView":"desk",
            "jianDrawerOpen":false,
            "previewOpen":true,
            "openTabs":[],
            "activeTabId":null,
            "previewTabs":[]
        }})
    );

    // 体形状 400 同现役 safeJson 口径：不可解析/空体 → workspace 校验 400；
    // null/标量 → invalid JSON body；数组 → 无字段对象 → workspace 校验 400。
    // 全部不落盘。
    let before = std::fs::read(s.prefs()).unwrap();
    let (status, body) = s.raw("PUT", path, "{bad-json", &s.token).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"workspace must be a non-empty path"}));
    let (status, body) = s.raw("PUT", path, "", &s.token).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"workspace must be a non-empty path"}));
    let (status, body) = s.request("PUT", path, json!([]), &s.token).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"workspace must be a non-empty path"}));
    let (status, body) = s.request("PUT", path, Value::Null, &s.token).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"invalid JSON body"}));
    let (status, body) = s.raw("PUT", path, "\"str\"", &s.token).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body, json!({"error":"invalid JSON body"}));
    assert_eq!(std::fs::read(s.prefs()).unwrap(), before, "400 不得落盘");

    // 重启持久化读回。
    let home = s.close().await;
    let s = Server::boot(Some(home)).await;
    let (status, body) = s.request("GET", &get_repo, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"state":expected}));
    let home = s.close().await;
    std::fs::remove_dir_all(home).unwrap();
}

// R08-T01：现役 GET 路径 GC（shared/workspace-persistence-gc.ts：ENOENT/ENOTDIR
// 裁剪并落盘、其他错误保留）与 normalizeWorkspaceUiState 的 50 个 workspace
// updatedAt 降序裁剪。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_workspace_ui_state_gc_prunes_missing_and_trims_over_fifty() {
    let s = Server::boot(None).await;
    let path = "/lingxi/v1/preferences/workspace-ui-state";
    let keep_a = s.home.join("keep-a");
    std::fs::create_dir_all(&keep_a).unwrap();
    let keep_a_key = keep_a.to_string_lossy().into_owned();
    let missing_key = s.home.join("missing-ws").to_string_lossy().into_owned();
    // ENOTDIR 形态：路径分量是文件而非目录。
    std::fs::write(s.home.join("afile"), b"x").unwrap();
    let notadir_key = s
        .home
        .join("afile")
        .join("child")
        .to_string_lossy()
        .into_owned();
    let get_keep_a = format!(
        "{path}?workspace={}&surface=electron",
        url_encode(&keep_a_key)
    );

    // PUT 不做 GC：missing 与 ENOTDIR workspace 先落盘。
    let (status, _) = s
        .request(
            "PUT",
            path,
            json!({"workspace":keep_a_key,"state":{"updatedAt":10,"previewOpen":true}}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200);
    let (status, _) = s
        .request(
            "PUT",
            path,
            json!({"workspace":missing_key,"state":{"updatedAt":20,"previewOpen":true}}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200);
    let (status, _) = s
        .request(
            "PUT",
            path,
            json!({"workspace":notadir_key,"state":{"updatedAt":30,"previewOpen":true}}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200);
    let stored: Value = serde_json::from_slice(&std::fs::read(s.prefs()).unwrap()).unwrap();
    assert_eq!(
        stored["workspace_ui_state"]["workspaces"]
            .as_object()
            .unwrap()
            .len(),
        3,
        "{stored}"
    );

    // GET 触发 GC：missing(ENOENT) 与 notadir(ENOTDIR) 裁剪并落盘；keep-a 保留。
    let (status, body) = s.request("GET", &get_keep_a, Value::Null, &s.token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["state"]["updatedAt"], json!(10), "{body}");
    let stored: Value = serde_json::from_slice(&std::fs::read(s.prefs()).unwrap()).unwrap();
    let workspaces = stored["workspace_ui_state"]["workspaces"]
        .as_object()
        .unwrap();
    assert!(workspaces.contains_key(keep_a_key.as_str()), "{stored}");
    assert!(!workspaces.contains_key(missing_key.as_str()), "{stored}");
    assert!(!workspaces.contains_key(notadir_key.as_str()), "{stored}");

    // EACCES 等未知错误保留（现役 classifyWorkspacePathForGc → "unknown"）。
    // 以 0o000 父目录制造 PermissionDenied；root 运行时 stat 成功同样判定保留，
    // 断言在两种环境下一致。
    #[cfg(unix)]
    let locked_key = {
        use std::os::unix::fs::PermissionsExt;
        let locked = s.home.join("locked");
        std::fs::create_dir_all(locked.join("inner")).unwrap();
        let locked_key = locked.join("inner").to_string_lossy().into_owned();
        let (status, _) = s
            .request(
                "PUT",
                path,
                json!({"workspace":locked_key,"state":{"updatedAt":40,"previewOpen":true}}),
                &s.token,
            )
            .await;
        assert_eq!(status, 200);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let (status, _) = s.request("GET", &get_keep_a, Value::Null, &s.token).await;
        assert_eq!(status, 200);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let stored: Value = serde_json::from_slice(&std::fs::read(s.prefs()).unwrap()).unwrap();
        let workspaces = stored["workspace_ui_state"]["workspaces"]
            .as_object()
            .unwrap();
        assert!(
            workspaces.contains_key(locked_key.as_str()),
            "未知错误（EACCES）workspace 必须保留: {stored}"
        );
        locked_key
    };

    // >50 个 workspace 的 updatedAt 降序裁剪：播种 52 个真实目录
    // （updatedAt 1000..=1051），再一次 PUT（updatedAt 9999）触发规范化落盘。
    let mut ws_keys = Vec::new();
    for i in 0..52 {
        let dir = s.home.join(format!("ws-{i:02}"));
        std::fs::create_dir_all(&dir).unwrap();
        ws_keys.push(dir.to_string_lossy().into_owned());
    }
    let ws_new = s.home.join("ws-new");
    std::fs::create_dir_all(&ws_new).unwrap();
    let ws_new_key = ws_new.to_string_lossy().into_owned();
    let mut stored: Value = serde_json::from_slice(&std::fs::read(s.prefs()).unwrap()).unwrap();
    for (i, key) in ws_keys.iter().enumerate() {
        let updated = 1000 + i as i64;
        stored["workspace_ui_state"]["workspaces"][key.as_str()] = json!({
            "updatedAt":updated,
            "surfaces":{"electron":{"updatedAt":updated,"previewOpen":true}}
        });
    }
    std::fs::write(s.prefs(), serde_json::to_vec(&stored).unwrap()).unwrap();
    let (status, _) = s
        .request(
            "PUT",
            path,
            json!({"workspace":ws_new_key,"state":{"updatedAt":9999,"previewOpen":true}}),
            &s.token,
        )
        .await;
    assert_eq!(status, 200);
    let stored: Value = serde_json::from_slice(&std::fs::read(s.prefs()).unwrap()).unwrap();
    let workspaces = stored["workspace_ui_state"]["workspaces"]
        .as_object()
        .unwrap();
    assert_eq!(workspaces.len(), 50, "{stored}");
    assert!(workspaces.contains_key(ws_new_key.as_str()), "{stored}");
    assert!(workspaces.contains_key(ws_keys[51].as_str()), "{stored}");
    assert!(workspaces.contains_key(ws_keys[3].as_str()), "{stored}");
    assert!(!workspaces.contains_key(ws_keys[2].as_str()), "{stored}");
    assert!(!workspaces.contains_key(ws_keys[0].as_str()), "{stored}");
    assert!(!workspaces.contains_key(keep_a_key.as_str()), "{stored}");
    #[cfg(unix)]
    assert!(!workspaces.contains_key(locked_key.as_str()), "{stored}");

    // 被裁剪的 workspace 目录仍存在但已不在状态中 → GET state:null；
    // 现存 workspace GET 正常；GC 无裁剪不再落盘。
    let before = std::fs::read(s.prefs()).unwrap();
    let (status, body) = s
        .request(
            "GET",
            &format!(
                "{path}?workspace={}&surface=electron",
                url_encode(&ws_keys[2])
            ),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"state":null}));
    let (status, body) = s
        .request(
            "GET",
            &format!(
                "{path}?workspace={}&surface=electron",
                url_encode(&ws_new_key)
            ),
            Value::Null,
            &s.token,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["state"]["updatedAt"], json!(9999), "{body}");
    assert_eq!(
        std::fs::read(s.prefs()).unwrap(),
        before,
        "GC 未变化不得落盘"
    );

    let home = s.close().await;
    std::fs::remove_dir_all(home).unwrap();
}
