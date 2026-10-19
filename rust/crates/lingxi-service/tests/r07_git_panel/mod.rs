//! R07-T07 γ：git 面板 22 路由集成测试。
//!
//! 覆盖（对照现役 server/routes/git-environment.ts + server/git/git-command.ts）：
//! status（占位形状/三类变更合计/detached 短哈希/ahead-behind-pushable）、
//! worktree-info/worktrees（主树与链接树）、branches（current/detached/
//! checkedOutElsewhere）、log（limit 钳制/refs 装饰/多父）、log-stats
//! （批量/40-hex 白名单/全零跳过/合并提交 first-parent 口径）、file-diff
//! （已跟踪/未跟踪合成/二进制/非法路径 400）、checkout、create-branch、
//! commit/amend/stage/unstage（含未出生 HEAD 的 rm --cached 臂）、stash
//! 全流（含 ^3 未跟踪恢复/path_dirty/not_in_stash/pop 冲突）、discard、
//! push/pull/fetch 结果码桶（no_remote/no_upstream/nothing_to_push/
//! already_up_to_date/diverged/local_changes/fetch_failed）、worktree-create
//! 矩阵、目录门禁（GET 403/POST 400/不存在 400/studio 挂载根/agentId 显式
//! home）、ai-commit-message（净化/账本/400/500/502/503/chat 回退）、
//! scope 门禁（log-stats=files.read 的现役特例）。
//!
//! 夹具：真 TCP loopback + 真实 ServiceState（注入 workspace_root 为批准
//! 根）；真实 git 二进制对临时仓执行（被移植面的基板，本地无网络）；远程
//! = 本地 bare 仓；模型面 = 确定性 openai-completions TCP 替身（diary 夹
//! 具同构）。

use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

// ── 确定性模型替身（openai-completions SSE，diary 夹具同构）──

#[derive(Clone)]
struct StubConfig {
    content: String,
    fail: bool,
    delay_ms: u64,
}

struct ModelStub {
    requests: Arc<Mutex<Vec<Value>>>,
    config: Arc<Mutex<StubConfig>>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

async fn read_request(socket: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    loop {
        let n = socket.read(&mut block).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&block[..n]);
        if let Some(index) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let headers = String::from_utf8(bytes[..index].to_vec()).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= index + 4 + length {
                return serde_json::from_slice(&bytes[index + 4..index + 4 + length]).unwrap();
            }
        }
    }
}

// ── 服务夹具 ──

struct Harness {
    root: PathBuf,
    home: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
    stub: Option<ModelStub>,
    clock: Arc<lingxi_service::inject::ManualClock>,
}

impl Harness {
    /// 无模型面（ai-commit-message 的 503 臂与非模型路由）。
    async fn boot(tag: &str) -> Self {
        Self::boot_impl(tag, None).await
    }

    /// 模型面：summarize/chat 槽按需配置（都走同一 TCP 替身）。
    async fn boot_with_models(tag: &str, summarize: bool, chat: bool) -> Self {
        Self::boot_impl(tag, Some((summarize, chat))).await
    }

    async fn boot_impl(tag: &str, models: Option<(bool, bool)>) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r07-git-{tag}-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            serde_json::to_string(&json!({"current_agent_id":"a"})).unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        let root = root.canonicalize().unwrap();
        let home = home.canonicalize().unwrap();
        let workspace = workspace.canonicalize().unwrap();

