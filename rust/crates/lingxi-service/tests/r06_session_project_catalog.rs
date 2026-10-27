//! R06 会话项目/文件夹目录（现役 /session-projects* 全量面端口）：
//! 创建/修改/删除/重排/目录树/项目删除会话改派的正反证明。
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
    home: PathBuf,
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-projects-{uniq}"));
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
        home,
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

#[tokio::test]
async fn project_folder_catalog_roundtrip_and_rejections() {
    let server = boot().await;

    // 目录初始为空。
    let (status, body) = ok(&server, "GET", "/lingxi/v1/session-projects", None).await;
    assert_eq!(status, 200, "目录读取：{body}");
    assert_eq!(body["catalog"]["folders"], json!([]));
    assert_eq!(body["catalog"]["projects"], json!([]));

    // 创建文件夹与项目：成功 / 空名折叠 / 未知文件夹。
    let (_, f1) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/folders",
        Some(&json!({"name": "  工作   区  "})),
    )
    .await;
    assert_eq!(f1["folder"]["name"], json!("工作 区"), "空白折叠");
    let f1_id = f1["folder"]["id"].as_str().unwrap().to_string();
    let (_, p1) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/projects",
        Some(&json!({"name": "项目一", "folderId": f1_id})),
    )
    .await;
    let p1_id = p1["project"]["id"].as_str().unwrap().to_string();
    assert_eq!(p1["project"]["folderId"], json!(f1_id));
    let (_, p2) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/projects",
        Some(&json!({"name": "根项目"})),
    )
    .await;
    assert_eq!(p2["project"]["folderId"], Value::Null);
    let p2_id = p2["project"]["id"].as_str().unwrap().to_string();

    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/folders",
        Some(&json!({"name": "  "})),
    )
    .await;
    assert_eq!(status, 400, "文件夹空名 400");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/projects",
        Some(&json!({"name": "X", "folderId": "ghost"})),
    )
    .await;
    assert_eq!(status, 400, "未知文件夹 400");
    let (status, _) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/session-projects/projects/ghost",
        Some(&json!({"name": "Y"})),
    )
    .await;
    assert_eq!(status, 400, "未知项目 400");

    // 修改：改名 / 移动到根（order 接尾）。
    let (_, p1b) = ok(
        &server,
        "PATCH",
        &format!("/lingxi/v1/session-projects/projects/{p1_id}"),
        Some(&json!({"name": "项目一改", "folderId": null})),
    )
    .await;
    assert_eq!(p1b["project"]["name"], json!("项目一改"));
    assert_eq!(p1b["project"]["folderId"], Value::Null);
    assert_eq!(p1b["project"]["order"], json!(1), "移动后 order 接在根末尾");

    // 重排：文件夹全局 / 项目作用域内。
    let (_, f2) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/folders",
        Some(&json!({"name": "归档"})),
    )
    .await;
    let f2_id = f2["folder"]["id"].as_str().unwrap().to_string();
    let (_, cat) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/folders/reorder",
        Some(&json!({"folderIds": [f2_id, f1_id]})),
    )
    .await;
    let folder_order: Vec<&str> = cat["catalog"]["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        folder_order,
        vec![f2_id.as_str(), f1_id.as_str()],
        "文件夹重排"
    );

    ok(
        &server,
        "PATCH",
        &format!("/lingxi/v1/session-projects/projects/{p1_id}"),
        Some(&json!({"folderId": f2_id})),
    )
    .await;
    let (_, cat) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/projects/reorder",
        Some(&json!({"folderId": f2_id, "projectIds": [p1_id]})),
    )
    .await;
    let in_f2: Vec<&str> = cat["catalog"]["projects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["folderId"] == json!(f2_id))
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(in_f2, vec![p1_id.as_str()], "作用域内项目重排");

    // 项目删除：目录消失 + 活动会话归属清空（按现役 cwd: 报告）。
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions",
        Some(&json!({"sessionId": "sp1", "agentId": "ag", "title": "T"})),
    )
    .await;
    assert_eq!(status, 201);
    ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/sp1/project",
        Some(&json!({"projectId": p2_id})),
    )
    .await;
    let (status, body) = ok(
        &server,
        "DELETE",
        &format!("/lingxi/v1/session-projects/projects/{p2_id}"),
        None,
    )
    .await;
    assert_eq!(status, 200, "项目删除：{body}");
    assert_eq!(body["assignment"]["projectId"], json!("cwd:"));
    assert_eq!(
        body["assignment"]["sessionIds"],
        json!(["sp1"]),
        "受影响会话回报"
    );
    let (_, body) = ok(&server, "GET", "/lingxi/v1/sessions/sp1/project", None).await;
    assert_eq!(body["projectId"], Value::Null, "会话归属清空");

    // 文件夹删除：其下项目移到根，项目与会话不删除。
    let (status, body) = ok(
        &server,
        "DELETE",
        &format!("/lingxi/v1/session-projects/folders/{f2_id}"),
        None,
    )
    .await;
    assert_eq!(status, 200, "文件夹删除：{body}");
    let projects: Vec<&str> = body["catalog"]["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert!(projects.contains(&p1_id.as_str()), "项目保留并移到根");
    let (_, cat) = ok(&server, "GET", "/lingxi/v1/session-projects", None).await;
    let p1_after = cat["catalog"]["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == json!(p1_id))
        .unwrap();
    assert_eq!(p1_after["folderId"], Value::Null, "归属改根");

    // auto id：删除为无操作；更新按 upsert 创建。
    let (status, _) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/session-projects/projects/cwd:%2Ftmp",
        None,
    )
    .await;
    assert_eq!(status, 200, "auto id 删除无操作");
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/session-projects/projects/cwd:%2Ftmp",
        Some(&json!({"name": "自动项目"})),
    )
    .await;
    assert_eq!(status, 200, "auto id upsert：{body}");
    assert_eq!(body["project"]["name"], json!("自动项目"));

    let (status, _) = http(&server, "GET", "/lingxi/v1/session-projects", None, None).await;
    assert_eq!(status, 401, "无凭证 401");

    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}

