/**
 * model-observability-accounting-projection.ts — Durable Accounting
 * Projection（Phase 8 §八～十六）。
 *
 * 关系（单向，§十二）：Provider → Usage Ledger（accounting truth source）→
 * model_call_usage（read-optimized durable projection）。projection 永远不
 * 反过来控制 Usage Ledger。
 *
 * 关联纪律（§十三）：只有 entry.metadata.modelCallId 存在且合法才投影；
 * 不通过时间/modelId/session/顺序猜测对应 call。无 modelCallId → 不投影。
 *
 * 内容边界（§十一）：只保存安全 numeric accounting + status + identity。
 * **绝不保存** ledger error.message（可能来自业务/Provider Error，Observable
 * Metadata Safe Contract 禁止其进入长期 projection）；error.name 同样不存
 * （model_calls.error_name 已有 observer 侧事实）。
 *
 * 幂等（§十四）：model_call_id 是 PRIMARY KEY，UPSERT；同一 modelCallId
 * 重复进入不产生 duplicate。写入必须位于 persistence writer 事务内
 * （§一百二十七：独立 projection writer，query 层不写）。
 */

/** Usage Ledger entry 的投影输入形状（lib/llm/usage-ledger.ts normalize 后）。 */
export type UsageLedgerEntryLike = {
  requestId?: unknown;
  startedAt?: unknown;
  endedAt?: unknown;
  durationMs?: unknown;
  status?: unknown;
  metadata?: unknown;
  usage?: unknown;
  rawUsageShape?: unknown;
};

export type ModelCallUsageRow = {
  model_call_id: string;
  usage_request_id: string | null;
  started_at: string | null;
  ended_at: string | null;
  duration_ms: number | null;
  usage_status: string;
  input_total_tokens: number | null;
  input_uncached_tokens: number | null;
  output_total_tokens: number | null;
  reasoning_tokens: number | null;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
  cache_miss_tokens: number | null;
  cache_hit: number | null;
  cache_created: number | null;
  cache_hit_ratio: number | null;
  total_tokens: number | null;
  cost_total: number | null;
  raw_usage_shape: string | null;
};

const USAGE_STATUSES = new Set(["ok", "error", "aborted", "usage_missing"]);

function textOrNull(value: unknown, max = 256): string | null {
  return typeof value === "string" && value.trim() ? value.trim().slice(0, max) : null;
}

function intOrNull(value: unknown): number | null {
  const n = Number(value);
  return Number.isFinite(n) ? Math.trunc(n) : null;
}

function ratioOrNull(value: unknown): number | null {
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}

/**
 * ledger entry → model_call_usage row。无合法 metadata.modelCallId → null
 * （§十三：不猜）。usage 为 null 时 numeric 列全 NULL、status 保留
 * ledger 事实（如 usage_missing）。
 */
export function modelCallUsageRowFromLedgerEntry(
  entry: UsageLedgerEntryLike,
): ModelCallUsageRow | null {
  if (!entry || typeof entry !== "object") return null;
  const metadata = entry.metadata && typeof entry.metadata === "object" && !Array.isArray(entry.metadata)
    ? entry.metadata as Record<string, unknown>
    : null;
  const modelCallId = textOrNull(metadata?.modelCallId, 256);
  if (!modelCallId) return null;

  const status = typeof entry.status === "string" && USAGE_STATUSES.has(entry.status)
    ? entry.status
    : "usage_missing";

  const usage = entry.usage && typeof entry.usage === "object" && !Array.isArray(entry.usage)
    ? entry.usage as Record<string, unknown>
    : null;
  const input = usage && usage.input && typeof usage.input === "object" ? usage.input as Record<string, unknown> : {};
  const output = usage && usage.output && typeof usage.output === "object" ? usage.output as Record<string, unknown> : {};
  const cache = usage && usage.cache && typeof usage.cache === "object" ? usage.cache as Record<string, unknown> : {};

  return {
    model_call_id: modelCallId,
    usage_request_id: textOrNull(entry.requestId, 256),
    started_at: textOrNull(entry.startedAt, 64),
    ended_at: textOrNull(entry.endedAt, 64),
    duration_ms: intOrNull(entry.durationMs),
    usage_status: status,
    input_total_tokens: intOrNull(input.totalTokens),
    input_uncached_tokens: intOrNull(input.uncachedTokens),
    output_total_tokens: intOrNull(output.totalTokens),
    reasoning_tokens: intOrNull(output.reasoningTokens),
    cache_read_tokens: intOrNull(cache.readTokens),
    cache_write_tokens: intOrNull(cache.writeTokens),
    cache_miss_tokens: intOrNull(cache.missTokens),
    cache_hit: typeof cache.hit === "boolean" ? (cache.hit ? 1 : 0) : intOrNull(cache.hit),
    cache_created: typeof cache.created === "boolean" ? (cache.created ? 1 : 0) : intOrNull(cache.created),
    cache_hit_ratio: ratioOrNull(cache.hitRatio),
    total_tokens: intOrNull(usage?.totalTokens),
    cost_total: ratioOrNull(usage?.costTotal),
    raw_usage_shape: textOrNull(entry.rawUsageShape, 512),
  };
}

