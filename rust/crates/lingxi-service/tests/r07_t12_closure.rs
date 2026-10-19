//! R07-T12：A23 运行时路由矩阵（每个已注册路由真实可达、受鉴权把守）与
//! A24 禁止代理回旧内核（纯 Rust 链闭环：会话→执行→账本→观测，无旧服务）。
use lingxi_kernel::ports::StoragePort;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r07-t12-{tag}-{}-{}",
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

struct Server {
    addr: SocketAddr,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
    state: ServiceState,
    home: PathBuf,
}

impl Server {
    /// 裸 TCP HTTP 调用（与 r07_t10_observability 同一已证形状）。
    async fn call(
        &self,
        token: Option<&str>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Vec<u8>) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        let mut stream = TcpStream::connect(self.addr).await.expect("connect");
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
        (status, buf[header_end + 4..].to_vec())
    }

    async fn json(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let (status, raw) = self.call(Some(&self.token.clone()), method, path, body).await;
        let parsed = serde_json::from_slice(&raw).unwrap_or_else(|_| {
            panic!("{method} {path} -> {status} non-JSON: {}", String::from_utf8_lossy(&raw))
        });
        (status, parsed)
    }

    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
        self.state.storage().close().await.expect("close storage");
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

async fn boot(home: PathBuf, deps: ServiceDeps) -> Server {
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let serving = state.clone();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            serving,
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
    Server { addr, token, stop: stop_tx, handle, state, home }
}

/// 与 A23 静态生产者同一提取法（同源不同实现）：只扫 cfg(test) 之前的
/// 生产代码，逐字符提取 `.route("..."` 的路径与注册文件。不用正则 crate。
fn registered_routes() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut routes = Vec::new();
    for entry in std::fs::read_dir(&src).expect("service src dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read source");
        let production = match text.find("#[cfg(test)]") {
            Some(cut) => &text[..cut],
            None => &text,
        };
        let needle = ".route(\"";
        let mut rest = production;
        while let Some(at) = rest.find(needle) {
            let after = &rest[at + needle.len()..];
            let end = after.find('"').expect("route literal closes");
            let route = &after[..end];
            if route.starts_with('/') {
                routes.push((
                    route.to_string(),
                    entry.file_name().to_string_lossy().into_owned(),
                ));
            }
            rest = &after[end + 1..];
        }
    }
    routes.sort();
    routes.dedup();
    routes
}

/// 路径参数段替换为探测值；鉴权先于参数解析，404 判定不受影响。
fn probe_path(route: &str) -> String {
    let mut out = String::with_capacity(route.len());
    let mut rest = route;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').expect("param closes") + open;
        out.push_str("probe");
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// A23 运行时半边：每条静态注册的路由在真实服务上——持本地令牌探测不得 404
/// （未接线的声明是空洞）；无凭证不得越过鉴权（公共面除外）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r07_t12_a23_runtime_route_matrix() {
    // 探针要对每条路由各打两发（全表 320+ 次），默认 240/窗 的对等限流会
    // 在中途 429——本用例测的是接线与鉴权，不是限流（限流另有 R02 专属
    // 测试），故把窗口预算抬到探针总量够不着的高度。
    let deps = ServiceDeps {
        rate_max: 100_000,
        ..ServiceDeps::default()
    };
    let server = boot(unique_dir("route-matrix"), deps).await;
    let routes = registered_routes();
    assert!(
        routes.len() > 100,
        "route inventory suspiciously small: {}",
        routes.len()
    );

    // 公共面精确镜像 classify_route 的 GET-Public 分支（auth.rs）：
    // 健康、OAuth 回调、web-auth 会话查询、mobile/desktop 静态壳。
    // 新增公共路由若未同步本清单会被 unguarded 响亮揪出——这是有意的。
    // 注意 web-auth/login|logout 仅 POST 公共，GET 探测必须仍 401。
    let public_get = [
        "/lingxi/v1/health",
        "/lingxi/v1/mcp/oauth/callback",
        "/lingxi/v1/web-auth/session",
        "/mobile",
        "/mobile/",
        "/mobile/{*path}",
        "/desktop",
        "/desktop/",
        "/desktop/{*path}",
    ];
    let mut holes = Vec::new();
    let mut unguarded = Vec::new();
    for (route, source) in &routes {
        let probe = probe_path(route);
        // 空洞判定用无人注册的 TRACE 方法：已注册路径 → 路由层 405（处理器
        // 不参与，静态资产缺文件、资源不存在的处理器级 404 都不干扰）；
        // 未注册路径 → 404。鉴权先于路由，持本地令牌直达路由层。
        let (with, _) = server
            .call(Some(&server.token.clone()), "TRACE", &probe, None)
            .await;
        if with == 404 {
            holes.push(format!("{route} ({source})"));
        }
        if public_get.contains(&route.as_str()) {
            continue;
        }
        let (without, _) = server.call(None, "GET", &probe, None).await;
        if without != 401 {
            unguarded.push(format!("{route} ({source}) -> {without}"));
        }
    }
    assert!(holes.is_empty(), "注册声明但未接线的路由空洞: {holes:?}");
    assert!(unguarded.is_empty(), "无凭证未 401 的受守路由: {unguarded:?}");
    server.stop().await;
}

