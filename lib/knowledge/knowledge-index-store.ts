import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

import { buildFtsLiteralQuery, buildSearchDocumentText } from "../search/search-text.ts";
import { KnowledgeError } from "./errors.ts";
import type { KnowledgeChunkDraft, KnowledgeChunkSpanDraft } from "./chunker.ts";

const require = createRequire(import.meta.url);
const KNOWLEDGE_INDEX_SCHEMA_VERSION = 2;
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

/** chunkProfileHash 即 chunker.ts 的 chunkerConfigId（sha256 前 16 hex），是跨库身份键。 */
function requiredChunkProfileHash(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{16}$/u.test(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "chunkProfileHash is invalid");
  }
  return value;
}

function requiredVariantStatus(value: unknown): KnowledgeChunkIndexVariantStatus {
  if (value !== "building" && value !== "ready" && value !== "failed" && value !== "retiring") {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "chunkIndexVariant status is invalid");
  }
  return value;
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

export type KnowledgeChunkIndexVariantStatus = "building" | "ready" | "failed" | "retiring";

export interface KnowledgeChunkIndexVariant {
  id: string;
  parseArtifactId: string;
  chunkProfileHash: string;
  status: KnowledgeChunkIndexVariantStatus;
  blockFingerprint: string;
  createdAt: string;
  updatedAt: string;
}

/** search 的 scope 单元：检索范围精确到 (parseArtifactId, chunkProfileHash) 一个索引变体。 */
export interface KnowledgeChunkSearchScope {
  parseArtifactId: string;
  chunkProfileHash: string;
}

/**
 * section 约束的 ordinal 过滤（Phase 8 broad，§三十九）：chunkIndexVariantId →
 * 允许参与的 ordinal 闭区间列表（section 的 headingPath 分桶在 ordinal 空间上
 * 近似连续，桶内 ordinal 合并为相邻区间）。空区间列表 = 该变体本轮不参与检索。
 */
export type KnowledgeOrdinalRange = readonly [number, number];

export interface StoredKnowledgeChunk extends KnowledgeChunkDraft {
  chunkIndexVariantId: string;
}

export interface IndexedKnowledgeChunk extends StoredKnowledgeChunk {
  score: number;
}

/**
 * ChunkIndexVariant 身份 = (parseArtifactId, chunkProfileHash)，id 确定性生成：
 * 'civ_' + sha256(parseArtifactId + '\0' + chunkProfileHash) 前 32 hex。
 * 同一输入必得同一 id，调用方可离线计算，无需先查库。
 */
export function knowledgeChunkIndexVariantId(parseArtifactId: string, chunkProfileHash: string): string {
  const digest = crypto.createHash("sha256")
    .update(`${parseArtifactId}\0${chunkProfileHash}`, "utf8")
    .digest("hex")
    .slice(0, 32);
  return `civ_${digest}`;
}

