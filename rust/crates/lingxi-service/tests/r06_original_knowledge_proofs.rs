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
async fn original_notebook_delete_purges_owned_sources_and_keeps_shared_ones() {
    use lingxi_adapters::storage::knowledge::{KnowledgeFactStore, KnowledgeIndexStore};
    let home = unique_dir("original-notebook-delete");
    let state = boot("original-notebook-delete", &home, None).await;
    let studio = state.knowledge().studio_id().to_string();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let knowledge = state.knowledge();
    let doomed = knowledge.create_notebook("待删笔记本").unwrap();
    let doomed_id = doomed["id"].as_str().unwrap().to_string();
    let keeper = knowledge.create_notebook("保留笔记本").unwrap();
    let keeper_id = keeper["id"].as_str().unwrap().to_string();
    let mut owned = Vec::new();
    for (name, text) in [
        ("独占一", NOTE_TEXT_A),
        ("独占二", "# 独占二\n\n第二份独占来源正文。"),
    ] {
        let imported = knowledge
            .import_pasted_text(&doomed_id, text, Some(name))
            .unwrap();
        owned.push(imported["source"]["id"].as_str().unwrap().to_string());
    }
    let shared = knowledge
        .import_pasted_text(
            &doomed_id,
            "# 共享\n\n同时属于两个笔记本。",
            Some("共享来源"),
        )
        .unwrap();
    let shared_id = shared["source"]["id"].as_str().unwrap().to_string();
    facts.add_membership(&keeper_id, &shared_id).unwrap();
    let other = knowledge
        .import_pasted_text(&keeper_id, "# 无关\n\n不属于待删笔记本。", Some("无关来源"))
        .unwrap();
    let other_id = other["source"]["id"].as_str().unwrap().to_string();
    let server = serve(state.clone()).await;
    for source in owned.iter().chain([&shared_id, &other_id]) {
        let nb = if *source == other_id {
            &keeper_id
        } else {
            &doomed_id
        };
        wait_until_job(&server, nb, source, "pending_embedding").await;
    }
    // 删除前的真实物理痕迹：快照文件、解析产物文件、FTS 变体、任务行。
    let index = KnowledgeIndexStore::open(&home.join("knowledge")).unwrap();
    let mut owned_files = Vec::new();
    let mut owned_artifacts = Vec::new();
    for source in &owned {
        let snapshot = facts.latest_snapshot_for_source(source).unwrap();
        let snapshot_file = home.join("knowledge").join(&snapshot.storage_path);
        assert!(snapshot_file.exists());
        owned_files.push(snapshot_file);
        let artifacts = facts.list_source_parse_artifacts(source).unwrap();
        assert!(!artifacts.is_empty());
        for (artifact_id, snapshot_id) in artifacts {
            let path = home
                .join("knowledge")
                .join("artifacts")
                .join(snapshot_id)
                .join(format!("{artifact_id}.json"));
            assert!(path.exists());
            assert!(!index
                .list_ready_variant_metadata(&artifact_id)
                .unwrap()
                .is_empty());
            owned_files.push(path);
            owned_artifacts.push(artifact_id);
        }
        assert!(!facts
            .list_ingestion_jobs(None, Some(source), 50)
            .unwrap()
            .is_empty());
    }
    let shared_snapshot = facts.latest_snapshot_for_source(&shared_id).unwrap();
    let other_snapshot = facts.latest_snapshot_for_source(&other_id).unwrap();
    let other_jobs_before = facts
        .list_ingestion_jobs(Some(&keeper_id), Some(&other_id), 50)
        .unwrap();
    assert!(facts
        .list_ingestion_jobs(Some(&doomed_id), Some(&shared_id), 50)
        .unwrap()
        .iter()
        .any(|job| job.status == "pending_embedding"));

    let (status, result) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/notebooks/{doomed_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["notebook"]["id"], doomed_id);
    assert!(result["notebook"]["deletedAt"].is_string());
    let mut deleted: Vec<String> = result["deletedSourceIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    deleted.sort();
    let mut expected = owned.clone();
    expected.sort();
    assert_eq!(deleted, expected, "只删除独占来源，共享来源不在结果里");
    assert!(result["cancelledJobs"].as_u64().unwrap() >= 2);
    // 笔记本已不可见，独占来源的事实行/任务/文件/索引全部清理。
    assert!(facts.get_notebook(&studio, &doomed_id).is_err());
    let (status, error) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/notebooks/{doomed_id}"),
            None,
        )
        .await;
    assert_eq!(status, 404, "{error}");
    for source in &owned {
        assert!(facts.get_source(source).is_err());
        assert!(facts
            .list_ingestion_jobs(None, Some(source), 50)
            .unwrap()
            .is_empty());
        assert!(facts
            .list_source_parse_artifacts(source)
            .unwrap()
            .is_empty());
    }
    for path in &owned_files {
        assert!(!path.exists(), "独占来源文件必须清理: {}", path.display());
    }
    for artifact in &owned_artifacts {
        assert!(index
            .list_ready_variant_metadata(artifact)
            .unwrap()
            .is_empty());
    }
    // 共享来源只失去该笔记本的 membership：行、文件、另一笔记本关系保留，本笔记本的活跃任务被取消。
    assert_eq!(facts.get_source(&shared_id).unwrap().id, shared_id);
    assert_eq!(
        facts.read_snapshot_bytes(&shared_snapshot).unwrap(),
        "# 共享\n\n同时属于两个笔记本。".as_bytes()
    );
    assert!(!facts
        .get_membership(&doomed_id, &shared_id)
        .unwrap()
        .removed_at
        .is_none());
    assert!(facts
        .get_membership(&keeper_id, &shared_id)
        .unwrap()
        .removed_at
        .is_none());
    let shared_jobs = facts
        .list_ingestion_jobs(Some(&doomed_id), Some(&shared_id), 50)
        .unwrap();
    assert!(!shared_jobs.is_empty());
    assert!(shared_jobs
        .iter()
        .all(|job| job.status == "failed" && job.cancelled_at.is_some()));
    // 无关来源完全不受影响。
    assert_eq!(facts.get_source(&other_id).unwrap().id, other_id);
    assert_eq!(
        facts.read_snapshot_bytes(&other_snapshot).unwrap(),
        "# 无关\n\n不属于待删笔记本。".as_bytes()
    );
    assert_eq!(
        facts
            .list_ingestion_jobs(Some(&keeper_id), Some(&other_id), 50)
            .unwrap(),
        other_jobs_before
    );
    let (status, listed) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/notebooks/{keeper_id}/sources"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{listed}");
    let mut kept: Vec<&str> = listed["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["source"]["id"].as_str().unwrap())
        .collect();
    kept.sort();
    let mut kept_expected = vec![shared_id.as_str(), other_id.as_str()];
    kept_expected.sort();
    assert_eq!(kept, kept_expected);
    server.stop().await;
}

#[tokio::test]
async fn original_notebook_delete_refuses_frozen_foreign_and_missing_without_side_effects() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-notebook-delete-refuse");
    let state = boot("original-notebook-delete-refuse", &home, None).await;
    let studio = state.knowledge().studio_id().to_string();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let knowledge = state.knowledge();
    let notebook = knowledge.create_notebook("冻结保护").unwrap();
    let id = notebook["id"].as_str().unwrap().to_string();
    let mut sources = Vec::new();
    for (name, text) in [("甲", NOTE_TEXT_A), ("乙", "# 乙\n\n另一份来源。")] {
        let imported = knowledge.import_pasted_text(&id, text, Some(name)).unwrap();
        sources.push(imported["source"]["id"].as_str().unwrap().to_string());
    }
    let server = serve(state.clone()).await;
    for source in &sources {
        wait_until_job(&server, &id, source, "pending_embedding").await;
    }
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/knowledge/turn-scopes",
            Some(json!({"notebookIds":[id],"sessionId":"notebook-delete-freeze","turnId":"one"})),
        )
        .await;
    assert_eq!(status, 201);
    let snapshots: Vec<_> = sources
        .iter()
        .map(|s| facts.latest_snapshot_for_source(s).unwrap())
        .collect();
    let artifact_files: Vec<_> = sources
        .iter()
        .flat_map(|s| facts.list_source_parse_artifacts(s).unwrap())
        .map(|(artifact, snapshot)| {
            home.join("knowledge")
                .join("artifacts")
                .join(snapshot)
                .join(format!("{artifact}.json"))
        })
        .collect();
    assert!(!artifact_files.is_empty() && artifact_files.iter().all(|p| p.exists()));
    let jobs_before = facts.list_ingestion_jobs(None, None, 1000).unwrap();
    let notebook_before = facts.get_notebook(&studio, &id).unwrap();
    let members_before: Vec<_> = sources
        .iter()
        .map(|s| facts.get_membership(&id, s).unwrap())
        .collect();
    assert_eq!(facts.list_notebook_sources(&id).unwrap().len(), 2);
    let assert_untouched = |label: &str| {
        assert_eq!(
            facts.get_notebook(&studio, &id).unwrap(),
            notebook_before,
            "{label}"
        );
        let members_now: Vec<_> = sources
            .iter()
            .map(|s| facts.get_membership(&id, s).unwrap())
            .collect();
        assert_eq!(members_now, members_before, "{label}");
        assert_eq!(
            facts.list_ingestion_jobs(None, None, 1000).unwrap(),
            jobs_before,
            "{label}: 拒绝不能取消或新增任务"
        );
        for (source, snapshot) in sources.iter().zip(&snapshots) {
            assert_eq!(
                facts.get_source(source).unwrap().deleted_at,
                None,
                "{label}"
            );
            assert!(
                facts.read_snapshot_bytes(snapshot).is_ok(),
                "{label}: 快照文件必须保留"
            );
        }
        assert!(artifact_files.iter().all(|p| p.exists()), "{label}");
    };
    // 活跃 scope 冻结了成员来源：409，笔记本/成员/来源/任务/文件原样保留。
    let (status, error) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/notebooks/{id}"),
            None,
        )
        .await;
    assert_eq!(status, 409, "{error}");
    assert_eq!(error["error"], "KNOWLEDGE_CONFLICT");
    assert_untouched("frozen");
    // 他 studio 的笔记本与不存在的笔记本：404，且不碰任何事实。
    let foreign_nb = facts.create_notebook("other-studio", "外部笔记本").unwrap();
    let (foreign_source, foreign_snapshot) = facts
        .create_source_with_snapshot(
            "other-studio",
            "pasted_text",
            "外部来源",
            &json!({}),
            b"foreign studio body",
            "text/plain",
        )
        .unwrap();
    facts
        .add_membership(&foreign_nb.id, &foreign_source.id)
        .unwrap();
    for target in [foreign_nb.id.as_str(), "knb_missing"] {
        let (status, error) = server
            .request(
                "DELETE",
                &format!("/lingxi/v1/knowledge/notebooks/{target}"),
                None,
            )
            .await;
        assert_eq!(status, 404, "{target}: {error}");
        assert_eq!(error["error"], "KNOWLEDGE_NOT_FOUND");
    }
    assert_eq!(
        facts
            .get_notebook("other-studio", &foreign_nb.id)
            .unwrap()
            .deleted_at,
        None
    );
    assert_eq!(
        facts
            .get_membership(&foreign_nb.id, &foreign_source.id)
            .unwrap()
            .removed_at,
        None
    );
    assert_eq!(
        facts.read_snapshot_bytes(&foreign_snapshot).unwrap(),
        b"foreign studio body"
    );
    assert_eq!(
        facts.get_source(&foreign_source.id).unwrap().deleted_at,
        None
    );
    assert_untouched("foreign/missing");
    // 冻结解除后同一删除成功：证明上面的 409 确由冻结引用造成。
    let empty = knowledge.create_notebook("接管冻结").unwrap();
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/knowledge/turn-scopes",
            Some(json!({"notebookIds":[empty["id"]],"sessionId":"notebook-delete-freeze","turnId":"two"})),
        )
        .await;
    assert_eq!(status, 201);
    let (status, result) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/notebooks/{id}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["deletedSourceIds"].as_array().unwrap().len(), 2);
    for (source, snapshot) in sources.iter().zip(&snapshots) {
        assert!(facts.get_source(source).is_err());
        assert!(!home.join("knowledge").join(&snapshot.storage_path).exists());
    }
    assert!(artifact_files.iter().all(|p| !p.exists()));
    assert_eq!(
        facts.get_source(&foreign_source.id).unwrap().deleted_at,
        None
    );
    server.stop().await;
}

