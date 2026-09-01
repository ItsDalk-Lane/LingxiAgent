/**
 * model-observability-schema.ts — Durable Model Observatory 的 SQLite schema
 * 与 migration contract（Phase 7）。
 *
 * 一个 LINGXI_HOME → 一个全局 observability store：
 *
 *     {lingxiHome}/model-observability/observability.sqlite (+ -wal / -shm)
 *     {lingxiHome}/model-observability/blobs/{shard}/{blobId}.bin
 *
 * 设计决策（MODEL_OBSERVABILITY_STORAGE_AUDIT.md）：
 *   - 单 DB 逻辑分表（traces / model_calls / model_attempts / payload_records /
 *     blob_objects / payload_blob_refs / observability_meta）：call/attempt/
 *     payload/blob ref 的 retention 与 GC 需要事务一致性（任务书 §九）。
 *   - PRAGMA user_version 管理 schema 版本；每次升级都必须显式
 *     migration，并在同一个 transaction 内单调推进。
 *   - 未知高版本 / migration 失败 / 损坏：**禁用 persistence、保留数据库、
 *     主程序正常继续**——observability 永远不能阻止主程序启动（§二十七～二十九）。
 *   - WAL + busy_timeout：同 LINGXI_HOME 多进程短暂并发安全（audit Q3）。
 *   - secure_delete=ON：敏感正文删除时清零 freelist 页（删除只发生在
 *     maintenance 路径，不进模型热路径；代价记录在 audit pragma 表）。
 *   - auto_vacuum=INCREMENTAL：retention 删除后 incremental_vacuum 收缩文件。
 *   - 无 FOREIGN KEY：显式容忍 out-of-order persistence / partial crash
 *     （§二十四）；关联完整性由 call/attempt shell upsert 与读侧解释承担。
 */

import fs from "fs";
import path from "path";
import { createRequire } from "module";

export const MODEL_OBSERVABILITY_SCHEMA_VERSION = 4;

/**
 * read side 支持的 schema 版本闭集（Phase 8 §七）：v1 历史库不迁移也可读
 * （accounting projection 标 unavailable）；v2 起有 model_call_usage；
 * v3 起有运行时显式 usage correlation 事实；v4 增加不含正文的来源名称快照。
 */
export const MODEL_OBSERVABILITY_SUPPORTED_READ_VERSIONS: readonly number[] = [1, 2, 3, 4];

/** store 目录约定（audit Q1 决策）。 */
export const MODEL_OBSERVABILITY_DIR_NAME = "model-observability";
export const MODEL_OBSERVABILITY_DB_FILE_NAME = "observability.sqlite";
export const MODEL_OBSERVABILITY_BLOBS_DIR_NAME = "blobs";

export function modelObservabilityDbPath(lingxiHome: string): string {
  return path.join(lingxiHome, MODEL_OBSERVABILITY_DIR_NAME, MODEL_OBSERVABILITY_DB_FILE_NAME);
}

export function modelObservabilityBlobsRoot(lingxiHome: string): string {
  return path.join(lingxiHome, MODEL_OBSERVABILITY_DIR_NAME, MODEL_OBSERVABILITY_BLOBS_DIR_NAME);
}

const require = createRequire(import.meta.url);
let BetterSqliteDatabase: any = null;

export function loadBetterSqliteDatabase(): any {
  if (!BetterSqliteDatabase) {
    const mod = require("better-sqlite3");
    BetterSqliteDatabase = mod?.default || mod;
  }
  return BetterSqliteDatabase;
}

/**
 * schema 打开/初始化失败的安全 reasonCode 闭集（coordinator 的
 * storeDisabledReasonCode；绝不包含内容/路径细节之外的信息）。
 */
export const MODEL_OBSERVABILITY_STORE_DISABLED_REASONS = [
  "disabled_by_policy",
  "schema_newer",
  "migration_failed",
  "database_corrupt",
  "open_failed",
] as const;
export type ModelObservabilityStoreDisabledReason =
  typeof MODEL_OBSERVABILITY_STORE_DISABLED_REASONS[number];

