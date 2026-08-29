import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

import { KnowledgeError } from "./errors.ts";

const require = createRequire(import.meta.url);
const VECTOR_INDEX_SCHEMA_VERSION = 1;
let BetterSqliteDatabase: any = null;

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

export interface VectorSearchResult {
  chunkId: string;
  parseArtifactId: string;
  ordinal: number;
  score: number;
}

export interface VectorIndexAdapter {
  hasArtifact(input: {
    parseArtifactId: unknown;
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
  }): boolean;
  buildOrReplaceArtifact(input: {
    parseArtifactId: unknown;
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
    entries: VectorIndexEntry[];
  }): void;
  removeArtifact(parseArtifactId: unknown): void;
  search(input: {
    parseArtifactIds: unknown;
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
}

/**
 * 不依赖本机扩展的可移植向量缓存。它只保存可重算的 Chunk 向量，事实库损坏
 * 处理与这里完全分开；缓存打不开时只重建这个精确文件。
 */
export class PortableVectorIndexAdapter implements VectorIndexAdapter {
  declare db: any;
  readonly dbPath: string;
  private readonly Database: any;
  private readonly now: () => string;

  constructor(options: PortableVectorIndexAdapterOptions) {
    if (!options?.dbPath || !path.isAbsolute(options.dbPath)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "PortableVectorIndexAdapter requires an absolute dbPath");
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
          CREATE TABLE vector_artifacts (
            parse_artifact_id TEXT NOT NULL,
            model_key TEXT NOT NULL,
            chunk_fingerprint TEXT NOT NULL,
            dimensions INTEGER NOT NULL CHECK(dimensions > 0),
            indexed_at TEXT NOT NULL,
            PRIMARY KEY(parse_artifact_id, model_key)
          );

          CREATE TABLE chunk_vectors (
            parse_artifact_id TEXT NOT NULL,
            model_key TEXT NOT NULL,
            chunk_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
            dimensions INTEGER NOT NULL CHECK(dimensions > 0),
            vector BLOB NOT NULL,
            PRIMARY KEY(model_key, chunk_id),
            UNIQUE(parse_artifact_id, model_key, ordinal)
          );

          CREATE INDEX idx_chunk_vectors_scope
            ON chunk_vectors(model_key, parse_artifact_id, ordinal);
        `);
        this.db.pragma(`user_version = ${VECTOR_INDEX_SCHEMA_VERSION}`);
      })();
    }
    if (this.db.pragma("quick_check", { simple: true }) !== "ok") {
      throw new Error("vector_index_quick_check_failed");
    }
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

  hasArtifact(input: {
    parseArtifactId: unknown;
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
  }): boolean {
    const parseArtifactId = requiredId(input?.parseArtifactId, "parseArtifactId");
    const fingerprint = requiredId(input?.chunkFingerprint, "chunkFingerprint");
    const model = this.normalizeModel(input.model);
    const row = this.db.prepare(`
      SELECT chunk_fingerprint, dimensions
      FROM vector_artifacts
      WHERE parse_artifact_id = ? AND model_key = ?
    `).get(parseArtifactId, model.key);
    return !!row && row.chunk_fingerprint === fingerprint && Number(row.dimensions) === model.dimensions;
  }

  buildOrReplaceArtifact(input: {
    parseArtifactId: unknown;
    chunkFingerprint: unknown;
    model: VectorIndexModelIdentity;
    entries: VectorIndexEntry[];
  }): void {
    const parseArtifactId = requiredId(input?.parseArtifactId, "parseArtifactId");
    const fingerprint = requiredId(input?.chunkFingerprint, "chunkFingerprint");
    const model = this.normalizeModel(input.model);
    if (!Array.isArray(input?.entries) || input.entries.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector entries must not be empty");
    }
    const insert = this.db.prepare(`
      INSERT INTO chunk_vectors (
        parse_artifact_id, model_key, chunk_id, ordinal, dimensions, vector
      ) VALUES (?, ?, ?, ?, ?, ?)
    `);
    this.db.transaction(() => {
      this.db.prepare(`
        DELETE FROM chunk_vectors WHERE parse_artifact_id = ? AND model_key = ?
      `).run(parseArtifactId, model.key);
      input.entries.forEach((entry, index) => {
        if (
          entry?.parseArtifactId !== parseArtifactId
          || entry.ordinal !== index
        ) {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector entry identity is invalid");
        }
        insert.run(
          parseArtifactId,
          model.key,
          requiredId(entry.chunkId, "chunkId"),
          index,
          model.dimensions,
          vectorBuffer(entry.vector, model.dimensions),
        );
      });
      this.db.prepare(`
        INSERT INTO vector_artifacts (
          parse_artifact_id, model_key, chunk_fingerprint, dimensions, indexed_at
        ) VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(parse_artifact_id, model_key) DO UPDATE SET
          chunk_fingerprint = excluded.chunk_fingerprint,
          dimensions = excluded.dimensions,
          indexed_at = excluded.indexed_at
      `).run(parseArtifactId, model.key, fingerprint, model.dimensions, this.now());
    })();
  }

  removeArtifact(parseArtifactId: unknown): void {
    const artifactId = requiredId(parseArtifactId, "parseArtifactId");
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM chunk_vectors WHERE parse_artifact_id = ?`).run(artifactId);
      this.db.prepare(`DELETE FROM vector_artifacts WHERE parse_artifact_id = ?`).run(artifactId);
    })();
  }

  search(input: {
    parseArtifactIds: unknown;
    model: VectorIndexModelIdentity;
    queryVector: number[];
    limit?: unknown;
  }): VectorSearchResult[] {
    if (!Array.isArray(input?.parseArtifactIds) || input.parseArtifactIds.length === 0 || input.parseArtifactIds.length > 512) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector search scope is invalid");
    }
    const artifactIds = [...new Set(input.parseArtifactIds.map(id => requiredId(id, "parseArtifactId")))];
    const model = this.normalizeModel(input.model);
    const query = readVector(vectorBuffer(input.queryVector, model.dimensions), model.dimensions);
    const limit = input.limit == null ? 12 : Number(input.limit);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 1000) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Vector search limit is invalid");
    }
    const placeholders = artifactIds.map(() => "?").join(", ");
    try {
      const rows = this.db.prepare(`
        SELECT parse_artifact_id, chunk_id, ordinal, dimensions, vector
        FROM chunk_vectors
        WHERE model_key = ? AND parse_artifact_id IN (${placeholders})
      `).all(model.key, ...artifactIds);
      return rows.map((row: any) => {
        if (Number(row.dimensions) !== model.dimensions) {
          throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector dimensions are corrupt");
        }
        return {
          chunkId: row.chunk_id,
          parseArtifactId: row.parse_artifact_id,
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