// R08-T01：旧 /api/session-permission-mode 端口——GET/POST 默认域、会话级
// 写入、显式拒绝分支的真实服务证明（engine 侧 session-permission-mode.ts 语义）。
#[tokio::test]
async fn session_permission_mode_defaults_session_write_and_rejections() {
    let server = boot().await;
    let mode_url = "/lingxi/v1/session-permission-mode";

    // 初始状态：无焦点会话、无偏好默认 → auto/operate/auto。
    let (status, body) = ok(&server, "GET", mode_url, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mode"], json!("auto"));
    assert_eq!(body["accessMode"], json!("operate"));
    assert_eq!(body["defaultMode"], json!("auto"));

    // 默认域写偏好默认（无 sessionPath/pending 标志）。
    let (status, body) = ok(&server, "POST", mode_url, Some(&json!({"mode": "ask"}))).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["defaultMode"], json!("ask"));
    let (status, body) = ok(&server, "GET", mode_url, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mode"], json!("ask"), "无会话时 mode 跟随默认");
    assert_eq!(body["defaultMode"], json!("ask"));

    // pendingNewSession 写默认（下一会话消费），不触碰会话级。
    let (status, body) = ok(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "operate", "pendingNewSession": true})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["defaultMode"], json!("operate"));

    // 非法 mode 显式 400。
    let (status, err) = ok(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "nonsense"})),
    )
    .await;
    assert_eq!(status, 400, "{err}");

    // currentSessionOnly 无活动会话显式 409，不静默写默认。
    let (status, err) = ok(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "operate", "currentSessionOnly": true})),
    )
    .await;
    assert_eq!(status, 409, "{err}");
    assert_eq!(err["ok"], json!(false));

    // 未知会话 sessionPath 显式 409，不静默创建。
    let (status, err) = ok(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "read_only", "sessionPath": "native:missing-session"})),
    )
    .await;
    assert_eq!(status, 409, "{err}");
    assert_eq!(err["ok"], json!(false));

    // 会话级 auto 显式 409（auto 只属于默认域）。
    let (status, err) = ok(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "auto", "sessionPath": "native:missing-session"})),
    )
    .await;
    assert_eq!(status, 409, "{err}");

    // 旧 JSONL 路径不猜映射：显式 409，不静默落到默认域。
    let (status, err) = ok(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "ask", "sessionPath": "/legacy/s1.jsonl"})),
    )
    .await;
    assert_eq!(status, 409, "{err}");
    let (_, state) = ok(&server, "GET", mode_url, None).await;
    assert_eq!(state["defaultMode"], json!("operate"), "拒绝分支不得写默认");

    // 真实会话（显式 sessionId，无需模型面，沿归属测试口径）。
    let session_id = "r08-perm-session";
    let (status, created) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions",
        Some(&json!({"sessionId": session_id, "agentId": "a", "title": "R08 权限接线"})),
    )
    .await;
    assert_eq!(status, 201, "session create: {created}");

    // 会话级写入：native locator 定位，默认域不变。
    let (status, body) = ok(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "read_only", "sessionPath": format!("native:{session_id}")})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["mode"], json!("read_only"));
    assert_eq!(body["accessMode"], json!("read_only"));
    assert_eq!(body["defaultMode"], json!("operate"), "会话级写不改默认");

    // 聚焦该会话后 GET 反映会话级显式值。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/switch",
        Some(&json!({"sessionId": session_id})),
    )
    .await;
    assert_eq!(status, 200, "switch: {body}");
    let (status, body) = ok(&server, "GET", mode_url, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mode"], json!("read_only"), "焦点会话显式值");
    assert_eq!(body["accessMode"], json!("read_only"));
    assert_eq!(body["defaultMode"], json!("operate"));

    // currentSessionOnly 命中焦点会话。
    let (status, body) = ok(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "ask", "currentSessionOnly": true})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mode"], json!("ask"));
    let (status, body) = ok(&server, "GET", mode_url, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mode"], json!("ask"));

    // 默认域写不改焦点会话显式值（engine 同语义）。
    let (status, body) = ok(&server, "POST", mode_url, Some(&json!({"mode": "auto"}))).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["defaultMode"], json!("auto"));
    let (status, body) = ok(&server, "GET", mode_url, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mode"], json!("ask"), "会话显式值优先于默认");
    assert_eq!(body["defaultMode"], json!("auto"));

    // 无凭证 401（LocalOnly 路由政策）。
    let (status, _) = http(
        &server,
        "POST",
        mode_url,
        Some(&json!({"mode": "ask"})),
        None,
    )
    .await;
    assert_eq!(status, 401);
    let (status, _) = http(&server, "GET", mode_url, None, None).await;
    assert_eq!(status, 401);

    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}

