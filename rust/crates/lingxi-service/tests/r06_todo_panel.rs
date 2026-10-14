//! R06 G 块「计划/待办面板」（现役 sessions.ts:1852-2044 三路由的端口）：
//! 真实 HTTP + 真实 SQLite 的正反证明。覆盖 complete/cancel 成功与广播、
//! 幂等不重复写、版本冲突 409（todo_version_mismatch）、dismiss 收纳后
//! 历史保留与再 dismiss 行为、跨会话不串、未知会话 404、无凭证 401、
//! 流式中 409（complete/cancel；现役 dismiss 无流式闸，一并断言）。
//!
//! todos 模块已在 lib.rs 注册为 crate 模块；本测试经库导出引用处理器并
//! 自行挂载（authenticate → Extension<Principal>，无凭证 401），处理器与
//! 生产注册面同一实体。
use lingxi_service::todos;

use std::time::Duration;

use axum::response::IntoResponse as _;
use lingxi_service::{
    prepare_layout, EndpointError, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState, LOCAL_OWNER_USER_ID,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    addr: String,
    token: String,
    state: ServiceState,
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-todo-panel-{uniq}"));
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
    let token = state.auth().local_token().to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new()
        .route(
            "/lingxi/v1/sessions/todos/complete",
            axum::routing::post(todos::complete_route),
        )
        .route(
            "/lingxi/v1/sessions/todos/cancel",
            axum::routing::post(todos::cancel_route),
        )
        .route(
            "/lingxi/v1/sessions/todos/dismiss",
            axum::routing::post(todos::dismiss_route),
        )
        .route(
            "/lingxi/v1/sessions/todos",
            axum::routing::get(todos::panel_route),
        )
        .route(
            "/lingxi/v1/app-events",
            axum::routing::get(lingxi_service::appevents::events_route),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_guard,
        ))
        .with_state(state.clone());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    Server {
        addr: addr.to_string(),
        token,
        state,
        handle,
    }
}

/// 测试内鉴权扩展层：authenticate → Extension<Principal>；无/坏凭证 401。
/// 与生产 auth_guard 同一 authenticate 入口（本地 token → local owner）。
async fn auth_guard(
    axum::extract::State(state): axum::extract::State<ServiceState>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let authorization = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    match state.auth().authenticate(
        authorization.as_deref(),
        None,
        false,
        lingxi_service::ConnectionKind::Local,
    ) {
        Ok(principal) => {
            req.extensions_mut().insert(principal);
            next.run(req).await
        }
        Err(denial) => EndpointError::unauthorized(denial.reason).into_response(),
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

async fn create_session(server: &Server, session_id: &str) {
    lingxi_adapters::storage::session_admin::create_session(
        server.state.storage(),
        session_id,
        "agent-todo",
        LOCAL_OWNER_USER_ID,
        "todo 面板证明",
        None,
        &[],
        true,
        now_ms(),
    )
    .await
    .expect("seed session");
}

fn item(id: &str, content: &str, status: &str) -> Value {
    json!({"id": id, "content": content, "status": status})
}

/// 以"模型写入"的初始清单（action=replace、removed=false）种入快照条目。
async fn seed_todos(server: &Server, session_id: &str, items: &[Value]) -> String {
    let entry = todos::TodoStateEntry {
        version: "seed".to_string(), // 读路径重算版本，不信任存储值
        todos: items
            .iter()
            .map(|value| serde_json::from_value(value.clone()).expect("item"))
            .collect(),
        removed: false,
        action: "replace".to_string(),
    };
    todos::append_state_entry(&server.state, session_id, &entry, now_ms())
        .await
        .expect("seed entry")
}

async fn stored_entries(server: &Server, session_id: &str) -> Vec<todos::TodoStateEntry> {
    let rows = lingxi_adapters::storage::canonical_message_store::read_branch_entries_by_type(
        server.state.storage(),
        session_id,
        todos::TODO_STATE_ENTRY_TYPE,
    )
    .await
    .expect("read entries");
    rows.iter()
        .rev() // 存储返回分支序新→旧；这里转旧→新便于断言追加顺序
        .map(|row| serde_json::from_str(&row.content_json).expect("entry"))
        .collect()
}

async fn app_events(server: &Server) -> Vec<Value> {
    let (_, body) = ok(server, "GET", "/lingxi/v1/app-events?since=0", None).await;
    body["events"].as_array().cloned().unwrap_or_default()
}

fn todos_changed_events<'a>(events: &'a [Value], session_id: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| {
            event["type"] == todos::TODOS_CHANGED_EVENT
                && event["payload"]["sessionId"] == session_id
        })
        .collect()
}

