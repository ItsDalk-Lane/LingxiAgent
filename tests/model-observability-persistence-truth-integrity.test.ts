/**
 * Phase 10.1 持久化对抗测试：事务重试、失败回执与 Blob 引用完整性。
 *
 * 这些测试故意在真实 SQLite transaction 回调末尾抛错，确保验证的是
 * “已经处理过整批后 rollback”的最坏路径，而不是在事务开始前短路。
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createModelCallPayloadCaptureSession } from "../lib/llm/model-call-payload-capture.ts";
import { installModelObservabilityPersistence } from "../lib/llm/model-observability-persistence.ts";
import {
  loadBetterSqliteDatabase,
  modelObservabilityDbPath,
  openModelObservabilityDatabase,
} from "../lib/llm/model-observability-schema.ts";
import { createModelObservabilityBlobStore } from "../lib/llm/model-observability-blob-store.ts";

const MODEL = { provider: "openai", modelId: "gpt-test", api: "openai-completions" };
const SOURCE = { subsystem: "test", operation: "truth-integrity", surface: "server", trigger: "test" };

const homes = new Set<string>();
const handles = new Set<ReturnType<typeof installModelObservabilityPersistence>>();

afterEach(async () => {
  vi.restoreAllMocks();
  for (const handle of handles) await handle.close();
  handles.clear();
  for (const home of homes) fs.rmSync(home, { recursive: true, force: true });
  homes.clear();
});

function makeHome(prefix: string): string {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  homes.add(home);
  return home;
}

function lifecycleEvent(callId: string, traceId: string) {
  return {
    eventType: "logical_call_start" as const,
    timestamp: new Date().toISOString(),
    callId,
    attemptId: null,
    traceId,
    parentCallId: null,
    model: MODEL,
    source: SOURCE,
    attribution: { kind: "test" },
  };
}

function payloadRecord(callId: string, traceId: string, payload: unknown) {
  return {
    schemaVersion: 1 as const,
    kind: "semantic_request" as const,
    capturedAt: new Date().toISOString(),
    callId,
    traceId,
    parentCallId: null,
    attemptId: null,
    providerRequestOrdinal: null,
    model: MODEL,
    source: SOURCE,
    attribution: { kind: "test" },
    visibility: "full" as const,
    fidelity: "runtime_exact" as const,
    sanitization: { redacted: false, truncated: false, degraded: false, actions: [] },
    payload,
  };
}

/**
 * 新库建表占第 1 个 transaction；从第 2 个开始是 coordinator flush。
 * failTransactions 中的编号会在真实 transaction 回调末尾抛错并回滚。
 */
function databaseFailingTransactions(failTransactions: ReadonlySet<number>) {
  const RealDatabase = loadBetterSqliteDatabase();
  let transactionNumber = 0;
  return class TransactionFaultDatabase {
    private readonly real: any;

    constructor(...args: any[]) {
      this.real = new RealDatabase(...args);
    }

    pragma(...args: any[]) { return this.real.pragma(...args); }
    prepare(...args: any[]) { return this.real.prepare(...args); }
    exec(...args: any[]) { return this.real.exec(...args); }
    close() { return this.real.close(); }

    transaction(fn: () => unknown) {
      transactionNumber += 1;
      if (!failTransactions.has(transactionNumber)) return this.real.transaction(fn);
      return this.real.transaction(() => {
        const value = fn();
        const error = new Error(`simulated SQLITE_BUSY in transaction ${transactionNumber}`) as Error & { code?: string };
        error.code = "SQLITE_BUSY";
        throw error;
        return value;
      });
    }
  };
}

function trackHandle(handle: ReturnType<typeof installModelObservabilityPersistence>) {
  handles.add(handle);
  return handle;
}

