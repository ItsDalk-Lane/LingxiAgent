//! R06 F 块两后端叶子（真实 HTTP + 真实文件系统，boot 脚手架照
//! r06_session_authorized_folders.rs，含 BOOT_SEQ 原子序号）：
//!
//! - R00-T02-LA-337738D5D233 工作区技能目录删除（POST /desk/delete-skill，
//!   desk.ts:1383-1412）：只允许当前工作区四原技能根内的已知目录；递归
//!   删除后重新同步目录（目录视图 + skills-changed 事件），失败返回错误；
//!   每个失败/冲突/拒绝的返回状态与实际副作用一致。
//! - R00-T02-LA-FAE2BDEFDB04 组合包导出（POST /skills/bundles/:id/export，
//!   package-service.ts）：构建可下载组合包（store-mode zip 落盘）与结果
//!   元数据；id 不存在/打包失败明确错误，不扩大写入或披露范围。
//!
//! 两个新路由尚未进 lib.rs（主控统一注册），测试内以 axum::Router 挂载：
//! 生产 `build_router` 与新路由合并，新路由由测试本地中间件执行与生产
//! `auth_guard` 同源的 authenticate + authorize（真实 AuthService 与真实
//! classify_route/authorize 表）。zip 的可解性/条目/字节一致性由
//! python3 -c zipfile 实际校验（testzip CRC + namelist + 逐文件比对）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lingxi_service::{
    build_router, classify_route, prepare_layout, HomeSource, NetworkMode, RoutePolicy,
    ServiceConfig, ServiceDeps, ServiceState, LOCAL_OWNER_USER_ID,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── boot 脚手架（r06_session_authorized_folders.rs 同款 + 自定路由挂载）──

struct Server {
    addr: SocketAddr,
    home: PathBuf,
    workspace: Option<PathBuf>,
    token: String,
    readonly_token: String,
    state: ServiceState,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

impl Server {
    fn workspace(&self) -> &Path {
        self.workspace.as_deref().expect("booted with workspace")
    }
}

async fn boot(tag: &str, with_workspace: bool) -> Server {
    // 同进程内并行 boot 的 SystemTime 纳秒可能落在同一时钟粒度内（pid 相同），
    // 纯 pid+nanos 曾让两个测试引导同一 runs.db 并在迁移窗口互撞
    // （"table sessions already exists"）；原子序号保证 home 唯一。
    static BOOT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let uniq = format!(
        "{}-{}-{}-{tag}",
        std::process::id(),
        BOOT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06-skill-del-exp-home-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06-skill-del-exp-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    // 记忆材料是服务面文件，启动前落盘（r06_original_skills_bundle_proofs 同款）。
    let agent_dir = home.join("agents").join("agentA");
    std::fs::create_dir_all(agent_dir.join("memory")).expect("mkdir agent");
    std::fs::write(
        agent_dir.join("config.yaml"),
        "locale: zh\nagent:\n  name: 助手\nmemory:\n  enabled: true\n",
    )
    .expect("write agent config");
    let home = home.canonicalize().expect("canonical home");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("loopback"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let deps = ServiceDeps {
        workspace_root: with_workspace.then_some(workspace.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let readonly_token = state
        .auth()
        .issue_device_credential(LOCAL_OWNER_USER_ID, &["settings.read"], None)
        .expect("只读凭证")
        .secret;
    // 两条新路由已在生产 build_router 注册（含完整 auth_guard 链）——本测试
    // 直接经生产路由面验证，不再本地合并重复挂载。
    let app = build_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        // 生产 run() 以 connect_info 起服务（鉴权审计等处理器要求该扩展），
        // 本地同口径包装，避免 desk/skills 等路由缺扩展 500。
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stop_rx.await;
        })
        .await
        .expect("serve loop");
    });
    Server {
        addr,
        home,
        workspace: with_workspace.then_some(workspace),
        token,
        readonly_token,
        state,
        stop,
        handle,
    }
}

async fn http(
    server: &Server,
    method: &str,
    path: &str,
    body: Option<&Value>,
    token: Option<&str>,
) -> (u16, Value) {
    let task = async {
        let mut socket = TcpStream::connect(server.addr).await.unwrap();
        let payload = body.map(|value| value.to_string()).unwrap_or_default();
        let auth = token
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{payload}",
            addr = server.addr,
            len = payload.len(),
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        socket.read_to_end(&mut response).await.unwrap();
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .expect("HTTP 头存在");
        let headers = String::from_utf8_lossy(&response[..boundary]);
        let status: u16 = headers
            .split_whitespace()
            .nth(1)
            .expect("状态码")
            .parse()
            .expect("合法状态码");
        let body = &response[boundary + 4..];
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(body).unwrap_or_else(|error| {
                panic!(
                    "JSON 体解析失败 method={method} path={path} status={status} headers={headers} error={error} body={}",
                    String::from_utf8_lossy(body)
                )
            })
        };
        (status, value)
    };
    tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .expect("HTTP 往返在限时内")
}

