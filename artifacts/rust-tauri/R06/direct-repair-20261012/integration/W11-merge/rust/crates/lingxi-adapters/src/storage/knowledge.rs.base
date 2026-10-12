//! R06-T07 知识库存储：三库布局 + 快照/解析产物文件存储。
//!
//! 现役三文件布局（knowledge.db / knowledge-index.db / knowledge-vector.db，
//! `lib/knowledge/knowledge-store.ts`、`knowledge-index-store.ts`、
//! `vector-index-adapter.ts`）的 Rust 同构：
//! - `knowledge.db`（事实库）：notebooks / sources / notebook_sources /
//!   content_snapshots / parse_artifacts / knowledge_blocks /
//!   knowledge_citations / ingestion_jobs / knowledge_turn_scopes(+sources)。
//! - `knowledge_index.db`（FTS 缓存）：chunk_index_variants + knowledge_chunks
//!   (+FTS5) + sections。可重建缓存：打不开/迁移失败 → 只重建该文件。
//! - `knowledge_vector.db`（向量缓存）：vector_index_variants + chunk_vectors
//!   （f32 LE BLOB，维度铁律）。可重建缓存：同上。
//! - 快照字节 `knowledge/snapshots/<id>`；解析产物 JSON
//!   `knowledge/artifacts/<snapshotId>/<artifactId>.json`。
//!   索引可重建 ≠ 源文件可删。
//!
//! 退役边界：不建 research_runs / analysis_* / coverage_* 等已退役研究引擎表
//! （兼容残留叶 F-D08-C-RESEARCH-LEDGER）；runs.db v1–v9 冻结不动。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use lingxi_kernel::knowledge::KnowledgeError;
use rusqlite::{params, Connection, OptionalExtension};

pub use lingxi_kernel::knowledge::{
    build_chunks, chunk_index_variant_id, chunker_config_id, embedding_model_key,
    estimate_text_tokens, sha256_hex, vector_index_variant_id, ChunkDraft, ChunkSpan, ChunkerBlock,
};

