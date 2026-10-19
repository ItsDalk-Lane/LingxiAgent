//! R07-T07 β4：日记 write/list 集成测试。
//!
//! 覆盖：GET list（缺目录 {files:[]}、.md 过滤、UTF-16 序倒排、locale
//! 目录名）；POST write（零材料 zh/en 400、targetDate 校验回落、摘要
//! 材料直用、无摘要会话临时补齐、陈旧摘要 messageCount 补充、
//! previous-summary 更新臂、<mood> 剥离、标题兜底与文件名后缀推导、
//! LLM 失败 400、memory 槽未配置 500、范围外会话无材料、人格模板填
//! 充进系统提示、model_call_usage 账本 origin=memory.diary.*）；auth
//! LocalOnly（无 token 401、设备凭证 403）。
//!
//! 夹具：真 TCP loopback 服务 + 真 ServiceState + 确定性 openai-
//! completions 替身（dream 夹具模式）；摘要按现役落盘契约直接写文件；
//! 会话消息经真实 execute（chat 模型走同一替身）入 run DB。

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

/// pi SUMMARIZATION_SYSTEM_PROMPT（与 diary.rs 同一常量文本）。
const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";

#[derive(Clone)]
struct StubConfig {
    /// memory 槽调用返回 500（传输失败）。
    fail_memory: bool,
    /// 临时摘要回复文本。
    memory_reply: String,
    /// 终稿日记回复；None → 默认带标题文本。
    diary_reply: Option<String>,
}

struct Fixture {
    home: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    base: String,
    token: String,
    device: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stub_config: Arc<Mutex<StubConfig>>,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
    stub_stop: tokio::sync::oneshot::Sender<()>,
    stub_task: tokio::task::JoinHandle<()>,
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

impl Fixture {
    async fn boot(tag: &str) -> Self {
        Self::boot_with(tag, Some("zh"), true).await
    }

    /// locale = None 不写 preferences locale（默认 zh）；with_memory =
    /// false 时模型面只配 chat 槽（memory 槽缺失 → 现役 chat 回退臂，
    /// γ 修复 B 后仍成功）。
    async fn boot_with(tag: &str, locale: Option<&str>, with_memory: bool) -> Self {
        Self::boot_impl(tag, locale, true, with_memory).await
    }

    /// chat 与 memory 槽双缺（现役回退链尽头 → 500 路径）。
    async fn boot_without_models(tag: &str) -> Self {
        Self::boot_impl(tag, Some("zh"), false, false).await
    }

    async fn boot_impl(
        tag: &str,
        locale: Option<&str>,
        with_chat: bool,
        with_memory: bool,
    ) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r07-diary-{tag}-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let product = root.join("product");
        for (dir, name, text) in [
            ("identity-templates", "lingxi", "身份模板 {{agentName}}"),
            ("agents-templates", "lingxi", "助手说明 {{agentId}}"),
            (
                "yuan",
                "lingxi",
                "你是 {{agentName}}，用户的协作助手 {{userName}}",
            ),
        ] {
            std::fs::create_dir_all(product.join(dir)).unwrap();
            std::fs::write(product.join(dir).join(format!("{name}.md")), text).unwrap();
        }
        let agent_dir = home.join("agents").join("a");
        std::fs::create_dir_all(agent_dir.join("memory")).unwrap();
        std::fs::write(
            agent_dir.join("config.yaml"),
            "locale: zh\nagent:\n  name: 灵犀\nmemory:\n  enabled: true\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        let mut prefs = json!({"current_agent_id": "a"});
        if let Some(locale) = locale {
            prefs["locale"] = json!(locale);
        }
        std::fs::write(
            home.join("user/preferences.json"),
            serde_json::to_string(&prefs).unwrap(),
        )
        .unwrap();

        // ── 确定性供应商替身（dream 夹具同构） ──
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stub_addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stub_config = Arc::new(Mutex::new(StubConfig {
            fail_memory: false,
            memory_reply: "临时摘要：用户讨论了日记功能。".to_string(),
            diary_reply: None,
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
                    let system = request["messages"][0]["content"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let config = served_config.lock().unwrap().clone();
                    let is_memory = request["model"] == "memory-model";
                    if is_memory && config.fail_memory {
                        let wire = "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
                        socket.write_all(wire.as_bytes()).await.unwrap();
                        return;
                    }
                    let content = if !is_memory {
                        "好的，记下了。".to_string()
                    } else if system == SUMMARIZATION_SYSTEM_PROMPT {
                        config.memory_reply.clone()
                    } else {
                        config.diary_reply.clone().unwrap_or_else(|| {
                            "# 2026-10-17：充实的一天\n\n今天和用户聊了很多。".to_string()
                        })
                    };
                    let frame = json!({"id":"diary","choices":[{"index":0,"delta":{"role":"assistant","content":content},"finish_reason":"stop"}]});
                    let sse = format!(
                        "data: {frame}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        json!({"id":"diary","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":7}})
                    );
                    let wire = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}", sse.len());
                    socket.write_all(wire.as_bytes()).await.unwrap();
                });
            }
        });

