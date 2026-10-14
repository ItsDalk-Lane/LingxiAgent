//! R06 收口（纯服务端六叶）集成证明：
//! 1. `R00-T02-LA-F28086B64522` conversation-map 布局 GET：返回持久化
//!    布局或默认空布局；未知会话/无凭证拒绝且不落盘。
//! 2. `R00-T02-LA-5370DA3742CB` positions merge：仅指定卡片变化。
//! 3. `R00-T02-LA-5271217DB01B` positions replace：未指定坐标被删除。
//! 4. `R00-T02-LA-94B9BE3AA7A6` collapsed 整体替换（持久化折叠列表）。
//! 5. `R00-T02-LA-D51B2A1FA68A` hana_card_guide 工具：返回指南文本、
//!    无卡片产物；指南资源缺失显式失败（单元侧 cardguide.rs）。
//! 6. `R00-T02-LA-3562B5D44C5E` 检查点 restore：恢复文件到写回位置并
//!    返回位置；检查点不存在 404。
//!
//! 布局/存档面全部经真实 HTTP + 真实落盘（conversation-map.json /
//! data_home/checkpoints/*.json，TS 落盘契约）；工具面经真实统一网关
//! （prepare → execute_prepared，生产注册函数 cardguide::register_…）。
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-cmap-{uniq}"));
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