        // 计划到期由测试时钟推进，不把 HTTP/磁盘耗时误当成业务过期。
        let clock = Arc::new(lingxi_service::inject::ManualClock::new(1_800_000_000_000));
        let mut deps = ServiceDeps {
            clock: clock.clone(),
            workspace_root: Some(workspace.clone()),
            ..ServiceDeps::default()
        };
        let mut stub = None;
        if let Some((with_summarize, with_chat)) = models {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let stub_addr = listener.local_addr().unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stub_config = Arc::new(Mutex::new(StubConfig {
                content: "fix: 默认答复".to_string(),
                fail: false,
                delay_ms: 0,
            }));
            let saved = requests.clone();
            let served_config = stub_config.clone();
            let (stub_stop, mut stop_rx) = tokio::sync::oneshot::channel();
            let stub_task = tokio::spawn(async move {
                loop {
                    let accepted =
                        tokio::select! {_=&mut stop_rx=>break,value=listener.accept()=>value};
                    let (mut socket, _) = accepted.unwrap();
                    let saved = saved.clone();
                    let served_config = served_config.clone();
                    tokio::spawn(async move {
                        let request = read_request(&mut socket).await;
                        saved.lock().unwrap().push(request.clone());
                        let config = served_config.lock().unwrap().clone();
                        if config.delay_ms > 0 {
                            let mut probe = [0u8; 1];
                            tokio::select! {
                                _=tokio::time::sleep(std::time::Duration::from_millis(config.delay_ms))=>{},
                                _=socket.read(&mut probe)=>return,
                            }
                        }
                        if config.fail {
                            let wire = "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
                            socket.write_all(wire.as_bytes()).await.unwrap();
                            return;
                        }
                        let frame = json!({"id":"git","choices":[{"index":0,"delta":{"role":"assistant","content":config.content},"finish_reason":"stop"}]});
                        let sse = format!(
                            "data: {frame}\n\ndata: {}\n\ndata: [DONE]\n\n",
                            json!({"id":"git","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":7}})
                        );
                        let wire = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}", sse.len());
                        socket.write_all(wire.as_bytes()).await.unwrap();
                    });
                }
            });
            let mut model_map = json!({});
            if with_summarize {
                model_map["summarize"] = json!({"provider":"git-stub","model":"summarize-model","compat":{"contextWindow":200000}});
            }
            if with_chat {
                model_map["chat"] = json!({"provider":"git-stub","model":"chat-model","capabilities":{"tools":true},"compat":{"contextWindow":200000}});
            }
            let plane_json = json!({
                "providers": {"git-stub": {"protocol":"openai-completions","endpoint":format!("http://{stub_addr}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-git-key"}}},
                "models": model_map,
            });
            let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(
                &plane_json.to_string(),
            )
            .unwrap();
            let credential = Arc::new(
                lingxi_service::credentials::CredentialService::bootstrap(
                    &plane,
                    &root.join("credentials"),
                    clock.clone(),
                )
                .unwrap(),
            );
            deps.model_gateway = Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            ));
            deps.credential_service = Some(credential);
            deps.network_plane =
                Some(lingxi_adapters::models::network::NetworkPlane::direct_isolated());
            stub = Some(ModelStub {
                requests,
                config: stub_config,
                stop: stub_stop,
                task: stub_task,
            });
        }

        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            deps,
        )
        .await
        .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel::<()>();
        let (ready, address) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let server = tokio::spawn(async move {
            lingxi_service::run(
                serving,
                async {
                    let _ = rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
            .unwrap();
        });
        let base = format!("http://{}", address.await.unwrap());
        Self {
            root,
            home,
            workspace,
            state,
            base,
            stop,
            server,
            stub,
            clock,
        }
    }

    fn set_stub(&self, content: &str, fail: bool) {
        let stub = self.stub.as_ref().expect("model stub");
        *stub.config.lock().unwrap() = StubConfig {
            content: content.to_string(),
            fail,
            delay_ms: 0,
        };
    }

    fn stub_requests(&self) -> Vec<Value> {
        self.stub
            .as_ref()
            .expect("model stub")
            .requests
            .lock()
            .unwrap()
            .clone()
    }

    async fn http(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        self.http_as(&self.state.auth().local_token(), method, path, body)
            .await
    }

    async fn http_as(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn http_q_as(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> (u16, Value) {
        let mut url = format!("{}{path}", self.base);
        if !query.is_empty() {
            let encoded: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
                .collect();
            url = format!("{url}?{}", encoded.join("&"));
        }
        let response = reqwest::Client::new()
            .request(method, url)
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// GET ?dir=<dir>（本机 token）。
    async fn get_dir(&self, route: &str, dir: &Path) -> (u16, Value) {
        let dir = dir.to_string_lossy().into_owned();
        let token = self.state.auth().local_token();
        self.http_q_as(&token, reqwest::Method::GET, route, &[("dir", &dir)])
            .await
    }

    /// POST {...body}（本机 token）。
    async fn post(&self, route: &str, body: Value) -> (u16, Value) {
        self.http(reqwest::Method::POST, route, Some(body)).await
    }

    async fn shutdown(self) {
        let _ = self.stop.send(());
        let _ = self.server.await;
        if let Some(stub) = self.stub {
            let _ = stub.stop.send(());
            let _ = stub.task.await;
        }
        self.state.storage().close().await.unwrap();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// application/x-www-form-urlencoded 的最小编码。
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ── 真实 git 夹具（被移植面的基板；本地临时仓，无网络）──

/// git 配置隔离：屏蔽全局/系统 gitconfig（本机 tag.gpgsign=true 之类
/// 的机器差异不得漏进测试；候选服务与 helper 同进程继承进程 env，
/// 值恒定幂等）。
fn isolate_git_config() {
    std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
    std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
}

fn git(dir: &Path, args: &[&str]) -> String {
    isolate_git_config();
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} @ {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn write_file(dir: &Path, rel: &str, content: &str) {
    let abs = dir.join(rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(abs, content).unwrap();
}

/// 标准仓：main 分支 + 一个提交（a.md 两行）。
fn make_repo(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main", "."]);
    git(path, &["config", "user.name", "Lingxi Test"]);
    git(path, &["config", "user.email", "test@lingxi.local"]);
    write_file(path, "a.md", "line1\nline2\n");
    git(path, &["add", "-A"]);
    git(path, &["commit", "-q", "-m", "init"]);
}

fn make_bare(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "--bare", "-b", "main", "."]);
}

fn clone_repo(bare: &Path, dest: &Path) {
    git(
        Path::new("/"),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            dest.to_str().unwrap(),
        ],
    );
    git(dest, &["config", "user.name", "Other"]);
    git(dest, &["config", "user.email", "other@lingxi.local"]);
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

fn commit_all(repo: &Path, message: &str) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
    head(repo)
}

fn dir_body(dir: &Path) -> Value {
    json!({"dir": dir.to_string_lossy()})
}

// ── status ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_reports_totals_states_and_placeholders() {
    let h = Harness::boot("status").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);

    // 未暂存修改（+3 -1）+ 已暂存新增（+1）+ 未跟踪（+2，按行计数）。
    write_file(&repo, "a.md", "line1\nline2x\nline3\nline4\n");
    write_file(&repo, "b.md", "hello\n");
    git(&repo, &["add", "b.md"]);
    write_file(&repo, "c.md", "x\ny\n");

    let (code, body) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));
    assert_eq!(body["currentBranch"], json!("main"));
    assert_eq!(body["detached"], json!(false));
    assert_eq!(body["stagedTotal"], json!({"additions": 1, "deletions": 0}));
    assert_eq!(
        body["unstagedTotal"],
        json!({"additions": 5, "deletions": 1})
    );
    assert_eq!(body["total"], json!({"additions": 6, "deletions": 1}));
    assert_eq!(body["commitable"], json!(true));
    assert_eq!(body["hasRemote"], json!(false));
    assert_eq!(body["hasUpstream"], json!(false));
    assert_eq!(body["ahead"], json!(0));
    assert_eq!(body["behind"], json!(0));
    assert_eq!(body["pushable"], json!(false));
    let files = body["files"].as_array().unwrap();
    assert_eq!(files.len(), 3, "{body}");
    // localeCompare 序：a.md < b.md < c.md
    assert_eq!(
        files[0],
        json!({"path": "a.md", "additions": 3, "deletions": 1, "state": "modified", "staged": false})
    );
    assert_eq!(
        files[1],
        json!({"path": "b.md", "additions": 1, "deletions": 0, "state": "added", "staged": true})
    );
    assert_eq!(
        files[2],
        json!({"path": "c.md", "additions": 2, "deletions": 0, "state": "untracked", "staged": false})
    );

    // 非仓库目录：占位形状（isRepo=false 零值面）。
    let (code, body) = h.get_dir("/lingxi/v1/git/status", &h.workspace).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(false));
    assert_eq!(body["currentBranch"], Value::Null);
    assert_eq!(body["detached"], json!(false));
    assert_eq!(body["files"], json!([]));
    assert_eq!(body["commitable"], json!(false));
    assert_eq!(body["total"], json!({"additions": 0, "deletions": 0}));

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_detached_head_reports_short_hash() {
    let h = Harness::boot("detached").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let full = head(&repo);
    git(&repo, &["checkout", "-q", "--detach", "HEAD"]);

    let (code, body) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["detached"], json!(true));
    assert_eq!(body["currentBranch"], json!(full[..7].to_string()));

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_ahead_behind_pushable_with_bare_remote() {
    let h = Harness::boot("ahead").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let bare = h.root.join("bare.git");
    make_bare(&bare);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);

    // 已建立 upstream 且无领先：pushable=false。
    let (code, body) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["hasRemote"], json!(true));
    assert_eq!(body["hasUpstream"], json!(true));
    assert_eq!(body["ahead"], json!(0));
    assert_eq!(body["behind"], json!(0));
    assert_eq!(body["pushable"], json!(false));

    // 本地领先 1：pushable=true。
    write_file(&repo, "a.md", "line1\nline2\nline3\n");
    commit_all(&repo, "second");
    let (_, body) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(body["ahead"], json!(1), "{body}");
    assert_eq!(body["pushable"], json!(true));

    // push 后回落（push 路由本身的桶在专门测试覆盖）。
    let (code, body) = h.post("/lingxi/v1/git/push", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    let (_, body) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(body["ahead"], json!(0));
    assert_eq!(body["pushable"], json!(false));

    // 分叉：远端与本地各进一步 → ahead=1 behind=1。
    let other = h.root.join("other");
    clone_repo(&bare, &other);
    write_file(&other, "r1.md", "r1\n");
    commit_all(&other, "remote one");
    git(&other, &["push", "-q", "origin", "main"]);
    write_file(&repo, "l1.md", "l1\n");
    commit_all(&repo, "local one");
    // status 不拉取（现役同口径）：behind 相对最近抓取的 upstream 引
    // 用计算，先经 fetch 路由刷新。
    let (code, _) = h.post("/lingxi/v1/git/fetch", dir_body(&repo)).await;
    assert_eq!(code, 200);
    let (_, body) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(body["ahead"], json!(1), "{body}");
    assert_eq!(body["behind"], json!(1), "{body}");
    assert_eq!(body["pushable"], json!(true));

    h.shutdown().await;
}

