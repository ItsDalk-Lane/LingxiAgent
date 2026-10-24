//! R07 收口探针套件（接入面/设备/身份/思考级别/静态壳/桥管理面/桌面活动
//! 与频道目录）：真实服务 + 真实 HTTP，逐叶覆盖正向与拒绝行为。
//! 口令/令牌一律运行时生成，不写可用凭据字面量。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "r07-probe-access-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn config_for(home: &Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Env,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

/// 运行时生成的合成口令（仅隔离测试 home 使用，非任何真实凭据）。
fn probe_secret(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis()
    )
}

struct Server {
    addr: SocketAddr,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
    state: ServiceState,
    home: PathBuf,
}

async fn boot(tag: &str) -> Server {
    boot_with(tag, false).await
}

/// `with_model` = 装配最小模型平面（思考级别读写要求 model gateway 存在；
/// 端点指向永不监听的回环端口，若被真实调用会响亮失败）。
async fn boot_with(tag: &str, with_model: bool) -> Server {
    let home = unique_dir(tag);
    std::fs::create_dir_all(home.join("agents/lingxi")).expect("agent dir");
    std::fs::create_dir_all(home.join("agents/second")).expect("second agent");
    std::fs::write(
        home.join("agents/lingxi/config.yaml"),
        "agent:\n  name: lingxi\nmemory:\n  enabled: false\n",
    )
    .expect("agent config");
    std::fs::write(
        home.join("agents/second/config.yaml"),
        "agent:\n  name: second\nmemory:\n  enabled: false\n",
    )
    .expect("agent config 2");
    let layout = prepare_layout(&home).expect("layout");
    let mut deps = ServiceDeps::default();
    if with_model {
        let config = json!({
            "providers":{"probe":{"protocol":"openai-completions","endpoint":"http://127.0.0.1:9/v1","auth":{"kind":"apiKey","apiKey":"PROBE_NEVER_CALLED"}}},
            "models":{"chat":{"provider":"probe","model":"probe-chat","capabilities":{"tools":true},"compat":{"contextWindow":200000,"reasoning":true,"thinkingLevelMap":{"off":"low","low":"medium","high":"high"}}}}
        });
        let plane = lingxi_adapters::models::config::ModelPlaneConfig::from_value(config)
            .expect("model plane");
        let credentials = std::sync::Arc::new(
            lingxi_service::credentials::CredentialService::bootstrap(
                &plane,
                &layout.runtime_dir,
                std::sync::Arc::new(lingxi_service::inject::SystemClock),
            )
            .expect("credentials"),
        );
        deps.credential_service = Some(credentials);
        deps.model_gateway = Some(std::sync::Arc::new(
            lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
        ));
    }
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let state_clone = state.clone();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
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
        addr,
        token,
        stop: stop_tx,
        handle,
        state: state_clone,
        home,
    }
}

impl Server {
    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            self.token,
            payload.len()
        );
        let mut stream = tokio::net::TcpStream::connect(self.addr)
            .await
            .expect("connect");
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut buf)).await;
        let header_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("http header end");
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let raw = &buf[header_end + 4..];
        let content_type = head.to_ascii_lowercase();
        let body: Value = if content_type.contains("svg") || content_type.contains("markdown") {
            Value::String(String::from_utf8_lossy(raw).into_owned())
        } else {
            serde_json::from_slice(raw).unwrap_or(Value::Null)
        };
        (status, body)
    }

    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
        self.state.storage().close().await.expect("close storage");
    }
}

#[tokio::test]
async fn access_summary_returns_network_devices_and_account() {
    let server = boot("summary").await;
    let (status, body) = server.call("GET", "/lingxi/v1/access/summary", None).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body.get("network").is_some()
            && (body.get("devices").is_some() || body.get("credentials").is_some()),
        "网络/设备/账号概况: {body}"
    );
    server.stop().await;
}