        let mut models = json!({});
        if with_chat {
            models["chat"] = json!({"provider":"diary-stub","model":"chat-model","capabilities":{"tools":true},"compat":{"contextWindow":200000}});
        }
        if with_memory {
            models["memory"] = json!({"provider":"diary-stub","model":"memory-model","compat":{"contextWindow":200000}});
        }
        let plane_json = json!({
            "providers": {"diary-stub": {"protocol":"openai-completions","endpoint":format!("http://{stub_addr}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-diary-key"}}},
            "models": models,
        });
        let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(
            &plane_json.to_string(),
        )
        .unwrap();
        let credential = Arc::new(
            lingxi_service::credentials::CredentialService::bootstrap(
                &plane,
                &root.join("credentials"),
                Arc::new(lingxi_service::inject::SystemClock),
            )
            .unwrap(),
        );
        let gateway =
            Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane));
        let compiler = Arc::new(
            lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
                lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                    &home,
                    product.clone(),
                    lingxi_service::context_compiler::HostFacts {
                        platform: "darwin".into(),
                        os_type: "Darwin".into(),
                        os_release: "test".into(),
                        shell_label: "zsh".into(),
                        workspace: workspace.to_string_lossy().into_owned(),
                    },
                ),
            )),
        );
        let config = ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        };
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            config,
            &layout,
            ServiceDeps {
                model_gateway: Some(gateway),
                credential_service: Some(credential),
                network_plane: Some(
                    lingxi_adapters::models::network::NetworkPlane::direct_isolated(),
                ),
                workspace_root: Some(workspace.clone()),
                product_material_dir: Some(product),
                context_compiler: Some(compiler),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let device = state
            .auth()
            .issue_device_credential("user_local", &[], None)
            .unwrap()
            .secret;
        let (stop, stop_rx) = tokio::sync::oneshot::channel();
        let (ready, ready_rx) = tokio::sync::oneshot::channel();
        let served = state.clone();
        let server = tokio::spawn(async move {
            run(
                served,
                async {
                    let _ = stop_rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
            .unwrap();
        });
        let addr = ready_rx.await.unwrap();
        Self {
            home,
            workspace,
            state,
            base: format!("http://{addr}"),
            token,
            device,
            requests,
            stub_config,
            stop,
            server,
            stub_stop,
            stub_task,
        }
    }

    async fn http(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        self.http_as(&self.token, method, path, body).await
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

    /// 现役 catch 后 {} 的等价面：非 JSON 正文。
    async fn http_raw_body(&self, path: &str, raw: &str) -> (u16, Value) {
        let response = reqwest::Client::new()
            .request(reqwest::Method::POST, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header("Content-Type", "application/json")
            .body(raw.to_string())
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn create_session(&self, session_id: &str) {
        let (code, body) = self
            .http(
                reqwest::Method::POST,
                "/lingxi/v1/sessions",
                Some(json!({"sessionId": session_id, "agentId": "a", "title": "日记源会话"})),
            )
            .await;
        assert_eq!(code, 201, "会话 {session_id} 创建：{body}");
    }

    /// 经真实 execute（chat 模型走替身）把一轮对话写进 run DB。
    async fn chat(&self, session_id: &str, input: &str) {
        let (code, started) = self
            .http(
                reqwest::Method::POST,
                &format!("/lingxi/v1/sessions/{session_id}/execute"),
                Some(json!({"input": input})),
            )
            .await;
        assert_eq!(code, 200, "execute {session_id}: {started}");
        let run_id = started["runId"].as_str().unwrap().to_string();
        for _ in 0..400 {
            let status = self
                .state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id=?1",
                    vec![run_id.clone()],
                )
                .await
                .unwrap();
            if status.as_deref() == Some("completed") {
                return;
            }
            assert_ne!(status.as_deref(), Some("failed"), "Run 失败");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("Run 未及时完成");
    }

    /// 按现役 summaries 落盘契约直接写摘要文件（时间戳可控）。
    fn write_summary(&self, session_id: &str, summary: &Value) {
        let dir = self
            .home
            .join("agents")
            .join("a")
            .join("memory")
            .join("summaries");
        std::fs::create_dir_all(&dir).unwrap();
        let mut value = summary.clone();
        value["session_id"] = json!(session_id);
        std::fs::write(dir.join(format!("{session_id}.json")), value.to_string()).unwrap();
    }

    fn memory_requests(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["model"] == "memory-model")
            .cloned()
            .collect()
    }

    async fn shutdown(self) {
        let _ = self.stop.send(());
        self.server.await.unwrap();
        let _ = self.stub_stop.send(());
        self.stub_task.await.unwrap();
        self.state.storage().close().await.unwrap();
    }
}

// ── 时间助手（与 diary.rs 同一 04:00 逻辑日算法，测试侧独立计算） ──

/// 当前逻辑日 [start, end)（本地 04:00 界）的 UTC unix ms。
fn logical_range_now() -> (i64, i64) {
    use chrono::{Datelike, TimeZone, Timelike};
    let now = chrono::Local::now();
    let base = if now.hour() < 4 {
        now - chrono::Duration::days(1)
    } else {
        now
    };
    let start = chrono::Local
        .with_ymd_and_hms(base.year(), base.month(), base.day(), 4, 0, 0)
        .single()
        .unwrap();
    let next = start + chrono::Duration::days(1);
    (start.timestamp_millis(), next.timestamp_millis())
}

/// 相对当前逻辑日的偏移日期（YYYY-MM-DD，本地日历）。
fn logical_date_offset(days: i64) -> String {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let base = if now.hour() < 4 {
        now - chrono::Duration::days(1)
    } else {
        now
    };
    let date = chrono::NaiveDate::from_ymd_opt(base.year(), base.month(), base.day()).unwrap()
        + chrono::Duration::days(days);
    date.format("%Y-%m-%d").to_string()
}

fn iso_of(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn diary_dir(fx: &Fixture, label: &str) -> PathBuf {
    fx.workspace.join("OH-Works").join(label)
}

// ── 列表 ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_missing_dir_then_utf16_descending() {
    let fx = Fixture::boot("list").await;
    let (code, body) = fx
        .http(reqwest::Method::GET, "/lingxi/v1/diary/list", None)
        .await;
    assert_eq!(code, 200);
    assert_eq!(body, json!({"files": []}));

    let dir = diary_dir(&fx, "日记");
    std::fs::create_dir_all(&dir).unwrap();
    for name in [
        "2026-10-01.md",
        "2026-10-15 长标题.md",
        "notes.txt",
        "2026-09-30.md",
        "2026-10-15.md",
    ] {
        std::fs::write(dir.join(name), "x").unwrap();
    }
    let (code, body) = fx
        .http(reqwest::Method::GET, "/lingxi/v1/diary/list", None)
        .await;
    assert_eq!(code, 200);
    assert_eq!(
        body["files"],
        json!([
            "2026-10-15.md",
            "2026-10-15 长标题.md",
            "2026-10-01.md",
            "2026-09-30.md"
        ])
    );
    fx.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_uses_locale_diary_dir_label() {
    let fx = Fixture::boot_with("list-en", Some("en"), true).await;
    let dir = diary_dir(&fx, "Diary");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("2026-10-16.md"), "x").unwrap();
    // 中文目录下的文件不应出现在 en 标签目录的列表里。
    let zh_dir = diary_dir(&fx, "日记");
    std::fs::create_dir_all(&zh_dir).unwrap();
    std::fs::write(zh_dir.join("2026-10-17.md"), "x").unwrap();
    let (code, body) = fx
        .http(reqwest::Method::GET, "/lingxi/v1/diary/list", None)
        .await;
    assert_eq!(code, 200);
    assert_eq!(body["files"], json!(["2026-10-16.md"]));
    fx.shutdown().await;
}

// ── 写入：材料为空与 targetDate 语义 ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_zero_materials_localized_and_target_date_fallback() {
    let fx = Fixture::boot("zero").await;
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 400);
    assert_eq!(body["error"], "今天还没有对话记录，没什么可写的");
    assert!(body.get("warnings").is_none(), "无警告不投影 warnings 字段");

    // 合法过去日期同样零材料（范围内无会话/摘要）。
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({"targetDate": "2020-01-02"})),
        )
        .await;
    assert_eq!(code, 400);
    assert_eq!(body["error"], "今天还没有对话记录，没什么可写的");

    // 非法日期（13 月 / 99 日 / 0099 年 TS quirk / 非日期形状）全部回落今天。
    for bad in ["2026-13-01", "2026-02-31", "0099-01-01", "not-a-date"] {
        let (code, body) = fx
            .http(
                reqwest::Method::POST,
                "/lingxi/v1/diary/write",
                Some(json!({"targetDate": bad})),
            )
            .await;
        assert_eq!(code, 400, "{bad}: {body}");
        assert_eq!(body["error"], "今天还没有对话记录，没什么可写的");
    }
    // 非字符串 targetDate → undefined（今天）；非 JSON body → catch 后 {}。
    let (code, _) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({"targetDate": 42})),
        )
        .await;
    assert_eq!(code, 400);
    let (code, _) = fx.http_raw_body("/lingxi/v1/diary/write", "{oops").await;
    assert_eq!(code, 400, "非 JSON body 应继续走今天路径");
    fx.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_zero_materials_english_when_locale_en() {
    let fx = Fixture::boot_with("zero-en", Some("en"), true).await;
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 400);
    assert_eq!(
        body["error"],
        "No conversations today — nothing to write about."
    );
    fx.shutdown().await;
}