fn version_of(items: &[Value]) -> String {
    let parsed: Vec<todos::TodoItem> = items
        .iter()
        .map(|value| serde_json::from_value(value.clone()).expect("item"))
        .collect();
    todos::compute_todo_list_version(&parsed)
}

/// complete/cancel 成功与广播、幂等不重复写、dismiss 收纳与历史保留。
#[tokio::test]
async fn complete_cancel_dismiss_semantics_and_broadcast() {
    let server = boot().await;
    create_session(&server, "s-todo").await;
    let seed = [
        item("t1", "梳理现役语义", "pending"),
        item("t2", "写适配层查询", "in_progress"),
        item("t3", "已取消项保持", "cancelled"),
    ];
    seed_todos(&server, "s-todo", &seed).await;

    // 空面板对照：无条目的会话。
    create_session(&server, "s-empty").await;
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/todos?sessionId=s-empty",
        None,
    )
    .await;
    assert_eq!(status, 200, "空面板读取：{body}");
    assert_eq!(
        body["panel"],
        json!({"removed": true, "dismissed": false, "todos": []}),
        "无快照 → removed 面板且无 version 键"
    );

    // 面板读：返回种子清单与重算版本。
    let expected_version = version_of(&seed);
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/todos?sessionId=s-todo",
        None,
    )
    .await;
    assert_eq!(status, 200, "面板读取：{body}");
    assert_eq!(body["panel"]["todos"], json!(seed));
    assert_eq!(body["panel"]["version"], json!(expected_version));
    assert_eq!(body["panel"]["finished"], json!(false));
    assert_eq!(body["panel"]["allCompleted"], json!(false));

    // complete：携带当前版本成功；未完成→completed、取消项保持。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"sessionId": "s-todo", "version": expected_version})),
    )
    .await;
    assert_eq!(status, 200, "complete：{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(
        body["panel"]["todos"],
        json!([
            item("t1", "梳理现役语义", "completed"),
            item("t2", "写适配层查询", "completed"),
            item("t3", "已取消项保持", "cancelled"),
        ]),
        "complete：未完成→completed，已取消保持"
    );
    assert_eq!(body["panel"]["finished"], json!(true));
    assert_eq!(
        body["panel"]["allCompleted"],
        json!(false),
        "含取消项，非全部完成"
    );
    let completed_version = body["panel"]["version"].as_str().expect("版本").to_string();
    assert_eq!(
        completed_version,
        version_of(&[
            item("t1", "梳理现役语义", "completed"),
            item("t2", "写适配层查询", "completed"),
            item("t3", "已取消项保持", "cancelled"),
        ]),
        "响应版本 = 对新清单重算的 FNV 版本"
    );

    // 广播：todos-changed 携带 sessionId/agentId/panel。
    let events = app_events(&server).await;
    let changed = todos_changed_events(&events, "s-todo");
    assert_eq!(
        changed.len(),
        1,
        "complete 后恰一条 todos-changed：{events:?}"
    );
    assert_eq!(changed[0]["payload"]["agentId"], json!("agent-todo"));
    assert_eq!(changed[0]["payload"]["panel"]["finished"], json!(true));

    // 幂等：已完结再 complete 返回当前面板、不重复写（仍广播，现役同款）。
    let (status, again) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"sessionId": "s-todo", "version": completed_version})),
    )
    .await;
    assert_eq!(status, 200, "幂等 complete：{again}");
    assert_eq!(again["panel"], body["panel"], "幂等：面板与上次一致");
    let entries = stored_entries(&server, "s-todo").await;
    assert_eq!(entries.len(), 2, "不重复写：仍只有种子+complete 两条");
    assert_eq!(entries[1].action, "complete_all");
    assert!(!entries[1].removed);

    // dismiss：收纳（已结束且未收纳）→ removed 面板、历史保留。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/dismiss",
        Some(&json!({"sessionId": "s-todo"})),
    )
    .await;
    assert_eq!(status, 200, "dismiss：{body}");
    assert_eq!(
        body["panel"],
        json!({"removed": true, "dismissed": true, "todos": []}),
        "dismiss 面板恒为 removed"
    );
    let entries = stored_entries(&server, "s-todo").await;
    assert_eq!(entries.len(), 3, "dismiss 追加收纳记录");
    assert_eq!(entries[2].action, "dismiss");
    assert!(entries[2].removed, "收纳条目 removed=true");
    assert_eq!(entries[2].todos.len(), 3, "收纳不改写清单内容");
    assert!(!entries[0].removed, "历史记录保留（种子条目仍在）");

    // 再 dismiss：幂等 no-op，不重复写。
    let (status, again) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/dismiss",
        Some(&json!({"sessionId": "s-todo"})),
    )
    .await;
    assert_eq!(status, 200, "再 dismiss：{again}");
    assert_eq!(again["panel"], body["panel"]);
    assert_eq!(
        stored_entries(&server, "s-todo").await.len(),
        3,
        "再 dismiss 不重复写"
    );

    // 收纳后 complete：不复活清单（removed 分支返回当前面板不写）。
    let (status, after) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"sessionId": "s-todo"})),
    )
    .await;
    assert_eq!(status, 200, "收纳后 complete：{after}");
    assert_eq!(after["panel"]["removed"], json!(true));
    assert_eq!(
        stored_entries(&server, "s-todo").await.len(),
        3,
        "收纳后 complete 不写"
    );

    server.handle.abort();
    let _ = server.handle.await;
}