/// 最小 OpenAI 兼容 SSE 替身：一次应答即完结（无工具），记录全部请求路径。
struct Stub {
    addr: SocketAddr,
    paths: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Stub {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let seen = paths.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 16384];
                    loop {
                        let count = socket.read(&mut buf).await.unwrap_or(0);
                        if count == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..count]);
                        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let head = String::from_utf8_lossy(&raw);
                    let path = head
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("")
                        .to_string();
                    seen.lock().unwrap().push(path);
                    // 与 r06_chat_wire 替身同一已证帧形：内容→stop→usage→DONE。
                    let mut body = String::new();
                    for frame in [
                        json!({"id":"c","choices":[{"index":0,"delta":{"role":"assistant","content":"闭环完成"},"finish_reason":null}]}),
                        json!({"id":"c","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                        json!({"id":"c","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":4}}),
                    ] {
                        body.push_str(&format!("data: {frame}\n\n"));
                    }
                    body.push_str("data: [DONE]\n\n");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Stub { addr, paths, task }
    }
}

/// A24：停用旧 Pi 内核后纯 Rust 链各能力域闭环——会话创建、真实执行、
/// 用量落账、观测查询全通；唯一出站 HTTP 是替身供应商（无任何旧服务地址）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r07_t12_a24_closed_loop_without_legacy_kernel() {
    let home = unique_dir("no-legacy");
    let stub = Stub::start().await;

    // 最小产品目录：只有人格模板，没有任何旧内核（server/、node 运行时）。
    // 装配形状与 r06_chat_wire 的 boot_with_quota 一致。
    let product = home.join("product");
    std::fs::create_dir_all(product.join("yuan")).expect("yuan dir");
    std::fs::write(
        product.join("yuan/lingxi.md"),
        "你叫{{agentName}}，与用户一起完成任务。",
    )
    .expect("yuan template");

    // 会话创建需要真实存在的助手（R07 目录收紧），按已证夹具登记 lingxi。
    std::fs::create_dir_all(home.join("agents/lingxi")).expect("lingxi dir");
    std::fs::write(
        home.join("agents/lingxi/config.yaml"),
        "agent:\n  name: lingxi\n  yuan: lingxi\nmemory:\n  enabled: false\n",
    )
    .expect("lingxi config");

    // 模型平面：唯一供应商是 loopback 替身（合成 key，非真实凭证）。
    let config = json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":format!("http://{}/v1",stub.addr),"auth":{"kind":"apiKey","apiKey":"A24_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true,"imageInput":true},"compat":{"contextWindow":200000,"reasoning":true}},"title":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000,"reasoning":true}},"summarize":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}}}});
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::from_value(config)
        .expect("model plane");
    let layout = prepare_layout(&home).expect("layout");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credentials"),
    );
    let compiler = Arc::new(
        lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
            lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                &home,
                product,
                lingxi_service::context_compiler::HostFacts {
                    platform: std::env::consts::OS.into(),
                    os_type: std::env::consts::OS.into(),
                    os_release: String::new(),
                    shell_label: "zsh".into(),
                    workspace: home.to_string_lossy().into_owned(),
                },
            ),
        )),
    );
    let server = boot(
        home.clone(),
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            credential_service: Some(credentials),
            context_compiler: Some(compiler),
            workspace_root: Some(home.clone()),
            ..ServiceDeps::default()
        },
    )
    .await;

    // 会话创建（默认 lingxi 助手，真实创建链）。
    let (status, body) = server
        .json(
            "POST",
            "/lingxi/v1/sessions",
            Some(json!({"sessionId":"a24","agentId":"lingxi","title":"a24"})),
        )
        .await;
    assert_eq!(status, 201, "session create: {body}");

    // 真实执行闭环。
    let (status, body) = server
        .json(
            "POST",
            "/lingxi/v1/sessions/a24/execute",
            Some(json!({"input":"闭环验证"})),
        )
        .await;
    assert_eq!(status, 200, "execute: {body}");
    let run_id = body["runId"].as_str().expect("runId").to_string();

    // run 真实完结（与 r06_chat_wire 同一已证判据）。
    let status = server
        .state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id=?1",
            vec![run_id.clone()],
        )
        .await
        .expect("query runs")
        .expect("run row exists");
    assert_eq!(status, "completed", "run must complete");

    // 用量账本真实落行（模型调用事实在观测面可查）。
    let (status, calls) = server
        .json(
            "POST",
            "/lingxi/v1/observability/query/calls",
            Some(json!({"filter":{"sessionId":"a24"}})),
        )
        .await;
    assert_eq!(status, 200, "observability query: {calls}");
    assert!(
        calls["calls"].as_array().is_some_and(|rows| !rows.is_empty()),
        "run 的模型调用必须真实落账可查: {calls}"
    );

    // 全部出站 HTTP 只去了替身供应商；没有旧服务地址被触碰。
    let paths = stub.paths.lock().unwrap().clone();
    assert!(!paths.is_empty(), "替身必须收到真实模型请求");
    assert!(
        paths.iter().all(|p| p.starts_with("/v1/")),
        "出站请求不得离开供应商替身边界: {paths:?}"
    );

    server.stop().await;
    stub.task.abort();
}