export class ModelObservabilitySchemaError extends Error {
  declare reasonCode: ModelObservabilityStoreDisabledReason;

  constructor(reasonCode: ModelObservabilityStoreDisabledReason, message: string, cause?: unknown) {
    super(message);
    this.name = "ModelObservabilitySchemaError";
    this.reasonCode = reasonCode;
    if (cause !== undefined) (this as any).cause = cause;
  }
}

/** SQLite 文件损坏类错误码（best-effort 识别，不做穷举承诺）。 */
const CORRUPT_SQLITE_CODES = new Set(["SQLITE_NOTADB", "SQLITE_CORRUPT", "SQLITE_FORMAT"]);

/* ── v1 DDL ──────────────────────────────────────────────────────────── */

const V1_DDL = `
CREATE TABLE observability_meta (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);

CREATE TABLE traces (
  trace_id TEXT PRIMARY KEY,
  origin TEXT,
  first_seen_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  call_count INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE model_calls (
  call_id TEXT PRIMARY KEY,
  trace_id TEXT,
  parent_call_id TEXT,
  provider TEXT,
  model_id TEXT,
  api TEXT,
  subsystem TEXT,
  operation TEXT,
  surface TEXT,
  trigger TEXT,
  attribution_kind TEXT,
  session_id TEXT,
  session_path TEXT,
  conversation_id TEXT,
  conversation_type TEXT,
  agent_id TEXT,
  child_agent_id TEXT,
  child_session_id TEXT,
  child_session_path TEXT,
  task_id TEXT,
  call_purpose TEXT,
  started_at TEXT,
  semantic_completed_at TEXT,
  ended_at TEXT,
  terminal_status TEXT,
  error_name TEXT,
  error_code TEXT,
  input_shape TEXT,
  provenance_precision TEXT,
  provenance_section_count INTEGER,
  provenance_categories_json TEXT,
  provenance_opaque_count INTEGER,
  attribution_json TEXT,
  source_json TEXT,
  safe_details_json TEXT,
  persistence_completeness TEXT NOT NULL DEFAULT 'partial',
  interrupted_by_restart INTEGER NOT NULL DEFAULT 0,
  payload_availability TEXT
);

CREATE TABLE model_attempts (
  attempt_id TEXT PRIMARY KEY,
  call_id TEXT NOT NULL,
  started_at TEXT,
  request_prepared_at TEXT,
  response_received_at TEXT,
  error_at TEXT,
  provider_request_id TEXT,
  http_status INTEGER,
  attempt_visibility TEXT,
  provider_wire_visibility TEXT,
  error_name TEXT,
  error_code TEXT,
  safe_details_json TEXT
);

CREATE TABLE payload_records (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  call_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  attempt_id TEXT,
  provider_request_ordinal INTEGER,
  captured_at TEXT NOT NULL,
  visibility TEXT NOT NULL,
  fidelity TEXT NOT NULL,
  sanitization_status TEXT NOT NULL,
  redacted INTEGER NOT NULL,
  truncated INTEGER NOT NULL,
  degraded INTEGER NOT NULL,
  payload_json TEXT,
  semantic_input_provenance_json TEXT,
  provider_request_provenance_json TEXT,
  record_char_count INTEGER
);

CREATE TABLE blob_objects (
  blob_id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  byte_length INTEGER NOT NULL,
  media_type TEXT,
  state TEXT NOT NULL,
  relative_path TEXT NOT NULL
);

CREATE TABLE payload_blob_refs (
  payload_record_id INTEGER NOT NULL,
  blob_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (payload_record_id, blob_id)
);

CREATE INDEX idx_traces_last_seen ON traces(last_seen_at);
CREATE INDEX idx_model_calls_trace ON model_calls(trace_id);
CREATE INDEX idx_model_calls_started ON model_calls(started_at);
CREATE INDEX idx_model_calls_model ON model_calls(provider, model_id);
CREATE INDEX idx_model_calls_subsystem ON model_calls(subsystem, operation);
CREATE INDEX idx_model_calls_terminal ON model_calls(terminal_status);
CREATE INDEX idx_model_calls_attribution_kind ON model_calls(attribution_kind);
CREATE INDEX idx_model_calls_session ON model_calls(session_id);
CREATE INDEX idx_model_calls_agent ON model_calls(agent_id);
CREATE INDEX idx_model_calls_task ON model_calls(task_id);
CREATE INDEX idx_model_attempts_call ON model_attempts(call_id);
CREATE INDEX idx_payload_records_call ON payload_records(call_id);
CREATE INDEX idx_payload_records_call_kind ON payload_records(call_id, kind);
CREATE INDEX idx_payload_records_attempt ON payload_records(attempt_id);
CREATE INDEX idx_payload_records_call_ordinal ON payload_records(call_id, provider_request_ordinal);
CREATE INDEX idx_payload_blob_refs_blob ON payload_blob_refs(blob_id);
CREATE INDEX idx_blob_objects_state ON blob_objects(state);
CREATE INDEX idx_blob_objects_created ON blob_objects(created_at);
`;

