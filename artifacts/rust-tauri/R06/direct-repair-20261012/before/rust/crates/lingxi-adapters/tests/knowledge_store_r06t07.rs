//! R06-T07 存储层测试：三库（facts / index FTS5 / vector）+ 快照文件存储。
//!
//! 索引兼容矩阵的存储语义：变体身份确定性、指纹门、维度门（禁混查）、
//! 缓存库损坏 reset 只重建缓存、schema newer 显式拒绝、研究引擎表缺席
//! （兼容残留叶：不恢复已退役的独立知识研究引擎）。

use std::path::PathBuf;

use lingxi_adapters::storage::knowledge as ks;

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-kstore-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

fn block(id: &str, ordinal: usize, text: &str, locator: serde_json::Value) -> ks::BlockRow {
    ks::BlockRow {
        id: id.to_string(),
        parse_artifact_id: "kpa_1".into(),
        ordinal,
        text: text.to_string(),
        text_sha256: ks::sha256_hex(text.as_bytes()),
        locator_type: "markdown".into(),
        locator,
    }
}

#[test]
fn identity_algorithms_are_deterministic_and_frozen() {
    // civ_/viv_ 的确定性派生（vector-index-adapter.ts:91-104 同算法）。
    let civ = ks::chunk_index_variant_id("kpa_1", "abcdef0123456789");
    assert!(civ.starts_with("civ_"));
    assert_eq!(civ.len(), 4 + 32);
    assert_eq!(civ, ks::chunk_index_variant_id("kpa_1", "abcdef0123456789"));
    assert_ne!(civ, ks::chunk_index_variant_id("kpa_2", "abcdef0123456789"));
    let viv = ks::vector_index_variant_id(&civ, "modelkey");
    assert!(viv.starts_with("viv_"));
    assert_eq!(viv, ks::vector_index_variant_id(&civ, "modelkey"));
    assert_ne!(viv, ks::vector_index_variant_id(&civ, "otherkey"));
    // modelKey = sha256(JSON[provider, modelId, protocol, dimensions])。
    let key = ks::embedding_model_key("embed", "embed-model-a", "openai-embeddings", 2);
    assert_eq!(key.len(), 64);
    assert_eq!(
        key,
        ks::embedding_model_key("embed", "embed-model-a", "openai-embeddings", 2),
        "model key is deterministic"
    );
    assert_ne!(
        key,
        ks::embedding_model_key("embed", "embed-model-a", "openai-embeddings", 3)
    );
}

#[test]
fn facts_store_crud_membership_and_turn_scope_freeze() {
    let root = unique_dir("facts");
    let store = ks::KnowledgeFactStore::open(&root).expect("open facts");

    let nb = store.create_notebook("studio", "笔记本A").expect("create");
    let nb2 = store.create_notebook("studio", "笔记本B").expect("create2");
    assert!(nb.id.starts_with("knb_"));

    // 来源 + 快照。
    let (source, snapshot) = store
        .create_source_with_snapshot(
            "studio",
            "pasted_text",
            "甲来源",
            &serde_json::json!({"kind": "pasted_text"}),
            b"snap-bytes",
            "text/plain",
        )
        .expect("source");
    store
        .add_membership(nb.id.as_str(), source.id.as_str())
        .expect("membership");
    let entries = store
        .list_notebook_sources(nb.id.as_str())
        .expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source.id, source.id);

    // turn scope 冻结：同会话旧 scope 关闭 + 逐源冻结最新快照/产物。
    store
        .begin_parse_artifact("kpa_1", snapshot.id.as_str(), "lingxi-citation", "1", "cfg")
        .expect("begin artifact");
    store
        .complete_parse_artifact("kpa_1", "ready", &[], "citation_grade", None)
        .expect("complete artifact");
    store
        .add_membership(nb2.id.as_str(), source.id.as_str())
        .expect("membership 2");
    let scope = store
        .create_turn_scope(
            "studio",
            "sess-1",
            "turn-1",
            &[nb.id.clone(), nb2.id.clone()],
        )
        .expect("scope");
    assert_eq!(
        scope.sources.len(),
        1,
        "same source merged across notebooks"
    );
    assert_eq!(scope.sources[0].parse_artifact_id.as_deref(), Some("kpa_1"));
    assert_eq!(scope.sources[0].notebook_ids.len(), 2);

    let scope2 = store
        .create_turn_scope("studio", "sess-1", "turn-2", std::slice::from_ref(&nb.id))
        .expect("scope 2");
    let old = store
        .get_turn_scope(scope.id.as_str())
        .expect("old")
        .expect("exists");
    assert_eq!(
        old.status, "closed",
        "new scope supersedes the previous active one"
    );
    assert_eq!(scope2.status, "active");

    // membership remove ≠ 删除源。
    store
        .remove_source_from_notebook(nb2.id.as_str(), source.id.as_str())
        .expect("remove");
    let entries = store
        .list_notebook_sources(nb.id.as_str())
        .expect("entries");
    assert_eq!(entries.len(), 1, "other membership survives");

    // delete wins：标记删除后 ensure 拒绝。
    store.delete_source(source.id.as_str()).expect("delete");
    assert!(store.get_source(source.id.as_str()).is_err());
}