// R08-T01：旧 /api/session-projects/session-assignment 端口（唯一
// MAPPED_ROUTE_MISSING 项）——分配/清除/读回/拒绝的真实服务证明。
#[tokio::test]
async fn session_assignment_assigns_clears_and_rejects_unknown() {
    let server = boot().await;
    // 准备一个真实会话（显式 sessionId，无需模型面）与项目。
    let session_id = "r08-assign-session".to_string();
    let (status, created) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions",
        Some(&json!({"sessionId": session_id, "agentId": "a", "title": "R08 归属验收"})),
    )
    .await;
    assert_eq!(status, 201, "session create: {created}");
    let session_path = format!("native:{session_id}");
    let (status, project_res) = ok(
        &server,
        "POST",
        "/lingxi/v1/session-projects/projects",
        Some(&json!({"name":"R08 归属项目"})),
    )
    .await;
    assert_eq!(status, 200, "{project_res}");
    let project_id = project_res["project"]["id"]
        .as_str()
        .expect("project id")
        .to_string();

    let assignment = "/lingxi/v1/session-projects/session-assignment";
    // 分配：旧契约 body {sessionPath, projectId}，native locator 定位。
    let (status, body) = ok(
        &server,
        "POST",
        assignment,
        Some(&json!({"sessionPath": session_path, "projectId": project_id})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["assignment"]["projectId"], json!(project_id));
    // studio-list 真实读回归属。
    let (status, list) = ok(&server, "GET", "/lingxi/v1/sessions/studio-list", None).await;
    assert_eq!(status, 200, "{list}");
    let row = list["sessions"]
        .as_array()
        .expect("sessions 数组")
        .iter()
        .find(|row| row["sessionId"] == json!(session_id))
        .expect("会话在列表中");
    assert_eq!(row["projectId"], json!(project_id));
    // 清除归属（projectId=null）。
    let (status, body) = ok(
        &server,
        "POST",
        assignment,
        Some(&json!({"sessionPath": session_path, "projectId": null})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["assignment"]["projectId"], json!(null));
    let (status, list) = ok(&server, "GET", "/lingxi/v1/sessions/studio-list", None).await;
    assert_eq!(status, 200);
    let row = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sessionId"] == json!(session_id))
        .unwrap();
    assert_eq!(row["projectId"], json!(null), "清除后读回为 null");
    // 未知项目显式拒绝（engine 同语义 project not found）。
    let (status, err) = ok(
        &server,
        "POST",
        assignment,
        Some(&json!({"sessionPath": session_path, "projectId": "no-such-project"})),
    )
    .await;
    assert_eq!(status, 400, "{err}");
    assert_eq!(err["error"], json!("project not found"));
    // 未知会话显式拒绝，不静默创建。
    let (status, err) = ok(
        &server,
        "POST",
        assignment,
        Some(&json!({"sessionPath": "native:missing-session", "projectId": project_id})),
    )
    .await;
    assert_eq!(status, 400, "{err}");
    assert_eq!(err["error"], json!("session not found"));
    // 缺 sessionPath 显式拒绝。
    let (status, err) = ok(&server, "POST", assignment, Some(&json!({}))).await;
    assert_eq!(status, 400, "{err}");
    assert_eq!(err["error"], json!("sessionPath is required"));
    // 无凭证 401（沿本套件既有口径）。
    let (status, _) = http(&server, "POST", assignment, None, None).await;
    assert_eq!(status, 401);

    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}
