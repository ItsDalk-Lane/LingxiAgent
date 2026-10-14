//! R06 A 块剩余 7 叶的专属检查：PUT /agents/{id}/config 供应商读回链
//! （保存/删除/内联凭证转存三近义叶）、current_status 只读面、experiments
//! 读/写、cache-snapshot observation 读。
//!
//! 路由尚未在 lib.rs 注册（注册由主控统一），这里用 axum::Router 自行
//! 挂载处理器 + 真实 AuthService 鉴权层（本地令牌 401/200 语义）。
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::response::IntoResponse as _;
use lingxi_service::agents_admin::agent_config_readback as readback;
use lingxi_service::{
    prepare_layout, ConnectionKind, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ── 服务骨架（BOOT_SEQ 原子序号防时钟碰撞）──────────────────────────────────

struct Server {
    addr: String,
    token: String,
    home: PathBuf,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

async fn auth_guard_port(
    axum::extract::State(state): axum::extract::State<ServiceState>,
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
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
            axum::Json(json!({ "error": "invalid_credential" })),
        )
            .into_response(),
    }
}

async fn boot() -> Server {
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-readback-{uniq}"));
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
    let router = axum::Router::new()
        // 助手集合面（建助手/切换焦点，为 A/B 双路径准备真实数据）。
        .route(
            "/lingxi/v1/agents",
            axum::routing::post(lingxi_service::agents_admin::agents_create_route),
        )
        .route(
            "/lingxi/v1/agents/switch",
            axum::routing::post(lingxi_service::agents_admin::agents_switch_route),
        )
        // 本块五个处理器。
        .route(
            "/lingxi/v1/agents/{agent_id}/config",
            axum::routing::get(readback::agent_config_get_route)
                .put(readback::agent_config_put_route),
        )
        .route(
            "/lingxi/v1/current-status",
            axum::routing::get(readback::current_status_route),
        )
        .route(
            "/lingxi/v1/experiments",
            axum::routing::get(readback::experiments_list_route),
        )
        .route(
            "/lingxi/v1/experiments/{experiment_id}",
            axum::routing::patch(readback::experiment_patch_route),
        )
        .route(
            "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation",
            axum::routing::get(readback::cache_snapshot_observation_get_route)
                .delete(readback::cache_snapshot_observation_delete_route),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_guard_port,
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
            .expect("readback fixture HTTP 服务");
    });
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

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_default()).unwrap_or(Value::Null)
}

fn read_catalog(server: &Server) -> Value {
    read_json(&server.home.join("provider-catalog.json"))
}

fn preferences(server: &Server) -> Value {
    read_json(&server.home.join("user/preferences.json"))
}

fn yaml_of(server: &Server, agent_id: &str) -> String {
    std::fs::read_to_string(
        server
            .home
            .join("agents")
            .join(agent_id)
            .join("config.yaml"),
    )
    .unwrap_or_default()
}

fn mtime(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap()
}

async fn shutdown(server: Server) {
    server.stop.send(()).expect("仍在监听");
    let _ = tokio::time::timeout(Duration::from_secs(10), server.handle)
        .await
        .expect("限时停机");
    let _ = std::fs::remove_dir_all(&server.home);
}

/// 造两个助手：A（请求助手，带旧内联 api/embedding_api 凭证）与 B（焦点）。
async fn make_agents(server: &Server) {
    ok(
        server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "A", "id": "agreada"})),
    )
    .await;
    ok(
        server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "B", "id": "agreadb"})),
    )
    .await;
    ok(
        server,
        "POST",
        "/lingxi/v1/agents/switch",
        Some(&json!({"id": "agreadb"})),
    )
    .await;
    // A 预置旧内联凭证（磁盘直写，等同现役迁移前的 config.yaml 状态）。
    std::fs::write(
        server.home.join("agents/agreada/config.yaml"),
        "agent:\n  name: A\napi:\n  provider: myprov\n  api_key: old-inline\n  base_url: https://a.example/v1\nembedding_api:\n  provider: embedprov\n",
    )
    .expect("写 A 配置");
}

