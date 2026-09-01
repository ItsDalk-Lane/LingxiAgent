/**
 * model-observability-query-types.ts — Phase 8 统一 Query Contract。
 *
 * 同一份 Filter Contract 驱动：Call 列表 / Trace 列表 / Aggregate Group By /
 * Drill-down（call/trace/payload detail）/ Export（§十七：route 不拼 SQL、
 * export 不另起一套 filter、UI 不知 SQL）。
 *
 * 安全纪律（§二十一/六十六）：
 *   - sort / groupBy / metric / dimension 全部闭集映射到 SQL 列，禁止
 *     ORDER BY ${userInput} / GROUP BY ${userInput}；
 *   - 所有 value 走绑定参数；
 *   - normalizeModelObservabilityQuery 对 unknown field / invalid enum /
 *     oversized array / invalid date / invalid cursor 一律显式 400，不静默吞。
 *
 * 日期语义（§四十四）：since inclusive、until exclusive，全接口统一。
 * category 语义（§十九）：category ≡ subsystem（与旧 Usage UI 一致），
 * callPurpose / operation 是独立维度。
 */

import { createHash } from "node:crypto";
import {
  MODEL_OBSERVABILITY_FILTER_MAX_VALUES,
  MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS,
  MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS,
  MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT,
  MODEL_OBSERVABILITY_PAGE_MAX_LIMIT,
  MODEL_OBSERVABILITY_SORT_KEYS,
  type ModelObservabilityDateBucket,
  type ModelObservabilityGroupByDimension,
  type ModelObservabilityMultiValueField,
  type ModelObservabilitySortKey,
} from "../../shared/model-observability-api-contract.ts";

/* ── Browser-safe wire contract re-export（Phase 9 §九）───────────────────
 *
 * 纯 JSON wire DTO/枚举的唯一事实源是
 * shared/model-observability-api-contract.ts（无 node 依赖，renderer 可用）。
 * 本文件继续负责 normalize / cursor / fingerprint / server 侧类型；既有
 * import 站点不变（全部经此 re-export）。
 */
export {
  MODEL_OBSERVABILITY_FILTER_MAX_VALUES,
  MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS,
  MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS,
  MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT,
  MODEL_OBSERVABILITY_PAGE_MAX_LIMIT,
  MODEL_OBSERVABILITY_SORT_KEYS,
  MODEL_OBSERVABILITY_TERMINAL_STATUSES,
  MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES,
  MODEL_OBSERVABILITY_USAGE_AVAILABILITIES,
  MODEL_OBSERVABILITY_USAGE_AGGREGATE_AVAILABILITIES,
} from "../../shared/model-observability-api-contract.ts";
export type {
  ModelObservabilityCallFilterInput,
  ModelObservabilityMultiValueField,
  ModelObservabilitySortKey,
  ModelObservabilityGroupByDimension,
  ModelObservabilityDateBucket,
  ModelObservabilityTerminalStatus,
  ModelObservabilityPayloadAvailability,
  ModelObservabilityUsageAvailability,
  ModelObservabilityUsageAggregateAvailability,
  ModelObservabilityUsageSummary,
  ModelObservabilityCallListItem,
  ModelObservabilityDataCompleteness,
  ModelObservabilityCallPage,
  ModelObservabilityTraceListItem,
  ModelObservabilityTracePage,
  ModelObservabilityGroupMetrics,
  ModelObservabilityGroupValues,
  ModelObservabilityGroupBucket,
  ModelObservabilityAggregateResult,
} from "../../shared/model-observability-api-contract.ts";

/* ── Filter contract ─────────────────────────────────────────────────── */

export type ModelObservabilityNormalizedFilter = {
  since: string | null;
  until: string | null;
  traceId: string | null;
  parentCallId: string | null;
  callId: string | null;
  multi: Partial<Record<ModelObservabilityMultiValueField, string[]>>;
  interruptedByRestart: boolean | null;
  hasPayload: boolean | null;
};

export type NormalizedModelObservabilityQuery = {
  filter: ModelObservabilityNormalizedFilter;
  sort: ModelObservabilitySortKey;
  limit: number;
  cursor: string | null;
};