// ── 写入：摘要材料直用 + 终稿链 ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_with_summary_material_produces_diary_file() {
    let fx = Fixture::boot("summary").await;
    let (start, _end) = logical_range_now();
    fx.write_summary(
        "ghost-session",
        &json!({
            "summary": "用户和助手讨论了日记迁移的验收口径。",
            "created_at": iso_of(start + 3_600_000),
            "updated_at": iso_of(start + 3_600_000),
            "messageCount": 4,
        }),
    );
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let logical_date = body["logicalDate"].as_str().unwrap();
    assert_eq!(logical_date, logical_date_offset(0));
    assert_eq!(
        body["content"],
        "# 2026-10-17：充实的一天\n\n今天和用户聊了很多。"
    );
    let file_path = PathBuf::from(body["filePath"].as_str().unwrap());
    assert_eq!(
        file_path,
        diary_dir(&fx, "日记").join(format!("{logical_date} 充实的一天.md"))
    );
    assert_eq!(
        std::fs::read_to_string(&file_path).unwrap(),
        "# 2026-10-17：充实的一天\n\n今天和用户聊了很多。\n"
    );

    // 终稿请求：系统提示 = 填充后人格（identity+yuan+AGENTS.md），用户
    // 提示含摘要材料、无活动段、含写作约束与逻辑日指令。
    let memory = fx.memory_requests();
    assert_eq!(memory.len(), 1, "无会话时不应有临时摘要调用: {memory:?}");
    let final_call = &memory[0];
    let system = final_call["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("身份模板 灵犀"), "{system}");
    assert!(
        system.contains("你是 灵犀，用户的协作助手 用户"),
        "{system}"
    );
    assert!(system.contains("助手说明 a"), "{system}");
    let user = final_call["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(user.contains("# 今日对话摘要"), "{user}");
    assert!(
        user.contains("### ghost-session\n\n用户和助手讨论了日记迁移的验收口径。"),
        "{user}"
    );
    assert!(!user.contains("临时补齐"), "{user}");
    assert!(user.contains("# 写作约束"), "{user}");
    assert!(user.contains("你叫灵犀，用户叫用户"), "{user}");
    assert!(
        user.contains(&format!("请为 {logical_date} 写一篇日记。")),
        "{user}"
    );

    // 账本：origin=memory.diary.write / purpose=auxiliary.memory。
    let origin = fx
        .state
        .storage()
        .query_one_text(
            "SELECT origin FROM model_call_usage WHERE origin='memory.diary.write'",
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(origin.as_deref(), Some("memory.diary.write"));
    fx.shutdown().await;
}

// ── 写入：无摘要会话走临时补齐 ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_session_without_summary_uses_temporary_material() {
    let fx = Fixture::boot("temp").await;
    fx.create_session("s-talk").await;
    fx.chat("s-talk", "今天帮我把日记迁移的测试写完").await;

    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{body}");

    let memory = fx.memory_requests();
    assert_eq!(memory.len(), 2, "临时补齐 + 终稿各一次: {memory:?}");
    let temp = &memory[0];
    assert_eq!(
        temp["messages"][0]["content"].as_str().unwrap(),
        SUMMARIZATION_SYSTEM_PROMPT
    );
    let temp_user = temp["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(temp_user.contains("<conversation>"), "{temp_user}");
    assert!(
        temp_user.contains("[User]: 今天帮我把日记迁移的测试写完"),
        "{temp_user}"
    );
    assert!(
        temp_user.contains("[Assistant]: 好的，记下了。"),
        "{temp_user}"
    );
    assert!(!temp_user.contains("<previous-summary>"), "{temp_user}");
    assert!(
        temp_user.contains("Additional focus: 这份摘要只作为今天私人日记的临时素材"),
        "{temp_user}"
    );

    let final_user = memory[1]["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        final_user
            .contains("### s-talk（临时补齐，不写回 session）\n\n临时摘要：用户讨论了日记功能。"),
        "{final_user}"
    );
    fx.shutdown().await;
}

// ── 写入：陈旧摘要（messageCount 少于范围消息）追加临时补充 ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_stale_summary_appends_supplement_with_previous() {
    let fx = Fixture::boot("stale").await;
    fx.create_session("s-stale").await;
    fx.chat("s-stale", "第一轮：讨论摘要口径").await;
    fx.chat("s-stale", "第二轮：继续日记验收").await;
    let (start, _end) = logical_range_now();
    // 摘要 messageCount=2（一轮用户+助手），实际范围消息 4 条 → 补充臂；
    // selectSupplement 按 messageCount 切片落在范围集上（现役怪癖原样）。
    fx.write_summary(
        "s-stale",
        &json!({
            "summary": "已覆盖：用户讨论了摘要口径。",
            "created_at": iso_of(start + 1_800_000),
            "updated_at": iso_of(start + 1_800_000),
            "messageCount": 2,
        }),
    );
    fx.stub_config.lock().unwrap().memory_reply = "补充：第二轮继续了日记验收。".to_string();

    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{body}");

    let memory = fx.memory_requests();
    assert_eq!(memory.len(), 2, "补充 + 终稿: {memory:?}");
    let temp_user = memory[0]["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    // messageCount=2 切片 → 只补第二轮。
    assert!(!temp_user.contains("第一轮"), "{temp_user}");
    assert!(
        temp_user.contains("[User]: 第二轮：继续日记验收"),
        "{temp_user}"
    );
    assert!(
        temp_user.contains("<previous-summary>\n已覆盖：用户讨论了摘要口径。\n</previous-summary>"),
        "{temp_user}"
    );
    assert!(
        temp_user.contains("NEW conversation messages"),
        "{temp_user}"
    );

    let final_user = memory[1]["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        final_user.contains("### s-stale\n\n已覆盖：用户讨论了摘要口径。"),
        "{final_user}"
    );
    assert!(
        final_user
            .contains("### s-stale（临时补齐，不写回 session）\n\n补充：第二轮继续了日记验收。"),
        "{final_user}"
    );
    fx.shutdown().await;
}

// ── 写入：范围外会话不产生材料 ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_for_yesterday_ignores_todays_session() {
    let fx = Fixture::boot("range").await;
    fx.create_session("s-today").await;
    fx.chat("s-today", "今天的对话").await;
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({"targetDate": logical_date_offset(-1)})),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["error"], "今天还没有对话记录，没什么可写的");
    assert!(fx.memory_requests().is_empty(), "范围外不得调用模型");
    fx.shutdown().await;
}

