-- 真实 v17 知识库导出；禁止用新库降低 user_version 重新生成。
-- 来源提交：c3033b05e09877bf425b3fd0e5ea9cf9b065c8da
-- 来源文件：lib/knowledge/knowledge-store.ts，SHA256：d4fa36fc8983a1ba5db13cdc0e12dc8e26460ba0c4e3e61e719442719c7e14c5
-- 通过该版本真实 KnowledgeStore 建库及公开写入方法产生合成数据，然后导出 sqlite_master 与逐表原始行。
-- 使用固定时间与顺序标识；数据含双笔记本共享源、处理/解析产物、原文块、引用、待识别源、摄入进度和冻结范围。
PRAGMA foreign_keys = OFF;
BEGIN TRANSACTION;
CREATE TABLE notebooks (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        name TEXT NOT NULL CHECK(length(trim(name)) > 0),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        deleted_at TEXT
      , embedding_model_ref TEXT, rerank_model_ref TEXT, chunk_target_chars INTEGER DEFAULT 1200
        CHECK(chunk_target_chars IS NULL OR (chunk_target_chars >= 100 AND chunk_target_chars <= 100000)), retrieval_top_k INTEGER DEFAULT 12
        CHECK(retrieval_top_k IS NULL OR (retrieval_top_k >= 1 AND retrieval_top_k <= 1000)), retrieval_profile_id TEXT, vector_retention_days INTEGER);
CREATE TABLE sources (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        source_type TEXT NOT NULL CHECK(source_type IN ('file', 'pasted_text', 'web_snapshot')),
        display_name TEXT NOT NULL CHECK(length(trim(display_name)) > 0),
        origin_metadata_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        deleted_at TEXT
      , orphaned_at TEXT);
CREATE TABLE notebook_sources (
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        added_at TEXT NOT NULL,
        removed_at TEXT, relative_path TEXT, folder_node TEXT, display_order INTEGER,
        PRIMARY KEY(notebook_id, source_id),
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT
      );
CREATE TABLE content_snapshots (
        id TEXT PRIMARY KEY,
        source_id TEXT NOT NULL,
        sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
        mime_type TEXT NOT NULL,
        byte_size INTEGER NOT NULL CHECK(byte_size >= 0),
        storage_path TEXT NOT NULL UNIQUE,
        captured_at TEXT NOT NULL,
        UNIQUE(source_id, sha256),
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT
      );
CREATE TABLE parse_artifacts (
        id TEXT PRIMARY KEY,
        content_snapshot_id TEXT NOT NULL,
        parser_id TEXT NOT NULL,
        parser_version TEXT NOT NULL,
        parser_config_hash TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('parsing', 'ready', 'needs_ocr', 'failed')),
        warnings_json TEXT NOT NULL,
        semantic_artifact_path TEXT,
        created_at TEXT NOT NULL,
        completed_at TEXT, fidelity TEXT NOT NULL DEFAULT 'citation_grade', processing_artifact_id TEXT,
        UNIQUE(content_snapshot_id, parser_id, parser_version, parser_config_hash),
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT
      );
CREATE TABLE knowledge_blocks (
        id TEXT PRIMARY KEY,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        text TEXT NOT NULL,
        text_sha256 TEXT NOT NULL CHECK(length(text_sha256) = 64),
        locator_type TEXT NOT NULL CHECK(locator_type IN ('text', 'markdown', 'pdf', 'html')),
        locator_payload_json TEXT NOT NULL,
        UNIQUE(parse_artifact_id, ordinal),
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );
CREATE TABLE knowledge_citations (
        id TEXT PRIMARY KEY,
        parse_artifact_id TEXT NOT NULL,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        canonical_text TEXT NOT NULL,
        canonical_text_sha256 TEXT NOT NULL CHECK(length(canonical_text_sha256) = 64),
        created_at TEXT NOT NULL,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );
CREATE TABLE ingestion_jobs (
        id TEXT PRIMARY KEY,
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        artifact_id TEXT,
        phase TEXT NOT NULL CHECK(phase IN ('parse', 'chunk', 'fts_index', 'embed', 'done')),
        status TEXT NOT NULL CHECK(status IN ('queued', 'running', 'pending_embedding', 'failed', 'done')),
        attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),
        retry_after TEXT,
        error TEXT,
        chunker_config_id TEXT NOT NULL CHECK(length(chunker_config_id) = 16),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL, progress_done INTEGER NOT NULL DEFAULT 0, progress_total INTEGER, embedding_stats TEXT, cancelled_at TEXT,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );
