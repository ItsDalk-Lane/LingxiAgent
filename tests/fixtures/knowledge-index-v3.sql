-- 从提交 350c0588 的真实 KnowledgeIndexStore v3 创建并通过公开写入接口填充。
-- 保留两来源、旧 v2 变体、原文定位及目录；FTS 由原始触发器建立。
BEGIN TRANSACTION;
CREATE TABLE chunk_index_variants (
    id TEXT PRIMARY KEY,
    parse_artifact_id TEXT NOT NULL,
    chunk_profile_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('building', 'ready', 'failed', 'retiring')),
    block_fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(parse_artifact_id, chunk_profile_hash)
  );
CREATE TABLE knowledge_chunks (
      row_id INTEGER PRIMARY KEY AUTOINCREMENT,
      id TEXT NOT NULL UNIQUE,
      parse_artifact_id TEXT NOT NULL,
      chunk_index_variant_id TEXT NOT NULL,
      ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
      text TEXT NOT NULL,
      token_count INTEGER NOT NULL CHECK(token_count > 0),
      search_text TEXT NOT NULL,
      spans_json TEXT NOT NULL,
      UNIQUE(chunk_index_variant_id, ordinal)
    );
CREATE INDEX idx_knowledge_chunks_artifact
    ON knowledge_chunks(parse_artifact_id, ordinal);
CREATE VIRTUAL TABLE knowledge_chunks_fts USING fts5(
    text,
    search_text,
    content=knowledge_chunks,
    content_rowid=row_id,
    tokenize='unicode61'
  );
CREATE TRIGGER knowledge_chunks_ai AFTER INSERT ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(rowid, text, search_text)
    VALUES (new.row_id, new.text, new.search_text);
  END;
CREATE TRIGGER knowledge_chunks_ad AFTER DELETE ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts, rowid, text, search_text)
    VALUES ('delete', old.row_id, old.text, old.search_text);
  END;
CREATE TRIGGER knowledge_chunks_au AFTER UPDATE ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts, rowid, text, search_text)
    VALUES ('delete', old.row_id, old.text, old.search_text);
    INSERT INTO knowledge_chunks_fts(rowid, text, search_text)
    VALUES (new.row_id, new.text, new.search_text);
  END;
CREATE TABLE chunk_index_variant_metadata (
    chunk_index_variant_id TEXT PRIMARY KEY,
    parse_artifact_id TEXT NOT NULL,
    chunk_count INTEGER NOT NULL,
    first_heading_path_json TEXT,
    section_keys_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );
CREATE INDEX idx_chunk_variant_metadata_artifact
    ON chunk_index_variant_metadata(parse_artifact_id);
INSERT INTO chunk_index_variants (id, parse_artifact_id, chunk_profile_hash, status, block_fingerprint, created_at, updated_at) VALUES ('civ_ebd018b15471db94784bc84ddf489f49', 'artifact-v2', '2222222222222222', 'ready', 'fingerprint-artifact-v2', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z');
INSERT INTO chunk_index_variants (id, parse_artifact_id, chunk_profile_hash, status, block_fingerprint, created_at, updated_at) VALUES ('civ_0bdbe3aedd66737cec395717f6ceab19', 'artifact-other', 'aaaaaaaaaaaaaaaa', 'ready', 'fingerprint-artifact-other', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z');
INSERT INTO knowledge_chunks (row_id, id, parse_artifact_id, chunk_index_variant_id, ordinal, text, token_count, search_text, spans_json) VALUES (1, 'chunk-artifact-v2', 'artifact-v2', 'civ_ebd018b15471db94784bc84ddf489f49', 0, 'AuroraQuokka 旧版章节数据。', 20, 'auroraquokka 旧版章节数据。 auroraquokka 旧版 章节 数据 版章 节数 旧版章 版章节 章节数 节数据', '[{"blockId":"block-artifact-v2","blockStartOffset":0,"blockEndOffset":20,"chunkStartOffset":0,"chunkEndOffset":20}]');
INSERT INTO knowledge_chunks (row_id, id, parse_artifact_id, chunk_index_variant_id, ordinal, text, token_count, search_text, spans_json) VALUES (2, 'chunk-artifact-other', 'artifact-other', 'civ_0bdbe3aedd66737cec395717f6ceab19', 0, '另一来源 PrivacyHeron 不应越界。', 20, '另一来源 privacyheron 不应越界。 privacyheron 来源 越界 另一 一来 另一来 一来源 不应 应越 不应越 应越界', '[{"blockId":"block-artifact-other","blockStartOffset":0,"blockEndOffset":23,"chunkStartOffset":0,"chunkEndOffset":23}]');
INSERT INTO chunk_index_variant_metadata (chunk_index_variant_id, parse_artifact_id, chunk_count, first_heading_path_json, section_keys_json, created_at, updated_at) VALUES ('civ_ebd018b15471db94784bc84ddf489f49', 'artifact-v2', 1, '["旧版章节"]', '["旧版章节"]', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z');
INSERT INTO chunk_index_variant_metadata (chunk_index_variant_id, parse_artifact_id, chunk_count, first_heading_path_json, section_keys_json, created_at, updated_at) VALUES ('civ_0bdbe3aedd66737cec395717f6ceab19', 'artifact-other', 1, '["旧版章节"]', '["旧版章节"]', '2026-09-04T00:00:00.000Z', '2026-09-04T00:00:00.000Z');
PRAGMA user_version = 3;
COMMIT;
