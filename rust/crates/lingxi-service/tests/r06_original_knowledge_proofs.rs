//! 原F-D08逐断言补充证明；仅使用合成目录、本机模型替身和真实认证/存储/工具链。
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
async fn embedding_stub_response(axum::Json(parsed): axum::Json<Value>) -> axum::Json<Value> {
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
    let usage = json!({ "prompt_tokens": inputs.len() * 5, "total_tokens": inputs.len() * 5 });
    axum::Json(json!({"data": data, "usage": usage, "model": model}))
}

async fn spawn_embedding_stub() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("stub bind");
    let addr = listener.local_addr().expect("stub addr");
    // HTTP请求体完整到达就回应，不能等待客户端关闭连接才发送响应。
    let router = axum::Router::new().route(
        "/v1/embeddings",
        axum::routing::post(embedding_stub_response),
    );
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("embedding fixture HTTP服务");
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

#[tokio::test]
async fn original_notebook_source_routes_reject_without_changing_rows() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-http-rows");
    let state = boot("original-http-rows", &home, None).await;
    let studio = state.knowledge().studio_id().to_string();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let server = serve(state).await;
    let base = "/lingxi/v1/knowledge";
    let (status, created) = server
        .request(
            "POST",
            &format!("{base}/notebooks"),
            Some(json!({"name":"原义笔记本"})),
        )
        .await;
    assert_eq!(status, 201);
    let id = created["notebook"]["id"].as_str().unwrap();
    assert_eq!(facts.get_notebook(&studio, id).unwrap().name, "原义笔记本");
    let before = facts.list_notebooks(&studio).unwrap();
    for body in [
        json!({"name":""}),
        json!({"name":"valid","forgedStudio":"foreign"}),
        json!({"name":4}),
    ] {
        let (status, error) = server
            .request("POST", &format!("{base}/notebooks"), Some(body))
            .await;
        assert_eq!(status, 400, "{error}");
        assert_eq!(
            facts.list_notebooks(&studio).unwrap(),
            before,
            "拒绝不能新增笔记本"
        );
    }
    let (status, renamed) = server
        .request(
            "PATCH",
            &format!("{base}/notebooks/{id}"),
            Some(json!({"name":"已改名"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(renamed["notebook"]["name"], "已改名");
    assert_eq!(facts.get_notebook(&studio, id).unwrap().name, "已改名");
    let unchanged = facts.get_notebook(&studio, id).unwrap();
    let (status, _) = server
        .request(
            "PATCH",
            &format!("{base}/notebooks/{id}"),
            Some(json!({"name":""})),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(facts.get_notebook(&studio, id).unwrap(), unchanged);
    let (status, one) = server
        .request("GET", &format!("{base}/notebooks/{id}"), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(one["notebook"]["name"], "已改名");
    let (status, missing) = server
        .request("GET", &format!("{base}/notebooks/knb_missing"), None)
        .await;
    assert_eq!(status, 404);
    assert_eq!(missing["error"], "KNOWLEDGE_NOT_FOUND");
    let (status, imported) = server
        .request(
            "POST",
            &format!("{base}/notebooks/{id}/sources"),
            Some(json!({"kind":"pasted_text","text":NOTE_TEXT_A,"displayName":"原义来源"})),
        )
        .await;
    assert_eq!(status, 201);
    let source = imported["source"]["id"].as_str().unwrap();
    assert_eq!(facts.get_source(source).unwrap().studio_id, studio);
    assert_eq!(
        facts.latest_snapshot_for_source(source).unwrap().id,
        imported["snapshot"]["id"]
    );
    assert!(
        facts.latest_job_for_source(id, source).unwrap().is_some(),
        "成功导入必须真实入队"
    );
    let snapshot = facts.latest_snapshot_for_source(source).unwrap();
    let (status, listed) = server
        .request("GET", &format!("{base}/notebooks/{id}/sources"), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(listed["sources"].as_array().unwrap().len(), 1);
    assert_eq!(listed["sources"][0]["source"]["id"], source);
    assert_eq!(
        facts.latest_snapshot_for_source(source).unwrap(),
        snapshot,
        "列表读取不再导入"
    );
    for body in [
        json!({"kind":"bad","text":"bad"}),
        json!({"kind":"pasted_text","text":""}),
        json!({"kind":"file","filePath":"relative.txt"}),
    ] {
        let (status, error) = server
            .request(
                "POST",
                &format!("{base}/notebooks/{id}/sources"),
                Some(body),
            )
            .await;
        assert_eq!(status, 400, "{error}");
        assert_eq!(facts.list_notebook_sources(id).unwrap().len(), 1);
        assert_eq!(facts.latest_snapshot_for_source(source).unwrap(), snapshot);
    }
    let foreign = facts
        .create_notebook("foreign-studio", "不披露的笔记本")
        .unwrap();
    let foreign_before = facts.get_notebook("foreign-studio", &foreign.id).unwrap();
    for (method, suffix, body) in [
        ("GET", "", None),
        ("GET", "/sources", None),
        ("PATCH", "", Some(json!({"name":"越权修改"}))),
        (
            "POST",
            "/sources",
            Some(json!({"kind":"pasted_text","text":"越权导入"})),
        ),
        ("DELETE", "", None),
    ] {
        let (status, error) = server
            .request(
                method,
                &format!("{base}/notebooks/{}{suffix}", foreign.id),
                body,
            )
            .await;
        assert_eq!(status, 404, "{error}");
        assert_eq!(
            facts.get_notebook("foreign-studio", &foreign.id).unwrap(),
            foreign_before
        );
        assert!(facts.list_notebook_sources(&foreign.id).unwrap().is_empty());
    }
    let (status, list) = server
        .request("GET", &format!("{base}/notebooks"), None)
        .await;
    assert_eq!(status, 200);
    let entry = list["notebooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .unwrap();
    assert!(entry["config"].is_object());
    assert!(entry["chunkTargetCharsEffective"].is_u64());
    assert_eq!(entry["sourceCount"], 1);
    assert!(entry["ingestion"].is_object());
    assert!(!list.to_string().contains("不披露的笔记本"));
    assert_eq!(facts.list_notebook_sources(id).unwrap().len(), 1);
    server.stop().await;
}

#[tokio::test]
async fn original_artifact_citation_snapshot_boundaries_and_heading_fallback() {
    use lingxi_adapters::storage::knowledge::{
        BlockRow, ChunkDraft, ChunkSpan, KnowledgeFactStore, KnowledgeIndexStore, SectionMetaRow,
    };
    let home = unique_dir("original-read-boundaries");
    let state = boot("original-read-boundaries", &home, None).await;
    let knowledge = state.knowledge().clone();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let server = serve(state).await;
    let notebook = knowledge.create_notebook("原文范围").unwrap();
    let id = notebook["id"].as_str().unwrap();
    let html = "<html><script>window.FOREIGN_SHOULD_NOT_RUN=1</script><p>原文</p></html>";
    let imported = knowledge
        .import_pasted_text(id, html, Some("安全HTML原文"))
        .unwrap();
    let source = imported["source"]["id"].as_str().unwrap();
    let snapshot = imported["snapshot"]["id"].as_str().unwrap();
    let artifact = imported["parseArtifact"]["id"].as_str().unwrap();
    wait_until_job(&server, id, source, "pending_embedding").await;
    let (_, headers, bytes) = server
        .request_raw(
            "GET",
            &format!("/lingxi/v1/knowledge/snapshots/{snapshot}/content"),
            None,
        )
        .await;
    assert_eq!(bytes, html.as_bytes());
    let header = |key| {
        headers
            .iter()
            .find(|(name, _)| name == key)
            .unwrap()
            .1
            .as_str()
    };
    assert!(header("content-type").starts_with("text/plain"));
    assert!(header("content-security-policy").contains("sandbox"));
    assert_eq!(header("x-content-type-options"), "nosniff");
    let (status, cards) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/parse-artifacts/{artifact}/chunks"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(cards["chunkerConfigId"].as_str().unwrap().len(), 16);
    for (i, chunk) in cards["chunks"].as_array().unwrap().iter().enumerate() {
        assert_eq!(chunk["ordinal"], i + 1, "全部卡片必须有序");
    }
    let (status, blocks) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/parse-artifacts/{artifact}/blocks"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert!(blocks["blocks"].as_array().unwrap().iter().any(|b| {
        b["text"]
            .as_str()
            .unwrap()
            .contains("FOREIGN_SHOULD_NOT_RUN")
    }));
    let block = &blocks["blocks"][0];
    let text = block["text"].as_str().unwrap();
    assert!(block["locator"]["headingPath"]
        .as_array()
        .is_none_or(Vec::is_empty));
    let index = KnowledgeIndexStore::open(&home.join("knowledge")).unwrap();
    index
        .replace_artifact_chunks(
            artifact,
            "original-heading-proof",
            "original-heading-proof",
            &[ChunkDraft {
                id: "chunk_original_heading".into(),
                ordinal: 0,
                section_id: Some("section_original_heading".into()),
                text: text.into(),
                token_count: 1,
                spans: vec![ChunkSpan {
                    block_id: block["id"].as_str().unwrap().into(),
                    block_start_offset: 0,
                    block_end_offset: text.chars().count(),
                    chunk_start_offset: 0,
                    chunk_end_offset: text.chars().count(),
                }],
            }],
            &[SectionMetaRow {
                id: "section_original_heading".into(),
                section_ordinal: 0,
                heading_path: vec!["补齐的真实章节元数据".into()],
                start_block_ordinal: 0,
                end_block_ordinal: 0,
            }],
        )
        .unwrap();
    let citation = knowledge
        .create_citation(
            artifact,
            block["id"].as_str().unwrap(),
            0,
            text.chars().count(),
        )
        .unwrap();
    let (status, resolved) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/citations/{}", citation.id),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(resolved["citation"]["canonicalText"], text);
    assert_eq!(resolved["source"]["id"], source);
    assert_eq!(resolved["snapshot"]["id"], snapshot);
    assert_eq!(
        resolved["viewer"]["locator"]["headingPath"],
        json!(["补齐的真实章节元数据"])
    );
    assert_eq!(
        resolved["viewer"]["contentUrl"],
        format!("/lingxi/v1/knowledge/snapshots/{snapshot}/content")
    );
    let (foreign, foreign_snapshot) = facts
        .create_source_with_snapshot(
            "foreign-studio",
            "pasted_text",
            "FOREIGN_SECRET_SOURCE",
            &json!({}),
            b"FOREIGN_SECRET_BYTES",
            "text/plain",
        )
        .unwrap();
    facts
        .begin_parse_artifact(
            "foreign-proof-artifact",
            &foreign_snapshot.id,
            "lingxi-citation",
            "1",
            "foreign-proof",
        )
        .unwrap();
    facts
        .complete_parse_artifact(
            "foreign-proof-artifact",
            "ready",
            &[],
            "citation_grade",
            None,
        )
        .unwrap();
    facts
        .insert_blocks(&[BlockRow {
            id: "foreign-proof-block".into(),
            parse_artifact_id: "foreign-proof-artifact".into(),
            ordinal: 0,
            text: "FOREIGN_SECRET_BYTES".into(),
            text_sha256: lingxi_kernel::knowledge::sha256_hex(b"FOREIGN_SECRET_BYTES"),
            locator_type: "text".into(),
            locator: json!({}),
        }])
        .unwrap();
    let foreign_citation = facts
        .create_citation("foreign-proof-artifact", "foreign-proof-block", 0, 20)
        .unwrap();
    for path in [
        format!("snapshots/{}/content", foreign_snapshot.id),
        format!("citations/{}", foreign_citation.id),
        "parse-artifacts/foreign-proof-artifact/blocks".into(),
        "parse-artifacts/foreign-proof-artifact/chunks".into(),
        "parse-artifacts/missing/blocks".into(),
        "parse-artifacts/missing/chunks".into(),
        "citations/missing".into(),
    ] {
        let (status, _, body) = server
            .request_raw("GET", &format!("/lingxi/v1/knowledge/{path}"), None)
            .await;
        assert_eq!(status, 404, "{path}");
        assert!(!String::from_utf8_lossy(&body).contains("FOREIGN_SECRET"));
    }
    assert_eq!(
        facts.get_source(&foreign.id).unwrap().display_name,
        "FOREIGN_SECRET_SOURCE"
    );
    assert_eq!(
        facts.read_snapshot_bytes(&foreign_snapshot).unwrap(),
        b"FOREIGN_SECRET_BYTES"
    );
    assert_eq!(facts.get_source(source).unwrap().id, source);
    server.stop().await;
}

#[tokio::test]
async fn original_settings_refresh_remove_reingest_and_purge_use_real_facts() {
    use lingxi_adapters::storage::knowledge::{KnowledgeFactStore, KnowledgeIndexStore};
    let home = unique_dir("original-write-facts");
    let stub = spawn_embedding_stub().await;
    let state = boot("original-write-facts", &home, Some(("embed-model-a", stub))).await;
    let knowledge = state.knowledge().clone();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let notebook = knowledge.create_notebook("原义写操作").unwrap();
    let id = notebook["id"].as_str().unwrap();
    let a = knowledge
        .import_pasted_text(id, NOTE_TEXT_A, Some("共享来源"))
        .unwrap();
    let source = a["source"]["id"].as_str().unwrap();
    let second = knowledge.create_notebook("共享笔记本").unwrap();
    let second_id = second["id"].as_str().unwrap();
    facts.add_membership(second_id, source).unwrap();
    let b = knowledge
        .import_pasted_text(id, "另一实际来源和独立摄入任务。", Some("第二来源"))
        .unwrap();
    let source_b = b["source"]["id"].as_str().unwrap();
    let server = serve(state.clone()).await;
    wait_until_job(&server, id, source, "done").await;
    wait_until_job(&server, id, source_b, "done").await;
    let before = facts.get_notebook_config(id).unwrap();
    let jobs_before = facts
        .list_ingestion_jobs(Some(id), None, 200)
        .unwrap()
        .len();
    for body in [
        json!({"chunkTargetChars":1}),
        json!({"retrievalTopK":1001}),
        json!({"vectorRetentionDays":0}),
        json!({"embeddingModelRef":{"id":"","provider":"embed"}}),
        json!({"unknown":true}),
        json!({"chunkTargetChars":"bad"}),
    ] {
        let (status, error) = server
            .request(
                "PUT",
                &format!("/lingxi/v1/knowledge/notebooks/{id}/settings"),
                Some(body),
            )
            .await;
        assert_eq!(status, 400, "{error}");
        assert_eq!(facts.get_notebook_config(id).unwrap(), before);
        assert_eq!(
            facts
                .list_ingestion_jobs(Some(id), None, 200)
                .unwrap()
                .len(),
            jobs_before
        );
    }
    for body in [
        json!({"chunkTargetChars":300}),
        json!({"embeddingModelRef":{"id":"embed-model-b","provider":"embed"}}),
    ] {
        let before = facts.list_ingestion_jobs(Some(id), None, 200).unwrap();
        let (status, changed) = server
            .request(
                "PUT",
                &format!("/lingxi/v1/knowledge/notebooks/{id}/settings"),
                Some(body),
            )
            .await;
        assert_eq!(status, 200, "{changed}");
        let after = facts.list_ingestion_jobs(Some(id), None, 200).unwrap();
        let new = after
            .iter()
            .filter(|job| !before.iter().any(|old| old.id == job.id))
            .collect::<Vec<_>>();
        assert_eq!(new.len(), 2, "分块或嵌入变化必须为全部来源重建");
        assert!(new.iter().any(|job| job.source_id == source));
        assert!(new.iter().any(|job| job.source_id == source_b));
    }
    let snap = facts.latest_snapshot_for_source(source).unwrap();
    let (status, removed) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/notebooks/{id}/sources/{source}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{removed}");
    assert!(removed["membership"]["removedAt"].is_string());
    assert!(facts
        .get_membership(second_id, source)
        .unwrap()
        .removed_at
        .is_none());
    assert_eq!(facts.get_source(source).unwrap().id, source);
    assert_eq!(
        facts.read_snapshot_bytes(&snap).unwrap(),
        NOTE_TEXT_A.as_bytes()
    );
    let (status, _) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/notebooks/{id}/sources/foreign-missing"),
            None,
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(
        facts.read_snapshot_bytes(&snap).unwrap(),
        NOTE_TEXT_A.as_bytes()
    );
    // 文件刷新：实际磁盘未变不新增版本，变更才重建，非local owner不能写。
    let dir = unique_dir("original-local-file");
    let file = dir.join("proof.md");
    std::fs::write(&file, "# 原章节\n\n原始文件正文。").unwrap();
    let original = knowledge
        .import_file(second_id, file.to_str().unwrap(), Some("刷新文件"))
        .unwrap();
    let old_source = original["source"]["id"].as_str().unwrap();
    let (status, same) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{second_id}/sources/{old_source}/refresh"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(same["changed"], false);
    assert_eq!(same["snapshot"]["id"], original["snapshot"]["id"]);
    std::fs::write(&file, "# 新章节\n\n已改变的真实文件正文。").unwrap();
    let (status, refreshed) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{second_id}/sources/{old_source}/refresh"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{refreshed}");
    assert_eq!(refreshed["changed"], true);
    assert_ne!(refreshed["snapshot"]["id"], original["snapshot"]["id"]);
    assert_eq!(refreshed["parseArtifact"]["status"], "ready");
    let refreshed_source = refreshed["source"]["id"].as_str().unwrap();
    assert!(facts
        .latest_job_for_source(second_id, refreshed_source)
        .unwrap()
        .is_some());
    let retained = facts.latest_snapshot_for_source(refreshed_source).unwrap();
    let credential = state
        .auth()
        .issue_device_credential("user_local", &[], None)
        .unwrap();
    let mut server = server;
    let local_token = std::mem::replace(&mut server.token, credential.secret);
    let (status, _) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{second_id}/sources/{refreshed_source}/refresh"
            ),
            None,
        )
        .await;
    assert_eq!(status, 403, "已认证设备也不能冒充本地主人刷新");
    server.token = local_token;
    std::fs::remove_file(&file).unwrap();
    let (status, error) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{second_id}/sources/{refreshed_source}/refresh"
            ),
            None,
        )
        .await;
    assert_eq!(status, 400, "{error}");
    assert_eq!(
        facts.latest_snapshot_for_source(refreshed_source).unwrap(),
        retained
    );
    // 永久删除清真实快照和索引；删除后重新摄入不得复活源。
    wait_until_job(&server, second_id, refreshed_source, "done").await;
    let artifact = refreshed["parseArtifact"]["id"].as_str().unwrap();
    let index = KnowledgeIndexStore::open(&home.join("knowledge")).unwrap();
    assert!(!index
        .list_ready_variant_metadata(artifact)
        .unwrap()
        .is_empty());
    let (status, purge) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/sources/{refreshed_source}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{purge}");
    assert!(purge["cancelledJobs"].is_u64());
    assert_eq!(purge["purge"]["snapshots"], true);
    assert_eq!(purge["purge"]["indexVariants"], true);
    assert!(facts.get_source(refreshed_source).is_err());
    assert!(!home.join("knowledge").join(&retained.storage_path).exists());
    assert!(index
        .list_ready_variant_metadata(artifact)
        .unwrap()
        .is_empty());
    let (status, _) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{second_id}/sources/{refreshed_source}/reingest"
            ),
            None,
        )
        .await;
    assert_eq!(status, 404);
    server.stop().await;
}

