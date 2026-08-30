/**
 * 不依赖本机扩展的可移植向量缓存（knowledge-vector.db）。它只保存可重算的 Chunk 向量，
 * 事实库损坏处理与这里完全分开；缓存打不开时只重建这个精确文件。
 *
 * ── Schema v2 契约（VectorIndexVariant，Phase 1 / P0 索引身份）──────────────────
 *
 * 向量身份不再是 (parse_artifact_id, model_key)，而是：
 *
 *   ChunkIndexVariant  = (parseArtifactId, chunkProfileHash)
 *     id = 'civ_' + sha256(parseArtifactId + '\0' + chunkProfileHash) 前 32 hex
 *     chunkProfileHash 即 FTS 库 chunk_index_variants.chunk_profile_hash
 *     （= lib/knowledge/chunker.ts 的 knowledgeChunkerConfigId，跨库身份键）
 *   VectorIndexVariant = (chunkIndexVariantId, embeddingModelKey)
 *     id = 'viv_' + sha256(chunkIndexVariantId + '\0' + modelKey) 前 32 hex
 *     modelKey 沿用 knowledge-query-service.ts 的算法
 *     （sha256(JSON[provider, modelId, protocol, dimensions])）
 *
 * 两张表：
 *   vector_index_variants(id PK, chunk_index_variant_id, parse_artifact_id 冗余,
 *     model_key, chunk_fingerprint, dimensions,
 *     status ∈ building/ready/failed/retiring, created_at, updated_at,
 *     UNIQUE(chunk_index_variant_id, model_key))
 *   chunk_vectors(vector_index_variant_id, ..., UNIQUE(vector_index_variant_id, ordinal))
 *
 * 跨库无 FK：chunk_index_variant_id 是纯字符串身份，对 FTS 库只作逻辑引用。
 * 迁移 v1→v2 在单事务内完成（建表 → 回填 → 重建 chunk_vectors → DROP vector_artifacts），
 * 只建立身份、映射已有向量，禁止触发任何重新 embedding；resolver 缺失/返回 null 的行
 * 以 chunkProfileHash = 'legacy_unknown' 建档（宁可标记 legacy 也不丢向量）。
 * 注意：迁移抛错会被 openWithRecovery 按「缓存损坏」语义删库重建，因此
 * profileHashResolver 必须是全函数（任何 parseArtifactId 都不许抛）。
 *
 * ── API 锚点 ────────────────────────────────────────────────────────────────
 * hasArtifact / buildOrReplaceArtifact 以 (chunkIndexVariantId + parseArtifactId)
 * 或 vectorIndexVariantId 为锚；search 的 scope 以 vectorIndexVariantIds 为锚，
 * 可选叠加 parseArtifactIds 过滤。保留旧式裸 parseArtifactId 锚作为显式过渡路径
 * （跟随该 (artifact, model) 已存在的 variant，无则以 legacy_unknown 建档），
 * 集成波次完成后退役。
 *
 * ── 批级 checkpoint 构建协议（Phase 3 / 任务书 §十四/§十五）──────────────────
 *
 * 摄入 embed 相位不再攒内存最后一次落库，而是按 64 块/批边嵌边持久化：
 *   beginVectorVariantBuild   ensure variant 行（status=building，记录指纹/维度）；
 *                             既有行指纹或维度漂移 → 显式清旧向量重建
 *                             （resetStaleVectors=true，调用方必须留痕，不静默）；
 *                             指纹/维度一致 → 保留已落库向量（断点续嵌的锚）。
 *   upsertChunkVectorBatch    每批嵌入成功后单事务 INSERT OR REPLACE
 *                             （按 PK(vector_index_variant_id, chunk_id) 幂等）；
 *                             指纹/维度/modelKey 与 variant 记录不符 → 拒绝混写。
 *   completeVectorVariantBuild 完整性校验（条数 + ordinal 0..n-1 连续覆盖）后
 *                             status=ready；校验不过抛错，variant 保持 building。
 *   failVectorVariantBuild    status=failed，已落库向量一律保留（付费产物不删）。
 *   listVariantsByChunkIndexVariant / listVariantVectorChunkIds
 *                             恢复时 diff 缺失 chunk 集合的只读面。
 * 中断（abort/进程退出/批次失败）语义：variant 停在中断时状态（building/failed），
 * 已落库向量绝不删除；重启由调用方 diff 缺失集合只补未完成的 chunk。
 *
 * ── 集成层接线状态 ──────────────────────────────────────────────────────────
 * 1. knowledge-manager.ts（new PortableVectorIndexAdapter 处）：打开 FTS 库后已注入
 *    profileHashResolver = (parseArtifactId) => indexStore 读
 *    chunk_index_variants.chunk_profile_hash（无行 → null）。
 * 2. knowledge-query-service.ts embedArtifactForIngestion（摄入相位）：
 *    hasArtifact / buildOrReplaceArtifact 已改传 chunkIndexVariantId（由 FTS 侧
 *    variant 身份提供），裸 parseArtifactId 锚仅余外部过渡调用方。
 *    （Phase 2 起查询侧只做 hasArtifact 只读判定 + search，不再写入。）
 * 3. knowledge-query-service.ts retrieve：search 已改传 vectorIndexVariantIds
 *    （按 scope artifact × 当前 RetrievalProfile 的 civ × modelKey 确定性求 id）。
 * 4. 孤儿 GC（待建）：removeVariant / removeArtifact 用于零引用 variant 清扫；
 *    vector_index_variants.parse_artifact_id 冗余列即为此预留。
 */
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

