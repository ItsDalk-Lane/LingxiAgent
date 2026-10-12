import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { KnowledgeError } from "./errors.ts";

const require = createRequire(import.meta.url);
export const KNOWLEDGE_ANN_INDEX_FORMAT_VERSION = 1;

function assertCacheSchema(db: any, supported: number): void {
  const version = Number(db.pragma("user_version", { simple: true }));
  if (version > supported) throw new KnowledgeError("KNOWLEDGE_SCHEMA_NEWER", "Knowledge cache schema newer; original preserved");
  if (!Number.isSafeInteger(version) || version < 0) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Knowledge cache schema invalid");
}

/** 初始化也必须保护持久化参数；不能把 WAL 切换放在版本事务之前。 */
export function openKnowledgeCacheDatabase(Database: any, dbPath: string, supported: number,
  initialize: (db: any) => void): { db: any; closeGuard: any } {
  let db: any, closeGuard: any;
  try {
    if (fs.existsSync(dbPath)) {
      closeGuard = new Database(dbPath, { readonly: true, fileMustExist: true });
      assertCacheSchema(closeGuard, supported);
    }
    db = new Database(dbPath);
    db.pragma("busy_timeout = 5000");
    // 已有 WAL 不必再持久化切换；只读守卫阻止最后一个可写句柄关闭时 checkpoint。
    const journal = db.pragma("journal_mode", { simple: true });
    if (journal === "wal") closeGuard?.pragma("user_version", { simple: true });
    if (journal !== "wal") {
      db.pragma("locking_mode = EXCLUSIVE");
      db.transaction(() => assertCacheSchema(db, supported)).exclusive();
      // EXCLUSIVE 提交后仍保留文件锁；NORMAL 本身不释放锁，下一次数据库访问才释放。
      // 先恢复 NORMAL 再切 WAL，避免进入无法释放的 WAL 独占模式。
      db.pragma("locking_mode = NORMAL");
      if (db.pragma("journal_mode = WAL", { simple: true }) !== "wal") {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Knowledge cache WAL initialization refused");
      }
    }
    closeGuard ??= new Database(dbPath, { readonly: true, fileMustExist: true });
    // 无长期读事务，不钉住旧快照；自动 checkpoint 与普通并发读写仍可使用。
    closeGuard.pragma("user_version", { simple: true });
    db.pragma("synchronous = NORMAL");
    initialize(db);
    return { db, closeGuard };
  } catch (error) {
    // WAL 守卫先前已取得共享文件锁；回滚模式拒绝则尚未发生持久化切换。
    try { db?.close(); } finally { closeGuard?.close(); }
    throw error;
  }
}

export function closeKnowledgeCacheDatabase(db: any, closeGuard: any): void {
  try { db?.close(); } finally { closeGuard?.close(); }
}

export interface KnowledgeAnnVariant {
  vectorIndexVariantId: string;
  modelKey: string;
  dimensions: number;
  chunkFingerprint: string;
  vectorCount: number;
  indexFormatVersion: number;
  fileName: string;
  status: "building" | "ready" | "failed";
  createdAt: string;
  updatedAt: string;
}

export function knowledgeAnnFileName(modelKey: string, variantId: string): string {
  if (!/^[a-zA-Z0-9_-]{1,256}$/.test(modelKey) || !/^viv_[a-f0-9]{32}$/.test(variantId)) {
    throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "ANN identity is invalid");
  }
  return `${modelKey.slice(0, 16)}/${variantId}.usearch`;
}

/** 独立的可重建目录库，失败不得触碰保存原始向量的数据库。 */
export class AnnIndexStore {
  readonly dbPath: string;
  private readonly db: any;
  private readonly closeGuard: any;
  private readonly now: () => string;