async fn http_raw(
    server: &Server,
    method: &str,
    path: &str,
    payload: String,
    token: Option<&str>,
) -> (u16, Value) {
    let task = async {
        let mut socket = tokio::net::TcpStream::connect(&server.addr).await.unwrap();
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
    http_raw(
        server,
        method,
        path,
        body.map(|value| value.to_string()).unwrap_or_default(),
        Some(&server.token),
    )
    .await
}

async fn no_auth(server: &Server, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    http_raw(
        server,
        method,
        path,
        body.map(|value| value.to_string()).unwrap_or_default(),
        None,
    )
    .await
}

async fn create_session(server: &Server, id: &str, agent: &str) {
    let (status, body) = ok(
        server,
        "POST",
        "/lingxi/v1/sessions",
        Some(&json!({"sessionId": id, "agentId": agent, "title": "R06 cmap"})),
    )
    .await;
    assert_eq!(status, 201, "会话 {id} 创建：{body}");
}

fn layout_file(server: &Server) -> PathBuf {
    server.home.join("conversation-map.json")
}

async fn stop(server: Server) {
    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}

/// 叶 1-4：会话地图布局面（GET 默认/持久化、merge、replace、collapsed
/// 整体替换；TS tests/conversation-map-route.test.ts 的语义等价断言集）。
#[tokio::test]
async fn conversation_map_layout_get_merge_replace_collapsed() {
    let server = boot().await;
    create_session(&server, "s-map", "agentA").await;
    let base = "/lingxi/v1/sessions/s-map/conversation-map/layout";

    // ── 叶4 F28086B64522：GET 返回默认空布局（文件缺失）──
    let (status, body) = ok(&server, "GET", base, None).await;
    assert_eq!(status, 200, "GET 默认布局：{body}");
    assert_eq!(
        body,
        json!({"version": 1, "positions": {}, "collapsed": [], "updatedAt": null}),
        "默认空布局形状"
    );

    // 反路径：未知会话 404；无凭证 401（分类表 chat scope + 归属闸）。
    let (status, _) = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ghost/conversation-map/layout",
        None,
    )
    .await;
    assert_eq!(status, 404, "未知会话 404");
    let (status, _) = no_auth(&server, "GET", base, None).await;
    assert_eq!(status, 401, "无凭证 401");
    let (status, _) = no_auth(
        &server,
        "PUT",
        base,
        Some(&json!({"positions": {"x": {"x": 1, "y": 1}}})),
    )
    .await;
    assert_eq!(status, 401, "无凭证 PUT 401");
    assert!(!layout_file(&server).exists(), "拒绝路径不落盘");

    // ── 叶2 5370DA3742CB merge：种 a/b，仅指定卡片变化 ──
    let (status, put) = ok(
        &server,
        "PUT",
        base,
        Some(&json!({
            "positions": {"session-a": {"x": 10, "y": 20}, "session-b": {"x": 30, "y": 40}},
            "collapsed": ["session-b"],
        })),
    )
    .await;
    assert_eq!(status, 200, "种布局：{put}");
    assert_eq!(put["ok"], json!(true));
    assert!(put["updatedAt"].is_string(), "updatedAt 字符串");
    let (status, body) = ok(&server, "GET", base, None).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["positions"],
        json!({"session-a": {"x": 10, "y": 20}, "session-b": {"x": 30, "y": 40}})
    );
    assert_eq!(body["collapsed"], json!(["session-b"]));
    assert_eq!(body["updatedAt"], put["updatedAt"], "读回时间戳一致");

    // 合并：默认只动指定 key（session-a 原值保留）。
    let (status, _) = ok(
        &server,
        "PUT",
        base,
        Some(&json!({"positions": {"session-b": {"x": 50, "y": 60}}})),
    )
    .await;
    assert_eq!(status, 200, "合并写");
    let (_, body) = ok(&server, "GET", base, None).await;
    assert_eq!(
        body["positions"],
        json!({"session-a": {"x": 10, "y": 20}, "session-b": {"x": 50, "y": 60}}),
        "merge：仅指定卡片变化"
    );
    assert_eq!(
        body["collapsed"],
        json!(["session-b"]),
        "未传 collapsed 不动"
    );

    // 取整 + 夹取 + 丢非数值（conversation-map.ts sanitizePositionsInput）。
    // tie 是 JS Math.round 的半数语义（floor(x+0.5)：-2.5 → -2，
    // Rust f64::round 会给 -3——端口必须与 JS 一致）。
    let (status, _) = ok(
        &server,
        "PUT",
        base,
        Some(&json!({"positions": {
            "too-big": {"x": 999999999, "y": -999999999},
            "tie": {"x": -2.5, "y": 2.5},
            "not-numeric": {"x": "1", "y": 2},
            "not-object": 42,
            "ok-frac": {"x": 0.5, "y": -7.6},
        }})),
    )
    .await;
    assert_eq!(status, 200, "清洗写");
    let (_, body) = ok(&server, "GET", base, None).await;
    assert_eq!(
        body["positions"]["too-big"],
        json!({"x": 100000, "y": -100000}),
        "越限坐标夹取"
    );
    assert_eq!(
        body["positions"]["ok-frac"],
        json!({"x": 1, "y": -8}),
        "坐标取整（JS Math.round 语义）"
    );
    assert_eq!(
        body["positions"]["tie"],
        json!({"x": -2, "y": 3}),
        "半数取整与 JS 一致（-2.5 → -2，不是 -3）"
    );
    assert!(
        body["positions"].get("not-numeric").is_none()
            && body["positions"].get("not-object").is_none(),
        "非数值/非对象项丢弃"
    );
    assert_eq!(
        body["positions"]["session-a"],
        json!({"x": 10, "y": 20}),
        "清洗合并不动既有 key"
    );

    // ── 叶1 5271217DB01B replace：replacePositions=true 整体替换 ──
    let (status, _) = ok(
        &server,
        "PUT",
        base,
        Some(&json!({
            "positions": {"only-keep": {"x": 7, "y": 8}},
            "replacePositions": true,
        })),
    )
    .await;
    assert_eq!(status, 200, "replace 写");
    let (_, body) = ok(&server, "GET", base, None).await;
    assert_eq!(
        body["positions"],
        json!({"only-keep": {"x": 7, "y": 8}}),
        "未指定坐标被删除（整体替换）"
    );
    assert_eq!(
        body["collapsed"],
        json!(["session-b"]),
        "未传 collapsed 保留"
    );

    // 空表 + replacePositions：清空全部坐标（布局重置）。
    let (status, _) = ok(
        &server,
        "PUT",
        base,
        Some(&json!({"positions": {}, "replacePositions": true})),
    )
    .await;
    assert_eq!(status, 200);
    let (_, body) = ok(&server, "GET", base, None).await;
    assert_eq!(body["positions"], json!({}), "replace 清空");

    // ── 叶3 94B9BE3AA7A6 collapsed 整体替换 ──
    let (status, _) = ok(
        &server,
        "PUT",
        base,
        Some(&json!({"positions": {"p1": {"x": 1, "y": 1}}})),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = ok(
        &server,
        "PUT",
        base,
        Some(&json!({"collapsed": ["c2", "c3"]})),
    )
    .await;
    assert_eq!(status, 200, "collapsed 写");
    let (_, body) = ok(&server, "GET", base, None).await;
    assert_eq!(body["collapsed"], json!(["c2", "c3"]), "折叠列表整体更新");
    assert_eq!(
        body["positions"],
        json!({"p1": {"x": 1, "y": 1}}),
        "仅 collapsed 时 positions 不动"
    );
    // 空数组也是整体替换（清空折叠列表）。
    let (status, _) = ok(&server, "PUT", base, Some(&json!({"collapsed": []}))).await;
    assert_eq!(status, 200);
    let (_, body) = ok(&server, "GET", base, None).await;
    assert_eq!(body["collapsed"], json!([]), "空数组整体清空");
    assert_eq!(body["version"], json!(1));

    // 落盘契约：conversation-map.json 形状（version/positions/collapsed/updatedAt）。
    let raw = std::fs::read_to_string(layout_file(&server)).expect("布局文件已落盘");
    let disk: Value = serde_json::from_str(&raw).expect("布局文件合法 JSON");
    assert_eq!(disk["version"], json!(1));
    assert!(disk["positions"].is_object());
    assert!(disk["collapsed"].is_array());
    assert!(disk["updatedAt"].is_string());

    // ── 反路径：无效 body 不落盘（错误对用户可见）──
    let (_, before) = ok(&server, "GET", base, None).await;
    let (status, body) = http_raw(
        &server,
        "PUT",
        base,
        "not-json".to_string(),
        Some(&server.token),
    )
    .await;
    assert_eq!(status, 400, "非 JSON body 400：{body}");
    let (status, body) = ok(&server, "PUT", base, Some(&json!({"unrelated": true}))).await;
    assert_eq!(status, 400, "缺 positions/collapsed 400：{body}");
    let (status, body) = ok(&server, "PUT", base, Some(&json!([1, 2, 3]))).await;
    assert_eq!(status, 400, "数组 body 400：{body}");
    let (status, body) = ok(&server, "PUT", base, Some(&json!(42))).await;
    assert_eq!(status, 400, "标量 body 400：{body}");
    let (_, after) = ok(&server, "GET", base, None).await;
    assert_eq!(before, after, "拒绝路径布局不变（无错误持久化）");

    // 未知会话 PUT：404 且不落盘。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/sessions/ghost/conversation-map/layout",
        Some(&json!({"positions": {"z": {"x": 1, "y": 1}}})),
    )
    .await;
    assert_eq!(status, 404, "未知会话 PUT 404");
    let (_, after) = ok(&server, "GET", base, None).await;
    assert_eq!(before, after, "越权写未触碰布局");

    // 损坏布局文件读回：防御性归一化 → 默认布局（现役 readLayoutFile 契约）。
    drop(before);
    drop(after);
    std::fs::write(layout_file(&server), "{corrupted").expect("写坏布局");
    let (status, body) = ok(&server, "GET", base, None).await;
    assert_eq!(status, 200, "损坏文件不炸路由");
    assert_eq!(body["positions"], json!({}), "回退默认形状");

    stop(server).await;
}

