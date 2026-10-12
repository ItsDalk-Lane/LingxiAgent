//! R06-T07 服务级测试（真 TCP + 真 HTTP + 真 bootstrap，T05/T06 TestServer 模式）。
//!
//! R06-A13「检索引用可验证且不串来源」：两个来源空间（笔记本）含相近内容；
//! scope 编译只列所选笔记本的来源；引用（citation）能定位到实际原文
//! （block 偏移 + canonicalText == 快照原文切片）；伪造/越权 scope 被拒。
//! （检索通道的来源隔离由 r06_t07_knowledge_tools 经 R04 网关真面证明。）
//!
//! R06-A14「embedding 变化不静默损坏」：dim=2 嵌入替身建索引 → 切 dim=3
//! 模型（同 data_home 重启）→ 检索显式降级 FTS + KNOWLEDGE_VECTOR_NOT_READY，
//! 不返回 dim=2 变体的貌似正常命中；重建后恢复 hybrid 且源文件/快照不变。
//!
//! 环境口径：外部 embedding 提供方使用本机 loopback 确定性替身（openai-embeddings
//! 线格式；维度由模型名决定），不冒称真实供应商验证。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── embedding 确定性替身 ─────────────────────────────────────────────────────

/// openai-embeddings 线格式的确定性替身：维度由请求的 model 名决定
/// （`embed-model-a` → 2 维、`embed-model-b` → 3 维、其余 → 4 维），向量值由
/// 输入文本哈希确定性导出，可复现且与真实供应商无关。
async fn spawn_embedding_stub() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("stub bind");
    let addr = listener.local_addr().expect("stub addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut buf))
                    .await;
                let raw = String::from_utf8_lossy(&buf);
                let body_start = raw.split("\r\n\r\n").nth(1).unwrap_or("");
                let parsed: Value = serde_json::from_str(body_start).unwrap_or(Value::Null);
                let model = parsed["model"]
                    .as_str()
                    .unwrap_or("embed-model-other")
                    .to_string();
                let dims = match model.as_str() {
                    "embed-model-a" => 2,
                    "embed-model-b" => 3,
                    _ => 4,
                };
                let inputs = parsed["input"].as_array().cloned().unwrap_or_default();
                let data: Vec<Value> = inputs
                    .iter()
                    .enumerate()
                    .map(|(index, input)| {
                        let text = input.as_str().unwrap_or("");
                        let mut seed = {
                            let mut acc = 0x811c9dc5_u32;
                            for byte in text.bytes() {
                                acc ^= byte as u32;
                                acc = acc.wrapping_mul(0x01000193);
                            }
                            acc
                        };
                        let mut vector = Vec::with_capacity(dims);
                        for _ in 0..dims {
                            seed = seed.wrapping_mul(0x01000193).wrapping_add(1);
                            vector.push(((seed % 2000) as f32 - 1000.0) / 1000.0);
                        }
                        json!({"index": index, "embedding": vector})
                    })
                    .collect();
                let usage =
                    json!({ "prompt_tokens": inputs.len() * 5, "total_tokens": inputs.len() * 5 });
                let body = json!({"data": data, "usage": usage, "model": model}).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    addr
}

// ── 服务骨架 ────────────────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t07-knowledge-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn config_for(home: &Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