// ── worktree-info / worktrees / branches ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worktree_info_and_list_main_and_linked() {
    let h = Harness::boot("wtinfo").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let linked = h.root.join("linked");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );

    // 主树视角。
    let (code, body) = h.get_dir("/lingxi/v1/git/worktree-info", &repo).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));
    assert_eq!(body["isMain"], json!(true));
    assert_eq!(body["name"], Value::Null);
    assert_eq!(body["branch"], json!("main"));
    assert_eq!(body["path"], json!(repo.to_string_lossy()));
    assert_eq!(body["mainPath"], json!(repo.to_string_lossy()));

    // 链接树视角（在批准根之外——经 dir 门禁需先挂进 workspace；直接
    // 查询 workspace 内主树即可，链接树信息经主树的 list 验证）。
    let (code, body) = h.get_dir("/lingxi/v1/git/worktrees", &repo).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));
    let worktrees = body["worktrees"].as_array().unwrap();
    assert_eq!(worktrees.len(), 2, "{body}");
    assert_eq!(worktrees[0]["isMain"], json!(true));
    assert_eq!(worktrees[0]["current"], json!(true));
    assert_eq!(worktrees[0]["branch"], json!("main"));
    assert_eq!(
        std::fs::canonicalize(worktrees[0]["path"].as_str().unwrap()).unwrap(),
        repo
    );
    assert_eq!(worktrees[1]["isMain"], json!(false));
    assert_eq!(worktrees[1]["current"], json!(false));
    assert_eq!(worktrees[1]["branch"], json!("linked"));
    assert_eq!(worktrees[1]["detached"], json!(false));
    // 落地根 = <主树父级>/worktrees。
    assert_eq!(
        body["root"],
        json!(h.workspace.join("worktrees").to_string_lossy())
    );
    assert_eq!(body["mainPath"], json!(repo.to_string_lossy()));

    // 非仓库。
    let (code, body) = h
        .get_dir("/lingxi/v1/git/worktree-info", &h.workspace)
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(false));
    assert_eq!(body["isMain"], json!(true));
    assert_eq!(body["path"], Value::Null);
    let (code, body) = h.get_dir("/lingxi/v1/git/worktrees", &h.workspace).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(false));
    assert_eq!(body["worktrees"], json!([]));
    assert_eq!(body["root"], Value::Null);

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn branches_current_detached_and_checked_out_elsewhere() {
    let h = Harness::boot("branches").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let linked = h.root.join("linked");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );

    let (code, body) = h.get_dir("/lingxi/v1/git/branches", &repo).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));
    assert_eq!(body["current"], json!("main"));
    assert_eq!(body["detached"], json!(false));
    assert_eq!(
        body["branches"],
        json!([
            {"name": "linked", "current": false, "checkedOutElsewhere": true},
            {"name": "main", "current": true, "checkedOutElsewhere": false}
        ])
    );

    // detached：current=null、detached=true。
    git(&repo, &["checkout", "-q", "--detach", "HEAD"]);
    let (_, body) = h.get_dir("/lingxi/v1/git/branches", &repo).await;
    assert_eq!(body["detached"], json!(true));
    assert_eq!(body["current"], Value::Null);

    // 非仓库占位。
    let (code, body) = h.get_dir("/lingxi/v1/git/branches", &h.workspace).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        body,
        json!({"isRepo": false, "branches": [], "detached": false, "current": null})
    );

    h.shutdown().await;
}

// ── log / log-stats ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn log_clamps_limit_and_parses_refs() {
    let h = Harness::boot("log").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let first = head(&repo);
    write_file(&repo, "a.md", "line1\nline2\nline3\n");
    let second = commit_all(&repo, "feat: 第二\n\n- 要点一\n- 要点二");
    git(&repo, &["tag", "v1.0"]);

    let (code, body) = h.get_dir("/lingxi/v1/git/log", &repo).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));
    let commits = body["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 2, "{body}");
    assert_eq!(commits[0]["hash"], json!(second));
    assert_eq!(commits[0]["shortHash"], json!(second[..7].to_string()));
    assert_eq!(commits[0]["subject"], json!("feat: 第二"));
    assert_eq!(
        commits[0]["message"],
        json!("feat: 第二\n\n- 要点一\n- 要点二")
    );
    assert_eq!(commits[0]["authorName"], json!("Lingxi Test"));
    assert!(commits[0]["committedAt"].as_i64().unwrap() > 0);
    assert_eq!(commits[0]["parents"], json!([first]));
    let refs = commits[0]["refs"].as_array().unwrap();
    assert!(
        refs.contains(&json!({"kind": "head", "name": "main"})),
        "{refs:?}"
    );
    assert!(
        refs.contains(&json!({"kind": "tag", "name": "v1.0"})),
        "{refs:?}"
    );
    assert_eq!(commits[1]["hash"], json!(first));
    assert_eq!(commits[1]["parents"], json!([]));

    // limit 钳制：1 下界（0/负数→1），NaN → 默认 300，上限 1000。
    for (limit, expected) in [("1", 1), ("0", 1), ("-5", 1), ("abc", 2), ("9999", 2)] {
        let dir = repo.to_string_lossy().into_owned();
        let token = h.state.auth().local_token();
        let (code, body) = h
            .http_q_as(
                &token,
                reqwest::Method::GET,
                "/lingxi/v1/git/log",
                &[("dir", &dir), ("limit", limit)],
            )
            .await;
        assert_eq!(code, 200, "{body}");
        assert_eq!(
            body["commits"].as_array().unwrap().len(),
            expected,
            "limit={limit}"
        );
    }

    // 非仓库。
    let (code, body) = h.get_dir("/lingxi/v1/git/log", &h.workspace).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"isRepo": false, "commits": []}));

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn log_stats_batch_whitelist_and_merge_first_parent() {
    let h = Harness::boot("logstats").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let h1 = head(&repo);
    // c2：a.md +1-1，b.md +1-0 → {2,1,2}
    write_file(&repo, "a.md", "line1x\nline2\n");
    write_file(&repo, "b.md", "b1\n");
    let h2 = commit_all(&repo, "second");
    // c3：空提交（全零统计 → 不进结果，现役跳过口径）
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "empty"]);
    let h3 = head(&repo);
    // 合并提交：--diff-merges=first-parent → 相对主线侧的增量（m.md +1）
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    write_file(&repo, "m.md", "m1\n");
    commit_all(&repo, "feature work");
    git(&repo, &["checkout", "-q", "main"]);
    write_file(&repo, "n.md", "n1\n");
    commit_all(&repo, "main work");
    git(
        &repo,
        &["merge", "--no-ff", "-q", "-m", "merge feature", "feature"],
    );
    let hm = head(&repo);

    let (code, body) = h
        .post(
            "/lingxi/v1/git/log-stats",
            json!({"dir": repo.to_string_lossy(), "hashes": [h2, h1, h3, hm, "not-a-hash", "1234", h2]}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));
    let stats = body["stats"].as_object().unwrap();
    assert_eq!(stats.len(), 3, "{body}");
    assert_eq!(
        stats[&h1],
        json!({"additions": 2, "deletions": 0, "changedFiles": 1})
    );
    assert_eq!(
        stats[&h2],
        json!({"additions": 2, "deletions": 1, "changedFiles": 2})
    );
    assert_eq!(
        stats[&hm],
        json!({"additions": 1, "deletions": 0, "changedFiles": 1})
    );

    // 非仓库。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/log-stats",
            json!({"dir": h.workspace.to_string_lossy(), "hashes": [h1]}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"isRepo": false, "stats": {}}));

    h.shutdown().await;
}

