import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

import { buildFtsLiteralQuery, buildSearchDocumentText } from "../search/search-text.ts";
import { KnowledgeError } from "./errors.ts";
import type { KnowledgeChunkDraft, KnowledgeChunkSpanDraft } from "./chunker.ts";

const require = createRequire(import.meta.url);
const KNOWLEDGE_INDEX_SCHEMA_VERSION = 1;
let BetterSqliteDatabase: any = null;

function loadDatabase() {
  if (!BetterSqliteDatabase) {
    const mod = require("better-sqlite3");
    BetterSqliteDatabase = mod?.default || mod;
  }
  return BetterSqliteDatabase;
}

function requiredId(value: unknown, field: string): string {
  if (typeof value !== "string" || !value.trim() || value.length > 128) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is invalid`);
  }
  return value.trim();
}

function parseSpans(value: unknown): KnowledgeChunkSpanDraft[] {
  try {
    const parsed = JSON.parse(typeof value === "string" ? value : "");
    if (!Array.isArray(parsed)) throw new Error("not_array");
    for (const span of parsed) {
      if (
        !span
        || typeof span.blockId !== "string"
        || !Number.isSafeInteger(span.blockStartOffset)
        || !Number.isSafeInteger(span.blockEndOffset)
        || !Number.isSafeInteger(span.chunkStartOffset)
        || !Number.isSafeInteger(span.chunkEndOffset)
        || span.blockStartOffset < 0
        || span.blockEndOffset <= span.blockStartOffset
        || span.chunkStartOffset < 0
        || span.chunkEndOffset <= span.chunkStartOffset
      ) {
        throw new Error("invalid_span");
      }
    }
    return parsed;
  } catch {
    throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge search index is corrupt");
  }
}

function serializeSpans(spans: KnowledgeChunkSpanDraft[]): string {
  const serialized = JSON.stringify(spans);
  if (Buffer.byteLength(serialized, "utf8") > 512 * 1024) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge chunk span map is too large");
  }
  return serialized;
}

export interface IndexedKnowledgeChunk extends KnowledgeChunkDraft {
  score: number;
}

export interface KnowledgeIndexStoreOptions {
  dbPath: string;
  Database?: any;
  now?: () => string;
}

/**
 * 可重建的全文检索缓存。事实仍只存在于 knowledge.db 与托管解析产物中。
 */
export class KnowledgeIndexStore {
  declare db: any;
  readonly dbPath: string;
  private readonly Database: any;
  private readonly now: () => string;

  constructor(options: KnowledgeIndexStoreOptions) {
    if (!options?.dbPath || !path.isAbsolute(options.dbPath)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "KnowledgeIndexStore requires an absolute dbPath");
    }
    this.dbPath = options.dbPath;
    this.Database = options.Database || loadDatabase();
    this.now = options.now || (() => new Date().toISOString());
    fs.mkdirSync(path.dirname(this.dbPath), { recursive: true, mode: 0o700 });
    this.openWithRecovery();
  }

  private openWithRecovery() {
    try {
      this.open();
    } catch {
      try { this.db?.close?.(); } catch { /* 丢弃可重建缓存时保留原始处理路径。 */ }
      this.db = null;
      // 索引不是事实；打开或迁移失败时只删除精确的索引文件，再从 Block 重建。
      for (const suffix of ["", "-wal", "-shm"]) {
        try { fs.unlinkSync(`${this.dbPath}${suffix}`); } catch (error: any) {
          if (error?.code !== "ENOENT") {
            throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge search index cannot be rebuilt");
          }
        }
      }
      try {
        this.open();
      } catch {
        try { this.db?.close?.(); } catch { /* 保留重建失败。 */ }
        this.db = null;
        throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge search index cannot be opened");
      }
    }
  }

  private open() {
    this.db = new this.Database(this.dbPath);
    this.db.pragma("journal_mode = WAL");
    this.db.pragma("synchronous = NORMAL");
    this.db.pragma("busy_timeout = 5000");
    const version = Number(this.db.pragma("user_version", { simple: true }));
    if (!Number.isSafeInteger(version) || version < 0 || version > KNOWLEDGE_INDEX_SCHEMA_VERSION) {
      throw new Error("unsupported_index_schema");
    }
    if (version === 0) {
      this.db.transaction(() => {
        this.createSchema();
        this.db.pragma(`user_version = ${KNOWLEDGE_INDEX_SCHEMA_VERSION}`);
      })();
    }
    const check = this.db.pragma("quick_check", { simple: true });
    if (check !== "ok") throw new Error("index_quick_check_failed");
  }

  private createSchema() {
    this.db.exec(`
      CREATE TABLE artifact_indexes (
        parse_artifact_id TEXT PRIMARY KEY,
        block_fingerprint TEXT NOT NULL,
        chunker_version TEXT NOT NULL,
        indexed_at TEXT NOT NULL
      );

      CREATE TABLE knowledge_chunks (
        row_id INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT NOT NULL UNIQUE,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        text TEXT NOT NULL,
        token_count INTEGER NOT NULL CHECK(token_count > 0),
        search_text TEXT NOT NULL,
        spans_json TEXT NOT NULL,
        UNIQUE(parse_artifact_id, ordinal)
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
    `);
  }

  hasArtifactFingerprint(parseArtifactId: unknown, fingerprint: unknown, chunkerVersion: unknown): boolean {
    const artifactId = requiredId(parseArtifactId, "parseArtifactId");
    const row = this.db.prepare(`
      SELECT block_fingerprint, chunker_version
      FROM artifact_indexes
      WHERE parse_artifact_id = ?
    `).get(artifactId);
    return !!row && row.block_fingerprint === fingerprint && row.chunker_version === chunkerVersion;
  }

  replaceArtifactChunks(input: {
    parseArtifactId: unknown;
    blockFingerprint: unknown;
    chunkerVersion: unknown;
    chunks: KnowledgeChunkDraft[];
  }) {
    const parseArtifactId = requiredId(input?.parseArtifactId, "parseArtifactId");
    const blockFingerprint = requiredId(input?.blockFingerprint, "blockFingerprint");
    const chunkerVersion = requiredId(input?.chunkerVersion, "chunkerVersion");
    if (!Array.isArray(input?.chunks) || input.chunks.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge chunks must not be empty");
    }
    const insert = this.db.prepare(`
      INSERT INTO knowledge_chunks (
        id, parse_artifact_id, ordinal, text, token_count, search_text, spans_json
      ) VALUES (?, ?, ?, ?, ?, ?, ?)
    `);
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_chunks WHERE parse_artifact_id = ?`).run(parseArtifactId);
      for (const [index, chunk] of input.chunks.entries()) {
        if (chunk.parseArtifactId !== parseArtifactId || chunk.ordinal !== index || !chunk.text) {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge chunk identity is invalid");
        }
        insert.run(
          requiredId(chunk.id, "chunkId"),
          parseArtifactId,
          index,
          chunk.text,
          chunk.tokenCount,
          buildSearchDocumentText(chunk.text),
          serializeSpans(chunk.spans),
        );
      }
      this.db.prepare(`
        INSERT INTO artifact_indexes (
          parse_artifact_id, block_fingerprint, chunker_version, indexed_at
        ) VALUES (?, ?, ?, ?)
        ON CONFLICT(parse_artifact_id) DO UPDATE SET
          block_fingerprint = excluded.block_fingerprint,
          chunker_version = excluded.chunker_version,
          indexed_at = excluded.indexed_at
      `).run(parseArtifactId, blockFingerprint, chunkerVersion, this.now());
    })();
  }

  listArtifactChunks(parseArtifactId: unknown): KnowledgeChunkDraft[] {
    const artifactId = requiredId(parseArtifactId, "parseArtifactId");
    return this.db.prepare(`
      SELECT * FROM knowledge_chunks
      WHERE parse_artifact_id = ?
      ORDER BY ordinal ASC
    `).all(artifactId).map((row: any) => ({
      id: row.id,
      parseArtifactId: row.parse_artifact_id,
      ordinal: Number(row.ordinal),
      text: row.text,
      tokenCount: Number(row.token_count),
      spans: parseSpans(row.spans_json),
    }));
  }

  search(input: {
    parseArtifactIds: unknown;
    query: unknown;
    limit?: unknown;
  }): IndexedKnowledgeChunk[] {
    if (!Array.isArray(input?.parseArtifactIds) || input.parseArtifactIds.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search scope must not be empty");
    }
    if (input.parseArtifactIds.length > 512) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search scope is too large");
    }
    const artifactIds = [...new Set(input.parseArtifactIds.map(id => requiredId(id, "parseArtifactId")))];
    if (typeof input.query !== "string" || !input.query.trim() || input.query.length > 4000) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search query is invalid");
    }
    const ftsQuery = buildFtsLiteralQuery(input.query);
    if (!ftsQuery) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search query has no searchable terms");
    }
    const limit = input.limit == null ? 12 : Number(input.limit);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 50) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search limit is invalid");
    }
    const placeholders = artifactIds.map(() => "?").join(", ");
    try {
      return this.db.prepare(`
        SELECT c.*, bm25(knowledge_chunks_fts, 1.0, 0.35) AS score
        FROM knowledge_chunks_fts
        JOIN knowledge_chunks c ON c.row_id = knowledge_chunks_fts.rowid
        WHERE knowledge_chunks_fts MATCH ?
          AND c.parse_artifact_id IN (${placeholders})
        ORDER BY score ASC, c.parse_artifact_id ASC, c.ordinal ASC
        LIMIT ?
      `).all(ftsQuery, ...artifactIds, limit).map((row: any) => ({
        id: row.id,
        parseArtifactId: row.parse_artifact_id,
        ordinal: Number(row.ordinal),
        text: row.text,
        tokenCount: Number(row.token_count),
        spans: parseSpans(row.spans_json),
        score: Number(row.score),
      }));
    } catch (error) {
      if (error instanceof KnowledgeError) throw error;
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge search index query failed");
    }
  }

  health() {
    const result = this.db.pragma("quick_check", { simple: true });
    return { status: result === "ok" ? "ready" as const : "corrupt" as const };
  }

  reset() {
    try { this.db?.close?.(); } catch { /* 可重建缓存无需保留关闭错误。 */ }
    this.db = null;
    for (const suffix of ["", "-wal", "-shm"]) {
      try { fs.unlinkSync(`${this.dbPath}${suffix}`); } catch (error: any) {
        if (error?.code !== "ENOENT") {
          throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge search index cannot be reset");
        }
      }
    }
    this.openWithRecovery();
  }

  close() {
    if (!this.db) return;
    this.db.close();
    this.db = null;
  }
}
