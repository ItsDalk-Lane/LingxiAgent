/**
 * model-observability-persistence.ts — Durable Persistence Coordinator（Phase 7）。
 *
 * 唯一职责：把 Phase 1～6 的运行时事实安全投影为跨进程重启可恢复的 durable
 * Model Observatory（traces / model_calls / model_attempts / payload_records /
 * blob_objects / payload_blob_refs），同时保证：
 *
 *   Storage 永远是 Observer/Capture 的消费者，而不是模型执行依赖（任务书 §二）。
 *
 * 关键不变量：
 *   - handler 只 enqueue（§三十五）：handleModelCallEvent /
 *     handleModelCallPayloadRecord 内严禁同步 SQLite/文件写；模型热路径不等待
 *     commit（§三十四）。
 *   - 三个 bounded queue（trace / payload / blob）+ 显式 drop 计数（§三十九/四十）；
 *     Trace metadata 优先级高于 Payload（§四十一）——一个 1MB response 拖不死
 *     Trace queue。
 *   - flush = blob 文件先写（§七十二）→ 单 transaction 提交 blob metadata +
 *     trace 投影 + payload records + refs + health meta（§三十七 batch）。
 *   - transaction throw → 整批 rollback 后才允许 retry（§四十九）；单次重试仍
 *     失败 → drop batch + 计数（防 poison batch livelock，诚实缺失）。
 *   - graceful shutdown：flush + close + uninstall，恢复先前 observer/sink/
 *     externalizer（§四十五/八十五）；测试间全局状态不泄漏。
 *   - 默认 policy = disabled：生产行为与 Phase 6 完全一致（§七十九）；开启
 *     必须经显式 engine/runtime option（§八十/八十一）。
 *   - 打开失败（损坏/未知高版本/迁移失败）→ disabled handle + reasonCode，
 *     主程序正常继续（§二十七～二十九）。
 *
 * Crash durability 诚实语义（§四十四）：接受进程异常崩溃时最后一个尚未 flush
 * 的 batch 可能丢失；不要求 logical_call_start 同步落盘后才发送 Provider 请求。
 */

import fs from "fs";
import path from "path";
import type { ModelCallEvent, ModelCallObserver } from "./model-call-observer.ts";
import { getModelCallObserver, setModelCallObserver } from "./model-call-observer.ts";
import type { ModelCallPayloadRecord } from "./model-call-payload-types.ts";
import type { ModelCallPayloadSink } from "./model-call-payload-capture.ts";
import {
  getModelCallBlobExternalizer,
  getModelCallPayloadSink,
  setModelCallBlobExternalizer,
  setModelCallPayloadSink,
} from "./model-call-payload-capture.ts";
import { runWithoutModelTrace } from "./model-trace-scope.ts";
import {
  MODEL_OBSERVABILITY_SCHEMA_VERSION,
  ModelObservabilitySchemaError,
  modelObservabilityDbPath,
  openModelObservabilityDatabase,
} from "./model-observability-schema.ts";
import { createModelObservabilityTraceStore } from "./model-observability-trace-store.ts";
import { createModelObservabilityPayloadStore } from "./model-observability-payload-store.ts";
import {
  backfillModelCallUsageFromLedgerEntries,
  createModelObservabilityAccountingProjection,
  MODEL_OBSERVABILITY_USAGE_BACKFILL_META_KEY,
  type UsageLedgerEntryLike,
} from "./model-observability-accounting-projection.ts";
import {
  createModelObservabilityBlobStore,
  mintModelObservabilityBlobId,
} from "./model-observability-blob-store.ts";
import {
  normalizeModelObservabilityRetentionPolicy,
  runModelObservabilityMaintenance,
  type ModelObservabilityMaintenanceStats,
  type ModelObservabilityRetentionPolicy,
} from "./model-observability-retention.ts";
import { ensureSecretDirModeSync, ensureSecretFileModeSync } from "../../shared/secret-fs.ts";

/* ── Policy contract（§七十九～八十三）────────────────────────────────── */

export type ModelObservabilityPersistenceLimits = {
  maxQueuedTraceEvents: number;
  maxQueuedPayloadRecords: number;
  maxQueuedBlobs: number;
  /** 已 staged 未落盘 blob 的字节预算（§七十三）。 */
  maxPendingBlobBytes: number;
};

export const DEFAULT_MODEL_OBSERVABILITY_PERSISTENCE_LIMITS: ModelObservabilityPersistenceLimits = {
  maxQueuedTraceEvents: 4096,
  maxQueuedPayloadRecords: 2048,
  maxQueuedBlobs: 256,
  maxPendingBlobBytes: 64 * 1024 * 1024,
};