// ── file-diff ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_diff_tracked_untracked_binary_and_invalid() {
    let h = Harness::boot("filediff").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);

    // 干净已跟踪文件：空 patch、非二进制。
    let (code, body) = h
        .http_q_as(
            &h.state.auth().local_token(),
            reqwest::Method::GET,
            "/lingxi/v1/git/file-diff",
            &[("dir", &repo.to_string_lossy()), ("file", "a.md")],
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"path": "a.md", "patch": "", "binary": false}));

    // 已跟踪修改。
    write_file(&repo, "a.md", "line1\nline2x\nline3\n");
    let (code, body) = h
        .http_q_as(
            &h.state.auth().local_token(),
            reqwest::Method::GET,
            "/lingxi/v1/git/file-diff",
            &[("dir", &repo.to_string_lossy()), ("file", "a.md")],
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let patch = body["patch"].as_str().unwrap();
    assert!(patch.contains("diff --git a/a.md b/a.md"), "{patch}");
    assert!(patch.contains("-line2"), "{patch}");
    assert!(patch.contains("+line2x"), "{patch}");
    assert_eq!(body["binary"], json!(false));

    // 未跟踪文本：合成全新增 patch。
    write_file(&repo, "c.md", "x\ny\n");
    let (code, body) = h
        .http_q_as(
            &h.state.auth().local_token(),
            reqwest::Method::GET,
            "/lingxi/v1/git/file-diff",
            &[("dir", &repo.to_string_lossy()), ("file", "c.md")],
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let patch = body["patch"].as_str().unwrap();
    assert!(patch.contains("new file mode 100644"), "{patch}");
    assert!(patch.contains("@@ -0,0 +1,2 @@"), "{patch}");
    assert!(patch.contains("+x\n+y"), "{patch}");
    assert_eq!(body["binary"], json!(false));

    // 未跟踪二进制（含 NUL）：binary=true、patch 空。
    std::fs::write(repo.join("bin.dat"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    let (code, body) = h
        .http_q_as(
            &h.state.auth().local_token(),
            reqwest::Method::GET,
            "/lingxi/v1/git/file-diff",
            &[("dir", &repo.to_string_lossy()), ("file", "bin.dat")],
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        body,
        json!({"path": "bin.dat", "patch": "", "binary": true})
    );

    // 非法路径：.. 穿越 / 绝对路径 / 缺省空串 → 400。
    for bad in ["../escape", "/etc/passwd"] {
        let (code, body) = h
            .http_q_as(
                &h.state.auth().local_token(),
                reqwest::Method::GET,
                "/lingxi/v1/git/file-diff",
                &[("dir", &repo.to_string_lossy()), ("file", bad)],
            )
            .await;
        assert_eq!(code, 400, "{bad}: {body}");
        assert_eq!(body["error"], json!("invalid file path"));
    }
    let (code, body) = h.get_dir("/lingxi/v1/git/file-diff", &repo).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["error"], json!("invalid file path"));

    h.shutdown().await;
}

// ── checkout / create-branch ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checkout_matrix() {
    let h = Harness::boot("checkout").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    git(&repo, &["branch", "feature"]);

    // 缺 branch 字段 → 400 branch required。
    let (code, body) = h.post("/lingxi/v1/git/checkout", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body, json!({"error": "branch required"}));

    // 非法名（前导 -，防 option 注入）→ 500 invalid branch name。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/checkout",
            json!({"dir": repo.to_string_lossy(), "branch": "-x"}),
        )
        .await;
    assert_eq!(code, 500, "{body}");
    assert_eq!(body, json!({"error": "invalid branch name"}));

    // 不存在 → 500 branch not found。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/checkout",
            json!({"dir": repo.to_string_lossy(), "branch": "nope"}),
        )
        .await;
    assert_eq!(code, 500, "{body}");
    assert_eq!(body, json!({"error": "branch not found"}));

    // 成功：200 {ok, branch}，当前分支随之切换。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/checkout",
            json!({"dir": repo.to_string_lossy(), "branch": "feature"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "branch": "feature"}));
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(status["currentBranch"], json!("feature"));

    // 非仓库 → 400 not a git repo。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/checkout",
            json!({"dir": h.workspace.to_string_lossy(), "branch": "main"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body, json!({"error": "not a git repo"}));

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_branch_matrix() {
    let h = Harness::boot("mkbranch").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);

    // 成功：checkout -b，当前分支切换。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/create-branch",
            json!({"dir": repo.to_string_lossy(), "name": "feat"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "branch": "feat"}));
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(status["currentBranch"], json!("feat"));

    // 重名 → already_exists（message 缺席 → error 落 code）。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/create-branch",
            json!({"dir": repo.to_string_lossy(), "name": "feat"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("already_exists"));
    assert_eq!(body["branch"], json!("feat"));
    assert_eq!(body["error"], json!("already_exists"));

    // 非法名。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/create-branch",
            json!({"dir": repo.to_string_lossy(), "name": "has space"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("invalid_name"));
    assert_eq!(body["error"], json!("invalid_name"));

    // base 生效 / 非法 base。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/create-branch",
            json!({"dir": repo.to_string_lossy(), "name": "based", "base": "main"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(status["currentBranch"], json!("based"));
    let (code, body) = h
        .post(
            "/lingxi/v1/git/create-branch",
            json!({"dir": repo.to_string_lossy(), "name": "badbase", "base": "nope"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("invalid_base"));
    assert_eq!(body["branch"], json!("badbase"));
    assert_eq!(body["error"], json!("invalid_base"));

    // 缺 name → 400；非仓库 → 400。
    let (code, body) = h
        .post("/lingxi/v1/git/create-branch", dir_body(&repo))
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body, json!({"error": "name required"}));
    let (code, body) = h
        .post(
            "/lingxi/v1/git/create-branch",
            json!({"dir": h.workspace.to_string_lossy(), "name": "x"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body, json!({"error": "not a git repo"}));

    h.shutdown().await;
}

// ── commit / amend / stage / unstage ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commit_flow() {
    let h = Harness::boot("commit").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);

    // 空信息 → nothing_to_commit + message（plain 信封：无 error 字段）。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/commit",
            json!({"dir": repo.to_string_lossy(), "message": "   "}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("nothing_to_commit"));
    assert_eq!(body["message"], json!("empty commit message"));

    // 干净树 → nothing_to_commit（无 message）。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/commit",
            json!({"dir": repo.to_string_lossy(), "message": "x"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("nothing_to_commit"));

    // 仅未暂存 + includeUnstaged:false → nothing_staged。
    write_file(&repo, "a.md", "line1\nline2\nline3\n");
    let (code, body) = h
        .post(
            "/lingxi/v1/git/commit",
            json!({"dir": repo.to_string_lossy(), "message": "x", "includeUnstaged": false}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("nothing_staged"));

    // 默认 includeUnstaged=true：add -A 后提交；信息 trim；返回 head。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/commit",
            json!({"dir": repo.to_string_lossy(), "message": "  提交更改  "}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let new_head = head(&repo);
    assert_eq!(body, json!({"ok": true, "head": new_head}));
    let (_, log) = h.get_dir("/lingxi/v1/git/log", &repo).await;
    assert_eq!(log["commits"][0]["subject"], json!("提交更改"));

    // 先暂存 + includeUnstaged:false → 提交暂存内容。
    write_file(&repo, "b.md", "b1\n");
    git(&repo, &["add", "b.md"]);
    let (code, body) = h
        .post(
            "/lingxi/v1/git/commit",
            json!({"dir": repo.to_string_lossy(), "message": "手动暂存", "includeUnstaged": false}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    let (_, log) = h.get_dir("/lingxi/v1/git/log", &repo).await;
    assert_eq!(log["commits"][0]["subject"], json!("手动暂存"));

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn amend_flow() {
    let h = Harness::boot("amend").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);

    // 未出生仓库：nothing_to_commit + no commit to amend。
    let repo2 = h.workspace.join("repo2");
    std::fs::create_dir_all(&repo2).unwrap();
    git(&repo2, &["init", "-q", "-b", "main", "."]);
    git(&repo2, &["config", "user.name", "Lingxi Test"]);
    git(&repo2, &["config", "user.email", "test@lingxi.local"]);
    let (code, body) = h.post("/lingxi/v1/git/amend", dir_body(&repo2)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("nothing_to_commit"));
    assert_eq!(body["message"], json!("no commit to amend"));

    // 改信息：head 变化、subject 更新。
    let before = head(&repo);
    let (code, body) = h
        .post(
            "/lingxi/v1/git/amend",
            json!({"dir": repo.to_string_lossy(), "message": "init v2"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_ne!(body["head"], json!(before));
    let (_, log) = h.get_dir("/lingxi/v1/git/log", &repo).await;
    assert_eq!(log["commits"][0]["subject"], json!("init v2"));

    // --no-edit 臂（message 缺席）：内容并入、信息不变。
    write_file(&repo, "a.md", "line1\nline2\nline3\n");
    git(&repo, &["add", "a.md"]);
    let (code, body) = h.post("/lingxi/v1/git/amend", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["head"], json!(head(&repo)));
    let (_, log) = h.get_dir("/lingxi/v1/git/log", &repo).await;
    assert_eq!(log["commits"][0]["subject"], json!("init v2"));

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stage_unstage_flows() {
    let h = Harness::boot("stage").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    write_file(&repo, "a.md", "line1\nline2\nchanged\n");
    write_file(&repo, "b.md", "new\n");

    // 按路径暂存。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/stage",
            json!({"dir": repo.to_string_lossy(), "paths": ["a.md"]}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "paths": ["a.md"]}));
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    let files = status["files"].as_array().unwrap();
    let a = files.iter().find(|f| f["path"] == json!("a.md")).unwrap();
    assert_eq!(a["staged"], json!(true));
    let b = files.iter().find(|f| f["path"] == json!("b.md")).unwrap();
    assert_eq!(b["state"], json!("untracked"));

    // 非法路径 → invalid_path（plain 信封无 error 字段）。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/stage",
            json!({"dir": repo.to_string_lossy(), "paths": ["../x"]}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("invalid_path"));

    // 整仓暂存（paths 缺席 → add -A）。
    let (code, body) = h.post("/lingxi/v1/git/stage", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "paths": []}));
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    let files = status["files"].as_array().unwrap();
    assert!(files.iter().all(|f| f["staged"] == json!(true)), "{status}");

    // 按路径取消暂存（有 HEAD：restore --staged）。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/unstage",
            json!({"dir": repo.to_string_lossy(), "paths": ["a.md"]}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "paths": ["a.md"]}));
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    let files = status["files"].as_array().unwrap();
    let a = files.iter().find(|f| f["path"] == json!("a.md")).unwrap();
    assert_eq!(a["staged"], json!(false));

    // 未出生 HEAD：rm --cached 臂（restore --staged 在无 HEAD 时不可用）。
    let repo2 = h.workspace.join("repo2");
    std::fs::create_dir_all(&repo2).unwrap();
    git(&repo2, &["init", "-q", "-b", "main", "."]);
    git(&repo2, &["config", "user.name", "Lingxi Test"]);
    git(&repo2, &["config", "user.email", "test@lingxi.local"]);
    write_file(&repo2, "x.md", "x\n");
    let (code, _) = h.post("/lingxi/v1/git/stage", dir_body(&repo2)).await;
    assert_eq!(code, 200);
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo2).await;
    assert_eq!(status["files"][0]["staged"], json!(true));
    assert_eq!(status["files"][0]["state"], json!("added"));
    let (code, body) = h.post("/lingxi/v1/git/unstage", dir_body(&repo2)).await;
    assert_eq!(code, 200, "{body}");
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo2).await;
    assert_eq!(status["files"][0]["staged"], json!(false));
    assert_eq!(status["files"][0]["state"], json!("untracked"));

    h.shutdown().await;
}

// ── stash 全流 ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stash_full_flow() {
    let h = Harness::boot("stash").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);

    // 储藏（含未跟踪）→ 工作区干净。
    write_file(&repo, "a.md", "line1\nline2\nstashed\n");
    write_file(&repo, "c.md", "untracked\n");
    let (code, body) = h
        .post(
            "/lingxi/v1/git/stash",
            json!({"dir": repo.to_string_lossy(), "message": "暂存一下"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true}));
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(status["files"], json!([]), "{status}");

    // stashes 列表：tracked/untracked 分列（^3 臂）。
    let (code, body) = h.get_dir("/lingxi/v1/git/stashes", &repo).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));
    let stashes = body["stashes"].as_array().unwrap();
    assert_eq!(stashes.len(), 1, "{body}");
    assert_eq!(stashes[0]["ref"], json!("stash@{0}"));
    assert!(stashes[0]["message"].as_str().unwrap().contains("暂存一下"));
    assert_eq!(stashes[0]["tracked"], json!(["a.md"]));
    assert_eq!(stashes[0]["untracked"], json!(["c.md"]));

    // 单路径恢复（已跟踪）：stash 保留。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/unstash",
            json!({"dir": repo.to_string_lossy(), "path": "a.md"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        body,
        json!({"ok": true, "path": "a.md", "stash": "stash@{0}"})
    );
    assert!(!repo.join("c.md").exists());

    // 路径脏 → path_dirty。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/unstash",
            json!({"dir": repo.to_string_lossy(), "path": "a.md"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("path_dirty"));
    assert_eq!(body["path"], json!("a.md"));
    assert_eq!(body["error"], json!("path_dirty"));

    // 单路径恢复（未跟踪，^3 源）。
    git(&repo, &["checkout", "-q", "--", "a.md"]);
    let (code, body) = h
        .post(
            "/lingxi/v1/git/unstash",
            json!({"dir": repo.to_string_lossy(), "path": "c.md"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        std::fs::read_to_string(repo.join("c.md")).unwrap(),
        "untracked\n"
    );

    // 不在任何储藏里 → not_in_stash。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/unstash",
            json!({"dir": repo.to_string_lossy(), "path": "nope.md"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("not_in_stash"));
    assert_eq!(body["path"], json!("nope.md"));
    assert_eq!(body["error"], json!("not_in_stash"));

    // pop：恢复并出栈。先移走单路径恢复过的 c.md（restore 不出栈，
    // stash 仍含 c.md；不清理则 pop 撞 "already exists"——现役同语义）。
    std::fs::remove_file(repo.join("c.md")).unwrap();
    let (code, body) = h.post("/lingxi/v1/git/unstash", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true}));
    let (_, body) = h.get_dir("/lingxi/v1/git/stashes", &repo).await;
    assert_eq!(body["stashes"], json!([]));

    // 空栈 pop → no_stash。
    let (code, body) = h.post("/lingxi/v1/git/unstash", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("no_stash"));
    assert_eq!(body["error"], json!("no_stash"));

    // 干净树 stash → nothing_to_stash。
    git(&repo, &["checkout", "-q", "--", "a.md"]);
    std::fs::remove_file(repo.join("c.md")).unwrap();
    let (code, body) = h.post("/lingxi/v1/git/stash", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("nothing_to_stash"));
    assert_eq!(body["error"], json!("nothing_to_stash"));

    // 按路径储藏：只动 a.md。
    write_file(&repo, "b.md", "b1\n");
    git(&repo, &["add", "b.md"]);
    git(&repo, &["commit", "-q", "-m", "add b"]);
    write_file(&repo, "a.md", "line1\nline2\npath-stash\n");
    write_file(&repo, "b.md", "b1\nb2\n");
    let (code, body) = h
        .post(
            "/lingxi/v1/git/stash",
            json!({"dir": repo.to_string_lossy(), "paths": ["a.md"]}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "paths": ["a.md"]}));
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    let files = status["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "{status}");
    assert_eq!(files[0]["path"], json!("b.md"));

    // pop 冲突：主线已改写同文件 → unstash_conflict，stash 保留。
    write_file(&repo, "a.md", "conflicted\n");
    git(&repo, &["add", "a.md"]);
    git(&repo, &["commit", "-q", "-m", "conflicting"]);
    let (code, body) = h.post("/lingxi/v1/git/unstash", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("unstash_conflict"));
    assert!(!body["error"].as_str().unwrap().is_empty(), "{body}");
    let (_, body) = h.get_dir("/lingxi/v1/git/stashes", &repo).await;
    assert_eq!(body["stashes"].as_array().unwrap().len(), 1);

    h.shutdown().await;
}

// ── discard ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discard_flows() {
    let h = Harness::boot("discard").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    write_file(&repo, "a.md", "line1\nline2\nchanged\n");
    write_file(&repo, "b.md", "new\n");
    git(&repo, &["add", "b.md"]);
    write_file(&repo, "c.md", "untracked\n");

    // 按路径丢弃：只还原 a.md。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/discard",
            json!({"dir": repo.to_string_lossy(), "paths": ["a.md"]}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "paths": ["a.md"]}));
    assert_eq!(
        std::fs::read_to_string(repo.join("a.md")).unwrap(),
        "line1\nline2\n"
    );
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(status["files"].as_array().unwrap().len(), 2, "{status}");

    // 整仓丢弃：restore（含已暂存新增连工作区文件一起移除，真实 git
    // 语义已取证）+ clean -fd 清未跟踪。
    let (code, body) = h.post("/lingxi/v1/git/discard", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "paths": []}));
    assert!(!repo.join("b.md").exists());
    assert!(!repo.join("c.md").exists());
    let (_, status) = h.get_dir("/lingxi/v1/git/status", &repo).await;
    assert_eq!(status["files"], json!([]), "{status}");

    // 干净 → nothing_to_discard。
    let (code, body) = h.post("/lingxi/v1/git/discard", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("nothing_to_discard"));
    assert_eq!(body["paths"], json!([]));
    assert_eq!(body["error"], json!("nothing_to_discard"));

    // 非法路径。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/discard",
            json!({"dir": repo.to_string_lossy(), "paths": ["-flag"]}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("invalid_path"));
    assert_eq!(body["error"], json!("invalid_path"));

    h.shutdown().await;
}