#[tokio::test]
async fn original_failed_and_untracked_jobs_retry_and_delete_wins() {
    use lingxi_adapters::storage::knowledge::{KnowledgeFactStore, KnowledgeIndexStore};
    let home = unique_dir("original-retry-jobs");
    let state = boot("original-retry-jobs", &home, None).await;
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let notebook = state.knowledge().create_notebook("重试原义").unwrap();
    let id = notebook["id"].as_str().unwrap();
    let imported = state
        .knowledge()
        .import_pasted_text(id, "实际保存的失败任务原文。", Some("失败任务"))
        .unwrap();
    let source = imported["source"]["id"].as_str().unwrap();
    // 只准备已有failed状态；重试与唤醒仍经实际HTTP和后台worker执行。
    let claimed = facts.claim_next_job().unwrap().unwrap();
    assert_eq!(claimed.source_id, source);
    facts.fail_job(&claimed.id, "合成的先前索引失败").unwrap();
    let (untracked, snapshot) = facts
        .create_source_with_snapshot(
            state.knowledge().studio_id(),
            "pasted_text",
            "未跟踪任务",
            &json!({}),
            b"Original untracked source body.",
            "text/plain",
        )
        .unwrap();
    facts.add_membership(id, &untracked.id).unwrap();
    assert!(facts
        .latest_job_for_source(id, &untracked.id)
        .unwrap()
        .is_none());
    let server = serve(state.clone()).await;
    let (status, retried) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{id}/sources/{source}/reingest"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{retried}");
    assert_eq!(retried["retried"], true);
    assert_eq!(retried["job"]["id"], claimed.id);
    let settled = wait_until_job(&server, id, source, "pending_embedding").await;
    assert_eq!(settled["id"], claimed.id);
    let (status, created) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{id}/sources/{}/reingest",
                untracked.id
            ),
            None,
        )
        .await;
    assert_eq!(status, 200, "{created}");
    assert_eq!(created["retried"], false);
    assert!(created["job"]["id"].is_string());
    wait_until_job(&server, id, &untracked.id, "pending_embedding").await;
    let unchanged = facts
        .latest_job_for_source(id, &untracked.id)
        .unwrap()
        .unwrap();
    let (status, error) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{id}/sources/{}/reingest",
                untracked.id
            ),
            None,
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(error["error"], "KNOWLEDGE_CONFLICT");
    assert_eq!(
        facts
            .latest_job_for_source(id, &untracked.id)
            .unwrap()
            .unwrap(),
        unchanged
    );
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/knowledge/turn-scopes",
            Some(json!({"notebookIds":[id],"sessionId":"original-delete-freeze","turnId":"one"})),
        )
        .await;
    assert_eq!(status, 201);
    let (status, error) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/sources/{}", untracked.id),
            None,
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(error["error"], "KNOWLEDGE_CONFLICT");
    assert_eq!(
        facts.read_snapshot_bytes(&snapshot).unwrap(),
        b"Original untracked source body."
    );
    let empty = state.knowledge().create_notebook("结束冻结").unwrap();
    let (status,_)=server.request("POST","/lingxi/v1/knowledge/turn-scopes",Some(json!({"notebookIds":[empty["id"]],"sessionId":"original-delete-freeze","turnId":"two"}))).await;
    assert_eq!(status, 201);
    let artifact = facts
        .latest_parse_artifact_for_snapshot(&snapshot.id)
        .unwrap()
        .unwrap();
    let (status, purge) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/sources/{}", untracked.id),
            None,
        )
        .await;
    assert_eq!(status, 200, "{purge}");
    assert!(
        purge["cancelledJobs"].as_u64().unwrap() >= 1,
        "pending任务必须取消"
    );
    assert!(facts.get_source(&untracked.id).is_err());
    assert!(!home.join("knowledge").join(&snapshot.storage_path).exists());
    assert!(KnowledgeIndexStore::open(&home.join("knowledge"))
        .unwrap()
        .list_ready_variant_metadata(&artifact.id)
        .unwrap()
        .is_empty());
    let (status, _) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{id}/sources/{}/reingest",
                untracked.id
            ),
            None,
        )
        .await;
    assert_eq!(status, 404);
    assert!(
        facts.get_source(&untracked.id).is_err(),
        "重新摄入不能复活已删来源"
    );
    server.stop().await;
}

