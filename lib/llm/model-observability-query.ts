/**
 * model-observability-query.ts — Unified Query Service（Phase 8 唯一 Query
 * Truth，§十七）。
 *
 * Server route 只调用本服务；未来 UI 只调用 API；export 复用同一 filter
 * contract。本层纪律：
 *   - read-only：绝不 DELETE/UPDATE/INSERT（§一百二十七）；accounting live
 *     projection 位于 persistence writer（accounting-projection.ts）。
 *   - 不 flush writer（§五十）：读 committed durable state；pending 队列经
 *     health 的 queue counts 表达。
 *   - 独立 readonly WAL connection（audit Q12）：与 active writer 并发安全；
 *     reconfigure/DB 重建后 lazy reopen（§九十一）。
 *   - query 不隐式创建 store（§九十二）：DB 不存在 → absent。
 *   - 维度/排序闭集映射 SQL（§二十一），值全绑定；payload 正文只在 exact
 *     retrieval 返回（§三十五），列表/详情默认 metadata。
 *   - 不读 blob 文件、不调用模型 runtime（§八十五/八十六）；OPAQUE/
 *     UNAVAILABLE/METADATA_ONLY 不升级（§八十七）。
 */

import fs from "fs";
import { modelObservabilityDbPath } from "./model-observability-schema.ts";
import { resolveExistingModelObservabilityBlobPath } from "./model-observability-blob-store.ts";
import { openModelObservabilityReadDatabase } from "./model-observability-read-database.ts";
import { resolveModelObservabilitySourceIdentity } from "./model-observability-source-identity.ts";
import { MODEL_OBSERVABILITY_BLOB_ID_PATTERN } from "../../shared/model-observability-api-contract.ts";
import {
  decodeModelObservabilityCallCursor,
  decodeModelObservabilityTraceCursor,
  encodeModelObservabilityCallCursor,
  encodeModelObservabilityTraceCursor,
  type ModelObservabilityAggregateResult,
  type ModelObservabilityCallListItem,
  type ModelObservabilityCallPage,
  type ModelObservabilityDataCompleteness,
  type ModelObservabilityGroupBucket,
  type ModelObservabilityGroupMetrics,
  type ModelObservabilityGroupByDimension,
  type ModelObservabilityNormalizedFilter,
  type ModelObservabilityMultiValueField,
  type ModelObservabilityPayloadAvailability,
  type ModelObservabilityTraceListItem,
  type ModelObservabilityTracePage,
  type ModelObservabilityUsageAvailability,
  type NormalizedModelObservabilityAggregateQuery,
  type NormalizedModelObservabilityQuery,
  type NormalizedModelObservabilityTraceQuery,
} from "./model-observability-query-types.ts";

export type ModelObservabilityQueryErrorCode =
  | "absent"
  | "unavailable"
  | "invalid_cursor"
  | "invalid_blob_id"
  | "not_found"
  | "blob_missing"
  | "query_failed";

export type ModelObservabilityQueryResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: { code: ModelObservabilityQueryErrorCode; message: string; reasonCode?: string } };

/* ── 内部可识别异常（runQuery 转义为显式 error code，绝不 500）──────── */

class CursorError extends Error {}
class NotFoundError extends Error {
  declare reasonCode: string;
  constructor(message: string, reasonCode: string) {
    super(message);
    this.reasonCode = reasonCode;
  }
}
/** §一百二十九：DB ref 存在但磁盘文件缺失——显式 blob_missing（绝不 500）。 */
class BlobMissingError extends Error {}
class DateBucketTooComplexError extends Error {}

/* ── Filter → SQL（闭集映射；值全绑定）───────────────────────────────── */

type SqlWithParams = { sql: string; params: unknown[] };

const MULTI_FIELD_COLUMNS: Record<ModelObservabilityMultiValueField, string> = {
  provider: "provider",
  modelId: "model_id",
  api: "api",
  subsystem: "subsystem",
  operation: "operation",
  surface: "surface",
  trigger: "trigger",
  callPurpose: "call_purpose",
  attributionKind: "attribution_kind",
  sessionId: "session_id",
  sessionPath: "session_path",
  conversationId: "conversation_id",
  conversationType: "conversation_type",
  agentId: "agent_id",
  childAgentId: "child_agent_id",
  childSessionId: "child_session_id",
  taskId: "task_id",
  inputShape: "input_shape",
  provenancePrecision: "provenance_precision",
  terminalStatus: "terminal_status",
  payloadAvailability: "payload_availability",
};

/**
 * 构建 model_calls 的 WHERE 片段（列名来自闭集映射，值全绑定）。同字段多值
 * OR、跨字段 AND（§二十）。payloadAvailability 的 present/unknown 由 payload
 * row 存在性派生（§三十七），不能只看列。
 */
function buildCallFilterSql(
  filter: ModelObservabilityNormalizedFilter,
  alias: string,
): SqlWithParams {
  const clauses: string[] = [];
  const params: unknown[] = [];
  const col = (name: string) => `${alias}.${name}`;

  // since inclusive / until exclusive（§四十四，全接口统一）。
  if (filter.since) {
    clauses.push(`${col("started_at")} >= ?`);
    params.push(filter.since);
  }
  if (filter.until) {
    clauses.push(`${col("started_at")} < ?`);
    params.push(filter.until);
  }
  const equals: Array<[string | null, string]> = [
    [filter.traceId, "trace_id"],
    [filter.parentCallId, "parent_call_id"],
    [filter.callId, "call_id"],
  ];
  for (const [value, column] of equals) {
    if (value) {
      clauses.push(`${col(column)} = ?`);
      params.push(value);
    }
  }
  for (const [field, values] of Object.entries(filter.multi ?? {}) as Array<
    [ModelObservabilityMultiValueField, string[]]
  >) {
    if (field === "terminalStatus") {
      const alternatives: string[] = [];
      const concrete = values.filter((v) => v !== "incomplete");
      if (concrete.length > 0) {
        alternatives.push(`${col("terminal_status")} IN (${concrete.map(() => "?").join(",")})`);
        params.push(...concrete);
      }
      if (values.includes("incomplete")) {
        alternatives.push(`(${col("terminal_status")} IS NULL OR ${col("terminal_status")} = '')`);
      }
      if (alternatives.length > 0) clauses.push(`(${alternatives.join(" OR ")})`);
      continue;
    }
    if (field === "payloadAvailability") {
      const alternatives: string[] = [];
      const explicitStates = ["dropped", "expired", "not_captured"];
      const noExplicitState = `(${col("payload_availability")} IS NULL OR ${col("payload_availability")} NOT IN ('dropped','expired','not_captured'))`;
      for (const value of values) {
        if (explicitStates.includes(value)) {
          alternatives.push(`${col("payload_availability")} = ?`);
          params.push(value);
        } else if (value === "present") {
          alternatives.push(`(${noExplicitState} AND EXISTS (SELECT 1 FROM payload_records pr WHERE pr.call_id = ${col("call_id")}))`);
        } else if (value === "unknown") {
          alternatives.push(`(${noExplicitState} AND NOT EXISTS (SELECT 1 FROM payload_records pr WHERE pr.call_id = ${col("call_id")}))`);
        }
      }
      if (alternatives.length > 0) clauses.push(`(${alternatives.join(" OR ")})`);
      continue;
    }
    const column = MULTI_FIELD_COLUMNS[field];
    clauses.push(`${col(column)} IN (${values.map(() => "?").join(",")})`);
    params.push(...values);
  }
  if (filter.interruptedByRestart != null) {
    clauses.push(`${col("interrupted_by_restart")} = ?`);
    params.push(filter.interruptedByRestart ? 1 : 0);
  }
  if (filter.hasPayload != null) {
    clauses.push(
      filter.hasPayload
        ? `EXISTS (SELECT 1 FROM payload_records pr WHERE pr.call_id = ${col("call_id")})`
        : `NOT EXISTS (SELECT 1 FROM payload_records pr WHERE pr.call_id = ${col("call_id")})`,
    );
  }
  return { sql: clauses.length > 0 ? clauses.join(" AND ") : "1 = 1", params };
}

/**
 * keyset 续页条件（§二十五：started_at DESC, call_id DESC；NULL 最后）。
 * 跨越 NULL 边界时（lastStartedAt 非 null）：NULL 行整体排在所有非 null 行
 * 之后，因此 NULL 区域不再受 call_id 上界约束；进入 NULL 区域后
 * （lastStartedAt null）才用 call_id 续页。
 */
function buildCallKeysetSql(cursor: { lastStartedAt: string | null; lastCallId: string }, alias: string): SqlWithParams {
  const col = (name: string) => `${alias}.${name}`;
  if (cursor.lastStartedAt === null) {
    return {
      sql: `(${col("started_at")} IS NULL AND ${col("call_id")} < ?)`,
      params: [cursor.lastCallId],
    };
  }
  return {
    sql: `(${col("started_at")} IS NULL
       OR (${col("started_at")} IS NOT NULL AND (${col("started_at")} < ? OR (${col("started_at")} = ? AND ${col("call_id")} < ?))))`,
    params: [cursor.lastStartedAt, cursor.lastStartedAt, cursor.lastCallId],
  };
}

const CALL_ORDER_SQL = "(started_at IS NULL) ASC, started_at DESC, call_id DESC";

/* ── row mappers ────────────────────────────────────────────────────── */