describe("Model Observatory persistence truth integrity", () => {
  it("Blob 文件只写一次：第一次 DB transaction rollback，重试仍保留原始字节", async () => {
    const home = makeHome("hana-obs-blob-retry-");
    const pattern = Buffer.alloc(1024 * 1024);
    for (let i = 0; i < pattern.length; i += 1) pattern[i] = i % 251;

    const writeFileSync = fs.writeFileSync.bind(fs);
    const writes: string[] = [];
    vi.spyOn(fs, "writeFileSync").mockImplementation(((file: fs.PathOrFileDescriptor, data: any, options?: any) => {
      if (typeof file === "string" && file.includes("mb_retryblob1234")) writes.push(file);
      return writeFileSync(file, data, options);
    }) as typeof fs.writeFileSync);

    const handle = trackHandle(installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true, persistPayloads: true, persistBlobs: true },
      Database: databaseFailingTransactions(new Set([2])) as any,
      randomBlobToken: () => "retryblob1234",
    }));
    handle.observer!.handleModelCallEvent(lifecycleEvent("mc_blob_retry", "mt_blob_retry"));
    const session = createModelCallPayloadCaptureSession({ callId: "mc_blob_retry", traceId: "mt_blob_retry" });
    expect(session).not.toBeNull();
    session!.captureSemanticRequest({ inputShape: "speech_transcribe", parameters: { audio: pattern } });

    handle.flushSync();
    expect(handle.getHealth().writeFailures).toBe(0);
    expect(writes).toHaveLength(1);

    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      const store = createModelObservabilityBlobStore({ lingxiHome: home, db });
      const meta = store.getBlobMetadata("mb_retryblob1234");
      expect(meta).toMatchObject({ byte_length: pattern.byteLength, state: "ready" });
      expect(store.readBlob("mb_retryblob1234")?.equals(pattern)).toBe(true);
      const payload = db.prepare(`SELECT id, payload_json FROM payload_records WHERE call_id = ?`).get("mc_blob_retry");
      const descriptor = JSON.parse(payload.payload_json).parameters.audio;
      expect(descriptor).toMatchObject({ blobId: "mb_retryblob1234", captureStatus: "stored" });
      expect(db.prepare(`SELECT blob_id FROM payload_blob_refs WHERE payload_record_id = ?`).all(payload.id))
        .toEqual([{ blob_id: "mb_retryblob1234" }]);
      expect(db.prepare(`
        SELECT COUNT(*) AS n FROM payload_blob_refs r
        LEFT JOIN blob_objects b ON b.blob_id = r.blob_id
        WHERE b.blob_id IS NULL
      `).get().n).toBe(0);
    } finally {
      db.close();
    }
  });

  it("rollback 不重复累计 drop：序列化丢弃只计一次，并留下逐调用 dropped 事实", () => {
    const home = makeHome("hana-obs-drop-delta-");
    const handle = trackHandle(installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true, persistPayloads: true },
      Database: databaseFailingTransactions(new Set([2])) as any,
    }));
    handle.observer!.handleModelCallEvent(lifecycleEvent("mc_store_drop", "mt_store_drop"));
    handle.sink!.handleModelCallPayloadRecord(payloadRecord(
      "mc_store_drop",
      "mt_store_drop",
      { retained: "small", oversized: "x".repeat(1_100_000) },
    ));

    handle.flushSync();
    expect(handle.getHealth().droppedPayloadRecords).toBe(1);

    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(db.prepare(`SELECT COUNT(*) AS n FROM payload_records WHERE call_id = ?`).get("mc_store_drop").n).toBe(0);
      expect(db.prepare(`SELECT payload_availability FROM model_calls WHERE call_id = ?`).get("mc_store_drop"))
        .toEqual({ payload_availability: "dropped" });
    } finally {
      db.close();
    }
  });

  it("连续两次写事务失败后保持 degraded；无新事件时 close 仍补记失败回执", async () => {
    const home = makeHome("hana-obs-dirty-receipt-");
    const handle = trackHandle(installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true, persistPayloads: true },
      Database: databaseFailingTransactions(new Set([2, 3])) as any,
    }));
    handle.observer!.handleModelCallEvent(lifecycleEvent("mc_final_failure", "mt_final_failure"));
    handle.sink!.handleModelCallPayloadRecord(payloadRecord("mc_final_failure", "mt_final_failure", { value: "lost" }));

    handle.flushSync();
    expect(handle.getHealth()).toMatchObject({
      status: "degraded",
      storeDisabledReasonCode: "write_failed_pending_receipt",
      droppedTraceEvents: 1,
      droppedPayloadRecords: 1,
      writeFailures: 1,
    });

    await handle.close();
    handles.delete(handle);
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      const rows = db.prepare(`SELECT key, value_json FROM observability_meta WHERE key IN (
        'droppedTraceEvents', 'droppedPayloadRecords', 'writeFailures'
      )`).all();
      const counters = Object.fromEntries(rows.map((row: any) => [row.key, JSON.parse(row.value_json)]));
      expect(counters).toEqual({
        droppedPayloadRecords: 1,
        droppedTraceEvents: 1,
        writeFailures: 1,
      });
    } finally {
      db.close();
    }
  });

  it("Blob 文件写失败时 descriptor 降级且绝不产生 dangling ref", () => {
    const home = makeHome("hana-obs-blob-fail-");
    const writeFileSync = fs.writeFileSync.bind(fs);
    vi.spyOn(fs, "writeFileSync").mockImplementation(((file: fs.PathOrFileDescriptor, data: any, options?: any) => {
      if (typeof file === "string" && file.includes("mb_failblob1234")) throw new Error("simulated blob disk failure");
      return writeFileSync(file, data, options);
    }) as typeof fs.writeFileSync);

    const handle = trackHandle(installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true, persistPayloads: true, persistBlobs: true },
      randomBlobToken: () => "failblob1234",
    }));
    handle.observer!.handleModelCallEvent(lifecycleEvent("mc_blob_fail", "mt_blob_fail"));
    const session = createModelCallPayloadCaptureSession({ callId: "mc_blob_fail", traceId: "mt_blob_fail" });
    session!.captureSemanticRequest({ inputShape: "speech_transcribe", parameters: { audio: Buffer.from("FAIL_ME") } });
    handle.flushSync();

    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      const row = db.prepare(`SELECT id, payload_json FROM payload_records WHERE call_id = ?`).get("mc_blob_fail");
      const descriptor = JSON.parse(row.payload_json).parameters.audio;
      expect(descriptor.captureStatus).toBe("store_failed");
      expect(descriptor.blobId).toBeUndefined();
      expect(db.prepare(`SELECT COUNT(*) AS n FROM blob_objects WHERE blob_id = ?`).get("mb_failblob1234").n).toBe(0);
      expect(db.prepare(`SELECT COUNT(*) AS n FROM payload_blob_refs WHERE payload_record_id = ?`).get(row.id).n).toBe(0);
      expect(db.prepare(`
        SELECT COUNT(*) AS n FROM payload_blob_refs r
        LEFT JOIN blob_objects b ON b.blob_id = r.blob_id
        WHERE b.blob_id IS NULL
      `).get().n).toBe(0);
    } finally {
      db.close();
    }
  });
});
