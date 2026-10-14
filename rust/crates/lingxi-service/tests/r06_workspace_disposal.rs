//! R06 C1 收尾：工作台处置（POST /sessions/workspace-disposal）与孤儿清扫
//!（POST /sessions/sweep-orphaned-workspaces）的正反证明，全部经真实
//! HTTP + 真实 SQLite + 真实鉴权链（transport_guard → auth_guard）。
//! 现役锚点：server/routes/sessions.ts:2670 / :2762。
//!
//! 路由尚未在 lib.rs 的 build_router 注册（本任务不动 lib.rs），由
//! `session_admin::workspace_disposal_routes` 挂载同款鉴权链。
use std::path::PathBuf;
use std::time::Duration;

use lingxi_service::session_admin::{DisposalAction, SessionAdminService};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DISPOSAL: &str = "/lingxi/v1/sessions/workspace-disposal";
const SWEEP: &str = "/lingxi/v1/sessions/sweep-orphaned-workspaces";

struct Server {
    addr: String,
    token: String,
    home: PathBuf,
    state: ServiceState,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

async fn boot() -> Server {
    // 同进程内并行 boot 的 SystemTime 纳秒可能落在同一时钟粒度内（pid 相同），
    // 纯 pid+nanos 曾让两个测试引导同一 runs.db 并在迁移窗口互撞
    // （"table sessions already exists"）；原子序号保证 home 唯一。
    static BOOT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let uniq = format!(
        "{}-{}-{}",
        std::process::id(),
        BOOT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06-c1-wsdisposal-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("mkdir home");
    let home = home.canonicalize().expect("canonical home");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("loopback"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, ServiceDeps::default())
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let app = lingxi_service::session_admin::workspace_disposal_routes(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (stop, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        tokio::select! {
            _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            ) => {}
            _ = &mut stop_rx => {}
        }
    });
    Server {
        addr: addr.to_string(),
        token,
        home,
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
        let mut socket = tokio::net::TcpStream::connect(&server.addr).await.unwrap();
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
            serde_json::from_slice(body).expect("JSON 体")
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

/// 建一个本地主人名下的活动会话（服务层直建——被测面是处置/清扫，
/// 创建路由不在本挂载内）。
async fn mk_session(server: &Server, sid: &str) {
    server
        .state
        .session_admin()
        .create_session(
            sid,
            "agent-x",
            lingxi_service::auth::LOCAL_OWNER_USER_ID,
            &format!("会话 {sid}"),
            None,
            &[],
            true,
            1_000,
        )
        .await
        .expect("创建会话");
}

async fn set_workspace(server: &Server, sid: &str, mount: Option<&str>, cwd: Option<&str>) {
    let (status, body) = ok(
        server,
        "PUT",
        &format!("/lingxi/v1/sessions/{sid}/workspace"),
        Some(&json!({"workspaceMountId": mount, "cwd": cwd})),
    )
    .await;
    assert_eq!(status, 200, "PUT workspace {sid}：{body}");
}

async fn lifecycle(server: &Server, sid: &str) -> String {
    lingxi_adapters::storage::session_tree::get_session_row(server.state.storage(), sid)
        .await
        .expect("读会话行")
        .map(|row| row.lifecycle)
        .unwrap_or_else(|| "gone".to_string())
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06-wsdisposal-dir-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.canonicalize().expect("canonical")
}

fn write_registry(home: &std::path::Path, mounts: Value) {
    std::fs::write(
        home.join("studio-mounts.json"),
        serde_json::to_string_pretty(&json!({"schemaVersion": 1, "mounts": mounts, "createdAt": "2026-10-13T00:00:00.000Z", "updatedAt": "2026-10-13T00:00:00.000Z"})).unwrap(),
    )
    .expect("写 studio-mounts.json");
}

fn mount_entry(id: &str, status: &str) -> Value {
    json!({
        "schemaVersion": 1,
        "mountId": id,
        "hostStudioId": "studio_main",
        "sourceKind": "storage",
        "provider": "local_fs",
        "label": format!("挂载 {id}"),
        "presentation": "folder",
        "capabilities": ["list", "read"],
        "grantId": null,
        "status": status,
        "createdAt": "2026-10-13T00:00:00.000Z",
        "updatedAt": "2026-10-13T00:00:00.000Z",
    })
}

async fn stop(server: Server) {
    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
}

// ── 鉴权：无凭证 401；缺身份 400（原样错误体）────────────────────────────

#[tokio::test]
async fn disposal_and_sweep_require_credentials_and_identity() {
    let server = boot().await;

    for (method, path) in [("POST", DISPOSAL), ("POST", SWEEP)] {
        let (status, _) = http(&server, method, path, Some(&json!({})), None).await;
        assert_eq!(status, 401, "{path} 无凭证必须 401");
    }
    let (status, _) = http(
        &server,
        "PUT",
        "/lingxi/v1/sessions/ghost/workspace",
        Some(&json!({})),
        None,
    )
    .await;
    assert_eq!(status, 401, "PUT workspace 无凭证必须 401");

    // 缺身份：空 body / 空白字符串 / 只有 action。
    for payload in [
        json!({}),
        json!({"action": "archive"}),
        json!({"workspaceMountId": "   ", "cwd": "  "}),
    ] {
        let (status, body) = ok(&server, "POST", DISPOSAL, Some(&payload)).await;
        assert_eq!(status, 400, "缺身份必须 400：{payload} → {body}");
        assert_eq!(body["error"], json!("missing workspace identity"));
        assert_eq!(body["code"], json!("missing_workspace_identity"));
    }

    // 孤儿清扫不读 body：空对象即合法。bootstrap 会种两个合成会话
    //（sess_local_alpha/beta，无工作台身份）——计入 scanned、不参与清扫。
    let (status, body) = ok(&server, "POST", SWEEP, Some(&json!({}))).await;
    assert_eq!(status, 200, "清扫空库 200：{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["scanned"], json!(2), "合成种子会话计入扫描面");
    assert_eq!(body["archived"], json!(0));

    stop(server).await;
}

// ── PUT workspace：落库 / 清除 / 404 / 归档后 409 ─────────────────────────

#[tokio::test]
async fn workspace_put_roundtrip_clear_and_rejections() {
    let server = boot().await;
    mk_session(&server, "s-ws").await;

    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/s-ws/workspace",
        Some(&json!({"workspaceMountId": " mount-a ", "cwd": " /tmp/some/dir "})),
    )
    .await;
    assert_eq!(status, 200, "PUT：{body}");
    assert_eq!(body["workspaceMountId"], json!("mount-a"), "trim 后落库");
    assert_eq!(body["cwd"], json!("/tmp/some/dir"), "trim 后落库");

    let row = lingxi_adapters::storage::session_admin::list_active_session_workspaces(
        server.state.storage(),
        None,
    )
    .await
    .expect("扫描");
    let mine = row.iter().find(|r| r.session_id == "s-ws").expect("行");
    assert_eq!(mine.workspace_mount_id.as_deref(), Some("mount-a"));
    assert_eq!(mine.cwd.as_deref(), Some("/tmp/some/dir"));

    // 空串 = 清除（NULL）。
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/s-ws/workspace",
        Some(&json!({"workspaceMountId": "", "cwd": ""})),
    )
    .await;
    assert_eq!(status, 200, "清除：{body}");
    assert_eq!(body["workspaceMountId"], serde_json::Value::Null);
    assert_eq!(body["cwd"], serde_json::Value::Null);

    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/ghost/workspace",
        Some(&json!({})),
    )
    .await;
    assert_eq!(status, 404, "未知会话 404");

    // 归档后生命周期拒绝。
    server
        .state
        .session_admin()
        .archive_session("s-ws", None, 2_000)
        .await
        .expect("归档");
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/s-ws/workspace",
        Some(&json!({"workspaceMountId": "late"})),
    )
    .await;
    assert_eq!(status, 409, "归档后写入 409");