import { KnowledgeError } from "./errors.ts";

const require = createRequire(import.meta.url);
const VECTOR_INDEX_SCHEMA_VERSION = 2;
let BetterSqliteDatabase: any = null;

/** 迁移回填兜底：无法解析 chunkProfileHash 的历史数据统一挂到这个身份下。 */
export const LEGACY_UNKNOWN_CHUNK_PROFILE_HASH = "legacy_unknown";

/** ChunkIndexVariant 确定性 id：'civ_' + sha256(parseArtifactId + '\0' + chunkProfileHash) 前 32 hex。 */
export function knowledgeChunkIndexVariantId(parseArtifactId: string, chunkProfileHash: string): string {
  const digest = crypto.createHash("sha256")
    .update(`${parseArtifactId}\0${chunkProfileHash}`, "utf8")
    .digest("hex");
  return `civ_${digest.slice(0, 32)}`;
}

/** VectorIndexVariant 确定性 id：'viv_' + sha256(chunkIndexVariantId + '\0' + modelKey) 前 32 hex。 */
export function knowledgeVectorIndexVariantId(chunkIndexVariantId: string, modelKey: string): string {
  const digest = crypto.createHash("sha256")
    .update(`${chunkIndexVariantId}\0${modelKey}`, "utf8")
    .digest("hex");
  return `viv_${digest.slice(0, 32)}`;
}

function loadDatabase() {
  if (!BetterSqliteDatabase) {
    const mod = require("better-sqlite3");
    BetterSqliteDatabase = mod?.default || mod;
  }
  return BetterSqliteDatabase;
}