#[tokio::test]
async fn original_source_delete_refuses_foreign_studio_without_changing_rows_or_files() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-source-delete-foreign");
    let state = boot("original-source-delete-foreign", &home, None).await;
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let knowledge = state.knowledge();
    // 他 studio 的来源带成员关系与一个已失败任务（在 worker 启动前准备）。
    let foreign_nb = facts.create_notebook("other-studio", "外部笔记本").unwrap();
    let (foreign, snapshot) = facts
        .create_source_with_snapshot(
            "other-studio",
            "pasted_text",
            "外部来源",
            &json!({}),
            b"foreign studio body",
            "text/plain",
        )
        .unwrap();
    facts.add_membership(&foreign_nb.id, &foreign.id).unwrap();
    facts
        .enqueue_source_ingestion(&foreign_nb.id, &foreign.id, None, "cfg")
        .unwrap();
    let claimed = facts.claim_next_job().unwrap().unwrap();
    assert_eq!(claimed.source_id, foreign.id);
    facts.fail_job(&claimed.id, "外部先前失败").unwrap();
    let foreign_job = facts.get_job(&claimed.id).unwrap();
    let local_nb = knowledge.create_notebook("本地").unwrap();
    let local_nb_id = local_nb["id"].as_str().unwrap().to_string();
    let local = knowledge
        .import_pasted_text(&local_nb_id, "# 本地\n\n本地来源正文。", Some("本地来源"))
        .unwrap();
    let local_id = local["source"]["id"].as_str().unwrap().to_string();
    let local_snapshot = facts.latest_snapshot_for_source(&local_id).unwrap();
    let server = serve(state).await;
    wait_until_job(&server, &local_nb_id, &local_id, "pending_embedding").await;
    let foreign_membership = facts.get_membership(&foreign_nb.id, &foreign.id).unwrap();
    let foreign_file = home.join("knowledge").join(&snapshot.storage_path);
    assert!(foreign_file.exists());
    for target in [foreign.id.as_str(), "ksrc_missing"] {
        let (status, error) = server
            .request(
                "DELETE",
                &format!("/lingxi/v1/knowledge/sources/{target}"),
                None,
            )
            .await;
        assert_eq!(status, 404, "{target}: {error}");
        assert_eq!(error["error"], "KNOWLEDGE_NOT_FOUND");
    }
    assert_eq!(facts.get_source(&foreign.id).unwrap(), foreign);
    assert_eq!(
        facts.get_membership(&foreign_nb.id, &foreign.id).unwrap(),
        foreign_membership
    );
    assert_eq!(facts.get_job(&claimed.id).unwrap(), foreign_job);
    assert_eq!(foreign_job.cancelled_at, None);
    assert_eq!(
        facts
            .list_ingestion_jobs(None, Some(&foreign.id), 50)
            .unwrap()
            .len(),
        1
    );
    assert!(foreign_file.exists());
    assert_eq!(
        facts.read_snapshot_bytes(&snapshot).unwrap(),
        b"foreign studio body"
    );
    // 成功路径：本 studio 来源被清理，且不波及他 studio 来源。
    let (status, purge) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/sources/{local_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{purge}");
    assert!(purge["cancelledJobs"].as_u64().unwrap() >= 1);
    assert!(facts.get_source(&local_id).is_err());
    assert!(!home
        .join("knowledge")
        .join(&local_snapshot.storage_path)
        .exists());
    assert!(facts
        .list_ingestion_jobs(None, Some(&local_id), 50)
        .unwrap()
        .is_empty());
    assert_eq!(facts.get_source(&foreign.id).unwrap(), foreign);
    assert!(foreign_file.exists());
    assert_eq!(facts.get_job(&claimed.id).unwrap(), foreign_job);
    server.stop().await;
}