CREATE TABLE chunk_profiles (
        id TEXT PRIMARY KEY,
        profile_hash TEXT NOT NULL UNIQUE CHECK(length(profile_hash) = 16),
        strategy TEXT CHECK(strategy IS NULL OR strategy IN ('fixed', 'markdown', 'text', 'pdf', 'html')),
        target_chars INTEGER CHECK(target_chars IS NULL OR (target_chars >= 100 AND target_chars <= 100000)),
        target_chars_source TEXT CHECK(target_chars_source IS NULL OR target_chars_source IN ('explicit', 'auto')),
        chunker_version TEXT,
        structural_options_json TEXT,
        profile_type TEXT NOT NULL CHECK(profile_type IN ('standard', 'legacy')),
        created_at TEXT NOT NULL
      );
CREATE TABLE retrieval_profiles (
        id TEXT PRIMARY KEY,
        profile_key TEXT NOT NULL UNIQUE CHECK(length(profile_key) = 16),
        chunk_profile_id TEXT NOT NULL,
        embedding_model_ref TEXT,
        rerank_model_ref TEXT,
        retrieval_top_k INTEGER CHECK(retrieval_top_k IS NULL OR (retrieval_top_k >= 1 AND retrieval_top_k <= 1000)),
        created_at TEXT NOT NULL,
        FOREIGN KEY(chunk_profile_id) REFERENCES chunk_profiles(id) ON DELETE RESTRICT
      );
CREATE TABLE knowledge_turn_scopes (
        id TEXT PRIMARY KEY,
        turn_id TEXT NOT NULL CHECK(length(trim(turn_id)) > 0),
        session_path TEXT NOT NULL CHECK(length(trim(session_path)) > 0),
        studio_id TEXT NOT NULL,
        notebook_ids_json TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('active', 'closed')),
        created_at TEXT NOT NULL
      );
CREATE TABLE knowledge_turn_scope_sources (
        scope_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT,
        notebook_ids_json TEXT NOT NULL,
        PRIMARY KEY(scope_id, source_id),
        FOREIGN KEY(scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );
CREATE TABLE knowledge_coverage_plans (
        id TEXT PRIMARY KEY,
        turn_scope_id TEXT,
        question TEXT NOT NULL CHECK(length(trim(question)) > 0),
        intent TEXT NOT NULL CHECK(intent IN ('fact_lookup', 'cross_source_synthesis', 'whole_scope_analysis', 'global_negative', 'open_summary')),
        coverage_mode TEXT NOT NULL CHECK(coverage_mode IN ('high_recall', 'broad', 'exhaustive')),
        requires_completeness INTEGER NOT NULL CHECK(requires_completeness IN (0, 1)),
        scope_level TEXT NOT NULL CHECK(scope_level IN ('local', 'source', 'multi_source', 'notebook', 'multi_notebook', 'whole_scope')),
        sub_queries_json TEXT NOT NULL,
        confidence REAL NOT NULL CHECK(confidence >= 0 AND confidence <= 1),
        matched_rule_ids_json TEXT NOT NULL,
        classifier_used TEXT NOT NULL CHECK(classifier_used IN ('rules', 'llm', 'rules+llm')),
        degrade_reason TEXT,
        created_at TEXT NOT NULL,
        FOREIGN KEY(turn_scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT
      );
CREATE TABLE coverage_runs (
        id TEXT PRIMARY KEY,
        turn_scope_id TEXT NOT NULL,
        manifest_hash TEXT NOT NULL CHECK(length(manifest_hash) = 64),
        manifest_json TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'complete', 'partial', 'cancelled', 'failed')),
        expected_units INTEGER NOT NULL CHECK(expected_units >= 0),
        processed_units INTEGER NOT NULL DEFAULT 0 CHECK(processed_units >= 0),
        failed_units INTEGER NOT NULL DEFAULT 0 CHECK(failed_units >= 0),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(turn_scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT
      );
CREATE TABLE coverage_shards (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        unit_ids_json TEXT NOT NULL,
        context_before_ids_json TEXT NOT NULL,
        context_after_ids_json TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'cancelled')),
        attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count >= 0),
        result_json TEXT,
        updated_at TEXT NOT NULL,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES coverage_runs(id) ON DELETE RESTRICT
      );
