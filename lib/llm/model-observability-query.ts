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
import path from "path";
import { modelObservabilityBlobsRoot, modelObservabilityDbPath } from "./model-observability-schema.ts";
import { openModelObservabilityReadDatabase } from "./model-observability-read-database.ts";
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
      const concrete = values.filter((v) => v !== "incomplete");
      if (concrete.length > 0) {
        clauses.push(`${col("terminal_status")} IN (${concrete.map(() => "?").join(",")})`);
        params.push(...concrete);
      }
      if (values.includes("incomplete")) {
        clauses.push(`(${col("terminal_status")} IS NULL OR ${col("terminal_status")} = '')`);
      }
      continue;
    }
    if (field === "payloadAvailability") {
      const columnStates = values.filter((v) => v !== "present" && v !== "unknown");
      if (columnStates.length > 0) {
        clauses.push(`${col("payload_availability")} IN (${columnStates.map(() => "?").join(",")})`);
        params.push(...columnStates);
      }
      if (values.includes("present")) {
        clauses.push(`EXISTS (SELECT 1 FROM payload_records pr WHERE pr.call_id = ${col("call_id")})`);
      }
      if (values.includes("unknown")) {
        clauses.push(
          `${col("payload_availability")} IS NULL AND NOT EXISTS (SELECT 1 FROM payload_records pr WHERE pr.call_id = ${col("call_id")})`,
        );
      }
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

function intOrNull(value: unknown): number | null {
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}

function boolFlag(value: unknown): boolean {
  return value === 1 || value === true;
}

function durationMs(startedAt: string | null, endedAt: string | null): number | null {
  if (!startedAt || !endedAt) return null;
  const start = new Date(startedAt).getTime();
  const end = new Date(endedAt).getTime();
  if (!Number.isFinite(start) || !Number.isFinite(end)) return null;
  return Math.max(0, end - start);
}

function parseCategories(raw: unknown): string[] {
  if (typeof raw !== "string" || !raw) return [];
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((c): c is string => typeof c === "string") : [];
  } catch {
    return [];
  }
}