export type ModelObservabilityPersistencePolicy = {
  enabled: boolean;
  /** trace/call/attempt metadata 持久化（§八十二；enabled 时默认 true）。 */
  persistTraceMetadata?: boolean;
  /** sanitized payload 正文持久化（默认 false：不默认永久记录 Prompt）。 */
  persistPayloads?: boolean;
  /** raw binary blob 持久化（默认 false；persistPayloads=false 时强制 false，§八十三）。 */
  persistBlobs?: boolean;
  retention?: unknown;
  limits?: Partial<ModelObservabilityPersistenceLimits>;
  flushIntervalMs?: number;
  maintenanceIntervalMs?: number;
};

export const DISABLED_MODEL_OBSERVABILITY_PERSISTENCE_POLICY: ModelObservabilityPersistencePolicy = {
  enabled: false,
};

export function normalizeModelObservabilityPersistencePolicy(
  input: unknown,
): Required<Pick<ModelObservabilityPersistencePolicy, "enabled" | "persistTraceMetadata" | "persistPayloads" | "persistBlobs">> & {
  retention: ModelObservabilityRetentionPolicy;
  limits: ModelObservabilityPersistenceLimits;
  flushIntervalMs: number;
  maintenanceIntervalMs: number;
} {
  const source = input && typeof input === "object" ? input as Record<string, unknown> : {};
  const enabled = source.enabled === true;
  const persistPayloads = enabled && source.persistPayloads === true;
  const persistTraceMetadata = enabled ? source.persistTraceMetadata !== false : false;
  return {
    enabled,
    persistTraceMetadata,
    persistPayloads,
    persistBlobs: enabled && persistPayloads && source.persistBlobs === true,
    retention: normalizeModelObservabilityRetentionPolicy(source.retention),
    limits: { ...DEFAULT_MODEL_OBSERVABILITY_PERSISTENCE_LIMITS, ...(source.limits as Partial<ModelObservabilityPersistenceLimits> ?? {}) },
    flushIntervalMs: typeof source.flushIntervalMs === "number" && source.flushIntervalMs >= 50
      ? Math.floor(source.flushIntervalMs)
      : 2000,
    maintenanceIntervalMs: typeof source.maintenanceIntervalMs === "number" && source.maintenanceIntervalMs >= 1000
      ? Math.floor(source.maintenanceIntervalMs)
      : 60 * 60 * 1000,
  };
}

/* ── Health state（§四十二：绝不包含正文）─────────────────────────────── */

export type ModelObservabilityHealth = {
  status: "active" | "degraded" | "disabled" | "closed";
  storeDisabledReasonCode: string | null;
  persistTraceMetadata: boolean;
  persistPayloads: boolean;
  persistBlobs: boolean;
  queuedTraceEvents: number;
  queuedPayloadRecords: number;
  queuedBlobs: number;
  /** Phase 8：待投影的 llm_usage entries（accounting projection 队列）。 */
  queuedUsageEntries: number;
  pendingBlobBytes: number;
  droppedTraceEvents: number;
  droppedPayloadRecords: number;
  droppedBlobs: number;
  /** Phase 8：accounting projection 队列溢出/写入失败计数。 */
  droppedUsageEntries: number;
  writeFailures: number;
  maintenanceErrors: number;
  lastFlushAt: string | null;
  lastSuccessfulFlushAt: string | null;
  lastMaintenanceAt: string | null;
  schemaVersion: number | null;
  /** 本次打开时 Startup Reconciliation 标记的崩溃遗留 call 数（§四十六）。 */
  interruptedByRestartCalls: number;
};

export type ModelObservabilityPersistenceHandle = {
  readonly policy: ReturnType<typeof normalizeModelObservabilityPersistencePolicy>;
  readonly observer: ModelCallObserver | null;
  readonly sink: ModelCallPayloadSink | null;
  getHealth(): ModelObservabilityHealth;
  /** 立即 flush 队列（测试/显式触发；模型热路径永远不调用它等待）。 */
  flushSync(): void;
  runMaintenance(): ModelObservabilityMaintenanceStats | null;
  /**
   * Phase 8（§十四）：接入 Usage Ledger → llm_usage 事件流。幂等（重复调用
   * 先退订旧 consumer）。包含 bounded ledger best-effort backfill（§十五，
   * 只做一次，meta key 标记）。返回 backfill 报告（disabled 时 null）。
   */
  initializeAccounting(options: {
    listLedgerEntries: () => unknown[];
    subscribeUsage: (consumer: (entry: unknown) => void) => () => void;
  }): { backfilled: number; skipped: number; backfillSource: "bounded_usage_ledger" } | null;
  /** flush（bounded）+ 停 timer + close DB + uninstall（恢复先前注册对象）。幂等。 */
  close(): Promise<void>;
  /** 只恢复先前 observer/sink/externalizer，不关 DB（close 会一并做）。 */
  uninstall(): void;
};

