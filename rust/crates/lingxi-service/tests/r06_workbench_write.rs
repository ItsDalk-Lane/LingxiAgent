//! R06-R2（EE18A57F7348）：现役 workbench writeText 保存面
//! （core/mount-aware-file-service.ts:143-181 + mobile-workbench 路由
//! :99-100 的端口映射）——同版本写入新内容与新版本；版本冲突保持外部
//! 内容并显式保存失败；非法名字显式拒绝；「封面冲突基于新快照最多再试」
//! 的编排链（stat → 冲突 → 再 stat → 重试；再失败 → 显式冲突错误）。
//!
//! 夹具：真 TCP loopback + 真实 ResourceIO 工作区平面（bootstrap 注入
//! workspace_root），处理器为 pub（注册片段留主控），测试内挂载。
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, ConnectionKind, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Server {
    addr: SocketAddr,
    workspace: PathBuf,
    home: PathBuf,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

async fn auth_guard(
    axum::extract::State(state): axum::extract::State<ServiceState>,
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let authorization = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    match state
        .auth()
        .authenticate(authorization.as_deref(), None, false, ConnectionKind::Local)
    {
        Ok(principal) => {
            req.extensions_mut().insert(principal);
            next.run(req).await
        }
        Err(_) => (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "invalid_credential" })),
        )
            .into_response(),
    }
}

