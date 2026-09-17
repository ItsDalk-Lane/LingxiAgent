/**
 * model-observability-data-management.ts — 「设置」子页后端：存储概况 + 手动删除
 * （2026-09-17 用户定稿：全部 / 按天 × 轨迹与元数据 / Payload 正文 / 媒体 Blob）。
 *
 * 契约：
 *   - 删除以**完整 trace** 为单位（retention §五十六：绝不删碎 trace 树）。
 *     「按天」= traces.last_seen_at 落在该日历日的整棵 trace。
 *   - 选「轨迹与元数据」会连带该 trace 的 payload 与 blob 引用（完整性，
 *     §五十六/七十一）；「Payload 正文」单选只删正文并把 call 标 expired。
 *   - 「媒体 Blob」只删除所选范围内**不再被引用**的 blob 文件；仍被正文引用
 *     的 blob 留给该类删除或 GC（§九十一），绝不制造 dangling ref。
 *   - 「全部」+ 轨迹删除后做一次 compact（昂贵操作只在用户明确清空时执行）。
 *   - 绝不返回绝对路径（§一百二十八）；体积是真实文件/元数据字节数。
 */
import fs from "node:fs";
import type { ModelObservabilityBlobStore } from "./model-observability-blob-store.ts";
import { compactModelObservabilityDatabase } from "./model-observability-schema.ts";

export type ModelObservabilityStorageDay = {
  date: string;
  calls: number;
  mediaBytes: number;
  payloadChars: number;
};

export type ModelObservabilityStorageOverview = {
  days: number;
  oldestAt: string | null;
  newestAt: string | null;
  calls: number;
  sizes: {
    databaseBytes: number;
    mediaBytes: number;
    /** payload 正文体量估算（按 record_char_count 字符数）。 */
    payloadEstimateChars: number;
  };
  perDay: ModelObservabilityStorageDay[];
};

function hasTable(db: any, table: string): boolean {
  return db.prepare(
    `SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?`,
  ).get(table) != null;
}

function fileSizeBytes(path: string): number {
  try {
    return fs.statSync(path).size;
  } catch {
    return 0;
  }
}

export function computeModelObservabilityStorage({ db, dbPath }: {
  db: any;
  dbPath: string;
}): ModelObservabilityStorageOverview {
  const dayRows = db.prepare(
    `SELECT substr(started_at, 1, 10) AS date, COUNT(*) AS calls,
            MIN(started_at) AS oldest, MAX(started_at) AS newest
       FROM model_calls WHERE started_at IS NOT NULL
       GROUP BY date ORDER BY date ASC`,
  ).all();
  const payloadChars = db.prepare(
    `SELECT COALESCE(SUM(record_char_count), 0) AS chars FROM payload_records`,
  ).get()?.chars ?? 0;
  const mediaBytes = db.prepare(
    `SELECT COALESCE(SUM(byte_length), 0) AS bytes FROM blob_objects WHERE state = 'ready'`,
  ).get()?.bytes ?? 0;
  const mediaByDay = new Map<string, number>(db.prepare(
    `SELECT substr(created_at, 1, 10) AS date, COALESCE(SUM(byte_length), 0) AS bytes
       FROM blob_objects WHERE state = 'ready' GROUP BY date`,
  ).all().map((row: any) => [row.date, row.bytes]));
  const charsByDay = new Map<string, number>(db.prepare(
    `SELECT substr(captured_at, 1, 10) AS date, COALESCE(SUM(record_char_count), 0) AS chars
       FROM payload_records GROUP BY date`,
  ).all().map((row: any) => [row.date, row.chars]));

  const databaseBytes = ["", "-wal", "-shm"].reduce(
    (sum, suffix) => sum + fileSizeBytes(dbPath + suffix), 0,
  );
  const perDay: ModelObservabilityStorageDay[] = dayRows.map((row: any) => ({
    date: row.date,
    calls: row.calls,
    mediaBytes: mediaByDay.get(row.date) ?? 0,
    payloadChars: charsByDay.get(row.date) ?? 0,
  }));
  const calls = dayRows.reduce((sum: number, row: any) => sum + row.calls, 0);
  return {
    days: dayRows.length,
    oldestAt: dayRows[0]?.oldest ?? null,
    newestAt: dayRows[dayRows.length - 1]?.newest ?? null,
    calls,
    sizes: { databaseBytes, mediaBytes, payloadEstimateChars: payloadChars },
    perDay,
  };
}