#[tokio::test]
async fn original_directory_import_reports_each_effect_and_rejects_invalid_root() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-directory-import");
    let state = boot("original-directory-import", &home, None).await;
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let notebook = state.knowledge().create_notebook("目录原义").unwrap();
    let id = notebook["id"].as_str().unwrap();
    let dir = unique_dir("original-directory-documents");
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    std::fs::write(dir.join("first.md"), "# 目录一\n\n真实目录正文一。").unwrap();
    std::fs::write(dir.join("copy.md"), "# 目录一\n\n真实目录正文一。").unwrap();
    std::fs::write(dir.join("nested/second.txt"), "真实目录正文二。").unwrap();
    std::fs::write(dir.join("bad.md"), b"binary\0must-not-import").unwrap();
    let server = serve(state).await;
    let route = format!("/lingxi/v1/knowledge/notebooks/{id}/import-directory");
    let (status, report) = server
        .request("POST", &route, Some(json!({"dirPath":dir})))
        .await;
    assert_eq!(status, 201, "{report}");
    assert_eq!(report["imported"].as_array().unwrap().len(), 2);
    assert_eq!(report["skipped"].as_array().unwrap().len(), 1);
    assert_eq!(report["skipped"][0]["reason"], "duplicate_content");
    assert_eq!(report["failed"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["failed"][0]["error"],
        "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED"
    );
    for imported in report["imported"].as_array().unwrap() {
        let source = imported["sourceId"].as_str().unwrap();
        assert_eq!(imported["ingestion"], "enqueued");
        assert!(facts.get_source(source).is_ok());
        assert!(facts.latest_job_for_source(id, source).unwrap().is_some());
        wait_until_job(&server, id, source, "pending_embedding").await;
    }
    let before = facts
        .list_notebook_sources(id)
        .unwrap()
        .iter()
        .map(|entry| entry.source.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(before.len(), 2, "失败文件不能伪装成导入成功");
    for body in [
        json!({"dirPath":"relative-dir"}),
        json!({"dirPath":home.join("knowledge")}),
        json!({}),
    ] {
        let (status, error) = server.request("POST", &route, Some(body)).await;
        assert!(status == 400 || status == 403, "{error}");
        assert_eq!(
            facts
                .list_notebook_sources(id)
                .unwrap()
                .iter()
                .map(|entry| entry.source.id.clone())
                .collect::<Vec<_>>(),
            before
        );
    }
    server.stop().await;
}

async fn boot_chat(home: &Path, endpoint: &str) -> ServiceState {
    boot_chat_with_embedding(home, endpoint, false).await
}

async fn boot_chat_with_embedding(home: &Path, endpoint: &str, embedding: bool) -> ServiceState {
    let layout = prepare_layout(home).unwrap();
    let config_path = home.join("synthetic-service.json");
    let mut configuration = json!({"home":home,"providers":{"main":{"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"F13_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"main","model":"synthetic","capabilities":{"tools":true},"compat":{"contextWindow":1000000}}}});
    if embedding {
        configuration["providers"]["embed"] = json!({
            "protocol":"openai-embeddings",
            "endpoint":endpoint,
            "auth":{"kind":"apiKey","apiKey":"F13_SYNTHETIC_EMBED_ONLY"}
        });
        configuration["models"]["embedding"] = json!({"provider":"embed","model":"synthetic"});
    }
    std::fs::write(&config_path, configuration.to_string()).unwrap();
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .unwrap()
            .unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.to_path_buf(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        },
        &layout,
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            model_plane_source: Some(source),
            credential_service: Some(credentials),
            workspace_root: Some(home.to_path_buf()),
            // 本证明套件的单会话场景驱动 17+ 轮工具调用（原义务的逐案探针），
            // 超过默认 32 轮预算：按场景显式放宽（预算语义本身不变，越界仍响亮失败）。
            run_limits: lingxi_service::runs::RunDriveLimits {
                max_model_turns: 64,
                ..lingxi_service::runs::RunDriveLimits::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap()
}

#[derive(Default)]
struct KnowledgeProofScript {
    calls: Vec<(String, Value)>,
    requests: Vec<Value>,
    before_read: Option<Arc<dyn Fn() + Send + Sync>>,
    before_no_index: Option<Arc<dyn Fn() + Send + Sync>>,
}
fn proof_content(message: &Value) -> String {
    message["content"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            message["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
}
fn proof_selection(body: &Value) -> Value {
    let system = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "system")
        .map(proof_content)
        .collect::<Vec<_>>()
        .join("\n");
    let tail = system
        .split_once("本轮知识资料范围：")
        .expect("范围来自真实认证提交")
        .1;
    serde_json::Deserializer::from_str(tail)
        .into_iter::<Value>()
        .next()
        .unwrap()
        .unwrap()
}
async fn knowledge_proof_provider(
    axum::extract::State(script): axum::extract::State<Arc<std::sync::Mutex<KnowledgeProofScript>>>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let selection = proof_selection(&body);
    let (step, call, callback, no_index) = {
        let mut script = script.lock().unwrap();
        let step = script.requests.len();
        let call = script.calls.get(step / 2).cloned();
        let callback = script.before_read.clone();
        script.requests.push(body.clone());
        (step, call, callback, script.before_no_index.clone())
    };
    let delta = if let Some((tool, mut args)) = call {
        let (name, arguments) = if step % 2 == 0 {
            ("mcp_describe_tool", json!({"name":tool}))
        } else {
            if args.get("scopeId").is_some() {
                args["scopeId"] = selection["scopeId"].clone();
            }
            if tool == "knowledge_read" && step / 2 == 7 {
                callback.expect("冻结读取前刷新外部文件")();
            }
            if tool == "knowledge_search" && step / 2 == 11 {
                no_index.expect("原文仍在但索引缓存缺失的边界")();
            }
            ("mcp_call", json!({"tool":tool,"arguments":args}))
        };
        json!({"role":"assistant","tool_calls":[{"index":0,"id":if step%2==0{format!("original-description-{}",step/2)}else{format!("original-result-{}",step/2)},"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]})
    } else {
        json!({"role":"assistant","content":"原知识行为证明完成"})
    };
    let tools = delta.get("tool_calls").is_some();
    let frames = [
        json!({"id":format!("original-knowledge-{step}"),"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
        json!({"id":format!("original-knowledge-{step}"),"choices":[{"index":0,"delta":{},"finish_reason":if tools{"tool_calls"}else{"stop"}}]}),
        json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":4}}),
    ];
    let stream = frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n";
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        stream,
    )
        .into_response()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_tools_use_authentic_run_scope_and_frozen_citable_source() {
    use lingxi_kernel::ports::StoragePort;
    use std::future::IntoFuture;
    let home = unique_dir("original-tools");
    let docs = unique_dir("original-tool-document");
    let file = docs.join("proof.md");
    let old = format!(
        "# 原章节\n\nORIGINAL_PROOF 柠檬酸含量为百分之七。{}",
        "原始上下文材料。".repeat(100)
    );
    std::fs::write(&file, &old).unwrap();
    let script = Arc::new(std::sync::Mutex::new(KnowledgeProofScript::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let upstream = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route(
                    "/chat/completions",
                    axum::routing::post(knowledge_proof_provider),
                )
                .with_state(script.clone()),
        )
        .into_future(),
    );
    let state = boot_chat(&home, &endpoint).await;
    let knowledge = state.knowledge().clone();
    let nb = knowledge.create_notebook("只授权甲").unwrap();
    let id = nb["id"].as_str().unwrap();
    let a = knowledge
        .import_file(id, file.to_str().unwrap(), Some("甲原文"))
        .unwrap();
    let src = a["source"]["id"].as_str().unwrap();
    let server = serve(state.clone()).await;
    // 甲的真实入库稳定后再通过正式API创建乙，避免测试准备与后台写入争用SQLite。
    wait_until_job(&server, id, src, "pending_embedding").await;
    let foreign_knowledge =
        lingxi_service::knowledge::KnowledgeService::new(&home, "foreign-studio".into()).unwrap();
    let other = foreign_knowledge.create_notebook("未授权乙").unwrap();
    let other_id = other["id"].as_str().unwrap();
    let b = foreign_knowledge
        .import_pasted_text(other_id, "FOREIGN_B 柠檬酸含量为百分之九。", Some("乙原文"))
        .unwrap();
    let foreign = b["source"]["id"].as_str().unwrap();
    let (refresh_file, refresh_notebook, refresh_source) =
        (file.clone(), id.to_string(), src.to_string());
    script.lock().unwrap().before_read = Some(Arc::new(move || {
        std::fs::write(&refresh_file, "# 新章节\n\nNEW_VERSION_SHOULD_NOT_BE_READ").unwrap();
        assert_eq!(
            knowledge
                .refresh_file_source(&refresh_notebook, &refresh_source)
                .unwrap()["changed"],
            true
        );
    }));
    let (index_root, original_artifact) = (
        home.join("knowledge"),
        a["parseArtifact"]["id"].as_str().unwrap().to_string(),
    );
    script.lock().unwrap().before_no_index = Some(Arc::new(move || {
        lingxi_adapters::storage::knowledge::KnowledgeIndexStore::open(&index_root)
            .unwrap()
            .remove_artifact(&original_artifact)
            .unwrap();
    }));
    // 无结果夹具使用独有汉字词，并按生产分词确认每个词都不在真实输入和标题中。
    let no_match_query = "氙铷锆铪";
    let no_match_terms = lingxi_adapters::storage::knowledge::tokenize_search_text(no_match_query);
    assert!(!no_match_terms.is_empty());
    for term in &no_match_terms {
        for corpus in [
            old.as_str(),
            "FOREIGN_B 柠檬酸含量为百分之九。",
            "甲原文",
            "乙原文",
            "只授权甲",
            "未授权乙",
        ] {
            assert!(
                !lingxi_adapters::storage::knowledge::tokenize_search_text(corpus).contains(term),
                "无结果测试词不能匹配准备的原文或标题：{term}"
            );
        }
    }
    script.lock().unwrap().calls = vec![
        (
            "knowledge_search".into(),
            json!({"scopeId":"runtime","query":"柠檬酸含量","channel":"fts"}),
        ),
        (
            "knowledge_search".into(),
            json!({"scopeId":"runtime","query":"柠檬酸含量","channel":"hybrid"}),
        ),
        (
            "knowledge_search".into(),
            json!({"scopeId":"runtime","query":no_match_query,"channel":"fts"}),
        ),
        (
            "knowledge_search".into(),
            json!({"scopeId":"runtime","query":"柠檬酸含量","sourceIds":[foreign],"channel":"fts"}),
        ),
        (
            "knowledge_grep".into(),
            json!({"scopeId":"runtime","pattern":"柠檬酸含量"}),
        ),
        (
            "knowledge_grep".into(),
            json!({"scopeId":"runtime","pattern":"NO_LITERAL_MATCH_1234"}),
        ),
        ("knowledge_outline".into(), json!({"scopeId":"runtime"})),
        (
            "knowledge_read".into(),
            json!({"scopeId":"runtime","sourceId":src,"maxChars":256}),
        ),
        (
            "knowledge_read".into(),
            json!({"scopeId":"runtime","sourceId":foreign}),
        ),
        (
            "knowledge_read".into(),
            json!({"scopeId":"runtime","sourceId":"source-missing"}),
        ),
        (
            "knowledge_read".into(),
            json!({"scopeId":"runtime","sourceId":src,"offset":999999}),
        ),
        (
            "knowledge_search".into(),
            json!({"scopeId":"runtime","query":"柠檬酸含量","channel":"fts"}),
        ),
        (
            "knowledge_grep".into(),
            json!({"scopeId":"runtime","pattern":"柠檬酸含量","sourceIds":[foreign]}),
        ),
        (
            "knowledge_grep".into(),
            json!({"scopeId":"runtime","pattern":"柠檬酸含量","sourceIds":[src,foreign]}),
        ),
        (
            "knowledge_grep".into(),
            json!({"scopeId":"runtime","pattern":"柠檬酸含量","sourceIds":["missing-grep-source"]}),
        ),
        (
            "knowledge_grep".into(),
            json!({"scopeId":"runtime","pattern":"柠檬酸含量","sourceIds":[]}),
        ),
        (
            "knowledge_grep".into(),
            json!({"scopeId":"runtime","pattern":"柠檬酸含量","headingFilter":false}),
        ),
        (
            "knowledge_grep".into(),
            json!({"scopeId":"runtime","pattern":"柠檬酸含量","headingFilter":"HEADING_NEVER_MATCHES_ORIGINAL_PROOF"}),
        ),
    ];
    wait_until_job(&server, other_id, foreign, "pending_embedding").await;
    let (status,executed)=server.request("POST","/lingxi/v1/sessions/sess_local_alpha/execute",Some(json!({"input":"核对只选甲来源的原义知识行为","knowledge":{"notebookIds":[id],"sourceIds":[src]}}))).await;
    assert_eq!(status, 200, "{executed}");
    let run = executed["runId"].as_str().unwrap();
    assert_eq!(
        state
            .storage()
            .load_run(&lingxi_protocol::RunId::new(run))
            .await
            .unwrap()
            .unwrap()
            .status,
        lingxi_protocol::RunStatus::Completed
    );
    let requests = script.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 37);
    let final_body = requests.last().unwrap();
    let result = |index: usize| {
        final_body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| {
                m["role"] == "tool" && m["tool_call_id"] == format!("original-result-{index}")
            })
            .map(proof_content)
            .unwrap()
    };
    let search: Value = serde_json::from_str(&result(0)).unwrap();
    let hits = search["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|hit| hit["sourceId"] == src));
    assert!(hits.iter().all(|hit| hit["score"].is_number()));
    assert!(!search.to_string().contains("FOREIGN_B"));
    let span = &hits
        .iter()
        .find(|hit| hit["kind"] == "original-text")
        .unwrap()["spans"][0];
    let citation = span["citationId"].as_str().unwrap();
    assert!(span["citationMarkdown"]
        .as_str()
        .unwrap()
        .contains(citation));
    let (status, resolved) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/citations/{citation}"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(resolved["citation"]["canonicalText"], span["text"]);
    assert_eq!(resolved["snapshot"]["id"], a["snapshot"]["id"]);
    let degraded: Value = serde_json::from_str(&result(1)).unwrap();
    assert_eq!(degraded["mode"], "fts");
    assert!(
        !degraded["degraded"].as_array().unwrap().is_empty(),
        "无模型能力必须显式说明"
    );
    assert!(degraded["searchedVectorVariants"]
        .as_array()
        .unwrap()
        .is_empty());
    let none: Value = serde_json::from_str(&result(2)).unwrap();
    assert!(
        none["hits"].as_array().unwrap().is_empty(),
        "真实无匹配词必须零命中，query={no_match_query}，result={none}"
    );
    // 原 search 语义（knowledge-search-service.ts:142）：sourceIds 含越权源时
    // 整单 SCOPE_VIOLATION，不部分放行、不以空命中成功。
    assert!(
        result(3).contains("KNOWLEDGE_SCOPE_VIOLATION"),
        "只指定未授权乙时必须整单范围违规：{}",
        result(3)
    );
    let grep: Value = serde_json::from_str(&result(4)).unwrap();
    assert!(!grep["matches"].as_array().unwrap().is_empty());
    assert!(grep["matches"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["sourceId"] == src));
    let matched = &grep["matches"][0];
    assert!(matched["snippet"].as_str().unwrap().contains("柠檬酸含量"));
    assert!(matched["blockId"].is_string());
    assert!(matched["startOffset"].is_u64());
    // 以工具实际返回的blockId回读权威事实，核对同一来源、章节和字符范围。
    let facts =
        lingxi_adapters::storage::knowledge::KnowledgeFactStore::open(&home.join("knowledge"))
            .unwrap();
    let returned_block = facts
        .get_blocks_by_ids(&[matched["blockId"].as_str().unwrap().to_string()])
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        returned_block.parse_artifact_id,
        a["parseArtifact"]["id"].as_str().unwrap()
    );
    assert_eq!(
        matched["headingPath"],
        returned_block.locator["headingPath"]
    );
    assert_eq!(matched["headingPath"], json!(["原章节"]));
    let precise_match: String = returned_block
        .text
        .chars()
        .skip(matched["startOffset"].as_u64().unwrap() as usize)
        .take(
            (matched["endOffset"].as_u64().unwrap() - matched["startOffset"].as_u64().unwrap())
                as usize,
        )
        .collect();
    assert_eq!(precise_match, "柠檬酸含量");
    let grep_none: Value = serde_json::from_str(&result(5)).unwrap();
    assert!(grep_none["matches"].as_array().unwrap().is_empty());
    let outline: Value = serde_json::from_str(&result(6)).unwrap();
    assert_eq!(outline["notebooks"].as_array().unwrap().len(), 1);
    assert_eq!(
        outline["notebooks"][0]["sources"].as_array().unwrap().len(),
        1
    );
    assert_eq!(outline["notebooks"][0]["sources"][0]["sourceId"], src);
    assert!(!outline["notebooks"][0]["sources"][0]["sections"]
        .as_array()
        .unwrap()
        .is_empty());
    let read: Value = serde_json::from_str(&result(7)).unwrap();
    let text = read["spans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|span| span["text"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("ORIGINAL_PROOF"));
    assert!(!text.contains("NEW_VERSION_SHOULD_NOT_BE_READ"));
    assert_eq!(read["truncated"], true);
    assert_eq!(read["returnedChars"], 256);
    assert!(read["nextOffset"].is_u64());
    let read_citation = read["spans"][0]["citationId"].as_str().unwrap();
    let (_, located) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/citations/{read_citation}"),
            None,
        )
        .await;
    assert_eq!(located["snapshot"]["id"], a["snapshot"]["id"]);
    for index in [8, 9] {
        assert!(result(index).contains("KNOWLEDGE_SCOPE_VIOLATION"));
        assert!(!result(index).contains("FOREIGN_B"));
    }
    assert!(result(10).contains("KNOWLEDGE_INVALID_ARGUMENT"));
    assert!(!result(10).contains("FOREIGN_B"));
    let no_index: Value = serde_json::from_str(&result(11)).unwrap();
    assert!(
        no_index["hits"].as_array().unwrap().is_empty(),
        "索引缺失不得伪造命中"
    );
    assert!(no_index["degraded"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["reason"] == "KNOWLEDGE_INDEX_BUILDING"));
    for index in [12, 13, 14] {
        assert!(
            result(index).contains("KNOWLEDGE_SCOPE_VIOLATION"),
            "{}",
            result(index)
        );
        assert!(!result(index).contains("FOREIGN_B"));
        assert!(!result(index).contains("\"matches\""));
    }
    assert!(
        result(15).contains("KNOWLEDGE_INVALID_ARGUMENT"),
        "{}",
        result(15)
    );
    assert!(
        result(16).contains("tool arguments violate the current schema"),
        "{}",
        result(16)
    );
    let heading_none: Value = serde_json::from_str(&result(17)).unwrap();
    assert_eq!(heading_none["matches"], json!([]));
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

type ManageProofObserver = Arc<dyn Fn(usize, &Value) + Send + Sync>;

#[derive(Default)]
struct ManageProofScript {
    calls: Vec<Value>,
    requests: Vec<Value>,
    observe: Option<ManageProofObserver>,
    tool: Option<String>,
}

fn manage_result(body: &Value, index: usize) -> String {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| {
            message["role"] == "tool" && message["tool_call_id"] == format!("manage-result-{index}")
        })
        .map(proof_content)
        .expect("真实模型历史应包含管理工具结果")
}

async fn manage_proof_provider(
    axum::extract::State(script): axum::extract::State<Arc<std::sync::Mutex<ManageProofScript>>>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let (step, call, observe, tool) = {
        let mut script = script.lock().unwrap();
        let step = script.requests.len();
        script.requests.push(body.clone());
        (
            step,
            step.checked_sub(1)
                .and_then(|index| script.calls.get(index))
                .cloned(),
            script.observe.clone(),
            script
                .tool
                .clone()
                .unwrap_or_else(|| "knowledge_manage".into()),
        )
    };
    if let Some(observe) = observe {
        observe(step, &body);
    }
    let delta = if step == 0 {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"manage-description","type":"function","function":{"name":"mcp_describe_tool","arguments":json!({"name":tool}).to_string()}}]})
    } else if let Some(mut call) = call {
        if call["sourceId"] == "from_actual_add_result" {
            let actual: Value = serde_json::from_str(&manage_result(&body, 0)).unwrap();
            call["sourceId"] = actual["sourceId"].clone();
        }
        if call["sourceId"] == "from_actual_refresh_result" {
            let actual: Value = serde_json::from_str(&manage_result(&body, 2)).unwrap();
            call["sourceId"] = actual["source"]["id"].clone();
        }
        json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("manage-result-{}",step-1),"type":"function","function":{"name":"mcp_call","arguments":json!({"tool":tool,"arguments":call}).to_string()}}]})
    } else {
        json!({"role":"assistant","content":"管理原子行为证明完成"})
    };
    let has_tools = delta.get("tool_calls").is_some();
    let frames = [
        json!({"id":format!("manage-proof-{step}"),"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
        json!({"id":format!("manage-proof-{step}"),"choices":[{"index":0,"delta":{},"finish_reason":if has_tools{"tool_calls"}else{"stop"}}]}),
        json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":4}}),
    ];
    let stream = frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n";
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        stream,
    )
        .into_response()
}