/// 叶 6 `R00-T02-LA-3562B5D44C5E`：独立改前存档 restore（写回位置返回、
/// 404、显式失败；lib/checkpoint-store.ts restore 语义）。
#[tokio::test]
async fn checkpoint_restore_route() {
    let server = boot().await;
    let ckpt_dir = server.home.join("checkpoints");
    std::fs::create_dir_all(&ckpt_dir).expect("mkdir checkpoints");

    // 存档一（文本）：TS 落盘契约 {ts, sessionPath, tool, source, reason,
    // path, content, size}；目标目录尚不存在 → restore 须 mkdir -p。
    let target = server.home.join("restore-target").join("note.md");
    std::fs::write(
        ckpt_dir.join("1710000000_abcd.json"),
        serde_json::to_string(&json!({
            "ts": 1710000000000u64,
            "sessionPath": null,
            "tool": "write",
            "source": "llm",
            "reason": "tool-write",
            "path": target.to_str().unwrap(),
            "content": "line1\nline2",
            "size": 12,
        }))
        .unwrap(),
    )
    .expect("种文本存档");

    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/checkpoints/1710000000_abcd/restore",
        None,
    )
    .await;
    assert_eq!(status, 200, "restore：{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(
        body["restoredTo"],
        json!(target.to_str().unwrap()),
        "返回写回位置"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "line1\nline2",
        "存档字节写回"
    );
    assert!(
        ckpt_dir.join("1710000000_abcd.json").exists(),
        "restore 不删档（独立快照，可重复恢复）"
    );

    // 重复恢复：目标被外部改动后，restore 恢复存档字节（覆盖写回）。
    std::fs::write(&target, "CHANGED").unwrap();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/checkpoints/1710000000_abcd/restore",
        None,
    )
    .await;
    assert_eq!(status, 200, "重复 restore：{body}");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "line1\nline2",
        "覆盖写回存档内容"
    );

    // 存档二（二进制 base64）：无损写回（非 utf-8 字节保真）。
    let binary: Vec<u8> = vec![0x00, 0x01, 0xFF, 0xFE, 0x80, 0x7F];
    let encoded = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(&binary)
    };
    let target_b = server.home.join("restore-target").join("blob.bin");
    std::fs::write(
        ckpt_dir.join("1710000001_ff.json"),
        serde_json::to_string(&json!({
            "ts": 1710000001000u64,
            "sessionPath": null,
            "tool": "edit",
            "source": "llm",
            "reason": "tool-edit",
            "path": target_b.to_str().unwrap(),
            "content_b64": encoded,
            "size": binary.len(),
        }))
        .unwrap(),
    )
    .expect("种二进制存档");
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/checkpoints/1710000001_ff/restore",
        None,
    )
    .await;
    assert_eq!(status, 200, "二进制 restore：{body}");
    assert_eq!(body["restoredTo"], json!(target_b.to_str().unwrap()));
    assert_eq!(
        std::fs::read(&target_b).unwrap(),
        binary,
        "base64 存档字节保真"
    );

    // ── 反路径 ──
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/checkpoints/ghost/restore",
        None,
    )
    .await;
    assert_eq!(status, 404, "检查点不存在 404：{body}");
    assert_eq!(body["error"], json!("checkpoint not found"), "现役错误词");

    // 损坏存档（非法 JSON）→ 显式 500，不呈现成功结果。
    std::fs::write(ckpt_dir.join("bad-json.json"), "{corrupted").unwrap();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/checkpoints/bad-json/restore",
        None,
    )
    .await;
    assert_eq!(status, 500, "损坏存档显式失败：{body}");
    assert!(body["error"].is_string(), "错误对用户可见");

    // 存档缺内容载荷 → 显式 500。
    std::fs::write(
        ckpt_dir.join("bad-shape.json"),
        serde_json::to_string(&json!({"ts": 1, "path": "/tmp/nowhere-r06.md"})).unwrap(),
    )
    .unwrap();
    let (status, body) = ok(
        &server,
        "POST",
        "/lingxi/v1/checkpoints/bad-shape/restore",
        None,
    )
    .await;
    assert_eq!(status, 500, "缺内容显式失败：{body}");
    assert!(
        !std::path::Path::new("/tmp/nowhere-r06.md").exists(),
        "失败路径不写半截文件"
    );

    // 无凭证 → 401（LocalOnly 面）。
    let (status, _) = no_auth(
        &server,
        "POST",
        "/lingxi/v1/checkpoints/1710000000_abcd/restore",
        None,
    )
    .await;
    assert_eq!(status, 401, "无凭证 401");

    stop(server).await;
}

