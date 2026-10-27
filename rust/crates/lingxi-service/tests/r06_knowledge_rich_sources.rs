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
static EMBED_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
async fn embedding_stub_response(axum::Json(parsed): axum::Json<Value>) -> axum::Json<Value> {
    EMBED_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
    let state = ServiceState::bootstrap_with_deps(config_for(home), &layout, deps)
        .await
        .expect("bootstrap");
    state
        .knowledge()
        .set_parser_binary(PathBuf::from(env!("CARGO_BIN_EXE_lingxi-service")))
        .unwrap();
    state
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

fn package(entries: &[(&str, String)]) -> Vec<u8> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn docx(text: &str) -> Vec<u8> {
    package(&[("word/document.xml",format!("<w:document xmlns:w=\"x\"><w:body><w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Heading</w:t></w:r></w:p><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body></w:document>"))])
}
fn xlsx() -> Vec<u8> {
    package(&[("xl/workbook.xml","<workbook xmlns:r=\"r\"><sheets><sheet name=\"Alpha\" r:id=\"one\"/><sheet name=\"Beta\" r:id=\"two\"/></sheets></workbook>".into()),("xl/_rels/workbook.xml.rels","<Relationships><Relationship Id=\"one\" Type=\"x/worksheet\" Target=\"worksheets/one.xml\"/><Relationship Id=\"two\" Type=\"x/worksheet\" Target=\"worksheets/two.xml\"/></Relationships>".into()),("xl/worksheets/one.xml","<worksheet><sheetData><row r=\"3\"><c r=\"B3\" t=\"inlineStr\"><is><t>Alpha target</t></is></c></row></sheetData></worksheet>".into()),("xl/worksheets/two.xml","<worksheet><sheetData><row r=\"7\"><c r=\"C7\" t=\"inlineStr\"><is><t>Beta target</t></is></c></row></sheetData></worksheet>".into())])
}
#[tokio::test]
async fn real_http_office_csv_jobs_snapshot_processing_structure_citations_and_restart() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let initial_calls = EMBED_CALLS.load(std::sync::atomic::Ordering::SeqCst);
    let home = unique_dir("rich-formats");
    let documents = unique_dir("rich-documents");
    let stub = spawn_embedding_stub().await;
    let state = boot("rich-formats", &home, Some(("embed-model-a", stub))).await;
    let knowledge = state.knowledge().clone();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let notebook = knowledge.create_notebook("结构来源").unwrap();
    let id = notebook["id"].as_str().unwrap();
    let server = serve(state).await;
    let mut imported = Vec::new();
    for (name, bytes, needle) in [
        (
            "real.docx",
            docx("DOCX exact target 𠀀"),
            "DOCX exact target 𠀀",
        ),
        ("real.xlsx", xlsx(), "Beta target"),
        (
            "real.csv",
            "one,\"quoted, exact\"\n𠀀,CSV target\n".as_bytes().to_vec(),
            "𠀀 | CSV target",
        ),
    ] {
        let path = documents.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let (status, payload) = server
            .request(
                "POST",
                &format!("/lingxi/v1/knowledge/notebooks/{id}/sources"),
                Some(json!({"kind":"file","filePath":path})),
            )
            .await;
        assert_eq!(status, 201, "{payload}");
        let src = payload["source"]["id"].as_str().unwrap();
        let snapshot = payload["snapshot"]["id"].as_str().unwrap();
        let artifact = payload["parseArtifact"]["id"].as_str().unwrap();
        assert_eq!(payload["parseArtifact"]["fidelity"], "structural");
        let current = facts.get_parse_artifact(artifact).unwrap();
        assert!(current.processing_artifact_id.is_some());
        let snap = facts.get_snapshot(snapshot).unwrap();
        assert_eq!(facts.read_snapshot_bytes(&snap).unwrap(), bytes);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let (status, blockbody) = server
            .request(
                "GET",
                &format!("/lingxi/v1/knowledge/parse-artifacts/{artifact}/blocks"),
                None,
            )
            .await;
        assert_eq!(status, 200);
        let blocks = blockbody["blocks"].as_array().unwrap();
        let block = blocks.iter().find(|b| b["text"] == needle).unwrap();
        if name.ends_with("xlsx") {
            assert_eq!(block["locator"]["source"]["sheet"], "Beta");
            assert_eq!(block["locator"]["source"]["row"], 7);
            assert_eq!(block["locator"]["source"]["cells"], json!(["C7"]));
        }
        let citation = knowledge
            .create_citation(
                artifact,
                block["id"].as_str().unwrap(),
                0,
                needle.chars().count(),
            )
            .unwrap();
        let (status, resolved) = server
            .request(
                "GET",
                &format!("/lingxi/v1/knowledge/citations/{}", citation.id),
                None,
            )
            .await;
        assert_eq!(status, 200, "{resolved}");
        assert_eq!(resolved["citation"]["canonicalText"], needle);
        assert_eq!(resolved["snapshot"]["id"], snapshot);
        let (status, chunks) = server
            .request(
                "GET",
                &format!("/lingxi/v1/knowledge/parse-artifacts/{artifact}/chunks"),
                None,
            )
            .await;
        assert_eq!(status, 200);
        assert!(chunks["chunkerConfigId"].is_string());
        assert!(!chunks["chunks"].as_array().unwrap().is_empty());
        wait_until_job(&server, id, src, "done").await;
        imported.push((
            src.to_string(),
            snapshot.to_string(),
            artifact.to_string(),
            citation.id,
        ));
    }
    let conn = rusqlite::Connection::open(home.join("knowledge/knowledge.db")).unwrap();
    let processing:i64=conn.query_row("SELECT count(*) FROM processing_artifacts WHERE status='ready' AND output_sha256 IS NOT NULL AND fidelity='structural'",[],|r|r.get(0)).unwrap();
    assert_eq!(processing, 3);
    let byte_rows: i64 = conn
        .query_row(
            "SELECT count(*) FROM processing_artifacts WHERE locator_map_json LIKE '%sheet_row%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(byte_rows, 1);
    let before = paid_rows(&home);
    let calls = EMBED_CALLS.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(calls - initial_calls, 3);
    server.stop().await;
    drop(knowledge);
    drop(facts);
    drop(conn);
    let state = boot("rich-restart", &home, Some(("embed-model-a", stub))).await;
    let server = serve(state).await;
    for (_, _, _, citation) in imported {
        let (status, _) = server
            .request(
                "GET",
                &format!("/lingxi/v1/knowledge/citations/{citation}"),
                None,
            )
            .await;
        assert_eq!(status, 200);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(paid_rows(&home), before, "重启不能重复覆盖收费向量");
    assert_eq!(EMBED_CALLS.load(std::sync::atomic::Ordering::SeqCst), calls);
    server.stop().await;
}
#[tokio::test]
async fn public_web_text_snapshot_and_private_urls_zero_effects() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("web-snapshot");
    let state = boot("web-snapshot", &home, None).await;
    let knowledge = state.knowledge().clone();
    let notebook = knowledge.create_notebook("网页").unwrap();
    let id = notebook["id"].as_str().unwrap();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let server = serve(state).await;
    let route = format!("/lingxi/v1/knowledge/notebooks/{id}/sources");
    let (status, missing) = server
        .request(
            "POST",
            &route,
            Some(json!({"kind":"web_snapshot","url":"https://example.com"})),
        )
        .await;
    assert_eq!(status, 400, "{missing}");
    assert_eq!(missing["error"], "KNOWLEDGE_WEB_FETCH_UNAVAILABLE");
    knowledge.set_url_provider(Some(Arc::new(
        lingxi_service::resource_url::UrlProvider::new(&home),
    )));
    for url in [
        "http://127.0.0.1/PRIVATE",
        "http://[::1]/PRIVATE",
        "http://localhost/PRIVATE",
        "file:///etc/passwd",
        "http://user:secret@example.com",
    ] {
        let (status, err) = server
            .request(
                "POST",
                &route,
                Some(json!({"kind":"web_snapshot","url":url})),
            )
            .await;
        assert_eq!(status, 502, "{url}: {err}");
        assert_eq!(err["error"], "KNOWLEDGE_WEB_FETCH_FAILED");
        assert_eq!(facts.list_notebook_sources(id).unwrap().len(), 0);
    }
    let (status, payload) = server
        .request(
            "POST",
            &route,
            Some(json!({"kind":"web_snapshot","url":"https://1.1.1.1/cdn-cgi/trace"})),
        )
        .await;
    assert_eq!(
        status, 201,
        "真实公网失败需保留NETWORK_REFUSED，不改为成功: {payload}"
    );
    assert_eq!(payload["source"]["sourceType"], "web_snapshot");
    let source = facts
        .get_source(payload["source"]["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        source.origin_metadata["originalUrl"],
        "https://1.1.1.1/cdn-cgi/trace"
    );
    assert!(source.origin_metadata["finalUrl"]
        .as_str()
        .unwrap()
        .starts_with("https://1.1.1.1/cdn-cgi/trace"));
    assert!(source.origin_metadata["fetchedAt"]
        .as_str()
        .unwrap()
        .ends_with('Z'));
    let snapshot = facts
        .get_snapshot(payload["snapshot"]["id"].as_str().unwrap())
        .unwrap();
    let bytes = facts.read_snapshot_bytes(&snapshot).unwrap();
    assert!(String::from_utf8(bytes).unwrap().contains("h=1.1.1.1"));
    let blocks = facts
        .list_artifact_blocks(payload["parseArtifact"]["id"].as_str().unwrap())
        .unwrap();
    assert!(blocks.iter().any(|b| b.text.contains("h=1.1.1.1")));
    assert!(blocks.iter().all(|b| b.locator["lineStart"].is_number()));
    server.stop().await;
}

fn paid_rows(home: &Path) -> Vec<(String, String, i64, Vec<u8>)> {
    let c = rusqlite::Connection::open(home.join("knowledge/knowledge_vector.db")).unwrap();
    let mut q=c.prepare("SELECT vector_index_variant_id,chunk_id,ordinal,vector FROM chunk_vectors ORDER BY vector_index_variant_id,chunk_id").unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
#[tokio::test]
async fn refresh_refuses_other_studio_and_nonmember_before_reading_source() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("refresh-owner");
    let documents = unique_dir("refresh-private-documents");
    let file = documents.join("private.txt");
    std::fs::write(&file, "OTHER_STUDIO_SECRET").unwrap();
    let state = boot("refresh-owner", &home, None).await;
    let local = state.knowledge().clone();
    let notebook = local.create_notebook("本地").unwrap();
    let id = notebook["id"].as_str().unwrap();
    let foreign =
        lingxi_service::knowledge::KnowledgeService::new(&home, "foreign-studio".into()).unwrap();
    let foreign_nb = foreign.create_notebook("其它主体").unwrap();
    let payload = foreign
        .import_file(
            foreign_nb["id"].as_str().unwrap(),
            file.to_str().unwrap(),
            Some("私有来源名称"),
        )
        .unwrap();
    let src = payload["source"]["id"].as_str().unwrap();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let before = facts
        .list_notebook_sources(id)
        .unwrap()
        .iter()
        .map(|e| e.source.id.clone())
        .collect::<Vec<_>>();
    let source_before = facts.get_source(src).unwrap();
    let server = serve(state).await;
    let (status, error) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{id}/sources/{src}/refresh"),
            None,
        )
        .await;
    assert_eq!(status, 404, "跨主体来源必须在读取前拒绝: {error}");
    assert!(!error.to_string().contains("私有来源名称"));
    assert!(!error.to_string().contains("private.txt"));
    assert_eq!(
        facts
            .list_notebook_sources(id)
            .unwrap()
            .iter()
            .map(|e| e.source.id.clone())
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(facts.get_source(src).unwrap(), source_before);
    let same = local.import_pasted_text(id, "本地原文", None).unwrap();
    let other = local.create_notebook("同主体另一集合").unwrap();
    let (status, _) = server
        .request(
            "POST",
            &format!(
                "/lingxi/v1/knowledge/notebooks/{}/sources/{}/refresh",
                other["id"].as_str().unwrap(),
                same["source"]["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_eq!(status, 404);
    server.stop().await;
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
    let state = ServiceState::bootstrap_with_deps(
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
    .unwrap();
    state
        .knowledge()
        .set_parser_binary(PathBuf::from(env!("CARGO_BIN_EXE_lingxi-service")))
        .unwrap();
    state
}

use pdf_extract::{
    content::{Content, Operation},
    dictionary, Document, Object, Stream,
};
fn pdf(texts: &[Option<&str>]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let mut kids = Vec::new();
    for text in texts {
        let ops = match text {
            Some(text) => vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![72.into(), 720.into()]),
                Operation::new("Tj", vec![Object::string_literal(*text)]),
                Operation::new("ET", vec![]),
            ],
            None => vec![],
        };
        let stream = doc.add_object(Stream::new(
            dictionary! {},
            Content { operations: ops }.encode().unwrap(),
        ));
        let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()],"Contents"=>stream,"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}}});
        kids.push(page.into());
    }
    doc.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>texts.len() as i64}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", catalog);
    doc.compress();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
#[derive(Default)]
struct RichRunScript {
    calls: Vec<(String, Value)>,
    requests: Vec<Value>,
}
fn content(value: &Value) -> String {
    value["content"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            value["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
}
async fn rich_run_provider(
    axum::extract::State(script): axum::extract::State<Arc<std::sync::Mutex<RichRunScript>>>,
    axum::Json(body): axum::Json<Value>,
) -> impl axum::response::IntoResponse {
    let (step, call) = {
        let mut s = script.lock().unwrap();
        let step = s.requests.len();
        let call = s.calls.get(step / 2).cloned();
        s.requests.push(body.clone());
        (step, call)
    };
    let delta = if let Some((tool, mut args)) = call {
        let system = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "system")
            .map(content)
            .collect::<Vec<_>>()
            .join("\n");
        let tail = system.split_once("本轮知识资料范围：").unwrap().1;
        let selection = serde_json::Deserializer::from_str(tail)
            .into_iter::<Value>()
            .next()
            .unwrap()
            .unwrap();
        args["scopeId"] = selection["scopeId"].clone();
        let (name, args) = if step % 2 == 0 {
            ("mcp_describe_tool", json!({"name":tool}))
        } else {
            ("mcp_call", json!({"tool":tool,"arguments":args}))
        };
        json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("rich-{step}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]})
    } else {
        json!({"role":"assistant","content":"引用已核对"})
    };
    let tools = delta.get("tool_calls").is_some();
    let frames = [
        json!({"id":format!("rich-{step}"),"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":if tools{"tool_calls"}else{"stop"}}]}),
        json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":4}}),
    ];
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        frames
            .iter()
            .map(|f| format!("data: {f}\n\n"))
            .collect::<String>()
            + "data: [DONE]\n\n",
    )
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_model_run_describes_searches_reads_and_cites_office_csv_pdf_sources() {
    use std::future::IntoFuture;
    let home = unique_dir("rich-model");
    let docs = unique_dir("rich-model-docs");
    let script = Arc::new(std::sync::Mutex::new(RichRunScript::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let upstream = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route("/chat/completions", axum::routing::post(rich_run_provider))
                .route("/embeddings", axum::routing::post(embedding_stub_response))
                .with_state(script.clone()),
        )
        .into_future(),
    );
    let state = boot_chat_with_embedding(&home, &endpoint, true).await;
    let ledger_path = state.storage().db_path().to_path_buf();
    let knowledge = state.knowledge().clone();
    let nb = knowledge.create_notebook("真实格式检索").unwrap();
    let id = nb["id"].as_str().unwrap();
    let server = serve(state).await;
    let mut sources = Vec::new();
    for (name, bytes, query) in [
        ("a.docx", docx("DOCXMODEL citrus proof"), "DOCXMODEL"),
        ("a.xlsx", xlsx(), "Beta"),
        ("a.csv", b"CSVMODEL,exact row proof\n".to_vec(), "CSVMODEL"),
        (
            "a.pdf",
            pdf(&[Some("PDFMODEL page one"), Some("PDFMODEL page two")]),
            "PDFMODEL",
        ),
    ] {
        let path = docs.join(name);
        std::fs::write(&path, bytes).unwrap();
        let imported = knowledge
            .import_file(id, path.to_str().unwrap(), Some(name))
            .unwrap();
        let src = imported["source"]["id"].as_str().unwrap().to_string();
        wait_until_job(&server, id, &src, "done").await;
        let mut s = script.lock().unwrap();
        s.calls.push((
            "knowledge_search".into(),
            json!({"query":query,"channel":"fts","sourceIds":[src]}),
        ));
        s.calls.push((
            "knowledge_read".into(),
            json!({"sourceId":src,"maxChars":1024}),
        ));
        sources.push(src);
    }
    let(status,result)=server.request("POST","/lingxi/v1/sessions/sess_local_alpha/execute",Some(json!({"input":"逐个查找原文并引用，不能混来源","knowledge":{"notebookIds":[id],"sourceIds":sources}}))).await;
    assert_eq!(status, 200, "{result}");
    let requests = script.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 17);
    let last = requests.last().unwrap();
    for (i, src) in sources.iter().enumerate() {
        for (offset, is_read) in [(1, false), (3, true)] {
            let call = format!("rich-{}", 4 * i + offset);
            let message = last["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["role"] == "tool" && m["tool_call_id"] == call)
                .unwrap();
            let body: Value = serde_json::from_str(&content(message)).unwrap();
            let hits = body[if is_read { "spans" } else { "hits" }]
                .as_array()
                .unwrap();
            assert!(!hits.is_empty(), "{body}");
            if !is_read {
                assert!(hits.iter().all(|v| v["sourceId"] == *src), "{body}");
            }
            if is_read {
                for span in hits {
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
                    assert_eq!(resolved["source"]["id"], *src);
                    assert_eq!(resolved["citation"]["canonicalText"], span["text"]);
                    if i == 3 {
                        assert!(resolved["block"]["locator"]["page"].is_number());
                    }
                }
            }
        }
    }
    let ledger = rusqlite::Connection::open(ledger_path).unwrap();
    let chat:i64=ledger.query_row("SELECT count(*) FROM model_call_usage WHERE purpose='chat' AND provider='main' AND outcome='succeeded' AND transport_attempts=1 AND input_tokens=20 AND output_tokens=4",[],|r|r.get(0)).unwrap();
    assert_eq!(chat, 17);
    let embedded:i64=ledger.query_row("SELECT count(*) FROM model_call_usage WHERE provider='embed' AND outcome='succeeded' AND transport_attempts=1",[],|r|r.get(0)).unwrap();
    assert_eq!(embedded, 4);
    server.stop().await;
    upstream.abort();
    let _ = upstream.await;
}
#[tokio::test]
async fn ready_processing_output_is_reused_with_identity_and_corruption_is_explicit() {
    use lingxi_adapters::storage::knowledge::{sha256_hex, KnowledgeFactStore};
    let home = unique_dir("processing-reuse");
    let state = boot("processing-reuse", &home, None).await;
    let knowledge = state.knowledge().clone();
    let nb = knowledge.create_notebook("复用").unwrap();
    let id = nb["id"].as_str().unwrap();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let bytes = docx("READY_CACHE_EXACT");
    let parsed = lingxi_service::knowledge_processors::parse_document(
        Path::new(env!("CARGO_BIN_EXE_lingxi-service")),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        &bytes,
    )
    .unwrap();
    let config = sha256_hex(
        br#"{"processorId":"lingxi-office-docx","version":"1","maxCells":100000,"maxRows":50000}"#,
    );
    let unused = home.join("do-not-execute");
    std::fs::write(&unused, "not executable").unwrap();
    knowledge.set_parser_binary(unused).unwrap();
    let server = serve(state).await;
    for corrupt in [false, true] {
        let (source, snapshot) = facts
            .create_source_with_snapshot(
                knowledge.studio_id(),
                "file",
                "已完成处理但尚未解析",
                &json!({"kind":"file","fileName":"ready.docx"}),
                &bytes,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            )
            .unwrap();
        facts.add_membership(id, &source.id).unwrap();
        let row = facts
            .begin_processing_artifact(&snapshot.id, "lingxi-office-docx", "1", &config)
            .unwrap();
        let text = parsed
            .blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let map = Value::Object(
            parsed
                .blocks
                .iter()
                .enumerate()
                .map(|(i, b)| (i.to_string(), b.locator["source"].clone()))
                .collect(),
        );
        facts
            .complete_processing_artifact(&row, &text, &map, &parsed.warnings)
            .unwrap();
        if corrupt {
            std::fs::write(
                home.join("knowledge")
                    .join(format!("artifacts/{}/{}.txt", snapshot.id, row.id)),
                "CORRUPTED_CACHE",
            )
            .unwrap();
        }
        let (status, result) = server
            .request(
                "POST",
                &format!(
                    "/lingxi/v1/knowledge/notebooks/{id}/sources/{}/reingest",
                    source.id
                ),
                None,
            )
            .await;
        assert_eq!(status, 200, "{result}");
        assert_eq!(result["retried"], false);
        wait_until_job(
            &server,
            id,
            &source.id,
            if corrupt {
                "failed"
            } else {
                "pending_embedding"
            },
        )
        .await;
        let artifact = facts
            .latest_parse_artifact_for_snapshot(&snapshot.id)
            .unwrap()
            .unwrap();
        if corrupt {
            assert_eq!(artifact.status, "failed");
            assert!(artifact.warnings.join(" ").contains("hash mismatch"));
        } else {
            assert_eq!(artifact.status, "ready");
            assert_eq!(
                artifact.processing_artifact_id.as_deref(),
                Some(row.id.as_str())
            );
            assert_eq!(
                facts
                    .list_artifact_blocks(&artifact.id)
                    .unwrap()
                    .iter()
                    .map(|b| b.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                text
            );
        }
        assert_eq!(facts.read_snapshot_bytes(&snapshot).unwrap(), bytes);
    }
    server.stop().await;
}
#[tokio::test]
async fn real_html_transport_snapshot_dom_embed_and_redirect_refusal() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    use std::future::IntoFuture;
    let home = unique_dir("real-html");
    let embedding = spawn_embedding_stub().await;
    let state = boot("real-html", &home, Some(("embed-model-a", embedding))).await;
    let knowledge = state.knowledge().clone();
    let nb = knowledge.create_notebook("网页结构").unwrap();
    let id = nb["id"].as_str().unwrap();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    const HTML:&str="<!doctype html><html><body><h1>真实标题𠀀</h1><p>HTMLWIRE exact original<script>SECRET_HIDDEN</script> line</p><p>second line</p></body></html>";
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let fixture = tokio::spawn(
        axum::serve(
            socket,
            axum::Router::new()
                .route(
                    "/source",
                    axum::routing::get(|| async {
                        (
                            [(axum::http::header::CONTENT_TYPE, "text/html; charset=UTF-8")],
                            HTML,
                        )
                    }),
                )
                .route(
                    "/redirect-private",
                    axum::routing::get(|| async {
                        (
                            axum::http::StatusCode::FOUND,
                            [(axum::http::header::LOCATION, "http://127.0.0.1/secret")],
                        )
                    }),
                )
                .route(
                    "/binary",
                    axum::routing::get(|| async {
                        (
                            [(axum::http::header::CONTENT_TYPE, "application/pdf")],
                            "%PDF-not-html",
                        )
                    }),
                ),
        )
        .into_future(),
    );
    knowledge.set_url_provider(Some(Arc::new(
        lingxi_service::resource_url::UrlProvider::with_fixture_http(&home, addr).unwrap(),
    )));
    let server = serve(state).await;
    let route = format!("/lingxi/v1/knowledge/notebooks/{id}/sources");
    let url = format!("http://r06-fixture.invalid:{}/source", addr.port());
    let (status, payload) = server
        .request(
            "POST",
            &route,
            Some(json!({"kind":"web_snapshot","url":url})),
        )
        .await;
    assert_eq!(status, 201, "{payload}");
    let snapshot = facts
        .get_snapshot(payload["snapshot"]["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        facts.read_snapshot_bytes(&snapshot).unwrap(),
        HTML.as_bytes()
    );
    assert_eq!(snapshot.mime_type, "text/html");
    let artifact = payload["parseArtifact"]["id"].as_str().unwrap();
    let blocks = facts.list_artifact_blocks(artifact).unwrap();
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[1].text, "HTMLWIRE exact original line");
    assert!(!blocks.iter().any(|b| b.text.contains("SECRET_HIDDEN")));
    assert_eq!(blocks[1].locator["headingPath"], json!(["真实标题𠀀"]));
    assert_eq!(
        blocks[1].locator["structuralPath"],
        "html:nth-of-type(1) > body:nth-of-type(1) > p:nth-of-type(1)"
    );
    let src = payload["source"]["id"].as_str().unwrap();
    wait_until_job(&server, id, src, "done").await;
    let citation = knowledge
        .create_citation(artifact, &blocks[1].id, 0, blocks[1].text.chars().count())
        .unwrap();
    assert_eq!(
        knowledge.resolve_citation(&citation.id).unwrap()["citation"]["canonicalText"],
        blocks[1].text
    );
    for (suffix, expected) in [("redirect-private", 502), ("binary", 422)] {
        let(status,error)=server.request("POST",&route,Some(json!({"kind":"web_snapshot","url":format!("http://r06-fixture.invalid:{}/{suffix}",addr.port())}))).await;
        assert_eq!(status, expected, "{error}");
        assert_eq!(facts.list_notebook_sources(id).unwrap().len(), 1);
    }
    // 同一fixture URL不能通过普通生产构造器启用本机例外。
    assert!(lingxi_service::resource_url::UrlProvider::new(&home)
        .read(&url)
        .await
        .is_err());
    server.stop().await;
    fixture.abort();
    let _ = fixture.await;
}
#[tokio::test]
async fn failed_office_retry_and_changed_refresh_keep_original_snapshot_and_processing_identity() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("failed-processor");
    let docs = unique_dir("failed-processor-files");
    let file = docs.join("broken.docx");
    let bad = b"not a zip document";
    std::fs::write(&file, bad).unwrap();
    let state = boot("failed-processor", &home, None).await;
    let knowledge = state.knowledge().clone();
    let nb = knowledge.create_notebook("失败与刷新").unwrap();
    let id = nb["id"].as_str().unwrap();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let server = serve(state).await;
    let (status, error) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{id}/sources"),
            Some(json!({"kind":"file","filePath":file})),
        )
        .await;
    assert_eq!(status, 422, "{error}");
    assert_eq!(error["error"], "KNOWLEDGE_PARSE_FAILED");
    let entries = facts.list_notebook_sources(id).unwrap();
    assert_eq!(entries.len(), 1);
    let source = &entries[0].source.id;
    let snapshot = entries[0].snapshot.clone();
    assert_eq!(facts.read_snapshot_bytes(&snapshot).unwrap(), bad);
    assert!(facts.latest_job_for_source(id, source).unwrap().is_none());
    let failed_artifact = entries[0].parse_artifact.as_ref().unwrap();
    assert_eq!(failed_artifact.status, "failed");
    let c = rusqlite::Connection::open(home.join("knowledge/knowledge.db")).unwrap();
    let original_processing: String = c
        .query_row(
            "SELECT id FROM processing_artifacts WHERE content_snapshot_id=?1 AND status='failed'",
            [&snapshot.id],
            |r| r.get(0),
        )
        .unwrap();
    let retry = format!("/lingxi/v1/knowledge/notebooks/{id}/sources/{source}/reingest");
    let (status, created) = server.request("POST", &retry, None).await;
    assert_eq!(status, 200);
    assert_eq!(created["retried"], false);
    let job = wait_until_job(&server, id, source, "failed").await;
    assert_eq!(job["attempt"], 1);
    let (status, retried) = server.request("POST", &retry, None).await;
    assert_eq!(status, 200, "{retried}");
    assert_eq!(retried["retried"], true);
    let job = wait_until_job(&server, id, source, "failed").await;
    assert_eq!(job["attempt"], 2);
    let same: String = c
        .query_row(
            "SELECT id FROM processing_artifacts WHERE content_snapshot_id=?1",
            [&snapshot.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(same, original_processing);
    std::fs::write(&file, docx("REFRESH actual new content")).unwrap();
    let (status, new) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{id}/sources/{source}/refresh"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{new}");
    assert_eq!(new["changed"], true);
    assert_ne!(new["snapshot"]["id"], snapshot.id);
    assert_ne!(new["parseArtifact"]["id"], failed_artifact.id);
    let new_source = new["source"]["id"].as_str().unwrap();
    wait_until_job(&server, id, new_source, "pending_embedding").await;
    assert_eq!(facts.read_snapshot_bytes(&snapshot).unwrap(), bad);
    let rows_before: i64 = c
        .query_row("SELECT count(*) FROM processing_artifacts", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rows_before, 2);
    let (status, unchanged) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{id}/sources/{new_source}/refresh"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(unchanged["changed"], false);
    assert_eq!(
        c.query_row("SELECT count(*) FROM processing_artifacts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        rows_before
    );
    server.stop().await;
}
#[tokio::test]
async fn image_only_pdf_keeps_needs_ocr_and_explicit_job_refusal_without_model_calls() {
    use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
    let home = unique_dir("pdf-needs-ocr");
    let docs = unique_dir("pdf-needs-ocr-file");
    let path = docs.join("scanned.pdf");
    let bytes = pdf(&[None]);
    std::fs::write(&path, &bytes).unwrap();
    let state = boot("pdf-needs-ocr", &home, None).await;
    let ledger = state.storage().db_path().to_path_buf();
    let nb = state.knowledge().create_notebook("扫描PDF").unwrap();
    let id = nb["id"].as_str().unwrap();
    let server = serve(state).await;
    let (status, result) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{id}/sources"),
            Some(json!({"kind":"file","filePath":path})),
        )
        .await;
    assert_eq!(status, 201, "{result}");
    assert_eq!(result["parseArtifact"]["status"], "needs_ocr");
    assert_eq!(result["parseArtifact"]["warnings"], json!(["needs_ocr"]));
    let source = result["source"]["id"].as_str().unwrap();
    let job = wait_until_job(&server, id, source, "failed").await;
    assert!(job["error"].as_str().unwrap().contains("OCR"));
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    assert!(facts
        .list_artifact_blocks(result["parseArtifact"]["id"].as_str().unwrap())
        .unwrap()
        .is_empty());
    let snapshot = facts
        .get_snapshot(result["snapshot"]["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(facts.read_snapshot_bytes(&snapshot).unwrap(), bytes);
    let c = rusqlite::Connection::open(ledger).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM model_call_usage", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    server.stop().await;
}
#[test]
fn additive_processing_schema_preserves_older_rows_and_refuses_unverified_output() {
    use lingxi_adapters::storage::knowledge::{sha256_hex, KnowledgeFactStore};
    let home = unique_dir("processing-column");
    let root = home.join("knowledge");
    let facts = KnowledgeFactStore::open(&root).unwrap();
    let (_, snapshot) = facts
        .create_source_with_snapshot("synthetic", "file", "source", &json!({}), b"x", "text/csv")
        .unwrap();
    let row = facts
        .begin_processing_artifact(
            &snapshot.id,
            "lingxi-office-csv",
            "1",
            &sha256_hex(b"config"),
        )
        .unwrap();
    facts
        .complete_processing_artifact(
            &row,
            "x",
            &json!({"0":{"kind":"csv_row","sheet":"sheet1","row":1,"cells":["R1C1"]}}),
            &[],
        )
        .unwrap();
    drop(facts);
    let c = rusqlite::Connection::open(root.join("knowledge.db")).unwrap();
    c.execute_batch("ALTER TABLE processing_artifacts DROP COLUMN output_sha256")
        .unwrap();
    drop(c);
    let facts = KnowledgeFactStore::open(&root).unwrap();
    let restored = facts
        .processing_artifact(
            &snapshot.id,
            "lingxi-office-csv",
            "1",
            &sha256_hex(b"config"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(restored.id, row.id);
    assert_eq!(restored.status, "ready");
    assert!(restored.output_sha256.is_none());
    assert!(facts.read_processing_output(&restored).is_err());
    assert_eq!(facts.read_snapshot_bytes(&snapshot).unwrap(), b"x");
}