#[tokio::test]
async fn put_config_provider_save_readback() {
    let server = boot().await;
    make_agents(&server).await;
    let home = &server.home;
    let a_config = home.join("agents/agreada/config.yaml");
    let _b_config = home.join("agents/agreadb/config.yaml");

    // ── 保存（9E3981A9BFCA）：纯供应商请求 → 目录/插件/投影/焦点 YAML ──
    let a_before = std::fs::read(&a_config).unwrap();
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({
            "providers": {
                "my-custom": {
                    "api_key": "sk-custom-1",
                    "base_url": "https://custom.example/v1",
                    "models": ["m-one", { "id": "m-two", "name": "M Two" }]
                }
            }
        })),
    )
    .await;
    assert_eq!(status, 200, "供应商保存：{body}");

    // 权威目录：自定义 provider 的用户数据（含明文 key）以 catalog 为准。
    let catalog = read_catalog(&server);
    assert_eq!(
        catalog["providers"]["my-custom"]["api_key"],
        json!("sk-custom-1"),
        "目录保存 api_key"
    );
    assert!(
        !catalog["providers"]["my-custom"]
            .as_object()
            .unwrap()
            .contains_key("base_url"),
        "base_url 是定义字段：目录 overlay 不含（留在插件文件）"
    );
    // 定义字段进插件文件（本地自定义 → 条件插件文件存在）。
    let plugin_json = read_json(&home.join("provider-plugins/my-custom/providers/my-custom.json"));
    assert_eq!(plugin_json["id"], json!("my-custom"), "插件文件落盘");
    assert_eq!(
        plugin_json["defaultBaseUrl"],
        json!("https://custom.example/v1")
    );
    assert!(
        home.join("provider-plugins/my-custom/manifest.json")
            .is_file(),
        "manifest 存在"
    );
    // 目录 overlay 不含定义字段。
    assert!(
        !catalog["providers"]["my-custom"]
            .as_object()
            .unwrap()
            .contains_key("displayName"),
        "定义字段留在插件文件，目录只存 overlay"
    );

    // 模型投影：派生自目录；无凭证的 key 不出现在投影（运行时引用）。
    let models_json = read_json(&home.join("models.json"));
    let projected = &models_json["providers"]["my-custom"];
    assert_eq!(projected["baseUrl"], json!("https://custom.example/v1"));
    assert_eq!(
        projected["apiKey"],
        json!("hana-runtime-api-key:my-custom"),
        "有字面 key → 运行时引用"
    );
    let ids: Vec<&str> = projected["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["m-one", "m-two"], "chat 模型按目录顺序投影");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.join("models.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "秘密投影 0600");
    }

    // 焦点 B 的 config.yaml 被刷新重写（纯供应商成功不经 A 的显式保存）。
    assert!(
        yaml_of(&server, "agreadb").contains("name: B"),
        "B 语义不变（仍叫 B）"
    );
    assert!(
        !yaml_of(&server, "agreadb").contains("my-custom"),
        "B 不含供应商数据"
    );
    // 请求助手 A 的 config.yaml 不经显式保存 → 字节不变。
    assert_eq!(
        std::fs::read(&a_config).unwrap(),
        a_before,
        "A 未被显式保存，字节不变"
    );
    // 仅另含全局字段才变化 preferences：纯供应商请求不写偏好。
    assert!(
        !home.join("user/preferences.json").exists()
            || !preferences(&server)
                .as_object()
                .unwrap()
                .contains_key("locale"),
        "纯供应商请求不写 locale"
    );

    // ── 投影幂等：内容不变且权限正确 → models.json 不重写 ──
    let models_path = home.join("models.json");
    let first_mtime = mtime(&models_path);
    tokio::time::sleep(Duration::from_millis(60)).await;
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({
            "providers": {
                "my-custom": {
                    "api_key": "sk-custom-1",
                    "base_url": "https://custom.example/v1",
                    "models": ["m-one", { "id": "m-two", "name": "M Two" }]
                }
            }
        })),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        mtime(&models_path),
        first_mtime,
        "内容不变且权限正确时不重写 models.json"
    );

    // ── 同请求多项：后项失败保留前项（部分成功，不把 HTTP 错误当回滚）──
    // 注：serde_json 无 preserve_order，providers 键按字典序处理
    // （aa-first 先于 zz-bad）——逐项顺序处理语义与现役一致。
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({
            "providers": {
                "aa-first": { "api_key": "k1", "base_url": "https://first.example/v1", "models": ["a"] },
                "zz-bad": { "api_key": "k2", "base_url": "https://bad.example/v1", "models": [ { "id": "m", "token": "cred" } ] }
            }
        })),
    )
    .await;
    assert_eq!(status, 500, "后项校验失败如实报错：{body}");
    let catalog = read_catalog(&server);
    assert!(
        catalog["providers"]["aa-first"]["api_key"] == json!("k1"),
        "前项保留（部分成功）"
    );
    assert!(
        catalog["providers"].get("zz-bad").is_none(),
        "失败项不入目录"
    );
    assert!(
        catalog["providers"]["my-custom"].is_object(),
        "先前保存不受影响"
    );

    // ── 全局字段：另含 locale 才写 preferences ──
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({
            "providers": { "my-custom": null },
            "locale": "zh-CN"
        })),
    )
    .await;
    assert_eq!(status, 200, "删除+全局：{body}");
    assert_eq!(
        preferences(&server)["locale"],
        json!("zh-CN"),
        "全局键落偏好"
    );
    assert!(
        !yaml_of(&server, "agreada").contains("zh-CN"),
        "全局键不落助手配置"
    );

    // ── 删除（38A989948FDD）：目录条目删、本地插件连文件删、投影收敛 ──
    let catalog = read_catalog(&server);
    assert!(
        catalog["providers"].get("my-custom").is_none(),
        "删除后目录无条目"
    );
    assert!(
        catalog["meta"]["deletedProviders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == &json!("my-custom")),
        "deletedProviders 记录"
    );
    assert!(
        !home.join("provider-plugins/my-custom").exists(),
        "本地插件连文件删除"
    );
    assert!(
        read_json(&home.join("models.json"))["providers"]
            .get("my-custom")
            .is_none(),
        "投影收敛"
    );

    // 删除不存在的条目：目录文件不改写。
    let catalog_bytes = std::fs::read(home.join("provider-catalog.json")).unwrap();
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "providers": { "never-existed": null } })),
    )
    .await;
    assert_eq!(status, 200, "不存在条目删除返回成功");
    assert_eq!(
        std::fs::read(home.join("provider-catalog.json")).unwrap(),
        catalog_bytes,
        "目录不改写"
    );

    // ── 内置供应商：不写插件文件；删除后内置声明仍在（再存仍无插件）──
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreadb/config",
        Some(&json!({
            "providers": { "deepseek": { "api_key": "sk-ds", "base_url": "https://api.deepseek.com", "models": ["deepseek-v4-pro"] } }
        })),
    )
    .await;
    assert_eq!(status, 200, "内置保存：{body}");
    assert!(
        read_catalog(&server)["providers"]["deepseek"]["api_key"] == json!("sk-ds"),
        "内置条目进目录"
    );
    assert!(
        !home.join("provider-plugins/deepseek").exists(),
        "内置供应商不写插件文件"
    );
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreadb/config",
        Some(&json!({ "providers": { "deepseek": null } })),
    )
    .await;
    assert_eq!(status, 200);
    assert!(read_catalog(&server)["providers"].get("deepseek").is_none());
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreadb/config",
        Some(&json!({
            "providers": { "deepseek": { "api_key": "sk-ds2", "base_url": "https://api.deepseek.com", "models": ["deepseek-v4-pro"] } }
        })),
    )
    .await;
    assert_eq!(status, 200, "删除后再存：内置声明仍在");
    assert!(
        !home.join("provider-plugins/deepseek").exists(),
        "内置声明仍在（重建不写插件）"
    );

    // ── 校验拒绝：official deepseek 保留模型 id ──
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreadb/config",
        Some(&json!({
            "providers": { "deepseek": { "api_key": "k", "base_url": "https://api.deepseek.com", "models": ["deepseek"] } }
        })),
    )
    .await;
    assert_eq!(status, 500, "保留模型 id 拒绝");
    assert!(
        read_catalog(&server)["providers"]["deepseek"]["api_key"] == json!("sk-ds2"),
        "拒绝后目录保持先前值"
    );

    shutdown(server).await;
}

