//! R07-T12：A23 运行时路由矩阵（每个已注册路由真实可达、受鉴权把守）与
//! A24 禁止代理回旧内核（纯 Rust 链闭环：会话→执行→账本→观测，无旧服务）。
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
        tokio::time::timeout(Duration::from_secs(30), stream.read_to_end(&mut buf))
            .await
            .expect("HTTP read timeout")
            .expect("HTTP read");
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
        let (status, raw) = self
            .call(Some(&self.token.clone()), method, path, body)
            .await;
        let parsed = serde_json::from_slice(&raw).unwrap_or_else(|_| {
            panic!(
                "{method} {path} -> {status} non-JSON: {}",
                String::from_utf8_lossy(&raw)
            )
        });
        (status, parsed)
    }

    async fn stop(self) {
        let _ = self.stop.send(());
        let result = tokio::time::timeout(Duration::from_secs(15), self.handle)
            .await
            .expect("service must stop")
            .expect("service task join")
            .expect("service stop result");
        assert!(
            !result.drain_timed_out,
            "default process/worker drain must be confirmed"
        );
        self.state.storage().close().await.expect("close storage");
        // 独立合成根保留给同入口重开及证据核对，不复制任何真实用户数据。
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
    Server {
        addr,
        token,
        stop: stop_tx,
        handle,
        state,
        home,
    }
}

use lingxi_kernel::{
    ports::{StoragePort, ToolOutcome},
    toolcatalog::ToolTargetRef,
    RunContext,
};
use lingxi_protocol::{RunId, SessionId, ToolCallId};
use std::collections::{HashMap, VecDeque};