  constructor(options: { dbPath: string; Database?: any; now?: () => string }) {
    if (!path.isAbsolute(options.dbPath)) throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "ANN database path must be absolute");
    this.dbPath = options.dbPath;
    this.now = options.now ?? (() => new Date().toISOString());
    fs.mkdirSync(path.dirname(options.dbPath), { recursive: true, mode: 0o700 });
    const Database = options.Database ?? require("better-sqlite3");
    let meta: fs.Stats | undefined;
    try { meta = fs.lstatSync(options.dbPath); }
    catch (error: unknown) { if ((error as NodeJS.ErrnoException)?.code !== "ENOENT") throw error; }
    if (meta) {
      if (!meta.isFile() || meta.nlink !== 1) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "ANN catalog is not an exclusive regular file");
      if (meta.size > 0) {
        const fd = fs.openSync(options.dbPath, "r"), header = Buffer.alloc(16);
        try {
          if (fs.readSync(fd, header, 0, 16, 0) !== 16 || !header.equals(Buffer.from("SQLite format 3\0"))) {
            throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "ANN catalog corrupt; original preserved");
          }
        } finally { fs.closeSync(fd); }
      }
    }
    const opened = openKnowledgeCacheDatabase(Database, options.dbPath, 1, db => {
      db.transaction(() => {
        assertCacheSchema(db, 1);
        const version = Number(db.pragma("user_version", { simple: true }));
        if (version === 0) {
        db.exec(`CREATE TABLE ann_variants (
          vector_index_variant_id TEXT PRIMARY KEY,
          model_key TEXT NOT NULL,
          dimensions INTEGER NOT NULL,
          chunk_fingerprint TEXT NOT NULL,
          vector_count INTEGER NOT NULL,
          index_format_version INTEGER NOT NULL,
          file_name TEXT NOT NULL UNIQUE,
          status TEXT NOT NULL CHECK(status IN ('building', 'ready', 'failed')),
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );`);
        db.pragma("user_version = 1");
        }
      }).immediate();
      if (db.pragma("quick_check", { simple: true }) !== "ok") throw new Error("ann_quick_check_failed");
    });
    this.db = opened.db;
    this.closeGuard = opened.closeGuard;
  }

  private assertWritableSchema(): void {
    const version = Number(this.db.pragma("user_version", { simple: true }));
    if (version > 1) throw new KnowledgeError("KNOWLEDGE_SCHEMA_NEWER", "ANN schema newer; old writer refused");
    if (!Number.isSafeInteger(version) || version < 0) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "ANN schema invalid");
  }

  get(vectorIndexVariantId: string): KnowledgeAnnVariant | null {
    const row = this.db.prepare("SELECT * FROM ann_variants WHERE vector_index_variant_id = ?").get(vectorIndexVariantId);
    if (!row) return null;
    if (row.file_name !== knowledgeAnnFileName(row.model_key, row.vector_index_variant_id)
      || !Number.isSafeInteger(row.dimensions) || row.dimensions <= 0
      || !Number.isSafeInteger(row.vector_count) || row.vector_count < 0
      || !Number.isSafeInteger(row.index_format_version) || row.index_format_version <= 0
      || !["building", "ready", "failed"].includes(row.status) || typeof row.chunk_fingerprint !== "string") {
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "ANN metadata is corrupt");
    }
    return { vectorIndexVariantId: row.vector_index_variant_id, modelKey: row.model_key,
      dimensions: row.dimensions, chunkFingerprint: row.chunk_fingerprint, vectorCount: row.vector_count,
      indexFormatVersion: row.index_format_version, fileName: row.file_name, status: row.status,
      createdAt: row.created_at, updatedAt: row.updated_at };
  }

  begin(input: Omit<KnowledgeAnnVariant, "status" | "createdAt" | "updatedAt" | "indexFormatVersion" | "fileName">): KnowledgeAnnVariant {
    return this.db.transaction(() => {
      this.assertWritableSchema();
    const fileName = knowledgeAnnFileName(input.modelKey, input.vectorIndexVariantId), now = this.now();
    if (!Number.isSafeInteger(input.dimensions) || input.dimensions <= 0
      || !Number.isSafeInteger(input.vectorCount) || input.vectorCount <= 0 || !input.chunkFingerprint) {
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "ANN build metadata is invalid");
    }
    this.db.prepare(`INSERT INTO ann_variants (
      vector_index_variant_id, model_key, dimensions, chunk_fingerprint, vector_count,
      index_format_version, file_name, status, created_at, updated_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, 'building', ?, ?)
    ON CONFLICT(vector_index_variant_id) DO UPDATE SET
      model_key = excluded.model_key, dimensions = excluded.dimensions, chunk_fingerprint = excluded.chunk_fingerprint,
      vector_count = excluded.vector_count, index_format_version = excluded.index_format_version,
      file_name = excluded.file_name, status = 'building', updated_at = excluded.updated_at
    `).run(input.vectorIndexVariantId, input.modelKey, input.dimensions, input.chunkFingerprint, input.vectorCount,
      KNOWLEDGE_ANN_INDEX_FORMAT_VERSION, fileName, now, now);
    return this.get(input.vectorIndexVariantId)!;
    }).immediate();
  }

  markReady(vectorIndexVariantId: string): void {
    return this.db.transaction(() => {
      this.assertWritableSchema();
    if (this.db.prepare("UPDATE ann_variants SET status = 'ready', updated_at = ? WHERE vector_index_variant_id = ? AND status = 'building'")
      .run(this.now(), vectorIndexVariantId).changes !== 1) throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "ANN build is not active");
    }).immediate();
  }
  markFailed(vectorIndexVariantId: string): void {
    return this.db.transaction(() => {
      this.assertWritableSchema();
    this.db.prepare("UPDATE ann_variants SET status = 'failed', updated_at = ? WHERE vector_index_variant_id = ?").run(this.now(), vectorIndexVariantId);
    }).immediate();
  }
  listInterrupted(): string[] {
    return this.db.prepare("SELECT vector_index_variant_id FROM ann_variants WHERE status = 'building' ORDER BY vector_index_variant_id")
      .all().map((row: any) => row.vector_index_variant_id);
  }
  close(): void { closeKnowledgeCacheDatabase(this.db, this.closeGuard); }
}
