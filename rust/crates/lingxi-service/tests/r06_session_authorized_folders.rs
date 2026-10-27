//! R06 会话授权目录（现役 GET/PATCH /sessions/authorized-folders 端口）：
//! 读 / add / remove / set 的正反证明，全部经真实 HTTP + 真实 SQLite。
use std::path::PathBuf;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    addr: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-auth-folders-{uniq}"));
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
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<std::net::SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = ready_rx.await.expect("readiness");
    Server {
        addr: addr.to_string(),
        token,
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

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06-auth-folders-dir-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.canonicalize().expect("canonical")
}

#[tokio::test]
async fn authorized_folders_roundtrip_and_rejections() {
    let server = boot().await;
    let d1 = temp_dir("one");
    let d2 = temp_dir("two");
    let d3 = temp_dir("three");
    let file_path = d1.join("plain.txt");
    std::fs::write(&file_path, "文件不是目录").unwrap();

    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions",
        Some(&json!({"sessionId": "s1", "agentId": "agent-x", "title": "授权目录证明"})),
    )
    .await;
    assert_eq!(status, 201, "会话创建");

    // 读取：初始为空。
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/s1/authorized-folders",
        None,
    )
    .await;
    assert_eq!(status, 200, "读取：{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["authorizedFolders"], json!([]));
    assert_eq!(body["workspaceFolders"], json!([]));
    assert_eq!(body["sandboxFolders"], json!([]));

    // add：成功 / 规范化 / 去重 / 反例。
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "add", "folder": d1})),
    )
    .await;
    assert_eq!(status, 200, "add d1：{body}");
    assert_eq!(body["authorizedFolders"], json!([d1.to_str().unwrap()]));

    let (_, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "add", "folder": d1.join(".").to_str().unwrap()})),
    )
    .await;
    assert_eq!(
        body["authorizedFolders"],
        json!([d1.to_str().unwrap()]),
        "重复 add 幂等"
    );

    for (payload, label) in [
        (
            json!({"action": "add", "folder": "/definitely/not/here"}),
            "不存在目录",
        ),
        (
            json!({"action": "add", "folder": file_path}),
            "文件不是目录",
        ),
        (json!({"action": "add"}), "缺 folder"),
    ] {
        let (status, body) = ok(
            &server,
            "PATCH",
            "/lingxi/v1/sessions/s1/authorized-folders",
            Some(&payload),
        )
        .await;
        assert_eq!(status, 400, "{label} 必须 400：{body}");
    }
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/s1/authorized-folders",
        None,
    )
    .await;
    assert_eq!(
        body["authorizedFolders"],
        json!([d1.to_str().unwrap()]),
        "失败后列表不变"
    );

    // set：整体替换 / 坏条目拒绝且不动旧值。
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "set", "folders": [d2, d3]})),
    )
    .await;
    assert_eq!(status, 200, "set：{body}");
    assert_eq!(
        body["authorizedFolders"],
        json!([d2.to_str().unwrap(), d3.to_str().unwrap()])
    );
    let (status, _) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "set", "folders": [d1, "/definitely/not/here"]})),
    )
    .await;
    assert_eq!(status, 400, "set 坏条目 400");
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/s1/authorized-folders",
        None,
    )
    .await;
    assert_eq!(
        body["authorizedFolders"],
        json!([d2.to_str().unwrap(), d3.to_str().unwrap()]),
        "set 失败保持原列表"
    );
    let (status, _) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "set"})),
    )
    .await;
    assert_eq!(status, 400, "set 缺 folders 400");

    // remove：命中 / 容忍缺席。
    let (_, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "remove", "folder": d2})),
    )
    .await;
    assert_eq!(body["authorizedFolders"], json!([d3.to_str().unwrap()]));
    let (_, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "remove", "folder": d2})),
    )
    .await;
    assert_eq!(
        body["authorizedFolders"],
        json!([d3.to_str().unwrap()]),
        "移除缺席幂等"
    );

    // 未知 action / 未知会话 / 归档后生命周期拒绝 / 鉴权。
    let (status, _) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "bogus"})),
    )
    .await;
    assert_eq!(status, 400, "未知 action 400");
    let (status, _) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ghost/authorized-folders",
        None,
    )
    .await;
    assert_eq!(status, 404, "未知会话 404");

    // 跨会话隔离：s1 的批准/替换只作用于 s1，不扩大其他会话的范围。
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions",
        Some(&json!({"sessionId": "s2", "agentId": "agent-x", "title": "隔离对照"})),
    )
    .await;
    assert_eq!(status, 201, "对照会话创建");
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/s2/authorized-folders",
        None,
    )
    .await;
    assert_eq!(
        body["authorizedFolders"],
        json!([]),
        "对照会话初始范围应为空"
    );
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "add", "folder": d1})),
    )
    .await;
    assert_eq!(status, 200, "s1 再批准：{body}");
    assert_eq!(
        body["authorizedFolders"],
        json!([d3.to_str().unwrap(), d1.to_str().unwrap()]),
        "s1 范围按 add 语义增长"
    );
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/s2/authorized-folders",
        None,
    )
    .await;
    assert_eq!(
        body["authorizedFolders"],
        json!([]),
        "s1 的批准不得扩大 s2 的范围"
    );

    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/archive",
        Some(&json!({"sessionId": "s1"})),
    )
    .await;
    assert_eq!(status, 200, "归档成功");
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/s1/authorized-folders",
        None,
    )
    .await;
    assert_eq!(status, 409, "归档后读取 409：{body}");
    let (status, _) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/sessions/s1/authorized-folders",
        Some(&json!({"action": "add", "folder": d1})),
    )
    .await;
    assert_eq!(status, 409, "归档后写入 409");

    let (status, _) = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/s1/authorized-folders",
        None,
        None,
    )
    .await;
    assert_eq!(status, 401, "无凭证 401");

    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    for dir in [&d1, &d2, &d3] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