#[derive(Default)]
struct ExternalState {
    plans: HashMap<String, VecDeque<(String, Value)>>,
    requests: Vec<Value>,
    serial: u64,
}
struct Stub {
    addr: SocketAddr,
    state: Arc<Mutex<ExternalState>>,
    task: tokio::task::JoinHandle<()>,
}
impl Stub {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(ExternalState::default()));
        let observed = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let observed = observed.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buffer = [0u8; 16384];
                    let end = loop {
                        let count = socket.read(&mut buffer).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buffer[..count]);
                        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            break end + 4;
                        }
                        assert!(raw.len() < 2 * 1024 * 1024, "synthetic HTTP header cap");
                    };
                    let header = String::from_utf8_lossy(&raw[..end]);
                    let length = header
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .expect("external JSON content length");
                    assert!(length < 8 * 1024 * 1024, "synthetic model input bounded");
                    while raw.len() < end + length {
                        let count = socket.read(&mut buffer).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buffer[..count]);
                    }
                    let body: Value = serde_json::from_slice(&raw[end..end + length]).unwrap();
                    let serialized = body.to_string();
                    let (step, serial) = {
                        let mut state = observed.lock().unwrap();
                        state.requests.push(body.clone());
                        state.serial += 1;
                        let serial = state.serial;
                        let scene = state
                            .plans
                            .keys()
                            .find(|scene| {
                                serialized.contains(scene.as_str())
                                    && state
                                        .plans
                                        .get(*scene)
                                        .is_some_and(|steps| !steps.is_empty())
                                    && body["tools"].as_array().is_some_and(|t| !t.is_empty())
                            })
                            .cloned();
                        let step = scene
                            .and_then(|scene| state.plans.get_mut(&scene).unwrap().pop_front());
                        (step, serial)
                    };
                    let frames = if let Some((suffix, args)) = step {
                        let name = body["tools"]
                            .as_array()
                            .and_then(|tools| {
                                tools
                                    .iter()
                                    .filter_map(|t| t["function"]["name"].as_str())
                                    .find(|name| name.ends_with(&suffix))
                            })
                            .unwrap_or("INDEPENDENT_TOOL_NOT_DECLARED");
                        vec![
                            json!({"id":format!("independent-{serial}"),"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":format!("independent-call-{serial}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]}),
                            json!({"id":format!("independent-{serial}"),"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":2}}),
                        ]
                    } else {
                        vec![
                            json!({"id":format!("independent-{serial}"),"choices":[{"index":0,"delta":{"role":"assistant","content":"INDEPENDENT_DEFAULT_PLANE_DONE"},"finish_reason":null}]}),
                            json!({"id":format!("independent-{serial}"),"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":2}}),
                        ]
                    };
                    let mut payload = String::new();
                    for frame in frames {
                        payload.push_str(&format!("data: {frame}\n\n"));
                    }
                    payload.push_str("data: [DONE]\n\n");
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        Self { addr, state, task }
    }
    fn plan(&self, scene: &str, steps: Vec<(String, Value)>) {
        let previous = self
            .state
            .lock()
            .unwrap()
            .plans
            .insert(scene.into(), steps.into());
        assert!(previous.is_none(), "scenario ids may not be reused");
    }
    fn requests(&self) -> Vec<Value> {
        self.state.lock().unwrap().requests.clone()
    }
    fn remaining(&self, scene: &str) -> usize {
        self.state.lock().unwrap().plans[scene].len()
    }
}
fn describe(name: &str) -> (String, Value) {
    ("mcp_describe_tool".into(), json!({"name":name}))
}
fn call(name: &str, args: Value) -> (String, Value) {
    ("mcp_call".into(), json!({"tool":name,"arguments":args}))
}
fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
async fn default_server(home: &Path, workspace: &Path, endpoint: SocketAddr) -> Server {
    let product = home.join("product");
    std::fs::create_dir_all(product.join("yuan")).unwrap();
    std::fs::write(
        product.join("yuan/lingxi.md"),
        "你叫{{agentName}}，测试仅使用合成资料。",
    )
    .unwrap();
    std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
    std::fs::write(
        home.join("agents/lingxi/config.yaml"),
        "agent:\n  name: lingxi\n  yuan: lingxi\nmemory:\n  enabled: false\n",
    )
    .unwrap();
    let config = json!({"providers":{"remote":{"protocol":"openai-completions","endpoint":format!("http://{endpoint}/v1"),"auth":{"kind":"apiKey","apiKey":"INDEPENDENT_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"remote","model":"chat","capabilities":{"tools":true,"imageInput":true},"compat":{"contextWindow":200000,"reasoning":true}},"title":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}},"summarize":{"provider":"remote","model":"chat","capabilities":{"tools":true},"compat":{"contextWindow":200000}}}});
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::from_value(config).unwrap();
    let layout = prepare_layout(home).unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    let compiler = Arc::new(
        lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
            lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                home,
                product,
                lingxi_service::context_compiler::HostFacts {
                    platform: std::env::consts::OS.into(),
                    os_type: std::env::consts::OS.into(),
                    os_release: String::new(),
                    shell_label: "zsh".into(),
                    workspace: workspace.to_string_lossy().into_owned(),
                },
            ),
        )),
    );
    // 不注入TurnProvider/ToolRegistry/ToolGateway/ToolExecutor/ProcessSupervisor，不手工registrar。
    boot(
        home.to_path_buf(),
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            credential_service: Some(credentials),
            context_compiler: Some(compiler),
            workspace_root: Some(workspace.to_path_buf()),
            ..ServiceDeps::default()
        },
    )
    .await
}
macro_rules! check {($cond:expr,$($arg:tt)*)=>{if !($cond){return Err(format!($($arg)*));}};}
async fn run_scene(
    server: &Server,
    stub: &Stub,
    scene: &str,
    steps: Vec<(String, Value)>,
) -> Result<(String, Vec<lingxi_kernel::model_exchange::ExchangeItem>), String> {
    // 历史使用公开的真实模型投影；核心与存储不替身。
    stub.plan(scene, steps);
    let (status, body) = server
        .json(
            "POST",
            "/lingxi/v1/sessions/independent-default/execute",
            Some(json!({"input":scene})),
        )
        .await;
    check!(status == 200, "execute status {status}: {body}");
    let id = body["runId"]
        .as_str()
        .ok_or("missing real run id")?
        .to_owned();
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let value = server
                .state
                .storage()
                .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![id.clone()])
                .await
                .map_err(|e| e.to_string())?
                .ok_or("run absent")?;
            if ["completed", "failed", "cancelled", "no_final"].contains(&value.as_str()) {
                return Ok::<_, String>(value);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| format!("real run deadline {scene}"))??;
    let status = server
        .state
        .storage()
        .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![id.clone()])
        .await
        .map_err(|e| e.to_string())?
        .unwrap();
    check!(status == "completed", "real run {scene} ended {status}");
    check!(
        stub.remaining(scene) == 0,
        "model never consumed all steps for {scene}"
    );
    let history = lingxi_service::model_history::ModelHistory::new(server.state.storage().clone())
        .load(
            &lingxi_kernel::Principal::LocalUser,
            "lingxi",
            "independent-default",
            "",
        )
        .await
        .map_err(|e| e.to_string())?;
    println!("INDEPENDENT_REAL_RUN {scene} {id} completed");
    Ok((id, history))
}
fn history_text(history: &[lingxi_kernel::model_exchange::ExchangeItem]) -> String {
    use lingxi_kernel::model_exchange::ExchangeItem;
    history
        .iter()
        .filter_map(|item| match item {
            ExchangeItem::ToolResult {
                outcome: ToolOutcome::Success { result },
                ..
            } => Some(
                result
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        lingxi_protocol::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
async fn prepare_five(server: &Server) -> Result<(), String> {
    use lingxi_service::toolgateway::{
        CallerSurface, InvocationPermissionContext, InvocationRequest, PolicyVerdict,
    };
    let gateway = server
        .state
        .tool_gateway()
        .ok_or("production gateway absent")?;
    let ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("independent-default"),
        run_id: RunId::new("preparation-only-not-executed"),
        attempt: lingxi_protocol::AttemptId::new("preparation-only-not-executed#a1"),
        generation: 1,
    };
    for (name, args) in [
        (
            "run_tools",
            json!({"code":"return 'prepare only';","description":"独立准备检查，不执行"}),
        ),
        (
            "run_code",
            json!({"action":"status","language":"python3","code":""}),
        ),
        (
            "ast_grep",
            json!({"language":"typescript","pattern":"console.log($A)","files":["structure.ts"]}),
        ),
        ("lsp", json!({"action":"status"})),
        (
            "security_scan",
            json!({"mode":"path","path":"security-fixture.ts"}),
        ),
    ] {
        let target = gateway
            .registry()
            .resolve(&ToolTargetRef::ByName { name: name.into() })
            .map_err(|e| format!("missing actual default registration {name}: {e:?}"))?;
        let prepared = gateway
            .prepare(InvocationRequest::from_trusted_entry(
                &ctx,
                CallerSurface::UserRun,
                "lingxi",
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                ToolTargetRef::ByTargetId {
                    target_id: target.clone(),
                },
                None,
                args,
                ToolCallId::new(format!("prepare-only-{name}")),
            ))
            .map_err(|e| format!("default prepare failed {name}: {e}"))?;
        check!(
            matches!(prepared.policy, PolicyVerdict::Allowed),
            "unexpected default prepare policy {name}: {:?}",
            prepared.policy
        );
        println!(
            "INDEPENDENT_DEFAULT_TARGET_PREPARED {name} {}",
            target.as_str()
        );
    }
    Ok(())
}
async fn exercise(server: &Server, stub: &Stub, workspace: &Path) -> Result<(), String> {
    prepare_five(server).await?;
    let (_,seed)=run_scene(server,stub,"INDEPENDENT_SCENE_SEED",vec![describe("run_code"),call("run_code",json!({"action":"run","language":"python3","code":"x=40\nprint('INDEPENDENT_STATE_SEEDED')"}))]).await?;
    check!(
        history_text(&seed).contains("INDEPENDENT_STATE_SEEDED"),
        "default model seed did not actually execute"
    );
    let (_,query)=run_scene(server,stub,"INDEPENDENT_SCENE_QUERY",vec![describe("run_code"),call("run_code",json!({"action":"run","language":"python3","code":"from pathlib import Path\np=Path('one-effect.txt')\np.write_text(str(int(p.read_text())+1) if p.exists() else '1')\nprint('INDEPENDENT_STATE_RESULT',x+2)"}))]).await?;
    check!(
        history_text(&query).contains("INDEPENDENT_STATE_RESULT 42"),
        "default model did not retain real Python state"
    );
    check!(
        std::fs::read_to_string(workspace.join("one-effect.txt")).map_err(|e| e.to_string())?
            == "1",
        "actual code file effect not exactly once"
    );
    let (_, scan) = run_scene(
        server,
        stub,
        "INDEPENDENT_SCENE_SECURITY",
        vec![
            describe("security_scan"),
            call(
                "security_scan",
                json!({"mode":"path","path":"security-fixture.ts"}),
            ),
        ],
    )
    .await?;
    let scan_text = history_text(&scan);
    check!(
        scan_text.contains("security-fixture.ts") && scan_text.contains("secret_api_key_literal"),
        "default scan lacks actual file/rule finding"
    );
    check!(
        !scan_text.contains("ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        "scan projection leaked synthetic hit body"
    );
    let (ptc,projected)=run_scene(server,stub,"INDEPENDENT_SCENE_PTC",vec![describe("read"),describe("write"),describe("run_tools"),call("run_tools",json!({"description":"真实读写子调用，只返回所选汇总","code":"await tools.read({path:'ptc-input.txt'});await tools.write({path:'ptc-effect.txt',content:'ONE_FROM_PTC'});return 'INDEPENDENT_PTC_SUMMARY';"}))]).await?;
    check!(
        std::fs::read_to_string(workspace.join("ptc-effect.txt")).map_err(|e| e.to_string())?
            == "ONE_FROM_PTC",
        "PTC child did not actually write"
    );
    check!(
        history_text(&projected).contains("INDEPENDENT_PTC_SUMMARY"),
        "PTC selected summary missing"
    );
    check!(
        !history_text(&projected).contains("PTC_PRIVATE_CHILD_DO_NOT_SEND"),
        "PTC child text entered model history"
    );
    let journal = server
        .state
        .storage()
        .load_invocation_journal(&RunId::new(ptc))
        .await
        .map_err(|e| e.to_string())?;
    let children: Vec<_> = journal
        .iter()
        .filter(|row| row.journal_id.contains(":ptc:"))
        .collect();
    check!(
        children.len() == 2,
        "PTC requires two durable child journals, found {}",
        children.len()
    );
    check!(
        children.iter().all(|row| row.receipt.as_ref().is_some_and(
            |r| r.dispatched && r.outcome == lingxi_kernel::ports::ReceiptOutcome::Succeeded
        )),
        "child receipts are missing/not confirmed"
    );
    let wire = serde_json::to_string(&stub.requests()).map_err(|e| e.to_string())?;
    check!(
        !wire.contains("PTC_PRIVATE_CHILD_DO_NOT_SEND"),
        "PTC raw child leaked into actual HTTP model request"
    );
    std::fs::create_dir_all(server.home.join("independent-evidence")).map_err(|e| e.to_string())?;
    std::fs::write(
        server.home.join("independent-evidence/model-requests.json"),
        serde_json::to_vec_pretty(&stub.requests()).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!("INDEPENDENT_DEFAULT_MODEL_EXECUTED run_code security_scan run_tools; real storage/child receipts/model projection verified");
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn default_composition_registry_preparation_and_real_model_tools() {
    let root = unique_dir("default-composition");
    let home = root.join("data");
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let home = home.canonicalize().unwrap();
    std::fs::write(
        workspace.join("structure.ts"),
        "console.log('real syntax');\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("security-fixture.ts"),
        "const token='ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("ptc-input.txt"),
        "PTC_PRIVATE_CHILD_DO_NOT_SEND",
    )
    .unwrap();
    let stub = Stub::start().await;
    let server = default_server(&home, &workspace, stub.addr).await;
    server
        .state
        .session_admin()
        .create_session(
            "independent-default",
            "lingxi",
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
            "独立默认入口验收",
            Some("operate"),
            &[],
            false,
            timestamp(),
        )
        .await
        .unwrap();
    let result = exercise(&server, &stub, &workspace).await;
    // 即使业务断言失败，也先走实际服务退出，不能让持久解释器遗留。
    server.stop().await;
    if let Err(error) = result {
        stub.task.abort();
        panic!("independent default composition FAIL: {error}");
    }
    let reopened = default_server(&home, &workspace, stub.addr).await;
    let history =
        lingxi_service::model_history::ModelHistory::new(reopened.state.storage().clone())
            .load(
                &lingxi_kernel::Principal::LocalUser,
                "lingxi",
                "independent-default",
                "",
            )
            .await;
    let history_result = history.map_err(|e| e.to_string()).and_then(|history| {
        if !history_text(&history).contains("INDEPENDENT_STATE_RESULT 42") {
            return Err("real reopened history lost tool result".into());
        }
        if std::fs::read_to_string(workspace.join("one-effect.txt")).map_err(|e| e.to_string())?
            != "1"
        {
            return Err("history reopen repeated code side effect".into());
        }
        Ok(())
    });
    reopened.stop().await;
    stub.task.abort();
    history_result
        .expect("independent reopen history must preserve actual outcome without execution");
    println!(
        "INDEPENDENT_REAL_STORAGE_REOPEN_NO_REEXECUTION {}",
        root.display()
    );
}