#[test]
fn schema_newer_fails_loud() {
    let root = unique_dir("schema");
    {
        let store = ks::KnowledgeFactStore::open(&root).expect("open");
        drop(store);
    }
    // 伪装未来版本。
    let conn = rusqlite::Connection::open(root.join("knowledge.db")).expect("raw open");
    conn.pragma_update(None, "user_version", 99).expect("bump");
    drop(conn);
    let err = match ks::KnowledgeFactStore::open(&root) {
        Err(err) => err,
        Ok(_) => panic!("newer schema must be refused loudly"),
    };
    assert_eq!(err.code, "KNOWLEDGE_SCHEMA_NEWER", "{err}");
}

#[test]
fn index_store_fingerprint_gate_and_fts_search() {
    let root = unique_dir("index");
    let store = ks::KnowledgeIndexStore::open(&root).expect("open index");
    let profile = "0123456789abcdef";
    let variant = store
        .ensure_chunk_index_variant("kpa_1", profile, "fingerprint-1")
        .expect("ensure variant");

    // 分块 + FTS。
    let chunks = vec![ks::ChunkDraft {
        id: format!("chunk_{}", &ks::sha256_hex(b"chunk-0")[..32]),
        ordinal: 0,
        section_id: None,
        text: "灵犀知识库检索验证正文".into(),
        token_count: 10,
        spans: vec![ks::ChunkSpan {
            block_id: "kb_0".into(),
            block_start_offset: 0,
            block_end_offset: 11,
            chunk_start_offset: 0,
            chunk_end_offset: 11,
        }],
    }];
    store
        .replace_artifact_chunks(
            "kpa_1",
            profile,
            "fingerprint-1",
            &chunks,
            &[ks::SectionMetaRow {
                id: "section_1".into(),
                section_ordinal: 0,
                heading_path: vec!["第一章".into()],
                start_block_ordinal: 0,
                end_block_ordinal: 0,
            }],
        )
        .expect("replace chunks");
    store
        .set_variant_status(variant.id.as_str(), "ready")
        .expect("ready");

    // 同指纹命中 → 无需重建（指纹门在完整替换之后判定）。
    assert!(
        store
            .has_artifact_fingerprint("kpa_1", profile, "fingerprint-1")
            .expect("gate"),
        "same fingerprint is a hit"
    );

    let hits = store
        .search_fts(
            &[variant.id.as_str()],
            &ks::build_fts_literal_query("灵犀 检索"),
            10,
        )
        .expect("fts");
    assert!(
        !hits.is_empty(),
        "FTS finds the chunk: {:?}",
        hits.iter()
            .map(|h| h.chunk.chunk_id.clone())
            .collect::<Vec<_>>()
    );
    let empty = store
        .search_fts(
            &[variant.id.as_str()],
            &ks::build_fts_literal_query("绝不存在的字面量zzz"),
            10,
        )
        .expect("fts");
    assert!(empty.is_empty());

    // 指纹漂移 → gate 失败（重建在途降级的依据）。
    assert!(
        !store
            .has_artifact_fingerprint("kpa_1", profile, "fingerprint-2")
            .expect("gate"),
        "drifted fingerprint is a miss"
    );
}