// ── 写入：后处理（mood 剥离、标题兜底、文件名推导） ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_strips_mood_blocks_and_fallback_title() {
    let fx = Fixture::boot("mood").await;
    let (start, _end) = logical_range_now();
    fx.write_summary(
        "ghost-mood",
        &json!({
            "summary": "一段摘要。",
            "created_at": iso_of(start + 3_600_000),
            "updated_at": iso_of(start + 3_600_000),
        }),
    );
    fx.stub_config.lock().unwrap().diary_reply =
        Some("<mood>开心</mood>正文开头<pulse>x</pulse>中段<reflect>r</reflect>尾。".to_string());
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let logical_date = body["logicalDate"].as_str().unwrap();
    assert_eq!(
        body["content"],
        format!("# {logical_date}\n\n正文开头中段尾。")
    );
    let file_path = PathBuf::from(body["filePath"].as_str().unwrap());
    assert_eq!(
        file_path,
        diary_dir(&fx, "日记").join(format!("{logical_date}.md"))
    );
    fx.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_title_date_prefix_and_illegal_chars_shape_filename() {
    let fx = Fixture::boot("title").await;
    let (start, _end) = logical_range_now();
    fx.write_summary(
        "ghost-title",
        &json!({
            "summary": "一段摘要。",
            "created_at": iso_of(start + 3_600_000),
            "updated_at": iso_of(start + 3_600_000),
        }),
    );
    let logical_date = logical_date_offset(0);
    fx.stub_config.lock().unwrap().diary_reply =
        Some(format!("# {logical_date}： 整理/日记:非法?字符\n\n正文。"));
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    // 日期前缀剥掉，非法字符剥掉（/ : ?），保留其余。
    let file_path = PathBuf::from(body["filePath"].as_str().unwrap());
    assert_eq!(
        file_path.file_name().unwrap().to_string_lossy(),
        format!("{logical_date} 整理日记非法字符.md")
    );
    assert_eq!(
        body["content"],
        format!("# {logical_date}： 整理/日记:非法?字符\n\n正文。")
    );
    fx.shutdown().await;
}

