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
  status: "active" | "disabled" | "closed";
  storeDisabledReasonCode: string | null;
  persistTraceMetadata: boolean;
  persistPayloads: boolean;
  persistBlobs: boolean;
  queuedTraceEvents: number;
  queuedPayloadRecords: number;
  queuedBlobs: number;
  pendingBlobBytes: number;
  droppedTraceEvents: number;
  droppedPayloadRecords: number;
  droppedBlobs: number;
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
  /** flush（bounded）+ 停 timer + close DB + uninstall（恢复先前注册对象）。幂等。 */
  close(): Promise<void>;
  /** 只恢复先前 observer/sink/externalizer，不关 DB（close 会一并做）。 */
  uninstall(): void;
};

/* ── 内部队列 item ───────────────────────────────────────────────────── */

type StagedBlob = { blobId: string; bytes: Uint8Array; mediaType: string | null };

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
        pendingBlobBytes: 0,
        droppedTraceEvents: 0,
        droppedPayloadRecords: 0,
        droppedBlobs: 0,
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
}: {
  lingxiHome: string;
  policy?: ModelObservabilityPersistencePolicy | null;
  now?: () => string;
  Database?: any;
  randomBlobToken?: (() => string) | null;
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

  // Startup Reconciliation（§四十六）：崩溃遗留 call 只标记 interrupted，不伪造终态。
  let interruptedCalls = 0;
  try {
    interruptedCalls = traceStore.reconcileAfterRestart();
  } catch { /* reconciliation 失败不阻止 store 可用 */ }

  /* ── Coordinator 状态 ── */
  const traceQueue: ModelCallEvent[] = [];
  const payloadQueue: ModelCallPayloadRecord[] = [];
  const blobQueue: StagedBlob[] = [];
  /** queue overflow drop 的 payload callId（flush 时落 payload_availability='dropped'）。 */
  const droppedPayloadCallIds: string[] = [];
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
    pendingBlobBytes: 0,
    droppedTraceEvents: 0,
    droppedPayloadRecords: 0,
    droppedBlobs: 0,
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
      health.writeFailures += readCounter("writeFailures");
    } catch { /* 计数恢复 best-effort */ }
  }
  restorePersistedCounters();

  function persistHealthMeta(): void {
    upsertMeta.run("droppedTraceEvents", JSON.stringify(health.droppedTraceEvents));
    upsertMeta.run("droppedPayloadRecords", JSON.stringify(health.droppedPayloadRecords));
    upsertMeta.run("droppedBlobs", JSON.stringify(health.droppedBlobs));
    upsertMeta.run("writeFailures", JSON.stringify(health.writeFailures));
    upsertMeta.run("schemaVersion", JSON.stringify(MODEL_OBSERVABILITY_SCHEMA_VERSION));
    upsertMeta.run("lastSuccessfulFlushAt", JSON.stringify(health.lastSuccessfulFlushAt));
    upsertMeta.run("lastMaintenanceAt", JSON.stringify(health.lastMaintenanceAt));
  }

  function refreshQueueHealth(): void {
    health.queuedTraceEvents = traceQueue.length;
    health.queuedPayloadRecords = payloadQueue.length;
    health.queuedBlobs = blobQueue.length;
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
    blobs: Array<StagedBlob & { byteLength?: number }>;
    droppedCalls: string[];
  };

  function takeBatch(): FlushBatch {
    return {
      traceEvents: traceQueue.splice(0, traceQueue.length),
      payloadRecords: payloadQueue.splice(0, payloadQueue.length),
      blobs: blobQueue.splice(0, blobQueue.length),
      droppedCalls: droppedPayloadCallIds.splice(0, droppedPayloadCallIds.length),
    };
  }

  function commitBatch(batch: FlushBatch): void {
    const failedBlobIds = new Set<string>();
    // ① blob 文件先写（§七十二：blob durable 先于 committed payload ref）。
    for (const staged of batch.blobs) {
      const byteLength = staged.bytes.byteLength;
      const ok = blobStore.writeBlobFile(staged.blobId, staged.bytes);
      if (!ok) {
        failedBlobIds.add(staged.blobId);
        health.droppedBlobs += 1;
      }
      staged.bytes = new Uint8Array(0); // 释放 staged 字节引用（byteLength 已记下）。
      staged.byteLength = byteLength;
    }
    // ② 单 transaction：blob metadata + trace 投影 + payload + refs + health meta。
    const commit = db.transaction(() => {
      for (const staged of batch.blobs) {
        if (!failedBlobIds.has(staged.blobId)) {
          blobStore.insertBlobRow(staged.blobId, staged.byteLength, staged.mediaType);
        }
      }
      for (const event of batch.traceEvents) {
        traceStore.applyEvent(event);
      }
      for (const record of batch.payloadRecords) {
        const inserted = payloadStore.insertRecord(record, {
          failedBlobIds,
          isBlobDurable: (blobId) => blobStore.getBlobMetadata(blobId) != null,
          now,
        });
        if (inserted === null) health.droppedPayloadRecords += 1;
      }
      if (batch.droppedCalls.length > 0) {
        traceStore.markPayloadAvailability(batch.droppedCalls, "dropped");
      }
      persistHealthMeta();
    });
    commit();
  }

  function flushOnce(): void {
    if (closed || flushing) return;
    if (traceQueue.length === 0 && payloadQueue.length === 0 && blobQueue.length === 0
      && droppedPayloadCallIds.length === 0) {
      return;
    }
    flushing = true;
    try {
      const batch = takeBatch();
      try {
        commitBatch(batch);
        health.lastSuccessfulFlushAt = now();
      } catch (firstError) {
        // rollback 后才允许 retry（§四九）：同一批整体重试一次；再失败 → 诚实 drop。
        try {
          commitBatch(batch);
          health.lastSuccessfulFlushAt = now();
        } catch (secondError) {
          health.droppedTraceEvents += batch.traceEvents.length;
          health.droppedPayloadRecords += batch.payloadRecords.length;
          health.droppedBlobs += batch.blobs.length;
          health.writeFailures += 1;
        }
      }
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
  const priorExternalizer = null; // registry 在 capture 模块内默认 null。

  const observerImpl: ModelCallObserver = normalized.persistTraceMetadata
    ? {
      handleModelCallEvent(event) {
        handleTraceEvent(event);
        if (priorObserver && priorObserver !== observerImpl) {
          try { priorObserver.handleModelCallEvent(event); } catch { /* 链上故障隔离 */ }
        }
      },
    }
    : null;

  const sinkImpl: ModelCallPayloadSink = normalized.persistPayloads
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
    if (externalizer) setModelCallBlobExternalizer(priorExternalizer);
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