// ── 行类型 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct NotebookRow {
    pub id: String,
    pub studio_id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceRow {
    pub id: String,
    pub studio_id: String,
    pub source_type: String,
    pub display_name: String,
    pub origin_metadata: serde_json::Value,
    pub created_at: String,
    pub deleted_at: Option<String>,
    pub orphaned_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MembershipRow {
    pub notebook_id: String,
    pub source_id: String,
    pub added_at: String,
    pub removed_at: Option<String>,
    pub relative_path: Option<String>,
    pub folder_node: Option<String>,
    pub display_order: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotRow {
    pub id: String,
    pub source_id: String,
    pub sha256: String,
    pub mime_type: String,
    pub byte_size: i64,
    pub storage_path: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseArtifactRow {
    pub id: String,
    pub content_snapshot_id: String,
    pub parser_id: String,
    pub parser_version: String,
    pub parser_config_hash: String,
    pub status: String,
    pub warnings: Vec<String>,
    pub fidelity: String,
    pub processing_artifact_id: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockRow {
    pub id: String,
    pub parse_artifact_id: String,
    pub ordinal: usize,
    pub text: String,
    pub text_sha256: String,
    pub locator_type: String,
    pub locator: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CitationRow {
    pub id: String,
    pub parse_artifact_id: String,
    pub block_id: String,
    pub start_offset: usize,
    pub end_offset: usize,
    pub canonical_text: String,
    pub canonical_text_sha256: String,
    pub created_at: String,
}

pub struct ResolvedCitation {
    pub citation: CitationRow,
    pub block: BlockRow,
    pub snapshot: SnapshotRow,
    pub source: SourceRow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: String,
    pub notebook_id: String,
    pub source_id: String,
    pub artifact_id: Option<String>,
    pub phase: String,
    pub status: String,
    pub attempt: i64,
    pub error: Option<String>,
    pub chunker_config_id: String,
    pub cancelled_at: Option<String>,
    pub progress_done: i64,
    pub progress_total: Option<i64>,
    pub embedding_stats: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelRef {
    pub id: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NotebookConfigRow {
    pub embedding_model_ref: Option<ModelRef>,
    pub rerank_model_ref: Option<ModelRef>,
    pub chunk_target_chars: Option<i64>,
    pub retrieval_top_k: Option<i64>,
    pub vector_retention_days: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnScopeSourceRow {
    pub scope_id: String,
    pub source_id: String,
    pub content_snapshot_id: String,
    pub parse_artifact_id: Option<String>,
    pub notebook_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnScopeRow {
    pub id: String,
    pub turn_id: String,
    pub session_id: String,
    pub studio_id: String,
    pub notebook_ids: Vec<String>,
    pub status: String,
    pub created_at: String,
    pub sources: Vec<TurnScopeSourceRow>,
}

pub struct NotebookSourceEntry {
    pub source: SourceRow,
    pub snapshot: SnapshotRow,
    pub membership: MembershipRow,
    pub parse_artifact: Option<ParseArtifactRow>,
}

// ── 工具 ────────────────────────────────────────────────────────────────────

/// 不透明 id：`prefix_` + 12 随机字节 hex（TS newId 的等价承担——身份是不透明
/// 字符串，具体形状不进任何契约；差异台账 D 项申报 uuid→CSPRNG hex）。
pub fn new_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 12];
    getrandom::getrandom(&mut bytes).expect("csprng id bytes");
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{prefix}_{hex}")
}

/// ISO 8601 zulu 毫秒时间戳（TS new Date().toISOString() 同形）。
fn now_iso(clock_ms: impl Fn() -> i64) -> String {
    let ms = clock_ms();
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// ISO 8601 zulu 格式的毫秒时钟（服务注入；测试可用固定闭包）。
pub type ClockMs = Arc<dyn Fn() -> i64 + Send + Sync>;

/// days-since-epoch → (y, m, d)（Howard Hinnant civil_from_days 算法）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn parse_json(raw: Option<String>) -> serde_json::Value {
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn open_connection(path: &Path) -> Result<Connection, KnowledgeError> {
    let conn = Connection::open(path)
        .map_err(|e| KnowledgeError::storage_invalid(format!("cannot open knowledge db: {e}")))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
    Ok(conn)
}

fn check_user_version(conn: &Connection, supported: i64) -> Result<(), KnowledgeError> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
    if current > supported {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_SCHEMA_NEWER",
            format!("knowledge schema version {current} is newer than supported {supported}"),
        ));
    }
    Ok(())
}

fn lock_conn(conn: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    conn.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub const MIN_CHUNK: i64 = 100;
pub const MAX_CHUNK: i64 = 100_000;

/// ── 检索分词（lib/search/search-text.ts 的 Rust 承担）──────────────────────
///
/// FTS 写入侧派生文本 = 归一化原文 + ASCII 词 + CJK 2/3-gram；查询侧 = 同法
/// 分词后 OR 连接的引号字面量。与 TS 的差异：jieba 词级分词缺席（词典依赖
/// 不引入），CJK 召回由 n-gram 层承担——差异台账 D 项申报；NFKC 归一化缺席
/// 同条申报。
const CJK_NGRAM_SIZES: [usize; 2] = [2, 3];

fn is_cjk_char(ch: char) -> bool {
    // Han / Hiragana / Katakana / Hangul（search-text.ts CJK_RUN_RE 口径）。
    matches!(ch,
        '\u{4e00}'..='\u{9fff}'
        | '\u{3400}'..='\u{4dbf}'
        | '\u{3040}'..='\u{309f}'
        | '\u{30a0}'..='\u{30ff}'
        | '\u{ac00}'..='\u{d7af}')
}

fn normalize_search_text(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 写入 FTS 的派生文本（buildSearchDocumentText 同形）。
pub fn build_search_document_text(value: &str) -> String {
    let normalized = normalize_search_text(value);
    if normalized.is_empty() {
        return String::new();
    }
    let mut parts = vec![normalized.clone()];
    parts.extend(tokenize_search_text(&normalized));
    parts.join(" ")
}

/// 查询分词（tokenizeSearchText 的 ASCII + CJK n-gram 层；jieba 缺席，D 项）。
pub fn tokenize_search_text(value: &str) -> Vec<String> {
    let normalized = normalize_search_text(value);
    if normalized.is_empty() {
        return Vec::new();
    }
    let mut tokens: Vec<String> = Vec::new();
    let mut push = |token: &str| {
        let token = token.trim();
        let punctuation_only = !token.is_empty()
            && token
                .chars()
                .all(|ch| ch.is_whitespace() || ch.is_ascii_punctuation());
        if token.is_empty() || punctuation_only {
            return;
        }
        if token.chars().count() == 1 && token.chars().all(is_cjk_char) {
            return;
        }
        if !tokens.iter().any(|existing| existing == token) {
            tokens.push(token.to_string());
        }
    };
    let mut ascii_word = String::new();
    let mut cjk_run: Vec<char> = Vec::new();
    let flush_ascii = |ascii_word: &mut String, push: &mut dyn FnMut(&str)| {
        if !ascii_word.is_empty() {
            push(ascii_word);
            ascii_word.clear();
        }
    };
    for ch in normalized.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-' {
            flush_ascii(&mut ascii_word, &mut push);
            cjk_run.clear();
            ascii_word.push(ch);
        } else if is_cjk_char(ch) {
            flush_ascii(&mut ascii_word, &mut push);
            cjk_run.push(ch);
        } else {
            flush_ascii(&mut ascii_word, &mut push);
            cjk_run.clear();
        }
        // CJK run 的 n-gram 在收尾统一产出（此处只累积）。
        if cjk_run.len() >= CJK_NGRAM_SIZES.iter().copied().max().unwrap_or(3) {
            for size in CJK_NGRAM_SIZES {
                if cjk_run.len() >= size {
                    let start = cjk_run.len() - size;
                    let gram: String = cjk_run[start..].iter().collect();
                    push(&gram);
                }
            }
        }
    }
    flush_ascii(&mut ascii_word, &mut push);
    tokens
}

/// FTS5 字面量查询（buildFtsLiteralQuery 同形：引号转义 + OR 连接）。
pub fn build_fts_literal_query(value: &str) -> String {
    tokenize_search_text(value)
        .into_iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// ChunkSpan 的 JSON 序列化（kernel 类型无 serde derive——adapters 侧本地转换，
/// 形状与 TS KnowledgeChunkSpanDraft 一致：blockId/blockStartOffset/blockEndOffset/
/// chunkStartOffset/chunkEndOffset）。
fn spans_to_json(spans: &[ChunkSpan]) -> String {
    let entries: Vec<serde_json::Value> = spans
        .iter()
        .map(|span| {
            serde_json::json!({
                "blockId": span.block_id,
                "blockStartOffset": span.block_start_offset,
                "blockEndOffset": span.block_end_offset,
                "chunkStartOffset": span.chunk_start_offset,
                "chunkEndOffset": span.chunk_end_offset,
            })
        })
        .collect();
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into())
}

fn spans_from_json(raw: &str) -> Vec<ChunkSpan> {
    serde_json::from_str::<Vec<serde_json::Value>>(raw)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| ChunkSpan {
            block_id: entry["blockId"].as_str().unwrap_or_default().to_string(),
            block_start_offset: entry["blockStartOffset"].as_u64().unwrap_or(0) as usize,
            block_end_offset: entry["blockEndOffset"].as_u64().unwrap_or(0) as usize,
            chunk_start_offset: entry["chunkStartOffset"].as_u64().unwrap_or(0) as usize,
            chunk_end_offset: entry["chunkEndOffset"].as_u64().unwrap_or(0) as usize,
        })
        .collect()
}

// ── 事实库 ──────────────────────────────────────────────────────────────────

pub struct KnowledgeFactStore {
    conn: Mutex<Connection>,
    root: PathBuf,
    clock: ClockMs,
}

const FACTS_SCHEMA_VERSION: i64 = 1;

const FACTS_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS notebooks (
    id         TEXT PRIMARY KEY,
    studio_id  TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
);
CREATE TABLE IF NOT EXISTS notebook_config (
    notebook_id          TEXT PRIMARY KEY REFERENCES notebooks(id),
    embedding_model_json TEXT,
    rerank_model_json    TEXT,
    chunk_target_chars   INTEGER,
    retrieval_top_k      INTEGER,
    vector_retention_days INTEGER,
    updated_at           TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sources (
    id              TEXT PRIMARY KEY,
    studio_id       TEXT NOT NULL,
    source_type     TEXT NOT NULL CHECK(source_type IN ('file','pasted_text','web_snapshot')),
    display_name    TEXT NOT NULL,
    origin_metadata TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL,
    deleted_at      TEXT,
    orphaned_at     TEXT
);
CREATE TABLE IF NOT EXISTS notebook_sources (
    notebook_id   TEXT NOT NULL REFERENCES notebooks(id),
    source_id     TEXT NOT NULL REFERENCES sources(id),
    added_at      TEXT NOT NULL,
    removed_at    TEXT,
    relative_path TEXT,
    folder_node   TEXT,
    display_order INTEGER,
    PRIMARY KEY (notebook_id, source_id)
);
CREATE TABLE IF NOT EXISTS content_snapshots (
    id           TEXT PRIMARY KEY,
    source_id    TEXT NOT NULL REFERENCES sources(id),
    sha256       TEXT NOT NULL,
    mime_type    TEXT NOT NULL,
    byte_size    INTEGER NOT NULL,
    storage_path TEXT NOT NULL,
    captured_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_snapshots_source ON content_snapshots(source_id, captured_at);
CREATE TABLE IF NOT EXISTS parse_artifacts (
    id                     TEXT PRIMARY KEY,
    content_snapshot_id    TEXT NOT NULL REFERENCES content_snapshots(id),
    parser_id              TEXT NOT NULL,
    parser_version         TEXT NOT NULL,
    parser_config_hash     TEXT NOT NULL,
    status                 TEXT NOT NULL CHECK(status IN ('parsing','ready','needs_ocr','failed')),
    warnings               TEXT NOT NULL DEFAULT '[]',
    fidelity               TEXT NOT NULL DEFAULT 'citation_grade',
    processing_artifact_id TEXT,
    semantic_artifact_path TEXT,
    created_at             TEXT NOT NULL,
    completed_at           TEXT
);
CREATE INDEX IF NOT EXISTS idx_artifacts_snapshot ON parse_artifacts(content_snapshot_id);
CREATE TABLE IF NOT EXISTS knowledge_blocks (
    id                TEXT PRIMARY KEY,
    parse_artifact_id TEXT NOT NULL REFERENCES parse_artifacts(id),
    ordinal           INTEGER NOT NULL,
    text              TEXT NOT NULL,
    text_sha256       TEXT NOT NULL,
    locator_type      TEXT NOT NULL,
    locator           TEXT NOT NULL DEFAULT '{}',
    UNIQUE (parse_artifact_id, ordinal)
);
CREATE TABLE IF NOT EXISTS knowledge_citations (
    id                    TEXT PRIMARY KEY,
    parse_artifact_id     TEXT NOT NULL REFERENCES parse_artifacts(id),
    block_id              TEXT NOT NULL REFERENCES knowledge_blocks(id),
    start_offset          INTEGER NOT NULL,
    end_offset            INTEGER NOT NULL,
    canonical_text        TEXT NOT NULL,
    canonical_text_sha256 TEXT NOT NULL,
    created_at            TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ingestion_jobs (
    id                TEXT PRIMARY KEY,
    notebook_id       TEXT NOT NULL,
    source_id         TEXT NOT NULL,
    artifact_id       TEXT,
    phase             TEXT NOT NULL CHECK(phase IN ('parse','chunk','fts_index','embed','done')),
    status            TEXT NOT NULL CHECK(status IN ('queued','running','pending_embedding','failed','done')),
    attempt           INTEGER NOT NULL DEFAULT 0,
    retry_after       TEXT,
    error             TEXT,
    chunker_config_id TEXT NOT NULL,
    cancelled_at      TEXT,
    progress_done     INTEGER NOT NULL DEFAULT 0,
    progress_total    INTEGER,
    embedding_stats   TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_jobs_source ON ingestion_jobs(source_id, created_at);
CREATE INDEX IF NOT EXISTS idx_jobs_notebook ON ingestion_jobs(notebook_id, created_at);
CREATE TABLE IF NOT EXISTS knowledge_turn_scopes (
    id                TEXT PRIMARY KEY,
    turn_id           TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    studio_id         TEXT NOT NULL,
    notebook_ids_json TEXT NOT NULL,
    status            TEXT NOT NULL CHECK(status IN ('active','closed')),
    created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_scopes_session ON knowledge_turn_scopes(session_id, status);
CREATE TABLE IF NOT EXISTS knowledge_turn_scope_sources (
    scope_id            TEXT NOT NULL REFERENCES knowledge_turn_scopes(id),
    source_id           TEXT NOT NULL,
    content_snapshot_id TEXT NOT NULL,
    parse_artifact_id   TEXT,
    notebook_ids_json   TEXT NOT NULL,
    PRIMARY KEY (scope_id, source_id)
);
"#;

impl KnowledgeFactStore {
    /// Opens (creating if needed) `knowledge.db` under `root` (schema v1).
    pub fn open(root: &Path) -> Result<Self, KnowledgeError> {
        Self::open_with_clock(
            root,
            Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
            }),
        )
    }

    pub fn open_with_clock(root: &Path, clock: ClockMs) -> Result<Self, KnowledgeError> {
        std::fs::create_dir_all(root)
            .map_err(|e| KnowledgeError::storage_invalid(format!("knowledge root: {e}")))?;
        let conn = open_connection(&root.join("knowledge.db"))?;
        check_user_version(&conn, FACTS_SCHEMA_VERSION)?;
        conn.execute_batch(FACTS_SCHEMA_SQL)
            .map_err(|e| KnowledgeError::storage_invalid(format!("facts schema: {e}")))?;
        conn.pragma_update(None, "user_version", FACTS_SCHEMA_VERSION)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
            root: root.to_path_buf(),
            clock,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn has_table(&self, table: &str) -> bool {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
            params![table],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false)
    }

    // ── notebooks ───────────────────────────────────────────────────────────

    pub fn create_notebook(
        &self,
        studio_id: &str,
        name: &str,
    ) -> Result<NotebookRow, KnowledgeError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 256 {
            return Err(KnowledgeError::invalid_argument(
                "Notebook name must be 1..=256 characters",
            ));
        }
        let id = new_id("knb");
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "INSERT INTO notebooks (id, studio_id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, studio_id, name, now],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_notebook(studio_id, &id)
    }

    pub fn list_notebooks(&self, studio_id: &str) -> Result<Vec<NotebookRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, studio_id, name, created_at, updated_at, deleted_at FROM notebooks \
                 WHERE studio_id=?1 AND deleted_at IS NULL ORDER BY created_at, id",
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let rows = stmt
            .query_map(params![studio_id], Self::map_notebook)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    pub fn get_notebook(
        &self,
        studio_id: &str,
        notebook_id: &str,
    ) -> Result<NotebookRow, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT id, studio_id, name, created_at, updated_at, deleted_at FROM notebooks \
             WHERE id=?1 AND studio_id=?2 AND deleted_at IS NULL",
            params![notebook_id, studio_id],
            Self::map_notebook,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
        .ok_or_else(|| KnowledgeError::not_found("Knowledge notebook was not found"))
    }

    fn map_notebook(row: &rusqlite::Row<'_>) -> rusqlite::Result<NotebookRow> {
        Ok(NotebookRow {
            id: row.get(0)?,
            studio_id: row.get(1)?,
            name: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            deleted_at: row.get(5)?,
        })
    }

    pub fn rename_notebook(
        &self,
        studio_id: &str,
        notebook_id: &str,
        name: &str,
    ) -> Result<NotebookRow, KnowledgeError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 256 {
            return Err(KnowledgeError::invalid_argument(
                "Notebook name must be 1..=256 characters",
            ));
        }
        self.get_notebook(studio_id, notebook_id)?;
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE notebooks SET name=?1, updated_at=?2 WHERE id=?3",
            params![name, now, notebook_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_notebook(studio_id, notebook_id)
    }

    pub fn delete_notebook(
        &self,
        studio_id: &str,
        notebook_id: &str,
    ) -> Result<NotebookRow, KnowledgeError> {
        self.get_notebook(studio_id, notebook_id)?;
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE notebooks SET deleted_at=?1, updated_at=?1 WHERE id=?2",
            params![now, notebook_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        conn.execute(
            "UPDATE notebook_sources SET removed_at=?1 WHERE notebook_id=?2 AND removed_at IS NULL",
            params![now, notebook_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        conn.query_row(
            "SELECT id, studio_id, name, created_at, updated_at, deleted_at FROM notebooks WHERE id=?1",
            params![notebook_id],
            Self::map_notebook,
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    // ── notebook config ─────────────────────────────────────────────────────

    pub fn get_notebook_config(
        &self,
        notebook_id: &str,
    ) -> Result<NotebookConfigRow, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let row = conn
            .query_row(
                "SELECT embedding_model_json, rerank_model_json, chunk_target_chars, retrieval_top_k, vector_retention_days FROM notebook_config WHERE notebook_id=?1",
                params![notebook_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let map_ref = |raw: &Option<String>| -> Option<ModelRef> {
            raw.as_deref()
                .and_then(|raw| serde_json::from_str::<ModelRef>(raw).ok())
        };
        match row {
            Some((embedding, rerank, chunk, top_k, retention)) => Ok(NotebookConfigRow {
                embedding_model_ref: map_ref(&embedding),
                rerank_model_ref: map_ref(&rerank),
                chunk_target_chars: chunk,
                retrieval_top_k: top_k,
                vector_retention_days: retention,
            }),
            None => Ok(NotebookConfigRow::default()),
        }
    }

    /// 配置更新（routes/knowledge.ts:302-318 的 PUT settings；数值范围与模型
    /// 引用形状在此校验——返回更新前后快照供调用方判定重建）。
    pub fn update_notebook_config(
        &self,
        notebook_id: &str,
        embedding_model_ref: Option<Option<ModelRef>>,
        rerank_model_ref: Option<Option<ModelRef>>,
        chunk_target_chars: Option<Option<i64>>,
        retrieval_top_k: Option<Option<i64>>,
        vector_retention_days: Option<Option<i64>>,
    ) -> Result<(NotebookConfigRow, NotebookConfigRow), KnowledgeError> {
        let before = self.get_notebook_config(notebook_id)?;
        let validate_ref = |model_ref: Option<&ModelRef>| -> Result<(), KnowledgeError> {
            if let Some(model_ref) = model_ref {
                if model_ref.id.trim().is_empty() || model_ref.provider.trim().is_empty() {
                    return Err(KnowledgeError::invalid_argument(
                        "Model refs are complete {id, provider} pairs",
                    ));
                }
            }
            Ok(())
        };
        validate_ref(
            embedding_model_ref
                .as_ref()
                .and_then(|inner| inner.as_ref()),
        )?;
        validate_ref(rerank_model_ref.as_ref().and_then(|inner| inner.as_ref()))?;
        if let Some(Some(value)) = chunk_target_chars {
            if !(MIN_CHUNK..=MAX_CHUNK).contains(&value) {
                return Err(KnowledgeError::invalid_argument(
                    "chunkTargetChars must be between 100 and 100000",
                ));
            }
        }
        if let Some(Some(value)) = retrieval_top_k {
            if !(0..=1000).contains(&value) {
                return Err(KnowledgeError::invalid_argument(
                    "retrievalTopK must be between 0 and 1000",
                ));
            }
        }
        if let Some(Some(value)) = vector_retention_days {
            if value <= 0 {
                return Err(KnowledgeError::invalid_argument(
                    "vectorRetentionDays must be a positive number of days",
                ));
            }
        }
        let merged = NotebookConfigRow {
            embedding_model_ref: embedding_model_ref.unwrap_or(before.embedding_model_ref.clone()),
            rerank_model_ref: rerank_model_ref.unwrap_or(before.rerank_model_ref.clone()),
            chunk_target_chars: chunk_target_chars.unwrap_or(before.chunk_target_chars),
            retrieval_top_k: retrieval_top_k.unwrap_or(before.retrieval_top_k),
            vector_retention_days: vector_retention_days.unwrap_or(before.vector_retention_days),
        };
        let conn = lock_conn(&self.conn);
        conn.execute(
            "INSERT INTO notebook_config (notebook_id, embedding_model_json, rerank_model_json, chunk_target_chars, retrieval_top_k, vector_retention_days, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(notebook_id) DO UPDATE SET embedding_model_json=?2, rerank_model_json=?3, chunk_target_chars=?4, retrieval_top_k=?5, vector_retention_days=?6, updated_at=?7",
            params![
                notebook_id,
                merged.embedding_model_ref.as_ref().map(|m| serde_json::to_string(m).unwrap_or_default()),
                merged.rerank_model_ref.as_ref().map(|m| serde_json::to_string(m).unwrap_or_default()),
                merged.chunk_target_chars,
                merged.retrieval_top_k,
                merged.vector_retention_days,
                now_iso(&*self.clock),
            ],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok((before, merged))
    }

    // ── sources / snapshots / membership ────────────────────────────────────

    pub fn create_source_with_snapshot(
        &self,
        studio_id: &str,
        source_type: &str,
        display_name: &str,
        origin_metadata: &serde_json::Value,
        bytes: &[u8],
        mime_type: &str,
    ) -> Result<(SourceRow, SnapshotRow), KnowledgeError> {
        let source_id = new_id("ksrc");
        let snapshot_id = new_id("ksnap");
        let now = now_iso(&*self.clock);
        let sha = sha256_hex(bytes);
        let snapshots_dir = self.root.join("snapshots");
        std::fs::create_dir_all(&snapshots_dir)
            .map_err(|e| KnowledgeError::storage_invalid(format!("snapshots dir: {e}")))?;
        let storage_path = format!("snapshots/{snapshot_id}");
        std::fs::write(snapshots_dir.join(&snapshot_id), bytes)
            .map_err(|e| KnowledgeError::storage_invalid(format!("snapshot write: {e}")))?;
        let conn = lock_conn(&self.conn);
        conn.execute(
            "INSERT INTO sources (id, studio_id, source_type, display_name, origin_metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![source_id, studio_id, source_type, display_name, origin_metadata.to_string(), now],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        conn.execute(
            "INSERT INTO content_snapshots (id, source_id, sha256, mime_type, byte_size, storage_path, captured_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![snapshot_id, source_id, sha, mime_type, bytes.len() as i64, storage_path, now],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        let source = self.get_source(&source_id)?;
        let snapshot = self.latest_snapshot_for_source(&source_id)?;
        Ok((source, snapshot))
    }

    pub fn get_source(&self, source_id: &str) -> Result<SourceRow, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT id, studio_id, source_type, display_name, origin_metadata, created_at, deleted_at, orphaned_at FROM sources WHERE id=?1 AND deleted_at IS NULL",
            params![source_id],
            Self::map_source,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
        .ok_or_else(|| KnowledgeError::not_found("Knowledge source was not found"))
    }

    fn map_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRow> {
        Ok(SourceRow {
            id: row.get(0)?,
            studio_id: row.get(1)?,
            source_type: row.get(2)?,
            display_name: row.get(3)?,
            origin_metadata: parse_json(row.get(4)?),
            created_at: row.get(5)?,
            deleted_at: row.get(6)?,
            orphaned_at: row.get(7)?,
        })
    }

    pub fn latest_snapshot_for_source(
        &self,
        source_id: &str,
    ) -> Result<SnapshotRow, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT id, source_id, sha256, mime_type, byte_size, storage_path, captured_at FROM content_snapshots WHERE source_id=?1 ORDER BY captured_at DESC, id DESC LIMIT 1",
            params![source_id],
            Self::map_snapshot,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
        .ok_or_else(|| KnowledgeError::not_found("Knowledge source has no content snapshot"))
    }

    pub fn get_snapshot(&self, snapshot_id: &str) -> Result<SnapshotRow, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT id, source_id, sha256, mime_type, byte_size, storage_path, captured_at FROM content_snapshots WHERE id=?1",
            params![snapshot_id],
            Self::map_snapshot,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
        .ok_or_else(|| KnowledgeError::not_found("Knowledge snapshot was not found"))
    }

    fn map_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotRow> {
        Ok(SnapshotRow {
            id: row.get(0)?,
            source_id: row.get(1)?,
            sha256: row.get(2)?,
            mime_type: row.get(3)?,
            byte_size: row.get(4)?,
            storage_path: row.get(5)?,
            captured_at: row.get(6)?,
        })
    }

    pub fn read_snapshot_bytes(&self, snapshot: &SnapshotRow) -> Result<Vec<u8>, KnowledgeError> {
        std::fs::read(self.root.join(&snapshot.storage_path))
            .map_err(|e| KnowledgeError::storage_invalid(format!("snapshot read: {e}")))
    }

    /// 解析产物 JSON 原子落盘（artifacts/<snapshotId>/<artifactId>.json）。
    pub fn write_semantic_artifact(
        &self,
        snapshot_id: &str,
        artifact_id: &str,
        payload: &serde_json::Value,
    ) -> Result<String, KnowledgeError> {
        let directory = self.root.join("artifacts").join(snapshot_id);
        std::fs::create_dir_all(&directory)
            .map_err(|e| KnowledgeError::storage_invalid(format!("artifacts dir: {e}")))?;
        let file = directory.join(format!("{artifact_id}.json"));
        let temp = directory.join(format!(
            ".{artifact_id}.{}.tmp",
            new_id("tmp").trim_start_matches("tmp_")
        ));
        std::fs::write(&temp, payload.to_string())
            .map_err(|e| KnowledgeError::storage_invalid(format!("artifact write: {e}")))?;
        std::fs::rename(&temp, &file)
            .map_err(|e| KnowledgeError::storage_invalid(format!("artifact publish: {e}")))?;
        Ok(format!("artifacts/{snapshot_id}/{artifact_id}.json"))
    }

    pub fn add_membership(
        &self,
        notebook_id: &str,
        source_id: &str,
    ) -> Result<MembershipRow, KnowledgeError> {
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "INSERT INTO notebook_sources (notebook_id, source_id, added_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(notebook_id, source_id) DO UPDATE SET removed_at=NULL",
            params![notebook_id, source_id, now],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        // 复活语义：重新加入任一笔记本即清除 orphan 标记。
        conn.execute(
            "UPDATE sources SET orphaned_at=NULL WHERE id=?1",
            params![source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_membership(notebook_id, source_id)
    }

    pub fn get_membership(
        &self,
        notebook_id: &str,
        source_id: &str,
    ) -> Result<MembershipRow, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT notebook_id, source_id, added_at, removed_at, relative_path, folder_node, display_order FROM notebook_sources WHERE notebook_id=?1 AND source_id=?2",
            params![notebook_id, source_id],
            Self::map_membership,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
        .ok_or_else(|| KnowledgeError::not_found("Knowledge source membership was not found"))
    }

    fn map_membership(row: &rusqlite::Row<'_>) -> rusqlite::Result<MembershipRow> {
        Ok(MembershipRow {
            notebook_id: row.get(0)?,
            source_id: row.get(1)?,
            added_at: row.get(2)?,
            removed_at: row.get(3)?,
            relative_path: row.get(4)?,
            folder_node: row.get(5)?,
            display_order: row.get(6)?,
        })
    }

    pub fn remove_source_from_notebook(
        &self,
        notebook_id: &str,
        source_id: &str,
    ) -> Result<MembershipRow, KnowledgeError> {
        let membership = self.get_membership(notebook_id, source_id)?;
        if membership.removed_at.is_some() {
            return Ok(membership);
        }
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE notebook_sources SET removed_at=?1 WHERE notebook_id=?2 AND source_id=?3",
            params![now, notebook_id, source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        // 零活跃 membership → orphan 标记（保留期后 GC；本任务不做定时 GC，D 项）。
        let active: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notebook_sources WHERE source_id=?1 AND removed_at IS NULL",
                params![source_id],
                |row| row.get(0),
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        if active == 0 {
            conn.execute(
                "UPDATE sources SET orphaned_at=?1 WHERE id=?2",
                params![now, source_id],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        }
        drop(conn);
        self.get_membership(notebook_id, source_id)
    }

    pub fn list_notebook_sources(
        &self,
        notebook_id: &str,
    ) -> Result<Vec<NotebookSourceEntry>, KnowledgeError> {
        let sources: Vec<SourceRow> = {
            let conn = lock_conn(&self.conn);
            let mut stmt = conn
                .prepare(
                    "SELECT s.id, s.studio_id, s.source_type, s.display_name, s.origin_metadata, s.created_at, s.deleted_at, s.orphaned_at
                     FROM notebook_sources ns JOIN sources s ON s.id = ns.source_id
                     WHERE ns.notebook_id=?1 AND ns.removed_at IS NULL AND s.deleted_at IS NULL
                     ORDER BY ns.added_at, s.id",
                )
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            let rows = stmt
                .query_map(params![notebook_id], Self::map_source)
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
        };
        let mut entries = Vec::new();
        for source in sources {
            let snapshot = self.latest_snapshot_for_source(&source.id)?;
            let membership = self.get_membership(notebook_id, &source.id)?;
            let parse_artifact = self.latest_parse_artifact_for_snapshot(&snapshot.id)?;
            entries.push(NotebookSourceEntry {
                source,
                snapshot,
                membership,
                parse_artifact,
            });
        }
        Ok(entries)
    }

    /// delete wins：标记删除（后续 ensure/enqueue/refresh 显式失败）。
    pub fn delete_source(&self, source_id: &str) -> Result<SourceRow, KnowledgeError> {
        let source = self.get_source(source_id)?;
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE sources SET deleted_at=?1 WHERE id=?2",
            params![now, source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(SourceRow {
            deleted_at: Some(now),
            ..source
        })
    }

    /// 物理清理（purge）：索引/向量变体由调用方经缓存库清除，这里清事实行 +
    /// 快照/产物文件（引用与块随 artifact 级联删除）。
    pub fn purge_source(&self, source_id: &str) -> Result<(), KnowledgeError> {
        let snapshots: Vec<SnapshotRow> = {
            let conn = lock_conn(&self.conn);
            let mut stmt = conn
                .prepare("SELECT id, source_id, sha256, mime_type, byte_size, storage_path, captured_at FROM content_snapshots WHERE source_id=?1")
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            let rows = stmt
                .query_map(params![source_id], Self::map_snapshot)
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
        };
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM knowledge_turn_scope_sources WHERE source_id=?1",
            params![source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM ingestion_jobs WHERE source_id=?1",
            params![source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM notebook_sources WHERE source_id=?1",
            params![source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM knowledge_citations WHERE parse_artifact_id IN (SELECT id FROM parse_artifacts WHERE content_snapshot_id IN (SELECT id FROM content_snapshots WHERE source_id=?1))",
            params![source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM knowledge_blocks WHERE parse_artifact_id IN (SELECT id FROM parse_artifacts WHERE content_snapshot_id IN (SELECT id FROM content_snapshots WHERE source_id=?1))",
            params![source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM parse_artifacts WHERE content_snapshot_id IN (SELECT id FROM content_snapshots WHERE source_id=?1)",
            params![source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM content_snapshots WHERE source_id=?1",
            params![source_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute("DELETE FROM sources WHERE id=?1", params![source_id])
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.commit()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        for snapshot in snapshots {
            let _ = std::fs::remove_file(self.root.join(&snapshot.storage_path));
        }
        Ok(())
    }

    // ── parse artifacts / blocks / citations ────────────────────────────────

    pub fn begin_parse_artifact(
        &self,
        artifact_id: &str,
        snapshot_id: &str,
        parser_id: &str,
        parser_version: &str,
        parser_config_hash: &str,
    ) -> Result<ParseArtifactRow, KnowledgeError> {
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "INSERT INTO parse_artifacts (id, content_snapshot_id, parser_id, parser_version, parser_config_hash, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'parsing', ?6)",
            params![artifact_id, snapshot_id, parser_id, parser_version, parser_config_hash, now],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_parse_artifact(artifact_id)
    }

    pub fn complete_parse_artifact(
        &self,
        artifact_id: &str,
        status: &str,
        warnings: &[&str],
        fidelity: &str,
        semantic_artifact_path: Option<&str>,
    ) -> Result<ParseArtifactRow, KnowledgeError> {
        let now = now_iso(&*self.clock);
        let warnings_json = serde_json::to_string(warnings).unwrap_or_else(|_| "[]".into());
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE parse_artifacts SET status=?1, warnings=?2, fidelity=?3, semantic_artifact_path=?4, completed_at=?5 WHERE id=?6",
            params![status, warnings_json, fidelity, semantic_artifact_path, now, artifact_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_parse_artifact(artifact_id)
    }

    pub fn fail_parse_artifact(
        &self,
        artifact_id: &str,
        reason: &str,
    ) -> Result<(), KnowledgeError> {
        let now = now_iso(&*self.clock);
        let warnings = serde_json::to_string(&[reason]).unwrap_or_else(|_| "[]".into());
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE parse_artifacts SET status='failed', warnings=?1, completed_at=?2 WHERE id=?3",
            params![warnings, now, artifact_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    pub fn get_parse_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<ParseArtifactRow, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT id, content_snapshot_id, parser_id, parser_version, parser_config_hash, status, warnings, fidelity, processing_artifact_id, created_at, completed_at FROM parse_artifacts WHERE id=?1",
            params![artifact_id],
            Self::map_artifact,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
        .ok_or_else(|| KnowledgeError::not_found("Knowledge parse artifact was not found"))
    }

    fn map_artifact(row: &rusqlite::Row<'_>) -> rusqlite::Result<ParseArtifactRow> {
        Ok(ParseArtifactRow {
            id: row.get(0)?,
            content_snapshot_id: row.get(1)?,
            parser_id: row.get(2)?,
            parser_version: row.get(3)?,
            parser_config_hash: row.get(4)?,
            status: row.get(5)?,
            warnings: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
            fidelity: row.get(7)?,
            processing_artifact_id: row.get(8)?,
            created_at: row.get(9)?,
            completed_at: row.get(10)?,
        })
    }

    pub fn latest_parse_artifact_for_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<ParseArtifactRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT id, content_snapshot_id, parser_id, parser_version, parser_config_hash, status, warnings, fidelity, processing_artifact_id, created_at, completed_at FROM parse_artifacts WHERE content_snapshot_id=?1 ORDER BY created_at DESC, id DESC LIMIT 1",
            params![snapshot_id],
            Self::map_artifact,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    pub fn insert_blocks(&self, blocks: &[BlockRow]) -> Result<(), KnowledgeError> {
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        for block in blocks {
            tx.execute(
                "INSERT INTO knowledge_blocks (id, parse_artifact_id, ordinal, text, text_sha256, locator_type, locator) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![block.id, block.parse_artifact_id, block.ordinal as i64, block.text, block.text_sha256, block.locator_type, block.locator.to_string()],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    fn map_block(row: &rusqlite::Row<'_>) -> rusqlite::Result<BlockRow> {
        Ok(BlockRow {
            id: row.get(0)?,
            parse_artifact_id: row.get(1)?,
            ordinal: row.get::<_, i64>(2)? as usize,
            text: row.get(3)?,
            text_sha256: row.get(4)?,
            locator_type: row.get(5)?,
            locator: parse_json(row.get(6)?),
        })
    }

    pub fn list_artifact_blocks(&self, artifact_id: &str) -> Result<Vec<BlockRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, parse_artifact_id, ordinal, text, text_sha256, locator_type, locator FROM knowledge_blocks WHERE parse_artifact_id=?1 ORDER BY ordinal",
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let rows = stmt
            .query_map(params![artifact_id], Self::map_block)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    pub fn get_blocks_by_ids(&self, block_ids: &[String]) -> Result<Vec<BlockRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut blocks = Vec::new();
        for block_id in block_ids {
            if let Some(block) = conn
                .query_row(
                    "SELECT id, parse_artifact_id, ordinal, text, text_sha256, locator_type, locator FROM knowledge_blocks WHERE id=?1",
                    params![block_id],
                    Self::map_block,
                )
                .optional()
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
            {
                blocks.push(block);
            }
        }
        Ok(blocks)
    }

    /// 创建引用：偏移锚定块文本（Unicode scalar 口径——差异台账 D 项），canonical
    /// text 即切片。
    pub fn create_citation(
        &self,
        artifact_id: &str,
        block_id: &str,
        start_offset: usize,
        end_offset: usize,
    ) -> Result<CitationRow, KnowledgeError> {
        let block = self
            .list_artifact_blocks(artifact_id)?
            .into_iter()
            .find(|block| block.id == block_id)
            .ok_or_else(|| {
                KnowledgeError::index_invalid("Read range is outside its frozen raw block")
            })?;
        let chars: Vec<char> = block.text.chars().collect();
        if end_offset > chars.len() || start_offset > end_offset {
            return Err(KnowledgeError::index_invalid(
                "Read range is outside its frozen raw block",
            ));
        }
        let canonical: String = chars[start_offset..end_offset].iter().collect();
        let canonical_sha = sha256_hex(canonical.as_bytes());
        let id = new_id("kcit");
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "INSERT INTO knowledge_citations (id, parse_artifact_id, block_id, start_offset, end_offset, canonical_text, canonical_text_sha256, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, artifact_id, block_id, start_offset as i64, end_offset as i64, canonical, canonical_sha, now],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(CitationRow {
            id,
            parse_artifact_id: artifact_id.to_string(),
            block_id: block_id.to_string(),
            start_offset,
            end_offset,
            canonical_text: canonical,
            canonical_text_sha256: canonical_sha,
            created_at: now,
        })
    }

    pub fn resolve_citation(&self, citation_id: &str) -> Result<ResolvedCitation, KnowledgeError> {
        let citation = {
            let conn = lock_conn(&self.conn);
            conn.query_row(
                "SELECT id, parse_artifact_id, block_id, start_offset, end_offset, canonical_text, canonical_text_sha256, created_at FROM knowledge_citations WHERE id=?1",
                params![citation_id],
                |row| {
                    Ok(CitationRow {
                        id: row.get(0)?,
                        parse_artifact_id: row.get(1)?,
                        block_id: row.get(2)?,
                        start_offset: row.get::<_, i64>(3)? as usize,
                        end_offset: row.get::<_, i64>(4)? as usize,
                        canonical_text: row.get(5)?,
                        canonical_text_sha256: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
            .ok_or_else(|| KnowledgeError::not_found("Knowledge citation was not found"))?
        };
        let block = self
            .get_blocks_by_ids(std::slice::from_ref(&citation.block_id))?
            .into_iter()
            .next()
            .ok_or_else(|| KnowledgeError::not_found("Knowledge citation block was not found"))?;
        let artifact = self.get_parse_artifact(&citation.parse_artifact_id)?;
        let snapshot = self.get_snapshot(&artifact.content_snapshot_id)?;
        let source = self.get_source(&snapshot.source_id)?;
        Ok(ResolvedCitation {
            citation,
            block,
            snapshot,
            source,
        })
    }

    // ── ingestion jobs ──────────────────────────────────────────────────────

    pub fn enqueue_source_ingestion(
        &self,
        notebook_id: &str,
        source_id: &str,
        artifact_id: Option<&str>,
        chunker_config_id: &str,
    ) -> Result<JobRow, KnowledgeError> {
        // delete wins：已删源显式失败，绝不静默入队。
        self.get_source(source_id)?;
        let id = new_id("kjob");
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "INSERT INTO ingestion_jobs (id, notebook_id, source_id, artifact_id, phase, status, chunker_config_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'parse', 'queued', ?5, ?6, ?6)",
            params![id, notebook_id, source_id, artifact_id, chunker_config_id, now],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_job(&id)
    }

    pub fn get_job(&self, job_id: &str) -> Result<JobRow, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(JOB_SELECT, params![job_id], Self::map_job)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    const JOB_COLUMNS: &'static str = "id, notebook_id, source_id, artifact_id, phase, status, attempt, error, chunker_config_id, cancelled_at, progress_done, progress_total, embedding_stats, created_at, updated_at";

    fn map_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRow> {
        Ok(JobRow {
            id: row.get(0)?,
            notebook_id: row.get(1)?,
            source_id: row.get(2)?,
            artifact_id: row.get(3)?,
            phase: row.get(4)?,
            status: row.get(5)?,
            attempt: row.get(6)?,
            error: row.get(7)?,
            chunker_config_id: row.get(8)?,
            cancelled_at: row.get(9)?,
            progress_done: row.get(10)?,
            progress_total: row.get(11)?,
            embedding_stats: row
                .get::<_, Option<String>>(12)?
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            created_at: row.get(13)?,
            updated_at: row.get(14)?,
        })
    }

    pub fn list_ingestion_jobs(
        &self,
        notebook_id: Option<&str>,
        source_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<JobRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut sql = format!("SELECT {} FROM ingestion_jobs WHERE 1=1", Self::JOB_COLUMNS);
        if notebook_id.is_some() {
            sql.push_str(" AND notebook_id=?1");
        }
        if source_id.is_some() {
            let index = if notebook_id.is_some() { 2 } else { 1 };
            sql.push_str(&format!(" AND source_id=?{index}"));
        }
        sql.push_str(&format!(" ORDER BY created_at DESC, id DESC LIMIT {limit}"));
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let rows = match (notebook_id, source_id) {
            (Some(nb), Some(src)) => stmt
                .query_map(params![nb, src], Self::map_job)
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?,
            (Some(nb), None) => stmt
                .query_map(params![nb], Self::map_job)
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?,
            (None, Some(src)) => stmt
                .query_map(params![src], Self::map_job)
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?,
            (None, None) => stmt
                .query_map([], Self::map_job)
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?,
        };
        Ok(rows)
    }

    pub fn count_jobs_by_status(
        &self,
        notebook_id: Option<&str>,
    ) -> Result<serde_json::Value, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut counts = serde_json::json!({
            "queued": 0, "running": 0, "pending_embedding": 0, "failed": 0, "done": 0
        });
        let mut stmt = conn
            .prepare("SELECT status, COUNT(*) FROM ingestion_jobs WHERE (?1 IS NULL OR notebook_id=?1) GROUP BY status")
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let rows = stmt
            .query_map(params![notebook_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        for row in rows {
            let (status, count) =
                row.map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            counts[status.as_str()] = serde_json::json!(count);
        }
        Ok(counts)
    }

    pub fn latest_job_for_source(
        &self,
        notebook_id: &str,
        source_id: &str,
    ) -> Result<Option<JobRow>, KnowledgeError> {
        Ok(self
            .list_ingestion_jobs(Some(notebook_id), Some(source_id), 1)?
            .into_iter()
            .next())
    }

    /// 取下一个可执行 job（queued；cancel 检查在服务编排内）。
    pub fn claim_next_job(&self) -> Result<Option<JobRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let next: Option<String> = conn
            .query_row(
                "SELECT id FROM ingestion_jobs WHERE status='queued' AND cancelled_at IS NULL ORDER BY created_at, id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let Some(id) = next else { return Ok(None) };
        let now = now_iso(&*self.clock);
        conn.execute(
            "UPDATE ingestion_jobs SET status='running', attempt=attempt+1, updated_at=?1 WHERE id=?2 AND status='queued'",
            params![now, id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_job(&id).map(Some)
    }

    pub fn set_job_phase(&self, job_id: &str, phase: &str) -> Result<(), KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE ingestion_jobs SET phase=?1, updated_at=?2 WHERE id=?3",
            params![phase, now_iso(&*self.clock), job_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    pub fn set_job_progress(
        &self,
        job_id: &str,
        done: i64,
        total: Option<i64>,
    ) -> Result<(), KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE ingestion_jobs SET progress_done=?1, progress_total=?2, updated_at=?3 WHERE id=?4",
            params![done, total, now_iso(&*self.clock), job_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    pub fn complete_job(
        &self,
        job_id: &str,
        embedding_stats: &serde_json::Value,
    ) -> Result<(), KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE ingestion_jobs SET phase='done', status='done', error=NULL, embedding_stats=?1, updated_at=?2 WHERE id=?3",
            params![embedding_stats.to_string(), now_iso(&*self.clock), job_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    pub fn fail_job(&self, job_id: &str, error: &str) -> Result<(), KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE ingestion_jobs SET status='failed', error=?1, updated_at=?2 WHERE id=?3",
            params![error, now_iso(&*self.clock), job_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    /// pending_embedding 显式终态：FTS 已可查、嵌入模型未配置（禁静默降级）。
    pub fn mark_job_pending_embedding(&self, job_id: &str) -> Result<(), KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE ingestion_jobs SET phase='embed', status='pending_embedding', updated_at=?1 WHERE id=?2",
            params![now_iso(&*self.clock), job_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    /// pending_embedding 的补跑重排（模型就绪信号 / 启动恢复）。
    pub fn requeue_pending_embedding_jobs(&self) -> Result<usize, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let count = conn
            .execute(
                "UPDATE ingestion_jobs SET status='queued', updated_at=?1 WHERE status='pending_embedding' AND cancelled_at IS NULL",
                params![now_iso(&*self.clock)],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(count)
    }

    /// failed 手动重试（requeue；attempt 保留、从失败 phase 续跑）。
    pub fn requeue_job(&self, job_id: &str) -> Result<JobRow, KnowledgeError> {
        let job = self.get_job(job_id)?;
        if job.status != "failed" {
            return Err(KnowledgeError::conflict(
                "Only failed ingestion jobs can be retried",
            ));
        }
        // 取消留痕的 job 不可重试（被删源永不复活；types.ts:521-525 同语义）。
        if job.cancelled_at.is_some() {
            return Err(KnowledgeError::conflict(
                "Cancelled ingestion jobs cannot be retried",
            ));
        }
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE ingestion_jobs SET status='queued', error=NULL, updated_at=?1 WHERE id=?2",
            params![now_iso(&*self.clock), job_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_job(job_id)
    }

    /// delete wins：取消该源全部活跃 job（failed 终态 + cancelledAt 显式标注）。
    pub fn cancel_jobs_for_source(&self, source_id: &str) -> Result<usize, KnowledgeError> {
        let now = now_iso(&*self.clock);
        let conn = lock_conn(&self.conn);
        let count = conn
            .execute(
                "UPDATE ingestion_jobs SET status='failed', cancelled_at=?1, error='cancelled: source deleted', updated_at=?1 WHERE source_id=?2 AND status IN ('queued','running','pending_embedding')",
                params![now, source_id],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(count)
    }

    // ── turn scopes ─────────────────────────────────────────────────────────

    /// 创建轮 scope：同会话旧 active 关闭（supersede）；逐选中笔记本取活跃
    /// membership 并冻结每源最新 snapshot/artifact（knowledge-store.ts:3281-3344）。
    pub fn create_turn_scope(
        &self,
        studio_id: &str,
        session_id: &str,
        turn_id: &str,
        notebook_ids: &[String],
    ) -> Result<TurnScopeRow, KnowledgeError> {
        if notebook_ids.is_empty() {
            return Err(KnowledgeError::invalid_argument(
                "notebookIds must be a non-empty array",
            ));
        }
        let mut unique_ids: Vec<String> = Vec::new();
        for notebook_id in notebook_ids {
            if !unique_ids.contains(notebook_id) {
                unique_ids.push(notebook_id.clone());
            }
        }
        for notebook_id in &unique_ids {
            self.get_notebook(studio_id, notebook_id)?;
        }
        let id = new_id("kts");
        let now = now_iso(&*self.clock);
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "UPDATE knowledge_turn_scopes SET status='closed' WHERE session_id=?1 AND status='active'",
            params![session_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "INSERT INTO knowledge_turn_scopes (id, turn_id, session_id, studio_id, notebook_ids_json, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)",
            params![id, turn_id, session_id, studio_id, serde_json::to_string(&unique_ids).unwrap_or_default(), now],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let mut frozen: Vec<TurnScopeSourceRow> = Vec::new();
        for notebook_id in &unique_ids {
            let mut stmt = tx
                .prepare(
                    "SELECT s.id FROM notebook_sources ns JOIN sources s ON s.id = ns.source_id
                     WHERE ns.notebook_id=?1 AND ns.removed_at IS NULL AND s.deleted_at IS NULL
                     ORDER BY ns.added_at, s.id",
                )
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            let source_ids = stmt
                .query_map(params![notebook_id], |row| row.get::<_, String>(0))
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            for source_id in source_ids {
                if let Some(existing) = frozen.iter_mut().find(|row| row.source_id == source_id) {
                    existing.notebook_ids.push(notebook_id.clone());
                    // 同一源被多个选中笔记本引用时合并为一行——冻结行的
                    // notebook_ids 同步回写（合并语义对读取侧可见）。
                    tx.execute(
                        "UPDATE knowledge_turn_scope_sources SET notebook_ids_json=?1 WHERE scope_id=?2 AND source_id=?3",
                        params![serde_json::to_string(&existing.notebook_ids).unwrap_or_default(), id, source_id],
                    )
                    .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
                    continue;
                }
                let snapshot_id: String = tx
                    .query_row(
                        "SELECT id FROM content_snapshots WHERE source_id=?1 ORDER BY captured_at DESC, id DESC LIMIT 1",
                        params![source_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
                    .ok_or_else(|| {
                        KnowledgeError::not_found("Knowledge source has no content snapshot")
                    })?;
                let artifact_id: Option<String> = tx
                    .query_row(
                        "SELECT id FROM parse_artifacts WHERE content_snapshot_id=?1 AND status IN ('ready','needs_ocr') ORDER BY created_at DESC, id DESC LIMIT 1",
                        params![snapshot_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
                tx.execute(
                    "INSERT INTO knowledge_turn_scope_sources (scope_id, source_id, content_snapshot_id, parse_artifact_id, notebook_ids_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, source_id, snapshot_id, artifact_id, serde_json::to_string(&[notebook_id]).unwrap_or_default()],
                )
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
                frozen.push(TurnScopeSourceRow {
                    scope_id: id.clone(),
                    source_id,
                    content_snapshot_id: snapshot_id,
                    parse_artifact_id: artifact_id,
                    notebook_ids: vec![notebook_id.clone()],
                });
            }
        }
        tx.commit()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_turn_scope(&id)?
            .ok_or_else(|| KnowledgeError::storage_invalid("turn scope vanished"))
    }

    pub fn get_turn_scope(&self, scope_id: &str) -> Result<Option<TurnScopeRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let scope = conn
            .query_row(
                "SELECT id, turn_id, session_id, studio_id, notebook_ids_json, status, created_at FROM knowledge_turn_scopes WHERE id=?1",
                params![scope_id],
                |row| {
                    Ok(TurnScopeRow {
                        id: row.get(0)?,
                        turn_id: row.get(1)?,
                        session_id: row.get(2)?,
                        studio_id: row.get(3)?,
                        notebook_ids: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                        status: row.get(5)?,
                        created_at: row.get(6)?,
                        sources: Vec::new(),
                    })
                },
            )
            .optional()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let Some(mut scope) = scope else {
            return Ok(None);
        };
        let mut stmt = conn
            .prepare("SELECT scope_id, source_id, content_snapshot_id, parse_artifact_id, notebook_ids_json FROM knowledge_turn_scope_sources WHERE scope_id=?1 ORDER BY source_id")
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let sources = stmt
            .query_map(params![scope_id], |row| {
                Ok(TurnScopeSourceRow {
                    scope_id: row.get(0)?,
                    source_id: row.get(1)?,
                    content_snapshot_id: row.get(2)?,
                    parse_artifact_id: row.get(3)?,
                    notebook_ids: serde_json::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or_default(),
                })
            })
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        scope.sources = sources;
        Ok(Some(scope))
    }

    /// 活跃 scope 是否冻结引用该源（delete wins 的 409 依据）。
    pub fn any_active_scope_freezes_source(&self, source_id: &str) -> Result<bool, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM knowledge_turn_scope_sources s
                 JOIN knowledge_turn_scopes t ON t.id = s.scope_id
                 WHERE s.source_id=?1 AND t.status='active'",
                params![source_id],
                |row| row.get(0),
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(count > 0)
    }
}

const JOB_SELECT: &str = "SELECT id, notebook_id, source_id, artifact_id, phase, status, attempt, error, chunker_config_id, cancelled_at, progress_done, progress_total, embedding_stats, created_at, updated_at FROM ingestion_jobs WHERE id=?1";

// ── FTS 索引缓存库 ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct IndexVariantRow {
    pub id: String,
    pub parse_artifact_id: String,
    pub chunk_profile_hash: String,
    pub block_fingerprint: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredChunk {
    pub chunk_id: String,
    pub variant_id: String,
    pub ordinal: usize,
    pub section_id: Option<String>,
    pub text: String,
    pub token_count: usize,
    pub spans: Vec<ChunkSpan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionMetaRow {
    pub id: String,
    pub section_ordinal: usize,
    pub heading_path: Vec<String>,
    pub start_block_ordinal: usize,
    pub end_block_ordinal: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexHit {
    pub variant_id: String,
    pub chunk: StoredChunk,
    pub score: f64,
}

pub struct KnowledgeIndexStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
}

const INDEX_SCHEMA_VERSION: i64 = 1;

impl KnowledgeIndexStore {
    /// 打开 FTS 缓存库；打不开/损坏 → 按「缓存损坏」语义删库重建（只重建该文件）。
    pub fn open(root: &Path) -> Result<Self, KnowledgeError> {
        std::fs::create_dir_all(root)
            .map_err(|e| KnowledgeError::storage_invalid(format!("knowledge root: {e}")))?;
        let db_path = root.join("knowledge_index.db");
        match Self::try_open(&db_path) {
            Ok(store) => Ok(store),
            Err(first_error) => {
                tracing::warn!(
                    error = %first_error,
                    "knowledge index cache unreadable; resetting the cache file"
                );
                let _ = std::fs::remove_file(&db_path);
                let _ = std::fs::remove_file(root.join("knowledge_index.db-wal"));
                let _ = std::fs::remove_file(root.join("knowledge_index.db-shm"));
                Self::try_open(&db_path)
            }
        }
    }

    fn try_open(db_path: &Path) -> Result<Self, KnowledgeError> {
        let conn = open_connection(db_path)?;
        check_user_version(&conn, INDEX_SCHEMA_VERSION)?;
        conn.execute_batch(INDEX_SCHEMA_SQL)
            .map_err(|e| KnowledgeError::storage_invalid(format!("index schema: {e}")))?;
        conn.pragma_update(None, "user_version", INDEX_SCHEMA_VERSION)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
        })
    }

    pub fn ensure_chunk_index_variant(
        &self,
        parse_artifact_id: &str,
        chunk_profile_hash: &str,
        block_fingerprint: &str,
    ) -> Result<IndexVariantRow, KnowledgeError> {
        let id = chunk_index_variant_id(parse_artifact_id, chunk_profile_hash);
        let conn = lock_conn(&self.conn);
        conn.execute(
            "INSERT INTO chunk_index_variants (id, parse_artifact_id, chunk_profile_hash, block_fingerprint, status) VALUES (?1, ?2, ?3, ?4, 'building')
             ON CONFLICT(id) DO NOTHING",
            params![id, parse_artifact_id, chunk_profile_hash, block_fingerprint],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        drop(conn);
        self.get_variant(&id)?
            .ok_or_else(|| KnowledgeError::storage_invalid("chunk index variant vanished"))
    }

    pub fn get_variant(&self, variant_id: &str) -> Result<Option<IndexVariantRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT id, parse_artifact_id, chunk_profile_hash, block_fingerprint, status FROM chunk_index_variants WHERE id=?1",
            params![variant_id],
            Self::map_variant,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    fn map_variant(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexVariantRow> {
        Ok(IndexVariantRow {
            id: row.get(0)?,
            parse_artifact_id: row.get(1)?,
            chunk_profile_hash: row.get(2)?,
            block_fingerprint: row.get(3)?,
            status: row.get(4)?,
        })
    }

    pub fn set_variant_status(&self, variant_id: &str, status: &str) -> Result<(), KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE chunk_index_variants SET status=?1 WHERE id=?2",
            params![status, variant_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    /// 指纹门：同 (artifact, profile) 变体 ready 且指纹一致才整体跳过重建。
    pub fn has_artifact_fingerprint(
        &self,
        parse_artifact_id: &str,
        chunk_profile_hash: &str,
        block_fingerprint: &str,
    ) -> Result<bool, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let row = conn
            .query_row(
                "SELECT block_fingerprint, status FROM chunk_index_variants WHERE parse_artifact_id=?1 AND chunk_profile_hash=?2",
                params![parse_artifact_id, chunk_profile_hash],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(
            matches!(row, Some((fingerprint, status)) if fingerprint == block_fingerprint && status == "ready"),
        )
    }

    /// chunk+FTS 同事务幂等替换（查询只读 ready 变体，不读半写状态）。
    pub fn replace_artifact_chunks(
        &self,
        parse_artifact_id: &str,
        chunk_profile_hash: &str,
        block_fingerprint: &str,
        chunks: &[ChunkDraft],
        sections: &[SectionMetaRow],
    ) -> Result<String, KnowledgeError> {
        let variant = self.ensure_chunk_index_variant(
            parse_artifact_id,
            chunk_profile_hash,
            block_fingerprint,
        )?;
        if chunks.is_empty() {
            return Err(KnowledgeError::index_invalid(
                "Ready Knowledge source produced no searchable chunks",
            ));
        }
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "UPDATE chunk_index_variants SET block_fingerprint=?1, status='building' WHERE id=?2",
            params![block_fingerprint, variant.id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM knowledge_chunks WHERE variant_id=?1",
            params![variant.id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        for chunk in chunks {
            tx.execute(
                "INSERT INTO knowledge_chunks (variant_id, chunk_id, ordinal, section_id, text, search_text, token_count, spans_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![variant.id, chunk.id, chunk.ordinal as i64, chunk.section_id, chunk.text, build_search_document_text(&chunk.text), chunk.token_count as i64, spans_to_json(&chunk.spans)],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        }
        tx.execute(
            "INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts) VALUES ('rebuild')",
            [],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.execute(
            "DELETE FROM knowledge_sections WHERE parse_artifact_id=?1",
            params![parse_artifact_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        for section in sections {
            tx.execute(
                "INSERT INTO knowledge_sections (id, parse_artifact_id, section_ordinal, heading_path_json, start_block_ordinal, end_block_ordinal) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![section.id, parse_artifact_id, section.section_ordinal as i64, serde_json::to_string(&section.heading_path).unwrap_or_default(), section.start_block_ordinal as i64, section.end_block_ordinal as i64],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        }
        tx.execute(
            "UPDATE chunk_index_variants SET status='ready' WHERE id=?1",
            params![variant.id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        tx.commit()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(variant.id)
    }

    fn map_stored_chunk(
        variant_id: &str,
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<StoredChunk> {
        Ok(StoredChunk {
            chunk_id: row.get(0)?,
            variant_id: variant_id.to_string(),
            ordinal: row.get::<_, i64>(1)? as usize,
            section_id: row.get(2)?,
            text: row.get(3)?,
            token_count: row.get::<_, i64>(4)? as usize,
            spans: spans_from_json(&row.get::<_, String>(5)?),
        })
    }

    pub fn list_variant_chunks(
        &self,
        variant_id: &str,
    ) -> Result<Vec<StoredChunk>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT chunk_id, ordinal, section_id, text, token_count, spans_json FROM knowledge_chunks WHERE variant_id=?1 ORDER BY ordinal",
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let rows = stmt
            .query_map(params![variant_id], |row| {
                Self::map_stored_chunk(variant_id, row)
            })
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    /// FTS 检索：限定变体集合（scope 作用域的存储侧体现）。query 为 FTS5 MATCH
    /// 词项（服务层构造）；空查询回落全变体前 N。
    pub fn search_fts(
        &self,
        variant_ids: &[&str],
        query: &str,
        limit: usize,
    ) -> Result<Vec<IndexHit>, KnowledgeError> {
        if variant_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = variant_ids
            .iter()
            .enumerate()
            .map(|(index, _)| format!("?{}", index + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let trimmed = query.trim();
        let match_index = variant_ids.len() + 1;
        let sql = if trimmed.is_empty() {
            format!(
                "SELECT c.chunk_id, c.ordinal, c.section_id, c.text, c.token_count, c.spans_json, c.variant_id \
                 FROM knowledge_chunks c WHERE c.variant_id IN ({placeholders}) ORDER BY c.ordinal LIMIT {limit}"
            )
        } else {
            format!(
                "SELECT c.chunk_id, c.ordinal, c.section_id, c.text, c.token_count, c.spans_json, c.variant_id, bm25(knowledge_chunks_fts) AS score \
                 FROM knowledge_chunks_fts f JOIN knowledge_chunks c ON c.rowid = f.rowid \
                 WHERE knowledge_chunks_fts MATCH ?{match_index} AND c.variant_id IN ({placeholders}) \
                 ORDER BY score LIMIT {limit}"
            )
        };
        let conn = lock_conn(&self.conn);
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let mut bound: Vec<rusqlite::types::Value> = variant_ids
            .iter()
            .map(|id| rusqlite::types::Value::Text((*id).to_string()))
            .collect();
        if !trimmed.is_empty() {
            bound.push(rusqlite::types::Value::Text(trimmed.to_string()));
        }
        let scored = !trimmed.is_empty();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bound), move |row| {
                let chunk = StoredChunk {
                    chunk_id: row.get(0)?,
                    variant_id: row.get(6)?,
                    ordinal: row.get::<_, i64>(1)? as usize,
                    section_id: row.get(2)?,
                    text: row.get(3)?,
                    token_count: row.get::<_, i64>(4)? as usize,
                    spans: spans_from_json(&row.get::<_, String>(5)?),
                };
                let score: f64 = if scored {
                    row.get(7).unwrap_or(0.0)
                } else {
                    0.0
                };
                Ok(IndexHit {
                    score,
                    variant_id: chunk.variant_id.clone(),
                    chunk,
                })
            })
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let hits = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(hits)
    }

    pub fn get_ready_variant_metadata(
        &self,
        parse_artifact_id: &str,
        chunk_profile_hash: &str,
    ) -> Result<Option<(String, usize)>, KnowledgeError> {
        let id = chunk_index_variant_id(parse_artifact_id, chunk_profile_hash);
        let row = self.get_variant(&id)?;
        match row {
            Some(variant) if variant.status == "ready" => {
                let conn = lock_conn(&self.conn);
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM knowledge_chunks WHERE variant_id=?1",
                        params![id],
                        |row| row.get(0),
                    )
                    .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
                Ok(Some((id, count as usize)))
            }
            _ => Ok(None),
        }
    }

    pub fn list_ready_variant_metadata(
        &self,
        parse_artifact_id: &str,
    ) -> Result<Vec<(String, usize)>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id FROM chunk_index_variants WHERE parse_artifact_id=?1 AND status='ready'",
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let ids = stmt
            .query_map(params![parse_artifact_id], |row| row.get::<_, String>(0))
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let mut out = Vec::new();
        for id in ids {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM knowledge_chunks WHERE variant_id=?1",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            out.push((id, count as usize));
        }
        Ok(out)
    }

    pub fn list_artifact_section_metadata(
        &self,
        parse_artifact_id: &str,
    ) -> Result<Vec<SectionMetaRow>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, section_ordinal, heading_path_json, start_block_ordinal, end_block_ordinal FROM knowledge_sections WHERE parse_artifact_id=?1 ORDER BY section_ordinal",
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let rows = stmt
            .query_map(params![parse_artifact_id], |row| {
                Ok(SectionMetaRow {
                    id: row.get(0)?,
                    section_ordinal: row.get::<_, i64>(1)? as usize,
                    heading_path: serde_json::from_str(&row.get::<_, String>(2)?)
                        .unwrap_or_default(),
                    start_block_ordinal: row.get::<_, i64>(3)? as usize,
                    end_block_ordinal: row.get::<_, i64>(4)? as usize,
                })
            })
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    /// 缓存级 reset（损坏自愈；只动缓存文件）。
    pub fn reset(&self) -> Result<(), KnowledgeError> {
        let _ = std::fs::remove_file(&self.db_path);
        Ok(())
    }
}

const INDEX_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS chunk_index_variants (
    id                 TEXT PRIMARY KEY,
    parse_artifact_id  TEXT NOT NULL,
    chunk_profile_hash TEXT NOT NULL,
    block_fingerprint  TEXT NOT NULL,
    status             TEXT NOT NULL CHECK(status IN ('building','ready','failed')),
    UNIQUE (parse_artifact_id, chunk_profile_hash)
);
CREATE TABLE IF NOT EXISTS knowledge_chunks (
    variant_id   TEXT NOT NULL REFERENCES chunk_index_variants(id),
    chunk_id     TEXT NOT NULL,
    ordinal      INTEGER NOT NULL,
    section_id   TEXT,
    text         TEXT NOT NULL,
    search_text  TEXT NOT NULL DEFAULT '',
    token_count  INTEGER NOT NULL,
    spans_json   TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (variant_id, ordinal)
);
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_fts USING fts5(
    text,
    search_text,
    content='knowledge_chunks',
    content_rowid='rowid',
    tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS chunks_fts_ai AFTER INSERT ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(rowid, text, search_text) VALUES (new.rowid, new.text, new.search_text);
END;
CREATE TRIGGER IF NOT EXISTS chunks_fts_ad AFTER DELETE ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts, rowid, text, search_text) VALUES ('delete', old.rowid, old.text, old.search_text);
END;
CREATE TABLE IF NOT EXISTS knowledge_sections (
    id                  TEXT NOT NULL,
    parse_artifact_id   TEXT NOT NULL,
    section_ordinal     INTEGER NOT NULL,
    heading_path_json   TEXT NOT NULL DEFAULT '[]',
    start_block_ordinal INTEGER NOT NULL,
    end_block_ordinal   INTEGER NOT NULL,
    PRIMARY KEY (parse_artifact_id, section_ordinal)
);
"#;

// ── 向量缓存库 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct VectorVariantRecord {
    pub variant_id: String,
    pub chunk_index_variant_id: String,
    pub parse_artifact_id: String,
    pub model_key: String,
    pub chunk_fingerprint: String,
    pub dimensions: u32,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorEntry {
    pub chunk_id: String,
    pub ordinal: usize,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub variant_id: String,
    pub chunk_id: String,
    pub parse_artifact_id: String,
    pub score: f32,
}

pub struct BuildBegin {
    pub variant_id: String,
    pub reset_stale: bool,
}

pub struct KnowledgeVectorStore {
    conn: Mutex<Connection>,
}

const VECTOR_SCHEMA_VERSION: i64 = 1;

const VECTOR_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS vector_index_variants (
    id                     TEXT PRIMARY KEY,
    chunk_index_variant_id TEXT NOT NULL,
    parse_artifact_id      TEXT NOT NULL,
    model_key              TEXT NOT NULL,
    chunk_fingerprint      TEXT NOT NULL,
    dimensions             INTEGER NOT NULL CHECK(dimensions > 0),
    status                 TEXT NOT NULL CHECK(status IN ('building','ready','failed')),
    UNIQUE (chunk_index_variant_id, model_key)
);
CREATE INDEX IF NOT EXISTS idx_vector_variants_artifact ON vector_index_variants(parse_artifact_id);
CREATE TABLE IF NOT EXISTS chunk_vectors (
    vector_index_variant_id TEXT NOT NULL REFERENCES vector_index_variants(id),
    chunk_id     TEXT NOT NULL,
    ordinal      INTEGER NOT NULL,
    vector       BLOB NOT NULL,
    PRIMARY KEY (vector_index_variant_id, chunk_id)
);
"#;

impl KnowledgeVectorStore {
    pub fn open(root: &Path) -> Result<Self, KnowledgeError> {
        std::fs::create_dir_all(root)
            .map_err(|e| KnowledgeError::storage_invalid(format!("knowledge root: {e}")))?;
        let db_path = root.join("knowledge_vector.db");
        match Self::try_open(&db_path) {
            Ok(store) => Ok(store),
            Err(first_error) => {
                tracing::warn!(
                    error = %first_error,
                    "knowledge vector cache unreadable; resetting the cache file"
                );
                let _ = std::fs::remove_file(&db_path);
                let _ = std::fs::remove_file(root.join("knowledge_vector.db-wal"));
                let _ = std::fs::remove_file(root.join("knowledge_vector.db-shm"));
                Self::try_open(&db_path)
            }
        }
    }

    fn try_open(db_path: &Path) -> Result<Self, KnowledgeError> {
        let conn = open_connection(db_path)?;
        check_user_version(&conn, VECTOR_SCHEMA_VERSION)?;
        conn.execute_batch(VECTOR_SCHEMA_SQL)
            .map_err(|e| KnowledgeError::storage_invalid(format!("vector schema: {e}")))?;
        conn.pragma_update(None, "user_version", VECTOR_SCHEMA_VERSION)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let _ = db_path;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// begin：既有行指纹或维度漂移 → 显式清旧向量重建（resetStaleVectors 留痕）；
    /// 一致 → 保留已落库向量（断点续嵌锚）。维度 1..=65536。
    pub fn begin_variant_build(
        &self,
        chunk_index_variant_id: &str,
        parse_artifact_id: &str,
        model_key: &str,
        chunk_fingerprint: &str,
        dimensions: u32,
    ) -> Result<BuildBegin, KnowledgeError> {
        if dimensions == 0 || dimensions > 65_536 {
            return Err(KnowledgeError::invalid_argument(
                "Vector dimensions are invalid",
            ));
        }
        let variant_id = vector_index_variant_id(chunk_index_variant_id, model_key);
        let conn = lock_conn(&self.conn);
        let existing = conn
            .query_row(
                "SELECT id, chunk_index_variant_id, parse_artifact_id, model_key, chunk_fingerprint, dimensions, status FROM vector_index_variants WHERE id=?1",
                params![variant_id],
                Self::map_variant,
            )
            .optional()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let reset_stale = match &existing {
            Some(record)
                if record.chunk_fingerprint != chunk_fingerprint
                    || record.dimensions != dimensions =>
            {
                conn.execute(
                    "DELETE FROM chunk_vectors WHERE vector_index_variant_id=?1",
                    params![variant_id],
                )
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
                true
            }
            _ => false,
        };
        conn.execute(
            "INSERT INTO vector_index_variants (id, chunk_index_variant_id, parse_artifact_id, model_key, chunk_fingerprint, dimensions, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'building')
             ON CONFLICT(id) DO UPDATE SET status='building', chunk_fingerprint=?5, dimensions=?6",
            params![variant_id, chunk_index_variant_id, parse_artifact_id, model_key, chunk_fingerprint, dimensions],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(BuildBegin {
            variant_id,
            reset_stale,
        })
    }

    /// 每批幂等 upsert；维度与 variant 记录不符 → 拒绝混写（KNOWLEDGE_CONFLICT）。
    pub fn upsert_batch(
        &self,
        variant_id: &str,
        entries: &[VectorEntry],
    ) -> Result<(), KnowledgeError> {
        let mut conn = lock_conn(&self.conn);
        let variant = conn
            .query_row(
                "SELECT id, chunk_index_variant_id, parse_artifact_id, model_key, chunk_fingerprint, dimensions, status FROM vector_index_variants WHERE id=?1",
                params![variant_id],
                Self::map_variant,
            )
            .optional()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
            .ok_or_else(|| KnowledgeError::conflict("vector variant does not exist"))?;
        for entry in entries {
            if entry.vector.len() != variant.dimensions as usize {
                return Err(KnowledgeError::conflict(
                    "vector dimension does not match the variant identity",
                ));
            }
        }
        let tx = conn
            .transaction()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        for entry in entries {
            let mut blob = Vec::with_capacity(entry.vector.len() * 4);
            for value in &entry.vector {
                blob.extend_from_slice(&value.to_le_bytes());
            }
            tx.execute(
                "INSERT INTO chunk_vectors (vector_index_variant_id, chunk_id, ordinal, vector) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(vector_index_variant_id, chunk_id) DO UPDATE SET ordinal=?3, vector=?4",
                params![variant_id, entry.chunk_id, entry.ordinal as i64, blob],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    /// 完整性校验（条数 + ordinal 去重计数一致且非空）后置 ready。
    pub fn complete_build(
        &self,
        variant_id: &str,
        expected_count: usize,
    ) -> Result<(), KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunk_vectors WHERE vector_index_variant_id=?1",
                params![variant_id],
                |row| row.get(0),
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let distinct_ordinals: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT ordinal) FROM chunk_vectors WHERE vector_index_variant_id=?1",
                params![variant_id],
                |row| row.get(0),
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        if count != expected_count as i64
            || distinct_ordinals != expected_count as i64
            || count == 0
        {
            return Err(KnowledgeError::index_invalid(
                "vector build completeness check failed; variant stays building",
            ));
        }
        conn.execute(
            "UPDATE vector_index_variants SET status='ready' WHERE id=?1",
            params![variant_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    pub fn fail_build(&self, variant_id: &str) -> Result<(), KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.execute(
            "UPDATE vector_index_variants SET status='failed' WHERE id=?1",
            params![variant_id],
        )
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(())
    }

    fn map_variant(row: &rusqlite::Row<'_>) -> rusqlite::Result<VectorVariantRecord> {
        Ok(VectorVariantRecord {
            variant_id: row.get(0)?,
            chunk_index_variant_id: row.get(1)?,
            parse_artifact_id: row.get(2)?,
            model_key: row.get(3)?,
            chunk_fingerprint: row.get(4)?,
            dimensions: row.get::<_, i64>(5)? as u32,
            status: row.get(6)?,
        })
    }

    pub fn get_variant(
        &self,
        variant_id: &str,
    ) -> Result<Option<VectorVariantRecord>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        conn.query_row(
            "SELECT id, chunk_index_variant_id, parse_artifact_id, model_key, chunk_fingerprint, dimensions, status FROM vector_index_variants WHERE id=?1",
            params![variant_id],
            Self::map_variant,
        )
        .optional()
        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    pub fn list_variants_by_artifact(
        &self,
        parse_artifact_id: &str,
    ) -> Result<Vec<VectorVariantRecord>, KnowledgeError> {
        let conn = lock_conn(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, chunk_index_variant_id, parse_artifact_id, model_key, chunk_fingerprint, dimensions, status FROM vector_index_variants WHERE parse_artifact_id=?1 ORDER BY id",
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let rows = stmt
            .query_map(params![parse_artifact_id], Self::map_variant)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))
    }

    /// 检索：限定变体集合（viv 身份即来源/分块/模型三维身份）；查询向量维度
    /// 与变体不一致 → 显式拒绝（绝不跨维度余弦，绝不返回貌似正常的结果）。
    pub fn search(
        &self,
        variant_ids: &[String],
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<VectorHit>, KnowledgeError> {
        if variant_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut hits: Vec<VectorHit> = Vec::new();
        for variant_id in variant_ids {
            let Some(variant) = self.get_variant(variant_id)? else {
                continue;
            };
            if variant.status != "ready" {
                continue;
            }
            if variant.dimensions as usize != query.len() {
                return Err(KnowledgeError::retrieval_unavailable(
                    "query embedding dimensions do not match the indexed variant",
                ));
            }
            let conn = lock_conn(&self.conn);
            let mut stmt = conn
                .prepare(
                    "SELECT chunk_id, vector FROM chunk_vectors WHERE vector_index_variant_id=?1",
                )
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            let rows = stmt
                .query_map(params![variant_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            for row in rows {
                let (chunk_id, blob) =
                    row.map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
                if blob.len() != variant.dimensions as usize * 4 {
                    return Err(KnowledgeError::index_invalid(
                        "stored vector has a corrupted length",
                    ));
                }
                let mut vector = Vec::with_capacity(variant.dimensions as usize);
                for chunk in blob.as_chunks::<4>().0 {
                    vector.push(f32::from_le_bytes(*chunk));
                }
                let score = lingxi_kernel::knowledge::cosine(query, &vector)?;
                hits.push(VectorHit {
                    variant_id: variant_id.clone(),
                    chunk_id,
                    parse_artifact_id: variant.parse_artifact_id.clone(),
                    score,
                });
            }
        }
        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    /// delete wins / GC：清 artifact 的全部向量变体。
    pub fn remove_artifact(&self, parse_artifact_id: &str) -> Result<usize, KnowledgeError> {
        let variants = self.list_variants_by_artifact(parse_artifact_id)?;
        let mut conn = lock_conn(&self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        let mut removed = 0;
        for variant in &variants {
            tx.execute(
                "DELETE FROM chunk_vectors WHERE vector_index_variant_id=?1",
                params![variant.variant_id],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            tx.execute(
                "DELETE FROM vector_index_variants WHERE id=?1",
                params![variant.variant_id],
            )
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
            removed += 1;
        }
        tx.commit()
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        Ok(removed)
    }
}