// ── 写入：失败面 ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_llm_failure_maps_400_with_localized_message() {
    let fx = Fixture::boot("llm-fail").await;
    let (start, _end) = logical_range_now();
    fx.write_summary(
        "ghost-fail",
        &json!({
            "summary": "一段摘要。",
            "created_at": iso_of(start + 3_600_000),
            "updated_at": iso_of(start + 3_600_000),
        }),
    );
    fx.stub_config.lock().unwrap().fail_memory = true;
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    let message = body["error"].as_str().unwrap();
    assert!(message.starts_with("LLM 调用失败: "), "{message}");
    assert!(
        body.get("warnings").is_none(),
        "LLM 失败不投影 warnings: {body}"
    );
    fx.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_temporary_failure_surfaces_warning_envelope() {
    let fx = Fixture::boot("temp-fail").await;
    fx.create_session("s-fail").await;
    fx.chat("s-fail", "唯一一轮").await;
    fx.stub_config.lock().unwrap().fail_memory = true;
    // 唯一材料来源的临时摘要失败 → 零材料 + warnings 的 400 变体。
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    let message = body["error"].as_str().unwrap();
    assert!(
        message.starts_with("日记材料准备失败：今天的对话都没能整理成可用素材。"),
        "{message}"
    );
    let warnings = body["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["sessionId"], "s-fail");
    assert_eq!(warnings[0]["stage"], "temporary-summary");
    assert!(!warnings[0]["message"].as_str().unwrap().is_empty());
    // 失败详情进 error 文本（formatMaterialWarnings 形状）。
    assert!(message.contains("s-fail temporary-summary: "), "{message}");
    fx.shutdown().await;
}

// γ 修复 B：现役 memory 槽 fallback=chat（auxiliary-slots.ts:92-97）——
// 槽未配置但 chat 在时日记经 chat 模型完成。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_memory_slot_unconfigured_falls_back_to_chat() {
    let fx = Fixture::boot_with("no-memory", Some("zh"), false).await;
    fx.create_session("s-fallback").await;
    fx.chat("s-fallback", "今天聊了记忆槽的回退机制").await;
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let path = body["filePath"].as_str().unwrap();
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.contains("好的，记下了。"), "{content}");
    assert!(
        fx.memory_requests().is_empty(),
        "memory 槽缺失时不应有 memory-model 请求"
    );
    // 账本如实记 chat 路由（served-by 是解析结果而非请求身份）。
    let model = fx
        .state
        .storage()
        .query_one_text(
            "SELECT model FROM model_call_usage WHERE origin='memory.diary.write'",
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(model.as_deref(), Some("chat-model"));
    fx.shutdown().await;
}