// ── push / pull / fetch 结果码桶（远程 = 本地 bare 仓）──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn push_pull_fetch_buckets() {
    let h = Harness::boot("sync").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);

    // 无远程：三桶全 no_remote。
    for route in ["push", "pull", "fetch"] {
        let (code, body) = h
            .post(&format!("/lingxi/v1/git/{route}"), dir_body(&repo))
            .await;
        assert_eq!(code, 400, "{route}: {body}");
        assert_eq!(body["ok"], json!(false), "{route}");
        assert_eq!(body["code"], json!("no_remote"), "{route}: {body}");
    }

    // 有远程无 upstream：pull → no_upstream；push → -u 建立跟踪。
    let bare = h.root.join("bare.git");
    make_bare(&bare);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    let (code, body) = h.post("/lingxi/v1/git/pull", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["code"], json!("no_upstream"));
    let (code, body) = h.post("/lingxi/v1/git/push", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true}));

    // 再推：nothing_to_push。
    let (code, body) = h.post("/lingxi/v1/git/push", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["code"], json!("nothing_to_push"));

    // 远端进一步 → pull {ok, pulled:1}；再拉 → already_up_to_date。
    let other = h.root.join("other");
    clone_repo(&bare, &other);
    write_file(&other, "r1.md", "r1\n");
    commit_all(&other, "remote one");
    git(&other, &["push", "-q", "origin", "main"]);
    let (code, body) = h.post("/lingxi/v1/git/pull", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "pulled": 1}));
    assert!(repo.join("r1.md").exists());
    let (code, body) = h.post("/lingxi/v1/git/pull", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["code"], json!("already_up_to_date"));

    // 分叉 → diverged。
    write_file(&repo, "l1.md", "l1\n");
    commit_all(&repo, "local one");
    write_file(&other, "r2.md", "r2\n");
    commit_all(&other, "remote two");
    git(&other, &["push", "-q", "origin", "main"]);
    let (code, body) = h.post("/lingxi/v1/git/pull", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["code"], json!("diverged"));

    // 本地脏文件与待并入文件重叠 → local_changes（结构性预判）。
    git(&repo, &["reset", "-q", "--hard", "origin/main"]);
    write_file(&other, "a.md", "remote edit\n");
    commit_all(&other, "remote edits a");
    git(&other, &["push", "-q", "origin", "main"]);
    write_file(&repo, "a.md", "local dirty\n");
    let (code, body) = h.post("/lingxi/v1/git/pull", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["code"], json!("local_changes"));

    // 清脏后 fetch/pull 成功。
    git(&repo, &["checkout", "-q", "--", "a.md"]);
    let (code, body) = h.post("/lingxi/v1/git/fetch", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true}));
    let (code, body) = h.post("/lingxi/v1/git/pull", dir_body(&repo)).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"ok": true, "pulled": 1}));

    // 远程不可达 → fetch_failed（pull 先 fetch，同桶）。
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "origin",
            h.root.join("gone.git").to_str().unwrap(),
        ],
    );
    let (code, body) = h.post("/lingxi/v1/git/fetch", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("fetch_failed"));
    assert!(!body["message"].as_str().unwrap().is_empty(), "{body}");
    let (code, body) = h.post("/lingxi/v1/git/pull", dir_body(&repo)).await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["code"], json!("fetch_failed"));

    h.shutdown().await;
}