async fn start_manage_model(
    script: Arc<std::sync::Mutex<ManageProofScript>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new()
                .route(
                    "/chat/completions",
                    axum::routing::post(manage_proof_provider),
                )
                .route(
                    "/embeddings",
                    axum::routing::post(|| async {
                        (
                            axum::http::StatusCode::BAD_GATEWAY,
                            axum::Json(json!({"error":{"message":"SYNTHETIC_BACKGROUND_EMBEDDING_FAILURE"}})),
                        )
                    }),
                )
                .with_state(script),
        )
        .await
        .unwrap();
    });
    (endpoint, handle)
}

async fn create_manage_session(server: &TestServer, id: &str, permission: &str) {
    let (_, current) = server
        .request("GET", "/lingxi/v1/sessions/sess_local_alpha", None)
        .await;
    let (status, created) = server.request("POST","/lingxi/v1/sessions",Some(json!({"sessionId":id,"agentId":current["agentId"],"title":"原管理工具证明","permissionMode":permission}))).await;
    assert_eq!(status, 201, "{created}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_manage_actual_authorized_actions_read_back_facts_and_jobs() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-manage-actions");
    let docs = unique_dir("original-manage-actions-files");
    let file = docs.join("refresh.md");
    std::fs::write(&file, "# 初始章节\n\nREFRESH_INITIAL").unwrap();
    let script = Arc::new(std::sync::Mutex::new(ManageProofScript::default()));
    let (endpoint, upstream) = start_manage_model(script.clone()).await;
    let state = boot_chat(&home, &endpoint).await;
    let facts = Arc::new(KnowledgeFactStore::open(&home.join("knowledge")).unwrap());
    let notebook = state.knowledge().create_notebook("管理目标").unwrap();
    let shared = state.knowledge().create_notebook("共享引用").unwrap();
    let id = notebook["id"].as_str().unwrap().to_string();
    let shared_id = shared["id"].as_str().unwrap().to_string();
    let initial_file = state
        .knowledge()
        .import_file(&id, file.to_str().unwrap(), None)
        .unwrap();
    let file_source = initial_file["source"]["id"].as_str().unwrap().to_string();
    let frozen_bytes = Arc::new(std::sync::Mutex::new(None::<(String, Vec<u8>)>));
    let before_reindex = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (observe_facts, observe_home, observe_file, observe_id, observe_shared) = (
        facts.clone(),
        home.clone(),
        file.clone(),
        id.clone(),
        shared_id.clone(),
    );
    let (saved_bytes, saved_jobs) = (frozen_bytes.clone(), before_reindex.clone());
    script.lock().unwrap().observe = Some(Arc::new(move |step, body| {
        match step {
            2 => {
                let added: Value = serde_json::from_str(&manage_result(body, 0)).unwrap();
                assert_eq!(added["action"], "add");
                let source = added["sourceId"].as_str().unwrap();
                assert_eq!(
                    observe_facts.get_source(source).unwrap().display_name,
                    "工具实际新增原文"
                );
                let snapshot = observe_facts.latest_snapshot_for_source(source).unwrap();
                let bytes =
                    std::fs::read(observe_home.join("knowledge").join(&snapshot.storage_path))
                        .unwrap();
                assert_eq!(bytes, b"MANAGE_ACTUAL_ADDED_BODY");
                assert!(observe_facts
                    .latest_job_for_source(&observe_id, source)
                    .unwrap()
                    .is_some());
                // 只准备同studio共享引用；remove自身仍由真实工具执行。
                observe_facts
                    .add_membership(&observe_shared, source)
                    .unwrap();
                *saved_bytes.lock().unwrap() = Some((source.to_string(), bytes));
            }
            3 => {
                let removed: Value = serde_json::from_str(&manage_result(body, 1)).unwrap();
                assert_eq!(removed["action"], "remove");
                assert!(removed["membership"]["removedAt"].is_string());
                let saved = saved_bytes.lock().unwrap();
                let (source, bytes) = saved.as_ref().unwrap();
                assert!(!observe_facts
                    .list_notebook_sources(&observe_id)
                    .unwrap()
                    .iter()
                    .any(|entry| entry.source.id == *source));
                assert!(observe_facts
                    .list_notebook_sources(&observe_shared)
                    .unwrap()
                    .iter()
                    .any(|entry| entry.source.id == *source));
                assert!(observe_facts.get_source(source).is_ok());
                let snapshot = observe_facts.latest_snapshot_for_source(source).unwrap();
                assert_eq!(
                    &std::fs::read(observe_home.join("knowledge").join(&snapshot.storage_path))
                        .unwrap(),
                    bytes
                );
                std::fs::write(&observe_file, "# 实际刷新章节\n\nREFRESH_ACTUAL_CHANGED").unwrap();
            }
            4 => {
                let refreshed: Value = serde_json::from_str(&manage_result(body, 2)).unwrap();
                assert_eq!(refreshed["action"], "refresh");
                assert_eq!(refreshed["changed"], true);
                let source = refreshed["source"]["id"].as_str().unwrap();
                let snapshot = observe_facts.latest_snapshot_for_source(source).unwrap();
                assert_eq!(refreshed["snapshot"]["id"], snapshot.id);
                assert_eq!(
                    std::fs::read(observe_home.join("knowledge").join(&snapshot.storage_path))
                        .unwrap(),
                    "# 实际刷新章节\n\nREFRESH_ACTUAL_CHANGED".as_bytes()
                );
                assert_eq!(refreshed["parseArtifact"]["status"], "ready");
                assert!(observe_facts
                    .latest_job_for_source(&observe_id, source)
                    .unwrap()
                    .is_some());
                *saved_jobs.lock().unwrap() = observe_facts
                    .list_ingestion_jobs(Some(&observe_id), None, 200)
                    .unwrap()
                    .iter()
                    .map(|job| job.id.clone())
                    .collect();
            }
            5 => {
                let same: Value = serde_json::from_str(&manage_result(body, 3)).unwrap();
                let changed: Value = serde_json::from_str(&manage_result(body, 2)).unwrap();
                assert_eq!(same["action"], "refresh");
                assert_eq!(same["changed"], false);
                assert_eq!(same["snapshot"]["id"], changed["snapshot"]["id"]);
                let jobs: Vec<_> = observe_facts
                    .list_ingestion_jobs(Some(&observe_id), None, 200)
                    .unwrap()
                    .iter()
                    .map(|job| job.id.clone())
                    .collect();
                assert_eq!(
                    &jobs,
                    &*saved_jobs.lock().unwrap(),
                    "文件未变不得新增快照/摄入任务"
                );
            }
            6 => {
                let reindex: Value = serde_json::from_str(&manage_result(body, 4)).unwrap();
                assert_eq!(reindex["action"], "reindex");
                assert_eq!(reindex["status"], "queued");
                let before = saved_jobs.lock().unwrap();
                let jobs = observe_facts
                    .list_ingestion_jobs(Some(&observe_id), None, 200)
                    .unwrap();
                let new: Vec<_> = jobs
                    .iter()
                    .filter(|job| !before.contains(&job.id))
                    .collect();
                let current = observe_facts.list_notebook_sources(&observe_id).unwrap();
                assert_eq!(new.len(), current.len());
                assert!(!new.is_empty());
                assert!(current
                    .iter()
                    .all(|source| new.iter().any(|job| job.source_id == source.source.id)));
            }
            _ => {}
        }
    }));
    script.lock().unwrap().calls = vec![
        json!({"action":"add","notebookId":id,"kind":"pasted_text","text":"MANAGE_ACTUAL_ADDED_BODY","displayName":"工具实际新增原文"}),
        json!({"action":"remove","notebookId":id,"sourceId":"from_actual_add_result"}),
        json!({"action":"refresh","notebookId":id,"sourceId":file_source}),
        json!({"action":"refresh","notebookId":id,"sourceId":"from_actual_refresh_result"}),
        json!({"action":"reindex","notebookId":id}),
    ];
    let server = serve(state.clone()).await;
    create_manage_session(&server, "original-manage-operate", "operate").await;
    let (status, executed) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/original-manage-operate/execute",
            Some(json!({"input":"按明确授权依次新增原文、移除共享引用、刷新文件并重排索引"})),
        )
        .await;
    assert_eq!(status, 200, "{executed}");
    let wire = script.lock().unwrap().requests.clone();
    assert_eq!(wire.len(), 7);
    assert_eq!(
        serde_json::from_str::<Value>(&manage_result(wire.last().unwrap(), 4)).unwrap()["status"],
        "queued"
    );
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_manage_refusals_and_no_write_authorization_preserve_facts() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-manage-refusals");
    let script = Arc::new(std::sync::Mutex::new(ManageProofScript::default()));
    let (endpoint, upstream) = start_manage_model(script.clone()).await;
    let state = boot_chat(&home, &endpoint).await;
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let notebook = state.knowledge().create_notebook("拒绝不得改写").unwrap();
    let id = notebook["id"].as_str().unwrap();
    let foreign_knowledge =
        lingxi_service::knowledge::KnowledgeService::new(&home, "foreign-manage-studio".into())
            .unwrap();
    let foreign = foreign_knowledge
        .create_notebook("外studio原始名称")
        .unwrap();
    let foreign_id = foreign["id"].as_str().unwrap();
    let foreign_source = foreign_knowledge
        .import_pasted_text(foreign_id, "FOREIGN_MANAGE_BYTES", Some("外部源"))
        .unwrap();
    let source_id = foreign_source["source"]["id"].as_str().unwrap();
    let source_ids = |notebook_id: &str| {
        facts
            .list_notebook_sources(notebook_id)
            .unwrap()
            .iter()
            .map(|row| row.source.id.clone())
            .collect::<Vec<_>>()
    };
    let server = serve(state.clone()).await;
    wait_until_job(&server, foreign_id, source_id, "pending_embedding").await;
    // 等真实后台初始化稳定后再取基线，初始化写入不能误算为拒绝操作的副作用。
    let snapshot = facts.latest_snapshot_for_source(source_id).unwrap();
    let foreign_before = facts.get_source(source_id).unwrap();
    let initial_jobs = facts.list_ingestion_jobs(Some(id), None, 200).unwrap();
    let foreign_jobs = facts
        .list_ingestion_jobs(Some(foreign_id), None, 200)
        .unwrap();
    // 相对文件引用在真实历史/归属检查处结束本Run，单独核明确错误与零知识副作用。
    script.lock().unwrap().calls =
        vec![json!({"action":"add","notebookId":id,"kind":"file","filePath":"relative.md"})];
    create_manage_session(&server, "original-manage-path-refused", "operate").await;
    let (status, error) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/original-manage-path-refused/execute",
            Some(json!({"input":"验证非法相对文件路径明确拒绝且不改写资料"})),
        )
        .await;
    assert_eq!(status, 500, "{error}");
    assert!(
        error
            .to_string()
            .contains("session_file_reference_not_owned:relative.md"),
        "{error}"
    );
    assert!(source_ids(id).is_empty());
    assert_eq!(source_ids(foreign_id), vec![source_id.to_string()]);
    assert_eq!(facts.get_source(source_id).unwrap(), foreign_before);
    assert_eq!(
        facts.list_ingestion_jobs(Some(id), None, 200).unwrap(),
        initial_jobs
    );
    assert_eq!(
        facts
            .list_ingestion_jobs(Some(foreign_id), None, 200)
            .unwrap(),
        foreign_jobs
    );
    assert_eq!(
        std::fs::read(home.join("knowledge").join(&snapshot.storage_path)).unwrap(),
        b"FOREIGN_MANAGE_BYTES"
    );
    // 其余拒绝在另一真实Run继续，不能把上轮错误当成工具返回后的可继续结果。
    script.lock().unwrap().requests.clear();
    script.lock().unwrap().calls = vec![
        json!({"action":"add","notebookId":id,"kind":"invalid-kind","text":"不得登记"}),
        json!({"action":"invalid-action","notebookId":id}),
        json!({"action":"add","notebookId":foreign_id,"kind":"pasted_text","text":"不得写外库"}),
        json!({"action":"remove","notebookId":foreign_id,"sourceId":source_id}),
        json!({"action":"refresh","notebookId":foreign_id,"sourceId":source_id}),
        json!({"action":"reindex","notebookId":foreign_id}),
    ];
    create_manage_session(&server, "original-manage-refused", "operate").await;
    let (status, executed) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/original-manage-refused/execute",
            Some(json!({"input":"验证输入与外工作空间拒绝边界"})),
        )
        .await;
    assert_eq!(status, 200, "{executed}");
    let body = script.lock().unwrap().requests.last().unwrap().clone();
    assert!(manage_result(&body, 0).contains("tool arguments violate the current schema"));
    assert!(manage_result(&body, 0).contains("kind"));
    assert!(manage_result(&body, 1).contains("tool arguments violate the current schema"));
    assert!(manage_result(&body, 1).contains("action"));
    for index in [2, 3, 4, 5] {
        assert!(manage_result(&body, index).contains("KNOWLEDGE_NOT_FOUND"));
    }
    assert!(source_ids(id).is_empty());
    assert_eq!(source_ids(foreign_id), vec![source_id.to_string()]);
    assert_eq!(facts.get_source(source_id).unwrap(), foreign_before);
    assert_eq!(
        facts.list_ingestion_jobs(Some(id), None, 200).unwrap(),
        initial_jobs
    );
    assert_eq!(
        facts
            .list_ingestion_jobs(Some(foreign_id), None, 200)
            .unwrap(),
        foreign_jobs
    );
    assert_eq!(
        std::fs::read(home.join("knowledge").join(&snapshot.storage_path)).unwrap(),
        b"FOREIGN_MANAGE_BYTES"
    );
    // 新的真实read_only会话没有写授权，不允许arguments或目录描述冒充批准。
    script.lock().unwrap().requests.clear();
    script.lock().unwrap().calls = vec![
        json!({"action":"add","notebookId":id,"kind":"pasted_text","text":"UNAPPROVED_MUST_NOT_EXIST"}),
    ];
    create_manage_session(&server, "original-manage-no-authorization", "read_only").await;
    let (status, executed) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/original-manage-no-authorization/execute",
            Some(json!({"input":"读取模式的写工具拒绝"})),
        )
        .await;
    assert_eq!(status, 200, "{executed}");
    let body = script.lock().unwrap().requests.last().unwrap().clone();
    assert!(manage_result(&body, 0).contains("ACTION_BLOCKED_BY_READ_ONLY"));
    assert!(source_ids(id).is_empty());
    assert_eq!(
        facts.list_ingestion_jobs(Some(id), None, 200).unwrap(),
        initial_jobs
    );
    assert_eq!(facts.get_source(source_id).unwrap(), foreign_before);
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_manage_interactive_unapproved_and_cancelled_leave_no_sources_or_jobs() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-manage-interactive");
    let script = Arc::new(std::sync::Mutex::new(ManageProofScript::default()));
    let (endpoint, upstream) = start_manage_model(script.clone()).await;
    let state = boot_chat(&home, &endpoint).await;
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let notebook = state.knowledge().create_notebook("未批准不得写入").unwrap();
    let id = notebook["id"].as_str().unwrap();
    assert!(facts.list_notebook_sources(id).unwrap().is_empty());
    let jobs_before = facts.list_ingestion_jobs(Some(id), None, 200).unwrap();
    script.lock().unwrap().calls = vec![json!({
        "action":"add","notebookId":id,"kind":"pasted_text","text":"UNAPPROVED_INTERACTIVE_BYTES"
    })];
    let server = serve(state.clone()).await;
    let session = "original-manage-interactive";
    create_manage_session(&server, session, "ask").await;
    let path = format!("/lingxi/v1/sessions/{session}/execute");
    {
        let executing = server.request(
            "POST",
            &path,
            Some(json!({"input":"管理工具必须等待批准，不能先写来源"})),
        );
        tokio::pin!(executing);
        let waiting = async {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                let run = state.storage().query_one_text(
                "SELECT run_id FROM runs WHERE session_id=?1 AND status='waiting_approval' ORDER BY rowid DESC LIMIT 1",
                vec![session.to_string()],
            ).await.unwrap();
                if let Some(run) = run {
                    break run;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "真实管理调用未进入持久审批等待"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        let run_id = tokio::select! {
            result = &mut executing => panic!("未经批准执行不应提前结束：{result:?}"),
            run_id = waiting => run_id,
        };
        assert!(facts.list_notebook_sources(id).unwrap().is_empty());
        assert_eq!(
            facts.list_ingestion_jobs(Some(id), None, 200).unwrap(),
            jobs_before
        );
        assert_eq!(
            script.lock().unwrap().requests.len(),
            2,
            "描述后真实写调用等待审批，没有返回成功工具结果"
        );
        state.runs().cancel_run(&run_id, "取消尚未批准的知识写入");
        let _ = tokio::time::timeout(Duration::from_secs(10), &mut executing)
            .await
            .unwrap();
        assert_eq!(
            state
                .storage()
                .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run_id])
                .await
                .unwrap()
                .as_deref(),
            Some("cancelled")
        );
        assert!(facts.list_notebook_sources(id).unwrap().is_empty());
        assert_eq!(
            facts.list_ingestion_jobs(Some(id), None, 200).unwrap(),
            jobs_before
        );
    }
    script.lock().unwrap().requests.clear();
    create_manage_session(&server, "original-manage-approved-control", "operate").await;
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/original-manage-approved-control/execute",
            Some(json!({"input":"明确授权新增，同一管理参数作为批准正控"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let recorded: Value = serde_json::from_str(&manage_result(
        script.lock().unwrap().requests.last().unwrap(),
        0,
    ))
    .unwrap();
    assert_eq!(recorded["action"], "add");
    assert_eq!(facts.list_notebook_sources(id).unwrap().len(), 1);
    assert_eq!(
        facts
            .list_ingestion_jobs(Some(id), None, 200)
            .unwrap()
            .len(),
        1
    );
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_manage_add_preserves_source_and_reports_background_index_failure_separately() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-manage-background-failure");
    let script = Arc::new(std::sync::Mutex::new(ManageProofScript::default()));
    let (endpoint, upstream) = start_manage_model(script.clone()).await;
    let state = boot_chat_with_embedding(&home, &endpoint, true).await;
    let notebook = state.knowledge().create_notebook("后台失败单列").unwrap();
    let id = notebook["id"].as_str().unwrap();
    script.lock().unwrap().calls = vec![json!({
        "action":"add","notebookId":id,"kind":"pasted_text","text":"PERSISTED_DESPITE_INDEX_FAILURE"
    })];
    let server = serve(state).await;
    create_manage_session(&server, "original-manage-background-failure", "operate").await;
    let (status, body) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/original-manage-background-failure/execute",
            Some(json!({"input":"导入原文，后台失败必须独立报告不能伪报索引完成"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let imported: Value = serde_json::from_str(&manage_result(
        script.lock().unwrap().requests.last().unwrap(),
        0,
    ))
    .unwrap();
    assert_eq!(imported["action"], "add");
    let source_id = imported["sourceId"].as_str().unwrap();
    assert_eq!(imported["source"]["parseArtifact"]["status"], "ready");
    let failed = wait_until_job(&server, id, source_id, "failed").await;
    assert_eq!(failed["sourceId"], source_id);
    assert!(
        failed
            .to_string()
            .contains("SYNTHETIC_BACKGROUND_EMBEDDING_FAILURE"),
        "{failed}"
    );
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    assert_eq!(facts.list_notebook_sources(id).unwrap().len(), 1);
    let snapshot = facts.latest_snapshot_for_source(source_id).unwrap();
    assert_eq!(
        std::fs::read(home.join("knowledge").join(snapshot.storage_path)).unwrap(),
        b"PERSISTED_DESPITE_INDEX_FAILURE"
    );
    let (status, listed) = server
        .request("GET", "/lingxi/v1/knowledge/notebooks", None)
        .await;
    assert_eq!(status, 200, "{listed}");
    let reported = listed["notebooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    assert_eq!(reported["ingestion"]["failed"], 1);
    assert_eq!(reported["ingestion"]["done"], 0);
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

async fn execute_file_product_tool(
    server: &TestServer,
    script: &Arc<std::sync::Mutex<ManageProofScript>>,
    session: &str,
    tool: &str,
    arguments: Value,
) -> String {
    {
        let mut script = script.lock().unwrap();
        script.requests.clear();
        script.tool = Some(tool.into());
        script.calls = vec![arguments];
    }
    let (status, body) = server
        .request(
            "POST",
            &format!("/lingxi/v1/sessions/{session}/execute"),
            Some(json!({"input":"验证真实文件工具字节与会话文件登记，不伪报成功"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    manage_result(script.lock().unwrap().requests.last().unwrap(), 0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_model_write_edit_register_real_session_products_and_update_metadata() {
    let home = unique_dir("original-tool-file-products");
    let script = Arc::new(std::sync::Mutex::new(ManageProofScript::default()));
    let (endpoint, upstream) = start_manage_model(script.clone()).await;
    let state = boot_chat(&home, &endpoint).await;
    let server = serve(state.clone()).await;
    let session = "original-tool-file-products";
    create_manage_session(&server, session, "operate").await;
    create_manage_session(&server, "other-tool-file-session", "operate").await;
    let file = home.join("created-product.txt");
    let written = execute_file_product_tool(
        &server,
        &script,
        session,
        "write",
        json!({
            "path":file, "content":"ORIGINAL_CHANGE"
        }),
    )
    .await;
    assert!(written.contains("Successfully wrote"), "{written}");
    assert_eq!(std::fs::read(&file).unwrap(), b"ORIGINAL_CHANGE");
    let files_path = format!("/lingxi/v1/sessions/{session}/files");
    let (status, files) = server.request("GET", &files_path, None).await;
    assert_eq!(status, 200, "{files}");
    assert_eq!(files["files"].as_array().unwrap().len(), 1);
    let registered = &files["files"][0];
    assert_eq!(
        registered["realPath"],
        file.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(registered["sizeBytes"], 15);
    assert_eq!(registered["origin"], "agent_artifact");
    assert_eq!(registered["status"], "available");
    let file_id = registered["fileId"].as_str().unwrap().to_string();
    let (_, other) = server
        .request(
            "GET",
            "/lingxi/v1/sessions/other-tool-file-session/files",
            None,
        )
        .await;
    assert!(other["files"].as_array().unwrap().is_empty());
    let read =
        execute_file_product_tool(&server, &script, session, "read", json!({"path":file})).await;
    assert!(read.contains("ORIGINAL_CHANGE"), "{read}");
    let edited = execute_file_product_tool(&server, &script, session, "edit", json!({
        "path":file, "edits":[{"oldText":"ORIGINAL_CHANGE","newText":"REPLACED_WITH_LONGER_BODY"}]
    })).await;
    assert!(edited.contains("Successfully replaced"), "{edited}");
    assert_eq!(std::fs::read(&file).unwrap(), b"REPLACED_WITH_LONGER_BODY");
    let (_, after) = server.request("GET", &files_path, None).await;
    assert_eq!(after["files"].as_array().unwrap().len(), 1);
    assert_eq!(after["files"][0]["fileId"], file_id);
    assert_eq!(after["files"][0]["sizeBytes"], 25);
    let disk_mtime = std::fs::metadata(&file)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert_eq!(after["files"][0]["mtimeMs"], disk_mtime);
    let mismatch = execute_file_product_tool(
        &server,
        &script,
        session,
        "edit",
        json!({
            "path":file, "edits":[{"oldText":"MISSING_OLD_TEXT","newText":"MUST_NOT_WRITE"}]
        }),
    )
    .await;
    assert!(!mismatch.contains("Successfully replaced"), "{mismatch}");
    assert_eq!(std::fs::read(&file).unwrap(), b"REPLACED_WITH_LONGER_BODY");
    let (_, unchanged) = server.request("GET", &files_path, None).await;
    assert_eq!(unchanged, after);
    std::fs::write(&file, b"EXTERNAL_USER_CHANGE").unwrap();
    let conflict = execute_file_product_tool(
        &server,
        &script,
        session,
        "write",
        json!({
            "path":file, "content":"MUST_NOT_OVERWRITE"
        }),
    )
    .await;
    assert!(!conflict.contains("Successfully wrote"), "{conflict}");
    assert_eq!(std::fs::read(&file).unwrap(), b"EXTERNAL_USER_CHANGE");
    let (_, refused) = server.request("GET", &files_path, None).await;
    assert_eq!(refused, after);
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_model_file_registration_failure_reports_written_bytes_without_success() {
    let home = unique_dir("original-tool-registration-failure");
    let script = Arc::new(std::sync::Mutex::new(ManageProofScript::default()));
    let (endpoint, upstream) = start_manage_model(script.clone()).await;
    let state = boot_chat(&home, &endpoint).await;
    let server = serve(state).await;
    let session = "original-tool-registration-failure";
    create_manage_session(&server, session, "operate").await;
    let db = rusqlite::Connection::open(home.join("lingxi-service/data/runs.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER r06_tool_file_registration_refused BEFORE INSERT ON session_files \
         BEGIN SELECT RAISE(ABORT, 'actual_tool_registration_failure'); END;",
    )
    .unwrap();
    let file = home.join("written-without-registration.txt");
    let failed = execute_file_product_tool(
        &server,
        &script,
        session,
        "write",
        json!({
            "path":file, "content":"ACTUAL_WRITTEN_WITH_FAILED_REGISTRATION"
        }),
    )
    .await;
    assert!(
        failed.contains("file write occurred but product registration failed"),
        "{failed}"
    );
    assert!(
        failed.contains("actual_tool_registration_failure"),
        "{failed}"
    );
    assert!(!failed.contains("Successfully wrote"), "{failed}");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"ACTUAL_WRITTEN_WITH_FAILED_REGISTRATION"
    );
    let (_, files) = server
        .request("GET", &format!("/lingxi/v1/sessions/{session}/files"), None)
        .await;
    assert!(files["files"].as_array().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER r06_tool_file_registration_refused")
        .unwrap();
    drop(db);
    let read =
        execute_file_product_tool(&server, &script, session, "read", json!({"path":file})).await;
    assert!(
        read.contains("ACTUAL_WRITTEN_WITH_FAILED_REGISTRATION"),
        "{read}"
    );
    let retried = execute_file_product_tool(
        &server,
        &script,
        session,
        "write",
        json!({
            "path":file, "content":"RECOVERED_REGISTERED_CONTENT"
        }),
    )
    .await;
    assert!(retried.contains("Successfully wrote"), "{retried}");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"RECOVERED_REGISTERED_CONTENT"
    );
    let (_, files) = server
        .request("GET", &format!("/lingxi/v1/sessions/{session}/files"), None)
        .await;
    assert_eq!(files["files"].as_array().unwrap().len(), 1);
    assert_eq!(files["files"][0]["sizeBytes"], 28);
    let registered_before = files.clone();
    let db = rusqlite::Connection::open(home.join("lingxi-service/data/runs.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER r06_tool_metadata_refused BEFORE UPDATE OF size_bytes ON session_files \
         BEGIN SELECT RAISE(ABORT, 'actual_tool_metadata_failure'); END;",
    )
    .unwrap();
    let failed_edit = execute_file_product_tool(&server, &script, session, "edit", json!({
        "path":file, "edits":[{"oldText":"RECOVERED_REGISTERED_CONTENT","newText":"WRITTEN_EDIT_METADATA_FAILED"}]
    })).await;
    assert!(
        failed_edit.contains("file write occurred but product registration failed"),
        "{failed_edit}"
    );
    assert!(
        failed_edit.contains("actual_tool_metadata_failure"),
        "{failed_edit}"
    );
    assert!(
        !failed_edit.contains("Successfully replaced"),
        "{failed_edit}"
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"WRITTEN_EDIT_METADATA_FAILED"
    );
    let (_, retained) = server
        .request("GET", &format!("/lingxi/v1/sessions/{session}/files"), None)
        .await;
    assert_eq!(retained, registered_before);
    db.execute_batch("DROP TRIGGER r06_tool_metadata_refused")
        .unwrap();
    drop(db);
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}