/// 带/不带 embedding 模型面（指向 loopback 替身）的 bootstrap；`model_id` 决定替身维度。
async fn boot(tag: &str, home: &Path, embedding: Option<(&str, SocketAddr)>) -> ServiceState {
    let layout = prepare_layout(home).expect("prepare layout");
    let deps = match embedding {
        Some((model_id, stub_addr)) => {
            let config_root = unique_dir(&format!("{tag}-cfg"));
            let config_path = config_root.join("service.json");
            let endpoint = format!("http://{stub_addr}/v1");
            let config_json = format!(
                r#"{{"home": {}, "providers": {{"embed": {{"protocol": "openai-embeddings", "endpoint": {}, "auth": {{"kind": "apiKey", "apiKey": "R06T07_STUB_KEY"}}}}}}, "models": {{"embedding": {{"provider": "embed", "model": {}}}}}}}"#,
                serde_json::to_string(&home.to_string_lossy()).expect("home json"),
                serde_json::to_string(&endpoint).expect("endpoint json"),
                serde_json::to_string(model_id).expect("model json"),
            );
            std::fs::write(&config_path, &config_json).expect("write config");
            let _file =
                lingxi_service::config::read_service_config(&config_path).expect("service config");
            let (source, plane) = lingxi_service::config::resolve_model_plane(
                Some(&config_path),
                &layout.runtime_dir,
            )
            .expect("plane resolves")
            .expect("plane present");
            let credential_service = Arc::new(
                lingxi_service::credentials::CredentialService::bootstrap(
                    &plane,
                    &layout.runtime_dir,
                    Arc::new(lingxi_service::inject::SystemClock),
                )
                .expect("credential service"),
            );
            ServiceDeps {
                model_gateway: Some(Arc::new(
                    lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
                )),
                model_plane_source: Some(source),
                credential_service: Some(credential_service),
                ..ServiceDeps::default()
            }
        }
        None => ServiceDeps::default(),
    };
    ServiceState::bootstrap_with_deps(config_for(home), &layout, deps)
        .await
        .expect("bootstrap")
}

struct TestServer {
    addr: SocketAddr,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn serve(state: ServiceState) -> TestServer {
    let token = state.auth().local_token();
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
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        addr,
        token,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let (status, _, body) = self.request_raw(method, path, body).await;
        if std::env::var("R06T07_DEBUG").is_ok() {
            eprintln!(
                "DEBUG {method} {path} -> {status}: {}",
                String::from_utf8_lossy(&body)
            );
        }
        let parsed = if body.is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(_) => {
                    eprintln!(
                        "DEBUG non-JSON body for {method} {path}: {}",
                        String::from_utf8_lossy(&body)
                    );
                    Value::Null
                }
            }
        };
        (status, parsed)
    }

    async fn request_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            self.token,
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
        let headers: Vec<(String, String)> = head
            .split("\r\n")
            .skip(1)
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            })
            .collect();
        (status, headers, buf[header_end + 4..].to_vec())
    }

    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
    }
}