// 槽与 chat 双缺 = 现役回退链尽头 → 500。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_memory_and_chat_both_unconfigured_maps_500() {
    let fx = Fixture::boot_without_models("no-models").await;
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 500, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("记忆模型未配置"),
        "{body}"
    );
    fx.shutdown().await;
}

// ── auth：diary 面 LocalOnly（现役 /api/* 兜底 STUDIO_OWNER 口径） ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn diary_routes_require_local_owner() {
    let fx = Fixture::boot("auth").await;
    for (method, path) in [
        (reqwest::Method::GET, "/lingxi/v1/diary/list"),
        (reqwest::Method::POST, "/lingxi/v1/diary/write"),
    ] {
        let response = reqwest::Client::new()
            .request(method.clone(), format!("{}{path}", fx.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 401, "{method} {path} 无 token");
        let (code, _) = fx.http_as(&fx.device, method.clone(), path, None).await;
        assert_eq!(code, 403, "{method} {path} 设备凭证");
    }
    fx.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn diary_includes_same_agent_real_cron_activity_but_excludes_promoted_activity() {
    let fx = Fixture::boot("activity-material").await;
    let (start, _) = logical_range_now();
    fx.write_summary("diary-material",&json!({"summary":"用户讨论了今天的工作。","created_at":iso_of(start+1000),"updated_at":iso_of(start+1000),"messageCount":1}));
    fx.create_session("cron-diary-source").await;
    let due = fx.state.clock().now_unix_ms() + 1000;
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/cron",
            Some(json!({
                "action":"add","actorAgentId":"a","type":"at","schedule":due,
                "prompt":"后台整理真实材料","label":"日记活动接线",
                "executionContext":{"sourceSessionId":"cron-diary-source"}
            })),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    tokio::time::timeout(
        Duration::from_secs(10),
        fx.state.automation().check_due(&fx.state),
    )
    .await
    .unwrap()
    .unwrap();
    let (code, list) = fx
        .http(reqwest::Method::GET, "/lingxi/v1/desk/activities", None)
        .await;
    assert_eq!(code, 200, "{list}");
    let activity = list["activities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["label"] == "日记活动接线")
        .unwrap();
    assert_eq!(activity["status"], "done", "{activity}");
    let id = activity["id"].as_str().unwrap().to_owned();
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let requests = fx.memory_requests();
    let prompt = requests.last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(
        prompt.contains("# 今日后台活动（巡检与定时任务）"),
        "{prompt}"
    );
    assert!(prompt.contains("定时任务:日记活动接线"), "{prompt}");
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            &format!("/lingxi/v1/desk/activities/{id}/promote"),
            None,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let (code, body) = fx
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/diary/write",
            Some(json!({})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let requests = fx.memory_requests();
    let prompt = requests.last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(
        !prompt.contains("# 今日后台活动（巡检与定时任务）"),
        "{prompt}"
    );
    fx.shutdown().await;
}
