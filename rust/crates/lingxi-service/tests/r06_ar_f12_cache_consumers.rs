//! F12：真实知识服务删除入口必须保留其它来源的全文及付费向量。
use lingxi_adapters::storage::knowledge as ks;
use lingxi_service::knowledge::KnowledgeService;
use rusqlite::Connection;
use std::path::PathBuf;

fn world() -> (PathBuf, String, String, String, String) {
    static FIXTURE_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let home = PathBuf::from(std::env::var_os("LINGXI_HOME").expect("合成HOME")).join(format!(
        "consumer-{}-{}-{}",
        std::process::id(),
        FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = home.join("knowledge");
    let facts = ks::KnowledgeFactStore::open(&root).unwrap();
    let index = ks::KnowledgeIndexStore::open(&root).unwrap();
    let vectors = ks::KnowledgeVectorStore::open(&root).unwrap();
    let mut sources = Vec::new();
    let mut variants = Vec::new();
    for (source_name, artifacts) in [
        ("source-a", vec!["artifact-a-old", "artifact-a"]),
        ("source-b", vec!["artifact-b"]),
    ] {
        let (source, snapshot) = facts
            .create_source_with_snapshot(
                "studio",
                "pasted_text",
                source_name,
                &serde_json::json!({}),
                source_name.as_bytes(),
                "text/plain",
            )
            .unwrap();
        sources.push(source.id);
        for artifact in artifacts {
            facts
                .begin_parse_artifact(artifact, &snapshot.id, "synthetic", "1", "config")
                .unwrap();
            facts
                .complete_parse_artifact(artifact, "ready", &[], "exact", None)
                .unwrap();
            let variant = index
                .ensure_chunk_index_variant(artifact, "profile", "fingerprint")
                .unwrap();
            let build = vectors
                .begin_variant_build(
                    &variant.id,
                    artifact,
                    "paid-synthetic-model",
                    "fingerprint",
                    2,
                )
                .unwrap();
            vectors
                .upsert_batch(
                    &build.variant_id,
                    &[ks::VectorEntry {
                        chunk_id: format!("chunk-{artifact}"),
                        ordinal: 0,
                        vector: vec![0.2, 0.8],
                    }],
                )
                .unwrap();
            vectors.complete_build(&build.variant_id, 1).unwrap();
            if artifact == "artifact-b" {
                variants.push(variant.id);
                variants.push(build.variant_id);
            }
        }
    }
    (
        home,
        sources[0].clone(),
        sources[1].clone(),
        variants[0].clone(),
        variants[1].clone(),
    )
}

#[test]
fn deletion_of_a_keeps_b_and_removes_all_a_versions() {
    let (home, source_a, source_b, index_b, vector_b) = world();
    let service = KnowledgeService::new(&home, "studio".into()).unwrap();
    let receipt = service.delete_source(&source_a).unwrap();
    assert_eq!(receipt["purge"]["indexVariants"], true);
    let root = home.join("knowledge");
    let index = ks::KnowledgeIndexStore::open(&root).unwrap();
    assert!(index.get_variant(&index_b).unwrap().is_some());
    for artifact in ["artifact-a-old", "artifact-a"] {
        let id = ks::chunk_index_variant_id(artifact, "profile");
        assert!(index.get_variant(&id).unwrap().is_none());
    }
    let vectors = ks::KnowledgeVectorStore::open(&root).unwrap();
    assert_eq!(
        vectors.search(&[vector_b], &[0.2, 0.8], 5).unwrap().len(),
        1
    );
    for artifact in ["artifact-a-old", "artifact-a"] {
        assert!(vectors
            .list_variants_by_artifact(artifact)
            .unwrap()
            .is_empty());
    }
    let facts = ks::KnowledgeFactStore::open(&root).unwrap();
    let snapshot = facts.latest_snapshot_for_source(&source_b).unwrap();
    assert_eq!(facts.read_snapshot_bytes(&snapshot).unwrap(), b"source-b");
}

#[test]
fn deletion_cache_error_is_reported_and_source_original_remains() {
    let (home, source_a, _, _, _) = world();
    let service = KnowledgeService::new(&home, "studio".into()).unwrap();
    let root = home.join("knowledge");
    let newer = Connection::open(root.join("knowledge_vector.db")).unwrap();
    newer.execute_batch("PRAGMA user_version=999").unwrap();
    let error = service.delete_source(&source_a).unwrap_err();
    assert_eq!(error.code, "KNOWLEDGE_SCHEMA_NEWER");
    let facts = ks::KnowledgeFactStore::open(&root).unwrap();
    assert!(facts.get_source(&source_a).is_ok());
    let snapshot = facts.latest_snapshot_for_source(&source_a).unwrap();
    assert_eq!(facts.read_snapshot_bytes(&snapshot).unwrap(), b"source-a");
    assert_eq!(
        newer
            .query_row("SELECT count(*) FROM chunk_vectors", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

async fn ingestion_case(upgrade: bool) {
    use lingxi_kernel::knowledge::{
        resolve_chunker_config, ChunkerBlock, KNOWLEDGE_CHUNK_TARGET_CHARS,
    };
    let (home, _, _, _, _) = world();
    let service = std::sync::Arc::new(KnowledgeService::new(&home, "studio".into()).unwrap());
    let notebook = service.create_notebook("摄入保护").unwrap();
    let notebook_id = notebook["id"].as_str().unwrap();
    let imported = service
        .import_pasted_text(notebook_id, "独立摄入的真实原文", None)
        .unwrap();
    let source_id = imported["source"]["id"].as_str().unwrap();
    let artifact_id = imported["parseArtifact"]["id"].as_str().unwrap();
    let root = home.join("knowledge");
    let facts = ks::KnowledgeFactStore::open(&root).unwrap();
    let blocks: Vec<ChunkerBlock> = facts
        .list_artifact_blocks(artifact_id)
        .unwrap()
        .into_iter()
        .map(|block| ChunkerBlock {
            id: block.id,
            ordinal: block.ordinal,
            text: block.text,
            text_sha256: block.text_sha256,
            locator_type: block.locator_type,
            locator: block.locator,
        })
        .collect();
    let (_, _, profile) =
        resolve_chunker_config(&blocks, Some(KNOWLEDGE_CHUNK_TARGET_CHARS)).unwrap();
    let index = ks::KnowledgeIndexStore::open(&root).unwrap();
    let variant = index
        .ensure_chunk_index_variant(artifact_id, &profile, "not-yet-built")
        .unwrap();
    let observer = Connection::open(root.join("knowledge_index.db")).unwrap();
    if upgrade {
        observer.execute_batch("PRAGMA user_version=999").unwrap();
    }
    service.start_worker().unwrap();
    let job = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let job = facts
                .latest_job_for_source(notebook_id, source_id)
                .unwrap()
                .unwrap();
            if matches!(job.status.as_str(), "failed" | "pending_embedding") {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    service.shutdown();
    if upgrade {
        assert_eq!(job.status, "failed");
        assert!(job
            .error
            .as_deref()
            .unwrap()
            .contains("KNOWLEDGE_SCHEMA_NEWER"));
        assert_eq!(
            index.get_variant(&variant.id).unwrap().unwrap().status,
            "building"
        );
        assert_eq!(
            observer
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            999
        );
        assert!(index.list_variant_chunks(&variant.id).unwrap().is_empty());
        // 失败原由与状态一起可重开读取，不把兼容性拒绝当正常摄入。
        drop(observer);
        assert!(ks::KnowledgeIndexStore::open(&root).is_err());
        let reopened = ks::KnowledgeFactStore::open(&root).unwrap();
        let snapshot = reopened.latest_snapshot_for_source(source_id).unwrap();
        assert_eq!(
            reopened.read_snapshot_bytes(&snapshot).unwrap(),
            "独立摄入的真实原文".as_bytes()
        );
        assert_eq!(
            reopened
                .latest_job_for_source(notebook_id, source_id)
                .unwrap()
                .unwrap()
                .error,
            job.error
        );
    } else {
        assert_eq!(job.status, "pending_embedding");
        // 现役 worker 在派生算法版本化的缓存 profile 下建变体
        // （current_cache_profile_hash，4cdbd0f99 起）；本用例预建的原始
        // profile 变体扮演旧式残留——worker 不得复用它，也不得破坏它，
        // 必须把自己算法版本的变体建到 ready。
        let worker_variant_id = lingxi_kernel::knowledge::chunk_index_variant_id(
            artifact_id,
            &ks::current_cache_profile_hash(&profile),
        );
        assert_eq!(
            index.get_variant(&worker_variant_id).unwrap().unwrap().status,
            "ready"
        );
        assert!(!index
            .list_variant_chunks(&worker_variant_id)
            .unwrap()
            .is_empty());
        assert_eq!(
            index.get_variant(&variant.id).unwrap().unwrap().status,
            "building",
            "旧式残留变体必须保持原样，worker 不得静默接管"
        );
    }
}

#[tokio::test]
async fn ingestion_normal_positive_keeps_pending_embedding_and_ready_fulltext() {
    ingestion_case(false).await;
}

#[tokio::test]
async fn ingestion_future_schema_failure_keeps_new_index_status_and_reports_rejection() {
    ingestion_case(true).await;
}

struct LoopbackEmbedder(std::net::SocketAddr);
impl lingxi_service::knowledge::KnowledgeEmbedder for LoopbackEmbedder {
    fn identity(
        &self,
    ) -> Result<
        Option<lingxi_service::knowledge::EmbeddingModelIdentity>,
        lingxi_kernel::knowledge::KnowledgeError,
    > {
        Ok(Some(
            lingxi_service::knowledge::EmbeddingModelIdentity::new(
                "synthetic",
                "loopback",
                "test",
                2,
            ),
        ))
    }
    fn embed<'a>(
        &'a self,
        texts: Vec<String>,
        _: lingxi_service::knowledge::EmbeddingInputType,
    ) -> lingxi_service::knowledge::EmbedFuture<'a> {
        Box::pin(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let body = serde_json::to_string(&texts).unwrap();
            let mut socket = tokio::net::TcpStream::connect(self.0).await.unwrap();
            socket.write_all(format!("POST /embed HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", self.0, body.len(), body).as_bytes()).await.unwrap();
            let mut response = Vec::new();
            socket.read_to_end(&mut response).await.unwrap();
            let response = String::from_utf8(response).unwrap();
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            if !headers.starts_with("HTTP/1.1 200") {
                return Err(lingxi_kernel::knowledge::KnowledgeError::new(
                    "SYNTHETIC_PROVIDER_ERROR",
                    "loopback response failure",
                ));
            }
            Ok((
                self.identity()?.unwrap(),
                serde_json::from_str(body).unwrap(),
            ))
        })
    }
}

async fn vector_ingestion_upgrade_case(provider_error: bool) {
    let (home, _, _, _, _) = world();
    let db_path = home.join("knowledge/knowledge_vector.db");
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server_calls = calls.clone();
    let app = axum::Router::new().route(
        "/embed",
        axum::routing::post(move |axum::Json(texts): axum::Json<Vec<String>>| {
            let db_path = db_path.clone();
            let calls = server_calls.clone();
            async move {
                let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if call == 1 {
                    // 只有外部协议响应被替代；另一真实SQLite连接在首批写入后升级。
                    Connection::open(db_path)
                        .unwrap()
                        .execute_batch("PRAGMA user_version=999")
                        .unwrap();
                    if provider_error {
                        return (
                            axum::http::StatusCode::BAD_GATEWAY,
                            axum::Json(Vec::<Vec<f32>>::new()),
                        );
                    }
                }
                (
                    axum::http::StatusCode::OK,
                    axum::Json(vec![vec![0.2_f32, 0.8]; texts.len()]),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let service = std::sync::Arc::new(KnowledgeService::new(&home, "studio".into()).unwrap());
    let notebook = service.create_notebook("跨版本向量摄入").unwrap();
    let notebook_id = notebook["id"].as_str().unwrap();
    service.set_embedder(Some(std::sync::Arc::new(LoopbackEmbedder(address))));
    let imported = service
        .import_pasted_text(notebook_id, &"独立资料内容 ".repeat(30000), None)
        .unwrap();
    let source_id = imported["source"]["id"].as_str().unwrap();
    let facts = ks::KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    service.start_worker().unwrap();
    let job = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let job = facts
                .latest_job_for_source(notebook_id, source_id)
                .unwrap()
                .unwrap();
            if job.status == "failed" {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    service.shutdown();
    stop.send(()).unwrap();
    server.await.unwrap();
    assert!(job
        .error
        .as_deref()
        .unwrap()
        .contains("KNOWLEDGE_SCHEMA_NEWER"));
    if provider_error {
        assert!(job
            .error
            .as_deref()
            .unwrap()
            .contains("SYNTHETIC_PROVIDER_ERROR"));
        assert!(job
            .error
            .as_deref()
            .unwrap()
            .contains("cache status unchanged"));
    } else {
        // RV6-F12-01 回归闸：upsert 路径主/清理错误同码（均 SCHEMA_NEWER），
        // 追加语义的固定后缀是唯一可区分信号——替换语义下该后缀消失。
        assert!(
            job.error
                .as_deref()
                .unwrap()
                .contains("cleanup fail_build rejected"),
            "主错误必须保留且清理失败以追加出现（当前错误串：{}）",
            job.error.as_deref().unwrap()
        );
    }
    let observer = Connection::open(home.join("knowledge/knowledge_vector.db")).unwrap();
    assert_eq!(
        observer
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        999
    );
    let preserved: i64 = observer
        .query_row(
            "SELECT count(*) FROM chunk_vectors WHERE vector_index_variant_id IN (SELECT id FROM vector_index_variants WHERE parse_artifact_id=?1)",
            [imported["parseArtifact"]["id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved, 64);
    let status: String = observer
        .query_row(
            "SELECT status FROM vector_index_variants WHERE parse_artifact_id=?1",
            [imported["parseArtifact"]["id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "building");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn vector_ingestion_future_write_rejection_retains_paid_batch_and_error_code() {
    vector_ingestion_upgrade_case(false).await;
}

#[tokio::test]
async fn vector_ingestion_provider_failure_records_cleanup_version_rejection() {
    vector_ingestion_upgrade_case(true).await;
}