async fn wait_until_job(
    server: &TestServer,
    notebook_id: &str,
    source_id: &str,
    expect_status: &str,
) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (status, jobs) = server
            .request(
                "GET",
                &format!(
                    "/lingxi/v1/knowledge/ingestion?notebookId={notebook_id}&sourceId={source_id}"
                ),
                None,
            )
            .await;
        assert_eq!(status, 200, "{jobs}");
        let hit = jobs["jobs"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .find(|j| j["status"] == expect_status)
            .cloned();
        if let Some(job) = hit {
            return job;
        }
        if std::time::Instant::now() > deadline {
            panic!("job never reached {expect_status}: {jobs}");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

const NOTE_TEXT_A: &str = "# 第一章 灵犀架构\n\n灵犀是有记忆的个人 AI 助手。\n它包含 Rust 服务与知识库。\n\n# 第二章 检索\n\n知识库支持来源作用域检索。\n引用必须能定位原文。";

// ── 测试 ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn notebook_crud_settings_and_ingestion_summary() {
    let home = unique_dir("crud");
    let state = boot("crud", &home, None).await;
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";

    // 创建：返回 201。
    let (status, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "资料库A"})),
        )
        .await;
    assert_eq!(status, 201, "{created}");
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();
    assert_eq!(created["notebook"]["name"], "资料库A");

    // body 精确键校验：多余键 → KNOWLEDGE_INVALID_ARGUMENT。
    let (status, err) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "x", "extra": 1})),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(err["error"], "KNOWLEDGE_INVALID_ARGUMENT");

    // 列表：config + 生效分块 + 摄入汇总。
    let (status, listed) = server
        .request("GET", &format!("{base}/notebooks"), None)
        .await;
    assert_eq!(status, 200);
    let entry = listed["notebooks"]
        .as_array()
        .expect("array")
        .iter()
        .find(|n| n["id"] == notebook_id.as_str())
        .expect("entry")
        .clone();
    assert!(entry["config"].is_object());
    assert!(entry["chunkTargetCharsEffective"].is_u64());
    assert_eq!(entry["sourceCount"], 0);
    assert_eq!(entry["ingestion"]["done"], 0);

    // 改名 + 查单。
    let (status, renamed) = server
        .request(
            "PATCH",
            &format!("{base}/notebooks/{notebook_id}"),
            Some(json!({"name": "资料库A2"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(renamed["notebook"]["name"], "资料库A2");

    // settings：非法数值 → 400；合法 → 200。
    let (status, err) = server
        .request(
            "PUT",
            &format!("{base}/notebooks/{notebook_id}/settings"),
            Some(json!({"chunkTargetChars": 1})),
        )
        .await;
    assert_eq!(status, 400, "{err}");
    let (status, cfg) = server
        .request(
            "PUT",
            &format!("{base}/notebooks/{notebook_id}/settings"),
            Some(json!({"chunkTargetChars": 800})),
        )
        .await;
    assert_eq!(status, 200, "{cfg}");
    assert_eq!(cfg["config"]["chunkTargetChars"], 800);

    // 删除 + 复查 404。
    let (status, _) = server
        .request("DELETE", &format!("{base}/notebooks/{notebook_id}"), None)
        .await;
    assert_eq!(status, 200);
    let (status, err) = server
        .request("GET", &format!("{base}/notebooks/{notebook_id}"), None)
        .await;
    assert_eq!(status, 404);
    assert_eq!(err["error"], "KNOWLEDGE_NOT_FOUND");

    server.stop().await;
}

#[tokio::test]
async fn import_pasted_text_parses_chunks_and_ingests_with_stub_embeddings() {
    let home = unique_dir("ingest");
    let stub_addr = spawn_embedding_stub().await;
    let state = boot("ingest", &home, Some(("embed-model-a", stub_addr))).await;
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";

    let (status, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "摄入库"})),
        )
        .await;
    assert_eq!(status, 201, "create");
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();

    let (status, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "pasted_text", "text": NOTE_TEXT_A, "displayName": "架构笔记"})),
        )
        .await;
    assert_eq!(status, 201, "{imported}");
    let source_id = imported["source"]["id"]
        .as_str()
        .expect("source id")
        .to_string();
    let snapshot_id = imported["snapshot"]["id"]
        .as_str()
        .expect("snapshot id")
        .to_string();
    let artifact_id = imported["parseArtifact"]["id"]
        .as_str()
        .expect("artifact id")
        .to_string();
    assert_eq!(imported["source"]["sourceType"], "pasted_text");
    assert_eq!(imported["parseArtifact"]["status"], "ready");
    // storagePath / semanticArtifactPath 不出域。
    assert!(imported["snapshot"]["storagePath"].is_null());
    assert!(imported["parseArtifact"]["semanticArtifactPath"].is_null());

    // 摄入完成（parse→chunk→fts_index→embed→done），embeddingStats 记录真实身份。
    let job = wait_until_job(&server, &notebook_id, &source_id, "done").await;
    assert_eq!(job["phase"], "done");
    let dims = job["embeddingStats"]["model"]["dimensions"]
        .as_u64()
        .expect("dims");
    assert_eq!(dims, 2, "stub embed-model-a is dim=2: {job}");
    assert!(
        job["embeddingStats"]["model"]["key"]
            .as_str()
            .expect("model key")
            .len()
            == 64
    );

    // 解析块：逐行块 + markdown locator。
    let (status, blocks) = server
        .request(
            "GET",
            &format!("{base}/parse-artifacts/{artifact_id}/blocks"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let blocks = blocks["blocks"].as_array().expect("blocks").clone();
    assert!(blocks.len() >= 4, "line blocks: {blocks:?}");
    // pasted_text 以 text/plain 引用级解析（TS importPastedText 恒 text/plain）。
    assert_eq!(blocks[0]["locatorType"], "text");

    // 分块卡片：冻结契约 {chunkerConfigId, chunks:[{id, ordinal 1-based, text,...}]}。
    let (status, cards) = server
        .request(
            "GET",
            &format!("{base}/parse-artifacts/{artifact_id}/chunks"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{cards}");
    assert_eq!(
        cards["chunkerConfigId"].as_str().expect("config id").len(),
        16
    );
    let chunks = cards["chunks"].as_array().expect("chunks").clone();
    assert!(!chunks.is_empty());
    assert_eq!(chunks[0]["ordinal"], 1);
    assert!(chunks[0]["text"].as_str().expect("text").contains("灵犀"));

    // 不存在的 artifact → 404。
    let (status, err) = server
        .request(
            "GET",
            &format!("{base}/parse-artifacts/kpa_missing/chunks"),
            None,
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(err["error"], "KNOWLEDGE_NOT_FOUND");

    // 来源列表：source + snapshot + membership + parseArtifact；存储路径不出域。
    let (status, sources) = server
        .request(
            "GET",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let entry = &sources["sources"][0];
    assert_eq!(entry["source"]["id"], source_id.as_str());
    assert_eq!(entry["snapshot"]["id"], snapshot_id.as_str());
    assert_eq!(entry["membership"]["notebookId"], notebook_id.as_str());
    assert!(entry["snapshot"]["storagePath"].is_null());

    server.stop().await;
}

#[tokio::test]
async fn ingestion_without_model_lands_pending_embedding_then_completes_after_reboot() {
    let home = unique_dir("pending");
    // 第一棒：无模型面 → embed 相位显式 pending_embedding（禁静默降级红线）。
    let state = boot("pending-a", &home, None).await;
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";
    let (_, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "待嵌库"})),
        )
        .await;
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();
    let (_, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "pasted_text", "text": "灵犀知识库待嵌入验证文本", "displayName": "待嵌"})),
        )
        .await;
    let source_id = imported["source"]["id"]
        .as_str()
        .expect("source id")
        .to_string();
    let job = wait_until_job(&server, &notebook_id, &source_id, "pending_embedding").await;
    // FTS 已可查：FTS 相位已完成。
    assert_eq!(job["phase"], "embed");
    server.stop().await;

    // 第二棒：同一 data_home 配上模型 → 启动恢复把 pending_embedding 重新入队并完成。
    let stub_addr = spawn_embedding_stub().await;
    let state = boot("pending-b", &home, Some(("embed-model-a", stub_addr))).await;
    let server = serve(state).await;
    wait_until_job(&server, &notebook_id, &source_id, "done").await;
    server.stop().await;
}

