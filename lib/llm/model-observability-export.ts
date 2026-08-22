/**
 * model-observability-export.ts — Export Contract（Phase 8 §七十三～八十二）。
 *
 * 统一入口消费 NormalizedModelObservabilityQuery（§七十八：与 Query 完全
 * 相同的 Filter Contract，export 不另起一套 filter）。
 *
 *   - 独立版本：MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION=1，与 SQLite
 *     user_version 各自演化（§七十四）。
 *   - 默认 metadata-only（§七十五）：includePayloads=false 不导出正文；
 *     includePayloads=true 只导出**已 Sanitized 的** Payload Store 内容——
 *     系统根本不存在 raw payload store，也没有 includeRaw 选项（§七十六）。
 *   - 永不导出 blob bytes（§七十七）：external_blob descriptor / blob metadata
 *     之外的只有 blobIds 列表，不 base64。
 *   - JSONL streaming（§七十九/一百一十九）：async generator 按 keyset 页
 *     迭代，不 const all = queryEverything()。
 *   - bounded（§八十一）：maxCalls 上限；超限调用方返回显式 limit error
 *     （本模块 countMatchedCalls 由调用方先验），不 OOM。
 *   - 毒丸纪律（§一百一十八）：只读 Sanitized Payload Store，不碰任何 raw
 *     runtime object；OPAQUE/UNAVAILABLE/METADATA_ONLY 原样保留（§八十七）。
 */

import {
  MODEL_OBSERVABILITY_PAGE_MAX_LIMIT,
  type NormalizedModelObservabilityQuery,
} from "./model-observability-query-types.ts";
import type { ModelObservabilityQueryService } from "./model-observability-query.ts";

export const MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION = 1;

export const MODEL_OBSERVABILITY_EXPORT_DEFAULT_MAX_CALLS = 50_000;
export const MODEL_OBSERVABILITY_EXPORT_MAX_CALLS_LIMIT = 100_000;

export type ModelObservabilityExportOptions = {
  includePayloads: boolean;
  maxCalls: number;
};

export type ModelObservabilityExportManifest = {
  type: "manifest";
  exportSchemaVersion: number;
  exportedAt: string;
  includePayloads: boolean;
  storageSchemaVersion: number | null;
  totalCalls: number;
  backfillSource: string | null;
  dataCompleteness: Record<string, number> | null;
};

export type ModelObservabilityExportCallBundle = {
  type: "model_call";
  schemaVersion: number;
  call: unknown;
  trace: unknown;
  attempts: unknown;
  usage: unknown;
  payloads: unknown;
};

export type ModelObservabilityExportLimitError = {
  kind: "limit_error";
  matchedCalls: number;
  maxCalls: number;
};

export type ModelObservabilityExportStart =
  | { kind: "ready"; manifest: ModelObservabilityExportManifest; iterate: () => AsyncGenerator<string> }
  | { kind: "absent" }
  | { kind: "limit_error"; matchedCalls: number; maxCalls: number }
  | { kind: "unavailable"; reasonCode: string };

/** normalize export 请求（route 层 400 语义）。 */
export function normalizeModelObservabilityExportOptions(input: unknown): { ok: true; value: ModelObservabilityExportOptions } | { ok: false; error: { code: string; message: string } } {
  const source = input && typeof input === "object" && !Array.isArray(input) ? input as Record<string, unknown> : {};
  for (const key of Object.keys(source)) {
    if (key !== "query" && key !== "includePayloads" && key !== "maxCalls") {
      return { ok: false, error: { code: "unknown_field", message: `unknown export field "${key}"` } };
    }
  }
  let includePayloads = false;
  if (source.includePayloads !== undefined) {
    if (typeof source.includePayloads !== "boolean") {
      return { ok: false, error: { code: "invalid_filter", message: "includePayloads must be a boolean" } };
    }
    includePayloads = source.includePayloads;
  }
  let maxCalls = MODEL_OBSERVABILITY_EXPORT_DEFAULT_MAX_CALLS;
  if (source.maxCalls !== undefined && source.maxCalls !== null) {
    const n = Number(source.maxCalls);
    if (!Number.isInteger(n) || n <= 0 || n > MODEL_OBSERVABILITY_EXPORT_MAX_CALLS_LIMIT) {
      return { ok: false, error: { code: "invalid_limit", message: `maxCalls must be an integer in 1..${MODEL_OBSERVABILITY_EXPORT_MAX_CALLS_LIMIT}` } };
    }
    maxCalls = n;
  }
  return { ok: true, value: { includePayloads, maxCalls } };
}

