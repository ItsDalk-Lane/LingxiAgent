//! EX09：真实SQLite检索派生层；原文与引用不能因归一化被改写。
use lingxi_adapters::storage::knowledge as ks;
use std::path::PathBuf;
fn root(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "lingxi-ex09-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn draft(id: &str, text: &str) -> ks::ChunkDraft {
    ks::ChunkDraft {
        id: id.into(),
        ordinal: 0,
        section_id: None,
        text: text.into(),
        token_count: 5,
        spans: vec![],
    }
}
#[test]
fn real_fts_ascii_words_do_not_degrade_into_individual_letter_matches() {
    let p = root("ascii");
    let s = ks::KnowledgeIndexStore::open(&p).unwrap();
    let a = s
        .replace_artifact_chunks(
            "artifact-a",
            "profile",
            "fp-a",
            &[draft("alpha-chunk", "alpha")],
            &[],
        )
        .unwrap();
    let b = s
        .replace_artifact_chunks(
            "artifact-b",
            "profile",
            "fp-b",
            &[draft("beta-chunk", "beta")],
            &[],
        )
        .unwrap();
    let q = ks::build_fts_literal_query("alpha");
    let hits = s.search_fts(&[&a, &b], &q, 10).unwrap();
    assert_eq!(
        hits.iter()
            .map(|h| h.chunk.chunk_id.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha-chunk"],
        "查询不得退化为逐字母OR: {q}"
    );
    drop(s);
    std::fs::remove_dir_all(p).unwrap();
}
#[test]
fn real_fts_fullwidth_and_two_cjk_characters_find_only_matching_original_chunks() {
    let p = root("nfkc-cjk");
    let s = ks::KnowledgeIndexStore::open(&p).unwrap();
    let original = "Ｑｕａｒｔｚ　上海规则";
    let a = s
        .replace_artifact_chunks(
            "artifact-a",
            "profile",
            "fp-a",
            &[draft("target", original)],
            &[],
        )
        .unwrap();
    let b = s
        .replace_artifact_chunks(
            "artifact-b",
            "profile",
            "fp-b",
            &[draft("other", "北京说明")],
            &[],
        )
        .unwrap();
    for word in ["quartz", "上海"] {
        let q = ks::build_fts_literal_query(word);
        assert!(!q.is_empty(), "双字查询必须保留搜索词");
        let hits = s.search_fts(&[&a, &b], &q, 10).unwrap();
        assert_eq!(hits.len(), 1, "{word}: {q}");
        assert_eq!(hits[0].chunk.chunk_id, "target");
        assert_eq!(hits[0].chunk.text, original);
    }
    drop(s);
    std::fs::remove_dir_all(p).unwrap();
}
#[test]
fn jieba_search_produces_dictionary_words_beyond_fixed_ngrams() {
    let tokens = ks::tokenize_search_text("南京市长江大桥");
    for word in ["南京市", "长江大桥"] {
        assert!(
            tokens.iter().any(|t| t == word),
            "真实Jieba搜索词缺席: {tokens:?}"
        );
    }
    assert!(!tokens.iter().any(|t| t == "南"));
}

#[test]
fn normalization_scripts_dedup_punctuation_and_96_literal_terms_follow_actual_search_rules() {
    assert_eq!(
        ks::normalize_search_text("\u{feff}ＦＯＯ\tK　\nBAR\u{feff}"),
        "foo k bar"
    );
    assert_eq!(
        ks::normalize_search_text("a\u{85}b"),
        "a\u{85}b",
        "不扩张ECMAScript空白集合"
    );
    let tokens = ks::tokenize_search_text(
        "Ｆｏｏ-bar.BAZ_17 foo-bar.baz_17 上海 とうきょう カタカナ 한국어 𠀀𠀁 。！？★",
    );
    assert_eq!(tokens[0], "foo-bar.baz_17");
    assert_eq!(
        tokens
            .iter()
            .filter(|s| s.as_str() == "foo-bar.baz_17")
            .count(),
        1
    );
    for term in ["上海", "とう", "カタ", "한국", "𠀀𠀁"] {
        assert!(tokens.iter().any(|s| s == term), "{term}: {tokens:?}");
    }
    for term in ["上", "𠀀", "。", "★"] {
        assert!(!tokens.iter().any(|s| s == term));
    }
    assert!(ks::tokenize_search_text("，。！？★").is_empty());
    let query = ks::build_fts_literal_query(
        &(0..160)
            .map(|n| format!("word{n}"))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let terms = query.split(" OR ").collect::<Vec<_>>();
    assert_eq!(terms.len(), 96);
    assert_eq!(terms[0], "\"word0\"");
    assert_eq!(terms[95], "\"word95\"");
    let p = root("injection");
    let s = ks::KnowledgeIndexStore::open(&p).unwrap();
    let id = s
        .replace_artifact_chunks(
            "artifact",
            "profile",
            "fp",
            &[draft("chunk", "ordinary evidence")],
            &[],
        )
        .unwrap();
    for input in ["\" OR *", "NEAR(ordinary, evidence)", "{text}:* NOT x"] {
        let q = ks::build_fts_literal_query(input);
        assert!(!q.is_empty());
        s.search_fts(&[&id], &q, 10)
            .expect("只接受字面词，不能把输入操作符传给MATCH解析器");
    }
    drop(s);
    std::fs::remove_dir_all(p).unwrap();
}

fn import_legacy(p: &std::path::Path) -> serde_json::Value {
    for (name, sql) in [
        (
            "knowledge_index.db",
            include_str!("fixtures/ex09-legacy-index.sql"),
        ),
        (
            "knowledge_vector.db",
            include_str!("fixtures/ex09-legacy-vector.sql"),
        ),
    ] {
        let conn = rusqlite::Connection::open(p.join(name)).unwrap();
        conn.execute_batch(sql).unwrap();
    }
    serde_json::from_str(include_str!("fixtures/ex09-legacy-source.json")).unwrap()
}
fn raw_chunks(p: &std::path::Path) -> Vec<(String, String, String)> {
    let c = rusqlite::Connection::open(p.join("knowledge_index.db")).unwrap();
    let rows = c
        .prepare(
            "SELECT chunk_id,text,search_text FROM knowledge_chunks ORDER BY variant_id,ordinal",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    rows
}
fn raw_vectors(p: &std::path::Path) -> Vec<(String, String, String)> {
    let c = rusqlite::Connection::open(p.join("knowledge_vector.db")).unwrap();
    let rows = c.prepare("SELECT vector_index_variant_id,chunk_id,hex(vector) FROM chunk_vectors ORDER BY vector_index_variant_id,ordinal").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    rows
}
fn original_block(source: &serde_json::Value) -> ks::ChunkerBlock {
    let text = source["original"].as_str().unwrap();
    ks::ChunkerBlock {
        id: "block".into(),
        ordinal: 0,
        text: text.into(),
        text_sha256: ks::sha256_hex(text.as_bytes()),
        locator_type: "text".into(),
        locator: serde_json::json!({}),
    }
}
#[test]
fn actual_old_gapped_sqlite_cache_is_stale_then_explicit_rebuild_preserves_source_citations_and_old_vectors(
) {
    let p = root("old-gapped");
    let old = import_legacy(&p);
    let original = old["original"].as_str().unwrap();
    let old_profile = old["profile"].as_str().unwrap();
    let fingerprint = old["fingerprint"].as_str().unwrap();
    let old_civ = old["civ"].as_str().unwrap();
    let old_viv = old["viv"].as_str().unwrap();
    assert_eq!(
        ks::chunk_index_variant_id("artifact", old_profile),
        old_civ,
        "原历史civ算法不变"
    );
    assert_eq!(
        ks::vector_index_variant_id(old_civ, "old-model"),
        old_viv,
        "原历史viv算法不变"
    );
    let before_chunks = raw_chunks(&p);
    let before_vectors = raw_vectors(&p);
    assert!(original.contains("上海"));
    assert!(
        before_chunks
            .iter()
            .all(|(_, text, _)| !text.contains("上海")),
        "真实旧生产forward-overlap产物遗漏原文字节"
    );
    let facts = ks::KnowledgeFactStore::open(&p).unwrap();
    let (_, snapshot) = facts
        .create_source_with_snapshot(
            "studio",
            "pasted_text",
            "旧资料",
            &serde_json::json!({}),
            original.as_bytes(),
            "text/plain",
        )
        .unwrap();
    facts
        .begin_parse_artifact("artifact", &snapshot.id, "text", "1", "config")
        .unwrap();
    facts
        .insert_blocks(&[ks::BlockRow {
            id: "block".into(),
            parse_artifact_id: "artifact".into(),
            ordinal: 0,
            text: original.into(),
            text_sha256: ks::sha256_hex(original.as_bytes()),
            locator_type: "text".into(),
            locator: serde_json::json!({}),
        }])
        .unwrap();
    facts
        .complete_parse_artifact("artifact", "ready", &[], "citation_grade", None)
        .unwrap();
    let width_citation = facts.create_citation("artifact", "block", 0, 6).unwrap();
    let gap_citation = facts
        .create_citation("artifact", "block", 100, 104)
        .unwrap();
    assert_eq!(width_citation.canonical_text, "Ｑｕａｒｔｚ");
    assert_eq!(gap_citation.canonical_text, "上海失落");
    let index = ks::KnowledgeIndexStore::open(&p).unwrap();
    let vectors = ks::KnowledgeVectorStore::open(&p).unwrap();
    assert_eq!(
        index.derived_algorithm_status(old_civ).unwrap(),
        Some(ks::DerivedAlgorithmStatus::LegacyStale)
    );
    assert_eq!(index.get_variant(old_civ).unwrap().unwrap().status, "stale");
    assert!(index.artifact_has_stale_variants("artifact").unwrap());
    assert!(
        !index
            .has_artifact_fingerprint("artifact", old_profile, fingerprint)
            .unwrap(),
        "同原profile与blocks指纹不得偷复用旧漏字算法"
    );
    assert!(index
        .get_ready_variant_metadata("artifact", old_profile)
        .unwrap()
        .is_none());
    assert!(index
        .list_ready_variant_metadata("artifact")
        .unwrap()
        .is_empty());
    assert!(index.list_variant_chunks(old_civ).is_err());
    assert!(index
        .search_fts(&[old_civ], &ks::build_fts_literal_query("上海"), 10)
        .is_err());
    assert_eq!(raw_chunks(&p), before_chunks);
    assert_eq!(raw_vectors(&p), before_vectors);
    assert_eq!(
        vectors.get_variant(old_viv).unwrap().unwrap().status,
        "stale"
    );
    assert!(vectors.checkpoint_entries(old_viv).is_err());
    assert!(
        vectors.search(&[old_viv.into()], &[1.0, 0.0], 10).is_err(),
        "显式选择旧向量不能静默正常返回"
    );
    assert!(
        vectors
            .begin_variant_build(old_civ, "artifact", "old-model", "old-chunk-fingerprint", 2)
            .is_err(),
        "同旧paid身份不得冒认新checkpoint"
    );
    let blocks = vec![original_block(&old)];
    let current_profile = ks::current_cache_profile_hash(old_profile);
    assert_ne!(current_profile, old_profile);
    assert_eq!(current_profile, ks::current_cache_profile_hash(old_profile));
    let chunks = ks::build_chunks("artifact", &blocks, Some(100)).unwrap();
    assert!(
        chunks.iter().any(|c| c.text.contains("上海")),
        "真实当前反向overlap必须补回遗漏原文"
    );
    let new_civ = index
        .replace_artifact_chunks("artifact", &current_profile, fingerprint, &chunks, &[])
        .unwrap();
    assert_ne!(new_civ, old_civ);
    assert!(index
        .has_artifact_fingerprint("artifact", &current_profile, fingerprint)
        .unwrap());
    for term in ["quartz", "上海"] {
        let hits = index
            .search_fts(&[&new_civ], &ks::build_fts_literal_query(term), 10)
            .unwrap();
        assert!(!hits.is_empty(), "新派生FTS真实召回旧漏字: {term}");
        assert!(hits
            .iter()
            .all(|hit| chunks.iter().any(|c| c.id == hit.chunk.chunk_id
                && c.text == hit.chunk.text
                && c.spans == hit.chunk.spans)));
    }
    let new_build = vectors
        .begin_variant_build(
            &new_civ,
            "artifact",
            "old-model",
            "old-chunk-fingerprint",
            2,
        )
        .unwrap();
    assert_ne!(new_build.variant_id, old_viv);
    assert!(
        vectors
            .checkpoint_entries(&new_build.variant_id)
            .unwrap()
            .is_empty(),
        "同模型/指纹也不能拿旧代际向量当新结果"
    );
    vectors
        .upsert_batch(
            &new_build.variant_id,
            &chunks
                .iter()
                .map(|c| ks::VectorEntry {
                    chunk_id: c.id.clone(),
                    ordinal: c.ordinal,
                    vector: vec![0.0, 1.0],
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
    vectors
        .complete_build_plan(
            &new_build.variant_id,
            &chunks
                .iter()
                .map(|c| (c.id.clone(), c.ordinal))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let matches = vectors
        .search(
            std::slice::from_ref(&new_build.variant_id),
            &[0.0, 1.0],
            100,
        )
        .unwrap();
    assert_eq!(matches.len(), chunks.len());
    assert!(matches.iter().all(|h| h.variant_id == new_build.variant_id));
    assert_eq!(
        raw_vectors(&p)
            .into_iter()
            .filter(|(v, _, _)| v == old_viv)
            .collect::<Vec<_>>(),
        before_vectors
    );
    assert_eq!(
        facts.read_snapshot_bytes(&snapshot).unwrap(),
        original.as_bytes()
    );
    assert_eq!(
        facts.list_artifact_blocks("artifact").unwrap()[0].text,
        original
    );
    assert_eq!(
        facts
            .resolve_citation(&width_citation.id)
            .unwrap()
            .citation
            .canonical_text,
        width_citation.canonical_text
    );
    assert_eq!(
        facts
            .resolve_citation(&gap_citation.id)
            .unwrap()
            .citation
            .canonical_text,
        gap_citation.canonical_text
    );
    drop(index);
    drop(vectors);
    let restarted = ks::KnowledgeIndexStore::open(&p).unwrap();
    assert!(restarted
        .has_artifact_fingerprint("artifact", &current_profile, fingerprint)
        .unwrap());
    assert_eq!(
        restarted.get_variant(old_civ).unwrap().unwrap().status,
        "stale"
    );
    drop(restarted);
    drop(facts);
    std::fs::remove_dir_all(p).unwrap();
}
#[test]
fn failed_explicit_replacement_cannot_promote_legacy_tag_or_erase_original_cached_rows() {
    let p = root("legacy-atomic");
    let old = import_legacy(&p);
    let before = raw_chunks(&p);
    let s = ks::KnowledgeIndexStore::open(&p).unwrap();
    let profile = old["profile"].as_str().unwrap();
    let fp = old["fingerprint"].as_str().unwrap();
    let id = old["civ"].as_str().unwrap();
    assert!(s
        .replace_artifact_chunks(
            "artifact",
            profile,
            fp,
            &[
                draft("duplicate-1", "replacement"),
                draft("duplicate-2", "replacement")
            ],
            &[]
        )
        .is_err());
    assert_eq!(raw_chunks(&p), before);
    assert_eq!(s.get_variant(id).unwrap().unwrap().status, "stale");
    assert!(!s.has_artifact_fingerprint("artifact", profile, fp).unwrap());
    assert!(
        s.set_variant_status(id, "ready").is_err(),
        "单改状态不能把旧算法升级为当前"
    );
    let current = ks::build_chunks("artifact", &[original_block(&old)], Some(100)).unwrap();
    s.replace_artifact_chunks("artifact", profile, fp, &current, &[])
        .unwrap();
    assert!(s.has_artifact_fingerprint("artifact", profile, fp).unwrap());
    assert_eq!(
        s.derived_algorithm_status(id).unwrap(),
        Some(ks::DerivedAlgorithmStatus::Current)
    );
    drop(s);
    std::fs::remove_dir_all(p).unwrap();
}
#[test]
fn unknown_or_damaged_algorithm_metadata_is_preserved_and_never_reclassified_as_legacy() {
    for mutation in [
        "UPDATE knowledge_derived_algorithms SET algorithm='unknown-v99'",
        "DELETE FROM knowledge_derived_algorithms",
        "DROP TABLE knowledge_derived_algorithms",
        "PRAGMA application_id=123",
    ] {
        let p = root("metadata-refuse");
        let s = ks::KnowledgeIndexStore::open(&p).unwrap();
        s.replace_artifact_chunks(
            "artifact",
            "profile",
            "fp",
            &[draft("chunk", "original")],
            &[],
        )
        .unwrap();
        drop(s);
        let db = p.join("knowledge_index.db");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(mutation).unwrap();
        c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
        drop(c);
        let before = std::fs::read(&db).unwrap();
        let error = ks::KnowledgeIndexStore::open(&p)
            .err()
            .expect("坏元数据必须拒绝");
        assert_eq!(error.code, "KNOWLEDGE_STORAGE_INVALID", "{error}");
        assert_eq!(std::fs::read(&db).unwrap(), before);
        assert!(!std::fs::read_dir(&p).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("quarantine")));
        std::fs::remove_dir_all(p).unwrap();
    }
}
#[test]
fn algorithm_metadata_is_removed_with_real_cache_cleanup_and_future_schema_still_wins() {
    let p = root("metadata-cleanup");
    let s = ks::KnowledgeIndexStore::open(&p).unwrap();
    let id = s
        .replace_artifact_chunks(
            "artifact",
            "profile",
            "fp",
            &[draft("chunk", "original")],
            &[],
        )
        .unwrap();
    s.remove_artifact("artifact").unwrap();
    s.replace_artifact_chunks("next", "profile", "fp", &[draft("next", "next")], &[])
        .unwrap();
    s.reset().unwrap();
    drop(s);
    let v = ks::KnowledgeVectorStore::open(&p).unwrap();
    let b = v
        .begin_variant_build(&id, "artifact", "model", "fp", 2)
        .unwrap();
    v.remove_artifact("artifact").unwrap();
    v.begin_variant_build("new-civ", "next", "model", "fp", 2)
        .unwrap();
    drop(v);
    let index = ks::KnowledgeIndexStore::open(&p).unwrap();
    drop(index);
    let vector = ks::KnowledgeVectorStore::open(&p).unwrap();
    drop(vector);
    let c = rusqlite::Connection::open(p.join("knowledge_vector.db")).unwrap();
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM knowledge_derived_algorithms WHERE variant_id=?1",
            [b.variant_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    c.execute_batch("UPDATE knowledge_derived_algorithms SET algorithm='unknown'; PRAGMA user_version=99; PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    drop(c);
    assert_eq!(
        ks::KnowledgeVectorStore::open(&p).err().unwrap().code,
        "KNOWLEDGE_SCHEMA_NEWER"
    );
    std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn live_corrupted_algorithm_metadata_refuses_ready_reads_and_vector_resume_without_payload_changes()
{
    let p = root("live-metadata");
    let index = ks::KnowledgeIndexStore::open(&p).unwrap();
    let civ = index
        .replace_artifact_chunks(
            "artifact",
            "profile",
            "fp",
            &[draft("chunk", "original")],
            &[],
        )
        .unwrap();
    let vector = ks::KnowledgeVectorStore::open(&p).unwrap();
    let build = vector
        .begin_variant_build(&civ, "artifact", "model", "fp", 2)
        .unwrap();
    let entry = ks::VectorEntry {
        chunk_id: "chunk".into(),
        ordinal: 0,
        vector: vec![1.0, 0.0],
    };
    vector
        .upsert_batch(&build.variant_id, std::slice::from_ref(&entry))
        .unwrap();
    vector
        .complete_build_plan(&build.variant_id, &[("chunk".into(), 0)])
        .unwrap();
    let before_index = raw_chunks(&p);
    let before_vector = raw_vectors(&p);
    for name in ["knowledge_index.db", "knowledge_vector.db"] {
        let c = rusqlite::Connection::open(p.join(name)).unwrap();
        c.execute(
            "UPDATE knowledge_derived_algorithms SET algorithm='unknown'",
            [],
        )
        .unwrap();
    }
    assert_eq!(
        index
            .has_artifact_fingerprint("artifact", "profile", "fp")
            .unwrap_err()
            .code,
        "KNOWLEDGE_STORAGE_INVALID"
    );
    assert!(index
        .get_ready_variant_metadata("artifact", "profile")
        .is_err());
    assert!(index
        .replace_artifact_chunks(
            "artifact",
            "profile",
            "fp",
            &[draft("new", "not committed")],
            &[]
        )
        .is_err());
    assert!(vector.get_variant(&build.variant_id).is_err());
    assert!(vector.list_variants_by_artifact("artifact").is_err());
    assert!(vector.checkpoint_entries(&build.variant_id).is_err());
    assert!(vector.upsert_batch(&build.variant_id, &[entry]).is_err());
    assert!(vector
        .complete_build_plan(&build.variant_id, &[("chunk".into(), 0)])
        .is_err());
    assert_eq!(raw_chunks(&p), before_index);
    assert_eq!(raw_vectors(&p), before_vector);
    drop(index);
    drop(vector);
    std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn actual_current_typescript_tokenizer_corpus_matches_without_rewriting_reference() {
    // 直接执行现役TS模块生成；没有改动现役参考或原有golden。
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/ex09-current-ts-search.json")).unwrap();
    for case in cases {
        let input = case["value"].as_str().unwrap();
        assert_eq!(
            ks::normalize_search_text(input),
            case["normalized"].as_str().unwrap(),
            "normalized: {input}"
        );
        assert_eq!(
            serde_json::json!(ks::tokenize_search_text(input)),
            case["tokens"],
            "tokens: {input}"
        );
        assert_eq!(
            ks::build_fts_literal_query(input),
            case["literal"].as_str().unwrap(),
            "literal: {input}"
        );
    }
}

#[test]
fn unknown_legacy_shape_and_corrupt_legacy_vectors_are_refused_without_upgrading_or_quarantine() {
    let p = root("unknown-old-shape");
    let db = p.join("knowledge_index.db");
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch(
        "CREATE TABLE chunk_index_variants(id TEXT PRIMARY KEY); PRAGMA user_version=1;",
    )
    .unwrap();
    drop(c);
    let before = std::fs::read(&db).unwrap();
    assert_eq!(
        ks::KnowledgeIndexStore::open(&p).err().unwrap().code,
        "KNOWLEDGE_STORAGE_INVALID"
    );
    assert_eq!(std::fs::read(&db).unwrap(), before);
    std::fs::remove_dir_all(p).unwrap();
    let p = root("corrupt-old-vector");
    import_legacy(&p);
    let db = p.join("knowledge_vector.db");
    let c = rusqlite::Connection::open(&db).unwrap();
    let mut bytes = f32::NAN.to_le_bytes().to_vec();
    bytes.extend_from_slice(&0_f32.to_le_bytes());
    c.execute("UPDATE chunk_vectors SET vector=?1", [bytes])
        .unwrap();
    drop(c);
    let before = std::fs::read(&db).unwrap();
    assert_eq!(
        ks::KnowledgeVectorStore::open(&p).err().unwrap().code,
        "KNOWLEDGE_STORAGE_INVALID"
    );
    assert_eq!(std::fs::read(&db).unwrap(), before);
    let c = rusqlite::Connection::open(&db).unwrap();
    let tables: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='knowledge_derived_algorithms'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0, "坏旧库不能先被登记成合法LegacyStale");
    drop(c);
    assert!(!std::fs::read_dir(&p).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("quarantine")));
    std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn supplementary_han_queries_keep_utf16_tokens_and_never_match_unrelated_documents() {
    let p = root("supplementary-han");
    let index = ks::KnowledgeIndexStore::open(&p).unwrap();
    let target = index
        .replace_artifact_chunks("target", "p", "fp", &[draft("target-chunk", "𠀀 𰀀")], &[])
        .unwrap();
    let decoy = index
        .replace_artifact_chunks(
            "decoy",
            "p",
            "fp",
            &[draft("decoy-chunk", "unrelated quartz")],
            &[],
        )
        .unwrap();
    for query in ["𠀀", "𰀀"] {
        assert_eq!(ks::tokenize_search_text(query), vec![query]);
        let hits = index
            .search_fts(&[&target, &decoy], &ks::build_fts_literal_query(query), 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk.chunk_id, "target-chunk");
        assert_eq!(hits[0].chunk.text, "𠀀 𰀀");
    }
    for query in ["，。！？★", "上", ""] {
        assert!(ks::build_fts_literal_query(query).is_empty());
        assert!(index
            .search_fts(&[&target, &decoy], &ks::build_fts_literal_query(query), 10)
            .unwrap()
            .is_empty());
    }
    drop(index);
    std::fs::remove_dir_all(p).unwrap();
}