#[test]
fn vector_store_dimension_gate_refuses_mixed_search() {
    let root = unique_dir("vector");
    let store = ks::KnowledgeVectorStore::open(&root).expect("open vector");
    let civ = ks::chunk_index_variant_id("kpa_1", "abcdef0123456789");
    let key_a = ks::embedding_model_key("embed", "embed-model-a", "openai-embeddings", 2);
    let build = store
        .begin_variant_build(&civ, "kpa_1", &key_a, "fp-1", 2)
        .expect("begin build");
    assert!(!build.reset_stale);
    store
        .upsert_batch(
            build.variant_id.as_str(),
            &[ks::VectorEntry {
                chunk_id: "chunk_0".into(),
                ordinal: 0,
                vector: vec![0.1, 0.2],
            }],
        )
        .expect("upsert");
    // 维度与变体不符的写入被拒（KNOWLEDGE_CONFLICT——禁混写）。
    let err = store
        .upsert_batch(
            build.variant_id.as_str(),
            &[ks::VectorEntry {
                chunk_id: "chunk_1".into(),
                ordinal: 1,
                vector: vec![0.1, 0.2, 0.3],
            }],
        )
        .expect_err("mixed-dimension write must be refused");
    assert_eq!(err.code, "KNOWLEDGE_CONFLICT", "{err}");
    store
        .complete_build(build.variant_id.as_str(), 1)
        .expect("complete");

    let variant = store
        .get_variant(build.variant_id.as_str())
        .expect("variant")
        .expect("exists");
    assert_eq!(variant.dimensions, 2);
    assert_eq!(variant.status, "ready");
    assert_eq!(variant.model_key, key_a);

    // 检索只在给定变体集合内；查询向量维度不符 → 显式错误（绝不返回貌似正常结果）。
    let hits = store
        .search(std::slice::from_ref(&build.variant_id), &[0.1, 0.2], 5)
        .expect("search");
    assert_eq!(hits.len(), 1);
    let err = store
        .search(std::slice::from_ref(&build.variant_id), &[0.1, 0.2, 0.3], 5)
        .expect_err("dimension mismatch must refuse the search");
    assert_eq!(err.code, "KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "{err}");
    // 未知变体 id → 空结果（调用方降级），绝不跨变体兜底。
    let none = store
        .search(&["viv_missing".to_string()], &[0.1, 0.2], 5)
        .expect("search");
    assert!(none.is_empty());
}

#[test]
fn cache_corruption_resets_only_the_cache_file() {
    let root = unique_dir("corrupt");
    let facts = ks::KnowledgeFactStore::open(&root).expect("facts");
    let index = ks::KnowledgeIndexStore::open(&root).expect("index");
    drop(index);
    // 破坏索引库文件。
    std::fs::write(root.join("knowledge_index.db"), b"not a sqlite file at all").expect("corrupt");
    // 打开失败 → 按缓存语义 reset 重建（facts 不受影响）。
    let index = ks::KnowledgeIndexStore::open(&root).expect("reopen resets the cache");
    assert!(index
        .list_ready_variant_metadata("kpa_1")
        .expect("query")
        .is_empty());
    assert!(
        facts.list_notebooks("studio").is_ok(),
        "facts store untouched"
    );
}

#[test]
fn citation_roundtrip_and_offsets() {
    let root = unique_dir("citation");
    let store = ks::KnowledgeFactStore::open(&root).expect("open");
    let (_, snapshot) = store
        .create_source_with_snapshot(
            "studio",
            "pasted_text",
            "源",
            &serde_json::json!({}),
            b"bytes",
            "text/plain",
        )
        .expect("source");
    store
        .begin_parse_artifact("kpa_1", snapshot.id.as_str(), "lingxi-citation", "1", "cfg")
        .expect("begin");
    store
        .complete_parse_artifact("kpa_1", "ready", &[], "citation_grade", None)
        .expect("complete");
    store
        .insert_blocks(&[block(
            "kb_0",
            0,
            "引用定位验证段落。",
            serde_json::json!({"headingPath": []}),
        )])
        .expect("blocks");

    let citation = store
        .create_citation("kpa_1", "kb_0", 0, 8)
        .expect("citation");
    assert_eq!(citation.canonical_text, "引用定位验证段落");
    let resolved = store
        .resolve_citation(citation.id.as_str())
        .expect("resolve");
    assert_eq!(resolved.block.text, "引用定位验证段落。");
    assert_eq!(resolved.snapshot.id, snapshot.id);
    assert_eq!(resolved.source.display_name, "源");
    // 越界偏移显式拒绝。
    let err = store
        .create_citation("kpa_1", "kb_0", 5, 999)
        .expect_err("out of range");
    assert_eq!(err.code, "KNOWLEDGE_INDEX_INVALID", "{err}");
}

#[test]
fn ingestion_job_lifecycle_and_delete_wins() {
    let root = unique_dir("jobs");
    let store = ks::KnowledgeFactStore::open(&root).expect("open");
    let nb = store.create_notebook("studio", "库").expect("nb");
    let (source, _) = store
        .create_source_with_snapshot(
            "studio",
            "pasted_text",
            "源",
            &serde_json::json!({}),
            b"b",
            "text/plain",
        )
        .expect("source");
    store
        .add_membership(nb.id.as_str(), source.id.as_str())
        .expect("membership");

    let job = store
        .enqueue_source_ingestion(nb.id.as_str(), source.id.as_str(), None, "cfg")
        .expect("enqueue");
    assert_eq!(job.status, "queued");

    let claimed = store.claim_next_job().expect("claim").expect("job");
    assert_eq!(claimed.id, job.id);
    assert_eq!(claimed.status, "running");

    // done 之后的 requeue 拒绝（409 语义由服务层映射）。
    store
        .complete_job(
            job.id.as_str(),
            &serde_json::json!({"chunksNewlyEmbedded": 0}),
        )
        .expect("complete");
    let err = store
        .requeue_job(job.id.as_str())
        .expect_err("done job is not requeueable");
    assert_eq!(err.code, "KNOWLEDGE_CONFLICT", "{err}");

    // pending_embedding 显式终态 + 恢复重排。
    let job2 = store
        .enqueue_source_ingestion(nb.id.as_str(), source.id.as_str(), None, "cfg")
        .expect("enqueue 2");
    store
        .mark_job_pending_embedding(job2.id.as_str())
        .expect("pending embedding");
    let resumed = store.requeue_pending_embedding_jobs().expect("requeue");
    assert_eq!(resumed, 1);

    // delete wins：取消活跃 job；cancelled job 不可 requeue。
    let job3 = store
        .enqueue_source_ingestion(nb.id.as_str(), source.id.as_str(), None, "cfg")
        .expect("enqueue 3");
    let cancelled = store
        .cancel_jobs_for_source(source.id.as_str())
        .expect("cancel");
    assert_eq!(cancelled, 2, "requeued job2 and fresh job3 are both active");
    let err = store
        .requeue_job(job3.id.as_str())
        .expect_err("cancelled job is not requeueable");
    assert_eq!(err.code, "KNOWLEDGE_CONFLICT", "{err}");
}

#[test]
fn research_engine_tables_are_absent_from_the_new_store() {
    let root = unique_dir("noresearch");
    let store = ks::KnowledgeFactStore::open(&root).expect("open");
    for table in [
        "research_runs",
        "analysis_manifests",
        "analysis_units",
        "coverage_runs",
        "knowledge_runs",
        "research_jobs",
    ] {
        assert!(
            !store.has_table(table),
            "the retired research engine table {table} must not be restored"
        );
    }
    for table in [
        "notebooks",
        "sources",
        "content_snapshots",
        "knowledge_turn_scopes",
        "ingestion_jobs",
    ] {
        assert!(store.has_table(table), "active table {table} must exist");
    }
}