/**
 * 启动一次 export：预检（query 可用性 + 预 count + limit）→ manifest +
 * streaming iterator。iterate() 每次产出一行 JSONL（含末尾 \n）。
 */
export function startModelObservabilityExport(
  queryService: ModelObservabilityQueryService,
  query: NormalizedModelObservabilityQuery,
  options: ModelObservabilityExportOptions,
): ModelObservabilityExportStart {
  const countResult = queryService.queryAggregate({
    filter: query.filter,
    groupBy: [],
    dateBucket: null,
  });
  if (countResult.ok === false) {
    if (countResult.error.code === "absent") return { kind: "absent" };
    return { kind: "unavailable", reasonCode: countResult.error.reasonCode ?? countResult.error.code };
  }
  const matchedCalls = countResult.value.overall.callCount;
  if (matchedCalls > options.maxCalls) {
    return { kind: "limit_error", matchedCalls, maxCalls: options.maxCalls };
  }
  const healthResult = queryService.getHealth();
  const health = healthResult.ok === true ? healthResult.value : null;

  const manifest: ModelObservabilityExportManifest = {
    type: "manifest",
    exportSchemaVersion: MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION,
    exportedAt: new Date().toISOString(),
    includePayloads: options.includePayloads,
    storageSchemaVersion: health?.schemaVersion ?? null,
    totalCalls: matchedCalls,
    // §十五：bounded ledger backfill 的诚实标注（有 accounting projection 才有）。
    backfillSource: health?.accountingProjectionAvailable ? "bounded_usage_ledger" : null,
    dataCompleteness: health?.dataCompleteness ?? null,
  };

  async function* iterate(): AsyncGenerator<string> {
    yield `${JSON.stringify(manifest)}\n`;
    let cursor: string | null = null;
    let exported = 0;
    // keyset 分页流式输出；异常页宁可中断也不静默截断（调用方看到的部分行
    // 都有 manifest.totalCalls 可对账）。
    while (exported < matchedCalls) {
      const page = queryService.queryCalls({ ...query, cursor, limit: MODEL_OBSERVABILITY_PAGE_MAX_LIMIT });
      if (page.ok === false) break;
      if (page.value.calls.length === 0) break;
      for (const call of page.value.calls) {
        const bundle = buildExportBundle(queryService, call.callId, options);
        if (!bundle) continue;
        yield `${JSON.stringify(bundle)}\n`;
        exported += 1;
      }
      cursor = page.value.nextCursor;
      if (!cursor) break;
    }
  }

  return { kind: "ready", manifest, iterate };
}

function buildExportBundle(
  queryService: ModelObservabilityQueryService,
  callId: string,
  options: ModelObservabilityExportOptions,
): ModelObservabilityExportCallBundle | null {
  const detail = queryService.queryCallDetail(callId);
  if (detail.ok === false) return null;
  const { call, trace, attempts, payloadRecords } = detail.value;
  const usage = (call as { usage?: unknown }).usage ?? null;
  let payloads: unknown = payloadRecords;
  if (options.includePayloads && payloadRecords.length > 0) {
    // 逐条 exact retrieval（§七十二：只返回一条 record 的端点语义同源）；
    // OPAQUE/UNAVAILABLE/corrupt 原样保留 metadata + contentState。
    const bodies = [];
    for (const record of payloadRecords) {
      const retrieved = queryService.getPayloadRecord(record.id);
      if (retrieved.ok === true) bodies.push(retrieved.value);
      else bodies.push(record);
    }
    payloads = bodies;
  }
  return {
    type: "model_call",
    schemaVersion: MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION,
    call,
    trace,
    attempts,
    usage,
    payloads,
  };
}
