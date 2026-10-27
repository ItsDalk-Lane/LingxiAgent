//! R06-R2（38EEA99077E8）：现役 `GET /sessions` 的 Studio 列表投影
//! （server/routes/sessions.ts:773-853 的端口映射）——经 `sessions.read`
//! 与 Studio 身份限制返回会话摘要、修订点、归属和运行徽章。
//!
//! 对照纪律（RC-3）：
//! - sessions.read scope 闸 + same-Studio projection v0（:776-790）：
//!   主体 Studio 与 server Studio 不一致 → 403 `studio_scope_mismatch`；
//!   本地主人（无 Studio 声明）不触发。
//! - 归属（agentId/agentName/projectId/parentSessionId）+ 摘要
//!   （title/hasSummary，`lib/memory/session-summary.ts` 落盘契约）+
//!   修订点（磁盘 stat 签名的端口映射：与 /history ETag 同源字符串）+
//!   运行徽章（现役 loopStatus running/paused 注入的端口映射：run 库
//!   活动行）。
//! - 拥有者隔离：设备主体仅本人会话（sessions.rs can_access 同口径）。
//!
//! 夹具：真 TCP loopback × 2 共享同一 ServiceState——生产路由（建会话/
//! execute）走 `run()`；投影路由（pub 处理器，注册片段留主控）挂独立
//! listener，鉴权经与生产同源的 `state.auth().authenticate`。
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
use lingxi_service::auth::{CredentialKind, Principal, PrincipalKind, TrustState};
use lingxi_service::{
    prepare_layout, run, ConnectionKind, HomeSource, NetworkMode, ServeOutcome, ServiceConfig,
    ServiceDeps, ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// ── 闸门 provider：execute 挂起直到释放（运行徽章的真实活动行来源） ──

struct GatedProvider {
    release: Arc<tokio::sync::watch::Sender<bool>>,
}

impl TurnProviderPort for GatedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let mut gate = self.release.subscribe();
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            gate.wait_for(|open| *open)
                .await
                .expect("gate channel open");
            ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "释放后的答复".to_string(),
                        }],
                        model_call_id: Some(ModelCallId::new(call_id)),
                    },
                },
            )
        })
    }
}

// ── 夹具 ──

struct Fixture {
    /// 生产路由 listener（建会话 / execute）。
    main_addr: SocketAddr,
    /// 投影路由 listener（本套件挂载的 pub 处理器）。
    projection_addr: SocketAddr,
    token: String,
    home: PathBuf,
    state: ServiceState,
    release: Arc<tokio::sync::watch::Sender<bool>>,
    stops: Vec<tokio::sync::oneshot::Sender<()>>,
    handles: Vec<tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>>,
}

async fn auth_guard(
    axum::extract::State(state): axum::extract::State<ServiceState>,
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // 测试专用身份注入：x-test-principal 构造生产链之外的主体形态
    // （Studio 不匹配 / 无 scope 的设备主体），其余走真实 authenticate。
    let injected = req
        .headers()
        .get("x-test-principal")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let principal = match injected.as_deref() {
        Some("foreign-studio") => Some(test_device_principal(
            Some("studio_other"),
            vec!["sessions.read".to_string()],
        )),
        Some("no-scope") => Some(test_device_principal(None, vec![])),
        Some("device-owner") => Some(test_device_principal(
            None,
            vec!["sessions.read".to_string()],
        )),
        _ => None,
    };
    let principal = match principal {
        Some(principal) => principal,
        None => {
            let authorization = req
                .headers()
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            match state.auth().authenticate(
                authorization.as_deref(),
                None,
                false,
                ConnectionKind::Local,
            ) {
                Ok(principal) => principal,
                Err(_) => {
                    return (
                        axum::http::StatusCode::UNAUTHORIZED,
                        axum::Json(serde_json::json!({ "error": "invalid_credential" })),
                    )
                        .into_response()
                }
            }
        }
    };
    req.extensions_mut().insert(principal);
    next.run(req).await
}