/* ── 手动删除 ───────────────────────────────────────────────────────── */

export type ModelObservabilityDeleteCategory = 'trace' | 'payload' | 'media';
/** 双闭日期窗口（本地日历日）；from = to 即单日。空 windows = 全部数据。 */
export type ModelObservabilityDeleteWindow = { from: string; to: string };
export type ModelObservabilityDeleteInput = {
  categories: ModelObservabilityDeleteCategory[];
  windows: ModelObservabilityDeleteWindow[];
};

export type ModelObservabilityDeleteStats = {
  deletedTraces: number;
  deletedPayloadRecords: number;
  deletedBlobFiles: number;
  compacted: boolean;
  ranAt: string;
};

export type ModelObservabilityDataManagementContext = {
  db: any;
  blobStore: Pick<ModelObservabilityBlobStore, "deleteBlobs">;
  markPayloadAvailability: (callIds: string[], availability: "expired") => void;
  dbPath: string;
  now?: () => string;
};

function chunkIds(ids: string[]): string[][] {
  const chunks: string[][] = [];
  for (let i = 0; i < ids.length; i += 500) chunks.push(ids.slice(i, i + 500));
  return chunks;
}

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

export function validateModelObservabilityDeleteInput(
  input: unknown,
): { ok: true; value: ModelObservabilityDeleteInput } | { ok: false; message: string; field?: string } {
  const source = input && typeof input === "object" && !Array.isArray(input)
    ? input as Record<string, unknown> : null;
  if (!source) return { ok: false, message: "request body must be a JSON object" };
  const categories = source.categories;
  if (!Array.isArray(categories) || categories.length === 0
    || !categories.every((c) => c === "trace" || c === "payload" || c === "media")) {
    return { ok: false, message: "categories must be a non-empty array of trace/payload/media", field: "categories" };
  }
  if (source.windows !== undefined && !Array.isArray(source.windows)) {
    return { ok: false, message: "windows must be an array of {from, to}", field: "windows" };
  }
  const windows: ModelObservabilityDeleteWindow[] = [];
  for (const raw of Array.isArray(source.windows) ? source.windows : []) {
    const entry = raw && typeof raw === "object" && !Array.isArray(raw)
      ? raw as Record<string, unknown> : null;
    const from = typeof entry?.from === "string" ? entry.from : "";
    const to = typeof entry?.to === "string" ? entry.to : "";
    if (!DATE_RE.test(from) || !DATE_RE.test(to)) {
      return { ok: false, message: "window from/to must be YYYY-MM-DD", field: "windows" };
    }
    if (from > to) {
      return { ok: false, message: "window from must be <= to", field: "windows" };
    }
    windows.push({ from, to });
    if (windows.length > 366) {
      return { ok: false, message: "too many windows (max 366)", field: "windows" };
    }
  }
  return { ok: true, value: { categories: categories as ModelObservabilityDeleteCategory[], windows } };
}

/**
 * 手动删除。范围以 trace 为粒度；「轨迹与元数据」连带正文与 blob 引用
 * （完整性）；「Payload 正文」单选只删正文；「媒体 Blob」只删范围内不再被
 * 引用的 blob 文件。
 */
