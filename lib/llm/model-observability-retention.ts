/**
 * model-observability-retention.ts — Retention / GC contract（Phase 7）。
 *
 * 分离的 retention 维度（§五十三）：正文比 metadata 敏感也更占空间，Store
 * Contract 必须允许 Trace retention > Payload retention——正文过期后仍能知道
 * 「某天发生过一次调用、用了哪个模型、状态如何」，但 Prompt 已删除。
 *
 * 删除单位（§五十六）：Payload 按**完整 trace** 成组删除、Trace metadata 按
 * 完整 trace 删除——绝不随机删掉某个 trace 中间的一条 Call 导致树断裂。
 *
 * 本轮不拍板产品默认值（§五十四）：最终 UI 默认值留给 Phase 8；但显式开启
 * persistence 而未给 policy 时必须落到**集中定义、明确记录、可测试**的 safe
 * fallback（§五十五），不散落魔法数字。
 *
 * Usage Ledger 不受影响（§五十九）：独立 accounting projection，不在此触碰。
 */

import type { ModelObservabilityBlobStore } from "./model-observability-blob-store.ts";
import { compactModelObservabilityDatabase } from "./model-observability-schema.ts";

export type ModelObservabilityRetentionPolicy = {
  /** trace metadata（traces/model_calls/model_attempts）最大保留时长。 */
  traceMaxAgeMs: number | null;
  /** payload 正文（payload_records）最大保留时长；可短于 traceMaxAgeMs。 */
  payloadMaxAgeMs: number | null;
  /**
   * blob 最大保留时长。只作用于 refless blob（§九十一）：被引用 blob 的寿命
   * 由其 payload retention 决定——删除仍被引用的 blob 会制造 dangling ref，
   * 违反 §七十一。
   */
  blobMaxAgeMs: number | null;
  maxTraceRows: number | null;
  maxPayloadBytes: number | null;
  maxBlobBytes: number | null;
};

const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * Safe fallback（§五十五）：persistence 开启但未配置 retention 时使用。
 * 集中定义于此；改动需同步 OBSERVABILITY_STORAGE_PROGRESS.md 与审计文档。
 */
export const SAFE_FALLBACK_MODEL_OBSERVABILITY_RETENTION: ModelObservabilityRetentionPolicy = {
  traceMaxAgeMs: 180 * DAY_MS,
  payloadMaxAgeMs: 30 * DAY_MS,
  blobMaxAgeMs: 30 * DAY_MS,
  maxTraceRows: null,
  maxPayloadBytes: null,
  maxBlobBytes: null,
};

export function normalizeModelObservabilityRetentionPolicy(
  input: unknown,
): ModelObservabilityRetentionPolicy {
  const source = input && typeof input === "object" ? input as Record<string, unknown> : {};
  const intOrNull = (value: unknown): number | null =>
    typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.floor(value) : null;
  const fallback = SAFE_FALLBACK_MODEL_OBSERVABILITY_RETENTION;
  return {
    traceMaxAgeMs: intOrNull(source.traceMaxAgeMs) ?? fallback.traceMaxAgeMs,
    payloadMaxAgeMs: intOrNull(source.payloadMaxAgeMs) ?? fallback.payloadMaxAgeMs,
    blobMaxAgeMs: intOrNull(source.blobMaxAgeMs) ?? fallback.blobMaxAgeMs,
    maxTraceRows: intOrNull(source.maxTraceRows),
    maxPayloadBytes: intOrNull(source.maxPayloadBytes),
    maxBlobBytes: intOrNull(source.maxBlobBytes),
  };
}

export type ModelObservabilityMaintenanceStats = {
  payloadExpiredTraces: number;
  payloadExpiredCalls: number;
  prunedTraces: number;
  gcBlobs: number;
  orphanBlobFiles: number;
  missingBlobs: number;
  blobBytesCapped: number;
  compacted: boolean;
  ranAt: string;
};

type MaintenanceContext = {
  db: any;
  blobStore: ModelObservabilityBlobStore;
  markPayloadAvailability(callIds: string[], availability: "expired"): void;
  now?: () => string;
};

function collectTraceCallIds(db: any, traceIds: string[]): string[] {
  const out: string[] = [];
  for (const chunk of chunkIds(traceIds)) {
    const placeholders = chunk.map(() => "?").join(",");
    out.push(...db.prepare(
      `SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})`,
    ).all(...chunk).map((row: any) => row.call_id));
  }
  return out;
}

/** SQLite 绑定变量数有上限（旧版 999）：按 500 一批执行 IN 删除。 */
function chunkIds(ids: string[]): string[][] {
  const chunks: string[][] = [];
  for (let i = 0; i < ids.length; i += 500) chunks.push(ids.slice(i, i + 500));
  return chunks;
}