fn test_device_principal(studio: Option<&str>, scopes: Vec<String>) -> Principal {
    Principal {
        schema_version: lingxi_service::auth::AUTH_SCHEMA_VERSION,
        principal_id: "principal_test_device".to_string(),
        kind: PrincipalKind::Device,
        user_id: Some("device-owner-user".to_string()),
        studio_id: studio.map(str::to_string),
        server_node_id: None,
        device_id: Some("test-device".to_string()),
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: CredentialKind::DeviceCredential,
        trust_state: TrustState::Lan,
        scopes,
    }
}

async fn boot(tag: &str) -> Fixture {
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
    let home = std::env::temp_dir().join(format!("lingxi-r06r2-slp-{uniq}"));
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
    let (release, gate_rx) = tokio::sync::watch::channel(false);
    let release = Arc::new(release);
    let provider = Arc::new(GatedProvider {
        release: Arc::clone(&release),
    });
    let state = ServiceState::bootstrap_with_deps(
        config,
        &layout,
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");
    let token = state.auth().local_token().to_string();
    release.send(false).ok();
    drop(gate_rx);

    // listener 1：生产路由。
    let main_state = state.clone();
    let (main_stop, main_stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (main_ready, main_ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let main_handle = tokio::spawn(async move {
        run(
            main_state,
            async {
                let _ = main_stop_rx.await;
            },
            |addr| {
                let _ = main_ready.send(addr);
            },
            None,
        )
        .await
    });
    let main_addr = main_ready_rx.await.expect("main readiness");

    // listener 2：投影路由（pub 处理器 + 与生产同源鉴权）。
    let projection_router = axum::Router::new()
        .route(
            "/lingxi/v1/sessions/studio-list",
            axum::routing::get(lingxi_service::sessions::sessions_studio_list_route),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_guard,
        ))
        .with_state(state.clone());
    let projection_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind projection");
    let projection_addr = projection_listener.local_addr().expect("projection addr");
    let (projection_stop, projection_stop_rx) = tokio::sync::oneshot::channel::<()>();
    let projection_handle = tokio::spawn(async move {
        axum::serve(projection_listener, projection_router)
            .with_graceful_shutdown(async {
                let _ = projection_stop_rx.await;
            })
            .await
            .expect("projection fixture HTTP 服务");
        Ok::<ServeOutcome, ServiceError>(ServeOutcome {
            drain_timed_out: false,
        })
    });

    Fixture {
        main_addr,
        projection_addr,
        token,
        home,
        state,
        release,
        stops: vec![main_stop, projection_stop],
        handles: vec![main_handle, projection_handle],
    }
}

/// 通用 HTTP 往返（文本体）。
async fn http_raw(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    headers: &[(&str, &str)],
) -> (u16, String, Vec<(String, String)>) {
    let task = async {
        let mut socket = TcpStream::connect(addr).await.expect("connect");
        let payload = body.unwrap_or_default().to_string();
        let mut header_lines = String::new();
        for (key, value) in headers {
            header_lines.push_str(&format!("{key}: {value}\r\n"));
        }
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{header_lines}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len(),
        );
        socket.write_all(request.as_bytes()).await.expect("write");
        let mut response = Vec::new();
        socket.read_to_end(&mut response).await.expect("read");
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .expect("HTTP 头存在");
        let header_text = String::from_utf8_lossy(&response[..boundary]).to_string();
        let status: u16 = header_text
            .split_whitespace()
            .nth(1)
            .expect("状态码")
            .parse()
            .expect("合法状态码");
        let response_headers: Vec<(String, String)> = header_text
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .collect();
        (
            status,
            String::from_utf8_lossy(&response[boundary + 4..]).to_string(),
            response_headers,
        )
    };
    tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .expect("HTTP 往返在限时内")
}

async fn projection_get(
    fx: &Fixture,
    path: &str,
    extra_headers: &[(&str, &str)],
) -> (u16, serde_json::Value) {
    let bearer = format!("Bearer {}", fx.token);
    let mut headers: Vec<(&str, &str)> = vec![("Authorization", bearer.as_str())];
    headers.extend(extra_headers.iter().copied());
    let (status, body, _) = http_raw(fx.projection_addr, "GET", path, None, &headers).await;
    let value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&body).expect("JSON 体")
    };
    (status, value)
}