#[tokio::test]
async fn put_config_inline_credential_transfer_readback() {
    let server = boot().await;
    make_agents(&server).await;
    let home = &server.home;

    // ── 转存（464E58DEC9E2）：api 块凭证 → 目录；A YAML 清空不留明文 ──
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "api": { "api_key": "sk-new-1" } })),
    )
    .await;
    assert_eq!(status, 200, "转存：{body}");
    let catalog = read_catalog(&server);
    assert_eq!(
        catalog["providers"]["myprov"]["api_key"],
        json!("sk-new-1"),
        "fallback provider 来自 A 的 config.yaml api.provider"
    );
    let a_yaml = yaml_of(&server, "agreada");
    assert!(!a_yaml.contains("sk-new-1"), "A YAML 不留明文");
    assert!(!a_yaml.contains("old-inline"), "旧明文被清空");
    let a_json = lingxi_adapters_yaml_json(&server, "agreada");
    assert_eq!(a_json["api"]["api_key"], json!(""), "清空字段写回 YAML");
    assert_eq!(a_json["api"]["base_url"], json!(""), "清空字段写回 YAML");
    assert_eq!(a_json["api"]["provider"], json!("myprov"), "provider 保留");
    // 464E58DEC9E2 G6（端口结构行为）：显式 agentId=A 的保存只写 A 的
    // YAML——端口无懒加载运行时实例概念，不存在「A 未加载回退焦点 B」
    // 分支；焦点 B 的 YAML 不被 A 的显式保存污染（无回退写入）。
    let b_yaml_after_a_save = yaml_of(&server, "agreadb");
    assert!(
        !b_yaml_after_a_save.contains("sk-new-1"),
        "显式 A 保存不回退写焦点 B"
    );
    assert!(
        !b_yaml_after_a_save.contains("myprov"),
        "A 的 provider 归属不串入 B"
    );

    // models.json 投影已刷新（providersChanged）。
    assert!(home.join("models.json").is_file(), "投影生成");

    // ── 掩码值不覆盖目录密文 ──
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "api": { "provider": "myprov", "api_key": "********" } })),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        read_catalog(&server)["providers"]["myprov"]["api_key"],
        json!("sk-new-1"),
        "掩码解析回既有值"
    );

    // ── embedding_api 路径（显式 provider）──
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "embedding_api": { "provider": "embed-explicit", "api_key": "sk-emb", "base_url": "https://emb.example/v1" } })),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        read_catalog(&server)["providers"]["embed-explicit"]["api_key"],
        json!("sk-emb")
    );

    // ── provider 无法解析 → 先 400，目录不动 ──
    // 造一个没有 api.provider 的助手。
    ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "C", "id": "agreadc"})),
    )
    .await;
    let catalog_before = std::fs::read(home.join("provider-catalog.json")).unwrap();
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreadc/config",
        Some(&json!({ "api": { "api_key": "sk-nope" } })),
    )
    .await;
    assert_eq!(status, 400, "provider 缺失 400：{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("provider is required"),
        "错误对用户可见"
    );
    assert_eq!(
        std::fs::read(home.join("provider-catalog.json")).unwrap(),
        catalog_before,
        "400 不动目录"
    );

    // ── 转存成功后后段失败：目录已存凭证不当作回滚 ──
    // providers 先于 api 处理：providers 失败时 api 转存未执行。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({
            "providers": { "zz-bad": { "api_key": "x", "base_url": "https://bad.example/v1", "models": [ { "id": "m", "token": "cred" } ] } },
            "api": { "provider": "keptprov", "api_key": "kept-key" }
        })),
    )
    .await;
    assert_eq!(status, 500, "providers 段失败：{status}");
    assert!(
        read_catalog(&server)["providers"].get("keptprov").is_none(),
        "失败请求里 api 段未执行（顺序：providers 在 api 前）"
    );
    // 单独转存成功。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "api": { "provider": "keptprov", "api_key": "kept-key" } })),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        read_catalog(&server)["providers"]["keptprov"]["api_key"],
        json!("kept-key")
    );
    // 再来一个失败请求：目录中已存凭证保留（HTTP 报错不是回滚）。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({
            "providers": { "zz-bad2": { "api_key": "x", "base_url": "https://k2.example/v1", "models": [ { "id": "m", "token": "cred" } ] } }
        })),
    )
    .await;
    assert_eq!(status, 500, "后续失败");
    assert_eq!(
        read_catalog(&server)["providers"]["keptprov"]["api_key"],
        json!("kept-key"),
        "HTTP 报错不回滚目录中已存凭证"
    );

    // ── A=B：焦点助手自身的显式保存 ──
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreadb/config",
        Some(&json!({ "agent": { "name": "B2" } })),
    )
    .await;
    assert_eq!(status, 200);
    assert!(yaml_of(&server, "agreadb").contains("B2"), "焦点显式保存");

    shutdown(server).await;
}

/// 用与实现相同的 YAML→JSON 读取核对助手配置。
fn lingxi_adapters_yaml_json(server: &Server, agent_id: &str) -> Value {
    lingxi_adapters::storage::agent_memory::config::read_yaml_mapping_as_json(
        &server
            .home
            .join("agents")
            .join(agent_id)
            .join("config.yaml"),
    )
    .expect("YAML 合法")
    .expect("配置存在")
}

#[tokio::test]
async fn put_config_validation_and_auth() {
    let server = boot().await;
    make_agents(&server).await;

    // 无凭证 401（所有五类路由）。
    for (method, path) in [
        ("PUT", "/lingxi/v1/agents/agreada/config"),
        ("GET", "/lingxi/v1/current-status?action=list"),
        ("GET", "/lingxi/v1/experiments"),
        ("PATCH", "/lingxi/v1/experiments/session.compaction_mode"),
        (
            "GET",
            "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        ),
    ] {
        let (status, _) = http(&server, method, path, None, None).await;
        assert_eq!(status, 401, "{method} {path} 无凭证 401");
    }

    // 未知助手 404。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/nope/config",
        Some(&json!({ "agent": { "name": "x" } })),
    )
    .await;
    assert_eq!(status, 404);

    // 白名单：tools.disabled 拒核心工具。
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "tools": { "disabled": ["bash"] } })),
    )
    .await;
    assert_eq!(status, 400, "核心工具不可禁用：{body}");

    // experience.enabled 类型。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "experience": { "enabled": "yes" } })),
    )
    .await;
    assert_eq!(status, 400);

    // workspace_context 校验。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "workspace_context": { "custom_file_name": "../evil" } })),
    )
    .await;
    assert_eq!(status, 400);

    // yuan 校验：未注册模板 400。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "agent": { "yuan": "no-such-yuan" } })),
    )
    .await;
    assert_eq!(status, 400, "未知 yuan 拒绝");

    // 非对象 body。
    let (status, _) = http(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!("string")),
        Some(&server.token),
    )
    .await;
    assert_eq!(status, 400, "非对象 body 400");

    // agent 范围键 + 全局键混合：locale 不进 A 的 YAML。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "agent": { "name": "A2" }, "locale": "en-US", "keep_awake": true })),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(preferences(&server)["locale"], json!("en-US"));
    assert_eq!(preferences(&server)["keep_awake"], true);
    assert!(yaml_of(&server, "agreada").contains("A2"));
    assert!(
        !yaml_of(&server, "agreada").contains("en-US"),
        "locale 只进全局"
    );

    // editor 排版（24BC2E961E3E 的 PARTIAL 关联：editor 双控件的服务端
    // 写路径——合并 + 钳制 + 补默认；UI 双控件本身不在后端叶范围）。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "editor": { "markdown": { "bodyFontSize": 20, "lineHeight": 9 } } })),
    )
    .await;
    assert_eq!(status, 200, "editor 写入");
    let editor = &preferences(&server)["editor"]["markdown"];
    assert_eq!(editor["bodyFontSize"].as_f64(), Some(20.0), "显式值生效");
    assert_eq!(editor["lineHeight"].as_f64(), Some(2.2), "超上限钳制");
    assert_eq!(editor["fontPreset"], json!("follow"), "缺省补默认");
    assert_eq!(
        editor["heading1FontSize"].as_f64(),
        Some(28.0),
        "未触字段保持默认"
    );

    // memory.enabled 时间戳。
    let (status, _) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreada/config",
        Some(&json!({ "memory": { "enabled": false } })),
    )
    .await;
    assert_eq!(status, 200);
    let a_json = lingxi_adapters_yaml_json(&server, "agreada");
    assert!(a_json["memory"]["disabledSince"].is_string(), "关闭时间戳");

    shutdown(server).await;
}