export function deleteModelObservabilityData(
  ctx: ModelObservabilityDataManagementContext,
  input: ModelObservabilityDeleteInput,
): ModelObservabilityDeleteStats {
  const { db, blobStore } = ctx;
  const wantsTrace = input.categories.includes("trace");
  // 轨迹删除天然连带正文（删除内含 payload 清理），不重复计。
  const wantsPayloadOnly = input.categories.includes("payload") && !wantsTrace;
  const wantsMedia = input.categories.includes("media");
  const deleteAll = input.windows.length === 0;

  const traceIdSet = new Set<string>();
  if (deleteAll) {
    for (const row of db.prepare(`SELECT trace_id FROM traces`).all()) traceIdSet.add(row.trace_id);
  } else {
    for (const window of input.windows) {
      for (const row of db.prepare(
        `SELECT trace_id FROM traces WHERE substr(last_seen_at, 1, 10) BETWEEN ? AND ?`,
      ).all(window.from, window.to)) {
        traceIdSet.add(row.trace_id);
      }
    }
  }
  const traceIds = [...traceIdSet];

  const stats: ModelObservabilityDeleteStats = {
    deletedTraces: 0,
    deletedPayloadRecords: 0,
    deletedBlobFiles: 0,
    compacted: false,
    ranAt: ctx.now?.() ?? new Date().toISOString(),
  };

  if ((wantsTrace || wantsPayloadOnly) && traceIds.length > 0) {
    db.transaction(() => {
      for (const chunk of chunkIds(traceIds)) {
        const placeholders = chunk.map(() => "?").join(",");
        if (wantsTrace) {
          if (hasTable(db, "model_call_usage")) {
            db.prepare(
              `DELETE FROM model_call_usage WHERE model_call_id IN (
                 SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
               )`,
            ).run(...chunk);
          }
          db.prepare(
            `DELETE FROM payload_blob_refs WHERE payload_record_id IN (
               SELECT id FROM payload_records WHERE call_id IN (
                 SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
               )
             )`,
          ).run(...chunk);
          db.prepare(
            `DELETE FROM payload_records WHERE call_id IN (
               SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
             )`,
          ).run(...chunk);
          db.prepare(
            `DELETE FROM model_attempts WHERE call_id IN (
               SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})
             )`,
          ).run(...chunk);
          db.prepare(`DELETE FROM model_calls WHERE trace_id IN (${placeholders})`).run(...chunk);
          db.prepare(`DELETE FROM traces WHERE trace_id IN (${placeholders})`).run(...chunk);
          stats.deletedTraces += chunk.length;
        } else {
          const callIds = db.prepare(
            `SELECT call_id FROM model_calls WHERE trace_id IN (${placeholders})`,
          ).all(...chunk).map((row: any) => row.call_id);
          ctx.markPayloadAvailability(callIds, "expired");
          for (const part of chunkIds(callIds)) {
            const ph = part.map(() => "?").join(",");
            db.prepare(
              `DELETE FROM payload_blob_refs WHERE payload_record_id IN (
                 SELECT id FROM payload_records WHERE call_id IN (${ph})
               )`,
            ).run(...part);
            const info = db.prepare(
              `DELETE FROM payload_records WHERE call_id IN (${ph})`,
            ).run(...part);
            stats.deletedPayloadRecords += info.changes;
          }
        }
      }
    })();
  }

  if (wantsMedia) {
    const blobIdSet = new Set<string>();
    for (const window of deleteAll
      ? [{ from: "0000-01-01", to: "9999-12-31" }]
      : input.windows) {
      for (const row of db.prepare(
        `SELECT blob_id FROM blob_objects WHERE substr(created_at, 1, 10) BETWEEN ? AND ?
           AND NOT EXISTS (
             SELECT 1 FROM payload_blob_refs r WHERE r.blob_id = blob_objects.blob_id
           )`,
      ).all(window.from, window.to)) {
        blobIdSet.add(row.blob_id);
      }
    }
    stats.deletedBlobFiles = blobStore.deleteBlobs([...blobIdSet]);
  }

  if (deleteAll && wantsTrace) {
    compactModelObservabilityDatabase(db);
    stats.compacted = true;
  }
  return stats;
}