/** payload_availability 真相枚举（§三十七：NULL 不折叠，无 payload row → unknown）。 */
function payloadAvailabilityOf(columnValue: unknown, recordCount: number): ModelObservabilityPayloadAvailability {
  if (recordCount > 0) return "present";
  const value = textOrNull(columnValue);
  if (value === "expired" || value === "dropped" || value === "not_captured") return value;
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
      // 连接可能已失效（writer close / 文件被替换）：失效缓存，下次查询重开。
      invalidate();
      if (process.env.LINGXI_OBS_QUERY_DEBUG) console.error("[obs-query]", error);
      return fail("query_failed", "observability query failed");
    }
  }

  /* ── 全局 drop counters（observability_meta 持久化 + DB 内事实）──────── */

  function readDataCompleteness(db: any): ModelObservabilityDataCompleteness {
    const out: ModelObservabilityDataCompleteness = {
      droppedTraceEvents: 0,
      droppedPayloadRecords: 0,
      droppedBlobs: 0,
      interruptedByRestartCalls: 0,
    };
    try {
      const readMeta = (key: string): number => {
        const row = db.prepare(`SELECT value_json FROM observability_meta WHERE key = ?`).get(key);
        if (!row) return 0;
        try {
          const value = JSON.parse(row.value_json);
          return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.floor(value) : 0;
        } catch {
          return 0;
        }
      };
      out.droppedTraceEvents = readMeta("droppedTraceEvents");
      out.droppedPayloadRecords = readMeta("droppedPayloadRecords");
      out.droppedBlobs = readMeta("droppedBlobs");
      out.interruptedByRestartCalls = Number(
        db.prepare(`SELECT COUNT(*) AS n FROM model_calls WHERE interrupted_by_restart = 1`).get().n ?? 0,
      );
    } catch {
      // meta 读取失败：诚实返回保守 0 计数。
    }
    return out;
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

  function payloadSummaries(db: any, callIds: string[]): Map<string, { count: number; providerRequests: number }> {
    const out = new Map<string, { count: number; providerRequests: number }>();
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

  function usageOf(
    reader: CachedReader,
    usage: Record<string, unknown> | undefined,
  ): ModelObservabilityCallListItem["usage"] {
    if (!reader.hasAccounting) {
      return { availability: "projection_unavailable", status: null, summary: null };
    }
    if (!usage) {
      return { availability: "not_correlated", status: null, summary: null };
    }
    return {
      availability: "present",
      status: textOrNull(usage.usage_status),
      summary: {
        inputTokens: intOrNull(usage.input_total_tokens),
        outputTokens: intOrNull(usage.output_total_tokens),
        reasoningTokens: intOrNull(usage.reasoning_tokens),
        cacheReadTokens: intOrNull(usage.cache_read_tokens),
        cacheWriteTokens: intOrNull(usage.cache_write_tokens),
        totalTokens: intOrNull(usage.total_tokens),
        costTotal: intOrNull(usage.cost_total),
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
    },
  ): ModelObservabilityCallListItem {
    const startedAt = textOrNull(row.started_at);
    const endedAt = textOrNull(row.ended_at);
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
      provenancePrecision: textOrNull(row.provenance_precision),
      provenance: {
        sectionCount: intOrNull(row.provenance_section_count),
        opaqueCount: intOrNull(row.provenance_opaque_count),
        categories: parseCategories(row.provenance_categories_json),
      },
      payloadAvailability: payloadAvailabilityOf(row.payload_availability, extras.payloadRecordCount),
      payloadRecordCount: extras.payloadRecordCount,
      usage: usageOf(reader, extras.usageRow),
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
      const filterSql = buildCallFilterSql(query.filter, "c");
      const clauses = [filterSql.sql];
      const params = [...filterSql.params];
      if (query.origin) {
        clauses.push("t.origin = ?");
        params.push(query.origin);
      }
      if (query.cursor) {
        const decoded = decodeModelObservabilityTraceCursor(query.cursor, query.filter, query.origin);
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
         ORDER BY (t.last_seen_at IS NULL) ASC, t.last_seen_at DESC, t.trace_id DESC
         LIMIT ?`,
      ).all(...params, query.limit + 1);

      const hasMore = rows.length > query.limit;
      const page = hasMore ? rows.slice(0, query.limit) : rows;
      const traces: ModelObservabilityTraceListItem[] = page.map((row) => ({
        traceId: String(row.trace_id),
        origin: textOrNull(row.origin),
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
          { lastSeenAt: textOrNull(last.last_seen_at), lastTraceId: String(last.trace_id) },
          query.filter,
          query.origin,
        );
      }
      return { traces, nextCursor };
    });
  }

  /* ── Aggregate Group By（§三十九～四十五：SQLite 内完成，不整表进内存）── */

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

  const USAGE_METRIC_SQL = `
    SUM(CASE WHEN u.model_call_id IS NOT NULL THEN 1 ELSE 0 END) AS usage_covered,
    SUM(CASE WHEN u.model_call_id IS NOT NULL AND u.usage_status = 'usage_missing' THEN 1 ELSE 0 END) AS usage_missing,
    SUM(COALESCE(u.input_total_tokens, 0)) AS input_tokens,
    SUM(COALESCE(u.output_total_tokens, 0)) AS output_tokens,
    SUM(COALESCE(u.reasoning_tokens, 0)) AS reasoning_tokens,
    SUM(COALESCE(u.cache_read_tokens, 0)) AS cache_read_tokens,
    SUM(COALESCE(u.cache_write_tokens, 0)) AS cache_write_tokens,
    SUM(COALESCE(u.total_tokens, 0)) AS total_tokens,
    SUM(u.cost_total) AS cost_total,
    SUM(CASE WHEN u.cache_hit = 1 THEN 1 ELSE 0 END) AS cache_hit_count,
    SUM(CASE WHEN u.cache_hit IS NOT NULL THEN 1 ELSE 0 END) AS cache_observed_count`;

  /** v1（无 model_call_usage 表）：usage 指标全 0 / NULL，availability 由 queryHealth 表达。 */
  const ZERO_USAGE_METRIC_SQL = `
    0 AS usage_covered, 0 AS usage_missing,
    0 AS input_tokens, 0 AS output_tokens, 0 AS reasoning_tokens,
    0 AS cache_read_tokens, 0 AS cache_write_tokens, 0 AS total_tokens,
    NULL AS cost_total, 0 AS cache_hit_count, 0 AS cache_observed_count`;

  function metricsFromRow(row: Record<string, unknown>): ModelObservabilityGroupMetrics {
    const observed = Number(row.duration_observed_count ?? 0);
    const totalMs = Number(row.duration_total_ms ?? 0);
    return {
      callCount: Number(row.call_count ?? 0),
      traceCount: Number(row.trace_count ?? 0),
      okCount: Number(row.ok_count ?? 0),
      errorCount: Number(row.error_count ?? 0),
      abortedCount: Number(row.aborted_count ?? 0),
      incompleteCount: Number(row.incomplete_count ?? 0),
      attemptCount: Number(row.attempt_count ?? 0),
      durationObservedCount: observed,
      durationTotalMs: totalMs,
      durationAverageMs: observed > 0 ? Math.round(totalMs / observed) : null,
      usageCoveredCalls: Number(row.usage_covered ?? 0),
      usageMissingCalls: Number(row.usage_missing ?? 0),
      inputTokens: Number(row.input_tokens ?? 0),
      outputTokens: Number(row.output_tokens ?? 0),
      reasoningTokens: Number(row.reasoning_tokens ?? 0),
      cacheReadTokens: Number(row.cache_read_tokens ?? 0),
      cacheWriteTokens: Number(row.cache_write_tokens ?? 0),
      totalTokens: Number(row.total_tokens ?? 0),
      costTotal: row.cost_total == null ? null : Number(row.cost_total),
      cacheHitCount: Number(row.cache_hit_count ?? 0),
      cacheObservedCount: Number(row.cache_observed_count ?? 0),
    };
  }

  function queryAggregate(query: NormalizedModelObservabilityAggregateQuery): ModelObservabilityQueryResult<ModelObservabilityAggregateResult> {
    return runQuery((reader) => {
      const filterSql = buildCallFilterSql(query.filter, "mc");
      const fromSql = reader.hasAccounting
        ? "FROM model_calls mc LEFT JOIN model_call_usage u ON u.model_call_id = mc.call_id"
        : "FROM model_calls mc";
      const usageMetrics = reader.hasAccounting ? USAGE_METRIC_SQL : ZERO_USAGE_METRIC_SQL;

      // 维度表达式 → g0..gN 别名（闭集映射；date 绑定 offset 参数，§四十三）。
      const groupExpressions: Array<{ dimension: ModelObservabilityGroupByDimension; column: string | null; expr: string; params: unknown[] }> = [];
      for (const dimension of query.groupBy) {
        if (dimension === "date") {
          groupExpressions.push({
            dimension,
            column: null,
            expr: `strftime('%Y-%m-%d', mc.started_at, printf('%+d minutes', ?))`,
            params: [query.dateBucket?.utcOffsetMinutes ?? 0],
          });
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
        return { groups: [], overall: metricsFromRow(overallRow) };
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
        return { key: keyParts.join("::"), values, metrics: metricsFromRow(row) };
      });
      return { groups, overall: metricsFromRow(overallRow) };
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
        providerRequestOrdinal: intOrNull(row.provider_request_ordinal),
        capturedAt: String(row.captured_at ?? ""),
        visibility: String(row.visibility ?? ""),
        fidelity: String(row.fidelity ?? ""),
        sanitizationStatus: String(row.sanitization_status ?? ""),
        redacted: boolFlag(row.redacted),
        truncated: boolFlag(row.truncated),
        degraded: boolFlag(row.degraded),
        recordCharCount: intOrNull(row.record_char_count),
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
          httpStatus: intOrNull(attempt.http_status),
          attemptVisibility: textOrNull(attempt.attempt_visibility),
          providerWireVisibility: textOrNull(attempt.provider_wire_visibility),
          errorName: textOrNull(attempt.error_name),
          errorCode: textOrNull(attempt.error_code),
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
        usageAggregate = { availability: "projection_unavailable", summary: null };
      } else {
        const rows = [...usage.values()];
        if (rows.length === 0) {
          usageAggregate = { availability: "not_correlated", summary: null };
        } else {
          const sum = (key: string): number => rows.reduce((acc, row) => acc + (Number(row[key] ?? 0) || 0), 0);
          const costValues = rows
            .map((row) => row.cost_total)
            .filter((v) => v != null)
            .map(Number);
          usageAggregate = {
            availability: "present",
            summary: {
              inputTokens: sum("input_total_tokens"),
              outputTokens: sum("output_total_tokens"),
              reasoningTokens: sum("reasoning_tokens"),
              cacheReadTokens: sum("cache_read_tokens"),
              cacheWriteTokens: sum("cache_write_tokens"),
              totalTokens: sum("total_tokens"),
              costTotal: costValues.length > 0 ? costValues.reduce((a, b) => a + b, 0) : null,
            },
          };
        }
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
        providerRequestOrdinal: intOrNull(row.provider_request_ordinal),
        capturedAt: String(row.captured_at ?? ""),
        visibility: String(row.visibility ?? ""),
        fidelity: String(row.fidelity ?? ""),
        sanitizationStatus: String(row.sanitization_status ?? ""),
        redacted: boolFlag(row.redacted),
        truncated: boolFlag(row.truncated),
        degraded: boolFlag(row.degraded),
        recordCharCount: intOrNull(row.record_char_count),
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
      const parseProvenance = (raw: unknown): unknown => {
        if (typeof raw !== "string" || !raw) return null;
        try {
          return JSON.parse(raw);
        } catch {
          return null;
        }
      };
      return {
        ...metadata,
        contentAvailable,
        contentState,
        payload,
        semanticInputProvenance: parseProvenance(row.semantic_input_provenance_json),
        providerRequestProvenance: parseProvenance(row.provider_request_provenance_json),
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
          dataCompleteness: {
            droppedTraceEvents: 0,
            droppedPayloadRecords: 0,
            droppedBlobs: 0,
            interruptedByRestartCalls: 0,
          },
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
      return {
        queryStatus: "ready",
        queryStatusReason: null,
        schemaVersion: reader.schemaVersion,
        accountingProjectionAvailable: reader.hasAccounting,
        oldestCallAt: textOrNull(counts.oldest),
        newestCallAt: textOrNull(counts.newest),
        callCount: Number(counts.calls ?? 0),
        traceCount: Number(counts.traces ?? 0),
        payloadRecordCount: Number(counts.payloads ?? 0),
        usageProjectionCount: usageCount,
        dataCompleteness: readDataCompleteness(reader.db),
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

  function getStoredBlob(blobId: string, { includeBytes = true }: { includeBytes?: boolean } = {}): ModelObservabilityQueryResult<ModelObservabilityStoredBlob> {
    if (typeof blobId !== "string" || !MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test(blobId)) {
      return fail("invalid_blob_id", "blob id has an invalid format", "invalid_blob_id");
    }
    return runQuery((reader) => {
      const row = reader.db.prepare(
        `SELECT blob_id, byte_length, media_type, state FROM blob_objects WHERE blob_id = ?`,
      ).get(blobId);
      if (!row) throw new NotFoundError("blob not found", "blob_not_found");
      if (row.state === "missing") throw new BlobMissingError("blob file is marked missing");
      const shard = blobId.slice(0, 2).replace(/[^a-z0-9]/gi, "0") || "00";
      const root = modelObservabilityBlobsRoot(lingxiHome);
      const abs = path.resolve(root, shard, `${blobId}.bin`);
      if (!abs.startsWith(path.resolve(root) + path.sep)) {
        // 理论上 blobId 闭集已杜绝；双保险，命中即数据异常，不当 500 抛出去。
        throw new BlobMissingError("blob path escapes store root");
      }
      let bytes: Buffer | null = null;
      let byteLength = Number(row.byte_length ?? 0) || 0;
      try {
        if (includeBytes) {
          bytes = fs.readFileSync(abs);
          byteLength = bytes.byteLength;
        } else {
          byteLength = fs.statSync(abs).size;
        }
      } catch {
        throw new BlobMissingError("blob file is missing on disk");
      }
      return {
        blobId,
        mediaType: typeof row.media_type === "string" && row.media_type ? row.media_type : null,
        byteLength,
        bytes,
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

/** Stored blob 读取结果（server-only：bytes 是 Buffer，不进 browser 契约）。 */
export type ModelObservabilityStoredBlob = {
  blobId: string;
  mediaType: string | null;
  byteLength: number;
  bytes: Buffer | null;
};

export type ModelObservabilityQueryService = ReturnType<typeof createModelObservabilityQueryService>;