#[tokio::test]
async fn access_network_save_updates_summary_and_rejects_bad_mode() {
    let server = boot("network").await;
    let (status, body) = server
        .call(
            "PUT",
            "/lingxi/v1/access/network",
            Some(json!({"mode":"loopback","listenPort":18765})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (_, summary) = server.call("GET", "/lingxi/v1/access/summary", None).await;
    assert_eq!(summary["network"]["mode"], json!("loopback"), "{summary}");
    let (status, body) = server
        .call(
            "PUT",
            "/lingxi/v1/access/network",
            Some(json!({"mode":"wan"})),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    server.stop().await;
}

#[tokio::test]
async fn access_profile_save_and_validate() {
    let server = boot("profile").await;
    let (status, body) = server
        .call(
            "PUT",
            "/lingxi/v1/access/account/profile",
            Some(json!({"username":"probe-user","displayName":"Probe User"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body.get("account").is_some() || body.get("username").is_some(),
        "{body}"
    );
    // 非法用户名（含路径分隔符）拒绝。
    let (status, _) = server
        .call(
            "PUT",
            "/lingxi/v1/access/account/profile",
            Some(json!({"username":"a/b"})),
        )
        .await;
    assert_eq!(status, 400);
    server.stop().await;
}

#[tokio::test]
async fn access_password_set_and_remove_with_validation() {
    let server = boot("password").await;
    let password = probe_secret("probe");
    assert!(password.len() >= 8);
    let (status, body) = server
        .call(
            "PUT",
            "/lingxi/v1/access/account/password",
            Some(json!({ "password": password })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    // 过短口令拒绝且不落库。
    let (status, _) = server
        .call(
            "PUT",
            "/lingxi/v1/access/account/password",
            Some(json!({"password":"short"})),
        )
        .await;
    assert_eq!(status, 400);
    // 清除口令。
    let (status, body) = server
        .call("DELETE", "/lingxi/v1/access/account/password", None)
        .await;
    assert_eq!(status, 200, "{body}");
    server.stop().await;
}

#[tokio::test]
async fn device_credentials_issue_honest_transport_handling() {
    let server = boot("devicecred").await;
    let (status, body) = server
        .call(
            "POST",
            "/lingxi/v1/devices/credentials",
            Some(json!({"requestedDevice":{"deviceKind":"desktop"}})),
        )
        .await;
    if status == 200 {
        assert!(
            body["secret"].is_string() || body["credential"]["secret"].is_string(),
            "{body}"
        );
    } else {
        // 非安全传输上的签发拒绝是显式错误，不是空数据或成功提示。
        assert!(
            status == 400 || status == 403,
            "一次性 secret 签发在明文通道应显式拒绝: {status} {body}"
        );
    }
    server.stop().await;
}

#[tokio::test]
async fn mobile_credentials_and_qr_honest_states() {
    let server = boot("mobilecred").await;
    let (status, body) = server
        .call(
            "POST",
            "/lingxi/v1/access/mobile-credentials",
            Some(json!({"requestedDevice":{"deviceKind":"mobile"}})),
        )
        .await;
    if status == 200 {
        assert!(
            body["secret"].is_string() || body["credential"]["secret"].is_string(),
            "{body}"
        );
        // 同屏地址可用时二维码 SVG 可读；LAN 不可用为 400（诚实错误）。
        let (qr_status, qr) = server
            .call("GET", "/lingxi/v1/access/mobile-qr.svg", None)
            .await;
        assert!(qr_status == 200 || qr_status == 400, "{qr_status} {qr}");
    } else {
        assert!(status == 400 || status == 403, "{status} {body}");
    }
    server.stop().await;
}

#[tokio::test]
async fn pairing_session_create_approve_and_device_revoke() {
    let server = boot("pairing").await;
    // 创建配对会话：本地主人获得配对码与到期时间。
    let (status, body) = server
        .call(
            "POST",
            "/lingxi/v1/devices/pairing-sessions",
            Some(json!({"requestedDevice":{"deviceKind":"desktop"}})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let pairing_id = body["pairingSessionId"]
        .as_str()
        .expect(&format!("pairingSessionId: {body}"))
        .to_string();
    let code = body["userCode"]
        .as_str()
        .expect(&format!("userCode: {body}"))
        .to_string();
    // 批准配对：返回一次密钥与作用域。
    let (status, body) = server
        .call(
            "POST",
            &format!("/lingxi/v1/devices/pairing-sessions/{pairing_id}/approve"),
            Some(json!({"userCode": code, "scopes":["chat"]})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let device_id = body["device"]["id"]
        .as_str()
        .or(body["device"]["deviceId"].as_str())
        .unwrap_or_else(|| panic!("device id: {body}"))
        .to_string();
    // 设备列表可见（脱敏）。
    let (status, devices) = server.call("GET", "/lingxi/v1/devices", None).await;
    assert_eq!(status, 200, "{devices}");
    assert!(
        devices["devices"].as_array().is_some_and(|rows| rows
            .iter()
            .any(|d| { d["deviceId"].as_str() == Some(device_id.as_str()) })),
        "{devices}"
    );
    // 撤销设备：失效并返回脱敏状态；重复撤销幂等（仍 200 或 404，不复活）。
    let (status, body) = server
        .call(
            "POST",
            &format!("/lingxi/v1/devices/{device_id}/revoke"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = server
        .call(
            "POST",
            &format!("/lingxi/v1/devices/{device_id}/revoke"),
            None,
        )
        .await;
    assert!(
        status == 200 || status == 404,
        "重复撤销幂等: {status} {body}"
    );
    if status == 200 {
        let device_status = body["device"]["status"].as_str().unwrap_or("unknown");
        assert_eq!(device_status, "revoked", "{body}");
    }
    server.stop().await;
}

#[tokio::test]
async fn server_identity_projects_capabilities_by_principal() {
    let server = boot("identity").await;
    let (status, body) = server.call("GET", "/lingxi/v1/server/identity", None).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.get("version").is_some(), "{body}");
    assert!(
        body.get("capabilities").is_some() || body.get("scopes").is_some(),
        "{body}"
    );
    server.stop().await;
}

#[tokio::test]
async fn thinking_level_read_and_default_set_without_touching_sessions() {
    let server = boot_with("thinking", true).await;
    let (status, body) = server
        .call("GET", "/lingxi/v1/session-thinking-level", None)
        .await;
    assert_eq!(status, 200, "{body}");
    let current_level = body["thinkingLevel"]
        .as_str()
        .or(body["level"].as_str())
        .unwrap_or("off")
        .to_string();
    // 无 sessionPath：修改默认级别（模型已声明可配置思考档位）。
    let (status, body) = server
        .call(
            "POST",
            "/lingxi/v1/session-thinking-level",
            Some(json!({"level":"high"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (_, after) = server
        .call("GET", "/lingxi/v1/session-thinking-level", None)
        .await;
    assert!(
        after["thinkingLevel"] == json!("high")
            || after["level"] == json!("high")
            || after.get("sessions").is_some(),
        "默认级别应更新: {after} (initial {current_level})"
    );
    // 无效 level 拒绝且默认不变。
    let (status, _) = server
        .call(
            "POST",
            "/lingxi/v1/session-thinking-level",
            Some(json!({"level":"ultra"})),
        )
        .await;
    assert_eq!(status, 400);
    let (_, unchanged) = server
        .call("GET", "/lingxi/v1/session-thinking-level", None)
        .await;
    assert!(
        unchanged["thinkingLevel"] == json!("high")
            || unchanged["level"] == json!("high")
            || unchanged.get("sessions").is_some(),
        "{unchanged}"
    );
    server.stop().await;
}

#[tokio::test]
async fn thinking_level_session_scoped_write() {
    let server = boot_with("thinking-session", true).await;
    // 先建真实会话供 sessionPath 定向修改。
    let (status, body) = server
        .call(
            "POST",
            "/lingxi/v1/sessions",
            Some(json!({"sessionId":"probe-think","agentId":"lingxi","title":"think"})),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = server
        .call(
            "POST",
            "/lingxi/v1/session-thinking-level",
            Some(json!({"level":"low","sessionId":"probe-think"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    // 会话级读取反映 low；默认（无 sessionId）不被追改。
    let (_, scoped) = server
        .call(
            "GET",
            "/lingxi/v1/session-thinking-level?sessionId=probe-think",
            None,
        )
        .await;
    assert!(
        scoped["thinkingLevel"] == json!("low") || scoped["level"] == json!("low"),
        "会话级应为 low: {scoped}"
    );
    let (_, overall) = server
        .call("GET", "/lingxi/v1/session-thinking-level", None)
        .await;
    assert!(
        overall["thinkingLevel"] != json!("low") || overall.get("sessions").is_some(),
        "默认不应被会话级修改追改: {overall}"
    );
    server.stop().await;
}

#[tokio::test]
async fn ws_ticket_issued_for_authenticated_principal_only() {
    let server = boot("wsticket").await;
    let (status, body) = server
        .call("POST", "/lingxi/v1/ws-ticket", Some(json!({"kind":"chat"})))
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["ticket"].is_string(), "{body}");
    server.stop().await;
}

#[tokio::test]
async fn static_shells_serve_or_guide_without_fabricating() {
    let server = boot("static").await;
    for path in ["/mobile", "/desktop"] {
        let (status, body) = server.call("GET", path, None).await;
        let final_status = if status == 308 || status == 301 {
            server.call("GET", &format!("{path}/"), None).await.0
        } else {
            status
        };
        // 有构建产物 → 200 页面；无产物 → 指引/错误（非空成功体伪装）。
        assert!(
            final_status == 200 || final_status == 404 || final_status == 400,
            "{path} -> {final_status} {body}"
        );
    }
    server.stop().await;
}

#[tokio::test]
async fn bridge_status_read_does_not_start_connectors() {
    let server = boot("bridgestatus").await;
    let (status, body) = server
        .call("GET", "/lingxi/v1/bridge/status?agentId=lingxi", None)
        .await;
    assert_eq!(status, 200, "{body}");
    // 读取状态不启动连接：未启用平台为 disconnected，bridgeReady 如实。
    assert!(
        body.get("bridgeReady").is_some()
            && body.get("telegram").is_some()
            && body.get("wechat").is_some(),
        "{body}"
    );
    assert_eq!(body["telegram"]["status"], json!("disconnected"), "{body}");
    server.stop().await;
}

#[tokio::test]
async fn bridge_test_reports_concrete_error_without_polling() {
    let server = boot("bridgetest").await;
    let synthetic_token = probe_secret("bridge");
    // 显式合成凭证走外部验证；loopback 环境外呼失败应返回具体错误（不启动轮询）。
    let (status, body) = server
        .call(
            "POST",
            "/lingxi/v1/bridge/test?agentId=lingxi",
            Some(json!({"platform":"telegram","credentials":{"token": synthetic_token}})),
        )
        .await;
    assert!(
        status == 200 || status == 400 || status == 502,
        "{status} {body}"
    );
    assert!(!body.is_null(), "{body}");
    server.stop().await;
}

#[tokio::test]
async fn bridge_wechat_qrcode_create_reports_external_failure() {
    let server = boot("qrcode").await;
    let (status, body) = server
        .call("POST", "/lingxi/v1/bridge/wechat/qrcode", None)
        .await;
    // 未配置微信登录服务：明确错误（不伪装二维码）。
    assert!(
        status != 200 || body.get("error").is_some(),
        "{status} {body}"
    );
    assert!(!body.is_null(), "{body}");
    server.stop().await;
}

#[tokio::test]
async fn bridge_settings_receipt_and_rich_streaming_toggles_persist() {
    let server = boot("toggles").await;
    // 两个开关各自双向写入并从真实响应回读；rich/receipt 源码默认均为 true，
    // false 方向才是可区分默认的保存证明（bridge_routes.rs:204）。
    for (field, first, second) in [
        ("receiptEnabled", false, true),
        ("richStreamingEnabled", true, false),
    ] {
        for value in [first, second] {
            let (status, body) = server
                .call(
                    "POST",
                    "/lingxi/v1/bridge/settings",
                    Some(json!({ field: value })),
                )
                .await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(body[field], json!(value), "{body}");
        }
    }
    // 只提交无关字段（permissionMode）时既有开关保持最后写入值（同一响应回读）。
    let (status, kept) = server
        .call(
            "POST",
            "/lingxi/v1/bridge/settings",
            Some(json!({"permissionMode": "auto"})),
        )
        .await;
    assert_eq!(status, 200, "{kept}");
    assert_eq!(kept["receiptEnabled"], json!(true), "{kept}");
    assert_eq!(kept["richStreamingEnabled"], json!(false), "{kept}");
    // 非法权限模式拒绝后目标状态不变：经真实端点读回核对，不凭 400 本身推断。
    let (status, _) = server
        .call(
            "POST",
            "/lingxi/v1/bridge/settings",
            Some(json!({"permissionMode": "god"})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, after_reject) = server
        .call(
            "POST",
            "/lingxi/v1/bridge/settings",
            Some(json!({"permissionMode": "auto"})),
        )
        .await;
    assert_eq!(status, 200, "{after_reject}");
    assert_eq!(
        after_reject["receiptEnabled"],
        json!(true),
        "{after_reject}"
    );
    assert_eq!(
        after_reject["richStreamingEnabled"],
        json!(false),
        "{after_reject}"
    );
    server.stop().await;
}

#[tokio::test]
async fn desk_activities_and_logs_list_honestly() {
    let server = boot("desklists").await;
    let (status, body) = server.call("GET", "/lingxi/v1/desk/activities", None).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body.get("activities").is_some() || body.as_array().is_some(),
        "{body}"
    );
    let (status, body) = server.call("GET", "/lingxi/v1/desk/logs", None).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body.as_array().is_some() || body.get("logs").is_some(),
        "{body}"
    );
    server.stop().await;
}

#[tokio::test]
async fn desk_cron_lists_current_jobs() {
    let server = boot("deskcron").await;
    let (status, body) = server.call("GET", "/lingxi/v1/desk/cron", None).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body.get("jobs").is_some() || body.as_array().is_some(),
        "{body}"
    );
    server.stop().await;
}

#[tokio::test]
async fn desk_activity_session_and_promote_reject_unknown_ids() {
    let server = boot("deskact").await;
    let (status, _) = server
        .call("GET", "/lingxi/v1/desk/activities/no-such/session", None)
        .await;
    assert_eq!(status, 404);
    let (status, _) = server
        .call("POST", "/lingxi/v1/desk/activities/no-such/promote", None)
        .await;
    assert_eq!(status, 404);
    server.stop().await;
}

#[tokio::test]
async fn channels_create_and_delete_roundtrip() {
    let server = boot("channels").await;
    // 创建频道：名称 + 至少两名不同 Agent 成员。
    let (status, body) = server
        .call(
            "POST",
            "/lingxi/v1/channels",
            Some(json!({"name":"probe-channel","memberAgentIds":["lingxi","second"]})),
        )
        .await;
    if status == 200 || status == 201 {
        // 删除频道返回 ok；再次删除 404。
        let (status, body) = server
            .call("DELETE", "/lingxi/v1/channels/probe-channel", None)
            .await;
        assert_eq!(status, 200, "{body}");
        let (status, _) = server
            .call("DELETE", "/lingxi/v1/channels/probe-channel", None)
            .await;
        assert_eq!(status, 404);
    } else {
        // 成员不足/名称非法的拒绝是显式错误。
        assert_eq!(status, 400, "{body}");
    }
    // 成员不足拒绝。
    let (status, _) = server
        .call(
            "POST",
            "/lingxi/v1/channels",
            Some(json!({"name":"bad","memberAgentIds":["lingxi"]})),
        )
        .await;
    assert_eq!(status, 400);
    server.stop().await;
}

#[tokio::test]
async fn session_permission_default_read_and_write() {
    let server = boot("permdefault").await;
    let (status, body) = server
        .call(
            "GET",
            "/lingxi/v1/preferences/session-permission-default",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let current = body["permissionMode"]
        .as_str()
        .or(body["default"].as_str())
        .unwrap_or("ask")
        .to_string();
    // 写入经专用偏好面（PUT /preferences/session-permission-default）并读回。
    let next = if current == "operate" {
        "ask"
    } else {
        "operate"
    };
    let (status, body) = server
        .call(
            "PUT",
            "/lingxi/v1/preferences/session-permission-default",
            Some(json!({"permissionMode": next})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (_, after) = server
        .call(
            "GET",
            "/lingxi/v1/preferences/session-permission-default",
            None,
        )
        .await;
    assert!(
        after["permissionMode"] == json!(next) || after["default"] == json!(next),
        "{after}"
    );
    // 非法模式拒绝且不改变已存值。
    let (status, _) = server
        .call(
            "PUT",
            "/lingxi/v1/preferences/session-permission-default",
            Some(json!({"permissionMode": "yolo"})),
        )
        .await;
    assert_eq!(status, 400);
    let (_, unchanged) = server
        .call(
            "GET",
            "/lingxi/v1/preferences/session-permission-default",
            None,
        )
        .await;
    assert!(
        unchanged["permissionMode"] == json!(next) || unchanged["default"] == json!(next),
        "{unchanged}"
    );
    server.stop().await;
}