/* ── v2 DDL（Phase 8：Durable Accounting Projection，任务书 §十/十一）─────
 *
 * model_call_usage 是 Usage Ledger 的 read-optimized durable projection，
 * 不是 Ledger replacement（§十二）：Provider → Usage Ledger（truth）→
 * projection。只存 numeric accounting + status + identity；**绝不存**
 * ledger error.message / error.name（Observable Metadata Safe Contract，
 * §十一）。写入幂等（PK = model_call_id，同 callId 重复 upsert 不产生
 * duplicate，§十三/十四）；retention 随对应 trace 删除（§十六，retention.ts）。
 */
const V2_DDL = `
CREATE TABLE model_call_usage (
  model_call_id TEXT PRIMARY KEY,
  usage_request_id TEXT,
  started_at TEXT,
  ended_at TEXT,
  duration_ms INTEGER,
  usage_status TEXT NOT NULL,
  input_total_tokens INTEGER,
  input_uncached_tokens INTEGER,
  output_total_tokens INTEGER,
  reasoning_tokens INTEGER,
  cache_read_tokens INTEGER,
  cache_write_tokens INTEGER,
  cache_miss_tokens INTEGER,
  cache_hit INTEGER,
  cache_created INTEGER,
  cache_hit_ratio REAL,
  total_tokens INTEGER,
  cost_total REAL,
  raw_usage_shape TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_model_call_usage_status ON model_call_usage(usage_status);
CREATE INDEX idx_model_calls_conversation ON model_calls(conversation_id);
`;

/* ── v3 DDL（Phase 10.1：explicit usage correlation truth）─────────
 *
 * 缺 usage row 可能是投影丢失、历史不完整或真的无法关联，不能由
 * Query 猜测。该列只允许运行时明确写入 not_correlated；NULL 表示
 * 没有这个明确事实。
 */
const V3_DDL = `
ALTER TABLE model_calls ADD COLUMN usage_correlation_state TEXT
  CHECK (usage_correlation_state IS NULL OR usage_correlation_state = 'not_correlated');
`;

/* ── v4 DDL：业务来源名称快照（不保存消息正文）──────────────────────── */
const V4_DDL = `
CREATE TABLE source_identity_snapshots (
  kind TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  title TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (kind, entity_id)
);
CREATE INDEX idx_source_identity_snapshots_updated
  ON source_identity_snapshots(updated_at);
`;

/**
 * 打开（必要时创建）observability 数据库并应用到受支持 schema。
 *
 * 失败语义（§二十七～二十九）：抛 ModelObservabilitySchemaError（带 reasonCode），
 * 绝不删除/重建数据库；调用方据此禁用 persistence 而不是让主程序失败。
 */