/// cancel 语义 + 版本冲突 409 + 跨会话不串。
#[tokio::test]
async fn cancel_semantics_version_mismatch_and_isolation() {
    let server = boot().await;
    create_session(&server, "s-cancel").await;
    let seed = [
        item("c1", "待开始", "pending"),
        item("c2", "进行中", "in_progress"),
        item("c3", "受阻", "blocked"),
        item("c4", "已完成保持", "completed"),
    ];
    seed_todos(&server, "s-cancel", &seed).await;
    // 对照会话（跨会话不串）。
    create_session(&server, "s-other").await;
    let other_seed = [item("o1", "对照清单", "pending")];
    seed_todos(&server, "s-other", &other_seed).await;

    // 版本冲突：客户端版本 ≠ 当前快照 → 409 todo_version_mismatch，不写。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/cancel",
        Some(&json!({"sessionId": "s-cancel", "version": "tvdeadbeef"})),
    )
    .await;
    assert_eq!(status, 409, "版本冲突：{body}");
    assert_eq!(body["code"], json!("todo_version_mismatch"));
    assert_eq!(
        stored_entries(&server, "s-cancel").await.len(),
        1,
        "版本冲突不写存储"
    );

    // cancel：非终态→cancelled，已完成保持；不带版本沿用旧行为成功。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/cancel",
        Some(&json!({"sessionId": "s-cancel"})),
    )
    .await;
    assert_eq!(status, 200, "cancel：{body}");
    assert_eq!(
        body["panel"]["todos"],
        json!([
            item("c1", "待开始", "cancelled"),
            item("c2", "进行中", "cancelled"),
            item("c3", "受阻", "cancelled"),
            item("c4", "已完成保持", "completed"),
        ]),
        "cancel：非终态→cancelled，已完成保持"
    );
    assert_eq!(body["panel"]["finished"], json!(true));
    assert_eq!(
        body["panel"]["allCompleted"],
        json!(false),
        "含保持完成的条目与取消条目：finished 但非 allCompleted"
    );
    let entries = stored_entries(&server, "s-cancel").await;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].action, "cancel_remaining");
    assert!(!entries[1].removed);

    // 广播面：cancel 有一条 todos-changed。
    let events = app_events(&server).await;
    assert_eq!(todos_changed_events(&events, "s-cancel").len(), 1);

    // 跨会话不串：s-other 面板与存储不受影响。
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/todos?sessionId=s-other",
        None,
    )
    .await;
    assert_eq!(status, 200, "对照面板：{body}");
    assert_eq!(body["panel"]["todos"], json!(other_seed), "对照清单原样");
    assert_eq!(body["panel"]["version"], json!(version_of(&other_seed)));
    assert_eq!(
        stored_entries(&server, "s-other").await.len(),
        1,
        "对照会话存储不变"
    );

    server.handle.abort();
    let _ = server.handle.await;
}