/* ── 内部队列 item ───────────────────────────────────────────────────── */

type StagedBlob = { blobId: string; bytes: Uint8Array; mediaType: string | null };
type PreparedBlob = {
  blobId: string;
  byteLength: number;
  mediaType: string | null;
  durable: boolean;
};
type TransactionHealthDelta = {
  droppedPayloadRecords: number;
  droppedUsageEntries: number;
};

const EMPTY_TRANSACTION_HEALTH_DELTA: TransactionHealthDelta = {
  droppedPayloadRecords: 0,
  droppedUsageEntries: 0,
};

function nowIso(): string {
  return new Date().toISOString();
}

function createDisabledHandle(
  reasonCode: string,
  policy: ReturnType<typeof normalizeModelObservabilityPersistencePolicy>,
): ModelObservabilityPersistenceHandle {
  return {
    policy,
    observer: null,
    sink: null,
    getHealth() {
      return {
        status: "disabled",
        storeDisabledReasonCode: reasonCode,
        persistTraceMetadata: false,
        persistPayloads: false,
        persistBlobs: false,
        queuedTraceEvents: 0,
        queuedPayloadRecords: 0,
        queuedBlobs: 0,
        queuedUsageEntries: 0,
        pendingBlobBytes: 0,
        droppedTraceEvents: 0,
        droppedPayloadRecords: 0,
        droppedBlobs: 0,
        droppedUsageEntries: 0,
        writeFailures: 0,
        maintenanceErrors: 0,
        lastFlushAt: null,
        lastSuccessfulFlushAt: null,
        lastMaintenanceAt: null,
        schemaVersion: null,
        interruptedByRestartCalls: 0,
      };
    },
    flushSync() { /* disabled：快路径 no-op */ },
    runMaintenance() { return null; },
    initializeAccounting() { return null; },
    async close() { /* disabled：无资源 */ },
    uninstall() { /* 无安装 */ },
  };
}

/**
 * 安装 durable observability persistence（真实生产 wiring，§八十）。
 *
 * 默认（enabled=false / policy 缺省）→ disabled handle：不打开任何文件，
 * 生产行为与 Phase 6 完全一致。打开失败 → disabled handle（主程序不受影响）。
 */