#[tokio::test]
async fn a14_switching_to_incompatible_embedding_dimension_refuses_vector_channel_until_rebuild() {
    let home = unique_dir("a14");
    let base = "/lingxi/v1/knowledge";

    // 第一棒：dim=2 模型建索引（HTTP 驱动导入与摄入）。
    let stub_addr = spawn_embedding_stub().await;
    let state = boot("a14-a", &home, Some(("embed-model-a", stub_addr))).await;
    let server = serve(state).await;
    let (_, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "向量库"})),
        )
        .await;
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();
    let (_, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "pasted_text", "text": "灵犀向量检索验收文本：来源隔离与维度门。", "displayName": "向量源"})),
        )
        .await;
    let source_id = imported["source"]["id"]
        .as_str()
        .expect("source id")
        .to_string();
    let snapshot_id = imported["snapshot"]["id"]
        .as_str()
        .expect("snapshot id")
        .to_string();
    let artifact_id = imported["parseArtifact"]["id"]
        .as_str()
        .expect("artifact id")
        .to_string();
    let job = wait_until_job(&server, &notebook_id, &source_id, "done").await;
    let dims_a = job["embeddingStats"]["model"]["dimensions"]
        .as_u64()
        .expect("dims a");
    assert_eq!(dims_a, 2, "index built with dimension A=2: {job}");
    let model_key_a = job["embeddingStats"]["model"]["key"]
        .as_str()
        .expect("key a")
        .to_string();
    server.stop().await;

    // 第二棒：同一 data_home 切到 dim=3 模型（真实切换 = 模型配置变更 + 重启）。
    let stub_addr = spawn_embedding_stub().await;
    let state = boot("a14-b", &home, Some(("embed-model-b", stub_addr))).await;
    let server_state = state.clone();
    let server = serve(state).await;
    // scope 编译 + 检索（工具执行器消费的同一服务入口）。
    let knowledge = server_state.knowledge();
    let scope = knowledge
        .create_turn_scope("sess-a14", "turn-a14-1", std::slice::from_ref(&notebook_id))
        .expect("scope");
    let result = knowledge
        .search_in_scope(&lingxi_service::knowledge::SearchRequest {
            scope_id: scope.id.clone(),
            session_id: "sess-a14".into(),
            query: "来源隔离与维度门".into(),
            channel: lingxi_service::knowledge::SearchChannel::Hybrid,
            notebook_ids: vec![],
            source_ids: vec![],
            section_keys: vec![],
            limit: 12,
        })
        .await
        .expect("search");
    // 显式安全策略：向量通道拒绝（不兼容身份），FTS 候选保留，绝不返回貌似正常的错检索。
    assert_eq!(
        result.mode, "fts",
        "vector channel must refuse after model change"
    );
    assert!(
        result
            .degraded
            .iter()
            .any(|d| d.reason == "KNOWLEDGE_VECTOR_NOT_READY" && d.detail.contains("rebuild")),
        "degraded must carry the rebuild hint: {:?}",
        result.degraded
    );
    // 对照组：dim=2 的旧变体仍在库（可重建 ≠ 旧数据被偷换），但它的向量绝不参与检索。
    let variants = knowledge
        .vector_variant_report(artifact_id.as_str())
        .expect("variant report");
    assert!(
        variants
            .iter()
            .any(|v| v.model_key == model_key_a && v.dimensions == 2 && v.status == "ready"),
        "old dim=2 variant must remain inspectable: {variants:?}"
    );
    assert!(
        !variants.iter().any(|v| v.dimensions == 3),
        "no dim=3 variant before rebuild: {variants:?}"
    );

    // 重建路径：按新模型全量重建（settings 链语义）→ 不丢来源文件。
    knowledge
        .rebuild_notebook(notebook_id.as_str())
        .expect("rebuild enqueue");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let variants = knowledge
            .vector_variant_report(artifact_id.as_str())
            .expect("variant report");
        if variants
            .iter()
            .any(|v| v.dimensions == 3 && v.status == "ready")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "rebuild never produced a ready dim=3 variant: {variants:?}"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let result = knowledge
        .search_in_scope(&lingxi_service::knowledge::SearchRequest {
            scope_id: scope.id,
            session_id: "sess-a14".into(),
            query: "来源隔离与维度门".into(),
            channel: lingxi_service::knowledge::SearchChannel::Hybrid,
            notebook_ids: vec![],
            source_ids: vec![],
            section_keys: vec![],
            limit: 12,
        })
        .await
        .expect("search after rebuild");
    assert_eq!(result.mode, "hybrid", "rebuild restores the vector channel");
    assert!(result
        .searched_vector_variants
        .iter()
        .any(|v| v["dimensions"] == 3));
    // 重建不删源文件：快照身份与字节不变。
    let (status, sources) = server
        .request(
            "GET",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        sources["sources"][0]["snapshot"]["id"],
        snapshot_id.as_str()
    );
    server.stop().await;
}