function requiredId(value: unknown, field: string, max = 256): string {
  if (typeof value !== "string" || !value.trim() || value.length > max) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is invalid`);
  }
  return value.trim();
}

function requiredDimensions(value: unknown): number {
  const dimensions = Number(value);
  if (!Number.isSafeInteger(dimensions) || dimensions <= 0 || dimensions > 65_536) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector dimensions are invalid");
  }
  return dimensions;
}

function vectorBuffer(value: unknown, dimensions: number): Buffer {
  if (
    !Array.isArray(value)
    || value.length !== dimensions
    || value.some((item) => typeof item !== "number" || !Number.isFinite(item))
  ) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector values are invalid");
  }
  const buffer = Buffer.allocUnsafe(dimensions * 4);
  value.forEach((item, index) => buffer.writeFloatLE(item, index * 4));
  return buffer;
}

function readVector(value: unknown, dimensions: number): number[] {
  if (!Buffer.isBuffer(value) || value.byteLength !== dimensions * 4) {
    throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector index is corrupt");
  }
  const vector = new Array<number>(dimensions);
  for (let index = 0; index < dimensions; index += 1) {
    const item = value.readFloatLE(index * 4);
    if (!Number.isFinite(item)) {
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector index is corrupt");
    }
    vector[index] = item;
  }
  return vector;
}

function cosine(left: number[], right: number[]): number {
  let dot = 0;
  let leftNorm = 0;
  let rightNorm = 0;
  for (let index = 0; index < left.length; index += 1) {
    dot += left[index] * right[index];
    leftNorm += left[index] * left[index];
    rightNorm += right[index] * right[index];
  }
  if (leftNorm === 0 || rightNorm === 0) return 0;
  const score = dot / (Math.sqrt(leftNorm) * Math.sqrt(rightNorm));
  if (!Number.isFinite(score)) {
    throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector score is invalid");
  }
  return Math.max(-1, Math.min(1, score));
}

export interface VectorIndexModelIdentity {
  key: string;
  provider: string;
  modelId: string;
  protocol: string;
  dimensions: number;
}

export interface VectorIndexEntry {
  chunkId: string;
  parseArtifactId: string;
  ordinal: number;
  vector: number[];
}

/** 批级 checkpoint 的单批条目：parseArtifactId 由 variant 行冗余列带出，调用方不重复传。 */
export type VectorIndexBatchEntry = Omit<VectorIndexEntry, "parseArtifactId">;

export interface VectorVariantBuildBegin {
  vectorIndexVariantId: string;
  /**
   * true = 既有 variant 的 chunk 指纹或维度与本次构建不符（内容已变），
   * 旧向量已显式清除、variant 重置为 building 从头构建——调用方必须留痕（不静默）。
   */
  resetStaleVectors: boolean;
}

export interface VectorSearchResult {
  chunkId: string;
  parseArtifactId: string;
  vectorIndexVariantId: string;
  ordinal: number;
  score: number;
}

export type VectorIndexVariantStatus = "building" | "ready" | "failed" | "retiring";

export interface VectorIndexVariantRecord {
  id: string;
  chunkIndexVariantId: string;
  parseArtifactId: string;
  modelKey: string;
  chunkFingerprint: string;
  dimensions: number;
  status: VectorIndexVariantStatus;
  createdAt: string;
  updatedAt: string;
}

/**
 * variant 锚点：chunkIndexVariantId（+ parseArtifactId）优先；vectorIndexVariantId
 * 次之；裸 parseArtifactId 为显式过渡路径（见文件头契约）。
 */
export interface VectorIndexVariantAnchor {
  vectorIndexVariantId?: unknown;
  chunkIndexVariantId?: unknown;
  parseArtifactId?: unknown;
}

export interface VectorIndexAdapter {
  hasArtifact(input: VectorIndexVariantAnchor & {
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
  }): boolean;
  buildOrReplaceArtifact(input: VectorIndexVariantAnchor & {
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
    entries: VectorIndexEntry[];
  }): { vectorIndexVariantId: string };
  removeArtifact(parseArtifactId: unknown): void;
  removeVariant(vectorIndexVariantId: unknown): void;
  getVariant(vectorIndexVariantId: unknown): VectorIndexVariantRecord | null;
  setVariantStatus(vectorIndexVariantId: unknown, status: VectorIndexVariantStatus): void;
  /** 只读：列出某 ChunkIndexVariant 下的全部向量变体（所有模型、所有状态），恢复 diff 用。 */
  listVariantsByChunkIndexVariant(chunkIndexVariantId: unknown): VectorIndexVariantRecord[];
  /** 只读：某 variant 已落库的 chunk_id 集合（checkpoint 断点），恢复时与全集 diff。 */
  listVariantVectorChunkIds(vectorIndexVariantId: unknown): string[];
  /**
   * 批级 checkpoint 构建协议（见文件头）：ensure variant 行并置 building。
   * 指纹/维度漂移 → 显式清旧向量重建（resetStaleVectors=true）；一致 → 保留断点向量。
   */
  beginVectorVariantBuild(input: VectorIndexVariantAnchor & {
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
  }): VectorVariantBuildBegin;
  /** 每批嵌入成功后单事务持久化（INSERT OR REPLACE 按 (variant, chunk_id) 幂等）。 */
  upsertChunkVectorBatch(input: {
    vectorIndexVariantId: unknown;
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
    entries: VectorIndexBatchEntry[];
  }): void;
  /** 完整性校验（条数 = expectedChunkCount 且 ordinal 0..n-1 连续）后置 ready。 */
  completeVectorVariantBuild(input: {
    vectorIndexVariantId: unknown;
    chunkFingerprint: unknown;
    expectedChunkCount: unknown;
    model?: VectorIndexModelIdentity;
  }): void;
  /** 显式失败终态：status=failed，已落库向量保留（付费产物不删）。 */
  failVectorVariantBuild(vectorIndexVariantId: unknown): void;
  search(input: {
    vectorIndexVariantIds?: unknown;
    parseArtifactIds?: unknown;
    model: VectorIndexModelIdentity;
    queryVector: number[];
    limit?: unknown;
  }): VectorSearchResult[];
  health(): { status: "ready" | "corrupt" };
  rebuild(): void;
  close(): void;
}

export interface PortableVectorIndexAdapterOptions {
  dbPath: string;
  Database?: any;
  now?: () => string;
  /**
   * 迁移 v1→v2 回填用：parseArtifactId → chunkProfileHash（FTS 库
   * chunk_index_variants.chunk_profile_hash）。由 KnowledgeManager 打开两库后注入；
   * 缺失或返回 null 的行以 LEGACY_UNKNOWN_CHUNK_PROFILE_HASH 建档。
   * 必须是全函数：抛错会中止迁移并触发缓存级重建（见文件头）。
   */
  profileHashResolver?: (parseArtifactId: string) => string | null;
}

export class PortableVectorIndexAdapter implements VectorIndexAdapter {
  declare db: any;
  readonly dbPath: string;
  private readonly Database: any;
  private readonly now: () => string;
  private readonly profileHashResolver: ((parseArtifactId: string) => string | null) | null;

  constructor(options: PortableVectorIndexAdapterOptions) {
    if (!options?.dbPath || !path.isAbsolute(options.dbPath)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "PortableVectorIndexAdapter requires an absolute dbPath");
    }
    this.dbPath = options.dbPath;
    this.Database = options.Database || loadDatabase();
    this.now = options.now || (() => new Date().toISOString());
    this.profileHashResolver = typeof options.profileHashResolver === "function"
      ? options.profileHashResolver
      : null;
    fs.mkdirSync(path.dirname(this.dbPath), { recursive: true, mode: 0o700 });
    this.openWithRecovery();
  }

  private openWithRecovery() {
    try {
      this.open();
    } catch {
      this.closeQuietly();
      this.deleteFiles();
      try {
        this.open();
      } catch {
        this.closeQuietly();
        throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector index cannot be opened");
      }
    }
  }

  private open() {
    this.db = new this.Database(this.dbPath);
    this.db.pragma("journal_mode = WAL");
    this.db.pragma("synchronous = NORMAL");
    this.db.pragma("busy_timeout = 5000");
    const version = Number(this.db.pragma("user_version", { simple: true }));
    if (!Number.isSafeInteger(version) || version < 0 || version > VECTOR_INDEX_SCHEMA_VERSION) {
      throw new Error("unsupported_vector_index_schema");
    }
    if (version === 0) {
      this.db.transaction(() => {
        this.db.exec(`
          CREATE TABLE vector_index_variants (
            id TEXT PRIMARY KEY,
            chunk_index_variant_id TEXT NOT NULL,
            parse_artifact_id TEXT NOT NULL,
            model_key TEXT NOT NULL,
            chunk_fingerprint TEXT NOT NULL,
            dimensions INTEGER NOT NULL CHECK(dimensions > 0),
            status TEXT NOT NULL CHECK(status IN ('building', 'ready', 'failed', 'retiring')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(chunk_index_variant_id, model_key)
          );

          CREATE TABLE chunk_vectors (
            vector_index_variant_id TEXT NOT NULL,
            parse_artifact_id TEXT NOT NULL,
            model_key TEXT NOT NULL,
            chunk_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
            dimensions INTEGER NOT NULL CHECK(dimensions > 0),
            vector BLOB NOT NULL,
            PRIMARY KEY(vector_index_variant_id, chunk_id),
            UNIQUE(vector_index_variant_id, ordinal)
          );

          CREATE INDEX idx_chunk_vectors_scope
            ON chunk_vectors(vector_index_variant_id, parse_artifact_id, ordinal);
          CREATE INDEX idx_chunk_vectors_artifact
            ON chunk_vectors(parse_artifact_id);
          CREATE INDEX idx_vector_index_variants_artifact
            ON vector_index_variants(parse_artifact_id);
        `);
        this.db.pragma(`user_version = ${VECTOR_INDEX_SCHEMA_VERSION}`);
      })();
    } else if (version === 1) {
      this.migrateV1ToV2();
    }
    if (this.db.pragma("quick_check", { simple: true }) !== "ok") {
      throw new Error("vector_index_quick_check_failed");
    }
  }

  /**
   * v1→v2：单事务内建 variant 表、按 (parse_artifact_id, model_key) 回填身份、
   * 重建 chunk_vectors 补上 vector_index_variant_id，最后 DROP vector_artifacts。
   * 只搬数据与身份，不重算任何向量。chunk_vectors 中出现但 vector_artifacts 缺失的
   * (artifact, model) 分区按 sentinel 指纹补建 variant（指纹永不命中 → 调用方自然重建，
   * 显式而非静默）。
   */
  private migrateV1ToV2() {
    this.db.transaction(() => {
      this.db.exec(`
        CREATE TABLE vector_index_variants (
          id TEXT PRIMARY KEY,
          chunk_index_variant_id TEXT NOT NULL,
          parse_artifact_id TEXT NOT NULL,
          model_key TEXT NOT NULL,
          chunk_fingerprint TEXT NOT NULL,
          dimensions INTEGER NOT NULL CHECK(dimensions > 0),
          status TEXT NOT NULL CHECK(status IN ('building', 'ready', 'failed', 'retiring')),
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          UNIQUE(chunk_index_variant_id, model_key)
        );

        CREATE TABLE chunk_vectors_next (
          vector_index_variant_id TEXT NOT NULL,
          parse_artifact_id TEXT NOT NULL,
          model_key TEXT NOT NULL,
          chunk_id TEXT NOT NULL,
          ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
          dimensions INTEGER NOT NULL CHECK(dimensions > 0),
          vector BLOB NOT NULL,
          PRIMARY KEY(vector_index_variant_id, chunk_id),
          UNIQUE(vector_index_variant_id, ordinal)
        );
      `);

      const scopeKey = (parseArtifactId: string, modelKey: string) => `${parseArtifactId}\0${modelKey}`;
      const variantByScope = new Map<string, string>();
      const insertVariant = this.db.prepare(`
        INSERT INTO vector_index_variants (
          id, chunk_index_variant_id, parse_artifact_id, model_key,
          chunk_fingerprint, dimensions, status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, 'ready', ?, ?)
      `);
      const resolveProfileHash = (parseArtifactId: string): string => {
        const resolved = this.profileHashResolver?.(parseArtifactId);
        return typeof resolved === "string" && resolved.trim()
          ? resolved.trim()
          : LEGACY_UNKNOWN_CHUNK_PROFILE_HASH;
      };
      const registerVariant = (input: {
        parseArtifactId: string;
        modelKey: string;
        chunkFingerprint: string;
        dimensions: number;
        createdAt: string;
      }) => {
        const key = scopeKey(input.parseArtifactId, input.modelKey);
        if (variantByScope.has(key)) return;
        const civ = knowledgeChunkIndexVariantId(input.parseArtifactId, resolveProfileHash(input.parseArtifactId));
        const viv = knowledgeVectorIndexVariantId(civ, input.modelKey);
        insertVariant.run(
          viv, civ, input.parseArtifactId, input.modelKey,
          input.chunkFingerprint, input.dimensions, input.createdAt, this.now(),
        );
        variantByScope.set(key, viv);
      };

      const artifactRows = this.db.prepare(`
        SELECT parse_artifact_id, model_key, chunk_fingerprint, dimensions, indexed_at
        FROM vector_artifacts
      `).all();
      for (const row of artifactRows) {
        registerVariant({
          parseArtifactId: row.parse_artifact_id,
          modelKey: row.model_key,
          chunkFingerprint: row.chunk_fingerprint,
          dimensions: Number(row.dimensions),
          createdAt: row.indexed_at,
        });
      }
      // vector_artifacts 缺失的孤儿向量分区：补 sentinel variant，不丢向量。
      const orphanScopes = this.db.prepare(`
        SELECT DISTINCT parse_artifact_id, model_key, dimensions FROM chunk_vectors
      `).all();
      for (const row of orphanScopes) {
        registerVariant({
          parseArtifactId: row.parse_artifact_id,
          modelKey: row.model_key,
          chunkFingerprint: LEGACY_UNKNOWN_CHUNK_PROFILE_HASH,
          dimensions: Number(row.dimensions),
          createdAt: this.now(),
        });
      }

      const vectorRows = this.db.prepare(`
        SELECT parse_artifact_id, model_key, chunk_id, ordinal, dimensions, vector
        FROM chunk_vectors
      `).all();
      const insertVector = this.db.prepare(`
        INSERT INTO chunk_vectors_next (
          vector_index_variant_id, parse_artifact_id, model_key,
          chunk_id, ordinal, dimensions, vector
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `);
      for (const row of vectorRows) {
        const viv = variantByScope.get(scopeKey(row.parse_artifact_id, row.model_key));
        if (!viv) {
          throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector index migration lost a variant scope");
        }
        insertVector.run(
          viv, row.parse_artifact_id, row.model_key,
          row.chunk_id, Number(row.ordinal), Number(row.dimensions), row.vector,
        );
      }

      this.db.exec(`
        DROP TABLE chunk_vectors;
        ALTER TABLE chunk_vectors_next RENAME TO chunk_vectors;
        CREATE INDEX idx_chunk_vectors_scope
          ON chunk_vectors(vector_index_variant_id, parse_artifact_id, ordinal);
        CREATE INDEX idx_chunk_vectors_artifact
          ON chunk_vectors(parse_artifact_id);
        CREATE INDEX idx_vector_index_variants_artifact
          ON vector_index_variants(parse_artifact_id);
        DROP TABLE vector_artifacts;
      `);
      this.db.pragma(`user_version = ${VECTOR_INDEX_SCHEMA_VERSION}`);
    })();
  }

  private closeQuietly() {
    try { this.db?.close?.(); } catch { /* 缓存关闭失败不覆盖原错误。 */ }
    this.db = null;
  }

  private deleteFiles() {
    for (const suffix of ["", "-wal", "-shm"]) {
      try { fs.unlinkSync(`${this.dbPath}${suffix}`); } catch (error: any) {
        if (error?.code !== "ENOENT") {
          throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector index cannot be rebuilt");
        }
      }
    }
  }

  private normalizeModel(model: VectorIndexModelIdentity): VectorIndexModelIdentity {
    const dimensions = requiredDimensions(model?.dimensions);
    return {
      key: requiredId(model?.key, "model.key", 512),
      provider: requiredId(model?.provider, "model.provider"),
      modelId: requiredId(model?.modelId, "model.modelId", 512),
      protocol: requiredId(model?.protocol, "model.protocol"),
      dimensions,
    };
  }

  private mapVariantRow(row: any): VectorIndexVariantRecord {
    return {
      id: row.id,
      chunkIndexVariantId: row.chunk_index_variant_id,
      parseArtifactId: row.parse_artifact_id,
      modelKey: row.model_key,
      chunkFingerprint: row.chunk_fingerprint,
      dimensions: Number(row.dimensions),
      status: row.status,
      createdAt: row.created_at,
      updatedAt: row.updated_at,
    };
  }

  /**
   * 读侧锚点解析：vectorIndexVariantId 直查；(chunkIndexVariantId, model) 按唯一约束查；
   * 裸 parseArtifactId 为过渡路径——跟随该 (artifact, model) 下已存在的全部 variant
   * （过渡期至多一个；多 profile 并存后调用方必须改传显式锚点）。
   */
  private selectVariantRows(anchor: VectorIndexVariantAnchor, modelKey: string): any[] {
    if (anchor?.vectorIndexVariantId != null) {
      const id = requiredId(anchor.vectorIndexVariantId, "vectorIndexVariantId", 64);
      const row = this.db.prepare(`
        SELECT * FROM vector_index_variants WHERE id = ? AND model_key = ?
      `).get(id, modelKey);
      return row ? [row] : [];
    }
    if (anchor?.chunkIndexVariantId != null) {
      const civ = requiredId(anchor.chunkIndexVariantId, "chunkIndexVariantId", 64);
      return this.db.prepare(`
        SELECT * FROM vector_index_variants
        WHERE chunk_index_variant_id = ? AND model_key = ?
      `).all(civ, modelKey);
    }
    const parseArtifactId = requiredId(anchor?.parseArtifactId, "parseArtifactId");
    return this.db.prepare(`
      SELECT * FROM vector_index_variants
      WHERE parse_artifact_id = ? AND model_key = ?
      ORDER BY created_at, id
    `).all(parseArtifactId, modelKey);
  }

  hasArtifact(input: VectorIndexVariantAnchor & {
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
  }): boolean {
    const fingerprint = requiredId(input?.chunkFingerprint, "chunkFingerprint");
    const model = this.normalizeModel(input.model);
    return this.selectVariantRows(input, model.key).some(row => (
      row.status === "ready"
      && row.chunk_fingerprint === fingerprint
      && Number(row.dimensions) === model.dimensions
    ));
  }

  /**
   * 写侧锚点解析：显式 (chunkIndexVariantId, parseArtifactId) 直接确定 variant 身份；
   * 裸 parseArtifactId 过渡路径沿用该 (artifact, model) 已迁入/已建的第一个 variant，
   * 不存在则以 legacy_unknown profile 建档——与迁移回填落点一致，旧调用方不会
   * 因身份漂移而误判全量失效。
   */
  private resolveBuildAnchor(anchor: VectorIndexVariantAnchor, modelKey: string): {
    vectorIndexVariantId: string;
    chunkIndexVariantId: string;
    parseArtifactId: string;
  } {
    let chunkIndexVariantId: string;
    let parseArtifactId: string;
    if (anchor?.chunkIndexVariantId != null) {
      chunkIndexVariantId = requiredId(anchor.chunkIndexVariantId, "chunkIndexVariantId", 64);
      parseArtifactId = requiredId(anchor.parseArtifactId, "parseArtifactId");
    } else {
      parseArtifactId = requiredId(anchor?.parseArtifactId, "parseArtifactId");
      const existing = this.db.prepare(`
        SELECT chunk_index_variant_id FROM vector_index_variants
        WHERE parse_artifact_id = ? AND model_key = ?
        ORDER BY created_at, id LIMIT 1
      `).get(parseArtifactId, modelKey);
      chunkIndexVariantId = existing?.chunk_index_variant_id
        ?? knowledgeChunkIndexVariantId(parseArtifactId, LEGACY_UNKNOWN_CHUNK_PROFILE_HASH);
    }
    const vectorIndexVariantId = knowledgeVectorIndexVariantId(chunkIndexVariantId, modelKey);
    if (anchor?.vectorIndexVariantId != null) {
      const expected = requiredId(anchor.vectorIndexVariantId, "vectorIndexVariantId", 64);
      if (expected !== vectorIndexVariantId) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "vectorIndexVariantId does not match its identity inputs");
      }
    }
    return { vectorIndexVariantId, chunkIndexVariantId, parseArtifactId };
  }

  buildOrReplaceArtifact(input: VectorIndexVariantAnchor & {
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
    entries: VectorIndexEntry[];
  }): { vectorIndexVariantId: string } {
    const fingerprint = requiredId(input?.chunkFingerprint, "chunkFingerprint");
    const model = this.normalizeModel(input.model);
    if (!Array.isArray(input?.entries) || input.entries.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector entries must not be empty");
    }
    const anchor = this.resolveBuildAnchor(input, model.key);
    const insert = this.db.prepare(`
      INSERT INTO chunk_vectors (
        vector_index_variant_id, parse_artifact_id, model_key,
        chunk_id, ordinal, dimensions, vector
      ) VALUES (?, ?, ?, ?, ?, ?, ?)
    `);
    this.db.transaction(() => {
      this.db.prepare(`
        DELETE FROM chunk_vectors WHERE vector_index_variant_id = ?
      `).run(anchor.vectorIndexVariantId);
      input.entries.forEach((entry, index) => {
        if (
          entry?.parseArtifactId !== anchor.parseArtifactId
          || entry.ordinal !== index
        ) {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector entry identity is invalid");
        }
        insert.run(
          anchor.vectorIndexVariantId,
          anchor.parseArtifactId,
          model.key,
          requiredId(entry.chunkId, "chunkId"),
          index,
          model.dimensions,
          vectorBuffer(entry.vector, model.dimensions),
        );
      });
      this.db.prepare(`
        INSERT INTO vector_index_variants (
          id, chunk_index_variant_id, parse_artifact_id, model_key,
          chunk_fingerprint, dimensions, status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, 'ready', ?, ?)
        ON CONFLICT(id) DO UPDATE SET
          chunk_fingerprint = excluded.chunk_fingerprint,
          dimensions = excluded.dimensions,
          status = 'ready',
          updated_at = excluded.updated_at
      `).run(
        anchor.vectorIndexVariantId,
        anchor.chunkIndexVariantId,
        anchor.parseArtifactId,
        model.key,
        fingerprint,
        model.dimensions,
        this.now(),
        this.now(),
      );
    })();
    return { vectorIndexVariantId: anchor.vectorIndexVariantId };
  }

  removeArtifact(parseArtifactId: unknown): void {
    const artifactId = requiredId(parseArtifactId, "parseArtifactId");
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM chunk_vectors WHERE parse_artifact_id = ?`).run(artifactId);
      this.db.prepare(`DELETE FROM vector_index_variants WHERE parse_artifact_id = ?`).run(artifactId);
    })();
  }

  removeVariant(vectorIndexVariantId: unknown): void {
    const id = requiredId(vectorIndexVariantId, "vectorIndexVariantId", 64);
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM chunk_vectors WHERE vector_index_variant_id = ?`).run(id);
      this.db.prepare(`DELETE FROM vector_index_variants WHERE id = ?`).run(id);
    })();
  }

  getVariant(vectorIndexVariantId: unknown): VectorIndexVariantRecord | null {
    const id = requiredId(vectorIndexVariantId, "vectorIndexVariantId", 64);
    const row = this.db.prepare(`SELECT * FROM vector_index_variants WHERE id = ?`).get(id);
    return row ? this.mapVariantRow(row) : null;
  }

  setVariantStatus(vectorIndexVariantId: unknown, status: VectorIndexVariantStatus): void {
    const id = requiredId(vectorIndexVariantId, "vectorIndexVariantId", 64);
    if (!["building", "ready", "failed", "retiring"].includes(status)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector index variant status is invalid");
    }
    const result = this.db.prepare(`
      UPDATE vector_index_variants SET status = ?, updated_at = ? WHERE id = ?
    `).run(status, this.now(), id);
    if (result.changes === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector index variant does not exist");
    }
  }

  listVariantsByChunkIndexVariant(chunkIndexVariantId: unknown): VectorIndexVariantRecord[] {
    const civ = requiredId(chunkIndexVariantId, "chunkIndexVariantId", 64);
    return this.db.prepare(`
      SELECT * FROM vector_index_variants
      WHERE chunk_index_variant_id = ?
      ORDER BY created_at, id
    `).all(civ).map((row: any) => this.mapVariantRow(row));
  }

  listVariantVectorChunkIds(vectorIndexVariantId: unknown): string[] {
    const id = requiredId(vectorIndexVariantId, "vectorIndexVariantId", 64);
    return this.db.prepare(`
      SELECT chunk_id FROM chunk_vectors WHERE vector_index_variant_id = ? ORDER BY ordinal
    `).all(id).map((row: any) => row.chunk_id);
  }

  /**
   * 写侧防混写守卫：variant 行必须存在且模型身份（model_key 由身份函数保证同 viv
   * 同 model_key，仍显式校验）、chunk 指纹、维度三者与调用方一致——任一不符说明
   * 调用方在往错误的构建上下文里写（并发交叉/断点漂移），显式拒绝而非混写。
   * 不用 KNOWLEDGE_INDEX_INVALID：该码在摄入侧触发缓存级重建重试，身份不符不是损坏。
   */
  private requireVariantForBuild(vectorIndexVariantId: unknown, chunkFingerprint: unknown, model?: VectorIndexModelIdentity): any {
    const id = requiredId(vectorIndexVariantId, "vectorIndexVariantId", 64);
    const fingerprint = requiredId(chunkFingerprint, "chunkFingerprint");
    const row = this.db.prepare(`SELECT * FROM vector_index_variants WHERE id = ?`).get(id);
    if (!row) {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Vector index variant build does not exist");
    }
    if (row.chunk_fingerprint !== fingerprint) {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Vector index variant build fingerprint mismatch");
    }
    if (model) {
      const normalized = this.normalizeModel(model);
      if (row.model_key !== normalized.key || Number(row.dimensions) !== normalized.dimensions) {
        throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Vector index variant build model mismatch");
      }
    }
    return row;
  }

  beginVectorVariantBuild(input: VectorIndexVariantAnchor & {
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
  }): VectorVariantBuildBegin {
    const fingerprint = requiredId(input?.chunkFingerprint, "chunkFingerprint");
    const model = this.normalizeModel(input.model);
    const anchor = this.resolveBuildAnchor(input, model.key);
    return this.db.transaction(() => {
      const row = this.db.prepare(`
        SELECT * FROM vector_index_variants WHERE id = ?
      `).get(anchor.vectorIndexVariantId);
      if (!row) {
        this.db.prepare(`
          INSERT INTO vector_index_variants (
            id, chunk_index_variant_id, parse_artifact_id, model_key,
            chunk_fingerprint, dimensions, status, created_at, updated_at
          ) VALUES (?, ?, ?, ?, ?, ?, 'building', ?, ?)
        `).run(
          anchor.vectorIndexVariantId,
          anchor.chunkIndexVariantId,
          anchor.parseArtifactId,
          model.key,
          fingerprint,
          model.dimensions,
          this.now(),
          this.now(),
        );
        return { vectorIndexVariantId: anchor.vectorIndexVariantId, resetStaleVectors: false };
      }
      if (row.model_key !== model.key) {
        // viv = f(civ, modelKey) 确定性派生，命中此处即身份函数被破坏，显式拒绝。
        throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Vector index variant build model mismatch");
      }
      const drift = row.chunk_fingerprint !== fingerprint || Number(row.dimensions) !== model.dimensions;
      if (drift) {
        // 内容/维度漂移：旧向量对新指纹已失效，显式清库重建（允许删除的唯一场景——
        // 内容已变，向量不再是任何可用断点）；调用方按 resetStaleVectors 留痕。
        this.db.prepare(`
          DELETE FROM chunk_vectors WHERE vector_index_variant_id = ?
        `).run(anchor.vectorIndexVariantId);
        this.db.prepare(`
          UPDATE vector_index_variants
          SET chunk_fingerprint = ?, dimensions = ?, status = 'building', updated_at = ?
          WHERE id = ?
        `).run(fingerprint, model.dimensions, this.now(), anchor.vectorIndexVariantId);
        return { vectorIndexVariantId: anchor.vectorIndexVariantId, resetStaleVectors: true };
      }
      // 断点续嵌：指纹/维度一致，已落库向量全部保留，只把状态拨回 building。
      this.db.prepare(`
        UPDATE vector_index_variants SET status = 'building', updated_at = ? WHERE id = ?
      `).run(this.now(), anchor.vectorIndexVariantId);
      return { vectorIndexVariantId: anchor.vectorIndexVariantId, resetStaleVectors: false };
    })();
  }

  upsertChunkVectorBatch(input: {
    vectorIndexVariantId: unknown;
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
    entries: VectorIndexBatchEntry[];
  }): void {
    const model = this.normalizeModel(input?.model);
    if (!Array.isArray(input?.entries) || input.entries.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector entries must not be empty");
    }
    const upsert = this.db.prepare(`
      INSERT OR REPLACE INTO chunk_vectors (
        vector_index_variant_id, parse_artifact_id, model_key,
        chunk_id, ordinal, dimensions, vector
      ) VALUES (?, ?, ?, ?, ?, ?, ?)
    `);
    this.db.transaction(() => {
      const variant = this.requireVariantForBuild(
        input.vectorIndexVariantId,
        input.chunkFingerprint,
        model,
      );
      for (const entry of input.entries) {
        const ordinal = Number(entry?.ordinal);
        if (!Number.isSafeInteger(ordinal) || ordinal < 0) {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector entry ordinal is invalid");
        }
        upsert.run(
          variant.id,
          variant.parse_artifact_id,
          model.key,
          requiredId(entry.chunkId, "chunkId"),
          ordinal,
          model.dimensions,
          vectorBuffer(entry.vector, model.dimensions),
        );
      }
    })();
  }

  completeVectorVariantBuild(input: {
    vectorIndexVariantId: unknown;
    chunkFingerprint: unknown;
    expectedChunkCount: unknown;
    model?: VectorIndexModelIdentity;
  }): void {
    const expected = Number(input?.expectedChunkCount);
    if (!Number.isSafeInteger(expected) || expected <= 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector variant expected chunk count is invalid");
    }
    this.db.transaction(() => {
      const variant = this.requireVariantForBuild(
        input?.vectorIndexVariantId,
        input?.chunkFingerprint,
        input?.model,
      );
      const stats = this.db.prepare(`
        SELECT COUNT(*) AS count, COUNT(DISTINCT ordinal) AS distinctOrdinals,
          MIN(ordinal) AS minOrdinal, MAX(ordinal) AS maxOrdinal
        FROM chunk_vectors WHERE vector_index_variant_id = ?
      `).get(variant.id);
      const complete = Number(stats.count) === expected
        && Number(stats.distinctOrdinals) === expected
        && Number(stats.minOrdinal) === 0
        && Number(stats.maxOrdinal) === expected - 1;
      if (!complete) {
        // 完整性不过：保持 building（已落库向量保留），调用方按失败路径分类留痕。
        throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Vector index variant build is incomplete");
      }
      this.db.prepare(`
        UPDATE vector_index_variants SET status = 'ready', updated_at = ? WHERE id = ?
      `).run(this.now(), variant.id);
    })();
  }

  failVectorVariantBuild(vectorIndexVariantId: unknown): void {
    // 显式终态：只翻状态，已落库向量（已付费的嵌入产物）一律保留。
    this.setVariantStatus(vectorIndexVariantId, "failed");
  }

  search(input: {
    vectorIndexVariantIds?: unknown;
    parseArtifactIds?: unknown;
    model: VectorIndexModelIdentity;
    queryVector: number[];
    limit?: unknown;
  }): VectorSearchResult[] {
    const normalizeScope = (value: unknown, field: string): string[] | null => {
      if (value == null) return null;
      if (!Array.isArray(value) || value.length === 0 || value.length > 512) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector search scope is invalid");
      }
      return [...new Set(value.map(id => requiredId(id, field, 64)))];
    };
    const variantIds = normalizeScope(input?.vectorIndexVariantIds, "vectorIndexVariantId");
    // 过渡能力：按 parse_artifact_id 过滤（单用等价 v1 scope；与 variant 列表叠加时取交集）。
    const artifactIds = normalizeScope(input?.parseArtifactIds, "parseArtifactId");
    if (!variantIds && !artifactIds) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector search scope is invalid");
    }
    const model = this.normalizeModel(input.model);
    const query = readVector(vectorBuffer(input.queryVector, model.dimensions), model.dimensions);
    const limit = input.limit == null ? 12 : Number(input.limit);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 1000) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector search limit is invalid");
    }
    const conditions = ["model_key = ?"];
    const params: unknown[] = [model.key];
    if (variantIds) {
      conditions.push(`vector_index_variant_id IN (${variantIds.map(() => "?").join(", ")})`);
      params.push(...variantIds);
    }
    if (artifactIds) {
      conditions.push(`parse_artifact_id IN (${artifactIds.map(() => "?").join(", ")})`);
      params.push(...artifactIds);
    }
    try {
      const rows = this.db.prepare(`
        SELECT vector_index_variant_id, parse_artifact_id, chunk_id, ordinal, dimensions, vector
        FROM chunk_vectors
        WHERE ${conditions.join(" AND ")}
      `).all(...params);
      return rows.map((row: any) => {
        if (Number(row.dimensions) !== model.dimensions) {
          throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector dimensions are corrupt");
        }
        return {
          chunkId: row.chunk_id,
          parseArtifactId: row.parse_artifact_id,
          vectorIndexVariantId: row.vector_index_variant_id,
          ordinal: Number(row.ordinal),
          score: cosine(query, readVector(row.vector, model.dimensions)),
        };
      }).sort((left, right) => (
        right.score - left.score
        || left.parseArtifactId.localeCompare(right.parseArtifactId)
        || left.ordinal - right.ordinal
      )).slice(0, limit);
    } catch (error) {
      if (error instanceof KnowledgeError) throw error;
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector index query failed");
    }
  }

  health() {
    return { status: this.db.pragma("quick_check", { simple: true }) === "ok" ? "ready" as const : "corrupt" as const };
  }

  rebuild(): void {
    this.closeQuietly();
    this.deleteFiles();
    this.openWithRecovery();
  }

  close(): void {
    this.closeQuietly();
  }
}