async fn ok(server: &Server, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    http(server, method, path, body, Some(&server.token)).await
}

async fn stop_and_clean(server: Server) {
    server.stop.send(()).expect("server still listening");
    tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("server stops within timeout")
        .expect("server task join");
    let _ = std::fs::remove_dir_all(&server.home);
    if let Some(workspace) = &server.workspace {
        let _ = std::fs::remove_dir_all(workspace);
    }
}

// ── 夹具 ────────────────────────────────────────────────────────────────────

/// 工作区技能包：SKILL.md（frontmatter name）+ 嵌套 examples 文件。
fn write_workspace_skill(workspace: &Path, root: &str, name: &str) -> PathBuf {
    let dir = workspace.join(root).join(name);
    std::fs::create_dir_all(dir.join("examples")).expect("mkdir workspace skill");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 工作区技能\n---\n{name} 正文"),
    )
    .expect("write workspace SKILL.md");
    std::fs::write(dir.join("examples/example.txt"), "示例内容").expect("write example");
    dir
}

/// 用户池技能包（home/skills/<name>）。
fn write_user_skill(home: &Path, name: &str) -> PathBuf {
    let dir = home.join("skills").join(name);
    std::fs::create_dir_all(dir.join("examples")).expect("mkdir user skill");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 用户技能\n---\n{name} 正文"),
    )
    .expect("write user SKILL.md");
    std::fs::write(dir.join("examples/example.txt"), "示例内容").expect("write example");
    dir
}