#[tokio::test]
async fn settings_change_triggers_rebuild_and_variants_coexist() {
    let home = unique_dir("settings");
    let stub_addr = spawn_embedding_stub().await;
    let state = boot("settings", &home, Some(("embed-model-a", stub_addr))).await;
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";
    let (_, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "重建库"})),
        )
        .await;
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();
    let (_, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "pasted_text", "text": "分块配置变化触发全量重建，旧变体并建不覆盖。", "displayName": "重建源"})),
        )
        .await;
    let source_id = imported["source"]["id"]
        .as_str()
        .expect("source id")
        .to_string();
    wait_until_job(&server, &notebook_id, &source_id, "done").await;

    // 改 chunkTargetChars → 触发重建 job（rerank/topK 不触发）。
    let (status, _) = server
        .request(
            "PUT",
            &format!("{base}/notebooks/{notebook_id}/settings"),
            Some(json!({"chunkTargetChars": 300})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, jobs) = server
        .request(
            "GET",
            &format!("{base}/ingestion?notebookId={notebook_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let job_count = jobs["jobs"].as_array().expect("jobs").len();
    assert!(
        job_count >= 2,
        "settings change enqueues a rebuild job: {jobs}"
    );

    // rerank/topK 更新不触发重建。
    let (status, _) = server
        .request(
            "PUT",
            &format!("{base}/notebooks/{notebook_id}/settings"),
            Some(json!({"retrievalTopK": 5})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, jobs) = server
        .request(
            "GET",
            &format!("{base}/ingestion?notebookId={notebook_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        jobs["jobs"].as_array().expect("jobs").len(),
        job_count,
        "topK change must not rebuild"
    );

    // 重建 job 收尾（done / pending_embedding 终态）。
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (_, jobs) = server
            .request(
                "GET",
                &format!("{base}/ingestion?notebookId={notebook_id}"),
                None,
            )
            .await;
        let settled = jobs["jobs"]
            .as_array()
            .map(|js| {
                js.iter().all(|j| {
                    j["status"] == "done"
                        || j["status"] == "pending_embedding"
                        || j["status"] == "failed"
                })
            })
            .unwrap_or(false);
        if settled {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "rebuild never settled: {jobs}"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    server.stop().await;
}

#[tokio::test]
async fn reingest_membership_and_delete_semantics() {
    let home = unique_dir("semantics");
    let state = boot("semantics", &home, None).await;
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";
    let (_, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "语义库"})),
        )
        .await;
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();
    let (_, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "pasted_text", "text": "成员关系与删除语义验证。", "displayName": "语义源"})),
        )
        .await;
    let source_id = imported["source"]["id"]
        .as_str()
        .expect("source id")
        .to_string();

    // reingest：最新 job 非 failed → 409。
    wait_until_job(&server, &notebook_id, &source_id, "pending_embedding").await;
    let (status, err) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources/{source_id}/reingest"),
            None,
        )
        .await;
    assert_eq!(status, 409, "{err}");
    assert_eq!(err["error"], "KNOWLEDGE_CONFLICT");

    // membership remove ≠ 删除源：从笔记本移除后，源行仍可被显式读取
    //（物理删除只走 DELETE /sources/{id}）。
    let (status, removed) = server
        .request(
            "DELETE",
            &format!("{base}/notebooks/{notebook_id}/sources/{source_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{removed}");
    assert!(removed["membership"]["removedAt"].is_string());
    let (status, sources) = server
        .request(
            "GET",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        sources["sources"].as_array().map(|a| a.len()),
        Some(0),
        "membership is gone"
    );

    // DELETE sources/:id（delete wins）：取消活跃 job + 清理。
    let (status, _) = server
        .request("DELETE", &format!("{base}/sources/{source_id}"), None)
        .await;
    assert_eq!(status, 200, "hard delete succeeds");
    // 并发 reingest 在删除后显式失败（delete wins）。
    let (status, err) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources/{source_id}/reingest"),
            None,
        )
        .await;
    assert_eq!(status, 404, "{err}");

    server.stop().await;
}