CREATE TABLE evidence_manifests (
        id TEXT PRIMARY KEY,
        turn_scope_id TEXT NOT NULL,
        session_path TEXT NOT NULL CHECK(length(trim(session_path)) > 0),
        turn_id TEXT NOT NULL CHECK(length(trim(turn_id)) > 0),
        coverage_mode TEXT CHECK(coverage_mode IS NULL OR coverage_mode IN ('high_recall', 'broad', 'exhaustive')),
        executed_coverage_mode TEXT CHECK(executed_coverage_mode IS NULL OR executed_coverage_mode IN ('high_recall', 'broad', 'exhaustive')),
        notebook_ids_json TEXT NOT NULL,
        coverage_run_id TEXT,
        coverage_manifest_hash TEXT CHECK(coverage_manifest_hash IS NULL OR length(coverage_manifest_hash) = 64),
        created_at TEXT NOT NULL,
        FOREIGN KEY(turn_scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT,
        FOREIGN KEY(coverage_run_id) REFERENCES coverage_runs(id) ON DELETE RESTRICT
      );
CREATE TABLE evidence_manifest_entries (
        manifest_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        source_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT,
        chunk_profile_hash TEXT,
        chunk_index_variant_id TEXT,
        vector_index_variant_ids_json TEXT NOT NULL,
        chunk_ids_json TEXT NOT NULL,
        neighbor_chunk_ids_json TEXT NOT NULL,
        block_spans_json TEXT NOT NULL,
        citation_labels_json TEXT NOT NULL,
        PRIMARY KEY(manifest_id, ordinal, source_id),
        FOREIGN KEY(manifest_id) REFERENCES evidence_manifests(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );
CREATE TABLE processing_artifacts (
        id TEXT PRIMARY KEY,
        content_snapshot_id TEXT NOT NULL,
        processor_id TEXT NOT NULL,
        processor_version TEXT NOT NULL,
        processor_config_hash TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('processing', 'ready', 'failed')),
        fidelity TEXT CHECK(fidelity IN ('citation_grade', 'structural', 'semantic_only')),
        output_mime TEXT,
        output_path TEXT,
        locator_map_json TEXT NOT NULL DEFAULT '{}',
        warnings_json TEXT NOT NULL DEFAULT '[]',
        created_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(content_snapshot_id, processor_id, processor_version, processor_config_hash),
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT
      );
INSERT INTO "notebooks" ("id", "studio_id", "name", "created_at", "updated_at", "deleted_at", "embedding_model_ref", "rerank_model_ref", "chunk_target_chars", "retrieval_top_k", "retrieval_profile_id", "vector_retention_days") VALUES ('nb_v17_001', 'fixture-v17-studio', '迁移前的甲资料', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z', NULL, '{"id":"fixture-embedding","provider":"fixture-provider"}', '{"id":"fixture-rerank","provider":"fixture-provider"}', 512, 24, NULL, 90);
INSERT INTO "notebooks" ("id", "studio_id", "name", "created_at", "updated_at", "deleted_at", "embedding_model_ref", "rerank_model_ref", "chunk_target_chars", "retrieval_top_k", "retrieval_profile_id", "vector_retention_days") VALUES ('nb_v17_002', 'fixture-v17-studio', '迁移前的乙资料', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z', NULL, NULL, NULL, NULL, NULL, NULL, NULL);
INSERT INTO "sources" ("id", "studio_id", "source_type", "display_name", "origin_metadata_json", "created_at", "deleted_at", "orphaned_at") VALUES ('src_v17_ready', 'fixture-v17-studio', 'file', '历史事实.txt', '{"fileName":"历史事实.txt","marker":"O''Reilly"}', '2026-09-04T00:00:00.000Z', NULL, NULL);
INSERT INTO "sources" ("id", "studio_id", "source_type", "display_name", "origin_metadata_json", "created_at", "deleted_at", "orphaned_at") VALUES ('src_v17_ocr', 'fixture-v17-studio', 'file', '待识别扫描.pdf', '{"fileName":"待识别扫描.pdf"}', '2026-09-04T00:00:00.000Z', NULL, NULL);
INSERT INTO "notebook_sources" ("notebook_id", "source_id", "added_at", "removed_at", "relative_path", "folder_node", "display_order") VALUES ('nb_v17_001', 'src_v17_ready', '2026-09-04T00:00:00.000Z', NULL, '历史/事实.txt', '历史', 2);
INSERT INTO "notebook_sources" ("notebook_id", "source_id", "added_at", "removed_at", "relative_path", "folder_node", "display_order") VALUES ('nb_v17_002', 'src_v17_ready', '2026-09-04T00:00:00.000Z', NULL, NULL, NULL, NULL);
INSERT INTO "notebook_sources" ("notebook_id", "source_id", "added_at", "removed_at", "relative_path", "folder_node", "display_order") VALUES ('nb_v17_002', 'src_v17_ocr', '2026-09-04T00:00:00.000Z', NULL, NULL, NULL, NULL);
INSERT INTO "content_snapshots" ("id", "source_id", "sha256", "mime_type", "byte_size", "storage_path", "captured_at") VALUES ('snap_v17_ready', 'src_v17_ready', '0ebfe32df314a6b1b34bcfe65221ddf14b6bcc8eb2cce1e4e2ee4864681d4012', 'text/plain', 96, 'sources/src_v17_ready/snap_v17_ready.txt', '2026-09-04T00:00:00.000Z');
INSERT INTO "content_snapshots" ("id", "source_id", "sha256", "mime_type", "byte_size", "storage_path", "captured_at") VALUES ('snap_v17_ocr', 'src_v17_ocr', '1759310ab11053297113c688c781bd6829682f06d4e0b6d32e4be1c83d8ff5db', 'application/pdf', 128, 'sources/src_v17_ocr/snap_v17_ocr.pdf', '2026-09-04T00:00:00.000Z');
INSERT INTO "parse_artifacts" ("id", "content_snapshot_id", "parser_id", "parser_version", "parser_config_hash", "status", "warnings_json", "semantic_artifact_path", "created_at", "completed_at", "fidelity", "processing_artifact_id") VALUES ('parse_v17_ready', 'snap_v17_ready', 'plain-text', '1', '38ed5169efed49e03cef0565ee7df2aaf5725b28ba70dd3535d9ac6c4e2bcf01', 'ready', '["合成迁移样本"]', 'artifacts/parse_v17_ready.json', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z', 'citation_grade', 'proc_v17_ready');
INSERT INTO "parse_artifacts" ("id", "content_snapshot_id", "parser_id", "parser_version", "parser_config_hash", "status", "warnings_json", "semantic_artifact_path", "created_at", "completed_at", "fidelity", "processing_artifact_id") VALUES ('parse_v17_ocr', 'snap_v17_ocr', 'pdf', '1', 'f563ae779c2a8420888718a0fda0fdf4e79c03b6da6e65f1c8198dc6bed2b122', 'needs_ocr', '["扫描页没有可提取文本"]', 'artifacts/parse_v17_ocr.json', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z', 'citation_grade', NULL);
INSERT INTO "knowledge_blocks" ("id", "parse_artifact_id", "ordinal", "text", "text_sha256", "locator_type", "locator_payload_json") VALUES ('block_v17_003', 'parse_v17_ready', 0, '迁移前资料：项目 O''Reilly 在九月完成。', '3e76e529e4cf4b1f77d3bd88f4a87167bb3919669dd4f1dd5bf2b02b564911e3', 'text', '{"line":1,"headingPath":["历史事实"]}');
INSERT INTO "knowledge_blocks" ("id", "parse_artifact_id", "ordinal", "text", "text_sha256", "locator_type", "locator_payload_json") VALUES ('block_v17_004', 'parse_v17_ready', 1, '另一条事实：预算保持 32 万元。', 'aa9a96c388da810b26664de382b7bcf57e8de1d17b1f1bd13560171bb7380e1a', 'text', '{"line":2,"headingPath":["历史事实"]}');
INSERT INTO "knowledge_citations" ("id", "parse_artifact_id", "block_id", "start_offset", "end_offset", "canonical_text", "canonical_text_sha256", "created_at") VALUES ('cite_v17_005', 'parse_v17_ready', 'block_v17_003', 0, 24, '迁移前资料：项目 O''Reilly 在九月完成。', '3e76e529e4cf4b1f77d3bd88f4a87167bb3919669dd4f1dd5bf2b02b564911e3', '2026-09-04T00:00:00.000Z');
INSERT INTO "ingestion_jobs" ("id", "notebook_id", "source_id", "artifact_id", "phase", "status", "attempt", "retry_after", "error", "chunker_config_id", "created_at", "updated_at", "progress_done", "progress_total", "embedding_stats", "cancelled_at") VALUES ('ingjob_v17_006', 'nb_v17_001', 'src_v17_ready', 'parse_v17_ready', 'parse', 'running', 0, NULL, NULL, '59c16fec7f0e65e8', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z', 1, 2, NULL, NULL);
INSERT INTO "knowledge_turn_scopes" ("id", "turn_id", "session_path", "studio_id", "notebook_ids_json", "status", "created_at") VALUES ('kts_v17_007', 'turn-v17', '/synthetic/sessions/v17.jsonl', 'fixture-v17-studio', '["nb_v17_001","nb_v17_002"]', 'active', '2026-09-04T00:00:00.000Z');
INSERT INTO "knowledge_turn_scope_sources" ("scope_id", "source_id", "content_snapshot_id", "parse_artifact_id", "notebook_ids_json") VALUES ('kts_v17_007', 'src_v17_ready', 'snap_v17_ready', 'parse_v17_ready', '["nb_v17_001","nb_v17_002"]');
INSERT INTO "knowledge_turn_scope_sources" ("scope_id", "source_id", "content_snapshot_id", "parse_artifact_id", "notebook_ids_json") VALUES ('kts_v17_007', 'src_v17_ocr', 'snap_v17_ocr', 'parse_v17_ocr', '["nb_v17_002"]');
INSERT INTO "processing_artifacts" ("id", "content_snapshot_id", "processor_id", "processor_version", "processor_config_hash", "status", "fidelity", "output_mime", "output_path", "locator_map_json", "warnings_json", "created_at", "completed_at") VALUES ('proc_v17_ready', 'snap_v17_ready', 'fixture-text', '1', '655b0e388a682c5c2f210d23c9048bcbf6547593dcf25717937cac3d2f956413', 'ready', 'citation_grade', 'text/plain', 'processed/snap_v17_ready/proc_v17_ready.txt', '{"paragraph":1}', '[]', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z');
CREATE INDEX idx_notebooks_studio_active
        ON notebooks(studio_id, deleted_at, updated_at DESC);
CREATE INDEX idx_sources_studio_active
        ON sources(studio_id, deleted_at, created_at DESC);
CREATE INDEX idx_notebook_sources_active
        ON notebook_sources(notebook_id, removed_at, added_at);
CREATE INDEX idx_source_notebooks_active
        ON notebook_sources(source_id, removed_at, added_at);
CREATE INDEX idx_content_snapshots_source
        ON content_snapshots(source_id, captured_at DESC);
CREATE INDEX idx_parse_artifacts_snapshot
        ON parse_artifacts(content_snapshot_id, created_at DESC);
CREATE INDEX idx_parse_artifacts_status
        ON parse_artifacts(status, created_at);
CREATE INDEX idx_knowledge_blocks_artifact
        ON knowledge_blocks(parse_artifact_id, ordinal);
CREATE INDEX idx_knowledge_citations_artifact
        ON knowledge_citations(parse_artifact_id, created_at);
CREATE INDEX idx_knowledge_citations_block
        ON knowledge_citations(block_id, start_offset, end_offset);
CREATE INDEX idx_ingestion_jobs_status
        ON ingestion_jobs(status, retry_after, created_at);
CREATE INDEX idx_ingestion_jobs_source
        ON ingestion_jobs(source_id, created_at DESC);
CREATE INDEX idx_ingestion_jobs_notebook
        ON ingestion_jobs(notebook_id, created_at DESC);
CREATE INDEX idx_knowledge_turn_scopes_session
        ON knowledge_turn_scopes(session_path, status, created_at DESC);
CREATE INDEX idx_sources_orphaned
        ON sources(orphaned_at) WHERE orphaned_at IS NOT NULL;
CREATE UNIQUE INDEX idx_ingestion_jobs_active
        ON ingestion_jobs(notebook_id, source_id)
        WHERE status IN ('queued', 'running', 'pending_embedding');
CREATE INDEX idx_knowledge_coverage_plans_scope
        ON knowledge_coverage_plans(turn_scope_id, created_at DESC);
CREATE INDEX idx_knowledge_coverage_plans_created
        ON knowledge_coverage_plans(created_at DESC);
CREATE INDEX idx_coverage_runs_manifest
        ON coverage_runs(manifest_hash, created_at DESC);
CREATE INDEX idx_coverage_runs_status
        ON coverage_runs(status, updated_at);
CREATE INDEX idx_coverage_shards_run_status
        ON coverage_shards(run_id, status, ordinal);
CREATE INDEX idx_evidence_manifests_scope
        ON evidence_manifests(turn_scope_id, created_at DESC);
CREATE INDEX idx_evidence_manifests_turn
        ON evidence_manifests(turn_id, created_at DESC);
CREATE INDEX idx_evidence_manifests_run
        ON evidence_manifests(coverage_run_id) WHERE coverage_run_id IS NOT NULL;
CREATE INDEX idx_evidence_manifest_entries_source
        ON evidence_manifest_entries(source_id);
CREATE INDEX idx_processing_artifacts_snapshot
        ON processing_artifacts(content_snapshot_id, created_at DESC);
PRAGMA user_version = 17;
COMMIT;
PRAGMA foreign_keys = ON;