/** v1 历史库没有 model_call_usage；maintenance 必须能在 v1 上安全运行。 */
function hasTable(db: any, table: string): boolean {
  const row = db.prepare(
    `SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?`,
  ).get(table);
  return row != null;
}

/** 删除一组 trace 的全部 payload_records + refs，并把 call 标记 expired（§五十七）。 */
function deletePayloadsForTraces(ctx: MaintenanceContext, traceIds: string[]): { calls: number } {
  if (traceIds.length === 0) return { calls: 0 };
  const callIds = collectTraceCallIds(ctx.db, traceIds);
  ctx.markPayloadAvailability(callIds, "expired");
  for (const chunk of chunkIds(traceIds)) {
    const placeholders = chunk.map(() => "?").join(",");
    ctx.db.prepare(
      `DELETE FROM payload_blob_refs WHERE payload_record_id IN (
         SELECT id FROM payload_records WHERE call_id IN (
           SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
         )
       )`,
    ).run(...chunk);
    ctx.db.prepare(
      `DELETE FROM payload_records WHERE call_id IN (
         SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
       )`,
    ).run(...chunk);
  }
  return { calls: callIds.length };
}

/** 删除完整 trace（usage projection → payload refs → payloads → attempts → calls → trace row）。 */
function deleteTraces(ctx: MaintenanceContext, traceIds: string[]): void {
  if (traceIds.length === 0) return;
  for (const chunk of chunkIds(traceIds)) {
    const placeholders = chunk.map(() => "?").join(",");
    // Phase 8 §十六：usage projection 随对应 trace 删除，不产生 orphan
    // （projection 引用 model_call_id；trace 删除后该 call 不再可查）。
    if (hasTable(ctx.db, "model_call_usage")) {
      ctx.db.prepare(
        `DELETE FROM model_call_usage WHERE model_call_id IN (
           SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
         )`,
      ).run(...chunk);
    }
    ctx.db.prepare(
      `DELETE FROM payload_blob_refs WHERE payload_record_id IN (
         SELECT id FROM payload_records WHERE call_id IN (
           SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
         )
       )`,
    ).run(...chunk);
    ctx.db.prepare(
      `DELETE FROM payload_records WHERE call_id IN (
         SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
       )`,
    ).run(...chunk);
    ctx.db.prepare(
      `DELETE FROM model_attempts WHERE call_id IN (
         SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
       )`,
    ).run(...chunk);
    ctx.db.prepare(`DELETE FROM model_calls WHERE trace_id IN (${placeholders})`).run(...chunk);
    ctx.db.prepare(`DELETE FROM traces WHERE trace_id IN (${placeholders})`).run(...chunk);
  }
}

/**
 * 执行一次 retention + GC maintenance（startup once / 低频 timer / 显式触发；
 * 绝不在模型调用路径执行，§八十六）。
 *
 * 顺序：payload expiry → payload byte cap → trace expiry → trace row cap →
 * blob GC（refless + blobMaxAge + maxBlobBytes）→ orphan/missing recovery →
 * compact。每阶段独立 transaction：一段失败不影响已完成段（调用方捕获计数）。
 */