function textOrNull(value: unknown): string | null {
  return typeof value === "string" && value ? value : null;
}

function finiteNumberOrNull(value: unknown): number | null {
  if (value === null || value === undefined || value === "") return null;
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}

function finiteIntegerOrNull(value: unknown): number | null {
  const n = finiteNumberOrNull(value);
  return n !== null && Number.isInteger(n) ? n : null;
}

function boolFlag(value: unknown): boolean {
  return value === 1 || value === true;
}

const OUTPUT_BUDGET_OWNERSHIPS = new Set([
  "absent",
  "user-explicit",
  "system-explicit",
  "hana-chat-default",
  "sdk-derived",
]);

/**
 * 从 attempt.safe_details_json（provider_request_prepared 的持久 details）中
 * 提取 Output Budget Fact。数据是落盘后的旧版本或手改文件：形状不符时显式
 * 返回 null，不按快照伪造（与 sanitize 系列同款纪律）。
 */
function extractOutputBudgetFact(safeDetailsJson: unknown): ModelObservabilityOutputBudgetFact | null {
  if (typeof safeDetailsJson !== "string" || !safeDetailsJson) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(safeDetailsJson);
  } catch {
    return null;
  }
  const fact = parsed && typeof parsed === "object" && !Array.isArray(parsed)
    ? (parsed as Record<string, unknown>).outputBudget
    : undefined;
  if (!fact || typeof fact !== "object" || Array.isArray(fact)) return null;
  const source = fact as Record<string, unknown>;
  if (
    (source.field !== null && typeof source.field !== "string")
    || !OUTPUT_BUDGET_OWNERSHIPS.has(String(source.ownership))
    || (source.composition !== "included" && source.composition !== "separate")
  ) {
    return null;
  }
  for (const key of ["value", "chatDefault", "declaredMaxOutput"] as const) {
    if (source[key] !== null && !Number.isFinite(Number(source[key]))) return null;
  }
  return {
    field: typeof source.field === "string" ? source.field : null,
    value: source.value === null ? null : Number(source.value),
    composition: source.composition,
    ownership: source.ownership as ModelObservabilityOutputBudgetFact["ownership"],
    chatDefault: source.chatDefault === null ? null : Number(source.chatDefault),
    declaredMaxOutput: source.declaredMaxOutput === null ? null : Number(source.declaredMaxOutput),
  };
}

function durationMs(startedAt: string | null, endedAt: string | null): number | null {
  if (!startedAt || !endedAt) return null;
  const start = new Date(startedAt).getTime();
  const end = new Date(endedAt).getTime();
  if (!Number.isFinite(start) || !Number.isFinite(end)) return null;
  return Math.max(0, end - start);
}

function parseCategories(raw: unknown): { values: string[]; state: "present" | "absent" | "corrupt" } {
  if (typeof raw !== "string" || !raw) return { values: [], state: "absent" };
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return { values: [], state: "corrupt" };
    return {
      values: parsed.filter((category): category is string => typeof category === "string"),
      state: "present",
    };
  } catch {
    return { values: [], state: "corrupt" };
  }
}

/** payload_availability 真相枚举（§三十七：NULL 不折叠，无 payload row → unknown）。 */
function payloadAvailabilityOf(columnValue: unknown, recordCount: number): ModelObservabilityPayloadAvailability {
  const value = textOrNull(columnValue);
  if (value === "expired" || value === "dropped" || value === "not_captured") return value;
  if (recordCount > 0) return "present";
  return "unknown";
}

/* ── 公共 DTO（route / export 消费）───────────────────────────────────
 *
 * Drill-down/health DTO 定义在 shared/model-observability-api-contract.ts
 * （browser-safe 单一事实源，Phase 9 §九），此处 re-export 保持既有 import
 * 站点不变。
 */
export type {
  ModelObservabilityQueryHealth,
  ModelObservabilityPayloadRecordMetadata,
  ModelObservabilityPayloadRecordDetail,
  ModelObservabilityCallRef,
  ModelObservabilityAttemptSummary,
  ModelObservabilityCallDetail,
  ModelObservabilityTraceDetail,
} from "../../shared/model-observability-api-contract.ts";
import type {
  ModelObservabilityQueryHealth,
  ModelObservabilityPayloadRecordMetadata,
  ModelObservabilityPayloadRecordDetail,
  ModelObservabilityCallRef,
  ModelObservabilityAttemptSummary,
  ModelObservabilityOutputBudgetFact,
  ModelObservabilityCallDetail,
  ModelObservabilityTraceDetail,
} from "../../shared/model-observability-api-contract.ts";

type CachedReader = {
  db: any;
  schemaVersion: number;
  hasAccounting: boolean;
  fileMtimeMs: number;
  fileSize: number;
};