/// 叶 5 `R00-T02-LA-D51B2A1FA68A`：hana_card_guide 经真实统一网关返回
/// 指南全册文本；本调用不创建卡片产物（单文本块、零 resource_refs）；
/// schema 越界参数被网关拒绝。指南资源缺失显式失败在 cardguide.rs 单元。
#[tokio::test]
async fn hana_card_guide_tool_through_gateway() {
    use std::sync::Arc;

    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetRef};
    use lingxi_kernel::RunContext;
    use lingxi_protocol::ToolCallId;
    use lingxi_service::approval_service::ApprovalService;
    use lingxi_service::cardguide;
    use lingxi_service::inject::SystemClock;
    use lingxi_service::toolgateway::{
        CallerSurface, InvocationPermissionContext, InvocationRequest, ToolInvocationGateway,
        ToolPolicyPort, DEFAULT_LIVE_PREPARED_CAP, DEFAULT_PREPARED_TTL_MS,
    };

    // 生产注册函数 + 真实网关（与 lib.rs 工具面同一构造点形状；
    // 静态只读指南不依赖 workspace / 模型平面）。
    let registry = Arc::new(ToolRegistry::new());
    let policy: Arc<dyn ToolPolicyPort> = Arc::new(ApprovalService::new(Arc::new(SystemClock)));
    let budget = SchemaBudget::default();
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        policy,
        Arc::new(SystemClock),
        budget,
        DEFAULT_PREPARED_TTL_MS,
        DEFAULT_LIVE_PREPARED_CAP,
    ));
    cardguide::register_card_guide_tool(&registry, gateway.as_ref(), &budget);

    let ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("s-card-guide".to_string()),
        run_id: lingxi_protocol::RunId::new("r-card-guide".to_string()),
        attempt: lingxi_protocol::AttemptId::new("r-card-guide#a1".to_string()),
        generation: 1,
    };

    let prepare = |args: Value, call: ToolCallId| {
        gateway.prepare(InvocationRequest::from_trusted_entry(
            &ctx,
            CallerSurface::UserRun,
            "agent-default",
            InvocationPermissionContext::UserSession {
                mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
            },
            ToolTargetRef::ByName {
                name: "hana_card_guide".to_string(),
            },
            None,
            args,
            call,
        ))
    };

    // 指南文本可见：省略 modules / 合法 modules 都返回全册（v1 语义）。
    for args in [
        json!({}),
        json!({"modules": ["diagram", "chart", "interactive", "data_record"]}),
    ] {
        let call = ToolCallId::new(format!("call-guide-{}", args.to_string().len()));
        let prepared = prepare(args.clone(), call.clone())
            .unwrap_or_else(|refusal| panic!("prepare refused for {args}: {refusal:?}"));
        let result = gateway
            .execute_prepared(&ctx, &call, &prepared.handle)
            .await
            .unwrap_or_else(|refusal| panic!("execute refused for {args}: {refusal:?}"));
        match result.outcome {
            lingxi_kernel::ports::ToolOutcome::Success { result: success } => {
                assert_eq!(success.content.len(), 1, "单内容块（无卡片产物）");
                let text = match &success.content[0] {
                    lingxi_protocol::ContentBlock::Text { text } => text.clone(),
                    other => panic!("非文本内容块：{other:?}"),
                };
                assert_eq!(text, cardguide::HANDBOOK, "全册指南文本（v1 恒返回全文）");
                for anchor in [
                    "--accent",
                    "EB Garamond",
                    "#537D96",
                    "#1B365D",
                    "position: fixed",
                    "sr-only",
                ] {
                    assert!(text.contains(anchor), "指南缺核心锚点 {anchor}");
                }
                assert!(
                    success.resource_refs.is_empty(),
                    "没有卡片产物（零资源引用）"
                );
                assert!(!success.truncated, "指南未被截断");
            }
            other => panic!("期望成功结果，得到 {other:?}"),
        }
    }

    // 反路径：越界 modules / 未知字段被网关 schema 拒绝（不进执行器）。
    for bad in [
        json!({"modules": ["bogus"]}),
        json!({"modules": "diagram"}),
        json!({"surprise": 1}),
    ] {
        let call = ToolCallId::new("call-guide-bad");
        assert!(
            prepare(bad.clone(), call).is_err(),
            "非法参数 {bad} 应在 prepare 拒绝"
        );
    }
}