#[tokio::test]
async fn current_status_list_get_and_readonly() {
    let server = boot().await;
    make_agents(&server).await;

    // 键目录与 get 一致：list 返回的每个键都能 get 200。
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/current-status?action=list",
        None,
    )
    .await;
    assert_eq!(status, 200, "list：{body}");
    let keys: Vec<String> = body["available"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["key"].as_str().unwrap().to_string())
        .collect();
    assert!(!keys.is_empty(), "目录非空");
    for key in &keys {
        let (status, body) = ok(
            &server,
            "GET",
            &format!("/lingxi/v1/current-status?action=get&key={key}"),
            None,
        )
        .await;
        assert_eq!(status, 200, "get {key}：{body}");
        assert!(body.get(key).is_some(), "get {key} 返回同名载荷");
    }

    // agent 值与运行时一致：焦点是 B。
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/current-status?action=get&key=agent",
        None,
    )
    .await;
    assert_eq!(body["agent"]["id"], json!("agreadb"), "焦点 B");
    assert_eq!(body["agent"]["name"], json!("B"));

    // 模型：无 models.chat → null（未知值显式标未知，不猜造）。
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/current-status?action=get&key=model",
        None,
    )
    .await;
    assert_eq!(body["model"]["id"], Value::Null, "无模型显式 null");
    // 写入 models.chat 后与配置一致。
    std::fs::write(
        server.home.join("agents/agreadb/config.yaml"),
        "agent:\n  name: B\nmodels:\n  chat:\n    id: gpt-x\n    provider: my-custom\n",
    )
    .unwrap();
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/current-status?action=get&key=model",
        None,
    )
    .await;
    assert_eq!(body["model"]["id"], json!("gpt-x"));
    assert_eq!(body["model"]["provider"], json!("my-custom"));

    // 会话：无 current_session_id → sessionId null。
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/current-status?action=get&key=session",
        None,
    )
    .await;
    assert_eq!(body["session"]["sessionId"], Value::Null);

    // time：真实当前时间形状。
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/current-status?action=get&key=time",
        None,
    )
    .await;
    let iso = body["time"]["iso"].as_str().unwrap();
    assert!(chrono_iso_ok(iso), "ISO 形状：{iso}");
    assert!(body["time"]["localDateTime"].as_str().unwrap().len() >= 19);
    assert!(
        body["time"]["utcOffset"].as_str().unwrap().starts_with('+')
            || body["time"]["utcOffset"].as_str().unwrap().starts_with('-')
    );

    // 未知 key / 缺 key：明确 400，不猜造。
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/current-status?action=get&key=nonexistent",
        None,
    )
    .await;
    assert_eq!(status, 400, "未知 key：{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("Unknown status key"));
    let (status, _) = ok(&server, "GET", "/lingxi/v1/current-status?action=get", None).await;
    assert_eq!(status, 400, "缺 key 400");

    // 不修改状态：偏好与配置字节不变。
    let prefs_path = server.home.join("user/preferences.json");
    let prefs_before = std::fs::read(&prefs_path).unwrap();
    let b_before = std::fs::read(server.home.join("agents/agreadb/config.yaml")).unwrap();
    for key in &keys {
        let _ = ok(
            &server,
            "GET",
            &format!("/lingxi/v1/current-status?action=get&key={key}"),
            None,
        )
        .await;
    }
    assert_eq!(
        std::fs::read(&prefs_path).unwrap(),
        prefs_before,
        "偏好不变"
    );
    assert_eq!(
        std::fs::read(server.home.join("agents/agreadb/config.yaml")).unwrap(),
        b_before,
        "配置不变"
    );

    shutdown(server).await;
}

fn chrono_iso_ok(iso: &str) -> bool {
    // RFC3339（毫秒）粗检：YYYY-MM-DDTHH:MM:SS.sssZ
    iso.len() == 24
        && iso.ends_with('Z')
        && iso.as_bytes().get(10) == Some(&b'T')
        && iso.as_bytes().get(4) == Some(&b'-')
        && iso.as_bytes().get(13) == Some(&b':')
}

#[tokio::test]
async fn experiments_read_and_write() {
    let server = boot().await;

    // 读（A5DE44FF584F）：已解析实验与有效值。
    let (status, body) = ok(&server, "GET", "/lingxi/v1/experiments", None).await;
    assert_eq!(status, 200, "读：{body}");
    let experiments = body["experiments"].as_array().unwrap();
    let ids: Vec<&str> = experiments
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            "session.compaction_mode",
            "session.instant_simple_compaction",
            "provider.deepseek_roleplay_reasoning_patch",
            "subagent.proactive_delegation",
        ],
        "注册表四项"
    );
    let by_id = |id: &str| {
        experiments
            .iter()
            .find(|e| e["id"].as_str() == Some(id))
            .unwrap()
            .clone()
    };
    assert_eq!(
        by_id("session.compaction_mode")["value"],
        json!("auto"),
        "默认值"
    );
    assert_eq!(
        by_id("session.instant_simple_compaction")["value"],
        json!(false)
    );
    assert_eq!(
        by_id("provider.deepseek_roleplay_reasoning_patch")["value"],
        json!(false)
    );

    // 写（C99A4322D0C1）：保存已注册实验 value 并返回生效值。
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/experiments/session.compaction_mode",
        Some(&json!({ "value": "pi_compatible" })),
    )
    .await;
    assert_eq!(status, 200, "写：{body}");
    assert_eq!(body["value"], json!("pi_compatible"), "返回生效值");
    assert_eq!(
        preferences(&server)["experiments"]["session.compaction_mode"],
        json!("pi_compatible"),
        "真实持久化"
    );
    let (_, body) = ok(&server, "GET", "/lingxi/v1/experiments", None).await;
    assert_eq!(
        body["experiments"][0]["value"],
        json!("pi_compatible"),
        "读回一致"
    );

    // 布尔实验。
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/experiments/session.instant_simple_compaction",
        Some(&json!({ "value": true })),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["value"], json!(true));

    // 无效输入：错误可见、无错误持久化。
    let prefs_path = server.home.join("user/preferences.json");
    let prefs_before = std::fs::read(&prefs_path).unwrap();
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/experiments/session.compaction_mode",
        Some(&json!({ "value": "banana" })),
    )
    .await;
    assert_eq!(status, 400, "非法枚举 400：{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("invalid experiment value"));
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/experiments/unknown-experiment",
        Some(&json!({ "value": true })),
    )
    .await;
    assert_eq!(status, 400, "未注册 id 400：{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("unknown experiment id"));
    assert_eq!(
        std::fs::read(&prefs_path).unwrap(),
        prefs_before,
        "拒绝路径零持久化"
    );

    // instant_simple 的旧版本读兼容：compaction mode 存过 lossy_local。
    let mut prefs = preferences(&server);
    prefs["experiments"] = json!({ "session.compaction_mode": "lossy_local" });
    std::fs::write(&prefs_path, serde_json::to_string_pretty(&prefs).unwrap()).unwrap();
    let (_, body) = ok(&server, "GET", "/lingxi/v1/experiments", None).await;
    assert_eq!(
        body["experiments"][1]["value"],
        json!(true),
        "v0.444.0 读兼容：compaction 存 lossy_local → instant 有效"
    );
    // 存了非布尔 → false。
    let mut prefs = preferences(&server);
    prefs["experiments"] = json!({ "session.instant_simple_compaction": "junk" });
    std::fs::write(&prefs_path, serde_json::to_string_pretty(&prefs).unwrap()).unwrap();
    let (_, body) = ok(&server, "GET", "/lingxi/v1/experiments", None).await;
    assert_eq!(
        body["experiments"][1]["value"],
        json!(false),
        "非布尔存值 → false"
    );

    // 退役实验：值固定 off（写入返回并落盘）。
    let (status, body) = ok(
        &server,
        "PATCH",
        "/lingxi/v1/experiments/memory.cache_snapshot_reflection",
        Some(&json!({ "value": "shadow" })),
    )
    .await;
    assert_eq!(status, 200, "退役实验：{body}");
    assert_eq!(body["value"], json!("off"), "退役值固定 off");
    assert_eq!(
        preferences(&server)["experiments"]["memory.cache_snapshot_reflection"],
        json!("off")
    );

    shutdown(server).await;
}