#[tokio::test]
async fn source_delete_is_blocked_while_turn_scope_freezes_it() {
    let home = unique_dir("freeze");
    let state = boot("freeze", &home, None).await;
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";
    let (_, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "冻结库"})),
        )
        .await;
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();
    let (_, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "pasted_text", "text": "活跃轮冻结引用时删除被拒。", "displayName": "冻结源"})),
        )
        .await;
    let source_id = imported["source"]["id"]
        .as_str()
        .expect("source id")
        .to_string();

    // 建立活跃 turn scope（会话提交链的生产入口）。
    let (status, scope) = server
        .request(
            "POST",
            &format!("{base}/turn-scopes"),
            Some(json!({"notebookIds": [notebook_id], "sessionId": "sess-freeze", "turnId": "turn-1"})),
        )
        .await;
    assert_eq!(status, 201, "{scope}");
    let scope_id = scope["scope"]["id"].as_str().expect("scope id").to_string();

    // 活跃 turn 冻结引用时 409 拒绝硬删除。
    let (status, err) = server
        .request("DELETE", &format!("{base}/sources/{source_id}"), None)
        .await;
    assert_eq!(status, 409, "{err}");
    assert_eq!(err["error"], "KNOWLEDGE_CONFLICT");

    // 同会话新一轮 scope supersede 旧 scope（旧 scope 关闭）；新 scope 不含该
    // 源 → 冻结解除后删除放行。
    let (_, other) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "空库"})),
        )
        .await;
    let other_id = other["notebook"]["id"].as_str().expect("other").to_string();
    let (status, _) = server
        .request(
            "POST",
            &format!("{base}/turn-scopes"),
            Some(
                json!({"notebookIds": [other_id], "sessionId": "sess-freeze", "turnId": "turn-2"}),
            ),
        )
        .await;
    assert_eq!(status, 201);
    let (status, old_scope) = server
        .request("GET", &format!("{base}/turn-scopes/{scope_id}"), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(old_scope["scope"]["status"], "closed");

    let (status, _) = server
        .request("DELETE", &format!("{base}/sources/{source_id}"), None)
        .await;
    assert_eq!(status, 200);

    server.stop().await;
}