#[tokio::test]
async fn original_reingest_refuses_foreign_and_non_member_sources_without_creating_jobs() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-reingest-membership");
    let state = boot("original-reingest-membership", &home, None).await;
    let studio = state.knowledge().studio_id().to_string();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let knowledge = state.knowledge();
    let nb_a = knowledge.create_notebook("甲").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let nb_b = knowledge.create_notebook("乙").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    // 仅属于 B 的本 studio 来源；曾属于 A 后被移除的来源；他 studio 来源（伪造 A 的 membership 行）。
    let only_b = knowledge
        .import_pasted_text(&nb_b, "# 乙独有\n\n只在乙里。", Some("乙独有"))
        .unwrap()["source"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let removed = knowledge
        .import_pasted_text(&nb_a, "# 被移除\n\n曾在甲里。", Some("被移除"))
        .unwrap()["source"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (foreign, foreign_snapshot) = facts
        .create_source_with_snapshot(
            "other-studio",
            "pasted_text",
            "外部来源",
            &json!({}),
            b"foreign studio body",
            "text/plain",
        )
        .unwrap();
    facts.add_membership(&nb_a, &foreign.id).unwrap();
    let (untracked, _) = facts
        .create_source_with_snapshot(
            &studio,
            "pasted_text",
            "未跟踪",
            &json!({}),
            b"Untracked member body.",
            "text/plain",
        )
        .unwrap();
    facts.add_membership(&nb_a, &untracked.id).unwrap();
    let server = serve(state).await;
    wait_until_job(&server, &nb_b, &only_b, "pending_embedding").await;
    wait_until_job(&server, &nb_a, &removed, "pending_embedding").await;
    facts.remove_source_from_notebook(&nb_a, &removed).unwrap();
    let total_before = facts.list_ingestion_jobs(None, None, 1000).unwrap().len();
    let foreign_before = facts.get_source(&foreign.id).unwrap();
    let membership_before = facts.get_membership(&nb_a, &foreign.id).unwrap();
    let removed_jobs = facts
        .list_ingestion_jobs(Some(&nb_a), Some(&removed), 50)
        .unwrap();
    let only_b_jobs = facts
        .list_ingestion_jobs(Some(&nb_b), Some(&only_b), 50)
        .unwrap();
    let refusals = [
        (nb_a.as_str(), foreign.id.as_str(), "foreign studio source"),
        (
            nb_a.as_str(),
            only_b.as_str(),
            "member of another notebook only",
        ),
        (nb_a.as_str(), removed.as_str(), "membership removed"),
        ("knb_missing", untracked.id.as_str(), "missing notebook"),
        (nb_a.as_str(), "ksrc_missing", "missing source"),
    ];
    for (notebook, source, label) in refusals {
        let (status, error) = server
            .request(
                "POST",
                &format!("/lingxi/v1/knowledge/notebooks/{notebook}/sources/{source}/reingest"),
                None,
            )
            .await;
        assert_eq!(status, 404, "{label}: {error}");
        assert_eq!(error["error"], "KNOWLEDGE_NOT_FOUND", "{label}");
        assert_eq!(
            facts.list_ingestion_jobs(None, None, 1000).unwrap().len(),
            total_before,
            "{label}: 拒绝不能创建任务行"
        );
    }
    assert!(facts
        .list_ingestion_jobs(None, Some(&foreign.id), 50)
        .unwrap()
        .is_empty());
    assert_eq!(
        facts
            .list_ingestion_jobs(Some(&nb_a), Some(&removed), 50)
            .unwrap(),
        removed_jobs
    );
    assert_eq!(
        facts
            .list_ingestion_jobs(Some(&nb_b), Some(&only_b), 50)
            .unwrap(),
        only_b_jobs
    );
    assert!(facts
        .list_ingestion_jobs(Some(&nb_a), Some(&only_b), 50)
        .unwrap()
        .is_empty());
    assert_eq!(facts.get_source(&foreign.id).unwrap(), foreign_before);
    assert_eq!(
        facts.get_membership(&nb_a, &foreign.id).unwrap(),
        membership_before
    );
    assert_eq!(
        facts.read_snapshot_bytes(&foreign_snapshot).unwrap(),
        b"foreign studio body"
    );
    // 成功路径：本 studio 的成员来源（尚无任务）被兜底入队，恰好新增一个任务。
    assert!(facts
        .list_ingestion_jobs(Some(&nb_a), Some(&untracked.id), 50)
        .unwrap()
        .is_empty());
    let (status, created) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{nb_a}/sources/{}/reingest",
                untracked.id
            ),
            None,
        )
        .await;
    assert_eq!(status, 200, "{created}");
    assert_eq!(created["retried"], false);
    assert_eq!(
        facts
            .list_ingestion_jobs(Some(&nb_a), Some(&untracked.id), 50)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        facts.list_ingestion_jobs(None, None, 1000).unwrap().len(),
        total_before + 1
    );
    wait_until_job(&server, &nb_a, &untracked.id, "pending_embedding").await;
    // 非 failed 的最新任务仍是 409 且不新增任务。
    let (status, error) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{nb_a}/sources/{}/reingest",
                untracked.id
            ),
            None,
        )
        .await;
    assert_eq!(status, 409, "{error}");
    assert_eq!(
        facts.list_ingestion_jobs(None, None, 1000).unwrap().len(),
        total_before + 1
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

/// 直接读权威事实库等待另一 studio 的 job 到达目标状态（该 studio 不能经被测 HTTP 面观察）。
async fn wait_for_facts_job(home: &Path, notebook_id: &str, source_id: &str, expect_status: &str) {
    let facts =
        lingxi_adapters::storage::knowledge::KnowledgeFactStore::open(&home.join("knowledge"))
            .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let jobs = facts
            .list_ingestion_jobs(Some(notebook_id), Some(source_id), 200)
            .unwrap();
        if jobs.iter().any(|job| job.status == expect_status) {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!("facts job never reached {expect_status}: {jobs:?}");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

fn job_ids(listing: &Value) -> Vec<String> {
    let mut ids = listing["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| job["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn counts_total(listing: &Value) -> u64 {
    listing["counts"]
        .as_object()
        .unwrap()
        .values()
        .map(|count| count.as_u64().unwrap())
        .sum()
}

#[tokio::test]
async fn original_ingestion_listing_is_studio_scoped_and_reports_unreachable_sources() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("original-ingestion-listing");
    let state = boot("original-ingestion-listing", &home, None).await;
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let docs = unique_dir("original-ingestion-listing-files");
    let own_file = docs.join("own-original.md");
    let foreign_file = docs.join("foreign-original.md");
    std::fs::write(&own_file, "# 本地原文\n\n本 studio 的可达原文件。").unwrap();
    std::fs::write(&foreign_file, "# 外部原文\n\n外部 studio 的原文件。").unwrap();
    let knowledge = state.knowledge().clone();
    let notebook = knowledge.create_notebook("任务列表甲").unwrap();
    let nb = notebook["id"].as_str().unwrap().to_string();
    let second_notebook = knowledge.create_notebook("任务列表丙").unwrap();
    let nb2 = second_notebook["id"].as_str().unwrap().to_string();
    let file_import = knowledge
        .import_file(&nb, own_file.to_str().unwrap(), Some("本地文件"))
        .unwrap();
    let file_source = file_import["source"]["id"].as_str().unwrap().to_string();
    let pasted = knowledge
        .import_pasted_text(&nb, "仅粘贴文本，没有原文件路径。", Some("粘贴"))
        .unwrap();
    let pasted_source = pasted["source"]["id"].as_str().unwrap().to_string();
    let other_nb_source = knowledge
        .import_pasted_text(&nb2, "第二个本地笔记本的来源。", Some("丙来源"))
        .unwrap()["source"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let server = serve(state.clone()).await;
    for (notebook_id, source_id) in [
        (&nb, &file_source),
        (&nb, &pasted_source),
        (&nb2, &other_nb_source),
    ] {
        wait_until_job(&server, notebook_id, source_id, "pending_embedding").await;
    }
    // 第二个 studio 的真实笔记本/来源/任务（含其原文件路径）。
    let foreign_knowledge =
        lingxi_service::knowledge::KnowledgeService::new(&home, "foreign-studio".into()).unwrap();
    let foreign_nb = foreign_knowledge.create_notebook("外部笔记本").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let foreign_import = foreign_knowledge
        .import_file(
            &foreign_nb,
            foreign_file.to_str().unwrap(),
            Some("外部文件"),
        )
        .unwrap();
    let foreign_source = foreign_import["source"]["id"].as_str().unwrap().to_string();
    wait_for_facts_job(&home, &foreign_nb, &foreign_source, "pending_embedding").await;
    let foreign_jobs_before = facts
        .list_ingestion_jobs(Some(&foreign_nb), None, 200)
        .unwrap();
    assert_eq!(foreign_jobs_before.len(), 1, "外部 studio 必须有真实 job");
    let all_jobs_before = facts.list_ingestion_jobs(None, None, 500).unwrap();
    assert_eq!(all_jobs_before.len(), 4, "库中共 3 个本地 + 1 个外部 job");

    let list = |query: String| {
        let server = &server;
        async move {
            let (status, body) = server
                .request(
                    "GET",
                    &format!("/lingxi/v1/knowledge/ingestion{query}"),
                    None,
                )
                .await;
            assert_eq!(status, 200, "{body}");
            body
        }
    };
    // 成功：无筛选 → 仅本 studio 的 3 个 job 与对应计数，外部任何 id 都不出现。
    let all = list(String::new()).await;
    assert_eq!(all["jobs"].as_array().unwrap().len(), 3, "{all}");
    assert_eq!(all["counts"]["pending_embedding"], 3);
    assert_eq!(counts_total(&all), 3);
    let mut stored_own = all_jobs_before
        .iter()
        .filter(|job| job.notebook_id != foreign_nb)
        .map(|job| job.id.clone())
        .collect::<Vec<_>>();
    stored_own.sort();
    assert_eq!(
        job_ids(&all),
        stored_own,
        "返回的正是本 studio 实际入库的任务"
    );
    for job in all["jobs"].as_array().unwrap() {
        assert!([&nb, &nb2].contains(&&job["notebookId"].as_str().unwrap().to_string()));
    }
    let rendered = all.to_string();
    for foreign_id in [&foreign_nb, &foreign_source, &foreign_jobs_before[0].id] {
        assert!(!rendered.contains(foreign_id.as_str()), "{rendered}");
    }
    assert_eq!(all["unreachableSources"], json!([]), "原文件都可达");

    // 边界：notebookId / sourceId 筛选。
    let by_notebook = list(format!("?notebookId={nb}")).await;
    assert_eq!(by_notebook["jobs"].as_array().unwrap().len(), 2);
    assert!(by_notebook["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|job| job["notebookId"] == nb.as_str()));
    assert_eq!(counts_total(&by_notebook), 2);
    let by_source = list(format!("?sourceId={file_source}")).await;
    assert_eq!(by_source["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(by_source["jobs"][0]["sourceId"], file_source.as_str());
    let by_both = list(format!("?notebookId={nb2}&sourceId={other_nb_source}")).await;
    assert_eq!(by_both["jobs"].as_array().unwrap().len(), 1);
    let mismatch = list(format!("?notebookId={nb2}&sourceId={file_source}")).await;
    assert_eq!(mismatch["jobs"], json!([]));

    // 隔离：外部 notebook / source 与不存在的 id 同样得到空 jobs、零计数，不披露存在性。
    let foreign_by_notebook = list(format!("?notebookId={foreign_nb}")).await;
    let missing_by_notebook = list("?notebookId=notebook-never-existed".into()).await;
    assert_eq!(foreign_by_notebook["jobs"], json!([]));
    assert_eq!(counts_total(&foreign_by_notebook), 0);
    assert_eq!(foreign_by_notebook["jobs"], missing_by_notebook["jobs"]);
    assert_eq!(foreign_by_notebook["counts"], missing_by_notebook["counts"]);
    let foreign_by_source = list(format!("?sourceId={foreign_source}")).await;
    let missing_by_source = list("?sourceId=source-never-existed".into()).await;
    assert_eq!(foreign_by_source["jobs"], json!([]));
    assert_eq!(foreign_by_source["jobs"], missing_by_source["jobs"]);
    let foreign_both = list(format!(
        "?notebookId={foreign_nb}&sourceId={foreign_source}"
    ))
    .await;
    assert_eq!(foreign_both["jobs"], json!([]));
    assert_eq!(counts_total(&foreign_both), 0);
    // 外部 studio 的计数/任务不被任何查询改动，也没有新增任务。
    assert_eq!(
        facts
            .list_ingestion_jobs(Some(&foreign_nb), None, 200)
            .unwrap(),
        foreign_jobs_before
    );
    assert_eq!(
        facts.list_ingestion_jobs(None, None, 500).unwrap(),
        all_jobs_before
    );

    // 不可达：本地与外部原文件都被删除；只列本 studio 的 file 来源，且是显式标注的状态。
    std::fs::remove_file(&own_file).unwrap();
    std::fs::remove_file(&foreign_file).unwrap();
    let unreachable = list(String::new()).await;
    let entries = unreachable["unreachableSources"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{unreachable}");
    assert_eq!(entries[0]["sourceId"], file_source.as_str());
    assert_eq!(entries[0]["studioId"], knowledge.studio_id());
    assert_eq!(entries[0]["notebooks"], json!([nb]));
    assert_eq!(entries[0]["unreachable"], true);
    assert_eq!(entries[0]["watching"], false);
    assert_eq!(entries[0]["detection"], "on_demand_stat");
    assert!(entries[0]["unreachableSince"].is_null());
    assert!(entries[0]["unreachableReason"]
        .as_str()
        .unwrap()
        .starts_with("ENOENT:"));
    let rendered = unreachable.to_string();
    for foreign_id in [&foreign_nb, &foreign_source] {
        assert!(!rendered.contains(foreign_id.as_str()), "{rendered}");
    }
    assert!(!rendered.contains(foreign_file.to_str().unwrap()));
    assert!(
        !rendered.contains(own_file.to_str().unwrap()),
        "不披露原路径"
    );
    // 筛选不影响不可达清单（原路由同口径），jobs 仍按筛选收窄。
    let filtered = list(format!("?notebookId={nb2}")).await;
    assert_eq!(filtered["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(
        filtered["unreachableSources"],
        unreachable["unreachableSources"]
    );
    // 原文件恢复 → 状态实时回到可达，不留陈旧标记。
    std::fs::write(&own_file, "# 本地原文\n\n恢复后的原文件。").unwrap();
    assert_eq!(list(String::new()).await["unreachableSources"], json!([]));
    // 整个读取期间没有写入任何 job。
    assert_eq!(
        facts.list_ingestion_jobs(None, None, 500).unwrap(),
        all_jobs_before
    );
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_outline_sections_drive_knowledge_read_and_refuse_out_of_scope() {
    use lingxi_adapters::storage::knowledge::{KnowledgeFactStore, KnowledgeIndexStore};
    use std::future::IntoFuture;
    let home = unique_dir("original-outline-read");
    let docs = unique_dir("original-outline-read-files");
    let paragraph =
        |mark: &str, index: usize| format!("{mark}_P{index} {}", "章节内的连续原文。".repeat(70));
    let section = |title: &str, mark: &str, paragraphs: usize| {
        let body = (0..paragraphs)
            .map(|index| paragraph(mark, index))
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("# {title}\n\n{body}")
    };
    let document = [
        section("第一章 灵犀架构", "SEC_ONE", 6),
        section("第二章 检索", "SEC_TWO", 2),
        section("第三章 引用", "SEC_THREE", 2),
    ]
    .join("\n\n");
    let main_file = docs.join("main.md");
    std::fs::write(&main_file, &document).unwrap();
    let second_file = docs.join("second.md");
    std::fs::write(
        &second_file,
        section("同笔记本另一来源", "SECOND_SOURCE", 2),
    )
    .unwrap();
    let empty_file = docs.join("empty.md");
    std::fs::write(&empty_file, "\n\n   \n").unwrap();

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
    let server = serve(state.clone()).await;
    let nb = knowledge.create_notebook("导航甲").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    // 每次入库后等后台任务稳定再继续写入：知识库 SQLite 不设忙等待，准备阶段不得与后台写入争用。
    let main = knowledge
        .import_file(&nb, main_file.to_str().unwrap(), Some("主来源"))
        .unwrap();
    let src = main["source"]["id"].as_str().unwrap().to_string();
    let main_artifact = main["parseArtifact"]["id"].as_str().unwrap().to_string();
    wait_until_job(&server, &nb, &src, "pending_embedding").await;
    let second = knowledge
        .import_file(&nb, second_file.to_str().unwrap(), Some("同笔记本未选来源"))
        .unwrap();
    let second_src = second["source"]["id"].as_str().unwrap().to_string();
    let second_artifact = second["parseArtifact"]["id"].as_str().unwrap().to_string();
    wait_until_job(&server, &nb, &second_src, "pending_embedding").await;
    // 空文档：解析被显式拒绝，来源仍在笔记本内，没有可导航章节。
    let empty_error = knowledge
        .import_file(&nb, empty_file.to_str().unwrap(), Some("空文档"))
        .unwrap_err();
    assert_eq!(empty_error.code, "KNOWLEDGE_PARSE_FAILED");
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let empty_src = facts
        .list_notebook_sources(&nb)
        .unwrap()
        .into_iter()
        .find(|entry| entry.source.display_name == "空文档")
        .expect("空文档来源已登记")
        .source
        .id;
    let foreign_knowledge =
        lingxi_service::knowledge::KnowledgeService::new(&home, "foreign-studio".into()).unwrap();
    let foreign_nb = foreign_knowledge.create_notebook("导航乙").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let foreign = foreign_knowledge
        .import_pasted_text(
            &foreign_nb,
            &format!("# 外部章节\n\nFOREIGN_B_MARK {}", "外部原文。".repeat(40)),
            Some("外部来源"),
        )
        .unwrap();
    let foreign_src = foreign["source"]["id"].as_str().unwrap().to_string();
    let foreign_artifact = foreign["parseArtifact"]["id"].as_str().unwrap().to_string();
    wait_for_facts_job(&home, &foreign_nb, &foreign_src, "pending_embedding").await;

    let index = KnowledgeIndexStore::open(&home.join("knowledge")).unwrap();
    let ready = |artifact: &str| {
        let variants = index.list_ready_variant_metadata(artifact).unwrap();
        assert_eq!(variants.len(), 1);
        index.list_variant_chunks(&variants[0].0).unwrap()
    };
    let sections = index
        .list_artifact_section_metadata(&main_artifact)
        .unwrap();
    let chunks = ready(&main_artifact);
    assert_eq!(sections.len(), 3, "三章各一节：{sections:?}");
    let (section_one, section_two) = (&sections[0], &sections[1]);
    let section_one_chunks = chunks
        .iter()
        .filter(|chunk| chunk.section_id.as_deref() == Some(section_one.id.as_str()))
        .collect::<Vec<_>>();
    assert!(section_one_chunks.len() >= 2, "第一章必须跨多个片段");
    let first_chunk = section_one_chunks[0];
    let middle_position = chunks
        .iter()
        .position(|chunk| chunk.chunk_id == section_one_chunks[1].chunk_id)
        .unwrap();
    let second_section = index
        .list_artifact_section_metadata(&second_artifact)
        .unwrap()[0]
        .clone();
    let second_chunk = ready(&second_artifact)[0].clone();
    let foreign_section = index
        .list_artifact_section_metadata(&foreign_artifact)
        .unwrap()[0]
        .clone();
    let foreign_chunk = ready(&foreign_artifact)[0].clone();
    let rows_before = (
        facts.list_notebook_sources(&nb).unwrap().len(),
        facts.list_ingestion_jobs(None, None, 500).unwrap(),
    );

    let read = |args: Value| ("knowledge_read".to_string(), args);
    let outline = |args: Value| ("knowledge_outline".to_string(), args);
    script.lock().unwrap().before_read = Some(Arc::new(|| {}));
    script.lock().unwrap().calls = vec![
        outline(json!({"scopeId":"runtime","sourceId":src})), // 0
        read(
            json!({"scopeId":"runtime","sourceId":src,"sectionId":section_two.id,"maxChars":8000}),
        ), // 1
        read(
            json!({"scopeId":"runtime","sourceId":src,"sectionId":section_one.id,"maxChars":8000}),
        ), // 2
        read(
            json!({"scopeId":"runtime","sourceId":src,"aroundChunkId":first_chunk.chunk_id,"maxChars":8000}),
        ), // 3
        read(
            json!({"scopeId":"runtime","sourceId":src,"aroundChunkId":section_one_chunks[1].chunk_id,"neighborWindow":0,"maxChars":8000}),
        ), // 4
        read(
            json!({"scopeId":"runtime","sourceId":src,"aroundChunkId":section_one_chunks[1].chunk_id,"neighborWindow":1,"maxChars":8000}),
        ), // 5
        read(json!({"scopeId":"runtime","sourceId":src,"sectionId":second_section.id})), // 6
        read(json!({"scopeId":"runtime","sourceId":src,"sectionId":foreign_section.id})), // 7
        read(json!({"scopeId":"runtime","sourceId":src,"aroundChunkId":foreign_chunk.chunk_id})), // 8
        read(json!({"scopeId":"runtime","sourceId":src,"aroundChunkId":second_chunk.chunk_id})), // 9
        read(json!({"scopeId":"runtime","sourceId":src,"sectionId":"section-never-existed"})), // 10
        read(
            json!({"scopeId":"runtime","sourceId":src,"sectionId":section_one.id,"aroundChunkId":first_chunk.chunk_id}),
        ), // 11
        read(
            json!({"scopeId":"runtime","sourceId":src,"sectionId":section_one.id,"fromOrdinal":1}),
        ), // 12
        read(json!({"scopeId":"runtime","sourceId":src,"neighborWindow":2})),                  // 13
        read(json!({"scopeId":"runtime","sourceId":second_src,"sectionId":second_section.id})), // 14
        read(json!({"scopeId":"runtime","sourceId":src,"sectionId":section_one.id,"maxChars":256})), // 15
        read(
            json!({"scopeId":"runtime","sourceId":src,"sectionId":section_two.id,"offset":999999}),
        ), // 16
        outline(json!({"scopeId":"runtime","sourceId":foreign_src})), // 17
        outline(json!({"scopeId":"runtime","sourceId":second_src})),  // 18
        outline(json!({"scopeId":"runtime","notebookId":foreign_nb})), // 19
        outline(json!({"scopeId":"runtime","notebookId":"notebook-never-existed"})), // 20
        outline(json!({"scopeId":"runtime","sourceId":"source-never-existed"})), // 21
        outline(json!({"scopeId":"runtime","notebookId":nb,"sourceId":foreign_src})), // 22
        outline(json!({"scopeId":"runtime","sourceId":empty_src})),   // 23
        read(json!({"scopeId":"runtime","sourceId":empty_src,"sectionId":section_one.id})), // 24
        outline(json!({"scopeId":"runtime"})),                        // 25
    ];
    let (status, executed) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"核对章节导航与范围拒绝","knowledge":{"notebookIds":[nb],"sourceIds":[src, empty_src]}})),
        )
        .await;
    assert_eq!(status, 200, "{executed}");
    let requests = script.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 26 * 2 + 1);
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
    let blocks = facts.list_artifact_blocks(&main_artifact).unwrap();
    let block_text = |block_id: &str| blocks.iter().find(|block| block.id == block_id).unwrap();
    let joined = |value: &Value| {
        value["spans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|span| span["text"].as_str().unwrap())
            .collect::<String>()
    };
    let expected_range = |start: usize, end: usize| {
        blocks
            .iter()
            .filter(|block| (start..=end).contains(&block.ordinal))
            .map(|block| block.text.as_str())
            .collect::<String>()
    };

    // 成功：outline 给出可导航章节与块范围，且受冻结 scope 约束（只有主来源）。
    let listed: Value = serde_json::from_str(&result(0)).unwrap();
    assert_eq!(listed["notebooks"].as_array().unwrap().len(), 1);
    assert_eq!(
        listed["notebooks"][0]["sources"].as_array().unwrap().len(),
        1
    );
    let listed_sections = listed["notebooks"][0]["sources"][0]["sections"]
        .as_array()
        .unwrap();
    assert_eq!(listed_sections.len(), 3);
    assert_eq!(listed_sections[1]["sectionId"], section_two.id.as_str());
    assert_eq!(
        listed_sections[1]["startBlockOrdinal"],
        section_two.start_block_ordinal
    );
    assert_eq!(
        listed_sections[1]["endBlockOrdinal"],
        section_two.end_block_ordinal
    );
    assert_eq!(listed_sections[1]["headingPath"], json!(["第二章 检索"]));
    // outline 的 sectionId 直接用于 read：只读该章块范围，不夹带其它章节/来源。
    let read_two: Value = serde_json::from_str(&result(1)).unwrap();
    assert_eq!(read_two["mode"], "section");
    assert_eq!(read_two["sectionId"], section_two.id.as_str());
    assert_eq!(read_two["parentSectionHeading"], json!(["第二章 检索"]));
    assert_eq!(read_two["sourceId"], src.as_str());
    assert_eq!(read_two["truncated"], false);
    let text_two = joined(&read_two);
    assert_eq!(
        text_two,
        expected_range(
            section_two.start_block_ordinal,
            section_two.end_block_ordinal
        )
    );
    assert!(text_two.contains("SEC_TWO_P0") && text_two.contains("SEC_TWO_P1"));
    for other in ["SEC_ONE", "SEC_THREE", "SECOND_SOURCE", "FOREIGN_B"] {
        assert!(!text_two.contains(other), "章节读取越界读到 {other}");
    }
    for span in read_two["spans"].as_array().unwrap() {
        let ordinal = span["blockOrdinal"].as_u64().unwrap() as usize;
        assert!(
            (section_two.start_block_ordinal..=section_two.end_block_ordinal).contains(&ordinal)
        );
        let block = block_text(span["blockId"].as_str().unwrap());
        assert_eq!(block.parse_artifact_id, main_artifact);
        assert!(span["citationMarkdown"]
            .as_str()
            .unwrap()
            .contains(span["citationId"].as_str().unwrap()));
    }
    let read_one: Value = serde_json::from_str(&result(2)).unwrap();
    let text_one = joined(&read_one);
    assert_eq!(
        text_one,
        expected_range(
            section_one.start_block_ordinal,
            section_one.end_block_ordinal
        )
    );
    assert!(text_one.contains("SEC_ONE_P5") && !text_one.contains("SEC_TWO"));
    // aroundChunkId 缺省窗口 → 恢复到所在父章节；显式窗口 → 精读相邻片段。
    let around_section: Value = serde_json::from_str(&result(3)).unwrap();
    assert_eq!(around_section["mode"], "section");
    assert_eq!(
        around_section["aroundChunkId"],
        first_chunk.chunk_id.as_str()
    );
    assert_eq!(joined(&around_section), text_one);
    let chunk_text = |chunk: &lingxi_adapters::storage::knowledge::StoredChunk| {
        chunk
            .spans
            .iter()
            .map(|span| {
                block_text(&span.block_id)
                    .text
                    .chars()
                    .skip(span.block_start_offset)
                    .take(span.block_end_offset - span.block_start_offset)
                    .collect::<String>()
            })
            .collect::<String>()
    };
    let around_only: Value = serde_json::from_str(&result(4)).unwrap();
    assert_eq!(around_only["mode"], "around-chunk");
    assert_eq!(around_only["neighborWindow"], 0);
    assert_eq!(
        around_only["requestedRange"],
        json!([middle_position + 1, middle_position + 1])
    );
    assert_eq!(joined(&around_only), chunk_text(section_one_chunks[1]));
    assert!(joined(&around_only).len() < text_one.len());
    let around_window: Value = serde_json::from_str(&result(5)).unwrap();
    assert_eq!(around_window["mode"], "around-chunk");
    assert_eq!(
        around_window["requestedRange"],
        json!([middle_position, middle_position + 2])
    );
    assert_eq!(around_window["totalChunks"], chunks.len());
    assert!(joined(&around_window).len() > joined(&around_only).len());
    assert!(joined(&around_window).contains(&chunk_text(section_one_chunks[1])));
    // 续读：同一章节分页，offset 之间无重叠/遗漏。
    let paged: Value = serde_json::from_str(&result(15)).unwrap();
    assert_eq!(paged["mode"], "section");
    assert_eq!(paged["returnedChars"], 256);
    assert_eq!(paged["truncated"], true);
    assert_eq!(paged["nextOffset"], 256);
    assert_eq!(paged["totalChars"], text_one.chars().count());
    assert_eq!(
        joined(&paged),
        text_one.chars().take(256).collect::<String>()
    );

    // 跨来源隔离：同笔记本未选来源、外部 studio、不存在的章节/片段一律范围违规，
    // 且错误文本不泄露其原文。
    for index in [6, 7, 8, 9, 10, 14] {
        let message = result(index);
        assert!(
            message.contains("KNOWLEDGE_SCOPE_VIOLATION"),
            "{index}: {message}"
        );
        for leaked in ["SECOND_SOURCE", "FOREIGN_B", "\"spans\""] {
            assert!(!message.contains(leaked), "{index}: {message}");
        }
    }
    // 选择器冲突/非法 → 参数错误，不静默降级为整源读取。
    for index in [11, 12, 13] {
        let message = result(index);
        assert!(
            message.contains("KNOWLEDGE_INVALID_ARGUMENT"),
            "{index}: {message}"
        );
        assert!(!message.contains("\"spans\""), "{index}: {message}");
    }
    assert!(
        result(16).contains("KNOWLEDGE_INVALID_ARGUMENT"),
        "{}",
        result(16)
    );

    // outline 范围拒绝：未选来源/外部来源/外部笔记本/不存在 id 都是错误而不是空成功。
    for index in [17, 18, 19, 20, 21, 22] {
        let message = result(index);
        assert!(
            message.contains("KNOWLEDGE_SCOPE_VIOLATION"),
            "{index}: {message}"
        );
        assert!(!message.contains("\"notebooks\""), "{index}: {message}");
        for leaked in ["同笔记本未选来源", "外部来源", "导航乙"] {
            assert!(!message.contains(leaked), "{index}: {message}");
        }
    }
    // 空文档只返回空大纲（来源在、无章节、无片段），而不是错误；读取章节则显式失败。
    let empty_outline: Value = serde_json::from_str(&result(23)).unwrap();
    assert_eq!(empty_outline["notebooks"].as_array().unwrap().len(), 1);
    let empty_sources = empty_outline["notebooks"][0]["sources"].as_array().unwrap();
    assert_eq!(empty_sources.len(), 1);
    assert_eq!(empty_sources[0]["sourceId"], empty_src.as_str());
    assert_eq!(empty_sources[0]["sections"], json!([]));
    assert_eq!(empty_sources[0]["chunkCount"], 0);
    assert!(
        result(24).contains("KNOWLEDGE_PARSE_NOT_READY"),
        "{}",
        result(24)
    );
    let whole: Value = serde_json::from_str(&result(25)).unwrap();
    let whole_sources = whole["notebooks"][0]["sources"].as_array().unwrap();
    assert_eq!(whole_sources.len(), 2, "scope 内正好两个来源");
    assert!(whole_sources
        .iter()
        .all(|entry| entry["sourceId"] == src.as_str() || entry["sourceId"] == empty_src.as_str()));
    // 读取与拒绝都没有改动来源或摄入任务。
    assert_eq!(
        (
            facts.list_notebook_sources(&nb).unwrap().len(),
            facts.list_ingestion_jobs(None, None, 500).unwrap()
        ),
        rows_before
    );
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn original_grep_filters_reject_invalid_values_and_apply_heading_prefix() {
    use std::future::IntoFuture;
    let home = unique_dir("original-grep-filters");
    let docs = unique_dir("original-grep-filters-files");
    let file = docs.join("grep.md");
    std::fs::write(
        &file,
        "# 原章节\n\nGREP_FILTER_ALPHA 位于原章节。\n\n# 另一章\n\nGREP_FILTER_ALPHA 位于另一章。",
    )
    .unwrap();
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
    let nb = knowledge.create_notebook("过滤甲").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let src = knowledge
        .import_file(&nb, file.to_str().unwrap(), Some("过滤原文"))
        .unwrap()["source"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let server = serve(state.clone()).await;
    wait_until_job(&server, &nb, &src, "pending_embedding").await;
    let facts =
        lingxi_adapters::storage::knowledge::KnowledgeFactStore::open(&home.join("knowledge"))
            .unwrap();
    let jobs_before = facts.list_ingestion_jobs(None, None, 500).unwrap();
    let grep = |args: Value| ("knowledge_grep".to_string(), args);
    script.lock().unwrap().calls = vec![
        grep(
            json!({"scopeId":"runtime","pattern":"GREP_FILTER_ALPHA","headingFilter":"章".repeat(257)}),
        ), // 0
        grep(json!({"scopeId":"runtime","pattern":"GREP_FILTER_ALPHA","sourceIds":[" "]})), // 1
        grep(json!({"scopeId":"runtime","pattern":"GREP_FILTER_ALPHA","headingFilter":"原章"})), // 2
        grep(
            json!({"scopeId":"runtime","pattern":"GREP_FILTER_ALPHA","headingFilter":"  原章节  ","sourceIds":[src]}),
        ), // 3
        grep(json!({"scopeId":"runtime","pattern":"GREP_FILTER_ALPHA"})), // 4
        grep(
            json!({"scopeId":"runtime","pattern":"GREP_FILTER_ALPHA","headingFilter":"章".repeat(256)}),
        ), // 5
        grep(json!({"scopeId":"runtime","pattern":"x".repeat(513)})),     // 6
    ];
    let (status, executed) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(json!({"input":"核对 grep 过滤器","knowledge":{"notebookIds":[nb],"sourceIds":[src]}})),
        )
        .await;
    assert_eq!(status, 200, "{executed}");
    let requests = script.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 7 * 2 + 1);
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
    for index in [0, 1, 6] {
        let message = result(index);
        assert!(
            message.contains("KNOWLEDGE_INVALID_ARGUMENT"),
            "{index}: {message}"
        );
        assert!(
            !message.contains("GREP_FILTER_ALPHA 位于"),
            "{index}: {message}"
        );
        assert!(!message.contains("\"matches\""), "{index}: {message}");
    }
    assert!(
        result(0).contains("headingFilter exceeds the 256"),
        "{}",
        result(0)
    );
    assert!(
        result(1).contains("sourceIds must be a non-empty array"),
        "{}",
        result(1)
    );
    // 成功边界：标题前缀只保留原章节命中；trim 后同口径；恰好 256 字符合法但无命中。
    for index in [2, 3] {
        let filtered: Value = serde_json::from_str(&result(index)).unwrap();
        let matches = filtered["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1, "{index}: {filtered}");
        assert_eq!(matches[0]["sourceId"], src.as_str());
        assert_eq!(matches[0]["headingPath"], json!(["原章节"]));
        assert!(matches[0]["snippet"]
            .as_str()
            .unwrap()
            .contains("位于原章节"));
    }
    let unfiltered: Value = serde_json::from_str(&result(4)).unwrap();
    assert_eq!(unfiltered["matches"].as_array().unwrap().len(), 2);
    let boundary: Value = serde_json::from_str(&result(5)).unwrap();
    assert_eq!(boundary["matches"], json!([]));
    assert_eq!(
        facts.list_ingestion_jobs(None, None, 500).unwrap(),
        jobs_before
    );
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

async fn boot_chat(home: &Path, endpoint: &str) -> ServiceState {
    boot_chat_with_embedding(home, endpoint, false).await
}

async fn boot_chat_with_embedding(home: &Path, endpoint: &str, embedding: bool) -> ServiceState {
    let layout = prepare_layout(home).unwrap();
    // 产品模板属于本用例的隔离材料，初始化不得改写仓库里的现役模板。
    let product = home.join("product-materials");
    let originals = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib");
    for name in ["identity-templates", "agents-templates", "yuan"] {
        let destination = product.join(name);
        std::fs::create_dir_all(&destination).unwrap();
        for entry in std::fs::read_dir(originals.join(name)).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                std::fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
            }
        }
    }
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
            product_material_dir: Some(product),
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
    wait_for_facts_job(&home, other_id, foreign, "pending_embedding").await;
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
    wait_for_facts_job(&home, foreign_id, source_id, "pending_embedding").await;
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

#[tokio::test]
async fn regex_grep_current_run_uses_real_patterns_precise_unicode_offsets_and_rejects_invalid() {
    use std::future::IntoFuture;
    let home = unique_dir("regex-grep-current");
    let docs = unique_dir("regex-grep-current-source");
    let file = docs.join("patterns.md");
    let original = "# 模式原文\n\n前缀💡 编号A12与B003\nliteral.*marker";
    std::fs::write(&file, original).unwrap();
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
    let nb = knowledge.create_notebook("正则真实原文").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let src = knowledge
        .import_file(&nb, file.to_str().unwrap(), Some("模式原文"))
        .unwrap()["source"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let server = serve(state.clone()).await;
    wait_until_job(&server, &nb, &src, "pending_embedding").await;
    let facts =
        lingxi_adapters::storage::knowledge::KnowledgeFactStore::open(&home.join("knowledge"))
            .unwrap();
    let jobs_before = facts.list_ingestion_jobs(None, None, 500).unwrap();
    let call = |args: Value| ("knowledge_grep".to_string(), args);
    script.lock().unwrap().calls = vec![
        call(json!({"scopeId":"runtime","pattern":"(?:A[0-9]{2}|B[0-9]{3})","regexp":true})),
        call(json!({"scopeId":"runtime","pattern":"(?:A[0-9]{2}|B[0-9]{3})","regex":true})),
        call(json!({"scopeId":"runtime","pattern":"[","regexp":true})),
        call(json!({"scopeId":"runtime","pattern":"literal.*marker","regexp":false})),
        call(json!({"scopeId":"runtime","pattern":"$","regexp":true})),
        call(json!({"scopeId":"runtime","pattern":"A12","regexp":true,"regex":false})),
    ];
    let (status, executed) = server.request("POST", "/lingxi/v1/sessions/sess_local_alpha/execute", Some(json!({"input":"按真实正则扫描冻结原文","knowledge":{"notebookIds":[nb],"sourceIds":[src]}}))).await;
    assert_eq!(status, 200, "{executed}");
    let requests = script.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 13);
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
    let first: Value = serde_json::from_str(&result(0)).expect("现役regexp参数必须真正执行正则");
    let compatible: Value = serde_json::from_str(&result(1)).unwrap();
    assert_eq!(first, compatible, "旧regex别名保持同一真实语义");
    let hits = first["matches"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "{first}");
    for (hit, expected) in hits.iter().zip(["A12", "B003"]) {
        assert_eq!(hit["sourceId"], src.as_str());
        let artifact = hit["parseArtifactId"].as_str().unwrap();
        let blocks = facts.list_artifact_blocks(artifact).unwrap();
        let block = blocks
            .iter()
            .find(|b| b.id == hit["blockId"].as_str().unwrap())
            .unwrap();
        let chars: Vec<char> = block.text.chars().collect();
        let start = hit["startOffset"].as_u64().unwrap() as usize;
        let end = hit["endOffset"].as_u64().unwrap() as usize;
        assert_eq!(
            chars[start..end].iter().collect::<String>(),
            expected,
            "Unicode前缀后位置必须回指真实原文"
        );
        assert!(start > 0);
    }
    for index in [2, 5] {
        let error = result(index);
        assert!(
            error.contains("KNOWLEDGE_INVALID_ARGUMENT"),
            "{index}: {error}"
        );
        assert!(!error.contains("\"matches\""));
    }
    let literal: Value = serde_json::from_str(&result(3)).unwrap();
    assert_eq!(literal["matches"].as_array().unwrap().len(), 1);
    let zero: Value = serde_json::from_str(&result(4)).unwrap();
    let zero_hits = zero["matches"].as_array().unwrap();
    assert!(!zero_hits.is_empty());
    assert_eq!(
        zero["truncated"], false,
        "零宽匹配必须推进，不能重复填满结果"
    );
    for hit in zero_hits {
        assert_eq!(hit["startOffset"], hit["endOffset"]);
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    assert_eq!(
        facts.list_ingestion_jobs(None, None, 500).unwrap(),
        jobs_before
    );
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test]
async fn ingested_overlapping_chunks_preserve_gap_text_and_real_run_can_retrieve_it() {
    use lingxi_adapters::storage::knowledge::{KnowledgeFactStore, KnowledgeIndexStore};
    use std::future::IntoFuture;
    let home = unique_dir("overlap-real-run");
    // 前一片段恰好在100字符处结束，旧错误步进会跳过紧随其后的整个检索词。
    let original = format!(
        "{}原空隙必检索{}",
        "a".repeat(100),
        "0123456789汉💡".repeat(40)
    );
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
    let notebook = knowledge.create_notebook("全部原文必须可检索").unwrap();
    let nb = notebook["id"].as_str().unwrap();
    let server = serve(state.clone()).await;
    let (status, configured) = server
        .request(
            "PUT",
            &format!("/lingxi/v1/knowledge/notebooks/{nb}/settings"),
            Some(json!({"chunkTargetChars":100})),
        )
        .await;
    assert_eq!(status, 200, "{configured}");
    let imported = knowledge
        .import_pasted_text(nb, &original, Some("空隙原文"))
        .unwrap();
    let src = imported["source"]["id"].as_str().unwrap();
    let artifact = imported["parseArtifact"]["id"].as_str().unwrap();
    wait_until_job(&server, nb, src, "pending_embedding").await;
    let (status, cards) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/parse-artifacts/{artifact}/chunks"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{cards}");
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let index = KnowledgeIndexStore::open(&home.join("knowledge")).unwrap();
    // 原文卡片采用自动预览配置；本例必须检查实际100字符配置摄入的持久索引。
    let variants = index.list_ready_variant_metadata(artifact).unwrap();
    assert_eq!(variants.len(), 1);
    let chunks = index.list_variant_chunks(&variants[0].0).unwrap();
    assert_eq!(chunks.len(), variants[0].1);
    assert!(chunks.len() > 3);
    for block in facts.list_artifact_blocks(artifact).unwrap() {
        let chars: Vec<char> = block.text.chars().collect();
        let mut covered = vec![false; chars.len()];
        for chunk in &chunks {
            for span in chunk.spans.iter().filter(|span| span.block_id == block.id) {
                covered[span.block_start_offset..span.block_end_offset].fill(true);
                assert_eq!(
                    chunk
                        .text
                        .chars()
                        .skip(span.chunk_start_offset)
                        .take(span.chunk_end_offset - span.chunk_start_offset)
                        .collect::<String>(),
                    chars[span.block_start_offset..span.block_end_offset]
                        .iter()
                        .collect::<String>()
                );
            }
        }
        assert!(
            covered.into_iter().all(|value| value),
            "真实摄入后持久片段不得遗漏原文字符"
        );
    }
    script.lock().unwrap().calls = vec![(
        "knowledge_search".into(),
        json!({"scopeId":"runtime","query":"原空隙必检索","channel":"fts"}),
    )];
    let (status, result) = server.request("POST", "/lingxi/v1/sessions/sess_local_alpha/execute", Some(json!({"input":"检索此前空隙内的原文","knowledge":{"notebookIds":[nb],"sourceIds":[src]}}))).await;
    assert_eq!(status, 200, "{result}");
    let requests = script.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 3);
    let body = requests.last().unwrap();
    let delivered = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == "original-result-0")
        .map(proof_content)
        .unwrap();
    let found: Value = serde_json::from_str(&delivered).unwrap();
    assert!(!found["hits"].as_array().unwrap().is_empty(), "{found}");
    assert!(delivered.contains("原空隙必检索"), "{delivered}");
    assert!(delivered.contains(src));
    let snapshot = imported["snapshot"]["id"].as_str().unwrap();
    let (status, _, bytes) = server
        .request_raw(
            "GET",
            &format!("/lingxi/v1/knowledge/snapshots/{snapshot}/content"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(bytes, original.as_bytes());
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn legacy_cache_requires_explicit_rebuild_before_paid_query_and_preserves_original_source() {
    use lingxi_adapters::storage::knowledge as ks;
    use lingxi_kernel::knowledge::{block_fingerprint, resolve_chunker_config, ChunkerBlock};
    use std::future::IntoFuture;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let home = unique_dir("legacy-cache-real-run");
    let script = Arc::new(std::sync::Mutex::new(KnowledgeProofScript::default()));
    let charges = Arc::new(AtomicUsize::new(0));
    let counted = charges.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route(
            "/chat/completions",
            axum::routing::post(knowledge_proof_provider),
        )
        .with_state(script.clone())
        .route(
            "/embeddings",
            axum::routing::post(move |body: axum::Json<Value>| {
                let counter = counted.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    embedding_stub_response(body).await
                }
            }),
        );
    let upstream = tokio::spawn(axum::serve(listener, router).into_future());
    let fixture: Value = serde_json::from_str(include_str!(
        "../../lingxi-adapters/tests/fixtures/ex09-legacy-source.json"
    ))
    .unwrap();
    let original = fixture["original"].as_str().unwrap();
    let state = boot_chat(&home, &endpoint).await;
    let notebook = state.knowledge().create_notebook("旧缓存显式重建").unwrap();
    let nb = notebook["id"].as_str().unwrap().to_string();
    state
        .knowledge()
        .update_notebook_settings(&nb, &json!({"chunkTargetChars":100}))
        .unwrap();
    let imported = state
        .knowledge()
        .import_pasted_text(&nb, original, Some("旧缺口原文"))
        .unwrap();
    let source = imported["source"]["id"].as_str().unwrap().to_string();
    let artifact = imported["parseArtifact"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let snapshot = imported["snapshot"]["id"].as_str().unwrap().to_string();
    let server = serve(state).await;
    wait_until_job(&server, &nb, &source, "pending_embedding").await;
    server.stop().await;
    let facts = ks::KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    // 准备已有完成摄入的旧用户状态，避免启动自动续跑掩盖查询旧缓存的行为。
    assert_eq!(facts.requeue_pending_embedding_jobs().unwrap(), 1);
    let job = facts.claim_next_job().unwrap().unwrap();
    facts
        .complete_job(&job.id, &json!({"fixture":"completed legacy ingestion"}))
        .unwrap();
    let blocks = facts.list_artifact_blocks(&artifact).unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].text, original);
    let raw_blocks = blocks
        .iter()
        .map(|block| ChunkerBlock {
            id: block.id.clone(),
            ordinal: block.ordinal,
            text: block.text.clone(),
            text_sha256: block.text_sha256.clone(),
            locator_type: block.locator_type.clone(),
            locator: block.locator.clone(),
        })
        .collect::<Vec<_>>();
    let raw_profile = resolve_chunker_config(&raw_blocks, Some(100)).unwrap().2;
    assert_eq!(raw_profile, fixture["profile"].as_str().unwrap());
    drop(facts);
    // 真实旧生产SQL仅重绑本用例来源身份，保留旧漏字正文和收费向量。
    for (filename, sql) in [
        (
            "knowledge_index.db",
            include_str!("../../lingxi-adapters/tests/fixtures/ex09-legacy-index.sql"),
        ),
        (
            "knowledge_vector.db",
            include_str!("../../lingxi-adapters/tests/fixtures/ex09-legacy-vector.sql"),
        ),
    ] {
        let path = home.join("knowledge").join(filename);
        for suffix in ["", "-wal", "-shm"] {
            let file = PathBuf::from(format!("{}{suffix}", path.display()));
            if file.exists() {
                std::fs::remove_file(file).unwrap();
            }
        }
        let sql = sql
            .replace("'artifact'", &format!("'{artifact}'"))
            .replace("\"block\"", &format!("\"{}\"", blocks[0].id))
            .replace(
                fixture["fingerprint"].as_str().unwrap(),
                &block_fingerprint(&raw_blocks),
            );
        rusqlite::Connection::open(path)
            .unwrap()
            .execute_batch(&sql)
            .unwrap();
    }
    let old_vector_bytes = || {
        let conn = rusqlite::Connection::open(home.join("knowledge/knowledge_vector.db")).unwrap();
        let mut statement = conn.prepare(
            "SELECT vector FROM chunk_vectors WHERE vector_index_variant_id=?1 ORDER BY ordinal",
        ).unwrap();
        let rows = statement
            .query_map([fixture["viv"].as_str().unwrap()], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    let paid_before = old_vector_bytes();
    assert_eq!(paid_before.len(), 3);
    let state = boot_chat_with_embedding(&home, &endpoint, true).await;
    let knowledge = state.knowledge().clone();
    let server = serve(state).await;
    let execute_search = |channel: &str| {
        let mut fixture = script.lock().unwrap();
        fixture.requests.clear();
        fixture.calls = vec![(
            "knowledge_search".into(),
            json!({"scopeId":"runtime","query":"上海 quartz","channel":channel}),
        )];
    };
    execute_search("hybrid");
    let input =
        json!({"input":"检索完整原文","knowledge":{"notebookIds":[nb],"sourceIds":[source]}});
    let (status, result) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(input.clone()),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let delivered = || {
        let request = script.lock().unwrap().requests.last().unwrap().clone();
        let text = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|message| {
                message["role"] == "tool" && message["tool_call_id"] == "original-result-0"
            })
            .map(proof_content)
            .unwrap();
        serde_json::from_str::<Value>(&text).unwrap()
    };
    let stale = delivered();
    assert!(stale["hits"].as_array().unwrap().is_empty(), "{stale}");
    assert!(
        stale["degraded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["reason"] == "KNOWLEDGE_INDEX_ALGORITHM_STALE"),
        "{stale}"
    );
    assert_eq!(
        charges.load(Ordering::SeqCst),
        0,
        "旧缓存不可触发付费查询或暗中重建"
    );
    assert_eq!(old_vector_bytes(), paid_before);
    knowledge.rebuild_notebook(&nb).unwrap();
    let facts = ks::KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let rebuilding = facts
        .list_ingestion_jobs(Some(&nb), Some(&source), 20)
        .unwrap()
        .into_iter()
        .find(|item| item.id != job.id)
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let current = facts.get_job(&rebuilding.id).unwrap();
        if current.status == "done" {
            break;
        }
        assert_ne!(current.status, "failed", "{current:?}");
        assert!(std::time::Instant::now() < deadline, "{current:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    drop(facts);
    let ingestion_charges = charges.load(Ordering::SeqCst);
    assert!(ingestion_charges > 0);
    assert_eq!(old_vector_bytes(), paid_before);
    let index = ks::KnowledgeIndexStore::open(&home.join("knowledge")).unwrap();
    assert!(index.artifact_has_stale_variants(&artifact).unwrap());
    let new_civ = index
        .get_ready_variant_metadata(&artifact, &ks::current_cache_profile_hash(&raw_profile))
        .unwrap()
        .unwrap()
        .0;
    assert_ne!(new_civ, fixture["civ"].as_str().unwrap());
    for term in ["上海", "quartz"] {
        assert!(!index
            .search_fts(&[&new_civ], &ks::build_fts_literal_query(term), 20)
            .unwrap()
            .is_empty());
    }
    drop(index);
    execute_search("fts");
    let (status, result) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(input.clone()),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let found = delivered();
    assert!(!found["hits"].as_array().unwrap().is_empty(), "{found}");
    assert!(
        found["degraded"].as_array().unwrap().is_empty(),
        "新版本就绪后旧收费行不再导致错误：{found}"
    );
    assert!(found.to_string().contains("上海"));
    assert_eq!(charges.load(Ordering::SeqCst), ingestion_charges);
    let (status, _, bytes) = server
        .request_raw(
            "GET",
            &format!("/lingxi/v1/knowledge/snapshots/{snapshot}/content"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(bytes, original.as_bytes());
    server.stop().await;
    drop(knowledge);
    let state = boot_chat_with_embedding(&home, &endpoint, true).await;
    let server = serve(state).await;
    execute_search("fts");
    let (status, result) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(input),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert!(!delivered()["hits"].as_array().unwrap().is_empty());
    assert_eq!(
        charges.load(Ordering::SeqCst),
        ingestion_charges,
        "重启不重复收费或重建"
    );
    assert_eq!(old_vector_bytes(), paid_before);
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ex09_section_filter_applies_before_real_top_one_and_keeps_citable_source() {
    use std::future::IntoFuture;
    let home = unique_dir("ex09-section-boundary");
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
    let server = serve(state).await;
    let nb = knowledge.create_notebook("章节边界").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    knowledge
        .update_notebook_settings(&nb, &json!({"chunkTargetChars":100}))
        .unwrap();
    let file = home.join("chapters.md");
    std::fs::write(
        &file,
        format!(
            "# Other\n\n{}\n\n# Chosen\n\nneedle SELECTED {}",
            ("needle ".repeat(30) + "UNSELECTED.\n\n").repeat(8),
            "padding ".repeat(30)
        ),
    )
    .unwrap();
    let imported = knowledge
        .import_file(&nb, file.to_str().unwrap(), Some("章节来源"))
        .unwrap();
    let src = imported["source"]["id"].as_str().unwrap().to_string();
    wait_until_job(&server, &nb, &src, "pending_embedding").await;
    script.lock().unwrap().calls = vec![(
        "knowledge_search".into(),
        json!({"scopeId":"runtime","query":"needle","channel":"fts","sectionKeys":["Chosen"],"limit":1}),
    )];
    let(status,result)=server.request("POST","/lingxi/v1/sessions/sess_local_alpha/execute",Some(json!({"input":"只检索指定章节","knowledge":{"notebookIds":[nb],"sourceIds":[src]}}))).await;
    assert_eq!(status, 200, "{result}");
    let request = script.lock().unwrap().requests.last().unwrap().clone();
    let text = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "tool" && m["tool_call_id"] == "original-result-0")
        .map(proof_content)
        .unwrap();
    let found: Value = serde_json::from_str(&text).unwrap();
    std::fs::write(
        home.join("actual-filtered-tool-result.json"),
        found.to_string(),
    )
    .unwrap();
    assert_eq!(found["hits"].as_array().unwrap().len(), 1, "{found}");
    assert_eq!(
        found["hits"][0]["headingPath"],
        json!(["Chosen"]),
        "必须在top1之前过滤：{found}"
    );
    assert!(found.to_string().contains("SELECTED"), "{found}");
    assert!(!found.to_string().contains("UNSELECTED"), "{found}");
    assert!(
        found["hits"][0]["spans"][0]["citationMarkdown"]
            .as_str()
            .is_some(),
        "{found}"
    );
    println!("EX09_FILTER_REAL_HOME {}", home.display());
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

fn ex09_delivered(script: &Arc<std::sync::Mutex<KnowledgeProofScript>>, index: usize) -> String {
    let request = script.lock().unwrap().requests.last().unwrap().clone();
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == format!("original-result-{index}")
        })
        .map(proof_content)
        .unwrap()
}

async fn ex09_execute_search(
    server: &TestServer,
    script: &Arc<std::sync::Mutex<KnowledgeProofScript>>,
    nb: &str,
    source: &str,
    args: Value,
) -> String {
    {
        let mut pending = script.lock().unwrap();
        pending.requests.clear();
        pending.calls = vec![("knowledge_search".into(), args)];
    }
    let mut input = json!({"input":"核对指定检索边界","knowledge":{"notebookIds":[nb]}});
    if !source.is_empty() {
        input["knowledge"]["sourceIds"] = json!([source]);
    }
    let (status, result) = server
        .request(
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            Some(input),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    ex09_delivered(script, 0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ex09_real_hybrid_section_filter_rejects_foreign_keys_and_reports_unconnected_rerank() {
    use std::future::IntoFuture;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let home = unique_dir("ex09-hybrid-filter");
    let script = Arc::new(std::sync::Mutex::new(KnowledgeProofScript::default()));
    let embeddings = Arc::new(AtomicUsize::new(0));
    let sent = embeddings.clone();
    let reranks = Arc::new(AtomicUsize::new(0));
    let reranked = reranks.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router=axum::Router::new().route("/chat/completions",axum::routing::post(knowledge_proof_provider)).with_state(script.clone())
        .route("/embeddings",axum::routing::post(move|body:axum::Json<Value>|{let sent=sent.clone();async move{sent.fetch_add(1,Ordering::SeqCst);let data=body["input"].as_array().unwrap().iter().enumerate().map(|(index,text)|json!({"index":index,"embedding":if text.as_str().unwrap().contains("SELECTED_BODY"){vec![0.0,1.0,0.0,0.0]}else{vec![1.0,0.0,0.0,0.0]}})).collect::<Vec<_>>();axum::Json(json!({"data":data,"usage":{"prompt_tokens":5,"total_tokens":5}}))}}))
        .route("/rerank",axum::routing::post(move||{let reranked=reranked.clone();async move{reranked.fetch_add(1,Ordering::SeqCst);axum::Json(json!({"results":[]}))}}));
    let upstream = tokio::spawn(axum::serve(listener, router).into_future());
    let state = boot_chat_with_embedding(&home, &endpoint, true).await;
    let knowledge = state.knowledge().clone();
    let server = serve(state).await;
    let nb = knowledge.create_notebook("Hybrid章节").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    knowledge
        .update_notebook_settings(&nb, &json!({"chunkTargetChars":100}))
        .unwrap();
    let file = home.join("hybrid.md");
    std::fs::write(
        &file,
        format!(
            "# Other\n\n{}\n\n# Chosen\n\nneedle SELECTED_BODY {}",
            ("needle ".repeat(30) + "OUTSIDE_CHAPTER.\n\n").repeat(8),
            "padding ".repeat(30)
        ),
    )
    .unwrap();
    let own = knowledge
        .import_file(&nb, file.to_str().unwrap(), Some("授权来源"))
        .unwrap();
    let src = own["source"]["id"].as_str().unwrap().to_string();
    wait_until_job(&server, &nb, &src, "done").await;
    let otherfile = home.join("excluded.md");
    std::fs::write(&otherfile, "# Excluded\n\nneedle EXCLUDED_SOURCE_SECRET").unwrap();
    let other = knowledge
        .import_file(&nb, otherfile.to_str().unwrap(), Some("未选来源"))
        .unwrap();
    let other_src = other["source"]["id"].as_str().unwrap();
    wait_until_job(&server, &nb, other_src, "done").await;
    knowledge
        .update_notebook_settings(
            &nb,
            &json!({"rerankModelRef":{"provider":"configured-provider","id":"configured-rerank"}}),
        )
        .unwrap();
    let args = json!({"scopeId":"runtime","query":"needle","channel":"hybrid","sectionKeys":["Chosen"],"limit":3});
    let before = embeddings.load(Ordering::SeqCst);
    let hybrid: Value =
        serde_json::from_str(&ex09_execute_search(&server, &script, &nb, &src, args.clone()).await)
            .unwrap();
    assert_eq!(embeddings.load(Ordering::SeqCst), before + 1);
    assert_eq!(hybrid["mode"], "hybrid");
    assert!(!hybrid["hits"].as_array().unwrap().is_empty(), "{hybrid}");
    for hit in hybrid["hits"].as_array().unwrap() {
        assert_eq!(hit["sourceId"], src);
        assert_eq!(hit["headingPath"], json!(["Chosen"]), "{hybrid}");
        assert_eq!(hit["grain"], "span");
        assert_eq!(hit["kind"], "original-text");
        assert!(hit["spans"]
            .as_array()
            .is_some_and(|spans| !spans.is_empty()));
    }
    assert!(!hybrid.to_string().contains("OUTSIDE_CHAPTER"));
    assert!(!hybrid.to_string().contains("EXCLUDED_SOURCE_SECRET"));
    assert!(
        hybrid["degraded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason"] == "KNOWLEDGE_RERANK_NOT_CONNECTED"
                && d["detail"].as_str().unwrap().contains("configured-rerank")),
        "{hybrid}"
    );
    assert_eq!(reranks.load(Ordering::SeqCst), 0);
    knowledge
        .update_notebook_settings(&nb, &json!({"rerankModelRef":null}))
        .unwrap();
    assert!(knowledge.get_notebook_config(&nb).unwrap()["rerankModelRef"].is_null());
    let unconfigured: Value =
        serde_json::from_str(&ex09_execute_search(&server, &script, &nb, &src, args.clone()).await)
            .unwrap();
    let order = |value: &Value| {
        value["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit["chunkId"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        order(&unconfigured),
        order(&hybrid),
        "未接通重排必须保留实际融合排序"
    );
    assert!(unconfigured["degraded"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["reason"] != "KNOWLEDGE_RERANK_NOT_CONNECTED"));
    knowledge
        .update_notebook_settings(
            &nb,
            &json!({"rerankModelRef":{"provider":"configured-provider","id":"configured-rerank"}}),
        )
        .unwrap();
    let mut local = args.clone();
    local["channel"] = json!("fts");
    let before = embeddings.load(Ordering::SeqCst);
    let fts: Value =
        serde_json::from_str(&ex09_execute_search(&server, &script, &nb, &src, local).await)
            .unwrap();
    assert_eq!(embeddings.load(Ordering::SeqCst), before);
    assert_eq!(fts["mode"], "fts");
    assert!(fts["degraded"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["reason"] != "KNOWLEDGE_RERANK_NOT_CONNECTED"));
    assert!(fts["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["headingPath"] == json!(["Chosen"])));
    // 非本来源的真实章节、未知键、非法数组均不能静默变成全范围，也不得发查询嵌入。
    for keys in [
        json!(["Excluded"]),
        json!(["never-existed"]),
        json!([""]),
        json!([" "]),
        json!(["Chosen", 7]),
        Value::Null,
        json!("Chosen"),
    ] {
        let mut denied = args.clone();
        denied["sectionKeys"] = keys.clone();
        let text = ex09_execute_search(&server, &script, &nb, &src, denied).await;
        assert!(text.starts_with("tool error"), "{keys}: {text}");
        assert!(!text.contains("EXCLUDED_SOURCE_SECRET"));
        assert_eq!(embeddings.load(Ordering::SeqCst), before);
    }
    let mut cross_source = args.clone();
    cross_source["sourceIds"] = json!([src]);
    cross_source["sectionKeys"] = json!(["Excluded"]);
    let denied = ex09_execute_search(&server, &script, &nb, "", cross_source).await;
    assert!(denied.contains("KNOWLEDGE_SCOPE_VIOLATION"), "{denied}");
    assert_eq!(embeddings.load(Ordering::SeqCst), before);
    // 只有空sectionKeys表示清除章节筛选；显式空来源/笔记本范围不能扩大成全部。
    for field in ["sourceIds", "notebookIds"] {
        let mut zero = args.clone();
        zero[field] = json!([]);
        zero["sectionKeys"] = json!([]);
        let no_source: Value =
            serde_json::from_str(&ex09_execute_search(&server, &script, &nb, &src, zero).await)
                .unwrap();
        assert!(
            no_source["hits"].as_array().unwrap().is_empty(),
            "显式空{field}不能扩大范围：{no_source}"
        );
        assert!(no_source["degraded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason"] == "KNOWLEDGE_EMPTY_FILTER"));
        assert_eq!(embeddings.load(Ordering::SeqCst), before);
        assert_eq!(reranks.load(Ordering::SeqCst), 0);
    }
    let mut empty = args;
    empty["sectionKeys"] = json!([]);
    empty["channel"] = json!("fts");
    let cleared: Value =
        serde_json::from_str(&ex09_execute_search(&server, &script, &nb, &src, empty).await)
            .unwrap();
    assert!(!cleared["hits"].as_array().unwrap().is_empty());
    assert!(cleared["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["sourceId"] == src));
    assert_eq!(embeddings.load(Ordering::SeqCst), before);
    std::fs::write(home.join("hybrid-result.json"), hybrid.to_string()).unwrap();
    println!("EX09_HYBRID_REAL_HOME {}", home.display());
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ex09_source_section_and_span_grains_deliver_real_navigation_and_citations() {
    use lingxi_adapters::storage::knowledge as ks;
    use std::future::IntoFuture;
    let home = unique_dir("ex09-grains");
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
    let nb = knowledge.create_notebook("粒度来源").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    knowledge
        .update_notebook_settings(&nb, &json!({"chunkTargetChars":100}))
        .unwrap();
    // 受控解析器输出：章节标题来自结构元数据，不在正文里；生产存储/分块/检索不替身化。
    let original = "body_unique_original 为真实原文，导航后才能引用。";
    let facts = ks::KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let (source, snapshot) = facts
        .create_source_with_snapshot(
            knowledge.studio_id(),
            "pasted_text",
            "TitoloSourceQzx",
            &json!({}),
            original.as_bytes(),
            "text/plain",
        )
        .unwrap();
    facts.add_membership(&nb, &source.id).unwrap();
    let artifact = ks::new_id("kpa");
    facts
        .begin_parse_artifact(
            &artifact,
            &snapshot.id,
            "controlled-structure",
            "1",
            "config",
        )
        .unwrap();
    let block = ks::BlockRow {
        id: ks::new_id("kb"),
        parse_artifact_id: artifact.clone(),
        ordinal: 0,
        text: original.into(),
        text_sha256: ks::sha256_hex(original.as_bytes()),
        locator_type: "markdown".into(),
        locator: json!({"headingPath":["HeadingSectionVbn"]}),
    };
    facts.insert_blocks(std::slice::from_ref(&block)).unwrap();
    facts
        .complete_parse_artifact(&artifact, "ready", &[], "citation_grade", None)
        .unwrap();
    let blocks = vec![ks::ChunkerBlock {
        id: block.id.clone(),
        ordinal: 0,
        text: block.text.clone(),
        text_sha256: block.text_sha256.clone(),
        locator_type: block.locator_type.clone(),
        locator: block.locator.clone(),
    }];
    let profile = ks::current_cache_profile_hash(
        &lingxi_kernel::knowledge::resolve_chunker_config(&blocks, Some(100))
            .unwrap()
            .2,
    );
    let chunks = ks::build_chunks(&artifact, &blocks, Some(100)).unwrap();
    let sections = lingxi_kernel::knowledge::build_sections(&artifact, &blocks);
    let metas = sections
        .iter()
        .map(|section| ks::SectionMetaRow {
            id: section.id.clone(),
            section_ordinal: section.section_ordinal,
            heading_path: section.heading_path.clone(),
            start_block_ordinal: section.start_block_ordinal,
            end_block_ordinal: section.end_block_ordinal,
        })
        .collect::<Vec<_>>();
    ks::KnowledgeIndexStore::open(&home.join("knowledge"))
        .unwrap()
        .replace_artifact_chunks(
            &artifact,
            &profile,
            &lingxi_kernel::knowledge::block_fingerprint(&blocks),
            &chunks,
            &metas,
        )
        .unwrap();
    let section = &sections[0].id;
    {
        let mut pending = script.lock().unwrap();
        pending.calls = vec![
            (
                "knowledge_search".into(),
                json!({"scopeId":"runtime","query":"TitoloSourceQzx","channel":"hybrid"}),
            ),
            (
                "knowledge_search".into(),
                json!({"scopeId":"runtime","query":"HeadingSectionVbn","channel":"hybrid"}),
            ),
            (
                "knowledge_search".into(),
                json!({"scopeId":"runtime","query":"body_unique_original","channel":"fts"}),
            ),
            (
                "knowledge_read".into(),
                json!({"scopeId":"runtime","sourceId":source.id,"sectionId":section,"maxChars":8000}),
            ),
            (
                "knowledge_search".into(),
                json!({"scopeId":"runtime","query":"NOMATCH_NEVER_IN_DOCUMENT","channel":"hybrid"}),
            ),
            (
                "knowledge_search".into(),
                json!({"scopeId":"runtime","query":format!("{} TitoloSourceQzx",(0..120).map(|n|format!("term{n}")).collect::<Vec<_>>().join(" ")),"channel":"hybrid"}),
            ),
        ];
    }
    let server = serve(state).await;
    let(status,result)=server.request("POST","/lingxi/v1/sessions/sess_local_alpha/execute",Some(json!({"input":"核对三种检索粒度与真实导航","knowledge":{"notebookIds":[nb],"sourceIds":[source.id]}}))).await;
    assert_eq!(status, 200, "{result}");
    for (index, grain) in [(0, "source"), (1, "section")] {
        let found: Value = serde_json::from_str(&ex09_delivered(&script, index)).unwrap();
        assert_eq!(found["hits"].as_array().unwrap().len(), 1, "{found}");
        let hint = &found["hits"][0];
        assert_eq!(hint["grain"], grain);
        assert_eq!(hint["kind"], "navigation-hint");
        assert!(hint.get("spans").is_none());
        assert!(!hint.to_string().contains("citationMarkdown"));
        assert_eq!(hint["readMore"]["sourceId"], source.id);
        assert_eq!(hint["readMore"]["sectionId"], section.as_str());
    }
    let span: Value = serde_json::from_str(&ex09_delivered(&script, 2)).unwrap();
    assert_eq!(span["hits"][0]["grain"], "span");
    assert_eq!(span["hits"][0]["kind"], "original-text");
    assert_eq!(span["hits"][0]["spans"][0]["text"], original);
    let citation = span["hits"][0]["spans"][0]["citationId"].as_str().unwrap();
    let (status, resolved) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/citations/{citation}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{resolved}");
    assert!(resolved.to_string().contains(original));
    assert!(resolved.to_string().contains(&snapshot.id));
    let read: Value = serde_json::from_str(&ex09_delivered(&script, 3)).unwrap();
    assert!(read.to_string().contains(original));
    assert!(read.to_string().contains("citationMarkdown"));
    let no_match: Value = serde_json::from_str(&ex09_delivered(&script, 4)).unwrap();
    assert!(no_match["hits"].as_array().unwrap().is_empty());
    let outside_term_budget: Value = serde_json::from_str(&ex09_delivered(&script, 5)).unwrap();
    assert!(
        outside_term_budget["hits"].as_array().unwrap().is_empty(),
        "标题线索须与FTS遵守相同96词预算：{outside_term_budget}"
    );
    assert_eq!(
        facts
            .get_blocks_by_ids(std::slice::from_ref(&block.id))
            .unwrap()[0],
        block
    );
    std::fs::write(
        home.join("grains-provider-requests.json"),
        serde_json::to_vec_pretty(&script.lock().unwrap().requests).unwrap(),
    )
    .unwrap();
    println!("EX09_GRAINS_REAL_HOME {}", home.display());
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}