#[tokio::test]
async fn cache_snapshot_observation_read() {
    let server = boot().await;
    make_agents(&server).await;

    let observation_path = server
        .home
        .join("agents/agreada/experiments/cache-snapshot-reflection/latest.json");

    // 无文件 → null（不 404，不猜造）。
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
    )
    .await;
    assert_eq!(status, 200, "读：{body}");
    assert_eq!(body["observation"], Value::Null, "缺失显式 null");

    // 写入观察记录 → 读取并规范化。
    std::fs::create_dir_all(observation_path.parent().unwrap()).unwrap();
    std::fs::write(
        &observation_path,
        serde_json::to_string_pretty(&json!({
            "agentId": "agreada",
            "sessionPath": "/tmp/s.jsonl",
            "trigger": "session_close",
            "createdAt": "2026-10-01T00:00:00.000Z",
            "mode": "write",
            "status": "success",
            "reason": "",
            "usage": { "model": "m-1", "cachedTokens": 100, "missTokens": 5, "latencyMs": 12 },
            "summaryPreview": "S",
            "memoryMdPreview": "M",
            "baseMemoryMdHash": "h1",
            "cacheStrategy": "session_snapshot",
            "strict": true,
            "cachePrefixHash": "cp",
            "parentCachePrefixHash": "pp",
            "contractDiffs": [{ "field": "x" }],
            "degradeReason": "",
            "diagnostics": {
                "errorName": "None",
                "stack": ["a", "b", "c", "d", "e"],
                "requestModel": { "id": "m-1", "provider": "p", "api": "openai-completions", "hasBaseUrl": true, "hasQuirks": false }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let bytes_before = std::fs::read(&observation_path).unwrap();
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
    )
    .await;
    assert_eq!(status, 200);
    let observation = &body["observation"];
    assert_eq!(observation["version"], json!(1));
    assert_eq!(observation["agentId"], json!("agreada"));
    assert_eq!(observation["status"], json!("success"));
    assert_eq!(observation["mode"], json!("write"));
    assert_eq!(observation["cacheStrategy"], json!("session_snapshot"));
    assert_eq!(
        observation["diagnostics"]["stack"]
            .as_array()
            .unwrap()
            .len(),
        4,
        "stack 截断到 4 行"
    );
    assert_eq!(
        observation["diagnostics"]["requestModel"]["id"],
        json!("m-1")
    );
    assert_eq!(
        std::fs::read(&observation_path).unwrap(),
        bytes_before,
        "读取不修改文件"
    );

    // 规范化分支：非法 status/mode/策略被显式处理。
    std::fs::write(
        &observation_path,
        serde_json::to_string_pretty(&json!({
            "status": "weird", "mode": "shadow", "createdAt": "not-a-date",
            "cacheStrategy": "bogus"
        }))
        .unwrap(),
    )
    .unwrap();
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
    )
    .await;
    assert_eq!(status, 400, "未知 cache 策略显式报错：{body}");

    std::fs::write(
        &observation_path,
        serde_json::to_string_pretty(&json!({
            "status": "weird", "mode": "bogus", "createdAt": "not-a-date"
        }))
        .unwrap(),
    )
    .unwrap();
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["observation"]["status"],
        json!("failed"),
        "非法 status 归一 failed"
    );
    assert_eq!(
        body["observation"]["mode"],
        json!("shadow"),
        "非法 mode 归一 shadow"
    );
    assert!(
        chrono_iso_ok(body["observation"]["createdAt"].as_str().unwrap()),
        "非法日期替换为当前时间"
    );

    // 非法输入：缺 agentId / 路径逃逸 / 非法 JSON 体。
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation",
        None,
    )
    .await;
    assert_eq!(status, 400, "缺 agentId：{body}");
    let (status, _) = ok(
        &server,
        "GET",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=..%2Fescape",
        None,
    )
    .await;
    assert_eq!(status, 400, "路径逃逸拒绝");
    std::fs::write(&observation_path, b"[1,2,3]").unwrap();
    let (status, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
    )
    .await;
    assert_eq!(status, 400, "数组体拒绝：{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("must be an object"),
        "错误对用户可见"
    );

    shutdown(server).await;
}

/// B 块收尾叶（3C8CE915255E）：删除记忆缓存观察记录——settings.write 门
/// （classify_route）、unlink 真实文件、ENOENT 时 deleted:false、
/// 缺 agentId 400、无凭证 401。
#[tokio::test]
async fn cache_snapshot_observation_delete() {
    let server = boot().await;
    make_agents(&server).await;

    let observation_path = server
        .home
        .join("agents/agreada/experiments/cache-snapshot-reflection/latest.json");
    std::fs::create_dir_all(observation_path.parent().unwrap()).unwrap();
    std::fs::write(&observation_path, b"{\"status\":\"success\"}").unwrap();

    // 无文件助手：ENOENT → deleted:false（成功语义，非错误）。
    let (status, body) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreadb",
        None,
    )
    .await;
    assert_eq!(status, 200, "无文件删除：{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["deleted"], json!(false));

    // 有文件：真实 unlink + 读面变 null + 再删 deleted:false。
    let (status, body) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
    )
    .await;
    assert_eq!(status, 200, "删除：{body}");
    assert_eq!(body["deleted"], json!(true));
    assert!(!observation_path.exists(), "文件已移除");
    let (_, body) = ok(
        &server,
        "GET",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
    )
    .await;
    assert_eq!(body["observation"], Value::Null, "删除后读 null");
    let (_, body) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
    )
    .await;
    assert_eq!(body["deleted"], json!(false), "重复删除幂等");

    // 反例：缺 agentId 400；路径逃逸 400；无凭证 401。
    let (status, _) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation",
        None,
    )
    .await;
    assert_eq!(status, 400, "缺 agentId");
    let (status, _) = ok(
        &server,
        "DELETE",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=..%2Fescape",
        None,
    )
    .await;
    assert_eq!(status, 400, "路径逃逸拒绝");
    let (status, _) = http(
        &server,
        "DELETE",
        "/lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=agreada",
        None,
        None,
    )
    .await;
    assert_eq!(status, 401, "无凭证 401");

    shutdown(server).await;
}

// ── GET /agents/{id}/config（R00-T02-LA-BC675C664910）────────────────────────

fn write_preferences(server: &Server, mutate: impl FnOnce(&mut serde_json::Map<String, Value>)) {
    let mut prefs = preferences(server);
    if !prefs.is_object() {
        prefs = json!({});
    }
    mutate(prefs.as_object_mut().unwrap());
    std::fs::create_dir_all(server.home.join("user")).unwrap();
    std::fs::write(
        server.home.join("user/preferences.json"),
        serde_json::to_string_pretty(&prefs).unwrap(),
    )
    .unwrap();
}