export function installModelObservabilityPersistence({
  lingxiHome,
  policy,
  now = nowIso,
  Database = null,
  randomBlobToken = null,
  reconcileAfterRestart = true,
}: {
  lingxiHome: string;
  policy?: ModelObservabilityPersistencePolicy | null;
  now?: () => string;
  Database?: any;
  randomBlobToken?: (() => string) | null;
  /** 同一进程内 generation 切换不是 restart，调用方应传 false。 */
  reconcileAfterRestart?: boolean;
}): ModelObservabilityPersistenceHandle {
  const normalized = normalizeModelObservabilityPersistencePolicy(policy ?? DISABLED_MODEL_OBSERVABILITY_PERSISTENCE_POLICY);
  if (!normalized.enabled) return createDisabledHandle("disabled_by_policy", normalized);
  if (typeof lingxiHome !== "string" || !lingxiHome.trim()) {
    return createDisabledHandle("open_failed", normalized);
  }

  let db: any;
  try {
    db = openModelObservabilityDatabase(
      modelObservabilityDbPath(lingxiHome),
      Database ? { Database } : {},
    );
  } catch (error) {
    const reason = error instanceof ModelObservabilitySchemaError
      ? error.reasonCode
      : "open_failed";
    return createDisabledHandle(reason, normalized);
  }

  // 私有目录/文件权限（§七十七/七十八：目录先收紧；-wal/-shm 同样 0600）。
  try {
    const storeDir = path.dirname(modelObservabilityDbPath(lingxiHome));
    fs.mkdirSync(storeDir, { recursive: true });
    if (process.platform !== "win32") {
      ensureSecretDirModeSync(storeDir);
    }
    for (const suffix of ["", "-wal", "-shm"]) {
      try {
        ensureSecretFileModeSync(`${modelObservabilityDbPath(lingxiHome)}${suffix}`);
      } catch { /* best-effort；Windows 语义见 secret-fs */ }
    }
  } catch { /* best-effort */ }

  const traceStore = createModelObservabilityTraceStore({ db, now });
  const payloadStore = createModelObservabilityPayloadStore({ db, traceStore });
  /** blob store 总是创建（maintenance 需要）；externalizer 只在 persistBlobs 时安装。 */
  const blobStore = createModelObservabilityBlobStore({ lingxiHome, db, now });
  if (normalized.persistBlobs) blobStore.ensurePrivateRoot();
  /** Phase 8：accounting projection writer（幂等 upsert，flush 事务内提交）。 */
  const accountingProjection = createModelObservabilityAccountingProjection({ db });

  // Startup Reconciliation（§四十六）：崩溃遗留 call 只标记 interrupted，不伪造终态。
  let interruptedCalls = 0;
  if (reconcileAfterRestart) {
    try {
      interruptedCalls = traceStore.reconcileAfterRestart();
    } catch { /* reconciliation 失败不阻止 store 可用 */ }
  }

  /* ── Coordinator 状态 ── */
  const traceQueue: ModelCallEvent[] = [];
  const payloadQueue: ModelCallPayloadRecord[] = [];
  const blobQueue: StagedBlob[] = [];
  /** Phase 8：待投影的 llm_usage entries（§十四 live ingestion 队列）。 */
  const usageQueue: UsageLedgerEntryLike[] = [];
  const MAX_QUEUED_USAGE_ENTRIES = Math.min(normalized.limits.maxQueuedPayloadRecords, 2048);
  /** queue overflow drop 的 payload callId（flush 时落 payload_availability='dropped'）。 */
  const droppedPayloadCallIds: string[] = [];
  /** §三十八：persistPayloads=false 时 call end 的 not_captured 标记队列。 */
  const notCapturedCallIds: string[] = [];
  const MAX_DROPPED_CALL_IDS = 512;
  const health: ModelObservabilityHealth = {
    status: "active",
    storeDisabledReasonCode: null,
    persistTraceMetadata: normalized.persistTraceMetadata,
    persistPayloads: normalized.persistPayloads,
    persistBlobs: normalized.persistBlobs,
    queuedTraceEvents: 0,
    queuedPayloadRecords: 0,
    queuedBlobs: 0,
    queuedUsageEntries: 0,
    pendingBlobBytes: 0,
    droppedTraceEvents: 0,
    droppedPayloadRecords: 0,
    droppedBlobs: 0,
    droppedUsageEntries: 0,
    writeFailures: 0,
    maintenanceErrors: 0,
    lastFlushAt: null,
    lastSuccessfulFlushAt: null,
    lastMaintenanceAt: null,
    schemaVersion: MODEL_OBSERVABILITY_SCHEMA_VERSION,
    interruptedByRestartCalls: interruptedCalls,
  };

  let closed = false;
  let flushing = false;
  let flushScheduled = false;
  let flushTimer: NodeJS.Timeout | null = null;
  let maintenanceTimer: NodeJS.Timeout | null = null;
  /**
   * 最终写失败后的内存回执。队列已经被取走时也必须让 timer/maintenance/close
   * 继续尝试把 drop/writeFailures 计数补进 DB。
   */
  let pendingHealthMetaDirty = false;

  const upsertMeta = db.prepare(
    `INSERT INTO observability_meta (key, value_json) VALUES (?, ?)
     ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json`,
  );

  /** 启动时恢复历史 drop/失败计数（§四十三：Phase 8 能诚实告知观测有缺失）。 */
  function restorePersistedCounters(): void {
    try {
      const read = db.prepare(`SELECT value_json FROM observability_meta WHERE key = ?`);
      const readCounter = (key: string): number => {
        try {
          const row = read.get(key);
          const value = row ? JSON.parse(row.value_json) : 0;
          return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.floor(value) : 0;
        } catch {
          return 0;
        }
      };
      health.droppedTraceEvents += readCounter("droppedTraceEvents");
      health.droppedPayloadRecords += readCounter("droppedPayloadRecords");
      health.droppedBlobs += readCounter("droppedBlobs");
      health.droppedUsageEntries += readCounter("droppedUsageEntries");
      health.writeFailures += readCounter("writeFailures");
    } catch { /* 计数恢复 best-effort */ }
  }
  restorePersistedCounters();

  function persistHealthMeta({
    delta = EMPTY_TRANSACTION_HEALTH_DELTA,
    successfulFlushAt = health.lastSuccessfulFlushAt,
  }: {
    delta?: TransactionHealthDelta;
    successfulFlushAt?: string | null;
  } = {}): void {
    upsertMeta.run("droppedTraceEvents", JSON.stringify(health.droppedTraceEvents));
    upsertMeta.run(
      "droppedPayloadRecords",
      JSON.stringify(health.droppedPayloadRecords + delta.droppedPayloadRecords),
    );
    upsertMeta.run("droppedBlobs", JSON.stringify(health.droppedBlobs));
    upsertMeta.run(
      "droppedUsageEntries",
      JSON.stringify(health.droppedUsageEntries + delta.droppedUsageEntries),
    );
    upsertMeta.run("writeFailures", JSON.stringify(health.writeFailures));
    upsertMeta.run("schemaVersion", JSON.stringify(MODEL_OBSERVABILITY_SCHEMA_VERSION));
    upsertMeta.run("lastSuccessfulFlushAt", JSON.stringify(successfulFlushAt));
    upsertMeta.run("lastMaintenanceAt", JSON.stringify(health.lastMaintenanceAt));
  }

  function applyTransactionHealthDelta(delta: TransactionHealthDelta): void {
    health.droppedPayloadRecords += delta.droppedPayloadRecords;
    health.droppedUsageEntries += delta.droppedUsageEntries;
  }

  function acknowledgePersistedHealthMeta(): void {
    pendingHealthMetaDirty = false;
    if (!closed && health.status === "degraded") health.status = "active";
    if (health.storeDisabledReasonCode === "write_failed_pending_receipt") {
      health.storeDisabledReasonCode = null;
    }
  }

  function markFailureReceiptPending(): void {
    pendingHealthMetaDirty = true;
    health.status = "degraded";
    health.storeDisabledReasonCode = "write_failed_pending_receipt";
  }

  function persistPendingHealthMeta(): boolean {
    if (!pendingHealthMetaDirty || closed) return !pendingHealthMetaDirty;
    try {
      db.transaction(() => persistHealthMeta())();
      acknowledgePersistedHealthMeta();
      return true;
    } catch {
      return false;
    }
  }

  function refreshQueueHealth(): void {
    health.queuedTraceEvents = traceQueue.length;
    health.queuedPayloadRecords = payloadQueue.length;
    health.queuedBlobs = blobQueue.length;
    health.queuedUsageEntries = usageQueue.length;
    health.pendingBlobBytes = blobQueue.reduce((sum, item) => sum + item.bytes.byteLength, 0);
  }

  /* ── Handler：只 enqueue（§三十五）────────────────────────────────── */

  function handleTraceEvent(event: ModelCallEvent): void {
    if (closed) return;
    try {
      if (traceQueue.length >= normalized.limits.maxQueuedTraceEvents) {
        // Trace metadata 是最高优先级通道（§四十一）：trace 队列自身溢出仍显式计数。
        health.droppedTraceEvents += 1;
        return;
      }
      traceQueue.push(event);
      // §三十八（v2 运行时证据）：persistTraceMetadata=true 且 persistPayloads=false
      // 时，call 结束即可明确标 not_captured（仅当前 NULL，不覆盖既有事实）。
      if (
        event.eventType === "logical_call_end"
        && normalized.persistTraceMetadata
        && !normalized.persistPayloads
        && typeof event.callId === "string" && event.callId
      ) {
        notCapturedCallIds.push(event.callId);
      }
      refreshQueueHealth();
      scheduleFlush();
    } catch { /* enqueue 失败绝不影响模型调用 */ }
  }

  function handlePayloadRecord(record: ModelCallPayloadRecord): void {
    if (closed) return;
    try {
      if (payloadQueue.length >= normalized.limits.maxQueuedPayloadRecords) {
        health.droppedPayloadRecords += 1;
        noteDroppedPayloadCall(record);
        return;
      }
      payloadQueue.push(record);
      refreshQueueHealth();
      scheduleFlush();
    } catch { /* never break */ }
  }

  function noteDroppedPayloadCall(record: ModelCallPayloadRecord): void {
    if (droppedPayloadCallIds.length < MAX_DROPPED_CALL_IDS && typeof record?.callId === "string") {
      droppedPayloadCallIds.push(record.callId);
    }
  }

  /**
   * Phase 8（§十四）：llm_usage consumer——Usage Ledger append 事件的 live
   * accounting ingestion。无 metadata.modelCallId 的 entry 由 projection
   * 静默跳过（§十三不猜）；queue 溢出显式计数。
   */
  function handleUsageEntry(entry: unknown): void {
    if (closed) return;
    try {
      if (usageQueue.length >= MAX_QUEUED_USAGE_ENTRIES) {
        health.droppedUsageEntries += 1;
        return;
      }
      usageQueue.push(entry as UsageLedgerEntryLike);
      refreshQueueHealth();
      scheduleFlush();
    } catch { /* accounting ingestion 失败绝不影响模型调用 */ }
  }

  /** privileged externalizer（§六十一）：只在 persistBlobs 时创建；size/queue cap 内复制字节。 */
  const externalizer = normalized.persistBlobs
    ? {
      stageBinary(input: { bytes: Uint8Array; mediaType: string | null }): { blobId: string } | null {
        try {
          if (closed || !input || typeof input.bytes?.byteLength !== "number") return null;
          if (!blobStore.isEligibleSize(input.bytes.byteLength)) {
            health.droppedBlobs += 1; // size cap 拒绝也是显式缺失（§四十/七十三）
            return null;
          }
          if (blobQueue.length >= normalized.limits.maxQueuedBlobs) {
            health.droppedBlobs += 1;
            return null;
          }
          const pending = health.pendingBlobBytes + input.bytes.byteLength;
          if (pending > normalized.limits.maxPendingBlobBytes) {
            health.droppedBlobs += 1;
            return null;
          }
          const copy = Uint8Array.from(input.bytes);
          const blobId = mintModelObservabilityBlobId(randomBlobToken ?? undefined);
          blobQueue.push({ blobId, bytes: copy, mediaType: input.mediaType ?? null });
          refreshQueueHealth();
          scheduleFlush();
          return { blobId };
        } catch {
          return null;
        }
      },
    }
    : null;

  /* ── Flush（§三十七 batch transaction）────────────────────────────── */

  function scheduleFlush(): void {
    if (closed || flushScheduled || flushing) return;
    flushScheduled = true;
    setImmediate(() => {
      flushScheduled = false;
      flushOnce();
    });
  }

  type FlushBatch = {
    traceEvents: ModelCallEvent[];
    payloadRecords: ModelCallPayloadRecord[];
    usageEntries: UsageLedgerEntryLike[];
    blobs: StagedBlob[];
    droppedCalls: string[];
    notCapturedCalls: string[];
  };

  function takeBatch(): FlushBatch {
    return {
      traceEvents: traceQueue.splice(0, traceQueue.length),
      payloadRecords: payloadQueue.splice(0, payloadQueue.length),
      usageEntries: usageQueue.splice(0, usageQueue.length),
      blobs: blobQueue.splice(0, blobQueue.length),
      droppedCalls: droppedPayloadCallIds.splice(0, droppedPayloadCallIds.length),
      notCapturedCalls: notCapturedCallIds.splice(0, notCapturedCallIds.length),
    };
  }

  /**
   * Filesystem phase：每批只执行一次。后续 SQLite rollback/retry 复用这个不可变
   * 结果，绝不再拿已经释放或改变过的 bytes 重写文件。
   */
  function prepareBlobFiles(batch: FlushBatch): PreparedBlob[] {
    const prepared: PreparedBlob[] = [];
    for (const staged of batch.blobs) {
      const byteLength = staged.bytes.byteLength;
      const durable = blobStore.writeBlobFile(staged.blobId, staged.bytes);
      if (!durable) health.droppedBlobs += 1;
      prepared.push({
        blobId: staged.blobId,
        byteLength,
        mediaType: staged.mediaType,
        durable,
      });
    }
    return prepared;
  }

  function releaseBatchBlobBytes(batch: FlushBatch): void {
    for (const staged of batch.blobs) staged.bytes = new Uint8Array(0);
  }

  /** SQLite phase：可以安全重试；这里不做任何文件写入，也不直接改 JS health。 */
  function commitDatabaseBatch(
    batch: FlushBatch,
    preparedBlobs: readonly PreparedBlob[],
  ): { delta: TransactionHealthDelta; successfulFlushAt: string } {
    const failedBlobIds = new Set(
      preparedBlobs.filter((blob) => !blob.durable).map((blob) => blob.blobId),
    );
    const durableBlobIds = new Set(
      preparedBlobs.filter((blob) => blob.durable).map((blob) => blob.blobId),
    );
    const delta: TransactionHealthDelta = {
      droppedPayloadRecords: 0,
      droppedUsageEntries: 0,
    };
    const serializationDroppedCalls: string[] = [];
    const successfulFlushAt = now();
    const commit = db.transaction(() => {
      for (const prepared of preparedBlobs) {
        if (prepared.durable) {
          blobStore.insertBlobRow(prepared.blobId, prepared.byteLength, prepared.mediaType);
        }
      }
      for (const event of batch.traceEvents) {
        traceStore.applyEvent(event);
      }
      // Phase 8：accounting projection（幂等 upsert；无 modelCallId 静默跳过）。
      for (const entry of batch.usageEntries) {
        try {
          accountingProjection.upsertLedgerEntry(entry, { now });
        } catch {
          delta.droppedUsageEntries += 1;
        }
      }
      if (batch.notCapturedCalls.length > 0) {
        traceStore.markPayloadAvailability(batch.notCapturedCalls, "not_captured");
      }
      for (const record of batch.payloadRecords) {
        const inserted = payloadStore.insertRecord(record, {
          failedBlobIds,
          isBlobDurable: (blobId) => durableBlobIds.has(blobId) || blobStore.getBlobMetadata(blobId) != null,
          now,
        });
        if (inserted === null) {
          delta.droppedPayloadRecords += 1;
          if (typeof record?.callId === "string" && record.callId) {
            serializationDroppedCalls.push(record.callId);
          }
        }
      }
      const allDroppedCalls = [...batch.droppedCalls, ...serializationDroppedCalls];
      if (allDroppedCalls.length > 0) {
        traceStore.markPayloadAvailability(allDroppedCalls, "dropped");
      }
      persistHealthMeta({ delta, successfulFlushAt });
    });
    commit();
    return { delta, successfulFlushAt };
  }

  function flushOnce(): void {
    if (closed || flushing) return;
    if (traceQueue.length === 0 && payloadQueue.length === 0 && blobQueue.length === 0
      && usageQueue.length === 0
      && droppedPayloadCallIds.length === 0 && notCapturedCallIds.length === 0) {
      // 最终失败后队列已经为空，仍需主动补写 health 回执。
      persistPendingHealthMeta();
      return;
    }
    flushing = true;
    try {
      const batch = takeBatch();
      const preparedBlobs = prepareBlobFiles(batch);
      try {
        const committed = commitDatabaseBatch(batch, preparedBlobs);
        applyTransactionHealthDelta(committed.delta);
        health.lastSuccessfulFlushAt = committed.successfulFlushAt;
        acknowledgePersistedHealthMeta();
      } catch (firstError) {
        // rollback 后才允许 retry（§四九）：同一批整体重试一次；再失败 → 诚实 drop。
        try {
          const committed = commitDatabaseBatch(batch, preparedBlobs);
          applyTransactionHealthDelta(committed.delta);
          health.lastSuccessfulFlushAt = committed.successfulFlushAt;
          acknowledgePersistedHealthMeta();
        } catch (secondError) {
          health.droppedTraceEvents += batch.traceEvents.length;
          health.droppedPayloadRecords += batch.payloadRecords.length;
          health.droppedUsageEntries += batch.usageEntries.length;
          // Filesystem phase 已失败的 blob 在 prepare 时计过；这里只计文件已
          // durable、但 metadata/ref 因最终 DB 失败而丢失的 blob。
          health.droppedBlobs += preparedBlobs.filter((blob) => blob.durable).length;
          health.writeFailures += 1;
          markFailureReceiptPending();
        }
      }
      releaseBatchBlobBytes(batch);
      health.lastFlushAt = now();
    } finally {
      flushing = false;
      refreshQueueHealth();
    }
  }

  /* ── Maintenance（§八十六/八十七：startup once + 低频 timer + 显式）── */

  function runMaintenanceInternal(): ModelObservabilityMaintenanceStats | null {
    if (closed) return null;
    try {
      const stats = runWithoutModelTrace(() => runModelObservabilityMaintenance(
        {
          db,
          blobStore,
          markPayloadAvailability: (callIds, availability) =>
            traceStore.markPayloadAvailability(callIds, availability),
          now,
        },
        { policy: normalized.retention },
      ));
      health.lastMaintenanceAt = now();
      try {
        db.transaction(() => {
          upsertMeta.run("lastMaintenanceAt", JSON.stringify(health.lastMaintenanceAt));
          persistHealthMeta();
        })();
        acknowledgePersistedHealthMeta();
      } catch { /* meta 持久化 best-effort */ }
      return stats;
    } catch {
      health.maintenanceErrors += 1;
      return null;
    }
  }

  /* ── Registry 安装（§八十四 composite：persistent + 既有 sink 并行）── */

  const priorObserver = getModelCallObserver();
  const priorSink = getModelCallPayloadSink();
  const priorExternalizer = getModelCallBlobExternalizer();

  const observerImpl: ModelCallObserver | null = normalized.persistTraceMetadata
    ? {
      handleModelCallEvent(event) {
        handleTraceEvent(event);
        if (priorObserver && priorObserver !== observerImpl) {
          try { priorObserver.handleModelCallEvent(event); } catch { /* 链上故障隔离 */ }
        }
      },
    }
    : null;

  const sinkImpl: ModelCallPayloadSink | null = normalized.persistPayloads
    ? {
      handleModelCallPayloadRecord(record) {
        handlePayloadRecord(record);
        if (priorSink && priorSink !== sinkImpl) {
          try { priorSink.handleModelCallPayloadRecord(record); } catch { /* 链上故障隔离 */ }
        }
      },
    }
    : null;

  if (observerImpl) setModelCallObserver(observerImpl);
  if (sinkImpl) setModelCallPayloadSink(sinkImpl);
  if (externalizer) setModelCallBlobExternalizer(externalizer);

  /* ── Timers ── */
  flushTimer = setInterval(() => {
    try {
      flushOnce();
    } catch { /* timer 回调绝不抛出 */ }
  }, normalized.flushIntervalMs);
  flushTimer.unref?.();

  const startupMaintenance = setImmediate(() => {
    runMaintenanceInternal();
    maintenanceTimer = setInterval(() => {
      runMaintenanceInternal();
    }, normalized.maintenanceIntervalMs);
    maintenanceTimer.unref?.();
  });
  startupMaintenance.unref?.();

  let uninstalled = false;
  function uninstallRegistries(): void {
    if (uninstalled) return;
    uninstalled = true;
    if (observerImpl && getModelCallObserver() === observerImpl) setModelCallObserver(priorObserver);
    if (sinkImpl && getModelCallPayloadSink() === sinkImpl) setModelCallPayloadSink(priorSink);
    if (externalizer && getModelCallBlobExternalizer() === externalizer) {
      setModelCallBlobExternalizer(priorExternalizer);
    }
  }

  /* ── Phase 8：Usage Ledger → projection wiring（§十四/十五）────────── */

  let usageUnsubscribe: (() => void) | null = null;

  function initializeAccounting(options: {
    listLedgerEntries: () => unknown[];
    subscribeUsage: (consumer: (entry: unknown) => void) => () => void;
  }): { backfilled: number; skipped: number; backfillSource: "bounded_usage_ledger" } | null {
    if (closed) return null;
    // 幂等：重复调用先退订旧 consumer（engine reconfigure 后重新 wire）。
    if (usageUnsubscribe) {
      try { usageUnsubscribe(); } catch { /* best-effort */ }
      usageUnsubscribe = null;
    }
    let report: { backfilled: number; skipped: number; backfillSource: "bounded_usage_ledger" } | null = null;
    try {
      // §十五：bounded ledger best-effort backfill——只做一次（meta key 标记），
      // 不是完整历史 backfill；报告标注 backfill source。
      const alreadyBackfilled = db
        .prepare(`SELECT 1 FROM observability_meta WHERE key = ?`)
        .get(MODEL_OBSERVABILITY_USAGE_BACKFILL_META_KEY);
      if (!alreadyBackfilled) {
        const entries = options.listLedgerEntries();
        const result = db.transaction(() =>
          backfillModelCallUsageFromLedgerEntries(accountingProjection, entries, db, { now }),
        )();
        report = { backfilled: result.projected, skipped: result.skipped, backfillSource: "bounded_usage_ledger" };
      }
    } catch {
      // backfill 失败：live ingestion 照常；下次启动可重试（meta 未写）。
    }
    try {
      usageUnsubscribe = options.subscribeUsage((entry) => {
        if (entry && typeof entry === "object" && (entry as Record<string, unknown>).type === "llm_usage") {
          handleUsageEntry((entry as Record<string, unknown>).entry);
        }
      });
    } catch {
      usageUnsubscribe = null;
    }
    return report ?? { backfilled: 0, skipped: 0, backfillSource: "bounded_usage_ledger" };
  }

  const handle: ModelObservabilityPersistenceHandle = {
    policy: normalized,
    observer: observerImpl,
    sink: sinkImpl,
    getHealth(): ModelObservabilityHealth {
      refreshQueueHealth();
      return { ...health };
    },
    flushSync() {
      flushOnce();
    },
    runMaintenance() {
      return runMaintenanceInternal();
    },
    initializeAccounting,
    async close() {
      if (closed) return;
      try {
        if (flushTimer) clearInterval(flushTimer);
        if (maintenanceTimer) clearInterval(maintenanceTimer);
        // final flush 必须在置 closed 之前执行（flushOnce 对 closed 直接返回）。
        try { flushOnce(); } catch { /* close 期 flush 失败：队列内容丢失已计数 */ }
      } finally {
        closed = true;
        uninstallRegistries();
        if (usageUnsubscribe) {
          try { usageUnsubscribe(); } catch { /* best-effort */ }
          usageUnsubscribe = null;
        }
        try { db.close(); } catch { /* close 失败不阻塞退出 */ }
        health.status = "closed";
      }
    },
    uninstall() {
      uninstallRegistries();
    },
  };
  return handle;
}