/** 已归一的空 filter（「不过滤」的规范表达；测试/内部调用用）。 */
export const EMPTY_MODEL_OBSERVABILITY_FILTER: ModelObservabilityNormalizedFilter = {
  since: null,
  until: null,
  traceId: null,
  parentCallId: null,
  callId: null,
  multi: {},
  interruptedByRestart: null,
  hasPayload: null,
};

/* ── Group By contract ──────────────────────────────────────────────── */

export type NormalizedModelObservabilityAggregateQuery = {
  filter: ModelObservabilityNormalizedFilter;
  groupBy: ModelObservabilityGroupByDimension[];
  dateBucket: ModelObservabilityDateBucket | null;
};

/* ── normalize ──────────────────────────────────────────────────────── */

export type ModelObservabilityQueryNormalizerError = {
  code:
    | "unknown_field"
    | "invalid_enum"
    | "invalid_filter"
    | "invalid_date"
    | "invalid_limit"
    | "invalid_cursor";
  message: string;
  field?: string;
};

export type NormalizedQueryResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: ModelObservabilityQueryNormalizerError };

const TERMINAL_STATUS_VALUES = new Set(["ok", "error", "aborted", "incomplete"]);
const PAYLOAD_AVAILABILITY_VALUES = new Set([
  "present",
  "expired",
  "dropped",
  "not_captured",
  "unknown",
]);

const KNOWN_FILTER_FIELDS = new Set([
  "since", "until", "traceId", "parentCallId", "callId",
  "provider", "modelId", "api", "subsystem", "category", "operation",
  "surface", "trigger", "callPurpose", "terminalStatus", "attributionKind",
  "sessionId", "sessionPath", "conversationId", "conversationType",
  "agentId", "childAgentId", "childSessionId", "taskId",
  "inputShape", "provenancePrecision", "payloadAvailability",
  "interruptedByRestart", "hasPayload",
]);

/** category → subsystem 的 alias 映射后的多值字段集合。 */
const MULTI_FIELDS = new Set<ModelObservabilityMultiValueField>([
  "provider", "modelId", "api", "subsystem", "operation", "surface", "trigger",
  "callPurpose", "terminalStatus", "attributionKind", "sessionId", "sessionPath",
  "conversationId", "conversationType", "agentId", "childAgentId", "childSessionId",
  "taskId", "inputShape", "provenancePrecision", "payloadAvailability",
]);

function normalizeIsoDate(value: unknown, field: string): NormalizedQueryResult<string> {
  if (typeof value !== "string" || !value.trim()) {
    return { ok: false, error: { code: "invalid_date", message: `${field} must be an ISO-8601 string`, field } };
  }
  const trimmed = value.trim();
  const parsed = new Date(trimmed);
  if (Number.isNaN(parsed.getTime())) {
    return { ok: false, error: { code: "invalid_date", message: `${field} is not a valid ISO-8601 date`, field } };
  }
  // 统一成全零填充 UTC ISO（字典序比较语义：started_at 全部为同格式 ISO 串）。
  return { ok: true, value: parsed.toISOString() };
}