fn agent_config_path(server: &Server, agent_id: &str) -> PathBuf {
    server
        .home
        .join("agents")
        .join(agent_id)
        .join("config.yaml")
}

const A_SECRETS: [&str; 7] = [
    "sk-plain-A-secret",
    "sk-embed-secret",
    "tg-bot-secret-123",
    "pw-secret-456",
    "tok-in-list-789",
    "sk-custom-plain-1",
    "oauth-client-secret-0",
];

#[tokio::test]
async fn get_config_reads_real_yaml_injects_globals_and_masks_every_secret() {
    let server = boot().await;
    make_agents(&server).await;
    // 目录里的供应商（经真实 PUT 写入，含明文 key 与两个模型）。
    let (status, body) = ok(
        &server,
        "PUT",
        "/lingxi/v1/agents/agreadb/config",
        Some(&json!({
            "providers": {
                "my-custom": {
                    "api_key": "sk-custom-plain-1",
                    "base_url": "https://custom.example/v1",
                    "models": ["m-one", { "id": "m-two", "name": "M Two" }]
                },
                "keyless": { "base_url": "https://keyless.example/v1", "models": [] }
            }
        })),
    )
    .await;
    assert_eq!(status, 200, "准备供应商：{body}");

    // A 的真实 config.yaml：多处秘密 + yaml 里的全局键诱饵 + 非布尔 experience。
    std::fs::write(
        agent_config_path(&server, "agreada"),
        "agent:\n  name: A\n  tagline: only-in-A\n\
api:\n  provider: myprov\n  api_key: sk-plain-A-secret\n  base_url: https://a.example/v1\n\
embedding_api:\n  provider: embedprov\n  api_key: sk-embed-secret\n\
experience:\n  note: keep-me\n\
channels:\n  telegram:\n    botToken: tg-bot-secret-123\n    chat: public-chat\n\
other:\n  password: pw-secret-456\n  clientSecret: oauth-client-secret-0\n  items:\n    - token: tok-in-list-789\n      label: visible-label\n\
locale: yaml-decoy-locale\nuser:\n  name: yaml-decoy-name\n",
    )
    .unwrap();
    write_preferences(&server, |prefs| {
        prefs.insert("locale".into(), json!("zh-CN"));
        prefs.insert("userName".into(), json!("Tester"));
        prefs.insert("timezone".into(), json!("Asia/Shanghai"));
        prefs.insert("sandbox".into(), json!(false));
        prefs.insert("keep_awake".into(), json!(true));
        prefs.insert("update_channel".into(), json!("beta"));
        prefs.insert("thinking_level".into(), json!("HIGH"));
        prefs.insert(
            "file_backup".into(),
            json!({ "retention_days": 7, "enabled": true }),
        );
        prefs.insert(
            "bridge".into(),
            json!({ "permissionMode": "operate", "receiptEnabled": false }),
        );
        prefs.insert(
            "automation".into(),
            json!({ "permissionMode": "read_only" }),
        );
        prefs.insert("heartbeat_master".into(), json!(false));
        prefs.insert("channels_enabled".into(), json!(true));
        prefs.insert("network_proxy".into(), json!({ "mode": "direct" }));
        prefs.insert(
            "learn_skills".into(),
            json!({ "enabled": false, "safety_review": true }),
        );
    });

    let a_yaml_before = std::fs::read(agent_config_path(&server, "agreada")).unwrap();
    let b_yaml_before = std::fs::read(agent_config_path(&server, "agreadb")).unwrap();
    let prefs_before = std::fs::read(server.home.join("user/preferences.json")).unwrap();
    let catalog_before = std::fs::read(server.home.join("provider-catalog.json")).unwrap();

    // 焦点是 B，读的是路径指定的 A。
    let (status, body) = ok(&server, "GET", "/lingxi/v1/agents/agreada/config", None).await;
    assert_eq!(status, 200, "{body}");

    // 读的是 A 的真实 config.yaml（含 yaml 里非秘密内容原样）。
    assert_eq!(body["agent"]["name"], json!("A"));
    assert_eq!(body["agent"]["tagline"], json!("only-in-A"));
    assert_eq!(body["api"]["provider"], json!("myprov"));
    assert_eq!(body["api"]["base_url"], json!("https://a.example/v1"));
    assert_eq!(body["channels"]["telegram"]["chat"], json!("public-chat"));
    assert_eq!(body["other"]["items"][0]["label"], json!("visible-label"));
    // experience.enabled 归一为布尔（缺省 → false），其余字段保留。
    assert_eq!(body["experience"]["enabled"], json!(false));
    assert_eq!(body["experience"]["note"], json!("keep-me"));
    // _raw 结构。
    assert_eq!(
        body["_raw"],
        json!({
            "api": { "provider": "myprov", "base_url": "https://a.example/v1" },
            "embedding_api": { "provider": "embedprov", "base_url": "" },
        })
    );

    // 全部秘密字段（递归，含数组内）掩码为 ********，且整个响应体不含任何明文。
    assert_eq!(body["api"]["api_key"], json!("********"));
    assert_eq!(body["embedding_api"]["api_key"], json!("********"));
    assert_eq!(body["channels"]["telegram"]["botToken"], json!("********"));
    assert_eq!(body["other"]["password"], json!("********"));
    assert_eq!(body["other"]["clientSecret"], json!("********"));
    assert_eq!(body["other"]["items"][0]["token"], json!("********"));
    let raw_text = body.to_string();
    for secret in A_SECRETS {
        assert!(!raw_text.contains(secret), "响应泄露明文 {secret}");
    }

    // 全局字段：偏好值覆盖 yaml 里的同名诱饵，缺省项取现役默认。
    assert_eq!(body["locale"], json!("zh-CN"));
    assert_eq!(body["user"]["name"], json!("Tester"));
    assert_eq!(body["timezone"], json!("Asia/Shanghai"));
    assert_eq!(body["sandbox"], json!(false));
    assert_eq!(body["sandbox_network"], json!(true));
    assert_eq!(body["hardware_acceleration"], json!(true));
    assert_eq!(body["file_backup"], json!({ "retention_days": 7.0 }));
    assert_eq!(body["update_channel"], json!("beta"));
    assert_eq!(body["auto_check_updates"], json!(true));
    assert_eq!(body["keep_awake"], json!(true));
    assert_eq!(body["thinking_level"], json!("high"));
    assert_eq!(body["editor"]["markdown"]["bodyFontSize"], json!(16.0));
    assert_eq!(
        body["capabilities"]["learn_skills"],
        json!({ "enabled": false, "safety_review": true })
    );
    assert_eq!(body["desk"]["heartbeat_master"], json!(false));
    assert_eq!(body["channels"]["enabled"], json!(true));
    assert_eq!(body["bridge"]["permissionMode"], json!("operate"));
    assert_eq!(body["bridge"]["readOnly"], json!(false));
    assert_eq!(body["bridge"]["receiptEnabled"], json!(false));
    assert_eq!(body["bridge"]["richStreamingEnabled"], json!(true));
    assert_eq!(body["automation"]["permissionMode"], json!("read_only"));
    assert_eq!(body["network_proxy"]["mode"], json!("direct"));
    assert_eq!(
        body["network_proxy"]["noProxy"],
        json!("localhost, 127.0.0.1, ::1")
    );

    // 供应商摘要：掩码 key、base_url、模型数。
    let custom = &body["providers"]["my-custom"];
    assert_eq!(custom["base_url"], json!("https://custom.example/v1"));
    assert_eq!(custom["api_key"], json!("********"));
    assert_eq!(custom["model_count"], json!(2));
    assert_eq!(custom["models"].as_array().unwrap().len(), 2);
    assert!(custom["api"].is_string());
    let keyless = &body["providers"]["keyless"];
    assert_eq!(keyless["api_key"], json!(""), "无 key 的供应商掩码为空串");
    assert_eq!(keyless["model_count"], json!(0));

    // availableTools：真实列表，含可选开关（即使该助手不是焦点），无重复。
    let tools: Vec<&str> = body["availableTools"]
        .as_array()
        .expect("availableTools 是数组")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for expected in [
        "read",
        "exec_command",
        "web_fetch",
        "lsp",
        "workflow",
        "update_settings",
    ] {
        assert!(tools.contains(&expected), "缺 {expected}: {tools:?}");
    }
    assert!(!tools.contains(&"computer"), "Computer Use 未启用 → 隐藏");
    let mut deduped = tools.clone();
    deduped.sort_unstable();
    deduped.dedup();
    assert_eq!(deduped.len(), tools.len(), "无重复");

    // 读面不写：config.yaml / 偏好 / 目录 / 焦点助手配置全部字节不变。
    assert_eq!(
        std::fs::read(agent_config_path(&server, "agreada")).unwrap(),
        a_yaml_before
    );
    assert_eq!(
        std::fs::read(agent_config_path(&server, "agreadb")).unwrap(),
        b_yaml_before
    );
    assert_eq!(
        std::fs::read(server.home.join("user/preferences.json")).unwrap(),
        prefs_before
    );
    assert_eq!(
        std::fs::read(server.home.join("provider-catalog.json")).unwrap(),
        catalog_before
    );

    shutdown(server).await;
}