async fn main_post(fx: &Fixture, path: &str, body: &str) -> (u16, serde_json::Value) {
    let bearer = format!("Bearer {}", fx.token);
    let (status, text, _) = http_raw(
        fx.main_addr,
        "POST",
        path,
        Some(body),
        &[("Authorization", bearer.as_str())],
    )
    .await;
    let value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    (status, value)
}

async fn wait_run_terminal(fx: &Fixture, run_id: &str) {
    for _ in 0..400 {
        let row = fx
            .state
            .storage()
            .query_one_text(
                "SELECT status FROM runs WHERE run_id = ?1",
                vec![run_id.to_string()],
            )
            .await
            .ok()
            .flatten();
        if let Some(status) = row {
            if !matches!(
                status.as_str(),
                "queued" | "running" | "waiting_approval" | "cancelling"
            ) {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("run {run_id} 未在限时内到终态");
}

/// 主体断言：Studio 身份闸 + 摘要/修订点/归属/运行徽章的完整列表投影。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn studio_list_projection_carries_summary_revision_attribution_and_badges() {
    let fx = boot("projection").await;
    // 建两个会话（不同归属助手）。
    for (sid, agent) in [("s-alpha", "agent-a"), ("s-beta", "agent-b")] {
        let (status, body) = main_post(
            &fx,
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": sid,
                "agentId": agent,
                "title": format!("{sid} 标题"),
            })
            .to_string(),
        )
        .await;
        assert_eq!(status, 201, "create {sid}: {body}");
    }
    // agent-b 的滚动摘要记录（现役 session-summary 落盘契约）。
    let summary_dir = fx.home.join("agents/agent-b/memory/summaries");
    std::fs::create_dir_all(&summary_dir).expect("mkdir summaries");
    std::fs::write(
        summary_dir.join("s-beta.json"),
        serde_json::json!({"summary": "beta 的滚动摘要", "updatedAt": "2026-10-13T00:00:00Z"})
            .to_string(),
    )
    .expect("写摘要记录");

    // 本地主人：完整投影（摘要/修订点/归属/运行徽章）。
    let (status, body) = projection_get(&fx, "/lingxi/v1/sessions/studio-list", &[]).await;
    assert_eq!(status, 200, "studio list: {body}");
    let sessions = body["sessions"].as_array().expect("sessions 数组").clone();
    // 两个新建会话 + 两个 R02-T03 合成种子会话（同属本地主人，同列表可见）。
    assert_eq!(sessions.len(), 4, "新建×2 + 合成种子×2: {sessions:?}");
    assert!(
        sessions
            .iter()
            .any(|session| session["sessionId"] == "sess_local_alpha"),
        "合成种子在列表（本地主人全见）"
    );
    let by_id: std::collections::HashMap<String, serde_json::Value> = sessions
        .iter()
        .map(|session| {
            (
                session["sessionId"].as_str().unwrap().to_string(),
                session.clone(),
            )
        })
        .collect();
    let alpha = &by_id["s-alpha"];
    assert_eq!(alpha["title"], "s-alpha 标题", "摘要标题");
    assert_eq!(alpha["agentId"], "agent-a", "归属 agentId");
    assert_eq!(
        alpha["agentName"], "agent-a",
        "归属 agentName（无名单时如实回 id）"
    );
    assert_eq!(alpha["hasSummary"], false, "无摘要记录");
    assert!(
        alpha["revision"].as_str().unwrap().starts_with("r"),
        "修订点存在: {}",
        alpha["revision"]
    );
    assert_eq!(alpha["running"], false, "空闲徽章");
    assert_eq!(alpha["activeRuns"].as_array().unwrap().len(), 0);
    let beta = &by_id["s-beta"];
    assert_eq!(beta["agentName"], "agent-b");
    assert_eq!(beta["hasSummary"], true, "摘要记录在场");
    assert_eq!(beta["running"], false);

    // 运行徽章：execute 挂起（provider 闸门）→ activeRuns 非空 → running。
    let exec_task = {
        let addr = fx.main_addr;
        let token = fx.token.clone();
        tokio::spawn(async move {
            let payload = serde_json::json!({ "input": "挂起中的任务" }).to_string();
            let request = format!(
                "POST /lingxi/v1/sessions/s-alpha/execute HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len(),
            );
            let mut socket = TcpStream::connect(addr).await.unwrap();
            use tokio::io::AsyncWriteExt;
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            use tokio::io::AsyncReadExt;
            socket.read_to_end(&mut response).await.unwrap();
            response
        })
    };
    let mut running_seen = false;
    for _ in 0..400 {
        let (_, body) = projection_get(&fx, "/lingxi/v1/sessions/studio-list", &[]).await;
        let alpha = &body["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .find(|session| session["sessionId"] == "s-alpha")
            .expect("s-alpha");
        if alpha["running"] == serde_json::json!(true)
            && !alpha["activeRuns"].as_array().unwrap().is_empty()
        {
            running_seen = true;
            // 活动列表纪律不因运行态改变：四个可见会话仍在主列表。
            assert_eq!(body["sessions"].as_array().unwrap().len(), 4);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(running_seen, "运行徽章必须出现（活动 run 行）");
    // 释放闸门：run 到终态 → 徽章回落。
    fx.release.send(true).expect("release gate");
    let _ = exec_task.await.expect("execute 完成");
    let (_, body) = projection_get(&fx, "/lingxi/v1/sessions/studio-list", &[]).await;
    let alpha = body["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["sessionId"] == "s-alpha")
        .expect("s-alpha");
    assert_eq!(alpha["running"], false, "终态后徽章回落");
    // 修订点推进：execute 落消息后 beta 的 revision 变化。
    let (_, body) = projection_get(&fx, "/lingxi/v1/sessions/studio-list", &[]).await;
    let beta_before = body["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["sessionId"] == "s-beta")
        .expect("s-beta")["revision"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, body) = main_post(
        &fx,
        "/lingxi/v1/sessions/s-beta/execute",
        &serde_json::json!({ "input": "推进修订点" }).to_string(),
    )
    .await;
    assert_eq!(status, 200, "execute beta: {body}");
    let run_id = body["runId"].as_str().unwrap().to_string();
    wait_run_terminal(&fx, &run_id).await;
    let (_, body) = projection_get(&fx, "/lingxi/v1/sessions/studio-list", &[]).await;
    let beta = body["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["sessionId"] == "s-beta")
        .expect("s-beta");
    assert_ne!(
        beta["revision"].as_str().unwrap(),
        beta_before,
        "修订点随写入推进（同会话写入前后不同）"
    );

    // Studio 身份闸：主体 Studio 与 server 不一致 → 403 studio_scope_mismatch。
    let (status, body) = projection_get(
        &fx,
        "/lingxi/v1/sessions/studio-list",
        &[("x-test-principal", "foreign-studio")],
    )
    .await;
    assert_eq!(status, 403, "Studio 不一致: {body}");
    assert_eq!(body["error"], "studio_scope_mismatch");

    // scope 闸：无 sessions.read 的设备主体 → 403 insufficient_scope。
    let (status, body) = projection_get(
        &fx,
        "/lingxi/v1/sessions/studio-list",
        &[("x-test-principal", "no-scope")],
    )
    .await;
    assert_eq!(status, 403, "scope 不足: {body}");
    assert_eq!(body["error"], "insufficient_scope");

    // 拥有者隔离：设备主体（本人无会话）→ 空列表（can_access 同口径）。
    let (status, body) = projection_get(
        &fx,
        "/lingxi/v1/sessions/studio-list",
        &[("x-test-principal", "device-owner")],
    )
    .await;
    assert_eq!(status, 200, "设备主体可见自己的空集: {body}");
    assert_eq!(
        body["sessions"].as_array().unwrap().len(),
        0,
        "不泄露本地主人会话"
    );

    for stop in fx.stops {
        let _ = stop.send(());
    }
    for handle in fx.handles {
        let _ = handle.await;
    }
    let _ = std::fs::remove_dir_all(&fx.home);
}