export function openModelObservabilityDatabase(
  dbPath: string,
  options: { Database?: any } = {},
): any {
  const Database = options.Database || loadBetterSqliteDatabase();
  fs.mkdirSync(path.dirname(dbPath), { recursive: true });
  let db: any = null;
  try {
    db = new Database(dbPath);
    db.pragma("busy_timeout = 5000");
    db.pragma("journal_mode = WAL");
    db.pragma("synchronous = NORMAL");
    db.pragma("secure_delete = ON");
    db.pragma("foreign_keys = OFF");
    db.pragma("trusted_schema = OFF");

    const current = Number(db.pragma("user_version", { simple: true }));
    if (current > MODEL_OBSERVABILITY_SCHEMA_VERSION) {
      // 未来版本写的库：保留数据，禁用本进程持久化（绝不 DROP/重建）。
      throw new ModelObservabilitySchemaError(
        "schema_newer",
        `observability schema ${current} is newer than supported ${MODEL_OBSERVABILITY_SCHEMA_VERSION}`,
      );
    }
    if (current === 0) {
      // auto_vacuum 必须在建表之前设置。
      db.pragma("auto_vacuum = INCREMENTAL");
    }
    migrateModelObservabilitySchema(db, current);
    return db;
  } catch (error) {
    try {
      db?.close?.();
    } catch {
      // 保留原始初始化错误；清理失败是次要的。
    }
    if (error instanceof ModelObservabilitySchemaError) throw error;
    const code = String((error as any)?.code ?? "");
    if (CORRUPT_SQLITE_CODES.has(code)) {
      throw new ModelObservabilitySchemaError(
        "database_corrupt",
        "observability database appears corrupt; persistence disabled, file preserved",
        error,
      );
    }
    throw new ModelObservabilitySchemaError(
      "open_failed",
      `observability database could not be opened: ${code || (error as any)?.message || "unknown error"}`,
      error,
    );
  }
}

/**
 * 显式 migration（§二十六）：v(n) → v(n+1) 单调推进，全部在一个 transaction 内。
 * v1 是首个版本（0 = 全新库 → 建表）。失败 → SQLite 自动 rollback → 包装为
 * migration_failed（调用方禁用 store）。
 */
export function migrateModelObservabilitySchema(db: any, currentVersion: number): void {
  const target = MODEL_OBSERVABILITY_SCHEMA_VERSION;
  if (currentVersion >= target) return;
  try {
    db.transaction(() => {
      let version = currentVersion;
      while (version < target) {
        switch (version) {
          case 0:
            db.exec(V1_DDL);
            break;
          case 1:
            // v1 → v2：只新增 model_call_usage + conversation index；
            // v1 既有行不动（§九十四 data preservation）。
            db.exec(V2_DDL);
            break;
          case 2:
            // v2 → v3：只新增闭集事实列；既有 call/usage 行原样保留。
            db.exec(V3_DDL);
            break;
          case 3:
            // v3 → v4：新增不含正文的名称快照；既有调用与载荷原样保留。
            db.exec(V4_DDL);
            break;
          default:
            throw new Error(`no migration step from observability schema ${version}`);
        }
        version += 1;
      }
      db.pragma(`user_version = ${target}`);
    })();
  } catch (error) {
    throw new ModelObservabilitySchemaError(
      "migration_failed",
      `observability schema migration v${currentVersion}→v${target} failed`,
      error,
    );
  }
}

/** 只读探测当前 user_version（诊断/测试用；不修改数据库）。 */
export function readModelObservabilitySchemaVersion(db: any): number {
  return Number(db.pragma("user_version", { simple: true }));
}

/** maintenance 路径：retention 删除后收缩数据库文件（§八十八）。 */
export function compactModelObservabilityDatabase(db: any): void {
  try {
    db.pragma("wal_checkpoint(TRUNCATE)");
    db.pragma("incremental_vacuum");
  } catch {
    // 收缩是 best-effort：失败只影响磁盘占用，不影响正确性。
  }
}