#[tokio::test]
async fn get_config_identity_is_keyed_by_path_not_focus_and_never_discloses_another_agent() {
    let server = boot().await;
    make_agents(&server).await;
    std::fs::write(
        agent_config_path(&server, "agreada"),
        "agent:\n  name: A\napi:\n  provider: pa\n  api_key: sk-only-in-a\nmarker_a: alpha\n",
    )
    .unwrap();
    std::fs::write(
        agent_config_path(&server, "agreadb"),
        "agent:\n  name: B\napi:\n  provider: pb\n  api_key: sk-only-in-b\nmarker_b: beta\n",
    )
    .unwrap();

    // 焦点 = B：读 A 得 A，读 B 得 B，互不披露。
    let (status, a_body) = ok(&server, "GET", "/lingxi/v1/agents/agreada/config", None).await;
    assert_eq!(status, 200);
    assert_eq!(a_body["agent"]["name"], json!("A"));
    assert_eq!(a_body["marker_a"], json!("alpha"));
    assert!(a_body.get("marker_b").is_none());
    let a_text = a_body.to_string();
    assert!(
        !a_text.contains("sk-only-in-b") && !a_text.contains("\"B\"") && !a_text.contains("beta")
    );
    let (status, b_body) = ok(&server, "GET", "/lingxi/v1/agents/agreadb/config", None).await;
    assert_eq!(status, 200);
    assert_eq!(b_body["agent"]["name"], json!("B"));
    assert_eq!(b_body["marker_b"], json!("beta"));
    assert!(b_body.get("marker_a").is_none());
    assert!(!b_body.to_string().contains("sk-only-in-a"));

    // 切焦点到 A：读 B 仍是 B（焦点不决定身份）。
    let (status, _) = ok(
        &server,
        "POST",
        "/lingxi/v1/agents/switch",
        Some(&json!({"id": "agreada"})),
    )
    .await;
    assert_eq!(status, 200);
    let (_, b_after_switch) = ok(&server, "GET", "/lingxi/v1/agents/agreadb/config", None).await;
    assert_eq!(b_after_switch["agent"]["name"], json!("B"));
    assert_eq!(b_after_switch["marker_b"], json!("beta"));
    assert_eq!(b_after_switch["_raw"]["api"]["provider"], json!("pb"));
    assert_eq!(b_after_switch["api"]["api_key"], json!("********"));

    shutdown(server).await;
}

#[tokio::test]
async fn get_config_provider_registry_failure_gives_empty_providers_not_an_error() {
    let server = boot().await;
    make_agents(&server).await;
    std::fs::write(
        agent_config_path(&server, "agreada"),
        "agent:\n  name: A\napi:\n  provider: pa\n  api_key: sk-still-masked\n",
    )
    .unwrap();
    // 损坏的权威目录：供应商读取失败。
    let catalog_path = server.home.join("provider-catalog.json");
    std::fs::write(&catalog_path, "{ this is not json").unwrap();
    let broken = std::fs::read(&catalog_path).unwrap();

    let (status, body) = ok(&server, "GET", "/lingxi/v1/agents/agreada/config", None).await;
    assert_eq!(status, 200, "供应商失败不致整体失败：{body}");
    assert_eq!(
        body["providers"],
        json!({}),
        "失败 → 空对象（非 null/数组）"
    );
    assert!(body["providers"].is_object());
    // 其余内容照常：yaml、全局注入、availableTools、掩码。
    assert_eq!(body["agent"]["name"], json!("A"));
    assert_eq!(body["api"]["api_key"], json!("********"));
    assert!(body["availableTools"]
        .as_array()
        .is_some_and(|t| !t.is_empty()));
    assert!(!body.to_string().contains("sk-still-masked"));
    // 读面不修复/改写损坏目录。
    assert_eq!(std::fs::read(&catalog_path).unwrap(), broken);

    shutdown(server).await;
}

#[tokio::test]
async fn get_config_without_catalog_reads_without_creating_it() {
    let server = boot().await;
    make_agents(&server).await;
    assert!(!server.home.join("provider-catalog.json").exists());
    let (status, body) = ok(&server, "GET", "/lingxi/v1/agents/agreada/config", None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["providers"], json!({}));
    assert!(
        !server.home.join("provider-catalog.json").exists(),
        "读面不因遗留目录迁移而落盘"
    );
    shutdown(server).await;
}