/**
 * projection writer（在 coordinator 的 flush transaction 内调用；本模块
 * 不自行开事务——§一百二十七）。
 */
export function createModelObservabilityAccountingProjection({ db }: { db: any }) {
  const upsert = db.prepare(`
    INSERT INTO model_call_usage (
      model_call_id, usage_request_id, started_at, ended_at, duration_ms, usage_status,
      input_total_tokens, input_uncached_tokens, output_total_tokens, reasoning_tokens,
      cache_read_tokens, cache_write_tokens, cache_miss_tokens,
      cache_hit, cache_created, cache_hit_ratio,
      total_tokens, cost_total, raw_usage_shape, created_at, updated_at
    ) VALUES (
      @model_call_id, @usage_request_id, @started_at, @ended_at, @duration_ms, @usage_status,
      @input_total_tokens, @input_uncached_tokens, @output_total_tokens, @reasoning_tokens,
      @cache_read_tokens, @cache_write_tokens, @cache_miss_tokens,
      @cache_hit, @cache_created, @cache_hit_ratio,
      @total_tokens, @cost_total, @raw_usage_shape, @ts, @ts
    )
    ON CONFLICT(model_call_id) DO UPDATE SET
      usage_request_id = excluded.usage_request_id,
      started_at = excluded.started_at,
      ended_at = excluded.ended_at,
      duration_ms = excluded.duration_ms,
      usage_status = excluded.usage_status,
      input_total_tokens = excluded.input_total_tokens,
      input_uncached_tokens = excluded.input_uncached_tokens,
      output_total_tokens = excluded.output_total_tokens,
      reasoning_tokens = excluded.reasoning_tokens,
      cache_read_tokens = excluded.cache_read_tokens,
      cache_write_tokens = excluded.cache_write_tokens,
      cache_miss_tokens = excluded.cache_miss_tokens,
      cache_hit = excluded.cache_hit,
      cache_created = excluded.cache_created,
      cache_hit_ratio = excluded.cache_hit_ratio,
      total_tokens = excluded.total_tokens,
      cost_total = excluded.cost_total,
      raw_usage_shape = excluded.raw_usage_shape,
      updated_at = excluded.updated_at
  `);

  return {
    /** 幂等 upsert 一条 ledger entry（无 modelCallId 静默跳过并返回 false）。 */
    upsertLedgerEntry(entry: UsageLedgerEntryLike, options: { now?: () => string } = {}): boolean {
      const row = modelCallUsageRowFromLedgerEntry(entry);
      if (!row) return false;
      upsert.run({ ...row, ts: options.now?.() ?? new Date().toISOString() });
      return true;
    },
  };
}

export type ModelObservabilityAccountingProjection = ReturnType<typeof createModelObservabilityAccountingProjection>;

/** bounded ledger backfill 的 meta key（首次 v2 启用标记，§十五）。 */
export const MODEL_OBSERVABILITY_USAGE_BACKFILL_META_KEY = "usageLedgerBackfillCompletedAt";

/**
 * 从当前 bounded Usage Ledger（≤5000 条）做 best-effort 幂等 backfill。
 * 这**不是**完整历史 backfill（ledger 自身只有最近 5000 条）；调用方报告
 * 必须标注 backfill source = bounded Usage Ledger。
 */
export function backfillModelCallUsageFromLedgerEntries(
  projection: ModelObservabilityAccountingProjection,
  entries: unknown[],
  db: { prepare: (sql: string) => any },
  options: { now?: () => string } = {},
): { projected: number; skipped: number } {
  let projected = 0;
  let skipped = 0;
  for (const entry of entries) {
    if (projection.upsertLedgerEntry(entry as UsageLedgerEntryLike, options)) projected += 1;
    else skipped += 1;
  }
  db.prepare(
    `INSERT INTO observability_meta (key, value_json) VALUES (?, ?)
     ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json`,
  ).run(
    MODEL_OBSERVABILITY_USAGE_BACKFILL_META_KEY,
    JSON.stringify(options.now?.() ?? new Date().toISOString()),
  );
  return { projected, skipped };
}