    stop(server).await;
}

// ── 处置：mountId 直配 / cwd 双形态（canonical、trim 回退）/ 不误伤 ───────

#[tokio::test]
async fn disposal_matches_mount_and_cwd_forms() {
    let server = boot().await;
    let dir_cwd = temp_dir("cwd");
    let dir_link_target = temp_dir("linktarget");
    let dir_removed = temp_dir("willremove");

    for sid in [
        "s-m1", "s-m2", "s-other", "s-cwd", "s-link", "s-gone", "s-none",
    ] {
        mk_session(&server, sid).await;
    }
    for (sid, mount, cwd) in [
        ("s-m1", Some("ws-a"), None),
        ("s-m2", Some("ws-a"), None),
        ("s-other", Some("ws-b"), None),
        ("s-cwd", None, Some(dir_cwd.to_str().unwrap())),
        ("s-link", None, Some(dir_link_target.to_str().unwrap())),
        ("s-gone", None, Some(dir_removed.to_str().unwrap())),
    ] {
        set_workspace(&server, sid, mount, cwd).await;
    }
    // 目录先落库再删除：trim 回退形态的处置身份。
    std::fs::remove_dir_all(&dir_removed).expect("删目录");

    // mountId 直配：只动 ws-a 名下两本账。
    let (status, body) = ok(
        &server,
        "POST",
        DISPOSAL,
        Some(&json!({"workspaceMountId": "ws-a"})),
    )
    .await;
    assert_eq!(status, 200, "处置 ws-a：{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["action"], json!("archive"));
    assert_eq!(body["matched"], json!(2));
    assert_eq!(body["disposed"], json!(2));
    assert_eq!(body["skippedStreaming"], json!(0));
    assert_eq!(body["errors"], json!([]));
    assert_eq!(lifecycle(&server, "s-m1").await, "archived");
    assert_eq!(lifecycle(&server, "s-m2").await, "archived");
    assert_eq!(
        lifecycle(&server, "s-other").await,
        "active",
        "别的工作台不动"
    );
    assert_eq!(lifecycle(&server, "s-cwd").await, "active");

    // cwd 字面相等。
    let (_, body) = ok(
        &server,
        "POST",
        DISPOSAL,
        Some(&json!({"cwd": dir_cwd.to_str().unwrap()})),
    )
    .await;
    assert_eq!(body["matched"], json!(1), "cwd 命中一本：{body}");
    assert_eq!(body["disposed"], json!(1));
    assert_eq!(lifecycle(&server, "s-cwd").await, "archived");

    // cwd 符号链接形态 → canonical 相等（macOS /tmp 与 /private/tmp 同库）。
    let link =
        std::env::temp_dir().join(format!("lingxi-r06-wsdisposal-link-{}", std::process::id()));
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&dir_link_target, &link).expect("建符号链接");
    let (_, body) = ok(
        &server,
        "POST",
        DISPOSAL,
        Some(&json!({"cwd": link.to_str().unwrap()})),
    )
    .await;
    assert_eq!(body["matched"], json!(1), "符号链接归一后命中：{body}");
    assert_eq!(body["disposed"], json!(1));
    assert_eq!(lifecycle(&server, "s-link").await, "archived");
    let _ = std::fs::remove_file(&link);

    // cwd 指向已删除目录：canonicalize 失败 → trim 相等仍命中（工作台先删
    // 目录再处置的现役路径）。
    let (_, body) = ok(
        &server,
        "POST",
        DISPOSAL,
        Some(&json!({"cwd": dir_removed.to_str().unwrap()})),
    )
    .await;
    assert_eq!(body["matched"], json!(1), "已删目录按 trim 命中：{body}");
    assert_eq!(body["disposed"], json!(1));
    assert_eq!(lifecycle(&server, "s-gone").await, "archived");

    // 无工作台身份的会话任何处置都不命中。
    assert_eq!(lifecycle(&server, "s-none").await, "active");

    stop(server).await;
    for dir in [dir_cwd, dir_link_target] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

// ── 处置：action=delete 行真删 + 输入草稿清理 ─────────────────────────────

#[tokio::test]
async fn disposal_delete_removes_rows_and_input_drafts() {
    let server = boot().await;
    mk_session(&server, "s-del").await;
    set_workspace(&server, "s-del", Some("ws-del"), None).await;

    // 现役 input-drafts.v1.json 形态：两个 surface 的 sessions 桶。
    let drafts = json!({
        "version": 1,
        "surfaces": {
            "electron": {"home": {"text": "首页草稿", "updatedAt": 1}, "sessions": {"s-del": {"text": "被删会话的草稿", "updatedAt": 2}}},
            "pwa": {"home": null, "sessions": {}}
        }
    });
    std::fs::write(
        server.home.join("input-drafts.v1.json"),
        serde_json::to_string(&drafts).unwrap(),
    )
    .expect("种草稿");

    let (status, body) = ok(
        &server,
        "POST",
        DISPOSAL,
        Some(&json!({"action": "delete", "workspaceMountId": "ws-del"})),
    )
    .await;
    assert_eq!(status, 200, "delete：{body}");
    assert_eq!(body["action"], json!("delete"));
    assert_eq!(body["matched"], json!(1));
    assert_eq!(body["disposed"], json!(1));
    assert_eq!(body["errors"], json!([]));

    // 行真删（含归档态——不是归档后残留）。
    let row =
        lingxi_adapters::storage::session_tree::get_session_row(server.state.storage(), "s-del")
            .await
            .expect("查询");
    assert!(row.is_none(), "delete 后行必须消失");

    // 草稿条目清理，首页草稿保留。
    let after: Value = serde_json::from_slice(
        &std::fs::read(server.home.join("input-drafts.v1.json")).expect("草稿文件仍在"),
    )
    .expect("合法 JSON");
    assert!(
        after["surfaces"]["electron"]["sessions"]
            .as_object()
            .is_some_and(|m| !m.contains_key("s-del")),
        "被删会话的草稿必须清理：{after}"
    );
    assert_eq!(
        after["surfaces"]["electron"]["home"]["text"],
        json!("首页草稿")
    );

    // 对照：archive 只归档不删行。
    mk_session(&server, "s-arch").await;
    set_workspace(&server, "s-arch", Some("ws-arch"), None).await;
    let (_, body) = ok(
        &server,
        "POST",
        DISPOSAL,
        Some(&json!({"workspaceMountId": "ws-arch"})),
    )
    .await;
    assert_eq!(body["disposed"], json!(1));
    assert_eq!(
        lifecycle(&server, "s-arch").await,
        "archived",
        "归档态行保留"
    );

    stop(server).await;
}

// ── 处置：流式跳过 + 逐项失败进 errors（不中断）─────────────────────────

#[tokio::test]
async fn disposal_skips_busy_and_collects_per_session_errors() {
    let server = boot().await;
    for sid in ["s-busy", "s-kids", "s-ok"] {
        mk_session(&server, sid).await;
        set_workspace(&server, sid, Some("ws-x"), None).await;
    }
    // 子对话：s-kids 有直接子 → archive 拒绝 child_sessions_present（现役
    // 经 archiveActiveSessionCore 不查子对话直接搬文件；Rust 侧复用既有
    // archive_session 原语，宁报错不静默孤儿化谱系，按条目进 errors）。
    mk_session(&server, "s-child").await;
    server
        .state
        .session_admin()
        .set_session_lineage("s-child", "s-kids", 1)
        .await
        .expect("挂谱系");
    // 流式闸桩：占住 s-busy 的会话准入（在跑 run 同款占用）。
    let permit = server
        .state
        .storage()
        .session_admission()
        .try_acquire(
            &["s-busy"],
            lingxi_service::session_supervisor::SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
        )
        .expect("占住准入");

    let (status, body) = ok(
        &server,
        "POST",
        DISPOSAL,
        Some(&json!({"workspaceMountId": "ws-x"})),
    )
    .await;
    assert_eq!(status, 200, "批处置 200：{body}");
    assert_eq!(body["matched"], json!(3));
    assert_eq!(body["disposed"], json!(1), "只有 s-ok 处置成：{body}");
    assert_eq!(body["skippedStreaming"], json!(1), "流式中的跳过并计数");
    let errors = body["errors"].as_array().expect("errors 数组");
    assert_eq!(errors.len(), 1, "子对话冲突进 errors：{body}");
    assert_eq!(errors[0]["path"], json!("s-kids"));
    assert!(
        errors[0]["message"]
            .as_str()
            .is_some_and(|m| m.contains("child_sessions_present")),
        "message 口径：{}",
        errors[0]
    );
    assert_eq!(lifecycle(&server, "s-ok").await, "archived");
    assert_eq!(lifecycle(&server, "s-busy").await, "active", "流式中不动");
    assert_eq!(lifecycle(&server, "s-kids").await, "active", "失败项不动");
    drop(permit);

    // 释放后重跑：busy 项可处置（child 项仍按 errors 上报）。
    let (_, body) = ok(
        &server,
        "POST",
        DISPOSAL,
        Some(&json!({"workspaceMountId": "ws-x"})),
    )
    .await;
    assert_eq!(body["matched"], json!(2), "s-kids 与 s-busy 仍在活动面");
    assert_eq!(body["disposed"], json!(1), "s-busy 处置成");
    assert_eq!(body["skippedStreaming"], json!(0));
    assert_eq!(body["errors"].as_array().unwrap().len(), 1);

    stop(server).await;
}

// ── 清扫：两种孤儿 + 非孤儿不动 + 注册表正常读取 ─────────────────────────

#[tokio::test]
async fn sweep_archives_mount_and_dir_orphans_only() {
    let server = boot().await;
    let dir_alive = temp_dir("alive");
    let dir_dead = temp_dir("dead");

    write_registry(
        &server.home,
        json!([
            mount_entry("m-live", "active"),
            mount_entry("m-off", "disabled")
        ]),
    );

    for (sid, mount, cwd) in [
        ("s-live", Some("m-live"), None),
        ("s-off", Some("m-off"), None),
        ("s-default", Some("default"), None),
        ("s-dir-alive", None, Some(dir_alive.to_str().unwrap())),
        ("s-dir-dead", None, Some(dir_dead.to_str().unwrap())),
        ("s-none", None, None),
    ] {
        mk_session(&server, sid).await;
        set_workspace(&server, sid, mount, cwd).await;
    }
    std::fs::remove_dir_all(&dir_dead).expect("目录被直接删除");

    let (status, body) = ok(&server, "POST", SWEEP, Some(&json!({}))).await;
    assert_eq!(status, 200, "清扫：{body}");
    assert_eq!(body["ok"], json!(true));
    // 6 个夹具会话 + 2 个合成种子（无身份、非孤儿）都计入扫描面。
    assert_eq!(body["scanned"], json!(8), "全部活动会话计入扫描面");
    assert_eq!(body["archived"], json!(2), "两种孤儿各一本：{body}");
    assert_eq!(body["skippedStreaming"], json!(0));
    assert_eq!(body["errors"], json!([]));
    assert_eq!(
        lifecycle(&server, "s-off").await,
        "archived",
        "挂载被移除的孤儿"
    );
    assert_eq!(
        lifecycle(&server, "s-dir-dead").await,
        "archived",
        "目录消失的孤儿"
    );
    assert_eq!(
        lifecycle(&server, "s-live").await,
        "active",
        "活动挂载不清扫"
    );
    assert_eq!(
        lifecycle(&server, "s-default").await,
        "active",
        "default 挂载不清扫"
    );
    assert_eq!(
        lifecycle(&server, "s-dir-alive").await,
        "active",
        "目录仍在的 cwd 会话不清扫"
    );
    assert_eq!(
        lifecycle(&server, "s-none").await,
        "active",
        "无身份会话不清扫"
    );

    // 再扫一遍幂等（无孤儿可清；剩 4 夹具 + 2 种子）。
    let (_, body) = ok(&server, "POST", SWEEP, Some(&json!({}))).await;
    assert_eq!(body["archived"], json!(0));
    assert_eq!(body["scanned"], json!(6));

    stop(server).await;
    let _ = std::fs::remove_dir_all(dir_alive);
}

// ── 清扫：注册表缺失 / 损坏按仅 default（宁可不清扫也不误归档）──────────

#[tokio::test]
async fn sweep_registry_missing_or_corrupt_falls_back_to_default_only() {
    // 缺失：非 default 挂载会话都成孤儿，但 default 与现存 cwd 会话不动。
    let server = boot().await;
    let dir_alive = temp_dir("alive2");
    for (sid, mount, cwd) in [
        ("s-mt", Some("m-any"), None),
        ("s-default", Some("default"), None),
        ("s-dir", None, Some(dir_alive.to_str().unwrap())),
        ("s-none", None, None),
    ] {
        mk_session(&server, sid).await;
        set_workspace(&server, sid, mount, cwd).await;
    }
    let (_, body) = ok(&server, "POST", SWEEP, Some(&json!({}))).await;
    assert_eq!(body["scanned"], json!(6), "4 夹具 + 2 合成种子");
    assert_eq!(
        body["archived"],
        json!(1),
        "只清非 default 挂载孤儿：{body}"
    );
    assert_eq!(lifecycle(&server, "s-mt").await, "archived");
    assert_eq!(lifecycle(&server, "s-default").await, "active");
    assert_eq!(lifecycle(&server, "s-dir").await, "active");
    assert_eq!(lifecycle(&server, "s-none").await, "active");

    // 损坏（非法 JSON 字节）：同仅 default 口径，且不崩。
    std::fs::write(server.home.join("studio-mounts.json"), b"{not-json").expect("写坏注册表");
    let (status, body) = ok(&server, "POST", SWEEP, Some(&json!({}))).await;
    assert_eq!(status, 200, "注册表损坏不 500：{body}");
    assert_eq!(body["archived"], json!(0), "无新孤儿");

    // 形状损坏（mounts 非数组）：同上。
    std::fs::write(
        server.home.join("studio-mounts.json"),
        serde_json::to_string(&json!({"schemaVersion": 1, "mounts": 42})).unwrap(),
    )
    .expect("写形状损坏注册表");
    let (status, body) = ok(&server, "POST", SWEEP, Some(&json!({}))).await;
    assert_eq!(status, 200, "形状损坏不 500：{body}");
    assert_eq!(body["archived"], json!(0));

    stop(server).await;
    let _ = std::fs::remove_dir_all(dir_alive);
}

// ── 清扫：流式中的孤儿跳过并计数 ─────────────────────────────────────────

#[tokio::test]
async fn sweep_skips_streaming_orphans() {
    let server = boot().await;
    mk_session(&server, "s-orphan").await;
    set_workspace(&server, "s-orphan", Some("m-gone"), None).await;
    let permit = server
        .state
        .storage()
        .session_admission()
        .try_acquire(
            &["s-orphan"],
            lingxi_service::session_supervisor::SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
        )
        .expect("占住准入");

    let (_, body) = ok(&server, "POST", SWEEP, Some(&json!({}))).await;
    assert_eq!(body["scanned"], json!(3), "1 夹具 + 2 合成种子");
    assert_eq!(body["archived"], json!(0), "流式中的孤儿不动：{body}");
    assert_eq!(body["skippedStreaming"], json!(1), "跳过并计数");
    assert_eq!(lifecycle(&server, "s-orphan").await, "active");
    drop(permit);

    let (_, body) = ok(&server, "POST", SWEEP, Some(&json!({}))).await;
    assert_eq!(body["archived"], json!(1), "释放后补扫");
    assert_eq!(body["skippedStreaming"], json!(0));

    stop(server).await;
}

// ── 跨主体不越权：设备主体视野外的会话不进任何处置账 ──────────────────────

#[tokio::test]
async fn cross_principal_sessions_never_enter_disposal_or_sweep() {
    let server = boot().await;
    mk_session(&server, "s-owner").await;
    set_workspace(&server, "s-owner", Some("ws-cp"), None).await;

    // 服务层 owner 过滤（现役 listSessions 的主体视野）：设备主体只看自己。
    let admin: &lingxi_service::session_admin::SessionAdminService = server.state.session_admin();
    let view = SessionAdminService::dispose_workspace_sessions(
        admin,
        Some("user_remote"),
        DisposalAction::Archive,
        Some("ws-cp"),
        None,
        3_000,
    )
    .await
    .expect("服务调用");
    assert_eq!(view.matched, 0, "跨主体会话不进 matched");
    assert_eq!(view.disposed, 0);
    assert_eq!(lifecycle(&server, "s-owner").await, "active");

    let mounts = std::collections::BTreeSet::from(["default".to_string()]);
    let sweep =
        SessionAdminService::sweep_orphaned_sessions(admin, Some("user_remote"), &mounts, 3_000)
            .await
            .expect("服务调用");
    assert_eq!(sweep.scanned, 0, "跨主体会话不进扫描面");
    assert_eq!(sweep.archived, 0);

    // 本地主人视野照旧：处置可命中。
    let view = SessionAdminService::dispose_workspace_sessions(
        admin,
        None,
        DisposalAction::Archive,
        Some("ws-cp"),
        None,
        3_000,
    )
    .await
    .expect("服务调用");
    assert_eq!(view.matched, 1);
    assert_eq!(view.disposed, 1);
    assert_eq!(lifecycle(&server, "s-owner").await, "archived");

    stop(server).await;
}