#[tokio::test]
async fn get_config_gcs_gone_workspace_history_before_answering_only_for_that_agent() {
    let server = boot().await;
    make_agents(&server).await;
    let ws = server.home.join("ws");
    let keep = ws.join("keep");
    let keep2 = ws.join("keep2");
    std::fs::create_dir_all(&keep).unwrap();
    std::fs::create_dir_all(&keep2).unwrap();
    let gone = ws.join("gone");
    let gone2 = ws.join("gone2");
    let (keep_s, keep2_s, gone_s, gone2_s) = (
        keep.to_string_lossy().to_string(),
        keep2.to_string_lossy().to_string(),
        gone.to_string_lossy().to_string(),
        gone2.to_string_lossy().to_string(),
    );
    std::fs::write(
        agent_config_path(&server, "agreada"),
        format!(
            "agent:\n  name: A\ncwd_history:\n  - {gone_s}\n  - {keep_s}/\n  - {gone2_s}\n  - {keep2_s}\nlast_cwd: {gone_s}\ndesk:\n  home_folder: {gone2_s}\n  heartbeat_enabled: true\n"
        ),
    )
    .unwrap();
    // 另一助手同样带已消失目录：不属于本次请求，必须原样不动。
    let b_yaml = format!("agent:\n  name: B\ncwd_history:\n  - {gone_s}\nlast_cwd: {gone_s}\n");
    std::fs::write(agent_config_path(&server, "agreadb"), &b_yaml).unwrap();
    write_preferences(&server, |prefs| {
        prefs.insert("locale".into(), json!("en-US"));
        prefs.insert(
            "workspace_ui_state".into(),
            json!({ "version": 2, "workspaces": {
                keep_s.clone(): { "updatedAt": 1, "surfaces": {} },
                gone_s.clone(): { "updatedAt": 2, "surfaces": {} },
            }}),
        );
    });

    let (status, body) = ok(&server, "GET", "/lingxi/v1/agents/agreada/config", None).await;
    assert_eq!(status, 200, "{body}");
    // 响应已是 GC 之后的结果（先 GC 再读）。
    assert_eq!(body["cwd_history"], json!([keep_s, keep2_s]));
    assert!(body.get("last_cwd").is_none(), "last_cwd 已清：{body}");
    assert!(
        body["desk"].get("home_folder").is_none(),
        "home_folder 已清"
    );
    assert_eq!(
        body["desk"]["heartbeat_enabled"],
        json!(true),
        "同块其它键保留"
    );
    assert_eq!(body["agent"]["name"], json!("A"));

    // 磁盘副作用：A 的 YAML 真的被改写，B 的原样。
    let disk = lingxi_adapters_yaml_json(&server, "agreada");
    assert_eq!(disk["cwd_history"], json!([keep_s, keep2_s]));
    assert!(disk.get("last_cwd").is_none());
    assert!(disk["desk"].get("home_folder").is_none());
    assert_eq!(disk["desk"]["heartbeat_enabled"], json!(true));
    assert_eq!(
        std::fs::read_to_string(agent_config_path(&server, "agreadb")).unwrap(),
        b_yaml
    );
    // 偏好里已消失的工作区 UI 状态被清，存活项与无关偏好保留。
    let prefs = preferences(&server);
    let workspaces = prefs["workspace_ui_state"]["workspaces"]
        .as_object()
        .unwrap();
    assert!(workspaces.contains_key(&keep_s) && !workspaces.contains_key(&gone_s));
    assert_eq!(prefs["locale"], json!("en-US"));

    // 幂等：再读无变化 → 不重写文件。
    let a_mtime = mtime(&agent_config_path(&server, "agreada"));
    let prefs_mtime = mtime(&server.home.join("user/preferences.json"));
    let (status, again) = ok(&server, "GET", "/lingxi/v1/agents/agreada/config", None).await;
    assert_eq!(status, 200);
    assert_eq!(again["cwd_history"], body["cwd_history"]);
    assert_eq!(mtime(&agent_config_path(&server, "agreada")), a_mtime);
    assert_eq!(
        mtime(&server.home.join("user/preferences.json")),
        prefs_mtime
    );

    shutdown(server).await;
}

#[tokio::test]
async fn get_config_unknown_or_invalid_agent_is_404_with_no_side_effects() {
    let server = boot().await;
    make_agents(&server).await;
    let gone = server
        .home
        .join("ws/never-created")
        .to_string_lossy()
        .to_string();
    std::fs::write(
        agent_config_path(&server, "agreada"),
        format!("agent:\n  name: A\ncwd_history:\n  - {gone}\n"),
    )
    .unwrap();
    write_preferences(&server, |prefs| {
        prefs.insert(
            "workspace_ui_state".into(),
            json!({ "version": 2, "workspaces": { gone.clone(): { "updatedAt": 1, "surfaces": {} } } }),
        );
    });
    let a_before = std::fs::read(agent_config_path(&server, "agreada")).unwrap();
    let prefs_before = std::fs::read(server.home.join("user/preferences.json")).unwrap();
    let agents_before: Vec<_> = {
        let mut names: Vec<_> = std::fs::read_dir(server.home.join("agents"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        names
    };

    for path in [
        "/lingxi/v1/agents/nope/config",
        "/lingxi/v1/agents/Bad%20Id/config",
        "/lingxi/v1/agents/..%2Fagreada/config",
    ] {
        let (status, body) = ok(&server, "GET", path, None).await;
        assert_eq!(status, 404, "{path}: {body}");
        assert_eq!(body, json!({ "error": "agent not found" }), "{path}");
    }
    // 无凭证 401。
    let (status, _) = http(
        &server,
        "GET",
        "/lingxi/v1/agents/agreada/config",
        None,
        None,
    )
    .await;
    assert_eq!(status, 401);

    // 拒绝路径不触发 GC，也不建目录/写偏好。
    assert_eq!(
        std::fs::read(agent_config_path(&server, "agreada")).unwrap(),
        a_before
    );
    assert_eq!(
        std::fs::read(server.home.join("user/preferences.json")).unwrap(),
        prefs_before
    );
    let mut agents_after: Vec<_> = std::fs::read_dir(server.home.join("agents"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    agents_after.sort();
    assert_eq!(agents_after, agents_before);

    shutdown(server).await;
}

#[tokio::test]
async fn get_config_experience_enabled_is_boolean_and_invalid_config_fails_loudly() {
    let server = boot().await;
    make_agents(&server).await;
    // enabled: true 原样为 true；B 无 experience 块 → 归一出 {enabled:false}。
    std::fs::write(
        agent_config_path(&server, "agreada"),
        "agent:\n  name: A\nexperience:\n  enabled: true\n",
    )
    .unwrap();
    let (_, a_body) = ok(&server, "GET", "/lingxi/v1/agents/agreada/config", None).await;
    assert_eq!(a_body["experience"], json!({ "enabled": true }));
    std::fs::write(agent_config_path(&server, "agreadb"), "agent:\n  name: B\n").unwrap();
    let (_, b_body) = ok(&server, "GET", "/lingxi/v1/agents/agreadb/config", None).await;
    assert_eq!(b_body["experience"], json!({ "enabled": false }));

    // 违反端口配置校验的磁盘内容：显式 500，不静默改写/降级，也不泄露密钥。
    let invalid =
        "agent:\n  name: A\napi:\n  api_key: sk-invalid-case\nexperience:\n  enabled: \"yes\"\n";
    std::fs::write(agent_config_path(&server, "agreada"), invalid).unwrap();
    let (status, body) = ok(&server, "GET", "/lingxi/v1/agents/agreada/config", None).await;
    assert_eq!(status, 500, "{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("experience.enabled"));
    assert!(!body.to_string().contains("sk-invalid-case"));
    assert_eq!(
        std::fs::read_to_string(agent_config_path(&server, "agreada")).unwrap(),
        invalid
    );

    shutdown(server).await;
}
