import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { createRequire } from "node:module";

import { KnowledgeError } from "./errors.ts";
import {
  KNOWLEDGE_CHUNK_TARGET_CHARS,
  MAX_KNOWLEDGE_CHUNK_TARGET_CHARS,
  MIN_KNOWLEDGE_CHUNK_TARGET_CHARS,
  computeAutoChunkTargetChars,
} from "./chunker.ts";
import type {
  ContentSnapshot,
  ImportedKnowledgeSource,
  IngestionJob,
  IngestionJobStatus,
  IngestionPhase,
  KnowledgeBlock,
  KnowledgeCitation,
  KnowledgeModelRef,
  KnowledgeNotebook,
  KnowledgeParseArtifact,
  KnowledgeParseStatus,
  KnowledgeSource,
  KnowledgeSourceType,
  NotebookConfig,
  NotebookSourceMembership,
  ResolvedKnowledgeCitation,
} from "./types.ts";
import type { KnowledgeBlockDraft } from "./source-adapters.ts";

export const KNOWLEDGE_SCHEMA_VERSION = 9;

const SOURCE_TYPES = new Set<KnowledgeSourceType>(["file", "pasted_text", "web_snapshot"]);
const PARSE_STATUSES = new Set<KnowledgeParseStatus>(["parsing", "ready", "needs_ocr", "failed"]);
const INGESTION_PHASES = new Set<IngestionPhase>(["parse", "chunk", "fts_index", "embed", "done"]);
const INGESTION_STATUSES = new Set<IngestionJobStatus>(["queued", "running", "pending_embedding", "failed", "done"]);

/**
 * retrieval_top_k 的 sanity 边界。v8 起笔记本列 NULL = 无上限（返回全部
 * 匹配块）；"无上限"在检索核心的物理边界就是 MAX（防病态全表膨胀）。
 * KNOWLEDGE_DEFAULT_RETRIEVAL_TOP_K 已随 v8 语义反转退役，保留导出仅供
 * 旧测试/文档理解 v7 及以前的行为（NULL 当时回退 12）。
 */
export const KNOWLEDGE_DEFAULT_RETRIEVAL_TOP_K = 12;
const MIN_RETRIEVAL_TOP_K = 1;
export const MAX_RETRIEVAL_TOP_K = 1000;
/** 向量保留天数边界：1 天 ~ 10 年。 */
export const MIN_VECTOR_RETENTION_DAYS = 1;
export const MAX_VECTOR_RETENTION_DAYS = 3650;
const require = createRequire(import.meta.url);
let BetterSqliteDatabase: any = null;

function loadDatabase() {
  if (!BetterSqliteDatabase) {
    const mod = require("better-sqlite3");
    BetterSqliteDatabase = mod?.default || mod;
  }
  return BetterSqliteDatabase;
}

function requiredString(value: unknown, field: string, maxLength: number): string {
  if (typeof value !== "string") {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be a string`);
  }
  const normalized = value.trim();
  if (!normalized) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must not be empty`);
  }
  if (normalized.length > maxLength) {
    throw new KnowledgeError(
      "KNOWLEDGE_INVALID_ARGUMENT",
      `${field} exceeds the ${maxLength} character limit`,
      { field, maxLength },
    );
  }
  return normalized;
}

function parseObjectJson(value: unknown, field: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(typeof value === "string" ? value : "");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("not an object");
    return parsed;
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
}

function serializeObjectJson(value: unknown, field: string): string {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be an object`);
  }
  const serialized = JSON.stringify(value);
  if (Buffer.byteLength(serialized, "utf8") > 64 * 1024) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is too large`);
  }
  return serialized;
}

function parseStringArrayJson(value: unknown, field: string): string[] {
  try {
    const parsed = JSON.parse(typeof value === "string" ? value : "");
    if (!Array.isArray(parsed) || parsed.some((entry) => typeof entry !== "string")) {
      throw new Error("not a string array");
    }
    return parsed;
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
}

function serializeStringArray(value: unknown, field: string): string {
  if (!Array.isArray(value) || value.some((entry) => typeof entry !== "string" || entry.length > 256)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be a string array`);
  }
  return JSON.stringify(value);
}

function storagePath(value: unknown): string {
  const normalized = requiredString(value, "storagePath", 1024);
  if (
    path.isAbsolute(normalized)
    || normalized.includes("\\")
    || normalized.split("/").some((segment) => segment === ".." || segment === "")
  ) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "storagePath must be a safe relative path");
  }
  return normalized;
}

function sourceType(value: unknown): KnowledgeSourceType {
  if (typeof value !== "string" || !SOURCE_TYPES.has(value as KnowledgeSourceType)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "sourceType is unsupported");
  }
  return value as KnowledgeSourceType;
}

function sha256(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/u.test(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "sha256 must be 64 lowercase hex characters");
  }
  return value;
}

function byteSize(value: unknown): number {
  if (!Number.isSafeInteger(value) || Number(value) < 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "byteSize must be a non-negative integer");
  }
  return Number(value);
}

function toNotebook(row: any): KnowledgeNotebook | null {
  if (!row) return null;
  return {
    id: row.id,
    studioId: row.studio_id,
    name: row.name,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    deletedAt: row.deleted_at || null,
  };
}

function toSource(row: any): KnowledgeSource | null {
  if (!row) return null;
  return {
    id: row.id,
    studioId: row.studio_id,
    sourceType: row.source_type,
    displayName: row.display_name,
    originMetadata: parseObjectJson(row.origin_metadata_json, "origin metadata"),
    createdAt: row.created_at,
    deletedAt: row.deleted_at || null,
  };
}

function toMembership(row: any): NotebookSourceMembership | null {
  if (!row) return null;
  return {
    notebookId: row.notebook_id,
    sourceId: row.source_id,
    addedAt: row.added_at,
    removedAt: row.removed_at || null,
  };
}

function toSnapshot(row: any): ContentSnapshot | null {
  if (!row) return null;
  return {
    id: row.id,
    sourceId: row.source_id,
    sha256: row.sha256,
    mimeType: row.mime_type,
    byteSize: Number(row.byte_size),
    storagePath: row.storage_path,
    capturedAt: row.captured_at,
  };
}

function toParseArtifact(row: any): KnowledgeParseArtifact | null {
  if (!row) return null;
  return {
    id: row.id,
    contentSnapshotId: row.content_snapshot_id,
    parserId: row.parser_id,
    parserVersion: row.parser_version,
    parserConfigHash: row.parser_config_hash,
    status: row.status,
    warnings: parseStringArrayJson(row.warnings_json, "parse warnings"),
    semanticArtifactPath: row.semantic_artifact_path || null,
    createdAt: row.created_at,
    completedAt: row.completed_at || null,
  };
}

function toBlock(row: any): KnowledgeBlock | null {
  if (!row) return null;
  return {
    id: row.id,
    parseArtifactId: row.parse_artifact_id,
    ordinal: Number(row.ordinal),
    text: row.text,
    textSha256: row.text_sha256,
    locatorType: row.locator_type,
    locator: parseObjectJson(row.locator_payload_json, "block locator"),
  };
}

function toCitation(row: any): KnowledgeCitation | null {
  if (!row) return null;
  return {
    id: row.id,
    parseArtifactId: row.parse_artifact_id,
    blockId: row.block_id,
    startOffset: Number(row.start_offset),
    endOffset: Number(row.end_offset),
    canonicalText: row.canonical_text,
    canonicalTextSha256: row.canonical_text_sha256,
    createdAt: row.created_at,
  };
}

/** 与 shared/model-ref.ts 的持久化纪律一致：完整 {id, provider}，不做按 id 降级。 */
function serializeModelRef(value: unknown, field: string): string {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be an object`);
  }
  const ref = value as Record<string, unknown>;
  const id = requiredString(ref.id, `${field}.id`, 256);
  const provider = requiredString(ref.provider, `${field}.provider`, 256);
  return JSON.stringify({ id, provider });
}

function parseModelRefJson(value: unknown, field: string): KnowledgeModelRef | null {
  if (value == null) return null;
  const parsed = parseObjectJson(value, field);
  if (
    typeof parsed.id !== "string" || !parsed.id
    || typeof parsed.provider !== "string" || !parsed.provider
  ) {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
  return { id: parsed.id, provider: parsed.provider };
}

function optionalIntegerInRange(
  value: unknown,
  field: string,
  min: number,
  max: number,
): number | null {
  if (value == null) return null;
  if (!Number.isSafeInteger(value) || Number(value) < min || Number(value) > max) {
    throw new KnowledgeError(
      "KNOWLEDGE_INVALID_ARGUMENT",
      `${field} must be an integer between ${min} and ${max}`,
      { field, min, max },
    );
  }
  return Number(value);
}

function chunkerConfigId(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{16}$/u.test(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "chunkerConfigId must be 16 lowercase hex characters");
  }
  return value;
}