export function runModelObservabilityMaintenance(
  ctx: MaintenanceContext,
  options: { policy: ModelObservabilityRetentionPolicy },
): ModelObservabilityMaintenanceStats {
  const { db, blobStore } = ctx;
  const now = ctx.now?.() ?? new Date().toISOString();
  const stats: ModelObservabilityMaintenanceStats = {
    payloadExpiredTraces: 0,
    payloadExpiredCalls: 0,
    prunedTraces: 0,
    gcBlobs: 0,
    orphanBlobFiles: 0,
    missingBlobs: 0,
    blobBytesCapped: 0,
    compacted: false,
    ranAt: now,
  };
  const policy = options.policy;

  // ① payload expiry（payloadMaxAgeMs < traceMaxAgeMs 时正文先过期，§五十七）。
  if (policy.payloadMaxAgeMs !== null) {
    const cutoff = new Date(Date.now() - policy.payloadMaxAgeMs).toISOString();
    const expiredTraces: string[] = db.prepare(
      `SELECT trace_id FROM traces WHERE last_seen_at < ?`,
    ).all(cutoff).map((row: any) => row.trace_id);
    stats.payloadExpiredTraces = expiredTraces.length;
    if (expiredTraces.length > 0) {
      db.transaction(() => {
        const result = deletePayloadsForTraces(ctx, expiredTraces);
        stats.payloadExpiredCalls = result.calls;
      })();
    }
  }

  // ② payload byte cap：按 trace 由旧到新删 payload 直到达标（完整 trace 单位）。
  if (policy.maxPayloadBytes !== null) {
    db.transaction(() => {
      let totalBytes = db.prepare(`SELECT COALESCE(SUM(record_char_count), 0) AS bytes FROM payload_records`).get().bytes;
      if (totalBytes <= policy.maxPayloadBytes!) return;
      const oldest: Array<{ trace_id: string }> = db.prepare(
        `SELECT trace_id FROM traces ORDER BY last_seen_at ASC`,
      ).all();
      for (const row of oldest) {
        if (totalBytes <= policy.maxPayloadBytes!) break;
        const before = totalBytes;
        const result = deletePayloadsForTraces(ctx, [row.trace_id]);
        stats.payloadExpiredTraces += 1;
        stats.payloadExpiredCalls += result.calls;
        const after = db.prepare(`SELECT COALESCE(SUM(record_char_count), 0) AS bytes FROM payload_records`).get().bytes;
        totalBytes = after;
        if (after >= before && before > policy.maxPayloadBytes!) {
          // 该 trace 无 payload：继续下一个 oldest（上面 break 条件最终会终止）。
        }
      }
    })();
  }

  // ③ trace expiry：完整 trace 删除（payload 早已过期或同批带走）。
  if (policy.traceMaxAgeMs !== null) {
    const cutoff = new Date(Date.now() - policy.traceMaxAgeMs).toISOString();
    const expiredTraces: string[] = db.prepare(
      `SELECT trace_id FROM traces WHERE last_seen_at < ?`,
    ).all(cutoff).map((row: any) => row.trace_id);
    stats.prunedTraces = expiredTraces.length;
    if (expiredTraces.length > 0) {
      db.transaction(() => {
        deleteTraces(ctx, expiredTraces);
      })();
    }
  }

  // ④ trace row cap：最旧 trace 整树删除直到达标。
  if (policy.maxTraceRows !== null) {
    db.transaction(() => {
      let traceCount = db.prepare(`SELECT COUNT(*) AS n FROM traces`).get().n;
      while (traceCount > policy.maxTraceRows!) {
        const oldest = db.prepare(`SELECT trace_id FROM traces ORDER BY last_seen_at ASC LIMIT 1`).get();
        if (!oldest) break;
        deleteTraces(ctx, [oldest.trace_id]);
        stats.prunedTraces += 1;
        const next = db.prepare(`SELECT COUNT(*) AS n FROM traces`).get().n;
        if (next >= traceCount) break;
        traceCount = next;
      }
    })();
  }

  // ⑤ orphan usage projection 清理（§十六）：trace/call 已删除（或 trace 事件
  // 整批丢失）后遗留的 model_call_usage 不允许永久 orphan。
  if (hasTable(db, "model_call_usage")) {
    db.transaction(() => {
      db.prepare(
        `DELETE FROM model_call_usage WHERE model_call_id NOT IN (SELECT call_id FROM model_calls)`,
      ).run();
    })();
  }

  // ⑥ blob GC：先清 refless（立即，§九十一），再按 blobMaxAge/maxBlobBytes 约束。
  const refless = blobStore.collectGarbageBlobs();
  stats.gcBlobs += refless.length;
  if (policy.blobMaxAgeMs !== null) {
    const cutoff = new Date(Date.now() - policy.blobMaxAgeMs).toISOString();
    const aged = db.prepare(
      `SELECT blob_id FROM blob_objects WHERE created_at < ? AND NOT EXISTS (
         SELECT 1 FROM payload_blob_refs r WHERE r.blob_id = blob_objects.blob_id
       )`,
    ).all(cutoff).map((row: any) => row.blob_id);
    stats.gcBlobs += blobStore.deleteBlobs(aged);
  }
  if (policy.maxBlobBytes !== null) {
    let total = db.prepare(
      `SELECT COALESCE(SUM(byte_length), 0) AS bytes FROM blob_objects WHERE state = 'ready'`,
    ).get().bytes;
    if (total > policy.maxBlobBytes!) {
      const candidates = db.prepare(
        `SELECT blob_id, byte_length FROM blob_objects
         WHERE NOT EXISTS (SELECT 1 FROM payload_blob_refs r WHERE r.blob_id = blob_objects.blob_id)
         ORDER BY created_at ASC`,
      ).all();
      let capped = 0;
      for (const candidate of candidates) {
        if (total <= policy.maxBlobBytes!) break;
        blobStore.deleteBlobs([candidate.blob_id]);
        total -= candidate.byte_length;
        capped += 1;
      }
      stats.blobBytesCapped = capped;
    }
  }

  // ⑦ orphan / missing recovery（§九十二/九十三）。
  stats.orphanBlobFiles = blobStore.recoverOrphanBlobFiles();
  stats.missingBlobs = blobStore.recoverMissingBlobs();

  // ⑧ 文件收缩（§八十八：不在模型热路径执行）。
  compactModelObservabilityDatabase(db);
  stats.compacted = true;
  return stats;
}