/// 闸序反例：未知会话 404、无凭证 401、缺 sessionId 400、归档 409。
#[tokio::test]
async fn gate_rejections() {
    let server = boot().await;
    create_session(&server, "s-gate").await;

    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"sessionId": "ghost"})),
    )
    .await;
    assert_eq!(status, 404, "未知会话 404");
    let (status, _) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/todos?sessionId=ghost",
        None,
    )
    .await;
    assert_eq!(status, 404, "未知会话面板读 404");

    let (status, _) = http(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"sessionId": "s-gate"})),
        None,
    )
    .await;
    assert_eq!(status, 401, "无凭证 401");

    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"version": "tv00000000"})),
    )
    .await;
    assert_eq!(status, 400, "缺 sessionId 400");

    // 归档会话：complete 与面板读都拒绝（活动闸）。
    lingxi_adapters::storage::session_admin::archive_session(
        server.state.storage(),
        "s-gate",
        None,
        &[],
        now_ms(),
    )
    .await
    .expect("archive");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"sessionId": "s-gate"})),
    )
    .await;
    assert_eq!(status, 409, "归档会话 complete 409");
    let (status, _) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/todos?sessionId=s-gate",
        None,
    )
    .await;
    assert_eq!(status, 409, "归档会话面板读 409");

    server.handle.abort();
    let _ = server.handle.await;
}

/// 流式中 409：complete/cancel 拒绝；现役 dismiss 无流式闸，照常成功。
/// 并发闸桩 = 会话 supervisor 的 run 租约（真实流式占位，同一准入面）。
#[tokio::test]
async fn streaming_gate_complete_cancel_reject_dismiss_passes() {
    let server = boot().await;
    create_session(&server, "s-stream").await;
    // 已结束的清单（dismiss 的可收纳前提）；流式闸在读快照之前，
    // complete/cancel 对任何清单状态都在流式中被拒。
    let seed = [item("x1", "流式中已完成的清单", "completed")];
    seed_todos(&server, "s-stream", &seed).await;

    // 持有 run 租约（流式占位；与 execute_for 内部同一 try_begin_run）。
    let lease = server
        .state
        .sessions()
        .session_supervisor()
        .try_begin_run("s-stream")
        .expect("占位流式");

    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"sessionId": "s-stream"})),
    )
    .await;
    assert_eq!(status, 409, "流式中 complete 409：{body}");
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/cancel",
        Some(&json!({"sessionId": "s-stream"})),
    )
    .await;
    assert_eq!(status, 409, "流式中 cancel 409");
    assert_eq!(
        stored_entries(&server, "s-stream").await.len(),
        1,
        "流式拒绝不写存储"
    );

    // 现役 dismiss 路由无流式闸：收纳照常成功并真实落记录（端口如实保留）。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/dismiss",
        Some(&json!({"sessionId": "s-stream"})),
    )
    .await;
    assert_eq!(status, 200, "流式中 dismiss 成功（现役无此闸）：{body}");
    assert_eq!(
        body["panel"],
        json!({"removed": true, "dismissed": true, "todos": []})
    );
    let entries = stored_entries(&server, "s-stream").await;
    assert_eq!(entries.len(), 2, "dismiss 在流式中真实追加收纳记录");
    assert!(entries[1].removed);
    assert_eq!(entries[1].action, "dismiss");

    drop(lease);
    // 流式结束后：已收纳态 complete 返回当前面板不写。
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/todos/complete",
        Some(&json!({"sessionId": "s-stream"})),
    )
    .await;
    assert_eq!(status, 200, "流式结束后 complete：{body}");
    assert_eq!(body["panel"]["removed"], json!(true));
    assert_eq!(
        stored_entries(&server, "s-stream").await.len(),
        2,
        "complete 幂等未再写"
    );

    server.handle.abort();
    let _ = server.handle.await;
}

/// 版本算法端口证明：与现役 computeTodoListVersion 的 FNV-1a/UTF-16 口径
/// 对齐（同输入同输出；非 ASCII 码元路径也参与哈希）。
#[tokio::test]
async fn version_hash_matches_incumbent_shape() {
    let items = vec![
        todos::TodoItem {
            id: "a".into(),
            content: "中文内容".into(),
            status: "in_progress".into(),
        },
        todos::TodoItem {
            id: "b".into(),
            content: "plain".into(),
            status: "completed".into(),
        },
    ];
    // 期望值由现役算法独立复算（JSON 串 [["中文内容","","in_progress",""],
    // ["plain","","completed",""]] 的 UTF-16 码元 FNV-1a）。
    let expected = {
        let text = r#"[["中文内容","","in_progress",""],["plain","","completed",""]]"#;
        let mut hash: u32 = 0x811c_9dc5;
        for unit in text.encode_utf16() {
            hash ^= u32::from(unit);
            hash = hash.wrapping_add(
                (hash << 1)
                    .wrapping_add(hash << 4)
                    .wrapping_add(hash << 7)
                    .wrapping_add(hash << 8)
                    .wrapping_add(hash << 24),
            );
        }
        format!("tv{hash:08x}")
    };
    assert_eq!(todos::compute_todo_list_version(&items), expected);
}