// ── worktree-create ──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worktree_create_matrix() {
    let h = Harness::boot("wtcreate").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let root = h.workspace.join("worktrees");

    // 成功：<主树父级>/worktrees/<name> + wt/<name> 分支。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/worktree-create",
            json!({"dir": repo.to_string_lossy(), "name": "wt1"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        body,
        json!({"ok": true, "path": root.join("wt1").to_string_lossy(), "branch": "wt/wt1"})
    );
    assert!(root.join("wt1").is_dir());
    let (_, list) = h.get_dir("/lingxi/v1/git/worktrees", &repo).await;
    assert_eq!(list["worktrees"].as_array().unwrap().len(), 2);

    // 目录已存在 → exists。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/worktree-create",
            json!({"dir": repo.to_string_lossy(), "name": "wt1"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("exists"));
    assert_eq!(body["path"], json!(root.join("wt1").to_string_lossy()));
    assert_eq!(body["branch"], json!("wt/wt1"));
    assert_eq!(body["error"], json!("exists"));

    // 分支已被占用（目录缺席）→ branch_exists。
    git(&repo, &["branch", "wt/wt2"]);
    let (code, body) = h
        .post(
            "/lingxi/v1/git/worktree-create",
            json!({"dir": repo.to_string_lossy(), "name": "wt2"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("branch_exists"));
    assert_eq!(body["path"], json!(root.join("wt2").to_string_lossy()));
    assert_eq!(body["branch"], json!("wt/wt2"));
    assert_eq!(body["error"], json!("branch_exists"));

    // 非法名 / 非法 base / base 生效。
    let (code, body) = h
        .post(
            "/lingxi/v1/git/worktree-create",
            json!({"dir": repo.to_string_lossy(), "name": "bad name"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("invalid_name"));
    assert_eq!(body["error"], json!("invalid_name"));
    let (code, body) = h
        .post(
            "/lingxi/v1/git/worktree-create",
            json!({"dir": repo.to_string_lossy(), "name": "wt3", "base": "nope"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["code"], json!("invalid_base"));
    assert_eq!(body["branch"], json!("wt/wt3"));
    assert_eq!(body["error"], json!("invalid_base"));
    let (code, body) = h
        .post(
            "/lingxi/v1/git/worktree-create",
            json!({"dir": repo.to_string_lossy(), "name": "wt3", "base": "main"}),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["branch"], json!("wt/wt3"));

    // 缺 name → 400。
    let (code, body) = h
        .post("/lingxi/v1/git/worktree-create", dir_body(&repo))
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body, json!({"error": "name required"}));

    h.shutdown().await;
}

// ── 目录门禁（isApprovedDir 三层）──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gate_query_403_body_400_studio_mount_and_agent() {
    let h = Harness::boot("gate").await;
    let outside = h.root.join("outside-repo");
    make_repo(&outside);
    let outside2 = h.root.join("outside-repo-2");
    make_repo(&outside2);

    // GET 未获批 → 403 dir not allowed；POST 未获批 → 400 invalid dir。
    let (code, body) = h.get_dir("/lingxi/v1/git/status", &outside).await;
    assert_eq!(code, 403, "{body}");
    assert_eq!(body, json!({"error": "dir not allowed"}));
    let (code, body) = h
        .post(
            "/lingxi/v1/git/commit",
            json!({"dir": outside.to_string_lossy(), "message": "x"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body, json!({"error": "invalid dir"}));

    // 不存在的目录（GET）→ 400 invalid dir。
    let (code, body) = h
        .get_dir("/lingxi/v1/git/status", &h.root.join("gone"))
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body, json!({"error": "invalid dir"}));

    // studio-mounts.json 的 active local_fs 挂载根获批。
    std::fs::write(
        h.home.join("studio-mounts.json"),
        serde_json::to_string(&json!({
            "schemaVersion": 1,
            "mounts": [{
                "schemaVersion": 1,
                "mountId": "m-git-gate",
                "hostStudioId": "studio-test",
                "sourceKind": "storage",
                "provider": "local_fs",
                "rootLocator": {"path": outside.to_string_lossy()},
                "label": "挂载",
                "presentation": "folder",
                "status": "active",
                "capabilities": ["read", "write", "list"]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let (code, body) = h.get_dir("/lingxi/v1/git/status", &outside).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));

    // agentId 臂：无 agentId → 403；未知 agent → 403；配置了
    // desk.home_folder 的 agent → 200。
    let dir2 = outside2.to_string_lossy().into_owned();
    let token = h.state.auth().local_token();
    let (code, _) = h
        .http_q_as(
            &token,
            reqwest::Method::GET,
            "/lingxi/v1/git/status",
            &[("dir", &dir2)],
        )
        .await;
    assert_eq!(code, 403);
    let (code, _) = h
        .http_q_as(
            &token,
            reqwest::Method::GET,
            "/lingxi/v1/git/status",
            &[("dir", &dir2), ("agentId", "missing")],
        )
        .await;
    assert_eq!(code, 403);
    std::fs::write(
        h.home.join("agents/a/config.yaml"),
        format!(
            "agent:\n  name: a\nmemory:\n  enabled: false\ndesk:\n  home_folder: \"{}\"\n",
            outside2.display()
        ),
    )
    .unwrap();
    let (code, body) = h
        .http_q_as(
            &token,
            reqwest::Method::GET,
            "/lingxi/v1/git/status",
            &[("dir", &dir2), ("agentId", "a")],
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));

    h.shutdown().await;
}

// ── ai-commit-message ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ai_commit_message_happy_path_sanitizes_and_records_usage() {
    let h = Harness::boot_with_models("ai", true, true).await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    write_file(&repo, "a.md", "line1\nline2\nline3\n");
    h.set_stub(
        "```markdown\ncommit: feat(git): 面板提交\n\n\n- 要点一  \n- 要点二\n```",
        false,
    );

    let (code, body) = h
        .post("/lingxi/v1/git/ai-commit-message", dir_body(&repo))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        body,
        json!({"message": "feat(git): 面板提交\n\n- 要点一\n- 要点二"})
    );

    // 替身收到的请求：summarize 槽模型 + 现役系统提示 + 变更上下文。
    let requests = h.stub_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0]["model"], json!("summarize-model"));
    let system = requests[0]["messages"][0]["content"].as_str().unwrap();
    assert!(system.starts_with("你是 git 提交信息生成器"), "{system}");
    let prompt = requests[0]["messages"][1]["content"].as_str().unwrap();
    assert!(prompt.contains("变更文件（共 1 个）"), "{prompt}");
    assert!(prompt.contains("a.md"), "{prompt}");

    // 用量账本：origin=git-commit-message / purpose=auxiliary.summarize。
    let model = h
        .state
        .storage()
        .query_one_text(
            "SELECT model FROM model_call_usage WHERE origin='git-commit-message'",
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(model.as_deref(), Some("summarize-model"));
    let purpose = h
        .state
        .storage()
        .query_one_text(
            "SELECT purpose FROM model_call_usage WHERE origin='git-commit-message'",
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(purpose.as_deref(), Some("auxiliary.summarize"));

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ai_commit_message_failure_envelopes() {
    // 无变更 → 400 no changes（模型面可用，证明是变更检查触发）。
    let h = Harness::boot_with_models("ai-fail", true, false).await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let (code, body) = h
        .post("/lingxi/v1/git/ai-commit-message", dir_body(&repo))
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body, json!({"error": "no changes"}));

    // 模型只回围栏 → 净化后为空 → 502。
    write_file(&repo, "a.md", "changed\n");
    h.set_stub("```\n```", false);
    let (code, body) = h
        .post("/lingxi/v1/git/ai-commit-message", dir_body(&repo))
        .await;
    assert_eq!(code, 502, "{body}");
    assert_eq!(body, json!({"error": "ai produced empty message"}));

    // 供应商 500 → 500（错误文本非空）。
    h.set_stub("x", true);
    let (code, body) = h
        .post("/lingxi/v1/git/ai-commit-message", dir_body(&repo))
        .await;
    assert_eq!(code, 500, "{body}");
    assert!(
        !body["error"].as_str().unwrap_or_default().is_empty(),
        "{body}"
    );
    h.shutdown().await;

    // summarize/chat 双缺 → 500 现役原文。
    let h = Harness::boot_with_models("ai-none", false, false).await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    write_file(&repo, "a.md", "changed\n");
    let (code, body) = h
        .post("/lingxi/v1/git/ai-commit-message", dir_body(&repo))
        .await;
    assert_eq!(code, 500, "{body}");
    assert_eq!(
        body,
        json!({"error": "summarize slot unavailable (no model configured and no chat fallback)"})
    );
    h.shutdown().await;

    // 无模型面（hub 缺失的候选人对等）→ 503 llm unavailable。
    let h = Harness::boot("ai-nogw").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    write_file(&repo, "a.md", "changed\n");
    let (code, body) = h
        .post("/lingxi/v1/git/ai-commit-message", dir_body(&repo))
        .await;
    assert_eq!(code, 503, "{body}");
    assert_eq!(body, json!({"error": "llm unavailable"}));
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ai_commit_message_chat_fallback() {
    // summarize 槽缺席 → chat 回退（现役 auxiliary-slots.ts 口径）。
    let h = Harness::boot_with_models("ai-chat", false, true).await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    write_file(&repo, "a.md", "changed\n");
    h.set_stub("fix: 修复登录", false);

    let (code, body) = h
        .post("/lingxi/v1/git/ai-commit-message", dir_body(&repo))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body, json!({"message": "fix: 修复登录"}));
    let requests = h.stub_requests();
    assert_eq!(requests[0]["model"], json!("chat-model"));
    let model = h
        .state
        .storage()
        .query_one_text(
            "SELECT model FROM model_call_usage WHERE origin='git-commit-message'",
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(model.as_deref(), Some("chat-model"));

    h.shutdown().await;
}

// ── scope 门禁（中间件信封；log-stats = files.read 的现役特例）──

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_scope_gates() {
    let h = Harness::boot("auth").await;
    let repo = h.workspace.join("repo");
    make_repo(&repo);
    let dir = repo.to_string_lossy().into_owned();

    // 无凭证 → 401。
    let response = reqwest::Client::new()
        .get(format!(
            "{}/lingxi/v1/git/status?dir={}",
            h.base,
            urlencode(&dir)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 401);

    // 无 scope 凭证 → 403 insufficient_scope files.read。
    let denied = h
        .state
        .auth()
        .issue_device_credential("user_local", &[], None)
        .unwrap()
        .secret;
    let (code, body) = h
        .http_q_as(
            &denied,
            reqwest::Method::GET,
            "/lingxi/v1/git/status",
            &[("dir", &dir)],
        )
        .await;
    assert_eq!(code, 403, "{body}");
    assert_eq!(body["details"]["reason"], json!("insufficient_scope"));
    assert_eq!(body["details"]["requiredScope"], json!("files.read"));

    // files.read：GET 面放行；log-stats（POST 但 files.read）放行；
    // 其它 POST → 403 files.write。
    let reader = h
        .state
        .auth()
        .issue_device_credential("user_local", &["files.read"], None)
        .unwrap()
        .secret;
    let (code, _) = h
        .http_q_as(
            &reader,
            reqwest::Method::GET,
            "/lingxi/v1/git/status",
            &[("dir", &dir)],
        )
        .await;
    assert_eq!(code, 200);
    let (code, _) = h
        .http_q_as(
            &reader,
            reqwest::Method::GET,
            "/lingxi/v1/git/log",
            &[("dir", &dir)],
        )
        .await;
    assert_eq!(code, 200);
    let (code, body) = h
        .http_as(
            &reader,
            reqwest::Method::POST,
            "/lingxi/v1/git/log-stats",
            Some(json!({"dir": dir, "hashes": []})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["isRepo"], json!(true));
    let (code, body) = h
        .http_as(
            &reader,
            reqwest::Method::POST,
            "/lingxi/v1/git/commit",
            Some(json!({"dir": dir, "message": "x"})),
        )
        .await;
    assert_eq!(code, 403, "{body}");
    assert_eq!(body["details"]["requiredScope"], json!("files.write"));

    // files.write 不覆盖读面。
    let writer = h
        .state
        .auth()
        .issue_device_credential("user_local", &["files.write"], None)
        .unwrap()
        .secret;
    let (code, body) = h
        .http_q_as(
            &writer,
            reqwest::Method::GET,
            "/lingxi/v1/git/status",
            &[("dir", &dir)],
        )
        .await;
    assert_eq!(code, 403, "{body}");
    assert_eq!(body["details"]["requiredScope"], json!("files.read"));

    // 其它动词 → /api/* fallback = LocalOnly（设备凭证 403）。
    let (code, body) = h
        .http_q_as(
            &reader,
            reqwest::Method::DELETE,
            "/lingxi/v1/git/status",
            &[("dir", &dir)],
        )
        .await;
    assert_eq!(code, 403, "{body}");
    assert_eq!(body["details"]["reason"], json!("local_owner_required"));

    h.shutdown().await;
}

/// 使用实际计划和实际 Run 建立活动，外部模型回复由现有 TCP 替身提供。
async fn summary_activity(h: &Harness) -> String {
    h.state
        .session_admin()
        .create_session(
            "summary-source",
            "a",
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
            "摘要来源",
            Some("operate"),
            &[h.workspace.to_string_lossy().into_owned()],
            true,
            h.state.clock().now_unix_ms(),
        )
        .await
        .unwrap();
    let due = h.state.clock().now_unix_ms() + 100;
    let (code,body)=h.http(reqwest::Method::POST,"/lingxi/v1/desk/cron",Some(json!({"action":"add","actorAgentId":"a","type":"at","schedule":due,"prompt":"核对日程并整理工作台","label":"活动摘要检查","executionContext":{"sourceSessionId":"summary-source"}}))).await;
    assert_eq!(code, 200, "{body}");
    h.clock.advance(150);
    h.state.automation().check_due(&h.state).await.unwrap();
    let (code, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/activities", None)
        .await;
    assert_eq!(code, 200, "{body}");
    body["activities"][0]["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn activity_summary_actual_http_uses_summarize_slot_and_records_success_and_failure() {
    let h = Harness::boot_with_models("activity-summary", true, true).await;
    let id = summary_activity(&h).await;
    h.set_stub("核对了日程，并整理了工作台。", false);
    let (code, body) = h
        .post("/lingxi/v1/desk/activities/summarize", json!({"id":id}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["summary"], "核对了日程，并整理了工作台。");
    let request = h.stub_requests().pop().unwrap();
    assert_eq!(request["model"], "summarize-model");
    assert!(request["messages"]
        .to_string()
        .contains("核对日程并整理工作台"));
    let row=h.state.storage().query_one_text("SELECT json_object('outcome',outcome,'purpose',purpose,'usageState',usage_state,'cause',cause_ref) FROM model_call_usage WHERE origin='activity_summary_quick'",vec![]).await.unwrap().unwrap();
    let row: Value = serde_json::from_str(&row).unwrap();
    assert_eq!(row["outcome"], "succeeded");
    assert_eq!(row["purpose"], "auxiliary.summarize");
    assert_eq!(row["usageState"], "reported");
    assert_eq!(row["cause"], format!("activity:{id}"));
    let before_repair = h.stub_requests().len();
    h.set_stub(&"长".repeat(90), false);
    let (code, body) = h
        .post("/lingxi/v1/desk/activities/summarize", json!({"id":id}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["error"], "activity summary length repair exhausted");
    assert_eq!(
        h.stub_requests().len(),
        before_repair + 3,
        "首调与最多两次长度修复均实际调用并记账"
    );
    h.set_stub("", true);
    let (code, body) = h
        .post("/lingxi/v1/desk/activities/summarize", json!({"id":id}))
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(body["error"].is_string());
    let failures=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM model_call_usage WHERE origin='activity_summary_quick' AND outcome='failed'",vec![]).await.unwrap().unwrap();
    assert_eq!(failures, "1");
    assert_eq!(
        h.state
            .runs()
            .quotas()
            .in_use(lingxi_service::quotas::QuotaResource::Model),
        0
    );
    h.shutdown().await;
}

#[tokio::test]
async fn activity_summary_revocation_cancels_call_records_unknown_usage_and_releases_permit() {
    let h = Harness::boot_with_models("activity-summary-cancel", true, true).await;
    let id = summary_activity(&h).await;
    h.stub.as_ref().unwrap().config.lock().unwrap().delay_ms = 5_000;
    let prior = h.stub_requests().len();
    let client = reqwest::Client::new();
    let request = client
        .post(format!("{}/lingxi/v1/desk/activities/summarize", h.base))
        .bearer_auth(h.state.auth().local_token())
        .json(&json!({"id":id}));
    let pending = tokio::spawn(async move { request.send().await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while h.stub_requests().len() == prior {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (code, body) = h
        .http(
            reqwest::Method::PATCH,
            "/lingxi/v1/sessions/summary-source/authorized-folders",
            Some(json!({"action":"set","folders":[]})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let response = tokio::time::timeout(std::time::Duration::from_secs(3), pending)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status().as_u16(), 403);
    let row=h.state.storage().query_one_text("SELECT json_object('outcome',outcome,'usageState',usage_state,'attempts',transport_attempts) FROM model_call_usage WHERE origin='activity_summary_quick'",vec![]).await.unwrap().unwrap();
    let row: Value = serde_json::from_str(&row).unwrap();
    assert_eq!(row["outcome"], "cancelled");
    assert_eq!(row["usageState"], "unknown");
    assert!(row["attempts"].is_null());
    assert_eq!(
        h.state
            .runs()
            .quotas()
            .in_use(lingxi_service::quotas::QuotaResource::Model),
        0
    );
    h.shutdown().await;
}

#[tokio::test]
async fn activity_summary_empty_missing_and_device_permission_are_honest() {
    let h = Harness::boot("activity-summary-permission").await;
    let (code, body) = h
        .post("/lingxi/v1/desk/activities/summarize", json!({}))
        .await;
    assert_eq!(code, 200);
    assert_eq!(body["error"], "id required");
    let (code, body) = h
        .post(
            "/lingxi/v1/desk/activities/summarize",
            json!({"id":"missing"}),
        )
        .await;
    assert_eq!(code, 200);
    assert!(body["summary"].is_null());
    let device = h
        .state
        .auth()
        .issue_device_credential("user_local", &["chat", "settings.read"], None)
        .unwrap()
        .secret;
    let (code, _) = h
        .http_as(
            &device,
            reqwest::Method::POST,
            "/lingxi/v1/desk/activities/summarize",
            Some(json!({"id":"missing"})),
        )
        .await;
    assert_eq!(code, 403);
    h.shutdown().await;
}
