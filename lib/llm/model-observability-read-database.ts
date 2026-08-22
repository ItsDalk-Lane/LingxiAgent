/**
 * model-observability-read-database.ts — 历史只读查询入口（Phase 8 §六/七）。
 *
 * 不要用 openModelObservabilityDatabase() 做历史查询：它带 mkdir/create/
 * migration/PRAGMA write 职责。read side 契约：
 *   - fileMustExist：DB 不存在 → status="absent"，**绝不创建文件**（§九十二：
 *     打开 Usage 页不能在磁盘建出 model-observability/）；
 *   - readonly + query_only=ON：物理只读（连误写都不可能）；
 *   - 不 CREATE TABLE / migration / VACUUM / 改 user_version（§六：读取绝不
 *     迁移；v1 历史库保持 v1 继续可读）；
 *   - busy_timeout=5000：与 active writer（WAL）短暂并发安全；
 *   - schema version 识别 + supportedReadVersions 闭集（§七）：v1 读 Phase 7
 *     字段，v2 才有 accounting projection；更高版本 → unavailable。
 */

import fs from "fs";
import { loadBetterSqliteDatabase } from "./model-observability-schema.ts";

export const MODEL_OBSERVABILITY_READ_SUPPORTED_VERSIONS: readonly number[] = [1, 2];

export type ModelObservabilityReadDatabaseStatus =
  | "ready"
  | "absent"
  | "schema_newer"
  | "unreadable";

export type ModelObservabilityReadDatabase = {
  status: ModelObservabilityReadDatabaseStatus;
  db: any | null;
  schemaVersion: number | null;
  reasonCode: string | null;
};

/**
 * 打开只读连接。所有失败路径都返回显式 status，不抛异常、不创建文件。
 * 调用方负责 close（返回 ready 时 db 非 null）。
 */
export function openModelObservabilityReadDatabase(
  dbPath: string,
  options: { Database?: any } = {},
): ModelObservabilityReadDatabase {
  const absent = (): ModelObservabilityReadDatabase => ({
    status: "absent",
    db: null,
    schemaVersion: null,
    reasonCode: "database_absent",
  });
  if (typeof dbPath !== "string" || !dbPath.trim()) {
    return { status: "unreadable", db: null, schemaVersion: null, reasonCode: "invalid_path" };
  }
  try {
    if (!fs.existsSync(dbPath)) return absent();
  } catch {
    return absent();
  }
  const Database = options.Database || loadBetterSqliteDatabase();
  let db: any = null;
  try {
    // readonly=true：better-sqlite3 以只读模式打开；fileMustExist：不存在时报错
    // 而不是建空库（双保险，existsSync 已挡了一层——TOCTOU 窗口内也安全）。
    db = new Database(dbPath, { readonly: true, fileMustExist: true });
    db.pragma("busy_timeout = 5000");
    db.pragma("query_only = ON");
    // WAL readonly 连接读取 committed 状态；不触碰 journal/synchronous。
    const version = Number(db.pragma("user_version", { simple: true }));
    if (version > Math.max(...MODEL_OBSERVABILITY_READ_SUPPORTED_VERSIONS)) {
      try { db.close(); } catch { /* best-effort */ }
      return { status: "schema_newer", db: null, schemaVersion: version, reasonCode: "schema_newer" };
    }
    if (!MODEL_OBSERVABILITY_READ_SUPPORTED_VERSIONS.includes(version)) {
      try { db.close(); } catch { /* best-effort */ }
      return { status: "unreadable", db: null, schemaVersion: version, reasonCode: "schema_version_unreadable" };
    }
    return { status: "ready", db, schemaVersion: version, reasonCode: null };
  } catch (error) {
    try { db?.close?.(); } catch { /* best-effort */ }
    return {
      status: "unreadable",
      db: null,
      schemaVersion: null,
      reasonCode: "open_failed",
    };
  }
}

/** read side 是否具备 accounting projection 表（§七：v1 → unavailable）。 */
export function readDatabaseHasAccountingProjection(read: ModelObservabilityReadDatabase): boolean {
  if (read.status !== "ready" || !read.db) return false;
  try {
    const row = read.db
      .prepare(`SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'model_call_usage'`)
      .get();
    return row != null;
  } catch {
    return false;
  }
}