/// 目录快照（相对路径 + 字节，排序）——副作用一致性断言用。
fn file_snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        if !dir.exists() {
            return;
        }
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.push((
                    path.strip_prefix(root).unwrap().to_string_lossy().into(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// python3 -c zipfile 实际校验：CRC（testzip）、条目清单、逐文件字节一致、
/// bundle.json 可解析。期望清单经 stdin 传入（避免任何字面量转义歧义），
/// 返回 {"manifest":…,"names":[…]}。
fn python_zip_verdict(zip_path: &Path, expected: &Value) -> Value {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let script = format!(
        r#"
import json, sys, zipfile
expected = json.loads(sys.stdin.buffer.read().decode("utf-8"))
with zipfile.ZipFile({zip_path:?}) as zf:
    bad = zf.testzip()
    assert bad is None, "CRC mismatch: %s" % bad
    names = sorted(zf.namelist())
    assert names == sorted(expected["names"]), "entry mismatch: %s" % names
    for name, want in expected["contents"].items():
        got = zf.read(name)
        assert got == want.encode("utf-8"), "content mismatch: %s" % name
    manifest = json.loads(zf.read("bundle.json").decode("utf-8"))
print(json.dumps({{"manifest": manifest, "names": names}}))
"#
    );
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python3");
    child
        .stdin
        .take()
        .expect("python stdin")
        .write_all(expected.to_string().as_bytes())
        .expect("write expected json");
    let output = child.wait_with_output().expect("python3 completes");
    assert!(
        output.status.success(),
        "python zipfile 校验失败\nstderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    serde_json::from_slice(&output.stdout).expect("python 校验输出 JSON")
}

/// desk 删除/导出拒绝面的 home 侧关键材料：用户技能池 + 助手配置 +
/// 组合存储字节（后台知识库会写 knowledge/，不纳入比对——与断言相关的
/// 写入面只有这三处）。
type HomeMaterial = (
    Vec<(String, Vec<u8>)>,
    Vec<(String, Vec<u8>)>,
    Option<Vec<u8>>,
);

fn home_material(server: &Server) -> HomeMaterial {
    (
        file_snapshot(&server.home.join("skills")),
        file_snapshot(&server.home.join("agents")),
        std::fs::read(server.home.join("skill-bundles.json")).ok(),
    )
}

fn bundles_store(server: &Server) -> lingxi_adapters::storage::skill_library::BundlesStore {
    lingxi_adapters::storage::skill_library::BundlesStore::new(&server.home)
}

async fn desk_skill_names(server: &Server) -> Vec<String> {
    let (status, body) = ok(server, "GET", "/lingxi/v1/desk/skills?agentId=agentA", None).await;
    assert_eq!(status, 200, "{body}");
    body["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .filter_map(|skill| skill["name"].as_str().map(str::to_string))
        .collect()
}

async fn skills_changed_events(server: &Server) -> Vec<Value> {
    let (status, body) = ok(server, "GET", "/lingxi/v1/app-events", None).await;
    assert_eq!(status, 200, "{body}");
    body["events"]
        .as_array()
        .expect("events array")
        .iter()
        .filter(|event| event["type"] == "skills-changed")
        .cloned()
        .collect()
}

// ── 鉴权分类（auth.rs 现有写分类，零改动验证）──────────────────────────────

#[test]
fn routes_follow_incumbent_write_classification() {
    // desk 面 = LocalOnly（与 GET /lingxi/v1/desk/skills 同族）。
    assert_eq!(
        classify_route("POST", "/lingxi/v1/desk/delete-skill"),
        RoutePolicy::LocalOnly
    );
    // 组合包导出 = settings.write（route-security.ts:495-517 的 bundles/export
    // 写分类）。
    assert_eq!(
        classify_route("POST", "/lingxi/v1/skills/bundles/demo/export"),
        RoutePolicy::Scope("settings.write")
    );
}

// ── 叶子 1：工作区技能目录删除 ─────────────────────────────────────────────

/// 断言 1：只允许当前工作区已知技能目录；递归删除后重新同步目录（目录
/// 视图 + skills-changed 事件），失败返回错误（见下一测试）。
#[tokio::test]
async fn workspace_skill_dir_delete_only_allows_known_workspace_dirs_then_resyncs() {
    let server = boot("ws-del-positive", true).await;
    // 四原根中的 standard（.agents/skills）与 compatible（.codex/skills）两个家族。
    let standard = write_workspace_skill(server.workspace(), ".agents/skills", "ws-alpha");
    let compatible = write_workspace_skill(server.workspace(), ".codex/skills", "ws-beta");
    // 用户池技能不在 desk 删除面内（GAP：普通 user 技能 DELETE 不能代替）。
    let user_skill = write_user_skill(&server.home, "user-pool-skill");

    // 生产 GET /desk/skills 面先证明两个工作区技能在目录里。
    let names = desk_skill_names(&server).await;
    assert!(names.contains(&"ws-alpha".to_string()), "{names:?}");
    assert!(names.contains(&"ws-beta".to_string()), "{names:?}");
    assert!(!names.contains(&"user-pool-skill".to_string()), "{names:?}");

    // 删除 standard 根下技能：递归（嵌套 examples 一并删除）。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/desk/delete-skill",
        Some(&json!({"skillDir": standard})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(!standard.exists(), "技能目录已递归删除");
    assert!(!standard.join("examples/example.txt").exists());

    // 重新同步：目录视图不再列 ws-alpha，ws-beta 保留。
    let names = desk_skill_names(&server).await;
    assert_eq!(names, vec!["ws-beta".to_string()], "重同步后的目录视图");

    // skills-changed 事件（agentId 缺省 → null，desk.ts:1406 agentId||null）。
    let events = skills_changed_events(&server).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["payload"], json!({ "agentId": null }));

    // compatible 根（.codex/skills）同样是已知根；带 agentId 时事件跟随。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/desk/delete-skill",
        Some(&json!({"skillDir": compatible, "agentId": "agentA"})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(!compatible.exists());
    let names = desk_skill_names(&server).await;
    assert!(names.is_empty(), "两个根的技能都删除后目录为空：{names:?}");
    let events = skills_changed_events(&server).await;
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[1]["payload"], json!({ "agentId": "agentA" }));

    // desk 删除面不触碰用户池。
    assert!(user_skill.join("SKILL.md").is_file());
    stop_and_clean(server).await;
}

/// 断言 2：失败、冲突或拒绝时返回的状态与实际副作用一致（快照比对）。
#[tokio::test]
async fn workspace_skill_dir_delete_rejections_match_actual_side_effects() {
    let server = boot("ws-del-negative", true).await;
    let skill = write_workspace_skill(server.workspace(), ".agents/skills", "keep-me");
    let user_skill = write_user_skill(&server.home, "user-pool-skill");
    let plain_file = server.workspace().join(".agents/skills/plain.txt");
    std::fs::write(&plain_file, "不是目录").unwrap();
    let sibling_dir = server.workspace().join("random-dir");
    std::fs::create_dir_all(&sibling_dir).unwrap();
    std::fs::write(sibling_dir.join("keep.txt"), "保留").unwrap();
    let outside_dir = std::env::temp_dir().join(format!(
        "lingxi-r06-skill-del-exp-outside-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&outside_dir);
    let outside_skill = write_workspace_skill(&outside_dir, ".agents/skills", "outside-skill");

    let mut rejections: Vec<(Value, u16, &str)> = vec![
        (json!({}), 400, "缺 skillDir"),
        (json!({"skillDir": ""}), 400, "空 skillDir"),
        (json!({"skillDir": 42}), 400, "非字符串 skillDir"),
        (
            json!({"skillDir": server.workspace().join(".agents/skills/ghost")}),
            404,
            "根内不存在",
        ),
        (
            json!({"skillDir": user_skill}),
            403,
            "用户池技能不在工作区技能根",
        ),
        (
            json!({"skillDir": server.workspace().join(".agents")}),
            403,
            "技能根的父目录",
        ),
        (
            json!({"skillDir": sibling_dir}),
            403,
            "工作区内但不在四原根",
        ),
        (json!({"skillDir": outside_skill}), 403, "工作区外目录"),
        (
            json!({"skillDir": skill, "dir": "relative/path"}),
            400,
            "相对 dir",
        ),
        (
            json!({"skillDir": skill, "dir": outside_dir}),
            403,
            "dir 越出批准根",
        ),
        (
            json!({"skillDir": skill, "dir": plain_file}),
            400,
            "dir 是文件",
        ),
        (
            json!({"skillDir": skill, "dir": server.workspace().join("missing-dir")}),
            404,
            "dir 不存在",
        ),
        // 文件不是目录：根内普通文件（containment 通过 → 确为目录检查 400）。
        (json!({"skillDir": plain_file}), 400, "根内文件不是目录"),
    ];
    #[cfg(unix)]
    {
        // symlink 越界（canonical 解析到工作区外 → 403，目标不受影响）。
        let symlink_escape = server.workspace().join(".agents/skills/escape");
        std::os::unix::fs::symlink(&outside_skill, &symlink_escape).expect("symlink escape");
        rejections.push((json!({"skillDir": symlink_escape}), 403, "symlink 越界"));
    }

    let before_workspace = file_snapshot(server.workspace());
    let before_home = home_material(&server);
    let before_outside = file_snapshot(&outside_dir);
    for (payload, expected, label) in &rejections {
        let (status, body) = ok(
            &server,
            "POST",
            "/lingxi/v1/desk/delete-skill",
            Some(payload),
        )
        .await;
        assert_eq!(*expected, status, "{label}：{body}");
        assert_ne!(body["ok"], json!(true), "{label}：不得回报成功");
        assert!(body.get("error").is_some(), "{label}：错误必须显式：{body}");
        // 副作用一致：拒绝路径零写入。
        assert_eq!(
            file_snapshot(server.workspace()),
            before_workspace,
            "{label}"
        );
        assert_eq!(home_material(&server), before_home, "{label}");
        assert_eq!(file_snapshot(&outside_dir), before_outside, "{label}");
    }
    assert!(skill.join("SKILL.md").is_file(), "目标技能仍存在");
    assert!(user_skill.join("SKILL.md").is_file(), "用户池技能仍存在");

    // 非法 JSON 体 → 400。
    let (status, _) = http(
        &server,
        "POST",
        "/lingxi/v1/desk/delete-skill",
        None,
        Some(&server.token),
    )
    .await;
    assert_eq!(status, 400, "空体不是合法 JSON");
    let mut socket = TcpStream::connect(server.addr).await.unwrap();
    let payload = "{\"skillDir\":";
    let request = format!(
        "POST /lingxi/v1/desk/delete-skill HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        server.addr,
        server.token,
        payload.len(),
        payload,
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await.unwrap();
    let headers = String::from_utf8_lossy(&response);
    let status: u16 = headers
        .split_whitespace()
        .nth(1)
        .expect("状态码")
        .parse()
        .expect("合法状态码");
    assert_eq!(status, 400, "非法 JSON 体");

    // 递归删除失败 → 500，且目录保留（unix 权限真实失败）。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&skill).unwrap().permissions().mode();
        std::fs::set_permissions(&skill, std::fs::Permissions::from_mode(0o555)).unwrap();
        let (status, body) = ok(
            &server,
            "POST",
            "/lingxi/v1/desk/delete-skill",
            Some(&json!({"skillDir": skill})),
        )
        .await;
        std::fs::set_permissions(&skill, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(status, 500, "递归删除失败必须显式 500：{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("failed to delete skill directory"),
            "{body}"
        );
        assert!(skill.is_dir(), "失败后目录保留");
        assert!(skill.join("SKILL.md").is_file(), "失败后内容保留");
    }

    // dir 覆盖路径的正例：批准根内子工作区。
    let sub = server.workspace().join("sub");
    let sub_skill = write_workspace_skill(&sub, ".agents/skills", "sub-skill");
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/desk/delete-skill",
        Some(&json!({"skillDir": sub_skill, "dir": sub})),
    )
    .await;
    assert_eq!(status, 200, "dir 覆盖（批准根内）：{body}");
    assert!(!sub_skill.exists());
    // TS isInsidePath 的等值语义（resolved === base 允许删除根本身）。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/desk/delete-skill",
        Some(&json!({"skillDir": sub.join(".agents/skills"), "dir": sub})),
    )
    .await;
    assert_eq!(status, 200, "根等值允许（desk.ts isInsidePath）：{body}");
    assert!(!sub.join(".agents/skills").exists());

    // 无工作区：400 No active workspace；带 dir 无批准根 → 403。
    let bare = boot("ws-del-no-workspace", false).await;
    let (status, body) = ok(
        &bare,
        "POST",
        "/lingxi/v1/desk/delete-skill",
        Some(&json!({"skillDir": skill})),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("No active workspace"));
    let (status, body) = ok(
        &bare,
        "POST",
        "/lingxi/v1/desk/delete-skill",
        Some(&json!({"skillDir": skill, "dir": bare.home})),
    )
    .await;
    assert_eq!(status, 403, "无批准根时 dir 不可用：{body}");
    assert!(skill.join("SKILL.md").is_file());

    let _ = std::fs::remove_dir_all(&outside_dir);
    stop_and_clean(bare).await;
    stop_and_clean(server).await;
}

/// 无凭证 401 / 非本地 owner（只读设备凭证）403 / 拒绝零副作用。
#[tokio::test]
async fn delete_and_export_reject_missing_or_insufficient_credentials() {
    let server = boot("ws-del-exp-auth", true).await;
    let skill = write_workspace_skill(server.workspace(), ".agents/skills", "auth-guard-skill");
    let user_skill = write_user_skill(&server.home, "auth-export-skill");
    let bundle = bundles_store(&server)
        .create("鉴权组合", &["auth-export-skill".into()], "user", None)
        .unwrap();
    let bundle_id = bundle["id"].as_str().unwrap().to_string();
    let export_path = format!("/lingxi/v1/skills/bundles/{bundle_id}/export");
    let delete_body = json!({"skillDir": skill});

    let before_workspace = file_snapshot(server.workspace());
    let before_home = home_material(&server);
    for (path, body, label) in [
        (
            "/lingxi/v1/desk/delete-skill",
            &delete_body,
            "delete 无凭证",
        ),
        (&export_path, &Value::Null, "export 无凭证"),
    ] {
        let (status, body_value) = http(&server, "POST", path, None, None).await;
        assert_eq!(status, 401, "{label}：{body_value}");
        let (status, body_value) =
            http(&server, "POST", path, Some(body), Some("invalid-token")).await;
        assert_eq!(status, 401, "{label} 伪造凭证：{body_value}");
        let (status, body_value) = http(
            &server,
            "POST",
            path,
            Some(body),
            Some(&server.readonly_token),
        )
        .await;
        assert_eq!(status, 403, "{label} 只读凭证：{body_value}");
    }
    assert!(skill.join("SKILL.md").is_file(), "拒绝路径零副作用");
    assert_eq!(file_snapshot(server.workspace()), before_workspace);
    assert_eq!(home_material(&server), before_home);
    let _ = user_skill;
    stop_and_clean(server).await;
}

// ── 叶子 2：组合包导出 ──────────────────────────────────────────────────────

/// 断言 1：导出后构建可下载组合包（zip 实际可解、条目字节一致、清单正确）
/// 与结果元数据（filePath/fileName/skillCount/warnings，文件名唯一化）。
#[tokio::test]
async fn export_builds_downloadable_zip_and_result_metadata() {
    let server = boot("ws-export-positive", true).await;
    let one = write_user_skill(&server.home, "export-one");
    // 无 frontmatter name：导出名走 SKILL.md 标题回退（package-service.ts:63-76）。
    let two = server.home.join("skills").join("export-two");
    std::fs::create_dir_all(&two).expect("mkdir");
    std::fs::write(two.join("SKILL.md"), "# exported-heading\n标题正文").expect("write");
    let bundle = bundles_store(&server)
        .create(
            "导出组合 Alpha",
            &[
                "export-one".into(),
                "ghost-skill".into(),
                "export-two".into(),
            ],
            "user",
            None,
        )
        .unwrap();
    let bundle_id = bundle["id"].as_str().unwrap().to_string();
    let before_skills = file_snapshot(&server.home.join("skills"));

    let (status, body) = ok(
        &server,
        "POST",
        &format!("/lingxi/v1/skills/bundles/{bundle_id}/export"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    // 文件名 slug（CJK 保留、空格折叠、<slug>-skillbundle.zip）。
    assert_eq!(body["fileName"], json!("导出组合-alpha-skillbundle.zip"));
    let zip_path = server.workspace().join("导出组合-alpha-skillbundle.zip");
    assert_eq!(body["filePath"], json!(zip_path));
    assert!(zip_path.is_file(), "可下载包已落盘");
    // 结果元数据：组合回执 + 缺技能降级 warnings（只披露请求名）。
    assert_eq!(
        body["bundle"],
        json!({"id": bundle_id, "name": "导出组合 Alpha", "skillCount": 2})
    );
    assert_eq!(
        body["warnings"],
        json!([{"type": "missing-skill", "name": "ghost-skill"}])
    );

    // python3 zipfile 实际校验：CRC / 条目 / 字节一致 / 清单。
    let expected = json!({
        "names": [
            "bundle.json",
            "skills/export-one/SKILL.md",
            "skills/export-one/examples/example.txt",
            "skills/export-two/SKILL.md",
        ],
        "contents": {
            "skills/export-one/SKILL.md": std::fs::read_to_string(one.join("SKILL.md")).unwrap(),
            "skills/export-one/examples/example.txt":
                std::fs::read_to_string(one.join("examples/example.txt")).unwrap(),
            "skills/export-two/SKILL.md": std::fs::read_to_string(two.join("SKILL.md")).unwrap(),
        },
    });
    let verdict = python_zip_verdict(&zip_path, &expected);
    let manifest = &verdict["manifest"];
    assert_eq!(manifest["kind"], json!("SkillBundle"));
    assert_eq!(manifest["schemaVersion"], json!(1));
    assert_eq!(
        manifest["package"]["name"],
        json!("导出组合-alpha-skillbundle.zip")
    );
    assert!(manifest["package"]["exportedAt"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(manifest["bundle"]["name"], json!("导出组合 Alpha"));
    assert_eq!(manifest["bundle"]["source"], json!("user"));
    assert_eq!(manifest["bundle"]["sourcePackage"], json!(null));
    assert_eq!(
        manifest["skills"]["bundles"],
        json!([{
            "name": "导出组合 Alpha",
            "skills": [
                {"name": "export-one", "path": "skills/export-one"},
                {"name": "exported-heading", "path": "skills/export-two"},
            ],
        }])
    );

    // 源技能不动（导出是只读面）。
    assert_eq!(file_snapshot(&server.home.join("skills")), before_skills);
    // 临时导出根清理（.ephemeral/skill-bundle-exports 下无残留）。
    let ephemeral = server.home.join(".ephemeral").join("skill-bundle-exports");
    if ephemeral.is_dir() {
        let leftover = std::fs::read_dir(&ephemeral).unwrap().count();
        assert_eq!(leftover, 0, "临时导出根必须清理");
    }

    // 目标目录文件名唯一化：<slug>-skillbundle-2.zip。
    let (status, second) = ok(
        &server,
        "POST",
        &format!("/lingxi/v1/skills/bundles/{bundle_id}/export"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(
        second["fileName"],
        json!("导出组合-alpha-skillbundle-2.zip")
    );
    assert!(server
        .workspace()
        .join("导出组合-alpha-skillbundle-2.zip")
        .is_file());
    assert!(zip_path.is_file(), "首次导出的包保留");
    stop_and_clean(server).await;
}

/// 断言 2：id 不存在/打包失败明确错误；不扩大写入或披露范围。
#[tokio::test]
async fn export_rejections_are_explicit_without_expanding_writes() {
    let server = boot("ws-export-negative", true).await;
    let good = write_user_skill(&server.home, "good-skill");
    let store = bundles_store(&server);

    // 未知 id → 404。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/skills/bundles/no-such-bundle/export",
        None,
    )
    .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], json!("skill bundle not found"));

    // 空白 id（服务面直调，HTTP 路径参数不可能为空白）。
    let empty = server
        .state
        .skills()
        .export_skill_bundle("   ")
        .expect_err("空白 id 必须报错");
    assert_eq!(empty.status, 400);
    assert_eq!(empty.message, "bundle id is required");

    // 零可导出 → 400 + 目标目录无产物。
    store
        .save(&[
            json!({"id":"all-missing","name":"All Missing","skillNames":["ghost-a","ghost-b"]}),
        ])
        .unwrap();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/skills/bundles/all-missing/export",
        None,
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error"],
        json!("skill bundle has no exportable skills")
    );
    assert!(
        server.workspace().read_dir().unwrap().next().is_none(),
        "失败导出不得在目标目录留产物"
    );

    // 技能源含 symlink → 400，源保留、无产物、临时根清理。
    #[cfg(unix)]
    {
        let linked = server.home.join("skills").join("linked-skill");
        std::fs::create_dir_all(linked.join("examples")).expect("mkdir");
        std::fs::write(
            linked.join("SKILL.md"),
            "---\nname: linked-skill\n---\n正文",
        )
        .unwrap();
        std::os::unix::fs::symlink(linked.join("SKILL.md"), linked.join("examples/link.md"))
            .expect("symlink in package");
        store
            .save(&[json!({"id":"linked","name":"Linked","skillNames":["linked-skill"]})])
            .unwrap();
        let (status, body) = ok(
            &server,
            "POST",
            "/lingxi/v1/skills/bundles/linked/export",
            None,
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("skill contains symlink"),
            "{body}"
        );
        assert!(linked.join("SKILL.md").is_file(), "源保留");
        assert!(
            server.workspace().read_dir().unwrap().next().is_none(),
            "symlink 拒绝不留产物"
        );
        let ephemeral = server.home.join(".ephemeral").join("skill-bundle-exports");
        if ephemeral.is_dir() {
            assert_eq!(
                std::fs::read_dir(&ephemeral).unwrap().count(),
                0,
                "失败后临时根清理"
            );
        }
    }

    // 外部路径技能不在可导出范围（allowedRoots = 用户技能池，package-service.ts:120-122）。
    let external_dir = server.home.join("external-skills");
    let external_skill = write_workspace_skill(&external_dir, ".", "external-skill");
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/skills/external-paths",
        Some(&json!({"paths":[external_dir]})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    store
        .save(&[json!({"id":"mixed","name":"Mixed","skillNames":["good-skill","external-skill"]})])
        .unwrap();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/skills/bundles/mixed/export",
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["warnings"],
        json!([{"type": "missing-skill", "name": "external-skill"}]),
        "外部技能降级为 warning，不入包"
    );
    assert_eq!(body["bundle"]["skillCount"], json!(1));
    let zip_path = PathBuf::from(body["filePath"].as_str().unwrap());
    let verdict = python_zip_verdict(
        &zip_path,
        &json!({
            "names": [
                "bundle.json",
                "skills/good-skill/SKILL.md",
                "skills/good-skill/examples/example.txt",
            ],
            "contents": {
                "skills/good-skill/SKILL.md":
                    std::fs::read_to_string(good.join("SKILL.md")).unwrap(),
                "skills/good-skill/examples/example.txt":
                    std::fs::read_to_string(good.join("examples/example.txt")).unwrap(),
            },
        }),
    );
    assert_eq!(verdict["manifest"]["bundle"]["name"], json!("Mixed"));
    assert!(external_skill.join("SKILL.md").is_file(), "外部源不被触碰");
    assert_eq!(file_snapshot(&external_dir).len(), 2, "外部目录内容不变");

    // zip 落盘失败（目标目录只读）→ 500 + 无半成品。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(server.workspace())
            .unwrap()
            .permissions()
            .mode();
        std::fs::set_permissions(server.workspace(), std::fs::Permissions::from_mode(0o555))
            .unwrap();
        let (status, body) = ok(
            &server,
            "POST",
            "/lingxi/v1/skills/bundles/mixed/export",
            None,
        )
        .await;
        std::fs::set_permissions(server.workspace(), std::fs::Permissions::from_mode(mode))
            .unwrap();
        assert_eq!(status, 500, "打包失败必须显式 500：{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("write zip package"),
            "{body}"
        );
        let zips: Vec<_> = std::fs::read_dir(server.workspace())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".zip"))
            .collect();
        assert_eq!(zips.len(), 1, "失败导出不得新增/截断 zip：{zips:?}");
    }

    // 组合存储损坏 → 500（原语义 raw error 无 status → 500），文件不变。
    let bundle_file = server.home.join("skill-bundles.json");
    let valid = std::fs::read(&bundle_file).unwrap();
    let corrupt = b"corrupt bundle input".to_vec();
    std::fs::write(&bundle_file, &corrupt).unwrap();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/skills/bundles/mixed/export",
        None,
    )
    .await;
    assert_eq!(status, 500, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid skill bundles"),
        "{body}"
    );
    assert_eq!(
        std::fs::read(&bundle_file).unwrap(),
        corrupt,
        "损坏存储不被改写"
    );

    // 无工作区服务器：默认目标目录回退 lingxi home。
    std::fs::write(&bundle_file, &valid).unwrap();
    let bare = boot("ws-export-no-workspace", false).await;
    let bare_skill = write_user_skill(&bare.home, "fallback-skill");
    let bare_bundle = bundles_store(&bare)
        .create("回退组合", &["fallback-skill".into()], "user", None)
        .unwrap();
    let bare_id = bare_bundle["id"].as_str().unwrap().to_string();
    let (status, body) = ok(
        &bare,
        "POST",
        &format!("/lingxi/v1/skills/bundles/{bare_id}/export"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let bare_zip = PathBuf::from(body["filePath"].as_str().unwrap());
    assert!(
        bare_zip.starts_with(&bare.home),
        "无工作区时目标目录回退 home：{bare_zip:?}"
    );
    let verdict = python_zip_verdict(
        &bare_zip,
        &json!({
            "names": [
                "bundle.json",
                "skills/fallback-skill/SKILL.md",
                "skills/fallback-skill/examples/example.txt",
            ],
            "contents": {
                "skills/fallback-skill/SKILL.md":
                    std::fs::read_to_string(bare_skill.join("SKILL.md")).unwrap(),
                "skills/fallback-skill/examples/example.txt":
                    std::fs::read_to_string(bare_skill.join("examples/example.txt")).unwrap(),
            },
        }),
    );
    assert_eq!(verdict["manifest"]["bundle"]["name"], json!("回退组合"));
    let _ = std::fs::remove_dir_all(&external_dir);
    stop_and_clean(bare).await;
    stop_and_clean(server).await;
}