export function createModelObservabilityQueryService({ lingxiHome }: { lingxiHome: string }) {
  const dbPath = modelObservabilityDbPath(lingxiHome);
  let cached: CachedReader | null = null;
  let closed = false;

  function closeCached(): void {
    if (cached) {
      try { cached.db.close(); } catch { /* best-effort */ }
      cached = null;
    }
  }

  /** reconfigure/DB 重建后失效（§九十一）；下次查询 lazy reopen。 */
  function invalidate(): void {
    closeCached();
  }

  function close(): void {
    closed = true;
    closeCached();
  }

  /**
   * 打开（或复用）readonly 连接。文件 mtime/size 变化（writer 重建/reconfigure）
   * → reopen，绝不持有已 close 的 handle。
   */
  function openCurrent(): { ok: true; reader: CachedReader } | { ok: false; code: "absent" | "unavailable"; reasonCode: string } {
    if (closed) return { ok: false, code: "unavailable", reasonCode: "service_closed" };
    let stat: fs.Stats;
    try {
      stat = fs.statSync(dbPath);
    } catch {
      closeCached();
      return { ok: false, code: "absent", reasonCode: "database_absent" };
    }
    if (!stat.isFile()) {
      closeCached();
      return { ok: false, code: "absent", reasonCode: "database_absent" };
    }
    if (cached && cached.fileMtimeMs === stat.mtimeMs && cached.fileSize === stat.size) {
      return { ok: true, reader: cached };
    }
    closeCached();
    const opened = openModelObservabilityReadDatabase(dbPath);
    if (opened.status !== "ready" || !opened.db) {
      return {
        ok: false,
        code: opened.status === "absent" ? "absent" : "unavailable",
        reasonCode: opened.reasonCode ?? opened.status,
      };
    }
    let hasAccounting = false;
    try {
      hasAccounting = opened.db
        .prepare(`SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'model_call_usage'`)
        .get() != null;
    } catch {
      hasAccounting = false;
    }
    cached = {
      db: opened.db,
      schemaVersion: opened.schemaVersion ?? 0,
      hasAccounting,
      fileMtimeMs: stat.mtimeMs,
      fileSize: stat.size,
    };
    return { ok: true, reader: cached };
  }

  function fail<T>(code: ModelObservabilityQueryErrorCode, message: string, reasonCode?: string): ModelObservabilityQueryResult<T> {
    return { ok: false, error: { code, message, reasonCode } };
  }

  /** 查询执行包装：CursorError/NotFound 显式转义；SQL 抛错 → 失效缓存 + query_failed。 */
  function runQuery<T>(fn: (reader: CachedReader) => T): ModelObservabilityQueryResult<T> {
    let opened: ReturnType<typeof openCurrent>;
    try {
      opened = openCurrent();
    } catch {
      return fail("unavailable", "observability read database could not be opened", "open_failed");
    }
    if (opened.ok === false) {
      return fail(
        opened.code === "absent" ? "absent" : "unavailable",
        opened.code === "absent"
          ? "model observability store has not been initialized"
          : "model observability store is not readable",
        opened.reasonCode,
      );
    }
    try {
      return { ok: true, value: fn(opened.reader) };
    } catch (error) {
      if (error instanceof CursorError) {
        return fail("invalid_cursor", error.message);
      }
      if (error instanceof NotFoundError) {
        return fail("not_found", error.message, error.reasonCode);
      }
      if (error instanceof BlobMissingError) {
        return fail("blob_missing", error.message, "blob_file_missing");
      }
      if (error instanceof DateBucketTooComplexError) {
        return fail("query_failed", error.message, "date_bucket_segment_limit_exceeded");
      }
      // 连接可能已失效（writer close / 文件被替换）：失效缓存，下次查询重开。
      invalidate();
      if (process.env.LINGXI_OBS_QUERY_DEBUG) console.error("[obs-query]", error);
      return fail("query_failed", "observability query failed");
    }
  }

  /* ── 全局 drop counters（observability_meta 持久化 + DB 内事实）──────── */

  function unknownDataCompleteness(): ModelObservabilityDataCompleteness {
    return {
      status: "unknown",
      droppedTraceEvents: null,
      droppedPayloadRecords: null,
      droppedBlobs: null,
      interruptedByRestartCalls: null,
    };
  }

  function readDataCompleteness(db: any): ModelObservabilityDataCompleteness {
    try {
      const readMeta = (key: string): number => {
        const row = db.prepare(`SELECT value_json FROM observability_meta WHERE key = ?`).get(key);
        if (!row) return 0;
        const value = JSON.parse(row.value_json);
        if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
          throw new Error("invalid observability completeness counter");
        }
        return Math.floor(value);
      };
      const interruptedByRestartCalls = finiteIntegerOrNull(
        db.prepare(`SELECT COUNT(*) AS n FROM model_calls WHERE interrupted_by_restart = 1`).get()?.n,
      );
      if (interruptedByRestartCalls === null || interruptedByRestartCalls < 0) {
        throw new Error("invalid interrupted call count");
      }
      return {
        status: "known",
        droppedTraceEvents: readMeta("droppedTraceEvents"),
        droppedPayloadRecords: readMeta("droppedPayloadRecords"),
        droppedBlobs: readMeta("droppedBlobs"),
        interruptedByRestartCalls,
      };
    } catch {
      return unknownDataCompleteness();
    }
  }

  /* ── batch summaries（§四十六：一次 IN 查询，不做 N+1）──────────────── */

  function attemptCounts(db: any, callIds: string[]): Map<string, number> {
    const out = new Map<string, number>();
    if (callIds.length === 0) return out;
    const rows = db.prepare(
      `SELECT call_id, COUNT(*) AS n FROM model_attempts WHERE call_id IN (${callIds.map(() => "?").join(",")}) GROUP BY call_id`,
    ).all(...callIds);
    for (const row of rows) out.set(row.call_id, Number(row.n ?? 0));
    return out;
  }

  /** 最早一次 attempt 收到 provider 响应的时刻（响应到达事实；无事实为 null）。 */
  function attemptFirstResponses(db: any, callIds: string[]): Map<string, string> {
    const out = new Map<string, string>();
    if (callIds.length === 0) return out;
    const rows = db.prepare(
      `SELECT call_id, MIN(response_received_at) AS first_response_at
       FROM model_attempts
       WHERE call_id IN (${callIds.map(() => "?").join(",")})
         AND response_received_at IS NOT NULL AND response_received_at != ''
       GROUP BY call_id`,
    ).all(...callIds);
    for (const row of rows) {
      const value = textOrNull(row.first_response_at);
      if (value) out.set(String(row.call_id), value);
    }
    return out;
  }

  function payloadSummaries(db: any, callIds: string[]): Map<string, { count: number; providerRequests: number }> {    const out = new Map<string, { count: number; providerRequests: number }>();
    if (callIds.length === 0) return out;
    const rows = db.prepare(
      `SELECT call_id, COUNT(*) AS n,
              SUM(CASE WHEN kind = 'provider_request' THEN 1 ELSE 0 END) AS provider_requests
       FROM payload_records WHERE call_id IN (${callIds.map(() => "?").join(",")}) GROUP BY call_id`,
    ).all(...callIds);
    for (const row of rows) {
      out.set(row.call_id, { count: Number(row.n ?? 0), providerRequests: Number(row.provider_requests ?? 0) });
    }
    return out;
  }

  function usageRows(db: any, callIds: string[]): Map<string, Record<string, unknown>> {
    const out = new Map<string, Record<string, unknown>>();
    if (callIds.length === 0) return out;
    const rows = db.prepare(
      `SELECT * FROM model_call_usage WHERE model_call_id IN (${callIds.map(() => "?").join(",")})`,
    ).all(...callIds);
    for (const row of rows) out.set(String(row.model_call_id), row);
    return out;
  }

  const USAGE_INTEGER_FIELDS = [
    "duration_ms",
    "input_total_tokens",
    "input_uncached_tokens",
    "output_total_tokens",
    "reasoning_tokens",
    "cache_read_tokens",
    "cache_write_tokens",
    "cache_miss_tokens",
    "total_tokens",
  ] as const;

  const USAGE_BOOLEAN_FIELDS = ["cache_hit", "cache_created"] as const;

  function isUsageRowCorrupt(usage: Record<string, unknown>): boolean {
    for (const field of USAGE_INTEGER_FIELDS) {
      const value = usage[field];
      if (value !== null && value !== undefined
        && (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0)) {
        return true;
      }
    }
    const cost = usage.cost_total;
    if (cost !== null && cost !== undefined
      && (typeof cost !== "number" || !Number.isFinite(cost) || cost < 0)) {
      return true;
    }
    const ratio = usage.cache_hit_ratio;
    if (ratio !== null && ratio !== undefined
      && (typeof ratio !== "number" || !Number.isFinite(ratio) || ratio < 0 || ratio > 1)) {
      return true;
    }
    return USAGE_BOOLEAN_FIELDS.some((field) => {
      const value = usage[field];
      return value !== null && value !== undefined && value !== 0 && value !== 1;
    });
  }

  function usageOf(
    reader: CachedReader,
    usage: Record<string, unknown> | undefined,
    correlationState: unknown,
  ): ModelObservabilityCallListItem["usage"] {
    if (!reader.hasAccounting) {
      return { availability: "projection_unavailable", status: null, summary: null };
    }
    if (!usage) {
      return {
        availability: correlationState === "not_correlated" ? "not_correlated" : "unknown",
        status: null,
        summary: null,
      };
    }
    if (isUsageRowCorrupt(usage)) {
      return {
        availability: "corrupt",
        status: textOrNull(usage.usage_status),
        summary: null,
      };
    }
    return {
      availability: "present",
      status: textOrNull(usage.usage_status),
      summary: {
        inputTokens: finiteIntegerOrNull(usage.input_total_tokens),
        inputUncachedTokens: finiteIntegerOrNull(usage.input_uncached_tokens),
        outputTokens: finiteIntegerOrNull(usage.output_total_tokens),
        reasoningTokens: finiteIntegerOrNull(usage.reasoning_tokens),
        cacheReadTokens: finiteIntegerOrNull(usage.cache_read_tokens),
        cacheWriteTokens: finiteIntegerOrNull(usage.cache_write_tokens),
        totalTokens: finiteIntegerOrNull(usage.total_tokens),
        costTotal: finiteNumberOrNull(usage.cost_total),
      },
    };
  }

  function callItemFromRow(
    reader: CachedReader,
    row: Record<string, unknown>,
    extras: {
      payloadRecordCount: number;
      attemptCount: number;
      providerRequestCount: number;
      usageRow: Record<string, unknown> | undefined;
      firstResponseAt: string | null;
    },
  ): ModelObservabilityCallListItem {
    const startedAt = textOrNull(row.started_at);
    const endedAt = textOrNull(row.ended_at);
    const categories = parseCategories(row.provenance_categories_json);
    return {
      callId: String(row.call_id ?? ""),
      traceId: textOrNull(row.trace_id),
      parentCallId: textOrNull(row.parent_call_id),
      startedAt,
      endedAt,
      durationMs: durationMs(startedAt, endedAt),
      terminalStatus: textOrNull(row.terminal_status),
      persistenceCompleteness: textOrNull(row.persistence_completeness) ?? "partial",
      interruptedByRestart: boolFlag(row.interrupted_by_restart),
      model: {
        provider: textOrNull(row.provider),
        modelId: textOrNull(row.model_id),
        api: textOrNull(row.api),
      },
      sourceIdentity: resolveModelObservabilitySourceIdentity(reader.db, row),
      source: {
        subsystem: textOrNull(row.subsystem),
        operation: textOrNull(row.operation),
        surface: textOrNull(row.surface),
        trigger: textOrNull(row.trigger),
      },
      attribution: {
        kind: textOrNull(row.attribution_kind),
        sessionId: textOrNull(row.session_id),
        sessionPath: textOrNull(row.session_path),
        conversationId: textOrNull(row.conversation_id),
        conversationType: textOrNull(row.conversation_type),
        agentId: textOrNull(row.agent_id),
        childAgentId: textOrNull(row.child_agent_id),
        childSessionId: textOrNull(row.child_session_id),
        taskId: textOrNull(row.task_id),
      },
      callPurpose: textOrNull(row.call_purpose),
      inputShape: textOrNull(row.input_shape),
      firstResponseAt: extras.firstResponseAt,
      provenancePrecision: textOrNull(row.provenance_precision),
      provenance: {
        sectionCount: finiteIntegerOrNull(row.provenance_section_count),
        opaqueCount: finiteIntegerOrNull(row.provenance_opaque_count),
        categories: categories.values,
        categoriesState: categories.state,
      },
      payloadAvailability: payloadAvailabilityOf(row.payload_availability, extras.payloadRecordCount),
      payloadRecordCount: extras.payloadRecordCount,
      usage: usageOf(reader, extras.usageRow, row.usage_correlation_state),
      attemptCount: extras.attemptCount,
      providerRequestCount: extras.providerRequestCount,
    };
  }

  /* ── Call list（§二十二/二十五/二十六/二十七）───────────────────────── */

  function queryCalls(query: NormalizedModelObservabilityQuery): ModelObservabilityQueryResult<ModelObservabilityCallPage> {
    return runQuery((reader) => {
      const filterSql = buildCallFilterSql(query.filter, "mc");
      let keysetSql: SqlWithParams | null = null;
      if (query.cursor) {
        const decoded = decodeModelObservabilityCallCursor(query.cursor, query.filter, query.sort);
        if (decoded.ok === false) throw new CursorError(decoded.error.message);
        keysetSql = buildCallKeysetSql(decoded.value, "mc");
      }
      const whereSql = keysetSql ? `${filterSql.sql} AND ${keysetSql.sql}` : filterSql.sql;
      const params = [...filterSql.params, ...(keysetSql ? keysetSql.params : [])];
      const rows: Array<Record<string, unknown>> = reader.db.prepare(
        `SELECT mc.* FROM model_calls mc WHERE ${whereSql} ORDER BY ${CALL_ORDER_SQL} LIMIT ?`,
      ).all(...params, query.limit + 1);

      const hasMore = rows.length > query.limit;
      const page = hasMore ? rows.slice(0, query.limit) : rows;
      const callIds = page.map((row) => String(row.call_id));
      const attempts = attemptCounts(reader.db, callIds);
      const firstResponses = attemptFirstResponses(reader.db, callIds);
      const payloads = payloadSummaries(reader.db, callIds);
      const usage = reader.hasAccounting ? usageRows(reader.db, callIds) : new Map<string, Record<string, unknown>>();

      const calls = page.map((row) => {
        const callId = String(row.call_id);
        const payload = payloads.get(callId) ?? { count: 0, providerRequests: 0 };
        return callItemFromRow(reader, row, {
          payloadRecordCount: payload.count,
          attemptCount: attempts.get(callId) ?? 0,
          providerRequestCount: payload.providerRequests,
          usageRow: usage.get(callId),
          firstResponseAt: firstResponses.get(callId) ?? null,
        });
      });

      let nextCursor: string | null = null;
      if (hasMore && page.length > 0) {
        const last = page[page.length - 1];
        nextCursor = encodeModelObservabilityCallCursor(
          { lastStartedAt: textOrNull(last.started_at), lastCallId: String(last.call_id) },
          query.filter,
          query.sort,
        );
      }
      return { calls, nextCursor, dataCompleteness: readDataCompleteness(reader.db) };
    });
  }

  /* ── Trace list（§二十八/二十九：filter 语义 = trace 内至少一条 call 命中）── */

  function queryTraces(query: NormalizedModelObservabilityTraceQuery): ModelObservabilityQueryResult<ModelObservabilityTracePage> {
    return runQuery((reader) => {
      const filterSql = buildCallFilterSql(query.filter, "candidate");
      const clauses = [
        `EXISTS (
          SELECT 1 FROM model_calls candidate
          WHERE candidate.trace_id = t.trace_id AND ${filterSql.sql}
        )`,
      ];
      const params = [...filterSql.params];
      // 产品口径（2026-09-05）：调用轨迹列表面向对话/任务轨迹；origin 为空的
      // singleton 辅助调用（技能名翻译/知识滚动等）默认不进列表（调用台账仍可见）。
      if (!query.includeSingleton) {
        clauses.push("t.origin IS NOT NULL");
      }
      if (query.origin) {
        clauses.push("t.origin = ?");
        params.push(query.origin);
      }
      if (query.cursor) {
        const decoded = decodeModelObservabilityTraceCursor(query.cursor, query.filter, query.origin, query.minCallCount, query.includeSingleton);
        if (decoded.ok === false) throw new CursorError(decoded.error.message);
        const { lastSeenAt, lastTraceId } = decoded.value;
        if (lastSeenAt === null) {
          clauses.push("(t.last_seen_at IS NULL AND t.trace_id < ?)");
          params.push(lastTraceId);
        } else {
          clauses.push(
            "(t.last_seen_at IS NULL OR (t.last_seen_at IS NOT NULL AND (t.last_seen_at < ? OR (t.last_seen_at = ? AND t.trace_id < ?))))",
          );
          params.push(lastSeenAt, lastSeenAt, lastTraceId);
        }
      }
      const rows: Array<Record<string, unknown>> = reader.db.prepare(
        `SELECT t.trace_id AS trace_id, t.origin AS origin, t.first_seen_at AS first_seen_at,
                t.last_seen_at AS last_seen_at,
                COUNT(c.call_id) AS call_count,
                SUM(CASE WHEN c.terminal_status = 'ok' THEN 1 ELSE 0 END) AS ok_count,
                SUM(CASE WHEN c.terminal_status = 'error' THEN 1 ELSE 0 END) AS error_count,
                SUM(CASE WHEN c.terminal_status = 'aborted' THEN 1 ELSE 0 END) AS aborted_count,
                SUM(CASE WHEN c.terminal_status IS NULL OR c.terminal_status = '' THEN 1 ELSE 0 END) AS incomplete_count
         FROM traces t
         JOIN model_calls c ON c.trace_id = t.trace_id
         WHERE ${clauses.join(" AND ")}
         GROUP BY t.trace_id, t.origin, t.first_seen_at, t.last_seen_at
         ${query.minCallCount !== null ? "HAVING COUNT(c.call_id) >= ?" : ""}
         ORDER BY (t.last_seen_at IS NULL) ASC, t.last_seen_at DESC, t.trace_id DESC
         LIMIT ?`,
      ).all(...params, ...(query.minCallCount !== null ? [query.minCallCount] : []), query.limit + 1);

      const hasMore = rows.length > query.limit;
      const page = hasMore ? rows.slice(0, query.limit) : rows;
      const traceIds = page.map((row) => String(row.trace_id));
      const rootByTrace = new Map<string, Record<string, unknown>>();
      if (traceIds.length > 0) {
        const rootRows: Array<Record<string, unknown>> = reader.db.prepare(
          `SELECT * FROM model_calls
           WHERE trace_id IN (${traceIds.map(() => "?").join(",")})
           ORDER BY trace_id,
             CASE WHEN parent_call_id IS NULL OR parent_call_id = '' THEN 0 ELSE 1 END,
             CASE WHEN started_at IS NULL OR started_at = '' THEN 1 ELSE 0 END,
             started_at,
             call_id`,
        ).all(...traceIds);
        for (const rootRow of rootRows) {
          const traceId = String(rootRow.trace_id ?? "");
          if (!rootByTrace.has(traceId)) rootByTrace.set(traceId, rootRow);
        }
      }
      const traces: ModelObservabilityTraceListItem[] = page.map((row) => ({
        traceId: String(row.trace_id),
        origin: textOrNull(row.origin),
        sourceIdentity: resolveModelObservabilitySourceIdentity(
          reader.db,
          rootByTrace.get(String(row.trace_id)) ?? row,
        ),
        firstSeenAt: String(row.first_seen_at ?? ""),
        lastSeenAt: String(row.last_seen_at ?? ""),
        callCount: Number(row.call_count ?? 0),
        terminalOk: Number(row.ok_count ?? 0),
        terminalError: Number(row.error_count ?? 0),
        terminalAborted: Number(row.aborted_count ?? 0),
        incomplete: Number(row.incomplete_count ?? 0),
      }));
      let nextCursor: string | null = null;
      if (hasMore && page.length > 0) {
        const last = page[page.length - 1];
        nextCursor = encodeModelObservabilityTraceCursor(
          { lastSeenAt: textOrNull(last.lastSeenAt), lastTraceId: String(last.trace_id) },
          query.filter,
          query.origin,
          query.minCallCount,
          query.includeSingleton,
        );
      }
      return { traces, nextCursor };
    });
  }

  /* ── Aggregate Group By（§三十九～四十五：SQLite 内完成，不整表进内存）── */

  /* Phase 10 DST 专项（§八十一）：date bucket 支持 IANA timeZone。固定
   * utcOffsetMinutes 对历史跨 DST 窗口分错日期；这里把过滤集的时间范围
   * 按 DST 段展开成有界 CASE（retention 有界 → 段数 ≤ 上限，超限诚实报错）。
   */
  const DATE_BUCKET_MAX_SEGMENTS = 16;
  const DATE_BUCKET_TRANSITION_SCAN_MS = 6 * 60 * 60 * 1000;
  const timeZoneFormatters = new Map<string, Intl.DateTimeFormat>();

  function timeZoneOffsetMinutesAt(timeZone: string, epochMs: number): number {
    let formatter = timeZoneFormatters.get(timeZone);
    if (!formatter) {
      formatter = new Intl.DateTimeFormat("en-US", { timeZone, timeZoneName: "longOffset" });
      timeZoneFormatters.set(timeZone, formatter);
    }
    const formatted = formatter
      .formatToParts(new Date(epochMs))
      .find((part) => part.type === "timeZoneName")?.value ?? "GMT+00:00";
    const match = /GMT([+-])(\d{2}):(\d{2})/.exec(formatted);
    if (!match) return 0; // "GMT"（UTC）或不可解析 → 0
    const sign = match[1] === "-" ? -1 : 1;
    return sign * (Number(match[2]) * 60 + Number(match[3]));
  }

  function nextTimeZoneTransition(timeZone: string, fromMs: number, untilMs: number): number | null {
    let scanStart = fromMs;
    let scanOffset = timeZoneOffsetMinutesAt(timeZone, scanStart);
    while (scanStart < untilMs) {
      const scanEnd = Math.min(untilMs, scanStart + DATE_BUCKET_TRANSITION_SCAN_MS);
      const endOffset = timeZoneOffsetMinutesAt(timeZone, scanEnd);
      if (endOffset !== scanOffset) {
        let lo = scanStart;
        let hi = scanEnd;
        // scanStart 保持旧 offset，scanEnd 已是新 offset；找第一毫秒边界。
        while (hi - lo > 1) {
          const mid = Math.floor((lo + hi) / 2);
          if (timeZoneOffsetMinutesAt(timeZone, mid) === scanOffset) lo = mid;
          else hi = mid;
        }
        return hi;
      }
      scanStart = scanEnd;
      scanOffset = endOffset;
    }
    return null;
  }

  function dateBucketExpression(
    reader: { db: any; hasAccounting: boolean },
    query: NormalizedModelObservabilityAggregateQuery,
    filterSql: { sql: string; params: unknown[] },
  ): { sql: string; params: unknown[] } {
    const fixedOffset = query.dateBucket?.utcOffsetMinutes;
    if (typeof fixedOffset === "number") {
      return {
        sql: `strftime('%Y-%m-%d', mc.started_at, printf('%+d minutes', ?))`,
        params: [fixedOffset],
      };
    }
    const timeZone = query.dateBucket?.timeZone;
    if (!timeZone) {
      return {
        sql: `strftime('%Y-%m-%d', mc.started_at, printf('%+d minutes', ?))`,
        params: [0],
      };
    }
    // 过滤集的 [min,max] started_at（同一 WHERE；一次轻量 MIN/MAX）。
    const rangeRow = reader.db.prepare(
      `SELECT MIN(mc.started_at) AS minAt, MAX(mc.started_at) AS maxAt FROM model_calls mc WHERE ${filterSql.sql}`,
    ).get(...filterSql.params) as { minAt: string | null; maxAt: string | null };
    if (!rangeRow?.minAt || !rangeRow?.maxAt) {
      const offsetNow = timeZoneOffsetMinutesAt(timeZone, Date.now());
      return {
        sql: `strftime('%Y-%m-%d', mc.started_at, printf('%+d minutes', ?))`,
        params: [offsetNow],
      };
    }
    const startMs = new Date(rangeRow.minAt).getTime();
    const endMs = new Date(rangeRow.maxAt).getTime();
    const segments: Array<{ boundaryIso: string | null; offset: number }> = [];
    let cursor = startMs;
    segments.push({ boundaryIso: null, offset: timeZoneOffsetMinutesAt(timeZone, cursor) });
    while (cursor < endMs) {
      const transition = nextTimeZoneTransition(timeZone, cursor, endMs);
      if (transition === null) break;
      if (segments.length >= DATE_BUCKET_MAX_SEGMENTS) {
        throw new DateBucketTooComplexError("date bucket range exceeds the supported transition limit");
      }
      segments.push({
        boundaryIso: new Date(transition).toISOString(),
        offset: timeZoneOffsetMinutesAt(timeZone, transition),
      });
      cursor = transition;
    }
    // 展开为 CASE：started_at（ISO 文本字典序）与 boundary 同构可比较。
    // 段 i 的区间 = [b_i, b_{i+1})（b_0 = 负无穷）；CASE 分支
    // `WHEN started_at < b_{i+1} THEN offset_i`，ELSE = 最后一段 offset。
    const branches = segments.slice(0, segments.length - 1).map((segment, index) => ({
      boundary: segments[index + 1].boundaryIso as string,
      whenSql: `WHEN mc.started_at < ? THEN strftime('%Y-%m-%d', mc.started_at, printf('%+d minutes', ?))`,
      params: [segments[index + 1].boundaryIso as string, segment.offset],
    }));
    const lastOffset = segments[segments.length - 1].offset;
    const baseSql = `strftime('%Y-%m-%d', mc.started_at, printf('%+d minutes', ?))`;
    if (branches.length === 0) {
      return { sql: baseSql, params: [lastOffset] };
    }
    const sql = `CASE ${branches.map((branch) => branch.whenSql).join(" ")} ELSE ${baseSql} END`;
    const params = branches.flatMap((branch) => branch.params).concat([lastOffset]);
    return { sql, params };
  }

  const GROUP_DIMENSION_SQL: Record<ModelObservabilityGroupByDimension, string[]> = {
    date: [], // 动态构造（strftime + offset 绑定参数）。
    provider: ["provider"],
    model: ["provider", "model_id"],
    category: ["subsystem"],
    operation: ["operation"],
    callPurpose: ["call_purpose"],
    status: ["terminal_status"],
    attributionKind: ["attribution_kind"],
    session: ["session_id"],
    conversation: ["conversation_id"],
    agent: ["agent_id"],
    task: ["task_id"],
    inputShape: ["input_shape"],
    provenancePrecision: ["provenance_precision"],
  };

  const METRIC_SQL = `
    COUNT(*) AS call_count,
    COUNT(DISTINCT mc.trace_id) AS trace_count,
    SUM(CASE WHEN mc.terminal_status = 'ok' THEN 1 ELSE 0 END) AS ok_count,
    SUM(CASE WHEN mc.terminal_status = 'error' THEN 1 ELSE 0 END) AS error_count,
    SUM(CASE WHEN mc.terminal_status = 'aborted' THEN 1 ELSE 0 END) AS aborted_count,
    SUM(CASE WHEN mc.terminal_status IS NULL OR mc.terminal_status = '' THEN 1 ELSE 0 END) AS incomplete_count,
    SUM((SELECT COUNT(*) FROM model_attempts ma WHERE ma.call_id = mc.call_id)) AS attempt_count,
    SUM(CASE WHEN mc.started_at IS NOT NULL AND mc.ended_at IS NOT NULL
        THEN CAST((julianday(mc.ended_at) - julianday(mc.started_at)) * 86400000.0 AS INTEGER) ELSE 0 END) AS duration_total_ms,
    SUM(CASE WHEN mc.started_at IS NOT NULL AND mc.ended_at IS NOT NULL THEN 1 ELSE 0 END) AS duration_observed_count`;

  function usageMetricSql(reader: CachedReader): string {
    if (!reader.hasAccounting) {
      return `
        0 AS usage_covered, 0 AS usage_corrupt, 0 AS usage_not_correlated, 0 AS usage_missing,
        0 AS input_tokens_observed, NULL AS input_tokens,
        0 AS output_tokens_observed, NULL AS output_tokens,
        0 AS reasoning_tokens_observed, NULL AS reasoning_tokens,
        0 AS cache_read_tokens_observed, NULL AS cache_read_tokens,
        0 AS cache_write_tokens_observed, NULL AS cache_write_tokens,
        0 AS total_tokens_observed, NULL AS total_tokens,
        0 AS cost_total_observed, NULL AS cost_total,
        NULL AS cache_hit_count, 0 AS cache_observed_count`;
    }
    const validInteger = (column: string) =>
      `(typeof(u.${column}) = 'integer' AND u.${column} >= 0)`;
    const validNumber = (column: string) =>
      `(typeof(u.${column}) IN ('integer','real') AND u.${column} >= 0 AND abs(u.${column}) <= 1.7976931348623157e308)`;
    const corruptPredicate = [
      ...USAGE_INTEGER_FIELDS.map((column) =>
        `(u.${column} IS NOT NULL AND NOT ${validInteger(column)})`),
      `(u.cost_total IS NOT NULL AND NOT ${validNumber("cost_total")})`,
      `(u.cache_hit_ratio IS NOT NULL AND NOT (${validNumber("cache_hit_ratio")} AND u.cache_hit_ratio <= 1))`,
      ...USAGE_BOOLEAN_FIELDS.map((column) =>
        `(u.${column} IS NOT NULL AND NOT (typeof(u.${column}) = 'integer' AND u.${column} IN (0,1)))`),
    ].join(" OR ");
    const notCorrelated = reader.schemaVersion >= 3
      ? "SUM(CASE WHEN u.model_call_id IS NULL AND mc.usage_correlation_state = 'not_correlated' THEN 1 ELSE 0 END)"
      : "0";
    return `
      SUM(CASE WHEN u.model_call_id IS NOT NULL AND NOT (${corruptPredicate}) THEN 1 ELSE 0 END) AS usage_covered,
      SUM(CASE WHEN u.model_call_id IS NOT NULL AND (${corruptPredicate}) THEN 1 ELSE 0 END) AS usage_corrupt,
      ${notCorrelated} AS usage_not_correlated,
      SUM(CASE WHEN u.model_call_id IS NOT NULL AND u.usage_status = 'usage_missing' THEN 1 ELSE 0 END) AS usage_missing,
      SUM(CASE WHEN ${validInteger("input_total_tokens")} THEN 1 ELSE 0 END) AS input_tokens_observed,
      SUM(CASE WHEN ${validInteger("input_total_tokens")} THEN u.input_total_tokens ELSE NULL END) AS input_tokens,
      SUM(CASE WHEN ${validInteger("output_total_tokens")} THEN 1 ELSE 0 END) AS output_tokens_observed,
      SUM(CASE WHEN ${validInteger("output_total_tokens")} THEN u.output_total_tokens ELSE NULL END) AS output_tokens,
      SUM(CASE WHEN ${validInteger("reasoning_tokens")} THEN 1 ELSE 0 END) AS reasoning_tokens_observed,
      SUM(CASE WHEN ${validInteger("reasoning_tokens")} THEN u.reasoning_tokens ELSE NULL END) AS reasoning_tokens,
      SUM(CASE WHEN ${validInteger("cache_read_tokens")} THEN 1 ELSE 0 END) AS cache_read_tokens_observed,
      SUM(CASE WHEN ${validInteger("cache_read_tokens")} THEN u.cache_read_tokens ELSE NULL END) AS cache_read_tokens,
      SUM(CASE WHEN ${validInteger("cache_write_tokens")} THEN 1 ELSE 0 END) AS cache_write_tokens_observed,
      SUM(CASE WHEN ${validInteger("cache_write_tokens")} THEN u.cache_write_tokens ELSE NULL END) AS cache_write_tokens,
      SUM(CASE WHEN ${validInteger("total_tokens")} THEN 1 ELSE 0 END) AS total_tokens_observed,
      SUM(CASE WHEN ${validInteger("total_tokens")} THEN u.total_tokens ELSE NULL END) AS total_tokens,
      SUM(CASE WHEN ${validNumber("cost_total")} THEN 1 ELSE 0 END) AS cost_total_observed,
      SUM(CASE WHEN ${validNumber("cost_total")} THEN u.cost_total ELSE NULL END) AS cost_total,
      SUM(CASE WHEN u.cache_hit = 1 THEN 1 ELSE 0 END) AS cache_hit_count,
      SUM(CASE WHEN typeof(u.cache_hit) = 'integer' AND u.cache_hit IN (0,1) THEN 1 ELSE 0 END) AS cache_observed_count`;
  }

  function metricsFromRow(
    row: Record<string, unknown>,
    { hasAccounting }: { hasAccounting: boolean },
  ): ModelObservabilityGroupMetrics {
    const observed = Number(row.duration_observed_count ?? 0);
    const totalMs = Number(row.duration_total_ms ?? 0);
    const callCount = Number(row.call_count ?? 0);
    const usageCoveredCalls = Number(row.usage_covered ?? 0);
    const usageCorruptCalls = Number(row.usage_corrupt ?? 0);
    const usageNotCorrelatedCalls = Number(row.usage_not_correlated ?? 0);
    const usageUnknownCalls = Math.max(
      0,
      callCount - usageCoveredCalls - usageCorruptCalls - usageNotCorrelatedCalls,
    );
    const usageAggregateAvailability = !hasAccounting
      ? "projection_unavailable" as const
      : usageCorruptCalls > 0
        ? "corrupt" as const
      : usageUnknownCalls === 0
        ? "complete" as const
        : usageCoveredCalls + usageNotCorrelatedCalls > 0
          ? "partial" as const
          : "unknown" as const;
    const aggregateNumber = (value: unknown, observedKey: string): number | null => {
      if (callCount === 0) return 0;
      if (Number(row[observedKey] ?? 0) <= 0) return null;
      return finiteNumberOrNull(value);
    };
    const aggregateInteger = (value: unknown, observedKey: string): number | null => {
      const result = aggregateNumber(value, observedKey);
      return result === null ? null : Math.trunc(result);
    };
    const cacheObservedCount = Number(row.cache_observed_count ?? 0);
    return {
      callCount,
      traceCount: Number(row.trace_count ?? 0),
      okCount: Number(row.ok_count ?? 0),
      errorCount: Number(row.error_count ?? 0),
      abortedCount: Number(row.aborted_count ?? 0),
      incompleteCount: Number(row.incomplete_count ?? 0),
      attemptCount: Number(row.attempt_count ?? 0),
      durationObservedCount: observed,
      durationTotalMs: totalMs,
      durationAverageMs: observed > 0 ? Math.round(totalMs / observed) : null,
      usageAggregateAvailability,
      usageCoveredCalls,
      usageCorruptCalls,
      usageNotCorrelatedCalls,
      usageUnknownCalls,
      usageMissingCalls: Number(row.usage_missing ?? 0),
      inputTokens: aggregateInteger(row.input_tokens, "input_tokens_observed"),
      outputTokens: aggregateInteger(row.output_tokens, "output_tokens_observed"),
      reasoningTokens: aggregateInteger(row.reasoning_tokens, "reasoning_tokens_observed"),
      cacheReadTokens: aggregateInteger(row.cache_read_tokens, "cache_read_tokens_observed"),
      cacheWriteTokens: aggregateInteger(row.cache_write_tokens, "cache_write_tokens_observed"),
      totalTokens: aggregateInteger(row.total_tokens, "total_tokens_observed"),
      costTotal: aggregateNumber(row.cost_total, "cost_total_observed"),
      cacheHitCount: callCount === 0 ? 0 : cacheObservedCount > 0 ? Number(row.cache_hit_count ?? 0) : null,
      cacheObservedCount,
    };
  }

  function queryAggregate(query: NormalizedModelObservabilityAggregateQuery): ModelObservabilityQueryResult<ModelObservabilityAggregateResult> {
    return runQuery((reader) => {
      const filterSql = buildCallFilterSql(query.filter, "mc");
      const fromSql = reader.hasAccounting
        ? "FROM model_calls mc LEFT JOIN model_call_usage u ON u.model_call_id = mc.call_id"
        : "FROM model_calls mc";
      const usageMetrics = usageMetricSql(reader);

      // 维度表达式 → g0..gN 别名（闭集映射；date 绑定 offset 参数，§四十三）。
      const groupExpressions: Array<{ dimension: ModelObservabilityGroupByDimension; column: string | null; expr: string; params: unknown[] }> = [];
      for (const dimension of query.groupBy) {
        if (dimension === "date") {
          const dateExpr = dateBucketExpression(reader, query, filterSql);
          groupExpressions.push({ dimension, column: null, expr: dateExpr.sql, params: dateExpr.params });
          continue;
        }
        for (const column of GROUP_DIMENSION_SQL[dimension]) {
          groupExpressions.push({ dimension, column, expr: `mc.${column}`, params: [] });
        }
      }

      const groupParamList = groupExpressions.flatMap((g) => g.params);
      const overallRow = reader.db.prepare(
        `SELECT ${METRIC_SQL}, ${usageMetrics} ${fromSql} WHERE ${filterSql.sql}`,
      ).get(...filterSql.params);

      if (query.groupBy.length === 0) {
        return { groups: [], overall: metricsFromRow(overallRow, reader) };
      }

      const selectDims = groupExpressions.map((g, i) => `${g.expr} AS g${i}`).join(", ");
      const groupBySql = groupExpressions.map((_, i) => `g${i}`).join(", ");
      // 参数顺序：SELECT 维度表达式参数在前，WHERE filter 参数在后。
      const rows: Array<Record<string, unknown>> = reader.db.prepare(
        `SELECT ${selectDims}, ${METRIC_SQL}, ${usageMetrics}
         ${fromSql}
         WHERE ${filterSql.sql}
         GROUP BY ${groupBySql}
         ORDER BY ${groupBySql} ASC`,
      ).all(...groupParamList, ...filterSql.params);

      const groups: ModelObservabilityGroupBucket[] = rows.map((row) => {
        const values: ModelObservabilityGroupBucket["values"] = {};
        const keyParts: string[] = [];
        groupExpressions.forEach((g, i) => {
          const value = textOrNull(row[`g${i}`]);
          if (g.dimension === "model") {
            if (g.column === "provider") values.provider = value;
            else values.modelId = value;
          } else if (g.dimension === "date") {
            values.date = value;
          } else {
            values[g.dimension] = value;
          }
          keyParts.push(value ?? "∅");
        });
        return { key: keyParts.join("::"), values, metrics: metricsFromRow(row, reader) };
      });
      return { groups, overall: metricsFromRow(overallRow, reader) };
    });
  }

  /* ── Call detail（§三十二/三十三/三十四）───────────────────────────── */

  function payloadMetadataRows(db: any, callId: string): ModelObservabilityPayloadRecordMetadata[] {
    const rows: Array<Record<string, unknown>> = db.prepare(
      `SELECT * FROM payload_records WHERE call_id = ? ORDER BY id`,
    ).all(callId);
    const out: ModelObservabilityPayloadRecordMetadata[] = [];
    for (const row of rows) {
      const blobIds: string[] = db.prepare(
        `SELECT blob_id FROM payload_blob_refs WHERE payload_record_id = ? ORDER BY blob_id`,
      ).all(row.id).map((r: Record<string, unknown>) => String(r.blob_id));
      out.push({
        id: Number(row.id),
        callId: String(row.call_id),
        kind: String(row.kind),
        attemptId: textOrNull(row.attempt_id),
        providerRequestOrdinal: finiteIntegerOrNull(row.provider_request_ordinal),
        capturedAt: String(row.captured_at ?? ""),
        visibility: String(row.visibility ?? ""),
        fidelity: String(row.fidelity ?? ""),
        sanitizationStatus: String(row.sanitization_status ?? ""),
        redacted: boolFlag(row.redacted),
        truncated: boolFlag(row.truncated),
        degraded: boolFlag(row.degraded),
        recordCharCount: finiteIntegerOrNull(row.record_char_count),
        hasBody: typeof row.payload_json === "string" && row.payload_json.length > 0,
        hasSemanticProvenance: typeof row.semantic_input_provenance_json === "string"
          && (row.semantic_input_provenance_json as string).length > 0,
        hasProviderProvenance: typeof row.provider_request_provenance_json === "string"
          && (row.provider_request_provenance_json as string).length > 0,
        blobIds,
      });
    }
    return out;
  }

  function callRefFromRow(row: Record<string, unknown> | undefined): ModelObservabilityCallRef | null {
    if (!row) return null;
    return {
      callId: String(row.call_id),
      startedAt: textOrNull(row.started_at),
      terminalStatus: textOrNull(row.terminal_status),
      modelId: textOrNull(row.model_id),
    };
  }

  function queryCallDetail(callId: string): ModelObservabilityQueryResult<ModelObservabilityCallDetail> {
    return runQuery((reader) => {
      const row = reader.db.prepare(`SELECT * FROM model_calls WHERE call_id = ?`).get(callId);
      if (!row) throw new NotFoundError(`call ${callId} not found`, "not_found");
      const payloadMeta = payloadMetadataRows(reader.db, callId);
      const attempts = attemptCounts(reader.db, [callId]).get(callId) ?? 0;
      const usage = reader.hasAccounting ? usageRows(reader.db, [callId]).get(callId) : undefined;
      const call = callItemFromRow(reader, row, {
        payloadRecordCount: payloadMeta.length,
        attemptCount: attempts,
        providerRequestCount: payloadMeta.filter((p) => p.kind === "provider_request").length,
        usageRow: usage,
        firstResponseAt: attemptFirstResponses(reader.db, [callId]).get(callId) ?? null,
      });

      const traceRow = call.traceId
        ? reader.db.prepare(`SELECT * FROM traces WHERE trace_id = ?`).get(call.traceId)
        : null;
      const parentRow = call.parentCallId
        ? reader.db.prepare(
          `SELECT call_id, started_at, terminal_status, model_id FROM model_calls WHERE call_id = ?`,
        ).get(call.parentCallId)
        : undefined;
      const childRows: Array<Record<string, unknown>> = reader.db.prepare(
        `SELECT call_id, started_at, terminal_status, model_id FROM model_calls WHERE parent_call_id = ? ORDER BY started_at, call_id`,
      ).all(callId);
      const attemptRows: Array<Record<string, unknown>> = reader.db.prepare(
        `SELECT * FROM model_attempts WHERE call_id = ? ORDER BY started_at, rowid`,
      ).all(callId);

      return {
        call,
        trace: traceRow
          ? {
            traceId: String(traceRow.trace_id),
            origin: textOrNull(traceRow.origin),
            firstSeenAt: textOrNull(traceRow.first_seen_at),
            lastSeenAt: textOrNull(traceRow.last_seen_at),
          }
          : null,
        parentCall: callRefFromRow(parentRow),
        childCalls: childRows.map((child) => callRefFromRow(child)!).filter(Boolean),
        attempts: attemptRows.map((attempt) => ({
          attemptId: String(attempt.attempt_id),
          startedAt: textOrNull(attempt.started_at),
          requestPreparedAt: textOrNull(attempt.request_prepared_at),
          responseReceivedAt: textOrNull(attempt.response_received_at),
          errorAt: textOrNull(attempt.error_at),
          providerRequestId: textOrNull(attempt.provider_request_id),
          httpStatus: finiteIntegerOrNull(attempt.http_status),
          attemptVisibility: textOrNull(attempt.attempt_visibility),
          providerWireVisibility: textOrNull(attempt.provider_wire_visibility),
          errorName: textOrNull(attempt.error_name),
          errorCode: textOrNull(attempt.error_code),
          outputBudget: extractOutputBudgetFact(attempt.safe_details_json),
        })),
        payloadRecords: payloadMeta,
      };
    });
  }

  /* ── Trace detail（§三十/三十一：graph + cycle safe）───────────────── */

  function queryTraceDetail(traceId: string): ModelObservabilityQueryResult<ModelObservabilityTraceDetail> {
    return runQuery((reader) => {
      const traceRow = reader.db.prepare(`SELECT * FROM traces WHERE trace_id = ?`).get(traceId);
      const callRows: Array<Record<string, unknown>> = reader.db.prepare(
        `SELECT * FROM model_calls WHERE trace_id = ? ORDER BY started_at, call_id`,
      ).all(traceId);
      if (!traceRow && callRows.length === 0) {
        throw new NotFoundError(`trace ${traceId} not found`, "not_found");
      }
      const callIds = callRows.map((row) => String(row.call_id));
      const attempts = attemptCounts(reader.db, callIds);
      const firstResponses = attemptFirstResponses(reader.db, callIds);
      const payloads = payloadSummaries(reader.db, callIds);
      const usage = reader.hasAccounting ? usageRows(reader.db, callIds) : new Map<string, Record<string, unknown>>();
      const calls = callRows.map((row) => {
        const callId = String(row.call_id);
        const payload = payloads.get(callId) ?? { count: 0, providerRequests: 0 };
        return callItemFromRow(reader, row, {
          payloadRecordCount: payload.count,
          attemptCount: attempts.get(callId) ?? 0,
          providerRequestCount: payload.providerRequests,
          usageRow: usage.get(callId),
          firstResponseAt: firstResponses.get(callId) ?? null,
        });
      });

      // ── graph（roots / edges / orphanEdges / cycle-safe）──
      const known = new Set(callIds);
      const byParent = new Map<string, string[]>();
      const roots: Array<{ callId: string; orphanParent: boolean }> = [];
      const edges: Array<{ parentCallId: string; childCallId: string }> = [];
      const orphanEdges: Array<{ childCallId: string; missingParentCallId: string }> = [];
      for (const call of calls) {
        const parent = call.parentCallId;
        if (!parent) {
          roots.push({ callId: call.callId, orphanParent: false });
        } else if (known.has(parent)) {
          edges.push({ parentCallId: parent, childCallId: call.callId });
          const list = byParent.get(parent) ?? [];
          list.push(call.callId);
          byParent.set(parent, list);
        } else {
          // parent 指向 trace 外/不存在的 call：诚实标 orphan（§三十），不偷偷变 root。
          roots.push({ callId: call.callId, orphanParent: true });
          orphanEdges.push({ childCallId: call.callId, missingParentCallId: parent });
        }
      }
      // cycle 检测（§三十一）：parent 指针构成 functional graph（每节点 ≤1
      // parent）——沿 parent 链走，灰色节点重逢即环；iterative 三色标记，
      // O(n) 且不递归（损坏库也可能无 root 纯环，检测不依赖 roots 存在）。
      let graphIntegrity: "ok" | "degraded" = "ok";
      {
        const parentOf = new Map<string, string>();
        for (const edge of edges) parentOf.set(edge.childCallId, edge.parentCallId);
        const state = new Map<string, "visiting" | "done">();
        for (const callId of known) {
          if (state.has(callId)) continue;
          const path: string[] = [];
          const onPath = new Set<string>();
          let node: string | null = callId;
          while (node && !state.has(node)) {
            state.set(node, "visiting");
            path.push(node);
            onPath.add(node);
            node = parentOf.get(node) ?? null;
          }
          // while 提前退出且停在本 walk 的 visiting 节点上 → 父链绕回 = 环。
          if (node && state.get(node) === "visiting" && onPath.has(node)) {
            graphIntegrity = "degraded";
          }
          for (const visited of path) {
            state.set(visited, "done");
            onPath.delete(visited);
          }
        }
      }

      const counts = { ok: 0, error: 0, aborted: 0, incomplete: 0 };
      const payloadCompleteness = { present: 0, expired: 0, dropped: 0, notCaptured: 0, unknown: 0 };
      for (const call of calls) {
        if (call.terminalStatus === "ok") counts.ok += 1;
        else if (call.terminalStatus === "error") counts.error += 1;
        else if (call.terminalStatus === "aborted") counts.aborted += 1;
        else counts.incomplete += 1;
        if (call.payloadAvailability === "present") payloadCompleteness.present += 1;
        else if (call.payloadAvailability === "expired") payloadCompleteness.expired += 1;
        else if (call.payloadAvailability === "dropped") payloadCompleteness.dropped += 1;
        else if (call.payloadAvailability === "not_captured") payloadCompleteness.notCaptured += 1;
        else payloadCompleteness.unknown += 1;
      }

      let usageAggregate: ModelObservabilityTraceDetail["usageAggregate"];
      if (!reader.hasAccounting) {
        usageAggregate = {
          availability: "projection_unavailable",
          coveredCalls: 0,
          corruptCalls: 0,
          notCorrelatedCalls: 0,
          unknownCalls: calls.length,
          totalCalls: calls.length,
          summary: null,
        };
      } else {
        const coveredCalls = calls.filter((call) => call.usage.availability === "present").length;
        const corruptCalls = calls.filter((call) => call.usage.availability === "corrupt").length;
        const notCorrelatedCalls = calls.filter((call) => call.usage.availability === "not_correlated").length;
        const unknownCalls = Math.max(0, calls.length - coveredCalls - corruptCalls - notCorrelatedCalls);
        const summaries = calls
          .map((call) => call.usage.summary)
          .filter((summary): summary is NonNullable<typeof summary> => summary !== null);
        const sumKnown = (key: keyof (typeof summaries)[number]): number | null => {
          const values = summaries
            .map((summary) => summary[key])
            .filter((value): value is number => typeof value === "number" && Number.isFinite(value));
          return values.length > 0 ? values.reduce((total, value) => total + value, 0) : null;
        };
        usageAggregate = {
          availability: corruptCalls > 0
            ? "corrupt"
            : unknownCalls === 0
            ? "complete"
            : coveredCalls + notCorrelatedCalls > 0
              ? "partial"
              : "unknown",
          coveredCalls,
          corruptCalls,
          notCorrelatedCalls,
          unknownCalls,
          totalCalls: calls.length,
          summary: summaries.length > 0
            ? {
              inputTokens: sumKnown("inputTokens"),
              inputUncachedTokens: sumKnown("inputUncachedTokens"),
              outputTokens: sumKnown("outputTokens"),
              reasoningTokens: sumKnown("reasoningTokens"),
              cacheReadTokens: sumKnown("cacheReadTokens"),
              cacheWriteTokens: sumKnown("cacheWriteTokens"),
              totalTokens: sumKnown("totalTokens"),
              costTotal: sumKnown("costTotal"),
            }
            : null,
        };
      }

      return {
        trace: {
          traceId: String(traceRow?.trace_id ?? traceId),
          origin: textOrNull(traceRow?.origin),
          firstSeenAt: String(traceRow?.first_seen_at ?? calls[0]?.startedAt ?? ""),
          lastSeenAt: String(traceRow?.last_seen_at ?? ""),
          callCount: calls.length,
          terminalOk: counts.ok,
          terminalError: counts.error,
          terminalAborted: counts.aborted,
          incomplete: counts.incomplete,
          sourceIdentity: resolveModelObservabilitySourceIdentity(
            reader.db,
            callRows.find((row) => !textOrNull(row.parent_call_id)) ?? callRows[0] ?? traceRow ?? {},
          ),
        },
        calls,
        roots,
        edges,
        orphanEdges,
        graphIntegrity,
        usageAggregate,
        payloadCompleteness,
        dataCompleteness: readDataCompleteness(reader.db),
      };
    });
  }

  /* ── Exact payload retrieval（§三十五/三十六）──────────────────────── */

  function getPayloadRecord(payloadRecordId: number): ModelObservabilityQueryResult<ModelObservabilityPayloadRecordDetail> {
    return runQuery((reader) => {
      const row = reader.db.prepare(`SELECT * FROM payload_records WHERE id = ?`).get(payloadRecordId);
      if (!row) throw new NotFoundError(`payload record ${payloadRecordId} not found`, "not_found");
      const blobIds: string[] = reader.db.prepare(
        `SELECT blob_id FROM payload_blob_refs WHERE payload_record_id = ? ORDER BY blob_id`,
      ).all(row.id).map((r: Record<string, unknown>) => String(r.blob_id));

      const metadata: ModelObservabilityPayloadRecordMetadata = {
        id: Number(row.id),
        callId: String(row.call_id),
        kind: String(row.kind),
        attemptId: textOrNull(row.attempt_id),
        providerRequestOrdinal: finiteIntegerOrNull(row.provider_request_ordinal),
        capturedAt: String(row.captured_at ?? ""),
        visibility: String(row.visibility ?? ""),
        fidelity: String(row.fidelity ?? ""),
        sanitizationStatus: String(row.sanitization_status ?? ""),
        redacted: boolFlag(row.redacted),
        truncated: boolFlag(row.truncated),
        degraded: boolFlag(row.degraded),
        recordCharCount: finiteIntegerOrNull(row.record_char_count),
        hasBody: typeof row.payload_json === "string" && (row.payload_json as string).length > 0,
        hasSemanticProvenance: typeof row.semantic_input_provenance_json === "string"
          && (row.semantic_input_provenance_json as string).length > 0,
        hasProviderProvenance: typeof row.provider_request_provenance_json === "string"
          && (row.provider_request_provenance_json as string).length > 0,
        blobIds,
      };

      const visibility = metadata.visibility;
      let payloadJson = row.payload_json;
      let contentAvailable = false;
      let contentState: ModelObservabilityPayloadRecordDetail["contentState"] = "null_payload";
      let payload: unknown = null;
      if (visibility === "opaque" || visibility === "unavailable") {
        // §八十七：OPAQUE/UNAVAILABLE 不升级为空对象；payload_json 本应为 NULL，
        // 即使磁盘被手工塞了值也不返回。
        contentState = "opaque_or_unavailable";
        payloadJson = null;
      } else if (typeof payloadJson !== "string" || payloadJson.length === 0) {
        contentState = "null_payload";
      } else {
        try {
          payload = JSON.parse(payloadJson);
          contentAvailable = true;
          contentState = "present";
        } catch {
          // §三十六：损坏 → corrupt，绝不 500、绝不返回 raw malformed string。
          contentState = "corrupt";
        }
      }
      const parseProvenance = (raw: unknown): {
        state: "present" | "absent" | "corrupt";
        value: unknown;
      } => {
        if (typeof raw !== "string" || !raw) return { state: "absent", value: null };
        try {
          return { state: "present", value: JSON.parse(raw) };
        } catch {
          return { state: "corrupt", value: null };
        }
      };
      const semanticInputProvenance = parseProvenance(row.semantic_input_provenance_json);
      const providerRequestProvenance = parseProvenance(row.provider_request_provenance_json);
      return {
        ...metadata,
        contentAvailable,
        contentState,
        payload,
        semanticInputProvenanceState: semanticInputProvenance.state,
        semanticInputProvenance: semanticInputProvenance.value,
        providerRequestProvenanceState: providerRequestProvenance.state,
        providerRequestProvenance: providerRequestProvenance.value,
      };
    });
  }

  /* ── Query health（§四十九：不含任何正文）──────────────────────────── */

  function getHealth(): ModelObservabilityQueryResult<ModelObservabilityQueryHealth> {
    const opened = openCurrent();
    if (opened.ok === false) {
      return {
        ok: true,
        value: {
          queryStatus: opened.code === "absent" ? "absent" : "unavailable",
          queryStatusReason: opened.reasonCode ?? opened.code,
          schemaVersion: null,
          accountingProjectionAvailable: false,
          oldestCallAt: null,
          newestCallAt: null,
          callCount: 0,
          traceCount: 0,
          payloadRecordCount: 0,
          usageProjectionCount: 0,
          dataCompleteness: unknownDataCompleteness(),
        },
      };
    }
    return runQuery((reader) => {
      const counts = reader.db.prepare(
        `SELECT
           (SELECT COUNT(*) FROM model_calls) AS calls,
           (SELECT COUNT(*) FROM traces) AS traces,
           (SELECT COUNT(*) FROM payload_records) AS payloads,
           (SELECT MIN(started_at) FROM model_calls WHERE started_at IS NOT NULL) AS oldest,
           (SELECT MAX(started_at) FROM model_calls WHERE started_at IS NOT NULL) AS newest`,
      ).get();
      const usageCount = reader.hasAccounting
        ? Number(reader.db.prepare(`SELECT COUNT(*) AS n FROM model_call_usage`).get().n ?? 0)
        : 0;
      const dataCompleteness = readDataCompleteness(reader.db);
      return {
        queryStatus: dataCompleteness.status === "known" ? "ready" : "degraded",
        queryStatusReason: dataCompleteness.status === "known" ? null : "data_completeness_unknown",
        schemaVersion: reader.schemaVersion,
        accountingProjectionAvailable: reader.hasAccounting,
        oldestCallAt: textOrNull(counts.oldest),
        newestCallAt: textOrNull(counts.newest),
        callCount: Number(counts.calls ?? 0),
        traceCount: Number(counts.traces ?? 0),
        payloadRecordCount: Number(counts.payloads ?? 0),
        usageProjectionCount: usageCount,
        dataCompleteness,
      };
    });
  }

  /* ── Stored blob exact retrieval（Phase 9 §一百一十九～一百三十一）──────
   *
   * 唯一允许的 blob 读取面：exact blobId，无 list/search/path 参数。
   *   - blobId 格式闭集校验（mb_ + bounded token），非法一律显式 invalid_blob_id。
   *   - 路径由 blobId 重算（shard 规则与 blob-store relativePathFor 同源）——
   *     绝不信任 DB 的 relative_path（篡改后 traversal 防线）；resolve 后强制
   *     落在 blobs root 内。
   *   - 只读：不 mkdir、不建库、不做 missing 标记写回（readBlob 的
   *     UPDATE-on-missing 是写侧语义，readonly 连接不可用）。
   *   - 绝不访问外部引用（local_file_reference / signed URL / external URL）。
   */

  function getStoredBlob(blobId: string): ModelObservabilityQueryResult<ModelObservabilityStoredBlob> {
    if (typeof blobId !== "string" || !MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test(blobId)) {
      return fail("invalid_blob_id", "blob id has an invalid format", "invalid_blob_id");
    }
    return runQuery((reader) => {
      const row = reader.db.prepare(
        `SELECT blob_id, byte_length, media_type, state FROM blob_objects WHERE blob_id = ?`,
      ).get(blobId);
      if (!row) throw new NotFoundError("blob not found", "blob_not_found");
      if (row.state === "missing") throw new BlobMissingError("blob file is marked missing");
      const filePath = resolveExistingModelObservabilityBlobPath(lingxiHome, blobId);
      if (!filePath) throw new BlobMissingError("blob file is missing on disk");
      let byteLength: number;
      try {
        byteLength = fs.statSync(filePath).size;
      } catch {
        throw new BlobMissingError("blob file is missing on disk");
      }
      return {
        blobId,
        mediaType: typeof row.media_type === "string" && row.media_type ? row.media_type : null,
        byteLength,
        filePath,
      };
    });
  }

  return {
    queryCalls,
    queryTraces,
    queryAggregate,
    queryCallDetail,
    queryTraceDetail,
    getPayloadRecord,
    getStoredBlob,
    getHealth,
    invalidate,
    close,
  };
}

/** Stored blob 读取结果（server-only：本地路径不进入 browser wire contract）。 */
export type ModelObservabilityStoredBlob = {
  blobId: string;
  mediaType: string | null;
  byteLength: number;
  filePath: string;
};

export type ModelObservabilityQueryService = ReturnType<typeof createModelObservabilityQueryService>;