function isoTimestampOrNull(value: unknown, field: string): string | null {
  if (value == null) return null;
  if (typeof value !== "string" || Number.isNaN(Date.parse(value))) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be an ISO timestamp`);
  }
  return value;
}

function toNotebookConfig(row: any): NotebookConfig {
  return {
    embeddingModelRef: parseModelRefJson(row?.embedding_model_ref, "embedding model ref"),
    rerankModelRef: parseModelRefJson(row?.rerank_model_ref, "rerank model ref"),
    chunkTargetChars: row?.chunk_target_chars == null ? null : Number(row.chunk_target_chars),
    retrievalTopK: row?.retrieval_top_k == null ? null : Number(row.retrieval_top_k),
    vectorRetentionDays: row?.vector_retention_days == null ? null : Number(row.vector_retention_days),
  };
}

function toIngestionJob(row: any): IngestionJob | null {
  if (!row) return null;
  if (!INGESTION_PHASES.has(row.phase) || !INGESTION_STATUSES.has(row.status)) {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Ingestion job state is invalid");
  }
  return {
    id: row.id,
    notebookId: row.notebook_id,
    sourceId: row.source_id,
    artifactId: row.artifact_id || null,
    phase: row.phase,
    status: row.status,
    attempt: Number(row.attempt),
    retryAfter: row.retry_after || null,
    error: row.error || null,
    chunkerConfigId: row.chunker_config_id,
    progressDone: Number(row.progress_done ?? 0),
    progressTotal: row.progress_total == null ? null : Number(row.progress_total),
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export interface ResolvedNotebookConfig {
  embeddingModelRef: KnowledgeModelRef | null;
  rerankModelRef: KnowledgeModelRef | null;
  /** null = 自动（按嵌入模型上下文 ×80% 派生，摄入时计算）；遗留显式值仍生效。 */
  chunkTargetChars: number | null;
  /** null = 无上限（返回全部匹配块）；正整数 = 最大召回数。 */
  retrievalTopK: number | null;
  /** null = 永久保留（默认）；正整数 = 旧版本向量 N 天未被查询命中即回收。 */
  vectorRetentionDays: number | null;
}

/**
 * 笔记本配置解析（v8 起）：仅笔记本列，无全局偏好级。模型引用未配置返回
 * null：摄入落 pending_embedding，查询走纯 FTS 并显式标注 retrievalMode="fts"
 * （禁静默降级，不偷换其他模型）。数值项 NULL = 自动/无上限语义。
 */
export function resolveNotebookConfig(config: NotebookConfig): ResolvedNotebookConfig {
  return {
    embeddingModelRef: config.embeddingModelRef ?? null,
    rerankModelRef: config.rerankModelRef ?? null,
    chunkTargetChars: config.chunkTargetChars ?? null,
    retrievalTopK: config.retrievalTopK ?? null,
    vectorRetentionDays: config.vectorRetentionDays ?? null,
  };
}

/**
 * 生效分块尺寸：显式列 > 嵌入模型上下文 ×80% 自动值（computeAutoChunkTargetChars）。
 * 摄入侧与查询侧懒构建必须经同一函数解析——两侧各自解析曾是查询侧按默认 1200
 * 重建索引、与摄入侧指纹互相打架的根因（configId 进 chunk id，尺寸不同即全量重建+重嵌）。
 */
export function resolveEffectiveChunkTargetChars(
  resolved: Pick<ResolvedNotebookConfig, "chunkTargetChars" | "embeddingModelRef">,
  getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null,
): number {
  return resolved.chunkTargetChars
    ?? computeAutoChunkTargetChars(
      resolved.embeddingModelRef
        ? getEmbeddingModelContextWindow?.(resolved.embeddingModelRef) ?? null
        : null,
    );
}

export interface KnowledgeStoreOptions {
  dbPath: string;
  Database?: any;
  now?: () => string;
  idGenerator?: (prefix: string) => string;
}

/** Knowledge 领域事实库；索引和大文件字节不写入这里。 */
export class KnowledgeStore {
  declare db: any;
  private readonly now: () => string;
  private readonly idGenerator: (prefix: string) => string;

  constructor(options: KnowledgeStoreOptions) {
    if (!options?.dbPath || !path.isAbsolute(options.dbPath)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "KnowledgeStore requires an absolute dbPath");
    }
    fs.mkdirSync(path.dirname(options.dbPath), { recursive: true, mode: 0o700 });
    const Database = options.Database || loadDatabase();
    this.db = new Database(options.dbPath);
    this.now = options.now || (() => new Date().toISOString());
    this.idGenerator = options.idGenerator || ((prefix) => `${prefix}_${crypto.randomUUID()}`);

    try {
      this.db.pragma("journal_mode = WAL");
      this.db.pragma("synchronous = NORMAL");
      this.db.pragma("foreign_keys = ON");
      this.db.pragma("busy_timeout = 5000");
      this.migrate();
    } catch (error) {
      try {
        this.db.close();
      } catch {
        // 保留原始建库错误。
      }
      this.db = null;
      throw error;
    }
  }

  private migrate() {
    const current = Number(this.db.pragma("user_version", { simple: true }));
    if (!Number.isSafeInteger(current) || current < 0) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Knowledge database schema version is invalid");
    }
    if (current > KNOWLEDGE_SCHEMA_VERSION) {
      throw new KnowledgeError(
        "KNOWLEDGE_SCHEMA_NEWER",
        "Knowledge database uses a newer schema",
        { supportedVersion: KNOWLEDGE_SCHEMA_VERSION },
      );
    }
    if (current === KNOWLEDGE_SCHEMA_VERSION) return;

    this.db.transaction(() => {
      let version = current;
      while (version < KNOWLEDGE_SCHEMA_VERSION) {
        if (version === 0) this.createSchemaV1();
        if (version === 1) this.createSchemaV2();
        if (version === 2) this.createSchemaV3();
        if (version === 3) this.createSchemaV4();
        if (version === 4) this.createSchemaV5();
        if (version === 5) this.createSchemaV6();
        if (version === 6) this.createSchemaV7();
        if (version === 7) this.createSchemaV8();
        if (version === 8) this.createSchemaV9();
        version += 1;
      }
      this.db.pragma(`user_version = ${KNOWLEDGE_SCHEMA_VERSION}`);
    })();
  }

  private createSchemaV1() {
    this.db.exec(`
      CREATE TABLE notebooks (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        name TEXT NOT NULL CHECK(length(trim(name)) > 0),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        deleted_at TEXT
      );

      CREATE INDEX idx_notebooks_studio_active
        ON notebooks(studio_id, deleted_at, updated_at DESC);

      CREATE TABLE sources (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        source_type TEXT NOT NULL CHECK(source_type IN ('file', 'pasted_text', 'web_snapshot')),
        display_name TEXT NOT NULL CHECK(length(trim(display_name)) > 0),
        origin_metadata_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        deleted_at TEXT
      );

      CREATE INDEX idx_sources_studio_active
        ON sources(studio_id, deleted_at, created_at DESC);

      CREATE TABLE notebook_sources (
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        added_at TEXT NOT NULL,
        removed_at TEXT,
        PRIMARY KEY(notebook_id, source_id),
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_notebook_sources_active
        ON notebook_sources(notebook_id, removed_at, added_at);
      CREATE INDEX idx_source_notebooks_active
        ON notebook_sources(source_id, removed_at, added_at);

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

      CREATE INDEX idx_content_snapshots_source
        ON content_snapshots(source_id, captured_at DESC);
    `);
  }

  private createSchemaV2() {
    this.db.exec(`
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
        completed_at TEXT,
        UNIQUE(content_snapshot_id, parser_id, parser_version, parser_config_hash),
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_parse_artifacts_snapshot
        ON parse_artifacts(content_snapshot_id, created_at DESC);
      CREATE INDEX idx_parse_artifacts_status
        ON parse_artifacts(status, created_at);

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

      CREATE INDEX idx_knowledge_blocks_artifact
        ON knowledge_blocks(parse_artifact_id, ordinal);

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

      CREATE INDEX idx_knowledge_citations_artifact
        ON knowledge_citations(parse_artifact_id, created_at);
      CREATE INDEX idx_knowledge_citations_block
        ON knowledge_citations(block_id, start_offset, end_offset);
    `);
  }

  private createSchemaV3() {
    this.db.exec(`
      CREATE TABLE scope_snapshots (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        mode TEXT NOT NULL CHECK(mode IN ('quick', 'research')),
        created_at TEXT NOT NULL
      );

      CREATE INDEX idx_scope_snapshots_studio
        ON scope_snapshots(studio_id, created_at DESC);

      CREATE TABLE scope_notebooks (
        scope_snapshot_id TEXT NOT NULL,
        notebook_id TEXT NOT NULL,
        notebook_name TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(scope_snapshot_id, notebook_id),
        UNIQUE(scope_snapshot_id, ordinal),
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT
      );

      CREATE TABLE scope_sources (
        scope_snapshot_id TEXT NOT NULL,
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        source_display_name TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(scope_snapshot_id, notebook_id, source_id),
        UNIQUE(scope_snapshot_id, ordinal),
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_scope_sources_artifact
        ON scope_sources(scope_snapshot_id, parse_artifact_id);
      CREATE INDEX idx_scope_sources_snapshot
        ON scope_sources(content_snapshot_id);

      CREATE TABLE knowledge_runs (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        mode TEXT NOT NULL CHECK(mode IN ('quick', 'research')),
        question TEXT NOT NULL,
        scope_snapshot_id TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'cancelled')),
        retrieval_mode TEXT NOT NULL CHECK(retrieval_mode IN ('fts', 'hybrid')),
        answer_text TEXT,
        error_code TEXT,
        created_at TEXT NOT NULL,
        completed_at TEXT,
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_runs_studio
        ON knowledge_runs(studio_id, created_at DESC);
      CREATE INDEX idx_knowledge_runs_status
        ON knowledge_runs(status, created_at);

      CREATE TABLE knowledge_run_retrievals (
        run_id TEXT NOT NULL,
        rank INTEGER NOT NULL CHECK(rank > 0),
        chunk_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        score REAL NOT NULL,
        PRIMARY KEY(run_id, rank),
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE TABLE knowledge_run_citations (
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        marker INTEGER NOT NULL CHECK(marker > 0),
        citation_id TEXT NOT NULL,
        candidate_ref TEXT NOT NULL,
        PRIMARY KEY(run_id, ordinal),
        UNIQUE(run_id, marker),
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_run_citations_citation
        ON knowledge_run_citations(citation_id);
    `);
  }

  private createSchemaV4() {
    this.db.exec(`
      CREATE TABLE research_runs (
        run_id TEXT PRIMARY KEY,
        host_task_id TEXT NOT NULL UNIQUE,
        state TEXT NOT NULL CHECK(state IN (
          'queued', 'preparing_scope', 'building_manifest', 'scanning',
          'building_claims', 'checking_contradictions', 'synthesizing',
          'completed', 'recovering', 'partial', 'failed', 'canceled'
        )),
        spec_json TEXT NOT NULL,
        error_code TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        completed_at TEXT,
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_runs_state
        ON research_runs(state, updated_at);

      CREATE TABLE analysis_manifests (
        run_id TEXT PRIMARY KEY,
        source_count INTEGER NOT NULL CHECK(source_count >= 0),
        parse_artifact_count INTEGER NOT NULL CHECK(parse_artifact_count >= 0),
        block_count INTEGER NOT NULL CHECK(block_count >= 0),
        unit_count INTEGER NOT NULL CHECK(unit_count > 0),
        primary_char_count INTEGER NOT NULL CHECK(primary_char_count > 0),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE analysis_units (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        priority INTEGER NOT NULL CHECK(priority >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        primary_char_count INTEGER NOT NULL CHECK(primary_char_count > 0),
        context_char_count INTEGER NOT NULL CHECK(context_char_count >= 0),
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, parse_artifact_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_analysis_units_run_status
        ON analysis_units(run_id, status, priority, ordinal);

      CREATE TABLE analysis_unit_spans (
        unit_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('primary', 'context')),
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        block_id TEXT NOT NULL,
        block_ordinal INTEGER NOT NULL CHECK(block_ordinal >= 0),
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        PRIMARY KEY(unit_id, kind, ordinal),
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_analysis_unit_spans_block
        ON analysis_unit_spans(block_id, kind, start_offset, end_offset);

      CREATE TABLE execution_batches (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        estimated_chars INTEGER NOT NULL CHECK(estimated_chars > 0),
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_execution_batches_run_status
        ON execution_batches(run_id, status, ordinal);

      CREATE TABLE execution_batch_units (
        batch_id TEXT NOT NULL,
        unit_id TEXT NOT NULL UNIQUE,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(batch_id, ordinal),
        FOREIGN KEY(batch_id) REFERENCES execution_batches(id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_jobs (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        phase TEXT NOT NULL CHECK(phase IN ('claim_build', 'final_synthesis')),
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        input_refs_json TEXT NOT NULL,
        output_json TEXT,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, phase, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE task_attempts (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        work_type TEXT NOT NULL CHECK(work_type IN ('scan_batch', 'claim_job', 'contradiction_check', 'synthesis_job')),
        work_id TEXT NOT NULL,
        attempt_number INTEGER NOT NULL CHECK(attempt_number > 0),
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'canceled')),
        error_code TEXT,
        output_json TEXT,
        started_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(work_type, work_id, attempt_number),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_task_attempts_run
        ON task_attempts(run_id, work_type, started_at);

      CREATE TABLE analysis_unit_results (
        unit_id TEXT PRIMARY KEY,
        attempt_id TEXT NOT NULL,
        result_json TEXT NOT NULL,
        completed_at TEXT NOT NULL,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(attempt_id) REFERENCES task_attempts(id) ON DELETE RESTRICT
      );

      CREATE TABLE evidence_validations (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        origin_type TEXT NOT NULL CHECK(origin_type IN ('analysis', 'contradiction')),
        origin_id TEXT NOT NULL,
        candidate_ordinal INTEGER NOT NULL CHECK(candidate_ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('validated', 'invalid')),
        reason_code TEXT,
        citation_id TEXT,
        canonical_quote TEXT,
        quote_checksum TEXT,
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        UNIQUE(origin_type, origin_id, candidate_ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_evidence_validations_run
        ON evidence_validations(run_id, status, created_at);

      CREATE TABLE research_evidence (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        validation_id TEXT NOT NULL UNIQUE,
        citation_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        canonical_quote TEXT NOT NULL,
        quote_checksum TEXT NOT NULL CHECK(length(quote_checksum) = 64),
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(validation_id) REFERENCES evidence_validations(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_evidence_run
        ON research_evidence(run_id, created_at);

      CREATE TABLE research_claims (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        origin_job_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        text TEXT NOT NULL,
        support_status TEXT NOT NULL CHECK(support_status IN ('supported', 'partial', 'disputed', 'insufficient')),
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        UNIQUE(origin_job_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(origin_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_claims_run
        ON research_claims(run_id, ordinal);

      CREATE TABLE claim_evidence (
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('supports', 'contradicts', 'context')),
        PRIMARY KEY(claim_id, evidence_id, relation),
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE TABLE claim_packs (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        claim_ids_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE contradiction_manifests (
        run_id TEXT PRIMARY KEY,
        unit_count INTEGER NOT NULL CHECK(unit_count >= 0),
        claim_pack_count INTEGER NOT NULL CHECK(claim_pack_count >= 0),
        total_check_count INTEGER NOT NULL CHECK(total_check_count >= 0),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE contradiction_checks (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        claim_pack_id TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        attempt_id TEXT,
        result_json TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, unit_id, claim_pack_id),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_pack_id) REFERENCES claim_packs(id) ON DELETE RESTRICT,
        FOREIGN KEY(attempt_id) REFERENCES task_attempts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_contradiction_checks_run_status
        ON contradiction_checks(run_id, status, unit_id);

      CREATE TABLE research_contradictions (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        check_id TEXT NOT NULL,
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('contradicts', 'context')),
        explanation TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(check_id, claim_id, evidence_id),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(check_id) REFERENCES contradiction_checks(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_reports (
        run_id TEXT PRIMARY KEY,
        synthesis_job_id TEXT NOT NULL UNIQUE,
        report_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(synthesis_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_report_citations (
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        marker INTEGER NOT NULL CHECK(marker > 0),
        evidence_id TEXT NOT NULL,
        citation_id TEXT NOT NULL,
        PRIMARY KEY(run_id, ordinal),
        UNIQUE(run_id, marker),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );
    `);
  }

  private createSchemaV5() {
    this.db.exec(`
      CREATE TABLE research_verification_steps (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        trigger_synthesis_job_id TEXT NOT NULL UNIQUE,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        requests_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(trigger_synthesis_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_steps_run
        ON research_verification_steps(run_id, status, ordinal);

      CREATE TABLE research_verification_cells (
        id TEXT PRIMARY KEY,
        step_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        result_json TEXT,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(step_id, unit_id),
        UNIQUE(step_id, ordinal),
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_cells_step
        ON research_verification_cells(step_id, status, ordinal);

      CREATE TABLE research_verification_attempts (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        step_id TEXT NOT NULL,
        cell_id TEXT NOT NULL,
        attempt_number INTEGER NOT NULL CHECK(attempt_number > 0),
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'canceled')),
        error_code TEXT,
        output_json TEXT,
        started_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(cell_id, attempt_number),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(cell_id) REFERENCES research_verification_cells(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_attempts_run
        ON research_verification_attempts(run_id, started_at);

      CREATE TABLE research_verification_relations (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        step_id TEXT NOT NULL,
        cell_id TEXT NOT NULL,
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('supports', 'contradicts', 'context')),
        explanation TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(cell_id, claim_id, evidence_id, relation),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(cell_id) REFERENCES research_verification_cells(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_relations_run
        ON research_verification_relations(run_id, relation, created_at);
    `);
  }

  private createSchemaV6() {
    // 笔记本级配置：模型引用存 {id, provider} JSON，NULL = 继承全局偏好；
    // 数值列带默认值（旧行回填 1200/12），显式置 NULL = 回退内置默认。
    this.db.exec(`
      ALTER TABLE notebooks ADD COLUMN embedding_model_ref TEXT;
      ALTER TABLE notebooks ADD COLUMN rerank_model_ref TEXT;
      ALTER TABLE notebooks ADD COLUMN chunk_target_chars INTEGER DEFAULT 1200
        CHECK(chunk_target_chars IS NULL OR (chunk_target_chars >= 100 AND chunk_target_chars <= 100000));
      ALTER TABLE notebooks ADD COLUMN retrieval_top_k INTEGER DEFAULT 12
        CHECK(retrieval_top_k IS NULL OR (retrieval_top_k >= 1 AND retrieval_top_k <= 1000));

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
        updated_at TEXT NOT NULL,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_ingestion_jobs_status
        ON ingestion_jobs(status, retry_after, created_at);
      CREATE INDEX idx_ingestion_jobs_source
        ON ingestion_jobs(source_id, created_at DESC);
      CREATE INDEX idx_ingestion_jobs_notebook
        ON ingestion_jobs(notebook_id, created_at DESC);
    `);

    // 提问功能已删（Phase 1），V3-V5 研究表零读路径，全部是从 V1-V2 核心表
    // 派生的产物，与 v6 新列同事务 DROP。按 子表 → 父表 顺序，避免 foreign_keys
    // 开启时 DROP 父表的隐式 DELETE 触发子行 FK 检查；IF EXISTS 容忍残缺的旧库。
    this.db.exec(`
      DROP TABLE IF EXISTS research_verification_relations;
      DROP TABLE IF EXISTS research_verification_attempts;
      DROP TABLE IF EXISTS research_verification_cells;
      DROP TABLE IF EXISTS research_verification_steps;
      DROP TABLE IF EXISTS research_report_citations;
      DROP TABLE IF EXISTS research_reports;
      DROP TABLE IF EXISTS research_contradictions;
      DROP TABLE IF EXISTS contradiction_checks;
      DROP TABLE IF EXISTS contradiction_manifests;
      DROP TABLE IF EXISTS claim_packs;
      DROP TABLE IF EXISTS claim_evidence;
      DROP TABLE IF EXISTS research_claims;
      DROP TABLE IF EXISTS research_evidence;
      DROP TABLE IF EXISTS evidence_validations;
      DROP TABLE IF EXISTS analysis_unit_results;
      DROP TABLE IF EXISTS task_attempts;
      DROP TABLE IF EXISTS research_jobs;
      DROP TABLE IF EXISTS execution_batch_units;
      DROP TABLE IF EXISTS execution_batches;
      DROP TABLE IF EXISTS analysis_unit_spans;
      DROP TABLE IF EXISTS analysis_units;
      DROP TABLE IF EXISTS analysis_manifests;
      DROP TABLE IF EXISTS research_runs;
      DROP TABLE IF EXISTS knowledge_run_citations;
      DROP TABLE IF EXISTS knowledge_run_retrievals;
      DROP TABLE IF EXISTS knowledge_runs;
      DROP TABLE IF EXISTS scope_sources;
      DROP TABLE IF EXISTS scope_notebooks;
      DROP TABLE IF EXISTS scope_snapshots;
    `);
  }

  private createSchemaV7() {
    // 嵌入进度列：done 从 0 递增（每批嵌入后由摄入 worker 落库）；
    // total NULL = 尚未进入 embed 相位（parse/chunk/fts_index 阶段无进度语义）。
    this.db.exec(`
      ALTER TABLE ingestion_jobs ADD COLUMN progress_done INTEGER NOT NULL DEFAULT 0;
      ALTER TABLE ingestion_jobs ADD COLUMN progress_total INTEGER;
    `);
  }

  private createSchemaV8() {
    // retrieval_top_k 语义反转：NULL = 无上限（新默认，返回全部匹配块）。
    // v6 的 ADD COLUMN … DEFAULT 12 把存量行回填成显式 12，与用户手填的 12
    // 不可区分——统一清 NULL，让所有笔记本从新的"无上限"默认起步；此前显式
    // 设置过召回数的笔记本同样回到无上限（符合"默认无上限"的需求方向，用户
    // 可在设置里重新指定最大召回数）。纯数据迁移，无 DDL 结构变更。
    this.db.exec(`
      UPDATE notebooks SET retrieval_top_k = NULL WHERE deleted_at IS NULL;
    `);
  }

  private createSchemaV9() {
    // 向量保留策略：NULL = 永久保留（默认）；正整数 = 旧版本向量超过 N 天
    // 未被查询命中即由 sweep 回收（换模型/重嵌产生的作废向量不再无限叠加）。
    // 列存在检查：测试压版本重开时 ALTER 会对已存在列报 duplicate，需幂等。
    const columns = this.db.prepare(`PRAGMA table_info(notebooks)`).all() as any[];
    if (columns.some((col) => col.name === "vector_retention_days")) return;
    this.db.exec(`
      ALTER TABLE notebooks ADD COLUMN vector_retention_days INTEGER;
    `);
  }

  private newId(prefix: string): string {
    return requiredString(this.idGenerator(prefix), `${prefix} id`, 128);
  }

  private activeNotebook(studioId: string, notebookId: string): KnowledgeNotebook {
    const row = this.db.prepare(`
      SELECT * FROM notebooks
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).get(notebookId, studioId);
    const notebook = toNotebook(row);
    if (!notebook) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook not found");
    return notebook;
  }

  private activeSource(studioId: string, sourceId: string): KnowledgeSource {
    const row = this.db.prepare(`
      SELECT * FROM sources
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).get(sourceId, studioId);
    const source = toSource(row);
    if (!source) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge source not found");
    return source;
  }

  createNotebook(input: { studioId: unknown; name: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const name = requiredString(input?.name, "name", 120);
    const id = this.newId("nb");
    const now = this.now();
    // v8 起显式写 NULL：retrieval_top_k/chunk_target_chars 列的 DDL DEFAULT
    // （12/1200）是 v6 遗留，新笔记本必须以"无上限/自动"起步而非撞上旧默认。
    this.db.prepare(`
      INSERT INTO notebooks (id, studio_id, name, created_at, updated_at, deleted_at, retrieval_top_k, chunk_target_chars)
      VALUES (?, ?, ?, ?, ?, NULL, NULL, NULL)
    `).run(id, studioId, name, now, now);
    return this.getNotebook({ studioId, notebookId: id });
  }

  listNotebooks(input: { studioId: unknown }): KnowledgeNotebook[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    return this.db.prepare(`
      SELECT * FROM notebooks
      WHERE studio_id = ? AND deleted_at IS NULL
      ORDER BY updated_at DESC, id ASC
    `).all(studioId).map(toNotebook);
  }

  getNotebook(input: { studioId: unknown; notebookId: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    return this.activeNotebook(studioId, notebookId);
  }

  renameNotebook(input: { studioId: unknown; notebookId: unknown; name: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const name = requiredString(input?.name, "name", 120);
    this.activeNotebook(studioId, notebookId);
    this.db.prepare(`
      UPDATE notebooks SET name = ?, updated_at = ?
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).run(name, this.now(), notebookId, studioId);
    return this.activeNotebook(studioId, notebookId);
  }

  deleteNotebook(input: { studioId: unknown; notebookId: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const notebook = this.activeNotebook(studioId, notebookId);
    const deletedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        UPDATE notebooks SET deleted_at = ?, updated_at = ?
        WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
      `).run(deletedAt, deletedAt, notebookId, studioId);
      this.db.prepare(`
        UPDATE notebook_sources SET removed_at = ?
        WHERE notebook_id = ? AND removed_at IS NULL
      `).run(deletedAt, notebookId);
    })();
    return { ...notebook, updatedAt: deletedAt, deletedAt };
  }

  getNotebookConfig(input: { studioId: unknown; notebookId: unknown }): NotebookConfig {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    return toNotebookConfig(this.db.prepare(`
      SELECT embedding_model_ref, rerank_model_ref, chunk_target_chars, retrieval_top_k, vector_retention_days
      FROM notebooks
      WHERE id = ? AND studio_id = ?
    `).get(notebookId, studioId));
  }

  /**
   * 源的全部解析产物 id（含历史版本）：派生索引清理用。
   * 软删除源同样返回——索引清理正是删除语义的一部分。
   */
  listSourceArtifactIds(input: { sourceId: unknown }): string[] {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return this.db.prepare(`
      SELECT pa.id FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      WHERE cs.source_id = ?
      ORDER BY pa.created_at
    `).all(sourceId).map((row: any) => row.id);
  }

  /**
   * 仍活跃挂靠该源的笔记本 id（membership 未移除且笔记本未删除）。
   * 空数组 = 孤儿源：没有任何笔记本可达，派生索引可清理。
   */
  listActiveNotebookIdsForSource(input: { sourceId: unknown }): string[] {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return this.db.prepare(`
      SELECT ns.notebook_id FROM notebook_sources ns
      JOIN notebooks n ON n.id = ns.notebook_id
      WHERE ns.source_id = ? AND ns.removed_at IS NULL AND n.deleted_at IS NULL
    `).all(sourceId).map((row: any) => row.notebook_id);
  }

  /**
   * 孤儿源的全部解析产物 id：源本身未删（快照/产物仍活跃）但已无任何
   * 活跃挂靠笔记本（UI 不可达）。历史删除动作发生在删除清理逻辑上线前的
   * 残留由 sweep 兜底回收；新删除已由 manager 即时清理，通常为空。
   */
  listOrphanArtifactIds(): string[] {
    return this.db.prepare(`
      SELECT pa.id FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id AND s.deleted_at IS NULL
      WHERE NOT EXISTS (
        SELECT 1 FROM notebook_sources ns
        JOIN notebooks n ON n.id = ns.notebook_id AND n.deleted_at IS NULL
        WHERE ns.source_id = s.id AND ns.removed_at IS NULL
      )
    `).all().map((row: any) => row.id);
  }

  /**
   * 向量 sweep 的归属视图：全部活跃源的解析产物 × 是否该源最新产物 ×
   * 活跃挂靠笔记本的保留策略（取最宽松的最大值；任一笔记本未配置 = 该源
   * 永久保留，不误删仍被引用的向量）。历史产物（isLatestForSource=false）
   * 与同产物非当前模型身份的向量是"旧版本"回收候选。
   */
  listArtifactVectorSweepRows(): Array<{
    artifactId: string;
    sourceId: string;
    isLatestForSource: boolean;
    retentionDays: number | null;
  }> {
    const retentionBySource = new Map<string, number | null>();
    for (const row of this.db.prepare(`
      SELECT ns.source_id,
             MAX(n.vector_retention_days) AS max_days,
             COUNT(n.vector_retention_days) AS configured_count,
             COUNT(*) AS total_count
      FROM notebook_sources ns
      JOIN notebooks n ON n.id = ns.notebook_id
      WHERE ns.removed_at IS NULL AND n.deleted_at IS NULL
      GROUP BY ns.source_id
    `).all() as any[]) {
      // 任一挂靠笔记本未配置保留策略 = 该源永久保留。
      retentionBySource.set(
        row.source_id,
        Number(row.configured_count) < Number(row.total_count)
          ? null
          : row.max_days == null ? null : Number(row.max_days),
      );
    }
    const latestBySource = new Map<string, string>();
    const artifacts: Array<{ sourceId: string; artifactId: string }> = this.db.prepare(`
      SELECT cs.source_id, pa.id AS artifact_id
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id AND s.deleted_at IS NULL
      ORDER BY pa.created_at
    `).all().map((row: any) => ({ sourceId: row.source_id, artifactId: row.artifact_id }));
    for (const { sourceId, artifactId } of artifacts) {
      latestBySource.set(sourceId, artifactId);
    }
    return artifacts.map(({ sourceId, artifactId }) => ({
      artifactId,
      sourceId,
      isLatestForSource: latestBySource.get(sourceId) === artifactId,
      retentionDays: retentionBySource.get(sourceId) ?? null,
    }));
  }

  /**
   * 笔记本配置部分更新：字段 omitted → 不变；null → 清除为 NULL
   * （模型引用回 NULL = 未配置，数值回 NULL = 自动分块/无上限召回）；
   * 否则校验后写入。至少给一个字段，避免空调用被静默接受。
   */
  updateNotebookConfig(input: {
    studioId: unknown;
    notebookId: unknown;
    embeddingModelRef?: unknown;
    rerankModelRef?: unknown;
    chunkTargetChars?: unknown;
    retrievalTopK?: unknown;
    vectorRetentionDays?: unknown;
  }): NotebookConfig {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    const assignments: string[] = [];
    const params: unknown[] = [];
    const hasField = (field: string) => Object.prototype.hasOwnProperty.call(input ?? {}, field);
    if (hasField("embeddingModelRef")) {
      assignments.push("embedding_model_ref = ?");
      params.push(input.embeddingModelRef == null
        ? null
        : serializeModelRef(input.embeddingModelRef, "embeddingModelRef"));
    }
    if (hasField("rerankModelRef")) {
      assignments.push("rerank_model_ref = ?");
      params.push(input.rerankModelRef == null
        ? null
        : serializeModelRef(input.rerankModelRef, "rerankModelRef"));
    }
    if (hasField("chunkTargetChars")) {
      assignments.push("chunk_target_chars = ?");
      params.push(optionalIntegerInRange(
        input.chunkTargetChars,
        "chunkTargetChars",
        MIN_KNOWLEDGE_CHUNK_TARGET_CHARS,
        MAX_KNOWLEDGE_CHUNK_TARGET_CHARS,
      ));
    }
    if (hasField("retrievalTopK")) {
      assignments.push("retrieval_top_k = ?");
      params.push(optionalIntegerInRange(
        input.retrievalTopK,
        "retrievalTopK",
        MIN_RETRIEVAL_TOP_K,
        MAX_RETRIEVAL_TOP_K,
      ));
    }
    if (hasField("vectorRetentionDays")) {
      assignments.push("vector_retention_days = ?");
      params.push(optionalIntegerInRange(
        input.vectorRetentionDays,
        "vectorRetentionDays",
        MIN_VECTOR_RETENTION_DAYS,
        MAX_VECTOR_RETENTION_DAYS,
      ));
    }
    if (assignments.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Notebook config update requires at least one field");
    }
    this.db.prepare(`
      UPDATE notebooks SET ${assignments.join(", ")}, updated_at = ?
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).run(...params, this.now(), notebookId, studioId);
    return this.getNotebookConfig({ studioId, notebookId });
  }

  /**
   * 一次性迁移（v8 配套）：把已退役的全局嵌入/重排引用写入所有未单独配置
   * 的活跃笔记本（WHERE … IS NULL 保证不覆盖显式配置）。迁移值与旧解析链
   * 会解析出的同一引用，语义零变化，不触发重建。幂等：列已写/无全局值时 0 行。
   */
  migrateLegacyGlobalModelRefs(input: {
    embeddingModelRef: KnowledgeModelRef | null;
    rerankModelRef: KnowledgeModelRef | null;
  }): { notebooksUpdated: number } {
    let notebooksUpdated = 0;
    this.db.transaction(() => {
      if (input.embeddingModelRef) {
        notebooksUpdated += this.db.prepare(`
          UPDATE notebooks SET embedding_model_ref = ?, updated_at = ?
          WHERE embedding_model_ref IS NULL AND deleted_at IS NULL
        `).run(JSON.stringify(input.embeddingModelRef), this.now()).changes;
      }
      if (input.rerankModelRef) {
        notebooksUpdated += this.db.prepare(`
          UPDATE notebooks SET rerank_model_ref = ?, updated_at = ?
          WHERE rerank_model_ref IS NULL AND deleted_at IS NULL
        `).run(JSON.stringify(input.rerankModelRef), this.now()).changes;
      }
    })();
    return { notebooksUpdated };
  }

  createSourceWithSnapshot(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId?: unknown;
    snapshotId?: unknown;
    sourceType: unknown;
    displayName: unknown;
    originMetadata: unknown;
    snapshot: {
      sha256: unknown;
      mimeType: unknown;
      byteSize: unknown;
      storagePath: unknown;
    };
  }): ImportedKnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    const id = input.sourceId == null
      ? this.newId("src")
      : requiredString(input.sourceId, "sourceId", 128);
    const snapshotId = input.snapshotId == null
      ? this.newId("snap")
      : requiredString(input.snapshotId, "snapshotId", 128);
    const normalizedType = sourceType(input.sourceType);
    const displayName = requiredString(input.displayName, "displayName", 255);
    const originMetadataJson = serializeObjectJson(input.originMetadata, "originMetadata");
    const snapshotSha = sha256(input.snapshot?.sha256);
    const mimeType = requiredString(input.snapshot?.mimeType, "mimeType", 255).toLowerCase();
    const snapshotBytes = byteSize(input.snapshot?.byteSize);
    const snapshotStoragePath = storagePath(input.snapshot?.storagePath);
    const now = this.now();

    this.db.transaction(() => {
      this.db.prepare(`
        INSERT INTO sources (
          id, studio_id, source_type, display_name, origin_metadata_json, created_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, NULL)
      `).run(id, studioId, normalizedType, displayName, originMetadataJson, now);
      this.db.prepare(`
        INSERT INTO content_snapshots (
          id, source_id, sha256, mime_type, byte_size, storage_path, captured_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `).run(snapshotId, id, snapshotSha, mimeType, snapshotBytes, snapshotStoragePath, now);
      this.db.prepare(`
        INSERT INTO notebook_sources (notebook_id, source_id, added_at, removed_at)
        VALUES (?, ?, ?, NULL)
      `).run(notebookId, id, now);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();

    return {
      source: this.activeSource(studioId, id),
      snapshot: this.getContentSnapshot({ studioId, snapshotId }),
      membership: this.getMembership(notebookId, id),
    };
  }

  private getMembership(notebookId: string, sourceId: string): NotebookSourceMembership {
    const membership = toMembership(this.db.prepare(`
      SELECT * FROM notebook_sources WHERE notebook_id = ? AND source_id = ?
    `).get(notebookId, sourceId));
    if (!membership) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook source membership not found");
    return membership;
  }

  addSourceToNotebook(input: { studioId: unknown; notebookId: unknown; sourceId: unknown }) {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const existing = toMembership(this.db.prepare(`
      SELECT * FROM notebook_sources WHERE notebook_id = ? AND source_id = ?
    `).get(notebookId, sourceId));
    if (existing && existing.removedAt === null) return existing;

    const now = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        INSERT INTO notebook_sources (notebook_id, source_id, added_at, removed_at)
        VALUES (?, ?, ?, NULL)
        ON CONFLICT(notebook_id, source_id) DO UPDATE SET
          added_at = excluded.added_at,
          removed_at = NULL
      `).run(notebookId, sourceId, now);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();
    return this.getMembership(notebookId, sourceId);
  }

  removeSourceFromNotebook(input: { studioId: unknown; notebookId: unknown; sourceId: unknown }) {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const membership = this.getMembership(notebookId, sourceId);
    if (membership.removedAt !== null) {
      throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook source membership not found");
    }
    const now = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        UPDATE notebook_sources SET removed_at = ?
        WHERE notebook_id = ? AND source_id = ? AND removed_at IS NULL
      `).run(now, notebookId, sourceId);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();
    return { ...membership, removedAt: now };
  }

  listNotebookSources(input: { studioId: unknown; notebookId: unknown }): Array<{
    source: KnowledgeSource;
    snapshot: ContentSnapshot;
    membership: NotebookSourceMembership;
    parseArtifact: KnowledgeParseArtifact | null;
  }> {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    const rows = this.db.prepare(`
      SELECT
        s.*,
        ns.notebook_id AS membership_notebook_id,
        ns.source_id AS membership_source_id,
        ns.added_at AS membership_added_at,
        ns.removed_at AS membership_removed_at,
        cs.id AS snapshot_id,
        cs.sha256 AS snapshot_sha256,
        cs.mime_type AS snapshot_mime_type,
        cs.byte_size AS snapshot_byte_size,
        cs.storage_path AS snapshot_storage_path,
        cs.captured_at AS snapshot_captured_at,
        pa.id AS parse_id,
        pa.content_snapshot_id AS parse_content_snapshot_id,
        pa.parser_id AS parse_parser_id,
        pa.parser_version AS parse_parser_version,
        pa.parser_config_hash AS parse_parser_config_hash,
        pa.status AS parse_status,
        pa.warnings_json AS parse_warnings_json,
        pa.semantic_artifact_path AS parse_semantic_artifact_path,
        pa.created_at AS parse_created_at,
        pa.completed_at AS parse_completed_at
      FROM notebook_sources ns
      JOIN sources s ON s.id = ns.source_id
      JOIN content_snapshots cs ON cs.id = (
        SELECT inner_cs.id FROM content_snapshots inner_cs
        WHERE inner_cs.source_id = s.id
        ORDER BY inner_cs.captured_at DESC, inner_cs.id DESC
        LIMIT 1
      )
      LEFT JOIN parse_artifacts pa ON pa.id = (
        SELECT inner_pa.id FROM parse_artifacts inner_pa
        WHERE inner_pa.content_snapshot_id = cs.id
        ORDER BY inner_pa.created_at DESC, inner_pa.id DESC
        LIMIT 1
      )
      WHERE ns.notebook_id = ?
        AND ns.removed_at IS NULL
        AND s.studio_id = ?
        AND s.deleted_at IS NULL
      ORDER BY ns.added_at ASC, s.id ASC
    `).all(notebookId, studioId);

    return rows.map((row: any) => ({
      source: toSource(row)!,
      membership: {
        notebookId: row.membership_notebook_id,
        sourceId: row.membership_source_id,
        addedAt: row.membership_added_at,
        removedAt: row.membership_removed_at || null,
      },
      snapshot: {
        id: row.snapshot_id,
        sourceId: row.id,
        sha256: row.snapshot_sha256,
        mimeType: row.snapshot_mime_type,
        byteSize: Number(row.snapshot_byte_size),
        storagePath: row.snapshot_storage_path,
        capturedAt: row.snapshot_captured_at,
      },
      parseArtifact: row.parse_id ? {
        id: row.parse_id,
        contentSnapshotId: row.parse_content_snapshot_id,
        parserId: row.parse_parser_id,
        parserVersion: row.parse_parser_version,
        parserConfigHash: row.parse_parser_config_hash,
        status: row.parse_status,
        warnings: parseStringArrayJson(row.parse_warnings_json, "parse warnings"),
        semanticArtifactPath: row.parse_semantic_artifact_path || null,
        createdAt: row.parse_created_at,
        completedAt: row.parse_completed_at || null,
      } : null,
    }));
  }

  /**
   * source-file-watcher 的启动扫描：全部活跃 file 源 × 活跃 membership
   * （一行一条 membership，watcher 按 sourceId 聚合成多笔记本 watch 项）。
   * originMetadata.originalPath 缺失/非绝对路径的行跳过——这类源无法 refresh
   * （refreshFileSource 会抛 KNOWLEDGE_STORAGE_INVALID），watch 无意义。
   */
  listWatchableFileSources(): Array<{
    studioId: string;
    notebookId: string;
    sourceId: string;
    originalPath: string;
  }> {
    const rows = this.db.prepare(`
      SELECT s.id AS source_id, s.studio_id, s.origin_metadata_json, ns.notebook_id
      FROM sources s
      JOIN notebook_sources ns ON ns.source_id = s.id AND ns.removed_at IS NULL
      JOIN notebooks n ON n.id = ns.notebook_id AND n.deleted_at IS NULL
      WHERE s.source_type = 'file' AND s.deleted_at IS NULL
      ORDER BY s.id ASC, ns.notebook_id ASC
    `).all();
    const result: Array<{
      studioId: string;
      notebookId: string;
      sourceId: string;
      originalPath: string;
    }> = [];
    for (const row of rows as any[]) {
      const metadata = parseObjectJson(row.origin_metadata_json, "origin metadata");
      const originalPath = metadata.originalPath;
      if (typeof originalPath !== "string" || !path.isAbsolute(originalPath)) continue;
      result.push({
        studioId: row.studio_id,
        notebookId: row.notebook_id,
        sourceId: row.source_id,
        originalPath,
      });
    }
    return result;
  }

  getContentSnapshot(input: { studioId: unknown; snapshotId: unknown }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const snapshotId = requiredString(input?.snapshotId, "snapshotId", 128);
    const snapshot = toSnapshot(this.db.prepare(`
      SELECT cs.*
      FROM content_snapshots cs
      JOIN sources s ON s.id = cs.source_id
      WHERE cs.id = ? AND s.studio_id = ?
    `).get(snapshotId, studioId));
    if (!snapshot) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Content snapshot not found");
    return snapshot;
  }

  createContentSnapshot(input: {
    studioId: unknown;
    sourceId: unknown;
    snapshotId?: unknown;
    sha256: unknown;
    mimeType: unknown;
    byteSize: unknown;
    storagePath: unknown;
  }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeSource(studioId, sourceId);
    const snapshotSha = sha256(input.sha256);
    const existing = toSnapshot(this.db.prepare(`
      SELECT * FROM content_snapshots WHERE source_id = ? AND sha256 = ?
    `).get(sourceId, snapshotSha));
    if (existing) return existing;
    const snapshotId = input.snapshotId == null
      ? this.newId("snap")
      : requiredString(input.snapshotId, "snapshotId", 128);
    const mimeType = requiredString(input.mimeType, "mimeType", 255).toLowerCase();
    const snapshotBytes = byteSize(input.byteSize);
    const snapshotStoragePath = storagePath(input.storagePath);
    this.db.prepare(`
      INSERT INTO content_snapshots (
        id, source_id, sha256, mime_type, byte_size, storage_path, captured_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?)
    `).run(
      snapshotId,
      sourceId,
      snapshotSha,
      mimeType,
      snapshotBytes,
      snapshotStoragePath,
      this.now(),
    );
    return this.getContentSnapshot({ studioId, snapshotId });
  }

  getSource(input: { studioId: unknown; sourceId: unknown }): KnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return this.activeSource(studioId, sourceId);
  }

  getLatestContentSnapshotForSource(input: { studioId: unknown; sourceId: unknown }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeSource(studioId, sourceId);
    const snapshot = toSnapshot(this.db.prepare(`
      SELECT * FROM content_snapshots
      WHERE source_id = ?
      ORDER BY captured_at DESC, id DESC
      LIMIT 1
    `).get(sourceId));
    if (!snapshot) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Content snapshot not found");
    return snapshot;
  }

  findParseArtifactByIdentity(input: {
    studioId: unknown;
    contentSnapshotId: unknown;
    parserId: unknown;
    parserVersion: unknown;
    parserConfigHash: unknown;
  }): KnowledgeParseArtifact | null {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSnapshotId = requiredString(input?.contentSnapshotId, "contentSnapshotId", 128);
    const parserId = requiredString(input?.parserId, "parserId", 128);
    const parserVersion = requiredString(input?.parserVersion, "parserVersion", 64);
    const parserConfigHash = sha256(input?.parserConfigHash);
    return toParseArtifact(this.db.prepare(`
      SELECT pa.*
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE pa.content_snapshot_id = ?
        AND pa.parser_id = ?
        AND pa.parser_version = ?
        AND pa.parser_config_hash = ?
        AND s.studio_id = ?
    `).get(contentSnapshotId, parserId, parserVersion, parserConfigHash, studioId));
  }

  beginParseArtifact(input: {
    studioId: unknown;
    contentSnapshotId: unknown;
    parseArtifactId?: unknown;
    parserId: unknown;
    parserVersion: unknown;
    parserConfigHash: unknown;
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSnapshotId = requiredString(input?.contentSnapshotId, "contentSnapshotId", 128);
    // 先经过 studio 归属检查，不能靠外键存在性代替授权。
    this.getContentSnapshot({ studioId, snapshotId: contentSnapshotId });
    const parserId = requiredString(input?.parserId, "parserId", 128);
    const parserVersion = requiredString(input?.parserVersion, "parserVersion", 64);
    const parserConfigHash = sha256(input?.parserConfigHash);
    const existing = this.findParseArtifactByIdentity({
      studioId,
      contentSnapshotId,
      parserId,
      parserVersion,
      parserConfigHash,
    });
    if (existing) {
      this.db.transaction(() => {
        this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(existing.id);
        this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(existing.id);
        this.db.prepare(`
          UPDATE parse_artifacts
          SET status = 'parsing', warnings_json = '[]', semantic_artifact_path = NULL, completed_at = NULL
          WHERE id = ?
        `).run(existing.id);
      })();
      return this.getParseArtifact({ studioId, parseArtifactId: existing.id });
    }

    const id = input.parseArtifactId == null
      ? this.newId("parse")
      : requiredString(input.parseArtifactId, "parseArtifactId", 128);
    const now = this.now();
    this.db.prepare(`
      INSERT INTO parse_artifacts (
        id, content_snapshot_id, parser_id, parser_version, parser_config_hash,
        status, warnings_json, semantic_artifact_path, created_at, completed_at
      ) VALUES (?, ?, ?, ?, ?, 'parsing', '[]', NULL, ?, NULL)
    `).run(id, contentSnapshotId, parserId, parserVersion, parserConfigHash, now);
    return this.getParseArtifact({ studioId, parseArtifactId: id });
  }

  completeParseArtifact(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    status: "ready" | "needs_ocr";
    warnings: unknown;
    semanticArtifactPath: unknown;
    blocks: KnowledgeBlockDraft[];
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    if (input.status !== "ready" && input.status !== "needs_ocr") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Parse completion status is invalid");
    }
    const warningsJson = serializeStringArray(input.warnings, "warnings");
    const semanticArtifactPath = storagePath(input.semanticArtifactPath);
    if (!Array.isArray(input.blocks)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "blocks must be an array");
    }
    if (input.status === "needs_ocr" && input.blocks.length > 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "needs_ocr artifacts cannot contain blocks");
    }
    if (input.status === "ready" && input.blocks.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "ready artifacts require at least one block");
    }

    const normalizedBlocks = input.blocks.map((block, index) => {
      if (block.ordinal !== index) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Block ordinals must be contiguous");
      }
      const text = requiredString(block.text, "block text", 2_000_000);
      const locatorType = block.locatorType;
      if (!new Set(["text", "markdown", "pdf", "html"]).has(locatorType)) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Block locator type is invalid");
      }
      return {
        id: this.newId("block"),
        ordinal: index,
        text,
        textSha256: crypto.createHash("sha256").update(text, "utf8").digest("hex"),
        locatorType,
        locatorJson: serializeObjectJson(block.locator, "block locator"),
      };
    });
    const completedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(parseArtifactId);
      const insert = this.db.prepare(`
        INSERT INTO knowledge_blocks (
          id, parse_artifact_id, ordinal, text, text_sha256, locator_type, locator_payload_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `);
      for (const block of normalizedBlocks) {
        insert.run(
          block.id,
          parseArtifactId,
          block.ordinal,
          block.text,
          block.textSha256,
          block.locatorType,
          block.locatorJson,
        );
      }
      this.db.prepare(`
        UPDATE parse_artifacts
        SET status = ?, warnings_json = ?, semantic_artifact_path = ?, completed_at = ?
        WHERE id = ?
      `).run(input.status, warningsJson, semanticArtifactPath, completedAt, parseArtifactId);
    })();
    return this.getParseArtifact({ studioId, parseArtifactId });
  }

  failParseArtifact(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    warnings?: unknown;
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    const warningsJson = serializeStringArray(input.warnings ?? ["parse_failed"], "warnings");
    const completedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`
        UPDATE parse_artifacts
        SET status = 'failed', warnings_json = ?, semantic_artifact_path = NULL, completed_at = ?
        WHERE id = ?
      `).run(warningsJson, completedAt, parseArtifactId);
    })();
    return this.getParseArtifact({ studioId, parseArtifactId });
  }

  getParseArtifact(input: { studioId: unknown; parseArtifactId: unknown }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    const artifact = toParseArtifact(this.db.prepare(`
      SELECT pa.*
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE pa.id = ? AND s.studio_id = ?
    `).get(parseArtifactId, studioId));
    if (!artifact) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Parse artifact not found");
    if (!PARSE_STATUSES.has(artifact.status)) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Parse artifact status is invalid");
    }
    return artifact;
  }

  listArtifactBlocks(input: { studioId: unknown; parseArtifactId: unknown }): KnowledgeBlock[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    return this.db.prepare(`
      SELECT * FROM knowledge_blocks
      WHERE parse_artifact_id = ?
      ORDER BY ordinal ASC
    `).all(parseArtifactId).map(toBlock);
  }

  createCitation(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    blockId: unknown;
    startOffset: unknown;
    endOffset: unknown;
  }): KnowledgeCitation {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    const artifact = this.getParseArtifact({ studioId, parseArtifactId });
    if (artifact.status !== "ready") {
      throw new KnowledgeError("KNOWLEDGE_PARSE_NOT_READY", "Parse artifact is not ready for citation");
    }
    const blockId = requiredString(input?.blockId, "blockId", 128);
    const block = toBlock(this.db.prepare(`
      SELECT * FROM knowledge_blocks WHERE id = ? AND parse_artifact_id = ?
    `).get(blockId, parseArtifactId));
    if (!block) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge block not found");
    if (!Number.isSafeInteger(input.startOffset) || !Number.isSafeInteger(input.endOffset)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Citation offsets must be integers");
    }
    const startOffset = Number(input.startOffset);
    const endOffset = Number(input.endOffset);
    if (startOffset < 0 || endOffset <= startOffset || endOffset > block.text.length) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Citation offsets are outside the block");
    }
    const canonicalText = block.text.slice(startOffset, endOffset);
    const id = this.newId("cite");
    const createdAt = this.now();
    const canonicalTextSha256 = crypto.createHash("sha256").update(canonicalText, "utf8").digest("hex");
    this.db.prepare(`
      INSERT INTO knowledge_citations (
        id, parse_artifact_id, block_id, start_offset, end_offset,
        canonical_text, canonical_text_sha256, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    `).run(
      id,
      parseArtifactId,
      blockId,
      startOffset,
      endOffset,
      canonicalText,
      canonicalTextSha256,
      createdAt,
    );
    return toCitation(this.db.prepare(`SELECT * FROM knowledge_citations WHERE id = ?`).get(id))!;
  }

  resolveCitation(input: { studioId: unknown; citationId: unknown }): ResolvedKnowledgeCitation {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const citationId = requiredString(input?.citationId, "citationId", 128);
    const row = this.db.prepare(`
      SELECT
        kc.id AS citation_id,
        kc.parse_artifact_id AS citation_parse_artifact_id,
        kc.block_id AS citation_block_id,
        kc.start_offset AS citation_start_offset,
        kc.end_offset AS citation_end_offset,
        kc.canonical_text AS citation_canonical_text,
        kc.canonical_text_sha256 AS citation_canonical_text_sha256,
        kc.created_at AS citation_created_at,
        kb.id AS block_id,
        kb.parse_artifact_id AS block_parse_artifact_id,
        kb.ordinal AS block_ordinal,
        kb.text AS block_text,
        kb.text_sha256 AS block_text_sha256,
        kb.locator_type AS block_locator_type,
        kb.locator_payload_json AS block_locator_payload_json,
        pa.id AS artifact_id,
        pa.content_snapshot_id AS artifact_content_snapshot_id,
        pa.parser_id AS artifact_parser_id,
        pa.parser_version AS artifact_parser_version,
        pa.parser_config_hash AS artifact_parser_config_hash,
        pa.status AS artifact_status,
        pa.warnings_json AS artifact_warnings_json,
        pa.semantic_artifact_path AS artifact_semantic_artifact_path,
        pa.created_at AS artifact_created_at,
        pa.completed_at AS artifact_completed_at,
        cs.id AS snapshot_id,
        cs.source_id AS snapshot_source_id,
        cs.sha256 AS snapshot_sha256,
        cs.mime_type AS snapshot_mime_type,
        cs.byte_size AS snapshot_byte_size,
        cs.storage_path AS snapshot_storage_path,
        cs.captured_at AS snapshot_captured_at,
        s.id AS source_id,
        s.studio_id AS source_studio_id,
        s.source_type AS source_type,
        s.display_name AS source_display_name,
        s.origin_metadata_json AS source_origin_metadata_json,
        s.created_at AS source_created_at,
        s.deleted_at AS source_deleted_at
      FROM knowledge_citations kc
      JOIN knowledge_blocks kb ON kb.id = kc.block_id AND kb.parse_artifact_id = kc.parse_artifact_id
      JOIN parse_artifacts pa ON pa.id = kc.parse_artifact_id
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE kc.id = ? AND s.studio_id = ?
    `).get(citationId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge citation not found");

    return {
      citation: {
        id: row.citation_id,
        parseArtifactId: row.citation_parse_artifact_id,
        blockId: row.citation_block_id,
        startOffset: Number(row.citation_start_offset),
        endOffset: Number(row.citation_end_offset),
        canonicalText: row.citation_canonical_text,
        canonicalTextSha256: row.citation_canonical_text_sha256,
        createdAt: row.citation_created_at,
      },
      block: {
        id: row.block_id,
        parseArtifactId: row.block_parse_artifact_id,
        ordinal: Number(row.block_ordinal),
        text: row.block_text,
        textSha256: row.block_text_sha256,
        locatorType: row.block_locator_type,
        locator: parseObjectJson(row.block_locator_payload_json, "block locator"),
      },
      artifact: {
        id: row.artifact_id,
        contentSnapshotId: row.artifact_content_snapshot_id,
        parserId: row.artifact_parser_id,
        parserVersion: row.artifact_parser_version,
        parserConfigHash: row.artifact_parser_config_hash,
        status: row.artifact_status,
        warnings: parseStringArrayJson(row.artifact_warnings_json, "parse warnings"),
        semanticArtifactPath: row.artifact_semantic_artifact_path || null,
        createdAt: row.artifact_created_at,
        completedAt: row.artifact_completed_at || null,
      },
      snapshot: {
        id: row.snapshot_id,
        sourceId: row.snapshot_source_id,
        sha256: row.snapshot_sha256,
        mimeType: row.snapshot_mime_type,
        byteSize: Number(row.snapshot_byte_size),
        storagePath: row.snapshot_storage_path,
        capturedAt: row.snapshot_captured_at,
      },
      source: {
        id: row.source_id,
        studioId: row.source_studio_id,
        sourceType: row.source_type,
        displayName: row.source_display_name,
        originMetadata: parseObjectJson(row.source_origin_metadata_json, "origin metadata"),
        createdAt: row.source_created_at,
        deletedAt: row.source_deleted_at || null,
      },
    };
  }

  countContentSnapshots(input: { studioId: unknown; sourceId: unknown }): number {
    const source = this.getSource(input);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count FROM content_snapshots WHERE source_id = ?
    `).get(source.id).count);
  }

  countParseArtifacts(input: { studioId: unknown; sourceId: unknown }): number {
    const source = this.getSource(input);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      WHERE cs.source_id = ?
    `).get(source.id).count);
  }

  /**
   * 入队一个摄入 job（phase 链 parse → chunk → fts_index → embed → done）。
   * 同一 notebook+source 已有活跃 job（queued/running/pending_embedding）时直接去重返回，
   * 不重复排队；done/failed 的历史 job 不挡新的摄入（配置变更重建语义）。
   * chunkerConfigId 记录触发摄入的笔记本分块配置——一源多笔记本配置冲突时以触发方为准。
   */
  enqueueIngestionJob(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
    chunkerConfigId: unknown;
    artifactId?: unknown;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const membership = toMembership(this.db.prepare(`
      SELECT * FROM notebook_sources
      WHERE notebook_id = ? AND source_id = ? AND removed_at IS NULL
    `).get(notebookId, sourceId));
    if (!membership) {
      throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge source is not in this Notebook");
    }
    const configId = chunkerConfigId(input.chunkerConfigId);
    const artifactId = input.artifactId == null
      ? null
      : this.getParseArtifact({ studioId, parseArtifactId: input.artifactId }).id;

    const existing = toIngestionJob(this.db.prepare(`
      SELECT * FROM ingestion_jobs
      WHERE notebook_id = ? AND source_id = ?
        AND status IN ('queued', 'running', 'pending_embedding')
      ORDER BY created_at DESC, id DESC
      LIMIT 1
    `).get(notebookId, sourceId));
    if (existing) return existing;

    const id = this.newId("ingjob");
    const now = this.now();
    this.db.prepare(`
      INSERT INTO ingestion_jobs (
        id, notebook_id, source_id, artifact_id, phase, status,
        attempt, retry_after, error, chunker_config_id, created_at, updated_at
      ) VALUES (?, ?, ?, ?, 'parse', 'queued', 0, NULL, NULL, ?, ?, ?)
    `).run(id, notebookId, sourceId, artifactId, configId, now, now);
    return this.getIngestionJob({ studioId, jobId: id });
  }

  getIngestionJob(input: { studioId: unknown; jobId: unknown }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const jobId = requiredString(input?.jobId, "jobId", 128);
    const job = toIngestionJob(this.db.prepare(`
      SELECT j.*
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE j.id = ? AND nb.studio_id = ?
    `).get(jobId, studioId));
    if (!job) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Ingestion job not found");
    return job;
  }

  /**
   * 原子认领下一个到期 queued job（retry_after 未到的跳过），置 running。
   * 队列由本进程内串行 worker 消费（engine 级，跨 studio）；同步驱动下单事务即原子。
   */
  claimNextIngestionJob(): IngestionJob | null {
    const now = this.now();
    return this.db.transaction(() => {
      const row = this.db.prepare(`
        SELECT * FROM ingestion_jobs
        WHERE status = 'queued' AND (retry_after IS NULL OR retry_after <= ?)
        ORDER BY created_at ASC, id ASC
        LIMIT 1
      `).get(now);
      if (!row) return null;
      this.db.prepare(`
        UPDATE ingestion_jobs SET status = 'running', updated_at = ?
        WHERE id = ?
      `).run(now, row.id);
      return toIngestionJob(this.db.prepare(`SELECT * FROM ingestion_jobs WHERE id = ?`).get(row.id));
    })();
  }

  private runningIngestionJob(studioId: unknown, jobId: unknown): IngestionJob {
    const job = this.getIngestionJob({ studioId, jobId });
    if (job.status !== "running") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Ingestion job is not running");
    }
    return job;
  }

  /** 推进到下一个待执行 phase；parse 完成时顺带绑定产生的 parse artifact。 */
  updateIngestionJobPhase(input: {
    studioId: unknown;
    jobId: unknown;
    phase: unknown;
    artifactId?: unknown;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    const phase = input.phase;
    if (typeof phase !== "string" || !INGESTION_PHASES.has(phase as IngestionPhase) || phase === "done") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion phase is invalid");
    }
    const artifactId = input.artifactId == null
      ? job.artifactId
      : this.getParseArtifact({ studioId, parseArtifactId: input.artifactId }).id;
    this.db.prepare(`
      UPDATE ingestion_jobs SET phase = ?, artifact_id = ?, updated_at = ?
      WHERE id = ?
    `).run(phase, artifactId, this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** 摄入 worker 的嵌入进度落库：仅 running 可写；total 首次给出时初始化（NULL → 已知值）。 */
  updateIngestionJobProgress(input: {
    studioId: unknown;
    jobId: unknown;
    done: unknown;
    total?: unknown;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    const done = input?.done;
    if (!Number.isSafeInteger(done) || Number(done) < 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion progress done must be a non-negative integer");
    }
    if (input?.total != null && (!Number.isSafeInteger(input.total) || Number(input.total) < 0)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion progress total must be a non-negative integer");
    }
    const total = input?.total == null ? job.progressTotal : Number(input.total);
    if (total != null && Number(done) > total) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion progress done must not exceed total");
    }
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET progress_done = ?, progress_total = ?, updated_at = ?
      WHERE id = ?
    `).run(Number(done), total, this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  completeIngestionJob(input: { studioId: unknown; jobId: unknown }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET phase = 'done', status = 'done', error = NULL, retry_after = NULL,
        progress_done = COALESCE(progress_total, progress_done), updated_at = ?
      WHERE id = ?
    `).run(this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** 显式终态（非失败）：FTS 已可查、嵌入模型未配置，等模型就绪信号补跑。 */
  markIngestionJobPendingEmbedding(input: { studioId: unknown; jobId: unknown }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    this.db.prepare(`
      UPDATE ingestion_jobs SET phase = 'embed', status = 'pending_embedding', updated_at = ?
      WHERE id = ?
    `).run(this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /**
   * 记录一次失败：attempt + 1，进度重置（重跑从 0 计，防 UI 显示旧进度回退）。
   * 带 retryAfter → 回到 queued 等退避到期；不带 → 标 failed
   * （attempt 上限判定在服务层，store 只做状态机）。
   */
  failIngestionJob(input: {
    studioId: unknown;
    jobId: unknown;
    error: unknown;
    retryAfter?: unknown;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    const error = requiredString(input.error, "error", 512);
    const retryAfter = isoTimestampOrNull(input.retryAfter, "retryAfter");
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET status = ?, attempt = attempt + 1, error = ?, retry_after = ?,
        progress_done = 0, progress_total = NULL, updated_at = ?
      WHERE id = ?
    `).run(retryAfter ? "queued" : "failed", error, retryAfter, this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** UI 手动重试：failed → queued，attempt 归零、进度重置；phase 保留，从失败的 phase 续跑（各步幂等）。 */
  requeueIngestionJob(input: { studioId: unknown; jobId: unknown }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.getIngestionJob({ studioId, jobId: input?.jobId });
    if (job.status !== "failed") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Only failed ingestion jobs can be retried");
    }
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET status = 'queued', attempt = 0, error = NULL, retry_after = NULL,
        progress_done = 0, progress_total = NULL, updated_at = ?
      WHERE id = ?
    `).run(this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** 模型就绪信号：全部 pending_embedding 一次性置回 queued 补跑嵌入。返回置回数量。 */
  requeuePendingEmbeddingIngestionJobs(): number {
    const result = this.db.prepare(`
      UPDATE ingestion_jobs SET status = 'queued', updated_at = ?
      WHERE status = 'pending_embedding'
    `).run(this.now());
    return Number(result.changes);
  }

  /**
   * 启动恢复：running 残留（进程崩溃/强杀中断）重置回 queued 续跑。
   * 各 phase 幂等（fingerprint/hasArtifact 判断），从 phase 断点续跑无副作用。返回重置数量。
   */
  requeueRunningIngestionJobs(): number {
    const result = this.db.prepare(`
      UPDATE ingestion_jobs SET status = 'queued', updated_at = ?
      WHERE status = 'running'
    `).run(this.now());
    return Number(result.changes);
  }

  /** 摄入 worker 跨 studio 认领 job 后回查归属（job 行不冗余存 studio_id，经 notebook join 推导）。 */
  getIngestionJobOwner(input: { jobId: unknown }): {
    studioId: string;
    notebookId: string;
    sourceId: string;
  } | null {
    const jobId = requiredString(input?.jobId, "jobId", 128);
    const row = this.db.prepare(`
      SELECT nb.studio_id AS studio_id, j.notebook_id AS notebook_id, j.source_id AS source_id
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE j.id = ?
    `).get(jobId);
    if (!row) return null;
    return {
      studioId: row.studio_id,
      notebookId: row.notebook_id,
      sourceId: row.source_id,
    };
  }

  /** 跨 studio 列出 pending_embedding job（模型就绪补跑判定用），每行附归属 studioId。 */
  listPendingEmbeddingIngestionJobs(): Array<IngestionJob & { studioId: string }> {
    return this.db.prepare(`
      SELECT j.*, nb.studio_id AS studio_id
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE j.status = 'pending_embedding'
      ORDER BY j.created_at ASC, j.id ASC
    `).all().map((row: any) => ({ ...(toIngestionJob(row) as IngestionJob), studioId: row.studio_id }));
  }

  /**
   * 源的最新摄入 job。可选 notebookId 过滤：一源多笔记本时各笔记本的
   * 摄入状态彼此独立（job 按 notebook+source 去重入队），不传则跨笔记本
   * 取最新一条（源级视图用）。
   */
  getLatestIngestionJobForSource(input: {
    studioId: unknown;
    sourceId: unknown;
    notebookId?: unknown;
  }): IngestionJob | null {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeSource(studioId, sourceId);
    const notebookId = input?.notebookId == null
      ? null
      : requiredString(input.notebookId, "notebookId", 128);
    return toIngestionJob(notebookId == null
      ? this.db.prepare(`
          SELECT * FROM ingestion_jobs
          WHERE source_id = ?
          ORDER BY created_at DESC, id DESC
          LIMIT 1
        `).get(sourceId)
      : this.db.prepare(`
          SELECT * FROM ingestion_jobs
          WHERE source_id = ? AND notebook_id = ?
          ORDER BY created_at DESC, id DESC
          LIMIT 1
        `).get(sourceId, notebookId));
  }

  listIngestionJobs(input: {
    studioId: unknown;
    notebookId?: unknown;
    sourceId?: unknown;
    statuses?: unknown;
    limit?: unknown;
  }): IngestionJob[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const clauses = ["nb.studio_id = ?"];
    const params: unknown[] = [studioId];
    if (input?.notebookId != null) {
      clauses.push("j.notebook_id = ?");
      params.push(requiredString(input.notebookId, "notebookId", 128));
    }
    if (input?.sourceId != null) {
      clauses.push("j.source_id = ?");
      params.push(requiredString(input.sourceId, "sourceId", 128));
    }
    if (input?.statuses != null) {
      if (
        !Array.isArray(input.statuses) || input.statuses.length === 0
        || input.statuses.some((status) => !INGESTION_STATUSES.has(status))
      ) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "statuses must be a non-empty ingestion status array");
      }
      clauses.push(`j.status IN (${input.statuses.map(() => "?").join(", ")})`);
      params.push(...input.statuses);
    }
    const limit = optionalIntegerInRange(input?.limit, "limit", 1, 500) ?? 100;
    params.push(limit);
    return this.db.prepare(`
      SELECT j.*
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE ${clauses.join(" AND ")}
      ORDER BY j.created_at DESC, j.id DESC
      LIMIT ?
    `).all(...params).map(toIngestionJob);
  }

  countIngestionJobsByStatus(input: {
    studioId: unknown;
    notebookId?: unknown;
  }): Record<IngestionJobStatus, number> {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const clauses = ["nb.studio_id = ?"];
    const params: unknown[] = [studioId];
    if (input?.notebookId != null) {
      clauses.push("j.notebook_id = ?");
      params.push(requiredString(input.notebookId, "notebookId", 128));
    }
    const counts: Record<IngestionJobStatus, number> = {
      queued: 0,
      running: 0,
      pending_embedding: 0,
      failed: 0,
      done: 0,
    };
    const rows = this.db.prepare(`
      SELECT j.status AS status, COUNT(*) AS count
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE ${clauses.join(" AND ")}
      GROUP BY j.status
    `).all(...params);
    for (const row of rows) {
      if (INGESTION_STATUSES.has(row.status)) {
        counts[row.status as IngestionJobStatus] = Number(row.count);
      }
    }
    return counts;
  }

  close() {
    if (!this.db) return;
    this.db.close();
    this.db = null;
  }
}