function mapVariantRow(row: any): KnowledgeChunkIndexVariant {
  return {
    id: row.id,
    parseArtifactId: row.parse_artifact_id,
    chunkProfileHash: row.chunk_profile_hash,
    status: row.status,
    blockFingerprint: row.block_fingerprint,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

const CHUNK_INDEX_VARIANTS_DDL = `
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
`;

function knowledgeChunksDdl(tableName: string): string {
  return `
    CREATE TABLE ${tableName} (
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
  `;
}

const CHUNK_SEARCH_OBJECTS_DDL = `
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
`;

export interface KnowledgeIndexStoreOptions {
  dbPath: string;
  Database?: any;
  now?: () => string;
}

/**
 * 可重建的全文检索缓存。事实仍只存在于 knowledge.db 与托管解析产物中。
 *
 * 契约（schema v2，P0 索引身份重构）：
 * - 派生 chunk 的身份锚是 ChunkIndexVariant：(parse_artifact_id, chunk_profile_hash)，
 *   见 chunk_index_variants 表；chunk_profile_hash 即 chunker.ts 的 chunkerConfigId。
 * - knowledge_chunks 以 chunk_index_variant_id + ordinal 为唯一约束
 *   （不再是 parse_artifact_id + ordinal），同一 artifact 的多个分块配置变体并存、互不覆盖。
 * - 写入/检索 API 全部以变体为锚：replaceArtifactChunks / hasArtifactFingerprint 接收
 *   (parseArtifactId, chunkProfileHash)；search 的 scope 是 { parseArtifactId, chunkProfileHash }
 *   对列表，只命中 status='ready' 的变体；scope 中的变体尚未建立时该 scope 自然无结果
 *   （index missing 由 ingestion 负责补齐，本层不现场重建）。
 * - v1 → v2 迁移在单个事务内完成：CREATE 新表 + 按 chunkerConfigId 回填变体 +
 *   INSERT SELECT 搬移 chunk + DROP 旧表 + RENAME + 重建 FTS，不丢既有索引数据，
 *   也不触发任何重新分块/embedding。artifact_indexes 表数据并入 chunk_index_variants 后
 *   DROP（先回填再退役，库内不再有两处身份真相）。
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
    } else if (version === 1) {
      this.migrateV1ToV2();
    }
    const check = this.db.pragma("quick_check", { simple: true });
    if (check !== "ok") throw new Error("index_quick_check_failed");
  }

  private createSchema() {
    this.db.exec(CHUNK_INDEX_VARIANTS_DDL);
    this.db.exec(knowledgeChunksDdl("knowledge_chunks"));
    this.db.exec(CHUNK_SEARCH_OBJECTS_DDL);
  }

  /**
   * v1（artifact_indexes + UNIQUE(parse_artifact_id, ordinal)）→ v2（chunk_index_variants）。
   * 单事务：建变体表并回填 → 建新 chunk 表并 INSERT SELECT 搬移 → DROP 旧表 → RENAME →
   * 重建 FTS 与触发器 → DROP artifact_indexes。chunker_version 列实存 chunkerConfigId，
   * 直接作为 chunk_profile_hash；少数远古行存的是常量版本号（非 16 hex），无法推导真实
   * 分块配置——按迁移原则不伪造 profile，跳过其变体与 chunk（ fingerprint 必不匹配，
   * 下次使用必然整体重建，等价于显式重建而非数据丢失）。
   */
  private migrateV1ToV2() {
    this.db.transaction(() => {
      const legacyIndexes = this.db.prepare(`
        SELECT parse_artifact_id, block_fingerprint, chunker_version, indexed_at
        FROM artifact_indexes
      `).all();
      this.db.exec(CHUNK_INDEX_VARIANTS_DDL);
      this.db.exec(knowledgeChunksDdl("knowledge_chunks_v2"));
      const insertVariant = this.db.prepare(`
        INSERT INTO chunk_index_variants (
          id, parse_artifact_id, chunk_profile_hash, status, block_fingerprint, created_at, updated_at
        ) VALUES (?, ?, ?, 'ready', ?, ?, ?)
      `);
      for (const row of legacyIndexes) {
        if (!/^[0-9a-f]{16}$/u.test(row.chunker_version)) continue;
        insertVariant.run(
          knowledgeChunkIndexVariantId(row.parse_artifact_id, row.chunker_version),
          row.parse_artifact_id,
          row.chunker_version,
          row.block_fingerprint,
          row.indexed_at,
          row.indexed_at,
        );
      }
      this.db.exec(`
        INSERT INTO knowledge_chunks_v2 (
          row_id, id, parse_artifact_id, chunk_index_variant_id,
          ordinal, text, token_count, search_text, spans_json
        )
        SELECT c.row_id, c.id, c.parse_artifact_id, v.id,
          c.ordinal, c.text, c.token_count, c.search_text, c.spans_json
        FROM knowledge_chunks c
        JOIN chunk_index_variants v ON v.parse_artifact_id = c.parse_artifact_id;

        DROP TABLE IF EXISTS knowledge_chunks_fts;
        DROP TABLE knowledge_chunks;
        ALTER TABLE knowledge_chunks_v2 RENAME TO knowledge_chunks;
      `);
      this.db.exec(CHUNK_SEARCH_OBJECTS_DDL);
      // 外联 FTS 表随新表重建为空，用 rebuild 从内容表整体回填，无需逐行重插。
      this.db.exec(`INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts) VALUES('rebuild');`);
      this.db.exec(`DROP TABLE artifact_indexes;`);
      this.db.pragma(`user_version = ${KNOWLEDGE_INDEX_SCHEMA_VERSION}`);
    })();
  }

  /** 按身份解析变体；不存在返回 null（missing 不等于 corrupt，由 ingestion 补齐）。 */
  resolveChunkIndexVariant(parseArtifactId: unknown, chunkProfileHash: unknown): KnowledgeChunkIndexVariant | null {
    const artifactId = requiredId(parseArtifactId, "parseArtifactId");
    const profileHash = requiredChunkProfileHash(chunkProfileHash);
    const row = this.db.prepare(`
      SELECT * FROM chunk_index_variants
      WHERE parse_artifact_id = ? AND chunk_profile_hash = ?
    `).get(artifactId, profileHash);
    return row ? mapVariantRow(row) : null;
  }

  /** 只读变体身份和 SQL 计数，避免把全部 chunk 搬进查询进程。 */
  getReadyVariantMetadata(input: {
    parseArtifactId: unknown;
    chunkProfileHash: unknown;
  }): {
    id: string;
    parseArtifactId: string;
    chunkProfileHash: string;
    blockFingerprint: string;
    chunkCount: number;
  } | null {
    const artifactId = requiredId(input?.parseArtifactId, "parseArtifactId");
    const profileHash = requiredChunkProfileHash(input?.chunkProfileHash);
    const row = this.db.prepare(`
      SELECT v.id, v.parse_artifact_id, v.chunk_profile_hash, v.block_fingerprint,
        (SELECT COUNT(*) FROM knowledge_chunks c WHERE c.chunk_index_variant_id = v.id) AS chunk_count
      FROM chunk_index_variants v
      WHERE v.parse_artifact_id = ? AND v.chunk_profile_hash = ? AND v.status = 'ready'
    `).get(artifactId, profileHash);
    return row ? {
      id: row.id,
      parseArtifactId: row.parse_artifact_id,
      chunkProfileHash: row.chunk_profile_hash,
      blockFingerprint: row.block_fingerprint,
      chunkCount: Number(row.chunk_count),
    } : null;
  }

  /** 幂等建立 building 状态的变体行；已存在（任意状态）则原样返回，不回退状态。 */
  ensureChunkIndexVariant(input: {
    parseArtifactId: unknown;
    chunkProfileHash: unknown;
    blockFingerprint: unknown;
  }): KnowledgeChunkIndexVariant {
    const artifactId = requiredId(input?.parseArtifactId, "parseArtifactId");
    const profileHash = requiredChunkProfileHash(input?.chunkProfileHash);
    const blockFingerprint = requiredId(input?.blockFingerprint, "blockFingerprint");
    const existing = this.resolveChunkIndexVariant(artifactId, profileHash);
    if (existing) return existing;
    const now = this.now();
    this.db.prepare(`
      INSERT INTO chunk_index_variants (
        id, parse_artifact_id, chunk_profile_hash, status, block_fingerprint, created_at, updated_at
      ) VALUES (?, ?, ?, 'building', ?, ?, ?)
    `).run(knowledgeChunkIndexVariantId(artifactId, profileHash), artifactId, profileHash, blockFingerprint, now, now);
    return this.resolveChunkIndexVariant(artifactId, profileHash)!;
  }

  setChunkIndexVariantStatus(chunkIndexVariantId: unknown, status: unknown): KnowledgeChunkIndexVariant {
    const variantId = requiredId(chunkIndexVariantId, "chunkIndexVariantId");
    const nextStatus = requiredVariantStatus(status);
    const result = this.db.prepare(`
      UPDATE chunk_index_variants SET status = ?, updated_at = ? WHERE id = ?
    `).run(nextStatus, this.now(), variantId);
    if (result.changes !== 1) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "chunkIndexVariantId is unknown");
    }
    return mapVariantRow(this.db.prepare(`SELECT * FROM chunk_index_variants WHERE id = ?`).get(variantId));
  }

  /**
   * 命中条件：变体存在且 status='ready' 且 block 指纹一致。身份含 chunkProfileHash，
   * 不同分块配置的变体各自独立判定，互不判失效。
   */
  hasArtifactFingerprint(parseArtifactId: unknown, chunkProfileHash: unknown, fingerprint: unknown): boolean {
    const variant = this.resolveChunkIndexVariant(parseArtifactId, chunkProfileHash);
    return !!variant && variant.status === "ready" && variant.blockFingerprint === fingerprint;
  }

  /**
   * 以 (parseArtifactId, chunkProfileHash) 锚定的幂等替换：只重建该变体自己的 chunk
   * 集合（DELETE 按 chunk_index_variant_id），同 artifact 的其他变体不受影响；
   * 完成后变体置为 ready。单事务，FTS 由触发器同步。
   */
  replaceArtifactChunks(input: {
    parseArtifactId: unknown;
    chunkProfileHash: unknown;
    blockFingerprint: unknown;
    chunks: KnowledgeChunkDraft[];
  }) {
    const parseArtifactId = requiredId(input?.parseArtifactId, "parseArtifactId");
    const chunkProfileHash = requiredChunkProfileHash(input?.chunkProfileHash);
    const blockFingerprint = requiredId(input?.blockFingerprint, "blockFingerprint");
    if (!Array.isArray(input?.chunks) || input.chunks.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge chunks must not be empty");
    }
    const variantId = knowledgeChunkIndexVariantId(parseArtifactId, chunkProfileHash);
    const insert = this.db.prepare(`
      INSERT INTO knowledge_chunks (
        id, parse_artifact_id, chunk_index_variant_id, ordinal, text, token_count, search_text, spans_json
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    `);
    this.db.transaction(() => {
      const now = this.now();
      this.db.prepare(`
        INSERT INTO chunk_index_variants (
          id, parse_artifact_id, chunk_profile_hash, status, block_fingerprint, created_at, updated_at
        ) VALUES (?, ?, ?, 'ready', ?, ?, ?)
        ON CONFLICT(parse_artifact_id, chunk_profile_hash) DO UPDATE SET
          status = 'ready',
          block_fingerprint = excluded.block_fingerprint,
          updated_at = excluded.updated_at
      `).run(variantId, parseArtifactId, chunkProfileHash, blockFingerprint, now, now);
      this.db.prepare(`DELETE FROM knowledge_chunks WHERE chunk_index_variant_id = ?`).run(variantId);
      for (const [index, chunk] of input.chunks.entries()) {
        if (chunk.parseArtifactId !== parseArtifactId || chunk.ordinal !== index || !chunk.text) {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge chunk identity is invalid");
        }
        insert.run(
          requiredId(chunk.id, "chunkId"),
          parseArtifactId,
          variantId,
          index,
          chunk.text,
          chunk.tokenCount,
          buildSearchDocumentText(chunk.text),
          serializeSpans(chunk.spans),
        );
      }
    })();
  }

  /** 删除某解析产物的全部 FTS chunk 行（源被移除/孤儿清理时调用）。
   * 注：v2 起索引登记并入 chunk_index_variants、artifact_indexes 表已 DROP，
   * 这里只清 knowledge_chunks。 */
  removeArtifact(parseArtifactId: unknown) {
    const artifactId = requiredId(parseArtifactId, "parseArtifactId");
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_chunks WHERE parse_artifact_id = ?`).run(artifactId);
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

  listVariantChunks(chunkIndexVariantId: unknown): StoredKnowledgeChunk[] {
    const variantId = requiredId(chunkIndexVariantId, "chunkIndexVariantId");
    return this.db.prepare(`
      SELECT * FROM knowledge_chunks
      WHERE chunk_index_variant_id = ?
      ORDER BY ordinal ASC
    `).all(variantId).map((row: any) => ({
      id: row.id,
      parseArtifactId: row.parse_artifact_id,
      chunkIndexVariantId: row.chunk_index_variant_id,
      ordinal: Number(row.ordinal),
      text: row.text,
      tokenCount: Number(row.token_count),
      spans: parseSpans(row.spans_json),
    }));
  }

  /**
   * 按变体 + ordinal 精确读块（Phase 8 邻接扩展，§三十六）：同变体内邻接
   * ordinal 的定点回读，不存在的 ordinal 自然缺席（窗口越过源边界）。
   * 只读、无自愈——行损坏由检索路径的 reset 自愈负责，这里如实抛出。
   */
  readVariantChunks(chunkIndexVariantId: unknown, ordinals: number[]): StoredKnowledgeChunk[] {
    const variantId = requiredId(chunkIndexVariantId, "chunkIndexVariantId");
    if (
      !Array.isArray(ordinals)
      || ordinals.length === 0
      || ordinals.some(ordinal => !Number.isSafeInteger(ordinal) || ordinal < 0)
    ) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge chunk ordinals are invalid");
    }
    const placeholders = ordinals.map(() => "?").join(", ");
    return this.db.prepare(`
      SELECT * FROM knowledge_chunks
      WHERE chunk_index_variant_id = ? AND ordinal IN (${placeholders})
      ORDER BY ordinal ASC
    `).all(variantId, ...ordinals).map((row: any) => ({
      id: row.id,
      parseArtifactId: row.parse_artifact_id,
      chunkIndexVariantId: row.chunk_index_variant_id,
      ordinal: Number(row.ordinal),
      text: row.text,
      tokenCount: Number(row.token_count),
      spans: parseSpans(row.spans_json),
    }));
  }

  /** 某 artifact 的全部 ChunkIndexVariant（所有 profile、所有状态）；Source 生命周期清理与 GC 用。 */
  listChunkIndexVariantsByArtifact(parseArtifactId: unknown): KnowledgeChunkIndexVariant[] {
    const artifactId = requiredId(parseArtifactId, "parseArtifactId");
    return this.db.prepare(`
      SELECT * FROM chunk_index_variants WHERE parse_artifact_id = ?
      ORDER BY created_at ASC, id ASC
    `).all(artifactId).map(mapVariantRow);
  }

  /** 全部 ChunkIndexVariant（诊断面：零引用 variant GC 候选扫描用）。 */
  listChunkIndexVariants(): KnowledgeChunkIndexVariant[] {
    return this.db.prepare(`
      SELECT * FROM chunk_index_variants ORDER BY created_at ASC, id ASC
    `).all().map(mapVariantRow);
  }

  /**
   * 删除某 artifact 的全部 chunk 变体（含 chunk 行与 FTS 行，触发器同步）：
   * Source 显式删除 / orphan GC 的派生索引清理（§十八/§十九）。单事务；
   * 返回删除的变体数。索引库是可重建缓存，删除即彻底（无 retiring 过渡）。
   */
  removeChunkIndexVariantsByArtifact(parseArtifactId: unknown): number {
    const artifactId = requiredId(parseArtifactId, "parseArtifactId");
    return Number(this.db.transaction(() => {
      const variants = this.db.prepare(`
        SELECT id FROM chunk_index_variants WHERE parse_artifact_id = ?
      `).all(artifactId) as any[];
      const deleteChunks = this.db.prepare(`
        DELETE FROM knowledge_chunks WHERE chunk_index_variant_id = ?
      `);
      const deleteVariant = this.db.prepare(`
        DELETE FROM chunk_index_variants WHERE id = ?
      `);
      for (const variant of variants) {
        deleteChunks.run(variant.id);
        deleteVariant.run(variant.id);
      }
      return variants.length;
    })());
  }

  /**
   * scope 为 { parseArtifactId, chunkProfileHash } 对列表；先解析为 ready 变体 id 再检索。
   * 解析不到 ready 变体的 scope 不参与检索（该 profile 尚未建索引，非错误、非降级）。
   *
   * ordinalRangesByChunkIndexVariantId（Phase 8 broad，§三十九）：按变体限定参与
   * 检索的 ordinal 闭区间（section-constrained secondary retrieval）。约束了空
   * 区间列表的变体不产生任何结果；未约束的变体不受影响。区间值非法抛 INVALID。
   */
  search(input: {
    scopes: unknown;
    query: unknown;
    limit?: unknown;
    ordinalRangesByChunkIndexVariantId?: ReadonlyMap<string, KnowledgeOrdinalRange[]>;
  }): IndexedKnowledgeChunk[] {
    if (!Array.isArray(input?.scopes) || input.scopes.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search scope must not be empty");
    }
    if (input.scopes.length > 512) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search scope is too large");
    }
    const scopeKeys = new Set<string>();
    const scopePairs: Array<{ parseArtifactId: string; chunkProfileHash: string }> = [];
    for (const scope of input.scopes) {
      const pair = {
        parseArtifactId: requiredId(scope?.parseArtifactId, "parseArtifactId"),
        chunkProfileHash: requiredChunkProfileHash(scope?.chunkProfileHash),
      };
      const key = `${pair.parseArtifactId}\0${pair.chunkProfileHash}`;
      if (!scopeKeys.has(key)) {
        scopeKeys.add(key);
        scopePairs.push(pair);
      }
    }
    if (typeof input.query !== "string" || !input.query.trim() || input.query.length > 4000) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search query is invalid");
    }
    const ftsQuery = buildFtsLiteralQuery(input.query);
    if (!ftsQuery) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search query has no searchable terms");
    }
    const limit = input.limit == null ? 12 : Number(input.limit);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 1000) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search limit is invalid");
    }
    const variantIds: string[] = [];
    const resolve = this.db.prepare(`
      SELECT id FROM chunk_index_variants
      WHERE parse_artifact_id = ? AND chunk_profile_hash = ? AND status = 'ready'
    `);
    for (const pair of scopePairs) {
      const row = resolve.get(pair.parseArtifactId, pair.chunkProfileHash);
      if (row) variantIds.push(row.id);
    }
    if (variantIds.length === 0) return [];
    // ordinal 约束归一：值域校验 + 变体去重；空区间列表的变体从可检索集合剔除。
    const ordinalRanges = input.ordinalRangesByChunkIndexVariantId ?? null;
    let variantFilterSql = "";
    const variantFilterParams: unknown[] = [];
    if (ordinalRanges) {
      const unfiltered: string[] = [];
      const seen = new Set<string>();
      for (const variantId of variantIds) {
        if (seen.has(variantId)) continue;
        seen.add(variantId);
        const ranges = ordinalRanges.get(variantId);
        if (!ranges) {
          unfiltered.push(variantId);
          continue;
        }
        const normalized: KnowledgeOrdinalRange[] = [];
        for (const range of ranges) {
          if (
            !Array.isArray(range) || range.length !== 2
            || !Number.isSafeInteger(range[0]) || !Number.isSafeInteger(range[1])
            || range[0] < 0 || range[1] < range[0]
          ) {
            throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search ordinal range is invalid");
          }
          normalized.push(range);
        }
        if (normalized.length === 0) continue;
        const rangeClauses = normalized
          .map(([low, high]) => (low === high
            ? "(c.chunk_index_variant_id = ? AND c.ordinal = ?)"
            : "(c.chunk_index_variant_id = ? AND c.ordinal BETWEEN ? AND ?)"))
          .join(" OR ");
        variantFilterSql += (variantFilterSql ? " OR " : "") + (normalized.length > 1 ? `(${rangeClauses})` : rangeClauses);
        for (const [low, high] of normalized) {
          variantFilterParams.push(variantId, ...(low === high ? [low] : [low, high]));
        }
      }
      if (unfiltered.length > 0) {
        const unfilteredSql = `c.chunk_index_variant_id IN (${unfiltered.map(() => "?").join(", ")})`;
        variantFilterSql = variantFilterSql ? `(${unfilteredSql} OR ${variantFilterSql})` : unfilteredSql;
        variantFilterParams.unshift(...unfiltered);
      }
      if (!variantFilterSql) return [];
    }
    const placeholders = variantIds.map(() => "?").join(", ");
    try {
      return this.db.prepare(`
        SELECT c.*, bm25(knowledge_chunks_fts, 1.0, 0.35) AS score
        FROM knowledge_chunks_fts
        JOIN knowledge_chunks c ON c.row_id = knowledge_chunks_fts.rowid
        WHERE knowledge_chunks_fts MATCH ?
          AND (${variantFilterSql || `c.chunk_index_variant_id IN (${placeholders})`})
        ORDER BY score ASC, c.parse_artifact_id ASC, c.ordinal ASC
        LIMIT ?
      `).all(ftsQuery, ...(variantFilterSql ? variantFilterParams : variantIds), limit).map((row: any) => ({
        id: row.id,
        parseArtifactId: row.parse_artifact_id,
        chunkIndexVariantId: row.chunk_index_variant_id,
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

  /** 编译后的范围直接进 SQL，排名、去重和条数限制都由数据库完成。 */
  searchReadyVariantIds(input: {
    chunkIndexVariantIds: string[];
    query: string;
    limit: number;
  }): IndexedKnowledgeChunk[] {
    const ids = [...new Set(input.chunkIndexVariantIds.map(id => requiredId(id, "chunkIndexVariantId")))];
    if (ids.length === 0) return [];
    if (typeof input.query !== "string" || !input.query.trim() || input.query.length > 4000) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search query is invalid");
    }
    if (!Number.isSafeInteger(input.limit) || input.limit < 1 || input.limit > 1000) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search limit is invalid");
    }
    const query = buildFtsLiteralQuery(input.query);
    if (!query) return [];
    try {
      // JSON 表值避免来源数量增长触及 SQLite 绑定参数上限。
      return this.db.prepare(`
        SELECT c.*, bm25(knowledge_chunks_fts, 1.0, 0.35) AS score
        FROM knowledge_chunks_fts
        JOIN knowledge_chunks c ON c.row_id = knowledge_chunks_fts.rowid
        JOIN chunk_index_variants v ON v.id = c.chunk_index_variant_id AND v.status = 'ready'
        WHERE knowledge_chunks_fts MATCH ?
          AND c.chunk_index_variant_id IN (SELECT value FROM json_each(?))
        ORDER BY score ASC, c.parse_artifact_id ASC, c.ordinal ASC, c.id ASC
        LIMIT ?
      `).all(query, JSON.stringify(ids), input.limit).map((row: any) => ({
        id: row.id,
        parseArtifactId: row.parse_artifact_id,
        chunkIndexVariantId: row.chunk_index_variant_id,
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