async fn boot(tag: &str) -> Server {
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
    let home = std::env::temp_dir().join(format!("lingxi-r06r2-wb-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("mkdir home");
    let home = home.canonicalize().expect("canonical home");
    let workspace = home.join("workspace");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("loopback"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(
        config,
        &layout,
        ServiceDeps {
            workspace_root: Some(workspace.clone()),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");
    let token = state.auth().local_token().to_string();
    let router = axum::Router::new()
        .route(
            "/lingxi/v1/workbench/actions",
            axum::routing::post(lingxi_service::resourceio::workbench_write_text_route),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_guard,
        ))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await
            .expect("workbench fixture HTTP 服务");
    });
    Server {
        addr,
        workspace,
        home,
        token,
        stop,
        handle,
    }
}

async fn action(server: &Server, body: &serde_json::Value) -> (u16, serde_json::Value) {
    let task = async {
        let mut socket = TcpStream::connect(server.addr).await.unwrap();
        let payload = body.to_string();
        let request = format!(
            "POST /lingxi/v1/workbench/actions HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{payload}",
            addr = server.addr,
            token = server.token,
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
        let value = serde_json::from_slice(&response[boundary + 4..]).expect("JSON 体");
        (status, value)
    };
    tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .expect("HTTP 往返在限时内")
}

/// 版本对象（stat 同源）：从响应里取 version 的 {mtimeMs, size}。
fn version_of(value: &serde_json::Value) -> serde_json::Value {
    value["version"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workbench_write_text_same_version_conflict_and_retry_chain() {
    let server = boot("write").await;
    let note = server.workspace.join("note.md");
    std::fs::write(&note, "old").expect("seed note");

    // ── 无 expectedVersion 的普通写入（现役 write 分支）──
    let (status, body) = action(
        &server,
        &serde_json::json!({ "action": "writeText", "name": "note.md", "content": "plain" }),
    )
    .await;
    assert_eq!(status, 200, "plain write: {body}");
    assert_eq!(body["ok"], true, "普通写入成功");
    assert!(body["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["name"] == "note.md"));
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "plain");

    // ── 陈旧版本 → 冲突：ok:false + conflict:true + 当前版本，磁盘不动 ──
    std::fs::write(&note, "v1").expect("reset v1");
    let stale_meta = std::fs::metadata(&note).unwrap();
    let stale_version = serde_json::json!({
        "mtimeMs": stale_mtime_minus_one(&stale_meta),
        "size": stale_meta.len(),
    });
    let (status, body) = action(
        &server,
        &serde_json::json!({
            "action": "writeText",
            "name": "note.md",
            "content": "should not write",
            "expectedVersion": stale_version,
        }),
    )
    .await;
    assert_eq!(status, 200, "冲突是结果不是异常（现役 :160-171）: {body}");
    assert_eq!(body["ok"], false, "显式保存失败");
    assert_eq!(body["conflict"], true);
    assert_eq!(
        body["version"]["size"],
        serde_json::json!(stale_meta.len()),
        "返回当前版本"
    );
    assert_eq!(
        std::fs::read_to_string(&note).unwrap(),
        "v1",
        "版本冲突保持外部内容"
    );

    // ── 同版本 → ok:true + 新版本 + 磁盘内容吻合 ──
    let current_version = version_of(&body);
    let (status, body) = action(
        &server,
        &serde_json::json!({
            "action": "writeText",
            "name": "note.md",
            "content": "new body",
            "expectedVersion": current_version,
        }),
    )
    .await;
    assert_eq!(status, 200, "same-version save: {body}");
    assert_eq!(body["ok"], true, "同版本保存成功");
    assert_eq!(
        body["version"]["size"],
        serde_json::json!("new body".len()),
        "新版本反映新内容尺寸"
    );
    assert_eq!(
        std::fs::read_to_string(&note).unwrap(),
        "new body",
        "磁盘内容与新版本吻合"
    );

    // ── 封面冲突重试链：冲突 → 基于新快照再试成功 ──
    // 第一击用旧快照 → 冲突；re-stat 拿新快照；第二击同版本成功。
    let v_new_body = version_of(&body);
    // 外部再改一次（模拟封面变更竞争）。
    std::fs::write(&note, "externally-changed").expect("external write");
    let (status, first_try) = action(
        &server,
        &serde_json::json!({
            "action": "writeText",
            "name": "note.md",
            "content": "retry attempt",
            "expectedVersion": v_new_body,
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(first_try["conflict"], true, "外部已改 → 重试首击冲突");
    let fresh_snapshot = version_of(&first_try);
    let (status, retried) = action(
        &server,
        &serde_json::json!({
            "action": "writeText",
            "name": "note.md",
            "content": "retry attempt",
            "expectedVersion": fresh_snapshot,
        }),
    )
    .await;
    assert_eq!(status, 200, "基于新快照的重试: {retried}");
    assert_eq!(retried["ok"], true, "再试成功");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "retry attempt");

    // ── 重试仍失败 → 显式错误（不显示保存成功）──
    std::fs::write(&note, "changed-again").expect("外部第三次修改");
    let (status, again) = action(
        &server,
        &serde_json::json!({
            "action": "writeText",
            "name": "note.md",
            "content": "wont land",
            "expectedVersion": fresh_snapshot,
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(again["ok"], false, "仍失败给冲突错误");
    assert_eq!(again["conflict"], true);
    assert_eq!(
        std::fs::read_to_string(&note).unwrap(),
        "changed-again",
        "失败的保存不覆盖用户后改"
    );

    // ── 非法名字 / 未知 action：显式 400，不呈现保存成功 ──
    for bad_name in ["../evil.txt", "a/b.txt", ".", ""] {
        let (status, body) = action(
            &server,
            &serde_json::json!({ "action": "writeText", "name": bad_name, "content": "x" }),
        )
        .await;
        assert_eq!(status, 400, "非法名字 {bad_name:?} 必须 400: {body}");
        assert_eq!(body["error"], "invalid_name");
    }
    let (status, body) = action(
        &server,
        &serde_json::json!({ "action": "purge", "name": "note.md" }),
    )
    .await;
    assert_eq!(status, 400, "未知 action: {body}");
    assert_eq!(body["error"], "unknown_action");
    assert_eq!(
        std::fs::read_to_string(&note).unwrap(),
        "changed-again",
        "拒绝路径不落盘"
    );

    server.stop.send(()).ok();
    let _ = server.handle.await;
    let _ = std::fs::remove_dir_all(&server.home);
}

/// mtimeMs 减一毫秒（构造陈旧但不畸形的版本对象）。
fn stale_mtime_minus_one(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .saturating_sub(1)
}