function normalizeTextOrNull(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function normalizeMultiValue(
  field: ModelObservabilityMultiValueField,
  value: unknown,
): NormalizedQueryResult<string[] | null> {
  const values = Array.isArray(value) ? value : [value];
  const out: string[] = [];
  for (const item of values) {
    const text = typeof item === "string" ? item.trim() : "";
    if (!text) continue;
    if (field === "terminalStatus" && !TERMINAL_STATUS_VALUES.has(text)) {
      return { ok: false, error: { code: "invalid_enum", message: `${field} value "${text}" is not a terminal status`, field } };
    }
    if (field === "payloadAvailability" && !PAYLOAD_AVAILABILITY_VALUES.has(text)) {
      return { ok: false, error: { code: "invalid_enum", message: `${field} value "${text}" is not a payload availability`, field } };
    }
    if (text.length > 1024) {
      return { ok: false, error: { code: "invalid_filter", message: `${field} value exceeds 1024 chars`, field } };
    }
    out.push(text);
  }
  if (out.length > MODEL_OBSERVABILITY_FILTER_MAX_VALUES) {
    return { ok: false, error: { code: "invalid_filter", message: `${field} exceeds ${MODEL_OBSERVABILITY_FILTER_MAX_VALUES} values`, field } };
  }
  return { ok: true, value: out.length > 0 ? out : null };
}

export function normalizeModelObservabilityFilter(
  input: unknown,
): NormalizedQueryResult<ModelObservabilityNormalizedFilter> {
  if (input === null || input === undefined) {
    input = {};
  }
  if (typeof input !== "object" || Array.isArray(input)) {
    return { ok: false, error: { code: "invalid_filter", message: "filter must be an object" } };
  }
  const source = input as Record<string, unknown>;
  for (const key of Object.keys(source)) {
    if (!KNOWN_FILTER_FIELDS.has(key)) {
      return { ok: false, error: { code: "unknown_field", message: `unknown filter field "${key}"`, field: key } };
    }
  }

  const filter: ModelObservabilityNormalizedFilter = {
    since: null,
    until: null,
    traceId: null,
    parentCallId: null,
    callId: null,
    multi: {},
    interruptedByRestart: null,
    hasPayload: null,
  };

  if (source.since !== undefined && source.since !== null && source.since !== "") {
    const since = normalizeIsoDate(source.since, "since");
    if (since.ok === false) return since;
    filter.since = since.value;
  }
  if (source.until !== undefined && source.until !== null && source.until !== "") {
    const until = normalizeIsoDate(source.until, "until");
    if (until.ok === false) return until;
    filter.until = until.value;
  }
  if (filter.since && filter.until && filter.since >= filter.until) {
    return { ok: false, error: { code: "invalid_date", message: "since must be before until (since inclusive, until exclusive)", field: "since" } };
  }
  filter.traceId = source.traceId !== undefined ? normalizeTextOrNull(source.traceId) : null;
  filter.parentCallId = source.parentCallId !== undefined ? normalizeTextOrNull(source.parentCallId) : null;
  filter.callId = source.callId !== undefined ? normalizeTextOrNull(source.callId) : null;

  for (const field of MULTI_FIELDS) {
    let raw = source[field];
    if (field === "subsystem" && raw === undefined) {
      raw = source.category; // §十九 alias：category ≡ subsystem。
    }
    if (field === "subsystem" && raw !== undefined && source.category !== undefined
      && source.subsystem !== undefined) {
      return { ok: false, error: { code: "invalid_filter", message: "specify either subsystem or category, not both", field: "category" } };
    }
    if (raw === undefined || raw === null) continue;
    const result = normalizeMultiValue(field, raw);
    if (result.ok === false) return result;
    if (result.value) filter.multi[field] = result.value;
  }

  if (source.interruptedByRestart !== undefined && source.interruptedByRestart !== null) {
    if (typeof source.interruptedByRestart !== "boolean") {
      return { ok: false, error: { code: "invalid_filter", message: "interruptedByRestart must be a boolean", field: "interruptedByRestart" } };
    }
    filter.interruptedByRestart = source.interruptedByRestart;
  }
  if (source.hasPayload !== undefined && source.hasPayload !== null) {
    if (typeof source.hasPayload !== "boolean") {
      return { ok: false, error: { code: "invalid_filter", message: "hasPayload must be a boolean", field: "hasPayload" } };
    }
    filter.hasPayload = source.hasPayload;
  }
  return { ok: true, value: filter };
}

export function normalizeModelObservabilityQuery(input: unknown): NormalizedQueryResult<NormalizedModelObservabilityQuery> {
  if (input === null || input === undefined) input = {};
  if (typeof input !== "object" || Array.isArray(input)) {
    return { ok: false, error: { code: "invalid_filter", message: "query body must be an object" } };
  }
  const source = input as Record<string, unknown>;
  const allowed = new Set(["filter", "sort", "limit", "cursor"]);
  for (const key of Object.keys(source)) {
    if (!allowed.has(key)) {
      return { ok: false, error: { code: "unknown_field", message: `unknown query field "${key}"`, field: key } };
    }
  }
  const filter = normalizeModelObservabilityFilter(source.filter);
  if (filter.ok === false) return filter;

  let sort: ModelObservabilitySortKey = "started_at_desc";
  if (source.sort !== undefined && source.sort !== null) {
    if (source.sort !== "started_at_desc") {
      return { ok: false, error: { code: "invalid_enum", message: `sort "${String(source.sort)}" is not supported`, field: "sort" } };
    }
    sort = source.sort;
  }

  let limit = MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT;
  if (source.limit !== undefined && source.limit !== null) {
    const n = Number(source.limit);
    if (!Number.isInteger(n) || n <= 0 || n > MODEL_OBSERVABILITY_PAGE_MAX_LIMIT) {
      return { ok: false, error: { code: "invalid_limit", message: `limit must be an integer in 1..${MODEL_OBSERVABILITY_PAGE_MAX_LIMIT}`, field: "limit" } };
    }
    limit = n;
  }

  let cursor: string | null = null;
  if (source.cursor !== undefined && source.cursor !== null) {
    if (typeof source.cursor !== "string" || !source.cursor || source.cursor.length > 512) {
      return { ok: false, error: { code: "invalid_cursor", message: "cursor must be a bounded opaque string", field: "cursor" } };
    }
    cursor = source.cursor;
  }
  return { ok: true, value: { filter: filter.value, sort, limit, cursor } };
}

export function normalizeModelObservabilityAggregateQuery(input: unknown): NormalizedQueryResult<NormalizedModelObservabilityAggregateQuery> {
  if (input === null || input === undefined) input = {};
  if (typeof input !== "object" || Array.isArray(input)) {
    return { ok: false, error: { code: "invalid_filter", message: "aggregate body must be an object" } };
  }
  const source = input as Record<string, unknown>;
  const allowed = new Set(["filter", "groupBy", "dateBucket"]);
  for (const key of Object.keys(source)) {
    if (!allowed.has(key)) {
      return { ok: false, error: { code: "unknown_field", message: `unknown aggregate field "${key}"`, field: key } };
    }
  }
  const filter = normalizeModelObservabilityFilter(source.filter);
  if (filter.ok === false) return filter;

  const groupByRaw = Array.isArray(source.groupBy) ? source.groupBy : source.groupBy === undefined ? [] : [source.groupBy];
  const groupBy: ModelObservabilityGroupByDimension[] = [];
  for (const item of groupByRaw) {
    if (typeof item !== "string" || !(MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS as readonly string[]).includes(item)) {
      return { ok: false, error: { code: "invalid_enum", message: `groupBy "${String(item)}" is not a supported dimension`, field: "groupBy" } };
    }
    const dim = item as ModelObservabilityGroupByDimension;
    if (!groupBy.includes(dim)) groupBy.push(dim);
  }
  if (groupBy.length > MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS) {
    return { ok: false, error: { code: "invalid_filter", message: `groupBy supports at most ${MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS} dimensions`, field: "groupBy" } };
  }

  let dateBucket: ModelObservabilityDateBucket | null = null;
  if (source.dateBucket !== undefined && source.dateBucket !== null) {
    const bucket = source.dateBucket as Record<string, unknown>;
    if (!bucket || typeof bucket !== "object" || Array.isArray(bucket)) {
      return { ok: false, error: { code: "invalid_filter", message: "dateBucket must be an object", field: "dateBucket" } };
    }
    for (const key of Object.keys(bucket)) {
      if (key !== "bucket" && key !== "utcOffsetMinutes" && key !== "timeZone") {
        // 未知字段显式拒绝（Phase 10 DST 专项：修复前 timeZone 被静默忽略成 UTC）。
        return { ok: false, error: { code: "unknown_field", message: `unknown dateBucket field "${key}"`, field: `dateBucket.${key}` } };
      }
    }
    if (bucket.bucket !== "day") {
      return { ok: false, error: { code: "invalid_enum", message: `dateBucket.bucket "${String(bucket.bucket)}" is not supported (only "day")`, field: "dateBucket.bucket" } };
    }
    const hasTimeZone = bucket.timeZone !== undefined && bucket.timeZone !== null;
    const hasOffset = bucket.utcOffsetMinutes !== undefined && bucket.utcOffsetMinutes !== null;
    if (hasTimeZone && hasOffset) {
      return { ok: false, error: { code: "invalid_filter", message: "dateBucket: specify either timeZone or utcOffsetMinutes, not both", field: "dateBucket.timeZone" } };
    }
    if (hasTimeZone) {
      const timeZone = String(bucket.timeZone);
      if (!isValidIanaTimeZone(timeZone)) {
        return { ok: false, error: { code: "invalid_filter", message: `dateBucket.timeZone "${timeZone}" is not a valid IANA time zone`, field: "dateBucket.timeZone" } };
      }
      dateBucket = { bucket: "day", timeZone };
    } else {
      const offsetRaw = bucket.utcOffsetMinutes === undefined ? 0 : bucket.utcOffsetMinutes;
      const offset = Number(offsetRaw);
      if (!Number.isInteger(offset) || offset < -1440 || offset > 1440) {
        return { ok: false, error: { code: "invalid_filter", message: "dateBucket.utcOffsetMinutes must be an integer in -1440..1440", field: "dateBucket.utcOffsetMinutes" } };
      }
      dateBucket = { bucket: "day", utcOffsetMinutes: offset };
    }
  }
  if (groupBy.includes("date") && !dateBucket) {
    dateBucket = { bucket: "day", utcOffsetMinutes: 0 };
  }
  if (!groupBy.includes("date") && dateBucket) {
    return { ok: false, error: { code: "invalid_filter", message: "dateBucket requires groupBy dimension \"date\"", field: "dateBucket" } };
  }
  return { ok: true, value: { filter: filter.value, groupBy, dateBucket } };
}

/* ── Trace list query（§二十八：filter 语义 = trace 中至少存在一条符合
 *    filter 的 call；origin 是 trace 自身维度）────────────────────────── */

export type NormalizedModelObservabilityTraceQuery = {
  filter: ModelObservabilityNormalizedFilter;
  origin: string | null;
  /** 轨迹列表只保留 call 总数 ≥ 该值的 trace（null = 不过滤；单次调用 trace 由调用台账覆盖）。 */
  minCallCount: number | null;
  limit: number;
  cursor: string | null;
};

export function normalizeModelObservabilityTraceQuery(input: unknown): NormalizedQueryResult<NormalizedModelObservabilityTraceQuery> {
  if (input === null || input === undefined) input = {};
  if (typeof input !== "object" || Array.isArray(input)) {
    return { ok: false, error: { code: "invalid_filter", message: "trace query body must be an object" } };
  }
  const source = input as Record<string, unknown>;
  const allowed = new Set(["filter", "origin", "minCallCount", "limit", "cursor"]);
  for (const key of Object.keys(source)) {
    if (!allowed.has(key)) {
      return { ok: false, error: { code: "unknown_field", message: `unknown trace query field "${key}"`, field: key } };
    }
  }
  const filter = normalizeModelObservabilityFilter(source.filter);
  if (filter.ok === false) return filter;

  let origin: string | null = null;
  if (source.origin !== undefined && source.origin !== null) {
    if (typeof source.origin !== "string" || !source.origin.trim() || source.origin.length > 64) {
      return { ok: false, error: { code: "invalid_filter", message: "origin must be a bounded string", field: "origin" } };
    }
    origin = source.origin.trim();
  }

  let minCallCount: number | null = null;
  if (source.minCallCount !== undefined && source.minCallCount !== null) {
    const n = Number(source.minCallCount);
    if (!Number.isInteger(n) || n < 1 || n > 1000) {
      return { ok: false, error: { code: "invalid_filter", message: "minCallCount must be an integer in 1..1000", field: "minCallCount" } };
    }
    minCallCount = n;
  }

  let limit = MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT;
  if (source.limit !== undefined && source.limit !== null) {
    const n = Number(source.limit);
    if (!Number.isInteger(n) || n <= 0 || n > MODEL_OBSERVABILITY_PAGE_MAX_LIMIT) {
      return { ok: false, error: { code: "invalid_limit", message: `limit must be an integer in 1..${MODEL_OBSERVABILITY_PAGE_MAX_LIMIT}`, field: "limit" } };
    }
    limit = n;
  }

  let cursor: string | null = null;
  if (source.cursor !== undefined && source.cursor !== null) {
    if (typeof source.cursor !== "string" || !source.cursor || source.cursor.length > 512) {
      return { ok: false, error: { code: "invalid_cursor", message: "cursor must be a bounded opaque string", field: "cursor" } };
    }
    cursor = source.cursor;
  }
  return { ok: true, value: { filter: filter.value, origin, minCallCount, limit, cursor } };
}

/* ── Cursor codec（§二十五/二十六：opaque、bounded、与 filter 绑定）──── */

const CURSOR_VERSION = 1;
const CURSOR_MAX_CHARS = 512;

function base64UrlEncode(text: string): string {
  return Buffer.from(text, "utf-8").toString("base64url");
}

function base64UrlDecode(text: string): string | null {
  try {
    return Buffer.from(text, "base64url").toString("utf-8");
  } catch {
    return null;
  }
}

/**
 * normalized filter 的稳定指纹（cursor 与 query 绑定，§二十六：filter 改变
 * → cursor invalid）。canonical JSON：固定 key 顺序。extra 携带该查询形态
 * 特有的额外维度（如 trace 查询的 origin）。
 */
export function modelObservabilityQueryFingerprint(
  filter: ModelObservabilityNormalizedFilter,
  sort: ModelObservabilitySortKey,
  extra?: Record<string, unknown>,
): string {
  const multi: Record<string, string[]> = {};
  for (const key of Object.keys(filter.multi).sort()) {
    const field = key as ModelObservabilityMultiValueField;
    multi[key] = [...(filter.multi[field] ?? [])].sort();
  }
  const canonical = JSON.stringify({
    since: filter.since,
    until: filter.until,
    traceId: filter.traceId,
    parentCallId: filter.parentCallId,
    callId: filter.callId,
    multi,
    interruptedByRestart: filter.interruptedByRestart,
    hasPayload: filter.hasPayload,
    sort,
    ...(extra ?? {}),
  });
  return createHash("sha256").update(canonical).digest("hex").slice(0, 16);
}

export type ModelObservabilityKeysetCursor = {
  lastStartedAt: string | null;
  lastCallId: string;
};

export type ModelObservabilityTraceKeysetCursor = {
  lastSeenAt: string | null;
  lastTraceId: string;
};

/** 编码 call keyset cursor；不抛异常。 */
export function encodeModelObservabilityCallCursor(
  position: ModelObservabilityKeysetCursor,
  filter: ModelObservabilityNormalizedFilter,
  sort: ModelObservabilitySortKey,
): string {
  const payload = {
    v: CURSOR_VERSION,
    kind: "calls" as const,
    fp: modelObservabilityQueryFingerprint(filter, sort),
    s: position.lastStartedAt,
    c: position.lastCallId,
  };
  return base64UrlEncode(JSON.stringify(payload));
}

export function decodeModelObservabilityCallCursor(
  raw: string,
  filter: ModelObservabilityNormalizedFilter,
  sort: ModelObservabilitySortKey,
): NormalizedQueryResult<ModelObservabilityKeysetCursor> {
  const invalid = (message: string): NormalizedQueryResult<ModelObservabilityKeysetCursor> => ({
    ok: false,
    error: { code: "invalid_cursor", message, field: "cursor" },
  });
  if (typeof raw !== "string" || !raw || raw.length > CURSOR_MAX_CHARS) {
    return invalid("cursor is missing or oversized");
  }
  let decoded: unknown;
  try {
    const text = base64UrlDecode(raw);
    if (text === null) return invalid("cursor is not valid base64url");
    decoded = JSON.parse(text);
  } catch {
    return invalid("cursor payload is corrupt");
  }
  if (!decoded || typeof decoded !== "object" || Array.isArray(decoded)) {
    return invalid("cursor payload is not an object");
  }
  const payload = decoded as Record<string, unknown>;
  if (payload.v !== CURSOR_VERSION || payload.kind !== "calls") {
    return invalid("cursor version/kind mismatch");
  }
  if (payload.fp !== modelObservabilityQueryFingerprint(filter, sort)) {
    return invalid("cursor was issued for a different query");
  }
  const lastCallId = typeof payload.c === "string" && payload.c ? payload.c : "";
  if (!lastCallId || lastCallId.length > 256) {
    return invalid("cursor call id is invalid");
  }
  const startedAt = typeof payload.s === "string" && payload.s ? payload.s : null;
  if (startedAt && Number.isNaN(new Date(startedAt).getTime())) {
    return invalid("cursor started_at is not a date");
  }
  return { ok: true, value: { lastStartedAt: startedAt, lastCallId } };
}

export function encodeModelObservabilityTraceCursor(
  position: ModelObservabilityTraceKeysetCursor,
  filter: ModelObservabilityNormalizedFilter,
  origin: string | null = null,
  minCallCount: number | null = null,
): string {
  const payload = {
    v: CURSOR_VERSION,
    kind: "traces" as const,
    fp: modelObservabilityQueryFingerprint(filter, "started_at_desc", { origin, minCallCount }),
    s: position.lastSeenAt,
    t: position.lastTraceId,
  };
  return base64UrlEncode(JSON.stringify(payload));
}

export function decodeModelObservabilityTraceCursor(
  raw: string,
  filter: ModelObservabilityNormalizedFilter,
  origin: string | null = null,
  minCallCount: number | null = null,
): NormalizedQueryResult<ModelObservabilityTraceKeysetCursor> {
  const invalid = (message: string): NormalizedQueryResult<ModelObservabilityTraceKeysetCursor> => ({
    ok: false,
    error: { code: "invalid_cursor", message, field: "cursor" },
  });
  if (typeof raw !== "string" || !raw || raw.length > CURSOR_MAX_CHARS) {
    return invalid("cursor is missing or oversized");
  }
  let decoded: unknown;
  try {
    const text = base64UrlDecode(raw);
    if (text === null) return invalid("cursor is not valid base64url");
    decoded = JSON.parse(text);
  } catch {
    return invalid("cursor payload is corrupt");
  }
  if (!decoded || typeof decoded !== "object" || Array.isArray(decoded)) {
    return invalid("cursor payload is not an object");
  }
  const payload = decoded as Record<string, unknown>;
  if (payload.v !== CURSOR_VERSION || payload.kind !== "traces") {
    return invalid("cursor version/kind mismatch");
  }
  if (payload.fp !== modelObservabilityQueryFingerprint(filter, "started_at_desc", { origin, minCallCount })) {
    return invalid("cursor was issued for a different query");
  }
  const lastTraceId = typeof payload.t === "string" && payload.t ? payload.t : "";
  if (!lastTraceId || lastTraceId.length > 256) {
    return invalid("cursor trace id is invalid");
  }
  const lastSeenAt = typeof payload.s === "string" && payload.s ? payload.s : null;
  if (lastSeenAt && Number.isNaN(new Date(lastSeenAt).getTime())) {
    return invalid("cursor last_seen_at is not a date");
  }
  return { ok: true, value: { lastSeenAt, lastTraceId } };
}

/* ── Response DTO（§二十二：列表永远是轻量 metadata，无正文）────────────
 *
 * DTO 定义已迁移到 shared/model-observability-api-contract.ts（browser-safe
 * 单一事实源），经本文件头部 re-export 保持既有 import 站点不变。
 */

/* ── IANA time zone 校验（Phase 10 DST 专项）──────────────────────────── */

const ianaTimeZoneCache = new Map<string, boolean>();

/** Intl 构造试探（无网络/无磁盘；失败 = invalid_filter，不静默回落 UTC）。 */
export function isValidIanaTimeZone(timeZone: string): boolean {
  if (typeof timeZone !== "string" || !timeZone || timeZone.length > 64) return false;
  const cached = ianaTimeZoneCache.get(timeZone);
  if (cached !== undefined) return cached;
  let valid = false;
  try {
    new Intl.DateTimeFormat("en-US", { timeZone, timeZoneName: "longOffset" });
    valid = true;
  } catch {
    valid = false;
  }
  ianaTimeZoneCache.set(timeZone, valid);
  return valid;
}