#[tokio::test]
async fn snapshot_content_serves_bytes_with_security_headers() {
    let home = unique_dir("snapshot");
    let state = boot("snapshot", &home, None).await;
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";
    let (_, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "快照库"})),
        )
        .await;
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();
    let html = "<html><body><h1>网页快照</h1><p>冻结原文只作证据。</p></body></html>";
    let (_, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "pasted_text", "text": html, "displayName": "网页快照"})),
        )
        .await;
    let snapshot_id = imported["snapshot"]["id"]
        .as_str()
        .expect("snapshot id")
        .to_string();
    let sha = imported["snapshot"]["sha256"]
        .as_str()
        .expect("sha")
        .to_string();

    let (status, headers, body) = server
        .request_raw(
            "GET",
            &format!("{base}/snapshots/{snapshot_id}/content"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    // HTML 以 text/plain 呈现 + CSP sandbox/nosniff + ETag=sha256（冻结网页原文
    // 只作证据字节保存，绝不在 Lingxi 自身源下执行其中脚本）。
    assert!(
        header("content-type").starts_with("text/plain"),
        "{}",
        header("content-type")
    );
    assert!(header("content-security-policy").contains("sandbox"));
    assert_eq!(header("x-content-type-options"), "nosniff");
    assert_eq!(header("etag"), format!("\"{sha}\""));
    assert_eq!(String::from_utf8_lossy(&body), html);

    // studio 内其他资源不可由此枚举：未知快照 404。
    let (status, _, _) = server
        .request_raw(
            "GET",
            &format!("{base}/snapshots/ksnap_missing/content"),
            None,
        )
        .await;
    assert_eq!(status, 404);

    server.stop().await;
}

#[tokio::test]
async fn citation_resolution_returns_block_snapshot_source_and_viewer() {
    let home = unique_dir("citation");
    let state = boot("citation", &home, None).await;
    let server_state = state.clone();
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";
    let (_, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "引用库"})),
        )
        .await;
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();
    let (_, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "pasted_text", "text": "引用定位验证段落。\n第二行原文。", "displayName": "引用源"})),
        )
        .await;
    let artifact_id = imported["parseArtifact"]["id"]
        .as_str()
        .expect("artifact id")
        .to_string();

    // 服务面创建引用（工具 readKnowledgeCitationPage 的同一入口），HTTP 解析。
    let knowledge = server_state.knowledge();
    let (status, blocks) = server
        .request(
            "GET",
            &format!("{base}/parse-artifacts/{artifact_id}/blocks"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let block_id = blocks["blocks"][0]["id"]
        .as_str()
        .expect("block id")
        .to_string();
    let citation = knowledge
        .create_citation(artifact_id.as_str(), block_id.as_str(), 0, 8)
        .expect("citation");
    let (status, resolved) = server
        .request("GET", &format!("{base}/citations/{}", citation.id), None)
        .await;
    assert_eq!(status, 200, "{resolved}");
    assert_eq!(resolved["citation"]["canonicalText"], "引用定位验证段落");
    assert_eq!(
        resolved["citation"]["canonicalTextSha256"],
        citation.canonical_text_sha256
    );
    assert_eq!(resolved["block"]["ordinal"], 0);
    assert_eq!(resolved["source"]["displayName"], "引用源");
    assert!(resolved["viewer"]["contentUrl"]
        .as_str()
        .expect("viewer url")
        .contains("/snapshots/"));
    // 引用能定位实际文本：块文本即原行（含句号），偏移 0..8 切出前 8 字符。
    assert_eq!(resolved["block"]["text"], "引用定位验证段落。");

    // 不存在的引用 → 404。
    let (status, err) = server
        .request("GET", &format!("{base}/citations/kcit_missing"), None)
        .await;
    assert_eq!(status, 404);
    assert_eq!(err["error"], "KNOWLEDGE_NOT_FOUND");

    server.stop().await;
}

#[tokio::test]
async fn file_import_security_rules() {
    let home = unique_dir("fileimport");
    let state = boot("fileimport", &home, None).await;
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";
    let (_, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name": "文件库"})),
        )
        .await;
    let notebook_id = created["notebook"]["id"].as_str().expect("id").to_string();

    // 相对路径 → 400。
    let (status, err) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "file", "filePath": "relative/path.txt", "displayName": "x"})),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(err["error"], "KNOWLEDGE_IMPORT_PATH_INVALID");

    // 不存在 → 404?（TS KNOWLEDGE_IMPORT_NOT_FOUND → errorStatus default 400；
    // routes L30-62：NOT_FOUND 不在 map 中 → 400）
    let (status, err) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(
                json!({"kind": "file", "filePath": "/nonexistent/r06t07.txt", "displayName": "x"}),
            ),
        )
        .await;
    assert_eq!(status, 400, "{err}");
    assert_eq!(err["error"], "KNOWLEDGE_IMPORT_NOT_FOUND");

    // 敏感路径（lingxiHome 内）→ 403 PATH_BLOCKED。
    let sensitive = home.join("knowledge").join("blocked.txt");
    std::fs::write(&sensitive, "inside home").expect("write");
    let (status, err) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "file", "filePath": sensitive.to_string_lossy(), "displayName": "x"})),
        )
        .await;
    assert_eq!(status, 403, "{err}");
    assert_eq!(err["error"], "KNOWLEDGE_IMPORT_PATH_BLOCKED");

    // 二进制内容伪装文本 → TYPE_UNSUPPORTED。
    let binary = unique_dir("fileimport-bin").join("binary.md");
    std::fs::write(&binary, [0x23_u8, 0x20, 0x00, 0x62]).expect("write binary");
    let (status, err) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(json!({"kind": "file", "filePath": binary.to_string_lossy(), "displayName": "x"})),
        )
        .await;
    assert_eq!(status, 422, "{err}");
    assert_eq!(err["error"], "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED");

    // 真实 .md 文件导入成功（file kind + markdown 解析）。
    let dir = unique_dir("fileimport-docs");
    let file = dir.join("doc.md");
    std::fs::write(&file, "# 标题\n\n文件导入正文。").expect("write md");
    let (status, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/sources"),
            Some(
                json!({"kind": "file", "filePath": file.to_string_lossy(), "displayName": "文档"}),
            ),
        )
        .await;
    assert_eq!(status, 201, "{imported}");
    assert_eq!(imported["parseArtifact"]["status"], "ready");

    // 目录导入：递归展开 + sha 去重（第二个相同文件被 skip）。
    std::fs::write(dir.join("doc2.md"), "另一个文件。").expect("write md2");
    let (status, dir_import) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{notebook_id}/import-directory"),
            Some(json!({"dirPath": dir.to_string_lossy()})),
        )
        .await;
    assert_eq!(status, 201, "{dir_import}");
    assert!(dir_import["imported"].as_array().expect("imported").len() >= 2);
    assert!(dir_import["skipped"]
        .as_array()
        .expect("skipped")
        .is_empty());

    server.stop().await;
}

#[tokio::test]
async fn research_engine_tables_are_not_restored() {
    let home = unique_dir("noresearch");
    let state = boot("noresearch", &home, None).await;
    let knowledge = state.knowledge();
    // 兼容残留叶：退役的独立知识研究引擎不恢复——新库不建 research/analysis/
    // coverage 表；旧 research 表的存在不构成恢复理由。
    assert!(
        !knowledge.has_table("research_runs"),
        "the retired research engine must not be restored"
    );
    assert!(!knowledge.has_table("analysis_manifests"));
    assert!(!knowledge.has_table("coverage_runs"));
    assert!(!knowledge.has_table("knowledge_runs"));
    assert!(knowledge.has_table("notebooks"));
    assert!(knowledge.has_table("knowledge_turn_scopes"));
}
